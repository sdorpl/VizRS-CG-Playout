// Live ingest in the sidecar (DOKUMENTACJA §1, Live ingest): a live source of
// a channel comes in as MPEG-TS from a listener (`vizrs-ffmpeg vizrs-ingest`,
// src/listener.rs) and goes out to the ffmpeg overlay, which puts it on the
// channel in place of the schedule (src/takeover.rs).
//
//   POST /ingest?channel=1     Authorization: Bearer <ingest_token>; the reply
//                              head comes at once, then the body is MPEG-TS
//                              until the connection closes
//   GET  /ingest?channel=1     the live source: PAT, PMT and the stream from
//                              its last keyframe on; ends with the source
//   GET  /ingest/state?channel=1
//                              {"enabled":…,"live":…,"audio":…,"video":…}
//
// The channel is live from the first video keyframe of the source until its
// connection ends or sends nothing for `IDLE`. One source per channel: a
// second one while the first is live is refused (an active takeover is not
// pre-empted, as in VizRS ffPlayout System).

use std::collections::{HashMap, VecDeque};
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::http::{self, Request};
use crate::json::{self, Value};

pub const PACKET: usize = 188;
/// A source that sends nothing this long has ended.
pub const IDLE: Duration = Duration::from_secs(5);
/// Most bytes kept of the current group of pictures for readers that join.
const MAX_BUFFER: usize = 32 * 1024 * 1024;
/// A reader that cannot take the stream this long is dropped.
const READER_TIMEOUT: Duration = Duration::from_secs(5);

/// Video codec of a PMT stream type.
fn video_codec(stream_type: u8) -> Option<&'static str> {
    match stream_type {
        0x01 | 0x02 => Some("mpeg2"),
        0x10 => Some("mpeg4"),
        0x1b => Some("h264"),
        0x24 => Some("hevc"),
        _ => None,
    }
}

fn is_audio(stream_type: u8) -> bool {
    matches!(stream_type, 0x03 | 0x04 | 0x0f | 0x11 | 0x81 | 0x87)
}

/// Payload of a TS packet (after the adaptation field), if any.
fn payload(packet: &[u8]) -> &[u8] {
    let control = (packet[3] >> 4) & 0x3;
    match control {
        0x1 => &packet[4..],
        0x3 => {
            let start = 5 + packet[4] as usize;
            if start < PACKET { &packet[start..] } else { &[] }
        }
        _ => &[],
    }
}

/// The section of a PSI packet that starts one (pointer field skipped).
fn section(packet: &[u8]) -> Option<&[u8]> {
    if packet[1] & 0x40 == 0 {
        return None;
    }
    let data = payload(packet);
    let pointer = *data.first()? as usize;
    let section = data.get(1 + pointer..)?;
    let length = ((*section.get(1)? as usize & 0x0f) << 8) | *section.get(2)? as usize;
    section.get(..(3 + length).min(section.len()))
}

/// The video PES in `packet` starts with a picture decoders can start from:
/// H.264 IDR or SPS, HEVC IRAP or VPS/SPS (a source without the random access flag).
fn starts_picture(data: &[u8], codec: &str) -> bool {
    // PES header: 00 00 01, stream id, length (2), flags (2), header length.
    if data.len() < 9 || data[..3] != [0, 0, 1] {
        return false;
    }
    let body = data.get(9 + data[8] as usize..).unwrap_or(&[]);
    body.windows(4).any(|w| {
        w[..3] == [0, 0, 1]
            && match codec {
                "h264" => matches!(w[3] & 0x1f, 5 | 7),
                "hevc" => matches!((w[3] >> 1) & 0x3f, 16..=21 | 32 | 33),
                _ => false,
            }
    })
}

/// What a TS scan found out of the source so far.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Streams {
    pub video: Option<&'static str>,
    pub audio: bool,
}

/// Cuts MPEG-TS into runs that start at video keyframes and keeps the
/// latest PAT and PMT, for readers that join in the middle.
#[derive(Default)]
pub struct Scanner {
    partial: Vec<u8>,
    pmt_pid: Option<u16>,
    video_pid: Option<u16>,
    pub streams: Streams,
    pub pat: Vec<u8>,
    pub pmt: Vec<u8>,
}

