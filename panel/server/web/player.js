// Podgląd strumienia kanału: okno w rogu ekranu, które zostaje przy zmianie widoków.
// Źródło: HLS legacy przez serwer panelu (/api/stream/…) – ta sama sesja segmentera, co u widzów,
// więc podgląd niczego nie koduje drugi raz. Segmenty MPEG-TS przepakowuje mux.js (MSE), a obraz
// jest równany do zegara anteny (PROGRAM-DATE-TIME) ze stałym opóźnieniem: kanał z nakładką grafiki
// powstaje w tempie zegara (CG Playout wyprzedza go o LEAD_SECONDS), więc najnowszy segment kończy się
// ok. „teraz” – podgląd grający dokładnie „teraz” stawałby co segment. Opóźnienie to najgorsze z ostatniej
// minuty spóźnienie końca najnowszego segmentu względem zegara plus zapas (zwykle 3–5 s).

import { h, btn, cmd } from './core.js';
import { Transmuxer } from './mux.js';

const AHEAD = 8;       // s bufora przed bieżącym obrazem (pobieramy najwyżej tyle naprzód)
const KEEP = 20;       // s bufora za bieżącym obrazem; starsze dane MSE usuwamy
const MARGIN = 2;      // s zapasu bufora ponad najgorsze spóźnienie najnowszego segmentu
const MIN_DELAY = 1;   // s – najmniejsze opóźnienie podglądu za anteną
const MAX_DELAY = 30;  // s – największe (playlista wyraźnie za zegarem: i tak „opóźnienie” w stanie)
const LAG_WINDOW = 60000; // ms pomiarów spóźnienia segmentów, z których liczymy opóźnienie
const RETRIES = 5;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Playlista HLS: {master, variant, codecs, target, segments: [{seq, uri, dur, pdt, disc}]}. */
export function parsePlaylist(text, base) {
  const lines = text.split(/\r?\n/).map((l) => l.trim()).filter(Boolean);
  const res = { master: false, variant: null, codecs: '', target: 4, segments: [] };
  let seq = 0, dur = 0, pdt = null, disc = false;
  for (let i = 0; i < lines.length; i++) {
    const l = lines[i];
    if (l.startsWith('#EXT-X-STREAM-INF')) {
      res.master = true;
      res.codecs = /CODECS="([^"]*)"/.exec(l)?.[1] || '';
      const uri = lines.slice(i + 1).find((x) => !x.startsWith('#'));
      if (uri && !res.variant) res.variant = new URL(uri, base).href;
    } else if (l.startsWith('#EXT-X-TARGETDURATION:')) res.target = parseFloat(l.slice(22)) || 4;
    else if (l.startsWith('#EXT-X-MEDIA-SEQUENCE:')) seq = parseInt(l.slice(22), 10) || 0;
    else if (l.startsWith('#EXTINF:')) dur = parseFloat(l.slice(8)) || 0;
    else if (l.startsWith('#EXT-X-PROGRAM-DATE-TIME:')) { const t = Date.parse(l.slice(25)); pdt = isNaN(t) ? null : t; }
    else if (l === '#EXT-X-DISCONTINUITY') disc = true;
    else if (!l.startsWith('#') && !res.master) {
      // segment bez własnego PROGRAM-DATE-TIME: czas poprzedniego + jego długość (HLS), chyba że po nieciągłości
      const prev = res.segments[res.segments.length - 1];
      if (pdt == null && !disc && prev?.pdt != null) pdt = prev.pdt + prev.dur * 1000;
      res.segments.push({ seq: seq++, uri: new URL(l, base).href, dur, pdt, disc });
      pdt = null; disc = false;
    }
  }
  return res;
}

class NeedNative extends Error {}

/** Odtwarzacz jednego kanału w elemencie <video>; report(tekst, rodzaj) – stan dla operatora. */
export class Preview {
  constructor(video, report) {
    this.video = video;
    this.report = report;
    this.run = 0;
    this.skew = 0;  // zegar serwera − zegar przeglądarki (ms), z nagłówka X-VizRS-Now
    this.delay = MIN_DELAY; // s, o ile podgląd idzie za zegarem anteny (measure)
    this.lags = [];
  }

  stop() {
    this.run++;
    this.abort?.abort();
    this.teardown();
    this.channel = null;
  }

  /** Odłącza źródło od <video> (MSE albo natywny HLS). */
  teardown() {
    clearInterval(this.syncTimer);
    this.video.pause();
    this.video.removeAttribute('src');
    this.video.load();
    if (this.url) URL.revokeObjectURL(this.url);
    this.url = null;
    this.epoch = null;
    this.delay = MIN_DELAY;
    this.lags = [];
  }

