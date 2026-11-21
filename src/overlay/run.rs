//! A run of the overlay: plain ffmpeg or with graphics, live takeover, the sequence, forwarded signals.

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
    live: bool,
) -> io::Result<(Frames, FrameSocket)> {
    let area = match (place, follow) {
        (true, true) => "&area=1&follow=1",
        (true, false) => "&area=1",
        _ => "",
    };
    let own = if live { "&live=1" } else { "" };
    let path = format!("/ffmpeg?channel={}&wait={CLAIM_WAIT_MS}{area}{own}", quote_plus(channel));
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
/// without a source the standby picture of a live programme; `marks` – the
/// watermarks of a live programme's item on top.
pub(crate) fn live_command(
    settings: &Settings,
    args: &[OsString],
    job: &Job,
    elapsed: f64,
    source: Option<&takeover::Source>,
    graphics: Option<(&Frames, i32)>,
    marks: Option<&takeover::Marks>,
) -> Vec<OsString> {
    let full = graphics.filter(|(f, _)| f.areas.is_empty()).map(|(f, _)| (f.width, f.height));
    let rate = graphics.map_or("", |(f, _)| f.rate.as_str());
    let format = takeover::channel_format(&settings.ffmpeg, args, job, &settings.fps, full, rate);
    let input = graphics.map(|(frames, fd)| takeover::Graphics { input: raw_input(frames, fd), areas: &frames.areas });
    let picture = match source {
        Some(source) => takeover::Picture::Live(source),
        None => takeover::Picture::Standby(standby_image(settings)),
    };
    adjusted(takeover::live_args(args, job, elapsed, picture, &format, input.as_ref(), marks), settings, None)
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
    marks: &Watermarks,
) -> i32 {
    let watch = frames.zones_rev.filter(|_| place).map(|rev| ZoneWatch::start(&settings.sidecar, &frames.session, rev));
    // A live programme of the schedule keeps its item's watermarks on air; a takeover has its own (`live_watermark`).
    let programme_marks = job.slot.then(|| live_marks(marks, &frames.rate, &frames.matrix));
    let takeover_marks = TakeoverMarks::new(&frames.rate, &frames.matrix);
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
            Some(source) => {
                let own = if job.slot { programme_marks.clone() } else { takeover_marks.of(settings, &job.channel, source) };
                live_command(settings, args, job, elapsed, Some(source), Some((frames, fd)), own.as_ref())
            }
            None if job.slot => live_command(settings, args, job, elapsed, None, Some((frames, fd)), programme_marks.as_ref()),
            None => adjusted(job.program(&overlay_args(args, canvas, fd, frames, marks), elapsed), settings, frames.burst(elapsed)),
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

/// The graphics of the live runs of a piece whose item has no VizRS element:
/// a session of the overlay's own in the sidecar (`POST /etv?…&live=1`), only
/// the channel's graphics – the `on_live` of the takeover (a LIVE logo) – and
/// its frames relayed to each live run.
pub(crate) struct LiveGraphics {
    feed: Feed,
    session: TcpStream,
}

impl LiveGraphics {
    /// The session and its frames, `format` the channel's.
    pub(crate) fn start(settings: &Settings, channel: &str, format: &takeover::Format) -> io::Result<LiveGraphics> {
        let session = open_live_session(&settings.sidecar, channel, format)?;
        let (frames, socket) = match take_over(settings, channel, &format.rate, true, false, true) {
            Ok(taken) => taken,
            Err(e) => {
                let _ = session.shutdown(Shutdown::Both);
                return Err(e);
            }
        };
        Ok(LiveGraphics { feed: relayed(settings, frames, socket, true), session })
    }

    /// The frames end, then the session (the sidecar ends it with its reader).
    pub(crate) fn close(&self) {
        self.feed.control.close();
        let _ = self.session.shutdown(Shutdown::Both);
    }
}

/// `POST /etv?channel=N&live=1` with the channel's size and rate as template
/// data; its packets are read and thrown away (nobody draws them) until the
/// socket is shut down. The socket for that.
pub(crate) fn open_live_session(sidecar: &str, channel: &str, format: &takeover::Format) -> io::Result<TcpStream> {
    let url = Url::split(sidecar).ok_or_else(|| error(format!("bad sidecar URL {sidecar}")))?;
    let mut stream = connect(&url)?;
    let body = json::object([
        ("MediaItem_Title", "live ingest".into()),
        ("Resolution", json::object([("Width", u64::from(format.width).into()), ("Height", u64::from(format.height).into())])),
        ("FrameRate", Value::Number(rate_number(&format.rate))),
    ])
    .to_string();
    let head = format!(
        "POST /etv?channel={}&live=1 HTTP/1.0\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        quote_plus(channel),
        url.netloc,
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    let head = read_head_text(&mut stream, "live graphics session")?;
    let line = head.lines().next().unwrap_or("");
    if status_of(line) != "200" {
        return Err(error(format!("live graphics session: {line}")));
    }
    stream.set_read_timeout(None)?;
    let mut packets = stream.try_clone()?;
    std::thread::spawn(move || {
        let mut buffer = [0u8; 16 * 1024];
        while matches!(packets.read(&mut buffer), Ok(n) if n > 0) {}
    });
    Ok(stream)
}

/// A rate of ffmpeg (`25`, `30000/1001`) as a number; 25 when it is none.
pub(crate) fn rate_number(rate: &str) -> f64 {
    let value = match rate.split_once('/') {
        Some((n, d)) => n.trim().parse::<f64>().ok().zip(d.trim().parse::<f64>().ok()).map(|(n, d)| n / d),
        None => rate.trim().parse::<f64>().ok(),
    };
    value.filter(|v| v.is_finite() && *v > 0.0).unwrap_or(25.0)
}

/// A piece whose item has no VizRS element, which live ingest can take over:
/// the schedule as ErsatzTV made it (its watermarks too), the live runs with
/// the channel's live graphics (`LiveGraphics`) – the LIVE logo goes on air
/// with the studio whatever the item. They are set up in the background at the
/// first live run, which never waits for them: it starts without graphics and
/// is restarted with them once they are there (as for widened zones). Not to
/// be had (no sidecar session, an error): the live runs without graphics.
fn without_element(settings: &Settings, args: &[OsString], job: &Job, (on, audio): (bool, bool)) -> i32 {
    if job.slot {
        return programme_without_element(settings, args, job, (on, audio));
    }
    let graphics = PieceGraphics::default();
    let marks_of = OnceLock::new();
    let takeover_marks = |source: &takeover::Source| {
        marks_of
            .get_or_init(|| {
                let format = takeover::channel_format(&settings.ffmpeg, args, job, &settings.fps, None, "");
                TakeoverMarks::new(&format.rate, crate::yuva::ffmpeg_matrix(format.height))
            })
            .of(settings, &job.channel, source)
    };
    let run = |elapsed: f64, source: Option<&takeover::Source>| -> io::Result<Child> {
        let Some(source) = source else {
            graphics.bare.store(false, Ordering::Relaxed);
            return spawn(settings, adjusted(job.program(args, elapsed), settings, None), None);
        };
        let marks = takeover_marks(source);
        let ready = graphics.ready(settings, args, job);
        let Some(live) = ready.as_ref() else {
            graphics.bare.store(true, Ordering::Relaxed);
            return spawn(settings, live_command(settings, args, job, elapsed, Some(source), None, marks.as_ref()), None);
        };
        graphics.bare.store(false, Ordering::Relaxed);
        let (reader, writer) = frame_channel()?;
        let fd = reader.as_raw_fd();
        let child = spawn(
            settings,
            live_command(settings, args, job, elapsed, Some(source), Some((&live.feed.frames, fd)), marks.as_ref()),
            Some(fd),
        )?;
        drop(reader);
        live.feed.control.attach(writer);
        Ok(child)
    };
    // The studio already on air (a live going on from the piece before): the
    // graphics set up first, a short while at most, so that the piece's first
    // live run has them – not a restart (a blink of the logo) every piece.
    if on {
        let until = std::time::Instant::now() + LIVE_GRAPHICS_WAIT;
        while graphics.ready(settings, args, job).is_none() && graphics.setting_up() && std::time::Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    // A live run without graphics stops for the graphics that came meanwhile.
    let restart = || graphics.bare.load(Ordering::Relaxed) && graphics.ready(settings, args, job).is_some();
    let code = sequence(settings, job, on, audio, true, &run, &restart);
    graphics.close();
    code
}

/// A live programme of the schedule whose item has no VizRS element: no
/// takeover (its graphics are the item's – here none), the studio or the
/// standby picture with the item's watermarks on top, as ErsatzTV would draw
/// them (from the bridge, by the item of ErsatzTV's region canvas).
fn programme_without_element(settings: &Settings, args: &[OsString], job: &Job, (on, audio): (bool, bool)) -> i32 {
    let marks = programme_watermarks(settings, args, job);
    let run = |elapsed: f64, source: Option<&takeover::Source>| -> io::Result<Child> {
        spawn(settings, live_command(settings, args, job, elapsed, source, None, marks.as_ref()), None)
    };
    sequence(settings, job, on, audio, true, &run, &|| false)
}

/// The watermarks of a live programme's item with no VizRS element, as images
/// for its live runs; none without a region canvas, a bridge token or an answer in time.
fn programme_watermarks(settings: &Settings, args: &[OsString], job: &Job) -> Option<takeover::Marks> {
    let (canvas, url) = find_region_canvas(args)?;
    if settings.bridge_token.is_empty() {
        return None;
    }
    let marks = match fetch_watermarks(&url, &canvas, &settings.bridge_token).recv_timeout(BRIDGE_WAIT) {
        Ok(Ok(marks)) => marks,
        Ok(Err(e)) => {
            log(format_args!("watermarks of the live programme: {e}; none drawn"));
            return None;
        }
        Err(_) => {
            log("watermarks of the live programme: no answer from the bridge in time; none drawn");
            return None;
        }
    };
    if marks.list.is_empty() {
        return None;
    }
    log(format_args!("{} ErsatzTV watermark(s) of the live programme on its live runs", marks.list.len()));
    let format = takeover::channel_format(&settings.ffmpeg, args, job, &settings.fps, None, "");
    Some(live_marks(&marks, &format.rate, crate::yuva::ffmpeg_matrix(format.height)))
}

/// The ErsatzTV watermarks of a live takeover (`live_watermark` of the
/// channel or of its input, from the sidecar's state at each live run), from
/// VizRS Bridge – asked again only when the ids change (another input).
/// No token, no answer in time or an error: none, the takeover goes on without them.
struct TakeoverMarks {
    rate: String,
    matrix: String,
    last: Mutex<Option<(String, Option<takeover::Marks>)>>,
}

impl TakeoverMarks {
    fn new(rate: &str, matrix: &str) -> TakeoverMarks {
        TakeoverMarks { rate: rate.to_owned(), matrix: matrix.to_owned(), last: Mutex::new(None) }
    }

    fn of(&self, settings: &Settings, channel: &str, source: &takeover::Source) -> Option<takeover::Marks> {
        let ids = source.live_watermark.as_str();
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, marks)) = last.as_ref().filter(|(seen, _)| seen == ids) {
            return marks.clone();
        }
        let marks = if ids.is_empty() || settings.bridge_token.is_empty() {
            None
        } else {
            match watermarks_by_id(&settings.bridge_url, channel, ids, &settings.bridge_token) {
                Ok(marks) if !marks.list.is_empty() => {
                    log(format_args!("{} ErsatzTV watermark(s) of the live takeover ({ids})", marks.list.len()));
                    Some(live_marks(&marks, &self.rate, &self.matrix))
                }
                Ok(_) => {
                    log(format_args!("watermarks of the live takeover ({ids}): none of them has an image; none drawn"));
                    None
                }
                Err(e) => {
                    log(format_args!("watermarks of the live takeover ({ids}): {e}; none drawn"));
                    None
                }
            }
        };
        *last = Some((ids.to_owned(), marks.clone()));
        marks
    }
}

/// How long a piece starting with the studio on air waits for its live graphics.
const LIVE_GRAPHICS_WAIT: Duration = Duration::from_millis(1500);

/// The live graphics of one piece (`without_element`): asked for once, in the background.
#[derive(Default)]
struct PieceGraphics {
    /// The setup under way.
    pending: Mutex<Option<mpsc::Receiver<io::Result<LiveGraphics>>>>,
    /// Set up (`Some`), or given up for this piece (`asked` and none pending).
    graphics: Mutex<Option<LiveGraphics>>,
    asked: AtomicBool,
    /// The run of the moment is a live run without them.
    bare: AtomicBool,
}

impl PieceGraphics {
    /// The graphics when they are there; the first call starts setting them up.
    fn ready(&self, settings: &Settings, args: &[OsString], job: &Job) -> std::sync::MutexGuard<'_, Option<LiveGraphics>> {
        if !self.asked.swap(true, Ordering::Relaxed) {
            let format = takeover::channel_format(&settings.ffmpeg, args, job, &settings.fps, None, "");
            let (sender, receiver) = mpsc::channel();
            let (settings, format, channel) = (settings.clone(), format, job.channel.clone());
            std::thread::spawn(move || {
                let _ = sender.send(LiveGraphics::start(&settings, &channel, &format));
            });
            *self.pending.lock().unwrap_or_else(|e| e.into_inner()) = Some(receiver);
        }
        let mut graphics = self.graphics.lock().unwrap_or_else(|e| e.into_inner());
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        let answer = pending.as_ref().map(mpsc::Receiver::try_recv);
        match answer {
            Some(Ok(Ok(live))) => {
                log(format_args!("live graphics of channel {} on the live runs (no VizRS element on the item)", job.channel));
                *graphics = Some(live);
                *pending = None;
            }
            Some(Ok(Err(e))) => {
                log(format_args!("live graphics of channel {}: {e}; the live runs without graphics", job.channel));
                *pending = None;
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => *pending = None,
            Some(Err(mpsc::TryRecvError::Empty)) | None => {}
        }
        graphics
    }

    /// The graphics are still being set up.
    fn setting_up(&self) -> bool {
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).is_some()
    }

    /// The piece is over: its graphics end; graphics still being set up end when they come.
    fn close(&self) {
        if let Some(live) = self.graphics.lock().unwrap_or_else(|e| e.into_inner()).take() {
            live.close();
        }
        if let Some(pending) = self.pending.lock().unwrap_or_else(|e| e.into_inner()).take() {
            std::thread::spawn(move || {
                if let Ok(Ok(live)) = pending.recv() {
                    live.close();
                }
            });
        }
    }
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
            Some((job, on, audio)) => without_element(settings, &args, &job, (on, audio)),
            None => plain(settings, args),
        };
    };

    let headers = parse_headers(&args[canvas.start..canvas.index]);
    let rate = headers.iter().find(|(name, _)| name == "x-etv-frame-rate").map_or("", |(_, v)| v.as_str()).to_owned();
    let canvas_url = args[canvas.index + 1].to_string_lossy().into_owned();
    // The item's watermarks from the bridge, asked while the session is taken over.
    let watermarks = (!settings.bridge_token.is_empty()).then(|| fetch_watermarks(&canvas_url, &canvas, &settings.bridge_token));
    let drain = Drain::start(canvas_url, headers);

    let place = with_position(&args, input_number(&args, canvas.index), 2, 2).is_some();
    // Runs that can widen the zones: live, or the piece as runs (below).
    let runs = live.is_some() || (settings.ingest && Job::of(&args, Some(&canvas.channel)).is_some_and(|job| !job.slot));
    let (frames, socket) = match take_over(settings, &canvas.channel, &rate, place, runs, false) {
        Ok(taken) => taken,
        Err(e) => {
            log(format_args!("{e}; ErsatzTV graphics instead"));
            drain.close();
            plain(settings, args)
        }
    };

    let marks = match watermarks.map(|answer| answer.recv_timeout(BRIDGE_WAIT)) {
        Some(Ok(Ok(marks))) => {
            if !marks.list.is_empty() {
                log(format_args!("{} ErsatzTV watermark(s) on top of the graphics", marks.list.len()));
            }
            marks
        }
        Some(Ok(Err(e))) => {
            log(format_args!("watermarks: {e}; none drawn"));
            Watermarks::default()
        }
        Some(Err(_)) => {
            log("watermarks: no answer from the bridge in time; none drawn");
            Watermarks::default()
        }
        None => Watermarks::default(),
    };

    if let Some((job, on, audio)) = live {
        return with_graphics(settings, &args, &canvas, &job, (on, audio), (frames, socket, place), drain, true, &marks);
    }
    // Zones that may widen in the middle of the piece (S3b): the piece as runs
    // too, with live left alone (VIZRS_FFMPEG_INGEST allows the runs).
    if settings.ingest && frames.zones_rev.is_some() {
        if let Some(job) = Job::of(&args, Some(&canvas.channel)).filter(|job| !job.slot) {
            return with_graphics(settings, &args, &canvas, &job, (false, false), (frames, socket, place), drain, false, &marks);
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
    command.args(adjusted(overlay_args(&args, &canvas, fd, &frames, &marks), settings, frames.burst(0.0)));
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
