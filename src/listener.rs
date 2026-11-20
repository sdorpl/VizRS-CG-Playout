// Live ingest listeners: `vizrs-ffmpeg vizrs-ingest` (DOKUMENTACJA §1, Live
// ingest), run as a service of its own in the ErsatzTV image (its ffmpeg).
// Each listener is an ffmpeg that waits for a publisher (RTMP `-listen 1`,
// SRT `mode=listener`), remuxes it to MPEG-TS without transcoding and hands
// it to the sidecar (`POST /ingest?channel=N`); then it listens again.
// The list comes from the sidecar (`GET /ingest/listeners`, set in VizRS Live
// through the live API) every few seconds; changed listeners restart, the
// others go on. An older sidecar without it: `VIZRS_INGEST`.
//
//     VIZRS_INGEST         channel=URL, separated by ";" or white space:
//                          "1=rtmp://0.0.0.0:1936/live/KEY; 2=srt://0.0.0.0:9000?passphrase=…"
//     VIZRS_INGEST_TOKEN   shared secret with the sidecar (its ingest_token)
//     VIZRS_INGEST_IDLE_MS a publisher silent this long has ended (default 15000)
//     VIZRS_CG_URL         sidecar, e.g. http://cg:7700
//     VIZRS_FFMPEG         ffmpeg (default /usr/local/bin/ffmpeg)
//     VIZRS_INGEST_LOG     a file the log lines also go to, with the time
//                          (compose: <etv_config>/vizrs-live/logs/vizrs-ingest.log)
//     VIZRS_YTDLP          yt-dlp for YouTube inputs (default /vizrs/yt-dlp)
//     VIZRS_YTDLP_COOKIES  a cookies file for yt-dlp (a server YouTube asks to sign in);
//                          empty: the one VizRS Live uploads (src/yt_cookies.rs), a copy of
//     VIZRS_YTDLP_PANEL_COOKIES  (default /etv-config/vizrs-live/youtube-cookies.txt)
//
// A YouTube input (`1=https://www.youtube.com/…`) has no publisher: yt-dlp
// finds its stream, ffmpeg reads it and the rest goes as from a publisher.
// A link input (`1=https://host/live.m3u8`, `rtmp://host/app/key`,
// `srt://host:port`, `rtsp://…` – a host not of this machine) has none either:
// ffmpeg reads the stream itself, and reads it again whenever it ends.
//
// ffmpeg's RTMP server takes any stream name and only warns about one it
// does not expect: that publisher is cut off here, so the key is the password.

use std::ffi::OsString;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use crate::net::{Url, connect, error, read_head, status_of};
use crate::overlay::quote_plus;

const DEFAULT_FFMPEG: &str = "/usr/local/bin/ffmpeg";
/// The sidecar installs it next to `vizrs-ffmpeg` (Dockerfile, DOKUMENTACJA §1, Live ingest).
const DEFAULT_YTDLP: &str = "/vizrs/yt-dlp";
/// The cookies file VizRS Live uploads, as the ingest container sees the ErsatzTV config (compose).
const PANEL_COOKIES: &str = "/etv-config/vizrs-live/youtube-cookies.txt";
/// A yt-dlp lookup that takes longer is stopped.
const RESOLVE_LIMIT: Duration = Duration::from_secs(90);
/// A YouTube input that is not on (a broadcast not started, a refused lookup) is asked again this often.
const YOUTUBE_AGAIN: Duration = Duration::from_secs(30);
/// A link input that gave nothing (the stream not on, a refused address) is read again this often.
const LINK_AGAIN: Duration = Duration::from_secs(10);
/// ffmpeg's read and connect limit for a link input (microseconds): a server
/// that stops sending ends the read instead of hanging it.
const LINK_TIMEOUT_US: &str = "15000000";
/// The longest link input address.
const LINK_MAX: usize = 1000;
/// What the takeover can take copied into MPEG-TS: H.264 and AAC (not VP9,
/// AV1 or Opus), at most 1080p; a broadcast's audio has no codec in its list,
/// so only WebM (Opus) audio is left out.
const YOUTUBE_FORMAT: &str = "bv*[vcodec^=avc1][height<=1080]+ba[ext!=webm]/b[vcodec^=avc1][height<=1080]/b[vcodec^=avc1]";
/// Longer than the sidecar's `ingest::DEFAULT_IDLE`: a short silence (a relay
/// that reads HLS sends 2 s segments in bursts) only ends the live on air
/// there, and the publisher stays connected, so the next bytes bring it back
/// at once. Dropping the publisher makes it reconnect, which a relay like
/// datarhei Restreamer does only 15 s later.
const DEFAULT_IDLE_MS: u64 = 15_000;
const _: () = assert!(DEFAULT_IDLE_MS > crate::ingest::DEFAULT_IDLE.as_millis() as u64);
const AGAIN: Duration = Duration::from_secs(1);
const MAX_LISTENERS: usize = 16;
/// How often the list is asked for from the sidecar.
const POLL: Duration = Duration::from_secs(5);
/// Ports in the container the compose file maps to the host (listeners set in VizRS Live use only these).
pub const RTMP_PORTS: std::ops::RangeInclusive<u16> = 1936..=1943;
pub const SRT_PORTS: std::ops::RangeInclusive<u16> = 9000..=9007;

/// What an input is: a listener a publisher comes to, a YouTube address the
/// service fetches itself (yt-dlp finds the stream, ffmpeg reads it) or a
/// link to a stream ffmpeg reads (HLS, RTMP, SRT caller, RTSP).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Rtmp,
    Srt,
    YouTube,
    Link,
}

/// The host of a URL's netloc: no userinfo, port or IPv6 brackets, lower case.
fn host_of(netloc: &str) -> String {
    let host_port = netloc.rsplit('@').next().unwrap_or("");
    let host = match host_port.rfind(':').filter(|&i| !host_port[i..].contains(']')) {
        Some(i) => &host_port[..i],
        None => host_port,
    };
    host.trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase()
}

/// The kind of an input URL; `None` = not one of them. `rtmp://` and `srt://`
/// on 0.0.0.0, `::` or a loopback address listen; on another host they are a link to read.
pub fn kind_of(url: &str) -> Option<Kind> {
    let parts = Url::split(url)?;
    let host = host_of(parts.netloc);
    if host.is_empty() || host.contains(|c: char| c.is_whitespace() || c.is_control()) {
        return None;
    }
    // This machine's addresses: nothing to read from there in a container, a listener binds them.
    let listens = matches!(host.as_str(), "0.0.0.0" | "::" | "::1" | "localhost") || host.starts_with("127.");
    match parts.scheme.as_str() {
        "rtmp" if listens => Some(Kind::Rtmp),
        "srt" if listens => Some(Kind::Srt),
        "rtmp" | "rtmps" | "srt" | "rtsp" | "rtsps" if !listens => Some(Kind::Link),
        "http" | "https" if !listens => {
            let short = ["www.", "m.", "music."].iter().find_map(|p| host.strip_prefix(p)).unwrap_or(&host);
            if matches!(short, "youtube.com" | "youtu.be") { (parts.path.len() > 1).then_some(Kind::YouTube) } else { Some(Kind::Link) }
        }
        _ => None,
    }
}

/// A YouTube channel's live page (`/@name/live`, `/channel/ID/live`): an input
/// that waits for each of the channel's broadcasts and stays. Any other
/// YouTube address is one video, a broadcast or a recording, played once.
pub fn youtube_channel_live(url: &str) -> bool {
    let path = Url::split(url).map_or("", |u| u.path).trim_end_matches('/');
    path.rsplit_once('/').is_some_and(|(before, last)| last.eq_ignore_ascii_case("live") && !before.is_empty())
}

