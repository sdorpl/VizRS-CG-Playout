// Clip context from the ErsatzTV template data (stdin JSON of the script
// element) and rendering of the VizRS command templates (DOKUMENTACJA §3).

use regex::{Regex, RegexBuilder};

use crate::config::AdMatch;
use crate::json::{self, Value};
use crate::time;

/// Seconds of a .NET TimeSpan in JSON (`[-][d.]hh:mm:ss[.fffffff]`), or a number.
pub fn time_span_seconds(value: Option<&Value>) -> f64 {
    match value {
        Some(Value::Number(n)) => *n,
        Some(Value::String(s)) => parse_time_span(s.trim()).unwrap_or(0.0),
        _ => 0.0,
    }
}

fn digits(part: &str) -> Option<f64> {
    (!part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())).then(|| part.parse().ok()).flatten()
}

fn parse_time_span(text: &str) -> Option<f64> {
    let (minus, text) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let mut parts = text.split(':');
    let (first, minutes, seconds) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let (days, hours) = match first.split_once('.') {
        Some((d, h)) => (digits(d)?, digits(h)?),
        None => (0.0, digits(first)?),
    };
    let seconds = match seconds.split_once('.') {
        Some((whole, fraction)) => {
            digits(whole)?;
            digits(fraction)?;
            seconds.parse().ok()?
        }
        None => digits(seconds)?,
    };
    let total = days * 86_400.0 + hours * 3600.0 + digits(minutes)? * 60.0 + seconds;
    Some(if minus { -total } else { total })
}

/// JavaScript's `Math.round`: halves go up.
fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// `hh:mm:ss` for durations, wrapping past 24 h.
pub fn clock(seconds: f64) -> String {
    let total = js_round(seconds).max(0.0) as u64;
    format!("{:02}:{:02}:{:02}", total / 3600 % 24, total / 60 % 60, total % 60)
}

/// Wall clock `hh:mm:ss` in the offset ErsatzTV sent ("2026-10-24T20:15:00+02:00").
fn local_time(iso: &str) -> String {
    let bytes = iso.as_bytes();
    (0..bytes.len())
        .filter(|&i| bytes[i] == b'T' && i + 9 <= bytes.len())
        .map(|i| &bytes[i + 1..i + 9])
        .find(|t| t.iter().enumerate().all(|(j, b)| if j == 2 || j == 5 { *b == b':' } else { b.is_ascii_digit() }))
        .map(|t| String::from_utf8_lossy(t).into_owned())
        .unwrap_or_default()
}

/// Values must not break `key=value|key=value` or the line protocol.
pub fn clean_value(value: &str) -> String {
    value.replace(['\r', '\n'], "").replace('|', "/")
}

/// `String(value)` of JavaScript for the values template data carries.
pub fn js_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(items) => items.iter().map(|v| if v.is_null() { String::new() } else { js_string(v) }).collect::<Vec<_>>().join(","),
        Value::Object(_) => "[object Object]".into(),
        other => other.to_string(),
    }
}

/// `Number(value)` of JavaScript, NaN as `None`.
fn js_number(value: Option<&Value>) -> Option<f64> {
    match value {
        None | Some(Value::Null) => Some(0.0),
        Some(Value::Number(n)) => Some(*n),
        Some(Value::Bool(b)) => Some(f64::from(u8::from(*b))),
        Some(Value::String(s)) if s.trim().is_empty() => Some(0.0),
        Some(Value::String(s)) => s.trim().parse().ok().filter(|n: &f64| !n.is_nan()),
        Some(_) => None,
    }
}

/// `data.key ?? ""` as a string.
fn text(data: &Value, key: &str) -> String {
    data.get(key).filter(|v| !v.is_null()).map(js_string).unwrap_or_default()
}

