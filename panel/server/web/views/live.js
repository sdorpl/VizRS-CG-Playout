// Widoki „Sloty na żywo” (stanowisko MCR) i „Audycje LIVE” – całe dawne VizRS Live Panel z VizRS CG Playout (`/live`, tam już usunięte)
// przeniesione do VizRS Live. Logika (odbiór sygnału, przejęcie kanału, pliki audycji) zostaje w CG Playout;
// panel rozmawia z nim przez pośrednika serwera /api/cg/live/… (token CG zostaje w serwerze).

import { VIEWS, S, h, head, panel, pill, btn, table, toast, cmd, cmdNote, confirmDlg, dialog, has, route, pad2, hm, pl, operatorName, onChannel } from '../core.js';
import { cg, allPages } from './grafika.js';
import { cmdEditor } from '../cmdedit.js';
import { fromLegacy, toReplace } from './harmonogramy.js';
import { minutesOf, hhmm, span } from '../parts.js';

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
    if (i.kind === 'youtube') {
      return h('div', { class: 'input' }, h('h4', null, 'YouTube (CG Playout pobiera sam)'),
        h('dl', { class: 'kv' }, ...plainField('Adres', i.url || '', `Adres YouTube ${id}`)),
        h('p', { class: 'dim' }, youtubeNote(i.youtube)));
    }
    if (i.kind === 'link') {
      return h('div', { class: 'input' }, h('h4', null, 'Link do strumienia (CG Playout pobiera sam)'),
        h('dl', { class: 'kv' }, ...secretField('Adres', i.url || '', `Link wejścia ${id}`)),
        h('p', { class: 'dim' }, LINK_NOTE));
    }
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
// YouTube: adres https://www.youtube.com/… albo https://youtu.be/…; strona live kanału (…/@nazwa/live) czeka na każdą transmisję,
// inny adres to jeden film – transmisja do końca albo nagranie raz, potem CG Playout sam usuwa wejście z listy.
const YOUTUBE = /^https?:\/\/((www|m|music)\.)?(youtube\.com|youtu\.be)\/\S+$/i;
const youtubeChannel = (url) => /\/[^/?#]+\/live\/?(\?|#|$)/i.test(url.replace(/^https?:\/\/[^/]+/i, ''));
const youtubeNote = (what) => (what === 'channel'
  ? 'Strona live kanału: każda jego transmisja przejmuje antenę, wejście zostaje na liście.'
  : 'Jeden film: transmisja do jej końca albo nagranie raz, w czasie rzeczywistym – potem wejście znika z listy.');
// Link: strumień, który CG Playout sam pobiera – HLS (.m3u8) i inny strumień HTTP, RTMP(S), SRT (caller), RTSP – z dowolnego hosta
// spoza tej maszyny (rtmp:// i srt:// na 0.0.0.0 i adresie lokalnym to odbiornik, do którego przychodzi nadawca). Wejście zostaje na liście; po końcu strumienia pobiera go znowu.
const LINK = /^(https?|rtmps?|srt|rtsps?):\/\/[^\s/?#;]+([/?#]\S*)?$/i;
const LISTEN_HOST = /^\w+:\/\/([^@/?#]*@)?(0\.0\.0\.0|\[::1?\]|127\.[\d.]+|localhost)(:|\/|\?|$)/i;
const YOUTUBE_HOST = /^https?:\/\/((www|m|music)\.)?(youtube\.com|youtu\.be)(:|\/|\?|$)/i;
const isLink = (url) => LINK.test(url) && !LISTEN_HOST.test(url) && !YOUTUBE_HOST.test(url);
const LINK_HELP = 'link do strumienia: https://…/index.m3u8, rtmp(s)://serwer/app/klucz, srt://serwer:port albo rtsp://… (nie 0.0.0.0 ani 127.0.0.1 – to adres odbiornika RTMP/SRT)';
const LINK_NOTE = 'CG Playout sam pobiera strumień (H.264 + AAC, bez przekodowania) i przejmuje nim kanał jak sygnałem ze studia; gdy strumień się skończy albo zerwie, pobiera go znowu – wejście zostaje na liście.';
/** Link bez sekretów: bez loginu, zapytania (tokeny) i ostatniej części ścieżki RTMP (klucz). */
const linkShown = (url) => {
  const [base, query] = url.replace(/^(\w+:\/\/)[^@/]*@/, '$1***@').split('?');
  return (/^rtmps?:/i.test(base) && base.split('/').length > 4 ? base.replace(/\/[^/]+$/, '/***') : base) + (query ? '?***' : '');
};
const randomKey = () => Array.from(crypto.getRandomValues(new Uint8Array(12)), (b) => b.toString(16).padStart(2, '0')).join('');

function parseInput(l) {
  const r = parseAddress(l);
  r.on_live = l.on_live || '';
  return r;
}

function parseAddress(l) {
  if (YOUTUBE.test(l.url)) return { channel: l.channel, kind: 'youtube', url: l.url, port: 0, app: '', secret: '', rest: '' };
  if (isLink(l.url)) return { channel: l.channel, kind: 'link', url: l.url, port: 0, app: '', secret: '', rest: '' };
  const m = /^(rtmp|srt):\/\/[^/:?]*:(\d+)([^?]*)\??(.*)$/i.exec(l.url) || [];
  const kind = (m[1] || 'rtmp').toLowerCase();
  const path = m[3] || '';
  const params = new URLSearchParams(m[4] || '');
  const secret = kind === 'rtmp' ? path.split('/').pop() : params.get('passphrase') || '';
  params.delete('passphrase');
  return { channel: l.channel, kind, port: +m[2] || 0, app: kind === 'rtmp' ? path.slice(0, path.length - secret.length - 1) || '/live' : '', secret, rest: params.toString() };
}

function inputUrl(r) {
  if (r.kind === 'youtube' || r.kind === 'link') return r.url;
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
  // Grafika wejścia (CG Playout z `input_on_live` w features): na czas przejęcia z tego wejścia zamiast grafiki live kanału.
  const ownGraphics = (view?.state.features || []).includes('input_on_live');
  const pages = ownGraphics ? await allPages(view?.state.channels || []) : new Map();
  const ports = data.ports || { rtmp: [1936, 1943], srt: [9000, 9007] };
  const range = (k) => { const [a, b] = ports[k]; return Array.from({ length: b - a + 1 }, (_, i) => a + i); };
  const rows = (data.listeners || []).map(parseInput);
  const freePort = (kind) => range(kind).find((p) => !rows.some((r) => r.kind === kind && r.port === p)) || ports[kind][0];
  const channels = [...new Set([...(view?.state.channels || []).map((c) => c.number), ...rows.map((r) => r.channel)])];
  const nameOf = (n) => (view?.state.channels || []).find((c) => c.number === n)?.name || '';
  if (forChannel != null) rows.push({ channel: forChannel, kind: 'rtmp', port: freePort('rtmp'), app: '/live', secret: randomKey(), rest: '' });
  const list = h('div', { class: 'inputs-edit', 'data-inputs-edit': '' });
  const opt = (v, t, sel) => h('option', { value: String(v), selected: sel }, t);
  // edytor grafiki wejścia – jeden na wiersz, zachowany przy przerysowaniu
  const graphicsOf = (r, n) => {
    if (!ownGraphics) return null;
    if (!r.ed) {
      r.ed = cmdEditor({ id: `in-gr-${n}`, label: `Grafika wejścia ${n + 1}`, value: r.on_live || null, pages, scoped: true, noOff: true, inheritText: 'grafika live kanału' });
    }
    return h('div', { class: 'in-graphics' }, h('span', { class: 'dim' }, 'Na antenie z tego wejścia:'), r.ed.el);
  };
  const row = (r, n, ...parts) => h('div', { class: 'in-row' }, h('div', { class: 'bar' }, ...parts), graphicsOf(r, n));
  function draw() {
    list.replaceChildren(...(rows.length ? rows.map((r, n) => {
      const ch = h('select', { 'aria-label': `Kanał wejścia ${n + 1}` }, channels.map((c) => opt(c, `${c} ${nameOf(c)}`.trim(), c === r.channel)));
      ch.addEventListener('change', () => { r.channel = ch.value; });
      const kind = h('select', { 'aria-label': `Rodzaj wejścia ${n + 1}` }, opt('rtmp', 'RTMP', r.kind === 'rtmp'), opt('srt', 'SRT', r.kind === 'srt'), opt('youtube', 'YouTube', r.kind === 'youtube'), opt('link', 'Link (HLS, RTMP, SRT, RTSP)', r.kind === 'link'));
      kind.addEventListener('change', () => {
        r.kind = kind.value;
        if (r.kind === 'youtube' || r.kind === 'link') r.url = r.url || ''; else { r.port = freePort(r.kind); r.app = '/live'; if (!r.secret) r.secret = randomKey(); }
        draw();
      });
      const del = btn('×', () => { rows.splice(n, 1); draw(); }, 'danger');
      if (r.kind === 'youtube') {
        const url = h('input', { value: r.url || '', class: 'mono grow', placeholder: 'https://www.youtube.com/watch?v=… albo https://www.youtube.com/@kanał/live', 'aria-label': `Adres YouTube wejścia ${n + 1}` });
        const note = h('span', { class: 'dim' }, r.url ? youtubeNote(youtubeChannel(r.url) ? 'channel' : 'video') : '');
        url.addEventListener('input', () => { r.url = url.value.trim(); note.textContent = YOUTUBE.test(r.url) ? youtubeNote(youtubeChannel(r.url) ? 'channel' : 'video') : 'adres YouTube: https://www.youtube.com/… albo https://youtu.be/…'; });
        return row(r, n, ch, kind, url, del, note);
      }
      if (r.kind === 'link') {
        const url = h('input', { value: r.url || '', class: 'mono grow', placeholder: 'https://serwer/live/index.m3u8, rtmp://serwer/app/klucz, srt://serwer:9000, rtsp://kamera/…', 'aria-label': `Link wejścia ${n + 1}` });
        const note = h('span', { class: 'dim' }, '');
        const explain = () => { note.textContent = !r.url || isLink(r.url) ? '' : LINK_HELP; };
        url.addEventListener('input', () => { r.url = url.value.trim(); explain(); });
        explain();
        return row(r, n, ch, kind, url, del, note);
      }
      const port = h('select', { 'aria-label': `Port wejścia ${n + 1}` }, range(r.kind).map((p) => opt(p, String(p), p === r.port)));
      if (!range(r.kind).includes(r.port)) port.prepend(opt(r.port, `${r.port} (spoza zakresu)`, true));
      port.addEventListener('change', () => { r.port = +port.value; });
      const secret = h('input', { value: r.secret, class: 'mono grow', placeholder: r.kind === 'rtmp' ? 'klucz strumienia' : 'hasło 10–79 znaków (puste = bez hasła)', 'aria-label': `${r.kind === 'rtmp' ? 'Klucz' : 'Hasło'} wejścia ${n + 1}` });
      secret.addEventListener('input', () => { r.secret = secret.value.trim(); });
      return row(r, n, ch, kind, port, secret,
        btn('Losuj', () => { r.secret = randomKey(); draw(); }),
        del);
    }) : [h('div', { class: 'empty' }, 'Brak wejść – żaden kanał nie przyjmie sygnału ze studia.')]),
    btn('+ Wejście', () => { rows.push({ channel: channels[0] || '1', kind: 'rtmp', port: freePort('rtmp'), app: '/live', secret: randomKey(), rest: '' }); draw(); }));
  }
  draw();
  const body = h('div', null,
    h('p', { class: 'dim' }, `Każde wejście to osobny port: RTMP ${ports.rtmp[0]}–${ports.rtmp[1]}, SRT ${ports.srt[0]}–${ports.srt[1]} (UDP). ` +
      'Klucz RTMP i hasło SRT to hasła nadawcy – po zmianie podaj je nadawcy na nowo. Zapis działa w ciągu kilku sekund, bez restartu; trwający sygnał na zmienionym wejściu zostanie przerwany. ' +
      'Wejście YouTube nie zajmuje portu: CG Playout sam pobiera transmisję albo nagranie (H.264 do 1080p) i przejmuje kanał jak sygnał ze studia – poza audycjami LIVE tylko przy „studio także poza audycjami”. ' +
      'Tak samo wejście „Link”: CG Playout sam pobiera strumień spod adresu (HLS .m3u8, RTMP, SRT, RTSP; obraz H.264 i dźwięk AAC).'),
    data.from === 'env' ? h('p', { class: 'dim' }, 'Teraz obowiązuje VIZRS_INGEST z konfiguracji CG Playout; po zapisie – lista z panelu.') : null,
    data.ingest ? null : h('div', { class: 'note warn' }, 'Odbiór sygnałów w CG Playout jest wyłączony (brak VIZRS_INGEST_TOKEN) – wejścia zapiszą się, ale nie zadziałają.'),
    ownGraphics ? h('p', { class: 'dim' }, 'Grafika wejścia (np. logo z dopiskiem NA ŻYWO) wchodzi, gdy kanał przejmuje sygnał z tego wejścia – zamiast grafiki live kanału z ekranu Grafika kanału; po zejściu sygnału wraca grafika ramówki.') : null,
    list);
  if (!await dialog('Wejścia studia', body, [['Anuluj', false], ['Zapisz', true, 'acc']])) return;
  const bad = rows.find((r) => r.kind === 'youtube' && !YOUTUBE.test(r.url || ''));
  if (bad) { toast(`Kanał ${bad.channel}: to nie jest adres YouTube (https://www.youtube.com/… albo https://youtu.be/…)`, 'bad'); return; }
  const badLink = rows.find((r) => r.kind === 'link' && !isLink(r.url || ''));
  if (badLink) { toast(`Kanał ${badLink.channel}: ${LINK_HELP}`, 'bad'); return; }
  let listeners;
  try {
    listeners = rows.map((r) => {
      const item = { channel: r.channel, url: inputUrl(r) };
      if (ownGraphics) item.on_live = r.ed ? r.ed.read() : r.on_live || null;
      return item;
    });
  } catch (e) { toast(e.message, 'bad'); return; }
  try {
    await cg('live/inputs', 'PUT', { listeners });
    toast('Wejścia zapisane w CG Playout – odbiorniki przejmą je w ciągu kilku sekund');
  } catch (e) { toast(e.message, 'bad'); }
  route(true);
}

// ---------- Ciasteczka YouTube (yt-dlp wejść YouTube) ----------
// YouTube każe serwerom (adresy serwerowni) się zalogować („Sign in to confirm you're not a bot”) –
// CG Playout podaje yt-dlp ciasteczka zalogowanego konta, wgrane tutaj. Wartości nie wracają do panelu.
const COOKIES_MAX = 512 * 1024;
const plDate = (iso) => new Date(iso).toLocaleString('pl-PL', { dateStyle: 'medium', timeStyle: 'short' });

function cookiesState(st) {
  if (!st.store) return h('div', { class: 'note warn' }, 'CG Playout nie ma etv_config – ciasteczek nie ma gdzie zapisać.');
  if (!st.present) return h('div', { class: 'note', 'data-cookies-state': 'none' }, 'Brak ciasteczek – wejścia YouTube idą bez logowania (na serwerze YouTube zwykle je odrzuca).');
  if (st.error) return h('div', { class: 'note err', 'data-cookies-state': 'bad' }, `Zapisany plik jest zły: ${st.error}`);
  const days = st.expires ? (Date.parse(st.expires) - Date.now()) / 86400000 : null;
  const cls = !st.signed_in || (days != null && days < 0) ? 'err' : days != null && days < 7 ? 'warn' : 'ok';
  return h('div', { class: `note ${cls}`, 'data-cookies-state': cls },
    `Wgrane ${st.saved_at ? plDate(st.saved_at) : ''}: ${st.youtube} z youtube.com (razem ${st.cookies}). `,
    st.signed_in ? 'Konto zalogowane' : 'Brak ciasteczek zalogowanego konta (SID, LOGIN_INFO) – YouTube może dalej odmawiać',
    st.signed_in ? (days == null ? ', ciasteczka sesji (bez daty ważności).' : days < 0 ? `, ważność minęła ${plDate(st.expires)} – wgraj nowe.` : `, ważne do ${plDate(st.expires)}.`) : '.');
}

async function editCookies() {
  let st;
  try { st = await cg('live/youtube-cookies'); } catch (e) { toast(e.message, 'bad'); return; }
  const text = h('textarea', { rows: 6, class: 'mono', spellcheck: 'false', autocomplete: 'off', placeholder: '# Netscape HTTP Cookie File\n.youtube.com\tTRUE\t/\tTRUE\t1893456000\tSID\t…', 'aria-label': 'Treść pliku cookies' });
  const file = h('input', { type: 'file', accept: '.txt,.json,text/plain,application/json', 'aria-label': 'Plik cookies' });
  const info = h('span', { class: 'dim' });
  file.addEventListener('change', async () => {
    const f = file.files[0];
    if (!f) return;
    if (f.size > COOKIES_MAX) { info.textContent = `Plik za duży (${Math.round(f.size / 1024)} KB, najwyżej 512 KB) – wyeksportuj tylko youtube.com.`; return; }
    text.value = await f.text();
    info.textContent = `Wczytano ${f.name}.`;
  });
  const body = h('div', { class: 'cookies-edit', 'data-cookies': '' },
    cookiesState(st),
    h('p', { class: 'dim' }, 'YouTube każe serwerom w serwerowniach się zalogować („Sign in to confirm you’re not a bot”) – wtedy wejście YouTube czeka bez końca. ' +
      'CG Playout poda pobieraniu ciasteczka zalogowanego konta Google. Jak je zdobyć:'),
    h('ol', { class: 'dim' },
      h('li', null, 'Najlepiej osobne konto Google (YouTube może je zablokować za pobieranie).'),
      h('li', null, 'W przeglądarce otwórz okno prywatne, zaloguj się na youtube.com i otwórz dowolny film.'),
      h('li', null, 'Rozszerzeniem do eksportu ciasteczek (format cookies.txt / Netscape albo JSON) zapisz ciasteczka youtube.com.'),
      h('li', null, 'Zamknij okno prywatne bez wylogowania – inaczej YouTube unieważni te ciasteczka.'),
      h('li', null, 'Każdy serwer dostaje własny eksport (osobne logowanie): te same ciasteczka użyte z dwóch serwerów YouTube unieważnia.'),
      h('li', null, 'Wgraj plik albo wklej jego treść niżej i zapisz.')),
    h('div', { class: 'bar' }, file, info),
    text,
    h('p', { class: 'dim' }, 'Zapis działa od następnego pytania YouTube (wejście w oczekiwaniu pyta od razu); ciasteczka odświeżane przez YouTube CG Playout zapisuje sam (data „Wgrane” to wtedy ostatnie odświeżenie). Ciasteczka to logowanie do konta: leżą na serwerze tylko dla CG Playout, panel pokazuje jedynie liczbę i ważność.'));
  const buttons = [['Anuluj', false]];
  if (st.present) buttons.push(['Usuń ciasteczka', 'delete', 'danger']);
  buttons.push(['Zapisz', 'save', 'acc']);
  const choice = await dialog('Ciasteczka YouTube', body, buttons);
  if (choice === 'delete') {
    if (!await confirmDlg('Usunąć ciasteczka YouTube?', 'Wejścia YouTube pójdą bez logowania – na serwerze YouTube zwykle je odrzuca.', 'Usuń')) return;
    try { await cg('live/youtube-cookies', 'DELETE'); toast('Ciasteczka YouTube usunięte'); } catch (e) { toast(e.message, 'bad'); }
    return;
  }
  if (choice !== 'save') return;
  if (!text.value.trim()) { toast('Wgraj plik cookies albo wklej jego treść', 'bad'); return; }
  try {
    const r = await cg('live/youtube-cookies', 'PUT', { text: text.value });
    toast(r.signed_in ? `Ciasteczka YouTube zapisane (${r.youtube} z youtube.com, konto zalogowane)` : 'Ciasteczka zapisane, ale bez zalogowanego konta – YouTube może dalej odmawiać', r.signed_in ? undefined : 'bad');
  } catch (e) { toast(e.message, 'bad'); }
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
    inputPicker(c),
    h('div', { class: 'bar ctl' }, main,
      h('label', { class: 'chk' }, anytime, 'studio także poza audycjami'),
      btn('⟲ Przebuduj ramówkę', () => act(c, 'rebuild', `Kanał ${c.number}: ramówka przebudowana – EPG odświeży się za chwilę`))),
    how);
}

// ---------- Sloty na żywo: wybór wejścia kanału ----------
// CG Playout z `input_select`: kanał bierze wybrane wejście, pozostałe czekają (YouTube i linki wtedy nie pobierają);
// „automatycznie” = pierwsze z sygnałem. Wybór zapisuje CG Playout (PUT /api/live/channels/<n> {"input"}).
const INPUT_STATE = { on: '● nadaje', waiting: '● sygnał czeka', off: '○ nie pobiera' };
const inputName = (i) => i.label || ({ rtmp: `RTMP ${i.port}`, srt: `SRT ${i.port}`, youtube: 'YouTube', link: 'Link' })[i.kind] || i.kind;

function inputPicker(c) {
  const list = c.inputs || [];
  if (!(view?.state.features || []).includes('input_select') || list.length < 2) return null;
  const sel = h('select', { 'aria-label': `Wejście kanału ${c.number}`, class: 'grow', 'data-input-select': c.number },
    h('option', { value: '', selected: !c.input }, 'automatycznie – pierwsze z sygnałem'),
    list.map((i) => h('option', { value: i.id, selected: c.input === i.id }, inputName(i) + (INPUT_STATE[i.state] ? `  ${INPUT_STATE[i.state]}` : ''))));
  sel.addEventListener('change', async () => {
    sel.disabled = true;
    try {
      await cg(`live/channels/${encodeURIComponent(c.number)}`, 'PUT', { input: sel.value || null });
      toast(sel.value ? `Kanał ${c.number}: wejście ${sel.selectedOptions[0].textContent.split('  ')[0]}` : `Kanał ${c.number}: wejście automatycznie`);
    } catch (e) { toast(e.message, 'bad'); }
    route(true);
  });
  return h('div', { class: 'bar in-pick' }, h('span', { class: 'dim' }, 'Wejście:'), sel);
}

// ---------- Sloty na żywo: tabela wejść (jak w makiecie) ----------
/** Adres wejścia bez sekretów (klucz RTMP i hasło SRT są tylko w „Jak nadawać na ten kanał”). */
function inputAddr(i) {
  if (i.kind === 'youtube') return [i.url || '', i.youtube === 'channel' ? ' (strona live kanału)' : ' (jeden film)'];
  if (i.kind === 'link') return [linkShown(i.url || ''), ' (pobiera CG Playout)'];
  const host = i.host || location.hostname;
  if (i.kind === 'rtmp') return [`rtmp://${host}:${i.port}${i.app || ''}`, i.key ? ' + klucz' : ''];
  return [`srt://${host}:${i.port}` + (i.streamid ? `?streamid=${encodeURIComponent(i.streamid)}` : ''), i.passphrase ? ' + hasło' : ''];
}
function inputsTable(list) {
  const rows = list.flatMap((c) => (c.inputs || []).map((i, n) => ({ c, i, n })));
  return table(rows, [
    [({ c }) => h('span', null, h('span', { class: 'num' }, c.number), ' ', c.name || ''), 'Kanał'],
    [({ i }) => ({ rtmp: 'RTMP', srt: 'SRT', youtube: 'YouTube', link: 'Link' })[i.kind] || i.kind, 'Wejście'],
    [({ i }) => { const [a, extra] = inputAddr(i); return h('span', null, h('span', { class: 'mono' }, a), h('span', { class: 'dim' }, extra)); }, 'Adres'],
    [({ c, i }) => {
      const chosen = c.input && c.input === i.id ? [' ', pill('wybrane', 'info')] : null;
      // CG Playout z `input_select` podaje stan każdego wejścia; starszy – tylko kanału
      if ('state' in i) {
        const p = i.state === 'on' ? (c.hold ? pill('wstrzymany', 'warn') : pill(c.on_air ? 'nadaje · na antenie' : 'nadaje', c.on_air ? 'on' : 'ok'))
          : i.state === 'waiting' ? pill('sygnał czeka', 'warn') : i.state === 'off' ? pill('nie wybrane') : pill('czeka');
        return h('span', null, p, chosen);
      }
      return c.hold ? pill('wstrzymany', 'warn') : c.source?.live ? pill(c.on_air ? 'nadaje · na antenie' : 'nadaje', c.on_air ? 'on' : 'ok') : pill('czeka');
    }, 'Stan'],
    [({ c, i }) => (c.source?.live && (!('state' in i) || i.state === 'on') ? `${c.source.video || '?'} · ${c.source.audio ? 'z dźwiękiem' : 'bez dźwięku'}` : h('span', { class: 'dim' }, 'brak sygnału')), 'Obraz'],
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
      (state.features || []).includes('youtube_cookies') ? btn('Ciasteczka YouTube…', () => editCookies()) : null,
      btn('Ustaw wejścia…', () => editInputs())), view.inputs, cmdNote('CG Playout /api/live/state (inputs, source), /api/live/inputs')),
  ];
};

// ---------- Audycje LIVE ----------
const STEP = {
  file: 'Audycja zapisana (plik Remote Stream)', scan: 'Biblioteka „Na żywo” – skan', collection: 'Kolekcja w ErsatzTV', delete: 'Usunięcie',
  schedule: 'Godzina emisji w harmonogramie', rebuild: 'Ramówka kanału przebudowana',
};

async function rebuild(channel) {
  try { await cg(`live/channels/${encodeURIComponent(channel)}/rebuild`, 'POST'); toast(`Kanał ${channel}: ramówka przebudowana – EPG odświeży się za chwilę`); } catch (e) { toast(e.message, 'bad'); }
}

// ---------- godziny emisji: pozycje harmonogramu kanału ----------
// Audycja trafia do ramówki jako pozycja harmonogramu (playout klasyczny kanału): kolekcja „LIVE · tytuł”, stała godzina,
// ściśle, jedna pozycja. Panel wstawia ją sam (GetProgramScheduleItems → ReplaceProgramScheduleItems, jak ekran Harmonogramy)
// i przebudowuje ramówkę przez CG Playout – operator nie składa pozycji ręcznie.
const AIR_CMDS = ['GetPagedPlayouts', 'GetAllProgramSchedules', 'GetPlayoutAlternateSchedules', 'GetProgramScheduleItems', 'ReplaceProgramScheduleItems'];
const canAir = () => AIR_CMDS.every(has);

/** „20:00” / „7:5” → minuty od północy; null = zły zapis. */
function parseHm(t) {
  const m = /^\s*(\d{1,2})[:.](\d{2})\s*$/.exec(t || '');
  return m && +m[1] < 24 && +m[2] < 60 ? +m[1] * 60 + +m[2] : null;
}

const DAYS = { Monday: 'pn', Tuesday: 'wt', Wednesday: 'śr', Thursday: 'czw', Friday: 'pt', Saturday: 'sob', Sunday: 'nd' };
/** Kiedy kanał gra harmonogram alternatywny: „sob, nd”, „wybrane dni miesiąca”… (z GetPlayoutAlternateSchedules). */
function whenText(a) {
  const d = (a.DaysOfWeek || []).filter((x) => DAYS[x]);
  const parts = [d.length && d.length < 7 ? d.map((x) => DAYS[x]).join(', ') : null,
    (a.DaysOfMonth || []).length && a.DaysOfMonth.length < 31 ? 'wybrane dni miesiąca' : null,
    (a.MonthsOfYear || []).length && a.MonthsOfYear.length < 12 ? 'wybrane miesiące' : null].filter(Boolean);
  return parts.join(' · ') || 'codziennie';
}

/**
 * Harmonogramy playoutu kanału (główny pierwszy, z opisem dni) albo { error } – ramówka nieklasyczna albo brak playoutu.
 * Harmonogram główny gra w dni, których nie zajmują alternatywne.
 */
async function channelSchedules(channel) {
  const [res, all] = await Promise.all([cmd('GetPagedPlayouts', { Query: '', PageNum: 0, PageSize: 500 }), cmd('GetAllProgramSchedules')]);
  const p = (res?.Page || []).find((x) => x.ChannelNumber === channel);
  if (!p) return { error: `Kanał ${channel} nie ma playoutu – załóż go w Playoutach (rodzaj: harmonogram).` };
  if (p.ScheduleKind !== 'Classic') {
    return { error: `Kanał ${channel} ma ramówkę „${pl(p.ScheduleKind)}”, nie harmonogram – emisje tej audycji wstawisz tam ręcznie (kolekcja inteligentna „LIVE · …”).` };
  }
  const byId = new Map((all || []).map((s) => [s.Id, s]));
  // najwyższy Index = harmonogram główny (jak w Playoutach), reszta to harmonogramy alternatywne (np. weekend)
  const alts = [...((await cmd('GetPlayoutAlternateSchedules', { PlayoutId: p.PlayoutId })) || [])].sort((a, b) => b.Index - a.Index);
  const when = new Map();
  alts.forEach((a, i) => { if (i > 0) when.set(a.ProgramScheduleId, [...(when.get(a.ProgramScheduleId) || []), whenText(a)]); });
  let ids = [...new Set(alts.map((a) => a.ProgramScheduleId))];
  if (!ids.length) ids = (all || []).filter((s) => s.Name === p.ScheduleName).map((s) => s.Id);
  if (!ids.length) return { error: `Nie znalazłem harmonogramu playoutu kanału ${channel}.` };
  return {
    playout: p,
    schedules: ids.map((id, i) => ({
      id, name: byId.get(id)?.Name ?? `ID ${id}`, shuffle: !!byId.get(id)?.ShuffleScheduleItems, main: i === 0,
      when: i === 0 ? (alts.length > 1 ? 'pozostałe dni' : 'codziennie') : (when.get(id) || ['codziennie']).join('; '),
    })),
  };
}

const isLive = (it, coll) => it.CollectionType === 'SmartCollection' && it.SmartCollectionId === coll;
const CONTENT = ['CollectionType', 'CollectionId', 'MultiCollectionId', 'SmartCollectionId', 'MediaItemId', 'PlaylistId', 'SearchQuery'];
const sameContent = (a, b) => CONTENT.every((k) => (a[k] ?? null) === (b[k] ?? null));
const isFixed = (it) => it.StartType === 'Fixed' && it.StartTime;

/**
 * Wstawia pozycję audycji o `min` do pozycji harmonogramu (tablica z fromLegacy) – w bloku stałej godziny, w który wpada
 * (za jego pozycjami „po poprzedniej”, przed następną stałą godziną). Gdy po audycji byłaby od razu następna stała godzina,
 * a przed nią treść „wypełnij”, za audycją wraca ta sama treść (kopia „po poprzedniej”) – inaczej do tej godziny byłaby dziura.
 * Zwraca { items, note } albo rzuca Error.
 */
function insertAiring(items, coll, min, name, extra = {}) {
  const fixed = items.map((it, i) => ({ it, i })).filter(({ it }) => isFixed(it));
  if (fixed.some(({ it }) => minutesOf(it.StartTime) === min)) throw new Error(`o ${hhmm(min)} harmonogram ma już pozycję o stałej godzinie`);
  let at = items.length;
  if (fixed.length) {
    // blok, w który wpada godzina: ostatnia stała godzina ≤ min (z zawinięciem doby); wstawiamy przed następną stałą pozycją z listy
    const before = fixed.filter(({ it }) => minutesOf(it.StartTime) < min);
    const anchor = (before.length ? before : fixed).reduce((a, b) => (minutesOf(b.it.StartTime) > minutesOf(a.it.StartTime) ? b : a));
    const nextFixed = fixed.find(({ i }) => i > anchor.i) || fixed[0];
    // brak stałej pozycji za kotwicą na liście: blok kotwicy biegnie do końca listy (i dalej od jej początku) – audycja na koniec
    at = nextFixed.i > anchor.i ? nextFixed.i : items.length;
  }
  // `extra`: pola tej emisji (zachowanie godziny, tytuł w EPG, znaki wodne) i reszta dawnej pozycji przy zmianie godziny
  const live = {
    ...fromLegacy({ StartType: 'Fixed', FixedStartTimeBehavior: 'Strict', PlayoutMode: 'One', PlaybackOrder: 'Chronological', CollectionType: 'SmartCollection' }),
    ...extra, StartType: 'Fixed', StartTime: span(min), CollectionType: 'SmartCollection', SmartCollectionId: coll, _name: name,
  };
  const out = [...items.slice(0, at), live, ...items.slice(at)];
  const prev = items.length ? items[(at - 1 + items.length) % items.length] : null;
  const next = items.length ? items[at % items.length] : null;
  let note = null;
  if (next && isFixed(next) && prev && !isLive(prev, coll)) {
    if (prev.PlayoutMode === 'Flood') out.splice(at + 1, 0, { ...prev, StartType: 'Dynamic', StartTime: null, FixedStartTimeBehavior: null });
    else note = `po audycji do ${(next.StartTime || '').slice(0, 5)} legacy pokaże wypełniacz zapasowy albo planszę offline`;
  }
  return { items: out, note };
}

/** Usuwa pozycję audycji (indeks) razem z kopią treści, którą wstawił insertAiring. */
function removeAiring(items, i) {
  const out = [...items];
  out.splice(i, 1);
  const prev = out[i - 1], cont = out[i];
  if (prev && cont && cont.StartType === 'Dynamic' && cont.PlayoutMode === 'Flood' && sameContent(prev, cont)) out.splice(i, 1);
  return out;
}

/** Zapis pozycji harmonogramu – jak ekran Harmonogramy (`warning` mostu = zapisane, DOKUMENTACJA §13). */
const saveItems = (id, items) => cmd('ReplaceProgramScheduleItems', { ProgramScheduleId: id, Items: items.map((it, i) => toReplace(it, i + 1)) });
const loadItems = async (id) => ((await cmd('GetProgramScheduleItems', { Id: id })) || []).sort((a, b) => a.Index - b.Index).map(fromLegacy);
const airingsOf = (items, coll) => items.map((it, i) => ({ it, i })).filter(({ it }) => isLive(it, coll) && isFixed(it));

async function rebuildStep(channel) {
  try { await cg(`live/channels/${encodeURIComponent(channel)}/rebuild`, 'POST'); return { step: 'rebuild', ok: true, detail: 'EPG odświeży się za chwilę' }; } catch (e) { return { step: 'rebuild', ok: false, detail: e.message }; }
}

// ---------- okno audycji: EPG + emisje ----------
const BEHAVIOR_OPTS = [['Strict', 'ściśle'], ['Flexible', 'elastycznie']];
const RATINGS = ['bez ograniczeń', '7', '12', '16', '18'];

/** Emisje audycji z harmonogramów kanału: wiersze edytora (godzina, harmonogram, pola pozycji). */
function airingRows(ch, loaded, coll) {
  if (coll == null) return [];
  return ch.schedules.flatMap((sc) => airingsOf(loaded.get(sc.id) || [], coll).map(({ it }) => ({
    schedule: sc.id, time: it.StartTime.slice(0, 5), behavior: it.FixedStartTimeBehavior || 'Strict', title: it.CustomTitle || '',
    watermark: (it.WatermarkIds || [])[0] ?? '', orig: it,
  }))).sort((a, b) => a.time.localeCompare(b.time));
}

/**
 * Zapis emisji: w każdym harmonogramie kanału zdejmuje dotychczasowe pozycje audycji i wstawia te z edytora (z polami
 * dawnej pozycji, np. wypełniaczami i elementami graficznymi z Harmonogramów); harmonogram bez zmian nie jest zapisywany.
 * Zwraca kroki dla kreatora; rzuca Error przed pierwszym zapisem, gdy emisja jest błędna.
 */
async function saveAirings(show, ch, loaded, rows) {
  const plan = [];
  for (const sc of ch.schedules) {
    const want = rows.filter((r) => r.schedule === sc.id).map((r) => ({ ...r, min: parseHm(r.time) }));
    const bad = want.find((r) => r.min == null);
    if (bad) throw new Error(`emisja „${bad.time || '(pusta)'}”: godzina w zapisie gg:mm, np. 20:00`);
    if (want.length && sc.shuffle) throw new Error(`harmonogram „${sc.name}” tasuje pozycje – nie może mieć stałej godziny`);
    const before = loaded.get(sc.id) || [];
    let items = before;
    for (const { i } of airingsOf(items, show.collection_id).reverse()) items = removeAiring(items, i);
    const notes = [];
    for (const r of want.sort((x, y) => x.min - y.min)) {
      const wm = r.watermark === '' ? [] : r.orig && String((r.orig.WatermarkIds || [])[0] ?? '') === String(r.watermark) ? r.orig.WatermarkIds : [Number(r.watermark)];
      try {
        const out = insertAiring(items, show.collection_id, r.min, show.collection_name,
          { ...(r.orig || {}), FixedStartTimeBehavior: r.behavior, CustomTitle: r.title.trim(), WatermarkIds: wm });
        items = out.items;
        if (out.note) notes.push(`${hhmm(r.min)}: ${out.note}`);
      } catch (e) { throw new Error(`„${sc.name}”: ${e.message}`); }
    }
    const key = (list) => JSON.stringify(list.map((it, i) => toReplace(it, i + 1)));
    if (key(items) === key(before)) continue;
    plan.push({ sc, items, detail: `„${sc.name}” (${sc.when}): ${want.length ? want.map((r) => hhmm(r.min)).join(', ') : 'bez emisji'}${notes.length ? ' – ' + notes.join('; ') : ''}` });
  }
  const steps = [];
  for (const p of plan) {
    try { await saveItems(p.sc.id, p.items); steps.push({ step: 'schedule', ok: true, detail: p.detail }); } catch (e) { steps.push({ step: 'schedule', ok: false, detail: `„${p.sc.name}”: ${e.message}` }); }
  }
  return steps;
}

/** Wynik zapisu: kroki CG Playout i panelu (✓/!); bez komend harmonogramu w moście – instrukcja ręczna. */
async function doneDialog(title, data, show) {
  const steps = (data?.steps || []).map((s) => h('li', { class: s.ok ? 'ok' : 'bad' }, h('span', { class: 'ic' }, s.ok ? '✓' : '!'), STEP[s.step] || s.step, s.detail ? h('small', { class: 'dim' }, ' ' + s.detail) : null));
  const manual = show && !canAir();
  const body = h('div', { class: 'wizard', 'data-wizard': '' },
    steps.length ? h('ol', { class: 'steps' }, steps) : null,
    manual ? h('div', { class: 'note' }, h('b', null, 'Godzina emisji: '),
      `most legacy nie ma komend harmonogramu – w Harmonogramach dodaj pozycję: stała godzina, ściśle, kolekcja inteligentna „${show.collection_name || 'LIVE · ' + show.title}”, jedna pozycja; potem przebuduj ramówkę.`) : null);
  if ((await dialog(title, body, manual ? [['Gotowe', null], ['Otwórz Harmonogramy', 'open', 'acc']] : [['Gotowe', null, 'acc']])) === 'open') location.hash = '#harmonogramy';
}

const findShow = async (id) => ((await cg('live/shows').catch(() => null))?.shows || []).find((x) => x.id === id) || null;

/**
 * Okno audycji: dane w EPG (tytuł, długość, kategoria wiekowa, opis – plik audycji w CG Playout) i emisje w ramówce kanału
 * (godzina, harmonogram = dni, ściśle / elastycznie, własny tytuł emisji w EPG, znak wodny – pozycje harmonogramu w legacy).
 * Jeden zapis: audycja w CG Playout, emisje w harmonogramach, przebudowa ramówki.
 */
async function showDialog(channels, show) {
  const f = {
    channel: h('select', { id: 'ls-channel' }, channels.map((c) => h('option', { value: c.number, selected: show?.channel === c.number }, `${c.number} · ${c.name || ''}`))),
    title: h('input', { id: 'ls-title', value: show?.title || '', maxlength: 200 }),
    duration: h('input', { id: 'ls-duration', value: show?.duration || '01:00:00', placeholder: 'gg:mm:ss', class: 'mono' }),
    rating: h('input', { id: 'ls-rating', value: show?.content_rating || '', maxlength: 20, list: 'ls-ratings', placeholder: 'brak' }),
    plot: h('textarea', { id: 'ls-plot', rows: 3, maxlength: 2000 }),
  };
  f.plot.value = show?.plot || '';
  if (show) f.channel.disabled = true;
  const ratingOk = !show || 'content_rating' in show;  // starszy CG Playout nie zna pola – wtedy go nie pokazujemy

  // emisje: harmonogramy kanału i ich pozycje (ładowane przy otwarciu i zmianie kanału)
  const air = canAir();
  let ch = null, loaded = new Map(), rows = [], watermarks = [];
  const airBox = h('div', { class: 'ls-airings', 'data-airings': '' });
  const endOf = (t) => { const m = parseHm(t), d = /^(\d+):(\d{2}):(\d{2})$/.exec(f.duration.value.trim()); return m == null || !d ? '' : '→ ' + hhmm(m + +d[1] * 60 + +d[2] + Math.round(+d[3] / 60)); };
  const drawAirings = () => {
    if (!air) { airBox.replaceChildren(h('p', { class: 'dim' }, 'Most legacy nie ma komend harmonogramu – emisje ustawisz w Harmonogramach.')); return; }
    if (!ch) { airBox.replaceChildren(h('p', { class: 'dim' }, 'Wczytuję harmonogramy kanału…')); return; }
    if (ch.error) { airBox.replaceChildren(h('p', { class: 'note' }, ch.error)); return; }
    const sel = (opts, value, label, on) => {
      const el = h('select', { 'aria-label': label }, opts.map(([v, t]) => h('option', { value: v, selected: String(v) === String(value) }, t)));
      el.onchange = () => on(el.value);
      return el;
    };
    const table_ = rows.length ? h('table', { class: 'ls-air-table' },
      h('thead', null, h('tr', null, ['Godzina', '', 'Dni · harmonogram', 'Start', 'Tytuł tej emisji w EPG', 'Znak wodny', ''].map((t) => h('th', null, t)))),
      h('tbody', null, rows.map((r, n) => {
        const time = h('input', { value: r.time, placeholder: 'gg:mm', class: 'mono', maxlength: 5, 'aria-label': `Godzina emisji ${n + 1}` });
        const end = h('span', { class: 'dim mono' }, endOf(r.time));
        time.oninput = () => { r.time = time.value; end.textContent = endOf(r.time); };
        const title = h('input', { value: r.title, maxlength: 200, placeholder: f.title.value.trim() || 'jak audycja', 'aria-label': `Tytuł emisji ${n + 1}` });
        title.oninput = () => { r.title = title.value; };
        return h('tr', null,
          h('td', null, time), h('td', null, end),
          h('td', null, sel(ch.schedules.map((sc) => [sc.id, `${sc.when} · ${sc.name}`]), r.schedule, `Harmonogram emisji ${n + 1}`, (v) => { r.schedule = Number(v); })),
          h('td', null, sel(BEHAVIOR_OPTS, r.behavior, `Start emisji ${n + 1}`, (v) => { r.behavior = v; })),
          h('td', null, title),
          h('td', null, sel([['', '— brak —'], ...watermarks.map((w) => [w.Id, w.Name])], r.watermark, `Znak wodny emisji ${n + 1}`, (v) => { r.watermark = v; })),
          h('td', null, btn('✕', () => { rows.splice(n, 1); drawAirings(); }, 'danger')));
      })))
      : h('p', { class: 'dim' }, 'Brak emisji – audycja nie jest w ramówce.');
    airBox.replaceChildren(table_,
      h('div', { class: 'bar' }, btn('+ Emisja', () => { rows.push({ schedule: ch.schedules[0].id, time: '', behavior: 'Strict', title: '', watermark: '', orig: null }); drawAirings(); airBox.querySelector('tbody tr:last-child input')?.focus(); }),
        h('span', { class: 'dim' }, (ch.schedules.length > 1 ? 'Dni wybiera harmonogram (alternatywne i ich dni ustawiasz w Playoutach). ' : 'Emisja codziennie – inne dni: harmonogram alternatywny w Playoutach. ') + 'Start ściśle czeka na godzinę, elastycznie wchodzi zaraz po poprzedniej pozycji.')));
  };
  const loadChannel = async (channel) => {
    ch = null; loaded = new Map(); rows = []; drawAirings();
    try {
      const c = await channelSchedules(channel);
      if (!c.error) {
        await Promise.all(c.schedules.map(async (sc) => loaded.set(sc.id, await loadItems(sc.id))));
        if (!watermarks.length && has('GetAllWatermarks')) watermarks = (await cmd('GetAllWatermarks').catch(() => [])) || [];
      }
      if (channel !== f.channel.value) return;  // operator zmienił kanał w trakcie
      ch = c;
      rows = airingRows(ch, loaded, show?.collection_id);
    } catch (e) { ch = { error: e.message }; }
    drawAirings();
  };
  if (air) { f.channel.onchange = () => loadChannel(f.channel.value); loadChannel(f.channel.value); } else drawAirings();
  f.duration.oninput = () => airBox.querySelectorAll('tbody tr').forEach((tr, n) => { tr.children[1].firstChild.textContent = endOf(rows[n]?.time); });

  const body = h('div', { class: 'form live-show-ed' },
    h('div', { class: 'sec' }, 'W EPG'),
    h('label', { for: 'ls-channel' }, 'Kanał'), f.channel,
    h('label', { for: 'ls-title' }, 'Tytuł'), f.title,
    h('span'), h('span', { class: 'dim' }, 'widoczny w EPG i jako {title} w grafice VizRS; emisja może mieć własny tytuł'),
    h('label', { for: 'ls-duration' }, 'Długość'), f.duration,
    h('span'), h('span', { class: 'dim' }, 'ile trwa każda emisja – po niej ramówka wraca do swojej treści'),
    ratingOk ? [h('label', { for: 'ls-rating' }, 'Kategoria wiekowa'), f.rating, h('datalist', { id: 'ls-ratings' }, RATINGS.map((r) => h('option', { value: r })))] : null,
    h('label', { for: 'ls-plot' }, 'Opis'), f.plot,
    h('span'), h('span', { class: 'dim' }, 'w czasie audycji bez sygnału studia kanał pokazuje czerń z ciszą'),
    h('div', { class: 'sec' }, 'Emisje w ramówce'), airBox);
  if (!(await dialog(show ? `Audycja LIVE: „${show.title}”` : 'Nowa audycja LIVE', body, [['Anuluj', false], [show ? 'Zapisz' : 'Utwórz', true, 'acc']]))) return;

  const data = { channel: f.channel.value, title: f.title.value.trim(), duration: f.duration.value.trim(), plot: f.plot.value };
  if (ratingOk) data.content_rating = f.rating.value.trim();
  const editAir = air && ch && !ch.error;
  if (editAir) {
    const bad = rows.find((r) => parseHm(r.time) == null);
    if (bad) { toast(`Emisja „${bad.time || '(pusta)'}”: godzina w zapisie gg:mm, np. 20:00`, 'bad'); return; }
  }
  const changed = !show || ['title', 'duration', 'plot', 'content_rating'].some((k) => k in data && String(data[k]) !== String(show[k] ?? ''));
  let r = { steps: [] };
  try {
    if (changed) r = show ? await cg('live/shows/' + encodeURIComponent(show.id), 'PUT', data) : await cg('live/shows', 'POST', data);
  } catch (e) { toast(e.message, 'bad'); route(true); return; }
  const saved = show && !changed ? show : (await findShow(r?.show?.id || show?.id)) || { ...(show || {}), ...(r?.show || {}) };
  const steps = [...(r?.steps || [])];
  if (editAir && (rows.length || airingRows(ch, loaded, saved.collection_id).length)) {
    if (saved.collection_id == null) steps.push({ step: 'schedule', ok: false, detail: 'kolekcja audycji jeszcze nie istnieje w ErsatzTV – emisje zapiszesz po „⟳ Odśwież z ErsatzTV”' });
    else {
      try { steps.push(...(await saveAirings(saved, ch, loaded, rows))); } catch (e) { steps.push({ step: 'schedule', ok: false, detail: e.message }); }
    }
  }
  if ((changed && show) || steps.some((x) => x.step === 'schedule' && x.ok)) steps.push(await rebuildStep(saved.channel || data.channel));
  await route(true);
  if (!steps.length) { toast('Bez zmian'); return; }
  await doneDialog(`„${saved.title || data.title}” ${show ? 'zapisana' : 'jest'} w ErsatzTV`, { steps }, show ? null : saved);
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

/** Godziny emisji audycji z harmonogramów kanałów: Map(collection_id → „20:00 · sob, nd 18:00”); null = nie da się odczytać. */
async function airTimes(list) {
  if (!canAir()) return null;
  const out = new Map();
  try {
    for (const channel of new Set(list.filter((s) => s.collection_id != null).map((s) => s.channel))) {
      const ch = await channelSchedules(channel);
      if (ch.error) continue;
      for (const sc of ch.schedules) {
        for (const { it } of (await loadItems(sc.id)).map((it2, i) => ({ it: it2, i })).filter(({ it: x }) => x.CollectionType === 'SmartCollection' && isFixed(x))) {
          const t = `${it.StartTime.slice(0, 5)}${sc.when === 'codziennie' ? '' : ' (' + sc.when + ')'}`;
          out.set(it.SmartCollectionId, [...(out.get(it.SmartCollectionId) || []), t]);
        }
      }
    }
  } catch { return null; }
  return out;
}

function airCell(s, shows, times) {
  const now = cgNow();
  let when;
  if (s.next) {
    const more = s.airings > 1 ? ` · +${s.airings - 1}` : '';
    when = s.next.start <= now ? pill(`● teraz · do ${at(s.next.stop)}${more}`, 'on') : `${day(s.next.start)} ${at(s.next.start)}${more}`;
  }
  const t = times?.get(s.collection_id);
  if (!s.next) when = shows.guide_known === false ? h('span', { class: 'dim' }, '–') : t ? pill('brak w EPG – przebuduj ramówkę', 'warn') : pill('nie ma w ramówce', 'warn');
  return h('span', { class: 'air-cell' }, when, t ? h('small', { class: 'dim', 'data-air-times': '' }, 'emisje: ' + [...t].sort().join(', ')) : null);
}

function statusCell(s, shows) {
  const lib = shows.settings?.library != null;
  const coll = s.collection === true ? 'ok' : s.collection == null ? '' : 'bad';
  return h('span', { class: 'bar' }, pill('plik', s.file_ok === false ? 'bad' : 'ok'), pill('biblioteka', lib ? 'ok' : 'bad'), pill('kolekcja', coll));
}

VIEWS.audycje = async () => {
  const title = 'Audycje LIVE';
  const desc = 'Audycje na żywo kanałów: CG Playout zakłada w legacy plik Remote Stream i kolekcję inteligentną „LIVE · tytuł”; godziny emisji panel wstawia do harmonogramu kanału.';
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
  const times = await airTimes(shows.shows || []);
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
          [(s) => h('span', { class: 'air-cell', title: s.plot || '' }, s.title, h('small', { class: 'dim mono' }, s.collection === true ? s.collection_name : 'kolekcja: ⟳ Odśwież'),
            s.content_rating ? h('small', { class: 'dim' }, 'kategoria ' + s.content_rating) : null), 'Tytuł (EPG)'],
          ['duration', 'Długość', { class: 'num' }],
          [(s) => statusCell(s, shows), 'W ErsatzTV'],
          [(s) => airCell(s, shows, times), 'Emisja w ramówce'],
          [(s) => h('span', { class: 'bar' }, btn('Edytuj…', () => showDialog(channels, s), s.next || times?.has(s.collection_id) ? null : 'acc'), btn('🗑', () => del(s), 'danger')), ''],
        ], { empty: 'Brak audycji LIVE. Dodaj pierwszą przyciskiem „+ Nowa audycja LIVE”.' }),
        h('p', { class: 'dim live-side' }, 'Wszystko o audycji w jednym oknie („Edytuj…”): tytuł, długość, kategoria wiekowa i opis w EPG oraz emisje – godzina, dni (harmonogram), start ściśle / elastycznie, własny tytuł emisji i znak wodny. Panel zapisuje emisje w harmonogramie kanału i przebudowuje ramówkę.'),
        cmdNote('CG Playout /api/live/shows', ...AIR_CMDS)),
      h('div', { class: 'cols' },
        await libraryPanel(shows),
        panel('Przebuduj ramówkę', null, h('div', { class: 'live-side' },
          h('p', { class: 'dim' }, 'Po zmianie harmonogramu przebuduj playout kanału, żeby audycje trafiły do EPG.'),
          numbers.length ? h('div', { class: 'bar' }, numbers.map((n) => btn(`⟲ kanał ${n}`, async () => { await rebuild(n); route(true); }))) : h('div', { class: 'empty' }, 'Brak kanałów.'))))),
  ];
};
