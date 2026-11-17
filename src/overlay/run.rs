//! Przebieg nakładki: zwykłe ffmpeg albo z grafiką, przejęcie anteny (live), sekwencja, przekazywanie sygnałów.

use super::*;

pub(crate) fn log(message: impl std::fmt::Display) {
    let _ = writeln!(io::stderr(), "vizrs-ffmpeg: {message}");
}

/// Reads the ErsatzTV canvas and throws it away until closed.
#[derive(Clone, Default)]
pub(crate) struct Drain {
    socket: Arc<Mutex<Option<TcpStream>>>,
    closed: Arc<Mutex<bool>>,
}

impl Drain {
    pub(crate) fn start(url: String, headers: Vec<(String, String)>) -> Drain {
        let drain = Drain::default();
        let this = drain.clone();
        std::thread::spawn(move || {
            let _ = this.run(&url, &headers);
        });
        drain
    }

    pub(crate) fn run(&self, url: &str, headers: &[(String, String)]) -> io::Result<()> {
        let url = Url::split(url).ok_or_else(|| error("bad canvas URL".into()))?;
        let mut stream = connect(&url)?;
        {
            let closed = self.closed.lock().unwrap_or_else(|e| e.into_inner());
            if *closed {
                return Ok(());
            }
            *self.socket.lock().unwrap_or_else(|e| e.into_inner()) = Some(stream.try_clone()?);
        }
        let mut request = format!("GET {} HTTP/1.1\r\nHost: {}\r\nAccept-Encoding: identity\r\n", url.target(), url.netloc);
        for (name, value) in headers {
            request.push_str(&format!("{name}: {value}\r\n"));
        }
        request.push_str("\r\n");
        stream.write_all(request.as_bytes())?;
        read_head(&mut stream, "canvas")?;
        stream.set_read_timeout(None)?;
        let mut buffer = vec![0u8; 1 << 16];
        while stream.read(&mut buffer)? > 0 {}
        Ok(())
    }

    pub(crate) fn close(&self) {
        let mut closed = self.closed.lock().unwrap_or_else(|e| e.into_inner());
        *closed = true;
        if let Some(socket) = self.socket.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let _ = socket.shutdown(Shutdown::Both);
        }
    }
}

/// `GET path` on the sidecar: status and body.
pub(crate) fn sidecar_get(sidecar: &str, path: &str) -> io::Result<(String, Vec<u8>)> {
    let url = Url::split(sidecar).ok_or_else(|| error(format!("bad sidecar URL {sidecar}")))?;
    let mut stream = connect(&url)?;
    stream.write_all(format!("GET {path} HTTP/1.0\r\nHost: {}\r\n\r\n", url.netloc).as_bytes())?;
    let mut reply = Vec::new();
    stream.read_to_end(&mut reply)?;
    let end = reply.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| error("sidecar: no reply".into()))?;
    let head = String::from_utf8_lossy(&reply[..end]);
    let status = status_of(head.lines().next().unwrap_or("")).to_owned();
    Ok((status, reply[end + 4..].to_vec()))
}

/// Socket positioned at the body of the sidecar's frame stream.
/// A frame stream of the sidecar: delta records (src/delta.rs; a sidecar that
/// knows them says so in `X-VizRS-Frames`) or whole frames (an older one).
pub struct FrameSocket {
    pub stream: TcpStream,
    pub delta: bool,
}

pub(crate) fn open_frames(sidecar: &str, path: &str) -> io::Result<FrameSocket> {
    let url = Url::split(sidecar).ok_or_else(|| error(format!("bad sidecar URL {sidecar}")))?;
    let mut stream = connect(&url)?;
    stream.write_all(format!("GET {path}&delta=1 HTTP/1.0\r\nHost: {}\r\n\r\n", url.netloc).as_bytes())?;
    let head = read_head_text(&mut stream, "frame stream")?;
    let line = head.lines().next().unwrap_or("");
    if status_of(line) != "200" {
        return Err(error(format!("frame stream: {line}")));
    }
    let delta = head.lines().any(|l| {
        l.split_once(':').is_some_and(|(name, value)| name.trim().eq_ignore_ascii_case("x-vizrs-frames") && value.trim() == "delta")
    });
    stream.set_read_timeout(None)?;
    stream.set_write_timeout(None)?;
    // ffmpeg blocks on this socket: a sidecar gone without closing it must not stop the channel.
    sys::keepalive(stream.as_raw_fd(), 3, 2, 4)?;
    Ok(FrameSocket { stream, delta })
}

