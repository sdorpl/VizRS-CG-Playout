//! Akceleracja sprzętowa WebKitGTK (Linux) – ustawienia przed startem GTK, wspólne dla okien pakietu VizRS
//! (ten sam plik w VizRS CG Artist `src/gpu.rs`, VizRS CG System `source/shell/src/gpu.rs`, VizRS Live `desktop/src/gpu.rs`).
//!
//! WebKitGTK sam wyłącza akcelerację przy sterowniku własnościowym NVIDIA („Policy: never” w `webkit://gpu`): kanwa rysuje się
//! wtedy na CPU (pomiar 2026-11-13: animacja 3–5 kl./s, z akceleracją 61). Wymuszony renderer DMA-BUF działa z NVIDIA przez GBM
//! (`nvidia-drm.modeset=1`); na Waylandzie potrzebuje też `__NV_DISABLE_EXPLICIT_SYNC=1`, inaczej kompozytor zrywa połączenie
//! (błąd protokołu 71). Inne karty (Mesa: Intel, AMD, nouveau) i maszyny bez GPU – wybór zostaje przy WebKitGTK.
//!
//! Przy NVIDIA rysowanie Skia (rasteryzacja warstw) idzie na CPU – `WEBKIT_SKIA_ENABLE_CPU_RENDERING=1`; składanie warstw
//! zostaje na GPU. Rasteryzacja Skia na GPU wywraca proces strony w `libnvidia-eglcore` (zgłoszenie 2026-11-14, sterownik
//! 580, Quadro P400, WebKitGTK 2.52: segfault wątku `SkiaGPUWorker` po kilku sekundach albo przy zmianie rozmiaru okna –
//! pusta lub nieaktualna treść okna, utracone przełączenie serwera). Pomiar: 3 awarie w 3 uruchomieniach → 0, animacja
//! 61–62 kl./s bez zmian.
//!
//! `VIZRS_GPU` (w VizRS CG System także `--gpu`): `auto` (domyślnie; `on` – to samo), `off` – bez akceleracji (zepsuty sterownik),
//! `x11` – okno przez XWayland z akceleracją, `wayland` – nie przełączaj na XWayland przy skali ułamkowej,
//! `system` – nic nie zmieniaj.
//!
//! Skala ułamkowa KDE (`scale.rs`): w trybie `auto` sesja Wayland z `[Xwayland] Scale` niecałkowitym (np. 1.25) otwiera
//! okno przez XWayland – ostre piksele i bufor 1× zamiast ×2 (GTK 3 nie zna skali ułamkowej). Zmienne `WEBKIT_*` / `GDK_BACKEND` ustawione przez użytkownika mają zawsze pierwszeństwo.

/// Stan karty NVIDIA ze sterownikiem własnościowym (moduł `nvidia_drm`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nvidia {
    /// brak sterownika własnościowego NVIDIA
    None,
    /// `nvidia_drm` z `modeset=Y` i węzłem `/dev/dri/renderD*` – GBM dostępne
    Modeset,
    /// `nvidia_drm` bez `modeset` – bez GBM, renderer DMA-BUF nie ruszy
    NoModeset,
}

/// Zmienne do ustawienia i opis decyzji (do dziennika).
#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    pub set: Vec<(&'static str, &'static str)>,
    pub note: String,
}

const USER_KEYS: [&str; 5] = [
    "WEBKIT_DISABLE_COMPOSITING_MODE",
    "WEBKIT_DISABLE_DMABUF_RENDERER",
    "WEBKIT_FORCE_DMABUF_RENDERER",
    "WEBKIT_FORCE_COMPOSITING_MODE",
    "WEBKIT_DMABUF_RENDERER_FORCE_SHM",
];

