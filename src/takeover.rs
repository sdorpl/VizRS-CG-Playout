// Live takeover in the ffmpeg overlay (VIZRS_FFMPEG_INGEST=1, DOKUMENTACJA §1,
// Live ingest): a piece of the channel that ErsatzTV transcodes to HLS is
// played as a sequence of ffmpeg runs into the same playlist (ErsatzTV writes
// it with `append_list`):
//
//   the piece from where it is (`-ss` + produced, `-t` remaining)
//   └─ live on the channel → that run stops (SIGINT: ffmpeg closes the segment)
//      the live source of the sidecar (GET /ingest?take=<rest of the piece>) until
//      it ends or the piece does; the next live run goes on where it stopped
//      └─ the piece again, from where the schedule is by now
//
// The same runs serve the graphics zones of `area: auto` (PLAN-strefy S3b):
// zones widened in the middle of the piece stop the run of the piece, and the
// next one, from where it was, overlays the frames in the new zones.
//
// Every run gets `-output_ts_offset` of what the piece produced so far, so the
// timestamps go on as from one ffmpeg; the length produced is read from the
// playlist (EXTINF of the new segments), as ErsatzTV counts it. Live replaces
// the schedule, it does not delay it: the programme goes on where the clock is.
// The live run is a command of its own (the source scaled to the channel,
// graphics overlaid, ErsatzTV's encoder and HLS options); the piece's own
// command stays as it was. Whatever goes wrong with live: the piece plays on.

use std::collections::HashSet;
use std::ffi::OsString;
use std::io::{self, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use regex::Regex;

use crate::area::Area;
use crate::json;
use crate::net;
use crate::overlay::{placed_areas, quote_plus};

/// Shorter remaining time than this ends the piece.
const DONE: f64 = 0.1;
const POLL: Duration = Duration::from_millis(250);
/// A live run that brought less than this: live is left alone for `SUPPRESS`.
const SHORT_LIVE: f64 = 1.0;
/// ...but only a run that had at least this much of the piece to make. The tail
/// of a piece after a live run stopped for overrunning (a second or two) is
/// shorter than ffmpeg's start: judging the source by it put the schedule on air
/// for that tail (S1, 2026-11-13: "gave no picture" after every overrun).
const JUDGED_LIVE: f64 = 5.0;
const SUPPRESS: Duration = Duration::from_secs(30);
const PROBE_WAIT: Duration = Duration::from_secs(3);
/// A run that has not ended this long after SIGINT gets SIGTERM, and SIGKILL
/// `KILL_GRACE` later: a hung ffmpeg (e.g. CUDA with a graphics pipe) must not
/// keep live off the channel and hold the GPU.
const STOP_GRACE: Duration = Duration::from_secs(3);
const KILL_GRACE: Duration = Duration::from_secs(2);
/// A live run reads a source that never ends, and ffmpeg with `-t` reached
/// can wait on its inputs instead of exiting (ErsatzTV then sees the piece
/// stalled for a minute and the viewers' players freeze). The live run goes at
/// the clock: alive this long past the rest of the piece, it is stopped.
const LIVE_OVERRUN: Duration = Duration::from_secs(3);
/// A long live run writes its pace to the trace this often: a run slower than
/// the clock drains the channel's HLS buffer long before it ends.
const LIVE_PACE_EVERY: Duration = Duration::from_secs(120);
/// Global options of ErsatzTV's command that the live run keeps (the rest
/// before the first input are options of the media input).
const GLOBAL_FLAGS: [&str; 5] = ["-hide_banner", "-nostdin", "-nostats", "-y", "-benchmark"];
const GLOBAL_WITH_VALUE: [&str; 9] = [
    "-loglevel",
    "-v",
    "-threads",
    "-filter_threads",
    "-filter_complex_threads",
    "-init_hw_device",
    "-filter_hw_device",
    "-vaapi_device",
    "-qsv_device",
];
/// Output options of ErsatzTV's command the live run sets itself.
const OUTPUT_DROPPED: [&str; 11] =
    ["-filter_complex", "-map", "-t", "-ss", "-output_ts_offset", "-vf", "-af", "-filter:v", "-filter:a", "-shortest_buf_duration", "-to"];
const INITIAL_DISCONTINUITY: &str = "mpegts_flags=+initial_discontinuity";

/// A transcode of a piece to HLS that live can take over.
#[derive(Debug, PartialEq)]
pub struct Job {
    pub channel: String,
    pub playlist: PathBuf,
    /// Length of the piece (`-t` of the output), seconds.
    pub duration: f64,
    /// Its `-output_ts_offset`, seconds.
    pub offset: f64,
    /// A live programme of the schedule (its media is the live slot of the
    /// sidecar, src/live_shows.rs): live or the standby picture, never the media.
    pub slot: bool,
}

/// Channel of a live slot input (`…/ingest?channel=N&slot=1`) of `args`.
pub fn slot_channel(args: &[OsString]) -> Option<String> {
    let input = (0..args.len().saturating_sub(1)).find(|&i| args[i] == "-i").map(|i| arg(args, i + 1))?;
    let url = net::Url::split(input)?;
    let params: Vec<(String, String)> = url
        .query
        .split('&')
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (crate::http::percent_decode(k), crate::http::percent_decode(v)))
        .collect();
    let param = |name: &str| params.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
    (url.path.ends_with("/ingest") && param("slot").as_deref() == Some("1")).then(|| param("channel")).flatten().filter(|c| !c.is_empty())
}

fn arg(args: &[OsString], i: usize) -> &str {
    args.get(i).and_then(|a| a.to_str()).unwrap_or("")
}

/// Index of the last `-i` (its value follows).
fn last_input(args: &[OsString]) -> Option<usize> {
    (0..args.len().saturating_sub(1)).rev().find(|&i| args[i] == "-i")
}

/// ffmpeg's duration syntax: `44000ms`, `1500000us`, `44.5`, `44s`, `00:00:44.000`.
pub fn parse_time(text: &str) -> Option<f64> {
    let text = text.trim();
    let (sign, text) = text.strip_prefix('-').map_or((1.0, text), |t| (-1.0, t));
    let seconds = if let Some(ms) = text.strip_suffix("ms") {
        ms.parse::<f64>().ok()? / 1000.0
    } else if let Some(us) = text.strip_suffix("us") {
        us.parse::<f64>().ok()? / 1e6
    } else if text.contains(':') {
        text.split(':').try_fold(0.0, |total, part| part.parse::<f64>().ok().map(|p| total * 60.0 + p))?
    } else {
        text.strip_suffix('s').unwrap_or(text).parse().ok()?
    };
    (seconds.is_finite()).then_some(sign * seconds)
}

