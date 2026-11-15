// Widoki grupy „Antena”: Na antenie, Przewodnik EPG.

import { openMonitor, Preview } from '../player.js';
import { scheduleCheck, checkItems, missingTitles, itemKey } from '../check.js';
import { S, VIEWS, h, btn, pill, head, panel, cmdNote, toast, confirmDlg, cmd, api, route, pl, hm, hms, dur, isFiller, trioLink, vizrsPreview, vizrsUrl, onChannel, dialog } from '../core.js';

/** Przebudowa playoutu przez kolejkę legacy (też w widoku Playouty). */
export async function build(p, mode) {
  if (mode === 'Reset' && !(await confirmDlg('Playout od nowa', `Zbudować playout kanału ${p.ChannelNumber} · ${p.ChannelName} od nowa? Ramówka zostanie ułożona ponownie od teraz.`, 'Buduj od nowa'))) return;
  try {
    await cmd('BuildPlayout', { PlayoutId: p.PlayoutId, Mode: mode });
    toast(`Playout ${p.ChannelName}: zlecono (${pl(mode)}). Legacy buduje go w tle.`, 'ok');
    setTimeout(() => route(true), 2500);
  } catch (e) { toast(e.message, 'bad'); }
}

const SESSION_PL = { start: 'uruchomiona', stop: 'zatrzymana', restart: 'zrestartowana' };

/** Emisja kanału w legacy: start, stop, restart sesji nadawania (POST /api/session/{kanał}/…). */
async function session(c, action, after) {
  if (action !== 'start' && !(await confirmDlg(action === 'stop' ? 'Zatrzymaj emisję' : 'Restart emisji',
    action === 'stop'
      ? `Zatrzymać emisję kanału ${c.Number} · ${c.Name}? Widzowie stracą obraz. Legacy uruchomi kanał znowu, gdy widz poprosi o strumień.`
      : `Zrestartować emisję kanału ${c.Number} · ${c.Name}? Obraz u widzów przerwie się na kilka sekund.`,
    action === 'stop' ? 'Zatrzymaj' : 'Restartuj'))) return;
  try {
    await api(`/api/session/${encodeURIComponent(c.Number)}/${action}`, { method: 'POST', body: '{}' });
    toast(`Kanał ${c.Number}: emisja ${SESSION_PL[action]}.`, 'ok');
  } catch (e) { toast(`Emisja kanału ${c.Number}: ${e.message}`, 'bad'); }
  after();
}

/** Cała grafika VizRS na wszystkich kanałach: zdjęcie albo przywrócenie (CG Playout /api/live/graphics/off|on). */
async function graphicsSwitch(off, after) {
  if (off && !(await confirmDlg('Zdejmij grafikę', 'Zdjąć całą grafikę VizRS ze wszystkich kanałów? VizRS zagra animacje wyjścia (OUT), potem wizja idzie bez grafiki aż do „Przywróć grafikę”, które wprowadzi strony z animacją wejścia (TAKE).', 'Zdejmij grafikę'))) return;
  try {
    await api(`/api/cg/live/graphics/${off ? 'off' : 'on'}`, { method: 'POST', body: '{}' });
    toast(off ? 'Grafika schodzi z anteny (animacje wyjścia) na wszystkich kanałach.' : 'Grafika wraca na antenę (animacje wejścia).', off ? 'bad' : 'ok');
  } catch (e) { toast('Grafika: ' + e.message, 'bad'); }
  after();
}

// ---------- K10: mozaika kanałów (multiview) ----------
// Każdy podgląd to sesja Next, która koduje kanał – liczba kafli ograniczona opcją serwera (--multiview-max, domyślnie 4).
let mosaicOn = false;
let mosaicSound = null;  // numer kanału z dźwiękiem (jeden naraz)
function mosaicStorage(val) {
  try { if (val === undefined) return localStorage.getItem('vizrs-live-mosaic') === '1'; localStorage.setItem('vizrs-live-mosaic', val ? '1' : '0'); } catch { /* bez localStorage */ }
  return false;
}
mosaicOn = mosaicStorage();

