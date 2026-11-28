# STATUS – stan prac

> Aktualizuj w tym samym commicie, który zmienia stan. Plan: [plany/PLAN-mcr.md](plany/PLAN-mcr.md), znane braki: [LUKI.md](LUKI.md), opis działania: [DOKUMENTACJA.md](DOKUMENTACJA.md), specyfikacja API: [API.md](API.md), zmiany wydań: [CHANGELOG.md](../CHANGELOG.md).
>
> **Ostatnia aktualizacja:** 2026-11-28

## Bieżący etap

**Wydanie 26.11.35** (2026-11-27) na stosach s1, s2 i s3 (Portainer, obrazy `:latest`; s3 – stos `vizplay-s3` od 2026-11-21). Sidecar i panel wychodzą razem (jedna wersja `vizrs-cg` = `vizrs-live`, wydanie z bramką CI od 26.11.32); wersje sąsiadów – `ZGODNOSC.md` w [sdorpl/VizRS](https://github.com/sdorpl/VizRS/blob/main/ZGODNOSC.md), zmiany – [CHANGELOG.md](../CHANGELOG.md).

Ostatnie wydania (szczegóły w CHANGELOG):
- 26.11.29–26.11.30 – zdarzenia wtórne (K15) z ważnością i zatwierdzaniem (K16), symbol `{next_start_iso}` (Za chwilę Z4), okno AppImage logowane kontem (K20), renderer natywny z VizRS CG System 26.11.13;
- 26.11.31–26.11.33 – okno AppImage z GStreamerem i podglądem `/output`, grafika VizRS na pozycji bez elementu VizRS, wydanie z bramką CI;
- 26.11.34–26.11.35 – grafika na pozycji bez elementu także przy `ingest_anytime`, yt-dlp 2026.11.23 dla wejścia YouTube.

**PLAN-mcr** ([plany/PLAN-mcr.md](plany/PLAN-mcr.md)): K1–K21 zrobione i wydane; do zrobienia K22–K23 (nagła emisja live), K24 po K23 (droga W1); otwarte Q1 (ręcznie) i propozycje bez decyzji (§7.3, §8).

## Następne

1. **Q1 (ręcznie, z użytkownikiem):** Sloty na żywo z prawdziwym sygnałem OBS → CG Playout, Plex z prawdziwym kontem (LUKI #3).
2. **Nagła emisja live „dziś” – K22–K24** ([PLAN-mcr](plany/PLAN-mcr.md), decyzje 2026-11-28: wariant B – okno przejęcia w CG Playout, los pozycji pod spodem wybiera operator): K22 Refresh zamiast Reset przy przebudowie, K23 okno nagłej emisji z datą (CG + panel), K24 „zachowaj odcinek” – rozpoznanie zrobione 2026-11-28: legacy nie ma komendy cofającej postęp serii; decyzja W1 – jednodniowy harmonogram alternatywny z rokiem tylko dla `keep`, wykonanie po K23.
3. **Do decyzji:** „Ramówka dnia” i kreator szablonu dnia (PLAN-mcr §7.3 – przy akceptacji nowe numery, bo K19/K20 to dziś konta), próba N0/N1 z ErsatzTV Next (§8), D3 (CG Playout przestaje pisać do legacy) i D4 (jedna nakładka compose) z §9.

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
| K1–K21 | PLAN-mcr: sesje, CSP, kontrola ramówki, mozaika, import ramówki, warstwy i TAKE/OUT, as-run, zdarzenia wtórne z ważnością, alarmy, konta operatorów, okno logowane kontem, jedno konto w pakiecie | [plany/PLAN-mcr.md](plany/PLAN-mcr.md) §4 |

## Panel – historia wydań

Wydania od 26.11.11 (wspólne z sidecarem) opisuje [CHANGELOG.md](../CHANGELOG.md); niżej historia panelu sprzed połączenia.

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

- Sloty na żywo z prawdziwym sygnałem z OBS i Plex z prawdziwym kontem – Q1 (LUKI #3).
- Lokalny stos `virtrs` (poza repo): `.env` poprawiony 2026-11-16 (`COMPOSE_FILE` z `docker-compose.live.yml` i `panel/compose.build.yml`, `COMPOSE_PROFILES=grafika,ingest`; kopia `.env.bak-przed-p4`). Kroki ręczne sprzed połączenia (nakładka `compose.cg-playout.yml`, legacy v26.12.0, `VIZRS_LIVE_VIZRS_URL`) są nieaktualne – historia w gicie (`d849485` i wcześniej).

## Panel – blokery / uwagi

- e2e wymaga Playwright (`PLAYWRIGHT_DIR` albo `panel/tests/local.env`) i Dockera (stos testowy).
- Prawdziwe usługi (`panel/tests/real.sh`) sprawdza się ręcznie; przed `panel/tests/stack.sh test` zawsze `panel/tests/real.sh down` (inaczej liczby pozycji w e2e się nie zgadzają).
