// Sidecar end to end as a black box (sidecar.mjs: target/release/virtrs-cg or
// VIRTRS_CG_BIN): fake VizRS (TCP), the real script client (sidecar.mjs) and
// ffmpeg overlay (target/release/vizrs-ffmpeg or VIZRS_FFMPEG_BIN). The browser exits
// at once, so captures send transparent frames; graphics content is tested
// with a real Chromium in chromium.test.mjs.

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { chmodSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import http from "node:http";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";

import { blankYuva, readFrames, yuvaSize } from "./helpers.mjs";
import { CLIENT, runClient, startSidecar } from "./sidecar.mjs";

const WRAPPER = process.env.VIZRS_FFMPEG_BIN || fileURLToPath(new URL("../../target/release/vizrs-ffmpeg", import.meta.url));

/** VizRS stand-in: records commands, `STATE` returns `layers`. */
async function fakeVizrs(layers = []) {
  const received = [];
  const sockets = new Set();
  const server = net.createServer((socket) => {
    sockets.add(socket);
    socket.on("close", () => sockets.delete(socket));
    let buffer = "";
    socket.setEncoding("utf8");
    socket.on("data", (chunk) => {
      buffer += chunk;
      let index;
      while ((index = buffer.indexOf("\n")) >= 0) {
        const line = buffer.slice(0, index);
        buffer = buffer.slice(index + 1);
        received.push(line);
        socket.write(line === "STATE" ? `OK ${JSON.stringify({ layers })}\n` : "OK {}\n");
      }
    });
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const close = () => {
    for (const socket of sockets) socket.destroy();
    server.close();
  };
  return { received, address: `127.0.0.1:${server.address().port}`, close };
}

/** Sidecar for one test, stopped after it. */
async function sidecar(t, config, env) {
  const cg = await startSidecar(config, env);
  t.after(cg.stop);
  return cg;
}

function listen(server) {
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server.address().port)));
}

const item = (path, start, extra = {}) => ({
  Resolution: { Width: 8, Height: 4 },
  FrameRate: 25,
  MediaItem_Path: path,
  MediaItem_Title: path.split("/").pop().replace(".mp4", ""),
  MediaItem_Start: start,
  MediaItem_Stop: start.replace(":00:00", ":30:00"),
  ...extra,
});

test("client streams frames paced by the clock, commands reach VizRS once per item", async (t) => {
  const vizrs = await fakeVizrs();
  t.after(vizrs.close);
  const cg = await sidecar(t, {
    vizrs: { address: vizrs.address, token: "secret" },
    on_clip: "TAKE 4001 headline={title}",
    lead_seconds: 0.4,
    // At once and never out, as before in/out_seconds (the items are in the past).
    in_seconds: 0,
    out_seconds: -1,
  });
  const url = `${cg.url}/etv?channel=1`;

  const stream = await runClient(url, item("/m/News.mp4", "2026-10-24T20:00:00+02:00"), 1000);
  const frames = readFrames(stream, 8, 4);

  // ~1 s at 25 fps plus 0.4 s lead, never more (paced, not a burst). The
  // client's start eats into the second, more on a busy host (Chromium tests
  // run in parallel), hence the low floor.
  assert.ok(frames.length >= 15 && frames.length <= 36, `frames: ${frames.length}`);
  assert.ok(frames.every((frame) => frame.every((b) => b === 0)), "commands only: transparent");

  // A second session on the same item (viewer joins, seek) sends nothing again.
  await runClient(url, item("/m/News.mp4", "2026-10-24T20:00:00+02:00", { MediaItem_StreamSeek: "00:05:00" }), 300);
  await sleep(100);
  assert.deepEqual(vizrs.received, ["AUTH secret", "TAKE 4001 headline=News"]);
  assert.equal((await cg.health()).sessions, 0, "every session ended");
});

test("channels: each channel commands its own VizRS, the others the main one", async (t) => {
  const [main, second] = [await fakeVizrs(), await fakeVizrs()];
  t.after(main.close);
  t.after(second.close);
  const cg = await sidecar(t, {
    vizrs: { address: main.address },
    on_clip: "TAKE 1 headline={title}",
    in_seconds: 0,
    out_seconds: -1,
    channels: { 2: { vizrs: { address: second.address }, on_clip: "TAKE 2 headline={title}", shows: [{ show_title: "Kino", on_clip: "TAKE 3" }] } },
  });

  await runClient(`${cg.url}/etv?channel=1`, item("/m/One.mp4", "2026-10-24T20:00:00+02:00"), 300);
  await runClient(`${cg.url}/etv?channel=2`, item("/m/Two.mp4", "2026-10-24T20:00:00+02:00"), 300);
  const show = { MediaItem_ShowTitle: "Kino" };
  await runClient(`${cg.url}/etv?channel=2`, item("/m/Film.mp4", "2026-10-24T21:00:00+02:00", show), 300);
  await runClient(`${cg.url}/etv?channel=1`, item("/m/Film.mp4", "2026-10-24T21:00:00+02:00", show), 300);
  await sleep(100);
  assert.deepEqual(main.received, ["TAKE 1 headline=One", "TAKE 1 headline=Film"]);
  assert.deepEqual(second.received, ["TAKE 2 headline=Two", "TAKE 3"]);
});