/** Kafle mozaiki dla kanałów; zwraca { el, mark(numer → [błędy]) }. Podglądy zatrzymuje zmiana ekranu (S.cleanup). */
function mosaic(channels) {
  const max = S.status?.multiviewMax ?? 4;
  const shown = channels.slice(0, max);
  const tiles = shown.map((c) => {
    const video = h('video', { muted: true, playsinline: true, 'aria-label': `Podgląd kanału ${c.Number}` });
    video.muted = mosaicSound !== String(c.Number);
    const st = h('span', { class: 'pill' }, 'ŁĄCZĘ');
    const snd = h('span', { class: 'pill' + (video.muted ? '' : ' on') }, video.muted ? 'wycisz.' : 'dźwięk');
    const err = h('div', { class: 'mv-err', hidden: true });
    const el = h('div', { class: 'mv-tile', 'data-mv': String(c.Number), tabindex: '0', role: 'button', title: 'Klik – dźwięk z tego kanału (pozostałe wyciszone)' },
      video, h('div', { class: 'mv-cap' }, h('b', null, `${c.Number} · ${c.Name}`), h('span', { class: 'grow' }), snd, st), err);
    const LABEL = { on: 'NA ŻYWO', info: 'NA ŻYWO', warn: 'POZA ZEGAREM', bad: 'BŁĄD', none: 'BRAK', '': 'ŁĄCZĘ' };
    const preview = new Preview(video, (text, kind = '') => { st.textContent = LABEL[kind] ?? kind; st.className = 'pill' + (kind ? ' ' + kind : ''); st.title = text; });
    const toggle = () => {
      mosaicSound = mosaicSound === String(c.Number) ? null : String(c.Number);
      for (const t of tiles) {
        t.video.muted = mosaicSound !== t.num;
        t.snd.textContent = t.video.muted ? 'wycisz.' : 'dźwięk';
        t.snd.className = 'pill' + (t.video.muted ? '' : ' on');
      }
    };
    el.addEventListener('click', toggle);
    el.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); toggle(); } });
    return { el, video, snd, err, preview, num: String(c.Number) };
  });
  tiles.forEach((t) => t.preview.start(t.num));
  S.cleanup.push(() => tiles.forEach((t) => t.preview.stop()));
  const more = channels.length > shown.length
    ? h('div', { class: 'hint pad' }, `Pokazuję ${shown.length} z ${channels.length} kanałów – limit podglądów VIZRS_LIVE_MULTIVIEW_MAX=${max} (każdy podgląd to sesja Next, która koduje; wybierz kanał w pasku górnym, by zobaczyć inne).`)
    : null;
  return {
    el: panel('Mozaika', h('span', { class: 'dim' }, 'wyciszone – klik w kafel włącza jego dźwięk; czerwona ramka = błąd ramówki'),
      h('div', { class: 'mv', 'data-mosaic': '' }, tiles.map((t) => t.el)), more,
      h('div', { class: 'cmd' }, 'Podgląd: /api/stream/{kanał}.m3u8 (jak „Podgląd ▶”) · każdy kafel = sesja segmentera Next kanału')),
    mark: (errs) => tiles.forEach((t) => {
      const e = errs(t.num);
      t.el.classList.toggle('bad', !!e.length);
      t.err.hidden = !e.length;
      t.err.textContent = e.length ? '⚠ ' + e[0] + (e.length > 1 ? ` (+${e.length - 1})` : '') : '';
    }),
  };
}

