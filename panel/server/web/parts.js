// VizRS Live – wspólne części ekranów: „lista + edytor”, edytor listy pozycji, wybór treści.

import { h, btn, head, panel, cmdNote, table, toast, dialog, confirmDlg, cmd, argsFor, missing, missingNote, route, pl } from './core.js';

// ---------- wzorzec ekranu „lista + edytor” ----------
/**
 * Ekran: lista z zaznaczeniem po lewej, edytor zaznaczonej pozycji po prawej, „+ Nowy”, „Zapisz”,
 * „Usuń” z potwierdzeniem, „Kopiuj” (gdy podano komendę kopiowania).
 *
 * cfg = {
 *   title, desc, listCmd,                            // listCmd – zapytanie listy (do kontroli białej listy)
 *   noun: { list, new, add, create, one(dane), title(row) },  // napisy: panel listy, nowy, „+ Nowy”, „Załóż”
 *   commands: { create, update, delete, copy? },     // nazwy komend legacy
 *   createCmd: (data, ctx) => nazwa,                 // komenda zakładania zależna od danych (np. rodzaj playoutu); domyślnie commands.create
 *   needs: [nazwy],                                  // dodatkowe komendy wymagane przez ekran
 *   load: async () => ({ rows, ...ctx }),            // dane listy i list wyboru
 *   id: (row) => id, cols: [...kolumny tabeli] albo (ctx) => [...],
 *   editor: (row|null, ctx) => { el, read() },       // formularz (form.js)
 *   create: (data, ctx) => args, update: (data, row, ctx) => args, del: (row) => args,
 *   createdId: (wynik) => id, confirmDelete: (row, ctx) => tekst,
 *   copy: (row, nazwa) => args, copiedId: (wynik) => id,
 *   headExtra: (ctx) => [przyciski/odnośniki], footer: (row, ctx) => węzeł,
 *   loadOne: async (row, ctx) => ({...}),            // dane zaznaczonej pozycji (np. jej pozycje), dołożone do ctx
 *   below: (row, ctx) => węzeł | [węzły],            // pod listą i edytorem, na całą szerokość (np. edytor pozycji)
 *   beforeSave: (ctx) => void,                       // sprawdzenie przed zapisem; Error = komunikat, nic nie zapisuje
 *   afterSave: async (id, ctx) => void,              // po zapisie (np. zapis pozycji); Error = komunikat
 *   afterDelete: async (row, ctx) => void,           // po usunięciu (np. sprzątanie ustawień poza legacy); Error = komunikat
 *   locked: (row, ctx) => węzeł | null,              // pozycja zarządzana gdzie indziej: uwaga zamiast „Zapisz”, „Usuń”, „Kopiuj”
 * }
 */
