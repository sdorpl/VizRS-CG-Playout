//! Minimalny serwer HTTP/1.1: jedno żądanie na połączenie (`Connection: close`), limity rozmiaru i czasu.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// CSP każdej odpowiedzi (też UI z `--web` i API): bez skryptów i stylów inline, bez osadzania panelu w ramce.
/// Źródła obrazów i ramek tylko ze znanych originów (K2): ustawia je [`set_csp`] przy starcie z konfiguracji;
/// bez wywołania (testy) – najwęższa wersja: obrazy `'self' data:`, ramki `'none'`.
static CSP: std::sync::OnceLock<String> = std::sync::OnceLock::new();

pub fn csp() -> &'static str {
    CSP.get_or_init(|| build_csp(&[], &[]))
}

/// Ustala CSP raz, przy starcie (kolejne wywołania nic nie zmieniają).
pub fn set_csp(img: &[String], frame: &[String]) {
    let _ = CSP.set(build_csp(img, frame));
}

pub fn build_csp(img: &[String], frame: &[String]) -> String {
    let mut img_src = String::from("'self' data:");
    for o in img {
        img_src.push(' ');
        img_src.push_str(o);
    }
    let frame_src = if frame.is_empty() {
        "'none'".to_string()
    } else {
        frame.join(" ")
    };
    format!("default-src 'self'; img-src {img_src}; frame-src {frame_src}; connect-src 'self'; media-src 'self' blob:; style-src 'self'; script-src 'self'; object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'")
}

/// Origin adresu do CSP: `http(s)://host[:port]` (bez ścieżki); sam port `:N` (VizRS pod hostem panelu)
/// → `http://*:N https://*:N`; inny zapis – brak źródła.
pub fn csp_origin(url: &str) -> Vec<String> {
    if let Some(p) = url.strip_prefix(':') {
        return match p.parse::<u16>() {
            Ok(n) if n > 0 => vec![format!("http://*:{n}"), format!("https://*:{n}")],
            _ => vec![],
        };
    }
    for scheme in ["http://", "https://"] {
        if let Some(rest) = url.strip_prefix(scheme) {
            let host = rest.split(['/', '?', '#']).next().unwrap_or("");
            let ok = !host.is_empty()
                && host
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-.:[]".contains(c));
            return if ok {
                vec![format!("{scheme}{host}")]
            } else {
                vec![]
            };
        }
    }
    vec![]
}
pub const MAX_HEAD: usize = 16 * 1024;
pub const MAX_BODY: usize = 1024 * 1024;
/// Limit treści dla wgrywania obrazów (`/api/upload/…`).
pub const MAX_UPLOAD: usize = 10 * 1024 * 1024;
pub const UPLOAD_PREFIX: &str = "/api/upload/";

#[derive(Debug, Default)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// adres klienta (połączenie TCP; nagłówkom X-Forwarded-For nie ufamy)
    pub peer: Option<std::net::IpAddr>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn cookie(&self, name: &str) -> Option<&str> {
        self.header("cookie")?.split(';').find_map(|part| {
            let (k, v) = part.trim().split_once('=')?;
            (k == name).then_some(v)
        })
    }
}

