// Footprints of VizRS pages (PLAN-strefy S1, `area: auto`): where on the
// channel each page draws, learnt from the pictures while it is on air. The
// zones of an ffmpeg run are then the footprints of the pages that are (or
// will be) on air, instead of the whole channel (S2).
//
// A footprint only grows: every picture taken while the page is on air adds
// what it draws, so the far phases of an animation (a slide-in from the side,
// a box that unfolds) end up inside it. Pictures are read as cells of a
// `GRID` (a cell is drawn if any of its pixels has alpha), not as visible
// rectangles: graphics stacked one above the other (a QR code beside a title
// over a ticker) are one rectangle, but stay their own cells. The cells of a
// page are joined into rectangles, snapped to the grid with a margin and
// merged, at most `MAX_ZONES` per page.
//
// With several pages on air, a cell that came up within `FRESH` of a take
// (`since` of `STATE`; takes within `TIE` of each other are one moment and
// teach nobody), not drawn for `QUIET` before it, is the page's taken then,
// even inside another page's footprint (a wipe's bar over a title's bar); a
// crawl's letters coming and going are not a new page's. Else a cell inside
// the footprint of a page on air is known. Else, with every page on air
// known, a cell next to the footprint of one page only (`NEAR`) is that
// page's: its animation going further, a countdown's digits. Else the page taken last before the cell came up,
// within `RECENT`, with the same quiet before; a cell drawn for longer than
// `RECENT` goes to the only page on air with no footprint yet. Else nobody
// learns it: a wrong footprint never shrinks, a missing one costs the whole
// channel. A page whose take the sidecar saw and that drew nothing of its own
// for `RECENT` (every cell drawn inside the footprints on air) gets an empty
// footprint. For `OUT_GRACE` after a page went off the air nobody learns: its
// out animation is still drawn, and a page alone would get its pixels. Cells
// drawn at the first picture after a pause (`LOOK_GAP`: no session, so no
// pictures) came up unseen: not fresh, not after any take.
// The key of a page is its VizRS id with its number and the channel size, so
// another show (other ids) starts with no footprints.
//
// Kept in `<etv_config>/vizrs-live/footprints.json` (next to the live API's
// `control.json`); a missing or broken file, or one of an older `VERSION`
// (footprints learnt from visible rectangles), = no footprints = whole channel.

use std::collections::{BTreeMap, HashMap};
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
/// A cell coming up this long (ms) after a take is the page's taken then.
const RECENT: u64 = 15_000;
/// ...if it was not drawn for this long (ms) before the take.
const QUIET: u64 = 2_000;
/// Within this long (ms) after a take a new cell is the new page's, even next to another page.
const FRESH: u64 = 3_000;
/// A cell this close (px) to one page's footprint (and to no other's) is that page's.
const NEAR: u32 = 32;
/// Takes this close (ms) are one moment: their new cells are nobody's.
const TIE: u64 = 500;
/// After a page goes off the air its out animation may last this long (ms): no learning.
const OUT_GRACE: u64 = 3_000;
/// Pictures further apart than this (ms) are not one watch: what came up in between came up unseen.
const LOOK_GAP: u64 = 5_000;
/// `Seen::since` of a cell that came up unseen (long ago, for the rules).
const UNSEEN: u64 = 1;
/// Clocks of VizRS and the sidecar may differ by this much (ms); a cell seen
/// more than this before a take was there before it.
const SLACK: u64 = 250;
/// Version of `footprints.json`; older files are left unread.
const VERSION: u64 = 2;

/// The cells of a picture that it draws on (a `GRID` square with any alpha).
#[derive(Clone, Debug)]
pub struct Cells {
    size: (u32, u32),
    cols: usize,
    rows: usize,
    on: Vec<bool>,
}

impl Cells {
    fn empty((w, h): (u32, u32)) -> Cells {
        let (cols, rows) = (w.div_ceil(GRID) as usize, h.div_ceil(GRID) as usize);
        Cells { size: (w, h), cols, rows, on: vec![false; cols * rows] }
    }

