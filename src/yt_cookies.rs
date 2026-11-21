// YouTube cookies for the yt-dlp of YouTube inputs (DOKUMENTACJA §1, Live
// ingest): YouTube asks a server (an address of a data centre) to sign in, so
// yt-dlp needs the cookies of a signed-in account. VizRS Live uploads them
// (`PUT /api/live/youtube-cookies`, src/panel.rs) to
// `<etv_config>/vizrs-live/youtube-cookies.txt` (mode 0600); the ingest
// service sees the same file (`/etv-config/…` in its container) and hands
// yt-dlp a private copy of it (src/listener.rs), which yt-dlp may refresh.
//
// Accepted: the Netscape cookies file (`cookies.txt`, what yt-dlp reads) and
// the JSON a browser extension exports (a list of `{domain, name, value, path,
// secure, hostOnly, httpOnly, expirationDate}`), stored as Netscape. The
// values are a sign-in: they never go to a log or back to the panel.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use crate::json::{self, Value};

/// The file in `<etv_config>/vizrs-live/`.
pub const FILE: &str = "youtube-cookies.txt";
/// Next to it: the time YouTube ended the session of those cookies (the ingest
/// service writes it, a new upload removes it).
pub const ENDED: &str = "youtube-cookies.ended";
/// The longest file the panel may upload (a whole browser's export is longer: only youtube.com is needed).
pub const MAX_TEXT: usize = 512 * 1024;
/// The most cookies in one file.
const MAX_COOKIES: usize = 5000;
/// Cookies of a signed-in Google account on youtube.com (their expiry is shown).
const SIGN_IN: [&str; 5] = ["SID", "__Secure-1PSID", "__Secure-3PSID", "SAPISID", "LOGIN_INFO"];
/// A session yt-dlp signs in with: `LOGIN_INFO` and one of the API keys it
/// signs requests with. When YouTube ends a session it deletes `LOGIN_INFO`,
/// `SID`, `SAPISID`… and leaves the third-party ones (`__Secure-3PSID`).
const SESSION_KEYS: [&str; 3] = ["SAPISID", "__Secure-1PAPISID", "__Secure-3PAPISID"];
const HEADER: &str = "# Netscape HTTP Cookie File";

/// One cookie (a line of the Netscape file).
#[derive(Debug, PartialEq)]
pub struct Cookie {
    pub domain: String,
    pub subdomains: bool,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    /// Unix seconds; 0 = a session cookie.
    pub expires: i64,
    pub name: String,
    pub value: String,
}

impl Cookie {
    fn youtube(&self) -> bool {
        let domain = self.domain.trim_start_matches('.').to_ascii_lowercase();
        domain == "youtube.com" || domain.ends_with(".youtube.com")
    }

    fn line(&self) -> String {
        let flag = |b: bool| if b { "TRUE" } else { "FALSE" };
        format!(
            "{}{}\t{}\t{}\t{}\t{}\t{}\t{}",
            if self.http_only { "#HttpOnly_" } else { "" },
            self.domain,
            flag(self.subdomains),
            self.path,
            flag(self.secure),
            self.expires,
            self.name,
            self.value
        )
    }
}

/// What the panel shows of a cookies file (never the values).
#[derive(Debug, PartialEq)]
pub struct Summary {
    pub cookies: usize,
    /// Cookies of youtube.com.
    pub youtube: usize,
    /// A signed-in account's cookies are among them (`SIGN_IN`).
    pub signed_in: bool,
    /// The first of the sign-in cookies to expire (Unix seconds), none = session cookies only.
    pub expires: Option<i64>,
}

fn clean(field: &str, what: &str, line: usize) -> Result<String, String> {
    if field.chars().any(char::is_control) {
        return Err(format!("wiersz {line}: {what} ze znakiem sterującym"));
    }
    Ok(field.to_owned())
}

fn flag(field: &str, line: usize) -> Result<bool, String> {
    match field.to_ascii_uppercase().as_str() {
        "TRUE" => Ok(true),
        "FALSE" => Ok(false),
        _ => Err(format!("wiersz {line}: „{}” zamiast TRUE albo FALSE", field.chars().take(20).collect::<String>())),
    }
}

