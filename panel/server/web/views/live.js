// Widoki „Sloty na żywo” (stanowisko MCR) i „Audycje LIVE” – całe dawne VizRS Live Panel z VizRS CG Playout (`/live`, tam już usunięte)
// przeniesione do VizRS Live. Logika (odbiór sygnału, przejęcie kanału, pliki audycji) zostaje w CG Playout;
// panel rozmawia z nim przez pośrednika serwera /api/cg/live/… (token CG zostaje w serwerze).

import { VIEWS, S, h, head, panel, pill, btn, table, toast, cmd, cmdNote, confirmDlg, dialog, has, route, pad2, hm, operatorName, onChannel } from '../core.js';
import { cg } from './grafika.js';

const cgMissing = () => h('div', { class: 'note' },
  'VizRS CG Playout nie jest podłączony. Uruchom panel z --cg-url http://<cg>:7700 i --cg-token <panel_token CG Playout> ' +
  '(VIZRS_LIVE_CG_URL, VIZRS_LIVE_CG_TOKEN); w CG Playout ustaw panel_token (VIZRS_PANEL_TOKEN).');
const cgError = (e) => h('div', { class: 'note err', 'data-cg-error': '' }, 'VizRS CG Playout: ' + e.message);

// ---------- czas CG Playout ----------
// Liczniki i odliczania liczone wg zegara CG Playout (różnica do zegara przeglądarki z każdego stanu).
let skew = 0;
const cgNow = () => Date.now() - skew;
const clock = (s) => { s = Math.max(0, Math.floor(s)); return `${pad2(Math.floor(s / 3600))}:${pad2(Math.floor(s / 60) % 60)}:${pad2(s % 60)}`; };
const short = (s) => clock(s).replace(/^00:/, '');
const at = (ms) => hm(new Date(ms));
function day(ms) {
  const d = new Date(ms);
  return new Date(cgNow()).toDateString() === d.toDateString() ? 'dziś' : d.toLocaleDateString('pl-PL', { weekday: 'short', day: 'numeric', month: 'numeric' });
}

/** Pamięć przeglądarki bywa zablokowana – wtedy po prostu nie pamiętamy. */
const store = {
  get: (k) => { try { return localStorage.getItem(k) || ''; } catch { return ''; } },
  set: (k, v) => { try { localStorage.setItem(k, v); } catch { /* bez pamięci */ } },
};

/** Liczniki „od” (sygnał studia) i „do” (odliczanie do audycji) – tyka co 0,5 s bez przebudowy widoku. */
function tick() {
  document.querySelectorAll('[data-since]').forEach((el) => { el.textContent = clock((cgNow() - +el.dataset.since) / 1000); });
  document.querySelectorAll('[data-until]').forEach((el) => { el.textContent = short((+el.dataset.until - cgNow()) / 1000); });
}

// ---------- Sloty na żywo: dane nadawania ----------
// Dane nadawania z CG Playout (`inputs` kanału: wejścia z panelu albo VIZRS_INGEST): RTMP – serwer i klucz jak w OBS, SRT – adres z hasłem.
// Host nieznany (VIZRS_INGEST_PUBLIC puste) = host, z którego operator otwiera panel.
const copyBtn = (label, text) => btn('Kopiuj', async () => {
  try { await navigator.clipboard.writeText(text); toast(`Skopiowano: ${label}`); } catch { toast('Przeglądarka nie pozwoliła skopiować – zaznacz i skopiuj ręcznie.', 'bad'); }
});

// odsłonięte pola i rozwinięte „Jak nadawać” przetrwają odświeżanie stanu (do przeładowania strony)
const revealed = new Set();
// „Studio także poza audycjami” kliknięte, a jeszcze niepotwierdzone w stanie CG: kanał → {value, until}.
// Odczyt stanu co 1 s, wysłany przed zapisem, cofałby pole do starej wartości (operator klikał wtedy znowu).
const anytimeSent = new Map();
const anytimeShown = (c) => {
  const p = anytimeSent.get(c.number);
  if (p && (p.value === c.anytime || Date.now() > p.until)) anytimeSent.delete(c.number);
  return anytimeSent.has(c.number) ? anytimeSent.get(c.number).value : c.anytime;
};
const opened = new Set();

function secretField(label, value, aria) {
  const open = revealed.has(aria);
  const out = h('input', { type: open ? 'text' : 'password', readonly: true, value, class: 'mono grow', 'aria-label': aria });
  const show = btn(open ? 'Ukryj' : 'Pokaż', () => {
    const hidden = out.type === 'password';
    out.type = hidden ? 'text' : 'password';
    show.textContent = hidden ? 'Ukryj' : 'Pokaż';
    if (hidden) revealed.add(aria); else revealed.delete(aria);
  });
  return [h('dt', null, label), h('dd', { class: 'bar' }, out, show, copyBtn(label, value))];
}

function plainField(label, value, aria) {
  return [h('dt', null, label), h('dd', { class: 'bar' }, h('input', { readonly: true, value, class: 'mono grow', 'aria-label': aria }), copyBtn(label, value))];
}

