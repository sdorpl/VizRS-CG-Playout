// Playlisty (R4): grupy playlist, playlisty z edytorem pozycji i podglądem playoutu.
// Odpowiednik Playlists + Playlist Editor w legacy. Pojedyncze pozycje (film, inne wideo…) dodaje się
// w Przeglądarce mediów („Dodaj do…”); w edytorze można zmienić ich kolejność i ustawienia.

import { VIEWS, h, btn, panel, cmd, pl, pill, table, toast, dialog, route, cmdNote } from '../core.js';
import { form } from '../form.js';
import { importSchedule } from './import.js';
import { crudView, itemsEditor, contentPicker } from '../parts.js';

const PICK_KINDS = ['Collection', 'TelevisionShow', 'TelevisionSeason', 'Artist', 'MultiCollection', 'SmartCollection'];
const SINGLE = ['Movie', 'Episode', 'MusicVideo', 'OtherVideo', 'Song', 'Image'];

/** Kolejności jak w PlaylistEditor legacy; pojedyncza pozycja nie ma kolejności. */
function orders(type) {
  switch (type) {
    case 'MultiCollection': return ['Shuffle'];
    case 'Collection': case 'SmartCollection': return ['Chronological', 'Shuffle', 'Random', 'ShuffleInOrder'];
    case 'TelevisionShow': return ['SeasonEpisode', 'Chronological', 'Shuffle', 'Random'];
    default: return SINGLE.includes(type) ? [] : ['Chronological', 'Shuffle', 'Random'];
  }
}

/** Pozycja z GetPlaylistItems → pozycja edytora (pola ReplacePlaylistItem + nazwa do wyświetlenia). */
const fromVm = (i) => ({
  CollectionType: i.CollectionType, CollectionId: i.Collection?.Id ?? null, MultiCollectionId: i.MultiCollection?.Id ?? null,
  SmartCollectionId: i.SmartCollection?.Id ?? null, MediaItemId: i.MediaItem?.MediaItemId ?? null,
  Name: i.Collection?.Name ?? i.MultiCollection?.Name ?? i.SmartCollection?.Name ?? i.MediaItem?.Name ?? '',
  PlaybackOrder: i.PlaybackOrder, Count: i.Count, PlayAll: i.PlayAll, IncludeInProgramGuide: i.IncludeInProgramGuide,
});
const toArgs = (items) => items.map((i, n) => ({
  Index: n, CollectionType: i.CollectionType, CollectionId: i.CollectionId, MultiCollectionId: i.MultiCollectionId,
  SmartCollectionId: i.SmartCollectionId, MediaItemId: i.MediaItemId,
  PlaybackOrder: SINGLE.includes(i.CollectionType) ? 'None' : i.PlaybackOrder, Count: i.PlayAll ? null : i.Count, PlayAll: !!i.PlayAll,
  IncludeInProgramGuide: !!i.IncludeInProgramGuide,
}));
const howMany = (i) => (i.PlayAll ? 'wszystkie' : String(i.Count ?? 1));

/** Panel boczny edytora pozycji playlisty. */
function itemEdit(it) {
  const single = SINGLE.includes(it.CollectionType);
  const pick = single ? null : contentPicker(it, { idPrefix: 'pl-cp-', kinds: PICK_KINDS, label: it.Name });
  let f;
  const box = h('div');
  const build = (v) => form('ReplacePlaylistItems', [
    single ? { key: '_single', label: 'Pozycja', render: () => ({ el: h('div', null, `${pl(it.CollectionType)}: `, h('b', null, it.Name || `id ${it.MediaItemId}`)), read: () => undefined }) } : null,
    single ? null : { key: 'PlaybackOrder', label: 'Kolejność', optional: false, options: orders(v.CollectionType).map((o) => [o, pl(o)]) },
    { key: 'PlayAll', label: 'Ile', kind: 'check', text: 'odtwórz wszystkie pozycje' },
    { key: 'Count', label: 'Liczba pozycji', kind: 'number', optional: true, placeholder: '1', show: (x) => !x.PlayAll, hint: 'Puste = 1.' },
    { key: 'IncludeInProgramGuide', label: 'Przewodnik', kind: 'check', text: 'pokazuj w EPG' },
  ].filter(Boolean), v, { idPrefix: 'pl-' });
  const render = (v) => { f = build(v); box.replaceChildren(f.el); };
  render(it);
  if (pick) {
    pick.el.addEventListener('change', (e) => {
      if (e.target.id !== 'pl-cp-kind') return;
      const v = { ...f.peek(), CollectionType: e.target.value };
      if (!orders(v.CollectionType).includes(v.PlaybackOrder)) v.PlaybackOrder = orders(v.CollectionType)[0];
      render(v);
    });
  }
  return {
    el: h('div', null, pick ? h('div', { class: 'form' }, h('label', null, 'Treść'), pick.el) : null, box),
    read: () => {
      const v = f.read();
      if (!v.PlayAll && v.Count != null && !(v.Count > 0)) throw new Error('Pole „Liczba pozycji”: podaj liczbę większą od zera.');
      if (single) return { ...it, ...v };
      const c = pick.read();
      const ok = orders(c.CollectionType);
      return { ...it, ...v, ...c, Name: pick.label().replace(/^[^:]*: /, ''), PlaybackOrder: ok.includes(v.PlaybackOrder) ? v.PlaybackOrder : ok[0] };
    },
  };
}

