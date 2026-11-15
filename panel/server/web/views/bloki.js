// Bloki i szablony dnia (R6). Odpowiednik Blocks, Block Editor, Templates i Template Editor w legacy.
//
// Blok: stała długość (wielokrotność 5 min) i lista pozycji. Legacy nie ma UpdateBlock – nazwę, długość
// i tryb zatrzymania zapisuje ReplaceBlockItems razem z pozycjami. Szablon dnia: bloki o stałych godzinach
// startu; nazwę i pozycje zapisuje ReplaceTemplateItems (legacy odrzuca nakładające się bloki).

import { VIEWS, h, btn, panel, cmd, pl, table, toast, route, cmdNote } from '../core.js';
import { form, parseTimeSpan } from '../form.js';
import { watermarkChoices } from './znaki.js';
import { graphicsChoices } from './grafika.js';
import { crudView, itemsEditor, contentPicker, emptyContent, groupActions, dayBar, minutesOf, hhmm, span } from '../parts.js';

const TAB = { i: 0 };
const KINDS = ['Collection', 'TelevisionShow', 'TelevisionSeason', 'SmartCollection', 'SearchQuery'];

/** Kolejności jak w BlockEditor legacy. */
function orders(type) {
  switch (type) {
    case 'Collection': case 'SmartCollection': return ['Chronological', 'Shuffle', 'Random', 'RandomRotation'];
    case 'TelevisionShow': return ['SeasonEpisode', 'Chronological', 'Shuffle', 'Random'];
    default: return ['Chronological', 'Shuffle', 'Random'];
  }
}

// ---------- bloki ----------
/** Pozycja z GetBlockItems → pozycja edytora (pola ReplaceBlockItem + nazwa). */
const fromVm = (i) => ({
  ...emptyContent(i.CollectionType),
  CollectionId: i.Collection?.Id ?? null, MultiCollectionId: i.MultiCollection?.Id ?? null, SmartCollectionId: i.SmartCollection?.Id ?? null,
  MediaItemId: i.MediaItem?.MediaItemId ?? null, SearchTitle: i.SearchTitle || '', SearchQuery: i.SearchQuery || '',
  Name: i.Collection?.Name ?? i.SmartCollection?.Name ?? i.MultiCollection?.Name ?? i.MediaItem?.Name ?? i.SearchTitle ?? '',
  PlaybackOrder: i.PlaybackOrder, IncludeInProgramGuide: !!i.IncludeInProgramGuide, DisableWatermarks: !!i.DisableWatermarks,
  WatermarkIds: (i.Watermarks || []).map((w) => w.Id), GraphicsElementIds: (i.GraphicsElements || []).map((g) => g.Id),
});
const itemArgs = (items) => items.map((i, n) => ({
  Index: n, CollectionType: i.CollectionType, CollectionId: i.CollectionId, MultiCollectionId: i.MultiCollectionId,
  SmartCollectionId: i.SmartCollectionId, MediaItemId: i.MediaItemId, SearchTitle: i.SearchTitle || '', SearchQuery: i.SearchQuery || '',
  PlaybackOrder: i.PlaybackOrder, IncludeInProgramGuide: !!i.IncludeInProgramGuide, DisableWatermarks: !!i.DisableWatermarks,
  WatermarkIds: i.WatermarkIds || [], GraphicsElementIds: i.GraphicsElementIds || [],
}));
const blockArgs = (b, d, items) => ({ BlockGroupId: b.GroupId, BlockId: b.Id, Name: d.Name.trim(), Minutes: d.Minutes, StopScheduling: d.StopScheduling, Items: itemArgs(items) });