function inputsBox(c) {
  const list = c.inputs || [];
  if (!list.length) {
    return h('div', { class: 'note', 'data-inputs': c.number }, 'Ten kanał nie ma wejścia studia. ',
      btn('+ Dodaj wejście', () => editInputs(c.number)));
  }
  return h('div', { class: 'inputs', 'data-inputs': c.number }, list.map((i, n) => {
    const host = i.host || location.hostname;
    const id = `${c.number}-${n + 1}`;
    if (i.kind === 'rtmp') {
      const server = `rtmp://${host}:${i.port}${i.app || ''}`;
      return h('div', { class: 'input' }, h('h4', null, 'RTMP (OBS, vMix, koder sprzętowy)'),
        h('dl', { class: 'kv' }, ...plainField('Serwer', server, `Serwer RTMP ${id}`), ...secretField('Klucz strumienia', i.key, `Klucz RTMP ${id}`)),
        h('p', { class: 'dim' }, 'OBS: Ustawienia → Strumień → Usługa „Własny…”, Serwer i Klucz strumienia jak wyżej. Wyjście: H.264 + AAC, interwał klatek kluczowych 1–2 s.'));
    }
    const q = [['mode', 'caller'], i.passphrase ? ['passphrase', i.passphrase] : null, i.streamid ? ['streamid', i.streamid] : null].filter(Boolean);
    const url = `srt://${host}:${i.port}?` + q.map(([k, v]) => `${k}=${encodeURIComponent(v)}`).join('&');
    return h('div', { class: 'input' }, h('h4', null, 'SRT (MPEG-TS)'),
      h('dl', { class: 'kv' }, ...secretField('Adres SRT', url, `Adres SRT ${id}`),
        ...(i.passphrase ? secretField('Hasło (passphrase)', i.passphrase, `Hasło SRT ${id}`) : [])),
      h('p', { class: 'dim' }, 'Nadawca w trybie caller; w OBS: Serwer = cały adres SRT, Klucz pusty.'));
  }));
}

// ---------- Sloty na żywo: ustawianie wejść ----------
// Lista wejść CG Playout (/api/live/inputs, plik inputs.json); odbiorniki CG przejmują ją w ciągu kilku sekund, bez restartu.
// Adres odbiornika: rtmp://0.0.0.0:<port>/live/<klucz> albo srt://0.0.0.0:<port>?passphrase=<hasło>; porty tylko z zakresu wystawionego przez compose CG.
const randomKey = () => Array.from(crypto.getRandomValues(new Uint8Array(12)), (b) => b.toString(16).padStart(2, '0')).join('');

function parseInput(l) {
  const m = /^(rtmp|srt):\/\/[^/:?]*:(\d+)([^?]*)\??(.*)$/i.exec(l.url) || [];
  const kind = (m[1] || 'rtmp').toLowerCase();
  const path = m[3] || '';
  const params = new URLSearchParams(m[4] || '');
  const secret = kind === 'rtmp' ? path.split('/').pop() : params.get('passphrase') || '';
  params.delete('passphrase');
  return { channel: l.channel, kind, port: +m[2] || 0, app: kind === 'rtmp' ? path.slice(0, path.length - secret.length - 1) || '/live' : '', secret, rest: params.toString() };
}

function inputUrl(r) {
  if (r.kind === 'rtmp') return `rtmp://0.0.0.0:${r.port}${r.app || '/live'}/${r.secret}`;
  const q = [r.secret ? `passphrase=${r.secret}` : '', r.rest].filter(Boolean).join('&');
  return `srt://0.0.0.0:${r.port}` + (q ? `?${q}` : '');
}

