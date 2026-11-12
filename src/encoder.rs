// Encoder settings of the ffmpeg overlay, set in VizRS Live (DOKUMENTACJA §1
// „Koder z VizRS Live”): the x264/x265 preset, the H.264 profile of every H.264
// encoder, libx264 options, the presets of NVENC and QSV and the B-frames of the
// hardware encoders (NVENC, QSV, VAAPI). ErsatzTV's next streaming engine passes
// none of these to ffmpeg. The sidecar keeps them in
// `<etv_config>/vizrs-live/encoder.json` (`PUT /api/live/encoder`);
// `vizrs-ffmpeg` in the ErsatzTV container reads the same file
// (`/config/vizrs-live/encoder.json`) at each start, so a change works from the
// next ffmpeg ErsatzTV starts.
//
// A key in the file is in force even when empty (empty = the encoder's
// default), not the environment's. A key missing from the file (a file of an
// older version) and a missing file take the environment's value
// (`VIZRS_FFMPEG_PRESET`, `VIZRS_FFMPEG_X264_PARAMS`, `VIZRS_FFMPEG_NVENC_BF`).
// The sidecar fills the file from the environment once, at its start, like
// `inputs.json` from `VIZRS_INGEST`; VizRS Live writes every key.
//
// The file is checked again on reading: a value outside the lists below is
// left out (the default counts), never passed to ffmpeg.

use std::path::Path;

use crate::json::{self, Value};

/// The file next to `control.json` and `inputs.json`.
pub const FILE: &str = "encoder.json";
/// Where `vizrs-ffmpeg` looks for it in the ErsatzTV container (`VIZRS_FFMPEG_SETTINGS`).
pub const DEFAULT_PATH: &str = "/config/vizrs-live/encoder.json";

/// x264/x265 presets, fastest first.
pub const PRESETS: [&str; 10] = ["ultrafast", "superfast", "veryfast", "faster", "fast", "medium", "slow", "slower", "veryslow", "placebo"];

/// H.264 profiles (8-bit 4:2:0; `high10` and the like need other pixel formats).
/// VAAPI calls baseline `constrained_baseline`.
pub const PROFILES: [&str; 3] = ["baseline", "main", "high"];

/// NVENC presets (SDK 10+), fastest first; the encoder's default is p4.
pub const NVENC_PRESETS: [&str; 7] = ["p1", "p2", "p3", "p4", "p5", "p6", "p7"];

/// QSV presets, fastest first; the encoder's default is the driver's.
pub const QSV_PRESETS: [&str; 7] = ["veryfast", "faster", "fast", "medium", "slow", "slower", "veryslow"];

/// Most B-frames the panel may set for hardware encoders.
pub const MAX_BFRAMES: u32 = 4;

/// libx264 options the panel may set: quality and speed only. Left out on
/// purpose: the GOP and scene cuts (`keyint`, `scenecut`: HLS segments), threads,
/// rate control (the FFmpeg profile's bitrate) and anything naming a file.
pub const X264_KEYS: [&str; 20] = [
    "bframes",
    "b-adapt",
    "b-pyramid",
    "ref",
    "subme",
    "me",
    "merange",
    "trellis",
    "partitions",
    "rc-lookahead",
    "sync-lookahead",
    "aq-mode",
    "aq-strength",
    "psy-rd",
    "mbtree",
    "weightp",
    "weightb",
    "cabac",
    "8x8dct",
    "fast-pskip",
];

const MAX_OPTIONS: usize = 16;
const MAX_VALUE: usize = 32;

/// The keys of the file, in its order.
pub const KEYS: [&str; 6] = ["preset", "profile", "x264_params", "nvenc_preset", "qsv_preset", "bframes"];

/// Every setting; empty = the encoder's default.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Encoder {
    /// libx264 / libx265 preset.
    pub preset: String,
    /// H.264 profile of libx264, NVENC, QSV and VAAPI.
    pub profile: String,
    /// libx264 options.
    pub x264_params: String,
    pub nvenc_preset: String,
    pub qsv_preset: String,
    /// B-frames of NVENC, QSV and VAAPI encoders (a number).
    pub bframes: String,
}

impl Encoder {
    fn field(&self, key: &str) -> &str {
        match key {
            "preset" => &self.preset,
            "profile" => &self.profile,
            "x264_params" => &self.x264_params,
            "nvenc_preset" => &self.nvenc_preset,
            "qsv_preset" => &self.qsv_preset,
            _ => &self.bframes,
        }
    }

    fn field_mut(&mut self, key: &str) -> &mut String {
        match key {
            "preset" => &mut self.preset,
            "profile" => &mut self.profile,
            "x264_params" => &mut self.x264_params,
            "nvenc_preset" => &mut self.nvenc_preset,
            "qsv_preset" => &mut self.qsv_preset,
            _ => &mut self.bframes,
        }
    }

