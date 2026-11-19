//! Konfiguracja panelu: opcje CLI i zmienne środowiskowe, pomoc, sprawdzenie adresu VizRS.

use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub bind: String,
    pub port: u16,
    pub bridge_url: String,
    pub bridge_token: String,
    pub token: Option<String>,
    pub stream_url: String,
    pub vizrs_url: String,
    /// token wyjścia VizRS (tylko odczyt, jak w OBS) – trafia do przeglądarki w adresie podglądu /output
    pub vizrs_output_token: String,
    /// VizRS z antenami kanałów (CG Playout `channel_buses`): podgląd `bus=ch-N`, Trio `?channel=N`
    pub vizrs_channels: bool,
    pub cg_url: String,
    pub cg_token: String,
    pub web: Option<PathBuf>,
    /// najwyżej tyle podglądów w mozaice Na antenie (K10; każdy to sesja Next, która koduje); 0 = bez mozaiki
    pub multiview_max: u32,
    /// katalog danych panelu (konta i sesje); `None` = bez kont, logowanie tylko tokenem
    pub data: Option<PathBuf>,
}

pub(crate) const HELP: &str = "vizrs-live – panel kanału nad ErsatzTV legacy

Użycie: vizrs-live [opcje]

  --bind ADRES          adres nasłuchu (VIZRS_LIVE_BIND, domyślnie 127.0.0.1)
  --port N              port panelu (VIZRS_LIVE_PORT, domyślnie 8090)
  --bridge URL          ErsatzTV legacy z mostem (VIZRS_LIVE_BRIDGE, domyślnie http://127.0.0.1:8409)
  --bridge-token T      token mostu, = VIZRS_BRIDGE_TOKEN w legacy (VIZRS_LIVE_BRIDGE_TOKEN, wymagany)
  --token T             token awaryjny panelu: skrypty (Bearer) i pierwsze konto (VIZRS_LIVE_TOKEN)
  --data KATALOG        konta operatorów i sesje (VIZRS_LIVE_DATA; w obrazie /data); poza 127.0.0.1
                        wymagany token albo katalog danych
  --stream-url URL      publiczny adres strumieni legacy do odnośników (VIZRS_LIVE_STREAM_URL)
  --vizrs-url URL       VizRS CG System widziany z przeglądarki: podgląd /output i odnośnik do Trio;
                        sam port (:8788) = ten sam host co panel (VIZRS_LIVE_VIZRS_URL)
  --vizrs-output-token T  token wyjścia VizRS (tylko odczyt) do podglądu /output (VIZRS_LIVE_VIZRS_OUTPUT_TOKEN)
  --vizrs-channels      VizRS z antenami kanałów, jak channel_buses w CG Playout (VIZRS_LIVE_VIZRS_CHANNELS=1)
  --cg-url URL          VizRS CG Playout (sidecar, np. http://cg:7700) – grafika i sloty na żywo (VIZRS_LIVE_CG_URL)
  --cg-token T          token panelu CG Playout, = panel_token / VIZRS_PANEL_TOKEN tam (VIZRS_LIVE_CG_TOKEN)
  --multiview-max N     najwyżej tyle podglądów w mozaice Na antenie, 0–16, 0 = bez mozaiki
                        (VIZRS_LIVE_MULTIVIEW_MAX, domyślnie 4; każdy podgląd to sesja Next, która koduje)
  --web KATALOG         UI z dysku zamiast wbudowanego (praca nad UI)
  --healthcheck         sprawdź działający panel (GET /health), kod wyjścia 0/1
  --version, --help
";

/// Opcje z argumentów i zmiennych środowiskowych (argumenty mają pierwszeństwo).
pub fn parse_config(
    args: &[String],
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<Config, String> {
    let mut c = Config {
        bind: env("VIZRS_LIVE_BIND").unwrap_or_else(|| "127.0.0.1".into()),
        port: match env("VIZRS_LIVE_PORT") {
            Some(p) => p.parse().map_err(|_| format!("zły VIZRS_LIVE_PORT: {p}"))?,
            None => 8090,
        },
        bridge_url: env("VIZRS_LIVE_BRIDGE").unwrap_or_else(|| "http://127.0.0.1:8409".into()),
        bridge_token: env("VIZRS_LIVE_BRIDGE_TOKEN").unwrap_or_default(),
        token: env("VIZRS_LIVE_TOKEN").filter(|t| !t.is_empty()),
        stream_url: env("VIZRS_LIVE_STREAM_URL").unwrap_or_default(),
        vizrs_url: env("VIZRS_LIVE_VIZRS_URL").unwrap_or_default(),
        vizrs_output_token: env("VIZRS_LIVE_VIZRS_OUTPUT_TOKEN").unwrap_or_default(),
        vizrs_channels: matches!(
            env("VIZRS_LIVE_VIZRS_CHANNELS").as_deref(),
            Some("1" | "true" | "yes")
        ),
        cg_url: env("VIZRS_LIVE_CG_URL").unwrap_or_default(),
        cg_token: env("VIZRS_LIVE_CG_TOKEN").unwrap_or_default(),
        web: None,
        multiview_max: match env("VIZRS_LIVE_MULTIVIEW_MAX") {
            Some(v) => v
                .parse()
                .map_err(|_| format!("zły VIZRS_LIVE_MULTIVIEW_MAX: {v}"))?,
            None => 4,
        },
        data: env("VIZRS_LIVE_DATA")
            .filter(|d| !d.is_empty())
            .map(PathBuf::from),
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = || {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{a}: brak wartości"))
        };
        match a.as_str() {
            "--bind" => c.bind = val()?,
            "--port" => {
                c.port = val()?
                    .parse()
                    .map_err(|_| "--port: zła liczba".to_string())?
            }
            "--bridge" => c.bridge_url = val()?,
            "--bridge-token" => c.bridge_token = val()?,
            "--token" => c.token = Some(val()?).filter(|t| !t.is_empty()),
            "--stream-url" => c.stream_url = val()?,
            "--vizrs-url" => c.vizrs_url = val()?,
            "--vizrs-output-token" => c.vizrs_output_token = val()?,
            "--vizrs-channels" => c.vizrs_channels = true,
            "--cg-url" => c.cg_url = val()?,
            "--cg-token" => c.cg_token = val()?,
            "--web" => c.web = Some(PathBuf::from(val()?)),
            "--data" => c.data = Some(PathBuf::from(val()?)).filter(|d| !d.as_os_str().is_empty()),
            "--multiview-max" => {
                c.multiview_max = val()?
                    .parse()
                    .map_err(|_| "--multiview-max: zła liczba".to_string())?
            }
            other => return Err(format!("nieznana opcja: {other} (--help)")),
        }
    }
    if c.multiview_max > MULTIVIEW_LIMIT {
        return Err(format!(
            "--multiview-max: najwyżej {MULTIVIEW_LIMIT} (każdy podgląd mozaiki to sesja Next, która koduje)"
        ));
    }
    bridge::parse_url(&c.bridge_url)?;
    if !c.cg_url.is_empty() {
        bridge::parse_url(&c.cg_url).map_err(|e| format!("--cg-url: {e}"))?;
        if c.cg_token.is_empty() || c.cg_token.contains(['\r', '\n']) {
            return Err(
                "--cg-url wymaga tokenu panelu CG Playout: --cg-token / VIZRS_LIVE_CG_TOKEN".into(),
            );
        }
    }
    if c.bridge_token.len() < 16 {
        return Err("brak tokenu mostu: --bridge-token / VIZRS_LIVE_BRIDGE_TOKEN (min. 16 znaków, ten sam co VIZRS_BRIDGE_TOKEN w legacy)".into());
    }
    if let Some(t) = &c.token {
        if t.len() < 16
            || !t
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || "-._~".contains(ch))
        {
            return Err("token panelu: min. 16 znaków, tylko litery, cyfry i -._~".into());
        }
    }
    let loopback = c
        .bind
        .parse::<IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(c.bind == "localhost");
    if !loopback && c.token.is_none() && c.data.is_none() {
        return Err(format!(
            "panel na {} wymaga kont (--data / VIZRS_LIVE_DATA) albo tokenu (--token / VIZRS_LIVE_TOKEN)",
            c.bind
        ));
    }
    for u in [&mut c.stream_url, &mut c.vizrs_url, &mut c.cg_url] {
        while u.ends_with('/') {
            u.pop();
        }
    }
    check_vizrs(&c)?;
    Ok(c)
}

/// Adres VizRS trafia do przeglądarki (ramka, odnośnik): tylko `http(s)://…` albo sam port `:N`;
/// token wyjścia – znaki tokenu VizRS (bezpieczne w adresie).
pub(crate) fn check_vizrs(c: &Config) -> Result<(), String> {
    let u = &c.vizrs_url;
    let port_only = u
        .strip_prefix(':')
        .is_some_and(|p| p.parse::<u16>().is_ok_and(|n| n > 0));
    let absolute = (u.starts_with("http://") || u.starts_with("https://"))
        && !u
            .chars()
            .any(|ch| ch.is_whitespace() || "\"'<>?#".contains(ch));
    if !(u.is_empty() || port_only || absolute) {
        return Err(format!(
            "--vizrs-url: oczekiwany http(s)://host:port albo sam port (:8788), jest {u}"
        ));
    }
    let t = &c.vizrs_output_token;
    if t.len() > 128
        || !t
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "-_.".contains(ch))
    {
        return Err("--vizrs-output-token: tylko litery, cyfry i -_. (jak token VizRS)".into());
    }
    if u.is_empty() && (!t.is_empty() || c.vizrs_channels) {
        return Err("--vizrs-output-token i --vizrs-channels wymagają --vizrs-url".into());
    }
    Ok(())
}