    /// The cells of a picture's visible rectangles (RGBA, channel coordinates).
    pub fn of_picture(rects: &[crate::packets::Rect], size: (u32, u32)) -> Cells {
        let mut cells = Cells::empty(size);
        let g = GRID as usize;
        for rect in rects {
            let stride = rect.w as usize * 4;
            for row in 0..rect.h as usize {
                let y = rect.y as usize + row;
                if y >= size.1 as usize {
                    break;
                }
                let line = &rect.pixels[row * stride..(row + 1) * stride];
                let base = (y / g) * cells.cols;
                let mut col = 0usize;
                while col < rect.w as usize {
                    let x = rect.x as usize + col;
                    if x >= size.0 as usize {
                        break;
                    }
                    // To the end of this cell: one alpha is enough.
                    let end = ((x / g + 1) * g - rect.x as usize).min(rect.w as usize);
                    let cell = base + x / g;
                    if !cells.on[cell] && line[col * 4..end * 4].chunks_exact(4).any(|p| p[3] != 0) {
                        cells.on[cell] = true;
                    }
                    col = end;
                }
            }
        }
        cells
    }

    /// Cells of boxes (x, y, w, h) drawn whole.
    pub fn of_boxes(boxes: &[(u32, u32, u32, u32)], size: (u32, u32)) -> Cells {
        let mut cells = Cells::empty(size);
        for &(x, y, w, h) in boxes {
            let (x1, y1) = ((x + w).min(size.0), (y + h).min(size.1));
            if x1 <= x || y1 <= y {
                continue;
            }
            for row in (y / GRID) as usize..=((y1 - 1) / GRID) as usize {
                for col in (x / GRID) as usize..=((x1 - 1) / GRID) as usize {
                    cells.on[row * cells.cols + col] = true;
                }
            }
        }
        cells
    }

    fn area(&self, index: usize) -> Area {
        let (col, row) = ((index % self.cols) as u32, (index / self.cols) as u32);
        let (x, y) = (col * GRID, row * GRID);
        Area { x, y, w: GRID.min(self.size.0 - x), h: GRID.min(self.size.1 - y) }
    }

    /// The cells drawn, as runs along each row of cells: (x, y, w, h).
    pub fn boxes(&self) -> Vec<(u32, u32, u32, u32)> {
        let mut out = Vec::new();
        for row in 0..self.rows {
            let mut col = 0;
            while col < self.cols {
                if !self.on[row * self.cols + col] {
                    col += 1;
                    continue;
                }
                let start = col;
                while col < self.cols && self.on[row * self.cols + col] {
                    col += 1;
                }
                let (a, b) = (self.area(row * self.cols + start), self.area(row * self.cols + col - 1));
                out.push((a.x, a.y, b.x + b.w - a.x, a.h));
            }
        }
        out
    }
}

/// When each cell of a channel's picture came up (ms; 0 = not drawn), and
/// when it was drawn last before that; `start` = since when the channel is watched.
#[derive(Debug, Default)]
struct Seen {
    start: u64,
    /// The pages on air at the last picture, and when one of them last went off.
    pages: Vec<String>,
    left: Option<u64>,
    /// When the last picture was looked at.
    looked: u64,
    size: (u32, u32),
    since: Vec<u64>,
    before: Vec<u64>,
    last: Vec<u64>,
}

