// The VizRS native renderer (`vizrs-render stream`, a program of VizRS CG
// System) as a child process: it reads the VizRS events, fonts and assets
// itself, like the /output page, and draws a frame when asked (DOKUMENTACJA
// §1, VizRS DOKUMENTACJA §10.1). Only the protocol is used, no VizRS code.
//
// stdin: `F <time_ms> <wall_ms> [cf]` per frame; stdout: a 16-byte header
// (`VZR1`, kind, flags, 2 zero bytes, width and height u32 LE), then RGBA
// (straight alpha) for kind 1. Kinds: 0 unchanged, 1 picture, 2 empty,
// 3 not drawn (`c`: something on air needs Chromium), 4 picture of a region:
// x, y, width, height (u32 LE), its RGBA, the rest transparent, 5 pictures of
// several `--area`: their count (u32 LE), then each as in kind 4, in channel
// pixels, 6 a patch of the last picture (kind 1 or 4, or a patch) – asked for
// with `p`: a count (u32 LE), then each rectangle as in kind 4, whose pixels
// replace those of the last picture. Flags: bit 0 all on air
// drawn natively, bit 1 connected to VizRS.

use std::io::{BufReader, Read, Write};
use std::os::unix::process::CommandExt;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::area::{Area, cut_to};
use crate::json::{self, Value};
use crate::packets::{Rect, crop};
use crate::rects::visible_rects;
use crate::sys;

/// Longest wait for one frame; a renderer slower than this is restarted.
pub const FRAME_TIMEOUT: Duration = Duration::from_secs(2);
const MAGIC: &[u8; 4] = b"VZR1";
const MAX_SIDE: u32 = 8192;
const MAX_REGIONS: u32 = 64;

/// Which renderer draws the burn-in (`renderer`, DOKUMENTACJA §5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Renderer {
    /// Headless Chromium only (the default).
    Chromium,
    /// The native renderer only; what it cannot draw is left out.
    Native,
    /// The native renderer while everything on air is native, Chromium (kept ready) otherwise.
    Auto,
}

impl Renderer {
    pub fn parse(spec: &str) -> Result<Renderer, String> {
        match spec.trim().to_ascii_lowercase().as_str() {
            "" | "chromium" => Ok(Renderer::Chromium),
            "native" => Ok(Renderer::Native),
            "auto" => Ok(Renderer::Auto),
            _ => Err(format!("renderer \"{spec}\" (expected chromium, native or auto)")),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Renderer::Chromium => "chromium",
            Renderer::Native => "native",
            Renderer::Auto => "auto",
        }
    }
}

/// `renderer_bin` (`VIZRS_RENDERER_BIN`), else `vizrs-render` next to this program or on `PATH`.
pub fn find_renderer(configured: &str) -> String {
    if !configured.trim().is_empty() {
        return configured.trim().to_owned();
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
        let beside = dir.join("vizrs-render");
        if beside.is_file() {
            return beside.to_string_lossy().into_owned();
        }
    }
    "vizrs-render".into()
}

#[derive(Debug, PartialEq)]
pub enum Kind {
    Unchanged,
    Picture(Vec<u8>),
    /// A picture transparent outside the region (x, y, w, h) of the frame.
    Region(Area, Vec<u8>),
    /// Pictures of the areas, in channel pixels (a renderer given several `--area`).
    Regions(Vec<(Area, Vec<u8>)>),
    Empty,
    /// Something on air needs Chromium; nothing drawn.
    Skipped,
    /// Rectangles (x, y, w, h of the frame) of the last picture with new pixels; the rest unchanged.
    Patch(Vec<(Area, Vec<u8>)>),
}

#[derive(Debug)]
pub struct Reply {
    pub kind: Kind,
    /// Everything on air is drawn natively.
    pub complete: bool,
    pub connected: bool,
    pub width: u32,
    pub height: u32,
}

