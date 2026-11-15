//! VizRS Live – panel kanału nad ErsatzTV legacy.
//!
//! Serwer podaje UI panelu i przekazuje komendy do mostu VizRS Bridge (w procesie legacy).
//! Token mostu zostaje po stronie serwera; przeglądarka zna tylko (opcjonalny) token panelu.

mod bridge;
mod http;
mod json;
mod vod;

use std::net::{IpAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use bridge::Bridge;
use http::{json_str, Request, Response};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_CONN: usize = 64;
const COOKIE: &str = "vizrs_live";

const STATIC: &[(&str, &str, &str)] = &[
    (
        "/",
        "text/html; charset=utf-8",
        include_str!("../web/index.html"),
    ),
    (
        "/app.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/app.js"),
    ),
    (
        "/core.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/core.js"),
    ),
    (
        "/form.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/form.js"),
    ),
    (
        "/parts.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/parts.js"),
    ),
    (
        "/when.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/when.js"),
    ),
    (
        "/cmdedit.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/cmdedit.js"),
    ),
    (
        "/clip.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/clip.js"),
    ),
    (
        "/alarms.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/alarms.js"),
    ),
    (
        "/check.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/check.js"),
    ),
    (
        "/tasks.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/tasks.js"),
    ),
    (
        "/player.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/player.js"),
    ),
    (
        "/mux.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/mux.js"),
    ),
    (
        "/views/antena.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/views/antena.js"),
    ),
    (
        "/views/kanaly.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/views/kanaly.js"),
    ),
    (
        "/views/profile.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/views/profile.js"),
    ),
    (
        "/views/znaki.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/views/znaki.js"),
    ),
    (
        "/views/wypelniacze.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/views/wypelniacze.js"),
    ),
    (
        "/views/media.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/views/media.js"),
    ),
    (
        "/views/zrodla.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/views/zrodla.js"),
    ),
    (
        "/views/listy.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/views/listy.js"),
    ),
    (
        "/views/playlisty.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/views/playlisty.js"),
    ),
    (
        "/views/playouty.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/views/playouty.js"),
    ),
    (
        "/views/bloki.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/views/bloki.js"),
    ),
    (
        "/views/deco.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/views/deco.js"),
    ),
    (
        "/views/kalendarz.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/views/kalendarz.js"),
    ),
    (
        "/views/harmonogramy.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/views/harmonogramy.js"),
    ),
    (
        "/views/grafika.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/views/grafika.js"),
    ),
    (
        "/views/live.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/views/live.js"),
    ),
    (
        "/views/asrun.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/views/asrun.js"),
    ),
    (
        "/views/import.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/views/import.js"),
    ),
    (
        "/views/system.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/views/system.js"),
    ),
    (
        "/desktop.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/desktop.js"),
    ),
    (
        "/vizrs-ui.css",
        "text/css; charset=utf-8",
        include_str!("../web/vizrs-ui.css"),
    ),
    (
        "/style.css",
        "text/css; charset=utf-8",
        include_str!("../web/style.css"),
    ),
];

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub bind: String,
    pub port: u16,
    pub bridge_url: String,
    pub bridge_token: String,
    pub token: Option<String>,
    pub stream_url: String,
    pub vizrs_url: String,
    /// token wyjścia VizRS (tylko odczyt, jak w OBS) – trafia do przeglądarki w adresie podglądu /output
    pub vizrs_output_token: String,
    /// VizRS z antenami kanałów (CG Playout `channel_buses`): podgląd `bus=ch-N`, Trio `?channel=N`
    pub vizrs_channels: bool,
    pub cg_url: String,
    pub cg_token: String,
    pub web: Option<PathBuf>,
    /// najwyżej tyle podglądów w mozaice Na antenie (K10; każdy to sesja Next, która koduje); 0 = bez mozaiki
    pub multiview_max: u32,
}

/// Górna granica `--multiview-max` (ochrona maszyny: każdy podgląd to transkodująca sesja Next).
pub const MULTIVIEW_LIMIT: u32 = 16;

const HELP: &str = "vizrs-live – panel kanału nad ErsatzTV legacy

