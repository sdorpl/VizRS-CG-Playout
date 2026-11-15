//! Pośrednik panelu zdalnego: serwer strony okna na `127.0.0.1` (losowy port). Każde żądanie strony idzie do panelu
//! VizRS Live z tokenem okna (`Authorization: Bearer …`), więc **token nie trafia do strony** (wzór: pośrednik
//! VizRS CG Artist, `local::start_remote`). Dostęp tylko dla okna: sekret w pierwszym adresie → ciasteczko
//! `HttpOnly`, kontrola `Host` (ochrona przed DNS rebinding) i `Origin`. Odpowiedzi idą strumieniem (podgląd HLS,
//! odtwarzanie pliku z `Range`), bez buforowania całości.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use ureq::http;

/// Największa treść żądania strony (wgrywanie obrazów: panel przyjmuje do 10 MB).
const MAX_BODY: usize = 64 * 1024 * 1024;
const MAX_HEAD: usize = 64 * 1024;
const COOKIE: &str = "vizrs_desktop";
/// Nagłówki żądania strony przekazywane do panelu (reszta – w tym ciasteczka – zostaje w oknie).
const FORWARD_REQ: &[&str] = &[
    "accept",
    "content-type",
    "range",
    "if-none-match",
    "if-modified-since",
    "x-vizrs-live",
    "x-operator",
    "accept-language",
];
/// Nagłówki odpowiedzi panelu przekazywane stronie (bez `Set-Cookie`).
const FORWARD_RES: &[&str] = &[
    "content-type",
    "content-length",
    "content-range",
    "accept-ranges",
    "cache-control",
    "etag",
    "last-modified",
    "content-disposition",
    "content-security-policy",
    "x-content-type-options",
    "x-frame-options",
    "referrer-policy",
];

pub struct Proxy {
    /// `http://127.0.0.1:<port>`
    pub base: String,
    pub secret: String,
    stop: Arc<AtomicBool>,
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // obudź pętlę accept
        let _ = TcpStream::connect(self.base.trim_start_matches("http://"));
    }
}

impl Proxy {
    /// Adres pierwszej strony okna (sekret → ciasteczko, potem przekierowanie na `/`).
    pub fn start_url(&self) -> String {
        format!("{}/__vizrs_desktop?k={}", self.base, self.secret)
    }
}

/// Losowy sekret z generatora systemu.
pub fn secret() -> Result<String, String> {
    let mut b = [0u8; 24];
    getrandom::fill(&mut b).map_err(|e| format!("generator losowy: {e}"))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// Adres panelu bez końcowego `/` (`https://live.example.pl`, opcjonalnie z przedrostkiem ścieżki).
fn clean_target(url: &str) -> Result<String, String> {
    let u = url.trim().trim_end_matches('/');
    let rest = u
        .strip_prefix("https://")
        .or_else(|| u.strip_prefix("http://"))
        .ok_or(format!(
            "adres musi zaczynać się od http:// albo https:// („{u}”)"
        ))?;
    if rest.is_empty()
        || rest
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || "@?#".contains(c))
    {
        return Err(format!("niepoprawny adres panelu „{u}”"));
    }
    Ok(u.to_string())
}

fn agent(timeout: Option<Duration>) -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(0)
        .timeout_global(timeout)
        .timeout_connect(Some(Duration::from_secs(10)))
        .user_agent(format!("vizrs-live-desktop/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// Przed przełączeniem okna: `GET /api/status` z tokenem – dostępność, token, czy to panel VizRS Live.
pub fn check(url: &str, token: Option<&str>) -> Result<(), String> {
    let target = clean_target(url)?;
    let mut b = http::Request::builder()
        .method("GET")
        .uri(format!("{target}/api/status"))
        .header("Accept", "application/json");
    if let Some(t) = token {
        b = b.header("Authorization", format!("Bearer {t}"));
    }
    let req = b.body(()).map_err(|e| format!("{target}: {e}"))?;
    let mut res = agent(Some(Duration::from_secs(8)))
        .run(req)
        .map_err(|e| format!("Brak połączenia z {target}: {e}"))?;
    let status = res.status().as_u16();
    let body = res
        .body_mut()
        .with_config()
        .limit(1024 * 1024)
        .read_to_vec()
        .unwrap_or_default();
    match status {
        200 if serde_json::from_slice::<serde_json::Value>(&body).is_ok_and(|v| v.is_object()) => {
            Ok(())
        }
        200 => Err(format!(
            "{target} odpowiada, ale to nie jest panel VizRS Live"
        )),
        401 => Err(if token.is_some() {
            "Panel odrzucił token.".into()
        } else {
            "Panel wymaga tokenu.".into()
        }),
        code => Err(format!("{target}: odpowiedź HTTP {code}")),
    }
}

/// Uruchamia pośrednika do panelu `url` z tokenem `token`.
pub fn start(url: &str, token: Option<String>) -> Result<Proxy, String> {
    let target = clean_target(url)?;
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("pośrednik: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("pośrednik: {e}"))?
        .port();
    let base = format!("http://127.0.0.1:{port}");
    let secret = secret()?;
    let stop = Arc::new(AtomicBool::new(false));
    let ctx = Arc::new(Ctx {
        target,
        token,
        secret: secret.clone(),
        host: format!("127.0.0.1:{port}"),
        agent: agent(None),
    });
    let st = stop.clone();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            if st.load(Ordering::Relaxed) {
                break;
            }
            let Ok(s) = conn else { continue };
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                if let Err(e) = handle(&ctx, s) {
                    eprintln!("pośrednik: {e}");
                }
            });
        }
    });
    Ok(Proxy { base, secret, stop })
}

