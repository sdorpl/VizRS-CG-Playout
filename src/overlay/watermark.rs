//! ErsatzTV's watermarks over a canvas taken over: VizRS Bridge says which
//! ones ErsatzTV draws on the item and where, the overlay draws them in ffmpeg.
//!
//! ErsatzTV Next puts the watermarks of an item with a graphics element into
//! the same canvas as the element. The overlay replaces that canvas with the
//! frames of the sidecar, so without this the watermarks are gone whenever
//! VizRS graphics are on the item. Each watermark becomes an image input of
//! ffmpeg, scaled, with ErsatzTV's opacity (intermittent: 1 s fade in, shown,
//! 1 s fade out, on the time of day), prepared as the graphics are (`format`,
//! `hwupload`…) and overlaid with the graphics overlay over the media and the
//! VizRS graphics: watermarks always on top. No bridge, no token or any error: no watermarks, as before.

use super::*;

/// Most watermarks of one item drawn.
const MAX_WATERMARKS: usize = 8;
/// Largest coordinate and side accepted from the bridge.
const MAX_SIDE: f64 = 16384.0;
/// Most bytes of the bridge's reply.
const MAX_REPLY: u64 = 1 << 20;
/// How long the overlay waits for the bridge (in parallel with the takeover).
pub const BRIDGE_WAIT: Duration = Duration::from_millis(1500);

/// One watermark: image, place in the channel, opacity, timing.
#[derive(Clone, Debug, PartialEq)]
pub struct Watermark {
    /// Local path or `http(s)` URL of the image (ErsatzTV's).
    pub image: String,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    /// 0–1.
    pub opacity: f64,
    /// Intermittent: every N seconds of the day shown for M seconds.
    pub every: Option<(f64, f64)>,
}

/// The watermarks of an item and ErsatzTV's time of day when they came.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Watermarks {
    pub list: Vec<Watermark>,
    /// Seconds since midnight in ErsatzTV when the bridge replied.
    pub time_of_day: f64,
    /// When the reply came.
    pub at: Option<std::time::Instant>,
}

impl Watermarks {
    /// ErsatzTV's time of day now.
    pub(crate) fn time_of_day_now(&self) -> f64 {
        self.time_of_day + self.at.map_or(0.0, |at| at.elapsed().as_secs_f64())
    }
}

/// The bridge's reply (`GET /vizrs-bridge/watermarks/<channel>/<item>`):
/// `{"ok":true,"result":{"time_of_day":…,"watermarks":[…]}}`. Entries that do
/// not make sense are left out.
pub fn parse_watermarks(body: &str) -> io::Result<Watermarks> {
    let reply = json::parse(body).map_err(|e| error(format!("bridge: {e}")))?;
    if reply.get("ok").and_then(Value::as_bool) != Some(true) {
        let message = reply.get("error").and_then(Value::as_str).unwrap_or("no reason");
        return Err(error(format!("bridge: {message}")));
    }
    let result = reply.get("result").ok_or_else(|| error("bridge: no result".into()))?;
    let time_of_day = result.get("time_of_day").and_then(Value::as_f64).filter(|t| t.is_finite() && *t >= 0.0).unwrap_or(0.0);
    let list =
        result.get("watermarks").and_then(Value::as_array).unwrap_or(&[]).iter().filter_map(watermark).take(MAX_WATERMARKS).collect();
    Ok(Watermarks { list, time_of_day, at: Some(std::time::Instant::now()) })
}

