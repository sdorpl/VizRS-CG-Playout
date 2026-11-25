//! VizRS CG Playout – aplikacja panelu kanału (dawniej VizRS Live) na Tauri 2 (Linux, Windows, macOS).
//!
//!   vizrs-live-desktop [connect] [--connect URL] [--token T]
//!
//! Natywna aplikacja sterowania: klient panelu działającego na serwerze w stosie (np. VPS, studio). Strona to ten
//! sam panel co w przeglądarce (serwuje go `vizrs-live` na serwerze); okno łączy się z nim przez pośrednika na
//! `127.0.0.1` (`proxy`), który dokleja sesję konta albo token – strona ich nie zna. Okno niczego nie uruchamia lokalnie: panel bez
//! ErsatzTV i jego mostu nie ma czym sterować. Bez zapisanego panelu „przy uruchomieniu” – okno „Połącz z panelem”.
//!
//! Strona nie ma dostępu do systemu – tylko komunikaty z białej listy (`bridge`) przez komendę `ipc`, i tylko
//! od strony bieżącego panelu (uprawnienie w działaniu: `allow_page`).

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod bridge;
mod conn_url;
mod connect;
mod gpu;
mod ipc;
mod menu;
mod proxy;
mod scale;
mod screen;
mod window;

use ipc::*;
use menu::*;
use window::*;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;
#[cfg(target_os = "linux")]
use std::time::Instant;

use serde_json::Value;
use tauri::ipc::CapabilityBuilder;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::webview::{NewWindowResponse, PageLoadEvent};
#[cfg(not(target_os = "linux"))]
use tauri::window::Color;
use tauri::{
    AppHandle, Manager, Theme, UserAttentionType, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

const CONNECT: &str = "connect";
const RELEASES: &str = "https://github.com/sdorpl/VizRS-CG-Playout/releases";
const TITLE: &str = "VizRS CG Playout";
/// Okno czeka ukryte na pierwszą klatkę strony (bez białego błysku), ale nie dłużej niż tyle.
const REVEAL_AFTER: Duration = Duration::from_millis(2500);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Panel,
    Connect,
}

struct Args {
    kind: Kind,
    connect: Option<String>,
    token: Option<String>,
    probe: Option<(PathBuf, PathBuf)>,
}

fn help() -> &'static str {
    "vizrs-live-desktop – aplikacja VizRS CG Playout: natywny klient panelu (Tauri 2)

Użycie: vizrs-live-desktop [connect] [opcje]

Klient panelu na serwerze. Bez opcji: zapisany panel „przy uruchomieniu”, a bez niego okno „Połącz z panelem”.

  connect         najpierw okno „Połącz z panelem” (adres i token panelu, lista zapisanych)
                  Bez tokenu: logowanie kontem operatora w oknie panelu, sesja zapamiętana na tym komputerze.
  --connect URL   panel na serwerze, np. https://live.example.pl albo http://serwer:8090 (VIZRS_LIVE_CONNECT)
  --token T       token panelu (VIZRS_LIVE_TOKEN) – zostaje w programie, strona go nie dostaje
  --probe JS --probe-out JSON   narzędzie testowe: po wczytaniu strony wykonaj skrypt, zapisz wynik, zakończ

