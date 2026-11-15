//! Skala ułamkowa ekranu (Linux) – ten sam plik w VizRS CG Artist `src/scale.rs`, VizRS CG System `source/shell/src/scale.rs`
//! i VizRS Live `desktop/src/scale.rs`. GTK 3 / WebKitGTK 4.1 nie zna skalowania ułamkowego Waylanda – przy 125 % zaokrągla
//! w górę do 2, rysuje stronę w buforze ×2 (`devicePixelRatio` 2), a kompozytor zmniejsza ją ×0,625. Skutek (pomiar
//! 2026-11-14, KDE Plasma Wayland 125 %, Quadro P400): nierówne cienkie linie, 2,56× więcej pikseli (animacja 5 kl./s
//! zamiast 18 przez XWayland) i nieprawdziwe „1:1” na kanwie.
//!
//! KDE zapisuje skalę dla programów X11 w `kwinrc` (`[Xwayland] Scale=1.25`, gdy programy X11 „skalują się same” –
//! domyślnie w Plasmie 6): okno przez XWayland ma wtedy natywne piksele, a WebKitGTK sam powiększa stronę o
//! `Xft.dpi / 96`. Innych pulpitów nie zgadujemy (GNOME rozmywa programy X11 przy skali ułamkowej) – tam
//! `VIZRS_SCALE` podaje skalę tylko do „1:1” na kanwie.

/// `Scale` z sekcji `[Xwayland]` pliku `kwinrc`.
pub fn kwinrc_xwayland(text: &str) -> Option<f64> {
    let mut section = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            section = line == "[Xwayland]";
        } else if section {
            if let Some(v) = line.strip_prefix("Scale=") {
                return valid(v.trim().parse().ok()?);
            }
        }
    }
    None
}

fn valid(s: f64) -> Option<f64> {
    (s.is_finite() && (0.5..=4.0).contains(&s)).then_some(s)
}

/// Skala nie jest całkowita (1.25, 1.5…).
pub fn fractional(s: f64) -> bool {
    (s - s.round()).abs() > 0.01
}

/// Skala programów X11 w sesji KDE (`$XDG_CONFIG_HOME/kwinrc`); poza KDE i bez wpisu – brak.
pub fn kde_xwayland() -> Option<f64> {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    if !desktop.split(':').any(|d| d.eq_ignore_ascii_case("KDE")) {
        return None;
    }
    let dir = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|d| !d.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".config")))?;
    kwinrc_xwayland(&std::fs::read_to_string(dir.join("kwinrc")).ok()?)
}

/// Ile pikseli ekranu przypada na piksel CSS przy powiększeniu interfejsu 100 % – dla „1:1” na kanwie.
/// `VIZRS_SCALE` (np. `1.25`) ma pierwszeństwo; potem skala KDE; inaczej 1 (jak dawniej).
#[allow(dead_code)] // używa tylko VizRS CG Artist („1:1” kanwy); okno Trio i VizRS Live – tylko przełączenie na XWayland
pub fn screen_scale(env: Option<&str>, kde: Option<f64>) -> f64 {
    env.and_then(|v| valid(v.trim().replace(',', ".").parse().ok()?))
        .or(kde)
        .unwrap_or(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skala_xwayland_z_kwinrc() {
        let t = "[Compositing]\nScale=3\n\n[Xwayland]\nScale=1.25\n\n[Wayland]\nScale=2\n";
        assert_eq!(kwinrc_xwayland(t), Some(1.25));
        assert_eq!(
            kwinrc_xwayland("[Xwayland]\nXwaylandEavesdrops=None\n"),
            None
        );
        assert_eq!(kwinrc_xwayland("[Xwayland]\nScale=abc\n"), None);
        assert_eq!(kwinrc_xwayland("[Xwayland]\nScale=40\n"), None);
        assert_eq!(kwinrc_xwayland(""), None);
    }

    #[test]
    fn ulamkowa_i_calkowita() {
        assert!(fractional(1.25) && fractional(1.5));
        assert!(!fractional(1.0) && !fractional(2.0));
    }

    #[test]
    fn skala_ekranu_zmienna_ma_pierwszenstwo() {
        assert_eq!(screen_scale(Some("1,5"), Some(1.25)), 1.5);
        assert_eq!(screen_scale(Some("zła"), Some(1.25)), 1.25);
        assert_eq!(screen_scale(None, None), 1.0);
    }
}