/// A listener: the channel it feeds and the URL it listens on.
#[derive(Clone, Debug, PartialEq)]
pub struct Listener {
    pub channel: String,
    pub url: String,
}

/// `VIZRS_INGEST`: listeners and the errors of the entries left out.
pub fn parse_listeners(text: &str) -> (Vec<Listener>, Vec<String>) {
    let mut listeners: Vec<Listener> = Vec::new();
    let mut errors = Vec::new();
    for entry in text.split(|c: char| c == ';' || c.is_whitespace()).filter(|e| !e.is_empty()) {
        let Some((channel, url)) = entry.split_once('=') else {
            errors.push(format!("\"{entry}\": expected channel=URL"));
            continue;
        };
        if kind_of(url).is_none() {
            errors.push(format!("\"{entry}\": only rtmp:// and srt:// listeners on 0.0.0.0, YouTube addresses and links to a stream"));
        } else if channel.trim().is_empty() {
            errors.push(format!("\"{entry}\": no channel"));
        } else if listeners.iter().any(|l| l.url == url) {
            errors.push(format!("\"{entry}\": the same URL twice"));
        } else if listeners.len() == MAX_LISTENERS {
            errors.push(format!("\"{entry}\": at most {MAX_LISTENERS} listeners"));
        } else {
            listeners.push(Listener { channel: channel.trim().to_owned(), url: url.to_owned() });
        }
    }
    (listeners, errors)
}

/// The stream name an RTMP listener expects (last part of its path).
pub fn rtmp_key(url: &str) -> Option<&str> {
    let path = Url::split(url)?.path;
    path.rsplit('/').next().filter(|k| !k.is_empty() && path.matches('/').count() >= 2)
}

/// What a publisher needs to reach a listener (live API, VizRS Live): RTMP server and
/// stream key as OBS asks for them, or the SRT address and passphrase. `host`
/// "" = unknown (the panel puts in its own); a port given replaces the first
/// port of the range (compose maps `INGEST_RTMP_PORT`/`INGEST_SRT_PORT` to it), the
/// other ports of the range are the same on the host.
pub fn publisher(listener: &Listener, host: &str, rtmp_port: Option<u16>, srt_port: Option<u16>) -> crate::json::Value {
    use crate::json;
    if kind_of(&listener.url) == Some(Kind::YouTube) {
        let live = if youtube_channel_live(&listener.url) { "channel" } else { "video" };
        return json::object([("kind", "youtube".into()), ("url", listener.url.as_str().into()), ("youtube", live.into())]);
    }
    if kind_of(&listener.url) == Some(Kind::Link) {
        let scheme = Url::split(&listener.url).map_or(String::new(), |u| u.scheme);
        return json::object([("kind", "link".into()), ("url", listener.url.as_str().into()), ("scheme", scheme.as_str().into())]);
    }
    let url = Url::split(&listener.url);
    let kind = url.as_ref().map_or(String::new(), |u| u.scheme.to_ascii_lowercase());
    let own_port = url.as_ref().and_then(|u| u.netloc.rsplit_once(':')).and_then(|(_, p)| p.parse::<u16>().ok());
    let base = |own: Option<u16>, first: u16, given: Option<u16>| match own {
        Some(p) if p != first => Some(p),
        _ => given.or(own),
    };
    let port = if kind == "rtmp" {
        base(own_port, *RTMP_PORTS.start(), rtmp_port).unwrap_or(1935)
    } else {
        base(own_port, *SRT_PORTS.start(), srt_port).unwrap_or(0)
    };
    let path = url.as_ref().map_or("", |u| u.path);
    let key = if kind == "rtmp" { rtmp_key(&listener.url).unwrap_or("") } else { "" };
    let app = if key.is_empty() { path } else { &path[..path.len() - key.len() - 1] };
    let query = url.as_ref().map_or("", |u| u.query.trim_start_matches('?'));
    let param = |name: &str| query.split('&').find_map(|kv| kv.strip_prefix(name).and_then(|v| v.strip_prefix('='))).unwrap_or("");
    json::object([
        ("kind", kind.as_str().into()),
        ("host", host.into()),
        ("port", f64::from(port).into()),
        ("app", app.into()),
        ("key", key.into()),
        ("passphrase", param("passphrase").into()),
        ("streamid", param("streamid").into()),
    ])
}

/// A short name of an input for the operator, without its secrets: `RTMP 1936`,
/// `SRT 9000`, `YouTube youtube.com/@name/live`, `Link HLS cdn.example.pl`.
pub fn label(listener: &Listener) -> String {
    let url = &listener.url;
    let Some(parts) = Url::split(url) else { return url.chars().take(60).collect() };
    let host = host_of(parts.netloc);
    match kind_of(url) {
        Some(Kind::Rtmp) => format!("RTMP {}", port_of(url).unwrap_or(1935)),
        Some(Kind::Srt) => format!("SRT {}", port_of(url).unwrap_or(0)),
        Some(Kind::YouTube) => {
            let host = host.strip_prefix("www.").unwrap_or(&host);
            let tail: String =
                format!("{}{}", parts.path, if parts.query.is_empty() { String::new() } else { format!("?{}", parts.query) });
            format!("YouTube {host}{}", tail.chars().take(60).collect::<String>())
        }
        Some(Kind::Link) => {
            let what = match parts.scheme.as_str() {
                "http" | "https" if parts.path.to_ascii_lowercase().ends_with(".m3u8") => "HLS",
                "http" | "https" => "HTTP",
                "rtmp" | "rtmps" => "RTMP",
                "srt" => "SRT",
                _ => "RTSP",
            };
            format!("Link {what} {host}")
        }
        None => url.chars().take(60).collect(),
    }
}

/// The port a listener URL binds.
pub fn port_of(url: &str) -> Option<u16> {
    Url::split(url).and_then(|u| u.netloc.rsplit_once(':').and_then(|(_, p)| p.parse().ok()))
}

/// What is wrong with a listener set in VizRS Live (ports the compose file maps,
/// one listener per port, SRT passphrase 10–79 characters); `None` = fine.
pub fn check_listeners(listeners: &[Listener]) -> Option<String> {
    let mut ports = Vec::new();
    for l in listeners {
        if l.url.contains(|c: char| c.is_whitespace() || c == ';') {
            return Some(format!("kanał {}: adres ze spacją albo średnikiem", l.channel));
        }
        if kind_of(&l.url) == Some(Kind::YouTube) {
            if l.url.len() > 500 {
                return Some(format!("kanał {}: adres YouTube dłuższy niż 500 znaków", l.channel));
            }
            continue;
        }
        if kind_of(&l.url) == Some(Kind::Link) {
            if l.url.len() > LINK_MAX {
                return Some(format!("kanał {}: link dłuższy niż {LINK_MAX} znaków", l.channel));
            }
            continue;
        }
        let rtmp = l.url.to_ascii_lowercase().starts_with("rtmp");
        let range = if rtmp { RTMP_PORTS } else { SRT_PORTS };
        let Some(port) = port_of(&l.url).filter(|p| range.contains(p)) else {
            return Some(format!("kanał {}: port spoza {}–{}", l.channel, range.start(), range.end()));
        };
        if ports.contains(&(rtmp, port)) {
            return Some(format!("kanał {}: port {port} już zajęty przez inne wejście", l.channel));
        }
        ports.push((rtmp, port));
        if rtmp && rtmp_key(&l.url).is_none() {
            return Some(format!("kanał {}: RTMP bez klucza (rtmp://0.0.0.0:{port}/live/<klucz>)", l.channel));
        }
        let query = l.url.split_once('?').map_or("", |(_, q)| q);
        if let Some(pass) = query.split('&').find_map(|kv| kv.strip_prefix("passphrase=")) {
            if !(10..=79).contains(&pass.len()) {
                return Some(format!("kanał {}: hasło SRT musi mieć 10–79 znaków", l.channel));
            }
        }
    }
    None
}

