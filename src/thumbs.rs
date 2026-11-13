// Thumbnails of VizRS pages for VizRS Live (`GET /api/live/thumbs/<number>.png`):
// the panel shows them next to the graphics elements of ErsatzTV that are VizRS
// pages (`page-<number>.yml`). The native renderer draws them
// (`vizrs-render thumb`, VizRS DOKUMENTACJA §10.1): the page after its way in,
// cropped to where it draws, scaled down. The show comes from VizRS's HTTP API
// as for the page bounds (src/bounds.rs); a thumbnail is kept in
// `<etv_config>/vizrs-live/thumbs/<number>-<signature>.png` and made again only
// when the page's revision or its scene's changes. A page that draws nothing or
// cannot be drawn is remembered as `<number>-<signature>.none` (404). One
// renderer run at a time makes every thumbnail missing, so a panel asking for
// twenty pages at once starts it once.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::bounds;
use crate::json::{self, Value};

/// How long a fetched show stands for the next requests (a panel asks for many pages at once).
const SHOW_FRESH: Duration = Duration::from_secs(10);
/// Width of a thumbnail in pixels.
const WIDTH: u32 = 320;

pub struct Thumbs {
    dir: PathBuf,
    bin: String,
    output_url: String,
    token: String,
    /// The show last fetched: when, its text and its pages; one renderer run at a time.
    show: Mutex<Option<(Instant, String, Vec<bounds::Page>)>>,
}

/// A thumbnail: the PNG and its tag (the page's signature).
pub struct Thumb {
    pub png: Vec<u8>,
    pub tag: String,
}

