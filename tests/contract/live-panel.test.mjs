// Live API for VizRS Live (DOKUMENTACJA §1, Live API; src/panel.rs): the token
// (no page and no cookie), refusal of other sites, the live programmes wizard against a
// fake ErsatzTV (file, library scan, smart collection, guide), holding a
// channel through /ingest/state, and the first filling of shows.json from
// VIZRS_LIVE_SHOWS.

import assert from "node:assert/strict";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";

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
