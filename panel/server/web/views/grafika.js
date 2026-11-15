// Widok „Grafika kanału” (R9); Sloty na żywo i Audycje LIVE – views/live.js.
// Dane VizRS CG Playout przez pośrednika serwera /api/cg/… (token CG zostaje w serwerze).

import { VIEWS, S, h, head, panel, pill, btn, table, toast, api, cmd, cmdNote, confirmDlg, dialog, has, route, dateTime, vizrsPreview, onChannel } from '../core.js';
import { cmdEditor, describe, fieldsUsed } from '../cmdedit.js';

/** Zapytanie do CG Playout przez serwer panelu; zwraca `result` albo rzuca ApiError. */
export async function cg(path, method = 'GET', body) {
  return (await api('/api/cg/' + path, { method, body: body == null ? undefined : JSON.stringify(body) })).result;
}

const cgMissing = () => h('div', { class: 'note' },
  'VizRS CG Playout nie jest podłączony. Uruchom panel z --cg-url http://<cg>:7700 i --cg-token <panel_token CG Playout> ' +
  '(VIZRS_LIVE_CG_URL, VIZRS_LIVE_CG_TOKEN); w CG Playout ustaw panel_token (VIZRS_PANEL_TOKEN).');

const cgError = (e) => h('div', { class: 'note err', 'data-cg-error': '' }, 'VizRS CG Playout: ' + e.message);

const sec = (s) => (s == null ? '—' : `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, '0')}`);

// ---------- Grafika kanału ----------
VIEWS.grafika = async () => {
  const [elements, health, live, graphics] = await Promise.all([
    has('GetAllGraphicsElements') ? cmd('GetAllGraphicsElements') : [],
    S.status.cg ? cg('health').catch((e) => e) : null,
    S.status.cg ? cg('live/state').catch(() => null) : null,
    S.status.cg ? cg('live/graphics').catch((e) => e) : null,
  ]);
  const rules = graphics && !(graphics instanceof Error) ? graphics.rules || {} : null;
  const channels = live?.channels || [];
  const pages = rules ? await allPages(channels) : new Map();

  const refresh = has('RefreshGraphicsElements') ? btn('Wczytaj pliki elementów', async () => {
    try { await cmd('RefreshGraphicsElements'); toast('Legacy wczytał elementy graficzne z dysku'); route(true); } catch (e) { toast(e.message, 'bad'); }
  }) : null;

  // strony VizRS kanału (GET /etv/pages?channel=N)
  const pagesBox = h('div', { class: 'pad', 'data-cg-pages': '' });
  const pick = h('select', { 'aria-label': 'Kanał stron VizRS', id: 'cg-pages-channel' },
    channels.map((c) => h('option', { value: c.number }, `${c.number} · ${c.name || 'kanał'}`)));
  const loadPages = async () => {
    if (!pick.value) return pagesBox.replaceChildren(h('div', { class: 'empty' }, 'CG Playout nie zna jeszcze żadnego kanału.'));
    try {
      const r = await cg('pages/' + encodeURIComponent(pick.value));
      pagesBox.replaceChildren(whenTable(r?.pages || [], pick.value, rules, elements, loadPages));
    } catch (e) { pagesBox.replaceChildren(cgError(e)); }
  };
  // kanał z paska górnego jako domyślny wybór stron (i anteny podglądu)
  if (S.channel && channels.some((c) => onChannel(c.number))) pick.value = S.channel;
  pick.addEventListener('change', loadPages);
  if (S.status.cg && !(health instanceof Error)) loadPages();

  const cgPanel = !S.status.cg ? cgMissing() : health instanceof Error ? cgError(health) : [
    panel('Stan VizRS CG Playout', h('span', null, pill('połączony', 'ok')),
      h('dl', { class: 'kv' },
        h('dt', null, 'Sesje elementu VizRS'), h('dd', { 'data-cg-sessions': '' }, String(health.sessions ?? 0)),
        h('dt', null, 'Nakładka ffmpeg'), h('dd', null, (health.overlay || []).length ? (health.overlay || []).map((c) => pill('kanał ' + c, 'info')) : '—'),
        h('dt', null, 'Live ingest'), h('dd', null, health.ingest == null ? pill('wyłączony') : pill('włączony', 'ok'))),
      table(health.captures || [], [['channel', 'Kanał'], ['size', 'Rozmiar'], ['fps', 'kl./s'], ['mode', 'Tryb'], ['drawn_by', 'Rysuje'], ['status', 'Stan']],
        { empty: 'Brak przechwytywania grafiki – żaden kanał nie gra teraz pozycji z elementem VizRS.' }),
      cmdNote('CG Playout GET /health')),
  ];
  const whenPanel = !S.status.cg || health instanceof Error ? null : panel('Kiedy strona wchodzi na antenę', h('span', null, pick), pagesBox,
      h('div', { class: 'hint pad' }, 'Trzy źródła naraz: czas strony z Trio (⏱ kanał – działa przez element legacy), reguły CG Playout z TAKE tej strony i element legacy przypisany w ramówce. Strona z więcej niż jednym źródłem może wejść dwa razy albo o innej porze, niż się wydaje.'),
      cmdNote('CG Playout GET /etv/pages?channel=N', 'GET /api/live/graphics', 'GetProgramScheduleItems', 'GetBlockItems', 'GetDecoById'));

  return [
    head('Grafika kanału', 'Kiedy jaka strona VizRS wchodzi na antenę (reguły CG Playout), stan VizRS CG Playout, strony VizRS, podgląd anteny i elementy graficzne legacy.', refresh),
    whenPanel,
    S.status.cg && !(health instanceof Error) ? rulesPanel(graphics, pages, channels, (live?.features || []).includes('on_live')) : null,
    h('div', { class: 'split wide' },
      h('div', { class: 'cols' }, cgPanel,
        panel('Elementy graficzne legacy', null,
          table(elements, [['Name', 'Nazwa'], [(g) => h('span', { class: 'mono' }, g.FileName), 'Plik']],
            { empty: 'Brak elementów. Element VizRS (vizrs.yml) zakłada CG Playout przy synchronizacji z legacy.' }),
          cmdNote('GetAllGraphicsElements', 'RefreshGraphicsElements'))),
      h('div', { class: 'cols' },
        vizrsPreview('Podgląd anteny VizRS (/output)', [], channels.length ? pick : null)
          || h('div', { class: 'note' }, 'Podgląd /output i Trio: uruchom panel z --vizrs-url (VIZRS_LIVE_VIZRS_URL), VizRS z tokenem – także z --vizrs-output-token.'))),
  ];
};

