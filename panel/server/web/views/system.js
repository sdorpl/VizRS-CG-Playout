// Widoki grupy „System” (R7): Ustawienia (panel + ustawienia legacy w sekcjach), Logi, Diagnostyka
// (kontrole, informacje o sprzęcie i wersjach, konserwacja, test odtwarzania, konsola komend).

import { S, VIEWS, api, h, btn, pill, head, panel, cmdNote, table, cmd, pl, toast, confirmDlg, route, hms, pad2, missingNote, has } from '../core.js';
import { form } from '../form.js';
import { contentPicker } from '../parts.js';
import { openMonitor } from '../player.js';
import { cg } from './grafika.js';
import { accels, accelCodecs } from './profile.js';
import { followTask } from '../tasks.js';

// ---------- Ustawienia ----------

const LEVELS = ['Verbose', 'Debug', 'Information', 'Warning', 'Error', 'Fatal'];
const LEVEL_PL = { Verbose: 'szczegółowy (Verbose)', Debug: 'diagnostyczny (Debug)', Information: 'informacje', Warning: 'ostrzeżenia', Error: 'błędy', Fatal: 'krytyczne' };
const levelOpts = LEVELS.map((l) => [l, LEVEL_PL[l]]);
const num = (key, label, min, hint, extra = {}) => ({ key, label, kind: 'number', optional: false, attrs: { min }, hint, ...extra });
/** Legacy (26.11.6) nie sprawdza zakresów w komendach Update…Settings – robi to tylko web GUI. Panel sprawdza sam (DOKUMENTACJA §5). */
function atLeast(v, key, min, label) {
  if (v[key] < min) throw new Error(`Pole „${label}”: co najmniej ${min}.`);
}

/**
 * Sekcja ustawień legacy: load() → {values, ctx}; fields(ctx) – opis pól form.js; save(values) – komenda Update…
 * Klucze pól to właściwości obiektu sekcji (np. XmltvSettings.DaysToBuild), więc rodzaj kontrolki podaje opis pola.
 */
