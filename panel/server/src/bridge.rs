//! Klient mostu VizRS Bridge w procesie ErsatzTV legacy (zwykłe HTTP w sieci wewnętrznej).

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub host: String,
    pub port: u16,
    pub base: String,
}

/// `http://host:port[/ścieżka]` → host, port, ścieżka bazowa bez końcowego `/`.
pub fn parse_url(url: &str) -> Result<Target, String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("adres mostu musi zaczynać się od http:// ({url})"))?;
    let (hostport, base) = match rest.find('/') {
        Some(i) => (&rest[..i], rest[i..].trim_end_matches('/')),
        None => (rest, ""),
    };
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) => (
            h,
            p.parse::<u16>().map_err(|_| format!("zły port w {url}"))?,
        ),
        _ => (hostport, 80),
    };
    if host.is_empty() {
        return Err(format!("brak hosta w {url}"));
    }
    Ok(Target {
        host: host.to_string(),
        port,
        base: base.to_string(),
    })
}

/// Klient HTTP usługi w sieci wewnętrznej: most w legacy albo VizRS CG Playout (R9).
pub struct Bridge {
    pub target: Target,
    pub token: String,
    /// Przedrostek ścieżek: `/vizrs-bridge` (most) albo pusty (CG Playout).
    pub prefix: &'static str,
    /// Nagłówek z tokenem: `X-VizRS-Token` (most) albo `Authorization` (CG Playout, `Bearer …`).
    pub auth: &'static str,
}

pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
    /// nagłówki odpowiedzi (nazwa małymi literami, wartość)
    pub headers: Vec<(String, String)>,
}

impl Reply {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// `GET` pod dowolny adres `http://` (Jellyfin – odtwarzanie bez legacy, `vod`): `path` z zapytaniem,
/// `extra` – wiersze nagłówka zakończone `\r\n`; treść najwyżej `limit` bajtów (więcej = ucięte).
pub fn fetch(target: &Target, path: &str, extra: &str, limit: u64) -> Result<Reply, String> {
    send(
        target,
        "GET",
        &format!("{}{path}", target.base),
        b"",
        extra,
        limit,
        "serwer mediów",
        false,
    )
}

/// Jedno żądanie HTTP/1.1 (bez TLS, `Connection: close`).
#[allow(clippy::too_many_arguments)]
fn send(
    t: &Target,
    method: &str,
    full_path: &str,
    body: &[u8],
    extra: &str,
    limit: u64,
    who: &str,
    strict: bool,
) -> Result<Reply, String> {
    let addr = (t.host.as_str(), t.port)
        .to_socket_addrs()
        .map_err(|e| format!("{who} {}:{} nieosiągalny: {e}", t.host, t.port))?
        .next()
        .ok_or_else(|| format!("{who} {}: brak adresu", t.host))?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(3))
        .map_err(|e| format!("{who} {}:{} nie odpowiada: {e}", t.host, t.port))?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(60)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(10)));
    let host = if t.port == 80 {
        t.host.clone()
    } else {
        format!("{}:{}", t.host, t.port)
    };
    let head = format!(
        "{method} {full_path} HTTP/1.1\r\nHost: {host}\r\n{extra}Content-Length: {}\r\nConnection: close\r\nUser-Agent: vizrs-live/{}\r\n\r\n",
        body.len(),
        env!("CARGO_PKG_VERSION")
    );
    s.write_all(head.as_bytes())
        .and_then(|_| s.write_all(body))
        .map_err(|e| format!("{who}: zapis: {e}"))?;
    let mut raw = Vec::new();
    s.take(limit + 64 * 1024)
        .read_to_end(&mut raw)
        .map_err(|e| format!("{who}: odczyt: {e}"))?;
    if strict {
        parse_reply(&raw)
    } else {
        parse_reply_lenient(&raw)
    }
}

impl Bridge {
    pub fn get(&self, path: &str) -> Result<Reply, String> {
        self.call("GET", path, b"", "application/json")
    }

    pub fn post(&self, path: &str, body: &[u8]) -> Result<Reply, String> {
        self.call("POST", path, body, "application/json")
    }

    /// Dowolna metoda z treścią JSON (pośrednik CG Playout), z nazwą stanowiska w `X-Operator` (dziennik CG Playout).
    /// `operator` musi być już oczyszczony (`main::operator_name`).
    pub fn send_as(
        &self,
        method: &str,
        path: &str,
        body: &[u8],
        operator: &str,
    ) -> Result<Reply, String> {
        self.call_as(method, path, body, "application/json", operator)
    }

    /// POST z własnym typem treści (wgrywanie obrazu).
    pub fn post_as(&self, path: &str, body: &[u8], content_type: &str) -> Result<Reply, String> {
        self.call("POST", path, body, content_type)
    }

    fn call(&self, method: &str, path: &str, body: &[u8], ct: &str) -> Result<Reply, String> {
        self.call_as(method, path, body, ct, "VizRS Live")
    }

