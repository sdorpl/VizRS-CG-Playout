//! Pośrednik panelu zdalnego: serwer strony okna na `127.0.0.1` (losowy port). Każde żądanie strony idzie do panelu
//! VizRS CG Playout z sesją konta (ciasteczko `vizrs_live`) albo z tokenem okna (`Authorization: Bearer …`), więc
//! **ani token, ani identyfikator sesji nie trafiają do strony** (wzór: pośrednik VizRS CG Artist,
//! `local::start_remote`). Sesję konta (K20) zakłada sama strona – ekranem logowania panelu: pośrednik zatrzymuje
//! `Set-Cookie: vizrs_live=…` z odpowiedzi panelu u siebie i dokleja je do kolejnych żądań; zmianę (logowanie,
//! wylogowanie) zgłasza programowi, który zapamiętuje sesję w `live-connections.json`. Dostęp tylko dla okna: sekret w pierwszym adresie → ciasteczko
//! `HttpOnly`, kontrola `Host` (ochrona przed DNS rebinding) i `Origin`. Odpowiedzi idą strumieniem (podgląd HLS,
//! odtwarzanie pliku z `Range`), bez buforowania całości.
//!
//! Drugi port – **pośrednik podglądu VizRS** (`Proxy::output`): ramka `<VizRS>/output` w panelu. VizRS po wejściu na
//! `/output?token=…` autoryzuje skrypty i `/events` ciasteczkiem `vizrs_token` (`SameSite=Strict`); strona okna ma
//! adres `127.0.0.1`, więc ramka z adresu VizRS byłaby obcą witryną i WebKit nie wysłałby ciasteczka (pusta ramka).
//! Przez ten port ramka jest tą samą witryną co panel (`127.0.0.1`, port nie zmienia witryny). Adres VizRS pośrednik
//! bierze z `/api/status` panelu (`vizrsUrl`); niczego od siebie nie dokleja – uprawnienia daje token wyjścia strony.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ureq::http;

/// Największa treść żądania strony (wgrywanie obrazów: panel przyjmuje do 10 MB).
const MAX_BODY: usize = 64 * 1024 * 1024;
const MAX_HEAD: usize = 64 * 1024;
const COOKIE: &str = "vizrs_desktop";
/// Ciasteczko sesji konta panelu (`panel/server`, `COOKIE`).
const SESSION: &str = "vizrs_live";
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
    /// `http://127.0.0.1:<port>` pośrednika podglądu VizRS (`/output`)
    pub output: String,
    pub secret: String,
    stop: Arc<AtomicBool>,
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // obudź pętle accept
        let _ = TcpStream::connect(self.base.trim_start_matches("http://"));
        let _ = TcpStream::connect(self.output.trim_start_matches("http://"));
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

/// `GET` z treścią JSON (do 1 MB): kod i obiekt (`None` – treść nie jest obiektem JSON).
fn get_json(
    target: &str,
    path: &str,
    auth: Auth<'_>,
) -> Result<(u16, Option<serde_json::Value>), String> {
    let b = http::Request::builder()
        .method("GET")
        .uri(format!("{target}{path}"))
        .header("Accept", "application/json");
    let req = auth
        .apply(b)
        .body(())
        .map_err(|e| format!("{target}: {e}"))?;
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
    let v = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .filter(|v| v.is_object());
    Ok((status, v))
}

/// Czym pośrednik loguje się do panelu: sesja konta ma pierwszeństwo (token awaryjny wygrałby w panelu i strona
/// nie pokazałaby konta), bez niej – token.
#[derive(Clone, Copy)]
struct Auth<'a> {
    token: Option<&'a str>,
    session: Option<&'a str>,
}

impl Auth<'_> {
    fn apply(self, b: http::request::Builder) -> http::request::Builder {
        match (self.session, self.token) {
            (Some(s), _) => b.header("Cookie", format!("{SESSION}={s}")),
            (None, Some(t)) => b.header("Authorization", format!("Bearer {t}")),
            (None, None) => b,
        }
    }
}

