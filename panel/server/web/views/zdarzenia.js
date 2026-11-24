// Zdarzenia wtórne (K15) z ważnością i zatwierdzaniem (K16) – reguły CG Playout `secondary` w GET/PUT /api/live/graphics.
// Edytor na ekranie Grafika kanału, oś pozycji (gdzie zdarzenia wchodzą względem startu i końca) i karta pozycji w EPG.
// Panel nie zna show_title pozycji legacy – w EPG wzorce sprawdza na tytule pozycji (przybliżenie, opisane w karcie).

import { S, h, panel, pill, btn, toast, api, route, cmdNote, dialog } from '../core.js';

const cgPut = async (doc) => (await api('/api/cg/live/graphics', { method: 'PUT', body: JSON.stringify(doc) })).result;

/** Dzisiejsza data lokalna RRRR-MM-DD (ważność K16 liczy CG Playout po dacie lokalnej startu pozycji). */
export const localDay = (d = new Date()) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
const addDays = (day, n) => { const d = new Date(day + 'T12:00:00'); d.setDate(d.getDate() + n); return localDay(d); };

/** Stan reguły w dniu `day`: { on: czy idzie na antenę, pills: [[tekst, rodzaj]], waiting, expiring }. */
export function ruleState(r, day = localDay()) {
  const pills = [];
  let on = true;
  const waiting = r.approved === null;
  if (waiting) { pills.push(['czeka na zatwierdzenie', 'warn']); on = false; }
  else if (r.approved && typeof r.approved === 'object') pills.push([`zatwierdził ${r.approved.by}`, 'ok']);
  if (r.valid_from && r.valid_from > day) { pills.push([`od ${r.valid_from}`, 'info']); on = false; }
  let expiring = false;
  if (r.valid_to) {
    if (r.valid_to < day) { pills.push(['wygasła ' + r.valid_to, 'bad']); on = false; }
    else if (r.valid_to === day) { pills.push(['wygasa dziś', 'warn']); expiring = true; }
    else if (r.valid_to === addDays(day, 1)) { pills.push(['wygasa jutro', 'warn']); expiring = true; }
    else if (r.valid_to <= addDays(day, 7)) { pills.push(['do ' + r.valid_to, 'info']); expiring = true; }
  }
  return { on, pills, waiting, expiring };
}

const evOn = (e, day) => e.approved !== null && !(e.valid_from && e.valid_from > day) && !(e.valid_to && e.valid_to < day);
const regex = (src) => { try { return src ? new RegExp(src, 'i') : null; } catch { return undefined; } };

/**
 * Zdarzenia reguł pasujących do pozycji (kanał, tytuł, start; panel nie zna show_title – wzorce tytułów na tytule).
 * Reguły ze wzorcem ścieżki pliku pomija (legacy nie podaje ścieżki pozycji) – liczy je w `skipped`.
 */
export function eventsFor(rules, channel, title, day = localDay(), start = null) {
  const out = [];
  out.skipped = 0;
  for (const r of rules || []) {
    if (r.channel && String(r.channel) !== String(channel)) continue;
    const t = r.target || {};
    if (t.start && (start == null || Math.abs(Date.parse(t.start) - start) > 2000)) continue;
    if (t.path) { out.skipped++; continue; }
    const pats = [t.show_title, t.title].filter(Boolean).map(regex);
    if (!pats.length && !t.start) continue;
    if (pats.some((p) => !p || !p.test(title))) continue;
    const st = ruleState(r, day);
    for (const e of r.events || []) out.push({ rule: r, ev: e, on: st.on && evOn(e, day) });
  }
  return out;
}

const when = (e) => (e.anchor === 'end' ? `${e.offset_s ?? 0} s przed końcem` : `${e.offset_s ?? 0} s po starcie`) + `, ${e.duration_s} s`;

/**
 * Oś pozycji: zdarzenia od startu (lewa część) i przed końcem (prawa). `length` (s) – prawdziwa długość pozycji;
 * bez niej obie części mają skalę z samych zdarzeń, a środek pozycji jest umowny („…”).
 */
