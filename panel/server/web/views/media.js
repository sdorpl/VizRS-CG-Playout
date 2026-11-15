// Widoki grupy „Media”: Biblioteki (lokalne, ze ścieżkami i skanem), Przeglądarka (wyszukiwarka legacy
// z zakładkami rodzajów, stronicowanie, karta pozycji) i Kosz (pliki, których skaner nie znalazł).
// Odpowiednik Local Libraries, Library Editor, Search i Trash w legacy.
//
// Ścieżki bibliotek to ścieżki w kontenerze legacy (np. /config/media/…, /media/…), nie na komputerze operatora.
// Postępu skanu legacy nie udostępnia zapytaniem (idzie przez zdarzenia Blazora) – panel odświeża liczbę pozycji.

import { VIEWS, h, btn, head, panel, pill, cmd, pl, table, toast, dialog, confirmDlg, route, cmdNote, missing, missingNote, S, legacyUrl, has } from '../core.js';
import { followTask } from '../tasks.js';
import { playSection, seconds } from '../clip.js';
import { form } from '../form.js';
import { crudView } from '../parts.js';

// ---------- biblioteki lokalne ----------
const KIND_HINT = {
  Movies: 'Filmy: katalog na film (Tytuł (rok)/plik).', Shows: 'Seriale: Serial/Sezon 01/odcinek S01E01.',
  OtherVideos: 'Inne wideo: dowolne pliki, foldery stają się tagami.', MusicVideos: 'Teledyski: Artysta/plik.',
  Songs: 'Muzyka: dowolne pliki dźwiękowe.', Images: 'Obrazy: pliki graficzne; czas wyświetlania per folder.',
  RemoteStreams: 'Strumienie zdalne: pliki .yml z adresem strumienia (np. audycje na żywo).',
};

/** Edytor ścieżek: wiersz = pole ścieżki, liczba pozycji, „Przenieś do…”, usuń. */
function pathsEditor(paths, lib, libs) {
  let rows = paths.map((p) => ({ ...p }));
  const el = h('div', { class: 'paths', id: 'lib-paths' });
  const render = () => {
    el.replaceChildren(
      ...(rows.length ? [] : [h('div', { class: 'empty' }, 'Brak ścieżek – dodaj katalog w kontenerze legacy.')]),
      ...rows.map((r, i) => {
        const input = h('input', { value: r.Path, class: 'mono grow', placeholder: '/config/media/…', 'aria-label': `Ścieżka ${i + 1}`,
          oninput: (e) => (r.Path = e.target.value) });
        const others = libs.filter((l) => l.Id !== lib?.Id && l.MediaKind === lib?.MediaKind);
        return h('div', { class: 'bar path' }, input,
          r.Id ? h('span', { class: 'dim num' }, `${r.Count ?? '?'} poz.`) : h('span', { class: 'dim' }, 'nowa'),
          r.Id && others.length ? btn('Przenieś…', () => movePath(r, others)) : null,
          h('button', { type: 'button', class: 'danger', title: 'Usuń ścieżkę', 'aria-label': `Usuń ścieżkę ${i + 1}`, onclick: async () => {
            if (r.Id && r.Count && !(await confirmDlg('Usuń ścieżkę', `Usunąć ścieżkę „${r.Path}”? Po zapisie legacy usunie jej ${r.Count} poz. z bazy (pliki zostają).`, 'Usuń'))) return;
            rows.splice(i, 1);
            render();
          } }, '✕'));
      }),
      h('div', { class: 'bar' }, btn('+ Dodaj ścieżkę', () => { rows.push({ Id: 0, Path: '' }); render(); el.querySelector('div.path:last-of-type input')?.focus(); })),
      h('div', { class: 'hint' }, 'Ścieżki w kontenerze legacy (np. /config/media/kanal albo zamontowany /media/…), nie na Twoim komputerze. Usunięcie ścieżki usuwa z legacy jej pozycje (pliki zostają).'));
  };
  render();
  return {
    el,
    read() {
      const out = rows.map((r) => ({ Id: r.Id || 0, Path: r.Path.trim() }));
      if (out.some((r) => !r.Path)) throw new Error('Ścieżka biblioteki nie może być pusta.');
      if (out.some((r) => !r.Path.startsWith('/'))) throw new Error('Ścieżka musi być bezwzględna (zaczynać się od /) – to ścieżka w kontenerze legacy.');
      if (new Set(out.map((r) => r.Path)).size !== out.length) throw new Error('Ta sama ścieżka jest w bibliotece dwa razy.');
      return out;
    },
  };
}

async function movePath(p, others) {
  const sel = h('select', { id: 'move-target' }, others.map((l) => h('option', { value: String(l.Id) }, l.Name)));
  const ok = await dialog('Przenieś ścieżkę', h('div', { class: 'form' }, h('p', null, `Przenieść „${p.Path}” (${p.Count ?? '?'} poz.) do innej biblioteki tego samego rodzaju? Pozycje przechodzą razem ze ścieżką.`),
    h('label', { for: 'move-target' }, 'Do biblioteki'), sel), [['Anuluj', false], ['Przenieś', true, 'acc']]);
  if (!ok) return;
  try {
    const r = await cmd('MoveLocalLibraryPath', { LibraryPathId: p.Id, TargetLibraryId: Number(sel.value) });
    toast(r?.queued ? 'Zlecono przeniesienie ścieżki (legacy robi to w tle).' : 'Przeniesiono ścieżkę.', 'ok');
    setTimeout(() => route(true), 1000);
  } catch (e) { toast('Legacy odrzucił przeniesienie: ' + e.message, 'bad'); }
}

