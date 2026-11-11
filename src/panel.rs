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
//   POST   /api/live/graphics/off · /on   all graphics off the air at once · back (control.json)
//   PUT    /api/live/graphics             replace them: graphics.json, from the next clip
//   GET    /api/live/inputs               studio inputs (listeners) and the ports they may use
//   PUT    /api/live/inputs               {"listeners":[{"channel","url"}]}: inputs.json; the
//                                         listener service takes them over within seconds
//   POST   /api/live/channels/<n>/take    {"page": "101"} one page on the channel's antenna (K13)
//   POST   /api/live/channels/<n>/continue {"layer": …} · /out {"layer": …} one layer
//   GET    /api/live/asrun?channel=&day=  the as-run log of a day (K14, src/asrun.rs)
//
// `features` of `/api/live/state` says what this build can do, so an older
// sidecar never breaks a newer VizRS Live: it shows only what is listed.
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
    /// All graphics taken off the air (ZDEJMIJ GRAFIKĘ in VizRS Live), on every channel.
    graphics_off: bool,
    /// When they were taken off (epoch ms), for the alarms of VizRS Live (K17).
    graphics_off_since: Option<i64>,
}

impl Settings {
    fn to_json(&self) -> String {
        let channels = self.channels.iter().map(|(number, c)| {
            (number.clone(), json::object([("hold", c.hold.into()), ("anytime", c.anytime.map_or(Value::Null, Value::from))]))
        });
        let value = json::object([
            ("library", self.library.map_or(Value::Null, Value::from)),
            ("channels", Value::Object(channels.collect())),
            ("graphics_off", self.graphics_off.into()),
            ("graphics_off_since", self.graphics_off_since.map_or(Value::Null, |t| (t as f64).into())),
        ]);
        format!("{value}\n")
    }