const SECTIONS = [
  {
    id: 'ffmpeg', title: 'FFmpeg i strumienie', needs: ['GetFFmpegSettings', 'UpdateFFmpegSettings', 'GetAllFFmpegProfiles', 'GetAllWatermarks', 'GetAllFillerPresets', 'GetMpegTsScripts'],
    desc: 'Ścieżki ffmpeg, domyślny profil nowych kanałów, napisy, globalny znak wodny i wypełniacz zapasowy, segmenter HLS. Odpowiednik Settings → FFmpeg w legacy.',
    load: async () => {
      const [values, profiles, marks, fillers, scripts] = await Promise.all([
        cmd('GetFFmpegSettings'), cmd('GetAllFFmpegProfiles'), cmd('GetAllWatermarks'), cmd('GetAllFillerPresets'), cmd('GetMpegTsScripts')]);
      return { values, ctx: { profiles, marks, fillers: fillers.filter((f) => f.FillerKind === 'Fallback'), scripts } };
    },
    fields: ({ profiles, marks, fillers, scripts }) => [
      { sec: 'Programy' },
      { key: 'FFmpegPath', label: 'ffmpeg', required: true, hint: 'Ścieżka w kontenerze legacy (w obrazie: /usr/local/bin/ffmpeg).' },
      { key: 'FFprobePath', label: 'ffprobe', required: true },
      { sec: 'Kanały' },
      { key: 'DefaultFFmpegProfileId', label: 'Profil nowych kanałów', optional: false, options: profiles.map((p) => [p.Id, p.Name]) },
      { key: 'PreferredAudioLanguageCode', label: 'Preferowany język dźwięku', placeholder: 'np. pol', hint: 'Kod ISO 639-2 (trzy litery): pol, eng, deu…' },
      { key: 'GlobalWatermarkId', label: 'Znak wodny globalny', none: '(brak)', options: marks.map((w) => [w.Id, w.Name]), hint: 'Dla kanałów bez własnego znaku wodnego.' },
      { key: 'GlobalFallbackFillerId', label: 'Wypełniacz zapasowy', none: '(brak)', options: fillers.map((f) => [f.Id, f.Name]), hint: fillers.length ? 'Gdy kanał nie ma czego nadawać.' : 'Brak wypełniaczy rodzaju „zapasowy” (Listy → Wypełniacze).' },
      { sec: 'Napisy i analiza' },
      { label: 'Opcje', group: [
        { key: 'UseEmbeddedSubtitles', label: 'napisy wbudowane', kind: 'check', text: 'używaj napisów wbudowanych w plik' },
        { key: 'ExtractEmbeddedSubtitles', label: 'wyodrębniaj', kind: 'check', text: 'wyodrębniaj napisy tekstowe' },
        { key: 'ProbeForInterlacedFrames', label: 'przeplot', kind: 'check', text: 'wykrywaj przeplot (wolniejszy skan)' },
        { key: 'SaveReports', label: 'raporty', kind: 'check', text: 'zapisuj raporty ffmpeg (diagnostyka)' },
      ] },
      { sec: 'Segmenter HLS' },
      num('HlsSegmenterIdleTimeout', 'Wyłącz bezczynny po', 30, 'Sekundy bez widza, po których segmenter staje (min. 30).', { unit: 's' }),
      num('WorkAheadSegmenterLimit', 'Segmentery z wyprzedzeniem', 0, 'Ile kanałów naraz może kodować szybciej niż w czasie rzeczywistym (0 = żaden).'),
      num('InitialSegmentCount', 'Segmenty na start', 1, 'Ile segmentów musi być gotowych, zanim widz dostanie playlistę (min. 1).'),
      { key: 'HlsDirectOutputFormat', label: 'Format HLS Direct', optional: false, options: [['MpegTs', 'MPEG-TS'], ['Mp4', 'MP4'], ['Mkv', 'MKV']] },
      { key: 'DefaultMpegTsScript', label: 'Skrypt MPEG-TS', optional: false, options: scripts.map((s) => [s.Id, s.Name]) },
    ],
    check: (v) => {
      atLeast(v, 'HlsSegmenterIdleTimeout', 30, 'Wyłącz bezczynny po');
      atLeast(v, 'WorkAheadSegmenterLimit', 0, 'Segmentery z wyprzedzeniem');
      atLeast(v, 'InitialSegmentCount', 1, 'Segmenty na start');
      if (v.PreferredAudioLanguageCode && !/^[a-z]{3}$/i.test(v.PreferredAudioLanguageCode)) throw new Error('Pole „Preferowany język dźwięku”: trzy litery (ISO 639-2), np. pol.');
    },
    save: (v) => cmd('UpdateFFmpegSettings', { Settings: v }),
    note: ['GetFFmpegSettings', 'UpdateFFmpegSettings'],
  },
  {
    id: 'playout', title: 'Playouty', needs: ['GetPlayoutSettings', 'UpdatePlayoutSettings'],
    desc: 'Na ile dni do przodu legacy buduje playouty. Odpowiednik Settings → Playout.',
    load: async () => ({ values: await cmd('GetPlayoutSettings'), ctx: {} }),
    fields: () => [
      num('DaysToBuild', 'Dni do zbudowania', 1, 'Playouty są budowane na tyle dni naprzód (min. 1).'),
      { key: 'SkipMissingItems', label: 'Brakujące pliki', kind: 'check', text: 'pomijaj pozycje bez pliku (niedostępne)' },
      num('ScriptedScheduleTimeoutSeconds', 'Limit czasu skryptu', 10, 'Playouty skryptowe: maksymalny czas działania skryptu (min. 10 s).', { unit: 's' }),
    ],
    check: (v) => { atLeast(v, 'DaysToBuild', 1, 'Dni do zbudowania'); atLeast(v, 'ScriptedScheduleTimeoutSeconds', 10, 'Limit czasu skryptu'); },
    save: (v) => cmd('UpdatePlayoutSettings', { PlayoutSettings: v }),
    note: ['GetPlayoutSettings', 'UpdatePlayoutSettings'],
  },
  {
    id: 'xmltv', title: 'Przewodnik XMLTV', needs: ['GetXmltvSettings', 'UpdateXmltvSettings'],
    desc: 'Przewodnik programowy dla odbiorników (xmltv.xml). Odpowiednik Settings → XMLTV.',
    load: async () => ({ values: await cmd('GetXmltvSettings'), ctx: {} }),
    fields: () => [
      num('DaysToBuild', 'Dni w przewodniku', 1, 'Nie więcej niż „Dni do zbudowania” playoutów – legacy odrzuci dłuższy przewodnik.'),
      { key: 'TimeZone', label: 'Strefa czasowa', optional: false, options: [['Local', 'lokalna (serwera legacy)'], ['Utc', 'UTC']] },
      { key: 'BlockBehavior', label: 'Bloki w przewodniku', optional: false, options: [['SplitTimeEvenly', 'dziel czas bloku po równo'], ['UseActualTimes', 'prawdziwe godziny pozycji']] },
    ],
    check: (v) => atLeast(v, 'DaysToBuild', 1, 'Dni w przewodniku'),
    save: (v) => cmd('UpdateXmltvSettings', { XmltvSettings: v }),
    note: ['GetXmltvSettings', 'UpdateXmltvSettings'],
  },
  {
    id: 'skaner', title: 'Skaner i HDHomeRun', needs: ['GetLibraryRefreshInterval', 'UpdateLibraryRefreshInterval', 'GetHDHRTunerCount', 'UpdateHDHRTunerCount', 'GetHDHRUUID'],
    desc: 'Co ile godzin legacy sam skanuje biblioteki; emulacja tunera HDHomeRun (Plex, Jellyfin, Emby jako odbiorniki).',
    load: async () => {
      const [LibraryRefreshInterval, TunerCount, Uuid] = await Promise.all([cmd('GetLibraryRefreshInterval'), cmd('GetHDHRTunerCount'), cmd('GetHDHRUUID')]);
      return { values: { LibraryRefreshInterval, TunerCount, Uuid }, ctx: {} };
    },
    fields: () => [
      { sec: 'Skaner bibliotek' },
      num('LibraryRefreshInterval', 'Skanuj co', 0, '0 = bez automatycznego skanu (tylko „Skanuj” w Media → Biblioteki).', { unit: 'godz.' }),
      { sec: 'HDHomeRun' },
      num('TunerCount', 'Liczba tunerów', 1, 'Ile kanałów naraz może oglądać odbiornik HDHomeRun.'),
      { key: 'Uuid', label: 'UUID urządzenia', attrs: { readonly: true }, hint: 'Nadany przez legacy, tylko do odczytu.' },
    ],
    check: (v) => {
      if (v.LibraryRefreshInterval < 0 || v.LibraryRefreshInterval >= 1000000) throw new Error('Pole „Skanuj co”: od 0 do 999999 godzin.');
      atLeast(v, 'TunerCount', 1, 'Liczba tunerów');
    },
    save: async (v, old) => {
      if (v.LibraryRefreshInterval !== old.LibraryRefreshInterval) await cmd('UpdateLibraryRefreshInterval', { LibraryRefreshInterval: v.LibraryRefreshInterval });
      if (v.TunerCount !== old.TunerCount) await cmd('UpdateHDHRTunerCount', { TunerCount: v.TunerCount });
    },
    note: ['UpdateLibraryRefreshInterval', 'UpdateHDHRTunerCount', 'GetHDHRUUID'],
  },
  {
    id: 'logi', title: 'Poziomy logów', needs: ['GetLoggingSettings', 'UpdateLoggingSettings'],
    desc: 'Najniższy poziom zapisywany do logu legacy, osobno dla obszarów. Zmiana działa od razu (System → Logi).',
    load: async () => ({ values: await cmd('GetLoggingSettings'), ctx: {} }),
    fields: () => [
      ['DefaultMinimumLogLevel', 'Domyślny'], ['ScanningMinimumLogLevel', 'Skanowanie'], ['SchedulingMinimumLogLevel', 'Układanie ramówki'],
      ['SearchingMinimumLogLevel', 'Wyszukiwanie'], ['StreamingMinimumLogLevel', 'Nadawanie'], ['HttpMinimumLogLevel', 'HTTP'],
    ].map(([key, label]) => ({ key, label, optional: false, options: levelOpts })),
    save: (v) => cmd('UpdateLoggingSettings', { LoggingSettings: v }),
    note: ['GetLoggingSettings', 'UpdateLoggingSettings'],
  },
  {
    id: 'gui', title: 'Web GUI legacy', needs: ['GetUiSettings', 'UpdateUiSettings'],
    desc: 'Wygląd web GUI legacy – dotyczy tylko administratora, operatorzy pracują w panelu (web GUI legacy zamyka R10).',
    load: async () => ({ values: await cmd('GetUiSettings'), ctx: {} }),
    fields: () => [
      { key: 'IsDarkMode', label: 'Motyw', kind: 'pick', optional: false, options: [[1, 'ciemny'], [0, 'jasny']] },
      { key: 'Language', label: 'Język GUI legacy', placeholder: 'np. en', hint: 'Kod kultury .NET (en, pl, de…); legacy pokazuje tylko języki, które ma przetłumaczone.' },
    ],
    // wybór tak/nie jako lista: form.js trzyma wartość liczbową, legacy chce bool
    wrap: (v) => ({ ...v, IsDarkMode: v.IsDarkMode ? 1 : 0 }),
    save: (v) => cmd('UpdateUiSettings', { UiSettings: { ...v, IsDarkMode: !!v.IsDarkMode } }),
    note: ['GetUiSettings', 'UpdateUiSettings'],
  },
];


