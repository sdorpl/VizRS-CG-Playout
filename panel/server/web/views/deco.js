// Wystrój (deco) i szablony deco (R6). Odpowiednik Decos, Deco Editor, Deco Templates i Deco Template Editor.
//
// Deco – oprawa bloku: znak wodny, grafika, treść przerw, wypełniacz domyślny, martwe powietrze. Każda część ma
// tryb: dziedzicz (z deco wyżej: szablon deco → deco domyślne playoutu), wyłącz, zastąp, scal (tylko znak i grafika).
// Szablon deco – deco w przedziałach godzin doby; przypisuje się go razem z szablonem dnia w kalendarzu playoutu.

import { VIEWS, h, panel, cmd, pl, route, cmdNote } from '../core.js';
import { form, parseTimeSpan } from '../form.js';
import { watermarkChoices } from './znaki.js';
import { graphicsChoices } from './grafika.js';
import { crudView, itemsEditor, contentPicker, emptyContent, groupActions, dayBar, minutesOf, hhmm, span } from '../parts.js';

const TAB = { i: 0 };
const FILL_KINDS = ['Collection', 'TelevisionShow', 'TelevisionSeason', 'Artist', 'MultiCollection', 'SmartCollection'];
const MODES4 = [['Inherit', 'dziedzicz'], ['Disable', 'wyłącz'], ['Override', 'zastąp'], ['Merge', 'scal']];
const MODES3 = MODES4.slice(0, 3);
const MODE_HINT = 'Dziedzicz – z szablonu deco albo deco domyślnego playoutu; zastąp – tylko te poniżej; scal – te poniżej i odziedziczone.';

/** Pola treści (prefiks DefaultFiller / DeadAirFallback) z widoku legacy → pola wyboru treści. */
function contentOf(d, p) {
  const c = { ...emptyContent(d[p + 'CollectionType'] || 'Collection') };
  c.CollectionId = d[p + 'Collection']?.Id ?? null;
  c.MultiCollectionId = d[p + 'MultiCollection']?.Id ?? null;
  c.SmartCollectionId = d[p + 'SmartCollection']?.Id ?? null;
  c.MediaItemId = d[p + 'MediaItem']?.MediaItemId ?? null;
  const name = d[p + 'Collection']?.Name ?? d[p + 'MultiCollection']?.Name ?? d[p + 'SmartCollection']?.Name ?? d[p + 'MediaItem']?.Name ?? null;
  return [c, name];
}
const contentArgs = (p, c) => ({
  [p + 'CollectionType']: c.CollectionType, [p + 'CollectionId']: c.CollectionId, [p + 'MediaItemId']: c.MediaItemId,
  [p + 'MultiCollectionId']: c.MultiCollectionId, [p + 'SmartCollectionId']: c.SmartCollectionId,
});

/** Treść przerw: playlisty na początku bloku (web GUI legacy 26.11 pozwala tylko na playlistę i „start bloku”). */
function breaksField(value) {
  const ed = itemsEditor({
    items: value,
    addText: '+ Dodaj treść przerwy',
    empty: 'Brak treści przerw.',
    cols: [[(b) => pl(b.CollectionType), 'Rodzaj'], [(b) => b.Name || '—', 'Treść', { class: 'w' }], [() => 'start bloku', 'Miejsce']],
    newItem: () => ({ Id: 0, ...emptyContent('Playlist'), Name: '', Placement: 'BlockStart' }),
    edit: (it) => {
      const pick = contentPicker(it, { idPrefix: 'brk-cp-', kinds: ['Playlist'], label: it.Name });
      return {
        el: h('div', { class: 'form' }, h('label', null, 'Playlista'), pick.el, h('label', null, 'Miejsce'), h('div', null, 'start bloku'),
          h('div', { class: 'hint' }, 'Legacy wstawia treść przerwy na początku każdego bloku z tym deco.')),
        read: () => ({ ...it, ...pick.read(), Name: pick.label().replace(/^[^:]*: /, '') }),
      };
    },
  });
  return { el: h('div', { id: 'deco-breaks' }, ed.el), read: () => { ed.commit(); return ed.value(); }, peek: () => ed.value() };
}

