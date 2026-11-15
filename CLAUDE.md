# VizRS Live

Polski panel kanału nad **ErsatzTV legacy** (rodzina VizRS). Cel: operator w ogóle nie korzysta z web GUI legacy – logika (baza, skaner, planowanie, nadawanie przez ErsatzTV Next) zostaje w legacy, VizRS Live ją pokazuje i nią steruje przez most w procesie legacy. Grafika kanału i sloty na żywo – przez VizRS CG Playout (osobne repo).
Plan i decyzje rodziny: `../VizRS CG System/docs/plany/PLAN-vizrs-live.md`.

<!-- pakiet:A – blok wspólny pakietu VizRS; źródło: VizRS CG System/CLAUDE.md, kopie: tools/sync-zasady.sh (nie edytuj w kopii) -->
Język pracy z użytkownikiem, dokumentacji, komentarzy, komunikatów i UI: **polski** – pisz zgodnie z otoczeniem.

## Pakiet VizRS

To repo jest częścią pakietu **VizRS**. Opis całego pakietu (mapa aplikacji, styki, stos, porty, plan porządków): [sdorpl/VizRS](https://github.com/sdorpl/VizRS) (lokalnie `../VizRS`).

| Aplikacja (katalog obok, `../`) | Rola | Dostawca styku |
|---|---|---|
| **VizRS CG System** (`VizRS CG System`) | silnik grafiki: Trio, wyjście `/output`, TCP 6100, HTTP/SSE, `vizrs-render`; źródło wspólnego kodu i zasad pakietu | protokół VizRS, `/output`, `vizrs-render`, format show i `.cgpack` (`docs/API.md`) |
| **VizRS CG Artist** (`VizRS CG Artist`) | edytor scen i animacji (Tauri 2) | – (klient CG System) |
| **VizRS CG Playout** (`VirtRS CG Playout`) | sidecar grafiki i live ingest obok ErsatzTV; Live API | Live API `:7700` (DOKUMENTACJA §1) |
| **VizRS Live** (`VizRS Live`) | panel kanału nad ErsatzTV legacy, VizRS Bridge, natywny klient panelu | most `/vizrs-bridge/*`, wspólny stos compose |

Kierunek (decyzja 2026-11-15): VizRS Live łączy się z VizRS CG Playout w jeden produkt **„VizRS CG Playout”** (plan: sdorpl/VizRS §3).

Zgodność – wszystkie aplikacje w bieżących wersjach działają razem:
- **Styk zmienia się po obu stronach w tym samym cyklu.** Opis styku jest u dostawcy; klient opisuje tylko, jak z niego korzysta.
- **Starsza wersja sąsiada nie psuje aplikacji ani emisji** – brak nowego pola lub ścieżki to „niedostępne w tej wersji”, nie błąd.
- **Wydania razem:** przed wydaniem sprawdź z bieżącymi wersjami sąsiadów; zmianę wymagającą nowszego sąsiada odnotuj w `CHANGELOG.md` z minimalną wersją.
<!-- /pakiet:A -->

## Zacznij każdą sesję tutaj

<!-- pakiet:S -->
- Przeczytaj [docs/STATUS.md](docs/STATUS.md) (bieżący etap, następne zadanie), `git log -5` i `git status`.
- Przeczytaj [docs/LUKI.md](docs/LUKI.md): znane braki, uszeregowane G1 (niezawodność i bezpieczeństwo) → G2 (wierność) → G3 (funkcje).
- Przy zmianie zachowania przeczytaj właściwą sekcję [docs/DOKUMENTACJA.md](docs/DOKUMENTACJA.md) – w tym samym commicie ją zaktualizujesz.
- Zmiana dotyka styku z inną aplikacją pakietu – przeczytaj opis styku u dostawcy (tabela „Pakiet VizRS”) i sprawdź drugą stronę.
<!-- /pakiet:S -->
- Plany panelu są w **jednym dokumencie** [docs/plany/PLAN-mcr.md](docs/plany/PLAN-mcr.md) (decyzja użytkownika 2026-11-09) – nowe punkty i analizy dopisuj tam, nie zakładaj nowych `PLAN-*.md`; spis i archiwum: [docs/plany/](docs/plany/README.md). Praca wg planu: **jeden punkt = jedna sesja** – weź pierwszy punkt „do zrobienia” ze spełnionym „Wymaga” (albo wskazany przez użytkownika), po skończeniu (warunki „gotowe”, commit, kolumna Stan) zakończ i zaproponuj nową sesję. Plan rozbudowy R0–R12 jest zrobiony: [PLAN-rozbudowy.md](docs/plany/PLAN-rozbudowy.md) – tam sekcja „Jak pracować” (wzór dla kolejnych planów i automatu `scripts/plan.sh`).
- Testy i wdrożenie: `tests/deploy.sh` (niżej „Testy”). Lokalnego stosu testowego nie stawiaj, chyba że użytkownik o to poprosi.

## Niepodważalne zasady

### Wspólne dla pakietu

<!-- pakiet:B -->
- **Emisja nie pada przez nasz błąd.** Błędy przez `Result`, limity czasu na połączeniach, blokady odporne na zatrucie; awaria grafiki, panelu albo zapisu na dysk nie zatrzymuje anteny ani kanału.
- **Dane z zewnątrz są obce.** Wejście z sieci, plików, ErsatzTV i sąsiednich aplikacji – walidacja i limity; tekst do DOM tylko przez `textContent` (bez `innerHTML`/`eval`); sekrety (tokeny) nie trafiają do przeglądarki ani do logów.
- **Zależności tylko z listy.** Serwery pakietu (`vizrs`, `vizrs-live`) nie mają zależności zewnętrznych; front-end to czysty JS (moduły ES) bez bundlera, bibliotek i CDN. Wyjątki (okna Tauri, renderer natywny, sidecar) – tylko crate'y wymienione w nagłówku ich `Cargo.toml`; nowa zależność to decyzja użytkownika.
- **Kod wspólny tylko przez synchronizację.** Kod, wygląd (`vizrs-ui.css`), moduły okna (`gpu.rs`, `scale.rs`, `screen.rs`) i zasady pakietu mają źródło w VizRS CG System; kopie aktualizują skrypty (`tools/sync-cg.sh`, `scripts/sync-ui.sh`, `tools/sync-zasady.sh`) ze sprawdzaniem `--check` w CI – kopii nie edytuje się ręcznie. Cudzy kod tylko na licencji zgodnej z MIT i z wpisem w `THIRD-PARTY.md`. ErsatzTV – wyłącznie przez jego interfejsy (element „script”, ścieżka FFmpeg, startup hook mostu, komendy MediatR), bez kopiowania i bez zmian w jego kodzie.
- **Licencja: MIT** (`LICENSE`, `Copyright (c) 2026 sdorpl`; pole `license` w manifestach); składniki zewnętrzne – `THIRD-PARTY.md`.
- **Numer wersji: rok.miesiąc.rewizja** (`26.11.1` = listopad 2026, pierwsze wydanie w miesiącu; kolejne `26.11.2`, w grudniu od `26.12.1`), bez zer wiodących (semver: Cargo, paczki, obrazy). Znacznik `v<wersja>` = wersja we wszystkich manifestach projektu (lista – „Tego projektu”). Wydania z gałęzi `main`; `main-oldstable` to stan sprzed 26.11.1.
- **Gdzie co opisywać:** README tylko wprowadza i odsyła; opis działania, opcje i zmienne tylko w `docs/DOKUMENTACJA.md` (nie powielaj w README, planach ani STATUS); API, którego aplikacja jest dostawcą – `docs/API.md` (albo sekcja wskazana w „Tego projektu”); praca użytkownika – `docs/INSTRUKCJA*.md`; wdrożenie i awarie – `docs/RUNBOOK.md`; plany – `docs/plany/` + wiersz w `docs/plany/README.md`; opis całego pakietu – sdorpl/VizRS.
- **Dokumentacja razem z kodem:** commit zmieniający zachowanie (UI, API, format, opcje, obrazy) aktualizuje w tym samym commicie `docs/DOKUMENTACJA.md`, a gdy dotyczy codziennej pracy – także instrukcję; API – `docs/API.md`; wdrożenie – `docs/RUNBOOK.md`; zmiany wydania – `CHANGELOG.md` (sekcja „Niewydane”); zamknięta luka – usuń wiersz z `docs/LUKI.md`; zmiana etapu – `docs/STATUS.md`.
- **Produkcji i danych użytkownika nie ruszaj bez prośby.** Testy na kopii danych, innym porcie albo stosie testowym; działających serwerów, stosów i kontenerów nie zatrzymuj (mogą nadawać).
- **Zobacz wynik.** Przy pracy nad UI zrzut ekranu (Playwright; pliki PNG poza repo) i obejrzenie go przed commitem.
<!-- /pakiet:B -->

### Tego projektu

- **Legacy bez zmian.** Nie forkujemy ErsatzTV i nic do niego nie zgłaszamy. Most ładuje się do oficjalnego obrazu przez `DOTNET_STARTUP_HOOKS` + `ASPNETCORE_HOSTINGSTARTUPASSEMBLIES`. **Żadnych zapisów wprost do bazy legacy** – tylko komendy legacy (MediatR), te same, których używa jego UI.
- **C# tylko w moście** (`bridge/`), jak najmniej: ogólny dyspozytor `POST /vizrs-bridge/command/{Nazwa}` z białą listą. Nowa funkcja panelu = wiersz w `bridge/commands.txt` + kod w Ruście/JS. Nowy kod C# tylko, gdy dyspozytor nie wystarcza (jak wgrywanie obrazu, R5) – wtedy mały, ogólny endpoint, opisany w DOKUMENTACJA §2.
- **Biała lista jest granicą bezpieczeństwa.** Nie dopisuj komend, które zwracają sekret w innym kształcie niż pole (`Get{Jellyfin,Emby,Plex}ConnectionParameters`), komend wewnętrznych (`TryCompletePlexPinFlow`, `RebuildSearchIndex`) ani takich, które legacy psuje (DOKUMENTACJA §5).
- **Zero zależności w serwerze.** `server/Cargo.toml` ma pustą sekcję `[dependencies]` – własny HTTP/1.1 (`http.rs`), klient mostu (`bridge.rs`), JSON. Nie dodawaj crate'ów. UI to czysty JS (moduły ES) bez bundlera, bibliotek i CDN.
- **Wyjątek: aplikacja `desktop/`** (`vizrs-live-desktop`, Tauri 2, decyzja użytkownika 2026-11-12, plan `../VizRS CG System/docs/plany/PLAN-wyglad-live-i-tauri.md` T2) – osobny workspace z własnym `Cargo.lock`; dozwolone tylko crate'y z nagłówka `desktop/Cargo.toml` (tauri, tauri-plugin-dialog, tauri-plugin-single-instance, serde_json, ureq/rustls, getrandom, webkit2gtk; tauri-build). **Natywny klient panelu na serwerze** (decyzja użytkownika 2026-11-15): nie wkompilowuje ani nie uruchamia panelu – łączy się z panelem przez pośrednika (`desktop/src/proxy.rs`), który dokleja token – **token nie trafia do strony**. Strona → okno tylko przez białą listę `desktop/src/bridge.rs` (komenda `ipc`, uprawnienie tylko dla adresu bieżącego panelu). Test: `node desktop/tests/okno.cjs` (otwiera okna).
- **UI jest wbudowane w binarkę** (`include_str!`, tablica `STATIC` w `server/src/main.rs`). Wniosek:
  - zmiana w `server/web/*` nie działa w uruchomionym panelu, dopóki nie przebudujesz binarki albo nie uruchomisz z `--web server/web`;
  - nowy plik w `server/web/` trzeba dopisać do `STATIC` (test `module_imports_resolve` pilnuje importów).
- **Dane z legacy i CG są obce.** Tekst do DOM tylko przez `textContent` (żadnego `innerHTML`/`eval`), bez stylów i skryptów inline (CSP na każdej odpowiedzi). Komunikat błędu legacy pokaż operatorowi (toast), z polskim wstępem.
- **Sekrety zostają w serwerze.** Token mostu i token CG Playout nie trafiają do przeglądarki; pola `…apikey/token/password/secret` w odpowiedziach maskuje `mask_secrets`. Każde żądanie zmieniające wymaga `X-VizRS-Live: 1` (CSRF).
- **CG Playout tylko przez jego API**, przez pośrednika `/api/cg/…` z białą listą ścieżek (DOKUMENTACJA §3). Panel nie zapisuje plików CG ani legacy (decyzja R2: YAML, skrypty i JSON leżą na wolumenie `/config`).
- **Lokalny stos użytkownika (`virtrs-*`) tylko przez `tests/deploy.sh`** (decyzja użytkownika 2026-11-07: „wdrażaj od razu, bez stosów do testów”): skrypt podmienia sam panel (`live`), przejście po ekranach jest tylko do odczytu, a błąd = powrót do poprzedniego obrazu. Legacy, CG Playout, VizRS i proxy nie restartuj, nie zapisuj nic do legacy ani CG poza tym, o co prosi użytkownik.
- **Gdzie co opisywać (uzupełnienie):** most i API panelu – DOKUMENTACJA §2–§3 (dostawca: most `/vizrs-bridge/*`); praca operatora – `docs/INSTRUKCJA.md`; nowy plan lub punkt – w `docs/plany/PLAN-mcr.md` (jeden dokument planów panelu).
- **Manifesty wersji:** znacznik `v<wersja>` = `server/Cargo.toml`, `desktop/Cargo.toml`, `desktop/tauri.conf.json`, `bridge/VizRS.Bridge.csproj` i stała `Version` w `bridge/Bridge.cs`.
- **Dokumentacja razem z kodem (uzupełnienie):** dotyczy też ekranów (DOKUMENTACJA §4 tabela ekranów), białej listy mostu i ograniczeń legacy (§5) oraz obrazów.

## Współpraca z Gemini

<!-- pakiet:C -->
Szczegóły: [docs/AGENCI.md](docs/AGENCI.md) – wspólny dla pakietu. W skrócie:

- **Claude dowodzi i jako jedyny pisze kod** (lista „Kod” niżej oraz pliki zasad, CI, Docker, testy, skrypty, manifesty, licencje, białe listy, decyzje w planach).
- **Gemini wykonuje lekkie zadania:** redaguje wskazane pliki `.md` z listy „Dokumentacja”, uruchamia bramkę z listy „Bramka” i streszcza wynik, streszcza długie wyjścia, przeszukuje źródła (także sąsiednich repozytoriów pakietu – tylko do czytania). Nie commituje.
- **Kontekst Gemini:** `AGENTS.md` to **kopia** tego pliku (nie dowiązanie: Portainer GitOps odrzuca repozytorium z symlinkiem, a Windows bywa z nimi zawodny) – zmienia się razem z `CLAUDE.md` (`tools/sync-zasady.sh` kopiuje, `--check` w CI porównuje). `agy` sam wczytuje `AGENTS.md` i `GEMINI.md`, także w `-p`; importów `@plik` nie rozwija. `GEMINI.md` i `docs/AGENCI.md` są wspólne dla pakietu (źródło: VizRS CG System) i niczego stąd nie powielają.
- **Wywołanie:** `~/.gemini/bin/agy` (poza `PATH`), przełączniki – AGENCI §5. Wygodniej przez wrapper wtyczki antigravity: `PATH="$HOME/.gemini/bin:$PATH" agy-delegate [-m "Gemini 3.8 Flash (High)"] [--digest] [--mode plan] --dir . "<zlecenie>"` (poziomu `--tier pro` w tej subskrypcji nie ma). Wrapper sam stawia `-p` na końcu i odcina stdin. Zamiast kodu 0 zwraca kod błędu: 10 – limit, 11 – logowanie, 12 – przekroczony czas, 14 – brak modelu, 15 – odmowa uprawnień, 3 – pusta odpowiedź. Na stderr wypisuje wiersz `AGY_USAGE` z tokenami i modelem. Tylko subskrypcja Google (bez kluczy API). `--yolo` / `--dangerously-skip-permissions` – nigdy: daje zgodę na cały komputer, a `--sandbox` jej nie ogranicza.
- **Próg opłacalności:** każde zlecenie ma koszt stały (zlecenie, czekanie, przegląd). Małe, jednorazowe i wymagające decyzji zadania rób sam – ich zlecanie to zmierzona strata. Zlecaj duże porcje tekstu przy niewielu decyzjach: prozę dokumentacji (też w kilku repozytoriach naraz, AGENCI §5), przeszukanie dużych źródeł, nieustrukturyzowane logi. Jedno duże zlecenie zamiast wielu małych. Bierz streszczenie (`--digest`), nie surowe wyjście; plików, które przerobił Gemini, nie czytaj ponownie – przeglądaj diff. Logi (bramka, kompilacja, testy) czytaj sam przez plik i filtr (`tail`, `grep`) – AGENCI §4.
- **Weryfikacja:** przed zleceniem migawka `git status` (i `git diff`, gdy pliki mają już zmiany), po zleceniu przegląd diffu – razem z usuniętymi wierszami. Kod 0 i raport „gotowe” niczego nie dowodzą. Bramkę uruchom sam, w czystym stanie – `agy` potrafi zmienić środowisko (połatać pakiety, podstawić atrapy), żeby test przeszedł. Każdą liczbę, ścieżkę i stan etapu sprawdź z kodem (Gemini dopowiada fakty).
<!-- /pakiet:C -->

### Ten projekt

- **Kod (tylko Claude):** `server/**` (`*.rs`, `server/web/*`), `bridge/**` (`*.cs`, `*.csproj`) i **`bridge/commands.txt`** (biała lista – granica bezpieczeństwa), `desktop/**`, `tests/**`, `scripts/**`, `docker/**`, `compose*.yml`, `.env.example`, `.github/workflows/**`, `tools/sync-zasady.sh`; plany w `docs/plany/`.
- **Dokumentacja (Gemini – tylko pliki wskazane w zleceniu):** `README.md`, `CHANGELOG.md`, `docs/DOKUMENTACJA.md`, `docs/INSTRUKCJA.md`, `docs/RUNBOOK.md`, `docs/STATUS.md`, `docs/LUKI.md`, `docs/README.md`.
- **Bramka (Gemini może uruchomić i streścić):** `cargo fmt --check --manifest-path server/Cargo.toml`, `cargo clippy`, `cargo test` (server, desktop), `tests/gate.sh`, `tests/stack.sh up|seed|test|down`, `tools/sync-zasady.sh --check`.
- **Zawsze:** produkcji nie ruszaj (stosy `virtrs-*`, serwery emisji, port 8409) – żadnego `docker compose` produkcji, zapisów w `etv-config/`, push ani tagów.

## Uruchamianie

```bash
cp .env.example .env && docker compose up -d --build     # produkcja / własny serwer (RUNBOOK §2)
cd server && cargo build --release                        # sam panel
./target/release/vizrs-live --bridge http://127.0.0.1:18409 --bridge-token … --web web   # UI z dysku
```

| Adres | Co |
|---|---|
| `http://<serwer>:8090/` | panel (logowanie tokenem `VIZRS_LIVE_TOKEN`) |
| `http://<serwer>:8409/iptv/channels.m3u`, `/iptv/xmltv.xml` | widzowie – przez proxy Caddy (web GUI legacy i most z zewnątrz = 403) |

Stos testowy (`tests/stack.sh up|seed|test|down|reset|mem`, dane w `$VIZRS_LIVE_TEST_DIR`, domyślnie `~/.cache/vizrs-live-test` – nie w `/tmp`, bo to często tmpfs, czyli RAM; limity pamięci: DOKUMENTACJA §7):

| Port (127.0.0.1) | Co |
|---|---|
| 18409 | legacy z mostem (`/vizrs-bridge/health`) |
| 18410 | proxy dla widzów |
| 18090 | panel testowy (z atrapą CG Playout) |
| 18096, 18700 | atrapy z `tests/e2e.cjs`: Jellyfin, VizRS CG Playout |
| 18196, 18896, 18324, 18701, 18092 | `tests/real.sh`: prawdziwe Jellyfin, Emby, Plex, CG Playout i drugi panel (ręcznie, poza bramką; przed `tests/stack.sh test` zawsze `tests/real.sh down`) |

Źródło legacy do podglądu: `git clone --depth 1 https://github.com/ErsatzTV/legacy` do katalogu tymczasowego (poza repo). Komendy ekranu legacy: `ErsatzTV/Pages/<Ekran>.razor` (`Mediator.Send(`, `new <Komenda>(`), definicje w `ErsatzTV.Application/**/Commands|Queries/*.cs`; parametry i wyliczenia daje działający most: `GET /api/commands` (konsola w Diagnostyce).

## Testy

```bash
tests/deploy.sh   # lokalnie, po każdej zmianie (ok. 20–60 s): cargo fmt/clippy/test, node --check, obraz panelu,
                  # podmiana `live` w stosie CG Playout, tests/prod-smoke.cjs (wszystkie ekrany, tylko odczyt), przy błędzie powrót
DEPLOY_SHOTS=<katalog poza repo> tests/deploy.sh   # to samo + zrzut każdego ekranu (przy pracy nad UI – obejrzyj)
tests/gate.sh     # pełna bramka ze stosem testowym i e2e – uruchamia ją CI (ci.yml) po pushu; lokalnie tylko na prośbę
tools/sync-zasady.sh --check   # zasady pakietu = CG System (wersja w docs/ZASADY_CG_SYSTEM)
```

`tests/deploy.sh` czyta `VIZRS_CG_DIR` (katalog stosu CG Playout, domyślnie `../VirtRS CG Playout`) i `PLAYWRIGHT_DIR` z `tests/local.env` (poza repo); token panelu bierze z `.env` CG Playout. Bezpiecznik `prod-smoke.cjs`: przeglądarka przepuszcza tylko GET, logowanie i komendy `Get…/Query…/Search…/Count…` + `NewFFmpegProfile` – każda inna próba oblewa przebieg.
Nowa funkcja = nowe przypadki w `tests/e2e.cjs` (sprawdzi je CI; w razie potrzeby dane w `tests/stack.sh seed`). Przy pracy nad UI **zobacz wynik** – zrzuty z `DEPLOY_SHOTS`, pliki PNG poza repo.

## Struktura

| Ścieżka | Rola |
|---|---|
| `bridge/Bridge.cs` | VizRS Bridge (C#, .NET 10): hak startowy, `/vizrs-bridge/{health,commands,command/…,upload/…}`, refleksja po `ErsatzTV.Application`, serializacja wyników |
| `bridge/commands.txt` | biała lista komend legacy (jedna w wierszu, `#` – komentarz) |
| `server/src/main.rs` | serwer panelu: opcje i `VIZRS_LIVE_*`, logowanie i limit prób, CSRF/CSP, `/api/*`, pośrednik CG (`/api/cg/…`), `mask_secrets`, `STATIC` |
| `server/src/bridge.rs` | klient HTTP mostu i CG Playout (sieć wewnętrzna) |
| `server/src/http.rs` | minimalny HTTP/1.1: jedno żądanie na połączenie, limity rozmiaru i czasu, nagłówki bezpieczeństwa |
| `desktop/` | aplikacja VizRS Live (Tauri 2): `src/main.rs` (opcje, okna, menu, komenda `ipc`), `src/proxy.rs` (pośrednik z tokenem), `src/connect.rs` + `ui/connect.html` (okno połączenia), `src/bridge.rs` (biała lista), `tests/okno.cjs`; `tauri.conf.json` (paczki: sam klient) |
| `server/web/desktop.js` | most panelu z aplikacją `desktop/` (tytuł okna, alarm); w przeglądarce nic nie robi |
| `server/web/app.js`, `core.js` | wejście i pasek górny; DOM (`h`, `table`…), `api`/`cmd`, toast, dialog, router (`VIEWS`), etykiety `PL`, kontrola białej listy |
| `server/web/form.js`, `parts.js`, `when.js`, `tasks.js`, `check.js` | formularz z metadanych komend; `crudView`, `itemsEditor`, `contentPicker`, `dayBar`; „kiedy obowiązuje”; wyniki zadań w tle z logu legacy; kontrola ramówki |
| `server/web/views/*.js` | ekrany (po jednym pliku na grupę menu albo duży ekran; spis: DOKUMENTACJA §3 „UI”) |
| `docker/` | `legacy-bridge.Dockerfile` (oficjalny legacy + DLL + `commands.txt` + workstation GC; bez limitu sterty – strojenie Ampere A1 w nakładce CG Playout), `live.Dockerfile` (`scratch`, musl, kompilacja krzyżowa z `-C target-cpu=neoverse-n1` dla arm64), `proxy.Caddyfile` |
| `compose.yml`, `.env.example` | wdrożenie: `live`, `ersatztv` (tylko `expose`), `proxy`; obrazy: panel i proxy `latest`, legacy najnowsze oficjalne |
| `compose.cg-playout.yml` | nakładka: VizRS Live w stosie VizRS CG Playout (RUNBOOK §2a) |
| `compose.vaapi.yml`, `compose.nvidia.yml` | nakładki: karta graficzna dla legacy – Intel/AMD (VAAPI, QSV), NVIDIA (NVENC przez CDI); RUNBOOK §2b |
| `tests/` | `deploy.sh` (szybkie testy + wdrożenie panelu na lokalny stos + `prod-smoke.cjs`, przejście tylko do odczytu), `stack.sh` (stos testowy, smoke, seed), `e2e.cjs` (Playwright + atrapy), `gate.sh` (pełna bramka, CI), `real.sh` (prawdziwe usługi), `mock-bridge.cjs` (atrapa mostu do zrzutów UI bez Dockera), `hwaccel.sh` (akceleracja Intel/AMD/NVIDIA w obrazie legacy – CI i wydanie) |
| `scripts/plan.sh` | automat planu: jeden punkt = jedna świeża sesja `claude -p`; dzienniki w `.plan-logs/` (poza repo) |
| `.github/workflows/` | `ci.yml` (bramka), `legacy-latest.yml` (co tydzień najnowsze legacy), `release.yml` (obrazy ghcr po tagu `v*`) |
| `docs/` | dokumentacja (spis: [docs/README.md](docs/README.md)); `docs/plany/` – plany (dokumenty projektowe, nie opis bieżącego działania) |

## Model w skrócie

Przeglądarka → `POST /api/cmd/{Nazwa}` (panel, ciasteczko + `X-VizRS-Live`) → `POST /vizrs-bridge/command/{Nazwa}` (token mostu, biała lista) → komenda legacy przez MediatR. Wynik: `{"ok":true,"result":…}`, odrzucenie legacy `{"ok":false,"error":…}`, zadanie w tle `queued` (202), wynik nieodczytywalny `warning`. Formularze budują się z metadanych `GET /api/commands` (typy parametrów, wyliczenia); wyliczeń typów zagnieżdżonych most nie opisuje – listy są wtedy w pliku ekranu. Zmiany ramówki wchodzą na antenę dopiero po przebudowie playoutu. Szczegóły: [docs/DOKUMENTACJA.md](docs/DOKUMENTACJA.md) §2–4.

## Commity

<!-- pakiet:D -->
- Jedna spójna zmiana = jeden commit, po polsku, opisowo (styl historii repo). Tylko zielone stany (bramka z „Testy”).
- Pliki dodawaj jawnie (`git add ścieżka`), nie `git add -A`. Nie commituj katalogów `target/`, zrzutów, logów, plików tymczasowych, `.env` ani sekretów.
- Commit i push tylko na prośbę użytkownika.
<!-- /pakiet:D -->
- Styl historii: „Sloty na żywo: jak nadawać – serwer i klucz RTMP, adres SRT z hasłem”; punkt planu: „R10: …”, „M2: …”. Zielony stan = `tests/deploy.sh`.
- Wyjątek: sesja automatu `scripts/plan.sh` robi lokalny commit, nigdy push.
- Nie commituj `server/target/`, `bridge/bin|obj/`, `.env`, `etv-config/`, `media/`, `tests/local.env` ani `.plan-logs/`.
