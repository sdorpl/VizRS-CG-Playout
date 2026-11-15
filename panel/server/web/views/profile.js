// Profile kodowania (R5): jak silnik Next koduje kanał. Odpowiednik FFmpeg Profiles + FFmpeg Editor legacy.

import { VIEWS, S, h, api, cmd, pl, pill, btn, panel, table, toast, confirmDlg, route, has, cmdNote, enumValues } from '../core.js';
import { form } from '../form.js';
import { crudView } from '../parts.js';

/**
 * Pola tylko dla starego silnika legacy (ThreadCount, PadMode, VideoProfile, VideoPreset, AllowBFrames,
 * VaapiDisplay, QsvExtraHardwareFrames, NormalizeColors) panel pokazuje tylko do odczytu w podpowiedzi –
 * zapis zostawia ich wartości bez zmian (z profilu albo z NewFFmpegProfile).
 */
const profName = (p) => p.Name;
const vid = (v) => v.NormalizeVideo;
const aud = (v) => v.NormalizeAudio;

function hwHint(supported) {
  return (v) => {
    const k = v.HardwareAcceleration;
    const list = `Sprzęt zgłoszony przez ffmpeg legacy: ${supported.map(pl).join(', ')}.`;
    if (!k || k === 'None') return list + ' Bez akceleracji kodowanie idzie na procesorze.';
    if (!supported.includes(k)) return `UWAGA: ffmpeg legacy nie zgłasza ${pl(k)} na tej maszynie – kanały z tym profilem pójdą programowo albo nie wystartują. ` + list;
    return list + (k === 'Vaapi' ? ' VAAPI potrzebuje węzła renderowania (/dev/dri/renderD*) przekazanego do kontenera legacy.' : '');
  };
}

/** Podpowiedź akceleracji wg silnika Next (K5); `probe === false` – Next jeszcze nie sprawdził sprzętu. */
function nextHint(probe, supported) {
  return (v) => {
    const c = nextCheck(v, probe || null);
    if (c.status === 'software') return hwHint(supported)(v);
    const [kind, text] = nextCheckText(v, c);
    const codecs = c.entry ? accelCodecs(c.entry) : null;
    return (kind === 'ok' ? '✓ ' : kind === 'bad' ? '✗ ' : kind === 'warn' ? '⚠ ' : '') + text
      + (codecs ? ` Dekodowanie: ${codecs.dec.join(', ') || '—'}; kodowanie: ${codecs.enc.join(', ') || '—'}.` : '');
  };
}

// ---------- klatkaż wyjściowy profilu (most: filtr na GetChannelFramerate, GET/POST /api/framerate…) ----------
/** Wartości listy „Klatkaż”: source – bez wyrównywania, auto – wylicza legacy, liczba – ustawia most. */
export const FRAME_RATES = [
  ['source', 'jak materiał (bez wyrównywania)'],
  ['auto', 'automatycznie – najniższy z ramówki (legacy)'],
  ['25', '25 kl./s (PAL, TV w Polsce)'],
  ['50', '50 kl./s'],
  ['24000/1001', '23,976 kl./s'],
  ['24', '24 kl./s'],
  ['30000/1001', '29,97 kl./s'],
  ['30', '30 kl./s'],
  ['60000/1001', '59,94 kl./s'],
  ['60', '60 kl./s'],
];
/** „30000/1001” → „29,97”; „25” → „25”. */
export function fmtRate(r) {
  const [n, d = '1'] = String(r).split('/');
  const v = Number(n) / Number(d);
  return Number.isFinite(v) ? String(Math.round(v * 1000) / 1000).replace('.', ',') : String(r);
}
/** Wybór z listy dla profilu: klatkaż ustawiony w moście, inaczej auto/source wg NormalizeFramerate. */
export const rateChoice = (p, rates) => rates?.[p?.Id] ?? (p?.NormalizeFramerate ? 'auto' : 'source');
/** Klatkaż do mostu z wyboru: liczba albo null (auto, source). */
export const rateToBridge = (choice) => (/^\d/.test(choice || '') ? choice : null);

function rateHint(v) {
  const c = v.FrameRateChoice;
  if (c === 'source') return 'Każdy materiał idzie z własnym klatkażem – przy mieszanej ramówce (25 i 23,976) część odbiorników gubi obraz na styku.';
  if (c === 'auto') return 'Legacy wybiera najniższy klatkaż powyżej 23 kl./s spośród materiałów w playoucie – jeden film 23,976 wśród materiałów 25 ustawi cały kanał na 23,976.';
  return `Kanał zawsze w ${fmtRate(c)} kl./s – Next dopasowuje każdy materiał, gubiąc albo powtarzając klatki. Ustawienie mostu VizRS (nie bazy legacy); działa od następnego startu strumienia kanału.`;
}