/// Przed przełączeniem okna: `GET /api/status` z sesją albo tokenem – dostępność, token, czy to panel VizRS CG
/// Playout. Bez tokenu i bez ważnej sesji wystarczą konta w panelu (`GET /api/auth`): strona pokaże logowanie
/// kontem albo zakładanie pierwszego konta.
pub fn check(url: &str, token: Option<&str>, session: Option<&str>) -> Result<(), String> {
    let target = clean_target(url)?;
    let (status, v) = get_json(&target, "/api/status", Auth { token, session })?;
    match status {
        200 if v.is_some() => Ok(()),
        200 => Err(format!(
            "{target} odpowiada, ale to nie jest panel VizRS CG Playout"
        )),
        401 if token.is_some() && session.is_none() => Err("Panel odrzucił token.".into()),
        401 => {
            let auth = get_json(
                &target,
                "/api/auth",
                Auth {
                    token: None,
                    session: None,
                },
            )
            .ok()
            .and_then(|(_, v)| v)
            .map(|v| v["result"].clone())
            .unwrap_or_default();
            if auth["accounts"].as_bool() == Some(true) || auth["setup"].as_bool() == Some(true) {
                Ok(())
            } else if token.is_some() {
                Err("Panel odrzucił token.".into())
            } else {
                Err("Panel wymaga tokenu (konta operatorów są w nim wyłączone).".into())
            }
        }
        code => Err(format!("{target}: odpowiedź HTTP {code}")),
    }
}

/// Zgłoszenie zmiany sesji konta (`None` – wylogowanie albo sesja wygasła).
pub type OnSession = Box<dyn Fn(Option<String>) + Send + Sync>;

/// Identyfikator sesji z `Set-Cookie` panelu: `Some(Some(id))` – nowa sesja, `Some(None)` – skasowana,
/// `None` – nagłówek nie dotyczy sesji.
fn session_from_set_cookie(v: &str) -> Option<Option<String>> {
    let mut parts = v.split(';').map(str::trim);
    let (k, val) = parts.next()?.split_once('=')?;
    if k.trim() != SESSION {
        return None;
    }
    let gone = val.is_empty()
        || parts.any(|a| {
            a.split_once('=').is_some_and(|(k, v)| {
                k.trim().eq_ignore_ascii_case("max-age")
                    && v.trim().parse::<i64>().is_ok_and(|n| n <= 0)
            })
        });
    if gone {
        return Some(None);
    }
    // identyfikator panelu: 64 znaki szesnastkowe; inne wartości – nie nasze, pomijamy
    (val.len() <= 256 && val.bytes().all(|b| b.is_ascii_alphanumeric()))
        .then(|| Some(val.to_string()))
}

/// Uruchamia pośrednika do panelu `url` z tokenem `token` i zapamiętaną sesją konta `session`;
/// `on_session` dostaje każdą zmianę sesji (logowanie i wylogowanie na stronie).
pub fn start(
    url: &str,
    token: Option<String>,
    session: Option<String>,
    on_session: Option<OnSession>,
) -> Result<Proxy, String> {
    let target = clean_target(url)?;
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("pośrednik: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("pośrednik: {e}"))?
        .port();
    let base = format!("http://127.0.0.1:{port}");
    let out_listener =
        TcpListener::bind("127.0.0.1:0").map_err(|e| format!("pośrednik podglądu: {e}"))?;
    let out_port = out_listener
        .local_addr()
        .map_err(|e| format!("pośrednik podglądu: {e}"))?
        .port();
    let output = format!("http://127.0.0.1:{out_port}");
    let secret = secret()?;
    let stop = Arc::new(AtomicBool::new(false));
    let ctx = Arc::new(Ctx {
        target,
        token,
        session: Mutex::new(session),
        on_session,
        secret: secret.clone(),
        host: format!("127.0.0.1:{port}"),
        out_host: format!("127.0.0.1:{out_port}"),
        vizrs: Mutex::new(None),
        agent: agent(None),
    });
    serve(listener, stop.clone(), ctx.clone(), handle);
    serve(out_listener, stop.clone(), ctx, handle_output);
    Ok(Proxy {
        base,
        output,
        secret,
        stop,
    })
}

fn serve(
    listener: TcpListener,
    stop: Arc<AtomicBool>,
    ctx: Arc<Ctx>,
    handler: fn(&Ctx, TcpStream) -> Result<(), String>,
) {
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            let Ok(s) = conn else { continue };
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                if let Err(e) = handler(&ctx, s) {
                    eprintln!("pośrednik: {e}");
                }
            });
        }
    });
}

