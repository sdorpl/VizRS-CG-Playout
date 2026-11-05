// Live ingest (DOKUMENTACJA §1, Live ingest): the sidecar's /ingest API with
// hand-made MPEG-TS, and the whole path with a real ffmpeg (VIZRS_TEST_FFMPEG
// or ffmpeg on PATH; skipped without one): an RTMP publisher on the listener
// (`vizrs-ffmpeg vizrs-ingest`) takes a piece over that the overlay plays to
// HLS with the graphics of the sidecar, and the piece goes on after it.

import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync } from "node:fs";
import http from "node:http";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";

import { CLIENT, startSidecar } from "./sidecar.mjs";

const WRAPPER = process.env.VIZRS_FFMPEG_BIN || fileURLToPath(new URL("../../target/release/vizrs-ffmpeg", import.meta.url));
const TOKEN = "ingest-test";

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

async function sidecar(t, config, env) {
  const cg = await startSidecar(config, env);
  t.after(cg.stop);
  return cg;
}

function listen(server) {
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server.address().port)));
}

function freePort() {
  return new Promise((resolve) => {
    const server = net.createServer();
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close(() => resolve(port));
    });
  });
}

// --- hand-made MPEG-TS -------------------------------------------------------

function packet(pid, { start = false, key = false, payload = [] } = {}) {
  const p = Buffer.alloc(188, 0xff);
  p[0] = 0x47;
  p[1] = ((pid >> 8) & 0x1f) | (start ? 0x40 : 0);
  p[2] = pid & 0xff;
  let at = 4;
  if (key) {
    p[3] = 0x30;
    p[4] = 1;
    p[5] = 0x40;
    at = 6;
  } else {
    p[3] = 0x10;
  }
  Buffer.from(payload).copy(p, at);
  return p;
}

const pat = () => packet(0, { start: true, payload: [0, 0x00, 0xb0, 0x0d, 0, 1, 0xc1, 0, 0, 0, 1, 0xf0, 0x00, 0, 0, 0, 0] });
const pmt = () =>
  packet(0x1000, {
    start: true,
    payload: [0, 0x02, 0xb0, 0x17, 0, 1, 0xc1, 0, 0, 0xe1, 0x00, 0xf0, 0x00, 0x1b, 0xe1, 0x00, 0xf0, 0x00, 0x0f, 0xe1, 0x01, 0xf0, 0x00, 0, 0, 0, 0],
  });
const video = (key, n) => packet(0x100, { start: true, key, payload: [n] });

/** `POST /ingest`: resolves with the status and a writer for the body. */
function postSource(cg, channel, token) {
  return new Promise((resolve, reject) => {
    const socket = net.connect(cg.port, "127.0.0.1", () => {
      socket.write(`POST /ingest?channel=${channel} HTTP/1.0\r\nHost: cg\r\nAuthorization: Bearer ${token}\r\n\r\n`);
    });
    let head = "";
    socket.on("data", function onData(chunk) {
      head += chunk.toString("latin1");
      if (head.includes("\r\n\r\n")) {
        socket.off("data", onData);
        resolve({ status: Number(head.split(" ")[1]), socket });
      }
    });
    socket.on("error", reject);
  });
}

async function state(cg, channel) {
  return (await fetch(`${cg.url}/ingest/state?channel=${channel}`)).json();
}

