// Emission logs for VizRS Live's "Dziennik emisji" next to the as-run log: the
// files the sidecar, the live ingest listeners and the channel takeover write on
// the ErsatzTV config volume, read for one channel and one (UTC) day.
//
//   GET /api/live/logs?source=<cg|ingest|takeover>&channel=<n>&day=<YYYY-MM-DD>
//     {"source", "channel", "day", "tz": "UTC", "store", "truncated",
//      "entries": [{"t" (ms), "level": "info|warn|error", "text"}]}
//
// cg       <etv_config>/vizrs-live/logs/vizrs-cg.log      (src/log.rs): lines of the
//          channel ("channel N", "kanał N", "(channel N)") and lines of no channel
// ingest   <etv_config>/vizrs-live/logs/vizrs-ingest.log  (src/listener.rs): lines of the channel
// takeover <etv_config>/logs/vizrs-takeover-<n>.log       (src/takeover.rs, ETV_CONFIG_FOLDER)
//
// Each with its rotated `.1`. Stream keys and passphrases in URLs are masked: the
// log is for the operator's screen, and a key there is a key in a screenshot.

use std::path::PathBuf;

use crate::json::{self, Value};

pub const SOURCES: &[&str] = &["cg", "ingest", "takeover"];

/// At most this many entries per answer (the newest kept): a day of a busy
/// channel's takeover trace is tens of thousands of lines.
pub const MAX_ENTRIES: usize = 3000;

/// The files of a source, the older (rotated) first.
pub fn files(etv_config: &str, source: &str, channel: &str) -> Vec<PathBuf> {
    if etv_config.is_empty() {
        return Vec::new();
    }
    let base = PathBuf::from(etv_config);
    let file = match source {
        "cg" => base.join("vizrs-live").join("logs").join("vizrs-cg.log"),
        "ingest" => base.join("vizrs-live").join("logs").join("vizrs-ingest.log"),
        "takeover" => {
            let name: String = channel.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
            base.join("logs").join(format!("vizrs-takeover-{name}.log"))
        }
        _ => return Vec::new(),
    };
    let mut old = file.clone().into_os_string();
    old.push(".1");
    vec![PathBuf::from(old), file]
}

/// The entries of a source for a channel on a day: (entries, more than `MAX_ENTRIES`).
pub fn read(etv_config: &str, source: &str, channel: &str, day: &str) -> (Vec<Value>, bool) {
    let mut entries = Vec::new();
    for path in files(etv_config, source, channel) {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        for line in text.lines() {
            if !line.starts_with(day) {
                continue;
            }
            if let Some(entry) = parse(source, channel, line) {
                entries.push(entry);
            }
        }
    }
    let truncated = entries.len() > MAX_ENTRIES;
    if truncated {
        entries.drain(..entries.len() - MAX_ENTRIES);
    }
    (entries, truncated)
}

/// One log line as an entry, `None` for a line of another channel or without a time.
fn parse(source: &str, channel: &str, line: &str) -> Option<Value> {
    let (time, rest) = line.split_once(' ')?;
    let t = crate::time::parse_iso(time)?;
    let (level, text) = match source {
        "cg" => {
            let (name, text) = rest.split_once(' ').unwrap_or((rest, ""));
            let level = match name {
                "WARN" => "warn",
                "ERROR" => "error",
                "INFO" | "DEBUG" => "info",
                _ => return None,
            };
            let mentioned = channels_in(text);
            if !mentioned.is_empty() && !mentioned.iter().any(|c| c == channel) {
                return None;
            }
            (level, text.to_owned())
        }
        "ingest" => {
            let text = rest.strip_prefix("vizrs-ingest: ").unwrap_or(rest);
            let mentioned = channels_in(text);
            if !mentioned.is_empty() && !mentioned.iter().any(|c| c == channel) {
                return None;
            }
            (ingest_level(text), text.to_owned())
        }
        "takeover" => {
            // "pid 123 channel 1: … | seq …, listed …": the playlist state stays, it says what the channel had.
            let text = rest.strip_prefix("pid ").and_then(|r| r.split_once(' ')).map_or(rest, |(_, r)| r);
            (takeover_level(text), text.to_owned())
        }
        _ => return None,
    };
    Some(json::object([("t", (t as f64).into()), ("level", level.into()), ("text", mask(&text).into())]))
}