fn watermark(entry: &Value) -> Option<Watermark> {
    let number = |key: &str| entry.get(key).and_then(Value::as_f64).filter(|n| n.is_finite());
    let side = |key: &str| number(key).filter(|n| (0.0..=MAX_SIDE).contains(n)).map(|n| n.round() as u32);
    let image = entry.get("image").and_then(Value::as_str)?;
    let local = image.starts_with('/') && !image.contains("/../");
    let remote = image.starts_with("http://") || image.starts_with("https://");
    if image.len() > 4096 || image.contains(['\0', '\n', '\r']) || !(local || remote) {
        return None;
    }
    let (width, height) = (side("width").filter(|&w| w > 0)?, side("height").filter(|&h| h > 0)?);
    let opacity = number("opacity").unwrap_or(1.0).clamp(0.0, 1.0);
    let intermittent = entry.get("mode").and_then(Value::as_str).is_some_and(|m| m.eq_ignore_ascii_case("intermittent"));
    let every = intermittent
        .then(|| Some((number("frequency_seconds")?, number("duration_seconds")?)))
        .flatten()
        .filter(|&(every, shown)| (1.0..=86400.0).contains(&every) && (0.0..=86400.0).contains(&shown));
    if intermittent && every.is_none() {
        return None;
    }
    Some(Watermark { image: image.to_owned(), x: side("x")?, y: side("y")?, width, height, opacity, every })
}

/// The watermarks of the canvas's item from VizRS Bridge in ErsatzTV (the
/// canvas's host), asked in the background; the receiver gets the answer.
pub fn fetch_watermarks(canvas_url: &str, canvas: &Canvas, token: &str) -> mpsc::Receiver<io::Result<Watermarks>> {
    let (sender, receiver) = mpsc::channel();
    let path = format!("/vizrs-bridge/watermarks/{}/{}", quote_plus(&canvas.channel), quote_plus(&canvas.item));
    let (url, token) = (canvas_url.to_owned(), token.to_owned());
    std::thread::spawn(move || {
        let _ = sender.send(bridge_get(&url, &path, &token).and_then(|body| parse_watermarks(&body)));
    });
    receiver
}

/// `GET path` on the host of `base` with the bridge token: the body of a 200.
fn bridge_get(base: &str, path: &str, token: &str) -> io::Result<String> {
    let url = Url::split(base).ok_or_else(|| error("bad canvas URL".into()))?;
    let mut stream = connect(&url)?;
    stream.set_read_timeout(Some(BRIDGE_WAIT))?;
    stream.write_all(format!("GET {path} HTTP/1.0\r\nHost: {}\r\nX-VizRS-Token: {token}\r\n\r\n", url.netloc).as_bytes())?;
    let mut reply = Vec::new();
    stream.take(MAX_REPLY).read_to_end(&mut reply)?;
    let end = reply.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| error("bridge: no reply".into()))?;
    let head = String::from_utf8_lossy(&reply[..end]);
    let status = status_of(head.lines().next().unwrap_or("")).to_owned();
    let body = String::from_utf8_lossy(&reply[end + 4..]).into_owned();
    if status != "200" {
        // The bridge's own message, if any (an older bridge: unknown address).
        return Err(parse_watermarks(&body).err().unwrap_or_else(|| error(format!("bridge: HTTP {status}"))));
    }
    Ok(body)
}

/// The command with the watermarks overlaid on top of the graphics of input
/// `graphics` (`graph_with_watermarks`), their images as inputs after
/// the last one. Unchanged without watermarks or when the graph cannot take them.
pub fn with_watermarks(args: Vec<OsString>, graphics: usize, marks: &Watermarks, rate: &str, matrix: &str) -> Vec<OsString> {
    if marks.list.is_empty() {
        return args;
    }
    let Some(last) = (0..args.len().saturating_sub(1)).rev().find(|&i| args[i] == "-i") else { return args };
    let first = input_number(&args, last) + 1;
    let time_of_day = marks.time_of_day_now();
    let graph = (0..args.len().saturating_sub(1)).filter(|&i| args[i] == "-filter_complex").find_map(|i| {
        let graph = graph_with_watermarks(args[i + 1].to_str()?, graphics, first, &marks.list, matrix, time_of_day)?;
        Some((i, graph))
    });
    let Some((at, graph)) = graph else {
        log("watermarks: the graphics overlay of this command cannot take them; none drawn");
        return args;
    };
    let rate = rate.bytes().all(|b| b.is_ascii_digit() || b == b'/' || b == b'.').then_some(rate).filter(|r| !r.is_empty());
    let mut inputs: Vec<OsString> = Vec::new();
    for mark in &marks.list {
        if mark.image.to_ascii_lowercase().ends_with(".gif") {
            inputs.extend(["-ignore_loop", "0"].map(OsString::from));
        } else {
            inputs.extend(["-loop", "1"].map(OsString::from));
            if let Some(rate) = rate {
                inputs.extend(["-framerate", rate].map(OsString::from));
            }
        }
        inputs.extend(["-i".into(), OsString::from(&mark.image)]);
    }
    let mut out = args;
    out[at + 1] = graph.into();
    out.splice(last + 2..last + 2, inputs);
    out
}

