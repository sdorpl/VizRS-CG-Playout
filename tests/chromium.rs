// Real headless Chromium on a local transparent page; skipped without a
// browser (VIZRS_BROWSER or PATH).

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use vizrs_cg::area::Area;
use vizrs_cg::capture::{Capture, CaptureMode, CaptureOptions, find_browser};
use vizrs_cg::frames::Source;

/// Serves `page` to every request; returns its URL.
fn serve(page: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut request = [0; 4096];
            let _ = stream.read(&mut request);
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
                page.len()
            );
        }
    });
    url
}

/// One browser at a time: cold starts of several at once on a small CI runner
/// take longer than the first DevTools call may wait.
fn serial() -> MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

fn capture(url: String, render: (u32, u32), out: (u32, u32), idle: Duration) -> Option<Arc<Capture>> {
    capture_in(CaptureMode::Screencast, url, render, out, idle)
}

fn capture_in(mode: CaptureMode, url: String, render: (u32, u32), out: (u32, u32), idle: Duration) -> Option<Arc<Capture>> {
    capture_area(mode, url, render, out, Vec::new(), idle)
}

fn capture_area(
    mode: CaptureMode,
    url: String,
    render: (u32, u32),
    out: (u32, u32),
    areas: Vec<Area>,
    idle: Duration,
) -> Option<Arc<Capture>> {
    capture_with(mode, url, render, out, areas, idle, false)
}

fn capture_with(
    mode: CaptureMode,
    url: String,
    render: (u32, u32),
    out: (u32, u32),
    areas: Vec<Area>,
    idle: Duration,
    tape: bool,
) -> Option<Arc<Capture>> {
    let Some(browser) = find_browser() else {
        eprintln!("no Chromium: skipped");
        return None;
    };
    // Without the sandbox where the host forbids it (CI runners, containers).
    let no_sandbox = std::env::var("VIZRS_BROWSER_NO_SANDBOX").as_deref() == Ok("1");
    Some(Capture::start(CaptureOptions {
        url,
        width: render.0,
        height: render.1,
        out_width: out.0,
        out_height: out.1,
        areas,
        fps: 25.0,
        max_fps: 0.0,
        mode,
        browser: Some(browser),
        no_sandbox,
        extra_args: Vec::new(),
        idle,
        history: Duration::from_secs(5),
        clock_offset: 0.0,
        tape,
        renderer: vizrs_cg::native::Renderer::Chromium,
        renderer_bin: String::new(),
    }))
}

fn until(what: &str, check: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(90);
    while !check() {
        assert!(Instant::now() < deadline, "timeout: {what}");
        thread::sleep(Duration::from_millis(50));
    }
}

const PAGE: &str = r#"<!doctype html><html><body style="margin:0;background:transparent">
<div style="position:absolute;left:40px;top:300px;width:400px;height:60px;background:rgb(255,0,0)"></div>
</body></html>"#;

fn boxes(capture: &Capture) -> Vec<(u32, u32, u32, u32)> {
    capture.current().picture().rects.iter().map(|r| (r.x, r.y, r.w, r.h)).collect()
}

#[test]
fn chromium_capture_gives_the_visible_box_of_a_transparent_page() {
    let _serial = serial();
    let Some(capture) = capture(serve(PAGE), (640, 360), (640, 360), Duration::from_secs(30)) else { return };
    // A cold browser may first send a half-painted frame; the picture must settle on the page.
    until("the red box", || boxes(&capture) == [(40, 300, 400, 60)]);
    assert_eq!(&capture.current().picture().rects[0].pixels[..4], &[255, 0, 0, 255]);
    capture.stop();
}

#[test]
fn chromium_capture_pauses_without_sessions_and_resumes_with_a_fresh_frame() {
    let _serial = serial();
    let Some(capture) = capture(serve(PAGE), (640, 360), (640, 360), Duration::from_millis(200)) else { return };
    let reader = capture.acquire(25.0);
    until("a first frame", || !capture.current().picture().rects.is_empty());
    capture.release(reader);
    until("paused", || capture.status() == "paused");

    // A static page still sends a frame when the screencast starts again.
    let seq = capture.current().seq;
    capture.acquire(25.0);
    assert_eq!(capture.status(), "capturing");
    until("a fresh frame", || capture.current().seq > seq);
    assert_eq!(capture.current().picture().rects.len(), 1);
    capture.stop();
}