/// ffmpeg's arguments for a listener: the publisher remuxed to MPEG-TS on stdout.
pub fn listen_args(url: &str) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-hide_banner", "-nostdin", "-nostats", "-loglevel", "warning"].iter().map(OsString::from).collect();
    let input = if url.to_ascii_lowercase().starts_with("rtmp") {
        args.extend(["-listen".into(), "1".into()]);
        url.to_owned()
    } else {
        // MPEG-TS: ffmpeg would analyse 5 s of it before the first packet goes out.
        args.extend(["-analyzeduration".into(), "2000000".into()]);
        if url.contains("mode=") { url.to_owned() } else { format!("{url}{}mode=listener", if url.contains('?') { '&' } else { '?' }) }
    };
    args.extend(["-i".into(), input.into()]);
    // Every packet flushed: a small stream would otherwise wait in the pipe buffer.
    let out = [
        "-map",
        "0:v:0",
        "-map",
        "0:a:0?",
        "-c",
        "copy",
        "-flush_packets",
        "1",
        "-f",
        "mpegts",
        "-mpegts_flags",
        "+resend_headers",
        "pipe:1",
    ];
    args.extend(out.iter().map(OsString::from));
    args
}

/// ffmpeg's arguments for a link input: the stream read (an HTTP one – HLS, a
/// file – in real time, as a recording would otherwise go at once) and
/// remuxed to MPEG-TS on stdout like a publisher. Only the protocols of its
/// own scheme: an HLS list cannot point ffmpeg at a local file.
pub fn link_args(url: &str) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-hide_banner", "-nostdin", "-nostats", "-loglevel", "warning"].iter().map(OsString::from).collect();
    let scheme = Url::split(url).map_or(String::new(), |u| u.scheme);
    let (protocols, options): (&str, &[&str]) = match scheme.as_str() {
        "http" | "https" => (
            "http,https,tcp,tls,crypto,httpproxy",
            &["-re", "-reconnect", "1", "-reconnect_streamed", "1", "-reconnect_delay_max", "5", "-rw_timeout", LINK_TIMEOUT_US],
        ),
        "rtmp" | "rtmps" => ("rtmp,rtmps,tcp,tls", &["-rw_timeout", LINK_TIMEOUT_US]),
        "rtsp" | "rtsps" => ("rtsp,rtsps,rtp,srtp,udp,tcp,tls", &["-rtsp_transport", "tcp", "-timeout", LINK_TIMEOUT_US]),
        // MPEG-TS: ffmpeg would analyse 5 s of it before the first packet goes out.
        _ => ("srt", &["-analyzeduration", "2000000", "-timeout", LINK_TIMEOUT_US]),
    };
    args.extend(["-protocol_whitelist".into(), protocols.into()]);
    args.extend(options.iter().map(OsString::from));
    let input = if scheme == "srt" && !url.contains("mode=") {
        format!("{url}{}mode=caller", if url.contains('?') { '&' } else { '?' })
    } else {
        url.to_owned()
    };
    args.extend(["-i".into(), input.into()]);
    let out = [
        "-map",
        "0:v:0",
        "-map",
        "0:a:0?",
        "-c",
        "copy",
        "-flush_packets",
        "1",
        "-f",
        "mpegts",
        "-mpegts_flags",
        "+resend_headers",
        "pipe:1",
    ];
    args.extend(out.iter().map(OsString::from));
    args
}

fn log(message: impl std::fmt::Display) {
    let _ = writeln!(io::stderr(), "vizrs-ingest: {message}");
    crate::log::to_file_only(format_args!("vizrs-ingest: {message}"));
}

/// The listeners' environment.
pub struct Settings {
    pub ffmpeg: OsString,
    pub sidecar: String,
    pub token: String,
    pub idle: Duration,
    /// `VIZRS_INGEST`: in force while the sidecar does not give its list.
    pub listeners: Vec<Listener>,
    /// yt-dlp, which finds the stream of a YouTube input (`VIZRS_YTDLP`).
    pub ytdlp: OsString,
    /// A cookies file for yt-dlp (`VIZRS_YTDLP_COOKIES`), for a server YouTube asks to sign in.
    pub cookies: Option<OsString>,
    /// Without it: the cookies file VizRS Live uploads (`VIZRS_YTDLP_PANEL_COOKIES`); yt-dlp gets a copy.
    pub panel_cookies: std::path::PathBuf,
}

impl Settings {
    pub fn from_env() -> Result<Settings, String> {
        let var = |name| std::env::var(name).unwrap_or_default();
        let (listeners, errors) = parse_listeners(&var("VIZRS_INGEST"));
        for e in errors {
            log(format_args!("VIZRS_INGEST {e}"));
        }
        let idle = match var("VIZRS_INGEST_IDLE_MS").trim() {
            "" => DEFAULT_IDLE_MS,
            text => text.parse().ok().filter(|&ms| ms > 0).ok_or(format!("VIZRS_INGEST_IDLE_MS=\"{text}\": milliseconds > 0"))?,
        };
        let settings = Settings {
            ffmpeg: std::env::var_os("VIZRS_FFMPEG").unwrap_or_else(|| DEFAULT_FFMPEG.into()),
            sidecar: var("VIZRS_CG_URL").trim_end_matches('/').to_owned(),
            token: var("VIZRS_INGEST_TOKEN").trim().to_owned(),
            idle: Duration::from_millis(idle),
            listeners,
            ytdlp: std::env::var_os("VIZRS_YTDLP").filter(|v| !v.is_empty()).unwrap_or_else(|| DEFAULT_YTDLP.into()),
            cookies: std::env::var_os("VIZRS_YTDLP_COOKIES").filter(|v| !v.is_empty()),
            panel_cookies: std::env::var_os("VIZRS_YTDLP_PANEL_COOKIES")
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| PANEL_COOKIES.into())
                .into(),
        };
        if settings.sidecar.is_empty() {
            return Err("VIZRS_CG_URL is empty".into());
        }
        if settings.token.is_empty() {
            return Err("VIZRS_INGEST_TOKEN is empty".into());
        }
        Ok(settings)
    }
}