test("ingest API: token, live from the first keyframe, readers join there, one source per channel", async (t) => {
  const cg = await sidecar(t, { ingest_token: TOKEN });
  assert.deepEqual(await state(cg, "1"), { enabled: true, live: false, audio: false, video: null, seconds: null });

  const refused = await postSource(cg, "1", "wrong");
  assert.equal(refused.status, 401);
  refused.socket.destroy();

  const source = await postSource(cg, "1", TOKEN);
  assert.equal(source.status, 200);
  source.socket.write(Buffer.concat([pat(), pmt(), video(false, 1)]));
  await sleep(200);
  assert.equal((await state(cg, "1")).live, false, "no keyframe yet");
  assert.equal((await fetch(`${cg.url}/ingest?channel=1`)).status, 404);

  source.socket.write(Buffer.concat([video(true, 2), video(false, 3)]));
  for (let i = 0; i < 40 && !(await state(cg, "1")).live; i++) await sleep(50);
  assert.deepEqual(await state(cg, "1"), { enabled: true, live: true, audio: true, video: "h264", seconds: 0 });
  assert.equal((await cg.health()).ingest[0].channel, "1");

  const second = await postSource(cg, "1", TOKEN);
  assert.equal(second.status, 409, "an active source is not pre-empted");
  second.socket.destroy();

  // A reader gets PAT, PMT and the stream from the keyframe on, then what follows.
  const reader = await fetch(`${cg.url}/ingest?channel=1`);
  assert.equal(reader.status, 200);
  const chunks = reader.body.getReader();
  let got = Buffer.alloc(0);
  while (got.length < 4 * 188) got = Buffer.concat([got, Buffer.from((await chunks.read()).value)]);
  assert.deepEqual(got, Buffer.concat([pat(), pmt(), video(true, 2), video(false, 3)]));
  source.socket.write(video(false, 4));
  while (got.length < 5 * 188) got = Buffer.concat([got, Buffer.from((await chunks.read()).value)]);
  assert.deepEqual(got.subarray(4 * 188), video(false, 4));

  // The source ends: the reader's stream ends, the channel is off.
  source.socket.end();
  for (;;) {
    const { done } = await chunks.read();
    if (done) break;
  }
  for (let i = 0; i < 40 && (await state(cg, "1")).live; i++) await sleep(50);
  assert.equal((await state(cg, "1")).live, false);

  // Without ingest_token: off.
  const off = await sidecar(t, {});
  assert.equal((await state(off, "1")).enabled, false);
  const none = await postSource(off, "1", "");
  assert.equal(none.status, 404);
  none.socket.destroy();
});

/** Colour of the pixel at x, y of the first frame of a segment. */
function pixel(file, x, y) {
  const raw = execFileSync(FFMPEG, ["-v", "error", "-i", file, "-frames:v", "1", "-vf", `crop=2:2:${x}:${y}`, "-f", "rawvideo", "-pix_fmt", "rgb24", "-"]);
  return [...raw.subarray(0, 3)];
}

