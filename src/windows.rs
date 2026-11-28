// Emergency live windows (K23 of VizRS Live's PLAN-mcr): a live broadcast
// announced the same day, at an hour the schedule already fills. ErsatzTV and
// its schedule stay as they are (variant B); within the window the channel
// behaves as with `anytime` and without `hold`: a source takes the antenna
// over, and only an explicit hold from the operator keeps it off. After the
// end the channel is back to what it was before the window.
//
//   GET    /api/live/channels/<n>/windows        {"windows": […], "now"}
//   POST   /api/live/channels/<n>/windows        {"start", "end", "input", "title", "under", "keep"}
//   DELETE /api/live/channels/<n>/windows/<id>   removes it (an active one ends at once)
//
// `start` and `end` are ISO times with an offset (`2026-11-28T22:00:00+01:00`,
// `…Z`) or epoch milliseconds. Windows live in `control.json` with the
// decisions on channels (src/panel.rs), so a restart before the window does
// not lose it; windows already over go away when read – except those that keep
// the episode (K24): VizRS Live put a one-day alternate schedule in ErsatzTV for
// them (`keep`) and removes it the day after; until it does (at most
// `KEEP_FOR_MS` after the end) the window stays on the list as `over`.

use std::sync::atomic::{AtomicU16, Ordering};

use crate::json::{self, Value};

/// Most windows kept, over all channels.
pub const MAX_WINDOWS: usize = 100;
/// Longest window.
pub const MAX_LENGTH_MS: i64 = 24 * 3_600_000;
/// A start this far in the past is still accepted (the form filled in at 21:59 for 22:00…).
pub const PAST_SLACK_MS: i64 = 60_000;
/// A window starts at most this far ahead.
pub const MAX_AHEAD_MS: i64 = 30 * 86_400_000;
/// Longest title (characters).
pub const TITLE_MAX: usize = 200;
/// A window over that kept the episode stays this long for VizRS Live to clean ErsatzTV up.
pub const KEEP_FOR_MS: i64 = 7 * 86_400_000;
/// Largest id of ErsatzTV (playout, schedule): a 32-bit integer.
const MAX_ETV_ID: f64 = 2_147_483_647.0;

/// What VizRS Live put in ErsatzTV so that the episode under the window airs
/// next time (K24, variant W1): a copy of the day's schedule without the item,
/// as the playout's alternate schedule for that one date.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keep {
    pub playout: u32,
    /// The copied schedule (and its alternate entry) to remove afterwards.
    pub schedule: u32,
    /// The local date of the alternate (`YYYY-MM-DD`).
    pub date: String,
}

impl Keep {
    fn to_value(&self) -> Value {
        json::object([
            ("playout", f64::from(self.playout).into()),
            ("schedule", f64::from(self.schedule).into()),
            ("date", self.date.as_str().into()),
        ])
    }

    fn from_value(value: &Value) -> Option<Keep> {
        let id = |name: &str| {
            value.get(name).and_then(Value::as_f64).filter(|n| n.fract() == 0.0 && *n >= 1.0 && *n <= MAX_ETV_ID).map(|n| n as u32)
        };
        let date = value.get("date").and_then(Value::as_str).filter(|d| crate::asrun::valid_day(d))?;
        Some(Keep { playout: id("playout")?, schedule: id("schedule")?, date: date.to_owned() })
    }
}

/// What happens to the schedule's item under the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Under {
    /// The item plays under the window and its episode is lost (ErsatzTV unchanged).
    Replace,
    /// The episode goes in the next airing (K24: done by VizRS Live in ErsatzTV).
    Keep,
}

impl Under {
    pub fn as_str(self) -> &'static str {
        match self {
            Under::Replace => "replace",
            Under::Keep => "keep",
        }
    }

    fn parse(text: &str) -> Option<Under> {
        match text {
            "replace" => Some(Under::Replace),
            "keep" => Some(Under::Keep),
            _ => None,
        }
    }
}