async function editInputs(forChannel) {
  let data;
  try { data = await cg('live/inputs'); } catch (e) {
    toast(/nie ma takiego adresu|brak tej funkcji/.test(e.message) ? 'Ten VizRS CG Playout nie pozwala ustawiać wejść z panelu – zaktualizuj go (VIZRS_INGEST w jego .env działa nadal).' : e.message, 'bad');
    return;
  }
  const ports = data.ports || { rtmp: [1936, 1943], srt: [9000, 9007] };
  const range = (k) => { const [a, b] = ports[k]; return Array.from({ length: b - a + 1 }, (_, i) => a + i); };
  const rows = (data.listeners || []).map(parseInput);
  const freePort = (kind) => range(kind).find((p) => !rows.some((r) => r.kind === kind && r.port === p)) || ports[kind][0];
  const channels = [...new Set([...(view?.state.channels || []).map((c) => c.number), ...rows.map((r) => r.channel)])];
  const nameOf = (n) => (view?.state.channels || []).find((c) => c.number === n)?.name || '';
  if (forChannel != null) rows.push({ channel: forChannel, kind: 'rtmp', port: freePort('rtmp'), app: '/live', secret: randomKey(), rest: '' });
  const list = h('div', { class: 'inputs-edit', 'data-inputs-edit': '' });
  const opt = (v, t, sel) => h('option', { value: String(v), selected: sel }, t);
  function draw() {
    list.replaceChildren(...(rows.length ? rows.map((r, n) => {
      const ch = h('select', { 'aria-label': `Kanał wejścia ${n + 1}` }, channels.map((c) => opt(c, `${c} ${nameOf(c)}`.trim(), c === r.channel)));
      ch.addEventListener('change', () => { r.channel = ch.value; });
      const kind = h('select', { 'aria-label': `Rodzaj wejścia ${n + 1}` }, opt('rtmp', 'RTMP', r.kind === 'rtmp'), opt('srt', 'SRT', r.kind === 'srt'));
      kind.addEventListener('change', () => { r.kind = kind.value; r.port = freePort(r.kind); r.app = '/live'; draw(); });
      const port = h('select', { 'aria-label': `Port wejścia ${n + 1}` }, range(r.kind).map((p) => opt(p, String(p), p === r.port)));
      if (!range(r.kind).includes(r.port)) port.prepend(opt(r.port, `${r.port} (spoza zakresu)`, true));
      port.addEventListener('change', () => { r.port = +port.value; });
      const secret = h('input', { value: r.secret, class: 'mono grow', placeholder: r.kind === 'rtmp' ? 'klucz strumienia' : 'hasło 10–79 znaków (puste = bez hasła)', 'aria-label': `${r.kind === 'rtmp' ? 'Klucz' : 'Hasło'} wejścia ${n + 1}` });
      secret.addEventListener('input', () => { r.secret = secret.value.trim(); });
      return h('div', { class: 'bar' }, ch, kind, port, secret,
        btn('Losuj', () => { r.secret = randomKey(); draw(); }),
        btn('×', () => { rows.splice(n, 1); draw(); }, 'danger'));
    }) : [h('div', { class: 'empty' }, 'Brak wejść – żaden kanał nie przyjmie sygnału ze studia.')]),
    btn('+ Wejście', () => { rows.push({ channel: channels[0] || '1', kind: 'rtmp', port: freePort('rtmp'), app: '/live', secret: randomKey(), rest: '' }); draw(); }));
  }
  draw();
  const body = h('div', null,
    h('p', { class: 'dim' }, `Każde wejście to osobny port: RTMP ${ports.rtmp[0]}–${ports.rtmp[1]}, SRT ${ports.srt[0]}–${ports.srt[1]} (UDP). ` +
      'Klucz RTMP i hasło SRT to hasła nadawcy – po zmianie podaj je nadawcy na nowo. Zapis działa w ciągu kilku sekund, bez restartu; trwający sygnał na zmienionym wejściu zostanie przerwany.'),
    data.from === 'env' ? h('p', { class: 'dim' }, 'Teraz obowiązuje VIZRS_INGEST z konfiguracji CG Playout; po zapisie – lista z panelu.') : null,
    data.ingest ? null : h('div', { class: 'note warn' }, 'Odbiór sygnałów w CG Playout jest wyłączony (brak VIZRS_INGEST_TOKEN) – wejścia zapiszą się, ale nie zadziałają.'),
    list);
  if (!await dialog('Wejścia studia', body, [['Anuluj', false], ['Zapisz', true, 'acc']])) return;
  try {
    await cg('live/inputs', 'PUT', { listeners: rows.map((r) => ({ channel: r.channel, url: inputUrl(r) })) });
    toast('Wejścia zapisane w CG Playout – odbiorniki przejmą je w ciągu kilku sekund');
  } catch (e) { toast(e.message, 'bad'); }
  route(true);
}

// ---------- Sloty na żywo: karta kanału ----------
// ZDEJMIJ jak w MCR: pierwsze kliknięcie uzbraja („POTWIERDŹ ZDJĘCIE” przez 3 s), drugie zdejmuje studio.
let armed = null;
let armTimer = null;

async function act(c, path, okMsg) {
  try { await cg(`live/channels/${encodeURIComponent(c.number)}/${path}`, 'POST'); toast(okMsg); } catch (e) { toast(e.message, 'bad'); }
  route(true);
}

/** Linia „teraz”: ostrzega, gdy audycja LIVE trwa albo zaczyna się w ciągu 10 min, a studia nie ma. */
function nowLine(c, signal) {
  const now = cgNow();
  const next = c.next_live;
  if (c.now?.live && !signal) {
    return [true, h('span', null, '⚠ ', h('b', null, c.now.title), ' trwa – brak studia'), h('span', null, `do ${at(c.now.stop)}`)];
  }
  if (next && next.start > now && next.start - now < 10 * 60000 && !signal) {
    return [true, h('span', null, '⚠ ', h('b', null, next.title), ' za ', h('span', { class: 'mono', 'data-until': next.start }, short((next.start - now) / 1000))), h('span', null, 'brak studia')];
  }
  if (c.now) return [false, h('span', null, 'Teraz: ', c.now.live ? [h('b', null, c.now.title), ' ', pill('LIVE', 'on')] : c.now.title), h('span', null, `do ${at(c.now.stop)}`)];
  if (next) return [false, h('span', null, 'Następna LIVE: ', h('b', null, next.title)), h('span', null, `${day(next.start)} ${at(next.start)}`)];
  return [false, h('span', { class: 'dim' }, 'Brak ramówki z ErsatzTV.'), h('span')];
}

