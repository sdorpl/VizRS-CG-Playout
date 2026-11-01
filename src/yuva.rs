// Graphics as raw yuva420p frames for ffmpeg (ErsatzTV with the ffmpeg
// overlay, src/overlay.rs): planes Y, U, V, A, limited range, straight
// alpha as ffmpeg's overlay filters expect. Transparent areas are Y 16,
// U/V 128, A 0. HD uses BT.709, smaller frames BT.601. The arithmetic follows
// the former Node.js sidecar step by step (doubles, truncation), so frames match
// its reference vectors (tests/vectors/frames.json) byte for byte.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use crate::packets::Rect;

type Matrix = [[f64; 3]; 3];
const BT709: Matrix = [[0.1826, 0.6142, 0.062], [-0.1006, -0.3386, 0.4392], [0.4392, -0.3989, -0.0403]];
const BT601: Matrix = [[0.2568, 0.5041, 0.0979], [-0.1482, -0.291, 0.4392], [0.4392, -0.3678, -0.0714]];

pub fn yuva_size(width: u32, height: u32) -> usize {
    let (w, h) = (width as usize, height as usize);
    w * h * 2 + w.div_ceil(2) * h.div_ceil(2) * 2
}

/// A yuva420p frame. One built from rectangles gives its memory back to a
/// small pool when the last session lets go of it: a ticker makes a new
/// frame at every frame, and fresh megabytes cost page faults every time.
pub struct Yuva {
    data: Vec<u8>,
    pooled: bool,
}

impl std::fmt::Debug for Yuva {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Yuva({} bytes)", self.data.len())
    }
}

impl std::ops::Deref for Yuva {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.data
    }
}

impl Drop for Yuva {
    fn drop(&mut self) {
        if self.pooled {
            let mut pool = pool();
            if pool.len() == POOL {
                pool.remove(0);
            }
            pool.push(std::mem::take(&mut self.data));
        }
    }
}

/// Frames kept for reuse: the sessions hold one each, the next is built meanwhile.
const POOL: usize = 4;

fn pool() -> MutexGuard<'static, Vec<Vec<u8>>> {
    static POOL: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
    POOL.lock().unwrap_or_else(|e| e.into_inner())
}

/// A frame to draw on: `blank`'s copy, in reused memory when there is some.
fn fresh(blank: &[u8]) -> Yuva {
    let reused = {
        let mut pool = pool();
        pool.iter().position(|data| data.len() == blank.len()).map(|i| pool.swap_remove(i))
    };
    let data = match reused {
        Some(mut data) => {
            data.copy_from_slice(blank);
            data
        }
        None => blank.to_vec(),
    };
    Yuva { data, pooled: true }
}

/// Fully transparent frames by size.
type Blanks = Mutex<HashMap<(u32, u32), Arc<Yuva>>>;

/// Fully transparent frame; shared, never written to.
fn blank(width: u32, height: u32) -> Arc<Yuva> {
    static BLANKS: OnceLock<Blanks> = OnceLock::new();
    let mut blanks = BLANKS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    Arc::clone(blanks.entry((width, height)).or_insert_with(|| {
        let mut frame = vec![0u8; yuva_size(width, height)];
        let luma = (width * height) as usize;
        let alpha = frame.len() - luma;
        frame[..luma].fill(16);
        frame[luma..alpha].fill(128);
        Arc::new(Yuva { data: frame, pooled: false })
    }))
}

/// JavaScript's `x | 0` for the values here (positive, far below 2^31).
fn trunc(x: f64) -> u8 {
    x as i32 as u8
}

/// Each term of a matrix row for every byte value, so a pixel costs three
/// lookups and two additions. The sums keep the order of the former Node.js
/// arithmetic (`16.5 + k0 * r + k1 * g + k2 * b`, left to right), so the
/// results are the same to the last bit.
struct Tables {
    y: [[f64; 256]; 3],
    u: [[f64; 256]; 3],
    v: [[f64; 256]; 3],
}

impl Tables {
    fn new([ky, ku, kv]: Matrix) -> Tables {
        let row = |k: [f64; 3], offset: f64| {
            let term = |i: usize, offset: f64| std::array::from_fn(|value| offset + k[i] * f64::from(value as u8));
            [term(0, offset), term(1, 0.0), term(2, 0.0)]
        };
        Tables { y: row(ky, 16.5), u: row(ku, 0.0), v: row(kv, 0.0) }
    }

    fn get(hd: bool) -> &'static Tables {
        static HD: OnceLock<Tables> = OnceLock::new();
        static SD: OnceLock<Tables> = OnceLock::new();
        if hd { HD.get_or_init(|| Tables::new(BT709)) } else { SD.get_or_init(|| Tables::new(BT601)) }
    }
}

