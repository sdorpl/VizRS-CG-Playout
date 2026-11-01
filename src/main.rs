// VizRS CG Playout sidecar: `virtrs-cg [config.json]` (or CONFIG=config.json).

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use virtrs_cg::config::Config;
use virtrs_cg::etv_sync;
use virtrs_cg::server::{MAX_BODY, Server};
use virtrs_cg::{error, http, info, log, sys, warn};

fn main() -> ExitCode {
    // Before any thread starts: SIGTERM and SIGINT go to the stop thread below.
    sys::block_stop_signals();
    log::set_debug(std::env::var("DEBUG").as_deref() == Ok("1"));

    let mut args = std::env::args_os().skip(1);
    let first = args.next();

    // virtrs-cg sync [--output-dir <DIR>] [--channel <CH>] [--url <VIZRS_HTTP>] [--token <TOKEN>] [--cg-url <URL>] [--prune]
    if first.as_deref().and_then(|s| s.to_str()) == Some("sync") {
        let mut output_dir = PathBuf::from("etv/templates/graphics-elements/script");
        let mut channel = "1".to_string();
        let mut vizrs_url = std::env::var("VIZRS_HTTP_URL").unwrap_or_else(|_| "http://vizrs:8088".into());
        let mut token = std::env::var("VIZRS_TOKEN").unwrap_or_default();
        let mut cg_url = std::env::var("VIZRS_CG_URL").unwrap_or_else(|_| "http://cg:7700".into());
        let mut show_file: Option<PathBuf> = None;
        let mut prune = false;

        while let Some(arg) = args.next() {
            let s = arg.to_string_lossy();
            match s.as_ref() {
                "--output-dir" | "-o" => {
                    if let Some(val) = args.next() {
                        output_dir = PathBuf::from(val);
                    }
                }
                "--channel" | "-c" => {
                    if let Some(val) = args.next() {
                        channel = val.to_string_lossy().into_owned();
                    }
                }
                "--url" | "-u" => {
                    if let Some(val) = args.next() {
                        vizrs_url = val.to_string_lossy().into_owned();
                    }
                }
                "--token" | "-t" => {
                    if let Some(val) = args.next() {
                        token = val.to_string_lossy().into_owned();
                    }
                }
                "--cg-url" => {
                    if let Some(val) = args.next() {
                        cg_url = val.to_string_lossy().into_owned();
                    }
                }
                "--file" | "-f" => {
                    if let Some(val) = args.next() {
                        show_file = Some(PathBuf::from(val));
                    }
                }
                "--prune" => prune = true,
                _ => {}
            }
        }

        let show_json = match show_file {
            Some(path) => match std::fs::read_to_string(&path) {
                Ok(content) => content,
                Err(e) => {
                    error!("read {}: {e}", path.display());
                    return ExitCode::FAILURE;
                }
            },
            None => match virtrs_cg::sync::fetch_show(&vizrs_url, &token) {
                Ok(content) => content,
                Err(e) => {
                    error!("fetch from {vizrs_url}: {e}");
                    return ExitCode::FAILURE;
                }
            },
        };

        let pages = match virtrs_cg::sync::parse_pages(&show_json) {
            Ok(pages) => pages,
            Err(e) => {
                error!("sync templates: {e}");
                return ExitCode::FAILURE;
            }
        };
        let element_url = format!("{}/etv?channel={channel}", cg_url.trim_end_matches('/'));
        let options = virtrs_cg::sync::SyncOptions { element_url: &element_url, rewrite: true, prune };
        match virtrs_cg::sync::sync_templates(&pages, &output_dir, options) {
            Ok(report) => {
                info!("synced {} pages to {}: {report}", pages.len(), output_dir.display());
                return ExitCode::SUCCESS;
            }
            Err(e) => {
                error!("sync templates: {e}");
                return ExitCode::FAILURE;
            }
        }
    }

    let file = first.or_else(|| std::env::var_os("CONFIG")).map(PathBuf::from);
    let config = match Config::load(file.as_deref()) {
        Ok((config, warnings)) => {
            for warning in warnings {
                warn!("{warning}");
            }
            config
        }
        Err(message) => {
            error!("{message}");
            return ExitCode::FAILURE;
        }
    };

    let listener = match TcpListener::bind(config.listen_addr()) {
        Ok(listener) => listener,
        Err(e) => {
            error!("listen on {}: {e}", config.listen);
            return ExitCode::FAILURE;
        }
    };

    let burn_in = if config.output_url.is_empty() { "off" } else { config.output_url.as_str() };
    info!("listening on {}; VizRS {}; burn-in {burn_in}", config.listen, config.vizrs.address);

    etv_sync::start(&config);
    let server = Server::new(config);
    let stopping = Arc::clone(&server);
    std::thread::spawn(move || {
        sys::wait_for_stop();
        // Browsers and their profiles go with the sidecar.
        stopping.stop_captures();
        std::process::exit(0);
    });
    http::serve(listener, MAX_BODY, Arc::new(move |request, stream| server.handle(request, stream)));
    ExitCode::FAILURE
}