function panelSection() {
  const st = S.status;
  const b = st.bridge?.result;
  return h('div', { class: 'cols' },
    panel('Panel', null, table([
      { k: 'Wersja panelu', v: st.panel }, { k: 'Most (legacy)', v: st.bridgeUrl }, { k: 'Strumienie dla widzów', v: st.streamUrl || '— (VIZRS_LIVE_STREAM_URL)' },
      { k: 'VizRS CG System', v: st.vizrsUrl || '— (VIZRS_LIVE_VIZRS_URL)' },
      { k: 'VizRS: token wyjścia do podglądu', v: st.vizrsOutputToken ? 'tak' : 'nie (VIZRS_LIVE_VIZRS_OUTPUT_TOKEN)' },
      { k: 'VizRS: anteny kanałów', v: st.vizrsChannels ? 'tak (bus=ch-N, Trio ?channel=N)' : 'nie (VIZRS_LIVE_VIZRS_CHANNELS)' }, { k: 'Token panelu', v: st.auth ? 'tak' : 'nie (tylko lokalnie)' },
    ], [['k', 'Ustawienie'], ['v', 'Wartość', { class: 'w' }]]),
    h('div', { class: 'cmd' }, 'Opcje panelu ustawia się przy starcie (opcje wiersza poleceń albo zmienne VIZRS_LIVE_*), nie tutaj.')),
    panel('Most', null, b ? table([
      { k: 'Wersja mostu', v: b.bridge }, { k: 'Wersja legacy', v: b.legacy }, { k: 'Komend znanych w legacy', v: b.commandsKnown },
      { k: 'Dozwolonych (biała lista)', v: b.commandsAllowed }, { k: 'Z listy, ale nieznanych w tej wersji', v: b.commandsMissing?.join(', ') || 'brak' },
    ], [['k', 'Pole'], ['v', 'Wartość', { class: 'w' }]]) : h('div', { class: 'empty err' }, st.bridgeError || 'most nieosiągalny')));
}

async function legacySection(sec) {
  const miss = sec.needs.filter((n) => !has(n));
  if (miss.length) return missingNote(miss);
  const { values, ctx } = await sec.load();
  const shown = sec.wrap ? sec.wrap(values) : values;
  const f = form('', sec.fields(ctx), shown, { idPrefix: `s-${sec.id}-` });
  const save = async () => {
    let v;
    try { v = f.read(); if (sec.check) sec.check(v); } catch (e) { return toast(e.message, 'bad'); }
    try { await sec.save(v, shown); } catch (e) { return toast('Legacy odrzucił zapis: ' + e.message, 'bad'); }
    toast(`${sec.title}: zapisano w legacy.`, 'ok');
    route(true);
  };
  const p = panel(sec.title, btn('Zapisz', save, 'acc'), h('p', { class: 'dim pad' }, sec.desc), f.el, cmdNote(...sec.note));
  p.dataset.set = sec.id;
  return p;
}

VIEWS.ustawienia = async () => {
  // jak w makiecie: sekcje legacy jako panele obok siebie, każda z własnym „Zapisz”
  const secs = await Promise.all(SECTIONS.map((sec) => legacySection(sec).catch((e) => panel(sec.title, null, h('div', { class: 'empty err' }, 'Legacy: ' + e.message)))));
  return [
    head('Ustawienia', 'Ustawienia legacy w sekcjach jak w Settings legacy oraz konfiguracja panelu. Każda sekcja zapisuje się osobno („Zapisz” w jej nagłówku).'),
    h('div', { class: 'cols' }, ...secs),
    panelSection(),
  ];
};

// ---------- Logi ----------

const LOG = { level: 'Information', text: '', page: 0, auto: false, src: 'legacy' };
const LOG_PAGE = 200;
const logTime = (t) => { const d = new Date(t); return `${pad2(d.getDate())}.${pad2(d.getMonth() + 1)} ${hms(d)}`; };
const levelKind = { Warning: 'warn', Error: 'bad', Fatal: 'bad', Information: 'info' };