fn ms(seconds: f64) -> OsString {
    format!("{}ms", (seconds * 1000.0).round().max(0.0) as u64).into()
}

impl Job {
    /// The HLS transcode in `args`: one output (the playlist, last), its `-t`,
    /// video encoded (not copied). The channel is that of a live slot input,
    /// `channel` (from the canvas) or the playlist's folder (ErsatzTV:
    /// `/transcode/<channel>/`).
    pub fn of(args: &[OsString], channel: Option<&str>) -> Option<Job> {
        let hls = (0..args.len().saturating_sub(1)).find(|&i| args[i] == "-f" && args[i + 1] == "hls")?;
        let last = last_input(args)?;
        let output = &args[last + 2..];
        let value = |name: &str| (0..output.len().saturating_sub(1)).rev().find(|&i| output[i] == name).map(|i| arg(output, i + 1));
        let codec = ["-c:v", "-vcodec", "-codec:v", "-c"].iter().find_map(|o| value(o)).unwrap_or("");
        if codec == "copy" || hls < last {
            return None;
        }
        let duration = parse_time(value("-t")?)?;
        let offset = value("-output_ts_offset").and_then(parse_time).unwrap_or(0.0);
        let playlist = PathBuf::from(args.last()?);
        let folder = playlist.parent().and_then(Path::file_name).and_then(|n| n.to_str()).unwrap_or("");
        let slot = slot_channel(args);
        let channel = slot.clone().unwrap_or_else(|| channel.unwrap_or(folder).to_owned());
        (duration > 0.0 && !channel.is_empty()).then_some(Job { channel, playlist, duration, offset, slot: slot.is_some() })
    }

    /// The output options of a run that starts `elapsed` into the piece.
    fn timing(&self, out: &mut Vec<OsString>, elapsed: f64) {
        let last = last_input(out).map_or(0, |i| i + 2);
        let mut i = last;
        while i + 1 < out.len() {
            let name = arg(out, i);
            if name == "-t"
                || name == "-output_ts_offset"
                || (elapsed > 0.0 && name == "-hls_segment_options" && arg(out, i + 1) == INITIAL_DISCONTINUITY)
            {
                out.drain(i..i + 2);
                continue;
            }
            i += 1;
        }
        let at = out.len().saturating_sub(1);
        let mut timing = vec!["-t".into(), ms(self.duration - elapsed)];
        if self.offset + elapsed > 0.0 {
            timing.extend(["-output_ts_offset".into(), ms(self.offset + elapsed)]);
        }
        out.splice(at..at, timing);
    }

    /// ErsatzTV's command of the piece, started `elapsed` into it: the seek of
    /// its inputs moved on (the first input gets one if it has none).
    pub fn program(&self, args: &[OsString], elapsed: f64) -> Vec<OsString> {
        let mut out = args.to_vec();
        if elapsed <= 0.0 {
            return out;
        }
        let inputs: Vec<usize> = (0..out.len().saturating_sub(1)).filter(|&i| out[i] == "-i").collect();
        // From the last input back, so that an inserted seek moves nothing still to do.
        for (n, &input) in inputs.iter().enumerate().rev() {
            let start = if n == 0 { 0 } else { inputs[n - 1] + 2 };
            let options = &out[start..input];
            let seek = (0..options.len().saturating_sub(1)).find(|&j| options[j] == "-ss").map(|j| start + j);
            let lavfi = (0..options.len().saturating_sub(1)).any(|j| options[j] == "-f" && options[j + 1] == "lavfi");
            let pipe = arg(&out, input + 1).starts_with("pipe:");
            match seek {
                Some(at) => {
                    let moved = parse_time(arg(&out, at + 1)).unwrap_or(0.0) + elapsed;
                    out[at + 1] = ms(moved);
                }
                None if n == 0 && !lavfi && !pipe => {
                    out.splice(input..input, ["-ss".into(), ms(elapsed)]);
                }
                None => {}
            }
        }
        self.timing(&mut out, elapsed);
        out
    }
}

/// The channel's picture, for the live run.
#[derive(Clone, Debug, PartialEq)]
pub struct Format {
    pub width: u32,
    pub height: u32,
    pub rate: String,
}

/// Graphics frames of the overlay for the live run: their input (options and
/// `-i pipe:N`) and areas (stacked; empty = the whole channel).
pub struct Graphics<'a> {
    pub input: Vec<OsString>,
    pub areas: &'a [(Area, u32)],
}

/// The live source of the sidecar.
#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub url: String,
    pub audio: bool,
}