function channelCard(c, receivedAt) {
  const src = c.source?.live ? c.source : null;
  const tally = c.hold ? h('span', { class: 'tally held' }, 'WSTRZYMANY') : c.on_air ? h('span', { class: 'tally air' }, 'NA ANTENIE') : h('span', { class: 'tally sched' }, 'RAMÓWKA');
  const thumb = src
    ? h('div', { class: 'thumb sig' + (c.hold ? ' held' : '') },
      h('span', { class: 'rec' }, h('span', { class: 'dot ' + (c.hold ? 'y' : 'r') }), h('span', { class: 'mono', 'data-since': receivedAt - (src.seconds || 0) * 1000 }, clock(src.seconds || 0))),
      h('div', null, h('div', { class: 'big-t' }, c.hold ? 'SYGNAŁ WSTRZYMANY' : 'SYGNAŁ ZE STUDIA'), h('div', { class: 'dim' }, `${src.video || '?'} · ${src.audio ? 'z dźwiękiem' : 'bez dźwięku'}`)))
    : h('div', { class: 'thumb' }, 'brak sygnału ze studia');
  const [warn, left, right] = nowLine(c, !!src);
  const isArmed = armed === c.number;
  const main = c.hold
    ? btn('▶ WPUŚĆ', () => act(c, 'release', `Kanał ${c.number}: studio wpuszczone`), 'big on')
    : btn(isArmed ? '■ POTWIERDŹ ZDJĘCIE' : '■ ZDEJMIJ', () => {
      if (armed !== c.number) {
        armed = c.number;
        clearTimeout(armTimer);
        armTimer = setTimeout(() => { if (armed === c.number) { armed = null; repaint(); } }, 3000);
        repaint();
        return;
      }
      armed = null;
      act(c, 'hold', `Kanał ${c.number}: studio zdjęte`);
    }, 'big off' + (isArmed ? ' arm' : ''));
  const anytimeNow = anytimeShown(c);
  const anytime = h('input', { type: 'checkbox', checked: anytimeNow, 'aria-label': `Studio poza audycjami ${c.number}` });
  anytime.addEventListener('change', async () => {
    const value = anytime.checked;
    anytimeSent.set(c.number, { value, until: Date.now() + 10000 });
    anytime.disabled = true;
    try { await cg(`live/channels/${encodeURIComponent(c.number)}`, 'PUT', { anytime: value }); toast('Zapisane w CG Playout'); } catch (e) { anytimeSent.delete(c.number); toast(e.message, 'bad'); }
    route(true);
  });
  const how = h('details', { class: 'how' }, h('summary', null, 'Jak nadawać na ten kanał'), inputsBox(c));
  how.open = opened.has(c.number);
  how.addEventListener('toggle', () => { if (how.open) opened.add(c.number); else opened.delete(c.number); });
  return h('div', { class: 'panel live-ch' + (c.on_air ? ' onair' : warn ? ' warn' : ''), 'data-live-channel': c.number },
    h('div', { class: 'live-h' }, h('span', { class: 'num' }, c.number), h('span', { class: 'name', title: c.name || '' }, c.name || `kanał ${c.number}`), tally),
    thumb,
    h('dl', { class: 'kv' },
      h('dt', null, 'Sygnał studia'), h('dd', null, src ? `${src.video || '?'}${src.audio ? ' + dźwięk' : ' bez dźwięku'}` : 'brak'),
      h('dt', null, 'Studio'), h('dd', null, (anytimeNow ? 'także poza audycjami' : 'tylko w audycjach LIVE') + (c.anytime_own == null ? ' (konfiguracja CG)' : ' (panel)')),
      // przejęcie przez nakładkę (CG Playout z `on_live` w features): grafika ramówki wstrzymana, na antenie grafika live kanału
      'takeover' in c ? [h('dt', null, 'Grafika'), h('dd', { 'data-live-graphics': '' }, c.takeover ? pill('live – ramówka wstrzymana', 'on') : 'z ramówki')] : null,
      h('dt', null, 'Najbliższa LIVE'), h('dd', null, c.next_live ? `${c.next_live.title} · ${day(c.next_live.start)} ${at(c.next_live.start)}` : '—')),
    h('div', { class: 'now' + (warn ? ' warn' : '') }, left, right),
    h('div', { class: 'bar ctl' }, main,
      h('label', { class: 'chk' }, anytime, 'studio także poza audycjami'),
      btn('⟲ Przebuduj ramówkę', () => act(c, 'rebuild', `Kanał ${c.number}: ramówka przebudowana – EPG odświeży się za chwilę`))),
    how);
}

// ---------- Sloty na żywo: tabela wejść (jak w makiecie) ----------
/** Adres wejścia bez sekretów (klucz RTMP i hasło SRT są tylko w „Jak nadawać na ten kanał”). */
function inputAddr(i) {
  const host = i.host || location.hostname;
  if (i.kind === 'rtmp') return [`rtmp://${host}:${i.port}${i.app || ''}`, i.key ? ' + klucz' : ''];
  return [`srt://${host}:${i.port}` + (i.streamid ? `?streamid=${encodeURIComponent(i.streamid)}` : ''), i.passphrase ? ' + hasło' : ''];
}
function inputsTable(list) {
  const rows = list.flatMap((c) => (c.inputs || []).map((i, n) => ({ c, i, n })));
  return table(rows, [
    [({ c }) => h('span', null, h('span', { class: 'num' }, c.number), ' ', c.name || ''), 'Kanał'],
    [({ i }) => (i.kind === 'rtmp' ? 'RTMP' : 'SRT'), 'Wejście'],
    [({ i }) => { const [a, extra] = inputAddr(i); return h('span', null, h('span', { class: 'mono' }, a), h('span', { class: 'dim' }, extra)); }, 'Adres'],
    [({ c }) => (c.hold ? pill('wstrzymany', 'warn') : c.source?.live ? pill(c.on_air ? 'nadaje · na antenie' : 'nadaje', c.on_air ? 'on' : 'ok') : pill('czeka')), 'Stan'],
    [({ c }) => (c.source?.live ? `${c.source.video || '?'} · ${c.source.audio ? 'z dźwiękiem' : 'bez dźwięku'}` : h('span', { class: 'dim' }, 'brak sygnału')), 'Obraz'],
  ], { empty: 'Żaden kanał nie ma wejścia studia – „Ustaw wejścia…”.' });
}

