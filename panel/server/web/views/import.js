// Import ramówki z pliku (K11, wzorzec Vizrt E): CSV albo XML z systemu emisyjnego → playlista legacy.
// Plik czyta przeglądarka (nic nie trafia na /config – decyzja R2); dopasowanie do mediów legacy przez wyszukiwarkę,
// zapis istniejącymi komendami CreatePlaylist + ReplacePlaylistItems. Godziny z pliku są tylko informacją –
// emisję o stałej godzinie ustawia pozycja harmonogramu („stała godzina”). Opis: DOKUMENTACJA §4 „Import ramówki”.

import { h, cmd, has, dialog, toast, table, pill, route } from '../core.js';

/** Rodzaje pozycji do dopasowania: zapytanie legacy → CollectionType pozycji playlisty. */
const KINDS = [
  ['QuerySearchIndexOtherVideos', 'OtherVideo', 'inne wideo'], ['QuerySearchIndexMovies', 'Movie', 'film'],
  ['QuerySearchIndexEpisodes', 'Episode', 'odcinek'], ['QuerySearchIndexMusicVideos', 'MusicVideo', 'teledysk'],
  ['QuerySearchIndexSongs', 'Song', 'utwór'], ['QuerySearchIndexImages', 'Image', 'obraz'],
];
export const MAX_ROWS = 500;

/** Nazwa pliku bez katalogu i rozszerzenia (legacy bez NFO nadaje taki tytuł). */
export const rawStem = (p) => p.replace(/\\/g, '/').split('/').pop().replace(/\.[A-Za-z0-9]{1,5}$/, '');
/** To samo z separatorami `_` `.` `-` zamienionymi na spacje (tytuł z metadanych zwykle tak wygląda). */
export const stem = (p) => rawStem(p).replace(/[_.]+|-(?=\S)/g, ' ').replace(/\s+/g, ' ').trim();

/** Jeden wiersz CSV z cudzysłowami ("a;b" i "" w środku). */
function splitCsv(line, sep) {
  const out = []; let cur = '', q = false;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i];
    if (q) { if (ch === '"' && line[i + 1] === '"') { cur += '"'; i++; } else if (ch === '"') q = false; else cur += ch; }
    else if (ch === '"') q = true;
    else if (ch === sep) { out.push(cur); cur = ''; }
    else cur += ch;
  }
  out.push(cur);
  return out.map((x) => x.trim());
}

const HEAD = { start: /^(start|godz|godzina|czas startu|time|begin)/i, title: /^(tytu|title|nazwa|program|audycja)/i, path: /^(plik|ścieżka|sciezka|path|file|clip)/i, duration: /^(dług|dlug|czas trwania|duration|length)/i };

/**
 * CSV: kolumny start, tytuł albo ścieżka pliku, długość – rozpoznane po nagłówku (pl/en) albo w tej kolejności.
 * Separator: `;`, `,` albo tabulator (wykrywany z pierwszego wiersza). Zwraca [{ start, title, path, duration }].
 */
export function parseCsv(text) {
  const lines = text.replace(/^\uFEFF/, '').split(/\r?\n/).filter((l) => l.trim() && !l.trim().startsWith('#'));
  if (!lines.length) return [];
  const sep = ['\t', ';', ','].map((s) => [s, lines[0].split(s).length]).sort((a, b) => b[1] - a[1])[0][0];
  let cols = { start: 0, title: 1, duration: 2, path: -1 };
  const first = splitCsv(lines[0], sep);
  if (first.some((c) => Object.values(HEAD).some((re) => re.test(c)))) {
    cols = { start: -1, title: -1, path: -1, duration: -1 };
    first.forEach((c, i) => { for (const [k, re] of Object.entries(HEAD)) if (cols[k] < 0 && re.test(c)) cols[k] = i; });
    lines.shift();
  }
  return lines.map((l) => {
    const f = splitCsv(l, sep);
    const get = (k) => (cols[k] >= 0 ? f[cols[k]] || '' : '');
    let title = get('title'), path = get('path');
    if (!path && /[\\/]/.test(title)) { path = title; title = ''; }
    return { start: get('start'), title, path, duration: get('duration') };
  }).filter((r) => r.title || r.path);
}

