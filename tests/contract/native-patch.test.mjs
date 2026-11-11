// `renderer: native` with patches: the sidecar asks for them (`p` in the
// frame line) and puts their pixels into the last picture of the renderer
// (VizRS DOKUMENTACJA §10.1, record 6). A fake renderer sends a red region
// first, then only patches: a green square inside the red box.

import assert from "node:assert/strict";
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { readFrames } from "./helpers.mjs";
import { runClient, startSidecar } from "./sidecar.mjs";

const FAKE = `#!/usr/bin/env node
const fs = require("fs");
const args = process.argv.slice(2);
const dir = process.env.FAKE_DIR;
const [w, h] = args[args.indexOf("--size") + 1].split("x").map(Number);
const u32s = (...v) => { const b = Buffer.alloc(v.length * 4); v.forEach((x, i) => b.writeUInt32LE(x, i * 4)); return b; };
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
    const want = line.split(" ")[3] || "";
    fs.appendFileSync(dir + "/flags", want + "\\n");
    if (n === 1 || want.includes("f") || !want.includes("p")) {
      // region 120x20 at (40, 100): red
      process.stdout.write(Buffer.concat([header(4), u32s(40, 100, 120, 20), Buffer.alloc(120 * 20 * 4, Buffer.from([255, 0, 0, 255]))]));
    } else {
      // patch: a green 10x10 square at (50, 105)
      process.stdout.write(Buffer.concat([header(6), u32s(1), u32s(50, 105, 10, 10), Buffer.alloc(10 * 10 * 4, Buffer.from([0, 255, 0, 255]))]));
    }
  }
});
`;

test("renderer native with patches: a patch changes only its pixels of the last picture", { timeout: 60000 }, async (t) => {
  const dir = mkdtempSync(join(tmpdir(), "virtrs-native-patch-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const bin = join(dir, "vizrs-render");
  writeFileSync(bin, FAKE);
  chmodSync(bin, 0o755);
  const cg = await startSidecar(
    { vizrs: { address: "127.0.0.1:9", token: "" }, output_url: "http://127.0.0.1:9/output", lead_seconds: 0, renderer: "native", renderer_bin: bin },
    { FAKE_DIR: dir },
  );
  t.after(cg.stop);
  const [width, height] = [320, 180];
  const data = { Resolution: { Width: width, Height: height }, FrameRate: 25, MediaItem_Path: "/m/News.mp4" };
  const last = readFrames(await runClient(`${cg.url}/etv?channel=1`, data, 2500), width, height).at(-1);
  const at = (px, py) => [...last.subarray((py * width + px) * 4, (py * width + px) * 4 + 4)];
  assert.deepEqual(at(55, 110), [0, 255, 0, 255], "the patch is on the picture");
  assert.deepEqual(at(100, 110), [255, 0, 0, 255], "the rest of the last picture stays");
  assert.deepEqual(at(20, 110), [0, 0, 0, 0]);
  const flags = readFileSync(join(dir, "flags"), "utf8").trim().split("\n");
  assert.ok(flags.every((f) => f.includes("p")), "every frame asks for patches");
});