// ---------- Kiedy strona wchodzi na antenę: czas z Trio, reguły CG Playout, element legacy w ramówce ----------
/** Numer strony VizRS elementu legacy: plik page-<nr>.yml albo nazwa „Strona (nr)” (synchronizacja CG Playout). */
export const elementPage = (g) => {
  const m = /page-(\d+)\.yml$/.exec(g.FileName || '') || /\((\d+)\)\s*$/.exec(g.Name || '');
  return m ? m[1] : null;
};

/** Czas strony na kanale z Trio (`playout` strony VizRS: at, out, ref). */
export function trioTime(p) {
  const t = p?.playout;
  if (!t || (t.at == null && t.out == null)) return null;
  const end = t.ref === 'programme' ? 'audycji (EPG)' : 'pozycji';
  const parts = [];
  if (t.at != null) parts.push(t.at < 0 ? `wchodzi ${-t.at} s przed końcem ${end}` : `wchodzi ${t.at} s po starcie`);
  if (t.out != null) parts.push(`schodzi ${t.out} s przed końcem`);
  return parts.join(', ');
}

/** Reguły CG Playout, które wprowadzają stronę (TAKE z jej numerem), jako „kiedy · zakres”; kanał spoza reguły – pominięty. */
export function rulesTaking(rules, number, channel) {
  if (!rules) return [];
  const takes = (text) => String(text).split('\n').some((line) => {
    const [verb, targets = ''] = line.trim().split(/\s+/);
    return /^take$/i.test(verb || '') && targets.split(',').includes(String(number));
  });
  return ruleRows(rules)
    .filter((x) => x.cmd !== '-' && takes(x.cmd))
    .filter((x) => !x.scope.startsWith('kanał ') || x.scope.startsWith(`kanał ${channel},`) || x.scope === `kanał ${channel}`)
    .map((x) => `${x.when} · ${x.scope}`);
}

