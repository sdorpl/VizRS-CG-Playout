// ffmpeg overlay for ErsatzTV: `vizrs-ffmpeg <ffmpeg arguments>`, set as the
// FFmpeg path in ErsatzTV (src/overlay.rs, DOKUMENTACJA §1).
// `vizrs-ffmpeg vizrs-ingest`: the live ingest listeners (src/listener.rs).

use vizrs_cg::listener;
use vizrs_cg::overlay::{self, Settings};

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let code = if args.first().is_some_and(|a| a == "vizrs-ingest") {
        // `docker logs` is gone with the container: publishers coming and going
        // (a live test overnight) are kept in a file on a volume.
        let file = std::env::var("VIZRS_INGEST_LOG").unwrap_or_default();
        if !file.trim().is_empty() {
            if let Err(error) = vizrs_cg::log::set_file(std::path::Path::new(file.trim())) {
                eprintln!("vizrs-ingest: log file {}: {error}; log on stderr only", file.trim());
            }
        }
        match listener::Settings::from_env() {
            Ok(settings) => listener::run(settings),
            Err(error) => {
                eprintln!("vizrs-ingest: {error}");
                2
            }
        }
    } else {
        overlay::run(&Settings::from_env(), args)
    };
    std::process::exit(code);
}
