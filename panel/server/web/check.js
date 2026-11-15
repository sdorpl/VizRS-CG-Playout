// Kontrola ramówki (K9) – na wzór automatycznej kontroli playlisty Vizrt, tylko z danych, które panel już ma.
// Liczona w przeglądarce przy odświeżeniu ekranu (bez pętli w serwerze), bez zapisu. Opis reguł: DOKUMENTACJA §4 „Kontrola ramówki”.

import { cmd, api, has, hm, S } from './core.js';
import { overlapWarnings } from './tasks.js';

/** Dziura krótsza niż tyle nie jest błędem (zaokrąglenia legacy). */
export const GAP_MIN_MS = 5000;
/** Playout kończący się wcześniej niż za tyle – błąd (legacy dokłada ramówkę przy przebudowie). */
export const HORIZON_MS = 24 * 3600e3;
/** Okno sprawdzania pozycji (dziury, nakładanie, brak pliku). */
export const WINDOW_MS = 24 * 3600e3;
const PAGE = 300;

const key = (it) => `${it.Start}|${it.Title}`;

/**
 * Reguły na liście pozycji jednego kanału (czysta funkcja – testowalna i tania; EPG liczy ją na swoich danych).
 * `missing` – zbiór tytułów pozycji z plikiem w koszu (legacy `state:FileNotFound`).
 * Zwraca { gaps: [{ s, e, before, after }], items: Map(klucz pozycji → [opis]), issues: [{ level, text, at }] }.
 */
export function checkItems(list, { now = Date.now(), missing = null, window = WINDOW_MS } = {}) {
  const gaps = [], issues = [];
  const items = new Map();
  const mark = (it, text) => { const k = key(it); if (!items.has(k)) items.set(k, []); items.get(k).push(text); };
  const end = now + window;
  let prev = null;
  for (const it of list) {
    const s = +new Date(it.Start), e = +new Date(it.Finish);
    if (s >= end) break;
    if (prev) {
      const pe = +new Date(prev.Finish);
      if (s - pe > GAP_MIN_MS) {
        gaps.push({ s: pe, e: s, before: prev.Title, after: it.Title });
        issues.push({ level: 'bad', at: pe, text: `dziura w ramówce ${hm(new Date(pe))}–${hm(new Date(s))} (${Math.round((s - pe) / 60000)} min) po „${prev.Title}” – leci wypełniacz zapasowy` });
      } else if (pe - s > GAP_MIN_MS) {
        mark(it, `nakłada się na „${prev.Title}” (do ${hm(new Date(pe))})`);
        issues.push({ level: 'bad', at: s, text: `pozycje się nakładają: „${it.Title}” o ${hm(new Date(s))} zaczyna się przed końcem „${prev.Title}”` });
      }
    }
    if (missing && e > now && [...missing].some((t) => t && (it.Title === t || it.Title.includes(t)))) {
      mark(it, 'plik w koszu legacy (FileNotFound) – pozycja nie zagra');
      issues.push({ level: 'bad', at: s, text: `„${it.Title}” o ${hm(new Date(s))} – plik w koszu legacy (FileNotFound)` });
    }
    prev = it;
  }
  return { gaps, items, issues };
}

/** Tytuły pozycji z plikiem w koszu (legacy `state:FileNotFound`); pusty zbiór, gdy legacy nie zna zapytań. */
export async function missingTitles() {
  if (!has('QuerySearchIndexAllItems')) return new Set();
  const all = await cmd('QuerySearchIndexAllItems', { Query: 'state:FileNotFound' }).catch(() => null);
  if (!all) return new Set();
  const kinds = [['OtherVideoIds', 'QuerySearchIndexOtherVideos'], ['MovieIds', 'QuerySearchIndexMovies'], ['EpisodeIds', 'QuerySearchIndexEpisodes'],
    ['MusicVideoIds', 'QuerySearchIndexMusicVideos'], ['SongIds', 'QuerySearchIndexSongs'], ['ImageIds', 'QuerySearchIndexImages'], ['RemoteStreamIds', 'QuerySearchIndexRemoteStreams']]
    .filter(([k, q]) => all[k]?.length && has(q));
  const res = await Promise.all(kinds.map(([, q]) => cmd(q, { Query: 'state:FileNotFound', PageNumber: 1, PageSize: 100 }).catch(() => null)));
  return new Set(res.flatMap((r) => (r?.Cards || []).map((c) => c.Title)).filter(Boolean));
}