/// Decyzja bez efektów ubocznych: `env` – odczyt zmiennej środowiska, `nv` – stan karty NVIDIA,
/// `xwl` – skala programów X11 w sesji KDE (`scale::kde_xwayland`).
pub fn decide(env: &dyn Fn(&str) -> Option<String>, nv: Nvidia, xwl: Option<f64>) -> Plan {
    let mode = env("VIZRS_GPU")
        .map(|v| v.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let mode = if mode.is_empty() {
        "auto".to_string()
    } else {
        mode
    };
    let none = |note: String| Plan {
        set: Vec::new(),
        note,
    };
    if mode == "system" {
        return none("VIZRS_GPU=system – ustawienia WebKitGTK bez zmian".into());
    }
    if let Some(k) = USER_KEYS.iter().find(|k| env(k).is_some()) {
        return none(format!("{k} ustawione przez użytkownika – bez zmian"));
    }
    if mode == "off" {
        return Plan {
            set: vec![("WEBKIT_DISABLE_COMPOSITING_MODE", "1")],
            note: "VIZRS_GPU=off – bez akceleracji".into(),
        };
    }
    if !["auto", "on", "x11", "wayland"].contains(&mode.as_str()) {
        return none(format!(
            "VIZRS_GPU={mode} – nieznana wartość (auto, on, off, x11, wayland, system), bez zmian"
        ));
    }
    // skala ułamkowa KDE: GTK 3 rysowałby ×2 i kompozytor zmniejszał – XWayland daje natywne piksele
    let frac = xwl.filter(|s| {
        mode != "wayland" && env("WAYLAND_DISPLAY").is_some() && crate::scale::fractional(*s)
    });
    let mut set = Vec::new();
    let mut pre = String::new();
    if (mode == "x11" || frac.is_some()) && env("GDK_BACKEND").is_none() {
        set.push(("GDK_BACKEND", "x11"));
        if let (Some(s), false) = (frac, mode == "x11") {
            pre = format!("skala ułamkowa {s} – okno przez XWayland; ");
        }
    }
    let plan = |set, note: &str| Plan {
        set,
        note: format!("{pre}{note}"),
    };
    match nv {
        Nvidia::Modeset => {
            set.push(("WEBKIT_FORCE_DMABUF_RENDERER", "1"));
            if env("__NV_DISABLE_EXPLICIT_SYNC").is_none() {
                set.push(("__NV_DISABLE_EXPLICIT_SYNC", "1"));
            }
            // rasteryzacja Skia na GPU wywraca proces strony w libnvidia-eglcore
            if env("WEBKIT_SKIA_ENABLE_CPU_RENDERING").is_none() {
                set.push(("WEBKIT_SKIA_ENABLE_CPU_RENDERING", "1"));
            }
            plan(
                set,
                "NVIDIA: akceleracja przez renderer DMA-BUF (GBM), rysowanie Skia na CPU",
            )
        }
        Nvidia::NoModeset => plan(
            set,
            "NVIDIA bez nvidia-drm.modeset=1 – WebKitGTK rysuje na CPU; włącz modeset w jądrze, żeby mieć akcelerację",
        ),
        Nvidia::None => plan(set, "akceleracja wg WebKitGTK"),
    }
}

/// Stan karty NVIDIA z `/sys` i `/dev/dri` (Linux).
pub fn nvidia() -> Nvidia {
    if !std::path::Path::new("/sys/module/nvidia_drm").exists() {
        return Nvidia::None;
    }
    // węzeł renderD* obsługiwany przez sterownik nvidia (GBM idzie przez niego)
    let render = std::fs::read_dir("/sys/class/drm")
        .map(|d| {
            d.flatten().any(|e| {
                e.file_name().to_string_lossy().starts_with("renderD")
                    && std::fs::read_link(e.path().join("device/driver"))
                        .map(|p| p.file_name().is_some_and(|n| n == "nvidia"))
                        .unwrap_or(false)
            })
        })
        .unwrap_or(false);
    // parametr bywa czytelny tylko dla roota (0400) – wtedy rozstrzyga węzeł renderD (sterownik ≥ 560 ma modeset domyślnie,
    // sesja Wayland na NVIDIA i tak go wymaga); jawne „N” wyłącza wymuszenie
    let modeset = std::fs::read_to_string("/sys/module/nvidia_drm/parameters/modeset")
        .map(|s| !s.trim().eq_ignore_ascii_case("n"))
        .unwrap_or(true);
    if modeset && render {
        Nvidia::Modeset
    } else {
        Nvidia::NoModeset
    }
}

/// Ustawia zmienne przed startem GTK / WebKitGTK – woła się na początku `main`, zanim powstaną wątki.
/// `mode` – tryb z wiersza poleceń (pierwszeństwo przed `VIZRS_GPU`). Zwraca opis decyzji (do dziennika).
/// Poza Linuksem nic nie robi.
pub fn prepare(mode: Option<&str>) -> String {
    if !cfg!(target_os = "linux") {
        return String::new();
    }
    let plan = decide(
        &|k| match (k, mode) {
            ("VIZRS_GPU", Some(m)) => Some(m.to_string()),
            _ => std::env::var(k).ok().filter(|v| !v.is_empty()),
        },
        nvidia(),
        crate::scale::kde_xwayland(),
    );
    for (k, v) in &plan.set {
        std::env::set_var(k, v);
    }
    if plan.set.is_empty() {
        plan.note
    } else {
        let vars: Vec<String> = plan.set.iter().map(|(k, v)| format!("{k}={v}")).collect();
        format!("{} ({})", plan.note, vars.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let v: Vec<(String, String)> = pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
        move |k| v.iter().find(|(a, _)| a == k).map(|(_, b)| b.clone())
    }

    #[test]
    fn skala_ulamkowa_kde_przelacza_na_xwayland() {
        let way = [("WAYLAND_DISPLAY", "wayland-0")];
        let p = decide(&env(&way), Nvidia::None, Some(1.25));
        assert_eq!(p.set, vec![("GDK_BACKEND", "x11")]);
        assert!(p.note.contains("1.25"));
        // skala całkowita, sesja X11, wybór użytkownika – bez przełączenia
        assert!(decide(&env(&way), Nvidia::None, Some(2.0)).set.is_empty());
        assert!(decide(&env(&[]), Nvidia::None, Some(1.25)).set.is_empty());
        let w = [("WAYLAND_DISPLAY", "wayland-0"), ("VIZRS_GPU", "wayland")];
        assert!(decide(&env(&w), Nvidia::None, Some(1.25)).set.is_empty());
        let g = [("WAYLAND_DISPLAY", "wayland-0"), ("GDK_BACKEND", "wayland")];
        assert!(decide(&env(&g), Nvidia::None, Some(1.25)).set.is_empty());
        let s = [("WAYLAND_DISPLAY", "wayland-0"), ("VIZRS_GPU", "system")];
        assert!(decide(&env(&s), Nvidia::None, Some(1.25)).set.is_empty());
        // NVIDIA: XWayland razem z rendererem DMA-BUF (pomiar: 18 kl./s zamiast 5)
        assert_eq!(decide(&env(&way), Nvidia::Modeset, Some(1.5)).set.len(), 4);
    }

    #[test]
    fn nvidia_modeset_wymusza_dmabuf_i_wylacza_explicit_sync() {
        let p = decide(&env(&[]), Nvidia::Modeset, None);
        assert_eq!(
            p.set,
            vec![
                ("WEBKIT_FORCE_DMABUF_RENDERER", "1"),
                ("__NV_DISABLE_EXPLICIT_SYNC", "1"),
                ("WEBKIT_SKIA_ENABLE_CPU_RENDERING", "1")
            ]
        );
    }

    #[test]
    fn nvidia_skia_na_cpu_chyba_ze_uzytkownik_wybral() {
        let p = decide(
            &env(&[("WEBKIT_SKIA_ENABLE_CPU_RENDERING", "0")]),
            Nvidia::Modeset,
            None,
        );
        assert!(!p
            .set
            .iter()
            .any(|(k, _)| *k == "WEBKIT_SKIA_ENABLE_CPU_RENDERING"));
        assert!(p.set.contains(&("WEBKIT_FORCE_DMABUF_RENDERER", "1")));
        // inne karty – Skia wg WebKitGTK
        assert!(decide(&env(&[]), Nvidia::None, None).set.is_empty());
    }

    #[test]
    fn inne_karty_i_nvidia_bez_modeset_bez_zmian() {
        assert!(decide(&env(&[]), Nvidia::None, None).set.is_empty());
        let p = decide(&env(&[]), Nvidia::NoModeset, None);
        assert!(p.set.is_empty() && p.note.contains("modeset"));
    }

    #[test]
    fn ustawienia_uzytkownika_maja_pierwszenstwo() {
        assert!(decide(
            &env(&[("WEBKIT_DISABLE_DMABUF_RENDERER", "1")]),
            Nvidia::Modeset,
            None
        )
        .set
        .is_empty());
        assert!(
            decide(&env(&[("VIZRS_GPU", "system")]), Nvidia::Modeset, None)
                .set
                .is_empty()
        );
        let p = decide(
            &env(&[("__NV_DISABLE_EXPLICIT_SYNC", "0")]),
            Nvidia::Modeset,
            None,
        );
        assert_eq!(
            p.set,
            vec![
                ("WEBKIT_FORCE_DMABUF_RENDERER", "1"),
                ("WEBKIT_SKIA_ENABLE_CPU_RENDERING", "1")
            ]
        );
    }

    #[test]
    fn vizrs_gpu_off_x11_i_nieznana_wartosc() {
        assert_eq!(
            decide(&env(&[("VIZRS_GPU", "off")]), Nvidia::Modeset, None).set,
            vec![("WEBKIT_DISABLE_COMPOSITING_MODE", "1")]
        );
        let p = decide(&env(&[("VIZRS_GPU", "X11")]), Nvidia::Modeset, None);
        assert_eq!(p.set[0], ("GDK_BACKEND", "x11"));
        assert_eq!(
            decide(
                &env(&[("VIZRS_GPU", "x11"), ("GDK_BACKEND", "wayland")]),
                Nvidia::None,
                None
            )
            .set,
            vec![]
        );
        assert_eq!(
            decide(&env(&[("VIZRS_GPU", "on")]), Nvidia::Modeset, None)
                .set
                .len(),
            3
        );
        assert!(
            decide(&env(&[("VIZRS_GPU", "turbo")]), Nvidia::Modeset, None)
                .set
                .is_empty()
        );
    }
}