VIEWS.antena = async () => {
  // Ramka VizRS w karcie kanału tylko przy antenach kanałów (--vizrs-channels); inaczej jedna wspólna po prawej.
  const multi = !!(S.status?.vizrsUrl && S.status.vizrsChannels);
  const load = async () => {
    const [allChannels, playouts, cgState, sessions] = await Promise.all([
      cmd('GetAllChannels', { ShowDisabled: false }),
      cmd('GetPagedPlayouts', { Query: '', PageNum: 0, PageSize: 500 }),
      S.status?.cg ? api('/api/cg/live/state').then((r) => r.result).catch((e) => e) : null,
      api('/api/sessions').then((r) => r.result || []).catch((e) => e)]);
    const channels = allChannels.filter((c) => onChannel(c.Number));
    const byNumber = new Map((playouts?.Page || []).map((p) => [p.ChannelNumber, p]));
    const items = await Promise.all(channels.map(async (c) => {
      const p = byNumber.get(c.Number);
      if (!p) return [];
      try { return (await cmd('GetFuturePlayoutItemsById', { PlayoutId: p.PlayoutId, ShowFiller: true, PageNum: 0, PageSize: 8 }))?.Page || []; }
      catch { return []; }
    }));
    return { channels, byNumber, items, cgState, sessions };
  };
  let d = await load();
  const key = (x) => x.channels.map((c) => c.Number).join(',');
  // Bez anten kanałów główna antena VizRS idzie na każdy kanał: przy jednej karcie ramka w karcie (jak w makiecie),
  // przy wielu – jedna wspólna po prawej, żeby nie renderować tej samej grafiki kilka razy.
  const inCard = !!S.status?.vizrsUrl && (multi || d.channels.length === 1);
  const sessionOf = (c) => (d.sessions instanceof Error ? undefined : d.sessions.find((x) => String(x.ChannelNumber) === String(c.Number)) || null);
  const cgOf = (c) => (d.cgState instanceof Error ? null : d.cgState?.channels?.find((x) => String(x.number) === String(c.Number))) || null;

  // karty: szkielet (z ramką VizRS) raz, treść podmieniana przy odświeżeniu
  let progressEls = [];
  const slots = d.channels.map((c) => {
    const state = h('span');
    const body = h('div', { class: 'chan-dyn' });
    const badge = h('span', { class: 'live', hidden: '' }, 'NA ŻYWO');
    const frame = inCard ? h('div', { class: 'vframe' },
      h('iframe', { class: 'preview', title: multi ? `Grafika VizRS kanału ${c.Number}` : 'Grafika VizRS (antena główna)', 'data-vizrs-output': '', src: vizrsUrl('/output', c.Number) }), badge) : null;
    return { c, state, body, badge, el: panel(`${c.Number} · ${c.Name}`, state, h('div', { class: 'chan' }, frame, body)) };
  });
  const summary = h('span');
  const gfx = h('span', { class: 'bar', 'data-graphics': '' });
  const eventsBox = h('div');
  const notesBox = h('div');
  const mvMax = S.status?.multiviewMax ?? 4;
  const mv = mosaicOn && mvMax > 0 && d.channels.length ? mosaic(d.channels) : null;
  // K9: kontrola ramówki – przy wejściu, co 5 min i na żądanie (24 h pozycji każdego kanału – za drogo co 30 s)
  let chk = null;
  const runCheck = async (manual) => {
    try { chk = await scheduleCheck(d.channels, d.byNumber, d.cgState); } catch (e) { if (manual) toast('Kontrola ramówki: ' + e.message, 'bad'); return; }
    fill();
    if (manual) { const n = [...chk.values()].reduce((a, r) => a + r.issues.filter((i) => i.level === 'bad').length, 0); toast(n ? `Kontrola ramówki: ${n} błędów – lista „Uwagi”.` : 'Kontrola ramówki: bez błędów na 24 h.', n ? 'bad' : 'ok'); }
  };

  const fill = () => {
    const now = Date.now();
    progressEls = [];
    let airing = 0;
    slots.forEach(({ c, state, body, badge }, i) => {
      const p = d.byNumber.get(c.Number);
      const list = d.items[i];
      const cg = cgOf(c);
      const cur = list.find((it) => new Date(it.Start) <= now && now < new Date(it.Finish));
      const next = list.filter((it) => new Date(it.Start) > now).slice(0, 3);
      if (cur) airing++;
      badge.hidden = !cg?.on_air;
      const bar = h('i');
      const left = h('span', { class: 'num' });
      if (cur) progressEls.push({ bar, left, start: +new Date(cur.Start), finish: +new Date(cur.Finish) });
      const ses = sessionOf(c);
      const bad = (chk?.get(String(c.Number))?.issues || []).filter((x) => x.level === 'bad');
      const badPill = bad.length ? pill(`BŁĄD RAMÓWKI${bad.length > 1 ? ' ×' + bad.length : ''}`, 'bad') : null;
      if (badPill) badPill.title = bad.map((x) => x.text).join('\n');
      slots[i].el.classList.toggle('chk-bad', !!bad.length);
      mv?.mark((num) => (chk?.get(num)?.issues || []).filter((x) => x.level === 'bad').map((x) => x.text));
      state.replaceChildren(...[!p ? pill('BRAK PLAYOUTU', 'warn') : cur ? pill('NA ANTENIE', 'on') : pill('ZASTĘPCZY', 'warn'), badPill,
        ses === undefined ? null : ses ? pill(`emisja: ${ses.State}`, 'on') : pill('emisja zatrzymana')].filter(Boolean));
      const stream = S.status.streamUrl ? h('a', { href: `${S.status.streamUrl}/iptv/channel/${encodeURIComponent(c.Number)}.m3u8`, target: '_blank', rel: 'noopener' }, 'Strumień ↗') : null;
      const nextLine = next.length ? h('div', { class: 'dim' }, 'Następnie: ', next.map((n, j) => [j ? ' · ' : '', h('span', { class: 'num' }, hm(new Date(n.Start))), ' ', n.Title])) : null;
      body.replaceChildren(
        cur
          ? h('div', { class: 'np' },
            h('div', { class: 'now' }, h('b', null, cur.Title), isFiller(cur) ? pill('wypełniacz') : null,
              h('span', { class: 'dim num' }, `${hm(new Date(cur.Start))}–${hm(new Date(cur.Finish))}`), h('span', { class: 'grow' }), left),
            h('div', { class: 'prog' }, bar), nextLine)
          : h('div', { class: 'np' },
            h('div', { class: 'now' }, h('b', null, p ? 'Wypełniacz zapasowy' : 'Brak ramówki'),
              h('span', { class: 'dim' }, p ? 'teraz nic nie jest zaplanowane' : 'kanał nie ma playoutu – załóż go w widoku Playouty')),
            nextLine),
        cgPills(cg, now) || '',
        layersRow(c, cg, d.cgState, refresh) || '',
        h('div', { class: 'bar' },
          btn('Podgląd ▶', () => openMonitor(c.Number)),
          ses ? null : btn('Uruchom emisję', () => session(c, 'start', refresh)),
          ses === null ? null : btn('Restart emisji', () => session(c, 'restart', refresh)),
          ses === null ? null : btn('Zatrzymaj emisję', () => session(c, 'stop', refresh), 'danger'),
          p ? btn('Przebuduj playout', () => build(p, 'Refresh')) : null,
          p ? btn('Od nowa', () => build(p, 'Reset'), 'danger') : null,
          stream,
          trioLink(c.Number)));
    });
    // starsze CG Playout (bez /api/live/graphics/off|on) nie podaje graphics_off – przełącznika wtedy nie ma
    const cgOk = S.status?.cg && d.cgState && !(d.cgState instanceof Error) && typeof d.cgState.settings?.graphics_off === 'boolean';
    const off = !!d.cgState?.settings?.graphics_off;
    gfx.replaceChildren(...(cgOk
      ? [off ? pill('GRAFIKA ZDJĘTA', 'bad') : pill('grafika na antenie', 'on'),
        off ? btn('Przywróć grafikę', () => graphicsSwitch(false, refresh), 'acc') : btn('Zdejmij grafikę', () => graphicsSwitch(true, refresh), 'danger')]
      : []));
    const total = slots.length;
    summary.replaceChildren(total ? pill(`${airing}/${total} ${total === 1 ? 'kanał' : 'kanałów'} w ramówce`, airing === total ? 'on' : 'warn') : null);
    const ev = upcoming(d);
    eventsBox.replaceChildren(ev.length
      ? h('div', { class: 'tl' }, ev.map((e) => h('div', { class: 'ev' }, h('span', { class: 'num' }, hms(new Date(e.t))), pill(e.ch),
        h('span', null, e.live ? [pill('LIVE', 'on'), ' '] : null, e.title,
          e.live ? h('span', { class: 'dim k' }, ' · audycja na żywo – studio przez CG Playout') : e.filler ? h('span', { class: 'dim k' }, ' · wypełniacz') : null))))
      : h('div', { class: 'empty' }, 'Brak zaplanowanych pozycji.'));
    const ns = notes(d, chk);
    notesBox.replaceChildren(ns.length
      ? h('div', { 'data-notes': '' }, ns.map(([kind, who, text, link]) => h('div', { class: 'alert' }, pill(who, kind), h('span', { class: 'w' }, text, link ? [' ', link] : null))))
      : h('div', { class: 'empty', 'data-notes': '' }, 'Bez uwag – wszystkie kanały mają ramówkę.'));
    tick();
  };
  const tick = () => {
    const now = Date.now();
    for (const e of progressEls) {
      e.bar.style.width = Math.min(100, Math.max(0, ((now - e.start) / (e.finish - e.start)) * 100)) + '%';
      e.left.textContent = '−' + dur(e.finish - now);
    }
  };
  const refresh = async () => {
    if (!slots.length && !d.channels.length) return route(true);
    let nd;
    try { nd = await load(); } catch (e) { return toast('Odświeżenie: ' + e.message, 'bad'); }
    if (key(nd) !== key(d)) return route(true);  // inne kanały – nowe karty
    d = nd;
    fill();
  };
  fill();
  runCheck(false);
  S.timers.push(setInterval(tick, 1000), setInterval(refresh, 30000), setInterval(() => runCheck(false), 300000));

  return [
    h('div', { class: 'head' }, h('h1', null, 'Na antenie'), summary, h('span', { class: 'grow' }), gfx,
      multi ? null : trioLink('', 'Otwórz Trio ↗'),
      mvMax > 0 ? (() => { const b = btn(mosaicOn ? 'Mozaika: wł.' : 'Mozaika', () => { mosaicOn = !mosaicOn; mosaicStorage(mosaicOn); route(true); }); b.classList.toggle('on', mosaicOn); b.setAttribute('aria-pressed', String(mosaicOn)); b.id = 'mosaic-toggle'; return b; })() : null,
      btn('Sprawdź ramówkę', () => runCheck(true)),
      btn('Odśwież', () => refresh()),
      h('p', null, 'Co teraz leci w ramówce każdego kanału, co następne, jaka grafika VizRS jest na wizji i czy studio jest na antenie. Tu też uruchomisz, zrestartujesz albo zatrzymasz emisję kanału i zdejmiesz całą grafikę. Dane z playoutów legacy i VizRS CG Playout, odświeżane co 30 s.')),
    mv?.el,
    h('div', { class: 'split wide' },
      h('div', { class: 'cols' }, slots.length ? slots.map((x) => x.el) : h('div', { class: 'note' }, 'Brak włączonych kanałów. Dodaj kanał w widoku Kanały.')),
      h('div', { class: 'cols' },
        inCard ? null : vizrsPreview('Grafika VizRS (/output)', d.channels.map((c) => [String(c.Number), `${c.Number} · ${c.Name}`])),
        panel('Najbliższe zdarzenia', null, eventsBox),
        panel('Uwagi', null, notesBox))),
    panel('Most', null, cmdNote('GetAllChannels', 'GetPagedPlayouts', 'GetFuturePlayoutItemsById', 'BuildPlayout', 'most: sesje (GET /api/sessions, start · stop · restart)', 'CG Playout GET /api/live/state · POST /api/live/graphics/off|on')),
  ];
};

