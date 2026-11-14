// Live API for VizRS Live (DOKUMENTACJA §1, Live API; src/panel.rs): the token
// (no page and no cookie), refusal of other sites, the live programmes wizard against a
// fake ErsatzTV (file, library scan, smart collection, guide), holding a
// channel through /ingest/state, and the first filling of shows.json from
// VIZRS_LIVE_SHOWS.

import assert from "node:assert/strict";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";

import { fakeVizrs } from "./helpers.mjs";
import { startSidecar } from "./sidecar.mjs";

const TOKEN = "panel-test";
const KEY = "etv-key";

/** ErsatzTV's API as the panel uses it; every call lands in `calls`. */
async function fakeErsatzTV() {
  const calls = [];
  const collections = [];
  let next = 40;
  const pad = (n) => String(n).padStart(2, "0");
  const xmltvTime = (ms) => {
    const d = new Date(ms);
    return `${d.getUTCFullYear()}${pad(d.getUTCMonth() + 1)}${pad(d.getUTCDate())}${pad(d.getUTCHours())}${pad(d.getUTCMinutes())}${pad(d.getUTCSeconds())} +0000`;
  };
  const server = http.createServer((req, res) => {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      calls.push({ method: req.method, url: req.url, key: req.headers["x-etv-api-key"], body });
      const json = (status, value) => {
        res.writeHead(status, { "Content-Type": "application/json" });
        res.end(value === undefined ? "" : JSON.stringify(value));
      };
      if (req.headers["x-etv-api-key"] !== KEY && req.url.startsWith("/api/")) return json(401);
      if (req.method === "GET" && req.url === "/api/channels") return json(200, [{ id: 1, number: "1", name: "BTB 24" }, { id: 2, number: "4", name: "BTB Info" }]);
      if (req.method === "GET" && req.url === "/api/collections/smart") return json(200, collections);
      if (req.method === "POST" && req.url === "/api/collections/smart/new") {
        const { name, query } = JSON.parse(body);
        const id = next++;
        collections.push({ id, name, query });
        return json(200, { id });
      }
      if (req.method === "PUT" && req.url === "/api/collections/smart/update") {
        const { id, name, query } = JSON.parse(body);
        const c = collections.find((x) => x.id === id);
        if (!c) return json(404);
        Object.assign(c, { name, query });
        return json(200, {});
      }
      const del = req.url.match(/^\/api\/collections\/smart\/delete\/(\d+)$/);
      if (req.method === "DELETE" && del) {
        const at = collections.findIndex((x) => x.id === Number(del[1]));
        if (at < 0) return json(404);
        collections.splice(at, 1);
        return json(200);
      }
      if (req.method === "POST" && /^\/api\/libraries\/\d+\/scan$/.test(req.url)) return json(200);
      if (req.method === "POST" && /^\/api\/channels\/[^/]+\/playout\/reset$/.test(req.url)) return json(req.url.includes("/9/") ? 404 : 200);
      if (req.method === "GET" && req.url === "/iptv/xmltv.xml") {
        const now = Date.now();
        const items = [
          ["C4.2.ersatztv.org", "Film &amp; serial", now - 600000, now + 600000],
          ["C4.2.ersatztv.org", "Rozmowa dnia", now + 600000, now + 3300000],
          ["C1.1.ersatztv.org", "Pogoda", now - 60000, now + 60000],
        ];
        res.writeHead(200, { "Content-Type": "application/xml" });
        return res.end(
          `﻿<?xml version="1.0" encoding="utf-8"?><tv>` +
            items.map(([ch, t, a, b]) => `<programme start="${xmltvTime(a)}" stop="${xmltvTime(b)}" channel="${ch}"><title lang="en">${t}</title></programme>`).join("") +
            "</tv>",
        );
      }
      json(404);
    });
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  return { url: `http://127.0.0.1:${server.address().port}`, calls, collections, close: () => new Promise((r) => server.close(r)) };
}

