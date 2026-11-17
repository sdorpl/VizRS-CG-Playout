//! Komunikaty strony i okna połączenia (komenda `ipc`, biała lista w `bridge`), sprawdzenie serwera, druga instancja.

use super::*;

pub(crate) fn on_conn(app: &AppHandle, body: &str) {
    let msg = match connect::parse_msg(body) {
        Ok(m) => m,
        Err(e) => return conn_error(app, &e),
    };
    match msg {
        connect::Msg::Cancel => {
            close_connect(app);
            if state(app).wins.is_empty() {
                app.exit(0);
            }
        }
        connect::Msg::Forget(url) => {
            let mut saved = connect::Saved::load();
            saved.forget(&url);
            if let Err(e) = saved.save() {
                conn_error(app, &e);
            }
        }
        connect::Msg::Connect(mut req) => {
            if req.token.is_none() {
                req.token = connect::Saved::load().token_for(&req.url);
            }
            let app = app.clone();
            std::thread::spawn(move || {
                let err = proxy::check(&req.url, req.token.as_deref()).err();
                let app2 = app.clone();
                let _ = app.run_on_main_thread(move || on_checked(&app2, req, err));
            });
        }
    }
}

pub(crate) fn on_checked(app: &AppHandle, req: connect::Request, err: Option<String>) {
    if let Some(e) = err {
        return conn_error(app, &e);
    }
    let connect::Request {
        url,
        token,
        keep_token,
        auto,
    } = req;
    let mut saved = connect::Saved::load();
    saved.remember(&url, token.as_deref(), keep_token, auto);
    if let Err(e) = saved.save() {
        eprintln!("zapis listy paneli: {e}");
    }
    match use_remote(app, url, token) {
        Ok(()) => switch(app),
        Err(e) => conn_error(app, &e),
    }
}

pub(crate) fn on_remote(app: &AppHandle, argv: &[String]) {
    if parse_args(argv).is_ok_and(|a| a.kind == Kind::Connect) {
        return open_connect(app);
    }
    let target = state(app).target_label();
    match target.and_then(|l| app.get_webview_window(&l)) {
        Some(w) => {
            let _ = w.unminimize();
            let _ = w.set_focus();
        }
        None => open_panel(app),
    }
}

#[tauri::command]
pub(crate) fn ipc(app: AppHandle, webview: tauri::Webview, body: String) {
    if let Some(rest) = body.strip_prefix("probe:") {
        let out = state(&app).probe.as_ref().map(|(_, o)| o.clone());
        if let Some(out) = out {
            if let Err(e) = std::fs::write(&out, rest) {
                eprintln!("{}: {e}", out.display());
            }
            app.exit(0);
        }
        return;
    }
    let label = webview.label().to_string();
    if label == CONNECT {
        return on_conn(&app, &body);
    }
    let Some(rest) = body.strip_prefix("app:") else {
        return;
    };
    let from = webview.url().map(|u| u.to_string()).unwrap_or_default();
    let (base, suffix) = {
        let a = state(&app);
        (
            a.page().map(|p| p.0).unwrap_or_default(),
            a.title().strip_prefix(TITLE).unwrap_or("").to_string(),
        )
    };
    if !label.starts_with("live-") || base.is_empty() || !from.starts_with(&format!("{base}/")) {
        return;
    }
    let Some(bridge::Msg::State { title, alert }) = bridge::parse(rest) else {
        return;
    };
    let changed = {
        let mut a = state(&app);
        let focused = a.focused.as_deref() == Some(label.as_str());
        let old = a.alert.insert(label.clone(), alert).unwrap_or(false);
        (old != alert).then_some(alert && !focused)
    };
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.set_title(&format!("{title}{suffix}"));
        if let Some(att) = changed {
            let _ = w.request_user_attention(att.then_some(UserAttentionType::Critical));
        }
    }
}