Użycie: vizrs-live [opcje]

  --bind ADRES          adres nasłuchu (VIZRS_LIVE_BIND, domyślnie 127.0.0.1)
  --port N              port panelu (VIZRS_LIVE_PORT, domyślnie 8090)
  --bridge URL          ErsatzTV legacy z mostem (VIZRS_LIVE_BRIDGE, domyślnie http://127.0.0.1:8409)
  --bridge-token T      token mostu, = VIZRS_BRIDGE_TOKEN w legacy (VIZRS_LIVE_BRIDGE_TOKEN, wymagany)
  --token T             token panelu dla operatorów (VIZRS_LIVE_TOKEN; wymagany poza 127.0.0.1)
  --stream-url URL      publiczny adres strumieni legacy do odnośników (VIZRS_LIVE_STREAM_URL)
  --vizrs-url URL       VizRS CG System widziany z przeglądarki: podgląd /output i odnośnik do Trio;
                        sam port (:8788) = ten sam host co panel (VIZRS_LIVE_VIZRS_URL)
  --vizrs-output-token T  token wyjścia VizRS (tylko odczyt) do podglądu /output (VIZRS_LIVE_VIZRS_OUTPUT_TOKEN)
  --vizrs-channels      VizRS z antenami kanałów, jak channel_buses w CG Playout (VIZRS_LIVE_VIZRS_CHANNELS=1)
  --cg-url URL          VizRS CG Playout (sidecar, np. http://cg:7700) – grafika i sloty na żywo (VIZRS_LIVE_CG_URL)
  --cg-token T          token panelu CG Playout, = panel_token / VIZRS_PANEL_TOKEN tam (VIZRS_LIVE_CG_TOKEN)
  --multiview-max N     najwyżej tyle podglądów w mozaice Na antenie, 0–16, 0 = bez mozaiki
                        (VIZRS_LIVE_MULTIVIEW_MAX, domyślnie 4; każdy podgląd to sesja Next, która koduje)
  --web KATALOG         UI z dysku zamiast wbudowanego (praca nad UI)
  --healthcheck         sprawdź działający panel (GET /health), kod wyjścia 0/1
  --version, --help
";

/// Opcje z argumentów i zmiennych środowiskowych (argumenty mają pierwszeństwo).
pub fn parse_config(
    args: &[String],
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<Config, String> {
    let mut c = Config {
        bind: env("VIZRS_LIVE_BIND").unwrap_or_else(|| "127.0.0.1".into()),
        port: match env("VIZRS_LIVE_PORT") {
            Some(p) => p.parse().map_err(|_| format!("zły VIZRS_LIVE_PORT: {p}"))?,
            None => 8090,
        },
        bridge_url: env("VIZRS_LIVE_BRIDGE").unwrap_or_else(|| "http://127.0.0.1:8409".into()),
        bridge_token: env("VIZRS_LIVE_BRIDGE_TOKEN").unwrap_or_default(),
        token: env("VIZRS_LIVE_TOKEN").filter(|t| !t.is_empty()),
        stream_url: env("VIZRS_LIVE_STREAM_URL").unwrap_or_default(),
        vizrs_url: env("VIZRS_LIVE_VIZRS_URL").unwrap_or_default(),
        vizrs_output_token: env("VIZRS_LIVE_VIZRS_OUTPUT_TOKEN").unwrap_or_default(),
        vizrs_channels: matches!(
            env("VIZRS_LIVE_VIZRS_CHANNELS").as_deref(),
            Some("1" | "true" | "yes")
        ),
        cg_url: env("VIZRS_LIVE_CG_URL").unwrap_or_default(),
        cg_token: env("VIZRS_LIVE_CG_TOKEN").unwrap_or_default(),
        web: None,
        multiview_max: match env("VIZRS_LIVE_MULTIVIEW_MAX") {
            Some(v) => v
                .parse()
                .map_err(|_| format!("zły VIZRS_LIVE_MULTIVIEW_MAX: {v}"))?,
            None => 4,
        },
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = || {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{a}: brak wartości"))
        };
        match a.as_str() {
            "--bind" => c.bind = val()?,
            "--port" => {
                c.port = val()?
                    .parse()
                    .map_err(|_| "--port: zła liczba".to_string())?
            }
            "--bridge" => c.bridge_url = val()?,
            "--bridge-token" => c.bridge_token = val()?,
            "--token" => c.token = Some(val()?).filter(|t| !t.is_empty()),
            "--stream-url" => c.stream_url = val()?,
            "--vizrs-url" => c.vizrs_url = val()?,
            "--vizrs-output-token" => c.vizrs_output_token = val()?,
            "--vizrs-channels" => c.vizrs_channels = true,
            "--cg-url" => c.cg_url = val()?,
            "--cg-token" => c.cg_token = val()?,
            "--web" => c.web = Some(PathBuf::from(val()?)),
            "--multiview-max" => {
                c.multiview_max = val()?
                    .parse()
                    .map_err(|_| "--multiview-max: zła liczba".to_string())?
            }
            other => return Err(format!("nieznana opcja: {other} (--help)")),
        }
    }
    if c.multiview_max > MULTIVIEW_LIMIT {
        return Err(format!(
            "--multiview-max: najwyżej {MULTIVIEW_LIMIT} (każdy podgląd mozaiki to sesja Next, która koduje)"
        ));
    }
    bridge::parse_url(&c.bridge_url)?;
    if !c.cg_url.is_empty() {
        bridge::parse_url(&c.cg_url).map_err(|e| format!("--cg-url: {e}"))?;
        if c.cg_token.is_empty() || c.cg_token.contains(['\r', '\n']) {
            return Err(
                "--cg-url wymaga tokenu panelu CG Playout: --cg-token / VIZRS_LIVE_CG_TOKEN".into(),
            );
        }
    }
    if c.bridge_token.len() < 16 {
        return Err("brak tokenu mostu: --bridge-token / VIZRS_LIVE_BRIDGE_TOKEN (min. 16 znaków, ten sam co VIZRS_BRIDGE_TOKEN w legacy)".into());
    }
    if let Some(t) = &c.token {
        if t.len() < 16
            || !t
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || "-._~".contains(ch))
        {
            return Err("token panelu: min. 16 znaków, tylko litery, cyfry i -._~".into());
        }
    }
    let loopback = c
        .bind
        .parse::<IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(c.bind == "localhost");
    if !loopback && c.token.is_none() {
        return Err(format!(
            "panel na {} wymaga tokenu (--token / VIZRS_LIVE_TOKEN)",
            c.bind
        ));
    }
    for u in [&mut c.stream_url, &mut c.vizrs_url, &mut c.cg_url] {
        while u.ends_with('/') {
            u.pop();
        }
    }
    check_vizrs(&c)?;
    Ok(c)
}

/// Adres VizRS trafia do przeglądarki (ramka, odnośnik): tylko `http(s)://…` albo sam port `:N`;
/// token wyjścia – znaki tokenu VizRS (bezpieczne w adresie).
fn check_vizrs(c: &Config) -> Result<(), String> {
    let u = &c.vizrs_url;
    let port_only = u
        .strip_prefix(':')
        .is_some_and(|p| p.parse::<u16>().is_ok_and(|n| n > 0));
    let absolute = (u.starts_with("http://") || u.starts_with("https://"))
        && !u
            .chars()
            .any(|ch| ch.is_whitespace() || "\"'<>?#".contains(ch));
    if !(u.is_empty() || port_only || absolute) {
        return Err(format!(
            "--vizrs-url: oczekiwany http(s)://host:port albo sam port (:8788), jest {u}"
        ));
    }
    let t = &c.vizrs_output_token;
    if t.len() > 128
        || !t
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "-_.".contains(ch))
    {
        return Err("--vizrs-output-token: tylko litery, cyfry i -_. (jak token VizRS)".into());
    }
    if u.is_empty() && (!t.is_empty() || c.vizrs_channels) {
        return Err("--vizrs-output-token i --vizrs-channels wymagają --vizrs-url".into());
    }
    Ok(())
}

struct App {
    cfg: Config,
    bridge: Bridge,
    cg: Option<Bridge>,
    /// nieudane logowania w bieżących oknach: na adres klienta i łącznie
    login_fails: std::sync::Mutex<LoginFails>,
    /// sesje przeglądarek (K1): losowy identyfikator w ciasteczku → ostatnie użycie
    sessions: std::sync::Mutex<Sessions>,
}

/// Sesja wygasa po tylu sekundach bezczynności (30 dni – jak dawne `Max-Age` ciasteczka).
pub const SESSION_IDLE: std::time::Duration = std::time::Duration::from_secs(2_592_000);
/// Najwyżej tyle sesji naraz; przy nadmiarze najpierw wygasłe, potem najdawniej używana.
pub const SESSION_MAX: usize = 256;

/// Magazyn sesji w pamięci (K1). Restart panelu = ponowne logowanie wszystkich przeglądarek.
#[derive(Default)]
pub struct Sessions {
    map: std::collections::HashMap<String, std::time::Instant>,
}

impl Sessions {
    /// Nowa sesja o identyfikatorze `id` (64 znaki szesnastkowe z [`random_id`]).
    pub fn create(&mut self, id: String, now: std::time::Instant) {
        if self.map.len() >= SESSION_MAX {
            self.map
                .retain(|_, t| now.duration_since(*t) < SESSION_IDLE);
        }
        if self.map.len() >= SESSION_MAX {
            if let Some(old) = self
                .map
                .iter()
                .min_by_key(|(_, t)| **t)
                .map(|(k, _)| k.clone())
            {
                self.map.remove(&old);
            }
        }
        self.map.insert(id, now);
    }
    /// `true` = sesja istnieje i nie wygasła; odświeża czas ostatniego użycia.
    pub fn touch(&mut self, id: &str, now: std::time::Instant) -> bool {
        let Some(key) = self.map.keys().find(|k| eq_ct(k, id)).cloned() else {
            return false;
        };
        let t = self.map.get_mut(&key).expect("klucz z mapy");
        if now.duration_since(*t) >= SESSION_IDLE {
            self.map.remove(&key);
            return false;
        }
        *t = now;
        true
    }
    pub fn remove(&mut self, id: &str) {
        if let Some(key) = self.map.keys().find(|k| eq_ct(k, id)).cloned() {
            self.map.remove(&key);
        }
    }
    #[cfg(test)]
    fn len(&self) -> usize {
        self.map.len()
    }
}

/// 32 losowe bajty z `/dev/urandom` jako 64 znaki szesnastkowe (bez crate'ów).
pub fn random_id() -> Result<String, String> {
    use std::io::Read;
    let mut b = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .map_err(|e| format!("/dev/urandom: {e}"))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// Okna nieudanych logowań: `(początek okna, liczba)` na adres IP klienta i łącznie (bezpiecznik).
#[derive(Default)]
struct LoginFails {
    per_ip: std::collections::HashMap<Option<std::net::IpAddr>, (std::time::Instant, u32)>,
    all: Option<(std::time::Instant, u32)>,
}

/// Najwyżej tyle nieudanych logowań z jednego adresu na [`LOGIN_WINDOW`] – potem 429 dla tego adresu do końca okna.
pub const LOGIN_MAX_FAILS: u32 = 10;
/// Bezpiecznik: tyle nieudanych logowań łącznie (z wielu adresów) blokuje logowanie wszystkim do końca okna.
pub const LOGIN_MAX_FAILS_ALL: u32 = 100;
pub const LOGIN_WINDOW: std::time::Duration = std::time::Duration::from_secs(60);
/// Najwyżej tyle adresów w pamięci limitu (po usunięciu wygasłych okien; nadmiar = bezpiecznik łączny).
const LOGIN_MAX_PEERS: usize = 4096;

/// `true` = logowanie z adresu `peer` zablokowane; `failed` = dolicz nieudaną próbę.
fn login_limited(app: &App, peer: Option<std::net::IpAddr>, failed: bool) -> bool {
    let mut g = app.login_fails.lock().unwrap_or_else(|e| e.into_inner());
    let now = std::time::Instant::now();
    let fresh = |w: &(std::time::Instant, u32)| now.duration_since(w.0) < LOGIN_WINDOW;
    if !g.all.as_ref().is_some_and(fresh) {
        g.all = Some((now, 0));
    }
    if failed {
        if let Some(a) = g.all.as_mut() {
            a.1 += 1;
        }
        if !g.per_ip.contains_key(&peer) && g.per_ip.len() >= LOGIN_MAX_PEERS {
            g.per_ip.retain(|_, w| fresh(w));
        }
        if g.per_ip.len() < LOGIN_MAX_PEERS || g.per_ip.contains_key(&peer) {
            let w = g.per_ip.entry(peer).or_insert((now, 0));
            if !fresh(w) {
                *w = (now, 0);
            }
            w.1 += 1;
        }
    }
    let mine = g.per_ip.get(&peer).filter(|w| fresh(w)).map_or(0, |w| w.1);
    mine >= LOGIN_MAX_FAILS || g.all.map_or(0, |a| a.1) >= LOGIN_MAX_FAILS_ALL
}

/// Ciasteczko sesji; `Secure`, gdy panel stoi za proxy TLS (`X-Forwarded-Proto: https`).
fn session_cookie(req: &Request, value: &str, max_age: u32) -> String {
    let secure = if req.header("x-forwarded-proto") == Some("https") {
        "; Secure"
    } else {
        ""
    };
    format!("{COOKIE}={value}; HttpOnly; SameSite=Strict; Path=/; Max-Age={max_age}{secure}")
}

fn cg_client(c: &Config) -> Option<Bridge> {
    (!c.cg_url.is_empty()).then(|| Bridge {
        target: bridge::parse_url(&c.cg_url).expect("sprawdzone w parse_config"),
        token: c.cg_token.clone(),
        prefix: "",
        auth: "Authorization",
    })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{HELP}");
        return;
    }
    if args.iter().any(|a| a == "--version") {
        println!("vizrs-live {VERSION}");
        return;
    }
    if args.iter().any(|a| a == "--healthcheck") {
        std::process::exit(healthcheck());
    }
    let cfg = match parse_config(&args, &|k| std::env::var(k).ok()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("vizrs-live: {e}");
            std::process::exit(2);
        }
    };
    let target = bridge::parse_url(&cfg.bridge_url).expect("sprawdzone w parse_config");
    let app = Arc::new(App {
        bridge: Bridge {
            target,
            token: cfg.bridge_token.clone(),
            prefix: "/vizrs-bridge",
            auth: "X-VizRS-Token",
        },
        cg: cg_client(&cfg),
        login_fails: std::sync::Mutex::new(LoginFails::default()),
        sessions: std::sync::Mutex::new(Sessions::default()),
        cfg,
    });

    http::set_csp(
        &http::csp_origin(&app.cfg.stream_url),
        &http::csp_origin(&app.cfg.vizrs_url),
    );
    let addr = format!("{}:{}", app.cfg.bind, app.cfg.port);
    let listener = match TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("vizrs-live: nie mogę nasłuchiwać na {addr}: {e}");
            std::process::exit(1);
        }
    };
    eprintln!(
        "vizrs-live {VERSION}: panel http://{addr}/ · most {} · token panelu: {}",
        app.cfg.bridge_url,
        if app.cfg.token.is_some() {
            "tak"
        } else {
            "nie (tylko lokalnie)"
        }
    );

    let active = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONN {
            active.fetch_sub(1, Ordering::SeqCst);
            let _ = Response::error(503, "za dużo połączeń").write_to(&stream);
            continue;
        }
        let (app, active) = (Arc::clone(&app), Arc::clone(&active));
        std::thread::spawn(move || {
            handle(&app, stream);
            active.fetch_sub(1, Ordering::SeqCst);
        });
    }
}

/// Dla HEALTHCHECK w obrazie bez powłoki: GET /health na lokalnym porcie panelu.
fn healthcheck() -> i32 {
    let port = std::env::var("VIZRS_LIVE_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(8090);
    let target = bridge::Target {
        host: "127.0.0.1".into(),
        port,
        base: String::new(),
    };
    let probe = (|| -> Result<bool, String> {
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], target.port));
        let mut s = TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(3))
            .map_err(|e| e.to_string())?;
        http::set_timeouts(&s, 3);
        use std::io::{Read, Write};
        s.write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .map_err(|e| e.to_string())?;
        let mut raw = Vec::new();
        s.read_to_end(&mut raw).map_err(|e| e.to_string())?;
        Ok(bridge::parse_reply(&raw)?.status == 200)
    })();
    match probe {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(e) => {
            eprintln!("healthcheck: {e}");
            1
        }
    }
}

fn handle(app: &App, stream: TcpStream) {
    http::set_timeouts(&stream, 15);
    let resp = match http::read_request(&stream) {
        Ok(mut req) => {
            req.peer = stream.peer_addr().ok().map(|a| a.ip());
            route(app, &req)
        }
        Err(http::ParseError::TooLarge) => Response::error(413, "żądanie za duże"),
        Err(http::ParseError::Bad(m)) => Response::error(400, m),
        Err(http::ParseError::Io) => return,
    };
    let _ = resp.write_to(&stream);
}

