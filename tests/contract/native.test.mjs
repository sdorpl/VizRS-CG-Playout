// `renderer: native`: the sidecar draws the burn-in with the VizRS native
// renderer (`vizrs-render stream`) instead of Chromium. A fake renderer here
// speaks the protocol (VizRS DOKUMENTACJA §10.1): it draws a red box, then
// answers "unchanged"; its first run exits after a few frames to check the
// restart. No browser is needed.

import assert from "node:assert/strict";
import { chmodSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { readFrames, yuvaSize } from "./helpers.mjs";
import { runClient, startSidecar } from "./sidecar.mjs";

// The fake renderer: arguments go to a file, frames are 16-byte headers + RGBA.
const FAKE = `#!/usr/bin/env node
const fs = require("fs");
const args = process.argv.slice(2);
const dir = process.env.FAKE_DIR;
fs.appendFileSync(dir + "/args", args.join(" ") + "\\n");
const first = !fs.existsSync(dir + "/started");
fs.writeFileSync(dir + "/started", "1");
const size = args[args.indexOf("--size") + 1].split("x").map(Number);
const ai = args.indexOf("--area");
const [ax, ay, w, h] = ai >= 0 ? args[ai + 1].split(",").map(Number) : [0, 0, ...size];
const areas = args.flatMap((a, i) => (a === "--area" ? [args[i + 1].split(",").map(Number)] : []));
const u32s = (...v) => { const b = Buffer.alloc(v.length * 4); v.forEach((x, i) => b.writeUInt32LE(x, i * 4)); return b; };
const header = (kind, flags) => { const b = Buffer.alloc(16); b.write("VZR1"); b[4] = kind; b[5] = flags; b.writeUInt32LE(w, 8); b.writeUInt32LE(h, 12); return b; };
let n = 0, rest = "";
process.stdin.on("data", (d) => {
  rest += d;
  let i;
  while ((i = rest.indexOf("\\n")) >= 0) {
    const line = rest.slice(0, i); rest = rest.slice(i + 1);
    if (line === "Q") process.exit(0);
    if (!line.startsWith("F ")) continue;
    n++;
    if (first && n > 5) process.exit(3);
    const force = (line.split(" ")[3] || "").includes("f");
    if (areas.length > 1 && (n === 1 || force)) {
      const b = Buffer.alloc(16); b.write("VZR1"); b[4] = 5; b[5] = 3; b.writeUInt32LE(size[0], 8); b.writeUInt32LE(size[1], 12);
      const parts = [b, u32s(areas.length)];
      for (const [x, y, aw, ah] of areas) parts.push(u32s(x, y, aw, ah), Buffer.alloc(aw * ah * 4, Buffer.from([0, 255, 0, 255])));
      process.stdout.write(Buffer.concat(parts));
    } else if (n === 1 || force) {
      const px = Buffer.alloc(w * h * 4);
      for (let y = 100; y < 120; y++) for (let x = 40; x < 160; x++) {
        const lx = x - ax, ly = y - ay;
        if (lx >= 0 && ly >= 0 && lx < w && ly < h) px.set([255, 0, 0, 255], (ly * w + lx) * 4);
      }
      process.stdout.write(Buffer.concat([header(1, 3), px]));
    } else process.stdout.write(header(0, 3));
  }
});
`;

test("renderer native: the VizRS native renderer draws the burn-in, restarted when it exits", { timeout: 60000 }, async (t) => {
  const dir = mkdtempSync(join(tmpdir(), "virtrs-native-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const bin = join(dir, "vizrs-render");
  writeFileSync(bin, FAKE);
  chmodSync(bin, 0o755);

  const cg = await startSidecar(
    { vizrs: { address: "127.0.0.1:9", token: "" }, output_url: "http://127.0.0.1:9/output?token=abc", lead_seconds: 0, renderer: "native", renderer_bin: bin },
    { FAKE_DIR: dir },
  );
  t.after(cg.stop);
  const [width, height] = [320, 180];
  const data = { Resolution: { Width: width, Height: height }, FrameRate: 25, MediaItem_Path: "/m/News.mp4" };

  const script = runClient(`${cg.url}/etv?channel=1`, data, 30000);
  const { session } = await (await fetch(`${cg.url}/ffmpeg?channel=1&wait=5000`)).json();
  const size = yuvaSize(width, height);
  const center = 110 * width + 100;
  const alpha = size - width * height;
  const abort = new AbortController();
  const frames = await fetch(`${cg.url}/ffmpeg/frames?session=${session}`, { signal: abort.signal });
  let buffer = Buffer.alloc(0);
  let drawn = null;
  const deadline = Date.now() + 20000;
  for await (const chunk of frames.body) {
    buffer = Buffer.concat([buffer, chunk]);
    while (buffer.length >= size) {
      const frame = buffer.subarray(0, size);
      buffer = buffer.subarray(size);
      if (frame[alpha + center] === 255) drawn = frame;
    }
    // past the first run's exit and the restart
    const restarted = existsSync(join(dir, "args")) && readFileSync(join(dir, "args"), "utf8").trim().split("\n").length >= 2;
    if ((drawn && restarted) || Date.now() > deadline) break;
  }
  abort.abort();
  assert.ok(drawn, "the native picture reaches the raw frames");
  assert.equal(drawn[center], 81); // opaque red, BT.601
  assert.equal(drawn[alpha], 0);
  const runs = readFileSync(join(dir, "args"), "utf8").trim().split("\n");
  assert.ok(runs.length >= 2, "restarted after it exited");
  assert.equal(runs[0], "stream --url http://127.0.0.1:9/output?token=abc --size 320x180");

  const health = await (await fetch(`${cg.url}/health`)).json();
  const capture = health.captures.find((c) => c.size === "320x180");
  assert.equal(capture.renderer, "native");
  assert.equal(capture.drawn_by, "native");
  await script;

  // Script element path: the same picture as ETV packets (the restart sent it again).
  const last = readFrames(await runClient(`${cg.url}/etv?channel=2`, data, 1500), width, height).at(-1);
  assert.deepEqual([...last.subarray(center * 4, center * 4 + 4)], [255, 0, 0, 255]);
});

test("renderer native with a graphics area: the renderer draws only the area", { timeout: 60000 }, async (t) => {
  const dir = mkdtempSync(join(tmpdir(), "virtrs-native-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const bin = join(dir, "vizrs-render");
  writeFileSync(bin, FAKE);
  chmodSync(bin, 0o755);
  writeFileSync(join(dir, "started"), "1"); // no exit in this test
  const cg = await startSidecar(
    { vizrs: { address: "127.0.0.1:9", token: "" }, output_url: "http://127.0.0.1:9/output", lead_seconds: 0, renderer: "native", renderer_bin: bin, area: "200x40+20+90" },
    { FAKE_DIR: dir },
  );
  t.after(cg.stop);
  const [width, height] = [320, 180];
  const data = { Resolution: { Width: width, Height: height }, FrameRate: 25, MediaItem_Path: "/m/News.mp4" };
  const last = readFrames(await runClient(`${cg.url}/etv?channel=1&area=200x40%2B20%2B90`, data, 2500), width, height).at(-1);
  const at = (px, py) => [...last.subarray((py * width + px) * 4, (py * width + px) * 4 + 4)];
  assert.deepEqual(at(100, 110), [255, 0, 0, 255]);
  assert.deepEqual(at(30, 110), [0, 0, 0, 0]); // outside the area
  const runs = readFileSync(join(dir, "args"), "utf8");
  assert.match(runs, /--size 320x180 --area 20,90,200,40/);
});

test("renderer native with several areas: one process, each area drawn on its own", { timeout: 60000 }, async (t) => {
  const dir = mkdtempSync(join(tmpdir(), "virtrs-native-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const bin = join(dir, "vizrs-render");
  writeFileSync(bin, FAKE);
  chmodSync(bin, 0o755);
  writeFileSync(join(dir, "started"), "1"); // no exit in this test
  const area = "300x20+10+150,40x20+270+10";
  const cg = await startSidecar(
    { vizrs: { address: "127.0.0.1:9", token: "" }, output_url: "http://127.0.0.1:9/output", lead_seconds: 0, renderer: "native", renderer_bin: bin, area },
    { FAKE_DIR: dir },
  );
  t.after(cg.stop);
  const [width, height] = [320, 180];
  const data = { Resolution: { Width: width, Height: height }, FrameRate: 25, MediaItem_Path: "/m/News.mp4" };
  const last = readFrames(await runClient(`${cg.url}/etv?channel=1&area=${encodeURIComponent(area)}`, data, 2500), width, height).at(-1);
  const at = (px, py) => [...last.subarray((py * width + px) * 4, (py * width + px) * 4 + 4)];
  assert.deepEqual(at(100, 160), [0, 255, 0, 255]);
  assert.deepEqual(at(280, 20), [0, 255, 0, 255]);
  assert.deepEqual(at(100, 90), [0, 0, 0, 0]); // between the areas
  const runs = readFileSync(join(dir, "args"), "utf8").trim().split("\n");
  assert.equal(runs.length, 1);
  assert.match(runs[0], /--area 10,150,300,20 --area 270,10,40,20/);
});

test("area auto: footprints of the page on air become the zones of the overlay's run", { timeout: 60000 }, async (t) => {
  const dir = mkdtempSync(join(tmpdir(), "virtrs-native-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const bin = join(dir, "vizrs-render");
  writeFileSync(bin, FAKE);
  chmodSync(bin, 0o755);
  writeFileSync(join(dir, "started"), "1"); // no exit in this test
  // A fake VizRS: page 4001 on air (STATE), every other command OK.
  const { createServer } = await import("node:net");
  const vizrs = createServer((socket) => {
    let rest = "";
    socket.on("data", (d) => {
      rest += d;
      let i;
      while ((i = rest.indexOf("\n")) >= 0) {
        const line = rest.slice(0, i);
        rest = rest.slice(i + 1);
        socket.write(line.endsWith("STATE") ? 'OK {"layers":[{"layer":"main","page":{"id":"p9","number":4001},"since":1}]}\n' : "OK\n");
      }
    });
  });
  await new Promise((resolve) => vizrs.listen(0, "127.0.0.1", resolve));
  t.after(() => vizrs.close());
  const cg = await startSidecar(
    {
      vizrs: { address: `127.0.0.1:${vizrs.address().port}`, token: "" },
      output_url: "http://127.0.0.1:9/output",
      lead_seconds: 0,
      renderer: "native",
      renderer_bin: bin,
      area: "auto",
    },
    { FAKE_DIR: dir },
  );
  t.after(cg.stop);
  const [width, height] = [320, 180];
  const data = { Resolution: { Width: width, Height: height }, FrameRate: 25, MediaItem_Path: "/m/News.mp4" };
  const script = runClient(`${cg.url}/etv?channel=1`, data, 8000);
  // the learner looks every 0.5 s
  await new Promise((resolve) => setTimeout(resolve, 2500));
  const claim = await (await fetch(`${cg.url}/ffmpeg?channel=1&wait=5000&area=1`)).json();
  // the red box 120x20 at 40,100, snapped out to 16 px with a 16 px margin
  assert.deepEqual(claim.areas, [{ x: 16, y: 80, w: 160, h: 64 }]);
  assert.equal(claim.width, 160);
  assert.equal(claim.height, 64);
  assert.match(claim.resume, /graphics=160x64%2B16%2B80/);
  const health = await (await fetch(`${cg.url}/health`)).json();
  assert.equal(health.zones.pages, 1);
  assert.deepEqual(health.zones.runs, [{ channel: "1", zones: "160x64+16+80", share_percent: 18, outside: 0, widenings: 0 }]);
  await script.catch(() => {});
});

test("area auto: zones widen in the middle of the piece, a further frame stream of the session carries the new ones", { timeout: 60000 }, async (t) => {
  const dir = mkdtempSync(join(tmpdir(), "virtrs-native-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const bin = join(dir, "vizrs-render");
  writeFileSync(bin, FAKE);
  chmodSync(bin, 0o755);
  writeFileSync(join(dir, "started"), "1"); // no exit in this test
  // A fake VizRS whose STATE the test changes: page 4001 first.
  let state = '{"layers":[{"layer":"main","page":{"id":"p9","number":4001},"since":1}]}';
  const { createServer } = await import("node:net");
  const vizrs = createServer((socket) => {
    let rest = "";
    socket.on("data", (d) => {
      rest += d;
      let i;
      while ((i = rest.indexOf("\n")) >= 0) {
        const line = rest.slice(0, i);
        rest = rest.slice(i + 1);
        socket.write(line.endsWith("STATE") ? `OK ${state}\n` : "OK\n");
      }
    });
  });
  await new Promise((resolve) => vizrs.listen(0, "127.0.0.1", resolve));
  t.after(() => vizrs.close());
  const cg = await startSidecar(
    {
      vizrs: { address: `127.0.0.1:${vizrs.address().port}`, token: "" },
      output_url: "http://127.0.0.1:9/output",
      lead_seconds: 0,
      renderer: "native",
      renderer_bin: bin,
      area: "auto",
    },
    { FAKE_DIR: dir },
  );
  t.after(cg.stop);
  const [width, height] = [320, 180];
  const data = { Resolution: { Width: width, Height: height }, FrameRate: 25, MediaItem_Path: "/m/News.mp4" };
  const script = runClient(`${cg.url}/etv?channel=1`, data, 12000);
  await new Promise((resolve) => setTimeout(resolve, 2500));
  const claim = await (await fetch(`${cg.url}/ffmpeg?channel=1&wait=5000&area=1`)).json();
  assert.deepEqual(claim.areas, [{ x: 16, y: 80, w: 160, h: 64 }]);
  assert.equal(claim.zones_rev, 0);

  // One yuva420p frame of w x h from a frame stream; the alpha of pixel (x, y).
  const reader = (response) => {
    const r = response.body.getReader();
    let buffered = Buffer.alloc(0);
    return async (w, h) => {
      const size = yuvaSize(w, h);
      while (buffered.length < size) {
        const { value, done } = await r.read();
        if (done) throw new Error("frame stream ended");
        buffered = Buffer.concat([buffered, value]);
      }
      const frame = buffered.subarray(0, size);
      buffered = buffered.subarray(size);
      const alphaStart = size - w * h;
      return (x, y) => frame[alphaStart + y * w + x];
    };
  };
  const first = new AbortController();
  const nextOfFirst = reader(await fetch(`${cg.url}/ffmpeg/frames?session=${claim.session}&area=1`, { signal: first.signal }));
  const zoned = await nextOfFirst(160, 64);
  assert.equal(zoned(50 - 16, 110 - 80), 255, "the red box in the zone");

  const zones = async () => (await fetch(`${cg.url}/ffmpeg/zones?session=${claim.session}`)).json();
  const before = await zones();
  assert.equal(before.rev, 0);
  assert.equal(before.zones, "160x64+16+80");

  // Two pages never seen, taken at the same moment (nobody learns from the
  // picture): pages with no footprint on air, the zones widen to the whole channel.
  state = '{"layers":[{"layer":"a","page":{"id":"p1","number":5005},"since":7},{"layer":"b","page":{"id":"p1","number":5006},"since":7}]}';
  let after = before;
  for (let i = 0; i < 50 && after.rev === 0; i++) {
    await new Promise((resolve) => setTimeout(resolve, 100));
    after = await zones();
  }
  assert.equal(after.rev, 1, "widened");
  assert.equal(after.zones, null, "the whole channel");
  assert.deepEqual([after.width, after.height], [width, height]);
  assert.deepEqual(after.areas, [{ x: 0, y: 0, w: width, h: height }]);
  assert.match(after.resume, /graphics=(auto)?(&|$)/, "resumed: the whole channel");

  // The overlay's next run: a further frame stream in the new zones; the first one goes.
  const further = new AbortController();
  t.after(() => further.abort());
  const path = `/ffmpeg/frames?session=${claim.session}&area=1&rev=${after.rev}&zones=&at=3.5`;
  const nextOfFurther = reader(await fetch(cg.url + path, { signal: further.signal }));
  first.abort();
  const whole = await nextOfFurther(width, height);
  assert.equal(whole(50, 110), 255, "the red box in the whole channel");
  assert.equal(whole(5, 5), 0);
  // The first stream gone does not end the session: the further one goes on.
  await new Promise((resolve) => setTimeout(resolve, 500));
  await nextOfFurther(width, height);
  const health = await (await fetch(`${cg.url}/health`)).json();
  assert.deepEqual(health.zones.runs, [{ channel: "1", zones: null, share_percent: 100, outside: 0, widenings: 1 }]);
  further.abort();
  await script.catch(() => {});
});

// `native_layers`: one renderer per layer of the show (`?layers=` of /output),
// the sidecar stacks their pictures. This fake draws a box per layer: blue for
// `back`, half-transparent red for `main` (over it), both without `layers=`.
const FAKE_LAYERS = `#!/usr/bin/env node
const fs = require("fs");
const args = process.argv.slice(2);
fs.appendFileSync(process.env.FAKE_DIR + "/args", args.join(" ") + "\\n");
const url = args[args.indexOf("--url") + 1];
const layer = (/[?&]layers=([^&]*)/.exec(url) || [])[1] || "";
const [w, h] = args[args.indexOf("--size") + 1].split("x").map(Number);
const boxes = { back: [20, 100, 140, 20, [0, 0, 255, 255]], main: [100, 90, 100, 40, [255, 0, 0, 128]] };
const mine = layer ? [boxes[layer]].filter(Boolean) : Object.values(boxes);
const header = (kind) => { const b = Buffer.alloc(16); b.write("VZR1"); b[4] = kind; b[5] = 3; b.writeUInt32LE(w, 8); b.writeUInt32LE(h, 12); return b; };
let n = 0, rest = "";
process.stdin.on("data", (d) => {
  rest += d;
  let i;
  while ((i = rest.indexOf("\\n")) >= 0) {
    const line = rest.slice(0, i); rest = rest.slice(i + 1);
    if (line === "Q") process.exit(0);
    if (!line.startsWith("F ")) continue;
    n++;
    const force = (line.split(" ")[3] || "").includes("f");
    if (n > 1 && !force) { process.stdout.write(header(0)); continue; }
    if (!mine.length) { process.stdout.write(header(2)); continue; }
    const px = Buffer.alloc(w * h * 4);
    for (const [bx, by, bw, bh, c] of mine) for (let y = by; y < by + bh; y++) for (let x = bx; x < bx + bw; x++) px.set(c, (y * w + x) * 4);
    process.stdout.write(Buffer.concat([header(1), px]));
  }
});
`;

test("native_layers: a renderer per layer of the show, their pictures stacked bottom first", { timeout: 60000 }, async (t) => {
  const dir = mkdtempSync(join(tmpdir(), "virtrs-native-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const bin = join(dir, "vizrs-render");
  writeFileSync(bin, FAKE_LAYERS);
  chmodSync(bin, 0o755);
  // VizRS HTTP: the show with its scenes on two layers (main listed first).
  const { createServer } = await import("node:http");
  const api = createServer((req, res) => {
    res.writeHead(req.url === "/api/show" ? 200 : 404, { "content-type": "application/json" });
    res.end(JSON.stringify({ scenes: [{ name: "Title", layer: "main" }, { name: "Ticker", layer: "back" }], pages: [] }));
  });
  await new Promise((r) => api.listen(0, "127.0.0.1", r));
  t.after(() => api.close());
  const output = `http://127.0.0.1:${api.address().port}/output?token=abc`;
  const cg = await startSidecar(
    { vizrs: { address: "127.0.0.1:9", token: "" }, output_url: output, lead_seconds: 0, renderer: "native", renderer_bin: bin },
    { FAKE_DIR: dir },
  );
  t.after(cg.stop);
  const [width, height] = [320, 180];
  const data = { Resolution: { Width: width, Height: height }, FrameRate: 25, MediaItem_Path: "/m/News.mp4" };
  const at = (frame, x, y) => [...frame.subarray((y * width + x) * 4, (y * width + x) * 4 + 4)];

  // Until the show is read, one renderer for all; then one per layer. (Each
  // session on a channel of its own: a second one on the same channel before
  // the first is gone would be the piece's second element, with no frames.)
  let last;
  let channel = 1;
  const deadline = Date.now() + 15000;
  while (Date.now() < deadline) {
    last = readFrames(await runClient(`${cg.url}/etv?channel=${channel++}`, data, 1500), width, height).at(-1);
    const runs = existsSync(join(dir, "args")) ? readFileSync(join(dir, "args"), "utf8") : "";
    if (/layers=back/.test(runs) && /layers=main/.test(runs)) break;
  }
  const runs = readFileSync(join(dir, "args"), "utf8").trim().split("\n");
  assert.ok(runs.includes(`stream --url ${output}&layers=back --size 320x180`), runs.join("\n"));
  assert.ok(runs.includes(`stream --url ${output}&layers=main --size 320x180`), runs.join("\n"));
  last = readFrames(await runClient(`${cg.url}/etv?channel=${channel}`, data, 1500), width, height).at(-1);
  assert.deepEqual(at(last, 30, 110), [0, 0, 255, 255], "back alone");
  assert.deepEqual(at(last, 150, 95), [255, 0, 0, 128], "main alone");
  assert.deepEqual(at(last, 120, 110), [128, 0, 127, 255], "main over back");
  const health = await (await fetch(`${cg.url}/health`)).json();
  assert.deepEqual(health.captures.find((c) => c.size === "320x180").layers, ["back", "main"]);
});