/// The sidecar's session for the canvas channel and its frame stream. With
/// `place` (the command can overlay the graphics anywhere) the frames are
/// those of the graphics area, if the channel has one; `follow` = the piece
/// goes as runs that can widen the zones in its middle (PLAN-strefy S3b).
pub(crate) fn take_over(
    settings: &Settings,
    channel: &str,
    header_rate: &str,
    place: bool,
    follow: bool,
) -> io::Result<(Frames, FrameSocket)> {
    let area = match (place, follow) {
        (true, true) => "&area=1&follow=1",
        (true, false) => "&area=1",
        _ => "",
    };
    let path = format!("/ffmpeg?channel={}&wait={CLAIM_WAIT_MS}{area}", quote_plus(channel));
    let (status, body) = sidecar_get(&settings.sidecar, &path)?;
    if status != "200" {
        return Err(error(format!("no VizRS session on channel {channel} ({status})")));
    }
    let info = json::parse(&String::from_utf8_lossy(&body)).map_err(|e| error(e.to_string()))?;
    let field = |key| info.get(key).filter(|v| !v.is_null()).ok_or_else(|| error(format!("'{key}'")));
    let size = |key| field(key).and_then(|v| v.as_f64().map(|n| n as u32).ok_or_else(|| error(format!("'{key}'"))));
    let rate = match (settings.fps.as_str(), header_rate) {
        ("", "") => field("fps")?.to_string(),
        ("", rate) => rate.to_owned(),
        (fps, _) => fps.to_owned(),
    };
    let session = match field("session")? {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    let mut areas = Vec::new();
    if place {
        // `areas` (stacked in the frames); an older sidecar sends only `x`, `y`.
        areas = stacked(&info)?;
        if areas.is_empty() {
            areas.push((Area { x: size("x")?, y: size("y")?, w: size("width")?, h: size("height")? }, 0));
        }
    }
    let matrix = info.get("matrix").and_then(Value::as_str).unwrap_or_default().to_owned();
    let resume = info.get("resume").and_then(Value::as_str).unwrap_or_default().to_owned();
    let zones_rev = info.get("zones_rev").and_then(Value::as_f64).map(|n| n as u64).filter(|_| place);
    let ahead = info.get("ahead").and_then(Value::as_f64).map(|seconds| (std::time::Instant::now(), seconds));
    let frames =
        Frames { width: size("width")?, height: size("height")?, rate, matrix, areas, resume, session: session.clone(), ahead, zones_rev };
    let path = format!("/ffmpeg/frames?session={}&fps={}{area}", quote_plus(&session), quote_plus(&frames.rate));
    Ok((frames, open_frames(&settings.sidecar, &path)?))
}

/// Replaces this process with the real ffmpeg (at the forced rate, preset and catch-up rate, if any).
pub(crate) fn plain(settings: &Settings, args: Vec<OsString>) -> ! {
    let error = Command::new(&settings.ffmpeg).args(adjusted(args, settings, None)).exec();
    log(format_args!("{}: {error}", settings.ffmpeg.to_string_lossy()));
    std::process::exit(127)
}

/// The transcode to HLS that live ingest can take over, with the channel's
/// live state then (`/ingest/state`: on, audio); None without
/// `VIZRS_FFMPEG_INGEST=1`, with ingest off in the sidecar, or for a piece of
/// the schedule while the sidecar takes over only live programmes
/// (`ingest_anytime` off). A live programme (slot) always goes this way, even
/// with the sidecar away: its media is the sidecar.
pub(crate) fn live_job(settings: &Settings, args: &[OsString], channel: Option<&str>) -> Option<(Job, bool, bool)> {
    if !settings.ingest || settings.sidecar.is_empty() {
        return None;
    }
    let job = Job::of(args, channel)?;
    match takeover::state(&settings.sidecar, &job.channel) {
        Some(state) => (state.enabled && (job.slot || state.anytime)).then_some((job, state.live, state.audio)),
        None => job.slot.then_some((job, false, false)),
    }
}

/// The real ffmpeg with `args`, graphics at `fd`, the forwarded signals unblocked.
pub(crate) fn spawn(settings: &Settings, args: Vec<OsString>, fd: Option<i32>) -> io::Result<Child> {
    let mut command = Command::new(&settings.ffmpeg);
    command.args(args);
    let fd = fd.unwrap_or(-1);
    // SAFETY: only async-signal-safe calls in the forked child.
    unsafe {
        command.pre_exec(move || sys::ffmpeg_child_setup(fd));
    }
    command.spawn()
}

/// The live run of the piece (src/takeover.rs), adjusted as every command;
/// without a source the standby picture of a live programme.
pub(crate) fn live_command(
    settings: &Settings,
    args: &[OsString],
    job: &Job,
    elapsed: f64,
    source: Option<&takeover::Source>,
    graphics: Option<(&Frames, i32)>,
) -> Vec<OsString> {
    let full = graphics.filter(|(f, _)| f.areas.is_empty()).map(|(f, _)| (f.width, f.height));
    let rate = graphics.map_or("", |(f, _)| f.rate.as_str());
    let format = takeover::channel_format(&settings.ffmpeg, args, job, &settings.fps, full, rate);
    let input = graphics.map(|(frames, fd)| takeover::Graphics { input: raw_input(frames, fd), areas: &frames.areas });
    let picture = match source {
        Some(source) => takeover::Picture::Live(source),
        None => takeover::Picture::Standby(standby_image(settings)),
    };
    adjusted(takeover::live_args(args, job, elapsed, picture, &format, input.as_ref()), settings, None)
}

/// `VIZRS_FFMPEG_STANDBY` when the file is there; else black.
pub(crate) fn standby_image(settings: &Settings) -> Option<&str> {
    let path = settings.standby.as_str();
    if path.is_empty() {
        return None;
    }
    if Path::new(path).is_file() {
        return Some(path);
    }
    log(format_args!("VIZRS_FFMPEG_STANDBY {path}: no such file; black instead"));
    None
}

/// Runs the piece through `takeover::sequence`, the signals of ErsatzTV forwarded to the run of the moment.
pub(crate) fn sequence(
    settings: &Settings,
    job: &Job,
    on: bool,
    audio: bool,
    takeover: bool,
    run: &dyn Fn(f64, Option<&takeover::Source>) -> io::Result<Child>,
    restart: &dyn Fn() -> bool,
) -> i32 {
    let current = Arc::new(AtomicU32::new(0));
    let stopping = Arc::new(AtomicBool::new(false));
    sys::block_signals(&FORWARDED);
    {
        let (current, stopping) = (Arc::clone(&current), Arc::clone(&stopping));
        std::thread::spawn(move || {
            loop {
                let signal = sys::wait_signal(&FORWARDED);
                stopping.store(true, Ordering::Relaxed);
                let pid = current.load(Ordering::Relaxed);
                if pid != 0 {
                    sys::send_signal(pid, signal);
                }
            }
        });
    }
    let runner = takeover::Runner { sidecar: &settings.sidecar, run, current: &current, stopping: &stopping, takeover, restart };
    takeover::sequence(&runner, job, on, audio)
}

/// A frame stream of the session and the frames it carries, relayed to the runs.
pub(crate) struct Feed {
    frames: Frames,
    control: SinkControl,
}

/// The piece with the graphics of the sidecar as runs (live ingest, zones
/// widened in the middle of the piece): every run gets its own frame socket of
/// the same frame stream; zones widened give the next run a new frame stream.
#[allow(clippy::too_many_arguments)]
pub(crate) fn with_graphics(
    settings: &Settings,
    args: &[OsString],
    canvas: &Canvas,
    job: &Job,
    (on, audio): (bool, bool),
    (frames, socket, place): (Frames, FrameSocket, bool),
    drain: Drain,
    takeover: bool,
) -> i32 {
    let watch = frames.zones_rev.filter(|_| place).map(|rev| ZoneWatch::start(&settings.sidecar, &frames.session, rev));
    let feed = Mutex::new(relayed(settings, frames, socket, place));
    let run = |elapsed: f64, source: Option<&takeover::Source>| -> io::Result<Child> {
        let mut current = feed.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(zones) = watch.as_ref().and_then(ZoneWatch::take) {
            let path = format!(
                "/ffmpeg/frames?session={}&fps={}&area=1&rev={}&zones={}&at={elapsed:.3}",
                quote_plus(&current.frames.session),
                quote_plus(&current.frames.rate),
                zones.rev,
                quote_plus(&zones.geometry)
            );
            match open_frames(&settings.sidecar, &path) {
                Ok(socket) => {
                    let frames = Frames {
                        width: zones.width,
                        height: zones.height,
                        rate: current.frames.rate.clone(),
                        matrix: current.frames.matrix.clone(),
                        areas: zones.areas,
                        resume: if current.frames.resume.is_empty() { String::new() } else { zones.resume },
                        session: current.frames.session.clone(),
                        ahead: current.frames.ahead,
                        zones_rev: Some(zones.rev),
                    };
                    let shown = if zones.geometry.is_empty() { "the whole channel" } else { zones.geometry.as_str() };
                    log(format_args!("graphics zones {shown} from {elapsed:.1} s of the piece"));
                    current.control.close();
                    *current = relayed(settings, frames, socket, place);
                }
                Err(e) => log(format_args!("frame stream of the new graphics zones: {e}; the zones before stay")),
            }
        }
        let frames = &current.frames;
        let (reader, writer) = frame_channel()?;
        let fd = reader.as_raw_fd();
        let command = match source {
            Some(source) => live_command(settings, args, job, elapsed, Some(source), Some((frames, fd))),
            None if job.slot => live_command(settings, args, job, elapsed, None, Some((frames, fd))),
            None => adjusted(job.program(&overlay_args(args, canvas, fd, frames), elapsed), settings, frames.burst(elapsed)),
        };
        let child = spawn(settings, command, Some(fd))?;
        drop(reader);
        current.control.attach(writer);
        Ok(child)
    };
    // The run stops for the new zones: its graphics end first (the sidecar may hold them).
    let restart = || {
        let newer = watch.as_ref().is_some_and(ZoneWatch::newer);
        if newer {
            feed.lock().unwrap_or_else(|e| e.into_inner()).control.cut();
        }
        newer
    };
    let code = sequence(settings, job, on, audio, takeover, &run, &restart);
    // The relay stops at its next frame, closing the frame stream: the sidecar ends the script.
    feed.lock().unwrap_or_else(|e| e.into_inner()).control.close();
    std::thread::sleep(SCRIPT_END);
    drain.close();
    code
}

/// `socket` (a frame stream of `frames`) relayed to the runs, reconnected
/// after a restart of the sidecar.
pub(crate) fn relayed(settings: &Settings, frames: Frames, socket: FrameSocket, place: bool) -> Feed {
    let frame = frame_bytes(frames.width, frames.height);
    let (sink, control) = Sink::new(frame);
    let area = if place { "&area=1" } else { "" };
    let resume = (!frames.resume.is_empty()).then(|| format!("/ffmpeg/resume?{}&fps={}{area}", frames.resume, quote_plus(&frames.rate)));
    let sidecar = settings.sidecar.clone();
    let interval = frame_interval(&frames.rate);
    std::thread::spawn(move || {
        let reconnect = |path: &str| {
            let (sender, receiver) = mpsc::channel();
            let (sidecar, path) = (sidecar.clone(), path.to_owned());
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(RECONNECT);
                    if let Ok(socket) = open_frames(&sidecar, &path) {
                        let _ = sender.send(socket);
                        return;
                    }
                }
            });
            receiver
        };
        let again = resume.as_deref().map(|path| move || reconnect(path));
        relay(socket, sink, frame, interval, again.as_ref().map(|f| f as &dyn Fn() -> mpsc::Receiver<FrameSocket>));
    });
    Feed { frames, control }
}