async function newGroup() {
  const input = h('input', { id: 'plg-name', placeholder: 'np. Pasma informacyjne' });
  const ok = await dialog('Nowa grupa playlist', h('div', { class: 'form' }, h('label', { for: 'plg-name' }, 'Nazwa grupy'), input), [['Anuluj', false], ['Załóż', true, 'acc']]);
  if (!ok || !input.value.trim()) return;
  try { await cmd('CreatePlaylistGroup', { Name: input.value.trim() }); toast('Założono grupę.', 'ok'); route(true); }
  catch (e) { toast('Legacy odrzucił: ' + e.message, 'bad'); }
}
async function deleteGroup(groups) {
  const own = groups.filter((g) => !g.IsSystem);
  if (!own.length) return toast('Nie ma grup do usunięcia (grupy systemowe zostają).', 'bad');
  const sel = h('select', { id: 'plg-del' }, own.map((g) => h('option', { value: String(g.Id) }, `${g.Name} (${g.PlaylistCount} playlist)`)));
  const ok = await dialog('Usuń grupę playlist', h('div', { class: 'form' }, h('label', { for: 'plg-del' }, 'Grupa'), sel,
    h('div', { class: 'hint' }, 'Usunięcie grupy usuwa też wszystkie jej playlisty. Tego nie da się cofnąć.')), [['Anuluj', false], ['Usuń', true, 'danger']]);
  if (!ok) return;
  try { await cmd('DeletePlaylistGroup', { PlaylistGroupId: Number(sel.value) }); toast('Usunięto grupę.', 'ok'); route(true); }
  catch (e) { toast('Legacy odrzucił: ' + e.message, 'bad'); }
}

/**
 * Podgląd: legacy liczy go z zapisanej playlisty (PreviewPlaylistPlayout czyta pozycje z bazy po PlaylistId,
 * DOKUMENTACJA §5), więc najpierw zapis bieżącej listy (ReplacePlaylistItems), potem podgląd.
 */
async function preview(p, name, items, out) {
  out.replaceChildren(h('span', { class: 'dim' }, 'Zapisuję i liczę podgląd…'));
  try {
    await cmd('ReplacePlaylistItems', { PlaylistId: p.Id, Name: name, Items: toArgs(items) });
  } catch (e) { return out.replaceChildren(h('div', { class: 'note err' }, 'Legacy odrzucił zapis: ' + e.message)); }
  try {
    const r = await cmd('PreviewPlaylistPlayout', { Data: { PlaylistId: p.Id, Name: name, Items: toArgs(items) } });
    out.replaceChildren(table(r || [], [['Start', 'Od', { class: 'num' }], ['Finish', 'Do', { class: 'num' }], ['Title', 'Tytuł', { class: 'w' }], ['Duration', 'Czas', { class: 'num' }]],
      { empty: 'Podgląd jest pusty – pozycje nie mają treści do odtworzenia.' }));
  } catch (e) { out.replaceChildren(h('div', { class: 'note err' }, 'Legacy nie policzył podglądu: ' + e.message)); }
}

