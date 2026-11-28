// Widok „Harmonogramy” (grupa Ramówka): ramówka klasyczna – ustawienia harmonogramu i jego pozycje.
// Odpowiednik Schedules + Schedule Editor + Schedule Items Editor w legacy.
// Pozycje zapisuje jedno ReplaceProgramScheduleItems (jak UI legacy); most zwraca wtedy `warning` (DOKUMENTACJA §13).
// Pola pozycji (ReplaceProgramScheduleItem) są typem zagnieżdżonym – most nie podaje ich wyliczeń, więc listy
// wyboru są tutaj, z tymi samymi ograniczeniami co w ScheduleItemsEditor.razor.

import { VIEWS, h, btn, panel, cmd, pl, pill, cmdNote, dialog, toast, route, argsFor, has } from '../core.js';
import { form, parseTimeSpan } from '../form.js';
import { watermarkChoices } from './znaki.js';
import { graphicsChoices } from './grafika.js';
import { cards, countQuery } from './listy.js';
import { crudView, itemsEditor, contentPicker, emptyContent, dayBar, minutesOf, hhmm, span } from '../parts.js';

// ---------- etykiety pól pozycji ----------
const L = {
  Dynamic: 'dynamiczny (po poprzedniej)', Fixed: 'o stałej godzinie',
  Strict: 'ściśle – zawsze czekaj na godzinę', Flexible: 'elastycznie – jak najszybciej po godzinie',
  Flood: 'wypełnij (do następnej stałej godziny)', One: 'jedna pozycja', Multiple: 'kilka pozycji', Duration: 'przez określony czas',
  None: 'brak', Chronological: 'chronologicznie', Random: 'losowo', Shuffle: 'tasuj', ShuffleInOrder: 'tasuj z zachowaniem kolejności',
  MultiEpisodeShuffle: 'tasuj odcinki wieloczęściowe', SeasonEpisode: 'sezon, odcinek', Marathon: 'maraton', RandomRotation: 'losowa rotacja',
  Show: 'serial', Season: 'sezon', Artist: 'artysta', Album: 'album', Director: 'reżyser',
  Count: 'liczba', CollectionSize: 'rozmiar kolekcji', PlaylistItemSize: 'rozmiar pozycji playlisty', MultiEpisodeGroupSize: 'rozmiar grupy odcinków',
  FillWithOrderedGroups: 'grupami po kolei', FillWithShuffledGroups: 'grupami w losowej kolejności',
  Offline: 'offline (pusty obraz)', Slate: 'plansza', Filler: 'wypełniacz', Normal: 'normalnie',
};
const opts = (...keys) => keys.map((k) => [k, L[k] || pl(k)]);

/** Kolejności odtwarzania dozwolone dla rodzaju treści (jak w legacy). */
function playbackOrders(type) {
  switch (type) {
    case 'MultiCollection': return ['Shuffle', 'ShuffleInOrder'];
    case 'Collection': case 'SmartCollection': case 'SearchQuery': return ['Chronological', 'Random', 'Shuffle', 'ShuffleInOrder', 'Marathon'];
    case 'TelevisionShow': return ['Chronological', 'SeasonEpisode', 'Random', 'Shuffle', 'MultiEpisodeShuffle'];
    case 'Playlist': case 'RerunFirstRun': case 'RerunRerun': return ['None'];
    default: return ['Chronological', 'Random', 'Shuffle'];
  }
}
function multipleModes(v) {
  return ['Count', v._content?.CollectionType === 'Playlist' ? 'PlaylistItemSize' : 'CollectionSize', v.PlaybackOrder === 'Chronological' ? 'MultiEpisodeGroupSize' : null].filter(Boolean);
}
const canFillWithGroups = (v) => ['Multiple', 'Duration'].includes(v.PlayoutMode) && v.PlaybackOrder !== 'ShuffleInOrder'
  && ['Collection', 'MultiCollection', 'SmartCollection'].includes(v._content?.CollectionType);

// ---------- pozycja: legacy → edycja → ReplaceProgramScheduleItem ----------
const CONTENT_KEYS = Object.keys(emptyContent());

/** Pozycja z GetProgramScheduleItems (model widoku, podtypy One/Multiple/Duration/Flood) w kształcie ReplaceProgramScheduleItem. */
export function fromLegacy(vm) {
  return {
    StartType: vm.StartType, StartTime: vm.StartTime ?? null, FixedStartTimeBehavior: vm.FixedStartTimeBehavior ?? null,
    PlayoutMode: vm.PlayoutMode, CollectionType: vm.CollectionType,
    CollectionId: vm.Collection?.Id ?? null, MultiCollectionId: vm.MultiCollection?.Id ?? null, SmartCollectionId: vm.SmartCollection?.Id ?? null,
    RerunCollectionId: vm.RerunCollection?.Id ?? null, MediaItemId: vm.MediaItem?.MediaItemId ?? null, PlaylistId: vm.Playlist?.Id ?? null,
    SearchTitle: vm.SearchTitle ?? '', SearchQuery: vm.SearchQuery ?? '',
    PlaybackOrder: vm.PlaybackOrder, MarathonGroupBy: vm.MarathonGroupBy ?? 'None', MarathonShuffleGroups: !!vm.MarathonShuffleGroups,
    MarathonShuffleItems: !!vm.MarathonShuffleItems, MarathonBatchSize: vm.MarathonBatchSize ?? null, FillWithGroupMode: vm.FillWithGroupMode ?? 'None',
    MultipleMode: vm.MultipleMode ?? 'Count', MultipleCount: vm.Count ?? '1',
    PlayoutDuration: vm.PlayoutDuration ?? null, TailMode: vm.TailMode ?? 'None', DiscardToFillAttempts: vm.DiscardToFillAttempts ?? 0,
    CustomTitle: vm.CustomTitle ?? '', GuideMode: vm.GuideMode ?? 'Normal',
    PreRollFillerId: vm.PreRollFiller?.Id ?? null, MidRollFillerId: vm.MidRollFiller?.Id ?? null, PostRollFillerId: vm.PostRollFiller?.Id ?? null,
    TailFillerId: vm.TailFiller?.Id ?? null, FallbackFillerId: vm.FallbackFiller?.Id ?? null,
    WatermarkIds: (vm.Watermarks || []).map((w) => w.Id), GraphicsElementIds: (vm.GraphicsElements || []).map((g) => g.Id),
    PreferredAudioLanguageCode: vm.PreferredAudioLanguageCode ?? null, PreferredAudioTitle: vm.PreferredAudioTitle ?? '',
    PreferredSubtitleLanguageCode: vm.PreferredSubtitleLanguageCode ?? null, SubtitleMode: vm.SubtitleMode ?? null,
    _name: vm.Name ? `${pl(vm.CollectionType)}: ${vm.Name}` : pl(vm.CollectionType),
  };
}