/** Zdarzenia VizRS CG Playout (dziennik operatora z /api/live/state) – CG nie wystawia pliku logu przez API. */
async function cgLog(srcSel) {
  const head_ = head('Logi', 'Dziennik zdarzeń VizRS CG Playout: przejęcia kanału, ZDEJMIJ / WPUŚĆ, przebudowy ramówki, audycje LIVE (od startu CG Playout).', btn('Odśwież', () => route(true)));
  let st;
  try { st = await cg('live/state'); } catch (e) { return [head_, h('div', { class: 'bar pad logbar' }, srcSel), h('div', { class: 'note err' }, 'VizRS CG Playout: ' + e.message)]; }
  const q = h('input', { id: 'log-q', type: 'search', placeholder: 'szukaj w treści…', value: LOG.text });
  q.addEventListener('keydown', (e) => { if (e.key === 'Enter') { LOG.text = q.value.trim(); route(); } });
  const t = LOG.text.toLowerCase();
  const rows = (st.events || []).filter((e) => !t || `${e.text} ${e.channel ?? ''}`.toLowerCase().includes(t));
  return [head_, panel('Wpisy', null,
    h('div', { class: 'bar pad logbar' }, srcSel, q, btn('Szukaj', () => { LOG.text = q.value.trim(); route(); })),
    h('div', { class: 'pad dim', id: 'log-count' }, `Zdarzeń: ${rows.length}${t ? ` (z ${(st.events || []).length})` : ''}.`),
    rows.length ? table(rows, [[(e) => logTime(e.at), 'Czas'], [(e) => (e.channel ? pill(`kanał ${e.channel}`) : '—'), 'Kanał'], [(e) => e.text, 'Wpis', { class: 'w log' }]])
      : h('div', { class: 'empty' }, 'Brak zdarzeń.'),
    cmdNote('CG Playout GET /api/live/state (events)'))];
}

VIEWS.logi = async () => {
  const srcSel = h('select', { id: 'log-src', 'aria-label': 'Źródło logów', onchange: () => { LOG.src = srcSel.value; LOG.page = 0; LOG.text = ''; route(true); } },
    h('option', { value: 'legacy' }, 'źródło: ErsatzTV legacy'), S.status.cg ? h('option', { value: 'cg' }, 'źródło: VizRS CG Playout') : null);
  if (!S.status.cg) LOG.src = 'legacy';
  srcSel.value = LOG.src;
  if (LOG.src === 'cg') return cgLog(srcSel);
  if (!has('GetRecentLogEntries')) return [head('Logi'), missingNote(['GetRecentLogEntries'])];
  const min = LEVELS.indexOf(LOG.level);
  // legacy filtruje tylko tekstem (poziom albo treść); próg poziomu panel nakłada na pobraną stronę
  const d = await cmd('GetRecentLogEntries', { PageNum: LOG.page, PageSize: LOG_PAGE, Filter: LOG.text });
  const rows = d.Page.filter((e) => LEVELS.indexOf(e.Level) >= min);
  const pages = Math.max(1, Math.ceil(d.TotalCount / LOG_PAGE));

  const lvl = h('select', { id: 'log-level', onchange: () => { LOG.level = lvl.value; route(true); } },
    LEVELS.map((l) => h('option', { value: l }, `od: ${LEVEL_PL[l]}`)));
  lvl.value = LOG.level;
  const q = h('input', { id: 'log-q', type: 'search', placeholder: 'szukaj w treści…', value: LOG.text });
  q.addEventListener('keydown', (e) => { if (e.key === 'Enter') { LOG.text = q.value.trim(); LOG.page = 0; route(); } });
  const auto = h('input', { type: 'checkbox', id: 'log-auto', checked: LOG.auto, onchange: () => { LOG.auto = auto.checked; route(true); } });
  if (LOG.auto && LOG.page === 0) S.timers.push(setInterval(() => route(true), 5000));
  const go = (p) => { LOG.page = p; route(); };

  return [
    head('Logi', 'Najnowsze wpisy logu legacy (od najnowszych). Poziomy zapisu: Ustawienia → Poziomy logów.',
      btn('Odśwież', () => route(true))),
    panel('Wpisy', null,
    h('div', { class: 'bar pad logbar' }, srcSel, lvl, q, btn('Szukaj', () => { LOG.text = q.value.trim(); LOG.page = 0; route(); }),
      h('label', { class: 'chk' }, auto, 'odświeżaj co 5 s')),
    h('div', { class: 'pad dim', id: 'log-count' }, `Pasujących wpisów: ${d.TotalCount}; na tej stronie (${LOG_PAGE}) od poziomu „${LEVEL_PL[LOG.level]}”: ${rows.length}.`),
    rows.length ? table(rows, [[(e) => logTime(e.Timestamp), 'Czas'], [(e) => pill(LEVEL_PL[e.Level] || e.Level, levelKind[e.Level]), 'Poziom'], [(e) => e.Message.trim(), 'Wpis', { class: 'w log' }]])
      : h('div', { class: 'empty' }, 'Brak wpisów.'),
    h('div', { class: 'bar pad' },
      h('button', { type: 'button', disabled: LOG.page === 0, onclick: () => go(LOG.page - 1) }, '← nowsze'),
      h('span', { class: 'dim' }, `strona ${LOG.page + 1} z ${pages}`),
      h('button', { type: 'button', disabled: LOG.page + 1 >= pages, onclick: () => go(LOG.page + 1) }, 'starsze →')),
    cmdNote('GetRecentLogEntries')),
  ];
};

// ---------- Diagnostyka ----------

/** Zadania konserwacji: [komenda, argumenty, etykieta, opis do potwierdzenia]. */
const MAINT = [
  ['EmptyTrash', {}, 'Opróżnij kosz', 'Usunąć z bazy legacy wszystkie pozycje z kosza (pliki, których już nie ma na dysku)? Tego nie da się cofnąć.'],
  ['DeleteOrphanedArtwork', { MaxToDelete: null }, 'Usuń osierocone grafiki', 'Usunąć z pamięci podręcznej legacy plakaty i miniatury, których nie używa żadna pozycja?'],
  ['DeleteOrphanedSubtitles', {}, 'Usuń osierocone napisy', 'Usunąć z bazy legacy napisy, które nie należą do żadnej pozycji?'],
  ['ReleaseMemory', { ForceAggressive: true }, 'Zwolnij pamięć', 'Wymusić odśmiecanie pamięci legacy? Nadawanie może na chwilę zwolnić.'],
];

