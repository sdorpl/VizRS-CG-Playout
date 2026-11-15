// Widok „Harmonogramy” (grupa Ramówka): ramówka klasyczna – ustawienia harmonogramu i jego pozycje.
// Odpowiednik Schedules + Schedule Editor + Schedule Items Editor w legacy.
// Pozycje zapisuje jedno ReplaceProgramScheduleItems (jak UI legacy); most zwraca wtedy `warning` (DOKUMENTACJA §5).
// Pola pozycji (ReplaceProgramScheduleItem) są typem zagnieżdżonym – most nie podaje ich wyliczeń, więc listy
// wyboru są tutaj, z tymi samymi ograniczeniami co w ScheduleItemsEditor.razor.

import { VIEWS, h, panel, cmd, pl, pill, cmdNote } from '../core.js';
import { form } from '../form.js';
import { watermarkChoices } from './znaki.js';
import { graphicsChoices } from './grafika.js';
import { crudView, itemsEditor, contentPicker, emptyContent, dayBar, minutesOf, hhmm } from '../parts.js';

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
function fromLegacy(vm) {
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
function toReplace(it, index) {
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
  loadOne: async (s) => ({ items: s ? (await cmd('GetProgramScheduleItems', { Id: s.Id })).sort((a, b) => a.Index - b.Index).map(fromLegacy) : [] }),
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
      '. Zmiany wejdą na antenę po przebudowie playoutu.') : null;
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
      await cmd('ReplaceProgramScheduleItems', { ProgramScheduleId: id, Items: items });  // `warning` = zapisane (DOKUMENTACJA §5)
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