/// Reads one record; `None` at the end of the stream.
pub fn read_reply(input: &mut impl Read) -> Result<Option<Reply>, String> {
    let mut head = [0u8; 16];
    match input.read_exact(&mut head) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.to_string()),
    }
    if &head[..4] != MAGIC {
        return Err("native renderer: bad record".into());
    }
    let width = u32::from_le_bytes([head[8], head[9], head[10], head[11]]);
    let height = u32::from_le_bytes([head[12], head[13], head[14], head[15]]);
    if width > MAX_SIDE || height > MAX_SIDE {
        return Err(format!("native renderer: frame {width}x{height}"));
    }
    let kind = match head[4] {
        0 => Kind::Unchanged,
        1 => {
            let mut rgba = vec![0u8; width as usize * height as usize * 4];
            input.read_exact(&mut rgba).map_err(|e| e.to_string())?;
            Kind::Picture(rgba)
        }
        2 => Kind::Empty,
        3 => Kind::Skipped,
        4 => {
            let (r, rgba) = read_region(input, width, height)?;
            Kind::Region(r, rgba)
        }
        5 => {
            let mut count = [0u8; 4];
            input.read_exact(&mut count).map_err(|e| e.to_string())?;
            let count = u32::from_le_bytes(count);
            if count > MAX_REGIONS {
                return Err(format!("native renderer: {count} regions"));
            }
            Kind::Regions((0..count).map(|_| read_region(input, width, height)).collect::<Result<_, _>>()?)
        }
        6 => {
            let mut count = [0u8; 4];
            input.read_exact(&mut count).map_err(|e| e.to_string())?;
            let count = u32::from_le_bytes(count);
            if count > MAX_REGIONS {
                return Err(format!("native renderer: {count} patches"));
            }
            Kind::Patch((0..count).map(|_| read_region(input, width, height)).collect::<Result<_, _>>()?)
        }
        other => return Err(format!("native renderer: record kind {other}")),
    };
    Ok(Some(Reply { kind, complete: head[5] & 1 != 0, connected: head[5] & 2 != 0, width, height }))
}

/// One region: x, y, width, height (u32 LE) and its RGBA, inside `width`x`height`.
fn read_region(input: &mut impl Read, width: u32, height: u32) -> Result<(Area, Vec<u8>), String> {
    let mut b = [0u8; 16];
    input.read_exact(&mut b).map_err(|e| e.to_string())?;
    let v = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let r = Area { x: v(0), y: v(4), w: v(8), h: v(12) };
    if u64::from(r.x) + u64::from(r.w) > u64::from(width) || u64::from(r.y) + u64::from(r.h) > u64::from(height) {
        return Err(format!("native renderer: region {}x{}+{}+{} outside {width}x{height}", r.w, r.h, r.x, r.y));
    }
    let mut rgba = vec![0u8; r.w as usize * r.h as usize * 4];
    input.read_exact(&mut rgba).map_err(|e| e.to_string())?;
    Ok((r, rgba))
}

/// Wall-clock milliseconds at monotonic time `ms` (`sys::monotonic_ms`), plus `offset` seconds.
pub fn wall_ms(ms: f64, offset: f64) -> f64 {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0);
    now + (ms - sys::monotonic_ms()) + offset * 1000.0
}

/// The running renderer.
pub struct Native {
    child: Child,
    stdin: ChildStdin,
    replies: Receiver<Result<Reply, String>>,
    /// The last picture (its region of the frame and RGBA), which a patch updates.
    base: Option<(Area, Vec<u8>)>,
}

