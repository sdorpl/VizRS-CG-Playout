//! ffmpeg arguments: graphics position, frame rate and blending, presets, profile, x264 parameters, B-frames, catch-up.

use super::*;

pub(crate) fn pair(args: &[OsString], i: usize, a: &str, b: &str) -> bool {
    args.get(i).is_some_and(|x| x == a) && args.get(i + 1).is_some_and(|y| y == b)
}

pub(crate) fn media_rate_filter() -> &'static Regex {
    static RATE: OnceLock<Regex> = OnceLock::new();
    // `fps=<rate>` first in a chain of an input's video: `[0:0]fps=25,scale…`.
    RATE.get_or_init(|| Regex::new(r"(^|;)(\[\d+(?::v)?(?::0)?\])fps=([0-9./]+)([,;\[])").expect("rate pattern"))
}

pub(crate) fn overlay_filter() -> &'static Regex {
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

/// One frame of `rate` (`25`, `30000/1001`); None for no rate.
pub(crate) fn frame_interval(rate: &str) -> Option<Duration> {
    parse_rate(rate).map(|fps| Duration::from_secs_f64(1.0 / fps))
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
pub(crate) fn source_rate(settings: &Settings, input: &str) -> Option<f64> {
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
pub(crate) fn after_encoder(args: Vec<OsString>, flag: &str, value: impl Fn(&str) -> Option<String>) -> Vec<OsString> {
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

/// `VIZRS_FFMPEG_NO_DISCARDCORRUPT=1`: a stream copy (`-c copy`, `-c:v copy`…)
/// of an HLS playlist input (`….m3u8`) loses `discardcorrupt` from the
/// `-fflags` of that input; `-fflags` left empty goes. The joins of the
/// pieces of a channel reset the MPEG-TS continuity counters, so the last
/// frame of each piece was flagged corrupt and dropped. Any other command
/// unchanged.
pub fn keeping_corrupt(args: Vec<OsString>, on: bool) -> Vec<OsString> {
    let copy = args.windows(2).any(|w| COPY.iter().any(|(flag, value)| w[0] == *flag && w[1] == *value));
    let Some(input) = args
        .windows(2)
        .position(|w| w[0] == "-i" && w[1].to_str().is_some_and(|i| i.split('?').next().is_some_and(|p| p.ends_with(".m3u8"))))
    else {
        return args;
    };
    if !on || !copy {
        return args;
    }
    // The `-fflags` of that input: after the input before it, before its `-i`.
    let start = args[..input].iter().rposition(|a| a == "-i").map_or(0, |i| i + 2);
    let Some(at) = (start..input).find(|&i| args[i] == "-fflags" && i + 1 < input) else { return args };
    let Some(flags) = args[at + 1].to_str() else { return args };
    // Only flags set with `+` (as ErsatzTV gives them): anything else unchanged.
    let Some(set) = flags.strip_prefix('+').filter(|f| !f.contains('-')) else { return args };
    if !set.split('+').any(|f| f == "discardcorrupt") {
        return args;
    }
    let kept: String = set.split('+').filter(|f| !f.is_empty() && *f != "discardcorrupt").map(|f| format!("+{f}")).collect();
    let mut out = args;
    if kept.is_empty() {
        out.drain(at..at + 2);
    } else {
        out[at + 1] = kept.into();
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

/// The command with the forced rate, blending, preset, profile, x264 options, B-frames and catch-up
/// rate, when set; `burst` = seconds of graphics ready at once (`paced_media`).
pub(crate) fn adjusted(args: Vec<OsString>, settings: &Settings, burst: Option<f64>) -> Vec<OsString> {
    let cuda = match settings.cuda {
        Cuda::Auto if nvenc_with_cuda(&args) => auto_cuda(free_gpu_memory(), settings.cuda_min_free),
        Cuda::Auto => Cuda::Full,
        mode => mode,
    };
    let args = match cuda {
        Cuda::Full | Cuda::Auto => cpu_synced_in_ram(args, settings.sync_ram),
        Cuda::Encode => cpu_synced_in_ram(cuda_encode_only(args), settings.sync_ram),
        Cuda::Overlay => overlay_paced(cuda_decode_on_cpu(args), burst, settings.sync_ram),
    };
    let rated = with_rate(args, &settings.fps);
    let source = if settings.blend { media_input(&rated).and_then(|input| source_rate(settings, &input)) } else { None };
    // After every graph rewrite: the media meets the graphics from frame 0 of the channel's rate.
    let blended = media_on_graphics_grid(with_blend(rated, settings.blend, source));
    // Each goes right after the encoder: the last inserted comes first (preset, profile, options).
    let encoded = with_presets(
        with_profile(with_x264_params(blended, &settings.x264_params), &settings.profile),
        &settings.preset,
        &settings.nvenc_preset,
        &settings.qsv_preset,
    );
    let tuned = with_catchup(with_bframes(encoded, &settings.bframes), &settings.catchup);
    keeping_corrupt(without_shortest(tuned, settings.no_shortest), settings.no_discardcorrupt)
}