test("channel_buses: ErsatzTV's channel names reach VizRS once", async (t) => {
  const seen = [];
  const vizrsHttp = http.createServer((request, response) => {
    let body = "";
    request.on("data", (chunk) => (body += chunk));
    request.on("end", () => {
      seen.push([request.url, request.headers["x-token"], body]);
      response.writeHead(200).end("{}");
    });
  });
  t.after(() => vizrsHttp.close());
  const port = await listen(vizrsHttp);
  const cg = await sidecar(t, { vizrs: { address: "127.0.0.1:9", token: "s" }, channel_buses: true, output_url: `http://127.0.0.1:${port}/output` });
  const post = () => fetch(`${cg.url}/etv/channels`, { method: "POST", body: JSON.stringify([{ number: "4", name: "BTB Info" }]) });
  await post();
  await post();
  assert.deepEqual(seen, [["/api/channels/4", "s", '{"name":"BTB Info"}']]);
});

test("channel_buses: a new channel uses its own antenna in VizRS", async (t) => {
  const vizrs = await fakeVizrs([{ page: { number: 7 } }]);
  t.after(vizrs.close);
  const cg = await sidecar(t, { vizrs: { address: vizrs.address }, on_clip: "TAKE 1", in_seconds: 0, out_seconds: -1, channel_buses: true });
  await runClient(`${cg.url}/etv?channel=9`, item("/m/One.mp4", "2026-10-24T20:00:00+02:00"), 300);
  await sleep(100);
  assert.deepEqual(vizrs.received, ["@9 TAKE 1"]);
});

test("page elements: TAKE in_seconds after the item's start, OUT out_seconds before its stop", async (t) => {
  const vizrs = await fakeVizrs();
  t.after(vizrs.close);
  const cg = await sidecar(t, {
    vizrs: { address: vizrs.address, token: "" },
    on_clip: "TAKE 4001",
    lead_seconds: 0,
    in_seconds: 0.8,
  }, { VIZRS_OUT_SECONDS: "0.3" });
  const now = Date.now();
  const timed = (path, length) => ({
    ...item(path, new Date(now).toISOString()),
    MediaItem_Stop: new Date(now + length).toISOString(),
  });
  // Client with &take=7303
  await runClient(`${cg.url}/etv?channel=1&take=7303`, timed("/m/Clip1.mp4", 2500), 100);
  // Client with &page=1001&fields=headline={title}
  await runClient(`${cg.url}/etv?channel=1&page=1001&fields=headline={title}`, timed("/m/Clip2.mp4", 2500), 100);
  assert.deepEqual(vizrs.received, [], "nothing before in_seconds");
  // Items shorter than in + out never go on air.
  await runClient(`${cg.url}/etv?channel=1&take=5002`, timed("/m/Short.mp4", 500), 100);
  await sleep(now + 1300 - Date.now());
  assert.deepEqual([...vizrs.received].sort(), ["TAKE 1001 headline=Clip2", "TAKE 7303"]);
  await sleep(now + 2500 - Date.now());
  assert.deepEqual(vizrs.received.slice(2).sort(), ["OUT 1001", "OUT 7303"]);
});

test("page elements follow the VizRS show through the client, on the client's channel", async (t) => {
  let pages = [{ number: 3001, name: "Ticker" }, { number: 4001, name: "Tytuł" }];
  const tokens = [];
  const vizrsHttp = http.createServer((request, response) => {
    tokens.push(request.headers["x-token"]);
    if (request.url !== "/api/show") return response.writeHead(404).end();
    response.writeHead(200, { "Content-Type": "application/json" }).end(JSON.stringify({ name: "Show", pages }));
  });
  t.after(() => vizrsHttp.close());
  const port = await listen(vizrsHttp);
  const vizrs = await fakeVizrs();
  t.after(vizrs.close);
  const cg = await sidecar(t, { vizrs: { address: vizrs.address, token: "secret" } }, { VIZRS_OUTPUT_URL: `http://127.0.0.1:${port}/output?token=out` });

  const config = mkdtempSync(path.join(os.tmpdir(), "etv-config-"));
  t.after(() => rmSync(config, { recursive: true, force: true }));
  const dir = path.join(config, "templates/graphics-elements/script");
  mkdirSync(dir, { recursive: true });
  const session = async (start) => {
    const url = `${cg.url}/etv?channel=4&render=720p`;
    await runClient(url, item("/m/Clip.mp4", start), 300, { ETV_CONFIG_FOLDER: config });
  };

  await session("2026-10-25T12:00:00Z");
  assert.deepEqual(readdirSync(dir).sort(), ["page-3001.yml", "page-4001.yml"]);
  assert.match(readFileSync(path.join(dir, "page-3001.yml"), "utf8"), /name: "Ticker \(3001\)"[^]*channel=4&render=720p&page=3001/);
  assert.equal(tokens[0], "secret", "the operator's token, not the one of /output");

  pages = [{ number: 3001, name: "Pasek" }, { number: 5001, name: "Wynik" }];
  await session("2026-10-25T13:00:00Z");
  assert.deepEqual(readdirSync(dir).sort(), ["page-3001.yml", "page-4001.yml", "page-5001.yml"]);
  assert.match(readFileSync(path.join(dir, "page-3001.yml"), "utf8"), /name: "Pasek \(3001\)"/);
  assert.match(readFileSync(path.join(dir, "page-4001.yml"), "utf8"), /name: "\[brak w VizRS\] Tytuł \(4001\)"/);
});