impl Native {
    /// Starts `vizrs-render stream` for the /output page `url`, the channel size and its graphics areas.
    pub fn start(bin: &str, url: &str, size: (u32, u32), areas: &[Area]) -> Result<Native, String> {
        let mut command = Command::new(bin);
        command.args(["stream", "--url", url, "--size", &format!("{}x{}", size.0, size.1)]);
        for a in areas {
            command.args(["--area", &format!("{},{},{},{}", a.x, a.y, a.w, a.h)]);
        }
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit());
        // SAFETY: child_setup only makes async-signal-safe calls.
        unsafe {
            command.pre_exec(|| sys::child_setup(&[]));
        }
        let mut child = command.spawn().map_err(|e| format!("start {bin}: {e}"))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let (tx, replies) = mpsc::channel();
        let _ = std::thread::Builder::new().name("native-frames".into()).spawn(move || {
            let mut input = BufReader::with_capacity(1 << 20, stdout);
            loop {
                let reply = match read_reply(&mut input) {
                    Ok(Some(reply)) => Ok(reply),
                    Ok(None) => Err("native renderer exited".into()),
                    Err(e) => Err(e),
                };
                let end = reply.is_err();
                if tx.send(reply).is_err() || end {
                    break;
                }
            }
        });
        Ok(Native { child, stdin, replies, base: None })
    }

    /// The frame at monotonic time `ms`: `complete_only` = only when all on air is native,
    /// `force` = the picture even when unchanged.
    pub fn frame(&mut self, ms: f64, wall: f64, complete_only: bool, force: bool) -> Result<Reply, String> {
        self.request(ms, wall, complete_only, force)?;
        self.receive(Instant::now() + FRAME_TIMEOUT)
    }

    /// Asks for the frame at `ms` (see `frame`); `receive` takes it, so several renderers draw at once.
    pub fn request(&mut self, ms: f64, wall: f64, complete_only: bool, force: bool) -> Result<(), String> {
        // `p`: patches welcome (a renderer without them ignores it and sends whole pictures)
        let flags = format!("{}{}p", if complete_only { "c" } else { "" }, if force { "f" } else { "" });
        writeln!(self.stdin, "F {ms:.3} {wall:.3} {flags}").and_then(|_| self.stdin.flush()).map_err(|e| format!("native renderer: {e}"))
    }

    /// The frame asked for, by `deadline`.
    pub fn receive(&mut self, deadline: Instant) -> Result<Reply, String> {
        match self.replies.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(reply) => reply,
            Err(RecvTimeoutError::Timeout) => Err("native renderer: no frame in time".into()),
            Err(RecvTimeoutError::Disconnected) => Err("native renderer exited".into()),
        }
    }

    /// The last picture (its region of the frame and RGBA), the base of the next patch.
    pub fn base(&self) -> Option<(Area, &[u8])> {
        self.base.as_ref().map(|(r, px)| (*r, px.as_slice()))
    }
    /// Keeps the picture (`region` of the frame, RGBA) as the base of the next patch.
    pub fn keep(&mut self, region: Area, rgba: Vec<u8>) {
        self.base = Some((region, rgba));
    }

    /// No base: the renderer sends a whole picture before patching again.
    pub fn forget(&mut self) {
        self.base = None;
    }

    /// The last picture with `patches` applied: its region of the frame and RGBA.
    pub fn patch(&mut self, patches: &[(Area, Vec<u8>)]) -> Result<(Area, &[u8]), String> {
        let base = self.base.as_mut().ok_or("native renderer: a patch without a picture")?;
        apply_patch(base, patches)?;
        Ok((base.0, &base.1))
    }
}

/// Puts the pixels of `patches` (rectangles of the frame) into the picture `base` (its region, RGBA).
pub fn apply_patch((region, rgba): &mut (Area, Vec<u8>), patches: &[(Area, Vec<u8>)]) -> Result<(), String> {
    for (p, px) in patches {
        if p.x < region.x || p.y < region.y || p.x + p.w > region.x + region.w || p.y + p.h > region.y + region.h {
            return Err(format!("native renderer: patch {}x{}+{}+{} outside the picture", p.w, p.h, p.x, p.y));
        }
        let row = p.w as usize * 4;
        for r in 0..p.h as usize {
            let to = (((p.y - region.y) as usize + r) * region.w as usize + (p.x - region.x) as usize) * 4;
            rgba[to..to + row].copy_from_slice(&px[r * row..(r + 1) * row]);
        }
    }
    Ok(())
}