/// One emergency live window of a channel.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub id: String,
    pub channel: String,
    pub start_ms: i64,
    pub end_ms: i64,
    /// The input the channel takes in the window (`listener::input_id`); None = the channel's choice.
    pub input: Option<String>,
    pub title: String,
    pub under: Under,
    pub created_by: String,
    /// With `Under::Keep`: what VizRS Live put in ErsatzTV for it.
    pub keep: Option<Keep>,
}

impl Window {
    pub fn active(&self, now_ms: i64) -> bool {
        self.start_ms <= now_ms && now_ms < self.end_ms
    }

    pub fn over(&self, now_ms: i64) -> bool {
        self.end_ms <= now_ms
    }

    /// Kept on the list: not over, or over but still with ErsatzTV to clean up.
    pub fn kept(&self, now_ms: i64) -> bool {
        !self.over(now_ms) || (self.keep.is_some() && now_ms < self.end_ms + KEEP_FOR_MS)
    }

    /// For the API and `control.json`; `active` only for the API (`now`).
    pub fn to_value(&self, now_ms: Option<i64>) -> Value {
        let mut members = vec![
            ("id".to_owned(), self.id.as_str().into()),
            ("channel".to_owned(), self.channel.as_str().into()),
            ("start".to_owned(), (self.start_ms as f64).into()),
            ("end".to_owned(), (self.end_ms as f64).into()),
            ("input".to_owned(), self.input.as_deref().map_or(Value::Null, Value::from)),
            ("title".to_owned(), self.title.as_str().into()),
            ("under".to_owned(), self.under.as_str().into()),
            ("created_by".to_owned(), self.created_by.as_str().into()),
            ("keep".to_owned(), self.keep.as_ref().map_or(Value::Null, Keep::to_value)),
        ];
        if let Some(now) = now_ms {
            members.push(("over".to_owned(), self.over(now).into()));
            members.push(("start_at".to_owned(), crate::time::iso_time(self.start_ms).into()));
            members.push(("end_at".to_owned(), crate::time::iso_time(self.end_ms).into()));
            members.push(("active".to_owned(), self.active(now).into()));
        }
        Value::Object(members)
    }

    /// A window read back from `control.json`; None for one that does not hold together.
    pub fn from_value(value: &Value) -> Option<Window> {
        let text = |name: &str| value.get(name).and_then(Value::as_str).map(str::to_owned);
        let window = Window {
            id: text("id").filter(|id| valid_id(id))?,
            channel: text("channel").filter(|c| valid_channel(c))?,
            start_ms: value.get("start").and_then(Value::as_f64).filter(|t| t.is_finite())? as i64,
            end_ms: value.get("end").and_then(Value::as_f64).filter(|t| t.is_finite())? as i64,
            input: text("input").filter(|id| valid_input_id(id)),
            title: text("title").and_then(|t| clean_title(&t).ok())?,
            under: text("under").as_deref().and_then(Under::parse).unwrap_or(Under::Replace),
            created_by: text("created_by").unwrap_or_default().chars().filter(|c| !c.is_control()).take(80).collect(),
            keep: value.get("keep").and_then(Keep::from_value),
        };
        (window.end_ms > window.start_ms).then_some(window)
    }
}

/// A channel number as the live API takes it.
pub fn valid_channel(channel: &str) -> bool {
    !channel.is_empty() && channel.len() <= 20 && channel.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
}