Plik → Połącz z panelem… (Ctrl+Shift+K). Działa jedna instancja programu – kolejne uruchomienie wysuwa okno."
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
    let mut a = Args {
        kind: Kind::Panel,
        connect: env("VIZRS_LIVE_CONNECT"),
        token: None,
        probe: None,
    };
    let (mut pj, mut po) = (None, None);
    let mut i = 0;
    while i < argv.len() {
        let flag = argv[i].clone();
        let mut val = || -> Result<String, String> {
            i += 1;
            argv.get(i).cloned().ok_or(format!("{flag}: brak wartości"))
        };
        match flag.as_str() {
            "connect" => a.kind = Kind::Connect,
            "--connect" => a.connect = Some(val()?),
            "--token" => a.token = Some(val()?),
            "--probe" => pj = Some(PathBuf::from(val()?)),
            "--probe-out" => po = Some(PathBuf::from(val()?)),
            "--help" | "-h" => {
                println!("{}", help());
                std::process::exit(0);
            }
            "--version" | "-V" => {
                println!("vizrs-live-desktop {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            other if other.starts_with("-psn_") => {}
            other => {
                return Err(format!(
                    "nieznany argument {other} (vizrs-live-desktop --help)"
                ))
            }
        }
        i += 1;
    }
    if a.connect.is_some() {
        a.token = a.token.or_else(|| env("VIZRS_LIVE_TOKEN"));
        a.connect = Some(connect::normalize_url(
            a.connect.as_deref().unwrap_or_default(),
        )?);
    }
    a.probe = match (pj, po) {
        (Some(j), Some(o)) => Some((j, o)),
        (None, None) => None,
        _ => return Err("--probe i --probe-out podaje się razem".into()),
    };
    Ok(a)
}

/// Gdzie pracuje okno.
enum Mode {
    /// panel na serwerze przez pośrednika z sesją konta albo tokenem
    Remote { url: String, proxy: proxy::Proxy },
    /// przed wyborem (okno połączenia)
    None,
}

struct App {
    mode: Mode,
    probe: Option<(String, PathBuf)>,
    wins: HashSet<String>,
    alert: HashMap<String, bool>,
    focused: Option<String>,
    next: u32,
    allowed: HashSet<String>,
}

impl App {
    /// Adres serwera strony okna i pierwszej strony.
    fn page(&self) -> Option<(String, String)> {
        match &self.mode {
            Mode::Remote { proxy, .. } => Some((proxy.base.clone(), proxy.start_url())),
            Mode::None => None,
        }
    }
    fn title(&self) -> String {
        match &self.mode {
            Mode::Remote { url, .. } => {
                format!("{TITLE} – {}", url.split("://").nth(1).unwrap_or(url))
            }
            _ => TITLE.into(),
        }
    }
    fn target_label(&self) -> Option<String> {
        self.focused
            .clone()
            .filter(|l| self.wins.contains(l))
            .or_else(|| self.wins.iter().next().cloned())
    }
}

type Shared = Mutex<App>;

fn state(app: &AppHandle) -> std::sync::MutexGuard<'_, App> {
    app.state::<Shared>()
        .inner()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

fn eval_in(app: &AppHandle, label: &str, js: &str) {
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.eval(js);
    }
}

fn error(app: &AppHandle, msg: &str) {
    eprintln!("{msg}");
    app.dialog()
        .message(msg.to_string())
        .title(TITLE)
        .kind(MessageDialogKind::Error)
        .show(|_| {});
}

/// Zmienne WebKitGTK ustawione przez `gpu::prepare` (nie przez użytkownika) – tylko dla tego okna.
static GPU_SET: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();

/// Zmienne, które hooki AppImage (linuxdeploy: gtk, gstreamer) i jego środowisko uruchomieniowe ustawiają
/// dla bibliotek z paczki – przeglądarka systemowa z nimi ładowałaby moduły GTK/GIO/GStreamera z paczki.
#[cfg_attr(any(target_os = "windows", target_os = "macos"), allow(dead_code))] // tylko Linux (AppImage, xdg-open)
const APPIMAGE_VARS: &[&str] = &[
    "APPDIR",
    "APPIMAGE",
    "ARGV0",
    "OWD",
    "GTK_DATA_PREFIX",
    "GTK_THEME",
    "GTK_EXE_PREFIX",
    "GTK_PATH",
    "GTK_IM_MODULE_FILE",
    "GSETTINGS_SCHEMA_DIR",
    "GI_TYPELIB_PATH",
    "GIO_MODULE_DIR",
    "GDK_PIXBUF_MODULE_FILE",
    "GST_REGISTRY_REUSE_PLUGIN_SCANNER",
    "GST_PLUGIN_SYSTEM_PATH",
    "GST_PLUGIN_SYSTEM_PATH_1_0",
    "GST_PLUGIN_PATH",
    "GST_PLUGIN_PATH_1_0",
    "GST_PLUGIN_SCANNER",
    "GST_PLUGIN_SCANNER_1_0",
    "GST_PTP_HELPER",
    "GST_PTP_HELPER_1_0",
];
/// Listy ścieżek, z których usuwamy wpisy wewnątrz paczki (reszta – systemu – zostaje).
#[cfg_attr(any(target_os = "windows", target_os = "macos"), allow(dead_code))] // tylko Linux (AppImage, xdg-open)
const APPIMAGE_LISTS: &[&str] = &["PATH", "LD_LIBRARY_PATH", "XDG_DATA_DIRS"];