/// What a run of a live slot or takeover shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Picture<'a> {
    /// The live source.
    Live(&'a Source),
    /// No source in a live programme: an image (path) or black, with silence.
    Standby(Option<&'a str>),
}

/// Size from `pad=W:H` or `scale=W:H` in ErsatzTV's filters (the picture is
/// scaled to the channel when it is not its size).
pub fn filter_size(args: &[OsString]) -> Option<(u32, u32)> {
    let pattern = Regex::new(r"(?:^|[,;\]])(?:pad|scale)\w*=(?:w=)?(\d+):(?:h=)?(\d+)").ok()?;
    (0..args.len().saturating_sub(1)).filter(|&i| args[i] == "-filter_complex").find_map(|i| {
        let found = pattern.captures(arg(args, i + 1))?;
        Some((found[1].parse().ok()?, found[2].parse().ok()?))
    })
}

/// The encoder of the output video.
fn encoder(args: &[OsString]) -> &str {
    let last = last_input(args).map_or(0, |i| i + 2);
    let output = &args[last..];
    (0..output.len().saturating_sub(1))
        .rev()
        .find(|&i| ["-c:v", "-vcodec", "-codec:v"].iter().any(|o| output[i] == *o))
        .map_or("", |i| arg(output, i + 1))
}

/// The live run: `picture` in place of the piece's media, scaled and padded
/// to the channel, at its rate, graphics overlaid as the piece overlays them,
/// with the piece's global, encoder and HLS options; starts `elapsed` into the piece.
pub fn live_args(
    args: &[OsString],
    job: &Job,
    elapsed: f64,
    picture: Picture,
    format: &Format,
    graphics: Option<&Graphics>,
) -> Vec<OsString> {
    let first = (0..args.len()).find(|&i| args[i] == "-i").unwrap_or(args.len());
    let mut out: Vec<OsString> = Vec::new();
    let mut i = 0;
    while i < first {
        if GLOBAL_FLAGS.iter().any(|f| args[i] == *f) {
            out.push(args[i].clone());
        } else if i + 1 < first && GLOBAL_WITH_VALUE.iter().any(|f| args[i] == *f) {
            out.extend([args[i].clone(), args[i + 1].clone()]);
            i += 1;
        }
        i += 1;
    }
    let (w, h) = (format.width, format.height);
    let silence = |out: &mut Vec<OsString>, realtime: bool| {
        let pace: &[&str] = if realtime { &["-re"] } else { &[] };
        let input = pace.iter().chain(&["-f", "lavfi", "-i", "anullsrc=channel_layout=stereo:sample_rate=48000"]);
        out.extend(input.map(OsString::from));
    };
    let mut inputs = 1;
    let audio_input = match picture {
        Picture::Live(source) => {
            // The sidecar starts the stream at a keyframe, PAT and PMT first: 1 s of
            // analysis is enough (ffmpeg's 5 s would stall the piece as long).
            let live = [
                "-fflags",
                "+genpts+discardcorrupt",
                "-analyzeduration",
                "1000000",
                "-thread_queue_size",
                "1024",
                "-f",
                "mpegts",
                "-rw_timeout",
                "5000000",
            ];
            out.extend(live.iter().map(OsString::from));
            // The input itself ends with the piece: ffmpeg with only the output's `-t`
            // reached waits on its inputs instead of exiting, and a live run that has
            // made up its start (src/ingest.rs) got there early and stood until
            // LIVE_OVERRUN stopped it (47 s for a 44 s piece). With the source's end
            // the overlay (`shortest=1`) and the run end as soon as the piece is made.
            out.extend(["-t".into(), ms(job.duration - elapsed), "-i".into(), source.url.as_str().into()]);
            if source.audio {
                "0:a:0".to_owned()
            } else {
                silence(&mut out, false);
                inputs += 1;
                "1:a".to_owned()
            }
        }
        Picture::Standby(image) => {
            // Generated pictures come as fast as asked: `-re` keeps them at the clock, as live.
            let video: Vec<String> = match image {
                Some(path) => ["-re", "-loop", "1", "-framerate", &format.rate, "-i", path].map(str::to_owned).to_vec(),
                None => ["-re", "-f", "lavfi", "-i", &format!("color=c=black:s={w}x{h}:r={}", format.rate)].map(str::to_owned).to_vec(),
            };
            out.extend(video.into_iter().map(OsString::from));
            silence(&mut out, true);
            inputs += 1;
            "1:a".to_owned()
        }
    };
    let mut video = format!(
        "[0:v:0]scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:-1:-1:color=black,setsar=1,fps={},format=yuv420p",
        format.rate
    );
    if let Some(graphics) = graphics {
        out.extend(graphics.input.iter().cloned());
        let number = inputs;
        // `shortest`: the graphics never end; the run ends with the picture.
        let base =
            format!("{video}[vz_live];[{number}:0]format=yuva420p[vz_gfx];[vz_live][vz_gfx]overlay=x=0:y=0:format=auto:shortest=1[vz_ov]");
        let at_origin = matches!(graphics.areas, [(a, _)] if (a.x, a.y) == (0, 0));
        video =
            if graphics.areas.is_empty() || at_origin { base.clone() } else { placed_areas(&base, number, graphics.areas).unwrap_or(base) };
        video.push_str(";[vz_ov]null");
    }
    let codec = encoder(args);
    if codec.ends_with("_vaapi") || codec.ends_with("_vulkan") {
        video.push_str(",format=nv12,hwupload");
    } else if codec.ends_with("_qsv") {
        video.push_str(",format=nv12");
    }
    video.push_str("[vz_v]");
    let audio = format!("[{audio_input}]aresample=48000:async=1:first_pts=0,aformat=channel_layouts=stereo[vz_a]");
    out.extend([
        "-filter_complex".into(),
        format!("{video};{audio}").into(),
        "-map".into(),
        "[vz_v]".into(),
        "-map".into(),
        "[vz_a]".into(),
    ]);

    let last = last_input(args).map_or(0, |i| i + 2);
    let output = &args[last..args.len().saturating_sub(1)];
    let mut j = 0;
    while j < output.len() {
        let name = arg(output, j);
        if OUTPUT_DROPPED.contains(&name) && j + 1 < output.len() {
            j += 2;
            continue;
        }
        if name == "-shortest" {
            j += 1;
            continue;
        }
        out.push(output[j].clone());
        j += 1;
    }
    out.push(args[args.len() - 1].clone());
    job.timing(&mut out, elapsed);
    out
}

/// Names and lengths (EXTINF) of the segments in an HLS playlist.
pub fn segments(playlist: &str) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    let mut length = None;
    for line in playlist.lines().map(str::trim) {
        if let Some(info) = line.strip_prefix("#EXTINF:") {
            length = info.split(',').next().and_then(|v| v.trim().parse::<f64>().ok());
        } else if !line.is_empty() && !line.starts_with('#') {
            if let Some(length) = length.take() {
                out.push((line.to_owned(), length));
            }
        }
    }
    out
}

fn read_segments(path: &Path) -> Vec<(String, f64)> {
    std::fs::read_to_string(path).map(|text| segments(&text)).unwrap_or_default()
}

/// Seconds the piece has produced: the segments not there when it started.
fn produced(path: &Path, before: &HashSet<String>) -> f64 {
    read_segments(path).iter().filter(|(name, _)| !before.contains(name)).map(|(_, length)| length).sum()
}

/// `width=…`/`height=…` of ffprobe's `default=nw=1` output.
pub fn probed_size(text: &str) -> Option<(u32, u32)> {
    let value = |key: &str| text.lines().find_map(|l| l.trim().strip_prefix(key)).and_then(|v| v.trim().parse().ok());
    Some((value("width=")?, value("height=")?)).filter(|&(w, h)| w > 0 && h > 0)
}

