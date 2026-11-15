// Znaki wodne (R5): obraz (wgrywany do legacy albo logo kanału), położenie, rozmiar, krycie, tryb;
// podgląd na makiecie kadru 16:9. Odpowiednik Watermarks + Watermark Editor legacy.

import { VIEWS, S, h, cmd, pl, pill, panel, toast, upload, legacyUrl, IMAGE_TYPES } from '../core.js';
import { form } from '../form.js';
import { crudView } from '../parts.js';

const WM_DEFAULTS = {
  Name: '', Image: { Path: '', ContentType: '' }, Mode: 'Permanent', ImageSource: 'Custom', Location: 'BottomRight', Size: 'Scaled',
  Width: 15, HorizontalMargin: 5, VerticalMargin: 5, FrequencyMinutes: 15, DurationSeconds: 15, Opacity: 100,
  PlaceWithinSourceContent: false, OpacityExpression: '', ZIndex: 0,
};
// siatka 3×3 jak kadr: wiersze góra/środek/dół
const GRID = ['TopLeft', 'TopMiddle', 'TopRight', 'LeftMiddle', 'MiddleCenter', 'RightMiddle', 'BottomLeft', 'BottomMiddle', 'BottomRight'];
const FREQ = [5, 10, 15, 20, 30, 60].map((m) => [m, `co ${m} min`]);

/** Adres obrazu znaku w legacy (jak lista Watermarks legacy: artwork/watermarks/{plik}?contentType=…). */
export function watermarkSrc(img) {
  if (!img?.Path) return null;
  return legacyUrl(`artwork/watermarks/${img.Path}` + (img.ContentType ? `?contentType=${encodeURIComponent(img.ContentType)}` : ''));
}

/** Pole obrazu: wgranie pliku (SaveArtworkToDisk) + miniatura; onPreview(src) – podgląd na makiecie. */
export function imageField(kind, value, opts = {}) {
  let cur = { Path: value?.Path || '', ContentType: value?.ContentType || '' };
  const id = opts.id || 'f-image';
  const thumb = h('img', { class: 'thumb', alt: '' });
  const info = h('span', { class: 'dim' });
  const file = h('input', { type: 'file', id, accept: IMAGE_TYPES.join(','), 'aria-label': opts.label || 'Wgraj obraz' });
  const show = (src) => {
    thumb.hidden = !src;
    if (src) thumb.src = src;
    info.textContent = cur.Path ? (opts.describe ? opts.describe(cur) : `plik w legacy: ${cur.Path.slice(0, 12)}…`) : 'brak obrazu';
    if (opts.onPreview) opts.onPreview(src);
  };
  file.addEventListener('change', async () => {
    const f = file.files[0];
    if (!f) return;
    try {
      const name = await upload(kind, f);
      cur = { Path: opts.pathOf ? opts.pathOf(name) : name, ContentType: f.type };
      const reader = new FileReader();
      reader.onload = () => { show(reader.result); file.dispatchEvent(new Event('input', { bubbles: true })); };
      reader.readAsDataURL(f);
      toast('Obraz wgrany do legacy – zapisz, żeby go użyć.', 'ok');
    } catch (e) {
      toast('Nie udało się wgrać obrazu: ' + e.message, 'bad');
    }
    file.value = '';
  });
  const el = h('div', { class: 'imgfield' }, thumb, h('div', { class: 'bar' }, file, info),
    h('div', { class: 'hint' }, 'PNG, JPEG, GIF albo WebP, do 10 MB. Plik trafia do pamięci obrazów legacy.'));
  show(opts.src ? opts.src(cur) : null);
  return { el, read: () => cur, peek: () => cur };
}