/// File name without directory and extension, as `path.parse(file).name`.
fn stem(file: &str) -> String {
    let base = file.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    match base.rfind('.') {
        Some(dot) if dot > 0 => base[..dot].to_owned(),
        _ => base.to_owned(),
    }
}

/// `Date.parse` of a value; `None` for NaN.
fn date(value: Option<&Value>) -> Option<i64> {
    value.and_then(Value::as_str).and_then(time::parse_iso)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClipContext {
    pub title: String,
    pub show_title: String,
    pub source: String,
    pub start: String,
    pub stop: String,
    pub duration: f64,
    pub seek: f64,
    pub next_title: String,
    pub next_start_full: String,
    pub next_duration: f64,
    /// Stop of the guide entry the item starts in: the end of the programme
    /// when breaks cut it into several items ("" without a guide).
    pub programme_stop: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
}

impl ClipContext {
    /// Context of the clip described by the ErsatzTV template data.
    pub fn from_data(data: &Value) -> ClipContext {
        let source = text(data, "MediaItem_Path");
        let start = date(data.get("MediaItem_Start"));
        let stop = date(data.get("MediaItem_Stop"));
        let truthy = |v: &&Value| match v {
            Value::String(s) => !s.trim().is_empty(),
            Value::Number(n) => *n != 0.0 && !n.is_nan(),
            Value::Bool(b) => *b,
            Value::Null => false,
            _ => !js_string(v).trim().is_empty(),
        };
        let title = [data.get("MediaItem_Title"), data.get("MediaItem_ShowTitle")]
            .into_iter()
            .flatten()
            .find(truthy)
            .map(js_string)
            .unwrap_or_else(|| stem(&source));

        // EPG entries start at the current programme; the next one starts after this clip.
        let after = start.unwrap_or_else(time::now_ms) + 1000;
        let epg = data.get("Epg").and_then(Value::as_array).unwrap_or(&[]);
        let next = epg.iter().find(|entry| date(entry.get("Start")).is_some_and(|s| s > after));
        let next_span = next.and_then(|n| Some((date(n.get("Start"))?, date(n.get("Stop"))?)));
        let programme = start.and_then(|start| {
            epg.iter()
                .find(|entry| matches!((date(entry.get("Start")), date(entry.get("Stop"))), (Some(s), Some(e)) if s <= start && start < e))
        });

        let size = |key| {
            let value = data.get("Resolution").and_then(|r| r.get(key)).and_then(Value::as_f64).unwrap_or(0.0);
            if value.is_finite() && value > 0.0 { value as u32 } else { 0 }
        };

        ClipContext {
            title,
            show_title: text(data, "MediaItem_ShowTitle"),
            source,
            start: text(data, "MediaItem_Start"),
            stop: text(data, "MediaItem_Stop"),
            duration: match (start, stop) {
                (Some(start), Some(stop)) => (stop - start) as f64 / 1000.0,
                _ => time_span_seconds(data.get("MediaItem_Duration")),
            },
            seek: time_span_seconds(data.get("MediaItem_StreamSeek")),
            next_title: next.map(|n| text(n, "Title")).unwrap_or_default(),
            next_start_full: next.and_then(|n| n.get("Start")).and_then(Value::as_str).map(local_time).unwrap_or_default(),
            next_duration: next_span.map_or(0.0, |(s, e)| (e - s) as f64 / 1000.0),
            programme_stop: programme.map(|p| text(p, "Stop")).unwrap_or_default(),
            width: size("Width"),
            height: size("Height"),
            fps: js_number(data.get("FrameRate")).filter(|f| *f != 0.0).unwrap_or(0.0),
        }
    }

    /// When the clip's first transcoded frame is on air (epoch ms): the item's
    /// start in the schedule plus the seek.
    pub fn air_ms(&self) -> Option<f64> {
        time::parse_iso(&self.start).map(|start| start as f64 + self.seek * 1000.0)
    }

    /// The context with the field names of the Node.js build.
    pub fn to_json(&self) -> Value {
        json::object([
            ("title", self.title.as_str().into()),
            ("show_title", self.show_title.as_str().into()),
            ("source", self.source.as_str().into()),
            ("start", self.start.as_str().into()),
            ("stop", self.stop.as_str().into()),
            ("duration", self.duration.into()),
            ("seek", self.seek.into()),
            ("next_title", self.next_title.as_str().into()),
            ("next_start_full", self.next_start_full.as_str().into()),
            ("next_duration", self.next_duration.into()),
            ("width", u64::from(self.width).into()),
            ("height", u64::from(self.height).into()),
            ("fps", self.fps.into()),
        ])
    }
}

/// Commercials by the configured regular expressions on path and title
/// (case-insensitive, also beyond ASCII, like JavaScript's `i` flag).
#[derive(Clone, Debug, Default)]
pub struct AdMatcher {
    path: Option<Regex>,
    title: Option<Regex>,
}

impl AdMatcher {
    /// The matcher and a warning per pattern that does not compile; such a
    /// pattern matches nothing (the Node.js build crashed on it).
    pub fn new(ad_match: &AdMatch) -> (AdMatcher, Vec<String>) {
        let mut warnings = Vec::new();
        let mut compile = |name: &str, pattern: &str| {
            if pattern.is_empty() {
                return None;
            }
            match RegexBuilder::new(pattern).case_insensitive(true).build() {
                Ok(regex) => Some(regex),
                Err(error) => {
                    let reason = error.to_string().lines().last().unwrap_or("").trim().to_owned();
                    warnings
                        .push(format!("ad_match.{name} \"{pattern}\" is not a valid regular expression ({reason}); it matches nothing"));
                    None
                }
            }
        };
        let path = compile("path", &ad_match.path);
        let title = compile("title", &ad_match.title);
        (AdMatcher { path, title }, warnings)
    }

    pub fn is_ad(&self, ctx: &ClipContext) -> bool {
        self.path.as_ref().is_some_and(|r| r.is_match(&ctx.source)) || self.title.as_ref().is_some_and(|r| r.is_match(&ctx.title))
    }
}

/// Compiled per-show graphics element configuration.
#[derive(Clone, Debug)]
pub struct CompiledShow {
    pattern: Option<Regex>,
    pub on_clip: Option<String>,
    pub on_ad: Option<String>,
    pub ad_matcher: Option<AdMatcher>,
    pub on_end: Option<String>,
    pub end_seconds: Option<f64>,
}

impl CompiledShow {
    pub fn matches(&self, show_title: &str) -> bool {
        self.pattern.as_ref().is_some_and(|r| r.is_match(show_title))
    }
}

/// Matches clips against per-show graphic elements configurations.
#[derive(Clone, Debug, Default)]
pub struct ShowMatcher {
    shows: Vec<CompiledShow>,
}

impl ShowMatcher {
    pub fn new(shows: &[crate::config::ShowOverride]) -> (ShowMatcher, Vec<String>) {
        let mut warnings = Vec::new();
        let mut compiled = Vec::new();
        for (i, show) in shows.iter().enumerate() {
            let pattern = if show.show_title.is_empty() {
                None
            } else {
                match RegexBuilder::new(&show.show_title).case_insensitive(true).build() {
                    Ok(regex) => Some(regex),
                    Err(error) => {
                        let reason = error.to_string().lines().last().unwrap_or("").trim().to_owned();
                        warnings.push(format!(
                            "shows[{i}].show_title \"{}\" is not a valid regular expression ({reason}); it matches nothing",
                            show.show_title
                        ));
                        None
                    }
                }
            };

            let ad_matcher = if let Some(ad_match) = &show.ad_match {
                let (matcher, ad_warnings) = AdMatcher::new(ad_match);
                for w in ad_warnings {
                    warnings.push(format!("shows[{i}].{w}"));
                }
                Some(matcher)
            } else {
                None
            };

            compiled.push(CompiledShow {
                pattern,
                on_clip: show.on_clip.clone(),
                on_ad: show.on_ad.clone(),
                ad_matcher,
                on_end: show.on_end.clone(),
                end_seconds: show.end_seconds,
            });
        }
        (ShowMatcher { shows: compiled }, warnings)
    }

    /// Finds the first matching show configuration for the given show_title.
    pub fn find(&self, show_title: &str) -> Option<&CompiledShow> {
        if show_title.is_empty() {
            return None;
        }
        self.shows.iter().find(|s| s.matches(show_title))
    }
}

fn render_line(line: &str, ctx: &ClipContext) -> String {
    // Replaced one after another, as the Node.js build does.
    let next_start: String = ctx.next_start_full.chars().take(5).collect();
    [
        ("{title}", clean_value(&ctx.title)),
        ("{show_title}", clean_value(&ctx.show_title)),
        ("{source}", clean_value(&ctx.source)),
        ("{duration}", clock(ctx.duration)),
        ("{next_title}", clean_value(&ctx.next_title)),
        ("{next_start}", next_start),
        ("{next_start_full}", ctx.next_start_full.clone()),
        ("{next_duration}", clock(ctx.next_duration)),
    ]
    .iter()
    .fold(line.to_owned(), |acc, (key, value)| acc.replace(key, value))
}

/// One command per line; empty lines and `#` comments are skipped.
pub fn render_commands(template: &str, ctx: &ClipContext) -> Vec<String> {
    template
        .split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| render_line(line, ctx))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_patterns_match_nothing() {
        let (matcher, warnings) = AdMatcher::new(&AdMatch { path: "(".into(), title: "(?=x)".into() });
        assert_eq!(warnings.len(), 2);
        assert!(warnings[0].starts_with("ad_match.path \"(\" is not a valid regular expression"), "{}", warnings[0]);
        assert!(!matcher.is_ad(&ClipContext { source: "(".into(), title: "x".into(), ..ClipContext::default() }));
    }

    #[test]
    fn file_stem() {
        assert_eq!(stem("/m/plansza.mp4"), "plansza");
        assert_eq!(stem("/m/Noc (2006).final.mkv"), "Noc (2006).final");
        assert_eq!(stem("/m/.hidden"), ".hidden");
        assert_eq!(stem("plik"), "plik");
        assert_eq!(stem(""), "");
    }

    #[test]
    fn show_matcher_overrides_and_warnings() {
        use crate::config::ShowOverride;

        let (matcher, warnings) = ShowMatcher::new(&[
            ShowOverride {
                show_title: "Wiadomości.*".into(),
                on_clip: Some("TAKE 1001".into()),
                on_ad: Some("TAKE 2001".into()),
                ad_match: Some(AdMatch { path: "reklama_news".into(), title: "".into() }),
                ..ShowOverride::default()
            },
            ShowOverride {
                show_title: "[bad".into(),
                on_clip: None,
                on_ad: None,
                ad_match: Some(AdMatch { path: "(".into(), title: "".into() }),
                ..ShowOverride::default()
            },
        ]);

        assert_eq!(warnings.len(), 2);
        assert!(warnings[0].contains("shows[1].show_title"));
        assert!(warnings[1].contains("shows[1].ad_match.path"));

        let found = matcher.find("Wiadomości wieczorne").unwrap();
        assert_eq!(found.on_clip.as_deref(), Some("TAKE 1001"));
        assert_eq!(found.on_ad.as_deref(), Some("TAKE 2001"));
        let ad_ctx = ClipContext { source: "/media/reklama_news_1.mp4".into(), ..ClipContext::default() };
        assert!(found.ad_matcher.as_ref().unwrap().is_ad(&ad_ctx));

        assert!(matcher.find("Inny program").is_none());
        assert!(matcher.find("").is_none());
    }
}