/// Seconds of an expiry (an integer, or a fraction some exporters write);
/// empty (yt-dlp's own file) or negative = a session cookie.
fn seconds(text: &str) -> Option<i64> {
    let text = text.trim();
    if text.is_empty() {
        return Some(0);
    }
    text.parse::<i64>().ok().or_else(|| text.parse::<f64>().ok().filter(|s| s.is_finite()).map(|s| s as i64)).map(|s| s.max(0))
}

fn netscape(text: &str) -> Result<Vec<Cookie>, String> {
    let mut cookies = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = n + 1;
        let raw = raw.trim_end_matches('\r');
        let (http_only, data) = match raw.strip_prefix("#HttpOnly_") {
            Some(rest) => (true, rest),
            None => (false, raw),
        };
        if data.trim().is_empty() || (!http_only && data.starts_with('#')) {
            continue;
        }
        let fields: Vec<&str> = data.split('\t').collect();
        // An empty value: some exporters leave out the last tab.
        if !(6..=7).contains(&fields.len()) {
            return Err(format!(
                "wiersz {line}: to nie jest plik cookies w formacie Netscape (7 pól rozdzielonych tabulatorem, ma {})",
                fields.len()
            ));
        }
        let expires = seconds(fields[4]).ok_or(format!("wiersz {line}: zła data wygaśnięcia"))?;
        let cookie = Cookie {
            domain: clean(fields[0].trim(), "domena", line)?,
            subdomains: flag(fields[1].trim(), line)?,
            path: clean(fields[2].trim(), "ścieżka", line)?,
            secure: flag(fields[3].trim(), line)?,
            http_only,
            expires,
            name: clean(fields[5], "nazwa", line)?,
            value: clean(fields.get(6).copied().unwrap_or(""), "wartość", line)?,
        };
        if cookie.domain.is_empty() || cookie.name.is_empty() {
            return Err(format!("wiersz {line}: brak domeny albo nazwy"));
        }
        cookies.push(cookie);
    }
    Ok(cookies)
}

fn from_json(items: &[Value]) -> Result<Vec<Cookie>, String> {
    let mut cookies = Vec::new();
    for (n, item) in items.iter().enumerate() {
        let at = n + 1;
        let text = |name: &str| item.get(name).and_then(Value::as_str);
        let truth = |name: &str| item.get(name).and_then(Value::as_bool).unwrap_or(false);
        let (Some(domain), Some(name)) = (text("domain"), text("name")) else {
            return Err(format!("ciasteczko {at}: brak pól domain i name (eksport JSON z rozszerzenia przeglądarki)"));
        };
        let expires = match item.get("expirationDate").or_else(|| item.get("expires")) {
            Some(Value::Number(s)) if s.is_finite() => (*s as i64).max(0),
            _ => 0,
        };
        let cookie = Cookie {
            domain: clean(domain.trim(), "domena", at)?,
            subdomains: item.get("hostOnly").and_then(Value::as_bool).map_or(domain.starts_with('.'), |host| !host),
            path: clean(text("path").filter(|p| !p.is_empty()).unwrap_or("/"), "ścieżka", at)?,
            secure: truth("secure"),
            http_only: truth("httpOnly"),
            expires,
            name: clean(name, "nazwa", at)?,
            value: clean(text("value").unwrap_or(""), "wartość", at)?,
        };
        if cookie.domain.is_empty() || cookie.name.is_empty() || cookie.name.contains('\t') || cookie.value.contains('\t') {
            return Err(format!("ciasteczko {at}: pusta domena albo nazwa"));
        }
        cookies.push(cookie);
    }
    Ok(cookies)
}

/// The cookies of an uploaded file: Netscape or a browser extension's JSON.
pub fn parse(text: &str) -> Result<Vec<Cookie>, String> {
    let text = text.trim_start_matches('\u{feff}');
    let trimmed = text.trim_start();
    let cookies = if trimmed.starts_with('[') || trimmed.starts_with('{') {
        match json::parse(trimmed) {
            Ok(Value::Array(items)) => from_json(&items)?,
            Ok(value) => match value.get("cookies") {
                Some(Value::Array(items)) => from_json(items)?,
                _ => return Err("JSON bez listy ciasteczek".into()),
            },
            Err(e) => return Err(format!("zły JSON: {e}")),
        }
    } else {
        netscape(text)?
    };
    if cookies.len() > MAX_COOKIES {
        return Err(format!("za dużo ciasteczek ({}, najwyżej {MAX_COOKIES}) – wyeksportuj tylko youtube.com", cookies.len()));
    }
    if !cookies.iter().any(Cookie::youtube) {
        return Err("w pliku nie ma ciasteczek youtube.com – wyeksportuj je z karty youtube.com, na której jesteś zalogowany".into());
    }
    Ok(cookies)
}

