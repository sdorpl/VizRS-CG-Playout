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
//   GET  /ingest?channel=1&take=44.000
//                              the same for a live run of the overlay that is
//                              to produce `take` seconds: from the keyframe
//                              nearest to where the run before stopped
//   GET  /ingest?channel=1&slot=1
//                              the live slot of a scheduled live programme
//                              (src/live_shows.rs): the source while live, else
//                              a second of black and silence without a length,
//                              so that ErsatzTV's ffprobe of the item works
//   GET  /ingest/state?channel=1
//                              {"enabled":…,"anytime":…,"hold":…,"live":…,"audio":…,"video":…}
//
// Readers of the overlay's live runs (`take=<seconds>`, src/takeover.rs) go on
// from where the run before stopped: the sidecar keeps the last `HISTORY` of
// the source and a cursor in its timestamps (the start keyframe of a run plus
// what it is to produce), and the next run starts at the keyframe nearest to
// it. ffmpeg then reads the seconds it lost starting at once, so the channel
// keeps real time across ErsatzTV's 44 s pieces (each new ffmpeg costs about
// a second, which a run that starts at the live edge never makes up: 0.96x).
// A cursor `MAX_LAG` behind the source (runs fallen behind, a held channel)
// goes back to the live edge, as every other reader starts.
//
// The channel is live from the first video keyframe of the source until its
// connection ends or sends nothing for its idle time (`ingest_idle_seconds`,
// `DEFAULT_IDLE`). One source per channel: a
// second one while the first is live is refused (an active takeover is not
// pre-empted, as in VizRS ffPlayout System).
//
// The live API (src/panel.rs, VizRS Live) decides per channel: a held channel is not live
// for the overlay and its readers, though its source stays connected, and
// `anytime` of a channel overrides `ingest_anytime`.

use std::collections::{HashMap, VecDeque};
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::http::{self, Request};
use crate::json::{self, Value};

pub const PACKET: usize = 188;
/// A source that sends nothing this long has ended (`ingest_idle_seconds`).
/// Above the 4 s a relay that reads HLS can go without a byte (datarhei
/// Restreamer: two 2 s segments at once), so such gaps leave the live on air.
pub const DEFAULT_IDLE: Duration = Duration::from_secs(8);
/// Most bytes kept of the source for readers that join or go on.
const MAX_BUFFER: usize = 32 * 1024 * 1024;
/// How much of the source is kept (by its timestamps), whole groups of pictures.
const HISTORY: f64 = 20.0;
/// A live run's cursor further than this behind the source starts at the live edge.
const MAX_LAG: f64 = 12.0;
/// MPEG timestamps: 90 kHz, 33 bits.
const PTS_HZ: f64 = 90_000.0;
const PTS_WRAP: u64 = 1 << 33;
/// A reader that cannot take the stream this long is dropped.
const READER_TIMEOUT: Duration = Duration::from_secs(5);
/// A second of black 320x180 H.264 and silent AAC in MPEG-TS: the live slot without a source.
pub const STANDBY: &[u8] = include_bytes!("standby.ts");

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

/// PTS of a PES that starts in `data` (the payload of a TS packet), if it has one.
fn pes_pts(data: &[u8]) -> Option<u64> {
    if data.len() < 14 || data[..3] != [0, 0, 1] || data[7] & 0x80 == 0 {
        return None;
    }
    let b = &data[9..14];
    Some(((b[0] as u64 >> 1) & 0x07) << 30 | (b[1] as u64) << 22 | (b[2] as u64 >> 1) << 15 | (b[3] as u64) << 7 | b[4] as u64 >> 1)
}

