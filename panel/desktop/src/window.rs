//! Okna: panel (skrypt mostu, wygląd systemu, odzyskiwanie po awarii strony, uprawnienie dla adresu panelu), okno połączenia, przełączanie serwera.

use super::*;

/// Tło okna przed stroną – `--bg` z `vizrs-ui.css` (Windows, macOS).
#[cfg(not(target_os = "linux"))]
pub(crate) const BACKGROUND: Color = Color(0x0e, 0x10, 0x15, 0xff);

/// Most strony: `window.__vizrsPost(tekst)` → komenda `ipc` (jak okna VizRS CG System i Artist).
pub(crate) const IPC_SHIM: &str = r#"(() => {
  window.__vizrsPost = (m) => { window.__TAURI_INTERNALS__.invoke('ipc', { body: String(m) }).catch((e) => console.error('ipc', e)); };
  window.__vizrsShell = { version: '__VERSION__', app: 'live', os: '__OS__' };
})();"#;

pub(crate) fn script() -> String {
    IPC_SHIM
        .replace("__VERSION__", env!("CARGO_PKG_VERSION"))
        .replace("__OS__", std::env::consts::OS)
}

pub(crate) fn use_remote(
    app: &AppHandle,
    url: String,
    token: Option<String>,
) -> Result<(), String> {
    // sesja konta (K20): zapamiętana dla tego panelu; logowanie i wylogowanie na stronie trafiają do listy paneli
    let session = connect::Saved::load().session_for(&url);
    let u = url.clone();
    let on_session: proxy::OnSession = Box::new(move |s| {
        let mut saved = connect::Saved::load();
        saved.set_session(&u, s.as_deref());
        if let Err(e) = saved.save() {
            eprintln!("zapis sesji konta: {e}");
        }
    });
    let p = proxy::start(&url, token, session, Some(on_session))?;
    let mut a = state(app);
    a.mode = Mode::Remote { url, proxy: p };
    Ok(())
}

/// Strona bieżącego panelu może wołać komendę `ipc` (adres zna dopiero program – uprawnienie w działaniu).
/// Ramka podglądu anteny VizRS (`<VizRS>/output` w widokach Na antenie i Grafika). WebKitGTK zgłasza
/// nawigację ramki tak samo jak strony – bez tego wyjątku podgląd szedłby do przeglądarki systemowej.
pub(crate) fn vizrs_output(u: &tauri::Url) -> bool {
    matches!(u.scheme(), "http" | "https") && u.path() == "/output"
}

pub(crate) fn allow_page(app: &AppHandle, base: &str) {
    let n = {
        let mut a = state(app);
        if !a.allowed.insert(base.to_string()) {
            return;
        }
        a.allowed.len()
    };
    let cap = CapabilityBuilder::new(format!("panel-{n}"))
        .remote(format!("{base}/*"))
        .window("live-*")
        .permission("allow-ipc");
    if let Err(e) = app.add_capability(cap) {
        eprintln!("uprawnienie mostu dla {base}: {e}");
    }
}

pub(crate) fn native_look<'a>(
    b: WebviewWindowBuilder<'a, tauri::Wry, AppHandle>,
) -> WebviewWindowBuilder<'a, tauri::Wry, AppHandle> {
    let b = b
        .theme(Some(Theme::Dark))
        .devtools(true)
        .general_autofill_enabled(false);
    #[cfg(not(target_os = "linux"))]
    let b = b.background_color(BACKGROUND);
    #[cfg(target_os = "macos")]
    let b = b.allow_link_preview(false);
    b
}

pub(crate) fn reveal(app: &AppHandle, label: &str) {
    if let Some(w) = app.get_webview_window(label) {
        if !w.is_visible().unwrap_or(true) {
            let _ = w.show();
            let _ = w.maximize();
            let _ = w.set_focus();
        }
    }
}

pub(crate) fn page_loaded(app: &AppHandle, label: &str) {
    if label != CONNECT {
        reveal(app, label);
    }
    let probe = state(app).probe.as_ref().map(|(c, _)| c.clone());
    if let Some(code) = probe {
        eval_in(app, label, &code);
    }
}

