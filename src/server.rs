// HTTP side of the sidecar.
//
//   POST /etv?channel=1[&render=720p][&area=1920x140+0+940]
//                         body: ErsatzTV template data (JSON from the script
//                         element's stdin); reply: endless ETV packet stream
//   GET  /etv/pages       pages of the VizRS show, for the page elements of
//                         ErsatzTV (`vizrs-etv-client`, src/sync.rs)
//   GET  /ffmpeg?channel=1[&wait=2000][&area=1]
//                         the ffmpeg overlay (vizrs-ffmpeg) takes over
//                         a script session of the channel: its packets
//                         stop, the reply gives the session, size, rate and
//                         colour matrix of the frames;
//                         with area=1 (the overlay can place the graphics)
//                         the graphics areas (`areas`: x, y, w, h; stacked
//                         one below the other in the frames), the size of
//                         the frames and the position x, y of the first
//   GET  /ffmpeg/frames?session=N[&fps=25/1][&area=1]
//                         raw yuva420p frames of that session for ffmpeg:
//                         the channel, or with area=1 its graphics areas
//   GET  /health          captures and sessions

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use crate::area::{Area, Layout, geometries};
use crate::capture::{Capture, CaptureMode, CaptureOptions, IDLE, find_browser};
use crate::config::Config;
use crate::frames::{Empty, Source};
use crate::http::{self, Request};
use crate::json::{self, Value};
use crate::native::{self, Renderer};
use crate::scale::render_size;
use crate::stream::{self, Control, Format, Options};
use crate::sync;
use crate::template::{AdMatcher, ClipContext, ShowMatcher, js_string};
use crate::vizrs::ChannelEvents;
use crate::yuva::ffmpeg_matrix;

pub const MAX_BODY: usize = 4 * 1024 * 1024;
// A channel served by the ffmpeg overlay holds its script packets this long
// for the overlay to take the session over; then ErsatzTV draws them itself.
const HOLD: Duration = Duration::from_millis(3000);
// A session taken over waits this long for the overlay's frame stream.
const FRAMES_WAIT: Duration = Duration::from_millis(10000);
const MAX_WAIT_MS: f64 = 10000.0;

/// `Number(text)` of JavaScript for the rates and waits in a query.
fn js_number(text: &str) -> f64 {
    let text = text.trim();
    if text.is_empty() { 0.0 } else { text.parse().unwrap_or(f64::NAN) }
}

/// "25", "25/1" or "30000/1001" as frames per second; 0 if not a rate.
pub fn parse_rate(value: Option<&str>) -> f64 {
    let mut parts = value.unwrap_or("").split('/');
    let num = js_number(parts.next().unwrap_or(""));
    let den = parts.next().map_or(1.0, js_number);
    let rate = num / den;
    if rate.is_finite() && rate > 0.0 { rate } else { 0.0 }
}

/// A delayed action that can be called off (`clearTimeout`).
struct Timer(Arc<AtomicBool>);

impl Timer {
    fn after(delay: Duration, action: impl FnOnce() + Send + 'static) -> Timer {
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancelled);
        let _ = thread::Builder::new().name("timer".into()).spawn(move || {
            thread::sleep(delay);
            if !flag.load(Ordering::Relaxed) {
                action();
            }
        });
        Timer(cancelled)
    }

    fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Graphics of a session: a capture, or transparent frames (burn-in off).
#[derive(Clone)]
enum Graphics {
    Empty,
    Capture(Arc<Capture>),
}

/// A script element session, for the ffmpeg overlay to take over.
struct Script {
    id: u64,
    channel: String,
    ctx: ClipContext,
    fps: f64,
    capture: Option<Arc<Capture>>,
    /// The session as a reader of `capture`.
    reader: u64,
    /// The packet stream to ErsatzTV: paused while taken over, closed when
    /// the overlay's frame stream ends.
    stream: Arc<Control>,
    claimed: AtomicBool,
    frames: AtomicBool,
    hold: Mutex<Option<Timer>>,
    claim_timer: Mutex<Option<Timer>>,
}

fn cancel(slot: &Mutex<Option<Timer>>) {
    if let Some(timer) = slot.lock().unwrap_or_else(|e| e.into_inner()).take() {
        timer.cancel();
    }
}

/// A `/ffmpeg` request waiting for a script session of its channel.
type Waiter = (u64, Sender<Arc<Script>>);