struct Ctx {
    target: String,
    token: Option<String>,
    session: Mutex<Option<String>>,
    on_session: Option<OnSession>,
    secret: String,
    host: String,
    /// `127.0.0.1:<port>` pośrednika podglądu VizRS
    out_host: String,
    /// adres VizRS z `/api/status` panelu (`None` – jeszcze nieznany albo panel bez VizRS)
    vizrs: Mutex<Option<String>>,
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
        return reply(&mut s, 403, &[], "tylko okno VizRS CG Playout").map_err(|e| e.to_string());
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
    let session = ctx
        .session
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    b = Auth {
        token: ctx.token.as_deref(),
        session: session.as_deref(),
    }
    .apply(b);
    let r = b.body(req.body.clone()).map_err(|e| e.to_string())?;
    let res = match ctx.agent.run(r) {
        Ok(res) => res,
        Err(e) => {
            return reply(s, 502, &[], &format!("panel nie odpowiada: {e}"))
                .map_err(|e| e.to_string())
        }
    };
    let status = res.status().as_u16();
    // sesja konta zostaje w pośredniku (strona nie dostaje Set-Cookie)
    for v in res.headers().get_all("set-cookie") {
        if let Some(new) = v.to_str().ok().and_then(session_from_set_cookie) {
            set_session(ctx, new);
        }
    }
    // sesja wygasła albo skasowana w panelu (np. zmiana hasła) – strona pokaże logowanie; bez sesji token
    if status == 401 && session.is_some() {
        set_session(ctx, None);
    }
    let mut head = format!(
        "HTTP/1.1 {status} {}\r\nConnection: close\r\n",
        reason(status)
    );
    let output = format!("http://{}", ctx.out_host);
    for name in FORWARD_RES {
        for v in res.headers().get_all(*name) {
            if let Ok(v) = v.to_str() {
                // ramka podglądu VizRS idzie przez pośrednika podglądu – jego adres w frame-src
                let v = if *name == "content-security-policy" {
                    add_frame_src(v, &output)
                } else {
                    v.to_string()
                };
                head.push_str(&format!("{name}: {v}\r\n"));
            }
        }
    }
    // adres VizRS dla pośrednika podglądu: z odpowiedzi /api/status (mała, JSON); strona dostaje w niej
    // adres pośrednika podglądu (`vizrsOutputProxy`, core.js `vizrsUrl`)
    if status == 200 && req.method == "GET" && req.target.split('?').next() == Some("/api/status") {
        let body = res
            .into_body()
            .with_config()
            .limit(1024 * 1024)
            .read_to_vec()
            .map_err(|e| e.to_string())?;
        let body = status_with_output(ctx, &body, &output).unwrap_or(body);
        let head = head
            .lines()
            .filter(|l| !l.to_ascii_lowercase().starts_with("content-length:"))
            .map(|l| format!("{l}\r\n"))
            .collect::<String>();
        s.write_all(format!("{head}content-length: {}\r\n\r\n", body.len()).as_bytes())
            .map_err(|e| e.to_string())?;
        return s.write_all(&body).map_err(|e| e.to_string());
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

/// Nagłówki żądania ramki podglądu przekazywane do VizRS (ciasteczka – tylko `vizrs_token`).
const OUTPUT_REQ: &[&str] = &[
    "accept",
    "content-type",
    "range",
    "if-none-match",
    "if-modified-since",
    "accept-language",
    "last-event-id",
];
/// Ciasteczko tokenu VizRS (`VizRS CG System`, `guard.rs`).
const VIZRS_COOKIE: &str = "vizrs_token";

/// `/api/status` panelu: zapamiętuje adres VizRS (`result.vizrsUrl`) i dopisuje `result.vizrsOutputProxy`
/// (tylko gdy VizRS jest znany); `None` – treść nie jest takim obiektem JSON.
fn status_with_output(ctx: &Ctx, body: &[u8], output: &str) -> Option<Vec<u8>> {
    let mut v = serde_json::from_slice::<serde_json::Value>(body).ok()?;
    let target = v["result"]["vizrsUrl"]
        .as_str()
        .and_then(|u| vizrs_target(&ctx.target, u));
    *ctx.vizrs.lock().unwrap_or_else(|e| e.into_inner()) = target.clone();
    let r = v.get_mut("result")?.as_object_mut()?;
    if target.is_some() {
        r.insert("vizrsOutputProxy".into(), output.into());
    }
    serde_json::to_vec(&v).ok()
}

/// Adres VizRS dla pośrednika: `http(s)://host[:port]` z `vizrsUrl` panelu; sam port (`:8788`) – host panelu.
pub(crate) fn vizrs_target(panel: &str, vizrs: &str) -> Option<String> {
    let v = vizrs.trim();
    if let Some(port) = v.strip_prefix(':') {
        port.parse::<u16>().ok().filter(|p| *p > 0)?;
        let (scheme, rest) = panel.split_once("://")?;
        let host = rest.split('/').next()?;
        // host bez portu (IPv6 w nawiasach zostaje w całości)
        let host = match host.rfind(':') {
            Some(i) if !host[i..].contains(']') => &host[..i],
            _ => host,
        };
        return Some(format!("{scheme}://{host}:{port}"));
    }
    let (scheme, rest) = v.split_once("://")?;
    if !matches!(scheme, "http" | "https") {
        return None;
    }
    let host = rest.split(['/', '?', '#']).next()?;
    (!host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-.:[]".contains(c)))
    .then(|| format!("{scheme}://{host}"))
}

/// CSP panelu z dopisanym `origin` w `frame-src` (`'none'` zastąpione).
pub(crate) fn add_frame_src(csp: &str, origin: &str) -> String {
    let mut found = false;
    let parts: Vec<String> = csp
        .split(';')
        .map(|d| {
            let t = d.trim();
            match t.strip_prefix("frame-src") {
                Some(rest) if rest.is_empty() || rest.starts_with(' ') => {
                    found = true;
                    let rest = rest.trim();
                    if rest.is_empty() || rest == "'none'" {
                        format!("frame-src {origin}")
                    } else {
                        format!("frame-src {rest} {origin}")
                    }
                }
                _ => t.to_string(),
            }
        })
        .filter(|d| !d.is_empty())
        .collect();
    let mut out = parts.join("; ");
    if !found {
        out.push_str(&format!("; frame-src {origin}"));
    }
    out
}

/// `Cookie` strony → tylko `vizrs_token` (ciasteczko okna i inne zostają w pośredniku).
fn output_cookie(header: Option<&str>) -> Option<String> {
    header?
        .split(';')
        .map(str::trim)
        .find(|c| c.split_once('=').is_some_and(|(k, _)| k == VIZRS_COOKIE))
        .map(str::to_string)
}

/// `Set-Cookie` VizRS → tylko `vizrs_token`, bez `Domain` i `Secure` (pośrednik jest na `http://127.0.0.1`).
fn output_set_cookie(v: &str) -> Option<String> {
    let mut parts = v.split(';').map(str::trim);
    let first = parts.next()?;
    if first.split_once('=')?.0 != VIZRS_COOKIE {
        return None;
    }
    let mut out = vec![first.to_string()];
    for a in parts {
        let k = a
            .split('=')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if !matches!(k.as_str(), "domain" | "secure" | "") {
            out.push(a.to_string());
        }
    }
    Some(out.join("; "))
}

/// Pośrednik podglądu VizRS: tylko ramka strony okna (ciasteczko okna – wspólne dla `127.0.0.1`), tylko odczyt
/// i zgłoszenia błędów scen (`GET`, `HEAD`, `POST`).
fn handle_output(ctx: &Ctx, mut s: TcpStream) -> Result<(), String> {
    let req = match read_request(&mut s) {
        Ok(r) => r,
        Err((code, msg)) => return reply(&mut s, code, &[], msg).map_err(|e| e.to_string()),
    };
    if req.header("host") != Some(ctx.out_host.as_str()) {
        return reply(&mut s, 403, &[], "zły Host").map_err(|e| e.to_string());
    }
    if let Some(o) = req.header("origin") {
        if o != format!("http://{}", ctx.out_host) && o != format!("http://{}", ctx.host) {
            return reply(&mut s, 403, &[], "obca strona").map_err(|e| e.to_string());
        }
    }
    if !req.cookie(COOKIE).is_some_and(|c| eq_ct(c, &ctx.secret)) {
        return reply(&mut s, 403, &[], "tylko okno VizRS CG Playout").map_err(|e| e.to_string());
    }
    if !matches!(req.method.as_str(), "GET" | "HEAD" | "POST") {
        return reply(&mut s, 405, &[], "podgląd VizRS – tylko odczyt").map_err(|e| e.to_string());
    }
    let Some(target) = ctx.vizrs.lock().unwrap_or_else(|e| e.into_inner()).clone() else {
        return reply(&mut s, 503, &[], "panel nie podał adresu VizRS").map_err(|e| e.to_string());
    };
    let mut b = http::Request::builder()
        .method(req.method.as_str())
        .uri(format!("{target}{}", req.target));
    for name in OUTPUT_REQ {
        if let Some(v) = req.header(name) {
            b = b.header(*name, v);
        }
    }
    if let Some(c) = output_cookie(req.header("cookie")) {
        b = b.header("Cookie", c);
    }
    let r = b.body(req.body.clone()).map_err(|e| e.to_string())?;
    let res = match ctx.agent.run(r) {
        Ok(res) => res,
        Err(e) => {
            return reply(&mut s, 502, &[], &format!("VizRS nie odpowiada: {e}"))
                .map_err(|e| e.to_string())
        }
    };
    let status = res.status().as_u16();
    let mut head = format!(
        "HTTP/1.1 {status} {}\r\nConnection: close\r\n",
        reason(status)
    );
    for name in FORWARD_RES {
        // ramka w oknie: X-Frame-Options / frame-ancestors VizRS liczą się względem adresu VizRS, nie pośrednika
        if *name == "x-frame-options" {
            continue;
        }
        for v in res.headers().get_all(*name) {
            if let Ok(v) = v.to_str() {
                head.push_str(&format!("{name}: {v}\r\n"));
            }
        }
    }
    for v in res.headers().get_all("set-cookie") {
        if let Some(c) = v.to_str().ok().and_then(output_set_cookie) {
            head.push_str(&format!("set-cookie: {c}\r\n"));
        }
    }
    head.push_str("\r\n");
    s.write_all(head.as_bytes()).map_err(|e| e.to_string())?;
    if req.method == "HEAD" {
        return Ok(());
    }
    let mut body = res.into_body().into_reader();
    // strumień zdarzeń /events – kopiowanie porcjami do zamknięcia
    std::io::copy(&mut body, &mut s).map(|_| ()).or_else(|e| {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            Ok(())
        } else {
            Err(e.to_string())
        }
    })
}

fn set_session(ctx: &Ctx, new: Option<String>) {
    let changed = {
        let mut g = ctx.session.lock().unwrap_or_else(|e| e.into_inner());
        let changed = *g != new;
        *g = new.clone();
        changed
    };
    if changed {
        if let Some(f) = &ctx.on_session {
            f(new);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vizrs_target_from_status() {
        assert_eq!(
            vizrs_target("https://emisja.btb.j00r.us", "https://s1.btb.j00r.us/").as_deref(),
            Some("https://s1.btb.j00r.us")
        );
        assert_eq!(
            vizrs_target("http://tv.lan:8090", ":8788").as_deref(),
            Some("http://tv.lan:8788")
        );
        assert_eq!(
            vizrs_target("http://[::1]:8090/p", ":8788").as_deref(),
            Some("http://[::1]:8788")
        );
        assert_eq!(vizrs_target("https://e.pl", "").as_deref(), None);
        assert_eq!(vizrs_target("https://e.pl", "ftp://x").as_deref(), None);
        assert_eq!(vizrs_target("https://e.pl", "http://a b").as_deref(), None);
        assert_eq!(vizrs_target("https://e.pl", ":0").as_deref(), None);
    }

    #[test]
    fn csp_frame_src() {
        let o = "http://127.0.0.1:5";
        assert_eq!(
            add_frame_src("default-src 'self'; frame-src 'none'; img-src 'self'", o),
            "default-src 'self'; frame-src http://127.0.0.1:5; img-src 'self'"
        );
        assert_eq!(
            add_frame_src("frame-src https://s1.pl; script-src 'self'", o),
            "frame-src https://s1.pl http://127.0.0.1:5; script-src 'self'"
        );
        assert_eq!(
            add_frame_src("default-src 'self'", o),
            "default-src 'self'; frame-src http://127.0.0.1:5"
        );
    }

    #[test]
    fn output_cookies() {
        assert_eq!(
            output_cookie(Some("vizrs_desktop=s; vizrs_token=t1; x=1")).as_deref(),
            Some("vizrs_token=t1")
        );
        assert_eq!(output_cookie(Some("vizrs_desktop=s")), None);
        assert_eq!(output_set_cookie("vizrs_token=t1; Path=/; SameSite=Strict; HttpOnly; Secure; Domain=s1.pl; Max-Age=9").as_deref(), Some("vizrs_token=t1; Path=/; SameSite=Strict; HttpOnly; Max-Age=9"));
        assert_eq!(output_set_cookie("vizrs_session=a; Path=/"), None);
    }

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
        let p = start(
            &format!("http://{paddr}"),
            Some("tok-123".into()),
            None,
            None,
        )
        .unwrap();
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

    #[test]
    fn session_cookie_parsing() {
        let id = "ab".repeat(32);
        assert_eq!(
            session_from_set_cookie(&format!(
                "vizrs_live={id}; HttpOnly; SameSite=Strict; Path=/; Max-Age=43200"
            )),
            Some(Some(id))
        );
        assert_eq!(
            session_from_set_cookie("vizrs_live=; HttpOnly; Path=/; Max-Age=0"),
            Some(None)
        );
        assert_eq!(
            session_from_set_cookie("vizrs_live=x1; max-age=0"),
            Some(None)
        );
        assert_eq!(session_from_set_cookie("inne=1; Path=/"), None);
        assert_eq!(
            session_from_set_cookie("vizrs_live=a;b"),
            Some(Some("a".into()))
        );
        assert_eq!(session_from_set_cookie("vizrs_live=a\"b"), None);
    }

    /// K20: logowanie kontem na stronie – pośrednik trzyma sesję u siebie (strona nie widzi ciasteczka),
    /// dokleja ją zamiast tokenu, zgłasza zmiany, a wylogowanie i 401 ją kasują.
    #[test]
    fn proxy_keeps_account_session() {
        let id = "c0".repeat(32);
        let panel = TcpListener::bind("127.0.0.1:0").unwrap();
        let paddr = panel.local_addr().unwrap();
        let pid = id.clone();
        std::thread::spawn(move || {
            for c in panel.incoming().flatten() {
                let mut c = c;
                let mut buf = [0u8; 8192];
                let n = c.read(&mut buf).unwrap_or(0);
                let text = String::from_utf8_lossy(&buf[..n]).to_string();
                let low = text.to_lowercase();
                let path = text.split(' ').nth(1).unwrap_or("").to_string();
                let (code, cookie) = match path.as_str() {
                    "/api/login" => (
                        "200 OK",
                        format!(
                            "Set-Cookie: vizrs_live={pid}; HttpOnly; Path=/; Max-Age=43200\r\n"
                        ),
                    ),
                    "/api/logout" => (
                        "200 OK",
                        "Set-Cookie: vizrs_live=; HttpOnly; Path=/; Max-Age=0\r\n".into(),
                    ),
                    "/api/expired" => ("401 Unauthorized", String::new()),
                    _ => ("200 OK", String::new()),
                };
                let body = format!(
                    "session={} bearer={}",
                    low.contains(&format!("cookie: vizrs_live={pid}")),
                    low.contains("authorization: bearer")
                );
                let _ = write!(c, "HTTP/1.1 {code}\r\nContent-Type: text/plain\r\n{cookie}Content-Length: {}\r\n\r\n{body}", body.len());
            }
        });
        let seen: Arc<Mutex<Vec<Option<String>>>> = Arc::default();
        let s2 = seen.clone();
        let p = start(
            &format!("http://{paddr}"),
            Some("tok-123".into()),
            None,
            Some(Box::new(move |s| s2.lock().unwrap().push(s))),
        )
        .unwrap();
        let host = p.base.trim_start_matches("http://").to_string();
        let ck = format!("Cookie: {COOKIE}={}\r\n", p.secret);
        let call = |method: &str, path: &str| {
            let mut c = TcpStream::connect(&host).unwrap();
            write!(
                c,
                "{method} {path} HTTP/1.1\r\nHost: {host}\r\n{ck}Content-Length: 0\r\n\r\n"
            )
            .unwrap();
            let mut out = String::new();
            c.read_to_string(&mut out).unwrap();
            out
        };
        assert!(call("GET", "/api/status").contains("session=false bearer=true"));
        let l = call("POST", "/api/login");
        assert!(
            !l.to_lowercase().contains("set-cookie"),
            "strona nie dostaje sesji: {l}"
        );
        assert!(call("GET", "/api/status").contains("session=true bearer=false"));
        call("POST", "/api/logout");
        assert!(call("GET", "/api/status").contains("session=false bearer=true"));
        call("POST", "/api/login");
        assert!(call("GET", "/api/expired").starts_with("HTTP/1.1 401"));
        assert!(call("GET", "/api/status").contains("session=false bearer=true"));
        assert_eq!(
            *seen.lock().unwrap(),
            vec![Some(id.clone()), None, Some(id), None]
        );
    }
}
