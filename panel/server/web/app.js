// VizRS Live – panel kanału. Wejście UI: start, stan mostu w pasku górnym, router.
// Dane: komendy ErsatzTV legacy przez most (POST /api/cmd/{Nazwa}); moduły:
//   core.js (DOM, API, komunikaty, router, PL) · form.js (formularz z metadanych)
//   parts.js („lista + edytor”, edytor pozycji, wybór treści, grupy, oś doby) · when.js (dni, miesiące, zakres dat)
//   · player.js + mux.js (podgląd strumienia kanału) · views/*.js (ekrany po grupach menu)

import { S, main, h, btn, head, api, cmd, ApiError, route, hms, dialog, operatorName, has } from './core.js';
import { openMonitor } from './player.js';
import { startAlarms } from './alarms.js';
import { desktopBridge } from './desktop.js';
import './views/antena.js';
import './views/kanaly.js';
import './views/profile.js';
import './views/znaki.js';
import './views/media.js';
import './views/zrodla.js';
import './views/listy.js';
import './views/playlisty.js';
import './views/wypelniacze.js';
import './views/bloki.js';
import './views/deco.js';
import './views/playouty.js';
import './views/harmonogramy.js';
import './views/grafika.js';
import './views/live.js';
import './views/system.js';
import './views/asrun.js';

async function refreshStatus() {
  const d = await api('/api/status');
  S.status = d.result;
  const b = S.status.bridge?.result;
  const sb = document.getElementById('st-bridge');
  const sl = document.getElementById('st-legacy');
  sb.textContent = b ? `most ${b.bridge}: OK` : 'most: brak połączenia';
  sb.className = 'pill ' + (b ? 'ok' : 'bad');
  sb.title = S.status.bridgeError || '';
  sl.textContent = b ? `legacy ${b.legacy}` : 'legacy: ?';
  sl.className = 'pill ' + (b ? 'info' : '');
  document.getElementById('logout').hidden = !S.status.auth;
  refreshWarnings();
}

/** Licznik playoutów, których legacy nie zbudował (jak licznik ostrzeżeń w menu legacy). */
async function refreshWarnings() {
  const w = document.getElementById('st-warn');
  if (!S.meta.has('GetPlayoutWarningsCount')) return;
  try {
    const n = await cmd('GetPlayoutWarningsCount');
    w.textContent = `playouty z błędem: ${n}`;
    w.hidden = !n;
  } catch { w.hidden = true; }
}

/** Wskaźnik „studio na antenie” w pasku górnym (VizRS CG Playout), widoczny na każdym ekranie. */
async function refreshStudio() {
  const el = document.getElementById('st-studio');
  if (!S.status?.cg) { el.hidden = true; return; }
  try {
    const st = (await api('/api/cg/live/state')).result;
    const air = (st.channels || []).filter((c) => c.on_air).map((c) => c.number);
    const held = (st.channels || []).filter((c) => c.hold).map((c) => c.number);
    el.textContent = air.length ? `studio na antenie: ${air.join(', ')}` : held.length ? `studio wstrzymane: ${held.join(', ')}` : 'CG Playout: połączony';
    el.className = 'pill ' + (air.length ? 'on' : held.length ? 'warn' : 'ok');
    el.title = air.length || held.length ? 'Kanały ze studiem (VizRS CG Playout) – Sloty na żywo' : 'VizRS CG Playout odpowiada; studio: ramówka (żaden kanał nie nadaje ze studia)';
    el.hidden = false;
  } catch { el.textContent = 'CG Playout: brak połączenia'; el.className = 'pill bad'; el.hidden = false; }
}

