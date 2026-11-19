//! Konta operatorów i sesje przeglądarek (logowanie loginem i hasłem, konfiguracja z poziomu panelu).
//!
//! Pliki w katalogu danych (`--data`, w obrazie `/data` na wolumenie): `users.json` (login, rola,
//! skrót hasła PBKDF2-HMAC-SHA256 z solą) i `sessions.json` (skrót SHA-256 identyfikatora sesji –
//! nie sam identyfikator – właściciel, ostatnie użycie). Zapis atomowy (plik tymczasowy + `rename`),
//! prawa tylko dla właściciela. `settings.json` – przełącznik wspólnego logowania w pakiecie
//! (konta panelu logują też do VizRS CG System – Trio, Artist; domyślnie wyłączony).
//! Bez katalogu danych konta są wyłączone, a sesje (logowanie tokenem)
//! żyją tylko w pamięci – jak przed kontami.

use crate::crypto::{hex, pbkdf2_sha256, random_bytes, sha256, unhex};
use crate::http::json_str;
use crate::json;
use std::collections::HashMap;
use std::path::PathBuf;

/// Iteracje PBKDF2 nowych haseł (zapisywane przy haśle – zmiana nie unieważnia starych).
pub const PBKDF2_ITER: u32 = if cfg!(test) { 2 } else { 310_000 };
/// Najwyżej tylu użytkowników (plik i lista w UI zostają małe).
pub const USERS_MAX: usize = 200;
/// Sesja bez użycia przez tyle sekund wygasa (30 dni).
pub const SESSION_IDLE_SECS: u64 = 2_592_000;
/// Najwyżej tyle sesji naraz; przy nadmiarze najpierw wygasłe, potem najdawniej używana.
pub const SESSION_MAX: usize = 256;
/// Odświeżenie czasu użycia sesji zapisuje plik najwyżej co tyle sekund.
const TOUCH_SAVE_SECS: u64 = 600;
pub const PASSWORD_MIN: usize = 8;
const PASSWORD_MAX: usize = 256;
const NAME_MAX: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Admin,
    Operator,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Operator => "operator",
        }
    }
    pub fn parse(s: &str) -> Option<Role> {
        match s {
            "admin" => Some(Role::Admin),
            "operator" => Some(Role::Operator),
            _ => None,
        }
    }
}

/// Kto wysłał żądanie. `user` pusty = token awaryjny (`--token`) albo panel bez logowania.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Who {
    pub user: String,
    pub role: Role,
}

impl Who {
    pub fn token() -> Who {
        Who {
            user: String::new(),
            role: Role::Admin,
        }
    }
    pub fn admin(&self) -> bool {
        self.role == Role::Admin
    }
}

/// Skrót hasła z solą – liczony bez blokady magazynu (PBKDF2 trwa ułamek sekundy).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hashed {
    salt: Vec<u8>,
    iter: u32,
    hash: [u8; 32],
}

impl Hashed {
    pub fn new(password: &str) -> Result<Hashed, String> {
        Hashed::with_iter(password, PBKDF2_ITER)
    }
    fn with_iter(password: &str, iter: u32) -> Result<Hashed, String> {
        let salt = random_bytes(16)?;
        let hash = pbkdf2_sha256(password.as_bytes(), &salt, iter);
        Ok(Hashed { salt, iter, hash })
    }
    /// Porównanie w stałym czasie.
    pub fn verify(&self, password: &str) -> bool {
        let h = pbkdf2_sha256(password.as_bytes(), &self.salt, self.iter);
        h.iter().zip(self.hash).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0
    }
    /// Skrót „na wszelki wypadek” dla nieznanego loginu – ten sam czas odpowiedzi co przy złym haśle.
    pub fn dummy() -> Hashed {
        Hashed {
            salt: vec![0; 16],
            iter: PBKDF2_ITER,
            hash: [0; 32],
        }
    }
}

#[derive(Clone, Debug)]
struct User {
    name: String,
    role: Role,
    pw: Hashed,
    created: u64,
}

#[derive(Clone, Debug)]
struct Sess {
    user: String,
    last: u64,
}

/// Login: małe litery (bez względu na wpisane), cyfry, `.`, `-`, `_`; 1–32 znaki.
pub fn normalize_name(raw: &str) -> Result<String, String> {
    let n = raw.trim().to_lowercase();
    if n.is_empty() || n.chars().count() > NAME_MAX {
        return Err(format!("login: od 1 do {NAME_MAX} znaków"));
    }
    if !n.chars().all(|c| c.is_alphanumeric() || "._-".contains(c)) {
        return Err("login: tylko litery, cyfry i . - _".into());
    }
    Ok(n)
}

