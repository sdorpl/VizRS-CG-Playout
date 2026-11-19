//! Sesje operatorów: identyfikatory, ciasteczko, limit nieudanych logowań, autoryzacja żądań, logowanie i wylogowanie.

use super::*;

/// 32 losowe bajty z `/dev/urandom` jako 64 znaki szesnastkowe – identyfikator sesji w ciasteczku.
pub fn random_id() -> Result<String, String> {
    crypto::random_bytes(32).map(|b| crypto::hex(&b))
}

/// Okna nieudanych logowań: `(początek okna, liczba)` na adres IP klienta i łącznie (bezpiecznik).
#[derive(Default)]
pub(crate) struct LoginFails {
    per_ip: std::collections::HashMap<Option<std::net::IpAddr>, (std::time::Instant, u32)>,
    all: Option<(std::time::Instant, u32)>,
}

/// Najwyżej tyle adresów w pamięci limitu (po usunięciu wygasłych okien; nadmiar = bezpiecznik łączny).
pub(crate) const LOGIN_MAX_PEERS: usize = 4096;

/// `true` = logowanie z adresu `peer` zablokowane; `failed` = dolicz nieudaną próbę.
pub(crate) fn login_limited(app: &App, peer: Option<std::net::IpAddr>, failed: bool) -> bool {
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
pub(crate) fn session_cookie(req: &Request, value: &str, max_age: u32) -> String {
    let secure = if req.header("x-forwarded-proto") == Some("https") {
        "; Secure"
    } else {
        ""
    };
    format!("{COOKIE}={value}; HttpOnly; SameSite=Strict; Path=/; Max-Age={max_age}{secure}")
}

pub(crate) fn eq_ct(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// Logowanie wymagane: jest token awaryjny albo konta (katalog danych).
pub(crate) fn auth_required(app: &App) -> bool {
    app.cfg.token.is_some() || app.cfg.data.is_some()
}

/// Kto wysłał żądanie; `None` = niezalogowany. Skrypty: token awaryjny w `Authorization: Bearer`;
/// przeglądarka: identyfikator sesji w ciasteczku (K1). Panel bez tokenu i bez kont (tylko 127.0.0.1) –
/// każdy jak administrator.
pub(crate) fn authorized(app: &App, req: &Request) -> Option<Who> {
    if !auth_required(app) {
        return Some(Who::token());
    }
    if let (Some(token), Some(given)) = (
        &app.cfg.token,
        req.header("authorization")
            .and_then(|h| h.strip_prefix("Bearer ")),
    ) {
        if eq_ct(given, token) {
            return Some(Who::token());
        }
    }
    let id = req.cookie(COOKIE)?;
    app.accounts
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .session_check(id, unix_now())
}

fn accounts(app: &App) -> std::sync::MutexGuard<'_, Store> {
    app.accounts.lock().unwrap_or_else(|e| e.into_inner())
}

/// Nowa sesja dla `user` (pusty = token) i ciasteczko z jej identyfikatorem.
fn start_session(app: &App, req: &Request, user: &str) -> Response {
    let id = match random_id() {
        Ok(id) => id,
        Err(e) => {
            eprintln!("login: {e}");
            return Response::error(500, "brak źródła losowości dla sesji");
        }
    };
    accounts(app).session_new(&id, user, unix_now());
    Response::json(200, "{\"ok\":true}").with_header(
        "Set-Cookie",
        session_cookie(req, &id, SESSION_IDLE_SECS as u32),
    )
}

fn failed_login(app: &App, req: &Request, what: &str, msg: &str) -> Response {
    login_limited(app, req.peer, true);
    match req.peer {
        Some(ip) => eprintln!("login: {what} ({ip})"),
        None => eprintln!("login: {what}"),
    }
    std::thread::sleep(std::time::Duration::from_millis(400));
    Response::error(401, msg)
}

fn body_json(req: &Request) -> Option<json::Value> {
    std::str::from_utf8(&req.body)
        .ok()
        .and_then(|t| json::parse(t).ok())
}

fn field<'a>(v: &'a json::Value, k: &str) -> &'a str {
    v.get(k).and_then(|x| x.str()).unwrap_or("")
}

/// `GET /api/auth` (bez logowania): jak się logować i kto jest zalogowany.
pub(crate) fn auth_state(app: &App, req: &Request) -> Response {
    let who = authorized(app, req);
    let (enabled, setup, broken) = {
        let a = accounts(app);
        (a.enabled(), a.needs_setup(), a.broken().map(json_str))
    };
    let user = match who {
        Some(w) if auth_required(app) => format!(
            "{{\"name\":{},\"role\":\"{}\"}}",
            json_str(&w.user),
            w.role.as_str()
        ),
        _ => "null".into(),
    };
    Response::json(
        200,
        format!(
            "{{\"ok\":true,\"result\":{{\"required\":{},\"accounts\":{enabled},\"setup\":{setup},\"token\":{},\"broken\":{},\"user\":{user}}}}}",
            auth_required(app),
            app.cfg.token.is_some(),
            broken.unwrap_or_else(|| "null".into())
        ),
    )
}