function decoEditor(d, ctx) {
  if (!d) {
    return form('CreateDeco', [
      { key: 'DecoGroupId', label: 'Grupa', options: ctx.groups.map((g) => [g.Id, g.Name]), optional: false, hint: ctx.groups.length ? 'Resztę ustawień edytuje się po założeniu.' : 'Najpierw załóż grupę („+ Grupa”).' },
      { key: 'Name', label: 'Nazwa', required: true, placeholder: 'np. Serwis z paskiem' },
    ], { Name: '', DecoGroupId: ctx.groups[0]?.Id ?? null }, { idPrefix: 'deco-' });
  }
  const [fill, fillName] = contentOf(d, 'DefaultFiller');
  const [air, airName] = contentOf(d, 'DeadAirFallback');
  const pickers = {};
  const picker = (key, value, label, prefix) => ({
    key, label: 'Treść', show: (v) => v[key.replace('_content', 'Mode')] === 'Override',
    render: () => { const p = contentPicker(value, { idPrefix: prefix, kinds: FILL_KINDS, label }); pickers[key] = p; return p; },
  });
  const v = {
    ...d,
    WatermarkIds: d.Watermarks.map((w) => w.Id), GraphicsElementIds: d.GraphicsElements.map((g) => g.Id),
    BreakContent: d.BreakContent.map((b) => ({
      Id: b.Id, ...emptyContent(b.CollectionType), CollectionId: b.Collection?.Id ?? null, MediaItemId: b.MediaItem?.MediaItemId ?? null,
      MultiCollectionId: b.MultiCollection?.Id ?? null, SmartCollectionId: b.SmartCollection?.Id ?? null, PlaylistId: b.Playlist?.Id ?? null,
      Name: b.Playlist?.Name ?? b.Collection?.Name ?? '', Placement: b.Placement,
    })),
  };
  const merges = (k) => (x) => ['Override', 'Merge'].includes(x[k]);
  return form('UpdateDeco', [
    { key: 'Name', label: 'Nazwa', required: true },
    { sec: 'Znak wodny' },
    { key: 'WatermarkMode', label: 'Tryb', optional: false, options: MODES4, hint: MODE_HINT },
    { key: 'WatermarkIds', label: 'Znaki wodne', kind: 'multi', options: watermarkChoices(ctx.watermarks), show: merges('WatermarkMode'), empty: 'brak znaków wodnych – załóż je w Znakach wodnych' },
    { key: 'UseWatermarkDuringFiller', label: 'W wypełniaczu', kind: 'check', text: 'pokazuj znak także w wypełniaczu', show: (x) => x.WatermarkMode === 'Override' },
    { sec: 'Grafika' },
    { key: 'GraphicsElementsMode', label: 'Tryb', optional: false, options: MODES4 },
    { key: 'GraphicsElementIds', label: 'Elementy', kind: 'multi', options: graphicsChoices(ctx.graphics), show: merges('GraphicsElementsMode'), empty: 'brak elementów graficznych' },
    { key: 'UseGraphicsElementsDuringFiller', label: 'W wypełniaczu', kind: 'check', text: 'pokazuj grafikę także w wypełniaczu', show: (x) => x.GraphicsElementsMode === 'Override' },
    { sec: 'Przerwy' },
    { key: 'BreakContentMode', label: 'Tryb', optional: false, options: MODES3 },
    { key: 'BreakContent', label: 'Treść przerw', render: (val) => breaksField(val), show: (x) => x.BreakContentMode === 'Override' },
    { sec: 'Wypełniacz domyślny' },
    { key: 'DefaultFillerMode', label: 'Tryb', optional: false, options: MODES3, hint: 'Czym legacy dopełnia blok, gdy jego pozycje skończą się przed końcem.' },
    picker('DefaultFiller_content', fill, fillName, 'dfill-cp-'),
    { key: 'DefaultFillerTrimToFit', label: 'Przycinanie', kind: 'check', text: 'przytnij ostatnią pozycję do końca bloku', show: (x) => x.DefaultFillerMode === 'Override' },
    { sec: 'Martwe powietrze' },
    { key: 'DeadAirFallbackMode', label: 'Tryb', optional: false, options: MODES3, hint: 'Co leci, gdy w danej godzinie nie ma żadnego bloku.' },
    picker('DeadAirFallback_content', air, airName, 'dair-cp-'),
  ], v, { idPrefix: 'deco-' });
}