#[test]
fn chromium_capture_renders_smaller_and_scales_to_the_channel() {
    let _serial = serial();
    // Proportional layout, like VizRS scaling its 1920x1080 stage to the viewport.
    const SCALED: &str = r#"<!doctype html><html><body style="margin:0;background:transparent">
<div style="position:absolute;left:10%;top:80%;width:50%;height:10%;background:rgb(255,0,0)"></div></body></html>"#;
    let Some(capture) = capture(serve(SCALED), (320, 180), (640, 360), Duration::from_secs(30)) else { return };
    until("the scaled box", || !capture.current().picture().rects.is_empty());

    // Red box 32,144 160x18 at 320x180, plus 4 px margin, doubled.
    let frame = capture.current().picture();
    let rect = &frame.rects[0];
    assert_eq!((rect.x, rect.y, rect.w, rect.h), (56, 280, 336, 52));
    let middle = (((rect.h >> 1) * rect.w + (rect.w >> 1)) * 4) as usize;
    assert_eq!(&rect.pixels[middle..middle + 4], &[255, 0, 0, 255]);
    assert_eq!(capture.key(), "640x360 (render 320x180)");
    capture.stop();
}

/// An animation at the channel rate: the screencast takes every second frame
/// at 25 fps (and screenshots the final state after 150 ms of quiet, without
/// holding back the screencast acks), so new pictures keep coming.
#[test]
fn chromium_capture_follows_an_animation_at_the_channel_rate() {
    let _serial = serial();
    const ANIMATED: &str = r#"<!doctype html><html><body style="margin:0;background:transparent">
<div id="box" style="position:absolute;top:100px;width:80px;height:40px;background:rgb(0,0,255)"></div>
<script>const box = document.getElementById("box");
(function step(t) { box.style.left = (t / 10 % 500) + "px"; requestAnimationFrame(step); })(0);</script>
</body></html>"#;
    let Some(capture) = capture(serve(ANIMATED), (640, 360), (640, 360), Duration::from_secs(30)) else { return };
    until("a first frame", || !capture.current().picture().rects.is_empty());
    thread::sleep(Duration::from_secs(1));
    let seq = capture.current().seq;
    thread::sleep(Duration::from_secs(2));
    let rate = (capture.current().seq - seq) as f64 / 2.0;
    capture.stop();
    // Chromium at 60 Hz / 2 = 30, minus what a busy test host drops.
    assert!(rate >= 12.0, "{rate} pictures per second");
}

/// The frames mode on chromium-headless-shell; another browser has no begin
/// frames and falls back to the screencast, which must show the page as well.
#[test]
fn chromium_frames_capture_shows_the_page_pauses_and_resumes() {
    let _serial = serial();
    let Some(capture) = capture_in(CaptureMode::Frames, serve(PAGE), (640, 360), (640, 360), Duration::from_millis(200)) else { return };
    let reader = capture.acquire(25.0);
    until("the red box", || boxes(&capture) == [(40, 300, 400, 60)]);
    eprintln!("capture mode: {}", capture.mode().name());
    capture.release(reader);
    until("paused", || capture.status() == "paused");

    let seq = capture.current().seq;
    capture.acquire(25.0);
    until("a fresh frame", || capture.current().seq > seq);
    assert_eq!(boxes(&capture), [(40, 300, 400, 60)]);
    capture.stop();
}

/// Begin frames: one picture per frame of the channel, dated on its grid
/// (40 ms at 25 fps), the animation one step further in each.
#[test]
fn chromium_frames_capture_draws_on_the_frame_grid_of_the_channel() {
    let _serial = serial();
    const STEPS: &str = r#"<!doctype html><html><body style="margin:0;background:transparent">
<div id="box" style="position:absolute;top:100px;width:20px;height:20px;background:rgb(0,0,255)"></div>
<script>const box = document.getElementById("box");
(function step(t) { box.style.left = (Math.floor(t / 40 + 0.001) % 600) + "px"; requestAnimationFrame(step); })(0);</script>
</body></html>"#;
    let Some(capture) = capture_in(CaptureMode::Frames, serve(STEPS), (640, 360), (640, 360), Duration::from_secs(30)) else { return };
    until("a first frame", || !capture.current().picture().rects.is_empty());
    thread::sleep(Duration::from_secs(2));
    if capture.mode() != CaptureMode::Frames {
        eprintln!("no begin frames in this browser: skipped");
        return capture.stop();
    }
    let seq = capture.current().seq;
    thread::sleep(Duration::from_secs(2));
    let end = capture.current();
    let rate = (end.seq - seq) as f64 / 2.0;

    // Back from the newest picture: every picture is on the grid, mostly one step apart.
    let (mut frame, mut steps, mut one) = (end, 0, 0);
    while steps < 40 {
        let before = capture.frame_at(frame.at - Duration::from_micros(1));
        if before.seq == 0 {
            break;
        }
        let gap = (frame.at - before.at).as_secs_f64() * 1000.0;
        let slots = (gap / 40.0).round();
        assert!(slots >= 1.0 && (gap - slots * 40.0).abs() < 0.05, "{gap} ms between pictures");
        let x = |f: &vizrs_cg::frames::Frame| f.picture().rects[0].x;
        if slots == 1.0 && x(&frame) == (x(&before) + 1) % 600 {
            one += 1;
        }
        (frame, steps) = (before, steps + 1);
    }
    capture.stop();
    assert!(rate >= 12.0, "{rate} pictures per second");
    assert!(one * 10 >= steps * 7, "{one} of {steps} pictures one step after the one before");
}

