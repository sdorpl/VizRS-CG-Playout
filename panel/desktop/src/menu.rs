//! Menu okna panelu: budowa, akcje, powiększenie.

use super::*;

pub(crate) fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    type Item = dyn tauri::menu::IsMenuItem<tauri::Wry>;
    let mac = cfg!(target_os = "macos");
    let item =
        |id: &str, label: &str, accel: Option<&str>| MenuItem::with_id(app, id, label, true, accel);
    let sep = || PredefinedMenuItem::separator(app);
    let quit = item(
        "quit",
        if mac {
            "Zakończ VizRS CG Playout"
        } else {
            "Zakończ"
        },
        if cfg!(target_os = "windows") {
            None
        } else {
            Some("CmdOrCtrl+Q")
        },
    )?;
    let connect = item("connect", "Połącz z panelem…", Some("CmdOrCtrl+Shift+K"))?;
    let new_window = item("new-window", "Nowe okno", Some("CmdOrCtrl+Shift+N"))?;
    let close = item("close", "Zamknij okno", Some("CmdOrCtrl+W"))?;
    let (s1, s2, s3) = (sep()?, sep()?, sep()?);
    let mut file: Vec<&Item> = vec![&connect, &s1, &new_window, &s2, &close];
    if !mac {
        file.extend([&s3 as &Item, &quit]);
    }
    let reload = item("reload", "Odśwież", Some("F5"))?;
    let full = item("fullscreen", "Pełny ekran", Some("F11"))?;
    let zin = item("zoom-in", "Powiększ", Some("CmdOrCtrl+="))?;
    let zout = item("zoom-out", "Pomniejsz", Some("CmdOrCtrl+-"))?;
    let z0 = item("zoom-reset", "Rozmiar 100%", Some("CmdOrCtrl+0"))?;
    let dev = item(
        "devtools",
        "Narzędzia deweloperskie",
        Some("CmdOrCtrl+Shift+I"),
    )?;
    let (s4, s5) = (sep()?, sep()?);
    let view: Vec<&Item> = vec![&reload, &full, &s4, &zin, &zout, &z0, &s5, &dev];
    let updates = item("updates", "Sprawdź aktualizacje", None)?;
    let about = item(
        "about",
        if mac {
            "O programie VizRS CG Playout"
        } else {
            "O programie"
        },
        None,
    )?;
    let s7 = sep()?;
    let mut help: Vec<&Item> = vec![&updates];
    if !mac {
        help.extend([&s7 as &Item, &about]);
    }
    let help_menu = Submenu::with_items(app, "Pomo&c", true, &help)?;
    #[allow(unused_mut)]
    let mut subs = vec![
        Submenu::with_items(app, "&Plik", true, &file)?,
        Submenu::with_items(app, "&Widok", true, &view)?,
    ];
    #[cfg(target_os = "macos")]
    {
        let app_menu = Submenu::with_items(
            app,
            "VizRS CG Playout",
            true,
            &[
                &about as &Item,
                &PredefinedMenuItem::separator(app)?,
                &PredefinedMenuItem::hide(app, Some("Ukryj VizRS CG Playout"))?,
                &PredefinedMenuItem::hide_others(app, Some("Ukryj pozostałe"))?,
                &PredefinedMenuItem::show_all(app, Some("Pokaż wszystko"))?,
                &PredefinedMenuItem::separator(app)?,
                &quit,
            ],
        )?;
        let edit = Submenu::with_items(
            app,
            "Edycja",
            true,
            &[
                &PredefinedMenuItem::undo(app, Some("Cofnij"))? as &Item,
                &PredefinedMenuItem::redo(app, Some("Ponów"))?,
                &PredefinedMenuItem::separator(app)?,
                &PredefinedMenuItem::cut(app, Some("Wytnij"))?,
                &PredefinedMenuItem::copy(app, Some("Kopiuj"))?,
                &PredefinedMenuItem::paste(app, Some("Wklej"))?,
                &PredefinedMenuItem::select_all(app, Some("Zaznacz wszystko"))?,
            ],
        )?;
        let window_menu = Submenu::with_items(
            app,
            "Okno",
            true,
            &[
                &PredefinedMenuItem::minimize(app, Some("Zminimalizuj"))? as &Item,
                &PredefinedMenuItem::maximize(app, Some("Powiększ okno"))?,
            ],
        )?;
        window_menu.set_as_windows_menu_for_nsapp()?;
        help_menu.set_as_help_menu_for_nsapp()?;
        subs.insert(0, app_menu);
        subs.insert(2, edit);
        subs.push(window_menu);
    }
    subs.push(help_menu);
    let refs: Vec<&Item> = subs.iter().map(|s| s as &Item).collect();
    Menu::with_items(app, &refs)
}

pub(crate) fn on_menu(app: &AppHandle, id: &str) {
    let target = state(app).target_label();
    let win = target.as_deref().and_then(|l| app.get_webview_window(l));
    match id {
        "connect" => open_connect(app),
        "new-window" => open_panel(app),
        "close" => {
            if let Some(w) = win {
                let _ = w.close();
            }
        }
        "quit" => app.exit(0),
        "reload" => {
            if let Some(w) = win {
                let _ = w.eval("location.reload()");
            }
        }
        "fullscreen" => {
            if let Some(w) = win {
                let _ = w.set_fullscreen(!w.is_fullscreen().unwrap_or(false));
            }
        }
        "zoom-in" | "zoom-out" | "zoom-reset" => {
            if let Some(w) = win {
                let z = zoom_step(app, w.label(), id);
                let _ = w.set_zoom(z);
            }
        }
        "devtools" => {
            if let Some(w) = win {
                w.open_devtools();
            }
        }
        "updates" => open_external(RELEASES),
        "about" => {
            let msg = {
                let a = state(app);
                let mode = match &a.mode {
                    Mode::Remote { url, .. } => {
                        format!("panel {url} (przez pośrednika okna – token zostaje w programie)")
                    }
                    Mode::None => "nie połączono".into(),
                };
                format!(
                    "VizRS CG Playout – natywny klient panelu (vizrs-live-desktop {}, Tauri 2)\n{mode}\n\nPowered by ErsatzTV (https://ersatztv.org) – playout kanałów; licencja MIT, składniki zewnętrzne: THIRD-PARTY.md",
                    env!("CARGO_PKG_VERSION")
                )
            };
            app.dialog().message(msg).title("O programie").show(|_| {});
        }
        _ => {}
    }
}

/// Powiększenie okna (menu Widok): ×1,1 / ÷1,1 / 100%, w granicach 30–300%.
pub(crate) fn zoom_step(app: &AppHandle, label: &str, id: &str) -> f64 {
    let zooms = app.state::<Zooms>();
    let mut m = zooms.lock().unwrap_or_else(|p| p.into_inner());
    let z = m.entry(label.to_string()).or_insert(1.0);
    *z = match id {
        "zoom-in" => (*z * 1.1).min(3.0),
        "zoom-out" => (*z / 1.1).max(0.3),
        _ => 1.0,
    };
    *z
}
