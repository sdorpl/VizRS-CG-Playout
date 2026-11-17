//! The crawl tape (`tape`): the sidecar draws the scrolling strip itself, the page only gives its film.

use super::*;

/// The crawl whose tape the sidecar draws (`tape`).
pub(super) struct Track {
    pub(super) id: u64,
    pub(super) key: String,
    /// Its area (index of the regions) and window in that area's pixels.
    pub(super) part: usize,
    pub(super) window: Area,
    pub(super) reel: Reel,
}

/// The tape over the frames of one page.
#[derive(Default)]
pub(super) struct TapeRun {
    pub(super) track: Option<Track>,
    /// Crawls (area, id) the sidecar gave back to the page after a failure.
    pub(super) refused: Vec<(usize, u64)>,
    /// Frame time of the next look for a crawl when none is drawn.
    pub(super) next_look: f64,
    /// The page picture under the tape: its PNG and its pixels.
    pub(super) base: Option<(Arc<[u8]>, Arc<crate::png::Picture>)>,
    /// The last composite pushed: its offset, page picture and newest film.
    pub(super) shown: Option<(i64, usize, usize)>,
}

impl Capture {
    /// The tape of one slot (after the page drew it): looks for a standing
    /// crawl, asks the page to leave its tape to the sidecar, films what the
    /// window needs and blends the tape into the shot of its area. True when
    /// that gives a new picture. Any trouble gives the crawl back to the page.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn tape_slot(
        &self,
        devtools: &DevTools,
        sessions: &[String],
        run: &mut TapeRun,
        parts: &[(Area, Area)],
        shots: &mut [Shot],
        start: f64,
        film: &dyn Fn(f64, (usize, Area), i64, u64) -> Result<Film, String>,
    ) -> bool {
        if run.track.is_none() && start < run.next_look {
            return false;
        }
        run.next_look = start + CHECK_MS;
        // Each area has its tab (the whole page in each): a crawl counts in the tab of the area it is in.
        let evaluate = |part: usize, expression: String| {
            let params = json::object([("expression", expression.into()), ("returnByValue", true.into())]);
            devtools.call("Runtime.evaluate", params, Some(&sessions[part]), CALL_TIMEOUT)
        };
        let give_back = |part: usize, id: u64| {
            let _ = evaluate(part, format!("window.vizrsTape && vizrsTape.external({id}, false)"));
        };
        // While a crawl is drawn only its tab is asked, else every tab.
        let looked: Vec<usize> = match &run.track {
            Some(t) => vec![t.part],
            None => (0..parts.len()).collect(),
        };
        let mut crawls: Vec<(Crawl, usize, Area)> = Vec::new();
        for part in looked {
            match evaluate(part, "window.vizrsTape ? vizrsTape.state() : null".into()) {
                Ok(result) => {
                    let state = parse_state(result.get("result").and_then(|r| r.get("value")).unwrap_or(&Value::Null));
                    crawls.extend(state.into_iter().filter_map(|c| c.window_in(parts[part].0).map(|w| (c, part, w))));
                }
                Err(error) => crate::warn!("burn-in {}: tape: {error}", self.key()),
            }
        }
        // The tracked crawl while it is on air, else the widest one inside an area.
        let tracked = run.track.as_ref().map(|t| (t.part, t.id));
        let chosen = crawls
            .iter()
            .filter(|(c, part, _)| !run.refused.contains(&(*part, c.id)))
            .max_by(|(a, pa, _), (b, pb, _)| (Some((*pa, a.id)) == tracked).cmp(&(Some((*pb, b.id)) == tracked)).then(a.w.total_cmp(&b.w)));
        let Some(&(ref crawl, part, window)) = chosen else {
            if let Some(track) = run.track.take() {
                give_back(track.part, track.id);
            }
            self.tape_crawl.store(0, Ordering::Relaxed);
            return false;
        };
        match &mut run.track {
            Some(t) if (t.part, t.id) == (part, crawl.id) => {
                if t.key != crawl.key || t.window != window {
                    (t.key, t.window, t.reel) = (crawl.key.clone(), window, Reel::default());
                }
            }
            track => {
                if let Some(old) = track.take() {
                    give_back(old.part, old.id);
                }
                if evaluate(part, format!("vizrsTape.external({}, true)", crawl.id)).is_err() {
                    return false;
                }
                crate::info!("burn-in {}: tape of crawl {} drawn by the sidecar", self.key(), crawl.id);
                *track = Some(Track { id: crawl.id, key: crawl.key.clone(), part, window, reel: Reel::default() });
            }
        }
        if !crawl.hidden {
            self.tape_crawl.store(0, Ordering::Relaxed);
            return false;
        }
        let track = run.track.as_mut().expect("tracked above");
        let s = crawl.offset();
        let mut time = start + 0.004;
        for _ in 0..3 {
            let Some(at) = track.reel.next_film(window, s) else { break };
            match film(time, (part, window), at, crawl.id) {
                Ok(new) => {
                    track.reel.films.push(Arc::new(new));
                    self.tape_films.fetch_add(1, Ordering::Relaxed);
                }
                Err(error) => {
                    crate::warn!("burn-in {}: tape: {error}; the page draws the crawl", self.key());
                    break;
                }
            }
            time += 0.002;
        }
        let Shot::Png(png) = &shots[part] else { return false };
        if !track.reel.covers(window, s) {
            self.tape_crawl.store(0, Ordering::Relaxed);
            run.refused.push((part, crawl.id));
            give_back(part, crawl.id);
            run.track = None;
            return false;
        }
        if run.base.as_ref().is_none_or(|(data, _)| !Arc::ptr_eq(data, png)) {
            match decode_png(png) {
                Ok(picture) => run.base = Some((Arc::clone(png), Arc::new(picture))),
                Err(error) => {
                    crate::warn!("burn-in {}: tape: {error}", self.key());
                    return false;
                }
            }
        }
        self.tape_crawl.store(crawl.id, Ordering::Relaxed);
        let (data, base) = run.base.as_ref().expect("decoded above");
        let last = track.reel.films.last().map_or(0, |f| Arc::as_ptr(f) as usize);
        let shown = (s, Arc::as_ptr(data) as *const u8 as usize, last);
        let new = run.shown != Some(shown);
        run.shown = Some(shown);
        let composite = Composite { base: Arc::clone(base), window, films: track.reel.films.clone(), s };
        shots[part] = Shot::Pixels(Arc::new(move || Ok(composite.render())));
        new
    }

    /// One film of the tape of crawl `id` at offset `s`: the tab of the area
    /// `clip` draws only that tape in one frame (`(time, interval)`), then the
    /// page again; `window` is the crawl window in the area.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn film(
        &self,
        devtools: &DevTools,
        session: &str,
        clip: Area,
        (time, interval): (f64, f64),
        window: Area,
        s: i64,
        id: u64,
    ) -> Result<Film, String> {
        let call = |method: &str, params: Value| devtools.call(method, params, Some(session), CALL_TIMEOUT);
        let expression = format!("vizrsTape.film({id}, {s})");
        call("Runtime.evaluate", json::object([("expression", expression.into()), ("returnByValue", true.into())]))?;
        let png = json::object([("format", "png".into()), ("optimizeForSpeed", true.into())]);
        let begin = |time: f64, png: Option<Value>| {
            let mut params = json::object([("frameTimeTicks", time.into()), ("interval", interval.into())]);
            if let (Some(png), Value::Object(members)) = (png, &mut params) {
                members.push(("screenshot".into(), png));
            }
            call("HeadlessExperimental.beginFrame", params)
        };
        let shot = begin(time, Some(png));
        // The page back as it was; this frame also takes the damage of the film.
        begin(time + 0.001, None)?;
        let shot = shot?;
        let data = shot.get("screenshotData").and_then(Value::as_str).ok_or("no film")?;
        let png = self.png_of(data, (clip.w, clip.h)).ok_or("bad film")?;
        let picture = decode_png(&png)?;
        Film::cut(&picture, window, s).ok_or_else(|| "film outside the area".into())
    }
}
