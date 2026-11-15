// Odtwarzanie pozycji z Przeglądarki mediów (film, odcinek, audycja) w karcie pozycji.
// Legacy nie podaje całego pliku – jedyna droga to jego test odtwarzania (/api/troubleshoot/playback.m3u8):
// 30 s pozycji od wybranej sekundy, kodowane wybranym profilem, jedna sesja naraz w całym legacy. Serwer panelu
// uruchamia ją (POST /api/play/{pozycja}/{profil}/{sekunda}) i przepuszcza HLS (/api/stream/session/.troubleshooting/…).
// Odtwarzacz pobiera cały fragment, a gdy w buforze zostaje mało obrazu, zamawia następne 30 s – ogląda się
// ciągiem, z krótką przerwą, gdy legacy startuje kodowanie kolejnego fragmentu.

import { api, h, btn } from './core.js';
import { parsePlaylist } from './player.js';
import { Transmuxer } from './mux.js';

export const CLIP = 30;  // s – długość fragmentu testu odtwarzania legacy
const LOW = 12;          // s obrazu w buforze, przy których zamawiamy następny fragment
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** „gg:mm:ss[.fff]” / „d.gg:mm:ss” (TimeSpan legacy) → sekundy; liczba – bez zmian. */
export function seconds(v) {
  if (typeof v === 'number') return v;
  const m = /^(?:(\d+)\.)?(\d+):(\d+):(\d+(?:\.\d+)?)$/.exec(String(v || '').trim());
  return m ? (+(m[1] || 0)) * 86400 + (+m[2]) * 3600 + (+m[3]) * 60 + parseFloat(m[4]) : 0;
}
/** Sekundy → „g:mm:ss” albo „m:ss”. */
export function clock(s) {
  s = Math.max(0, Math.floor(s));
  const hh = Math.floor(s / 3600), mm = Math.floor((s % 3600) / 60), ss = s % 60;
  return (hh ? `${hh}:${String(mm).padStart(2, '0')}` : String(mm)) + ':' + String(ss).padStart(2, '0');
}
/** „1:02:03”, „62:03”, „90” → sekundy albo null. */
export function parseClock(t) {
  const p = String(t).trim().split(':');
  if (!p.length || p.length > 3 || p.some((x) => !/^\d+$/.test(x))) return null;
  return p.reduce((a, x) => a * 60 + Number(x), 0);
}

export class ClipPlayer {
  /** report(tekst, rodzaj) – stan dla operatora: '' (ładuje), 'on' (gra), 'bad' (błąd), 'none' (koniec). */
  constructor(video, report) { this.video = video; this.report = report; this.run = 0; }

  stop() {
    this.run++;
    this.abort?.abort();
    const v = this.video;
    v.pause();
    v.removeAttribute('src');
    v.load();
    if (this.url) URL.revokeObjectURL(this.url);
    this.url = null;
  }

  /** Odtwarza pozycję od sekundy `from` (do `duration`, 0 = nieznana) profilem kodowania `profile`. */
  async play(item, profile, from, duration) {
    this.stop();
    const run = this.run, alive = () => run === this.run;
    this.abort = new AbortController();
    const v = this.video;
    if (!window.MediaSource) { this.report('Ta przeglądarka nie ma MediaSource – odtwarzanie w panelu niedostępne.', 'bad'); return; }
    const ms = new MediaSource();
    this.url = URL.createObjectURL(ms);
    v.src = this.url;
    await new Promise((r) => ms.addEventListener('sourceopen', r, { once: true }));
    const caps = { hevc: MediaSource.isTypeSupported('video/mp4; codecs="hvc1.1.6.L93.B0"'), ac3: MediaSource.isTypeSupported('audio/mp4; codecs="ac-3"') };
    const mux = new Transmuxer(caps);
    const bufs = {};
    let pos = from, base = null, started = false;
    try {
      while (alive() && (!duration || pos < duration - 0.5)) {
        // następny fragment dopiero, gdy w buforze zostaje mało obrazu (legacy koduje jeden naraz)
        while (alive() && base != null && this.ahead() > LOW) await sleep(500);
        if (!alive()) return;
        this.report(base == null ? `Legacy koduje fragment od ${clock(pos)}… (kilka sekund)` : `gra · ładuję dalszy ciąg od ${clock(pos)}`, base == null ? '' : 'on');
        const got = await this.clip(item, profile, pos, mux, bufs, ms, alive, (start) => {
          if (base == null) {
            base = start;
            // oś odtwarzacza ↔ pozycja w materiale: currentTime = sekunda pozycji
            for (const sb of Object.values(bufs)) sb.timestampOffset = from - base;
          }
        }, () => {
          if (started) return;
          started = true;
          v.currentTime = from;
          v.play().catch(() => {});
          this.report('gra', 'on');
        });
        if (!alive()) return;
        if (got <= 0) break;
        pos += got;
        if (started) this.report(`gra · w buforze do ${clock(pos)}`, 'on');
      }
      if (!alive()) return;
      await this.waitIdle(bufs);
      if (ms.readyState === 'open') ms.endOfStream();
      this.report(duration && pos >= duration - 0.5 ? 'koniec pozycji' : 'koniec fragmentów', 'none');
    } catch (e) {
      if (!alive() || e.name === 'AbortError') return;
      this.report(e.message, 'bad');
    }
  }

