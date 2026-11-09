// Frame stream of one session: ETV packets for an ErsatzTV script element, or
// raw yuva420p frames for ffmpeg (the ffmpeg overlay). The reader takes one frame per
// output frame; the stream is paced by the clock with a lead, so it follows
// `-readrate 1.0` with its initial burst but never runs far ahead of the live
// VizRS graphics. Every output frame takes the picture of its own moment, the
// lead's burst too: the graphics run `lead_seconds` behind, so the burst shows
// the pictures of the seconds before, moving, not one picture held. The
// timeline follows the air time of the item (its start in the schedule and the
// seek), so the 44 s pieces ErsatzTV transcodes an item in join without a jump.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::area::Layout;
use crate::frames::{Picture, Source};
use crate::packets::repeat_packet;

const TICK: Duration = Duration::from_millis(20);
/// Pictures are dated when Chromium drew them and arrive decoded up to ~100 ms
/// later; output frames look this far back so each finds the picture of its moment.
pub const SAMPLE_DELAY: Duration = Duration::from_millis(100);
/// An ETV reader this far behind the timeline (a stalled transcode) skips
/// ahead; the ffmpeg overlay instead takes the frames it missed at once.
const LATE: Duration = Duration::from_secs(1);
/// Raw frames later than the history reaches take their pictures this far
/// inside it, so the reader's small delays do not reach its edge.
const HEADROOM: Duration = Duration::from_millis(300);
/// The air time may put the first frame this far after the lead (ErsatzTV
/// ahead of the schedule): the reader waits. Further means a wrong clock.
const EARLY: Duration = Duration::from_secs(2);
/// An air time this far in the past is a wrong clock (or stale data) too.
const WRONG: Duration = Duration::from_secs(10);

/// Whether a stream's timeline is the air time (epoch ms) of its frame 0, not
/// its start: at most the lead and `EARLY` ahead of now, at most `WRONG` behind.
pub fn air_is_timeline(air_ms: f64, now_ms: f64, lead_s: f64) -> bool {
    air_ms <= now_ms + (lead_s + EARLY.as_secs_f64()) * 1000.0 && air_ms + WRONG.as_secs_f64() * 1000.0 >= now_ms
}

/// How far back the streams of `lead_seconds` take pictures.
pub fn history(lead_seconds: f64) -> Duration {
    Duration::from_secs_f64(lead_seconds.max(0.0)) + SAMPLE_DELAY + LATE + Duration::from_millis(500)
}

/// The moment whose picture a raw output frame takes: its own moment `at`
/// plus `shift`. A reader later than the history (a slow seek) must not take
/// "the oldest picture" when it reads, as ffmpeg reads a few frames at once:
/// that repeats pictures and jumps. The shift grows once to bring the frames
/// into the history, so the pictures still follow one another, and shrinks
/// when the reader catches up; a picture never comes from the future.
fn picture_moment(at: Instant, shift: &mut Duration, oldest: Instant, newest: Instant) -> Instant {
    let shown = at + *shift;
    if shown < oldest {
        *shift += oldest - shown + HEADROOM;
    } else if shown > newest {
        *shift = shift.saturating_sub(shown - newest);
    }
    at + *shift
}

/// Pause, resume and end of a stream from other threads.
pub struct Control {
    paused: Mutex<bool>,
    wake: Condvar,
    closed: AtomicBool,
}

