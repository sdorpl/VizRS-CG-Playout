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
//     VIZRS_YTDLP_COOKIES  a cookies file for yt-dlp (a server YouTube asks to sign in)
//
// A YouTube input (`1=https://www.youtube.com/…`) has no publisher: yt-dlp
// finds its stream, ffmpeg reads it and the rest goes as from a publisher.
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
/// A yt-dlp lookup that takes longer is stopped.
const RESOLVE_LIMIT: Duration = Duration::from_secs(90);
/// A YouTube input that is not on (a broadcast not started, a refused lookup) is asked again this often.
const YOUTUBE_AGAIN: Duration = Duration::from_secs(30);
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

/// What an input is: a listener a publisher comes to, or a YouTube address the
/// service fetches itself (yt-dlp finds the stream, ffmpeg reads it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Rtmp,
    Srt,
    YouTube,
}

/// The kind of an input URL; `None` = not one of them.
pub fn kind_of(url: &str) -> Option<Kind> {
    let parts = Url::split(url)?;
    match parts.scheme.as_str() {
        "rtmp" => Some(Kind::Rtmp),
        "srt" => Some(Kind::Srt),
        "http" | "https" => {
            let host = parts.netloc.rsplit('@').next().unwrap_or("").to_ascii_lowercase();
            let host = host.split(':').next().unwrap_or("");
            let host = ["www.", "m.", "music."].iter().find_map(|p| host.strip_prefix(p)).unwrap_or(host);
            (matches!(host, "youtube.com" | "youtu.be") && parts.path.len() > 1).then_some(Kind::YouTube)
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
            errors.push(format!("\"{entry}\": only rtmp:// and srt:// listeners and YouTube addresses"));
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
    if kind_of(&listener.url) == Some(Kind::YouTube) {
        return pull(settings, listener, running);
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

/// yt-dlp for a YouTube input, at most `RESOLVE_LIMIT`.
fn resolve(settings: &Settings, url: &str, running: &Running) -> Result<Found, String> {
    let mut command = Command::new(&settings.ytdlp);
    command.args(ytdlp_args(url, settings.cookies.as_deref())).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
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
    while !running.stop.load(Ordering::Relaxed) {
        let found = match resolve(settings, url, running) {
            Ok(found) => found,
            Err(e) => {
                if !running.stop.load(Ordering::Relaxed) {
                    note(format!("channel {channel}: YouTube {url}: {e}; asking again every {} s", YOUTUBE_AGAIN.as_secs()));
                }
                if wait(running, YOUTUBE_AGAIN) {
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
    loop {
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
                log(format_args!("channel {}: listening on {}", listener.channel, listener.url));
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
    use super::*;

    #[test]
    fn listeners() {
        let (list, errors) =
            parse_listeners("1=rtmp://0.0.0.0:1936/live/abc; 2=srt://0.0.0.0:9000?passphrase=x,y\n3=http://a 4 =rtmp://x/a/b");
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
        assert_eq!(kind_of("https://example.com/watch?v=abc"), None);
        assert_eq!(kind_of("https://notyoutube.com/watch?v=abc"), None);
        assert_eq!(kind_of("srt://0.0.0.0:9000"), Some(Kind::Srt));
        assert!(youtube_channel_live("https://www.youtube.com/@kanal/live"));
        assert!(youtube_channel_live("https://www.youtube.com/channel/UC123/live/"));
        assert!(!youtube_channel_live("https://www.youtube.com/live/abc123"));
        assert!(!youtube_channel_live("https://www.youtube.com/watch?v=abc"));
        let (list, errors) = parse_listeners("1=https://www.youtube.com/watch?v=abc&t=5 2=https://example.com/live");
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
}