fn eq_ct(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

fn authorized(app: &App, req: &Request) -> bool {
    let Some(token) = &app.cfg.token else {
        return true;
    };
    // skrypty: token panelu w nagłówku; przeglądarka: identyfikator sesji w ciasteczku (K1)
    if req
        .header("authorization")
        .and_then(|h| h.strip_prefix("Bearer "))
        .is_some_and(|t| eq_ct(t, token))
    {
        return true;
    }
    req.cookie(COOKIE).is_some_and(|id| {
        app.sessions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .touch(id, std::time::Instant::now())
    })
}

/// Zapytanie legacy tylko do odczytu (bezpieczne do ponowienia): `Get…`, `Query…`, `Search…`, `Count…`.
pub fn read_only(name: &str) -> bool {
    ["Get", "Query", "Search", "Count"].iter().any(|p| {
        name.strip_prefix(p)
            .is_some_and(|r| r.starts_with(|c: char| c.is_ascii_uppercase()))
    })
}

/// Początek odpowiedzi do dziennika (jedna linia, najwyżej 300 znaków).
fn log_snippet(body: &[u8]) -> String {
    String::from_utf8_lossy(body)
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(300)
        .collect()
}

/// Nazwa komendy legacy: litera, potem litery i cyfry (np. `GetAllChannels`).
pub fn valid_command(name: &str) -> bool {
    (1..=80).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name.chars().all(|c| c.is_ascii_alphanumeric())
}

fn route(app: &App, req: &Request) -> Response {
    let path = req.path.as_str();
    if path == "/health" {
        return Response::json(200, "{\"ok\":true}");
    }
    if !path.starts_with("/api/") {
        return static_file(app, req);
    }
    if req.method != "GET" && req.method != "HEAD" && req.header("x-vizrs-live") != Some("1") {
        return Response::error(
            403,
            "brak nagłówka X-VizRS-Live (ochrona przed obcymi stronami)",
        );
    }
    match (req.method.as_str(), path) {
        ("POST", "/api/login") => {
            let given = String::from_utf8_lossy(&req.body).trim().to_string();
            if app.cfg.token.is_some() && login_limited(app, req.peer, false) {
                return Response::error(429, "za dużo nieudanych logowań – odczekaj minutę");
            }
            match &app.cfg.token {
                Some(t) if eq_ct(&given, t) => {
                    let id = match random_id() {
                        Ok(id) => id,
                        Err(e) => {
                            eprintln!("login: {e}");
                            return Response::error(500, "brak źródła losowości dla sesji");
                        }
                    };
                    app.sessions
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .create(id.clone(), std::time::Instant::now());
                    Response::json(200, "{\"ok\":true}").with_header(
                        "Set-Cookie",
                        session_cookie(req, &id, SESSION_IDLE.as_secs() as u32),
                    )
                }
                Some(_) => {
                    login_limited(app, req.peer, true);
                    match req.peer {
                        Some(ip) => eprintln!("login: zły token ({ip})"),
                        None => eprintln!("login: zły token"),
                    }
                    std::thread::sleep(std::time::Duration::from_millis(400));
                    Response::error(401, "zły token panelu")
                }
                None => Response::json(200, "{\"ok\":true}"),
            }
        }
        ("POST", "/api/logout") => {
            if let Some(id) = req.cookie(COOKIE) {
                app.sessions
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(id);
            }
            Response::json(200, "{\"ok\":true}")
                .with_header("Set-Cookie", session_cookie(req, "", 0))
        }
        _ if !authorized(app, req) => Response::error(401, "zaloguj się tokenem panelu"),
        ("GET", "/api/status") => status(app),
        ("GET", "/api/exposure") => exposure(app),
        ("GET", p) if p.starts_with("/api/cg/live/thumbs/") => {
            cg_thumb(app, &p["/api/cg/live/thumbs/".len()..])
        }
        (m, p) if p.starts_with("/api/cg/") => cg_proxy(
            app,
            m,
            &p["/api/cg/".len()..],
            &req.body,
            &operator_name(req.header("x-operator")),
        ),
        ("GET", p) if p.starts_with("/api/stream/") => {
            stream_proxy(app, &p["/api/stream/".len()..])
        }
        ("POST", p) if p.starts_with("/api/play/") => play(app, &p["/api/play/".len()..]),
        ("POST", "/api/vod/open") => vod::open(&app.bridge, &req.body),
        ("GET", p) if p.starts_with("/api/vod/") => {
            vod::serve(&app.bridge, &p["/api/vod/".len()..], req)
        }
        ("GET", p) if p.starts_with("/api/artwork/") => artwork(app, &p["/api/artwork/".len()..]),
        ("GET", "/api/commands") => forward(app.bridge.get("/commands"), "commands"),
        ("GET", "/api/sessions") => forward(app.bridge.get("/sessions"), "sessions"),
        ("GET", "/api/framerates") => forward(app.bridge.get("/framerates"), "framerates"),
        ("POST", p) if p.starts_with("/api/framerate/") => {
            let id = &p["/api/framerate/".len()..];
            if !profile_id(id) {
                return Response::error(400, "zły identyfikator profilu kodowania");
            }
            forward(
                app.bridge.post(&format!("/framerate/{id}"), &req.body),
                "framerate",
            )
        }
        ("POST", p) if p.starts_with("/api/session/") => session(app, &p["/api/session/".len()..]),
        ("POST", p) if p.starts_with(http::UPLOAD_PREFIX) => {
            upload(app, req, &p[http::UPLOAD_PREFIX.len()..])
        }
        ("POST", p) if p.starts_with("/api/cmd/") => {
            let name = &p["/api/cmd/".len()..];
            if !valid_command(name) {
                return Response::error(400, "zła nazwa komendy");
            }
            let call = || {
                forward(
                    app.bridge.post(&format!("/command/{name}"), &req.body),
                    name,
                )
            };
            let mut r = call();
            // zapytanie tylko do odczytu, a legacy rzucił wyjątkiem (np. baza zajęta budową playoutu w tle) – jedna ponowna próba
            if r.status == 500 && read_only(name) {
                eprintln!(
                    "cmd {name} -> 500, ponawiam: {}",
                    log_snippet(&mask_secrets(&r.body))
                );
                std::thread::sleep(std::time::Duration::from_millis(250));
                r = call();
            }
            r.body = mask_secrets(&r.body);
            if r.status >= 500 {
                eprintln!("cmd {name} -> {}: {}", r.status, log_snippet(&r.body));
            } else {
                eprintln!("cmd {name} -> {}", r.status);
            }
            r
        }
        _ => Response::error(404, "nieznany adres API"),
    }
}

/// Końcówki nazw pól (małymi literami), których wartości tekstowe nie wychodzą do przeglądarki (R8).
const SECRET_SUFFIXES: &[&str] = &["apikey", "token", "password", "secret"];
/// Wartość zamaskowanego sekretu; pusty sekret zostaje pusty (panel wie, że nic nie zapisano).
pub const MASK: &str = "********";

/// Maskuje w odpowiedzi JSON tekstowe wartości pól o nazwach z `SECRET_SUFFIXES`
/// (np. `ApiKey` z `GetJellyfinSecrets`). Sekrety są w panelu tylko do zapisu.
pub fn mask_secrets(body: &[u8]) -> Vec<u8> {
    fn string_end(b: &[u8], start: usize) -> usize {
        // start wskazuje cudzysłów otwierający; zwraca indeks za cudzysłowem zamykającym
        let mut i = start + 1;
        while i < b.len() {
            match b[i] {
                b'\\' => i += 2,
                b'"' => return i + 1,
                _ => i += 1,
            }
        }
        b.len()
    }
    let skip_ws = |b: &[u8], mut i: usize| {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        i
    };
    let mut out = Vec::with_capacity(body.len());
    let mut i = 0;
    while i < body.len() {
        if body[i] != b'"' {
            out.push(body[i]);
            i += 1;
            continue;
        }
        let end = string_end(body, i);
        out.extend_from_slice(&body[i..end]);
        let key = String::from_utf8_lossy(&body[i + 1..end.saturating_sub(1).max(i + 1)])
            .to_ascii_lowercase();
        i = end;
        let colon = skip_ws(body, i);
        if colon >= body.len()
            || body[colon] != b':'
            || !SECRET_SUFFIXES.iter().any(|s| key.ends_with(s))
        {
            continue;
        }
        let v = skip_ws(body, colon + 1);
        if v >= body.len() || body[v] != b'"' {
            continue;
        }
        let vend = string_end(body, v);
        out.extend_from_slice(&body[i..v]);
        if vend - v > 2 {
            out.extend_from_slice(format!("\"{MASK}\"").as_bytes());
        } else {
            out.extend_from_slice(b"\"\"");
        }
        i = vend;
    }
    out
}

/// Rodzaje obrazów, które panel wgrywa do legacy (`ArtworkKind`).
const UPLOAD_KINDS: &[&str] = &["Logo", "Watermark"];

/// Typ obrazu z nagłówka zgodny z sygnaturą pliku; tylko PNG, JPEG, GIF, WebP (bez SVG – skrypty).
pub fn image_type_ok(content_type: &str, b: &[u8]) -> bool {
    match content_type {
        "image/png" => b.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => b.starts_with(&[0xFF, 0xD8, 0xFF]),
        "image/gif" => b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a"),
        "image/webp" => b.len() >= 12 && b.starts_with(b"RIFF") && &b[8..12] == b"WEBP",
        _ => false,
    }
}

/// Identyfikator profilu kodowania w adresie `/api/framerate/{id}`: liczba 1–999999999.
fn profile_id(s: &str) -> bool {
    (1..=9).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit()) && !s.starts_with('0')
}

/// `POST /api/upload/{Logo|Watermark}`: obraz do pamięci obrazów legacy przez most; wynik = nazwa pliku.
fn upload(app: &App, req: &Request, kind: &str) -> Response {
    if !UPLOAD_KINDS.contains(&kind) {
        return Response::error(400, "rodzaj obrazu: Logo albo Watermark");
    }
    let ct = req
        .header("content-type")
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if req.body.is_empty() {
        return Response::error(400, "pusty plik");
    }
    if !image_type_ok(&ct, &req.body) {
        return Response::error(
            415,
            "dozwolone tylko obrazy PNG, JPEG, GIF i WebP (typ i treść pliku muszą się zgadzać)",
        );
    }
    let r = forward(
        app.bridge
            .post_as(&format!("/upload/{kind}"), &req.body, &ct),
        "upload",
    );
    eprintln!("upload {kind} {} B -> {}", req.body.len(), r.status);
    r
}

