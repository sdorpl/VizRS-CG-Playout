// Alarmy operatora (K17, wzorzec Vizrt I) – w przeglądarce, na każdym ekranie: banner pod paskiem górnym
// i opcjonalny dźwięk (WebAudio, bez plików). Tylko z danych, które panel ma; co 60 s, bez pętli w serwerze.
// Webhook poza przeglądarką robi VizRS CG Playout (VIZRS_ALERT_WEBHOOK, kontrakt – DOKUMENTACJA §3).

import { S, h, btn, cmd, api, has, hm, onChannel } from './core.js';
import { checkItems } from './check.js';

export const ALARM_WINDOW_MS = 15 * 60e3;     // błąd ramówki w najbliższych 15 min
export const LIVE_WARN_MS = 5 * 60e3;         // slot LIVE za ≤ 5 min bez sygnału studia
export const GRAPHICS_OFF_MS = 5 * 60e3;      // grafika zdjęta dłużej niż 5 min
const SNOOZE_MS = 10 * 60e3;

const store = (k, v) => { try { if (v === undefined) return localStorage.getItem(k); localStorage.setItem(k, v); } catch { /* bez localStorage */ } return null; };
let graphicsOffSeen = null;   // kiedy panel pierwszy raz zobaczył zdjętą grafikę (gdy CG nie podaje graphics_off_since)
let known = new Set();
let audio = null;

/**
 * Alarmy z bieżącego stanu (czysta funkcja – testowalna): [{ key, level, text, href }].
 * d = { now, channels, playouts: Map(numer → playout), items: Map(numer → pozycje), cg: stan CG albo null, sessions: tablica albo null }.
 */
export function computeAlarms(d) {
  const out = [];
  const now = d.now;
  for (const c of d.channels) {
    const n = String(c.Number);
    const p = d.playouts.get(c.Number);
    if (!p) continue;
    const list = d.items.get(n) || [];
    if (!list.some((it) => +new Date(it.Start) <= now && now < +new Date(it.Finish))) {
      out.push({ key: `gap-now-${n}`, level: 'bad', text: `${c.Name}: przerwa w ramówce TERAZ – leci wypełniacz zapasowy`, href: '#playouty' });
    }
    const ck = checkItems(list, { now, window: ALARM_WINDOW_MS });
    for (const g of ck.gaps) if (g.s > now && g.s - now <= ALARM_WINDOW_MS) out.push({ key: `gap-${n}-${g.s}`, level: 'bad', text: `${c.Name}: dziura w ramówce o ${hm(new Date(g.s))} (${Math.round((g.e - g.s) / 60000)} min)`, href: '#epg' });
    const last = list[list.length - 1];
    if (last && list.length < 30 && +new Date(last.Finish) - now <= ALARM_WINDOW_MS) out.push({ key: `end-${n}`, level: 'bad', text: `${c.Name}: playout kończy się o ${hm(new Date(last.Finish))} – przebuduj`, href: '#playouty' });
  }
  const cg = d.cg;
  if (cg && !(cg instanceof Error)) {
    for (const ch of (cg.channels || []).filter((x) => onChannel(x.number))) {
      const who = ch.name || `kanał ${ch.number}`;
      const nl = ch.next_live;
      if (nl && nl.start - now <= LIVE_WARN_MS && nl.start > now - 60e3 && !ch.source?.live) {
        out.push({ key: `live-${ch.number}-${nl.start}`, level: 'bad', text: `${who}: slot na żywo „${nl.title}” o ${hm(new Date(nl.start))} – brak sygnału studia`, href: '#live' });
      }
      if (Array.isArray(d.sessions) && !d.sessions.some((s) => String(s.ChannelNumber) === String(ch.number))) {
        out.push({ key: `stopped-${ch.number}`, level: 'warn', text: `${who}: emisja kanału zatrzymana (CG Playout nadaje ten kanał, a legacy nie ma sesji)`, href: '#antena' });
      }
    }
    const off = cg.settings?.graphics_off;
    if (off) {
      const since = cg.settings.graphics_off_since ?? (graphicsOffSeen ??= now);
      if (now - since >= GRAPHICS_OFF_MS) out.push({ key: 'graphics-off', level: 'bad', text: `cała grafika VizRS zdjęta od ${hm(new Date(since))} – przywróć w Na antenie`, href: '#antena' });
    } else graphicsOffSeen = null;
  }
  return out;
}