function loudHint(v) {
  if (v.NormalizeLoudnessMode !== 'LoudNorm') return 'Telewizja w Polsce: −23 LUFS (EBU R128) – włącz loudnorm, żeby kanał nie był głośniejszy od innych.';
  const t = v.TargetLoudness;
  if (t == null) return 'Podaj poziom, dla TV w Polsce −23 LUFS.';
  return t === -23 ? 'Zgodne z EBU R128 (TV w Polsce).' : `Dla TV w Polsce zalecane −23 LUFS (teraz ${t}).`;
}

// ---------- K5: stan silnika Next (GetNextProbe – legacy ≥ 26.12.0) ----------
// Pola opisu Next mają nazwy snake_case (JsonPropertyName w legacy), wyliczenia – nazwy C# z mostu (np. „Available”);
// `nv` czyta oba zapisy, żeby panel nie zależał od serializacji.
const nv = (o, snake, pascal) => (o == null ? undefined : o[snake] ?? o[pascal]);
const ACCEL = { Amf: 'amf', Nvenc: 'cuda', Qsv: 'qsv', Rkmpp: 'rkmpp', Vaapi: 'vaapi', VideoToolbox: 'videotoolbox' };
const DRIVER = { iHD: 'ihd', i965: 'i965', RadeonSI: 'radeonsi' };
export const DEFAULT_VAAPI_DEVICE = '/dev/dri/renderD128';
const available = (a) => String(nv(a, 'status', 'Status') || '').toLowerCase() === 'available';
const encodes = (a, fmt, depth) => (nv(a, 'encode', 'Encode') || []).some((e) => nv(e, 'format', 'Format') === fmt && (nv(e, 'bit_depths', 'BitDepths') || []).map(Number).includes(depth));
/** Akceleracje z opisu sprzętu Next (`NextProbeResult.Capability.accels`). */
export const accels = (probe) => nv(nv(probe, 'Capability', 'capability'), 'accels', 'Accels') || [];

/**
 * Czy profil zakoduje się sprzętowo w silniku Next – ta sama logika co `NextHardware.Check` legacy (FFmpeg Editor 26.12).
 * Wynik: { status: 'software'|'not-probed'|'unsupported'|'not-on-platform'|'device-not-probed'|'unavailable'|'software-encode'|'hardware', entry, working }.
 */
export function nextCheck(v, probe) {
  const kind = v.HardwareAcceleration;
  if (!kind || kind === 'None') return { status: 'software', working: [] };
  const accel = ACCEL[kind];
  if (!accel) return { status: 'unsupported', working: [] };
  if (!probe) return { status: 'not-probed', working: [] };
  const fmt = v.VideoFormat === 'Hevc' ? 'hevc' : v.VideoFormat === 'Mpeg2Video' ? 'mpeg2video' : 'h264';
  const depth = fmt !== 'mpeg2video' && v.BitDepth === 'TenBit' ? 10 : 8;
  const same = accels(probe).filter((a) => nv(a, 'accel', 'Accel') === accel);
  if (!same.length) return { status: 'not-on-platform', working: [] };
  const dev = (v.VaapiDevice || '').trim() || DEFAULT_VAAPI_DEVICE;
  const drv = DRIVER[v.VaapiDriver] ?? null;
  const entry = accel === 'vaapi' ? same.find((a) => nv(a, 'device', 'Device') === dev && (nv(a, 'driver', 'Driver') ?? null) === drv) : same[0];
  const working = same.filter((a) => a !== entry && available(a) && encodes(a, fmt, depth));
  if (!entry) return { status: 'device-not-probed', working, fmt, depth };
  const status = available(entry) ? (encodes(entry, fmt, depth) ? 'hardware' : 'software-encode') : 'unavailable';
  return { status, entry, working, fmt, depth };
}

/**
 * Akceleracje, które na tej maszynie kodują (nazwy `HardwareAcceleration`: Nvenc, Qsv, Vaapi…).
 * Z silnika Next: wpisy dostępne i z co najmniej jednym koderem; bez komendy GetNextProbe (starsze legacy) – lista
 * zgłoszona przez ffmpeg legacy. `null` = nie wiadomo (Next jeszcze nie sprawdził sprzętu).
 */
