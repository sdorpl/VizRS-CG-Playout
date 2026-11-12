// Graphics area: the part of the channel the graphics occupy, e.g. a ticker
// `1920x140+0+940` (ImageMagick geometry). Chromium then draws only that part
// of /output (the page still lays out at full size), the sidecar decodes and
// converts only it, and the ffmpeg overlay gets frames of its size, overlaid
// at its position. Everything outside the area is cut off. A channel may have
// several areas (`1920x140+0+930,140x140+1760+40`): the overlay then gets them
// stacked in one frame (`Layout::atlas`) and overlays each at its place.

use crate::packets::{Rect, crop};

/// A rectangle of the channel in pixels; even, so 4:2:0 chroma stays aligned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Area {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Area {
    /// `""` or `full` = the whole channel (None); `WxH+X+Y` (`+X+Y` may be
    /// left out) in channel pixels. Clamped to the channel and made even:
    /// the position down, the size up. A space stands for `+` (an unencoded
    /// `+` in a URL query arrives as one).
    pub fn parse(spec: &str, width: u32, height: u32) -> Result<Option<Area>, String> {
        let value = spec.trim().to_ascii_lowercase().replace(' ', "+");
        if value.is_empty() || value == "full" {
            return Ok(None);
        }
        let bad = || format!("bad graphics area \"{spec}\" (full or WxH+X+Y, e.g. 1920x140+0+940)");
        let number =
            |text: &str| (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())).then(|| text.parse::<u32>().ok()).flatten();
        let (size, position) = value.split_once('+').map_or((value.as_str(), "0+0"), |(size, position)| (size, position));
        let (w, h) = size.split_once('x').and_then(|(w, h)| Some((number(w)?, number(h)?))).ok_or_else(bad)?;
        let (x, y) = position.split_once('+').and_then(|(x, y)| Some((number(x)?, number(y)?))).ok_or_else(bad)?;

        let (x0, y0) = (x.min(width) & !1, y.min(height) & !1);
        let x1 = x.saturating_add(w).min(width).saturating_add(1) & !1;
        let y1 = y.saturating_add(h).min(height).saturating_add(1) & !1;
        let (x1, y1) = (x1.min(width), y1.min(height));
        if x1 <= x0 || y1 <= y0 || w == 0 || h == 0 {
            return Err(format!("graphics area \"{spec}\" is outside the {width}x{height} channel"));
        }
        let area = Area { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
        Ok((area != Area { x: 0, y: 0, w: width, h: height }).then_some(area))
    }

    /// The same part of a picture of `to` instead of `from` (render size).
    pub fn scaled(self, from: (u32, u32), to: (u32, u32)) -> Area {
        if from == to {
            return self;
        }
        let edge = |v: u32, f: u32, t: u32| (f64::from(v) * f64::from(t) / f64::from(f)).round() as u32;
        let (x, y) = (edge(self.x, from.0, to.0), edge(self.y, from.1, to.1));
        let (x1, y1) = (edge(self.x + self.w, from.0, to.0).min(to.0), edge(self.y + self.h, from.1, to.1).min(to.1));
        Area { x, y, w: x1.saturating_sub(x).max(1), h: y1.saturating_sub(y).max(1) }
    }

    /// `WxH+X+Y`, as `/health` shows it.
    pub fn geometry(self) -> String {
        format!("{}x{}+{}+{}", self.w, self.h, self.x, self.y)
    }

    fn overlaps(self, other: Area) -> bool {
        self.x < other.x + other.w && other.x < self.x + self.w && self.y < other.y + other.h && other.y < self.y + self.h
    }

    /// `auto`: the zones follow the graphics on air (PLAN-strefy; footprints, src/footprint.rs).
    pub fn is_auto(spec: &str) -> bool {
        spec.trim().eq_ignore_ascii_case("auto")
    }

    /// Areas of a channel, separated by commas; empty = the whole channel.
    /// Overlapping areas become one (their bounding box): no pixel twice.
    /// `auto` draws the whole channel (the zones of a run come from footprints).
    pub fn parse_list(spec: &str, width: u32, height: u32) -> Result<Vec<Area>, String> {
        if Area::is_auto(spec) {
            return Ok(Vec::new());
        }
        let mut areas: Vec<Area> = Vec::new();
        for part in spec.split(',').filter(|p| !p.trim().is_empty()) {
            match Area::parse(part, width, height)? {
                Some(area) => areas.push(area),
                // One of them is the whole channel: the whole channel.
                None => return Ok(Vec::new()),
            }
        }
        let mut merged = true;
        while merged {
            merged = false;
            'pairs: for i in 0..areas.len() {
                for j in i + 1..areas.len() {
                    if areas[i].overlaps(areas[j]) {
                        areas[i] = union(&[areas[i], areas[j]]);
                        areas.remove(j);
                        merged = true;
                        break 'pairs;
                    }
                }
            }
        }
        Ok(if areas.len() == 1 && areas[0] == (Area { x: 0, y: 0, w: width, h: height }) { Vec::new() } else { areas })
    }
}

