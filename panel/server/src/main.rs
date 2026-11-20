//! VizRS Live – panel kanału nad ErsatzTV legacy.
//!
//! Serwer podaje UI panelu i przekazuje komendy do mostu VizRS Bridge (w procesie legacy).
//! Token mostu zostaje po stronie serwera; przeglądarka zna tylko (opcjonalny) token panelu.

mod accounts;
mod bridge;
mod config;
mod crypto;
mod http;
mod json;
mod proxy;
mod routes;
mod secrets;
mod session;
mod upload;
mod vod;

use accounts::*;
use config::*;
use proxy::*;
use routes::*;
use secrets::*;
use session::*;
use upload::*;

use std::net::{IpAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use bridge::Bridge;
use http::{json_str, Request, Response};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_CONN: usize = 64;
const COOKIE: &str = "vizrs_live";

/// Górna granica `--multiview-max` (ochrona maszyny: każdy podgląd to transkodująca sesja Next).
pub const MULTIVIEW_LIMIT: u32 = 16;

struct App {
    cfg: Config,
    bridge: Bridge,
    cg: Option<Bridge>,
    /// nieudane logowania w bieżących oknach: na adres klienta i łącznie
    login_fails: std::sync::Mutex<LoginFails>,
    /// konta operatorów i sesje przeglądarek (K1: losowy identyfikator w ciasteczku → kto, ostatnie użycie)
    accounts: std::sync::Mutex<Store>,
}

/// Najwyżej tyle nieudanych logowań z jednego adresu na [`LOGIN_WINDOW`] – potem 429 dla tego adresu do końca okna.
pub const LOGIN_MAX_FAILS: u32 = 10;
/// Bezpiecznik: tyle nieudanych logowań łącznie (z wielu adresów) blokuje logowanie wszystkim do końca okna.
pub const LOGIN_MAX_FAILS_ALL: u32 = 100;
pub const LOGIN_WINDOW: std::time::Duration = std::time::Duration::from_secs(60);
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
        accounts: std::sync::Mutex::new(Store::open(cfg.data.clone(), unix_now())),
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
            "nie"
        }
    );
    log_accounts(&app);

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

