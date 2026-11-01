// ffmpeg for ErsatzTV that overlays the VizRS graphics itself (binary
// vizrs-ffmpeg, set as the FFmpeg path in ErsatzTV; DOKUMENTACJA §1). Every
// call goes to the real ffmpeg unchanged, except a transcode that reads the
// ErsatzTV graphics canvas (`-i http://localhost:8409/internal/graphics/<channel>/<item>?...`):
//
// 1. the canvas is opened here and drained, so ErsatzTV still starts the VizRS
//    script element with the item's template data (commands, ads, EPG);
// 2. the sidecar takes that script session over (`GET /ffmpeg`): ErsatzTV gets
//    no more packets and its graphics engine waits at no cost;
// 3. ffmpeg reads raw yuva420p frames from the sidecar instead of the canvas
//    (`GET /ffmpeg/frames`), through a socket passed as `pipe:N`.
//
// ErsatzTV's own graphics elements of that item (text, image, watermarks in
// the canvas) are not drawn then. Whenever the sidecar does not answer, the
// original command runs: the item plays with ErsatzTV's graphics path.
//
// With VIZRS_FFMPEG_FPS every transcode to HLS runs at that rate (ErsatzTV
// takes the rate from the media): ErsatzTV's conversion of the media's rate
// (`[0:0]fps=25,…`, Normalize Framerate) converts to N instead, the video gets
// `fps=N` before the graphics overlay, so the graphics move at N frames per
// second, and the output `-r N`.
// With VIZRS_FFMPEG_PRESET a software encoder (libx264, libx265) without a
// preset gets `-preset:v <preset>`: ErsatzTV's next streaming engine leaves
// the preset of the FFmpeg profile out, so x264 runs at `medium`.
// With VIZRS_FFMPEG_READRATE_CATCHUP an HLS playlist input read at `-readrate`
// (ErsatzTV's MPEG-TS stream of a channel, read from its own HLS) gets
// `-readrate_catchup <rate>`: without it ffmpeg never makes up a stall at the
// live edge, so the stream falls further behind real time with every one.
// With VIZRS_FFMPEG_FPS_BLEND=1 the frame rate conversion of the media in a
// transcode to HLS (`[0:0]fps=25,…`, ErsatzTV's Normalize Framerate) blends
// neighbouring frames (`framerate=fps=25`): 60 or 30 fps media at 25 fps take
// every 2nd and 3rd frame in turn with `fps`, and motion judders. Frames that
// fall on an output frame (50 or 25 fps media) pass unchanged. Media no faster
// than the output keeps `fps` (its frames repeated, not blended): its rate
// comes from ffprobe, run in the background for the first piece of a media and
// cached for the next ones (through ErsatzTV's proxy a probe takes seconds).
//
//     VIZRS_CG_URL       sidecar, e.g. http://cg:7700 (empty: no overlay)
//     VIZRS_FFMPEG       real ffmpeg (default /usr/local/bin/ffmpeg)
//     VIZRS_FFMPEG_FPS   channel frame rate, e.g. 50 (empty: the media's rate)
//     VIZRS_FFMPEG_PRESET  x264/x265 preset, e.g. veryfast (empty: ErsatzTV's)
//     VIZRS_FFMPEG_READRATE_CATCHUP  catch-up rate of HLS inputs, e.g. 2 (empty: none)
//     VIZRS_FFMPEG_FPS_BLEND  1 = blend frames when converting the media's rate
//
// A static binary, so it runs in the ErsatzTV image of any architecture
// without extra packages. A canvas over https goes to plain ffmpeg.

use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::os::fd::AsRawFd;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use regex::Regex;

use crate::area::Area;
use crate::http::percent_decode;
use crate::json::{self, Value};
use crate::net::{Url, connect, error, read_head, status_of};
use crate::sys;

const DEFAULT_FFMPEG: &str = "/usr/local/bin/ffmpeg";
const CLAIM_WAIT_MS: u32 = 2000;
const SCRIPT_END: Duration = Duration::from_millis(500);
/// A probe without a result this long ago (failed, or still running) is run again.
const PROBE_RETRY: Duration = Duration::from_secs(60);
/// Read timeout of a probe of an http input, in microseconds.
const PROBE_TIMEOUT_US: &str = "20000000";
const CODEC: [&str; 3] = ["-vcodec", "-c:v", "-codec:v"];
const PRESET_ENCODERS: [&str; 2] = ["libx264", "libx265"];
const COPY: [(&str, &str); 4] = [("-c", "copy"), ("-c:v", "copy"), ("-vcodec", "copy"), ("-codec:v", "copy")];
// Options of the canvas input that do not apply to the raw frames.
const DROP_WITH_VALUE: [&str; 16] = [
    "-headers",
    "-protocol_whitelist",
    "-reconnect",
    "-reconnect_streamed",
    "-reconnect_delay_max",
    "-reconnect_on_network_error",
    "-rw_timeout",
    "-f",
    "-c:v",
    "-vcodec",
    "-codec:v",
    "-pix_fmt",
    "-video_size",
    "-s",
    "-framerate",
    "-r",
];
const FORWARDED: [i32; 4] = [sys::SIGTERM, sys::SIGINT, sys::SIGHUP, sys::SIGQUIT];

/// The overlay's environment.
pub struct Settings {
    pub ffmpeg: OsString,
    /// Sidecar URL without the trailing `/`; empty = no overlay.
    pub sidecar: String,
    /// Forced channel frame rate; empty = the media's.
    pub fps: String,
    /// Preset of software encoders without one; empty = ErsatzTV's.
    pub preset: String,
    /// Catch-up rate of HLS playlist inputs read at `-readrate`; empty = ffmpeg's.
    pub catchup: String,
    /// Blend frames in the media's frame rate conversion.
    pub blend: bool,
}

impl Settings {
    pub fn from_env() -> Settings {
        let var = |name| std::env::var(name).unwrap_or_default();
        Settings {
            ffmpeg: std::env::var_os("VIZRS_FFMPEG").unwrap_or_else(|| DEFAULT_FFMPEG.into()),
            sidecar: var("VIZRS_CG_URL").trim_end_matches('/').to_owned(),
            fps: var("VIZRS_FFMPEG_FPS").trim().to_owned(),
            preset: var("VIZRS_FFMPEG_PRESET").trim().to_owned(),
            catchup: var("VIZRS_FFMPEG_READRATE_CATCHUP").trim().to_owned(),
            blend: var("VIZRS_FFMPEG_FPS_BLEND").trim() == "1",
        }
    }
}