/// The Netscape file yt-dlp reads.
pub fn to_netscape(cookies: &[Cookie]) -> String {
    let mut text = format!("{HEADER}\n# VizRS CG Playout: ciasteczka YouTube z VizRS Live (yt-dlp, wejścia YouTube)\n\n");
    for cookie in cookies {
        text.push_str(&cookie.line());
        text.push('\n');
    }
    text
}

/// The cookies hold a session yt-dlp can sign in with (`SESSION_KEYS`).
pub fn signed_in(cookies: &[Cookie]) -> bool {
    let has = |name: &str| cookies.iter().any(|c| c.youtube() && c.name == name);
    has("LOGIN_INFO") && SESSION_KEYS.iter().any(|key| has(key))
}

pub fn summary(cookies: &[Cookie]) -> Summary {
    let sign_in: Vec<&Cookie> = cookies.iter().filter(|c| c.youtube() && SIGN_IN.contains(&c.name.as_str())).collect();
    Summary {
        cookies: cookies.len(),
        youtube: cookies.iter().filter(|c| c.youtube()).count(),
        signed_in: signed_in(cookies),
        expires: sign_in.iter().map(|c| c.expires).filter(|&s| s > 0).min(),
    }
}

pub fn path(folder: &Path) -> PathBuf {
    folder.join(FILE)
}

/// When YouTube ended the session of the stored cookies (ISO time), if it did.
pub fn ended(folder: &Path) -> Option<String> {
    let text = fs::read_to_string(folder.join(ENDED)).ok()?;
    let text = text.trim();
    (!text.is_empty() && text.len() < 40).then(|| text.to_owned())
}

/// Writes the file (mode 0600: it is a sign-in) in place of the old one at once;
/// new cookies, a new session: the mark of an ended one goes.
pub fn save(folder: &Path, text: &str) -> io::Result<()> {
    fs::create_dir_all(folder)?;
    let _ = fs::remove_file(folder.join(ENDED));
    let target = path(folder);
    let temporary = folder.join(format!(".{FILE}.{}.tmp", std::process::id()));
    let written = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temporary)
        .and_then(|mut file| file.write_all(text.as_bytes()).and_then(|()| file.sync_all()))
        .and_then(|()| fs::rename(&temporary, &target));
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

