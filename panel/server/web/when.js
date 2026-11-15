// VizRS Live – „kiedy obowiązuje”: dni tygodnia, dni miesiąca, miesiące, zakres dat.
// Wspólne dla harmonogramów zastępczych playoutu klasycznego (ReplacePlayoutAlternateSchedule)
// i kalendarza szablonów playoutu blokowego (ReplacePlayoutTemplate) – legacy wybiera jedne i drugie
// tym samym selektorem (AlternateScheduleSelector): pierwsza pasująca pozycja wg kolejności wygrywa.

import { h, btn, pad2 } from './core.js';
import { form } from './form.js';

export const DAYS = [['Monday', 'pon'], ['Tuesday', 'wt'], ['Wednesday', 'śr'], ['Thursday', 'czw'], ['Friday', 'pt'], ['Saturday', 'sob'], ['Sunday', 'nd']];
export const ALL_DAYS = DAYS.map(([d]) => d);
export const MONTHS = ['sty', 'lut', 'mar', 'kwi', 'maj', 'cze', 'lip', 'sie', 'wrz', 'paź', 'lis', 'gru'];
export const range = (a, b) => Array.from({ length: b - a + 1 }, (_, i) => a + i);
export const ALL_MDAYS = range(1, 31);
export const ALL_MONTHS = range(1, 12);
// DayOfWeek z .NET: 0 = niedziela
const JS_DAY = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];

/** „1-5, 7, 9-10” z listy liczb; `każdy`, gdy są wszystkie. */
export function rangeText(nums, all) {
  const l = [...new Set(nums)].sort((a, b) => a - b);
  if (l.length === all) return 'każdy';
  if (!l.length) return 'żaden';
  const out = [];
  for (let i = 0; i < l.length; i++) {
    const s = l[i];
    while (i < l.length - 1 && l[i + 1] === l[i] + 1) i++;
    out.push(s === l[i] ? String(s) : `${s}-${l[i]}`);
  }
  return out.join(', ');
}
export const daysText = (d) => (d.length === 7 ? 'każdy' : d.length ? DAYS.filter(([k]) => d.includes(k)).map(([, t]) => t).join(', ') : 'żaden');
export const monthsText = (m) => (m.length === 12 ? 'każdy' : m.length ? [...m].sort((a, b) => a - b).map((x) => MONTHS[x - 1]).join(', ') : 'żaden');
const dayMonth = (d, m, y) => `${d}.${pad2(m)}${y ? '.' + y : ''}`;
export const dateRangeText = (a) => (a.LimitToDateRange ? `${dayMonth(a.StartDay, a.StartMonth, a.StartYear)} – ${dayMonth(a.EndDay, a.EndMonth, a.EndYear)}` : '—');

/** Pola „kiedy” z pozycji legacy (kopie list, wartości domyślne). */
export const fromWhen = (a) => ({
  DaysOfWeek: [...(a.DaysOfWeek || [])], DaysOfMonth: [...(a.DaysOfMonth || [])], MonthsOfYear: [...(a.MonthsOfYear || [])],
  LimitToDateRange: !!a.LimitToDateRange, StartMonth: a.StartMonth || 1, StartDay: a.StartDay || 1, StartYear: a.StartYear ?? null,
  EndMonth: a.EndMonth || 12, EndDay: a.EndDay || 31, EndYear: a.EndYear ?? null,
});
/** Nowa pozycja – jak w legacy: każdy dzień i miesiąc, bez zakresu dat. */
export const newWhen = (days = ALL_DAYS) => fromWhen({ DaysOfWeek: days, DaysOfMonth: ALL_MDAYS, MonthsOfYear: ALL_MONTHS, StartMonth: 1, StartDay: 1, EndMonth: 1, EndDay: 1 });

/** Czy pozycja obowiązuje w danym dniu (jak AlternateScheduleSelector w legacy). */
export function matchesDate(a, d) {
  if (!a.DaysOfWeek.includes(JS_DAY[d.getDay()])) return false;
  if (!a.DaysOfMonth.includes(d.getDate())) return false;
  if (!a.MonthsOfYear.includes(d.getMonth() + 1)) return false;
  if (!a.LimitToDateRange) return true;
  if (a.StartYear != null && a.EndYear != null) {
    const s = new Date(a.StartYear, a.StartMonth - 1, a.StartDay), e = new Date(a.EndYear, a.EndMonth - 1, a.EndDay);
    const x = new Date(d.getFullYear(), d.getMonth(), d.getDate());
    return x >= s && x <= e;
  }
  const md = (m, dd) => m * 100 + dd;
  const x = md(d.getMonth() + 1, d.getDate()), s = md(a.StartMonth, a.StartDay), e = md(a.EndMonth, a.EndDay);
  return s <= e ? x >= s && x <= e : x >= s || x <= e;  // zakres przez Nowy Rok
}
/** Pierwsza pasująca pozycja (wg kolejności) dla dnia; -1, gdy żadna. */
export const pickFor = (items, d) => items.findIndex((a) => matchesDate(a, d));

