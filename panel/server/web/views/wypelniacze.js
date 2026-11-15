// Wypełniacze (R5): co wstawić przed / w trakcie / po pozycji, ogon i wypełniacz awaryjny.
// Odpowiednik Filler Presets + Filler Preset Editor legacy.

import { VIEWS, h, cmd, pl, pill } from '../core.js';
import { form } from '../form.js';
import { crudView, contentPicker } from '../parts.js';

const KINDS = ['PreRoll', 'MidRoll', 'PostRoll', 'Tail', 'Fallback'];
const MODES = ['Duration', 'Count', 'Pad', 'RandomCount'];
const PAD = [[5, 'do 5 min (:00, :05, :10…)'], [10, 'do 10 min (:00, :10, :20…)'], [15, 'do 15 min (:00, :15, :30, :45)'], [30, 'do 30 min (:00, :30)']];
// rodzaje treści jak w Filler Preset Editor legacy (playlista nie dla ogona i zapasowego)
const CONTENT = ['Collection', 'TelevisionShow', 'TelevisionSeason', 'Artist', 'MultiCollection', 'SmartCollection', 'Playlist'];
const KIND_HINT = {
  PreRoll: 'Przed każdą pozycją ramówki.', MidRoll: 'W przerwach pozycji (wg rozdziałów albo wyrażenia).', PostRoll: 'Po każdej pozycji.',
  Tail: 'Na końcu bloku czasowego, gdy pozycje nie wypełniają go do końca.', Fallback: 'Gdy nie ma nic innego do nadania (zamiast czarnej planszy).',
};
const noMode = (k) => k === 'Tail' || k === 'Fallback';

const describe = (x) => {
  if (noMode(x.FillerKind)) return '—';
  switch (x.FillerMode) {
    case 'Duration': return `przez ${x.Duration}`;
    case 'Count': return `${x.Count} poz.`;
    case 'RandomCount': return `losowo do ${x.Count} poz.`;
    case 'Pad': return `dopełnij do ${x.PadToNearestMinute} min`;
    default: return pl(x.FillerMode);
  }
};

/** Nazwa treści wypełniacza z list kolekcji (legacy zwraca tylko id). */
function contentName(x, ctx) {
  const find = (list, id) => list.find((c) => c.Id === id)?.Name;
  switch (x.CollectionType) {
    case 'Collection': return find(ctx.collections, x.CollectionId);
    case 'SmartCollection': return find(ctx.smart, x.SmartCollectionId);
    case 'MultiCollection': return find(ctx.multi, x.MultiCollectionId);
    case 'Playlist': return x.Playlist?.Name;
    default: return x.MediaItemId != null ? `id ${x.MediaItemId}` : null;
  }
}

