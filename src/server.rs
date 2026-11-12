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
//   GET  /ffmpeg/zones?session=N
//                         `area: auto`: the zones of the session now (`rev`
//                         grows when they widen in the middle of the piece,
//                         PLAN-strefy S3b), for the overlay's next run
//   GET  /ffmpeg/frames?session=N&rev=R&zones=WxH+X+Y,…&at=S[&fps=…]&area=1
//                         a further frame stream of the session in those
//                         zones (empty = the whole channel), its frame 0 at
//                         S seconds into the piece; it replaces the one before
//   GET /ingest/listeners  the listeners in force (VIZRS_INGEST or VizRS Live), for vizrs-ingest
//   POST /ingest?channel=1, GET /ingest?channel=1, GET /ingest/state?channel=1
//                         live ingest: a live source of the channel in, out
//                         to the overlay, its state (src/ingest.rs)
//   GET  /health          captures, sessions and live sources
//   /api/live/*           the live API for VizRS Live (src/panel.rs), with `panel_token`

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use crate::area::{Area, Layout, geometries};
use crate::bounds;
use crate::capture::{Capture, CaptureMode, CaptureOptions, IDLE, find_browser};
use crate::config::Config;
use crate::footprint::{self, Footprints};
use crate::frames::{Empty, Source};
use crate::graphics;
use crate::http::{self, Request};
use crate::ingest::Ingest;
use crate::json::{self, Value};
use crate::native::{self, Renderer};
use crate::overlay::quote_plus;
use crate::panel::Panel;
use crate::scale::render_size;
use crate::stream::{self, Control, Format, Options};
use crate::sync;
use crate::template::{AdMatcher, ClipContext, ShowMatcher, js_string};
use crate::vizrs::{self, ChannelEvents, Timing};
use crate::yuva::ffmpeg_matrix;

pub const MAX_BODY: usize = 4 * 1024 * 1024;
// A channel served by the ffmpeg overlay holds its script packets this long
// for the overlay to take the session over; then ErsatzTV draws them itself.
const HOLD: Duration = Duration::from_millis(3000);
// A session taken over waits this long for the overlay's frame stream.
const FRAMES_WAIT: Duration = Duration::from_millis(10000);
const MAX_WAIT_MS: f64 = 10000.0;
/// A second widening of a run's zones this soon after the first: the whole
/// channel until the end of the piece (PLAN-strefy S3b; no blinking runs).
const WIDEN_WINDOW: Duration = Duration::from_secs(10);
/// A frame stream asked with `delta=1` carries delta records (src/delta.rs), and says so.
pub const DELTA_HEADER: &str = "X-VizRS-Frames: delta\r\n";

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
    /// `render` and `area` of the session, for `/ffmpeg/resume`.
    render: String,
    area: String,
    /// With `area: auto`, the zones the overlay got for this run (PLAN-strefy S2).
    zones: Mutex<Option<Vec<Area>>>,
    /// Pictures with graphics outside those zones (cut off until the next run, S3a).
    outside: AtomicUsize,
    /// Revision of `zones`: grows with each widening in the middle of the piece (S3b).
    zones_rev: AtomicU64,
    /// The overlay asks for the zones (`/ffmpeg/zones`): it starts a new run when they widen.
    follows: AtomicBool,
    widenings: AtomicUsize,
    widened_at: Mutex<Option<Instant>>,
    /// With `area: auto`, the cells drawn in the pictures streamed since the
    /// footprints last learnt (all of them, not one every `LEARN_EVERY`: a
    /// slide-in shorter than that is in its page's footprint too).
    drawn: Mutex<Option<footprint::Cells>>,
    /// Frame streams of the session so far: only the last one ends the script.
    streams: AtomicU64,
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
    /// Pages taken by the elements of single pages (`page=` / `take=`) per
    /// channel, and when (epoch ms): their session may come just after the
    /// overlay took over the piece, so the zones count them beforehand.
    element_pages: HashMap<String, Vec<(String, u64)>>,
    /// Channels where a general VizRS element (no `page=` / `take=` / `cmd=`)
    /// started a session since the sidecar started: only those elements send
    /// the rules' commands (`on_clip`, `on_end`), so only these channels count
    /// the rules' pages in their zones.
    general: HashSet<String>,
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
    /// Events of each channel and the generation of the graphics rules they have.
    channels: Mutex<HashMap<String, (u64, Arc<ChannelEvents>)>>,
    graphics: Arc<graphics::Store>,
    sessions: Shared,
    count: AtomicUsize,
    empty: Empty,
    ingest: Ingest,
    panel: Arc<Panel>,
    /// Footprints of VizRS pages (`area: auto`, PLAN-strefy).
    pub footprints: Mutex<Footprints>,
    /// The layers of the show per /output URL, one native renderer each (`native_layers`).
    layers: Mutex<HashMap<String, ShowLayers>>,
    /// The graphics timeline of each channel's overlay, carried from one run to the next.
    carries: Mutex<HashMap<String, Arc<stream::Carry>>>,
}

/// The layers of a show for the native renderers of its /output URL.
struct ShowLayers {
    /// A channel showing it (its VizRS address, token and bus).
    channel: String,
    list: native::LayerList,
    /// When the show was read; `None` = not yet (read at once, in the background).
    fetched: Option<Instant>,
}