test("page elements: no VizRS, no change to the folder and frames as usual", async (t) => {
  const vizrs = await fakeVizrs();
  t.after(vizrs.close);
  const cg = await sidecar(t, { vizrs: { address: vizrs.address, token: "" } }, { VIZRS_OUTPUT_URL: "http://127.0.0.1:9/output" });
  assert.equal((await fetch(`${cg.url}/etv/pages`)).status, 502);

  const config = mkdtempSync(path.join(os.tmpdir(), "etv-config-"));
  t.after(() => rmSync(config, { recursive: true, force: true }));
  const dir = path.join(config, "templates/graphics-elements/script");
  mkdirSync(dir, { recursive: true });
  writeFileSync(path.join(dir, "page-3001.yml"), "# Generated from VizRS CG System page 3001\nname: \"Ticker (3001)\"\n");
  const frames = await runClient(`${cg.url}/etv?channel=1`, item("/m/Clip.mp4", "2026-10-25T12:00:00Z"), 300, { ETV_CONFIG_FOLDER: config });
  assert.ok(frames.length > 0);
  assert.deepEqual(readdirSync(dir), ["page-3001.yml"]);
});

test("two VizRS elements on one piece: one session carries the graphics, the other is transparent", async (t) => {
  const vizrs = await fakeVizrs();
  t.after(vizrs.close);
  const cg = await sidecar(t, {
    vizrs: { address: vizrs.address, token: "" },
    output_url: "http://127.0.0.1:9/output",
    lead_seconds: 0,
  });
  const clip = item("/m/News.mp4", "2026-10-24T20:00:00+02:00");
  const generic = runClient(`${cg.url}/etv?channel=1`, clip, 800);
  const ticker = runClient(`${cg.url}/etv?channel=1&page=3003`, clip, 800);
  await sleep(300);
  const claim = () => fetch(`${cg.url}/ffmpeg?channel=1&wait=0`).then((r) => r.status);
  assert.equal(await claim(), 200);
  assert.equal(await claim(), 404, "only one session of the piece has frames");
  await Promise.all([generic, ticker]);
  // The next piece (another seek) gets a carrier again.
  const next = runClient(`${cg.url}/etv?channel=1&page=3003`, { ...clip, MediaItem_StreamSeek: "00:00:44" }, 600);
  await sleep(300);
  assert.equal(await claim(), 200);
  await next;
});

test("a waiting overlay gets only the session with frames, not the other element of the piece", async (t) => {
  const vizrs = await fakeVizrs();
  t.after(vizrs.close);
  const cg = await sidecar(t, {
    vizrs: { address: vizrs.address, token: "" },
    output_url: "http://127.0.0.1:9/output",
    lead_seconds: 0,
  });
  const clip = item("/m/News.mp4", "2026-10-24T20:00:00+02:00");
  const generic = runClient(`${cg.url}/etv?channel=1`, clip, 1500);
  await sleep(300);
  assert.equal((await fetch(`${cg.url}/ffmpeg?channel=1&wait=0`)).status, 200);
  // A second overlay waits; the piece's other element arrives without frames.
  const waiting = fetch(`${cg.url}/ffmpeg?channel=1&wait=800`).then((r) => r.status);
  await sleep(100);
  const ticker = runClient(`${cg.url}/etv?channel=1&page=3003`, clip, 1000);
  assert.equal(await waiting, 404, "a session without frames is never handed over");
  await Promise.all([generic, ticker]);
});

test("render size: config default, per-channel override, one capture per size pair", async (t) => {
  const vizrs = await fakeVizrs();
  t.after(vizrs.close);
  const cg = await sidecar(t, {
    vizrs: { address: vizrs.address, token: "" },
    output_url: "http://127.0.0.1:9/output",
    lead_seconds: 0,
  }, { VIZRS_RENDER_SIZE: "720p" });
  const hd = { ...item("/m/News.mp4", "2026-10-24T20:00:00+02:00"), Resolution: { Width: 1920, Height: 1080 } };

  await runClient(`${cg.url}/etv?channel=1`, hd, 400);
  await runClient(`${cg.url}/etv?channel=2`, hd, 400);
  await runClient(`${cg.url}/etv?channel=3&render=auto`, hd, 400);
  await runClient(`${cg.url}/etv?channel=4&render=bogus`, hd, 400);

  const { captures } = await cg.health();
  assert.deepEqual(captures.map(({ size, render }) => [size, render]), [
    ["1920x1080", "1280x720"],
    ["1920x1080", "1920x1080"],
  ]);
});

test("capture rate: follows ErsatzTV's frame rate up and down, with the media", async (t) => {
  const cg = await sidecar(t, { vizrs: { address: "127.0.0.1:9", token: "" }, output_url: "http://127.0.0.1:9/output", lead_seconds: 0 });
  const rate = async () => (await cg.health()).captures[0].fps;
  const clip = (fps, hour) => ({ ...item(`/m/${fps}.mp4`, `2026-10-24T${hour}:00:00+02:00`), FrameRate: fps });
  await runClient(`${cg.url}/etv?channel=1`, clip(25, "20"), 200);
  assert.equal(await rate(), 25);

  // Overlapping items: the higher rate while both run, then the rate of the one left.
  const slow = runClient(`${cg.url}/etv?channel=1`, clip(25, "21"), 900);
  await sleep(200);
  const fast = runClient(`${cg.url}/etv?channel=1`, clip(60, "22"), 300);
  await sleep(150);
  assert.equal(await rate(), 60);
  await fast;
  await sleep(100);
  assert.equal(await rate(), 25);
  await slow;
});

