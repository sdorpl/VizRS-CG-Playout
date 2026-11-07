// Live ingest listeners: `vizrs-ffmpeg vizrs-ingest` (DOKUMENTACJA §1, Live
// ingest), run as a service of its own in the ErsatzTV image (its ffmpeg).
// Each listener is an ffmpeg that waits for a publisher (RTMP `-listen 1`,
// SRT `mode=listener`), remuxes it to MPEG-TS without transcoding and hands
// it to the sidecar (`POST /ingest?channel=N`); then it listens again.
//
//     VIZRS_INGEST         channel=URL, separated by ";" or white space:
//                          "1=rtmp://0.0.0.0:1936/live/KEY; 2=srt://0.0.0.0:9000?passphrase=…"
//     VIZRS_INGEST_TOKEN   shared secret with the sidecar (its ingest_token)
//     VIZRS_INGEST_IDLE_MS a publisher silent this long has ended (default 2000)
//     VIZRS_CG_URL         sidecar, e.g. http://cg:7700
//     VIZRS_FFMPEG         ffmpeg (default /usr/local/bin/ffmpeg)
//
// ffmpeg's RTMP server takes any stream name and only warns about one it
// does not expect: that publisher is cut off here, so the key is the password.

use std::ffi::OsString;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use crate::net::{Url, connect, error, read_head, status_of};
use crate::overlay::quote_plus;

const DEFAULT_FFMPEG: &str = "/usr/local/bin/ffmpeg";
const DEFAULT_IDLE_MS: u64 = 2000;
const AGAIN: Duration = Duration::from_secs(1);
const MAX_LISTENERS: usize = 16;

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
        let scheme = url.split("://").next().unwrap_or("").to_ascii_lowercase();
        if !matches!(scheme.as_str(), "rtmp" | "srt") || !url.contains("://") {
            errors.push(format!("\"{entry}\": only rtmp:// and srt:// listeners"));
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
/// "" = unknown (the panel puts in its own); a port given replaces the
/// listener's (compose maps the host port elsewhere).
pub fn publisher(listener: &Listener, host: &str, rtmp_port: Option<u16>, srt_port: Option<u16>) -> crate::json::Value {
    use crate::json;
    let url = Url::split(&listener.url);
    let kind = url.as_ref().map_or(String::new(), |u| u.scheme.to_ascii_lowercase());
    let own_port = url.as_ref().and_then(|u| u.netloc.rsplit_once(':')).and_then(|(_, p)| p.parse::<u16>().ok());
    let port = if kind == "rtmp" { rtmp_port.or(own_port).unwrap_or(1935) } else { srt_port.or(own_port).unwrap_or(0) };
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
}

/// The listeners' environment.
pub struct Settings {
    pub ffmpeg: OsString,
    pub sidecar: String,
    pub token: String,
    pub idle: Duration,
    pub listeners: Vec<Listener>,
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
        };
        if settings.sidecar.is_empty() {
            return Err("VIZRS_CG_URL is empty".into());
        }
        if settings.token.is_empty() {
            return Err("VIZRS_INGEST_TOKEN is empty".into());
        }
        if settings.listeners.is_empty() {
            return Err("VIZRS_INGEST has no listeners (channel=rtmp://… or channel=srt://…)".into());
        }
        Ok(settings)
    }
}

/// `POST /ingest` to the sidecar; the socket for the body once it accepted.
fn open_source(settings: &Settings, channel: &str) -> io::Result<TcpStream> {
    let url = Url::split(&settings.sidecar).ok_or_else(|| error(format!("bad sidecar URL {}", settings.sidecar)))?;
    let mut stream = connect(&url)?;
    let request = format!(
        "POST /ingest?channel={} HTTP/1.0\r\nHost: {}\r\nAuthorization: Bearer {}\r\nContent-Type: video/mp2t\r\n\r\n",
        quote_plus(channel),
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
            match open_source(settings, channel) {
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

/// One listener, forever: ffmpeg listens, a publisher comes, goes, ffmpeg listens again.
fn listen(settings: &Settings, listener: &Listener) -> ! {
    let key = listener.url.to_ascii_lowercase().starts_with("rtmp").then(|| rtmp_key(&listener.url)).flatten().map(str::to_owned);
    loop {
        let mut command = Command::new(&settings.ffmpeg);
        command.args(listen_args(&listener.url)).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        // SAFETY: only async-signal-safe calls in the forked child. ffmpeg goes
        // with this process: a listener left behind would keep the port.
        unsafe {
            command.pre_exec(|| crate::sys::ffmpeg_child_setup(-1));
        }
        let spawned = command.spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(e) => {
                log(format_args!("{}: {e}", settings.ffmpeg.to_string_lossy()));
                std::thread::sleep(AGAIN * 5);
                continue;
            }
        };
        // ffmpeg's messages go on to the log; a publisher with the wrong stream name is cut off.
        let wrong = Arc::new(AtomicBool::new(false));
        if let Some(stderr) = child.stderr.take() {
            let (wrong, pid, channel) = (Arc::clone(&wrong), child.id(), listener.channel.clone());
            let expecting = key.is_some();
            std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    if expecting && line.contains("Unexpected stream") {
                        wrong.store(true, Ordering::Relaxed);
                        crate::sys::send_signal(pid, crate::sys::SIGTERM);
                        log(format_args!("channel {channel}: publisher with a wrong stream key refused"));
                        continue;
                    }
                    log(format_args!("ffmpeg (channel {channel}): {line}"));
                }
            });
        }
        let served = serve(settings, listener, &mut child);
        let _ = child.kill();
        let _ = child.wait();
        if served && !wrong.load(Ordering::Relaxed) {
            log(format_args!("channel {}: publisher gone; listening on {}", listener.channel, listener.url));
        }
        std::thread::sleep(AGAIN);
    }
}

/// `vizrs-ffmpeg vizrs-ingest`: runs the listeners; returns only on a configuration error.
pub fn run(settings: Settings) -> i32 {
    let settings = Arc::new(settings);
    for listener in settings.listeners.clone() {
        log(format_args!("channel {}: listening on {}", listener.channel, listener.url));
        let settings = Arc::clone(&settings);
        std::thread::spawn(move || listen(&settings, &listener));
    }
    loop {
        std::thread::park();
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
        let srt = Listener { channel: "1".into(), url: "srt://0.0.0.0:9000?passphrase=tajne-haslo-1&latency=200000".into() };
        let v = publisher(&srt, "", None, Some(9100));
        assert_eq!(
            (get(&v, "kind"), get(&v, "port"), get(&v, "passphrase"), get(&v, "key")),
            ("\"srt\"".into(), "9100".into(), "\"tajne-haslo-1\"".into(), "\"\"".into())
        );
    }
}