/// A window id: 16 hexadecimal digits.
pub fn valid_id(id: &str) -> bool {
    id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

fn valid_input_id(id: &str) -> bool {
    valid_id(id)
}

static COUNTER: AtomicU16 = AtomicU16::new(0);

/// A new id: the time in milliseconds and a counter, as hexadecimal.
pub fn new_id(now_ms: i64) -> String {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{:012x}{:04x}", (now_ms as u64) & 0xffff_ffff_ffff, n)
}

fn clean_title(text: &str) -> Result<String, String> {
    let title: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let title = title.trim();
    if title.is_empty() {
        return Err("tytuł: wpisz tytuł emisji".into());
    }
    if title.chars().count() > TITLE_MAX {
        return Err(format!("tytuł: do {TITLE_MAX} znaków"));
    }
    Ok(title.to_owned())
}

/// A time of the panel's JSON: ISO with an offset, or epoch milliseconds.
fn time_of(value: Option<&Value>, name: &str) -> Result<i64, String> {
    let wrong = || format!("{name}: data i godzina ISO ze strefą (np. 2026-11-28T22:00:00+01:00) albo milisekundy");
    match value {
        Some(Value::String(text)) => {
            // Without an offset the time would be read as UTC – a wrong hour, so refused.
            let t = text.trim();
            let zoned =
                t.ends_with('Z') || t.ends_with('z') || t.get(t.len().saturating_sub(6)..).is_some_and(|s| s.starts_with(['+', '-']));
            if t.len() <= 10 || !zoned {
                return Err(wrong());
            }
            crate::time::parse_iso(t).ok_or_else(wrong)
        }
        Some(Value::Number(n)) if n.is_finite() && n.fract() == 0.0 && *n > 0.0 => Ok(*n as i64),
        _ => Err(wrong()),
    }
}

/// A new window of `channel` from the panel's JSON, checked against now and
/// the windows already there (`others`, all channels).
pub fn new_window(
    body: &Value,
    channel: &str,
    id: String,
    who: &str,
    now_ms: i64,
    others: &[Window],
    has_input: impl Fn(&str) -> bool,
) -> Result<Window, String> {
    if !valid_channel(channel) {
        return Err("kanał?".into());
    }
    let start_ms = time_of(body.get("start"), "początek")?;
    let end_ms = time_of(body.get("end"), "koniec")?;
    if end_ms <= start_ms {
        return Err("koniec musi być po początku".into());
    }
    if end_ms - start_ms > MAX_LENGTH_MS {
        return Err("okno nagłej emisji: najwyżej 24 godziny".into());
    }
    if start_ms < now_ms - PAST_SLACK_MS {
        return Err("początek już minął – okno nagłej emisji zaczyna się teraz albo później".into());
    }
    if start_ms > now_ms + MAX_AHEAD_MS {
        return Err("okno nagłej emisji: najwyżej 30 dni naprzód".into());
    }
    let input = match body.get("input") {
        None | Some(Value::Null) => None,
        Some(Value::String(id)) if id.is_empty() => None,
        Some(Value::String(id)) if valid_input_id(id) && has_input(id) => Some(id.clone()),
        Some(_) => return Err("wejście: kanał nie ma takiego wejścia (lista wejść się zmieniła?)".into()),
    };
    let title = match body.get("title") {
        Some(Value::String(t)) => clean_title(t)?,
        _ => return Err("tytuł: wpisz tytuł emisji".into()),
    };
    let under = match body.get("under") {
        None | Some(Value::Null) => Under::Replace,
        Some(Value::String(u)) => Under::parse(u).ok_or("under: replace albo keep")?,
        Some(_) => return Err("under: replace albo keep".into()),
    };
    let keep = match (under, body.get("keep")) {
        (Under::Keep, Some(value @ Value::Object(_))) => {
            Some(Keep::from_value(value).ok_or("keep: {playout, schedule, date RRRR-MM-DD} z ErsatzTV")?)
        }
        (Under::Keep, _) => return Err("„zachowaj odcinek”: brak harmonogramu na ten dzień w ErsatzTV (keep)".into()),
        (Under::Replace, None | Some(Value::Null)) => None,
        (Under::Replace, Some(_)) => return Err("keep tylko z under: keep".into()),
    };
    if others.len() >= MAX_WINDOWS {
        return Err(format!("najwyżej {MAX_WINDOWS} zaplanowanych okien"));
    }
    if let Some(o) = others.iter().find(|o| o.channel == channel && o.start_ms < end_ms && start_ms < o.end_ms) {
        return Err(format!(
            "kanał {channel} ma już w tym czasie okno „{}” ({} – {} UTC)",
            o.title,
            crate::time::iso_time(o.start_ms),
            crate::time::iso_time(o.end_ms)
        ));
    }
    let created_by = who.chars().filter(|c| !c.is_control()).take(80).collect();
    Ok(Window { id, channel: channel.to_owned(), start_ms, end_ms, input, title, under, created_by, keep })
}

/// The window of the channel on now, else the nearest coming one.
pub fn current_or_next<'a>(windows: &'a [Window], channel: &str, now_ms: i64) -> Option<&'a Window> {
    let mine = || windows.iter().filter(move |w| w.channel == channel && !w.over(now_ms));
    mine().find(|w| w.active(now_ms)).or_else(|| mine().min_by_key(|w| w.start_ms))
}