export function crudView(cfg) {
  let sel = null; // id zaznaczonej pozycji albo 'new'
  const c = cfg.commands;
  const required = [cfg.listCmd, c.create, c.update, c.delete, c.copy, ...(cfg.needs || [])].filter(Boolean);
  const view = async () => {
    const miss = missing(required);
    if (miss.length) return [head(cfg.title, cfg.desc), missingNote(miss)];
    const ctx = await cfg.load();
    const rows = ctx.rows || [];
    const current = sel === 'new' ? null : rows.find((r) => cfg.id(r) === sel) || rows[0] || null;
    if (sel !== 'new') sel = current ? cfg.id(current) : 'new';
    if (cfg.loadOne) Object.assign(ctx, await cfg.loadOne(current, ctx));
    const isNew = !current;
    const noun = cfg.noun;
    const ed = cfg.editor(current, ctx);

    const save = async () => {
      let data;
      try { data = ed.read(); if (cfg.beforeSave) cfg.beforeSave(ctx, data); } catch (e) { return toast(e.message, 'bad'); }
      let id;
      try {
        if (isNew) {
          const name = cfg.createCmd ? cfg.createCmd(data, ctx) : c.create;
          const r = await cmd(name, argsFor(name, cfg.create ? cfg.create(data, ctx) : data));
          id = cfg.createdId ? cfg.createdId(r) ?? null : null;
          sel = id;
        } else {
          await cmd(c.update, argsFor(c.update, cfg.update(data, current, ctx)));
          id = cfg.id(current);
        }
      } catch (e) { return toast('Legacy odrzucił zapis: ' + e.message, 'bad'); }
      try {
        if (cfg.afterSave && id != null) await cfg.afterSave(id, ctx, data);
        toast(`${noun.one(data)}: ${isNew ? 'zapisano w legacy' : 'zapisano'}.`, 'ok');
      } catch (e) { toast(e.message, 'bad'); }
      route(true);
    };
    const remove = async () => {
      const text = cfg.confirmDelete ? cfg.confirmDelete(current, ctx) : `Usunąć „${noun.one(current)}”? Tego nie da się cofnąć.`;
      if (!(await confirmDlg('Usuń', text, 'Usuń'))) return;
      try { await cmd(c.delete, cfg.del(current)); sel = null; toast(`${noun.one(current)}: usunięto.`, 'ok'); }
      catch (e) { return toast('Legacy odrzucił usunięcie: ' + e.message, 'bad'); }
      try { if (cfg.afterDelete) await cfg.afterDelete(current, ctx); } catch (e) { toast(e.message, 'bad'); }
      route(true);
    };
    const copy = async () => {
      const input = h('input', { id: 'copy-name', value: noun.one(current) + ' (kopia)' });
      const ok = await dialog('Kopiuj', h('div', { class: 'form' }, h('label', { for: 'copy-name' }, 'Nazwa kopii'), input), [['Anuluj', false], ['Kopiuj', true, 'acc']]);
      if (!ok || !input.value.trim()) return;
      try {
        const r = await cmd(c.copy, cfg.copy(current, input.value.trim()));
        sel = cfg.copiedId ? cfg.copiedId(r) ?? sel : sel;
        toast('Skopiowano.', 'ok');
        route(true);
      } catch (e) { toast('Legacy odrzucił kopiowanie: ' + e.message, 'bad'); }
    };

    const list = panel(noun.list, h('span', { class: 'dim' }, String(rows.length)),
      table(rows, typeof cfg.cols === 'function' ? cfg.cols(ctx) : cfg.cols, { onRow: (r) => { sel = cfg.id(r); route(true); }, selected: (r) => cfg.id(r) === sel, empty: cfg.empty }));
    const lock = !isNew && cfg.locked ? cfg.locked(current, ctx) : null;
    const editor = panel(isNew ? noun.new : noun.title(current),
      lock ? [] : [!isNew && c.copy ? btn('Kopiuj', copy) : null, !isNew ? btn('Usuń', remove, 'danger') : null, btn(isNew ? noun.create : 'Zapisz', save, 'acc')],
      lock, ed.el, cfg.footer ? cfg.footer(current, ctx) : null, cmdNote(...[isNew ? c.create : c.update, c.copy, c.delete].filter(Boolean)));
    return [
      head(cfg.title, cfg.desc, btn(noun.add, () => { sel = 'new'; route(true); }, 'acc'), cfg.headExtra ? cfg.headExtra(ctx) : null),
      h('div', { class: 'split' }, list, editor),
      ...[cfg.below ? cfg.below(current, ctx) : null].flat(),
    ];
  };
  /** Zaznaczenie z zewnątrz (np. przegląd wszystkich rodzajów): id pozycji albo 'new'; działa przy następnym przerysowaniu. */
  view.select = (id) => { sel = id; };
  view.cfg = cfg;
  return view;
}

// ---------- widok tylko do odczytu (lista z zakładkami) ----------
export const READONLY = 'Ten ekran na razie tylko czyta dane z legacy. Edycja dojdzie w kolejnych punktach planu (docs/plany/PLAN-rozbudowy.md).';

/** sources = [{ title, cmd, args?, pick?(wynik), cols, empty? }] – po zakładce na źródło. */
export function listView(title, desc, sources, footer) {
  return async () => {
    const miss = missing(sources.map((s) => s.cmd));
    if (miss.length) return [head(title, desc), missingNote(miss)];
    const results = await Promise.all(sources.map((s) => cmd(s.cmd, s.args || {}).then((r) => (s.pick ? s.pick(r) : r))));
    let tab = 0;
    const box = h('div');
    const render = () => {
      const s = sources[tab];
      box.replaceChildren(
        sources.length > 1 ? h('div', { class: 'tabs' }, sources.map((x, i) => h('button', { type: 'button', class: i === tab ? 'on' : null, onclick: () => { tab = i; render(); } }, `${x.title} (${results[i]?.length ?? 0})`))) : null,
        table(results[tab], s.cols, { empty: s.empty }));
    };
    render();
    return [
      head(title, desc),
      panel(sources.length > 1 ? 'Pozycje' : sources[0].title, null, box, cmdNote(...sources.map((s) => s.cmd))),
      footer ? h('div', { class: 'note' }, footer) : null,
    ];
  };
}