// Przypisania elementów graficznych w ramówce legacy (id elementu → [gdzie]); liczone na żądanie, bo to po zapytaniu na harmonogram, blok i deco.
let USES = null;
async function scanUses() {
  const uses = new Map();
  const add = (list, where) => (list || []).forEach((g) => { if (!uses.has(g.Id)) uses.set(g.Id, []); uses.get(g.Id).push(where); });
  const [schedules, blocks, decoGroups] = await Promise.all([
    has('GetAllProgramSchedules') ? cmd('GetAllProgramSchedules') : [],
    has('GetAllBlocks') ? cmd('GetAllBlocks') : [],
    has('GetAllDecoGroups') ? cmd('GetAllDecoGroups') : [],
  ]);
  await Promise.all([
    ...(has('GetProgramScheduleItems') ? schedules : []).map(async (s) => {
      for (const i of await cmd('GetProgramScheduleItems', { Id: s.Id })) add(i.GraphicsElements, `harmonogram „${s.Name}”, pozycja ${(i.Index ?? 0) + 1}`);
    }),
    ...(has('GetBlockItems') ? blocks : []).map(async (b) => {
      for (const i of await cmd('GetBlockItems', { BlockId: b.Id })) add(i.GraphicsElements, `blok „${b.Name}”, pozycja ${(i.Index ?? 0) + 1}`);
    }),
    ...(has('GetDecosByDecoGroupId') && has('GetDecoById') ? decoGroups : []).map(async (g) => {
      for (const d of await cmd('GetDecosByDecoGroupId', { DecoGroupId: g.Id })) {
        const full = await cmd('GetDecoById', { DecoId: d.Id });
        if (full?.GraphicsElementsMode !== 'Disable') add(full?.GraphicsElements, `deco „${d.Name}”`);
      }
    }),
  ]);
  return uses;
}

function whenTable(pages, channel, rules, elements, redraw) {
  const byPage = new Map();
  for (const g of elements || []) { const n = elementPage(g); if (n) byPage.set(n, [...(byPage.get(n) || []), g]); }
  const scan = btn(USES ? 'Sprawdź przypisania ponownie' : 'Sprawdź przypisania w ramówce', async () => {
    scan.disabled = true; scan.textContent = 'Sprawdzam…';
    try { USES = await scanUses(); } catch (e) { toast('Legacy odrzucił odczyt ramówki: ' + e.message, 'bad'); }
    redraw();
  });
  const sources = (p) => {
    const els = byPage.get(String(p.number)) || [];
    const used = USES ? els.flatMap((g) => USES.get(g.Id) || []) : null;
    return { trio: trioTime(p), rules: rulesTaking(rules, p.number, channel), els, used };
  };
  const rows = pages.map((p) => ({ ...p, src: sources(p) }));
  return h('div', { 'data-cg-when': '' },
    table(rows, [
      ['number', 'Nr', { class: 'num' }], ['name', 'Strona'],
      [(p) => p.src.trio || h('span', { class: 'dim' }, 'start pozycji (domyślnie)'), 'Czas z Trio (⏱ kanał)'],
      [(p) => (p.src.rules.length ? h('div', null, p.src.rules.map((x) => h('div', null, x))) : '—'), 'Reguły CG Playout'],
      [(p) => {
        if (!p.src.els.length) return '—';
        const names = p.src.els.map((g) => g.Name).join(', ');
        if (!p.src.used) return h('span', null, names, h('br'), h('span', { class: 'dim' }, 'przypisania: nie sprawdzono'));
        return h('span', null, names, h('br'), p.src.used.length ? h('span', { class: 'dim' }, p.src.used.join('; ')) : h('span', { class: 'dim' }, 'nieprzypisany w ramówce'));
      }, 'Element legacy w ramówce', { class: 'w' }],
      [(p) => {
        const { rules: r, used, els } = p.src;
        if (r.length && used?.length) return pill('reguła i ramówka – może wejść dwa razy', 'warn');
        if (r.length) return pill('reguła CG', 'info');
        if (used?.length) return pill(p.src.trio ? 'ramówka, czas z Trio' : 'ramówka', 'info');
        if (els.length && !used) return h('span', { class: 'dim' }, '? (sprawdź przypisania)');
        return pill('nie wchodzi sama');
      }, 'Skutek'],
    ], { empty: 'Show VizRS nie ma stron dla tego kanału (albo CG Playout nie widzi VizRS).' }),
    h('div', { class: 'bar pad' }, scan, USES ? h('span', { class: 'dim' }, 'przypisania z harmonogramów, bloków i deco legacy (stan z chwili sprawdzenia)') : null));
}