// ---------- liczniki w menu (jak w makiecie UI: „Kanały 3”, „Kolekcje 14”) ----------
const len = (cmdName, args) => cmd(cmdName, args).then((r) => (Array.isArray(r) ? r.length : r?.TotalCount ?? r?.Page?.length ?? 0));
/** Widok → [komendy z białej listy, licznik]. Bez którejś komendy licznika nie ma. */
const COUNTS = {
  kanaly: [['GetAllChannels'], () => len('GetAllChannels', { ShowDisabled: true })],
  profile: [['GetAllFFmpegProfiles'], () => len('GetAllFFmpegProfiles')],
  znaki: [['GetAllWatermarks'], () => len('GetAllWatermarks')],
  media: [['GetAllLocalLibraries'], () => len('GetAllLocalLibraries')],
  kolekcje: [['GetAllCollections', 'GetAllSmartCollections', 'GetAllMultiCollections', 'GetPagedRerunCollections'], async () => (await Promise.all([
    len('GetAllCollections'), len('GetAllSmartCollections'), len('GetAllMultiCollections'),
    len('GetPagedRerunCollections', { Query: '', PageNum: 0, PageSize: 1000 })])).reduce((a, b) => a + b, 0)],
  playlisty: [['GetAllPlaylistGroups', 'GetPlaylistsByPlaylistGroupId'], async () => {
    const groups = await cmd('GetAllPlaylistGroups');
    return (await Promise.all(groups.map((g) => len('GetPlaylistsByPlaylistGroupId', { PlaylistGroupId: g.Id })))).reduce((a, b) => a + b, 0);
  }],
  wypelniacze: [['GetAllFillerPresets'], () => len('GetAllFillerPresets')],
  harmonogramy: [['GetAllProgramSchedules'], () => len('GetAllProgramSchedules')],
  bloki: [['GetAllBlocks'], () => len('GetAllBlocks')],
  playouty: [['GetPagedPlayouts'], () => len('GetPagedPlayouts', { Query: '', PageNum: 0, PageSize: 500 })],
};

/** Odświeża licznik jednego widoku w menu (po starcie wszystkie, potem po każdym przerysowaniu tego widoku). */
async function refreshCount(view) {
  const c = COUNTS[view];
  const a = document.querySelector(`.side a[href="#${view}"]`);
  if (!c || !a || !c[0].every(has)) return;
  let n;
  try { n = await c[1](); } catch { return; }
  let el = a.querySelector('.n');
  if (!el) a.append((el = h('span', { class: 'n' })));
  el.textContent = String(n);
  if (view === 'kanaly') refreshChannels();
}

// ---------- wybór kanału w pasku górnym ----------
const CH_KEY = 'vizrs-live-channel';
const store = {
  get: () => { try { return localStorage.getItem(CH_KEY) || ''; } catch { return ''; } },
  set: (v) => { try { localStorage.setItem(CH_KEY, v); } catch { /* bez pamięci w przeglądarce */ } },
};

/** Lista kanałów w wyborze (po starcie i po zmianach na ekranie Kanały); kanał, którego już nie ma = wszystkie. */
async function refreshChannels() {
  const sel = document.getElementById('st-channel');
  if (!has('GetAllChannels')) return;
  let list;
  try { list = await cmd('GetAllChannels', { ShowDisabled: true }); } catch { return; }
  list.sort((a, b) => (parseFloat(a.Number) || 0) - (parseFloat(b.Number) || 0));
  sel.replaceChildren(h('option', { value: '' }, `Wszystkie kanały (${list.length})`),
    ...list.map((c) => h('option', { value: String(c.Number) }, `${c.Number} · ${c.Name}`)));
  const want = S.channel || store.get();
  S.channel = list.some((c) => String(c.Number) === want) ? want : '';
  sel.value = S.channel;
  sel.classList.toggle('on', !!S.channel);
  sel.hidden = false;
}

function channelSelect() {
  const sel = document.getElementById('st-channel');
  S.channel = store.get();
  sel.addEventListener('change', () => {
    S.channel = sel.value;
    store.set(S.channel);
    sel.classList.toggle('on', !!S.channel);
    route(true);
  });
}

