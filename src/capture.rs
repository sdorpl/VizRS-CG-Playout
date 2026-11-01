// Burn-in source: one headless Chromium per channel size renders the VizRS
// `/output` page with a transparent background. Chromium sends a PNG
// (DevTools `Page.startScreencast`) only when the page changes; it becomes the
// current frame for all sessions, with a history as long as the lead of the
// streams so each output frame can take the picture of its own moment. A PNG
// is decoded only when a session shows it. In the `frames` mode Chromium draws
// only when asked (`HeadlessExperimental.beginFrame`), once per frame of the
// channel, with that frame's time: animations advance one step per output frame.
// With a graphics area Chromium draws only that part of the page (a DevTools
// viewport; the page still lays out at full size), so PNGs are its size. With
// several areas the frames mode opens one tab per area, each with its area as
// the viewport (one PNG per area): a tab keeps its viewport, so its damage tells
// whether its area changed. The screencast has one viewport: their bounding
// box, cut to the areas.
// With `tape` the frames mode draws a standing VizRS crawl itself (`tape`):
// Chromium films its tape now and then, the sidecar shifts it every frame.

use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::area::{Area, cut_to, geometries, union};
use crate::devtools::{DevTools, Incoming};
use crate::frames::{Decode, Frame, History, Shot, Source};
use crate::json::{self, Value};
use crate::native::{self, Kind, Native, Renderer};
use crate::packets::{Rect, crop};
use crate::png::decode_png;
use crate::rects::visible_rects;
use crate::scale::scale_rects;
use crate::tape::{Composite, Crawl, Film, Reel, parse_state};
use crate::{base64, sys};

// First start on a cold host (CI runner, small ARM VM) can take 15–20 s.
const START_TIMEOUT: Duration = Duration::from_secs(30);
const CALL_TIMEOUT: Duration = Duration::from_secs(10);
const RESTART_DELAY: Duration = Duration::from_secs(3);
// Without sessions (ErsatzTV transcodes on demand) the page stops being
// captured after this long; the gap between two items is much shorter.
pub const IDLE: Duration = Duration::from_secs(30);
// Chromium composites at 60 Hz; the screencast takes every n-th frame.
const BROWSER_FPS: f64 = 60.0;
// With n > 1 a lone change (static page, last frame of an animation) may be
// skipped: after this long without frames a screenshot fetches the final state.
const SETTLE: Duration = Duration::from_millis(150);
// Frames mode: a page standing still is probed for damage without a picture;
// a screenshot this often catches a change the probes missed.
const CHECK_MS: f64 = 1000.0;
// Frames mode: missed slots are counted over this long; above a tenth, a warning.
const MISSED_WINDOW: Duration = Duration::from_secs(60);
// The rate guard: a window with over a tenth of the slots missed halves the
// drawing rate (an even cadence, unlike any other limit), not below MIN_HALVED_FPS.
const GUARD_WINDOW: Duration = Duration::from_secs(5);
const MIN_HALVED_FPS: f64 = 24.0;
// Back to the full rate after this long of drawing taking under CALM_SHARE of the
// time (at half the rate: fits the full one). Longer after a return that failed.
const CALM_SHARE: f64 = 0.4;
const RESTORE_AFTER: Duration = Duration::from_secs(60);
const MAX_RESTORE_AFTER: Duration = Duration::from_secs(3600);
const BROWSERS: [&str; 7] =
    ["chromium-headless-shell", "chromium", "chromium-browser", "google-chrome", "google-chrome-stable", "chrome", "headless_shell"];

/// `VIZRS_BROWSER`, or the first Chromium-like browser on `PATH`.
pub fn find_browser() -> Option<String> {
    if let Some(browser) = std::env::var_os("VIZRS_BROWSER").filter(|b| !b.is_empty()) {
        return Some(browser.to_string_lossy().into_owned());
    }
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .flat_map(|dir| BROWSERS.iter().map(move |name| dir.join(name)))
        .find(|candidate| is_executable(candidate))
        .map(|path| path.to_string_lossy().into_owned())
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// How Chromium's pictures are taken (`capture_mode`, DOKUMENTACJA §5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CaptureMode {
    /// `Page.startScreencast`: Chromium draws at 60 Hz, every n-th frame is taken.
    Screencast,
    /// `HeadlessExperimental.beginFrame`: one frame per frame of the channel,
    /// at its time (chromium-headless-shell only).
    Frames,
}

impl CaptureMode {
    /// `""` or `frames` (the default), `screencast`.
    pub fn parse(spec: &str) -> Result<CaptureMode, String> {
        match spec.trim().to_ascii_lowercase().as_str() {
            "screencast" => Ok(CaptureMode::Screencast),
            "" | "frames" => Ok(CaptureMode::Frames),
            _ => Err(format!("capture mode \"{spec}\" (expected screencast or frames)")),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            CaptureMode::Screencast => "screencast",
            CaptureMode::Frames => "frames",
        }
    }
}

/// When the frames mode draws at half the rate. Inheriting the channel's rate
/// is the default; this is for the host that cannot keep up with it.
struct RateGuard {
    window: Instant,
    drawn: u64,
    missed: u64,
    busy: Duration,
    calm_since: Option<Instant>,
    restored_at: Option<Instant>,
    restore_after: Duration,
}

impl RateGuard {
    fn new(now: Instant) -> RateGuard {
        RateGuard {
            window: now,
            drawn: 0,
            missed: 0,
            busy: Duration::ZERO,
            calm_since: None,
            restored_at: None,
            restore_after: RESTORE_AFTER,
        }
    }

    /// A new frame grid: its first slots say nothing yet.
    fn restart(&mut self, now: Instant) {
        (self.window, self.drawn, self.missed, self.busy) = (now, 0, 0, Duration::ZERO);
    }

    fn slot(&mut self, missed: u64, busy: Duration) {
        self.drawn += 1;
        self.missed += missed;
        self.busy += busy;
    }

    /// The rate to change to at the end of a window: `Some(halved)`.
    fn verdict(&mut self, now: Instant, halved: bool, can_halve: bool) -> Option<bool> {
        let elapsed = now.saturating_duration_since(self.window);
        if elapsed < GUARD_WINDOW {
            return None;
        }
        let (drawn, missed, busy) = (self.drawn, self.missed, self.busy);
        self.restart(now);
        if !halved {
            if !can_halve || missed * 10 <= drawn + missed || drawn == 0 {
                return None;
            }
            let failed = self.restored_at.is_some_and(|at| now.saturating_duration_since(at) < RESTORE_AFTER);
            self.restore_after = if failed { (self.restore_after * 2).min(MAX_RESTORE_AFTER) } else { RESTORE_AFTER };
            return Some(true);
        }
        if busy.as_secs_f64() >= CALM_SHARE * elapsed.as_secs_f64() {
            self.calm_since = None;
            return None;
        }
        if now.saturating_duration_since(*self.calm_since.get_or_insert(now)) < self.restore_after {
            return None;
        }
        self.calm_since = None;
        self.restored_at = Some(now);
        Some(false)
    }
}

/// Screencast step that keeps frames at or above the channel rate: 25 fps takes every second frame.
pub fn every_nth_frame(fps: f64) -> u32 {
    (BROWSER_FPS / fps.max(1.0)).floor().max(1.0) as u32
}

/// The channel rate, at most `max` (0 = no limit).
pub fn capped_fps(fps: f64, max: f64) -> f64 {
    if max > 0.0 { fps.min(max) } else { fps }
}

