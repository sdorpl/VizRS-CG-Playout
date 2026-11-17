//! Graphics zones (`area: auto`) and watching them change during a piece.

use super::*;

/// What the sidecar gives the overlay (`GET /ffmpeg`), with the rate decided.
pub struct Frames {
    pub width: u32,
    pub height: u32,
    pub rate: String,
    /// ffmpeg's name of the colour matrix of the frames; empty = not declared.
    pub matrix: String,
    /// Graphics areas of the channel and their rows in the frames (stacked);
    /// empty = the frames are the whole channel.
    pub areas: Vec<(Area, u32)>,
    /// Query of `/ffmpeg/resume` after a restart of the sidecar; empty = an older sidecar.
    pub resume: String,
    /// The sidecar's session taken over.
    pub session: String,
    /// Seconds of graphics the sidecar had ready at once when the session was
    /// taken over, and when (`ahead`); None = an older sidecar.
    pub ahead: Option<(std::time::Instant, f64)>,
    /// Revision of zones that may widen in the middle of the piece (`area:
    /// auto`, PLAN-strefy S3b); None = fixed for the piece (an older sidecar too).
    pub zones_rev: Option<u64>,
}

impl Frames {
    /// Seconds of graphics ready at once for a run from `elapsed` s of the
    /// piece: those ready at the takeover, plus the time since, minus the
    /// piece before the run. None = unknown (an older sidecar).
    pub(crate) fn burst(&self, elapsed: f64) -> Option<f64> {
        self.ahead.map(|(at, seconds)| seconds + at.elapsed().as_secs_f64() - elapsed)
    }
}

/// The parts of a frame layout in a reply of the sidecar (`areas`, stacked).
pub(crate) fn stacked(info: &Value) -> io::Result<Vec<(Area, u32)>> {
    let number =
        |value: &Value, key: &str| value.get(key).and_then(Value::as_f64).map(|n| n as u32).ok_or_else(|| error(format!("'areas.{key}'")));
    let mut areas = Vec::new();
    let mut row = 0;
    for area in info.get("areas").and_then(Value::as_array).unwrap_or(&[]) {
        let area = Area { x: number(area, "x")?, y: number(area, "y")?, w: number(area, "w")?, h: number(area, "h")? };
        areas.push((area, row));
        row += area.h;
    }
    Ok(areas)
}

/// The zones of a session now (`GET /ffmpeg/zones`).
#[derive(Clone, Debug, PartialEq)]
pub struct Zones {
    pub rev: u64,
    /// `WxH+X+Y,…`; empty = the whole channel.
    pub geometry: String,
    pub width: u32,
    pub height: u32,
    pub areas: Vec<(Area, u32)>,
    pub resume: String,
}

/// A reply of `/ffmpeg/zones`.
pub fn parse_zones(body: &str) -> io::Result<Zones> {
    let info = json::parse(body).map_err(|e| error(e.to_string()))?;
    let size = |key| info.get(key).and_then(Value::as_f64).map(|n| n as u32).ok_or_else(|| error(format!("'{key}'")));
    let areas = stacked(&info)?;
    if areas.is_empty() {
        return Err(error("'areas'".into()));
    }
    Ok(Zones {
        rev: info.get("rev").and_then(Value::as_f64).map(|n| n as u64).ok_or_else(|| error("'rev'".into()))?,
        geometry: info.get("zones").and_then(Value::as_str).unwrap_or_default().to_owned(),
        width: size("width")?,
        height: size("height")?,
        areas,
        resume: info.get("resume").and_then(Value::as_str).unwrap_or_default().to_owned(),
    })
}

/// The zones of the session, asked every `ZONES_POLL` in the background: the
/// newest revision waits for the next run.
pub(crate) struct ZoneWatch {
    latest: Arc<Mutex<Option<Zones>>>,
    used: AtomicU64,
    stop: Arc<AtomicBool>,
}

impl ZoneWatch {
    pub(crate) fn start(sidecar: &str, session: &str, rev: u64) -> ZoneWatch {
        let watch = ZoneWatch { latest: Arc::default(), used: AtomicU64::new(rev), stop: Arc::default() };
        let (latest, stop) = (Arc::clone(&watch.latest), Arc::clone(&watch.stop));
        let (sidecar, path) = (sidecar.to_owned(), format!("/ffmpeg/zones?session={}", quote_plus(session)));
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(ZONES_POLL);
                match sidecar_get(&sidecar, &path) {
                    Ok((status, body)) if status == "200" => {
                        if let Ok(zones) = parse_zones(&String::from_utf8_lossy(&body)) {
                            let mut latest = latest.lock().unwrap_or_else(|e| e.into_inner());
                            if latest.as_ref().is_none_or(|l| zones.rev > l.rev) {
                                *latest = Some(zones);
                            }
                        }
                    }
                    // The session is over (a restarted sidecar too): the zones stay.
                    Ok(_) => return,
                    Err(_) => {}
                }
            }
        });
        watch
    }

    /// Zones newer than those of the runs so far.
    pub(crate) fn newer(&self) -> bool {
        let used = self.used.load(Ordering::Relaxed);
        self.latest.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|z| z.rev > used)
    }

    /// The newer zones, now those of the runs.
    pub(crate) fn take(&self) -> Option<Zones> {
        let latest = self.latest.lock().unwrap_or_else(|e| e.into_inner()).clone()?;
        (latest.rev > self.used.swap(latest.rev, Ordering::Relaxed)).then_some(latest)
    }
}

impl Drop for ZoneWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