pub fn check_password(p: &str) -> Result<(), String> {
    let n = p.chars().count();
    if n < PASSWORD_MIN {
        return Err(format!("hasło: co najmniej {PASSWORD_MIN} znaków"));
    }
    if n > PASSWORD_MAX {
        return Err(format!("hasło: najwyżej {PASSWORD_MAX} znaków"));
    }
    Ok(())
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Jednorazowy kod pierwszej konfiguracji: 12 znaków (60 bitów) bez mylących (0/O, 1/I), np. `K7QM-3XRP-W9TD`.
fn setup_code() -> Result<String, String> {
    const ABC: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let b = random_bytes(12)?;
    let s: String = b.iter().map(|x| ABC[(*x & 31) as usize] as char).collect();
    Ok(format!("{}-{}-{}", &s[..4], &s[4..8], &s[8..]))
}

/// Kod wpisany przez człowieka: bez spacji i myślników, wielkość liter bez znaczenia.
fn code_eq(given: &str, code: &str) -> bool {
    let norm = |s: &str| -> String {
        s.chars()
            .filter(|c| !c.is_whitespace() && *c != '-')
            .flat_map(char::to_uppercase)
            .collect()
    };
    crate::session::eq_ct(&norm(given), &norm(code))
}

pub struct Store {
    dir: Option<PathBuf>,
    /// pliku kont nie da się odczytać ani zapisać – konta wyłączone, logowanie tylko tokenem
    broken: Option<String>,
    users: Vec<User>,
    sessions: HashMap<String, Sess>,
    last_save: u64,
    setup: Option<String>,
    sso: bool,
}

impl Default for Store {
    /// Bez katalogu danych: bez kont, sesje w pamięci.
    fn default() -> Self {
        Store {
            dir: None,
            broken: None,
            users: Vec::new(),
            sessions: HashMap::new(),
            last_save: 0,
            setup: None,
            sso: false,
        }
    }
}

impl Store {
    /// Wczytuje konta i sesje z `dir` (tworzy katalog). Błąd odczytu nie zatrzymuje panelu –
    /// konta wyłączone, powód w logu i w `/api/auth`; pliku z błędem nie nadpisujemy.
    pub fn open(dir: Option<PathBuf>, now: u64) -> Store {
        let mut s = Store {
            dir,
            ..Store::default()
        };
        let Some(d) = s.dir.clone() else {
            return s;
        };
        if let Err(e) = std::fs::create_dir_all(&d) {
            s.broken = Some(format!("katalog danych {}: {e}", d.display()));
            return s;
        }
        match std::fs::read_to_string(d.join("users.json")) {
            Ok(text) => match parse_users(&text) {
                Ok(u) => s.users = u,
                Err(e) => {
                    s.broken = Some(format!("users.json: {e}"));
                    return s;
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                s.broken = Some(format!("users.json: {e}"));
                return s;
            }
        }
        // przełącznik: zły albo brakujący plik = wyłączony (bezpieczniej)
        if let Ok(text) = std::fs::read_to_string(d.join("settings.json")) {
            s.sso = json::parse(&text).ok().and_then(|v| v.get("sso").cloned())
                == Some(json::Value::Bool(true));
        }
        // sesje to tylko wygoda: zły plik = wszyscy logują się od nowa
        if let Ok(text) = std::fs::read_to_string(d.join("sessions.json")) {
            s.sessions = parse_sessions(&text, now).unwrap_or_default();
        }
        let users = s.users.clone();
        s.sessions
            .retain(|_, x| x.user.is_empty() || users.iter().any(|u| u.name == x.user));
        // próba zapisu od razu: katalog tylko do odczytu wychodzi przy starcie, nie przy pierwszym koncie
        if let Err(e) = s.save_sessions(now) {
            s.broken = Some(e);
            return s;
        }
        if s.users.is_empty() {
            match setup_code() {
                Ok(c) => s.setup = Some(c),
                Err(e) => s.broken = Some(e),
            }
        }
        s
    }

    pub fn enabled(&self) -> bool {
        self.dir.is_some() && self.broken.is_none()
    }
    pub fn broken(&self) -> Option<&str> {
        self.broken.as_deref()
    }
    /// Kont jeszcze nie ma – panel prosi o pierwsze konto administratora.
    pub fn needs_setup(&self) -> bool {
        self.enabled() && self.users.is_empty()
    }
    pub fn setup_code(&self) -> Option<&str> {
        self.setup.as_deref()
    }
    pub fn setup_code_ok(&self, given: &str) -> bool {
        self.needs_setup() && self.setup.as_deref().is_some_and(|c| code_eq(given, c))
    }

    /// Skrót hasła konta (do sprawdzenia poza blokadą); `None` – nie ma takiego konta.
    pub fn credentials(&self, name: &str) -> Option<Hashed> {
        self.users
            .iter()
            .find(|u| u.name == name)
            .map(|u| u.pw.clone())
    }

    pub fn who(&self, user: &str) -> Option<Who> {
        if user.is_empty() {
            return Some(Who::token());
        }
        self.users.iter().find(|u| u.name == user).map(|u| Who {
            user: u.name.clone(),
            role: u.role,
        })
    }

    /// Wspólne logowanie w pakiecie: konta panelu logują też do VizRS CG System (tylko z kontami).
    pub fn sso(&self) -> bool {
        self.enabled() && self.sso
    }
    pub fn set_sso(&mut self, on: bool) -> Result<(), String> {
        let Some(d) = &self.dir else {
            return Err("konta wyłączone".into());
        };
        if !self.enabled() {
            return Err("konta wyłączone".into());
        }
        write_private(
            &d.join("settings.json"),
            format!("{{\"version\":1,\"sso\":{on}}}\n").as_bytes(),
        )?;
        self.sso = on;
        Ok(())
    }

    // ---------- sesje ----------

    pub fn session_new(&mut self, id: &str, user: &str, now: u64) {
        if self.sessions.len() >= SESSION_MAX {
            self.sessions
                .retain(|_, s| now.saturating_sub(s.last) < SESSION_IDLE_SECS);
        }
        if self.sessions.len() >= SESSION_MAX {
            if let Some(old) = self
                .sessions
                .iter()
                .min_by_key(|(_, s)| s.last)
                .map(|(k, _)| k.clone())
            {
                self.sessions.remove(&old);
            }
        }
        self.sessions.insert(
            hex(&sha256(id.as_bytes())),
            Sess {
                user: user.into(),
                last: now,
            },
        );
        self.persist_sessions(now);
    }

    /// Ważna sesja → kto; odświeża czas użycia. Sesja usuniętego konta przestaje działać od razu.
    pub fn session_check(&mut self, id: &str, now: u64) -> Option<Who> {
        let key = hex(&sha256(id.as_bytes()));
        let s = self.sessions.get(&key)?.clone();
        let who = (now.saturating_sub(s.last) < SESSION_IDLE_SECS)
            .then(|| self.who(&s.user))
            .flatten();
        match who {
            Some(w) => {
                if let Some(x) = self.sessions.get_mut(&key) {
                    x.last = now;
                }
                if now.saturating_sub(self.last_save) >= TOUCH_SAVE_SECS {
                    self.persist_sessions(now);
                }
                Some(w)
            }
            None => {
                self.sessions.remove(&key);
                self.persist_sessions(now);
                None
            }
        }
    }

    pub fn session_end(&mut self, id: &str, now: u64) {
        if self.sessions.remove(&hex(&sha256(id.as_bytes()))).is_some() {
            self.persist_sessions(now);
        }
    }

    /// Wylogowuje wszystkie sesje konta (poza `keep` – bieżącą sesją przy zmianie własnego hasła).
    pub fn end_user_sessions(&mut self, user: &str, keep: Option<&str>, now: u64) -> usize {
        let keep = keep.map(|id| hex(&sha256(id.as_bytes())));
        let before = self.sessions.len();
        self.sessions
            .retain(|k, s| s.user != user || keep.as_deref() == Some(k.as_str()));
        let n = before - self.sessions.len();
        if n > 0 {
            self.persist_sessions(now);
        }
        n
    }

    #[cfg(test)]
    pub(crate) fn session_count(&self) -> usize {
        self.sessions.len()
    }

    // ---------- konta ----------

    /// Lista kont do ekranu Użytkownicy (bez skrótów haseł).
    pub fn list_json(&self, now: u64) -> String {
        let items: Vec<String> = self
            .users
            .iter()
            .map(|u| {
                let n = self
                    .sessions
                    .values()
                    .filter(|s| s.user == u.name && now.saturating_sub(s.last) < SESSION_IDLE_SECS)
                    .count();
                let last = self
                    .sessions
                    .values()
                    .filter(|s| s.user == u.name)
                    .map(|s| s.last)
                    .max();
                format!(
                    "{{\"name\":{},\"role\":\"{}\",\"created\":{},\"sessions\":{n},\"lastSeen\":{}}}",
                    json_str(&u.name),
                    u.role.as_str(),
                    u.created,
                    last.map_or("null".into(), |t| t.to_string())
                )
            })
            .collect();
        format!("[{}]", items.join(","))
    }

    pub fn add_user(&mut self, name: &str, role: Role, pw: Hashed, now: u64) -> Result<(), String> {
        if !self.enabled() {
            return Err("konta wyłączone".into());
        }
        if self.users.iter().any(|u| u.name == name) {
            return Err(format!("konto {name} już istnieje"));
        }
        if self.users.len() >= USERS_MAX {
            return Err(format!("najwyżej {USERS_MAX} kont"));
        }
        let mut next = self.users.clone();
        next.push(User {
            name: name.into(),
            role,
            pw,
            created: now,
        });
        self.save_users(next)?;
        self.setup = None;
        Ok(())
    }

    fn admins(&self) -> usize {
        self.users.iter().filter(|u| u.role == Role::Admin).count()
    }

    pub fn set_role(&mut self, name: &str, role: Role) -> Result<(), String> {
        let i = self.index(name)?;
        if self.users[i].role == Role::Admin && role != Role::Admin && self.admins() == 1 {
            return Err("to jedyny administrator – najpierw nadaj tę rolę komuś innemu".into());
        }
        let mut next = self.users.clone();
        next[i].role = role;
        self.save_users(next)
    }

    pub fn set_password(&mut self, name: &str, pw: Hashed) -> Result<(), String> {
        let i = self.index(name)?;
        let mut next = self.users.clone();
        next[i].pw = pw;
        self.save_users(next)
    }

    pub fn delete(&mut self, name: &str, now: u64) -> Result<(), String> {
        let i = self.index(name)?;
        if self.users[i].role == Role::Admin && self.admins() == 1 {
            return Err("nie można usunąć jedynego administratora".into());
        }
        let mut next = self.users.clone();
        next.remove(i);
        self.save_users(next)?;
        self.end_user_sessions(name, None, now);
        Ok(())
    }

    fn index(&self, name: &str) -> Result<usize, String> {
        self.users
            .iter()
            .position(|u| u.name == name)
            .ok_or_else(|| format!("nie ma konta {name}"))
    }

    // ---------- pliki ----------

    /// Najpierw plik, potem pamięć: nieudany zapis nie zmienia kont.
    fn save_users(&mut self, next: Vec<User>) -> Result<(), String> {
        let Some(d) = &self.dir else {
            return Err("konta wyłączone".into());
        };
        let items: Vec<String> = next
            .iter()
            .map(|u| {
                format!(
                    "{{\"name\":{},\"role\":\"{}\",\"created\":{},\"iter\":{},\"salt\":\"{}\",\"hash\":\"{}\"}}",
                    json_str(&u.name),
                    u.role.as_str(),
                    u.created,
                    u.pw.iter,
                    hex(&u.pw.salt),
                    hex(&u.pw.hash)
                )
            })
            .collect();
        let body = format!("{{\"version\":1,\"users\":[\n{}\n]}}\n", items.join(",\n"));
        write_private(&d.join("users.json"), body.as_bytes())?;
        self.users = next;
        Ok(())
    }

    fn save_sessions(&mut self, now: u64) -> Result<(), String> {
        self.last_save = now;
        let Some(d) = &self.dir else {
            return Ok(());
        };
        let items: Vec<String> = self
            .sessions
            .iter()
            .map(|(k, s)| {
                format!(
                    "{{\"key\":\"{k}\",\"user\":{},\"last\":{}}}",
                    json_str(&s.user),
                    s.last
                )
            })
            .collect();
        let body = format!("{{\"version\":1,\"sessions\":[{}]}}\n", items.join(","));
        write_private(&d.join("sessions.json"), body.as_bytes())
    }

    /// Błąd zapisu sesji tylko do logu: logowanie działa dalej, sesje znikną najwyżej po restarcie.
    fn persist_sessions(&mut self, now: u64) {
        if let Err(e) = self.save_sessions(now) {
            eprintln!("sesje: {e}");
        }
    }
}

fn write_private(path: &std::path::Path, body: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    o.open(&tmp)
        .and_then(|mut f| {
            f.write_all(body)?;
            f.sync_all()
        })
        .and_then(|()| std::fs::rename(&tmp, path))
        .map_err(|e| format!("zapis {}: {e}", path.display()))
}

fn parse_users(text: &str) -> Result<Vec<User>, String> {
    let v = json::parse(text)?;
    let mut out: Vec<User> = Vec::new();
    for u in v.get("users").map(|a| a.arr()).unwrap_or(&[]) {
        let name = u
            .get("name")
            .and_then(|x| x.str())
            .ok_or("konto bez name")?;
        let name = normalize_name(name)?;
        let role = u
            .get("role")
            .and_then(|x| x.str())
            .and_then(Role::parse)
            .ok_or_else(|| format!("{name}: zła rola"))?;
        let iter = u
            .get("iter")
            .and_then(|x| x.num())
            .filter(|n| *n >= 1.0 && *n <= 10_000_000.0)
            .ok_or_else(|| format!("{name}: złe iter"))? as u32;
        let salt = u
            .get("salt")
            .and_then(|x| x.str())
            .and_then(unhex)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("{name}: zła sól"))?;
        let hash: [u8; 32] = u
            .get("hash")
            .and_then(|x| x.str())
            .and_then(unhex)
            .and_then(|h| h.try_into().ok())
            .ok_or_else(|| format!("{name}: zły skrót"))?;
        let created = u.get("created").and_then(|x| x.num()).unwrap_or(0.0) as u64;
        if out.iter().any(|x| x.name == name) {
            return Err(format!("konto {name} dwa razy"));
        }
        out.push(User {
            name,
            role,
            pw: Hashed { salt, iter, hash },
            created,
        });
    }
    Ok(out)
}

fn parse_sessions(text: &str, now: u64) -> Result<HashMap<String, Sess>, String> {
    let v = json::parse(text)?;
    let mut out = HashMap::new();
    for s in v.get("sessions").map(|a| a.arr()).unwrap_or(&[]) {
        let (Some(k), Some(user), Some(last)) = (
            s.get("key").and_then(|x| x.str()),
            s.get("user").and_then(|x| x.str()),
            s.get("last").and_then(|x| x.num()),
        ) else {
            continue;
        };
        let last = last as u64;
        if k.len() == 64
            && k.bytes().all(|b| b.is_ascii_hexdigit())
            && now.saturating_sub(last) < SESSION_IDLE_SECS
            && out.len() < SESSION_MAX
        {
            out.insert(
                k.to_string(),
                Sess {
                    user: user.into(),
                    last,
                },
            );
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("vizrs-live-accounts-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }
    /// Czas logowania przy iteracjach produkcji: `cargo test --release -- --ignored pbkdf2_speed --nocapture`.
    #[test]
    #[ignore]
    fn pbkdf2_speed() {
        let t = std::time::Instant::now();
        pbkdf2_sha256(b"haslo", b"sol-16-bajtow-ab", 310_000);
        eprintln!("PBKDF2 310000: {:?}", t.elapsed());
    }

    fn quick(p: &str) -> Hashed {
        Hashed::with_iter(p, 2).unwrap()
    }

    #[test]
    fn names_and_passwords() {
        assert_eq!(normalize_name("  Jan.Kowalski ").unwrap(), "jan.kowalski");
        assert_eq!(normalize_name("Łukasz_2").unwrap(), "łukasz_2");
        assert!(normalize_name("").is_err());
        assert!(normalize_name("a b").is_err());
        assert!(normalize_name("x\"y").is_err());
        assert!(normalize_name(&"a".repeat(33)).is_err());
        assert!(check_password("1234567").is_err());
        assert!(check_password("12345678").is_ok());
        let h = quick("tajne-hasło");
        assert!(h.verify("tajne-hasło") && !h.verify("tajne-haslo"));
        assert_ne!(quick("x").salt, quick("x").salt, "sól losowa");
    }

    #[test]
    fn setup_codes() {
        let c = setup_code().unwrap();
        assert_eq!(c.len(), 14);
        assert!(code_eq(&c.to_lowercase().replace('-', " "), &c));
        assert!(!code_eq("AAAA-AAAA-AAAA", &c) || c == "AAAA-AAAA-AAAA");
    }

    #[test]
    fn store_round_trip_and_rules() {
        let d = tmp("rt");
        let mut s = Store::open(Some(d.clone()), 1000);
        assert!(s.enabled() && s.needs_setup());
        let code = s.setup_code().unwrap().to_string();
        assert!(s.setup_code_ok(&code));
        s.add_user("jan", Role::Admin, quick("hasło-jana"), 1000)
            .unwrap();
        assert!(!s.needs_setup() && s.setup_code().is_none() && !s.setup_code_ok(&code));
        assert!(s.add_user("jan", Role::Operator, quick("x"), 1000).is_err());
        s.add_user("ola", Role::Operator, quick("hasło-oli"), 1000)
            .unwrap();
        assert!(s.set_role("jan", Role::Operator).is_err(), "jedyny admin");
        assert!(s.delete("jan", 1000).is_err(), "jedyny admin");
        s.session_new("sesja-oli", "ola", 1000);
        s.session_new("sesja-tokenu", "", 1000);
        assert_eq!(
            s.session_check("sesja-oli", 1001).map(|w| w.role),
            Some(Role::Operator)
        );
        assert_eq!(s.session_check("sesja-tokenu", 1001), Some(Who::token()));
        // pliki: identyfikatora sesji ani hasła nie ma w postaci jawnej
        let files = std::fs::read_to_string(d.join("users.json")).unwrap()
            + &std::fs::read_to_string(d.join("sessions.json")).unwrap();
        assert!(!files.contains("sesja-oli") && !files.contains("hasło"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let m = std::fs::metadata(d.join("users.json")).unwrap();
            assert_eq!(m.permissions().mode() & 0o777, 0o600);
        }
        // po restarcie: konta i sesje zostają
        let mut r = Store::open(Some(d.clone()), 2000);
        assert!(!r.needs_setup());
        assert!(r.credentials("ola").unwrap().verify("hasło-oli"));
        assert_eq!(r.session_check("sesja-oli", 2000).unwrap().user, "ola");
        // rola zmienia się od razu w działającej sesji; usunięcie konta kończy jego sesje
        r.set_role("ola", Role::Admin).unwrap();
        assert!(r.session_check("sesja-oli", 2001).unwrap().admin());
        r.set_role("jan", Role::Operator).unwrap();
        r.delete("jan", 2002).unwrap();
        r.set_role("ola", Role::Operator).unwrap_err();
        r.add_user("ewa", Role::Operator, quick("hasło-ewy"), 2003)
            .unwrap();
        r.session_new("e1", "ewa", 2003);
        r.session_new("e2", "ewa", 2003);
        assert_eq!(r.end_user_sessions("ewa", Some("e2"), 2004), 1);
        assert!(r.session_check("e1", 2005).is_none() && r.session_check("e2", 2005).is_some());
        r.delete("ewa", 2006).unwrap();
        assert!(r.session_check("e2", 2007).is_none());
        // wspólne logowanie: domyślnie wyłączone, przełącznik przeżywa restart
        assert!(!r.sso());
        r.set_sso(true).unwrap();
        assert!(Store::open(Some(d.clone()), 2008).sso());
        // wygasanie
        assert!(r
            .session_check("sesja-oli", 2001 + SESSION_IDLE_SECS)
            .is_none());
        let list = r.list_json(3000);
        assert!(list.contains("\"name\":\"ola\"") && !list.contains("hash"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn broken_file_is_not_overwritten() {
        let d = tmp("broken");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("users.json"), "{zepsuty").unwrap();
        let mut s = Store::open(Some(d.clone()), 1);
        assert!(!s.enabled() && !s.needs_setup() && s.broken().is_some());
        assert!(s.add_user("a", Role::Admin, quick("12345678"), 1).is_err());
        assert!(s.set_sso(true).is_err() && !s.sso());
        assert_eq!(
            std::fs::read_to_string(d.join("users.json")).unwrap(),
            "{zepsuty"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn memory_only_sessions_and_limit() {
        let mut s = Store::default();
        assert!(!s.enabled() && !s.needs_setup());
        for n in 0..SESSION_MAX + 5 {
            s.session_new(&format!("s{n}"), "", n as u64);
        }
        assert_eq!(s.session_count(), SESSION_MAX);
        assert!(
            s.session_check("s0", 10).is_none(),
            "najdawniej używana wypada"
        );
        assert!(s.session_check("s260", 300).is_some());
        s.session_end("s10", 11);
        assert!(s.session_check("s10", 12).is_none());
    }
}
