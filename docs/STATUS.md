# STATUS – stan prac

> Aktualizuj w tym samym commicie, który zmienia stan. Plan: [plany/PLAN-mcr.md](plany/PLAN-mcr.md), znane braki: [LUKI.md](LUKI.md), opis działania: [DOKUMENTACJA.md](DOKUMENTACJA.md), specyfikacja API: [API.md](API.md), zmiany wydań: [CHANGELOG.md](../CHANGELOG.md).
>
> **Ostatnia aktualizacja:** 2026-11-16

## Bieżący etap

**Połączenie z VizRS Live** – P1 (kod panelu, mostu i aplikacji okiennej w katalogu `panel/`) oraz P2 (jeden stos `docker-compose.yml` z profilami compose, nakładka `docker-compose.live.yml` i jedno wydanie w `.github/workflows/release.yml`) zrobione na gałęzi `polaczenie-live`. Wersja 26.11.11 przygotowana (niewydana). P4 dokumentacja w toku.

Wydanie 26.11.10 (2026-11-14): logo na przejęcie kanału przez live ingest (`on_live`), grafika ramówki wstrzymana na czas przejęcia. Stosy s1 i s2 (Portainer GitOps, `.release`) – obrazy `vizrs-cg` 26.11.10 i `vizrs` z wydań VizRS CG System.

## Następne

1. **P3 – przełączenie produkcji i wydanie:**
   - w Portainerze na stosach s1 i s2 zmienić `COMPOSE_PROFILES=ingest` na `grafika,ingest` (bezpieczne też ze starym compose),
   - scalenie gałęzi `polaczenie-live` do `main`,
   - tag `v26.11.11` (najpierw próbny `v26.11.11-test.1`),
   - obrazy panelu w nakładce idą z `:latest` (decyzja 2026-11-16): scalenie przeniesie s1/s2 z panelu 26.11.8 na 26.11.10 (`:latest` z repo VizRS Live; ta sama warstwa legacy co 26.11.8, bez migracji bazy), a wydanie 26.11.11 – na 26.11.11 przez `stack-refresh`; przed wydaniem kopia `etv-config` (obraz legacy z mostem budowany na najnowszym legacy).
2. **P4 – domknięcie połączenia repozytoriów:**
   - archiwizacja repo `sdorpl/VizRS-Live-Playout`,
   - linki w VizRS CG System i Artist,
   - zmiana nazwy katalogu `VirtRS CG Playout` → `VizRS CG Playout`.

## Panel – zrobione (wg planu)

**Grafika i sloty na żywo** – przez **VizRS CG Playout** (sidecar w tym repo): reguły grafiki (`/api/live/graphics`), stan wejść, przejęcie kanału, audycje LIVE. Logika zostaje w CG Playout; panel woła jego API przez pośrednika `/api/cg/…`.

| Punkt | Zakres | Gdzie |
|---|---|---|
| R0 | fundament UI: moduły, formularz z metadanych, wspólne edytory | `panel/server/web/{core,form,parts}.js` |
| R1–R2 | harmonogramy z pozycjami; playouty wszystkich rodzajów, harmonogramy zastępcze | `views/harmonogramy.js`, `views/playouty.js` |
| R3–R4 | biblioteki lokalne, skan, przeglądarka, kosz; kolekcje (4 rodzaje) i playlisty | `views/media.js`, `views/listy.js`, `views/playlisty.js` |
| R5 | profile kodowania, znaki wodne (wgrywanie obrazu – jedyny nowy endpoint mostu), wypełniacze, logo kanału | `views/{profile,znaki,wypelniacze}.js`, `bridge/` `/upload` |
| R6 | bloki, szablony dnia, deco, kalendarz tygodnia playoutu blokowego | `views/{bloki,deco,kalendarz}.js`, `when.js` |
| R7 | ustawienia legacy, logi, konserwacja, test odtwarzania | `views/system.js` |
| R8 | Jellyfin, Emby, Plex (PIN), strumienie zdalne; sekrety tylko do zapisu | `views/zrodla.js`, `mask_secrets` |
| R9 | grafika VizRS i sloty na żywo (pośrednik CG Playout) | `views/grafika.js`, `/api/cg/…` |
| R10 | web GUI legacy zamknięte za proxy Caddy, lista pokrycia, legacy v26.12.0, CI | `panel/docker/proxy.Caddyfile`, `.github/workflows/` |
| R11 | wydanie 1.0: instrukcja operatora, runbook, obrazy ghcr, przegląd bezpieczeństwa | `docs/INSTRUKCJA.md`, `docs/RUNBOOK.md`, `release.yml` |
| R12 | Sloty na żywo (MCR), Audycje LIVE, operator i studio w pasku górnym | `views/live.js`, `app.js` |

