// ErsatzTV's own API, for the Live Panel (docs/plany/PLAN-live-panel.md §6a,
// DOKUMENTACJA §1 „Live Panel”): channel names, library scans, smart
// collections of the live programmes, playout resets and the guide (XMLTV).
// The key comes from `api-secrets.json` in ErsatzTV's configuration folder,
// as for the channel sync (src/etv_sync.rs). Nothing here is on the air path.

use std::fs;
use std::io;
use std::path::Path;

use crate::json::{self, Value};
use crate::net::{self, error};

/// ErsatzTV as the sidecar reaches it.
#[derive(Clone)]
pub struct Etv {
    /// e.g. `http://ersatztv:8409`, no trailing slash.
    pub url: String,
    /// ErsatzTV's configuration folder (its `api-secrets.json`).
    pub config: String,
}

/// A smart collection of ErsatzTV.
#[derive(Clone, Debug, PartialEq)]
pub struct Collection {
    pub id: u64,
    pub name: String,
    pub query: String,
}

impl Etv {
    pub fn configured(&self) -> bool {
        !self.url.is_empty()
    }

    fn headers(&self) -> String {
        let key = fs::read_to_string(Path::new(&self.config).join("api-secrets.json"))
            .ok()
            .and_then(|text| json::parse(&text).ok())
            .and_then(|secrets| secrets.get("ApiKey").and_then(Value::as_str).map(str::to_owned))
            .unwrap_or_default();
        if key.is_empty() { String::new() } else { format!("X-Etv-Api-Key: {key}\r\n") }
    }

    fn call(&self, method: &str, path: &str, body: Option<&str>) -> io::Result<String> {
        if !self.configured() {
            return Err(error("no etv_url".into()));
        }
        let (status, reply) = net::request(method, &format!("{}{path}", self.url), &self.headers(), body)?;
        if (200..300).contains(&status) {
            Ok(reply)
        } else {
            let detail: String = reply.trim().chars().take(200).collect();
            Err(error(format!("ErsatzTV {method} {path}: HTTP {status} {detail}")))
        }
    }

    /// Numbers and names of the channels, in ErsatzTV's order.
    pub fn channels(&self) -> io::Result<Vec<(String, String)>> {
        let list = json::parse(&self.call("GET", "/api/channels", None)?).map_err(|e| error(e.to_string()))?;
        let list = list.as_array().ok_or_else(|| error("channels: expected an array".into()))?;
        Ok(list.iter().filter_map(|c| Some((c.get("number")?.as_str()?.to_owned(), c.get("name")?.as_str()?.to_owned()))).collect())
    }

    /// Queues a scan of a library (`POST /api/libraries/<id>/scan`).
    pub fn scan(&self, library: u64) -> io::Result<()> {
        self.call("POST", &format!("/api/libraries/{library}/scan"), None).map(drop)
    }

    /// Rebuilds the playout of a channel from its schedule.
    pub fn reset_playout(&self, channel: &str) -> io::Result<()> {
        self.call("POST", &format!("/api/channels/{}/playout/reset", crate::overlay::quote_plus(channel)), None).map(drop)
    }

    pub fn collections(&self) -> io::Result<Vec<Collection>> {
        let list = json::parse(&self.call("GET", "/api/collections/smart", None)?).map_err(|e| error(e.to_string()))?;
        let list = list.as_array().ok_or_else(|| error("smart collections: expected an array".into()))?;
        Ok(list
            .iter()
            .filter_map(|c| {
                Some(Collection {
                    id: c.get("id")?.as_f64()? as u64,
                    name: c.get("name")?.as_str()?.to_owned(),
                    query: c.get("query").and_then(Value::as_str).unwrap_or("").to_owned(),
                })
            })
            .collect())
    }

    /// A new smart collection; its id.
    pub fn create_collection(&self, name: &str, query: &str) -> io::Result<u64> {
        let body = json::object([("query", query.into()), ("name", name.into())]).to_string();
        let reply = self.call("POST", "/api/collections/smart/new", Some(&body))?;
        let id = json::parse(&reply).ok().and_then(|v| v.get("id").and_then(Value::as_f64));
        match id {
            Some(id) => Ok(id as u64),
            // An older reply without the id: look the collection up by name.
            None => self
                .collections()?
                .into_iter()
                .find(|c| c.name == name)
                .map(|c| c.id)
                .ok_or_else(|| error("collection created without an id".into())),
        }
    }

    pub fn update_collection(&self, id: u64, name: &str, query: &str) -> io::Result<()> {
        let body = json::object([("id", id.into()), ("name", name.into()), ("query", query.into())]).to_string();
        self.call("PUT", "/api/collections/smart/update", Some(&body)).map(drop)
    }

    pub fn delete_collection(&self, id: u64) -> io::Result<()> {
        self.call("DELETE", &format!("/api/collections/smart/delete/{id}"), None).map(drop)
    }

    /// The guide of all channels.
    pub fn guide(&self) -> io::Result<Vec<Programme>> {
        Ok(parse_xmltv(&self.call("GET", "/iptv/xmltv.xml", None)?))
    }
}

/// ErsatzTV's sort title (`SortTitle.GetSortTitle`), lower case as in its search index.
pub fn sort_title(title: &str) -> String {
    let lower = title.to_lowercase();
    let rest = ["the ", "a ", "an "].iter().find_map(|article| lower.strip_prefix(article)).unwrap_or(&lower);
    rest.replace('æ', "e")
}

