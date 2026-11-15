// Dziennik emisji (as-run, K14; wzorzec Vizrt C – Run Status): co faktycznie poszło na antenę kanału,
// a obok logi systemu emisji tego kanału i dnia – jedno miejsce dla VizRS i ErsatzTV:
//   antena (as-run)   – CG Playout GET /api/live/asrun?channel=&day=
//   live ingest       – CG Playout GET /api/live/logs?source=ingest (odbiorniki RTMP/SRT: nadawcy, błędy ffmpeg)
//   przejęcia kanału  – CG Playout GET /api/live/logs?source=takeover (przebiegi kawałek/live, tempo, oś czasu)
//   sidecar VizRS CG  – CG Playout GET /api/live/logs?source=cg (wiersze kanału i ogólne)
//   ErsatzTV legacy   – most GetRecentLogEntries (log legacy, wpisy z tego dnia; legacy nie dzieli logu na kanały)
//   wszystko          – powyższe na jednej osi czasu
// Dni w UTC (jak zapisuje CG Playout). Eksport CSV w przeglądarce. Kontrakt: DOKUMENTACJA §3 „Kontrakt CG Playout”.

import { VIEWS, S, h, head, panel, pill, btn, table, toast, api, cmd, has, hms, pad2, cmdNote, onChannel } from '../core.js';

const KIND = {
  item_start: ['start pozycji', 'on'], item_end: ['koniec pozycji', ''], take: ['TAKE strony', 'info'], continue: ['CONTINUE', 'info'], out: ['OUT strony', 'warn'],
  live_on: ['studio na antenie', 'on'], live_off: ['studio zdjęte', 'warn'], hold: ['ZDEJMIJ (studio)', 'warn'], release: ['WPUŚĆ (studio)', 'ok'],
  graphics_off: ['grafika zdjęta', 'bad'], graphics_on: ['grafika przywrócona', 'ok'],
  live_graphics_on: ['przejęcie live – grafika ramówki wstrzymana', 'on'], live_graphics_off: ['koniec przejęcia – grafika ramówki wraca', 'ok'],
};
/** Źródła: [etykieta w wyborze, etykieta w kolumnie „Źródło”]. */
export const SOURCES = {
  asrun: ['antena (dziennik emisji)', 'antena'],
  ingest: ['live ingest – odbiorniki', 'live ingest'],
  takeover: ['przejęcia kanału (ErsatzTV ↔ live)', 'przejęcia'],
  cg: ['sidecar VizRS CG', 'VizRS CG'],
  legacy: ['ErsatzTV legacy', 'ErsatzTV'],
  all: ['wszystko na jednej osi czasu', ''],
};
const LEVEL = { info: ['info', 'info'], warn: ['uwaga', 'warn'], error: ['błąd', 'bad'] };
const LEGACY_LEVEL = { Verbose: 'info', Debug: 'info', Information: 'info', Warning: 'warn', Error: 'error', Fatal: 'error' };
/** Najwięcej stron (po 200) logu legacy czytanych wstecz w poszukiwaniu wybranego dnia. */
const LEGACY_PAGES = 10;
const AS = { channel: '', day: '', src: 'asrun', minLevel: 'info' };
const todayUtc = () => new Date().toISOString().slice(0, 10);

