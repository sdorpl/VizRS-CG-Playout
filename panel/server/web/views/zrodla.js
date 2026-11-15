// Widok „Źródła zewnętrzne” (R8): Jellyfin, Emby, Plex i strumienie zdalne.
// Odpowiednik Media Sources → Jellyfin / Emby / Plex, Libraries Editor i Path Replacements Editor w legacy.
//
// Klucz API serwera mediów jest w panelu tylko do zapisu: serwer panelu maskuje go w każdej odpowiedzi
// (mask_secrets w server/src/main.rs), więc przy każdej zmianie połączenia trzeba wpisać go ponownie.
// Synchronizacje idą do kolejki skanera legacy (wynik `queued`); postęp i błędy – w Logach.

import { VIEWS, h, btn, head, panel, pill, cmd, pl, table, toast, confirmDlg, route, cmdNote, missing, missingNote, S, dateTime } from '../core.js';

const MASK = '********';
const TAB = { i: 0 };

/** Polecenia źródła: Jellyfin i Emby mają te same komendy z innym przedrostkiem. */
const names = (K) => ({
  all: `GetAll${K}MediaSources`, libs: `Get${K}LibrariesBySourceId`, prefs: `Update${K}LibraryPreferences`,
  syncSources: `Synchronize${K}MediaSources`, syncLibs: `Synchronize${K}Libraries`, force: `ForceSynchronize${K}LibraryById`,
  ifNeeded: `Synchronize${K}LibraryByIdIfNeeded`, colls: `Synchronize${K}Collections`,
  paths: `Get${K}PathReplacementsBySourceId`, savePaths: `Update${K}PathReplacements`,
});

const queuedMsg = (what) => `${what}: zlecono (kolejka skanera legacy). Postęp i błędy – w Logach.`;
async function run(name, args, okMsg, errMsg) {
  try {
    const r = await cmd(name, args);
    toast(okMsg, 'ok');
    return r ?? true;
  } catch (e) { toast(`${errMsg}: ${e.message}`, 'bad'); return null; }
}

/** Biblioteki serwera: zaznaczenie „synchronizuj”, synchronizacja pojedynczej biblioteki. */
async function librariesPanel(K, src, n) {
  const libs = (await cmd(n.libs, { [`${K}MediaSourceId`]: src.Id })) || [];
  libs.sort((a, b) => String(a.MediaKind).localeCompare(String(b.MediaKind)) || a.Name.localeCompare(b.Name, 'pl'));
  const was = new Map(libs.map((l) => [l.Id, l.ShouldSyncItems]));
  const id = (l) => `src-${K}-${src.Id}-lib-${l.Id}`;
  const sync = async (l) => {
    await run(n.syncLibs, { [`${K}MediaSourceId`]: src.Id }, queuedMsg('Lista bibliotek'), 'Legacy odrzucił odświeżenie');
    if (await run(n.force, { [`${K}LibraryId`]: l.Id, DeepScan: false }, queuedMsg(`${l.Name}: synchronizacja`), 'Legacy odrzucił synchronizację')
      && K === 'Plex') await run('SynchronizePlexNetworks', { PlexLibraryId: l.Id, ForceScan: true }, queuedMsg(`${l.Name}: synchronizacja`), 'Legacy odrzucił synchronizację sieci');
  };
  const save = async () => {
    const prefs = libs.map((l) => ({ Id: l.Id, ShouldSyncItems: document.getElementById(id(l)).checked }));
    if (!(await run(n.prefs, { Preferences: prefs }, 'Zapisano wybór bibliotek.', 'Legacy odrzucił wybór bibliotek'))) return;
    // jak Libraries Editor legacy: nowo włączone biblioteki od razu do synchronizacji
    for (const p of prefs.filter((p) => p.ShouldSyncItems && !was.get(p.Id))) {
      await run(n.syncLibs, { [`${K}MediaSourceId`]: src.Id }, queuedMsg('Synchronizacja'), 'Legacy odrzucił odświeżenie');
      await run(n.ifNeeded, { [`${K}LibraryId`]: p.Id }, queuedMsg('Synchronizacja'), 'Legacy odrzucił synchronizację');
    }
    route(true);
  };
  return h('div', { class: 'maint' }, h('h3', null, 'Biblioteki'),
    table(libs, [
      [(l) => h('input', { type: 'checkbox', id: id(l), checked: l.ShouldSyncItems, 'aria-label': `Synchronizuj ${l.Name}` }), 'Synchr.'],
      [(l) => h('label', { for: id(l) }, l.Name), 'Nazwa', { class: 'w' }],
      [(l) => pl(l.MediaKind), 'Rodzaj'],
      [(l) => (was.get(l.Id) ? btn('Synchronizuj', () => sync(l)) : h('span', { class: 'dim' }, '—')), '', { class: 'act' }],
    ], { empty: 'Legacy nie zna jeszcze bibliotek tego serwera – „Odśwież listę bibliotek”.' }),
    h('div', { class: 'bar' }, btn('Zapisz wybór bibliotek', save, 'acc'),
      btn('Odśwież listę bibliotek', async () => { await run(n.syncLibs, { [`${K}MediaSourceId`]: src.Id }, queuedMsg('Lista bibliotek'), 'Legacy odrzucił odświeżenie'); setTimeout(() => route(true), 3000); }),
      btn('Synchronizuj kolekcje', () => run(n.colls, { [`${K}MediaSourceId`]: src.Id, ForceScan: true, DeepScan: false }, queuedMsg('Kolekcje'), 'Legacy odrzucił synchronizację kolekcji')),
      h('span', { class: 'dim' }, 'Tylko zaznaczone biblioteki trafiają do legacy (i do Przeglądarki).')));
}

