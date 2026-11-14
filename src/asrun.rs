// As-run log (K14 of VizRS Live's PLAN-mcr): what actually went on the air of a
// channel — items starting and ending, pages taken and taken out, the studio
// taking a channel over and giving it back, all graphics off and back.
//
//   GET /api/live/asrun?channel=<n>&day=<YYYY-MM-DD>  {"entries":[{t, kind, …}], "day", "tz": "UTC"}
//
// One file per day, `<etv_config>/vizrs-live/asrun/asrun-YYYY-MM-DD.jsonl`, so a
// day is read without scanning everything and old days go away on their own
// (`KEEP_DAYS`). Days are UTC, as every time the sidecar writes; VizRS Live says
// so next to the date. Writing never blocks the air: errors only reach the log.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::RwLock;

use crate::json::{self, Value};
use crate::live_shows;
use crate::time;

/// Where alarms go (K17 of VizRS Live): `alert_webhook` / `VIZRS_ALERT_WEBHOOK`.
static WEBHOOK: RwLock<String> = RwLock::new(String::new());

/// The kinds worth waking somebody for: the signal gone, the studio held off,
/// all graphics off the air. VizRS Live shows its own alarms in the browser;
/// this is for everything outside it (a bell in the gallery, a message).
pub const ALARMS: &[&str] = &[LIVE_OFF, HOLD, GRAPHICS_OFF];

/// Sets the webhook of the alarms (at start; "" = none).
pub fn set_webhook(url: &str) {
    if let Ok(mut lock) = WEBHOOK.write() {
        *lock = url.trim().to_owned();
    }
}

fn webhook() -> String {
    WEBHOOK.read().map(|url| url.to_string()).unwrap_or_default()
}

/// An alarm leaves in the background: a slow or dead webhook must not hold the
/// sidecar up, and whatever happens the entry is already in the log.
fn alert(channel: &str, entry: &Entry, at_ms: i64) {
    if !ALARMS.contains(&entry.kind) {
        return;
    }
    let url = webhook();
    if url.is_empty() {
        return;
    }
    let text = [entry.title.as_str(), entry.detail.as_str(), entry.operator.as_str()].iter().find(|t| !t.is_empty()).copied();
    let body = json::object([
        ("kind", entry.kind.into()),
        ("channel", channel.into()),
        ("text", text.unwrap_or(entry.kind).into()),
        ("t", (at_ms as f64).into()),
    ])
    .to_string();
    let _ = std::thread::Builder::new().name("asrun-alert".into()).spawn(move || {
        if let Err(e) = crate::net::post(&url, "Content-Type: application/json\r\n", &body) {
            crate::warn!("alert webhook: {e}");
        }
    });
}

/// Days of the as-run log kept on disk.
pub const KEEP_DAYS: i64 = 30;
const DAY_MS: i64 = 86_400_000;

/// What happened. The panel knows these kinds; unknown ones it shows as they are.
pub const ITEM_START: &str = "item_start";
pub const ITEM_END: &str = "item_end";
pub const TAKE: &str = "take";
pub const CONTINUE: &str = "continue";
pub const OUT: &str = "out";
pub const HOLD: &str = "hold";
pub const RELEASE: &str = "release";
pub const LIVE_ON: &str = "live_on";
pub const LIVE_OFF: &str = "live_off";
pub const GRAPHICS_OFF: &str = "graphics_off";
pub const GRAPHICS_ON: &str = "graphics_on";
/// A live takeover holds the schedule's graphics (and puts `on_live` on air) · gives them back.
pub const LIVE_GRAPHICS_ON: &str = "live_graphics_on";
pub const LIVE_GRAPHICS_OFF: &str = "live_graphics_off";

/// One line of the log; empty fields are left out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    pub kind: &'static str,
    pub title: String,
    pub page: String,
    pub layer: Option<f64>,
    pub operator: String,
    pub detail: String,
}

impl Entry {
    pub fn of(kind: &'static str) -> Entry {
        Entry { kind, ..Entry::default() }
    }

    pub fn title(mut self, title: impl Into<String>) -> Entry {
        self.title = title.into();
        self
    }

    pub fn page(mut self, page: impl Into<String>) -> Entry {
        self.page = page.into();
        self
    }

    pub fn layer(mut self, layer: Option<f64>) -> Entry {
        self.layer = layer;
        self
    }

