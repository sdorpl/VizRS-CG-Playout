// Live API (DOKUMENTACJA §1 „Live API”): the live sources of the channels and
// the live programmes of ErsatzTV's schedule, for VizRS Live (the operator's
// panel, a separate application). The sidecar has no page of its own here.
//
//   GET    /api/live/state                channels: source, on air, hold, anytime,
//                                         now and next from the guide; events
//   PUT    /api/live/settings             {"library": id|null}
//   POST   /api/live/channels/<n>/hold    keep the studio off the channel
//   POST   /api/live/channels/<n>/release let it on again
//   PUT    /api/live/channels/<n>         {"anytime": bool|null}
//   POST   /api/live/channels/<n>/rebuild reset of the channel's playout in ErsatzTV
//   GET    /api/live/shows                live programmes with their state in ErsatzTV
//   POST   /api/live/shows                a new one: file, library scan, smart collection
//   PUT    /api/live/shows/<id>           change (the file and the item in ErsatzTV stay)
//   DELETE /api/live/shows/<id>           remove: file and collection
//   POST   /api/live/shows/scan           scan the library, make missing collections
//   GET    /api/live/graphics             graphics rules in force (src/graphics.rs)
//   PUT    /api/live/graphics             replace them: graphics.json, from the next clip
//
// The token (`panel_token`) comes in `X-Panel-Token` or `Authorization:
// Bearer`. Requests that change something are refused when a browser says they
// come from another site (`Sec-Fetch-Site`, `Origin`). Decisions on channels
// persist in `<etv_config>/vizrs-live/control.json`, operator actions go to
// `log.jsonl` next to it. Nothing here can stop the air: ErsatzTV calls run
// with timeouts and their failures are only shown.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use crate::config::{self, Config, LiveShow};
use crate::etv_api::{self, Collection, Etv, Programme};
use crate::http::{self, Request};
use crate::ingest::{ChannelControl, Ingest, constant_eq};
use crate::json::{self, Value};
use crate::live_shows;

/// A wrong token waits this long (slows guessing down).
const BAD_TOKEN_DELAY: Duration = Duration::from_secs(1);
/// ErsatzTV's channels, collections and guide are fetched this often.
const REFRESH: Duration = Duration::from_secs(30);
/// Items of the guide per channel in the state.
const GUIDE_ITEMS: usize = 8;
const MAX_LOG: u64 = 1024 * 1024;

/// What the panel keeps between restarts (`control.json`).
#[derive(Clone, Debug, Default, PartialEq)]
struct Settings {
    /// ErsatzTV's library of the live programmes (a Remote Streams library of `/config/vizrs-live`).
    library: Option<u64>,
    channels: BTreeMap<String, ChannelControl>,
}

impl Settings {
    fn to_json(&self) -> String {
        let channels = self.channels.iter().map(|(number, c)| {
            (number.clone(), json::object([("hold", c.hold.into()), ("anytime", c.anytime.map_or(Value::Null, Value::from))]))
        });
        let value =
            json::object([("library", self.library.map_or(Value::Null, Value::from)), ("channels", Value::Object(channels.collect()))]);
        format!("{value}\n")
    }

    fn from_json(text: &str) -> Result<Settings, String> {
        let value = json::parse(text).map_err(|e| e.to_string())?;
        let mut settings = Settings {
            library: value.get("library").and_then(Value::as_f64).filter(|n| *n >= 1.0 && n.fract() == 0.0).map(|n| n as u64),
            channels: BTreeMap::new(),
        };
        if let Some(Value::Object(channels)) = value.get("channels") {
            for (number, c) in channels {
                let control = ChannelControl {
                    hold: c.get("hold").and_then(Value::as_bool).unwrap_or(false),
                    anytime: c.get("anytime").and_then(Value::as_bool),
                };
                settings.channels.insert(number.clone(), control);
            }
        }
        Ok(settings)
    }
}

/// What the panel last got from ErsatzTV.
#[derive(Default)]
struct Cache {
    channels: Vec<(String, String)>,
    collections: Option<Vec<Collection>>,
    guide: Vec<Programme>,
    error: Option<String>,
    at: Option<Instant>,
}

/// One step of an operation on a programme, for the operator.
struct Step {
    name: &'static str,
    ok: bool,
    detail: String,
}

impl Step {
    fn of<T>(name: &'static str, result: io::Result<T>, ok_detail: impl FnOnce(T) -> String) -> Step {
        match result {
            Ok(value) => Step { name, ok: true, detail: ok_detail(value) },
            Err(e) => Step { name, ok: false, detail: e.to_string() },
        }
    }