impl Seen {
    fn update(&mut self, cells: &Cells, now: u64) {
        if self.size != cells.size || self.since.len() != cells.on.len() {
            let n = cells.on.len();
            *self = Seen { start: now, size: cells.size, since: vec![0; n], before: vec![0; n], last: vec![0; n], ..Seen::default() };
        }
        // The first picture after a pause (no session, no capture): its cells
        // came up at some unknown moment, maybe with a take seen only now.
        let first = self.looked == 0 || now.saturating_sub(self.looked) > LOOK_GAP;
        self.looked = now;
        for (i, &on) in cells.on.iter().enumerate() {
            if on {
                if first && self.since[i] == 0 {
                    self.since[i] = UNSEEN;
                    self.before[i] = now;
                } else if self.since[i] == 0 {
                    self.since[i] = now;
                    self.before[i] = self.last[i];
                }
                self.last[i] = now;
            } else {
                self.since[i] = 0;
            }
        }
    }
}

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
    /// Per channel: when its cells came up (not kept in the file).
    seen: HashMap<String, Seen>,
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
        if doc.get("version").and_then(Value::as_f64) != Some(VERSION as f64) {
            crate::info!("footprints: {} is of an older version; learning anew", path.display());
            footprints.dirty = true;
            return footprints;
        }
        if let Some(Value::Object(pages)) = doc.get("pages") {
            for (page, list) in pages {
                let size =
                    page.rsplit_once('@').and_then(|(_, s)| s.split_once('x')).and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)));
                let Some((w, h)) = size else { continue };
                // An empty list is a footprint too: a page that draws nothing of its own.
                let Some(list) = list.as_array() else { continue };
                let areas: Vec<Area> = list.iter().filter_map(|a| Area::parse(a.as_str()?, w, h).ok().flatten()).collect();
                footprints.pages.insert(page.clone(), areas);
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

    /// Learns from one picture of `channel` at `now` (ms, the clock of
    /// `since`): the `cells` drawn while `pages` are on air. Returns whether a
    /// footprint grew.
    pub fn learn(&mut self, channel: &str, pages: &[OnAir], cells: &Cells, now: u64) -> bool {
        let seen = self.seen.entry(channel.to_owned()).or_default();
        seen.update(cells, now);
        // A page gone off the air may still be drawn (its out animation): wait.
        if seen.pages.iter().any(|old| !pages.iter().any(|p| p.page == *old)) {
            seen.left = Some(now);
        }
        seen.pages = pages.iter().map(|p| p.page.clone()).collect();
        if pages.is_empty() || seen.left.is_some_and(|left| now.saturating_sub(left) < OUT_GRACE) {
            return false;
        }
        let size = cells.size;
        let n = pages.len();
        let known: Vec<Option<Vec<Area>>> = pages.iter().map(|p| self.get(&p.page, size).map(<[Area]>::to_vec)).collect();
        let unknown: Vec<usize> = (0..n).filter(|&p| known[p].is_none()).collect();
        let areas = |p: usize| known[p].as_deref().unwrap_or(&[]);
        let seen = &self.seen[channel];
        let mut owned: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut rules: Vec<Vec<&str>> = vec![Vec::new(); n];
        // Cells drawn outside every footprint on air (owned or not).
        let mut unexplained = false;
        for (i, &on) in cells.on.iter().enumerate() {
            if !on {
                continue;
            }
            let cell = cells.area(i);
            let inside = |p: usize| areas(p).iter().any(|a| contains(*a, cell));
            unexplained |= !(0..n).any(inside);
            let came = seen.since[i];
            let owner = if n == 1 {
                (!inside(0)).then_some((0, "alone on air"))
            } else {
                // The page taken last before the cell came up (takes within TIE of it: none), with the cell dark before.
                let takes: Vec<usize> = (0..n)
                    .filter(|&p| pages[p].since > 0 && pages[p].since <= came + SLACK && came.saturating_sub(pages[p].since) <= RECENT)
                    .collect();
                let latest = takes.iter().map(|&p| pages[p].since).max().unwrap_or(0);
                let last: Vec<usize> = takes.into_iter().filter(|&p| latest - pages[p].since <= TIE).collect();
                let taken = match last.as_slice() {
                    [p] if seen.before[i] + QUIET <= pages[*p].since => Some(*p),
                    _ => None,
                };
                match taken.filter(|&p| came.saturating_sub(pages[p].since) <= FRESH) {
                    // Fresh after a take: the new page's, even inside another page's footprint (a bar over a bar).
                    Some(p) => (!inside(p)).then_some((p, "came up with its take")),
                    None if (0..n).any(inside) => None,
                    None => {
                        // (Only with every page on air known: a page with no footprint may be the nearer one.)
                        let near: Vec<usize> = if unknown.is_empty() {
                            (0..n).filter(|&p| areas(p).iter().any(|a| gap(*a, cell) <= NEAR)).collect()
                        } else {
                            Vec::new()
                        };
                        match (near.as_slice(), taken) {
                            ([p], _) => Some((*p, "next to its footprint")),
                            ([], Some(p)) => Some((p, "came up after its take")),
                            ([], None) if now.saturating_sub(came) > RECENT && unknown.len() == 1 => {
                                Some((unknown[0], "the only page with no footprint"))
                            }
                            _ => None,
                        }
                    }
                }
            };
            if let Some((p, rule)) = owner {
                owned[p].push(i);
                if !rules[p].contains(&rule) {
                    rules[p].push(rule);
                }
            }
        }
        let mut grown = false;
        for (p, list) in owned.iter().enumerate() {
            for (x, y, w, h) in components(cells, list) {
                let area = snap((x, y, w, h), size);
                if self.add(&pages[p].page, area, size) {
                    crate::info!("footprints: {} draws {} ({})", pages[p].page, area.geometry(), rules[p].join(", "));
                    grown = true;
                }
            }
        }
        // A page taken while the channel was watched that drew nothing of its
        // own for RECENT (all inside the footprints of the others): an empty
        // footprint. Its pixels are in the zones while the others are on air;
        // without them, pixels outside the zones widen a run (S3b).
        for &p in &unknown {
            let page = &pages[p];
            if !unexplained
                && owned[p].is_empty()
                && page.since > seen_start(&self.seen, channel).saturating_add(SLACK)
                && now.saturating_sub(page.since) > RECENT
            {
                self.pages.insert(key(&page.page, size), Vec::new());
                self.dirty = true;
                crate::info!("footprints: {} draws nothing of its own (inside the pages on air)", page.page);
                grown = true;
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
        let doc = json::object([("version", VERSION.into()), ("pages", Value::Object(pages))]);
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

/// The zones of an ffmpeg run (PLAN-strefy S2, S3a): the footprints of the
/// pages on air (`on_air`, keys `id:number`) and of the pages the rules may
/// take (`numbers`, as in `TAKE 4001`), merged. An overlay that cannot widen
/// its zones in the middle of the piece (not `follows`, S3b) gets the
/// footprints of all the pages known on a channel of `size` too, so a known
/// page the operator takes is not cut off; one that can widens when that
/// happens, and a full-screen page seen once (a wipe) does not cost every
/// run the whole channel. `None` = the whole channel: a page on air or of the
/// rules without a footprint yet, no footprint at all, or zones over
/// `WHOLE_SHARE` of the channel.
pub fn zones(footprints: &Footprints, on_air: &[OnAir], numbers: &[String], size: (u32, u32), follows: bool) -> Option<Vec<Area>> {
    let suffix = format!("@{}x{}", size.0, size.1);
    let mut areas: Vec<Area> = Vec::new();
    for page in on_air {
        areas.extend_from_slice(footprints.get(&page.page, size)?);
    }
    for number in numbers {
        let rule = format!(":{number}{suffix}");
        let mut found = false;
        for (_, list) in footprints.pages.iter().filter(|(key, _)| key.ends_with(&rule)) {
            areas.extend_from_slice(list);
            found = true;
        }
        if !found {
            return None;
        }
    }
    if !follows {
        areas.extend(footprints.pages.iter().filter(|(key, _)| key.ends_with(&suffix)).flat_map(|(_, list)| list.iter().copied()));
    }
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

/// Why a run with `zones` must widen in the middle of its piece (PLAN-strefy
/// S3b), if it must: a page on air with no footprint yet (taken by the operator
/// or a rule; its first frames may not be drawn yet), a page whose footprint
/// lies outside the zones, or pixels drawn outside them.
pub fn widen_reason(
    footprints: &Footprints,
    zones: &[Area],
    on_air: &[OnAir],
    rects: &[(u32, u32, u32, u32)],
    size: (u32, u32),
) -> Option<String> {
    for page in on_air {
        match footprints.get(&page.page, size) {
            None => return Some(format!("page {} with no footprint", page.page)),
            Some(areas) if !areas.iter().all(|a| zones.iter().any(|z| contains(*z, *a))) => {
                return Some(format!("page {} outside the zones", page.page));
            }
            Some(_) => {}
        }
    }
    (!covered(zones, rects)).then(|| "graphics outside the zones".to_owned())
}

fn seen_start(seen: &HashMap<String, Seen>, channel: &str) -> u64 {
    seen.get(channel).map_or(u64::MAX, |s| s.start)
}

/// The bounding boxes (x, y, w, h) of the 4-connected groups of `cells` (indices).
fn components(cells: &Cells, list: &[usize]) -> Vec<(u32, u32, u32, u32)> {
    let mut mine = vec![false; cells.on.len()];
    for &i in list {
        mine[i] = true;
    }
    let mut out = Vec::new();
    for &start in list {
        if !mine[start] {
            continue;
        }
        mine[start] = false;
        let (mut c0, mut r0, mut c1, mut r1) = (usize::MAX, usize::MAX, 0, 0);
        let mut stack = vec![start];
        while let Some(i) = stack.pop() {
            let (c, r) = (i % cells.cols, i / cells.cols);
            (c0, r0, c1, r1) = (c0.min(c), r0.min(r), c1.max(c), r1.max(r));
            let mut visit = |j: usize| {
                if mine[j] {
                    mine[j] = false;
                    stack.push(j);
                }
            };
            if c > 0 {
                visit(i - 1);
            }
            if c + 1 < cells.cols {
                visit(i + 1);
            }
            if r > 0 {
                visit(i - cells.cols);
            }
            if r + 1 < cells.rows {
                visit(i + cells.cols);
            }
        }
        let (a, b) = (cells.area(r0 * cells.cols + c0), cells.area(r1 * cells.cols + c1));
        out.push((a.x, a.y, b.x + b.w - a.x, b.y + b.h - a.y));
    }
    out
}

/// A rectangle snapped out to the grid, with the margin, even and inside the channel.
fn snap((x, y, w, h): (u32, u32, u32, u32), (cw, ch): (u32, u32)) -> Area {
    let down = |v: u32| v.saturating_sub(MARGIN) / GRID * GRID;
    let up = |v: u32, max: u32| (v.saturating_add(MARGIN).div_ceil(GRID) * GRID).min(max);
    let (x0, y0) = (down(x), down(y));
    let (x1, y1) = (up(x.saturating_add(w), cw), up(y.saturating_add(h), ch));
    Area { x: x0 & !1, y: y0 & !1, w: (x1 - x0.min(x1)).max(2) & !1, h: (y1 - y0.min(y1)).max(2) & !1 }
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

    /// One picture of channel "1" at `now` (ms): boxes drawn whole.
    fn learn(f: &mut Footprints, pages: &[OnAir], boxes: &[(u32, u32, u32, u32)], now: u64) -> bool {
        f.learn("1", pages, &Cells::of_boxes(boxes, HD), now)
    }

    /// A page alone on a channel of its own: its footprint.
    fn alone(f: &mut Footprints, key: &str, boxes: &[(u32, u32, u32, u32)]) -> bool {
        f.learn(key, &[page(key, 1)], &Cells::of_boxes(boxes, HD), 10)
    }

    #[test]
    fn cells_of_a_picture_are_those_with_alpha() {
        let pixels = |w: u32, h: u32, on: &[(u32, u32)]| {
            let mut p = vec![0u8; (w * h * 4) as usize];
            for &(x, y) in on {
                p[((y * w + x) * 4 + 3) as usize] = 9;
            }
            p
        };
        // a rectangle over three cells across, one pixel drawn in the first and the last
        let rect = crate::packets::Rect { x: 10, y: 20, w: 40, h: 4, pixels: pixels(40, 4, &[(0, 0), (39, 3)]).into() };
        let cells = Cells::of_picture(&[rect], HD);
        assert_eq!(cells.boxes(), [(0, 16, 16, 16), (48, 16, 16, 16)]);
        assert_eq!(Cells::of_boxes(&[(10, 20, 40, 4)], HD).boxes(), [(0, 16, 64, 16)]);
        // a channel not a multiple of the grid: the last cells are cut
        assert_eq!(Cells::of_boxes(&[(1270, 710, 10, 10)], (1280, 720)).boxes(), [(1264, 704, 16, 16)]);
    }

    #[test]
    fn one_page_learns_snapped_rectangles_and_only_grows() {
        let mut f = Footprints::default();
        assert!(learn(&mut f, &[page("p9:4001", 1)], &[(100, 800, 600, 120)], 10));
        assert_eq!(f.get("p9:4001", HD).unwrap(), [Area { x: 80, y: 784, w: 640, h: 160 }]);
        // inside: nothing new
        assert!(!learn(&mut f, &[page("p9:4001", 1)], &[(120, 820, 100, 50)], 20));
        // a later phase of the animation further right: the footprint grows (merged, gap < 64)
        assert!(learn(&mut f, &[page("p9:4001", 1)], &[(700, 800, 300, 120)], 30));
        assert_eq!(f.get("p9:4001", HD).unwrap(), [Area { x: 80, y: 784, w: 944, h: 160 }]);
        // other channel size: its own footprint
        assert!(f.get("p9:4001", (1280, 720)).is_none());
    }

    #[test]
    fn far_apart_rectangles_stay_separate_up_to_the_limit() {
        let mut f = Footprints::default();
        alone(&mut f, "a:1", &[(0, 950, 1920, 100), (1700, 40, 160, 160)]);
        assert_eq!(f.get("a:1", HD).unwrap().len(), 2);
        let many: Vec<_> = (0..6).map(|i| (i * 300, 0, 50, 50)).collect();
        alone(&mut f, "b:2", &many);
        let zones = f.get("b:2", HD).unwrap();
        assert!(zones.len() <= MAX_ZONES, "at most MAX_ZONES: {zones:?}");
        assert!(many.iter().all(|&(x, y, w, h)| zones.iter().any(|z| contains(*z, Area { x, y, w, h }))), "all pixels covered");
    }

    #[test]
    fn with_several_pages_new_cells_go_to_the_page_taken_just_before() {
        let (logo, title, ticker) = ((1700, 40, 160, 160), (100, 850, 800, 120), (0, 1000, 1920, 64));
        let mut f = Footprints::default();
        learn(&mut f, &[page("logo:1", 1_000)], &[logo], 1_000);
        // the title taken at 60 s, drawn half a second later: the logo's cells stay the logo's
        let both = [page("logo:1", 1_000), page("title:2", 60_000)];
        assert!(learn(&mut f, &both, &[logo, title], 60_500));
        assert_eq!(f.get("logo:1", HD).unwrap(), [Area { x: 1680, y: 16, w: 208, h: 208 }]);
        assert_eq!(f.get("title:2", HD).unwrap(), [Area { x: 80, y: 832, w: 848, h: 160 }]);
        // cells coming up long after every take (a crawl's letters): nobody's
        let mut g = Footprints::default();
        assert!(!learn(&mut g, &both, &[logo, title], 200_000));
        // ...unless only one page on air has no footprint: cells drawn for long are its
        // (the title off: nobody learns while its out animation may be drawn)
        assert!(!learn(&mut g, &[page("logo:1", 1_000)], &[logo, title], 300_000));
        assert!(learn(&mut g, &[page("logo:1", 1_000)], &[logo], 304_000));
        learn(&mut g, &both, &[logo], 399_500);
        assert!(!learn(&mut g, &both, &[logo, title], 400_000), "just came up: not yet");
        assert!(learn(&mut g, &both, &[logo, title], 416_000));
        assert_eq!(g.get("title:2", HD), f.get("title:2", HD));
        // a cell drawn just before a take (a crawl under a new title) is not the new page's
        let mut h = Footprints::default();
        let crawl = [page("crawl:5", 1_000), page("title:2", 60_000)];
        learn(&mut h, &crawl[..1], &[ticker], 59_500);
        learn(&mut h, &crawl, &[], 60_000);
        assert!(learn(&mut h, &crawl, &[ticker, title], 60_500));
        assert_eq!(h.get("crawl:5", HD).unwrap(), [Area { x: 0, y: 976, w: 1920, h: 104 }], "alone on air at first");
        assert_eq!(h.get("title:2", HD).unwrap(), [Area { x: 80, y: 832, w: 848, h: 160 }]);
        // the same with the crawl unknown (two pages on air): its cells, drawn
        // just before the take, are still not the title's
        let mut k = Footprints::default();
        let three = [page("crawl:5", 1_000), page("qr:6", 500), page("title:2", 60_000)];
        assert!(!learn(&mut k, &three[..2], &[ticker], 59_500));
        learn(&mut k, &three, &[], 60_000);
        assert!(learn(&mut k, &three, &[ticker, title], 60_500));
        assert_eq!(k.get("title:2", HD).unwrap(), [Area { x: 80, y: 832, w: 848, h: 160 }]);
        assert!(k.get("crawl:5", HD).is_none() && k.get("qr:6", HD).is_none());
        // a page whose animation goes on after another page is taken (a QR box
        // unfolding, a countdown's digits): cells next to its footprint stay its own
        let mut m = Footprints::default();
        let qr = (40, 500, 200, 100);
        alone(&mut m, "logo:1", &[logo]);
        learn(&mut m, &[page("qr:6", 1_000)], &[qr], 1_500);
        let known = [page("qr:6", 1_000), page("logo:1", 2_000)];
        learn(&mut m, &known, &[qr, logo], 59_000);
        assert!(learn(&mut m, &known, &[qr, logo, (40, 600, 200, 60)], 65_000), "next to the QR, long after the takes");
        assert_eq!(m.get("qr:6", HD).unwrap(), [Area { x: 16, y: 480, w: 240, h: 208 }]);
        assert_eq!(m.get("logo:1", HD).unwrap().len(), 1);
        // next to a known page, with a page of no footprint on air: nobody's (it may be that one's)
        let unknown_too = [page("qr:6", 1_000), page("logo:1", 2_000), page("x:7", 500)];
        learn(&mut m, &unknown_too, &[qr, logo, (40, 600, 200, 60)], 66_000);
        assert!(!learn(&mut m, &unknown_too, &[qr, logo, (40, 600, 200, 60), (40, 700, 200, 20)], 70_000));
        // ...but within FRESH of a take a new cell is the new page's, even next to another one
        let later = [page("qr:6", 1_000), page("logo:1", 2_000), page("title:2", 80_000)];
        learn(&mut m, &known, &[qr, logo], 76_000);
        assert!(learn(&mut m, &later, &[qr, logo, (260, 500, 200, 100)], 81_000));
        assert_eq!(m.get("title:2", HD).unwrap(), [Area { x: 240, y: 480, w: 240, h: 144 }]);
        // a wipe whose bar lies over the title's bar, taken 42 ms after the
        // title: one moment, nobody learns; after RECENT it draws nothing of its own
        let mut w = Footprints::default();
        let bar = (0, 980, 1920, 96);
        learn(&mut w, &[page("p9:4001", 1_000)], &[title, bar], 1_000);
        let both = [page("p9:4001", 60_000), page("wipe:3003", 60_042)];
        learn(&mut w, &both[..1], &[], 59_000);
        assert!(!learn(&mut w, &both, &[title, bar], 60_500));
        assert!(w.get("wipe:3003", HD).is_none());
        learn(&mut w, &both, &[title, bar], 71_000);
        assert!(learn(&mut w, &both, &[title, bar], 75_500));
        assert_eq!(w.get("wipe:3003", HD), Some(&[][..]));
        // ...taken alone, its bar over the title's footprint is its own
        let mut v = Footprints::default();
        learn(&mut v, &[page("p9:4001", 1_000)], &[title, bar], 1_000);
        learn(&mut v, &[page("p9:4001", 1_000)], &[title], 59_500);
        let later = [page("p9:4001", 1_000), page("wipe:3003", 60_000)];
        assert!(learn(&mut v, &later, &[title, bar], 60_500));
        assert_eq!(v.get("wipe:3003", HD).unwrap(), [Area { x: 0, y: 960, w: 1920, h: 120 }]);
        // pages on air since before the sidecar watched: no empty footprint
        let mut u = Footprints::default();
        learn(&mut u, &[page("p9:4001", 1_000)], &[title], 500_000);
        assert!(!learn(&mut u, &[page("p9:4001", 1_000), page("x:1", 2_000)], &[title], 600_000));
        assert!(u.get("x:1", HD).is_none());
        // the first picture after a pause: cells that came up unseen are not a fresh take's
        let mut r = Footprints::default();
        learn(&mut r, &[page("logo:1", 1_000)], &[logo], 1_000);
        let paused = [page("logo:1", 1_000), page("title:2", 60_000), page("p7:3", 99_000)];
        assert!(!learn(&mut r, &paused, &[logo, title], 100_000));
        assert!(r.get("p7:3", HD).is_none() && r.get("title:2", HD).is_none());
        // two pages taken at the same moment: nobody learns
        assert!(!learn(&mut h, &[page("c:3", 90_000), page("d:4", 90_000)], &[(800, 400, 100, 100)], 90_500));
    }

    #[test]
    fn zones_of_a_run_from_pages_on_air_and_rule_pages() {
        let mut f = Footprints::default();
        alone(&mut f, "logo:3001", &[(1700, 40, 160, 160)]);
        alone(&mut f, "p9:4001", &[(100, 850, 800, 120)]);
        alone(&mut f, "qr:1401", &[(40, 300, 200, 200)]);
        // logo on air, the title will be taken by on_clip (TAKE 4001)
        let z = zones(&f, &[page("logo:3001", 1)], &["4001".into()], HD, true).unwrap();
        assert_eq!(z.len(), 2);
        // without following the zones: all the known pages (the operator may take them)
        assert_eq!(zones(&f, &[page("logo:3001", 1)], &["4001".into()], HD, false).unwrap().len(), 3);
        assert_eq!(zones(&f, &[], &[], HD, false).unwrap().len(), 3);
        // following, nothing on air and no rules: the whole channel
        assert_eq!(zones(&f, &[], &[], HD, true), None);
        assert!(covered(&z, &[(1700, 40, 160, 160), (100, 850, 800, 120)]));
        assert!(!covered(&z, &[(900, 400, 50, 50)]));
        // a page with no footprint: the whole channel
        assert_eq!(zones(&f, &[page("x:9", 1)], &[], HD, true), None);
        assert_eq!(zones(&f, &[], &["5005".into()], HD, true), None);
        // nothing known for another size: the whole channel
        assert_eq!(zones(&f, &[], &[], (1280, 720), false), None);
        // a full-screen page seen once (a wipe): the whole channel only while it is on air when following
        alone(&mut f, "full:7", &[(0, 0, 1920, 1080)]);
        assert_eq!(zones(&f, &[page("full:7", 1)], &[], HD, true), None);
        assert_eq!(zones(&f, &[page("logo:3001", 1)], &[], HD, false), None);
        assert_eq!(zones(&f, &[page("logo:3001", 1)], &[], HD, true).unwrap().len(), 1);
    }

    #[test]
    fn a_run_widens_for_unknown_pages_footprints_and_pixels_outside_its_zones() {
        let mut f = Footprints::default();
        alone(&mut f, "logo:3001", &[(1700, 40, 160, 160)]);
        let z = f.get("logo:3001", HD).unwrap().to_vec();
        let logo = [(1700, 40, 160, 160)];
        assert_eq!(widen_reason(&f, &z, &[page("logo:3001", 1)], &logo, HD), None);
        // the operator takes a page never seen: widen before it draws
        assert_eq!(widen_reason(&f, &z, &[page("x:9", 2)], &[], HD).as_deref(), Some("page x:9 with no footprint"));
        // a known page elsewhere on the screen, not in these zones
        alone(&mut f, "p9:4001", &[(100, 850, 800, 120)]);
        assert_eq!(widen_reason(&f, &z, &[page("p9:4001", 2)], &[], HD).as_deref(), Some("page p9:4001 outside the zones"));
        // pixels outside (an animation beyond the footprint)
        assert_eq!(widen_reason(&f, &z, &[page("logo:3001", 1)], &[(900, 400, 50, 50)], HD).as_deref(), Some("graphics outside the zones"));
    }

    #[test]
    fn saved_and_loaded() {
        let dir = std::env::temp_dir().join(format!("vizrs-footprints-{}", std::process::id()));
        let path = dir.join("vizrs-live").join("footprints.json");
        // a file of the first version (footprints from visible rectangles): not read
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"pages":{"p9:4001@1920x1080":["1712x560+16+320"]}}"#).unwrap();
        let mut f = Footprints::load(Some(path.clone()));
        assert!(f.get("p9:4001", HD).is_none());
        learn(&mut f, &[page("p9:4001", 1)], &[(100, 800, 600, 120)], 10);
        f.pages.insert(key("wipe:3003", HD), Vec::new());
        f.dirty = true;
        f.save();
        let back = Footprints::load(Some(path.clone()));
        assert_eq!(back.get("p9:4001", HD), f.get("p9:4001", HD));
        assert_eq!(back.get("wipe:3003", HD), Some(&[][..]), "an empty footprint stays one");
        std::fs::write(&path, "not json").unwrap();
        assert!(Footprints::load(Some(path)).get("p9:4001", HD).is_none(), "broken file: no footprints");
        let _ = std::fs::remove_dir_all(dir);
        assert_eq!(Footprints::default_path(""), None);
    }
}
