//! Pośrednik do ErsatzTV i VizRS CG Playout: odpowiedzi mostu, strumienie i playlisty, podgląd, grafiki, API grafiki (cg) i miniatury.

use super::*;

pub(crate) fn forward(r: Result<bridge::Reply, String>, what: &str) -> Response {
    match r {
        Ok(rep) if rep.body.first() == Some(&b'{') => Response::json(rep.status, rep.body),
        Ok(rep) => Response::error(
            502,
            &format!("most: nieoczekiwana odpowiedź {} dla {what}", rep.status),
        ),
        Err(e) => Response::error(502, &e),
    }
}

/// Numer kanału ErsatzTV w adresie strumienia: cyfry z kropkami (`24`, `1.1`), bez `..`.
pub(crate) fn channel_number(s: &str) -> bool {
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
pub(crate) const PLAY_SESSION: &str = ".troubleshooting";

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
pub(crate) fn play(app: &App, rest: &str) -> Response {
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
pub(crate) fn image_type(b: &[u8]) -> &'static str {
    match b {
        [0x89, b'P', b'N', b'G', ..] => "image/png",
        [b'G', b'I', b'F', ..] => "image/gif",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "image/webp",
        _ => "image/jpeg",
    }
}

pub(crate) fn artwork(app: &App, rest: &str) -> Response {
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
pub(crate) fn stream_proxy(app: &App, rest: &str) -> Response {
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
pub(crate) fn cg_segment(s: &str) -> bool {
    (1..=120).contains(&s.len())
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._".contains(c))
        && s != "."
        && s != ".."
}

/// Dzień `RRRR-MM-DD` (dziennik emisji, K14).
pub(crate) fn valid_day(s: &str) -> bool {
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
        // ciasteczka YouTube dla yt-dlp (wejścia YouTube na serwerze, od którego YouTube chce logowania)
        ("GET" | "PUT" | "DELETE", ["live", "youtube-cookies"]) => {
            "/api/live/youtube-cookies".into()
        }
        ("GET", ["live", "shows"]) | ("POST", ["live", "shows"]) => "/api/live/shows".into(),
        ("POST", ["live", "shows", "scan"]) => "/api/live/shows/scan".into(),
        ("PUT" | "DELETE", ["live", "shows", id]) => format!("/api/live/shows/{id}"),
        ("PUT", ["live", "channels", ch]) => format!("/api/live/channels/{ch}"),
        ("POST", ["live", "channels", ch, a @ ("hold" | "release" | "rebuild")]) => {
            format!("/api/live/channels/{ch}/{a}")
        }
        // K23: okna nagłej emisji kanału (lista, nowe, usunięcie) – granica danych w CG Playout (src/windows.rs)
        ("GET" | "POST", ["live", "channels", ch, "windows"]) => {
            format!("/api/live/channels/{ch}/windows")
        }
        ("DELETE", ["live", "channels", ch, "windows", id]) => {
            format!("/api/live/channels/{ch}/windows/{id}")
        }
        // K13: ręczne TAKE / CONTINUE / OUT pojedynczej strony na antenie kanału (kontrakt CG – DOKUMENTACJA §11)
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

/// Dekodowanie `%XX` (jak `decodeURIComponent`; zły ciąg zostaje dosłownie, złe UTF-8 → U+FFFD).
pub fn url_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
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
    String::from_utf8_lossy(&out).into_owned()
}

/// Podpis w dzienniku CG Playout: konto (gdy zalogowano kontem) i stanowisko z nagłówka `X-Operator`
/// przeglądarki (zakodowane `encodeURIComponent`, bo fetch nie wysyła polskich liter w nagłówku),
/// np. „VizRS Live · jan (Studio 1)”; bez znaków sterujących, do 40 znaków.
pub fn operator_for(user: &str, station: Option<&str>) -> String {
    let st = url_decode(station.unwrap_or(""));
    let st = st.trim();
    match (user.is_empty(), st.is_empty()) {
        (true, _) => operator_label(st),
        (false, true) => operator_label(user),
        (false, false) if st.eq_ignore_ascii_case(user) => operator_label(user),
        (false, false) => operator_label(&format!("{user} ({st})")),
    }
}

fn operator_label(raw: &str) -> String {
    let name: String = raw.chars().filter(|c| !c.is_control()).take(40).collect();
    match name.trim() {
        "" => "VizRS Live".into(),
        n => format!("VizRS Live · {n}"),
    }
}

/// Pośrednik `/api/cg/…` → VizRS CG Playout (R9): token CG zostaje w serwerze,
/// odpowiedź w kopercie panelu `{"ok":true,"result":…}` albo `{"ok":false,"error":…}`.
pub(crate) fn cg_proxy(
    app: &App,
    method: &str,
    rest: &str,
    body: &[u8],
    operator: &str,
) -> Response {
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

/// Zatwierdzenia zdarzeń wtórnych (K16) w regułach `secondary` dokumentu `PUT live/graphics` od operatora
/// (nie administratora): każde `approved: {by, at}` musi już być w CG Playout przy regule o tej samej treści
/// (bez nazwy i zatwierdzeń) – operator nie zatwierdza ani nie przenosi zatwierdzenia na inną treść. Cofnąć
/// zatwierdzenie (`null`) i usunąć regułę może. `Err` – komunikat odmowy.
pub fn approvals_kept(old: &[json::Value], new: &[json::Value]) -> Result<(), String> {
    fn strip(v: &json::Value) -> json::Value {
        match v {
            json::Value::Obj(m) => json::Value::Obj(
                m.iter()
                    .filter(|(k, _)| k != "approved" && k != "name")
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            if k == "events" {
                                json::Value::Arr(v.arr().iter().map(strip).collect())
                            } else {
                                v.clone()
                            },
                        )
                    })
                    .collect(),
            ),
            other => other.clone(),
        }
    }
    let approved = |v: &json::Value| {
        v.get("approved")
            .filter(|a| matches!(a, json::Value::Obj(_)))
            .map(json::Value::canon)
    };
    for rule in new {
        let key = strip(rule).canon();
        let twin = old.iter().find(|o| strip(o).canon() == key);
        let events = rule.get("events").map(json::Value::arr).unwrap_or(&[]);
        let mut wanted = vec![(approved(rule), twin.and_then(&approved))];
        for (i, e) in events.iter().enumerate() {
            let before = twin
                .and_then(|t| t.get("events"))
                .and_then(|ev| ev.arr().get(i))
                .and_then(&approved);
            wanted.push((approved(e), before));
        }
        if wanted
            .iter()
            .any(|(now, before)| now.is_some() && now != before)
        {
            let name = rule
                .get("name")
                .and_then(json::Value::str)
                .filter(|n| !n.is_empty())
                .unwrap_or("bez nazwy");
            return Err(format!(
                "zatwierdza tylko administrator panelu (reguła „{name}”)"
            ));
        }
    }
    Ok(())
}

/// Strażnik K16 przed `PUT live/graphics`: dla operatora porównuje zatwierdzenia z bieżącymi w CG Playout.
pub(crate) fn guard_approvals(app: &App, body: &[u8]) -> Option<Response> {
    let new = json::parse(std::str::from_utf8(body).ok()?).ok()?;
    let new_list = new.get("secondary").map(json::Value::arr).unwrap_or(&[]);
    let has_approval = |list: &[json::Value]| {
        list.iter().any(|r| {
            matches!(r.get("approved"), Some(json::Value::Obj(_)))
                || r.get("events")
                    .map(json::Value::arr)
                    .unwrap_or(&[])
                    .iter()
                    .any(|e| matches!(e.get("approved"), Some(json::Value::Obj(_))))
        })
    };
    if !has_approval(new_list) {
        return None;
    }
    let cg = app.cg.as_ref()?;
    let old = match cg.send_as("GET", "/api/live/graphics", &[], "VizRS Live") {
        Ok(r) if r.status == 200 => json::parse(&String::from_utf8_lossy(&r.body)).ok(),
        _ => None,
    };
    let Some(old) = old else {
        return Some(Response::error(
            502,
            "CG Playout: nie mogę sprawdzić zatwierdzeń (GET /api/live/graphics)",
        ));
    };
    let old_list = old
        .get("rules")
        .and_then(|r| r.get("secondary"))
        .map(json::Value::arr)
        .unwrap_or(&[]);
    approvals_kept(old_list, new_list)
        .err()
        .map(|e| Response::error(403, &e))
}

/// Numer strony z `<numer>.png` (miniatura strony VizRS); tylko cyfry, jak numery stron VizRS.
pub fn thumb_page(file: &str) -> Option<&str> {
    let n = file.strip_suffix(".png")?;
    (!n.is_empty() && n.len() <= 12 && n.bytes().all(|b| b.is_ascii_digit())).then_some(n)
}

/// `GET /api/cg/live/thumbs/<numer>.png` → miniatura strony VizRS z CG Playout (`/api/live/thumbs`),
/// obraz wprost (nie koperta JSON); 404 – CG Playout nie ma miniatury (panel pokazuje wtedy napis).
pub(crate) fn cg_thumb(app: &App, file: &str) -> Response {
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
