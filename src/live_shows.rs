// Live programmes in ErsatzTV's schedule (DOKUMENTACJA §1, Audycje na żywo w
// ramówce): each entry of `live_shows` becomes a Remote Stream definition
// (`<etv_config>/vizrs-live/<file>.yml`) that a Remote Streams library of
// ErsatzTV scans. Its URL is the live slot of the channel in the sidecar
// (`/ingest?channel=N&slot=1`), its title and description go to the guide,
// its length is the length in the schedule. The overlay plays the studio there
// while it is on air and a standby picture while it is not (src/takeover.rs).
// Files are written when they change and never deleted by the sync: ErsatzTV
// would drop the item from the schedules it is in (the Live Panel deletes the
// file of a programme the operator removes).
//
// The list itself lives in `<etv_config>/vizrs-live/shows.json`, which the Live
// Panel edits (src/panel.rs); `live_shows` / `VIZRS_LIVE_SHOWS` only fills it
// the first time, when there is no such file yet.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use crate::config::{self, LiveShow};
use crate::json::{self, Value};
use crate::overlay::quote_plus;
use crate::sync::write_atomic;

/// The folder of the definitions in ErsatzTV's configuration folder (`/config/vizrs-live` in ErsatzTV).
pub fn folder(etv_config: &str) -> PathBuf {
    Path::new(etv_config).join("vizrs-live")
}

/// A file name from a title: Polish letters without marks, the rest `-`.
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.to_lowercase().chars() {
        let c = match c {
            'ą' => 'a',
            'ć' => 'c',
            'ę' => 'e',
            'ł' => 'l',
            'ń' => 'n',
            'ó' => 'o',
            'ś' => 's',
            'ź' | 'ż' => 'z',
            c => c,
        };
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-');
    out.chars().take(60).collect::<String>().trim_end_matches('-').to_owned()
}

fn quoted(text: &str) -> String {
    let escaped: String = text
        .chars()
        .map(|c| match c {
            '"' => "\\\"".to_owned(),
            '\\' => "\\\\".to_owned(),
            '\n' => "\\n".to_owned(),
            '\r' | '\t' => " ".to_owned(),
            c => c.to_string(),
        })
        .collect();
    format!("\"{escaped}\"")
}