/// When Chromium drew a screencast frame (`metadata.timestamp`, seconds since
/// the epoch), on the monotonic clock; arrival time if missing or off.
pub fn drawn_at(timestamp: Option<f64>) -> Instant {
    let now = Instant::now();
    let Some(timestamp) = timestamp.filter(|t| t.is_finite()) else { return now };
    let wall = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
    let ago = wall - timestamp;
    if (0.0..1.0).contains(&ago) { now.checked_sub(Duration::from_secs_f64(ago)).unwrap_or(now) } else { now }
}

/// Removes a browser profile; its helper processes may still write for a
/// moment after the browser is killed, so a few tries (as the Node.js build).
fn remove_profile(profile: &Path) {
    for _ in 0..5 {
        if std::fs::remove_dir_all(profile).is_ok() || !profile.exists() {
            return;
        }
        thread::sleep(Duration::from_millis(200));
    }
}

pub struct CaptureOptions {
    pub url: String,
    /// Render size of the page.
    pub width: u32,
    pub height: u32,
    /// Size of the frames (the channel).
    pub out_width: u32,
    pub out_height: u32,
    /// Graphics areas of the channel (`area`): only they are drawn; none = all of it.
    pub areas: Vec<Area>,
    pub fps: f64,
    /// Capture rate limit (`max_fps`); 0 = the channel rate.
    pub max_fps: f64,
    pub mode: CaptureMode,
    pub browser: Option<String>,
    pub no_sandbox: bool,
    pub extra_args: Vec<String>,
    pub idle: Duration,
    /// How far back sessions take pictures (their lead).
    pub history: Duration,
    /// Seconds added to the page's clock (`clock_offset_seconds`).
    pub clock_offset: f64,
    /// The sidecar draws a standing VizRS crawl (`tape`, frames mode only).
    pub tape: bool,
    /// Who draws: Chromium, the VizRS native renderer, or the native one with Chromium ready (`renderer`).
    pub renderer: Renderer,
    /// The native renderer program (`renderer_bin`).
    pub renderer_bin: String,
}

/// What the browser loop is told from other threads.
enum Control {
    DevTools(Incoming),
    /// Start or stop capturing (sessions came or left, another rate).
    Active(bool),
}

/// The crawl whose tape the sidecar draws (`tape`).
struct Track {
    id: u64,
    key: String,
    /// Its area (index of the regions) and window in that area's pixels.
    part: usize,
    window: Area,
    reel: Reel,
}

/// The tape over the frames of one page.
#[derive(Default)]
struct TapeRun {
    track: Option<Track>,
    /// Crawls (area, id) the sidecar gave back to the page after a failure.
    refused: Vec<(usize, u64)>,
    /// Frame time of the next look for a crawl when none is drawn.
    next_look: f64,
    /// The page picture under the tape: its PNG and its pixels.
    base: Option<(Arc<[u8]>, Arc<crate::png::Picture>)>,
    /// The last composite pushed: its offset, page picture and newest film.
    shown: Option<(i64, usize, usize)>,
}

struct State {
    status: &'static str,
    /// Sessions reading frames, with their rates: the capture follows the highest.
    readers: Vec<(u64, f64)>,
    next_reader: u64,
    active: bool,
    fps: f64,
    /// Bumped by every acquire and release: an older idle timer does nothing.
    idle_generation: u64,
    control: Option<Sender<Control>>,
    child: Option<Child>,
    profile: Option<PathBuf>,
}

pub struct Capture {
    options: CaptureOptions,
    history: History,
    /// One PNG of the whole page or of the bounding box of the areas (screencast).
    decode: Arc<Decode>,
    /// One PNG per area (frames mode).
    decode_areas: Arc<Decode>,
    state: Mutex<State>,
    stopped: AtomicBool,
    /// The browser has no begin frames (not chromium-headless-shell): screencast instead.
    no_frames: AtomicBool,
    /// The frames mode draws at half the rate: the host does not keep up (`RateGuard`).
    halved: AtomicBool,
    /// The crawl whose tape the sidecar draws (0: none) and the films taken.
    tape_crawl: AtomicU64,
    tape_films: AtomicU64,
    /// The native renderer drew the last frame (`renderer` native or auto).
    native_drawing: AtomicBool,
}

static PROFILES: AtomicU64 = AtomicU64::new(0);

impl Capture {
    /// Starts the browser; the capture is active at first.
    pub fn start(options: CaptureOptions) -> Arc<Capture> {
        let fps = options.fps;
        let history = History::new(options.history);
        let decode = decoder(regions(&options, false), options.areas.clone());
        let decode_areas = decoder(regions(&options, true), Vec::new());
        let capture = Arc::new(Capture {
            options,
            history,
            decode,
            decode_areas,
            state: Mutex::new(State {
                status: "starting",
                readers: Vec::new(),
                next_reader: 0,
                active: true,
                fps,
                idle_generation: 0,
                control: None,
                child: None,
                profile: None,
            }),
            stopped: AtomicBool::new(false),
            no_frames: AtomicBool::new(false),
            halved: AtomicBool::new(false),
            tape_crawl: AtomicU64::new(0),
            tape_films: AtomicU64::new(0),
            native_drawing: AtomicBool::new(false),
        });
        let browser = Arc::clone(&capture);
        let _ = thread::Builder::new().name("capture".into()).spawn(move || browser.supervise());
        capture
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Channel size, as `/health` reports it.
    pub fn size(&self) -> String {
        format!("{}x{}", self.options.out_width, self.options.out_height)
    }

    /// Render size of the page.
    pub fn render(&self) -> String {
        format!("{}x{}", self.options.width, self.options.height)
    }

    /// Graphics areas of the channel; none = all of it.
    pub fn areas(&self) -> &[Area] {
        &self.options.areas
    }

    pub fn key(&self) -> String {
        let mut key = if self.size() == self.render() { self.size() } else { format!("{} (render {})", self.size(), self.render()) };
        if !self.areas().is_empty() {
            key.push_str(&format!(" area {}", geometries(self.areas())));
        }
        key
    }

    pub fn status(&self) -> &'static str {
        self.state().status
    }

    pub fn fps(&self) -> f64 {
        self.state().fps
    }

    /// The mode in use: `frames` falls back to the screencast on a browser without begin frames.
    pub fn mode(&self) -> CaptureMode {
        if self.no_frames.load(Ordering::Relaxed) { CaptureMode::Screencast } else { self.options.mode }
    }

    /// Frames per second Chromium draws in the frames mode, and at most takes in the screencast.
    /// With `tape` in the frames mode: the crawl the sidecar draws (0: none) and the films taken.
    pub fn tape(&self) -> Option<(u64, u64)> {
        (self.options.tape && self.mode() == CaptureMode::Frames)
            .then(|| (self.tape_crawl.load(Ordering::Relaxed), self.tape_films.load(Ordering::Relaxed)))
    }

    /// `renderer` and who draws now: "chromium" or "native".
    pub fn renderer(&self) -> (&'static str, &'static str) {
        let native = self.options.renderer == Renderer::Native || self.native_drawing.load(Ordering::Relaxed);
        let now = if native { "native" } else { "chromium" };
        (self.options.renderer.name(), now)
    }

    /// Frames per second Chromium draws now.
    pub fn capture_fps(&self) -> f64 {
        let fps = capped_fps(self.fps(), self.options.max_fps).max(1.0);
        if self.halved.load(Ordering::Relaxed) && fps / 2.0 >= MIN_HALVED_FPS { fps / 2.0 } else { fps }
    }

