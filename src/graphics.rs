// Graphics rules (on_clip, on_ad, ad_match, on_end, end_seconds, shows and
// their per-channel overrides, and on_live – only per channel) that the live API
// reads and replaces:
//
//   GET /api/live/graphics   {"rules": {…}, "source": "config"|"file", "store": bool}
//   PUT /api/live/graphics   {…} – the whole rules document, as in config.json
//
// Until the first PUT the rules come from config.json and the environment. A
// PUT validates the document as at start (and refuses patterns that do not
// compile), writes `<etv_config>/vizrs-live/graphics.json` and from then on the
// file wins over config.json. Channels pick the new rules from the next clip;
// graphics already planned stay. Addresses, tokens and output_url stay in
// config.json – they are not graphics rules.

use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::{RwLock, RwLockReadGuard};

use crate::config::{self, AdMatch, ChannelOverride, Config, ShowOverride};
use crate::json::{self, Value};
use crate::live_shows;
use crate::template::{AdMatcher, ShowMatcher};

/// Rules of one channel; `None` = the global value.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChannelRules {
    pub channel: String,
    pub on_clip: Option<String>,
    pub on_ad: Option<String>,
    pub on_end: Option<String>,
    pub end_seconds: Option<f64>,
    pub shows: Option<Vec<ShowOverride>>,
    /// Graphics of a live takeover of the channel (a LIVE logo); held schedule meanwhile.
    pub on_live: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rules {
    pub on_clip: String,
    pub on_ad: String,
    pub ad_match: AdMatch,
    pub on_end: String,
    pub end_seconds: f64,
    pub shows: Vec<ShowOverride>,
    pub channels: Vec<ChannelRules>,
}

impl Rules {
    /// The rules of a configuration.
    pub fn of(config: &Config) -> Rules {
        Rules {
            on_clip: config.on_clip.clone(),
            on_ad: config.on_ad.clone(),
            ad_match: config.ad_match.clone(),
            on_end: config.on_end.clone(),
            end_seconds: config.end_seconds,
            shows: config.shows.clone(),
            channels: config
                .channels
                .iter()
                .filter(|c| {
                    c.on_clip.is_some()
                        || c.on_ad.is_some()
                        || c.on_end.is_some()
                        || c.end_seconds.is_some()
                        || c.shows.is_some()
                        || c.on_live.is_some()
                })
                .map(|c| ChannelRules {
                    channel: c.channel.clone(),
                    on_clip: c.on_clip.clone(),
                    on_ad: c.on_ad.clone(),
                    on_end: c.on_end.clone(),
                    end_seconds: c.end_seconds,
                    shows: c.shows.clone(),
                    on_live: c.on_live.clone(),
                })
                .collect(),
        }
    }

    /// `config` with these rules instead of its own (the rest stays).
    pub fn apply(&self, config: &mut Config) {
        config.on_clip.clone_from(&self.on_clip);
        config.on_ad.clone_from(&self.on_ad);
        config.ad_match.clone_from(&self.ad_match);
        config.on_end.clone_from(&self.on_end);
        config.end_seconds = self.end_seconds;
        config.shows.clone_from(&self.shows);
        for own in &mut config.channels {
            own.on_clip = None;
            own.on_ad = None;
            own.on_end = None;
            own.end_seconds = None;
            own.shows = None;
            own.on_live = None;
        }
        for rules in &self.channels {
            let own = match config.channels.iter().position(|c| c.channel == rules.channel) {
                Some(i) => &mut config.channels[i],
                None => {
                    config.channels.push(ChannelOverride { channel: rules.channel.clone(), ..ChannelOverride::default() });
                    config.channels.last_mut().expect("just pushed")
                }
            };
            own.on_clip.clone_from(&rules.on_clip);
            own.on_ad.clone_from(&rules.on_ad);
            own.on_end.clone_from(&rules.on_end);
            own.end_seconds = rules.end_seconds;
            own.shows.clone_from(&rules.shows);
            own.on_live.clone_from(&rules.on_live);
        }
    }