/** Makieta kadru 16:9 z położeniem, rozmiarem, marginesami i kryciem znaku. */
function mockup() {
  const mark = h('div', { class: 'wm-mark' });
  const img = h('img', { alt: '' });
  const label = h('span');
  mark.append(img, label);
  const caption = h('div', { class: 'dim pad' });
  const frame = h('div', { class: 'wm-frame', role: 'img', 'aria-label': 'Podgląd położenia znaku wodnego w kadrze' }, h('span', { class: 'wm-safe' }), mark);
  let src = null;
  const update = (v) => {
    mark.hidden = v.Mode === 'None';
    const loc = v.Location || 'BottomRight';
    const col = /Left/.test(loc) ? 'l' : /Right/.test(loc) ? 'r' : 'c';
    const row = /^Top/.test(loc) ? 't' : /^Bottom/.test(loc) ? 'b' : 'm';
    const s = mark.style;
    s.left = s.right = s.top = s.bottom = s.transform = '';
    const hm = (v.HorizontalMargin ?? 0) + '%', vm = (v.VerticalMargin ?? 0) + '%';
    if (col === 'l') s.left = hm; else if (col === 'r') s.right = hm; else s.left = '50%';
    if (row === 't') s.top = vm; else if (row === 'b') s.bottom = vm; else s.top = '50%';
    s.transform = `translate(${col === 'c' ? '-50%' : '0'}, ${row === 'm' ? '-50%' : '0'})`;
    const actual = v.Size === 'ActualSize';
    s.width = actual ? (img.naturalWidth && src ? (img.naturalWidth / 1920) * 100 + '%' : '10%') : Math.max(1, Math.min(100, v.Width || 0)) + '%';
    s.opacity = String(Math.max(0, Math.min(100, v.Opacity ?? 100)) / 100);
    const logo = v.ImageSource === 'ChannelLogo';
    img.hidden = logo || !src;
    if (src && !logo) img.src = src;
    label.textContent = logo ? 'logo kanału' : src ? '' : 'znak';
    caption.textContent = v.Mode === 'None' ? 'Tryb „brak” – znak się nie pokazuje.'
      : v.Mode === 'Intermittent' ? `Pokazuje się co ${v.FrequencyMinutes} min na ${v.DurationSeconds} s.`
        : v.Mode === 'OpacityExpression' ? 'Krycie z wyrażenia – makieta pokazuje krycie bazowe.'
          : `Stale widoczny, krycie ${v.Opacity ?? 100}%.` + (S.status?.streamUrl ? '' : ' Panel bez --stream-url nie pokaże zapisanego obrazu z legacy (tylko świeżo wgrany).');
  };
  img.addEventListener('load', () => frame.dispatchEvent(new Event('wm-refresh')));
  return { el: h('div', null, frame, caption), update, setSrc: (x) => { src = x; } };
}

/** Wybór położenia: 9 przycisków w siatce kadru. */
function locationField(value) {
  let cur = value || 'BottomRight';
  const buttons = GRID.map((loc) => h('button', {
    type: 'button', class: loc === cur ? 'on' : null, title: pl(loc), 'aria-label': `Położenie: ${pl(loc)}`, 'aria-pressed': String(loc === cur),
    onclick: (e) => {
      cur = loc;
      buttons.forEach((b, i) => { b.classList.toggle('on', GRID[i] === cur); b.setAttribute('aria-pressed', String(GRID[i] === cur)); });
      e.currentTarget.dispatchEvent(new Event('change', { bubbles: true }));
    },
  }));
  const el = h('div', { class: 'wm-grid', id: 'f-Location', role: 'group', 'aria-label': 'Położenie' }, buttons);
  return { el, read: () => cur, peek: () => cur };
}