export function axis(events, length) {
  const bar = h('div', { class: 'se-axis', 'data-se-axis': '' }, h('span', { class: 'se-lab' }, 'start'), h('span', { class: 'se-lab end' }, 'koniec'));
  const span = (e) => (e.anchor === 'end' ? [length != null ? length - e.offset_s : -e.offset_s, (length != null ? length - e.offset_s : -e.offset_s) + e.duration_s] : [e.offset_s, e.offset_s + e.duration_s]);
  let place;
  if (length != null && length > 0) {
    place = ([a, b]) => [Math.max(0, a / length), Math.min(1, b / length)];
  } else {
    const head = Math.max(30, ...events.filter((x) => x.ev.anchor !== 'end').map((x) => x.ev.offset_s + x.ev.duration_s));
    const tail = Math.max(30, ...events.filter((x) => x.ev.anchor === 'end').map((x) => x.ev.offset_s));
    bar.append(h('span', { class: 'se-mid' }, '…'));
    place = ([a, b], end) => (end ? [0.55 + 0.45 * (1 + a / tail), 0.55 + 0.45 * Math.min(1, 1 + b / tail)] : [0.45 * (a / head), 0.45 * Math.min(1, b / head)]);
  }
  events.forEach((x, i) => {
    const [a, b] = place(span(x.ev), x.ev.anchor === 'end');
    const m = h('span', { class: 'se-ev' + (x.on ? '' : ' off') + (i % 2 ? ' alt' : ''), title: `strona ${x.ev.page}: ${when(x.ev)}${x.on ? '' : ' – nie wchodzi (zatwierdzenie albo ważność)'}` }, String(x.ev.page));
    m.style.left = (Math.max(0, Math.min(1, a)) * 100).toFixed(2) + '%';
    m.style.width = `max(34px, ${(Math.max(0.005, b - a) * 100).toFixed(2)}%)`;
    bar.append(m);
  });
  return bar;
}

// ---------- „Dodaj zdarzenie” z karty pozycji w EPG: nowa reguła na ekranie Grafika kanału ----------
let PREFILL = null;
const escapeRe = (t) => String(t).replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

/** Karta pozycji w EPG: pasujące zdarzenia wtórne na osi pozycji i „Dodaj zdarzenie”. */
export async function itemCard(it, channel) {
  const s = +new Date(it.Start), e = +new Date(it.Finish);
  let doc = null;
  if (S.status?.cg) doc = await api('/api/cg/live/graphics').then((r) => r.result).catch(() => null);
  const state = S.status?.cg ? await api('/api/cg/live/state').then((r) => r.result).catch(() => null) : null;
  const feat = (state?.features || []).includes('events');
  const list = feat && doc ? eventsFor(doc.rules?.secondary, channel, it.Title || '', localDay(new Date(s)), s) : [];
  const hm = (t) => new Date(t).toLocaleTimeString('pl-PL', { hour: '2-digit', minute: '2-digit' });
  const body = h('div', { 'data-item-card': '' },
    h('p', null, h('b', null, it.Title || '—'), ` · kanał ${channel} · ${hm(s)}–${hm(e)} (${Math.round((e - s) / 60e3)} min)`),
    !feat ? h('div', { class: 'note' }, 'Zdarzenia wtórne: ta wersja VizRS CG Playout ich nie ma (brak „events” w features) albo CG Playout nie jest podłączony.')
      : list.length ? [axis(list, (e - s) / 1000),
        h('ul', { class: 'se-list' }, list.map((x) => h('li', null, h('b', null, `strona ${x.ev.page}`), ` – ${when(x.ev)} · ${x.rule.name || 'reguła bez nazwy'}`, x.on ? null : [' ', pill('nie wchodzi', 'warn')])))]
        : h('p', { class: 'dim' }, 'Żadna reguła zdarzeń wtórnych nie pasuje do tej pozycji.'),
    list.skipped ? h('p', { class: 'dim' }, `Reguły po ścieżce pliku (${list.skipped}) – nie sprawdzone tutaj, legacy nie podaje ścieżki pozycji.`) : null,
    feat ? h('p', { class: 'hint' }, 'Wzorce reguł panel sprawdza na tytule pozycji; CG Playout porównuje „show_title” z tytułem audycji (serialu) – przy odcinkach wynik może się różnić.') : null);
  const v = await dialog('Pozycja ramówki', body, feat
    ? [['Zdarzenie: tylko ta pozycja', 'one'], ['Zdarzenie: każda emisja tytułu', 'add', 'acc'], ['Zamknij', false]] : [['Zamknij', false]]);
  if (v === 'add' || v === 'one') {
    PREFILL = v === 'one'
      ? { name: `${it.Title || ''} · ${hm(s)}`, channel: String(channel), start: new Date(s).toISOString() }
      : { name: it.Title || '', channel: String(channel), title: '^' + escapeRe(it.Title || '') + '$' };
    if (location.hash === '#grafika') route(true); else location.hash = '#grafika';
  }
}