// ---------- edytor listy pozycji ----------
/**
 * Lista pozycji (np. pozycje harmonogramu, bloku, playlisty): wiersze w górę/w dół, dodaj, usuń,
 * edycja wiersza w panelu bocznym. Zwraca { el, value() } – value() to bieżąca tablica pozycji.
 *
 * cfg = { items, cols: [[funkcja(pozycja, nr), nagłówek, {class}]], newItem: () => pozycja,
 *         edit: (pozycja, nr) => { el, read() }, title?, addText?, empty?, onChange?(pozycje), copy?: true }
 * commit() zatwierdza otwarty panel boczny (przed zapisem całej listy); rzuca Error, gdy pole jest złe.
 */
export function itemsEditor(cfg) {
  let items = (cfg.items || []).map((x) => ({ ...x }));
  let open = -1; // edytowany wiersz
  let editor = null;
  const el = h('div', { class: 'items' });
  const changed = () => { if (cfg.onChange) cfg.onChange(items); render(); };

  const move = (i, d) => {
    const j = i + d;
    if (j < 0 || j >= items.length) return;
    [items[i], items[j]] = [items[j], items[i]];
    if (open === i) open = j; else if (open === j) open = i;
    changed();
  };
  const del = (i) => {
    items.splice(i, 1);
    if (open === i) open = -1; else if (open > i) open--;
    changed();
  };
  const add = () => { items.push(cfg.newItem()); open = items.length - 1; changed(); };
  const dup = (i) => {
    items.splice(i + 1, 0, structuredClone(items[i]));
    if (open > i) open++;
    changed();
  };
  const apply = () => {
    let v;
    try { v = editor.read(); } catch (e) { return toast(e.message, 'bad'); }
    items[open] = v;
    open = -1;
    changed();
  };

  function render() {
    const rows = items.map((it, i) => {
      const tr = h('tr', { class: 'click' + (i === open ? ' sel' : '') },
        h('td', { class: 'num dim' }, String(i + 1)),
        cfg.cols.map(([f, , o]) => { const v = f(it, i); return h('td', { class: o && o.class }, v instanceof Node ? v : v == null || v === '' ? '—' : String(v)); }),
        h('td', { class: 'act' }, h('span', { class: 'bar' },
          cfg.copy ? h('button', { type: 'button', title: 'Kopiuj pozycję', 'aria-label': `Kopiuj pozycję ${i + 1}`, onclick: () => dup(i) }, '⧉') : null,
          h('button', { type: 'button', title: 'W górę', 'aria-label': `Pozycja ${i + 1} w górę`, disabled: i === 0, onclick: () => move(i, -1) }, '↑'),
          h('button', { type: 'button', title: 'W dół', 'aria-label': `Pozycja ${i + 1} w dół`, disabled: i === items.length - 1, onclick: () => move(i, 1) }, '↓'),
          h('button', { type: 'button', class: 'danger', title: 'Usuń pozycję', 'aria-label': `Usuń pozycję ${i + 1}`, onclick: () => del(i) }, '✕'))));
      tr.addEventListener('click', (e) => { if (!e.target.closest('button')) { open = open === i ? -1 : i; render(); } });
      return tr;
    });
    const list = items.length
      ? h('div', { class: 'tw' }, h('table', null,
        h('thead', null, h('tr', null, h('th', null, '#'), cfg.cols.map(([, t]) => h('th', null, t)), h('th', null, ''))),
        h('tbody', null, rows)))
      : h('div', { class: 'empty' }, cfg.empty || 'Brak pozycji. Dodaj pierwszą.');
    editor = open >= 0 ? cfg.edit(items[open], open) : null;
    el.replaceChildren(
      h('div', { class: 'items-list' }, list, h('div', { class: 'bar pad' }, btn(cfg.addText || '+ Dodaj pozycję', add))),
      editor ? h('div', { class: 'items-side' },
        h('header', null, h('h3', null, `Pozycja ${open + 1}`), h('span', { class: 'grow' }), btn('Anuluj', () => { open = -1; render(); }), btn('Zastosuj', apply, 'acc')),
        editor.el) : '');
    el.classList.toggle('open', !!editor);
  }
  render();
  const commit = () => {
    if (!editor) return;
    items[open] = editor.read();
    open = -1;
    changed();
  };
  return { el, value: () => items.map((x) => ({ ...x })), commit };
}