/// `HH:MM:SS` of ErsatzTV (TimeSpan).
pub fn clock(seconds: f64) -> String {
    let s = seconds.round() as u64;
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// File name of a programme, without the folder.
pub fn file_name(show: &LiveShow) -> String {
    let name = if show.file.trim().is_empty() { format!("{}-{}", slug(&show.channel), slug(&show.title)) } else { slug(&show.file) };
    format!("{name}.yml")
}

/// The Remote Stream definition of a programme; `sidecar` is the sidecar as ErsatzTV reaches it.
pub fn definition(show: &LiveShow, sidecar: &str) -> String {
    let url = format!("{sidecar}/ingest?channel={}&slot=1", quote_plus(&show.channel));
    let mut text = format!(
        "# VizRS CG Playout: live programme of channel {} (sidecar live_shows); changes here are overwritten.\n\
         title: {}\n",
        show.channel,
        quoted(&show.title)
    );
    if !show.plot.is_empty() {
        text.push_str(&format!("plot: {}\n", quoted(&show.plot)));
    }
    text.push_str(&format!("url: {}\nis_live: true\nduration: \"{}\"\n", quoted(&url), clock(show.seconds)));
    text
}

/// Writes the definitions that changed; how many.
pub fn write(shows: &[LiveShow], folder: &Path, sidecar: &str) -> io::Result<usize> {
    if shows.is_empty() {
        return Ok(0);
    }
    fs::create_dir_all(folder)?;
    let mut changed = 0;
    for show in shows {
        let path = folder.join(file_name(show));
        let text = definition(show, sidecar);
        if fs::read_to_string(&path).is_ok_and(|old| old == text) {
            continue;
        }
        write_atomic(&path, &text)?;
        changed += 1;
    }
    Ok(changed)
}

static STORE: Mutex<()> = Mutex::new(());

/// Held while `shows.json` and the definitions change: the periodic sync must
/// not write back the file of a programme the panel has just removed.
pub fn lock() -> MutexGuard<'static, ()> {
    STORE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Most programmes in `shows.json`.
pub const MAX_SHOWS: usize = 200;
const MAX_TITLE: usize = 200;
const MAX_PLOT: usize = 2000;
const MAX_CHANNEL: usize = 20;

/// `shows.json` in the folder of the definitions.
pub fn store_path(etv_config: &str) -> PathBuf {
    folder(etv_config).join("shows.json")
}

/// The file stem of a programme, its id in the panel.
pub fn id(show: &LiveShow) -> String {
    file_name(show).trim_end_matches(".yml").to_owned()
}

/// A programme as the Live Panel takes it: limits of length and characters
/// (data from a browser is hostile, as in VizRS `validate.rs`).
pub fn validate(show: &LiveShow) -> Result<(), String> {
    let channel = show.channel.trim();
    if channel.is_empty() || channel.len() > MAX_CHANNEL || !channel.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-') {
        return Err("kanał: numer kanału ErsatzTV (cyfry, kropka)".into());
    }
    let title = show.title.trim();
    if title.is_empty() || title.chars().count() > MAX_TITLE || title.chars().any(char::is_control) {
        return Err(format!("tytuł: 1–{MAX_TITLE} znaków, bez znaków sterujących"));
    }
    if show.plot.chars().count() > MAX_PLOT || show.plot.chars().any(|c| c.is_control() && c != '\n') {
        return Err(format!("opis: najwyżej {MAX_PLOT} znaków"));
    }
    if !(1.0..86_400.0).contains(&show.seconds) {
        return Err("długość: od 00:00:01 do 23:59:59".into());
    }
    if slug(&show.title).is_empty() {
        return Err("tytuł: potrzebna choć jedna litera albo cyfra".into());
    }
    Ok(())
}

/// The list as `shows.json`.
pub fn to_json(shows: &[LiveShow]) -> String {
    let items = shows.iter().map(|show| {
        json::object([
            ("channel", show.channel.as_str().into()),
            ("title", show.title.as_str().into()),
            ("duration", clock(show.seconds).into()),
            ("plot", show.plot.as_str().into()),
            ("file", id(show).into()),
            ("collection", show.collection.map_or(Value::Null, Value::from)),
        ])
    });
    let mut text = json::object([("shows", Value::Array(items.collect()))]).to_string();
    text.push('\n');
    text
}

/// `shows.json` read back; every programme gets its file name fixed.
pub fn from_json(text: &str) -> Result<Vec<LiveShow>, String> {
    let value = json::parse(text).map_err(|e| e.to_string())?;
    let mut shows = config::live_shows(value.get("shows").ok_or("shows.json: no \"shows\"")?)?;
    for show in &mut shows {
        show.file = id(show);
    }
    Ok(shows)
}

/// The programmes of `shows.json`; without the file, the configured ones
/// (`live_shows`, `VIZRS_LIVE_SHOWS`) become the file, once.
pub fn load(etv_config: &str, configured: &[LiveShow]) -> io::Result<Vec<LiveShow>> {
    let path = store_path(etv_config);
    match fs::read_to_string(&path) {
        Ok(text) => from_json(&text).map_err(|e| io::Error::other(format!("{}: {e}", path.display()))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            if configured.is_empty() {
                return Ok(Vec::new());
            }
            let shows: Vec<LiveShow> = configured.iter().map(|s| LiveShow { file: id(s), ..s.clone() }).collect();
            save(etv_config, &shows)?;
            crate::info!("{} live programmes of VIZRS_LIVE_SHOWS moved to {}; the Live Panel edits them now", shows.len(), path.display());
            Ok(shows)
        }
        Err(e) => Err(e),
    }
}

pub fn save(etv_config: &str, shows: &[LiveShow]) -> io::Result<()> {
    fs::create_dir_all(folder(etv_config))?;
    write_atomic(&store_path(etv_config), &to_json(shows))
}

