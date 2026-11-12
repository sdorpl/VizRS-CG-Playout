// Page bounds from VizRS (PLAN-strefy S6). With `area: auto` the sidecar
// asks the native renderer where each page of the show ever draws:
// `vizrs-render bounds` plays the page like on air (in, its stop points with
// a rest for loops, tickers and clocks, out) and gives the 16 px cells it
// drew, in channel pixels. Those bounds make a page known before it is ever
// seen on air – also pages taken together, from which no footprint is learnt
// – and the learnt footprint adds to them (a longer text than the page's own
// values). The show comes from VizRS's HTTP API (`GET /api/show`, operator
// token); a page is computed again only when its revision or its scene's
// changes. A page the native renderer does not fully draw gets no bounds.

use std::collections::HashMap;
use std::process::Command;

use crate::json::{self, Value};

/// A rectangle (x, y, w, h) in channel pixels.
pub type Rect = (u32, u32, u32, u32);

/// `GET /api/show` of the VizRS whose output is `output_url` (`http://vizrs:8088/output?token=…`).
pub fn show_url(output_url: &str) -> Option<String> {
    let parts = crate::net::Url::split(output_url)?;
    (parts.scheme == "http" && !parts.netloc.is_empty()).then(|| format!("http://{}/api/show", parts.netloc))
}

/// The show, with the operator token.
pub fn fetch_show(output_url: &str, token: &str) -> Result<String, String> {
    let url = show_url(output_url).ok_or_else(|| format!("no VizRS address in {output_url}"))?;
    let auth = if token.is_empty() { String::new() } else { format!("Authorization: Bearer {token}\r\n") };
    crate::net::get(&url, &auth).map_err(|e| e.to_string())
}

/// The show's revision in a `STATE` reply (`OK {…,"rev":185}`).
pub fn state_rev(reply: &str) -> Option<f64> {
    json::parse(reply.strip_prefix("OK")?.trim()).ok()?.get("rev")?.as_f64()
}

fn text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) if n.fract() == 0.0 => format!("{n:.0}"),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// A page of the show: its key as in `STATE` (`id:number`), its number, and
/// what its bounds come from (the page's revision and its scene's).
#[derive(Debug, PartialEq)]
pub struct Page {
    pub key: String,
    pub number: String,
    pub signature: String,
}

/// The pages of `show` (JSON of `/api/show`).
pub fn pages(show: &Value) -> Vec<Page> {
    let scenes = show.get("scenes").and_then(Value::as_array).unwrap_or(&[]);
    let mut out = Vec::new();
    for page in show.get("pages").and_then(Value::as_array).unwrap_or(&[]) {
        let number = text(page.get("number"));
        if number.is_empty() {
            continue;
        }
        let scene_id = text(page.get("scene"));
        let scene = scenes
            .iter()
            .find(|s| text(s.get("id")) == scene_id || text(s.get("name")) == scene_id)
            .map(|s| text(s.get("rev")))
            .unwrap_or_default();
        out.push(Page {
            key: format!("{}:{number}", text(page.get("id"))),
            signature: format!("{}|{scene_id}|{scene}", text(page.get("rev"))),
            number,
        });
    }
    out
}

/// One line of `vizrs-render bounds`: the page key and its rectangles (x, y,
/// w, h) in channel pixels; `None` = no bounds (not drawn natively, or an error).
pub fn parse_line(line: &str) -> Option<(String, Option<Vec<Rect>>)> {
    let v = json::parse(line).ok()?;
    let key = format!("{}:{}", text(v.get("id")), text(v.get("number")));
    let native = v.get("error").is_none() && v.get("issues").and_then(Value::as_array).is_some_and(|i| i.is_empty());
    let rects = v.get("rects").and_then(Value::as_array).filter(|_| native).map(|list| {
        list.iter()
            .filter_map(|r| {
                let r = r.as_array()?;
                let n = |i: usize| r.get(i).and_then(Value::as_f64).filter(|v| *v >= 0.0).map(|v| v as u32);
                Some((n(0)?, n(1)?, n(2)?, n(3)?))
            })
            .collect()
    });
    Some((key, rects))
}

/// Runs `bin bounds` for the pages `numbers` of the show in `show_file` at `size`; its output.
pub fn run(bin: &str, show_file: &str, output_url: &str, size: (u32, u32), numbers: &[String]) -> Result<String, String> {
    let mut cmd = Command::new(bin);
    cmd.args(["bounds", "--show", show_file, "--url", output_url, "--size", &format!("{}x{}", size.0, size.1)]);
    for n in numbers {
        cmd.args(["--page", n]);
    }
    let out = cmd.output().map_err(|e| format!("{bin}: {e}"))?;
    if !out.status.success() {
        return Err(format!("{bin} bounds: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// What has been computed: `id:number@WxH` → signature.
pub type Done = HashMap<String, String>;

pub fn done_key(page: &str, (w, h): (u32, u32)) -> String {
    format!("{page}@{w}x{h}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn show_address_from_the_output_url() {
        assert_eq!(show_url("http://vizrs:8088/output?token=t&bus=ch-1").as_deref(), Some("http://vizrs:8088/api/show"));
        assert_eq!(show_url("https://x/output"), None);
        assert_eq!(show_url("nonsense"), None);
    }

    #[test]
    fn revision_of_the_show_in_state() {
        assert_eq!(state_rev(r#"OK {"layers":[],"rev":185}"#), Some(185.0));
        assert_eq!(state_rev("ERR no"), None);
    }

    #[test]
    fn pages_with_their_signatures() {
        let show = json::parse(
            r#"{"scenes":[{"id":"s1","rev":3},{"id":"s2"}],"pages":[
                {"id":"p1","number":1001,"scene":"s1","rev":2},{"id":"p2","number":"7507","scene":"s2"},{"id":"p3","scene":"s1"}]}"#,
        )
        .unwrap();
        assert_eq!(
            pages(&show),
            vec![
                Page { key: "p1:1001".into(), number: "1001".into(), signature: "2|s1|3".into() },
                Page { key: "p2:7507".into(), number: "7507".into(), signature: "|s2|".into() },
            ]
        );
    }

    #[test]
    fn renderer_lines() {
        let ok = r#"{"cell":16,"id":"p1","issues":[],"number":"1001","rects":[[32,672,1536,16],[16,688,1568,160]],"size":"1600x900"}"#;
        assert_eq!(parse_line(ok), Some(("p1:1001".into(), Some(vec![(32, 672, 1536, 16), (16, 688, 1568, 160)]))));
        let not_native = r#"{"id":"p2","number":"1002","rects":[[0,0,16,16]],"issues":["ticker: font"]}"#;
        assert_eq!(parse_line(not_native), Some(("p2:1002".into(), None)));
        assert_eq!(parse_line(r#"{"id":"p3","number":"1003","error":"brak sceny x"}"#), Some(("p3:1003".into(), None)));
        assert_eq!(parse_line("garbage"), None);
    }
}
