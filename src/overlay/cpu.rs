//! The filter graph on the CPU: chains, scaling, overlaying, media pacing.

use super::*;

/// A filter graph with its CUDA filters as CPU filters; None = cannot tell.
pub(crate) fn cpu_filters(graph: &str) -> Option<String> {
    let chains: Vec<String> = graph.split(';').map(cpu_chain).collect::<Option<_>>()?;
    Some(chains.join(";"))
}

/// One chain `[in]f1,f2[out]`: uploads dropped, CUDA filters replaced.
pub(crate) fn cpu_chain(chain: &str) -> Option<String> {
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
pub(crate) fn leading_labels(chain: &str) -> usize {
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
pub(crate) fn trailing_labels(chain: &str) -> usize {
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
pub(crate) fn cpu_scale(options: &str) -> Vec<String> {
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
pub(crate) fn cpu_overlay(options: &str) -> String {
    let mut all: Vec<&str> = options.split(':').filter(|o| !o.is_empty()).collect();
    if !all.iter().any(|o| o.starts_with("format=")) {
        all.push("format=auto");
    }
    format!("overlay={}", all.join(":"))
}

/// Options of ErsatzTV's CUDA decoding left out with `VIZRS_FFMPEG_CUDA=overlay` (with their value).
pub(crate) const DECODE_OPTIONS: [&str; 3] = ["-hwaccel", "-hwaccel_output_format", "-hwaccel_device"];

/// Before an upload of decoded frames: the formats a CUDA device takes (the
/// decoder's own, e.g. yuv420p10le, becomes the nearest of them).
pub(crate) const UPLOAD_FORMATS: &str = "format=nv12|yuv420p|yuva420p|p010le|yuv444p|p016le|yuv444p16le";

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

/// The media of a `Cuda::Overlay` command: with `sync_ram` and a graph it
/// syncs in RAM (`media_waits_in_ram`), read as fast as the graphics come –
/// no `-readrate`: frames decoded ahead wait in RAM, and the graphics of the
/// sidecar (at most `lead_seconds` ahead of the clock) pace the piece. Its
/// burst never made up a piece's start: a seek reads from the keyframe before
/// it (up to a GOP, 5 s) and that counts against the burst, so the channel
/// settled about 8 s behind the schedule, and the overlay read the first
/// frames of each piece 16–21 s after their moment – older than the
/// history, the graphics jumped (entry animations cut). Otherwise
/// `paced_media`.
pub(crate) fn overlay_paced(args: Vec<OsString>, burst: Option<f64>, sync_ram: bool) -> Vec<OsString> {
    let synced = media_waits_in_ram(args.clone(), sync_ram);
    if synced != args {
        return synced;
    }
    paced_media(args, burst)
}