/// A graphics area: Chromium draws only that part, the rectangles keep the
/// channel's coordinates, graphics outside it are cut off; also rendered smaller.
#[test]
fn chromium_capture_of_a_graphics_area() {
    let _serial = serial();
    // A box inside the bottom strip, one above it, a static page.
    const STRIP: &str = r#"<!doctype html><html><body style="margin:0;background:transparent">
<div style="position:absolute;left:10%;top:85%;width:25%;height:10%;background:rgb(255,0,0)"></div>
<div style="position:absolute;left:50%;top:10%;width:20%;height:20%;background:rgb(0,0,255)"></div></body></html>"#;
    let area = Area { x: 0, y: 280, w: 640, h: 80 };
    for (mode, render) in [(CaptureMode::Screencast, (640, 360)), (CaptureMode::Frames, (640, 360)), (CaptureMode::Screencast, (320, 180))]
    {
        let url = serve(STRIP);
        let Some(capture) = capture_area(mode, url, render, (640, 360), vec![area], Duration::from_secs(30)) else { return };
        // Red box 64,306 160x36 of the channel; at half size scaled with a margin.
        let expected = if render == (640, 360) { vec![(64, 306, 160, 36)] } else { vec![(56, 298, 176, 52)] };
        until("the box of the area", || boxes(&capture) == expected);
        let rect = &capture.current().picture().rects[0];
        let middle = (((rect.h >> 1) * rect.w + (rect.w >> 1)) * 4) as usize;
        assert_eq!(&rect.pixels[middle..middle + 4], &[255, 0, 0, 255], "{}", mode.name());
        // Still the area after a while (no full-page picture from a screenshot).
        thread::sleep(Duration::from_millis(600));
        assert_eq!(boxes(&capture), expected, "{} {render:?}", mode.name());
        assert!(capture.key().ends_with("area 640x80+0+280"), "{}", capture.key());
        capture.stop();
    }
}

/// Two graphics areas: the frames mode draws each (one PNG per area), the
/// screencast their bounding box cut to them; graphics between them cut off.
#[test]
fn chromium_capture_of_two_graphics_areas() {
    let _serial = serial();
    // Red strip at the bottom, blue logo top right, green box between them.
    const TWO: &str = r#"<!doctype html><html><body style="margin:0;background:transparent">
<div style="position:absolute;left:40px;top:310px;width:200px;height:30px;background:rgb(255,0,0)"></div>
<div style="position:absolute;left:560px;top:20px;width:40px;height:40px;background:rgb(0,0,255)"></div>
<div style="position:absolute;left:300px;top:150px;width:40px;height:40px;background:rgb(0,255,0)"></div></body></html>"#;
    let areas = vec![Area { x: 0, y: 290, w: 640, h: 70 }, Area { x: 540, y: 0, w: 100, h: 80 }];
    for mode in [CaptureMode::Frames, CaptureMode::Screencast] {
        let Some(capture) = capture_area(mode, serve(TWO), (640, 360), (640, 360), areas.clone(), Duration::from_secs(30)) else { return };
        let expected = vec![(40, 310, 200, 30), (560, 20, 40, 40)];
        until("both boxes", || {
            let mut found = boxes(&capture);
            found.sort();
            found == expected
        });
        assert!(capture.key().ends_with("area 640x70+0+290,100x80+540+0"), "{}", capture.key());
        capture.stop();
    }
}

/// The crawl tape drawn by the sidecar: a page with the `vizrsTape`
/// interface of VizRS /output (a red bar, a crawl window on it, a tape of
/// white bands every 30 px moving 4 px per frame). The page hides its tape,
/// Chromium films it now and then, and every picture still shows the bands
/// over the bar, one step further than the picture before.
#[test]
fn chromium_frames_capture_draws_the_crawl_tape() {
    crawl_tape(Vec::new());
}