export function workingAccels(probe, supported) {
  if (probe === undefined) return (supported || []).filter((k) => k && k !== 'None');
  if (!probe) return null;
  const list = accels(probe);
  return Object.keys(ACCEL).filter((k) => list.some((a) => nv(a, 'accel', 'Accel') === ACCEL[k] && available(a) && (nv(a, 'encode', 'Encode') || []).length));
}
/** Czy profil chce akceleracji, której maszyna nie ma (pójdzie programowo). */
export const hwMissing = (p, working) => Array.isArray(working) && p.HardwareAcceleration && p.HardwareAcceleration !== 'None' && !working.includes(p.HardwareAcceleration);

/** Opis wyniku `nextCheck` po polsku – jak komunikaty edytora profilu legacy. */
export function nextCheckText(v, c) {
  const k = pl(v.HardwareAcceleration);
  const what = c.fmt ? `${c.fmt} ${c.depth}-bit` : '';
  const alt = c.working?.length ? ` Działa: ${c.working.map((a) => [nv(a, 'accel', 'Accel'), nv(a, 'device', 'Device'), nv(a, 'driver', 'Driver')].filter(Boolean).join(' ')).join(', ')}.` : '';
  const err = c.entry && nv(c.entry, 'error', 'Error') ? ` (${nv(c.entry, 'error', 'Error')})` : '';
  return {
    software: ['', ''],
    'not-probed': ['info', 'Silnik Next nie sprawdził jeszcze sprzętu – profilu nie da się ocenić (Diagnostyka → Silnik Next → „Sprawdź ponownie”).'],
    unsupported: ['warn', `Silnik Next nie obsługuje ${k} – kanały pójdą programowo.`],
    'not-on-platform': ['warn', `Silnik Next nie użyje ${k} na tej maszynie – kanały pójdą programowo.`],
    'device-not-probed': ['warn', `Next nie sprawdził tego urządzenia / sterownika VAAPI – nie wiadomo, czy zadziała.${alt}`],
    unavailable: ['bad', `Kanały pójdą programowo na tej maszynie${err}.${alt}`],
    'software-encode': ['warn', `${k} nie zakoduje ${what} na tej maszynie – dekodowanie i filtry sprzętowo, kodowanie programowo.${alt}`],
    hardware: ['ok', `Działa: ${k} koduje ${what} sprzętowo w silniku Next.`],
  }[c.status];
}

/** Kodeki dekodowania i kodowania sprzętowego akceleracji (do podpowiedzi i Diagnostyki). */
export function accelCodecs(a) {
  const dec = (nv(a, 'decode', 'Decode') || []).map((d) => nv(d, 'format', 'Format'));
  const enc = (nv(a, 'encode', 'Encode') || []).map((e) => `${nv(e, 'format', 'Format')} ${(nv(e, 'bit_depths', 'BitDepths') || []).join('/')}-bit`);
  return { dec, enc };
}

// ---------- K4: własne rozdzielczości (CreateCustomResolution, DeleteCustomResolution) ----------
/** Szerokość i wysokość własnej rozdzielczości: liczby parzyste 16–7680 × 16–4320 (kodery wymagają parzystych). */
export function checkResolution(w, hgt, existing) {
  const W = Number(w), H = Number(hgt);
  if (!Number.isInteger(W) || !Number.isInteger(H)) return 'podaj szerokość i wysokość w pikselach (liczby całkowite)';
  if (W < 16 || W > 7680 || H < 16 || H > 4320) return 'zakres: szerokość 16–7680, wysokość 16–4320';
  if (W % 2 || H % 2) return 'szerokość i wysokość muszą być parzyste (wymóg koderów H.264/HEVC)';
  if (existing.some((r) => r.Width === W && r.Height === H)) return `rozdzielczość ${W}x${H} już jest`;
  return null;
}

