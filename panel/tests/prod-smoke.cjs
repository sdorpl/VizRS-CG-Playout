// Przejście po działającym panelu TYLKO DO ODCZYTU (np. lokalna produkcja po tests/deploy.sh): logowanie,
// każdy ekran z menu, nagłówek ≠ „Błąd”, bez wyjątków JS, bez odpowiedzi 5xx, okno zamknięte po zmianie ekranu.
// Bezpiecznik: przeglądarka przepuszcza tylko odczyty – GET, logowanie i komendy odczytu z białej listy mostu:
// Get…, Query…, Search…, Count… i NewFFmpegProfile (domyślne wartości nowego profilu). Rodzaj z /api/commands nie
// wystarcza – most oznacza jako „query” tylko przestrzeń *.Queries, a większość odczytów legacy leży gdzie indziej.
// Każda inna próba (zapis do legacy albo CG Playout, Preview…, Build…) jest przerywana i oblewa przebieg.
// Niczego nie klika poza menu.
//
// Użycie: PANEL_TOKEN=… node tests/prod-smoke.cjs http://127.0.0.1:8090 [katalog-na-zrzuty]
// Playwright: PLAYWRIGHT_DIR (albo tests/local.env, jak e2e).

const fs = require('fs');
const path = require('path');

let dir = process.env.PLAYWRIGHT_DIR;
if (!dir) {
  const env = path.join(__dirname, 'local.env');
  const m = fs.existsSync(env) && /^PLAYWRIGHT_DIR="?([^"\n]*)"?/m.exec(fs.readFileSync(env, 'utf8'));
  dir = m ? m[1] : null;
}
if (!dir) { console.error('prod-smoke: ustaw PLAYWRIGHT_DIR (katalog z node_modules/playwright)'); process.exit(2); }
const { chromium } = require(path.join(dir, 'node_modules', 'playwright'));

const READ_CMD = /^\/api\/cmd\/(?:(?:Get|Query|Search|Count)[A-Za-z]+|NewFFmpegProfile)$/;
const BASE = (process.argv[2] || 'http://127.0.0.1:8090').replace(/\/$/, '');
const SHOTS = process.argv[3] || '';

let failed = 0;
const check = (ok, msg) => { console.log(`${ok ? 'ok  ' : 'BŁĄD'} ${msg}`); if (!ok) failed++; };

(async () => {
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1400, height: 900 } });
  const blocked = [];
  const errors = [];
  await page.route('**/api/**', (route) => {
    const r = route.request();
    const p = new URL(r.url()).pathname;
    if (r.method() === 'GET' || p === '/api/login' || READ_CMD.test(p)) return route.continue();
    blocked.push(`${r.method()} ${p}`);
    return route.abort('blockedbyclient');
  });
  page.on('pageerror', (e) => errors.push('wyjątek JS: ' + e.message));
  // K2: CSP zawężone do znanych originów – naruszenie (obraz, ramka spoza konfiguracji) = błąd przebiegu
  page.on('console', (m) => { if (m.type() === 'error' && /Content Security Policy/i.test(m.text())) errors.push('CSP: ' + m.text()); });
  page.on('response', (r) => { if (r.status() >= 500) errors.push(`${r.status()} ${new URL(r.url()).pathname}`); });

  try {
    await page.goto(`${BASE}/#antena`);
    const first = await Promise.race([
      page.waitForSelector('#login-token', { timeout: 15000 }).then(() => 'login'),
      page.waitForSelector('main[data-view]', { timeout: 15000 }).then(() => 'view'),
    ]);
    if (first === 'login') {
      if (!process.env.PANEL_TOKEN) throw new Error('panel wymaga logowania – ustaw PANEL_TOKEN');
      await page.fill('#login-token', process.env.PANEL_TOKEN);
      await page.click('#dlg-buttons button:has-text("Zaloguj")');
      await page.waitForSelector('main[data-view]', { timeout: 15000 });
    }
    const st = await page.evaluate(async () => (await (await fetch('/api/status')).json()).result);
    check(!!st?.bridge?.result, `most: ${st?.bridge?.result ? `${st.bridge.result.bridge}, legacy ${st.bridge.result.legacy}` : st?.bridgeError || 'brak'}`);

    const views = await page.$$eval('.side a[href^="#"]', (as) => as.map((a) => a.getAttribute('href').slice(1)));
    for (const v of views) {
      const before = errors.length;
      await page.goto(`${BASE}/#${v}`);
      try {
        await page.waitForSelector(`main[data-view="${v}"]`, { timeout: 20000 });
      } catch { check(false, `#${v}: ekran się nie załadował`); continue; }
      await page.waitForTimeout(300);
      const title = (await page.textContent('main .head h1')) || '';
      const err = await page.$$eval('main .note.err', (ns) => ns.map((n) => n.textContent.trim()).join(' | '));
      check(title && title !== 'Błąd' && errors.length === before, `#${v}: ${title}${err ? ` (uwaga: ${err})` : ''}${errors.length > before ? ' – ' + errors.slice(before).join('; ') : ''}`);
      if (SHOTS) await page.screenshot({ path: path.join(SHOTS, `${v}.png`), fullPage: true });
    }
    check(await page.isHidden('#overlay'), 'żadne okno nie zostało otwarte');
    check(blocked.length === 0, `tylko odczyty${blocked.length ? ' – zablokowane: ' + [...new Set(blocked)].join(', ') : ''}`);
  } catch (e) {
    check(false, e.message);
  }
  await browser.close();
  console.log(failed ? `\nprod-smoke: ${failed} błędów` : '\nprod-smoke: zielono');
  process.exit(failed ? 1 : 0);
})();