async function setup(t, env = {}, config = {}) {
  const etv = await fakeErsatzTV();
  t.after(etv.close);
  const folder = mkdtempSync(path.join(os.tmpdir(), "vizrs-etv-config-"));
  t.after(() => rmSync(folder, { recursive: true, force: true }));
  writeFileSync(path.join(folder, "api-secrets.json"), JSON.stringify({ ApiKey: KEY }));
  const cg = await startSidecar(
    { output_url: "", panel_token: TOKEN, ingest_token: "ing", etv_url: etv.url, etv_config: folder, sync_seconds: 3600, ...config },
    env,
  );
  t.after(cg.stop);
  const api = async (method, route, body, headers = {}) => {
    const r = await fetch(`${cg.url}/api/live/${route}`, {
      method,
      headers: { "X-Panel-Token": TOKEN, "X-Operator": "MCR 1", ...(body === undefined ? {} : { "Content-Type": "application/json" }), ...headers },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    return { status: r.status, body: await r.json() };
  };
  return { etv, folder, cg, api };
}

test("the live API is off without a token, needs it and has no page of its own", async (t) => {
  const off = await startSidecar({ output_url: "" });
  t.after(off.stop);
  assert.equal((await fetch(`${off.url}/api/live/state`)).status, 404);

  const { cg } = await setup(t);
  assert.equal((await fetch(`${cg.url}/live`)).status, 404, "the operator's page is VizRS Live");
  assert.equal((await fetch(`${cg.url}/live?token=${TOKEN}`)).status, 404);
  assert.equal((await fetch(`${cg.url}/api/live/state`)).status, 401);
  assert.equal((await fetch(`${cg.url}/api/live/state`, { headers: { Cookie: `vizrs_panel=${TOKEN}` } })).status, 401, "no cookie login");
  assert.equal((await fetch(`${cg.url}/api/live/state`, { headers: { "X-Panel-Token": "nope" } })).status, 401);
  assert.equal((await fetch(`${cg.url}/api/live/state`, { headers: { "X-Panel-Token": TOKEN } })).status, 200);
  assert.equal((await fetch(`${cg.url}/api/live/state`, { headers: { Authorization: `Bearer ${TOKEN}` } })).status, 200);
});

test("changes from other sites are refused", async (t) => {
  const { api } = await setup(t);
  const evil = await api("POST", "channels/1/hold", undefined, { Origin: "http://evil.example" });
  assert.equal(evil.status, 403);
  assert.equal((await api("POST", "channels/1/hold", undefined, { "Sec-Fetch-Site": "cross-site" })).status, 403);
  assert.equal((await api("GET", "state", undefined, { "Sec-Fetch-Site": "cross-site" })).status, 200);
});

test("a live programme from the panel is a file, a scan and a smart collection in ErsatzTV", async (t) => {
  const { etv, folder, api } = await setup(t);
  const missing = await api("POST", "shows", { channel: "4", title: "Rozmowa dnia", duration: "00:45:00" });
  assert.equal(missing.status, 200);
  assert.deepEqual(missing.body.steps.map((s) => [s.step, s.ok]), [["file", true], ["scan", false], ["collection", false]]);
  assert.equal((await api("DELETE", "shows/4-rozmowa-dnia")).status, 200);

  assert.equal((await api("PUT", "settings", { library: "x" })).status, 400);
  assert.equal((await api("PUT", "settings", { library: 7 })).status, 200);
  assert.deepEqual(JSON.parse(readFileSync(path.join(folder, "vizrs-live", "control.json"), "utf8")).library, 7);

  assert.equal((await api("POST", "shows", { channel: "4", title: "x".repeat(201), duration: 60 })).status, 400);
  assert.equal((await api("POST", "shows", { channel: "4", title: "A", duration: "25:00:00" })).status, 400);
  assert.equal((await api("POST", "shows", { channel: "4; x", title: "A", duration: 60 })).status, 400);

  const created = await api("POST", "shows", { channel: "4", title: "Rozmowa dnia", duration: "00:45:00", plot: "Gość w studiu" });
  assert.equal(created.status, 200);
  assert.deepEqual(created.body.steps.map((s) => [s.step, s.ok]), [["file", true], ["scan", true], ["collection", true]]);
  assert.equal(created.body.show.collection_name, "LIVE · Rozmowa dnia");
  const file = path.join(folder, "vizrs-live", "4-rozmowa-dnia.yml");
  assert.match(readFileSync(file, "utf8"), /title: "Rozmowa dnia"\nplot: "Gość w studiu"\nurl: "http:\/\/cg:7700\/ingest\?channel=4&slot=1"\nis_live: true\nduration: "00:45:00"/);
  assert.ok(etv.calls.some((c) => c.method === "POST" && c.url === "/api/libraries/7/scan" && c.key === KEY));
  assert.deepEqual(etv.collections, [{ id: 40, name: "LIVE · Rozmowa dnia", query: 'type:remote_stream AND library_id:7 AND sort_title:"rozmowa dnia"' }]);
  assert.equal(JSON.parse(readFileSync(path.join(folder, "vizrs-live", "shows.json"), "utf8")).shows[0].collection, 40);
  assert.equal((await api("POST", "shows", { channel: "4", title: "Rozmowa dnia", duration: 60 })).status, 409);

  const list = await api("GET", "shows");
  const show = list.body.shows[0];
  assert.equal(show.id, "4-rozmowa-dnia");
  assert.equal(show.collection, true);
  assert.equal(show.file_ok, true);
  assert.equal(show.next.title, "Rozmowa dnia");
  assert.equal(list.body.settings.library, 7);

  // A new title renames the collection; the file (the item in ErsatzTV) stays.
  const changed = await api("PUT", "shows/4-rozmowa-dnia", { channel: "4", title: "Rozmowa wieczoru", duration: "01:00:00", plot: "" });
  assert.equal(changed.status, 200);
  assert.deepEqual(etv.collections, [{ id: 40, name: "LIVE · Rozmowa wieczoru", query: 'type:remote_stream AND library_id:7 AND sort_title:"rozmowa wieczoru"' }]);
  assert.match(readFileSync(file, "utf8"), /title: "Rozmowa wieczoru"\nurl: .*\nis_live: true\nduration: "01:00:00"/);
  assert.equal((await api("PUT", "shows/nope", { channel: "4", title: "X", duration: 60 })).status, 404);

  // The collection was removed in ErsatzTV: „Odśwież” makes it again.
  etv.collections.length = 0;
  const scan = await api("POST", "shows/scan");
  assert.ok(scan.body.steps.every((s) => s.ok));
  assert.equal(etv.collections[0].name, "LIVE · Rozmowa wieczoru");

  assert.equal((await api("POST", "channels/4/rebuild")).status, 200);
  assert.ok(etv.calls.some((c) => c.method === "POST" && c.url === "/api/channels/4/playout/reset"));
  assert.equal((await api("POST", "channels/9/rebuild")).status, 502);

  const removed = await api("DELETE", "shows/4-rozmowa-dnia");
  assert.equal(removed.status, 200);
  assert.ok(!existsSync(file));
  assert.deepEqual(etv.collections, []);
  assert.deepEqual(JSON.parse(readFileSync(path.join(folder, "vizrs-live", "shows.json"), "utf8")).shows, []);
  const log = readFileSync(path.join(folder, "vizrs-live", "log.jsonl"), "utf8").trim().split("\n").map((l) => JSON.parse(l));
  assert.ok(log.some((l) => l.operator === "MCR 1" && l.action === "usunięta audycja „Rozmowa wieczoru”"));
});

test("holding a channel keeps its studio off and survives a restart", async (t) => {
  const { cg, api, folder } = await setup(t, {}, { ingest_anytime: true });
  const ingestState = async (url, ch) => (await fetch(`${url}/ingest/state?channel=${ch}`)).json();
  assert.deepEqual(await ingestState(cg.url, "4"), { enabled: true, anytime: true, hold: false, live: false, audio: false, video: null, seconds: null });

  assert.equal((await api("POST", "channels/4/hold")).status, 200);
  assert.equal((await ingestState(cg.url, "4")).hold, true);
  assert.equal((await api("PUT", "channels/4", { anytime: false })).status, 200);
  assert.equal((await ingestState(cg.url, "4")).anytime, false);
  assert.equal((await api("PUT", "channels/4", { anytime: "yes" })).status, 400);

  const state = (await api("GET", "state")).body;
  const ch = state.channels.find((c) => c.number === "4");
  assert.equal(ch.name, "BTB Info");
  assert.equal(ch.hold, true);
  assert.equal(ch.anytime, false);
  assert.equal(ch.now.title, "Film & serial");
  assert.equal(state.events[0].text, "MCR 1: studio tylko w audycjach na żywo");

  // A second sidecar on the same configuration starts with the hold.
  const again = await startSidecar({ output_url: "", panel_token: TOKEN, ingest_token: "ing", etv_config: folder, sync_seconds: 3600 });
  t.after(again.stop);
  assert.equal((await ingestState(again.url, "4")).hold, true);

  assert.equal((await api("POST", "channels/4/release")).status, 200);
  assert.equal((await ingestState(cg.url, "4")).hold, false);
});

test("the overlay's encoder: filled once from the environment, then set in the panel, checked, logged", async (t) => {
  const env = { VIZRS_FFMPEG_PRESET: "superfast", VIZRS_FFMPEG_X264_PARAMS: "", VIZRS_FFMPEG_NVENC_BF: "0" };
  const { api, folder } = await setup(t, env);
  const empty = { preset: "", profile: "", x264_params: "", nvenc_preset: "", qsv_preset: "", bframes: "" };

  const file = () => JSON.parse(readFileSync(path.join(folder, "vizrs-live", "encoder.json"), "utf8"));
  assert.deepEqual(file(), { ...empty, preset: "superfast", bframes: "0" }, "first filling from VIZRS_FFMPEG_PRESET and VIZRS_FFMPEG_NVENC_BF");
  const first = (await api("GET", "encoder")).body;
  assert.deepEqual(first.used, { ...empty, preset: "superfast", bframes: "0" });
  assert.equal(first.from, "panel");
  assert.ok(first.presets.includes("ultrafast") && first.x264_keys.includes("subme"));
  assert.deepEqual(first.profiles, ["baseline", "main", "high"]);
  assert.deepEqual(first.nvenc_presets, ["p1", "p2", "p3", "p4", "p5", "p6", "p7"]);
  assert.ok(first.qsv_presets.includes("veryslow") && first.max_bframes === 4);

  const want = { preset: "veryfast", profile: "high", x264_params: "bframes=0:subme=0", nvenc_preset: "p5", qsv_preset: "slow", bframes: "1" };
  const put = await api("PUT", "encoder", { ...want, profile: "High", x264_params: "bframes=0 : subme=0", bframes: 1 });
  assert.equal(put.status, 200);
  assert.deepEqual(put.body.used, want);
  assert.deepEqual(file(), want);

  assert.equal((await api("PUT", "encoder", { preset: "turbo" })).status, 400);
  assert.equal((await api("PUT", "encoder", { profile: "high10" })).status, 400);
  assert.equal((await api("PUT", "encoder", { x264_params: "keyint=1" })).status, 400);
  assert.equal((await api("PUT", "encoder", { x264_params: "stats=/tmp/x" })).status, 400);
  assert.equal((await api("PUT", "encoder", { nvenc_preset: "hq" })).status, 400);
  assert.equal((await api("PUT", "encoder", { qsv_preset: "ultrafast" })).status, 400);
  assert.equal((await api("PUT", "encoder", { bframes: "7" })).status, 400);
  assert.deepEqual(file(), want, "a refused change keeps the file");

  // Empty = the default, not the environment's; a restart does not fill the file again.
  assert.deepEqual((await api("PUT", "encoder", { preset: null, profile: "" })).body.used, empty);
  const again = await startSidecar({ output_url: "", panel_token: TOKEN, etv_config: folder, sync_seconds: 3600 }, env);
  t.after(again.stop);
  assert.deepEqual(file(), empty);
  const log = readFileSync(path.join(folder, "vizrs-live", "log.jsonl"), "utf8").trim().split("\n").map((l) => JSON.parse(l));
  assert.ok(log.some((l) => l.operator === "MCR 1" && l.action.includes("preset veryfast, profil high, opcje x264 bframes=0:subme=0, preset NVENC p5, preset QSV slow, klatki B sprzętowe 1")));
});

test("the overlay's encoder without a file: the environment, or the defaults", async (t) => {
  const { api } = await setup(t);
  const body = (await api("GET", "encoder")).body;
  assert.equal(body.from, "default");
  assert.deepEqual(body.used, { preset: "", profile: "", x264_params: "", nvenc_preset: "", qsv_preset: "", bframes: "" });
});

test("VIZRS_LIVE_SHOWS fills shows.json once", async (t) => {
  const env = { VIZRS_LIVE_SHOWS: JSON.stringify([{ channel: "1", title: "Wiadomości", duration: "00:30:00" }]) };
  const { folder, api } = await setup(t, env);
  const list = (await api("GET", "shows")).body;
  assert.deepEqual(list.shows.map((s) => [s.id, s.duration]), [["1-wiadomosci", "00:30:00"]]);
  assert.ok(existsSync(path.join(folder, "vizrs-live", "shows.json")));
});

test("the panel tells publishers where to send: RTMP server and key, SRT address and passphrase", async (t) => {
  const { api } = await setup(t, {
    VIZRS_INGEST: "4=rtmp://0.0.0.0:1936/live/studio; 4=srt://0.0.0.0:9000?passphrase=tajne-haslo-1; 7=rtmp://0.0.0.0:1937/in/drugi",
    VIZRS_INGEST_PUBLIC: "tv.example.pl",
    VIZRS_INGEST_RTMP_PORT: "1940",
  });
  const { body } = await api("GET", "state");
  const of = (n) => body.channels.find((c) => c.number === n).inputs;
  assert.deepEqual(of("4"), [
    { kind: "rtmp", host: "tv.example.pl", port: 1940, app: "/live", key: "studio", passphrase: "", streamid: "" },
    { kind: "srt", host: "tv.example.pl", port: 9000, app: "", key: "", passphrase: "tajne-haslo-1", streamid: "" },
  ]);
  assert.equal(of("7")[0].key, "drugi", "a channel with only a listener is listed too");
  assert.ok(body.channels.filter((c) => !["4", "7"].includes(c.number)).every((c) => c.inputs.length === 0), "other channels: no inputs");
});

// K12–K17 of VizRS Live's PLAN-mcr: the layers on air, TAKE / CONTINUE / OUT by
// hand, the as-run log and the alarm webhook. `features` of the state says which
// of them this build has, so an older sidecar never breaks a newer panel.
test("the state carries the layers on air and what this build can do", async (t) => {
  const vizrs = await fakeVizrs([{ layer: "main", page: { number: 101, name: "belka" } }, { layer: "bug", page: "4001" }]);
  t.after(vizrs.close);
  const { api } = await setup(t, {}, { vizrs: { address: vizrs.address, token: "" } });

  const state = (await api("GET", "state")).body;
  assert.deepEqual(state.features, ["layers", "take", "asrun", "encoder", "thumbs", "logs", "on_live"], "no webhook without one configured");
  // The background refresh reads the channels of ErsatzTV and STATE every two
  // seconds, so the first answer may have neither yet.
  const ch = await (async () => {
    let last;
    for (let i = 0; i < 40; i++) {
      last = (await api("GET", "state")).body.channels.find((c) => c.number === "4");
      if (last?.layers?.length) return last;
      await new Promise((r) => setTimeout(r, 250));
    }
    return last;
  })();
  assert.ok(ch, "the channels of ErsatzTV are in the state");
  const layers = ch.layers ?? [];
  assert.equal(layers.length, 2);
  assert.equal(layers[0].layer, "main");
  assert.equal(layers[0].page, "101");
  assert.equal(layers[0].name, "belka");
  assert.ok(layers[0].since > 0, "since when the page is on that layer");
  assert.equal(layers[1].page, "4001");
});

test("TAKE, CONTINUE and OUT by hand reach VizRS and the as-run log", async (t) => {
  const vizrs = await fakeVizrs();
  t.after(vizrs.close);
  const { api, folder } = await setup(t, {}, { vizrs: { address: vizrs.address, token: "" } });

  assert.equal((await api("POST", "channels/4/take", { page: "101" })).status, 200);
  assert.equal((await api("POST", "channels/4/continue", { layer: 10 })).status, 200);
  assert.equal((await api("POST", "channels/4/out", { layer: 10 })).status, 200);
  assert.ok(vizrs.received.includes("TAKE 101"), vizrs.received.join(" | "));
  assert.ok(vizrs.received.includes("CONTINUE 10") && vizrs.received.includes("OUT 10"));

  assert.equal((await api("POST", "channels/4/take", {})).status, 400, "a page is needed");
  assert.equal((await api("POST", "channels/4/out", {})).status, 400, "a layer is needed");
  assert.equal((await api("POST", "channels/x y/take", { page: "1" })).status, 400);

  const day = new Date().toISOString().slice(0, 10);
  const log = (await api("GET", `asrun?channel=4&day=${day}`)).body;
  assert.equal(log.day, day);
  assert.equal(log.tz, "UTC");
  const kinds = log.entries.map((e) => e.kind);
  assert.deepEqual(kinds, ["take", "continue", "out"], JSON.stringify(log.entries));
  assert.equal(log.entries[0].page, "101");
  assert.equal(log.entries[0].operator, "MCR 1", "the operator as VizRS Live sent it (it adds its own prefix)");
  assert.equal(log.entries[1].layer, 10);
  assert.ok(existsSync(path.join(folder, "vizrs-live", "asrun", `asrun-${day}.jsonl`)));

  // Another channel has its own log; a bad day is refused.
  assert.deepEqual((await api("GET", `asrun?channel=9&day=${day}`)).body.entries, []);
  assert.equal((await api("GET", "asrun?channel=4&day=2026-11-9")).status, 400);
});

// Emission logs next to the as-run log (src/logs.rs): the sidecar's own file, the
// ingest listeners' file and the takeover trace of a channel, one UTC day, keys masked.
test("emission logs of a channel and a day", async (t) => {
  const { api, folder } = await setup(t);
  const day = new Date().toISOString().slice(0, 10);
  const ingest = path.join(folder, "vizrs-live", "logs", "vizrs-ingest.log");
  writeFileSync(
    ingest,
    `${day}T00:00:01.000Z vizrs-ingest: channel 4: publisher on rtmp://0.0.0.0:1936/live/secret-key\n` +
      `${day}T00:00:02.000Z vizrs-ingest: channel 9: publisher on rtmp://0.0.0.0:1936/live/other\n` +
      `2020-01-01T00:00:00.000Z vizrs-ingest: channel 4: long ago\n`,
  );
  mkdirSync(path.join(folder, "logs"), { recursive: true });
  writeFileSync(
    path.join(folder, "logs", "vizrs-takeover-4.log"),
    `${day}T00:00:03.000Z pid 7 channel 4: the live source gave no picture; the schedule goes on | seq 1\n`,
  );

  const got = (await api("GET", `logs?source=ingest&channel=4&day=${day}`)).body;
  assert.equal(got.tz, "UTC");
  assert.deepEqual(got.entries.map((e) => e.text), ["channel 4: publisher on rtmp://0.0.0.0:1936/live/***"]);
  assert.equal(got.entries[0].level, "info");
  const takeover = (await api("GET", `logs?source=takeover&channel=4&day=${day}`)).body.entries;
  assert.equal(takeover[0].level, "warn");
  assert.match(takeover[0].text, /^channel 4: the live source gave no picture/);
  // The sidecar logs to its file from the start (etv_config set).
  const own = (await api("GET", `logs?source=cg&channel=4&day=${day}`)).body.entries;
  assert.ok(own.length > 0, "the sidecar's own log of today");
  assert.equal((await api("GET", `logs?source=etc&channel=4&day=${day}`)).status, 400);
  assert.equal((await api("GET", "logs?source=cg&channel=4&day=2026-1-1")).status, 400);
});

test("alarms of the as-run log reach the webhook", async (t) => {
  const alerts = [];
  const hook = http.createServer((req, res) => {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      alerts.push(JSON.parse(body));
      res.writeHead(204).end();
    });
  });
  await new Promise((resolve) => hook.listen(0, "127.0.0.1", resolve));
  t.after(() => hook.close());
  const { api } = await setup(t, { VIZRS_ALERT_WEBHOOK: `http://127.0.0.1:${hook.address().port}/alarm` });

  assert.deepEqual((await api("GET", "state")).body.features, ["layers", "take", "asrun", "encoder", "thumbs", "logs", "on_live", "webhook"]);
  assert.equal((await api("POST", "channels/4/hold")).status, 200);
  for (let i = 0; i < 40 && alerts.length === 0; i++) await new Promise((r) => setTimeout(r, 100));
  assert.equal(alerts.length, 1, "holding a channel is an alarm");
  assert.equal(alerts[0].kind, "hold");
  assert.equal(alerts[0].channel, "4");
  assert.ok(alerts[0].t > 0);
  // Letting the studio back on is no alarm.
  assert.equal((await api("POST", "channels/4/release")).status, 200);
  await new Promise((r) => setTimeout(r, 300));
  assert.equal(alerts.length, 1);
});

