// HTTP client side of the ErsatzTV programs (vizrs-etv-client, vizrs-ffmpeg):
// URLs, connections with timeouts and reply heads, on std alone.

use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

pub const TIMEOUT: Duration = Duration::from_secs(5);
const MAX_HEAD: usize = 8192;

/// Parts of an absolute URL, as `urllib.parse.urlsplit` gives them.
#[derive(Debug, PartialEq)]
pub struct Url<'a> {
    pub scheme: String,
    pub netloc: &'a str,
    pub path: &'a str,
    pub query: &'a str,
}

impl Url<'_> {
    pub fn split(url: &str) -> Option<Url<'_>> {
        let (scheme, rest) = url.split_once("://")?;
        let rest = rest.split('#').next().unwrap_or("");
        let (netloc, rest) = rest.find(['/', '?']).map_or((rest, ""), |i| rest.split_at(i));
        let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
        Some(Url { scheme: scheme.to_ascii_lowercase(), netloc, path, query })
    }

    /// Host and port of an `http` URL (default port 80), userinfo and IPv6 brackets removed.
    pub fn host_port(&self) -> Option<(String, u16)> {
        let host_port = self.netloc.rsplit('@').next().unwrap_or("");
        let (host, port) = match host_port.rfind(':').filter(|&i| !host_port[i..].contains(']')) {
            Some(i) => (&host_port[..i], host_port[i + 1..].parse().ok()?),
            None => (host_port, 80),
        };
        let host = host.trim_start_matches('[').trim_end_matches(']');
        (!host.is_empty()).then(|| (host.to_owned(), port))
    }

    /// Path and query for the request line.
    pub fn target(&self) -> String {
        let path = if self.path.is_empty() { "/" } else { self.path };
        if self.query.is_empty() { path.to_owned() } else { format!("{path}?{}", self.query) }
    }
}

pub fn error(message: String) -> io::Error {
    io::Error::other(message)
}

/// TCP connection to the URL's host with connect, read and write timeouts.
pub fn connect(url: &Url) -> io::Result<TcpStream> {
    let (host, port) = url.host_port().ok_or_else(|| error(format!("bad address {}", url.netloc)))?;
    let mut last = error(format!("{host}: no address"));
    for address in (host.as_str(), port).to_socket_addrs()? {
        match TcpStream::connect_timeout(&address, TIMEOUT) {
            Ok(stream) => {
                stream.set_read_timeout(Some(TIMEOUT))?;
                stream.set_write_timeout(Some(TIMEOUT))?;
                return Ok(stream);
            }
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Reads a reply head byte by byte: nothing of the body may stay in a buffer
/// (the overlay's frame stream socket goes to ffmpeg). Returns the status line.
pub fn read_head(stream: &mut TcpStream, what: &str) -> io::Result<String> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte)? == 0 || head.len() > MAX_HEAD {
            return Err(error(format!("{what}: no reply")));
        }
        head.push(byte[0]);
    }
    let text = String::from_utf8_lossy(&head);
    Ok(text.lines().next().unwrap_or("").to_owned())
}

/// `GET` of a whole small reply; `headers` are complete header lines. An
/// answer other than 200 is an error.
pub fn get(url: &str, headers: &str) -> io::Result<String> {
    let parts = Url::split(url).ok_or_else(|| error(format!("bad URL {url}")))?;
    let mut stream = connect(&parts)?;
    let request = format!(
        "GET {} HTTP/1.0\r\nHost: {}\r\nConnection: close\r\nAccept: application/json\r\n{headers}\r\n",
        parts.target(),
        parts.netloc
    );
    stream.write_all(request.as_bytes())?;
    let line = read_head(&mut stream, parts.netloc)?;
    let mut body = Vec::new();
    stream.read_to_end(&mut body)?;
    let body = String::from_utf8_lossy(&body).into_owned();
    match status_of(&line) {
        "200" => Ok(body),
        status => Err(error(format!("HTTP {status} from {}: {body}", parts.netloc))),
    }
}

/// `POST` of JSON; an answer other than 200 is an error.
pub fn post(url: &str, headers: &str, body: &str) -> io::Result<()> {
    let parts = Url::split(url).ok_or_else(|| error(format!("bad URL {url}")))?;
    let mut stream = connect(&parts)?;
    let request = format!(
        "POST {} HTTP/1.0\r\nHost: {}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{headers}\r\n{body}",
        parts.target(),
        parts.netloc,
        body.len()
    );
    stream.write_all(request.as_bytes())?;
    let line = read_head(&mut stream, parts.netloc)?;
    match status_of(&line) {
        "200" => Ok(()),
        status => Err(error(format!("HTTP {status} from {}", parts.netloc))),
    }
}

/// Status code of a status line.
pub fn status_of(line: &str) -> &str {
    line.split(' ').nth(1).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_parts() {
        let url = Url::split("HTTP://user@localhost:8409/internal/graphics/1/7?exp=1&sig=x#f").unwrap();
        assert_eq!(
            (url.scheme.as_str(), url.netloc, url.path, url.query),
            ("http", "user@localhost:8409", "/internal/graphics/1/7", "exp=1&sig=x")
        );
        assert_eq!(url.host_port(), Some(("localhost".into(), 8409)));
        assert_eq!(url.target(), "/internal/graphics/1/7?exp=1&sig=x");
        assert_eq!(Url::split("http://cg").unwrap().host_port(), Some(("cg".into(), 80)));
        assert_eq!(Url::split("http://[::1]:7700").unwrap().host_port(), Some(("::1".into(), 7700)));
        assert_eq!(Url::split("http://[::1]").unwrap().host_port(), Some(("::1".into(), 80)));
        assert_eq!(Url::split("/m/News.mp4"), None);
    }
}