// ---------- wybór treści ----------
const ID_FIELDS = ['CollectionId', 'MultiCollectionId', 'SmartCollectionId', 'RerunCollectionId', 'MediaItemId', 'PlaylistId'];

/** Rodzaje treści (CollectionType legacy): zapytanie wyszukiwania, pole id i etykieta wyniku. */
export const CONTENT_KINDS = {
  Collection: { search: 'SearchCollections', field: 'CollectionId', row: (r) => [r.Id, r.Name] },
  SmartCollection: { search: 'SearchSmartCollections', field: 'SmartCollectionId', row: (r) => [r.Id, r.Name] },
  MultiCollection: { search: 'SearchMultiCollections', field: 'MultiCollectionId', row: (r) => [r.Id, r.Name] },
  Playlist: { needs: ['GetAllPlaylistGroups', 'GetPlaylistsByPlaylistGroupId'], field: 'PlaylistId', list: playlists },
  TelevisionShow: { search: 'SearchTelevisionShows', field: 'MediaItemId', row: (r) => [r.MediaItemId, r.Name] },
  TelevisionSeason: { search: 'SearchTelevisionSeasons', field: 'MediaItemId', row: (r) => [r.MediaItemId, r.Name] },
  Artist: { search: 'SearchArtists', field: 'MediaItemId', row: (r) => [r.MediaItemId, r.Name] },
  RerunFirstRun: { search: 'SearchRerunCollections', field: 'RerunCollectionId', row: (r) => [r.Id, r.Name] },
  RerunRerun: { search: 'SearchRerunCollections', field: 'RerunCollectionId', row: (r) => [r.Id, r.Name] },
  SearchQuery: { query: true },
};

let playlistCache = null;
async function playlists() {
  if (!playlistCache) {
    playlistCache = (async () => {
      const groups = await cmd('GetAllPlaylistGroups');
      const lists = await Promise.all(groups.map((g) => cmd('GetPlaylistsByPlaylistGroupId', { PlaylistGroupId: g.Id })
        .then((ps) => ps.map((p) => [p.Id, `${g.Name} / ${p.Name}`]))));
      return lists.flat();
    })();
    setTimeout(() => (playlistCache = null), 30000);
  }
  return playlistCache;
}

/** Pusta treść w kształcie pól pozycji legacy (ReplaceProgramScheduleItem, pozycje bloku, playlisty…). */
export const emptyContent = (kind = 'Collection') => ({
  CollectionType: kind, CollectionId: null, MultiCollectionId: null, SmartCollectionId: null, RerunCollectionId: null,
  MediaItemId: null, PlaylistId: null, SearchTitle: '', SearchQuery: '',
});

/**
 * Wybór treści: rodzaj (kolekcja / inteligentna / wielokolekcja / playlista / serial / sezon / artysta /
 * powtórki / wyszukiwanie) + wyszukiwarka. value – pola treści pozycji legacy; opts = { kinds, label, idPrefix, optional }.
 * Zwraca { el, read(), peek(), label() }; read() daje pola treści (CollectionType + jedno id albo SearchTitle/SearchQuery).
 */
