// Minimal HTTP/1.1 server: one request per connection, a thread per
// connection. Replies are complete (with Content-Length) or an endless stream
// framed as Node.js does it: chunked for HTTP/1.1 (a reply ended only by the
// connection is cut short by some clients), raw bytes until the connection
// closes for HTTP/1.0 (the ErsatzTV programs).

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

// Request line plus headers; ErsatzTV and the clients send a few hundred bytes.
const MAX_HEAD: usize = 64 * 1024;
// A client that does not finish its request by then is dropped.
const READ_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// An HTTP/1.0 request: streams go out unframed.
    pub http10: bool,
}

impl Request {
    /// First query parameter `name`, decoded.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.query.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// Header `name` (case-insensitive).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// Malformed request: reply 400.
    Bad(&'static str),
    /// Body over the limit: reply 413.
    TooLarge,
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Error::Io(error)
    }
}

/// `%XX` and `+` of a query component; invalid escapes stay as they are.
pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' => {
                // Two hex digits; `from_str_radix` alone would also take a sign.
                let hex = bytes.get(i + 1..i + 3).filter(|h| h.iter().all(u8::is_ascii_hexdigit)).and_then(|h| std::str::from_utf8(h).ok());
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(b) => {
                        out.push(b);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|part| !part.is_empty())
        .map(|part| match part.split_once('=') {
            Some((k, v)) => (percent_decode(k), percent_decode(v)),
            None => (percent_decode(part), String::new()),
        })
        .collect()
}

fn read_line(reader: &mut impl BufRead, budget: &mut usize) -> Result<String, Error> {
    let mut line = Vec::new();
    let read = reader.by_ref().take(*budget as u64 + 1).read_until(b'\n', &mut line)?;
    if read == 0 {
        return Err(Error::Bad("connection closed before the request"));
    }
    if read > *budget {
        return Err(Error::Bad("request head too large"));
    }
    *budget -= read;
    while matches!(line.last(), Some(b'\n' | b'\r')) {
        line.pop();
    }
    String::from_utf8(line).map_err(|_| Error::Bad("request head is not UTF-8"))
}

/// Reads one request; the body up to `max_body` bytes (Content-Length or chunked).
pub fn read_request(reader: &mut impl BufRead, max_body: usize) -> Result<Request, Error> {
    let mut budget = MAX_HEAD;
    let line = read_line(reader, &mut budget)?;
    let mut parts = line.split(' ');
    let (Some(method), Some(target), Some(version)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(Error::Bad("bad request line"));
    };
    if !version.starts_with("HTTP/1.") {
        return Err(Error::Bad("unsupported HTTP version"));
    }

    let mut headers = Vec::new();
    loop {
        let line = read_line(reader, &mut budget)?;
        if line.is_empty() {
            break;
        }
        let (name, value) = line.split_once(':').ok_or(Error::Bad("bad header"))?;
        headers.push((name.trim().to_owned(), value.trim().to_owned()));
    }

    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut request = Request {
        method: method.to_owned(),
        path: percent_decode(path),
        query: parse_query(query),
        headers,
        body: Vec::new(),
        http10: version == "HTTP/1.0",
    };

    if request.header("transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
        request.body = read_chunked(reader, max_body)?;
    } else if let Some(length) = request.header("content-length") {
        let length: usize = length.parse().map_err(|_| Error::Bad("bad Content-Length"))?;
        if length > max_body {
            return Err(Error::TooLarge);
        }
        request.body = vec![0; length];
        reader.read_exact(&mut request.body)?;
    }
    Ok(request)
}

fn read_chunked(reader: &mut impl BufRead, max_body: usize) -> Result<Vec<u8>, Error> {
    let mut body = Vec::new();
    loop {
        let mut budget = MAX_HEAD;
        let line = read_line(reader, &mut budget)?;
        let size = line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size, 16).map_err(|_| Error::Bad("bad chunk size"))?;
        if size == 0 {
            // Trailers up to the empty line.
            while !read_line(reader, &mut budget)?.is_empty() {}
            return Ok(body);
        }
        if body.len() + size > max_body {
            return Err(Error::TooLarge);
        }
        let start = body.len();
        body.resize(start + size, 0);
        reader.read_exact(&mut body[start..])?;
        read_line(reader, &mut budget)?;
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        303 => "See Other",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        _ => "",
    }
}