function blockItemEdit(it, ctx) {
  const pick = contentPicker(it, { idPrefix: 'bi-cp-', kinds: KINDS, label: it.Name });
  let f;
  const box = h('div');
  const render = (v) => {
    f = form('ReplaceBlockItems', [
      { key: 'PlaybackOrder', label: 'Kolejność', optional: false, options: orders(v.CollectionType).map((o) => [o, pl(o)]) },
      { key: 'IncludeInProgramGuide', label: 'Przewodnik', kind: 'check', text: 'pokazuj w EPG' },
      { key: 'DisableWatermarks', label: 'Znaki wodne', kind: 'check', text: 'bez znaków wodnych w tej pozycji' },
      { key: 'WatermarkIds', label: 'Własne znaki wodne', kind: 'multi', options: watermarkChoices(ctx.watermarks), show: (x) => !x.DisableWatermarks, empty: 'brak znaków wodnych' },
      { key: 'GraphicsElementIds', label: 'Grafika', kind: 'multi', options: graphicsChoices(ctx.graphics), empty: 'brak elementów graficznych' },
    ], v, { idPrefix: 'bi-' });
    box.replaceChildren(f.el);
  };
  render(it);
  pick.el.addEventListener('change', (e) => {
    if (e.target.id !== 'bi-cp-kind') return;
    const v = { ...f.peek(), CollectionType: e.target.value };
    if (!orders(v.CollectionType).includes(v.PlaybackOrder)) v.PlaybackOrder = orders(v.CollectionType)[0];
    render(v);
  });
  return {
    el: h('div', null, h('div', { class: 'form' }, h('label', null, 'Treść'), pick.el), box),
    read: () => {
      const v = f.read();
      const c = pick.read();
      const ok = orders(c.CollectionType);
      return { ...it, ...v, ...c, Name: pick.label().replace(/^[^:]*: /, ''), PlaybackOrder: ok.includes(v.PlaybackOrder) ? v.PlaybackOrder : ok[0] };
    },
  };
}

async function blockPreview(b, ctx, out) {
  let d;
  try { d = ctx.editor.read(); ctx.itemsEd.commit(); } catch (e) { return toast(e.message, 'bad'); }
  out.replaceChildren(h('span', { class: 'dim' }, 'Liczę podgląd…'));
  try {
    const r = await cmd('PreviewBlockPlayout', { Data: blockArgs(b, d, ctx.itemsEd.value()) });
    const k = 1440 / d.Minutes;  // pasek = cały blok
    const segs = (r || []).map((x) => ({ start: minutesOf(x.Start), end: Math.max(minutesOf(x.Start) + 1, minutesOf(x.Finish) || 1440), label: x.Title }));
    const used = Math.min(d.Minutes, segs.at(-1)?.end ?? 0);
    out.replaceChildren(
      dayBar(segs.map((x) => ({ ...x, start: x.start * k, end: x.end * k, title: `${hhmm(x.start)}–${hhmm(x.end)} ${x.label}` })), { ruler: false, label: 'Wypełnienie bloku', empty: 'blok pusty' }),
      h('div', { class: 'hint' }, `Pasek = cały blok (${d.Minutes} min): ${segs.length} poz., zajęte ${used} min.`),
      table(r || [], [['Start', 'Od', { class: 'num' }], ['Finish', 'Do', { class: 'num' }], ['Title', 'Tytuł', { class: 'w' }], ['Duration', 'Czas', { class: 'num' }]],
        { empty: 'Podgląd jest pusty – pozycje nie mają treści do odtworzenia.' }));
  } catch (e) { out.replaceChildren(h('div', { class: 'note err' }, 'Legacy nie policzył podglądu: ' + e.message)); }
}

