// Graphics resolution: Chromium may render /output smaller (or larger) than
// the channel. ErsatzTV always draws on a canvas of the channel size, so the
// visible rectangles are scaled to it here, as libvips did for the Node.js
// build (`sharp` resize): alpha premultiplied, Lanczos3 when shrinking,
// bicubic when enlarging, pixel centres aligned.

use crate::packets::{Rect, crop};

// Transparent margin around each rectangle, so the resampling kernel sees
// the real edge instead of a repeated border pixel.
const PAD: u32 = 4;

/// JavaScript's `Math.round`: halves go up.
fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// Render size for a channel: "" or "auto" = the channel size, "720" or
/// "720p" = that height in the channel's proportions, "1280x720" = exact.
pub fn render_size(spec: &str, width: u32, height: u32) -> Result<(u32, u32), String> {
    let value = spec.trim().to_lowercase();
    if value.is_empty() || value == "auto" {
        return Ok((width, height));
    }

    let number = |text: &str| (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())).then(|| text.parse::<f64>().ok()).flatten();
    let (w, h) = if let Some((w, h)) = value.split_once('x').and_then(|(w, h)| Some((number(w)?, number(h)?))) {
        (w, h)
    } else if let Some(h) = number(value.strip_suffix('p').unwrap_or(&value)) {
        (js_round(f64::from(width) * h / f64::from(height) / 2.0) * 2.0, h)
    } else {
        return Err(format!("bad render size \"{spec}\" (auto, 720p or 1280x720)"));
    };

    if !(16.0..=7680.0).contains(&w) || !(16.0..=4320.0).contains(&h) {
        return Err(format!("render size {w}x{h} out of range"));
    }
    Ok((w as u32, h as u32))
}

fn lanczos3(x: f64) -> f64 {
    if x == 0.0 {
        return 1.0;
    }
    if x.abs() >= 3.0 {
        return 0.0;
    }
    let px = std::f64::consts::PI * x;
    3.0 * px.sin() * (px / 3.0).sin() / (px * px)
}

/// Catmull-Rom, libvips' "bicubic".
fn cubic(x: f64) -> f64 {
    let x = x.abs();
    if x < 1.0 {
        1.5 * x * x * x - 2.5 * x * x + 1.0
    } else if x < 2.0 {
        -0.5 * x * x * x + 2.5 * x * x - 4.0 * x + 2.0
    } else {
        0.0
    }
}

/// Source taps and their weights for every output position of one axis.
fn weights(src: usize, dst: usize) -> Vec<(usize, Vec<f64>)> {
    let scale = dst as f64 / src as f64;
    // Shrinking stretches the kernel over the source pixels it covers.
    let (kernel, support, stretch): (fn(f64) -> f64, f64, f64) =
        if scale < 1.0 { (lanczos3, 3.0 / scale, 1.0 / scale) } else { (cubic, 2.0, 1.0) };

    (0..dst)
        .map(|out| {
            // libvips centres pixels when shrinking; enlarging (vips_affine) samples at out / scale - 0.5.
            let centre = if scale < 1.0 { (out as f64 + 0.5) / scale - 0.5 } else { out as f64 / scale - 0.5 };
            let first = (centre - support).ceil() as i64;
            let last = (centre + support).floor() as i64;
            let mut taps: Vec<(i64, f64)> = (first..=last).map(|i| (i, kernel((i as f64 - centre) / stretch))).collect();
            let sum: f64 = taps.iter().map(|(_, w)| w).sum();
            for tap in &mut taps {
                tap.1 /= sum;
            }
            // Pixels past the edge repeat the edge pixel.
            let start = first.clamp(0, src as i64 - 1) as usize;
            let mut folded = vec![0.0; (last.clamp(0, src as i64 - 1) as usize) - start + 1];
            for (i, w) in taps {
                folded[i.clamp(0, src as i64 - 1) as usize - start] += w;
            }
            (start, folded)
        })
        .collect()
}

// The passes run in single precision on whole pixels (`[f32; 4]`, vectorised
// by the compiler), with each axis' weights in one flat table: a ticker is a
// new frame at every frame, so this runs 25 times a second. Within the
// vectors' tolerance of libvips.
type Pixel = [f32; 4];

/// `weights` of one axis, flat: output `i` reads `len[i]` pixels from
/// `start[i]` with the weights from `offset[i]`.
struct Taps {
    start: Vec<usize>,
    offset: Vec<usize>,
    len: Vec<usize>,
    weight: Vec<f32>,
}

impl Taps {
    fn new(src: usize, dst: usize) -> Taps {
        let mut taps =
            Taps { start: Vec::with_capacity(dst), offset: Vec::with_capacity(dst), len: Vec::with_capacity(dst), weight: Vec::new() };
        for (start, w) in weights(src, dst) {
            taps.start.push(start);
            taps.offset.push(taps.weight.len());
            taps.len.push(w.len());
            taps.weight.extend(w.iter().map(|&w| w as f32));
        }
        taps
    }

    fn of(&self, i: usize) -> (usize, &[f32]) {
        (self.start[i], &self.weight[self.offset[i]..self.offset[i] + self.len[i]])
    }
}

/// Rounded as `Math.round` and clipped to a byte; a cast instead of `floor`,
/// which is a library call on baseline x86-64.
fn to_byte(x: f32) -> u8 {
    (x.clamp(0.0, 255.0) + 0.5) as u8
}