    fn can_halve(&self) -> bool {
        capped_fps(self.fps(), self.options.max_fps) / 2.0 >= MIN_HALVED_FPS
    }

    pub fn current(&self) -> Arc<Frame> {
        self.history.current()
    }

    /// A session starts reading frames at `fps`; `release` takes what this returns.
    pub fn acquire(&self, fps: f64) -> u64 {
        let mut state = self.state();
        state.next_reader += 1;
        let reader = state.next_reader;
        state.readers.push((reader, fps));
        state.idle_generation += 1;
        if !state.active {
            self.set_active(&mut state, true);
        }
        self.follow_rate(&mut state);
        reader
    }

    /// The last session left: pause the capture once idle for `idle`.
    pub fn release(self: &Arc<Self>, reader: u64) {
        let generation = {
            let mut state = self.state();
            state.readers.retain(|(id, _)| *id != reader);
            if !state.readers.is_empty() {
                self.follow_rate(&mut state);
                return;
            }
            state.idle_generation += 1;
            state.idle_generation
        };
        let capture = Arc::clone(self);
        let _ = thread::Builder::new().name("capture-idle".into()).spawn(move || {
            thread::sleep(capture.options.idle);
            let mut state = capture.state();
            if state.idle_generation == generation && state.readers.is_empty() && state.active {
                capture.set_active(&mut state, false);
            }
        });
    }

    /// The session reads at another rate (the ffmpeg overlay's, not the template data's).
    pub fn set_rate(&self, reader: u64, fps: f64) {
        let mut state = self.state();
        if let Some(entry) = state.readers.iter_mut().find(|(id, _)| *id == reader) {
            entry.1 = fps;
        }
        self.follow_rate(&mut state);
    }

    fn set_active(&self, state: &mut State, active: bool) {
        state.active = active;
        crate::info!("burn-in {}: {}", self.key(), if active { "resumed" } else { "paused, no sessions" });
        if let Some(control) = &state.control {
            state.status = if active { "capturing" } else { "paused" };
            let _ = control.send(Control::Active(active));
        }
    }

    /// The rate of the sessions reading now, up and down: ErsatzTV's frame rate
    /// changes with the media without Normalize Framerate. Between sessions it stays.
    fn follow_rate(&self, state: &mut State) {
        let fps = state.readers.iter().fold(0.0, |max: f64, (_, fps)| max.max(*fps));
        if fps <= 0.0 || fps == state.fps {
            return;
        }
        let max = self.options.max_fps;
        let (before, after) = (capped_fps(state.fps, max), capped_fps(fps, max));
        state.fps = fps;
        let same = match self.mode() {
            CaptureMode::Screencast => every_nth_frame(before) == every_nth_frame(after),
            CaptureMode::Frames => before == after,
        };
        if same {
            return;
        }
        crate::info!("burn-in {}: capturing for {} fps", self.key(), Value::Number(fps));
        if state.active {
            // A new step needs a new screencast, a new rate a new frame grid.
            if let Some(control) = &state.control {
                let _ = control.send(Control::Active(true));
            }
        }
    }