export function contentPicker(value, opts = {}) {
  const prefix = opts.idPrefix ?? 'cp-';
  const kinds = opts.kinds || Object.keys(CONTENT_KINDS);
  const v = { ...emptyContent(kinds[0]), ...(value || {}) };
  let kind = kinds.includes(v.CollectionType) ? v.CollectionType : kinds[0];
  let chosen = null; // [id, nazwa]
  const def = CONTENT_KINDS[kind];
  if (def?.field && v[def.field] != null) chosen = [v[def.field], opts.label || `id ${v[def.field]}`];

  const kindSel = h('select', { id: prefix + 'kind', 'aria-label': 'Rodzaj treści' }, kinds.map((k) => h('option', { value: k }, pl(k))));
  kindSel.value = kind;
  const q = h('input', { id: prefix + 'q', type: 'search', placeholder: 'szukaj po nazwie…', 'aria-label': 'Szukaj treści', autocomplete: 'off' });
  const title = h('input', { id: prefix + 'title', value: v.SearchTitle || '', placeholder: 'np. Archiwum', 'aria-label': 'Tytuł wyszukiwania' });
  const query = h('input', { id: prefix + 'query', value: v.SearchQuery || '', class: 'mono', placeholder: 'np. type:other_video AND tag:info', 'aria-label': 'Zapytanie wyszukiwania' });
  const picked = h('div', { class: 'picked' });
  const results = h('div', { class: 'picks', role: 'listbox', 'aria-label': 'Wyniki' });
  const body = h('div', { class: 'cp-body' });
  const el = h('div', { class: 'picker', id: prefix.replace(/-$/, '') }, kindSel, body);

  let seq = 0, timer = null;
  const showPicked = () => picked.replaceChildren(chosen ? h('span', null, 'Wybrano: ', h('b', null, chosen[1])) : h('span', { class: 'dim' }, 'Nic nie wybrano.'));
  const search = async () => {
    clearTimeout(timer);
    const d = CONTENT_KINDS[kind];
    const my = ++seq;
    if (!d.list && !d.search) return;
    results.replaceChildren(h('span', { class: 'dim' }, 'Szukam…'));
    try {
      let rows;
      if (d.list) {
        const needle = q.value.trim().toLowerCase();
        rows = (await d.list()).filter(([, n]) => n.toLowerCase().includes(needle)).slice(0, 20);
      } else {
        rows = (await cmd(d.search, { Query: q.value.trim() })).map(d.row);
      }
      if (my !== seq) return;
      results.replaceChildren(...(rows.length ? rows.map(([id, name]) => h('button', {
        type: 'button', role: 'option', class: chosen && chosen[0] === id ? 'on' : null, 'aria-selected': String(!!(chosen && chosen[0] === id)),
        onclick: () => { chosen = [id, name]; showPicked(); search(); el.dispatchEvent(new Event('change', { bubbles: true })); },
      }, name)) : [h('span', { class: 'dim' }, 'Nic nie znaleziono.')]));
    } catch (e) {
      if (my === seq) results.replaceChildren(h('span', { class: 'err' }, e.message));
    }
  };
  const renderBody = () => {
    const d = CONTENT_KINDS[kind];
    const miss = missing(d.needs || (d.search ? [d.search] : []));
    if (miss.length) return body.replaceChildren(missingNote(miss));
    if (d.query) {
      return body.replaceChildren(h('div', { class: 'form tight' },
        h('label', { for: prefix + 'title' }, 'Tytuł'), title,
        h('label', { for: prefix + 'query' }, 'Zapytanie'), query,
        h('div', { class: 'hint' }, 'Składnia wyszukiwarki legacy, np. type:movie AND genre:comedy.')));
    }
    showPicked();
    body.replaceChildren(q, results, picked);
    search();
  };
  kindSel.addEventListener('change', () => { clearTimeout(timer); seq++; kind = kindSel.value; chosen = null; q.value = ''; renderBody(); });
  q.addEventListener('input', () => { clearTimeout(timer); timer = setTimeout(search, 250); });
  renderBody();

  const peek = () => {
    const d = CONTENT_KINDS[kind];
    const out = { ...emptyContent(kind) };
    if (d.query) { out.SearchTitle = title.value.trim(); out.SearchQuery = query.value.trim(); }
    else if (chosen) out[d.field] = chosen[0];
    return out;
  };
  return {
    el,
    peek,
    read() {
      const out = peek();
      const d = CONTENT_KINDS[kind];
      if (opts.optional) return out;
      if (d.query && !out.SearchQuery) { query.focus(); throw new Error('Wybór treści: podaj zapytanie wyszukiwania.'); }
      if (!d.query && !ID_FIELDS.some((f) => out[f] != null)) { q.focus(); throw new Error(`Wybór treści: wybierz ${pl(kind)} z listy.`); }
      return out;
    },
    label: () => (CONTENT_KINDS[kind].query ? `${pl(kind)}: ${title.value || query.value}` : chosen ? `${pl(kind)}: ${chosen[1]}` : '—'),
  };
}

