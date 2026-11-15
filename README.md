# VizRS CG Playout — grafika VizRS na kanałach ErsatzTV

> Do 2026-10-31 projekt nazywał się **VirtRS CG Playout** (repozytorium `sdorpl/VirtRS-CG-Playout`, GitHub przekierowuje stary adres). Nazwy techniczne bez zmian – binarka `vizrs-cg`, obraz `ghcr.io/sdorpl/vizrs-cg`; zmienne środowiskowe `VIZRS_*`.

Część rodziny **VizRS**: grafika z [VizRS CG System](../VizRS%20CG%20System)
na kanałach [ErsatzTV](https://ersatztv.org). Mały serwis pomocniczy (sidecar)
obok niezmienionego ErsatzTV:

- **Komendy na start klipu** – gdy zaczyna się pozycja playoutu, do VizRS idą
  komendy TCP (`TAKE`, `UPDATE`, `OUT`, `MACRO`, …) z danymi klipu
  i następnego programu (`{title}`, `{next_title}`, `{next_start}`, …).
- **Reklamy bez grafiki** – na reklamie wszystkie warstwy na antenie schodzą;
  strony wracają jednym `TAKE` na pierwszym zwykłym klipie.
- **Wtapianie (burn-in)** – strona `/output` VizRS renderowana bezgłowym
  Chromium (bez GPU) i nakładana na obraz w ffmpeg ErsatzTV (nakładka
  ffmpeg) albo przez element grafiki ErsatzTV.
- **Live ingest** – sygnał RTMP/SRT przejmuje kanał w miejsce harmonogramu
  (z grafiką VizRS), a po jego końcu program wraca; audycje na żywo mogą też
  być pozycjami ramówki i EPG (DOKUMENTACJA §1).
- **Live API** (`:7700/api/live/*`) dla **VizRS Live** – stan sygnałów studia,
  ZDEJMIJ / WPUŚĆ, audycje LIVE w ramówce ErsatzTV i reguły grafiki; stanowisko
  operatora to VizRS Live, sidecar nie ma własnego panelu (DOKUMENTACJA §1).

Sidecar to jeden plik binarny w Ruście (`vizrs-cg`) obok bezgłowego Chromium;
wydania: obraz `ghcr.io/sdorpl/vizrs-cg` (amd64, arm64) i archiwa dla Linuksa
x86_64 i arm64 (DOKUMENTACJA §4, §7).

## Pakiet VizRS

VizRS CG Playout to jedna z czterech aplikacji pakietu VizRS, które mają być ze sobą **zawsze zgodne**: [VizRS CG System](../VizRS%20CG%20System) (silnik grafiki, Trio, `/output`), **VizRS CG Playout** (to repo), [VizRS Live](../VizRS%20Live) (panel kanału nad ErsatzTV legacy) i [VizRS CG Artist](../VizRS%20CG%20Artist) (edytor grafik i animacji). Styki i zasady zgodności: [DOKUMENTACJA §1](docs/DOKUMENTACJA.md#pakiet-vizrs--zgodność-aplikacji).

| Repozytorium | Rola |
|---|---|
| [VizRS CG System](../VizRS%20CG%20System) | silnik grafiki: Trio (playout stron), `/output`, API TCP 6100 / HTTP / SSE |
| **VizRS CG Playout** (to repo) | ta sama integracja VizRS dla kanałów ErsatzTV, bez zmian w ErsatzTV |
| [VizRS Live](../VizRS%20Live) | panel kanału nad ErsatzTV legacy (sterowanie kanałami, ramówką, playoutami bez web GUI legacy) |
| [VizRS CG Artist](../VizRS%20CG%20Artist) | samodzielny edytor grafik i animacji (Tauri 2), sceny dla VizRS CG System |

Z VizRS CG System aplikację łączy tylko protokół VizRS (TCP 6100, `/output`), opisany w
[API.md](../VizRS%20CG%20System/docs/API.md) systemu VizRS; VizRS Live korzysta z
Live API (`/api/live/*`, DOKUMENTACJA §1).

## Szybki start

```sh
docker compose up -d
```

uruchamia VizRS (`:8788`, Trio pod `/trio`), sidecar (`:7700/health`)
i ErsatzTV (`:8409`). Pierwszy start wypełnia puste wolumeny danymi z `seed/`:
show VizRS i kanał 1 ErsatzTV z grafiką VizRS na odcinkach testowych.
Kolejne kanały: element grafiki **VizRS** w pozycji harmonogramu albo w **Deco**
kanału, potem reset playoutu. Przed otwarciem portów zmień `VIZRS_TOKEN`
i `VIZRS_OUTPUT_TOKEN`.

## Dokumentacja

| Dokument | Treść |
|---|---|
| [docs/DOKUMENTACJA.md](docs/DOKUMENTACJA.md) | działanie, konfiguracja, symbole zastępcze, Docker (Ampere A1, NVENC, VAAPI), awarie, testy, benchmark przechwytywania grafiki (Portainer) |
| [CLAUDE.md](CLAUDE.md) | dla programistów i agentów: zasady, struktura, testy, commity |

## Panel (`panel/`, dawny VizRS Live)

Od 2026-11-15 w tym repo jest też **panel kanału** – do tej pory osobne repo [VizRS-Live-Playout](https://github.com/sdorpl/VizRS-Live-Playout) (plan połączenia: [sdorpl/VizRS](https://github.com/sdorpl/VizRS) §3). Polski panel nad **ErsatzTV legacy**: kanały, ramówka, playouty, media, EPG, mozaika, alarmy, dziennik emisji, grafika kanału i sloty na żywo – bez web GUI ErsatzTV.

```
 przeglądarka / okno ──► vizrs-live (Rust, :8090) ──► ErsatzTV legacy (sieć wewnętrzna Dockera)
                         UI panelu + pośrednik          └─ VizRS Bridge (C#, w procesie legacy)
                               └─ /api/cg/* ──► sidecar `cg` (:7700, Live API)
 widzowie ──► proxy (Caddy, :8409) ──► legacy: tylko /iptv/*, /artwork/*, HDHomeRun (reszta 403)
```

- **VizRS Bridge** (`panel/bridge/`, C#): wtyczka ładowana do niezmienionego legacy (`DOTNET_STARTUP_HOOKS`), dyspozytor komend MediatR z białą listą `panel/bridge/commands.txt`.
- **vizrs-live** (`panel/server/`, Rust bez zależności): UI panelu i pośrednik do mostu i Live API sidecara; tokeny nie trafiają do przeglądarki.
- **Okno panelu** (`panel/desktop/`, Tauri 2, AppImage): natywny klient panelu na serwerze – adres panelu (np. przez proxy z TLS) i token.
- Wdrożenie: nakładka `docker-compose.live.yml` (Portainer) albo `panel/compose.cg-playout.yml`. Dokumentacja panelu: [docs/panel/](docs/panel/README.md) (DOKUMENTACJA, INSTRUKCJA operatora, RUNBOOK).

## Licencja

MIT – [LICENSE](LICENSE). Składniki zewnętrzne (fonty, biblioteki, obrazy): [THIRD-PARTY.md](THIRD-PARTY.md). Opis całego pakietu VizRS: [sdorpl/VizRS](https://github.com/sdorpl/VizRS). Współpracuje z [ErsatzTV](https://ersatztv.org) (legacy: Zlib, Next: MIT) bez zmian w jego kodzie – powered by ErsatzTV.