// ---------- Sloty na żywo: ramówka i zdarzenia ----------
const GUIDE_KEY = 'vizrs-live-guide';

function guideBox(state) {
  const list = (state.channels || []).filter((c) => onChannel(c.number));
  let ch = store.get(GUIDE_KEY);
  if (!list.some((c) => c.number === ch) && list.length) ch = list[0].number;
  const now = cgNow();
  const items = state.guide?.[ch] || [];
  return [
    h('div', { class: 'bar tabs-ch', role: 'tablist' }, list.map((c) => btn(`${c.number} ${c.name || ''}`.trim(), () => { store.set(GUIDE_KEY, c.number); repaint(); }, c.number === ch ? 'sel' : ''))),
    items.length
      ? h('div', { class: 'guide', 'data-guide': ch }, items.map((p) => {
        const cur = p.start <= now && now < p.stop;
        return h('div', { class: 'slot' + (cur ? ' cur' : '') }, h('span', { class: 'mono' }, at(p.start)),
          h('span', null, p.live ? h('b', null, p.title) : p.title),
          p.live ? pill(cur ? 'LIVE' : `LIVE ${at(p.start)}`, cur ? 'on' : 'warn') : h('span'));
      }))
      : h('div', { class: 'empty' }, state.etv?.configured === false ? 'CG Playout bez VIZRS_ETV_URL – nie zna ramówki.' : 'Brak ramówki z ErsatzTV.'),
  ];
}

function eventsBox(state) {
  const ev = state.events || [];
  if (!ev.length) return h('div', { class: 'empty' }, 'Brak zdarzeń od startu CG Playout.');
  return h('div', { class: 'tl' }, ev.slice(0, 40).map((e) => {
    const d = new Date(e.at);
    return h('div', { class: 'ev' }, h('span', { class: 'mono' }, `${hm(d)}:${pad2(d.getSeconds())}`), e.channel ? pill(`kanał ${e.channel}`) : h('span'), h('span', null, e.text));
  }));
}

/** Lampki: CG Playout, ErsatzTV (z punktu widzenia CG), odbiór sygnałów. */
function statusBar(state) {
  const live = (state.channels || []).filter((c) => c.source?.live).length;
  const etv = state.etv || {};
  const lamp = (text, kind, title) => h('span', { class: 'pill ' + kind, title: title || '' }, text);
  return h('div', { class: 'bar live-status', 'data-live-status': '' },
    lamp(`CG Playout ${state.version || ''}`.trim(), 'ok'),
    etv.configured === false ? lamp('ErsatzTV: brak VIZRS_ETV_URL w CG', '') : lamp(etv.ok === false ? 'ErsatzTV: błąd' : 'ErsatzTV: odpowiada', etv.ok === false ? 'bad' : 'ok', etv.error || ''),
    state.ingest ? lamp(`odbiór sygnałów: ${live}`, live ? 'on' : 'ok') : lamp('odbiór sygnałów wyłączony (VIZRS_INGEST_TOKEN)', 'warn'),
    h('span', { class: 'dim' }, `operator: ${operatorName() || '–'} (pasek górny)`));
}

// Stan widoku „Sloty na żywo”: odświeżany co sekundę w miejscu, przebudowa tylko po zmianie treści.
let view = null;

/** Podpis stanu bez upływu czasu: czasy do minuty, bez liczników sekund. */
function signature(state) {
  return JSON.stringify(state, (k, v) => (k === 'now' && typeof v === 'number') || k === 'seconds' || k === 'checked' ? undefined
    : (k === 'start' || k === 'stop' || k === 'at') && typeof v === 'number' ? Math.floor(v / 60000) : v);
}

function repaint(initial) {
  if (!view || (!initial && !view.root.isConnected)) return;
  const { state, receivedAt } = view;
  const list = (state.channels || []).filter((c) => onChannel(c.number));
  view.status.replaceChildren(statusBar(state));
  view.cards.replaceChildren(...(list.length ? list.map((c) => channelCard(c, receivedAt)) : [h('div', { class: 'note' }, 'CG Playout nie zna jeszcze kanałów (synchronizacja z legacy co 30 s), a żaden nadawca nie nadaje.')]));
  view.inputs.replaceChildren(inputsTable(list));
  view.guide.replaceChildren(...guideBox(state));
  view.events.replaceChildren(eventsBox(state));
  tick();
}

