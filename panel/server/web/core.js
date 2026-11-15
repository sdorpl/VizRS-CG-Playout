// VizRS Live – rdzeń UI: DOM, API mostu, komunikaty, router, etykiety PL.
// Cały tekst trafia do DOM przez textContent (dane z legacy są obce).

export const main = document.getElementById('main');

/** Stan wspólny widoków: status serwera, metadane komend mostu, odświeżanie bieżącego widoku. */
export const S = {
  status: null,      // /api/status
  meta: new Map(),   // nazwa komendy -> {name, kind, parameters}
  timers: [],
  cleanup: [],       // funkcje sprzątające przy zmianie ekranu (np. podglądy mozaiki – K10)
  vizrsAnt: null,    // wybrana antena kanału w podglądzie VizRS
  channel: '',       // kanał wybrany w pasku górnym ('' = wszystkie) – filtr ekranów kanałowych
};

/** Czy kanał o numerze `num` przechodzi przez wybór kanału z paska górnego (pusty wybór = wszystkie). */
export const onChannel = (num) => !S.channel || String(num) === S.channel;

// ---------- narzędzia DOM ----------
export function h(tag, attrs, ...kids) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v == null || v === false) continue;
    if (k === 'class') el.className = v;
    else if (k.startsWith('on')) el.addEventListener(k.slice(2), v);
    else if (k === 'value') el.value = v;
    else if (k === 'checked') el.checked = !!v;
    else el.setAttribute(k, v === true ? '' : v);
  }
  for (const kid of kids.flat(Infinity)) {
    if (kid == null || kid === false) continue;
    el.append(kid instanceof Node ? kid : document.createTextNode(String(kid)));
  }
  return el;
}
export const pill = (text, kind) => h('span', { class: 'pill' + (kind ? ' ' + kind : '') }, text);
export const btn = (text, onclick, cls) => h('button', { type: 'button', class: cls, onclick }, text);

export function head(title, desc, ...buttons) {
  return h('div', { class: 'head' }, h('h1', null, title), h('span', { class: 'grow' }), buttons, desc ? h('p', null, desc) : null);
}
export function panel(title, extras, ...body) {
  return h('div', { class: 'panel' }, h('header', null, h('h2', null, title), extras), body);
}
export function cmdNote(...names) {
  return h('div', { class: 'cmd' }, 'Pod spodem: ', names.flatMap((n, i) => [i ? ', ' : '', h('b', null, n)]));
}

export function fmt(v) {
  if (v == null || v === '') return '—';
  if (v === true) return 'tak';
  if (v === false) return 'nie';
  if (typeof v === 'string' && /^\d{4}-\d\d-\d\dT/.test(v)) return dateTime(v);
  if (typeof v === 'object') return Array.isArray(v) ? `${v.length} poz.` : (v.Name ?? JSON.stringify(v));
  return String(v);
}
export function get(obj, path) {
  return path.split('.').reduce((o, k) => (o == null ? undefined : o[k]), obj);
}
export const pad2 = (n) => String(n).padStart(2, '0');
export const hm = (d) => `${pad2(d.getHours())}:${pad2(d.getMinutes())}`;
export const hms = (d) => `${hm(d)}:${pad2(d.getSeconds())}`;
export function dateTime(s) {
  const d = new Date(s);
  if (isNaN(d)) return s;
  const today = new Date();
  return d.toDateString() === today.toDateString() ? hms(d) : `${pad2(d.getDate())}.${pad2(d.getMonth() + 1)} ${hm(d)}`;
}
export function dur(ms) {
  const s = Math.max(0, Math.round(ms / 1000));
  const hh = Math.floor(s / 3600), mm = Math.floor(s / 60) % 60, ss = s % 60;
  return (hh ? hh + ':' + pad2(mm) : mm) + ':' + pad2(ss);
}

/** Tabela: cols = [[klucz|funkcja, nagłówek, {class}]]; onRow(row) – kliknięcie wiersza. */
export function table(rows, cols, opts = {}) {
  if (!rows || !rows.length) return h('div', { class: 'empty' }, opts.empty || 'Brak pozycji.');
  if (!cols) cols = Object.keys(rows[0]).filter((k) => typeof rows[0][k] !== 'object').slice(0, 8).map((k) => [k, k]);
  const body = rows.map((r) => {
    const tr = h('tr', { class: [opts.onRow ? 'click' : '', opts.selected && opts.selected(r) ? 'sel' : ''].join(' ').trim() || null },
      cols.map(([k, , o]) => {
        const v = typeof k === 'function' ? k(r) : get(r, k);
        return h('td', { class: o && o.class }, v instanceof Node ? v : fmt(v));
      }));
    if (opts.onRow) tr.addEventListener('click', (e) => { if (!e.target.closest('button,a,input,label')) opts.onRow(r, tr); });
    return tr;
  });
  return h('div', { class: 'tw' }, h('table', null, h('thead', null, h('tr', null, cols.map(([, t]) => h('th', null, t)))), h('tbody', null, body)));
}