/// `scheme://host:port` of a URL (the sidecar from the address of the graphics element).
pub fn origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let host = rest.split(['/', '?', '#']).next().filter(|h| !h.is_empty())?;
    Some(format!("{scheme}://{host}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn show(title: &str) -> LiveShow {
        LiveShow { channel: "1".into(), title: title.into(), seconds: 1800.0, ..LiveShow::default() }
    }

    #[test]
    fn names_and_definitions() {
        assert_eq!(slug("Wiadomości – wydanie główne!"), "wiadomosci-wydanie-glowne");
        assert_eq!(file_name(&show("Rozmowa dnia")), "1-rozmowa-dnia.yml");
        assert_eq!(file_name(&LiveShow { file: "studio".into(), ..show("X") }), "studio.yml");
        assert_eq!(clock(5400.0), "01:30:00");
        let text = definition(&LiveShow { plot: "Prosto ze \"studia\"".into(), ..show("Wiadomości") }, "http://cg:7700");
        assert_eq!(
            text,
            "# VizRS CG Playout: live programme of channel 1 (sidecar live_shows); changes here are overwritten.\n\
             title: \"Wiadomości\"\nplot: \"Prosto ze \\\"studia\\\"\"\nurl: \"http://cg:7700/ingest?channel=1&slot=1\"\nis_live: true\nduration: \"00:30:00\"\n"
        );
        assert_eq!(origin("http://cg:7700/etv?channel=1").as_deref(), Some("http://cg:7700"));
        assert_eq!(origin("cg"), None);
    }

    #[test]
    fn written_when_changed() {
        let dir = std::env::temp_dir().join(format!("vizrs-live-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let shows = [show("A"), show("B")];
        assert_eq!(write(&shows, &dir, "http://cg:7700").unwrap(), 2);
        assert_eq!(write(&shows, &dir, "http://cg:7700").unwrap(), 0);
        assert_eq!(write(&[LiveShow { seconds: 60.0, ..show("A") }], &dir, "http://cg:7700").unwrap(), 1);
        assert!(fs::read_to_string(dir.join("1-a.yml")).unwrap().contains("duration: \"00:01:00\""));
        assert_eq!(write(&[], &dir.join("none"), "x").unwrap(), 0);
        assert!(!dir.join("none").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn store_moves_the_configured_shows_once() {
        let dir = std::env::temp_dir().join(format!("vizrs-store-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let config = dir.to_str().unwrap();
        assert_eq!(load(config, &[]).unwrap(), []);
        assert!(!store_path(config).exists());
        let shows = load(config, &[show("Wiadomości")]).unwrap();
        assert_eq!(shows[0].file, "1-wiadomosci");
        // The file now wins over the configuration.
        assert_eq!(load(config, &[show("Inna")]).unwrap(), shows);
        let edited = [LiveShow { collection: Some(12), plot: "a\nb".into(), ..shows[0].clone() }];
        save(config, &edited).unwrap();
        assert_eq!(load(config, &[]).unwrap(), edited);
        fs::write(store_path(config), "{").unwrap();
        assert!(load(config, &[]).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn validation() {
        assert_eq!(validate(&show("Wiadomości")), Ok(()));
        assert!(validate(&LiveShow { channel: "1; rm".into(), ..show("A") }).is_err());
        assert!(validate(&LiveShow { title: " ".into(), ..show("A") }).is_err());
        assert!(validate(&LiveShow { title: "x".repeat(201), ..show("A") }).is_err());
        assert!(validate(&LiveShow { title: "a\u{7}".into(), ..show("A") }).is_err());
        assert!(validate(&LiveShow { seconds: 86_400.0, ..show("A") }).is_err());
        assert!(validate(&LiveShow { seconds: 0.5, ..show("A") }).is_err());
        assert!(validate(&LiveShow { channel: "2.5".into(), ..show("„!”") }).is_err());
        assert_eq!(validate(&LiveShow { channel: "2.5".into(), ..show("Ż") }), Ok(()));
    }
}
