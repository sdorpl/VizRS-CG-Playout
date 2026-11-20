//! Media synchronised with the graphics in RAM, and the graphics frame grid.

use super::*;

/// `VIZRS_FFMPEG_SYNC_RAM=1` with the graphics overlaid on the CPU (`overlay`,
/// S1/S2 on Ampere): the media input loses its `-readrate` (and burst and
/// catch-up rate), as a graph synced in RAM on the card. `overlay` already
/// holds the media frames in RAM until the graphics of their moment come, so
/// the graphics of the sidecar (at most `lead_seconds` ahead of the clock)
/// pace the transcode. With ErsatzTV's `-readrate 1.0` an item started late
/// (the channel's timeline 10–15 s behind the clock, seconds to open the
/// media) never made it up: the overlay read the graphics 17–22 s after their
/// moment, older than the history, and they jumped 0.5–4.9 s (S1,
/// 2026-11-13). The sidecar gone, `relay` paces its transparent frames at the
/// channel's rate, so the media does not race ahead behind them. Any other
/// command unchanged: off, no graphics pipe, no CPU `overlay` reading it, a
/// graph on a card (`overlay_cuda`, `overlay_vaapi`, `overlay_qsv`).
pub fn cpu_synced_in_ram(args: Vec<OsString>, on: bool) -> Vec<OsString> {
    if !on {
        return args;
    }
    let text: Option<Vec<&str>> = args.iter().map(|a| a.to_str()).collect();
    let Some(text) = text else { return args };
    let inputs: Vec<usize> = (0..text.len().saturating_sub(1)).filter(|&i| text[i] == "-i").collect();
    let Some(graphics) = inputs.iter().position(|&i| text[i + 1].starts_with("pipe:")) else { return args };
    let Some(media) = inputs.iter().position(|&i| !text[i + 1].starts_with("pipe:")) else { return args };
    let graph = (0..text.len().saturating_sub(1)).find(|&i| text[i] == "-filter_complex" && text[i + 1].contains(&format!("[{graphics}:")));
    let Some(graph) = graph.map(|i| text[i + 1]) else { return args };
    let filters: Vec<&str> = graph
        .split([';', ','])
        .map(|f| {
            let f = &f[leading_labels(f)..];
            f.split(['=', '[']).next().unwrap_or("")
        })
        .collect();
    if !filters.contains(&"overlay") || filters.iter().any(|f| f.starts_with("overlay_")) {
        return args;
    }
    let from = if media == 0 { 0 } else { inputs[media - 1] + 2 };
    let to = inputs[media];
    let pacing = ["-readrate", "-readrate_initial_burst", "-readrate_catchup"];
    let mut out = Vec::with_capacity(args.len());
    let mut i = 0;
    while i < args.len() {
        if (from..to).contains(&i) && i + 1 < to && pacing.contains(&text[i]) {
            i += 2;
            continue;
        }
        out.push(args[i].clone());
        i += 1;
    }
    out
}

/// The filters that hold a media frame until the graphics of its moment are
/// there: a 2x2 cut of the graphics overlaid outside the picture (nothing
/// drawn), in the media's pixel format, but without alpha – `auto` offers
/// `yuva420p` too, which `scale_cuda` does not take (ffmpeg 8.1 of ErsatzTV);
/// after the graphics end the media goes on alone.
pub(crate) const RAM_SYNC: &str =
    "overlay=x=main_w:y=main_h:format=auto:eof_action=pass,format=nv12|yuv420p|p010le|yuv444p|p016le|yuv444p16le";

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
pub(crate) fn graph_waiting_in_ram(graph: &str, graphics: usize) -> Option<String> {
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
            // Watermarks (`[vz_kN]`, src/overlay/watermark.rs) are images, not the media.
            stream.is_some_and(|n| n != graphics) && uploads(body) && !chain.contains("[vz_k")
        })
        .collect();
    let [media] = media.as_slice() else { return None };
    let chain = &chains[*media];
    let (labels, body) = chain.split_at(leading_labels(chain));
    chains[*media] = format!("{labels}[vz_wt]{RAM_SYNC},{body}");
    let joined = chains.join(";").replacen(&label, "[vz_w1]", 1);
    Some(format!("{label}split=2[vz_w0][vz_w1];[vz_w0]crop=2:2:0:0[vz_wt];{joined}"))
}

/// A command overlaying raw graphics from a pipe (`-framerate R -i pipe:N`):
/// the media meets the graphics on their frame grid from 0 – the main input
/// of the graph's first overlay (in the channel's rate by then) goes through
/// `fps=R:start_time=0` (`[vz_grid]`). A seek (`-ss`) lands the media's first
/// frame a little after 0 (0.003–0.024 s measured, 2026-11-13); ErsatzTV's
/// `fps` put it on frame 1, the overlay gave it the graphics of frame 1 and
/// the output (`-fps_mode cfr`) filled frame 0 with a copy of it: at every
/// 44 s piece the graphics stepped one frame ahead, then stood for one (a
/// crawl jerked). Filled before the overlay, frame 0 takes the graphics of
/// frame 0; the media's frames keep their times (no shift against the
/// sound). Any other command unchanged: no graphics pipe or rate, no overlay
/// with two inputs in the graph reading the graphics.
pub fn media_on_graphics_grid(args: Vec<OsString>) -> Vec<OsString> {
    let text: Option<Vec<&str>> = args.iter().map(|a| a.to_str()).collect();
    let Some(text) = text else { return args };
    // Inputs in order: index of their `-i`, their value.
    let inputs: Vec<(usize, &str)> = text.windows(2).enumerate().filter(|(_, w)| w[0] == "-i").map(|(i, w)| (i, w[1])).collect();
    let Some(graphics) = inputs.iter().position(|(_, input)| input.starts_with("pipe:")) else { return args };
    let options_from = if graphics == 0 { 0 } else { inputs[graphics - 1].0 + 2 };
    let rate = text[options_from..inputs[graphics].0].windows(2).find(|w| w[0] == "-framerate").map(|w| w[1]);
    let Some(rate) = rate.filter(|r| !r.is_empty() && r.bytes().all(|b| b.is_ascii_digit() || b == b'/' || b == b'.')) else { return args };
    let Some(at) = (0..text.len().saturating_sub(1))
        .find(|&i| text[i] == "-filter_complex" && text[i + 1].contains(&format!("[{graphics}:")) && !text[i + 1].contains("[vz_grid]"))
    else {
        return args;
    };
    let mut chains: Vec<String> = text[at + 1].split(';').map(str::to_owned).collect();
    // The first overlay of two inputs: its first one is the media.
    let first = chains.iter().position(|chain| {
        let (labels, body) = chain.split_at(leading_labels(chain));
        labels.matches('[').count() == 2 && body.split([',', '=']).next().is_some_and(|f| f.starts_with("overlay"))
    });
    let Some(first) = first else { return args };
    let main = chains[first][..chains[first].find(']').map_or(0, |end| end + 1)].to_owned();
    chains[first] = chains[first].replacen(main.as_str(), "[vz_grid]", 1);
    let gridded = format!("{main}fps={rate}:start_time=0[vz_grid];{}", chains.join(";"));
    let mut out = args;
    out[at + 1] = gridded.into();
    out
}

/// ffmpeg's own initial burst of `-readrate`.
pub(crate) const MIN_BURST: f64 = 0.5;

/// More would be a wrong clock (the sidecar takes an air time at most 10 s behind).
pub(crate) const MAX_BURST: f64 = 30.0;
