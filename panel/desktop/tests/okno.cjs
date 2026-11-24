// Test aplikacji VizRS Live w działaniu (otwiera okna): wzór – tests/e2e/shell.js VizRS CG System.
//   node desktop/tests/okno.cjs [vizrs-live] [vizrs-live-desktop]
// Domyślnie server/target/release/vizrs-live i desktop/target/release/vizrs-live-desktop; atrapa mostu: tests/mock-bridge.cjs.
// Kroki: start bez zapisanego panelu = okno „Połącz z panelem” (okno niczego nie uruchamia lokalnie), panel na serwerze
// (tu: vizrs-live z tokenem) przez pośrednika (strona nie zna tokenu), okno połączenia – zły token i dobry;
// K20: panel z kontami – logowanie kontem na stronie, sesja w pośredniku i na liście zapisanych (strona jej nie zna).
'use strict';
const { spawn, spawnSync } = require('child_process');
const fs = require('fs');
const os = require('os');
const path = require('path');
const net = require('net');

const ROOT = path.join(__dirname, '..', '..');
const LIVE = path.resolve(process.argv[2] || path.join(ROOT, 'server/target/release/vizrs-live'));
const APP = path.resolve(process.argv[3] || path.join(ROOT, 'desktop/target/release/vizrs-live-desktop'));
const BTOKEN = 'okno-most-0123456789';
const PTOKEN = 'okno-panel-0123456789';
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'vizrs-live-okno-'));
const ENV = { ...process.env, VIZRS_CONFIG: path.join(tmp, 'cfg'), XDG_STATE_HOME: path.join(tmp, 'state') };
for (const k of ['VIZRS_LIVE_CONNECT', 'VIZRS_LIVE_TOKEN', 'VIZRS_LIVE_BIN']) delete ENV[k];

let failed = 0;
const ok = (c, name, info) => { if (c) console.log('PASS ' + name); else { failed++; console.log('FAIL ' + name + ' → ' + JSON.stringify(info)); } };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const freePort = () => new Promise((r) => { const s = net.createServer().listen(0, '127.0.0.1', () => { const p = s.address().port; s.close(() => r(p)); }); });
const up = async (port) => { try { const r = await fetch(`http://127.0.0.1:${port}/health`); return r.ok; } catch { return false; } };

// skrypt sondy: w oknie połączenia wypełnia formularz (token z window.name… nie – z pliku sondy), w oknie panelu czeka
// na stan mostu i odsyła wynik; w oknie połączenia po błędzie odsyła komunikat
const probeFile = (connect, login) => {
  const f = path.join(tmp, `probe-${Math.random().toString(36).slice(2)}.js`);
  fs.writeFileSync(f, `(async () => {
  const post = (r) => window.__vizrsPost('probe:' + JSON.stringify(r));
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const C = ${JSON.stringify(connect || null)};
  const L = ${JSON.stringify(login || null)};
  if (document.getElementById('go') && document.getElementById('url')) {
    if (!C) return post({ where: 'connect', unexpected: true });
    document.getElementById('url').value = C.url;
    document.getElementById('token').value = C.token;
    document.getElementById('go').click();
    for (let i = 0; i < 60; i++) { await sleep(100); const e = document.getElementById('err').textContent; if (e) return post({ where: 'connect', err: e }); }
    return;
  }
  let pill = '';
  for (let i = 0; i < 80; i++) {
    pill = document.getElementById('st-bridge')?.textContent || ''; if (/OK/.test(pill)) break;
    // ekran logowania panelu (K20): z danymi konta – zaloguj (strona się przeładuje i sonda ruszy od nowa)
    if (document.getElementById('login-user')) {
      if (!L) return post({ where: 'panel', login: true });
      document.getElementById('login-user').value = L.user;
      document.getElementById('login-pass').value = L.password;
      document.querySelector('#dlg-buttons button.acc').click();
      return;
    }
    await sleep(100);
  }
  const auth = await fetch('/api/auth').then((r) => r.json()).catch(() => ({}));
  const st = await fetch('/api/status').then((r) => r.status).catch(() => 0);
  post({ where: 'panel', href: location.href, pill, status: st, shell: !!window.__vizrsShell, cookie: document.cookie,
    html: document.documentElement.outerHTML.includes(${JSON.stringify(PTOKEN)}), side: document.querySelectorAll('.side a').length,
    user: auth.result?.user?.name || null });
})();`);
  return f;
};
const run = (args, env = ENV, timeout = 60000) => {
  const out = path.join(tmp, `out-${Math.random().toString(36).slice(2)}.json`);
  const r = spawnSync(APP, ['--probe-out', out, ...args], { encoding: 'utf8', timeout, env });
  let res = null;
  try { res = JSON.parse(fs.readFileSync(out, 'utf8')); } catch { /* brak wyniku */ }
  return { res, status: r.status, stderr: r.stderr };
};