/// The canvas input of an ffmpeg command.
#[derive(Debug, PartialEq)]
pub struct Canvas {
    /// First option of the canvas input (after the previous `-i <input>`).
    pub start: usize,
    /// Index of its `-i`.
    pub index: usize,
    /// ErsatzTV channel from the canvas path.
    pub channel: String,
}

fn pair(args: &[OsString], i: usize, a: &str, b: &str) -> bool {
    args.get(i).is_some_and(|x| x == a) && args.get(i + 1).is_some_and(|y| y == b)
}

fn media_rate_filter() -> &'static Regex {
    static RATE: OnceLock<Regex> = OnceLock::new();
    // `fps=<rate>` first in a chain of an input's video: `[0:0]fps=25,scale…`.
    RATE.get_or_init(|| Regex::new(r"(^|;)(\[\d+(?::v)?(?::0)?\])fps=([0-9./]+)([,;\[])").expect("rate pattern"))
}

fn overlay_filter() -> &'static Regex {
    static OVERLAY: OnceLock<Regex> = OnceLock::new();
    // The first overlay of two labelled inputs: [video][graphics]overlay…
    OVERLAY.get_or_init(|| Regex::new(r"\[([^\]]+)\]\[([^\]]+)\](overlay\w*)").expect("overlay pattern"))
}

/// The filter with the graphics of input `input` (the canvas) overlaid at
/// `x`, `y`: their frames are the graphics area, not the whole channel. None
/// when that cannot be done safely: no overlay of that input, positional
/// overlay options, or the graphics resized on the way.
pub fn placed(filter: &str, input: usize, x: u32, y: u32) -> Option<String> {
    placed_areas(filter, input, &[(Area { x, y, w: 0, h: 0 }, 0)])
}

/// The filter with the graphics areas of input `input` overlaid each at its
/// place: one area as `placed`; several stacked in the frames (`Layout`:
/// area and its row) are split, cropped, prepared as ErsatzTV prepares the
/// graphics (`format`, `hwupload`…) and overlaid one after another.
pub fn placed_areas(filter: &str, input: usize, areas: &[(Area, u32)]) -> Option<String> {
    let inputs = [format!("[{input}]"), format!("[{input}:0]"), format!("[{input}:v]"), format!("[{input}:v:0]")];
    // The chain that prepares the graphics, `[1:0]format=yuva420p[v_s0]`, if any.
    let chains: Vec<&str> = filter.split(';').collect();
    let chain = chains.iter().position(|chain| inputs.iter().any(|label| chain.starts_with(label.as_str())));
    let (label, source, body) = match chain.map(|i| chains[i]) {
        Some(chain) if chain.contains("scale") => return None,
        Some(chain) => {
            let (rest, label) = chain.rsplit_once('[').filter(|(_, l)| l.ends_with(']'))?;
            let source = inputs.iter().find(|l| chain.starts_with(l.as_str()))?;
            (label.trim_end_matches(']').to_owned(), source.clone(), rest[source.len()..].to_owned())
        }
        None => {
            let source = inputs.iter().find(|label| filter.contains(label.as_str()))?;
            (source.trim_matches(['[', ']']).to_owned(), source.clone(), String::new())
        }
    };
    let pattern = format!(r"\[([^\]]+)\]\[{}\](overlay\w*)(=[^\[,;]*)?", regex::escape(&label));
    let found = Regex::new(&pattern).ok()?.captures(filter)?;
    let mut options = Vec::new();
    for option in found.get(3).map_or("", |m| &m.as_str()[1..]).split(':').filter(|o| !o.is_empty()) {
        let (name, _) = option.split_once('=')?;
        if name != "x" && name != "y" {
            options.push(option.to_owned());
        }
    }
    let overlay = |graphics: &str, area: Area| {
        let mut all = options.clone();
        all.extend([format!("x={}", area.x), format!("y={}", area.y)]);
        format!("[{graphics}]{}={}", &found[2], all.join(":"))
    };
    let whole = found.get(0)?;
    let (before, after) = (&filter[..whole.start()], &filter[whole.end()..]);
    if let [(area, _)] = areas {
        return Some(format!("{before}[{}]{}{after}", &found[1], overlay(&label, *area)));
    }

    // Several areas: the graphics split, each area cropped out and prepared.
    let n = areas.len();
    let split: String = (0..n).map(|i| format!("[vz_s{i}]")).collect();
    let mut prepare = vec![format!("{source}split={n}{split}")];
    for (i, (area, row)) in areas.iter().enumerate() {
        let crop = format!("[vz_s{i}]crop={}:{}:0:{row}", area.w, area.h);
        prepare.push(if body.is_empty() { format!("{crop}[vz_g{i}]") } else { format!("{crop},{body}[vz_g{i}]") });
    }
    // Overlaid one after another: the last one goes on as the original overlay did.
    let mut chained = format!("[{}]", &found[1]);
    for (i, (area, _)) in areas.iter().enumerate() {
        chained.push_str(&overlay(&format!("vz_g{i}"), *area));
        if i + 1 < n {
            chained.push_str(&format!("[vz_o{i}];[vz_o{i}]"));
        }
    }
    let rest = format!("{before}{chained}{after}");
    let kept: Vec<&str> = rest.split(';').enumerate().filter(|&(i, _)| Some(i) != chain).map(|(_, c)| c).collect();
    Some(prepare.into_iter().chain(kept.into_iter().map(str::to_owned)).collect::<Vec<_>>().join(";"))
}

/// `args` with the canvas input (`-i` number `input`) overlaid at `x`, `y`.
pub fn with_position(args: &[OsString], input: usize, x: u32, y: u32) -> Option<Vec<OsString>> {
    with_areas(args, input, &[(Area { x, y, w: 0, h: 0 }, 0)])
}

/// `args` with the graphics areas of the canvas input overlaid (`placed_areas`).
pub fn with_areas(args: &[OsString], input: usize, areas: &[(Area, u32)]) -> Option<Vec<OsString>> {
    // ErsatzTV Next gives audio and video a `-filter_complex` each.
    (0..args.len().saturating_sub(1)).filter(|&i| args[i] == "-filter_complex").find_map(|i| {
        let filter = placed_areas(args[i + 1].to_str()?, input, areas)?;
        let mut out = args.to_vec();
        out[i + 1] = filter.into();
        Some(out)
    })
}