/** Nowa pozycja – wartości jak w legacy (AddScheduleItem). */
const newItem = () => ({
  ...fromLegacy({ StartType: 'Dynamic', PlayoutMode: 'One', PlaybackOrder: 'Shuffle', CollectionType: 'Collection' }), _name: '—',
});

/** Pozycja do ReplaceProgramScheduleItems: wszystkie pola (most nie uzupełnia pól typów zagnieżdżonych), pola ukryte wyczyszczone. */
export function toReplace(it, index) {
  const out = { Index: index };
  for (const [k, v] of Object.entries(it)) if (!k.startsWith('_')) out[k] = v;
  if (out.StartType !== 'Fixed') { out.StartTime = null; out.FixedStartTimeBehavior = null; }
  if (out.PlayoutMode !== 'Duration') out.PlayoutDuration = null;
  out.WatermarkIds ??= [];
  out.GraphicsElementIds ??= [];
  return out;
}

// ---------- opis pozycji w tabeli ----------
const BEHAVIOR = { Strict: 'ściśle', Flexible: 'elastycznie' };
/** „06:00 stała” (z nadpisaniem zachowania: „06:00 ściśle”) albo „po poprzedniej” – jak w makiecie UI. */
const startText = (it) => (it.StartType === 'Fixed'
  ? `${(it.StartTime || '').slice(0, 5) || '?'} ${BEHAVIOR[it.FixedStartTimeBehavior] || 'stała'}` : 'po poprzedniej');

/** Wypełniacze pozycji w skrócie: „przed: Reklamy · po: Zapowiedzi”. */
const FILLER_SLOTS = [['PreRollFillerId', 'przed'], ['MidRollFillerId', 'w trakcie'], ['PostRollFillerId', 'po'], ['TailFillerId', 'ogon'], ['FallbackFillerId', 'zapas']];
function fillerText(it, fillers) {
  const name = (id) => fillers.find((x) => x.Id === id)?.Name || `#${id}`;
  return FILLER_SLOTS.filter(([k]) => it[k] != null).map(([k, t]) => `${t}: ${name(it[k])}`).join(' · ');
}

/** Znaki wodne i elementy graficzne pozycji jako etykiety (puste = „—”). */
function graphicsCell(it, ctx) {
  const byId = (list, id) => list.find((x) => x.Id === id)?.Name || `#${id}`;
  const tags = [
    ...(it.WatermarkIds || []).map((id) => pill('znak: ' + byId(ctx.watermarks, id))),
    ...(it.GraphicsElementIds || []).map((id) => pill(byId(ctx.graphics, id), 'info')),
  ];
  return tags.length ? h('span', { class: 'bar' }, tags) : null;
}

/**
 * Podgląd 24 h: szkielet doby z pozycji o stałej godzinie – każda trwa do następnej stałej godziny, razem z pozycjami
 * „po poprzedniej” za nią. Dokładne godziny liczy legacy przy budowie playoutu (długości plików), panel ich nie zna.
 */
function dayPreview(items) {
  const fixed = items.map((it, i) => ({ it, i })).filter(({ it }) => it.StartType === 'Fixed' && it.StartTime);
  if (!fixed.length) {
    return h('p', { class: 'pad dim' }, items.length
      ? 'Brak pozycji o stałej godzinie – legacy układa pozycje po kolei, w pętli, bez punktów zaczepienia w dobie.'
      : 'Podgląd pojawi się po dodaniu pozycji.');
  }
  const sorted = [...fixed].sort((a, b) => minutesOf(a.it.StartTime) - minutesOf(b.it.StartTime));
  const segs = sorted.map(({ it, i }, n) => {
    const start = minutesOf(it.StartTime);
    let end = n + 1 < sorted.length ? minutesOf(sorted[n + 1].it.StartTime) : minutesOf(sorted[0].it.StartTime) + 1440;
    if (end <= start) end += 1440;
    // pozycje „po poprzedniej” idą za tą stałą pozycją aż do następnej stałej (w kolejności z tabeli, z zawinięciem)
    const tail = [];
    for (let j = (i + 1) % items.length; j !== i && items[j].StartType !== 'Fixed'; j = (j + 1) % items.length) tail.push(items[j]);
    const label = it._name.replace(/^[^:]*: /, '') + (tail.length ? ` +${tail.length}` : '');
    const title = `${hhmm(start)}–${hhmm(end % 1440)} ${[it, ...tail].map((x) => x._name).join(' → ')}`;
    return { start, end, label, title };
  });
  return dayBar(segs, { label: 'Podgląd doby: pozycje o stałej godzinie' });
}
function modeText(it) {
  switch (it.PlayoutMode) {
    case 'Multiple': return it.MultipleMode === 'Count' ? `kilka (${it.MultipleCount})` : `kilka: ${L[it.MultipleMode]}`;
    case 'Duration': return `przez ${it.PlayoutDuration || '?'}`;
    case 'Flood': return 'wypełnij';
    case 'One': return 'jedna';
    default: return pl(it.PlayoutMode);
  }
}