/** K6: wzorce wpisów legacy dla zadań konserwacji (większość na poziomie Debug – widać je przy takim progu logów). */
const MAINT_LOG = {
  EmptyTrash: /trash|deleted .*items?/i, DeleteOrphanedArtwork: /orphaned (artwork|actors)|artwork file|cache folder/i,
  DeleteOrphanedSubtitles: /subtitle/i, ReleaseMemory: /garbage collection/i,
};

async function maintenance(name, args, label, text) {
  if (!(await confirmDlg(label, text, label))) return;
  let r;
  try { r = await cmd(name, args); } catch (e) { toast(`${label}: legacy odrzucił – ${e.message}`, 'bad'); return; }
  if (!r?.queued) { toast(`${label}: zrobione.`, 'ok'); return; }
  await followTask({ label, match: MAINT_LOG[name], timeoutMs: 20000,
    empty: 'Legacy nie zapisał wpisu o tym zadaniu na bieżącym progu logów (wpisy konserwacji są na poziomie Debug – Ustawienia → Poziomy logów). Zadanie poszło w tle.' });
}

function playbackTest(channels) {
  const base = S.status.streamUrl;
  if (!channels.length) return h('div', { class: 'empty' }, 'Brak kanałów.');
  const sel = h('select', { id: 'pt-ch' }, channels.map((c) => h('option', { value: c.Number }, `${c.Number} · ${c.Name}`)));
  const links = h('div', { class: 'bar' });
  const draw = () => {
    const n = encodeURIComponent(sel.value);
    links.replaceChildren(
      btn('Odtwórz w panelu ▶', () => openMonitor(sel.value), 'acc'),
      base ? h('a', { href: `${base}/iptv/channel/${n}.m3u8`, target: '_blank', rel: 'noopener', id: 'pt-hls' }, 'HLS (.m3u8) ↗') : null,
      base ? h('a', { href: `${base}/iptv/channel/${n}.ts`, target: '_blank', rel: 'noopener' }, 'MPEG-TS (.ts) ↗') : null);
  };
  sel.addEventListener('change', draw);
  draw();
  return h('div', { class: 'form' }, h('label', { for: 'pt-ch' }, 'Kanał'), sel, h('label', null, 'Strumień'), links,
    h('div', { class: 'hint' }, '„Odtwórz w panelu” otwiera okno podglądu (ten sam strumień, co u widzów, równany do zegara anteny). ',
      base ? 'Odnośniki – do VLC/mpv. ' : 'Odnośniki do VLC/mpv – po ustawieniu --stream-url (VIZRS_LIVE_STREAM_URL). ',
      'Szczegółowy test pliku z wyborem profilu legacy robi w Blazorze – panel go nie powtarza (DOKUMENTACJA §5).'));
}

// ---------- Zamknięcie web GUI legacy (R10) ----------

/** Lista kontrolna pokrycia: menu web GUI legacy (Shared/MainLayout.razor, v26.11–26.12) → ekran panelu albo świadome pominięcie. */
export const COVERAGE = [
  ['Channels', 'kanaly'], ['FFmpeg Profiles', 'profile'], ['Watermarks', 'znaki'],
  ['Media Sources → Local', 'media'], ['Media Sources → Emby / Jellyfin / Plex', 'zrodla'],
  ['Media → Libraries', 'media'], ['Media → Trash', 'kosz'],
  ['Media → TV Shows / Movies / Music / Other Videos / Songs / Images / Remote Streams', 'przegladarka'],
  ['Lists → Collections / Smart / Multi / Rerun', 'kolekcje'], ['Lists → Playlists', 'playlisty'],
  ['Lists → Trakt Lists', null, 'pominięte: wymaga klucza API Trakt; listy Trakt założone wcześniej są widoczne jako playlisty systemowe (tylko do odczytu)'],
  ['Lists → Filler Presets', 'wypelniacze'],
  ['Scheduling → Schedules', 'harmonogramy'], ['Scheduling → Blocks / Templates', 'bloki'],
  ['Scheduling → Decos / Deco Templates', 'deco'], ['Scheduling → Playouts', 'playouty'],
  ['Settings → FFmpeg / Logging / HDHomeRun / Scanner / Playout / XMLTV', 'ustawienia'],
  ['Settings → UI', 'ustawienia', 'tylko wygląd web GUI legacy – bez znaczenia po jego zamknięciu'],
  ['Support → Health Checks', 'diagnostyka'], ['Support → Logs', 'logi'],
  ['Support → Troubleshooting', 'diagnostyka', 'częściowo: informacje o sprzęcie i odnośniki do strumienia; test pliku w odtwarzaczu Blazora – nie (DOKUMENTACJA §5)'],
  ['Pasek: M3U / XMLTV', 'kanaly', 'odnośniki channels.m3u i xmltv.xml (Kanały, Przewodnik EPG)'],
  ['Pasek: Docs, pomoc, GitHub', null, 'pominięte: odnośniki do stron projektu ErsatzTV, nie funkcje kanału'],
];

const SCREEN_PL = () => Object.fromEntries([...document.querySelectorAll('nav a[href^="#"]')].map((a) => [a.getAttribute('href').slice(1), a.textContent]));

/** Ocena sprawdzenia adresu dla widzów: web GUI i most mają być niedostępne, M3U dostępne. */
export function exposureVerdict(x) {
  if (!x || x.error) return { cls: 'info', text: `nie sprawdzono (${x?.error || 'brak danych'})` };
  const st = Object.fromEntries(x.probes.map((p) => [p.path, p.status]));
  if (Object.values(st).every((v) => v == null)) return { cls: 'info', text: 'adres dla widzów nieosiągalny z serwera panelu – sprawdź z zewnątrz' };
  const open = (v) => v != null && v >= 200 && v < 400;
  if (open(st['/']) || open(st['/channels']) || open(st['/vizrs-bridge/health']) || st['/vizrs-bridge/health'] === 401)
    return { cls: 'bad', text: 'web GUI legacy albo most widoczne dla widzów – wystaw legacy przez proxy (compose.yml, docker/proxy.Caddyfile)' };
  if (!open(st['/iptv/channels.m3u'])) return { cls: 'warn', text: 'web GUI zamknięte, ale M3U niedostępne – widzowie nie dostaną listy kanałów' };
  return { cls: 'ok', text: 'zamknięte: na zewnątrz tylko strumienie, M3U/XMLTV i HDHomeRun' };
}

