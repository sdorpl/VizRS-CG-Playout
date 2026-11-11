// Graphics as raw yuva420p frames for ffmpeg (ErsatzTV with the ffmpeg
// overlay, src/overlay.rs): planes Y, U, V, A, limited range, straight
// alpha as ffmpeg's overlay filters expect. Transparent areas are Y 16,
// U/V 128, A 0. HD uses BT.709, smaller frames BT.601. The arithmetic gives
// the former Node.js sidecar's results (its doubles, truncated) in integers, so
// frames match its reference vectors (tests/vectors/frames.json). A stream
// builds each frame from the one before (`Builder`): only changed rows.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use crate::area::{Area, Layout};
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

/// A frame to draw on: a copy of `blank` (or of the frame before), in reused memory when there is some.
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

/// A matrix row in ten-thousandths: the coefficients have four decimals, so
/// integer sums give the former Node.js arithmetic (`16.5 + k0 * r + k1 * g +
/// k2 * b` in doubles, truncated); only where the exact sum is a whole number
/// (about one in ten thousand) the doubles fell just short and gave one less.
type Coeffs = [[i32; 3]; 3];

const fn ten_thousandths(m: Matrix) -> Coeffs {
    let mut out = [[0i32; 3]; 3];
    let mut i = 0;
    while i < 3 {
        let mut j = 0;
        while j < 3 {
            let v = m[i][j] * 10000.0;
            out[i][j] = if v < 0.0 { (v - 0.5) as i32 } else { (v + 0.5) as i32 };
            j += 1;
        }
        i += 1;
    }
    out
}

const K709: Coeffs = ten_thousandths(BT709);
const K601: Coeffs = ten_thousandths(BT601);

fn coeffs(hd: bool) -> &'static Coeffs {
    if hd { &K709 } else { &K601 }
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
pub fn yuva_area(rects: &[Rect], (x, y, width, height): (u32, u32, u32, u32), hd: bool) -> Arc<Yuva> {
    let layout = Layout { width, height, parts: vec![(Area { x, y, w: width, h: height }, 0)] };
    Builder::new(&layout, hd).build(rects)
}

/// The yuva420p frames of one stream, picture after picture. Each picture is
/// laid out on an RGBA canvas of the frame; only the pairs of rows (one row of
/// chroma) whose pixels differ from the picture before are converted, the rest
/// of the frame is the one before. A crawl under a standing title costs the
/// crawl's rows, not the frame; a picture equal to the one before (a new shot
/// of a standing graphic) gives the same frame.
pub struct Builder {
    layout: Layout,
    hd: bool,
    /// The picture of `frame`, and the one before it (reused for the next).
    canvas: Vec<u8>,
    spare: Vec<u8>,
    /// Where each canvas has pixels: (x, y, w, h) in frame coordinates.
    drawn: Vec<(usize, usize, usize, usize)>,
    spare_drawn: Vec<(usize, usize, usize, usize)>,
    /// The rectangles of `frame` and where they went: a picture with the same
    /// places changes the canvas in place, only where its pixels are new.
    rects: Vec<Rect>,
    places: Vec<Place>,
    frame: Option<Arc<Yuva>>,
}

/// Where (a part of) a rectangle goes in the frame: rectangle `rect`, from its
/// pixel (sx, sy), to (dx, dy), `w`x`h` pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Place {
    rect: usize,
    sx: usize,
    sy: usize,
    dx: usize,
    dy: usize,
    w: usize,
    h: usize,
}

impl Place {
    fn region(&self) -> (usize, usize, usize, usize) {
        (self.dx, self.dy, self.w, self.h)
    }
}

impl Builder {
    pub fn new(layout: &Layout, hd: bool) -> Builder {
        Builder {
            layout: layout.clone(),
            hd,
            canvas: Vec::new(),
            spare: Vec::new(),
            drawn: Vec::new(),
            spare_drawn: Vec::new(),
            rects: Vec::new(),
            places: Vec::new(),
            frame: None,
        }
    }

