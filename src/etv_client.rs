// ErsatzTV script graphics element -> VizRS CG Playout sidecar (binary
// vizrs-etv-client, etv/templates/graphics-elements/script/vizrs.yml).
//
// ErsatzTV starts this for every playout item: the template data (JSON) comes
// on stdin, ETV graphics packets go to stdout. It only forwards both ways, so
// the browser keeps running in the sidecar between items.
//
//     vizrs-etv-client http://cg:7700/etv
//
// ErsatzTV tells a script element nothing about its channel, but it starts the
// element when ffmpeg opens the graphics canvas, whose URL names the channel:
// without "channel=" in the URL the client takes it from the youngest such
// process of the container, preferring one that plays the item's media. An
// explicit "channel=" wins.
//
// If the sidecar is not reachable it exits without output: the item plays
// without graphics. A static binary, so the ErsatzTV image needs no extra
// packages. Every session also brings the page elements of the folder in line
// with VizRS, in the background (src/sync.rs): this program runs where the
// ErsatzTV configuration is, and knows the element's channel from its URL.

use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStringExt;
use std::path::Path;
use std::thread;

use crate::json;
use crate::net::{self, Url, connect, error, read_head, status_of};
use crate::overlay;
use crate::sync::{self, SyncOptions, SyncReport};

pub const DEFAULT_URL: &str = "http://127.0.0.1:7700/etv";
const CHUNK: usize = 1 << 20;

fn log(message: impl std::fmt::Display) {
    let _ = writeln!(io::stderr(), "vizrs-etv: {message}");
}

/// `POST` of the template data; the stream positioned at the packets.
/// HTTP/1.0: the sidecar sends the packets as plain bytes until it closes.
fn open(url: &str, body: &[u8]) -> io::Result<std::net::TcpStream> {
    let url = Url::split(url).ok_or_else(|| error(format!("bad sidecar URL {url}")))?;
    let mut stream = connect(&url)?;
    let head = format!(
        "POST {} HTTP/1.0\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        url.target(),
        url.netloc,
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    let line = read_head(&mut stream, "sidecar")?;
    let status = status_of(&line);
    if status != "200" {
        return Err(error(format!("sidecar replied {status}")));
    }
    // The stream lasts as long as the item: no read timeout.
    stream.set_read_timeout(None)?;
    Ok(stream)
}

/// Page list from the sidecar -> elements in the ErsatzTV folder, if it is here.
fn sync_pages(url: &str) -> io::Result<Option<SyncReport>> {
    let dir = sync::templates_dir(std::env::var("ETV_CONFIG_FOLDER").ok().as_deref());
    if !dir.is_dir() {
        return Ok(None);
    }
    let pages_url = sync::pages_url(url).ok_or_else(|| error(format!("bad sidecar URL {url}")))?;
    let pages = sync::parse_pages(&net::get(&pages_url, "")?).map_err(error)?;
    let element_url = sync::channel_url(url);
    sync::sync_templates(&pages, &dir, SyncOptions { element_url: &element_url, rewrite: false, prune: false }).map(Some)
}

/// ErsatzTV's channels -> the sidecar, which names them in VizRS. The API key
/// stays in this container: the sidecar gets numbers and names only.
fn sync_channels(url: &str) -> io::Result<()> {
    let folder = std::env::var("ETV_CONFIG_FOLDER").ok().filter(|f| !f.is_empty()).unwrap_or_else(|| "/config".into());
    let secrets = json::parse(&std::fs::read_to_string(Path::new(&folder).join("api-secrets.json"))?).map_err(|e| error(e.to_string()))?;
    let key = secrets.get("ApiKey").and_then(|k| k.as_str()).unwrap_or("");
    let channels = json::parse(&net::get("http://127.0.0.1:8409/api/channels", &format!("X-Etv-Api-Key: {key}\r\n"))?)
        .map_err(|e| error(e.to_string()))?;
    let list = channels.as_array().ok_or_else(|| error("channels: expected an array".into()))?;
    let items = list.iter().filter_map(|c| {
        let number = c.get("number")?.as_str()?;
        Some(json::object([("number", number.into()), ("name", c.get("name")?.as_str()?.into())]))
    });
    let body = json::Value::Array(items.collect()).to_string();
    let base = url.split('?').next().unwrap_or(url);
    net::post(&format!("{base}/channels"), "", &body)
}

/// Start time of a process from its `stat`; the name before `)` may hold spaces.
fn start_time(stat: &str) -> Option<u64> {
    stat.rsplit_once(')')?.1.split_whitespace().nth(19)?.parse().ok()
}

/// The channel of the youngest process with a graphics canvas in `proc`; those
/// with the input `media` go first, so items starting together do not mix up.
fn canvas_channel(proc: &Path, media: Option<&str>) -> Option<String> {
    let mut best: Option<((bool, u64), String)> = None;
    for entry in std::fs::read_dir(proc).ok()?.flatten() {
        let Ok(cmdline) = std::fs::read(entry.path().join("cmdline")) else { continue };
        let args: Vec<OsString> = cmdline.split(|b| *b == 0).map(|a| OsString::from_vec(a.to_vec())).collect();
        let Some(canvas) = overlay::find_canvas(&args) else { continue };
        let Some(start) = std::fs::read_to_string(entry.path().join("stat")).ok().and_then(|s| start_time(&s)) else { continue };
        let rank = (media.is_some_and(|media| args.iter().any(|a| a.to_str() == Some(media))), start);
        if best.as_ref().is_none_or(|(top, _)| rank > *top) {
            best = Some((rank, canvas.channel));
        }
    }
    best.map(|(_, channel)| channel)
}

/// `url` with the channel, from the running ffmpeg unless it names one. Without
/// a canvas it stays as it is (the sidecar's default channel).
fn with_channel(url: &str, proc: &Path, media: Option<&str>) -> String {
    let query = url.split_once('?').map_or("", |(_, query)| query);
    if query.split('&').any(|p| p.split('=').next() == Some("channel")) {
        return url.to_owned();
    }
    let Some(channel) = canvas_channel(proc, media) else {
        log("no graphics canvas found: channel not known");
        return url.to_owned();
    };
    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}channel={}", overlay::quote_plus(&channel))
}

