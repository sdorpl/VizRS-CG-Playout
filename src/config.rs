// Configuration: defaults, a JSON file (optional), environment overrides, with
// the names and precedence of the Node.js sidecar (DOKUMENTACJA §5).

use std::path::Path;

use crate::json::{self, Value};

#[derive(Clone, Debug, PartialEq)]
pub struct Vizrs {
    pub address: String,
    pub token: String,
}

/// Regular expressions (case-insensitive) for commercials, on path and title.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AdMatch {
    pub path: String,
    pub title: String,
}

/// Per-show overrides for graphics elements: a show matched by `show_title`
/// (case-insensitive regex on `MediaItem_ShowTitle`) gets its own `on_clip`,
/// `on_ad` and/or `ad_match` instead of the global settings.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShowOverride {
    /// Case-insensitive regex matched against `show_title`.
    pub show_title: String,
    /// Override for `on_clip`; `None` = use the global value.
    pub on_clip: Option<String>,
    /// Override for `on_ad`; `None` = use the global value.
    pub on_ad: Option<String>,
    /// Override for `ad_match`; `None` = use the global value.
    pub ad_match: Option<AdMatch>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub listen: String,
    pub vizrs: Vizrs,
    /// VizRS /output page burned into the video; empty = commands only.
    pub output_url: String,
    pub on_clip: String,
    pub on_ad: String,
    pub ad_match: AdMatch,
    /// Per-show overrides for `on_clip`, `on_ad` and `ad_match`.
    pub shows: Vec<ShowOverride>,
    /// An item without the VizRS graphics element (no session after the
    /// previous item ends) is treated as a commercial.
    pub ad_without_element: bool,
    /// The graphics of an item go on air this long after its start…
    pub in_seconds: f64,
    /// …and out (`OUT` of the pages taken) this long before its stop; < 0 = no `OUT`.
    pub out_seconds: f64,
    /// How far the packet stream may run ahead of real time (ErsatzTV burst).
    pub lead_seconds: f64,
    /// Size Chromium renders /output at: "" = channel size, "720p", "1280x720".
    pub render_size: String,
    /// Graphics area of the channel: "" = all of it, "1920x140+0+940" = only that part.
    pub area: String,
    /// Upper limit of the capture rate, e.g. 25 on small ARM hosts; 0 = the channel rate.
    pub max_fps: f64,
    /// How Chromium's pictures are taken: "frames" (begin frames, the default) or "screencast".
    pub capture_mode: String,
    /// Seconds added to the clock of the graphics page: the transcode runs
    /// ahead of the air time, so clocks on air would run late by as much.
    pub clock_offset_seconds: f64,
    /// The sidecar draws a standing VizRS crawl itself (frames mode): Chromium films its tape only now and then.
    pub tape: bool,
    /// Who draws the burn-in: "chromium" (the default), "native" (VizRS `vizrs-render`) or "auto".
    pub renderer: String,
    /// The native renderer program; "" = `vizrs-render` next to the sidecar or on `PATH`.
    pub renderer_bin: String,
    pub browser: String,
    pub no_sandbox: bool,
    /// Extra Chromium flags, e.g. fewer raster threads on small ARM hosts.
    pub browser_args: Vec<String>,
    /// Wait after an item's stop before its gap counts as an ad (tests only).
    pub gap_grace_ms: Option<f64>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            listen: "0.0.0.0:7700".into(),
            vizrs: Vizrs { address: "127.0.0.1:6100".into(), token: String::new() },
            output_url: String::new(),
            on_clip: String::new(),
            on_ad: String::new(),
            ad_match: AdMatch::default(),
            shows: Vec::new(),
            ad_without_element: true,
            in_seconds: 5.0,
            out_seconds: 5.0,
            lead_seconds: 4.0,
            render_size: String::new(),
            area: String::new(),
            max_fps: 0.0,
            capture_mode: "frames".into(),
            clock_offset_seconds: 0.0,
            tape: false,
            renderer: "auto".into(),
            renderer_bin: String::new(),
            browser: String::new(),
            no_sandbox: false,
            browser_args: Vec::new(),
            gap_grace_ms: None,
        }
    }
}