    fn from_json(text: &str) -> Result<Settings, String> {
        let value = json::parse(text).map_err(|e| e.to_string())?;
        let mut settings = Settings {
            library: value.get("library").and_then(Value::as_f64).filter(|n| *n >= 1.0 && n.fract() == 0.0).map(|n| n as u64),
            channels: BTreeMap::new(),
            graphics_off: value.get("graphics_off").and_then(Value::as_bool).unwrap_or(false),
            graphics_off_since: value.get("graphics_off_since").and_then(Value::as_f64).map(|t| t as i64),
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

/// `inputs.json`: `{"listeners": "1=rtmp://…; 2=srt://…"}` (`VIZRS_INGEST`'s format).
fn inputs_to_json(list: &[crate::listener::Listener]) -> String {
    let text = list.iter().map(|l| format!("{}={}", l.channel, l.url)).collect::<Vec<_>>().join("; ");
    format!("{}\n", json::object([("listeners", text.into())]))
}

fn inputs_from_json(text: &str) -> Result<Vec<crate::listener::Listener>, String> {
    let value = json::parse(text).map_err(|e| e.to_string())?;
    let listeners = value.get("listeners").and_then(Value::as_str).ok_or("listeners?")?;
    let (list, errors) = crate::listener::parse_listeners(listeners);
    errors.first().map_or(Ok(list), |e| Err(e.clone()))
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
    /// The sidecar's configuration: VizRS of each channel (layers, TAKE/OUT) and the alert webhook.
    config: Config,
    /// Live ingest listeners in force: `inputs.json` (VizRS Live) or `VIZRS_INGEST`, and whether from the file.
    listeners: Mutex<(Vec<crate::listener::Listener>, bool)>,
    /// How publishers reach the listeners: host, host ports of the first port of each range.
    public: (String, Option<u16>, Option<u16>),
    graphics: Arc<crate::graphics::Store>,
    etv_config: String,
    configured: Vec<LiveShow>,
    anytime: bool,
    etv: Etv,
    settings: Mutex<Settings>,
    cache: Mutex<Cache>,
    /// The layers on air of each channel (K12), refreshed in the background.
    layers: Mutex<Layers>,
}

/// What VizRS has on the antenna of each channel, and since when.
#[derive(Default)]
struct Layers {
    by_channel: BTreeMap<String, Vec<Value>>,
    /// `channel|layer` → the page there and when it was first seen (epoch ms).
    since: BTreeMap<String, (String, i64)>,
    /// `channel|layer` of pages a person took in from VizRS Live (K13); until the
    /// next read shows the layer, the key is `channel|page`.
    manual: BTreeMap<String, String>,
    /// The latest successful read of each channel: `(layer, page)` and when (epoch ms).
    /// The as-run log compares two reads; after a failed one it starts over.
    seen: BTreeMap<String, (Vec<(String, String)>, i64)>,
}

/// Who changed the antenna, in the as-run log, when neither the sidecar nor VizRS Live did.
const FOREIGN_OPERATOR: &str = "VizRS (poza panelem)";
/// A command counts for the read after it even when it was sent this long before the read before.
const SENT_SLACK_MS: i64 = 5_000;

/// How often the layers on air are read from VizRS (`STATE` per channel).
const LAYERS_EVERY: Duration = Duration::from_secs(2);

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
        let panel = Arc::new(Panel {
            token: config.panel_token.clone(),
            listeners: Mutex::new((listeners, false)),
            public: (config.ingest_public.clone(), config.ingest_rtmp_port, config.ingest_srt_port),
            graphics,
            etv_config: config.etv_config.clone(),
            configured: config.live_shows.clone(),
            anytime: config.ingest_anytime,
            etv: Etv { url: config.etv_url.clone(), config: config.etv_config.clone() },
            settings: Mutex::new(Settings::default()),
            cache: Mutex::new(Cache::default()),
            layers: Mutex::new(Layers::default()),
            config: config.clone(),
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
        if let Some(path) = panel.inputs_path() {
            match fs::read_to_string(&path).map(|text| inputs_from_json(&text)) {
                Ok(Ok(list)) => *lock(&panel.listeners) = (list, true),
                Ok(Err(e)) => crate::warn!("{}: {e}; inputs from VIZRS_INGEST", path.display()),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => crate::warn!("{}: {e}", path.display()),
            }
        }
        // Graphics taken off stay off after a restart, as a hold does.
        if lock(&panel.settings).graphics_off {
            crate::vizrs::graphics_switch(true, false);
        }
        // A hold survives a restart: the studio must not come back on by itself.
        for (number, control) in &lock(&panel.settings).channels {
            ingest.set_control(number, control.clone());
        }
        if panel.enabled() {
            crate::info!("live API on /api/live");
            if !panel.config.vizrs.address.is_empty() {
                let watcher = Arc::clone(&panel);
                let _ = thread::Builder::new().name("panel-layers".into()).spawn(move || {
                    loop {
                        watcher.refresh_layers();
                        thread::sleep(LAYERS_EVERY);
                    }
                });
            }
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

    fn inputs_path(&self) -> Option<PathBuf> {
        (!self.etv_config.is_empty()).then(|| live_shows::folder(&self.etv_config).join("inputs.json"))
    }

    /// Inputs of a channel as publishers see them.
    fn inputs_of(&self, number: &str) -> Vec<Value> {
        let (host, rtmp, srt) = &self.public;
        let listeners = lock(&self.listeners);
        listeners.0.iter().filter(|l| l.channel == number).map(|l| crate::listener::publisher(l, host, *rtmp, *srt)).collect()
    }

    /// `GET /ingest/listeners` (the listener service, ingest token): `VIZRS_INGEST`'s format.
    pub fn listeners_request(&self, request: &Request, mut stream: TcpStream, ingest: &Ingest) -> io::Result<()> {
        if !ingest.authorized(request) {
            return http::respond(&mut stream, 401, None, b"");
        }
        let text = lock(&self.listeners).0.iter().map(|l| format!("{}={}", l.channel, l.url)).collect::<Vec<_>>().join("; ");
        http::respond(&mut stream, 200, Some("text/plain; charset=utf-8"), text.as_bytes())
    }

    fn inputs_json(&self, ingest: &Ingest) -> Value {
        use crate::listener::{RTMP_PORTS, SRT_PORTS};
        let (listeners, from_file) = lock(&self.listeners).clone();
        let range = |r: std::ops::RangeInclusive<u16>| Value::Array(vec![f64::from(*r.start()).into(), f64::from(*r.end()).into()]);
        let items = listeners.iter().map(|l| json::object([("channel", l.channel.as_str().into()), ("url", l.url.as_str().into())]));
        json::object([
            ("listeners", Value::Array(items.collect())),
            ("from", if from_file { "panel" } else { "env" }.into()),
            ("store", (!self.etv_config.is_empty()).into()),
            ("ingest", ingest.enabled().into()),
            ("ports", json::object([("rtmp", range(RTMP_PORTS)), ("srt", range(SRT_PORTS))])),
        ])
    }

    /// `PUT /api/live/inputs`: the whole list of inputs.
    fn put_inputs(&self, body: &Value, stream: &mut TcpStream, ingest: &Ingest, who: &str) -> io::Result<()> {
        let Some(Value::Array(items)) = body.get("listeners") else {
            return fail(stream, 400, "listeners: lista {channel, url}");
        };
        let mut entries = Vec::new();
        for item in items {
            let channel = item.get("channel").and_then(|v| v.as_str().map(str::to_owned).or_else(|| v.as_f64().map(|n| n.to_string())));
            let url = item.get("url").and_then(Value::as_str);
            match (channel, url) {
                (Some(c), Some(u)) if Self::valid_channel(c.trim()) => entries.push(format!("{}={}", c.trim(), u.trim())),
                _ => return fail(stream, 400, "wejście: kanał (np. 2) i adres rtmp:// albo srt://"),
            }
        }
        let (list, errors) = crate::listener::parse_listeners(&entries.join("; "));
        if let Some(e) = errors.first() {
            return fail(stream, 400, &format!("wejścia: {e}"));
        }
        if let Some(e) = crate::listener::check_listeners(&list) {
            return fail(stream, 400, &format!("wejścia: {e}"));
        }
        let Some(path) = self.inputs_path() else {
            return fail(stream, 409, "brak etv_config – wejść nie ma gdzie zapisać");
        };
        let written = fs::create_dir_all(live_shows::folder(&self.etv_config))
            .and_then(|()| crate::sync::write_atomic(&path, &inputs_to_json(&list)));
        if let Err(e) = written {
            return fail(stream, 500, &format!("zapis inputs.json: {e}"));
        }
        let channels: Vec<&str> = list.iter().map(|l| l.channel.as_str()).collect();
        self.record(
            ingest,
            who,
            "",
            &format!("wejścia studia zmienione: {}", if channels.is_empty() { "brak".into() } else { channels.join(", ") }),
        );
        *lock(&self.listeners) = (list, true);
        reply(stream, 200, &self.inputs_json(ingest))
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

    /// The channels the panel knows: ErsatzTV's, those with a source, a decision or a listener.
    fn known_channels(&self) -> Vec<String> {
        let mut numbers: Vec<String> = lock(&self.cache).channels.iter().map(|(n, _)| n.clone()).collect();
        let settings = lock(&self.settings);
        let extra = settings.channels.keys().cloned().chain(lock(&self.listeners).0.iter().map(|l| l.channel.clone()).collect::<Vec<_>>());
        for number in extra {
            if !numbers.contains(&number) {
                numbers.push(number);
            }
        }
        sort_channels(&mut numbers);
        numbers
    }

    /// `STATE` of every channel's antenna → the layers on air (K12), with the
    /// moment each page went on there and whether a person took it in (K13).
    fn refresh_layers(&self) {
        let now = crate::time::now_ms();
        for channel in self.known_channels() {
            let config = self.config.for_channel(&channel);
            let Ok(reply) = crate::vizrs::state(&config.vizrs.address, &config.vizrs.token, &config.command_prefix) else {
                lock(&self.layers).seen.remove(&channel);
                continue;
            };
            let mut layers = crate::vizrs::layers(&reply);
            let mut state = lock(&self.layers);
            let mut keys = Vec::new();
            let mut read = Vec::new();
            for layer in &mut layers {
                let name = layer.get("layer").map(crate::template::js_string).unwrap_or_default();
                let page = layer.get("page").map(crate::template::js_string).unwrap_or_default();
                read.push((name.clone(), page.clone()));
                let key = format!("{channel}|{name}");
                keys.push(key.clone());
                // A page taken in from the panel: its layer is known now.
                if let Some(taken) = state.manual.remove(&format!("{channel}|{page}")) {
                    state.manual.insert(key.clone(), taken);
                }
                let since = match state.since.get(&key) {
                    Some((seen, at)) if *seen == page => *at,
                    _ => {
                        state.since.insert(key.clone(), (page.clone(), now));
                        now
                    }
                };
                let manual = state.manual.get(&key).is_some_and(|p| *p == page);
                if let Value::Object(members) = layer {
                    members.push(("since".into(), (since as f64).into()));
                    if manual {
                        members.push(("manual".into(), true.into()));
                    }
                }
            }
            state.since.retain(|key, _| !key.starts_with(&format!("{channel}|")) || keys.contains(key));
            state.manual.retain(|key, _| !key.starts_with(&format!("{channel}|")) || keys.contains(key));
            // As-run (K14): what changed on air since the previous read and was not
            // sent by the sidecar (rules, VizRS Live) came from Trio, a macro and so on.
            if let Some((before, at)) = state.seen.insert(channel.clone(), (read.clone(), now)) {
                let sent = crate::vizrs::sent_since(&config.vizrs.address, &config.command_prefix, at - SENT_SLACK_MS);
                for (kind, layer, page) in crate::vizrs::foreign_changes(&before, &read, &sent) {
                    let name = layers
                        .iter()
                        .find(|l| l.get("page").map(crate::template::js_string).as_deref() == Some(page.as_str()))
                        .and_then(|l| l.get("name").and_then(Value::as_str))
                        .unwrap_or_default();
                    let entry = crate::asrun::Entry::of(if kind == "take" { crate::asrun::TAKE } else { crate::asrun::OUT })
                        .title(name)
                        .page(page.clone())
                        .layer(layer.parse::<f64>().ok())
                        .operator(FOREIGN_OPERATOR)
                        .detail(format!("wykryte na antenie (warstwa {layer}): Trio, makro, karuzela albo inne sterowanie VizRS"));
                    crate::asrun::log(&self.etv_config, &channel, &entry);
                }
            }
            state.by_channel.insert(channel, layers);
        }
    }

    fn layers_of(&self, channel: &str) -> Vec<Value> {
        lock(&self.layers).by_channel.get(channel).cloned().unwrap_or_default()
    }

    /// `POST /api/live/channels/<n>/take|continue|out` (K13): one page or layer
    /// on the channel's antenna, by hand from VizRS Live. A page taken in this
    /// way stays until it is taken out: the rules put their own pages on other
    /// layers, and the next `OUT` of this one lets them have it back.
    fn command(&self, channel: &str, action: &str, body: &Value, stream: &mut TcpStream, ingest: &Ingest, who: &str) -> io::Result<()> {
        if !Self::valid_channel(channel) {
            return fail(stream, 400, "zły numer kanału");
        }
        let page = body.get("page").map(crate::template::js_string).unwrap_or_default();
        let layer = body.get("layer").map(crate::template::js_string).unwrap_or_default();
        let target = if action == "take" { page.clone() } else { layer.clone() };
        if target.trim().is_empty() || target.contains(|c: char| c.is_whitespace()) {
            return fail(stream, 400, if action == "take" { "podaj stronę (page)" } else { "podaj warstwę (layer)" });
        }
        let verb = match action {
            "take" => "TAKE",
            "continue" => "CONTINUE",
            _ => "OUT",
        };
        let config = self.config.for_channel(channel);
        let command = format!("{verb} {target}");
        match crate::vizrs::send(&config.vizrs.address, &config.vizrs.token, &config.command_prefix, std::slice::from_ref(&command), false)
        {
            Ok(sent) => {
                let answer = sent.replies.first().map(|(_, r)| r.clone()).unwrap_or_default();
                if !answer.starts_with("OK") {
                    return fail(stream, 502, &format!("VizRS {command}: {answer}"));
                }
                let entry = crate::asrun::Entry::of(match action {
                    "take" => crate::asrun::TAKE,
                    "continue" => crate::asrun::CONTINUE,
                    _ => crate::asrun::OUT,
                })
                .page(page.clone())
                .layer(body.get("layer").and_then(Value::as_f64))
                .operator(who);
                crate::asrun::log(&self.etv_config, channel, &entry);
                self.record(ingest, who, channel, &format!("{command} (ręcznie z panelu)"));
                // The page a person took in is theirs until it goes out.
                {
                    // Which layer VizRS put the page on is known at the next refresh;
                    // until then the page itself marks the layer as a person's.
                    let mut state = lock(&self.layers);
                    match action {
                        "take" => {
                            state.manual.insert(format!("{channel}|{page}"), page.clone());
                        }
                        "out" => {
                            state.manual.remove(&format!("{channel}|{layer}"));
                        }
                        _ => {}
                    }
                }
                reply(stream, 200, &json::object([("ok", true.into()), ("reply", answer.as_str().into())]))
            }
            Err(e) => fail(stream, 502, &format!("VizRS: {e}")),
        }
    }

    /// `GET /api/live/asrun?channel=<n>&day=<YYYY-MM-DD>` (K14).
    fn asrun(&self, request: &Request, stream: &mut TcpStream) -> io::Result<()> {
        let channel = request.param("channel").unwrap_or("").to_owned();
        let day = request.param("day").unwrap_or("").to_owned();
        let day = if day.is_empty() { crate::asrun::day_of(crate::time::now_ms()) } else { day };
        if !Self::valid_channel(&channel) {
            return fail(stream, 400, "zły numer kanału");
        }
        if !crate::asrun::valid_day(&day) {
            return fail(stream, 400, "zły dzień (RRRR-MM-DD)");
        }
        let entries = crate::asrun::read(&self.etv_config, &channel, &day);
        reply(
            stream,
            200,
            &json::object([
                ("channel", channel.as_str().into()),
                ("day", day.as_str().into()),
                ("tz", "UTC".into()),
                ("keep_days", (crate::asrun::KEEP_DAYS as f64).into()),
                ("store", (!self.etv_config.is_empty()).into()),
                ("entries", Value::Array(entries)),
            ]),
        )
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
            ("POST", ["channels", channel, action @ ("take" | "continue" | "out")]) => {
                self.command(channel, action, &body, &mut stream, ingest, &who)
            }
            ("GET", ["asrun"]) => self.asrun(request, &mut stream),
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
            ("POST", ["graphics", action @ ("off" | "on")]) => self.graphics_switch(*action == "off", &mut stream, ingest, &who),
            ("GET", ["graphics"]) => reply(&mut stream, 200, &self.graphics.to_json()),
            ("PUT", ["graphics"]) => self.put_graphics(&body, &mut stream, ingest, &who),
            ("GET", ["inputs"]) => reply(&mut stream, 200, &self.inputs_json(ingest)),
            ("PUT", ["inputs"]) => self.put_inputs(&body, &mut stream, ingest, &who),
            ("GET", ["shows"]) => reply(&mut stream, 200, &self.shows_state()),
            ("POST", ["shows", "scan"]) => self.scan(&mut stream, ingest, &who),
            ("POST", ["shows"]) => self.create(&body, &mut stream, ingest, &who),
            ("PUT", ["shows", id]) => self.update(id, &body, &mut stream, ingest, &who),
            ("DELETE", ["shows", id]) => self.delete(id, &mut stream, ingest, &who),
            _ => fail(&mut stream, 404, "nie ma takiego adresu API"),
        }
    }

    /// `POST /api/live/graphics/off` · `/on`: every channel's graphics off the air at
    /// once (transparent frames from the next frame) or back. Kept in `control.json`.
    fn graphics_switch(&self, off: bool, stream: &mut TcpStream, ingest: &Ingest, who: &str) -> io::Result<()> {
        crate::vizrs::graphics_switch(off, true);
        let next = {
            let mut settings = lock(&self.settings);
            settings.graphics_off = off;
            settings.graphics_off_since = off.then(crate::time::now_ms);
            settings.clone()
        };
        crate::asrun::log(
            &self.etv_config,
            "",
            &crate::asrun::Entry::of(if off { crate::asrun::GRAPHICS_OFF } else { crate::asrun::GRAPHICS_ON }).operator(who),
        );
        self.record(
            ingest,
            who,
            "",
            if off { "ZDEJMIJ GRAFIKĘ – OUT grafiki na wszystkich kanałach" } else { "PRZYWRÓĆ GRAFIKĘ – TAKE stron z powrotem" },
        );
        // The switch works even when the disk fails; it is only not kept.
        if let Err(e) = self.save_settings(&next) {
            crate::warn!("control.json: {e}");
        }
        reply(stream, 200, &self.settings_json())
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

    /// What this build can do, for VizRS Live (an older sidecar simply lists less).
    fn features(&self) -> Vec<Value> {
        let mut features = vec![Value::from("layers"), Value::from("take"), Value::from("asrun")];
        if !self.config.alert_webhook.is_empty() {
            features.push("webhook".into());
        }
        features
    }

    fn settings_json(&self) -> Value {
        let settings = lock(&self.settings);
        json::object([
            ("library", settings.library.map_or(Value::Null, Value::from)),
            ("store", (!self.etv_config.is_empty()).into()),
            ("graphics_off", crate::frames::graphics_off().into()),
            ("graphics_off_since", settings.graphics_off_since.map_or(Value::Null, |t| (t as f64).into())),
        ])
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
            .chain(lock(&self.listeners).0.iter().map(|l| l.channel.clone()).collect::<Vec<_>>());
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
                    ("inputs", Value::Array(self.inputs_of(number))),
                    ("layers", Value::Array(self.layers_of(number))),
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
            ("features", Value::Array(self.features())),
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
        crate::asrun::log(
            &self.etv_config,
            channel,
            &crate::asrun::Entry::of(if hold { crate::asrun::HOLD } else { crate::asrun::RELEASE }).operator(who),
        );
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
    fn inputs_round_trip() {
        let (list, _) = crate::listener::parse_listeners("1=rtmp://0.0.0.0:1936/live/a; 2=srt://0.0.0.0:9000?passphrase=0123456789");
        assert_eq!(inputs_from_json(&inputs_to_json(&list)).unwrap(), list);
        assert_eq!(inputs_from_json(r#"{"listeners":""}"#).unwrap(), []);
        assert!(inputs_from_json(r#"{"listeners":"1=http://x"}"#).is_err());
    }

    #[test]
    fn settings_round_trip() {
        let mut settings =
            Settings { library: Some(7), channels: BTreeMap::new(), graphics_off: true, graphics_off_since: Some(1_700_000_000_000) };
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