    fn json(steps: &[Step]) -> Value {
        Value::Array(
            steps
                .iter()
                .map(|s| json::object([("step", s.name.into()), ("ok", s.ok.into()), ("detail", s.detail.as_str().into())]))
                .collect(),
        )
    }
}

pub struct Panel {
    token: String,
    /// Live ingest listeners (`VIZRS_INGEST`) and how publishers reach them.
    inputs: Vec<(String, Value)>,
    graphics: Arc<crate::graphics::Store>,
    etv_config: String,
    configured: Vec<LiveShow>,
    anytime: bool,
    etv: Etv,
    settings: Mutex<Settings>,
    cache: Mutex<Cache>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// The host part of an `Origin` (`http://host:port` → `host:port`).
fn origin_host(origin: &str) -> &str {
    origin.split_once("://").map_or(origin, |(_, rest)| rest).trim_end_matches('/')
}

/// A request that changes something does not come from a browser on another site.
fn same_site(request: &Request) -> bool {
    if request.header("sec-fetch-site").is_some_and(|site| site != "same-origin" && site != "none") {
        return false;
    }
    match (request.header("origin"), request.header("host")) {
        (Some("null"), _) => false,
        (Some(origin), Some(host)) => origin_host(origin).eq_ignore_ascii_case(host),
        (Some(_), None) => false,
        (None, _) => true,
    }
}

/// The operator's name from `X-Operator`: printable, at most 40 characters.
fn operator(request: &Request) -> String {
    let name: String = request.header("x-operator").unwrap_or("").chars().filter(|c| !c.is_control()).take(40).collect();
    let name = name.trim();
    if name.is_empty() { "operator".into() } else { name.to_owned() }
}

fn reply(stream: &mut TcpStream, status: u16, value: &Value) -> io::Result<()> {
    let headers = [("Content-Type", "application/json; charset=utf-8"), ("Cache-Control", "no-store")];
    http::respond_with(stream, status, &headers, value.to_string().as_bytes())
}

fn fail(stream: &mut TcpStream, status: u16, message: &str) -> io::Result<()> {
    reply(stream, status, &json::object([("error", message.into())]))
}

/// `"00:30:00"`, `"45:00"` or seconds, as in `live_shows`.
fn duration_of(value: Option<&Value>) -> Result<f64, String> {
    config::duration(value.ok_or("długość: brak")?, "długość").map_err(|_| "długość: od 00:00:01 do 23:59:59 (GG:MM:SS)".into())
}

fn text_of(body: &Value, name: &str) -> Result<String, String> {
    match body.get(name) {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(text)) => Ok(text.replace("\r\n", "\n")),
        Some(Value::Number(n)) if name == "channel" => Ok(n.to_string()),
        Some(_) => Err(format!("{name}: oczekiwano tekstu")),
    }
}

/// A programme from the JSON of the panel; `file` is the old one when editing.
fn show_of(body: &Value, file: &str) -> Result<LiveShow, String> {
    let show = LiveShow {
        channel: text_of(body, "channel")?.trim().to_owned(),
        title: text_of(body, "title")?.trim().to_owned(),
        seconds: duration_of(body.get("duration"))?,
        plot: text_of(body, "plot")?.trim().to_owned(),
        file: file.to_owned(),
        collection: None,
    };
    live_shows::validate(&show)?;
    Ok(show)
}

