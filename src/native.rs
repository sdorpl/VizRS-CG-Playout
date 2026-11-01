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
// pixels. Flags: bit 0 all on air
// drawn natively, bit 1 connected to VizRS.

use std::io::{BufReader, Read, Write};
use std::os::unix::process::CommandExt;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::area::{Area, cut_to};
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
        Ok(Native { child, stdin, replies })
    }

    /// The frame at monotonic time `ms`: `complete_only` = only when all on air is native,
    /// `force` = the picture even when unchanged.
    pub fn frame(&mut self, ms: f64, wall: f64, complete_only: bool, force: bool) -> Result<Reply, String> {
        let flags = format!("{}{}", if complete_only { "c" } else { "" }, if force { "f" } else { "" });
        writeln!(self.stdin, "F {ms:.3} {wall:.3} {flags}")
            .and_then(|_| self.stdin.flush())
            .map_err(|e| format!("native renderer: {e}"))?;
        match self.replies.recv_timeout(FRAME_TIMEOUT) {
            Ok(reply) => reply,
            Err(RecvTimeoutError::Timeout) => Err("native renderer: no frame in time".into()),
            Err(RecvTimeoutError::Disconnected) => Err("native renderer exited".into()),
        }
    }
}

impl Drop for Native {
    fn drop(&mut self) {
        let _ = writeln!(self.stdin, "Q");
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Visible rectangles of a native picture of `part` (channel coordinates), cut to `areas`.
pub fn picture_rects(rgba: &[u8], part: Area, areas: &[Area]) -> Vec<Rect> {
    let (w, h) = (part.w, part.h);
    if rgba.len() != w as usize * h as usize * 4 {
        return Vec::new();
    }
    let mut rects: Vec<Rect> = visible_rects(rgba, w, h)
        .into_iter()
        .map(|(x, y, rw, rh)| Rect { x: x + part.x, y: y + part.y, w: rw, h: rh, pixels: crop(rgba, w, (x, y, rw, rh)) })
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