  async start(ch, attempt = 0) {
    this.stop();
    const run = this.run;
    this.channel = ch;
    this.abort = new AbortController();
    const alive = () => run === this.run;
    this.report(attempt ? `Ponawiam (${attempt}/${RETRIES})…` : 'Uruchamiam strumień kanału… (gdy kanału nikt nie ogląda, legacy startuje sesję – do kilkunastu sekund)', '');
    try {
      const masterUrl = new URL(`/api/stream/${encodeURIComponent(ch)}.m3u8`, location.href).href;
      const master = parsePlaylist(await this.text(masterUrl), masterUrl);
      const variant = master.master ? master.variant : masterUrl;
      const hevc = /hvc1|hev1|av01/.test(master.codecs);
      const mse = window.MediaSource && MediaSource.isTypeSupported('video/mp4; codecs="avc1.640028"');
      // K18: HEVC i AC-3 przez MSE tylko tam, gdzie przeglądarka je dekoduje (np. Chrome/Edge ze sprzętowym HEVC, Safari)
      this.caps = { hevc: !!(window.MediaSource && MediaSource.isTypeSupported('video/mp4; codecs="hvc1.1.6.L93.B0"')), ac3: !!(window.MediaSource && MediaSource.isTypeSupported('audio/mp4; codecs="ac-3"')) };
      const av1 = /av01/.test(master.codecs);
      if (!variant) throw new Error('playlista kanału bez wariantu');
      if (mse && !av1 && (!hevc || this.caps.hevc)) {
        try { await this.playMse(variant, alive); return; } catch (e) { if (!(e instanceof NeedNative)) throw e; }
        this.teardown();
      }
      if (!this.video.canPlayType('application/vnd.apple.mpegurl')) {
        this.report(`Ta przeglądarka nie odtworzy strumienia (${master.codecs || 'kodek nieznany'}). Podgląd obsługuje H.264, a HEVC – gdy przeglądarka go dekoduje (Safari, Chrome/Edge ze sprzętowym HEVC); AV1 – VLC.`, 'bad');
        return;
      }
      this.video.src = masterUrl;
      this.video.play().catch(() => {});
      this.report('odtwarzacz HLS przeglądarki – opóźnienie zależy od przeglądarki', 'info');
    } catch (e) {
      if (!alive() || e.name === 'AbortError') return;
      if (attempt >= RETRIES) { this.report(`Podgląd zatrzymany: ${e.message}`, 'bad'); return; }
      this.report(`${e.message} – ponawiam za 3 s`, 'bad');
      await sleep(3000);
      if (alive()) this.start(ch, attempt + 1);
    }
  }

  async fetchOk(url) {
    const r = await fetch(url, { signal: this.abort.signal, cache: 'no-store' });
    if (!r.ok) {
      let msg = `HTTP ${r.status}`;
      try { msg = (await r.json()).error || msg; } catch { /* treść nie-JSON */ }
      throw new Error(msg);
    }
    return r;
  }

  async text(url) {
    const t0 = Date.now();
    const r = await this.fetchOk(url);
    const now = Number(r.headers.get('X-VizRS-Now'));
    if (now) this.skew = now - (t0 + Date.now()) / 2;
    return r.text();
  }

  /** Pokazywany czas anteny (s na osi odtwarzacza: zegar − opóźnienie) albo null bez PROGRAM-DATE-TIME. */
  target() { return this.epoch == null ? null : (Date.now() + this.skew - this.delay * 1000 - this.epoch) / 1000; }

  /** Opóźnienie za anteną: najgorsze z ostatniej minuty spóźnienie końca najnowszego segmentu + MARGIN. */
  measure(segs) {
    const last = segs[segs.length - 1];
    if (last?.pdt == null) return;
    const now = Date.now() + this.skew;
    this.lags = this.lags.filter((s) => now - s.at < LAG_WINDOW);
    this.lags.push({ at: now, lag: (now - (last.pdt + last.dur * 1000)) / 1000 });
    const worst = Math.max(...this.lags.map((s) => s.lag));
    this.delay = Math.min(MAX_DELAY, Math.max(MIN_DELAY, worst + MARGIN));
  }

  ahead() {
    const v = this.video, b = v.buffered;
    for (let i = 0; i < b.length; i++) if (b.start(i) <= v.currentTime + 0.5 && v.currentTime < b.end(i)) return b.end(i) - v.currentTime;
    return 0;
  }