struct Ctx {
    target: String,
    token: Option<String>,
    secret: String,
    host: String,
    agent: ureq::Agent,
}

struct Req {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Req {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
    fn cookie(&self, name: &str) -> Option<&str> {
        self.header("cookie")?
            .split(';')
            .filter_map(|c| c.trim().split_once('='))
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v)
    }
}

/// Czyta żądanie HTTP/1.1 z limitami (nagłówki 64 KB, treść `MAX_BODY`, tylko `Content-Length`).
fn read_request(s: &mut TcpStream) -> Result<Req, (u16, &'static str)> {
    s.set_read_timeout(Some(Duration::from_secs(30))).ok();
    let mut r = BufReader::new(s.try_clone().map_err(|_| (500, "gniazdo"))?);
    let mut head = Vec::new();
    loop {
        let mut line = Vec::new();
        let n = r
            .by_ref()
            .take((MAX_HEAD - head.len().min(MAX_HEAD)) as u64 + 1)
            .read_until(b'\n', &mut line)
            .map_err(|_| (400, "zły nagłówek"))?;
        if n == 0 {
            return Err((400, "przerwane żądanie"));
        }
        head.extend_from_slice(&line);
        if head.len() > MAX_HEAD {
            return Err((431, "za duże nagłówki"));
        }
        if line == b"\r\n" || line == b"\n" {
            break;
        }
    }
    let text = String::from_utf8(head).map_err(|_| (400, "nagłówki nie w UTF-8"))?;
    let mut lines = text.lines();
    let mut first = lines.next().unwrap_or_default().split(' ');
    let (method, target) = (
        first.next().unwrap_or_default().to_string(),
        first.next().unwrap_or_default().to_string(),
    );
    if !matches!(
        method.as_str(),
        "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE"
    ) || !target.starts_with('/')
    {
        return Err((400, "złe żądanie"));
    }
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    if headers.iter().any(|(k, _)| k == "transfer-encoding") {
        return Err((411, "potrzebne Content-Length"));
    }
    let len: usize = headers
        .iter()
        .find(|(k, _)| k == "content-length")
        .map(|(_, v)| v.parse().map_err(|_| (400, "złe Content-Length")))
        .transpose()?
        .unwrap_or(0);
    if len > MAX_BODY {
        return Err((413, "za duża treść"));
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body)
        .map_err(|_| (400, "przerwana treść"))?;
    Ok(Req {
        method,
        target,
        headers,
        body,
    })
}

fn reply(s: &mut TcpStream, code: u16, extra: &[(&str, &str)], body: &str) -> std::io::Result<()> {
    let mut head = format!("HTTP/1.1 {code} {}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n", reason(code), body.len());
    for (k, v) in extra {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    s.write_all(head.as_bytes())?;
    s.write_all(body.as_bytes())
}

fn reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        204 => "No Content",
        206 => "Partial Content",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        411 => "Length Required",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        502 => "Bad Gateway",
        _ => "Status",
    }
}