#[derive(Debug)]
pub enum ParseError {
    Io,
    Bad(&'static str),
    TooLarge,
}

impl From<io::Error> for ParseError {
    fn from(_: io::Error) -> Self {
        ParseError::Io
    }
}

/// Czyta jedno żądanie. Nagłówki ≤ [`MAX_HEAD`], treść ≤ [`MAX_BODY`] (wgrywanie obrazów ≤ [`MAX_UPLOAD`];
/// tylko `Content-Length`, bez chunked).
pub fn read_request<R: Read>(stream: R) -> Result<Request, ParseError> {
    let mut reader = BufReader::new(stream.take((MAX_HEAD + MAX_UPLOAD) as u64));
    let mut head_len = 0usize;
    let mut line = String::new();

    let mut next_line = |reader: &mut BufReader<_>, line: &mut String| -> Result<(), ParseError> {
        line.clear();
        let n = reader.read_line(line)?;
        if n == 0 {
            return Err(ParseError::Bad("połączenie zamknięte"));
        }
        head_len += n;
        if head_len > MAX_HEAD {
            return Err(ParseError::TooLarge);
        }
        Ok(())
    };

    next_line(&mut reader, &mut line)?;
    let mut parts = line.trim_end().splitn(3, ' ');
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().ok_or(ParseError::Bad("brak ścieżki"))?;
    let version = parts.next().unwrap_or("");
    if method.is_empty() || !version.starts_with("HTTP/1.") || !target.starts_with('/') {
        return Err(ParseError::Bad("zły wiersz żądania"));
    }
    let path = target.split('?').next().unwrap_or(target).to_string(); // zapytanie (?…) panelowi niepotrzebne

    let mut headers = Vec::new();
    loop {
        next_line(&mut reader, &mut line)?;
        let l = line.trim_end_matches(['\r', '\n']);
        if l.is_empty() {
            break;
        }
        let (k, v) = l.split_once(':').ok_or(ParseError::Bad("zły nagłówek"))?;
        headers.push((k.trim().to_string(), v.trim().to_string()));
        if headers.len() > 100 {
            return Err(ParseError::TooLarge);
        }
    }

    let mut req = Request {
        method,
        path,
        headers,
        body: Vec::new(),
        peer: None,
    };
    if req.header("transfer-encoding").is_some() {
        return Err(ParseError::Bad("Transfer-Encoding nieobsługiwane"));
    }
    if let Some(len) = req.header("content-length") {
        let len: usize = len
            .parse()
            .map_err(|_| ParseError::Bad("zły Content-Length"))?;
        let limit = if req.path.starts_with(UPLOAD_PREFIX) {
            MAX_UPLOAD
        } else {
            MAX_BODY
        };
        if len > limit {
            return Err(ParseError::TooLarge);
        }
        let mut body = vec![0u8; len];
        reader.read_exact(&mut body)?;
        req.body = body;
    }
    Ok(req)
}

pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: u16, content_type: &'static str, body: impl Into<Vec<u8>>) -> Self {
        Response {
            status,
            content_type,
            headers: Vec::new(),
            body: body.into(),
        }
    }

    pub fn json(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self::new(status, "application/json; charset=utf-8", body)
    }

    pub fn error(status: u16, msg: &str) -> Self {
        Self::json(
            status,
            format!("{{\"ok\":false,\"error\":{}}}", json_str(msg)),
        )
    }

    pub fn with_header(mut self, k: &str, v: String) -> Self {
        self.headers.push((k.to_string(), v));
        self
    }

    pub fn write_to<W: Write>(&self, mut w: W) -> io::Result<()> {
        let mut head = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\nReferrer-Policy: same-origin\r\nContent-Security-Policy: {}\r\n",
            self.status,
            reason(self.status),
            self.content_type,
            self.body.len(),
            csp()
        );
        for (k, v) in &self.headers {
            head.push_str(k);
            head.push_str(": ");
            head.push_str(v);
            head.push_str("\r\n");
        }
        head.push_str("\r\n");
        w.write_all(head.as_bytes())?;
        w.write_all(&self.body)?;
        w.flush()
    }
}

pub fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        206 => "Partial Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        416 => "Range Not Satisfiable",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Status",
    }
}

/// Tekst jako literał JSON (z cudzysłowami).
pub fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '<' || c == '>' => {
                out.push_str(&format!("\\u{:04x}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn set_timeouts(s: &TcpStream, secs: u64) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(secs)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(secs)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_post_with_body() {
        let raw = b"POST /api/cmd/GetAllChannels?x=1 HTTP/1.1\r\nHost: a\r\nContent-Length: 2\r\nCookie: a=1; vizrs_live=abc\r\n\r\n{}";
        let r = read_request(&raw[..]).unwrap();
        assert_eq!(r.method, "POST");
        assert_eq!(r.path, "/api/cmd/GetAllChannels");
        assert_eq!(r.body, b"{}");
        assert_eq!(r.header("HOST"), Some("a"));
        assert_eq!(r.cookie("vizrs_live"), Some("abc"));
    }

    #[test]
    fn rejects_big_body_and_chunked() {
        let raw = format!(
            "POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY + 1
        );
        assert!(matches!(
            read_request(raw.as_bytes()),
            Err(ParseError::TooLarge)
        ));
        let raw = format!(
            "POST /api/upload/Logo HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_UPLOAD + 1
        );
        assert!(matches!(
            read_request(raw.as_bytes()),
            Err(ParseError::TooLarge)
        ));
        let mut raw = format!(
            "POST /api/upload/Logo HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY + 1
        )
        .into_bytes();
        raw.extend(vec![0u8; MAX_BODY + 1]);
        assert_eq!(read_request(&raw[..]).unwrap().body.len(), MAX_BODY + 1);
        let raw = b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n";
        assert!(matches!(read_request(&raw[..]), Err(ParseError::Bad(_))));
        assert!(matches!(
            read_request(&b"GARBAGE\r\n\r\n"[..]),
            Err(ParseError::Bad(_))
        ));
    }

    #[test]
    fn json_escapes() {
        assert_eq!(json_str("a\"b\\c\n<"), "\"a\\\"b\\\\c\\n\\u003c\"");
        assert_eq!(json_str("żółć"), "\"żółć\"");
    }
}