  async playMse(variant, alive) {
    const v = this.video;
    const ms = new MediaSource();
    this.url = URL.createObjectURL(ms);
    v.src = this.url;
    await new Promise((r) => ms.addEventListener('sourceopen', r, { once: true }));
    const mux = new Transmuxer(this.caps || {});
    const bufs = {};
    let next = null, started = false, td = 4;
    this.syncTimer = setInterval(() => this.sync(td), 250);
    while (alive()) {
      const pl = parsePlaylist(await this.text(variant), variant);
      td = pl.target;
      const segs = pl.segments;
      this.measure(segs);
      if (!segs.length) { await sleep(1000); continue; }
      if (next == null || next < segs[0].seq || next > segs[segs.length - 1].seq + 1) next = this.pickStart(segs);
      for (const seg of segs) {
        if (!alive()) return;
        if (seg.seq < next) continue;
        if (started && this.ahead() > AHEAD) break;
        const data = new Uint8Array(await (await this.fetchOk(seg.uri)).arrayBuffer());
        if (!alive()) return;
        const r = mux.push(data, { pdt: seg.pdt, disc: seg.disc });
        if (r.error) throw new NeedNative(r.error);
        next = seg.seq + 1;
        if (!r.video && !r.audio) continue;
        if (!started) {
          for (const kind of ['video', 'audio']) {
            if (r[kind]?.init) bufs[kind] = Object.assign(ms.addSourceBuffer(`${kind}/mp4; codecs="${r[kind].codec}"`), { codec: r[kind].codec });
          }
          if (!bufs.video) throw new NeedNative('brak obrazu H.264/HEVC');
        }
        for (const kind of ['video', 'audio']) await this.append(bufs[kind], kind, r[kind]);
        if (!alive()) return;
        this.epoch = mux.epoch;
        if (!started) {
          started = true;
          const t = this.target();
          v.currentTime = t != null && t > r.start ? t : r.start;
          v.play().catch(() => {});
          this.info = `${mux.v.width}×${mux.v.height} · ${mux.v.hevc ? 'HEVC' : 'H.264'} · ${bufs.audio ? (bufs.audio.codec === 'ac-3' ? 'AC-3' : 'AAC') : `bez dźwięku (podgląd odtwarza AAC${this.caps?.ac3 ? ' i AC-3' : ''})`}`;
        }
      }
      for (const sb of Object.values(bufs)) {
        if (sb.buffered.length && sb.buffered.start(0) < v.currentTime - KEEP - 10) {
          await this.op(sb, () => sb.remove(0, v.currentTime - KEEP));
        }
      }
      await sleep(this.ahead() > td ? 1000 : 300);
    }
  }

  /** Pierwszy segment: ten, w którym wypada pokazywany czas anteny; bez PROGRAM-DATE-TIME – ostatni (krawędź na żywo). */
  pickStart(segs) {
    const now = Date.now() + this.skew - this.delay * 1000;
    if (segs[0].pdt == null) return segs[segs.length - 1].seq;
    let pick = segs[0];
    for (const s of segs) if (s.pdt != null && s.pdt <= now) pick = s;
    return pick.seq;
  }

  op(sb, f) {
    return new Promise((resolve, reject) => {
      const done = (ok) => () => { sb.removeEventListener('updateend', end); sb.removeEventListener('error', fail); ok ? resolve() : reject(new Error('przeglądarka odrzuciła fragment obrazu')); };
      const end = done(true), fail = done(false);
      sb.addEventListener('updateend', end);
      sb.addEventListener('error', fail);
      try { f(); } catch (e) { sb.removeEventListener('updateend', end); sb.removeEventListener('error', fail); reject(e); }
    });
  }

  async append(sb, kind, part) {
    if (!sb || !part) return;
    if (part.codec !== sb.codec && sb.changeType) { sb.changeType(`${kind}/mp4; codecs="${part.codec}"`); sb.codec = part.codec; }
    if (part.init) await this.op(sb, () => sb.appendBuffer(part.init));
    await this.op(sb, () => sb.appendBuffer(part.data));
  }