/// Stała po czasie porównania sekretu.
fn eq_ct(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

fn handle(ctx: &Ctx, mut s: TcpStream) -> Result<(), String> {
    let req = match read_request(&mut s) {
        Ok(r) => r,
        Err((code, msg)) => return reply(&mut s, code, &[], msg).map_err(|e| e.to_string()),
    };
    // tylko adres pośrednika (inna nazwa hosta = obca strona przez DNS rebinding)
    if req.header("host") != Some(ctx.host.as_str()) {
        return reply(&mut s, 403, &[], "zły Host").map_err(|e| e.to_string());
    }
    if let Some(o) = req.header("origin") {
        if o != format!("http://{}", ctx.host) && o != "null" {
            return reply(&mut s, 403, &[], "obca strona").map_err(|e| e.to_string());
        }
    }
    // pierwsza strona okna: sekret z adresu → ciasteczko tylko dla tego pośrednika
    if let Some(q) = req.target.strip_prefix("/__vizrs_desktop?k=") {
        if eq_ct(q, &ctx.secret) {
            let cookie = format!("{COOKIE}={}; Path=/; HttpOnly; SameSite=Strict", ctx.secret);
            return reply(
                &mut s,
                302,
                &[("Location", "/"), ("Set-Cookie", &cookie)],
                "",
            )
            .map_err(|e| e.to_string());
        }
        return reply(&mut s, 403, &[], "zły klucz").map_err(|e| e.to_string());
    }
    if !req.cookie(COOKIE).is_some_and(|c| eq_ct(c, &ctx.secret)) {
        return reply(&mut s, 403, &[], "tylko okno VizRS Live").map_err(|e| e.to_string());
    }
    forward(ctx, &req, &mut s)
}

fn forward(ctx: &Ctx, req: &Req, s: &mut TcpStream) -> Result<(), String> {
    let mut b = http::Request::builder()
        .method(req.method.as_str())
        .uri(format!("{}{}", ctx.target, req.target));
    for name in FORWARD_REQ {
        if let Some(v) = req.header(name) {
            b = b.header(*name, v);
        }
    }
    if let Some(t) = &ctx.token {
        b = b.header("Authorization", format!("Bearer {t}"));
    }
    let r = b.body(req.body.clone()).map_err(|e| e.to_string())?;
    let res = match ctx.agent.run(r) {
        Ok(res) => res,
        Err(e) => {
            return reply(s, 502, &[], &format!("panel nie odpowiada: {e}"))
                .map_err(|e| e.to_string())
        }
    };
    let status = res.status().as_u16();
    let mut head = format!(
        "HTTP/1.1 {status} {}\r\nConnection: close\r\n",
        reason(status)
    );
    for name in FORWARD_RES {
        for v in res.headers().get_all(*name) {
            if let Ok(v) = v.to_str() {
                head.push_str(&format!("{name}: {v}\r\n"));
            }
        }
    }
    head.push_str("\r\n");
    s.write_all(head.as_bytes()).map_err(|e| e.to_string())?;
    if req.method == "HEAD" {
        return Ok(());
    }
    let mut body = res.into_body().into_reader();
    // strumień: plik wideo, segmenty HLS – kopiowanie porcjami do zamknięcia (Connection: close)
    std::io::copy(&mut body, s).map(|_| ()).or_else(|e| {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            Ok(())
        } else {
            Err(e.to_string())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets() {
        assert_eq!(
            clean_target("https://live.example.pl/").unwrap(),
            "https://live.example.pl"
        );
        assert_eq!(
            clean_target(" http://10.0.0.5:8090 ").unwrap(),
            "http://10.0.0.5:8090"
        );
        assert!(clean_target("live.example.pl").is_err());
        assert!(clean_target("https://user@host").is_err());
        assert!(clean_target("https://").is_err());
    }

    #[test]
    fn secrets_differ_and_compare() {
        let (a, b) = (secret().unwrap(), secret().unwrap());
        assert_eq!(a.len(), 48);
        assert_ne!(a, b);
        assert!(eq_ct(&a, &a.clone()));
        assert!(!eq_ct(&a, &b));
        assert!(!eq_ct("ab", "abc"));
    }

    /// Pośrednik dokleja token, nie przepuszcza ciasteczek strony ani obcych hostów, wymaga sekretu okna.
    #[test]
    fn proxy_adds_token_and_guards() {
        // atrapa panelu: odsyła nagłówki żądania w treści
        let panel = TcpListener::bind("127.0.0.1:0").unwrap();
        let paddr = panel.local_addr().unwrap();
        std::thread::spawn(move || {
            for c in panel.incoming().flatten() {
                let mut c = c;
                let mut buf = [0u8; 8192];
                let n = c.read(&mut buf).unwrap_or(0);
                let text = String::from_utf8_lossy(&buf[..n]).to_lowercase();
                let body = format!(
                    "auth={} cookie={}",
                    text.contains("authorization: bearer tok-123"),
                    text.contains("cookie:")
                );
                let _ = write!(c, "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nSet-Cookie: x=1\r\nContent-Length: {}\r\n\r\n{body}", body.len());
            }
        });
        let p = start(&format!("http://{paddr}"), Some("tok-123".into())).unwrap();
        let host = p.base.trim_start_matches("http://").to_string();
        let get = |path: &str, extra: &str| {
            let mut c = TcpStream::connect(&host).unwrap();
            write!(c, "GET {path} HTTP/1.1\r\nHost: {host}\r\n{extra}\r\n").unwrap();
            let mut out = String::new();
            c.read_to_string(&mut out).unwrap();
            out
        };
        assert!(
            get("/api/status", "").starts_with("HTTP/1.1 403"),
            "bez sekretu okna"
        );
        let first = get(&format!("/__vizrs_desktop?k={}", p.secret), "");
        assert!(
            first.starts_with("HTTP/1.1 302") && first.contains("HttpOnly"),
            "{first}"
        );
        let ok = get(
            "/api/status",
            &format!("Cookie: {COOKIE}={}; other=1\r\n", p.secret),
        );
        assert!(
            ok.starts_with("HTTP/1.1 200")
                && ok.contains("auth=true cookie=false")
                && !ok.to_lowercase().contains("set-cookie"),
            "{ok}"
        );
        let mut c = TcpStream::connect(&host).unwrap();
        write!(
            c,
            "GET / HTTP/1.1\r\nHost: evil.example:80\r\nCookie: {COOKIE}={}\r\n\r\n",
            p.secret
        )
        .unwrap();
        let mut out = String::new();
        c.read_to_string(&mut out).unwrap();
        assert!(out.starts_with("HTTP/1.1 403"), "obcy Host");
    }
}
