// ErsatzTV graphics packets (script element, `format: packet`):
// 4 bytes magic "ETVG" (0x47565445 big endian), 2 bytes version 1,
// 1 byte type, 4 bytes payload length, payload. All big endian.
//
// Every packet yields one frame; `Repeat(n)` yields the previous frame
// n + 1 times. `Rectangles` erases the canvas first, so it must carry every
// visible pixel, not only the changed ones.

pub const MAGIC: u32 = 0x4756_5445;
pub const VERSION: u16 = 1;

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
pub enum Type {
    Full = 0,
    Repeat = 1,
    Clear = 2,
    Rectangles = 3,
}

/// A visible rectangle of RGBA pixels (`w * h * 4` bytes). The pixels are
/// shared: a picture that keeps a rectangle of the one before (a standing
/// layer) holds the same pixels, and a frame built from the one before skips it.
#[derive(Clone, Debug, PartialEq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub pixels: std::sync::Arc<[u8]>,
}

fn header(out: &mut Vec<u8>, kind: Type, length: usize) {
    out.extend_from_slice(&MAGIC.to_be_bytes());
    out.extend_from_slice(&VERSION.to_be_bytes());
    out.push(kind as u8);
    out.extend_from_slice(&(length as u32).to_be_bytes());
}

pub fn clear_packet() -> Vec<u8> {
    let mut out = Vec::with_capacity(11);
    header(&mut out, Type::Clear, 0);
    out
}

/// `frames` frames of the previous picture (at least 1).
pub fn repeat_packet(frames: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(15);
    header(&mut out, Type::Repeat, 4);
    out.extend_from_slice(&(frames.saturating_sub(1).min(u64::from(u32::MAX)) as u32).to_be_bytes());
    out
}

/// Rectangles of RGBA; the rest of the canvas is transparent.
pub fn rectangles_packet(rects: &[Rect]) -> Vec<u8> {
    let payload = 2 + rects.iter().map(|r| 8 + r.pixels.len()).sum::<usize>();
    let mut out = Vec::with_capacity(11 + payload);
    header(&mut out, Type::Rectangles, payload);
    out.extend_from_slice(&(rects.len() as u16).to_be_bytes());
    for rect in rects {
        assert_eq!(rect.pixels.len(), (rect.w * rect.h * 4) as usize, "rectangle size mismatch");
        for value in [rect.x, rect.y, rect.w, rect.h] {
            out.extend_from_slice(&(value as u16).to_be_bytes());
        }
        out.extend_from_slice(&rect.pixels);
    }
    out
}

/// Packet of a picture: its visible rectangles, or Clear.
pub fn picture_packet(rects: &[Rect]) -> Vec<u8> {
    if rects.is_empty() { clear_packet() } else { rectangles_packet(rects) }
}

/// Pixels of `(x, y, w, h)` from an RGBA picture `width` pixels wide.
pub fn crop(rgba: &[u8], width: u32, (x, y, w, h): (u32, u32, u32, u32)) -> Vec<u8> {
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for row in y..y + h {
        let start = ((row * width + x) * 4) as usize;
        out.extend_from_slice(&rgba[start..start + (w * 4) as usize]);
    }
    out
}