/// Number of the input `-i` at `index` among the inputs of `args`.
pub fn input_number(args: &[OsString], index: usize) -> usize {
    (0..index).filter(|&i| args[i] == "-i").count()
}

/// A transcode to HLS at `fps`: the media's rate converted to `fps`, `fps`
/// before the overlay, `-r` on the output.
pub fn with_rate(args: Vec<OsString>, fps: &str) -> Vec<OsString> {
    let hls = (0..args.len()).find(|&i| pair(&args, i, "-f", "hls"));
    let copy = (0..args.len()).any(|i| COPY.iter().any(|(a, b)| pair(&args, i, a, b)));
    let (Some(hls), false, false) = (hls, fps.is_empty(), copy) else {
        return args;
    };

    let mut out = args;
    for i in 0..out.len() - 1 {
        if out[i] != "-filter_complex" {
            continue;
        }
        if let Some(filter) = out[i + 1].to_str() {
            let media = media_rate_filter().replace_all(filter, |m: &regex::Captures| format!("{}{}fps={fps}{}", &m[1], &m[2], &m[4]));
            let replaced = overlay_filter()
                .replacen(&media, 1, |m: &regex::Captures| format!("[{0}]fps={fps}[{0}_fps];[{0}_fps][{1}]{2}", &m[1], &m[2], &m[3]));
            out[i + 1] = replaced.into_owned().into();
        }
    }
    out.splice(hls..hls, ["-r".into(), fps.into()]);
    out
}

/// A transcode to HLS converts the media's frame rate blending frames:
/// `[0:0]fps=25,` → `[0:0]framerate=fps=25,`. Media of a known rate (`source`)
/// no faster than the output keeps `fps`: its frames are repeated, not blended.
pub fn with_blend(args: Vec<OsString>, blend: bool, source: Option<f64>) -> Vec<OsString> {
    let hls = (0..args.len()).any(|i| pair(&args, i, "-f", "hls"));
    if !blend || !hls {
        return args;
    }
    let mut out = args;
    for i in 0..out.len() - 1 {
        if out[i] != "-filter_complex" {
            continue;
        }
        if let Some(filter) = out[i + 1].to_str() {
            let replaced = media_rate_filter().replace_all(filter, |m: &regex::Captures| {
                let slower = source.zip(parse_rate(&m[3])).is_some_and(|(source, output)| source <= output * 1.001);
                if slower { m[0].to_owned() } else { format!("{}{}framerate=fps={}{}", &m[1], &m[2], &m[3], &m[4]) }
            });
            out[i + 1] = replaced.into_owned().into();
        }
    }
    out
}

/// A frame rate: `25`, `30000/1001`; None for `0/0` and other nonsense.
pub fn parse_rate(text: &str) -> Option<f64> {
    let (num, den) = text.trim().split_once('/').unwrap_or((text.trim(), "1"));
    let rate = num.parse::<f64>().ok()? / den.parse::<f64>().ok()?;
    (rate.is_finite() && rate > 0.0).then_some(rate)
}

/// The media input whose rate a transcode to HLS converts (`[0:0]fps=25,…`).
pub fn media_input(args: &[OsString]) -> Option<String> {
    (0..args.len()).find(|&i| pair(args, i, "-f", "hls"))?;
    let label = (0..args.len().saturating_sub(1))
        .filter(|&i| args[i] == "-filter_complex")
        .find_map(|i| media_rate_filter().captures(args[i + 1].to_str()?).map(|m| m[2].to_owned()))?;
    let number: usize = label.trim_start_matches('[').split([':', ']']).next()?.parse().ok()?;
    let index = (0..args.len()).filter(|&i| args[i] == "-i").nth(number)?;
    args.get(index + 1).map(|input| input.to_string_lossy().into_owned())
}

/// The video rate in ffprobe's `default=nw=1` output: the average one, else `r_frame_rate`.
pub fn probed_rate(text: &str) -> Option<f64> {
    let value = |key: &str| text.lines().find_map(|line| line.strip_prefix(key)).and_then(parse_rate);
    value("avg_frame_rate=").or_else(|| value("r_frame_rate="))
}

/// Video rate of the media at `input`, from the cache of earlier pieces. A miss
/// starts ffprobe (next to ffmpeg) in the background, writing the cache for the
/// next pieces, and gives None: through ErsatzTV's proxy a probe takes seconds,
/// too long for the start of a piece.
fn source_rate(settings: &Settings, input: &str) -> Option<f64> {
    let dir = std::env::temp_dir().join("vizrs-ffmpeg-rates");
    let key = input.split('?').next().unwrap_or(input);
    // FNV-1a: the input without its query (signatures, expiry) names the file.
    let hash = key.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3));
    let file = dir.join(format!("{hash:016x}"));
    if let Ok(text) = std::fs::read_to_string(&file) {
        if let Some(rate) = probed_rate(&text) {
            return Some(rate);
        }
        let age = std::fs::metadata(&file).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
        if age.is_some_and(|age| age < PROBE_RETRY) {
            return None;
        }
    }
    let _ = std::fs::create_dir_all(&dir);
    let out = std::fs::File::create(&file).ok()?;
    let mut probe = Command::new(Path::new(&settings.ffmpeg).with_file_name("ffprobe"));
    probe.args(["-v", "error"]);
    if input.starts_with("http") {
        probe.args(["-rw_timeout", PROBE_TIMEOUT_US]);
    }
    probe.args(["-select_streams", "v:0", "-show_entries", "stream=r_frame_rate,avg_frame_rate", "-of", "default=nw=1"]);
    let _ = probe.arg(input).stdin(Stdio::null()).stdout(out).stderr(Stdio::null()).spawn();
    None
}