/// Channel numbers a line names: "channel 1", "channel 1:", "kanał 1", "(channel 1)".
fn channels_in(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for marker in ["channel ", "kanał ", "channel="] {
        let mut rest = text;
        while let Some(at) = rest.find(marker) {
            rest = &rest[at + marker.len()..];
            let number: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-').collect();
            let number = number.trim_end_matches(['.', '-']).to_owned();
            if !number.is_empty() && number.chars().next().is_some_and(|c| c.is_ascii_digit()) && !found.contains(&number) {
                found.push(number);
            }
        }
    }
    found
}

fn ingest_level(text: &str) -> &'static str {
    let lower = text.to_lowercase();
    if lower.contains("error") || lower.contains("failed") || lower.contains("refused") {
        "error"
    } else if lower.contains("gone") || lower.contains("cut off") || lower.contains("non-monotonic") || lower.contains("warn") {
        "warn"
    } else {
        "info"
    }
}

fn takeover_level(text: &str) -> &'static str {
    let lower = text.to_lowercase();
    if lower.contains("sigkill") || lower.contains("ffmpeg: ") || lower.contains("did not stop") {
        "error"
    } else if lower.contains("no picture")
        || lower.contains("behind the clock")
        || lower.contains("did not end")
        || lower.contains("sigterm")
        || lower.contains("schedule goes on")
    {
        "warn"
    } else {
        "info"
    }
}

