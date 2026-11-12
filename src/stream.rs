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
/// How late the ffmpeg overlay may read a raw frame and still get the picture
/// of its moment: its pipeline (media start, GPU, 3 s of `loudnorm` lookahead,
/// queues) reads a piece's frames up to 10–14 s after their moment at the
/// start of a piece (measured: Quadro P400, 1080p50, two areas), 16–21 s
/// when `-readrate` kept the channel ~8 s behind the schedule; synced in RAM
/// without it (`VIZRS_FFMPEG_SYNC_RAM`) a few seconds. Later than the history
/// reaches, the graphics have to jump ahead. A second of a crawl in the
/// history is about 5 MB.
const RAW_LATE: Duration = Duration::from_secs(12);
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

/// Seconds of a new raw frame stream ready at once: the lead plus how far its
/// air time is behind now (the frames missed come at once). A reader that
/// starts no further ahead than this never waits for the graphics (the
/// overlay's `-readrate_initial_burst`, PLAN-p400).
pub fn graphics_ahead(air_ms: Option<f64>, now_ms: f64, lead_s: f64) -> f64 {
    let lead_s = lead_s.max(0.0);
    let behind = air_ms.filter(|&air| air_is_timeline(air, now_ms, lead_s)).map_or(0.0, |air| (now_ms - air) / 1000.0);
    (lead_s + behind).max(0.0)
}

/// How far back the streams of `lead_seconds` take pictures.
pub fn history(lead_seconds: f64) -> Duration {
    Duration::from_secs_f64(lead_seconds.max(0.0)) + SAMPLE_DELAY + LATE.max(RAW_LATE) + Duration::from_millis(500)
}

/// The moment whose picture a raw output frame takes: its own moment `at`
/// plus `shift`. A reader later than the history (a slow seek) must not take
/// "the oldest picture" when it reads, as ffmpeg reads a few frames at once:
/// that repeats pictures and jumps. The shift grows once to bring the frames
/// into the history, so the pictures still follow one another. It never
/// shrinks: a reader catching up waits for the pictures instead (the stream
/// sends a frame once its picture is drawn), so every output frame moves the
/// graphics on by one frame – shrinking it held one picture for seconds.
fn picture_moment(at: Instant, shift: &mut Duration, oldest: Instant, newest: Instant) -> Instant {
    let shown = at + *shift;
    if shown < oldest {
        *shift += oldest - shown + HEADROOM;
    }
    (at + *shift).min(newest)
}

/// The graphics timeline of one channel's raw streams, from one run of the
/// ffmpeg overlay to the next: where frame 0 of a run takes its picture,
/// relative to the run's air time (seconds, signed). ErsatzTV transcodes an
/// item in 44 s pieces, each starting on the air time where the one before
/// ended – but often seconds off the clock (late after a slow start, or
/// further than the air time is trusted). A run placing itself by the clock
/// instead showed the graphics of up to seconds before or after: a jump at
/// every piece. Carried, the pictures follow one another across the pieces.
#[derive(Default)]
pub struct Carry(Mutex<Option<(f64, Instant)>>);

/// A timeline older than this is not carried: the channel stood (no viewers).
const CARRY_FOR: Duration = Duration::from_secs(10);

impl Carry {
    fn take(&self) -> Option<f64> {
        match *self.0.lock().unwrap_or_else(|e| e.into_inner()) {
            Some((offset, at)) if at.elapsed() < CARRY_FOR => Some(offset),
            _ => None,
        }
    }

    fn keep(&self, offset: f64) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some((offset, Instant::now()));
    }
}

