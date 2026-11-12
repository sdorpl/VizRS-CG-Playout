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
//     VIZRS_FFMPEG_X264_PARAMS  libx264 options, e.g. bframes=0:subme=0 (empty: none)
//     VIZRS_FFMPEG_SETTINGS  encoder settings set in VizRS Live (x264 preset, H.264 profile,
//                        x264 options, NVENC and QSV presets, hardware B-frames); each key
//                        the file has instead of the variables
//                        (default /config/vizrs-live/encoder.json, src/encoder.rs)
//     VIZRS_FFMPEG_READRATE_CATCHUP  catch-up rate of HLS inputs, e.g. 2 (empty: none)
//     VIZRS_FFMPEG_FPS_BLEND  1 = blend frames when converting the media's rate
//     VIZRS_FFMPEG_INGEST  1 = live ingest of the sidecar takes the channel over
//     VIZRS_FFMPEG_STANDBY image of a live programme without its source (empty: black)
//     VIZRS_FFMPEG_CUDA  encode = with an NVENC encoder, decoding and filters on the CPU;
//                        overlay = decoding on the CPU, filters and overlay on the card;
//                        auto = overlay while the card has VIZRS_FFMPEG_CUDA_MIN_FREE MiB free, encode below
//     VIZRS_FFMPEG_CUDA_MIN_FREE  free GPU memory in MiB for overlay in auto (default 400)
//     VIZRS_FFMPEG_NVENC_BF  B-frames of hardware encoders (NVENC, QSV, VAAPI), e.g. 0 (empty: ffmpeg's)
//     VIZRS_FFMPEG_SYNC_RAM  1 = with CUDA overlay, media frames wait for the graphics in RAM, not on the card
//
// The graphics of a canvas transcode with `-init_hw_device cuda` go to the
// card through `hwupload` (that device), not `hwupload_cuda`: each
// `hwupload_cuda` opens a CUDA context of its own, about 28 MiB of GPU memory
// per graphics area. With VIZRS_FFMPEG_NVENC_BF an NVENC encoder (`*_nvenc`)
// without `-bf` gets `-bf:v <n>`: ffmpeg's default B-frames hold reference
// frames on the card (Quadro P400, 1080p50: 139 MiB with the default, 99 with
// 1, 79 with 0).
//
// With VIZRS_FFMPEG_CUDA=encode a command that encodes with NVENC (`*_nvenc`)
// loses ErsatzTV's CUDA decoding and filters (`-hwaccel cuda`,
// `-init_hw_device cuda`, `hwupload`, `scale_cuda`, `overlay_cuda`, `pad_cuda`,
// `yadif_cuda`, `bwdif_cuda` become their CPU filters): only the encoder stays
// on the card. A process then takes about half the GPU memory (720p: 118 MiB
// instead of 218 MiB), for some CPU – on a card shared with a desktop, two
// pieces of a channel at once (one ending, the next starting) no longer run
// out of memory (`CUDA_ERROR_OUT_OF_MEMORY`).
//
// With VIZRS_FFMPEG_CUDA=overlay only ErsatzTV's CUDA decoding goes (`-hwaccel
// cuda` and its options): the decoded frames are uploaded (`format=…,hwupload`)
// where ErsatzTV's filters expect them on the card, scaling and the overlay of
// the graphics (`overlay_cuda`) stay there, and the frames for the encoder come
// back to the CPU (`hwdownload`): waiting for the muxer (3 s of `loudnorm`
// lookahead) on the card they ran it out of memory. A canvas transcode then
// reads the media at `-readrate 1` when ErsatzTV gives it no rate
// (`paced_media`), with an initial burst of the graphics the sidecar has
// ready at once (`ahead` of `/ffmpeg`): the graphics come in the channel's
// time anyway, media decoded ahead of them waited on the card in front of
// `overlay_cuda`, and without the burst a piece started late never made up
// the time lost (HLS at 0.93x, the player's buffer ran out). Measured on a Quadro P400
// (720p25 media to 1080p50 with a graphics strip): 0.33 of a core instead of
// 1.40 with `encode`, about the same GPU memory (213 MiB instead of 206).
//
// With VIZRS_FFMPEG_SYNC_RAM=1 and `overlay` (also chosen by `auto`) the
// media frames wait for the graphics of their moment in RAM, before they go
// to the card (`media_waits_in_ram`): `loudnorm` gives its audio about 3 s
// late, so ffmpeg decodes the media that far ahead, and each frame uploaded
// at once waited in front of `overlay_cuda` for graphics that come in the
// channel's time – about 3 MiB a 1080p frame, up to 560 MiB more on the card,
// and a burst ran a Quadro P400 out of memory. Measured (1080p50, loudnorm,
// graphics from a pipe): 91 MiB instead of 667, with or without `-readrate`
// and its burst; the frames take that RAM instead (+430 MB RSS) and the sync
// about 0.09 of a core (PLAN-p400, E9).
//
// With VIZRS_FFMPEG_CUDA=auto a command encoding with NVENC and CUDA asks
// `nvidia-smi` for the free memory of the card first (at most 1 s): with at
// least VIZRS_FFMPEG_CUDA_MIN_FREE MiB it runs as `overlay`, below as `encode`
// (half the GPU memory, more CPU) – a card shared with a desktop that filled
// it no longer ends a piece with `CUDA_ERROR_OUT_OF_MEMORY` and the
// fallback slate. Without `nvidia-smi`, or when it fails, `overlay`.
//
// With VIZRS_FFMPEG_INGEST=1 a transcode to HLS of a channel whose live
// source is on (sidecar `/ingest/state`) plays that source in its place
// (src/takeover.rs), with the graphics overlaid as on the schedule.
// The same runs widen the graphics zones of `area: auto` in the middle of a
// piece (PLAN-strefy S3b): the overlay follows `/ffmpeg/zones` of its session
// and, when they widen, stops the run (SIGINT) and starts the next from where
// the piece is, with a new frame stream of the session in the new zones.
//
// A static binary, so it runs in the ErsatzTV image of any architecture
// without extra packages. A canvas over https goes to plain ffmpeg.

use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::Duration;

use regex::Regex;

use crate::area::Area;
use crate::encoder::{self, Encoder};
use crate::http::percent_decode;
use crate::json::{self, Value};
use crate::net::{Url, connect, error, read_head, read_head_text, status_of};
use crate::sys;
use crate::takeover::{self, Job, Sink, SinkControl};

const DEFAULT_FFMPEG: &str = "/usr/local/bin/ffmpeg";
const CLAIM_WAIT_MS: u32 = 2000;
const SCRIPT_END: Duration = Duration::from_millis(500);
const RECONNECT: Duration = Duration::from_secs(1);
/// A probe without a result this long ago (failed, or still running) is run again.
/// How often the overlay asks for the zones of its session (as `takeover` for the live state).
const ZONES_POLL: Duration = Duration::from_millis(100);
const DEFAULT_CUDA_MIN_FREE: u64 = 400;
const GPU_QUERY_WAIT: Duration = Duration::from_secs(1);
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
    /// H.264 profile of libx264 without one (`-profile:v`); only from VizRS Live, empty = x264's.
    pub profile: String,
    /// Options of libx264 (`-x264-params`); empty = none.
    pub x264_params: String,
    /// Catch-up rate of HLS playlist inputs read at `-readrate`; empty = ffmpeg's.
    pub catchup: String,
    /// Blend frames in the media's frame rate conversion.
    pub blend: bool,
    /// Live ingest of the sidecar takes transcodes to HLS over (src/takeover.rs).
    pub ingest: bool,
    /// Picture of a live programme without its source; empty = black.
    pub standby: String,
    /// What of an NVENC command stays on the card (VIZRS_FFMPEG_CUDA).
    pub cuda: Cuda,
    /// Preset of NVENC encoders without one (p1–p7); only from VizRS Live, empty = ffmpeg's (p4).
    pub nvenc_preset: String,
    /// Preset of QSV encoders without one; only from VizRS Live, empty = the driver's.
    pub qsv_preset: String,
    /// B-frames of hardware encoders (NVENC, QSV, VAAPI) without `-bf`; empty = ffmpeg's.
    pub bframes: String,
    /// Free GPU memory (MiB) `Cuda::Auto` needs for `overlay` (VIZRS_FFMPEG_CUDA_MIN_FREE).
    pub cuda_min_free: u64,
    /// `-shortest` left out of commands whose output has a `-t` (VIZRS_FFMPEG_NO_SHORTEST).
    pub no_shortest: bool,
    /// With `Cuda::Overlay`, media frames wait for the graphics in RAM (VIZRS_FFMPEG_SYNC_RAM).
    pub sync_ram: bool,
}

/// What of a command encoding with NVENC stays on the card (`VIZRS_FFMPEG_CUDA`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Cuda {
    /// Empty: ErsatzTV's command (CUDA decoding, filters and encoder).
    #[default]
    Full,
    /// `encode`: only the encoder; decoding and filters on the CPU.
    Encode,
    /// `overlay`: decoding on the CPU; filters, the overlay of the graphics and the encoder on the card.
    Overlay,
    /// `auto`: `overlay` while the card has enough free memory, `encode` below.
    Auto,
}

impl Cuda {
    pub fn parse(spec: &str) -> Cuda {
        match spec.trim().to_ascii_lowercase().as_str() {
            "encode" => Cuda::Encode,
            "overlay" => Cuda::Overlay,
            "auto" => Cuda::Auto,
            _ => Cuda::Full,
        }
    }
}

impl Settings {
    pub fn from_env() -> Settings {
        let var = |name| std::env::var(name).unwrap_or_default();
        Settings {
            ffmpeg: std::env::var_os("VIZRS_FFMPEG").unwrap_or_else(|| DEFAULT_FFMPEG.into()),
            sidecar: var("VIZRS_CG_URL").trim_end_matches('/').to_owned(),
            fps: var("VIZRS_FFMPEG_FPS").trim().to_owned(),
            preset: var("VIZRS_FFMPEG_PRESET").trim().to_owned(),
            profile: String::new(),
            x264_params: var("VIZRS_FFMPEG_X264_PARAMS").trim().to_owned(),
            catchup: var("VIZRS_FFMPEG_READRATE_CATCHUP").trim().to_owned(),
            blend: var("VIZRS_FFMPEG_FPS_BLEND").trim() == "1",
            ingest: var("VIZRS_FFMPEG_INGEST").trim() == "1",
            standby: var("VIZRS_FFMPEG_STANDBY").trim().to_owned(),
            cuda: Cuda::parse(&var("VIZRS_FFMPEG_CUDA")),
            nvenc_preset: String::new(),
            qsv_preset: String::new(),
            bframes: var("VIZRS_FFMPEG_NVENC_BF").trim().to_owned(),
            cuda_min_free: var("VIZRS_FFMPEG_CUDA_MIN_FREE").trim().parse().unwrap_or(DEFAULT_CUDA_MIN_FREE),
            no_shortest: var("VIZRS_FFMPEG_NO_SHORTEST").trim() == "1",
            sync_ram: var("VIZRS_FFMPEG_SYNC_RAM").trim() == "1",
        }
        .with_encoder_file(&std::env::var("VIZRS_FFMPEG_SETTINGS").unwrap_or_else(|_| encoder::DEFAULT_PATH.into()))
    }