/// How long the zones count a page an element of a single page took on the channel.
const ELEMENT_PAGES_KEEP_MS: u64 = 600_000;
/// How often the layers on air are checked against the list, and how often the show is read anyway.
const LAYERS_EVERY: Duration = Duration::from_secs(2);
const SHOW_EVERY: Duration = Duration::from_secs(30);

/// How often the footprints learn from the pictures and `STATE` of each channel.
const LEARN_EVERY: Duration = Duration::from_millis(500);
/// How often learnt footprints are written to their file.
const SAVE_EVERY: Duration = Duration::from_secs(10);
/// How often the show's revision is checked for new page bounds (PLAN-strefy S6), and after a failure.
const BOUNDS_EVERY: Duration = Duration::from_secs(5);
const BOUNDS_RETRY: Duration = Duration::from_secs(60);
/// A channel size (width, height).
type Size = (u32, u32);

impl Server {
    pub fn new(config: Config) -> Arc<Server> {
        crate::asrun::set_webhook(&config.alert_webhook);
        let graphics = Arc::new(graphics::Store::new(&config));
        let (ads, warnings) = AdMatcher::new(&graphics.applied(&config).ad_match);
        for warning in warnings {
            crate::warn!("{warning}");
        }
        let (_, show_warnings) = ShowMatcher::new(&graphics.applied(&config).shows);
        for warning in show_warnings {
            crate::warn!("{warning}");
        }
        let browser = if config.browser.is_empty() { find_browser() } else { Some(config.browser.clone()) };
        let ingest = Ingest::with_config(&config.ingest_token, config.ingest_anytime, config.etv_config.clone())
            .with_idle(Duration::from_secs_f64(config.ingest_idle_seconds.clamp(1.0, 3600.0)));
        let panel = Panel::new(&config, &ingest, Arc::clone(&graphics));
        let footprints = Mutex::new(Footprints::load(Footprints::default_path(&config.etv_config)));
        Arc::new(Server {
            footprints,
            layers: Mutex::new(HashMap::new()),
            carries: Mutex::new(HashMap::new()),
            ingest,
            panel,
            config: Arc::new(config),
            ads: Arc::new(ads),
            browser,
            channels: Mutex::new(HashMap::new()),
            graphics,
            sessions: Shared::default(),
            count: AtomicUsize::new(0),
            empty: Empty::new(),
        })
    }

    /// With `native_layers`, keeps the layers of the shows of the native
    /// renderers up to date, in the background: a layer on air that is not on
    /// the list (a scene moved to a new layer) reads the show at once.
    pub fn start_layers(self: &Arc<Self>) {
        if !self.config.native_layers {
            return;
        }
        let server = Arc::clone(self);
        let _ = thread::Builder::new().name("layers".into()).spawn(move || {
            let mut checked = Instant::now();
            loop {
                thread::sleep(Duration::from_millis(100));
                let all = checked.elapsed() >= LAYERS_EVERY;
                if all {
                    checked = Instant::now();
                }
                server.refresh_layers(all);
            }
        });
    }

    /// The layers of the show of `url` for the native renderers of a capture
    /// (`native_layers`; read in the background, empty until then = one
    /// renderer for all); `None` = one renderer for all.
    fn layers_for(&self, channel: &str, url: &str) -> Option<native::LayerList> {
        if !self.config.native_layers || matches!(Renderer::parse(&self.config.renderer), Ok(Renderer::Chromium) | Err(_)) {
            return None;
        }
        let mut layers = self.layers.lock().unwrap_or_else(|e| e.into_inner());
        let entry = layers.entry(url.to_owned()).or_insert_with(|| ShowLayers {
            channel: channel.to_owned(),
            list: Arc::new(Mutex::new(Vec::new())),
            fetched: None,
        });
        Some(Arc::clone(&entry.list))
    }

    /// The layers of the show of `url` and those `on_air`, bottom first.
    fn read_layers(&self, channel: &str, url: &str, on_air: &[String]) -> Option<Vec<String>> {
        let config = self.config.for_channel(channel);
        let api = sync::http_url_of(url)?;
        let show = match sync::fetch_show(&api, &config.vizrs.token) {
            Ok(show) => show,
            Err(error) => {
                crate::debug!("native renderer: layers of the show: {error}");
                return None;
            }
        };
        let names = native::show_layers(&show).map_err(|error| crate::debug!("native renderer: {error}")).ok()?;
        Some(native::stack(names.into_iter().chain(on_air.iter().cloned())))
    }

    /// One step of `start_layers`: shows not read yet, and with `all` the
    /// layers on air of every show against its list.
    fn refresh_layers(&self, all: bool) {
        let entries: Vec<(String, String, native::LayerList, Option<Instant>)> = self
            .layers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|(_, e)| all || e.fetched.is_none())
            .map(|(url, e)| (url.clone(), e.channel.clone(), Arc::clone(&e.list), e.fetched))
            .collect();
        for (url, channel, list, fetched) in entries {
            let now = list.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let due = fetched.is_none_or(|f| f.elapsed() >= SHOW_EVERY);
            // Layers on air not on the list: only for a show read already (no API, no list, no `STATE`).
            let on_air = if !due && !now.is_empty() {
                let config = self.config.for_channel(&channel);
                vizrs::state(&config.vizrs.address, &config.vizrs.token, &config.command_prefix)
                    .map(|reply| native::state_layers(&reply))
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            if !due && on_air.iter().all(|l| now.contains(l)) {
                continue;
            }
            if let Some(entry) = self.layers.lock().unwrap_or_else(|e| e.into_inner()).get_mut(&url) {
                entry.fetched = Some(Instant::now());
            }
            let Some(names) = self.read_layers(&channel, &url, &on_air) else { continue };
            if names != now {
                crate::info!("native renderer: one per layer of the show: {}", names.join(", "));
                *list.lock().unwrap_or_else(|e| e.into_inner()) = names;
            }
        }
    }