// ---------- edytor jednej pozycji ----------
/** Lista wyboru, której wartości zależą od innych pól (np. kolejność od rodzaju treści); fill() przelicza listę. */
function dynSelect(id, label, value, choices, current) {
  const el = h('select', { id, 'aria-label': label });
  let last = value;
  const fill = () => {
    const list = choices(current());
    el.replaceChildren(...list.map((k) => h('option', { value: k }, L[k] || pl(k))));
    el.value = list.includes(last) ? last : list[0];
    last = el.value;
  };
  el.addEventListener('change', () => { last = el.value; });
  return { el, fill, ctl: { el, read: () => el.value, peek: () => el.value || last } };
}

function itemEditor(item, ctx, shuffle) {
  const root = h('div', { class: 'si-form' });
  const cp = contentPicker(item, { idPrefix: 'si-cp-', label: item._name?.replace(/^[^:]*: /, '') });
  let f = null;
  const current = () => (f ? f.peek() : { ...item, _content: cp.peek() });
  const order = dynSelect('si-PlaybackOrder', 'Kolejność odtwarzania', item.PlaybackOrder, (v) => playbackOrders(v._content?.CollectionType), current);
  const multi = dynSelect('si-MultipleMode', 'Liczba pozycji wg', item.MultipleMode, multipleModes, current);
  const fillers = (kind) => ctx.fillers.filter((x) => x.FillerKind === kind).map((x) => [x.Id, x.Name]);
  const langs = ctx.languages.map((x) => [x.ThreeLetterISOLanguageName, `${x.EnglishName} (${x.ThreeLetterISOLanguageName})`]);
  const is = (k, ...vals) => (v) => vals.includes(v[k]);
  const values = { ...item, _content: cp.peek(), _playlistShuffle: item.MarathonShuffleGroups };

  f = form('', [
    { sec: 'Start' },
    { key: 'StartType', label: 'Start', options: shuffle ? opts('Dynamic') : opts('Dynamic', 'Fixed'), optional: false,
      hint: shuffle ? 'Harmonogram tasuje pozycje – stała godzina niedostępna (jak w legacy).' : null },
    { key: 'StartTime', label: 'Godzina startu', kind: 'time', optional: false, placeholder: 'gg:mm', show: is('StartType', 'Fixed') },
    { key: 'FixedStartTimeBehavior', label: 'Godzina stała', options: opts('Strict', 'Flexible'), none: 'jak w harmonogramie', show: is('StartType', 'Fixed') },
    { sec: 'Treść' },
    { key: '_content', label: 'Treść', render: () => cp },
    { key: 'PlaybackOrder', label: 'Kolejność odtwarzania', render: () => order.ctl, hint: 'Lista zależy od rodzaju treści. Ta sama treść w kilku pozycjach musi mieć jedną kolejność.' },
    { key: 'MarathonGroupBy', label: 'Maraton: grupuj wg', options: opts('None', 'Show', 'Season', 'Artist', 'Album', 'Director'), optional: false, show: is('PlaybackOrder', 'Marathon') },
    { label: 'Maraton: tasowanie', show: is('PlaybackOrder', 'Marathon'), group: [
      { key: 'MarathonShuffleGroups', kind: 'check', text: 'tasuj grupy' }, { key: 'MarathonShuffleItems', kind: 'check', text: 'tasuj w grupie' }] },
    { key: 'MarathonBatchSize', label: 'Maraton: porcja', kind: 'number', step: 1, show: is('PlaybackOrder', 'Marathon'), hint: 'Ile pozycji z grupy przed przejściem dalej; puste lub 0 = wszystkie.' },
    { key: '_playlistShuffle', label: 'Playlista', kind: 'check', text: 'tasuj pozycje playlisty', show: (v) => v._content?.CollectionType === 'Playlist' },
    { sec: 'Tryb' },
    { key: 'PlayoutMode', label: 'Tryb', options: shuffle ? opts('One', 'Multiple', 'Duration') : opts('Flood', 'One', 'Multiple', 'Duration'), optional: false },
    { key: 'MultipleMode', label: 'Liczba pozycji wg', render: () => multi.ctl, show: is('PlayoutMode', 'Multiple') },
    { key: 'MultipleCount', label: 'Liczba', required: true, show: (v) => v.PlayoutMode === 'Multiple' && v.MultipleMode === 'Count',
      hint: 'Liczba albo wyrażenie z count (rozmiar kolekcji) i random (0…count−1).' },
    { key: 'PlayoutDuration', label: 'Czas', kind: 'time', optional: false, placeholder: 'gg:mm', show: is('PlayoutMode', 'Duration'), hint: 'Od 1 minuty do 24 godzin.' },
    { key: 'TailMode', label: 'Resztę czasu', options: opts('None', 'Offline', 'Filler'), optional: false, show: is('PlayoutMode', 'Duration') },
    { key: 'DiscardToFillAttempts', label: 'Próby dopasowania', kind: 'number', step: 1, optional: false, show: is('PlayoutMode', 'Duration'),
      hint: 'Ile za długich pozycji pominąć, szukając pasującej do pozostałego czasu.' },
    { key: 'FillWithGroupMode', label: 'Wypełniaj grupami', options: opts('None', 'FillWithOrderedGroups', 'FillWithShuffledGroups'), optional: false, show: canFillWithGroups },
    { sec: 'Przewodnik' },
    { key: 'CustomTitle', label: 'Tytuł własny', placeholder: 'zamiast tytułów pozycji w EPG' },
    { key: 'GuideMode', label: 'W przewodniku', options: [['Normal', 'normalnie'], ['Filler', 'jako wypełniacz (ukryte)']], optional: false },
    { sec: 'Wypełniacze' },
    { key: 'PreRollFillerId', label: 'Przed (pre-roll)', options: fillers('PreRoll'), none: 'brak' },
    { key: 'MidRollFillerId', label: 'W trakcie (mid-roll)', options: fillers('MidRoll'), none: 'brak' },
    { key: 'PostRollFillerId', label: 'Po (post-roll)', options: fillers('PostRoll'), none: 'brak' },
    { key: 'TailFillerId', label: 'Ogon', options: fillers('Tail'), none: 'brak', hint: 'Tylko w trybie „przez określony czas” z resztą czasu = wypełniacz.' },
    { key: 'FallbackFillerId', label: 'Zapasowy', options: fillers('Fallback'), none: 'brak' },
    { sec: 'Znaki wodne i grafika' },
    { key: 'WatermarkIds', label: 'Znaki wodne', kind: 'multi', options: watermarkChoices(ctx.watermarks), empty: 'brak znaków wodnych w legacy' },
    { key: 'GraphicsElementIds', label: 'Elementy graficzne', kind: 'multi', options: graphicsChoices(ctx.graphics), empty: 'brak elementów graficznych w legacy' },
    { sec: 'Audio i napisy' },
    { key: 'PreferredAudioLanguageCode', label: 'Język audio', options: langs, none: 'jak w kanale' },
    { key: 'PreferredAudioTitle', label: 'Tytuł ścieżki audio' },
    { key: 'PreferredSubtitleLanguageCode', label: 'Język napisów', options: langs, none: 'jak w kanale' },
    { key: 'SubtitleMode', label: 'Napisy', options: opts('None', 'Forced', 'Default', 'Any'), none: 'jak w kanale' },
  ], values, { idPrefix: 'si-' });
  // listy zależne po każdej zmianie (już po obsłudze zdarzenia w polu), potem ponownie pola warunkowe formularza
  const deps = (e) => {
    if (e?.target !== order.el) order.fill();
    if (e?.target !== multi.el) multi.fill();
    f.el.dispatchEvent(new Event('input'));
  };
  root.addEventListener('change', deps);
  root.append(f.el);
  deps();

  return {
    el: root,
    read() {
      const v = f.read();
      const out = { ...item, ...v, ...v._content, _name: cp.label() };
      if (out.CollectionType === 'Playlist') out.MarathonShuffleGroups = !!v._playlistShuffle;
      if (!canFillWithGroups(v)) out.FillWithGroupMode = 'None';
      delete out._content;
      delete out._playlistShuffle;
      for (const k of CONTENT_KEYS) out[k] ??= emptyContent()[k];
      return out;
    },
  };
}