/// The windows of a channel, by start.
pub fn of_channel<'a>(windows: &'a [Window], channel: &str) -> Vec<&'a Window> {
    let mut mine: Vec<&Window> = windows.iter().filter(|w| w.channel == channel).collect();
    mine.sort_by_key(|w| w.start_ms);
    mine
}

/// `{"windows": [...]}` of a channel for the API.
pub fn list_json(windows: &[Window], channel: &str, now_ms: i64) -> Value {
    json::object([
        ("windows", Value::Array(of_channel(windows, channel).into_iter().map(|w| w.to_value(Some(now_ms))).collect())),
        ("now", (now_ms as f64).into()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_795_000_000_000;
    const H: i64 = 3_600_000;

    fn body(start: Value, end: Value) -> Value {
        json::object([("start", start), ("end", end), ("title", "Debata na żywo".into())])
    }

    fn make(start: i64, end: i64, others: &[Window]) -> Result<Window, String> {
        new_window(&body((start as f64).into(), (end as f64).into()), "1", new_id(NOW), "jan", NOW, others, |_| false)
    }

    #[test]
    fn start_and_end() {
        let w = make(NOW + H, NOW + 2 * H, &[]).unwrap();
        assert!(!w.active(NOW) && w.active(NOW + H) && w.active(NOW + 2 * H - 1) && !w.active(NOW + 2 * H));
        assert!(w.over(NOW + 2 * H) && !w.over(NOW + H));
        assert_eq!(w.under, Under::Replace);
        assert!(make(NOW + H, NOW + H, &[]).is_err(), "end after start");
        assert!(make(NOW - 2 * PAST_SLACK_MS, NOW + H, &[]).is_err(), "start in the past");
        assert!(make(NOW - PAST_SLACK_MS / 2, NOW + H, &[]).is_ok(), "a minute of slack");
        assert!(make(NOW, NOW + 25 * H, &[]).is_err(), "at most 24 h");
    }

    #[test]
    fn iso_times_need_an_offset() {
        let now = crate::time::parse_iso("2026-11-28T12:00:00Z").unwrap();
        let at = |s: &str| new_window(&body(s.into(), "2026-11-28T23:00:00+01:00".into()), "1", new_id(NOW), "", now, &[], |_| false);
        let w = at("2026-11-28T22:00:00+01:00").unwrap();
        assert_eq!(w.end_ms - w.start_ms, H);
        assert_eq!(w.start_ms, crate::time::parse_iso("2026-11-28T21:00:00Z").unwrap());
        assert!(at("2026-11-28T22:00:00").is_err(), "no offset = refused");
        assert!(at("2026-11-28").is_err());
        assert!(at("jutro").is_err());
    }

    #[test]
    fn windows_of_a_channel_do_not_overlap() {
        let first = make(NOW + H, NOW + 2 * H, &[]).unwrap();
        let others = [first.clone()];
        assert!(make(NOW + 90 * 60_000, NOW + 3 * H, &others).unwrap_err().contains("ma już w tym czasie"));
        assert!(make(NOW + 2 * H, NOW + 3 * H, &others).is_ok(), "back to back");
        let other_channel =
            new_window(&body(((NOW + H) as f64).into(), ((NOW + 2 * H) as f64).into()), "2", new_id(NOW), "", NOW, &others, |_| false);
        assert!(other_channel.is_ok(), "another channel at the same time");
    }

    #[test]
    fn input_title_and_under() {
        let mut b = body(((NOW + H) as f64).into(), ((NOW + 2 * H) as f64).into());
        let with = |b: &Value, ok: bool| new_window(b, "1", new_id(NOW), "", NOW, &[], |id| ok && id == "00112233aabbccdd");
        if let Value::Object(m) = &mut b {
            m.push(("input".into(), "00112233aabbccdd".into()));
            m.push(("under".into(), "keep".into()));
        }
        assert!(with(&b, true).unwrap_err().contains("keep"), "keep needs what VizRS Live put in ErsatzTV");
        if let Value::Object(m) = &mut b {
            m.push(("keep".into(), json::parse(r#"{"playout": 3, "schedule": 41, "date": "2026-11-28"}"#).unwrap()));
        }
        let w = with(&b, true).unwrap();
        assert_eq!((w.input.as_deref(), w.under), (Some("00112233aabbccdd"), Under::Keep));
        assert_eq!(w.keep, Some(Keep { playout: 3, schedule: 41, date: "2026-11-28".into() }));
        let back = Window::from_value(&json::parse(&w.to_value(None).to_string()).unwrap()).unwrap();
        assert_eq!(back, w, "keep stored and read back");
        // Kept on the list after the end until VizRS Live cleans ErsatzTV up, at most a week.
        assert!(w.kept(w.end_ms + 1) && w.kept(w.end_ms + KEEP_FOR_MS - 1) && !w.kept(w.end_ms + KEEP_FOR_MS));
        if let Value::Object(m) = &mut b {
            m.retain(|(k, _)| k != "keep");
            m.push(("keep".into(), json::parse(r#"{"playout": 0, "schedule": 41, "date": "2026-11-28"}"#).unwrap()));
        }
        assert!(with(&b, true).is_err(), "ids from 1");
        assert!(with(&b, false).is_err(), "an input the channel has not");
        let mut bad = body(((NOW + H) as f64).into(), ((NOW + 2 * H) as f64).into());
        if let Value::Object(m) = &mut bad {
            m.retain(|(k, _)| k != "title");
            m.push(("title".into(), "  \n ".into()));
        }
        assert!(with(&bad, true).is_err(), "empty title");
    }

    #[test]
    fn stored_and_read_back() {
        let w = make(NOW + H, NOW + 2 * H, &[]).unwrap();
        let back = Window::from_value(&json::parse(&w.to_value(None).to_string()).unwrap()).unwrap();
        assert_eq!(back, w);
        assert!(Window::from_value(&json::parse(r#"{"id":"x","channel":"1","start":1,"end":2,"title":"t"}"#).unwrap()).is_none());
    }

    #[test]
    fn current_or_next_window() {
        let a = make(NOW + H, NOW + 2 * H, &[]).unwrap();
        let b = make(NOW + 3 * H, NOW + 4 * H, std::slice::from_ref(&a)).unwrap();
        let all = [b.clone(), a.clone()];
        assert_eq!(current_or_next(&all, "1", NOW).map(|w| &w.id), Some(&a.id));
        assert_eq!(current_or_next(&all, "1", NOW + 2 * H + 1).map(|w| &w.id), Some(&b.id));
        assert!(current_or_next(&all, "1", NOW + 5 * H).is_none());
        assert!(current_or_next(&all, "2", NOW).is_none());
    }
}