// ---------- komunikaty ----------
let toastTimer;
/** Komunikat bez stosu wywołań .NET (legacy zwraca wyjątek z „at Przestrzeń.Typ.Metoda(…) in /source/…”). */
export const shortError = (msg) => String(msg).replace(/\s+at\s+[A-Z][\w`<>]*(\.[\w`<>[\],]+)+[\s(][\s\S]*$/, '').trim() || String(msg);
export function toast(msg, kind) {
  const t = document.getElementById('toast');
  const text = shortError(msg);
  if (text !== String(msg)) console.warn('VizRS Live – pełny komunikat:', msg);
  t.textContent = text;
  t.title = text !== String(msg) ? String(msg) : '';
  t.className = 'toast' + (kind ? ' ' + kind : '');
  t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (t.hidden = true), kind === 'bad' ? 7000 : 3000);
}
export function dialog(title, body, buttons) {
  const ov = document.getElementById('overlay');
  document.getElementById('dlg-title').textContent = title;
  document.getElementById('dlg-body').replaceChildren(body);
  return new Promise((resolve) => {
    // zmiana ekranu (hash) zamyka okno jak „Anuluj” – inaczej zostaje nad nowym ekranem
    const onNav = () => close(false);
    const close = (v) => { window.removeEventListener('hashchange', onNav); ov.hidden = true; resolve(v); };
    window.addEventListener('hashchange', onNav);
    document.getElementById('dlg-buttons').replaceChildren(...buttons.map(([t, v, cls]) => btn(t, () => close(v), cls)));
    ov.hidden = false;
    const first = ov.querySelector('input, button.acc, button.danger');
    if (first) first.focus();
  });
}
export const confirmDlg = (title, text, okText = 'Tak', cls = 'danger') =>
  dialog(title, h('p', null, text), [['Anuluj', false], [okText, true, cls]]);

// ---------- API ----------
export class ApiError extends Error {}

/** Nazwa stanowiska / operatora (pasek górny) – trafia do dziennika CG Playout jako „VizRS Live · nazwa”. */
export function operatorName(set) {
  try {
    if (set != null) localStorage.setItem('vizrs-live-operator', set);
    return localStorage.getItem('vizrs-live-operator') || '';
  } catch { return ''; }
}
export async function api(path, opts = {}) {
  const res = await fetch(path, {
    method: opts.method || 'GET',
    headers: { 'X-VizRS-Live': '1', ...(path.startsWith('/api/cg/') && operatorName() ? { 'X-Operator': encodeURIComponent(operatorName()) } : {}), ...(opts.body != null ? { 'Content-Type': opts.type || 'application/json' } : {}) },
    body: opts.body,
    credentials: 'same-origin',
  });
  let data;
  try { data = await res.json(); } catch { throw new ApiError(`odpowiedź ${res.status} bez JSON`); }
  if (res.status === 401 && path !== '/api/login') { await login(); throw new ApiError('wymagane logowanie'); }
  if (!data.ok) throw new ApiError(data.error || `błąd ${res.status}`);
  return data;
}
/**
 * Komenda albo zapytanie legacy; zwraca wynik albo rzuca ApiError z komunikatem legacy.
 * `queued` – zadanie w tle legacy; `warning` – wykonane, wyniku nie da się odczytać (DOKUMENTACJA §5).
 */
export async function cmd(name, args = {}) {
  const d = await api('/api/cmd/' + name, { method: 'POST', body: JSON.stringify(args) });
  if (d.queued) return { queued: d.queued };
  if (d.warning) return { warning: d.warning, result: d.result };
  return d.result;
}
/** Limit wgrywanego obrazu – taki sam w serwerze panelu i w moście. */
export const MAX_UPLOAD = 10 * 1024 * 1024;
export const IMAGE_TYPES = ['image/png', 'image/jpeg', 'image/gif', 'image/webp'];
/**
 * Obraz do pamięci obrazów legacy (SaveArtworkToDisk przez POST /api/upload/{Logo|Watermark}).
 * Zwraca nazwę pliku w legacy (skrót SHA, np. „39B2…”).
 */