/// The smart collection query of a live programme: the Remote Stream of that
/// library whose title is exactly this one (`sort_title` is not tokenised, so
/// „Wiadomości” does not take „Wiadomości wieczorne”).
pub fn show_query(library: u64, title: &str) -> String {
    let phrase = sort_title(title).replace('\\', "\\\\").replace('"', "\\\"");
    format!("type:remote_stream AND library_id:{library} AND sort_title:\"{phrase}\"")
}

/// The name of a live programme's collection.
pub fn collection_name(title: &str) -> String {
    format!("LIVE · {title}")
}

/// One item of the guide.
#[derive(Clone, Debug, PartialEq)]
pub struct Programme {
    /// The channel number (ErsatzTV's channel id is `C<number>.<id>.ersatztv.org`).
    pub channel: String,
    pub title: String,
    pub start_ms: i64,
    pub stop_ms: i64,
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let at = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    let end = tag[at..].find('"')?;
    Some(unescape(&tag[at..at + end]))
}

fn unescape(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';').filter(|&e| e <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let c = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match c {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// `20261106133920 +0100` of XMLTV as milliseconds since the epoch.
pub fn xmltv_time(text: &str) -> Option<i64> {
    let text = text.trim();
    let digits = text.get(..14).filter(|d| d.bytes().all(|b| b.is_ascii_digit()))?;
    let n = |range: std::ops::Range<usize>| digits[range].parse::<i64>().ok();
    let (year, month, day) = (n(0..4)?, n(4..6)? as u32, n(6..8)?);
    let (hour, minute, second) = (n(8..10)?, n(10..12)?, n(12..14)?);
    if !(1..=12).contains(&month) {
        return None;
    }
    let days = crate::time::days_from_civil(year, month) + day - 1;
    let mut seconds = days * 86_400 + hour * 3600 + minute * 60 + second;
    let zone = text[14..].trim();
    if zone.len() == 5 && (zone.starts_with('+') || zone.starts_with('-')) {
        let offset = zone[1..3].parse::<i64>().ok()? * 3600 + zone[3..5].parse::<i64>().ok()? * 60;
        seconds -= if zone.starts_with('-') { -offset } else { offset };
    }
    Some(seconds * 1000)
}

/// The channel number of an ErsatzTV XMLTV channel id (`C1.145.ersatztv.org` → `1`).
fn channel_number(id: &str) -> String {
    // Numbers like 2.5 keep their dot: everything between `C` and the last part before the domain.
    match id.strip_prefix('C').and_then(|rest| rest.strip_suffix(".ersatztv.org")) {
        Some(inner) => inner.rsplit_once('.').map_or(inner, |(number, _)| number).to_owned(),
        None => id.to_owned(),
    }
}

/// The programmes of an XMLTV guide, in its order.
pub fn parse_xmltv(text: &str) -> Vec<Programme> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("<programme ") {
        rest = &rest[at..];
        let Some(head_end) = rest.find('>') else { break };
        let head = &rest[..head_end];
        let end = rest.find("</programme>").unwrap_or(rest.len());
        let body = &rest[head_end..end];
        let title = body.find("<title").and_then(|t| {
            let open = body[t..].find('>')? + t + 1;
            let close = body[open..].find("</title>")? + open;
            Some(unescape(body[open..close].trim()))
        });
        let start = attribute(head, "start").and_then(|s| xmltv_time(&s));
        let stop = attribute(head, "stop").and_then(|s| xmltv_time(&s));
        if let (Some(channel), Some(title), Some(start_ms), Some(stop_ms)) = (attribute(head, "channel"), title, start, stop) {
            out.push(Programme { channel: channel_number(&channel), title, start_ms, stop_ms });
        }
        if end == rest.len() {
            break;
        }
        rest = &rest[end..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries_match_one_title() {
        assert_eq!(sort_title("The Wiadomości"), "wiadomości");
        assert_eq!(sort_title("Æon"), "eon");
        assert_eq!(show_query(7, "Rozmowa \"dnia\""), r#"type:remote_stream AND library_id:7 AND sort_title:"rozmowa \"dnia\"""#);
        assert_eq!(collection_name("Wiadomości"), "LIVE · Wiadomości");
    }

    #[test]
    fn guide() {
        assert_eq!(xmltv_time("20261106133920 +0100"), Some(1_793_968_760_000));
        assert_eq!(xmltv_time("20261106123920 +0000"), xmltv_time("20261106133920 +0100"));
        assert_eq!(xmltv_time("2026"), None);
        assert_eq!(channel_number("C1.145.ersatztv.org"), "1");
        assert_eq!(channel_number("C2.5.17.ersatztv.org"), "2.5");
        let text = "\u{feff}<?xml version=\"1.0\"?><tv><channel id=\"C1.145.ersatztv.org\"><display-name>1</display-name></channel>\
            <programme start=\"20261106133920 +0100\" stop=\"20261106134120 +0100\" channel=\"C1.145.ersatztv.org\"><title lang=\"en\">Tom &amp; Jerry</title></programme>\
            <programme start=\"20261106134120 +0100\" stop=\"20261106134320 +0100\" channel=\"C4.9.ersatztv.org\"><title>Wiadomo&#347;ci</title><previously-shown/></programme>\
            <programme start=\"bad\" stop=\"x\" channel=\"C4.9.ersatztv.org\"><title>X</title></programme></tv>";
        let guide = parse_xmltv(text);
        assert_eq!(guide.len(), 2);
        assert_eq!(
            guide[0],
            Programme { channel: "1".into(), title: "Tom & Jerry".into(), start_ms: 1_793_968_760_000, stop_ms: 1_793_968_880_000 }
        );
        assert_eq!((guide[1].channel.as_str(), guide[1].title.as_str()), ("4", "Wiadomości"));
    }
}
