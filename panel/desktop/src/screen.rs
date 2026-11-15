//! Rozmiar okna nie większy niż ekran – wspólne dla okien pakietu VizRS (ten sam plik w VizRS CG Artist `src/screen.rs`,
//! VizRS CG System `source/shell/src/screen.rs`, VizRS Live `desktop/src/screen.rs`).
//!
//! Rozmiary okien są w pikselach logicznych. Przy skalowaniu ułamkowym ekran jest „mniejszy”: 1920×1080 przy 125 % to
//! 1536×864 logicznie, więc domyślne 1600×900 albo zapisany rozmiar z innego ekranu wychodziłyby poza krawędź
//! (zgłoszenie 2026-11-14: KDE Plasma Wayland 125 %). Okno dostaje najwyżej 92 % szerokości i 90 % wysokości obszaru
//! roboczego monitora.

/// Rozmiar (w, h) przycięty do obszaru roboczego `area` (px logiczne); nic nie zmienia, gdy obszar nieznany.
pub fn fit(w: f64, h: f64, area: Option<(f64, f64)>) -> (f64, f64) {
    match area {
        Some((aw, ah)) if aw >= 200.0 && ah >= 200.0 => {
            (w.min((aw * 0.92).floor()), h.min((ah * 0.9).floor()))
        }
        _ => (w, h),
    }
}

/// Obszar roboczy monitora głównego (albo pierwszego, gdy kompozytor nie wskazuje głównego, jak często Wayland), px logiczne.
pub fn work_area<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Option<(f64, f64)> {
    let m = app
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| app.available_monitors().ok()?.into_iter().next())?;
    let sf = m.scale_factor();
    if sf <= 0.0 {
        return None;
    }
    let wa = m.work_area().size;
    let (w, h) = if wa.width > 0 && wa.height > 0 {
        (wa.width, wa.height)
    } else {
        (m.size().width, m.size().height)
    };
    Some((f64::from(w) / sf, f64::from(h) / sf))
}

/// Rozmiar okna dopasowany do ekranu, na którym się otworzy.
pub fn fit_to_screen<R: tauri::Runtime>(app: &tauri::AppHandle<R>, w: f64, h: f64) -> (f64, f64) {
    fit(w, h, work_area(app))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn okno_wieksze_niz_ekran_125_procent_jest_przyciete() {
        // 1920×1080 przy 125 % = 1536×864 logicznie (pasek zadań zabiera część wysokości)
        assert_eq!(fit(1600.0, 900.0, Some((1536.0, 824.0))), (1413.0, 741.0));
    }

    #[test]
    fn mniejsze_okno_i_nieznany_ekran_bez_zmian() {
        assert_eq!(fit(1200.0, 700.0, Some((1920.0, 1040.0))), (1200.0, 700.0));
        assert_eq!(fit(1600.0, 900.0, None), (1600.0, 900.0));
        assert_eq!(fit(1600.0, 900.0, Some((0.0, 0.0))), (1600.0, 900.0));
    }
}