// ---------- K12 / K13: warstwy VizRS na antenie, ręczne TAKE / CONTINUE / OUT (kontrakt CG – DOKUMENTACJA §3) ----------
// CG Playout podaje w /api/live/state: kanał.layers = [{ layer, page, name?, scene?, since? }] i features: ['layers', 'take', …].
// Starszy CG Playout bez tych pól – pigułek i przycisków nie ma.
const cgFeature = (st, f) => !!(st && !(st instanceof Error) && Array.isArray(st.features) && st.features.includes(f));

async function cgAction(c, action, body, after) {
  try {
    await api(`/api/cg/live/channels/${encodeURIComponent(c.Number)}/${action}`, { method: 'POST', body: JSON.stringify(body) });
    toast(`Kanał ${c.Number}: ${{ take: 'strona wprowadzona', continue: 'dalej', out: 'warstwa zdjęta' }[action]}.`, 'ok');
  } catch (e) { toast(`Grafika kanału ${c.Number}: ${e.message}`, 'bad'); }
  setTimeout(after, 800);
}

/** „Wprowadź stronę…” – odpowiednik Quick Take: lista stron VizRS kanału (/etv/pages) i TAKE wybranej. */
async function quickTake(c, after) {
  let pages = [];
  try { pages = (await api(`/api/cg/pages/${encodeURIComponent(c.Number)}`)).result?.pages || []; } catch (e) { return toast('Strony VizRS: ' + e.message, 'bad'); }
  if (!pages.length) return toast('Show VizRS nie ma stron dla tego kanału.', 'bad');
  const sel = h('select', { id: 'qt-page', 'aria-label': 'Strona VizRS' }, pages.map((p) => h('option', { value: p.number }, `${p.number} · ${p.name || ''}`)));
  const ok = await dialog(`Wprowadź stronę · kanał ${c.Number}`, h('div', { class: 'form' },
    h('label', { for: 'qt-page' }, 'Strona'), sel,
    h('p', { class: 'dim' }, 'Ręcznie wprowadzona strona ma pierwszeństwo przed regułą grafiki, dopóki jej nie zdejmiesz („Zdejmij” przy warstwie).')),
  [['Anuluj', false], ['TAKE', true, 'acc']]);
  if (ok) cgAction(c, 'take', { page: sel.value }, after);
}