fn string(value: &Value, name: &str) -> Result<String, String> {
    match value {
        Value::String(s) => Ok(s.clone()),
        Value::Null => Ok(String::new()),
        _ => Err(format!("{name}: expected a string")),
    }
}

/// A number, also as a numeric string (the file may come from a template).
fn number(value: &Value, name: &str) -> Result<f64, String> {
    match value {
        Value::Number(n) => Ok(*n),
        Value::String(s) => s.trim().parse().map_err(|_| format!("{name}: expected a number")),
        _ => Err(format!("{name}: expected a number")),
    }
}

fn boolean(value: &Value, name: &str) -> Result<bool, String> {
    value.as_bool().ok_or_else(|| format!("{name}: expected true or false"))
}

/// `\n` in an environment value is a line break (one command per line).
fn unescape(value: &str) -> String {
    value.replace("\\n", "\n")
}

impl Config {
    /// Defaults with the file's options on top, then the environment
    /// (`env(name)`), as `loadConfig` of the Node.js sidecar. Returns the
    /// config and warnings (unknown options).
    pub fn from_values(file: Option<&Value>, env: &dyn Fn(&str) -> Option<String>) -> Result<(Config, Vec<String>), String> {
        let mut config = Config::default();
        let mut warnings = Vec::new();

        if let Some(file) = file {
            let Value::Object(members) = file else { return Err("config file: expected a JSON object".into()) };
            for (key, value) in members {
                match key.as_str() {
                    "listen" => config.listen = string(value, key)?,
                    "vizrs" => {
                        if let Some(v) = value.get("address") {
                            config.vizrs.address = string(v, "vizrs.address")?;
                        }
                        if let Some(v) = value.get("token") {
                            config.vizrs.token = string(v, "vizrs.token")?;
                        }
                    }
                    "output_url" => config.output_url = string(value, key)?,
                    "on_clip" => config.on_clip = string(value, key)?,
                    "on_ad" => config.on_ad = string(value, key)?,
                    "ad_match" => {
                        if let Some(v) = value.get("path") {
                            config.ad_match.path = string(v, "ad_match.path")?;
                        }
                        if let Some(v) = value.get("title") {
                            config.ad_match.title = string(v, "ad_match.title")?;
                        }
                    }
                    "shows" => {
                        let items = value.as_array().ok_or("shows: expected an array")?;
                        for (i, item) in items.iter().enumerate() {
                            let prefix = format!("shows[{i}]");
                            let show_title =
                                item.get("show_title").map(|v| string(v, &format!("{prefix}.show_title"))).transpose()?.unwrap_or_default();
                            if show_title.is_empty() {
                                return Err(format!("{prefix}: show_title is required"));
                            }
                            let on_clip = item.get("on_clip").map(|v| string(v, &format!("{prefix}.on_clip"))).transpose()?;
                            let on_ad = item.get("on_ad").map(|v| string(v, &format!("{prefix}.on_ad"))).transpose()?;
                            let ad_match = if item.get("ad_match").is_some_and(|v| !v.is_null()) {
                                let v = item.get("ad_match").unwrap();
                                let path =
                                    v.get("path").map(|p| string(p, &format!("{prefix}.ad_match.path"))).transpose()?.unwrap_or_default();
                                let title =
                                    v.get("title").map(|t| string(t, &format!("{prefix}.ad_match.title"))).transpose()?.unwrap_or_default();
                                Some(AdMatch { path, title })
                            } else {
                                None
                            };
                            config.shows.push(ShowOverride { show_title, on_clip, on_ad, ad_match });
                        }
                    }
                    "ad_without_element" => config.ad_without_element = boolean(value, key)?,
                    "in_seconds" => config.in_seconds = number(value, key)?,
                    "out_seconds" => config.out_seconds = number(value, key)?,
                    "lead_seconds" => config.lead_seconds = number(value, key)?,
                    "render_size" => config.render_size = string(value, key)?,
                    "area" => config.area = string(value, key)?,
                    "max_fps" => config.max_fps = number(value, key)?,
                    "capture_mode" => config.capture_mode = string(value, key)?,
                    "clock_offset_seconds" => config.clock_offset_seconds = number(value, key)?,
                    "tape" => config.tape = boolean(value, key)?,
                    "renderer" => config.renderer = string(value, key)?,
                    "renderer_bin" => config.renderer_bin = string(value, key)?,
                    "browser" => config.browser = string(value, key)?,
                    "no_sandbox" => config.no_sandbox = boolean(value, key)?,
                    "browser_args" => {
                        let items = value.as_array().ok_or("browser_args: expected an array of strings")?;
                        config.browser_args = items.iter().map(|v| string(v, "browser_args")).collect::<Result<_, _>>()?;
                    }
                    "gap_grace_ms" => config.gap_grace_ms = Some(number(value, key)?),
                    _ => warnings.push(format!("config: unknown option \"{key}\" ignored")),
                }
            }
        }

        if let Some(v) = env("LISTEN").filter(|v| !v.is_empty()) {
            config.listen = v;
        }
        if let Some(v) = env("VIZRS_ADDRESS").filter(|v| !v.is_empty()) {
            config.vizrs.address = v;
        }
        if let Some(v) = env("VIZRS_TOKEN") {
            config.vizrs.token = v;
        }
        if let Some(v) = env("VIZRS_OUTPUT_URL") {
            config.output_url = v;
        }
        if let Some(v) = env("VIZRS_ON_CLIP") {
            config.on_clip = unescape(&v);
        }
        if let Some(v) = env("VIZRS_ON_AD") {
            config.on_ad = unescape(&v);
        }
        if let Some(v) = env("VIZRS_AD_PATH") {
            config.ad_match.path = v;
        }
        if let Some(v) = env("VIZRS_AD_TITLE") {
            config.ad_match.title = v;
        }
        if let Some(v) = env("VIZRS_AD_WITHOUT_ELEMENT") {
            config.ad_without_element = v != "0";
        }
        if let Some(v) = env("LEAD_SECONDS").filter(|v| !v.is_empty()) {
            match v.trim().parse() {
                Ok(seconds) => config.lead_seconds = seconds,
                Err(_) => warnings.push(format!("LEAD_SECONDS=\"{v}\" is not a number; using {}", config.lead_seconds)),
            }
        }
        for (name, seconds) in [("VIZRS_IN_SECONDS", &mut config.in_seconds), ("VIZRS_OUT_SECONDS", &mut config.out_seconds)] {
            if let Some(v) = env(name).filter(|v| !v.is_empty()) {
                match v.trim().parse() {
                    Ok(value) => *seconds = value,
                    Err(_) => warnings.push(format!("{name}=\"{v}\" is not a number; using {seconds}")),
                }
            }
        }
        if let Some(v) = env("VIZRS_RENDER_SIZE") {
            config.render_size = v;
        }
        if let Some(v) = env("VIZRS_AREA") {
            config.area = v;
        }
        if let Some(v) = env("VIZRS_MAX_FPS").filter(|v| !v.is_empty()) {
            match v.trim().parse() {
                Ok(fps) => config.max_fps = fps,
                Err(_) => warnings.push(format!("VIZRS_MAX_FPS=\"{v}\" is not a number; using {}", config.max_fps)),
            }
        }
        if let Some(v) = env("VIZRS_CLOCK_OFFSET").filter(|v| !v.is_empty()) {
            match v.trim().parse() {
                Ok(seconds) => config.clock_offset_seconds = seconds,
                Err(_) => warnings.push(format!("VIZRS_CLOCK_OFFSET=\"{v}\" is not a number; using {}", config.clock_offset_seconds)),
            }
        }
        if let Some(v) = env("VIZRS_CAPTURE_MODE").filter(|v| !v.is_empty()) {
            config.capture_mode = v;
        }
        if let Some(v) = env("VIZRS_TAPE").filter(|v| !v.is_empty()) {
            config.tape = v == "1";
        }
        if let Some(v) = env("VIZRS_RENDERER").filter(|v| !v.is_empty()) {
            config.renderer = v;
        }
        if let Some(v) = env("VIZRS_RENDERER_BIN").filter(|v| !v.is_empty()) {
            config.renderer_bin = v;
        }
        if let Some(v) = env("VIZRS_BROWSER").filter(|v| !v.is_empty()) {
            config.browser = v;
        }
        if env("VIZRS_BROWSER_NO_SANDBOX").as_deref() == Some("1") {
            config.no_sandbox = true;
        }
        if let Some(v) = env("VIZRS_BROWSER_ARGS").filter(|v| !v.is_empty()) {
            config.browser_args = v.split_whitespace().map(str::to_owned).collect();
        }

        Ok((config, warnings))
    }