async function scan(lib) {
  try {
    await cmd('QueueLibraryScanByLibraryId', { LibraryId: lib.Id });
    toast(`${lib.Name}: zlecono skan. Liczba pozycji odświeża się co kilka sekund.`, 'ok');
    let n = 0;
    const t = setInterval(() => { if (++n > 6) clearInterval(t); route(true); }, 5000);
    S.timers.push(t);
  } catch (e) { toast('Legacy odrzucił skan: ' + e.message, 'bad'); }
}

VIEWS.media = crudView({
  title: 'Biblioteki',
  desc: 'Skąd biorą się pliki: biblioteki lokalne z katalogami w kontenerze legacy. Odpowiednik Media Sources → Local i Libraries.',
  listCmd: 'GetAllLocalLibraries',
  needs: ['GetLocalLibraryPaths', 'CountMediaItemsByLibrary', 'CountMediaItemsByLibraryPath', 'QueueLibraryScanByLibraryId', 'MoveLocalLibraryPath'],
  commands: { create: 'CreateLocalLibrary', update: 'UpdateLocalLibrary', delete: 'DeleteLocalLibrary' },
  noun: { list: 'Biblioteki lokalne', new: 'Nowa biblioteka', add: '+ Nowa biblioteka', create: 'Załóż bibliotekę', one: (l) => l.Name || 'Biblioteka', title: (l) => l.Name },
  load: async () => {
    const libs = await cmd('GetAllLocalLibraries');
    const counts = await Promise.all(libs.map((l) => cmd('CountMediaItemsByLibrary', { LibraryId: l.Id }).catch(() => null)));
    libs.forEach((l, i) => (l.Count = counts[i]));
    libs.sort((a, b) => a.MediaKind.localeCompare(b.MediaKind) || a.Name.localeCompare(b.Name, 'pl'));
    return { rows: libs };
  },
  loadOne: async (l) => {
    if (!l) return { paths: [] };
    const paths = await cmd('GetLocalLibraryPaths', { LocalLibraryId: l.Id });
    const counts = await Promise.all(paths.map((p) => cmd('CountMediaItemsByLibraryPath', { LibraryPathId: p.Id }).catch(() => null)));
    paths.forEach((p, i) => (p.Count = counts[i]));
    return { paths };
  },
  id: (l) => l.Id,
  cols: [['Name', 'Nazwa', { class: 'w' }], [(l) => pl(l.MediaKind), 'Rodzaj'], [(l) => l.Count ?? '?', 'Pozycje', { class: 'num' }]],
  editor: (l, ctx) => {
    ctx.current = l;
    const f = form('CreateLocalLibrary', [
      { key: 'Name', label: 'Nazwa', required: true },
      l ? null : { key: 'MediaKind', label: 'Rodzaj', hint: (v) => KIND_HINT[v.MediaKind] },
    ].filter(Boolean), l ? { Name: l.Name } : { Name: '', MediaKind: 'OtherVideos' }, { idPrefix: 'lib-' });
    ctx.pathsEd = pathsEditor(ctx.paths, l, ctx.rows);
    const info = l ? h('dl', { class: 'kv' }, h('dt', null, 'Rodzaj'), h('dd', null, pl(l.MediaKind)), h('dt', null, 'Pozycje'), h('dd', null, String(l.Count ?? '?'))) : null;
    return {
      el: h('div', null, info, f.el, h('div', { class: 'form' }, h('div', { class: 'sec' }, 'Ścieżki (w kontenerze legacy)')), ctx.pathsEd.el),
      read: () => ({ ...f.read(), Paths: ctx.pathsEd.read() }),
    };
  },
  beforeSave: (ctx, data) => { if (!data.Paths.length && !ctx.current) throw new Error('Dodaj co najmniej jedną ścieżkę.'); },
  create: (d) => ({ Name: d.Name.trim(), MediaKind: d.MediaKind, Paths: d.Paths.map((p) => p.Path) }),
  createdId: (r) => r?.Id,
  update: (d, l) => ({ Id: l.Id, Name: d.Name.trim(), Paths: d.Paths }),
  afterSave: async (id, ctx) => { if (!ctx.current) await scan({ Id: id, Name: 'Nowa biblioteka' }); },
  footer: (l) => (l ? h('div', { class: 'maint' }, h('h3', null, 'Skan'), h('div', { class: 'bar' }, btn('Skanuj teraz', () => scan(l), 'acc'),
    h('span', { class: 'dim' }, 'Legacy skanuje też sam co kilka godzin (Ustawienia → skaner).'))) : null),
  below: (l, ctx) => (ctx.rows.some((x) => x.MediaKind === 'Images') ? imageFoldersPanel() : null),
  del: (l) => ({ LocalLibraryId: l.Id }),
  confirmDelete: (l) => `Usunąć bibliotekę „${l.Name}”? Legacy usunie ${l.Count ?? 'jej'} pozycji z bazy (pliki na dysku zostają), znikną też z kolekcji i ramówek. Tego nie da się cofnąć.`,
});