/// The id of an input for the sidecar (`input=` of `POST /ingest`): a hash of
/// its entry, so that the address (an SRT passphrase, an RTMP key) is not in a
/// URL. The sidecar finds the input's own graphics by it (`on_live` of the input).
pub fn input_id(listener: &Listener) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in listener.channel.bytes().chain(*b"=").chain(listener.url.bytes()) {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// `POST /ingest` to the sidecar; the socket for the body once it accepted.
fn open_source(settings: &Settings, listener: &Listener) -> io::Result<TcpStream> {
    let channel = &listener.channel;
    let url = Url::split(&settings.sidecar).ok_or_else(|| error(format!("bad sidecar URL {}", settings.sidecar)))?;
    let mut stream = connect(&url)?;
    let request = format!(
        "POST /ingest?channel={}&input={} HTTP/1.0\r\nHost: {}\r\nAuthorization: Bearer {}\r\nContent-Type: video/mp2t\r\n\r\n",
        quote_plus(channel),
        input_id(listener),
        url.netloc,
        settings.token
    );
    stream.write_all(request.as_bytes())?;
    let line = read_head(&mut stream, "sidecar")?;
    match status_of(&line) {
        "200" => Ok(stream),
        "409" => Err(error(format!("channel {channel} is already live from another source"))),
        "401" => Err(error("the sidecar refused VIZRS_INGEST_TOKEN".into())),
        "404" => Err(error("live ingest is off in the sidecar (no ingest_token)".into())),
        status => Err(error(format!("sidecar: HTTP {status}"))),
    }
}

/// One publisher: ffmpeg's MPEG-TS to the sidecar until it ends or goes
/// silent; false when none came (ffmpeg ended while listening).
fn serve(settings: &Settings, listener: &Listener, child: &mut Child) -> bool {
    let channel = &listener.channel;
    let Some(mut stdout) = child.stdout.take() else { return false };
    let (sender, receiver) = mpsc::sync_channel::<Vec<u8>>(256);
    std::thread::spawn(move || {
        let mut buffer = vec![0u8; 64 * 1024];
        while let Ok(n) = stdout.read(&mut buffer) {
            if n == 0 || sender.send(buffer[..n].to_vec()).is_err() {
                return;
            }
        }
    });
    // Listening: no limit until the first bytes.
    let Ok(first) = receiver.recv() else { return false };
    log(format_args!("channel {channel}: publisher on {}", listener.url));
    let mut sidecar: Option<TcpStream> = None;
    let mut retry = Instant::now();
    let mut pending = Some(first);
    loop {
        let data = match pending.take() {
            Some(data) => data,
            None => match receiver.recv_timeout(settings.idle) {
                Ok(data) => data,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    log(format_args!("channel {channel}: nothing for {} ms; publisher dropped", settings.idle.as_millis()));
                    return true;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return true,
            },
        };
        if sidecar.is_none() && Instant::now() >= retry {
            match open_source(settings, listener) {
                Ok(stream) => sidecar = Some(stream),
                Err(e) => {
                    log(format_args!("channel {channel}: {e}; retrying"));
                    retry = Instant::now() + AGAIN;
                }
            }
        }
        if let Some(stream) = sidecar.as_mut() {
            if let Err(e) = stream.write_all(&data) {
                log(format_args!("channel {channel}: sidecar: {e}; reconnecting"));
                sidecar = None;
                retry = Instant::now() + AGAIN;
            }
        }
    }
}

/// A running listener: asked to stop, its ffmpeg (0 = none now).
#[derive(Default)]
struct Running {
    stop: AtomicBool,
    pid: AtomicU32,
}

impl Running {
    fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        let pid = self.pid.load(Ordering::Relaxed);
        if pid != 0 {
            crate::sys::send_signal(pid, crate::sys::SIGTERM);
        }
    }
}

/// ffmpeg of an input, its pid in `running`; its messages go on to the log
/// and, for an RTMP listener expecting `key`, a publisher with another stream
/// name is cut off (the flag says so).
fn spawn_ffmpeg(
    settings: &Settings,
    args: Vec<OsString>,
    listener: &Listener,
    running: &Running,
    key: bool,
) -> Option<(Child, Arc<AtomicBool>)> {
    let mut command = Command::new(&settings.ffmpeg);
    command.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    // SAFETY: only async-signal-safe calls in the forked child. ffmpeg goes
    // with this process: a listener left behind would keep the port.
    unsafe {
        command.pre_exec(|| crate::sys::ffmpeg_child_setup(-1));
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            log(format_args!("{}: {e}", settings.ffmpeg.to_string_lossy()));
            return None;
        }
    };
    running.pid.store(child.id(), Ordering::Relaxed);
    if running.stop.load(Ordering::Relaxed) {
        running.stop();
    }
    let wrong = Arc::new(AtomicBool::new(false));
    if let Some(stderr) = child.stderr.take() {
        let (wrong, pid, channel) = (Arc::clone(&wrong), child.id(), listener.channel.clone());
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if key && line.contains("Unexpected stream") {
                    wrong.store(true, Ordering::Relaxed);
                    crate::sys::send_signal(pid, crate::sys::SIGTERM);
                    log(format_args!("channel {channel}: publisher with a wrong stream key refused"));
                    continue;
                }
                log(format_args!("ffmpeg (channel {channel}): {line}"));
            }
        });
    }
    Some((child, wrong))
}

/// One listener until stopped: ffmpeg listens, a publisher comes, goes, ffmpeg listens again.
fn listen(settings: &Settings, listener: &Listener, running: &Running) {
    match kind_of(&listener.url) {
        Some(Kind::YouTube) => return pull(settings, listener, running),
        Some(Kind::Link) => return fetch(settings, listener, running),
        _ => {}
    }
    let key = listener.url.to_ascii_lowercase().starts_with("rtmp") && rtmp_key(&listener.url).is_some();
    while !running.stop.load(Ordering::Relaxed) {
        let Some((mut child, wrong)) = spawn_ffmpeg(settings, listen_args(&listener.url), listener, running, key) else {
            std::thread::sleep(AGAIN * 5);
            continue;
        };
        let served = serve(settings, listener, &mut child);
        let _ = child.kill();
        let _ = child.wait();
        running.pid.store(0, Ordering::Relaxed);
        if running.stop.load(Ordering::Relaxed) {
            log(format_args!("channel {}: stopped listening on {}", listener.channel, listener.url));
            return;
        }
        if served && !wrong.load(Ordering::Relaxed) {
            log(format_args!("channel {}: publisher gone; listening on {}", listener.channel, listener.url));
        }
        std::thread::sleep(AGAIN);
    }
}

/// yt-dlp's arguments: the live status, then the stream URLs (video, audio), one a line.
pub fn ytdlp_args(url: &str, cookies: Option<&std::ffi::OsStr>) -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "--ignore-config",
        "--no-warnings",
        "--no-playlist",
        "--no-cache-dir",
        "--socket-timeout",
        "15",
        "-f",
        YOUTUBE_FORMAT,
        "--print",
        "live_status",
        "--print",
        "urls",
    ]
    .iter()
    .map(OsString::from)
    .collect();
    if let Some(file) = cookies {
        args.extend(["--cookies".into(), file.to_owned()]);
    }
    args.extend(["--".into(), url.into()]);
    args
}

/// A YouTube input's stream as yt-dlp found it.
#[derive(Debug, PartialEq)]
pub struct Found {
    /// A broadcast now (else a recording or a video).
    pub live: bool,
    /// One URL (video with audio) or two (video, audio).
    pub urls: Vec<String>,
}

/// yt-dlp's output (`ytdlp_args`).
pub fn parse_found(output: &str) -> Result<Found, String> {
    let mut lines = output.lines().map(str::trim).filter(|l| !l.is_empty());
    let status = lines.next().ok_or("yt-dlp gave nothing")?;
    let urls: Vec<String> = lines.filter(|l| l.starts_with("https://") || l.starts_with("http://")).take(2).map(str::to_owned).collect();
    if urls.is_empty() {
        return Err(format!("yt-dlp gave no stream ({status})"));
    }
    Ok(Found { live: status == "is_live", urls })
}

/// The reason in yt-dlp's messages: the last `ERROR:` line, without the extractor and video ID.
fn ytdlp_error(stderr: &str) -> String {
    let line = stderr.lines().rev().find(|l| l.starts_with("ERROR:")).or_else(|| stderr.lines().rev().find(|l| !l.trim().is_empty()));
    let line = line.unwrap_or("no message").trim_start_matches("ERROR:").trim();
    let line = if line.starts_with('[') { line.split_once(": ").map_or(line, |(_, rest)| rest) } else { line };
    line.chars().take(300).collect()
}