fn forward(r: Result<bridge::Reply, String>, what: &str) -> Response {
    match r {
        Ok(rep) if rep.body.first() == Some(&b'{') => Response::json(rep.status, rep.body),
        Ok(rep) => Response::error(
            502,
            &format!("most: nieoczekiwana odpowiedź {} dla {what}", rep.status),
        ),
        Err(e) => Response::error(502, &e),
    }
}

/// `POST /api/session/{kanał}/{start|stop|restart}` – emisja kanału w legacy (Na antenie).
/// Stop: most (`StopChannel`, jak przycisk Stop legacy). Start: to samo żądanie, którym widz
/// uruchamia kanał (`/iptv/channel/{kanał}.m3u8`, segmenter). Restart: stop, potem start.
fn session(app: &App, rest: &str) -> Response {
    let Some((ch, action)) = rest
        .split_once('/')
        .filter(|(ch, a)| channel_number(ch) && matches!(*a, "start" | "stop" | "restart"))
    else {
        return Response::error(404, "nieznany adres sesji (start, stop, restart)");
    };
    let mut stopped = false;
    if action != "start" {
        match app.bridge.post(&format!("/session/{ch}/stop"), b"") {
            Ok(r) if r.status == 200 => {
                stopped = r.body.windows(13).any(|w| w == br#""result":true"#)
            }
            Ok(r) => return forward(Ok(r), "session stop"),
            Err(e) => return Response::error(502, &e),
        }
        eprintln!("session {ch} stop -> {stopped}");
    }
    if action != "stop" {
        if stopped {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        match app
            .bridge
            .get_plain(&format!("/iptv/channel/{ch}.m3u8?mode=segmenter&subtitles=false"))
        {
            Ok(r) if r.status == 200 => eprintln!("session {ch} start -> ok"),
            Ok(r) if r.status == 404 => {
                return Response::error(
                    200,
                    "legacy nie uruchomił kanału (kanał wyłączony, bez playoutu albo błąd kodowania – Logi legacy)",
                )
            }
            Ok(r) => return Response::error(502, &format!("legacy: odpowiedź {} przy starcie kanału", r.status)),
            Err(e) => return Response::error(502, &e),
        }
    }
    Response::json(
        200,
        format!(
            "{{\"ok\":true,\"result\":{{\"stopped\":{stopped},\"started\":{}}}}}",
            action != "stop"
        ),
    )
}

/// Numer kanału ErsatzTV w adresie strumienia: cyfry z kropkami (`24`, `1.1`), bez `..`.
fn channel_number(s: &str) -> bool {
    (1..=16).contains(&s.len())
        && s.chars().all(|c| c.is_ascii_digit() || c == '.')
        && s.starts_with(|c: char| c.is_ascii_digit())
        && s.ends_with(|c: char| c.is_ascii_digit())
        && !s.contains("..")
}

/// Biała lista podglądu strumienia (`/api/stream/…`) → ścieżka w legacy i typ treści.
/// Tylko HLS kanału: playlista kanału (zawsze segmenter – ta sama sesja legacy, co u widzów,
/// bez drugiego kodowania) oraz pliki sesji (`/iptv/session/{kanał}/{plik}`: playlista i segmenty).
pub fn stream_path(rest: &str) -> Option<(String, &'static str)> {
    const M3U8: &str = "application/vnd.apple.mpegurl";
    if let Some(ch) = rest.strip_suffix(".m3u8").filter(|c| channel_number(c)) {
        return Some((
            format!("/iptv/channel/{ch}.m3u8?mode=segmenter&subtitles=false"),
            M3U8,
        ));
    }
    let ["session", ch, file] = rest.split('/').collect::<Vec<_>>()[..] else {
        return None;
    };
    let (name, ext) = file.rsplit_once('.')?;
    let ct = match ext {
        "m3u8" => M3U8,
        "ts" => "video/mp2t",
        "m4s" | "mp4" => "video/mp4",
        _ => return None,
    };
    let name_ok = (1..=64).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    // `.troubleshooting` – sesja testu odtwarzania legacy (odtwarzanie pozycji z Przeglądarki, `play`)
    ((channel_number(ch) || ch == PLAY_SESSION) && name_ok)
        .then(|| (format!("/iptv/session/{ch}/{file}"), ct))
}

/// Katalog sesji testu odtwarzania legacy (`/iptv/session/.troubleshooting/…`).
const PLAY_SESSION: &str = ".troubleshooting";

/// `{pozycja}/{profil}/{sekunda}` → test odtwarzania legacy (`/api/troubleshoot/playback.m3u8`): 30 s pozycji
/// od `sekunda`, kodowane profilem; segmenter HLS (MPEG-TS), silnik legacy. Sekunda do 7 dni.
pub fn play_path(rest: &str) -> Option<String> {
    let [item, profile, seek] = rest.split('/').collect::<Vec<_>>()[..] else {
        return None;
    };
    let seek_ok = (1..=6).contains(&seek.len())
        && seek.bytes().all(|b| b.is_ascii_digit())
        && seek.parse::<u32>().is_ok_and(|n| n <= 7 * 86_400);
    (profile_id(item) && profile_id(profile) && seek_ok).then(|| {
        format!("/api/troubleshoot/playback.m3u8?mediaItem={item}&ffmpegProfile={profile}&streamingMode=HttpLiveStreamingSegmenter&streamingEngine=Legacy&seekSeconds={seek}")
    })
}

/// `POST /api/play/{pozycja}/{profil}/{sekunda}` – fragment pozycji do odtworzenia w panelu. Legacy czeka,
/// aż powstaną pierwsze segmenty, i przekierowuje na playlistę sesji testowej; panel oddaje jej adres przez
/// `/api/stream/` (ten sam pośrednik HLS co podgląd kanału). Jedna sesja testowa naraz w całym legacy.
fn play(app: &App, rest: &str) -> Response {
    let Some(path) = play_path(rest) else {
        return Response::error(400, "zły adres odtwarzania (pozycja/profil/sekunda)");
    };
    match app.bridge.get_plain(&path) {
        Ok(r) if r.status == 302 || r.status == 200 => Response::json(
            200,
            format!(
                "{{\"ok\":true,\"result\":{{\"playlist\":\"/api/stream/session/{PLAY_SESSION}/live.m3u8\"}}}}"
            ),
        ),
        Ok(r) if r.status == 404 => Response::error(
            409,
            "legacy nie uruchomił odtwarzania – trwa inny test odtwarzania (spróbuj za chwilę), pozycja nie ma pliku albo profil nie istnieje (Logi legacy)",
        ),
        Ok(r) if r.status == 401 || r.status == 403 => Response::error(
            502,
            "legacy wymaga logowania do web GUI – test odtwarzania (/api/troubleshoot) jest dla panelu niedostępny",
        ),
        Ok(r) if r.status == 500 => Response::error(
            502,
            "legacy: kodowanie fragmentu nie powiodło się – spróbuj innego profilu kodowania (np. bez akceleracji sprzętowej); szczegóły w Logach legacy",
        ),
        Ok(r) => Response::error(502, &format!("legacy: odpowiedź {} przy odtwarzaniu", r.status)),
        Err(e) => Response::error(502, &e),
    }
}

/// `{posters|thumbnails|fanart}/{plik hex}` albo `…/{jellyfin|emby|plex}/{id}` (pole `Poster` legacy) →
/// obraz legacy `/artwork/…`. Panel podaje okładki sam: adres dla widzów (`--stream-url`) bywa
/// `localhost`, niewidoczny z innego komputera albo telefonu.
pub fn artwork_path(rest: &str) -> Option<String> {
    let parts = rest.split('/').collect::<Vec<_>>();
    let ok = match parts[..] {
        [dir, file] => {
            matches!(dir, "posters" | "thumbnails" | "fanart")
                && (1..=64).contains(&file.len())
                && file.bytes().all(|b| b.is_ascii_hexdigit())
        }
        [dir, src, id] => {
            matches!(dir, "posters" | "thumbnails" | "fanart")
                && matches!(src, "jellyfin" | "emby" | "plex")
                && profile_id(id)
        }
        _ => false,
    };
    ok.then(|| format!("/artwork/{rest}"))
}

/// Typ obrazu po pierwszych bajtach (legacy podaje okładki Jellyfin/Emby/Plex w ich formacie).
fn image_type(b: &[u8]) -> &'static str {
    match b {
        [0x89, b'P', b'N', b'G', ..] => "image/png",
        [b'G', b'I', b'F', ..] => "image/gif",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "image/webp",
        _ => "image/jpeg",
    }
}

fn artwork(app: &App, rest: &str) -> Response {
    let Some(path) = artwork_path(rest) else {
        return Response::error(404, "nieznany adres okładki");
    };
    match app.bridge.get_plain(&path) {
        Ok(r) if r.status == 200 && !r.body.is_empty() => {
            let ct = image_type(&r.body);
            Response::new(200, ct, r.body)
        }
        Ok(r) if r.status == 404 => Response::error(404, "legacy nie ma tej okładki"),
        Ok(r) => Response::error(502, &format!("legacy: odpowiedź {} dla okładki", r.status)),
        Err(e) => Response::error(502, &e),
    }
}

/// Adresy sesji w playliście legacy (`http://host:port/iptv/session/…`, legacy składa je z nagłówka
/// `Host` – tu adres wewnętrzny) → ścieżki pośrednika panelu (`/api/stream/session/…`).
pub fn rewrite_playlist(text: &str) -> String {
    const FROM: &str = "/iptv/session/";
    let mut out = String::with_capacity(text.len() + 64);
    let mut rest = text;
    while let Some(i) = rest.find(FROM) {
        // początek adresu: za białym znakiem, cudzysłowem albo `=`
        let start = rest[..i]
            .rfind(|c: char| c.is_whitespace() || c == '"' || c == '=')
            .map_or(0, |j| j + 1);
        let url_prefix = &rest[start..i];
        out.push_str(&rest[..start]);
        if !url_prefix.is_empty() && !url_prefix.contains("://") {
            out.push_str(url_prefix); // coś innego niż adres – bez zmian
            out.push_str(FROM);
        } else {
            out.push_str("/api/stream/session/");
        }
        rest = &rest[i + FROM.len()..];
    }
    out.push_str(rest);
    out
}

/// `GET /api/stream/…` – podgląd kanału w panelu: HLS legacy przez serwer panelu (ta sama sesja
/// segmentera, co u widzów; nic się nie koduje drugi raz). Playlisty dostają przepisane adresy
/// i `X-VizRS-Now` (zegar serwera w ms – odtwarzacz równa się do `PROGRAM-DATE-TIME`).
fn stream_proxy(app: &App, rest: &str) -> Response {
    let Some((path, ct)) = stream_path(rest) else {
        return Response::error(404, "nieznany adres strumienia");
    };
    match app.bridge.get_plain(&path) {
        Ok(r) if r.status == 200 => {
            let playlist = ct.ends_with("mpegurl");
            let body = if playlist {
                rewrite_playlist(&String::from_utf8_lossy(&r.body)).into_bytes()
            } else {
                r.body
            };
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis());
            Response::new(200, ct, body).with_header("X-VizRS-Now", now.to_string())
        }
        Ok(r) if r.status == 404 => Response::error(
            404,
            "legacy nie podał strumienia (kanał wyłączony, sesja wygasła albo nie wystartowała – Logi legacy)",
        ),
        Ok(r) => Response::error(502, &format!("legacy: odpowiedź {} dla strumienia", r.status)),
        Err(e) => Response::error(502, &e),
    }
}