/// Środowisko programu zewnętrznego (`xdg-open` → przeglądarka): bez zmian paczki AppImage i bez
/// zmiennych WebKitGTK okna. `get` – bieżące środowisko; wynik: (nazwa, Some(nowa) | None = usuń).
#[cfg_attr(any(target_os = "windows", target_os = "macos"), allow(dead_code))] // tylko Linux (AppImage, xdg-open)
fn external_env(
    get: &dyn Fn(&str) -> Option<String>,
    gpu_set: &[String],
) -> Vec<(String, Option<String>)> {
    let mut out: Vec<(String, Option<String>)> =
        gpu_set.iter().map(|k| (k.clone(), None)).collect();
    let Some(appdir) = get("APPDIR").filter(|d| !d.is_empty()) else {
        return out;
    };
    let appdir = appdir.trim_end_matches('/').to_string();
    out.extend(APPIMAGE_VARS.iter().map(|k| (k.to_string(), None)));
    for k in APPIMAGE_LISTS {
        let Some(v) = get(k) else { continue };
        let kept: Vec<&str> = v
            .split(':')
            .filter(|p| !p.is_empty() && !p.starts_with(&appdir))
            .collect();
        out.push((k.to_string(), (!kept.is_empty()).then(|| kept.join(":"))));
    }
    out
}

fn open_external(target: &str) {
    #[cfg(target_os = "windows")]
    let r = Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", target])
        .spawn();
    #[cfg(target_os = "macos")]
    let r = Command::new("open").arg(target).spawn();
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let r = {
        let mut c = Command::new("xdg-open");
        let gpu = GPU_SET.get().map(Vec::as_slice).unwrap_or_default();
        for (k, v) in external_env(&|k| std::env::var(k).ok(), gpu) {
            match v {
                Some(v) => c.env(k, v),
                None => c.env_remove(k),
            };
        }
        c.arg(target).spawn()
    };
    if let Err(e) = r {
        eprintln!("nie mogę otworzyć {target}: {e}");
    }
}

// ------------------------------------------------------------------ menu

type Zooms = Mutex<HashMap<String, f64>>;

// ------------------------------------------------------------------ start