/** Zamiany ścieżek: ścieżka na serwerze mediów → ta sama ścieżka w kontenerze legacy. */
async function pathsPanel(K, src, n) {
  let rows = ((await cmd(n.paths, { [`${K}MediaSourceId`]: src.Id })) || []).map((p) => ({ Id: p.Id, Remote: p[`${K}Path`] ?? '', Local: p.LocalPath ?? '' }));
  const box = h('div', { class: 'paths', id: `src-${K}-${src.Id}-paths` });
  const render = () => box.replaceChildren(
    ...(rows.length ? [] : [h('div', { class: 'empty' }, 'Brak zamian – legacy czyta pliki pod tymi samymi ścieżkami co serwer mediów (albo strumieniuje z serwera).')]),
    ...rows.map((r, i) => h('div', { class: 'bar path' },
      h('input', { value: r.Remote, class: 'mono grow', placeholder: `ścieżka w ${K}`, 'aria-label': `Ścieżka w ${K} ${i + 1}`, oninput: (e) => (r.Remote = e.target.value) }),
      h('span', { class: 'dim' }, '→'),
      h('input', { value: r.Local, class: 'mono grow', placeholder: 'ścieżka w kontenerze legacy', 'aria-label': `Ścieżka w legacy ${i + 1}`, oninput: (e) => (r.Local = e.target.value) }),
      h('button', { type: 'button', class: 'danger', title: 'Usuń zamianę', 'aria-label': `Usuń zamianę ${i + 1}`, onclick: () => { rows.splice(i, 1); render(); } }, '✕'))),
    h('div', { class: 'bar' }, btn('+ Dodaj zamianę', () => { rows.push({ Id: 0, Remote: '', Local: '' }); render(); }),
      btn('Zapisz zamiany ścieżek', async () => {
        if (rows.some((r) => !r.Remote.trim() || !r.Local.trim())) { toast('Zamiana ścieżki wymaga obu ścieżek.', 'bad'); return; }
        const items = rows.map((r) => ({ Id: r.Id || 0, [`${K}Path`]: r.Remote.trim(), LocalPath: r.Local.trim() }));
        if (await run(n.savePaths, { [`${K}MediaSourceId`]: src.Id, PathReplacements: items }, 'Zapisano zamiany ścieżek.', 'Legacy odrzucił zamiany ścieżek')) route(true);
      }, 'acc')));
  render();
  return h('div', { class: 'maint' }, h('h3', null, 'Zamiany ścieżek'), box);
}

async function sourcePanel(K, src, n, extra) {
  const [libs, paths] = await Promise.all([librariesPanel(K, src, n), pathsPanel(K, src, n)]);
  return h('div', { class: 'panel src', 'data-source': `${K}-${src.Id}` },
    h('header', null, h('h2', null, src.Name || `${K} #${src.Id}`), h('span', { class: 'mono dim' }, src.Address || ''),
      src.MissingSince ? pill(`niedostępny od ${dateTime(src.MissingSince)}`, 'bad') : null, h('span', { class: 'grow' }), extra),
    libs, paths);
}