// Thumbnails of VizRS pages (src/thumbs.rs): the show from VizRS's HTTP API, one
// renderer run for every page missing, kept until the page's revision changes;
// a page that draws nothing is a 404 and is not drawn again.
const THUMB_RENDERER = `#!/usr/bin/env node
const fs = require("fs");
const a = process.argv.slice(2);
const dir = a[a.indexOf("--dir") + 1];
const pages = a.flatMap((x, i) => (x === "--page" ? [a[i + 1]] : []));
fs.appendFileSync(process.env.FAKE_LOG, pages.join(",") + "\\n");
for (const n of pages) {
  if (n === "3") { console.log(JSON.stringify({ number: n, empty: true, issues: [] })); continue; }
  const file = dir + "/" + n + ".png";
  fs.writeFileSync(file, "PNG " + n + " " + fs.readFileSync(a[a.indexOf("--show") + 1], "utf8").length);
  console.log(JSON.stringify({ number: n, file, crop: [0, 0, 10, 10], issues: [] }));
}
`;

test("thumbnails of VizRS pages are drawn once per revision and kept", async (t) => {
  const dir = mkdtempSync(path.join(os.tmpdir(), "vizrs-thumbs-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const bin = path.join(dir, "vizrs-render");
  writeFileSync(bin, THUMB_RENDERER);
  chmodSync(bin, 0o755);
  const log = path.join(dir, "runs.log");
  writeFileSync(log, "");
  let rev = 1;
  const show = () => ({ scenes: [{ id: "s", rev: 1 }], pages: [{ id: "a", number: 1, scene: "s", rev }, { id: "b", number: 2, scene: "s", rev: 1 }, { id: "c", number: 3, scene: "s" }] });
  const vizrsHttp = http.createServer((req, res) => {
    if (req.url !== "/api/show") return res.writeHead(404).end();
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify(show()));
  });
  await new Promise((r) => vizrsHttp.listen(0, "127.0.0.1", r));
  t.after(() => vizrsHttp.close());
  const { cg } = await setup(t, { FAKE_LOG: log }, { renderer_bin: bin, output_url: `http://127.0.0.1:${vizrsHttp.address().port}/output` });
  const get = (n) => fetch(`${cg.url}/api/live/thumbs/${n}`, { headers: { "X-Panel-Token": TOKEN } });
  const runs = () => readFileSync(log, "utf8").split("\n").filter(Boolean);

  const one = await get("1.png");
  assert.equal(one.status, 200);
  assert.equal(one.headers.get("content-type"), "image/png");
  assert.match(await one.text(), /^PNG 1 /);
  const tag = one.headers.get("etag");
  assert.deepEqual(runs(), ["1,2,3"], "one run for every page");
  assert.equal((await get("2.png")).status, 200);
  assert.equal((await get("3.png")).status, 404, "a page that draws nothing");
  assert.equal((await get("9.png")).status, 404, "no such page");
  assert.equal((await get("..%2F1.png")).status, 404);
  assert.equal((await fetch(`${cg.url}/api/live/thumbs/1.png`)).status, 401, "the panel token");
  assert.deepEqual(runs(), ["1,2,3"], "kept: nothing drawn again");

  rev = 2;
  await new Promise((r) => setTimeout(r, 10500)); // the show stands 10 s for the next requests
  const again = await get("1.png");
  assert.equal(again.status, 200);
  assert.notEqual(again.headers.get("etag"), tag, "a new revision, a new tag");
  assert.deepEqual(runs(), ["1,2,3", "1"], "only the changed page is drawn again");
});
