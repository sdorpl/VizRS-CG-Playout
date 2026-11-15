// Widok „Kanały” (lista + edytor). Profile kodowania i znaki wodne: views/profile.js, views/znaki.js.

import { S, VIEWS, h, pill, btn, cmd, pl, legacyUrl, dialog, toast, route, has } from '../core.js';
import { form } from '../form.js';
import { crudView } from '../parts.js';
import { imageField } from './znaki.js';

// --- kanały: pełna edycja ---
const CHANNEL_DEFAULTS = {
  Name: '', Number: '', Group: 'VizRS', Categories: '', FFmpegProfileId: 0, SlugSeconds: null,
  Logo: { Path: '', ContentType: '' }, StreamSelectorMode: 'Default', StreamSelector: '', PreferredAudioLanguageCode: '', PreferredAudioTitle: '',
  PlayoutSource: 'Generated', PlayoutMode: 'Continuous', MirrorSourceChannelId: null, PlayoutOffset: null, StreamingEngine: 'Next',
  NextEngineTextSubtitleMode: 'Burn', StreamingMode: 'HttpLiveStreamingSegmenter', WatermarkId: null, FallbackFillerId: null,
  PreferredSubtitleLanguageCode: '', SubtitleMode: 'None', MusicVideoCreditsMode: 'None', MusicVideoCreditsTemplate: '',
  SongVideoMode: 'Default', TranscodeMode: 'OnDemand', IdleBehavior: 'StopOnDisconnect', IsEnabled: true, ShowInEpg: true,
};
const chanName = (c) => `Kanał ${c.Number} · ${c.Name}`;

/**
 * Logo kanału: wgrany obraz (SaveArtworkToDisk, Logo → „iptv/logos/{plik}”, jak Channel Editor legacy)
 * albo zewnętrzny adres URL. Niezmienione logo zachowuje typ zawartości z legacy.
 */
function logoField(logo) {
  const before = { Path: logo?.Path || '', ContentType: logo?.ContentType || '' };
  const external = /^https?:\/\//.test(before.Path);
  const url = h('input', { id: 'f-LogoPath', value: external ? before.Path : '', placeholder: 'albo adres https://…/logo.png', 'aria-label': 'Logo (adres URL)' });
  const img = imageField('Logo', external ? null : before, {
    id: 'f-LogoFile', label: 'Wgraj logo',
    pathOf: (name) => `iptv/logos/${name}`,
    src: (cur) => legacyUrl(cur.Path),
    describe: (cur) => `plik w legacy: ${cur.Path.replace('iptv/logos/', '').slice(0, 12)}…`,
    onPreview: (src) => { if (src?.startsWith('data:')) url.value = ''; },
  });
  const read = () => {
    const u = url.value.trim();
    if (u) return u === before.Path ? before : { Path: u, ContentType: '' };
    const cur = img.read();
    return cur.Path === before.Path ? before : cur;
  };
  return { el: h('div', { class: 'picker' }, img.el, url), read, peek: read };
}

/** Numer kanału w legacy: liczba z kropką dziesiętną (legacy liczy z niego SortNumber; „1.5” – podkanał). */
export const validNumber = (n) => /^\d+(\.\d+)?$/.test(n);

/**
 * Sprawdza nową numerację przed wysłaniem (K3). `rows` = [{Id, Name, OriginalNumber, Number}].
 * Zwraca { errors: [tekst], changed: [wiersze ze zmianą] } – duplikaty i złe numery blokują zapis.
 */
export function checkNumbers(rows) {
  const errors = [];
  const seen = new Map();
  for (const r of rows) {
    const n = String(r.Number).trim();
    if (!validNumber(n)) errors.push(`„${r.Name}”: zły numer „${n}” (liczba, np. 12 albo 12.1)`);
    if (seen.has(n)) errors.push(`numer ${n} mają „${seen.get(n)}” i „${r.Name}”`);
    else seen.set(n, r.Name);
  }
  const changed = rows.filter((r) => String(r.Number).trim() !== r.OriginalNumber).map((r) => ({ ...r, Number: String(r.Number).trim() }));
  return { errors, changed };
}

/** Okno „Numeracja kanałów” – odpowiednik Edit Channel Numbers legacy (`GetAllChannelsForSort`, `UpdateChannelNumbers`). */
async function renumber() {
  let list;
  try { list = await cmd('GetAllChannelsForSort'); } catch (e) { toast('Legacy nie podał listy kanałów: ' + e.message, 'bad'); return; }
  const rows = list.map((c) => ({ Id: c.Id, Name: c.Name, OriginalNumber: c.OriginalNumber ?? c.Number, Number: c.Number }));
  const err = h('div', { class: 'note err', hidden: true, id: 'renumber-errors' });
  const body = h('div', { class: 'renumber' },
    h('p', { class: 'dim' }, 'Zmień numery w polach. Zapis idzie jednym wywołaniem – legacy zamienia numery w jednej transakcji, więc można je zamieniać miejscami. Zmiana trafia do channels.m3u i EPG bez przebudowy playoutów.'),
    h('div', { class: 'tw' }, h('table', null,
      h('thead', null, h('tr', null, h('th', null, 'Nr'), h('th', null, 'Kanał'), h('th', null, 'Był'))),
      h('tbody', null, rows.map((r, i) => {
        const was = h('td', { class: 'dim num' });
        const inp = h('input', { class: 'num-in', id: `renumber-${i}`, value: r.Number, 'aria-label': `Numer kanału ${r.Name}`, inputmode: 'decimal' });
        inp.addEventListener('input', () => { r.Number = inp.value; was.textContent = inp.value.trim() !== r.OriginalNumber ? r.OriginalNumber : ''; });
        return h('tr', null, h('td', null, inp), h('td', null, r.Name), was);
      })))),
    err);
  for (;;) {
    const ok = await dialog('Numeracja kanałów', body, [['Anuluj', false], ['Zapisz numerację', true, 'acc']]);
    if (!ok) return;
    const { errors, changed } = checkNumbers(rows);
    if (errors.length) { err.hidden = false; err.textContent = 'Popraw przed zapisem: ' + errors.join('; '); continue; }
    if (!changed.length) { toast('Numeracja bez zmian'); return; }
    try {
      await cmd('UpdateChannelNumbers', { Channels: changed.map((r) => ({ Id: r.Id, Number: r.Number, Name: r.Name, OriginalNumber: r.OriginalNumber })) });
      toast(`Zmieniono numery ${changed.length} kanałów`, 'ok');
      route(true);
      return;
    } catch (e) { err.hidden = false; err.textContent = 'Legacy odrzucił numerację: ' + e.message; }
  }
}