#[derive(Default)]
struct Sessions {
    captures: Vec<(String, Arc<Capture>)>,
    scripts: BTreeMap<u64, Arc<Script>>,
    waiters: HashMap<String, Vec<Waiter>>,
    /// Channels whose ffmpeg runs through the overlay: their sessions start held.
    overlay: Vec<String>,
    /// Pieces (channel, item, seek) whose graphics one session carries: the
    /// other VizRS elements of the piece send transparent frames, or ErsatzTV
    /// (or the overlay) would draw the same /output twice.
    carriers: HashSet<String>,
    next_id: u64,
    next_waiter: u64,
}

type Shared = Arc<Mutex<Sessions>>;

fn lock(sessions: &Shared) -> MutexGuard<'_, Sessions> {
    sessions.lock().unwrap_or_else(|e| e.into_inner())
}

pub struct Server {
    pub config: Arc<Config>,
    ads: Arc<AdMatcher>,
    browser: Option<String>,
    channels: Mutex<HashMap<String, Arc<ChannelEvents>>>,
    sessions: Shared,
    count: AtomicUsize,
    empty: Empty,
}

impl Server {
    pub fn new(config: Config) -> Arc<Server> {
        let (ads, warnings) = AdMatcher::new(&config.ad_match);
        for warning in warnings {
            crate::warn!("{warning}");
        }
        let (_, show_warnings) = ShowMatcher::new(&config.shows);
        for warning in show_warnings {
            crate::warn!("{warning}");
        }
        let browser = if config.browser.is_empty() { find_browser() } else { Some(config.browser.clone()) };
        Arc::new(Server {
            config: Arc::new(config),
            ads: Arc::new(ads),
            browser,
            channels: Mutex::new(HashMap::new()),
            sessions: Shared::default(),
            count: AtomicUsize::new(0),
            empty: Empty::new(),
        })
    }

    fn events_for(&self, channel: &str) -> Arc<ChannelEvents> {
        let mut channels = self.channels.lock().unwrap_or_else(|e| e.into_inner());
        let events = channels.entry(channel.to_owned()).or_insert_with(|| {
            let config = self.config.for_channel(channel);
            let (shows, warnings) = ShowMatcher::new(&config.shows);
            for warning in warnings {
                crate::warn!("channel {channel}: {warning}");
            }
            ChannelEvents::new(Arc::new(config), Arc::clone(&self.ads), Arc::new(shows))
        });
        Arc::clone(events)
    }

    /// One capture per /output page, channel size, render size and graphics area, shared by the channels that have them.
    fn capture_for(&self, url: &str, width: u32, height: u32, fps: f64, spec: &str, area: &str) -> Graphics {
        if url.is_empty() || width == 0 || height == 0 {
            return Graphics::Empty;
        }
        let (rw, rh) = render_size(spec, width, height).unwrap_or_else(|error| {
            crate::warn!("render size: {error}; using {width}x{height}");
            (width, height)
        });
        let mode = CaptureMode::parse(&self.config.capture_mode).unwrap_or_else(|error| {
            crate::warn!("{error}; using frames");
            CaptureMode::Frames
        });
        let renderer = Renderer::parse(&self.config.renderer).unwrap_or_else(|error| {
            crate::warn!("{error}; using chromium");
            Renderer::Chromium
        });
        let areas = Area::parse_list(area, width, height).unwrap_or_else(|error| {
            crate::warn!("{error}; using the whole channel");
            Vec::new()
        });
        let key = format!("{url}/{width}x{height}/{rw}x{rh}/{}", geometries(&areas));
        let mut sessions = lock(&self.sessions);
        if let Some((_, capture)) = sessions.captures.iter().find(|(k, _)| *k == key) {
            return Graphics::Capture(Arc::clone(capture));
        }
        let capture = Capture::start(CaptureOptions {
            url: url.to_owned(),
            width: rw,
            height: rh,
            out_width: width,
            out_height: height,
            areas,
            fps,
            max_fps: self.config.max_fps,
            mode,
            browser: self.browser.clone(),
            no_sandbox: self.config.no_sandbox,
            extra_args: self.config.browser_args.clone(),
            idle: IDLE,
            history: stream::history(self.config.lead_seconds),
            clock_offset: self.config.clock_offset_seconds,
            tape: self.config.tape,
            renderer,
            renderer_bin: native::find_renderer(&self.config.renderer_bin),
        });
        sessions.captures.push((key, Arc::clone(&capture)));
        Graphics::Capture(capture)
    }