    /// The frame of a picture's rectangles (channel coordinates).
    pub fn build(&mut self, rects: &[Rect]) -> Arc<Yuva> {
        let (w, h) = (self.layout.width as usize, self.layout.height as usize);
        if w == 0 || h == 0 {
            return blank(self.layout.width, self.layout.height);
        }
        let bytes = w * h * 4;
        let t = coeffs(self.hd);
        let places = places(&self.layout, rects);
        let apart = !places.iter().enumerate().any(|(i, a)| places[..i].iter().any(|b| overlaps(a.region(), b.region())));

        // The same places as the picture before: the canvas changes in place,
        // only the rectangles with new pixels (a standing layer keeps its own).
        let same = apart && places == self.places && rects.len() == self.rects.len() && self.canvas.len() == bytes;
        if let Some(previous) = self.frame.as_ref().filter(|_| same) {
            let mut spans: Vec<Option<(usize, usize)>> = vec![None; h.div_ceil(2)];
            for place in &places {
                let (new, old) = (&rects[place.rect], &self.rects[place.rect]);
                if Arc::ptr_eq(&new.pixels, &old.pixels) && (new.x, new.y, new.w, new.h) == (old.x, old.y, old.w, old.h) {
                    continue;
                }
                let stride = new.w as usize * 4;
                for r in 0..place.h {
                    let src = &new.pixels[(place.sy + r) * stride + place.sx * 4..][..place.w * 4];
                    let row = place.dy + r;
                    let dst = &mut self.canvas[(row * w + place.dx) * 4..][..place.w * 4];
                    if let Some((c0, c1)) = differing(dst, src) {
                        dst[c0 * 4..c1 * 4].copy_from_slice(&src[c0 * 4..c1 * 4]);
                        let (x0, x1) = ((place.dx + c0) & !1, place.dx + c1);
                        let span = &mut spans[row / 2];
                        *span = Some(span.map_or((x0, x1), |(a, b)| (a.min(x0), b.max(x1))));
                    }
                }
            }
            let changed: Vec<(usize, (usize, usize))> = spans.into_iter().enumerate().filter_map(|(by, s)| Some((by, s?))).collect();
            let frame = if changed.is_empty() {
                Arc::clone(previous)
            } else {
                let mut frame = fresh(previous);
                convert_all(&self.canvas, w, h, &mut frame.data, &changed, t);
                Arc::new(frame)
            };
            self.rects = rects.to_vec();
            self.frame = Some(Arc::clone(&frame));
            return frame;
        }

        if self.spare.len() != bytes {
            self.spare = vec![0; bytes];
            self.spare_drawn.clear();
        }
        // The new picture on the spare canvas: what the picture before last left there goes first.
        for &(x, y, rw, rh) in &self.spare_drawn {
            for row in y..y + rh {
                self.spare[(row * w + x) * 4..(row * w + x + rw) * 4].fill(0);
            }
        }
        self.spare_drawn.clear();
        compose(w, rects, &places, apart, &mut self.spare);
        self.spare_drawn.extend(places.iter().map(Place::region));

        let previous = self.frame.as_ref().filter(|_| self.canvas.len() == bytes);
        let frame = match previous {
            None => {
                if self.spare_drawn.is_empty() {
                    blank(self.layout.width, self.layout.height)
                } else {
                    let mut frame = fresh(&blank(self.layout.width, self.layout.height));
                    let all: Vec<(usize, (usize, usize))> = pairs(&self.spare_drawn, h).map(|by| (by, (0, w))).collect();
                    convert_all(&self.spare, w, h, &mut frame.data, &all, t);
                    Arc::new(frame)
                }
            }
            Some(previous) => {
                // Pairs of rows where either picture has pixels, and their changed columns.
                let mut both = self.drawn.clone();
                both.extend_from_slice(&self.spare_drawn);
                let changed: Vec<(usize, (usize, usize))> =
                    pairs(&both, h).filter_map(|by| Some((by, changed_columns(&self.canvas, &self.spare, w, h, by)?))).collect();
                if changed.is_empty() {
                    Arc::clone(previous)
                } else if self.spare_drawn.is_empty() {
                    blank(self.layout.width, self.layout.height)
                } else {
                    let mut frame = fresh(previous);
                    convert_all(&self.spare, w, h, &mut frame.data, &changed, t);
                    Arc::new(frame)
                }
            }
        };
        std::mem::swap(&mut self.canvas, &mut self.spare);
        std::mem::swap(&mut self.drawn, &mut self.spare_drawn);
        self.rects = rects.to_vec();
        self.places = places;
        self.frame = Some(Arc::clone(&frame));
        frame
    }
}

