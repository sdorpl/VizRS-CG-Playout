// Live programmes in ErsatzTV's schedule (DOKUMENTACJA §1, Audycje na żywo w
// ramówce): each entry of `live_shows` becomes a Remote Stream definition
// (`<etv_config>/vizrs-live/<file>.yml`) that a Remote Streams library of
// ErsatzTV scans. Its URL is the live slot of the channel in the sidecar
// (`/ingest?channel=N&slot=1`), its title and description go to the guide,
// its length is the length in the schedule. The overlay plays the studio there
// while it is on air and a standby picture while it is not (src/takeover.rs).
// Files are written when they change and never deleted: ErsatzTV would drop the
// item from the schedules it is in.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::config::LiveShow;
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
        LiveShow { channel: "1".into(), title: title.into(), seconds: 1800.0, plot: String::new(), file: String::new() }
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
}