/// Size of the newest segment in the playlist's folder: the channel's picture as it goes out.
fn segment_size(ffmpeg: &OsString, playlist: &Path) -> Option<(u32, u32)> {
    let folder = playlist.parent()?;
    let newest = std::fs::read_dir(folder)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "ts"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .max()?
        .1;
    let mut probe = Command::new(Path::new(ffmpeg).with_file_name("ffprobe"))
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height", "-of", "default=nw=1"])
        .arg(newest)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    while probe.try_wait().ok()?.is_none() {
        if started.elapsed() > PROBE_WAIT {
            let _ = probe.kill();
            let _ = probe.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut text = String::new();
    io::Read::read_to_string(&mut probe.stdout.take()?, &mut text).ok()?;
    probed_size(&text)
}

/// The channel's size: its latest segment, the graphics frames when they are
/// the whole channel (`full`), ErsatzTV's scaling, else 1920×1080. Its rate:
/// forced, the output's `-r`, the graphics frames', else 25.
pub fn channel_format(
    ffmpeg: &OsString,
    args: &[OsString],
    job: &Job,
    forced: &str,
    full: Option<(u32, u32)>,
    graphics_rate: &str,
) -> Format {
    let (width, height) = segment_size(ffmpeg, &job.playlist).or(full).or_else(|| filter_size(args)).unwrap_or((1920, 1080));
    let last = last_input(args).map_or(0, |i| i + 2);
    let output_rate = (last..args.len().saturating_sub(1)).rev().find(|&i| args[i] == "-r").map(|i| arg(args, i + 1).to_owned());
    let rate = [forced.to_owned(), output_rate.unwrap_or_default(), graphics_rate.to_owned()]
        .into_iter()
        .find(|r| !r.is_empty())
        .unwrap_or_else(|| "25".into());
    Format { width: width & !1, height: height & !1, rate }
}

/// `GET /ingest/state` of a channel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct State {
    pub enabled: bool,
    /// Takeover of any piece; false = only live programmes. An older sidecar: true.
    pub anytime: bool,
    pub live: bool,
    pub audio: bool,
}

pub fn state(sidecar: &str, channel: &str) -> Option<State> {
    let body = net::get(&format!("{sidecar}/ingest/state?channel={}", quote_plus(channel)), "").ok()?;
    let value = json::parse(&body).ok()?;
    let flag = |key, default| value.get(key).and_then(json::Value::as_bool).unwrap_or(default);
    Some(State { enabled: flag("enabled", false), anytime: flag("anytime", true), live: flag("live", false), audio: flag("audio", false) })
}

/// Live state of the channel, asked every `POLL` in the background.
struct Poller {
    live: Arc<AtomicBool>,
    audio: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

impl Poller {
    /// Not `active` (live may not take this piece over): never live, nothing asked.
    fn start(sidecar: &str, channel: &str, live: bool, audio: bool, active: bool) -> Poller {
        let poller = Poller { live: Arc::new(AtomicBool::new(live)), audio: Arc::new(AtomicBool::new(audio)), stop: Arc::default() };
        if !active {
            poller.live.store(false, Ordering::Relaxed);
            return poller;
        }
        let (live, audio, stop) = (Arc::clone(&poller.live), Arc::clone(&poller.audio), Arc::clone(&poller.stop));
        let (sidecar, channel) = (sidecar.to_owned(), channel.to_owned());
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(POLL);
                let (on, sound) = state(&sidecar, &channel).map_or((false, false), |s| (s.live, s.audio));
                live.store(on, Ordering::Relaxed);
                audio.store(sound, Ordering::Relaxed);
            }
        });
        poller
    }
}

impl Drop for Poller {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// What the sequence needs from the overlay.
pub struct Runner<'a> {
    pub sidecar: &'a str,
    /// Starts one run `elapsed` into the piece: its own command, or with a
    /// source the live run (graphics pipes are the overlay's business).
    pub run: &'a dyn Fn(f64, Option<&Source>) -> io::Result<Child>,
    /// The process now running, for the forwarded signals; 0 = none.
    pub current: &'a AtomicU32,
    /// ErsatzTV stopped the piece (a forwarded signal).
    pub stopping: &'a AtomicBool,
    /// Live may take the piece over; false = runs only for the graphics zones.
    pub takeover: bool,
    /// The run of the piece must start again (its graphics zones widened, S3b).
    pub restart: &'a dyn Fn() -> bool,
}

fn log(message: impl std::fmt::Display) {
    let _ = writeln!(io::stderr(), "vizrs-ffmpeg: {message}");
}

/// A trace file past this size is moved to `….1` (one old file kept).
const TRACE_MAX: u64 = 2 * 1024 * 1024;

/// Trace file of the channel: `$ETV_CONFIG_FOLDER/logs/vizrs-takeover-<channel>.log`
/// (it outlives the session and the container), else beside the playlist.
fn trace_path(job: &Job) -> Option<PathBuf> {
    let name: String = job.channel.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    let file = format!("vizrs-takeover-{name}.log");
    match std::env::var_os("ETV_CONFIG_FOLDER").filter(|v| !v.is_empty()) {
        Some(config) => Some(Path::new(&config).join("logs").join(file)),
        None => Some(job.playlist.parent()?.join(file)),
    }
}

/// State of the playlist as ErsatzTV Next reads it: media sequence, segments
/// listed, the last one with its PROGRAM-DATE-TIME, and segment files on disk.
fn playlist_state(path: &Path) -> String {
    let Ok(text) = std::fs::read_to_string(path) else {
        return "playlist missing".to_owned();
    };
    let sequence = text.lines().find_map(|l| l.trim().strip_prefix("#EXT-X-MEDIA-SEQUENCE:")).unwrap_or("?");
    let mut date = "?";
    let mut last_date = "?";
    let mut last = "-";
    let mut listed = 0;
    for line in text.lines().map(str::trim) {
        if let Some(d) = line.strip_prefix("#EXT-X-PROGRAM-DATE-TIME:") {
            date = d;
        } else if !line.is_empty() && !line.starts_with('#') {
            listed += 1;
            last = line;
            last_date = date;
        }
    }
    let on_disk = path
        .parent()
        .and_then(|dir| std::fs::read_dir(dir).ok())
        .map_or(0, |dir| dir.filter_map(Result::ok).filter(|e| e.path().extension().is_some_and(|x| x == "ts")).count());
    format!("seq {sequence}, listed {listed}, last {last} @ {last_date}, ts on disk {on_disk}")
}

/// `log` plus a line in the channel's trace file with the time and the playlist state:
/// the runs of a piece leave no other trace (ErsatzTV Next does not keep ffmpeg's stderr).
fn trace(job: &Job, message: impl std::fmt::Display) {
    let message = message.to_string();
    log(&message);
    let Some(path) = trace_path(job) else { return };
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > TRACE_MAX) {
        let mut old = path.clone().into_os_string();
        old.push(".1");
        let _ = std::fs::rename(&path, old);
    }
    let line = format!(
        "{} pid {} {message} | {}\n",
        crate::time::iso_time(crate::time::now_ms()),
        std::process::id(),
        playlist_state(&job.playlist)
    );
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = file.write_all(line.as_bytes());
    }
}

fn code(status: io::Result<ExitStatus>) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    match status {
        Ok(status) => status.code().or_else(|| status.signal().map(|s| 128 + s)).unwrap_or(1),
        Err(_) => 1,
    }
}