const blocks = crudView({
  title: 'Bloki i szablony',
  desc: 'Blok – stała długość i lista pozycji. Szablon dnia – bloki o stałych godzinach. Kalendarz tygodnia (który szablon w który dzień) jest w edytorze playoutu blokowego. Odpowiednik Blocks i Templates.',
  listCmd: 'GetAllBlocks',
  needs: ['GetAllBlockGroups', 'CreateBlockGroup', 'DeleteBlockGroup', 'GetBlockItems', 'PreviewBlockPlayout', 'GetAllWatermarks', 'GetAllGraphicsElements'],
  commands: { create: 'CreateBlock', update: 'ReplaceBlockItems', delete: 'DeleteBlock', copy: 'CopyBlock' },
  noun: { list: 'Bloki', new: 'Nowy blok', add: '+ Nowy blok', create: 'Załóż blok', one: (b) => b.Name, title: (b) => `${b.GroupName} / ${b.Name}` },
  load: async () => {
    const [rows, groups, watermarks, graphics] = await Promise.all([cmd('GetAllBlocks'), cmd('GetAllBlockGroups'), cmd('GetAllWatermarks'), cmd('GetAllGraphicsElements')]);
    rows.sort((a, b) => a.GroupName.localeCompare(b.GroupName, 'pl') || a.Name.localeCompare(b.Name, 'pl'));
    return { rows, groups, watermarks, graphics };
  },
  loadOne: async (b) => ({ items: b ? (await cmd('GetBlockItems', { BlockId: b.Id })).sort((x, y) => x.Index - y.Index).map(fromVm) : [] }),
  id: (b) => b.Id,
  cols: [['GroupName', 'Grupa'], ['Name', 'Blok', { class: 'w' }], [(b) => b.Minutes + ' min', 'Długość', { class: 'num' }]],
  empty: 'Brak bloków. Załóż grupę, potem blok.',
  headExtra: (ctx) => groupActions({ what: 'bloków', create: 'CreateBlockGroup', del: 'DeleteBlockGroup', idKey: 'BlockGroupId', groups: ctx.groups, prefix: 'bg', placeholder: 'np. Informacje' }),
  editor: (b, ctx) => {
    ctx.current = b;
    const f = form('ReplaceBlockItems', [
      b ? null : { key: 'BlockGroupId', label: 'Grupa', options: ctx.groups.map((g) => [g.Id, g.Name]), optional: false, hint: ctx.groups.length ? null : 'Najpierw załóż grupę („+ Grupa”).' },
      { key: 'Name', label: 'Nazwa', required: true, placeholder: 'np. Serwis 30' },
      { key: 'Minutes', label: 'Długość', unit: 'min', kind: 'number', optional: false, attrs: { min: 5, max: 1440, step: 5 }, hint: 'Wielokrotność 5 minut, najwyżej 24 h.' },
      { key: 'StopScheduling', label: 'Koniec bloku', optional: false, options: [['BeforeDurationEnd', 'nie przekraczaj długości (pozycja, która się nie zmieści, nie wchodzi)'], ['AfterDurationEnd', 'ostatnia pozycja może wyjść poza koniec']] },
    ].filter(Boolean), { Name: b?.Name || '', Minutes: b?.Minutes ?? 30, StopScheduling: b?.StopScheduling || 'BeforeDurationEnd', BlockGroupId: ctx.groups[0]?.Id ?? null }, { idPrefix: 'blk-' });
    ctx.editor = f;
    return f;
  },
  beforeSave: (ctx, d) => {
    if (!ctx.current && d.BlockGroupId == null) throw new Error('Wybierz grupę (albo załóż ją przyciskiem „+ Grupa”).');
    if (!(d.Minutes > 0 && d.Minutes % 5 === 0 && d.Minutes <= 1440)) throw new Error('Pole „Długość”: wielokrotność 5 minut, od 5 do 1440.');
    if (ctx.itemsEd) ctx.itemsEd.commit();
  },
  create: (d) => ({ BlockGroupId: d.BlockGroupId, Name: d.Name.trim() }),
  createdId: (r) => r?.Id,
  afterSave: async (id, ctx, d) => {
    if (ctx.current) return;
    try { await cmd('ReplaceBlockItems', blockArgs({ Id: id, GroupId: d.BlockGroupId }, d, [])); }
    catch (e) { throw new Error('Blok założony, ale legacy odrzucił długość: ' + e.message); }
  },
  update: (d, b, ctx) => blockArgs(b, d, ctx.itemsEd.value()),
  del: (b) => ({ BlockId: b.Id }),
  confirmDelete: (b) => `Usunąć blok „${b.Name}”? Zniknie też ze wszystkich szablonów dnia. Tego nie da się cofnąć.`,
  copy: (b, name) => ({ BlockId: b.Id, NewBlockGroupId: b.GroupId, NewBlockName: name }),
  copiedId: (r) => r?.Id,
  below: (b, ctx) => {
    if (!b) return null;
    ctx.itemsEd = itemsEditor({
      items: ctx.items,
      copy: true,
      empty: 'Blok jest pusty. Dodaj kolekcję, serial albo wyszukiwanie.',
      cols: [[(i) => pl(i.CollectionType), 'Rodzaj'], [(i) => i.Name || '—', 'Treść', { class: 'w' }], [(i) => pl(i.PlaybackOrder), 'Kolejność'],
        [(i) => (i.IncludeInProgramGuide ? 'tak' : 'nie'), 'EPG'], [(i) => (i.DisableWatermarks ? 'wyłączone' : i.WatermarkIds.length ? `${i.WatermarkIds.length}` : '—'), 'Znaki'],
        [(i) => (i.GraphicsElementIds.length ? `${i.GraphicsElementIds.length}` : '—'), 'Grafika']],
      newItem: () => ({ ...emptyContent('Collection'), Name: '', PlaybackOrder: 'Chronological', IncludeInProgramGuide: true, DisableWatermarks: false, WatermarkIds: [], GraphicsElementIds: [] }),
      edit: (it) => blockItemEdit(it, ctx),
    });
    const out = h('div', { id: 'blk-preview' });
    return [
      panel('Pozycje bloku', h('span', { class: 'dim' }, 'po kolei, w kółko do końca bloku; zapis: „Zapisz” u góry'), ctx.itemsEd.el, cmdNote('GetBlockItems', 'ReplaceBlockItems')),
      panel('Podgląd bloku', btn('Pokaż podgląd', () => blockPreview(b, ctx, out), 'acc'),
        h('div', { class: 'pad' }, h('div', { class: 'hint' }, 'Jak legacy wypełniłby blok bieżącymi (także niezapisanymi) pozycjami.'), out),
        cmdNote('PreviewBlockPlayout')),
    ];
  },
});