// ---------- przeglądarka i wyszukiwarka ----------
/** Rodzaje pozycji: zakładka, zapytanie legacy, przedrostek typu w wyszukiwarce, klucz w QuerySearchIndexAllItems. */
const KINDS = [
  ['OtherVideos', 'QuerySearchIndexOtherVideos', 'other_video', 'OtherVideoIds'],
  ['Movies', 'QuerySearchIndexMovies', 'movie', 'MovieIds'],
  ['Shows', 'QuerySearchIndexShows', 'show', 'ShowIds'],
  ['Seasons', 'QuerySearchIndexSeasons', 'season', 'SeasonIds'],
  ['Episodes', 'QuerySearchIndexEpisodes', 'episode', 'EpisodeIds'],
  ['MusicVideos', 'QuerySearchIndexMusicVideos', 'music_video', 'MusicVideoIds'],
  ['Artists', 'QuerySearchIndexArtists', 'artist', 'ArtistIds'],
  ['Songs', 'QuerySearchIndexSongs', 'song', 'SongIds'],
  ['Images', 'QuerySearchIndexImages', 'image', 'ImageIds'],
  ['RemoteStreams', 'QuerySearchIndexRemoteStreams', 'remote_stream', 'RemoteStreamIds'],
];
const KIND_PL = { Seasons: 'sezony', Episodes: 'odcinki', Artists: 'artyści' };
const kindLabel = (k) => KIND_PL[k] || pl(k);
const STATE = { Normal: null, FileNotFound: ['brak pliku', 'bad'], Unavailable: ['niedostępny', 'warn'], RemoteOnly: ['tylko zdalnie', 'info'] };
const statePill = (s) => (STATE[s] ? pill(...STATE[s]) : s && s !== 'Normal' ? pill(s) : null);
const PAGE = 50;
const B = { q: '', tab: 'OtherVideos', page: 1, sel: new Set() }; // sel – zaznaczone MediaItemId bieżącej zakładki
// widok wyników: tabela albo kafle (jak w makiecie UI), pamiętany w przeglądarce
const VIEW_KEY = 'vizrs-live-media-view';
const viewMode = (set) => {
  try { if (set) localStorage.setItem(VIEW_KEY, set); return localStorage.getItem(VIEW_KEY) || 'tabela'; } catch { return set || 'tabela'; }
};
/** Okładka legacy przez serwer panelu (/api/artwork/… → /artwork/…; adres dla widzów bywa „localhost”) –
 * plakat filmu/serialu/sezonu, miniatura pozostałych; bez obrazu null. */
function poster(kind, c, cls) {
  if (!c?.Poster) return null;
  const dir = ['Movies', 'Shows', 'Seasons'].includes(kind) ? 'posters' : 'thumbnails';
  const src = `/api/artwork/${dir}/${String(c.Poster).split('/').map(encodeURIComponent).join('/')}`;
  const img = h('img', { src, alt: '', loading: 'lazy', class: cls });
  img.addEventListener('error', () => img.remove());
  return img;
}

/** Bieżące zapytanie jako kolekcja inteligentna (CreateSmartCollection) – potem Kolekcje. */
async function saveSmart(q) {
  const input = h('input', { id: 'smart-name', value: q.length > 40 ? '' : q, placeholder: 'np. Archiwum INFO24' });
  const ok = await dialog('Zapisz jako kolekcję inteligentną', h('div', { class: 'form' },
    h('label', { for: 'smart-name' }, 'Nazwa'), input,
    h('span'), h('span', { class: 'dim mono' }, 'Zapytanie: ' + q),
    h('span'), h('span', { class: 'dim' }, 'Kolekcja inteligentna to zapisane zapytanie – lista pozycji aktualizuje się sama po skanie bibliotek.')),
  [['Anuluj', false], ['Zapisz', true, 'acc']]);
  if (!ok) return;
  if (!input.value.trim()) return toast('Podaj nazwę kolekcji.', 'bad');
  try {
    await cmd('CreateSmartCollection', { Name: input.value.trim(), Query: q });
    toast(`Zapisano kolekcję inteligentną „${input.value.trim()}”.`, 'ok');
    location.hash = '#kolekcje';
  } catch (e) { toast('Legacy odrzucił kolekcję: ' + e.message, 'bad'); }
}

const FIELDS = 'title:, type:, tag:, genre:, library_name:, state:, minutes:, height:, video_codec:, release_date:, plot:';

// ---------- K8: serial – skan jednego serialu, wyodrębnianie napisów osadzonych ----------
async function scanShow(id, title) {
  let show;
  try { show = await cmd('GetTelevisionShowById', { Id: id }); } catch (e) { return toast('Legacy nie podał serialu: ' + e.message, 'bad'); }
  if (!show?.LibraryId) return toast('Legacy nie podał biblioteki serialu – skanuj całą bibliotekę (Biblioteki).', 'bad');
  try {
    const ok = await cmd('QueueShowScanByLibraryId', { LibraryId: show.LibraryId, ShowId: id, ShowTitle: show.Title || title, DeepScan: false });
    toast(ok === false ? `${title}: legacy nie zlecił skanu (biblioteka zewnętrzna bez synchronizacji pozycji albo skan już trwa).` : `${title}: zlecono skan serialu – nowe odcinki pojawią się po skanie.`, ok === false ? 'bad' : 'ok');
  } catch (e) { toast('Legacy odrzucił skan serialu: ' + e.message, 'bad'); }
}
async function extractShowSubtitles(id, title) {
  try { await cmd('ExtractEmbeddedShowSubtitles', { ShowId: id }); } catch (e) { return toast('Legacy odrzucił wyodrębnianie: ' + e.message, 'bad'); }
  await followTask({ label: `Napisy · ${title}`, match: /subtitle/i, timeoutMs: 30000,
    empty: 'Legacy wyodrębnia napisy w tle; wpis w logu pojawia się zwykle tylko przy błędzie albo na poziomie Debug. Napisy tekstowe trafią do pamięci legacy i będą dostępne dla kanałów.' });
}
function showActions(id, title) {
  const b = [
    has('GetTelevisionShowById') && has('QueueShowScanByLibraryId') ? btn('Skanuj serial', () => scanShow(id, title)) : null,
    has('ExtractEmbeddedShowSubtitles') ? btn('Wyodrębnij napisy', () => extractShowSubtitles(id, title)) : null,
  ].filter(Boolean);
  return b.length ? h('div', { class: 'bar pad', id: 'show-actions' }, ...b) : null;
}