/// `graph` with watermarks `marks` (inputs `first`, `first + 1`…) overlaid
/// right after the graphics of input `graphics`, on top of them: each prepared
/// as the graphics are and overlaid by the same filter with its options at its
/// place. None when the graphics are not overlaid by a filter of two labelled
/// inputs with named options, or are scaled on the way.
pub fn graph_with_watermarks(
    graph: &str,
    graphics: usize,
    first: usize,
    marks: &[Watermark],
    matrix: &str,
    time_of_day: f64,
) -> Option<String> {
    let inputs = [format!("[{graphics}]"), format!("[{graphics}:0]"), format!("[{graphics}:v]"), format!("[{graphics}:v:0]")];
    let chain = graph.split(';').find(|chain| inputs.iter().any(|label| chain.starts_with(label.as_str())));
    let (label, body) = match chain {
        Some(chain) if chain.contains("scale") => return None,
        Some(chain) => {
            let (rest, label) = chain.rsplit_once('[').filter(|(_, l)| l.ends_with(']'))?;
            let source = inputs.iter().find(|l| chain.starts_with(l.as_str()))?;
            (label.trim_end_matches(']').to_owned(), rest[source.len()..].to_owned())
        }
        None => (inputs.iter().find(|label| graph.contains(label.as_str()))?.trim_matches(['[', ']']).to_owned(), String::new()),
    };
    let pattern = format!(r"\[([^\]]+)\]\[{}\](overlay\w*)(=[^\[,;]*)?", regex::escape(&label));
    let found = Regex::new(&pattern).ok()?.captures(graph)?;
    let mut options = Vec::new();
    for option in found.get(3).map_or("", |m| &m.as_str()[1..]).split(':').filter(|o| !o.is_empty()) {
        let (name, _) = option.split_once('=')?;
        if name != "x" && name != "y" {
            options.push(option.to_owned());
        }
    }
    let to_yuv = if matrix.is_empty() {
        "format=yuva420p".to_owned()
    } else {
        format!("scale=out_color_matrix={matrix}:out_range=tv,format=yuva420p")
    };
    let mut prepare = Vec::new();
    let mut overlays = Vec::new();
    for (i, mark) in marks.iter().enumerate() {
        let body = if body.is_empty() { String::new() } else { format!(",{body}") };
        prepare.push(format!(
            "[{}:v]scale={}:{},format=rgba{},{to_yuv}{body}[vz_k{i}]",
            first + i,
            mark.width,
            mark.height,
            opacity(mark, time_of_day)
        ));
        let mut all = options.clone();
        all.extend([format!("x={}", mark.x), format!("y={}", mark.y)]);
        let main = if i == 0 { "vz_kg".to_owned() } else { format!("vz_km{}", i - 1) };
        overlays.push(format!("[{main}][vz_k{i}]{}={}", &found[2], all.join(":")));
    }
    // The graphics overlay ends in `[vz_kg]`; the watermarks go over it one by
    // one, the last one with the rest of its chain (`[v_o0]`, `,format=…,hwupload[v_o0]`).
    let whole = found.get(0)?;
    let after = &graph[whole.end()..];
    let (tail, others) = after.split_at(after.find(';').unwrap_or(after.len()));
    let n = overlays.len();
    let chained: Vec<String> =
        overlays.into_iter().enumerate().map(|(i, o)| if i + 1 < n { format!("{o}[vz_km{i}]") } else { format!("{o}{tail}") }).collect();
    let rest = format!("{}[vz_kg];{}{others}", &graph[..whole.end()], chained.join(";"));
    Some(prepare.into_iter().chain([rest]).collect::<Vec<_>>().join(";"))
}

