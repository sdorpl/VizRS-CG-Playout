// Synchronization of VizRS pages with ErsatzTV script elements (DOKUMENTACJA §3):
// one .yml per page, kept up to date by every vizrs-etv-client session and by
// `virtrs-cg sync`:
//   name: "Ticker (7303)"
//   args:
//     - http://cg:7700/etv?channel=1&page=7303
//
// ErsatzTV drops an element whose file is missing at a refresh, and with it the
// element's places in schedules, decos and playout items. So files are replaced
// atomically and never deleted here: a page gone from VizRS (also for a while,
// after a show switch) only gets a mark in the element's name.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::json::{self, Value};
use crate::net::{self, Url, error};

/// Marks a file as ours: a page file without this line is left alone.
const GENERATED: &str = "# Generated from VizRS CG System page";
/// Before the name of an element whose page is not in VizRS.
const GONE: &str = "[brak w VizRS] ";
/// Inside the ErsatzTV configuration folder.
const SCRIPT_TEMPLATES: &str = "templates/graphics-elements/script";
/// What is specific to one element in its URL; the rest holds for all of the channel's.
const ELEMENT_PARAMS: [&str; 5] = ["page", "take", "cmd", "command", "fields"];

/// A page fetched from VizRS.
#[derive(Clone, Debug, PartialEq)]
pub struct PageInfo {
    pub number: u64,
    pub name: String,
}

/// How to treat the existing files.
#[derive(Clone, Copy, Debug)]
pub struct SyncOptions<'a> {
    /// URL of an element without the page, e.g. `http://cg:7700/etv?channel=1`.
    pub element_url: &'a str,
    /// Generated files get `element_url`; otherwise they keep their URL and only their name follows VizRS.
    pub rewrite: bool,
    /// Generated files of pages gone from VizRS are deleted, with their places in ErsatzTV.
    pub prune: bool,
}

#[derive(Debug, Default, PartialEq)]
pub struct SyncReport {
    pub created: usize,
    pub updated: usize,
    pub gone: usize,
    pub removed: usize,
}

impl SyncReport {
    pub fn changed(&self) -> bool {
        *self != SyncReport::default()
    }
}

impl fmt::Display for SyncReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} created, {} updated, {} gone from VizRS, {} removed", self.created, self.updated, self.gone, self.removed)
    }
}

/// A `.yml` of the folder that shows a VizRS page.
struct Element {
    path: PathBuf,
    text: String,
    page: u64,
    generated: bool,
}

/// The ErsatzTV folder of script elements, with the configuration folder of the ErsatzTV process.
pub fn templates_dir(config_folder: Option<&str>) -> PathBuf {
    Path::new(config_folder.filter(|f| !f.is_empty()).unwrap_or("/config")).join(SCRIPT_TEMPLATES)
}

/// The base address of VizRS from the address of its `/output` page.
pub fn http_url_of(output_url: &str) -> Option<String> {
    let url = Url::split(output_url)?;
    Some(format!("{}://{}", url.scheme, url.netloc))
}

/// The address of a sidecar's page list, from the URL of an element.
pub fn pages_url(element_url: &str) -> Option<String> {
    let url = Url::split(element_url)?;
    let channel = channel_of(element_url).map_or(String::new(), |channel| format!("?channel={channel}"));
    Some(format!("{}://{}{}/pages{channel}", url.scheme, url.netloc, url.path))
}

/// The `channel=` an element's URL names, still encoded.
pub fn channel_of(url: &str) -> Option<&str> {
    url.split_once('?')?.1.split('&').find_map(|p| p.strip_prefix("channel="))
}

/// The URL in an element's text.
fn url_of(text: &str) -> Option<&str> {
    text.lines().find_map(|line| line.trim().strip_prefix("- "))
}

/// The URL of a page's element, from `base`: another element of the channel,
/// whose own page or commands do not carry over.
pub fn channel_url(base: &str) -> String {
    let (path, query) = base.split_once('?').unwrap_or((base, ""));
    let shared: Vec<&str> =
        query.split('&').filter(|p| !p.is_empty() && !ELEMENT_PARAMS.contains(&p.split('=').next().unwrap_or(""))).collect();
    if shared.is_empty() { path.to_owned() } else { format!("{path}?{}", shared.join("&")) }
}

fn page_url(element_url: &str, number: u64) -> String {
    let separator = if element_url.contains('?') { '&' } else { '?' };
    format!("{element_url}{separator}page={number}")
}