    /// A rules document (config.json shape); missing fields are empty, 60 s before the end.
    pub fn parse(value: &Value) -> Result<Rules, String> {
        if !matches!(value, Value::Object(_)) {
            return Err("expected an object".into());
        }
        let text = |name: &str| value.get(name).map(|v| config::string(v, name)).transpose().map(Option::unwrap_or_default);
        let ad_match = match value.get("ad_match") {
            Some(v) if !v.is_null() => AdMatch {
                path: v.get("path").map(|p| config::string(p, "ad_match.path")).transpose()?.unwrap_or_default(),
                title: v.get("title").map(|t| config::string(t, "ad_match.title")).transpose()?.unwrap_or_default(),
            },
            _ => AdMatch::default(),
        };
        let mut rules = Rules {
            on_clip: text("on_clip")?,
            on_ad: text("on_ad")?,
            ad_match,
            on_end: text("on_end")?,
            end_seconds: value.get("end_seconds").map(|v| config::seconds(v, "end_seconds")).transpose()?.unwrap_or(60.0),
            shows: value.get("shows").map(|v| config::show_overrides(v, "shows")).transpose()?.unwrap_or_default(),
            channels: Vec::new(),
        };
        match value.get("channels") {
            None | Some(Value::Null) => {}
            Some(Value::Object(members)) => {
                for (channel, item) in members {
                    let prefix = format!("channels.{channel}");
                    if channel.trim().is_empty() || !matches!(item, Value::Object(_)) {
                        return Err(format!("{prefix}: expected a channel number and an object"));
                    }
                    let field = |name: &str| item.get(name).map(|v| config::string(v, &format!("{prefix}.{name}"))).transpose();
                    rules.channels.push(ChannelRules {
                        channel: channel.clone(),
                        on_clip: field("on_clip")?,
                        on_ad: field("on_ad")?,
                        on_end: field("on_end")?,
                        end_seconds: item.get("end_seconds").map(|v| config::seconds(v, &format!("{prefix}.end_seconds"))).transpose()?,
                        shows: item.get("shows").map(|v| config::show_overrides(v, &format!("{prefix}.shows"))).transpose()?,
                        on_live: field("on_live")?,
                    });
                }
            }
            Some(_) => return Err("channels: expected an object".into()),
        }
        Ok(rules)
    }

    /// Patterns that do not compile: at start a warning, here a refusal.
    pub fn problems(&self) -> Vec<String> {
        let mut problems = AdMatcher::new(&self.ad_match).1;
        problems.extend(ShowMatcher::new(&self.shows).1);
        for c in &self.channels {
            if let Some(shows) = &c.shows {
                problems.extend(ShowMatcher::new(shows).1.into_iter().map(|w| format!("channels.{}.{w}", c.channel)));
            }
        }
        problems
    }

    pub fn to_json(&self) -> Value {
        let opt = |v: &Option<String>| v.as_deref().map_or(Value::Null, Value::from);
        let show = |s: &ShowOverride| {
            let mut members = vec![("show_title".to_owned(), Value::from(s.show_title.as_str()))];
            for (name, v) in [("on_clip", &s.on_clip), ("on_ad", &s.on_ad), ("on_end", &s.on_end)] {
                if let Some(v) = v {
                    members.push((name.into(), v.as_str().into()));
                }
            }
            if let Some(m) = &s.ad_match {
                members.push(("ad_match".into(), ad_json(m)));
            }
            if let Some(n) = s.end_seconds {
                members.push(("end_seconds".into(), n.into()));
            }
            Value::Object(members)
        };
        let shows = |list: &[ShowOverride]| Value::Array(list.iter().map(show).collect());
        let channels = self
            .channels
            .iter()
            .map(|c| {
                let mut members = Vec::new();
                for (name, v) in [("on_clip", &c.on_clip), ("on_ad", &c.on_ad), ("on_end", &c.on_end), ("on_live", &c.on_live)] {
                    if v.is_some() {
                        members.push((name.to_owned(), opt(v)));
                    }
                }
                if let Some(n) = c.end_seconds {
                    members.push(("end_seconds".into(), n.into()));
                }
                if let Some(list) = &c.shows {
                    members.push(("shows".into(), shows(list)));
                }
                (c.channel.clone(), Value::Object(members))
            })
            .collect();
        json::object([
            ("on_clip", self.on_clip.as_str().into()),
            ("on_ad", self.on_ad.as_str().into()),
            ("ad_match", ad_json(&self.ad_match)),
            ("on_end", self.on_end.as_str().into()),
            ("end_seconds", self.end_seconds.into()),
            ("shows", shows(&self.shows)),
            ("channels", Value::Object(channels)),
        ])
    }
}

fn ad_json(m: &AdMatch) -> Value {
    json::object([("path", m.path.as_str().into()), ("title", m.title.as_str().into())])
}

/// The rules in force, with a number that changes with every replacement.
pub struct Store {
    path: Option<PathBuf>,
    state: RwLock<State>,
}

pub struct State {
    pub generation: u64,
    pub rules: Rules,
    pub from_file: bool,
}

impl Store {
    /// The rules of `config`, or of `graphics.json` when the panel saved one.
    pub fn new(config: &Config) -> Store {
        let path = (!config.etv_config.is_empty()).then(|| live_shows::folder(&config.etv_config).join("graphics.json"));
        let mut state = State { generation: 0, rules: Rules::of(config), from_file: false };
        if let Some(path) = &path {
            match fs::read_to_string(path) {
                Ok(text) => match json::parse(&text).map_err(|e| e.to_string()).and_then(|v| Rules::parse(&v)) {
                    Ok(rules) => {
                        for problem in rules.problems() {
                            crate::warn!("{}: {problem}", path.display());
                        }
                        crate::info!("graphics rules from {} (VizRS Live edits them; config.json rules are not used)", path.display());
                        state.rules = rules;
                        state.from_file = true;
                    }
                    Err(e) => crate::warn!("{}: {e}; graphics rules of config.json are used", path.display()),
                },
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => crate::warn!("{}: {e}; graphics rules of config.json are used", path.display()),
            }
        }
        Store { path, state: RwLock::new(state) }
    }

