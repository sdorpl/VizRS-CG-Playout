// Secondary events (K15) and their validity and approval (K16): rules that put
// pages on air at times of an item, chosen by the item's show title or title
// (case-insensitive regular expressions), on every channel or on one:
//
//   "secondary": [{
//     "name": "Za chwilę – Wiadomości",
//     "channel": "1",                       // optional, every channel without it
//     "target": {"show_title": "^Wiadomości"},   // or "title"; both = both must match
//     "events": [
//       {"page": "7501", "anchor": "start", "offset_s": 10, "duration_s": 8},
//       {"page": "7510", "anchor": "end", "offset_s": 30, "duration_s": 25,
//        "values": {"f0": "{next_title}"}}
//     ],
//     "valid_from": "2026-11-24", "valid_to": "2026-12-31",   // K16, optional
//     "approved": null                     // K16: null = waits, {by, at} = approved
//   }]
//
// An event goes on air `offset_s` after the item's start ("start") or before
// its end ("end": the programme's end, as on_end), and out `duration_s` later
// (at the item's stop at the latest). `values` are the page's fields (JSON
// fields of the TAKE line), with the placeholders of the commands ({title},
// {next_title}, …). `valid_from`/`valid_to` (YYYY-MM-DD, the local date of the
// item's start) and `approved` also go on single events. Without `approved`
// an entry needs no approval; with it, only `{by, at}` lets it on air.
//
// Two events of a rule that put the same page on air at overlapping times
// from the same anchor are refused (the second TAKE would cut the first). The
// sidecar does not know the layers of pages (VizRS does), nor the collection
// or the playout item of a clip (ErsatzTV does not give them to the script),
// so `target.collection` and `target.item_id` are refused with a message.

use regex::{Regex, RegexBuilder};

use crate::config::{self, MAX_SECONDS};
use crate::json::{self, Value};

/// Most rules and most events of a rule.
pub const MAX_RULES: usize = 200;
pub const MAX_EVENTS: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Start,
    End,
}

impl Anchor {
    fn as_str(self) -> &'static str {
        match self {
            Anchor::Start => "start",
            Anchor::End => "end",
        }
    }
}

/// Approval of a rule or an event (K16).
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Approval {
    /// No `approved` key: on air without approval.
    #[default]
    NotRequired,
    /// `approved: null`: waits for approval, not on air.
    Waiting,
    Approved {
        by: String,
        at: String,
    },
}

impl Approval {
    fn on_air(&self) -> bool {
        !matches!(self, Approval::Waiting)
    }

    fn to_json(&self) -> Option<Value> {
        match self {
            Approval::NotRequired => None,
            Approval::Waiting => Some(Value::Null),
            Approval::Approved { by, at } => Some(json::object([("by", by.as_str().into()), ("at", at.as_str().into())])),
        }
    }
}

/// Validity dates (YYYY-MM-DD), both optional.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Validity {
    pub from: Option<String>,
    pub to: Option<String>,
}

