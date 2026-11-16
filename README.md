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
  być pozycjami ramówki i EPG ([DOKUMENTACJA.md §1](docs/DOKUMENTACJA.md#1-jak-to-działa)).
- **Live API** (`:7700/api/live/*`) dla **panelu kanału** – stan sygnałów studia,
  ZDEJMIJ / WPUŚĆ, audycje LIVE w ramówce ErsatzTV i reguły grafiki; stanowisko
  operatora to panel kanału (dawny VizRS Live, katalog `panel/`), sidecar nie ma
  własnego panelu ([DOKUMENTACJA.md §1](docs/DOKUMENTACJA.md#1-jak-to-działa), [API.md §1](docs/API.md)).

Sidecar to jeden plik binarny w Ruście (`vizrs-cg`) obok bezgłowego Chromium;
wydania: obraz `ghcr.io/sdorpl/vizrs-cg` (amd64, arm64) i archiwa dla Linuksa
x86_64 i arm64 ([DOKUMENTACJA.md §4](docs/DOKUMENTACJA.md#4-uruchamianie-w-dockerze), [§7](docs/DOKUMENTACJA.md#7-testy)).

## Pakiet VizRS

VizRS CG Playout to jedna z aplikacji pakietu VizRS, które mają być ze sobą **zawsze zgodne**: [VizRS CG System](../VizRS%20CG%20System) (silnik grafiki, Trio, `/output`), **VizRS CG Playout** (to repo – sidecar oraz panel kanału w `panel/`, dawny VizRS Live) i [VizRS CG Artist](../VizRS%20CG%20Artist) (edytor grafik i animacji). Styki i zasady zgodności: [DOKUMENTACJA §1](docs/DOKUMENTACJA.md#pakiet-vizrs--zgodność-aplikacji).

| Repozytorium | Rola |
|---|---|
| [VizRS CG System](../VizRS%20CG%20System) | silnik grafiki: Trio (playout stron), `/output`, API TCP 6100 / HTTP / SSE |
| **VizRS CG Playout** (to repo) | integracja VizRS dla kanałów ErsatzTV (sidecar) oraz panel kanału (`panel/`) nad ErsatzTV legacy |
| [VizRS CG Artist](../VizRS%20CG%20Artist) | samodzielny edytor grafik i animacji (Tauri 2), sceny dla VizRS CG System |

Z VizRS CG System aplikację łączy tylko protokół VizRS (TCP 6100, `/output`), opisany w
[API.md](../VizRS%20CG%20System/docs/API.md) systemu VizRS; panel korzysta z
Live API sidecara (`/api/live/*`, [DOKUMENTACJA.md §1](docs/DOKUMENTACJA.md#1-jak-to-działa), [API.md §1](docs/API.md)).

## Szybki start

```sh
# Stos z grafiką VizRS:
COMPOSE_PROFILES=grafika docker compose up -d

# Z panelem kanału:
COMPOSE_PROFILES=grafika docker compose -f docker-compose.yml -f docker-compose.live.yml up -d

# Bez grafiki (sam ErsatzTV z panelem):
docker compose -f docker-compose.yml -f docker-compose.live.yml up -d
```

Profile compose w `docker-compose.yml`:
- bez profilu: sam ErsatzTV (`seed` + `ersatztv`), bez grafiki VizRS;
- profil `grafika`: dochodzi silnik VizRS (`:8788`, Trio pod `/trio`) i sidecar (`:7700/health`);
- profil `ingest` (wymaga `grafika`): usługa `ingest` (RTMP/SRT) – `COMPOSE_PROFILES=grafika,ingest`.

W Portainerze profil włącza zmienna środowiskowa stosu `COMPOSE_PROFILES`. Pierwszy start wypełnia puste wolumeny danymi z `seed/`:
show VizRS i kanał 1 ErsatzTV z grafiką VizRS na odcinkach testowych.
Kolejne kanały: element grafiki **VizRS** w pozycji harmonogramu albo w **Deco**
kanału, potem reset playoutu. Przed otwarciem portów zmień `VIZRS_TOKEN`
i `VIZRS_OUTPUT_TOKEN`.

## Dokumentacja

| Dokument | Treść |
|---|---|
| [docs/DOKUMENTACJA.md](docs/DOKUMENTACJA.md) | pełny opis techniczny i architektura: sidecar (§1–§8) oraz panel kanału (§9–§15) |
| [docs/API.md](docs/API.md) | specyfikacja interfejsów HTTP: Live API sidecara (§1), API panelu (§2), VizRS Bridge (§3) |
| [docs/INSTRUKCJA.md](docs/INSTRUKCJA.md) | codzienna praca operatora kanału |
| [docs/RUNBOOK.md](docs/RUNBOOK.md) | runbook administratora: wdrożenie, Docker/Portainer, akceleracja sprzętowa, kopie, awarie, wydania |
| [docs/STATUS.md](docs/STATUS.md) | stan prac, połączenie z VizRS Live i plan najbliższych kroków |
| [docs/LUKI.md](docs/LUKI.md) | znane braki i ograniczenia uszeregowane G1 → G2 → G3 |
| [CHANGELOG.md](CHANGELOG.md) | historia wydań produktu oraz historia wydań panelu |
| [docs/plany/](docs/plany/README.md) | plany rozwoju ([PLAN-mcr](docs/plany/PLAN-mcr.md)) i archiwum zakończonych planów |
| [CLAUDE.md](CLAUDE.md) | zasady pracy z repozytorium dla programistów i agentów |
| [docs/AGENCI.md](docs/AGENCI.md) | podział ról i współpraca Claude Code i Gemini (Antigravity CLI) w pakiecie VizRS |

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
- **Okno panelu** (`panel/desktop/`, Tauri 2, AppImage): natywny klient panelu na serwerze – łączy się z panelem (np. przez proxy z TLS i domeną na serwerze produkcyjnym) za pomocą tokenu (`VIZRS_LIVE_TOKEN`).
- Wdrożenie: jedna nakładka `docker-compose.live.yml` (Portainer i docker compose) oraz `docker-compose.yml` z profilami. Lokalna budowa: `panel/compose.build.yml`. Dokumentacja: [docs/DOKUMENTACJA.md](docs/DOKUMENTACJA.md) (§9–§15), [docs/INSTRUKCJA.md](docs/INSTRUKCJA.md), [docs/RUNBOOK.md](docs/RUNBOOK.md).

## Licencja

MIT – [LICENSE](LICENSE). Składniki zewnętrzne (fonty, biblioteki, obrazy): [THIRD-PARTY.md](THIRD-PARTY.md). Opis całego pakietu VizRS: [sdorpl/VizRS](https://github.com/sdorpl/VizRS). Współpracuje z [ErsatzTV](https://ersatztv.org) (legacy: Zlib, Next: MIT) bez zmian w jego kodzie – powered by ErsatzTV.
