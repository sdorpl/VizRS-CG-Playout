// VizRS Live – formularz z metadanych komend mostu (GET /api/commands).
//
// form(komenda, pola, wartości) buduje formularz: rodzaj kontrolki wynika z typu parametru komendy
// (tekst, liczba, tak/nie, wyliczenie, TimeSpan, data/godzina, lista id), opis pola daje etykietę PL,
// sekcję, źródło listy wyboru i podpowiedź. read() zbiera wartości (rzuca Error z opisem złego pola).

import { h, param, pl, pad2 } from './core.js';

/**
 * Opis pola:
 *   { sec: 'Ogólne' }                                   – nagłówek sekcji
 *   { key, label, kind?, options?, none?, hint?, placeholder?, attrs?, show?(wartości), text? }
 *     hint – tekst albo (wartości) => tekst (przeliczany po każdej zmianie, np. podpowiedź zależna od rodzaju)
 *   { label, group: [pola] }                            – kilka kontrolek w jednym wierszu (np. pola tak/nie)
 *   { key, label, render(wartość, wartości) -> {el, read()} } – kontrolka własna (np. wybór treści)
 * kind: text | number | check | enum | time | datetime | pick | multi (domyślnie z typu parametru komendy)
 * options: [[wartość, etykieta]] dla pick/multi/enum (enum domyślnie z mostu, etykiety z PL);
 *   w multi opcjonalnie trzeci element – miniatura: { src } (obraz; z { badge } – napis, gdy obrazu nie ma) albo { badge }.
 */
export function form(cmdName, fields, values = {}, opts = {}) {
  const prefix = opts.idPrefix ?? 'f-';
  const ctl = new Map();        // key -> {read(), el}
  const rows = [];              // {wrap, show}
  const hints = [];             // {el, fn} – podpowiedzi zależne od wartości
  const current = () => ({ ...values, ...Object.fromEntries([...ctl].map(([k, c]) => [k, c.peek()])) });

  const build = (f) => {
    const p = f.key ? param(cmdName, f.key) : null;
    const id = prefix + f.key;
    const label = f.label + (f.unit ? ` (${f.unit})` : '');
    if (f.render) {
      const c = f.render(values[f.key], values);
      ctl.set(f.key, { read: c.read, peek: c.peek || (() => values[f.key]) });
      return c.el;
    }
    const kind = f.kind || inferKind(p, f);
    const optional = f.optional ?? (p ? p.optional : true);
    const c = control(kind, id, values[f.key], { ...f, optional, type: p?.type || '', values: p?.values, label });
    ctl.set(f.key, c);
    return c.el;
  };

  const el = h('div', { class: 'form' });
  const add = (labelEl, ctrlEl, f) => {
    const hintEl = f.hint ? h('div', { class: 'hint' }, typeof f.hint === 'function' ? '' : f.hint) : null;
    if (typeof f.hint === 'function') hints.push({ el: hintEl, fn: f.hint });
    const wrap = h('div', { class: 'row' }, labelEl, ctrlEl, hintEl);
    if (f.show) rows.push({ wrap, show: f.show });
    el.append(wrap);
  };
  for (const f of fields) {
    if (!f) continue;
    if (f.sec) { el.append(h('div', { class: 'sec' }, f.sec)); continue; }
    if (f.group) { add(h('label', null, f.label), h('div', { class: 'bar' }, f.group.map(build)), f); continue; }
    const c = build(f);
    const kind = f.kind || inferKind(f.key ? param(cmdName, f.key) : null, f);
    add(h('label', kind === 'multi' || f.render ? null : { for: prefix + f.key }, f.label + (f.unit ? ` (${f.unit})` : '')), c, f);
  }
  const refresh = () => {
    const v = current();
    for (const r of rows) r.wrap.hidden = !r.show(v);
    for (const x of hints) { const t = x.fn(v) || ''; x.el.textContent = t; x.el.hidden = !t; }
  };
  el.addEventListener('change', refresh);
  el.addEventListener('input', refresh);
  refresh();

  return {
    el,
    /** Wartości formularza nałożone na wartości wejściowe; ukryte pola zostają jak w wartościach wejściowych. */
    read() {
      const out = { ...values };
      const v = current();
      const hidden = new Set();
      // pole grupy jest ukryte, gdy ukryta jest cała grupa
      for (const f of fields.flatMap((x) => (x?.group ? x.group.map((g) => ({ ...g, show: g.show || x.show })) : [x]))) if (f?.show && !f.show(v)) hidden.add(f.key);
      for (const [k, c] of ctl) if (!hidden.has(k)) out[k] = c.read();
      return out;
    },
    peek: current,
  };
}