  /** Jeden fragment: start w legacy, playlista do #EXT-X-ENDLIST, segmenty → MSE. Zwraca sekundy materiału. */
  async clip(item, profile, pos, mux, bufs, ms, alive, onStart, onAppended) {
    const r = await api(`/api/play/${item}/${profile}/${Math.floor(pos)}`, { method: 'POST', body: '' });
    const url = new URL(r.result.playlist, location.href).href;
    let next = 0, got = 0, idle = 0, first = true;
    while (alive()) {
      const text = await (await this.fetchOk(url)).text();
      const pl = parsePlaylist(text, url);
      const ended = /^#EXT-X-ENDLIST/m.test(text);
      let fresh = false;
      for (const seg of pl.segments) {
        if (seg.seq < next) continue;
        const data = new Uint8Array(await (await this.fetchOk(seg.uri)).arrayBuffer());
        if (!alive()) return 0;
        const out = mux.push(data, { disc: first });
        first = false;
        if (out.error) throw new Error(out.error === 'HEVC' ? 'Profil koduje HEVC, którego ta przeglądarka nie dekoduje – wybierz profil H.264.' : out.error);
        next = seg.seq + 1; got += seg.dur; fresh = true;
        if (!out.video && !out.audio) continue;
        for (const kind of ['video', 'audio']) {
          if (out[kind]?.init && !bufs[kind]) bufs[kind] = Object.assign(ms.addSourceBuffer(`${kind}/mp4; codecs="${out[kind].codec}"`), { codec: out[kind].codec });
        }
        if (!bufs.video) throw new Error('fragment bez obrazu H.264/HEVC');
        onStart(out.start);
        for (const kind of ['video', 'audio']) await this.append(bufs[kind], kind, out[kind]);
        onAppended();
      }
      if (ended) return got;
      idle = fresh ? 0 : idle + 1;
      if (idle > 40) return got; // 20 s bez nowych segmentów – legacy skończył bez ENDLIST
      await sleep(500);
    }
    return 0;
  }

  async fetchOk(url) {
    const r = await fetch(url, { signal: this.abort.signal, cache: 'no-store' });
    if (!r.ok) {
      let msg = `HTTP ${r.status}`;
      try { msg = (await r.json()).error || msg; } catch { /* nie-JSON */ }
      throw new Error(msg);
    }
    return r;
  }

  ahead() {
    const v = this.video, b = v.buffered;
    for (let i = 0; i < b.length; i++) if (b.start(i) <= v.currentTime + 0.5 && v.currentTime < b.end(i)) return b.end(i) - v.currentTime;
    return 0;
  }

  async waitIdle(bufs) {
    for (const sb of Object.values(bufs)) while (sb.updating) await sleep(50);
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
    if (part.init) await this.op(sb, () => sb.appendBuffer(part.init));
    await this.op(sb, () => sb.appendBuffer(part.data));
  }
}

/**
 * Sekcja „Odtwarzanie” karty pozycji: wideo, „od” (g:mm:ss), profil kodowania, ‹ 30 s / 30 s ›.
 * profiles – [{Id, Name}] z GetAllFFmpegProfiles; duration – sekundy pozycji (0 = nieznana).
 * Zwraca { el, stop } – stop() przy zamknięciu karty.
 */