    /// With `area: auto`, learns the footprints of the pages on air from the
    /// pictures of the channels' captures (PLAN-strefy S1), in the background.
    pub fn start_footprints(self: &Arc<Self>) {
        if !Area::is_auto(&self.config.area) {
            return;
        }
        let server = Arc::clone(self);
        let _ = thread::Builder::new().name("footprints".into()).spawn(move || {
            let mut saved = std::time::Instant::now();
            loop {
                thread::sleep(LEARN_EVERY);
                server.learn_footprints();
                if saved.elapsed() >= SAVE_EVERY {
                    server.footprints.lock().unwrap_or_else(|e| e.into_inner()).save();
                    saved = std::time::Instant::now();
                }
            }
        });
        let server = Arc::clone(self);
        let _ = thread::Builder::new().name("bounds".into()).spawn(move || server.bounds_loop());
        crate::info!("graphics zones: auto (footprints of the pages on air)");
    }

    /// PLAN-strefy S6: the bounds of the show's pages from the native renderer
    /// (`vizrs-render bounds`) at every channel size with a capture, computed
    /// again for a page whose revision or scene changed (`rev` of the show in `STATE`).
    fn bounds_loop(&self) {
        let bin = native::find_renderer(&self.config.renderer_bin);
        let mut done = bounds::Done::new();
        // the show's revision and the sizes the last update covered
        let mut last: Option<(Option<f64>, Vec<Size>)> = None;
        let mut failing = false;
        loop {
            thread::sleep(if failing { BOUNDS_RETRY } else { BOUNDS_EVERY });
            let mut sizes: Vec<(u32, u32)> = lock(&self.sessions)
                .scripts
                .values()
                .filter_map(|s| s.capture.as_ref().filter(|c| c.areas().is_empty()).map(|c| c.out_size()))
                .collect();
            sizes.sort_unstable();
            sizes.dedup();
            if sizes.is_empty() {
                continue;
            }
            let rev = vizrs::state(&self.config.vizrs.address, &self.config.vizrs.token, "").ok().as_deref().and_then(bounds::state_rev);
            let now = (rev, sizes.clone());
            if rev.is_some() && last.as_ref() == Some(&now) {
                continue;
            }
            match self.update_bounds(&bin, &sizes, &mut done) {
                Ok(()) => {
                    failing = false;
                    last = Some(now);
                }
                Err(e) => {
                    if !failing {
                        crate::warn!("graphics zones: no page bounds from VizRS ({e}); footprints are learnt from the pictures only");
                    }
                    failing = true;
                }
            }
        }
    }

    /// Bounds of the pages not computed yet (or changed since) at each of `sizes`.
    fn update_bounds(&self, bin: &str, sizes: &[(u32, u32)], done: &mut bounds::Done) -> Result<(), String> {
        let text = bounds::fetch_show(&self.config.output_url, &self.config.vizrs.token)?;
        let show = json::parse(&text).map_err(|_| "the show from VizRS is not JSON".to_owned())?;
        let pages = bounds::pages(&show);
        let file = std::env::temp_dir().join(format!("vizrs-cg-show-{}.json", std::process::id()));
        let file_name = file.to_string_lossy().into_owned();
        for &size in sizes {
            let todo: Vec<&bounds::Page> =
                pages.iter().filter(|p| done.get(&bounds::done_key(&p.key, size)) != Some(&p.signature)).collect();
            if todo.is_empty() {
                continue;
            }
            std::fs::write(&file, &text).map_err(|e| format!("{file_name}: {e}"))?;
            let numbers: Vec<String> = todo.iter().map(|p| p.number.clone()).collect();
            let started = std::time::Instant::now();
            let out = bounds::run(bin, &file_name, &self.config.output_url, size, &numbers);
            let _ = std::fs::remove_file(&file);
            let out = out?;
            let mut native = 0;
            {
                let mut footprints = self.footprints.lock().unwrap_or_else(|e| e.into_inner());
                for (key, rects) in out.lines().filter_map(bounds::parse_line) {
                    native += usize::from(rects.is_some());
                    footprints.set_bounds(&key, size, rects.as_deref());
                }
            }
            for page in todo {
                done.insert(bounds::done_key(&page.key, size), page.signature.clone());
            }
            crate::info!(
                "graphics zones: bounds of {} pages from VizRS at {}x{} in {:.1} s ({native} drawn natively)",
                numbers.len(),
                size.0,
                size.1,
                started.elapsed().as_secs_f64()
            );
        }
        Ok(())
    }

