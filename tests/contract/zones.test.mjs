// `area: auto` with the ffmpeg overlay (PLAN-strefy S3b): graphics leaving
// the zones of a run widen them in the middle of the piece, the overlay stops
// the run and the next one, from where the piece is, overlays the frames in the
// new zones – one HLS playlist, timestamps going on. A fake native renderer
// draws the graphics; a real ffmpeg (VIZRS_TEST_FFMPEG or `ffmpeg` in PATH)
// transcodes; skipped without it.

import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import http from "node:http";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";

import { CLIENT, startSidecar } from "./sidecar.mjs";

const WRAPPER = process.env.VIZRS_FFMPEG_BIN || fileURLToPath(new URL("../../target/release/vizrs-ffmpeg", import.meta.url));

function findFfmpeg() {
  const candidate = process.env.VIZRS_TEST_FFMPEG || "ffmpeg";
  try {
    execFileSync(candidate, ["-hide_banner", "-version"], { stdio: "ignore" });
    return candidate;
  } catch {
    return null;
  }
}
const FFMPEG = findFfmpeg();
const FFPROBE = FFMPEG && path.join(path.dirname(FFMPEG), "ffprobe").replace(/^ffprobe$/, "ffprobe");

// A full picture every frame: a red box always, a green one far from it once
// the file `wide` is there (an animation going beyond the page's footprint).
const RENDERER = `#!/usr/bin/env node
const fs = require("fs");
const args = process.argv.slice(2);
const [w, h] = args[args.indexOf("--size") + 1].split("x").map(Number);
const header = Buffer.alloc(16); header.write("VZR1"); header[4] = 1; header[5] = 3;
header.writeUInt32LE(w, 8); header.writeUInt32LE(h, 12);
const box = (px, x0, y0, x1, y1, rgba) => { for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) px.set(rgba, (y * w + x) * 4); };
let rest = "";
process.stdin.on("data", (d) => {
  rest += d;
  let i;
  while ((i = rest.indexOf("\\n")) >= 0) {
    const line = rest.slice(0, i); rest = rest.slice(i + 1);
    if (line === "Q") process.exit(0);
    if (!line.startsWith("F ")) continue;
    const px = Buffer.alloc(w * h * 4);
    box(px, 40, 100, 160, 120, [255, 0, 0, 255]);
    if (fs.existsSync(process.env.FAKE_DIR + "/wide")) box(px, 200, 10, 300, 40, [0, 255, 0, 255]);
    process.stdout.write(Buffer.concat([header, px]));
  }
});
`;

function listen(server) {
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server.address().port)));
}

function pixel(file, x, y) {
  const raw = execFileSync(FFMPEG, ["-v", "error", "-i", file, "-frames:v", "1", "-vf", `crop=2:2:${x}:${y}`, "-f", "rawvideo", "-pix_fmt", "rgb24", "-"]);
  return [...raw.subarray(0, 3)];
}