/// ffmpeg's arguments for a YouTube stream: read (a recording in real time,
/// from second `from`) and remuxed to MPEG-TS on stdout like a publisher.
pub fn pull_args(found: &Found, from: f64) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-hide_banner", "-nostdin", "-nostats", "-loglevel", "warning"].iter().map(OsString::from).collect();
    for url in &found.urls {
        if !found.live {
            args.push("-re".into());
            if from >= 1.0 {
                args.extend(["-ss".into(), format!("{from:.1}").into()]);
            }
        }
        args.extend(["-reconnect", "1", "-reconnect_streamed", "1", "-reconnect_delay_max", "5"].iter().map(OsString::from));
        args.extend(["-i".into(), url.into()]);
    }
    let audio = if found.urls.len() > 1 { "1:a:0?" } else { "0:a:0?" };
    let out =
        ["-map", "0:v:0", "-map", audio, "-c", "copy", "-flush_packets", "1", "-f", "mpegts", "-mpegts_flags", "+resend_headers", "pipe:1"];
    args.extend(out.iter().map(OsString::from));
    args
}

/// Waits `time` or until the input is stopped; true = stopped.
fn wait(running: &Running, time: Duration) -> bool {
    let end = Instant::now() + time;
    while Instant::now() < end {
        if running.stop.load(Ordering::Relaxed) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(250).min(end - Instant::now()));
    }
    running.stop.load(Ordering::Relaxed)
}

/// The cookies yt-dlp gets for a YouTube input: `VIZRS_YTDLP_COOKIES` as it
/// is, else a private copy of the file VizRS Live uploads (yt-dlp writes the
/// file it reads, and two inputs at once must not write the same one), made
/// again whenever the panel's file changes.
pub struct Cookies {
    panel: Option<std::path::PathBuf>,
    copy: std::path::PathBuf,
    /// Length and time of the panel's file the copy was made of.
    seen: Option<(u64, std::time::SystemTime)>,
}

/// What a cookies file is now: length and time of change; none = no file.
fn signature(path: &std::path::Path) -> Option<(u64, std::time::SystemTime)> {
    let meta = std::fs::metadata(path).ok().filter(std::fs::Metadata::is_file)?;
    Some((meta.len(), meta.modified().ok()?))
}

impl Cookies {
    pub fn new(settings: &Settings, copy: std::path::PathBuf) -> Cookies {
        Cookies { panel: settings.cookies.is_none().then(|| settings.panel_cookies.clone()), copy, seen: None }
    }

    /// The file for yt-dlp now, and a line for the log when that changed.
    pub fn file(&mut self, settings: &Settings) -> (Option<OsString>, Option<String>) {
        let Some(panel) = &self.panel else { return (settings.cookies.clone(), None) };
        let now = signature(panel);
        if now == self.seen {
            return (now.map(|_| self.copy.clone().into_os_string()), None);
        }
        let was = self.seen.take();
        let Some(now) = now else {
            let _ = std::fs::remove_file(&self.copy);
            return (None, was.map(|_| "YouTube cookies removed in VizRS Live".to_owned()));
        };
        // A copy with the panel file's mode (0600: it is a sign-in).
        let _ = std::fs::remove_file(&self.copy);
        match std::fs::copy(panel, &self.copy) {
            Ok(_) => {
                self.seen = Some(now);
                (Some(self.copy.clone().into_os_string()), Some(format!("YouTube cookies from VizRS Live ({} bytes)", now.0)))
            }
            Err(e) => (None, Some(format!("YouTube cookies from VizRS Live: {e}"))),
        }
    }

    /// The panel's file is not what the copy was made of (uploaded, replaced, removed).
    pub fn changed(&self) -> bool {
        self.panel.as_deref().is_some_and(|panel| signature(panel) != self.seen)
    }
}

impl Drop for Cookies {
    fn drop(&mut self) {
        if self.panel.is_some() {
            let _ = std::fs::remove_file(&self.copy);
        }
    }
}

/// YouTube's refusal of an address it takes for a bot, with what helps.
pub fn sign_in_hint(error: &str, cookies: bool) -> &'static str {
    if !error.contains("Sign in to confirm") && !error.contains("--cookies") {
        ""
    } else if cookies {
        " (with the cookies: they may have expired or the account signed out – upload new ones in VizRS Live)"
    } else {
        " (upload the cookies of a signed-in account in VizRS Live: Sloty na żywo, Ciasteczka YouTube)"
    }
}

/// Waits like `wait`, but ends early when the cookies change (asked again at once with them).
fn wait_or_cookies(running: &Running, time: Duration, cookies: &Cookies) -> bool {
    let end = Instant::now() + time;
    while Instant::now() < end {
        if wait(running, Duration::from_secs(1).min(end - Instant::now())) {
            return true;
        }
        if cookies.changed() {
            return false;
        }
    }
    running.stop.load(Ordering::Relaxed)
}