/** Rodzaj kontrolki z typu parametru mostu. */
function inferKind(p, f) {
  if (f.options) return /^List</.test(p?.type || '') ? 'multi' : 'pick';
  if (!p) return 'text';
  if (p.values) return 'enum';
  const t = p.type.replace(/\?$/, '');
  if (t === 'Boolean') return 'check';
  if (/^(U?Int(16|32|64)|Double|Single|Decimal|Byte)$/.test(t)) return 'number';
  if (t === 'TimeSpan') return 'time';
  if (t === 'DateTime' || t === 'DateTimeOffset' || t === 'DateOnly') return 'datetime';
  return 'text';
}

class FieldError extends Error {}
const fail = (o, el, msg) => { el.focus(); throw new FieldError(`Pole „${o.label}”: ${msg}`); };

function control(kind, id, value, o) {
  switch (kind) {
    case 'check': {
      const el = h('input', { type: 'checkbox', id, checked: !!value });
      const box = h('label', { class: 'chk' }, el, o.text || '');
      return { el: o.text ? box : el, read: () => el.checked, peek: () => el.checked };
    }
    case 'number': {
      const el = h('input', { id, type: 'number', step: o.step || (/Int/.test(o.type) ? 1 : 'any'), value: value ?? '', placeholder: o.placeholder, ...o.attrs });
      const read = () => {
        if (el.value.trim() === '') return o.optional ? null : fail(o, el, 'podaj liczbę');
        const n = Number(el.value);
        if (!Number.isFinite(n)) return fail(o, el, 'to nie jest liczba');
        if (/Int/.test(o.type) && !Number.isInteger(n)) return fail(o, el, 'podaj liczbę całkowitą');
        return n;
      };
      return { el, read, peek: () => (el.value === '' ? null : Number(el.value)) };
    }
    case 'enum':
    case 'pick': {
      const list = o.options || (o.values || []).map((v) => [v, pl(v)]);
      const opts = o.optional && kind === 'pick' ? [[null, o.none || '—'], ...list] : list;
      const el = h('select', { id, ...o.attrs }, opts.map(([v, t]) => h('option', { value: v == null ? '' : String(v) }, t)));
      el.value = value == null ? '' : String(value);
      if (el.selectedIndex < 0 && value != null) {  // wartość spoza listy (np. usunięty profil) – pokaż, nie gub
        el.append(h('option', { value: String(value) }, `${value} (spoza listy)`));
        el.value = String(value);
      }
      const numeric = kind === 'pick' && list.some(([v]) => typeof v === 'number');
      const conv = (s) => (s === '' ? null : numeric ? Number(s) : s);
      const read = () => {
        const v = conv(el.value);
        if (v == null && !o.optional) return fail(o, el, 'wybierz wartość');
        return v;
      };
      return { el, read, peek: () => conv(el.value) };
    }
    case 'multi': {
      if ((o.options || []).length > MULTI_PLAIN_MAX) return multiSearch(id, value, o);
      const set = new Set(value || []);
      const boxes = (o.options || []).map(([v, t, th]) => {
        const c = h('input', { type: 'checkbox', value: String(v), checked: set.has(v) });
        return [v, c, h('label', { class: 'chk' }, c, miniature(th), t)];
      });
      const el = h('div', { class: 'multi', id, role: 'group', 'aria-label': o.label },
        boxes.length ? boxes.map((b) => b[2]) : h('span', { class: 'dim' }, o.empty || 'brak pozycji do wyboru'));
      const read = () => boxes.filter((b) => b[1].checked).map((b) => b[0]);
      return { el, read, peek: read };
    }
    case 'time': {
      const el = h('input', { id, value: value == null ? '' : String(value), placeholder: o.placeholder || 'gg:mm:ss', inputmode: 'numeric', class: 'num', ...o.attrs });
      const read = () => {
        const s = el.value.trim();
        if (!s) return o.optional ? null : fail(o, el, 'podaj czas gg:mm:ss');
        const t = parseTimeSpan(s);
        return t ?? fail(o, el, 'czas w postaci gg:mm:ss (np. 06:00:00) albo d.gg:mm:ss');
      };
      return { el, read, peek: () => parseTimeSpan(el.value.trim()) };
    }
    case 'datetime': {
      const el = h('input', { id, type: 'datetime-local', step: 1, value: toLocalInput(value), ...o.attrs });
      const read = () => {
        if (!el.value) return o.optional ? null : fail(o, el, 'podaj datę i godzinę');
        return /Offset/.test(o.type) ? new Date(el.value).toISOString() : (el.value.length === 16 ? el.value + ':00' : el.value);
      };
      return { el, read, peek: () => el.value || null };
    }
    default: {
      const el = h('input', { id, value: value ?? '', placeholder: o.placeholder, ...o.attrs });
      const read = () => {
        if (o.required && !el.value.trim()) return fail(o, el, 'nie może być puste');
        return el.value;
      };
      return { el, read, peek: () => el.value };
    }
  }
}

