//! CUDA: wybór według wolnej pamięci GPU, kodowanie na GPU z dekodowaniem na CPU, rozmieszczanie klatek.

use super::*;

/// A command encoding with NVENC that decodes or filters with CUDA.
pub(crate) fn nvenc_with_cuda(args: &[OsString]) -> bool {
    let text: Vec<&str> = args.iter().filter_map(|a| a.to_str()).collect();
    let nvenc = text.windows(2).any(|w| CODEC.contains(&w[0]) && w[1].ends_with("_nvenc"));
    nvenc && (text.windows(2).any(|w| w[0] == "-hwaccel" && w[1] == "cuda") || text.iter().any(|a| a.contains("_cuda")))
}

/// `VIZRS_FFMPEG_CUDA=auto`: `overlay` with at least `min_free` MiB free on
/// the card or when that is unknown, `encode` below (logged).
pub fn auto_cuda(free: Option<u64>, min_free: u64) -> Cuda {
    match free {
        Some(free) if free < min_free => {
            log(format_args!("GPU memory {free} MiB free (< {min_free}): CUDA filters on the CPU (encode)"));
            Cuda::Encode
        }
        _ => Cuda::Overlay,
    }
}

/// Free memory of the first card in MiB from `nvidia-smi` (at most 1 s); None = unknown.
pub(crate) fn free_gpu_memory() -> Option<u64> {
    let mut query = Command::new("nvidia-smi")
        .args(["--query-gpu=memory.free", "--format=csv,noheader,nounits"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = std::time::Instant::now();
    loop {
        match query.try_wait().ok()? {
            Some(status) if status.success() => break,
            Some(_) => return None,
            None if started.elapsed() > GPU_QUERY_WAIT => {
                let _ = query.kill();
                let _ = query.wait();
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    let mut text = String::new();
    query.stdout.take()?.read_to_string(&mut text).ok()?;
    parse_free_memory(&text)
}

/// The first card's free MiB from `nvidia-smi --query-gpu=memory.free --format=csv,noheader,nounits`.
pub fn parse_free_memory(text: &str) -> Option<u64> {
    text.lines().next()?.trim().parse().ok()
}

/// Options of ErsatzTV's CUDA decoding left out with `VIZRS_FFMPEG_CUDA=encode` (with their value).
pub(crate) const CUDA_OPTIONS: [&str; 4] = ["-hwaccel", "-hwaccel_output_format", "-hwaccel_device", "-filter_hw_device"];

/// A command encoding with NVENC, its CUDA decoding and filters moved to the
/// CPU (`VIZRS_FFMPEG_CUDA=encode`); any other command, or one it cannot
/// rewrite, unchanged.
pub fn cuda_encode_only(args: Vec<OsString>) -> Vec<OsString> {
    let text: Option<Vec<&str>> = args.iter().map(|a| a.to_str()).collect();
    let Some(text) = text else { return args };
    let nvenc = text.windows(2).any(|w| ["-c:v", "-vcodec", "-codec:v"].contains(&w[0]) && w[1].ends_with("_nvenc"));
    let cuda = text.windows(2).any(|w| w[0] == "-hwaccel" && w[1] == "cuda") || text.iter().any(|a| a.contains("_cuda"));
    if !nvenc || !cuda {
        return args;
    }
    let mut out = Vec::with_capacity(args.len());
    let mut i = 0;
    while i < text.len() {
        let arg = text[i];
        let value = text.get(i + 1).copied().unwrap_or("");
        if CUDA_OPTIONS.contains(&arg) && i + 1 < text.len() {
            i += 2;
            continue;
        }
        if arg == "-init_hw_device" && value.starts_with("cuda") {
            i += 2;
            continue;
        }
        if ["-filter_complex", "-vf", "-filter:v"].contains(&arg) && i + 1 < text.len() {
            match cpu_filters(value) {
                Some(filters) => out.extend([OsString::from(arg), OsString::from(filters)]),
                None => return args,
            }
            i += 2;
            continue;
        }
        out.push(OsString::from(arg));
        i += 1;
    }
    out
}

/// A command encoding with NVENC from CUDA decoding, decoding on the CPU
/// (`VIZRS_FFMPEG_CUDA=overlay`): the decoded frames go to the card where
/// ErsatzTV's filters expect them, its CUDA filters and the overlay of the
/// graphics stay there, and the frames for the encoder come back to the CPU
/// (`place_frames`). Any other command unchanged.
pub fn cuda_decode_on_cpu(args: Vec<OsString>) -> Vec<OsString> {
    let text: Option<Vec<&str>> = args.iter().map(|a| a.to_str()).collect();
    let Some(text) = text else { return args };
    let nvenc = text.windows(2).any(|w| ["-c:v", "-vcodec", "-codec:v"].contains(&w[0]) && w[1].ends_with("_nvenc"));
    let decode = text.windows(2).any(|w| w[0] == "-hwaccel" && w[1] == "cuda");
    if !nvenc || !decode {
        return args;
    }
    let mapped: Vec<&str> = text
        .windows(2)
        .filter(|w| w[0] == "-map" && w[1].starts_with('['))
        .map(|w| w[1].trim_start_matches('[').trim_end_matches(']'))
        .collect();
    let mut card = Vec::new();
    let mut out = Vec::with_capacity(args.len() + 2);
    let mut i = 0;
    while i < text.len() {
        let arg = text[i];
        if DECODE_OPTIONS.contains(&arg) && i + 1 < text.len() {
            i += 2;
            continue;
        }
        if ["-filter_complex", "-vf", "-filter:v"].contains(&arg) && i + 1 < text.len() {
            let simple = arg != "-filter_complex";
            let graph: Vec<String> = text[i + 1].split(';').map(|chain| place_frames(chain, &mut card, &mapped, simple)).collect();
            out.extend([OsString::from(arg), OsString::from(graph.join(";"))]);
            i += 2;
            continue;
        }
        out.push(OsString::from(arg));
        i += 1;
    }
    out
}

/// One chain of a graph without CUDA decoding. The frames of an input stream
/// (`[0:0]…`, or a `-vf` without labels) are on the CPU now: an upload goes
/// before the first filter that needs them on the card, a download of them is
/// dropped. Frames on the card at the end of a chain for the encoder (`-map`,
/// or a `-vf`) come back to the CPU (`hwdownload`): the encoder waits for the
/// muxer – e.g. for 3 s of `loudnorm` lookahead in the audio – and the frames
/// waiting for it on the card filled its memory (`CUDA_ERROR_OUT_OF_MEMORY`
/// on a Quadro P400 within seconds); in RAM they cost nothing. `card`: the
/// labels whose frames are on the card, filled chain by chain.
pub(crate) fn place_frames(chain: &str, card: &mut Vec<String>, mapped: &[&str], simple: bool) -> String {
    let (inputs, rest) = chain.split_at(leading_labels(chain));
    let tail = trailing_labels(rest);
    let (body, outputs) = rest.split_at(rest.len() - tail);
    let labels =
        |text: &str| -> Vec<String> { text.split(']').filter(|l| !l.is_empty()).map(|l| l.trim_start_matches('[').to_owned()).collect() };
    let (ins, outs) = (labels(inputs), labels(outputs));
    let stream = |label: &String| label.split(':').next().is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    let decoded = ins.len() <= 1 && ins.iter().all(stream);
    let mut on_card = !decoded && ins.iter().any(|l| card.contains(l));
    let mut filters: Vec<String> = Vec::new();
    for filter in body.split(',').filter(|f| !f.is_empty()) {
        let name = filter.split_once('=').map_or(filter, |(name, _)| name);
        let formatted = filters.last().is_some_and(|f| f.starts_with("format="));
        match name {
            "hwupload" | "hwupload_cuda" => {
                if decoded && !on_card && !formatted {
                    filters.push(UPLOAD_FORMATS.into());
                }
                on_card = true;
            }
            "hwdownload" if decoded && !on_card => continue,
            "hwdownload" => on_card = false,
            cuda if cuda.ends_with("_cuda") => {
                if decoded && !on_card {
                    filters.extend([UPLOAD_FORMATS.into(), "hwupload".into()]);
                }
                on_card = true;
            }
            _ => {}
        }
        filters.push(filter.to_owned());
    }
    let encoder = if simple { outs.len() <= 1 } else { outs.len() == 1 && mapped.contains(&outs[0].as_str()) };
    if on_card && encoder {
        filters.extend(["hwdownload".into(), UPLOAD_FORMATS.into()]);
        on_card = false;
    }
    if on_card {
        card.extend(outs);
    }
    format!("{inputs}{}{outputs}", filters.join(","))
}