/// libvips reduces 8-bit images in 8 bits: each pass rounds and clips.
fn quantized(p: Pixel) -> Pixel {
    p.map(|v| f32::from(to_byte(v)))
}

/// Horizontal pass over the rows of `src` premultiplied RGBA pixels: each
/// to `dst` pixels, handed to `row` (its number and pixels) one at a time.
fn horizontal(input: &[Pixel], src: usize, dst: usize, quantize: bool, mut row: impl FnMut(usize, &[Pixel])) {
    let taps = Taps::new(src, dst);
    let mut out = vec![[0.0; 4]; dst];
    for (y, line) in input.chunks_exact(src).enumerate() {
        for (i, pixel) in out.iter_mut().enumerate() {
            let (start, w) = taps.of(i);
            let mut sum = [0.0f32; 4];
            for (w, p) in w.iter().zip(&line[start..]) {
                for c in 0..4 {
                    sum[c] += w * p[c];
                }
            }
            *pixel = if quantize { quantized(sum) } else { sum };
        }
        row(y, &out);
    }
}

/// Vertical pass: `src` rows of `width` premultiplied RGBA pixels to `dst`
/// rows, handed to `row` one at a time. All taps of a pixel at once: each
/// output pixel is written once.
fn vertical(input: &[Pixel], src: usize, dst: usize, width: usize, quantize: bool, mut row: impl FnMut(usize, &[Pixel])) {
    let taps = Taps::new(src, dst);
    let mut out = vec![[0.0; 4]; width];
    let mut lines: Vec<&[Pixel]> = Vec::new();
    for y in 0..dst {
        let (start, w) = taps.of(y);
        lines.clear();
        lines.extend(input[start * width..(start + w.len()) * width].chunks_exact(width));
        for (x, pixel) in out.iter_mut().enumerate() {
            let mut sum = [0.0f32; 4];
            for (w, line) in w.iter().zip(&lines) {
                let p = line[x];
                for c in 0..4 {
                    sum[c] += w * p[c];
                }
            }
            *pixel = if quantize { quantized(sum) } else { sum };
        }
        row(y, &out);
    }
}

/// Premultiplied pixels back to straight RGBA bytes.
fn unpremultiply(pixels: &[Pixel], out: &mut [u8]) {
    for (pixel, p) in out.chunks_exact_mut(4).zip(pixels) {
        let a = p[3].clamp(0.0, 255.0);
        let alpha = to_byte(a);
        // A pixel that ends up transparent carries no colour, as from libvips.
        if alpha > 0 {
            let k = 255.0 / a;
            for c in 0..3 {
                pixel[c] = to_byte(p[c] * k);
            }
        }
        pixel[3] = alpha;
    }
}

/// RGBA `sw`x`sh` resized to `dw`x`dh`, alpha premultiplied while resampling.
/// Shrinking goes as `vips_reduce` (vertical, then horizontal, each pass in 8 bits),
/// enlarging as one bicubic step without rounding in between. The second
/// pass goes straight to the output row by row: no picture of floats.
pub fn resize(rgba: &[u8], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<u8> {
    let premultiplied: Vec<Pixel> = rgba
        .chunks_exact(4)
        .map(|p| {
            let a = f32::from(p[3]) / 255.0;
            [f32::from(p[0]) * a, f32::from(p[1]) * a, f32::from(p[2]) * a, f32::from(p[3])]
        })
        .collect();

    let mut out = vec![0u8; dw * dh * 4];
    let mut put = |y: usize, row: &[Pixel]| unpremultiply(row, &mut out[y * dw * 4..(y + 1) * dw * 4]);
    if dw < sw || dh < sh {
        let mut first = Vec::with_capacity(sw * dh);
        vertical(&premultiplied, sh, dh, sw, true, |_, row| first.extend_from_slice(row));
        horizontal(&first, sw, dw, true, put);
    } else {
        let mut first = Vec::with_capacity(dw * sh);
        horizontal(&premultiplied, sw, dw, false, |_, row| first.extend_from_slice(row));
        vertical(&first, sh, dh, dw, false, &mut put);
    }
    out
}

/// Rectangles of a `width`x`height` picture, scaled with their pixels to `out_width`x`out_height`.
pub fn scale_rects(rgba: &[u8], width: u32, height: u32, rects: &[(u32, u32, u32, u32)], out_width: u32, out_height: u32) -> Vec<Rect> {
    let sx = f64::from(out_width) / f64::from(width);
    let sy = f64::from(out_height) / f64::from(height);

    rects
        .iter()
        .map(|&(rx, ry, rw, rh)| {
            let (x0, y0) = (rx.saturating_sub(PAD), ry.saturating_sub(PAD));
            let (x1, y1) = ((rx + rw + PAD).min(width), (ry + rh + PAD).min(height));
            let x = (f64::from(x0) * sx).floor() as u32;
            let y = (f64::from(y0) * sy).floor() as u32;
            let w = ((f64::from(x1) * sx).ceil() as u32).min(out_width) - x;
            let h = ((f64::from(y1) * sy).ceil() as u32).min(out_height) - y;
            let src = crop(rgba, width, (x0, y0, x1 - x0, y1 - y0));
            let pixels = resize(&src, (x1 - x0) as usize, (y1 - y0) as usize, w as usize, h as usize);
            Rect { x, y, w, h, pixels: pixels.into() }
        })
        .collect()
}