    /// `file` (optional) and the process environment.
    pub fn load(file: Option<&Path>) -> Result<(Config, Vec<String>), String> {
        let value = match file {
            Some(path) => {
                let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
                Some(json::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?)
            }
            None => None,
        };
        Config::from_values(value.as_ref(), &|name| std::env::var(name).ok())
    }

    /// The config as JSON, keys in the order of the Node.js defaults.
    pub fn to_json(&self) -> Value {
        let mut value = json::object([
            ("listen", self.listen.as_str().into()),
            ("vizrs", json::object([("address", self.vizrs.address.as_str().into()), ("token", self.vizrs.token.as_str().into())])),
            ("output_url", self.output_url.as_str().into()),
            ("on_clip", self.on_clip.as_str().into()),
            ("on_ad", self.on_ad.as_str().into()),
            ("ad_match", json::object([("path", self.ad_match.path.as_str().into()), ("title", self.ad_match.title.as_str().into())])),
            ("ad_without_element", self.ad_without_element.into()),
            ("lead_seconds", self.lead_seconds.into()),
            ("render_size", self.render_size.as_str().into()),
            ("area", self.area.as_str().into()),
            ("max_fps", self.max_fps.into()),
            ("capture_mode", self.capture_mode.as_str().into()),
            ("browser", self.browser.as_str().into()),
            ("no_sandbox", self.no_sandbox.into()),
            ("browser_args", self.browser_args.iter().map(String::as_str).collect::<Vec<_>>().into()),
        ]);
        if let (Some(ms), Value::Object(members)) = (self.gap_grace_ms, &mut value) {
            members.push(("gap_grace_ms".into(), ms.into()));
        }
        value
    }