async function poll() {
  if (!view || !view.root.isConnected) return;
  let state;
  const t0 = Date.now();
  try { state = await cg('live/state'); } catch (e) { view.status.replaceChildren(cgError(e)); return; }
  skew = (t0 + Date.now()) / 2 - state.now;
  const sig = signature(state);
  // przebudowa podczas wciśniętego przycisku zgubiłaby kliknięcie – wtedy czeka na następny cykl
  if (sig === view.sig || document.querySelector('main button:active')) return;
  Object.assign(view, { state, sig, receivedAt: cgNow() });
  repaint();
}

VIEWS.live = async () => {
  const title = 'Sloty na żywo';
  const desc = 'Stanowisko MCR: sygnały studia (RTMP, SRT) z VizRS CG Playout, ZDEJMIJ / WPUŚĆ, ramówka z audycjami LIVE i dziennik zdarzeń. Odświeżane co sekundę.';
  if (!S.status.cg) return [head(title, desc), cgMissing()];
  let state;
  const t0 = Date.now();
  try { state = await cg('live/state'); } catch (e) { return [head(title, desc), cgError(e)]; }
  skew = (t0 + Date.now()) / 2 - state.now;
  const root = h('div', { class: 'split wide' });
  view = { root, state, sig: signature(state), receivedAt: cgNow(), status: h('div'), cards: h('div', { class: 'live-grid' }), guide: h('div'), events: h('div'), inputs: h('div', { class: 'tw', 'data-inputs-table': '' }) };
  root.append(
    h('div', null, view.cards),
    h('div', { class: 'cols' },
      panel('Ramówka', h('span', { class: 'dim' }, 'EPG ErsatzTV, LIVE = audycja na żywo'), view.guide),
      panel('Zdarzenia', null, view.events, cmdNote('CG Playout /api/live/state'))));
  repaint(true);
  S.timers.push(setInterval(poll, 1000), setInterval(tick, 500));
  return [
    head(title, desc, btn('Audycje LIVE →', () => { location.hash = '#audycje'; })),
    view.status,
    h('div', { class: 'note' }, 'Jak to działa: nadawca (OBS, koder) wysyła obraz na wejście kanału – dane w „Jak nadawać na ten kanał”. ' +
      'Gdy sygnał dojdzie, kanał przechodzi na studio od pierwszej klatki kluczowej: zawsze („studio także poza audycjami”) albo tylko w czasie audycji LIVE z ramówki. ' +
      'Grafika VizRS zostaje na wierzchu. Koniec nadawania = powrót do ramówki. ZDEJMIJ (dwa kliknięcia) odcina studio mimo sygnału, aż do WPUŚĆ.'),
    root,
    panel('Wejścia studia', h('span', { class: 'bar' }, h('span', { class: 'dim' }, 'stan sygnału; klucze i hasła – „Jak nadawać na ten kanał” w karcie kanału'),
      btn('Ustaw wejścia…', () => editInputs())), view.inputs, cmdNote('CG Playout /api/live/state (inputs, source), /api/live/inputs')),
  ];
};

// ---------- Audycje LIVE ----------
const STEP = { file: 'Audycja zapisana (plik Remote Stream)', scan: 'Biblioteka „Na żywo” – skan', collection: 'Kolekcja w ErsatzTV', delete: 'Usunięcie' };

async function rebuild(channel) {
  try { await cg(`live/channels/${encodeURIComponent(channel)}/rebuild`, 'POST'); toast(`Kanał ${channel}: ramówka przebudowana – EPG odświeży się za chwilę`); } catch (e) { toast(e.message, 'bad'); }
}

/** Kreator po zapisie (i „jak dodać?”): kroki CG Playout, potem pozycja w Harmonogramach VizRS Live i przebudowa. */
async function doneDialog(title, data) {
  const steps = (data?.steps || []).map((s) => h('li', { class: s.ok ? 'ok' : 'bad' }, h('span', { class: 'ic' }, s.ok ? '✓' : '!'), STEP[s.step] || s.step, s.detail ? h('small', { class: 'dim' }, ' ' + s.detail) : null));
  const show = data?.show;
  const body = h('div', { class: 'wizard', 'data-wizard': '' },
    steps.length ? h('ol', { class: 'steps' }, steps) : null,
    show ? h('div', { class: 'note' },
      h('b', null, `Ostatni krok – wstaw audycję do ramówki kanału ${show.channel}:`),
      h('ol', null,
        h('li', null, 'Harmonogramy → harmonogram kanału → + Pozycja.'),
        h('li', null, 'Start: stała godzina (godzina emisji), zachowanie: ściśle.'),
        h('li', null, 'Treść: kolekcja inteligentna → ', h('b', { class: 'mono' }, show.collection_name || `LIVE · ${show.title}`), '.'),
        h('li', null, 'Tryb: jedna pozycja (One); opcjonalnie element graficzny VizRS.'),
        h('li', null, 'Zapisz pozycje, potem „Przebuduj ramówkę” – audycja pojawi się w EPG i tutaj w „Najbliższa emisja”.'))) : null);
  const r = await dialog(title, body, show
    ? [['Gotowe', null], [`⟲ Przebuduj ramówkę kanału ${show.channel}`, 'rebuild'], ['Otwórz Harmonogramy', 'open', 'acc']]
    : [['Gotowe', null, 'acc']]);
  if (r === 'rebuild') { await rebuild(show.channel); route(true); }
  if (r === 'open') location.hash = '#harmonogramy';
}

