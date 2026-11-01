// ffmpeg overlay for ErsatzTV: `vizrs-ffmpeg <ffmpeg arguments>`, set as the
// FFmpeg path in ErsatzTV (src/overlay.rs, DOKUMENTACJA §1).

use vizrs_cg::overlay::{self, Settings};

fn main() {
    let code = overlay::run(&Settings::from_env(), std::env::args_os().skip(1).collect());
    std::process::exit(code);
}