// ---------- K8: czas wyświetlania obrazów w folderach (GetImageFolders, UpdateImageFolderDuration) ----------
const IMG = { parent: null, trail: [] };
function imageFoldersPanel() {
  if (!has('GetImageFolders') || !has('UpdateImageFolderDuration')) return null;
  const box = h('div', { 'data-image-folders': '' }, h('div', { class: 'dim pad' }, 'Wczytuję foldery…'));
  const load = async () => {
    let rows;
    try { rows = await cmd('GetImageFolders', { LibraryFolderId: IMG.parent }); } catch (e) { box.replaceChildren(h('div', { class: 'empty err' }, 'Legacy: ' + e.message)); return; }
    const save = async (f, input) => {
      const v = input.value.trim();
      const n = v === '' ? null : Number(v);
      if (n != null && (!Number.isFinite(n) || n <= 0 || n > 86400)) return toast('Czas: liczba sekund 0–86400 albo puste (domyślny z ustawień).', 'bad');
      try { await cmd('UpdateImageFolderDuration', { LibraryFolderId: f.LibraryFolderId, ImageFolderDuration: n }); toast(`${f.Name}: ${n == null ? 'czas domyślny' : n + ' s na obraz'}`, 'ok'); }
      catch (e) { toast('Legacy odrzucił czas: ' + e.message, 'bad'); }
    };
    const crumbs = h('div', { class: 'bar pad' }, btn('Wszystkie', () => { IMG.parent = null; IMG.trail = []; load(); }),
      IMG.trail.map((t, i) => [h('span', { class: 'dim' }, '›'), btn(t.name, () => { IMG.parent = t.id; IMG.trail = IMG.trail.slice(0, i + 1); load(); })]));
    box.replaceChildren(crumbs, table(rows, [
      [(f) => (f.SubfolderCount ? h('a', { href: '#media', onclick: (e) => { e.preventDefault(); IMG.parent = f.LibraryFolderId; IMG.trail.push({ id: f.LibraryFolderId, name: f.Name }); load(); } }, f.Name + ' ›') : f.Name), 'Folder'],
      [(f) => h('span', { class: 'mono dim' }, f.FullPath), 'Ścieżka', { class: 'w' }],
      ['SubfolderCount', 'Podfoldery', { class: 'num' }], ['ImageCount', 'Obrazy', { class: 'num' }],
      [(f) => {
        const input = h('input', { type: 'number', min: 1, step: 1, value: f.DurationSeconds ?? '', placeholder: 'domyślny', 'aria-label': `Czas obrazu w folderze ${f.Name}`, class: 'img-dur' });
        return h('span', { class: 'bar' }, input, h('span', { class: 'dim' }, 's'), btn('Zapisz', () => save(f, input)));
      }, 'Czas obrazu'],
    ], { empty: 'Brak folderów z obrazami – dodaj bibliotekę rodzaju „Obrazy” i ją zeskanuj.' }));
  };
  load();
  return panel('Foldery obrazów', h('span', { class: 'dim' }, 'jak długo kanał pokazuje jeden obraz; puste = domyślny czas z ustawień legacy'), box,
    cmdNote('GetImageFolders', 'UpdateImageFolderDuration'));
}

// Serial, sezon i artysta to pozycje zbiorcze bez pliku: GetMediaItemInfo legacy rzuca dla nich wyjątek
// (MediaItemExtensions.GetHeadVersion – brak wersji pliku). Ich karta pokazuje zawartość z indeksu wyszukiwania.
const GROUP_KINDS = ['Shows', 'Seasons', 'Artists'];
const PLAY_CMDS = ['GetAllFFmpegProfiles'];
const ALL = 1000;
const epLabel = (e) => `S${String(e.Season ?? 0).padStart(2, '0')}E${String(e.Episode ?? 0).padStart(2, '0')}`;
const quoted = (s) => `"${String(s).replace(/["\\]/g, ' ')}"`;