// ---------- K25: kreator szablonu dnia ----------
// Bloki godzinowe (jak szablon generatora ffplayout) → pozycje harmonogramu klasycznego: stała godzina startu bloku,
// tryb „przez określony czas”, losowo albo po kolei, resztę czasu wypełniacz ogona (PLAN-mcr §5, K25).
const WIZ_KINDS = ['Collection', 'SmartCollection', 'SearchQuery'];
const WIZ_ORDERS = [['Shuffle', 'losowo (tasuj)'], ['Chronological', 'po kolei']];

/** Czas bloku jako TimeSpan legacy: „02:30:00”; pełna doba „1.00:00:00” (TimeSpan nie przyjmuje „24:00:00”). */
const durSpan = (min) => (min >= 1440 ? '1.00:00:00' : span(min));
/** „gg:mm” (godzina 0–23) → minuty od północy albo null. */
const parseStart = (s) => { const t = parseTimeSpan(String(s || '').trim()); return t && !t.includes('.') ? minutesOf(t) : null; };
/** Czas bloku „gg:mm” (do 24:00) albo TimeSpan legacy („1.00:00:00”) → minuty albo null. */
function parseDur(s) {
  const v = String(s || '').trim();
  const m = /^(?:(\d+)\.)?(\d{1,2}):(\d{2})(?::(\d{2}))?$/.exec(v);
  if (!m || +m[3] > 59 || (m[4] && +m[4] > 0)) return null;
  const min = (+(m[1] || 0) * 24 + +m[2]) * 60 + +m[3];
  return min >= 1 && min <= 1440 ? min : null;
}
const hasSource = (c) => (c?.CollectionType === 'SearchQuery' ? !!c.SearchQuery : c?.CollectionId != null || c?.SmartCollectionId != null);
/** Klucz treści jak CollectionKey w legacy: ta sama treść w kilku pozycjach musi mieć jedną kolejność odtwarzania. */
const sourceKey = (c) => [c.CollectionType, c.CollectionId, c.SmartCollectionId, c.CollectionType === 'SearchQuery' ? c.SearchQuery : ''].join('|');

/**
 * Kontrola bloków przed zapisem: blocks = [{ start, dur (minuty albo null), content (pola treści), order, label }] w kolejności wierszy.
 * Błędy blokują zapis (brak pól, źródła, nakładanie, kolejność godzin – najwyżej jedno przejście przez północ, dwie kolejności
 * jednej treści); ostrzeżenia nie (luki, niepełna doba). Zwraca { errors, warnings, total (minuty), abs (starty bezwzględne) }.
 */