// ---------- Reguły grafiki (CG Playout GET/PUT /api/live/graphics) ----------
// Pole reguły: null = dziedzicz (wartość globalna), '' = wyłączone, tekst = komendy VizRS. W edytorze – wybór grafik
// (cmdedit.js), w CG Playout – ten sam tekst komend co w config.json.
const numField = (v) => { const t = String(v).trim(); return t === '' ? null : Number(t); };

/** Strony show VizRS ze wszystkich kanałów CG Playout (zwykle jeden show) – nr → {name, fields?}. */
async function allPages(channels) {
  const out = new Map();
  const lists = await Promise.all((channels.length ? channels : [{ number: '' }]).map((c) => cg('pages/' + encodeURIComponent(c.number)).catch(() => null)));
  for (const r of lists) {
    for (const p of r?.pages || []) {
      const n = String(p.number);
      if (out.has(n)) continue;
      // pola strony – gdy CG Playout je poda (lista nazw albo mapa nazwa → wartość); inaczej podpowiedzi z bieżących reguł
      const fields = Array.isArray(p.fields) ? p.fields.map(String) : p.fields && typeof p.fields === 'object' ? Object.keys(p.fields) : [];
      out.set(n, { name: p.name || 'Strona', fields });
    }
  }
  return out;
}

/** Wiersze streszczenia „Kiedy / Co / Ile” z reguł CG Playout (stan zapisany). focus = selektor pola w edytorze obok. */
function ruleRows(r) {
  const rows = [];
  const end = (x) => x.end_seconds ?? r.end_seconds ?? 60;
  const what = (v) => (v == null ? null : v === '' || v === '-' ? '-' : v); // brak = jak globalnie, '' = wyłączone
  const add = (scope, x, focus, global) => {
    const ad = [r.ad_match?.path && `ścieżka ~ ${r.ad_match.path}`, r.ad_match?.title && `tytuł ~ ${r.ad_match.title}`].filter(Boolean).join(', ');
    const list = [
      ['on_clip', 'start pozycji', 'wg strony VizRS'],
      ['on_end', `${end(x)} s przed końcem audycji`, `ostatnie ${end(x)} s`],
      ['on_ad', 'przerwa reklamowa' + (global && ad ? ` (${ad})` : ''), 'na czas przerwy'],
      ['on_live', 'przejęcie przez live ingest (grafika ramówki wstrzymana)', 'na czas przejęcia'],
    ];
    for (const [key, when, len] of list) {
      const v = what(x[key]);
      if (v == null) continue;
      rows.push({ scope, when, key, cmd: v, len, focus: focus(key) });
    }
  };
  add('wszystkie kanały', r, (k) => `#gr-${k}`, true);
  (r.shows || []).forEach((x, i) => add(`audycja ~ ${x.show_title}`, x, (k) => `#gr-s${i}-${k}`));
  Object.entries(r.channels || {}).filter(([n]) => onChannel(n)).forEach(([n, x]) => {
    const i = Object.keys(r.channels).indexOf(n);
    add(`kanał ${n}`, x, (k) => `#gr-c${i}-${k}`);
    (x.shows || []).forEach((y) => add(`kanał ${n}, audycja ~ ${y.show_title}`, y, () => null));
  });
  return rows;
}

/** Komendy reguły czytelnie: „▶ Wprowadź 4001 · Pasek” + pola; tekst komend w podpowiedzi. */
function whatCell(x, pages) {
  if (x.cmd === '-') return x.key === 'on_ad' ? pill('OUT – grafika schodzi') : pill('wyłączone');
  return h('div', { class: 'ce-sum', title: x.cmd }, describe(x.cmd, pages).map((d) => h('div', null,
    h('span', { class: 'ce-ic' }, d.icon), ' ', d.raw ? h('span', { class: 'mono' }, d.text) : h('b', null, d.text),
    d.detail ? h('div', { class: 'dim ce-detail' }, d.detail) : null)));
}