fn main() {
    // akceleracja WebKitGTK: zmienne środowiska przed startem GTK i przed powstaniem wątków (gpu.rs)
    const GPU_VARS: &[&str] = &[
        "GDK_BACKEND",
        "WEBKIT_DISABLE_DMABUF_RENDERER",
        "WEBKIT_DISABLE_COMPOSITING_MODE",
        "WEBKIT_FORCE_DMABUF_RENDERER",
        "WEBKIT_SKIA_ENABLE_CPU_RENDERING",
        "WEBKIT_DMABUF_RENDERER_FORCE_SHM",
        "WEBKIT_FORCE_COMPOSITING_MODE",
        "__NV_DISABLE_EXPLICIT_SYNC",
    ];
    let before: Vec<bool> = GPU_VARS
        .iter()
        .map(|k| std::env::var_os(k).is_some())
        .collect();
    let gpu_note = gpu::prepare(None);
    let _ = GPU_SET.set(
        GPU_VARS
            .iter()
            .zip(before)
            .filter(|(k, had)| !had && std::env::var_os(k).is_some())
            .map(|(k, _)| k.to_string())
            .collect(),
    );
    if !gpu_note.is_empty() {
        eprintln!("GPU: {gpu_note}");
    }
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let probe = args
        .probe
        .as_ref()
        .map(|(js, out)| match std::fs::read_to_string(js) {
            Ok(code) => (code, out.clone()),
            Err(e) => {
                eprintln!("{}: {e}", js.display());
                std::process::exit(2);
            }
        });
    let auto = if args.connect.is_none() {
        connect::Saved::load().auto
    } else {
        None
    };
    let app_state = App {
        mode: Mode::None,
        probe,
        wins: HashSet::new(),
        alert: HashMap::new(),
        focused: None,
        next: 0,
        allowed: HashSet::new(),
    };
    let mut builder = tauri::Builder::default();
    if args.probe.is_none() {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            let argv: Vec<String> = argv.into_iter().skip(1).collect();
            on_remote(app, &argv);
        }));
    }
    let (kind, connect_url, token) = (args.kind, args.connect.clone(), args.token.clone());
    let built = builder
        .plugin(tauri_plugin_dialog::init())
        .manage(Mutex::new(app_state))
        .manage(Zooms::default())
        .invoke_handler(tauri::generate_handler![ipc::ipc])
        .setup(move |app| {
            let h = app.handle().clone();
            h.set_theme(Some(Theme::Dark));
            app.set_menu(build_menu(&h)?)?;
            app.on_menu_event(|app, ev| on_menu(app, ev.id().as_ref()));
            // panel na serwerze: --connect albo zapisany „przy uruchomieniu”; bez nich – okno połączenia
            let started = if let Some(c) = connect_url {
                Some(use_remote(&h, c, token))
            } else if let Some(a) = auto {
                let t = connect::Saved::load().token_for(&a);
                Some(use_remote(&h, a, t))
            } else {
                None
            };
            match started {
                None => open_connect(&h),
                Some(Ok(())) if kind != Kind::Connect => open_panel(&h),
                Some(Ok(())) => open_connect(&h),
                // zapisany panel nieosiągalny – wybór panelu z komunikatem
                Some(Err(e)) if kind == Kind::Panel => {
                    open_connect(&h);
                    let (h2, msg) = (h.clone(), e.clone());
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(800));
                        let h3 = h2.clone();
                        let _ = h2.run_on_main_thread(move || conn_error(&h3, &msg));
                    });
                    eprintln!("{e}");
                }
                Some(Err(e)) => {
                    eprintln!("{e}");
                    let h2 = h.clone();
                    h.dialog()
                        .message(e)
                        .title(TITLE)
                        .kind(MessageDialogKind::Error)
                        .show(move |_| h2.exit(1));
                }
            }
            Ok(())
        })
        .on_window_event(|w, ev| {
            let label = w.label().to_string();
            let app = w.app_handle();
            match ev {
                WindowEvent::Focused(true) if label != CONNECT => state(app).focused = Some(label),
                WindowEvent::Destroyed => {
                    {
                        let mut a = state(app);
                        a.wins.remove(&label);
                        a.alert.remove(&label);
                    }
                    if app.webview_windows().is_empty() {
                        app.exit(0);
                    }
                }
                _ => {}
            }
        })
        .build(tauri::generate_context!());
    let app = match built {
        Ok(a) => a,
        Err(e) => {
            eprintln!("vizrs-live-desktop: {e}");
            std::process::exit(1);
        }
    };
    // pośrednik kończy się razem z procesem
    app.run(|_, _| {});
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Result<Args, String> {
        parse_args(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn command_line() {
        assert_eq!(args(&[]).unwrap().kind, Kind::Panel);
        // dawny panel lokalny – usunięty: okno jest tylko klientem panelu na serwerze
        assert!(args(&["local"]).is_err());
        assert!(args(&["--live", "/x/vizrs-live"]).is_err());
        assert!(args(&["--", "--bridge", "http://b:8409"]).is_err());
        let a = args(&["--connect", "https://live.example.pl/", "--token", "abc"]).unwrap();
        assert_eq!(a.connect.as_deref(), Some("https://live.example.pl"));
        assert_eq!(a.token.as_deref(), Some("abc"));
        assert_eq!(args(&["connect"]).unwrap().kind, Kind::Connect);
        assert!(args(&["--probe", "a.js"]).is_err());
        assert!(args(&["--nieznana"]).is_err());
        assert!(args(&["-psn_0_1"]).is_ok());
    }

    #[test]
    fn external_env_without_appimage() {
        let env = |k: &str| (k == "PATH").then(|| "/usr/bin".to_string());
        let gpu = vec!["GDK_BACKEND".to_string()];
        assert_eq!(
            external_env(&env, &gpu),
            vec![("GDK_BACKEND".to_string(), None)]
        );
    }

    #[test]
    fn external_env_in_appimage() {
        let env = |k: &str| match k {
            "APPDIR" => Some("/tmp/.mount_X/".to_string()),
            "PATH" => Some("/tmp/.mount_X/usr/bin:/usr/bin".to_string()),
            "LD_LIBRARY_PATH" => Some("/tmp/.mount_X/usr/lib/:/tmp/.mount_X/lib64/:".to_string()),
            "XDG_DATA_DIRS" => Some(
                "/tmp/.mount_X//usr/share:/usr/share:/var/lib/flatpak/exports/share".to_string(),
            ),
            _ => None,
        };
        let e = external_env(&env, &[]);
        let get = |k: &str| e.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        assert_eq!(get("PATH"), Some(Some("/usr/bin".into())));
        assert_eq!(get("LD_LIBRARY_PATH"), Some(None));
        assert_eq!(
            get("XDG_DATA_DIRS"),
            Some(Some("/usr/share:/var/lib/flatpak/exports/share".into()))
        );
        for k in [
            "APPDIR",
            "GIO_MODULE_DIR",
            "GTK_PATH",
            "GST_PLUGIN_SYSTEM_PATH_1_0",
            "GTK_THEME",
        ] {
            assert_eq!(get(k), Some(None), "{k}");
        }
    }

    #[test]
    fn vizrs_output_frame() {
        let u = |s: &str| s.parse::<tauri::Url>().unwrap();
        assert!(vizrs_output(&u(
            "https://s1.btb.j00r.us/output?token=x&bus=ch-1"
        )));
        assert!(vizrs_output(&u("http://tv.lan:8788/output")));
        assert!(!vizrs_output(&u("https://s1.btb.j00r.us/trio?channel=1")));
        assert!(!vizrs_output(&u("https://example.com/output/inne")));
        assert!(!vizrs_output(&u("file:///output")));
    }
}