async function showDialog(channels, show) {
  const f = {
    channel: h('select', { id: 'ls-channel' }, channels.map((c) => h('option', { value: c.number, selected: show?.channel === c.number }, `${c.number} · ${c.name || ''}`))),
    title: h('input', { id: 'ls-title', value: show?.title || '', maxlength: 200 }),
    duration: h('input', { id: 'ls-duration', value: show?.duration || '01:00:00', placeholder: 'gg:mm:ss', class: 'mono' }),
    plot: h('textarea', { id: 'ls-plot', rows: 3, maxlength: 2000 }),
  };
  f.plot.value = show?.plot || '';
  if (show) f.channel.disabled = true;
  const body = h('div', { class: 'form' },
    h('label', { for: 'ls-channel' }, 'Kanał'), f.channel,
    h('label', { for: 'ls-title' }, 'Tytuł'), f.title,
    h('span'), h('span', { class: 'dim' }, 'widoczny w EPG i jako {title} w grafice VizRS'),
    h('label', { for: 'ls-duration' }, 'Długość'), f.duration,
    h('label', { for: 'ls-plot' }, 'Opis'), f.plot,
    h('span'), h('span', { class: 'dim' }, 'w czasie audycji bez sygnału studia kanał pokazuje czerń z ciszą'));
  if (!(await dialog(show ? 'Audycja LIVE' : 'Nowa audycja LIVE', body, [['Anuluj', false], [show ? 'Zapisz w ErsatzTV' : 'Utwórz w ErsatzTV', true, 'acc']]))) return;
  const data = { channel: f.channel.value, title: f.title.value.trim(), duration: f.duration.value.trim(), plot: f.plot.value };
  let r;
  try {
    r = show ? await cg('live/shows/' + encodeURIComponent(show.id), 'PUT', data) : await cg('live/shows', 'POST', data);
  } catch (e) { toast(e.message, 'bad'); route(true); return; }
  await route(true);
  await doneDialog(`„${r?.show?.title || data.title}” ${show ? 'zmieniona' : 'jest'} w ErsatzTV`, r);
}

/** Biblioteka „Na żywo”: Remote Streams z folderem /config/vizrs-live – wybór z bibliotek legacy zamiast wpisywania ID. */
async function libraryPanel(shows) {
  const settings = shows.settings || {};
  const lib = settings.library;
  const libs = has('GetAllLocalLibraries') ? (await cmd('GetAllLocalLibraries').catch(() => [])).filter((l) => l.MediaKind === 'RemoteStreams') : [];
  const pick = h('select', { 'aria-label': 'Biblioteka audycji LIVE' },
    h('option', { value: '' }, '— nie ustawiona —'),
    libs.map((l) => h('option', { value: l.Id, selected: l.Id === lib }, `${l.Name} (ID ${l.Id})`)),
    lib != null && !libs.some((l) => l.Id === lib) ? h('option', { value: lib, selected: true }, `ID ${lib} (nie ma w legacy)`) : null);
  let paths = null;
  if (lib != null && has('GetLocalLibraryPaths')) paths = await cmd('GetLocalLibraryPaths', { LocalLibraryId: lib }).catch(() => null);
  const hasFolder = paths?.some((p) => /\/vizrs-live\/?$/.test(p.Path || ''));
  const etv = shows.etv || {};
  return panel('Połączenie z ErsatzTV', null,
    h('dl', { class: 'kv' },
      h('dt', null, 'CG Playout ↔ legacy'), h('dd', null, etv.configured === false ? pill('brak VIZRS_ETV_URL', 'bad') : etv.ok ? pill('odpowiada', 'ok') : pill(etv.error || 'sprawdzam…', 'warn')),
      h('dt', null, 'Biblioteka'), h('dd', null, lib != null ? pill(`ID ${lib}`, 'ok') : pill('nie ustawiona', 'bad'),
        paths == null ? null : hasFolder ? ' z folderem /config/vizrs-live' : pill('bez folderu /config/vizrs-live', 'warn')),
      h('dt', null, 'Zapis audycji'), h('dd', null, settings.store === false ? pill('CG bez VIZRS_ETV_CONFIG – nie zapisze', 'bad') : pill('tak', 'ok'))),
    h('div', { class: 'live-side' }, h('div', { class: 'bar' }, pick, btn('Zapisz bibliotekę', async () => {
      try { await cg('live/settings', 'PUT', { library: pick.value === '' ? null : Number(pick.value) }); toast('Biblioteka audycji LIVE zapisana w CG Playout'); } catch (e) { toast(e.message, 'bad'); }
      route(true);
    })),
    h('p', { class: 'dim' }, libs.length
      ? 'Biblioteka Remote Streams w legacy z folderem /config/vizrs-live – tam CG Playout zapisuje pliki audycji.'
      : 'Legacy nie ma biblioteki Remote Streams. Załóż ją w Media → Biblioteki lokalne (rodzaj: strumienie zdalne) ze ścieżką /config/vizrs-live, potem wybierz tutaj.')),
    cmdNote('GetAllLocalLibraries', 'GetLocalLibraryPaths', 'CG Playout PUT /api/live/settings'));
}