fn sort_channels(channels: &mut [String]) {
    channels.sort_by(|a, b| match (a.parse::<f64>(), b.parse::<f64>()) {
        (Ok(x), Ok(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
        _ => a.cmp(b),
    });
}

fn programme_json(p: &Programme, live: bool) -> Value {
    json::object([
        ("title", p.title.as_str().into()),
        ("start", (p.start_ms as f64).into()),
        ("stop", (p.stop_ms as f64).into()),
        ("live", live.into()),
    ])
}

impl Panel {
    pub fn new(config: &Config, ingest: &Ingest, graphics: Arc<crate::graphics::Store>) -> Arc<Panel> {
        let (listeners, errors) = crate::listener::parse_listeners(&config.ingest_listeners);
        for error in errors {
            crate::warn!("VIZRS_INGEST: {error}");
        }
        let inputs = listeners
            .iter()
            .map(|l| {
                (l.channel.clone(), crate::listener::publisher(l, &config.ingest_public, config.ingest_rtmp_port, config.ingest_srt_port))
            })
            .collect();
        let panel = Arc::new(Panel {
            token: config.panel_token.clone(),
            inputs,
            graphics,
            etv_config: config.etv_config.clone(),
            configured: config.live_shows.clone(),
            anytime: config.ingest_anytime,
            etv: Etv { url: config.etv_url.clone(), config: config.etv_config.clone() },
            settings: Mutex::new(Settings::default()),
            cache: Mutex::new(Cache::default()),
        });
        if let Some(path) = panel.control_path() {
            match fs::read_to_string(&path) {
                Ok(text) => match Settings::from_json(&text) {
                    Ok(settings) => *lock(&panel.settings) = settings,
                    Err(e) => crate::warn!("{}: {e}; panel settings start empty", path.display()),
                },
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => crate::warn!("{}: {e}", path.display()),
            }
        }
        // A hold survives a restart: the studio must not come back on by itself.
        for (number, control) in &lock(&panel.settings).channels {
            ingest.set_control(number, control.clone());
        }
        if panel.enabled() {
            crate::info!("live API on /api/live");
            if panel.etv.configured() {
                let refresher = Arc::clone(&panel);
                let _ = thread::Builder::new().name("panel-etv".into()).spawn(move || {
                    loop {
                        refresher.refresh();
                        thread::sleep(REFRESH);
                    }
                });
            }
        }
        panel
    }

    pub fn enabled(&self) -> bool {
        !self.token.is_empty()
    }

    /// The live API's paths.
    pub fn owns(path: &str) -> bool {
        path == "/api/live" || path.starts_with("/api/live/")
    }

    fn control_path(&self) -> Option<PathBuf> {
        (!self.etv_config.is_empty()).then(|| live_shows::folder(&self.etv_config).join("control.json"))
    }

    fn save_settings(&self, settings: &Settings) -> io::Result<()> {
        let Some(path) = self.control_path() else { return Ok(()) };
        fs::create_dir_all(live_shows::folder(&self.etv_config))?;
        crate::sync::write_atomic(&path, &settings.to_json())
    }

    /// An operator action: the events of the panel and `log.jsonl`.
    fn record(&self, ingest: &Ingest, operator: &str, channel: &str, action: &str) {
        crate::info!("live API: {operator}: {action}{}", if channel.is_empty() { String::new() } else { format!(" (channel {channel})") });
        ingest.event(channel, format!("{operator}: {action}"));
        if self.etv_config.is_empty() {
            return;
        }
        let path = live_shows::folder(&self.etv_config).join("log.jsonl");
        if fs::metadata(&path).is_ok_and(|m| m.len() > MAX_LOG) {
            let _ = fs::rename(&path, path.with_extension("jsonl.1"));
        }
        let line = json::object([
            ("at", crate::time::iso_time(crate::time::now_ms()).into()),
            ("operator", operator.into()),
            ("channel", channel.into()),
            ("action", action.into()),
        ]);
        let written = fs::create_dir_all(live_shows::folder(&self.etv_config))
            .and_then(|()| OpenOptions::new().create(true).append(true).open(&path))
            .and_then(|mut file| writeln!(file, "{line}"));
        if let Err(e) = written {
            crate::warn!("live API log: {e}");
        }
    }

    /// Channels, collections and the guide of ErsatzTV, now.
    fn refresh(&self) {
        if !self.etv.configured() {
            return;
        }
        let channels = self.etv.channels();
        let collections = self.etv.collections();
        let guide = self.etv.guide();
        let mut cache = lock(&self.cache);
        cache.at = Some(Instant::now());
        cache.error = [channels.as_ref().err(), collections.as_ref().err(), guide.as_ref().err()]
            .into_iter()
            .flatten()
            .next()
            .map(ToString::to_string);
        if let Ok(channels) = channels {
            cache.channels = channels;
        }
        cache.collections = collections.ok();
        if let Ok(guide) = guide {
            cache.guide = guide;
        }
    }

    fn shows(&self) -> io::Result<Vec<LiveShow>> {
        if self.etv_config.is_empty() {
            return Ok(self.configured.clone());
        }
        live_shows::load(&self.etv_config, &self.configured)
    }

    pub fn handle(&self, request: &Request, mut stream: TcpStream, ingest: &Ingest) -> io::Result<()> {
        if !self.enabled() {
            return http::respond(&mut stream, 404, None, b"");
        }
        let path = request.path.trim_end_matches('/');
        let presented = request
            .header("x-panel-token")
            .map(str::to_owned)
            .or_else(|| request.header("authorization").and_then(|v| v.strip_prefix("Bearer ")).map(str::to_owned));
        match presented {
            Some(token) if constant_eq(token.trim().as_bytes(), self.token.as_bytes()) => {}
            Some(_) => {
                thread::sleep(BAD_TOKEN_DELAY);
                return fail(&mut stream, 401, "zły token panelu");
            }
            None => return fail(&mut stream, 401, "brak tokenu panelu"),
        }
        if request.method != "GET" && !same_site(request) {
            return fail(&mut stream, 403, "żądanie z obcej strony");
        }
        let body = if request.body.is_empty() {
            Value::Object(Vec::new())
        } else {
            match json::parse(&String::from_utf8_lossy(&request.body)) {
                Ok(value @ Value::Object(_)) => value,
                _ => return fail(&mut stream, 400, "oczekiwano obiektu JSON"),
            }
        };
        let who = operator(request);
        let parts: Vec<&str> = path.trim_start_matches("/api/live").split('/').filter(|p| !p.is_empty()).collect();
        match (request.method.as_str(), parts.as_slice()) {
            ("GET", ["state"]) => reply(&mut stream, 200, &self.state(ingest)),
            ("PUT", ["settings"]) => self.put_settings(&body, &mut stream, ingest, &who),
            ("POST", ["channels", channel, action @ ("hold" | "release")]) => {
                self.hold(channel, *action == "hold", &mut stream, ingest, &who)
            }
            ("PUT", ["channels", channel]) => self.put_channel(channel, &body, &mut stream, ingest, &who),
            ("POST", ["channels", channel, "rebuild"]) => {
                let result = self.etv.reset_playout(channel);
                if result.is_ok() {
                    self.record(ingest, &who, channel, "przebudowa ramówki (reset playoutu ErsatzTV)");
                }
                match result {
                    Ok(()) => reply(&mut stream, 200, &json::object([("ok", true.into())])),
                    Err(e) => fail(&mut stream, 502, &e.to_string()),
                }
            }
            ("GET", ["graphics"]) => reply(&mut stream, 200, &self.graphics.to_json()),
            ("PUT", ["graphics"]) => self.put_graphics(&body, &mut stream, ingest, &who),
            ("GET", ["shows"]) => reply(&mut stream, 200, &self.shows_state()),
            ("POST", ["shows", "scan"]) => self.scan(&mut stream, ingest, &who),
            ("POST", ["shows"]) => self.create(&body, &mut stream, ingest, &who),
            ("PUT", ["shows", id]) => self.update(id, &body, &mut stream, ingest, &who),
            ("DELETE", ["shows", id]) => self.delete(id, &mut stream, ingest, &who),
            _ => fail(&mut stream, 404, "nie ma takiego adresu API"),
        }
    }

    /// `PUT /api/live/graphics`: the whole rules document, in force from the next clip.
    fn put_graphics(&self, body: &Value, stream: &mut TcpStream, ingest: &Ingest, who: &str) -> io::Result<()> {
        use crate::graphics::Replace;
        match self.graphics.replace(body) {
            Ok(()) => {
                self.record(ingest, who, "", "reguły grafiki zmienione (od następnej pozycji)");
                reply(stream, 200, &self.graphics.to_json())
            }
            Err(Replace::Invalid(e)) => fail(stream, 400, &format!("reguły grafiki: {e}")),
            Err(Replace::NoStore) => fail(stream, 409, "brak etv_config – reguł nie ma gdzie zapisać"),
            Err(Replace::Io(e)) => fail(stream, 500, &format!("zapis graphics.json: {e}")),
        }
    }

    fn etv_json(&self, cache: &Cache) -> Value {
        json::object([
            ("configured", self.etv.configured().into()),
            ("ok", (cache.at.is_some() && cache.error.is_none()).into()),
            ("error", cache.error.as_deref().map_or(Value::Null, Value::from)),
            ("checked", cache.at.map_or(Value::Null, |t| (t.elapsed().as_secs_f64().floor()).into())),
        ])
    }

    fn settings_json(&self) -> Value {
        let settings = lock(&self.settings);
        json::object([("library", settings.library.map_or(Value::Null, Value::from)), ("store", (!self.etv_config.is_empty()).into())])
    }

    /// `/api/live/state`.
    fn state(&self, ingest: &Ingest) -> Value {
        let now = crate::time::now_ms();
        let shows = self.shows().unwrap_or_default();
        let is_live_show = |channel: &str, title: &str| shows.iter().any(|s| s.channel == channel && s.title == title);
        let cache = lock(&self.cache);
        let names: BTreeMap<&str, &str> = cache.channels.iter().map(|(n, name)| (n.as_str(), name.as_str())).collect();
        let mut numbers: Vec<String> = cache.channels.iter().map(|(n, _)| n.clone()).collect();
        let settings = lock(&self.settings).clone();
        let extra = ingest
            .channels()
            .into_iter()
            .chain(shows.iter().map(|s| s.channel.clone()))
            .chain(settings.channels.keys().cloned())
            .chain(self.inputs.iter().map(|(c, _)| c.clone()));
        for number in extra {
            if !numbers.contains(&number) {
                numbers.push(number);
            }
        }
        sort_channels(&mut numbers);

        let mut guide_by_channel = Vec::new();
        let channels: Vec<Value> = numbers
            .iter()
            .map(|number| {
                let items: Vec<&Programme> = cache.guide.iter().filter(|p| p.channel == *number && p.stop_ms > now).collect();
                let current = items.iter().find(|p| p.start_ms <= now);
                let next_live = items.iter().find(|p| is_live_show(number, &p.title));
                let control = ingest.control(number);
                let anytime = ingest.anytime_of(number);
                let source = ingest.source(number);
                let signal = source.as_ref().and_then(|s| s.get("live")).and_then(Value::as_bool).unwrap_or(false);
                let in_slot = current.is_some_and(|p| is_live_show(number, &p.title));
                let guide: Vec<Value> = items.iter().take(GUIDE_ITEMS).map(|p| programme_json(p, is_live_show(number, &p.title))).collect();
                guide_by_channel.push((number.clone(), Value::Array(guide)));
                json::object([
                    ("number", number.as_str().into()),
                    ("name", names.get(number.as_str()).map_or(Value::Null, |n| (*n).into())),
                    ("source", source.unwrap_or(Value::Null)),
                    ("on_air", (signal && !control.hold && (anytime || in_slot)).into()),
                    ("hold", control.hold.into()),
                    ("anytime", anytime.into()),
                    ("anytime_own", control.anytime.map_or(Value::Null, Value::from)),
                    ("now", current.map_or(Value::Null, |p| programme_json(p, in_slot))),
                    ("next_live", next_live.map_or(Value::Null, |p| programme_json(p, true))),
                    ("inputs", Value::Array(self.inputs.iter().filter(|(c, _)| c == number).map(|(_, v)| v.clone()).collect())),
                ])
            })
            .collect();
        let events = ingest
            .events()
            .into_iter()
            .take(40)
            .map(|(at, channel, text)| json::object([("at", (at as f64).into()), ("channel", channel.into()), ("text", text.into())]));
        json::object([
            ("version", env!("CARGO_PKG_VERSION").into()),
            ("now", (now as f64).into()),
            ("ingest", ingest.enabled().into()),
            ("anytime", self.anytime.into()),
            ("etv", self.etv_json(&cache)),
            ("settings", self.settings_json()),
            ("channels", Value::Array(channels)),
            ("guide", Value::Object(guide_by_channel)),
            ("events", Value::Array(events.collect())),
        ])
    }

    /// `/api/live/shows`.
    fn shows_state(&self) -> Value {
        let now = crate::time::now_ms();
        let (shows, error) = match self.shows() {
            Ok(shows) => (shows, None),
            Err(e) => (Vec::new(), Some(e.to_string())),
        };
        let folder = live_shows::folder(&self.etv_config);
        let cache = lock(&self.cache);
        let items = shows.iter().map(|show| {
            let collection = match (&cache.collections, show.collection) {
                (_, None) => Value::Bool(false),
                (Some(list), Some(id)) => list.iter().any(|c| c.id == id).into(),
                (None, Some(_)) => Value::Null,
            };
            let airings: Vec<&Programme> =
                cache.guide.iter().filter(|p| p.channel == show.channel && p.title == show.title && p.stop_ms > now).collect();
            json::object([
                ("id", live_shows::id(show).into()),
                ("channel", show.channel.as_str().into()),
                ("title", show.title.as_str().into()),
                ("duration", live_shows::clock(show.seconds).into()),
                ("plot", show.plot.as_str().into()),
                ("file", format!("vizrs-live/{}", live_shows::file_name(show)).into()),
                ("file_ok", (self.etv_config.is_empty() || folder.join(live_shows::file_name(show)).exists()).into()),
                ("collection", collection),
                ("collection_id", show.collection.map_or(Value::Null, Value::from)),
                ("collection_name", etv_api::collection_name(&show.title).into()),
                ("next", airings.first().map_or(Value::Null, |p| programme_json(p, true))),
                ("airings", (airings.len() as u64).into()),
            ])
        });
        let channels = cache.channels.iter().map(|(n, name)| json::object([("number", n.as_str().into()), ("name", name.as_str().into())]));
        json::object([
            ("shows", Value::Array(items.collect())),
            ("error", error.map_or(Value::Null, Value::from)),
            ("guide_known", (!cache.guide.is_empty()).into()),
            ("channels", Value::Array(channels.collect())),
            ("etv", self.etv_json(&cache)),
            ("settings", self.settings_json()),
            ("max", (live_shows::MAX_SHOWS as u64).into()),
        ])
    }

    fn put_settings(&self, body: &Value, stream: &mut TcpStream, ingest: &Ingest, who: &str) -> io::Result<()> {
        let mut settings = lock(&self.settings).clone();
        match body.get("library") {
            None => {}
            Some(Value::Null) => settings.library = None,
            Some(Value::Number(n)) if *n >= 1.0 && *n < 1e9 && n.fract() == 0.0 => settings.library = Some(*n as u64),
            Some(_) => return fail(stream, 400, "biblioteka: numer ID biblioteki ErsatzTV"),
        }
        if let Err(e) = self.save_settings(&settings) {
            return fail(stream, 500, &format!("zapis ustawień: {e}"));
        }
        *lock(&self.settings) = settings.clone();
        let library = settings.library.map_or("brak".into(), |l| l.to_string());
        self.record(ingest, who, "", &format!("ustawienia: biblioteka {library}"));
        reply(stream, 200, &self.settings_json())
    }

    fn set_channel(&self, channel: &str, ingest: &Ingest, change: impl FnOnce(&mut ChannelControl)) -> io::Result<ChannelControl> {
        let mut settings = lock(&self.settings);
        let mut control = settings.channels.get(channel).cloned().unwrap_or_default();
        change(&mut control);
        let mut next = settings.clone();
        if control == ChannelControl::default() {
            next.channels.remove(channel);
        } else {
            next.channels.insert(channel.to_owned(), control.clone());
        }
        // The decision takes effect even when the disk fails; it is only not kept.
        ingest.set_control(channel, control.clone());
        *settings = next.clone();
        drop(settings);
        self.save_settings(&next)?;
        Ok(control)
    }

    fn valid_channel(channel: &str) -> bool {
        !channel.is_empty() && channel.len() <= 20 && channel.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
    }

    fn hold(&self, channel: &str, hold: bool, stream: &mut TcpStream, ingest: &Ingest, who: &str) -> io::Result<()> {
        if !Self::valid_channel(channel) {
            return fail(stream, 400, "kanał?");
        }
        let saved = self.set_channel(channel, ingest, |c| c.hold = hold);
        self.record(ingest, who, channel, if hold { "studio wstrzymane (ZDEJMIJ)" } else { "studio wpuszczone (WPUŚĆ)" });
        match saved {
            Ok(_) => reply(stream, 200, &json::object([("hold", hold.into())])),
            Err(e) => fail(stream, 500, &format!("decyzja działa, ale nie zapisała się na dysku: {e}")),
        }
    }

    fn put_channel(&self, channel: &str, body: &Value, stream: &mut TcpStream, ingest: &Ingest, who: &str) -> io::Result<()> {
        if !Self::valid_channel(channel) {
            return fail(stream, 400, "kanał?");
        }
        let anytime = match body.get("anytime") {
            Some(Value::Bool(b)) => Some(*b),
            Some(Value::Null) => None,
            _ => return fail(stream, 400, "anytime: true, false albo null"),
        };
        let saved = self.set_channel(channel, ingest, |c| c.anytime = anytime);
        let text = match anytime {
            Some(true) => "studio także poza audycjami",
            Some(false) => "studio tylko w audycjach na żywo",
            None => "poza audycjami: jak w konfiguracji",
        };
        self.record(ingest, who, channel, text);
        match saved {
            Ok(_) => reply(stream, 200, &json::object([("anytime", ingest.anytime_of(channel).into())])),
            Err(e) => fail(stream, 500, &format!("decyzja działa, ale nie zapisała się na dysku: {e}")),
        }
    }

    /// Writes the list and its definitions.
    fn store(&self, shows: &[LiveShow]) -> io::Result<()> {
        live_shows::save(&self.etv_config, shows)?;
        live_shows::write(shows, &live_shows::folder(&self.etv_config), &crate::etv_sync::sidecar_origin(&self.etv_config)).map(drop)
    }

    /// The library scan, as a step.
    fn scan_step(&self) -> Step {
        match lock(&self.settings).library {
            None => Step { name: "scan", ok: false, detail: "ustaw ID biblioteki „Na żywo” w panelu".into() },
            Some(library) => Step::of("scan", self.etv.scan(library), |()| format!("skan biblioteki {library} zlecony")),
        }
    }

    /// Gives the programme its smart collection: the one it has (name and
    /// query brought in line), one of the same name, or a new one.
    fn ensure_collection(&self, show: &mut LiveShow) -> Step {
        let Some(library) = lock(&self.settings).library else {
            return Step { name: "collection", ok: false, detail: "bez ID biblioteki nie ma kolekcji".into() };
        };
        let name = etv_api::collection_name(&show.title);
        let query = etv_api::show_query(library, &show.title);
        let result = (|| {
            let existing = self.etv.collections()?;
            let own =
                show.collection.and_then(|id| existing.iter().find(|c| c.id == id)).or_else(|| existing.iter().find(|c| c.name == name));
            match own {
                Some(c) if c.name == name && c.query == query => Ok(c.id),
                Some(c) => self.etv.update_collection(c.id, &name, &query).map(|()| c.id),
                None => self.etv.create_collection(&name, &query),
            }
        })();
        if let Ok(id) = result {
            show.collection = Some(id);
        }
        Step::of("collection", result, |id| format!("{name} (ID {id})"))
    }

    fn done(&self, stream: &mut TcpStream, show: Option<&LiveShow>, steps: &[Step]) -> io::Result<()> {
        // The table shows what ErsatzTV has now.
        self.refresh();
        let show = show.map_or(Value::Null, |s| {
            json::object([
                ("id", live_shows::id(s).into()),
                ("channel", s.channel.as_str().into()),
                ("title", s.title.as_str().into()),
                ("collection_name", etv_api::collection_name(&s.title).into()),
            ])
        });
        reply(stream, 200, &json::object([("show", show), ("steps", Step::json(steps))]))
    }

    fn needs_store(&self, stream: &mut TcpStream) -> Option<io::Result<()>> {
        self.etv_config.is_empty().then(|| fail(stream, 409, "sidecar bez etv_config: audycji nie ma gdzie zapisać"))
    }

    fn create(&self, body: &Value, stream: &mut TcpStream, ingest: &Ingest, who: &str) -> io::Result<()> {
        if let Some(done) = self.needs_store(stream) {
            return done;
        }
        let mut show = match show_of(body, "") {
            Ok(show) => show,
            Err(e) => return fail(stream, 400, &e),
        };
        show.file = live_shows::id(&show);
        let mut steps = Vec::new();
        {
            let _store = live_shows::lock();
            let mut shows = match self.shows() {
                Ok(shows) => shows,
                Err(e) => return fail(stream, 500, &e.to_string()),
            };
            if shows.len() >= live_shows::MAX_SHOWS {
                return fail(stream, 409, &format!("najwyżej {} audycji", live_shows::MAX_SHOWS));
            }
            if shows.iter().any(|s| live_shows::id(s) == show.file) {
                return fail(stream, 409, "taka audycja na tym kanale już jest (ten sam plik)");
            }
            shows.push(show.clone());
            if let Err(e) = self.store(&shows) {
                return fail(stream, 500, &format!("zapis audycji: {e}"));
            }
        }
        steps.push(Step { name: "file", ok: true, detail: format!("vizrs-live/{}", live_shows::file_name(&show)) });
        self.record(ingest, who, &show.channel, &format!("dodana audycja „{}”", show.title));
        steps.push(self.scan_step());
        steps.push(self.ensure_collection(&mut show));
        self.remember_collection(&show);
        self.done(stream, Some(&show), &steps)
    }

    /// Keeps the collection id of a programme in `shows.json`.
    fn remember_collection(&self, show: &LiveShow) {
        let _store = live_shows::lock();
        let Ok(mut shows) = self.shows() else { return };
        let id = live_shows::id(show);
        let mut changed = false;
        for s in shows.iter_mut().filter(|s| live_shows::id(s) == id && s.collection != show.collection) {
            s.collection = show.collection;
            changed = true;
        }
        if changed {
            if let Err(e) = live_shows::save(&self.etv_config, &shows) {
                crate::warn!("live programmes: {e}");
            }
        }
    }

    fn update(&self, id: &str, body: &Value, stream: &mut TcpStream, ingest: &Ingest, who: &str) -> io::Result<()> {
        if let Some(done) = self.needs_store(stream) {
            return done;
        }
        let mut steps = Vec::new();
        let show = {
            let _store = live_shows::lock();
            let mut shows = match self.shows() {
                Ok(shows) => shows,
                Err(e) => return fail(stream, 500, &e.to_string()),
            };
            let Some(old) = shows.iter_mut().find(|s| live_shows::id(s) == id) else {
                return fail(stream, 404, "nie ma takiej audycji");
            };
            // The file stays: ErsatzTV keeps the item and the schedule items that use it.
            let mut show = match show_of(body, &old.file) {
                Ok(show) => show,
                Err(e) => return fail(stream, 400, &e),
            };
            show.collection = old.collection;
            *old = show.clone();
            if let Err(e) = self.store(&shows) {
                return fail(stream, 500, &format!("zapis audycji: {e}"));
            }
            show
        };
        steps.push(Step { name: "file", ok: true, detail: format!("vizrs-live/{}", live_shows::file_name(&show)) });
        self.record(ingest, who, &show.channel, &format!("zmieniona audycja „{}”", show.title));
        steps.push(self.scan_step());
        let mut show = show;
        steps.push(self.ensure_collection(&mut show));
        self.remember_collection(&show);
        self.done(stream, Some(&show), &steps)
    }

    fn delete(&self, id: &str, stream: &mut TcpStream, ingest: &Ingest, who: &str) -> io::Result<()> {
        if let Some(done) = self.needs_store(stream) {
            return done;
        }
        let mut steps = Vec::new();
        let show = {
            let _store = live_shows::lock();
            let mut shows = match self.shows() {
                Ok(shows) => shows,
                Err(e) => return fail(stream, 500, &e.to_string()),
            };
            let Some(at) = shows.iter().position(|s| live_shows::id(s) == id) else {
                return fail(stream, 404, "nie ma takiej audycji");
            };
            let show = shows.remove(at);
            if let Err(e) = live_shows::save(&self.etv_config, &shows) {
                return fail(stream, 500, &format!("zapis audycji: {e}"));
            }
            let path = live_shows::folder(&self.etv_config).join(live_shows::file_name(&show));
            steps.push(Step::of(
                "file",
                fs::remove_file(&path).or_else(|e| if e.kind() == io::ErrorKind::NotFound { Ok(()) } else { Err(e) }),
                |()| format!("vizrs-live/{} usunięty", live_shows::file_name(&show)),
            ));
            show
        };
        self.record(ingest, who, &show.channel, &format!("usunięta audycja „{}”", show.title));
        steps.push(self.scan_step());
        if let Some(collection) = show.collection {
            steps.push(Step::of("collection", self.etv.delete_collection(collection), |()| format!("kolekcja {collection} usunięta")));
        }
        self.done(stream, None, &steps)
    }

    fn scan(&self, stream: &mut TcpStream, ingest: &Ingest, who: &str) -> io::Result<()> {
        let mut steps = vec![self.scan_step()];
        let shows = self.shows().unwrap_or_default();
        if lock(&self.settings).library.is_some() {
            for mut show in shows {
                let step = self.ensure_collection(&mut show);
                if show.collection.is_some() {
                    self.remember_collection(&show);
                }
                steps.push(step);
            }
        }
        self.record(ingest, who, "", "odświeżenie audycji w ErsatzTV (skan, kolekcje)");
        self.done(stream, None, &steps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(headers: &[(&str, &str)]) -> Request {
        Request {
            method: "POST".into(),
            path: "/api/live/shows".into(),
            query: Vec::new(),
            headers: headers.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(),
            body: Vec::new(),
            http10: false,
        }
    }

    #[test]
    fn requests_from_other_sites_are_refused() {
        assert!(same_site(&request(&[("Host", "cg:7700")])));
        assert!(same_site(&request(&[("Host", "cg:7700"), ("Origin", "http://cg:7700"), ("Sec-Fetch-Site", "same-origin")])));
        assert!(!same_site(&request(&[("Host", "cg:7700"), ("Origin", "http://evil.example")])));
        assert!(!same_site(&request(&[("Host", "cg:7700"), ("Sec-Fetch-Site", "cross-site")])));
        assert!(!same_site(&request(&[("Host", "cg:7700"), ("Origin", "null")])));
    }

    #[test]
    fn operators() {
        let r = request(&[("X-Operator", " MCR 1\u{7} ")]);
        assert_eq!(operator(&r), "MCR 1");
        assert_eq!(operator(&request(&[])), "operator");
    }

    #[test]
    fn settings_round_trip() {
        let mut settings = Settings { library: Some(7), channels: BTreeMap::new() };
        settings.channels.insert("4".into(), ChannelControl { hold: true, anytime: Some(false) });
        assert_eq!(Settings::from_json(&settings.to_json()).unwrap(), settings);
        assert_eq!(Settings::from_json("{}").unwrap(), Settings::default());
    }

    #[test]
    fn shows_from_the_panel() {
        let body = json::parse(r#"{"channel":4,"title":" Rozmowa dnia ","duration":"00:45:00","plot":"Gość\r\nw studiu"}"#).unwrap();
        let show = show_of(&body, "").unwrap();
        assert_eq!(
            (show.channel.as_str(), show.title.as_str(), show.seconds, show.plot.as_str()),
            ("4", "Rozmowa dnia", 2700.0, "Gość\nw studiu")
        );
        assert!(show_of(&json::parse(r#"{"channel":"4","title":"X","duration":"25:00:00"}"#).unwrap(), "").is_err());
        assert!(show_of(&json::parse(r#"{"channel":"4","title":["X"],"duration":60}"#).unwrap(), "").is_err());
        let mut channels = vec!["10".to_owned(), "2".into(), "1.5".into(), "a".into()];
        sort_channels(&mut channels);
        assert_eq!(channels, ["1.5", "2", "10", "a"]);
    }
}