function beep() {
  try {
    audio ??= new AudioContext();
    const o = audio.createOscillator(), g = audio.createGain();
    o.frequency.value = 880; o.type = 'square';
    g.gain.setValueAtTime(0.0001, audio.currentTime);
    g.gain.exponentialRampToValueAtTime(0.2, audio.currentTime + 0.02);
    g.gain.exponentialRampToValueAtTime(0.0001, audio.currentTime + 0.5);
    o.connect(g).connect(audio.destination);
    o.start(); o.stop(audio.currentTime + 0.55);
  } catch { /* przeglądarka bez WebAudio */ }
}

async function gather() {
  const now = Date.now();
  const [channels, playouts, cg, sessions] = await Promise.all([
    cmd('GetAllChannels', { ShowDisabled: false }).catch(() => []),
    cmd('GetPagedPlayouts', { Query: '', PageNum: 0, PageSize: 500 }).then((r) => new Map((r?.Page || []).map((p) => [p.ChannelNumber, p]))).catch(() => new Map()),
    S.status?.cg ? api('/api/cg/live/state').then((r) => r.result).catch((e) => e) : null,
    api('/api/sessions').then((r) => r.result || []).catch(() => null)]);
  const mine = channels.filter((c) => onChannel(c.Number));
  const items = new Map();
  await Promise.all(mine.map(async (c) => {
    const p = playouts.get(c.Number);
    if (!p) return;
    const r = await cmd('GetFuturePlayoutItemsById', { PlayoutId: p.PlayoutId, ShowFiller: true, PageNum: 0, PageSize: 30 }).catch(() => null);
    items.set(String(c.Number), r?.Page || []);
  }));
  return { now, channels: mine, playouts, items, cg, sessions };
}

/** Banner alarmów pod paskiem górnym; uruchamia sprawdzanie co 60 s. */
export function startAlarms() {
  if (!has('GetAllChannels') || !has('GetFuturePlayoutItemsById')) return;
  const list = h('div', { class: 'alarm-list' });
  const sound = btn('', () => { store('vizrs-live-alarm-sound', store('vizrs-live-alarm-sound') === '1' ? '0' : '1'); label(); if (store('vizrs-live-alarm-sound') === '1') beep(); });
  sound.id = 'alarm-sound';
  const label = () => { sound.textContent = store('vizrs-live-alarm-sound') === '1' ? 'Dźwięk: wł.' : 'Dźwięk: wył.'; };
  label();
  const snooze = btn('Ukryj na 10 min', () => { store('vizrs-live-alarm-snooze', String(Date.now() + SNOOZE_MS)); el.hidden = true; });
  const el = h('div', { class: 'alarm', id: 'alarm', role: 'alert', hidden: true }, h('b', null, 'ALARM'), list, h('span', { class: 'grow' }), sound, snooze);
  document.querySelector('.topbar').after(el);
  const run = async () => {
    let alarms;
    try { alarms = computeAlarms(await gather()); } catch { return; }
    const snoozed = Number(store('vizrs-live-alarm-snooze') || 0) > Date.now();
    list.replaceChildren(...alarms.map((a) => h('a', { href: a.href, class: 'alarm-item ' + a.level }, a.text)));
    el.classList.toggle('warn-only', alarms.every((a) => a.level !== 'bad'));
    const fresh = alarms.filter((a) => !known.has(a.key) && a.level === 'bad');
    known = new Set(alarms.map((a) => a.key));
    el.hidden = !alarms.length || (snoozed && !fresh.length);
    if (fresh.length && store('vizrs-live-alarm-sound') === '1') beep();
  };
  run();
  setInterval(run, 60000);
}
