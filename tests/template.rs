// Clip context, commands and ads as the Node.js build gives them
// (tests/vectors/template.json, vizrs.json).

mod common;

use vizrs_cg::config::AdMatch;
use vizrs_cg::json::Value;
use vizrs_cg::template::{AdMatcher, ClipContext, clean_value, clock, render_commands, time_span_seconds};
use vizrs_cg::time::parse_iso;
use vizrs_cg::vizrs::on_air;

use common::{assert_same, vectors};

fn cases<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key).and_then(Value::as_array).unwrap_or_else(|| panic!("{key}"))
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_owned()
}

/// The context back from its JSON form (the vectors store the Node.js one).
fn context(v: &Value) -> ClipContext {
    let n = |key| v.get(key).and_then(Value::as_f64).unwrap_or(0.0);
    ClipContext {
        title: text(v, "title"),
        show_title: text(v, "show_title"),
        source: text(v, "source"),
        start: text(v, "start"),
        stop: text(v, "stop"),
        duration: n("duration"),
        seek: n("seek"),
        next_title: text(v, "next_title"),
        next_start_full: text(v, "next_start_full"),
        next_duration: n("next_duration"),
        width: n("width") as u32,
        height: n("height") as u32,
        fps: n("fps"),
    }
}

#[test]
fn clip_context_matches_node() {
    let v = vectors("template");
    for case in cases(&v, "contexts") {
        let ctx = ClipContext::from_data(case.get("data").unwrap());
        assert_same(&ctx.to_json(), case.get("ctx").unwrap(), &format!("data {}", case.get("data").unwrap()));
    }
}

#[test]
fn commands_match_node() {
    let v = vectors("template");
    for case in cases(&v, "commands") {
        let ctx = context(case.get("ctx").unwrap());
        let commands: Vec<Value> = render_commands(&text(case, "template"), &ctx).into_iter().map(Value::from).collect();
        assert_same(&Value::Array(commands), case.get("commands").unwrap(), &format!("template {:?}", text(case, "template")));
    }
}

#[test]
fn time_spans_clocks_and_values_match_node() {
    let v = vectors("template");
    for case in cases(&v, "time_spans") {
        let input = case.get("input").filter(|i| !i.is_null());
        assert_eq!(Value::from(time_span_seconds(input)), *case.get("seconds").unwrap(), "{:?}", input);
    }
    for case in cases(&v, "clocks") {
        assert_eq!(clock(case.get("seconds").unwrap().as_f64().unwrap()), text(case, "clock"), "{case}");
    }
    for case in cases(&v, "clean") {
        let input = match case.get("input").unwrap() {
            Value::Null => String::new(),
            other => vizrs_cg::template::js_string(other),
        };
        assert_eq!(clean_value(&input), text(case, "value"), "{case}");
    }
}

#[test]
fn ads_match_node() {
    let v = vectors("template");
    for case in cases(&v, "ads") {
        let m = case.get("ad_match").unwrap();
        let (matcher, warnings) = AdMatcher::new(&AdMatch { path: text(m, "path"), title: text(m, "title") });
        assert!(warnings.is_empty(), "{warnings:?}");
        let c = case.get("ctx").unwrap();
        let ctx = ClipContext { source: text(c, "source"), title: text(c, "title"), ..ClipContext::default() };
        assert_eq!(Value::from(matcher.is_ad(&ctx)), *case.get("ad").unwrap(), "{case}");
    }
}

#[test]
fn dates_match_node() {
    let v = vectors("template");
    for case in cases(&v, "dates") {
        let expected = case.get("ms").unwrap().as_f64().map(|ms| ms as i64);
        assert_eq!(parse_iso(&text(case, "input")), expected, "{case}");
    }
}

#[test]
fn state_replies_match_node() {
    let v = vectors("vizrs");
    for case in cases(&v, "on_air") {
        let result = match on_air(&text(case, "reply")) {
            Some((out, pages)) => vizrs_cg::json::object([("out", out.into()), ("pages", pages.into())]),
            None => Value::Null,
        };
        assert_same(&result, case.get("result").unwrap(), &text(case, "reply"));
    }
}
