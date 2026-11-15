//! Okno „Połącz z panelem”: adres i token panelu VizRS Live (np. na VPS), lista zapisanych połączeń.
//! Wzór: okno połączenia VizRS CG System (`source/shell/src/connect.rs`) – bez wspólnego crate'a.
//!
//! Strona okna jest wbudowana w program (nie pochodzi z panelu), więc tylko ona może wysłać przez most
//! komunikat połączenia – strona panelu nie może przełączyć okna na inny adres.
//! Lista leży w `live-connections.json` w katalogu konfiguracji użytkownika (Linux `~/.config/vizrs`,
//! Windows `%APPDATA%\VizRS`, macOS `~/Library/Application Support/VizRS`); token tylko na życzenie.

use std::path::PathBuf;

use serde_json::{json, Value};

/// Najwięcej zapisanych serwerów.
const MAX_SAVED: usize = 20;

/// Zapisane połączenia.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Saved {
    /// (adres, token, jeśli zapamiętany)
    pub servers: Vec<(String, Option<String>)>,
    /// adres, z którym okno łączy się przy uruchomieniu (bez `--connect`)
    pub auto: Option<String>,
}

/// Plik `live-connections.json`.
pub fn config_path() -> Option<PathBuf> {
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if let Some(p) = env("VIZRS_CONFIG") {
        return Some(p.join("live-connections.json"));
    }
    let dir = if cfg!(windows) {
        env("APPDATA")?.join("VizRS")
    } else if cfg!(target_os = "macos") {
        env("HOME")?.join("Library/Application Support/VizRS")
    } else {
        env("XDG_CONFIG_HOME")
            .or_else(|| env("HOME").map(|h| h.join(".config")))?
            .join("vizrs")
    };
    Some(dir.join("live-connections.json"))
}

impl Saved {
    pub fn parse(text: &str) -> Saved {
        let Ok(v) = serde_json::from_str::<Value>(text) else {
            return Saved::default();
        };
        let servers = v["servers"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|s| {
                        let url = normalize_url(s["url"].as_str()?).ok()?;
                        let token = s["token"]
                            .as_str()
                            .filter(|t| valid_token(t))
                            .map(str::to_string);
                        Some((url, token))
                    })
                    .take(MAX_SAVED)
                    .collect()
            })
            .unwrap_or_default();
        let auto = v["auto"].as_str().and_then(|u| normalize_url(u).ok());
        Saved { servers, auto }
    }

    pub fn to_json(&self) -> String {
        let servers: Vec<Value> = self
            .servers
            .iter()
            .map(|(u, t)| json!({ "url": u, "token": t }))
            .collect();
        serde_json::to_string_pretty(&json!({ "servers": servers, "auto": self.auto }))
            .unwrap_or_default()
    }

    pub fn load() -> Saved {
        config_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|t| Saved::parse(&t))
            .unwrap_or_default()
    }

    /// Zapis z prawami tylko dla właściciela (plik może zawierać token).
    pub fn save(&self) -> Result<(), String> {
        let p = config_path().ok_or("brak katalogu konfiguracji użytkownika")?;
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        let mut opt = std::fs::OpenOptions::new();
        opt.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opt.mode(0o600);
        }
        use std::io::Write;
        opt.open(&p)
            .and_then(|mut f| f.write_all(self.to_json().as_bytes()))
            .map_err(|e| format!("{}: {e}", p.display()))
    }

    /// Serwer na początek listy (ostatnio używany).
    pub fn remember(&mut self, url: &str, token: Option<&str>, keep_token: bool, auto: bool) {
        self.servers.retain(|(u, _)| u != url);
        self.servers.insert(
            0,
            (
                url.to_string(),
                token.filter(|_| keep_token).map(str::to_string),
            ),
        );
        self.servers.truncate(MAX_SAVED);
        if auto {
            self.auto = Some(url.to_string());
        } else if self.auto.as_deref() == Some(url) {
            self.auto = None;
        }
    }

    pub fn forget(&mut self, url: &str) {
        self.servers.retain(|(u, _)| u != url);
        if self.auto.as_deref() == Some(url) {
            self.auto = None;
        }
    }

    pub fn token_for(&self, url: &str) -> Option<String> {
        self.servers
            .iter()
            .find(|(u, _)| u == url)
            .and_then(|(_, t)| t.clone())
    }
}

/// Adres serwera: `http(s)://host[:port][/ścieżka]`, bez końcowego `/`. Sam host → `https://`.
pub fn normalize_url(s: &str) -> Result<String, String> {
    let s = s.trim().trim_end_matches('/');
    if s.is_empty() {
        return Err("podaj adres serwera".into());
    }
    let s = if s.contains("://") {
        s.to_string()
    } else {
        format!("https://{s}")
    };
    let rest = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
        .ok_or(format!(
            "adres musi zaczynać się od http:// albo https:// („{s}”)"
        ))?;
    let host = rest.split('/').next().unwrap_or("");
    let ok_char = |c: char| c.is_ascii_alphanumeric() || "-._:[]/~%".contains(c);
    if host.is_empty() || !rest.chars().all(ok_char) || s.len() > 300 {
        return Err(format!("niepoprawny adres serwera „{s}”"));
    }
    Ok(s)
}