/// The opacity filter of a watermark (RGBA): none at full opacity, a fixed
/// alpha, or ErsatzTV's intermittent curve on the time of day (`T` of the
/// image input counts from the start of this command). No `;` or `[` inside:
/// the graph is split on them later.
fn opacity(mark: &Watermark, time_of_day: f64) -> String {
    match mark.every {
        Some((every, shown)) => {
            let phase = format!("mod({:.3}+T,{every})", time_of_day % every);
            let curve = format!(
                "if(lt({phase},1),{phase},if(lt({phase},{in_view}),1,if(lt({phase},{out}),{out}-{phase},0)))",
                in_view = 1.0 + shown,
                out = 2.0 + shown
            );
            format!(",geq=r='r(X,Y)':g='g(X,Y)':b='b(X,Y)':a='alpha(X,Y)*{curve}*{:.3}'", mark.opacity)
        }
        None if mark.opacity < 1.0 => format!(",colorchannelmixer=aa={:.3}", mark.opacity),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    fn mark(x: u32, y: u32) -> Watermark {
        Watermark { image: "/config/cache/artwork/watermarks/ab/logo.png".into(), x, y, width: 160, height: 80, opacity: 1.0, every: None }
    }

    #[test]
    fn reply_of_the_bridge() {
        let body = r#"{"ok":true,"result":{"time_of_day":3600.5,"watermarks":[
            {"image":"/c/logo.png","x":1700,"y":40,"width":180,"height":90,"opacity":0.8,"mode":"Permanent"},
            {"image":"/c/odd.png","x":10,"y":10,"width":50,"height":20,"opacity":1,"mode":"Intermittent","frequency_seconds":900,"duration_seconds":30},
            {"image":"relative.png","x":0,"y":0,"width":10,"height":10},
            {"image":"/c/a.png","x":0,"y":0,"width":0,"height":10},
            {"image":"/c/b.png","x":0,"y":0,"width":10,"height":10,"mode":"Intermittent","frequency_seconds":0,"duration_seconds":5},
            {"image":"/c/../etc/x.png","x":0,"y":0,"width":10,"height":10}
        ]}}"#;
        let marks = parse_watermarks(body).unwrap();
        assert_eq!(marks.time_of_day, 3600.5);
        assert_eq!(
            marks.list,
            [
                Watermark { image: "/c/logo.png".into(), x: 1700, y: 40, width: 180, height: 90, opacity: 0.8, every: None },
                Watermark { image: "/c/odd.png".into(), x: 10, y: 10, width: 50, height: 20, opacity: 1.0, every: Some((900.0, 30.0)) },
            ]
        );
        let refused = parse_watermarks(r#"{"ok":false,"error":"pozycja nieznana"}"#).unwrap_err();
        assert!(refused.to_string().contains("pozycja nieznana"));
        assert!(parse_watermarks("<html>").is_err());
        assert!(parse_watermarks(r#"{"ok":true,"result":{}}"#).unwrap().list.is_empty());
    }

    #[test]
    fn watermarks_on_top_of_the_graphics_of_each_graph() {
        let software = "[0:0]scale=8:4[v_m0];[1:0]format=yuva420p[v_s0];[v_m0][v_s0]overlay=x=0:y=0[v_o0]";
        assert_eq!(
            graph_with_watermarks(software, 1, 3, &[mark(1700, 40)], "bt709", 0.0).unwrap(),
            "[3:v]scale=160:80,format=rgba,scale=out_color_matrix=bt709:out_range=tv,format=yuva420p,format=yuva420p[vz_k0];\
             [0:0]scale=8:4[v_m0];[1:0]format=yuva420p[v_s0];\
             [v_m0][v_s0]overlay=x=0:y=0[vz_kg];[vz_kg][vz_k0]overlay=x=1700:y=40[v_o0]"
        );
        // CUDA: prepared and overlaid on the card as the graphics.
        let cuda = "[1:0]format=yuva420p,hwupload_cuda[v_s0];[v_m0][v_s0]overlay_cuda=x=0:y=0[v_o0]";
        let two = [mark(10, 20), Watermark { opacity: 0.5, ..mark(30, 40) }];
        assert_eq!(
            graph_with_watermarks(cuda, 1, 2, &two, "", 0.0).unwrap(),
            "[2:v]scale=160:80,format=rgba,format=yuva420p,format=yuva420p,hwupload_cuda[vz_k0];\
             [3:v]scale=160:80,format=rgba,colorchannelmixer=aa=0.500,format=yuva420p,format=yuva420p,hwupload_cuda[vz_k1];\
             [1:0]format=yuva420p,hwupload_cuda[v_s0];\
             [v_m0][v_s0]overlay_cuda=x=0:y=0[vz_kg];[vz_kg][vz_k0]overlay_cuda=x=10:y=20[vz_km0];\
             [vz_km0][vz_k1]overlay_cuda=x=30:y=40[v_o0]"
        );
        // VAAPI: the options of the overlay kept, the upload after it stays with the graphics.
        let vaapi = "[1:0]format=yuva420p[v_s0];[v_m0][v_s0]overlay=format=0,format=nv12|vaapi,hwupload[v_o0]";
        let graph = graph_with_watermarks(vaapi, 1, 2, &[mark(4, 6)], "", 0.0).unwrap();
        assert!(
            graph.contains("[v_m0][v_s0]overlay=format=0[vz_kg];[vz_kg][vz_k0]overlay=format=0:x=4:y=6,format=nv12|vaapi,hwupload[v_o0]"),
            "{graph}"
        );
        // Graphics used directly.
        assert_eq!(
            graph_with_watermarks("[0][1]overlay", 1, 2, &[mark(4, 6)], "", 0.0).unwrap(),
            "[2:v]scale=160:80,format=rgba,format=yuva420p[vz_k0];[0][1]overlay[vz_kg];[vz_kg][vz_k0]overlay=x=4:y=6"
        );
        // Positional options, graphics scaled on the way, no overlay of the graphics: nothing drawn.
        assert_eq!(graph_with_watermarks("[0][1]overlay=5:5", 1, 2, &[mark(0, 0)], "", 0.0), None);
        assert_eq!(graph_with_watermarks("[1:0]scale=1920:1080[g];[0][g]overlay", 1, 2, &[mark(0, 0)], "", 0.0), None);
        assert_eq!(graph_with_watermarks("[0]scale=8:4[v]", 1, 2, &[mark(0, 0)], "", 0.0), None);
    }

    #[test]
    fn intermittent_follows_the_time_of_day() {
        let blink = Watermark { every: Some((900.0, 30.0)), opacity: 0.8, ..mark(0, 0) };
        let filter = opacity(&blink, 3605.25);
        // 3605.25 s of the day = 5.25 s into a 15 min period; 1 s fade in, 30 s shown, 1 s fade out.
        assert!(filter.contains("mod(5.250+T,900)"), "{filter}");
        assert!(filter.contains("if(lt(mod(5.250+T,900),31),1,if(lt(mod(5.250+T,900),32),32-mod(5.250+T,900),0))"), "{filter}");
        assert!(filter.ends_with("*0.800'"), "{filter}");
        assert!(!filter.contains(';') && !filter.contains('['));
        assert_eq!(opacity(&mark(0, 0), 0.0), "");
    }

    #[test]
    fn images_as_inputs_after_the_last_one() {
        let command = args(&[
            "-i",
            "/m/a.mp4",
            "-f",
            "rawvideo",
            "-framerate",
            "50",
            "-i",
            "pipe:5",
            "-filter_complex",
            "[1:0]format=yuva420p[v_s0];[0:0][v_s0]overlay=x=0:y=0[v]",
            "-map",
            "[v]",
            "-map",
            "0:a",
            "o.m3u8",
        ]);
        let gif = Watermark { image: "/c/anim.GIF".into(), ..mark(8, 8) };
        let marks = Watermarks { list: vec![mark(1700, 40), gif], time_of_day: 0.0, at: None };
        let out = with_watermarks(command.clone(), 1, &marks, "50", "bt709");
        let text: Vec<&str> = out.iter().map(|a| a.to_str().unwrap()).collect();
        assert_eq!(
            &text[8..17],
            ["-loop", "1", "-framerate", "50", "-i", "/config/cache/artwork/watermarks/ab/logo.png", "-ignore_loop", "0", "-i"]
        );
        assert_eq!(text[17], "/c/anim.GIF");
        assert!(text[19].starts_with("[2:v]scale=160:80") && text[19].contains(";[3:v]scale=160:80"), "{}", text[19]);
        assert_eq!(&text[20..], ["-map", "[v]", "-map", "0:a", "o.m3u8"]);
        // No watermarks, or a graph that cannot take them: the command as it was.
        assert_eq!(with_watermarks(command.clone(), 1, &Watermarks::default(), "50", ""), command);
        let positional = args(&["-i", "a", "-i", "pipe:5", "-filter_complex", "[0][1]overlay=5:5", "o"]);
        assert_eq!(with_watermarks(positional.clone(), 1, &marks, "50", ""), positional);
    }

    #[test]
    fn areas_and_ram_sync_keep_working_with_watermarks() {
        let graph =
            "[0:0]scale=8:4,format=nv12,hwupload[v_m0];[1:0]format=yuva420p,hwupload_cuda[v_s0];[v_m0][v_s0]overlay_cuda=x=0:y=0[v_o0]";
        let with_marks = graph_with_watermarks(graph, 1, 2, &[mark(1700, 40)], "", 0.0).unwrap();
        // Graphics areas: still found by their label, the watermark keeps its place.
        let (ticker, logo) = (Area { x: 0, y: 930, w: 1920, h: 140 }, Area { x: 1760, y: 40, w: 140, h: 140 });
        let placed = placed_areas(&with_marks, 1, &[(ticker, 0), (logo, 140)]).unwrap();
        assert!(placed.contains("[v_m0][vz_g0]overlay_cuda=x=0:y=930[vz_o0];[vz_o0][vz_g1]overlay_cuda=x=1760:y=40[vz_kg]"), "{placed}");
        assert!(placed.ends_with("[vz_kg][vz_k0]overlay_cuda=x=1700:y=40[v_o0]"), "{placed}");
        // Media synced in RAM: the watermark chain uploads too, but it is not the media.
        let synced = graph_waiting_in_ram(&with_marks, 1).unwrap();
        assert!(synced.contains("[0:0][vz_wt]overlay=x=main_w"), "{synced}");
        assert!(synced.contains("[2:v]scale=160:80,format=rgba,format=yuva420p,format=yuva420p,hwupload_cuda[vz_k0]"), "{synced}");
        // On the CPU (VIZRS_FFMPEG_CUDA=encode): uploads gone, the overlays on the CPU.
        let cpu = cpu_filters(&with_marks).unwrap();
        assert!(cpu.contains("[2:v]scale=160:80,format=rgba,format=yuva420p,format=yuva420p[vz_k0]"), "{cpu}");
        assert!(!cpu.contains("_cuda") && !cpu.contains("hwupload"), "{cpu}");
    }
}