/// A double-quoted YAML string. ErsatzTV runs the whole file through Scriban
/// first, so a brace of the name is escaped for YAML too.
fn yaml_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '{' => out.push_str("\\x7b"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn name_line(page: &PageInfo) -> String {
    format!("name: {}", yaml_string(&format!("{} ({})", page.name, page.number)))
}

/// Generates the YAML content of an ErsatzTV script element for a VizRS page.
pub fn generate_template_yaml(page: &PageInfo, element_url: &str) -> String {
    format!(
        "{GENERATED} {number}\n\
         {name}\n\
         command: /vizrs/vizrs-etv-client\n\
         args:\n\
         \x20 - {url}\n\
         z_index: 100\n\
         epg_entries: 3\n\
         start_seconds: 0\n\
         duration_seconds: 172800\n\
         pixel_format: rgba\n\
         format: packet\n",
        number = page.number,
        name = name_line(page),
        url = page_url(element_url, page.number),
    )
}

/// Parses pages from VizRS show JSON text.
pub fn parse_pages(json_text: &str) -> Result<Vec<PageInfo>, String> {
    let root = json::parse(json_text).map_err(|e| format!("invalid JSON: {e}"))?;
    let pages_val = root.get("pages").and_then(Value::as_array).ok_or_else(|| "missing or non-array 'pages' field".to_owned())?;

    let mut pages = Vec::new();
    for p in pages_val {
        let number = match p.get("number") {
            Some(Value::Number(n)) => *n as u64,
            Some(Value::String(s)) => s.parse().unwrap_or(0),
            _ => continue,
        };
        if number == 0 {
            continue;
        }
        let name = p.get("name").and_then(Value::as_str).unwrap_or("Strona").trim().to_owned();
        pages.push(PageInfo { number, name });
    }
    Ok(pages)
}

/// The pages for the sidecar's `/etv/pages`, in the format of `parse_pages`.
pub fn pages_json(pages: &[PageInfo]) -> String {
    let pages = pages.iter().map(|p| json::object([("number", p.number.into()), ("name", p.name.as_str().into())]));
    json::object([("pages", Value::Array(pages.collect()))]).to_string()
}

/// Queries VizRS HTTP API for the active show JSON.
pub fn fetch_show(api_url: &str, token: &str) -> io::Result<String> {
    let url = if api_url.ends_with("/api/show") { api_url.to_owned() } else { format!("{}/api/show", api_url.trim_end_matches('/')) };
    let headers = if token.is_empty() { String::new() } else { format!("X-Token: {token}\r\n") };
    net::get(&url, &headers)
}

/// The page an element's URL shows.
fn page_of(text: &str) -> Option<u64> {
    let url = url_of(text)?;
    let (_, query) = url.split_once('?')?;
    query.split('&').find_map(|p| p.strip_prefix("page=").or_else(|| p.strip_prefix("take=")))?.parse().ok()
}

fn read_elements(dir: &Path) -> io::Result<Vec<Element>> {
    let mut elements = Vec::new();
    for entry in fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "yml") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else { continue };
        if let Some(page) = page_of(&text) {
            let generated = text.starts_with(GENERATED);
            elements.push(Element { path, text, page, generated });
        }
    }
    Ok(elements)
}

fn with_name(text: &str, name: impl FnOnce(&str) -> String) -> String {
    let mut name = Some(name);
    let mut out = String::with_capacity(text.len() + GONE.len());
    for line in text.lines() {
        match name.take_if(|_| line.starts_with("name:")) {
            Some(name) => out.push_str(&name(line)),
            None => out.push_str(line),
        }
        out.push('\n');
    }
    out
}

/// A reader of the folder sees the old file or the new one, never a part or none.
fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    let file = path.file_name().and_then(|n| n.to_str()).ok_or_else(|| error(format!("bad path {}", path.display())))?;
    let temporary = path.with_file_name(format!(".{file}.{}.tmp", std::process::id()));
    fs::write(&temporary, text)?;
    fs::rename(&temporary, path).inspect_err(|_| {
        let _ = fs::remove_file(&temporary);
    })
}