export function checkBlocks(blocks) {
  const errors = [], warnings = [];
  const n = (i) => `Blok ${i + 1}`;
  if (!blocks.length) errors.push('Dodaj co najmniej jeden blok.');
  blocks.forEach((b, i) => {
    if (b.start == null) errors.push(`${n(i)}: podaj godzinę startu gg:mm (00:00–23:59).`);
    if (b.dur == null) errors.push(`${n(i)}: podaj czas bloku gg:mm (od 00:01 do 24:00).`);
    if (!hasSource(b.content)) errors.push(`${n(i)}: wybierz źródło (kolekcję, inteligentną kolekcję albo zapytanie wyszukiwania).`);
  });
  if (errors.length) return { errors, warnings, total: 0, abs: [] };
  const orders = new Map();
  for (const [i, b] of blocks.entries()) {
    const k = sourceKey(b.content);
    if (orders.has(k) && orders.get(k).order !== b.order) {
      errors.push(`${n(orders.get(k).i)} i ${n(i)}: to samo źródło (${b.label}) raz losowo, raz po kolei – legacy wymaga jednej kolejności dla tej samej treści.`);
    } else if (!orders.has(k)) orders.set(k, { order: b.order, i });
  }
  // starty bezwzględne: przejście przez północ dokłada dobę; więcej niż doba od pierwszego bloku = bloki nie po kolei
  const abs = [];
  let off = 0;
  blocks.forEach((b, i) => { if (i && b.start + off < abs[i - 1]) off += 1440; abs.push(b.start + off); });
  const span24 = (a, b) => `${hhmm(a)}–${hhmm(b)}`;
  for (let i = 1; i < blocks.length; i++) {
    const end = abs[i - 1] + blocks[i - 1].dur;
    if (abs[i] - abs[0] >= 1440) {
      errors.push(`${n(i)} (${hhmm(abs[i])}) nie jest po kolei – ułóż bloki w kolejności godzin (najwyżej jedno przejście przez północ).`);
      return { errors, warnings, total: 0, abs };
    }
    if (abs[i] < end) errors.push(`${n(i)} (start ${hhmm(abs[i])}) nakłada się na ${n(i - 1)} (${span24(abs[i - 1], end)}).`);
    else if (abs[i] > end) warnings.push(`Luka ${span24(end, abs[i])} między blokiem ${i} a ${i + 1}.`);
  }
  const last = blocks.length - 1;
  const endLast = abs[last] + blocks[last].dur;
  if (endLast > abs[0] + 1440) errors.push(`${n(last)} (${span24(abs[last], endLast)}) nakłada się na ${n(0)} następnego dnia (start ${hhmm(abs[0])}).`);
  else if (endLast < abs[0] + 1440) warnings.push(`Luka ${span24(endLast, abs[0])} ${blocks.length > 1 ? `między blokiem ${last + 1} a 1 (następnego dnia)` : 'do startu bloku następnego dnia'}.`);
  const total = blocks.reduce((t, b) => t + b.dur, 0);
  if (!errors.length && total < 1440) {
    warnings.push(`Bloki dają razem ${span(total).slice(0, 5)} z 24:00 – niepełna doba. W lukach legacy pokaże wypełniacz zapasowy albo planszę offline.`);
  }
  return { errors, warnings, total, abs };
}

/** Bloki → pozycje harmonogramu (kształt edytora pozycji; do zapisu przez toReplace). Zachowanie godziny stałej – z harmonogramu. */
export function blocksToItems(blocks) {
  return blocks.map((b) => ({
    ...fromLegacy({ StartType: 'Fixed', PlayoutMode: 'Duration', PlaybackOrder: b.order, CollectionType: b.content.CollectionType }),
    ...emptyContent(b.content.CollectionType), ...b.content,
    StartTime: span(b.start), FixedStartTimeBehavior: null, PlayoutDuration: durSpan(b.dur),
    TailMode: b.tail != null ? 'Filler' : 'Offline', TailFillerId: b.tail ?? null,
    _name: b.label,
  }));
}

/** Pozycje harmonogramu, które kreator umie pokazać jako bloki (stała godzina + „przez określony czas” + kolekcja / inteligentna / zapytanie). */
export function blocksFromItems(items) {
  return items.filter((it) => it.StartType === 'Fixed' && it.StartTime && it.PlayoutMode === 'Duration' && WIZ_KINDS.includes(it.CollectionType)
    && ['Shuffle', 'Chronological'].includes(it.PlaybackOrder)).map((it) => ({
    start: minutesOf(it.StartTime), dur: parseDur(it.PlayoutDuration), order: it.PlaybackOrder,
    tail: it.TailMode === 'Filler' ? it.TailFillerId : null, label: it._name,
    content: Object.fromEntries(Object.keys(emptyContent()).map((k) => [k, it[k]])),
  }));
}

/** Czy źródła bloków mają pozycje (pusta kolekcja zatrzymuje budowę playoutu). Zwraca { errors, warnings }. */
async function sourceSizes(blocks) {
  const errors = [], warnings = [];
  const seen = new Map();
  let smart = null;
  for (const b of blocks) {
    const c = b.content;
    const k = sourceKey(c);
    if (seen.has(k)) continue;
    seen.set(k, true);
    try {
      let count = null;
      if (c.CollectionType === 'Collection' && has('GetCollectionCards')) count = cards(await cmd('GetCollectionCards', { Id: c.CollectionId })).length;
      else if (c.CollectionType === 'SmartCollection' && has('GetAllSmartCollections') && has('QuerySearchIndexAllItems')) {
        smart ??= await cmd('GetAllSmartCollections');
        const q = smart.find((x) => x.Id === c.SmartCollectionId)?.Query;
        if (q == null) { errors.push(`Źródło „${b.label}”: nie ma takiej inteligentnej kolekcji w legacy.`); continue; }
        count = await countQuery(q);
      } else if (c.CollectionType === 'SearchQuery' && has('QuerySearchIndexAllItems')) count = await countQuery(c.SearchQuery);
      if (count === 0) errors.push(`Źródło „${b.label}” jest puste – pusta treść zatrzymuje budowę playoutu.`);
      else if (count == null) warnings.push(`Źródło „${b.label}”: nie sprawdzono, czy ma pozycje.`);
    } catch (e) { warnings.push(`Źródło „${b.label}”: nie sprawdzono, czy ma pozycje (${e.message}).`); }
  }
  return { errors, warnings };
}

/** Jeden wiersz kreatora: start · czas · źródło · kolejność · wypełniacz ogona. */
let wizSeq = 0;
function wizardRow(b, tails, onRemove) {
  const k = ++wizSeq;
  const start = h('input', { id: `kw${k}-start`, class: 'num', value: b?.start != null ? hhmm(b.start) : '', placeholder: 'gg:mm', inputmode: 'numeric', 'aria-label': 'Start bloku' });
  const dur = h('input', { id: `kw${k}-dur`, class: 'num', value: b?.dur != null ? span(b.dur).slice(0, 5) : '', placeholder: 'gg:mm', inputmode: 'numeric', 'aria-label': 'Czas bloku' });
  const cp = contentPicker(b?.content || emptyContent('SmartCollection'), { kinds: WIZ_KINDS, idPrefix: `kw${k}-cp-`, label: b?.label?.replace(/^[^:]*: /, '') });
  const order = h('select', { id: `kw${k}-order`, 'aria-label': 'Kolejność' }, WIZ_ORDERS.map(([v, t]) => h('option', { value: v }, t)));
  order.value = b?.order || 'Shuffle';
  const tail = h('select', { id: `kw${k}-tail`, 'aria-label': 'Wypełniacz ogona' }, h('option', { value: '' }, 'brak – plansza offline'),
    tails.map((x) => h('option', { value: String(x.Id) }, x.Name)));
  tail.value = b?.tail != null && tails.some((x) => x.Id === b.tail) ? String(b.tail) : (b ? '' : tails[0] ? String(tails[0].Id) : '');
  const del = h('button', { type: 'button', class: 'danger', title: 'Usuń blok', 'aria-label': 'Usuń blok', onclick: () => onRemove(row) }, '✕');
  const el = h('div', { class: 'kw-row' }, h('span', { class: 'kw-n dim' }), start, dur, cp.el, order, tail, del);
  const row = {
    el,
    read: () => ({ start: parseStart(start.value), dur: parseDur(dur.value), content: cp.peek(), label: cp.label(), order: order.value, tail: tail.value ? Number(tail.value) : null }),
  };
  return row;
}