/// `POST /api/login`: JSON `{"user","password"}` (konto) albo `{"token"}` / sam tekst tokenu
/// w treści (token awaryjny; dawne okna, skrypty i testy).
pub(crate) fn login(app: &App, req: &Request) -> Response {
    if !auth_required(app) {
        return Response::json(200, "{\"ok\":true}");
    }
    if login_limited(app, req.peer, false) {
        return Response::error(429, "za dużo nieudanych logowań – odczekaj minutę");
    }
    let raw = String::from_utf8_lossy(&req.body).trim().to_string();
    let v = raw.starts_with('{').then(|| body_json(req)).flatten();
    let token_given = match &v {
        Some(v) if v.get("user").is_some() => None,
        Some(v) => Some(field(v, "token").to_string()),
        None => Some(raw),
    };
    if let Some(t) = token_given {
        return match &app.cfg.token {
            Some(tok) if eq_ct(&t, tok) => start_session(app, req, ""),
            _ => failed_login(app, req, "zły token", "zły token panelu"),
        };
    }
    let v = v.expect("JSON z polem user");
    let name = match check_credentials(app, req, &v) {
        Ok(n) => n,
        Err(r) => return r,
    };
    eprintln!("login: {name}");
    start_session(app, req, &name)
}

/// Login i hasło z JSON `{"user","password"}` → konto (znormalizowany login) albo odpowiedź błędu
/// (zły login i złe hasło – ta sama odpowiedź; liczy się do limitu logowań).
fn check_credentials(app: &App, req: &Request, v: &json::Value) -> Result<String, Response> {
    let Ok(name) = normalize_name(field(v, "user")) else {
        return Err(failed_login(app, req, "zły login", "zły login albo hasło"));
    };
    let password = field(v, "password");
    let stored = {
        let a = accounts(app);
        if !a.enabled() {
            return Err(Response::error(
                409,
                "konta są wyłączone – zaloguj się tokenem awaryjnym",
            ));
        }
        a.credentials(&name)
    };
    // PBKDF2 poza blokadą; nieznany login liczy skrót tak samo długo
    let ok = match &stored {
        Some(h) => h.verify(password),
        None => {
            Hashed::dummy().verify(password);
            false
        }
    };
    if !ok {
        return Err(failed_login(
            app,
            req,
            &format!("złe hasło albo login {name}"),
            "zły login albo hasło",
        ));
    }
    Ok(name)
}

fn who_json(w: &Who) -> String {
    format!(
        "{{\"name\":{},\"role\":\"{}\"}}",
        json_str(&w.user),
        w.role.as_str()
    )
}

/// Wspólne logowanie w pakiecie (`/api/sso…`): VizRS CG System (Trio, Artist) loguje kontami panelu.
/// `GET /api/sso` – stan przełącznika (bez logowania); `PUT` – przełącznik (administrator);
/// `POST /api/sso/verify {"user","password"}` – sprawdza konto i zakłada sesję panelu dla serwera
/// CG System (identyfikator w odpowiedzi; CG System sprawdza nią konto później); `GET /api/sso/session`
/// z ciasteczkiem `vizrs_live` – kto (zalogowany w panelu pod tym samym hostem wchodzi do Trio bez
/// ponownego logowania). Przełącznik wyłączony = 403 i koniec ważności sesji w CG System.
pub(crate) fn sso(app: &App, req: &Request, rest: &str) -> Response {
    let (available, enabled) = {
        let a = accounts(app);
        (a.enabled(), a.sso())
    };
    match (req.method.as_str(), rest) {
        ("GET", "") => Response::json(
            200,
            format!(
                "{{\"ok\":true,\"result\":{{\"available\":{available},\"enabled\":{enabled}}}}}"
            ),
        ),
        ("PUT", "") => {
            match authorized(app, req) {
                Some(w) if w.admin() => {}
                Some(_) => return Response::error(403, "tylko administrator"),
                None => return Response::error(401, "zaloguj się"),
            }
            let on = body_json(req).and_then(|v| v.get("enabled").cloned())
                == Some(json::Value::Bool(true));
            match accounts(app).set_sso(on) {
                Ok(()) => {
                    eprintln!(
                        "konta: wspólne logowanie w pakiecie {}",
                        if on { "włączone" } else { "wyłączone" }
                    );
                    Response::json(200, "{\"ok\":true}")
                }
                Err(e) => Response::error(409, &e),
            }
        }
        _ if !enabled => {
            Response::error(403, "wspólne logowanie w pakiecie jest wyłączone w panelu")
        }
        ("POST", "verify") => {
            if login_limited(app, req.peer, false) {
                return Response::error(429, "za dużo nieudanych logowań – odczekaj minutę");
            }
            let Some(v) = body_json(req) else {
                return Response::error(400, "oczekiwano JSON {user, password}");
            };
            let name = match check_credentials(app, req, &v) {
                Ok(n) => n,
                Err(r) => return r,
            };
            let id = match random_id() {
                Ok(id) => id,
                Err(e) => return Response::error(500, &e),
            };
            let mut a = accounts(app);
            a.session_new(&id, &name, unix_now());
            let who = a.who(&name).unwrap_or_else(Who::token);
            drop(a);
            eprintln!("login: {name} (VizRS CG System)");
            Response::json(
                200,
                format!(
                    "{{\"ok\":true,\"result\":{{\"user\":{},\"session\":\"{id}\"}}}}",
                    who_json(&who)
                ),
            )
        }
        ("GET", "session") => {
            let who = req
                .cookie(COOKIE)
                .and_then(|id| accounts(app).session_check(id, unix_now()));
            match who {
                Some(w) if !w.user.is_empty() => Response::json(
                    200,
                    format!("{{\"ok\":true,\"result\":{{\"user\":{}}}}}", who_json(&w)),
                ),
                _ => Response::error(401, "brak sesji konta panelu"),
            }
        }
        _ => Response::error(404, "nieznany adres wspólnego logowania"),
    }
}