/// Seconds from timestamp `b` to `a` (negative when `a` is earlier), across the 33-bit wrap.
fn pts_seconds(a: u64, b: u64) -> f64 {
    let d = a.wrapping_sub(b) % PTS_WRAP;
    let signed = if d >= PTS_WRAP / 2 { d as i64 - PTS_WRAP as i64 } else { d as i64 };
    signed as f64 / PTS_HZ
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

/// A run of whole packets; `key` = it starts at a video keyframe, `pts` its timestamp.
#[derive(Debug, PartialEq)]
pub struct Run {
    pub key: bool,
    pub pts: Option<u64>,
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
            match (runs.last_mut(), key) {
                (Some(run), None) => run.data.extend_from_slice(&packet),
                _ => runs.push(Run { key: key.is_some(), pts: key.flatten(), data: packet }),
            }
        }
        self.partial.drain(..at);
        runs
    }

    /// Notes PSI of `packet`; Some (with its PTS, if any) when it starts a video keyframe.
    fn scan(&mut self, packet: &[u8]) -> Option<Option<u64>> {
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
            return None;
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
            return None;
        }
        if Some(pid) != self.video_pid || packet[1] & 0x40 == 0 {
            return None;
        }
        let random_access = packet[3] & 0x20 != 0 && packet[4] > 0 && packet[5] & 0x40 != 0;
        let key = random_access || self.streams.video.is_some_and(|codec| starts_picture(payload(packet), codec));
        key.then(|| pes_pts(payload(packet)))
    }
}

/// A run kept for the readers.
struct Entry {
    number: u64,
    data: Arc<Vec<u8>>,
    key: bool,
    pts: Option<u64>,
}

/// Where a reader starts or goes on.
#[derive(Clone, Copy, Debug, PartialEq)]
enum From {
    /// At the last keyframe, with the headers.
    Edge,
    /// At run `n` (a keyframe), with the headers; the live edge if it is gone.
    Start(u64),
    /// After run `n`; at the live edge, with the headers, when fallen behind the buffer.
    After(u64),
}

/// The stream a source keeps for its readers.
struct Buffer {
    /// Runs of the last `HISTORY`, from a keyframe on, with their numbers.
    runs: VecDeque<Entry>,
    bytes: usize,
    next: u64,
    pat: Arc<Vec<u8>>,
    pmt: Arc<Vec<u8>>,
    streams: Streams,
    /// A keyframe has come: the source is on the channel.
    live: bool,
    ended: bool,
    since: Option<Instant>,
    /// Where the next live run goes on (PTS), set by the run before.
    cursor: Option<u64>,
}

impl Buffer {
    /// Number of the last keyframe run.
    fn edge(&self) -> Option<u64> {
        self.runs.iter().rev().find(|e| e.key).map(|e| e.number)
    }