/// A software video encoder without a preset gets `-preset:v <preset>`.
pub fn with_preset(args: Vec<OsString>, preset: &str) -> Vec<OsString> {
    let has_preset = args.iter().any(|a| a.to_str().is_some_and(|a| a == "-preset" || a.starts_with("-preset:")));
    if preset.is_empty() || has_preset {
        return args;
    }
    let mut out = Vec::with_capacity(args.len() + 2);
    let mut previous: Option<&OsString> = None;
    for arg in &args {
        out.push(arg.clone());
        if previous.is_some_and(|p| CODEC.iter().any(|c| p == c)) && PRESET_ENCODERS.iter().any(|e| arg == e) {
            out.extend(["-preset:v".into(), preset.into()]);
        }
        previous = Some(arg);
    }
    out
}

/// An HLS playlist input read at `-readrate` without a catch-up rate gets
/// `-readrate_catchup <rate>` after its `-readrate`.
pub fn with_catchup(args: Vec<OsString>, rate: &str) -> Vec<OsString> {
    if rate.is_empty() {
        return args;
    }
    let mut out = Vec::with_capacity(args.len() + 2);
    // Index in `out` after the value of the pending input's `-readrate`.
    let mut readrate: Option<usize> = None;
    let mut has_catchup = false;
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-i" && i + 1 < args.len() {
            let playlist = args[i + 1].to_str().is_some_and(|input| input.split('?').next().is_some_and(|path| path.ends_with(".m3u8")));
            if let (Some(at), false, true) = (readrate, has_catchup, playlist) {
                out.splice(at..at, ["-readrate_catchup".into(), rate.into()]);
            }
            out.extend([arg.clone(), args[i + 1].clone()]);
            (readrate, has_catchup) = (None, false);
            i += 2;
            continue;
        }
        out.push(arg.clone());
        if arg == "-readrate" && i + 1 < args.len() {
            out.push(args[i + 1].clone());
            readrate = Some(out.len());
            i += 2;
            continue;
        }
        has_catchup |= arg == "-readrate_catchup";
        i += 1;
    }
    out
}

/// The command with the forced rate, blending, preset and catch-up rate, when set.
fn adjusted(args: Vec<OsString>, settings: &Settings) -> Vec<OsString> {
    let rated = with_rate(args, &settings.fps);
    let source = if settings.blend { media_input(&rated).and_then(|input| source_rate(settings, &input)) } else { None };
    let blended = with_blend(rated, settings.blend, source);
    with_catchup(with_preset(blended, &settings.preset), &settings.catchup)
}

/// `/internal/graphics/<channel>/<item>`: the channel, still percent-encoded.
fn canvas_channel(path: &str) -> Option<&str> {
    let (channel, item) = path.strip_prefix("/internal/graphics/")?.split_once('/')?;
    let valid = !channel.is_empty() && !item.is_empty() && item.bytes().all(|b| b.is_ascii_digit());
    valid.then_some(channel)
}

/// `urllib.parse.unquote`: `%XX` only, `+` stays.
fn unquote(text: &str) -> String {
    percent_decode(&text.replace('+', "%2B"))
}

/// The canvas input of `args`, if any.
pub fn find_canvas(args: &[OsString]) -> Option<Canvas> {
    let mut start = 0;
    for i in 0..args.len().saturating_sub(1) {
        if args[i] != "-i" {
            continue;
        }
        let url = args[i + 1].to_str().and_then(Url::split);
        if let Some((url, channel)) = url.and_then(|u| canvas_channel(u.path).map(|c| (u, c))).filter(|(u, _)| u.scheme == "http") {
            // A region canvas only carries watermarks: no VizRS element on the item.
            let region = url.query.split('&').any(|p| p.split_once('=').is_some_and(|(k, v)| unquote(k) == "region" && !v.is_empty()));
            if region {
                return None;
            }
            return Some(Canvas { start, index: i, channel: unquote(channel) });
        }
        start = i + 2;
    }
    None
}

/// Headers of the `-headers` options (`name: value` lines).
pub fn parse_headers(options: &[OsString]) -> Vec<(String, String)> {
    let mut headers: Vec<(String, String)> = Vec::new();
    for i in 0..options.len().saturating_sub(1) {
        if options[i] != "-headers" {
            continue;
        }
        for line in options[i + 1].to_string_lossy().split('\n') {
            let Some((name, value)) = line.trim_end_matches('\r').split_once(':') else { continue };
            let (name, value) = (name.trim(), value.trim());
            if name.is_empty() {
                continue;
            }
            match headers.iter_mut().find(|(n, _)| n == name) {
                Some(header) => header.1 = value.to_owned(),
                None => headers.push((name.to_owned(), value.to_owned())),
            }
        }
    }
    headers
}

/// What the sidecar gives the overlay (`GET /ffmpeg`), with the rate decided.
pub struct Frames {
    pub width: u32,
    pub height: u32,
    pub rate: String,
    /// ffmpeg's name of the colour matrix of the frames; empty = not declared.
    pub matrix: String,
    /// Graphics areas of the channel and their rows in the frames (stacked);
    /// empty = the frames are the whole channel.
    pub areas: Vec<(Area, u32)>,
}

/// The command with the canvas input replaced by raw frames from `pipe:<fd>`.
pub fn overlay_args(args: &[OsString], canvas: &Canvas, fd: i32, frames: &Frames) -> Vec<OsString> {
    let options = &args[canvas.start..canvas.index];
    let mut out: Vec<OsString> = args[..canvas.start].to_vec();
    let mut i = 0;
    while i < options.len() {
        if i + 1 < options.len() && DROP_WITH_VALUE.iter().any(|o| options[i] == *o) {
            i += 2;
            continue;
        }
        out.push(options[i].clone());
        i += 1;
    }
    let size = format!("{}x{}", frames.width, frames.height);
    let raw = ["-f", "rawvideo", "-pix_fmt", "yuva420p", "-video_size", &size, "-framerate", &frames.rate];
    out.extend(raw.iter().map(OsString::from));
    // Undeclared, the overlay negotiates an unknown colour space for the whole
    // chain and swscale sends every scaled frame of the media through RGB.
    if !frames.matrix.is_empty() {
        out.extend(["-colorspace", &frames.matrix, "-color_range", "tv"].iter().map(OsString::from));
    }
    out.extend(["-i".into(), format!("pipe:{fd}").into()]);
    out.extend_from_slice(&args[canvas.index + 2..]);
    // Frames of graphics areas: each overlaid at its place (checked by `with_position` beforehand).
    let at_origin = matches!(frames.areas.as_slice(), [(a, _)] if (a.x, a.y) == (0, 0));
    if !frames.areas.is_empty() && !at_origin {
        out = with_areas(&out, input_number(args, canvas.index), &frames.areas).unwrap_or(out);
    }
    out
}

