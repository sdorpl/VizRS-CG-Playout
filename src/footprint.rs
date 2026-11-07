// Footprints of VizRS pages (PLAN-strefy S1, `area: auto`): where on the
// channel each page draws, learnt from the pictures while it is on air. The
// zones of an ffmpeg run are then the footprints of the pages that are (or
// will be) on air, instead of the whole channel (S2).
//
// A footprint only grows: every picture taken while the page is on air adds
// its visible rectangles, so the far phases of an animation (a slide-in from
// the side, a box that unfolds) end up inside it. Rectangles are snapped to a
// grid with a margin and merged, at most `MAX_ZONES` per page.
//
// With several pages on air a rectangle goes to the page whose footprint it
// overlaps most; one that overlaps none goes to the page taken last (`since`
// of `STATE`), and is left out when two pages were taken at the same moment.
// The key of a page is its VizRS id with its number and the channel size, so
// another show (other ids) starts with no footprints.
//
// Kept in `<etv_config>/vizrs-live/footprints.json` (next to the live API's
// `control.json`); a missing or broken file = no footprints = whole channel.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::area::Area;
use crate::json::{self, Value};

/// Grid the rectangles are snapped to, and the margin around them, in pixels.
const GRID: u32 = 16;
const MARGIN: u32 = 16;
/// Rectangles closer than this are merged into one.
const MERGE_GAP: u32 = 64;
/// Most rectangles kept per page; beyond that their bounding box.
pub const MAX_ZONES: usize = 4;

/// A page on air: its key (`id:number`) and when it was taken (ms; 0 = unknown).
#[derive(Clone, Debug, PartialEq)]
pub struct OnAir {
    pub page: String,
    pub since: u64,
}

/// Pages on the air of a `STATE` reply, in layer order.
pub fn on_air(state_reply: &str) -> Option<Vec<OnAir>> {
    let state = json::parse(state_reply.strip_prefix("OK")?.trim()).ok()?;
    let layers = state.get("layers")?.as_array()?;
    let mut pages = Vec::new();
    for layer in layers {
        let Some(page) = layer.get("page") else { continue };
        let key = match page {
            Value::Object(_) => {
                let number = page.get("number").map(text).unwrap_or_default();
                let id = page.get("id").map(text).unwrap_or_default();
                if number.is_empty() && id.is_empty() {
                    continue;
                }
                format!("{id}:{number}")
            }
            Value::Null => continue,
            other => format!(":{}", text(other)),
        };
        let since = layer.get("since").and_then(Value::as_f64).map_or(0, |s| s.max(0.0) as u64);
        pages.push(OnAir { page: key, since });
    }
    Some(pages)
}

fn text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => {
            if n.fract() == 0.0 {
                format!("{}", *n as i64)
            } else {
                n.to_string()
            }
        }
        _ => String::new(),
    }
}

/// The footprints of the pages, per channel size.
#[derive(Debug, Default)]
pub struct Footprints {
    path: Option<PathBuf>,
    /// `page@WxH` → rectangles.
    pages: BTreeMap<String, Vec<Area>>,
    dirty: bool,
}

fn key(page: &str, (w, h): (u32, u32)) -> String {
    format!("{page}@{w}x{h}")
}