function operatorButton() {
  const b = document.getElementById('st-operator');
  const show = () => (b.textContent = `operator: ${operatorName() || '–'}`);
  show();
  b.addEventListener('click', async () => {
    const input = h('input', { id: 'op-name', maxlength: 40, value: operatorName() });
    const body = h('div', { class: 'form' }, h('label', { for: 'op-name' }, 'Stanowisko / operator'), input,
      h('span'), h('span', { class: 'dim' }, 'Trafia do dziennika zdarzeń CG Playout (ZDEJMIJ, WPUŚĆ, audycje). Pamiętane w tej przeglądarce.'));
    if (await dialog('Nazwa stanowiska', body, [['Anuluj', false], ['Zapisz', true, 'acc']])) {
      operatorName(input.value.trim().slice(0, 40));
      show();
    }
  });
}

// ---------- menu: zwijanie do ikon (szeroki ekran, pamiętane) i szuflada (wąski ekran) ----------
const NAV_KEY = 'vizrs-live-nav-min';
function navToggle() {
  const app = document.querySelector('.app');
  const narrow = window.matchMedia('(max-width: 760px)');
  const b = document.getElementById('nav-toggle');
  // podpowiedź z nazwą ekranu – potrzebna, gdy menu pokazuje same ikony
  document.querySelectorAll('.side a').forEach((a) => { a.title ||= a.firstChild?.textContent.trim() || ''; });
  try { app.classList.toggle('nav-min', localStorage.getItem(NAV_KEY) === '1'); } catch { /* bez pamięci w przeglądarce */ }
  const sync = () => b.setAttribute('aria-expanded', String(narrow.matches ? app.classList.contains('nav-open') : !app.classList.contains('nav-min')));
  b.addEventListener('click', (e) => {
    e.stopPropagation();
    if (narrow.matches) app.classList.toggle('nav-open');
    else {
      const min = app.classList.toggle('nav-min');
      try { localStorage.setItem(NAV_KEY, min ? '1' : '0'); } catch { /* bez pamięci w przeglądarce */ }
    }
    sync();
  });
  const close = () => { if (app.classList.contains('nav-open')) { app.classList.remove('nav-open'); sync(); } };
  window.addEventListener('hashchange', close);
  document.addEventListener('click', (e) => { if (!e.target.closest('.side')) close(); });
  document.addEventListener('keydown', (e) => { if (e.key === 'Escape') close(); });
  narrow.addEventListener('change', () => { app.classList.remove('nav-open'); sync(); });
  sync();
}

function clock() {
  const c = document.getElementById('clock');
  const tick = () => (c.textContent = hms(new Date()));
  tick();
  setInterval(tick, 1000);
}

async function start() {
  clock();
  navToggle();
  operatorButton();
  channelSelect();
  document.getElementById('st-monitor').addEventListener('click', () => openMonitor());
  document.getElementById('logout').addEventListener('click', async () => { await api('/api/logout', { method: 'POST', body: '' }); location.reload(); });
  try {
    await refreshStatus();
    if (S.status.bridge) {
      const c = await api('/api/commands');
      S.meta = new Map(c.result.map((x) => [x.name, x]));
      refreshWarnings();
    }
  } catch (e) {
    if (!(e instanceof ApiError)) throw e;
  }
  setInterval(() => refreshStatus().catch(() => {}), 15000);
  window.addEventListener('hashchange', () => route());
  if (!S.status?.bridge) {
    main.replaceChildren(head('Brak połączenia z mostem'), h('div', { class: 'note err' }, S.status?.bridgeError || 'Serwer panelu nie odpowiada.'),
      h('div', { class: 'note' }, 'Sprawdź, czy ErsatzTV legacy działa z mostem (obraz vizrs-live-legacy) i czy VIZRS_LIVE_BRIDGE_TOKEN = VIZRS_BRIDGE_TOKEN.'),
      btn('Spróbuj ponownie', () => location.reload()));
    return;
  }
  // ekran przerysowany (wejście, zapis, odświeżenie) → jego licznik w menu
  new MutationObserver(() => refreshCount(main.dataset.view)).observe(main, { attributes: true, attributeFilter: ['data-render'] });
  await refreshChannels();
  route();
  Object.keys(COUNTS).forEach(refreshCount);
  refreshStudio();
  setInterval(refreshStudio, 5000);
  startAlarms();
}

start();
desktopBridge();