/// `application/x-www-form-urlencoded` value, as `urllib.parse.urlencode`.
fn quote_plus(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' => (b as char).to_string(),
            b' ' => "+".to_owned(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn log(message: impl std::fmt::Display) {
    let _ = writeln!(io::stderr(), "vizrs-ffmpeg: {message}");
}

/// Reads the ErsatzTV canvas and throws it away until closed.
#[derive(Clone, Default)]
struct Drain {
    socket: Arc<Mutex<Option<TcpStream>>>,
    closed: Arc<Mutex<bool>>,
}

impl Drain {
    fn start(url: String, headers: Vec<(String, String)>) -> Drain {
        let drain = Drain::default();
        let this = drain.clone();
        std::thread::spawn(move || {
            let _ = this.run(&url, &headers);
        });
        drain
    }

    fn run(&self, url: &str, headers: &[(String, String)]) -> io::Result<()> {
        let url = Url::split(url).ok_or_else(|| error("bad canvas URL".into()))?;
        let mut stream = connect(&url)?;
        {
            let closed = self.closed.lock().unwrap_or_else(|e| e.into_inner());
            if *closed {
                return Ok(());
            }
            *self.socket.lock().unwrap_or_else(|e| e.into_inner()) = Some(stream.try_clone()?);
        }
        let mut request = format!("GET {} HTTP/1.1\r\nHost: {}\r\nAccept-Encoding: identity\r\n", url.target(), url.netloc);
        for (name, value) in headers {
            request.push_str(&format!("{name}: {value}\r\n"));
        }
        request.push_str("\r\n");
        stream.write_all(request.as_bytes())?;
        read_head(&mut stream, "canvas")?;
        stream.set_read_timeout(None)?;
        let mut buffer = vec![0u8; 1 << 16];
        while stream.read(&mut buffer)? > 0 {}
        Ok(())
    }

    fn close(&self) {
        let mut closed = self.closed.lock().unwrap_or_else(|e| e.into_inner());
        *closed = true;
        if let Some(socket) = self.socket.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let _ = socket.shutdown(Shutdown::Both);
        }
    }
}

/// `GET path` on the sidecar: status and body.
fn sidecar_get(sidecar: &str, path: &str) -> io::Result<(String, Vec<u8>)> {
    let url = Url::split(sidecar).ok_or_else(|| error(format!("bad sidecar URL {sidecar}")))?;
    let mut stream = connect(&url)?;
    stream.write_all(format!("GET {path} HTTP/1.0\r\nHost: {}\r\n\r\n", url.netloc).as_bytes())?;
    let mut reply = Vec::new();
    stream.read_to_end(&mut reply)?;
    let end = reply.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| error("sidecar: no reply".into()))?;
    let head = String::from_utf8_lossy(&reply[..end]);
    let status = status_of(head.lines().next().unwrap_or("")).to_owned();
    Ok((status, reply[end + 4..].to_vec()))
}

/// Socket positioned at the body of the sidecar's frame stream.
fn open_frames(sidecar: &str, path: &str) -> io::Result<TcpStream> {
    let url = Url::split(sidecar).ok_or_else(|| error(format!("bad sidecar URL {sidecar}")))?;
    let mut stream = connect(&url)?;
    stream.write_all(format!("GET {path} HTTP/1.0\r\nHost: {}\r\n\r\n", url.netloc).as_bytes())?;
    let line = read_head(&mut stream, "frame stream")?;
    if status_of(&line) != "200" {
        return Err(error(format!("frame stream: {line}")));
    }
    stream.set_read_timeout(None)?;
    stream.set_write_timeout(None)?;
    // ffmpeg blocks on this socket: a sidecar gone without closing it must not stop the channel.
    sys::keepalive(stream.as_raw_fd(), 3, 2, 4)?;
    Ok(stream)
}