VIEWS.wypelniacze = crudView({
  title: 'Wypełniacze',
  desc: 'Co wstawić przed, w trakcie i po pozycji, czym dopełnić czas do pełnych minut, co nadać awaryjnie. Odpowiednik Filler Presets w legacy. Wypełniacz przypisuje się w pozycji harmonogramu albo (awaryjny) w kanale.',
  listCmd: 'GetAllFillerPresets',
  needs: ['GetAllCollections', 'GetAllSmartCollections', 'GetAllMultiCollections'],
  commands: { create: 'CreateFillerPreset', update: 'UpdateFillerPreset', delete: 'DeleteFillerPreset' },
  noun: { list: 'Wypełniacze', new: 'Nowy wypełniacz', add: '+ Nowy wypełniacz', create: 'Załóż wypełniacz', one: (x) => x.Name, title: (x) => x.Name },
  load: async () => {
    const [rows, collections, smart, multi] = await Promise.all([
      cmd('GetAllFillerPresets'), cmd('GetAllCollections'), cmd('GetAllSmartCollections'), cmd('GetAllMultiCollections')]);
    return { rows, collections, smart, multi };
  },
  id: (x) => x.Id,
  cols: (ctx) => [
    ['Name', 'Nazwa'], [(x) => pill(pl(x.FillerKind), x.FillerKind === 'Fallback' ? 'warn' : null), 'Miejsce'], [describe, 'Ile'],
    [(x) => `${pl(x.CollectionType)}: ${contentName(x, ctx) ?? '—'}`, 'Treść'], [(x) => (x.AllowWatermarks ? 'tak' : 'nie'), 'Znaki wodne'],
  ],
  editor: (x, ctx) => {
    const base = {
      Name: '', FillerKind: 'PreRoll', FillerMode: 'Count', Duration: null, Count: 1, PadToNearestMinute: 30, AllowWatermarks: false,
      CollectionType: 'SmartCollection', Expression: '', UseChaptersAsMediaItems: false,
      ...(x || {}), PlaylistId: x?.Playlist?.Id ?? null,
    };
    if (x && (!x.FillerMode || x.FillerMode === 'None')) base.FillerMode = 'Count';
    const content = (v) => contentPicker(v, { idPrefix: 'fp-cp-', kinds: CONTENT, label: x ? contentName(x, ctx) : null });
    return form(x ? 'UpdateFillerPreset' : 'CreateFillerPreset', [
      { sec: 'Ogólne' },
      { key: 'Name', label: 'Nazwa', required: true, placeholder: 'np. Zapowiedzi do pełnej połówki' },
      { key: 'FillerKind', label: 'Miejsce', options: KINDS.map((k) => [k, pl(k)]), attrs: x ? { disabled: true } : null,
        hint: (v) => KIND_HINT[v.FillerKind] + (x ? ' Miejsca nie da się zmienić (jak w legacy).' : '') },
      { key: 'FillerMode', label: 'Tryb', options: MODES.map((m) => [m, pl(m)]), show: (v) => !noMode(v.FillerKind),
        hint: (v) => (v._content?.CollectionType === 'Playlist' ? 'Dla playlisty legacy zawsze liczy pozycje (tryb „liczba”).' : '') },
      { key: 'Duration', label: 'Czas', kind: 'time', show: (v) => !noMode(v.FillerKind) && v.FillerMode === 'Duration' },
      { key: 'Count', label: 'Liczba pozycji', kind: 'number', show: (v) => !noMode(v.FillerKind) && (v.FillerMode === 'Count' || v.FillerMode === 'RandomCount') },
      { key: 'PadToNearestMinute', label: 'Dopełnij', options: PAD, show: (v) => !noMode(v.FillerKind) && v.FillerMode === 'Pad',
        hint: 'Wypełniacz dobiera pozycje tak, żeby następna audycja zaczęła się o pełnej godzinie z siatki.' },
      { key: 'AllowWatermarks', kind: 'check', label: 'Znaki wodne', text: 'pokazuj znak wodny kanału także na wypełniaczu' },
      { sec: 'Treść' },
      { key: '_content', label: 'Treść', render: () => content(base) },
      { key: 'Expression', label: 'Wyrażenie przerw', attrs: { class: 'mono' }, placeholder: 'np. point > 300', show: (v) => v.FillerKind === 'MidRoll',
        hint: 'Kiedy wstawiać przerwy w trakcie pozycji (składnia legacy); puste = w punktach rozdziałów.' },
      { key: 'UseChaptersAsMediaItems', kind: 'check', label: 'Rozdziały', text: 'traktuj rozdziały jak osobne pozycje', show: (v) => v.FillerKind !== 'Fallback' },
    ], base);
  },
  beforeSave: (ctx, d) => {
    if (noMode(d.FillerKind) && d._content?.CollectionType === 'Playlist') throw new Error('Ogon i wypełniacz awaryjny nie mogą brać treści z playlisty (jak w legacy).');
    if (!noMode(d.FillerKind) && d.FillerMode === 'Duration' && !d.Duration) throw new Error('Pole „Czas”: podaj czas gg:mm:ss.');
    if (!noMode(d.FillerKind) && (d.FillerMode === 'Count' || d.FillerMode === 'RandomCount') && !(d.Count > 0)) throw new Error('Pole „Liczba pozycji”: podaj liczbę większą od zera.');
  },
  create: (d) => args(d),
  update: (d, x) => ({ ...args(d), Id: x.Id }),
  del: (x) => ({ FillerPresetId: x.Id }),
});

/** Pola komendy jak FillerPresetEditViewModel legacy: wartości spoza trybu = null, tryb dla ogona/zapasowego = None. */
function args(d) {
  const c = d._content;
  const mode = noMode(d.FillerKind) ? 'None' : c.CollectionType === 'Playlist' ? 'Count' : d.FillerMode;
  return {
    Name: d.Name.trim(), FillerKind: d.FillerKind, FillerMode: mode,
    Duration: mode === 'Duration' ? d.Duration : null,
    Count: mode === 'Count' || mode === 'RandomCount' ? d.Count : null,
    PadToNearestMinute: mode === 'Pad' ? d.PadToNearestMinute : null,
    AllowWatermarks: d.AllowWatermarks,
    CollectionType: c.CollectionType, CollectionId: c.CollectionId, MediaItemId: c.MediaItemId, MultiCollectionId: c.MultiCollectionId,
    SmartCollectionId: c.SmartCollectionId, PlaylistId: c.PlaylistId,
    Expression: d.FillerKind === 'MidRoll' ? d.Expression || '' : '',
    UseChaptersAsMediaItems: d.FillerKind === 'Fallback' ? false : d.UseChaptersAsMediaItems,
  };
}