/** Karta serialu, sezonu albo artysty – bez GetMediaItemInfo. back – powrót do karty nadrzędnej. */
async function groupCard(id, title, kind, card, back) {
  const kv = (rows) => h('dl', { class: 'kv' }, rows.filter((r) => r[1] != null && r[1] !== '').flatMap(([k, v]) => [h('dt', null, k), h('dd', null, String(v))]));
  const buttons = [...(back ? [['‹ Wróć', 'back']] : []), ['Zamknij', true, 'acc']];
  const art = poster(kind === 'Artists' ? 'Shows' : kind, card, 'card-poster');
  const open = async (body) => { if ((await dialog(title, h('div', { class: 'card', id: 'item-card' }, art ? h('div', { class: 'card-head' }, art, h('div', null, body[0])) : body[0], body.slice(1)), buttons)) === 'back') back(); };
  if (kind === 'Artists') {
    return open([kv([['Rodzaj', 'artysta'], ['Id', id]]), h('p', { class: 'dim pad' }, 'Artysta grupuje utwory i teledyski – znajdziesz je w zakładkach „Utwory” i „Teledyski” (szukaj po tytule).')]);
  }
  const showTitle = kind === 'Shows' ? title : card?.ShowTitle || title;
  let eps = [], seasons = [];
  try {
    const [e, s] = await Promise.all([
      has('QuerySearchIndexEpisodes') ? cmd('QuerySearchIndexEpisodes', { Query: `type:episode AND show_title:${quoted(showTitle)}`, PageNumber: 1, PageSize: ALL }) : null,
      kind === 'Shows' && has('QuerySearchIndexSeasons') ? cmd('QuerySearchIndexSeasons', { Query: `type:season AND show_title:${quoted(showTitle)}`, PageNumber: 1, PageSize: ALL }) : null,
    ]);
    // ten sam tytuł może mieć kilka seriali – zostają tylko pozycje tego serialu / sezonu
    eps = (e?.Cards || []).filter((x) => (kind === 'Shows' ? x.ShowId === id : x.SeasonId === id));
    // bez numerów w nazwie pliku legacy daje E00 – wtedy kolejność naturalna po tytule (#2 przed #10)
    eps.sort((a, b) => (a.Season ?? 0) - (b.Season ?? 0) || (a.Episode ?? 0) - (b.Episode ?? 0)
      || String(a.Title || '').localeCompare(String(b.Title || ''), 'pl', { numeric: true }));
    const showId = kind === 'Shows' ? id : eps[0]?.ShowId;
    seasons = (s?.Cards || []).filter((x) => eps.some((ep) => ep.SeasonId === x.TelevisionSeasonId) || showId == null);
    seasons.sort((a, b) => (a.TelevisionSeasonNumber ?? 0) - (b.TelevisionSeasonNumber ?? 0));
  } catch (err) { toast('Legacy odrzucił wyszukiwanie odcinków: ' + err.message, 'bad'); }
  const self = () => groupCard(id, title, kind, card, back);
  // dwie kolumny (czytelne też na telefonie): numer ze stanem pod spodem, tytuł z plikiem pod spodem –
  // plik tylko, gdy coś wnosi (nazwa bez rozszerzenia ≠ tytuł)
  const epNo = (e) => h('div', { class: 'ep-no' },
    h('span', { class: 'mono' + (e.Episode ? '' : ' dim'), title: e.Episode ? null : 'Legacy nie odczytał numeru odcinka z nazwy pliku (wzór: S01E01).' }, epLabel(e)), statePill(e.State));
  const epTitle = (e) => {
    const file = (e.Path || '').split('/').pop();
    const same = file.replace(/\.[^.]+$/, '').trim() === String(e.Title || '').trim();
    return h('div', { class: 'ep-title' }, h('span', null, e.Title || file || '—'), file && !same ? h('span', { class: 'mono dim' }, file) : null);
  };
  const epTable = table(eps, [[epNo, 'Odcinek'], [epTitle, 'Tytuł / plik', { class: 'w' }]],
  { onRow: (e) => itemCard(e.MediaItemId, `${showTitle} · ${epLabel(e)} ${e.Title || ''}`.trim(), 'Episodes', { ...e, ShowPoster: card?.Poster || card?.ShowPoster }, self), empty: 'Brak odcinków w indeksie legacy – zeskanuj bibliotekę.' });
  const per = (sid) => eps.filter((e) => e.SeasonId === sid).length;
  const seasonTable = kind === 'Shows' && seasons.length ? [h('h3', null, 'Sezony'), table(seasons, [
    [(x) => `Sezon ${x.TelevisionSeasonNumber ?? '?'}`, 'Sezon'], [(x) => per(x.TelevisionSeasonId), 'Odcinki', { class: 'num' }], ['TelevisionSeasonId', 'Id', { class: 'num' }]],
  { onRow: (x) => groupCard(x.TelevisionSeasonId, `${showTitle} · sezon ${x.TelevisionSeasonNumber ?? '?'}`, 'Seasons', { ...x, Poster: x.Poster || card?.Poster }, self) })] : null;
  return open([
    kv([['Rodzaj', kind === 'Shows' ? 'serial' : 'sezon'], kind === 'Seasons' ? ['Serial', showTitle] : null, kind === 'Shows' ? ['Sezony', seasons.length] : null,
      ['Odcinki', eps.length], ['Id', id]].filter(Boolean)),
    kind === 'Shows' ? showActions(id, title) : null,
    seasonTable, h('h3', null, 'Odcinki'), epTable,
    cmdNote('QuerySearchIndexEpisodes', ...(kind === 'Shows' ? ['QuerySearchIndexSeasons'] : []))]);
}

/** Źródło pliku do odtwarzania wprost: Jellyfin (pozycje Jellyfin… z legacy) albo plik biblioteki lokalnej; Emby, Plex,
 * strumienie zdalne – null (zostaje legacy). Ścieżka: z karty (odcinki), GetMovieById (filmy), względna z karty „Inne”. */
