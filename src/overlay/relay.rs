//! Strumień klatek grafiki: przekazywanie z gniazda sidecara do ffmpeg, argumenty wejścia surowego, wspólne przesyłanie na GPU.

use super::*;

/// Bytes of one yuva420p frame.
pub(crate) fn frame_bytes(width: u32, height: u32) -> usize {
    let (w, h) = (width as usize, height as usize);
    2 * w * h + 2 * w.div_ceil(2) * h.div_ceil(2)
}

/// Frames from the sidecar -> ffmpeg's input. The sidecar gone (a new version
/// of the stack), ffmpeg gets transparent frames, one every `interval` (the
/// channel's rate: a media read without `-readrate`, synced in RAM, would
/// otherwise race to the end of the item behind them; `None` = as fast as it
/// reads them), so the channel goes on; a thread reconnects and the frames
/// follow. Without a way back (`resume` empty) ffmpeg sees the end of its
/// input, as before. A delta stream is patched into the frame shown, and
/// ffmpeg gets it whole.
pub(crate) fn relay(
    socket: FrameSocket,
    mut pipe: impl Write,
    frame: usize,
    interval: Option<Duration>,
    reconnect: Option<&dyn Fn() -> mpsc::Receiver<FrameSocket>>,
) {
    let mut buffer = vec![0u8; 1 << 20];
    let blank = vec![0u8; frame];
    let mut shown = vec![0u8; frame];
    let mut source = socket;
    let mut partial = 0;
    loop {
        if source.delta {
            let mut reader = io::BufReader::with_capacity(1 << 16, &source.stream);
            while crate::delta::read_record(&mut reader, &mut shown).is_ok() {
                if pipe.write_all(&shown).is_err() {
                    return;
                }
            }
        } else {
            loop {
                match source.stream.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if pipe.write_all(&buffer[..n]).is_err() {
                            return;
                        }
                        partial = (partial + n) % frame;
                    }
                }
            }
        }
        let Some(reconnect) = reconnect else { return };
        log("frame stream lost; transparent frames until the sidecar is back");
        if pipe.write_all(&blank[..(frame - partial) % frame]).is_err() {
            return;
        }
        partial = 0;
        let back = reconnect();
        let mut next = std::time::Instant::now();
        source = loop {
            if pipe.write_all(&blank).is_err() {
                return;
            }
            if let Some(interval) = interval {
                next += interval;
                // Behind by more than a second (a stalled reader): from now, no burst.
                let now = std::time::Instant::now();
                if next + Duration::from_secs(1) < now {
                    next = now;
                }
                std::thread::sleep(next.saturating_duration_since(now));
            }
            if let Ok(socket) = back.try_recv() {
                log("frame stream back");
                break socket;
            }
        };
    }
}

/// The descriptor pair ffmpeg reads the raw graphics frames from (`pipe:<fd>`
/// takes any descriptor): a Unix socket, not a pipe. Pipes count towards the
/// pipe memory of their user, shared by every container running as root and by
/// docker-proxy (1 MiB pipes per proxied connection): past
/// `fs.pipe-user-pages-soft` (64 MiB) the kernel gives every new pipe of a
/// container 8 KiB, a 3.6 MB frame of 1600x900 then takes ~450 hand-overs and
/// the channel falls behind real time (0.96x measured with 45 viewers).
/// A socket's buffer is `net.core.wmem_default` (~208 KiB), whatever the pipes.
pub fn frame_channel() -> io::Result<(UnixStream, UnixStream)> {
    UnixStream::pair()
}

/// The input of the raw graphics frames at `pipe:<fd>`, with its options.
pub(crate) fn raw_input(frames: &Frames, fd: i32) -> Vec<OsString> {
    let size = format!("{}x{}", frames.width, frames.height);
    let raw = ["-f", "rawvideo", "-pix_fmt", "yuva420p", "-video_size", &size, "-framerate", &frames.rate];
    let mut out: Vec<OsString> = raw.iter().map(OsString::from).collect();
    // Undeclared, the overlay negotiates an unknown colour space for the whole
    // chain and swscale sends every scaled frame of the media through RGB.
    if !frames.matrix.is_empty() {
        out.extend(["-colorspace", &frames.matrix, "-color_range", "tv"].iter().map(OsString::from));
    }
    out.extend(["-i".into(), format!("pipe:{fd}").into()]);
    out
}

/// The command with the canvas input replaced by raw frames from `pipe:<fd>`.
pub fn overlay_args(args: &[OsString], canvas: &Canvas, fd: i32, frames: &Frames) -> Vec<OsString> {
    let options = &args[canvas.start..canvas.index];
    let mut out: Vec<OsString> = args[..canvas.start].to_vec();
    let mut i = 0;
    while i < options.len() {
        if i + 1 < options.len() && DROP_WITH_VALUE.iter().any(|o| options[i] == *o) {
            i += 2;
            continue;
        }
        out.push(options[i].clone());
        i += 1;
    }
    out.extend(raw_input(frames, fd));
    out.extend_from_slice(&args[canvas.index + 2..]);
    // Frames of graphics areas: each overlaid at its place (checked by `with_position` beforehand).
    let at_origin = matches!(frames.areas.as_slice(), [(a, _)] if (a.x, a.y) == (0, 0));
    if !frames.areas.is_empty() && !at_origin {
        out = with_areas(&out, input_number(args, canvas.index), &frames.areas).unwrap_or(out);
    }
    shared_upload(out)
}

pub(crate) fn upload_filter() -> &'static Regex {
    static UPLOAD: OnceLock<Regex> = OnceLock::new();
    // `hwupload_cuda` without options, a whole filter of a chain.
    UPLOAD.get_or_init(|| Regex::new(r"(^|[,;\]])hwupload_cuda([,;\[]|$)").expect("upload pattern"))
}

/// A command with one CUDA device (`-init_hw_device cuda`) uploads through it:
/// `hwupload_cuda` (each one a CUDA context of its own, ~28 MiB of GPU memory)
/// becomes `hwupload`. Any other command unchanged.
pub fn shared_upload(args: Vec<OsString>) -> Vec<OsString> {
    let devices: Vec<&OsString> = args.windows(2).filter(|w| w[0] == "-init_hw_device").map(|w| &w[1]).collect();
    let cuda =
        matches!(devices.as_slice(), [d] if d.to_str().is_some_and(|d| d == "cuda" || d.starts_with("cuda:") || d.starts_with("cuda=")));
    if !cuda {
        return args;
    }
    let mut out = args;
    for i in 0..out.len().saturating_sub(1) {
        if out[i] != "-filter_complex" {
            continue;
        }
        let Some(graph) = out[i + 1].to_str() else { continue };
        // Twice: neighbouring uploads share the separator between them.
        let once = upload_filter().replace_all(graph, "${1}hwupload${2}").into_owned();
        out[i + 1] = upload_filter().replace_all(&once, "${1}hwupload${2}").into_owned().into();
    }
    out
}

/// `application/x-www-form-urlencoded` value, as `urllib.parse.urlencode`.
pub fn quote_plus(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' => (b as char).to_string(),
            b' ' => "+".to_owned(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}