    /// Whole groups of pictures off the front past `HISTORY` (or `MAX_BUFFER`);
    /// a source without timestamps keeps only its last one.
    fn trim(&mut self) {
        loop {
            let newest = self.runs.iter().rev().find(|e| e.key);
            let over_time = match (self.runs.front().and_then(|e| e.pts), newest.map(|e| e.pts)) {
                (Some(front), Some(Some(newest))) => pts_seconds(newest, front) > HISTORY,
                (_, Some(_)) => newest.map(|e| e.number) != self.runs.front().map(|e| e.number),
                _ => false,
            };
            if !over_time && self.bytes <= MAX_BUFFER {
                return;
            }
            let has_next_key = self.runs.iter().skip(1).any(|e| e.key);
            if !has_next_key {
                // One group of pictures over the bytes: its oldest runs go, as before.
                while self.bytes > MAX_BUFFER && self.runs.len() > 1 {
                    let old = self.runs.pop_front().expect("runs");
                    self.bytes -= old.data.len();
                }
                return;
            }
            let old = self.runs.pop_front().expect("runs");
            self.bytes -= old.data.len();
            while self.runs.front().is_some_and(|e| !e.key) {
                let old = self.runs.pop_front().expect("runs");
                self.bytes -= old.data.len();
            }
        }
    }
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
                cursor: None,
            }),
            changed: Condvar::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Buffer> {
        self.buffer.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The runs of a scan go to the readers, from the first keyframe on.
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
            let key = run.key && !buffer.pmt.is_empty();
            if key && !buffer.live {
                buffer.live = true;
                buffer.since = Some(Instant::now());
            } else if !buffer.live {
                continue;
            }
            buffer.bytes += run.data.len();
            let number = buffer.next;
            buffer.next += 1;
            buffer.runs.push_back(Entry { number, data: Arc::new(run.data), key, pts: run.pts.filter(|_| key) });
            buffer.trim();
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

    /// Where a live run that is to produce `take` seconds starts: at the
    /// keyframe nearest to the cursor the run before left, or at the live edge
    /// (no cursor, a source without timestamps, the cursor `MAX_LAG` behind).
    /// The cursor moves on by `take` from the keyframe taken; also returned:
    /// how far that keyframe is behind the live edge (s), and how far behind the
    /// cursor was when it was dropped for `MAX_LAG` (live runs slower than the
    /// clock: the channel falls behind and its HLS buffer drains).
    fn follow(&self, take: f64) -> (From, f64, Option<f64>) {
        let mut buffer = self.lock();
        let keys: Vec<(u64, u64)> = buffer.runs.iter().filter(|e| e.key).filter_map(|e| e.pts.map(|p| (e.number, p))).collect();
        let Some(&(_, edge)) = keys.last() else {
            buffer.cursor = None;
            return (From::Edge, 0.0, None);
        };
        let dropped = buffer.cursor.map(|cursor| pts_seconds(edge, cursor)).filter(|lag| *lag > MAX_LAG);
        let pick = match buffer.cursor {
            Some(cursor) if pts_seconds(edge, cursor) <= MAX_LAG => keys
                .iter()
                .min_by(|a, b| pts_seconds(a.1, cursor).abs().total_cmp(&pts_seconds(b.1, cursor).abs()))
                .copied()
                .unwrap_or((0, edge)),
            _ => *keys.last().expect("keys"),
        };
        let ticks = (take.max(0.0) * PTS_HZ).round() as u64;
        buffer.cursor = Some((pick.1 + ticks) % PTS_WRAP);
        (From::Start(pick.0), pts_seconds(edge, pick.1), dropped)
    }

    /// The stream for a reader from `from` (the headers first when it starts),
    /// then the number of the last run sent. None when the source has ended.
    fn next(&self, from: From, wait: Duration) -> Option<Batch> {
        let mut buffer = self.lock();
        loop {
            if buffer.ended {
                return None;
            }
            let newest = buffer.runs.back().map(|e| e.number);
            let fresh = match (newest, from) {
                (None, _) => false,
                (Some(_), From::Edge) => true,
                (Some(newest), From::Start(n)) => newest >= n,
                (Some(newest), From::After(a)) => newest > a,
            };
            if buffer.live && fresh {
                break;
            }
            let (guard, timeout) = self.changed.wait_timeout(buffer, wait).unwrap_or_else(|e| e.into_inner());
            buffer = guard;
            if timeout.timed_out() {
                return Some((
                    Vec::new(),
                    match from {
                        From::After(a) => Some(a),
                        _ => None,
                    },
                ));
            }
        }
        let front = buffer.runs.front().map_or(0, |e| e.number);
        let start = match from {
            From::Start(n) if n >= front => Some(n),
            From::After(a) if a + 1 >= front => None,
            _ => buffer.edge(),
        };
        let mut out = Vec::new();
        match start {
            Some(start) => {
                // Joining, going on from a cursor, or fallen behind the buffer: the headers first.
                out.push(Arc::clone(&buffer.pat));
                out.push(Arc::clone(&buffer.pmt));
                out.extend(buffer.runs.iter().filter(|e| e.number >= start).map(|e| Arc::clone(&e.data)));
            }
            None => {
                let after = match from {
                    From::After(a) => a,
                    _ => unreachable!("only After goes on"),
                };
                out.extend(buffer.runs.iter().filter(|e| e.number > after).map(|e| Arc::clone(&e.data)));
            }
        }
        let last = buffer.runs.back().map(|e| e.number);
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

/// Most events kept for the live API.
const MAX_EVENTS: usize = 100;

/// What the operator decided for a channel (live API).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChannelControl {
    /// The studio is kept off the channel, even with a source.
    pub hold: bool,
    /// Overrides `ingest_anytime` for the channel.
    pub anytime: Option<bool>,
}

/// Something that happened to the live sources: epoch ms, channel, text.
pub type Event = (i64, String, String);

/// The live sources of the sidecar, by channel.
pub struct Ingest {
    token: String,
    /// A source takes any piece over, not only live programmes.
    anytime: bool,
    /// A source silent this long has ended.
    idle: Duration,
    /// ErsatzTV's config folder: where the as-run log lives ("" = no log).
    etv_config: String,
    feeds: Mutex<HashMap<String, Arc<Feed>>>,
    control: Mutex<HashMap<String, ChannelControl>>,
    events: Mutex<VecDeque<Event>>,
}

impl Ingest {
    pub fn new(token: &str, anytime: bool) -> Ingest {
        Ingest::with_config(token, anytime, String::new())
    }

    /// With ErsatzTV's config folder the signal of a channel also reaches the as-run log (K14).
    pub fn with_config(token: &str, anytime: bool, etv_config: String) -> Ingest {
        Ingest {
            token: token.to_owned(),
            anytime,
            idle: DEFAULT_IDLE,
            etv_config,
            feeds: Mutex::new(HashMap::new()),
            control: Mutex::new(HashMap::new()),
            events: Mutex::new(VecDeque::new()),
        }
    }

    /// A source that sends nothing for `idle` has ended (at least 1 s).
    pub fn with_idle(mut self, idle: Duration) -> Ingest {
        self.idle = idle.max(Duration::from_secs(1));
        self
    }

    pub fn idle(&self) -> Duration {
        self.idle
    }

    pub fn control(&self, channel: &str) -> ChannelControl {
        self.control.lock().unwrap_or_else(|e| e.into_inner()).get(channel).cloned().unwrap_or_default()
    }

    pub fn set_control(&self, channel: &str, control: ChannelControl) {
        let mut all = self.control.lock().unwrap_or_else(|e| e.into_inner());
        if control == ChannelControl::default() {
            all.remove(channel);
        } else {
            all.insert(channel.to_owned(), control);
        }
    }

    /// Whether a source of the channel takes any piece over.
    pub fn anytime_of(&self, channel: &str) -> bool {
        self.control(channel).anytime.unwrap_or(self.anytime)
    }

    /// A line for the live API's events.
    pub fn event(&self, channel: &str, text: impl Into<String>) {
        let mut events = self.events.lock().unwrap_or_else(|e| e.into_inner());
        if events.len() >= MAX_EVENTS {
            events.pop_front();
        }
        events.push_back((crate::time::now_ms(), channel.to_owned(), text.into()));
    }

    /// The events, newest first.
    pub fn events(&self) -> Vec<Event> {
        self.events.lock().unwrap_or_else(|e| e.into_inner()).iter().rev().cloned().collect()
    }

    /// The source of a channel (live, audio, video, seconds), None without one.
    pub fn source(&self, channel: &str) -> Option<Value> {
        self.feed(channel).map(|feed| feed.state())
    }

    /// The channels with a source.
    pub fn channels(&self) -> Vec<String> {
        let mut channels: Vec<String> = self.feeds().keys().cloned().collect();
        channels.sort();
        channels
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

    /// The channel's source is live and not held: the studio is on the channel.
    pub fn is_live(&self, channel: &str) -> bool {
        self.feed(channel).is_some_and(|feed| feed.is_live()) && !self.control(channel).hold
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
        let hold = self.control(channel).hold;
        if let Value::Object(members) = &mut state {
            if hold {
                // Held: the overlay leaves (or does not take) the channel.
                for (key, value) in members.iter_mut() {
                    if key == "live" {
                        *value = false.into();
                    }
                }
            }
            members.insert(0, ("enabled".into(), self.enabled().into()));
            members.insert(1, ("anytime".into(), self.anytime_of(channel).into()));
            members.insert(2, ("hold".into(), hold.into()));
        }
        http::respond(&mut stream, 200, Some("application/json"), state.to_string().as_bytes())
    }

    /// The listeners' token in `Authorization: Bearer` (ingest on).
    pub fn authorized(&self, request: &Request) -> bool {
        let bearer = request.header("authorization").and_then(|v| v.strip_prefix("Bearer ")).unwrap_or("");
        self.enabled() && constant_eq(bearer.trim().as_bytes(), self.token.as_bytes())
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
            self.event(&channel, "odbiornik: zły token (odrzucony)");
            return http::respond(&mut stream, 401, None, b"");
        }
        if channel.is_empty() {
            return http::respond(&mut stream, 400, Some("text/plain"), b"channel?");
        }
        let Some(feed) = self.start(&channel) else {
            crate::warn!("ingest: channel {channel} already has a live source; the new one is refused");
            self.event(&channel, "drugi sygnał odrzucony (kanał ma już źródło)");
            return http::respond(&mut stream, 409, None, b"");
        };
        stream.write_all(b"HTTP/1.0 200 OK\r\nConnection: close\r\n\r\n")?;
        stream.set_read_timeout(Some(self.idle))?;
        crate::info!("ingest: channel {channel}: source connected");
        self.event(&channel, "nadawca połączony");
        crate::asrun::log(&self.etv_config, &channel, &crate::asrun::Entry::of(crate::asrun::LIVE_ON).detail("nadawca połączony"));
        let result = self.receive(&channel, &feed, &mut stream);
        let seconds = feed.lock().since.map(|t| t.elapsed().as_secs());
        self.finish(&channel, &feed);
        match (&result, seconds) {
            (Err(error), Some(s)) => crate::info!("ingest: channel {channel}: live ended after {s} s ({error})"),
            (Ok(()), Some(s)) => crate::info!("ingest: channel {channel}: live ended after {s} s"),
            (_, None) => crate::info!("ingest: channel {channel}: source gone before a keyframe"),
        }
        match seconds {
            Some(s) => {
                let text = format!("sygnał zakończony po {}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60);
                crate::asrun::log(&self.etv_config, &channel, &crate::asrun::Entry::of(crate::asrun::LIVE_OFF).detail(text.clone()));
                self.event(&channel, text);
            }
            None => {
                crate::asrun::log(
                    &self.etv_config,
                    &channel,
                    &crate::asrun::Entry::of(crate::asrun::LIVE_OFF).detail("nadawca rozłączony przed klatką kluczową"),
                );
                self.event(&channel, "nadawca rozłączony przed klatką kluczową");
            }
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
                let audio = if streams.audio { "z dźwiękiem" } else { "bez dźwięku" };
                self.event(channel, format!("sygnał ({}, {audio})", streams.video.unwrap_or("?")));
            }
        }
    }

    /// `GET /ingest`: the live source of a channel for the overlay's ffmpeg.
    pub fn read_request(&self, request: &Request, mut stream: TcpStream) -> io::Result<()> {
        let channel = request.param("channel").unwrap_or("");
        let held = self.control(channel).hold;
        let Some(feed) = self.feed(channel).filter(|f| f.is_live() && !held) else {
            if request.param("slot") == Some("1") {
                // No Content-Length: ffprobe gives no length, ErsatzTV takes the programme's.
                stream.write_all(b"HTTP/1.0 200 OK\r\nContent-Type: video/mp2t\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n")?;
                return stream.write_all(STANDBY);
            }
            return http::respond(&mut stream, 404, None, b"");
        };
        stream.set_write_timeout(Some(READER_TIMEOUT))?;
        let head = "HTTP/1.0 200 OK\r\nContent-Type: video/mp2t\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n";
        stream.write_all(head.as_bytes())?;
        // A live run of the overlay (`take`) goes on from where the one before stopped.
        let take = request.param("take").and_then(|t| t.parse::<f64>().ok()).filter(|t| t.is_finite() && *t > 0.0);
        let mut from = match take {
            Some(take) => {
                let (from, behind, dropped) = feed.follow(take);
                match dropped {
                    Some(lag) => crate::warn!(
                        "ingest: channel {channel}: live run of {take:.1} s from the live edge: the runs before left it {lag:.1} s behind (slower than the clock, or the channel held)"
                    ),
                    None => crate::info!("ingest: channel {channel}: live run of {take:.1} s from {behind:.1} s behind the source"),
                }
                from
            }
            None => From::Edge,
        };
        while let Some((runs, last)) = feed.next(from, Duration::from_secs(1)) {
            if self.control(channel).hold {
                // Held from the panel: the reader (ErsatzTV in a live slot) ends.
                return Ok(());
            }
            for run in runs {
                stream.write_all(&run)?;
            }
            if let Some(last) = last {
                from = From::After(last);
            }
        }
        Ok(())
    }
}

/// Comparison that takes as long for every wrong token.
pub fn constant_eq(a: &[u8], b: &[u8]) -> bool {
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

        let (first, last) = feed.next(From::Edge, Duration::from_millis(10)).unwrap();
        let joined: Vec<u8> = first.iter().flat_map(|r| r.iter().copied()).collect();
        assert_eq!(joined, [psi(), packet(0x100, true, true, &[2]), packet(0x101, true, false, &[3])].concat());

        // Nothing new: an empty answer after the wait, then the next run.
        let last = From::After(last.unwrap());
        assert_eq!(feed.next(last, Duration::from_millis(10)).unwrap().0.len(), 0);
        let runs = scanner.push(&packet(0x100, true, true, &[4]));
        feed.add(&scanner, runs);
        let (next, _) = feed.next(last, Duration::from_millis(10)).unwrap();
        assert_eq!(next.iter().flat_map(|r| r.iter().copied()).collect::<Vec<_>>(), packet(0x100, true, true, &[4]));

        feed.end();
        assert!(feed.next(last, Duration::from_millis(10)).is_none());
        assert!(!feed.is_live());
    }

    /// A video PES header with `pts` (90 kHz).
    fn pes(pts: u64) -> Vec<u8> {
        let b = [
            0x21 | ((pts >> 29) & 0x0e) as u8,
            (pts >> 22) as u8,
            0x01 | ((pts >> 14) & 0xfe) as u8,
            (pts >> 7) as u8,
            0x01 | ((pts << 1) & 0xfe) as u8,
        ];
        [&[0, 0, 1, 0xe0, 0, 0, 0x80, 0x80, 5][..], &b].concat()
    }

    /// A feed with groups of pictures of one second: keyframes at PTS `from`..`to` s.
    fn feed_of_seconds(from: u64, to: u64) -> (Feed, Scanner) {
        let feed = Feed::new();
        let mut scanner = Scanner::default();
        let runs = scanner.push(&psi());
        feed.add(&scanner, runs);
        for second in from..to {
            add_second(&feed, &mut scanner, second);
        }
        (feed, scanner)
    }

    fn add_second(feed: &Feed, scanner: &mut Scanner, second: u64) {
        let key = packet(0x100, true, true, &pes((second * 90_000) % PTS_WRAP));
        let runs = scanner.push(&[key, packet(0x101, true, false, &[second as u8])].concat());
        feed.add(scanner, runs);
    }

    /// The PTS (s) of the first keyframe a reader from `from` gets.
    fn first_second(feed: &Feed, from: From) -> u64 {
        let (runs, _) = feed.next(from, Duration::from_millis(10)).unwrap();
        let data: Vec<u8> = runs.iter().flat_map(|r| r.iter().copied()).collect();
        assert_eq!(&data[..2 * PACKET], &psi()[..], "the headers first");
        pes_pts(payload(&data[2 * PACKET..3 * PACKET])).unwrap() / 90_000
    }

    #[test]
    fn timestamps_of_keyframes() {
        assert_eq!(pes_pts(&pes(123_456_789)), Some(123_456_789));
        assert_eq!(pes_pts(&pes(PTS_WRAP - 1)), Some(PTS_WRAP - 1));
        assert_eq!(pes_pts(&[0, 0, 1, 0xe0, 0, 0, 0x80, 0, 0, 0, 0, 0, 0, 0]), None, "no PTS flag");
        assert_eq!(pts_seconds(180_000, 90_000), 1.0);
        assert_eq!(pts_seconds(90_000, 180_000), -1.0);
        // Across the 33-bit wrap.
        assert_eq!(pts_seconds(45_000, PTS_WRAP - 45_000), 1.0);
        let mut scanner = Scanner::default();
        scanner.push(&psi());
        let runs = scanner.push(&packet(0x100, true, true, &pes(900_000)));
        assert_eq!((runs[0].key, runs[0].pts), (true, Some(900_000)));
    }

    #[test]
    fn history_keeps_whole_groups_of_pictures() {
        let (feed, _) = feed_of_seconds(0, 40);
        let buffer = feed.lock();
        let front = buffer.runs.front().unwrap();
        assert!(front.key, "the buffer starts at a keyframe");
        let span = pts_seconds(buffer.runs.back().unwrap().pts.unwrap_or(39 * 90_000), front.pts.unwrap());
        assert!((HISTORY - 1.0..=HISTORY).contains(&span), "{span}");
        drop(buffer);
        // A source without timestamps keeps its last group of pictures, as before.
        let feed = Feed::new();
        let mut scanner = Scanner::default();
        for value in 0..5 {
            let runs = scanner.push(&[psi(), packet(0x100, true, true, &[value]), packet(0x101, true, false, &[value])].concat());
            feed.add(&scanner, runs);
        }
        assert_eq!(feed.lock().runs.len(), 1);
        assert_eq!(feed.follow(44.0).0, From::Edge);
    }

    #[test]
    fn live_runs_go_on_from_the_cursor() {
        let (feed, mut scanner) = feed_of_seconds(100, 110);
        // The first run starts at the live edge; the cursor is 4 s on from there.
        let (from, behind, dropped) = feed.follow(4.0);
        assert_eq!((first_second(&feed, from), behind, dropped), (109, 0.0, None));
        // The run took 5.5 s of the clock for its 4 s: the next one goes on at 113, behind the edge.
        for second in 110..115 {
            add_second(&feed, &mut scanner, second);
        }
        let (from, behind, dropped) = feed.follow(4.0);
        assert_eq!((first_second(&feed, from), behind, dropped), (113, 1.0, None));
        // A cursor further than MAX_LAG behind (a held channel): the live edge again.
        for second in 115..135 {
            add_second(&feed, &mut scanner, second);
        }
        // The cursor was at 117: 17 s behind the edge, dropped (and said so in the log).
        let (from, behind, dropped) = feed.follow(4.0);
        assert_eq!((first_second(&feed, from), behind, dropped), (134, 0.0, Some(17.0)));
        // Other readers start at the live edge whatever the cursor.
        assert_eq!(first_second(&feed, From::Edge), 134);
        // A start gone from the buffer: the live edge.
        assert_eq!(first_second(&feed, From::Start(0)), 134);
    }

    #[test]
    fn idle_time_of_a_source() {
        assert_eq!(Ingest::new("t", true).idle(), DEFAULT_IDLE);
        assert_eq!(Ingest::new("t", true).with_idle(Duration::from_secs(12)).idle(), Duration::from_secs(12));
        assert_eq!(Ingest::new("t", true).with_idle(Duration::from_millis(200)).idle(), Duration::from_secs(1));
    }

    #[test]
    fn one_source_per_channel() {
        let ingest = Ingest::new("t", true);
        let first = ingest.start("1").unwrap();
        assert!(ingest.start("1").is_none(), "an active source is not pre-empted");
        assert!(ingest.start("2").is_some());
        ingest.finish("1", &first);
        assert!(ingest.start("1").is_some());
        assert!(!Ingest::new("", true).enabled());
        // The standby second is MPEG-TS with H.264 and audio.
        let mut scanner = Scanner::default();
        let runs = scanner.push(STANDBY);
        assert_eq!(scanner.streams, Streams { video: Some("h264"), audio: true });
        assert!(runs.iter().any(|r| r.key));
        assert!(constant_eq(b"abc", b"abc") && !constant_eq(b"abc", b"abd") && !constant_eq(b"ab", b"abc"));
    }
}