/** Grupa pól wyboru ze skrótami (np. „robocze”, „weekend”); read() wymaga co najmniej jednej wartości. */
export function checks(id, label, options, value, shortcuts, tight) {
  const set = new Set(value || []);
  const boxes = options.map(([v, t]) => {
    const c = h('input', { type: 'checkbox', value: String(v), checked: set.has(v) });
    return [v, c, h('label', { class: 'chk' }, c, t)];
  });
  const el = h('div', { id, role: 'group', 'aria-label': label });
  const setAll = (vals) => {
    for (const [v, c] of boxes) c.checked = vals.includes(v);
    el.dispatchEvent(new Event('change', { bubbles: true }));
  };
  el.append(h('div', { class: 'multi' + (tight ? ' tight' : '') }, boxes.map((b) => b[2])),
    h('div', { class: 'bar' }, shortcuts.map(([t, vals]) => btn(t, () => setAll(vals)))));
  const peek = () => boxes.filter((b) => b[1].checked).map((b) => b[0]);
  return {
    el,
    peek,
    read() {
      const v = peek();
      if (!v.length) {
        boxes[0][1].focus();
        throw new Error(`Pole „${label}”: zaznacz co najmniej jedną wartość – inaczej pozycja nigdy się nie włączy.`);
      }
      return v;
    },
  };
}

/**
 * Edytor pozycji „co + kiedy”: pola `what` (np. wybór harmonogramu albo szablonu) + dni, miesiące, zakres dat.
 * prefix – przedrostek id kontrolek (np. 'alt-', 'pt-').
 */
export function whenEditor(item, what, prefix) {
  const monthOpts = MONTHS.map((m, i) => [i + 1, m]);
  const f = form('', [
    ...what,
    { sec: 'Kiedy' },
    { key: 'DaysOfWeek', label: 'Dni tygodnia', render: (v) => checks(prefix + 'DaysOfWeek', 'Dni tygodnia', DAYS, v,
      [['wszystkie', ALL_DAYS], ['robocze', ALL_DAYS.slice(0, 5)], ['weekend', ALL_DAYS.slice(5)], ['żaden', []]]) },
    { key: 'DaysOfMonth', label: 'Dni miesiąca', render: (v) => checks(prefix + 'DaysOfMonth', 'Dni miesiąca', ALL_MDAYS.map((d) => [d, String(d)]), v,
      [['wszystkie', ALL_MDAYS], ['żaden', []]], true) },
    { key: 'MonthsOfYear', label: 'Miesiące', render: (v) => checks(prefix + 'MonthsOfYear', 'Miesiące', monthOpts, v, [['wszystkie', ALL_MONTHS], ['żaden', []]]) },
    { key: 'LimitToDateRange', label: 'Zakres dat', kind: 'check', text: 'tylko w zakresie dat' },
    { label: 'Od', show: (v) => v.LimitToDateRange, group: [
      { key: 'StartDay', label: 'Od: dzień', kind: 'number', step: 1, optional: false, attrs: { min: 1, max: 31, 'aria-label': 'Od: dzień' } },
      { key: 'StartMonth', label: 'Od: miesiąc', options: monthOpts, optional: false, attrs: { 'aria-label': 'Od: miesiąc' } },
      { key: 'StartYear', label: 'Od: rok', kind: 'number', step: 1, placeholder: 'rok', attrs: { 'aria-label': 'Od: rok' } }] },
    { label: 'Do', show: (v) => v.LimitToDateRange, hint: 'Bez roku zakres powtarza się co roku (może przechodzić przez Nowy Rok). Rok podaj w obu polach albo w żadnym.', group: [
      { key: 'EndDay', label: 'Do: dzień', kind: 'number', step: 1, optional: false, attrs: { min: 1, max: 31, 'aria-label': 'Do: dzień' } },
      { key: 'EndMonth', label: 'Do: miesiąc', options: monthOpts, optional: false, attrs: { 'aria-label': 'Do: miesiąc' } },
      { key: 'EndYear', label: 'Do: rok', kind: 'number', step: 1, placeholder: 'rok', attrs: { 'aria-label': 'Do: rok' } }] },
  ], item, { idPrefix: prefix });
  return {
    el: f.el,
    read() {
      const v = f.read();
      if (v.LimitToDateRange) {
        for (const k of ['StartDay', 'EndDay']) if (!(v[k] >= 1 && v[k] <= 31)) throw new Error('Zakres dat: dzień od 1 do 31.');
        if ((v.StartYear == null) !== (v.EndYear == null)) throw new Error('Zakres dat: podaj rok w obu polach albo w żadnym.');
      }
      return v;
    },
  };
}