/// Brings the elements in `dir` in line with `pages`. A hand-written file
/// (no `GENERATED` line) of a page stands for it and stays as it is.
pub fn sync_templates(pages: &[PageInfo], dir: &Path, options: SyncOptions) -> io::Result<SyncReport> {
    fs::create_dir_all(dir)?;
    let all = read_elements(dir)?;
    // Each channel keeps its own elements: another channel's page list knows nothing of them.
    let (elements, others): (Vec<Element>, Vec<Element>) =
        all.into_iter().partition(|e| options.rewrite || url_of(&e.text).and_then(channel_of) == channel_of(options.element_url));
    let mut report = SyncReport::default();

    for page in pages {
        if !elements.iter().any(|e| e.page == page.number) {
            // The plain name may belong to another channel.
            let file = match channel_of(options.element_url).filter(|_| others.iter().any(|e| e.page == page.number)) {
                Some(channel) => format!("page-{}-ch{channel}.yml", page.number),
                None => format!("page-{}.yml", page.number),
            };
            write_atomic(&dir.join(file), &generate_template_yaml(page, options.element_url))?;
            report.created += 1;
        }
        for element in elements.iter().filter(|e| e.page == page.number && e.generated) {
            let text = if options.rewrite {
                generate_template_yaml(page, options.element_url)
            } else {
                with_name(&element.text, |_| name_line(page))
            };
            if text != element.text {
                write_atomic(&element.path, &text)?;
                report.updated += 1;
            }
        }
    }

    for element in elements.iter().filter(|e| e.generated && !pages.iter().any(|p| p.number == e.page)) {
        // An empty list says more about VizRS than about the pages.
        if options.prune && !pages.is_empty() {
            fs::remove_file(&element.path)?;
            report.removed += 1;
            continue;
        }
        let text = with_name(&element.text, |line| match line.strip_prefix("name: \"") {
            Some(name) if !name.starts_with(GONE) => format!("name: \"{GONE}{name}"),
            _ => line.to_owned(),
        });
        if text != element.text {
            write_atomic(&element.path, &text)?;
            report.gone += 1;
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    const URL: &str = "http://cg:7700/etv?channel=4";
    const KEEP: SyncOptions = SyncOptions { element_url: URL, rewrite: false, prune: false };

    fn page(number: u64, name: &str) -> PageInfo {
        PageInfo { number, name: name.into() }
    }

    /// An empty folder, deleted with the value.
    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Folder {
            let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let dir = std::env::temp_dir().join(format!("virtrs-sync-{name}-{}-{unique}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            Folder(dir)
        }

        fn read(&self, file: &str) -> String {
            fs::read_to_string(self.0.join(file)).unwrap()
        }

        fn files(&self) -> Vec<String> {
            let mut files: Vec<_> = fs::read_dir(&self.0).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
            files.sort();
            files
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn format_template_matches_user_requirement() {
        let yaml = generate_template_yaml(&page(7303, "Ticker"), "http://cg:7700/etv?channel=1");
        assert!(yaml.contains("name: \"Ticker (7303)\""));
        assert!(yaml.contains("args:\n  - http://cg:7700/etv?channel=1&page=7303"));
    }

    #[test]
    fn names_cannot_break_the_element() {
        let yaml = generate_template_yaml(&page(1, "A \"b\" \\ {{ c }}\nd"), URL);
        assert!(yaml.contains(r#"name: "A \"b\" \\ \x7b\x7b c }} d (1)""#), "{yaml}");
    }

    #[test]
    fn parse_pages_from_show_json() {
        let json = r#"{
            "pages": [
                { "number": 1001, "name": "Belka – budżet" },
                { "number": 7303, "name": "Ticker pilny" }
            ]
        }"#;
        let pages = parse_pages(json).unwrap();
        assert_eq!(pages, vec![page(1001, "Belka – budżet"), page(7303, "Ticker pilny")]);
        assert_eq!(parse_pages(&pages_json(&pages)).unwrap(), pages);
        assert!(parse_pages("{}").is_err());
    }

    #[test]
    fn channels_keep_their_own_elements() {
        let folder = Folder::new("channels");
        let sync = |url: &str, pages: &[PageInfo]| {
            sync_templates(pages, &folder.0, SyncOptions { element_url: url, rewrite: false, prune: false }).unwrap()
        };
        sync("http://cg:7700/etv?channel=1", &[page(1, "Jeden")]);
        sync("http://cg:7700/etv?channel=2", &[page(1, "Dwa"), page(2, "Inna")]);
        // Channel 2's list changes nothing of channel 1's element.
        assert!(folder.read("page-1.yml").contains("Jeden"));
        assert!(folder.read("page-1.yml").contains("channel=1&page=1"));
        assert!(folder.read("page-1-ch2.yml").contains("channel=2&page=1"));
        assert!(folder.read("page-2.yml").contains("channel=2&page=2"));
    }

    #[test]
    fn urls_of_the_channel() {
        assert_eq!(http_url_of("http://vizrs:8088/output?token=x").unwrap(), "http://vizrs:8088");
        assert_eq!(pages_url("http://cg:7700/etv?channel=4&page=1").unwrap(), "http://cg:7700/etv/pages?channel=4");
        assert_eq!(pages_url("http://cg:7700/etv?page=1").unwrap(), "http://cg:7700/etv/pages");
        // What holds for the channel stays, the element's own commands do not.
        let element = "http://cg:7700/etv?channel=4&render=720p&area=10x10+0+0&page=3001&fields=a=b";
        assert_eq!(channel_url(element), "http://cg:7700/etv?channel=4&render=720p&area=10x10+0+0");
        assert_eq!(channel_url("http://cg:7700/etv?take=1"), "http://cg:7700/etv");
        assert_eq!(page_url("http://cg:7700/etv", 1), "http://cg:7700/etv?page=1");
    }

    #[test]
    fn pages_follow_vizrs_and_keep_their_url() {
        let folder = Folder::new("follow");
        let report = sync_templates(&[page(3001, "Ticker"), page(4001, "Tytuł")], &folder.0, KEEP).unwrap();
        assert_eq!(report, SyncReport { created: 2, ..SyncReport::default() });
        assert_eq!(folder.files(), ["page-3001.yml", "page-4001.yml"]);

        // Nothing changed: nothing written.
        assert!(!sync_templates(&[page(3001, "Ticker"), page(4001, "Tytuł")], &folder.0, KEEP).unwrap().changed());

        // The same client after a rename: a new name, the same URL.
        let report = sync_templates(&[page(3001, "Pasek"), page(4001, "Tytuł")], &folder.0, KEEP).unwrap();
        assert_eq!(report, SyncReport { updated: 1, ..SyncReport::default() });
        let text = folder.read("page-3001.yml");
        assert!(text.contains("name: \"Pasek (3001)\"") && text.contains("channel=4&page=3001"), "{text}");
        let other = SyncOptions { element_url: "http://cg:7700/etv?channel=9", ..KEEP };

        // Asked for: the URL changes too.
        let rewrite = SyncOptions { rewrite: true, ..other };
        sync_templates(&[page(3001, "Pasek"), page(4001, "Tytuł")], &folder.0, rewrite).unwrap();
        assert!(folder.read("page-3001.yml").contains("channel=9&page=3001"));
    }

    #[test]
    fn a_page_gone_keeps_its_element() {
        let folder = Folder::new("gone");
        sync_templates(&[page(3001, "Ticker"), page(4001, "Tytuł")], &folder.0, KEEP).unwrap();

        let report = sync_templates(&[page(4001, "Tytuł")], &folder.0, KEEP).unwrap();
        assert_eq!(report, SyncReport { gone: 1, ..SyncReport::default() });
        assert!(folder.read("page-3001.yml").contains("name: \"[brak w VizRS] Ticker (3001)\""));
        assert!(!sync_templates(&[page(4001, "Tytuł")], &folder.0, KEEP).unwrap().changed());

        // Back after a show switch: the mark goes.
        let report = sync_templates(&[page(3001, "Ticker"), page(4001, "Tytuł")], &folder.0, KEEP).unwrap();
        assert_eq!(report, SyncReport { updated: 1, ..SyncReport::default() });
        assert!(folder.read("page-3001.yml").contains("name: \"Ticker (3001)\""));

        // An empty show deletes nothing, even when asked to.
        let prune = SyncOptions { prune: true, ..KEEP };
        sync_templates(&[], &folder.0, prune).unwrap();
        assert_eq!(folder.files(), ["page-3001.yml", "page-4001.yml"]);
        let report = sync_templates(&[page(4001, "Tytuł")], &folder.0, prune).unwrap();
        assert_eq!(report.removed, 1);
        assert_eq!(folder.files(), ["page-4001.yml"]);
    }

    #[test]
    fn hand_written_elements_stay() {
        let folder = Folder::new("hand");
        let hand = "name: Mój ticker\nargs:\n  - http://cg:7700/etv?channel=4&area=1x1+0+0&page=3003\n";
        fs::write(folder.0.join("ticker.yml"), hand).unwrap();
        let other = "name: Inny\nargs:\n  - http://cg:7700/etv?channel=4&take=5001\n";
        fs::write(folder.0.join("page-5001.yml"), other).unwrap();
        let general = "name: VizRS\nargs:\n  - http://cg:7700/etv?channel=4\n";
        fs::write(folder.0.join("vizrs.yml"), general).unwrap();

        sync_templates(&[page(3003, "Ticker wipe")], &folder.0, SyncOptions { prune: true, rewrite: true, ..KEEP }).unwrap();
        assert_eq!(folder.files(), ["page-5001.yml", "ticker.yml", "vizrs.yml"], "the page has its element already");
        assert_eq!(folder.read("ticker.yml"), hand);
        assert_eq!(folder.read("page-5001.yml"), other);
    }
}