impl Validity {
    /// Whether `day` (YYYY-MM-DD) is inside.
    pub fn holds(&self, day: &str) -> bool {
        self.from.as_deref().is_none_or(|f| f <= day) && self.to.as_deref().is_none_or(|t| day <= t)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub page: String,
    pub anchor: Anchor,
    pub offset_s: f64,
    pub duration_s: f64,
    /// Page fields (a JSON object), `None` = the page's own.
    pub values: Option<Value>,
    pub valid: Validity,
    pub approved: Approval,
}

impl Event {
    /// The TAKE line of the event (placeholders rendered later, per clip).
    pub fn command(&self) -> String {
        match &self.values {
            Some(values) => format!("TAKE {} {values}", self.page),
            None => format!("TAKE {}", self.page),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rule {
    pub name: String,
    pub channel: Option<String>,
    pub show_title: String,
    pub title: String,
    pub events: Vec<Event>,
    pub valid: Validity,
    pub approved: Approval,
}

fn text(value: Option<&Value>, name: &str) -> Result<String, String> {
    value.map(|v| config::string(v, name)).transpose().map(|t| t.unwrap_or_default().trim().to_owned())
}

fn day(value: Option<&Value>, name: &str) -> Result<Option<String>, String> {
    let d = text(value, name)?;
    if d.is_empty() {
        return Ok(None);
    }
    let b = d.as_bytes();
    let ok = b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
        && (1..=12).contains(&d[5..7].parse::<u32>().unwrap_or(0))
        && (1..=31).contains(&d[8..10].parse::<u32>().unwrap_or(0));
    if ok { Ok(Some(d)) } else { Err(format!("{name}: expected a date YYYY-MM-DD")) }
}

fn validity(item: &Value, prefix: &str) -> Result<Validity, String> {
    let valid = Validity {
        from: day(item.get("valid_from"), &format!("{prefix}.valid_from"))?,
        to: day(item.get("valid_to"), &format!("{prefix}.valid_to"))?,
    };
    if let (Some(f), Some(t)) = (&valid.from, &valid.to) {
        if f > t {
            return Err(format!("{prefix}: valid_from {f} is after valid_to {t}"));
        }
    }
    Ok(valid)
}

fn approval(item: &Value, prefix: &str) -> Result<Approval, String> {
    match item.get("approved") {
        None => Ok(Approval::NotRequired),
        Some(Value::Null) => Ok(Approval::Waiting),
        Some(v @ Value::Object(_)) => {
            let by = text(v.get("by"), &format!("{prefix}.approved.by"))?;
            let at = text(v.get("at"), &format!("{prefix}.approved.at"))?;
            if by.is_empty() {
                return Err(format!("{prefix}.approved.by: who approved is required"));
            }
            Ok(Approval::Approved { by: by.chars().take(80).collect(), at: at.chars().take(40).collect() })
        }
        Some(_) => Err(format!("{prefix}.approved: expected null or {{\"by\", \"at\"}}")),
    }
}

fn event(item: &Value, prefix: &str) -> Result<Event, String> {
    if !matches!(item, Value::Object(_)) {
        return Err(format!("{prefix}: expected an object"));
    }
    let page = text(item.get("page"), &format!("{prefix}.page"))?;
    if page.is_empty() || page.len() > 64 || page.contains(|c: char| c.is_whitespace() || c == ',') {
        return Err(format!("{prefix}.page: a page number or name (no spaces or commas)"));
    }
    let anchor = match text(item.get("anchor"), &format!("{prefix}.anchor"))?.as_str() {
        "" | "start" => Anchor::Start,
        "end" => Anchor::End,
        other => return Err(format!("{prefix}.anchor: expected \"start\" or \"end\", not \"{other}\"")),
    };
    let offset_s = item.get("offset_s").map(|v| config::seconds(v, &format!("{prefix}.offset_s"))).transpose()?.unwrap_or(0.0);
    let duration_s = match item.get("duration_s") {
        Some(v) => config::seconds(v, &format!("{prefix}.duration_s"))?,
        None => return Err(format!("{prefix}.duration_s: how long the page stays on air is required")),
    };
    if duration_s <= 0.0 {
        return Err(format!("{prefix}.duration_s: expected more than 0 and at most {MAX_SECONDS} seconds"));
    }
    let values = match item.get("values") {
        None | Some(Value::Null) => None,
        Some(v @ Value::Object(members)) => {
            if members.iter().any(|(_, f)| !matches!(f, Value::String(_) | Value::Number(_) | Value::Bool(_))) {
                return Err(format!("{prefix}.values: field values are text, numbers or true/false"));
            }
            (!members.is_empty()).then(|| v.clone())
        }
        Some(_) => return Err(format!("{prefix}.values: expected an object of page fields")),
    };
    Ok(Event { page, anchor, offset_s, duration_s, values, valid: validity(item, prefix)?, approved: approval(item, prefix)? })
}

fn pattern(text: &str, name: &str) -> Result<Option<Regex>, String> {
    if text.is_empty() {
        return Ok(None);
    }
    RegexBuilder::new(text).case_insensitive(true).size_limit(1 << 20).build().map(Some).map_err(|error| {
        let reason = error.to_string().lines().last().unwrap_or("").trim().to_owned();
        format!("{name} \"{text}\" is not a valid regular expression ({reason})")
    })
}

/// Events of a rule that put the same page on air at overlapping times from the same anchor.
fn overlaps(events: &[Event], prefix: &str) -> Result<(), String> {
    for (i, a) in events.iter().enumerate() {
        for (j, b) in events.iter().enumerate().skip(i + 1) {
            if a.page != b.page || a.anchor != b.anchor {
                continue;
            }
            // On the item's clock: from start the page is on at [offset, offset + duration);
            // from the end at [-offset, -offset + duration).
            let span = |e: &Event| match e.anchor {
                Anchor::Start => (e.offset_s, e.offset_s + e.duration_s),
                Anchor::End => (-e.offset_s, -e.offset_s + e.duration_s),
            };
            let ((a0, a1), (b0, b1)) = (span(a), span(b));
            if a0 < b1 && b0 < a1 {
                return Err(format!(
                    "{prefix}.events[{j}]: page {} is on air from events[{i}] at that time – the second TAKE would cut it",
                    b.page
                ));
            }
        }
    }
    Ok(())
}

/// The `secondary` array of a configuration or a rules document.
pub fn parse(value: &Value, name: &str) -> Result<Vec<Rule>, String> {
    let items = match value {
        Value::Null => return Ok(Vec::new()),
        Value::Array(items) => items,
        _ => return Err(format!("{name}: expected an array")),
    };
    if items.len() > MAX_RULES {
        return Err(format!("{name}: at most {MAX_RULES} rules"));
    }
    let mut rules = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let prefix = format!("{name}[{i}]");
        if !matches!(item, Value::Object(_)) {
            return Err(format!("{prefix}: expected an object"));
        }
        let target = item.get("target").filter(|t| !t.is_null());
        if target.is_some_and(|t| !matches!(t, Value::Object(_))) {
            return Err(format!("{prefix}.target: expected an object"));
        }
        for unsupported in ["collection", "item_id"] {
            if target.and_then(|t| t.get(unsupported)).is_some_and(|v| !v.is_null()) {
                return Err(format!(
                    "{prefix}.target.{unsupported}: not supported – ErsatzTV does not give the sidecar the collection or the playout item of a clip; use show_title or title"
                ));
            }
        }
        let show_title = text(target.and_then(|t| t.get("show_title")), &format!("{prefix}.target.show_title"))?;
        let title = text(target.and_then(|t| t.get("title")), &format!("{prefix}.target.title"))?;
        if show_title.is_empty() && title.is_empty() {
            return Err(format!("{prefix}.target: show_title or title is required"));
        }
        pattern(&show_title, &format!("{prefix}.target.show_title"))?;
        pattern(&title, &format!("{prefix}.target.title"))?;
        let channel = text(item.get("channel"), &format!("{prefix}.channel"))?;
        if channel.len() > 16 || channel.contains(|c: char| !c.is_ascii_alphanumeric() && c != '.' && c != '-') {
            return Err(format!("{prefix}.channel: a channel number"));
        }
        let list = item.get("events").and_then(Value::as_array).ok_or(format!("{prefix}.events: expected an array"))?;
        if list.is_empty() || list.len() > MAX_EVENTS {
            return Err(format!("{prefix}.events: 1 to {MAX_EVENTS} events"));
        }
        let events = list.iter().enumerate().map(|(j, e)| event(e, &format!("{prefix}.events[{j}]"))).collect::<Result<Vec<_>, _>>()?;
        overlaps(&events, &prefix)?;
        rules.push(Rule {
            name: text(item.get("name"), &format!("{prefix}.name"))?.chars().take(120).collect(),
            channel: (!channel.is_empty()).then_some(channel),
            show_title,
            title,
            events,
            valid: validity(item, &prefix)?,
            approved: approval(item, &prefix)?,
        });
    }
    Ok(rules)
}

fn validity_members(members: &mut Vec<(String, Value)>, valid: &Validity, approved: &Approval) {
    if let Some(f) = &valid.from {
        members.push(("valid_from".into(), f.as_str().into()));
    }
    if let Some(t) = &valid.to {
        members.push(("valid_to".into(), t.as_str().into()));
    }
    if let Some(a) = approved.to_json() {
        members.push(("approved".into(), a));
    }
}

pub fn to_json(rules: &[Rule]) -> Value {
    Value::Array(
        rules
            .iter()
            .map(|r| {
                let mut target = Vec::new();
                if !r.show_title.is_empty() {
                    target.push(("show_title".to_owned(), Value::from(r.show_title.as_str())));
                }
                if !r.title.is_empty() {
                    target.push(("title".to_owned(), Value::from(r.title.as_str())));
                }
                let events = r
                    .events
                    .iter()
                    .map(|e| {
                        let mut m = vec![
                            ("page".to_owned(), Value::from(e.page.as_str())),
                            ("anchor".to_owned(), e.anchor.as_str().into()),
                            ("offset_s".to_owned(), e.offset_s.into()),
                            ("duration_s".to_owned(), e.duration_s.into()),
                        ];
                        if let Some(v) = &e.values {
                            m.push(("values".into(), v.clone()));
                        }
                        validity_members(&mut m, &e.valid, &e.approved);
                        Value::Object(m)
                    })
                    .collect();
                let mut members = vec![("name".to_owned(), Value::from(r.name.as_str()))];
                if let Some(c) = &r.channel {
                    members.push(("channel".into(), c.as_str().into()));
                }
                members.push(("target".into(), Value::Object(target)));
                members.push(("events".into(), Value::Array(events)));
                validity_members(&mut members, &r.valid, &r.approved);
                Value::Object(members)
            })
            .collect(),
    )
}

/// One event to plan in a clip: the rule and event numbers (for the once-per-item
/// key) and the event.
pub struct Planned<'a> {
    pub rule: usize,
    pub event: usize,
    pub item: &'a Event,
}

/// The rules of one channel, compiled.
#[derive(Debug, Default)]
pub struct Compiled {
    rules: Vec<(Rule, Option<Regex>, Option<Regex>)>,
}

impl Compiled {
    /// The rules for `channel` (those of every channel and its own).
    pub fn new(rules: &[Rule], channel: &str) -> Compiled {
        Compiled {
            rules: rules
                .iter()
                .filter(|r| r.channel.as_deref().is_none_or(|c| c == channel))
                .map(|r| (r.clone(), pattern(&r.show_title, "").ok().flatten(), pattern(&r.title, "").ok().flatten()))
                .collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Events on air in an item with `show_title` and `title` starting on `day`
    /// (local YYYY-MM-DD): matching, valid and approved rules and events.
    pub fn plan(&self, show_title: &str, title: &str, day: &str) -> Vec<Planned<'_>> {
        let mut planned = Vec::new();
        for (i, (rule, show, own)) in self.rules.iter().enumerate() {
            let fits = |p: &Option<Regex>, text: &str, source: &str| source.is_empty() || p.as_ref().is_some_and(|p| p.is_match(text));
            if !fits(show, show_title, &rule.show_title) || !fits(own, title, &rule.title) {
                continue;
            }
            if !rule.valid.holds(day) || !rule.approved.on_air() {
                continue;
            }
            for (j, event) in rule.events.iter().enumerate() {
                if event.valid.holds(day) && event.approved.on_air() {
                    planned.push(Planned { rule: i, event: j, item: event });
                }
            }
        }
        planned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(text: &str) -> Result<Vec<Rule>, String> {
        parse(&json::parse(text).unwrap(), "secondary")
    }

    #[test]
    fn round_trip() {
        let doc = r#"[{"name":"Za chwilę","channel":"1","target":{"show_title":"^Wiadomości"},
            "events":[{"page":"7501","anchor":"start","offset_s":10,"duration_s":8},
                      {"page":"7510","anchor":"end","offset_s":30,"duration_s":25,"values":{"f0":"{next_title}"},"approved":{"by":"jan","at":"2026-11-24T10:00:00Z"}}],
            "valid_from":"2026-11-24","valid_to":"2026-12-31","approved":null}]"#;
        let parsed = rules(doc).unwrap();
        assert_eq!(parsed[0].events[1].command(), r#"TAKE 7510 {"f0":"{next_title}"}"#);
        assert_eq!(parsed[0].approved, Approval::Waiting);
        let again = parse(&to_json(&parsed), "secondary").unwrap();
        assert_eq!(again, parsed);
        assert_eq!(rules("null").unwrap(), Vec::new());
    }

    #[test]
    fn refuses_bad_rules() {
        let bad = [
            (r#"[{"events":[{"page":"1","duration_s":5}]}]"#, "show_title or title is required"),
            (r#"[{"target":{"collection":"Filmy"},"events":[{"page":"1","duration_s":5}]}]"#, "target.collection: not supported"),
            (r#"[{"target":{"item_id":5},"events":[{"page":"1","duration_s":5}]}]"#, "target.item_id: not supported"),
            (r#"[{"target":{"title":"("},"events":[{"page":"1","duration_s":5}]}]"#, "not a valid regular expression"),
            (r#"[{"target":{"title":"x"},"events":[]}]"#, "1 to 20 events"),
            (r#"[{"target":{"title":"x"},"events":[{"page":"1"}]}]"#, "duration_s: how long"),
            (r#"[{"target":{"title":"x"},"events":[{"page":"1","duration_s":0}]}]"#, "more than 0"),
            (r#"[{"target":{"title":"x"},"events":[{"page":"1 2","duration_s":3}]}]"#, "page: a page number"),
            (r#"[{"target":{"title":"x"},"events":[{"page":"1","anchor":"middle","duration_s":3}]}]"#, "anchor"),
            (r#"[{"target":{"title":"x"},"events":[{"page":"1","duration_s":3,"values":{"a":[1]}}]}]"#, "values"),
            (r#"[{"target":{"title":"x"},"valid_from":"2026-13-01","events":[{"page":"1","duration_s":3}]}]"#, "YYYY-MM-DD"),
            (
                r#"[{"target":{"title":"x"},"valid_from":"2026-12-02","valid_to":"2026-12-01","events":[{"page":"1","duration_s":3}]}]"#,
                "is after",
            ),
            (r#"[{"target":{"title":"x"},"approved":{"at":"x"},"events":[{"page":"1","duration_s":3}]}]"#, "who approved"),
            (
                r#"[{"target":{"title":"x"},"events":[{"page":"7","offset_s":10,"duration_s":10},{"page":"7","offset_s":15,"duration_s":3}]}]"#,
                "events[1]: page 7 is on air from events[0]",
            ),
            (
                r#"[{"target":{"title":"x"},"events":[{"page":"7","anchor":"end","offset_s":30,"duration_s":10},{"page":"7","anchor":"end","offset_s":25,"duration_s":3}]}]"#,
                "would cut it",
            ),
        ];
        for (doc, message) in bad {
            let error = rules(doc).unwrap_err();
            assert!(error.contains(message), "{doc}: {error}");
        }
        // The same page from both anchors, or one after the other: fine.
        assert!(rules(r#"[{"target":{"title":"x"},"events":[{"page":"7","offset_s":10,"duration_s":5},{"page":"7","offset_s":15,"duration_s":5},{"page":"7","anchor":"end","offset_s":10,"duration_s":5}]}]"#).is_ok());
    }

    #[test]
    fn plan_matches_validity_and_approval() {
        let parsed = rules(
            r#"[{"channel":"2","target":{"show_title":"^wiadomości"},"events":[{"page":"1","duration_s":5}]},
                {"target":{"show_title":"^Wiadomości","title":"Sport"},"events":[{"page":"2","duration_s":5}]},
                {"target":{"title":"Film"},"valid_to":"2026-11-30","events":[{"page":"3","duration_s":5},{"page":"4","duration_s":5,"approved":null},{"page":"5","duration_s":5,"valid_from":"2026-11-25"}]},
                {"target":{"title":"Film"},"approved":null,"events":[{"page":"6","duration_s":5}]},
                {"target":{"title":"Film"},"approved":{"by":"ola","at":""},"events":[{"page":"8","duration_s":5}]}]"#,
        )
        .unwrap();
        let pages = |c: &Compiled, show: &str, title: &str, day: &str| {
            c.plan(show, title, day).iter().map(|p| p.item.page.clone()).collect::<Vec<_>>()
        };
        let one = Compiled::new(&parsed, "1");
        let two = Compiled::new(&parsed, "2");
        assert_eq!(pages(&two, "Wiadomości", "Wydanie", "2026-11-24"), ["1"]);
        assert_eq!(pages(&one, "Wiadomości", "Wydanie", "2026-11-24"), Vec::<String>::new());
        assert_eq!(pages(&one, "Wiadomości", "Sport dnia", "2026-11-24"), ["2"]);
        assert_eq!(pages(&one, "", "Film tygodnia", "2026-11-24"), ["3", "8"]);
        assert_eq!(pages(&one, "", "Film tygodnia", "2026-11-25"), ["3", "5", "8"]);
        assert_eq!(pages(&one, "", "Film tygodnia", "2026-12-01"), ["8"]);
    }
}
