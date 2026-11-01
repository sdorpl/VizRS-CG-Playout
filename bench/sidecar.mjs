#!/usr/bin/env node
// Sidecar benchmark as a black box: a build (target/release/vizrs-cg, or
// another one with VIRTRS_CG_BIN) burns bench/scene.html in for one session,
// and the CPU and memory of the sidecar process itself are measured (the
// browser is measured apart from it). Modes (BENCH_MODES):
//   etv      ETV packets for the script element (vizrs-etv-client),
//   ffmpeg   raw yuva420p frames for the ffmpeg overlay at BENCH_FFMPEG_FPS.
// On the animated scene every output frame is read as ErsatzTV or ffmpeg
// would take it: the scene's marker tells whether the motion moved by exactly
// one step (even), stood or skipped.
// Results: a table and one JSON line on stdout (DOKUMENTACJA §8).
//
//   cargo build --release && node bench/sidecar.mjs
//   VIRTRS_CG_BIN=/usr/local/bin/vizrs-cg node bench/sidecar.mjs

import { readFileSync, readdirSync } from "node:fs";
import http from "node:http";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";

import { findBrowser, startSidecar } from "../tests/contract/sidecar.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const env = process.env;
const [WIDTH, HEIGHT] = (env.BENCH_SIZE || "1920x1080").split("x").map(Number);
const FPS = Number(env.BENCH_FPS || 25);
const FFMPEG_FPS = Number(env.BENCH_FFMPEG_FPS || 50);
const WARMUP = Number(env.BENCH_WARMUP || 10);
const DURATION = Number(env.BENCH_DURATION || 30);
const SCENE = env.BENCH_URL ? "url" : env.BENCH_SCENE || "animated";
const MODES = (env.BENCH_MODES || "etv,ffmpeg").split(",").filter(Boolean);
// Sidecar options under test: render_size, capture_mode, max_fps.
const RENDER = env.BENCH_RENDER || "";
const CAPTURE = env.BENCH_CAPTURE || "frames";
const MAX_FPS = Number(env.BENCH_MAX_FPS || 0);
// The rate of the scene and of the evenness check, for a capture that lowers its
// rate by itself (the rate guard, DOKUMENTACJA §5): the rate it should end up at.
const SCENE_FPS = Number(env.BENCH_SCENE_FPS || 0);
// The sidecar draws a standing VizRS crawl itself (tape; BENCH_URL with a crawl on air).
const TAPE = env.BENCH_TAPE === "1";
// Graphics areas (WxH+X+Y, comma-separated): the marker goes to the top-left
// corner of the first, ffmpeg asks for their frames.
const AREA = env.BENCH_AREA || "";
const [AX, AY] = (/^\d+x\d+\+(\d+)\+(\d+)/.exec(AREA) ?? [0, 0, 0]).slice(1).map(Number);
const CLK_TCK = 100;
const HEADER = 11;

/** CPU seconds (user + system) and resident memory of one process. */
function usage(pid) {
  const stat = readFileSync(`/proc/${pid}/stat`, "utf8");
  const fields = stat.slice(stat.lastIndexOf(")") + 2).split(" ");
  const rss = Number(/VmRSS:\s+(\d+)/.exec(readFileSync(`/proc/${pid}/status`, "utf8"))[1]);
  return { cpu: (Number(fields[11]) + Number(fields[12])) / CLK_TCK, rss_mb: rss / 1024 };
}

/** CPU seconds of the descendants of `pid` (the browser and its helpers). */
function descendantsCpu(pid) {
  const all = new Map();
  for (const name of readdirSync("/proc")) {
    if (!/^\d+$/.test(name)) continue;
    try {
      const stat = readFileSync(`/proc/${name}/stat`, "utf8");
      const fields = stat.slice(stat.lastIndexOf(")") + 2).split(" ");
      all.set(Number(name), { ppid: Number(fields[1]), cpu: (Number(fields[11]) + Number(fields[12])) / CLK_TCK });
    } catch {
      // gone
    }
  }
  const pids = new Set([pid]);
  for (let grew = true; grew;) {
    grew = false;
    for (const [child, info] of all) {
      if (!pids.has(child) && pids.has(info.ppid)) {
        pids.add(child);
        grew = true;
      }
    }
  }
  pids.delete(pid);
  return [...pids].reduce((sum, child) => sum + all.get(child).cpu, 0);
}

/**
 * Marker of bench/scene.html: 16 cells of 8x8 px at the render size, sampled
 * in their middle at the channel size; returns a reader of `(x, y) -> [white,
 * opaque]` per cell, or null.
 */
function markerPoints(render) {
  const [rw, rh] = render.split("x").map(Number);
  const [bx, by] = [Math.round((AX / WIDTH) * rw), Math.round((AY / HEIGHT) * rh)];
  return Array.from({ length: 16 }, (_, bit) => [Math.floor(((bx + 8 * bit + 4) * WIDTH) / rw), Math.floor(((by + 4) * HEIGHT) / rh)]);
}

/** Animation step from 16 [white, opaque] samples, or null without the marker. */
function stepOf(samples) {
  if (samples.some(([, opaque]) => !opaque)) return null;
  return samples.reduce((step, [white], bit) => step | (white ? 1 << bit : 0), 0);
}

