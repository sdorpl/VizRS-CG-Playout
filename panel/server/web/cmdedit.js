// Edytor komend VizRS dla reguł grafiki (Grafika kanału): zamiast tekstu „TAKE 4001 headline={title}”
// lista kroków – akcja, strony z show VizRS (lista z CG Playout /etv/pages), pola strony z symbolami ramówki.
// Zapis i odczyt w tej samej składni tekstowej co config.json CG Playout (DOKUMENTACJA CG §3); linia, której
// edytor nie rozumie (komentarz, @N, inna komenda), zostaje krokiem „komenda tekstowa” bez zmian.

import { h } from './core.js';

/** Symbole ramówki wstawiane w wartości pól (CG Playout, DOKUMENTACJA §3). */
export const SYMBOLS = [
  ['{title}', 'tytuł pozycji'], ['{show_title}', 'tytuł serialu'], ['{duration}', 'długość pozycji'], ['{source}', 'ścieżka pliku'],
  ['{next_title}', 'następny – tytuł'], ['{next_start}', 'następny – start gg:mm'], ['{next_start_full}', 'następny – start gg:mm:ss'], ['{next_duration}', 'następny – długość'],
  ['{next2_title}', 'po nim – tytuł'], ['{next2_start}', 'po nim – start gg:mm'], ['{next2_start_full}', 'po nim – start gg:mm:ss'], ['{next2_duration}', 'po nim – długość'],
];

export const ACTS = {
  TAKE: { label: 'Wprowadź', fields: true, icon: '▶' },
  CONTINUE: { label: 'Dalej', icon: '⏭' },
  OUT: { label: 'Zdejmij', icon: '■' },
  UPDATE: { label: 'Podmień dane', fields: true, single: true, icon: '✎' },
  raw: { label: 'Komenda tekstowa', icon: '›' },
};

const PAGES_RE = /^\d+(,\d+)*$/;
const KEY_RE = /^[\w.-]+$/;

/** Linia komendy → krok edytora: {act, pages, fields: [[k, v]], json} albo {act: 'raw', text}. */
export function parseLine(line) {
  const text = line.trim();
  const raw = { act: 'raw', text };
  const m = /^(TAKE|OUT|CONTINUE|UPDATE)\s+(\S+)(?:\s+([\s\S]*))?$/i.exec(text);
  if (!m || !PAGES_RE.test(m[2])) return raw;
  const act = m[1].toUpperCase();
  const pages = m[2].split(',');
  const rest = (m[3] || '').trim();
  if (!ACTS[act].fields) return rest ? raw : { act, pages, fields: [] };
  if (ACTS[act].single && pages.length > 1) return raw;
  if (!rest) return { act, pages, fields: [] };
  if (rest.startsWith('{')) {
    let obj;
    try { obj = JSON.parse(rest); } catch { return raw; }
    if (!obj || typeof obj !== 'object' || Array.isArray(obj) || Object.values(obj).some((v) => typeof v !== 'string')) return raw;
    return { act, pages, fields: Object.entries(obj), json: true };
  }
  const fields = [];
  for (const part of rest.split('|')) {
    const i = part.indexOf('=');
    const k = i < 0 ? '' : part.slice(0, i).trim();
    if (!KEY_RE.test(k)) return raw;
    fields.push([k, part.slice(i + 1)]);
  }
  return { act, pages, fields };
}

/** Tekst komend (linie) → kroki; puste linie pomijane. */
export const parseCmds = (text) => String(text || '').split('\n').filter((l) => l.trim()).map(parseLine);

/** Krok → linia komendy; JSON pól, gdy wartość ma „|” albo koniec linii (składnia VizRS) albo tak było w źródle. */
export function lineOf(s) {
  if (s.act === 'raw') return s.text.trim();
  const head = `${s.act} ${s.pages.join(',')}`;
  const fields = (s.fields || []).filter(([k]) => k.trim());
  if (!ACTS[s.act].fields || !fields.length) return head;
  if (s.json || fields.some(([, v]) => /[|\n]/.test(v))) return `${head} ${JSON.stringify(Object.fromEntries(fields.map(([k, v]) => [k.trim(), v])))}`;
  return `${head} ${fields.map(([k, v]) => `${k.trim()}=${v}`).join('|')}`;
}