/** Pigułki warstw („W10 · belka 101”) i przyciski Dalej / Zdejmij przy każdej, gdy CG je obsługuje. */
function layersRow(c, cg, st, after) {
  if (!cgFeature(st, 'layers') || !cg) return null;
  const layers = Array.isArray(cg.layers) ? cg.layers : [];
  const take = cgFeature(st, 'take');
  const items = layers.map((l) => {
    // CG Playout podaje warstwę tak, jak nazywa ją VizRS: liczba („10”) albo nazwa („main”).
    const where = /^\d+$/.test(String(l.layer)) ? `W${l.layer}` : String(l.layer);
    const p = pill(`${where} · ${l.name || l.page}`, 'info');
    p.title = [l.scene ? `scena ${l.scene}` : '', l.since ? `od ${hm(new Date(l.since))}` : '', l.manual ? 'wprowadzona ręcznie' : 'z reguły'].filter(Boolean).join(' · ');
    return h('span', { class: 'layer', 'data-layer': String(l.layer) }, p,
      take ? btn('Dalej', () => cgAction(c, 'continue', { layer: l.layer }, after), 'sm') : null,
      take ? btn('Zdejmij', () => cgAction(c, 'out', { layer: l.layer }, after), 'sm danger') : null);
  });
  return h('div', { class: 'layers', 'data-layers': '' },
    items.length ? items : h('span', { class: 'dim' }, 'grafika VizRS: brak warstw na antenie'),
    take ? btn('Wprowadź stronę…', () => quickTake(c, after)) : null);
}

