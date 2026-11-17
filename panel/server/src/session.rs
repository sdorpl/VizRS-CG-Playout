//! Sesje operatorów: identyfikatory, ciasteczko, limit nieudanych logowań, autoryzacja żądań, logowanie i wylogowanie.

use super::*;

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
    pub(crate) fn len(&self) -> usize {
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

pub(crate) fn authorized(app: &App, req: &Request) -> bool {
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