VIEWS.playlisty = crudView({
  title: 'Playlisty',
  desc: 'Ustalona kolejność pozycji z różnych źródeł, w grupach. Odpowiednik Playlists w legacy. Playlistę wybiera się w harmonogramie jako treść.',
  listCmd: 'GetAllPlaylistGroups',
  needs: ['GetPlaylistsByPlaylistGroupId', 'GetPlaylistItems', 'CreatePlaylistGroup', 'DeletePlaylistGroup', 'PreviewPlaylistPlayout'],
  commands: { create: 'CreatePlaylist', update: 'ReplacePlaylistItems', delete: 'DeletePlaylist' },
  noun: { list: 'Playlisty', new: 'Nowa playlista', add: '+ Nowa playlista', create: 'Załóż playlistę', one: (p) => p.Name, title: (p) => `${p.Group} / ${p.Name}` },
  load: async () => {
    const groups = await cmd('GetAllPlaylistGroups');
    const lists = await Promise.all(groups.map((g) => cmd('GetPlaylistsByPlaylistGroupId', { PlaylistGroupId: g.Id }).then((ps) => ps.map((p) => ({ ...p, Group: g.Name })))));
    return { rows: lists.flat(), groups };
  },
  loadOne: async (p) => ({ items: p ? (await cmd('GetPlaylistItems', { PlaylistId: p.Id })).sort((a, b) => a.Index - b.Index).map(fromVm) : [] }),
  id: (p) => p.Id,
  cols: [['Group', 'Grupa'], ['Name', 'Playlista', { class: 'w' }], [(p) => (p.IsSystem ? pill('systemowa') : h('span')), '']],
  empty: 'Brak playlist. Załóż grupę, potem playlistę.',
  headExtra: (ctx) => [btn('+ Grupa', newGroup), btn('Usuń grupę…', () => deleteGroup(ctx.groups)), btn('Import ramówki z pliku…', () => importSchedule(ctx.groups))],
  editor: (p, ctx) => {
    ctx.current = p;
    const groups = ctx.groups.filter((g) => !g.IsSystem);
    const f = form('CreatePlaylist', [
      p ? null : { key: 'PlaylistGroupId', label: 'Grupa', options: groups.map((g) => [g.Id, g.Name]), hint: groups.length ? null : 'Najpierw załóż grupę („+ Grupa”).' },
      { key: 'Name', label: 'Nazwa', required: true, placeholder: 'np. Poranek' },
    ].filter(Boolean), { Name: p?.Name || '', PlaylistGroupId: groups[0]?.Id ?? null }, { idPrefix: 'pls-' });
    return { el: h('div', null, p?.IsSystem ? h('div', { class: 'note' }, 'Playlista systemowa (np. z listy Trakt) – legacy nie pozwala jej edytować.') : null, f.el), read: f.read };
  },
  beforeSave: (ctx, d) => { if (ctx.current?.IsSystem) throw new Error('Playlisty systemowej nie da się zmienić.'); if (!ctx.current && d.PlaylistGroupId == null) throw new Error('Wybierz grupę (albo załóż ją przyciskiem „+ Grupa”).'); },
  create: (d) => ({ PlaylistGroupId: d.PlaylistGroupId, Name: d.Name.trim() }),
  createdId: (r) => r?.Id,
  update: (d, p, ctx) => { ctx.itemsEd.commit(); return { PlaylistId: p.Id, Name: d.Name.trim(), Items: toArgs(ctx.itemsEd.value()) }; },
  del: (p) => ({ PlaylistId: p.Id }),
  below: (p, ctx) => {
    if (!p) return null;
    ctx.itemsEd = itemsEditor({
      items: ctx.items,
      copy: true,
      empty: 'Playlista jest pusta. Dodaj kolekcję albo serial tutaj, a pojedyncze pliki w Przeglądarce mediów.',
      cols: [[(i) => pl(i.CollectionType), 'Rodzaj'], [(i) => i.Name || '—', 'Treść', { class: 'w' }],
        [(i) => (SINGLE.includes(i.CollectionType) ? '—' : pl(i.PlaybackOrder)), 'Kolejność'], [howMany, 'Ile', { class: 'num' }],
        [(i) => (i.IncludeInProgramGuide ? 'tak' : 'nie'), 'EPG']],
      newItem: () => ({ CollectionType: 'Collection', CollectionId: null, MultiCollectionId: null, SmartCollectionId: null, MediaItemId: null, Name: '', PlaybackOrder: 'Chronological', Count: null, PlayAll: false, IncludeInProgramGuide: true }),
      edit: (it) => itemEdit(it),
    });
    const out = h('div', { id: 'pl-preview' });
    return [
      panel('Pozycje playlisty', h('span', { class: 'dim' }, 'zapis: „Zapisz” u góry'), ctx.itemsEd.el,
        h('div', { class: 'bar pad' }, btn('Dodaj pliki z Przeglądarki mediów…', () => (location.hash = '#przegladarka'))),
        cmdNote('GetPlaylistItems', 'ReplacePlaylistItems', 'AddItemsToPlaylist')),
      panel('Podgląd playoutu', btn('Zapisz i pokaż podgląd', () => {
        try { ctx.itemsEd.commit(); } catch (e) { return toast(e.message, 'bad'); }
        preview(p, document.getElementById('pls-Name').value.trim() || p.Name, ctx.itemsEd.value(), out);
      }, 'acc'),
        h('div', { class: 'pad' }, h('div', { class: 'hint' }, 'Jak legacy ułożyłby tę playlistę od północy. Legacy liczy podgląd z zapisanej playlisty, więc przycisk najpierw zapisuje pozycje.'), out),
        cmdNote('ReplacePlaylistItems', 'PreviewPlaylistPlayout')),
    ];
  },
});