    /// The zones of a run of `channel` (PLAN-strefy S2): footprints of the pages
    /// on its antenna and of the pages its rules take, and of all the pages known
    /// for an overlay that cannot widen them (not `follows`); `None` = the whole channel.
    fn zones_for(&self, channel: &str, size: (u32, u32), follows: bool) -> Option<Vec<Area>> {
        let config = self.graphics.applied(&self.config.for_channel(channel));
        let on_air = footprint::on_air(&vizrs::state(&config.vizrs.address, &config.vizrs.token, &config.command_prefix).ok()?)?;
        let mut lines: Vec<String> = Vec::new();
        // The rules go out only through general elements: a channel with
        // elements of single pages alone never takes their pages, and a page
        // that never comes on air would cost every run the whole channel.
        if lock(&self.sessions).general.contains(channel) {
            let mut add = |text: &str| lines.extend(text.lines().map(str::to_owned));
            add(&config.on_clip);
            add(&config.on_end);
            for show in &config.shows {
                add(show.on_clip.as_deref().unwrap_or(""));
                add(show.on_end.as_deref().unwrap_or(""));
            }
        }
        let mut numbers = vizrs::taken_pages(&lines);
        // Pages this piece may take besides its rules: back from a break, or by
        // the elements of single pages (their session may come after this).
        let now = crate::time::now_ms() as u64;
        numbers.extend(self.events_for(channel).break_pages(now));
        let elements = lock(&self.sessions).element_pages.get(channel).cloned().unwrap_or_default();
        numbers.extend(elements.into_iter().filter(|(_, at)| now.saturating_sub(*at) <= ELEMENT_PAGES_KEEP_MS).map(|(page, _)| page));
        let mut seen = HashSet::new();
        numbers.retain(|p| !p.contains('{') && seen.insert(p.clone()));
        let footprints = self.footprints.lock().unwrap_or_else(|e| e.into_inner());
        let leaving = footprints.leaving(channel, crate::time::now_ms() as u64);
        footprint::zones(&footprints, &on_air, &leaving, &numbers, size, follows)
    }

    /// One learning step: per channel with a capture, its last picture and the pages on its antenna.
    fn learn_footprints(&self) {
        let mut on_air: HashMap<String, Vec<footprint::OnAir>> = HashMap::new();
        // The cells each channel draws, as boxes (finer than the visible rectangles).
        let mut drawn: HashMap<String, Vec<(u32, u32, u32, u32)>> = HashMap::new();
        let channels: Vec<(String, Arc<Capture>)> = {
            let sessions = lock(&self.sessions);
            let mut seen = Vec::<(String, Arc<Capture>)>::new();
            for script in sessions.scripts.values() {
                if let Some(capture) = &script.capture {
                    if !seen.iter().any(|(c, _)| *c == script.channel) {
                        seen.push((script.channel.clone(), Arc::clone(capture)));
                    }
                }
            }
            seen
        };
        for (channel, capture) in channels {
            // A capture with fixed areas draws only them: nothing to learn about the rest.
            if !capture.areas().is_empty() {
                continue;
            }
            // The picture before `STATE`: a page taken in between is on the list
            // but not in the picture, never the other way round (its pixels would
            // go to the pages on air before it).
            let now = crate::time::now_ms() as u64;
            let picture = capture.current().picture();
            let config = self.config.for_channel(&channel);
            let pages = match vizrs::state(&config.vizrs.address, &config.vizrs.token, &config.command_prefix) {
                Ok(reply) => footprint::on_air(&reply).unwrap_or_default(),
                Err(_) => continue,
            };
            let last = footprint::Cells::of_picture(&picture.rects, capture.out_size());
            // With the cells of every picture the channel's sessions streamed since the last step.
            let mut cells = last.clone();
            for script in lock(&self.sessions).scripts.values().filter(|s| s.channel == channel) {
                if let Some(drawn) = script.drawn.lock().unwrap_or_else(|e| e.into_inner()).take() {
                    cells.add(&drawn);
                }
            }
            if self.footprints.lock().unwrap_or_else(|e| e.into_inner()).learn_between(&channel, &pages, &cells, &last, now) {
                crate::debug!("footprints: channel {channel} learnt from {} page(s)", pages.len());
            }
            on_air.insert(channel.clone(), pages);
            drawn.insert(channel, cells.boxes());
        }
        self.widen_runs(&on_air, &drawn);
    }

    /// Runs of the overlay whose graphics leave their zones (PLAN-strefy S3a,
    /// S3b): with an overlay that follows the zones, they widen and its next
    /// run (started at once) gets them; else they are cut off until the next run.
    fn widen_runs(&self, on_air: &HashMap<String, Vec<footprint::OnAir>>, drawn: &HashMap<String, Vec<(u32, u32, u32, u32)>>) {
        let running: Vec<Arc<Script>> =
            lock(&self.sessions).scripts.values().filter(|s| s.claimed.load(Ordering::Relaxed)).cloned().collect();
        for script in running {
            if script.capture.is_none() {
                continue;
            }
            let zones = script.zones.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let Some(zones) = zones.filter(|z| !z.is_empty()) else { continue };
            let Some(rects) = drawn.get(&script.channel) else { continue };
            let size = (script.ctx.width, script.ctx.height);
            let pages = on_air.get(&script.channel).map_or(&[][..], Vec::as_slice);
            let reason = {
                let footprints = self.footprints.lock().unwrap_or_else(|e| e.into_inner());
                footprint::widen_reason(&footprints, &zones, pages, rects, size)
            };
            let Some(reason) = reason else { continue };
            if !footprint::covered(&zones, rects) {
                script.outside.fetch_add(1, Ordering::Relaxed);
            }
            if script.follows.load(Ordering::Relaxed) {
                self.widen(&script, pages, rects, &reason);
            } else if !footprint::covered(&zones, rects) && script.outside.load(Ordering::Relaxed) == 1 {
                crate::warn!("channel {}: graphics outside the zones of this run, cut off until the next run", script.channel);
            }
        }
    }