/** Nazwa strony z listy CG Playout. */
export const pageName = (pages, n) => pages.get(String(n))?.name;

/** Krótki opis komend po polsku do streszczenia reguł: [{icon, text, detail, raw}]. */
export function describe(text, pages) {
  return parseCmds(text).map((s) => {
    if (s.act === 'raw') return { icon: ACTS.raw.icon, text: s.text, raw: true };
    const names = s.pages.map((n) => `${n}${pageName(pages, n) ? ' · ' + pageName(pages, n) : ''}`).join(', ');
    const detail = (s.fields || []).map(([k, v]) => `${k} = ${v}`).join(' · ');
    return { icon: ACTS[s.act].icon, text: `${ACTS[s.act].label} ${names}`, detail };
  });
}

/** Nazwy pól używane w komendach stron (z bieżących reguł) – podpowiedzi, gdy CG Playout nie podaje pól strony. */
export function fieldsUsed(texts) {
  const out = new Map();
  for (const t of texts) {
    for (const s of parseCmds(t)) {
      if (!s.fields) continue;
      for (const n of s.pages) for (const [k] of s.fields) { if (!out.has(n)) out.set(n, new Set()); out.get(n).add(k); }
    }
  }
  return out;
}

let seq = 0;

/**
 * Edytor jednego pola reguły (on_clip / on_end / on_ad).
 * opts: id (id wyboru trybu – cel „kliknij wiersz”), value (null = jak globalnie, '' = wyłączone, tekst komend),
 * scoped (reguła audycji / kanału: jest „jak globalnie”), ad (przerwa reklamowa: „wyłączone” = grafika schodzi),
 * pages (Map nr → {name, fields?}), used (Map nr → Set nazw pól z reguł), label (do aria).
 * Zwraca { el, read, focus }; read() → null | '' | tekst, rzuca Error przy kroku bez strony.
 */