// ---------- K5: silnik Next (legacy ≥ 26.12.0; starsze – brak komend = brak karty) ----------
const sv = (o, snake, pascal) => (o == null ? undefined : o[snake] ?? o[pascal]);
const ACCEL_ST = { available: ['działa', 'ok'], ffmpeg_missing: ['brak w ffmpeg', 'bad'], ffmpegmissing: ['brak w ffmpeg', 'bad'], probe_failed: ['test nieudany', 'bad'], probefailed: ['test nieudany', 'bad'], skipped: ['pominięta', ''], timeout: ['przekroczony czas', 'bad'] };
const NEXT_CMDS = ['GetNextVersion', 'GetNextDescribe', 'GetNextProbe'];

async function loadNext() {
  if (!NEXT_CMDS.some(has)) return null;
  const [version, describe, probe] = await Promise.all(NEXT_CMDS.map((n) => (has(n) ? cmd(n).catch((e) => ({ error: e.message })) : null)));
  return { version, describe, probe };
}

function nextPanel(n) {
  if (!n) return null;
  const refresh = has('RefreshNextProbe') ? btn('Sprawdź ponownie', async () => {
    toast('Silnik Next sprawdza sprzęt…');
    try { await cmd('RefreshNextProbe'); toast('Sprzęt sprawdzony ponownie.', 'ok'); route(true); } catch (e) { toast('Legacy: ' + e.message, 'bad'); }
  }) : null;
  const d = n.describe && !n.describe.error ? n.describe : null;
  const c = sv(d, 'contract', 'Contract');
  const schema = (x) => (x ? `${sv(x, 'breaking', 'Breaking')}.${sv(x, 'compatible', 'Compatible')}` : '—');
  const sc = sv(d, 'schema_capability', 'SchemaCapability');
  const rows = [
    { k: 'Wersja', v: typeof n.version === 'string' ? n.version || '—' : n.version?.error ? 'błąd: ' + n.version.error : sv(sv(d, 'binary', 'Binary'), 'version', 'Version') || '—' },
    c ? { k: 'Schematy (playout / kanał)', v: `${schema(sv(c, 'playout_schema', 'PlayoutSchema'))} / ${schema(sv(c, 'channel_config_schema', 'ChannelConfigSchema'))} · segment ${sv(c, 'segment_seconds', 'SegmentSeconds')} s, klatka kluczowa co ${sv(c, 'keyframe_seconds', 'KeyframeSeconds')} s` } : null,
    sc ? { k: 'Obsługiwane', v: `obraz: ${(sv(sc, 'video_formats', 'VideoFormats') || []).join(', ')} · dźwięk: ${(sv(sc, 'audio_formats', 'AudioFormats') || []).join(', ')} · akceleracje: ${(sv(sc, 'accels', 'Accels') || []).join(', ')}` } : null,
    n.describe?.error ? { k: 'Opis Next', v: 'błąd: ' + n.describe.error } : null,
  ].filter(Boolean);
  const p = n.probe && !n.probe.error ? n.probe : null;
  const list = accels(p);
  const probed = p ? sv(p, 'ProbedAt', 'probed_at') : null;
  const ff = sv(sv(p, 'Capability', 'capability'), 'ffmpeg', 'Ffmpeg');
  if (p) rows.push({ k: 'Sprzęt sprawdzony', v: `${probed ? new Date(probed).toLocaleString('pl-PL') : '—'}${ff ? ` · ffmpeg ${sv(ff, 'version', 'Version') || '?'} (${sv(ff, 'path', 'Path') || '?'})` : ''}` });
  else rows.push({ k: 'Sprzęt', v: n.probe?.error ? 'błąd: ' + n.probe.error : 'Next jeszcze nie sprawdził sprzętu – „Sprawdź ponownie”' });
  return panel('Silnik Next', refresh,
    h('div', { 'data-next': '' }, table(rows, [['k', 'Pole'], ['v', 'Wartość', { class: 'w' }]])),
    list.length ? table(list, [
      [(a) => { const st = String(sv(a, 'status', 'Status') || '').toLowerCase(); const [t, k] = ACCEL_ST[st] || [st || '?', '']; return pill(t, k); }, 'Stan'],
      [(a) => [sv(a, 'accel', 'Accel'), sv(a, 'device', 'Device'), sv(a, 'driver', 'Driver')].filter(Boolean).join(' · '), 'Akceleracja'],
      [(a) => accelCodecs(a).dec.join(', ') || '—', 'Dekodowanie'],
      [(a) => accelCodecs(a).enc.join(', ') || '—', 'Kodowanie'],
      [(a) => sv(a, 'error', 'Error') || '', 'Błąd', { class: 'w' }],
    ]) : null,
    cmdNote(...NEXT_CMDS.filter(has), ...(has('RefreshNextProbe') ? ['RefreshNextProbe'] : [])));
}

function exposurePanel(x) {
  const v = exposureVerdict(x);
  const names = SCREEN_PL();
  return [
    panel('Web GUI legacy dla widzów', btn('Sprawdź ponownie', () => route(true)),
      h('div', { class: 'pad', id: 'exp-verdict' }, pill(v.text, v.cls)),
      x?.probes?.length ? table(x.probes, [['path', 'Ścieżka'], [(p) => (p.status == null ? '—' : String(p.status)), 'Kod HTTP', { class: 'num' }],
        [(p) => (p.path.startsWith('/iptv/') ? 'ma być otwarte' : 'ma być zamknięte (403/404)'), 'Oczekiwane', { class: 'w' }]]) : null,
      h('div', { class: 'cmd' }, `Adres dla widzów: ${x?.url || '—'} (sprawdza serwer panelu ze swojej sieci). Operatorzy pracują tylko w panelu; legacy ma na zewnątrz tylko /iptv/*, /artwork/* i HDHomeRun (DOKUMENTACJA §8).`)),
    panel('Pokrycie web GUI legacy', null,
      table(COVERAGE.map(([legacy, screen, note]) => ({ legacy, screen, note })), [['legacy', 'Ekran legacy'],
        [(r) => (r.screen ? h('a', { href: '#' + r.screen }, names[r.screen] || r.screen) : pill('pominięte', 'warn')), 'W panelu'],
        [(r) => r.note || '', 'Uwagi', { class: 'w' }]]),
      h('div', { class: 'cmd' }, 'Menu web GUI legacy (Shared/MainLayout.razor) → odpowiednik w panelu albo świadome pominięcie (DOKUMENTACJA §8).'))];
}