function resolutionsPanel(resolutions, profiles) {
  const canAdd = has('CreateCustomResolution'), canDel = has('DeleteCustomResolution');
  const w = h('input', { id: 'res-w', type: 'number', min: 16, max: 7680, step: 2, placeholder: 'szer.', 'aria-label': 'Szerokość' });
  const hh = h('input', { id: 'res-h', type: 'number', min: 16, max: 4320, step: 2, placeholder: 'wys.', 'aria-label': 'Wysokość' });
  const add = async () => {
    const e = checkResolution(w.value, hh.value, resolutions);
    if (e) { toast('Rozdzielczość: ' + e, 'bad'); return; }
    try { await cmd('CreateCustomResolution', { Width: Number(w.value), Height: Number(hh.value) }); toast(`Dodano ${w.value}x${hh.value}`, 'ok'); route(true); } catch (err) { toast('Legacy odrzucił rozdzielczość: ' + err.message, 'bad'); }
  };
  const del = async (r) => {
    const used = profiles.filter((p) => p.Resolution?.Id === r.Id);
    if (!(await confirmDlg('Usuń rozdzielczość', used.length ? `Rozdzielczość ${r.Name} mają profile: ${used.map((p) => p.Name).join(', ')}. Legacy może odmówić. Usunąć?` : `Usunąć własną rozdzielczość ${r.Name}?`, 'Usuń'))) return;
    try { await cmd('DeleteCustomResolution', { ResolutionId: r.Id }); toast(`Usunięto ${r.Name}`, 'ok'); route(true); } catch (err) { toast('Legacy odrzucił usunięcie: ' + err.message, 'bad'); }
  };
  return panel('Rozdzielczości', h('span', { class: 'dim' }, String(resolutions.length)),
    table(resolutions, [['Name', 'Nazwa'], [(r) => `${r.Width} × ${r.Height}`, 'Piksele', { class: 'num' }],
      [(r) => (r.IsCustom ? pill('własna', 'info') : h('span', { class: 'dim' }, 'wbudowana')), 'Rodzaj'],
      [(r) => profiles.filter((p) => p.Resolution?.Id === r.Id).length || '—', 'Profile', { class: 'num' }],
      [(r) => (r.IsCustom && canDel ? btn('Usuń', () => del(r), 'danger sm') : ''), '']]),
    canAdd ? h('div', { class: 'bar pad res-add' }, w, h('span', null, '×'), hh, btn('+ Dodaj rozdzielczość', add, 'acc')) : null,
    cmdNote('GetAllResolutions', 'CreateCustomResolution', 'DeleteCustomResolution'));
}

// ---------- koder programowy nakładki VizRS CG Playout (/api/cg/live/encoder) ----------
/** Uwagi do presetów x264 (pomiary A1, CG Playout docs/plany/PLAN-ampere.md). */
const PRESET_NOTES = {
  ultrafast: 'najszybszy; na A1 odrzucony: −3,4…−5 dB SSIM',
  superfast: 'kilka kanałów, materiały 1080p50/60, ciągły pasek',
  veryfast: 'domyślny na Ampere A1',
  medium: 'domyślny x264 – za wolny na A1',
};
/** Zmierzone, a dotąd niewdrożone opcje libx264 (A1, 900p25, Gothic: CPU / SSIM względem samego presetu). */
export const X264_VARIANTS = [
  ['subme=0', 'subme=0 – CPU −8%, SSIM −0,39 dB'],
  ['bframes=0', 'bframes=0 – CPU −15%, SSIM −0,77 dB'],
  ['bframes=0:subme=0', 'bframes=0:subme=0 – CPU −20%, SSIM −1,44 dB'],
];
/** Profile H.264 libx264 (8-bit 4:2:0). */
const PROFILE_NOTES = {
  baseline: 'bez klatek B i CABAC – najsłabsza kompresja, bardzo stare odbiorniki',
  main: 'zgodność ze starszymi dekoderami i telewizorami – domyślny NVENC',
  high: 'najlepsza kompresja – domyślny x264',
};
/** Presety NVENC (SDK 10+). */
const NVENC_NOTES = { p1: 'najszybszy, najniższa jakość', p2: 'szybszy', p3: 'szybki', p4: 'średni – domyślny NVENC', p5: 'wolny, dobra jakość', p6: 'wolniejszy, lepsza jakość', p7: 'najwolniejszy, najlepsza jakość' };
/** Klatki B koderów sprzętowych: pamięć karty kodera NVENC 1080p50 (pomiar Quadro P400, CG Playout PLAN-p400). */
const BFRAME_NOTES = { 0: 'najmniej pamięci karty (NVENC 1080p50: 79 MiB zamiast 139)', 1: 'NVENC 1080p50: 99 MiB' };
const FROM = {
  panel: 'ustawione w panelu',
  env: 'z .env serwera – zapisz, by przenieść do panelu',
  default: 'domyślne',
};

/** Wiadomo, że maszyna nie koduje sprzętowo (pusta lista działających akceleracji). */
const noHw = (working) => Array.isArray(working) && !working.length;

/** Treść pola „w użyciu”: wartość albo opis domyślnej. */
export function encoderUsed(value, empty) {
  return value || empty;
}

