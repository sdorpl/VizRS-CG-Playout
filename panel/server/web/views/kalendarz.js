// Kalendarz tygodnia playoutu blokowego (R6): szablony dnia przypisane do dni (z szablonem deco) i deco domyślne.
// Odpowiednik Playout Templates Editor i Block Playout Editor (deco domyślne) w legacy. Wybór szablonu na dzień –
// jak w legacy: pierwsza pasująca pozycja wg kolejności (when.js). Zmiany wchodzą na antenę po przebudowie playoutu.

import { h, btn, panel, pill, cmd, pl, pad2, cmdNote } from '../core.js';
import { form } from '../form.js';
import { itemsEditor, dayBar, minutesOf, hhmm } from '../parts.js';
import { fromWhen, newWhen, whenEditor, pickFor, daysText, rangeText, monthsText, dateRangeText, DAYS } from '../when.js';

export const CAL_COMMANDS = ['GetPlayoutTemplates', 'ReplacePlayoutTemplateItems', 'GetDecoByPlayoutId', 'UpdateDefaultDeco', 'GetAllTemplates', 'GetTemplateItems',
  'GetAllBlocks', 'GetBlockItems', 'GetAllDecoTemplateGroups', 'GetDecoTemplatesByDecoTemplateGroupId', 'GetDecoTemplateItems', 'GetAllDecoGroups', 'GetDecosByDecoGroupId'];

const fromVm = (t) => ({ Id: t.Id, TemplateId: t.Template?.Id ?? null, DecoTemplateId: t.DecoTemplate?.Id ?? null, ...fromWhen(t) });
// widok kalendarza: siatka godziny × dni (makieta) albo paski doby, pamiętany w przeglądarce
const CAL_KEY = 'vizrs-live-cal-view';
const calMode = (set) => {
  try { if (set) localStorage.setItem(CAL_KEY, set); return localStorage.getItem(CAL_KEY) || 'siatka'; } catch { return set || 'siatka'; }
};
/** Dni tygodnia (indeksy 0–6) jako „pon–pt”, „sob, nd”. */
const daysSpan = (idx) => {
  const runs = [];
  for (const i of idx) { const r = runs[runs.length - 1]; if (r && r[1] === i - 1) r[1] = i; else runs.push([i, i]); }
  return runs.map(([a, b]) => (b - a >= 2 ? `${DAYS[a][1]}–${DAYS[b][1]}` : a === b ? DAYS[a][1] : `${DAYS[a][1]}, ${DAYS[b][1]}`)).join(', ');
};
/** Numer tygodnia ISO 8601. */
const isoWeek = (d) => {
  const x = new Date(Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()));
  x.setUTCDate(x.getUTCDate() + 3 - ((x.getUTCDay() + 6) % 7));
  const y = new Date(Date.UTC(x.getUTCFullYear(), 0, 4));
  return 1 + Math.round(((x - y) / 864e5 - 3 + ((y.getUTCDay() + 6) % 7)) / 7);
};
const MONTHS = ['stycznia', 'lutego', 'marca', 'kwietnia', 'maja', 'czerwca', 'lipca', 'sierpnia', 'września', 'października', 'listopada', 'grudnia'];
const weekStart = (d) => { const x = new Date(d.getFullYear(), d.getMonth(), d.getDate()); x.setDate(x.getDate() - ((x.getDay() + 6) % 7)); return x; };

/** Dane kalendarza dla playoutu blokowego (dołożone do ctx edytora playoutu). */
export async function loadCalendar(p) {
  const [pts, templates, blocks, dtGroups, decoGroups, deco] = await Promise.all([
    cmd('GetPlayoutTemplates', { PlayoutId: p.PlayoutId }), cmd('GetAllTemplates'), cmd('GetAllBlocks'), cmd('GetAllDecoTemplateGroups'), cmd('GetAllDecoGroups'),
    cmd('GetDecoByPlayoutId', { PlayoutId: p.PlayoutId }).catch(() => null)]);
  const [dts, decos] = await Promise.all([
    Promise.all(dtGroups.map((g) => cmd('GetDecoTemplatesByDecoTemplateGroupId', { DecoTemplateGroupId: g.Id }))).then((l) => l.flat()),
    Promise.all(decoGroups.map((g) => cmd('GetDecosByDecoGroupId', { DecoGroupId: g.Id }))).then((l) => l.flat())]);
  const [tItems, dtItems] = await Promise.all([
    Promise.all(templates.map((t) => cmd('GetTemplateItems', { TemplateId: t.Id }).then((r) => [t.Id, r]))).then((l) => new Map(l)),
    Promise.all(dts.map((t) => cmd('GetDecoTemplateItems', { DecoTemplateId: t.Id }).then((r) => [t.Id, r]))).then((l) => new Map(l))]);
  const items = [...pts].sort((a, b) => a.Index - b.Index).map(fromVm);
  const defaultDeco = deco?.Id ?? null;
  return { cal: { items, before: JSON.stringify(items), templates, blocks, dts, decos, tItems, dtItems, defaultDeco, week: weekStart(new Date()) } };
}