    /// The encoder settings of VizRS Live (`encoder.json`, src/encoder.rs) over
    /// those of the environment, key by key; `path` empty or no file = the environment's.
    pub fn with_encoder_file(self, path: &str) -> Settings {
        let env = Encoder {
            preset: self.preset.clone(),
            profile: self.profile.clone(),
            x264_params: self.x264_params.clone(),
            nvenc_preset: self.nvenc_preset.clone(),
            qsv_preset: self.qsv_preset.clone(),
            bframes: self.bframes.clone(),
        };
        let (Encoder { preset, profile, x264_params, nvenc_preset, qsv_preset, bframes }, _) =
            Encoder::in_force((!path.is_empty()).then(|| Path::new(path)), &env);
        Settings { preset, profile, x264_params, nvenc_preset, qsv_preset, bframes, ..self }
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

/// `<flag> <value>` right after the video encoder (`-c:v <name>`) when
/// `value(name)` gives one and the command has no `<flag>` of its own (`-preset`
/// or `-preset:v`; `flag` without the stream specifier).
fn after_encoder(args: Vec<OsString>, flag: &str, value: impl Fn(&str) -> Option<String>) -> Vec<OsString> {
    let own = args.iter().any(|a| a.to_str().is_some_and(|a| a == flag || a.strip_prefix(flag).is_some_and(|rest| rest.starts_with(':'))));
    if own {
        return args;
    }
    let mut out = Vec::with_capacity(args.len() + 2);
    let mut previous: Option<&OsString> = None;
    for arg in &args {
        out.push(arg.clone());
        let encoder = previous.is_some_and(|p| CODEC.iter().any(|c| p == c)).then(|| arg.to_str()).flatten();
        if let Some(v) = encoder.and_then(&value).filter(|v| !v.is_empty()) {
            out.extend([format!("{flag}:v").into(), v.into()]);
        }
        previous = Some(arg);
    }
    out
}

/// A software video encoder without a preset gets `-preset:v <preset>`.
pub fn with_preset(args: Vec<OsString>, preset: &str) -> Vec<OsString> {
    with_presets(args, preset, "", "")
}

/// The preset of the encoder in the command: x264/x265 (`software`), NVENC
/// (`nvenc`: p1–p7) or QSV (`qsv`); a command with its own `-preset` keeps it.
/// VAAPI has no preset.
pub fn with_presets(args: Vec<OsString>, software: &str, nvenc: &str, qsv: &str) -> Vec<OsString> {
    after_encoder(args, "-preset", |encoder| {
        let preset = if PRESET_ENCODERS.contains(&encoder) {
            software
        } else if encoder.ends_with("_nvenc") {
            nvenc
        } else if encoder.ends_with("_qsv") {
            qsv
        } else {
            ""
        };
        Some(preset.to_owned())
    })
}

/// An H.264 encoder without a profile (libx264, NVENC, QSV, VAAPI) gets
/// `-profile:v <profile>` (baseline, main, high; VAAPI: `constrained_baseline`
/// for baseline). HEVC and other encoders keep theirs: their profiles differ.
pub fn with_profile(args: Vec<OsString>, profile: &str) -> Vec<OsString> {
    if profile.is_empty() {
        return args;
    }
    after_encoder(args, "-profile", |encoder| encoder::profile_for(encoder, profile))
}

/// A libx264 encoder gets `-x264-params:v <params>`; options it already has
/// (ErsatzTV's `-x264-params`) come first, so ours win where both set one.
pub fn with_x264_params(args: Vec<OsString>, params: &str) -> Vec<OsString> {
    if params.is_empty() {
        return args;
    }
    let existing = args.iter().position(|a| a.to_str().is_some_and(|a| a == "-x264-params" || a.starts_with("-x264-params:")));
    if let Some(i) = existing.filter(|i| i + 1 < args.len()) {
        let mut out = args;
        let joined = match out[i + 1].to_str() {
            Some(old) if !old.is_empty() => format!("{old}:{params}"),
            _ => params.to_owned(),
        };
        out[i + 1] = joined.into();
        return out;
    }
    let mut out = Vec::with_capacity(args.len() + 2);
    let mut previous: Option<&OsString> = None;
    for arg in &args {
        out.push(arg.clone());
        if previous.is_some_and(|p| CODEC.iter().any(|c| p == c)) && arg == "libx264" {
            out.extend(["-x264-params:v".into(), params.into()]);
        }
        previous = Some(arg);
    }
    out
}

/// A hardware video encoder (NVENC, QSV, VAAPI) without `-bf` gets `-bf:v <frames>` (a number).
pub fn with_bframes(args: Vec<OsString>, frames: &str) -> Vec<OsString> {
    if frames.is_empty() || !frames.bytes().all(|b| b.is_ascii_digit()) {
        return args;
    }
    after_encoder(args, "-bf", |encoder| encoder::is_hardware(encoder).then(|| frames.to_owned()))
}

/// The command without `-shortest` when its output ends at a `-t` anyway.
/// With `apad` and `loudnorm` (ErsatzTV: about 3 s ahead), `-shortest` holds the
/// raw video frames until the audio of the same time comes: 150 frames of
/// 1080p50, 657 MB of ffmpeg's memory instead of 268 MB, twice at a change of
/// item (VIZRS_FFMPEG_NO_SHORTEST). The output is as long either way.
pub fn without_shortest(args: Vec<OsString>, on: bool) -> Vec<OsString> {
    let Some(last) = (0..args.len().saturating_sub(1)).rev().find(|&i| args[i] == "-i") else { return args };
    let output = last + 2;
    let ends = args.iter().skip(output).any(|a| a == "-t");
    if !on || !ends {
        return args;
    }
    args.into_iter().enumerate().filter(|(i, a)| *i < output || a != "-shortest").map(|(_, a)| a).collect()
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

/// The command with the forced rate, blending, preset, profile, x264 options, B-frames and catch-up
/// rate, when set; `burst` = seconds of graphics ready at once (`paced_media`).
fn adjusted(args: Vec<OsString>, settings: &Settings, burst: Option<f64>) -> Vec<OsString> {
    let cuda = match settings.cuda {
        Cuda::Auto if nvenc_with_cuda(&args) => auto_cuda(free_gpu_memory(), settings.cuda_min_free),
        Cuda::Auto => Cuda::Full,
        mode => mode,
    };
    let args = match cuda {
        Cuda::Full | Cuda::Auto => args,
        Cuda::Encode => cuda_encode_only(args),
        Cuda::Overlay => media_waits_in_ram(paced_media(cuda_decode_on_cpu(args), burst), settings.sync_ram),
    };
    let rated = with_rate(args, &settings.fps);
    let source = if settings.blend { media_input(&rated).and_then(|input| source_rate(settings, &input)) } else { None };
    let blended = with_blend(rated, settings.blend, source);
    // Each goes right after the encoder: the last inserted comes first (preset, profile, options).
    let encoded = with_presets(
        with_profile(with_x264_params(blended, &settings.x264_params), &settings.profile),
        &settings.preset,
        &settings.nvenc_preset,
        &settings.qsv_preset,
    );
    let tuned = with_catchup(with_bframes(encoded, &settings.bframes), &settings.catchup);
    without_shortest(tuned, settings.no_shortest)
}

/// A command encoding with NVENC that decodes or filters with CUDA.
fn nvenc_with_cuda(args: &[OsString]) -> bool {
    let text: Vec<&str> = args.iter().filter_map(|a| a.to_str()).collect();
    let nvenc = text.windows(2).any(|w| CODEC.contains(&w[0]) && w[1].ends_with("_nvenc"));
    nvenc && (text.windows(2).any(|w| w[0] == "-hwaccel" && w[1] == "cuda") || text.iter().any(|a| a.contains("_cuda")))
}

/// `VIZRS_FFMPEG_CUDA=auto`: `overlay` with at least `min_free` MiB free on
/// the card or when that is unknown, `encode` below (logged).
pub fn auto_cuda(free: Option<u64>, min_free: u64) -> Cuda {
    match free {
        Some(free) if free < min_free => {
            log(format_args!("GPU memory {free} MiB free (< {min_free}): CUDA filters on the CPU (encode)"));
            Cuda::Encode
        }
        _ => Cuda::Overlay,
    }
}

/// Free memory of the first card in MiB from `nvidia-smi` (at most 1 s); None = unknown.
fn free_gpu_memory() -> Option<u64> {
    let mut query = Command::new("nvidia-smi")
        .args(["--query-gpu=memory.free", "--format=csv,noheader,nounits"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = std::time::Instant::now();
    loop {
        match query.try_wait().ok()? {
            Some(status) if status.success() => break,
            Some(_) => return None,
            None if started.elapsed() > GPU_QUERY_WAIT => {
                let _ = query.kill();
                let _ = query.wait();
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    let mut text = String::new();
    query.stdout.take()?.read_to_string(&mut text).ok()?;
    parse_free_memory(&text)
}

/// The first card's free MiB from `nvidia-smi --query-gpu=memory.free --format=csv,noheader,nounits`.
pub fn parse_free_memory(text: &str) -> Option<u64> {
    text.lines().next()?.trim().parse().ok()
}

/// Options of ErsatzTV's CUDA decoding left out with `VIZRS_FFMPEG_CUDA=encode` (with their value).
const CUDA_OPTIONS: [&str; 4] = ["-hwaccel", "-hwaccel_output_format", "-hwaccel_device", "-filter_hw_device"];

/// A command encoding with NVENC, its CUDA decoding and filters moved to the
/// CPU (`VIZRS_FFMPEG_CUDA=encode`); any other command, or one it cannot
/// rewrite, unchanged.
pub fn cuda_encode_only(args: Vec<OsString>) -> Vec<OsString> {
    let text: Option<Vec<&str>> = args.iter().map(|a| a.to_str()).collect();
    let Some(text) = text else { return args };
    let nvenc = text.windows(2).any(|w| ["-c:v", "-vcodec", "-codec:v"].contains(&w[0]) && w[1].ends_with("_nvenc"));
    let cuda = text.windows(2).any(|w| w[0] == "-hwaccel" && w[1] == "cuda") || text.iter().any(|a| a.contains("_cuda"));
    if !nvenc || !cuda {
        return args;
    }
    let mut out = Vec::with_capacity(args.len());
    let mut i = 0;
    while i < text.len() {
        let arg = text[i];
        let value = text.get(i + 1).copied().unwrap_or("");
        if CUDA_OPTIONS.contains(&arg) && i + 1 < text.len() {
            i += 2;
            continue;
        }
        if arg == "-init_hw_device" && value.starts_with("cuda") {
            i += 2;
            continue;
        }
        if ["-filter_complex", "-vf", "-filter:v"].contains(&arg) && i + 1 < text.len() {
            match cpu_filters(value) {
                Some(filters) => out.extend([OsString::from(arg), OsString::from(filters)]),
                None => return args,
            }
            i += 2;
            continue;
        }
        out.push(OsString::from(arg));
        i += 1;
    }
    out
}

/// A filter graph with its CUDA filters as CPU filters; None = cannot tell.
fn cpu_filters(graph: &str) -> Option<String> {
    let chains: Vec<String> = graph.split(';').map(cpu_chain).collect::<Option<_>>()?;
    Some(chains.join(";"))
}

/// One chain `[in]f1,f2[out]`: uploads dropped, CUDA filters replaced.
fn cpu_chain(chain: &str) -> Option<String> {
    // input labels, filters, output labels
    let (inputs, rest) = chain.split_at(leading_labels(chain));
    let tail = trailing_labels(rest);
    let (body, outputs) = rest.split_at(rest.len() - tail);
    let mut filters = Vec::new();
    for filter in body.split(',').filter(|f| !f.is_empty()) {
        let (name, options) = filter.split_once('=').unwrap_or((filter, ""));
        match name {
            "hwupload" | "hwupload_cuda" => {}
            "scale_cuda" => filters.extend(cpu_scale(options)),
            "overlay_cuda" => filters.push(cpu_overlay(options)),
            "pad_cuda" => filters.push(format!("pad={options}")),
            "yadif_cuda" => filters.push(if options.is_empty() { "yadif".into() } else { format!("yadif={options}") }),
            "bwdif_cuda" => filters.push(if options.is_empty() { "bwdif".into() } else { format!("bwdif={options}") }),
            other if other.ends_with("_cuda") || other == "hwdownload" => return None,
            _ => filters.push(filter.to_owned()),
        }
    }
    if filters.is_empty() {
        filters.push("null".into());
    }
    Some(format!("{inputs}{}{outputs}", filters.join(",")))
}

/// Bytes of the `[label]` run at the start.
fn leading_labels(chain: &str) -> usize {
    let mut at = 0;
    while chain[at..].starts_with('[') {
        match chain[at..].find(']') {
            Some(end) => at += end + 1,
            None => break,
        }
    }
    at
}

/// Bytes of the `[label]` run at the end.
fn trailing_labels(chain: &str) -> usize {
    let mut end = chain.len();
    while chain[..end].ends_with(']') {
        match chain[..end].rfind('[') {
            Some(start) => end = start,
            None => break,
        }
    }
    chain.len() - end
}

/// `scale_cuda=W:H:format=F:…` → `scale=W:H` and `format=F` (CUDA-only options dropped).
fn cpu_scale(options: &str) -> Vec<String> {
    let mut size = Vec::new();
    let mut named = Vec::new();
    let mut format = None;
    for (n, option) in options.split(':').filter(|o| !o.is_empty()).enumerate() {
        match option.split_once('=') {
            Some(("format", f)) => format = Some(f.to_owned()),
            Some(("w" | "h" | "force_original_aspect_ratio" | "force_divisible_by", _)) => named.push(option.to_owned()),
            Some(_) => {}
            None if n < 2 => size.push(option.to_owned()),
            None => {}
        }
    }
    let mut out = Vec::new();
    if !size.is_empty() || !named.is_empty() {
        let mut all = size;
        all.extend(named);
        out.push(format!("scale={}", all.join(":")));
    }
    if let Some(format) = format {
        out.push(format!("format={format}"));
    }
    out
}

/// `overlay_cuda=x=X:y=Y:…` → `overlay=…:format=auto` (yuva graphics over yuv video).
fn cpu_overlay(options: &str) -> String {
    let mut all: Vec<&str> = options.split(':').filter(|o| !o.is_empty()).collect();
    if !all.iter().any(|o| o.starts_with("format=")) {
        all.push("format=auto");
    }
    format!("overlay={}", all.join(":"))
}

/// Options of ErsatzTV's CUDA decoding left out with `VIZRS_FFMPEG_CUDA=overlay` (with their value).
const DECODE_OPTIONS: [&str; 3] = ["-hwaccel", "-hwaccel_output_format", "-hwaccel_device"];
/// Before an upload of decoded frames: the formats a CUDA device takes (the
/// decoder's own, e.g. yuv420p10le, becomes the nearest of them).
const UPLOAD_FORMATS: &str = "format=nv12|yuv420p|yuva420p|p010le|yuv444p|p016le|yuv444p16le";

/// A canvas transcode overlaying the graphics on the card (`overlay_cuda`
/// with raw frames from `pipe:N`) reads the media (its first input) at
/// `-readrate 1` when it has no rate of its own (`VIZRS_FFMPEG_CUDA=overlay`).
/// The graphics come in the channel's time (with `lead_seconds` ahead), so
/// the transcode runs no faster with the media read at full speed – the
/// decoded frames only wait for them on the card in front of `overlay_cuda`
/// (Quadro P400, 1080p50, three areas: 387 MiB instead of 159). With `burst`
/// (seconds of graphics the sidecar has ready at once) the media starts that
/// far ahead too (`-readrate_initial_burst`, at least ffmpeg's 0.5 s): a piece
/// started late makes up the time lost, as ErsatzTV's own burst does, without
/// frames waiting for graphics. Any other command unchanged.
pub fn paced_media(args: Vec<OsString>, burst: Option<f64>) -> Vec<OsString> {
    let text: Option<Vec<&str>> = args.iter().map(|a| a.to_str()).collect();
    let Some(text) = text else { return args };
    let on_card = text.windows(2).any(|w| w[0] == "-filter_complex" && w[1].contains("overlay_cuda"));
    let raw = text.windows(2).any(|w| w[0] == "-i" && w[1].starts_with("pipe:"));
    let Some(first) = text.iter().position(|a| *a == "-i") else { return args };
    if !on_card || !raw || text[1 + first].starts_with("pipe:") || text[..first].contains(&"-readrate") {
        return args;
    }
    let mut rate: Vec<OsString> = vec!["-readrate".into(), "1".into()];
    if let Some(burst) = burst.filter(|b| b.is_finite()) {
        rate.extend(["-readrate_initial_burst".into(), format!("{:.1}", burst.clamp(MIN_BURST, MAX_BURST)).into()]);
    }
    let mut out = args;
    out.splice(first..first, rate);
    out
}

/// The filters that hold a media frame until the graphics of its moment are
/// there: a 2x2 cut of the graphics overlaid outside the picture (nothing
/// drawn), in the media's pixel format, but without alpha – `auto` offers
/// `yuva420p` too, which `scale_cuda` does not take (ffmpeg 8.1 of ErsatzTV);
/// after the graphics end the media goes on alone.
const RAM_SYNC: &str = "overlay=x=main_w:y=main_h:format=auto:eof_action=pass,format=nv12|yuv420p|p010le|yuv444p|p016le|yuv444p16le";

/// `VIZRS_FFMPEG_SYNC_RAM=1` with `overlay`: in the graph that overlays the
/// graphics on the card (`overlay_cuda`), the one media chain that uploads
/// decoded frames (`[N:…]…,hwupload,…`) first syncs them with the graphics of
/// the pipe input on the CPU (`RAM_SYNC`), so frames decoded ahead – for the
/// 3 s of `loudnorm` lookahead, a burst, no `-readrate` – wait in RAM, and
/// only frames whose graphics are there go to the card. The graphics input
/// is split for it. Any other command (off, no graphics pipe, no
/// `overlay_cuda`, no such media chain or more than one, graphics used twice)
/// unchanged.
pub fn media_waits_in_ram(args: Vec<OsString>, on: bool) -> Vec<OsString> {
    if !on {
        return args;
    }
    let text: Option<Vec<&str>> = args.iter().map(|a| a.to_str()).collect();
    let Some(text) = text else { return args };
    let inputs: Vec<&str> = text.windows(2).filter(|w| w[0] == "-i").map(|w| w[1]).collect();
    let Some(graphics) = inputs.iter().position(|input| input.starts_with("pipe:")) else { return args };
    let Some(at) = text.windows(2).position(|w| w[0] == "-filter_complex" && w[1].contains("overlay_cuda")) else { return args };
    match graph_waiting_in_ram(text[at + 1], graphics) {
        Some(graph) => {
            let mut out = args;
            out[at + 1] = graph.into();
            out
        }
        None => args,
    }
}

/// `graph` with its media frames synced with the graphics of input `graphics` before their upload.
fn graph_waiting_in_ram(graph: &str, graphics: usize) -> Option<String> {
    if graph.contains("[vz_w") {
        return None;
    }
    let pattern = Regex::new(&format!(r"\[{graphics}(:[^\]]*)?\]")).ok()?;
    let mut uses = pattern.find_iter(graph);
    let label = uses.next()?.as_str().to_owned();
    if uses.next().is_some() {
        return None;
    }
    let mut chains: Vec<String> = graph.split(';').map(str::to_owned).collect();
    let uploads = |body: &str| {
        let filters = &body[..body.len() - trailing_labels(body)];
        filters.split(',').any(|f| matches!(f.split_once('=').map_or(f, |(n, _)| n), "hwupload" | "hwupload_cuda"))
    };
    let media: Vec<usize> = (0..chains.len())
        .filter(|&i| {
            let chain = &chains[i];
            let (labels, body) = chain.split_at(leading_labels(chain));
            let input = labels.strip_prefix('[').and_then(|l| l.strip_suffix(']')).filter(|l| !l.contains('['));
            let stream = input.and_then(|l| l.split(':').next()).and_then(|n| n.parse::<usize>().ok());
            stream.is_some_and(|n| n != graphics) && uploads(body)
        })
        .collect();
    let [media] = media.as_slice() else { return None };
    let chain = &chains[*media];
    let (labels, body) = chain.split_at(leading_labels(chain));
    chains[*media] = format!("{labels}[vz_wt]{RAM_SYNC},{body}");
    let joined = chains.join(";").replacen(&label, "[vz_w1]", 1);
    Some(format!("{label}split=2[vz_w0][vz_w1];[vz_w0]crop=2:2:0:0[vz_wt];{joined}"))
}

/// ffmpeg's own initial burst of `-readrate`.
const MIN_BURST: f64 = 0.5;
/// More would be a wrong clock (the sidecar takes an air time at most 10 s behind).
const MAX_BURST: f64 = 30.0;

/// A command encoding with NVENC from CUDA decoding, decoding on the CPU
/// (`VIZRS_FFMPEG_CUDA=overlay`): the decoded frames go to the card where
/// ErsatzTV's filters expect them, its CUDA filters and the overlay of the
/// graphics stay there, and the frames for the encoder come back to the CPU
/// (`place_frames`). Any other command unchanged.
pub fn cuda_decode_on_cpu(args: Vec<OsString>) -> Vec<OsString> {
    let text: Option<Vec<&str>> = args.iter().map(|a| a.to_str()).collect();
    let Some(text) = text else { return args };
    let nvenc = text.windows(2).any(|w| ["-c:v", "-vcodec", "-codec:v"].contains(&w[0]) && w[1].ends_with("_nvenc"));
    let decode = text.windows(2).any(|w| w[0] == "-hwaccel" && w[1] == "cuda");
    if !nvenc || !decode {
        return args;
    }
    let mapped: Vec<&str> = text
        .windows(2)
        .filter(|w| w[0] == "-map" && w[1].starts_with('['))
        .map(|w| w[1].trim_start_matches('[').trim_end_matches(']'))
        .collect();
    let mut card = Vec::new();
    let mut out = Vec::with_capacity(args.len() + 2);
    let mut i = 0;
    while i < text.len() {
        let arg = text[i];
        if DECODE_OPTIONS.contains(&arg) && i + 1 < text.len() {
            i += 2;
            continue;
        }
        if ["-filter_complex", "-vf", "-filter:v"].contains(&arg) && i + 1 < text.len() {
            let simple = arg != "-filter_complex";
            let graph: Vec<String> = text[i + 1].split(';').map(|chain| place_frames(chain, &mut card, &mapped, simple)).collect();
            out.extend([OsString::from(arg), OsString::from(graph.join(";"))]);
            i += 2;
            continue;
        }
        out.push(OsString::from(arg));
        i += 1;
    }
    out
}

/// One chain of a graph without CUDA decoding. The frames of an input stream
/// (`[0:0]…`, or a `-vf` without labels) are on the CPU now: an upload goes
/// before the first filter that needs them on the card, a download of them is
/// dropped. Frames on the card at the end of a chain for the encoder (`-map`,
/// or a `-vf`) come back to the CPU (`hwdownload`): the encoder waits for the
/// muxer – e.g. for 3 s of `loudnorm` lookahead in the audio – and the frames
/// waiting for it on the card filled its memory (`CUDA_ERROR_OUT_OF_MEMORY`
/// on a Quadro P400 within seconds); in RAM they cost nothing. `card`: the
/// labels whose frames are on the card, filled chain by chain.
fn place_frames(chain: &str, card: &mut Vec<String>, mapped: &[&str], simple: bool) -> String {
    let (inputs, rest) = chain.split_at(leading_labels(chain));
    let tail = trailing_labels(rest);
    let (body, outputs) = rest.split_at(rest.len() - tail);
    let labels =
        |text: &str| -> Vec<String> { text.split(']').filter(|l| !l.is_empty()).map(|l| l.trim_start_matches('[').to_owned()).collect() };
    let (ins, outs) = (labels(inputs), labels(outputs));
    let stream = |label: &String| label.split(':').next().is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    let decoded = ins.len() <= 1 && ins.iter().all(stream);
    let mut on_card = !decoded && ins.iter().any(|l| card.contains(l));
    let mut filters: Vec<String> = Vec::new();
    for filter in body.split(',').filter(|f| !f.is_empty()) {
        let name = filter.split_once('=').map_or(filter, |(name, _)| name);
        let formatted = filters.last().is_some_and(|f| f.starts_with("format="));
        match name {
            "hwupload" | "hwupload_cuda" => {
                if decoded && !on_card && !formatted {
                    filters.push(UPLOAD_FORMATS.into());
                }
                on_card = true;
            }
            "hwdownload" if decoded && !on_card => continue,
            "hwdownload" => on_card = false,
            cuda if cuda.ends_with("_cuda") => {
                if decoded && !on_card {
                    filters.extend([UPLOAD_FORMATS.into(), "hwupload".into()]);
                }
                on_card = true;
            }
            _ => {}
        }
        filters.push(filter.to_owned());
    }
    let encoder = if simple { outs.len() <= 1 } else { outs.len() == 1 && mapped.contains(&outs[0].as_str()) };
    if on_card && encoder {
        filters.extend(["hwdownload".into(), UPLOAD_FORMATS.into()]);
        on_card = false;
    }
    if on_card {
        card.extend(outs);
    }
    format!("{inputs}{}{outputs}", filters.join(","))
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
    /// Query of `/ffmpeg/resume` after a restart of the sidecar; empty = an older sidecar.
    pub resume: String,
    /// The sidecar's session taken over.
    pub session: String,
    /// Seconds of graphics the sidecar had ready at once when the session was
    /// taken over, and when (`ahead`); None = an older sidecar.
    pub ahead: Option<(std::time::Instant, f64)>,
    /// Revision of zones that may widen in the middle of the piece (`area:
    /// auto`, PLAN-strefy S3b); None = fixed for the piece (an older sidecar too).
    pub zones_rev: Option<u64>,
}

impl Frames {
    /// Seconds of graphics ready at once for a run from `elapsed` s of the
    /// piece: those ready at the takeover, plus the time since, minus the
    /// piece before the run. None = unknown (an older sidecar).
    fn burst(&self, elapsed: f64) -> Option<f64> {
        self.ahead.map(|(at, seconds)| seconds + at.elapsed().as_secs_f64() - elapsed)
    }
}

/// The parts of a frame layout in a reply of the sidecar (`areas`, stacked).
fn stacked(info: &Value) -> io::Result<Vec<(Area, u32)>> {
    let number =
        |value: &Value, key: &str| value.get(key).and_then(Value::as_f64).map(|n| n as u32).ok_or_else(|| error(format!("'areas.{key}'")));
    let mut areas = Vec::new();
    let mut row = 0;
    for area in info.get("areas").and_then(Value::as_array).unwrap_or(&[]) {
        let area = Area { x: number(area, "x")?, y: number(area, "y")?, w: number(area, "w")?, h: number(area, "h")? };
        areas.push((area, row));
        row += area.h;
    }
    Ok(areas)
}

/// The zones of a session now (`GET /ffmpeg/zones`).
#[derive(Clone, Debug, PartialEq)]
pub struct Zones {
    pub rev: u64,
    /// `WxH+X+Y,…`; empty = the whole channel.
    pub geometry: String,
    pub width: u32,
    pub height: u32,
    pub areas: Vec<(Area, u32)>,
    pub resume: String,
}

/// A reply of `/ffmpeg/zones`.
pub fn parse_zones(body: &str) -> io::Result<Zones> {
    let info = json::parse(body).map_err(|e| error(e.to_string()))?;
    let size = |key| info.get(key).and_then(Value::as_f64).map(|n| n as u32).ok_or_else(|| error(format!("'{key}'")));
    let areas = stacked(&info)?;
    if areas.is_empty() {
        return Err(error("'areas'".into()));
    }
    Ok(Zones {
        rev: info.get("rev").and_then(Value::as_f64).map(|n| n as u64).ok_or_else(|| error("'rev'".into()))?,
        geometry: info.get("zones").and_then(Value::as_str).unwrap_or_default().to_owned(),
        width: size("width")?,
        height: size("height")?,
        areas,
        resume: info.get("resume").and_then(Value::as_str).unwrap_or_default().to_owned(),
    })
}

/// The zones of the session, asked every `ZONES_POLL` in the background: the
/// newest revision waits for the next run.
struct ZoneWatch {
    latest: Arc<Mutex<Option<Zones>>>,
    used: AtomicU64,
    stop: Arc<AtomicBool>,
}

impl ZoneWatch {
    fn start(sidecar: &str, session: &str, rev: u64) -> ZoneWatch {
        let watch = ZoneWatch { latest: Arc::default(), used: AtomicU64::new(rev), stop: Arc::default() };
        let (latest, stop) = (Arc::clone(&watch.latest), Arc::clone(&watch.stop));
        let (sidecar, path) = (sidecar.to_owned(), format!("/ffmpeg/zones?session={}", quote_plus(session)));
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(ZONES_POLL);
                match sidecar_get(&sidecar, &path) {
                    Ok((status, body)) if status == "200" => {
                        if let Ok(zones) = parse_zones(&String::from_utf8_lossy(&body)) {
                            let mut latest = latest.lock().unwrap_or_else(|e| e.into_inner());
                            if latest.as_ref().is_none_or(|l| zones.rev > l.rev) {
                                *latest = Some(zones);
                            }
                        }
                    }
                    // The session is over (a restarted sidecar too): the zones stay.
                    Ok(_) => return,
                    Err(_) => {}
                }
            }
        });
        watch
    }

    /// Zones newer than those of the runs so far.
    fn newer(&self) -> bool {
        let used = self.used.load(Ordering::Relaxed);
        self.latest.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|z| z.rev > used)
    }

    /// The newer zones, now those of the runs.
    fn take(&self) -> Option<Zones> {
        let latest = self.latest.lock().unwrap_or_else(|e| e.into_inner()).clone()?;
        (latest.rev > self.used.swap(latest.rev, Ordering::Relaxed)).then_some(latest)
    }
}

impl Drop for ZoneWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Bytes of one yuva420p frame.
fn frame_bytes(width: u32, height: u32) -> usize {
    let (w, h) = (width as usize, height as usize);
    2 * w * h + 2 * w.div_ceil(2) * h.div_ceil(2)
}

/// Frames from the sidecar -> ffmpeg's input. The sidecar gone (a new version
/// of the stack), ffmpeg gets transparent frames, as fast as it reads them, so
/// the channel goes on; a thread reconnects and the frames follow. Without a
/// way back (`resume` empty) ffmpeg sees the end of its input, as before.
/// A delta stream is patched into the frame shown, and ffmpeg gets it whole.
fn relay(socket: FrameSocket, mut pipe: impl Write, frame: usize, reconnect: Option<&dyn Fn() -> mpsc::Receiver<FrameSocket>>) {
    let mut buffer = vec![0u8; 1 << 20];
    let blank = vec![0u8; frame];
    let mut shown = vec![0u8; frame];
    let mut source = socket;
    let mut partial = 0;
    loop {
        if source.delta {
            let mut reader = io::BufReader::with_capacity(1 << 16, &source.stream);
            while crate::delta::read_record(&mut reader, &mut shown).is_ok() {
                if pipe.write_all(&shown).is_err() {
                    return;
                }
            }
        } else {
            loop {
                match source.stream.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if pipe.write_all(&buffer[..n]).is_err() {
                            return;
                        }
                        partial = (partial + n) % frame;
                    }
                }
            }
        }
        let Some(reconnect) = reconnect else { return };
        log("frame stream lost; transparent frames until the sidecar is back");
        if pipe.write_all(&blank[..(frame - partial) % frame]).is_err() {
            return;
        }
        partial = 0;
        let back = reconnect();
        source = loop {
            if pipe.write_all(&blank).is_err() {
                return;
            }
            if let Ok(socket) = back.try_recv() {
                log("frame stream back");
                break socket;
            }
        };
    }
}

/// The descriptor pair ffmpeg reads the raw graphics frames from (`pipe:<fd>`
/// takes any descriptor): a Unix socket, not a pipe. Pipes count towards the
/// pipe memory of their user, shared by every container running as root and by
/// docker-proxy (1 MiB pipes per proxied connection): past
/// `fs.pipe-user-pages-soft` (64 MiB) the kernel gives every new pipe of a
/// container 8 KiB, a 3.6 MB frame of 1600x900 then takes ~450 hand-overs and
/// the channel falls behind real time (0.96x measured with 45 viewers).
/// A socket's buffer is `net.core.wmem_default` (~208 KiB), whatever the pipes.
pub fn frame_channel() -> io::Result<(UnixStream, UnixStream)> {
    UnixStream::pair()
}

/// The input of the raw graphics frames at `pipe:<fd>`, with its options.
fn raw_input(frames: &Frames, fd: i32) -> Vec<OsString> {
    let size = format!("{}x{}", frames.width, frames.height);
    let raw = ["-f", "rawvideo", "-pix_fmt", "yuva420p", "-video_size", &size, "-framerate", &frames.rate];
    let mut out: Vec<OsString> = raw.iter().map(OsString::from).collect();
    // Undeclared, the overlay negotiates an unknown colour space for the whole
    // chain and swscale sends every scaled frame of the media through RGB.
    if !frames.matrix.is_empty() {
        out.extend(["-colorspace", &frames.matrix, "-color_range", "tv"].iter().map(OsString::from));
    }
    out.extend(["-i".into(), format!("pipe:{fd}").into()]);
    out
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
    out.extend(raw_input(frames, fd));
    out.extend_from_slice(&args[canvas.index + 2..]);
    // Frames of graphics areas: each overlaid at its place (checked by `with_position` beforehand).
    let at_origin = matches!(frames.areas.as_slice(), [(a, _)] if (a.x, a.y) == (0, 0));
    if !frames.areas.is_empty() && !at_origin {
        out = with_areas(&out, input_number(args, canvas.index), &frames.areas).unwrap_or(out);
    }
    shared_upload(out)
}

fn upload_filter() -> &'static Regex {
    static UPLOAD: OnceLock<Regex> = OnceLock::new();
    // `hwupload_cuda` without options, a whole filter of a chain.
    UPLOAD.get_or_init(|| Regex::new(r"(^|[,;\]])hwupload_cuda([,;\[]|$)").expect("upload pattern"))
}

/// A command with one CUDA device (`-init_hw_device cuda`) uploads through it:
/// `hwupload_cuda` (each one a CUDA context of its own, ~28 MiB of GPU memory)
/// becomes `hwupload`. Any other command unchanged.
pub fn shared_upload(args: Vec<OsString>) -> Vec<OsString> {
    let devices: Vec<&OsString> = args.windows(2).filter(|w| w[0] == "-init_hw_device").map(|w| &w[1]).collect();
    let cuda =
        matches!(devices.as_slice(), [d] if d.to_str().is_some_and(|d| d == "cuda" || d.starts_with("cuda:") || d.starts_with("cuda=")));
    if !cuda {
        return args;
    }
    let mut out = args;
    for i in 0..out.len().saturating_sub(1) {
        if out[i] != "-filter_complex" {
            continue;
        }
        let Some(graph) = out[i + 1].to_str() else { continue };
        // Twice: neighbouring uploads share the separator between them.
        let once = upload_filter().replace_all(graph, "${1}hwupload${2}").into_owned();
        out[i + 1] = upload_filter().replace_all(&once, "${1}hwupload${2}").into_owned().into();
    }
    out
}

/// `application/x-www-form-urlencoded` value, as `urllib.parse.urlencode`.
pub fn quote_plus(text: &str) -> String {
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
/// A frame stream of the sidecar: delta records (src/delta.rs; a sidecar that
/// knows them says so in `X-VizRS-Frames`) or whole frames (an older one).
pub struct FrameSocket {
    pub stream: TcpStream,
    pub delta: bool,
}

fn open_frames(sidecar: &str, path: &str) -> io::Result<FrameSocket> {
    let url = Url::split(sidecar).ok_or_else(|| error(format!("bad sidecar URL {sidecar}")))?;
    let mut stream = connect(&url)?;
    stream.write_all(format!("GET {path}&delta=1 HTTP/1.0\r\nHost: {}\r\n\r\n", url.netloc).as_bytes())?;
    let head = read_head_text(&mut stream, "frame stream")?;
    let line = head.lines().next().unwrap_or("");
    if status_of(line) != "200" {
        return Err(error(format!("frame stream: {line}")));
    }
    let delta = head.lines().any(|l| {
        l.split_once(':').is_some_and(|(name, value)| name.trim().eq_ignore_ascii_case("x-vizrs-frames") && value.trim() == "delta")
    });
    stream.set_read_timeout(None)?;
    stream.set_write_timeout(None)?;
    // ffmpeg blocks on this socket: a sidecar gone without closing it must not stop the channel.
    sys::keepalive(stream.as_raw_fd(), 3, 2, 4)?;
    Ok(FrameSocket { stream, delta })
}

/// The sidecar's session for the canvas channel and its frame stream. With
/// `place` (the command can overlay the graphics anywhere) the frames are
/// those of the graphics area, if the channel has one; `follow` = the piece
/// goes as runs that can widen the zones in its middle (PLAN-strefy S3b).
fn take_over(settings: &Settings, channel: &str, header_rate: &str, place: bool, follow: bool) -> io::Result<(Frames, FrameSocket)> {
    let area = match (place, follow) {
        (true, true) => "&area=1&follow=1",
        (true, false) => "&area=1",
        _ => "",
    };
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
        areas = stacked(&info)?;
        if areas.is_empty() {
            areas.push((Area { x: size("x")?, y: size("y")?, w: size("width")?, h: size("height")? }, 0));
        }
    }
    let matrix = info.get("matrix").and_then(Value::as_str).unwrap_or_default().to_owned();
    let resume = info.get("resume").and_then(Value::as_str).unwrap_or_default().to_owned();
    let zones_rev = info.get("zones_rev").and_then(Value::as_f64).map(|n| n as u64).filter(|_| place);
    let ahead = info.get("ahead").and_then(Value::as_f64).map(|seconds| (std::time::Instant::now(), seconds));
    let frames =
        Frames { width: size("width")?, height: size("height")?, rate, matrix, areas, resume, session: session.clone(), ahead, zones_rev };
    let path = format!("/ffmpeg/frames?session={}&fps={}{area}", quote_plus(&session), quote_plus(&frames.rate));
    Ok((frames, open_frames(&settings.sidecar, &path)?))
}

/// Replaces this process with the real ffmpeg (at the forced rate, preset and catch-up rate, if any).
fn plain(settings: &Settings, args: Vec<OsString>) -> ! {
    let error = Command::new(&settings.ffmpeg).args(adjusted(args, settings, None)).exec();
    log(format_args!("{}: {error}", settings.ffmpeg.to_string_lossy()));
    std::process::exit(127)
}

/// The transcode to HLS that live ingest can take over, with the channel's
/// live state then (`/ingest/state`: on, audio); None without
/// `VIZRS_FFMPEG_INGEST=1`, with ingest off in the sidecar, or for a piece of
/// the schedule while the sidecar takes over only live programmes
/// (`ingest_anytime` off). A live programme (slot) always goes this way, even
/// with the sidecar away: its media is the sidecar.
fn live_job(settings: &Settings, args: &[OsString], channel: Option<&str>) -> Option<(Job, bool, bool)> {
    if !settings.ingest || settings.sidecar.is_empty() {
        return None;
    }
    let job = Job::of(args, channel)?;
    match takeover::state(&settings.sidecar, &job.channel) {
        Some(state) => (state.enabled && (job.slot || state.anytime)).then_some((job, state.live, state.audio)),
        None => job.slot.then_some((job, false, false)),
    }
}

/// The real ffmpeg with `args`, graphics at `fd`, the forwarded signals unblocked.
fn spawn(settings: &Settings, args: Vec<OsString>, fd: Option<i32>) -> io::Result<Child> {
    let mut command = Command::new(&settings.ffmpeg);
    command.args(args);
    let fd = fd.unwrap_or(-1);
    // SAFETY: only async-signal-safe calls in the forked child.
    unsafe {
        command.pre_exec(move || sys::ffmpeg_child_setup(fd));
    }
    command.spawn()
}

/// The live run of the piece (src/takeover.rs), adjusted as every command;
/// without a source the standby picture of a live programme.
fn live_command(
    settings: &Settings,
    args: &[OsString],
    job: &Job,
    elapsed: f64,
    source: Option<&takeover::Source>,
    graphics: Option<(&Frames, i32)>,
) -> Vec<OsString> {
    let full = graphics.filter(|(f, _)| f.areas.is_empty()).map(|(f, _)| (f.width, f.height));
    let rate = graphics.map_or("", |(f, _)| f.rate.as_str());
    let format = takeover::channel_format(&settings.ffmpeg, args, job, &settings.fps, full, rate);
    let input = graphics.map(|(frames, fd)| takeover::Graphics { input: raw_input(frames, fd), areas: &frames.areas });
    let picture = match source {
        Some(source) => takeover::Picture::Live(source),
        None => takeover::Picture::Standby(standby_image(settings)),
    };
    adjusted(takeover::live_args(args, job, elapsed, picture, &format, input.as_ref()), settings, None)
}

/// `VIZRS_FFMPEG_STANDBY` when the file is there; else black.
fn standby_image(settings: &Settings) -> Option<&str> {
    let path = settings.standby.as_str();
    if path.is_empty() {
        return None;
    }
    if Path::new(path).is_file() {
        return Some(path);
    }
    log(format_args!("VIZRS_FFMPEG_STANDBY {path}: no such file; black instead"));
    None
}

/// Runs the piece through `takeover::sequence`, the signals of ErsatzTV forwarded to the run of the moment.
fn sequence(
    settings: &Settings,
    job: &Job,
    on: bool,
    audio: bool,
    takeover: bool,
    run: &dyn Fn(f64, Option<&takeover::Source>) -> io::Result<Child>,
    restart: &dyn Fn() -> bool,
) -> i32 {
    let current = Arc::new(AtomicU32::new(0));
    let stopping = Arc::new(AtomicBool::new(false));
    sys::block_signals(&FORWARDED);
    {
        let (current, stopping) = (Arc::clone(&current), Arc::clone(&stopping));
        std::thread::spawn(move || {
            loop {
                let signal = sys::wait_signal(&FORWARDED);
                stopping.store(true, Ordering::Relaxed);
                let pid = current.load(Ordering::Relaxed);
                if pid != 0 {
                    sys::send_signal(pid, signal);
                }
            }
        });
    }
    let runner = takeover::Runner { sidecar: &settings.sidecar, run, current: &current, stopping: &stopping, takeover, restart };
    takeover::sequence(&runner, job, on, audio)
}

/// A frame stream of the session and the frames it carries, relayed to the runs.
struct Feed {
    frames: Frames,
    control: SinkControl,
}

/// The piece with the graphics of the sidecar as runs (live ingest, zones
/// widened in the middle of the piece): every run gets its own frame socket of
/// the same frame stream; zones widened give the next run a new frame stream.
#[allow(clippy::too_many_arguments)]
fn with_graphics(
    settings: &Settings,
    args: &[OsString],
    canvas: &Canvas,
    job: &Job,
    (on, audio): (bool, bool),
    (frames, socket, place): (Frames, FrameSocket, bool),
    drain: Drain,
    takeover: bool,
) -> i32 {
    let watch = frames.zones_rev.filter(|_| place).map(|rev| ZoneWatch::start(&settings.sidecar, &frames.session, rev));
    let feed = Mutex::new(relayed(settings, frames, socket, place));
    let run = |elapsed: f64, source: Option<&takeover::Source>| -> io::Result<Child> {
        let mut current = feed.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(zones) = watch.as_ref().and_then(ZoneWatch::take) {
            let path = format!(
                "/ffmpeg/frames?session={}&fps={}&area=1&rev={}&zones={}&at={elapsed:.3}",
                quote_plus(&current.frames.session),
                quote_plus(&current.frames.rate),
                zones.rev,
                quote_plus(&zones.geometry)
            );
            match open_frames(&settings.sidecar, &path) {
                Ok(socket) => {
                    let frames = Frames {
                        width: zones.width,
                        height: zones.height,
                        rate: current.frames.rate.clone(),
                        matrix: current.frames.matrix.clone(),
                        areas: zones.areas,
                        resume: if current.frames.resume.is_empty() { String::new() } else { zones.resume },
                        session: current.frames.session.clone(),
                        ahead: current.frames.ahead,
                        zones_rev: Some(zones.rev),
                    };
                    let shown = if zones.geometry.is_empty() { "the whole channel" } else { zones.geometry.as_str() };
                    log(format_args!("graphics zones {shown} from {elapsed:.1} s of the piece"));
                    current.control.close();
                    *current = relayed(settings, frames, socket, place);
                }
                Err(e) => log(format_args!("frame stream of the new graphics zones: {e}; the zones before stay")),
            }
        }
        let frames = &current.frames;
        let (reader, writer) = frame_channel()?;
        let fd = reader.as_raw_fd();
        let command = match source {
            Some(source) => live_command(settings, args, job, elapsed, Some(source), Some((frames, fd))),
            None if job.slot => live_command(settings, args, job, elapsed, None, Some((frames, fd))),
            None => adjusted(job.program(&overlay_args(args, canvas, fd, frames), elapsed), settings, frames.burst(elapsed)),
        };
        let child = spawn(settings, command, Some(fd))?;
        drop(reader);
        current.control.attach(writer);
        Ok(child)
    };
    // The run stops for the new zones: its graphics end first (the sidecar may hold them).
    let restart = || {
        let newer = watch.as_ref().is_some_and(ZoneWatch::newer);
        if newer {
            feed.lock().unwrap_or_else(|e| e.into_inner()).control.cut();
        }
        newer
    };
    let code = sequence(settings, job, on, audio, takeover, &run, &restart);
    // The relay stops at its next frame, closing the frame stream: the sidecar ends the script.
    feed.lock().unwrap_or_else(|e| e.into_inner()).control.close();
    std::thread::sleep(SCRIPT_END);
    drain.close();
    code
}

/// `socket` (a frame stream of `frames`) relayed to the runs, reconnected
/// after a restart of the sidecar.
fn relayed(settings: &Settings, frames: Frames, socket: FrameSocket, place: bool) -> Feed {
    let frame = frame_bytes(frames.width, frames.height);
    let (sink, control) = Sink::new(frame);
    let area = if place { "&area=1" } else { "" };
    let resume = (!frames.resume.is_empty()).then(|| format!("/ffmpeg/resume?{}&fps={}{area}", frames.resume, quote_plus(&frames.rate)));
    let sidecar = settings.sidecar.clone();
    std::thread::spawn(move || {
        let reconnect = |path: &str| {
            let (sender, receiver) = mpsc::channel();
            let (sidecar, path) = (sidecar.clone(), path.to_owned());
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(RECONNECT);
                    if let Ok(socket) = open_frames(&sidecar, &path) {
                        let _ = sender.send(socket);
                        return;
                    }
                }
            });
            receiver
        };
        let again = resume.as_deref().map(|path| move || reconnect(path));
        relay(socket, sink, frame, again.as_ref().map(|f| f as &dyn Fn() -> mpsc::Receiver<FrameSocket>));
    });
    Feed { frames, control }
}

