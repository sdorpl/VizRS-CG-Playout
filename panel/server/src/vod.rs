//! Odtwarzanie pozycji z Przeglądarki bez legacy i bez transkodowania (`/api/vod/…`).
//!
//! `POST /api/vod/open` `{"source":"jellyfin"|"local","title":…,"path":…}` – serwer sprawdza źródło i zwraca krótki
//! identyfikator; `GET /api/vod/{id}` z nagłówkiem `Range` – kawałek pliku (≤ [`CHUNK`], 206). Przeglądarka gra plik
//! sama (`<video>`), z przewijaniem po całym materiale.
//! - Jellyfin: adres i klucz z legacy (`GetJellyfinSecrets` przez most) – zostają w serwerze; pozycję w Jellyfin
//!   panel znajduje po ścieżce, którą podaje legacy (`/Items?SearchTerm=…&Fields=Path`), plik bierze z
//!   `/Videos/{id}/stream?static=true` (to samo, co legacy podaje swojemu ffmpeg – bez transkodowania).
//! - plik lokalny: media zamontowane w kontenerze panelu tak jak w legacy (`/media`); tylko pliki wewnątrz ścieżek
//!   bibliotek lokalnych legacy (`GetLocalLibraryPaths`), po `canonicalize` (bez `..` i dowiązań poza bibliotekę).

use std::collections::VecDeque;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::bridge::{self, Bridge, Target};
use crate::http::{json_str, Request, Response};
use crate::json::{self, Value};

/// Największy kawałek odpowiedzi (przeglądarka sama dopyta o następny zakres).
pub const CHUNK: u64 = 8 * 1024 * 1024;
/// Pierwszy kawałek od początku pliku – mniejszy, żeby obraz ruszył szybciej (nagłówek MP4 jest na początku).
const FIRST: u64 = 2 * 1024 * 1024;

fn chunk_end(start: u64, want_end: Option<u64>) -> u64 {
    let size = if start == 0 { FIRST } else { CHUNK };
    want_end.unwrap_or(u64::MAX).min(start + size - 1)
}
const MAX_ITEMS: usize = 256;
const CREDS_TTL: Duration = Duration::from_secs(300);

#[derive(Clone, Debug, PartialEq)]
enum Source {
    Jellyfin(String),
    Local(PathBuf),
}

struct Vod {
    next: u64,
    items: VecDeque<(String, Source)>,
    /// adres Jellyfin, klucz, kiedy pobrane z legacy
    creds: Option<(Target, String, Instant)>,
}

static VOD: Mutex<Vod> = Mutex::new(Vod {
    next: 0,
    items: VecDeque::new(),
    creds: None,
});

fn state() -> std::sync::MutexGuard<'static, Vod> {
    VOD.lock().unwrap_or_else(|e| e.into_inner())
}

fn remember(src: Source) -> String {
    let mut v = state();
    if let Some((k, _)) = v.items.iter().find(|(_, s)| *s == src) {
        return k.clone();
    }
    v.next += 1;
    let key = format!("{:x}", v.next);
    v.items.push_back((key.clone(), src));
    while v.items.len() > MAX_ITEMS {
        v.items.pop_front();
    }
    key
}

fn lookup(key: &str) -> Option<Source> {
    state()
        .items
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, s)| s.clone())
}

/// Wynik komendy legacy przez most (`result`) albo komunikat.
fn command(b: &Bridge, name: &str, args: &str) -> Result<Value, String> {
    let r = b.post(&format!("/command/{name}"), args.as_bytes())?;
    let v = json::parse(&String::from_utf8_lossy(&r.body))
        .map_err(|e| format!("legacy ({name}): {e}"))?;
    if v.get("ok") != Some(&Value::Bool(true)) {
        return Err(format!(
            "legacy ({name}): {}",
            v.get("error").and_then(Value::str).unwrap_or("odmowa")
        ));
    }
    Ok(v.get("result").cloned().unwrap_or(Value::Null))
}

/// Adres i klucz Jellyfin z legacy (pamiętane 5 min; `fresh` – pobierz od nowa, np. po 401).
fn creds(b: &Bridge, fresh: bool) -> Result<(Target, String), String> {
    if !fresh {
        if let Some((t, k, at)) = &state().creds {
            if at.elapsed() < CREDS_TTL {
                return Ok((t.clone(), k.clone()));
            }
        }
    }
    let r = command(b, "GetJellyfinSecrets", "{}")?;
    let addr = r
        .get("Address")
        .and_then(Value::str)
        .unwrap_or("")
        .trim()
        .trim_end_matches('/');
    let key = r.get("ApiKey").and_then(Value::str).unwrap_or("").trim();
    if addr.is_empty() || key.is_empty() {
        return Err("legacy nie ma połączenia z Jellyfin (Źródła zewnętrzne)".into());
    }
    if !key.bytes().all(|c| c.is_ascii_graphic()) {
        return Err("klucz Jellyfin w legacy ma niedozwolone znaki".into());
    }
    let t = bridge::parse_url(addr).map_err(|_| "Jellyfin pod https:// – serwer panelu nie obsługuje TLS (odtwarzanie przez legacy działa)".to_string())?;
    state().creds = Some((t.clone(), key.to_string(), Instant::now()));
    Ok((t, key.to_string()))
}