/// Plays the piece `job` as runs of its own command and of the live source; the exit code.
pub fn sequence(runner: &Runner, job: &Job, live_now: bool, audio_now: bool) -> i32 {
    let before: HashSet<String> = read_segments(&job.playlist).into_iter().map(|(name, _)| name).collect();
    let poller = Poller::start(runner.sidecar, &job.channel, live_now, audio_now, runner.takeover);
    let source_url = format!("{}/ingest?channel={}", runner.sidecar, quote_plus(&job.channel));
    let mut elapsed = 0.0;
    trace(
        job,
        format_args!(
            "channel {}: piece of {:.1} s (offset {:.1} s, live {live_now}, slot {})",
            job.channel, job.duration, job.offset, job.slot
        ),
    );
    // A live source that gave no picture is left alone until it goes or for `SUPPRESS`.
    let quiet: Mutex<Option<Instant>> = Mutex::new(None);
    let live = || {
        let on = poller.live.load(Ordering::Relaxed);
        let mut quiet = quiet.lock().unwrap_or_else(|e| e.into_inner());
        if quiet.is_some_and(|until| !on || Instant::now() >= until) {
            *quiet = None;
        }
        on && quiet.is_none()
    };
    loop {
        if job.duration - elapsed < DONE {
            return 0;
        }
        // `take`: the sidecar goes on from where the live run before stopped (src/ingest.rs).
        let source = live().then(|| Source {
            url: format!("{source_url}&take={:.3}", job.duration - elapsed),
            audio: poller.audio.load(Ordering::Relaxed),
        });
        if source.is_some() {
            trace(job, format_args!("channel {}: live from {elapsed:.1} s of the piece", job.channel));
        }
        let mut child = match (runner.run)(elapsed, source.as_ref()) {
            Ok(child) => child,
            Err(e) if source.is_some() => {
                trace(job, format_args!("live run: {e}; the schedule goes on"));
                *quiet.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now() + SUPPRESS);
                continue;
            }
            Err(e) => {
                trace(job, format_args!("ffmpeg: {e}"));
                return 127;
            }
        };
        runner.current.store(child.id(), Ordering::Relaxed);
        let mut interrupted = false;
        let mut restarted = false;
        let mut stop = Stop::default();
        let started = Instant::now();
        let mut paced = started;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) => {}
                Err(e) => break Err(e),
            }
            if source.is_some() && paced.elapsed() >= LIVE_PACE_EVERY {
                paced = Instant::now();
                let (made, wall) = (produced(&job.playlist, &before) - elapsed, started.elapsed().as_secs_f64());
                trace(job, format_args!("channel {}: live run made {made:.1} s in {wall:.1} s ({})", job.channel, pace(made, wall)));
            }
            if source.is_none() && !interrupted && !runner.stopping.load(Ordering::Relaxed) && live() {
                // ffmpeg ends on SIGINT as on `q`: the segment it writes is closed and listed.
                crate::sys::send_signal(child.id(), crate::sys::SIGINT);
                interrupted = true;
                stop.since = Some(Instant::now());
            }
            if source.is_none() && !interrupted && !runner.stopping.load(Ordering::Relaxed) && (runner.restart)() {
                crate::sys::send_signal(child.id(), crate::sys::SIGINT);
                interrupted = true;
                restarted = true;
                stop.since = Some(Instant::now());
            }
            if source.is_some() && !interrupted && overran(started.elapsed(), job.duration - elapsed) {
                trace(job, format_args!("channel {}: the live run did not end with the piece; stopping it", job.channel));
                crate::sys::send_signal(child.id(), crate::sys::SIGINT);
                interrupted = true;
                stop.since = Some(Instant::now());
            }
            if let Some(step) = stop.escalate(&mut child) {
                trace(job, format_args!("channel {}: the piece did not stop on SIGINT; {step}", job.channel));
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        runner.current.store(0, Ordering::Relaxed);
        let now = produced(&job.playlist, &before);
        if runner.stopping.load(Ordering::Relaxed) {
            return code(status);
        }
        if source.is_some() {
            let brought = now - elapsed;
            let wall = started.elapsed().as_secs_f64();
            trace(
                job,
                format_args!(
                    "channel {}: live run ended after {brought:.1} s in {wall:.1} s ({}, exit {})",
                    job.channel,
                    pace(brought, wall),
                    code(status)
                ),
            );
            if no_picture(brought, job.duration - elapsed) {
                trace(job, format_args!("channel {}: the live source gave no picture; the schedule goes on", job.channel));
                *quiet.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now() + SUPPRESS);
            }
        } else if !interrupted {
            let exit = code(status);
            trace(job, format_args!("channel {}: piece ended (exit {exit})", job.channel));
            return exit;
        } else if restarted {
            trace(
                job,
                format_args!(
                    "channel {}: piece restarted for new graphics zones after {:.1} s (exit {})",
                    job.channel,
                    now - elapsed,
                    code(status)
                ),
            );
        } else {
            trace(
                job,
                format_args!("channel {}: piece stopped for live after {:.1} s (exit {})", job.channel, now - elapsed, code(status)),
            );
        }
        elapsed = elapsed.max(now);
    }
}

/// Pace of a run for the trace: seconds of the piece made per second of the clock
/// (below 1.00x the channel falls behind and its HLS buffer drains).
fn pace(brought: f64, wall: f64) -> String {
    if wall > 0.0 { format!("{:.2}x", brought / wall) } else { "-".to_owned() }
}

/// A live run that brought `brought` of `remaining` seconds: its source gave no picture.
fn no_picture(brought: f64, remaining: f64) -> bool {
    brought < SHORT_LIVE && remaining >= JUDGED_LIVE
}

/// A live run alive `running` with `remaining` seconds of the piece to produce has overrun.
fn overran(running: Duration, remaining: f64) -> bool {
    running > Duration::from_secs_f64(remaining.max(0.0)) + LIVE_OVERRUN
}

/// Stopping a run after SIGINT: SIGTERM after `STOP_GRACE`, SIGKILL `KILL_GRACE` later.
#[derive(Default)]
struct Stop {
    since: Option<Instant>,
    termed: bool,
    killed: bool,
}

impl Stop {
    /// The next step for a run still alive, if one is due now (what was done, for the log).
    fn escalate(&mut self, child: &mut Child) -> Option<&'static str> {
        let waited = self.since?.elapsed();
        if !self.termed && waited >= STOP_GRACE {
            self.termed = true;
            crate::sys::send_signal(child.id(), crate::sys::SIGTERM);
            return Some("SIGTERM");
        }
        if !self.killed && waited >= STOP_GRACE + KILL_GRACE {
            self.killed = true;
            let _ = child.kill();
            return Some("SIGKILL");
        }
        None
    }
}

