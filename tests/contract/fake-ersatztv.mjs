// Fake ErsatzTV for the contract tests of the panel API: channels, smart collections, library scans,
// playout resets and the XMLTV guide, as the panel uses them.
import http from "node:http";

/** The API key the fake accepts (`x-etv-api-key`). */
export const KEY = "etv-key";

/** ErsatzTV's API as the panel uses it; every call lands in `calls`. */
export async function fakeErsatzTV() {
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