/// The same with two graphics areas, the crawl in the second one: its own tab.
#[test]
fn chromium_frames_capture_draws_the_crawl_tape_in_a_second_area() {
    crawl_tape(vec![Area { x: 0, y: 0, w: 100, h: 80 }, Area { x: 0, y: 280, w: 640, h: 80 }]);
}

fn crawl_tape(areas: Vec<Area>) {
    let _serial = serial();
    const CRAWL: &str = r#"<!doctype html><html><body style="margin:0;background:transparent">
<div id="bar" style="position:absolute;left:0;top:300px;width:640px;height:40px;background:rgb(200,0,0)"></div>
<div style="position:absolute;left:100px;top:300px;width:400px;height:40px;overflow:hidden">
<div id="tape" style="position:absolute;left:0;top:0;width:200000px;height:40px;will-change:transform;
background:repeating-linear-gradient(90deg,#fff 0 10px,transparent 10px 30px)"></div></div>
<script>const bar = document.getElementById("bar"), tape = document.getElementById("tape");
let ext = false, film = null, d = 0, hidden = false;
window.vizrsTape = {
  state: () => ({ crawls: [{ id: 7, x: 100, y: 300, w: 400, h: 40, s: d, hidden, key: "k" }] }),
  external: (id, on) => { ext = on; return true; },
  film: (id, s) => { film = s; return true; },
};
(function frame(t) {
  d = Math.floor(t / 40 + 0.001) * 4;
  if (film !== null) {
    tape.style.transform = `translateX(${-film}px)`;
    bar.style.visibility = "hidden";
    tape.style.visibility = "visible";
    film = null;
  } else {
    bar.style.visibility = "visible";
    tape.style.transform = `translateX(${-d}px)`;
    hidden = ext;
    tape.style.visibility = hidden ? "hidden" : "visible";
  }
  requestAnimationFrame(frame);
})(0);</script></body></html>"#;
    let Some(capture) = capture_with(CaptureMode::Frames, serve(CRAWL), (640, 360), (640, 360), areas, Duration::from_secs(30), true)
    else {
        return;
    };
    until("a first frame", || !capture.current().picture().rects.is_empty());
    thread::sleep(Duration::from_secs(2));
    if capture.mode() != CaptureMode::Frames {
        eprintln!("no begin frames in this browser: skipped");
        return capture.stop();
    }
    until("the tape drawn by the sidecar", || capture.tape().is_some_and(|(crawl, _)| crawl == 7));
    thread::sleep(Duration::from_secs(3));

    // Row 320 of a picture: white (tape), red (bar) or other, x 0..640.
    let row = |frame: &vizrs_cg::frames::Frame| {
        let mut row = vec![b'.'; 640];
        for r in &frame.picture().rects {
            if (r.y..r.y + r.h).contains(&320) {
                for x in 0..r.w {
                    let p = &r.pixels[(((320 - r.y) * r.w + x) * 4) as usize..][..4];
                    row[(r.x + x) as usize] = match p {
                        [255, 255, 255, 255] => b'w',
                        [200, 0, 0, 255] => b'r',
                        _ => b'.',
                    };
                }
            }
        }
        row
    };
    let (mut frame, mut pairs, mut one) = (capture.current(), 0, 0);
    while pairs < 40 {
        let before = capture.frame_at(frame.at - Duration::from_micros(1));
        if before.seq == 0 {
            break;
        }
        let (now, then) = (row(&frame), row(&before));
        // The bar outside the window, white bands of 10 in every 30 px inside it, nothing else.
        assert!(now[..100].iter().chain(&now[500..]).all(|&c| c == b'r'), "{}", String::from_utf8_lossy(&now));
        let white = now[100..500].iter().filter(|&&c| c == b'w').count();
        assert!((125..=140).contains(&white), "{white} white columns: {}", String::from_utf8_lossy(&now));
        assert!(now[100..500].iter().all(|&c| c != b'.'), "{}", String::from_utf8_lossy(&now));
        let gap = ((frame.at - before.at).as_secs_f64() * 1000.0 / 40.0).round();
        if gap == 1.0 && now[100..496] == then[104..500] {
            one += 1;
        }
        (frame, pairs) = (before, pairs + 1);
    }
    let (_, films) = capture.tape().unwrap();
    capture.stop();
    assert!(pairs >= 20, "{pairs} pictures");
    assert!(one * 10 >= pairs * 7, "{one} of {pairs} pictures one step after the one before");
    // 100 px of tape per second, a window of 400 px: a film every few seconds.
    assert!((1..=6).contains(&films), "{films} films");
}