/** Steps of an ETV packet stream: one per output frame, as ErsatzTV takes them. */
function packetSteps(points, push) {
  let chunks = [];
  let length = 0;
  let last = null;
  const rectangles = (packet) => {
    const samples = points.map(() => [false, false]);
    let offset = HEADER + 2;
    for (let i = packet.readUInt16BE(HEADER); i > 0; i--) {
      const [x, y, w, h] = [0, 2, 4, 6].map((at) => packet.readUInt16BE(offset + at));
      offset += 8;
      points.forEach(([px, py], cell) => {
        if (px >= x && px < x + w && py >= y && py < y + h) {
          const at = offset + ((py - y) * w + (px - x)) * 4;
          samples[cell] = [packet[at] > 127, packet[at + 3] > 127];
        }
      });
      offset += w * h * 4;
    }
    return stepOf(samples);
  };
  return (chunk) => {
    chunks.push(chunk);
    length += chunk.length;
    for (;;) {
      if (length < HEADER) return;
      let buffer = chunks.length === 1 ? chunks[0] : Buffer.concat(chunks, length);
      const size = HEADER + buffer.readUInt32BE(7);
      if (length < size) {
        chunks = [buffer];
        return;
      }
      const type = buffer[6];
      if (type === 3) last = rectangles(buffer);
      else if (type === 2) last = null;
      const frames = type === 1 ? buffer.readUInt32BE(HEADER) + 1 : 1;
      for (let i = 0; i < frames; i++) push(last);
      buffer = buffer.subarray(size);
      chunks = buffer.length ? [buffer] : [];
      length = buffer.length;
    }
  };
}

/** Steps of raw yuva420p frames of `part` (x, y, w, h): luma and alpha of the marker cells in each frame. */
function yuvaSteps(points, push, [px, py, pw, ph]) {
  const cw = Math.ceil(pw / 2);
  const ch = Math.ceil(ph / 2);
  const size = pw * ph * 2 + cw * ch * 2;
  const alpha = pw * ph + 2 * cw * ch;
  // Byte offsets in a frame, in order: luma of each cell, then its alpha.
  const local = points.map(([x, y]) => [x - px, y - py]);
  const wanted = local.flatMap(([x, y]) => [y * pw + x, alpha + y * pw + x]).map((at, i) => ({ at, i })).sort((a, b) => a.at - b.at);
  let position = 0;
  let values = [];
  return (chunk) => {
    let start = 0;
    while (start < chunk.length) {
      const end = Math.min(chunk.length, start + size - position);
      for (const { at, i } of wanted) {
        if (at >= position && at < position + end - start) values[i] = chunk[start + at - position];
      }
      position += end - start;
      start = end;
      if (position === size) {
        const samples = points.map((_, cell) => [values[2 * cell] > 125, values[2 * cell + 1] > 127]);
        push(stepOf(samples));
        position = 0;
        values = [];
      }
    }
  };
}

/**
 * Share of output frames that move as they should, stand where they should not
 * or skip: the scene steps once per `hold` output frames (1, or 2 with a 50 fps
 * output and a capture limited to 25), so an even frame is one step after
 * `hold` frames, or no step before that.
 */
function motion(steps, hold) {
  let [one, stood, skipped] = [0, 0, 0];
  let since = 0;
  for (let i = 1; i < steps.length; i++) {
    if (steps[i] === null || steps[i - 1] === null) continue;
    const delta = (steps[i] - steps[i - 1] + 65536) % 65536;
    since += 1;
    if (delta === 0 && since < hold) one += 1;
    else if (delta === 0) stood += 1;
    else if (delta === 1 && since === hold) one += 1;
    else if (delta < 1000) skipped += 1;
    if (delta !== 0) since = 0;
  }
  const total = one + stood + skipped;
  const pct = (n) => (total ? Math.round((n / total) * 1000) / 10 : null);
  return { even_percent: pct(one), stood_percent: pct(stood), skipped_percent: pct(skipped) };
}