    /// Kills the browser and removes its profile; the capture does not restart.
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
        let mut state = self.state();
        if let Some(mut child) = state.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(profile) = state.profile.take() {
            remove_profile(&profile);
        }
    }

    fn supervise(self: Arc<Self>) {
        while !self.stopped.load(Ordering::Relaxed) {
            let run = if self.options.renderer == Renderer::Native { self.run_native() } else { self.run_browser() };
            if let Err(error) = run {
                if !self.stopped.load(Ordering::Relaxed) {
                    crate::error!("burn-in {}: {error}", self.key());
                }
            }
            if self.stopped.load(Ordering::Relaxed) {
                break;
            }
            // No graphics until the browser is back; playout continues.
            self.history.set(Vec::new(), Instant::now());
            self.state().status = "restarting";
            thread::sleep(RESTART_DELAY);
        }
    }

    fn browser_command(&self, browser: &str, profile: &Path, mode: CaptureMode) -> Command {
        let o = &self.options;
        let mut command = Command::new(browser);
        command
            .args([
                "--headless=new",
                "--disable-gpu",
                "--remote-debugging-pipe",
                "--hide-scrollbars",
                "--mute-audio",
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-extensions",
                "--disable-background-timer-throttling",
                "--disable-renderer-backgrounding",
                "--disable-backgrounding-occluded-windows",
                "--force-device-scale-factor=1",
                // Less background work next to the encoder (CPU-only hosts such as Ampere A1).
                "--disable-background-networking",
                "--disable-component-update",
                "--disable-default-apps",
                "--disable-sync",
                "--metrics-recording-only",
                "--no-pings",
                "--renderer-process-limit=1",
                "--disable-features=Translate,MediaRouter,OptimizationHints,AutofillServerCommunication",
            ])
            .arg(format!("--user-data-dir={}", profile.display()))
            .arg(format!("--window-size={},{}", o.width, o.height));
        if o.no_sandbox {
            command.arg("--no-sandbox");
        }
        if mode == CaptureMode::Frames {
            // Not --disable-new-content-rendering-timeout: with it every frame reports damage.
            command.args(["--enable-begin-frame-control", "--run-all-compositor-stages-before-draw"]);
        }
        // The page opens in the startup tab: a tab created later over DevTools
        // stays hidden in headless mode and never paints.
        command.args(&o.extra_args).arg(&o.url).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        command
    }

    fn run_browser(self: &Arc<Self>) -> Result<(), String> {
        let browser = self.options.browser.clone().ok_or("no Chromium found (set VIZRS_BROWSER)")?;
        let profile = std::env::temp_dir().join(format!("virtrs-cg-{}-{}", std::process::id(), PROFILES.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&profile).map_err(|e| format!("profile {}: {e}", profile.display()))?;

        // Chromium reads commands on fd 3 and writes replies and events on fd 4.
        let (commands_read, commands_write) = std::io::pipe().map_err(|e| e.to_string())?;
        let (events_read, events_write) = std::io::pipe().map_err(|e| e.to_string())?;
        let mode = self.mode();
        let mut command = self.browser_command(&browser, &profile, mode);
        let fds = [(commands_read.as_raw_fd(), 3), (events_write.as_raw_fd(), 4)];
        // SAFETY: child_setup only makes async-signal-safe calls.
        unsafe {
            command.pre_exec(move || sys::child_setup(&fds));
        }
        let spawned = command.spawn();
        // The child has its copies; ours would keep the pipes open after it dies.
        drop(commands_read);
        drop(events_write);
        let child = match spawned {
            Ok(child) => child,
            Err(e) => {
                remove_profile(&profile);
                return Err(format!("start {browser}: {e}"));
            }
        };

        let (control, incoming) = mpsc::channel();
        {
            let mut state = self.state();
            if self.stopped.load(Ordering::Relaxed) {
                drop(state);
                let mut child = child;
                let _ = child.kill();
                let _ = child.wait();
                remove_profile(&profile);
                return Ok(());
            }
            state.child = Some(child);
            state.profile = Some(profile);
            state.control = Some(control.clone());
        }

        let (events, devtools_events) = mpsc::channel();
        let devtools = DevTools::start(events_read, commands_write, events);
        let forward = control.clone();
        let _ = thread::Builder::new().name("devtools-events".into()).spawn(move || {
            for event in devtools_events {
                if forward.send(Control::DevTools(event)).is_err() {
                    break;
                }
            }
        });

        let result = self.session(&devtools, &incoming, mode);

        let mut state = self.state();
        state.control = None;
        if let Some(mut child) = state.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(profile) = state.profile.take() {
            remove_profile(&profile);
        }
        result
    }

    /// The part of the channel a one-area native renderer draws: that area, else all of it.
    fn native_part(&self) -> Area {
        let o = &self.options;
        if o.areas.is_empty() { Area { x: 0, y: 0, w: o.out_width, h: o.out_height } } else { union(&o.areas) }
    }

    fn start_native(&self) -> Result<Native, String> {
        let o = &self.options;
        Native::start(&o.renderer_bin, &o.url, (o.out_width, o.out_height), &o.areas)
    }

    /// One native frame into the history; false when the renderer left it to Chromium.
    fn native_slot(&self, native: &mut Native, start: f64, at: Instant, complete_only: bool, force: bool) -> Result<bool, String> {
        let reply = native.frame(start, native::wall_ms(start, self.options.clock_offset), complete_only, force)?;
        match reply.kind {
            Kind::Skipped => return Ok(false),
            Kind::Unchanged => {}
            Kind::Empty => self.history.set(Vec::new(), at),
            Kind::Picture(rgba) => {
                let part = self.native_part();
                if (reply.width, reply.height) != (part.w, part.h) {
                    return Err(format!("native renderer: frame {}x{} for {}x{}", reply.width, reply.height, part.w, part.h));
                }
                self.history.set(native::picture_rects(&rgba, part, &self.options.areas), at);
            }
            Kind::Regions(regions) => {
                let rects = regions.iter().flat_map(|(region, rgba)| native::picture_rects(rgba, *region, &self.options.areas));
                self.history.set(rects.collect(), at);
            }
            Kind::Region(r, rgba) => {
                let part = self.native_part();
                if (reply.width, reply.height) != (part.w, part.h) {
                    return Err(format!("native renderer: frame {}x{} for {}x{}", reply.width, reply.height, part.w, part.h));
                }
                let region = Area { x: part.x + r.x, y: part.y + r.y, w: r.w, h: r.h };
                self.history.set(native::picture_rects(&rgba, region, &self.options.areas), at);
            }
        }
        Ok(true)
    }

    /// `renderer: native`: the VizRS native renderer instead of Chromium, one frame per frame of the channel.
    fn run_native(self: &Arc<Self>) -> Result<(), String> {
        let (control, incoming) = mpsc::channel();
        self.state().control = Some(control);
        let result = self.native_loop(&incoming);
        self.state().control = None;
        self.native_drawing.store(false, Ordering::Relaxed);
        result
    }

    fn native_loop(&self, incoming: &mpsc::Receiver<Control>) -> Result<(), String> {
        let mut native = self.start_native()?;
        let (origin_ms, origin) = (sys::monotonic_ms(), Instant::now());
        let instant = |ms: f64| {
            let offset = Duration::from_secs_f64((ms - origin_ms).abs() / 1000.0);
            if ms >= origin_ms { origin.checked_add(offset) } else { origin.checked_sub(offset) }.unwrap_or(origin)
        };
        let mut active = {
            let mut state = self.state();
            state.status = if state.active { "capturing" } else { "paused" };
            state.active
        };
        crate::info!("burn-in {}: drawing {} with the native renderer", self.key(), self.options.url);
        let mut frame_ms = 1000.0 / self.capture_fps();
        let mut slot = (sys::monotonic_ms() / frame_ms).ceil();
        let mut force = true;
        loop {
            let wait = if active { instant(slot * frame_ms).saturating_duration_since(Instant::now()) } else { Duration::from_secs(3600) };
            match incoming.recv_timeout(wait) {
                Err(RecvTimeoutError::Disconnected) | Ok(Control::DevTools(Incoming::Closed)) => {
                    return Err("native renderer stopped".into());
                }
                Ok(Control::DevTools(_)) => {}
                Ok(Control::Active(on)) => {
                    active = on;
                    frame_ms = 1000.0 / self.capture_fps();
                    slot = (sys::monotonic_ms() / frame_ms).ceil();
                    force = true;
                }
                Err(RecvTimeoutError::Timeout) => {
                    let start = slot * frame_ms;
                    self.native_slot(&mut native, start, instant(start), false, force)?;
                    self.native_drawing.store(true, Ordering::Relaxed);
                    force = false;
                    slot = (slot + 1.0).max((sys::monotonic_ms() / frame_ms).floor());
                }
            }
        }
    }

    /// The startup tab once it shows the page (it starts as `about:blank`).
    fn page_target(&self, devtools: &DevTools) -> Result<String, String> {
        let deadline = Instant::now() + START_TIMEOUT;
        while Instant::now() < deadline {
            let targets = devtools.call("Target.getTargets", json::object([]), None, START_TIMEOUT)?;
            let page = targets.get("targetInfos").and_then(Value::as_array).and_then(|infos| {
                infos.iter().find(|info| {
                    info.get("type").and_then(Value::as_str) == Some("page")
                        && info.get("url").and_then(Value::as_str).is_some_and(|url| url.starts_with("http"))
                })
            });
            if let Some(id) = page.and_then(|info| info.get("targetId")).and_then(Value::as_str) {
                return Ok(id.to_owned());
            }
            thread::sleep(Duration::from_millis(100));
        }
        Err("page did not open".into())
    }

    fn session(self: &Arc<Self>, devtools: &Arc<DevTools>, incoming: &mpsc::Receiver<Control>, mode: CaptureMode) -> Result<(), String> {
        let target = self.page_target(devtools)?;
        let attached = devtools.call(
            "Target.attachToTarget",
            json::object([("targetId", target.into()), ("flatten", true.into())]),
            None,
            CALL_TIMEOUT,
        )?;
        let session = attached.get("sessionId").and_then(Value::as_str).ok_or("no sessionId")?.to_owned();
        let call = |method: &str, params: Value| devtools.call(method, params, Some(&session), CALL_TIMEOUT);
        let (width, height) = (u64::from(self.options.width), u64::from(self.options.height));
        // A graphics area: the page lays out at full size, Chromium draws only the area.
        let parts = regions(&self.options, mode == CaptureMode::Frames);
        let area = (!self.options.areas.is_empty()).then(|| parts[0].0);
        self.prepare(devtools, &session, area)?;
        if mode == CaptureMode::Frames {
            // The startup tab draws the first area, a tab of its own each further one.
            let mut sessions = vec![session];
            for &(clip, _) in &parts[1..] {
                sessions.push(self.area_tab(devtools, clip)?);
            }
            return self.frames(devtools, &sessions, incoming);
        }

        // Not faster than the channel needs: half the PNG work at 24–30 fps.
        let screencast = |active: bool| -> Result<u32, String> {
            if active {
                let n = every_nth_frame(self.capture_fps());
                call("Page.stopScreencast", json::object([]))?;
                let params = json::object([
                    ("format", "png".into()),
                    ("maxWidth", width.into()),
                    ("maxHeight", height.into()),
                    ("everyNthFrame", u64::from(n).into()),
                ]);
                call("Page.startScreencast", params)?;
                Ok(n)
            } else {
                call("Page.stopScreencast", json::object([]))?;
                Ok(0)
            }
        };

        let active = self.state().active;
        let mut step = if active { screencast(true)? } else { 0 };
        {
            let mut state = self.state();
            state.status = if state.active { "capturing" } else { "paused" };
        }
        crate::info!("burn-in {}: capturing {}", self.key(), self.options.url);

        // A screenshot of the final state when the screencast went quiet.
        let shooting = Arc::new(AtomicBool::new(false));
        // With a graphics area a static page gets no screencast frame at all: always a screenshot.
        let settles = |step: u32| step > 1 || (step > 0 && area.is_some());
        let mut settle_at = settles(step).then(|| Instant::now() + SETTLE);
        loop {
            let wait = settle_at.map_or(Duration::from_secs(3600), |at| at.saturating_duration_since(Instant::now()));
            match incoming.recv_timeout(wait) {
                Err(RecvTimeoutError::Timeout) => {
                    settle_at = None;
                    // On its own thread, as the Node.js build awaited it: the loop keeps
                    // acking screencast frames meanwhile, one shot at a time.
                    if self.state().active && !shooting.swap(true, Ordering::Relaxed) {
                        let (capture, devtools, session, shooting) =
                            (Arc::clone(self), Arc::clone(devtools), session.clone(), Arc::clone(&shooting));
                        let _ = thread::Builder::new().name("screenshot".into()).spawn(move || {
                            let mut params = json::object([("format", "png".into())]);
                            if let (Some(area), Value::Object(members)) = (area, &mut params) {
                                members.push(("clip".into(), rectangle(area)));
                            }
                            match devtools.call("Page.captureScreenshot", params, Some(&session), CALL_TIMEOUT) {
                                Ok(shot) => {
                                    if let Some(data) = shot.get("data").and_then(Value::as_str) {
                                        capture.push_png(data, Instant::now());
                                    }
                                }
                                Err(error) => crate::warn!("burn-in {}: {error}", capture.key()),
                            }
                            shooting.store(false, Ordering::Relaxed);
                        });
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return Err("browser exited".into()),
                Ok(Control::Active(on)) => {
                    match screencast(on) {
                        Ok(n) => step = n,
                        Err(error) => crate::warn!("burn-in {}: {error}", self.key()),
                    }
                    settle_at = (on && settles(step)).then(|| Instant::now() + SETTLE);
                }
                Ok(Control::DevTools(Incoming::Closed)) => return Err("browser exited".into()),
                Ok(Control::DevTools(Incoming::Event(event))) => {
                    let method = event.get("method").and_then(Value::as_str).unwrap_or("");
                    let params = event.get("params").cloned().unwrap_or(Value::Null);
                    match method {
                        "Page.screencastFrame" if event.get("sessionId").and_then(Value::as_str) == Some(session.as_str()) => {
                            // Ack at once so Chromium renders the next frame while this one is decoded.
                            let ack = json::object([("sessionId", params.get("sessionId").cloned().unwrap_or(Value::Null))]);
                            let _ = devtools.send("Page.screencastFrameAck", ack, Some(&session));
                            if let Some(data) = params.get("data").and_then(Value::as_str) {
                                let timestamp = params.get("metadata").and_then(|m| m.get("timestamp")).and_then(Value::as_f64);
                                self.push_png(data, drawn_at(timestamp));
                            }
                            settle_at = (settles(step) && self.state().active).then(|| Instant::now() + SETTLE);
                        }
                        "Inspector.targetCrashed" | "Target.targetCrashed" | "Target.detachedFromTarget" => {
                            return Err(format!("page closed ({method})"));
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    /// A tab drawing graphics area `area` (`None`: the whole page) with a transparent background.
    fn prepare(&self, devtools: &DevTools, session: &str, area: Option<Area>) -> Result<(), String> {
        let call = |method: &str, params: Value| devtools.call(method, params, Some(session), CALL_TIMEOUT);
        call("Emulation.setDeviceMetricsOverride", self.metrics(area))?;
        // Transparency only through DevTools: `--default-background-color` crashes current renderers.
        let transparent = json::object([("r", 0u64.into()), ("g", 0u64.into()), ("b", 0u64.into()), ("a", 0u64.into())]);
        call("Emulation.setDefaultBackgroundColorOverride", json::object([("color", transparent)]))?;
        call("Page.enable", json::object([]))?;
        if self.options.clock_offset != 0.0 {
            let script = clock_script(self.options.clock_offset);
            call("Page.addScriptToEvaluateOnNewDocument", json::object([("source", script.as_str().into())]))?;
            call("Runtime.evaluate", json::object([("expression", script.as_str().into())]))?;
        }
        Ok(())
    }

    /// One more tab of the page for graphics area `area` (frames mode): its
    /// session. Begin frames drive it as the startup tab, in a window of its
    /// own (a plain tab opened over DevTools stays hidden and never paints).
    fn area_tab(&self, devtools: &DevTools, area: Area) -> Result<String, String> {
        let params =
            json::object([("url", self.options.url.as_str().into()), ("enableBeginFrameControl", true.into()), ("newWindow", true.into())]);
        let created = devtools.call("Target.createTarget", params, None, START_TIMEOUT)?;
        let target = created.get("targetId").and_then(Value::as_str).ok_or("no targetId")?;
        let params = json::object([("targetId", target.into()), ("flatten", true.into())]);
        let attached = devtools.call("Target.attachToTarget", params, None, CALL_TIMEOUT)?;
        let session = attached.get("sessionId").and_then(Value::as_str).ok_or("no sessionId")?.to_owned();
        self.prepare(devtools, &session, Some(area))?;
        Ok(session)
    }

    /// `Emulation.setDeviceMetricsOverride`: the page size, and the part drawn (`viewport`).
    fn metrics(&self, viewport: Option<Area>) -> Value {
        let o = &self.options;
        let mut metrics = json::object([
            ("width", u64::from(o.width).into()),
            ("height", u64::from(o.height).into()),
            ("deviceScaleFactor", 1u64.into()),
            ("mobile", false.into()),
        ]);
        if let (Some(viewport), Value::Object(members)) = (viewport, &mut metrics) {
            members.push(("viewport".into(), rectangle(viewport)));
        }
        metrics
    }

    /// The frames mode: Chromium draws once per slot of the channel's frame
    /// grid, with the slot's time as frame time, so an animation advances
    /// exactly one frame per output frame. A picture is dated by its slot. A
    /// screenshot always redraws and reports damage: while the page moves
    /// every slot asks for one; once it stands still, a slot only probes for
    /// damage and a screenshot follows when there is some. Slots missed by a
    /// slow host are skipped (the picture before repeats), never made up.
    /// Several graphics areas: each slot draws the tab of each area (`sessions`,
    /// one per area) at the same frame time; the picture has one PNG per area.
    fn frames(&self, devtools: &DevTools, sessions: &[String], incoming: &mpsc::Receiver<Control>) -> Result<(), String> {
        let begin = |session: &str, time: f64, interval: f64, screenshot: bool| {
            let mut params = json::object([("frameTimeTicks", time.into()), ("interval", interval.into())]);
            if let (true, Value::Object(members)) = (screenshot, &mut params) {
                let png = json::object([("format", "png".into()), ("optimizeForSpeed", true.into())]);
                members.push(("screenshot".into(), png));
            }
            devtools.call("HeadlessExperimental.beginFrame", params, Some(session), CALL_TIMEOUT)
        };
        // Chromium's frame times are CLOCK_MONOTONIC in ms, as `Instant`.
        let (origin_ms, origin) = (sys::monotonic_ms(), Instant::now());
        let instant = |ms: f64| {
            let offset = Duration::from_secs_f64((ms - origin_ms).abs() / 1000.0);
            if ms >= origin_ms { origin.checked_add(offset) } else { origin.checked_sub(offset) }.unwrap_or(origin)
        };

        let mut frame_ms = 1000.0 / self.capture_fps();
        let now = sys::monotonic_ms();
        if let Err(error) = sessions.iter().try_for_each(|session| begin(session, now, frame_ms, false).map(drop)) {
            if error.contains("wasn't found") {
                self.no_frames.store(true, Ordering::Relaxed);
                crate::warn!(
                    "burn-in {}: no begin frames in this browser (needs chromium-headless-shell); using the screencast",
                    self.key()
                );
            }
            return Err(error);
        }
        let mut active = {
            let mut state = self.state();
            state.status = if state.active { "capturing" } else { "paused" };
            state.active
        };
        crate::info!("burn-in {}: capturing {} (frames)", self.key(), self.options.url);

        // One tab per area.
        let parts = regions(&self.options, true);
        let fresh = || (vec![false; parts.len()], vec![f64::NEG_INFINITY; parts.len()], vec![None::<String>; parts.len()]);
        let mut slot = (sys::monotonic_ms() / frame_ms).ceil();
        let (mut moving, mut last_shot, mut last_png) = fresh();
        let mut pngs: Vec<Option<Arc<[u8]>>> = vec![None; parts.len()];
        let mut tape = self.options.tape.then(TapeRun::default);
        // Slots of the window drawn and missed (a host too slow for the rate).
        let (mut window, mut drawn, mut missed) = (Instant::now(), 0u64, 0u64);
        let mut guard = RateGuard::new(Instant::now());
        // `renderer: auto`: the native renderer draws while all on air is native; Chromium waits without drawing.
        let auto = self.options.renderer == Renderer::Auto;
        let mut native: Option<Native> = None;
        let mut native_retry = Instant::now();
        let mut native_warned = false;
        let mut from_native = false;
        loop {
            let wait = if active { instant(slot * frame_ms).saturating_duration_since(Instant::now()) } else { Duration::from_secs(3600) };
            match incoming.recv_timeout(wait) {
                Err(RecvTimeoutError::Disconnected) | Ok(Control::DevTools(Incoming::Closed)) => return Err("browser exited".into()),
                Ok(Control::Active(on)) => {
                    active = on;
                    (window, drawn, missed) = (Instant::now(), 0, 0);
                    guard.restart(Instant::now());
                    // A new grid at the current rate; the first slot sends a fresh picture.
                    frame_ms = 1000.0 / self.capture_fps();
                    slot = (sys::monotonic_ms() / frame_ms).ceil();
                    (moving, last_shot, last_png) = fresh();
                }
                Ok(Control::DevTools(Incoming::Event(event))) => {
                    let method = event.get("method").and_then(Value::as_str).unwrap_or("");
                    if matches!(method, "Inspector.targetCrashed" | "Target.targetCrashed" | "Target.detachedFromTarget") {
                        return Err(format!("page closed ({method})"));
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    let started = Instant::now();
                    let start = slot * frame_ms;
                    let mut changed = false;
                    if auto && native.is_none() && Instant::now() >= native_retry {
                        match self.start_native() {
                            Ok(n) => native = Some(n),
                            Err(error) => {
                                if !native_warned {
                                    crate::warn!("burn-in {}: {error}; Chromium draws", self.key());
                                    native_warned = true;
                                }
                                native_retry = Instant::now() + RESTART_DELAY * 10;
                            }
                        }
                    }
                    let drawn_natively = match native.as_mut() {
                        Some(n) => match self.native_slot(n, start, instant(start), true, !from_native) {
                            Ok(drawn) => drawn,
                            Err(error) => {
                                crate::warn!("burn-in {}: {error}; Chromium draws", self.key());
                                native = None;
                                native_retry = Instant::now() + RESTART_DELAY;
                                false
                            }
                        },
                        None => false,
                    };
                    if drawn_natively != from_native {
                        crate::info!("burn-in {}: {} draws", self.key(), if drawn_natively { "the native renderer" } else { "Chromium" });
                        from_native = drawn_natively;
                        self.native_drawing.store(drawn_natively, Ordering::Relaxed);
                        // Back to Chromium: a fresh picture of every area at the first slot.
                        (moving, last_shot, last_png) = fresh();
                        pngs = vec![None; parts.len()];
                    }
                    for (i, (&(clip, _), session)) in parts.iter().zip(sessions).enumerate().filter(|_| !drawn_natively) {
                        let time = start;
                        let check = time - last_shot[i] >= CHECK_MS;
                        let mut result = begin(session, time, frame_ms, moving[i] || check)?;
                        if result.get("screenshotData").is_some() {
                            last_shot[i] = time;
                        } else if result.get("hasDamage").and_then(Value::as_bool) == Some(true) {
                            last_shot[i] = time;
                            result = begin(session, time + 0.001, frame_ms, true)?;
                        }
                        let shot = result.get("screenshotData").and_then(Value::as_str);
                        moving[i] = shot.is_some_and(|data| last_png[i].as_deref() != Some(data));
                        match shot {
                            Some(data) if moving[i] => {
                                if let Some(png) = self.png_of(data, (clip.w, clip.h)) {
                                    pngs[i] = Some(png.into());
                                    changed = true;
                                }
                                last_png[i] = Some(data.to_owned());
                            }
                            // The page stood still; the frame after a screenshot reports damage, an empty frame takes it.
                            Some(_) => {
                                begin(session, time + 0.002, frame_ms, false)?;
                            }
                            None => {}
                        }
                    }
                    // A picture once every area has one: a PNG for each, the tape blended into its area.
                    if let Some(all) = pngs.iter().cloned().collect::<Option<Vec<_>>>().filter(|_| !drawn_natively) {
                        let mut shots: Vec<Shot> = all.into_iter().map(Shot::Png).collect();
                        let blended = match tape.as_mut() {
                            Some(run) => {
                                let film = |at: f64, (part, window): (usize, Area), s: i64, id: u64| {
                                    self.film(devtools, &sessions[part], parts[part].0, (at, frame_ms), window, s, id)
                                };
                                self.tape_slot(devtools, sessions, run, &parts, &mut shots, start, &film)
                            }
                            None => false,
                        };
                        if changed || blended {
                            self.history.set_shots(shots, instant(start), &self.decode_areas);
                        }
                    }
                    // Behind the grid: on with the slot of now; the missed ones repeat the picture.
                    let next = (slot + 1.0).max((sys::monotonic_ms() / frame_ms).floor());
                    (drawn, missed) = (drawn + 1, missed + (next - slot - 1.0) as u64);
                    guard.slot((next - slot - 1.0) as u64, started.elapsed());
                    slot = next;
                    if let Some(halved) = guard.verdict(Instant::now(), self.halved.load(Ordering::Relaxed), self.can_halve()) {
                        self.halved.store(halved, Ordering::Relaxed);
                        let fps = self.capture_fps();
                        if halved {
                            crate::warn!(
                                "burn-in {}: too slow for the channel's rate, drawing {fps} fps; a graphics area helps",
                                self.key()
                            );
                        } else {
                            crate::info!("burn-in {}: back to {fps} fps", self.key());
                        }
                        frame_ms = 1000.0 / fps;
                        slot = (sys::monotonic_ms() / frame_ms).ceil();
                        (moving, last_shot, last_png) = fresh();
                    }
                    if window.elapsed() >= MISSED_WINDOW {
                        if missed * 10 > drawn + missed {
                            crate::warn!(
                                "burn-in {}: {missed} of {} frames not drawn in time; lower max_fps or render_size",
                                self.key(),
                                drawn + missed
                            );
                        }
                        (window, drawn, missed) = (Instant::now(), 0, 0);
                    }
                }
            }
        }
    }

    /// The tape of one slot (after the page drew it): looks for a standing
    /// crawl, asks the page to leave its tape to the sidecar, films what the
    /// window needs and blends the tape into the shot of its area. True when
    /// that gives a new picture. Any trouble gives the crawl back to the page.
    #[allow(clippy::too_many_arguments)]
    fn tape_slot(
        &self,
        devtools: &DevTools,
        sessions: &[String],
        run: &mut TapeRun,
        parts: &[(Area, Area)],
        shots: &mut [Shot],
        start: f64,
        film: &dyn Fn(f64, (usize, Area), i64, u64) -> Result<Film, String>,
    ) -> bool {
        if run.track.is_none() && start < run.next_look {
            return false;
        }
        run.next_look = start + CHECK_MS;
        // Each area has its tab (the whole page in each): a crawl counts in the tab of the area it is in.
        let evaluate = |part: usize, expression: String| {
            let params = json::object([("expression", expression.into()), ("returnByValue", true.into())]);
            devtools.call("Runtime.evaluate", params, Some(&sessions[part]), CALL_TIMEOUT)
        };
        let give_back = |part: usize, id: u64| {
            let _ = evaluate(part, format!("window.vizrsTape && vizrsTape.external({id}, false)"));
        };
        // While a crawl is drawn only its tab is asked, else every tab.
        let looked: Vec<usize> = match &run.track {
            Some(t) => vec![t.part],
            None => (0..parts.len()).collect(),
        };
        let mut crawls: Vec<(Crawl, usize, Area)> = Vec::new();
        for part in looked {
            match evaluate(part, "window.vizrsTape ? vizrsTape.state() : null".into()) {
                Ok(result) => {
                    let state = parse_state(result.get("result").and_then(|r| r.get("value")).unwrap_or(&Value::Null));
                    crawls.extend(state.into_iter().filter_map(|c| c.window_in(parts[part].0).map(|w| (c, part, w))));
                }
                Err(error) => crate::warn!("burn-in {}: tape: {error}", self.key()),
            }
        }
        // The tracked crawl while it is on air, else the widest one inside an area.
        let tracked = run.track.as_ref().map(|t| (t.part, t.id));
        let chosen = crawls
            .iter()
            .filter(|(c, part, _)| !run.refused.contains(&(*part, c.id)))
            .max_by(|(a, pa, _), (b, pb, _)| (Some((*pa, a.id)) == tracked).cmp(&(Some((*pb, b.id)) == tracked)).then(a.w.total_cmp(&b.w)));
        let Some(&(ref crawl, part, window)) = chosen else {
            if let Some(track) = run.track.take() {
                give_back(track.part, track.id);
            }
            self.tape_crawl.store(0, Ordering::Relaxed);
            return false;
        };
        match &mut run.track {
            Some(t) if (t.part, t.id) == (part, crawl.id) => {
                if t.key != crawl.key || t.window != window {
                    (t.key, t.window, t.reel) = (crawl.key.clone(), window, Reel::default());
                }
            }
            track => {
                if let Some(old) = track.take() {
                    give_back(old.part, old.id);
                }
                if evaluate(part, format!("vizrsTape.external({}, true)", crawl.id)).is_err() {
                    return false;
                }
                crate::info!("burn-in {}: tape of crawl {} drawn by the sidecar", self.key(), crawl.id);
                *track = Some(Track { id: crawl.id, key: crawl.key.clone(), part, window, reel: Reel::default() });
            }
        }
        if !crawl.hidden {
            self.tape_crawl.store(0, Ordering::Relaxed);
            return false;
        }
        let track = run.track.as_mut().expect("tracked above");
        let s = crawl.offset();
        let mut time = start + 0.004;
        for _ in 0..3 {
            let Some(at) = track.reel.next_film(window, s) else { break };
            match film(time, (part, window), at, crawl.id) {
                Ok(new) => {
                    track.reel.films.push(Arc::new(new));
                    self.tape_films.fetch_add(1, Ordering::Relaxed);
                }
                Err(error) => {
                    crate::warn!("burn-in {}: tape: {error}; the page draws the crawl", self.key());
                    break;
                }
            }
            time += 0.002;
        }
        let Shot::Png(png) = &shots[part] else { return false };
        if !track.reel.covers(window, s) {
            self.tape_crawl.store(0, Ordering::Relaxed);
            run.refused.push((part, crawl.id));
            give_back(part, crawl.id);
            run.track = None;
            return false;
        }
        if run.base.as_ref().is_none_or(|(data, _)| !Arc::ptr_eq(data, png)) {
            match decode_png(png) {
                Ok(picture) => run.base = Some((Arc::clone(png), Arc::new(picture))),
                Err(error) => {
                    crate::warn!("burn-in {}: tape: {error}", self.key());
                    return false;
                }
            }
        }
        self.tape_crawl.store(crawl.id, Ordering::Relaxed);
        let (data, base) = run.base.as_ref().expect("decoded above");
        let last = track.reel.films.last().map_or(0, |f| Arc::as_ptr(f) as usize);
        let shown = (s, Arc::as_ptr(data) as *const u8 as usize, last);
        let new = run.shown != Some(shown);
        run.shown = Some(shown);
        let composite = Composite { base: Arc::clone(base), window, films: track.reel.films.clone(), s };
        shots[part] = Shot::Pixels(Arc::new(move || Ok(composite.render())));
        new
    }

    /// One film of the tape of crawl `id` at offset `s`: the tab of the area
    /// `clip` draws only that tape in one frame (`(time, interval)`), then the
    /// page again; `window` is the crawl window in the area.
    #[allow(clippy::too_many_arguments)]
    fn film(
        &self,
        devtools: &DevTools,
        session: &str,
        clip: Area,
        (time, interval): (f64, f64),
        window: Area,
        s: i64,
        id: u64,
    ) -> Result<Film, String> {
        let call = |method: &str, params: Value| devtools.call(method, params, Some(session), CALL_TIMEOUT);
        let expression = format!("vizrsTape.film({id}, {s})");
        call("Runtime.evaluate", json::object([("expression", expression.into()), ("returnByValue", true.into())]))?;
        let png = json::object([("format", "png".into()), ("optimizeForSpeed", true.into())]);
        let begin = |time: f64, png: Option<Value>| {
            let mut params = json::object([("frameTimeTicks", time.into()), ("interval", interval.into())]);
            if let (Some(png), Value::Object(members)) = (png, &mut params) {
                members.push(("screenshot".into(), png));
            }
            call("HeadlessExperimental.beginFrame", params)
        };
        let shot = begin(time, Some(png));
        // The page back as it was; this frame also takes the damage of the film.
        begin(time + 0.001, None)?;
        let shot = shot?;
        let data = shot.get("screenshotData").and_then(Value::as_str).ok_or("no film")?;
        let png = self.png_of(data, (clip.w, clip.h)).ok_or("bad film")?;
        let picture = decode_png(&png)?;
        Film::cut(&picture, window, s).ok_or_else(|| "film outside the area".into())
    }

    /// The PNG of a screenshot if it has the size drawn (not the page resizing).
    fn png_of(&self, data: &str, size: (u32, u32)) -> Option<Vec<u8>> {
        let png = base64::decode(data).map_err(|error| crate::warn!("burn-in {}: {error}", self.key())).ok()?;
        match png_size(&png) {
            Some(drawn) if drawn == size => Some(png),
            Some((w, h)) => {
                crate::warn!("burn-in {}: frame {w}x{h} skipped", self.key());
                None
            }
            None => {
                crate::warn!("burn-in {}: not a PNG", self.key());
                None
            }
        }
    }

    /// A new picture of the screencast; one of another size (the page resizing) is skipped.
    fn push_png(&self, data: &str, at: Instant) {
        let (clip, _) = regions(&self.options, false)[0];
        if let Some(png) = self.png_of(data, (clip.w, clip.h)) {
            self.history.set_png(png, at, &self.decode);
        }
    }
}

/// Width and height from the PNG header.
/// JavaScript that moves the page's `Date` forward by `seconds`; once only, for
/// the page open now and every page after it. Animations do not use `Date`.
pub fn clock_script(seconds: f64) -> String {
    format!(
        "(() => {{ if (globalThis.__vizrsClock) return; globalThis.__vizrsClock = true; \
         const shift = {ms}, real = Date, now = () => real.now() + shift; \
         globalThis.Date = new Proxy(real, {{ \
         construct: (t, a, n) => Reflect.construct(t, a.length ? a : [now()], n), \
         apply: () => new real(now()).toString(), \
         get: (t, k) => (k === 'now' ? now : Reflect.get(t, k)) }}); }})();",
        ms = (seconds * 1000.0).round()
    )
}

fn png_size(png: &[u8]) -> Option<(u32, u32)> {
    let header = png.get(..24).filter(|h| h.starts_with(b"\x89PNG\r\n\x1a\n") && &h[12..16] == b"IHDR")?;
    let number = |at: usize| u32::from_be_bytes([header[at], header[at + 1], header[at + 2], header[at + 3]]);
    Some((number(16), number(20)))
}

/// `{x, y, width, height, scale}` of DevTools (a viewport, a screenshot clip).
fn rectangle(area: Area) -> Value {
    json::object([
        ("x", u64::from(area.x).into()),
        ("y", u64::from(area.y).into()),
        ("width", u64::from(area.w).into()),
        ("height", u64::from(area.h).into()),
        ("scale", 1u64.into()),
    ])
}

/// Parts of the page drawn into a PNG each: the part at the render size (the
/// viewport) and the part of the channel it is. No areas: the whole page.
/// Areas: each of them (`each`, the frames mode) or their bounding box.
fn regions(o: &CaptureOptions, each: bool) -> Vec<(Area, Area)> {
    let scale = |area: Area| area.scaled((o.out_width, o.out_height), (o.width, o.height));
    match o.areas.as_slice() {
        [] => vec![(Area { x: 0, y: 0, w: o.width, h: o.height }, Area { x: 0, y: 0, w: o.out_width, h: o.out_height })],
        areas if each => areas.iter().map(|&area| (scale(area), area)).collect(),
        areas => vec![(union(&areas.iter().map(|&a| scale(a)).collect::<Vec<_>>()), union(areas))],
    }
}

/// Visible rectangles of PNG number `index` (of `regions`) at the channel
/// size, in channel coordinates, cut to `cut` (the areas, for their bounding box).
fn decoder(regions: Vec<(Area, Area)>, cut: Vec<Area>) -> Arc<Decode> {
    Arc::new(move |index: usize, shot: &Shot| {
        let &(clip, part) = regions.get(index).ok_or("no such area")?;
        let (width, height) = (clip.w, clip.h);
        let picture = shot.picture()?;
        if (picture.width, picture.height) != (width, height) {
            return Err(format!("frame {}x{} skipped", picture.width, picture.height));
        }
        let boxes = visible_rects(&picture.rgba, width, height);
        let mut rects = if (width, height) != (part.w, part.h) {
            scale_rects(&picture.rgba, width, height, &boxes, part.w, part.h)
        } else {
            boxes.into_iter().map(|(x, y, w, h)| Rect { x, y, w, h, pixels: crop(&picture.rgba, width, (x, y, w, h)) }).collect()
        };
        for rect in &mut rects {
            (rect.x, rect.y) = (rect.x + part.x, rect.y + part.y);
        }
        Ok(if cut.len() > 1 { cut_to(rects, &cut) } else { rects })
    })
}

impl Source for Capture {
    fn frame_at(&self, time: Instant) -> Arc<Frame> {
        self.history.frame_at(time)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screencast_step_keeps_the_channel_rate() {
        for (fps, n) in [(0.0, 60), (1.0, 60), (24.0, 2), (25.0, 2), (29.97, 2), (30.0, 2), (50.0, 1), (60.0, 1), (100.0, 1)] {
            assert_eq!(every_nth_frame(fps), n, "{fps}");
        }
    }

    #[test]
    fn the_rate_guard_halves_a_host_too_slow_and_returns() {
        let start = Instant::now();
        let at = |seconds: u64| start + Duration::from_secs(seconds);
        let mut guard = RateGuard::new(start);
        // Keeping up, or nothing to halve to (a 25 fps channel): no change.
        guard.slot(1, Duration::ZERO);
        (0..20).for_each(|_| guard.slot(0, Duration::ZERO));
        assert_eq!(guard.verdict(at(1), false, true), None, "a window takes 5 s");
        assert_eq!(guard.verdict(at(5), false, true), None);
        guard.slot(5, Duration::ZERO);
        assert_eq!(guard.verdict(at(10), false, false), None);
        guard.slot(5, Duration::ZERO);
        assert_eq!(guard.verdict(at(15), false, true), Some(true));

        // Half the rate still takes most of the time: it stays.
        guard.slot(0, Duration::from_secs(4));
        assert_eq!(guard.verdict(at(20), true, true), None);
        // Calm for a minute: back.
        guard.slot(0, Duration::from_millis(100));
        assert_eq!(guard.verdict(at(25), true, true), None);
        guard.slot(0, Duration::from_millis(100));
        assert_eq!(guard.verdict(at(85), true, true), Some(false));

        // That failed at once: the next return waits twice as long.
        guard.slot(9, Duration::ZERO);
        assert_eq!(guard.verdict(at(90), false, true), Some(true));
        assert_eq!(guard.verdict(at(95), true, true), None);
        assert_eq!(guard.verdict(at(155), true, true), None);
        assert_eq!(guard.verdict(at(215), true, true), Some(false));
    }

    #[test]
    fn max_fps_lowers_the_screencast_rate() {
        assert_eq!(every_nth_frame(capped_fps(50.0, 25.0)), 2);
        assert_eq!(every_nth_frame(capped_fps(25.0, 50.0)), 2);
        assert_eq!(every_nth_frame(capped_fps(50.0, 0.0)), 1);
    }

    #[test]
    fn png_size_reads_the_header() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend_from_slice(&1920u32.to_be_bytes());
        png.extend_from_slice(&1080u32.to_be_bytes());
        assert_eq!(png_size(&png), Some((1920, 1080)));
        assert_eq!(png_size(&png[..20]), None);
        assert_eq!(png_size(b"GIF89a, not a PNG at all"), None);
    }

    #[test]
    fn drawn_at_uses_the_timestamp_only_when_plausible() {
        let wall = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64();
        let before = Instant::now();
        assert!(drawn_at(Some(wall - 0.2)) < before);
        assert!(drawn_at(Some(wall - 5.0)) >= before);
        assert!(drawn_at(Some(wall + 5.0)) >= before);
        assert!(drawn_at(None) >= before);
    }
}
