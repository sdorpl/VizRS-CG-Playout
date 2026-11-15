// Wyniki zadań w tle legacy (K6). Zadania `queued` (sprawdzanie nakładania, konserwacja) nie zwracają wyniku –
// legacy zapisuje go tylko w swoim logu. Panel po zleceniu czyta GetRecentLogEntries (≤ 60 s) i pokazuje
// wpisy z czasem po zleceniu, pasujące do zadania. Wzorce komunikatów legacy: DOKUMENTACJA §4 „Wyniki zadań w tle”.

import { h, cmd, has, dialog, toast, table, pill, hms } from './core.js';

/** Komunikat legacy przy nakładaniu (CheckForOverlappingPlayoutItemsHandler; legacy loguje tylko, gdy znajdzie nakładanie). */
export const OVERLAP_RE = /Playout for channel (.+) has overlapping playout items/;
/** Zapas na różnicę zegarów przeglądarki i serwera legacy. */
const SKEW_MS = 5000;
const LEVEL_KIND = { Warning: 'warn', Error: 'bad', Fatal: 'bad', Information: 'info' };

/** Wpisy logu legacy z czasem ≥ `since` (ms), pasujące do filtra tekstu legacy i wyrażenia `match`. */
export async function logSince(since, filter = '', match = null, pageSize = 200) {
  if (!has('GetRecentLogEntries')) return null;
  const d = await cmd('GetRecentLogEntries', { PageNum: 0, PageSize: pageSize, Filter: filter });
  return (d?.Page || []).filter((e) => Date.parse(e.Timestamp) >= since && (!match || match.test(e.Message || '')));
}

/**
 * Nakładanie pozycji wykryte przez legacy od `since` (ms): [{ channel, time }] – najnowszy wpis na kanał.
 * Używa Na antenie / EPG / Playouty (kontrola ramówki, K9); tylko odczyt.
 */
export async function overlapWarnings(since) {
  const rows = await logSince(since, 'overlapping playout items', OVERLAP_RE).catch(() => null);
  if (!rows) return [];
  const by = new Map();
  for (const e of rows) {
    const ch = OVERLAP_RE.exec(e.Message)[1];
    const t = Date.parse(e.Timestamp);
    if (!by.has(ch) || by.get(ch).time < t) by.set(ch, { channel: ch, time: t });
  }
  return [...by.values()];
}

/**
 * Czeka na wpisy zadania w logu legacy i pokazuje je w oknie.
 * opts: { label, filter, match, until?(wpisy) => true (koniec czekania), timeoutMs (domyślnie 60 s), empty (tekst przy braku wpisów) }
 */
export async function followTask(opts) {
  if (!has('GetRecentLogEntries')) { toast(`${opts.label}: zlecone w tle legacy – wynik w Logach.`, 'ok'); return null; }
  const since = Date.now() - SKEW_MS;
  const timeout = opts.timeoutMs ?? 60000;
  toast(`${opts.label}: zlecone w tle legacy – czekam na wynik w logu (do ${Math.round(timeout / 1000)} s)…`, 'ok');
  let rows = [];
  const where = location.hash;
  for (const t0 = Date.now(); Date.now() - t0 < timeout;) {
    // operator przeszedł na inny ekran – bez okna nad cudzym ekranem, wynik zostaje w Logach
    if (location.hash !== where) { toast(`${opts.label}: wynik w System → Logi.`); return null; }
    await new Promise((r) => setTimeout(r, 2500));
    try { rows = await logSince(since, opts.filter || '', opts.match); } catch { rows = []; }
    if (rows.length && (!opts.until || opts.until(rows))) break;
  }
  if (location.hash !== where) { toast(`${opts.label}: wynik w System → Logi.`); return null; }
  const body = rows.length
    ? h('div', { class: 'task-result' },
      table(rows, [[(e) => hms(new Date(e.Timestamp)), 'Czas'], [(e) => pill(e.Level, LEVEL_KIND[e.Level]), 'Poziom'], ['Message', 'Wpis legacy', { class: 'w' }]]))
    : h('div', { class: 'task-result' }, h('p', null, opts.empty || 'Legacy nie zapisał w logu nic o tym zadaniu – szczegóły (także wpisy Debug) w System → Logi.'));
  await dialog(`${opts.label} – wynik`, body, [['Zamknij', true, 'acc']]);
  return rows;
}