async function directSource(id, kind, info, card) {
  const k = String(info.Kind || '');
  if (/^(Emby|Plex)/.test(k) || kind === 'RemoteStreams') return null;
  const source = /^Jellyfin/.test(k) ? 'jellyfin' : 'local';
  let path = card?.Path || card?.LocalPath;
  if (!path && kind === 'Movies' && has('GetMovieById')) path = (await cmd('GetMovieById', { Id: id }).catch(() => null))?.Path;
  if (!path && source === 'local' && kind === 'OtherVideos') path = card?.Subtitle;
  return path ? { source, title: card?.Title || info.Title || '', path } : null;
}

/** Karta pozycji: GetMediaItemInfo (strumienie, rozdziały, czas); serial, sezon, artysta – groupCard (lista odcinków, skan, napisy – K8). */
export async function itemCard(id, title, kind, card, back) {
  if (GROUP_KINDS.includes(kind)) return groupCard(id, title, kind, card, back);
  let info;
  try { info = await cmd('GetMediaItemInfo', { Id: id }); } catch (e) { return toast('Legacy nie zwrócił informacji o pozycji: ' + e.message, 'bad'); }
  const kv = (rows) => h('dl', { class: 'kv' }, rows.filter((r) => r[1] != null && r[1] !== '').flatMap(([k, v]) => [h('dt', null, k), h('dd', null, v instanceof Node ? v : String(v))]));
  const streams = table(info.Streams || [], [['Index', '#', { class: 'num' }], [(s) => ({ Video: 'obraz', Audio: 'dźwięk', Subtitle: 'napisy', Attachment: 'załącznik' }[s.Kind] || s.Kind), 'Rodzaj'],
    [(s) => (s.Codec || '').toUpperCase() === 'H264' ? 'H.264' : s.Codec, 'Kodek'], ['Profile', 'Profil'], ['Language', 'Język'], ['Channels', 'Kanały', { class: 'num' }],
    [(s) => [s.Default ? 'domyślny' : '', s.Forced ? 'wymuszony' : ''].filter(Boolean).join(', '), 'Flagi']], { empty: 'Brak strumieni – legacy jeszcze nie przeanalizował pliku.' });
  const chapters = info.Chapters?.length ? table(info.Chapters, [['ChapterId', '#', { class: 'num' }], ['StartTime', 'Od'], ['EndTime', 'Do'], ['Title', 'Tytuł']]) : null;
  // odtwarzanie w karcie (clip.js): plik wprost z Jellyfin albo biblioteki lokalnej (/api/vod/…, bez legacy i transkodowania),
  // zapasowo test odtwarzania legacy po 30 s; obrazy – bez
  let clip = null;
  if (kind !== 'Images') {
    const profiles = PLAY_CMDS.every(has) ? await cmd('GetAllFFmpegProfiles').catch(() => []) : [];
    const direct = await directSource(id, kind, info, card);
    clip = playSection(id, seconds(info.Duration), profiles, direct, info.Streams);
  }
  const art = poster(kind, card, 'card-poster') || (card?.ShowPoster ? poster('Shows', { Poster: card.ShowPoster }, 'card-poster') : null);
  const facts = kv([['Rodzaj', info.Kind], ['Biblioteka', info.LibraryName], ['Stan', statePill(info.State) || 'OK'], ['Czas', String(info.Duration || '').replace(/\.\d+$/, '') || null],
    ['Obraz', info.Width ? `${info.Width}×${info.Height}, ${info.DisplayAspectRatio || ''} ${info.VideoScanKind === 'Interlaced' ? 'z przeplotem' : ''}`.trim() : null],
    ['Klatki', info.RFrameRate], ['Id', info.Id]]);
  const v = await dialog(info.Title || title, h('div', { class: 'card', id: 'item-card' },
    art ? h('div', { class: 'card-head' }, art, facts) : facts,
    clip ? [h('h3', null, 'Odtwarzanie'), clip.el] : null,
    h('h3', null, 'Strumienie'), streams, chapters ? (info.Chapters.length > 12 ? h('details', { class: 'how' }, h('summary', null, `Rozdziały (${info.Chapters.length})`), chapters) : [h('h3', null, 'Rozdziały'), chapters]) : null, cmdNote('GetMediaItemInfo', ...(clip ? ['/api/vod (Jellyfin / plik)', 'legacy /api/troubleshoot/playback'] : []))),
  [...(back ? [['‹ Wróć', 'back']] : []), ['Zamknij', true, 'acc']]);
  clip?.stop();
  if (v === 'back') back();
}

async function search(kind, q, page) {
  const k = KINDS.find((x) => x[0] === kind);
  const query = q.trim() ? `type:${k[2]} AND (${q.trim()})` : `type:${k[2]}`;
  return cmd(k[1], { Query: query, PageNumber: page, PageSize: PAGE });
}

/** Liczby pozycji na zakładkach: QuerySearchIndexAllItems zwraca listy id po rodzajach. */
async function counts(q) {
  const r = await cmd('QuerySearchIndexAllItems', { Query: q.trim() || '*' });
  return Object.fromEntries(KINDS.map(([k, , , key]) => [k, r?.[key]?.length ?? 0]));
}