/// The overlay: `vizrs-ffmpeg <ffmpeg arguments>`; returns the exit code.
pub fn run(settings: &Settings, args: Vec<OsString>) -> i32 {
    let canvas = find_canvas(&args).filter(|_| !settings.sidecar.is_empty());
    let live = live_job(settings, &args, canvas.as_ref().map(|c| c.channel.as_str()));
    // Not a canvas transcode: plain ffmpeg, at the forced rate, preset and catch-up rate when set.
    let Some(canvas) = canvas else {
        return match live {
            Some((job, on, audio)) => {
                let program = |elapsed: f64| adjusted(job.program(&args, elapsed), settings, None);
                let run = |elapsed: f64, source: Option<&takeover::Source>| match source {
                    Some(source) => spawn(settings, live_command(settings, &args, &job, elapsed, Some(source), None), None),
                    None if job.slot => spawn(settings, live_command(settings, &args, &job, elapsed, None, None), None),
                    None => spawn(settings, program(elapsed), None),
                };
                sequence(settings, &job, on, audio, true, &run, &|| false)
            }
            None => plain(settings, args),
        };
    };

    let headers = parse_headers(&args[canvas.start..canvas.index]);
    let rate = headers.iter().find(|(name, _)| name == "x-etv-frame-rate").map_or("", |(_, v)| v.as_str()).to_owned();
    let drain = Drain::start(args[canvas.index + 1].to_string_lossy().into_owned(), headers);

    let place = with_position(&args, input_number(&args, canvas.index), 2, 2).is_some();
    // Runs that can widen the zones: live, or the piece as runs (below).
    let runs = live.is_some() || (settings.ingest && Job::of(&args, Some(&canvas.channel)).is_some_and(|job| !job.slot));
    let (frames, socket) = match take_over(settings, &canvas.channel, &rate, place, runs) {
        Ok(taken) => taken,
        Err(e) => {
            log(format_args!("{e}; ErsatzTV graphics instead"));
            drain.close();
            plain(settings, args)
        }
    };

    if let Some((job, on, audio)) = live {
        return with_graphics(settings, &args, &canvas, &job, (on, audio), (frames, socket, place), drain, true);
    }
    // Zones that may widen in the middle of the piece (S3b): the piece as runs
    // too, with live left alone (VIZRS_FFMPEG_INGEST allows the runs).
    if settings.ingest && frames.zones_rev.is_some() {
        if let Some(job) = Job::of(&args, Some(&canvas.channel)).filter(|job| !job.slot) {
            return with_graphics(settings, &args, &canvas, &job, (false, false), (frames, socket, place), drain, false);
        }
    }
    let (reader, writer) = match frame_channel() {
        Ok(pair) => pair,
        Err(e) => {
            log(format_args!("frame socket: {e}; ErsatzTV graphics instead"));
            drain.close();
            plain(settings, args)
        }
    };
    let fd = reader.as_raw_fd();
    // Forwarded to ffmpeg from here on; ffmpeg_child_setup unblocks them in the child.
    sys::block_signals(&FORWARDED);
    let mut command = Command::new(&settings.ffmpeg);
    command.args(adjusted(overlay_args(&args, &canvas, fd, &frames), settings, frames.burst(0.0)));
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
    drop(reader);
    let frame = frame_bytes(frames.width, frames.height);
    let area = if place { "&area=1" } else { "" };
    let resume = (!frames.resume.is_empty()).then(|| format!("/ffmpeg/resume?{}&fps={}{area}", frames.resume, quote_plus(&frames.rate)));
    let sidecar = settings.sidecar.clone();
    std::thread::spawn(move || {
        let reconnect = |path: &str| {
            let (sender, receiver) = mpsc::channel();
            let (sidecar, path) = (sidecar.clone(), path.to_owned());
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(RECONNECT);
                    if let Ok(socket) = open_frames(&sidecar, &path) {
                        let _ = sender.send(socket);
                        return;
                    }
                }
            });
            receiver
        };
        let again = resume.as_deref().map(|path| move || reconnect(path));
        relay(socket, writer, frame, again.as_ref().map(|f| f as &dyn Fn() -> mpsc::Receiver<FrameSocket>));
    });

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
    fn cuda_encode_only_moves_decoding_and_filters_to_the_cpu() {
        let real = args(&[
            "-threads",
            "1",
            "-init_hw_device",
            "cuda",
            "-hwaccel",
            "cuda",
            "-hwaccel_output_format",
            "cuda",
            "-i",
            "/m/a.mp4",
            "-f",
            "rawvideo",
            "-i",
            "pipe:6",
            "-filter_complex",
            "[1:0]split=2[vz_s0][vz_s1];[vz_s0]crop=1056:320:0:0,format=yuva420p,hwupload_cuda[vz_g0];[vz_s1]crop=1280:80:0:320,format=yuva420p,hwupload_cuda[vz_g1];[0:0]hwupload,fps=23620/1000,setsar=1,scale_cuda=format=yuv420p[v_m0];[v_m0]fps=25[v_m0_fps];[v_m0_fps][vz_g0]overlay_cuda=x=112:y=208[vz_o0];[vz_o0][vz_g1]overlay_cuda=x=0:y=640[v_o0]",
            "-map",
            "[v_o0]",
            "-vcodec",
            "h264_nvenc",
            "-b:v",
            "2000k",
            "-f",
            "hls",
            "o.m3u8",
        ]);
        let cpu = cuda_encode_only(real);
        assert_eq!(
            cpu,
            args(&[
                "-threads",
                "1",
                "-i",
                "/m/a.mp4",
                "-f",
                "rawvideo",
                "-i",
                "pipe:6",
                "-filter_complex",
                "[1:0]split=2[vz_s0][vz_s1];[vz_s0]crop=1056:320:0:0,format=yuva420p[vz_g0];[vz_s1]crop=1280:80:0:320,format=yuva420p[vz_g1];[0:0]fps=23620/1000,setsar=1,format=yuv420p[v_m0];[v_m0]fps=25[v_m0_fps];[v_m0_fps][vz_g0]overlay=x=112:y=208:format=auto[vz_o0];[vz_o0][vz_g1]overlay=x=0:y=640:format=auto[v_o0]",
                "-map",
                "[v_o0]",
                "-vcodec",
                "h264_nvenc",
                "-b:v",
                "2000k",
                "-f",
                "hls",
                "o.m3u8",
            ])
        );
        // scale with a size, a lone upload, deinterlacing
        let other = cuda_encode_only(args(&[
            "-hwaccel",
            "cuda",
            "-i",
            "a",
            "-vf",
            "[0:0]hwupload_cuda,yadif_cuda,scale_cuda=1920:1080:format=nv12:interp_algo=lanczos[v]",
            "-c:v",
            "hevc_nvenc",
            "o",
        ]));
        assert_eq!(other, args(&["-i", "a", "-vf", "[0:0]yadif,scale=1920:1080,format=nv12[v]", "-c:v", "hevc_nvenc", "o"]));
        assert_eq!(cpu_chain("[a]hwupload_cuda[b]").unwrap(), "[a]null[b]");
        // not NVENC, no CUDA, or an unknown CUDA filter: unchanged
        let x264 = args(&["-hwaccel", "cuda", "-i", "a", "-c:v", "libx264", "o"]);
        assert_eq!(cuda_encode_only(x264.clone()), x264);
        let plain = args(&["-i", "a", "-c:v", "h264_nvenc", "o"]);
        assert_eq!(cuda_encode_only(plain.clone()), plain);
        let unknown = args(&["-hwaccel", "cuda", "-i", "a", "-vf", "tonemap_cuda=x", "-c:v", "h264_nvenc", "o"]);
        assert_eq!(cuda_encode_only(unknown.clone()), unknown);
    }

    #[test]
    fn cuda_decode_on_cpu_keeps_filters_and_overlay_on_the_card() {
        let graph = "[1:0]split=2[vz_s0][vz_s1];[vz_s0]crop=1056:320:0:0,format=yuva420p,hwupload_cuda[vz_g0];\
                     [vz_s1]crop=1280:80:0:320,format=yuva420p,hwupload_cuda[vz_g1];\
                     [0:0]hwupload,fps=23620/1000,setsar=1,scale_cuda=format=yuv420p[v_m0];[v_m0]fps=25[v_m0_fps];\
                     [v_m0_fps][vz_g0]overlay_cuda=x=112:y=208[vz_o0];[vz_o0][vz_g1]overlay_cuda=x=0:y=640[v_o0]";
        let real = args(&[
            "-threads",
            "1",
            "-init_hw_device",
            "cuda",
            "-hwaccel",
            "cuda",
            "-hwaccel_output_format",
            "cuda",
            "-i",
            "/m/a.mp4",
            "-f",
            "rawvideo",
            "-i",
            "pipe:6",
            "-filter_complex",
            "[0:1]loudnorm=I=-23,apad[a]",
            "-filter_complex",
            graph,
            "-map",
            "[v_o0]",
            "-map",
            "[a]",
            "-vcodec",
            "h264_nvenc",
            "o.m3u8",
        ]);
        let placed = graph
            .replace("[0:0]hwupload,", &format!("[0:0]{UPLOAD_FORMATS},hwupload,"))
            .replace("overlay_cuda=x=0:y=640[v_o0]", &format!("overlay_cuda=x=0:y=640,hwdownload,{UPLOAD_FORMATS}[v_o0]"));
        assert_eq!(
            cuda_decode_on_cpu(real),
            args(&[
                "-threads",
                "1",
                "-init_hw_device",
                "cuda",
                "-i",
                "/m/a.mp4",
                "-f",
                "rawvideo",
                "-i",
                "pipe:6",
                "-filter_complex",
                "[0:1]loudnorm=I=-23,apad[a]",
                "-filter_complex",
                &placed,
                "-map",
                "[v_o0]",
                "-map",
                "[a]",
                "-vcodec",
                "h264_nvenc",
                "o.m3u8",
            ])
        );
        // CUDA filters straight on the decoded frames: an upload before the first, a download at the end
        let direct = args(&[
            "-hwaccel",
            "cuda",
            "-hwaccel_output_format",
            "cuda",
            "-i",
            "a",
            "-vf",
            "yadif_cuda,scale_cuda=1920:1080",
            "-c:v",
            "hevc_nvenc",
            "o",
        ]);
        assert_eq!(
            cuda_decode_on_cpu(direct),
            args(&[
                "-i",
                "a",
                "-vf",
                &format!("{UPLOAD_FORMATS},hwupload,yadif_cuda,scale_cuda=1920:1080,hwdownload,{UPLOAD_FORMATS}"),
                "-c:v",
                "hevc_nvenc",
                "o"
            ])
        );
        let mut card = Vec::new();
        // a download of decoded frames (already on the CPU) dropped, the later upload kept; not for the encoder: stays on the card
        assert_eq!(
            place_frames("[0:0]hwdownload,format=nv12,fps=25,hwupload_cuda[v]", &mut card, &[], false),
            format!("[0:0]format=nv12,fps=25,{UPLOAD_FORMATS},hwupload_cuda[v]")
        );
        assert_eq!(card, ["v"]);
        // the graphics (format before the upload) unchanged; a chain of labels on the card keeps them there
        assert_eq!(place_frames("[1:0]format=yuva420p,hwupload_cuda[g]", &mut card, &[], false), "[1:0]format=yuva420p,hwupload_cuda[g]");
        assert_eq!(place_frames("[v]fps=50[w]", &mut card, &[], false), "[v]fps=50[w]");
        assert_eq!(
            place_frames("[w][g]overlay_cuda=x=0:y=0[o]", &mut card, &["o"], false),
            format!("[w][g]overlay_cuda=x=0:y=0,hwdownload,{UPLOAD_FORMATS}[o]")
        );
        assert!(!card.contains(&"o".to_owned()));
        // not NVENC, or no CUDA decoding (ErsatzTV already uploads): unchanged
        let x264 = args(&["-hwaccel", "cuda", "-i", "a", "-c:v", "libx264", "o"]);
        assert_eq!(cuda_decode_on_cpu(x264.clone()), x264);
        let uploaded =
            args(&["-init_hw_device", "cuda", "-i", "a", "-vf", "hwupload_cuda,scale_cuda=1920:1080", "-c:v", "h264_nvenc", "o"]);
        assert_eq!(cuda_decode_on_cpu(uploaded.clone()), uploaded);
        assert_eq!((Cuda::parse(""), Cuda::parse(" Encode "), Cuda::parse("overlay")), (Cuda::Full, Cuda::Encode, Cuda::Overlay));
        assert_eq!(Cuda::parse("AUTO"), Cuda::Auto);
    }

    #[test]
    fn cuda_auto_falls_back_to_encode_on_a_full_card() {
        assert_eq!(parse_free_memory("439\n"), Some(439));
        assert_eq!(parse_free_memory(" 1200\n80\n"), Some(1200));
        assert_eq!(parse_free_memory("[N/A]\n"), None);
        assert_eq!(parse_free_memory(""), None);
        assert_eq!(auto_cuda(Some(299), 300), Cuda::Encode);
        assert_eq!(auto_cuda(Some(300), 300), Cuda::Overlay);
        assert_eq!(auto_cuda(None, 300), Cuda::Overlay);
        let nvenc = args(&["-hwaccel", "cuda", "-i", "a", "-c:v", "h264_nvenc", "o"]);
        assert!(nvenc_with_cuda(&nvenc));
        assert!(nvenc_with_cuda(&args(&["-i", "a", "-vf", "scale_cuda=8:4", "-vcodec", "h264_nvenc", "o"])));
        assert!(!nvenc_with_cuda(&args(&["-i", "a", "-c:v", "h264_nvenc", "o"])));
        assert!(!nvenc_with_cuda(&args(&["-hwaccel", "cuda", "-i", "a", "-c:v", "libx264", "o"])));
    }

    #[test]
    fn relay_keeps_whole_frames_and_comes_back() {
        use std::net::TcpListener;
        struct Tap(Arc<Mutex<Vec<u8>>>);
        impl Write for Tap {
            fn write(&mut self, data: &[u8]) -> io::Result<usize> {
                let mut seen = self.0.lock().unwrap();
                // Enough seen: ffmpeg is gone.
                if seen.len() > 4000 && seen.contains(&2) {
                    return Err(io::ErrorKind::BrokenPipe.into());
                }
                seen.extend_from_slice(data);
                Ok(data.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let first = TcpStream::connect(address).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        // One and a half frames of 4 bytes, then the sidecar is gone.
        server.write_all(&[1; 6]).unwrap();
        drop(server);
        let reconnect = || {
            let (sender, receiver) = mpsc::channel();
            let socket = TcpStream::connect(address).unwrap();
            let (mut back, _) = listener.accept().unwrap();
            back.write_all(&[2; 4]).unwrap();
            drop(back);
            sender.send(FrameSocket { stream: socket, delta: false }).unwrap();
            receiver
        };
        let seen = Arc::new(Mutex::new(Vec::new()));
        relay(FrameSocket { stream: first, delta: false }, Tap(Arc::clone(&seen)), 4, Some(&reconnect));
        let seen = seen.lock().unwrap();
        assert_eq!(&seen[..8], &[1, 1, 1, 1, 1, 1, 0, 0], "the broken frame is filled");
        let at = seen.iter().position(|b| *b == 2).unwrap();
        assert_eq!(at % 4, 0, "the frames stay aligned");
    }

    /// Delta records become whole frames for ffmpeg; a record broken off is
    /// not shown, and the stream after a reconnection starts with a whole frame.
    #[test]
    fn relay_rebuilds_whole_frames_from_delta_records() {
        use std::net::TcpListener;
        // a 2x2 frame: Y 4, U 1, V 1, A 4 bytes
        let (one, two) = ([1u8; 10], [1u8, 1, 1, 1, 1, 1, 1, 1, 7, 7]);
        let mut records = Vec::new();
        crate::delta::write_record(&mut records, None, &one, 2, 2).unwrap();
        crate::delta::write_record(&mut records, Some(&one), &two, 2, 2).unwrap();
        records.extend_from_slice(&crate::delta::UNCHANGED);
        records.extend_from_slice(&crate::delta::MAGIC[..]); // broken off
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let first = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        server.write_all(&records).unwrap();
        drop(server);
        let mut seen = Vec::new();
        relay(FrameSocket { stream: first, delta: true }, &mut seen, 10, None);
        assert_eq!(seen, [&one[..], &two, &two].concat());
    }

    #[test]
    fn zones_of_a_session_from_the_sidecar() {
        let zones = parse_zones(
            r#"{"rev":2,"zones":"448x96+1440+32,1920x216+0+864","width":1920,"height":312,
                "areas":[{"x":1440,"y":32,"w":448,"h":96},{"x":0,"y":864,"w":1920,"h":216}],"resume":"channel=1"}"#,
        )
        .unwrap();
        assert_eq!(zones.rev, 2);
        assert_eq!((zones.width, zones.height), (1920, 312));
        // stacked: the second part starts below the first
        assert_eq!(zones.areas[1], (Area { x: 0, y: 864, w: 1920, h: 216 }, 96));
        // the whole channel: no zones, one part at the origin
        let whole = parse_zones(r#"{"rev":3,"zones":null,"width":1920,"height":1080,"areas":[{"x":0,"y":0,"w":1920,"h":1080}]}"#).unwrap();
        assert_eq!((whole.geometry.as_str(), whole.areas.len()), ("", 1));
        assert!(parse_zones(r#"{"rev":1,"width":1,"height":1,"areas":[]}"#).is_err());
    }

    #[test]
    fn frames_declare_their_colour_matrix() {
        let input = args(&["-i", "media", "-i", "http://localhost/internal/graphics/4/1", "-f", "hls", "o"]);
        let canvas = Canvas { start: 2, index: 2, channel: "4".into() };
        let mut frames = Frames {
            width: 8,
            height: 4,
            rate: "25".into(),
            matrix: "bt709".into(),
            areas: vec![],
            resume: String::new(),
            session: "1".into(),
            ahead: None,
            zones_rev: None,
        };
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
    fn bframes_only_for_hardware_encoders_without_them() {
        let nvenc = args(&["-i", "a", "-vcodec", "h264_nvenc", "-b:v", "6000k", "o"]);
        assert_eq!(with_bframes(nvenc.clone(), "0"), args(&["-i", "a", "-vcodec", "h264_nvenc", "-bf:v", "0", "-b:v", "6000k", "o"]));
        assert_eq!(with_bframes(nvenc.clone(), ""), nvenc);
        assert_eq!(with_bframes(nvenc.clone(), "x"), nvenc);
        let own = args(&["-i", "a", "-c:v", "hevc_nvenc", "-bf", "2", "o"]);
        assert_eq!(with_bframes(own.clone(), "0"), own);
        let x264 = args(&["-i", "a", "-vcodec", "libx264", "o"]);
        assert_eq!(with_bframes(x264.clone(), "0"), x264);
        for hardware in ["h264_qsv", "h264_vaapi", "hevc_vaapi"] {
            assert_eq!(with_bframes(args(&["-c:v", hardware, "o"]), "1"), args(&["-c:v", hardware, "-bf:v", "1", "o"]));
        }
    }

    #[test]
    fn presets_of_each_encoder() {
        let run = |encoder: &str| with_presets(args(&["-i", "a", "-c:v", encoder, "o"]), "veryfast", "p5", "slow");
        assert_eq!(run("libx264"), args(&["-i", "a", "-c:v", "libx264", "-preset:v", "veryfast", "o"]));
        assert_eq!(run("hevc_nvenc"), args(&["-i", "a", "-c:v", "hevc_nvenc", "-preset:v", "p5", "o"]));
        assert_eq!(run("h264_qsv"), args(&["-i", "a", "-c:v", "h264_qsv", "-preset:v", "slow", "o"]));
        assert_eq!(run("h264_vaapi"), args(&["-i", "a", "-c:v", "h264_vaapi", "o"]));
        let nvenc = args(&["-i", "a", "-c:v", "h264_nvenc", "o"]);
        assert_eq!(with_presets(nvenc.clone(), "veryfast", "", "slow"), nvenc);
        let own = args(&["-i", "a", "-c:v", "h264_nvenc", "-preset", "p7", "o"]);
        assert_eq!(with_presets(own.clone(), "", "p1", ""), own);
    }

    #[test]
    fn shortest_goes_only_with_an_output_duration() {
        let full = args(&["-t", "44000ms", "-i", "m.mp4", "-i", "pipe:6", "-map", "[v]", "-t", "44000ms", "-shortest", "-f", "hls", "o"]);
        let without = args(&["-t", "44000ms", "-i", "m.mp4", "-i", "pipe:6", "-map", "[v]", "-t", "44000ms", "-f", "hls", "o"]);
        assert_eq!(without_shortest(full.clone(), true), without);
        assert_eq!(without_shortest(full.clone(), false), full);
        // No -t of the output (only of an input): -shortest ends it, it stays.
        let open = args(&["-t", "44000ms", "-i", "m.mp4", "-map", "[v]", "-shortest", "-f", "hls", "o"]);
        assert_eq!(without_shortest(open.clone(), true), open);
    }

    #[test]
    fn media_paced_only_in_canvas_transcodes_overlaid_on_the_card() {
        let canvas = |head: &[&str], filter: &str| {
            let mut all: Vec<&str> = head.to_vec();
            all.extend(["-i", "http://m", "-f", "rawvideo", "-i", "pipe:6", "-filter_complex", filter, "-c:v", "h264_nvenc", "o"]);
            args(&all)
        };
        let on_card = "[0:0]hwupload[m];[1:0]hwupload[g];[m][g]overlay_cuda[v]";
        assert_eq!(paced_media(canvas(&["-ss", "5ms"], on_card), None), canvas(&["-ss", "5ms", "-readrate", "1"], on_card));
        // Graphics ready ahead: the media bursts as far (at least ffmpeg's 0.5 s, at most 30 s).
        let burst = |seconds: &str| canvas(&["-readrate", "1", "-readrate_initial_burst", seconds], on_card);
        assert_eq!(paced_media(canvas(&[], on_card), Some(7.46)), burst("7.5"));
        assert_eq!(paced_media(canvas(&[], on_card), Some(-2.0)), burst("0.5"));
        assert_eq!(paced_media(canvas(&[], on_card), Some(99.0)), burst("30.0"));
        // Its own rate, the overlay on the CPU, or no raw graphics: unchanged.
        let own = canvas(&["-readrate", "1.0"], on_card);
        assert_eq!(paced_media(own.clone(), Some(4.0)), own);
        let cpu = canvas(&[], "[0:0][1:0]overlay[v]");
        assert_eq!(paced_media(cpu.clone(), Some(4.0)), cpu);
        let plain = args(&["-i", "http://m", "-filter_complex", on_card, "-c:v", "h264_nvenc", "o"]);
        assert_eq!(paced_media(plain.clone(), Some(4.0)), plain);
    }

    #[test]
    fn media_waits_for_the_graphics_in_ram_before_its_upload() {
        let canvas = |filter: &str| {
            args(&[
                "-init_hw_device",
                "cuda",
                "-readrate",
                "1",
                "-i",
                "/m/a.mp4",
                "-f",
                "rawvideo",
                "-i",
                "pipe:6",
                "-filter_complex",
                "[0:1]loudnorm=I=-23,apad[a]",
                "-filter_complex",
                filter,
                "-map",
                "[v_o0]",
                "-map",
                "[a]",
                "-vcodec",
                "h264_nvenc",
                "o.m3u8",
            ])
        };
        // ErsatzTV's graph after `cuda_decode_on_cpu` and two graphics areas (`placed_areas`).
        let graph = format!(
            "[1:0]split=2[vz_s0][vz_s1];[vz_s0]crop=1056:320:0:0,format=yuva420p,hwupload[vz_g0];\
             [vz_s1]crop=1280:80:0:320,format=yuva420p,hwupload[vz_g1];\
             [0:0]{UPLOAD_FORMATS},hwupload,fps=23620/1000,setsar=1,scale_cuda=format=yuv420p[v_m0];[v_m0]fps=25[v_m0_fps];\
             [v_m0_fps][vz_g0]overlay_cuda=x=112:y=208[vz_o0];[vz_o0][vz_g1]overlay_cuda=x=0:y=640,hwdownload,{UPLOAD_FORMATS}[v_o0]"
        );
        let synced = format!(
            "[1:0]split=2[vz_w0][vz_w1];[vz_w0]crop=2:2:0:0[vz_wt];\
             [vz_w1]split=2[vz_s0][vz_s1];[vz_s0]crop=1056:320:0:0,format=yuva420p,hwupload[vz_g0];\
             [vz_s1]crop=1280:80:0:320,format=yuva420p,hwupload[vz_g1];\
             [0:0][vz_wt]{RAM_SYNC},{UPLOAD_FORMATS},hwupload,fps=23620/1000,setsar=1,scale_cuda=format=yuv420p[v_m0];\
             [v_m0]fps=25[v_m0_fps];\
             [v_m0_fps][vz_g0]overlay_cuda=x=112:y=208[vz_o0];[vz_o0][vz_g1]overlay_cuda=x=0:y=640,hwdownload,{UPLOAD_FORMATS}[v_o0]"
        );
        assert_eq!(media_waits_in_ram(canvas(&graph), true), canvas(&synced));
        // Twice: the graph is already synced.
        assert_eq!(media_waits_in_ram(canvas(&synced), true), canvas(&synced));
        // One area, the graphics straight into the overlay.
        let direct = "[0:0]format=yuv420p,hwupload[m];[m][1:v]overlay_cuda=x=0:y=780,hwdownload,format=yuv420p[v_o0]";
        assert_eq!(
            media_waits_in_ram(canvas(direct), true),
            canvas(&format!(
                "[1:v]split=2[vz_w0][vz_w1];[vz_w0]crop=2:2:0:0[vz_wt];\
                 [0:0][vz_wt]{RAM_SYNC},format=yuv420p,hwupload[m];[m][vz_w1]overlay_cuda=x=0:y=780,hwdownload,format=yuv420p[v_o0]"
            ))
        );
        // Off, the overlay on the CPU, no graphics pipe, two uploading media chains, graphics used twice: unchanged.
        assert_eq!(media_waits_in_ram(canvas(&graph), false), canvas(&graph));
        let cpu = canvas("[0:0][1:0]overlay=x=0:y=0[v_o0]");
        assert_eq!(media_waits_in_ram(cpu.clone(), true), cpu);
        let no_pipe = args(&["-i", "a", "-i", "b", "-filter_complex", &graph, "-c:v", "h264_nvenc", "o"]);
        assert_eq!(media_waits_in_ram(no_pipe.clone(), true), no_pipe);
        let two = canvas("[0:0]hwupload[m];[2:0]hwupload[n];[m][1:0]overlay_cuda[o];[o][n]overlay_cuda[v_o0]");
        assert_eq!(media_waits_in_ram(two.clone(), true), two);
        let twice = canvas("[0:0]hwupload[m];[1:0]hwupload[g];[m][g]overlay_cuda[o];[o][1:0]overlay_cuda[v_o0]");
        assert_eq!(media_waits_in_ram(twice.clone(), true), twice);
    }

    #[test]
    fn graphics_upload_through_the_one_cuda_device() {
        let graph = "[1:0]split=2[vz_s0][vz_s1];[vz_s0]crop=8:2:0:0,format=yuva420p,hwupload_cuda[vz_g0];\
                     [vz_s1]crop=8:2:0:2,hwupload_cuda[vz_g1];[0:0]hwupload,scale_cuda=8:4[v];[v][vz_g0]overlay_cuda[o0];\
                     [o0][vz_g1]overlay_cuda[o]";
        let command = |device: &[&str], graph: &str| {
            let mut all: Vec<&str> = device.to_vec();
            all.extend(["-i", "a", "-i", "pipe:6", "-filter_complex", graph, "-c:v", "h264_nvenc", "o"]);
            args(&all)
        };
        let shared = graph.replace("hwupload_cuda", "hwupload");
        assert_eq!(shared_upload(command(&["-init_hw_device", "cuda"], graph)), command(&["-init_hw_device", "cuda"], &shared));
        assert_eq!(shared_upload(command(&["-init_hw_device", "cuda=gpu:0"], graph)), command(&["-init_hw_device", "cuda=gpu:0"], &shared));
        // No device of its own, two devices, or an upload with options: unchanged.
        assert_eq!(shared_upload(command(&[], graph)), command(&[], graph));
        let two = ["-init_hw_device", "cuda", "-init_hw_device", "vaapi"];
        assert_eq!(shared_upload(command(&two, graph)), command(&two, graph));
        let device = "[1:0]hwupload_cuda=device=1[g]";
        assert_eq!(shared_upload(command(&["-init_hw_device", "cuda"], device)), command(&["-init_hw_device", "cuda"], device));
        assert_eq!(
            shared_upload(command(&["-init_hw_device", "cuda"], "[1:0]hwupload_cuda,hwupload_cuda[g]")),
            command(&["-init_hw_device", "cuda"], "[1:0]hwupload,hwupload[g]")
        );
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
    fn x264_params_only_for_libx264() {
        let x264 = args(&["-i", "a", "-vcodec", "libx264", "-b:v", "1500k", "-f", "hls", "o"]);
        assert_eq!(
            with_x264_params(x264.clone(), "bframes=0:subme=0"),
            args(&["-i", "a", "-vcodec", "libx264", "-x264-params:v", "bframes=0:subme=0", "-b:v", "1500k", "-f", "hls", "o"])
        );
        assert_eq!(with_x264_params(x264.clone(), ""), x264);
        let own = args(&["-i", "a", "-c:v", "libx264", "-x264-params", "keyint=50", "o"]);
        assert_eq!(with_x264_params(own, "subme=0"), args(&["-i", "a", "-c:v", "libx264", "-x264-params", "keyint=50:subme=0", "o"]));
        for other in ["libx265", "h264_nvenc", "h264_vaapi"] {
            let command = args(&["-i", "a", "-c:v", other, "o"]);
            assert_eq!(with_x264_params(command.clone(), "subme=0"), command);
        }
    }

    #[test]
    fn profile_for_h264_encoders_without_one() {
        let x264 = args(&["-i", "a", "-c:v", "libx264", "o"]);
        assert_eq!(with_profile(x264.clone(), "main"), args(&["-i", "a", "-c:v", "libx264", "-profile:v", "main", "o"]));
        assert_eq!(with_profile(x264.clone(), ""), x264);
        let own = args(&["-i", "a", "-c:v", "libx264", "-profile:v", "high", "o"]);
        assert_eq!(with_profile(own.clone(), "main"), own);
        let nvenc = args(&["-i", "a", "-c:v", "h264_nvenc", "o"]);
        assert_eq!(with_profile(nvenc, "high"), args(&["-i", "a", "-c:v", "h264_nvenc", "-profile:v", "high", "o"]));
        let vaapi = args(&["-c:v", "h264_vaapi", "o"]);
        assert_eq!(with_profile(vaapi, "baseline"), args(&["-c:v", "h264_vaapi", "-profile:v", "constrained_baseline", "o"]));
        let hevc = args(&["-c:v", "hevc_nvenc", "o"]);
        assert_eq!(with_profile(hevc.clone(), "main"), hevc);
    }

    #[test]
    fn encoder_file_over_environment() {
        let dir = std::env::temp_dir().join(format!("vizrs-overlay-encoder-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(encoder::FILE);
        let env = || Settings { preset: "veryfast".into(), x264_params: "subme=0".into(), ..Settings::from_env().with_encoder_file("") };
        let missing = env().with_encoder_file(path.to_str().unwrap());
        assert_eq!((missing.preset.as_str(), missing.x264_params.as_str()), ("veryfast", "subme=0"));
        std::fs::write(&path, r#"{"preset":"superfast","profile":"main","x264_params":"keyint=1"}"#).unwrap();
        let set = env().with_encoder_file(path.to_str().unwrap());
        assert_eq!((set.preset.as_str(), set.profile.as_str(), set.x264_params.as_str()), ("superfast", "main", ""), "the file as a whole");
        let _ = std::fs::remove_dir_all(&dir);
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