    pub fn read(&self) -> RwLockReadGuard<'_, State> {
        self.state.read().unwrap_or_else(|e| e.into_inner())
    }

    pub fn generation(&self) -> u64 {
        self.read().generation
    }

    /// `config` with the rules in force.
    pub fn applied(&self, config: &Config) -> Config {
        let mut config = config.clone();
        self.read().rules.apply(&mut config);
        config
    }

    pub fn to_json(&self) -> Value {
        let state = self.read();
        json::object([
            ("rules", state.rules.to_json()),
            ("source", if state.from_file { "file" } else { "config" }.into()),
            ("store", self.path.is_some().into()),
        ])
    }

    /// Validates, writes the file and puts the rules in force. `Err` = nothing changed.
    pub fn replace(&self, value: &Value) -> Result<(), Replace> {
        let rules = Rules::parse(value).map_err(Replace::Invalid)?;
        let problems = rules.problems();
        if !problems.is_empty() {
            return Err(Replace::Invalid(problems.join("; ")));
        }
        let Some(path) = &self.path else { return Err(Replace::NoStore) };
        let mut state = self.state.write().unwrap_or_else(|e| e.into_inner());
        let mut text = rules.to_json().to_string();
        text.push('\n');
        fs::create_dir_all(path.parent().expect("graphics.json has a folder"))
            .and_then(|()| crate::sync::write_atomic(path, &text))
            .map_err(Replace::Io)?;
        state.rules = rules;
        state.from_file = true;
        state.generation += 1;
        Ok(())
    }
}

#[derive(Debug)]
pub enum Replace {
    Invalid(String),
    NoStore,
    Io(io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("vizrs-graphics-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir.to_string_lossy().into_owned()
    }

    #[test]
    fn round_trip_and_apply() {
        let doc = json::parse(
            r#"{"on_clip":"TAKE 4001 headline={title}","ad_match":{"path":"/reklamy/","title":""},"on_end":"TAKE 1405","end_seconds":30,
               "shows":[{"show_title":"^Wiad","on_clip":"TAKE 1001","end_seconds":10}],
               "channels":{"2":{"on_clip":"TAKE 7","on_live":"TAKE 7510","shows":[{"show_title":"x","on_end":""}]}}}"#,
        )
        .unwrap();
        let rules = Rules::parse(&doc).unwrap();
        assert_eq!(Rules::parse(&rules.to_json()).unwrap(), rules);
        let mut config = Config::default();
        config.channels.push(ChannelOverride {
            channel: "3".into(),
            address: Some("v3:6100".into()),
            on_ad: Some("OUT".into()),
            ..Default::default()
        });
        rules.apply(&mut config);
        assert_eq!(config.on_clip, "TAKE 4001 headline={title}");
        assert_eq!(config.end_seconds, 30.0);
        assert_eq!(config.for_channel("2").on_clip, "TAKE 7");
        assert_eq!(config.for_channel("2").shows[0].on_end.as_deref(), Some(""));
        assert_eq!((config.for_channel("2").on_live.as_str(), config.for_channel("1").on_live.as_str()), ("TAKE 7510", ""));
        let three = config.for_channel("3");
        assert_eq!((three.vizrs.address.as_str(), three.on_ad.as_str()), ("v3:6100", ""));
        assert_eq!(Rules::of(&config), rules);
    }

    #[test]
    fn refuses_bad_documents() {
        for bad in [r#"[]"#, r#"{"end_seconds":-1}"#, r#"{"shows":[{"on_clip":"x"}]}"#, r#"{"channels":[]}"#, r#"{"on_clip":5}"#] {
            assert!(Rules::parse(&json::parse(bad).unwrap()).is_err(), "{bad}");
        }
        let rules = Rules::parse(&json::parse(r#"{"ad_match":{"path":"(","title":""},"shows":[{"show_title":"["}]}"#).unwrap()).unwrap();
        assert_eq!(rules.problems().len(), 2);
    }

    #[test]
    fn store_saves_and_loads() {
        let dir = temp("store");
        let config = Config { etv_config: dir.clone(), on_clip: "TAKE 1".into(), ..Config::default() };
        let store = Store::new(&config);
        assert_eq!(store.to_json().get("source").and_then(Value::as_str), Some("config"));
        assert!(matches!(store.replace(&json::parse(r#"{"shows":[{"show_title":"("}]}"#).unwrap()), Err(Replace::Invalid(_))));
        assert_eq!(store.generation(), 0);
        store.replace(&json::parse(r#"{"on_clip":"TAKE 2","channels":{"1":{"on_end":""}}}"#).unwrap()).unwrap();
        assert_eq!(store.generation(), 1);
        assert_eq!(store.applied(&config).on_clip, "TAKE 2");
        let again = Store::new(&config);
        assert_eq!(again.read().rules, store.read().rules);
        assert!(again.read().from_file);
        let _ = fs::remove_dir_all(&dir);
        let none = Store::new(&Config::default());
        assert!(matches!(none.replace(&json::parse("{}").unwrap()), Err(Replace::NoStore)));
    }
}