/** XML: każdy element z dzieckiem/atrybutem title|tytul|path|plik (np. <event>, <item>, <program>). */
export function parseXml(text) {
  const doc = new DOMParser().parseFromString(text, 'application/xml');
  if (doc.querySelector('parsererror')) throw new Error('plik XML jest niepoprawny');
  const pick = (el, names) => {
    for (const n of names) {
      const a = el.getAttribute(n);
      if (a) return a.trim();
      const c = [...el.children].find((x) => x.localName.toLowerCase() === n);
      if (c) return c.textContent.trim();
    }
    return '';
  };
  const rows = [];
  for (const el of doc.querySelectorAll('*')) {
    const title = pick(el, ['title', 'tytul', 'tytuł', 'name']);
    const path = pick(el, ['path', 'plik', 'file', 'clip']);
    if (!title && !path) continue;
    if ([...el.children].some((c) => pick(c, ['title', 'tytul', 'tytuł', 'path', 'plik', 'file']))) continue; // rodzic zdarzeń
    rows.push({ start: pick(el, ['start', 'time', 'godzina', 'begin']), title, path, duration: pick(el, ['duration', 'dlugosc', 'długość', 'length']) });
  }
  return rows.filter((r) => r.title || r.path);
}

/** Kandydaci legacy dla wiersza: po tytule (z pliku albo z nazwy pliku), dokładna zgodność bez wielkości liter. */
async function candidates(row, cache) {
  if (!row.title && row.path) {
    // ścieżka: najpierw tytuł = nazwa pliku (legacy bez metadanych), potem nazwa z separatorami zamienionymi na spacje
    const raw = await candidates({ ...row, title: rawStem(row.path) }, cache);
    return raw.length && !raw[0].loose ? raw : candidates({ ...row, title: stem(row.path) }, cache);
  }
  const want = (row.title || '').trim();
  if (!want) return [];
  if (cache.has(want)) return cache.get(want);
  const q = `title:"${want.replace(/["\\]/g, ' ')}"`;
  const res = await Promise.all(KINDS.filter(([n]) => has(n)).map(([n, type, label]) =>
    cmd(n, { Query: q, PageNumber: 1, PageSize: 10 }).then((r) => (r?.Cards || []).map((c) => ({ id: c.MediaItemId, title: c.Title, sub: c.Subtitle || '', type, label }))).catch(() => [])));
  const all = res.flat();
  const norm = (s) => s.toLocaleLowerCase('pl').trim();
  let exact = all.filter((c) => norm(c.title) === norm(want));
  // ścieżka w pliku: gdy kilka pozycji ma ten tytuł, wygrywa ta z pasującą ścieżką w opisie karty
  if (row.path && exact.length > 1) { const byPath = exact.filter((c) => c.sub && row.path.endsWith(c.sub.replace(/^.*[\\/]/, ''))); if (byPath.length) exact = byPath; }
  const out = exact.length ? exact : all.slice(0, 5).map((c) => ({ ...c, loose: true }));
  cache.set(want, out);
  return out;
}