/// Numer kanału ErsatzTV albo id audycji CG Playout: litery, cyfry, kropka, myślnik, podkreślnik.
fn cg_segment(s: &str) -> bool {
    (1..=120).contains(&s.len())
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._".contains(c))
        && s != "."
        && s != ".."
}

/// Dzień `RRRR-MM-DD` (dziennik emisji, K14).
fn valid_day(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

/// Biała lista pośrednika CG Playout: (metoda, ścieżka w panelu) → ścieżka w CG Playout.
pub fn cg_path(method: &str, rest: &str) -> Option<String> {
    let seg: Vec<&str> = rest.split('/').collect();
    if !seg.iter().skip(1).all(|s| cg_segment(s)) {
        return None;
    }
    let p = match (method, seg.as_slice()) {
        ("GET", ["health"]) => "/health".into(),
        ("GET", ["ingest", ch]) => format!("/ingest/state?channel={ch}"),
        ("GET", ["pages", ch]) => format!("/etv/pages?channel={ch}"),
        ("GET", ["live", "state"]) => "/api/live/state".into(),
        ("GET" | "PUT", ["live", "graphics"]) => "/api/live/graphics".into(),
        ("POST", ["live", "graphics", a @ ("off" | "on")]) => format!("/api/live/graphics/{a}"),
        ("PUT", ["live", "settings"]) => "/api/live/settings".into(),
        ("GET" | "PUT", ["live", "inputs"]) => "/api/live/inputs".into(),
        // koder programowy nakładki ffmpeg (preset, opcje x264) – encoder.json CG Playout
        ("GET" | "PUT", ["live", "encoder"]) => "/api/live/encoder".into(),
        ("GET", ["live", "shows"]) | ("POST", ["live", "shows"]) => "/api/live/shows".into(),
        ("POST", ["live", "shows", "scan"]) => "/api/live/shows/scan".into(),
        ("PUT" | "DELETE", ["live", "shows", id]) => format!("/api/live/shows/{id}"),
        ("PUT", ["live", "channels", ch]) => format!("/api/live/channels/{ch}"),
        ("POST", ["live", "channels", ch, a @ ("hold" | "release" | "rebuild")]) => {
            format!("/api/live/channels/{ch}/{a}")
        }
        // K13: ręczne TAKE / CONTINUE / OUT pojedynczej strony na antenie kanału (kontrakt CG – DOKUMENTACJA §3)
        ("POST", ["live", "channels", ch, a @ ("take" | "continue" | "out")]) => {
            format!("/api/live/channels/{ch}/{a}")
        }
        // logi emisji kanału z jednego dnia obok dziennika: sidecar CG, odbiorniki live ingest, przejęcia kanału
        ("GET", ["live", "logs", src @ ("cg" | "ingest" | "takeover"), ch, day])
            if valid_day(day) =>
        {
            format!("/api/live/logs?source={src}&channel={ch}&day={day}")
        }
        // K14: dziennik emisji kanału z jednego dnia (RRRR-MM-DD)
        ("GET", ["live", "asrun", ch, day]) if valid_day(day) => {
            format!("/api/live/asrun?channel={ch}&day={day}")
        }
        _ => return None,
    };
    Some(p)
}

/// Nazwa stanowiska do dziennika CG Playout: nagłówek `X-Operator` przeglądarki
/// (zakodowany `encodeURIComponent`, bo fetch nie wysyła polskich liter w nagłówku),
/// bez znaków sterujących, do 40 znaków; zawsze z przedrostkiem „VizRS Live”.
pub fn operator_name(raw: Option<&str>) -> String {
    let bytes = raw.unwrap_or("").as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    let name: String = String::from_utf8_lossy(&out)
        .chars()
        .filter(|c| !c.is_control())
        .take(40)
        .collect();
    match name.trim() {
        "" => "VizRS Live".into(),
        n => format!("VizRS Live · {n}"),
    }
}

/// Pośrednik `/api/cg/…` → VizRS CG Playout (R9): token CG zostaje w serwerze,
/// odpowiedź w kopercie panelu `{"ok":true,"result":…}` albo `{"ok":false,"error":…}`.
fn cg_proxy(app: &App, method: &str, rest: &str, body: &[u8], operator: &str) -> Response {
    let Some(cg) = &app.cg else {
        return Response::error(
            409,
            "VizRS CG Playout nie jest podłączony (--cg-url / VIZRS_LIVE_CG_URL)",
        );
    };
    let Some(path) = cg_path(method, rest) else {
        return Response::error(404, "nieznany adres CG Playout");
    };
    let body = if method == "GET" || method == "DELETE" {
        &[][..]
    } else {
        body
    };
    match cg.send_as(method, &path, body, operator) {
        Ok(r) if (200..300).contains(&r.status) => {
            let inner = if r.body.is_empty() {
                b"null".to_vec()
            } else {
                r.body
            };
            if !matches!(inner.first(), Some(b'{' | b'[' | b'n')) {
                return Response::error(502, "CG Playout: odpowiedź bez JSON");
            }
            let mut out = b"{\"ok\":true,\"result\":".to_vec();
            out.extend_from_slice(&mask_secrets(&inner));
            out.push(b'}');
            eprintln!("cg {method} {path} -> {}", r.status);
            Response::json(200, out)
        }
        Ok(r) => {
            let text = String::from_utf8_lossy(&r.body);
            let msg = cg_error(&text).unwrap_or_else(|| text.chars().take(300).collect());
            eprintln!("cg {method} {path} -> {}", r.status);
            // jak przy legacy: odrzucenie (walidacja CG Playout) = 200 z ok:false, awaria = 502
            let status = match r.status {
                401 | 404 => 502,
                400..=499 => 200,
                s => s,
            };
            let msg = match r.status {
                401 => "CG Playout odrzucił token panelu (--cg-token = panel_token CG Playout)"
                    .to_string(),
                404 if msg.is_empty() => {
                    "CG Playout: brak tej funkcji (panel_token pusty albo starsza wersja)"
                        .to_string()
                }
                _ => format!("CG Playout odrzucił: {msg}"),
            };
            Response::error(status, &msg)
        }
        Err(e) => Response::error(502, &e.replace("most", "CG Playout")),
    }
}

/// Numer strony z `<numer>.png` (miniatura strony VizRS); tylko cyfry, jak numery stron VizRS.
pub fn thumb_page(file: &str) -> Option<&str> {
    let n = file.strip_suffix(".png")?;
    (!n.is_empty() && n.len() <= 12 && n.bytes().all(|b| b.is_ascii_digit())).then_some(n)
}

/// `GET /api/cg/live/thumbs/<numer>.png` → miniatura strony VizRS z CG Playout (`/api/live/thumbs`),
/// obraz wprost (nie koperta JSON); 404 – CG Playout nie ma miniatury (panel pokazuje wtedy napis).
fn cg_thumb(app: &App, file: &str) -> Response {
    let Some(cg) = &app.cg else {
        return Response::error(409, "VizRS CG Playout nie jest podłączony");
    };
    let Some(n) = thumb_page(file) else {
        return Response::error(404, "nieznana miniatura");
    };
    match cg.send_as(
        "GET",
        &format!("/api/live/thumbs/{n}.png"),
        &[],
        "VizRS Live",
    ) {
        Ok(r) if r.status == 200 && r.body.starts_with(b"\x89PNG\r\n\x1a\n") => {
            Response::new(200, "image/png", r.body)
        }
        Ok(r) if r.status == 404 => Response::error(404, "brak miniatury tej strony"),
        Ok(r) => Response::error(
            502,
            &format!("CG Playout: odpowiedź {} dla miniatury", r.status),
        ),
        Err(e) => Response::error(502, &e.replace("most", "CG Playout")),
    }
}

/// Komunikat z `{"error":"…"}` CG Playout (bez parsera JSON: pierwszy napis po kluczu).
pub fn cg_error(body: &str) -> Option<String> {
    let i = body.find("\"error\"")? + 7;
    let rest = body[i..]
        .trim_start()
        .strip_prefix(':')?
        .trim_start()
        .strip_prefix('"')?;
    let mut out = String::new();
    let mut esc = false;
    for c in rest.chars() {
        match (esc, c) {
            (true, 'n') => {
                out.push(' ');
                esc = false
            }
            (true, c) => {
                out.push(c);
                esc = false
            }
            (false, '\\') => esc = true,
            (false, '"') => return Some(out),
            (false, c) => out.push(c),
        }
    }
    None
}

fn status(app: &App) -> Response {
    let (bridge, err) = match app.bridge.get("/health") {
        Ok(r) if r.status == 200 && r.body.first() == Some(&b'{') => (
            String::from_utf8_lossy(&r.body).into_owned(),
            "null".to_string(),
        ),
        Ok(r) => (
            "null".into(),
            json_str(&format!(
                "most odpowiedział {}: {}",
                r.status,
                String::from_utf8_lossy(&r.body)
            )),
        ),
        Err(e) => ("null".into(), json_str(&e)),
    };
    Response::json(
        200,
        format!(
            "{{\"ok\":true,\"result\":{{\"panel\":{},\"bridgeUrl\":{},\"streamUrl\":{},\"vizrsUrl\":{},\"vizrsOutputToken\":{},\"vizrsChannels\":{},\"multiviewMax\":{},\"cg\":{},\"auth\":{},\"bridge\":{bridge},\"bridgeError\":{err}}}}}",
            json_str(VERSION),
            json_str(&app.cfg.bridge_url),
            json_str(&app.cfg.stream_url),
            json_str(&app.cfg.vizrs_url),
            json_str(&app.cfg.vizrs_output_token),
            app.cfg.vizrs_channels,
            app.cfg.multiview_max,
            app.cg.is_some(),
            app.cfg.token.is_some()
        ),
    )
}

/// Ścieżki sprawdzane pod adresem dla widzów (R10): web GUI legacy, most, M3U.
pub const EXPOSURE_PROBES: [&str; 4] = [
    "/",
    "/channels",
    "/vizrs-bridge/health",
    "/iptv/channels.m3u",
];

/// Czy pod adresem dla widzów (`--stream-url`) widać web GUI legacy (R10). Kody HTTP ścieżek
/// z `EXPOSURE_PROBES`; ocenę robi UI. Sprawdza serwer panelu – z jego sieci, nie z internetu.
fn exposure(app: &App) -> Response {
    if app.cfg.stream_url.is_empty() {
        return Response::json(
            200,
            "{\"ok\":true,\"result\":{\"url\":\"\",\"error\":\"panel bez --stream-url\",\"probes\":[]}}",
        );
    }
    let target = match bridge::parse_url(&app.cfg.stream_url) {
        Ok(t) => t,
        Err(e) => {
            return Response::json(
                200,
                format!(
                    "{{\"ok\":true,\"result\":{{\"url\":{},\"error\":{},\"probes\":[]}}}}",
                    json_str(&app.cfg.stream_url),
                    json_str(&e)
                ),
            )
        }
    };
    let probe = Bridge {
        target,
        token: String::new(),
        prefix: "",
        auth: "X-VizRS-Probe",
    };
    let probes: Vec<String> = EXPOSURE_PROBES
        .iter()
        .map(|p| match probe.get(p) {
            Ok(r) => format!("{{\"path\":{},\"status\":{}}}", json_str(p), r.status),
            Err(e) => format!(
                "{{\"path\":{},\"status\":null,\"error\":{}}}",
                json_str(p),
                json_str(&e)
            ),
        })
        .collect();
    Response::json(
        200,
        format!(
            "{{\"ok\":true,\"result\":{{\"url\":{},\"error\":null,\"probes\":[{}]}}}}",
            json_str(&app.cfg.stream_url),
            probes.join(",")
        ),
    )
}

fn static_file(app: &App, req: &Request) -> Response {
    if req.method != "GET" && req.method != "HEAD" {
        return Response::error(405, "tylko GET");
    }
    let path = if req.path == "/index.html" {
        "/"
    } else {
        req.path.as_str()
    };
    let Some((p, ct, body)) = STATIC.iter().find(|(p, _, _)| *p == path) else {
        return Response::new(404, "text/plain; charset=utf-8", "nie ma takiej strony");
    };
    if let Some(dir) = &app.cfg.web {
        let file = if *p == "/" { "index.html" } else { &p[1..] };
        if let Ok(b) = std::fs::read(dir.join(file)) {
            return Response::new(200, ct, b);
        }
    }
    Response::new(200, ct, body.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(args: &[&str], env: &[(&str, &str)]) -> Result<Config, String> {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env: Vec<(String, String)> = env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        parse_config(&args, &|k| {
            env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
        })
    }

    const T: &str = "0123456789abcdef";

    #[test]
    fn config_defaults_and_overrides() {
        let c = cfg(&["--bridge-token", T], &[]).unwrap();
        assert_eq!(
            (c.bind.as_str(), c.port, c.bridge_url.as_str()),
            ("127.0.0.1", 8090, "http://127.0.0.1:8409")
        );
        let c = cfg(
            &["--port", "9000"],
            &[
                ("VIZRS_LIVE_PORT", "1"),
                ("VIZRS_LIVE_BRIDGE_TOKEN", T),
                ("VIZRS_LIVE_VIZRS_URL", "http://v:8088/"),
            ],
        )
        .unwrap();
        assert_eq!(c.port, 9000);
        assert_eq!(c.vizrs_url, "http://v:8088");
        assert!(!c.vizrs_channels && c.vizrs_output_token.is_empty());
    }

    #[test]
    fn config_vizrs() {
        let c = cfg(
            &["--vizrs-url", ":8789", "--vizrs-channels"],
            &[
                ("VIZRS_LIVE_BRIDGE_TOKEN", T),
                ("VIZRS_LIVE_VIZRS_OUTPUT_TOKEN", "out-Tok.1_x"),
            ],
        )
        .unwrap();
        assert_eq!(
            (
                c.vizrs_url.as_str(),
                c.vizrs_output_token.as_str(),
                c.vizrs_channels
            ),
            (":8789", "out-Tok.1_x", true)
        );
        let env = [
            ("VIZRS_LIVE_BRIDGE_TOKEN", T),
            ("VIZRS_LIVE_VIZRS_CHANNELS", "1"),
        ];
        assert!(
            cfg(&["--vizrs-url", "http://v:8088"], &env)
                .unwrap()
                .vizrs_channels
        );
        for bad in [
            "v:8088",
            ":0",
            ":x",
            "javascript:alert(1)",
            "http://v/\"x",
            "http://v/?token=a",
        ] {
            assert!(
                cfg(&["--bridge-token", T, "--vizrs-url", bad], &[]).is_err(),
                "{bad}"
            );
        }
        let url = [
            "--bridge-token",
            T,
            "--vizrs-url",
            ":8788",
            "--vizrs-output-token",
        ];
        assert!(cfg(&[&url[..], &["a&b=c"]].concat(), &[]).is_err());
        assert!(
            cfg(&["--bridge-token", T, "--vizrs-output-token", "abc"], &[]).is_err(),
            "bez adresu"
        );
        assert!(
            cfg(&["--bridge-token", T, "--vizrs-channels"], &[]).is_err(),
            "bez adresu"
        );
    }

    #[test]
    fn config_rejects_unsafe() {
        assert!(cfg(&[], &[]).is_err(), "brak tokenu mostu");
        assert!(
            cfg(&["--bridge-token", T, "--bind", "0.0.0.0"], &[]).is_err(),
            "sieć bez tokenu panelu"
        );
        assert!(cfg(
            &["--bridge-token", T, "--bind", "0.0.0.0", "--token", T],
            &[]
        )
        .is_ok());
        assert!(cfg(&["--bridge-token", T, "--token", "krótki"], &[]).is_err());
        assert!(cfg(
            &["--bridge-token", T, "--token", "zły token ze spacją!!"],
            &[]
        )
        .is_err());
        assert!(cfg(&["--bridge-token", T, "--bridge", "https://x"], &[]).is_err());
        assert!(cfg(&["--bridge-token", T, "--nieznana"], &[]).is_err());
    }

    #[test]
    fn command_names() {
        assert!(valid_command("GetAllChannels"));
        assert!(!valid_command(""));
        assert!(!valid_command("../health"));
        assert!(!valid_command("1Abc"));
        assert!(!valid_command("Get%20All"));
    }

    #[test]
    fn image_types() {
        assert!(image_type_ok("image/png", b"\x89PNG\r\n\x1a\n...."));
        assert!(!image_type_ok("image/png", b"GIF89a"));
        assert!(image_type_ok("image/jpeg", &[0xFF, 0xD8, 0xFF, 0xE0]));
        assert!(image_type_ok("image/webp", b"RIFF\0\0\0\0WEBPVP8 "));
        assert!(!image_type_ok("image/svg+xml", b"<svg"));
        assert!(!image_type_ok("text/html", b"<html"));
    }

    #[test]
    fn play_and_artwork_paths() {
        assert_eq!(
            play_path("42/1/90").as_deref(),
            Some("/api/troubleshoot/playback.m3u8?mediaItem=42&ffmpegProfile=1&streamingMode=HttpLiveStreamingSegmenter&streamingEngine=Legacy&seekSeconds=90")
        );
        assert!(play_path("42/1/0").is_some());
        for bad in [
            "42/1",
            "0/1/0",
            "42/0/0",
            "42/1/-5",
            "42/1/604801",
            "42/1/1/2",
            "x/1/0",
            "42/1/0&a=1",
        ] {
            assert!(play_path(bad).is_none(), "{bad}");
        }
        assert_eq!(
            stream_path(".troubleshooting/x"),
            None,
            "sama nazwa katalogu bez session/ – nie"
        );
        assert_eq!(
            stream_path("session/.troubleshooting/live000003.ts").map(|x| x.0),
            Some("/iptv/session/.troubleshooting/live000003.ts".to_string())
        );
        assert!(stream_path("session/..troubleshooting/live.m3u8").is_none());
        assert_eq!(
            artwork_path("posters/jellyfin/5").as_deref(),
            Some("/artwork/posters/jellyfin/5")
        );
        assert_eq!(
            artwork_path("thumbnails/0a1b2c").as_deref(),
            Some("/artwork/thumbnails/0a1b2c")
        );
        for bad in [
            "posters/../x",
            "logos/abc",
            "posters/jellyfin/0",
            "posters/kodi/5",
            "posters/abc.jpg",
            "posters/jellyfin/5/6",
            "posters",
        ] {
            assert!(artwork_path(bad).is_none(), "{bad}");
        }
        assert_eq!(image_type(b"\x89PNG\r\n"), "image/png");
        assert_eq!(image_type(b"RIFF1234WEBPVP8"), "image/webp");
        assert_eq!(image_type(&[0xff, 0xd8, 0xff]), "image/jpeg");
    }

    #[test]
    fn profile_id_guards() {
        assert!(profile_id("1") && profile_id("42") && profile_id("999999999"));
        for bad in ["", "0", "01", "-1", "1/2", "../x", "1a", "1234567890"] {
            assert!(!profile_id(bad), "{bad}");
        }
    }

    #[test]
    fn upload_guards() {
        let a = app(None);
        let mut r = req(
            "POST",
            "/api/upload/Logo",
            &[("X-VizRS-Live", "1"), ("Content-Type", "image/svg+xml")],
        );
        r.body = b"<svg/>".to_vec();
        assert_eq!(route(&a, &r).status, 415, "SVG odrzucony");
        r.headers[1].1 = "image/png".into();
        assert_eq!(route(&a, &r).status, 415, "treść nie PNG");
        r.path = "/api/upload/Poster".into();
        assert_eq!(route(&a, &r).status, 400, "zły rodzaj");
        r.path = "/api/upload/Logo".into();
        r.body = b"\x89PNG\r\n\x1a\nxxxx".to_vec();
        assert_eq!(route(&a, &r).status, 502, "dobry PNG idzie do mostu");
    }

    #[test]
    fn secrets_masked() {
        let m = |s: &str| String::from_utf8(mask_secrets(s.as_bytes())).unwrap();
        assert_eq!(
            m(r#"{"ok":true,"result":{"Address":"http://j:8096","ApiKey" : "abc\"d"}}"#),
            r#"{"ok":true,"result":{"Address":"http://j:8096","ApiKey" : "********"}}"#
        );
        assert_eq!(m(r#"{"apiKey":"","x":1}"#), r#"{"apiKey":"","x":1}"#);
        assert_eq!(
            m(r#"[{"AuthToken":"t","Name":"ApiKey"}]"#),
            r#"[{"AuthToken":"********","Name":"ApiKey"}]"#
        );
        // wartość nietekstowa i tekst wyglądający jak klucz zostają
        assert_eq!(
            m(r#"{"Token":null,"s":"apikey: x"}"#),
            r#"{"Token":null,"s":"apikey: x"}"#
        );
        assert_eq!(m(r#"{"Name":"zażółć"}"#), r#"{"Name":"zażółć"}"#);
    }

    #[test]
    fn cg_config() {
        let t = "--bridge-token";
        let k = "0123456789abcdef";
        assert!(cfg(&[t, k, "--cg-url", "http://cg:7700"], &[]).is_err());
        assert!(cfg(
            &[t, k, "--cg-url", "https://cg"],
            &[("VIZRS_LIVE_CG_TOKEN", "x")]
        )
        .is_err());
        let c = cfg(
            &[t, k],
            &[
                ("VIZRS_LIVE_CG_URL", "http://cg:7700/"),
                ("VIZRS_LIVE_CG_TOKEN", "x"),
            ],
        )
        .unwrap();
        assert_eq!(
            (c.cg_url.as_str(), c.cg_token.as_str()),
            ("http://cg:7700", "x")
        );
    }

    #[test]
    fn stream_whitelist() {
        let p = |r: &str| stream_path(r).map(|(p, _)| p);
        assert_eq!(
            p("24.m3u8").as_deref(),
            Some("/iptv/channel/24.m3u8?mode=segmenter&subtitles=false")
        );
        assert_eq!(
            p("1.1.m3u8").as_deref(),
            Some("/iptv/channel/1.1.m3u8?mode=segmenter&subtitles=false")
        );
        assert_eq!(
            p("session/24/live000123.ts").as_deref(),
            Some("/iptv/session/24/live000123.ts")
        );
        assert_eq!(
            stream_path("session/24/live.m3u8").unwrap().1,
            "application/vnd.apple.mpegurl"
        );
        for bad in [
            "x.m3u8",
            "..m3u8",
            "24.ts",
            "session/../live.m3u8",
            "session/24/../x.ts",
            "session/24/a/b.ts",
            "session/24/live.html",
            "session/24/.ts",
            "session/1..2/live.ts",
            "session/24",
            "",
        ] {
            assert!(stream_path(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn playlist_rewrite() {
        let master = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nhttp://ersatztv:8409/iptv/session/24/live.m3u8\n";
        assert_eq!(
            rewrite_playlist(master),
            "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\n/api/stream/session/24/live.m3u8\n"
        );
        assert_eq!(
            rewrite_playlist("#EXT-X-MEDIA:URI=\"http://h/iptv/session/1/s.m3u8\"\nlive000001.ts"),
            "#EXT-X-MEDIA:URI=\"/api/stream/session/1/s.m3u8\"\nlive000001.ts"
        );
        assert_eq!(rewrite_playlist("x/iptv/session/1"), "x/iptv/session/1");
    }

    #[test]
    fn stream_route_needs_login() {
        assert_eq!(
            route(&app(Some("t")), &req("GET", "/api/stream/24.m3u8", &[])).status,
            401
        );
        assert_eq!(
            route(&app(None), &req("GET", "/api/stream/session/24/x.exe", &[])).status,
            404
        );
    }

    #[test]
    fn cg_whitelist_k13_k14() {
        assert_eq!(
            cg_path("POST", "live/channels/24/take").as_deref(),
            Some("/api/live/channels/24/take")
        );
        assert_eq!(
            cg_path("POST", "live/channels/24/out").as_deref(),
            Some("/api/live/channels/24/out")
        );
        assert!(cg_path("GET", "live/channels/24/take").is_none());
        assert_eq!(
            cg_path("GET", "live/asrun/24/2026-11-09").as_deref(),
            Some("/api/live/asrun?channel=24&day=2026-11-09")
        );
        assert!(cg_path("GET", "live/asrun/24/2026-11-9").is_none());
        assert!(cg_path("GET", "live/asrun/24/x&y=1").is_none());
        assert_eq!(
            cg_path("GET", "live/logs/ingest/24/2026-11-14").as_deref(),
            Some("/api/live/logs?source=ingest&channel=24&day=2026-11-14")
        );
        assert!(cg_path("GET", "live/logs/etc/24/2026-11-14").is_none());
        assert!(cg_path("GET", "live/logs/cg/24/2026-11-1").is_none());
        assert!(cg_path("POST", "live/logs/cg/24/2026-11-14").is_none());
    }

    #[test]
    fn cg_whitelist() {
        assert_eq!(cg_path("GET", "health").as_deref(), Some("/health"));
        assert_eq!(
            cg_path("GET", "ingest/24").as_deref(),
            Some("/ingest/state?channel=24")
        );
        assert_eq!(
            cg_path("GET", "pages/1.5").as_deref(),
            Some("/etv/pages?channel=1.5")
        );
        assert_eq!(
            cg_path("POST", "live/channels/24/hold").as_deref(),
            Some("/api/live/channels/24/hold")
        );
        assert_eq!(
            cg_path("DELETE", "live/shows/24-wiadomosci").as_deref(),
            Some("/api/live/shows/24-wiadomosci")
        );
        assert_eq!(
            cg_path("PUT", "live/graphics").as_deref(),
            Some("/api/live/graphics")
        );
        assert_eq!(cg_path("DELETE", "live/graphics"), None);
        assert_eq!(
            cg_path("POST", "live/graphics/off").as_deref(),
            Some("/api/live/graphics/off")
        );
        assert_eq!(cg_path("POST", "live/graphics/x"), None);
        assert_eq!(thumb_page("1001.png"), Some("1001"));
        assert_eq!(thumb_page("1001"), None);
        assert_eq!(thumb_page("..%2F1.png"), None);
        assert_eq!(thumb_page(".png"), None);
        assert_eq!(
            cg_path("PUT", "live/settings").as_deref(),
            Some("/api/live/settings")
        );
        assert_eq!(cg_path("GET", "live/settings"), None);
        assert_eq!(
            cg_path("PUT", "live/inputs").as_deref(),
            Some("/api/live/inputs")
        );
        assert_eq!(cg_path("DELETE", "live/inputs"), None);
        assert_eq!(
            cg_path("PUT", "live/encoder").as_deref(),
            Some("/api/live/encoder")
        );
        assert_eq!(
            cg_path("GET", "live/encoder").as_deref(),
            Some("/api/live/encoder")
        );
        assert_eq!(cg_path("DELETE", "live/encoder"), None);
        assert_eq!(cg_path("PUT", "live/encoder/x"), None);
        assert_eq!(operator_name(None), "VizRS Live");
        assert_eq!(operator_name(Some("  ")), "VizRS Live");
        assert_eq!(
            operator_name(Some("Stanowisko%20%C5%81%0D%0AX-Evil:%201")),
            "VizRS Live · Stanowisko ŁX-Evil: 1"
        );
        assert_eq!(operator_name(Some("a%zz")), "VizRS Live · a%zz");
        assert_eq!(
            operator_name(Some(&"x".repeat(99))).chars().count(),
            13 + 40
        );
        assert_eq!(cg_path("GET", "ingest/24&x=1"), None);
        assert_eq!(cg_path("GET", "pages/.."), None);
        assert_eq!(cg_path("DELETE", "live/channels/24"), None);
        assert_eq!(cg_path("POST", "live/channels/24/delete"), None);
        assert_eq!(
            cg_error("{\"error\": \"zły \\\"tytuł\\\"\"}").as_deref(),
            Some("zły \"tytuł\"")
        );
        assert_eq!(cg_error("nic"), None);
        let a = app(None);
        assert_eq!(route(&a, &req("GET", "/api/cg/health", &[])).status, 409);
        assert_eq!(
            route(&a, &req("PUT", "/api/cg/live/channels/1", &[])).status,
            403
        );
        let csrf = [("x-vizrs-live", "1")];
        for bad in [
            "/api/session/1/kill",
            "/api/session/../stop",
            "/api/session/1",
        ] {
            assert_eq!(route(&a, &req("POST", bad, &csrf)).status, 404, "{bad}");
        }
        assert_eq!(
            route(&a, &req("POST", "/api/session/1/stop", &[])).status,
            403
        );
    }

    #[test]
    fn constant_time_eq() {
        assert!(eq_ct("abc", "abc"));
        assert!(!eq_ct("abc", "abd"));
        assert!(!eq_ct("abc", "ab"));
    }

    fn app(token: Option<&str>) -> App {
        let mut c = cfg(
            &["--bridge-token", T, "--bridge", "http://127.0.0.1:1"],
            &[],
        )
        .unwrap();
        c.token = token.map(String::from);
        App {
            bridge: Bridge {
                target: bridge::parse_url(&c.bridge_url).unwrap(),
                token: T.into(),
                prefix: "/vizrs-bridge",
                auth: "X-VizRS-Token",
            },
            cg: None,
            login_fails: std::sync::Mutex::new(LoginFails::default()),
            sessions: std::sync::Mutex::new(Sessions::default()),
            cfg: c,
        }
    }

    fn req(method: &str, path: &str, headers: &[(&str, &str)]) -> Request {
        Request {
            method: method.into(),
            path: path.into(),
            headers: headers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn every_static_file_served() {
        let a = app(Some(T));
        for (p, ct, body) in STATIC {
            let r = route(&a, &req("GET", p, &[]));
            assert_eq!(r.status, 200, "{p}");
            assert!(!body.is_empty(), "{p} pusty");
            assert_eq!(r.content_type, *ct, "{p}");
        }
    }

    /// Każdy import modułu w UI (`from './x.js'`, `import './views/y.js'`) wskazuje plik ze STATIC.
    #[test]
    fn module_imports_resolve() {
        for (p, _, body) in STATIC.iter().filter(|(p, _, _)| p.ends_with(".js")) {
            let dir = &p[..p.rfind('/').unwrap() + 1];
            for line in body.lines() {
                let l = line.trim_start();
                if !l.starts_with("import ") {
                    continue;
                }
                let Some(spec) = l.split(['\'', '"']).nth(1) else {
                    continue;
                };
                let mut parts: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
                for seg in spec.split('/') {
                    match seg {
                        "." => {}
                        ".." => {
                            parts.pop();
                        }
                        s => parts.push(s),
                    }
                }
                let target = format!("/{}", parts.join("/"));
                assert!(
                    STATIC.iter().any(|(q, _, _)| *q == target),
                    "{p}: import {spec} -> {target} nie ma w STATIC"
                );
            }
        }
    }

    #[test]
    fn routes_and_guards() {
        let a = app(Some(T));
        a.sessions
            .lock()
            .unwrap()
            .create(T.into(), std::time::Instant::now());
        assert_eq!(route(&a, &req("GET", "/", &[])).status, 200);
        assert_eq!(
            route(&a, &req("GET", "/health", &[])).status,
            200,
            "zdrowie bez tokenu"
        );
        assert_eq!(route(&a, &req("GET", "/nie", &[])).status, 404);
        assert_eq!(route(&a, &req("GET", "/api/status", &[])).status, 401);
        assert_eq!(
            route(
                &a,
                &req(
                    "POST",
                    "/api/cmd/GetAllChannels",
                    &[("Cookie", &format!("vizrs_live={T}"))]
                )
            )
            .status,
            403,
            "bez X-VizRS-Live"
        );
        let ok = route(
            &a,
            &req(
                "GET",
                "/api/status",
                &[("Authorization", &format!("Bearer {T}"))],
            ),
        );
        assert_eq!(ok.status, 200);
        assert!(
            String::from_utf8_lossy(&ok.body).contains("\"bridgeError\":\""),
            "most nieosiągalny opisany w statusie"
        );
        let bad = route(
            &a,
            &req(
                "POST",
                "/api/cmd/..",
                &[
                    ("X-VizRS-Live", "1"),
                    ("Cookie", &format!("vizrs_live={T}")),
                ],
            ),
        );
        assert_eq!(bad.status, 400);
        let down = route(
            &a,
            &req(
                "POST",
                "/api/cmd/GetAllChannels",
                &[
                    ("X-VizRS-Live", "1"),
                    ("Cookie", &format!("vizrs_live={T}")),
                ],
            ),
        );
        assert_eq!(down.status, 502);
        assert_eq!(
            route(&app(None), &req("GET", "/api/commands", &[])).status,
            502,
            "bez tokenu panelu lokalnie wpuszcza"
        );
    }

    #[test]
    fn read_only_commands() {
        assert!(
            read_only("GetFuturePlayoutItemsById")
                && read_only("QuerySearchIndexMovies")
                && read_only("CountMediaItemsByLibrary")
        );
        assert!(
            !read_only("BuildPlayout")
                && !read_only("Getaway")
                && !read_only("SearchX".trim_end_matches('X'))
                && !read_only("UpdatePlayout")
        );
    }

    #[test]
    fn login_limit_and_cookie() {
        let a = app(Some("0123456789abcdef"));
        let ip = |n: u8| Some(std::net::IpAddr::from([10, 0, 0, n]));
        for _ in 0..LOGIN_MAX_FAILS {
            assert!(!login_limited(&a, ip(1), false));
            login_limited(&a, ip(1), true);
        }
        let mut r = req("POST", "/api/login", &[("X-VizRS-Live", "1")]);
        r.body = b"0123456789abcdef".to_vec();
        r.peer = ip(1);
        assert_eq!(
            route(&a, &r).status,
            429,
            "nawet dobry token po 10 błędach z tego adresu"
        );
        let mut other = req("POST", "/api/login", &[("X-VizRS-Live", "1")]);
        other.body = r.body.clone();
        other.peer = ip(2);
        assert_eq!(route(&a, &other).status, 200, "inny adres loguje się dalej");
        // bezpiecznik: wiele adresów razem blokuje wszystkich
        let all = app(Some("0123456789abcdef"));
        for n in 0..LOGIN_MAX_FAILS_ALL {
            login_limited(&all, ip((n % 200) as u8 + 3), true);
        }
        assert!(
            login_limited(&all, ip(250), false),
            "po {LOGIN_MAX_FAILS_ALL} błędach łącznie blokada dla wszystkich"
        );
        let b = app(Some("0123456789abcdef"));
        let ok = route(&b, &r);
        assert_eq!(ok.status, 200);
        let c = &ok.headers.iter().find(|h| h.0 == "Set-Cookie").unwrap().1;
        assert!(c.contains("HttpOnly") && c.contains("SameSite=Strict") && !c.contains("Secure"));
        let mut tls = req(
            "POST",
            "/api/login",
            &[("X-VizRS-Live", "1"), ("X-Forwarded-Proto", "https")],
        );
        tls.body = r.body.clone();
        let c2 = route(&b, &tls);
        assert!(c2.headers.iter().any(|h| h.1.ends_with("; Secure")));
        let mut out = Vec::new();
        Response::json(200, "{}").write_to(&mut out).unwrap();
        let head = String::from_utf8_lossy(&out);
        assert!(head.contains("frame-ancestors 'none'") && head.contains("X-Frame-Options: DENY"));
    }

    #[test]
    fn multiview_option() {
        let base = ["--bridge-token", T];
        assert_eq!(cfg(&base, &[]).unwrap().multiview_max, 4);
        assert_eq!(
            cfg(&base, &[("VIZRS_LIVE_MULTIVIEW_MAX", "0")])
                .unwrap()
                .multiview_max,
            0
        );
        assert_eq!(
            cfg(&["--bridge-token", T, "--multiview-max", "9"], &[])
                .unwrap()
                .multiview_max,
            9
        );
        assert!(cfg(&["--bridge-token", T, "--multiview-max", "17"], &[]).is_err());
        assert!(cfg(&base, &[("VIZRS_LIVE_MULTIVIEW_MAX", "x")]).is_err());
    }

    #[test]
    fn csp_known_origins() {
        assert_eq!(
            http::csp_origin("http://tv.lan:8409/base"),
            vec!["http://tv.lan:8409"]
        );
        assert_eq!(
            http::csp_origin("https://[::1]:8788"),
            vec!["https://[::1]:8788"]
        );
        assert_eq!(
            http::csp_origin(":8788"),
            vec!["http://*:8788", "https://*:8788"]
        );
        assert!(http::csp_origin("").is_empty() && http::csp_origin("ftp://x").is_empty());
        assert!(http::csp_origin("http://a b").is_empty());
        let bare = http::build_csp(&[], &[]);
        assert!(bare.contains("img-src 'self' data:;") && bare.contains("frame-src 'none'"));
        assert!(!bare.contains(" http:") && !bare.contains(" https:"));
        let full = http::build_csp(
            &http::csp_origin("http://tv.lan:8409"),
            &http::csp_origin(":8788"),
        );
        assert!(full.contains("img-src 'self' data: http://tv.lan:8409;"));
        assert!(full.contains("frame-src http://*:8788 https://*:8788;"));
    }

    #[test]
    fn sessions_expire_limit_logout() {
        let t0 = std::time::Instant::now();
        let mut s = Sessions::default();
        s.create("a".into(), t0);
        assert!(s.touch("a", t0 + std::time::Duration::from_secs(10)));
        assert!(!s.touch("b", t0));
        // bezczynność liczona od ostatniego użycia
        assert!(s.touch("a", t0 + SESSION_IDLE));
        assert!(!s.touch(
            "a",
            t0 + SESSION_IDLE * 2 + std::time::Duration::from_secs(11)
        ));
        assert_eq!(s.len(), 0, "wygasła sesja usunięta");
        for n in 0..SESSION_MAX + 5 {
            s.create(
                format!("s{n}"),
                t0 + std::time::Duration::from_secs(n as u64),
            );
        }
        assert_eq!(s.len(), SESSION_MAX, "limit sesji");
        assert!(!s.touch("s0", t0), "najdawniej używana wypada pierwsza");
        s.remove("s10");
        assert!(!s.touch("s10", t0 + std::time::Duration::from_secs(1000)));
        let id = random_id().unwrap();
        assert_eq!(id.len(), 64);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(id, random_id().unwrap());
    }

    #[test]
    fn login_session_logout_flow() {
        let tok = "0123456789abcdef";
        let a = app(Some(tok));
        let mut l = req("POST", "/api/login", &[("X-VizRS-Live", "1")]);
        l.body = tok.as_bytes().to_vec();
        let ok = route(&a, &l);
        let c = &ok.headers.iter().find(|h| h.0 == "Set-Cookie").unwrap().1;
        let id = c
            .strip_prefix("vizrs_live=")
            .and_then(|r| r.split(';').next())
            .unwrap()
            .to_string();
        assert!(!c.contains(tok), "token panelu nie trafia do ciasteczka");
        assert_eq!(id.len(), 64);
        let cookie = format!("vizrs_live={id}");
        assert_eq!(
            route(&a, &req("GET", "/api/exposure", &[("Cookie", &cookie)])).status,
            200
        );
        assert_eq!(
            route(
                &a,
                &req(
                    "GET",
                    "/api/exposure",
                    &[("Cookie", &format!("vizrs_live={tok}"))]
                )
            )
            .status,
            401,
            "sam token w ciasteczku już nie wpuszcza"
        );
        assert_eq!(
            route(
                &a,
                &req(
                    "GET",
                    "/api/exposure",
                    &[("Authorization", &format!("Bearer {tok}"))]
                )
            )
            .status,
            200,
            "token w nagłówku dla skryptów"
        );
        let out = route(
            &a,
            &req(
                "POST",
                "/api/logout",
                &[("X-VizRS-Live", "1"), ("Cookie", &cookie)],
            ),
        );
        assert_eq!(out.status, 200);
        assert_eq!(
            route(&a, &req("GET", "/api/exposure", &[("Cookie", &cookie)])).status,
            401,
            "po wylogowaniu sesja nie działa"
        );
    }

    #[test]
    fn exposure_probes() {
        let mut a = app(None);
        let r = route(&a, &req("GET", "/api/exposure", &[]));
        assert_eq!(r.status, 200);
        assert!(String::from_utf8_lossy(&r.body).contains("panel bez --stream-url"));
        a.cfg.stream_url = "http://127.0.0.1:1".into();
        let body = String::from_utf8_lossy(&route(&a, &req("GET", "/api/exposure", &[])).body)
            .into_owned();
        for p in EXPOSURE_PROBES {
            assert!(
                body.contains(&format!("{{\"path\":\"{p}\",\"status\":null")),
                "{body}"
            );
        }
        assert_eq!(
            route(&app(Some("t")), &req("GET", "/api/exposure", &[])).status,
            401
        );
    }
}