function rulesSummary(r, pages) {
  const rows = ruleRows(r);
  const go = (row) => {
    const el = row.focus && document.querySelector(row.focus);
    if (!el) return toast('Ta reguła (audycja w regule kanału) jest tylko w config.json CG Playout – panel jej nie edytuje.');
    const card = el.closest('details');
    if (card) card.open = true;
    el.scrollIntoView({ block: 'center' }); el.focus();
  };
  return panel(S.channel ? `Reguły · kanał ${S.channel}` : 'Reguły · wszystkie kanały', h('span', { class: 'dim' }, 'kliknij wiersz, by edytować'),
    h('div', { class: 'tw', 'data-rules-summary': '' }, table(rows, [
      [(x) => h('span', null, x.when, h('br'), h('span', { class: 'dim' }, x.scope)), 'Kiedy', { class: 'w' }],
      [(x) => whatCell(x, pages), 'Co', { class: 'w' }],
      [(x) => (x.cmd === '-' && x.key !== 'on_ad' ? '—' : x.len), 'Ile'],
    ], { onRow: go, empty: 'Brak reguł – grafika nie zmienia się sama.' })),
    h('div', { class: 'hint pad' }, 'Stan zapisany w CG Playout. Reguła audycji (pierwsza pasująca wg tytułu) i reguła kanału zastępują regułę globalną; „wyłączone” – w tym zakresie nic nie wchodzi.'),
    cmdNote('CG Playout GET /api/live/graphics'));
}

const KEYS = [['on_clip', 'Na start pozycji'], ['on_end', 'Przed końcem audycji'], ['on_ad', 'Na przerwę reklamową']];
// Tylko w regule kanału, gdy CG Playout to umie (`features`: on_live): grafika na czas przejęcia kanału przez live ingest.
const LIVE_KEY = ['on_live', 'Na przejęcie przez live ingest'];