    /// Nothing set: ffmpeg as ErsatzTV calls it.
    pub fn is_empty(&self) -> bool {
        self == &Encoder::default()
    }

    pub fn to_value(&self) -> Value {
        Value::Object(KEYS.iter().map(|&k| (k.to_owned(), Value::from(self.field(k)))).collect())
    }

    pub fn to_json(&self) -> String {
        format!("{}\n", self.to_value())
    }

    /// A request or the environment, each value checked; a wrong one is an error.
    /// Missing keys and `null` are empty.
    pub fn from_value(value: &Value) -> Result<Encoder, String> {
        let mut encoder = Encoder::default();
        for key in KEYS {
            let text = match value.get(key) {
                None | Some(Value::Null) => String::new(),
                Some(Value::String(s)) => s.clone(),
                Some(Value::Number(n)) if key == "bframes" => format!("{n}"),
                Some(_) => return Err(format!("{key}: oczekiwano tekstu")),
            };
            let text = normalize(key, &text);
            check(key, &text)?;
            *encoder.field_mut(key) = text;
        }
        Ok(encoder)
    }

    /// The file at `path` over `env`: each key the file has (a wrong value =
    /// empty), the others from `env`. `None` = no file, or not a JSON object.
    pub fn read(path: &Path, env: &Encoder) -> Option<Encoder> {
        let text = std::fs::read_to_string(path).ok()?;
        let value = json::parse(&text).ok()?;
        if !matches!(value, Value::Object(_)) {
            return None;
        }
        let mut encoder = env.clone();
        for key in KEYS {
            let Some(found) = value.get(key) else { continue };
            let text = normalize(key, found.as_str().unwrap_or_default());
            *encoder.field_mut(key) = if check(key, &text).is_ok() { text } else { String::new() };
        }
        Some(encoder)
    }

    /// The settings in force: the file at `path` over `env` if there is a file, else `env`.
    pub fn in_force(path: Option<&Path>, env: &Encoder) -> (Encoder, bool) {
        match path.and_then(|p| Encoder::read(p, env)) {
            Some(file) => (file, true),
            None => (env.clone(), false),
        }
    }
}

fn normalize(key: &str, text: &str) -> String {
    let text = text.trim();
    match key {
        "x264_params" => normalize_x264(text),
        "profile" | "nvenc_preset" => text.to_ascii_lowercase(),
        _ => text.to_owned(),
    }
}

/// One value of `key`: empty, or from its list.
pub fn check(key: &str, text: &str) -> Result<(), String> {
    let one_of = |list: &[&str], what: &str| {
        if text.is_empty() || list.contains(&text) { Ok(()) } else { Err(format!("{what} „{text}”: jeden z {}", list.join(", "))) }
    };
    match key {
        "preset" => one_of(&PRESETS, "preset x264"),
        "profile" => one_of(&PROFILES, "profil H.264"),
        "x264_params" => check_x264_params(text),
        "nvenc_preset" => one_of(&NVENC_PRESETS, "preset NVENC"),
        "qsv_preset" => one_of(&QSV_PRESETS, "preset QSV"),
        _ if text.is_empty() => Ok(()),
        _ => match text.parse::<u32>() {
            Ok(n) if n <= MAX_BFRAMES && text.bytes().all(|b| b.is_ascii_digit()) => Ok(()),
            _ => Err(format!("klatki B „{text}”: liczba 0–{MAX_BFRAMES}")),
        },
    }
}

/// The H.264 profile `profile` as `encoder` names it; `None` = not an H.264 encoder.
pub fn profile_for(encoder: &str, profile: &str) -> Option<String> {
    match encoder {
        "libx264" | "h264_nvenc" | "h264_qsv" => Some(profile.to_owned()),
        "h264_vaapi" if profile == "baseline" => Some("constrained_baseline".to_owned()),
        "h264_vaapi" => Some(profile.to_owned()),
        _ => None,
    }
}

/// A hardware encoder whose B-frames the panel sets.
pub fn is_hardware(encoder: &str) -> bool {
    encoder.ends_with("_nvenc") || encoder.ends_with("_qsv") || encoder.ends_with("_vaapi")
}

/// `a = 1 : b=2 :` → `a=1:b=2`.
fn normalize_x264(params: &str) -> String {
    params
        .split(':')
        .map(|p| p.split('=').map(str::trim).collect::<Vec<_>>().join("="))
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(":")
}