/// Stan kont na starcie; bez kont – jednorazowy kod pierwszej konfiguracji (widać go w logu kontenera, np. w Portainerze).
fn log_accounts(app: &App) {
    let a = app.accounts.lock().unwrap_or_else(|e| e.into_inner());
    match (&app.cfg.data, a.broken()) {
        (None, _) if app.cfg.token.is_some() => {
            eprintln!("vizrs-live: konta wyłączone (bez --data / VIZRS_LIVE_DATA) – logowanie tokenem")
        }
        (None, _) => eprintln!("vizrs-live: bez logowania (tylko 127.0.0.1, bez tokenu i kont)"),
        (Some(d), Some(e)) => eprintln!(
            "vizrs-live: KONTA WYŁĄCZONE – {e} (katalog {}); logowanie tylko tokenem awaryjnym",
            d.display()
        ),
        (Some(d), None) => match a.setup_code() {
            Some(code) => eprintln!(
                "vizrs-live: brak kont – otwórz panel i utwórz konto administratora; kod pierwszej konfiguracji: {code}{}",
                if app.cfg.token.is_some() {
                    " (albo token VIZRS_LIVE_TOKEN)"
                } else {
                    ""
                }
            ),
            None => eprintln!("vizrs-live: konta w {}", d.display()),
        },
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

/// Wartość zamaskowanego sekretu; pusty sekret zostaje pusty (panel wie, że nic nie zapisano).
pub const MASK: &str = "********";

/// Ścieżki sprawdzane pod adresem dla widzów (R10): web GUI legacy, most, M3U.
pub const EXPOSURE_PROBES: [&str; 4] = [
    "/",
    "/channels",
    "/vizrs-bridge/health",
    "/iptv/channels.m3u",
];

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
            cg_path("DELETE", "live/youtube-cookies").as_deref(),
            Some("/api/live/youtube-cookies")
        );
        assert!(cg_path("POST", "live/youtube-cookies").is_none());
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
        assert_eq!(operator_for("", None), "VizRS Live");
        assert_eq!(operator_for("", Some("  ")), "VizRS Live");
        assert_eq!(
            operator_for("", Some("Stanowisko%20%C5%81%0D%0AX-Evil:%201")),
            "VizRS Live · Stanowisko ŁX-Evil: 1"
        );
        assert_eq!(operator_for("", Some("a%zz")), "VizRS Live · a%zz");
        assert_eq!(
            operator_for("", Some(&"x".repeat(99))).chars().count(),
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
            accounts: std::sync::Mutex::new(Store::default()),
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
        a.accounts.lock().unwrap().session_new(T, "", unix_now());
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
    fn random_session_ids() {
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

    fn post(path: &str, body: &str, cookie: Option<&str>) -> Request {
        let mut h = vec![("X-VizRS-Live", "1")];
        if let Some(c) = cookie {
            h.push(("Cookie", c));
        }
        let mut r = req("POST", path, &h);
        r.body = body.as_bytes().to_vec();
        r
    }
    fn cookie_of(r: &Response) -> String {
        let c = &r.headers.iter().find(|h| h.0 == "Set-Cookie").unwrap().1;
        c.split(';').next().unwrap().to_string()
    }
    fn body(r: &Response) -> String {
        String::from_utf8_lossy(&r.body).into_owned()
    }

    #[test]
    fn accounts_setup_login_roles() {
        let dir = std::env::temp_dir().join(format!("vizrs-live-routes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut a = app(Some("0123456789abcdef"));
        a.cfg.data = Some(dir.clone());
        a.accounts = std::sync::Mutex::new(Store::open(Some(dir.clone()), unix_now()));
        let code = a.accounts.lock().unwrap().setup_code().unwrap().to_string();
        let st = body(&route(&a, &req("GET", "/api/auth", &[])));
        assert!(
            st.contains("\"setup\":true") && st.contains("\"user\":null"),
            "{st}"
        );
        assert!(!st.contains(&code), "kodu nie ma w odpowiedzi");
        assert_eq!(route(&a, &req("GET", "/api/status", &[])).status, 401);
        // pierwsze konto: zły kod, słabe hasło, potem kod z logu
        assert_eq!(
            route(
                &a,
                &post(
                    "/api/setup",
                    r#"{"code":"ZLY","user":"jan","password":"hasło-jana"}"#,
                    None
                )
            )
            .status,
            401
        );
        let weak = format!(r#"{{"code":"{code}","user":"jan","password":"krótkie"}}"#);
        assert_eq!(route(&a, &post("/api/setup", &weak, None)).status, 400);
        let ok = format!(
            r#"{{"code":"{}","user":"Jan","password":"hasło-jana"}}"#,
            code.to_lowercase()
        );
        let r = route(&a, &post("/api/setup", &ok, None));
        assert_eq!(r.status, 200, "{}", body(&r));
        let jan = cookie_of(&r);
        assert_eq!(
            route(&a, &post("/api/setup", &ok, None)).status,
            409,
            "drugi raz już nie"
        );
        let me = body(&route(&a, &req("GET", "/api/auth", &[("Cookie", &jan)])));
        assert!(
            me.contains(r#""user":{"name":"jan","role":"admin"}"#),
            "{me}"
        );
        // administrator dodaje operatora
        let add = route(
            &a,
            &post(
                "/api/users",
                r#"{"user":"ola","password":"hasło-oli","role":"operator"}"#,
                Some(&jan),
            ),
        );
        assert_eq!(add.status, 200, "{}", body(&add));
        let list = body(&route(&a, &req("GET", "/api/users", &[("Cookie", &jan)])));
        assert!(list.contains("\"ola\"") && !list.contains("salt"), "{list}");
        // logowanie loginem i hasłem
        assert_eq!(
            route(
                &a,
                &post(
                    "/api/login",
                    r#"{"user":"ola","password":"zle-haslo"}"#,
                    None
                )
            )
            .status,
            401
        );
        assert_eq!(
            route(
                &a,
                &post(
                    "/api/login",
                    r#"{"user":"nikt","password":"hasło-oli"}"#,
                    None
                )
            )
            .status,
            401
        );
        let r = route(
            &a,
            &post(
                "/api/login",
                r#"{"user":"OLA","password":"hasło-oli"}"#,
                None,
            ),
        );
        assert_eq!(r.status, 200);
        let ola = cookie_of(&r);
        assert_eq!(
            route(&a, &req("GET", "/api/exposure", &[("Cookie", &ola)])).status,
            200
        );
        assert_eq!(
            route(&a, &req("GET", "/api/users", &[("Cookie", &ola)])).status,
            403,
            "operator nie zarządza kontami"
        );
        // własne hasło
        assert_eq!(
            route(
                &a,
                &post(
                    "/api/password",
                    r#"{"old":"zle","new":"nowe-hasło-oli"}"#,
                    Some(&ola)
                )
            )
            .status,
            403
        );
        assert_eq!(
            route(
                &a,
                &post(
                    "/api/password",
                    r#"{"old":"hasło-oli","new":"nowe-hasło-oli"}"#,
                    Some(&ola)
                )
            )
            .status,
            200
        );
        // token awaryjny dalej działa (tekst i JSON), sesja tokenu = administrator
        let t = route(&a, &post("/api/login", "0123456789abcdef", None));
        assert_eq!(t.status, 200);
        let tj = route(
            &a,
            &post("/api/login", r#"{"token":"0123456789abcdef"}"#, None),
        );
        assert_eq!(tj.status, 200);
        assert_eq!(
            route(&a, &req("GET", "/api/users", &[("Cookie", &cookie_of(&t))])).status,
            200
        );
        // jedyny administrator zostaje; usunięcie konta kończy jego sesję
        let mut del = req(
            "DELETE",
            "/api/users/jan",
            &[("X-VizRS-Live", "1"), ("Cookie", &jan)],
        );
        assert_eq!(route(&a, &del).status, 409);
        del.path = "/api/users/ola".into();
        assert_eq!(route(&a, &del).status, 200);
        assert_eq!(
            route(&a, &req("GET", "/api/exposure", &[("Cookie", &ola)])).status,
            401
        );
        // wspólne logowanie w pakiecie: domyślnie wyłączone, przełącza tylko administrator
        let sso = |a: &App| body(&route(a, &req("GET", "/api/sso", &[])));
        assert!(
            sso(&a).contains(r#""available":true,"enabled":false"#),
            "{}",
            sso(&a)
        );
        let verify = r#"{"user":"jan","password":"hasło-jana"}"#;
        assert_eq!(
            route(&a, &post("/api/sso/verify", verify, None)).status,
            403
        );
        let mut on = req(
            "PUT",
            "/api/sso",
            &[("X-VizRS-Live", "1"), ("Cookie", &jan)],
        );
        on.body = br#"{"enabled":true}"#.to_vec();
        assert_eq!(route(&a, &on).status, 200);
        assert!(sso(&a).contains(r#""enabled":true"#));
        assert_eq!(
            route(
                &a,
                &post(
                    "/api/sso/verify",
                    r#"{"user":"jan","password":"zle-haslo"}"#,
                    None
                )
            )
            .status,
            401
        );
        let v = route(&a, &post("/api/sso/verify", verify, None));
        assert_eq!(v.status, 200);
        let vb = body(&v);
        assert!(
            vb.contains(r#""user":{"name":"jan","role":"admin"}"#),
            "{vb}"
        );
        let sid = vb
            .split("\"session\":\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .to_string();
        assert_eq!(sid.len(), 64);
        let by_cookie =
            |a: &App, c: &str| route(a, &req("GET", "/api/sso/session", &[("Cookie", c)])).status;
        assert_eq!(
            by_cookie(&a, &format!("vizrs_live={sid}")),
            200,
            "sesja z verify"
        );
        assert_eq!(by_cookie(&a, &jan), 200, "sesja przeglądarki panelu");
        assert_eq!(
            by_cookie(&a, &cookie_of(&t)),
            401,
            "sesja tokenu to nie konto"
        );
        assert_eq!(by_cookie(&a, "vizrs_live=zly"), 401);
        on.body = br#"{"enabled":false}"#.to_vec();
        assert_eq!(route(&a, &on).status, 200);
        assert_eq!(
            by_cookie(&a, &jan),
            403,
            "wyłączone = koniec sesji w CG System"
        );
        // sesje przetrwały „restart” (nowy magazyn z tego samego katalogu)
        a.accounts = std::sync::Mutex::new(Store::open(Some(dir.clone()), unix_now()));
        assert_eq!(
            route(&a, &req("GET", "/api/exposure", &[("Cookie", &jan)])).status,
            200
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn operator_signature_with_account() {
        assert_eq!(operator_for("jan", None), "VizRS Live · jan");
        assert_eq!(
            operator_for("jan", Some("Studio%201")),
            "VizRS Live · jan (Studio 1)"
        );
        assert_eq!(operator_for("jan", Some("JAN")), "VizRS Live · jan");
    }

    #[test]
    fn data_dir_allows_network_without_token() {
        assert!(cfg(
            &["--bridge-token", T, "--bind", "0.0.0.0", "--data", "/x"],
            &[]
        )
        .is_ok());
        assert!(cfg(
            &["--bridge-token", T, "--bind", "0.0.0.0"],
            &[("VIZRS_LIVE_DATA", "/x")]
        )
        .is_ok());
        assert!(cfg(
            &["--bridge-token", T, "--bind", "0.0.0.0"],
            &[("VIZRS_LIVE_DATA", "")]
        )
        .is_err());
    }
}