/// Znaki tokenu takie jak w serwerze (`--token`).
pub fn valid_token(t: &str) -> bool {
    !t.is_empty()
        && t.len() <= 256
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
}

/// Żądanie połączenia z serwerem.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub url: String,
    pub token: Option<String>,
    /// zapamiętaj token w `live-connections.json`
    pub keep_token: bool,
    /// łącz przy uruchomieniu
    pub auto: bool,
}

/// Komunikat z okna połączenia.
#[derive(Debug, PartialEq)]
pub enum Msg {
    Connect(Request),
    Forget(String),
    Cancel,
}

pub fn parse_msg(body: &str) -> Result<Msg, String> {
    let v: Value = serde_json::from_str(body).map_err(|_| "zły komunikat".to_string())?;
    match v["action"].as_str() {
        Some("connect") => {
            let url = normalize_url(v["url"].as_str().unwrap_or(""))?;
            let token = v["token"].as_str().map(str::trim).filter(|t| !t.is_empty());
            if let Some(t) = token {
                if !valid_token(t) {
                    return Err(
                        "token może zawierać tylko litery A–Z a–z, cyfry i znaki - _ .".into(),
                    );
                }
            }
            Ok(Msg::Connect(Request {
                url,
                token: token.map(str::to_string),
                keep_token: v["keepToken"].as_bool().unwrap_or(false),
                auto: v["auto"].as_bool().unwrap_or(false),
            }))
        }
        Some("forget") => Ok(Msg::Forget(v["url"].as_str().unwrap_or("").to_string())),
        Some("cancel") => Ok(Msg::Cancel),
        _ => Err("zły komunikat".into()),
    }
}

/// Dane dla strony okna (`window.__vizrsConn`).
pub fn page_data(saved: &Saved, current: &str, current_token: Option<&str>) -> String {
    let servers: Vec<Value> = saved
        .servers
        .iter()
        .map(|(u, t)| json!({ "url": u, "hasToken": t.is_some() }))
        .collect();
    let remote = !current.is_empty();
    let data = json!({
        "current": current,
        "remote": remote,
        "token": if remote { current_token } else { None },
        "auto": saved.auto,
        "servers": servers,
    });
    format!("window.__vizrsConn = {data};")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(
            normalize_url(" https://cg.example.pl/ ").unwrap(),
            "https://cg.example.pl"
        );
        assert_eq!(
            normalize_url("cg.example.pl").unwrap(),
            "https://cg.example.pl"
        );
        assert_eq!(
            normalize_url("http://10.0.0.5:8088").unwrap(),
            "http://10.0.0.5:8088"
        );
        assert!(normalize_url("").is_err());
        assert!(normalize_url("ftp://x").is_err());
        assert!(normalize_url("https://a b").is_err());
        assert!(normalize_url("https://x/'><script>").is_err());
    }

    #[test]
    fn health() {}

    #[test]
    fn messages() {
        let m = parse_msg(r#"{"action":"connect","url":"cg.pl","token":" abc ","keepToken":true}"#)
            .unwrap();
        assert_eq!(
            m,
            Msg::Connect(Request {
                url: "https://cg.pl".into(),
                token: Some("abc".into()),
                keep_token: true,
                auto: false
            })
        );
        assert!(parse_msg(r#"{"action":"connect","url":"cg.pl","token":"a&b"}"#).is_err());
        assert!(parse_msg(r#"{"action":"local"}"#).is_err()); // panelu lokalnego już nie ma
        assert!(parse_msg("probe:{}").is_err());
    }

    #[test]
    fn saved_roundtrip() {
        let mut s = Saved::default();
        s.remember("https://a", Some("t1"), true, true);
        s.remember("https://b", Some("t2"), false, false);
        s.remember("https://a", Some("t3"), true, false);
        assert_eq!(
            s.servers,
            vec![
                ("https://a".into(), Some("t3".into())),
                ("https://b".into(), None)
            ]
        );
        assert_eq!(s.auto, None);
        s.remember("https://b", None, false, true);
        let back = Saved::parse(&s.to_json());
        assert_eq!(back, s);
        s.forget("https://b");
        assert_eq!(s.auto, None);
        assert_eq!(Saved::parse("{zły"), Saved::default());
        assert_eq!(
            Saved::parse(r#"{"servers":[{"url":"https://x","token":"z ł y"}]}"#).servers,
            vec![("https://x".into(), None)]
        );
    }
}