/// Empty, or `key=value` pairs separated by `:`, each key in `X264_KEYS` once,
/// each value letters, digits, `.`, `,`, `-` (e.g. `bframes=0:subme=0`).
pub fn check_x264_params(params: &str) -> Result<(), String> {
    if params.is_empty() {
        return Ok(());
    }
    let pairs: Vec<&str> = params.split(':').collect();
    if pairs.len() > MAX_OPTIONS {
        return Err(format!("opcje x264: najwyżej {MAX_OPTIONS}"));
    }
    let mut seen = Vec::new();
    for pair in pairs {
        let Some((key, value)) = pair.split_once('=') else {
            return Err(format!("opcja x264 „{pair}”: zapis klucz=wartość"));
        };
        if !X264_KEYS.contains(&key) {
            return Err(format!("opcja x264 „{key}” niedozwolona; dozwolone: {}", X264_KEYS.join(", ")));
        }
        if seen.contains(&key) {
            return Err(format!("opcja x264 „{key}” podana dwa razy"));
        }
        let allowed = |c: char| c.is_ascii_alphanumeric() || ".,-".contains(c);
        if value.is_empty() || value.len() > MAX_VALUE || !value.chars().all(allowed) {
            return Err(format!("opcja x264 „{key}”: wartość z liter, cyfr, „.”, „,”, „-” (do {MAX_VALUE} znaków)"));
        }
        seen.push(key);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> Result<Encoder, String> {
        Encoder::from_value(&json::parse(text).unwrap())
    }

    #[test]
    fn checks_values() {
        let e =
            parsed(r#"{"preset":"veryfast","x264_params":" bframes = 0 : subme=0 :","nvenc_preset":"P5","qsv_preset":"slow","bframes":0}"#)
                .unwrap();
        assert_eq!((e.preset.as_str(), e.x264_params.as_str()), ("veryfast", "bframes=0:subme=0"));
        assert_eq!((e.nvenc_preset.as_str(), e.qsv_preset.as_str(), e.bframes.as_str()), ("p5", "slow", "0"));
        assert_eq!(parsed(r#"{"profile":"Main"}"#).unwrap().profile, "main");
        assert_eq!(parsed(r#"{"preset":null}"#).unwrap(), Encoder::default());
        for bad in [
            r#"{"preset":"turbo"}"#,
            r#"{"preset":3}"#,
            r#"{"profile":"high10"}"#,
            r#"{"nvenc_preset":"hq"}"#,
            r#"{"qsv_preset":"ultrafast"}"#,
            r#"{"bframes":"5"}"#,
            r#"{"bframes":"-1"}"#,
            r#"{"bframes":"+1"}"#,
            r#"{"bframes":"1 -y"}"#,
            r#"{"x264_params":"keyint=50"}"#,
            r#"{"x264_params":"dump-yuv=/tmp/x"}"#,
            r#"{"x264_params":"subme=0:subme=1"}"#,
            r#"{"x264_params":"subme"}"#,
            r#"{"x264_params":"subme=0 -y"}"#,
        ] {
            assert!(parsed(bad).is_err(), "{bad}");
        }
        assert!(parsed(r#"{"x264_params":"psy-rd=1.0,0.15:aq-strength=0.8"}"#).is_ok());
    }

    #[test]
    fn profiles_by_encoder() {
        assert_eq!(profile_for("h264_vaapi", "baseline").as_deref(), Some("constrained_baseline"));
        assert_eq!(profile_for("h264_nvenc", "high").as_deref(), Some("high"));
        assert_eq!(profile_for("hevc_nvenc", "high"), None);
        assert_eq!(profile_for("libx265", "main"), None);
        assert!(is_hardware("h264_qsv") && is_hardware("hevc_vaapi") && !is_hardware("libx264"));
    }

    #[test]
    fn reads_the_file_over_the_environment_key_by_key() {
        let dir = std::env::temp_dir().join(format!("vizrs-encoder-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE);
        let env = Encoder { preset: "veryfast".into(), x264_params: "subme=0".into(), bframes: "0".into(), ..Encoder::default() };
        assert_eq!(Encoder::read(&path, &env), None);
        assert_eq!(Encoder::in_force(Some(&path), &env), (env.clone(), false), "no file: the environment");
        // A file of the previous version: no hardware keys, so B-frames stay the environment's.
        std::fs::write(&path, r#"{"preset":"superfast","profile":"main","x264_params":""}"#).unwrap();
        let (used, from_file) = Encoder::in_force(Some(&path), &env);
        assert!(from_file);
        assert_eq!(used, Encoder { preset: "superfast".into(), profile: "main".into(), bframes: "0".into(), ..Encoder::default() });
        // Wrong values: empty, never passed on.
        std::fs::write(&path, r#"{"preset":"turbo","nvenc_preset":"hq","bframes":"9"}"#).unwrap();
        let wrong = Encoder::read(&path, &env).unwrap();
        assert_eq!((wrong.preset.as_str(), wrong.nvenc_preset.as_str(), wrong.bframes.as_str()), ("", "", ""));
        let saved = Encoder { nvenc_preset: "p5".into(), profile: "high".into(), ..Encoder::default() };
        std::fs::write(&path, saved.to_json()).unwrap();
        assert_eq!(Encoder::read(&path, &env), Some(saved), "every key written: the environment no longer counts");
        std::fs::write(&path, "nie json").unwrap();
        assert_eq!(Encoder::read(&path, &env), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