export async function upload(kind, file) {
  if (!IMAGE_TYPES.includes(file.type)) throw new ApiError('dozwolone tylko obrazy PNG, JPEG, GIF i WebP');
  if (file.size > MAX_UPLOAD) throw new ApiError('obraz większy niż 10 MB');
  return (await api('/api/upload/' + kind, { method: 'POST', body: file, type: file.type })).result;
}
/** Adres pliku legacy dla przeglądarki (np. logo, znak wodny) – tylko gdy panel zna --stream-url. */
export function legacyUrl(path) {
  if (!path) return null;
  if (/^https?:\/\//.test(path)) return path;
  return S.status?.streamUrl ? S.status.streamUrl + '/' + path.replace(/^\//, '') : null;
}

// ---------- VizRS CG System (--vizrs-url): podgląd /output i Trio ----------
const vizrsCh = (channel) => (S.status?.vizrsChannels && channel != null && channel !== '' ? String(channel) : '');
/**
 * Adres VizRS dla przeglądarki albo null. Sam port w --vizrs-url (:8788) = host, pod którym operator
 * otworzył panel (ciasteczko VizRS działa tylko w tej samej witrynie). `/output` dostaje token wyjścia
 * (tylko odczyt) i antenę kanału `bus=ch-N`, Trio – `?channel=N` (przy --vizrs-channels).
 */
export function vizrsUrl(path, channel) {
  const base = S.status?.vizrsUrl;
  if (!base) return null;
  const root = base.startsWith(':') ? `${location.protocol}//${location.hostname}${base}` : base;
  const q = new URLSearchParams();
  const ch = vizrsCh(channel);
  if (path === '/output') {
    if (S.status.vizrsOutputToken) q.set('token', S.status.vizrsOutputToken);
    if (ch) q.set('bus', 'ch-' + ch);
  } else if (ch) q.set('channel', ch);
  const qs = q.toString();
  return root + path + (qs ? '?' + qs : '');
}
export const TRIO_HINT = 'Pierwsze wejście do Trio w tej przeglądarce: dopisz do adresu token operatora VizRS (?token=… albo &token=…); potem VizRS pamięta go w ciasteczku.';
/** Odnośnik „Trio ↗” (antena kanału przy --vizrs-channels) albo null. */
export function trioLink(channel, text = 'Trio ↗') {
  const href = vizrsUrl('/trio', channel);
  return href ? h('a', { href, target: '_blank', rel: 'noopener', title: TRIO_HINT, 'data-trio': '' }, text) : null;
}
/**
 * Panel podglądu anteny VizRS (/output) albo null bez --vizrs-url. Przy antenach kanałów ramka idzie za
 * wyborem kanału: własną listą z `choices` ([wartość, etykieta]) albo podanym `select` widoku.
 */
export function vizrsPreview(title, choices = [], select = null) {
  if (!S.status?.vizrsUrl) return null;
  const multi = !!S.status.vizrsChannels;
  const sel = multi && !select && choices.length
    ? h('select', { 'aria-label': 'Antena kanału VizRS', 'data-vizrs-channel': '' }, choices.map(([v, l]) => h('option', { value: v }, l)))
    : null;
  const src = select || sel;
  const frame = h('iframe', { class: 'preview', title: 'Podgląd grafiki VizRS', 'data-vizrs-output': '' });
  const trio = h('a', { target: '_blank', rel: 'noopener', title: TRIO_HINT, 'data-trio': '' }, 'Otwórz w Trio ↗');
  const which = h('span', { class: 'dim' });
  const set = () => {
    const ch = multi ? src?.value : '';
    frame.src = vizrsUrl('/output', ch);
    trio.href = vizrsUrl('/trio', ch);
    which.textContent = vizrsCh(ch) ? `Antena kanału ${ch} (bus=ch-${ch}). ` : multi ? 'Antena główna – przy antenach kanałów nie idzie na emisję. ' : 'Antena główna VizRS. ';
  };
  // wybór anteny przeżywa odświeżenie widoku (Antena przerysowuje się co 30 s)
  if (sel && choices.some(([v]) => v === S.vizrsAnt)) sel.value = S.vizrsAnt;
  src?.addEventListener('change', () => { S.vizrsAnt = src.value; set(); });
  set();
  return panel(title, sel, h('div', { class: 'pad' }, frame,
    h('div', { class: 'dim' }, which, trio),
    S.status.vizrsOutputToken ? null
      : h('div', { class: 'dim' }, 'VizRS z tokenem pokaże tu „wymagany token”: ustaw w panelu --vizrs-output-token (VIZRS_LIVE_VIZRS_OUTPUT_TOKEN = VIZRS_OUTPUT_TOKEN w VizRS) albo zaloguj tę przeglądarkę w Trio.')));
}

/** Tylko pola, które komenda przyjmuje (wg /vizrs-bridge/commands). */
export function argsFor(name, obj) {
  const m = S.meta.get(name);
  if (!m) return obj;
  const out = {};
  for (const p of m.parameters) if (p.name in obj) out[p.name] = obj[p.name];
  return out;
}
export const param = (name, p) => S.meta.get(name)?.parameters.find((x) => x.name === p);
export const enumValues = (name, p) => param(name, p)?.values || [];
export const has = (name) => S.meta.has(name);

/** Komendy potrzebne ekranowi, których nie ma na białej liście mostu (albo w tej wersji legacy). */
export const missing = (...names) => names.flat().filter((n) => !S.meta.has(n));

/** Komunikat zamiast pustego ekranu, gdy ekranowi brakuje komend. */
export function missingNote(names) {
  return h('div', { class: 'note err missing' },
    names.length === 1 ? 'Ta funkcja wymaga komendy ' : 'Ta funkcja wymaga komend ',
    names.flatMap((n, i) => [i ? ', ' : '', h('b', { class: 'mono' }, n)]),
    ' na białej liście mostu (bridge/commands.txt). Jeśli komenda jest na liście, sprawdź w Ustawieniach, czy ta wersja legacy ją zna.');
}
/** Widok wymagający komend: brakujące → komunikat, w przeciwnym razie zwykły widok. */
export function needs(names, view) {
  return async (...a) => {
    const miss = missing(names);
    if (miss.length) return [head(view.title || 'Brak komend'), missingNote(miss)];
    return view(...a);
  };
}

export async function login() {
  const input = h('input', { type: 'password', id: 'login-token', autocomplete: 'current-password', 'aria-label': 'Token panelu' });
  for (;;) {
    const ok = await dialog('Logowanie do VizRS Live', h('div', null, h('label', { for: 'login-token' }, 'Token panelu (VIZRS_LIVE_TOKEN)'), input), [['Zaloguj', true, 'acc']]);
    if (!ok) continue;
    try {
      await api('/api/login', { method: 'POST', body: input.value, type: 'text/plain' });
      location.reload();
      return;
    } catch (e) { toast(e.message, 'bad'); }
  }
}

// ---------- etykiety po polsku ----------
export const PL = {
  HttpLiveStreamingSegmenter: 'HLS Segmenter', HttpLiveStreamingDirect: 'HLS Direct', TransportStream: 'MPEG-TS', TransportStreamHybrid: 'MPEG-TS (hybryda)',
  Continuous: 'ciągły', OnDemand: 'na żądanie', Generated: 'własny playout', Mirror: 'kanał lustrzany',
  StopOnDisconnect: 'zatrzymaj, gdy nikt nie ogląda', KeepRunning: 'nadawaj zawsze',
  None: 'brak', Forced: 'tylko wymuszone', Default: 'domyślne', Any: 'dowolne', Burn: 'wypalaj', Convert: 'konwertuj',
  Classic: 'klasyczny', Block: 'blokowy', Sequential: 'sekwencyjny (YAML)', Scripted: 'skryptowy', ExternalJson: 'zewnętrzny JSON',
  Refresh: 'odśwież', Reset: 'od nowa', Continue: 'kontynuuj',
  Strict: 'ściśle', Flexible: 'elastycznie', Dynamic: 'dynamiczny', Fixed: 'stała godzina',
  Pass: 'OK', Warning: 'uwaga', Error: 'błąd', Info: 'info',
  Next: 'Next', Legacy: 'Legacy', Software: 'programowo', Qsv: 'QSV', Nvenc: 'NVENC', Vaapi: 'VAAPI', VideoToolbox: 'VideoToolbox', Amf: 'AMF', V4l2m2m: 'V4L2', Rkmpp: 'RKMPP',
  Permanent: 'stały', Intermittent: 'przerywany', OpacityExpression: 'wyrażenie krycia',
  Custom: 'wgrany obraz', ChannelLogo: 'logo kanału', Scaled: 'skalowany (% szerokości kadru)', ActualSize: 'rozmiar oryginalny',
  TopLeft: 'lewy górny', TopMiddle: 'górny środek', TopRight: 'prawy górny', LeftMiddle: 'lewy środek', MiddleCenter: 'środek',
  RightMiddle: 'prawy środek', BottomLeft: 'lewy dolny', BottomMiddle: 'dolny środek', BottomRight: 'prawy dolny',
  // profile kodowania
  H264: 'H.264', Hevc: 'HEVC (H.265)', Mpeg2Video: 'MPEG-2', Av1: 'AV1', Copy: 'kopiuj bez zmian', Aac: 'AAC', Ac3: 'AC-3', AacLatm: 'AAC LATM (tylko DVB-C)',
  EightBit: '8 bit', TenBit: '10 bit', ScaleAndPad: 'skaluj i dodaj pasy', Stretch: 'rozciągnij', Crop: 'przytnij',
  Off: 'wyłączona', LoudNorm: 'loudnorm (EBU R128)', Linear: 'liniowy', Clip: 'obcięcie', Gamma: 'gamma', Reinhard: 'Reinhard', Mobius: 'Möbius', Hable: 'Hable',
  PreRoll: 'przed (pre-roll)', MidRoll: 'w trakcie (mid-roll)', PostRoll: 'po (post-roll)', Tail: 'ogon', Fallback: 'zapasowy', DeadAir: 'martwe powietrze',
  Count: 'liczba', Duration: 'czas', Pad: 'dopełnienie', RandomCount: 'losowa liczba',
  Movies: 'filmy', Shows: 'seriale', MusicVideos: 'teledyski', OtherVideos: 'inne wideo', Songs: 'muzyka', Images: 'obrazy', RemoteStreams: 'strumienie zdalne',
  // rodzaje treści (CollectionType)
  Collection: 'kolekcja', SmartCollection: 'kolekcja inteligentna', MultiCollection: 'wielokolekcja', Playlist: 'playlista',
  TelevisionShow: 'serial', TelevisionSeason: 'sezon', Artist: 'artysta', SearchQuery: 'wyszukiwanie',
  RerunFirstRun: 'powtórki: premiery', RerunRerun: 'powtórki: powtórki',
  // pojedyncze pozycje (pozycje playlisty)
  Movie: 'film', Episode: 'odcinek', MusicVideo: 'teledysk', OtherVideo: 'inne wideo', Song: 'utwór', Image: 'obraz', RemoteStream: 'strumień zdalny',
  // kolejność odtwarzania
  // deco i bloki (R6)
  Inherit: 'dziedzicz', Disable: 'wyłącz', Override: 'zastąp', Merge: 'scal', RandomRotation: 'losowa rotacja',
  BlockStart: 'start bloku', BlockFinish: 'koniec bloku', BetweenBlockItems: 'między pozycjami', ChapterMarkers: 'znaczniki rozdziałów',
  BeforeDurationEnd: 'nie przekraczaj długości', AfterDurationEnd: 'może wyjść poza koniec',
  Chronological: 'chronologicznie', Random: 'losowo', Shuffle: 'tasuj', ShuffleInOrder: 'tasuj z zachowaniem kolejności', SeasonEpisode: 'sezon, odcinek',
};
export const isFiller = (it) => it.FillerKind != null && it.FillerKind !== 'None';
export const pl = (v) => (v == null ? '—' : PL[v] ?? String(v));

// ---------- router ----------
/** Rejestr widoków: nazwa (hash) -> async () => węzły. Wypełniają go moduły views/*. */
export const VIEWS = {};

export async function route(keepScroll) {
  S.timers.forEach(clearInterval);
  S.timers = [];
  S.cleanup.forEach((f) => { try { f(); } catch { /* sprzątanie bez wpływu na nowy ekran */ } });
  S.cleanup = [];
  let name = (location.hash || '#antena').slice(1);
  if (!VIEWS[name]) name = 'antena';
  document.querySelectorAll('.side a').forEach((a) => a.classList.toggle('on', a.getAttribute('href') === '#' + name));
  const top = main.scrollTop;
  delete main.dataset.view;
  // każde przejście ma numer: wolny ekran (np. odświeżenie listy playoutów po usunięciu), który skończy się
  // już po przejściu dalej, nie nadpisuje nowego widoku
  const gen = (S.routeGen = (S.routeGen || 0) + 1);
  let nodes;
  try {
    nodes = (await VIEWS[name]()).filter(Boolean);
  } catch (e) {
    nodes = [head('Błąd'), h('div', { class: 'note err' }, e.message), btn('Spróbuj ponownie', () => route())];
  }
  if (gen !== S.routeGen) return;
  main.replaceChildren(...nodes);
  main.dataset.view = name; // gotowy widok (testy e2e czekają na main[data-view])
  main.dataset.render = String((+main.dataset.render || 0) + 1);
  if (keepScroll === true) main.scrollTop = top;
}
