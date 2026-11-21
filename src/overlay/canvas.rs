//! The canvas input of an ffmpeg command: channel, headers.

use super::*;

/// The canvas input of an ffmpeg command.
#[derive(Debug, PartialEq)]
pub struct Canvas {
    /// First option of the canvas input (after the previous `-i <input>`).
    pub start: usize,
    /// Index of its `-i`.
    pub index: usize,
    /// ErsatzTV channel from the canvas path.
    pub channel: String,
    /// ErsatzTV playout item from the canvas path (digits).
    pub item: String,
}

/// `/internal/graphics/<channel>/<item>`: the channel (still percent-encoded) and the playout item.
pub(crate) fn canvas_target(path: &str) -> Option<(&str, &str)> {
    let (channel, item) = path.strip_prefix("/internal/graphics/")?.split_once('/')?;
    let valid = !channel.is_empty() && !item.is_empty() && item.bytes().all(|b| b.is_ascii_digit());
    valid.then_some((channel, item))
}

/// `urllib.parse.unquote`: `%XX` only, `+` stays.
pub(crate) fn unquote(text: &str) -> String {
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
        if let Some((url, (channel, item))) = url.and_then(|u| canvas_target(u.path).map(|t| (u, t))).filter(|(u, _)| u.scheme == "http") {
            // A region canvas only carries watermarks: no VizRS element on the item.
            let region = url.query.split('&').any(|p| p.split_once('=').is_some_and(|(k, v)| unquote(k) == "region" && !v.is_empty()));
            if region {
                return None;
            }
            return Some(Canvas { start, index: i, channel: unquote(channel), item: item.to_owned() });
        }
        start = i + 2;
    }
    None
}

/// The region canvas of `args` (ErsatzTV draws only the item's watermarks on
/// it: an item with no VizRS element), as a canvas: the channel and item to ask
/// the bridge for the watermarks of (a live programme's own, src/overlay/run.rs);
/// its URL.
pub fn find_region_canvas(args: &[OsString]) -> Option<(Canvas, String)> {
    let mut start = 0;
    for i in 0..args.len().saturating_sub(1) {
        if args[i] != "-i" {
            continue;
        }
        let text = args[i + 1].to_str().unwrap_or("");
        if let Some((url, (channel, item))) = Url::split(text).and_then(|u| canvas_target(u.path).map(|t| (u, t))) {
            let region = url.query.split('&').any(|p| p.split_once('=').is_some_and(|(k, v)| unquote(k) == "region" && !v.is_empty()));
            if url.scheme == "http" && region {
                return Some((Canvas { start, index: i, channel: unquote(channel), item: item.to_owned() }, text.to_owned()));
            }
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