/** Panel kodera nakładki: co obowiązuje i skąd; profil H.264, preset i opcje x264, presety NVENC i QSV, klatki B sprzętowe (od następnego ffmpeg, wszystkie kanały). */
function encoderPanel(enc, working) {
  const title = 'Koder (nakładka VizRS CG)';
  if (enc == null) return null;
  if (enc instanceof Error) {
    return panel(title, null, h('div', { class: 'note' },
      'VizRS CG Playout nie udostępnia ustawień kodera (starsza wersja albo brak połączenia): ', enc.message,
      '. Preset ustawia wtedy tylko .env stosu (VIZRS_FFMPEG_PRESET).'));
  }
  const used = enc.used || {};
  const preset = h('select', { id: 'enc-preset', 'aria-label': 'Preset x264' },
    h('option', { value: '' }, 'domyślny ErsatzTV (medium)'),
    (enc.presets || []).map((p) => h('option', { value: p }, PRESET_NOTES[p] ? `${p} – ${PRESET_NOTES[p]}` : p)));
  preset.value = used.preset || '';
  const profile = h('select', { id: 'enc-profile', 'aria-label': 'Profil H.264' },
    h('option', { value: '' }, noHw(working) ? 'domyślny kodera (x264: high)' : 'domyślny kodera (x264: high, NVENC: main)'),
    (enc.profiles || []).map((p) => h('option', { value: p }, PROFILE_NOTES[p] ? `${p} – ${PROFILE_NOTES[p]}` : p)));
  profile.value = used.profile || '';
  const known = X264_VARIANTS.map(([v]) => v);
  const variant = h('select', { id: 'enc-x264', 'aria-label': 'Opcje x264' },
    h('option', { value: '' }, 'bez opcji'),
    X264_VARIANTS.map(([v, label]) => h('option', { value: v }, label)),
    h('option', { value: 'custom' }, 'własne…'));
  const custom = h('input', { id: 'enc-x264-custom', type: 'text', placeholder: 'np. bframes=0:subme=1', 'aria-label': 'Własne opcje x264', size: 28 });
  const opts = used.x264_params || '';
  variant.value = !opts ? '' : known.includes(opts) ? opts : 'custom';
  custom.value = variant.value === 'custom' ? opts : '';
  const syncCustom = () => { custom.hidden = variant.value !== 'custom'; };
  variant.addEventListener('change', syncCustom);
  syncCustom();
  const choice = (id, label, def, values, notes, value) => {
    const el = h('select', { id, 'aria-label': label }, h('option', { value: '' }, def),
      values.map((v) => h('option', { value: String(v) }, notes[v] ? `${v} – ${notes[v]}` : String(v))));
    el.value = value || '';
    return el;
  };
  const nvenc = choice('enc-nvenc', 'Preset NVENC', 'domyślny NVENC (p4)', enc.nvenc_presets || [], NVENC_NOTES, used.nvenc_preset);
  const qsv = choice('enc-qsv', 'Preset QSV', 'domyślny sterownika', enc.qsv_presets || [], {}, used.qsv_preset);
  const counts = Array.from({ length: (enc.max_bframes ?? 4) + 1 }, (_, i) => i);
  const bframes = choice('enc-bframes', 'Klatki B koderów sprzętowych', 'domyślne kodera', counts, BFRAME_NOTES, used.bframes);
  const hardware = Boolean(enc.nvenc_presets);
  const softwareOnly = noHw(working);
  const save = async () => {
    const x264 = variant.value === 'custom' ? custom.value.trim() : variant.value;
    // PUT zastępuje wszystkie pola – ustawienia sprzętowe idą zawsze (z pól albo bez zmian z „w użyciu”)
    const body = { preset: preset.value, profile: profile.value, x264_params: x264,
      nvenc_preset: hardware ? nvenc.value : used.nvenc_preset || '', qsv_preset: hardware ? qsv.value : used.qsv_preset || '',
      bframes: hardware ? bframes.value : used.bframes ?? '' };
    try {
      await api('/api/cg/live/encoder', { method: 'PUT', body: JSON.stringify(body) });
      toast('Koder zapisany – działa od następnego startu ffmpeg (następna pozycja ramówki)', 'ok');
      route(true);
    } catch (e) { toast('VizRS CG Playout odrzucił ustawienia kodera: ' + e.message, 'bad'); }
  };
  const field = (label, control) => [h('dt', null, label), h('dd', null, control)];
  const editable = enc.store !== false;
  const sub = (text) => [h('dt', { class: 'dim' }, text), h('dd', null)];
  const hwFields = () => [
    ...sub('Sprzętowo (NVENC, QSV, VAAPI)'),
    ...field('Preset NVENC', editable ? nvenc : encoderUsed(used.nvenc_preset, 'domyślny NVENC (p4)')),
    ...field('Preset QSV', editable ? qsv : encoderUsed(used.qsv_preset, 'domyślny sterownika')),
    ...field('Klatki B', editable ? bframes : encoderUsed(used.bframes, 'domyślne kodera')),
  ];
  return panel(title, h('span', { class: 'dim' }, `wszystkie kanały · ${FROM[enc.from] || enc.from}`),
    softwareOnly
      ? h('div', { class: 'note pad', 'data-enc-software': '' },
        'Ta maszyna nie ma akceleracji sprzętowej – wszystkie kanały kodują programowo (libx264), więc obowiązują profil H.264, preset i opcje x264 poniżej. ',
        'O obciążeniu procesora decyduje preset: domyślny ErsatzTV (medium) jest ciężki, na Ampere A1 zalecany veryfast, przy wielu kanałach albo 1080p50 – superfast.')
      : null,
    h('dl', { class: 'kv' },
      ...field('Profil H.264', editable ? profile : encoderUsed(used.profile, 'domyślny kodera')),
      ...sub('Programowo (libx264)'),
      ...field('Preset x264', editable ? preset : encoderUsed(used.preset, 'domyślny ErsatzTV (medium)')),
      ...field('Opcje x264', editable ? h('span', { class: 'bar' }, variant, custom) : encoderUsed(used.x264_params, 'bez opcji')),
      ...(hardware && !softwareOnly ? hwFields() : [])),
    hardware && softwareOnly
      ? h('details', { class: 'pad', 'data-enc-hw': '' }, h('summary', { class: 'dim' }, 'Ustawienia koderów sprzętowych (NVENC, QSV, VAAPI) – nieużywane na tej maszynie'),
        h('dl', { class: 'kv' }, ...hwFields()))
      : null,
    editable
      ? h('div', { class: 'bar pad end' }, btn('Zapisz koder', save, 'acc'))
      : h('div', { class: 'note pad' }, 'CG Playout nie ma etv_config – ustawień nie ma gdzie zapisać; obowiązuje .env serwera.'),
    h('div', { class: 'hint pad dim' },
      'Silnik Next pomija preset i profil H.264 z profilu kodowania – także dla akceleracji sprzętowej – więc dokłada je nakładka vizrs-ffmpeg; ',
      'który zestaw działa, wynika z akceleracji w profilu kanału. Profil H.264 dotyczy wszystkich koderów H.264 (HEVC bez zmian; NVENC bez ustawienia koduje w main). ',
      'VAAPI nie ma presetu – dostaje profil i klatki B. Ustawienia stąd zastępują .env serwera (.env wypełnia je tylko przy pierwszym starcie). ',
      'Opcje x264 to wymiana jakości na procesor: włącz, gdy kanał nie trzyma tempa 1×. Dozwolone klucze własnych opcji: ', (enc.x264_keys || []).join(', '), '.'));
}