test("capture mode: frames by default, file, environment on top, a bad value falls back to frames", async (t) => {
  const hd = { ...item("/m/News.mp4", "2026-10-24T20:00:00+02:00"), Resolution: { Width: 1920, Height: 1080 } };
  const modes = [];
  for (const [file, env] of [["", {}], ["screencast", {}], ["screencast", { VIZRS_CAPTURE_MODE: "frames" }], ["bogus", {}]]) {
    const cg = await sidecar(t, { vizrs: { address: "127.0.0.1:9", token: "" }, output_url: "http://127.0.0.1:9/output", lead_seconds: 0, capture_mode: file }, env);
    await runClient(`${cg.url}/etv?channel=1`, hd, 300);
    modes.push((await cg.health()).captures[0].mode);
  }
  assert.deepEqual(modes, ["frames", "screencast", "frames", "frames"]);
});

test("client exits quietly when the sidecar is down", async () => {
  const stream = await runClient("http://127.0.0.1:9/etv", {}, 3000);
  assert.equal(stream.length, 0);
});

test("ads take graphics out; the first clip after the break takes them back", async (t) => {
  const vizrs = await fakeVizrs([{ layer: "main", page: "1001" }, { layer: "logo", page: "3001" }]);
  t.after(vizrs.close);
  const cg = await sidecar(t, {
    vizrs: { address: vizrs.address, token: "" },
    on_clip: "UPDATE 2001 t={title}",
    on_ad: "",
    ad_match: { path: "/reklamy/" },
    ad_without_element: false,
    in_seconds: 0,
  });
  const url = `${cg.url}/etv?channel=1`;

  await runClient(url, item("/reklamy/a.mp4", "2026-10-24T20:00:00+02:00"), 300);
  await runClient(url, item("/reklamy/b.mp4", "2026-10-24T21:00:00+02:00"), 300);
  await runClient(url, item("/m/film.mp4", "2026-10-24T22:00:00+02:00"), 300);
  await sleep(200);

  assert.deepEqual(vizrs.received, [
    "STATE", "OUT main,logo",
    "STATE", "OUT main,logo",
    "TAKE 1001,3001", "UPDATE 2001 t=film",
  ]);
});

test("an item without the VizRS element is an ad; the next item with it restores", async (t) => {
  const vizrs = await fakeVizrs([{ layer: "main", page: "1001" }]);
  t.after(vizrs.close);
  const cg = await sidecar(t, {
    vizrs: { address: vizrs.address, token: "" },
    on_clip: "UPDATE 2001 t={title}",
    on_ad: "",
    ad_without_element: true,
    gap_grace_ms: 50,
    in_seconds: 0,
  });
  const url = `${cg.url}/etv?channel=1`;
  const at = (ms) => new Date(Date.now() + ms).toISOString();
  const clip = (name, start, stop) => ({ ...item(`/m/${name}.mp4`, start), MediaItem_Stop: stop });

  // Overlapping items with the element (b starts before a ends): no ad in between.
  const a = runClient(url, clip("a", "2026-10-24T20:00:00+02:00", at(0)), 600);
  await sleep(300);
  const b = runClient(url, clip("b", "2026-10-24T20:30:00+02:00", at(600)), 600);
  await Promise.all([a, b]);
  await sleep(500);
  assert.deepEqual(vizrs.received, ["UPDATE 2001 t=a", "UPDATE 2001 t=b", "STATE", "OUT main"]);

  await runClient(url, clip("c", "2026-10-24T21:00:00+02:00", at(10000)), 300);
  await sleep(100);
  assert.deepEqual(vizrs.received.slice(4), ["TAKE 1001", "UPDATE 2001 t=c"]);
});

// ffmpeg stand-in: prints its arguments and, for a `pipe:N` input, how many
// bytes it read (FRAMES frames of FRAME_BYTES) and the first frame in hex.
const FAKE_FFMPEG = `#!/usr/bin/env node
const { readSync } = require("node:fs");
const args = process.argv.slice(2);
const result = { args };
for (const arg of args) {
  const fd = arg.startsWith("pipe:") ? Number(arg.slice(5)) : -1;
  if (fd <= 2) continue;
  const frame = Number(process.env.FRAME_BYTES);
  const data = Buffer.alloc(frame * Number(process.env.FRAMES));
  let read = 0;
  while (read < data.length) {
    let n;
    try { n = readSync(fd, data, read, data.length - read, null); } catch (e) { if (e.code === "EAGAIN") continue; throw e; }
    if (n === 0) break;
    read += n;
  }
  result.read = read;
  result.first = data.subarray(0, Math.min(frame, read)).toString("hex");
}
process.stdout.write(JSON.stringify(result) + "\\n");
`;

/** Runs the wrapper like ErsatzTV runs ffmpeg; resolves with the stand-in's report. */
function runWrapper(args, env) {
  return new Promise((resolve, reject) => {
    const child = spawn(WRAPPER, args, { env: { ...process.env, ...env } });
    const out = [];
    const err = [];
    child.stdout.on("data", (chunk) => out.push(chunk));
    child.stderr.on("data", (chunk) => err.push(chunk));
    child.on("error", reject);
    child.on("exit", (code) => {
      try {
        resolve({ code, stderr: Buffer.concat(err).toString(), ...JSON.parse(Buffer.concat(out).toString()) });
      } catch (error) {
        reject(new Error(`${error.message}: ${Buffer.concat(err)}`));
      }
    });
  });
}