/// Frames of the overlay for runs one after another: each run gets its own
/// frame socket (`overlay::frame_channel`), switched at a frame boundary.
/// Between runs (a new ffmpeg starting) the stream waits for the next one
/// instead of dropping frames: the sidecar then sends the frames it owes at
/// once (its timeline is the clock), so a run can make up its start, as
/// ErsatzTV's piece after a slow start does. Frames dropped would hold every
/// run to the clock from the moment it attached: ~1 s lost per run, a live
/// channel at 0.96x.
pub struct Sink {
    pipe: Option<UnixStream>,
    frame: usize,
    offset: usize,
    shared: Arc<(Mutex<SinkShared>, Condvar)>,
}

#[derive(Default)]
struct SinkShared {
    pending: Option<UnixStream>,
    /// The socket of the run attached last, to cut it off (`SinkControl::cut`).
    attached: Option<UnixStream>,
    closed: bool,
}

/// Hands new frame sockets to a `Sink` and closes it.
#[derive(Clone)]
pub struct SinkControl(Arc<(Mutex<SinkShared>, Condvar)>);

impl Sink {
    pub fn new(frame: usize) -> (Sink, SinkControl) {
        let shared = Arc::new((Mutex::new(SinkShared::default()), Condvar::new()));
        (Sink { pipe: None, frame: frame.max(1), offset: 0, shared: Arc::clone(&shared) }, SinkControl(shared))
    }
}

impl SinkControl {
    fn update(&self, change: impl FnOnce(&mut SinkShared)) {
        let (lock, changed) = &*self.0;
        change(&mut lock.lock().unwrap_or_else(|e| e.into_inner()));
        changed.notify_all();
    }

    /// The next frames go to `pipe`.
    pub fn attach(&self, pipe: UnixStream) {
        self.update(|shared| {
            shared.attached = pipe.try_clone().ok();
            shared.pending = Some(pipe);
        });
    }

    /// The run attached last reads the end of its graphics (EOF) now: a run
    /// whose frames the sidecar holds (they would leave its zones) is not
    /// stuck reading them and stops at once on SIGINT. The next frames wait
    /// for the next run.
    pub fn cut(&self) {
        self.update(|shared| {
            if let Some(pipe) = shared.attached.take() {
                let _ = pipe.shutdown(std::net::Shutdown::Both);
            }
        });
    }

    /// The relay ends with its next write (also one waiting for a run).
    pub fn close(&self) {
        self.update(|shared| {
            shared.closed = true;
            shared.attached = None;
        });
    }
}

impl Drop for Sink {
    /// The runs' sockets close with the relay (no copy kept for `cut`).
    fn drop(&mut self) {
        let mut shared = self.shared.0.lock().unwrap_or_else(|e| e.into_inner());
        shared.attached = None;
        shared.pending = None;
    }
}