export function clipSection(item, duration, profiles) {
  const video = h('video', { class: 'clip-video', controls: true, playsinline: true, hidden: true });
  const state = h('span', { class: 'pill' }, 'gotowe');
  const info = h('span', { class: 'dim' });
  const LABEL = { '': 'ŁADUJĘ', on: 'GRA', bad: 'BŁĄD', none: 'KONIEC' };
  const player = new ClipPlayer(video, (text, kind) => { state.textContent = LABEL[kind] ?? kind; state.className = 'pill' + (kind ? ' ' + kind : ''); info.textContent = text; });
  const from = h('input', { class: 'mono clip-from', value: '0:00', 'aria-label': 'Odtwarzaj od (g:mm:ss)', title: 'Od którego miejsca: 1:23:45, 12:30 albo sekundy' });
  const KEY = 'vizrs-live-clip-profile';
  const prof = h('select', { 'aria-label': 'Profil kodowania odtwarzania' }, profiles.map((p) => h('option', { value: p.Id }, p.Name)));
  try { const k = localStorage.getItem(KEY); if (k && profiles.some((p) => String(p.Id) === k)) prof.value = k; } catch { /* bez localStorage */ }
  prof.addEventListener('change', () => { try { localStorage.setItem(KEY, prof.value); } catch { /* bez localStorage */ } });
  const go = (s) => {
    const t = s ?? parseClock(from.value);
    if (t == null) return info.textContent = 'Podaj czas jako g:mm:ss, m:ss albo sekundy.';
    const at = duration ? Math.min(Math.max(0, t), Math.max(0, duration - 1)) : Math.max(0, t);
    from.value = clock(at);
    video.hidden = false;
    player.play(item, prof.value, at, duration);
  };
  const here = () => (video.hidden ? parseClock(from.value) ?? 0 : video.currentTime);
  from.addEventListener('keydown', (e) => { if (e.key === 'Enter') go(); });
  const el = h('div', { class: 'clip', 'data-clip': '' },
    video,
    h('div', { class: 'bar' },
      btn('▶ Odtwórz', () => go(), 'acc'), h('label', null, 'od'), from,
      btn('‹ 30 s', () => go(here() - CLIP)), btn('30 s ›', () => go(here() + CLIP)),
      btn('■', () => { player.stop(); state.textContent = 'zatrzymane'; state.className = 'pill'; info.textContent = ''; }),
      h('span', { class: 'grow' }), h('label', null, 'profil'), prof),
    h('div', { class: 'bar' }, state, info),
    h('div', { class: 'hint' }, `Przez test odtwarzania legacy: koduje po ${CLIP} s wybranym profilem i dokleja kolejne fragmenty – przy przejściu bywa krótka pauza. Jedno odtwarzanie naraz w całym legacy. ` +
      (duration ? `Długość: ${clock(duration)}.` : '')));
  return { el, stop: () => player.stop() };
}

// ---------- odtwarzanie bez legacy i bez transkodowania (/api/vod/…) ----------
const VIDEO_OK = { h264: 'avc1.640028', vp8: 'vp8', vp9: 'vp09.00.10.08', av1: 'av01.0.05M.08', hevc: 'hvc1.1.6.L93.B0' };
const AUDIO_OK = { aac: 'mp4a.40.2', mp3: 'mp3', opus: 'opus', vorbis: 'vorbis', flac: 'flac', ac3: 'ac-3', eac3: 'ec-3' };

/** Czy przeglądarka zagra plik sama: kodek obrazu i dźwięku z GetMediaItemInfo. Zwraca {ok, why}. */
export function directSupport(streams, path) {
  const v = document.createElement('video');
  const can = (t) => !!v.canPlayType(t) || !!(window.MediaSource && MediaSource.isTypeSupported(t));
  const ext = String(path || '').toLowerCase().split('.').pop();
  const box = ext === 'webm' || ext === 'mkv' ? 'video/webm' : 'video/mp4';
  const vs = (streams || []).find((s) => s.Kind === 'Video');
  const as = (streams || []).find((s) => s.Kind === 'Audio');
  const vc = String(vs?.Codec || '').toLowerCase().replace('h265', 'hevc');
  const ac = String(as?.Codec || '').toLowerCase();
  if (['avi', 'ts', 'm2ts', 'mpg', 'mpeg', 'wmv', 'flv'].includes(ext)) return { ok: false, why: `kontener .${ext} – przeglądarka go nie odtworzy` };
  if (vs && !(VIDEO_OK[vc] && can(`${box}; codecs="${VIDEO_OK[vc]}"`))) return { ok: false, why: `obraz ${vc || '?'} – przeglądarka go nie dekoduje` };
  if (as && !(AUDIO_OK[ac] && can(`${box}; codecs="${AUDIO_OK[ac]}"`))) return { ok: true, why: `dźwięk ${ac} – przeglądarka może go nie odtworzyć (obraz bez dźwięku)` };
  return { ok: true, why: '' };
}

/**
 * Sekcja „Odtwarzanie” karty pozycji. direct – {source: 'jellyfin'|'local', title, path} albo null (tylko przez legacy);
 * plik gra przeglądarka sama (/api/vod/…, bez legacy i transkodowania), a gdy nie umie – przełącza na test odtwarzania legacy.
 * Zwraca { el, stop }.
 */