VIEWS.diagnostyka = async () => {
  const [health, info, channels, exposure, cgHealth, cgState, warnings] = await Promise.all([
    cmd('GetAllHealthCheckResults', { Refresh: !!S.healthRefresh }).catch((e) => ({ error: e.message })),
    has('GetTroubleshootingInfo') ? cmd('GetTroubleshootingInfo', { NextVersion: '' }).catch((e) => ({ error: e.message })) : null,
    cmd('GetAllChannels', { ShowDisabled: false }).catch(() => []),
    api('/api/exposure').then((r) => r.result).catch((e) => ({ error: e.message, probes: [] })),
    S.status.cg ? cg('health').catch((e) => ({ error: e.message })) : null,
    S.status.cg ? cg('live/state').catch(() => null) : null,
    has('GetPlayoutWarningsCount') ? cmd('GetPlayoutWarningsCount').catch(() => null) : null,
  ]);
  const next = await loadNext();
  S.healthRefresh = false;
  const kind = { Pass: 'ok', Warning: 'warn', Error: 'bad', Info: 'info' };
  const commands = [...S.meta.values()];
  const sel = h('select', { id: 'con-cmd' }, commands.map((c) => h('option', { value: c.name }, `${c.name} (${c.kind === 'query' ? 'zapytanie' : 'komenda'})`)));
  const args = h('textarea', { id: 'con-args', rows: 8 });
  const out = h('pre', { class: 'out' }, '—');
  const template = () => {
    const m = S.meta.get(sel.value);
    const o = {};
    for (const p of m?.parameters || []) o[p.name] = p.values ? p.values[0] : p.type.startsWith('Int') ? 0 : p.type === 'Boolean' ? false : p.type === 'String' ? '' : null;
    args.value = JSON.stringify(o, null, 2);
  };
  sel.addEventListener('change', template);
  if (commands.length) template();
  const send = async () => {
    let body;
    try { body = JSON.parse(args.value || '{}'); } catch (e) { out.textContent = 'To nie jest JSON: ' + e.message; return; }
    out.textContent = '…';
    try { out.textContent = JSON.stringify(await cmd(sel.value, body), null, 2); }
    catch (e) { out.textContent = 'Błąd: ' + e.message; }
  };

  // wybór treści: pola treści pozycji (id kolekcji, serialu…) do użycia w konsoli
  const picked = h('pre', { class: 'out', id: 'cp-out' }, '—');
  const picker = contentPicker(null, { optional: true });
  const showPicked = () => (picked.textContent = JSON.stringify(Object.fromEntries(Object.entries(picker.peek()).filter(([, v]) => v != null && v !== '')), null, 2));
  picker.el.addEventListener('change', showPicked);
  picker.el.addEventListener('input', showPicked);
  showPicked();

  const infoRows = info && !info.error ? [
    { k: 'ErsatzTV legacy', v: info.Version }, { k: 'ffmpeg', v: info.FFmpegVersion }, { k: 'Silnik Next', v: info.NextVersion || '— (nie zgłoszony)' },
    ...(info.Cpus || []).map((c, i) => ({ k: `Procesor ${i + 1}`, v: `${c.Manufacturer} ${c.Name}` })),
    ...((info.VideoControllers || []).length ? info.VideoControllers.map((g, i) => ({ k: `Karta grafiki ${i + 1}`, v: `${g.Manufacturer ?? ''} ${g.Name ?? ''}`.trim() })) : [{ k: 'Karty grafiki', v: 'brak widocznych w kontenerze' }]),
    { k: 'Kanały / profile / znaki wodne', v: `${info.Channels?.length ?? '?'} / ${info.FFmpegProfiles?.length ?? '?'} / ${info.Watermarks?.length ?? '?'}` },
  ] : null;
  const capRefresh = has('RefreshFFmpegCapabilities') ? btn('Odśwież możliwości ffmpeg', async () => {
    try { await cmd('RefreshFFmpegCapabilities'); toast('Legacy sprawdzi ffmpeg i sprzęt w tle.', 'ok'); } catch (e) { toast(e.message, 'bad'); }
  }) : null;

  const n = (k, one, few, many) => `${k} ${k === 1 ? one : [2, 3, 4].includes(k % 10) && ![12, 13, 14].includes(k % 100) ? few : many}`;
  // „Stan” stosu (jak w makiecie): tylko z danych, które panel ma – bez zgadywania
  const b = S.status.bridge?.result;
  const hc = health.error ? null : { bad: health.filter((r) => r.Status === 'Error').length, warn: health.filter((r) => r.Status === 'Warning').length };
  const stack = [
    ['legacy', b ? ['OK', `ErsatzTV legacy ${b.legacy}`] : ['BŁĄD', S.status.bridgeError || 'most nieosiągalny']],
    ['most', b ? (b.commandsMissing?.length ? ['UWAGA', `VizRS Bridge ${b.bridge} · z białej listy nieznane w tej wersji legacy: ${b.commandsMissing.join(', ')}`]
      : ['OK', `VizRS Bridge ${b.bridge} · ${n(b.commandsAllowed, 'komenda', 'komendy', 'komend')} na białej liście`]) : ['BŁĄD', 'brak odpowiedzi']],
    ['kontrole legacy', !hc ? ['BŁĄD', health.error] : hc.bad ? ['BŁĄD', `${n(hc.bad, 'błąd', 'błędy', 'błędów')}, ${n(hc.warn, 'ostrzeżenie', 'ostrzeżenia', 'ostrzeżeń')} – tabela „Kontrole legacy” niżej`]
      : hc.warn ? ['UWAGA', `${n(hc.warn, 'ostrzeżenie', 'ostrzeżenia', 'ostrzeżeń')} – tabela „Kontrole legacy” niżej`] : ['OK', `${n(health.length, 'kontrola', 'kontrole', 'kontroli')} bez uwag`]],
    ['playouty', warnings == null ? null : warnings > 0 ? ['UWAGA', `${n(warnings, 'playout', 'playouty', 'playoutów')} z błędem ostatniej budowy – ekran Playouty`] : ['OK', 'ostatnie budowy bez błędów']],
    ['Next i ffmpeg', info && !info.error ? ['INFO', `Next ${info.NextVersion || '— (nie zgłoszony)'} · ffmpeg ${info.FFmpegVersion || '?'}`] : null],
    ['CG Playout', !S.status.cg ? ['—', 'nie ustawiony (VIZRS_LIVE_CG_URL)'] : cgHealth?.error ? ['BŁĄD', cgHealth.error]
      : ['OK', `połączony${cgState?.version ? ' · ' + cgState.version : ''} · kanałów: ${cgState?.channels?.length ?? '?'} · odbiór sygnałów ${cgState?.ingest ? 'włączony' : 'wyłączony'}`]],
    ['strefy grafiki', !cgHealth || cgHealth.error || !cgHealth.zones ? null
      : (() => {
        const runs = cgHealth.zones.runs || [];
        const txt = runs.length ? runs.map((r) => `kanał ${r.channel}: ${r.zones ? `${r.zones} (${r.share_percent}% ekranu)` : 'cały ekran'}${r.outside ? ` · grafika poza strefami: ${r.outside}` : ''}`).join('; ') : 'brak przebiegów nakładki';
        return [runs.some((r) => r.outside) ? 'UWAGA' : 'INFO', `auto (ślady stron: ${cgHealth.zones.pages}${cgHealth.zones.bounds != null ? `, granice z VizRS: ${cgHealth.zones.bounds}` : ''}) · ${txt}`];
      })()],
    ['VizRS', S.status.vizrsUrl ? ['INFO', `${S.status.vizrsUrl} – łączność widać w podglądzie /output (Grafika kanału)`] : ['—', 'nie ustawiony (VIZRS_LIVE_VIZRS_URL)']],
  ].filter(([, v]) => v);
  const stKind = { OK: 'ok', UWAGA: 'warn', BŁĄD: 'bad', INFO: 'info' };

  return [
    head('Diagnostyka', 'Stan całego stosu, kontrole legacy, wersje i sprzęt, konserwacja, test odtwarzania oraz konsola komend z białej listy.'),
    panel('Stan', btn('Sprawdź ponownie', () => { S.healthRefresh = true; route(true); }),
      h('div', { 'data-stack': '' }, table(stack.map(([k, [st, txt]]) => ({ k, st, txt })),
        [[(r) => pill(r.st, stKind[r.st]), 'Stan'], ['k', 'Składnik'], ['txt', 'Opis', { class: 'w' }]])),
      cmdNote('GET /api/status', 'GetAllHealthCheckResults', 'GetPlayoutWarningsCount', 'GetTroubleshootingInfo', 'CG Playout /health, /api/live/state')),
    h('div', { class: 'split' },
      panel('Kontrole legacy', btn('Sprawdź ponownie', () => { S.healthRefresh = true; route(true); }),
        health.error ? h('div', { class: 'empty err' }, health.error)
          : table(health, [[(r) => pill(pl(r.Status), kind[r.Status]), 'Stan'], ['Title', 'Kontrola'], [(r) => r.BriefMessage || r.Message, 'Opis', { class: 'w' }]]),
        cmdNote('GetAllHealthCheckResults')),
      panel('Wersje i sprzęt', capRefresh,
        !info ? missingNote(['GetTroubleshootingInfo']) : info.error ? h('div', { class: 'empty err' }, info.error)
          : table(infoRows, [['k', 'Pole'], ['v', 'Wartość', { class: 'w' }]]),
        cmdNote('GetTroubleshootingInfo', 'RefreshFFmpegCapabilities'))),
    nextPanel(next),
    ...exposurePanel(exposure),
    h('div', { class: 'split' },
      panel('Konserwacja', null,
        h('div', { class: 'bar pad', id: 'maint' }, MAINT.filter(([n]) => has(n)).map(([n, a, label, text]) => btn(label, () => maintenance(n, a, label, text)))),
        h('div', { class: 'cmd' }, 'Zadania w tle legacy – wynik i błędy w System → Logi. Przebudowa indeksu wyszukiwania: tylko przy starcie legacy (DOKUMENTACJA §5).'),
        cmdNote(...MAINT.map(([n]) => n))),
      panel('Test odtwarzania', null, playbackTest(channels), cmdNote('GetAllChannels'))),
    h('div', { class: 'split' },
      panel('Konsola komend', btn('Wyślij', send, 'acc'),
        h('div', { class: 'form' }, h('label', { for: 'con-cmd' }, 'Komenda'), sel, h('label', { for: 'con-args' }, 'Argumenty (JSON)'), args),
        h('div', { class: 'pad' }, out),
        h('div', { class: 'cmd' }, 'Tylko komendy z białej listy mostu. Komendy zmieniają dane legacy naprawdę.')),
      panel('Wybór treści', null,
        h('div', { class: 'pad' }, picker.el, h('p', { class: 'dim' }, 'Pola treści pozycji (do konsoli komend):'), picked),
        cmdNote('SearchCollections', 'SearchSmartCollections', 'SearchMultiCollections', 'GetAllPlaylistGroups', 'GetPlaylistsByPlaylistGroupId', 'SearchTelevisionShows', 'SearchTelevisionSeasons', 'SearchArtists'))),
  ];
};
