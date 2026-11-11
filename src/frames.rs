// Pictures of the graphics for the sessions: the current one and a history,
// so each output frame can take the picture of its own moment (`setFrame` /
// `frameAt` of the Node.js sidecar). A picture is dated when Chromium drew it,
// not when its decoding finished. The history covers the lead of the streams
// (seconds of pictures), so a picture stays a PNG until a session shows it. A
// picture of several graphics areas is one PNG per area; an area with the
// crawl tape drawn by the sidecar holds its pixels instead (`tape`).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use crate::packets::{Rect, picture_packet};

/// Turns shot number `index` of a picture (its graphics area) into visible
/// rectangles at the channel size, in channel coordinates.
pub type Decode = dyn Fn(usize, &Shot) -> Result<Vec<Rect>, String> + Send + Sync;

/// The pixels of a graphics area, made when a session shows the picture.
pub type Pixels = dyn Fn() -> Result<crate::png::Picture, String> + Send + Sync;

/// One graphics area of a picture: Chromium's PNG, or pixels the sidecar
/// composes (the crawl tape over the page, `tape`).
#[derive(Clone)]
pub enum Shot {
    Png(Arc<[u8]>),
    Pixels(Arc<Pixels>),
}

impl Shot {
    pub fn picture(&self) -> Result<crate::png::Picture, String> {
        match self {
            Shot::Png(data) => crate::png::decode_png(data),
            Shot::Pixels(make) => make(),
        }
    }
}

/// A decoded picture; built once, shared by the sessions of the same size.
/// Its yuva420p frames are built by each stream (`yuva::Builder`), from the
/// frame before.
#[derive(Debug)]
pub struct Picture {
    pub rects: Vec<Rect>,
    packet: OnceLock<Vec<u8>>,
}

impl Picture {
    pub fn new(rects: Vec<Rect>) -> Picture {
        Picture { rects, packet: OnceLock::new() }
    }

    /// ETV packet: the visible rectangles, or Clear.
    pub fn packet(&self) -> &[u8] {
        self.packet.get_or_init(|| picture_packet(&self.rects))
    }
}

enum Content {
    Decoded(Arc<Picture>),
    Png { data: Vec<Shot>, decode: Arc<Decode>, decoded: Mutex<Weak<Picture>> },
}

/// One picture; `seq` grows with every new picture.
pub struct Frame {
    pub seq: u64,
    /// When Chromium drew it.
    pub at: Instant,
    content: Content,
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frame").field("seq", &self.seq).field("at", &self.at).finish_non_exhaustive()
    }
}

impl Frame {
    pub fn new(seq: u64, rects: Vec<Rect>, at: Instant) -> Frame {
        Frame { seq, at, content: Content::Decoded(Arc::new(Picture::new(rects))) }
    }

    /// The decoded picture. A PNG is decoded again only after every session
    /// let go of it, so the history holds PNGs, not pixels.
    pub fn picture(&self) -> Arc<Picture> {
        match &self.content {
            Content::Decoded(picture) => Arc::clone(picture),
            Content::Png { data, decode, decoded } => {
                let mut decoded = decoded.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(picture) = decoded.upgrade() {
                    return picture;
                }
                let mut rects = Vec::new();
                for (index, png) in data.iter().enumerate() {
                    match decode(index, png) {
                        Ok(more) => rects.extend(more),
                        Err(error) => crate::warn!("burn-in: {error}"),
                    }
                }
                let picture = Arc::new(Picture::new(rects));
                *decoded = Arc::downgrade(&picture);
                picture
            }
        }
    }

    fn is_empty(&self) -> bool {
        matches!(&self.content, Content::Decoded(picture) if picture.rects.is_empty())
    }
}

/// Pictures for the sessions: the one shown at a moment.
pub trait Source: Send + Sync {
    fn frame_at(&self, time: Instant) -> Arc<Frame>;
}

