//! Routing panelu: strony i pliki statyczne (`STATIC`), API, komendy ErsatzTV (tylko do odczytu / dozwolone), stan i ekspozycja.

use super::*;

pub(crate) const STATIC: &[(&str, &str, &str)] = &[
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
        "/views/zdarzenia.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/views/zdarzenia.js"),
    ),
    (
        "/views/uzytkownicy.js",
        "text/javascript; charset=utf-8",
        include_str!("../web/views/uzytkownicy.js"),
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

pub(crate) fn route(app: &App, req: &Request) -> Response {
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
        ("GET", "/api/auth") => return auth_state(app, req),
        ("POST", "/api/login") => return login(app, req),
        ("POST", "/api/setup") => return setup(app, req),
        ("POST", "/api/logout") => return logout(app, req),
        (_, "/api/sso") => return sso(app, req, ""),
        (_, p) if p.starts_with("/api/sso/") => return sso(app, req, &p["/api/sso/".len()..]),
        _ => {}
    }
    let Some(who) = authorized(app, req) else {
        return Response::error(401, "zaloguj się");
    };
    match (req.method.as_str(), path) {
        ("POST", "/api/password") => change_password(app, req, &who),
        (_, "/api/users") => users(app, req, &who, ""),
        (_, p) if p.starts_with("/api/users/") => users(app, req, &who, &p["/api/users/".len()..]),
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
            &operator_for(&who.user, req.header("x-operator")),
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

/// Zapytanie legacy tylko do odczytu (bezpieczne do ponowienia): `Get…`, `Query…`, `Search…`, `Count…`.
pub fn read_only(name: &str) -> bool {
    ["Get", "Query", "Search", "Count"].iter().any(|p| {
        name.strip_prefix(p)
            .is_some_and(|r| r.starts_with(|c: char| c.is_ascii_uppercase()))
    })
}

/// Początek odpowiedzi do dziennika (jedna linia, najwyżej 300 znaków).
pub(crate) fn log_snippet(body: &[u8]) -> String {
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

pub(crate) fn status(app: &App) -> Response {
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
            auth_required(app)
        ),
    )
}

/// Czy pod adresem dla widzów (`--stream-url`) widać web GUI legacy (R10). Kody HTTP ścieżek
/// z `EXPOSURE_PROBES`; ocenę robi UI. Sprawdza serwer panelu – z jego sieci, nie z internetu.
pub(crate) fn exposure(app: &App) -> Response {
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

pub(crate) fn static_file(app: &App, req: &Request) -> Response {
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