// ---------- grupy (bloków, szablonów, deco, szablonów deco, playlist) ----------
/**
 * Przyciski „+ Grupa” i „Usuń grupę…” nad ekranem z pozycjami w grupach.
 * cfg = { what: 'bloków', create: 'CreateBlockGroup', del: 'DeleteBlockGroup', idKey: 'BlockGroupId',
 *         groups: [{Id, Name}], count?: (grupa) => liczba, prefix: 'bg', placeholder? }
 */
export function groupActions(cfg) {
  const add = async () => {
    const input = h('input', { id: cfg.prefix + '-name', placeholder: cfg.placeholder || 'np. Dzienne' });
    const ok = await dialog(`Nowa grupa ${cfg.what}`, h('div', { class: 'form' }, h('label', { for: cfg.prefix + '-name' }, 'Nazwa grupy'), input), [['Anuluj', false], ['Załóż', true, 'acc']]);
    if (!ok || !input.value.trim()) return;
    try { await cmd(cfg.create, { Name: input.value.trim() }); toast('Założono grupę.', 'ok'); route(true); }
    catch (e) { toast('Legacy odrzucił: ' + e.message, 'bad'); }
  };
  const del = async () => {
    if (!cfg.groups.length) return toast('Nie ma grup do usunięcia.', 'bad');
    const sel = h('select', { id: cfg.prefix + '-del' }, cfg.groups.map((g) => h('option', { value: String(g.Id) }, cfg.count ? `${g.Name} (${cfg.count(g)})` : g.Name)));
    const ok = await dialog(`Usuń grupę ${cfg.what}`, h('div', { class: 'form' }, h('label', { for: cfg.prefix + '-del' }, 'Grupa'), sel,
      h('div', { class: 'hint' }, 'Usunięcie grupy usuwa też wszystko, co w niej jest. Tego nie da się cofnąć.')), [['Anuluj', false], ['Usuń', true, 'danger']]);
    if (!ok) return;
    try { await cmd(cfg.del, { [cfg.idKey]: Number(sel.value) }); toast('Usunięto grupę.', 'ok'); route(true); }
    catch (e) { toast('Legacy odrzucił: ' + e.message, 'bad'); }
  };
  return [btn('+ Grupa', add), btn('Usuń grupę…', del)];
}

// ---------- oś doby (szablon dnia, szablon deco, kalendarz tygodnia) ----------
/** Minuty od północy z „06:30:00” albo z DateTime legacy „2026-11-06T06:30:00” (bez strefy – czas szablonu). */
export function minutesOf(s) {
  const m = /(?:T|^)(\d{1,2}):(\d{2})/.exec(s || '');
  return m ? +m[1] * 60 + +m[2] : 0;
}
export const hhmm = (min) => `${String(Math.floor(min / 60) % 24).padStart(2, '0')}:${String(min % 60).padStart(2, '0')}`;
export const span = (min) => `${String(Math.floor(min / 60)).padStart(2, '0')}:${String(min % 60).padStart(2, '0')}:00`;

/**
 * Pasek 24 h: segs = [{ start, end (minuty, end może przekroczyć 1440), label, cls?, title? }].
 * Segmenty po północy zawijają się na początek doby.
 */
export function dayBar(segs, opts = {}) {
  const el = h('div', { class: 'daybar' + (opts.cls ? ' ' + opts.cls : ''), role: 'img', 'aria-label': opts.label || 'Oś doby' });
  if (opts.ruler !== false) {
    for (let hh = 0; hh <= 24; hh += 3) {
      const t = h('span', { class: 'tick' }, hh < 24 ? String(hh).padStart(2, '0') : '');
      t.style.left = (hh / 24) * 100 + '%';
      el.append(t);
    }
  }
  const put = (s, a, b) => {
    const x = h('div', { class: 'seg' + (s.cls ? ' ' + s.cls : ''), title: s.title || `${hhmm(s.start)}–${hhmm(s.end)} ${s.label}` }, s.label);
    x.style.left = (a / 1440) * 100 + '%';
    x.style.width = `calc(${((b - a) / 1440) * 100}% - 1px)`;
    el.append(x);
  };
  for (const s of segs) {
    const a = Math.max(0, s.start), b = Math.min(s.end, 2880);
    if (b <= 1440) put(s, a, b);
    else { put(s, a, 1440); put(s, 0, b - 1440); }
  }
  if (!segs.length) el.append(h('span', { class: 'empty-bar' }, opts.empty || 'pusto'));
  return el;
}
