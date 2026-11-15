// Most panelu z aplikacją VizRS Live (desktop/, Tauri 2); w przeglądarce nic nie robi.
// Protokół jak okna VizRS CG System i VizRS CG Artist: window.__vizrsPost('app:' + JSON) – aplikacja przyjmuje
// tylko { t: 'state', title, alert } (biała lista: desktop/src/bridge.rs). Własny moduł zamiast native.js CG System:
// tamten buduje okno pytania przez innerHTML i style w wierszu, czego panel nie robi (CSP, tekst przez textContent).
const shell = window.__vizrsShell;

export function desktopBridge() {
  if (!shell || typeof window.__vizrsPost !== 'function') return;
  document.documentElement.classList.add('native');
  // tytuł okna: ekran (w pasku zadań widać, co jest otwarte); alarm (nie samo ostrzeżenie) – okno w tle prosi o uwagę
  const state = () => {
    const screen = document.querySelector('.side a.on')?.textContent?.trim();
    const alarm = document.querySelector('#alarm:not([hidden]):not(.warn-only)');
    return { t: 'state', title: (alarm ? '● ALARM – ' : '') + (screen ? screen + ' – ' : '') + 'VizRS Live', alert: !!alarm };
  };
  let last = '';
  const sync = () => {
    const j = JSON.stringify(state());
    if (j === last) return;
    last = j;
    try { window.__vizrsPost('app:' + j); } catch { /* okno zamykane */ }
  };
  sync();
  setInterval(sync, 500);
  // okno programu: bez skrótów przeglądarki (szukaj, drukuj, narzędzia); Odśwież i narzędzia są w menu Widok
  addEventListener('keydown', (e) => {
    const k = e.key.toLowerCase();
    if (['F3', 'F7', 'F12'].includes(e.key) || ((e.ctrlKey || e.metaKey) && !e.altKey && k.length === 1 && 'fghjpr'.includes(k))) e.preventDefault();
  }, true);
}