    /// `/health`: script sessions, captures, channels served by the ffmpeg overlay.
    pub fn health(&self) -> Value {
        let sessions = lock(&self.sessions);
        let captures = sessions
            .captures
            .iter()
            .map(|(_, c)| {
                let mut capture = json::object([
                    ("size", c.size().into()),
                    ("render", c.render().into()),
                    ("area", if c.areas().is_empty() { Value::Null } else { geometries(c.areas()).into() }),
                    ("fps", c.fps().into()),
                    ("drawn_fps", c.capture_fps().into()),
                    ("mode", c.mode().name().into()),
                    ("renderer", c.renderer().0.into()),
                    ("drawn_by", c.renderer().1.into()),
                    ("status", c.status().into()),
                    ("seq", c.current().seq.into()),
                ]);
                // The crawl whose tape the sidecar draws (null: none) and the films taken so far.
                if let (Some((crawl, films)), Value::Object(members)) = (c.tape(), &mut capture) {
                    let crawl = if crawl == 0 { Value::Null } else { crawl.into() };
                    members.push(("tape".into(), json::object([("crawl", crawl), ("films", films.into())])));
                }
                capture
            })
            .collect();
        json::object([
            ("sessions", (self.count.load(Ordering::Relaxed) as u64).into()),
            ("captures", Value::Array(captures)),
            ("overlay", sessions.overlay.iter().map(String::as_str).collect::<Vec<_>>().into()),
        ])
    }

    /// Kills every browser (the sidecar stops).
    pub fn stop_captures(&self) {
        for (_, capture) in &lock(&self.sessions).captures {
            capture.stop();
        }
    }