export function cmdEditor(opts) {
  const { pages, used = new Map(), label } = opts;
  const uid = `ce${++seq}`;
  const offText = opts.ad ? 'tylko zdejmij grafikę' : opts.scoped ? 'wyłączone – nic nie wchodzi' : 'nic nie wchodzi';
  const mode = h('select', { id: opts.id, class: 'ce-mode', 'aria-label': `${label}: tryb` },
    opts.scoped ? h('option', { value: 'inherit' }, 'jak globalnie') : null,
    h('option', { value: 'off' }, offText),
    h('option', { value: 'cmd' }, 'grafiki VizRS'));
  const v = opts.value;
  mode.value = v == null ? (opts.scoped ? 'inherit' : 'off') : v === '' ? 'off' : 'cmd';
  let steps = parseCmds(v);
  let textMode = false;

  const list = h('div', { class: 'ce-steps' });
  const area = h('textarea', { class: 'mono', rows: 3, 'aria-label': `${label}: komendy tekstowe`, hidden: true, spellcheck: 'false' });
  const prev = h('code', { class: 'ce-prev', title: 'Komenda wysyłana do VizRS' });
  const toggle = h('button', { type: 'button', class: 'ce-text', title: 'Edytuj te same komendy jako tekst (składnia CG Playout)' }, 'Tekst');
  const add = h('button', { type: 'button', class: 'ce-add' }, '+ Grafika');
  const body = h('div', { class: 'ce-body' }, list, area, h('div', { class: 'bar ce-foot' }, add, h('span', { class: 'grow' }), prev, toggle));
  const el = h('div', { class: 'ce', 'data-cmd-editor': opts.id || '' }, h('div', { class: 'ce-top' }, mode), body);

  const text = () => steps.map(lineOf).filter(Boolean).join('\n');
  const update = () => {
    body.hidden = mode.value !== 'cmd';
    if (!textMode) prev.textContent = text() || '—';
    prev.hidden = textMode;
  };

  const pageOpts = (taken) => [h('option', { value: '' }, taken.length ? '+ strona…' : 'wybierz stronę…'),
    ...[...pages].filter(([n]) => !taken.includes(n)).map(([n, p]) => h('option', { value: n }, `${n} · ${p.name}`))];
  const knownFields = (s) => [...new Set(s.pages.flatMap((n) => [...(pages.get(n)?.fields || []), ...(used.get(n) || [])]))];

  function stepEl(s, i) {
    const act = h('select', { class: 'ce-act', 'aria-label': `${label}: akcja ${i + 1}` },
      Object.entries(ACTS).map(([k, a]) => h('option', { value: k }, a.label)));
    act.value = s.act;
    act.addEventListener('change', () => {
      const to = act.value;
      if (to === 'raw') steps[i] = { act: 'raw', text: lineOf(s) };
      else if (s.act === 'raw') { const p = parseLine(s.text); steps[i] = { act: to, pages: p.pages || [], fields: ACTS[to].fields ? p.fields || [] : [], json: p.json }; }
      else steps[i] = { ...s, act: to, pages: ACTS[to].single ? s.pages.slice(0, 1) : s.pages, fields: ACTS[to].fields ? s.fields || [] : [] };
      render();
    });
    const tools = h('span', { class: 'ce-tools' },
      h('button', { type: 'button', title: 'W górę', 'aria-label': `W górę ${i + 1}`, disabled: i === 0, onclick: () => { [steps[i - 1], steps[i]] = [steps[i], steps[i - 1]]; render(); } }, '↑'),
      h('button', { type: 'button', title: 'W dół', 'aria-label': `W dół ${i + 1}`, disabled: i === steps.length - 1, onclick: () => { [steps[i + 1], steps[i]] = [steps[i], steps[i + 1]]; render(); } }, '↓'),
      h('button', { type: 'button', class: 'danger', title: 'Usuń krok', 'aria-label': `${label}: usuń krok ${i + 1}`, onclick: () => { steps.splice(i, 1); render(); } }, '✕'));

    if (s.act === 'raw') {
      const t = h('input', { class: 'mono', value: s.text, 'aria-label': `${label}: komenda ${i + 1}`, placeholder: 'np. UPDATE 1405 next=Teraz' });
      t.addEventListener('input', () => { s.text = t.value; update(); });
      return h('div', { class: 'ce-step raw' }, h('div', { class: 'ce-row' }, h('span', { class: 'ce-ic' }, ACTS.raw.icon), act, t, tools));
    }

    const chips = s.pages.map((n, j) => h('span', { class: 'ce-page' + (pages.has(n) ? '' : ' unknown'), title: pages.has(n) ? '' : 'Strony nie ma w show VizRS (albo CG Playout jej nie widzi)' },
      h('b', null, n), ' ', pageName(pages, n) || '(brak w show)',
      h('button', { type: 'button', 'aria-label': `Usuń stronę ${n}`, onclick: () => { s.pages.splice(j, 1); render(); } }, '×')));
    const pick = h('select', { class: 'ce-pick', 'aria-label': `${label}: strona ${i + 1}` }, pageOpts(s.pages));
    pick.addEventListener('change', () => {
      if (!pick.value) return;
      if (ACTS[s.act].single) s.pages = [pick.value]; else s.pages.push(pick.value);
      render();
    });
    const canPick = !(ACTS[s.act].single && s.pages.length) && pick.options.length > 1;
    const row = h('div', { class: 'ce-row' }, h('span', { class: 'ce-ic' }, ACTS[s.act].icon), act,
      h('span', { class: 'ce-pages' }, chips, canPick ? pick : null, !pages.size && !s.pages.length ? h('span', { class: 'dim' }, 'CG Playout nie podał stron show') : null), tools);
    if (!ACTS[s.act].fields) return h('div', { class: 'ce-step' }, row);

    const dl = `${uid}-f${i}`;
    const fieldRow = (f, j) => {
      const k = h('input', { class: 'mono ce-key', value: f[0], placeholder: 'pole', list: dl, 'aria-label': `${label}: pole ${i + 1}.${j + 1}` });
      const val = h('input', { class: 'ce-val', value: f[1], placeholder: 'wartość albo symbol {…}', 'aria-label': `${label}: wartość ${i + 1}.${j + 1}` });
      const sym = h('select', { class: 'ce-sym', title: 'Wstaw symbol ramówki', 'aria-label': `${label}: symbol ${i + 1}.${j + 1}` },
        h('option', { value: '' }, '{…}'), SYMBOLS.map(([sy, d]) => h('option', { value: sy }, `${d}  ${sy}`)));
      k.addEventListener('input', () => { f[0] = k.value; update(); });
      val.addEventListener('input', () => { f[1] = val.value; update(); });
      sym.addEventListener('change', () => {
        if (!sym.value) return;
        const a = val.selectionStart ?? val.value.length, b = val.selectionEnd ?? a, ins = sym.value;
        val.value = val.value.slice(0, a) + ins + val.value.slice(b);
        f[1] = val.value; sym.value = ''; update();
        val.focus(); val.setSelectionRange(a + ins.length, a + ins.length);
      });
      return h('div', { class: 'ce-field' }, k, h('span', { class: 'dim' }, '='), val, sym,
        h('button', { type: 'button', class: 'danger', title: 'Usuń pole', 'aria-label': `${label}: usuń pole ${i + 1}.${j + 1}`, onclick: () => { s.fields.splice(j, 1); render(); } }, '✕'));
    };
    const known = knownFields(s);
    const addField = h('button', { type: 'button', class: 'ce-addf' }, '+ pole');
    addField.addEventListener('click', () => { s.fields.push([known.find((x) => !s.fields.some(([k]) => k === x)) || '', '']); render(); });
    return h('div', { class: 'ce-step' }, row,
      h('div', { class: 'ce-fields' }, s.fields.map(fieldRow), h('datalist', { id: dl }, known.map((x) => h('option', { value: x }))),
        h('div', { class: 'bar' }, addField, s.fields.length ? null : h('span', { class: 'dim' }, 'strona wejdzie z wartościami zapisanymi w Trio'))));
  }

  function render() {
    list.replaceChildren(...(steps.length ? steps.map(stepEl) : [h('div', { class: 'ce-empty dim' }, 'Brak grafik – dodaj stronę VizRS.')]));
    update();
  }

  add.addEventListener('click', () => { steps.push({ act: 'TAKE', pages: [], fields: [] }); render(); list.lastElementChild?.querySelector('.ce-pick')?.focus(); });
  toggle.addEventListener('click', () => {
    if (!textMode) { area.value = text(); } else { steps = parseCmds(area.value); }
    textMode = !textMode;
    toggle.textContent = textMode ? 'Lista' : 'Tekst';
    toggle.classList.toggle('on', textMode);
    area.hidden = !textMode; list.hidden = textMode; add.hidden = textMode;
    if (textMode) area.focus();
    render();
  });
  mode.addEventListener('change', () => {
    if (mode.value === 'cmd' && !steps.length && !textMode) steps.push({ act: 'TAKE', pages: [], fields: [] });
    render();
  });
  render();

  return {
    el,
    focus: () => mode.focus(),
    read() {
      if (mode.value === 'inherit') return null;
      if (mode.value === 'off') return '';
      if (textMode) steps = parseCmds(area.value);
      const bad = steps.findIndex((s) => s.act !== 'raw' && !s.pages.length);
      if (bad >= 0) throw new Error(`${label}: krok ${bad + 1} („${ACTS[steps[bad].act].label}”) nie ma wybranej strony`);
      const t = text();
      return t || '';
    },
  };
}