const decos = crudView({
  title: 'Wystrój (deco)',
  desc: 'Oprawa bloku: znak wodny, grafika, przerwy, wypełniacz, martwe powietrze. Odpowiednik Decos.',
  listCmd: 'GetAllDecoGroups',
  needs: ['GetDecosByDecoGroupId', 'GetDecoById', 'CreateDecoGroup', 'DeleteDecoGroup', 'GetAllWatermarks', 'GetAllGraphicsElements'],
  commands: { create: 'CreateDeco', update: 'UpdateDeco', delete: 'DeleteDeco' },
  noun: { list: 'Deco', new: 'Nowe deco', add: '+ Nowe deco', create: 'Załóż deco', one: (d) => d.Name, title: (d) => `${d.DecoGroupName} / ${d.Name}` },
  load: async () => {
    const [groups, watermarks, graphics] = await Promise.all([cmd('GetAllDecoGroups'), cmd('GetAllWatermarks'), cmd('GetAllGraphicsElements')]);
    const lists = await Promise.all(groups.map((g) => cmd('GetDecosByDecoGroupId', { DecoGroupId: g.Id })));
    return { rows: lists.flat(), groups, watermarks, graphics };
  },
  // GetDecosByDecoGroupId nie dociąga treści (kolekcje, playlisty przerw) – do edycji i zapisu tylko pełne GetDecoById,
  // inaczej UpdateDeco skasowałby zapisaną treść (DOKUMENTACJA §5)
  loadOne: async (d) => ({ full: d ? await cmd('GetDecoById', { DecoId: d.Id }) : null }),
  id: (d) => d.Id,
  cols: [['DecoGroupName', 'Grupa'], ['Name', 'Deco', { class: 'w' }], [(d) => pl(d.WatermarkMode), 'Znak'], [(d) => pl(d.BreakContentMode), 'Przerwy'], [(d) => pl(d.DefaultFillerMode), 'Wypełniacz']],
  empty: 'Brak deco. Załóż grupę, potem deco.',
  headExtra: (ctx) => groupActions({ what: 'deco', create: 'CreateDecoGroup', del: 'DeleteDecoGroup', idKey: 'DecoGroupId', groups: ctx.groups, count: (g) => `${g.DecoCount} deco`, prefix: 'dg', placeholder: 'np. Oprawa INFO24' }),
  editor: (d, ctx) => { ctx.current = d; return decoEditor(d ? ctx.full : null, ctx); },
  beforeSave: (ctx, d) => { if (!ctx.current && d.DecoGroupId == null) throw new Error('Wybierz grupę (albo załóż ją przyciskiem „+ Grupa”).'); },
  create: (d) => ({ DecoGroupId: d.DecoGroupId, Name: d.Name.trim() }),
  createdId: (r) => r?.Id,
  update: (v, row, ctx) => {
    const d = ctx.full;
    const keep = (p) => contentArgs(p, contentOf(d, p)[0]);
    const fill = v.DefaultFillerMode === 'Override' ? contentArgs('DefaultFiller', v.DefaultFiller_content) : keep('DefaultFiller');
    const air = v.DeadAirFallbackMode === 'Override' ? contentArgs('DeadAirFallback', v.DeadAirFallback_content) : keep('DeadAirFallback');
    return {
      DecoId: d.Id, DecoGroupId: d.DecoGroupId, Name: v.Name.trim(),
      WatermarkMode: v.WatermarkMode, WatermarkIds: v.WatermarkIds, UseWatermarkDuringFiller: !!v.UseWatermarkDuringFiller,
      GraphicsElementsMode: v.GraphicsElementsMode, GraphicsElementIds: v.GraphicsElementIds, UseGraphicsElementsDuringFiller: !!v.UseGraphicsElementsDuringFiller,
      BreakContentMode: v.BreakContentMode,
      BreakContent: v.BreakContent.map((b) => ({ Id: b.Id || 0, CollectionType: b.CollectionType, CollectionId: b.CollectionId, MediaItemId: b.MediaItemId,
        MultiCollectionId: b.MultiCollectionId, SmartCollectionId: b.SmartCollectionId, PlaylistId: b.PlaylistId, Placement: b.Placement || 'BlockStart' })),
      DefaultFillerMode: v.DefaultFillerMode, ...fill, DefaultFillerTrimToFit: !!v.DefaultFillerTrimToFit,
      DeadAirFallbackMode: v.DeadAirFallbackMode, ...air,
    };
  },
  del: (d) => ({ DecoId: d.Id }),
  confirmDelete: (d) => `Usunąć deco „${d.Name}”? Zniknie też z szablonów deco i playoutów. Tego nie da się cofnąć.`,
});