/// The bounding box of `areas` (at least one).
pub fn union(areas: &[Area]) -> Area {
    let x = areas.iter().map(|a| a.x).min().unwrap_or(0);
    let y = areas.iter().map(|a| a.y).min().unwrap_or(0);
    let x1 = areas.iter().map(|a| a.x + a.w).max().unwrap_or(0);
    let y1 = areas.iter().map(|a| a.y + a.h).max().unwrap_or(0);
    Area { x, y, w: x1 - x, h: y1 - y }
}

/// `WxH+X+Y,…` of a list, as `/health` shows it.
pub fn geometries(areas: &[Area]) -> String {
    areas.iter().map(|a| a.geometry()).collect::<Vec<_>>().join(",")
}

/// The part of `rect` inside `area`, moved by `(dx, dy)` (signed).
pub fn cut(rect: &Rect, area: Area, (dx, dy): (i64, i64)) -> Option<Rect> {
    let x0 = rect.x.max(area.x);
    let y0 = rect.y.max(area.y);
    let x1 = (rect.x + rect.w).min(area.x + area.w);
    let y1 = (rect.y + rect.h).min(area.y + area.h);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let pixels = crop(&rect.pixels, rect.w, (x0 - rect.x, y0 - rect.y, x1 - x0, y1 - y0));
    let (x, y) = (i64::from(x0) + dx, i64::from(y0) + dy);
    Some(Rect { x: u32::try_from(x).ok()?, y: u32::try_from(y).ok()?, w: x1 - x0, h: y1 - y0, pixels: pixels.into() })
}

/// Rectangles cut to `areas` (all of them when there are none).
pub fn cut_to(rects: Vec<Rect>, areas: &[Area]) -> Vec<Rect> {
    if areas.is_empty() {
        return rects;
    }
    rects.iter().flat_map(|rect| areas.iter().filter_map(|&area| cut(rect, area, (0, 0)))).collect()
}

/// What a raw frame for ffmpeg carries: parts of the channel (`Area`) at a
/// row of the frame, left-aligned. The whole channel, one graphics area, or
/// several stacked one below the other.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub width: u32,
    pub height: u32,
    /// Each part of the channel and the row of the frame it starts at.
    pub parts: Vec<(Area, u32)>,
}

impl Layout {
    pub fn whole(width: u32, height: u32) -> Layout {
        Layout { width, height, parts: vec![(Area { x: 0, y: 0, w: width, h: height }, 0)] }
    }

    /// `areas` stacked: as wide as the widest, as high as all together.
    pub fn atlas(areas: &[Area]) -> Layout {
        let mut parts = Vec::with_capacity(areas.len());
        let mut row = 0;
        for &area in areas {
            parts.push((area, row));
            row += area.h;
        }
        Layout { width: areas.iter().map(|a| a.w).max().unwrap_or(0), height: row, parts }
    }

    /// One part at the frame's origin: the frame is that part of the picture.
    pub fn single(&self) -> Option<Area> {
        match self.parts.as_slice() {
            [(area, 0)] if (area.w, area.h) == (self.width, self.height) => Some(*area),
            _ => None,
        }
    }