/** Okno importu: plik → podgląd dopasowania → nowa playlista w wybranej grupie. */
export async function importSchedule(groups) {
  if (!groups.length) return toast('Najpierw załóż grupę playlist („+ Grupa”).', 'bad');
  const file = h('input', { type: 'file', id: 'imp-file', accept: '.csv,.txt,.tsv,.xml,text/csv,text/plain,application/xml,text/xml', 'aria-label': 'Plik ramówki' });
  const name = h('input', { id: 'imp-name', placeholder: 'np. Ramówka 12.11', 'aria-label': 'Nazwa playlisty' });
  const group = h('select', { id: 'imp-group', 'aria-label': 'Grupa playlist' }, groups.map((g) => h('option', { value: g.Id }, g.Name)));
  const go = await dialog('Import ramówki z pliku', h('div', { class: 'form imp' },
    h('p', { class: 'dim' }, 'CSV (start; tytuł albo ścieżka pliku; długość – z nagłówkiem po polsku lub angielsku albo w tej kolejności; separator ; , albo tabulator) albo XML (elementy z title/path). Panel dopasuje pozycje do mediów legacy i założy z nich playlistę – do użycia w harmonogramie. Godziny z pliku są tylko informacją.'),
    h('label', { for: 'imp-file' }, 'Plik'), file, h('label', { for: 'imp-name' }, 'Nazwa playlisty'), name, h('label', { for: 'imp-group' }, 'Grupa'), group),
  [['Anuluj', false], ['Dalej', true, 'acc']]);
  if (!go) return;
  const f = file.files?.[0];
  if (!f) return toast('Wybierz plik ramówki.', 'bad');
  if (f.size > 2 * 1024 * 1024) return toast('Plik większy niż 2 MB – podziel ramówkę.', 'bad');
  let rows;
  try {
    const text = await f.text();
    rows = /\.xml$/i.test(f.name) || /^\s*</.test(text) ? parseXml(text) : parseCsv(text);
  } catch (e) { return toast('Nie da się odczytać pliku: ' + e.message, 'bad'); }
  if (!rows.length) return toast('W pliku nie ma pozycji (tytuł albo ścieżka).', 'bad');
  if (rows.length > MAX_ROWS) return toast(`Za dużo pozycji (${rows.length}) – najwyżej ${MAX_ROWS} na import.`, 'bad');
  toast(`Dopasowuję ${rows.length} pozycji do mediów legacy…`);
  const cache = new Map();
  for (let i = 0; i < rows.length; i += 4) {
    await Promise.all(rows.slice(i, i + 4).map(async (r) => { r.cands = await candidates(r, cache); }));
  }
  const sels = rows.map((r, i) => {
    const s = h('select', { 'aria-label': `Dopasowanie pozycji ${i + 1}`, 'data-imp-row': String(i) },
      h('option', { value: '' }, '— pomiń —'),
      r.cands.map((c, j) => h('option', { value: String(j) }, `${c.title} · ${c.label}${c.sub ? ' · ' + c.sub : ''}${c.loose ? ' (podobny)' : ''}`)));
    s.value = r.cands.length && !r.cands[0].loose ? '0' : '';
    return s;
  });
  const state = (r) => (!r.cands.length ? pill('brak', 'bad') : r.cands[0].loose ? pill('podobne', 'warn') : r.cands.length > 1 ? pill('niejednoznaczne', 'warn') : pill('dopasowane', 'ok'));
  const count = (k) => rows.filter((r) => (k === 'ok' ? r.cands.length === 1 && !r.cands[0].loose : k === 'none' ? !r.cands.length : r.cands.length && (r.cands.length > 1 || r.cands[0].loose))).length;
  const ok = await dialog(`Podgląd importu · ${f.name}`, h('div', { class: 'imp-prev' },
    h('div', { class: 'bar' }, pill(`dopasowane ${count('ok')}`, 'ok'), pill(`do wyboru ${count('amb')}`, 'warn'), pill(`brak ${count('none')}`, 'bad')),
    table(rows, [[(r) => r.start || '—', 'Start', { class: 'num' }], [(r) => r.title || r.path, 'Z pliku', { class: 'w' }], [(r) => r.duration || '—', 'Długość', { class: 'num' }],
      [state, 'Stan'], [(r) => sels[rows.indexOf(r)], 'Pozycja legacy']]),
    h('p', { class: 'dim' }, 'Niedopasowane pozycje są pomijane. Kolejność playlisty = kolejność w pliku.')),
  [['Anuluj', false], ['Utwórz playlistę', true, 'acc']]);
  if (!ok) return;
  const items = rows.map((r, i) => (sels[i].value === '' ? null : r.cands[Number(sels[i].value)])).filter(Boolean)
    .map((c, n) => ({ Index: n, CollectionType: c.type, CollectionId: null, MultiCollectionId: null, SmartCollectionId: null, MediaItemId: c.id, PlaybackOrder: 'None', Count: null, PlayAll: false, IncludeInProgramGuide: true }));
  if (!items.length) return toast('Żadna pozycja nie jest dopasowana – nic nie zapisano.', 'bad');
  const plName = name.value.trim() || `Import ${f.name.replace(/\.[^.]+$/, '')}`;
  try {
    const created = await cmd('CreatePlaylist', { PlaylistGroupId: Number(group.value), Name: plName });
    const id = created?.Id ?? created?.PlaylistId;
    if (id == null) throw new Error('legacy nie podał identyfikatora nowej playlisty');
    await cmd('ReplacePlaylistItems', { PlaylistId: id, Name: plName, Items: items });
    toast(`Playlista „${plName}”: ${items.length} pozycji z ${rows.length} w pliku. Użyj jej w harmonogramie.`, 'ok');
    route(true);
  } catch (e) { toast('Legacy odrzucił import: ' + e.message, 'bad'); }
}