VIEWS.kanaly = crudView({
  title: 'Kanały',
  desc: 'Lista kanałów i edycja jednego. Odpowiednik Channels i Channel Editor w ErsatzTV legacy; zapis idzie wprost do legacy.',
  listCmd: 'GetAllChannels',
  needs: ['GetAllFFmpegProfiles', 'GetAllWatermarks', 'GetAllFillerPresets'],
  commands: { create: 'CreateChannel', update: 'UpdateChannel', delete: 'DeleteChannel' },
  noun: { list: 'Kanały', new: 'Nowy kanał', add: '+ Nowy kanał', create: 'Załóż kanał', one: chanName, title: chanName },
  load: async () => {
    const [rows, profiles, watermarks, fillers] = await Promise.all([
      cmd('GetAllChannels', { ShowDisabled: true }), cmd('GetAllFFmpegProfiles'), cmd('GetAllWatermarks'), cmd('GetAllFillerPresets')]);
    return { rows, profiles, watermarks, fillers };
  },
  id: (c) => c.Id,
  cols: ({ profiles }) => [
    ['Number', 'Nr', { class: 'num' }], ['Name', 'Nazwa'], ['Group', 'Grupa'],
    [(c) => profiles.find((p) => p.Id === c.FFmpegProfileId)?.Name, 'Profil'], [(c) => pl(c.StreamingMode), 'Strumień'],
    [(c) => (c.IsEnabled ? pill('włączony', 'ok') : pill('wyłączony')), 'Stan'],
  ],
  editor: (ch, { rows: channels, profiles, watermarks, fillers }) => {
    const base = { ...CHANNEL_DEFAULTS, ...(ch || {}), FFmpegProfileId: ch?.FFmpegProfileId ?? profiles[0]?.Id ?? 0 };
    if (!ch) base.Number = String(Math.max(0, ...channels.map((c) => parseFloat(c.Number) || 0)) + 1);
    return form(ch ? 'UpdateChannel' : 'CreateChannel', [
      { sec: 'Ogólne' },
      { key: 'Number', label: 'Numer', required: true },
      { key: 'Name', label: 'Nazwa', required: true },
      { key: 'Group', label: 'Grupa' },
      { key: 'Categories', label: 'Kategorie', placeholder: 'np. wiadomości, publicystyka' },
      { key: 'Logo', label: 'Logo', render: logoField },
      { label: 'Widoczność', group: [{ key: 'IsEnabled', kind: 'check', text: 'włączony' }, { key: 'ShowInEpg', kind: 'check', text: 'w przewodniku EPG' }] },
      { sec: 'Nadawanie' },
      { key: 'FFmpegProfileId', label: 'Profil kodowania', options: profiles.map((p) => [p.Id, p.Name]) },
      { key: 'StreamingMode', label: 'Tryb strumienia' },
      { key: 'PlayoutSource', label: 'Źródło playoutu' },
      { key: 'MirrorSourceChannelId', label: 'Kanał lustrzany', options: channels.filter((c) => c.Id !== ch?.Id).map((c) => [c.Id, `${c.Number} · ${c.Name}`]), show: (v) => v.PlayoutSource === 'Mirror' },
      { key: 'PlayoutMode', label: 'Tryb playoutu' },
      { key: 'IdleBehavior', label: 'Gdy nikt nie ogląda' },
      { sec: 'Treść' },
      { key: 'WatermarkId', label: 'Znak wodny', options: watermarks.map((w) => [w.Id, w.Name]), none: 'brak' },
      { key: 'FallbackFillerId', label: 'Wypełniacz awaryjny', options: fillers.map((x) => [x.Id, x.Name]), none: 'brak' },
      { key: 'PreferredAudioLanguageCode', label: 'Język audio', placeholder: 'pol' },
      { key: 'SubtitleMode', label: 'Napisy' },
      { key: 'PreferredSubtitleLanguageCode', label: 'Język napisów', placeholder: 'pol' },
      { key: 'NextEngineTextSubtitleMode', label: 'Napisy tekstowe (Next)' },
    ], base);
  },
  update: (data, ch) => ({ ...data, ChannelId: ch.Id }),
  createdId: (r) => r?.ChannelId,
  del: (ch) => ({ ChannelId: ch.Id }),
  confirmDelete: (ch) => `Usunąć kanał ${ch.Number} · ${ch.Name} razem z jego playoutem? Tego nie da się cofnąć.`,
  headExtra: () => [
    has('GetAllChannelsForSort') && has('UpdateChannelNumbers') ? btn('Numeracja…', renumber) : null,
    S.status.streamUrl ? h('a', { href: S.status.streamUrl + '/iptv/channels.m3u', target: '_blank', rel: 'noopener' }, 'channels.m3u ↗') : null],
});