// ---------- szablony deco ----------
const startMin = (i) => minutesOf(i.StartTime);
const endMin = (i) => { const s = startMin(i), e = minutesOf(i.EndTime); return e <= s ? e + 1440 : e; };

/** Pasek doby szablonu deco. */
export function decoTemplateBar(items, opts) {
  return dayBar(items.map((i) => ({ start: startMin(i), end: endMin(i), label: i.DecoName || `deco ${i.DecoId}`, cls: 'deco' })), opts);
}

const decoTemplates = crudView({
  title: 'Wystrój (deco)',
  desc: 'Szablon deco – które deco obowiązuje w których godzinach doby. Przypisuje się go razem z szablonem dnia w kalendarzu playoutu blokowego. Odpowiednik Deco Templates.',
  listCmd: 'GetAllDecoTemplateGroups',
  needs: ['GetDecoTemplatesByDecoTemplateGroupId', 'CreateDecoTemplateGroup', 'DeleteDecoTemplateGroup', 'GetDecoTemplateItems', 'GetAllDecoGroups', 'GetDecosByDecoGroupId'],
  commands: { create: 'CreateDecoTemplate', update: 'ReplaceDecoTemplateItems', delete: 'DeleteDecoTemplate' },
  noun: { list: 'Szablony deco', new: 'Nowy szablon deco', add: '+ Nowy szablon deco', create: 'Załóż szablon', one: (t) => t.Name, title: (t) => `${t.GroupName} / ${t.Name}` },
  load: async () => {
    const [groups, decoGroups] = await Promise.all([cmd('GetAllDecoTemplateGroups'), cmd('GetAllDecoGroups')]);
    const [lists, decoLists] = await Promise.all([
      Promise.all(groups.map((g) => cmd('GetDecoTemplatesByDecoTemplateGroupId', { DecoTemplateGroupId: g.Id }))),
      Promise.all(decoGroups.map((g) => cmd('GetDecosByDecoGroupId', { DecoGroupId: g.Id })))]);
    return { rows: lists.flat(), groups, decos: decoLists.flat() };
  },
  loadOne: async (t) => ({
    items: t ? (await cmd('GetDecoTemplateItems', { DecoTemplateId: t.Id })).map((i) => ({ DecoId: i.DecoId, DecoName: i.DecoName, StartTime: span(minutesOf(i.StartTime)), EndTime: span(minutesOf(i.EndTime)) }))
      .sort((a, b) => startMin(a) - startMin(b)) : [],
  }),
  id: (t) => t.Id,
  cols: [['GroupName', 'Grupa'], ['Name', 'Szablon deco', { class: 'w' }]],
  empty: 'Brak szablonów deco. Załóż grupę, potem szablon.',
  headExtra: (ctx) => groupActions({ what: 'szablonów deco', create: 'CreateDecoTemplateGroup', del: 'DeleteDecoTemplateGroup', idKey: 'DecoTemplateGroupId', groups: ctx.groups, count: (g) => `${g.DecoTemplateCount} szabl.`, prefix: 'dtg' }),
  editor: (t, ctx) => {
    ctx.current = t;
    return form('CreateDecoTemplate', [
      t ? null : { key: 'DecoTemplateGroupId', label: 'Grupa', options: ctx.groups.map((g) => [g.Id, g.Name]), optional: false, hint: ctx.groups.length ? null : 'Najpierw załóż grupę („+ Grupa”).' },
      { key: 'Name', label: 'Nazwa', required: true, placeholder: 'np. Dzień z paskiem' },
    ].filter(Boolean), { Name: t?.Name || '', DecoTemplateGroupId: ctx.groups[0]?.Id ?? null }, { idPrefix: 'dt-' });
  },
  beforeSave: (ctx, d) => {
    if (!ctx.current && d.DecoTemplateGroupId == null) throw new Error('Wybierz grupę (albo załóż ją przyciskiem „+ Grupa”).');
    if (ctx.itemsEd) ctx.itemsEd.commit();
  },
  create: (d) => ({ DecoTemplateGroupId: d.DecoTemplateGroupId, Name: d.Name.trim() }),
  createdId: (r) => r?.Id,
  update: (d, t, ctx) => ({
    DecoTemplateId: t.Id, DecoTemplateGroupId: t.DecoTemplateGroupId, Name: d.Name.trim(),
    Items: ctx.itemsEd.value().map((i) => ({ DecoId: i.DecoId, StartTime: i.StartTime, EndTime: i.EndTime })),
  }),
  del: (t) => ({ DecoTemplateId: t.Id }),
  confirmDelete: (t) => `Usunąć szablon deco „${t.Name}”? Zniknie też z kalendarzy playoutów. Tego nie da się cofnąć.`,
  below: (t, ctx) => {
    if (!t) return null;
    const bar = h('div', { id: 'dt-bar' });
    const draw = (items) => bar.replaceChildren(decoTemplateBar(items, { label: 'Szablon deco', empty: 'szablon pusty – dodaj deco' }));
    const decoName = (id) => { const d = ctx.decos.find((x) => x.Id === id); return d ? `${d.DecoGroupName} / ${d.Name}` : `deco ${id}`; };
    ctx.itemsEd = itemsEditor({
      items: ctx.items,
      addText: '+ Dodaj deco',
      empty: 'Szablon jest pusty. Dodaj deco na wybrane godziny.',
      onChange: draw,
      cols: [[(i) => hhmm(startMin(i)), 'Od', { class: 'num' }], [(i) => hhmm(endMin(i)), 'Do', { class: 'num' }], [(i) => decoName(i.DecoId), 'Deco', { class: 'w' }]],
      newItem: () => ({ DecoId: ctx.decos[0]?.Id ?? null, DecoName: ctx.decos[0]?.Name ?? '', StartTime: '00:00:00', EndTime: '00:00:00' }),
      edit: (it) => {
        const f = form('', [
          { key: 'DecoId', label: 'Deco', optional: false, options: ctx.decos.map((d) => [d.Id, `${d.DecoGroupName} / ${d.Name}`]), hint: ctx.decos.length ? null : 'Najpierw załóż deco w zakładce „Deco”.' },
          { key: 'StartTime', label: 'Od', kind: 'time', optional: false, placeholder: 'gg:mm' },
          { key: 'EndTime', label: 'Do', kind: 'time', optional: false, placeholder: 'gg:mm', hint: '„Do” wcześniejsze niż „Od” = przez północ; 00:00–00:00 = cała doba.' },
        ], it, { idPrefix: 'dti-' });
        return { el: f.el, read: () => { const v = f.read(); return { ...v, DecoName: ctx.decos.find((d) => d.Id === v.DecoId)?.Name ?? '', StartTime: parseTimeSpan(v.StartTime), EndTime: parseTimeSpan(v.EndTime) }; } };
      },
    });
    draw(ctx.items);
    return panel('Doba szablonu deco', h('span', { class: 'dim' }, 'zapis: „Zapisz” u góry'), bar, ctx.itemsEd.el, cmdNote('GetDecoTemplateItems', 'ReplaceDecoTemplateItems'));
  },
});

// ---------- ekran z zakładkami ----------
const TABS = [['Deco', decos], ['Szablony deco', decoTemplates]];

VIEWS.deco = async () => {
  const nodes = await TABS[TAB.i][1]();
  const tabs = h('div', { class: 'tabs page-tabs', role: 'tablist' }, TABS.map(([t], i) =>
    h('button', { type: 'button', role: 'tab', class: i === TAB.i ? 'on' : null, 'aria-selected': String(i === TAB.i), onclick: () => { TAB.i = i; route(); } }, t)));
  return [nodes[0], tabs, ...nodes.slice(1)];
};
