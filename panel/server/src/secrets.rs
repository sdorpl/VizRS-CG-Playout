//! Maskowanie sekretów (klucze API, tokeny, hasła) w odpowiedziach ErsatzTV przed wysłaniem do przeglądarki.

use super::*;

/// Końcówki nazw pól (małymi literami), których wartości tekstowe nie wychodzą do przeglądarki (R8).
pub(crate) const SECRET_SUFFIXES: &[&str] = &["apikey", "token", "password", "secret"];

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