/** Panele kalendarza pod edytorem playoutu. */
export function calendarPanels(p, ctx) {
  const c = ctx.cal;
  const tName = (id) => { const t = c.templates.find((x) => x.Id === id); return t ? `${t.GroupName} / ${t.Name}` : id == null ? '—' : `szablon ${id}`; };
  const dtName = (id) => { const t = c.dts.find((x) => x.Id === id); return t ? `${t.GroupName} / ${t.Name}` : id == null ? '—' : `szablon deco ${id}`; };
  const short = (list, id) => list.find((x) => x.Id === id)?.Name ?? String(id);
  const week = h('div', { class: 'week', id: 'cal-week' });
  const grid = h('div', { class: 'tw', id: 'cal-grid' });
  const tpills = h('div', { class: 'bar pad', id: 'cal-tpl' });
  const title = h('span', { class: 'dim', id: 'cal-range' });
  const detail = h('div', { id: 'cal-block' });
  const bItems = new Map(); // BlockId → pozycje (GetBlockItems), wczytywane przy pierwszym kliknięciu
  let picked = null;
  // szczegóły bloku pod siatką (jak w makiecie): długość, godziny, pozycje, deco w tej godzinie
  const showBlock = async (s, day, decos) => {
    picked = s;
    const blk = c.blocks.find((b) => b.Id === s.id);
    const head = h('div', { class: 'sec' }, `Blok „${s.label}” · ${blk?.Minutes ?? s.end - s.start} min · ${day} ${hhmm(s.start)}–${hhmm(s.end)}`);
    const deco = decos.find((d) => d.start <= s.start && d.end > s.start);
    const list = h('div', null, 'wczytuję…');
    detail.replaceChildren(h('div', { class: 'form cal-detail' }, head,
      h('label', null, 'Pozycje'), list,
      h('label', null, 'Wystrój'), h('div', null, deco ? `deco „${deco.label}” (z szablonu deco)` : c.defaultDeco != null ? 'deco domyślne playoutu' : 'bez deco',
        ' · ', h('a', { href: '#deco' }, 'Wystrój (deco)')),
      h('span'), h('div', null, h('a', { href: '#bloki' }, 'Edytuj w Bloki i szablony'))));
    try {
      if (!bItems.has(s.id)) bItems.set(s.id, await cmd('GetBlockItems', { BlockId: s.id }));
      if (picked !== s) return;
      const items = [...bItems.get(s.id)].sort((a, b) => a.Index - b.Index);
      const name = (i) => i.Collection?.Name ?? i.SmartCollection?.Name ?? i.MultiCollection?.Name ?? i.MediaItem?.Name ?? i.SearchTitle ?? pl(i.CollectionType);
      list.replaceChildren(...(items.length ? items.map((i, n) => h('div', null, `${n + 1}. ${name(i)} `, h('span', { class: 'dim' }, `(${pl(i.CollectionType)}, ${pl(i.PlaybackOrder)})`)))
        : [h('span', { class: 'err' }, 'blok bez pozycji – nic nie nada')]));
    } catch (e) { list.replaceChildren(h('span', { class: 'err' }, 'Legacy odrzucił: ' + e.message)); }
  };

  // dzień tygodnia (i = -1 to niedziela poprzedniego tygodnia – tylko dla bloków przechodzących przez północ)
  const dayData = (items, i) => {
    const d = new Date(c.week); d.setDate(d.getDate() + i);
    const k = pickFor(items, d);
    const it = items[k];
    const blocks = it ? (c.tItems.get(it.TemplateId) || []).map((x) => {
      const s = minutesOf(x.StartTime);
      const len = c.blocks.find((b) => b.Id === x.BlockId)?.Minutes ?? Math.max(1, minutesOf(x.EndTime) - s);
      return { start: s, end: s + len, label: x.BlockName, id: x.BlockId };
    }) : [];
    const decos = it && it.DecoTemplateId != null ? (c.dtItems.get(it.DecoTemplateId) || []).map((x) => {
      const s = minutesOf(x.StartTime); let e = minutesOf(x.EndTime); if (e <= s) e += 1440;
      return { start: s, end: e, label: x.DecoName, cls: 'deco' };
    }) : [];
    return { d, k, it, blocks, decos, today: d.toDateString() === new Date().toDateString() };
  };
  const dayHead = (x, i) => [`${DAYS[i][1]} ${pad2(x.d.getDate())}.${pad2(x.d.getMonth() + 1)}`,
    h('span', { class: x.it ? null : 'err', title: x.it ? tName(x.it.TemplateId) : null }, x.it ? `${x.k + 1}. ${short(c.templates, x.it.TemplateId)}` : 'brak szablonu')];

  const drawBars = (days) => week.replaceChildren(...days.flatMap((x, i) => [
    h('div', { class: 'day' + (x.today ? ' today' : '') },
      h('b', null, dayHead(x, i)[0]), dayHead(x, i)[1],
      x.it && x.it.DecoTemplateId != null ? h('span', { class: 'dim', title: dtName(x.it.DecoTemplateId) }, 'deco: ' + short(c.dts, x.it.DecoTemplateId)) : null),
    h('div', { class: 'bars' },
      dayBar(x.blocks, { label: `Bloki ${DAYS[i][1]}`, empty: x.it ? 'szablon bez bloków' : 'nic nie zaplanowane – martwe powietrze' }),
      dayBar(x.decos, { cls: 'thin', ruler: false, label: `Deco ${DAYS[i][1]}`, empty: c.defaultDeco != null ? 'deco domyślne' : 'bez deco' }))]));

  // siatka godziny × dni (jak w makiecie): bloki jako prostokąty, kolor stały dla bloku, pasek deco z prawej, linia „teraz”
  const drawGrid = (days, prev) => {
    const segs = days.map(() => []), dsegs = days.map(() => []);
    const put = (arr, i, s) => {
      if (s.end <= 1440) { if (i >= 0) arr[i].push(s); return; }
      if (i >= 0) arr[i].push({ ...s, end: 1440, more: ' (dalej po północy)' });
      if (i < 6) arr[i + 1].push({ ...s, start: 0, end: s.end - 1440, more: ' (od poprzedniego dnia)', spill: true });
    };
    [prev, ...days].forEach((x, j) => { x.blocks.forEach((s) => put(segs, j - 1, s)); x.decos.forEach((s) => put(dsegs, j - 1, s)); });
    const own = segs.flat().filter((s) => !s.spill);
    const all = own.length ? own : segs.flat();
    let r0 = all.length ? Math.floor(Math.min(...all.map((s) => s.start)) / 60) * 60 : 0;
    let r1 = all.length ? Math.ceil(Math.max(...all.map((s) => s.end)) / 60) * 60 : 1440;
    if (r1 - r0 < 240) { r1 = Math.min(1440, r0 + 240); r0 = Math.max(0, r1 - 240); }
    const hours = (r1 - r0) / 60, px = hours > 12 ? 24 : 36, pct = (m) => ((m - r0) / (r1 - r0)) * 100;
    const hrs = h('div', { class: 'hrs' });
    for (let m = r0; m < r1; m += 60) { const t = h('span', null, hhmm(m)); t.style.height = px + 'px'; hrs.append(t); }
    const now = new Date(), nowMin = now.getHours() * 60 + now.getMinutes();
    const spills = days.map(() => []);
    const cols = days.map((x, i) => {
      const col = h('div', { class: 'gday' + (x.today ? ' today' : ''), role: 'img', 'aria-label': `Bloki ${DAYS[i][1]}` });
      col.style.height = hours * px + 'px';
      col.style.backgroundSize = `100% ${px}px`;
      for (const s of dsegs[i]) {
        const e = h('div', { class: 'gdeco', title: `deco ${hhmm(s.start)}–${hhmm(s.end)} ${s.label}${s.more || ''}` });
        e.style.top = Math.max(0, pct(s.start)) + '%'; e.style.height = Math.min(100, pct(s.end)) - Math.max(0, pct(s.start)) + '%';
        col.append(e);
      }
      for (const s0 of segs[i]) {
        if (s0.end <= r0) { // ogon bloku z poprzedniego dnia poza zakresem godzin – znacznik u góry
          spills[i].push(h('span', { class: 'gspill', title: `00:00–${hhmm(s0.end)} ${s0.label}${s0.more}` }, `↑ ${s0.label} do ${hhmm(s0.end)}`));
          continue;
        }
        const s = { ...s0, start: Math.max(s0.start, r0) };
        const hpx = ((s.end - s.start) / 60) * px, tall = hpx >= 28;
        const b = h('div', { class: `gblk c${(s.id ?? 0) % 4}` + (hpx < 18 ? ' short' : ''), title: `${hhmm(s.start)}–${hhmm(s.end)} ${s.label}${s.more || ''}` },
          h('b', null, s.label), tall ? h('span', null, `${hhmm(s.start)}–${hhmm(s.end)}`) : null);
        b.tabIndex = 0;
        b.setAttribute('role', 'button');
        const open = () => showBlock(s0, `${DAYS[i][1]} ${pad2(x.d.getDate())}.${pad2(x.d.getMonth() + 1)}`, dsegs[i]);
        b.addEventListener('click', open);
        b.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); open(); } });
        b.style.top = pct(s.start) + '%';
        b.style.height = `calc(${pct(s.end) - pct(s.start)}% - 1px)`;
        col.append(b);
      }
      if (!segs[i].some((s) => s.end > r0)) col.append(h('span', { class: 'gempty' + (x.it ? '' : ' err') }, x.it ? 'szablon bez bloków' : 'martwe powietrze'));
      if (x.today && nowMin >= r0 && nowMin < r1) { const n = h('div', { class: 'gnow', title: 'teraz' }); n.style.top = pct(nowMin) + '%'; col.append(n); }
      return col;
    });
    const heads = days.map((x, i) => { const [d, t] = dayHead(x, i); return h('div', { class: 'hd' + (x.today ? ' today' : '') }, h('b', null, d), t, ...spills[i]); });
    grid.replaceChildren(h('div', { class: 'wgrid' }, h('div', { class: 'hd' }), ...heads, hrs, ...cols));
    // pigułki szablonów: który szablon w które dni tego tygodnia
    const byK = new Map();
    days.forEach((x, i) => { const key = x.it ? x.k : -1; byK.set(key, [...(byK.get(key) || []), i]); });
    tpills.replaceChildren(...[...byK].map(([k, idx]) => (k < 0
      ? pill('brak szablonu: ' + daysSpan(idx), 'warn')
      : pill(`„${short(c.templates, days[idx[0]].it.TemplateId)}”: ${daysSpan(idx)}`))));
  };

  const drawWeek = (items) => {
    const days = [0, 1, 2, 3, 4, 5, 6].map((i) => dayData(items, i));
    drawBars(days);
    drawGrid(days, dayData(items, -1));
    const end = new Date(c.week); end.setDate(end.getDate() + 6);
    const sameM = end.getMonth() === c.week.getMonth();
    title.textContent = `tydzień ${isoWeek(c.week)} (${c.week.getDate()}${sameM ? '' : ' ' + MONTHS[c.week.getMonth()]}–${end.getDate()} ${MONTHS[end.getMonth()]} ${end.getFullYear()})`;
    picked = null; detail.replaceChildren();
  };
  // przełącznik Siatka / Paski, pamiętany w przeglądarce
  let mode = calMode();
  const modeBtns = [['siatka', 'Siatka'], ['paski', 'Paski']].map(([m, t]) => {
    const b = btn(t, () => { mode = calMode(m); showMode(); });
    b.dataset.mode = m;
    return b;
  });
  const barsBox = h('div', null, week, h('div', { class: 'hint pad' }, 'Górny pasek – bloki szablonu dnia na całej dobie, dolny – deco z szablonu deco.'));
  const gridBox = h('div', null, tpills, grid, detail, h('div', { class: 'hint pad' }, 'Bloki szablonu dnia na osi godzin (zakres z bloków tygodnia); kliknij blok, by zobaczyć jego pozycje; wąski pasek z prawej strony dnia – deco z szablonu deco; czerwona linia – teraz.'));
  const showMode = () => {
    for (const b of modeBtns) { const on = b.dataset.mode === mode; b.classList.toggle('on', on); b.setAttribute('aria-pressed', String(on)); }
    gridBox.hidden = mode !== 'siatka'; barsBox.hidden = mode === 'siatka';
  };
  showMode();
  const shift = (n) => { c.week = new Date(c.week); c.week.setDate(c.week.getDate() + 7 * n); drawWeek(ctx.calEd.value()); };

  ctx.calEd = itemsEditor({
    items: c.items,
    copy: true,
    addText: '+ Dodaj szablon dnia',
    empty: 'Brak szablonów – playout blokowy nic nie nada. Dodaj szablon dnia (np. „robocze” i osobny na weekend).',
    onChange: drawWeek,
    cols: [[(t) => tName(t.TemplateId), 'Szablon dnia', { class: 'w' }], [(t) => dtName(t.DecoTemplateId), 'Szablon deco'],
      [(t) => daysText(t.DaysOfWeek), 'Dni tygodnia'], [(t) => rangeText(t.DaysOfMonth, 31), 'Dni miesiąca'], [(t) => monthsText(t.MonthsOfYear), 'Miesiące'], [dateRangeText, 'Zakres dat']],
    newItem: () => ({ Id: 0, TemplateId: c.templates[0]?.Id ?? null, DecoTemplateId: null, ...newWhen() }),
    edit: (t) => whenEditor(t, [
      { key: 'TemplateId', label: 'Szablon dnia', options: c.templates.map((x) => [x.Id, `${x.GroupName} / ${x.Name}`]), optional: false,
        hint: c.templates.length ? null : 'Najpierw załóż szablon dnia (Bloki i szablony → Szablony dnia).' },
      { key: 'DecoTemplateId', label: 'Szablon deco', kind: 'pick', options: c.dts.map((x) => [x.Id, `${x.GroupName} / ${x.Name}`]), none: 'bez szablonu deco (deco domyślne)' },
    ], 'pt-'),
  });
  const decoForm = form('UpdateDefaultDeco', [
    { key: 'DecoId', label: 'Deco domyślne', kind: 'pick', options: c.decos.map((d) => [d.Id, `${d.DecoGroupName} / ${d.Name}`]), none: 'bez deco',
      hint: 'Obowiązuje w godzinach, których nie pokrywa szablon deco dnia.' },
  ], { DecoId: c.defaultDeco }, { idPrefix: 'cal-' });
  ctx.calDeco = decoForm;
  drawWeek(c.items);
  return [
    panel(`Kalendarz tygodnia · ${p.ChannelName}`, [title, btn('‹ tydzień', () => shift(-1)), btn('dziś', () => { c.week = weekStart(new Date()); drawWeek(ctx.calEd.value()); }), btn('tydzień ›', () => shift(1)),
      ...modeBtns], gridBox, barsBox, h('div', { class: 'hint pad' }, 'Podgląd liczy się z bieżących (także niezapisanych) przypisań szablonów dni.'),
      cmdNote('GetPlayoutTemplates', 'GetTemplateItems', 'GetBlockItems', 'GetDecoTemplateItems')),
    panel('Szablony dni', h('span', { class: 'dim' }, 'pierwszy pasujący wygrywa; zapis razem z playoutem („Zapisz”), na antenie po przebudowie'),
      ctx.calEd.el, decoForm.el, cmdNote('ReplacePlayoutTemplateItems', 'GetDecoByPlayoutId', 'UpdateDefaultDeco')),
  ];
}

/** Zapis kalendarza (po „Zapisz” playoutu); zwraca true, gdy coś się zmieniło. */
export async function saveCalendar(id, ctx) {
  const c = ctx.cal;
  let changed = false;
  const items = ctx.calEd.value();
  if (JSON.stringify(items) !== c.before) {
    if (items.some((t) => t.TemplateId == null)) throw new Error('Kalendarz: każda pozycja musi mieć szablon dnia.');
    try { await cmd('ReplacePlayoutTemplateItems', { PlayoutId: id, Items: items.map((t, i) => ({ ...t, Index: i })) }); }
    catch (e) { throw new Error('Legacy odrzucił kalendarz szablonów: ' + e.message); }
    changed = true;
  }
  const deco = ctx.calDeco.read().DecoId;
  if (deco !== c.defaultDeco) {
    try { await cmd('UpdateDefaultDeco', { PlayoutId: id, DecoId: deco }); }
    catch (e) { throw new Error('Legacy odrzucił deco domyślne: ' + e.message); }
    changed = true;
  }
  return changed;
}