/// The pixels `[c0, c1)` of two rows of RGBA between their first and last difference, if any.
fn differing(a: &[u8], b: &[u8]) -> Option<(usize, usize)> {
    const CHUNK: usize = 64;
    if a == b {
        return None;
    }
    let chunks = a.len().div_ceil(CHUNK);
    let part = |x: &[u8], i: usize| -> std::ops::Range<usize> { i * CHUNK..((i + 1) * CHUNK).min(x.len()) };
    let differs = |i: &usize| a[part(a, *i)] != b[part(b, *i)];
    let first = (0..chunks).find(differs)?;
    let last = (0..chunks).rev().find(differs).unwrap_or(first);
    Some((first * CHUNK / 4, ((last + 1) * CHUNK).min(a.len()) / 4))
}

/// Where the rectangles go in a frame of `layout` (parts of the channel at rows of the frame).
fn places(layout: &Layout, rects: &[Rect]) -> Vec<Place> {
    let mut out = Vec::new();
    for &(area, row) in &layout.parts {
        for (i, rect) in rects.iter().enumerate() {
            let x0 = rect.x.max(area.x);
            let y0 = rect.y.max(area.y);
            let x1 = (rect.x + rect.w).min(area.x + area.w).min(area.x + layout.width);
            let y1 = (rect.y + rect.h).min(area.y + area.h);
            if x1 <= x0 || y1 <= y0 || row + (y0 - area.y) >= layout.height {
                continue;
            }
            let y1 = y1.min(area.y + layout.height - row);
            out.push(Place {
                rect: i,
                sx: (x0 - rect.x) as usize,
                sy: (y0 - rect.y) as usize,
                dx: (x0 - area.x) as usize,
                dy: (row + y0 - area.y) as usize,
                w: (x1 - x0) as usize,
                h: (y1 - y0) as usize,
            });
        }
    }
    out
}

/// The rectangles laid out on `canvas` (zeroed where they go) at their
/// `places`; with places not `apart`, a later rectangle covers an earlier one
/// where it has alpha.
fn compose(w: usize, rects: &[Rect], places: &[Place], apart: bool, canvas: &mut [u8]) {
    for place in places {
        let rect = &rects[place.rect];
        let stride = rect.w as usize * 4;
        for r in 0..place.h {
            let src = &rect.pixels[(place.sy + r) * stride + place.sx * 4..][..place.w * 4];
            let dst = &mut canvas[((place.dy + r) * w + place.dx) * 4..][..place.w * 4];
            if apart {
                dst.copy_from_slice(src);
            } else {
                for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                    if s[3] != 0 {
                        d.copy_from_slice(s);
                    }
                }
            }
        }
    }
}

fn overlaps((ax, ay, aw, ah): (usize, usize, usize, usize), (bx, by, bw, bh): (usize, usize, usize, usize)) -> bool {
    ax < bx + bw && bx < ax + aw && ay < by + bh && by < ay + ah
}

/// The pairs of rows (chroma rows) that `regions` reach, in order.
fn pairs(regions: &[(usize, usize, usize, usize)], h: usize) -> impl Iterator<Item = usize> {
    let mut wanted = vec![false; h.div_ceil(2)];
    for &(_, y, _, rh) in regions {
        if rh > 0 {
            wanted[y / 2..=(y + rh - 1) / 2].fill(true);
        }
    }
    wanted.into_iter().enumerate().filter_map(|(by, on)| on.then_some(by))
}

/// The columns `[x0, x1)` (x0 even) of the pair of rows `by` where the two
/// canvases differ, if they do.
fn changed_columns(a: &[u8], b: &[u8], w: usize, h: usize, by: usize) -> Option<(usize, usize)> {
    const CHUNK: usize = 64;
    let mut span: Option<(usize, usize)> = None;
    for row in [by * 2, by * 2 + 1].into_iter().filter(|&r| r < h) {
        let (ra, rb) = (&a[row * w * 4..(row + 1) * w * 4], &b[row * w * 4..(row + 1) * w * 4]);
        if ra == rb {
            continue;
        }
        let differs = |i: &usize| ra[i * CHUNK..((i + 1) * CHUNK).min(ra.len())] != rb[i * CHUNK..((i + 1) * CHUNK).min(rb.len())];
        let chunks = ra.len().div_ceil(CHUNK);
        let first = (0..chunks).find(differs).unwrap_or(0);
        let last = (0..chunks).rev().find(differs).unwrap_or(chunks - 1);
        let (x0, x1) = (first * CHUNK / 4, ((last + 1) * CHUNK / 4).min(w));
        span = Some(span.map_or((x0, x1), |(s0, s1)| (s0.min(x0), s1.max(x1))));
    }
    span.map(|(x0, x1)| (x0 & !1, x1))
}