impl Drop for Native {
    fn drop(&mut self) {
        let _ = writeln!(self.stdin, "Q");
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Where a VizRS layer is in the stack of the /output page (`layerZ`): back,
/// main and front, `z<N>_name` (N 0–99) and any other name between main and
/// front. Layers of the same place stack in the order they came.
pub fn layer_z(name: &str) -> u32 {
    match name {
        "back" => 1000,
        "main" => 2000,
        "front" => 3000,
        _ => {
            let n = name.strip_prefix('z').and_then(|rest| {
                let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
                (!digits.is_empty() && digits.len() <= 2 && rest[digits.len()..].starts_with('_')).then(|| digits.parse().unwrap_or(0))
            });
            2500 + n.unwrap_or(0)
        }
    }
}

/// `names` without repeats, bottom of the stack first (`layer_z`, then as listed).
pub fn stack(names: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in names {
        if !name.is_empty() && !out.contains(&name) {
            out.push(name);
        }
    }
    out.sort_by_key(|n| layer_z(n));
    out
}

/// The layers of a show (`GET /api/show`: the `layer` of each scene), bottom first.
pub fn show_layers(show_json: &str) -> Result<Vec<String>, String> {
    let show = json::parse(show_json).map_err(|e| format!("show: {e}"))?;
    let scenes = show.get("scenes").ok_or("show: no scenes")?;
    let list: Vec<&Value> = match scenes {
        Value::Array(items) => items.iter().collect(),
        Value::Object(members) => members.iter().map(|(_, v)| v).collect(),
        _ => return Err("show: scenes are not a list".into()),
    };
    Ok(stack(list.into_iter().map(|scene| scene.get("layer").and_then(Value::as_str).unwrap_or("main").to_owned())))
}

/// The layers on the air of a `STATE` reply.
pub fn state_layers(state_reply: &str) -> Vec<String> {
    let Some(state) = state_reply.strip_prefix("OK").and_then(|s| json::parse(s.trim()).ok()) else { return Vec::new() };
    let Some(layers) = state.get("layers").and_then(Value::as_array) else { return Vec::new() };
    layers.iter().filter_map(|l| l.get("layer").and_then(Value::as_str)).map(str::to_owned).collect()
}

/// The /output page `url` showing only `layer`.
pub fn layer_url(url: &str, layer: &str) -> String {
    let encoded: String = layer
        .bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect();
    format!("{url}{}layers={encoded}", if url.contains('?') { '&' } else { '?' })
}

/// What a frame of `Layered` gives.
#[derive(Debug)]
pub enum Drawn {
    /// Something on air needs Chromium (`complete_only`).
    Skipped,
    Unchanged,
    /// The new picture: visible rectangles in channel pixels.
    Changed(Vec<Rect>),
}

/// The layers of the show drawn by one native renderer each (`native_layers`).
pub type LayerList = Arc<Mutex<Vec<String>>>;

/// One renderer: its layer ("" = all of them), and the visible rectangles of its last picture.
struct Layer {
    name: String,
    native: Native,
    rects: Vec<Rect>,
    /// Started now: its first frame is asked for in full.
    fresh: bool,
}

/// The native renderers of a capture: one per layer of the show (`layers`,
/// `native_layers`) or one for all. Each frame is asked of all of them at once,
/// so they draw in parallel, and a standing layer answers "unchanged" and costs
/// nothing; the picture is their rectangles, bottom layer first, with the
/// overlapping ones of different layers blended into one ("over", straight alpha).
pub struct Layered {
    bin: String,
    url: String,
    size: (u32, u32),
    areas: Vec<Area>,
    /// The part of the channel the renderers draw: the area, else all of it.
    part: Area,
    names: Option<LayerList>,
    layers: Vec<Layer>,
    blends: Blends,
}

impl Layered {
    /// The renderers of the /output page `url`; `names` = the layers of the show
    /// (none, empty or several `areas`: one renderer for all).
    pub fn start(bin: &str, url: &str, size: (u32, u32), areas: &[Area], part: Area, names: Option<LayerList>) -> Result<Layered, String> {
        let names = names.filter(|_| areas.len() <= 1);
        let mut layered = Layered {
            bin: bin.to_owned(),
            url: url.to_owned(),
            size,
            areas: areas.to_vec(),
            part,
            names,
            layers: Vec::new(),
            blends: Vec::new(),
        };
        layered.follow()?;
        Ok(layered)
    }

    /// The layers drawn now (empty = one renderer for all).
    pub fn layer_names(&self) -> Vec<String> {
        self.layers.iter().map(|l| l.name.clone()).filter(|n| !n.is_empty()).collect()
    }

    /// Renderers for the layers of the show as they are now: new ones started, gone ones stopped.
    fn follow(&mut self) -> Result<(), String> {
        let wanted: Vec<String> = match &self.names {
            Some(list) => list.lock().unwrap_or_else(|e| e.into_inner()).clone(),
            None => Vec::new(),
        };
        let wanted = if wanted.is_empty() { vec![String::new()] } else { wanted };
        if self.layers.len() == wanted.len() && self.layers.iter().zip(&wanted).all(|(l, w)| l.name == *w) {
            return Ok(());
        }
        let mut old: Vec<Layer> = std::mem::take(&mut self.layers);
        for name in wanted {
            let layer = match old.iter().position(|l| l.name == name) {
                Some(i) => old.remove(i),
                None => {
                    let url = if name.is_empty() { self.url.clone() } else { layer_url(&self.url, &name) };
                    Layer { native: Native::start(&self.bin, &url, self.size, &self.areas)?, name, rects: Vec::new(), fresh: true }
                }
            };
            self.layers.push(layer);
        }
        if !self.layer_names().is_empty() {
            crate::debug!("native renderer: one per layer: {}", self.layer_names().join(", "));
        }
        Ok(())
    }

    /// The frame at monotonic time `ms` from all the renderers (see `Native::frame`).
    pub fn frame(&mut self, ms: f64, wall: f64, complete_only: bool, force: bool) -> Result<Drawn, String> {
        self.follow()?;
        for layer in &mut self.layers {
            layer.native.request(ms, wall, complete_only, force || layer.fresh)?;
        }
        let deadline = Instant::now() + FRAME_TIMEOUT;
        let (mut skipped, mut changed) = (false, false);
        let (part, areas) = (self.part, self.areas.clone());
        for layer in &mut self.layers {
            let reply = layer.native.receive(deadline)?;
            if matches!(reply.kind, Kind::Picture(_) | Kind::Region(..) | Kind::Patch(_)) && (reply.width, reply.height) != (part.w, part.h)
            {
                return Err(format!("native renderer: frame {}x{} for {}x{}", reply.width, reply.height, part.w, part.h));
            }
            let in_channel = |r: Area| Area { x: part.x + r.x, y: part.y + r.y, w: r.w, h: r.h };
            // The picture (Picture, Region) stays as the base of the next patch: with a patch only the
            // changed rectangles cross the pipe (a crawl under a large board: 0.6 MB a frame, not 4 MB).
            match reply.kind {
                Kind::Skipped => {
                    skipped = true;
                    continue;
                }
                Kind::Unchanged => {}
                Kind::Empty => {
                    layer.native.forget();
                    layer.rects.clear();
                    changed = true;
                }
                Kind::Picture(rgba) => {
                    layer.rects = picture_rects(&rgba, part, &areas);
                    layer.native.keep(Area { x: 0, y: 0, w: part.w, h: part.h }, rgba);
                    changed = true;
                }
                Kind::Regions(regions) => {
                    layer.native.forget();
                    layer.rects = regions.iter().flat_map(|(region, rgba)| picture_rects(rgba, *region, &areas)).collect();
                    changed = true;
                }
                Kind::Region(r, rgba) => {
                    layer.rects = picture_rects(&rgba, in_channel(r), &areas);
                    layer.native.keep(r, rgba);
                    changed = true;
                }
                Kind::Patch(patches) => {
                    let (r, rgba) = layer.native.patch(&patches)?;
                    layer.rects = picture_rects(rgba, in_channel(r), &areas);
                    changed = true;
                }
            }
            layer.fresh = false;
        }
        if skipped {
            return Ok(Drawn::Skipped);
        }
        if !changed {
            return Ok(Drawn::Unchanged);
        }
        Ok(Drawn::Changed(self.compose()))
    }

    /// The rectangles of all the layers, bottom first; see `compose`.
    fn compose(&mut self) -> Vec<Rect> {
        let layers: Vec<&[Rect]> = self.layers.iter().map(|l| l.rects.as_slice()).collect();
        compose(&layers, self.size, &mut self.blends)
    }
}

/// Blends of overlapping rectangles: the pixels they came from (held, so
/// their memory is not another's), and the result.
pub type Blends = Vec<(Vec<Arc<[u8]>>, Vec<Rect>)>;

/// The rectangles of the layers (bottom first) as one picture: rectangles of
/// different layers that overlap blended into the visible rectangles of their
/// group, the rest as they are (their pixels shared). A blend of the same
/// pixels as last time (`blends`) is not made again.
pub fn compose(layers: &[&[Rect]], size: (u32, u32), blends: &mut Blends) -> Vec<Rect> {
    let all: Vec<(usize, &Rect)> = layers.iter().enumerate().flat_map(|(i, rects)| rects.iter().map(move |r| (i, r))).collect();
    if layers.len() < 2 {
        blends.clear();
        return all.into_iter().map(|(_, r)| r.clone()).collect();
    }
    // Groups of overlapping rectangles (union-find).
    let mut parent: Vec<usize> = (0..all.len()).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for i in 0..all.len() {
        for j in i + 1..all.len() {
            if all[i].0 != all[j].0 && overlap(all[i].1, all[j].1) {
                let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                parent[a] = b;
            }
        }
    }
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut group_of = vec![usize::MAX; all.len()];
    for i in 0..all.len() {
        let r = root(&mut parent, i);
        if group_of[r] == usize::MAX {
            group_of[r] = groups.len();
            groups.push(Vec::new());
        }
        groups[group_of[r]].push(i);
    }
    let mut out = Vec::with_capacity(all.len());
    let mut kept = Vec::new();
    for group in groups {
        if group.len() == 1 {
            out.push(all[group[0]].1.clone());
            continue;
        }
        // Members in stack order (layer, then as drawn); a blend of the same pixels is kept.
        let members: Vec<&Rect> = group.iter().map(|&i| all[i].1).collect();
        let key: Vec<Arc<[u8]>> = members.iter().map(|r| Arc::clone(&r.pixels)).collect();
        let same = |k: &[Arc<[u8]>]| k.len() == key.len() && k.iter().zip(&key).all(|(a, b)| Arc::ptr_eq(a, b));
        let blended = match blends.iter().position(|(k, _)| same(k)) {
            Some(i) => blends.swap_remove(i).1,
            None => blend(&members, size),
        };
        out.extend(blended.iter().cloned());
        kept.push((key, blended));
    }
    *blends = kept;
    out
}

fn overlap(a: &Rect, b: &Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

/// Rectangles (bottom first) drawn over one another on their bounding box,
/// straight alpha "over" like the /output page; its visible rectangles.
fn blend(rects: &[&Rect], (cw, ch): (u32, u32)) -> Vec<Rect> {
    let x0 = rects.iter().map(|r| r.x).min().unwrap_or(0);
    let y0 = rects.iter().map(|r| r.y).min().unwrap_or(0);
    let x1 = rects.iter().map(|r| r.x + r.w).max().unwrap_or(0).min(cw);
    let y1 = rects.iter().map(|r| r.y + r.h).max().unwrap_or(0).min(ch);
    if x1 <= x0 || y1 <= y0 {
        return Vec::new();
    }
    let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
    let mut canvas = vec![0u8; w * h * 4];
    for rect in rects {
        let stride = rect.w as usize * 4;
        for row in 0..rect.h as usize {
            let y = rect.y as usize + row;
            if y >= y1 as usize {
                break;
            }
            let cols = (rect.w.min(x1 - rect.x)) as usize;
            let src = &rect.pixels[row * stride..row * stride + cols * 4];
            let at = ((y - y0 as usize) * w + (rect.x - x0) as usize) * 4;
            for (d, s) in canvas[at..at + cols * 4].chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                over(d, s);
            }
        }
    }
    picture_rects(&canvas, Area { x: x0, y: y0, w: w as u32, h: h as u32 }, &[])
}

/// `src` over `dst` (RGBA, straight alpha), into `dst`.
fn over(dst: &mut [u8], src: &[u8]) {
    let sa = u32::from(src[3]);
    if sa == 255 || dst[3] == 0 {
        if sa != 0 {
            dst.copy_from_slice(src);
        }
        return;
    }
    if sa == 0 {
        return;
    }
    let da = u32::from(dst[3]);
    // Alpha out, in 255ths of 255: sa*255 + da*(255-sa).
    let out = sa * 255 + da * (255 - sa);
    for c in 0..3 {
        let v = u32::from(src[c]) * sa * 255 + u32::from(dst[c]) * da * (255 - sa);
        dst[c] = ((v + out / 2) / out) as u8;
    }
    dst[3] = ((out + 127) / 255) as u8;
}

/// Visible rectangles of a native picture of `part` (channel coordinates), cut to `areas`.
pub fn picture_rects(rgba: &[u8], part: Area, areas: &[Area]) -> Vec<Rect> {
    let (w, h) = (part.w, part.h);
    if rgba.len() != w as usize * h as usize * 4 {
        return Vec::new();
    }
    let mut rects: Vec<Rect> = visible_rects(rgba, w, h)
        .into_iter()
        .map(|(x, y, rw, rh)| Rect { x: x + part.x, y: y + part.y, w: rw, h: rh, pixels: crop(rgba, w, (x, y, rw, rh)).into() })
        .collect();
    if areas.len() > 1 {
        rects = cut_to(rects, areas);
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regions_record() {
        let mut bytes = b"VZR1".to_vec();
        bytes.extend([5, 3, 0, 0]);
        bytes.extend(100u32.to_le_bytes());
        bytes.extend(50u32.to_le_bytes());
        bytes.extend(2u32.to_le_bytes());
        for (x, y, px) in [(10u32, 20u32, [1u8, 2, 3, 4]), (90, 40, [5, 6, 7, 8])] {
            for v in [x, y, 1, 1] {
                bytes.extend(v.to_le_bytes());
            }
            bytes.extend(px);
        }
        let reply = read_reply(&mut &bytes[..]).unwrap().unwrap();
        assert!(reply.complete && reply.connected);
        assert_eq!(
            reply.kind,
            Kind::Regions(vec![
                (Area { x: 10, y: 20, w: 1, h: 1 }, vec![1, 2, 3, 4]),
                (Area { x: 90, y: 40, w: 1, h: 1 }, vec![5, 6, 7, 8]),
            ])
        );
    }

    #[test]
    fn region_outside_the_channel_is_an_error() {
        let mut bytes = b"VZR1".to_vec();
        bytes.extend([5, 0, 0, 0]);
        bytes.extend(10u32.to_le_bytes());
        bytes.extend(10u32.to_le_bytes());
        bytes.extend(1u32.to_le_bytes());
        for v in [9u32, 0, 2, 1] {
            bytes.extend(v.to_le_bytes());
        }
        assert!(read_reply(&mut &bytes[..]).is_err());
    }

    fn record(kind: u8, flags: u8, w: u32, h: u32, px: &[u8]) -> Vec<u8> {
        let mut out = b"VZR1".to_vec();
        out.extend([kind, flags, 0, 0]);
        out.extend(w.to_le_bytes());
        out.extend(h.to_le_bytes());
        out.extend(px);
        out
    }

    #[test]
    fn patches_are_read_and_applied() {
        // rekord 6: ramka 4x3, jedna łata 2x1 w (1, 1)
        let mut patch = record(6, 1, 4, 3, &1u32.to_le_bytes());
        patch.extend([1u32, 1, 2, 1].iter().flat_map(|v| v.to_le_bytes()));
        patch.extend([7u8; 8]);
        let reply = read_reply(&mut &patch[..]).unwrap().unwrap();
        let Kind::Patch(patches) = reply.kind else { panic!("{:?}", reply.kind) };
        assert_eq!(patches, vec![(Area { x: 1, y: 1, w: 2, h: 1 }, vec![7; 8])]);
        // obraz 3x2 od (1, 0): łata w drugim wierszu, w dwóch pierwszych pikselach
        let mut base = (Area { x: 1, y: 0, w: 3, h: 2 }, vec![0u8; 3 * 2 * 4]);
        apply_patch(&mut base, &patches).unwrap();
        assert_eq!(&base.1[..12], &[0; 12]);
        assert_eq!(&base.1[12..20], &[7; 8]);
        assert_eq!(&base.1[20..24], &[0; 4]);
        let outside = vec![(Area { x: 0, y: 0, w: 1, h: 1 }, vec![1; 4])];
        assert!(apply_patch(&mut base, &outside).is_err());
    }

    #[test]
    fn records_are_read() {
        let mut data = record(1, 3, 1, 1, &[1, 2, 3, 4]);
        data.extend(record(3, 2, 1, 1, &[]));
        let mut region = record(4, 1, 10, 10, &[]);
        for v in [2u32, 3, 1, 1] {
            region.extend(v.to_le_bytes());
        }
        region.extend([9, 9, 9, 9]);
        data.extend(region);
        let mut input = data.as_slice();
        let a = read_reply(&mut input).unwrap().unwrap();
        assert_eq!(a.kind, Kind::Picture(vec![1, 2, 3, 4]));
        assert!(a.complete && a.connected);
        let b = read_reply(&mut input).unwrap().unwrap();
        assert_eq!(b.kind, Kind::Skipped);
        assert!(!b.complete);
        let c = read_reply(&mut input).unwrap().unwrap();
        assert_eq!(c.kind, Kind::Region(Area { x: 2, y: 3, w: 1, h: 1 }, vec![9, 9, 9, 9]));
        assert!(read_reply(&mut input).unwrap().is_none());
        assert!(read_reply(&mut &b"XXXX000000000000"[..]).is_err());
    }

    #[test]
    fn layers_stack_like_the_output_page() {
        let names = ["main", "qr", "back", "z05_x", "front", "count", "qr", ""].map(String::from);
        assert_eq!(stack(names), ["back", "main", "qr", "count", "z05_x", "front"]);
        assert_eq!((layer_z("z99_top"), layer_z("z100_x"), layer_z("zebra")), (2599, 2500, 2500));
        let show =
            r#"{"scenes":[{"name":"Belka","layer":"main"},{"name":"Ticker","layer":"back"},{"name":"QR","layer":"qr"},{"name":"Bez"}]}"#;
        assert_eq!(show_layers(show).unwrap(), ["back", "main", "qr"]);
        assert_eq!(show_layers(r#"{"scenes":{"a":{"layer":"front"},"b":{"layer":"back"}}}"#).unwrap(), ["back", "front"]);
        assert!(show_layers(r#"{"pages":[]}"#).is_err());
        let state = r#"OK {"layers":[{"layer":"back","page":{"id":"t"}},{"layer":"qr","page":{"id":"q"}}]}"#;
        assert_eq!(state_layers(state), ["back", "qr"]);
        assert!(state_layers("ERR no").is_empty());
        assert_eq!(layer_url("http://v:8088/output?token=a", "main"), "http://v:8088/output?token=a&layers=main");
        assert_eq!(layer_url("http://v:8088/output", "a b,c"), "http://v:8088/output?layers=a%20b%2Cc");
    }

    fn solid(x: u32, y: u32, w: u32, h: u32, px: [u8; 4]) -> Rect {
        Rect { x, y, w, h, pixels: px.repeat((w * h) as usize).into() }
    }

    fn pixel(rects: &[Rect], x: u32, y: u32) -> [u8; 4] {
        rects.iter().find(|r| x >= r.x && y >= r.y && x < r.x + r.w && y < r.y + r.h).map_or([0; 4], |r| {
            let at = (((y - r.y) * r.w + x - r.x) * 4) as usize;
            [r.pixels[at], r.pixels[at + 1], r.pixels[at + 2], r.pixels[at + 3]]
        })
    }

    #[test]
    fn layers_compose_over_one_another() {
        let mut d = [10, 20, 30, 255];
        over(&mut d, &[200, 100, 0, 255]);
        assert_eq!(d, [200, 100, 0, 255], "opaque over anything");
        let mut d = [0, 0, 0, 0];
        over(&mut d, &[200, 100, 0, 128]);
        assert_eq!(d, [200, 100, 0, 128], "over nothing: itself");
        let mut d = [0, 0, 255, 255];
        over(&mut d, &[255, 0, 0, 128]);
        assert_eq!(d, [128, 0, 127, 255], "half red over blue");
        let mut d = [0, 0, 255, 128];
        over(&mut d, &[255, 0, 0, 128]);
        assert_eq!(d, [170, 0, 85, 192], "half over half");

        // A ticker (bottom) and a logo far from it: the rectangles as they are, the same pixels.
        let ticker = [solid(0, 100, 320, 20, [0, 0, 255, 255])];
        let logo = [solid(280, 10, 30, 30, [255, 255, 255, 255])];
        let mut blends = Blends::new();
        let out = compose(&[&ticker, &logo], (320, 180), &mut blends);
        assert_eq!(out.len(), 2);
        assert!(Arc::ptr_eq(&out[0].pixels, &ticker[0].pixels) && Arc::ptr_eq(&out[1].pixels, &logo[0].pixels));
        assert!(blends.is_empty());
        // A half-transparent bar over the ticker: blended where they meet, the top layer on top.
        let bar = [solid(100, 90, 50, 20, [255, 0, 0, 128])];
        let out = compose(&[&ticker, &logo, &bar], (320, 180), &mut blends);
        assert_eq!(pixel(&out, 120, 105), [128, 0, 127, 255], "bar over ticker");
        assert_eq!(pixel(&out, 120, 95), [255, 0, 0, 128], "bar alone");
        assert_eq!(pixel(&out, 10, 110), [0, 0, 255, 255], "ticker alone");
        assert_eq!(pixel(&out, 290, 20), [255, 255, 255, 255], "logo untouched");
        assert_eq!(blends.len(), 1);
        // The same pixels again: the blend is kept, not made anew.
        let again = compose(&[&ticker, &logo, &bar], (320, 180), &mut blends);
        let blended =
            |rects: &[Rect]| rects.iter().find(|r| r.y <= 105 && r.y + r.h > 105 && r.x <= 120 && r.x + r.w > 120).cloned().unwrap();
        assert!(Arc::ptr_eq(&blended(&out).pixels, &blended(&again).pixels));
        // One layer: as it is.
        assert_eq!(compose(&[&ticker], (320, 180), &mut blends).len(), 1);
    }

    #[test]
    fn renderer_names() {
        assert_eq!(Renderer::parse("").unwrap(), Renderer::Chromium);
        assert_eq!(Renderer::parse("Auto").unwrap(), Renderer::Auto);
        assert!(Renderer::parse("gpu").is_err());
        assert_eq!(Renderer::Native.name(), "native");
    }

    #[test]
    fn rects_are_placed_in_the_channel() {
        let mut rgba = vec![0u8; 4 * 4 * 4];
        rgba[(4 + 1) * 4 + 3] = 255;
        let rects = picture_rects(&rgba, Area { x: 10, y: 20, w: 4, h: 4 }, &[]);
        assert_eq!(rects.len(), 1);
        assert_eq!((rects[0].x, rects[0].y), (11, 21));
        assert!(picture_rects(&rgba[..4], Area { x: 0, y: 0, w: 4, h: 4 }, &[]).is_empty());
    }
}