// ---------- edytor na ekranie Grafika kanału ----------
const fieldsText = (v) => Object.entries(v || {}).map(([k, x]) => `${k}=${x}`).join('\n');
function readFields(text, where) {
  const o = {};
  for (const line of text.split('\n').map((l) => l.trim()).filter(Boolean)) {
    const i = line.indexOf('=');
    if (i < 1) throw new Error(`${where}: pole strony jako „nazwa=wartość” (wiersz „${line}”)`);
    o[line.slice(0, i).trim()] = line.slice(i + 1);
  }
  return Object.keys(o).length ? o : undefined;
}
const num = (v) => (String(v).trim() === '' ? NaN : Number(v));

/**
 * Panel „Zdarzenia wtórne”: `doc` – odpowiedź GET /api/live/graphics, `pages` – strony VizRS (nr → {name}),
 * `features` – features stanu CG. Zapis to PUT całego dokumentu reguł z nową listą `secondary` (reszta bez zmian).
 */
export function secondaryPanel(doc, pages, cgChannels, features) {
  if (!features.includes('events')) {
    return panel('Zdarzenia wtórne', null, h('div', { class: 'note', 'data-se-missing': '' },
      'Ta wersja VizRS CG Playout nie ma zdarzeń wtórnych (brak „events” w features stanu /api/live/state) – zaktualizuj CG Playout (≥ 26.11.29).'));
  }
  const approval = features.includes('approval');
  // K16: zatwierdza administrator (panel bez kont i token awaryjny = administrator); serwer panelu pilnuje tego samego
  const canApprove = !S.auth?.user || S.auth.user.role === 'admin';
  const day = localDay();
  const rules = (doc.rules?.secondary || []).map((r) => structuredClone(r));
  const cards = [];
  const box = h('div', { class: 'rule-cards', 'data-se-rules': '' });
  const filter = h('select', { 'aria-label': 'Filtr zdarzeń wtórnych', id: 'se-filter', class: 'se-filter' },
    h('option', { value: '' }, 'wszystkie'), h('option', { value: 'waiting' }, 'czekają na zatwierdzenie'),
    h('option', { value: 'expiring' }, 'wygasają w ciągu 7 dni'), h('option', { value: 'off' }, 'nie wchodzą (ważność, zatwierdzenie)'));
  const applyFilter = () => cards.forEach((c) => {
    const st = ruleState(c.snapshot(), day);
    const f = filter.value;
    c.el.hidden = c.removed || (f === 'waiting' && !st.waiting) || (f === 'expiring' && !st.expiring) || (f === 'off' && st.on);
  });
  filter.addEventListener('change', applyFilter);

  const pageOptions = (value) => {
    const opts = [...pages.entries()].map(([n, p]) => h('option', { value: n }, `${n} · ${p.name}`));
    if (value && !pages.has(String(value))) opts.unshift(h('option', { value: String(value) }, `${value} (spoza show)`));
    return opts;
  };

  const card = (r, i, open) => {
    const c = { r, removed: false, events: [], el: null };
    const id = (k) => `se-${i}-${k}`;
    const name = h('input', { id: id('name'), value: r.name || '', placeholder: 'np. Wiadomości – belki', 'aria-label': `Nazwa reguły ${i + 1}` });
    const ch = h('select', { id: id('channel'), 'aria-label': `Kanał reguły ${i + 1}` }, h('option', { value: '' }, 'wszystkie kanały'),
      [...new Set([...cgChannels.map((x) => String(x.number)), ...(r.channel ? [String(r.channel)] : [])])].map((n) => h('option', { value: n }, `kanał ${n}`)));
    ch.value = r.channel || '';
    const show = h('input', { id: id('show'), class: 'mono', value: r.target?.show_title || '', placeholder: 'np. ^Wiadomości', 'aria-label': `Tytuł audycji (regex) ${i + 1}` });
    const title = h('input', { id: id('title'), class: 'mono', value: r.target?.title || '', placeholder: 'np. Sport', 'aria-label': `Tytuł pozycji (regex) ${i + 1}` });
    const path = h('input', { id: id('path'), class: 'mono', value: r.target?.path || '', placeholder: 'np. /media/seriale/', 'aria-label': `Ścieżka pliku (regex) ${i + 1}` });
    const start = h('input', { id: id('start'), class: 'mono', value: r.target?.start || '', placeholder: 'puste = każda pasująca pozycja', 'aria-label': `Tylko pozycja o starcie ${i + 1}` });
    const from = h('input', { id: id('from'), type: 'date', value: r.valid_from || '', 'aria-label': `Ważna od ${i + 1}` });
    const to = h('input', { id: id('to'), type: 'date', value: r.valid_to || '', 'aria-label': `Ważna do ${i + 1}` });
    // zatwierdzanie: brak klucza = bez zatwierdzania, null = czeka, {by, at} = zatwierdzona
    let approved = 'approved' in r ? r.approved : undefined;
    const apprBox = h('span', { class: 'bar' });
    const drawApproval = () => {
      const kids = [];
      if (approved === undefined) {
        kids.push(h('span', { class: 'dim' }, 'bez zatwierdzania'), btn('Wymagaj zatwierdzenia', () => { approved = null; redraw(); }));
      } else if (approved === null) {
        kids.push(pill('czeka na zatwierdzenie', 'warn'),
          canApprove ? btn('Zatwierdź', () => { approved = { by: S.auth?.user?.name || 'administrator', at: new Date().toISOString() }; redraw(); }, 'acc')
            : h('span', { class: 'dim' }, 'zatwierdza administrator'),
          btn('Bez zatwierdzania', () => { approved = undefined; redraw(); }));
      } else {
        kids.push(pill(`zatwierdził ${approved.by}${approved.at ? ' · ' + new Date(approved.at).toLocaleString('pl-PL') : ''}`, 'ok'),
          btn('Cofnij zatwierdzenie', () => { approved = null; redraw(); }));
        if (!canApprove) kids.push(h('span', { class: 'dim' }, 'zmiana treści cofnie zatwierdzenie – ponownie zatwierdzi administrator'));
      }
      apprBox.replaceChildren(...kids);
    };
    const evBox = h('div', { class: 'se-events' });
    const axisBox = h('div', null);
    const stateBox = h('span', { class: 'bar' });
    const titleEl = h('span', { class: 'rc-title' });

    const evRow = (e, j) => {
      const row = { e, removed: false };
      const page = h('select', { 'aria-label': `Strona zdarzenia ${j + 1} (reguła ${i + 1})` }, pageOptions(e.page));
      page.value = e.page != null ? String(e.page) : (pages.keys().next().value ?? '');
      const pageText = pages.size ? null : h('input', { class: 'mono num', value: e.page ?? '', placeholder: 'strona', 'aria-label': `Numer strony zdarzenia ${j + 1} (reguła ${i + 1})` });
      const anchor = h('select', { 'aria-label': `Od kiedy zdarzenie ${j + 1} (reguła ${i + 1})` }, h('option', { value: 'start' }, 'po starcie'), h('option', { value: 'end' }, 'przed końcem'));
      anchor.value = e.anchor === 'end' ? 'end' : 'start';
      const offset = h('input', { class: 'num', type: 'number', min: 0, step: 'any', value: e.offset_s ?? 0, 'aria-label': `Sekund (zdarzenie ${j + 1}, reguła ${i + 1})` });
      const dur = h('input', { class: 'num', type: 'number', min: 0.1, step: 'any', value: e.duration_s ?? 10, 'aria-label': `Czas na antenie (zdarzenie ${j + 1}, reguła ${i + 1})` });
      const vals = h('textarea', { class: 'mono', rows: 2, placeholder: 'pole=wartość (np. f0={next_title})', 'aria-label': `Pola strony (zdarzenie ${j + 1}, reguła ${i + 1})` });
      vals.value = fieldsText(e.values);
      row.read = (where) => {
        const p = (pageText ? pageText.value : page.value).trim();
        if (!p) throw new Error(`${where}: wybierz stronę`);
        const out = { ...e, page: p, anchor: anchor.value, offset_s: num(offset.value), duration_s: num(dur.value) };
        if (!(out.offset_s >= 0)) throw new Error(`${where}: sekundy od 0`);
        if (!(out.duration_s > 0)) throw new Error(`${where}: czas na antenie większy od 0`);
        const v = readFields(vals.value, where);
        if (v) out.values = v; else delete out.values;
        return out;
      };
      row.peek = () => { try { return row.read(''); } catch { return null; } };
      const rm = h('button', { type: 'button', class: 'danger', 'aria-label': `Usuń zdarzenie ${j + 1} (reguła ${i + 1})`, onclick: () => { row.removed = true; row.el.remove(); redraw(); } }, '×');
      row.el = h('div', { class: 'se-ev-row' }, pageText || page, anchor, offset, h('span', { class: 'dim' }, 's, na'), dur, h('span', { class: 'dim' }, 's'), vals, rm,
        e.approved === null || e.valid_from || e.valid_to ? h('span', { class: 'dim' }, 'zdarzenie ma własną ważność/zatwierdzenie (z config.json) – zostaje') : null);
      for (const x of [page, anchor, offset, dur, pageText].filter(Boolean)) x.addEventListener('input', () => redraw());
      c.events.push(row);
      return row.el;
    };

    c.snapshot = () => {
      const o = { name: name.value.trim(), target: {}, events: c.events.filter((x) => !x.removed).map((x) => x.peek()).filter(Boolean) };
      if (ch.value) o.channel = ch.value;
      if (show.value.trim()) o.target.show_title = show.value.trim();
      if (title.value.trim()) o.target.title = title.value.trim();
      if (path.value.trim()) o.target.path = path.value.trim();
      if (start.value.trim()) o.target.start = start.value.trim();
      if (from.value) o.valid_from = from.value;
      if (to.value) o.valid_to = to.value;
      if (approved !== undefined) o.approved = approved;
      return o;
    };
    c.read = (n) => {
      const where = `Reguła ${n} („${name.value.trim() || 'bez nazwy'}”)`;
      const o = c.snapshot();
      if (!o.target.show_title && !o.target.title && !o.target.path && !o.target.start) throw new Error(`${where}: podaj tytuł audycji albo pozycji, ścieżkę pliku albo start pozycji`);
      for (const [k, v] of Object.entries(o.target)) if (k !== 'start' && regex(v) === undefined) throw new Error(`${where}: „${v}” to nie jest poprawne wyrażenie regularne (${k})`);
      if (o.target.start && Number.isNaN(Date.parse(o.target.start))) throw new Error(`${where}: start pozycji jako data i godzina ISO (np. 2026-11-24T20:00:00Z)`);
      if (o.target.start && !o.channel) throw new Error(`${where}: reguła jednej pozycji wymaga kanału`);
      const evs = c.events.filter((x) => !x.removed);
      if (!evs.length) throw new Error(`${where}: dodaj co najmniej jedno zdarzenie`);
      o.events = evs.map((x, j) => x.read(`${where}, zdarzenie ${j + 1}`));
      if (o.valid_from && o.valid_to && o.valid_from > o.valid_to) throw new Error(`${where}: „ważna od” jest po „ważna do”`);
      // zmiana treści zatwierdzonej reguły cofa zatwierdzenie (promo idzie na antenę w zatwierdzonej postaci)
      const canon = (x) => (Array.isArray(x) ? x.map(canon) : x && typeof x === 'object'
        ? Object.fromEntries(Object.keys(x).sort().filter((k) => x[k] !== undefined).map((k) => [k, canon(x[k])])) : x);
      const strip = (x) => JSON.stringify(canon({ ...x, name: undefined, approved: undefined }));
      if (o.approved && typeof o.approved === 'object' && c.original && strip(o) !== strip(c.original)) { o.approved = null; c.reapprove = true; }
      return o;
    };
    c.original = r.events ? structuredClone(r) : null;
    const redraw = () => {
      drawApproval();
      const snap = c.snapshot();
      const st = ruleState(snap, day);
      titleEl.textContent = snap.name || (snap.target.start ? `pozycja ${new Date(snap.target.start).toLocaleString('pl-PL')}` : snap.target.show_title ? `audycja ~ ${snap.target.show_title}`
        : snap.target.title ? `pozycja ~ ${snap.target.title}` : snap.target.path ? `plik ~ ${snap.target.path}` : 'nowa reguła');
      stateBox.replaceChildren(...st.pills.map(([t, k]) => pill(t, k)), ...(st.on ? [] : [pill('nie wchodzi', 'warn')]));
      axisBox.replaceChildren(axis(snap.events.map((ev) => ({ ev, on: st.on && evOn(ev, day) })), null));
      applyFilter();
    };
    for (const x of [name, ch, show, title, path, start, from, to]) { x.addEventListener('input', redraw); x.addEventListener('change', redraw); }
    (r.events || []).forEach((e, j) => evBox.append(evRow(e, j)));
    const rm = h('button', { type: 'button', class: 'danger', 'aria-label': `Usuń regułę zdarzeń ${i + 1}`, onclick: (ev) => { ev.preventDefault(); c.removed = true; c.el.remove(); } }, 'Usuń');
    c.el = h('details', { class: 'rule-card', open: open || undefined, 'data-se-rule': String(i) },
      h('summary', null, titleEl, stateBox, h('span', { class: 'grow' }), rm),
      h('div', { class: 'form' },
        h('label', { for: id('name') }, 'Nazwa'), name,
        h('label', { for: id('channel') }, 'Kanał'), ch,
        h('label', { for: id('show') }, 'Tytuł audycji (regex)'), show,
        h('label', { for: id('title') }, 'Tytuł pozycji (regex)'), title,
        h('label', { for: id('path') }, 'Ścieżka pliku (regex)'), path,
        h('label', { for: id('start') }, 'Tylko pozycja o starcie'), start,
        approval ? [h('label', { for: id('from') }, 'Ważna od'), from, h('label', { for: id('to') }, 'Ważna do'), to, h('label', null, 'Zatwierdzenie'), apprBox] : null),
      h('h4', null, 'Zdarzenia'), evBox,
      h('div', { class: 'bar' }, btn('+ Zdarzenie', () => { evBox.append(evRow({ anchor: 'start', offset_s: 10, duration_s: 10 }, c.events.length)); redraw(); })),
      h('div', { class: 'pad' }, h('div', { class: 'dim' }, 'Oś pozycji (start i koniec – środek umowny):'), axisBox));
    cards.push(c);
    redraw();
    return c.el;
  };

  rules.forEach((r, i) => box.append(card(r, i)));
  if (PREFILL) {
    const p = PREFILL; PREFILL = null;
    box.append(card({ name: p.name, channel: p.channel, target: p.start ? { start: p.start } : { title: p.title }, events: [{ page: pages.keys().next().value ?? '', anchor: 'start', offset_s: 10, duration_s: 10 }] }, cards.length, true));
    setTimeout(() => box.lastElementChild?.scrollIntoView({ block: 'center' }), 0);
  }

  const save = async () => {
    let list;
    try { list = cards.filter((c) => !c.removed).map((c, n) => c.read(n + 1)); } catch (e) { return toast(e.message, 'bad'); }
    try {
      await cgPut({ ...doc.rules, secondary: list });
      toast(cards.some((c) => c.reapprove) ? 'Zdarzenia zapisane – zmieniona zatwierdzona reguła czeka na ponowne zatwierdzenie' : 'Zdarzenia wtórne zapisane w CG Playout – działają od następnej pozycji');
      route(true);
    } catch (e) { toast('CG Playout odrzucił zdarzenia: ' + e.message, 'bad'); }
  };
  const counts = rules.map((r) => ruleState(r, day));
  return panel('Zdarzenia wtórne', h('span', { class: 'bar' },
    counts.some((s) => s.waiting) ? pill(`czeka: ${counts.filter((s) => s.waiting).length}`, 'warn') : null,
    counts.some((s) => s.expiring) ? pill(`wygasa: ${counts.filter((s) => s.expiring).length}`, 'warn') : null,
    approval ? filter : null),
  h('div', { class: 'pad', 'data-se': '' },
    h('p', { class: 'dim' }, 'Strony VizRS w zaplanowanych chwilach pozycji wybranych po tytule: kilka zdarzeń na pozycję, np. belka tytułowa 10 s po starcie i „Za chwilę” 30 s przed końcem. ' +
      'Każde zdarzenie wchodzi raz na pozycję i schodzi po swoim czasie (najpóźniej z końcem pozycji). Ta sama strona dwa razy w nakładających się chwilach = błąd zapisu.' +
      (approval ? ' Reguła z zatwierdzaniem wchodzi dopiero po „Zatwierdź”; poza datami ważności nie wchodzi.' : '')),
    box,
    h('div', { class: 'bar' }, btn('+ Reguła zdarzeń', () => { box.append(card({ target: {}, events: [] }, cards.length, true)); box.lastElementChild.querySelector('input')?.focus(); })),
    h('div', { class: 'bar end' }, btn('Zapisz zdarzenia wtórne', save, 'acc'))),
  cmdNote('CG Playout GET/PUT /api/live/graphics (secondary)'));
}