impl Control {
    pub fn new(paused: bool) -> Arc<Control> {
        Arc::new(Control { paused: Mutex::new(paused), wake: Condvar::new(), closed: AtomicBool::new(false) })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, bool> {
        self.paused.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn pause(&self) {
        *self.lock() = true;
    }

    pub fn resume(&self) {
        *self.lock() = false;
        self.wake.notify_all();
    }

    /// The reader is gone or the session is over: the stream ends.
    pub fn close(&self) {
        self.closed.store(true, Ordering::Relaxed);
        let _guard = self.lock();
        self.wake.notify_all();
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }

    /// Blocks while paused; `true` if it was paused (the timeline starts again).
    fn wait_while_paused(&self) -> bool {
        let mut paused = self.lock();
        let was = *paused;
        while *paused && !self.is_closed() {
            paused = self.wake.wait(paused).unwrap_or_else(|e| e.into_inner());
        }
        was
    }
}

/// What a session reads.
#[derive(Clone, Debug, PartialEq)]
pub enum Format {
    /// ETV packets: a picture once, then one `Repeat` for any run of the same picture.
    Packets,
    /// Raw yuva420p of `layout` (the channel, its graphics area, or its areas
    /// stacked): every output frame carries all of it. `hd` = BT.709 (the
    /// channel has 720 lines or more).
    Yuva { layout: Layout, hd: bool },
}

pub struct Options {
    pub fps: f64,
    pub lead_seconds: f64,
    pub format: Format,
    /// When output frame 0 is on air (epoch ms: item start + seek), if known.
    pub air: Option<f64>,
}

/// Instant of an epoch time in milliseconds.
fn instant_of(epoch_ms: f64) -> Option<Instant> {
    let ahead = Duration::try_from_secs_f64((epoch_ms - crate::time::now_ms() as f64).abs() / 1000.0).ok()?;
    let now = Instant::now();
    if epoch_ms >= crate::time::now_ms() as f64 { now.checked_add(ahead) } else { now.checked_sub(ahead) }
}

/// Writes the stream until the writer fails or `control` closes; returns the
/// output frames written. Output frame `n` goes out at `base + (n - lead)`
/// frames and shows the picture of that moment less `SAMPLE_DELAY`; `base` is
/// the air time of frame 0, or the start of the stream. A resumed stream
/// without air time starts its timeline (and lead) again from the frames sent.
pub fn stream_frames(source: &dyn Source, out: &mut dyn Write, options: &Options, control: &Control) -> u64 {
    let frame_s = 1.0 / options.fps;
    let lead = (options.lead_seconds * options.fps).round().max(0.0) as u64;
    let lead_s = lead as f64 * frame_s;
    let now = Instant::now();
    let air = options.air.filter(|&air| air_is_timeline(air, crate::time::now_ms() as f64, lead_s)).and_then(instant_of);
    let mut base = air.unwrap_or(now);
    let mut sent: u64 = 0;
    let mut sent_seq: Option<u64> = None;
    // Raw frames: the picture shown, held while it lasts so its PNG is decoded
    // (and its yuva420p frame built) once, not on every output frame.
    // The yuva420p frame goes with it: a standing graphic (pictures sharing
    // their pixels) is converted once, not 50 times a second.
    let mut held: Option<(u64, Arc<Picture>, Arc<crate::yuva::Yuva>)> = None;
    // Raw frames: how far after its moment an output frame takes its picture.
    let mut shift = Duration::ZERO;

    // Moment of output frame `n` (signed: the lead's frames are before `base`).
    let moment = |base: Instant, n: u64| {
        let offset = (n as f64 - lead as f64) * frame_s - SAMPLE_DELAY.as_secs_f64();
        let at = if offset >= 0.0 {
            base.checked_add(Duration::from_secs_f64(offset))
        } else {
            base.checked_sub(Duration::from_secs_f64(-offset))
        };
        at.unwrap_or(base)
    };
    let frame_at = |base: Instant, n: u64| source.frame_at(moment(base, n));
    // Graphics off the air (VizRS Live): transparent frames, whatever the source has.
    let off = crate::frames::graphics_off;
    let kept = Duration::from_secs_f64(lead_s) + SAMPLE_DELAY + LATE;

    while !control.is_closed() {
        if control.wait_while_paused() {
            if air.is_none() {
                base = Instant::now() - Duration::from_secs_f64(sent.saturating_sub(lead) as f64 * frame_s);
            }
            continue;
        }

        // Not further behind than the history reaches: a late ETV reader skips
        // ahead. The overlay's ffmpeg keeps the timeline and catches up (a
        // piece whose media starts slowly, a channel behind the schedule) as
        // ErsatzTV does without graphics; its pictures stay in order
        // (`picture_moment`).
        let now = Instant::now();
        let oldest = now.checked_sub(kept).unwrap_or(now);
        if moment(base, sent) < oldest && matches!(options.format, Format::Packets) {
            base += oldest - moment(base, sent);
        }

        let elapsed = now.checked_duration_since(base).map_or(-base.duration_since(now).as_secs_f64(), |d| d.as_secs_f64());
        let due = ((elapsed / frame_s).floor() + 1.0 + lead as f64).max(0.0) as u64;
        while sent < due {
            let raw = matches!(options.format, Format::Yuva { .. });
            let frame = if off() {
                crate::frames::blank()
            } else if raw {
                let now = Instant::now();
                let (oldest, newest) = (now.checked_sub(kept).unwrap_or(now), now.checked_sub(SAMPLE_DELAY).unwrap_or(now));
                source.frame_at(picture_moment(moment(base, sent), &mut shift, oldest, newest))
            } else {
                frame_at(base, sent)
            };
            let written = if let Format::Yuva { layout, hd } = &options.format {
                sent += 1;
                let yuva = match &held {
                    Some((seq, _, yuva)) if *seq == frame.seq => Arc::clone(yuva),
                    _ => {
                        let picture = frame.picture();
                        let yuva = match &held {
                            Some((_, same, yuva)) if Arc::ptr_eq(same, &picture) => Arc::clone(yuva),
                            _ => picture.yuva(layout, *hd),
                        };
                        Arc::clone(&held.insert((frame.seq, picture, yuva)).2)
                    }
                };
                out.write_all(&yuva)
            } else if sent_seq != Some(frame.seq) {
                sent_seq = Some(frame.seq);
                sent += 1;
                out.write_all(frame.picture().packet())
            } else {
                let mut frames = 1;
                while sent + frames < due && frame_at(base, sent + frames).seq == frame.seq {
                    frames += 1;
                }
                sent += frames;
                out.write_all(&repeat_packet(frames))
            };
            if written.and_then(|()| out.flush()).is_err() {
                control.close();
                return sent;
            }
        }
        std::thread::sleep(TICK);
    }
    sent
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::{Empty, Frame, History};
    use crate::packets::Rect;

    const PACKETS: Format = Format::Packets;

    #[test]
    fn lead_frames_at_once_then_paced() {
        let control = Control::new(false);
        let mut out = Vec::new();
        let closer = Arc::clone(&control);
        let timer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(210));
            closer.close();
        });
        let sent = stream_frames(&Empty::new(), &mut out, &Options { fps: 25.0, lead_seconds: 0.2, format: PACKETS, air: None }, &control);
        timer.join().unwrap();
        // 5 lead frames, then one per 40 ms for ~210 ms.
        assert!((10..=13).contains(&sent), "sent {sent}");
        // One Clear, then only Repeats.
        assert_eq!(&out[6..7], &[2]);
        assert!(out[11..].chunks(15).all(|p| p[6] == 1));
    }

    #[test]
    fn a_paused_stream_writes_nothing_until_resumed() {
        let control = Control::new(true);
        let resumer = Arc::clone(&control);
        let timer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            resumer.resume();
            std::thread::sleep(Duration::from_millis(10));
            resumer.close();
        });
        let mut out = Vec::new();
        let sent = stream_frames(&Empty::new(), &mut out, &Options { fps: 25.0, lead_seconds: 0.2, format: PACKETS, air: None }, &control);
        timer.join().unwrap();
        // The lead again from the resume, not the 100 ms held.
        assert_eq!(sent, 6);
    }

    #[test]
    fn raw_stream_writes_the_whole_picture_on_every_output_frame() {
        struct Fixed(Arc<Frame>);
        impl Source for Fixed {
            fn frame_at(&self, _: Instant) -> Arc<Frame> {
                Arc::clone(&self.0)
            }
        }
        let history = Fixed(Arc::new(Frame::new(1, vec![Rect { x: 1, y: 0, w: 1, h: 1, pixels: vec![0, 0, 255, 255] }], Instant::now())));
        let control = Control::new(false);
        let closer = Arc::clone(&control);
        let timer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            closer.close();
        });
        let mut out = Vec::new();
        let sent = stream_frames(
            &history,
            &mut out,
            &Options { fps: 25.0, lead_seconds: 0.2, format: Format::Yuva { layout: Layout::whole(4, 2), hd: false }, air: None },
            &control,
        );
        timer.join().unwrap();
        assert_eq!(sent, 6);
        assert_eq!(out.len(), 6 * 20);
        assert!(out.chunks(20).all(|frame| frame[12 + 1] == 255));
    }

    /// The overlay's ffmpeg behind the timeline gets every frame it missed at
    /// once (it catches up), not just the history's worth like ErsatzTV.
    #[test]
    fn raw_stream_catches_up_a_late_reader() {
        let control = Control::new(false);
        let closer = Arc::clone(&control);
        let timer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(5));
            closer.close();
        });
        let mut out = Vec::new();
        let late = crate::time::now_ms() as f64 - 5000.0;
        let format = Format::Yuva { layout: Layout::whole(4, 2), hd: false };
        let sent = stream_frames(&past_pictures(), &mut out, &Options { fps: 10.0, lead_seconds: 0.8, format, air: Some(late) }, &control);
        timer.join().unwrap();
        // 5 s behind plus the 0.8 s lead, against 19 frames for an ETV reader.
        assert!((58..=59).contains(&sent), "sent {sent}");
        assert_eq!(out.len() as u64, sent * 20);
    }

    #[test]
    fn raw_stream_decodes_a_png_once_while_it_is_shown() {
        use crate::frames::{Decode, Shot};
        use std::sync::atomic::AtomicUsize;
        let decodes = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&decodes);
        let decode: Arc<Decode> = Arc::new(move |_, _: &Shot| {
            counter.fetch_add(1, Ordering::Relaxed);
            Ok(vec![Rect { x: 0, y: 0, w: 1, h: 1, pixels: vec![255; 4] }])
        });
        let history = History::new(Duration::from_secs(5));
        history.set_png(vec![1], Instant::now() - Duration::from_secs(2), &decode);
        let control = Control::new(false);
        let closer = Arc::clone(&control);
        let timer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            closer.close();
        });
        let mut out = Vec::new();
        let format = Format::Yuva { layout: Layout::whole(4, 2), hd: false };
        let sent = stream_frames(&history, &mut out, &Options { fps: 25.0, lead_seconds: 0.2, format, air: None }, &control);
        timer.join().unwrap();
        assert!(sent >= 8, "sent {sent}");
        assert_eq!(decodes.load(Ordering::Relaxed), 1);
    }

    /// Chromium draws every 40 ms; each picture arrives decoded 0–85 ms later.
    /// Every output frame still shows its own picture: steps of exactly one.
    #[test]
    fn pictures_decoded_with_varying_delay_each_land_in_their_own_output_frame() {
        let history = Arc::new(History::new(Duration::from_millis(500)));
        let control = Control::new(false);
        let (feeder, closer) = (Arc::clone(&history), Arc::clone(&control));
        let started = Instant::now();
        let deliver = std::thread::spawn(move || {
            let mut pending: Vec<(Instant, u8, Instant)> = Vec::new();
            let mut drawn = 0u64;
            let mut seed = 0x2545_f491_4f6c_dd1du64;
            while started.elapsed() < Duration::from_millis(1500) {
                let now = Instant::now();
                while started + Duration::from_millis(10 + drawn * 40) <= now {
                    let at = started + Duration::from_millis(10 + drawn * 40);
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    pending.push((at, (drawn % 250) as u8 + 1, at + Duration::from_millis(seed % 85)));
                    drawn += 1;
                }
                while pending.first().is_some_and(|p| p.2 <= now) {
                    let (at, value, _) = pending.remove(0);
                    feeder.set(vec![Rect { x: 0, y: 0, w: 1, h: 1, pixels: vec![value; 4] }], at);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            closer.close();
        });
        let mut out = Vec::new();
        stream_frames(&*history, &mut out, &Options { fps: 25.0, lead_seconds: 0.0, format: PACKETS, air: None }, &control);
        deliver.join().unwrap();

        let values = shown(&out);
        let values: Vec<u8> = values.into_iter().filter(|&v| v > 0).collect();
        let steps: Vec<i32> = values.windows(2).map(|w| i32::from(w[1]) - i32::from(w[0])).collect();
        let exact = steps.iter().filter(|&&s| s == 1).count();
        assert!(exact + 1 >= steps.len(), "steps: {steps:?}");
    }

    #[test]
    fn a_picture_moment_stays_in_the_history_and_in_the_past() {
        let t0 = Instant::now() + Duration::from_secs(100);
        let (oldest, newest) = (t0, t0 + Duration::from_secs(6));
        let mut shift = Duration::ZERO;
        // On time: its own moment.
        assert_eq!(picture_moment(t0 + Duration::from_secs(2), &mut shift, oldest, newest), t0 + Duration::from_secs(2));
        assert_eq!(shift, Duration::ZERO);
        // Later than the history: inside it, with the same shift from then on.
        let late = t0 - Duration::from_secs(3);
        assert_eq!(picture_moment(late, &mut shift, oldest, newest), t0 + HEADROOM);
        let next = late + Duration::from_millis(40);
        assert_eq!(picture_moment(next, &mut shift, oldest, newest), t0 + HEADROOM + Duration::from_millis(40));
        // Caught up: the shift goes, no picture from the future.
        let caught = t0 + Duration::from_secs(5);
        assert_eq!(picture_moment(caught, &mut shift, oldest, newest), newest);
        assert_eq!(picture_moment(newest, &mut shift, oldest, newest), newest);
        assert_eq!(shift, Duration::ZERO);
    }

    /// A source that remembers the pictures it gave.
    struct Recording(Arc<History>, Mutex<Vec<u64>>);
    impl Source for Recording {
        fn frame_at(&self, time: Instant) -> Arc<Frame> {
            let frame = self.0.frame_at(time);
            self.1.lock().unwrap().push(frame.seq);
            frame
        }
    }

    /// A reader like ffmpeg: nothing for `start`, then in real time, a few
    /// frames at once (its input queue).
    struct Paced {
        start: Option<Duration>,
        frame: Duration,
        burst: u32,
        read: u32,
        next: Instant,
    }
    impl Write for Paced {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            if let Some(start) = self.start.take() {
                std::thread::sleep(start);
                self.next = Instant::now();
            }
            self.next += self.frame;
            self.read += 1;
            if self.read.is_multiple_of(self.burst) {
                std::thread::sleep(self.next.saturating_duration_since(Instant::now()));
            }
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// ffmpeg started later than the history reaches and reads in real time
    /// from then on (a slow seek): the pictures still follow one another.
    #[test]
    fn a_reader_later_than_the_history_gets_consecutive_pictures() {
        let lead = 0.2;
        let history = Arc::new(History::new(super::history(lead)));
        let control = Control::new(false);
        let (feeder, closer) = (Arc::clone(&history), Arc::clone(&control));
        let started = Instant::now();
        let deliver = std::thread::spawn(move || {
            let mut drawn = 0u64;
            while started.elapsed() < Duration::from_millis(5500) {
                let at = started + Duration::from_millis(drawn * 40);
                std::thread::sleep(at.saturating_duration_since(Instant::now()));
                feeder.set(vec![Rect { x: 0, y: 0, w: 1, h: 1, pixels: vec![255; 4] }], at);
                drawn += 1;
                if started.elapsed() >= Duration::from_millis(5000) {
                    closer.close();
                }
            }
        });
        let source = Recording(Arc::clone(&history), Mutex::new(Vec::new()));
        let frame = Duration::from_millis(40);
        let mut out = Paced { start: Some(Duration::from_millis(2500)), frame, burst: 3, read: 0, next: Instant::now() };
        let format = Format::Yuva { layout: Layout::whole(4, 2), hd: false };
        stream_frames(&source, &mut out, &Options { fps: 25.0, lead_seconds: lead, format, air: None }, &control);
        deliver.join().unwrap();

        let seqs = source.1.lock().unwrap().clone();
        let seqs = &seqs[seqs.len() / 4..];
        let steps: Vec<u64> = seqs.windows(2).map(|w| w[1] - w[0]).collect();
        let even = steps.iter().filter(|&&s| s == 1).count();
        assert!(even * 100 >= steps.len() * 97, "{even} of {}: {steps:?}", steps.len());
    }

    /// Pixel value of every output frame of an ETV packet stream.
    fn shown(out: &[u8]) -> Vec<u8> {
        let mut values = Vec::new();
        let (mut offset, mut value) = (0, 0u8);
        while offset + 11 <= out.len() {
            let length = u32::from_be_bytes(out[offset + 7..offset + 11].try_into().unwrap()) as usize;
            let payload = &out[offset + 11..offset + 11 + length];
            let repeat = match out[offset + 6] {
                1 => u32::from_be_bytes(payload.try_into().unwrap()) as usize,
                2 => {
                    value = 0;
                    0
                }
                _ => {
                    value = payload[10];
                    0
                }
            };
            values.extend(std::iter::repeat_n(value, repeat + 1));
            offset += 11 + length;
        }
        values
    }

    /// One picture every 100 ms for the last two seconds, valued by its age.
    fn past_pictures() -> History {
        let history = History::new(Duration::from_secs(5));
        let now = Instant::now();
        for value in (1..=20u8).rev() {
            history.set(vec![Rect { x: 0, y: 0, w: 1, h: 1, pixels: vec![value; 4] }], now - Duration::from_millis(u64::from(value) * 100));
        }
        history
    }

    fn burst(source: &dyn Source, air: Option<f64>) -> Vec<u8> {
        let control = Control::new(false);
        let closer = Arc::clone(&control);
        let timer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(5));
            closer.close();
        });
        let mut out = Vec::new();
        stream_frames(source, &mut out, &Options { fps: 10.0, lead_seconds: 0.8, format: PACKETS, air }, &control);
        timer.join().unwrap();
        shown(&out)
    }

    /// The lead's burst shows the pictures of the moments before, not one held.
    #[test]
    fn the_burst_moves_through_the_past_pictures() {
        // Frames 0..=8 at once: moments -0.9 s .. -0.1 s, one picture each.
        assert_eq!(burst(&past_pictures(), None), vec![9, 8, 7, 6, 5, 4, 3, 2, 1]);
    }

    /// A piece that starts on air 0.25 s from now (ErsatzTV ahead) continues
    /// the timeline of the piece before: its burst is shorter and starts later.
    #[test]
    fn the_air_time_places_the_frames() {
        let air = crate::time::now_ms() as f64 + 250.0;
        assert_eq!(burst(&past_pictures(), Some(air)), vec![7, 6, 5, 4, 3, 2]);
        // ErsatzTV a little behind the schedule: the frames it missed, at once.
        let behind = crate::time::now_ms() as f64 - 550.0;
        assert_eq!(burst(&past_pictures(), Some(behind)), (2..=15).rev().collect::<Vec<u8>>());
        // Further behind: no further back than the history the lead needs.
        let late = crate::time::now_ms() as f64 - 5000.0;
        assert_eq!(burst(&past_pictures(), Some(late)), (1..=19).rev().collect::<Vec<u8>>());
        // A clock far off: the stream starts from now instead.
        for wrong in [3_600_000.0, -60_000.0] {
            assert_eq!(burst(&past_pictures(), Some(crate::time::now_ms() as f64 + wrong)).len(), 9);
        }
    }
}