impl Footprints {
    /// Footprints kept in `path` (read now; none if missing or broken).
    pub fn load(path: Option<PathBuf>) -> Footprints {
        let mut footprints = Footprints { path, ..Footprints::default() };
        let Some(path) = footprints.path.as_ref() else { return footprints };
        let Ok(text) = std::fs::read_to_string(path) else { return footprints };
        let Ok(doc) = json::parse(&text) else {
            crate::warn!("footprints: {} is not JSON; starting empty", path.display());
            return footprints;
        };
        if let Some(Value::Object(pages)) = doc.get("pages") {
            for (page, list) in pages {
                let size =
                    page.rsplit_once('@').and_then(|(_, s)| s.split_once('x')).and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)));
                let Some((w, h)) = size else { continue };
                let areas: Vec<Area> =
                    list.as_array().unwrap_or(&[]).iter().filter_map(|a| Area::parse(a.as_str()?, w, h).ok().flatten()).collect();
                if !areas.is_empty() {
                    footprints.pages.insert(page.clone(), areas);
                }
            }
        }
        footprints
    }

    /// The file next to the live API's `control.json`, if ErsatzTV's config is known.
    pub fn default_path(etv_config: &str) -> Option<PathBuf> {
        (!etv_config.is_empty()).then(|| Path::new(etv_config).join("vizrs-live").join("footprints.json"))
    }

    /// How many page footprints are known (all sizes).
    pub fn len(&self) -> usize {
        self.pages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    /// The footprint of `page` on a channel of `size`, if learnt.
    pub fn get(&self, page: &str, size: (u32, u32)) -> Option<&[Area]> {
        self.pages.get(&key(page, size)).map(Vec::as_slice)
    }

    /// Learns from one picture: `rects` (x, y, w, h in channel pixels) drawn
    /// while `pages` are on air. Returns whether a footprint grew.
    pub fn learn(&mut self, pages: &[OnAir], rects: &[(u32, u32, u32, u32)], size: (u32, u32)) -> bool {
        if pages.is_empty() || rects.is_empty() {
            return false;
        }
        let latest = pages.iter().map(|p| p.since).max().unwrap_or(0);
        let newest: Vec<&OnAir> = pages.iter().filter(|p| p.since == latest).collect();
        let mut grown = false;
        for &rect in rects {
            let area = snap(rect, size);
            let owner = if pages.len() == 1 {
                Some(&pages[0])
            } else {
                let best = pages
                    .iter()
                    .filter_map(|p| Some((p, self.get(&p.page, size)?.iter().map(|f| overlap(*f, area)).max().unwrap_or(0))))
                    .filter(|(_, o)| *o > 0)
                    .max_by_key(|(_, o)| *o)
                    .map(|(p, _)| p);
                best.or_else(|| (newest.len() == 1).then(|| newest[0]))
            };
            if let Some(owner) = owner {
                grown |= self.add(&owner.page, area, size);
            }
        }
        grown
    }

    fn add(&mut self, page: &str, area: Area, size: (u32, u32)) -> bool {
        let list = self.pages.entry(key(page, size)).or_default();
        if list.iter().any(|a| contains(*a, area)) {
            return false;
        }
        list.push(area);
        let merged = merge(std::mem::take(list), size);
        *list = merged;
        self.dirty = true;
        true
    }

    /// Writes the file if something changed since the last save.
    pub fn save(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let Some(path) = self.path.as_ref() else { return };
        let pages: Vec<(String, Value)> = self
            .pages
            .iter()
            .map(|(page, areas)| (page.clone(), Value::Array(areas.iter().map(|a| Value::String(a.geometry())).collect())))
            .collect();
        let doc = json::object([("pages", Value::Object(pages))]);
        let write = || -> std::io::Result<()> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let temporary = path.with_extension("json.tmp");
            std::fs::write(&temporary, doc.to_string())?;
            std::fs::rename(&temporary, path)
        };
        if let Err(e) = write() {
            crate::warn!("footprints: cannot write {}: {e}", path.display());
        }
    }
}

/// Zones covering more of the channel than this are not worth an atlas: the whole channel.
pub const WHOLE_SHARE: f64 = 0.6;

/// The zones of an ffmpeg run (PLAN-strefy S2, S3a): the footprints of all the
/// pages known on a channel of `size`, merged – so a known page the operator
/// takes in the middle of the run is not cut off. `None` = the whole channel:
/// a page on air (`on_air`, keys `id:number`) or a page the rules may take
/// (`numbers`, as in `TAKE 4001`) without a footprint yet, no footprint at all,
/// or zones over `WHOLE_SHARE` of the channel.
pub fn zones(footprints: &Footprints, on_air: &[OnAir], numbers: &[String], size: (u32, u32)) -> Option<Vec<Area>> {
    let suffix = format!("@{}x{}", size.0, size.1);
    for page in on_air {
        footprints.get(&page.page, size)?;
    }
    for number in numbers {
        let rule = format!(":{number}{suffix}");
        if !footprints.pages.keys().any(|key| key.ends_with(&rule)) {
            return None;
        }
    }
    let areas: Vec<Area> =
        footprints.pages.iter().filter(|(key, _)| key.ends_with(&suffix)).flat_map(|(_, list)| list.iter().copied()).collect();
    if areas.is_empty() {
        return None;
    }
    let areas = merge(areas, size);
    let pixels: u64 = areas.iter().map(|a| u64::from(a.w) * u64::from(a.h)).sum();
    let share = pixels as f64 / (f64::from(size.0) * f64::from(size.1));
    (share <= WHOLE_SHARE).then_some(areas)
}