/** Jellyfin albo Emby: jeden serwer (adres + klucz API) w magazynie sekretów legacy. */
async function keyed(K) {
  const n = names(K);
  const need = [n.all, `Get${K}Secrets`, `Save${K}Secrets`, `Disconnect${K}`, n.libs, n.prefs, n.syncLibs, n.force, n.paths, n.savePaths];
  const miss = missing(need);
  if (miss.length) return [missingNote(miss)];
  const [secrets, sources] = await Promise.all([cmd(`Get${K}Secrets`), cmd(n.all)]);
  const keySaved = secrets?.ApiKey === MASK;
  const addr = h('input', { id: `src-${K}-address`, value: secrets?.Address || '', class: 'mono', placeholder: 'http://jellyfin:8096', autocomplete: 'off' });
  const key = h('input', { id: `src-${K}-apikey`, type: 'password', autocomplete: 'new-password',
    placeholder: keySaved ? 'zapisany – wpisz ponownie, żeby zmienić połączenie' : 'klucz API z panelu administratora serwera' });
  const save = async () => {
    const Address = addr.value.trim(), ApiKey = key.value.trim();
    if (!/^https?:\/\/\S+$/.test(Address)) { toast('Adres serwera musi zaczynać się od http:// albo https://.', 'bad'); return; }
    if (!ApiKey) { toast('Wpisz klucz API (panel nie przechowuje go w przeglądarce).', 'bad'); return; }
    if (await run(`Save${K}Secrets`, { Secrets: { Address, ApiKey } }, `Połączono z ${K}. Legacy pobiera listę bibliotek.`, `Legacy nie połączył się z ${K}`)) setTimeout(() => route(true), 2000);
  };
  const disconnect = async () => {
    if (!(await confirmDlg(`Rozłącz ${K}`, `Rozłączyć ${K}? Legacy usunie serwer, jego biblioteki i wszystkie pozycje z bazy (znikną z kolekcji i ramówek) oraz zapisany klucz API. Tego nie da się cofnąć.`, 'Rozłącz'))) return;
    if (await run(`Disconnect${K}`, {}, `Rozłączono ${K}.`, 'Legacy odrzucił rozłączenie')) route();
  };
  const conn = panel('Połączenie', null,
    h('div', { class: 'form' },
      h('label', { for: addr.id }, 'Adres serwera'), addr,
      h('div', { class: 'hint' }, 'Adres widziany z kontenera legacy (np. nazwa usługi w sieci Dockera), nie z Twojej przeglądarki.'),
      h('label', { for: key.id }, 'Klucz API'), key,
      h('div', { class: 'hint' }, keySaved ? 'Klucz jest zapisany w legacy. Panel go nie pokazuje (tylko zapis).' : 'Legacy sprawdzi połączenie przed zapisem.')),
    h('div', { class: 'bar maint' }, btn(secrets?.Address ? 'Zapisz i połącz ponownie' : 'Połącz', save, 'acc'),
      sources.length ? btn('Odśwież serwer', () => run(n.syncSources, {}, queuedMsg('Serwer'), 'Legacy odrzucił odświeżenie')) : null,
      h('span', { class: 'grow' }),
      sources.length || secrets?.Address ? btn(`Rozłącz ${K}…`, disconnect, 'danger') : null));
  const panels = await Promise.all(sources.map((s) => sourcePanel(K, s, n)));
  return [conn, ...panels, cmdNote(...need)];
}