/// A run of whole packets; `key` = it starts at a video keyframe.
#[derive(Debug, PartialEq)]
pub struct Run {
    pub key: bool,
    pub data: Vec<u8>,
}

impl Scanner {
    /// Whole packets of `data` (with what was left of the last call) as runs.
    pub fn push(&mut self, data: &[u8]) -> Vec<Run> {
        self.partial.extend_from_slice(data);
        let mut runs: Vec<Run> = Vec::new();
        let mut at = 0;
        while at + PACKET <= self.partial.len() {
            if self.partial[at] != 0x47 {
                // Lost sync: the next sync byte.
                at += 1;
                continue;
            }
            let packet: Vec<u8> = self.partial[at..at + PACKET].to_vec();
            at += PACKET;
            let key = self.scan(&packet);
            match runs.last_mut() {
                Some(run) if !key => run.data.extend_from_slice(&packet),
                _ => runs.push(Run { key, data: packet }),
            }
        }
        self.partial.drain(..at);
        runs
    }

    /// Notes PSI of `packet`; true when it starts a video keyframe.
    fn scan(&mut self, packet: &[u8]) -> bool {
        let pid = (((packet[1] & 0x1f) as u16) << 8) | packet[2] as u16;
        if pid == 0 {
            if let Some(pat) = section(packet) {
                // program_number (2), reserved + PID (2) after the 8-byte header.
                let programs = pat.get(8..pat.len().saturating_sub(4)).unwrap_or(&[]);
                for entry in programs.chunks_exact(4) {
                    if u16::from_be_bytes([entry[0], entry[1]]) != 0 {
                        self.pmt_pid = Some(u16::from_be_bytes([entry[2] & 0x1f, entry[3]]));
                        self.pat = packet.to_vec();
                        break;
                    }
                }
            }
            return false;
        }
        if Some(pid) == self.pmt_pid {
            if let Some(pmt) = section(packet).filter(|s| s.first() == Some(&0x02) && s.len() >= 16) {
                let info = ((pmt[10] as usize & 0x0f) << 8) | pmt[11] as usize;
                let mut entries = pmt.get(12 + info..pmt.len().saturating_sub(4)).unwrap_or(&[]);
                let mut streams = Streams::default();
                let mut video_pid = None;
                while entries.len() >= 5 {
                    let stream_type = entries[0];
                    let es_pid = u16::from_be_bytes([entries[1] & 0x1f, entries[2]]);
                    let length = ((entries[3] as usize & 0x0f) << 8) | entries[4] as usize;
                    if let (None, Some(codec)) = (streams.video, video_codec(stream_type)) {
                        streams.video = Some(codec);
                        video_pid = Some(es_pid);
                    }
                    streams.audio |= is_audio(stream_type);
                    entries = entries.get(5 + length..).unwrap_or(&[]);
                }
                self.streams = streams;
                self.video_pid = video_pid;
                self.pmt = packet.to_vec();
            }
            return false;
        }
        if Some(pid) != self.video_pid || packet[1] & 0x40 == 0 {
            return false;
        }
        let random_access = packet[3] & 0x20 != 0 && packet[4] > 0 && packet[5] & 0x40 != 0;
        random_access || self.streams.video.is_some_and(|codec| starts_picture(payload(packet), codec))
    }
}

/// The stream a source keeps for its readers.
struct Buffer {
    /// Runs from the last keyframe on, with their numbers.
    runs: VecDeque<(u64, Arc<Vec<u8>>)>,
    bytes: usize,
    next: u64,
    pat: Arc<Vec<u8>>,
    pmt: Arc<Vec<u8>>,
    streams: Streams,
    /// A keyframe has come: the source is on the channel.
    live: bool,
    ended: bool,
    since: Option<Instant>,
}

/// Runs for a reader and the number of the last one.
type Batch = (Vec<Arc<Vec<u8>>>, Option<u64>);