export function playSection(item, duration, profiles, direct, streams) {
  const legacy = profiles.length ? clipSection(item, duration, profiles) : null;
  if (!direct) return legacy;
  const sup = directSupport(streams, direct.path);
  const video = h('video', { class: 'clip-video', controls: true, playsinline: true, preload: 'metadata', hidden: true });
  const state = h('span', { class: 'pill' }, 'gotowe');
  const info = h('span', { class: 'dim' }, sup.why);
  const from = h('input', { class: 'mono clip-from', value: '0:00', 'aria-label': 'Odtwarzaj od (g:mm:ss)', title: 'Od którego miejsca: 1:23:45, 12:30 albo sekundy' });
  const set = (text, kind) => { state.textContent = { '': 'ŁADUJĘ', on: 'GRA', bad: 'BŁĄD', none: 'PAUZA' }[kind] ?? kind; state.className = 'pill' + (kind ? ' ' + kind : ''); info.textContent = text; };
  let url = null;
  const directBox = h('div', { class: 'clip', 'data-vod': '' });
  const legacyBox = h('div', { hidden: true }, legacy?.el || h('div', { class: 'dim' }, 'Brak profilu kodowania – podgląd przez legacy niedostępny.'));
  const toLegacy = (why) => {
    video.pause(); video.removeAttribute('src'); video.load();
    directBox.hidden = true; legacyBox.hidden = false;
    if (why) legacyBox.prepend(h('div', { class: 'note' }, why));
  };
  const go = async (s) => {
    const t = s ?? parseClock(from.value);
    if (t == null) return set('Podaj czas jako g:mm:ss, m:ss albo sekundy.', 'bad');
    from.value = clock(t);
    try {
      if (!url) {
        set(direct.source === 'jellyfin' ? 'Szukam pozycji w Jellyfin…' : 'Otwieram plik…', '');
        url = (await api('/api/vod/open', { method: 'POST', body: JSON.stringify(direct) })).result.url;
        video.src = url;
      }
      video.hidden = false;
      const seek = () => { video.currentTime = Math.min(t, (video.duration || duration || t + 1) - 0.5); video.play().catch(() => {}); };
      if (video.readyState >= 1) seek(); else video.addEventListener('loadedmetadata', seek, { once: true });
    } catch (e) { set(e.message, 'bad'); }
  };
  video.addEventListener('playing', () => set(`plik prosto z ${direct.source === 'jellyfin' ? 'Jellyfin' : 'biblioteki'} – bez transkodowania${sup.why ? ' · ' + sup.why : ''}`, 'on'));
  video.addEventListener('pause', () => { if (!video.ended && url) set(`pauza · ${clock(video.currentTime)}`, 'none'); });
  video.addEventListener('waiting', () => set('buforuję…', ''));
  video.addEventListener('error', () => {
    const code = video.error?.code;
    if (!url) return;
    toLegacy(code === 4 || code === 3 ? 'Przeglądarka nie odtworzy tego pliku wprost (kodek albo kontener) – podgląd przez legacy z transkodowaniem.' : 'Plik nie dotarł (' + (video.error?.message || 'błąd sieci') + ') – podgląd przez legacy.');
  });
  from.addEventListener('keydown', (e) => { if (e.key === 'Enter') go(); });
  const here = () => (video.hidden ? parseClock(from.value) ?? 0 : video.currentTime);
  directBox.append(video,
    h('div', { class: 'bar' },
      btn('▶ Odtwórz', () => go(), 'acc'), h('label', null, 'od'), from,
      btn('‹ 30 s', () => go(Math.max(0, here() - CLIP))), btn('30 s ›', () => go(here() + CLIP)),
      h('span', { class: 'grow' }),
      legacy ? btn('Przez legacy (transkodowanie)', () => toLegacy('')) : null),
    h('div', { class: 'bar' }, state, info),
    h('div', { class: 'hint' }, `Plik idzie prosto ${direct.source === 'jellyfin' ? 'z Jellyfin' : 'z biblioteki lokalnej'} przez serwer panelu, bez legacy i bez transkodowania – pełne przewijanie paskiem odtwarzacza.` + (duration ? ` Długość: ${clock(duration)}.` : '')));
  if (!sup.ok) { toLegacy(`Odtwarzanie wprost niemożliwe: ${sup.why}. Podgląd przez legacy z transkodowaniem.`); }
  const el = h('div', null, directBox, legacyBox);
  if (legacy) legacyBox.querySelector('[data-clip]')?.append(h('div', { class: 'bar' }, btn('‹ Wprost z pliku', () => { legacy.stop(); legacyBox.hidden = true; directBox.hidden = false; })));
  return { el, stop: () => { video.pause(); video.removeAttribute('src'); video.load(); legacy?.stop(); } };
}