/** Do tylu pozycji wybór wielokrotny to zwykłe pola tak/nie; dłuższa lista – wyszukiwarka z etykietami. */
const MULTI_PLAIN_MAX = 6;
/** Miniatura pozycji wyboru: { src } – obraz (z { badge } jako zapasem), { badge } – napis; null – nic. */
function miniature(th, cls = '') {
  if (th?.src) {
    const img = h('img', { class: 'mini' + cls, src: th.src, alt: '', loading: 'lazy' });
    // brak obrazu (np. CG Playout bez miniatury) – napis zapasowy pozycji
    img.addEventListener('error', () => img.replaceWith(h('span', { class: 'mini badge' + cls }, th.badge || '?')), { once: true });
    return img;
  }
  return th?.badge ? h('span', { class: 'mini badge' + cls }, th.badge) : null;
}

/**
 * Powiększenie miniatury pod kursorem (lista wyboru, etykiety, pola tak/nie): jedno pływające okienko
 * dla całego panelu, bez przechwytywania kliknięć. Tylko urządzenia ze wskaźnikiem (na dotyku nie ma najechania).
 */
let miniPop = null;
function showMiniPop(img) {
  if (!img.complete || !img.naturalWidth) return;  // obraz jeszcze się nie wczytał albo go nie ma
  miniPop ||= document.body.appendChild(h('div', { class: 'mini-pop', 'aria-hidden': 'true' }, h('img', { alt: '' })));
  miniPop.firstChild.src = img.currentSrc || img.src;
  miniPop.hidden = false;
  const r = img.getBoundingClientRect();
  const w = miniPop.offsetWidth, ht = miniPop.offsetHeight, gap = 8;
  const left = Math.min(Math.max(gap, r.left + r.width / 2 - w / 2), innerWidth - w - gap);
  const top = r.top - ht - gap >= gap ? r.top - ht - gap : Math.min(r.bottom + gap, innerHeight - ht - gap);
  miniPop.style.left = left + 'px';
  miniPop.style.top = top + 'px';
}
const hideMiniPop = () => { if (miniPop) miniPop.hidden = true; };
if (typeof document !== 'undefined' && matchMedia?.('(hover: hover)').matches) {
  document.addEventListener('mouseover', (e) => { const t = e.target; if (t instanceof HTMLImageElement && t.classList.contains('mini')) showMiniPop(t); });
  document.addEventListener('mouseout', (e) => { if (e.target instanceof HTMLImageElement && e.target.classList.contains('mini')) hideMiniPop(); });
  addEventListener('scroll', hideMiniPop, true);
  document.addEventListener('mousedown', hideMiniPop, true);
}
const fold = (s) => String(s ?? '').toLocaleLowerCase('pl').normalize('NFD').replace(/[\u0300-\u036f]/g, '').replace(/ł/g, 'l');

/**
 * Wybór wielokrotny z długiej listy: wybrane pozycje jako etykiety z „×” nad polem wyszukiwania,
 * lista podpowiedzi zawężana w trakcie pisania (bez polskich znaków też trafia).
 * Klawiatura: ↑/↓ – podświetlenie, Enter – dodaj/usuń podświetloną, Esc – zamknij, Backspace w pustym polu – usuń ostatnią.
 * Zmiana wyboru wysyła zdarzenie „change” (odświeża pola warunkowe formularza).
 */