// ---------- szablony dnia ----------
const tplFromVm = (i) => ({ BlockId: i.BlockId, StartTime: span(minutesOf(i.StartTime)) });
const blockOf = (ctx, id) => ctx.blocks.find((b) => b.Id === id);
const startMin = (i) => minutesOf(i.StartTime);
const endMin = (ctx, i) => startMin(i) + (blockOf(ctx, i.BlockId)?.Minutes ?? 0);

/** Pasek doby szablonu; nakładające się bloki na czerwono. */
export function templateBar(ctx, items, opts) {
  const sorted = [...items].sort((a, b) => startMin(a) - startMin(b));
  return dayBar(sorted.map((i, n) => {
    const b = blockOf(ctx, i.BlockId);
    const overlap = n > 0 && startMin(i) < endMin(ctx, sorted[n - 1]);
    return { start: startMin(i), end: endMin(ctx, i), label: b ? b.Name : `blok ${i.BlockId}`, cls: overlap || endMin(ctx, i) > 1440 ? 'bad' : '' };
  }), opts);
}

const templates = crudView({
  title: 'Bloki i szablony',
  desc: 'Szablon dnia – bloki o stałych godzinach startu. Godziny bez bloku wypełnia legacy wg deco (martwe powietrze). Odpowiednik Templates.',
  listCmd: 'GetAllTemplates',
  needs: ['GetAllTemplateGroups', 'CreateTemplateGroup', 'DeleteTemplateGroup', 'GetTemplateItems', 'GetAllBlocks'],
  commands: { create: 'CreateTemplate', update: 'ReplaceTemplateItems', delete: 'DeleteTemplate', copy: 'CopyTemplate' },
  noun: { list: 'Szablony dnia', new: 'Nowy szablon dnia', add: '+ Nowy szablon', create: 'Załóż szablon', one: (t) => t.Name, title: (t) => `${t.GroupName} / ${t.Name}` },
  load: async () => {
    const [rows, groups, blocksList] = await Promise.all([cmd('GetAllTemplates'), cmd('GetAllTemplateGroups'), cmd('GetAllBlocks')]);
    rows.sort((a, b) => a.GroupName.localeCompare(b.GroupName, 'pl') || a.Name.localeCompare(b.Name, 'pl'));
    blocksList.sort((a, b) => a.GroupName.localeCompare(b.GroupName, 'pl') || a.Name.localeCompare(b.Name, 'pl'));
    return { rows, groups, blocks: blocksList };
  },
  loadOne: async (t) => ({ items: t ? (await cmd('GetTemplateItems', { TemplateId: t.Id })).map(tplFromVm).sort((a, b) => startMin(a) - startMin(b)) : [] }),
  id: (t) => t.Id,
  cols: [['GroupName', 'Grupa'], ['Name', 'Szablon', { class: 'w' }]],
  empty: 'Brak szablonów dnia. Załóż grupę, potem szablon.',
  headExtra: (ctx) => groupActions({ what: 'szablonów', create: 'CreateTemplateGroup', del: 'DeleteTemplateGroup', idKey: 'TemplateGroupId', groups: ctx.groups, count: (g) => `${g.TemplateCount} szabl.`, prefix: 'tg', placeholder: 'np. Tydzień' }),
  editor: (t, ctx) => {
    ctx.current = t;
    return form('CreateTemplate', [
      t ? null : { key: 'TemplateGroupId', label: 'Grupa', options: ctx.groups.map((g) => [g.Id, g.Name]), optional: false, hint: ctx.groups.length ? null : 'Najpierw załóż grupę („+ Grupa”).' },
      { key: 'Name', label: 'Nazwa', required: true, placeholder: 'np. Dzień roboczy' },
    ].filter(Boolean), { Name: t?.Name || '', TemplateGroupId: ctx.groups[0]?.Id ?? null }, { idPrefix: 'tpl-' });
  },
  beforeSave: (ctx, d) => {
    if (!ctx.current && d.TemplateGroupId == null) throw new Error('Wybierz grupę (albo załóż ją przyciskiem „+ Grupa”).');
    if (ctx.itemsEd) ctx.itemsEd.commit();
  },
  create: (d) => ({ TemplateGroupId: d.TemplateGroupId, Name: d.Name.trim() }),
  createdId: (r) => r?.Id,
  update: (d, t, ctx) => ({
    TemplateGroupId: t.TemplateGroupId, TemplateId: t.Id, Name: d.Name.trim(),
    Items: ctx.itemsEd.value().sort((a, b) => startMin(a) - startMin(b)).map((i) => ({ BlockId: i.BlockId, StartTime: i.StartTime })),
  }),
  del: (t) => ({ TemplateId: t.Id }),
  confirmDelete: (t) => `Usunąć szablon dnia „${t.Name}”? Zniknie też z kalendarzy playoutów blokowych. Tego nie da się cofnąć.`,
  copy: (t, name) => ({ TemplateId: t.Id, NewTemplateGroupId: t.TemplateGroupId, NewTemplateName: name }),
  copiedId: (r) => r?.Id,
  below: (t, ctx) => {
    if (!t) return null;
    const bar = h('div', { id: 'tpl-bar' });
    const drawBar = (items) => bar.replaceChildren(templateBar(ctx, items, { label: 'Szablon dnia', empty: 'szablon pusty – dodaj blok' }));
    ctx.itemsEd = itemsEditor({
      items: ctx.items,
      addText: '+ Dodaj blok',
      empty: 'Szablon jest pusty. Dodaj pierwszy blok.',
      onChange: drawBar,
      cols: [[(i) => hhmm(startMin(i)), 'Start', { class: 'num' }], [(i) => hhmm(endMin(ctx, i)), 'Koniec', { class: 'num' }],
        [(i) => { const b = blockOf(ctx, i.BlockId); return b ? `${b.GroupName} / ${b.Name}` : `blok ${i.BlockId}`; }, 'Blok', { class: 'w' }],
        [(i) => (blockOf(ctx, i.BlockId)?.Minutes ?? '?') + ' min', 'Długość', { class: 'num' }]],
      newItem: () => {
        const cur = ctx.itemsEd ? ctx.itemsEd.value() : ctx.items;
        const last = Math.max(0, ...cur.map((i) => endMin(ctx, i)));
        return { BlockId: ctx.blocks[0]?.Id ?? null, StartTime: span(Math.min(last, 1435)) };
      },
      edit: (it) => {
        const f = form('', [
          { key: 'BlockId', label: 'Blok', optional: false, options: ctx.blocks.map((b) => [b.Id, `${b.GroupName} / ${b.Name} (${b.Minutes} min)`]), hint: ctx.blocks.length ? null : 'Najpierw załóż blok w zakładce „Bloki”.' },
          { key: 'StartTime', label: 'Start', kind: 'time', optional: false, placeholder: 'gg:mm', hint: 'Godzina startu bloku w ciągu doby (np. 06:00).' },
        ], it, { idPrefix: 'ti-' });
        return { el: f.el, read: () => { const v = f.read(); return { ...v, StartTime: parseTimeSpan(v.StartTime) }; } };
      },
    });
    drawBar(ctx.items);
    return panel('Doba szablonu', h('span', { class: 'dim' }, 'zapis: „Zapisz” u góry; czerwone = nakładanie albo wyjście za północ'), bar, ctx.itemsEd.el, cmdNote('GetTemplateItems', 'ReplaceTemplateItems'));
  },
});

// ---------- ekran z zakładkami ----------
const TABS = [['Bloki', blocks], ['Szablony dnia', templates]];

VIEWS.bloki = async () => {
  const nodes = await TABS[TAB.i][1]();
  const tabs = h('div', { class: 'tabs page-tabs', role: 'tablist' }, TABS.map(([t], i) =>
    h('button', { type: 'button', role: 'tab', class: i === TAB.i ? 'on' : null, 'aria-selected': String(i === TAB.i), onclick: () => { TAB.i = i; route(); } }, t)));
  return [nodes[0], tabs, ...nodes.slice(1)];
};