test("ffmpeg overlay: raw frames from the sidecar, the script session stops its packets", async (t) => {
  const vizrs = await fakeVizrs();
  t.after(vizrs.close);
  const cg = await sidecar(t, {
    vizrs: { address: vizrs.address, token: "" },
    output_url: "http://127.0.0.1:9/output",
    on_clip: "TAKE 4001 headline={title}",
    lead_seconds: 0.4,
    // At once and never out, as before in/out_seconds (the items are in the past).
    in_seconds: 0,
    out_seconds: -1,
  });

  // ErsatzTV stand-in: the canvas request starts the script element with the item's data.
  const canvas = { requests: [], packetBytes: 0 };
  const etv = http.createServer((request, response) => {
    canvas.requests.push(request.headers);
    const child = spawn(CLIENT, [`${cg.url}/etv?channel=1`]);
    child.stdout.on("data", (chunk) => { canvas.packetBytes += chunk.length; });
    child.stdin.end(JSON.stringify(item("/m/News.mp4", "2026-10-24T20:00:00+02:00")));
    response.writeHead(200);
    response.flushHeaders();
    response.on("close", () => child.kill());
  });
  const etvPort = await listen(etv);
  t.after(() => etv.close());

  const dir = mkdtempSync(path.join(os.tmpdir(), "virtrs-ffmpeg-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const ffmpeg = path.join(dir, "ffmpeg");
  writeFileSync(`${ffmpeg}.js`, FAKE_FFMPEG);
  // awk records the signal mask ffmpeg starts with (Node and dash clear their own), then runs the stand-in.
  writeFileSync(`${ffmpeg}.awk`, `BEGIN {
  while ((getline line < "/proc/self/status") > 0) if (sub(/^SigBlk:[ \\t]*/, "", line)) print line > "${ffmpeg}.blocked"
  close("${ffmpeg}.blocked")
  command = "exec node '${ffmpeg}.js'"
  for (i = 2; i < ARGC; i++) { arg = ARGV[i]; gsub(/'/, "'\\\\''", arg); command = command " '" arg "'" }
  exit system(command)
}
`);
  writeFileSync(ffmpeg, `#!/usr/bin/env -S awk -f ${ffmpeg}.awk --\n`);
  chmodSync(ffmpeg, 0o755);

  const headers = "x-etv-channel:1\r\nx-etv-offset-ms:0\r\nx-etv-duration-ms:44000\r\nx-etv-frame-rate:25/1\r\n";
  const args = [
    "-threads", "1", "-i", "/m/News.mp4",
    "-protocol_whitelist", "file,http", "-headers", headers, "-t", "44000ms",
    "-i", `http://127.0.0.1:${etvPort}/internal/graphics/1/28750?exp=1&sig=x`,
    "-filter_complex", "[1:0]format=yuva420p[g]", "-f", "null", "-",
  ];
  const env = { VIZRS_FFMPEG: ffmpeg, VIZRS_CG_URL: cg.url, FRAME_BYTES: String(yuvaSize(8, 4)), FRAMES: "3" };

  const first = await runWrapper(args, env);
  const pipe = first.args.find((arg) => arg.startsWith("pipe:"));
  assert.deepEqual(first.args, [
    "-threads", "1", "-i", "/m/News.mp4",
    "-t", "44000ms", "-f", "rawvideo", "-pix_fmt", "yuva420p", "-video_size", "8x4", "-framerate", "25/1", "-colorspace", "smpte170m", "-color_range", "tv", "-i", pipe,
    "-filter_complex", "[1:0]format=yuva420p[g]", "-f", "null", "-",
  ]);
  assert.equal(first.read, 3 * yuvaSize(8, 4));
  assert.equal(first.first, blankYuva(8, 4).toString("hex"));
  const blocked = BigInt(`0x${readFileSync(`${ffmpeg}.blocked`, "utf8").trim()}`);
  const stop = (1n << 1n) | (1n << 14n); // SIGINT, SIGTERM
  assert.equal(blocked & stop, 0n, "ffmpeg starts with SIGINT and SIGTERM unblocked");
  assert.equal(canvas.requests[0]["x-etv-frame-rate"], "25/1", "the canvas is opened with ErsatzTV's headers");

  // The channel now runs through the overlay: its next script session sends ErsatzTV nothing.
  await sleep(300);
  const before = canvas.packetBytes;
  const second = await runWrapper(args, env);
  assert.equal(second.read, 3 * yuvaSize(8, 4));
  await sleep(300);
  assert.equal(canvas.packetBytes, before);
  assert.deepEqual(vizrs.received, ["TAKE 4001 headline=News"], "commands once, from the script session");

  // Sidecar down: the original command, unchanged.
  const fallback = await runWrapper(args, { ...env, VIZRS_CG_URL: "http://127.0.0.1:9" });
  assert.deepEqual(fallback.args, args);
  assert.match(fallback.stderr, /ErsatzTV graphics instead/);

  // Calls without the canvas go straight through.
  assert.deepEqual((await runWrapper(["-version"], env)).args, ["-version"]);
});

test("ffmpeg overlay at a forced rate, blending, preset and catch-up: the media rate converted to the channel rate, fps before the overlay, -r on the HLS output, framerate for faster media only, -preset:v after libx264, -readrate_catchup on HLS inputs", async (t) => {
  const vizrs = await fakeVizrs();
  t.after(vizrs.close);
  const cg = await sidecar(t, { vizrs: { address: vizrs.address, token: "" }, output_url: "http://127.0.0.1:9/output", lead_seconds: 0.4 });
  const etv = http.createServer((request, response) => {
    const child = spawn(CLIENT, [`${cg.url}/etv?channel=1`]);
    child.stdout.resume();
    child.stdin.end(JSON.stringify(item("/m/News.mp4", "2026-10-24T20:00:00+02:00")));
    response.writeHead(200);
    response.flushHeaders();
    response.on("close", () => child.kill());
  });
  const etvPort = await listen(etv);
  t.after(() => etv.close());
  const dir = mkdtempSync(path.join(os.tmpdir(), "virtrs-ffmpeg-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const ffmpeg = path.join(dir, "ffmpeg");
  writeFileSync(ffmpeg, FAKE_FFMPEG);
  chmodSync(ffmpeg, 0o755);
  // ffprobe next to ffmpeg: the media is 25 fps.
  const ffprobe = path.join(dir, "ffprobe");
  writeFileSync(ffprobe, "#!/bin/sh\nprintf 'r_frame_rate=25/1\\navg_frame_rate=25/1\\n'\n");
  chmodSync(ffprobe, 0o755);
  const env = {
    VIZRS_FFMPEG: ffmpeg, VIZRS_CG_URL: cg.url, VIZRS_FFMPEG_FPS: "50", VIZRS_FFMPEG_PRESET: "superfast", VIZRS_FFMPEG_READRATE_CATCHUP: "2",
    VIZRS_FFMPEG_FPS_BLEND: "1", FRAME_BYTES: String(yuvaSize(8, 4)), FRAMES: "2", TMPDIR: dir,
  };
  const hls = ["-g", "50", "-f", "hls", "-hls_time", "4", "/t/ffmpeg.m3u8"];
  const filter = "[0:0]fps=25,scale=8:4[v_m0];[1:0]format=yuva420p[v_s0];[v_m0][v_s0]overlay=x=0:y=0[v_o0]";

  const overlay = await runWrapper([
    "-i", "/m/News.mp4", "-headers", "x-etv-frame-rate:25/1\r\n",
    "-i", `http://127.0.0.1:${etvPort}/internal/graphics/1/7?exp=1&sig=x`,
    "-filter_complex", filter, "-map", "[v_o0]", "-vcodec", "libx264", ...hls,
  ], env);
  const pipe = overlay.args.find((arg) => arg.startsWith("pipe:"));
  assert.deepEqual(overlay.args, [
    "-i", "/m/News.mp4",
    "-f", "rawvideo", "-pix_fmt", "yuva420p", "-video_size", "8x4", "-framerate", "50", "-colorspace", "smpte170m", "-color_range", "tv", "-i", pipe,
    "-filter_complex", "[0:0]framerate=fps=50,scale=8:4[v_m0];[1:0]format=yuva420p[v_s0];[v_m0]fps=50[v_m0_fps];[v_m0_fps][v_s0]overlay=x=0:y=0[v_o0]",
    "-map", "[v_o0]", "-vcodec", "libx264", "-preset:v", "superfast", "-g", "50", "-r", "50", "-f", "hls", "-hls_time", "4", "/t/ffmpeg.m3u8",
  ]);
  assert.equal(overlay.read, 2 * yuvaSize(8, 4));
  assert.equal((await cg.health()).captures[0].fps, 50, "the capture follows the overlay's rate");

  // The first piece probed the media in the background; the next pieces know
  // it is no faster than the channel: frames repeated, not blended.
  const rates = path.join(dir, "vizrs-ffmpeg-rates");
  for (let i = 0; i < 100 && !readdirSync(rates).some((f) => readFileSync(path.join(rates, f), "utf8").includes("avg_frame_rate")); i++) await sleep(20);
  const media = ["-i", "/m/News.mp4", "-filter_complex", "[0:0]fps=25,scale=8:4[v]", "-map", "[v]", "-vcodec", "libx264", ...hls];
  assert.deepEqual((await runWrapper(media, env)).args, [
    "-i", "/m/News.mp4", "-filter_complex", "[0:0]fps=50,scale=8:4[v]", "-map", "[v]", "-vcodec", "libx264", "-preset:v", "superfast", "-g", "50", "-r", "50", "-f", "hls", "-hls_time", "4", "/t/ffmpeg.m3u8",
  ]);

  // Items without the canvas: the output rate and the preset.
  assert.deepEqual(
    (await runWrapper(["-i", "/m/Ad.mp4", "-vcodec", "libx264", ...hls], env)).args,
    ["-i", "/m/Ad.mp4", "-vcodec", "libx264", "-preset:v", "superfast", "-g", "50", "-r", "50", "-f", "hls", "-hls_time", "4", "/t/ffmpeg.m3u8"],
  );
  // A preset of ErsatzTV's own stays.
  const own = ["-i", "/m/Ad.mp4", "-vcodec", "libx264", "-preset:v", "veryfast", "-f", "null", "-"];
  assert.deepEqual((await runWrapper(own, env)).args, own);
  // Not a transcode to HLS (the canvas encoder, copies): unchanged.
  const canvasEncoder = ["-f", "rawvideo", "-i", "pipe:0", "-c:v", "ffv1", "-f", "nut", "pipe:1"];
  assert.deepEqual((await runWrapper(canvasEncoder, env)).args, canvasEncoder);
  const copy = ["-i", "/m/News.mp4", "-c", "copy", ...hls];
  assert.deepEqual((await runWrapper(copy, env)).args, copy);
  // ErsatzTV's MPEG-TS stream, read from its own HLS: catches up after a stall.
  const playlist = "http://localhost:8409/iptv/channel/1.m3u8?mode=segmenter&subtitles=false";
  const ts = ["-readrate", "1.0", "-readrate_initial_burst", "6", "-i", playlist, "-map", "0", "-c", "copy", "-f", "mpegts", "pipe:1"];
  assert.deepEqual(
    (await runWrapper(ts, env)).args,
    ["-readrate", "1.0", "-readrate_catchup", "2", "-readrate_initial_burst", "6", "-i", playlist, "-map", "0", "-c", "copy", "-f", "mpegts", "pipe:1"],
  );
});

test("graphics area: frames of the area overlaid at its place; full frames when the graph cannot place them", async (t) => {
  const vizrs = await fakeVizrs();
  t.after(vizrs.close);
  const cg = await sidecar(t, { vizrs: { address: vizrs.address, token: "" }, output_url: "http://127.0.0.1:9/output", lead_seconds: 0.4 });
  // The element of channel 1 has the area in its URL, as in vizrs.yml.
  const etv = http.createServer((request, response) => {
    const child = spawn(CLIENT, [`${cg.url}/etv?channel=1&area=4x2%2B4%2B2`]);
    child.stdout.resume();
    child.stdin.end(JSON.stringify(item("/m/News.mp4", "2026-10-24T20:00:00+02:00")));
    response.writeHead(200);
    response.flushHeaders();
    response.on("close", () => child.kill());
  });
  const etvPort = await listen(etv);
  t.after(() => etv.close());
  const dir = mkdtempSync(path.join(os.tmpdir(), "virtrs-ffmpeg-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const ffmpeg = path.join(dir, "ffmpeg");
  writeFileSync(ffmpeg, FAKE_FFMPEG);
  chmodSync(ffmpeg, 0o755);
  const canvas = ["-headers", "x-etv-frame-rate:25/1\r\n", "-i", `http://127.0.0.1:${etvPort}/internal/graphics/1/7?exp=1&sig=x`];
  const run = (filter, size) => runWrapper(
    ["-i", "/m/News.mp4", ...canvas, "-filter_complex", filter, "-map", "[v_o0]", "-f", "hls", "/t/ffmpeg.m3u8"],
    { VIZRS_FFMPEG: ffmpeg, VIZRS_CG_URL: cg.url, FRAME_BYTES: String(yuvaSize(...size)), FRAMES: "2" },
  );

  const placed = await run("[1:0]format=yuva420p[v_s0];[0:0][v_s0]overlay=x=0:y=0[v_o0]", [4, 2]);
  const pipe = placed.args.find((arg) => arg.startsWith("pipe:"));
  assert.deepEqual(placed.args, [
    "-i", "/m/News.mp4",
    "-f", "rawvideo", "-pix_fmt", "yuva420p", "-video_size", "4x2", "-framerate", "25/1", "-colorspace", "smpte170m", "-color_range", "tv", "-i", pipe,
    "-filter_complex", "[1:0]format=yuva420p[v_s0];[0:0][v_s0]overlay=x=4:y=2[v_o0]", "-map", "[v_o0]", "-f", "hls", "/t/ffmpeg.m3u8",
  ]);
  assert.equal(placed.read, 2 * yuvaSize(4, 2));
  assert.equal((await cg.health()).captures[0].area, "4x2+4+2");

  // Graphics scaled on the way: frames of the whole channel, the graph unchanged.
  await sleep(300);
  const whole = await run("[1:0]scale=8:4,format=yuva420p[v_s0];[0:0][v_s0]overlay=x=0:y=0[v_o0]", [8, 4]);
  assert.ok(whole.args.includes("8x4"));
  assert.ok(whole.args.includes("[1:0]scale=8:4,format=yuva420p[v_s0];[0:0][v_s0]overlay=x=0:y=0[v_o0]"));
  assert.equal(whole.read, 2 * yuvaSize(8, 4));
});

test("two graphics areas: stacked in one frame, split, cropped and overlaid each at its place", async (t) => {
  const vizrs = await fakeVizrs();
  t.after(vizrs.close);
  const cg = await sidecar(t, { vizrs: { address: vizrs.address, token: "" }, output_url: "http://127.0.0.1:9/output", lead_seconds: 0.4 });
  const etv = http.createServer((request, response) => {
    const child = spawn(CLIENT, [`${cg.url}/etv?channel=1&area=4x2+0+2,2x2+6+0`]);
    child.stdout.resume();
    child.stdin.end(JSON.stringify(item("/m/News.mp4", "2026-10-24T20:00:00+02:00")));
    response.writeHead(200);
    response.flushHeaders();
    response.on("close", () => child.kill());
  });
  const etvPort = await listen(etv);
  t.after(() => etv.close());
  const dir = mkdtempSync(path.join(os.tmpdir(), "virtrs-ffmpeg-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const ffmpeg = path.join(dir, "ffmpeg");
  writeFileSync(ffmpeg, FAKE_FFMPEG);
  chmodSync(ffmpeg, 0o755);

  const overlay = await runWrapper([
    "-i", "/m/News.mp4", "-headers", "x-etv-frame-rate:25/1\r\n",
    "-i", `http://127.0.0.1:${etvPort}/internal/graphics/1/7?exp=1&sig=x`,
    "-filter_complex", "[1:0]format=yuva420p[v_s0];[0:0][v_s0]overlay=x=0:y=0[v_o0]", "-map", "[v_o0]", "-f", "hls", "/t/ffmpeg.m3u8",
  ], { VIZRS_FFMPEG: ffmpeg, VIZRS_CG_URL: cg.url, FRAME_BYTES: String(yuvaSize(4, 4)), FRAMES: "2" });
  const pipe = overlay.args.find((arg) => arg.startsWith("pipe:"));
  // The strip (4x2) on rows 0-1 of the frame, the corner (2x2) on rows 2-3.
  assert.deepEqual(overlay.args, [
    "-i", "/m/News.mp4",
    "-f", "rawvideo", "-pix_fmt", "yuva420p", "-video_size", "4x4", "-framerate", "25/1", "-colorspace", "smpte170m", "-color_range", "tv", "-i", pipe,
    "-filter_complex",
    "[1:0]split=2[vz_s0][vz_s1];[vz_s0]crop=4:2:0:0,format=yuva420p[vz_g0];[vz_s1]crop=2:2:0:2,format=yuva420p[vz_g1];"
      + "[0:0][vz_g0]overlay=x=0:y=2[vz_o0];[vz_o0][vz_g1]overlay=x=6:y=0[v_o0]",
    "-map", "[v_o0]", "-f", "hls", "/t/ffmpeg.m3u8",
  ]);
  assert.equal(overlay.read, 2 * yuvaSize(4, 4));
  assert.equal((await cg.health()).captures[0].area, "4x2+0+2,2x2+6+0");
});

test("ffmpeg overlay with hardware acceleration: ErsatzTV's NVIDIA and VAAPI graphs", async (t) => {
  const vizrs = await fakeVizrs();
  t.after(vizrs.close);
  const cg = await sidecar(t, { vizrs: { address: vizrs.address, token: "" }, output_url: "http://127.0.0.1:9/output", lead_seconds: 0.4 });
  const etv = http.createServer((request, response) => {
    const child = spawn(CLIENT, [`${cg.url}/etv?channel=1`]);
    child.stdout.resume();
    child.stdin.end(JSON.stringify(item("/m/News.mp4", "2026-10-24T20:00:00+02:00")));
    response.writeHead(200);
    response.flushHeaders();
    response.on("close", () => child.kill());
  });
  const etvPort = await listen(etv);
  t.after(() => etv.close());
  const dir = mkdtempSync(path.join(os.tmpdir(), "virtrs-ffmpeg-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const ffmpeg = path.join(dir, "ffmpeg");
  writeFileSync(ffmpeg, FAKE_FFMPEG);
  chmodSync(ffmpeg, 0o755);
  const env = {
    VIZRS_FFMPEG: ffmpeg, VIZRS_CG_URL: cg.url, VIZRS_FFMPEG_FPS: "50",
    FRAME_BYTES: String(yuvaSize(8, 4)), FRAMES: "2",
  };

  // Filter graphs as ErsatzTV builds them (hardware input options stay with the media input).
  const cases = [
    {
      input: ["-init_hw_device", "cuda", "-hwaccel", "cuda", "-hwaccel_output_format", "cuda"],
      filter: "[0:0]hwupload,scale_cuda=8:4:format=yuv420p,setsar=1[v_m0];[1:0]format=yuva420p,hwupload_cuda[v_s0];[v_m0][v_s0]overlay_cuda=x=0:y=0[v_o0]",
      expected: "[0:0]hwupload,scale_cuda=8:4:format=yuv420p,setsar=1[v_m0];[1:0]format=yuva420p,hwupload_cuda[v_s0];[v_m0]fps=50[v_m0_fps];[v_m0_fps][v_s0]overlay_cuda=x=0:y=0[v_o0]",
      encoder: "h264_nvenc",
    },
    {
      input: ["-hwaccel", "vaapi", "-hwaccel_output_format", "vaapi", "-vaapi_device", "/dev/dri/renderD128"],
      filter: "[0:0]scale_vaapi=8:4:format=nv12,hwdownload,format=nv12[v_m0];[1:0]format=yuva420p[v_s0];[v_m0][v_s0]overlay=format=0,format=nv12|p010le|vaapi,hwupload[v_o0]",
      expected: "[0:0]scale_vaapi=8:4:format=nv12,hwdownload,format=nv12[v_m0];[1:0]format=yuva420p[v_s0];[v_m0]fps=50[v_m0_fps];[v_m0_fps][v_s0]overlay=format=0,format=nv12|p010le|vaapi,hwupload[v_o0]",
      encoder: "h264_vaapi",
    },
    {
      input: ["-hwaccel", "vaapi", "-hwaccel_output_format", "vaapi", "-vaapi_device", "/dev/dri/renderD128"],
      filter: "[0:0]scale_vaapi=8:4:format=nv12[v_m0];[1:0]format=yuva420p,hwupload[v_s0];[v_m0][v_s0]overlay_vaapi[v_o0]",
      expected: "[0:0]scale_vaapi=8:4:format=nv12[v_m0];[1:0]format=yuva420p,hwupload[v_s0];[v_m0]fps=50[v_m0_fps];[v_m0_fps][v_s0]overlay_vaapi[v_o0]",
      encoder: "h264_vaapi",
    },
  ];
  const hls = ["-g", "50", "-f", "hls", "-hls_time", "4", "/t/ffmpeg.m3u8"];
  for (const { input, filter, expected, encoder } of cases) {
    const result = await runWrapper([
      ...input, "-i", "/m/News.mp4", "-headers", "x-etv-frame-rate:25/1\r\n", "-t", "44000ms",
      "-i", `http://127.0.0.1:${etvPort}/internal/graphics/1/7?exp=1&sig=x`,
      "-filter_complex", filter, "-map", "[v_o0]", "-vcodec", encoder, ...hls,
    ], env);
    const pipe = result.args.find((arg) => arg.startsWith("pipe:"));
    assert.deepEqual(result.args, [
      ...input, "-i", "/m/News.mp4", "-t", "44000ms",
      "-f", "rawvideo", "-pix_fmt", "yuva420p", "-video_size", "8x4", "-framerate", "50", "-colorspace", "smpte170m", "-color_range", "tv", "-i", pipe,
      "-filter_complex", expected, "-map", "[v_o0]", "-vcodec", encoder, "-g", "50", "-r", "50", "-f", "hls", "-hls_time", "4", "/t/ffmpeg.m3u8",
    ], encoder);
    assert.equal(result.read, 2 * yuvaSize(8, 4), encoder);
  }
});