/** Pole CSV: cudzysłów, gdy jest separator, cudzysłów albo nowa linia. */
export const csvCell = (v) => { const s = v == null ? '' : String(v); return /[;"\n\r]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s; };

/** Wpis dziennika anteny jako wiersz logu (do widoku „wszystko” i CSV). */
export const asrunText = (e) => [KIND[e.kind]?.[0] || e.kind, e.title, e.page ? `strona ${e.page}${e.layer != null ? ' · W' + e.layer : ''}` : '', e.operator, e.detail].filter(Boolean).join(' · ');

/** Wpisy logu legacy z jednego dnia UTC: kolejne strony od najnowszych, aż do wpisów sprzed tego dnia. */
async function legacyDay(day) {
  const out = [];
  for (let p = 0; p < LEGACY_PAGES; p++) {
    const d = await cmd('GetRecentLogEntries', { PageNum: p, PageSize: 200, Filter: '' });
    const page = d.Page || [];
    for (const e of page) {
      const t = Date.parse(e.Timestamp);
      if (new Date(t).toISOString().slice(0, 10) === day) out.push({ t, src: 'legacy', level: LEGACY_LEVEL[e.Level] || 'info', text: String(e.Message || '').trim() });
    }
    const oldest = page.length ? Date.parse(page[page.length - 1].Timestamp) : 0;
    if (!page.length || new Date(oldest).toISOString().slice(0, 10) < day || (p + 1) * 200 >= d.TotalCount) break;
  }
  return out;
}

function exportCsv(rows, ch, day, src) {
  const lines = src === 'asrun'
    ? [['czas', 'zdarzenie', 'tytuł', 'strona', 'warstwa', 'operator', 'szczegóły'].join(';'),
      ...rows.map((e) => [new Date(e.t).toISOString(), e.kind, e.title, e.page, e.layer, e.operator, e.detail].map(csvCell).join(';'))]
    : [['czas', 'źródło', 'poziom', 'wpis'].join(';'),
      ...rows.map((e) => [new Date(e.t).toISOString(), SOURCES[e.src]?.[1] || e.src, e.level, e.text].map(csvCell).join(';'))];
  const blob = new Blob(['﻿' + lines.join('\r\n') + '\r\n'], { type: 'text/csv;charset=utf-8' });
  const a = h('a', { href: URL.createObjectURL(blob), download: `${src === 'asrun' ? 'dziennik-emisji' : 'logi-emisji-' + src}-${ch}-${day}.csv` });
  document.body.append(a);
  a.click();
  setTimeout(() => { URL.revokeObjectURL(a.href); a.remove(); }, 1000);
}

VIEWS.asrun = async () => {
  const title = 'Dziennik emisji';
  const desc = 'Co faktycznie poszło na antenę (start i koniec pozycji, strony grafiki, przejęcia studia, zdjęcie grafiki) oraz logi systemu emisji kanału z tego dnia: odbiorniki live ingest, przejęcia kanału przez live, sidecar VizRS CG i ErsatzTV legacy – osobno albo na jednej osi czasu. Dni w UTC; eksport do CSV.';
  if (!S.status.cg) return [head(title, desc), h('div', { class: 'note' }, 'VizRS CG Playout nie jest podłączony (VIZRS_LIVE_CG_URL) – dziennik emisji prowadzi CG Playout.')];
  let st;
  try { st = (await api('/api/cg/live/state')).result; } catch (e) { return [head(title, desc), h('div', { class: 'note err' }, 'VizRS CG Playout: ' + e.message)]; }
  const channels = (st.channels || []).filter((c) => onChannel(c.number));
  const features = Array.isArray(st.features) ? st.features : [];
  if (!features.includes('asrun')) {
    return [head(title, desc), h('div', { class: 'note', 'data-asrun-missing': '' }, 'Ta wersja VizRS CG Playout nie prowadzi dziennika emisji (brak „asrun” w features stanu /api/live/state). Zaktualizuj CG Playout – kontrakt w DOKUMENTACJA §3.')];
  }
  const logs = features.includes('logs');
  const legacy = has('GetRecentLogEntries');
  const available = ['asrun', ...(logs ? ['ingest', 'takeover', 'cg'] : []), ...(legacy ? ['legacy'] : [])];
  if (available.length > 1) available.push('all');
  if (!available.includes(AS.src)) AS.src = 'asrun';
  if (!channels.some((c) => String(c.number) === AS.channel)) AS.channel = String(channels[0]?.number ?? '');
  AS.day ||= todayUtc();

  const src = h('select', { id: 'asrun-src', 'aria-label': 'Źródło' }, available.map((k) => h('option', { value: k }, SOURCES[k][0])));
  src.value = AS.src;
  const sel = h('select', { id: 'asrun-ch', 'aria-label': 'Kanał' }, channels.map((c) => h('option', { value: c.number }, `${c.number} · ${c.name || ''}`)));
  sel.value = AS.channel;
  const day = h('input', { id: 'asrun-day', type: 'date', value: AS.day, 'aria-label': 'Dzień (UTC)', title: 'Dzień w UTC' });
  const lvl = h('select', { id: 'asrun-level', 'aria-label': 'Poziom' },
    h('option', { value: 'info' }, 'wszystkie poziomy'), h('option', { value: 'warn' }, 'od: uwaga'), h('option', { value: 'error' }, 'tylko błędy'));
  lvl.value = AS.minLevel;
  const filter = h('input', { id: 'asrun-q', type: 'search', placeholder: 'filtr: tytuł, strona, operator, treść…', 'aria-label': 'Filtr' });
  const box = h('div', { 'data-asrun': '' });
  const count = h('div', { class: 'pad dim', id: 'asrun-count' });
  let rows = [];
  let shownSrc = AS.src;
  const rank = { info: 0, warn: 1, error: 2 };

  const draw = () => {
    const q = filter.value.trim().toLocaleLowerCase('pl');
    lvl.hidden = shownSrc === 'asrun';
    if (shownSrc === 'asrun') {
      const shown = q ? rows.filter((e) => [e.title, e.page, e.operator, e.detail, KIND[e.kind]?.[0]].some((x) => String(x || '').toLocaleLowerCase('pl').includes(q))) : rows;
      count.textContent = `Wpisów: ${shown.length}${q ? ` (z ${rows.length})` : ''}.`;
      box.replaceChildren(table(shown, [
        [(e) => hms(new Date(e.t)), 'Czas', { class: 'num' }],
        [(e) => { const [t, k] = KIND[e.kind] || [e.kind, '']; return pill(t, k); }, 'Zdarzenie'],
        [(e) => e.title || '', 'Pozycja', { class: 'w' }],
        [(e) => (e.page ? `${e.page}${e.layer != null ? ' · W' + e.layer : ''}` : ''), 'Strona'],
        [(e) => e.operator || '', 'Operator'], [(e) => e.detail || '', 'Szczegóły'],
      ], { empty: 'Brak wpisów w tym dniu.' }));
      return;
    }
    const min = rank[AS.minLevel] ?? 0;
    const shown = rows.filter((e) => (rank[e.level] ?? 0) >= min && (!q || `${e.text} ${SOURCES[e.src]?.[1] || ''}`.toLocaleLowerCase('pl').includes(q)));
    count.textContent = `Wpisów: ${shown.length}${shown.length !== rows.length ? ` (z ${rows.length})` : ''}.`;
    box.replaceChildren(table(shown, [
      [(e) => hms(new Date(e.t)), 'Czas', { class: 'num' }],
      ...(shownSrc === 'all' ? [[(e) => pill(SOURCES[e.src]?.[1] || e.src), 'Źródło']] : []),
      [(e) => { const [t, k] = LEVEL[e.level] || [e.level, '']; return pill(t, k); }, 'Poziom'],
      [(e) => e.text, 'Wpis', { class: 'w log' }],
    ], { empty: 'Brak wpisów w tym dniu.' }));
  };

  const fetchSource = async (k) => {
    const ch = encodeURIComponent(AS.channel);
    if (k === 'asrun') {
      const r = (await api(`/api/cg/live/asrun/${ch}/${AS.day}`)).result;
      return (r?.entries || []).map((e) => ({ ...e, src: 'asrun', level: KIND[e.kind]?.[1] === 'bad' ? 'error' : KIND[e.kind]?.[1] === 'warn' ? 'warn' : 'info', text: asrunText(e) }));
    }
    if (k === 'legacy') return legacyDay(AS.day);
    const r = (await api(`/api/cg/live/logs/${k}/${ch}/${AS.day}`)).result;
    if (r?.truncated) toast(`${SOURCES[k][1]}: pokazane najnowsze wpisy dnia (limit CG Playout).`);
    return (r?.entries || []).map((e) => ({ ...e, src: k }));
  };

  const load = async () => {
    AS.channel = sel.value; AS.day = day.value || todayUtc(); AS.src = src.value;
    shownSrc = AS.src;
    box.replaceChildren(h('div', { class: 'dim pad' }, 'Wczytuję…'));
    count.textContent = '';
    try {
      if (AS.src === 'all') {
        const parts = await Promise.all(available.filter((k) => k !== 'all').map((k) => fetchSource(k).catch((e) => [{ t: Date.now(), src: k, level: 'error', text: `nie wczytano: ${e.message}` }])));
        rows = parts.flat();
      } else {
        rows = await fetchSource(AS.src);
      }
      rows.sort((a, b) => a.t - b.t);
      draw();
    } catch (e) { rows = []; box.replaceChildren(h('div', { class: 'note err' }, (AS.src === 'legacy' ? 'ErsatzTV legacy: ' : 'VizRS CG Playout: ') + e.message)); }
  };
  for (const el of [src, sel, day]) el.addEventListener('change', load);
  lvl.addEventListener('change', () => { AS.minLevel = lvl.value; draw(); });
  filter.addEventListener('input', draw);
  load();
  const notes = ['CG Playout GET /api/live/asrun?channel=&day=', ...(logs ? ['GET /api/live/logs?source=&channel=&day='] : []), ...(legacy ? ['GetRecentLogEntries'] : [])];
  return [
    head(title, desc,
      btn('Odśwież', load),
      btn('Eksport CSV', () => { if (!rows.length) return toast('Brak wpisów do eksportu.', 'bad'); exportCsv(rows, AS.channel, AS.day, shownSrc); }, 'acc')),
    panel('Wpisy', h('span', { class: 'bar asrun-bar' }, src, sel, day, lvl, filter), count, box,
      logs ? null : h('div', { class: 'pad dim' }, 'Logi live ingest, przejęć kanału i sidecara pojawią się po aktualizacji VizRS CG Playout (brak „logs” w features).'),
      cmdNote(...notes)),
  ];
};