function airCell(s, shows) {
  const now = cgNow();
  if (s.next) {
    const more = s.airings > 1 ? ` · +${s.airings - 1}` : '';
    return s.next.start <= now ? pill(`● teraz · do ${at(s.next.stop)}${more}`, 'on') : `${day(s.next.start)} ${at(s.next.start)}${more}`;
  }
  if (shows.guide_known === false) return h('span', { class: 'dim' }, '–');
  return h('span', { class: 'bar' }, pill('nie ma w ramówce', 'warn'), btn('jak dodać?', () => doneDialog(`Jak dodać „${s.title}” do ramówki`, { steps: [], show: s })));
}

function statusCell(s, shows) {
  const lib = shows.settings?.library != null;
  const coll = s.collection === true ? 'ok' : s.collection == null ? '' : 'bad';
  return h('span', { class: 'bar' }, pill('plik', s.file_ok === false ? 'bad' : 'ok'), pill('biblioteka', lib ? 'ok' : 'bad'), pill('kolekcja', coll));
}

VIEWS.audycje = async () => {
  const title = 'Audycje LIVE';
  const desc = 'Audycje na żywo kanałów: CG Playout zakłada w legacy plik Remote Stream i kolekcję inteligentną „LIVE · tytuł”; godzinę emisji daje pozycja w Harmonogramach.';
  if (!S.status.cg) return [head(title, desc), cgMissing()];
  let state, shows;
  const t0 = Date.now();
  try { [state, shows] = await Promise.all([cg('live/state'), cg('live/shows')]); } catch (e) { return [head(title, desc), cgError(e)]; }
  skew = (t0 + Date.now()) / 2 - state.now;
  const channels = shows.channels?.length ? shows.channels : state.channels || [];
  const del = async (s) => {
    if (!(await confirmDlg('Usunąć audycję LIVE?', `„${s.title}” (kanał ${s.channel}): ErsatzTV usunie ją z biblioteki i zdejmie z harmonogramów, w których jest.`, 'Usuń'))) return;
    let r;
    try { r = await cg('live/shows/' + encodeURIComponent(s.id), 'DELETE'); } catch (e) { toast(e.message, 'bad'); route(true); return; }
    await route(true);
    if (r?.steps?.length) await doneDialog(`„${s.title}” usunięta`, r); else toast('Audycja usunięta');
  };
  const numbers = [...new Set([...channels.map((c) => c.number), ...(shows.shows || []).map((s) => s.channel)])];
  const full = shows.max != null && (shows.shows || []).length >= shows.max;
  return [
    head(title, desc,
      btn('⟳ Odśwież z ErsatzTV', async () => {
        try {
          const r = await cg('live/shows/scan', 'POST');
          const bad = (r?.steps || []).filter((s) => !s.ok);
          toast(bad.length ? bad.map((s) => s.detail || s.step).join('; ') : 'Biblioteka przeskanowana, kolekcje gotowe', bad.length ? 'bad' : null);
        } catch (e) { toast(e.message, 'bad'); }
        route(true);
      }),
      full ? null : btn('+ Nowa audycja LIVE', () => showDialog(channels), 'acc')),
    h('div', { class: 'split wide' },
      panel('Audycje', null,
        shows.error ? h('div', { class: 'note err' }, 'shows.json: ' + shows.error) : null,
        table(shows.shows || [], [
          ['channel', 'Kanał', { class: 'num' }],
          [(s) => h('span', { title: s.plot || '' }, s.title), 'Tytuł (EPG)'],
          ['duration', 'Długość', { class: 'num' }],
          [(s) => statusCell(s, shows), 'W ErsatzTV'],
          [(s) => (s.collection === true ? h('span', { class: 'mono' }, s.collection_name) : h('span', { class: 'dim' }, '– (⟳ Odśwież)')), 'Kolekcja do harmonogramu'],
          [(s) => airCell(s, shows), 'Najbliższa emisja'],
          [(s) => h('span', { class: 'bar' }, btn('✎', () => showDialog(channels, s)), btn('🗑', () => del(s), 'danger')), ''],
        ], { empty: 'Brak audycji LIVE. Dodaj pierwszą przyciskiem „+ Nowa audycja LIVE”.' }),
        h('p', { class: 'dim live-side' }, 'Godzinę emisji ustawiasz w Harmonogramach: pozycja z kolekcją „LIVE · …”, start stały, tryb One. Resztę robi CG Playout.'),
        cmdNote('CG Playout /api/live/shows')),
      h('div', { class: 'cols' },
        await libraryPanel(shows),
        panel('Przebuduj ramówkę', null, h('div', { class: 'live-side' },
          h('p', { class: 'dim' }, 'Po zmianie harmonogramu przebuduj playout kanału, żeby audycje trafiły do EPG.'),
          numbers.length ? h('div', { class: 'bar' }, numbers.map((n) => btn(`⟲ kanał ${n}`, async () => { await rebuild(n); route(true); }))) : h('div', { class: 'empty' }, 'Brak kanałów.'))))),
  ];
};