/// Removes the file; none is fine.
pub fn remove(folder: &Path) -> io::Result<()> {
    let _ = fs::remove_file(folder.join(ENDED));
    match fs::remove_file(path(folder)) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE_TEXT: &str = "# Netscape HTTP Cookie File\r\n# comment\r\n\r\n.youtube.com\tTRUE\t/\tTRUE\t1893456000\t__Secure-3PSID\tabc\r\n#HttpOnly_.youtube.com\tTRUE\t/\tTRUE\t1800000000\tLOGIN_INFO\txyz\r\n.youtube.com\tTRUE\t/\tFALSE\t0\tPREF\r\n.google.com\tTRUE\t/\tTRUE\t1700000000\tSID\tg\r\n";

    #[test]
    fn netscape_file() {
        let cookies = parse(FILE_TEXT).unwrap();
        assert_eq!(cookies.len(), 4);
        assert!(cookies[1].http_only);
        assert_eq!(cookies[2].value, "", "a value left out with its tab");
        assert_eq!(
            summary(&cookies),
            Summary { cookies: 4, youtube: 3, signed_in: false, expires: Some(1_800_000_000) },
            "sign-in cookies of youtube.com only; LOGIN_INFO without an API key is no session"
        );
        let text = to_netscape(&cookies);
        assert!(text.starts_with(HEADER));
        assert!(text.contains("\n#HttpOnly_.youtube.com\tTRUE\t/\tTRUE\t1800000000\tLOGIN_INFO\txyz\n"));
        assert_eq!(parse(&text).unwrap(), cookies, "stored as read");
    }

    #[test]
    fn browser_extension_json() {
        let text = r#"[{"domain":".youtube.com","hostOnly":false,"httpOnly":true,"name":"SID","path":"/","secure":true,"expirationDate":1893456000.5,"value":"v"},
                       {"domain":"www.youtube.com","hostOnly":true,"name":"VISITOR","value":"w","session":true}]"#;
        let cookies = parse(text).unwrap();
        assert_eq!(cookies[0].line(), "#HttpOnly_.youtube.com\tTRUE\t/\tTRUE\t1893456000\tSID\tv");
        assert_eq!(cookies[1].line(), "www.youtube.com\tFALSE\t/\tFALSE\t0\tVISITOR\tw");
        assert!(parse(r#"{"cookies":[{"domain":".youtube.com","name":"a","value":"b"}]}"#).is_ok());
    }

    #[test]
    fn a_session_yt_dlp_signs_in_with() {
        let file = |names: &[&str]| -> Vec<Cookie> {
            parse(&names.iter().map(|n| format!(".youtube.com\tTRUE\t/\tTRUE\t1893456000\t{n}\tv\n")).collect::<String>()).unwrap()
        };
        assert!(signed_in(&file(&["LOGIN_INFO", "SAPISID", "SID"])));
        assert!(signed_in(&file(&["LOGIN_INFO", "__Secure-3PAPISID"])));
        // What YouTube leaves of an ended session (S1, 2026-11-20).
        assert!(!signed_in(&file(&["__Secure-3PSID", "__Secure-3PAPISID", "__Secure-1PSIDTS", "PREF", "VISITOR_INFO1_LIVE"])));
        let folder = std::env::temp_dir().join(format!("vizrs-yt-ended-{}", std::process::id()));
        save(&folder, "x").unwrap();
        assert_eq!(ended(&folder), None);
        fs::write(folder.join(ENDED), "2026-11-21T00:00:00.000Z\n").unwrap();
        assert_eq!(ended(&folder).as_deref(), Some("2026-11-21T00:00:00.000Z"));
        save(&folder, "y").unwrap();
        assert_eq!(ended(&folder), None, "a new upload, a new session");
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_file_ytdlp_writes_back() {
        // Python's MozillaCookieJar: no expiry for a session cookie.
        let text = "# Netscape HTTP Cookie File\n# http://curl.haxx.se/rfc/cookie_spec.html\n# This is a generated file!  Do not edit.\n\n.youtube.com\tTRUE\t/\tTRUE\t\tYSC\tx\n.youtube.com\tTRUE\t/\tTRUE\t1893456000\tSID\ts\n";
        let cookies = parse(text).unwrap();
        assert_eq!(cookies[0].expires, 0);
        assert_eq!(summary(&cookies).expires, Some(1_893_456_000));
    }

    #[test]
    fn wrong_files_are_refused() {
        assert!(parse("").unwrap_err().contains("youtube.com"));
        assert!(parse(".google.com\tTRUE\t/\tTRUE\t0\tSID\tg").unwrap_err().contains("youtube.com"));
        assert!(parse("SID=abc; HSID=def").unwrap_err().contains("Netscape"));
        assert!(parse(".youtube.com\tYES\t/\tTRUE\t0\tSID\tg").unwrap_err().contains("TRUE albo FALSE"));
        assert!(parse(".youtube.com\tTRUE\t/\tTRUE\tjutro\tSID\tg").unwrap_err().contains("wygaśnięcia"));
        assert!(parse("[1, 2]").unwrap_err().contains("domain"));
        assert!(parse(r#"[{"domain":".youtube.com","name":"a","value":"b\nc"}]"#).unwrap_err().contains("sterującym"));
    }

    #[test]
    fn saved_for_the_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let folder = std::env::temp_dir().join(format!("vizrs-yt-cookies-{}", std::process::id()));
        save(&folder, "x").unwrap();
        save(&folder, FILE_TEXT).unwrap();
        assert_eq!(fs::read_to_string(path(&folder)).unwrap(), FILE_TEXT);
        assert_eq!(fs::metadata(path(&folder)).unwrap().permissions().mode() & 0o777, 0o600);
        remove(&folder).unwrap();
        remove(&folder).unwrap();
        assert!(!path(&folder).exists());
        let _ = fs::remove_dir_all(&folder);
    }
}