(async () => {
  const kids = [];
  try {
    const BPORT = await freePort();
    const mock = spawn(process.execPath, [path.join(ROOT, 'tests/mock-bridge.cjs'), String(BPORT), BTOKEN], { stdio: 'ignore' });
    kids.push(mock);
    for (let i = 0; i < 50 && !(await fetch(`http://127.0.0.1:${BPORT}/vizrs-bridge/health`, { headers: { 'X-VizRS-Token': BTOKEN } }).then((r) => r.ok).catch(() => false)); i++) await sleep(100);

    // 0. opcje
    ok(spawnSync(APP, ['--version'], { encoding: 'utf8' }).stdout.startsWith('vizrs-live-desktop '), '--version');
    const bad = spawnSync(APP, ['--nieznana'], { encoding: 'utf8', timeout: 10000 });
    ok(bad.status === 2 && /nieznany argument/.test(bad.stderr), 'zły argument: kod 2 i komunikat (bez okna)', bad.status);

    // 1. start bez zapisanego panelu: okno połączenia, bez uruchamiania vizrs-live; dawny tryb lokalny = zły argument
    const before = new Set(spawnSync('pgrep', ['-x', 'vizrs-live'], { encoding: 'utf8' }).stdout.split('\n').filter(Boolean));
    const a = run(['--probe', probeFile()]);
    ok(a.res?.where === 'connect' && a.res.unexpected, 'start bez zapisanego panelu: okno „Połącz z panelem”', a.res || a.stderr);
    const after = spawnSync('pgrep', ['-x', 'vizrs-live'], { encoding: 'utf8' }).stdout.split('\n').filter((p) => p && !before.has(p));
    ok(after.length === 0, 'okno nie uruchamia vizrs-live', after);
    const loc = spawnSync(APP, ['local'], { encoding: 'utf8', timeout: 10000 });
    ok(loc.status === 2 && /nieznany argument/.test(loc.stderr), 'dawny tryb „local”: kod 2 (bez okna)', loc.status);

    // 2. panel na serwerze (tu: vizrs-live z tokenem) przez pośrednika – token zostaje w programie
    const PPORT = await freePort();
    const panel = spawn(LIVE, ['--bind', '127.0.0.1', '--port', String(PPORT), '--token', PTOKEN, '--bridge', `http://127.0.0.1:${BPORT}`, '--bridge-token', BTOKEN], { stdio: 'ignore' });
    kids.push(panel);
    for (let i = 0; i < 50 && !(await up(PPORT)); i++) await sleep(100);
    const noTok = await fetch(`http://127.0.0.1:${PPORT}/api/status`).then((r) => r.status);
    ok(noTok === 401, 'panel zdalny wymaga tokenu', noTok);
    const b = run(['--connect', `http://127.0.0.1:${PPORT}`, '--token', PTOKEN, '--probe', probeFile()]);
    ok(b.res?.where === 'panel' && b.res.status === 200 && /OK/.test(b.res.pill) && !b.res.href.includes(`:${PPORT}/`) && b.res.href.startsWith('http://127.0.0.1:'),
      'panel zdalny przez pośrednika: API z tokenem okna, adres pośrednika', b.res || b.stderr);
    ok(b.res && !b.res.html && !b.res.cookie.includes(PTOKEN), 'token panelu nie trafia do strony', b.res && { cookie: b.res.cookie, html: b.res.html });

    // 3. okno „Połącz z panelem”: zły token – komunikat; dobry – przełączenie, lista zapisanych bez tokenu
    const c1 = run(['connect', '--probe', probeFile({ url: `http://127.0.0.1:${PPORT}`, token: 'zly-token-0123456789' })]);
    ok(c1.res?.where === 'connect' && /odrzucił token/.test(c1.res.err), 'okno połączenia: zły token – komunikat', c1.res || c1.stderr);
    const c2 = run(['connect', '--probe', probeFile({ url: `http://127.0.0.1:${PPORT}`, token: PTOKEN })]);
    ok(c2.res?.where === 'panel' && c2.res.status === 200 && /OK/.test(c2.res.pill), 'okno połączenia: dobry token – panel w oknie', c2.res || c2.stderr);
    const saved = JSON.parse(fs.readFileSync(path.join(tmp, 'cfg/live-connections.json'), 'utf8'));
    ok(saved.servers?.[0]?.url === `http://127.0.0.1:${PPORT}` && saved.servers[0].token == null, 'panel na liście zapisanych, token tylko na życzenie', saved);
    if (process.platform !== 'win32') ok((fs.statSync(path.join(tmp, 'cfg/live-connections.json')).mode & 0o077) === 0, 'lista zapisanych tylko dla właściciela');
    const c0 = run(['connect', '--probe', probeFile({ url: `http://127.0.0.1:${PPORT}`, token: '' })]);
    ok(c0.res?.where === 'connect' && /wymaga tokenu/.test(c0.res.err), 'panel bez kont: bez tokenu – komunikat', c0.res || c0.stderr);

    // 4. K20: panel z kontami – okno bez tokenu, logowanie kontem na stronie panelu, sesja zostaje w programie
    const APORT = await freePort();
    const apanel = spawn(LIVE, ['--bind', '127.0.0.1', '--port', String(APORT), '--token', PTOKEN, '--data', path.join(tmp, 'data'), '--bridge', `http://127.0.0.1:${BPORT}`, '--bridge-token', BTOKEN], { stdio: 'ignore' });
    kids.push(apanel);
    for (let i = 0; i < 50 && !(await up(APORT)); i++) await sleep(100);
    const AURL = `http://127.0.0.1:${APORT}`;
    const ACC = { user: 'jan', password: 'okno-haslo-123' };
    const setup = await fetch(`${AURL}/api/setup`, { method: 'POST', headers: { 'X-VizRS-Live': '1', 'Content-Type': 'application/json' }, body: JSON.stringify({ code: PTOKEN, ...ACC }) });
    ok(setup.status === 200, 'panel z kontami: pierwsze konto', setup.status);
    const d0 = run(['connect', '--probe', probeFile({ url: AURL, token: '' })]);
    ok(d0.res?.where === 'panel' && d0.res.login, 'panel z kontami: okno łączy bez tokenu i pokazuje logowanie panelu', d0.res || d0.stderr);
    const d1 = run(['connect', '--probe', probeFile({ url: AURL, token: '' }, ACC)]);
    ok(d1.res?.where === 'panel' && d1.res.status === 200 && /OK/.test(d1.res.pill) && d1.res.user === 'jan',
      'logowanie kontem na stronie: panel w oknie jako jan', d1.res || d1.stderr);
    ok(d1.res && !d1.res.cookie.includes('vizrs_live'), 'sesja konta nie trafia do strony', d1.res && d1.res.cookie);
    const saved2 = JSON.parse(fs.readFileSync(path.join(tmp, 'cfg/live-connections.json'), 'utf8'));
    const entry = saved2.servers?.find((s) => s.url === AURL);
    ok(entry && /^[0-9a-f]{64}$/.test(entry.session || '') && entry.token == null && !JSON.stringify(saved2).includes(ACC.password),
      'sesja (nie hasło) na liście zapisanych', saved2);
    const d2 = run(['--connect', AURL, '--probe', probeFile()]);
    ok(d2.res?.where === 'panel' && d2.res.status === 200 && d2.res.user === 'jan', 'ponowne uruchomienie: zapamiętana sesja, bez logowania', d2.res || d2.stderr);
    const out = await fetch(`${AURL}/api/logout`, { method: 'POST', headers: { 'X-VizRS-Live': '1', Cookie: `vizrs_live=${entry.session}` } });
    ok(out.status === 200, 'wylogowanie sesji okna w panelu', out.status);
    const d3 = run(['--connect', AURL, '--probe', probeFile()]);
    ok(d3.res?.where === 'panel' && d3.res.login, 'sesja wygasła: znów logowanie panelu', d3.res || d3.stderr);
    const saved3 = JSON.parse(fs.readFileSync(path.join(tmp, 'cfg/live-connections.json'), 'utf8'));
    ok(saved3.servers?.find((s) => s.url === AURL)?.session == null, 'wygasła sesja skasowana z listy zapisanych', saved3);
  } finally {
    for (const k of kids) k.kill();
    fs.rmSync(tmp, { recursive: true, force: true });
  }
  console.log(failed ? `\n${failed} NIEUDANYCH` : '\nWszystkie testy aplikacji VizRS Live zaliczone');
  process.exit(failed ? 1 : 0);
})();