/// FNV-1a of the page's signature, as a file name part and an ETag.
pub fn tag(signature: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in signature.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// A page number as VizRS has them: digits only (it is a file name here).
pub fn valid_number(number: &str) -> bool {
    !number.is_empty() && number.len() <= 12 && number.bytes().all(|b| b.is_ascii_digit())
}

/// One line of `vizrs-render thumb`: the page number and the file it wrote (`None`: empty or not drawn).
pub fn parse_line(line: &str) -> Option<(String, Option<String>)> {
    let v = json::parse(line).ok()?;
    let number = match v.get("number")? {
        Value::String(s) => s.clone(),
        Value::Number(n) => format!("{n:.0}"),
        _ => return None,
    };
    let file = match v.get("file") {
        Some(Value::String(f)) if v.get("error").is_none() => Some(f.clone()),
        _ => None,
    };
    Some((number, file))
}

impl Thumbs {
    pub fn new(etv_config: &str, bin: String, output_url: &str, token: &str) -> Thumbs {
        let dir = if etv_config.is_empty() {
            std::env::temp_dir().join("vizrs-cg-thumbs")
        } else {
            crate::live_shows::folder(etv_config).join("thumbs")
        };
        Thumbs { dir, bin, output_url: output_url.to_owned(), token: token.to_owned(), show: Mutex::new(None) }
    }

    fn file(&self, number: &str, tag: &str, ext: &str) -> PathBuf {
        self.dir.join(format!("{number}-{tag}.{ext}"))
    }

    /// The thumbnail of page `number`; `Ok(None)` – no such page, or it draws nothing.
    pub fn get(&self, number: &str) -> Result<Option<Thumb>, String> {
        if !valid_number(number) {
            return Ok(None);
        }
        let mut show = self.show.lock().unwrap_or_else(|e| e.into_inner());
        if show.as_ref().is_none_or(|(at, _, _)| at.elapsed() > SHOW_FRESH) {
            let text = bounds::fetch_show(&self.output_url, &self.token)?;
            let pages = bounds::pages(&json::parse(&text).map_err(|_| "the show from VizRS is not JSON".to_owned())?);
            *show = Some((Instant::now(), text, pages));
        }
        let (_, text, pages) = show.as_ref().expect("fetched above");
        let Some(page) = pages.iter().find(|p| p.number == number) else {
            return Ok(None);
        };
        let wanted = tag(&page.signature);
        if !self.known(number, &wanted) {
            // every page without a thumbnail of its current revision, in one run
            let todo: Vec<(&str, String)> = pages
                .iter()
                .filter(|p| valid_number(&p.number))
                .map(|p| (p.number.as_str(), tag(&p.signature)))
                .filter(|(n, t)| !self.known(n, t))
                .collect();
            self.make(text, &todo)?;
        }
        match fs::read(self.file(number, &wanted, "png")) {
            Ok(png) => Ok(Some(Thumb { png, tag: wanted })),
            Err(_) => Ok(None),
        }
    }

    fn known(&self, number: &str, tag: &str) -> bool {
        self.file(number, tag, "png").exists() || self.file(number, tag, "none").exists()
    }

    /// Runs the renderer for `todo` (number, tag) and files what it made; older files of those pages go.
    fn make(&self, show: &str, todo: &[(&str, String)]) -> Result<(), String> {
        let work = self.dir.join(format!("work-{}", std::process::id()));
        fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
        let result = self.run(show, &work, todo);
        let _ = fs::remove_dir_all(&work);
        result
    }

    fn run(&self, show: &str, work: &Path, todo: &[(&str, String)]) -> Result<(), String> {
        let show_file = work.join("show.json");
        fs::write(&show_file, show).map_err(|e| format!("{}: {e}", show_file.display()))?;
        let mut cmd = Command::new(&self.bin);
        cmd.arg("thumb").arg("--show").arg(&show_file).arg("--dir").arg(work).args([
            "--url",
            &self.output_url,
            "--width",
            &WIDTH.to_string(),
        ]);
        for (number, _) in todo {
            cmd.args(["--page", number]);
        }
        let started = Instant::now();
        let out = cmd.output().map_err(|e| format!("{}: {e}", self.bin))?;
        if !out.status.success() {
            return Err(format!("{} thumb: {}", self.bin, String::from_utf8_lossy(&out.stderr).trim()));
        }
        let made: Vec<(String, Option<String>)> = String::from_utf8_lossy(&out.stdout).lines().filter_map(parse_line).collect();
        let mut drawn = 0;
        for (number, tag) in todo {
            self.forget(number);
            match made.iter().find(|(n, _)| n == number).and_then(|(_, f)| f.as_deref()) {
                Some(file) if fs::rename(file, self.file(number, tag, "png")).is_ok() => drawn += 1,
                _ => {
                    let _ = fs::write(self.file(number, tag, "none"), b"");
                }
            }
        }
        crate::info!("thumbnails: {} pages from VizRS in {:.1} s ({drawn} drawn)", todo.len(), started.elapsed().as_secs_f64());
        Ok(())
    }

    /// Removes the files of page `number` (older revisions).
    fn forget(&self, number: &str) {
        let prefix = format!("{number}-");
        for entry in fs::read_dir(&self.dir).into_iter().flatten().flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(&prefix) && (name.ends_with(".png") || name.ends_with(".none")) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_are_stable_and_differ() {
        assert_eq!(tag("2|s1|3"), tag("2|s1|3"));
        assert_ne!(tag("2|s1|3"), tag("2|s1|4"));
        assert_eq!(tag("").len(), 16);
    }

    #[test]
    fn page_numbers_are_digits() {
        assert!(valid_number("1001"));
        assert!(!valid_number(""));
        assert!(!valid_number("../1"));
        assert!(!valid_number("1001.png"));
        assert!(!valid_number("1234567890123"));
    }

    #[test]
    fn renderer_lines() {
        assert_eq!(
            parse_line(r#"{"number":"1001","file":"/w/1001.png","crop":[0,0,9,9],"issues":[]}"#),
            Some(("1001".into(), Some("/w/1001.png".into())))
        );
        assert_eq!(parse_line(r#"{"number":"7","empty":true,"issues":[]}"#), Some(("7".into(), None)));
        assert_eq!(parse_line(r#"{"number":"8","error":"brak sceny x"}"#), Some(("8".into(), None)));
        assert_eq!(parse_line("nonsense"), None);
    }
}