/// `POST /api/setup {"code","user","password"}` – pierwsze konto administratora. `code` = kod
/// z logu panelu albo token awaryjny. Działa tylko, dopóki nie ma żadnego konta.
pub(crate) fn setup(app: &App, req: &Request) -> Response {
    if login_limited(app, req.peer, false) {
        return Response::error(429, "za dużo nieudanych prób – odczekaj minutę");
    }
    let Some(v) = body_json(req) else {
        return Response::error(400, "oczekiwano JSON {code, user, password}");
    };
    let code = field(&v, "code");
    let code_ok = {
        let a = accounts(app);
        if !a.needs_setup() {
            return Response::error(409, "pierwsze konto już jest – zaloguj się");
        }
        a.setup_code_ok(code) || app.cfg.token.as_deref().is_some_and(|t| eq_ct(code, t))
    };
    if !code_ok {
        return failed_login(
            app,
            req,
            "zły kod pierwszej konfiguracji",
            "zły kod – kod jest w logu panelu (Portainer: kontener live → Logs)",
        );
    }
    let name = match normalize_name(field(&v, "user")) {
        Ok(n) => n,
        Err(e) => return Response::error(400, &e),
    };
    let password = field(&v, "password");
    if let Err(e) = check_password(password) {
        return Response::error(400, &e);
    }
    let pw = match Hashed::new(password) {
        Ok(h) => h,
        Err(e) => return Response::error(500, &e),
    };
    {
        let mut a = accounts(app);
        if !a.needs_setup() {
            return Response::error(409, "pierwsze konto już jest – zaloguj się");
        }
        if let Err(e) = a.add_user(&name, Role::Admin, pw, unix_now()) {
            return Response::error(500, &e);
        }
    }
    eprintln!("konta: pierwszy administrator {name}");
    start_session(app, req, &name)
}

pub(crate) fn logout(app: &App, req: &Request) -> Response {
    if let Some(id) = req.cookie(COOKIE) {
        accounts(app).session_end(id, unix_now());
    }
    Response::json(200, "{\"ok\":true}").with_header("Set-Cookie", session_cookie(req, "", 0))
}

/// `POST /api/password {"old","new"}` – własne hasło; inne sesje konta są wylogowane.
pub(crate) fn change_password(app: &App, req: &Request, who: &Who) -> Response {
    if who.user.is_empty() {
        return Response::error(400, "zalogowano tokenem – hasło zmienia się na koncie");
    }
    let Some(v) = body_json(req) else {
        return Response::error(400, "oczekiwano JSON {old, new}");
    };
    if let Err(e) = check_password(field(&v, "new")) {
        return Response::error(400, &e);
    }
    let stored = accounts(app).credentials(&who.user);
    if !stored.is_some_and(|h| h.verify(field(&v, "old"))) {
        std::thread::sleep(std::time::Duration::from_millis(400));
        return Response::error(403, "obecne hasło się nie zgadza");
    }
    let pw = match Hashed::new(field(&v, "new")) {
        Ok(h) => h,
        Err(e) => return Response::error(500, &e),
    };
    let mut a = accounts(app);
    if let Err(e) = a.set_password(&who.user, pw) {
        return Response::error(500, &e);
    }
    a.end_user_sessions(&who.user, req.cookie(COOKIE), unix_now());
    eprintln!("konta: {} zmienił hasło", who.user);
    Response::json(200, "{\"ok\":true}")
}