VIEWS.przegladarka = async () => {
  const names = ['QuerySearchIndexAllItems', 'GetMediaItemInfo', ...KINDS.map((k) => k[1])];
  const title = 'Przeglądarka mediów';
  const desc = 'Wszystko, co legacy zeskanował, z wyszukiwarką legacy. Odpowiednik Search i ekranów mediów.';
  const miss = missing(names);
  if (miss.length) return [head(title, desc), missingNote(miss)];
  const n = await counts(B.q);
  if (!n[B.tab]) B.tab = KINDS.find(([k]) => n[k])?.[0] || B.tab;
  const res = await search(B.tab, B.q, B.page);
  const total = res?.Count ?? 0;
  const pages = Math.max(1, Math.ceil(total / PAGE));

  const input = h('input', { id: 'media-q', type: 'search', value: B.q, class: 'mono grow', placeholder: 'np. Pogoda albo tag:info AND minutes:>1', 'aria-label': 'Szukaj w mediach' });
  const go = () => { B.q = input.value; B.page = 1; B.sel.clear(); route(); };
  input.addEventListener('keydown', (e) => { if (e.key === 'Enter') go(); });
  const tabs = h('div', { class: 'tabs' }, KINDS.map(([k]) => h('button', { type: 'button', class: k === B.tab ? 'on' : null, onclick: () => { B.tab = k; B.page = 1; B.sel.clear(); route(true); } }, `${kindLabel(k)} (${n[k]})`)));
  const rows = (res?.Cards || []).map((c) => ({ ...c }));
  const canAdd = !missing(['AddItemsToCollection', 'AddItemsToPlaylist', 'GetAllCollections', 'GetAllPlaylistGroups', 'GetPlaylistsByPlaylistGroupId']).length;
  const selCount = h('span', { class: 'dim', id: 'media-sel' });
  const showSel = () => (selCount.textContent = B.sel.size ? `zaznaczono ${B.sel.size} poz.` : '');
  showSel();
  const pickCol = [(c) => {
    const b = h('input', { type: 'checkbox', 'aria-label': `Zaznacz ${c.Title}`, checked: B.sel.has(c.MediaItemId) });
    b.addEventListener('change', () => { if (b.checked) B.sel.add(c.MediaItemId); else B.sel.delete(c.MediaItemId); showSel(); });
    return b;
  }, ''];
  const empty = B.q ? 'Nic nie pasuje do zapytania.' : 'Brak pozycji tego rodzaju – dodaj bibliotekę i ją zeskanuj.';
  const mode = viewMode();
  const list = mode === 'kafle'
    ? (rows.length ? h('div', { class: 'cards', 'data-media-tiles': '' }, rows.map((c) => {
      const tile = h('div', { class: 'mc', tabindex: '0', role: 'button', 'aria-label': `Karta pozycji ${c.Title}` },
        h('div', { class: 'th' }, poster(B.tab, c) || h('span', null, kindLabel(B.tab))),
        h('div', { class: 'tx' },
          h('span', { class: 'bar' }, canAdd ? pickCol[0](c) : null, h('b', null, c.Title)),
          h('span', { class: 'dim' }, c.Subtitle || ''), statePill(c.State)));
      const open = (e) => { if (!e.target.closest('input')) itemCard(c.MediaItemId, c.Title, B.tab, c); };
      tile.addEventListener('click', open);
      tile.addEventListener('keydown', (e) => { if (e.key === 'Enter') open(e); });
      return tile;
    })) : h('div', { class: 'empty' }, empty))
    : table(rows, [...(canAdd ? [pickCol] : []), ...(['Movies', 'Shows', 'Seasons'].includes(B.tab) ? [[(c) => poster(B.tab, c, 'row-poster') || h('span', { class: 'row-poster none' }), '']] : []), ['Title', 'Tytuł', { class: 'w' }], ['Subtitle', 'Opis / plik'], [(c) => statePill(c.State), 'Stan'], ['MediaItemId', 'Id', { class: 'num' }]],
      { onRow: (c) => itemCard(c.MediaItemId, c.Title, B.tab, c), empty });
  const modeBtn = (m, t) => { const b = btn(t, () => { viewMode(m); route(true); }); b.classList.toggle('on', m === mode); b.setAttribute('aria-pressed', String(m === mode)); return b; };
  const pager = pages > 1 ? h('div', { class: 'bar pad pager' },
    btn('‹ Poprzednia', () => { B.page--; route(true); }, B.page <= 1 ? 'off' : null), h('span', { class: 'dim' }, `strona ${B.page} z ${pages} · ${total} poz.`),
    btn('Następna ›', () => { B.page++; route(true); }, B.page >= pages ? 'off' : null)) : h('div', { class: 'bar pad dim' }, `${total} poz.`);
  pager.querySelectorAll('button.off').forEach((b) => (b.disabled = true));
  return [
    head(title, desc),
    h('div', { class: 'bar searchbar' }, input, btn('Szukaj', go, 'acc'), B.q ? btn('Wyczyść', () => { B.q = ''; B.page = 1; route(); }) : null,
      B.q.trim() && !missing(['CreateSmartCollection']).length ? btn('Zapisz jako kolekcję inteligentną', () => saveSmart(B.q.trim())) : null),
    h('div', { class: 'hint' }, `Bez pola szuka w tytule. Pola: ${FIELDS} Łącz AND / OR / NOT, frazy w cudzysłowie, np. library_name:"Archiwum INFO24".`),
    panel('Pozycje', h('span', { class: 'bar' }, h('span', { class: 'dim' }, 'kliknij pozycję, by zobaczyć kartę'), modeBtn('tabela', 'Tabela'), modeBtn('kafle', 'Kafle')), tabs, list, pager,
      canAdd && rows.length ? h('div', { class: 'bar pad' }, btn('Dodaj zaznaczone do…', () => addTo(B.tab), 'acc'), selCount) : null, cmdNote(KINDS.find((k) => k[0] === B.tab)[1], 'QuerySearchIndexAllItems', 'GetMediaItemInfo')),
  ];
};