    /// GET zwykłej ścieżki usługi – bez przedrostka i bez tokenu (strumienie `/iptv/…` legacy,
    /// które legacy podaje każdemu widzowi; token mostu nie wychodzi poza `/vizrs-bridge`).
    pub fn get_plain(&self, path: &str) -> Result<Reply, String> {
        self.request("GET", "", path, b"", "")
    }

    fn call_as(
        &self,
        method: &str,
        path: &str,
        body: &[u8],
        ct: &str,
        operator: &str,
    ) -> Result<Reply, String> {
        let auth = format!(
            "{}: {}{}\r\nX-Operator: {operator}\r\nContent-Type: {ct}\r\n",
            self.auth,
            if self.auth == "Authorization" {
                "Bearer "
            } else {
                ""
            },
            self.token,
        );
        self.request(method, self.prefix, path, body, &auth)
    }

    /// Jedno żądanie HTTP/1.1; `extra` – dodatkowe wiersze nagłówka (każdy zakończony `\r\n`).
    fn request(
        &self,
        method: &str,
        prefix: &str,
        path: &str,
        body: &[u8],
        extra: &str,
    ) -> Result<Reply, String> {
        let t = &self.target;
        send(
            t,
            method,
            &format!("{}{prefix}{path}", t.base),
            body,
            extra,
            32 * 1024 * 1024,
            "most",
            true,
        )
    }
}

/// Nagłówek odpowiedzi: (koniec nagłówka, status, nagłówki, chunked, Content-Length).
type Head = (usize, u16, Vec<(String, String)>, bool, Option<usize>);
fn parse_head(raw: &[u8]) -> Result<Head, String> {
    let end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("most: niepełna odpowiedź")?;
    let head = std::str::from_utf8(&raw[..end]).map_err(|_| "most: zły nagłówek")?;
    let mut lines = head.split("\r\n");
    let status: u16 = lines
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|s| s.parse().ok())
        .ok_or("most: zły wiersz statusu")?;
    let mut chunked = false;
    let mut length = None;
    let mut headers = Vec::new();
    for l in lines {
        if let Some((k, v)) = l.split_once(':') {
            let (k, v) = (k.trim(), v.trim());
            headers.push((k.to_ascii_lowercase(), v.to_string()));
            if k.eq_ignore_ascii_case("transfer-encoding") && v.eq_ignore_ascii_case("chunked") {
                chunked = true;
            } else if k.eq_ignore_ascii_case("content-length") {
                length = v.parse::<usize>().ok();
            }
        }
    }
    Ok((end + 4, status, headers, chunked, length))
}

/// Jak [`parse_reply`], ale treść ucięta limitem odczytu (strumień mediów) nie jest błędem.
fn parse_reply_lenient(raw: &[u8]) -> Result<Reply, String> {
    let (start, status, headers, chunked, length) = parse_head(raw)?;
    let rest = &raw[start..];
    let body = if chunked {
        dechunk(rest)?
    } else {
        rest[..length.map_or(rest.len(), |n| n.min(rest.len()))].to_vec()
    };
    Ok(Reply {
        status,
        body,
        headers,
    })
}

/// Odpowiedź HTTP/1.1: status + treść (Content-Length, chunked albo do końca połączenia).
pub fn parse_reply(raw: &[u8]) -> Result<Reply, String> {
    let (start, status, headers, chunked, length) = parse_head(raw)?;
    let rest = &raw[start..];
    let body = if chunked {
        dechunk(rest)?
    } else if let Some(n) = length {
        rest.get(..n).ok_or("most: ucięta odpowiedź")?.to_vec()
    } else {
        rest.to_vec()
    };
    Ok(Reply {
        status,
        body,
        headers,
    })
}

fn dechunk(mut data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let eol = data
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or("most: zły chunk")?;
        let size_txt = std::str::from_utf8(&data[..eol]).map_err(|_| "most: zły chunk")?;
        let size = usize::from_str_radix(size_txt.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| "most: zły rozmiar chunka")?;
        data = &data[eol + 2..];
        if size == 0 {
            return Ok(out);
        }
        out.extend_from_slice(data.get(..size).ok_or("most: ucięty chunk")?);
        data = data.get(size + 2..).ok_or("most: ucięty chunk")?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(
            parse_url("http://ersatztv:8409").unwrap(),
            Target {
                host: "ersatztv".into(),
                port: 8409,
                base: String::new()
            }
        );
        assert_eq!(parse_url("http://10.0.0.5/etv/").unwrap().base, "/etv");
        assert_eq!(parse_url("http://h").unwrap().port, 80);
        assert!(parse_url("https://h").is_err());
        assert!(parse_url("http://:80").is_err());
    }

    #[test]
    fn replies() {
        let r = parse_reply(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}xx").unwrap();
        assert_eq!((r.status, r.body.as_slice()), (200, &b"{}"[..]));
        let r = parse_reply(b"HTTP/1.1 403 Forbidden\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n").unwrap();
        assert_eq!((r.status, r.body.as_slice()), (403, &b"abcde"[..]));
        assert!(parse_reply(b"HTTP/1.1 200 OK\r\n").is_err());
    }
}
