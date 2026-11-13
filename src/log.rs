// Log lines as the Node.js sidecar wrote them: `<ISO time> LEVEL message`,
// INFO and DEBUG on stdout, WARN and ERROR on stderr; DEBUG only with DEBUG=1.
// With a log file (`set_file`: `<etv_config>/vizrs-live/logs/vizrs-cg.log`) every
// line also goes there: `docker logs` is gone with the container, and a stack
// redeployed overnight took the evidence of a live channel falling behind with it.

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::time;

static DEBUG: AtomicBool = AtomicBool::new(false);

/// A log file past this size is moved to `….1` (one old file kept).
pub const FILE_MAX: u64 = 4 * 1024 * 1024;

struct LogFile {
    path: PathBuf,
    file: File,
    size: u64,
}

static FILE: Mutex<Option<LogFile>> = Mutex::new(None);

/// Log lines also to `path` (its folder created), appended; an error leaves the log on stdout only.
pub fn set_file(path: &Path) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    let size = file.metadata().map_or(0, |m| m.len());
    *FILE.lock().unwrap_or_else(|e| e.into_inner()) = Some(LogFile { path: path.to_owned(), file, size });
    Ok(())
}

/// A line in the log file only, with the time first (the ingest listeners keep
/// their own stderr format, without it): nothing without `set_file`.
pub fn to_file_only(message: std::fmt::Arguments<'_>) {
    if FILE.lock().unwrap_or_else(|e| e.into_inner()).is_none() {
        return;
    }
    to_file(&format!("{} {message}\n", time::iso_time(time::now_ms())));
}

fn to_file(line: &str) {
    let mut guard = FILE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(log) = guard.as_mut() else { return };
    if log.size + line.len() as u64 > FILE_MAX {
        let mut old = log.path.clone().into_os_string();
        old.push(".1");
        let _ = std::fs::rename(&log.path, old);
        match OpenOptions::new().create(true).append(true).open(&log.path) {
            Ok(file) => {
                log.file = file;
                log.size = 0;
            }
            Err(_) => {
                *guard = None;
                return;
            }
        }
    }
    if log.file.write_all(line.as_bytes()).is_ok() {
        log.size += line.len() as u64;
    }
}

pub fn set_debug(on: bool) {
    DEBUG.store(on, Ordering::Relaxed);
}

pub fn debug_enabled() -> bool {
    DEBUG.load(Ordering::Relaxed)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

pub fn write(level: Level, message: std::fmt::Arguments<'_>) {
    let (name, to_stderr) = match level {
        Level::Debug if !debug_enabled() => return,
        Level::Debug => ("DEBUG", false),
        Level::Info => ("INFO", false),
        Level::Warn => ("WARN", true),
        Level::Error => ("ERROR", true),
    };
    let line = format!("{} {name} {message}\n", time::iso_time(time::now_ms()));
    to_file(&line);
    // A closed stdout/stderr must not take the sidecar down.
    let _ = if to_stderr { std::io::stderr().write_all(line.as_bytes()) } else { std::io::stdout().write_all(line.as_bytes()) };
}

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => { $crate::log::write($crate::log::Level::Debug, format_args!($($arg)*)) };
}

#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => { $crate::log::write($crate::log::Level::Info, format_args!($($arg)*)) };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => { $crate::log::write($crate::log::Level::Warn, format_args!($($arg)*)) };
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => { $crate::log::write($crate::log::Level::Error, format_args!($($arg)*)) };
}

#[cfg(test)]
mod tests {
    #[test]
    fn lines_go_to_the_file_too_and_it_rotates() {
        let dir = std::env::temp_dir().join(format!("vizrs-log-{}", std::process::id()));
        let path = dir.join("logs").join("vizrs-cg.log");
        super::set_file(&path).unwrap();
        crate::info!("first line");
        let text = std::fs::read_to_string(&path).unwrap();
        // Other tests log in parallel: their lines may be in the file too.
        assert!(text.contains(" INFO first line\n"), "{text}");
        super::to_file_only(format_args!("vizrs-ingest: a listener line"));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("Z vizrs-ingest: a listener line\n"), "{text}");
        // Past FILE_MAX the file moves to `.1` and a new one starts.
        let big = "x".repeat(64 * 1024);
        for _ in 0..(super::FILE_MAX / big.len() as u64 + 1) {
            crate::info!("{big}");
        }
        assert!(dir.join("logs").join("vizrs-cg.log.1").exists());
        assert!(std::fs::metadata(&path).unwrap().len() < super::FILE_MAX);
        *super::FILE.lock().unwrap() = None;
        let _ = std::fs::remove_dir_all(&dir);
    }
}