/// The overlay: `vizrs-ffmpeg <ffmpeg arguments>`; returns the exit code.
pub fn run(settings: &Settings, args: Vec<OsString>) -> i32 {
    let canvas = find_canvas(&args).filter(|_| !settings.sidecar.is_empty());
    let live = live_job(settings, &args, canvas.as_ref().map(|c| c.channel.as_str()));
    // Not a canvas transcode: plain ffmpeg, at the forced rate, preset and catch-up rate when set.
    let Some(canvas) = canvas else {
        return match live {
            Some((job, on, audio)) => {
                let program = |elapsed: f64| adjusted(job.program(&args, elapsed), settings, None);
                let run = |elapsed: f64, source: Option<&takeover::Source>| match source {
                    Some(source) => spawn(settings, live_command(settings, &args, &job, elapsed, Some(source), None), None),
                    None if job.slot => spawn(settings, live_command(settings, &args, &job, elapsed, None, None), None),
                    None => spawn(settings, program(elapsed), None),
                };
                sequence(settings, &job, on, audio, true, &run, &|| false)
            }
            None => plain(settings, args),
        };
    };

    let headers = parse_headers(&args[canvas.start..canvas.index]);
    let rate = headers.iter().find(|(name, _)| name == "x-etv-frame-rate").map_or("", |(_, v)| v.as_str()).to_owned();
    let drain = Drain::start(args[canvas.index + 1].to_string_lossy().into_owned(), headers);

    let place = with_position(&args, input_number(&args, canvas.index), 2, 2).is_some();
    // Runs that can widen the zones: live, or the piece as runs (below).
    let runs = live.is_some() || (settings.ingest && Job::of(&args, Some(&canvas.channel)).is_some_and(|job| !job.slot));
    let (frames, socket) = match take_over(settings, &canvas.channel, &rate, place, runs) {
        Ok(taken) => taken,
        Err(e) => {
            log(format_args!("{e}; ErsatzTV graphics instead"));
            drain.close();
            plain(settings, args)
        }
    };

    if let Some((job, on, audio)) = live {
        return with_graphics(settings, &args, &canvas, &job, (on, audio), (frames, socket, place), drain, true);
    }
    // Zones that may widen in the middle of the piece (S3b): the piece as runs
    // too, with live left alone (VIZRS_FFMPEG_INGEST allows the runs).
    if settings.ingest && frames.zones_rev.is_some() {
        if let Some(job) = Job::of(&args, Some(&canvas.channel)).filter(|job| !job.slot) {
            return with_graphics(settings, &args, &canvas, &job, (false, false), (frames, socket, place), drain, false);
        }
    }
    let (reader, writer) = match frame_channel() {
        Ok(pair) => pair,
        Err(e) => {
            log(format_args!("frame socket: {e}; ErsatzTV graphics instead"));
            drain.close();
            plain(settings, args)
        }
    };
    let fd = reader.as_raw_fd();
    // Forwarded to ffmpeg from here on; ffmpeg_child_setup unblocks them in the child.
    sys::block_signals(&FORWARDED);
    let mut command = Command::new(&settings.ffmpeg);
    command.args(adjusted(overlay_args(&args, &canvas, fd, &frames), settings, frames.burst(0.0)));
    // SAFETY: only async-signal-safe calls in the forked child.
    unsafe {
        command.pre_exec(move || sys::ffmpeg_child_setup(fd));
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            log(format_args!("{}: {e}", settings.ffmpeg.to_string_lossy()));
            drain.close();
            return 127;
        }
    };
    drop(reader);
    let frame = frame_bytes(frames.width, frames.height);
    let area = if place { "&area=1" } else { "" };
    let resume = (!frames.resume.is_empty()).then(|| format!("/ffmpeg/resume?{}&fps={}{area}", frames.resume, quote_plus(&frames.rate)));
    let sidecar = settings.sidecar.clone();
    let interval = frame_interval(&frames.rate);
    std::thread::spawn(move || {
        let reconnect = |path: &str| {
            let (sender, receiver) = mpsc::channel();
            let (sidecar, path) = (sidecar.clone(), path.to_owned());
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(RECONNECT);
                    if let Ok(socket) = open_frames(&sidecar, &path) {
                        let _ = sender.send(socket);
                        return;
                    }
                }
            });
            receiver
        };
        let again = resume.as_deref().map(|path| move || reconnect(path));
        relay(socket, writer, frame, interval, again.as_ref().map(|f| f as &dyn Fn() -> mpsc::Receiver<FrameSocket>));
    });

    let pid = child.id();
    std::thread::spawn(move || {
        loop {
            sys::send_signal(pid, sys::wait_signal(&FORWARDED));
        }
    });

    let status = child.wait();
    // The sidecar ends the script when ffmpeg's frame stream closes; closing the
    // canvas before that makes ErsatzTV log a cancelled script element.
    std::thread::sleep(SCRIPT_END);
    drain.close();
    match status {
        Ok(status) => status.code().or_else(|| status.signal().map(|s| 128 + s)).unwrap_or(1),
        Err(_) => 1,
    }
}