/// Whether every rectangle (x, y, w, h) lies inside one of the zones.
pub fn covered(zones: &[Area], rects: &[(u32, u32, u32, u32)]) -> bool {
    rects.iter().all(|&(x, y, w, h)| zones.iter().any(|z| contains(*z, Area { x, y, w, h })))
}

/// A rectangle snapped out to the grid, with the margin, even and inside the channel.
fn snap((x, y, w, h): (u32, u32, u32, u32), (cw, ch): (u32, u32)) -> Area {
    let down = |v: u32| v.saturating_sub(MARGIN) / GRID * GRID;
    let up = |v: u32, max: u32| (v.saturating_add(MARGIN).div_ceil(GRID) * GRID).min(max);
    let (x0, y0) = (down(x), down(y));
    let (x1, y1) = (up(x.saturating_add(w), cw), up(y.saturating_add(h), ch));
    Area { x: x0 & !1, y: y0 & !1, w: (x1 - x0.min(x1)).max(2) & !1, h: (y1 - y0.min(y1)).max(2) & !1 }
}

fn overlap(a: Area, b: Area) -> u64 {
    let w = (a.x + a.w).min(b.x + b.w).saturating_sub(a.x.max(b.x));
    let h = (a.y + a.h).min(b.y + b.h).saturating_sub(a.y.max(b.y));
    u64::from(w) * u64::from(h)
}

fn contains(outer: Area, inner: Area) -> bool {
    inner.x >= outer.x && inner.y >= outer.y && inner.x + inner.w <= outer.x + outer.w && inner.y + inner.h <= outer.y + outer.h
}