function multiSearch(id, value, o) {
  const opts = o.options.map(([v, t, th]) => ({ v, t: String(t ?? v), th, key: fold(t) + ' ' + fold(v) + ' ' + fold(th?.badge) }));
  const known = new Set(opts.map((x) => x.v));
  const extra = (value || []).filter((v) => !known.has(v)).map((v) => ({ v, t: `${v} (spoza listy)`, key: fold(v) }));
  const all = [...opts, ...extra];
  const sel = new Set(value || []);
  let shown = [];
  let hi = -1;

  const chips = h('div', { class: 'msel-chips' });
  const input = h('input', { type: 'search', class: 'msel-q', id, autocomplete: 'off', spellcheck: 'false', 'aria-label': `${o.label} – szukaj`,
    'aria-expanded': 'false', 'aria-controls': id + '-list', role: 'combobox', placeholder: `Szukaj i dodaj… (${opts.length} do wyboru)` });
  const list = h('div', { class: 'msel-list', id: id + '-list', role: 'listbox', 'aria-multiselectable': 'true', hidden: true });
  const el = h('div', { class: 'msel', role: 'group', 'aria-label': o.label }, chips, input, list);

  const changed = () => { drawChips(); drawList(); el.dispatchEvent(new Event('change', { bubbles: true })); };
  const toggle = (x) => { if (sel.has(x.v)) sel.delete(x.v); else sel.add(x.v); changed(); };

  function drawChips() {
    const picked = all.filter((x) => sel.has(x.v));
    chips.replaceChildren(...(picked.length
      ? picked.map((x) => h('span', { class: 'pill msel-chip', title: x.t }, miniature(x.th, ' sm'), h('span', null, x.t),
        h('button', { type: 'button', class: 'msel-x', 'aria-label': `Usuń: ${x.t}`, title: 'Usuń', onclick: () => { toggle(x); input.focus(); } }, '×')))
      : [h('span', { class: 'dim' }, 'nic nie wybrano')]),
      ...(picked.length > 1 ? [h('button', { type: 'button', class: 'msel-clear', onclick: () => { sel.clear(); changed(); input.focus(); } }, 'wyczyść')] : []));
  }

  function drawList() {
    if (list.hidden) return;
    const q = fold(input.value.trim());
    const words = q ? q.split(/\s+/) : [];
    shown = all.filter((x) => words.every((w) => x.key.includes(w)));
    hi = Math.min(Math.max(hi, words.length && shown.length ? 0 : -1), shown.length - 1);
    list.replaceChildren(...(shown.length
      ? shown.map((x, i) => h('div', { class: 'msel-opt' + (sel.has(x.v) ? ' on' : '') + (i === hi ? ' hi' : ''), id: `${id}-o${i}`, role: 'option',
        'aria-selected': sel.has(x.v) ? 'true' : 'false', onmousedown: (e) => { e.preventDefault(); hi = i; toggle(x); } },
        h('span', { class: 'msel-mark' }, sel.has(x.v) ? '✓' : ''), miniature(x.th) || h('span', { class: 'mini' }), h('span', { class: 'msel-t' }, x.t)))
      : [h('div', { class: 'dim msel-none' }, 'nic nie pasuje')]));
    if (hi >= 0) { input.setAttribute('aria-activedescendant', `${id}-o${hi}`); list.children[hi]?.scrollIntoView({ block: 'nearest' }); }
    else input.removeAttribute('aria-activedescendant');
  }

  const open = (yes) => { list.hidden = !yes; input.setAttribute('aria-expanded', String(yes)); if (yes) drawList(); else hi = -1; };
  input.addEventListener('focus', () => open(true));
  input.addEventListener('click', () => open(true));
  input.addEventListener('blur', () => open(false));
  input.addEventListener('input', () => { hi = -1; open(true); });
  input.addEventListener('keydown', (e) => {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      if (list.hidden) open(true);
      if (shown.length) hi = (hi + (e.key === 'ArrowDown' ? 1 : shown.length - 1) + (hi < 0 && e.key === 'ArrowUp' ? 1 : 0)) % shown.length;
      drawList();
    } else if (e.key === 'Enter') {
      e.preventDefault();  // nie zatwierdzaj okna/formularza
      const x = shown[hi >= 0 ? hi : 0];
      if (x && !list.hidden) toggle(x);
    } else if (e.key === 'Escape' && !list.hidden) {
      e.preventDefault(); e.stopPropagation();  // pierwsze Esc zamyka listę, nie okno
      if (input.value) { input.value = ''; hi = -1; drawList(); } else open(false);
    } else if (e.key === 'Backspace' && !input.value) {
      const last = all.filter((x) => sel.has(x.v)).pop();
      if (last) toggle(last);
    }
  });
  // „change” samego pola wyszukiwania nie jest zmianą wartości
  input.addEventListener('change', (e) => e.stopPropagation());

  drawChips();
  const read = () => all.filter((x) => sel.has(x.v)).map((x) => x.v);
  return { el, read, peek: read };
}

/** „6:00” → „06:00:00”, „1.02:00:00” zostaje; null, gdy to nie jest czas. */
export function parseTimeSpan(s) {
  const m = /^(?:(\d+)\.)?(\d{1,2}):(\d{2})(?::(\d{2}))?$/.exec(s || '');
  if (!m || +m[3] > 59 || (m[4] && +m[4] > 59) || (!m[1] && +m[2] > 23)) return null;
  return (m[1] ? m[1] + '.' : '') + pad2(m[2]) + ':' + m[3] + ':' + (m[4] || '00');
}

function toLocalInput(v) {
  if (!v) return '';
  const d = new Date(v);
  if (isNaN(d)) return '';
  return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}T${pad2(d.getHours())}:${pad2(d.getMinutes())}:${pad2(d.getSeconds())}`;
}
