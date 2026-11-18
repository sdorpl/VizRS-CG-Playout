// Ekran System → Użytkownicy: konta operatorów panelu (tylko administrator). API: /api/users (DOKUMENTACJA §11).

import { S, h, btn, head, panel, table, pill, api, dialog, confirmDlg, toast, route, VIEWS, cmdNote } from '../core.js';

const ROLES = { admin: 'administrator', operator: 'operator' };
const when = (t) => (t ? new Date(t * 1000).toLocaleString('pl-PL', { dateStyle: 'short', timeStyle: 'short' }) : '—');
const roleSelect = (id, value) => h('select', { id }, Object.entries(ROLES).map(([v, t]) => h('option', { value: v, selected: v === value || null }, t)));
const usersUrl = (name) => '/api/users/' + encodeURIComponent(name);

async function newPassword(u) {
  const p1 = h('input', { type: 'password', id: 'u-pass', autocomplete: 'new-password' });
  const p2 = h('input', { type: 'password', id: 'u-pass2', autocomplete: 'new-password' });
  const body = h('div', { class: 'form' },
    h('label', { for: 'u-pass' }, 'Nowe hasło'), p1, h('label', { for: 'u-pass2' }, 'Powtórz'), p2,
    h('span'), h('span', { class: 'dim' }, `Co najmniej 8 znaków. Konto ${u.name} zostanie wylogowane ze wszystkich przeglądarek.`));
  if (!(await dialog(`Nowe hasło: ${u.name}`, body, [['Anuluj', false], ['Ustaw hasło', true, 'acc']]))) return;
  if (p1.value !== p2.value) return toast('Hasła się różnią', 'bad');
  try {
    await api(usersUrl(u.name), { method: 'PUT', body: JSON.stringify({ password: p1.value }) });
    toast(`Hasło konta ${u.name} zmienione`);
    route(true);
  } catch (e) { toast(e.message, 'bad'); }
}

async function setRole(u, sel) {
  try {
    await api(usersUrl(u.name), { method: 'PUT', body: JSON.stringify({ role: sel.value }) });
    toast(`${u.name}: ${ROLES[sel.value]}`);
    route(true);
  } catch (e) { sel.value = u.role; toast(e.message, 'bad'); }
}

function addPanel() {
  const user = h('input', { id: 'nu-user', autocomplete: 'off', autocapitalize: 'none', spellcheck: 'false', maxlength: 32 });
  const pass = h('input', { type: 'password', id: 'nu-pass', autocomplete: 'new-password' });
  const role = roleSelect('nu-role', 'operator');
  const add = async () => {
    try {
      await api('/api/users', { method: 'POST', body: JSON.stringify({ user: user.value.trim(), password: pass.value, role: role.value }) });
      toast(`Dodano konto ${user.value.trim().toLowerCase()}`);
      route(true);
    } catch (e) { toast(e.message, 'bad'); }
  };
  return panel('Nowe konto', null, h('div', { class: 'form pad' },
    h('label', { for: 'nu-user' }, 'Login'), user,
    h('label', { for: 'nu-pass' }, 'Hasło'), pass,
    h('label', { for: 'nu-role' }, 'Rola'), role,
    h('span'), h('span', { class: 'dim' }, 'Login: litery, cyfry i . - _ (wielkość liter bez znaczenia). Hasło: co najmniej 8 znaków. Operator pracuje na antenie i w ramówce; administrator dodatkowo zarządza kontami.'),
    h('span'), h('div', { class: 'bar' }, btn('Dodaj konto', add, 'acc'))));
}

/** Wspólne logowanie w pakiecie: konta panelu logują też do VizRS Trio i Artist (opcja, domyślnie wyłączona). */
async function ssoPanel() {
  let st;
  try { st = (await api('/api/sso')).result; } catch (e) { return panel('Logowanie w pakiecie VizRS', null, h('div', { class: 'empty err' }, e.message)); }
  const box = h('input', { type: 'checkbox', id: 'sso-on', checked: st.enabled });
  box.addEventListener('change', async () => {
    try {
      await api('/api/sso', { method: 'PUT', body: JSON.stringify({ enabled: box.checked }) });
      toast(box.checked ? 'Konta panelu logują też do VizRS Trio i Artist' : 'Trio i Artist – tylko własny token');
    } catch (e) { box.checked = !box.checked; toast(e.message, 'bad'); }
  });
  return panel('Logowanie w pakiecie VizRS', null, h('div', { class: 'pad' },
    h('label', { class: 'chk' }, box, ' Konta panelu logują też do VizRS Trio i Artist (VizRS CG System w tym stosie)'),
    h('p', { class: 'dim' }, 'Opcja: ekran logowania Trio i Artist przyjmuje wtedy login i hasło z panelu (token Trio działa dalej), a kto jest zalogowany w panelu pod tym samym adresem serwera, wchodzi do Trio bez ponownego logowania. Wyłączenie albo usunięcie konta kończy jego sesje w Trio. Trio uruchomione samo (np. dla OBS) nie zależy od panelu – loguje się swoim tokenem. Wymaga VizRS CG System z obsługą kont panelu.')));
}

VIEWS.uzytkownicy = async () => {
  const title = head('Użytkownicy', 'Konta operatorów panelu: login i hasło zamiast wspólnego tokenu. Konta i sesje leżą w katalogu danych panelu (wolumen) – restart ani nowy obraz nie wylogowują.');
  if (!S.auth.accounts) {
    return [title, h('div', { class: 'note err' }, S.auth.broken
      ? `Konta są wyłączone: ${S.auth.broken}.`
      : 'Konta są wyłączone: panel działa bez katalogu danych (VIZRS_LIVE_DATA). W stosie compose katalog danych jest na wolumenie live-data – zaktualizuj stos.')];
  }
  if (S.auth.user?.role !== 'admin') return [title, h('div', { class: 'note' }, 'Kontami zarządza administrator.')];
  const users = (await api('/api/users')).result;
  const me = S.auth.user?.name;
  const rows = table(users, [
    [(u) => h('span', null, h('b', null, u.name), u.name === me ? ' (to Ty)' : ''), 'Login'],
    [(u) => { const s = roleSelect(`role-${u.name}`, u.role); s.setAttribute('aria-label', `Rola konta ${u.name}`); s.addEventListener('change', () => setRole(u, s)); return s; }, 'Rola'],
    [(u) => when(u.created), 'Utworzone'],
    [(u) => when(u.lastSeen), 'Ostatnio w panelu'],
    [(u) => (u.sessions ? pill(`${u.sessions}`, 'ok') : '0'), 'Sesje'],
    [(u) => h('div', { class: 'bar' },
      btn('Nowe hasło', () => newPassword(u)),
      btn('Wyloguj', async () => {
        try { await api(usersUrl(u.name) + '/logout', { method: 'POST', body: '' }); toast(`${u.name}: wylogowano`); route(true); } catch (e) { toast(e.message, 'bad'); }
      }),
      btn('Usuń', async () => {
        if (!(await confirmDlg('Usuń konto', `Usunąć konto ${u.name}? Jego sesje zostaną zamknięte.`, 'Usuń'))) return;
        try { await api(usersUrl(u.name), { method: 'DELETE' }); toast(`Usunięto konto ${u.name}`); route(true); } catch (e) { toast(e.message, 'bad'); }
      }, 'danger')), ''],
  ]);
  return [title, panel(`Konta (${users.length})`, null, rows, cmdNote('panel: GET/POST/PUT/DELETE /api/users')), addPanel(), await ssoPanel()];
};