VIEWS.profile = crudView({
  title: 'Profile kodowania',
  desc: 'Jak silnik Next koduje kanał: rozdzielczość, obraz, dźwięk, akceleracja sprzętowa. Odpowiednik FFmpeg Profiles w legacy. Zmiana działa od następnego startu strumienia kanału.',
  listCmd: 'GetAllFFmpegProfiles',
  needs: ['NewFFmpegProfile', 'GetAllResolutions', 'GetSupportedHardwareAccelerationKinds', 'GetAllChannels'],
  commands: { create: 'CreateFFmpegProfile', update: 'UpdateFFmpegProfile', delete: 'DeleteFFmpegProfile', copy: 'CopyFFmpegProfile' },
  noun: { list: 'Profile', new: 'Nowy profil', add: '+ Nowy profil', create: 'Załóż profil', one: profName, title: profName },
  load: async () => {
    const [rows, resolutions, supported, fresh, channels, probe] = await Promise.all([
      cmd('GetAllFFmpegProfiles'), cmd('GetAllResolutions'), cmd('GetSupportedHardwareAccelerationKinds'), cmd('NewFFmpegProfile'),
      cmd('GetAllChannels', { ShowDisabled: true }),
      // K5: stan sprzętu wg Next – starsze legacy bez komendy = brak (podpowiedź z ffmpeg jak dotąd)
      has('GetNextProbe') ? cmd('GetNextProbe').catch(() => null) : null]);
    // koder nakładki CG – bez CG Playout null (panel ukryty), starszy CG albo błąd = Error (komunikat)
    const enc = S.status?.cg ? await api('/api/cg/live/encoder').then((r) => r.result).catch((e) => e) : null;
    // klatkaż profili w moście – starszy most bez /framerates = null (zostaje przełącznik legacy)
    const fr = await api('/api/framerates').then((r) => r.result).catch(() => null);
    const pr = has('GetNextProbe') ? (probe ?? false) : undefined;
    return { rows, resolutions, supported, fresh, channels, fr, enc, probe: pr, working: workingAccels(pr, supported) };
  },
  id: (p) => p.Id,
  cols: ({ channels, fr, working }) => [
    ['Name', 'Nazwa'], ['Resolution.Name', 'Rozdzielczość'],
    [(p) => `${pl(p.VideoFormat)} · ${pl(p.BitDepth)}`, 'Obraz'], [(p) => (p.VideoBitrate ? p.VideoBitrate + ' kb/s' : '—'), 'Bitrate', { class: 'num' }],
    [(p) => `${pl(p.AudioFormat)} ${p.AudioBitrate ? p.AudioBitrate + ' kb/s' : ''}`, 'Dźwięk'],
    [(p) => (p.HardwareAcceleration === 'None' ? 'programowo'
      : hwMissing(p, working) ? pill(`${pl(p.HardwareAcceleration)} – brak, programowo`, 'warn') : pl(p.HardwareAcceleration)), 'Akceleracja'],
    [(p) => (p.NormalizeLoudnessMode === 'LoudNorm' ? `${p.TargetLoudness ?? '?'} LUFS` : '—'), 'Głośność'],
    [(p) => { const c = rateChoice(p, fr?.active ? fr.rates : null); return c === 'source' ? '—' : c === 'auto' ? 'auto' : fmtRate(c); }, 'Klatki', { class: 'num' }],
    [(p) => { const n = channels.filter((c) => c.FFmpegProfileId === p.Id).length; return n ? pill(String(n), 'info') : '—'; }, 'Kanały'],
  ],
  editor: (p, { resolutions, supported, fresh, probe, fr, working }) => {
    const base = { ...fresh, ...(p || {}), ResolutionId: (p || fresh).Resolution?.Id ?? resolutions[0]?.Id };
    if (!p) base.Name = '';
    // nowy profil na maszynie bez tej akceleracji (domyślna legacy) – od razu programowo
    if (!p && hwMissing(base, working)) base.HardwareAcceleration = 'None';
    const rates = fr?.active;
    if (rates) base.FrameRateChoice = rateChoice(p || fresh, fr.rates);
    const hw = enumValues('CreateFFmpegProfile', 'HardwareAcceleration')
      .filter((k) => k !== 'V4l2m2m')
      .map((k) => [k, k === 'None' ? 'brak (programowo)'
        : Array.isArray(working) ? (working.includes(k) ? pl(k) : `${pl(k)} – brak na tej maszynie (programowo)`)
          : supported.includes(k) ? pl(k) : `${pl(k)} – sprzęt nie zgłasza`]);
    return form(p ? 'UpdateFFmpegProfile' : 'CreateFFmpegProfile', [
      { sec: 'Ogólne' },
      { key: 'Name', label: 'Nazwa', required: true, placeholder: 'np. 720p H.264' },
      { label: 'Normalizacja', group: [{ key: 'NormalizeVideo', kind: 'check', text: 'obrazu' }, { key: 'NormalizeAudio', kind: 'check', text: 'dźwięku' }],
        hint: (v) => (!v.NormalizeVideo || !v.NormalizeAudio ? 'Bez normalizacji Next kopiuje strumień bez zmian (ze znacznikami czasu) – większość odbiorników sobie z tym nie radzi.' : '') },
      { key: 'ResolutionId', label: 'Rozdzielczość', options: resolutions.map((r) => [r.Id, r.Name]) },
      { key: 'ScalingBehavior', label: 'Dopasowanie obrazu', show: vid },
      { sec: 'Obraz' },
      { key: 'VideoFormat', label: 'Kodek', show: vid, hint: (v) => (v.VideoFormat === 'Copy' ? 'Obraz kopiowany bez zmian – zwykle powoduje problemy w odbiornikach.' : '') },
      { key: 'BitDepth', label: 'Głębia', show: vid },
      { key: 'VideoBitrate', label: 'Bitrate', unit: 'kb/s', show: vid },
      { key: 'VideoBufferSize', label: 'Bufor', unit: 'kb', show: vid },
      { key: 'HardwareAcceleration', label: 'Akceleracja sprzętowa', options: hw, show: vid, hint: probe === undefined ? hwHint(supported) : nextHint(probe, supported) },
      { key: 'VaapiDriver', label: 'Sterownik VAAPI', show: (v) => vid(v) && v.HardwareAcceleration === 'Vaapi' },
      { key: 'VaapiDevice', label: 'Urządzenie VAAPI', placeholder: '/dev/dri/renderD128', show: (v) => vid(v) && v.HardwareAcceleration === 'Vaapi' },
      { key: 'TonemapAlgorithm', label: 'Mapowanie HDR → SDR', show: vid },
      rates
        ? { key: 'FrameRateChoice', label: 'Klatkaż', options: FRAME_RATES, optional: false, show: vid, hint: rateHint }
        : null,
      { label: 'Obróbka', group: [
        rates ? null : { key: 'NormalizeFramerate', kind: 'check', text: 'stała liczba klatek' },
        { key: 'DeinterlaceVideo', kind: 'check', text: 'usuwaj przeplot' }].filter(Boolean), show: vid,
        hint: fr && !fr.active ? `Wybór klatkażu niedostępny – most: ${fr.problem}. Zostaje przełącznik legacy (najniższy klatkaż z ramówki).` : null },
      { sec: 'Dźwięk' },
      { key: 'AudioFormat', label: 'Kodek', show: aud, hint: (v) => (v.AudioFormat === 'AacLatm' ? 'AAC LATM tylko dla DVB-C; w pozostałych przypadkach AAC.' : v.AudioFormat === 'Copy' ? 'Dźwięk kopiowany bez zmian – zwykle powoduje problemy w odbiornikach.' : '') },
      { key: 'AudioBitrate', label: 'Bitrate', unit: 'kb/s', show: aud },
      { key: 'AudioBufferSize', label: 'Bufor', unit: 'kb', show: aud },
      { key: 'AudioChannels', label: 'Kanały', show: aud },
      { key: 'AudioSampleRate', label: 'Próbkowanie', unit: 'kHz', show: aud },
      { key: 'NormalizeLoudnessMode', label: 'Normalizacja głośności', show: aud, hint: loudHint },
      { key: 'TargetLoudness', label: 'Poziom docelowy', unit: 'LUFS', step: 0.5, placeholder: '-23', show: (v) => aud(v) && v.NormalizeLoudnessMode === 'LoudNorm' },
    ], base);
  },
  beforeSave: (ctx, d) => {
    if (d.FrameRateChoice !== undefined) d.NormalizeFramerate = d.FrameRateChoice !== 'source';
    if (d.NormalizeAudio && d.NormalizeLoudnessMode === 'LoudNorm' && d.TargetLoudness == null) throw new Error('Pole „Poziom docelowy”: podaj poziom w LUFS (TV w Polsce: −23).');
  },
  footer: (p, { fresh }) => h('div', { class: 'hint pad dim' },
    'Ustawienia tylko dla starego silnika legacy (wątki, preset, profil H.264, B-klatki, wyświetlacz VAAPI) panel zostawia bez zmian; preset i profil H.264 (programowo i sprzętowo) oraz opcje x264 ustawia panel „Koder” niżej',
    p ? '.' : ` (nowy profil: wartości domyślne legacy, np. profil ${fresh.VideoProfile || '—'}).`),
  below: (cur, { resolutions, rows, enc, working }) => [encoderPanel(enc, working), resolutionsPanel(resolutions, rows)],
  afterSave: async (id, { fr }, d) => {
    if (!fr?.active || d.FrameRateChoice === undefined) return;
    const want = rateToBridge(d.FrameRateChoice);
    if (want === (fr.rates[id] ?? null)) return;
    try { await api('/api/framerate/' + id, { method: 'POST', body: JSON.stringify({ rate: want }) }); } catch (e) { throw new Error('Profil zapisany, ale klatkaż nie: ' + e.message); }
  },
  afterDelete: async (p, { fr }) => {
    if (fr?.active && fr.rates[p.Id] != null) await api('/api/framerate/' + p.Id, { method: 'POST', body: JSON.stringify({ rate: null }) });
  },
  create: (d) => d,
  update: (d, p) => ({ ...d, FFmpegProfileId: p.Id }),
  createdId: (r) => r?.FFmpegProfileId,
  del: (p) => ({ FFmpegProfileId: p.Id }),
  confirmDelete: (p, { channels }) => {
    const used = channels.filter((c) => c.FFmpegProfileId === p.Id);
    return used.length
      ? `Profil „${p.Name}” mają kanały: ${used.map((c) => `${c.Number} · ${c.Name}`).join(', ')}. Legacy może odmówić usunięcia. Usunąć?`
      : `Usunąć profil „${p.Name}”? Tego nie da się cofnąć.`;
  },
  copy: (p, name) => ({ FFmpegProfileId: p.Id, Name: name }),
  copiedId: (r) => r?.Id,
});