    /// `host:port` of `listen`; an empty host listens on all interfaces.
    pub fn listen_addr(&self) -> String {
        match self.listen.rsplit_once(':') {
            Some(("", port)) => format!("0.0.0.0:{port}"),
            _ => self.listen.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| pairs.iter().find(|(k, _)| *k == name).map(|(_, v)| (*v).to_owned())
    }

    #[test]
    fn file_types_and_unknown_options() {
        let file = json::parse(r#"{"lead_seconds":"6","extra":1,"vizrs":{"token":"t"}}"#).unwrap();
        let (config, warnings) = Config::from_values(Some(&file), &env(&[])).unwrap();
        assert_eq!(config.lead_seconds, 6.0);
        assert_eq!(config.vizrs.address, "127.0.0.1:6100");
        assert_eq!(warnings, ["config: unknown option \"extra\" ignored"]);

        let bad = json::parse(r#"{"no_sandbox":"yes"}"#).unwrap();
        assert!(Config::from_values(Some(&bad), &env(&[])).unwrap_err().contains("no_sandbox"));
        assert!(Config::from_values(Some(&Value::Array(vec![])), &env(&[])).is_err());
    }

    #[test]
    fn clock_offset_from_the_file_and_the_environment() {
        let file = json::parse(r#"{"clock_offset_seconds":12.5}"#).unwrap();
        let (config, _) = Config::from_values(Some(&file), &env(&[])).unwrap();
        assert_eq!(config.clock_offset_seconds, 12.5);
        let (config, warnings) = Config::from_values(None, &env(&[("VIZRS_CLOCK_OFFSET", "28")])).unwrap();
        assert_eq!(config.clock_offset_seconds, 28.0);
        assert!(warnings.is_empty());
        let (config, warnings) = Config::from_values(None, &env(&[("VIZRS_CLOCK_OFFSET", "x")])).unwrap();
        assert_eq!(config.clock_offset_seconds, 0.0);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn bad_lead_seconds_in_env_keeps_the_value() {
        let (config, warnings) = Config::from_values(None, &env(&[("LEAD_SECONDS", "abc")])).unwrap();
        assert_eq!(config.lead_seconds, 4.0);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn in_and_out_seconds() {
        assert_eq!((Config::default().in_seconds, Config::default().out_seconds), (5.0, 5.0));
        let file = json::parse(r#"{"in_seconds":8,"out_seconds":"15"}"#).unwrap();
        let (config, _) = Config::from_values(Some(&file), &env(&[])).unwrap();
        assert_eq!((config.in_seconds, config.out_seconds), (8.0, 15.0));
        let (config, warnings) = Config::from_values(Some(&file), &env(&[("VIZRS_IN_SECONDS", "0"), ("VIZRS_OUT_SECONDS", "x")])).unwrap();
        assert_eq!((config.in_seconds, config.out_seconds), (0.0, 15.0));
        assert_eq!(warnings, ["VIZRS_OUT_SECONDS=\"x\" is not a number; using 15"]);
    }

    #[test]
    fn tape() {
        assert!(!Config::default().tape);
        let file = json::parse(r#"{"tape":true}"#).unwrap();
        assert!(Config::from_values(Some(&file), &env(&[])).unwrap().0.tape);
        assert!(!Config::from_values(Some(&file), &env(&[("VIZRS_TAPE", "0")])).unwrap().0.tape);
        assert!(Config::from_values(None, &env(&[("VIZRS_TAPE", "1")])).unwrap().0.tape);
    }

    #[test]
    fn renderer() {
        assert_eq!(Config::default().renderer, "auto");
        let file = json::parse(r#"{"renderer":"auto","renderer_bin":"/opt/vizrs-render"}"#).unwrap();
        let (config, warnings) = Config::from_values(Some(&file), &env(&[])).unwrap();
        assert_eq!((config.renderer.as_str(), config.renderer_bin.as_str()), ("auto", "/opt/vizrs-render"));
        assert!(warnings.is_empty());
        let (config, _) = Config::from_values(Some(&file), &env(&[("VIZRS_RENDERER", "native"), ("VIZRS_RENDERER_BIN", "/x")])).unwrap();
        assert_eq!((config.renderer.as_str(), config.renderer_bin.as_str()), ("native", "/x"));
    }

    #[test]
    fn listen_address() {
        let mut config = Config::default();
        assert_eq!(config.listen_addr(), "0.0.0.0:7700");
        config.listen = ":7701".into();
        assert_eq!(config.listen_addr(), "0.0.0.0:7701");
        config.listen = "[::1]:7700".into();
        assert_eq!(config.listen_addr(), "[::1]:7700");
    }

    #[test]
    fn shows_config_parsing() {
        let file = json::parse(
            r#"{
                "shows": [
                    {
                        "show_title": "Wiadomości",
                        "on_clip": "TAKE 1001",
                        "on_ad": "TAKE 2001",
                        "ad_match": { "path": "ad_news", "title": "reklama" }
                    },
                    {
                        "show_title": "Kino"
                    }
                ]
            }"#,
        )
        .unwrap();
        let (config, _) = Config::from_values(Some(&file), &env(&[])).unwrap();
        assert_eq!(config.shows.len(), 2);
        assert_eq!(config.shows[0].show_title, "Wiadomości");
        assert_eq!(config.shows[0].on_clip.as_deref(), Some("TAKE 1001"));
        assert_eq!(config.shows[0].on_ad.as_deref(), Some("TAKE 2001"));
        assert_eq!(config.shows[0].ad_match.as_ref().unwrap(), &AdMatch { path: "ad_news".into(), title: "reklama".into() });
        assert_eq!(config.shows[1].show_title, "Kino");
        assert_eq!(config.shows[1].on_clip, None);
        assert_eq!(config.shows[1].on_ad, None);
        assert_eq!(config.shows[1].ad_match, None);

        let bad = json::parse(r#"{"shows": [{}]}"#).unwrap();
        assert!(Config::from_values(Some(&bad), &env(&[])).unwrap_err().contains("show_title is required"));
    }
}