// ---------- Plex ----------
let plexPoll = null;
async function plex() {
  const n = names('Plex');
  const need = [n.all, 'StartPlexPinFlow', 'SignOutOfPlex', 'DeletePlexMediaSource', n.libs, n.prefs, n.syncLibs, n.force, n.paths, n.savePaths];
  const miss = missing(need);
  if (miss.length) return [missingNote(miss)];
  const sources = await cmd(n.all);
  const link = h('div', { id: 'plex-pin' });
  if (plexPoll) link.append(plexPoll.note);
  const signIn = async (force) => {
    let url;
    try { url = await cmd('StartPlexPinFlow', { ForceNewCredentials: force }); } catch (e) { toast('Legacy nie rozpoczął logowania do Plex: ' + e.message, 'bad'); return; }
    if (typeof url !== 'string' || !/^https:\/\//.test(url)) { toast('Legacy zwrócił nieoczekiwany adres logowania Plex.', 'bad'); return; }
    window.open(url, '_blank', 'noopener');
    const note = h('div', { class: 'note' }, 'Zaloguj się do Plex na otwartej stronie (jeśli się nie otworzyła: ',
      h('a', { href: url, target: '_blank', rel: 'noopener noreferrer' }, 'otwórz logowanie Plex'),
      '). Legacy czeka 2 minuty i sam dopisze serwery – lista odświeży się po zalogowaniu.');
    plexPoll = { note, until: Date.now() + 130000, count: sources.length };
    route(true);
  };
  if (plexPoll) {
    const t = setInterval(async () => {
      if (!plexPoll || Date.now() > plexPoll.until) { plexPoll = null; clearInterval(t); route(true); return; }
      const now = await cmd(n.all).catch(() => null);
      if (now && now.length !== plexPoll.count) { plexPoll = null; toast('Plex: dodano serwery.', 'ok'); route(true); }
    }, 5000);
    S.timers.push(t);
  }
  const signOut = async () => {
    if (!(await confirmDlg('Wyloguj z Plex', 'Wylogować z Plex? Legacy usunie wszystkie serwery Plex, ich biblioteki i pozycje z bazy (znikną z kolekcji i ramówek). Tego nie da się cofnąć.', 'Wyloguj'))) return;
    if (await run('SignOutOfPlex', {}, 'Wylogowano z Plex.', 'Legacy odrzucił wylogowanie')) route();
  };
  const del = (s) => async () => {
    if (!(await confirmDlg('Usuń serwer Plex', `Usunąć serwer „${s.Name}”? Legacy usunie jego biblioteki i pozycje z bazy. Tego nie da się cofnąć.`, 'Usuń'))) return;
    if (await run('DeletePlexMediaSource', { PlexMediaSourceId: s.Id }, `Usunięto serwer ${s.Name}.`, 'Legacy odrzucił usunięcie')) route();
  };
  const conn = panel('Konto Plex', null,
    h('p', { class: 'pad' }, 'Logowanie kodem PIN: panel otwiera stronę Plex, tam logujesz się swoim kontem. Legacy dostaje token i sam dodaje serwery tego konta; token nie trafia do panelu.'),
    link,
    h('div', { class: 'bar maint' }, btn(sources.length ? 'Dodaj serwery (zaloguj ponownie)' : 'Zaloguj do Plex', () => signIn(false), 'acc'),
      btn('Zaloguj innym kontem', () => signIn(true)),
      sources.length ? btn('Odśwież serwery', () => run('SynchronizePlexMediaSources', {}, 'Plex: odświeżanie serwerów zlecone.', 'Legacy odrzucił odświeżenie')) : null,
      h('span', { class: 'grow' }), sources.length ? btn('Wyloguj z Plex…', signOut, 'danger') : null));
  const panels = await Promise.all(sources.map((s) => sourcePanel('Plex', s, n, btn('Usuń serwer…', del(s), 'danger'))));
  return [conn, ...panels, cmdNote(...need)];
}

// ---------- strumienie zdalne ----------
const YAML_EXAMPLE = `# /config/media/strumienie/studio.yml
title: Studio na żywo
url: http://kamera:8080/live.m3u8
is_live: true
duration: 01:00:00   # dla ramówki: ile trwa pozycja`;

async function remote() {
  if (missing('GetAllLocalLibraries').length) return [missingNote(['GetAllLocalLibraries'])];
  const libs = (await cmd('GetAllLocalLibraries')).filter((l) => l.MediaKind === 'RemoteStreams');
  return [panel('Strumienie zdalne', null,
    h('p', { class: 'pad' }, 'Strumień zdalny (np. audycja na żywo, kamera, inny kanał) to w legacy plik .yml w bibliotece lokalnej rodzaju „strumienie zdalne”. ',
      'Panel nie zapisuje plików w kontenerze legacy – połóż plik na wolumenie konfiguracji i zeskanuj bibliotekę. Pozycje widać w Przeglądarce (zakładka „strumienie zdalne”) i dodaje się je do kolekcji i playlist jak pliki.'),
    h('div', { class: 'pad' }, h('pre', { class: 'out' }, YAML_EXAMPLE)),
    h('div', { class: 'maint' }, h('h3', null, 'Biblioteki strumieni'),
      table(libs, [['Name', 'Nazwa', { class: 'w' }]], { empty: 'Brak biblioteki strumieni zdalnych – załóż ją w Bibliotekach (rodzaj „strumienie zdalne”).' }),
      h('div', { class: 'bar' }, btn('Biblioteki', () => (location.hash = '#media')), btn('Przeglądarka', () => (location.hash = '#przegladarka'))))),
  cmdNote('GetAllLocalLibraries', 'QueueLibraryScanByLibraryId', 'QuerySearchIndexRemoteStreams')];
}

const TABS = [['Jellyfin', () => keyed('Jellyfin')], ['Emby', () => keyed('Emby')], ['Plex', plex], ['Strumienie zdalne', remote]];

VIEWS.zrodla = async () => {
  const body = await TABS[TAB.i][1]();
  const tabs = h('div', { class: 'tabs page-tabs', role: 'tablist' }, TABS.map(([t], i) =>
    h('button', { type: 'button', role: 'tab', class: i === TAB.i ? 'on' : null, 'aria-selected': String(i === TAB.i), onclick: () => { TAB.i = i; route(); } }, t)));
  return [head('Źródła zewnętrzne', 'Serwery mediów (Jellyfin, Emby, Plex) i strumienie zdalne. Odpowiednik Media Sources w legacy.'), tabs, ...body];
};