test("area auto: graphics beyond the zones widen them in the middle of the piece, the next run overlays them", { skip: !FFMPEG && "no ffmpeg", timeout: 90000 }, async (t) => {
  const dir = mkdtempSync(path.join(os.tmpdir(), "vizrs-zones-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const bin = path.join(dir, "vizrs-render");
  writeFileSync(bin, RENDERER);
  chmodSync(bin, 0o755);
  // The page on air has a footprint already (its red box): the first run gets that zone only.
  mkdirSync(path.join(dir, "vizrs-live"));
  writeFileSync(path.join(dir, "vizrs-live", "footprints.json"), JSON.stringify({ version: 2, pages: { "p9:4001@320x180": ["160x64+16+80"] } }));
  const vizrs = net.createServer((socket) => {
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
  await listen(vizrs);
  t.after(() => vizrs.close());

  const media = path.join(dir, "news.mp4");
  execFileSync(FFMPEG, [
    "-v", "error", "-f", "lavfi", "-i", "color=c=blue:s=320x180:r=25", "-f", "lavfi", "-i", "sine=f=440:sample_rate=48000",
    "-t", "30", "-c:v", "libx264", "-preset", "ultrafast", "-g", "25", "-c:a", "aac", "-shortest", media,
  ]);
  const out = path.join(dir, "transcode", "1");
  mkdirSync(out, { recursive: true });

  const cg = await startSidecar(
    {
      vizrs: { address: `127.0.0.1:${vizrs.address().port}`, token: "" },
      output_url: "http://127.0.0.1:9/output",
      lead_seconds: 0.4,
      in_seconds: 0,
      out_seconds: -1,
      renderer: "native",
      renderer_bin: bin,
      area: "auto",
      etv_config: dir,
    },
    { FAKE_DIR: dir },
  );
  t.after(cg.stop);

  // ErsatzTV stand-in: the canvas request starts the script element of the item.
  const etv = http.createServer((request, response) => {
    const child = spawn(CLIENT, [`${cg.url}/etv?channel=1`]);
    child.stdout.resume();
    child.stdin.end(JSON.stringify({ Resolution: { Width: 320, Height: 180 }, FrameRate: 25, MediaItem_Path: media, MediaItem_Title: "News" }));
    response.writeHead(200);
    response.flushHeaders();
    response.on("close", () => child.kill());
  });
  const etvPort = await listen(etv);
  t.after(() => etv.close());

  // The piece as ErsatzTV Next builds it; no live ingest in the sidecar: the runs serve the zones only.
  const headers = "x-etv-channel:1\r\nx-etv-offset-ms:0\r\nx-etv-duration-ms:14000\r\nx-etv-frame-rate:25/1\r\n";
  const piece = spawn(WRAPPER, [
    "-nostdin", "-hide_banner", "-loglevel", "error", "-readrate", "1.0", "-i", media,
    "-headers", headers, "-i", `http://127.0.0.1:${etvPort}/internal/graphics/1/7?exp=1&sig=x`,
    "-filter_complex", "[0:0]fps=25,setsar=1[v_m0];[1:0]format=yuva420p[v_s0];[v_m0][v_s0]overlay=x=0:y=0:format=auto[v_o0]",
    "-map", "[v_o0]", "-map", "0:1", "-c:v", "libx264", "-preset", "ultrafast", "-c:a", "aac", "-ac", "2", "-t", "14000ms", "-g", "50",
    "-f", "hls", "-hls_time", "2", "-hls_list_size", "0", "-hls_segment_filename", path.join(out, "live%06d.ts"), "-hls_segment_type", "mpegts",
    "-hls_flags", "program_date_time+omit_endlist+append_list+independent_segments", "-hls_segment_options", "mpegts_flags=+initial_discontinuity",
    path.join(out, "live.m3u8"),
  ], { env: { PATH: process.env.PATH, VIZRS_FFMPEG: FFMPEG, VIZRS_CG_URL: cg.url, VIZRS_FFMPEG_INGEST: "1" }, stdio: ["ignore", "ignore", "pipe"] });
  const pieceLog = [];
  piece.stderr.on("data", (chunk) => pieceLog.push(chunk));
  t.after(() => piece.kill("SIGKILL"));
  const pieceExit = new Promise((resolve) => piece.once("exit", resolve));

  await sleep(5000);
  const before = await (await fetch(`${cg.url}/health`)).json();
  assert.deepEqual(before.zones.runs.map((r) => [r.zones, r.widenings]), [["160x64+16+80", 0]], `the zone of the page\n${cg.logs()}`);
  writeFileSync(path.join(dir, "wide"), "1");

  const code = await pieceExit;
  const log = Buffer.concat(pieceLog).toString();
  assert.equal(code, 0, log);
  assert.match(log, /piece restarted for new graphics zones after \d+\.\d s/, log);
  // Seen in the picture streamed, not at the next learning step: the run stops before a frame cut off.
  assert.match(cg.logs(), /graphics zones( widened to|: the whole channel) .*\(graphics outside the zones, frames held\)/, cg.logs());

  // One playlist of the whole piece, timestamps going on across the runs.
  const playlist = readFileSync(path.join(out, "live.m3u8"), "utf8");
  const segments = [...playlist.matchAll(/#EXTINF:([\d.]+),\s*\n(?:#.*\n)*(live\d+\.ts)/g)].map((m) => [m[2], Number(m[1])]);
  const total = segments.reduce((sum, [, length]) => sum + length, 0);
  assert.ok(Math.abs(total - 14) < 0.3, `the piece is 14 s long: ${total}\n${playlist}`);
  let end = null;
  const green = [];
  for (const [name] of segments) {
    const file = path.join(out, name);
    assert.ok(existsSync(file), name);
    const probe = execFileSync(FFPROBE, ["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=start_time,duration", "-of", "csv=p=0", file]).toString();
    const [start, duration] = probe.trim().split("\n").filter(Boolean)[0].split(",").map(Number);
    if (end !== null) assert.ok(Math.abs(start - end) < 0.1, `${name} starts where the one before ends: ${start} vs ${end}`);
    end = start + duration;
    const [r, g, b] = pixel(file, 250, 25);
    green.push(g > 200 && r < 60 && b < 60);
  }
  // The red box in its zone; the green one, outside it, overlaid after the widening (before
  // S3b it stayed cut off until the end of the run, here the whole piece).
  assert.deepEqual(pixel(path.join(out, segments[segments.length - 1][0]), 100, 110).map((v) => v > 128), [true, false, false], "the red box");
  assert.ok(!green[0] && green[green.length - 1], `green only after the widening: ${JSON.stringify(green)}\n${log}\n${cg.logs()}`);
});