    /// New zones of a run: those of a run starting now, if they hold what is on
    /// air and drawn; else, or a second time within `WIDEN_WINDOW`, the whole channel.
    fn widen(&self, script: &Script, pages: &[footprint::OnAir], rects: &[(u32, u32, u32, u32)], reason: &str) {
        let size = (script.ctx.width, script.ctx.height);
        let mut widened_at = script.widened_at.lock().unwrap_or_else(|e| e.into_inner());
        let again = widened_at.is_some_and(|at| at.elapsed() < WIDEN_WINDOW);
        let zones = if again { None } else { self.zones_for(&script.channel, size, true) }.filter(|z| {
            let footprints = self.footprints.lock().unwrap_or_else(|e| e.into_inner());
            footprint::widen_reason(&footprints, z, pages, rects, size).is_none()
        });
        *widened_at = Some(Instant::now());
        match &zones {
            Some(z) => crate::info!("channel {}: graphics zones widened to {} ({reason})", script.channel, geometries(z)),
            None if again => crate::info!(
                "channel {}: graphics zones: the whole channel till the end of the piece ({reason}, widened again)",
                script.channel
            ),
            None => crate::info!("channel {}: graphics zones: the whole channel ({reason})", script.channel),
        }
        *script.zones.lock().unwrap_or_else(|e| e.into_inner()) = zones;
        script.widenings.fetch_add(1, Ordering::Relaxed);
        script.zones_rev.fetch_add(1, Ordering::Relaxed);
    }

    /// With `area: auto`, the cells of a picture streamed by `script`, added to
    /// those its footprints learn from next; `None` without `area: auto`.
    fn draws(&self, script: &Script, picture: &crate::frames::Picture, size: (u32, u32)) -> Option<footprint::Cells> {
        if !Area::is_auto(&self.config.area) {
            return None;
        }
        let cells = footprint::Cells::of_picture(&picture.rects, size);
        let mut drawn = script.drawn.lock().unwrap_or_else(|e| e.into_inner());
        match drawn.as_mut() {
            Some(drawn) => drawn.add(&cells),
            None => *drawn = Some(cells.clone()),
        }
        Some(cells)
    }

    /// A run with the zones of revision `rev` holds a picture whose `cells`
    /// leave them: zones newer than that holding it are enough (the overlay is
    /// on its way to them), else they widen now, not at the next learning step.
    fn widen_for(&self, script: &Script, rev: u64, cells: &footprint::Cells) {
        let newer = script.zones_rev.load(Ordering::Relaxed) > rev
            && script.zones.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_none_or(|z| z.is_empty() || cells.within(z));
        if newer {
            crate::debug!("channel {}: frames held for the graphics zones widened already", script.channel);
            return;
        }
        let config = self.config.for_channel(&script.channel);
        let pages = vizrs::state(&config.vizrs.address, &config.vizrs.token, &config.command_prefix)
            .ok()
            .and_then(|reply| footprint::on_air(&reply))
            .unwrap_or_default();
        script.outside.fetch_add(1, Ordering::Relaxed);
        self.widen(script, &pages, &cells.boxes(), "graphics outside the zones, frames held");
    }

    fn events_for(&self, channel: &str) -> Arc<ChannelEvents> {
        let generation = self.graphics.generation();
        let mut channels = self.channels.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((seen, events)) = channels.get_mut(channel) {
            if *seen != generation {
                // VizRS Live replaced the graphics rules: from this clip on.
                let (config, ads, shows) = self.rules_for(channel);
                events.set_rules(&config, ads, shows);
                *seen = generation;
            }
            return Arc::clone(events);
        }
        let (config, ads, shows) = self.rules_for(channel);
        let events = ChannelEvents::new(Arc::new(config), ads, shows, channel.to_owned());
        channels.insert(channel.to_owned(), (generation, Arc::clone(&events)));
        events
    }

    /// The channel's configuration with the graphics rules in force, and their matchers.
    fn rules_for(&self, channel: &str) -> (Config, Arc<AdMatcher>, Arc<ShowMatcher>) {
        let config = self.graphics.applied(&self.config).for_channel(channel);
        let ads = if self.graphics.generation() == 0 {
            Arc::clone(&self.ads)
        } else {
            let (ads, warnings) = AdMatcher::new(&config.ad_match);
            for warning in warnings {
                crate::warn!("channel {channel}: {warning}");
            }
            Arc::new(ads)
        };
        let (shows, warnings) = ShowMatcher::new(&config.shows);
        for warning in warnings {
            crate::warn!("channel {channel}: {warning}");
        }
        (config, ads, Arc::new(shows))
    }