    /// The picture's rectangles (channel coordinates) in frame coordinates.
    pub fn place(&self, rects: &[Rect]) -> Vec<Rect> {
        let mut out = Vec::new();
        for &(area, row) in &self.parts {
            let shift = (-i64::from(area.x), i64::from(row) - i64::from(area.y));
            out.extend(rects.iter().filter_map(|rect| cut(rect, area, shift)));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_full_and_errors() {
        let area = |spec| Area::parse(spec, 1920, 1080);
        assert_eq!(area(""), Ok(None));
        assert_eq!(area(" Full "), Ok(None));
        assert_eq!(area("1920x1080"), Ok(None));
        assert_eq!(area("1920x140+0+940"), Ok(Some(Area { x: 0, y: 940, w: 1920, h: 140 })));
        assert_eq!(area("400X100"), Ok(Some(Area { x: 0, y: 0, w: 400, h: 100 })));
        assert_eq!(area("1920x140 0 940"), area("1920x140+0+940"));
        // Even, inside the channel.
        assert_eq!(area("301x101+11+977"), Ok(Some(Area { x: 10, y: 976, w: 302, h: 102 })));
        assert_eq!(area("500x500+1700+900"), Ok(Some(Area { x: 1700, y: 900, w: 220, h: 180 })));
        for bad in ["1920", "axb", "10x10+1", "10x10-5-5", "0x10", "10x10+1920+0"] {
            assert!(area(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn lists_merge_overlaps() {
        let list = |spec| Area::parse_list(spec, 1920, 1080);
        assert!(Area::is_auto(" AUTO ") && list("auto").unwrap().is_empty(), "auto: the whole channel (zones from footprints)");
        assert_eq!(list(""), Ok(vec![]));
        assert_eq!(list("full"), Ok(vec![]));
        assert_eq!(list("1920x140+0+930,full"), Ok(vec![]));
        let (ticker, logo) = (Area { x: 0, y: 930, w: 1920, h: 140 }, Area { x: 1760, y: 40, w: 140, h: 140 });
        assert_eq!(list("1920x140+0+930, 140x140+1760+40"), Ok(vec![ticker, logo]));
        // Overlapping: one box; touching: two.
        assert_eq!(list("100x100+0+0,100x100+50+50"), Ok(vec![Area { x: 0, y: 0, w: 150, h: 150 }]));
        assert_eq!(list("100x100+0+0,100x100+100+0").unwrap().len(), 2);
        assert!(list("1920x140+0+930,bad").is_err());
        assert_eq!(geometries(&[ticker, logo]), "1920x140+0+930,140x140+1760+40");
    }

    #[test]
    fn rectangles_cut_and_stacked() {
        let rect = Rect { x: 2, y: 2, w: 4, h: 4, pixels: (0..64).collect::<Vec<u8>>().into() };
        let cut_rect = cut(&rect, Area { x: 4, y: 0, w: 10, h: 4 }, (0, 0)).unwrap();
        assert_eq!((cut_rect.x, cut_rect.y, cut_rect.w, cut_rect.h), (4, 2, 2, 2));
        assert_eq!(cut_rect.pixels[..], [8, 9, 10, 11, 12, 13, 14, 15, 24, 25, 26, 27, 28, 29, 30, 31]);
        assert!(cut(&rect, Area { x: 6, y: 0, w: 2, h: 2 }, (0, 0)).is_none());

        let atlas = Layout::atlas(&[Area { x: 0, y: 4, w: 8, h: 2 }, Area { x: 4, y: 0, w: 4, h: 4 }]);
        assert_eq!((atlas.width, atlas.height), (8, 6));
        assert_eq!(atlas.single(), None);
        assert_eq!(Layout::whole(8, 6).single(), Some(Area { x: 0, y: 0, w: 8, h: 6 }));
        let placed = atlas.place(std::slice::from_ref(&rect));
        // Rows 4-5 of the rectangle go to row 0; its part in x 4-5, rows 2-3, below the first area (row 2 + 2).
        assert_eq!(placed.iter().map(|r| (r.x, r.y, r.w, r.h)).collect::<Vec<_>>(), [(2, 0, 4, 2), (0, 4, 2, 2)]);
    }

    #[test]
    fn scaled_to_the_render_size() {
        let ticker = Area { x: 0, y: 940, w: 1920, h: 140 };
        assert_eq!(ticker.scaled((1920, 1080), (1920, 1080)), ticker);
        assert_eq!(ticker.scaled((1920, 1080), (1280, 720)), Area { x: 0, y: 627, w: 1280, h: 93 });
        assert_eq!(ticker.geometry(), "1920x140+0+940");
    }
}