function rulesPanel(doc, pages, cgChannels, onLive) {
  if (doc instanceof Error || !doc) {
    const e = doc || new Error('brak odpowiedzi');
    return panel('Reguły grafiki', null, h('div', { class: 'pad' },
      h('p', { class: 'dim' }, 'Ta wersja VizRS CG Playout nie ma API reguł grafiki (GET/PUT /api/live/graphics) – reguły zmienia się w jego config.json. Zaktualizuj CG Playout.'),
      cgError(e)));
  }
  const r = doc.rules || {};
  // podpowiedzi nazw pól: z komend wszystkich reguł
  const texts = [];
  const collect = (x) => [...KEYS, LIVE_KEY].forEach(([k]) => x?.[k] && texts.push(x[k]));
  collect(r); (r.shows || []).forEach(collect);
  Object.values(r.channels || {}).forEach((x) => { collect(x); (x.shows || []).forEach(collect); });
  const used = fieldsUsed(texts);
  const editor = (id, label, value, o = {}) => cmdEditor({ id, label, value, pages, used, ...o });
  const value = (v) => (v === '-' ? '' : v); // „-” ze starszych wersji panelu = wyłączone

  const g = Object.fromEntries(KEYS.map(([k, l]) => [k, editor(`gr-${k}`, l, value(r[k]) ?? '', { ad: k === 'on_ad' })]));
  const endSec = h('input', { id: 'gr-end_seconds', class: 'num', type: 'number', min: 1, value: r.end_seconds ?? 60, 'aria-label': 'Sekund przed końcem' });
  const adPath = h('input', { id: 'gr-ad_path', class: 'mono', value: r.ad_match?.path ?? '', placeholder: '/reklamy/', 'aria-label': 'Reklama: ścieżka (regex)' });
  const adTitle = h('input', { id: 'gr-ad_title', class: 'mono', value: r.ad_match?.title ?? '', placeholder: 'np. ^Reklama', 'aria-label': 'Reklama: tytuł (regex)' });

  // reguły audycji i kanałów – karty (zwijane), każda z trzema edytorami
  const cards = { shows: [], channels: [] };
  const knownCh = cgChannels.map((c) => String(c.number));
  const card = (kind, x, i, open) => {
    const p = kind === 'shows' ? 's' : 'c';
    const keys = kind === 'channels' && onLive ? [...KEYS, LIVE_KEY] : KEYS;
    // on_live nie ma wartości globalnej: „nic nie wchodzi” albo komendy (np. TAKE 7510 – logo NA ŻYWO)
    const eds = Object.fromEntries(keys.map(([k, l]) => [k, editor(`gr-${p}${i}-${k}`, `${l} (${kind === 'shows' ? 'audycja' : 'kanał'} ${i + 1})`,
      k === 'on_live' ? x[k] ?? '' : value(x[k]), { scoped: k !== 'on_live', ad: k === 'on_ad' })]));
    let key;
    if (kind === 'shows') key = h('input', { class: 'mono', value: x.show_title ?? '', placeholder: 'np. ^Wiadomości', 'aria-label': `Audycja (regex tytułu) ${i + 1}` });
    else {
      key = h('select', { 'aria-label': `Kanał ${i + 1}` }, h('option', { value: '' }, 'wybierz kanał…'),
        [...new Set([...knownCh, ...(x.channel ? [String(x.channel)] : [])])].map((n) => h('option', { value: n }, `kanał ${n}${cgChannels.find((c) => String(c.number) === n)?.name ? ' · ' + cgChannels.find((c) => String(c.number) === n).name : ''}`)));
      key.value = x.channel ?? '';
    }
    const end = h('input', { class: 'num', type: 'number', min: 1, value: x.end_seconds ?? '', placeholder: 'jak globalnie', 'aria-label': `s przed końcem ${kind === 'shows' ? 'audycji' : 'kanału'} ${i + 1}` });
    const title = h('span', { class: 'rc-title' });
    const sync = () => { title.textContent = kind === 'shows' ? (key.value ? `audycja ~ ${key.value}` : 'nowa reguła audycji') : key.value ? `kanał ${key.value}` : 'nowa reguła kanału'; };
    key.addEventListener('input', sync); key.addEventListener('change', sync); sync();
    const entry = { x, eds, key, end, el: null };
    const rm = h('button', { type: 'button', class: 'danger', 'aria-label': `Usuń regułę ${kind === 'shows' ? 'audycji' : 'kanału'} ${i + 1}`,
      onclick: (e) => { e.preventDefault(); entry.removed = true; entry.el.remove(); } }, 'Usuń');
    entry.el = h('details', { class: 'rule-card', open: open || undefined },
      h('summary', null, title, h('span', { class: 'grow' }), rm),
      h('div', { class: 'form' },
        h('label', null, kind === 'shows' ? 'Tytuł audycji (regex)' : 'Kanał'), key,
        keys.flatMap(([k, l]) => [h('label', { for: `gr-${p}${i}-${k}` }, l), eds[k].el]),
        h('label', null, 'Sekund przed końcem'), end),
      keys.includes(LIVE_KEY) ? h('div', { class: 'hint pad' }, 'Przejęcie przez live ingest: grafika ramówki schodzi i nie wchodzi do końca przejęcia, zamiast niej wchodzi ta (np. logo NA ŻYWO). Po przejęciu wraca grafika ramówki.') : null);
    cards[kind].push(entry);
    return entry.el;
  };
  const showsBox = h('div', { class: 'rule-cards', 'data-rules-shows': '' }, (r.shows || []).map((x, i) => card('shows', { ...x }, i)));
  const chBox = h('div', { class: 'rule-cards', 'data-rules-channels': '' },
    Object.entries(r.channels || {}).map(([channel, x], i) => card('channels', { channel, ...x }, i)));
  const addCard = (kind, box) => { box.append(card(kind, kind === 'shows' ? { show_title: '' } : { channel: '' }, cards[kind].length, true)); box.lastElementChild.querySelector('input, select')?.focus(); };

  const save = async () => {
    let body;
    try {
      const live = (kind) => cards[kind].filter((c) => !c.removed);
      const scoped = (c) => {
        const o = {};
        for (const k of Object.keys(c.eds)) { const v = c.eds[k].read(); if (v != null && !(k === 'on_live' && v === '')) o[k] = v; }
        if (!c.eds.on_live && c.x.on_live) o.on_live = c.x.on_live; // CG Playout bez on_live w features – bez zmian
        const e = numField(c.end.value);
        if (e != null && !Number.isNaN(e)) o.end_seconds = e;
        if (c.x.shows) o.shows = c.x.shows; // reguły audycji w regule kanału – tylko z config.json, bez zmian
        return o;
      };
      if (live('shows').some((c) => !c.key.value.trim())) throw new Error('Reguła audycji wymaga wzorca tytułu.');
      if (live('channels').some((c) => !c.key.value)) throw new Error('Reguła kanału wymaga numeru kanału.');
      body = {
        on_clip: g.on_clip.read() ?? '', on_end: g.on_end.read() ?? '', on_ad: g.on_ad.read() ?? '',
        end_seconds: numField(endSec.value) ?? 60,
        ad_match: { path: adPath.value.trim(), title: adTitle.value.trim() },
        shows: live('shows').map((c) => ({ show_title: c.key.value.trim(), ...scoped(c) })),
        channels: Object.fromEntries(live('channels').map((c) => [c.key.value, scoped(c)])),
      };
    } catch (e) { return toast(e.message, 'bad'); }
    try { await cg('live/graphics', 'PUT', body); toast('Reguły grafiki zapisane w CG Playout – działają od następnej pozycji'); route(true); } catch (e) { toast('CG Playout odrzucił reguły: ' + e.message, 'bad'); }
  };
  return h('div', { class: 'rules-stack' }, rulesSummary(r, pages), panel('Edytor reguł', h('span', null, doc.source === 'file' ? pill('z panelu', 'ok') : pill('z config.json')),
    h('div', { class: 'pad', 'data-rules': '' },
      h('p', { class: 'dim' }, 'Wybierz strony VizRS, które wchodzą na antenę: akcja (Wprowadź, Dalej, Zdejmij, Podmień dane), strona z show i pola strony – tekst albo symbol ramówki {…} (tytuł, następny program, godzina). ' +
        'Kroki idą po kolei. „Tekst” pokazuje te same komendy w składni CG Playout. Zmiana działa od następnej pozycji, bez restartu CG Playout.'),
      pages.size ? null : h('div', { class: 'note' }, 'CG Playout nie podał stron show VizRS – wybór stron jest pusty; zostaje „Komenda tekstowa” i tryb „Tekst”.'),
      h('div', { class: 'form' },
        ...KEYS.flatMap(([k, l]) => [h('label', { for: `gr-${k}` }, l), g[k].el]),
        h('label', { for: 'gr-end_seconds' }, 'Sekund przed końcem'), endSec,
        h('label', { for: 'gr-ad_path' }, 'Reklama – ścieżka (regex)'), adPath,
        h('label', { for: 'gr-ad_title' }, 'Reklama – tytuł (regex)'), adTitle),
      h('h3', null, 'Reguły audycji (pierwsza pasująca)'), showsBox,
      h('div', { class: 'bar' }, btn('+ Reguła audycji', () => addCard('shows', showsBox))),
      h('h3', null, 'Reguły kanałów'), chBox,
      h('div', { class: 'bar' }, btn('+ Reguła kanału', () => addCard('channels', chBox))),
      doc.store ? null : h('div', { class: 'note' }, 'CG Playout nie ma etv_config – reguł nie zapisze.'),
      h('div', { class: 'bar end' }, btn('Zapisz reguły grafiki', save, 'acc'))),
    cmdNote('CG Playout GET/PUT /api/live/graphics')));
}

// Rodzaj elementu legacy z katalogu pliku (graphics-elements/<rodzaj>/…) – legacy nie podaje obrazu elementu.
const GE_KINDS = { image: 'obraz', text: 'tekst', motion: 'ruch', script: 'skrypt', subtitle: 'napisy', subtitles: 'napisy' };

/**
 * Opcje wyboru elementów graficznych z miniaturą (form.js, kind 'multi'): strona VizRS – obraz strony z CG Playout
 * (`/api/cg/live/thumbs/<nr>.png`, renderer natywny; bez CG albo miniatury – napis „VizRS n”), inny element – rodzaj z katalogu.
 */
export const graphicsChoices = (list) => (list || []).map((g) => {
  const page = elementPage(g);
  if (page) return [g.Id, g.Name, { badge: 'VizRS ' + page, src: S.status?.cg ? `/api/cg/live/thumbs/${page}.png` : null }];
  const dir = /(?:^|\/)(image|text|motion|script|subtitles?)\//i.exec(g.FileName || '')?.[1]?.toLowerCase();
  return [g.Id, g.Name, { badge: GE_KINDS[dir] || 'YAML' }];
});