/// One live source of a channel.
pub struct Feed {
    buffer: Mutex<Buffer>,
    changed: Condvar,
}

impl Feed {
    fn new() -> Feed {
        Feed {
            buffer: Mutex::new(Buffer {
                runs: VecDeque::new(),
                bytes: 0,
                next: 0,
                pat: Arc::default(),
                pmt: Arc::default(),
                streams: Streams::default(),
                live: false,
                ended: false,
                since: None,
            }),
            changed: Condvar::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Buffer> {
        self.buffer.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The runs of a scan go to the readers: a keyframe starts the buffer anew.
    pub fn add(&self, scanner: &Scanner, runs: Vec<Run>) {
        let mut buffer = self.lock();
        buffer.streams = scanner.streams.clone();
        if !scanner.pat.is_empty() && *buffer.pat != scanner.pat {
            buffer.pat = Arc::new(scanner.pat.clone());
        }
        if !scanner.pmt.is_empty() && *buffer.pmt != scanner.pmt {
            buffer.pmt = Arc::new(scanner.pmt.clone());
        }
        for run in runs {
            if run.key && !buffer.pmt.is_empty() {
                buffer.runs.clear();
                buffer.bytes = 0;
                if !buffer.live {
                    buffer.live = true;
                    buffer.since = Some(Instant::now());
                }
            } else if !buffer.live {
                continue;
            }
            buffer.bytes += run.data.len();
            let number = buffer.next;
            buffer.next += 1;
            buffer.runs.push_back((number, Arc::new(run.data)));
            while buffer.bytes > MAX_BUFFER && buffer.runs.len() > 1 {
                let (_, old) = buffer.runs.pop_front().expect("runs");
                buffer.bytes -= old.len();
            }
        }
        drop(buffer);
        self.changed.notify_all();
    }

    fn end(&self) {
        self.lock().ended = true;
        self.changed.notify_all();
    }

    pub fn is_live(&self) -> bool {
        let buffer = self.lock();
        buffer.live && !buffer.ended
    }

    /// The stream for a reader: `first` = PAT, PMT and the buffer; then what
    /// comes after `after`. None when the source has ended.
    fn next(&self, after: Option<u64>, wait: Duration) -> Option<Batch> {
        let mut buffer = self.lock();
        loop {
            if buffer.ended {
                return None;
            }
            let fresh = match (buffer.runs.back(), after) {
                (Some((newest, _)), Some(after)) => *newest > after,
                (Some(_), None) => true,
                (None, _) => false,
            };
            if buffer.live && fresh {
                break;
            }
            let (guard, timeout) = self.changed.wait_timeout(buffer, wait).unwrap_or_else(|e| e.into_inner());
            buffer = guard;
            if timeout.timed_out() {
                return Some((Vec::new(), after));
            }
        }
        let mut out = Vec::new();
        if after.is_none() || buffer.runs.front().is_some_and(|(n, _)| Some(*n) > after.map(|a| a + 1)) {
            // Joining, or fallen behind the buffer: the headers and the last keyframe on.
            out.push(Arc::clone(&buffer.pat));
            out.push(Arc::clone(&buffer.pmt));
            out.extend(buffer.runs.iter().map(|(_, run)| Arc::clone(run)));
        } else {
            out.extend(buffer.runs.iter().filter(|(n, _)| Some(*n) > after).map(|(_, run)| Arc::clone(run)));
        }
        let last = buffer.runs.back().map(|(n, _)| *n);
        Some((out, last))
    }

    fn state(&self) -> Value {
        let buffer = self.lock();
        let live = buffer.live && !buffer.ended;
        json::object([
            ("live", live.into()),
            ("audio", buffer.streams.audio.into()),
            ("video", buffer.streams.video.map_or(Value::Null, Value::from)),
            ("seconds", buffer.since.filter(|_| live).map_or(Value::Null, |t| (t.elapsed().as_secs_f64().floor()).into())),
        ])
    }
}

/// The live sources of the sidecar, by channel.
pub struct Ingest {
    token: String,
    feeds: Mutex<HashMap<String, Arc<Feed>>>,
}

impl Ingest {
    pub fn new(token: &str) -> Ingest {
        Ingest { token: token.to_owned(), feeds: Mutex::new(HashMap::new()) }
    }

    pub fn enabled(&self) -> bool {
        !self.token.is_empty()
    }

    fn feeds(&self) -> MutexGuard<'_, HashMap<String, Arc<Feed>>> {
        self.feeds.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn feed(&self, channel: &str) -> Option<Arc<Feed>> {
        self.feeds().get(channel).cloned()
    }

    /// A new source of `channel`; None while another one is on it.
    fn start(&self, channel: &str) -> Option<Arc<Feed>> {
        let mut feeds = self.feeds();
        if feeds.get(channel).is_some_and(|feed| !feed.lock().ended) {
            return None;
        }
        let feed = Arc::new(Feed::new());
        feeds.insert(channel.to_owned(), Arc::clone(&feed));
        Some(feed)
    }

    fn finish(&self, channel: &str, feed: &Arc<Feed>) {
        feed.end();
        let mut feeds = self.feeds();
        if feeds.get(channel).is_some_and(|f| Arc::ptr_eq(f, feed)) {
            feeds.remove(channel);
        }
    }

    pub fn is_live(&self, channel: &str) -> bool {
        self.feed(channel).is_some_and(|feed| feed.is_live())
    }

    /// `/health`: the channels with a source.
    pub fn health(&self) -> Value {
        let mut feeds: Vec<(String, Arc<Feed>)> = self.feeds().iter().map(|(c, f)| (c.clone(), Arc::clone(f))).collect();
        feeds.sort_by(|a, b| a.0.cmp(&b.0));
        Value::Array(
            feeds
                .into_iter()
                .map(|(channel, feed)| {
                    let mut state = feed.state();
                    if let Value::Object(members) = &mut state {
                        members.insert(0, ("channel".into(), channel.into()));
                    }
                    state
                })
                .collect(),
        )
    }

    pub fn state_request(&self, request: &Request, mut stream: TcpStream) -> io::Result<()> {
        let channel = request.param("channel").unwrap_or("");
        let mut state = match self.feed(channel) {
            Some(feed) => feed.state(),
            None => json::object([("live", false.into()), ("audio", false.into()), ("video", Value::Null), ("seconds", Value::Null)]),
        };
        if let Value::Object(members) = &mut state {
            members.insert(0, ("enabled".into(), self.enabled().into()));
        }
        http::respond(&mut stream, 200, Some("application/json"), state.to_string().as_bytes())
    }

    /// `POST /ingest`: a source of a channel, MPEG-TS until the connection closes.
    pub fn source_request(&self, request: &Request, mut stream: TcpStream) -> io::Result<()> {
        let channel = request.param("channel").unwrap_or("").to_owned();
        let bearer = request.header("authorization").and_then(|v| v.strip_prefix("Bearer ")).unwrap_or("");
        if !self.enabled() {
            return http::respond(&mut stream, 404, Some("text/plain"), b"live ingest off (no ingest_token)");
        }
        if !constant_eq(bearer.trim().as_bytes(), self.token.as_bytes()) {
            crate::warn!("ingest: channel {channel}: bad token");
            return http::respond(&mut stream, 401, None, b"");
        }
        if channel.is_empty() {
            return http::respond(&mut stream, 400, Some("text/plain"), b"channel?");
        }
        let Some(feed) = self.start(&channel) else {
            crate::warn!("ingest: channel {channel} already has a live source; the new one is refused");
            return http::respond(&mut stream, 409, None, b"");
        };
        stream.write_all(b"HTTP/1.0 200 OK\r\nConnection: close\r\n\r\n")?;
        stream.set_read_timeout(Some(IDLE))?;
        crate::info!("ingest: channel {channel}: source connected");
        let result = self.receive(&channel, &feed, &mut stream);
        let seconds = feed.lock().since.map(|t| t.elapsed().as_secs());
        self.finish(&channel, &feed);
        match (&result, seconds) {
            (Err(error), Some(s)) => crate::info!("ingest: channel {channel}: live ended after {s} s ({error})"),
            (Ok(()), Some(s)) => crate::info!("ingest: channel {channel}: live ended after {s} s"),
            (_, None) => crate::info!("ingest: channel {channel}: source gone before a keyframe"),
        }
        Ok(())
    }

    fn receive(&self, channel: &str, feed: &Feed, stream: &mut TcpStream) -> io::Result<()> {
        let mut scanner = Scanner::default();
        let mut buffer = vec![0u8; 64 * 1024];
        let mut announced = false;
        loop {
            let n = stream.read(&mut buffer)?;
            if n == 0 {
                return Ok(());
            }
            let runs = scanner.push(&buffer[..n]);
            feed.add(&scanner, runs);
            if !announced && feed.is_live() {
                announced = true;
                let streams = &scanner.streams;
                let audio = if streams.audio { "with audio" } else { "without audio" };
                crate::info!("ingest: channel {channel}: live ({}, {audio})", streams.video.unwrap_or("?"));
            }
        }
    }

    /// `GET /ingest`: the live source of a channel for the overlay's ffmpeg.
    pub fn read_request(&self, request: &Request, mut stream: TcpStream) -> io::Result<()> {
        let channel = request.param("channel").unwrap_or("");
        let Some(feed) = self.feed(channel).filter(|f| f.is_live()) else {
            return http::respond(&mut stream, 404, None, b"");
        };
        stream.set_write_timeout(Some(READER_TIMEOUT))?;
        let head = "HTTP/1.0 200 OK\r\nContent-Type: video/mp2t\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n";
        stream.write_all(head.as_bytes())?;
        let mut after = None;
        while let Some((runs, last)) = feed.next(after, Duration::from_secs(1)) {
            for run in runs {
                stream.write_all(&run)?;
            }
            after = last;
        }
        Ok(())
    }
}

/// Comparison that takes as long for every wrong token.
fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A TS packet of `pid`: PUSI, an adaptation field with the random access flag when `key`.
    fn packet(pid: u16, start: bool, key: bool, payload: &[u8]) -> Vec<u8> {
        let mut p = vec![0x47, ((pid >> 8) as u8 & 0x1f) | if start { 0x40 } else { 0 }, pid as u8];
        if key {
            p.extend([0x30, 1, 0x40]);
        } else {
            p.push(0x10);
        }
        p.extend_from_slice(payload);
        p.resize(PACKET, 0xff);
        p
    }

    fn pat(pmt: u16) -> Vec<u8> {
        // pointer, table 0, length 13, ts id, version, section, last, program 1 → pmt, crc.
        let section = [0x00, 0x00, 0xb0, 0x0d, 0, 1, 0xc1, 0, 0, 0, 1, 0xe0 | (pmt >> 8) as u8, pmt as u8, 0, 0, 0, 0];
        packet(0, true, false, &section)
    }

    fn pmt(pid: u16, streams: &[(u8, u16)]) -> Vec<u8> {
        let mut body = vec![0x02, 0xb0, 0, 0, 1, 0xc1, 0, 0, 0xe1, 0x00, 0xf0, 0x00];
        for (stream_type, es) in streams {
            body.extend([*stream_type, 0xe0 | (es >> 8) as u8, *es as u8, 0xf0, 0x00]);
        }
        body.extend([0, 0, 0, 0]);
        let length = body.len() - 3;
        body[1] = 0xb0 | (length >> 8) as u8;
        body[2] = length as u8;
        let mut payload = vec![0];
        payload.extend(body);
        packet(pid, true, false, &payload)
    }

    fn psi() -> Vec<u8> {
        [pat(0x1000), pmt(0x1000, &[(0x1b, 0x100), (0x0f, 0x101)])].concat()
    }

    #[test]
    fn finds_streams_and_cuts_at_keyframes() {
        let mut scanner = Scanner::default();
        let data = [
            psi(),
            packet(0x100, true, false, &[]),
            packet(0x100, true, true, &[]),
            packet(0x101, true, false, &[]),
            packet(0x100, true, true, &[]),
        ]
        .concat();
        // Split in the middle of a packet: the rest comes with the next call.
        let mut runs = scanner.push(&data[..500]);
        runs.extend(scanner.push(&data[500..]));
        assert_eq!(scanner.streams, Streams { video: Some("h264"), audio: true });
        // The first call ends inside the third packet: a run also ends with a call.
        assert_eq!(runs.iter().map(|r| (r.key, r.data.len() / PACKET)).collect::<Vec<_>>(), [(false, 2), (false, 1), (true, 2), (true, 1)]);
        assert_eq!(scanner.pat, pat(0x1000));
        assert!(scanner.partial.is_empty());
    }

    #[test]
    fn keyframe_from_the_picture_without_the_flag() {
        let mut scanner = Scanner::default();
        scanner.push(&psi());
        // PES header (9 bytes, no optional fields) and an H.264 IDR slice.
        let idr = [0, 0, 1, 0xe0, 0, 0, 0x80, 0, 0, 0, 0, 0, 1, 0x65];
        let other = [0, 0, 1, 0xe0, 0, 0, 0x80, 0, 0, 0, 0, 0, 1, 0x41];
        let runs = scanner.push(&[packet(0x100, true, false, &other), packet(0x100, true, false, &idr)].concat());
        assert_eq!(runs.iter().map(|r| r.key).collect::<Vec<_>>(), [false, true]);
    }

    #[test]
    fn video_only_source_and_lost_sync() {
        let mut scanner = Scanner::default();
        let data = [vec![1, 2, 3], pat(0x20), pmt(0x20, &[(0x24, 0x30)]), packet(0x30, true, true, &[])].concat();
        let runs = scanner.push(&data);
        assert_eq!(scanner.streams, Streams { video: Some("hevc"), audio: false });
        assert_eq!(runs.last().map(|r| r.key), Some(true));
    }

    #[test]
    fn feed_goes_live_at_a_keyframe_and_readers_join_there() {
        let feed = Feed::new();
        let mut scanner = Scanner::default();
        let runs = scanner.push(&[psi(), packet(0x100, true, false, &[1])].concat());
        feed.add(&scanner, runs);
        assert!(!feed.is_live(), "no keyframe yet");
        let runs = scanner.push(&[packet(0x100, true, true, &[2]), packet(0x101, true, false, &[3])].concat());
        feed.add(&scanner, runs);
        assert!(feed.is_live());

        let (first, last) = feed.next(None, Duration::from_millis(10)).unwrap();
        let joined: Vec<u8> = first.iter().flat_map(|r| r.iter().copied()).collect();
        assert_eq!(joined, [psi(), packet(0x100, true, true, &[2]), packet(0x101, true, false, &[3])].concat());

        // Nothing new: an empty answer after the wait, then the next run.
        assert_eq!(feed.next(last, Duration::from_millis(10)).unwrap().0.len(), 0);
        let runs = scanner.push(&packet(0x100, true, true, &[4]));
        feed.add(&scanner, runs);
        let (next, _) = feed.next(last, Duration::from_millis(10)).unwrap();
        assert_eq!(next.iter().flat_map(|r| r.iter().copied()).collect::<Vec<_>>(), packet(0x100, true, true, &[4]));

        feed.end();
        assert!(feed.next(last, Duration::from_millis(10)).is_none());
        assert!(!feed.is_live());
    }

    #[test]
    fn one_source_per_channel() {
        let ingest = Ingest::new("t");
        let first = ingest.start("1").unwrap();
        assert!(ingest.start("1").is_none(), "an active source is not pre-empted");
        assert!(ingest.start("2").is_some());
        ingest.finish("1", &first);
        assert!(ingest.start("1").is_some());
        assert!(!Ingest::new("").enabled());
        assert!(constant_eq(b"abc", b"abc") && !constant_eq(b"abc", b"abd") && !constant_eq(b"ab", b"abc"));
    }
}