/// Seconds of a new raw stream ready at once when it goes on with the
/// channel's timeline (`carry`): every picture drawn since its frame 0's
/// moment, at most what the history holds. `None`: a new timeline
/// (`graphics_ahead`). The overlay's media starts that far ahead
/// (`-readrate_initial_burst`), so a piece makes up a slow start and the
/// channel keeps to the clock.
pub fn carried_ahead(carry: &Carry, air_ms: f64, now_ms: f64, lead_s: f64) -> Option<f64> {
    let offset = carry.take()?;
    let lead_s = lead_s.max(0.0);
    let start = air_ms / 1000.0 + offset;
    // As the stream: no timeline whose first picture is not drawn yet.
    if start > now_ms / 1000.0 + lead_s + 0.5 {
        return None;
    }
    let first = start - lead_s - SAMPLE_DELAY.as_secs_f64();
    let held = lead_s + (RAW_LATE.saturating_sub(HEADROOM)).as_secs_f64();
    Some((now_ms / 1000.0 - first).clamp(0.0, held))
}

/// `a - b` in seconds, signed.
fn seconds_between(a: Instant, b: Instant) -> f64 {
    a.checked_duration_since(b).map_or_else(|| -b.duration_since(a).as_secs_f64(), |d| d.as_secs_f64())
}

/// `at` moved by `seconds`, signed.
fn moved(at: Instant, seconds: f64) -> Instant {
    let by = Duration::from_secs_f64(seconds.abs());
    if seconds >= 0.0 { at.checked_add(by) } else { at.checked_sub(by) }.unwrap_or(at)
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
    /// channel has 720 lines or more). `delta`: each frame as a record of
    /// the rows changed since the one before (src/delta.rs).
    Yuva { layout: Layout, hd: bool, delta: bool },
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
    stream_watched(source, out, options, control, &mut |_| false)
}

/// What a watched stream does with a new picture before it goes out.
/// `true` = hold the stream: this picture and those after it wait (at most
/// `HOLD_MAX`) for the reader to hang up – a run of the ffmpeg overlay whose
/// zones the picture leaves (PLAN-strefy S3b) stops before a frame cut off.
pub type Watch<'a> = dyn FnMut(&Picture) -> bool + 'a;

/// A held stream goes on after this long anyway (the reader did not hang up).
pub const HOLD_MAX: Duration = Duration::from_secs(3);

/// A stream starting just as a paused capture resumes waits this long at most
/// for its first new picture, rather than send the lead from before the pause.
const READY_WAIT: Duration = Duration::from_millis(1500);

/// `stream_frames` with every new picture shown to `watch` before it goes out;
/// only raw frames can be held.
pub fn stream_watched(source: &dyn Source, out: &mut dyn Write, options: &Options, control: &Control, watch: &mut Watch) -> u64 {
    stream_carried(source, out, options, control, watch, None)
}