/** Zaznaczone pozycje do kolekcji zwykłej albo playlisty (AddItemsToCollection / AddItemsToPlaylist). */
async function addTo(kind) {
  if (!B.sel.size) return toast('Zaznacz pozycje do dodania.', 'bad');
  const [cols, groups] = await Promise.all([cmd('GetAllCollections'), cmd('GetAllPlaylistGroups')]);
  const lists = (await Promise.all(groups.map((g) => cmd('GetPlaylistsByPlaylistGroupId', { PlaylistGroupId: g.Id })
    .then((ps) => ps.filter((p) => !p.IsSystem).map((p) => [p.Id, `${g.Name} / ${p.Name}`]))))).flat();
  const opts = [...cols.map((c) => ['c' + c.Id, 'Kolekcja: ' + c.Name]), ...lists.map(([id, n]) => ['p' + id, 'Playlista: ' + n])];
  if (!opts.length) return toast('Nie ma kolekcji ani playlist – załóż je w Kolekcjach albo Playlistach.', 'bad');
  const sel = h('select', { id: 'add-target' }, opts.map(([v, t]) => h('option', { value: v }, t)));
  const ok = await dialog('Dodaj do…', h('div', { class: 'form' }, h('p', null, `Dodać ${B.sel.size} poz. (${kindLabel(kind)})?`),
    h('label', { for: 'add-target' }, 'Dokąd'), sel), [['Anuluj', false], ['Dodaj', true, 'acc']]);
  if (!ok) return;
  const key = KINDS.find((k) => k[0] === kind)[3];
  const ids = Object.fromEntries(KINDS.map((k) => [k[3], k[3] === key ? [...B.sel] : []]));
  const id = Number(sel.value.slice(1));
  try {
    if (sel.value[0] === 'c') await cmd('AddItemsToCollection', { CollectionId: id, ...ids });
    else await cmd('AddItemsToPlaylist', { PlaylistId: id, ...ids });
    toast(`Dodano ${B.sel.size} poz.: ${opts.find((o) => o[0] === sel.value)[1]}.`, 'ok');
    B.sel.clear();
    route(true);
  } catch (e) { toast('Legacy odrzucił dodanie: ' + e.message, 'bad'); }
}

// ---------- kosz ----------
VIEWS.kosz = async () => {
  const title = 'Kosz';
  const desc = 'Pozycje, których plików skaner już nie znalazł (stan „brak pliku”). Odpowiednik Trash w legacy.';
  const miss = missing(['DeleteItemsFromDatabase', 'EmptyTrash', ...KINDS.map((k) => k[1])]);
  if (miss.length) return [head(title, desc), missingNote(miss)];
  const all = (await Promise.all(KINDS.map(async ([k, c]) => {
    const r = await cmd(c, { Query: 'state:FileNotFound', PageNumber: 1, PageSize: 500 });
    return (r?.Cards || []).map((x) => ({ ...x, Kind: k }));
  }))).flat();
  const picked = new Set();
  const del = async () => {
    if (!picked.size) return toast('Zaznacz pozycje do usunięcia.', 'bad');
    if (!(await confirmDlg('Usuń z bazy', `Usunąć z bazy legacy ${picked.size} poz.? Znikną z kolekcji i ramówek. Tego nie da się cofnąć.`, 'Usuń'))) return;
    try { await cmd('DeleteItemsFromDatabase', { MediaItemIds: [...picked] }); toast('Usunięto z bazy.', 'ok'); route(true); }
    catch (e) { toast('Legacy odrzucił: ' + e.message, 'bad'); }
  };
  const empty = async () => {
    if (!(await confirmDlg('Opróżnij kosz', `Usunąć z bazy legacy wszystkie pozycje bez pliku (${all.length})? Tego nie da się cofnąć.`, 'Opróżnij kosz'))) return;
    try { const r = await cmd('EmptyTrash'); toast(r?.queued ? 'Zlecono opróżnienie kosza.' : 'Kosz opróżniony.', 'ok'); setTimeout(() => route(true), 1000); }
    catch (e) { toast('Legacy odrzucił: ' + e.message, 'bad'); }
  };
  const cols = [
    [(c) => { const b = h('input', { type: 'checkbox', 'aria-label': `Zaznacz ${c.Title}` }); b.addEventListener('change', () => (b.checked ? picked.add(c.MediaItemId) : picked.delete(c.MediaItemId))); return b; }, ''],
    ['Title', 'Tytuł', { class: 'w' }], [(c) => kindLabel(c.Kind), 'Rodzaj'], ['Subtitle', 'Opis / plik'], ['MediaItemId', 'Id', { class: 'num' }]];
  return [
    head(title, desc, all.length ? btn('Opróżnij kosz', empty, 'danger') : null),
    panel('Pozycje bez pliku', h('span', { class: 'dim' }, String(all.length)),
      table(all, cols, { empty: 'Kosz jest pusty – wszystkie pliki są na miejscu.' }),
      all.length ? h('div', { class: 'bar pad' }, btn('Usuń zaznaczone z bazy', del, 'danger')) : null,
      cmdNote('QuerySearchIndex… (state:FileNotFound)', 'DeleteItemsFromDatabase', 'EmptyTrash')),
    h('div', { class: 'note' }, 'Plik wrócił na miejsce? Zeskanuj bibliotekę – pozycja wyjdzie z kosza sama.'),
  ];
};