## Panel – historia wydań

### Wydanie 26.11.10 (2026-11-14, gałąź `main`)

Aplikacja VizRS Live (okno): przy NVIDIA rysowanie Skia na CPU – rasteryzacja na GPU wywracała proces strony (puste okno); po awarii procesu strony okno wczytuje stronę od nowa (`panel/desktop/src/gpu.rs`, `main.rs` – jak VizRS CG Artist 26.11.8 i okno Trio). Panel (`panel/server/`) bez zmian względem 26.11.9.

### Wydanie 26.11.9 (2026-11-14)

Aplikacja VizRS Live (okno): na KDE Plasma Wayland przy niecałkowitej skali okno otwiera się przez XWayland – ostre piksele zamiast bufora ×2 zmniejszanego przez kompozytor (`panel/desktop/src/scale.rs`, `gpu.rs` – jak VizRS CG Artist 26.11.7); pierwszy AppImage aplikacji w wydaniu. Panel (`panel/server/`) bez zmian względem 26.11.8.

### Wydanie 26.11.8 (2026-11-14)

26.11.7 nieopublikowane (bramka wydania: test e2e wybierał pole w zwiniętej karcie reguły kanału). Grafika na przejęcie kanału przez live ingest: w regule kanału logo (`on_live`, np. strona 7510 – NA ŻYWO na BTB), grafika ramówki wstrzymana na czas przejęcia; wiersz „Grafika” w Slotach na żywo, nowe zdarzenia w Dzienniku emisji – z CG Playout 26.11.10.

Wcześniej – 26.11.6 (2026-11-14): Dziennik emisji jako centralne miejsce logów (live ingest, przejęcia, sidecar CG – z CG Playout 26.11.9), aplikacja `desktop/`: akceleracja WebKitGTK na NVIDIA i rozmiar okna przy skalowaniu ułamkowym, nakładka CG Playout z `MEDIA_MOUNT`.

Wcześniej – 26.11.5 (2026-11-13): 26.11.2–26.11.4 nieopublikowane (bramka wydania: wyścig routera; zamierzony 404 miniatury liczony jako błąd testu; chwiejny test podglądu i nieaktualny `desktop/Cargo.lock`). Powiększenie miniatury po najechaniu. Aplikacja na Tauri 2 (`desktop/`), wspólny rdzeń wyglądu `vizrs-ui.css`, profile kodowania bez akceleracji, karta pozycji na telefonie, wybór znaków wodnych i elementów graficznych z wyszukiwarką i miniaturami (strony VizRS – z CG Playout 26.11.4). Szczegóły: CHANGELOG.

### Wydanie 26.11.1 (2026-11-12, gałąź `main`)

Pierwsze wydanie w numeracji rok.miesiąc.rewizja: zawartość 1.3.0-test.1 i późniejszych zmian z `next-v`; poprzednia `main` → `main-oldstable`.

### PLAN-mcr – wydanie testowe 1.3.0-test.1 (2026-11-10, gałąź `next-v`)