/// `stream_watched` whose raw frames go on from the shift of the channel's
/// run before (`carry`, only with an air time: the runs share its timeline).
pub fn stream_carried(
    source: &dyn Source,
    out: &mut dyn Write,
    options: &Options,
    control: &Control,
    watch: &mut Watch,
    carry: Option<&Carry>,
) -> u64 {
    let waiting = Instant::now();
    while !source.ready() && !control.is_closed() && waiting.elapsed() < READY_WAIT {
        std::thread::sleep(Duration::from_millis(5));
    }
    let frame_s = 1.0 / options.fps;
    let lead = (options.lead_seconds * options.fps).round().max(0.0) as u64;
    let lead_s = lead as f64 * frame_s;
    let now = Instant::now();
    let raw = matches!(options.format, Format::Yuva { .. });
    // The air time as given, trusted or not: the channel's timeline (`carry`) is relative to it.
    let given = options.air.and_then(instant_of).filter(|_| raw);
    let carry = carry.filter(|_| given.is_some());
    // Not a timeline whose first picture is not drawn yet (a jump in the schedule): the reader would wait for it.
    let carried =
        carry.and_then(Carry::take).filter(|&offset| given.is_some_and(|given| seconds_between(moved(given, offset), now) <= lead_s + 0.5));
    let air = options.air.filter(|&air| air_is_timeline(air, crate::time::now_ms() as f64, lead_s)).and_then(instant_of);
    let mut base = match (given, carried) {
        // The run before ended where this one starts: on with its timeline.
        (Some(given), Some(offset)) => moved(given, offset),
        _ => air.unwrap_or(now),
    };
    // The timeline is the air time's or the carried one; without them it starts again after a pause.
    let anchored = air.is_some() || carried.is_some();
    if raw {
        crate::info!(
            "graphics stream: frame 0 on air {:+.3} s from now ({})",
            seconds_between(base, now),
            match (carried, air) {
                (Some(_), _) => "the channel's timeline, from the run before",
                (None, Some(_)) => "air time",
                (None, None) => "no air time: from now",
            }
        );
    }
    // Where the timeline lies against the given air time, for the next run.
    let place = |base: Instant, shift: Duration| given.map(|given| seconds_between(base + shift, given));
    let mut sent: u64 = 0;
    let mut sent_seq: Option<u64> = None;
    // Raw frames: the picture shown, held while it lasts so its PNG is decoded
    // (and its yuva420p frame built) once, not on every output frame.
    // The yuva420p frame goes with it: a standing graphic (pictures sharing
    // their pixels) is converted once, not 50 times a second.
    let mut held: Option<(u64, Arc<Picture>, Arc<crate::yuva::Yuva>)> = None;
    // Raw frames: each one from the frame before, converting only what changed.
    let mut builder = match &options.format {
        Format::Yuva { layout, hd, .. } => Some(crate::yuva::Builder::new(layout, *hd)),
        Format::Packets => None,
    };
    // Raw frames: how far after its moment an output frame takes its picture.
    let mut shift = Duration::ZERO;
    let keep = |base: Instant, shift: Duration| {
        if let (Some(carry), Some(offset)) = (carry, place(base, shift)) {
            carry.keep(offset);
        }
    };
    // Delta frames: the frame sent last, and its size.
    let (delta, size) = match &options.format {
        Format::Yuva { layout, delta, .. } => (*delta, (layout.width, layout.height)),
        Format::Packets => (false, (0, 0)),
    };
    let mut last: Option<Arc<crate::yuva::Yuva>> = None;

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
    let kept = Duration::from_secs_f64(lead_s) + SAMPLE_DELAY + if raw { RAW_LATE } else { LATE };

    while !control.is_closed() {
        if control.wait_while_paused() {
            if !anchored {
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

        // Raw frames go out once their picture (their moment plus the shift) is drawn.
        let from = if matches!(options.format, Format::Yuva { .. }) { base + shift } else { base };
        let elapsed = now.checked_duration_since(from).map_or(-from.duration_since(now).as_secs_f64(), |d| d.as_secs_f64());
        let due = ((elapsed / frame_s).floor() + 1.0 + lead as f64).max(0.0) as u64;
        while sent < due {
            let raw = matches!(options.format, Format::Yuva { .. });
            let frame = if off() {
                crate::frames::blank()
            } else if raw {
                let now = Instant::now();
                let (oldest, newest) = (now.checked_sub(kept).unwrap_or(now), now.checked_sub(SAMPLE_DELAY).unwrap_or(now));
                let before = shift;
                let at = picture_moment(moment(base, sent), &mut shift, oldest, newest);
                if shift != before {
                    // The reader is later than the history: the graphics jump ahead (rare: a stalled transcode).
                    crate::warn!(
                        "graphics stream: frame {sent} read {:.1} s late; the graphics jump {:.2} s ahead",
                        (now.saturating_duration_since(moment(base, sent) + before)).as_secs_f64(),
                        (shift - before).as_secs_f64()
                    );
                    keep(base, shift);
                }
                source.frame_at(at)
            } else {
                frame_at(base, sent)
            };
            let written = if let Some(builder) = builder.as_mut() {
                sent += 1;
                let yuva = match &held {
                    Some((seq, _, yuva)) if *seq == frame.seq => Arc::clone(yuva),
                    _ => {
                        let picture = frame.picture();
                        let yuva = match &held {
                            Some((_, same, yuva)) if Arc::ptr_eq(same, &picture) => Arc::clone(yuva),
                            _ => {
                                if watch(&picture) && hold(control) {
                                    keep(base, shift);
                                    return sent - 1;
                                }
                                builder.build(&picture.rects)
                            }
                        };
                        Arc::clone(&held.insert((frame.seq, picture, yuva)).2)
                    }
                };
                if !delta {
                    out.write_all(&yuva)
                } else if last.as_ref().is_some_and(|l| Arc::ptr_eq(l, &yuva)) {
                    out.write_all(&crate::delta::UNCHANGED)
                } else {
                    let written = crate::delta::write_record(out, last.as_deref().map(|l| &l[..]), &yuva, size.0, size.1);
                    last = Some(yuva);
                    written
                }
            } else if sent_seq != Some(frame.seq) {
                sent_seq = Some(frame.seq);
                sent += 1;
                let picture = frame.picture();
                watch(&picture);
                out.write_all(picture.packet())
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
                keep(base, shift);
                return sent;
            }
        }
        keep(base, shift);
        std::thread::sleep(TICK);
    }
    keep(base, shift);
    sent
}

/// Holds a stream until its reader hangs up (true) or for `HOLD_MAX` (false).
fn hold(control: &Control) -> bool {
    let until = Instant::now() + HOLD_MAX;
    while !control.is_closed() && Instant::now() < until {
        std::thread::sleep(TICK);
    }
    control.is_closed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frames::{Empty, Frame, History};
    use crate::packets::Rect;

    const PACKETS: Format = Format::Packets;

    #[test]
    fn graphics_ahead_is_the_lead_plus_the_air_time_missed() {
        let now = 1_000_000.0;
        // 3 s behind the air time: the lead and those 3 s at once.
        assert_eq!(graphics_ahead(Some(now - 3000.0), now, 4.0), 7.0);
        // Ahead of the air time (ErsatzTV early): less than the lead.
        assert_eq!(graphics_ahead(Some(now + 1000.0), now, 4.0), 3.0);
        // No air time, or a wrong clock: the timeline starts now.
        assert_eq!(graphics_ahead(None, now, 4.0), 4.0);
        assert_eq!(graphics_ahead(Some(now - 60_000.0), now, 4.0), 4.0);
        assert_eq!(graphics_ahead(Some(now + 60_000.0), now, 4.0), 4.0);
        assert_eq!(graphics_ahead(None, now, -1.0), 0.0);
    }

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

    /// Right after a paused capture resumes, the lead waits for the first new
    /// picture instead of going out with the pictures from before the pause.
    #[test]
    fn the_lead_waits_for_a_resumed_capture() {
        let history = Arc::new(crate::frames::History::new(Duration::from_secs(2)));
        history.set(vec![Rect { x: 0, y: 0, w: 1, h: 1, pixels: vec![1, 1, 1, 255].into() }], Instant::now());
        history.restart();
        let control = Control::new(false);
        let (closer, drawer) = (Arc::clone(&control), Arc::clone(&history));
        let timer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(80));
            drawer.set(vec![Rect { x: 0, y: 0, w: 1, h: 1, pixels: vec![2, 2, 2, 255].into() }], Instant::now());
            std::thread::sleep(Duration::from_millis(20));
            closer.close();
        });
        let mut out = Vec::new();
        let options = Options {
            fps: 25.0,
            lead_seconds: 0.2,
            format: Format::Yuva { layout: Layout::whole(1, 1), hd: false, delta: false },
            air: None,
        };
        let sent = stream_frames(history.as_ref(), &mut out, &options, &control);
        timer.join().unwrap();
        assert!(sent >= 5, "sent {sent}");
        let frame = out.len() / sent as usize;
        // Every frame of the lead shows the new picture, none the old one.
        let first = out[..frame].to_vec();
        assert!(out.chunks(frame).all(|f| f == first.as_slice()));
        let mut old = Vec::new();
        let stale = crate::frames::History::new(Duration::from_secs(2));
        stale.set(vec![Rect { x: 0, y: 0, w: 1, h: 1, pixels: vec![1, 1, 1, 255].into() }], Instant::now());
        let control = Control::new(false);
        let closer = Arc::clone(&control);
        let timer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            closer.close();
        });
        stream_frames(&stale, &mut old, &options, &control);
        timer.join().unwrap();
        assert_ne!(&old[..frame], first.as_slice(), "the old picture differs from the new one");
    }

    #[test]
    fn raw_stream_writes_the_whole_picture_on_every_output_frame() {
        struct Fixed(Arc<Frame>);
        impl Source for Fixed {
            fn frame_at(&self, _: Instant) -> Arc<Frame> {
                Arc::clone(&self.0)
            }
        }
        let history =
            Fixed(Arc::new(Frame::new(1, vec![Rect { x: 1, y: 0, w: 1, h: 1, pixels: vec![0, 0, 255, 255].into() }], Instant::now())));
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
            &Options {
                fps: 25.0,
                lead_seconds: 0.2,
                format: Format::Yuva { layout: Layout::whole(4, 2), hd: false, delta: false },
                air: None,
            },
            &control,
        );
        timer.join().unwrap();
        assert_eq!(sent, 6);
        assert_eq!(out.len(), 6 * 20);
        assert!(out.chunks(20).all(|frame| frame[12 + 1] == 255));
    }

    /// A picture the watch holds never goes out: the stream waits for its
    /// reader to hang up (a run of the overlay stopped for wider zones).
    #[test]
    fn a_watched_raw_stream_holds_before_a_picture_and_ends_with_its_reader() {
        struct Two(Instant);
        impl Source for Two {
            fn frame_at(&self, at: Instant) -> Arc<Frame> {
                // the first picture, then another one (a graphic coming up)
                let late = at >= self.0 + Duration::from_millis(60);
                let alpha = if late { 255 } else { 0 };
                Arc::new(Frame::new(u64::from(late), vec![Rect { x: 0, y: 0, w: 1, h: 1, pixels: vec![0, 0, 0, alpha].into() }], self.0))
            }
        }
        let control = Control::new(false);
        let closer = Arc::clone(&control);
        let timer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            closer.close();
        });
        let mut out = Vec::new();
        let mut seen = 0;
        let started = Instant::now();
        // the graphic is in the pictures of output frame 2 on (frames 40 ms apart, looking 100 ms back)
        let sent = stream_watched(
            &Two(started.checked_sub(SAMPLE_DELAY).unwrap() + Duration::from_millis(60)),
            &mut out,
            &Options {
                fps: 25.0,
                lead_seconds: 0.0,
                format: Format::Yuva { layout: Layout::whole(2, 2), hd: false, delta: false },
                air: None,
            },
            &control,
            &mut |picture| {
                seen += 1;
                picture.rects[0].pixels[3] == 255
            },
        );
        timer.join().unwrap();
        // frames of the first picture only, none with the graphic (alpha plane at 6), then held to the end
        assert_eq!(seen, 2);
        assert_eq!(sent as usize * 10, out.len());
        assert!((1..=3).contains(&sent), "sent {sent}");
        assert!(out.chunks(10).all(|frame| frame[6] == 0), "a frame of the held picture went out");
        assert!(started.elapsed() >= Duration::from_millis(300));
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
        let format = Format::Yuva { layout: Layout::whole(4, 2), hd: false, delta: false };
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
            Ok(vec![Rect { x: 0, y: 0, w: 1, h: 1, pixels: vec![255; 4].into() }])
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
        let format = Format::Yuva { layout: Layout::whole(4, 2), hd: false, delta: false };
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
                    feeder.set(vec![Rect { x: 0, y: 0, w: 1, h: 1, pixels: vec![value; 4].into() }], at);
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
        // Caught up: the shift stays (shrinking it held one picture frame after
        // frame); never a picture from the future – the stream waits for it.
        let shifted = shift;
        let caught = t0 + Duration::from_secs(5);
        assert_eq!(picture_moment(caught, &mut shift, oldest, newest), newest);
        assert_eq!(picture_moment(newest, &mut shift, oldest, newest), newest);
        assert_eq!(shift, shifted);
    }

    #[test]
    fn a_carried_timeline_has_its_pictures_ready_at_once() {
        let now = 1_000_000_000.0;
        // No timeline before: the lead plus the air time missed (graphics_ahead).
        assert_eq!(carried_ahead(&Carry::default(), now, now, 4.0), None);
        let carry = Carry::default();
        // Frame 0 on air 2 s ago by the channel's timeline: 6.1 s drawn since its moment.
        carry.keep(-2.0);
        assert!((carried_ahead(&carry, now, now, 4.0).unwrap() - 6.1).abs() < 1e-6);
        // Further back than the history: what it holds.
        carry.keep(-30.0);
        assert!((carried_ahead(&carry, now, now, 4.0).unwrap() - (4.0 + 12.0 - 0.3)).abs() < 1e-6);
        // A timeline far ahead (a jump in the schedule): not carried.
        carry.keep(10.0);
        assert_eq!(carried_ahead(&carry, now, now, 4.0), None);
    }

    #[test]
    fn a_carried_shift_lasts_only_while_the_channel_runs() {
        let carry = Carry::default();
        assert_eq!(carry.take(), None);
        carry.keep(-1.5);
        assert_eq!(carry.take(), Some(-1.5));
        *carry.0.lock().unwrap() = Some((-1.5, Instant::now() - CARRY_FOR));
        assert_eq!(carry.take(), None);
    }

    /// Two runs of the overlay one after another (44 s pieces), the channel 15 s
    /// behind its schedule – further than the air time is trusted. The second
    /// run starts 1 s later on the air timeline: its first frame takes the
    /// picture 1 s after the first run's, on the timeline the first run had.
    /// Placed by the clock instead, both took the picture of "now" – a jump.
    #[test]
    fn the_next_run_goes_on_on_the_timeline_of_the_run_before() {
        // A picture per 20 ms over the last 5 s: picture k is 5000 - 20 k ms old.
        let history = Arc::new(History::new(Duration::from_secs(30)));
        let t0 = Instant::now();
        for k in 0..250u8 {
            let at = t0 - Duration::from_millis(5000 - 20 * u64::from(k));
            history.set(vec![Rect { x: 0, y: 0, w: 1, h: 1, pixels: vec![k, 0, 0, 255].into() }], at);
        }
        let first_picture = |carry: &Carry, air: f64| {
            let source = Recording(Arc::clone(&history), Mutex::new(Vec::new()));
            let control = Control::new(false);
            let closer = Arc::clone(&control);
            let timer = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(20));
                closer.close();
            });
            let mut out = Vec::new();
            let format = Format::Yuva { layout: Layout::whole(1, 1), hd: false, delta: false };
            let options = Options { fps: 50.0, lead_seconds: 3.0, format, air: Some(air) };
            stream_carried(&source, &mut out, &options, &control, &mut |_| false, Some(carry));
            timer.join().unwrap();
            source.1.lock().unwrap()[0]
        };
        let air = crate::time::now_ms() as f64 - 15_000.0;
        let carry = Carry::default();
        let first = first_picture(&carry, air);
        assert!(carry.take().is_some_and(|offset| (offset - 15.0).abs() < 0.5), "{:?}", carry.take());
        let next = first_picture(&carry, air + 1000.0);
        assert!((48..=52).contains(&(next - first)), "first {first}, next {next}");
        // Without the run before: by the clock, about where the first run started – 1 s back.
        let alone = first_picture(&Carry::default(), air + 1000.0);
        assert!(next > alone + 30, "next {next}, alone {alone}");
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
                feeder.set(vec![Rect { x: 0, y: 0, w: 1, h: 1, pixels: vec![255; 4].into() }], at);
                drawn += 1;
                if started.elapsed() >= Duration::from_millis(5000) {
                    closer.close();
                }
            }
        });
        let source = Recording(Arc::clone(&history), Mutex::new(Vec::new()));
        let frame = Duration::from_millis(40);
        let mut out = Paced { start: Some(Duration::from_millis(2500)), frame, burst: 3, read: 0, next: Instant::now() };
        let format = Format::Yuva { layout: Layout::whole(4, 2), hd: false, delta: false };
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
            history.set(
                vec![Rect { x: 0, y: 0, w: 1, h: 1, pixels: vec![value; 4].into() }],
                now - Duration::from_millis(u64::from(value) * 100),
            );
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