test("live takes a piece over: RTMP listener, overlay with graphics, the piece goes on after it", { skip: !FFMPEG && "no ffmpeg" }, async (t) => {
  const dir = mkdtempSync(path.join(os.tmpdir(), "vizrs-ingest-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const media = path.join(dir, "news.mp4");
  execFileSync(FFMPEG, [
    "-v", "error", "-f", "lavfi", "-i", "testsrc2=s=320x180:r=25", "-f", "lavfi", "-i", "sine=f=440:sample_rate=48000",
    "-t", "30", "-c:v", "libx264", "-preset", "ultrafast", "-g", "25", "-c:a", "aac", "-shortest", media,
  ]);
  const out = path.join(dir, "transcode", "1");
  mkdirSync(out, { recursive: true });

  const cg = await sidecar(t, { ingest_token: TOKEN, output_url: "http://127.0.0.1:9/output", lead_seconds: 0.4, in_seconds: 0, out_seconds: -1 });

  // ErsatzTV stand-in: the canvas request starts the script element of the item.
  const etv = http.createServer((request, response) => {
    const child = spawn(CLIENT, [`${cg.url}/etv?channel=1`]);
    child.stdout.resume();
    child.stdin.end(JSON.stringify({
      Resolution: { Width: 320, Height: 180 },
      FrameRate: 25,
      MediaItem_Path: media,
      MediaItem_Title: "News",
      MediaItem_Start: "2026-10-24T20:00:00+02:00",
      MediaItem_Stop: "2026-10-24T20:30:00+02:00",
    }));
    response.writeHead(200);
    response.flushHeaders();
    response.on("close", () => child.kill());
  });
  const etvPort = await listen(etv);
  t.after(() => etv.close());

  const rtmp = `rtmp://127.0.0.1:${await freePort()}/live/key`;
  const listener = spawn(WRAPPER, ["vizrs-ingest"], {
    env: { PATH: process.env.PATH, VIZRS_CG_URL: cg.url, VIZRS_INGEST_TOKEN: TOKEN, VIZRS_FFMPEG: FFMPEG, VIZRS_INGEST: `1=${rtmp}` },
    stdio: ["ignore", "ignore", "pipe"],
  });
  const listenerLog = [];
  listener.stderr.on("data", (chunk) => listenerLog.push(chunk));
  t.after(() => listener.kill("SIGKILL"));

  // The piece as ErsatzTV Next builds it: realtime, graphics canvas overlaid, 14 s to HLS.
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
  const pieceExit = new Promise((resolve) => piece.once("exit", resolve));

  // 3 s into the piece a publisher goes live for 4 s: 640x360 bars, other audio.
  await sleep(3000);
  const publisher = spawn(FFMPEG, [
    "-v", "error", "-re", "-f", "lavfi", "-i", "smptebars=s=640x360:r=30", "-f", "lavfi", "-i", "sine=f=880:sample_rate=44100",
    "-t", "4", "-c:v", "libx264", "-preset", "ultrafast", "-g", "15", "-c:a", "aac", "-f", "flv", rtmp,
  ], { stdio: "ignore" });
  t.after(() => publisher.kill("SIGKILL"));
  let seen = false;
  for (let i = 0; i < 60 && !seen; i++) {
    seen = (await state(cg, "1")).live;
    await sleep(100);
  }
  assert.ok(seen, `the channel went live\n${Buffer.concat(listenerLog)}`);

  const code = await pieceExit;
  const log = Buffer.concat(pieceLog).toString();
  assert.equal(code, 0, log);
  assert.match(log, /channel 1: live from \d+\.\d s of the piece/);

  // One playlist of the whole piece: its length, the channel's size, timestamps going on.
  const playlist = readFileSync(path.join(out, "live.m3u8"), "utf8");
  const lengths = [...playlist.matchAll(/#EXTINF:([\d.]+),\s*\n(?:#.*\n)*(live\d+\.ts)/g)].map((m) => [m[2], Number(m[1])]);
  const total = lengths.reduce((sum, [, length]) => sum + length, 0);
  assert.ok(Math.abs(total - 14) < 0.3, `the piece is 14 s long: ${total}\n${playlist}`);
  assert.deepEqual(readdirSync(out).filter((f) => f.endsWith(".ts")).sort(), lengths.map(([name]) => name).sort());
  let end = null;
  const colours = [];
  for (const [name] of lengths) {
    const file = path.join(out, name);
    // Video timing: AAC ends a little after the video, and ErsatzTV goes on where the video ends.
    const probe = execFileSync(FFPROBE, ["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height,start_time,duration", "-of", "csv=p=0", file]).toString();
    const [width, height, start, duration] = probe.trim().split("\n").filter(Boolean)[0].split(",").map(Number);
    assert.deepEqual([width, height], [320, 180], `${name}: the channel's size`);
    if (end !== null) assert.ok(Math.abs(start - end) < 0.1, `${name} starts where the one before ends: ${start} vs ${end}`);
    end = start + duration;
    colours.push(pixel(file, 10, 10));
  }
  // Bars (grey in the corner) in the middle, the test picture before and after.
  const grey = colours.map(([r, g, b]) => Math.abs(r - 191) < 12 && Math.abs(g - 191) < 12 && Math.abs(b - 191) < 12);
  const first = grey.indexOf(true);
  const last = grey.lastIndexOf(true);
  assert.ok(first > 0 && last < grey.length - 1, `live in the middle: ${JSON.stringify(colours)}\n${log}\n${Buffer.concat(listenerLog)}\n${playlist}\n${cg.logs()}`);
  assert.ok(grey.slice(first, last + 1).every(Boolean), "one live stretch");
});