/// The client: `vizrs-etv-client [url]`; always exits 0 (no graphics is no error).
pub fn run(url: &str) {
    let mut body = Vec::new();
    if let Err(e) = io::stdin().lock().read_to_end(&mut body) {
        log(e);
        return;
    }
    let sync_url = url.to_owned();
    let _ = thread::Builder::new().name("sync".into()).spawn(move || match sync_pages(&sync_url) {
        Ok(Some(report)) if report.changed() => log(format!("page elements: {report}")),
        Ok(_) => {}
        Err(e) => log(format!("page elements: {e}")),
    });
    let channels_url = url.to_owned();
    thread::spawn(move || {
        if let Err(e) = sync_channels(&channels_url) {
            log(format!("channel names: {e}"));
        }
    });
    let data = json::parse(&String::from_utf8_lossy(&body)).ok();
    let media = data.as_ref().and_then(|d| d.get("MediaItem_Path")).and_then(|p| p.as_str());
    let mut stream = match open(&with_channel(url, Path::new("/proc"), media), &body) {
        Ok(stream) => stream,
        Err(e) => {
            log(e);
            return;
        }
    };

    let mut out = io::stdout().lock();
    let mut buffer = vec![0u8; CHUNK];
    // Ends with the stream, or when ErsatzTV closes stdout.
    while let Ok(n @ 1..) = stream.read(&mut buffer) {
        if out.write_all(&buffer[..n]).and_then(|()| out.flush()).is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(proc: &Path, pid: u32, start: u64, url: &str) {
        let dir = proc.join(pid.to_string());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("cmdline"), format!("/vizrs/vizrs-ffmpeg\0-i\0/m/{pid}.mp4\0-i\0{url}\0-f\0hls\0o\0")).unwrap();
        let tail = (0..18).map(|_| "0").collect::<Vec<_>>().join(" ");
        std::fs::write(dir.join("stat"), format!("{pid} (vizrs ffmpeg) S {tail} {start} 0")).unwrap();
    }

    #[test]
    fn channel_comes_from_the_youngest_canvas() {
        let proc = std::env::temp_dir().join(format!("vizrs-proc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&proc);
        std::fs::create_dir_all(&proc).unwrap();
        assert_eq!(with_channel("http://cg:7700/etv", &proc, None), "http://cg:7700/etv");

        process(&proc, 10, 500, "http://localhost:8409/internal/graphics/2/7");
        process(&proc, 11, 900, "http://localhost:8409/internal/graphics/4/8");
        process(&proc, 12, 950, "http://localhost:8409/iptv/channel/9.m3u8");
        assert_eq!(with_channel("http://cg:7700/etv", &proc, None), "http://cg:7700/etv?channel=4");
        assert_eq!(with_channel("http://cg:7700/etv?page=3", &proc, None), "http://cg:7700/etv?page=3&channel=4");
        assert_eq!(with_channel("http://cg:7700/etv?channel=2&page=3", &proc, None), "http://cg:7700/etv?channel=2&page=3");
        // The item's media beats the youngest process.
        assert_eq!(with_channel("http://cg:7700/etv", &proc, Some("/m/10.mp4")), "http://cg:7700/etv?channel=2");
        assert_eq!(with_channel("http://cg:7700/etv", &proc, Some("/m/x.mp4")), "http://cg:7700/etv?channel=4");
        std::fs::remove_dir_all(&proc).unwrap();
    }
}