fn sum(t: &[[f64; 256]; 3], p: &[u8]) -> f64 {
    t[0][usize::from(p[0])] + t[1][usize::from(p[1])] + t[2][usize::from(p[2])]
}

/// ffmpeg's name of the colour matrix of the frames of a channel of `height` lines.
pub fn ffmpeg_matrix(height: u32) -> &'static str {
    if height >= 720 { "bt709" } else { "smpte170m" }
}

/// One yuva420p frame of `width`x`height` from visible rectangles of RGBA.
pub fn yuva_frame(rects: &[Rect], width: u32, height: u32) -> Arc<Yuva> {
    yuva_area(rects, (0, 0, width, height), height >= 720)
}

/// One yuva420p frame of the part `(x, y, width, height)` of a picture
/// (its graphics area): rectangles in picture coordinates, cut to the part.
/// `hd` = BT.709 (a channel of 720 lines or more), else BT.601.
pub fn yuva_area(rects: &[Rect], (ox, oy, width, height): (u32, u32, u32, u32), hd: bool) -> Arc<Yuva> {
    if rects.is_empty() {
        return blank(width, height);
    }

    let mut frame = fresh(&blank(width, height));
    let out = &mut frame.data;
    let t = Tables::get(hd);
    let w = width as usize;
    let cw = w.div_ceil(2);
    let ch = (height as usize).div_ceil(2);
    let u_off = w * height as usize;
    let v_off = u_off + cw * ch;
    let a_off = v_off + cw * ch;

    for rect in rects {
        // The rectangle in frame coordinates, cut to the frame.
        let (rx, ry) = (i64::from(rect.x) - i64::from(ox), i64::from(rect.y) - i64::from(oy));
        let (x0, y0) = (rx.max(0), ry.max(0));
        let (x1, y1) = ((rx + i64::from(rect.w)).min(i64::from(width)), (ry + i64::from(rect.h)).min(i64::from(height)));
        if x1 <= x0 || y1 <= y0 {
            continue;
        }
        let (x, y, rw, rh) = (x0 as usize, y0 as usize, (x1 - x0) as usize, (y1 - y0) as usize);
        // Offsets of the cut part inside the rectangle's pixels.
        let (sx, sy, stride) = ((x0 - rx) as usize, (y0 - ry) as usize, rect.w as usize);
        // Row `row` of the cut part: its `rw` RGBA pixels.
        let source = |row: usize| {
            let start = ((row + sy) * stride + sx) * 4;
            &rect.pixels[start..start + rw * 4]
        };

        for row in 0..rh {
            let line = (y + row) * w + x;
            let (luma, rest) = out.split_at_mut(a_off);
            let pixels = source(row).chunks_exact(4);
            for ((p, y), a) in pixels.zip(&mut luma[line..line + rw]).zip(&mut rest[line..line + rw]) {
                if p[3] != 0 {
                    *y = trunc(sum(&t.y, p));
                    *a = p[3];
                }
            }
        }

        // Chroma of each 2x2 block, weighted by alpha: no dark or coloured fringes.
        for by in y >> 1..=(y + rh - 1) >> 1 {
            let rows = [by * 2, by * 2 + 1].map(|r| r.checked_sub(y).filter(|&r| r < rh).map(source));
            for bx in x >> 1..=(x + rw - 1) >> 1 {
                let (mut sa, mut su, mut sv) = (0.0, 0.0, 0.0);
                for pixels in rows.iter().flatten() {
                    for c in [bx * 2, bx * 2 + 1] {
                        let Some(col) = c.checked_sub(x).filter(|&c| c < rw) else { continue };
                        let p = &pixels[col * 4..col * 4 + 4];
                        if p[3] == 0 {
                            continue;
                        }
                        let a = f64::from(p[3]);
                        sa += a;
                        su += a * sum(&t.u, p);
                        sv += a * sum(&t.v, p);
                    }
                }
                if sa == 0.0 {
                    continue;
                }
                out[u_off + by * cw + bx] = trunc(128.5 + su / sa);
                out[v_off + by * cw + bx] = trunc(128.5 + sv / sa);
            }
        }
    }

    Arc::new(frame)
}

#[cfg(test)]
mod matrix_tests {
    use super::ffmpeg_matrix;

    #[test]
    fn matrix_name_follows_the_channel_height() {
        assert_eq!(ffmpeg_matrix(719), "smpte170m");
        assert_eq!(ffmpeg_matrix(720), "bt709");
        assert_eq!(ffmpeg_matrix(900), "bt709");
    }
}