/** Wiersz stanu studia z CG Playout pod „teraz” (w makiecie – warstwy grafiki; CG nie podaje warstw, więc tylko to, co wie). */
function cgPills(cg, now) {
  if (!cg) return null;
  const out = [];
  if (cg.on_air) out.push(pill('studio NA ANTENIE', 'on'));
  else if (cg.source?.live) out.push(pill('sygnał studia', 'ok'));
  if (cg.hold) out.push(pill('studio wstrzymane', 'warn'));
  if (cg.anytime) out.push(pill('studio także poza audycjami'));
  const nl = cg.next_live;
  if (nl && nl.start > now) out.push(pill(`LIVE ${hm(new Date(nl.start))} · ${nl.title}`, 'info'));
  return out.length ? h('div', { class: 'layers', 'data-cg-state': '' }, out) : null;
}

/** Najbliższe zdarzenia: pozycje ramówki legacy wszystkich kanałów + najbliższe audycje LIVE z CG Playout. */
function upcoming(d) {
  const now = Date.now();
  const out = d.channels.flatMap((c, i) => d.items[i].filter((it) => new Date(it.Start) > now)
    .map((it) => ({ t: +new Date(it.Start), ch: c.Name, title: it.Title, filler: isFiller(it) })));
  const cg = d.cgState instanceof Error ? null : d.cgState;
  for (const ch of (cg?.channels || []).filter((x) => onChannel(x.number))) {
    const nl = ch.next_live;
    if (nl && nl.start > now) out.push({ t: nl.start, ch: ch.name || ch.number, title: nl.title, live: true });
  }
  // audycje LIVE zawsze na liście (najwyżej jedna na kanał), resztę miejsc biorą najbliższe pozycje ramówki
  const live = out.filter((e) => e.live);
  return [...out.filter((e) => !e.live).sort((a, b) => a.t - b.t).slice(0, Math.max(0, 12 - live.length)), ...live].sort((a, b) => a.t - b.t);
}

/** Uwagi dla operatora: [rodzaj pigułki, kto, tekst, odnośnik?] – tylko z danych, które panel ma. */
function notes(d, chk) {
  const now = Date.now();
  const link = (href, text) => h('a', { href }, text);
  const out = [];
  d.channels.forEach((c, i) => {
    const p = d.byNumber.get(c.Number);
    const b = p?.BuildStatus;
    if (!p) out.push(['warn', c.Name, 'kanał nie ma playoutu – nic nie nadaje z ramówki.', link('#playouty', 'Załóż playout')]);
    else if (b && !b.Success) out.push(['bad', c.Name, `legacy nie zbudował playoutu${b.Message ? ': ' + b.Message : '.'}`, link('#playouty', 'Playouty')]);
    else if (!d.items[i].some((it) => new Date(it.Start) <= now && now < new Date(it.Finish))) {
      out.push(['warn', c.Name, 'przerwa w ramówce – leci wypełniacz zapasowy kanału.', link('#playouty', 'Przebuduj w Playoutach')]);
    }
  });
  // K9: wynik kontroli ramówki (bez „brak playoutu” – jest wyżej)
  for (const c of d.channels) {
    for (const x of chk?.get(String(c.Number))?.issues || []) {
      if (/nie ma playoutu/.test(x.text)) continue;
      out.push([x.level, c.Name, x.text + '.', link('#playouty', 'Playouty')]);
    }
  }
  if (d.sessions instanceof Error) out.push(['warn', 'Emisja', 'stan sesji niedostępny: ' + d.sessions.message, null]);
  if (d.cgState instanceof Error) out.push(['bad', 'CG Playout', 'nie odpowiada: ' + d.cgState.message, null]);
  else {
    if (d.cgState?.settings?.graphics_off) out.push(['bad', 'Grafika', 'cała grafika VizRS zdjęta z anteny (ZDEJMIJ GRAFIKĘ) – przywróć przyciskiem u góry.', null]);
    for (const ch of (d.cgState?.channels || []).filter((x) => onChannel(x.number))) {
      const who = ch.name || `kanał ${ch.number}`;
      const nl = ch.next_live;
      if (ch.hold) out.push(['warn', who, 'studio wstrzymane (ZDEJMIJ) – kanał idzie z ramówki.', link('#live', 'Sloty na żywo')]);
      if (nl && nl.start > now && nl.start - now < 3600e3 && !ch.source?.live) {
        out.push(['info', who, `slot na żywo „${nl.title}” o ${hm(new Date(nl.start))} – studio jeszcze nie nadaje.`, link('#live', 'Sloty na żywo')]);
      }
    }
  }
  return out;
}