/** Okno „Kreator dnia…”: bloki → pozycje wybranego harmonogramu (zastąpienie po podglądzie) albo nowego. */
async function dayWizard(ctx) {
  const cur = ctx.current || null;
  const tails = ctx.fillers.filter((x) => x.FillerKind === 'Tail');
  const target = h('select', { id: 'kw-target', 'aria-label': 'Zapis do' },
    cur ? h('option', { value: 'cur' }, `wybrany harmonogram „${cur.Name}” – zastąp pozycje (${ctx.items.length})`) : null,
    h('option', { value: 'new' }, 'nowy harmonogram'));
  const name = h('input', { id: 'kw-name', placeholder: 'np. INFO24 – dzień powszedni' });
  const behavior = h('select', { id: 'kw-behavior', 'aria-label': 'Godziny stałe' }, opts('Strict', 'Flexible').map(([v, t]) => h('option', { value: v }, t)));
  const newOnly = [h('label', { for: 'kw-name' }, 'Nazwa'), name, h('label', { for: 'kw-behavior' }, 'Godziny stałe'), behavior];
  const syncTarget = () => newOnly.forEach((x) => { x.hidden = target.value !== 'new'; });
  target.addEventListener('change', syncTarget);

  const rows = [];
  const list = h('div', { class: 'kw-rows' });
  const check = h('div', { class: 'kw-check', 'aria-live': 'polite' });
  const renum = () => rows.forEach((r, i) => { r.el.querySelector('.kw-n').textContent = String(i + 1); });
  const remove = (r) => { rows.splice(rows.indexOf(r), 1); r.el.remove(); renum(); refresh(); };
  const add = (b) => {
    const r = wizardRow(b, tails, remove);
    rows.push(r); list.append(r.el); renum();
  };
  const addNext = () => {
    // następny blok zaczyna się tam, gdzie kończy się ostatni
    const last = rows.length ? rows[rows.length - 1].read() : null;
    add(last && last.start != null && last.dur != null ? { start: (last.start + last.dur) % 1440, dur: null, order: 'Shuffle', tail: last.tail, content: emptyContent('SmartCollection') } : null);
    refresh();
  };
  const read = () => rows.map((r) => r.read());
  const bar = (res, blocks) => (res.abs.length
    ? dayBar(res.abs.map((a, i) => ({ start: a % 1440, end: a % 1440 + blocks[i].dur, label: blocks[i].label.replace(/^[^:]*: /, '') })), { label: 'Podgląd doby: bloki kreatora' }) : '');
  const showCheck = (res, extra = { errors: [], warnings: [] }) => {
    const errors = [...res.errors, ...extra.errors], warnings = [...res.warnings, ...extra.warnings];
    const blocks = read();
    check.replaceChildren(
      bar(res, blocks),
      errors.length ? h('div', { class: 'note err' }, h('b', null, 'Do poprawy (zapis zablokowany):'), h('ul', null, errors.map((t) => h('li', null, t)))) : '',
      warnings.length ? h('div', { class: 'note' }, h('b', null, 'Ostrzeżenia:'), h('ul', null, warnings.map((t) => h('li', null, t)))) : '',
      !errors.length && !warnings.length && blocks.length ? h('p', { class: 'dim' }, 'Bloki po kolei, bez nakładania, pełna doba.') : '');
  };
  let timer = null;
  const refresh = () => { clearTimeout(timer); timer = setTimeout(() => showCheck(checkBlocks(read())), 150); };
  list.addEventListener('input', refresh);
  list.addEventListener('change', refresh);

  const fromItems = cur ? blocksFromItems(ctx.items) : [];
  if (fromItems.length) fromItems.forEach(add);
  else add(null);
  syncTarget();
  showCheck(checkBlocks(read()));

  const body = h('div', { class: 'kw' },
    h('div', { class: 'form' }, h('label', { for: 'kw-target' }, 'Zapis do'), target, ...newOnly),
    h('div', { class: 'kw-head dim', 'aria-hidden': 'true' }, h('span', null, '#'), h('span', null, 'Start'), h('span', null, 'Czas'), h('span', null, 'Źródło'),
      h('span', null, 'Kolejność'), h('span', null, 'Wypełniacz ogona'), h('span', null, '')),
    list,
    h('div', { class: 'bar' }, btn('+ Dodaj blok', addNext)),
    tails.length ? null : h('div', { class: 'hint' }, 'W legacy nie ma wypełniacza rodzaju „ogon” – resztę czasu bloku pokaże plansza offline. Wypełniacz załóż w Wypełniaczach.'),
    h('div', { class: 'hint' }, 'Każdy blok to pozycja o stałej godzinie w trybie „przez określony czas”; godzinę stałą (ściśle / elastycznie) bierze z harmonogramu. Po północy bloki mogą przejść raz na następny dzień.'),
    check);

  for (;;) {
    if (!(await dialog('Kreator dnia', body, [['Anuluj', false], ['Dalej', true, 'acc']]))) return;
    const blocks = read();
    const res = checkBlocks(blocks);
    const extra = { errors: [], warnings: [] };
    const replace = target.value === 'cur' && cur;
    if (!replace && !name.value.trim()) extra.errors.push('Podaj nazwę nowego harmonogramu.');
    if (!replace && ctx.rows.some((s) => s.Name === name.value.trim())) extra.errors.push(`Harmonogram „${name.value.trim()}” już jest – nazwy w legacy są unikalne.`);
    if (replace && cur.ShuffleScheduleItems) extra.errors.push('Wybrany harmonogram tasuje pozycje – stałe godziny bloków są wtedy niedostępne (jak w legacy). Wyłącz tasowanie albo zapisz do nowego.');
    if (!res.errors.length) {
      check.replaceChildren(h('p', { class: 'dim' }, 'Sprawdzam źródła w legacy…'));
      const sz = await sourceSizes(blocks);
      extra.errors.push(...sz.errors);
      extra.warnings.push(...sz.warnings);
    }
    showCheck(res, extra);
    if (res.errors.length || extra.errors.length) continue;

    const items = blocksToItems(blocks);
    const fillerName = (id) => ctx.fillers.find((x) => x.Id === id)?.Name || `#${id}`;
    const preview = h('div', { class: 'kw-prev' },
      bar(res, blocks),
      h('div', { class: 'tw' }, h('table', null,
        h('thead', null, h('tr', null, ['#', 'Start', 'Czas', 'Treść', 'Kolejność', 'Resztę czasu'].map((t) => h('th', null, t)))),
        h('tbody', null, items.map((it, i) => h('tr', null, h('td', { class: 'num dim' }, String(i + 1)), h('td', { class: 'num' }, it.StartTime.slice(0, 5)),
          h('td', { class: 'num' }, span(blocks[i].dur).slice(0, 5)), h('td', null, it._name), h('td', null, L[it.PlaybackOrder]),
          h('td', null, it.TailFillerId != null ? `wypełniacz: ${fillerName(it.TailFillerId)}` : 'plansza offline')))))),
      [...res.warnings, ...extra.warnings].length ? h('div', { class: 'note' }, h('b', null, 'Ostrzeżenia:'), h('ul', null, [...res.warnings, ...extra.warnings].map((t) => h('li', null, t)))) : '',
      replace ? h('div', { class: 'note err' }, `Obecne pozycje harmonogramu „${cur.Name}” (${ctx.items.length}) zostaną zastąpione`,
        ctx.items.length ? h('ul', null, ctx.items.map((it) => h('li', null, `${startText(it)} · ${it._name} · ${modeText(it)}`))) : '.',
        usedBy(cur, ctx.playouts).length ? h('p', null, 'Zmiana wejdzie na antenę kanałów: ', usedBy(cur, ctx.playouts).map((p) => `${p.ChannelNumber} · ${p.ChannelName}`).join(', '), ' – legacy przebuduje ich playouty sam po zapisie (Refresh, postęp seriali zostaje).') : null,
        h('p', null, 'Niezapisane zmiany w edytorze harmonogramu przepadną.')) : null);
    const go = await dialog(replace ? `Zastąpić pozycje „${cur.Name}”?` : `Założyć harmonogram „${name.value.trim()}”?`, preview,
      [['Wróć', 'back'], [replace ? 'Zastąp pozycje' : 'Załóż harmonogram', true, replace ? 'danger' : 'acc']]);
    if (go === 'back') continue;
    if (!go) return;

    let id = replace ? cur.Id : null;
    try {
      if (!replace) {
        const r = await cmd('CreateProgramSchedule', argsFor('CreateProgramSchedule', { Name: name.value.trim(), FixedStartTimeBehavior: behavior.value,
          KeepMultiPartEpisodesTogether: false, TreatCollectionsAsShows: false, ShuffleScheduleItems: false, RandomStartPoint: false }));
        id = r?.ProgramScheduleId ?? null;
        if (id == null) throw new Error('legacy nie podał id nowego harmonogramu');
      }
    } catch (e) { return toast('Legacy odrzucił założenie harmonogramu: ' + e.message, 'bad'); }
    try {
      await cmd('ReplaceProgramScheduleItems', { ProgramScheduleId: id, Items: items.map((it, i) => toReplace(it, i + 1)) });
      toast(`${replace ? cur.Name : name.value.trim()}: zapisano pozycje z kreatora (${items.length}).`, 'ok');
    } catch (e) {
      toast((replace ? 'Legacy odrzucił pozycje harmonogramu: ' : 'Harmonogram założony, ale legacy odrzucił pozycje: ') + e.message, 'bad');
    }
    VIEWS.harmonogramy.select(id);
    route(true);
    return;
  }
}