/// Converts the columns `[x0, x1)` (x0 even) of the pair of rows `by` of the
/// canvas into `out`: luma and alpha of each pixel, chroma of each 2x2 block
/// weighted by alpha (no dark or coloured fringes). Transparent pixels are Y
/// 16, A 0; a block with no alpha U/V 128.
fn convert(canvas: &[u8], w: usize, h: usize, out: &mut Planes, by: usize, (x0, x1): (usize, usize), k: &Coeffs) {
    let cw = w.div_ceil(2);
    let [ky, ku, kv] = *k;
    let rows = [by * 2, by * 2 + 1];

    for &row in rows.iter().filter(|&&r| r < h) {
        let src = &canvas[(row * w + x0) * 4..(row * w + x1) * 4];
        let line = (row - out.by0 * 2) * w;
        for ((p, y), a) in src.chunks_exact(4).zip(&mut out.y[line + x0..line + x1]).zip(&mut out.a[line + x0..line + x1]) {
            let (r, g, b) = (i32::from(p[0]), i32::from(p[1]), i32::from(p[2]));
            let value = ((165_000 + ky[0] * r + ky[1] * g + ky[2] * b) / 10_000) as u8;
            *y = if p[3] == 0 { 16 } else { value };
            *a = p[3];
        }
    }

    let full = by * 2 + 1 < h;
    let top = &canvas[by * 2 * w * 4..][..w * 4];
    let bottom = if full { &canvas[(by * 2 + 1) * w * 4..][..w * 4] } else { top };
    let at = (by - out.by0) * cw;
    let (u_row, v_row) = (&mut out.u[at..at + cw], &mut out.v[at..at + cw]);
    for bx in x0 / 2..x1.div_ceil(2) {
        let c = bx * 2;
        // A whole block of equal alpha (inside a graphic, or transparent): the
        // plain mean, a division by a constant.
        if full && c + 1 < w {
            let (t, b) = (&top[c * 4..c * 4 + 8], &bottom[c * 4..c * 4 + 8]);
            let a = t[3];
            if t[7] == a && b[3] == a && b[7] == a {
                let (u, v) = if a == 0 {
                    (128, 128)
                } else {
                    let sum = |i: usize| i32::from(t[i]) + i32::from(t[i + 4]) + i32::from(b[i]) + i32::from(b[i + 4]);
                    let (r, g, bl) = (sum(0), sum(1), sum(2));
                    ((5_140_000 + ku[0] * r + ku[1] * g + ku[2] * bl) / 40_000, (5_140_000 + kv[0] * r + kv[1] * g + kv[2] * bl) / 40_000)
                };
                u_row[bx] = u as u8;
                v_row[bx] = v as u8;
                continue;
            }
        }
        // The block's pixels: two columns unless at an odd right edge, two rows unless at an odd bottom.
        let cols = if c + 1 < w { 2 } else { 1 };
        let (pt, pb) = (&top[c * 4..(c + cols) * 4], &bottom[c * 4..(c + cols) * 4]);
        let pixels = pt.chunks_exact(4).chain(pb.chunks_exact(4).take(if full { cols } else { 0 }));
        // Sums of the channels weighted by alpha (and plain, for equal alpha):
        // the matrix is linear, so one product per block gives the sum of the pixels' values.
        let (mut n, mut sa, mut first, mut uniform) = (0i64, 0i64, 0u8, true);
        let (mut plain, mut weighted) = ([0i64; 3], [0i64; 3]);
        for p in pixels {
            if p[3] == 0 {
                uniform = false;
                continue;
            }
            if n == 0 {
                first = p[3];
            } else if p[3] != first {
                uniform = false;
            }
            let a = i64::from(p[3]);
            n += 1;
            sa += a;
            for i in 0..3 {
                plain[i] += i64::from(p[i]);
                weighted[i] += a * i64::from(p[i]);
            }
        }
        let dot = |k: [i32; 3], s: [i64; 3]| i64::from(k[0]) * s[0] + i64::from(k[1]) * s[1] + i64::from(k[2]) * s[2];
        let (u, v) = if sa == 0 {
            (128, 128)
        } else if uniform {
            // Equal alpha: the plain mean, a division by a constant.
            let mean = |s: i64| match n {
                4 => (1_285_000 * 4 + s) / 40_000,
                2 => (1_285_000 * 2 + s) / 20_000,
                _ => (1_285_000 * n + s) / (10_000 * n),
            };
            (mean(dot(ku, plain)), mean(dot(kv, plain)))
        } else {
            ((1_285_000 * sa + dot(ku, weighted)) / (10_000 * sa), (1_285_000 * sa + dot(kv, weighted)) / (10_000 * sa))
        };
        u_row[bx] = u as u8;
        v_row[bx] = v as u8;
    }
}