// Okno przewodnika: przesunięcie od „teraz” w krokach po 4 h (legacy podaje tylko przyszłe pozycje – wstecz się nie da).
let epgStep = 0;
const EPG_SPAN = 4 * 3600e3;
const EPG_MIN = 0.015;     // pozycja węższa niż 1,5 % okna (3,6 min) idzie do bloku „pierwsza +N”
const EPG_GROUP = 30 * 60e3; // blok krótkich pozycji najwyżej 30 min

/** Pozycje w oknie [t0, t1) jako bloki do narysowania: długie osobno, krótkie sklejone (z listą w podpowiedzi). */
function epgBlocks(list, t0, t1, liveAt, errs = new Map()) {
  const out = [];
  let grp = null;
  const flush = () => {
    if (!grp) return;
    const one = grp.items.length === 1;
    out.push({ s: grp.s, e: grp.e, label: one ? grp.items[0].Title : `${grp.items[0].Title} +${grp.items.length - 1}`, grp: !one,
      title: `${hm(new Date(grp.s))}–${hm(new Date(grp.e))}\n` + grp.items.map((it) => `${hm(new Date(it.Start))} ${it.Title}`).join('\n'),
      fill: grp.items.every(isFiller), live: grp.items.some((it) => liveAt(+new Date(it.Start))),
      err: grp.items.flatMap((it) => errs.get(itemKey(it)) || []) });
    grp = null;
  };
  for (const it of list) {
    const s = +new Date(it.Start), e = +new Date(it.Finish);
    if (e <= t0 || s >= t1) continue;
    if (e - s < EPG_MIN * (t1 - t0)) {
      if (grp && (s - grp.e > 60e3 || e - grp.s > EPG_GROUP)) flush();
      if (!grp) grp = { s, e, items: [] };
      grp.items.push(it);
      grp.e = e;
      continue;
    }
    flush();
    out.push({ s, e, label: it.Title, title: `${hm(new Date(s))}–${hm(new Date(e))} ${it.Title}`, fill: isFiller(it), live: liveAt(s), err: errs.get(itemKey(it)) || [] });
  }
  flush();
  return out;
}