fn auth(key: &str) -> String {
    format!("Authorization: MediaBrowser Client=\"VizRS Live\", Token=\"{key}\"\r\nX-Emby-Token: {key}\r\n")
}

/// Kodowanie wartości w zapytaniu URL.
pub fn pct(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn file_name(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn valid_item_id(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Pozycja Jellyfin o tej ścieżce wśród wyników wyszukiwania: dokładnie ta ścieżka, a gdy jej nie ma –
/// jedyna o tej samej nazwie pliku (legacy może mieć ścieżkę po zamianach ścieżek).
pub fn pick_item(items: &[Value], path: &str) -> Option<String> {
    let id = |it: &Value| {
        it.get("Id")
            .and_then(Value::str)
            .filter(|i| valid_item_id(i))
            .map(str::to_string)
    };
    if let Some(it) = items
        .iter()
        .find(|it| it.get("Path").and_then(Value::str) == Some(path))
    {
        return id(it);
    }
    let name = file_name(path);
    let same: Vec<_> = items
        .iter()
        .filter(|it| it.get("Path").and_then(Value::str).map(file_name) == Some(name))
        .collect();
    (same.len() == 1).then(|| id(same[0])).flatten()
}

fn find_jellyfin(b: &Bridge, title: &str, path: &str) -> Result<String, String> {
    let (t, key) = creds(b, false)?;
    let stem = file_name(path)
        .rsplit_once('.')
        .map_or(file_name(path), |(s, _)| s);
    let mut terms = vec![title.trim(), stem];
    terms.retain(|s| !s.is_empty());
    terms.dedup();
    for term in terms {
        let q = format!("/Items?Recursive=true&Fields=Path&Limit=200&IncludeItemTypes=Movie,Episode,Video,MusicVideo&SearchTerm={}", pct(term));
        let r = bridge::fetch(&t, &q, &auth(&key), 4 * 1024 * 1024)?;
        if r.status == 401 {
            state().creds = None;
            return Err("Jellyfin odrzucił klucz z legacy (401)".into());
        }
        if r.status != 200 {
            return Err(format!(
                "Jellyfin: odpowiedź {} przy wyszukiwaniu",
                r.status
            ));
        }
        let v =
            json::parse(&String::from_utf8_lossy(&r.body)).map_err(|e| format!("Jellyfin: {e}"))?;
        if let Some(id) = pick_item(v.get("Items").map_or(&[], Value::arr), path) {
            return Ok(id);
        }
    }
    Err("Jellyfin nie zna pozycji o tej ścieżce (biblioteka legacy nieaktualna? – synchronizuj Jellyfin w Źródłach zewnętrznych)".into())
}

/// Ścieżki bibliotek lokalnych legacy.
fn library_paths(b: &Bridge) -> Result<Vec<PathBuf>, String> {
    let libs = command(b, "GetAllLocalLibraries", "{}")?;
    let mut out = Vec::new();
    for l in libs.arr() {
        let Some(id) = l.get("Id").and_then(Value::num) else {
            continue;
        };
        let paths = command(
            b,
            "GetLocalLibraryPaths",
            &format!("{{\"LocalLibraryId\":{}}}", id as i64),
        )?;
        out.extend(
            paths
                .arr()
                .iter()
                .filter_map(|p| p.get("Path").and_then(Value::str))
                .map(PathBuf::from),
        );
    }
    Ok(out)
}

/// Plik lokalny pozycji: ścieżka bezwzględna albo względna do biblioteki (karta „Inne”); tylko wewnątrz bibliotek.
pub fn resolve_local(path: &str, libs: &[PathBuf]) -> Result<PathBuf, String> {
    if path.is_empty() || path.contains('\0') {
        return Err("pusta ścieżka".into());
    }
    let roots: Vec<PathBuf> = libs.iter().filter_map(|l| l.canonicalize().ok()).collect();
    if roots.is_empty() {
        return Err("biblioteki lokalne legacy nie są widoczne w kontenerze panelu – zamontuj media jak w legacy (/media, RUNBOOK)".into());
    }
    let candidates: Vec<PathBuf> = if Path::new(path).is_absolute() {
        vec![PathBuf::from(path)]
    } else {
        libs.iter().map(|l| l.join(path)).collect()
    };
    for c in candidates {
        let Ok(real) = c.canonicalize() else { continue };
        if real.is_file() && roots.iter().any(|r| real.starts_with(r)) {
            return Ok(real);
        }
    }
    Err("pliku nie ma w bibliotece lokalnej widocznej dla panelu (media zamontowane tak jak w legacy?)".into())
}

/// `POST /api/vod/open`.
pub fn open(b: &Bridge, body: &[u8]) -> Response {
    let Ok(v) = json::parse(&String::from_utf8_lossy(body)) else {
        return Response::error(400, "zła treść (JSON: source, title, path)");
    };
    let s = |k: &str| v.get(k).and_then(Value::str).unwrap_or("").to_string();
    let (source, title, path) = (s("source"), s("title"), s("path"));
    if path.is_empty() || path.len() > 4096 || title.len() > 1024 {
        return Response::error(400, "brak ścieżki pozycji");
    }
    let src = match source.as_str() {
        "jellyfin" => find_jellyfin(b, &title, &path).map(Source::Jellyfin),
        "local" => library_paths(b)
            .and_then(|libs| resolve_local(&path, &libs))
            .map(Source::Local),
        _ => return Response::error(400, "źródło: jellyfin albo local"),
    };
    match src {
        Ok(src) => {
            let key = remember(src);
            Response::json(
                200,
                format!(
                    "{{\"ok\":true,\"result\":{{\"id\":{},\"url\":{}}}}}",
                    json_str(&key),
                    json_str(&format!("/api/vod/{key}"))
                ),
            )
        }
        Err(e) => Response::error(404, &e),
    }
}

/// `Range: bytes=a-[b]` → (a, b); brak albo inny zapis – od początku.
pub fn parse_range(h: Option<&str>) -> (u64, Option<u64>) {
    let Some(r) = h.and_then(|h| h.trim().strip_prefix("bytes=")) else {
        return (0, None);
    };
    let r = r.split(',').next().unwrap_or("");
    match r.split_once('-') {
        Some((a, b)) => match a.trim().parse::<u64>() {
            Ok(a) => (a, b.trim().parse::<u64>().ok().filter(|b| *b >= a)),
            Err(_) => (0, None), // zakres od końca (bytes=-N) – przeglądarki go przy wideo nie używają
        },
        None => (0, None),
    }
}

/// Typ treści po rozszerzeniu albo typie z Jellyfin (stałe – nagłówek `X-Content-Type-Options: nosniff`).
pub fn media_type(name: &str) -> &'static str {
    let n = name.to_ascii_lowercase();
    let ext = n.rsplit(['.', '/']).next().unwrap_or("");
    match ext {
        "mp4" | "m4v" | "mov" => "video/mp4",
        // Matroska: przeglądarki z demuxerem WebM grają też mkv (H.264/VP9 + AAC/Opus)
        "webm" | "mkv" | "x-matroska" => "video/webm",
        "ts" | "mp2t" | "m2ts" => "video/mp2t",
        "ogv" | "ogg" => "video/ogg",
        "mp3" | "mpeg" => "audio/mpeg",
        "m4a" | "aac" => "audio/mp4",
        "flac" => "audio/flac",
        _ => "application/octet-stream",
    }
}

fn partial(ct: &'static str, body: Vec<u8>, start: u64, total: Option<u64>) -> Response {
    let end = start + body.len().max(1) as u64 - 1;
    let total = total.map_or("*".to_string(), |t| t.to_string());
    Response::new(206, ct, body)
        .with_header("Content-Range", format!("bytes {start}-{end}/{total}"))
        .with_header("Accept-Ranges", "bytes".into())
}

/// `GET /api/vod/{id}`.
pub fn serve(b: &Bridge, key: &str, req: &Request) -> Response {
    let Some(src) = lookup(key) else {
        return Response::error(
            404,
            "nieznane odtwarzanie – otwórz kartę pozycji jeszcze raz",
        );
    };
    let (start, want_end) = parse_range(req.header("range"));
    match src {
        Source::Local(p) => {
            let Ok(mut f) = std::fs::File::open(&p) else {
                return Response::error(404, "plik zniknął z biblioteki");
            };
            let size = f.metadata().map(|m| m.len()).unwrap_or(0);
            if start >= size {
                return Response::error(416, "zakres poza plikiem")
                    .with_header("Content-Range", format!("bytes */{size}"));
            }
            let end = want_end
                .unwrap_or(u64::MAX)
                .min(start + CHUNK - 1)
                .min(size - 1);
            let mut buf = vec![0u8; (end - start + 1) as usize];
            if f.seek(SeekFrom::Start(start))
                .and_then(|_| f.read_exact(&mut buf))
                .is_err()
            {
                return Response::error(500, "odczyt pliku nie powiódł się");
            }
            partial(media_type(&p.to_string_lossy()), buf, start, Some(size))
        }
        Source::Jellyfin(item) => {
            let get = |fresh: bool| -> Result<bridge::Reply, String> {
                let (t, k) = creds(b, fresh)?;
                let end = chunk_end(start, want_end);
                bridge::fetch(
                    &t,
                    &format!("/Videos/{item}/stream?static=true"),
                    &format!("{}Range: bytes={start}-{end}\r\n", auth(&k)),
                    CHUNK,
                )
            };
            let r = match get(false) {
                Ok(r) if r.status == 401 => get(true),
                r => r,
            };
            let r = match r {
                Ok(r) => r,
                Err(e) => return Response::error(502, &e),
            };
            let ct = media_type(r.header("content-type").unwrap_or(""));
            match r.status {
                206 => {
                    let cr = r.header("content-range").map(str::to_string);
                    let mut resp =
                        Response::new(206, ct, r.body).with_header("Accept-Ranges", "bytes".into());
                    if let Some(cr) = cr {
                        resp = resp.with_header("Content-Range", cr);
                    }
                    resp
                }
                // serwer bez obsługi Range: tylko od początku, pierwszy kawałek
                200 if start == 0 => {
                    let total = r.header("content-length").and_then(|n| n.parse().ok());
                    partial(ct, r.body, 0, total)
                }
                200 => Response::error(
                    502,
                    "Jellyfin nie obsługuje zakresów (Range) – przewijanie niemożliwe",
                ),
                416 => Response::error(416, "zakres poza plikiem"),
                401 => Response::error(502, "Jellyfin odrzucił klucz z legacy (401)"),
                404 => Response::error(404, "Jellyfin nie ma już tej pozycji"),
                s => Response::error(502, &format!("Jellyfin: odpowiedź {s} dla pliku")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_types_pct() {
        assert_eq!(parse_range(Some("bytes=0-")), (0, None));
        assert_eq!(parse_range(Some("bytes=100-199")), (100, Some(199)));
        assert_eq!(parse_range(Some("bytes=200-100")), (200, None));
        assert_eq!(parse_range(Some("bytes=-500")), (0, None));
        assert_eq!(parse_range(None), (0, None));
        assert_eq!(chunk_end(0, None), FIRST - 1);
        assert_eq!(chunk_end(10, None), 10 + CHUNK - 1);
        assert_eq!(chunk_end(10, Some(20)), 20);
        assert_eq!(media_type("/media/a/Odcinek 1.MP4"), "video/mp4");
        assert_eq!(media_type("video/x-matroska"), "video/webm");
        assert_eq!(media_type("x.exe"), "application/octet-stream");
        assert_eq!(pct("Laluś 2 & co"), "Lalu%C5%9B%202%20%26%20co");
    }

    #[test]
    fn picks_jellyfin_item() {
        let v = json::parse(r#"[{"Id":"aa11","Path":"/m/A/x.mp4"},{"Id":"bb22","Path":"/m/B/y.mp4"},{"Id":"cc33","Path":"/other/y.mp4"},{"Id":"bad id","Path":"/m/z.mp4"}]"#).unwrap();
        assert_eq!(pick_item(v.arr(), "/m/B/y.mp4").as_deref(), Some("bb22"));
        assert_eq!(
            pick_item(v.arr(), "/inna/ścieżka/x.mp4").as_deref(),
            Some("aa11"),
            "jedyna o tej nazwie pliku"
        );
        assert_eq!(
            pick_item(v.arr(), "/inna/y.mp4"),
            None,
            "dwie o tej nazwie – nie zgadujemy"
        );
        assert_eq!(pick_item(v.arr(), "/m/z.mp4"), None, "zły identyfikator");
    }

    #[test]
    fn local_files_only_inside_libraries() {
        let dir = std::env::temp_dir().join(format!("vizrs-vod-{}", std::process::id()));
        let lib = dir.join("lib");
        std::fs::create_dir_all(lib.join("shows")).unwrap();
        std::fs::write(lib.join("shows/a.mp4"), b"x").unwrap();
        std::fs::write(dir.join("secret.txt"), b"x").unwrap();
        let libs = vec![lib.clone()];
        let ok = resolve_local("shows/a.mp4", &libs).unwrap();
        assert!(ok.ends_with("shows/a.mp4"));
        assert_eq!(
            resolve_local(&lib.join("shows/a.mp4").to_string_lossy(), &libs).unwrap(),
            ok
        );
        assert!(resolve_local("../secret.txt", &libs).is_err());
        assert!(resolve_local(&dir.join("secret.txt").to_string_lossy(), &libs).is_err());
        assert!(
            resolve_local("shows", &libs).is_err(),
            "katalog to nie plik"
        );
        assert!(resolve_local("shows/a.mp4", &[dir.join("nie-ma")]).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
