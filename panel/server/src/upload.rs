//! Przesyłanie obrazów (logo, znak wodny) do ErsatzTV: rodzaje, sprawdzenie typu, profil.

use super::*;

/// Rodzaje obrazów, które panel wgrywa do legacy (`ArtworkKind`).
pub(crate) const UPLOAD_KINDS: &[&str] = &["Logo", "Watermark"];

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
pub(crate) fn profile_id(s: &str) -> bool {
    (1..=9).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit()) && !s.starts_with('0')
}

/// `POST /api/upload/{Logo|Watermark}`: obraz do pamięci obrazów legacy przez most; wynik = nazwa pliku.
pub(crate) fn upload(app: &App, req: &Request, kind: &str) -> Response {
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
