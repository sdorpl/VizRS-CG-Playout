// The sidecar keeps ErsatzTV in line with itself (DOKUMENTACJA §4): at the start
// it puts the ErsatzTV programs and the graphics element into the shared
// volumes, so they always match this version; then, every `sync_seconds`, it
// gives ErsatzTV's channels their antennas and names in VizRS and writes the
// page elements. Failures only reach the log: graphics never depend on this.

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::thread;
use std::time::Duration;

use crate::config::Config;
use crate::json::{self, Value};
use crate::net::{self, error};
use crate::sync::{self, SyncOptions};

/// Replaces the programs of `dir` one by one: a running ffmpeg keeps its file.
pub fn install_clients(source: &Path, dir: &Path) -> io::Result<usize> {
    let mut changed = 0;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let file = entry.file_name();
        let bytes = fs::read(entry.path())?;
        let target = dir.join(&file);
        if fs::read(&target).is_ok_and(|old| old == bytes) {
            continue;
        }
        let temporary = dir.join(format!(".{}.tmp", file.to_string_lossy()));
        fs::write(&temporary, &bytes)?;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o755))?;
        fs::rename(&temporary, &target)?;
        changed += 1;
    }
    Ok(changed)
}

/// The URL of an element's text, as a list item of its `args`.
fn element_url(text: &str) -> Option<&str> {
    text.lines().find_map(|line| line.strip_prefix("  - ")).filter(|url| url.starts_with("http"))
}

/// The general element, new version; its address is per instance and stays.
pub fn install_element(source: &Path, elements: &Path) -> io::Result<()> {
    let new = fs::read_to_string(source)?;
    let target = elements.join("vizrs.yml");
    let old = fs::read_to_string(&target).unwrap_or_default();
    let text = match (element_url(&old), element_url(&new)) {
        (Some(url), Some(default)) => new.replacen(default, url, 1),
        _ => new,
    };
    if text != old {
        fs::create_dir_all(elements)?;
        sync::write_atomic(&target, &text)?;
    }
    Ok(())
}

/// Numbers and names of ErsatzTV's channels.
fn etv_channels(config: &Config, folder: &Path) -> io::Result<Vec<(String, String)>> {
    let secrets = json::parse(&fs::read_to_string(folder.join("api-secrets.json"))?).map_err(|e| error(e.to_string()))?;
    let key = secrets.get("ApiKey").and_then(Value::as_str).unwrap_or("");
    let reply = net::get(&format!("{}/api/channels", config.etv_url), &format!("X-Etv-Api-Key: {key}\r\n"))?;
    let channels = json::parse(&reply).map_err(|e| error(e.to_string()))?;
    let list = channels.as_array().ok_or_else(|| error("channels: expected an array".into()))?;
    Ok(list.iter().filter_map(|c| Some((c.get("number")?.as_str()?.to_owned(), c.get("name")?.as_str()?.to_owned()))).collect())
}

/// Each channel gets its antenna and name, once per change of the list.
fn name_channels(config: &Config, folder: &Path, last: &mut Vec<(String, String)>) -> io::Result<()> {
    let channels = etv_channels(config, folder)?;
    if channels == *last {
        return Ok(());
    }
    let api = sync::http_url_of(&config.output_url).ok_or_else(|| error("no output_url".into()))?;
    let token = &config.vizrs.token;
    let headers = if token.is_empty() { String::new() } else { format!("X-Token: {token}\r\n") };
    for (number, name) in &channels {
        let body = json::object([("name", name.as_str().into())]).to_string();
        net::post(&format!("{api}/api/channels/{}", crate::overlay::quote_plus(number)), &headers, &body)?;
    }
    crate::info!("{} ErsatzTV channels named in VizRS", channels.len());
    *last = channels;
    Ok(())
}

fn sync_pages(config: &Config, elements: &Path) -> io::Result<()> {
    let api = sync::http_url_of(&config.output_url).ok_or_else(|| error("no output_url".into()))?;
    let pages = sync::parse_pages(&sync::fetch_show(&api, &config.vizrs.token)?).map_err(error)?;
    let general = fs::read_to_string(elements.join("vizrs.yml")).unwrap_or_default();
    let element_url = sync::channel_url(element_url(&general).unwrap_or("http://cg:7700/etv"));
    let report = sync::sync_templates(&pages, elements, SyncOptions { element_url: &element_url, rewrite: false, prune: false })?;
    if report.changed() {
        crate::info!("page elements: {report}");
    }
    Ok(())
}

/// At the start of the sidecar; each part is on only when configured.
pub fn start(config: &Config) {
    if !config.client_dir.is_empty() {
        match install_clients(Path::new(&config.client_source), Path::new(&config.client_dir)) {
            Ok(0) => {}
            Ok(changed) => crate::info!("{changed} ErsatzTV programs updated"),
            Err(e) => crate::warn!("ErsatzTV programs: {e}"),
        }
    }
    if config.etv_config.is_empty() {
        return;
    }
    let folder = Path::new(&config.etv_config).to_path_buf();
    let elements = sync::templates_dir(Some(&config.etv_config));
    if let Err(e) = install_element(&Path::new(&config.client_source).with_file_name("vizrs.yml"), &elements) {
        crate::warn!("graphics element: {e}");
    }
    let config = config.clone();
    let _ = thread::Builder::new().name("etv-sync".into()).spawn(move || {
        let mut named = Vec::new();
        loop {
            if config.channel_buses && !config.etv_url.is_empty() {
                if let Err(e) = name_channels(&config, &folder, &mut named) {
                    crate::warn!("channel names: {e}");
                }
            }
            if let Err(e) = sync_pages(&config, &elements) {
                crate::warn!("page elements: {e}");
            }
            thread::sleep(Duration::from_secs_f64(config.sync_seconds.max(1.0)));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn programs_and_element_are_installed() {
        let root = std::env::temp_dir().join(format!("vizrs-install-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (source, dir) = (root.join("client"), root.join("volume"));
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&dir).unwrap();
        fs::write(source.join("vizrs-ffmpeg"), "new").unwrap();
        assert_eq!(install_clients(&source, &dir).unwrap(), 1);
        assert_eq!(install_clients(&source, &dir).unwrap(), 0);
        assert_eq!(fs::metadata(dir.join("vizrs-ffmpeg")).unwrap().permissions().mode() & 0o777, 0o755);

        let template = root.join("vizrs.yml");
        fs::write(&template, "name: VizRS 2\nargs:\n  - http://cg:7700/etv\n").unwrap();
        fs::write(dir.join("vizrs.yml"), "name: VizRS\nargs:\n  - http://cg:7700/etv?channel=4\n").unwrap();
        install_element(&template, &dir).unwrap();
        assert_eq!(fs::read_to_string(dir.join("vizrs.yml")).unwrap(), "name: VizRS 2\nargs:\n  - http://cg:7700/etv?channel=4\n");
        fs::remove_dir_all(&root).unwrap();
    }
}