/**
 * Pełna kontrola kanałów (Na antenie, Playouty): pozycje na 24 h, koniec playoutu, wpisy legacy o nakładaniu (K6),
 * pliki w koszu, CG Playout (audycja LIVE bez wejścia studia, zdjęta grafika).
 * `channels` – kanały legacy, `byNumber` – playouty po numerze kanału, `cgState` – /api/live/state albo null.
 * Zwraca Map(numer kanału → { issues: [{ level, text }], items: Map(klucz → [opis]), gaps }).
 */
export async function scheduleCheck(channels, byNumber, cgState = null) {
  const now = Date.now();
  const [missing, overlaps, inputs] = await Promise.all([
    missingTitles(),
    overlapWarnings(now - 24 * 3600e3),
    S.status?.cg && cgState && !(cgState instanceof Error) ? api('/api/cg/live/inputs').then((r) => r.result?.listeners || null).catch(() => null) : null]);
  const out = new Map();
  await Promise.all(channels.map(async (c) => {
    const p = byNumber.get(c.Number);
    const r = { issues: [], items: new Map(), gaps: [] };
    out.set(String(c.Number), r);
    if (!p) { r.issues.push({ level: 'warn', text: 'kanał nie ma playoutu – nic nie nadaje z ramówki' }); return; }
    let page;
    try { page = await cmd('GetFuturePlayoutItemsById', { PlayoutId: p.PlayoutId, ShowFiller: true, PageNum: 0, PageSize: PAGE }); } catch (e) { r.issues.push({ level: 'warn', text: 'ramówki nie da się odczytać: ' + e.message }); return; }
    const list = page?.Page || [];
    const total = page?.TotalCount ?? list.length;
    let last = list[list.length - 1];
    if (total > list.length) {
      const lp = await cmd('GetFuturePlayoutItemsById', { PlayoutId: p.PlayoutId, ShowFiller: true, PageNum: total - 1, PageSize: 1 }).catch(() => null);
      last = lp?.Page?.[0] || last;
    }
    const lastEnd = last ? +new Date(last.Finish) : 0;
    if (!last) r.issues.push({ level: 'bad', text: 'playout bez przyszłych pozycji – przebuduj go' });
    else if (lastEnd - now < HORIZON_MS) r.issues.push({ level: 'bad', text: `playout kończy się o ${hm(new Date(lastEnd))} (za ${Math.max(0, Math.round((lastEnd - now) / 3600e3))} h) – przebuduj, zanim kanał zostanie bez ramówki` });
    const ci = checkItems(list, { now, missing });
    r.issues.push(...ci.issues);
    r.items = ci.items;
    r.gaps = ci.gaps;
    const ov = overlaps.find((o) => o.channel === c.Name);
    if (ov && !ci.issues.some((i) => /nakładają/.test(i.text))) r.issues.push({ level: 'bad', text: `legacy zgłosił nakładanie pozycji (${hm(new Date(ov.time))}, „Sprawdź nakładanie pozycji”) – przebuduj playout` });
    const cgc = cgState && !(cgState instanceof Error) ? (cgState.channels || []).find((x) => String(x.number) === String(c.Number)) : null;
    const nl = cgc?.next_live;
    if (nl && nl.start > now && nl.start - now < WINDOW_MS && Array.isArray(inputs) && !inputs.some((x) => String(x.channel) === String(c.Number))) {
      r.issues.push({ level: 'bad', text: `audycja LIVE „${nl.title}” o ${hm(new Date(nl.start))} – kanał nie ma wejścia studia (Sloty na żywo → „Ustaw wejścia…”)` });
    }
  }));
  return out;
}

/** Klucz pozycji do `items` z wyniku kontroli. */
export const itemKey = key;