/// The rows of the planes of a frame from the pair of rows `by0` on: Y and A
/// two rows per pair, U and V one.
struct Planes<'a> {
    by0: usize,
    y: &'a mut [u8],
    u: &'a mut [u8],
    v: &'a mut [u8],
    a: &'a mut [u8],
}

/// Changes this large (pixels) are converted on several threads.
const PARALLEL: usize = 150_000;
const MAX_THREADS: usize = 4;

/// Converts the `pairs` (pair of rows, columns; ascending) of the canvas into
/// `out`, a large change on several threads (each its own rows of the planes).
fn convert_all(canvas: &[u8], w: usize, h: usize, out: &mut [u8], pairs: &[(usize, (usize, usize))], k: &Coeffs) {
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    let (y, rest) = out.split_at_mut(w * h);
    let (u, rest) = rest.split_at_mut(cw * ch);
    let (v, a) = rest.split_at_mut(cw * ch);
    let pixels: usize = pairs.iter().map(|(_, (x0, x1))| (x1 - x0) * 2).sum();
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(MAX_THREADS);
    if pixels < PARALLEL || threads < 2 || pairs.len() < threads {
        let mut planes = Planes { by0: 0, y, u, v, a };
        for &(by, columns) in pairs {
            convert(canvas, w, h, &mut planes, by, columns, k);
        }
        return;
    }
    // Contiguous runs of pairs with about the same pixels each; planes split at their first pair.
    let share = pixels.div_ceil(threads);
    let mut runs: Vec<&[(usize, (usize, usize))]> = Vec::new();
    let (mut start, mut sum) = (0, 0);
    for (i, (_, (x0, x1))) in pairs.iter().enumerate() {
        sum += (x1 - x0) * 2;
        if sum >= share && i + 1 < pairs.len() {
            runs.push(&pairs[start..=i]);
            (start, sum) = (i + 1, 0);
        }
    }
    runs.push(&pairs[start..]);
    let (mut y, mut u, mut v, mut a) = (y, u, v, a);
    let mut jobs = Vec::new();
    for (i, run) in runs.iter().enumerate() {
        let by0 = run[0].0;
        // This run's rows end where the next run's begin (the last takes the rest).
        let end = runs.get(i + 1).map_or(ch, |next| next[0].0);
        let (luma_rows, chroma_rows) = ((end * 2).min(h) - by0 * 2, end - by0);
        let skip = |plane: &mut &mut [u8], at: usize| {
            let taken = std::mem::take(plane);
            *plane = &mut taken[at..];
        };
        if i == 0 {
            skip(&mut y, by0 * 2 * w);
            skip(&mut a, by0 * 2 * w);
            skip(&mut u, by0 * cw);
            skip(&mut v, by0 * cw);
        }
        let (ry, ty) = std::mem::take(&mut y).split_at_mut(luma_rows * w);
        let (ra, ta) = std::mem::take(&mut a).split_at_mut(luma_rows * w);
        let (ru, tu) = std::mem::take(&mut u).split_at_mut(chroma_rows * cw);
        let (rv, tv) = std::mem::take(&mut v).split_at_mut(chroma_rows * cw);
        (y, a, u, v) = (ty, ta, tu, tv);
        jobs.push((*run, Planes { by0, y: ry, u: ru, v: rv, a: ra }));
    }
    std::thread::scope(|scope| {
        for (run, mut planes) in jobs {
            scope.spawn(move || {
                for &(by, columns) in run {
                    convert(canvas, w, h, &mut planes, by, columns, k);
                }
            });
        }
    });
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

#[cfg(test)]
mod builder_tests {
    use super::*;

    /// A tiny deterministic generator (no dependency).
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u32 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (self.0 >> 33) as u32
        }
        fn below(&mut self, n: u32) -> u32 {
            self.next() % n.max(1)
        }
    }

    fn rect(rng: &mut Rng, cw: u32, ch: u32) -> Rect {
        let (x, y) = (rng.below(cw), rng.below(ch));
        let (w, h) = (1 + rng.below(cw - x), 1 + rng.below(ch - y));
        let alpha = [0u8, 255, 255, 128, 7];
        let pixels = (0..w * h)
            .flat_map(|_| {
                let a = alpha[rng.below(5) as usize];
                [rng.next() as u8, rng.next() as u8, rng.next() as u8, a]
            })
            .collect::<Vec<u8>>()
            .into();
        Rect { x, y, w, h, pixels }
    }

    #[test]
    fn a_frame_from_the_one_before_is_the_frame_built_anew() {
        let mut rng = Rng(7);
        let layouts = [
            (Layout::whole(37, 23), false),
            (Layout::whole(64, 40), true),
            (Layout::atlas(&[Area { x: 4, y: 2, w: 20, h: 9 }, Area { x: 30, y: 20, w: 14, h: 11 }]), true),
        ];
        for (layout, hd) in layouts {
            let (cw, ch) = (60, 40);
            let mut builder = Builder::new(&layout, hd);
            let mut rects: Vec<Rect> = Vec::new();
            for step in 0..60 {
                // Mostly small changes (a crawl), sometimes new geometry or nothing.
                match rng.below(5) {
                    0 => rects = (0..rng.below(4)).map(|_| rect(&mut rng, cw, ch)).collect(),
                    1 => {}
                    _ if !rects.is_empty() => {
                        let i = rng.below(rects.len() as u32) as usize;
                        let mut pixels = rects[i].pixels.to_vec();
                        for _ in 0..3 {
                            let at = rng.below(pixels.len() as u32) as usize;
                            pixels[at] = rng.next() as u8;
                        }
                        rects[i].pixels = pixels.into();
                    }
                    _ => rects.push(rect(&mut rng, cw, ch)),
                }
                let built = builder.build(&rects);
                let anew = Builder::new(&layout, hd).build(&rects);
                assert_eq!(&built[..], &anew[..], "step {step} of {layout:?}");
                assert_eq!(built.len(), yuva_size(layout.width, layout.height));
            }
        }
    }

    #[test]
    fn parallel_conversion_is_the_sequential_one() {
        let mut rng = Rng(11);
        let (w, h) = (801usize, 403usize);
        let canvas: Vec<u8> = (0..w * h * 4).map(|_| rng.next() as u8).collect();
        let pairs: Vec<(usize, (usize, usize))> =
            (0..h.div_ceil(2)).filter(|by| by % 7 != 3).map(|by| (by, if by % 5 == 0 { (10, 500) } else { (0, w) })).collect();
        let size = yuva_size(w as u32, h as u32);
        let (mut parallel, mut sequential) = (vec![7u8; size], vec![7u8; size]);
        convert_all(&canvas, w, h, &mut parallel, &pairs, &K709);
        {
            let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
            let (y, rest) = sequential.split_at_mut(w * h);
            let (u, rest) = rest.split_at_mut(cw * ch);
            let (v, a) = rest.split_at_mut(cw * ch);
            let mut planes = Planes { by0: 0, y, u, v, a };
            for &(by, columns) in &pairs {
                convert(&canvas, w, h, &mut planes, by, columns, &K709);
            }
        }
        assert!(pairs.iter().map(|(_, (a, b))| (b - a) * 2).sum::<usize>() >= PARALLEL);
        assert!(parallel == sequential);
    }

    #[test]
    fn integer_luma_and_chroma_are_the_doubles_truncated() {
        for (matrix, k) in [(BT709, K709), (BT601, K601)] {
            let mut rng = Rng(3);
            let mut mismatches = 0;
            for _ in 0..200_000 {
                let (r, g, b) = (rng.below(256), rng.below(256), rng.below(256));
                let (fr, fg, fb) = (f64::from(r), f64::from(g), f64::from(b));
                let double = |m: [f64; 3], offset: f64| (offset + m[0] * fr + m[1] * fg + m[2] * fb) as i32;
                let int = |k: [i32; 3], offset: i32| (offset + k[0] * r as i32 + k[1] * g as i32 + k[2] * b as i32) / 10_000;
                // Equal, or the exact sum is a whole number the doubles fall just short of.
                for (i, offset, doff) in [(0, 165_000, 16.5), (1, 1_285_000, 128.5), (2, 1_285_000, 128.5)] {
                    let (exact, d) = (int(k[i], offset), double(matrix[i], doff));
                    let whole = (offset + k[i][0] * r as i32 + k[i][1] * g as i32 + k[i][2] * b as i32) % 10_000 == 0;
                    assert!(exact == d || (whole && exact == d + 1), "row {i} of {r},{g},{b}: {exact} vs {d}");
                    mismatches += usize::from(exact != d);
                }
            }
            // Whole numbers are rare: about one sum in ten thousand.
            assert!(mismatches < 200, "{mismatches}");
        }
    }
}