// ---------- ekran ----------
VIEWS.harmonogramy = crudView({
  title: 'Harmonogramy',
  desc: 'Ramówka klasyczna: pozycje po kolei, z godzinami stałymi. Odpowiednik Schedules i Schedule Items Editor w legacy; zapis idzie wprost do legacy.',
  listCmd: 'GetAllProgramSchedules',
  needs: ['GetProgramScheduleItems', 'ReplaceProgramScheduleItems', 'GetAllFillerPresets', 'GetAllWatermarks', 'GetAllGraphicsElements', 'GetAllLanguageCodes', 'GetPagedPlayouts'],
  commands: { create: 'CreateProgramSchedule', update: 'UpdateProgramSchedule', delete: 'DeleteProgramSchedule', copy: 'CopyProgramSchedule' },
  noun: { list: 'Harmonogramy', new: 'Nowy harmonogram', add: '+ Nowy harmonogram', create: 'Załóż harmonogram', one: (s) => s.Name, title: (s) => s.Name },
  load: async () => {
    const [rows, fillers, watermarks, graphics, languages, playouts] = await Promise.all([
      cmd('GetAllProgramSchedules'), cmd('GetAllFillerPresets'), cmd('GetAllWatermarks'), cmd('GetAllGraphicsElements'),
      cmd('GetAllLanguageCodes'), cmd('GetPagedPlayouts', { Query: '', PageNum: 0, PageSize: 500 })]);
    rows.sort((a, b) => a.Name.localeCompare(b.Name, 'pl'));
    return { rows, fillers, watermarks, graphics, languages, playouts: playouts?.Page || [] };
  },
  loadOne: async (s) => ({ current: s, items: s ? (await cmd('GetProgramScheduleItems', { Id: s.Id })).sort((a, b) => a.Index - b.Index).map(fromLegacy) : [] }),
  headExtra: (ctx) => btn('Kreator dnia…', () => dayWizard(ctx)),
  id: (s) => s.Id,
  cols: ({ playouts }) => [
    ['Name', 'Nazwa'], [(s) => pl(s.FixedStartTimeBehavior), 'Godziny stałe'], ['ShuffleScheduleItems', 'Tasuj'],
    [(s) => usedBy(s, playouts).map((p) => p.ChannelNumber).join(', ') || '—', 'Kanały'],
  ],
  editor: (s, ctx) => {
    const base = { Name: '', KeepMultiPartEpisodesTogether: false, TreatCollectionsAsShows: false, ShuffleScheduleItems: false, RandomStartPoint: false, FixedStartTimeBehavior: 'Strict', ...(s || {}) };
    const f = form(s ? 'UpdateProgramSchedule' : 'CreateProgramSchedule', [
      { key: 'Name', label: 'Nazwa', required: true },
      { key: 'FixedStartTimeBehavior', label: 'Godziny stałe', hint: 'Domyślne zachowanie pozycji o stałej godzinie (pozycja może je zmienić).' },
      { label: 'Kolejność', group: [{ key: 'ShuffleScheduleItems', kind: 'check', text: 'tasuj pozycje' }, { key: 'RandomStartPoint', kind: 'check', text: 'losowy punkt startu' }] },
      { label: 'Odcinki', group: [{ key: 'KeepMultiPartEpisodesTogether', kind: 'check', text: 'części odcinków razem' }] },
      { key: 'TreatCollectionsAsShows', label: 'Kolekcje', kind: 'check', text: 'traktuj kolekcje jak seriale', show: (v) => v.KeepMultiPartEpisodesTogether },
    ], base);
    ctx.settings = f;
    ctx.itemsEd = itemsEditor({
      items: ctx.items,
      copy: true,
      cols: [[startText, 'Start', { class: 'num' }], [(it) => it._name, 'Treść', { class: 'w' }], [modeText, 'Tryb'], [(it) => L[it.PlaybackOrder] || pl(it.PlaybackOrder), 'Kolejność'],
        [(it) => fillerText(it, ctx.fillers), 'Wypełniacze', { class: 'w' }], [(it) => graphicsCell(it, ctx), 'Znaki i grafika']],
      newItem,
      edit: (it) => itemEditor(it, ctx, !!f.peek().ShuffleScheduleItems),
      empty: 'Harmonogram nie ma pozycji. Dodaj pierwszą – bez pozycji playout nic nie zbuduje.',
      onChange: (items) => ctx.day?.replaceChildren(dayPreview(items)),
    });
    ctx.day = h('div', { class: 'day-preview' }, dayPreview(ctx.items));
    return f;
  },
  below: (s, ctx) => [
    panel(`Pozycje harmonogramu${s ? ' · ' + s.Name : ''}`, h('span', { class: 'dim' }, 'zapis razem z ustawieniami („Zapisz”)'),
      ctx.itemsEd.el, cmdNote('GetProgramScheduleItems', 'ReplaceProgramScheduleItems')),
    panel('Podgląd 24 h', h('span', { class: 'dim' }, 'szkielet z godzin stałych; dokładne godziny liczy legacy przy budowie playoutu'), ctx.day),
  ],
  footer: (s, ctx) => {
    const used = s ? usedBy(s, ctx.playouts) : [];
    return used.length ? h('div', { class: 'note' }, 'Używają go playouty kanałów: ', used.map((p) => `${p.ChannelNumber} · ${p.ChannelName}`).join(', '),
      '. Po zapisie legacy sam przebuduje ich playouty (Refresh, postęp seriali zostaje).') : null;
  },
  beforeSave: (ctx, data) => {
    try { ctx.itemsEd.commit(); } catch (e) { throw new Error('Pozycja w edycji: ' + e.message); }
    const items = ctx.itemsEd.value();
    if (data.ShuffleScheduleItems) {
      const bad = items.findIndex((it) => it.StartType === 'Fixed' || it.PlayoutMode === 'Flood');
      if (bad >= 0) throw new Error(`Pozycja ${bad + 1}: harmonogram tasuje pozycje, więc nie może mieć stałej godziny ani trybu „wypełnij” (jak w legacy).`);
    }
  },
  afterSave: async (id, ctx) => {
    const items = ctx.itemsEd.value().map((it, i) => toReplace(it, i + 1));
    try {
      await cmd('ReplaceProgramScheduleItems', { ProgramScheduleId: id, Items: items });  // `warning` = zapisane (DOKUMENTACJA §13)
    } catch (e) {
      throw new Error('Ustawienia zapisane, ale legacy odrzucił pozycje harmonogramu: ' + e.message);
    }
  },
  create: (data) => data,
  update: (data, s) => ({ ...data, ProgramScheduleId: s.Id }),
  createdId: (r) => r?.ProgramScheduleId,
  copy: (s, name) => ({ ProgramScheduleId: s.Id, Name: name }),
  copiedId: (r) => r?.Id,
  del: (s) => ({ ProgramScheduleId: s.Id }),
  confirmDelete: (s, ctx) => {
    const used = usedBy(s, ctx.playouts);
    return `Usunąć harmonogram „${s.Name}” razem z pozycjami?`
      + (used.length ? ` Legacy usunie też playouty, które go używają (kanały: ${used.map((p) => `${p.ChannelNumber} · ${p.ChannelName}`).join(', ')}) – kanały przestaną nadawać ramówkę.` : '')
      + ' Tego nie da się cofnąć.';
  },
});

/** Playouty klasyczne korzystające z harmonogramu (nazwy harmonogramów są w legacy unikalne). */
const usedBy = (s, playouts) => playouts.filter((p) => p.ScheduleKind === 'Classic' && p.ScheduleName === s.Name);