/// Linux: proces strony WebKitGTK padł (sterownik GPU, brak pamięci) – WebKitGTK zostawia wtedy puste okno.
/// Strona wczytuje się od nowa z pierwszej strony bieżącego trybu (pośrednik: z sekretem), najwyżej 3 razy na minutę –
/// przy awarii za każdym wczytaniem okno zostaje puste zamiast mielić w kółko. Ten sam mechanizm w oknach
/// VizRS CG Artist i Trio (VizRS CG System).
pub(crate) fn reload_after_crash(app: &AppHandle, w: &tauri::WebviewWindow) {
    #[cfg(target_os = "linux")]
    {
        let app = app.clone();
        let _ = w.with_webview(move |pv| {
            use webkit2gtk::WebViewExt;
            let recent = std::cell::RefCell::new(Vec::<Instant>::new());
            pv.inner().connect_web_process_terminated(move |v, reason| {
                let mut r = recent.borrow_mut();
                r.retain(|t| t.elapsed() < Duration::from_secs(60));
                let url = (r.len() < 3)
                    .then(|| state(&app).page().map(|p| p.1))
                    .flatten();
                let what = if url.is_some() {
                    " – wczytuję stronę ponownie"
                } else {
                    " – okno zostaje puste"
                };
                eprintln!("proces strony zakończył się ({reason:?}){what}");
                if let Some(u) = url {
                    r.push(Instant::now());
                    v.load_uri(&u);
                }
            });
        });
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (app, w);
}

pub(crate) fn open_panel(app: &AppHandle) {
    let (page, title, label) = {
        let mut a = state(app);
        let Some(page) = a.page() else {
            drop(a);
            return open_connect(app);
        };
        a.next += 1;
        let label = format!("live-{}", a.next);
        a.wins.insert(label.clone());
        (page, a.title(), label)
    };
    let (base, start) = page;
    allow_page(app, &base);
    let Ok(url) = start.parse() else {
        return error(app, "zły adres panelu");
    };
    let (app_nav, app_new) = (app.clone(), app.clone());
    let (win_w, win_h) = screen::fit_to_screen(app, 1600.0, 900.0); // skalowanie ułamkowe: okno nie większe niż ekran
    let built = native_look(WebviewWindowBuilder::new(
        app,
        &label,
        WebviewUrl::External(url),
    ))
    .title(&title)
    .inner_size(win_w, win_h)
    .min_inner_size(400.0, 500.0)
    .visible(false)
    .initialization_script(script())
    .on_page_load(|w, p| {
        if p.event() == PageLoadEvent::Finished {
            page_loaded(w.app_handle(), w.label());
        }
    })
    // odnośniki poza panel (Trio VizRS, ErsatzTV, dokumentacja) – w przeglądarce systemowej
    .on_navigation(move |u| {
        let base = state(&app_nav).page().map(|p| p.0).unwrap_or_default();
        let ok = u.as_str().starts_with(&format!("{base}/"))
            || vizrs_output(u)
            || u.scheme() == "about"
            || u.scheme() == "blob"
            || u.scheme() == "data";
        if !ok {
            open_external(u.as_str());
        }
        ok
    })
    .on_new_window(move |u, _| {
        let base = state(&app_new).page().map(|p| p.0).unwrap_or_default();
        // strona panelu w nowym oknie programu nie ma sensu (ten sam panel) – zostaje w tym oknie
        if !u.as_str().starts_with(&format!("{base}/")) {
            open_external(u.as_str());
        }
        NewWindowResponse::Deny
    })
    .build();
    match built {
        Ok(w) => {
            #[cfg(target_os = "linux")]
            let _ = w.with_webview(|pv| {
                use webkit2gtk::{WebContextExt, WebViewExt};
                if let Some(ctx) = pv.inner().context() {
                    ctx.set_spell_checking_enabled(true);
                    ctx.set_spell_checking_languages(&["pl_PL"]);
                }
            });
            #[cfg(not(target_os = "linux"))]
            let _ = &w;
            reload_after_crash(app, &w);
            app.set_theme(Some(Theme::Dark));
            let (app2, l2) = (app.clone(), label.clone());
            std::thread::spawn(move || {
                std::thread::sleep(REVEAL_AFTER);
                let app3 = app2.clone();
                let _ = app2.run_on_main_thread(move || reveal(&app3, &l2));
            });
        }
        Err(e) => {
            state(app).wins.remove(&label);
            error(app, &format!("Nie udało się otworzyć okna: {e}"));
        }
    }
}

pub(crate) fn open_connect(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(CONNECT) {
        let _ = w.set_focus();
        return;
    }
    let data = {
        let a = state(app);
        let current = match &a.mode {
            Mode::Remote { url, .. } => url.clone(),
            Mode::None => String::new(),
        };
        connect::page_data(&connect::Saved::load(), &current, None)
    };
    let parent = state(app)
        .target_label()
        .and_then(|l| app.get_webview_window(&l));
    let mut b = native_look(WebviewWindowBuilder::new(
        app,
        CONNECT,
        WebviewUrl::App("connect.html".into()),
    ))
    .title("VizRS CG Playout – połącz z panelem")
    .inner_size(560.0, 600.0)
    .resizable(false)
    .maximizable(false)
    .minimizable(parent.is_none())
    .center()
    .initialization_script(format!("{}\n{data}", script()))
    .on_page_load(|w, p| {
        if p.event() == PageLoadEvent::Finished {
            page_loaded(w.app_handle(), w.label());
        }
    });
    if let Some(p) = &parent {
        b = match b.parent(p) {
            Ok(b) => b,
            Err(e) => return error(app, &format!("Nie udało się otworzyć okna połączenia: {e}")),
        };
    }
    match b.build() {
        Ok(w) => {
            #[cfg(not(target_os = "macos"))]
            let _ = w.remove_menu();
            let _ = w.set_focus();
        }
        Err(e) => error(app, &format!("Nie udało się otworzyć okna połączenia: {e}")),
    }
}

pub(crate) fn close_connect(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(CONNECT) {
        let _ = w.destroy();
    }
}

pub(crate) fn conn_error(app: &AppHandle, msg: &str) {
    eval_in(
        app,
        CONNECT,
        &format!(
            "window.__vizrsConnError && window.__vizrsConnError({})",
            Value::from(msg)
        ),
    );
}

/// Przełącza okna panelu na bieżący tryb (okno połączenia się zamyka); bez okien – otwiera panel.
pub(crate) fn switch(app: &AppHandle) {
    close_connect(app);
    let (page, title, labels) = {
        let a = state(app);
        (
            a.page(),
            a.title(),
            a.wins.iter().cloned().collect::<Vec<_>>(),
        )
    };
    if labels.is_empty() {
        return open_panel(app);
    }
    let Some((base, start)) = page else { return };
    allow_page(app, &base);
    for l in labels {
        if let Some(w) = app.get_webview_window(&l) {
            let _ = w.set_title(&title);
            if let Ok(u) = start.parse() {
                let _ = w.navigate(u);
            }
        }
    }
}