/// A complete reply; the connection closes after it.
pub fn respond(stream: &mut impl Write, status: u16, content_type: Option<&str>, body: &[u8]) -> io::Result<()> {
    match content_type {
        Some(content_type) => respond_with(stream, status, &[("Content-Type", content_type)], body),
        None => respond_with(stream, status, &[], body),
    }
}

/// A complete reply with header lines of its own.
pub fn respond_with(stream: &mut impl Write, status: u16, headers: &[(&str, &str)], body: &[u8]) -> io::Result<()> {
    let mut head = format!("HTTP/1.1 {status} {}\r\n", reason(status));
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n", body.len()));
    let mut reply = head.into_bytes();
    reply.extend_from_slice(body);
    stream.write_all(&reply)?;
    stream.flush()
}

// Bytes collected before a chunk goes out (a flush sends one chunk earlier).
const STREAM_BUFFER: usize = 64 * 1024;

/// Body of an endless binary stream. Writes collect until `flush`; each
/// flush is one chunk (HTTP/1.1) or plain bytes (HTTP/1.0).
pub struct Stream<W: Write> {
    inner: W,
    chunked: bool,
    buffer: Vec<u8>,
}

impl<W: Write> Stream<W> {
    /// Sends the head at once (a held session writes nothing at first, the
    /// client must see the reply anyway).
    pub fn start(inner: W, request: &Request) -> io::Result<Stream<W>> {
        Stream::start_with(inner, request, "")
    }

    /// `start` with more header lines (each ending in CRLF).
    pub fn start_with(mut inner: W, request: &Request, headers: &str) -> io::Result<Stream<W>> {
        let framing = if request.http10 { "Connection: close" } else { "Transfer-Encoding: chunked" };
        let head =
            format!("HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nCache-Control: no-store\r\n{headers}{framing}\r\n\r\n");
        inner.write_all(head.as_bytes())?;
        inner.flush()?;
        Ok(Stream { inner, chunked: !request.http10, buffer: Vec::with_capacity(STREAM_BUFFER) })
    }

    /// The end of the body: the last chunk, so the client reads to the end.
    pub fn finish(mut self) -> io::Result<()> {
        self.flush()?;
        if self.chunked {
            self.inner.write_all(b"0\r\n\r\n")?;
        }
        self.inner.flush()
    }
}

impl<W: Write> Write for Stream<W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if self.buffer.len() + data.len() > STREAM_BUFFER {
            self.flush()?;
        }
        // A frame of raw graphics (megabytes) goes out as it is, not copied into the buffer first.
        if data.len() >= STREAM_BUFFER {
            if self.chunked {
                self.inner.write_all(format!("{:x}\r\n", data.len()).as_bytes())?;
                self.inner.write_all(data)?;
                self.inner.write_all(b"\r\n")?;
            } else {
                self.inner.write_all(data)?;
            }
            return Ok(data.len());
        }
        self.buffer.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if !self.buffer.is_empty() {
            if self.chunked {
                let mut chunk = Vec::with_capacity(self.buffer.len() + 16);
                chunk.extend_from_slice(format!("{:x}\r\n", self.buffer.len()).as_bytes());
                chunk.extend_from_slice(&self.buffer);
                chunk.extend_from_slice(b"\r\n");
                self.inner.write_all(&chunk)?;
            } else {
                self.inner.write_all(&self.buffer)?;
            }
            self.buffer.clear();
        }
        self.inner.flush()
    }
}

pub type Handler = dyn Fn(Request, TcpStream) + Send + Sync;

/// Accepts connections until the listener fails; every request goes to
/// `handler` on its own thread, with the connection to reply on.
pub fn serve(listener: TcpListener, max_body: usize, handler: Arc<Handler>) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let handler = Arc::clone(&handler);
        let spawned = thread::Builder::new().name("http".into()).spawn(move || connection(stream, max_body, &*handler));
        if let Err(error) = spawned {
            crate::error!("http: no thread for a connection: {error}");
        }
    }
}