Zrobione w panelu: K1–K11, K18 (sesje, CSP, numeracja kanałów, rozdzielczości, silnik Next, wyniki zadań w tle, historia bloków, biblioteki, kontrola ramówki, mozaika, import ramówki, HEVC/AC-3). K12–K14 i K17 – strona panelu gotowa, włącza się po `features` z VizRS CG Playout; K15–K16 – kontrakt API ([DOKUMENTACJA.md §11](DOKUMENTACJA.md#11-panel-vizrs-live) oraz [API.md §2](API.md)). Zrzuty bez legacy: atrapa mostu `panel/tests/mock-bridge.cjs` (`MOCK_CG=1 MOCK_HLS=1`). e2e nowych punktów – do sprawdzenia w CI (bramka `release.yml` przed publikacją obrazów).

**Kroki ręczne po tym wydaniu:** (1) VizRS CG Playout – zaimplementować kontrakt K12–K17 (`features`, `layers`, `take/continue/out`, `asrun`, `VIZRS_ALERT_WEBHOOK`, potem K15/K16) i wydać razem z panelem; (2) Q1 z użytkownikiem; (3) po zielonej bramce i teście na stosie `virtrs` – scalenie `next-v` do `main` i wydanie 1.3.0.

**Jeden przebieg** luk i planu rozwoju: [PLAN-mcr.md](plany/PLAN-mcr.md) §4 (K1–K18 + Q1 ręcznie), realizacja na gałęzi **`next-v`**. Stan: przebieg wykonany do 1.3.0-test.1 (wyżej); następne – kontrakt CG Playout (K12–K17) i Q1. Decyzja użytkownika 2026-11-09: **zostajemy przy ErsatzTV legacy** (wariant A, PLAN-mcr §6). Punkty K12–K17 wymagają zmian w API VizRS CG Playout.

## Panel – kroki ręczne (czekają na użytkownika)

Sesje automatu niczego nie publikują i nie ruszają produkcji; to zostało do zrobienia ręcznie:

- Wydanie 1.2.3: tag `v1.2.3` wypchnięty 2026-11-08 (razem z VizRS CG Playout v2.12.0 – wejścia studia z panelu; s1 przez stack-refresh); pakiety ghcr publiczne.
- Lokalna produkcja (stos `virtrs` CG Playout) przełączona 2026-11-07 na VizRS Live nakładką `compose.cg-playout.yml` ([RUNBOOK.md §2a](RUNBOOK.md#2a-wdrożenie-w-portainerze-gitops-i-konfiguracja-proxy)): legacy v26.11.6 → v26.12.0 z mostem (migracja bazy; kopia w `~/vizrs-kopie/`), proxy na 8409, panel na 8090, CG Playout z live API. Do sprawdzenia: tunery i odtwarzacze, które korzystały z innych ścieżek niż `/iptv/*` (`curl -I`, [DOKUMENTACJA.md §15](DOKUMENTACJA.md#15-zamknięcie-web-gui-legacy-r10)).
- Produkcja (stos `virtrs`) po poprawce integracji z VizRS (Trio, podgląd `/output`): przebudować obraz panelu (`vizrs-live:local`), w `.env` CG Playout usunąć `VIZRS_LIVE_VIZRS_URL=http://127.0.0.1:8789` (nakładka ustawi `:8789` = host panelu, token wyjścia i anteny kanałów), `docker compose up -d live`; sprawdzić ramkę w Grafice kanału i „Trio ↗”.
- ~~Produkcja (stos `virtrs`) – legacy zawsze najnowsze~~ – zrobione 2026-11-07 21:15: kopia `~/vizrs-kopie/etv-config-2026-11-07-2114-przed-legacy-latest.tgz` i `.env`, `LIVE_LEGACY_IMAGE` usunięte z `.env` CG Playout, `ersatztv` na `vizrs-live-legacy:local` (najnowsze legacy = v26.12.0, most 1.2.0, `commandsMissing = []`, NVIDIA przez CDI, `panel/tests/hwaccel.sh` zielony).
- Przed wdrożeniem z grafiką: `panel_token` w CG Playout, `VIZRS_LIVE_CG_URL`/`VIZRS_LIVE_CG_TOKEN` w panelu, Sloty na żywo z prawdziwym sygnałem z OBS (LUKI #3).

## Panel – blokery / uwagi

- e2e wymaga Playwright (`PLAYWRIGHT_DIR` albo `panel/tests/local.env`) i Dockera (stos testowy).
- Prawdziwe usługi (`panel/tests/real.sh`) sprawdza się ręcznie; przed `panel/tests/stack.sh test` zawsze `panel/tests/real.sh down` (inaczej liczby pozycji w e2e się nie zgadzają).