/// The sidecar's session for the canvas channel and its frame stream. With
/// `place` (the command can overlay the graphics anywhere) the frames are
/// those of the graphics area, if the channel has one.
fn take_over(settings: &Settings, channel: &str, header_rate: &str, place: bool) -> io::Result<(Frames, TcpStream)> {
    let area = if place { "&area=1" } else { "" };
    let path = format!("/ffmpeg?channel={}&wait={CLAIM_WAIT_MS}{area}", quote_plus(channel));
    let (status, body) = sidecar_get(&settings.sidecar, &path)?;
    if status != "200" {
        return Err(error(format!("no VizRS session on channel {channel} ({status})")));
    }
    let info = json::parse(&String::from_utf8_lossy(&body)).map_err(|e| error(e.to_string()))?;
    let field = |key| info.get(key).filter(|v| !v.is_null()).ok_or_else(|| error(format!("'{key}'")));
    let size = |key| field(key).and_then(|v| v.as_f64().map(|n| n as u32).ok_or_else(|| error(format!("'{key}'"))));
    let rate = match (settings.fps.as_str(), header_rate) {
        ("", "") => field("fps")?.to_string(),
        ("", rate) => rate.to_owned(),
        (fps, _) => fps.to_owned(),
    };
    let session = match field("session")? {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    let mut areas = Vec::new();
    if place {
        // `areas` (stacked in the frames); an older sidecar sends only `x`, `y`.
        let number = |value: &Value, key: &str| {
            value.get(key).and_then(Value::as_f64).map(|n| n as u32).ok_or_else(|| error(format!("'areas.{key}'")))
        };
        let mut row = 0;
        for area in info.get("areas").and_then(Value::as_array).unwrap_or(&[]) {
            let area = Area { x: number(area, "x")?, y: number(area, "y")?, w: number(area, "w")?, h: number(area, "h")? };
            areas.push((area, row));
            row += area.h;
        }
        if areas.is_empty() {
            areas.push((Area { x: size("x")?, y: size("y")?, w: size("width")?, h: size("height")? }, 0));
        }
    }
    let matrix = info.get("matrix").and_then(Value::as_str).unwrap_or_default().to_owned();
    let frames = Frames { width: size("width")?, height: size("height")?, rate, matrix, areas };
    let path = format!("/ffmpeg/frames?session={}&fps={}{area}", quote_plus(&session), quote_plus(&frames.rate));
    Ok((frames, open_frames(&settings.sidecar, &path)?))
}

/// Replaces this process with the real ffmpeg (at the forced rate, preset and catch-up rate, if any).
fn plain(settings: &Settings, args: Vec<OsString>) -> ! {
    let error = Command::new(&settings.ffmpeg).args(adjusted(args, settings)).exec();
    log(format_args!("{}: {error}", settings.ffmpeg.to_string_lossy()));
    std::process::exit(127)
}

/// The overlay: `vizrs-ffmpeg <ffmpeg arguments>`; returns the exit code.
pub fn run(settings: &Settings, args: Vec<OsString>) -> i32 {
    // Not a canvas transcode: plain ffmpeg, at the forced rate, preset and catch-up rate when set.
    let Some(canvas) = find_canvas(&args).filter(|_| !settings.sidecar.is_empty()) else { plain(settings, args) };

    let headers = parse_headers(&args[canvas.start..canvas.index]);
    let rate = headers.iter().find(|(name, _)| name == "x-etv-frame-rate").map_or("", |(_, v)| v.as_str()).to_owned();
    let drain = Drain::start(args[canvas.index + 1].to_string_lossy().into_owned(), headers);

    let place = with_position(&args, input_number(&args, canvas.index), 2, 2).is_some();
    let (frames, socket) = match take_over(settings, &canvas.channel, &rate, place) {
        Ok(taken) => taken,
        Err(e) => {
            log(format_args!("{e}; ErsatzTV graphics instead"));
            drain.close();
            plain(settings, args)
        }
    };

    let fd = socket.as_raw_fd();
    // Forwarded to ffmpeg from here on; ffmpeg_child_setup unblocks them in the child.
    sys::block_signals(&FORWARDED);
    let mut command = Command::new(&settings.ffmpeg);
    command.args(adjusted(overlay_args(&args, &canvas, fd, &frames), settings));
    // SAFETY: only async-signal-safe calls in the forked child.
    unsafe {
        command.pre_exec(move || sys::ffmpeg_child_setup(fd));
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            log(format_args!("{}: {e}", settings.ffmpeg.to_string_lossy()));
            drain.close();
            return 127;
        }
    };
    drop(socket);

    let pid = child.id();
    std::thread::spawn(move || {
        loop {
            sys::send_signal(pid, sys::wait_signal(&FORWARDED));
        }
    });

    let status = child.wait();
    // The sidecar ends the script when ffmpeg's frame stream closes; closing the
    // canvas before that makes ErsatzTV log a cancelled script element.
    std::thread::sleep(SCRIPT_END);
    drain.close();
    match status {
        Ok(status) => status.code().or_else(|| status.signal().map(|s| 128 + s)).unwrap_or(1),
        Err(_) => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn frames_declare_their_colour_matrix() {
        let input = args(&["-i", "media", "-i", "http://localhost/internal/graphics/4/1", "-f", "hls", "o"]);
        let canvas = Canvas { start: 2, index: 2, channel: "4".into() };
        let mut frames = Frames { width: 8, height: 4, rate: "25".into(), matrix: "bt709".into(), areas: vec![] };
        let raw = ["-i", "media", "-f", "rawvideo", "-pix_fmt", "yuva420p", "-video_size", "8x4", "-framerate", "25"];
        let tail = ["-i", "pipe:5", "-f", "hls", "o"];
        let declared = [&raw[..], &["-colorspace", "bt709", "-color_range", "tv"], &tail].concat();
        assert_eq!(overlay_args(&input, &canvas, 5, &frames), args(&declared));
        // An older sidecar names no matrix: nothing is declared.
        frames.matrix.clear();
        assert_eq!(overlay_args(&input, &canvas, 5, &frames), args(&[&raw[..], &tail].concat()));
    }

    #[test]
    fn canvas_input() {
        let list = args(&["-i", "/m/a.mp4", "-headers", "h", "-i", "http://x:1/internal/graphics/2%2E1/28750?exp=1", "-f", "null", "-"]);
        assert_eq!(find_canvas(&list), Some(Canvas { start: 2, index: 4, channel: "2.1".into() }));
        assert_eq!(find_canvas(&args(&["-i", "http://x/internal/graphics/1/7?region=a"])), None);
        assert!(find_canvas(&args(&["-i", "http://x/internal/graphics/1/7?region="])).is_some());
        assert_eq!(find_canvas(&args(&["-i", "http://x/internal/graphics/1/a"])), None);
        assert_eq!(find_canvas(&args(&["-i", "file:///internal/graphics/1/7"])), None);
        assert_eq!(find_canvas(&args(&["-version"])), None);
    }

    #[test]
    fn headers() {
        let list = args(&["-headers", "a: 1\r\nx-etv-frame-rate:25/1\nbad\n: v\r\n", "-headers", "a:2"]);
        assert_eq!(parse_headers(&list), vec![("a".into(), "2".into()), ("x-etv-frame-rate".into(), "25/1".into())]);
    }

    #[test]
    fn rate_only_on_hls_transcodes() {
        let hls = args(&["-i", "a", "-filter_complex", "[0:0]x[v];[v][1:0]overlay_cuda=x=0[o];[o][2]overlay", "-f", "hls", "o.m3u8"]);
        assert_eq!(
            with_rate(hls.clone(), "50"),
            args(&[
                "-i",
                "a",
                "-filter_complex",
                "[0:0]x[v];[v]fps=50[v_fps];[v_fps][1:0]overlay_cuda=x=0[o];[o][2]overlay",
                "-r",
                "50",
                "-f",
                "hls",
                "o.m3u8"
            ])
        );
        assert_eq!(with_rate(hls.clone(), ""), hls);
        let copy = args(&["-i", "a", "-c:v", "copy", "-f", "hls", "o"]);
        assert_eq!(with_rate(copy.clone(), "50"), copy);
        // ErsatzTV's conversion of the media's rate converts to the channel's.
        let normalized = args(&["-i", "a", "-filter_complex", "[0:0]fps=25,scale=8:4[v];[v][1:0]overlay[o]", "-f", "hls", "o"]);
        assert_eq!(with_rate(normalized, "50")[3], "[0:0]fps=50,scale=8:4[v];[v]fps=50[v_fps];[v_fps][1:0]overlay[o]");
    }

    #[test]
    fn media_rate_and_input() {
        assert_eq!(parse_rate("30000/1001").map(|r| (r * 1000.0).round()), Some(29970.0));
        assert_eq!(parse_rate("50"), Some(50.0));
        assert_eq!(parse_rate("0/0"), None);
        assert_eq!(parse_rate("x"), None);
        assert_eq!(probed_rate("r_frame_rate=60/1\navg_frame_rate=60/1\n"), Some(60.0));
        assert_eq!(probed_rate("r_frame_rate=30/1\navg_frame_rate=0/0\n"), Some(30.0));
        assert_eq!(probed_rate(""), None);
        let hls = args(&[
            "-ss",
            "1s",
            "-i",
            "http://m/1?sig=a",
            "-i",
            "pipe:4",
            "-filter_complex",
            "[0:1]loudnorm[a]",
            "-filter_complex",
            "[0:0]fps=25,scale=8:4[v]",
            "-f",
            "hls",
            "o",
        ]);
        assert_eq!(media_input(&hls).as_deref(), Some("http://m/1?sig=a"));
        let second = args(&["-i", "a", "-i", "b", "-filter_complex", "[1:v]fps=25[v]", "-f", "hls", "o"]);
        assert_eq!(media_input(&second).as_deref(), Some("b"));
        assert_eq!(media_input(&args(&["-i", "a", "-filter_complex", "[0:0]fps=25[v]", "-f", "mpegts", "o"])), None);
        assert_eq!(media_input(&args(&["-i", "a", "-filter_complex", "[0:0]scale=8:4[v]", "-f", "hls", "o"])), None);
    }

    #[test]
    fn blend_only_the_media_rate_of_hls_transcodes() {
        let filter = "[0:0]fps=25,scale=1600:900:flags=bicubic,setsar=1[v_m0];[1:0]format=yuva420p[v_s0];[v_m0]fps=25[v_m0_fps];[v_m0_fps][v_s0]overlay=x=0:y=0[v_o0]";
        let hls = args(&["-i", "a", "-filter_complex", "[0:1]loudnorm[a]", "-filter_complex", filter, "-f", "hls", "o.m3u8"]);
        let blended = with_blend(hls.clone(), true, None);
        assert_eq!(blended[3], "[0:1]loudnorm[a]");
        assert_eq!(
            blended[5],
            "[0:0]framerate=fps=25,scale=1600:900:flags=bicubic,setsar=1[v_m0];[1:0]format=yuva420p[v_s0];[v_m0]fps=25[v_m0_fps];[v_m0_fps][v_s0]overlay=x=0:y=0[v_o0]"
        );
        assert_eq!(with_blend(hls.clone(), false, None), hls);
        // Faster media blends; media no faster than the output repeats its frames.
        assert_eq!(with_blend(hls.clone(), true, Some(30.0)), blended);
        assert_eq!(with_blend(hls.clone(), true, Some(25.0)), hls);
        let fifty = args(&["-i", "a", "-filter_complex", "[0:0]fps=50,scale=8:4[v]", "-f", "hls", "o"]);
        assert_eq!(with_blend(fifty.clone(), true, Some(25.0)), fifty);
        assert_eq!(with_blend(fifty.clone(), true, Some(60.0))[3], "[0:0]framerate=fps=50,scale=8:4[v]");
        // Not a transcode to HLS, and an fps with options.
        let other = args(&["-i", "a", "-filter_complex", "[0:0]fps=25,scale=8:4[v]", "-f", "mpegts", "o"]);
        assert_eq!(with_blend(other.clone(), true, None), other);
        let options = args(&["-i", "a", "-filter_complex", "[0:v]fps=25:round=up[v]", "-f", "hls", "o"]);
        assert_eq!(with_blend(options.clone(), true, None), options);
        let label = args(&["-i", "a", "-filter_complex", "[0:v]fps=30000/1001[v]", "-f", "hls", "o"]);
        assert_eq!(with_blend(label.clone(), true, None)[3], "[0:v]framerate=fps=30000/1001[v]");
        assert_eq!(with_blend(label.clone(), true, Some(29.97)), label);
    }

    #[test]
    fn preset_only_for_software_encoders_without_one() {
        let x264 = args(&["-i", "a", "-vcodec", "libx264", "-b:v", "1500k", "-f", "hls", "o"]);
        assert_eq!(
            with_preset(x264.clone(), "superfast"),
            args(&["-i", "a", "-vcodec", "libx264", "-preset:v", "superfast", "-b:v", "1500k", "-f", "hls", "o"])
        );
        assert_eq!(with_preset(x264.clone(), ""), x264);
        let x265 = args(&["-i", "a", "-c:v", "libx265", "o"]);
        assert_eq!(with_preset(x265, "veryfast"), args(&["-i", "a", "-c:v", "libx265", "-preset:v", "veryfast", "o"]));
        let own = args(&["-i", "a", "-vcodec", "libx264", "-preset:v", "medium", "o"]);
        assert_eq!(with_preset(own.clone(), "superfast"), own);
        let nvenc = args(&["-i", "a", "-vcodec", "h264_nvenc", "o"]);
        assert_eq!(with_preset(nvenc.clone(), "superfast"), nvenc);
        let canvas = args(&["-f", "rawvideo", "-i", "pipe:0", "-c:v", "ffv1", "-f", "nut", "pipe:1"]);
        assert_eq!(with_preset(canvas.clone(), "superfast"), canvas);
    }

    #[test]
    fn catchup_only_for_hls_inputs_at_a_readrate() {
        let hybrid = "http://localhost:8409/iptv/channel/4.m3u8?mode=segmenter&subtitles=false";
        let reader =
            args(&["-nostdin", "-readrate", "1.0", "-readrate_initial_burst", "6", "-i", hybrid, "-c", "copy", "-f", "mpegts", "pipe:1"]);
        assert_eq!(
            with_catchup(reader.clone(), "2"),
            args(&[
                "-nostdin",
                "-readrate",
                "1.0",
                "-readrate_catchup",
                "2",
                "-readrate_initial_burst",
                "6",
                "-i",
                hybrid,
                "-c",
                "copy",
                "-f",
                "mpegts",
                "pipe:1"
            ])
        );
        assert_eq!(with_catchup(reader.clone(), ""), reader);
        let own = args(&["-readrate", "1.0", "-readrate_catchup", "1.5", "-i", "a.m3u8", "-f", "mpegts", "o"]);
        assert_eq!(with_catchup(own.clone(), "2"), own);
        // The transcode of a media file, and a playlist read without a readrate.
        let media =
            args(&["-readrate", "1.0", "-i", "http://localhost:8409/internal/media/1?sig=x", "-i", "b.m3u8", "-f", "hls", "o.m3u8"]);
        assert_eq!(with_catchup(media.clone(), "2"), media);
        let second = args(&["-i", "a.mp4", "-readrate", "1", "-i", "b.m3u8", "o"]);
        assert_eq!(with_catchup(second, "2"), args(&["-i", "a.mp4", "-readrate", "1", "-readrate_catchup", "2", "-i", "b.m3u8", "o"]));
    }

    #[test]
    fn graphics_placed_at_the_area() {
        // ErsatzTV's software, NVIDIA and VAAPI graphs, the canvas as input 1.
        let cases = [
            ("[0:0]scale=8:4[v_m0];[1:0]format=yuva420p[v_s0];[v_m0][v_s0]overlay=x=0:y=0[v_o0]", "[v_m0][v_s0]overlay=x=10:y=940[v_o0]"),
            (
                "[1:0]format=yuva420p,hwupload_cuda[v_s0];[v_m0][v_s0]overlay_cuda=x=0:y=0[v_o0]",
                "[v_m0][v_s0]overlay_cuda=x=10:y=940[v_o0]",
            ),
            (
                "[1:0]format=yuva420p[v_s0];[v_m0][v_s0]overlay=format=0,format=nv12|vaapi,hwupload[v_o0]",
                "[v_m0][v_s0]overlay=format=0:x=10:y=940,format",
            ),
            ("[1:0]format=yuva420p,hwupload[v_s0];[v_m0][v_s0]overlay_vaapi[v_o0]", "[v_m0][v_s0]overlay_vaapi=x=10:y=940[v_o0]"),
            ("[0][1]overlay", "[0][1]overlay=x=10:y=940"),
            // A watermark (input 2) overlaid first: only the graphics move.
            (
                "[0:0][2:0]overlay=x=5:y=5[w];[1:0]format=yuva420p[g];[w][g]overlay[o]",
                "[0:0][2:0]overlay=x=5:y=5[w];[1:0]format=yuva420p[g];[w][g]overlay=x=10:y=940[o]",
            ),
        ];
        for (filter, expected) in cases {
            let placed = placed(filter, 1, 10, 940).unwrap();
            assert!(placed.contains(expected), "{placed}");
            assert_eq!(placed.matches("overlay").count(), filter.matches("overlay").count());
        }
        // Positional options, graphics scaled to the channel, no overlay of the canvas: not placed.
        assert_eq!(placed("[0][1]overlay=5:5", 1, 10, 940), None);
        assert_eq!(placed("[1:0]scale=1920:1080,format=yuva420p[g];[0][g]overlay", 1, 10, 940), None);
        assert_eq!(placed("[0]scale=8:4[v]", 1, 10, 940), None);
        assert_eq!(placed("[0][2]overlay", 1, 10, 940), None);

        // Two areas stacked: split, crop, prepare each, overlay one after another.
        let (ticker, logo) = (Area { x: 0, y: 930, w: 1920, h: 140 }, Area { x: 1760, y: 40, w: 140, h: 140 });
        let two = [(ticker, 0), (logo, 140)];
        assert_eq!(
            placed_areas("[0:0]scale=8:4[v_m0];[1:0]format=yuva420p,hwupload_cuda[v_s0];[v_m0][v_s0]overlay_cuda=x=0:y=0[v_o0]", 1, &two)
                .unwrap(),
            "[1:0]split=2[vz_s0][vz_s1];[vz_s0]crop=1920:140:0:0,format=yuva420p,hwupload_cuda[vz_g0];\
             [vz_s1]crop=140:140:0:140,format=yuva420p,hwupload_cuda[vz_g1];[0:0]scale=8:4[v_m0];\
             [v_m0][vz_g0]overlay_cuda=x=0:y=930[vz_o0];[vz_o0][vz_g1]overlay_cuda=x=1760:y=40[v_o0]"
        );
        assert_eq!(
            placed_areas("[0][1]overlay=format=0,format=nv12[o]", 1, &two).unwrap(),
            "[1]split=2[vz_s0][vz_s1];[vz_s0]crop=1920:140:0:0[vz_g0];[vz_s1]crop=140:140:0:140[vz_g1];\
             [0][vz_g0]overlay=format=0:x=0:y=930[vz_o0];[vz_o0][vz_g1]overlay=format=0:x=1760:y=40,format=nv12[o]"
        );
        assert_eq!(placed_areas("[1:0]scale=1920:1080[g];[0][g]overlay", 1, &two), None);

        // ErsatzTV Next: audio in a graph of its own, before the video's.
        let next = args(&[
            "-i",
            "a",
            "-i",
            "http://x/internal/graphics/4/7",
            "-filter_complex",
            "[0:1]loudnorm=I=-16[a]",
            "-filter_complex",
            "[0:0]fps=25[v_m0];[1:0]format=yuva420p[v_s0];[v_m0][v_s0]overlay=x=0:y=0:format=0[v_o0]",
            "-f",
            "hls",
            "o",
        ]);
        let moved = with_areas(&next, 1, &two).unwrap();
        assert_eq!(moved[5], next[5]);
        assert!(moved[7].to_str().unwrap().contains("[vz_o0][vz_g1]overlay=format=0:x=1760:y=40[v_o0]"), "{:?}", moved[7]);

        let list = args(&["-i", "a", "-i", "http://x/internal/graphics/1/7", "-filter_complex", "[0][1]overlay[o]", "-f", "hls", "o"]);
        assert_eq!(input_number(&list, 2), 1);
        assert_eq!(with_position(&list, 1, 2, 4).unwrap()[5], "[0][1]overlay=x=2:y=4[o]");
        assert_eq!(with_position(&args(&["-i", "a", "-f", "null", "-"]), 1, 2, 4), None);
    }

    #[test]
    fn form_values() {
        assert_eq!(quote_plus("1 2/ä"), "1+2%2F%C3%A4");
        assert_eq!(unquote("a+b%2E"), "a+b.");
    }
}