fn connection(stream: TcpStream, max_body: usize, handler: &Handler) {
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let Ok(read_half) = stream.try_clone() else { return };
    let mut reader = BufReader::new(read_half);
    let mut stream = stream;

    match read_request(&mut reader, max_body) {
        Ok(request) => {
            // Replies may stream for hours: no timeout from here on.
            let _ = stream.set_read_timeout(None);
            handler(request, stream);
        }
        Err(Error::Bad(message)) => {
            let _ = respond(&mut stream, 400, Some("text/plain"), message.as_bytes());
        }
        Err(Error::TooLarge) => {
            let _ = respond(&mut stream, 413, None, b"");
        }
        Err(Error::Io(_)) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(raw: &str) -> Result<Request, Error> {
        read_request(&mut raw.as_bytes(), 16)
    }

    #[test]
    fn request_with_query_and_body() {
        let request = parse("POST /etv?channel=2&render=720p&x=a%20b+c HTTP/1.1\r\nHost: h\r\ncontent-length: 4\r\n\r\n{}{}").unwrap();
        assert_eq!((request.method.as_str(), request.path.as_str()), ("POST", "/etv"));
        assert_eq!(request.param("channel"), Some("2"));
        assert_eq!(request.param("x"), Some("a b c"));
        assert_eq!(request.param("missing"), None);
        assert_eq!(request.header("Content-Length"), Some("4"));
        assert_eq!(request.body, b"{}{}");

        let request = parse("GET /ffmpeg/frames?session=3 HTTP/1.0\nHost: h\n\n").unwrap();
        assert_eq!(request.param("session"), Some("3"));
        assert!(request.body.is_empty());
    }

    #[test]
    fn chunked_body_and_limits() {
        let request = parse("POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2;x=y\r\nde\r\n0\r\n\r\n").unwrap();
        assert_eq!(request.body, b"abcde");
        assert!(matches!(parse("POST / HTTP/1.1\r\nContent-Length: 17\r\n\r\n"), Err(Error::TooLarge)));
        assert!(matches!(parse("POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n11\r\n"), Err(Error::TooLarge)));
        assert!(matches!(parse("GARBAGE\r\n\r\n"), Err(Error::Bad(_))));
        assert!(matches!(parse("GET / SPDY/3\r\n\r\n"), Err(Error::Bad(_))));
        assert!(matches!(parse(""), Err(Error::Bad(_))));
        let long = format!("GET /{} HTTP/1.1\r\n\r\n", "a".repeat(MAX_HEAD));
        assert!(matches!(parse(&long), Err(Error::Bad("request head too large"))));
    }

    #[test]
    fn streams_chunked_for_http11_and_plain_for_http10() {
        let http11 = parse("POST /etv HTTP/1.1\r\n\r\n").unwrap();
        let mut out = Vec::new();
        let mut stream = Stream::start(&mut out, &http11).unwrap();
        stream.write_all(b"ab").unwrap();
        stream.flush().unwrap();
        stream.flush().unwrap();
        stream.write_all(b"cde").unwrap();
        stream.finish().unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("Transfer-Encoding: chunked\r\n\r\n2\r\nab\r\n3\r\ncde\r\n0\r\n\r\n"), "{text:?}");

        let http10 = parse("GET /ffmpeg/frames HTTP/1.0\r\n\r\n").unwrap();
        let mut out = Vec::new();
        let mut stream = Stream::start(&mut out, &http10).unwrap();
        stream.write_all(b"ab").unwrap();
        stream.finish().unwrap();
        assert!(String::from_utf8(out).unwrap().ends_with("Connection: close\r\n\r\nab"));
    }

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("a%2Fb%zz%4"), "a/b%zz%4");
        assert_eq!(percent_decode("%C5%BC"), "ż");
        assert_eq!(percent_decode("%"), "%");
    }
}