VIEWS.znaki = crudView({
  title: 'Znaki wodne',
  desc: 'Logo albo plansza rysowana przez Next na obrazie kanału. Gdy kanał ma grafikę VizRS, logo zwykle daje VizRS. Odpowiednik Watermarks w legacy.',
  listCmd: 'GetAllWatermarks',
  needs: ['GetAllChannels'],
  commands: { create: 'CreateWatermark', update: 'UpdateWatermark', delete: 'DeleteWatermark', copy: 'CopyWatermark' },
  noun: { list: 'Znaki wodne', new: 'Nowy znak wodny', add: '+ Nowy znak', create: 'Załóż znak', one: (w) => w.Name, title: (w) => w.Name },
  load: async () => {
    const [rows, channels] = await Promise.all([cmd('GetAllWatermarks'), cmd('GetAllChannels', { ShowDisabled: true })]);
    return { rows, channels };
  },
  id: (w) => w.Id,
  cols: ({ channels }) => [
    [(w) => { const src = w.ImageSource === 'Custom' ? watermarkSrc(w.Image) : null; return src ? h('img', { class: 'thumb sm', src, alt: '' }) : pl(w.ImageSource); }, 'Obraz'],
    ['Name', 'Nazwa'], [(w) => pl(w.Mode), 'Tryb'], [(w) => pl(w.Location), 'Położenie'],
    [(w) => (w.Size === 'Scaled' ? `${w.Width}%` : 'oryginalny'), 'Rozmiar'], [(w) => w.Opacity + '%', 'Krycie', { class: 'num' }],
    [(w) => { const n = channels.filter((c) => c.WatermarkId === w.Id).length; return n ? pill(String(n), 'info') : '—'; }, 'Kanały'],
  ],
  editor: (w) => {
    const base = { ...WM_DEFAULTS, ...(w || {}) };
    const mock = mockup();
    let f = null;
    const refresh = () => { if (f) mock.update(f.peek()); };
    const img = (v) => imageField('Watermark', v, {
      label: 'Wgraj obraz znaku',
      src: (cur) => watermarkSrc(cur),
      onPreview: (src) => { mock.setSrc(src); refresh(); },
    });
    f = form(w ? 'UpdateWatermark' : 'CreateWatermark', [
      { sec: 'Obraz' },
      { key: 'Name', label: 'Nazwa', required: true, placeholder: 'np. Logo INFO24' },
      { key: 'ImageSource', label: 'Źródło obrazu', options: [['Custom', pl('Custom')], ['ChannelLogo', pl('ChannelLogo')]] },
      { key: 'Image', label: 'Obraz', render: img, show: (v) => v.ImageSource === 'Custom' },
      { key: 'Mode', label: 'Tryb', options: ['None', 'Permanent', 'Intermittent', 'OpacityExpression'].map((m) => [m, pl(m)]) },
      { key: 'FrequencyMinutes', label: 'Pokazuj', options: FREQ, show: (v) => v.Mode === 'Intermittent' },
      { key: 'DurationSeconds', label: 'Na czas', unit: 's', show: (v) => v.Mode === 'Intermittent' },
      { key: 'OpacityExpression', label: 'Wyrażenie krycia', placeholder: 'np. if(lt(mod(time,60),10),1,0)', attrs: { class: 'mono' }, show: (v) => v.Mode === 'OpacityExpression', hint: 'Wyrażenie ffmpeg (zmienna time, wynik 0–1).' },
      { sec: 'Położenie i wygląd' },
      { key: 'Location', label: 'Położenie', render: locationField },
      { key: 'Size', label: 'Rozmiar', options: ['Scaled', 'ActualSize'].map((m) => [m, pl(m)]) },
      { key: 'Width', label: 'Szerokość', unit: '% kadru', show: (v) => v.Size === 'Scaled' },
      { label: 'Margines', group: [{ key: 'HorizontalMargin', label: 'Margines poziomy', attrs: { 'aria-label': 'Margines poziomy (%)' } }, { key: 'VerticalMargin', label: 'Margines pionowy', attrs: { 'aria-label': 'Margines pionowy (%)' } }], hint: 'Poziomy i pionowy, w % kadru.' },
      { key: 'Opacity', label: 'Krycie', unit: '%', attrs: { min: 0, max: 100 } },
      { key: 'PlaceWithinSourceContent', kind: 'check', label: 'W obrazie źródła', text: 'licz położenie od obrazu, nie od kadru z pasami' },
      { key: 'ZIndex', label: 'Warstwa (z-index)', hint: 'Przy kilku znakach: wyższy jest na wierzchu.' },
    ], base);
    f.el.addEventListener('change', refresh);
    f.el.addEventListener('input', refresh);
    mock.el.firstChild.addEventListener('wm-refresh', refresh);
    refresh();
    return {
      el: h('div', { class: 'wm-ed' }, f.el, panel('Podgląd w kadrze 16:9', null, mock.el)),
      read: () => {
        const d = f.read();
        if (d.Opacity < 0 || d.Opacity > 100) throw new Error('Pole „Krycie”: od 0 do 100.');
        if (d.ImageSource === 'Custom' && !d.Image?.Path) throw new Error('Pole „Obraz”: wgraj obraz albo wybierz logo kanału.');
        if (d.ImageSource !== 'Custom') d.Image = { Path: '', ContentType: '' };
        return d;
      },
    };
  },
  update: (d, w) => ({ ...d, Id: w.Id }),
  createdId: (r) => r?.WatermarkId,
  del: (w) => ({ WatermarkId: w.Id }),
  confirmDelete: (w, { channels }) => {
    const used = channels.filter((c) => c.WatermarkId === w.Id);
    return `Usunąć znak wodny „${w.Name}”?` + (used.length ? ` Kanały ${used.map((c) => c.Number).join(', ')} stracą znak.` : '') + ' Tego nie da się cofnąć.';
  },
  copy: (w, name) => ({ WatermarkId: w.Id, Name: name }),
  copiedId: (r) => r?.Id,
});

/** Opcje wyboru znaków wodnych z miniaturą (form.js, kind 'multi'): obraz własny albo napis „logo” (logo kanału). */
export const watermarkChoices = (list) => (list || []).map((w) => {
  const src = w.ImageSource === 'Custom' ? watermarkSrc(w.Image) : null;
  return [w.Id, w.Name, src ? { src } : { badge: w.ImageSource === 'ChannelLogo' ? 'logo' : 'znak' }];
});