fn union(a: Area, b: Area) -> Area {
    let (x0, y0) = (a.x.min(b.x), a.y.min(b.y));
    let (x1, y1) = ((a.x + a.w).max(b.x + b.w), (a.y + a.h).max(b.y + b.h));
    Area { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
}

/// Gap between two rectangles (0 when they touch or overlap): the larger of the two axes.
fn gap(a: Area, b: Area) -> u32 {
    let dx = b.x.saturating_sub(a.x + a.w).max(a.x.saturating_sub(b.x + b.w));
    let dy = b.y.saturating_sub(a.y + a.h).max(a.y.saturating_sub(b.y + b.h));
    dx.max(dy)
}

/// Rectangles closer than `MERGE_GAP` merged, at most `MAX_ZONES` (else their bounding box).
pub fn merge(mut areas: Vec<Area>, _size: (u32, u32)) -> Vec<Area> {
    loop {
        let pair =
            (0..areas.len()).flat_map(|i| (i + 1..areas.len()).map(move |j| (i, j))).find(|&(i, j)| gap(areas[i], areas[j]) < MERGE_GAP);
        let Some((i, j)) = pair else { break };
        let b = areas.remove(j);
        areas[i] = union(areas[i], b);
    }
    // Contained ones go.
    let all = areas.clone();
    areas.retain(|a| !all.iter().any(|b| b != a && contains(*b, *a)));
    if areas.len() > MAX_ZONES {
        let bounding = areas.iter().copied().reduce(union).expect("non-empty");
        return vec![bounding];
    }
    areas
}

#[cfg(test)]
mod tests {
    use super::*;

    const HD: (u32, u32) = (1920, 1080);

    fn page(p: &str, since: u64) -> OnAir {
        OnAir { page: p.into(), since }
    }

    #[test]
    fn state_reply_gives_pages_with_ids_and_take_time() {
        let reply = r#"OK {"layers":[{"layer":"back","page":{"id":"ti","number":3003,"name":"Ticker"},"since":1794086357842},{"layer":"main","page":{"id":"p9","number":4001},"since":1794086357799},{"layer":"x","page":null}],"rev":0}"#;
        assert_eq!(on_air(reply).unwrap(), vec![page("ti:3003", 1794086357842), page("p9:4001", 1794086357799)]);
        assert_eq!(on_air(r#"OK {"layers":[{"layer":"main","page":"1001"}]}"#).unwrap(), vec![page(":1001", 0)]);
        assert_eq!(on_air("OK {}"), None);
        assert_eq!(on_air("ERR no"), None);
    }

    #[test]
    fn one_page_learns_snapped_rectangles_and_only_grows() {
        let mut f = Footprints::default();
        assert!(f.learn(&[page("p9:4001", 1)], &[(100, 800, 600, 120)], HD));
        assert_eq!(f.get("p9:4001", HD).unwrap(), [Area { x: 80, y: 784, w: 640, h: 160 }]);
        // inside: nothing new
        assert!(!f.learn(&[page("p9:4001", 1)], &[(120, 820, 100, 50)], HD));
        // a later phase of the animation further right: the footprint grows (merged, gap < 64)
        assert!(f.learn(&[page("p9:4001", 1)], &[(700, 800, 300, 120)], HD));
        assert_eq!(f.get("p9:4001", HD).unwrap(), [Area { x: 80, y: 784, w: 944, h: 160 }]);
        // other channel size: its own footprint
        assert!(f.get("p9:4001", (1280, 720)).is_none());
    }

    #[test]
    fn far_apart_rectangles_stay_separate_up_to_the_limit() {
        let mut f = Footprints::default();
        f.learn(&[page("a:1", 1)], &[(0, 950, 1920, 100), (1700, 40, 160, 160)], HD);
        assert_eq!(f.get("a:1", HD).unwrap().len(), 2);
        let many: Vec<_> = (0..6).map(|i| (i * 300, 0, 50, 50)).collect();
        f.learn(&[page("b:2", 1)], &many, HD);
        let zones = f.get("b:2", HD).unwrap();
        assert!(zones.len() <= MAX_ZONES, "at most MAX_ZONES: {zones:?}");
        assert!(many.iter().all(|&(x, y, w, h)| zones.iter().any(|z| contains(*z, Area { x, y, w, h }))), "all pixels covered");
    }

    #[test]
    fn with_several_pages_pixels_go_to_the_owner_or_the_page_taken_last() {
        let mut f = Footprints::default();
        f.learn(&[page("logo:1", 1)], &[(1700, 40, 160, 160)], HD);
        // logo on air, then a title taken later: the logo's pixels stay the logo's, new ones are the title's
        f.learn(&[page("logo:1", 1), page("title:2", 5)], &[(1700, 40, 160, 160), (100, 850, 800, 120)], HD);
        assert_eq!(f.get("logo:1", HD).unwrap().len(), 1);
        assert_eq!(f.get("title:2", HD).unwrap(), [Area { x: 80, y: 832, w: 848, h: 160 }]);
        // two pages taken at the same moment, a new rectangle owned by none: nobody learns
        assert!(!f.learn(&[page("c:3", 9), page("d:4", 9)], &[(800, 400, 100, 100)], HD));
    }

    #[test]
    fn zones_of_a_run_from_pages_on_air_and_rule_pages() {
        let mut f = Footprints::default();
        f.learn(&[page("logo:3001", 1)], &[(1700, 40, 160, 160)], HD);
        f.learn(&[page("p9:4001", 1)], &[(100, 850, 800, 120)], HD);
        // logo on air, the title will be taken by on_clip (TAKE 4001)
        let z = zones(&f, &[page("logo:3001", 1)], &["4001".into()], HD).unwrap();
        assert_eq!(z.len(), 2);
        // nothing on air and no rules: still all the known pages (the operator may take them)
        assert_eq!(zones(&f, &[], &[], HD).unwrap().len(), 2);
        assert!(covered(&z, &[(1700, 40, 160, 160), (100, 850, 800, 120)]));
        assert!(!covered(&z, &[(900, 400, 50, 50)]));
        // a page with no footprint: the whole channel
        assert_eq!(zones(&f, &[page("x:9", 1)], &[], HD), None);
        assert_eq!(zones(&f, &[], &["5005".into()], HD), None);
        // nothing known for another size: the whole channel
        assert_eq!(zones(&f, &[], &[], (1280, 720)), None);
        // a full-screen page: over WHOLE_SHARE = the whole channel
        f.learn(&[page("full:7", 1)], &[(0, 0, 1920, 1080)], HD);
        assert_eq!(zones(&f, &[page("full:7", 1)], &[], HD), None);
    }

    #[test]
    fn saved_and_loaded() {
        let dir = std::env::temp_dir().join(format!("vizrs-footprints-{}", std::process::id()));
        let path = dir.join("vizrs-live").join("footprints.json");
        let mut f = Footprints::load(Some(path.clone()));
        f.learn(&[page("p9:4001", 1)], &[(100, 800, 600, 120)], HD);
        f.save();
        let back = Footprints::load(Some(path.clone()));
        assert_eq!(back.get("p9:4001", HD), f.get("p9:4001", HD));
        std::fs::write(&path, "not json").unwrap();
        assert!(Footprints::load(Some(path)).get("p9:4001", HD).is_none(), "broken file: no footprints");
        let _ = std::fs::remove_dir_all(dir);
        assert_eq!(Footprints::default_path(""), None);
    }
}