    /// One capture per /output page, channel size, render size and graphics area, shared by the channels that have them.
    #[allow(clippy::too_many_arguments)]
    fn capture_for(&self, channel: &str, url: &str, width: u32, height: u32, fps: f64, spec: &str, area: &str) -> Graphics {
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
            gpu: self.config.browser_gpu,
            extra_args: self.config.browser_args.clone(),
            idle: IDLE,
            history: stream::history(self.config.lead_seconds),
            clock_offset: self.config.clock_offset_seconds,
            tape: self.config.tape,
            renderer,
            renderer_bin: native::find_renderer(&self.config.renderer_bin),
            layers: self.layers_for(channel, url),
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
                    ("layers", Value::Array(c.native_layers().into_iter().map(Value::String).collect())),
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
        // `area: auto` (PLAN-strefy S5): the zones of the overlay runs and the pages with footprints.
        let zones = Area::is_auto(&self.config.area).then(|| {
            let runs: Vec<Value> = sessions
                .scripts
                .values()
                .filter(|s| s.claimed.load(Ordering::Relaxed))
                .map(|s| {
                    let zones = s.zones.lock().unwrap_or_else(|e| e.into_inner()).clone();
                    let channel = f64::from(s.ctx.width) * f64::from(s.ctx.height);
                    let (geometry, share) = match zones.filter(|z| !z.is_empty()) {
                        Some(z) => {
                            let pixels: u64 = z.iter().map(|a| u64::from(a.w) * u64::from(a.h)).sum();
                            (Value::from(geometries(&z)), (100.0 * pixels as f64 / channel).round())
                        }
                        None => (Value::Null, 100.0),
                    };
                    json::object([
                        ("channel", s.channel.as_str().into()),
                        ("zones", geometry),
                        ("share_percent", share.into()),
                        ("outside", (s.outside.load(Ordering::Relaxed) as u64).into()),
                        ("widenings", (s.widenings.load(Ordering::Relaxed) as u64).into()),
                    ])
                })
                .collect();
            let footprints = self.footprints.lock().unwrap_or_else(|e| e.into_inner());
            let (pages, bounds) = (footprints.len(), footprints.bounds_len());
            drop(footprints);
            json::object([("runs", Value::Array(runs)), ("pages", (pages as u64).into()), ("bounds", (bounds as u64).into())])
        });
        json::object([
            ("sessions", (self.count.load(Ordering::Relaxed) as u64).into()),
            ("captures", Value::Array(captures)),
            ("overlay", sessions.overlay.iter().map(String::as_str).collect::<Vec<_>>().into()),
            ("ingest", if self.ingest.enabled() { self.ingest.health() } else { Value::Null }),
            ("zones", zones.unwrap_or(Value::Null)),
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
            (_, path) if Panel::owns(path) => self.panel.handle(&request, stream, &self.ingest),
            ("GET", "/health") => http::respond(&mut stream, 200, Some("application/json"), self.health().to_string().as_bytes()),
            ("GET", "/etv/pages") => self.pages_request(&request, stream),
            ("GET", "/ffmpeg") => self.claim_request(&request, stream),
            ("GET", "/ffmpeg/resume") => self.resume_request(&request, stream),
            ("GET", "/ffmpeg/zones") => self.zones_request(&request, stream),
            ("GET", "/ffmpeg/frames") => {
                self.frames_request(&request, stream);
                Ok(())
            }
            ("GET", "/ingest/listeners") => self.panel.listeners_request(&request, stream, &self.ingest),
            ("GET", "/ingest/state") => self.ingest.state_request(&request, stream),
            ("GET", "/ingest") => self.ingest.read_request(&request, stream),
            ("POST", "/ingest") => self.ingest.source_request(&request, stream),
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
        if placed && Area::is_auto(&self.config.area) && script.capture.as_ref().is_some_and(|c| c.areas().is_empty()) {
            // An overlay that can widen the zones in the middle of the piece (S3b) says so.
            let follows = request.param("follow") == Some("1");
            script.follows.store(follows, Ordering::Relaxed);
            let zones = self.zones_for(&channel, (script.ctx.width, script.ctx.height), follows);
            match &zones {
                Some(z) => {
                    let pixels: u64 = z.iter().map(|a| u64::from(a.w) * u64::from(a.h)).sum();
                    let share = 100.0 * pixels as f64 / (f64::from(script.ctx.width) * f64::from(script.ctx.height));
                    crate::info!("channel {channel}: graphics zones {} ({share:.0}% of the channel)", geometries(z));
                }
                None => crate::info!("channel {channel}: graphics zones: the whole channel"),
            }
            *script.zones.lock().unwrap_or_else(|e| e.into_inner()) = zones;
        }
        let layout = frame_layout(&script, placed);
        let now_ms = crate::time::now_ms() as f64;
        // Going on with the channel's timeline: all it has drawn since is ready at once.
        let carried = script.ctx.air_ms().and_then(|air| {
            let carry = self.carries.lock().unwrap_or_else(|e| e.into_inner()).get(&channel).cloned()?;
            crate::stream::carried_ahead(&carry, air, now_ms, self.config.lead_seconds)
        });
        let ahead = carried.unwrap_or_else(|| crate::stream::graphics_ahead(script.ctx.air_ms(), now_ms, self.config.lead_seconds));
        let mut reply = json::object([
            ("session", script.id.into()),
            ("ahead", ((ahead * 1000.0).round() / 1000.0).into()),
            ("width", u64::from(layout.width).into()),
            ("height", u64::from(layout.height).into()),
            ("fps", script.fps.into()),
            ("matrix", ffmpeg_matrix(script.ctx.height).into()),
            ("resume", resume_query(&script, placed).into()),
        ]);
        if let (true, Value::Object(members)) = (placed, &mut reply) {
            let (first, _) = layout.parts[0];
            members.push(("x".into(), u64::from(first.x).into()));
            members.push(("y".into(), u64::from(first.y).into()));
            members.push(("areas".into(), areas_json(&layout)));
            // Zones that may widen in the middle of the piece: the overlay follows `/ffmpeg/zones`.
            if script.zones.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|z| !z.is_empty()) {
                members.push(("zones_rev".into(), script.zones_rev.load(Ordering::Relaxed).into()));
            }
        }
        http::respond(&mut stream, 200, Some("application/json"), reply.to_string().as_bytes())
    }

    /// `/ffmpeg/zones`: the zones of a session taken over now, for the overlay's next run.
    fn zones_request(&self, request: &Request, mut stream: TcpStream) -> std::io::Result<()> {
        let id = js_number(request.param("session").unwrap_or(""));
        let script =
            lock(&self.sessions).scripts.get(&(id as u64)).filter(|s| id.fract() == 0.0 && s.claimed.load(Ordering::Relaxed)).cloned();
        let Some(script) = script else {
            return http::respond(&mut stream, 404, None, b"");
        };
        script.follows.store(true, Ordering::Relaxed);
        let layout = frame_layout(&script, true);
        let zones = script.zones.lock().unwrap_or_else(|e| e.into_inner()).clone().filter(|z| !z.is_empty());
        let reply = json::object([
            ("rev", script.zones_rev.load(Ordering::Relaxed).into()),
            ("zones", zones.as_deref().map_or(Value::Null, |z| geometries(z).into())),
            ("width", u64::from(layout.width).into()),
            ("height", u64::from(layout.height).into()),
            ("areas", areas_json(&layout)),
            ("resume", resume_query(&script, true).into()),
        ]);
        http::respond(&mut stream, 200, Some("application/json"), reply.to_string().as_bytes())
    }

    fn frames_request(&self, request: &Request, mut stream: TcpStream) {
        let id = js_number(request.param("session").unwrap_or(""));
        let script = lock(&self.sessions).scripts.get(&(id as u64)).filter(|_| id.fract() == 0.0).cloned();
        // A further stream (`rev`, S3b) replaces the one before; the first is taken once.
        let further = request.param("rev").is_some();
        let taken = script.filter(|s| s.claimed.load(Ordering::Relaxed) && (further || !s.frames.swap(true, Ordering::Relaxed)));
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
        let placed = request.param("area") == Some("1");
        // The zones this stream's run was built for, and their revision.
        let mut rev = script.zones_rev.load(Ordering::Relaxed);
        let (layout, air) = if further {
            // The zones the overlay's new run was built for, and that run's place in the piece.
            let (width, height) = (script.ctx.width, script.ctx.height);
            let layout = match Area::parse_list(request.param("zones").unwrap_or(""), width, height) {
                Ok(zones) if placed && !zones.is_empty() => Layout::atlas(&zones),
                Ok(_) => Layout::whole(width, height),
                Err(error) => {
                    crate::warn!("ffmpeg overlay: zones of session {id}: {error}");
                    let _ = http::respond(&mut stream, 400, None, b"");
                    return;
                }
            };
            let at = js_number(request.param("at").unwrap_or(""));
            let at = if at.is_finite() { at.max(0.0) } else { 0.0 };
            rev = request.param("rev").and_then(|r| r.parse().ok()).unwrap_or(rev);
            let zones = match layout_zones(&layout, width, height) {
                zones if zones.is_empty() => "the whole channel".to_owned(),
                zones => geometries(&zones),
            };
            crate::info!("channel {}: ffmpeg overlay run from {at:.1} s, graphics zones {zones}", script.channel);
            (layout, script.ctx.air_ms().map(|air| air + at * 1000.0))
        } else {
            (frame_layout(&script, placed), script.ctx.air_ms())
        };
        let number = script.streams.fetch_add(1, Ordering::Relaxed) + 1;
        let reader = capture.acquire(fps);
        let control = Control::new(false);
        watch_close(&stream, &control);
        let delta = request.param("delta") == Some("1");
        if let Ok(mut out) = http::Stream::start_with(stream, request, if delta { DELTA_HEADER } else { "" }) {
            let size = (script.ctx.width, script.ctx.height);
            let zones = layout_zones(&layout, size.0, size.1);
            let mut held = false;
            let format = Format::Yuva { layout, hd: script.ctx.height >= 720, delta };
            let options = Options { fps, lead_seconds: self.config.lead_seconds, format, air };
            let carry = Arc::clone(self.carries.lock().unwrap_or_else(|e| e.into_inner()).entry(script.channel.clone()).or_default());
            stream::stream_carried(
                &*capture,
                &mut out,
                &options,
                &control,
                &mut |picture| {
                    let Some(cells) = self.draws(&script, picture, size) else { return false };
                    // A picture leaving the zones of this run (PLAN-strefy S3b): the run
                    // stops before it, so no frame is cut off, and the next one shows it.
                    if held
                        || zones.is_empty()
                        || cells.within(&zones)
                        || !script.follows.load(Ordering::Relaxed)
                        || script.streams.load(Ordering::Relaxed) != number
                    {
                        return false;
                    }
                    held = true;
                    self.widen_for(&script, rev, &cells);
                    true
                },
                Some(&carry),
            );
        }
        capture.release(reader);
        // The transcode is over: the script ends with it, as without the overlay
        // (not when a further stream of the session replaced this one).
        if script.streams.load(Ordering::Relaxed) == number {
            script.stream.close();
        }
    }

    /// The frames of an overlay whose sidecar restarted: its ffmpeg runs on, the
    /// script session is gone, so the stream stands on its own until ffmpeg ends.
    fn resume_request(&self, request: &Request, mut stream: TcpStream) -> std::io::Result<()> {
        let channel = request.param("channel").unwrap_or("1");
        let (width, height) = (js_number(request.param("width").unwrap_or("")), js_number(request.param("height").unwrap_or("")));
        let fps = parse_rate(request.param("fps"));
        if !(width >= 1.0 && height >= 1.0 && fps > 0.0) {
            return http::respond(&mut stream, 400, None, b"");
        }
        let (width, height) = (width as u32, height as u32);
        let url = self.config.for_channel(channel).output_url;
        let spec = request.param("render").unwrap_or("");
        let area = request.param("graphics").unwrap_or("");
        let Graphics::Capture(capture) = self.capture_for(channel, &url, width, height, fps, spec, area) else {
            return http::respond(&mut stream, 404, None, b"");
        };
        crate::info!("channel {channel}: ffmpeg overlay reconnected");
        let reader = capture.acquire(fps);
        let control = Control::new(false);
        watch_close(&stream, &control);
        let delta = request.param("delta") == Some("1");
        if let Ok(mut out) = http::Stream::start_with(stream, request, if delta { DELTA_HEADER } else { "" }) {
            let layout = match capture.areas() {
                areas if request.param("area") == Some("1") && !areas.is_empty() => Layout::atlas(areas),
                _ => Layout::whole(width, height),
            };
            let format = Format::Yuva { layout, hd: height >= 720, delta };
            stream::stream_frames(
                &*capture,
                &mut out,
                &Options { fps, lead_seconds: self.config.lead_seconds, format, air: None },
                &control,
            );
        }
        capture.release(reader);
        Ok(())
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
        let (timing, warnings) = Timing::from_params(request.param("at"), request.param("out"), request.param("ref"), &self.config);
        for warning in warnings {
            crate::warn!("channel {channel}: {warning}");
        }
        events.session(&ctx, custom_commands.as_deref(), timing);
        if custom_commands.is_none() {
            lock(&self.sessions).general.insert(channel.clone());
        }
        if let Some(commands) = &custom_commands {
            let now = crate::time::now_ms() as u64;
            let mut sessions = lock(&self.sessions);
            let pages = sessions.element_pages.entry(channel.clone()).or_default();
            pages.retain(|(page, at)| now.saturating_sub(*at) <= ELEMENT_PAGES_KEEP_MS && !vizrs::taken_pages(commands).contains(page));
            pages.extend(vizrs::taken_pages(commands).into_iter().map(|page| (page, now)));
        }

        let piece = format!("{channel}|{}|{}", ctx.start, ctx.seek);
        let carrier = lock(&self.sessions).carriers.insert(piece.clone());
        let spec = request.param("render").unwrap_or(&self.config.render_size).to_owned();
        let area = request.param("area").unwrap_or(&self.config.area).to_owned();
        let capture = match carrier
            .then(|| self.capture_for(&channel, &self.config.for_channel(&channel).output_url, ctx.width, ctx.height, fps, &spec, &area))
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
                    render: spec.clone(),
                    area: area.clone(),
                    zones: Mutex::new(None),
                    outside: AtomicUsize::new(0),
                    zones_rev: AtomicU64::new(0),
                    follows: AtomicBool::new(false),
                    widenings: AtomicUsize::new(0),
                    widened_at: Mutex::new(None),
                    drawn: Mutex::new(None),
                    streams: AtomicU64::new(0),
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
            let size = (ctx.width, ctx.height);
            stream::stream_watched(source, &mut out, &options, &control, &mut |picture| {
                self.draws(&script, picture, size);
                false
            });
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
    if area {
        if let Some(zones) = script.zones.lock().unwrap_or_else(|e| e.into_inner()).as_ref().filter(|z| !z.is_empty()) {
            return Layout::atlas(zones);
        }
    }
    match script.capture.as_ref().map(|c| c.areas()) {
        Some(areas) if area && !areas.is_empty() => Layout::atlas(areas),
        _ => Layout::whole(script.ctx.width, script.ctx.height),
    }
}

/// Query of `/ffmpeg/resume` for the overlay of `script`: a resumed overlay
/// must get the same zones as its running ffmpeg graph, not `auto`.
fn resume_query(script: &Script, placed: bool) -> String {
    let graphics = match script.zones.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        Some(zones) if placed && !zones.is_empty() => geometries(zones),
        _ => script.area.clone(),
    };
    format!(
        "channel={}&width={}&height={}&render={}&graphics={}",
        quote_plus(&script.channel),
        script.ctx.width,
        script.ctx.height,
        quote_plus(&script.render),
        quote_plus(&graphics)
    )
}

/// The parts of a frame layout, as the overlay reads them (`areas`).
fn areas_json(layout: &Layout) -> Value {
    let areas = layout.parts.iter().map(|(a, _)| {
        json::object([
            ("x", u64::from(a.x).into()),
            ("y", u64::from(a.y).into()),
            ("w", u64::from(a.w).into()),
            ("h", u64::from(a.h).into()),
        ])
    });
    Value::Array(areas.collect())
}

/// The zones a layout carries; empty = the whole channel.
fn layout_zones(layout: &Layout, width: u32, height: u32) -> Vec<Area> {
    let zones: Vec<Area> = layout.parts.iter().map(|(a, _)| *a).collect();
    if zones == [Area { x: 0, y: 0, w: width, h: height }] { Vec::new() } else { zones }
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