// The scene and its headline events, as bench/bench.mjs serves them.
const server = http.createServer((req, res) => {
  if (req.url.startsWith("/scene.html")) {
    res.setHeader("Content-Type", "text/html; charset=utf-8");
    res.end(readFileSync(path.join(here, "scene.html")));
  } else if (req.url === "/events") {
    res.writeHead(200, { "Content-Type": "text/event-stream" });
    const send = () => res.write(`data: ${JSON.stringify({ title: "Wiadomości", sub: `Serwis o ${new Date().toLocaleTimeString("pl")}` })}\n\n`);
    send();
    const timer = setInterval(send, 3000);
    req.on("close", () => clearInterval(timer));
  } else {
    res.statusCode = 404;
    res.end();
  }
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
// BENCH_URL: the real VizRS /output instead of the scene (graphics must be on air).
// The scene steps at the rate of the mode's output frames.
const sceneUrl = (fps) => env.BENCH_URL || `http://127.0.0.1:${server.address().port}/scene.html?scene=${SCENE}&fps=${fps}&mx=${AX / WIDTH}&my=${AY / HEIGHT}`;

const browser = findBrowser();
if (!browser) throw new Error("no Chromium (set VIZRS_BROWSER)");
const data = { Resolution: { Width: WIDTH, Height: HEIGHT }, FrameRate: FPS, MediaItem_Path: "/m/bench.mp4" };

async function run(mode) {
  const fps = mode === "ffmpeg" ? FFMPEG_FPS : FPS;
  // The scene moves at most as often as the sidecar captures (max_fps), like VizRS ?fps=.
  const sceneFps = SCENE_FPS || (MAX_FPS > 0 ? Math.min(fps, MAX_FPS) : fps);
  const cg = await startSidecar(
    {
      vizrs: { address: "127.0.0.1:9", token: "" },
      output_url: sceneUrl(sceneFps),
      lead_seconds: 1,
      render_size: RENDER,
      capture_mode: CAPTURE,
      max_fps: MAX_FPS,
      area: AREA,
      tape: TAPE,
    },
    { VIZRS_BROWSER: browser, VIZRS_BROWSER_NO_SANDBOX: "1" },
  );
  const script = await fetch(`${cg.url}/etv?channel=1`, { method: "POST", body: JSON.stringify(data) });
  const started = (await cg.health()).captures[0] ?? {};
  const points = markerPoints(started.render ?? `${WIDTH}x${HEIGHT}`);
  // The frames ffmpeg gets: the graphics area (area=1), or the channel.
  // Several areas: stacked, the first one on top (where the marker is).
  const areas = started.area ? started.area.split(",").map((g) => g.split(/[x+]/).map(Number)) : [[WIDTH, HEIGHT, 0, 0]];
  const [ax, ay] = areas[0].slice(2);
  const [aw, ah] = [Math.max(...areas.map((a) => a[0])), areas.reduce((sum, a) => sum + a[1], 0)];
  const probe = { steps: null };
  const readers = [script.body.getReader()];
  let bytes = 0;
  // The overlay takes the session over; the script session stays open (and silent), as in ErsatzTV.
  if (mode === "ffmpeg") {
    const claim = await (await fetch(`${cg.url}/ffmpeg?channel=1&wait=5000&area=1`)).json();
    const frames = await fetch(`${cg.url}/ffmpeg/frames?session=${claim.session}&fps=${FFMPEG_FPS}&area=1`);
    readers.push(frames.body.getReader());
  }
  // Steps of the stream ErsatzTV or ffmpeg reads (the last one), kept during the measurement.
  const push = (step) => probe.steps?.push(step);
  const parse = mode === "ffmpeg" ? yuvaSteps(points, push, [ax, ay, aw, ah]) : packetSteps(points, push);
  const drain = Promise.all(readers.map(async (reader, index) => {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      bytes += value.length;
      if (index === readers.length - 1 && SCENE === "animated") parse(Buffer.from(value.buffer, value.byteOffset, value.length));
    }
  })).catch(() => {});

  await sleep(WARMUP * 1000);
  const health0 = await cg.health();
  const seq0 = health0.captures[0]?.seq ?? 0;
  const [u0, c0, b0, t0] = [usage(cg.pid), descendantsCpu(cg.pid), bytes, performance.now()];
  probe.steps = [];
  await sleep(DURATION * 1000);
  const seconds = (performance.now() - t0) / 1000;
  const [u1, c1, health1] = [usage(cg.pid), descendantsCpu(cg.pid), await cg.health()];
  const seq1 = health1.captures[0]?.seq ?? 0;
  // The crawl drawn by the sidecar at the end and the films taken while measuring.
  const [tape0, tape1] = [health0, health1].map((h) => h.captures[0]?.tape);
  const steps = probe.steps;
  probe.steps = null;
  for (const reader of readers) reader.cancel().catch(() => {});
  await drain;
  await cg.stop();

  const capture = health0.captures[0] ?? {};
  return {
    build: path.basename(env.VIRTRS_CG_BIN || "vizrs-cg"),
    mode,
    size: `${WIDTH}x${HEIGHT}`,
    render: capture.render,
    area: capture.area ?? "",
    fps,
    capture: capture.mode,
    max_fps: MAX_FPS,
    tape: TAPE ? `crawl ${tape1?.crawl ?? "–"}, ${(tape1?.films ?? 0) - (tape0?.films ?? 0)} films` : "",
    scene: SCENE,
    cpu_percent: Math.round(((u1.cpu - u0.cpu) / seconds) * 1000) / 10,
    browser_cpu_percent: Math.round(((c1 - c0) / seconds) * 1000) / 10,
    rss_mb: Math.round(u1.rss_mb),
    pictures_per_s: Math.round(((seq1 - seq0) / seconds) * 10) / 10,
    mb_per_s: Math.round(((bytes - b0) / seconds / 1e6) * 10) / 10,
    ...(SCENE === "animated" ? motion(steps, Math.max(1, Math.round(fps / sceneFps))) : {}),
  };
}

const results = [];
for (const mode of MODES) results.push(await run(mode));
server.close();
console.table(results);
console.log(JSON.stringify(results));