impl Write for Sink {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if self.offset == 0 {
            let (lock, changed) = &*self.shared;
            let mut shared = lock.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if shared.closed {
                    return Err(io::ErrorKind::BrokenPipe.into());
                }
                if let Some(pipe) = shared.pending.take() {
                    self.pipe = Some(pipe);
                }
                if self.pipe.is_some() {
                    break;
                }
                // No run: the stream waits for the next one.
                shared = changed.wait(shared).unwrap_or_else(|e| e.into_inner());
            }
        }
        let n = data.len().min(self.frame - self.offset);
        if let Some(pipe) = self.pipe.as_mut() {
            if pipe.write_all(&data[..n]).is_err() {
                // The run is over: the rest of this frame is dropped, the next one waits for a run.
                self.pipe = None;
            }
        }
        self.offset = (self.offset + n) % self.frame;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn hung_run_is_terminated_then_killed() {
        use super::{KILL_GRACE, STOP_GRACE, Stop};
        // ignores SIGINT and SIGTERM, like ffmpeg stuck on a graphics pipe
        let mut child = std::process::Command::new("sh").args(["-c", "trap '' INT TERM; sleep 30"]).spawn().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        crate::sys::send_signal(child.id(), crate::sys::SIGINT);
        let mut stop = Stop { since: Some(std::time::Instant::now()), ..Stop::default() };
        let mut steps = Vec::new();
        let start = std::time::Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            steps.extend(stop.escalate(&mut child));
            assert!(start.elapsed() < STOP_GRACE + KILL_GRACE + std::time::Duration::from_secs(3), "run not stopped");
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        assert_eq!(steps, ["SIGTERM", "SIGKILL"]);
        assert!(!status.success());
        assert!(start.elapsed() >= STOP_GRACE + KILL_GRACE);
    }

    #[test]
    fn playlist_state_lists_the_last_segment_and_files() {
        let dir = std::env::temp_dir().join(format!("vizrs-trace-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let playlist = dir.join("ffmpeg.m3u8");
        std::fs::write(
            &playlist,
            "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:7\n#EXTINF:4.0,\n#EXT-X-PROGRAM-DATE-TIME:T1\nlive000007.ts\n\
             #EXTINF:4.0,\n#EXT-X-PROGRAM-DATE-TIME:T2\nlive000008.ts\n",
        )
        .unwrap();
        std::fs::write(dir.join("live000008.ts"), b"").unwrap();
        let state = super::playlist_state(&playlist);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(state, "seq 7, listed 2, last live000008.ts @ T2, ts on disk 1");
        assert_eq!(super::playlist_state(&dir.join("none.m3u8")), "playlist missing");
    }

    #[test]
    fn live_run_past_the_piece_is_overrun() {
        use super::{LIVE_OVERRUN, overran};
        use std::time::Duration;
        assert!(!overran(Duration::from_secs(44), 44.0));
        assert!(!overran(Duration::from_secs(44) + LIVE_OVERRUN, 44.0));
        assert!(overran(Duration::from_secs(48), 44.0));
        assert!(overran(Duration::from_secs(4), -1.0));
    }

    #[test]
    fn a_short_tail_does_not_judge_the_source() {
        use super::{no_picture, pace};
        // The tail after an overrun (S1: 0.1–3 s left) proves nothing about the source.
        assert!(!no_picture(0.1, 0.1));
        assert!(!no_picture(0.7, 3.1));
        // A run with the piece ahead of it that brought nothing: no picture.
        assert!(no_picture(0.3, 44.0));
        assert!(!no_picture(1.5, 44.0));
        assert_eq!(pace(40.9, 47.0), "0.87x");
        assert_eq!(pace(44.0, 44.0), "1.00x");
        assert_eq!(pace(1.0, 0.0), "-");
    }

    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    const HLS: [&str; 15] = [
        "-f",
        "hls",
        "-hls_time",
        "4",
        "-hls_list_size",
        "0",
        "-hls_segment_filename",
        "/transcode/1/live%06d.ts",
        "-hls_segment_type",
        "mpegts",
        "-hls_flags",
        "program_date_time+omit_endlist+append_list+independent_segments",
        "-hls_segment_options",
        "mpegts_flags=+initial_discontinuity",
        "/transcode/1/live.m3u8",
    ];

    fn piece() -> Vec<OsString> {
        let mut list = vec![
            "-threads",
            "1",
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-fflags",
            "+genpts+discardcorrupt+igndts",
            "-ss",
            "12000ms",
            "-readrate",
            "1.0",
            "-noautorotate",
            "-i",
            "/m/News.mp4",
            "-filter_complex",
            "[0:1]aresample=48000[a]",
            "-filter_complex",
            "[0:0]fps=25,scale=1280:720:flags=fast_bilinear,setsar=1[v]",
            "-map",
            "[v]",
            "-map",
            "[a]",
            "-c:v",
            "libx264",
            "-b:v",
            "4000k",
            "-c:a",
            "aac",
            "-ac",
            "2",
            "-t",
            "44000ms",
            "-r",
            "25",
            "-fps_mode",
            "cfr",
            "-g",
            "50",
        ];
        list.extend(&HLS[..15]);
        args(&list)
    }

    #[test]
    fn times() {
        assert_eq!(parse_time("44000ms"), Some(44.0));
        assert_eq!(parse_time("1500000us"), Some(1.5));
        assert_eq!(parse_time("44.5"), Some(44.5));
        assert_eq!(parse_time("2s"), Some(2.0));
        assert_eq!(parse_time("01:00:02.5"), Some(3602.5));
        assert_eq!(parse_time("x"), None);
    }

    #[test]
    fn job_of_a_piece() {
        let job = Job::of(&piece(), None).unwrap();
        assert_eq!(job, Job { channel: "1".into(), playlist: "/transcode/1/live.m3u8".into(), duration: 44.0, offset: 0.0, slot: false });
        assert_eq!(Job::of(&piece(), Some("7")).unwrap().channel, "7");
        // Copied video, no length, not HLS: not taken over.
        let mut copy = piece();
        let at = copy.iter().position(|a| a == "libx264").unwrap();
        copy[at] = "copy".into();
        assert_eq!(Job::of(&copy, None), None);
        let short: Vec<OsString> = piece().into_iter().filter(|a| a != "-t" && a != "44000ms").collect();
        assert_eq!(Job::of(&short, None), None);
        assert_eq!(Job::of(&args(&["-i", "a", "-f", "mpegts", "-t", "1", "pipe:1"]), None), None);
    }

    #[test]
    fn program_goes_on_from_where_it_is() {
        let job = Job::of(&piece(), None).unwrap();
        assert_eq!(job.program(&piece(), 0.0), piece());
        let later = job.program(&piece(), 10.5);
        let text: Vec<&str> = later.iter().map(|a| a.to_str().unwrap()).collect();
        let at = |name: &str| text.iter().position(|a| *a == name).map(|i| text[i + 1]);
        assert_eq!(at("-ss"), Some("22500ms"), "the seek moves on");
        assert_eq!(at("-t"), Some("33500ms"));
        assert_eq!(at("-output_ts_offset"), Some("10500ms"));
        assert!(!text.contains(&INITIAL_DISCONTINUITY), "the timestamps go on");
        assert_eq!(text.last(), Some(&"/transcode/1/live.m3u8"));
        assert_eq!(text.len(), piece().len() + 2 - 2);

        // No seek: the first input gets one; an offset of the piece adds up.
        let mut list: Vec<&str> =
            vec!["-i", "/m/a.mp4", "-f", "lavfi", "-i", "anullsrc", "-c:v", "libx264", "-t", "8s", "-output_ts_offset", "100000ms"];
        list.extend(&HLS[..15]);
        let plain = args(&list);
        let job = Job::of(&plain, None).unwrap();
        assert_eq!(job.offset, 100.0);
        let later: Vec<String> = job.program(&plain, 2.0).iter().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(&later[..8], ["-ss", "2000ms", "-i", "/m/a.mp4", "-f", "lavfi", "-i", "anullsrc"]);
        let tail = &later[later.len() - 5..];
        assert_eq!(tail, ["-t", "6000ms", "-output_ts_offset", "102000ms", "/transcode/1/live.m3u8"]);
    }

    #[test]
    fn live_run_takes_the_encoder_and_hls_of_the_piece() {
        let job = Job::of(&piece(), None).unwrap();
        let format = Format { width: 1280, height: 720, rate: "25".into() };
        let source = Source { url: "http://cg:7700/ingest?channel=1".into(), audio: true };
        let live: Vec<String> = live_args(&piece(), &job, 4.0, Picture::Live(&source), &format, None)
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let expected = [
            "-threads",
            "1",
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-fflags",
            "+genpts+discardcorrupt",
            "-analyzeduration",
            "1000000",
            "-thread_queue_size",
            "1024",
            "-f",
            "mpegts",
            "-rw_timeout",
            "5000000",
            "-t",
            "40000ms",
            "-i",
            "http://cg:7700/ingest?channel=1",
            "-filter_complex",
            "[0:v:0]scale=1280:720:force_original_aspect_ratio=decrease,pad=1280:720:-1:-1:color=black,setsar=1,fps=25,format=yuv420p[vz_v];\
             [0:a:0]aresample=48000:async=1:first_pts=0,aformat=channel_layouts=stereo[vz_a]",
            "-map",
            "[vz_v]",
            "-map",
            "[vz_a]",
            "-c:v",
            "libx264",
            "-b:v",
            "4000k",
            "-c:a",
            "aac",
            "-ac",
            "2",
            "-r",
            "25",
            "-fps_mode",
            "cfr",
            "-g",
            "50",
            "-f",
            "hls",
            "-hls_time",
            "4",
            "-hls_list_size",
            "0",
            "-hls_segment_filename",
            "/transcode/1/live%06d.ts",
            "-hls_segment_type",
            "mpegts",
            "-hls_flags",
            "program_date_time+omit_endlist+append_list+independent_segments",
            "-t",
            "40000ms",
            "-output_ts_offset",
            "4000ms",
            "/transcode/1/live.m3u8",
        ];
        assert_eq!(live, expected);
    }

    #[test]
    fn live_run_without_audio_with_graphics_and_vaapi() {
        let mut list =
            vec!["-init_hw_device", "vaapi=hw:/dev/dri/renderD128", "-filter_hw_device", "hw", "-hwaccel", "vaapi", "-i", "/m/a.mp4"];
        list.extend(["-i", "http://localhost:8409/internal/graphics/1/7", "-c:v", "h264_vaapi", "-t", "10s"]);
        list.extend(&HLS[..15]);
        let piece = args(&list);
        let job = Job::of(&piece, Some("1")).unwrap();
        let format = Format { width: 1920, height: 1080, rate: "50".into() };
        let source = Source { url: "http://cg/ingest?channel=1".into(), audio: false };
        let area = [(Area { x: 0, y: 930, w: 1920, h: 140 }, 0)];
        let graphics = Graphics {
            input: args(&["-f", "rawvideo", "-pix_fmt", "yuva420p", "-video_size", "1920x140", "-framerate", "50", "-i", "pipe:5"]),
            areas: &area,
        };
        let live: Vec<String> = live_args(&piece, &job, 0.0, Picture::Live(&source), &format, Some(&graphics))
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            &live[..4],
            ["-init_hw_device", "vaapi=hw:/dev/dri/renderD128", "-filter_hw_device", "hw"],
            "no decoder options of the media"
        );
        assert!(live.windows(4).any(|w| w == ["-f", "lavfi", "-i", "anullsrc=channel_layout=stereo:sample_rate=48000"]));
        assert!(live.windows(2).any(|w| w == ["-i", "pipe:5"]));
        let filter = &live[live.iter().position(|a| a == "-filter_complex").unwrap() + 1];
        assert!(
            filter.contains(
                "[2:0]format=yuva420p[vz_gfx];[vz_live][vz_gfx]overlay=format=auto:shortest=1:x=0:y=930[vz_ov];[vz_ov]null,format=nv12,hwupload[vz_v]"
            ),
            "{filter}"
        );
        assert!(filter.ends_with("[1:a]aresample=48000:async=1:first_pts=0,aformat=channel_layouts=stereo[vz_a]"), "{filter}");
        assert!(!live.iter().any(|a| a == "-output_ts_offset"), "a piece starting at 0 has none");
    }

    #[test]
    fn live_slot_and_standby() {
        let mut list = vec!["-reconnect", "1", "-i", "http://cg:7700/ingest?channel=2&slot=1", "-c:v", "libx264", "-t", "1800000ms"];
        list.extend(&HLS[..15]);
        let slot = args(&list);
        assert_eq!(slot_channel(&slot).as_deref(), Some("2"));
        let job = Job::of(&slot, Some("9")).unwrap();
        assert_eq!((job.channel.as_str(), job.slot), ("2", true), "the slot names the channel");
        assert_eq!(slot_channel(&args(&["-i", "http://cg:7700/ingest?channel=2", "x"])), None);
        assert_eq!(slot_channel(&piece()), None);

        let format = Format { width: 1280, height: 720, rate: "25".into() };
        let black: Vec<String> =
            live_args(&slot, &job, 60.0, Picture::Standby(None), &format, None).iter().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(
            &black[..10],
            [
                "-re",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=1280x720:r=25",
                "-re",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=channel_layout=stereo:sample_rate=48000"
            ]
        );
        assert!(black.iter().any(|a| a.contains("[1:a]aresample")), "{black:?}");
        assert!(!black.iter().any(|a| a == "-reconnect"), "options of the slot input are not kept");
        assert!(black.windows(2).any(|w| w == ["-t", "1740000ms"]));
        let image: Vec<String> = live_args(&slot, &job, 0.0, Picture::Standby(Some("/media/slate.png")), &format, None)
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(&image[..7], ["-re", "-loop", "1", "-framerate", "25", "-i", "/media/slate.png"]);
    }

    #[test]
    fn sizes_and_playlists() {
        assert_eq!(filter_size(&piece()), Some((1280, 720)));
        assert_eq!(filter_size(&args(&["-filter_complex", "[0:0]pad=1920:1080:-1:-1:color=black[v]"])), Some((1920, 1080)));
        assert_eq!(filter_size(&args(&["-filter_complex", "[0:0]fps=25[v]"])), None);
        assert_eq!(probed_size("width=1600\nheight=900\n"), Some((1600, 900)));
        let playlist = "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:4.000000,\n#EXT-X-PROGRAM-DATE-TIME:x\nlive000000.ts\n#EXT-X-DISCONTINUITY\n#EXTINF:1.52,\nlive000001.ts\n";
        assert_eq!(segments(playlist), [("live000000.ts".into(), 4.0), ("live000001.ts".into(), 1.52)]);
    }

    #[test]
    fn sink_waits_for_a_run_and_switches_at_frame_boundaries() {
        use std::io::Read;
        let (mut sink, control) = Sink::new(4);
        // No run yet: the stream waits for one (nothing is dropped).
        let writer = std::thread::spawn(move || {
            sink.write_all(b"abcd").unwrap();
            sink
        });
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(!writer.is_finished(), "waits for a run");
        let (mut reader, socket) = crate::overlay::frame_channel().unwrap();
        control.attach(socket);
        let mut sink = writer.join().unwrap();
        sink.write_all(b"EF").unwrap();
        // The next run takes over at the frame boundary.
        let (mut second, socket) = crate::overlay::frame_channel().unwrap();
        control.attach(socket);
        sink.write_all(b"GHijkl").unwrap();
        drop(sink);
        let mut first = Vec::new();
        reader.read_to_end(&mut first).unwrap();
        assert_eq!(first, b"abcdEFGH");
        let mut next = Vec::new();
        second.read_to_end(&mut next).unwrap();
        assert_eq!(next, b"ijkl");

        // A run gone in the middle of a frame: its rest is dropped, the next frame waits.
        let (mut sink, control) = Sink::new(4);
        let (reader, socket) = crate::overlay::frame_channel().unwrap();
        control.attach(socket);
        drop(reader);
        sink.write_all(b"wxyz").unwrap();
        let closer = control.clone();
        let waiting = std::thread::spawn(move || sink.write_all(b"1234"));
        std::thread::sleep(std::time::Duration::from_millis(100));
        closer.close();
        assert!(waiting.join().unwrap().is_err(), "closed while waiting");

        let (mut sink, control) = Sink::new(2);
        control.close();
        assert!(sink.write_all(b"xx").is_err());

        // A run cut off (its frames held for new zones): it reads EOF at once,
        // the frames after it wait for the next run.
        let (mut sink, control) = Sink::new(2);
        let (mut reader, socket) = crate::overlay::frame_channel().unwrap();
        control.attach(socket);
        sink.write_all(b"ab").unwrap();
        control.cut();
        let mut got = Vec::new();
        reader.read_to_end(&mut got).unwrap();
        assert_eq!(got, b"ab");
        let (mut next, socket) = crate::overlay::frame_channel().unwrap();
        let later = std::thread::spawn(move || {
            sink.write_all(b"cdef").unwrap();
            sink
        });
        std::thread::sleep(std::time::Duration::from_millis(50));
        control.attach(socket);
        drop(later.join().unwrap());
        let mut got = Vec::new();
        next.read_to_end(&mut got).unwrap();
        assert_eq!(got, b"ef", "the frame written into the cut run is gone, the next one goes on");
    }
}