/// yt-dlp for a YouTube input, at most `RESOLVE_LIMIT`.
fn resolve(settings: &Settings, url: &str, cookies: Option<&std::ffi::OsStr>, running: &Running) -> Result<Found, String> {
    let mut command = Command::new(&settings.ytdlp);
    command.args(ytdlp_args(url, cookies)).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    // SAFETY: only async-signal-safe calls in the forked child.
    unsafe {
        command.pre_exec(|| crate::sys::ffmpeg_child_setup(-1));
    }
    let mut child = command.spawn().map_err(|e| format!("{}: {e}", settings.ytdlp.to_string_lossy()))?;
    running.pid.store(child.id(), Ordering::Relaxed);
    let read = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(pipe) = pipe {
                let _ = pipe.take(1 << 20).read_to_string(&mut text);
            }
            text
        })
    };
    let stdout = read(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let stderr = read(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if start.elapsed() < RESOLVE_LIMIT && !running.stop.load(Ordering::Relaxed) => {
                std::thread::sleep(Duration::from_millis(200))
            }
            _ => {
                // The standalone yt-dlp is a launcher with Python as its child: SIGTERM reaches both.
                crate::sys::send_signal(child.id(), crate::sys::SIGTERM);
                std::thread::sleep(Duration::from_secs(3));
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    running.pid.store(0, Ordering::Relaxed);
    let (stdout, stderr) = (stdout.join().unwrap_or_default(), stderr.join().unwrap_or_default());
    match status {
        None if running.stop.load(Ordering::Relaxed) => Err("stopped".into()),
        None => Err(format!("yt-dlp gave no answer in {} s", RESOLVE_LIMIT.as_secs())),
        Some(status) if !status.success() => Err(ytdlp_error(&stderr)),
        Some(_) => parse_found(&stdout),
    }
}

/// A YouTube video is over (a broadcast ended, a recording played): the
/// sidecar takes the input off its list (`POST /ingest/listeners/done`), so
/// that it is not played again; then it waits to be stopped.
fn done(settings: &Settings, listener: &Listener, running: &Running) {
    let headers = format!("Authorization: Bearer {}\r\n", settings.token);
    let target =
        format!("{}/ingest/listeners/done?channel={}&url={}", settings.sidecar, quote_plus(&listener.channel), quote_plus(&listener.url));
    match crate::net::request("POST", &target, &headers, Some("")) {
        Ok((200, _)) => log(format_args!("channel {}: YouTube input {} done, off the list", listener.channel, listener.url)),
        Ok((404, _)) => log(format_args!(
            "channel {}: YouTube input {} done; the sidecar cannot take it off its list (older version), it plays again only after a restart of this service",
            listener.channel, listener.url
        )),
        Ok((status, _)) => log(format_args!("channel {}: YouTube input {} done; sidecar: HTTP {status}", listener.channel, listener.url)),
        Err(e) => log(format_args!("channel {}: YouTube input {} done; sidecar: {e}", listener.channel, listener.url)),
    }
    while !wait(running, POLL) {}
}

/// A YouTube input until stopped: yt-dlp finds the stream, ffmpeg reads it and
/// it goes to the sidecar like a publisher. A channel's live page waits for
/// each broadcast and stays; one video is played once: a broadcast until it
/// ends, a recording to its end (a read that breaks off goes on where it was).
fn pull(settings: &Settings, listener: &Listener, running: &Running) {
    let (channel, url) = (&listener.channel, &listener.url);
    let page = youtube_channel_live(url);
    let mut seen_live = false;
    let mut played = 0.0f64;
    let mut last = String::new();
    let mut note = |message: String| {
        if message != last {
            log(&message);
            last = message;
        }
    };
    let mut cookies = Cookies::new(settings, std::env::temp_dir().join(format!("vizrs-ytdlp-cookies-{}.txt", input_id(listener))));
    while !running.stop.load(Ordering::Relaxed) {
        let (file, change) = cookies.file(settings);
        if let Some(change) = change {
            log(format_args!("channel {channel}: YouTube {url}: {change}"));
        }
        let found = match resolve(settings, url, file.as_deref(), running) {
            Ok(found) => found,
            Err(e) => {
                if !running.stop.load(Ordering::Relaxed) {
                    let hint = sign_in_hint(&e, file.is_some());
                    note(format!("channel {channel}: YouTube {url}: {e}{hint}; asking again every {} s", YOUTUBE_AGAIN.as_secs()));
                }
                if wait_or_cookies(running, YOUTUBE_AGAIN, &cookies) {
                    break;
                }
                continue;
            }
        };
        if !found.live && page {
            note(format!("channel {channel}: YouTube {url}: no broadcast now; asking again every {} s", YOUTUBE_AGAIN.as_secs()));
            if wait(running, YOUTUBE_AGAIN) {
                break;
            }
            continue;
        }
        if !found.live && seen_live {
            log(format_args!("channel {channel}: YouTube broadcast {url} ended"));
            return done(settings, listener, running);
        }
        note(if found.live {
            format!("channel {channel}: YouTube broadcast {url}")
        } else {
            format!("channel {channel}: YouTube recording {url} from {played:.0} s")
        });
        let started = Instant::now();
        let Some((mut child, _)) = spawn_ffmpeg(settings, pull_args(&found, played), listener, running, false) else {
            if wait(running, YOUTUBE_AGAIN) {
                break;
            }
            continue;
        };
        let served = serve(settings, listener, &mut child);
        let _ = child.kill();
        let finished = child.wait().is_ok_and(|s| s.success());
        running.pid.store(0, Ordering::Relaxed);
        if running.stop.load(Ordering::Relaxed) {
            break;
        }
        if found.live {
            seen_live |= served;
        } else if served && finished {
            log(format_args!("channel {channel}: YouTube recording {url} played to the end"));
            return done(settings, listener, running);
        } else if served {
            // Real time: the seconds of wall clock are the seconds played; a little again for the keyframe.
            played = (played + started.elapsed().as_secs_f64() - 2.0).max(0.0);
        }
        if wait(running, if served { AGAIN } else { YOUTUBE_AGAIN }) {
            break;
        }
    }
    log(format_args!("channel {channel}: stopped reading YouTube {url}"));
}

/// A link input until stopped: ffmpeg reads the stream and it goes to the
/// sidecar like a publisher; when it ends (a broadcast over, a server gone)
/// it is read again, so the input stays on the list like a listener.
fn fetch(settings: &Settings, listener: &Listener, running: &Running) {
    let (channel, url) = (&listener.channel, &listener.url);
    let mut failing = false;
    while !running.stop.load(Ordering::Relaxed) {
        let Some((mut child, _)) = spawn_ffmpeg(settings, link_args(url), listener, running, false) else {
            if wait(running, LINK_AGAIN) {
                break;
            }
            continue;
        };
        let served = serve(settings, listener, &mut child);
        let _ = child.kill();
        let _ = child.wait();
        running.pid.store(0, Ordering::Relaxed);
        if running.stop.load(Ordering::Relaxed) {
            break;
        }
        if served {
            log(format_args!("channel {channel}: stream {url} ended; reading it again"));
        } else if !failing {
            log(format_args!("channel {channel}: no stream from {url}; trying again every {} s", LINK_AGAIN.as_secs()));
        }
        failing = !served;
        if wait(running, if served { AGAIN } else { LINK_AGAIN }) {
            break;
        }
    }
    log(format_args!("channel {channel}: stopped reading {url}"));
}

/// This program's file: replaced (a new inode at its path) = a new release installed.
struct Program {
    path: Option<std::path::PathBuf>,
    inode: Option<u64>,
}

impl Program {
    fn now() -> Self {
        let path = std::env::current_exe().ok();
        let inode = path.as_deref().and_then(inode_of);
        Program { path, inode }
    }

    fn replaced(&self) -> bool {
        match (&self.path, self.inode) {
            (Some(path), Some(was)) => inode_of(path).is_some_and(|now| now != was),
            _ => false,
        }
    }
}

fn inode_of(path: &std::path::Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| m.ino())
}

/// The sidecar's list (`GET /ingest/listeners`); `None` = an older sidecar without it.
fn sidecar_listeners(settings: &Settings) -> io::Result<Option<Vec<Listener>>> {
    let headers = format!("Authorization: Bearer {}\r\n", settings.token);
    match crate::net::request("GET", &format!("{}/ingest/listeners", settings.sidecar), &headers, None)? {
        (200, body) => {
            let (listeners, errors) = parse_listeners(&body);
            for e in errors {
                log(format_args!("sidecar list: {e}"));
            }
            Ok(Some(listeners))
        }
        (404, _) => Ok(None),
        (401, _) => Err(error("the sidecar refused VIZRS_INGEST_TOKEN".into())),
        (status, _) => Err(error(format!("sidecar: HTTP {status}"))),
    }
}

/// `vizrs-ffmpeg vizrs-ingest`: runs the listeners, as the sidecar lists them, forever.
pub fn run(settings: Settings) -> i32 {
    let settings = Arc::new(settings);
    let mut running: Vec<(Listener, Arc<Running>)> = Vec::new();
    let mut last_error = String::new();
    let program = Program::now();
    loop {
        // The sidecar of a new release puts a new `vizrs-ffmpeg` in the volume: the
        // container (restart: unless-stopped) starts again with it. Until 2026-11-19
        // the service ran the old one until a restart by hand – Portainer recreates
        // only containers whose image changed (S1, S2: links refused for an hour).
        if program.replaced() {
            log(format_args!("a new vizrs-ffmpeg in the volume (the sidecar updated it): restarting"));
            return 0;
        }
        let wanted = match sidecar_listeners(&settings) {
            Ok(Some(list)) => Some(list),
            Ok(None) => Some(settings.listeners.clone()),
            Err(e) => {
                if e.to_string() != last_error {
                    log(format_args!("{e}; listeners stay as they are"));
                    last_error = e.to_string();
                }
                // Before the sidecar answers once: VIZRS_INGEST.
                running.is_empty().then(|| settings.listeners.clone())
            }
        };
        if let Some(wanted) = wanted {
            running.retain(|(listener, handle)| {
                let keep = wanted.contains(listener);
                if !keep {
                    handle.stop();
                }
                keep
            });
            for listener in wanted {
                if running.iter().any(|(l, _)| *l == listener) {
                    continue;
                }
                let doing = if matches!(kind_of(&listener.url), Some(Kind::Rtmp | Kind::Srt)) { "listening on" } else { "reading" };
                log(format_args!("channel {}: {doing} {}", listener.channel, listener.url));
                let handle = Arc::new(Running::default());
                running.push((listener.clone(), Arc::clone(&handle)));
                let settings = Arc::clone(&settings);
                std::thread::spawn(move || listen(&settings, &listener, &handle));
            }
        }
        std::thread::sleep(POLL);
    }
}

#[cfg(test)]
mod tests {
    use super::Program;

    #[test]
    fn a_program_replaced_by_rename_is_seen() {
        let dir = std::env::temp_dir().join(format!("vizrs-program-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("vizrs-ffmpeg");
        std::fs::write(&file, "old").unwrap();
        let program = Program { path: Some(file.clone()), inode: super::inode_of(&file) };
        assert!(!program.replaced());
        std::fs::write(dir.join(".new"), "new").unwrap();
        std::fs::rename(dir.join(".new"), &file).unwrap();
        assert!(program.replaced());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    use super::*;

    #[test]
    fn listeners() {
        let (list, errors) =
            parse_listeners("1=rtmp://0.0.0.0:1936/live/abc; 2=srt://0.0.0.0:9000?passphrase=x,y\n3=ftp://a 4 =rtmp://x/a/b");
        assert_eq!(
            list,
            [
                Listener { channel: "1".into(), url: "rtmp://0.0.0.0:1936/live/abc".into() },
                Listener { channel: "2".into(), url: "srt://0.0.0.0:9000?passphrase=x,y".into() },
            ]
        );
        assert_eq!(errors.len(), 3, "{errors:?}");
        let (_, errors) = parse_listeners("1=rtmp://h:1/a/b 2=rtmp://h:1/a/b");
        assert_eq!(errors, ["\"2=rtmp://h:1/a/b\": the same URL twice"]);
    }

    #[test]
    fn ffmpeg_listens_and_remuxes() {
        let text = |url| listen_args(url).iter().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>().join(" ");
        assert_eq!(
            text("rtmp://0.0.0.0:1936/live/k"),
            "-hide_banner -nostdin -nostats -loglevel warning -listen 1 -i rtmp://0.0.0.0:1936/live/k \
             -map 0:v:0 -map 0:a:0? -c copy -flush_packets 1 -f mpegts -mpegts_flags +resend_headers pipe:1"
        );
        assert!(text("srt://0.0.0.0:9000").contains("-analyzeduration 2000000 -i srt://0.0.0.0:9000?mode=listener -map"));
        assert!(text("srt://0.0.0.0:9000?latency=200000").contains("-i srt://0.0.0.0:9000?latency=200000&mode=listener -map"));
        assert!(!text("srt://0.0.0.0:9000").contains("-listen"));
        assert_eq!(rtmp_key("rtmp://0.0.0.0:1936/live/k"), Some("k"));
        assert_eq!(rtmp_key("rtmp://0.0.0.0:1936/live"), None);
    }

    #[test]
    fn listeners_from_the_panel() {
        let check = |text: &str| check_listeners(&parse_listeners(text).0);
        assert_eq!(check("1=rtmp://0.0.0.0:1936/live/a 2=srt://0.0.0.0:9001?passphrase=0123456789"), None);
        assert!(check("1=rtmp://0.0.0.0:1935/live/a").unwrap().contains("port spoza"));
        assert!(check("1=rtmp://0.0.0.0:1936/live/a 2=rtmp://0.0.0.0:1936/live/b").unwrap().contains("zajęty"));
        assert!(check("1=rtmp://0.0.0.0:1936/live").unwrap().contains("bez klucza"));
        assert!(check("1=srt://0.0.0.0:9000?passphrase=krotkie").unwrap().contains("10–79"));
        assert_eq!(check("1=rtmp://0.0.0.0:1936/live/a 1=srt://0.0.0.0:9000"), None);
    }

    #[test]
    fn publisher_settings() {
        use crate::json::Value;
        let get = |v: &Value, k: &str| v.get(k).map(|x| x.to_string()).unwrap_or_default();
        let rtmp = Listener { channel: "1".into(), url: "rtmp://0.0.0.0:1936/live/studio".into() };
        let v = publisher(&rtmp, "tv.example.pl", None, None);
        assert_eq!(
            (get(&v, "kind"), get(&v, "port"), get(&v, "app"), get(&v, "key")),
            ("\"rtmp\"".into(), "1936".into(), "\"/live\"".into(), "\"studio\"".into())
        );
        assert_eq!(get(&publisher(&rtmp, "", Some(1940), None), "port"), "1940");
        let second = Listener { channel: "2".into(), url: "rtmp://0.0.0.0:1937/live/b".into() };
        assert_eq!(get(&publisher(&second, "", Some(1940), None), "port"), "1937");
        let srt = Listener { channel: "1".into(), url: "srt://0.0.0.0:9000?passphrase=tajne-haslo-1&latency=200000".into() };
        let v = publisher(&srt, "", None, Some(9100));
        assert_eq!(
            (get(&v, "kind"), get(&v, "port"), get(&v, "passphrase"), get(&v, "key")),
            ("\"srt\"".into(), "9100".into(), "\"tajne-haslo-1\"".into(), "\"\"".into())
        );
    }

    #[test]
    fn input_id_is_stable_and_hides_the_address() {
        let (list, _) = parse_listeners("1=srt://0.0.0.0:9000?passphrase=0123456789; 2=srt://0.0.0.0:9001?passphrase=0123456789");
        let (a, b) = (input_id(&list[0]), input_id(&list[1]));
        assert_eq!(a.len(), 16);
        assert_ne!(a, b);
        assert_eq!(a, input_id(&list[0].clone()));
        assert!(!a.contains("0123456789"));
    }

    #[test]
    fn youtube_inputs() {
        assert_eq!(kind_of("https://www.youtube.com/watch?v=abc"), Some(Kind::YouTube));
        assert_eq!(kind_of("https://youtu.be/abc"), Some(Kind::YouTube));
        assert_eq!(kind_of("https://m.youtube.com/@kanal/live"), Some(Kind::YouTube));
        assert_eq!(kind_of("https://youtube.com/"), None);
        assert_eq!(kind_of("https://example.com/watch?v=abc"), Some(Kind::Link));
        assert_eq!(kind_of("https://notyoutube.com/watch?v=abc"), Some(Kind::Link));
        assert_eq!(kind_of("srt://0.0.0.0:9000"), Some(Kind::Srt));
        assert!(youtube_channel_live("https://www.youtube.com/@kanal/live"));
        assert!(youtube_channel_live("https://www.youtube.com/channel/UC123/live/"));
        assert!(!youtube_channel_live("https://www.youtube.com/live/abc123"));
        assert!(!youtube_channel_live("https://www.youtube.com/watch?v=abc"));
        let (list, errors) = parse_listeners("1=https://www.youtube.com/watch?v=abc&t=5 2=https://youtube.com/");
        assert_eq!(list, [Listener { channel: "1".into(), url: "https://www.youtube.com/watch?v=abc&t=5".into() }]);
        assert_eq!(errors.len(), 1);
        // No port: any number of YouTube inputs next to the listeners.
        assert_eq!(check_listeners(&parse_listeners("1=https://youtu.be/a 2=https://youtu.be/b 1=rtmp://0.0.0.0:1936/live/k").0), None);
        let v = publisher(&list[0], "", None, None);
        assert_eq!(v.get("kind").and_then(crate::json::Value::as_str), Some("youtube"));
        assert_eq!(v.get("youtube").and_then(crate::json::Value::as_str), Some("video"));
    }

    #[test]
    fn youtube_stream() {
        let args = ytdlp_args("https://youtu.be/a", None);
        let text: Vec<String> = args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(text[text.len() - 2..], ["--", "https://youtu.be/a"]);
        assert!(text.windows(2).any(|w| w == ["--print", "live_status"]));
        let live = parse_found("is_live\nhttps://v/1\nhttps://a/1\n").unwrap();
        assert_eq!(live, Found { live: true, urls: vec!["https://v/1".into(), "https://a/1".into()] });
        assert!(!parse_found("not_live\nhttps://av\n").unwrap().live);
        assert!(parse_found("is_upcoming\n").is_err());
        let text =
            |found: &Found, from| pull_args(found, from).iter().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>().join(" ");
        let l = text(&live, 0.0);
        assert!(!l.contains(" -re ") && l.contains("-map 0:v:0 -map 1:a:0? -c copy"), "{l}");
        let vod = Found { live: false, urls: vec!["https://av".into()] };
        let v = text(&vod, 61.25);
        assert!(v.contains("-re -ss 61.2 -reconnect 1") && v.contains("-map 0:a:0?"), "{v}");
        assert!(!text(&vod, 0.0).contains("-ss"));
        assert_eq!(
            ytdlp_error("WARNING: x\nERROR: [youtube] abc: This live event will begin in 3 hours.\n"),
            "This live event will begin in 3 hours."
        );
    }

    #[test]
    fn link_inputs() {
        assert_eq!(kind_of("https://cdn.example.pl/live/index.m3u8?token=x"), Some(Kind::Link));
        assert_eq!(kind_of("http://10.0.0.5:8080/stream.ts"), Some(Kind::Link));
        assert_eq!(kind_of("rtmp://studio.example.pl/live/abc"), Some(Kind::Link));
        assert_eq!(kind_of("rtmps://a.example.pl:443/app/k"), Some(Kind::Link));
        assert_eq!(kind_of("srt://relay.example.pl:9000?streamid=x"), Some(Kind::Link));
        assert_eq!(kind_of("rtsp://admin:p@192.168.1.9:554/h264"), Some(Kind::Link));
        assert_eq!(kind_of("rtmp://0.0.0.0:1936/live/k"), Some(Kind::Rtmp));
        assert_eq!(kind_of("srt://[::]:9000"), Some(Kind::Srt));
        assert_eq!(kind_of("rtmp://127.0.0.1:1936/live/k"), Some(Kind::Rtmp));
        assert_eq!(kind_of("srt://localhost:9000"), Some(Kind::Srt));
        assert_eq!(kind_of("http://127.0.0.1/x.m3u8"), None);
        assert_eq!(kind_of("rtmps://0.0.0.0:1936/live/k"), None);
        assert_eq!(kind_of("https://0.0.0.0/x.m3u8"), None);
        assert_eq!(kind_of("file:///etc/passwd"), None);
        assert_eq!(kind_of("https:///x.m3u8"), None);
        let check = |text: &str| check_listeners(&parse_listeners(text).0);
        assert_eq!(check("1=https://a.pl/x.m3u8 2=rtmp://b.pl/live/k 1=rtmp://0.0.0.0:1936/live/k"), None);
        assert!(check(&format!("1=https://a.pl/{}", "x".repeat(LINK_MAX))).unwrap().contains("dłuższy"));
        let v = publisher(&Listener { channel: "1".into(), url: "srt://relay.pl:9000".into() }, "", None, None);
        assert_eq!(v.get("kind").and_then(crate::json::Value::as_str), Some("link"));
        assert_eq!(v.get("scheme").and_then(crate::json::Value::as_str), Some("srt"));
        let text = |url: &str| link_args(url).iter().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>().join(" ");
        let hls = text("https://a.pl/x.m3u8");
        assert!(hls.contains("-protocol_whitelist http,https,tcp,tls,crypto,httpproxy -re -reconnect 1"), "{hls}");
        assert!(hls.contains("-i https://a.pl/x.m3u8 -map 0:v:0 -map 0:a:0? -c copy"), "{hls}");
        let rtmp = text("rtmp://b.pl/live/k");
        assert!(
            rtmp.contains("-protocol_whitelist rtmp,rtmps,tcp,tls -rw_timeout") && !rtmp.contains(" -re ") && !rtmp.contains("-listen"),
            "{rtmp}"
        );
        assert!(text("srt://r.pl:9000").contains("-i srt://r.pl:9000?mode=caller -map"));
        assert!(text("srt://r.pl:9000?latency=1").contains("-i srt://r.pl:9000?latency=1&mode=caller -map"));
        assert!(text("rtsp://c/h264").contains("-rtsp_transport tcp"));
        let name = |url: &str| label(&Listener { channel: "1".into(), url: url.into() });
        assert_eq!(name("rtmp://0.0.0.0:1937/live/tajny"), "RTMP 1937");
        assert_eq!(name("srt://0.0.0.0:9001?passphrase=tajne-haslo"), "SRT 9001");
        assert_eq!(name("https://www.youtube.com/@sejm/live"), "YouTube youtube.com/@sejm/live");
        assert_eq!(name("https://u:p@cdn.pl/a/index.m3u8?token=x"), "Link HLS cdn.pl");
        assert_eq!(name("rtmp://studio.pl/live/klucz"), "Link RTMP studio.pl");
    }

    #[test]
    fn cookies_from_the_panel_go_to_ytdlp_as_a_private_copy() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("vizrs-ingest-cookies-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let panel = dir.join("youtube-cookies.txt");
        let copy = dir.join("copy.txt");
        let settings = |cookies: Option<&str>| Settings {
            ffmpeg: "ffmpeg".into(),
            sidecar: String::new(),
            token: String::new(),
            idle: Duration::from_secs(1),
            listeners: Vec::new(),
            ytdlp: "yt-dlp".into(),
            cookies: cookies.map(OsString::from),
            panel_cookies: panel.clone(),
        };
        let own = settings(Some("/own/cookies.txt"));
        let mut cookies = Cookies::new(&own, copy.clone());
        assert_eq!(cookies.file(&own), (Some("/own/cookies.txt".into()), None), "VIZRS_YTDLP_COOKIES as it is");
        assert!(!cookies.changed());

        let from_panel = settings(None);
        let mut cookies = Cookies::new(&from_panel, copy.clone());
        assert_eq!(cookies.file(&from_panel), (None, None), "no file, nothing said");
        crate::yt_cookies::save(&dir, "one").unwrap();
        assert!(cookies.changed(), "uploaded: asked again at once");
        let (file, note) = cookies.file(&from_panel);
        assert_eq!(file, Some(copy.clone().into_os_string()));
        assert!(note.unwrap().contains("3 bytes"));
        assert_eq!(std::fs::read_to_string(&copy).unwrap(), "one");
        assert_eq!(std::fs::metadata(&copy).unwrap().permissions().mode() & 0o777, 0o600);
        std::fs::write(&copy, "refreshed by yt-dlp").unwrap();
        assert_eq!(cookies.file(&from_panel), (Some(copy.clone().into_os_string()), None), "yt-dlp's own changes stay");
        assert_eq!(std::fs::read_to_string(&copy).unwrap(), "refreshed by yt-dlp");
        crate::yt_cookies::save(&dir, "second").unwrap();
        assert!(cookies.changed());
        assert!(cookies.file(&from_panel).1.is_some());
        assert_eq!(std::fs::read_to_string(&copy).unwrap(), "second", "new ones from the panel win");
        crate::yt_cookies::remove(&dir).unwrap();
        assert_eq!(cookies.file(&from_panel), (None, Some("YouTube cookies removed in VizRS Live".into())));
        assert!(!copy.exists());
        crate::yt_cookies::save(&dir, "three").unwrap();
        cookies.file(&from_panel);
        drop(cookies);
        assert!(!copy.exists(), "the copy goes with the input");
        let _ = std::fs::remove_dir_all(&dir);

        let refused = "Sign in to confirm you’re not a bot. This helps protect our community. Use --cookies-from-browser or --cookies";
        assert!(sign_in_hint(refused, false).contains("upload the cookies"));
        assert!(sign_in_hint(refused, true).contains("expired"));
        assert_eq!(sign_in_hint("Video unavailable", false), "");
    }
}