    pub fn handle(&self, request: Request, mut stream: TcpStream) {
        let result = match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/health") => http::respond(&mut stream, 200, Some("application/json"), self.health().to_string().as_bytes()),
            ("GET", "/etv/pages") => self.pages_request(&request, stream),
            ("GET", "/ffmpeg") => self.claim_request(&request, stream),
            ("GET", "/ffmpeg/frames") => {
                self.frames_request(&request, stream);
                Ok(())
            }
            ("POST", "/etv") => {
                self.script_session(&request, stream);
                Ok(())
            }
            _ => http::respond(&mut stream, 404, None, b""),
        };
        if let Err(error) = result {
            crate::debug!("http {} {}: {error}", request.method, request.path);
        }
    }

    /// The pages of the channel's VizRS; the address of its API comes from `output_url`.
    fn pages_request(&self, request: &Request, mut stream: TcpStream) -> std::io::Result<()> {
        let config = self.config.for_channel(request.param("channel").unwrap_or(""));
        let pages = sync::http_url_of(&config.output_url).ok_or_else(|| "no output_url".to_owned()).and_then(|url| {
            let show = sync::fetch_show(&url, &config.vizrs.token).map_err(|e| e.to_string())?;
            sync::parse_pages(&show)
        });
        match pages {
            Ok(pages) => http::respond(&mut stream, 200, Some("application/json"), sync::pages_json(&pages).as_bytes()),
            Err(error) => {
                crate::warn!("VizRS pages: {error}");
                http::respond(&mut stream, 502, None, b"")
            }
        }
    }

    fn free(sessions: &Sessions, channel: &str) -> Option<Arc<Script>> {
        sessions.scripts.values().find(|s| s.channel == channel && !s.claimed.load(Ordering::Relaxed) && s.capture.is_some()).cloned()
    }

    /// The overlay takes the session: ErsatzTV gets no more packets from it.
    fn claim(sessions: &mut Sessions, script: &Arc<Script>) {
        script.claimed.store(true, Ordering::Relaxed);
        cancel(&script.hold);
        script.stream.pause();
        if !sessions.overlay.contains(&script.channel) {
            sessions.overlay.push(script.channel.clone());
        }
        // No frame stream after all (the overlay gave up): ErsatzTV draws again.
        let back = Arc::clone(script);
        let timer = Timer::after(FRAMES_WAIT, move || {
            back.claimed.store(false, Ordering::Relaxed);
            back.stream.resume();
        });
        *script.claim_timer.lock().unwrap_or_else(|e| e.into_inner()) = Some(timer);
    }

    fn wait_for_script(&self, channel: &str, wait: Duration) -> Option<Arc<Script>> {
        let (id, receiver) = {
            let mut sessions = lock(&self.sessions);
            if let Some(script) = Self::free(&sessions, channel) {
                Self::claim(&mut sessions, &script);
                return Some(script);
            }
            let (sender, receiver) = mpsc::channel();
            sessions.next_waiter += 1;
            let id = sessions.next_waiter;
            sessions.waiters.entry(channel.to_owned()).or_default().push((id, sender));
            (id, receiver)
        };
        if let Ok(script) = receiver.recv_timeout(wait) {
            return Some(script);
        }
        let mut sessions = lock(&self.sessions);
        if let Some(list) = sessions.waiters.get_mut(channel) {
            list.retain(|(w, _)| *w != id);
        }
        // A session may have been handed over just as the wait ran out.
        receiver.try_recv().ok()
    }

    fn register(&self, script: &Arc<Script>) {
        let mut sessions = lock(&self.sessions);
        sessions.scripts.insert(script.id, Arc::clone(script));
        // Only a session with frames: the other element of the same piece
        // (no capture) would leave the overlay with nothing to stream.
        if script.capture.is_none() {
            return;
        }
        let waiter = sessions.waiters.get_mut(&script.channel).filter(|list| !list.is_empty()).map(|list| list.remove(0));
        if let Some((_, waiter)) = waiter {
            Self::claim(&mut sessions, script);
            let _ = waiter.send(Arc::clone(script));
        }
    }

    fn claim_request(&self, request: &Request, mut stream: TcpStream) -> std::io::Result<()> {
        let channel = request.param("channel").unwrap_or("1").to_owned();
        let wait = js_number(request.param("wait").unwrap_or("2000"));
        let wait = if wait.is_nan() { 0.0 } else { wait.clamp(0.0, MAX_WAIT_MS) };
        let Some(script) = self.wait_for_script(&channel, Duration::from_secs_f64(wait / 1000.0)) else {
            return http::respond(&mut stream, 404, None, b"");
        };

        crate::info!("channel {channel}: ffmpeg overlay took over the graphics of {}", script.ctx.title);
        // Frames of the graphics areas only for an overlay that places them (area=1).
        let placed = request.param("area") == Some("1");
        let layout = frame_layout(&script, placed);
        let mut reply = json::object([
            ("session", script.id.into()),
            ("width", u64::from(layout.width).into()),
            ("height", u64::from(layout.height).into()),
            ("fps", script.fps.into()),
            ("matrix", ffmpeg_matrix(script.ctx.height).into()),
        ]);
        if let (true, Value::Object(members)) = (placed, &mut reply) {
            let (first, _) = layout.parts[0];
            members.push(("x".into(), u64::from(first.x).into()));
            members.push(("y".into(), u64::from(first.y).into()));
            let areas = layout.parts.iter().map(|(a, _)| {
                json::object([
                    ("x", u64::from(a.x).into()),
                    ("y", u64::from(a.y).into()),
                    ("w", u64::from(a.w).into()),
                    ("h", u64::from(a.h).into()),
                ])
            });
            members.push(("areas".into(), Value::Array(areas.collect())));
        }
        http::respond(&mut stream, 200, Some("application/json"), reply.to_string().as_bytes())
    }

    fn frames_request(&self, request: &Request, mut stream: TcpStream) {
        let id = js_number(request.param("session").unwrap_or(""));
        let script = lock(&self.sessions).scripts.get(&(id as u64)).filter(|_| id.fract() == 0.0).cloned();
        let taken = script.filter(|s| s.claimed.load(Ordering::Relaxed) && !s.frames.swap(true, Ordering::Relaxed));
        let Some((script, capture)) = taken.and_then(|s| s.capture.clone().map(|c| (s, c))) else {
            crate::warn!("ffmpeg overlay: no frames for session {id}; ErsatzTV draws the graphics");
            let _ = http::respond(&mut stream, 404, None, b"");
            return;
        };

        cancel(&script.claim_timer);
        let fps = match parse_rate(request.param("fps")) {
            0.0 => script.fps,
            fps => fps,
        };
        // ErsatzTV's rate as the overlay reads it, not the session's own.
        capture.set_rate(script.reader, fps);
        let reader = capture.acquire(fps);
        let control = Control::new(false);
        watch_close(&stream, &control);
        if let Ok(mut out) = http::Stream::start(stream, request) {
            let layout = frame_layout(&script, request.param("area") == Some("1"));
            let format = Format::Yuva { layout, hd: script.ctx.height >= 720 };
            stream::stream_frames(
                &*capture,
                &mut out,
                &Options { fps, lead_seconds: self.config.lead_seconds, format, air: script.ctx.air_ms() },
                &control,
            );
        }
        capture.release(reader);
        // The transcode is over: the script ends with it, as without the overlay.
        script.stream.close();
    }

    /// A script element session: commands in the background, frames at once,
    /// for as long as ErsatzTV reads them.
    fn script_session(&self, request: &Request, mut stream: TcpStream) {
        let body = String::from_utf8_lossy(&request.body);
        let data = match json::parse(if body.trim().is_empty() { "{}" } else { &body }) {
            Ok(data) => data,
            Err(error) => {
                let _ = http::respond(&mut stream, 400, None, format!("bad template data: {error}").as_bytes());
                return;
            }
        };
        let ctx = ClipContext::from_data(&data);
        let channel = request.param("channel").unwrap_or("1").to_owned();
        let fps = if ctx.fps > 0.0 { ctx.fps } else { 25.0 };
        crate::info!(
            "channel {channel}: {} ({}x{}@{}, seek {:.1} s)",
            ctx.title,
            ctx.width,
            ctx.height,
            js_string(&Value::Number(fps)),
            ctx.seek
        );

        // Commands go out in the background; frames start at once.
        let custom_commands = if let Some(cmd) = request.param("cmd").or_else(|| request.param("command")) {
            Some(vec![cmd.to_owned()])
        } else if let Some(take) = request.param("page").or_else(|| request.param("take")) {
            let fields = request.param("fields").map(|f| format!(" {f}")).unwrap_or_default();
            Some(vec![format!("TAKE {take}{fields}")])
        } else {
            None
        };

        let events = self.events_for(&channel);
        events.session(&ctx, custom_commands.as_deref());

        let piece = format!("{channel}|{}|{}", ctx.start, ctx.seek);
        let carrier = lock(&self.sessions).carriers.insert(piece.clone());
        let spec = request.param("render").unwrap_or(&self.config.render_size).to_owned();
        let area = request.param("area").unwrap_or(&self.config.area).to_owned();
        let capture = match carrier
            .then(|| self.capture_for(&self.config.for_channel(&channel).output_url, ctx.width, ctx.height, fps, &spec, &area))
        {
            Some(Graphics::Capture(capture)) => Some(capture),
            _ => None,
        };
        let reader = capture.as_ref().map_or(0, |capture| capture.acquire(fps));

        let held = capture.is_some() && lock(&self.sessions).overlay.contains(&channel);
        let control = Control::new(held);
        watch_close(&stream, &control);
        // A held session writes nothing at first; the client must see the reply at once.
        if let Ok(mut out) = http::Stream::start(stream, request) {
            self.count.fetch_add(1, Ordering::Relaxed);
            let script = {
                let mut sessions = lock(&self.sessions);
                sessions.next_id += 1;
                Arc::new(Script {
                    id: sessions.next_id,
                    channel: channel.clone(),
                    ctx: ctx.clone(),
                    fps,
                    capture: capture.clone(),
                    reader,
                    stream: Arc::clone(&control),
                    claimed: AtomicBool::new(false),
                    frames: AtomicBool::new(false),
                    hold: Mutex::new(None),
                    claim_timer: Mutex::new(None),
                })
            };
            if held {
                let (sessions, resume, channel) = (Arc::clone(&self.sessions), Arc::clone(&control), channel.clone());
                let timer = Timer::after(HOLD, move || {
                    lock(&sessions).overlay.retain(|c| *c != channel);
                    crate::warn!("channel {channel}: no ffmpeg overlay; ErsatzTV draws the graphics");
                    resume.resume();
                });
                *script.hold.lock().unwrap_or_else(|e| e.into_inner()) = Some(timer);
            }
            self.register(&script);

            let source: &dyn Source = match &capture {
                Some(capture) => &**capture,
                None => &self.empty,
            };
            let options = Options { fps, lead_seconds: self.config.lead_seconds, format: Format::Packets, air: ctx.air_ms() };
            stream::stream_frames(source, &mut out, &options, &control);
            let _ = out.finish();

            self.count.fetch_sub(1, Ordering::Relaxed);
            lock(&self.sessions).scripts.remove(&script.id);
            cancel(&script.hold);
            cancel(&script.claim_timer);
        }
        if let Some(capture) = &capture {
            capture.release(reader);
        }
        if carrier {
            lock(&self.sessions).carriers.remove(&piece);
        }
        events.ended(&ctx);
    }
}

/// What a session's raw frames carry: its graphics areas stacked when the
/// overlay places them (`area`), else the whole channel.
fn frame_layout(script: &Script, area: bool) -> Layout {
    match script.capture.as_ref().map(|c| c.areas()) {
        Some(areas) if area && !areas.is_empty() => Layout::atlas(areas),
        _ => Layout::whole(script.ctx.width, script.ctx.height),
    }
}

/// Closes `control` once the client hangs up: a stream that writes nothing
/// (held for the ffmpeg overlay) would not notice otherwise.
fn watch_close(stream: &TcpStream, control: &Arc<Control>) {
    let Ok(mut reader) = stream.try_clone() else { return };
    let control = Arc::clone(control);
    let _ = thread::Builder::new().name("http-close".into()).spawn(move || {
        let mut buffer = [0; 512];
        while matches!(reader.read(&mut buffer), Ok(n) if n > 0) {}
        control.close();
    });
}