    pub fn operator(mut self, operator: impl Into<String>) -> Entry {
        self.operator = operator.into();
        self
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Entry {
        self.detail = detail.into();
        self
    }

    fn to_json(&self, at_ms: i64) -> Value {
        let mut members = vec![
            ("t".to_owned(), (at_ms as f64).into()),
            ("at".to_owned(), time::iso_time(at_ms).into()),
            ("kind".to_owned(), self.kind.into()),
        ];
        for (name, text) in [("title", &self.title), ("page", &self.page), ("operator", &self.operator), ("detail", &self.detail)] {
            if !text.is_empty() {
                members.push((name.to_owned(), text.as_str().into()));
            }
        }
        if let Some(layer) = self.layer {
            members.push(("layer".to_owned(), layer.into()));
        }
        Value::Object(members)
    }
}

/// `YYYY-MM-DD` of a moment, UTC (the first ten characters of its ISO time).
pub fn day_of(at_ms: i64) -> String {
    time::iso_time(at_ms).chars().take(10).collect()
}

/// A day as `YYYY-MM-DD`: four digits, `-`, two, `-`, two.
pub fn valid_day(day: &str) -> bool {
    let b = day.as_bytes();
    b.len() == 10 && b[4] == b'-' && b[7] == b'-' && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

pub fn folder(etv_config: &str) -> PathBuf {
    live_shows::folder(etv_config).join("asrun")
}

fn file_of(etv_config: &str, channel: &str, day: &str) -> PathBuf {
    let _ = channel;
    folder(etv_config).join(format!("asrun-{day}.jsonl"))
}

/// Appends one entry for the channel. Without `etv_config` there is nowhere to
/// write, so nothing is: the log is a record, never a reason to stop the air.
pub fn log(etv_config: &str, channel: &str, entry: &Entry) {
    let at = time::now_ms();
    if etv_config.is_empty() {
        // Nowhere to write the day's file, but an alarm still goes out.
        alert(channel, entry, at);
        return;
    }
    let day = day_of(at);
    let path = file_of(etv_config, channel, &day);
    let mut value = entry.to_json(at);
    if let Value::Object(members) = &mut value {
        members.insert(3, ("channel".to_owned(), channel.into()));
    }
    let written = fs::create_dir_all(folder(etv_config))
        .and_then(|()| OpenOptions::new().create(true).append(true).open(&path))
        .and_then(|mut file| writeln!(file, "{value}"));
    if let Err(e) = written {
        crate::warn!("as-run log: {e}");
    }
    alert(channel, entry, at);
    sweep(etv_config, at);
}

/// Days older than `KEEP_DAYS` go away (once a day, when the first entry of a day is written).
fn sweep(etv_config: &str, now_ms: i64) {
    let oldest = day_of(now_ms - KEEP_DAYS * DAY_MS);
    let Ok(entries) = fs::read_dir(folder(etv_config)) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(day) = name.strip_prefix("asrun-").and_then(|n| n.strip_suffix(".jsonl")) else { continue };
        if valid_day(day) && day < oldest.as_str() {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// The entries of one channel on one day, oldest first.
pub fn read(etv_config: &str, channel: &str, day: &str) -> Vec<Value> {
    if etv_config.is_empty() || !valid_day(day) {
        return Vec::new();
    }
    let Ok(text) = fs::read_to_string(file_of(etv_config, channel, day)) else { return Vec::new() };
    text.lines()
        .filter_map(|line| json::parse(line).ok())
        .filter(|value| value.get("channel").and_then(Value::as_str).unwrap_or("") == channel)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        // A folder of its own per test: tests run at the same time, sometimes in the same millisecond.
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("vizrs-asrun-{}-{}-{n}", std::process::id(), time::now_ms()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn days_and_their_shape() {
        assert_eq!(day_of(1_792_865_700_123), "2026-10-24");
        assert!(valid_day("2026-11-10"));
        assert!(!valid_day("2026-11-9") && !valid_day("2026/11/10") && !valid_day("x&y=1"));
    }

    #[test]
    fn written_entries_come_back_for_their_channel() {
        let dir = temp();
        let config = dir.to_string_lossy().into_owned();
        log(&config, "24", &Entry::of(ITEM_START).title("Serwis"));
        log(&config, "24", &Entry::of(TAKE).page("101").layer(Some(10.0)).operator("VizRS Live · MCR"));
        log(&config, "25", &Entry::of(ITEM_START).title("Teledyski"));
        let today = day_of(time::now_ms());

        eprintln!("DBG {:?}", std::fs::read_to_string(folder(&config).join(format!("asrun-{today}.jsonl"))));
        let entries = read(&config, "24", &today);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].get("kind").and_then(Value::as_str), Some(ITEM_START));
        assert_eq!(entries[0].get("title").and_then(Value::as_str), Some("Serwis"));
        assert_eq!(entries[1].get("page").and_then(Value::as_str), Some("101"));
        assert_eq!(entries[1].get("layer").and_then(Value::as_f64), Some(10.0));
        assert_eq!(read(&config, "25", &today).len(), 1);
        assert!(read(&config, "24", "2000-01-01").is_empty());
        // No etv_config: nothing is written anywhere.
        log("", "24", &Entry::of(ITEM_START));
        assert!(read("", "24", &today).is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn old_days_go_away() {
        let dir = temp();
        let config = dir.to_string_lossy().into_owned();
        fs::create_dir_all(folder(&config)).unwrap();
        let old = folder(&config).join("asrun-2000-01-01.jsonl");
        fs::write(&old, "{}\n").unwrap();
        log(&config, "24", &Entry::of(ITEM_START));
        assert!(!old.exists(), "a day older than KEEP_DAYS is removed");
        assert!(folder(&config).join(format!("asrun-{}.jsonl", day_of(time::now_ms()))).exists());
        let _ = fs::remove_dir_all(dir);
    }
}
