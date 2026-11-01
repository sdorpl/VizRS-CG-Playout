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