VIEWS.epg = async () => {
  const t0d = new Date(); t0d.setMinutes(t0d.getMinutes() < 30 ? 0 : 30, 0, 0);
  const t0 = +t0d + epgStep * EPG_SPAN, t1 = t0 + EPG_SPAN;
  const [allChannels, res, cgState] = await Promise.all([
    cmd('GetAllChannels', { ShowDisabled: false }),
    cmd('GetPagedPlayouts', { Query: '', PageNum: 0, PageSize: 500 }),
    S.status?.cg ? api('/api/cg/live/state').then((r) => r.result).catch(() => null) : null]);
  const channels = allChannels.filter((c) => onChannel(c.Number)).sort((a, b) => (parseFloat(a.Number) || 0) - (parseFloat(b.Number) || 0));
  const byNumber = new Map((res?.Page || []).map((p) => [p.ChannelNumber, p]));
  // dość pozycji, żeby sięgnąć końca okna przy krótkich pozycjach (2 min → 120 na 4 h)
  const pageSize = Math.min(1000, 160 * (epgStep + 1));
  const missingP = missingTitles().catch(() => new Set());
  const lists = await Promise.all(channels.map((c) => {
    const p = byNumber.get(c.Number);
    return p ? cmd('GetFuturePlayoutItemsById', { PlayoutId: p.PlayoutId, ShowFiller: true, PageNum: 0, PageSize: pageSize }).then((r) => r?.Page || []).catch(() => []) : null;
  }));
  const missing = await missingP;
  const x = (t) => ((t - t0) / EPG_SPAN) * 100;
  const ruler = h('div', { class: 'ruler' }, [0, 1, 2, 3].map((k) => { const s = h('span', null, hm(new Date(t0 + k * 3600e3))); s.style.left = k * 25 + '%'; return s; }));
  const nowLine = () => { const l = h('div', { class: 'nowl' }); l.style.left = x(Date.now()) + '%'; return l; };
  const grid = h('div', { class: 'epg' }, h('div', { class: 'ch dim' }, 'kanał'), ruler);
  channels.forEach((c, i) => {
    grid.append(h('div', { class: 'ch' }, h('span', { class: 'num dim' }, c.Number), c.Name));
    const row = h('div', { class: 'row', 'data-epg-channel': String(c.Number) });
    if (!lists[i]) {
      row.append(h('div', { class: 'none' }, 'brak playoutu – ', h('a', { href: '#playouty' }, 'załóż w Playoutach')));
      grid.append(row);
      return;
    }
    // audycje LIVE z ramówki CG Playout (guide) – ta sama godzina startu co pozycja legacy
    const guide = (cgState?.guide?.[String(c.Number)] || []).filter((g) => g.live);
    const liveAt = (s) => guide.some((g) => Math.abs(g.start - s) < 90e3);
    const now = Date.now();
    // K9: kontrola ramówki na pozycjach okna – dziury jako czerwone pola, błędne pozycje na czerwono z opisem w podpowiedzi
    const ck = checkItems(lists[i], { now, missing, window: Math.max(0, t1 - now) });
    for (const g of ck.gaps) {
      if (g.e <= t0 || g.s >= t1) continue;
      const el = h('div', { class: 'p gap', title: `${hm(new Date(g.s))}–${hm(new Date(g.e))} dziura w ramówce – leci wypełniacz zapasowy` }, 'dziura');
      el.style.left = Math.max(0, x(g.s)) + '%';
      el.style.width = `calc(${Math.min(100, x(g.e)) - Math.max(0, x(g.s))}% - 2px)`;
      row.append(el);
    }
    for (const b of epgBlocks(lists[i], t0, t1, liveAt, ck.items)) {
      const el = h('div', { class: 'p' + (b.s <= now && now < b.e ? ' now' : '') + (b.fill ? ' fill' : '') + (b.grp ? ' grp' : '') + (b.live ? ' lv' : '') + (b.err.length ? ' err' : ''),
        title: b.title + (b.err.length ? '\n⚠ ' + b.err.join('\n⚠ ') : '') },
        b.live ? '● ' : '', b.err.length ? '⚠ ' : '', b.label);
      el.style.left = Math.max(0, x(b.s)) + '%';
      el.style.width = `calc(${Math.min(100, x(b.e)) - Math.max(0, x(b.s))}% - 2px)`;
      row.append(el);
    }
    if (now >= t0 && now < t1) row.append(nowLine());
    grid.append(row);
  });
  const go = (d) => { epgStep = d === 0 ? 0 : Math.max(0, epgStep + d); route(true); };
  const back = btn('← 4 h', () => go(-1));
  back.disabled = !epgStep;  // legacy podaje tylko przyszłe pozycje
  const day = new Date(t0).toLocaleDateString('pl-PL', { weekday: 'long', day: 'numeric', month: 'long' });
  return [
    head('Przewodnik EPG', 'Ramówka wszystkich kanałów na osi czasu, tak jak zobaczy ją widz. Krótkie pozycje są sklejone w bloki – lista w podpowiedzi.',
      h('span', { class: 'bar' },
        back, btn('teraz', () => go(0)), btn('4 h →', () => go(1)),
        S.status.streamUrl ? h('a', { href: S.status.streamUrl + '/iptv/xmltv.xml', target: '_blank', rel: 'noopener' }, 'xmltv.xml ↗') : null)),
    panel(`${day} · ${hm(new Date(t0))}–${hm(new Date(t1))}`, epgStep ? pill(`+${epgStep * 4} h`, 'info') : pill('teraz', 'on'),
      channels.length ? h('div', { class: 'tw' }, grid) : h('div', { class: 'empty' }, 'Brak włączonych kanałów.'),
      cmdNote('GetAllChannels', 'GetPagedPlayouts', 'GetFuturePlayoutItemsById', 'CG Playout GET /api/live/state (audycje LIVE)')),
  ];
};