  /** Co 250 ms: obraz równo z zegarem anteny – małe różnice tempem odtwarzania, duże skokiem. */
  sync(td) {
    const v = this.video, b = v.buffered;
    if (!b.length || this.channel == null) return;
    const inBuf = (t) => { for (let i = 0; i < b.length; i++) if (b.start(i) <= t && t < b.end(i) - 0.1) return true; return false; };
    if (!inBuf(v.currentTime)) { // dziura (np. na styku pozycji ramówki) – do następnego kawałka bufora
      for (let i = 0; i < b.length; i++) if (b.start(i) > v.currentTime && b.start(i) - v.currentTime < 2) { v.currentTime = b.start(i) + 0.01; break; }
    }
    const want = this.target();
    if (want == null) {
      const end = b.end(b.length - 1);
      if (end - v.currentTime > 2 * td + 1) v.currentTime = end - 1;
      this.report(this.info || '', 'on');
      return;
    }
    const lag = want - v.currentTime;
    if (Math.abs(lag) > 2) {
      if (inBuf(want)) v.currentTime = want;
      else if (lag > 0 && b.end(b.length - 1) - 0.5 > v.currentTime) v.currentTime = b.end(b.length - 1) - 0.5;
    }
    v.playbackRate = lag > 0.15 ? 1.06 : lag < -0.15 ? 0.94 : 1;
    if (v.paused) v.play().catch(() => {});
    const behind = `${this.delay.toFixed(1).replace('.', ',')} s za anteną`;
    this.report((Math.abs(lag) < 0.3 ? `równo z zegarem anteny, ${behind}` : `${lag > 0 ? 'opóźnienie' : 'wyprzedzenie'} ${Math.abs(lag).toFixed(1)} s ponad stałe ${behind}`) + (this.info ? ' · ' + this.info : ''), Math.abs(lag) < 1 ? 'on' : 'warn');
  }
}

// ---------- okno podglądu ----------

let dock = null;

function storage(key, val) {
  try {
    if (val === undefined) return localStorage.getItem(key);
    localStorage.setItem(key, val);
  } catch { /* przeglądarka bez localStorage */ }
  return null;
}

function buildDock() {
  const video = h('video', { id: 'mon-video', muted: true, playsinline: true });
  video.muted = true;
  const state = h('span', { class: 'pill', id: 'mon-state' }, 'ŁĄCZĘ');
  const info = h('span', { class: 'dim', id: 'mon-info' });
  const sel = h('select', { id: 'mon-ch', 'aria-label': 'Kanał podglądu' });
  const sound = btn('Dźwięk: wył.', () => { video.muted = !video.muted; sound.textContent = video.muted ? 'Dźwięk: wył.' : 'Dźwięk: wł.'; });
  sound.id = 'mon-sound';
  const el = h('div', { class: 'monitor', id: 'monitor', role: 'region', 'aria-label': 'Podgląd kanału' },
    h('header', null, sel, h('span', { class: 'grow' }),
      sound,
      btn('Większy', (e) => { el.classList.toggle('big'); e.target.textContent = el.classList.contains('big') ? 'Mniejszy' : 'Większy'; }),
      Object.assign(btn('Pełny', () => video.requestFullscreen?.().catch(() => {})), { title: 'Pełny ekran' }),
      Object.assign(btn('×', () => closeMonitor()), { title: 'Zamknij podgląd', id: 'mon-close' })),
    video,
    h('div', { class: 'mon-foot' }, state, info));
  const LABEL = { on: 'NA ŻYWO', info: 'NA ŻYWO', warn: 'POZA ZEGAREM', bad: 'BŁĄD', none: 'BRAK', '': 'ŁĄCZĘ' };
  const preview = new Preview(video, (text, kind = '') => {
    state.textContent = LABEL[kind] ?? kind;
    state.className = 'pill' + (kind ? ' ' + kind : '');
    info.textContent = text;
  });
  sel.addEventListener('change', () => { storage('vizrs-live-monitor', sel.value); preview.start(sel.value); });
  // karta w tle: nic nie pobieramy (sesja legacy wygaśnie sama, gdy nikt inny nie ogląda); po powrocie – znów na żywo
  document.addEventListener('visibilitychange', () => {
    if (el.hidden) return;
    if (document.hidden) preview.stop();
    else if (sel.value) preview.start(sel.value);
  });
  document.body.append(el);
  return { el, sel, preview };
}

/** Otwiera okno podglądu (kanał: numer albo ostatnio oglądany). */
export async function openMonitor(number) {
  dock ??= buildDock();
  dock.el.hidden = false;
  let channels = [];
  try { channels = await cmd('GetAllChannels', { ShowDisabled: false }); } catch { /* lista zostaje pusta */ }
  if (!channels.length) { dock.sel.replaceChildren(); dock.preview.stop(); dock.preview.report('Brak włączonych kanałów.', 'none'); return; }
  dock.sel.replaceChildren(...channels.map((c) => h('option', { value: c.Number }, `${c.Number} · ${c.Name}`)));
  const want = [number, storage('vizrs-live-monitor')].map((n) => n == null ? null : String(n)).find((n) => n && channels.some((c) => c.Number === n));
  dock.sel.value = want || channels[0].Number;
  storage('vizrs-live-monitor', dock.sel.value);
  dock.preview.start(dock.sel.value);
}

export function closeMonitor() {
  if (!dock) return;
  dock.preview.stop();
  dock.el.hidden = true;
}