/// All graphics off the air (`POST /api/live/graphics/off`): every stream sends
/// transparent frames until it is switched back on.
static GRAPHICS_OFF: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn graphics_off() -> bool {
    GRAPHICS_OFF.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_graphics_off(off: bool) {
    GRAPHICS_OFF.store(off, std::sync::atomic::Ordering::Relaxed);
}

/// The transparent frame every stream sends while the graphics are off.
pub fn blank() -> Arc<Frame> {
    static BLANK: OnceLock<Arc<Frame>> = OnceLock::new();
    Arc::clone(BLANK.get_or_init(|| Arc::new(Frame::new(0, Vec::new(), Instant::now()))))
}

/// Transparent frames: burn-in off, or the channel size unknown.
pub struct Empty(Arc<Frame>);

impl Empty {
    pub fn new() -> Empty {
        Empty(Arc::new(Frame::new(0, Vec::new(), Instant::now())))
    }
}

impl Default for Empty {
    fn default() -> Self {
        Empty::new()
    }
}

impl Source for Empty {
    fn frame_at(&self, _: Instant) -> Arc<Frame> {
        Arc::clone(&self.0)
    }
}

/// The current picture and the ones before it, for `keep`.
pub struct History {
    keep: Duration,
    frames: Mutex<VecDeque<Arc<Frame>>>,
}

impl History {
    pub fn new(keep: Duration) -> History {
        // Nothing before: pictures of the window may be dated before the start.
        let now = Instant::now();
        let empty = Frame::new(0, Vec::new(), now.checked_sub(keep).unwrap_or(now));
        History { keep, frames: Mutex::new(VecDeque::from([Arc::new(empty)])) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<Arc<Frame>>> {
        self.frames.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn current(&self) -> Arc<Frame> {
        Arc::clone(self.lock().back().expect("history is never empty"))
    }

    /// A new decoded picture drawn at `at`. Two empty pictures in a row are one.
    /// A picture equal to the current one shares its pixels (and the packet
    /// and yuva420p frame made from them): a standing graphic drawn 50 times
    /// a second would otherwise keep seconds of identical copies.
    pub fn set(&self, rects: Vec<Rect>, at: Instant) {
        let current = self.current();
        if rects.is_empty() && current.is_empty() {
            return;
        }
        match &current.content {
            Content::Decoded(picture) if picture.rects == rects => {
                let picture = Arc::clone(picture);
                self.push(at, |seq, at| Frame { seq, at, content: Content::Decoded(picture) });
            }
            _ => self.push(at, |seq, at| Frame::new(seq, rects, at)),
        }
    }

    /// A new PNG drawn at `at`, decoded once a session shows it.
    pub fn set_png(&self, data: Vec<u8>, at: Instant, decode: &Arc<Decode>) {
        self.set_pngs(vec![data.into()], at, decode);
    }

    /// A new picture of one PNG per graphics area, drawn at `at`.
    pub fn set_pngs(&self, data: Vec<Arc<[u8]>>, at: Instant, decode: &Arc<Decode>) {
        self.set_shots(data.into_iter().map(Shot::Png).collect(), at, decode);
    }

    /// A new picture of one shot per graphics area, drawn at `at`.
    pub fn set_shots(&self, data: Vec<Shot>, at: Instant, decode: &Arc<Decode>) {
        let decode = Arc::clone(decode);
        self.push(at, |seq, at| Frame { seq, at, content: Content::Png { data, decode, decoded: Mutex::new(Weak::new()) } });
    }

    /// Keeps the history in order and no longer than `keep`.
    fn push(&self, at: Instant, frame: impl FnOnce(u64, Instant) -> Frame) {
        let mut frames = self.lock();
        let current = frames.back().expect("history is never empty");
        let at = at.max(current.at);
        let frame = Arc::new(frame(current.seq + 1, at));
        frames.push_back(frame);
        // Keep the frame that was current when the window starts.
        while frames.len() > 1 && at.checked_sub(self.keep).is_some_and(|start| frames[1].at <= start) {
            frames.pop_front();
        }
    }
}

impl Source for History {
    /// The picture shown at `time`, like a playout taking the current frame.
    fn frame_at(&self, time: Instant) -> Arc<Frame> {
        let frames = self.lock();
        let found = frames.iter().skip(1).rev().find(|f| f.at <= time).unwrap_or(&frames[0]);
        Arc::clone(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(value: u8) -> Vec<Rect> {
        vec![Rect { x: 0, y: 0, w: 1, h: 1, pixels: vec![value; 4].into() }]
    }

    #[test]
    fn history_gives_the_picture_of_a_past_moment() {
        let history = History::new(Duration::from_millis(500));
        let t0 = Instant::now();
        history.set(pixel(1), t0 + Duration::from_millis(10));
        history.set(pixel(2), t0 + Duration::from_millis(50));
        assert_eq!(history.frame_at(t0).seq, 0);
        assert_eq!(history.frame_at(t0 + Duration::from_millis(30)).seq, 1);
        assert_eq!(history.frame_at(t0 + Duration::from_millis(60)).seq, 2);

        // Dated before the current picture: kept in order, not before it.
        history.set(pixel(3), t0);
        assert_eq!(history.current().at, t0 + Duration::from_millis(50));
        assert_eq!(history.frame_at(t0 + Duration::from_millis(60)).seq, 3);

        // Empty after empty is no new picture; old pictures leave the window.
        history.set(Vec::new(), t0 + Duration::from_secs(2));
        history.set(Vec::new(), t0 + Duration::from_secs(3));
        assert_eq!(history.current().seq, 4);
        assert_eq!(history.lock().len(), 2);
    }

    #[test]
    fn a_standing_picture_is_shared() {
        let history = History::new(Duration::from_secs(5));
        let t0 = Instant::now();
        history.set(pixel(1), t0);
        let first = history.current().picture();
        for i in 1..100 {
            history.set(pixel(1), t0 + Duration::from_millis(20 * i));
        }
        // Every moment has its own frame, all with the same pixels.
        assert_eq!(history.current().seq, 100);
        assert!(Arc::ptr_eq(&first, &history.frame_at(t0 + Duration::from_millis(1000)).picture()));
        history.set(pixel(2), t0 + Duration::from_secs(3));
        assert!(!Arc::ptr_eq(&first, &history.current().picture()));
        assert_eq!(history.current().picture().rects, pixel(2));
    }

    #[test]
    fn a_png_is_decoded_when_shown_and_dropped_after() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let decodes = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&decodes);
        let decode: Arc<Decode> = Arc::new(move |_, shot: &Shot| {
            counter.fetch_add(1, Ordering::Relaxed);
            let Shot::Png(data) = shot else { return Err("not a PNG".into()) };
            Ok(pixel(data[0]))
        });
        let history = History::new(Duration::from_secs(5));
        let t0 = Instant::now();
        history.set_png(vec![7], t0, &decode);
        assert_eq!(decodes.load(Ordering::Relaxed), 0);

        let frame = history.frame_at(t0);
        let shown = frame.picture();
        assert_eq!(shown.rects[0].pixels[0], 7);
        // A second session shares the picture while the first still shows it.
        assert!(Arc::ptr_eq(&shown, &frame.picture()));
        assert_eq!(decodes.load(Ordering::Relaxed), 1);
        drop(shown);
        frame.picture();
        assert_eq!(decodes.load(Ordering::Relaxed), 2);
    }
}