/// Stream keys, passphrases and tokens of URLs replaced with `***`.
pub fn mask(text: &str) -> String {
    let mut out = text.to_owned();
    for marker in ["/live/", "/live2/", "/app/", "passphrase=", "streamid=", "token=", "key=", "password="] {
        let mut from = 0;
        while let Some(at) = out[from..].find(marker) {
            let start = from + at + marker.len();
            let end = out[start..]
                .find(|c: char| c.is_whitespace() || matches!(c, '&' | '"' | '\'' | ')' | ';'))
                .map_or(out.len(), |e| start + e);
            if end > start && &out[start..end] != "***" {
                out.replace_range(start..end, "***");
            }
            from = start + 3;
            if from >= out.len() {
                break;
            }
        }
    }
    // user:password@ in URLs
    let mut from = 0;
    while let Some(at) = out[from..].find("://") {
        let start = from + at + 3;
        let host_end = out[start..].find(|c: char| c.is_whitespace() || c == '/').map_or(out.len(), |e| start + e);
        if let Some(at_sign) = out[start..host_end].rfind('@') {
            out.replace_range(start..start + at_sign, "***");
        }
        from = start;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(v: &Value) -> String {
        v.get("text").and_then(Value::as_str).unwrap_or("").to_owned()
    }
    fn level(v: &Value) -> String {
        v.get("level").and_then(Value::as_str).unwrap_or("").to_owned()
    }

    #[test]
    fn lines_of_the_channel_and_the_day_masked() {
        let dir = std::env::temp_dir().join(format!("vizrs-logs-{}-{}", std::process::id(), crate::time::now_ms()));
        let logs = dir.join("vizrs-live").join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::create_dir_all(dir.join("logs")).unwrap();
        std::fs::write(
            logs.join("vizrs-cg.log"),
            "2026-11-13T23:59:59.000Z INFO yesterday\n\
             2026-11-14T00:09:19.590Z INFO ingest: channel 1: source connected\n\
             2026-11-14T00:09:20.000Z INFO ingest: channel 2: source connected\n\
             2026-11-14T00:10:00.000Z WARN graphics stream: late\n\
             2026-11-14T00:11:00.000Z INFO live API: VizRS Live: studio wpuszczone (WPUŚĆ) (channel 1)\n",
        )
        .unwrap();
        std::fs::write(
            logs.join("vizrs-ingest.log.1"),
            "2026-11-14T00:01:00.000Z vizrs-ingest: channel 1: listening on srt://0.0.0.0:9000?passphrase=secret-pass\n",
        )
        .unwrap();
        std::fs::write(
            logs.join("vizrs-ingest.log"),
            "2026-11-14T00:09:15.371Z vizrs-ingest: channel 1: listening on rtmp://0.0.0.0:1936/live/change-me\n\
             2026-11-14T00:09:16.000Z vizrs-ingest: channel 1: publisher gone; listening on rtmp://0.0.0.0:1936/live/change-me\n\
             2026-11-14T00:09:17.000Z vizrs-ingest: channel 4: publisher on rtmp://0.0.0.0:1936/live/k\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("logs").join("vizrs-takeover-1.log"),
            "2026-11-14T00:22:01.293Z pid 109256 channel 1: live run ended after 44.0 s in 41.4 s (1.06x, exit 0) | seq 11664, listed 11\n\
             2026-11-14T00:22:02.000Z pid 109256 channel 1: the live source gave no picture; the schedule goes on | seq 1\n",
        )
        .unwrap();
        let config = dir.to_string_lossy().to_string();

        let (cg, more) = read(&config, "cg", "1", "2026-11-14");
        assert!(!more);
        let texts: Vec<String> = cg.iter().map(text).collect();
        assert_eq!(
            texts,
            ["ingest: channel 1: source connected", "graphics stream: late", "live API: VizRS Live: studio wpuszczone (WPUŚĆ) (channel 1)"]
        );
        assert_eq!(level(&cg[1]), "warn");

        let (ingest, _) = read(&config, "ingest", "1", "2026-11-14");
        let texts: Vec<String> = ingest.iter().map(text).collect();
        assert_eq!(
            texts,
            [
                "channel 1: listening on srt://0.0.0.0:9000?passphrase=***",
                "channel 1: listening on rtmp://0.0.0.0:1936/live/***",
                "channel 1: publisher gone; listening on rtmp://0.0.0.0:1936/live/***"
            ]
        );
        assert_eq!(level(&ingest[2]), "warn");

        let (takeover, _) = read(&config, "takeover", "1", "2026-11-14");
        assert_eq!(text(&takeover[0]), "channel 1: live run ended after 44.0 s in 41.4 s (1.06x, exit 0) | seq 11664, listed 11");
        assert_eq!(level(&takeover[1]), "warn");
        assert!(read(&config, "takeover", "2", "2026-11-14").0.is_empty());
        assert!(read(&config, "other", "1", "2026-11-14").0.is_empty());
        assert!(read("", "cg", "1", "2026-11-14").0.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn secrets_are_masked() {
        assert_eq!(mask("rtmps://a.rtmp.youtube.com/live2/hy9f-y98p"), "rtmps://a.rtmp.youtube.com/live2/***");
        assert_eq!(mask("http://admin:quxAa@localhost:8080/memfs/x.ts"), "http://***@localhost:8080/memfs/x.ts");
        assert_eq!(mask("srt://h:9000?mode=caller&passphrase=abc&latency=1"), "srt://h:9000?mode=caller&passphrase=***&latency=1");
        assert_eq!(mask("no secrets here"), "no secrets here");
        assert_eq!(mask("ends with /live/"), "ends with /live/");
    }

    #[test]
    fn channels_named_in_a_line() {
        assert_eq!(channels_in("ingest: channel 1: live"), ["1"]);
        assert_eq!(channels_in("studio (channel 24)"), ["24"]);
        assert_eq!(channels_in("kanał 1.5 i channel 2"), ["2", "1.5"]);
        assert!(channels_in("graphics stream: late").is_empty());
    }
}