/// `/api/users…` – konta (tylko administrator): lista, dodanie, rola i hasło, usunięcie, wylogowanie.
pub(crate) fn users(app: &App, req: &Request, who: &Who, rest: &str) -> Response {
    if !who.admin() {
        return Response::error(403, "tylko administrator zarządza kontami");
    }
    let now = unix_now();
    let actor = if who.user.is_empty() {
        "token"
    } else {
        &who.user
    };
    let (target, action) = match rest.split_once('/') {
        Some((n, a)) => (n, a),
        None => (rest, ""),
    };
    let target = if target.is_empty() {
        String::new()
    } else {
        match normalize_name(&crate::proxy::url_decode(target)) {
            Ok(n) => n,
            Err(e) => return Response::error(400, &e),
        }
    };
    let v = body_json(req);
    let role_of = |v: &Option<json::Value>| -> Result<Option<Role>, Response> {
        match v.as_ref().and_then(|v| v.get("role")).and_then(|r| r.str()) {
            None => Ok(None),
            Some(r) => Role::parse(r)
                .map(Some)
                .ok_or_else(|| Response::error(400, "rola: admin albo operator")),
        }
    };
    let password_of = |v: &Option<json::Value>| -> Result<Option<Hashed>, Response> {
        match v
            .as_ref()
            .and_then(|v| v.get("password"))
            .and_then(|r| r.str())
        {
            None | Some("") => Ok(None),
            Some(p) => {
                check_password(p).map_err(|e| Response::error(400, &e))?;
                Hashed::new(p)
                    .map(Some)
                    .map_err(|e| Response::error(500, &e))
            }
        }
    };
    let done = |r: Result<(), String>, what: String| match r {
        Ok(()) => {
            eprintln!("konta: {actor}: {what}");
            Response::json(200, "{\"ok\":true}")
        }
        Err(e) => Response::error(409, &e),
    };
    match (req.method.as_str(), target.is_empty(), action) {
        ("GET", true, "") => {
            let a = accounts(app);
            if !a.enabled() {
                return Response::error(409, "konta są wyłączone (brak katalogu danych)");
            }
            Response::json(
                200,
                format!("{{\"ok\":true,\"result\":{}}}", a.list_json(now)),
            )
        }
        ("POST", true, "") => {
            let Some(name) = v.as_ref().map(|v| field(v, "user")) else {
                return Response::error(400, "oczekiwano JSON {user, password, role}");
            };
            let name = match normalize_name(name) {
                Ok(n) => n,
                Err(e) => return Response::error(400, &e),
            };
            let role = match role_of(&v) {
                Ok(r) => r.unwrap_or(Role::Operator),
                Err(r) => return r,
            };
            let pw = match password_of(&v) {
                Ok(Some(p)) => p,
                Ok(None) => return Response::error(400, "podaj hasło nowego konta"),
                Err(r) => return r,
            };
            let r = accounts(app).add_user(&name, role, pw, now);
            done(r, format!("nowe konto {name} ({})", role.as_str()))
        }
        ("PUT", false, "") => {
            let role = match role_of(&v) {
                Ok(r) => r,
                Err(r) => return r,
            };
            let pw = match password_of(&v) {
                Ok(p) => p,
                Err(r) => return r,
            };
            let mut a = accounts(app);
            if let Some(role) = role {
                if let Err(e) = a.set_role(&target, role) {
                    return Response::error(409, &e);
                }
            }
            if let Some(pw) = pw {
                if let Err(e) = a.set_password(&target, pw) {
                    return Response::error(409, &e);
                }
                // nowe hasło od administratora: stare sesje konta (poza własną) wylogowane
                let keep = (target == who.user).then(|| req.cookie(COOKIE)).flatten();
                a.end_user_sessions(&target, keep, now);
            }
            drop(a);
            done(Ok(()), format!("zmiana konta {target}"))
        }
        ("DELETE", false, "") => {
            let r = accounts(app).delete(&target, now);
            done(r, format!("usunięte konto {target}"))
        }
        ("POST", false, "logout") => {
            let n = accounts(app).end_user_sessions(&target, None, now);
            done(Ok(()), format!("wylogowane sesje konta {target}: {n}"))
        }
        _ => Response::error(404, "nieznany adres kont"),
    }
}

/// `POST /api/session/{kanał}/{start|stop|restart}` – emisja kanału w legacy (Na antenie).
/// Stop: most (`StopChannel`, jak przycisk Stop legacy). Start: to samo żądanie, którym widz
/// uruchamia kanał (`/iptv/channel/{kanał}.m3u8`, segmenter). Restart: stop, potem start.
pub(crate) fn session(app: &App, rest: &str) -> Response {
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
