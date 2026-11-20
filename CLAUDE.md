# VizRS CG Playout

Sidecar (jeden plik binarny w Ruście + bezgłowy Chromium) łączący **ErsatzTV** z silnikiem grafiki **VizRS**: komendy TCP na start klipu,
reklamy bez grafiki i wtapianie `/output` – przez nakładkę ffmpeg (grafika wprost do ffmpeg ErsatzTV) albo element grafiki „script” ErsatzTV –
oraz live ingest (RTMP/SRT przejmuje kanał). Kod ErsatzTV zostaje niezmieniony.

**Panel** (`panel/`, do 2026-11-15 osobne repo VizRS Live – połączone, plan: sdorpl/VizRS §3): polski panel kanału nad **ErsatzTV legacy** – operator nie korzysta z web GUI legacy; logika (baza, skaner, planowanie, nadawanie przez ErsatzTV Next) zostaje w legacy, panel ją pokazuje i nią steruje przez **VizRS Bridge** (C#, w procesie legacy); grafikę kanału i sloty na żywo daje sidecar przez Live API. Do tego **natywny klient panelu** (Tauri 2, AppImage) dla stanowiska operatora.

Sidecar: kod, komentarze i komunikaty logu po angielsku; panel: po polsku. Dokumentacja i rozmowa po polsku.

<!-- pakiet:A – blok wspólny pakietu VizRS; źródło: VizRS CG System/CLAUDE.md, kopie: tools/sync-zasady.sh (nie edytuj w kopii) -->
Język pracy z użytkownikiem, dokumentacji, komentarzy, komunikatów i UI: **polski** – pisz zgodnie z otoczeniem.

## Pakiet VizRS

To repo jest częścią pakietu **VizRS**. Opis całego pakietu (mapa aplikacji, styki, stos, porty, plan porządków): [sdorpl/VizRS](https://github.com/sdorpl/VizRS) (lokalnie `../VizRS`).

| Aplikacja (katalog obok, `../`) | Rola | Dostawca styku |
|---|---|---|
| **VizRS CG System** (`VizRS CG System`) | silnik grafiki: Trio, wyjście `/output`, TCP 6100, HTTP/SSE, `vizrs-render`; źródło wspólnego kodu i zasad pakietu | protokół VizRS, `/output`, `vizrs-render`, format show i `.cgpack` (`docs/API.md`) |
| **VizRS CG Artist** (`VizRS CG Artist`) | edytor scen i animacji (Tauri 2) | – (klient CG System) |
| **VizRS CG Playout** (`VizRS CG Playout`) | sidecar grafiki i live ingest obok ErsatzTV; panel kanału nad ErsatzTV legacy (`panel/`, dawny VizRS Live): VizRS Bridge, natywny klient panelu; stos compose | Live API `:7700`, API panelu i most `/vizrs-bridge/*` (`docs/API.md`) |

VizRS Live (panel, most, okno) połączono 2026-11-15 z VizRS CG Playout w jeden produkt **„VizRS CG Playout”** z jedną wersją i jednym wydaniem (decyzja D1, plan: sdorpl/VizRS §3); repo `sdorpl/VizRS-Live-Playout` przechodzi do archiwum.

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
- Przy komendach VizRS sprawdź składnię w `../VizRS CG System/docs/API.md` (sekcja TCP).

- Plany panelu są w **jednym dokumencie** [docs/plany/PLAN-mcr.md](docs/plany/PLAN-mcr.md) (decyzja użytkownika 2026-11-09) – nowe punkty i analizy dopisuj tam, nie zakładaj nowych `PLAN-*.md`; spis i archiwum: [docs/plany/](docs/plany/README.md). Praca wg planu: **jeden punkt = jedna sesja** – weź pierwszy punkt „do zrobienia” ze spełnionym „Wymaga” (albo wskazany przez użytkownika), po skończeniu (warunki „gotowe”, commit, kolumna Stan) zakończ i zaproponuj nową sesję. Plan rozbudowy R0–R12 jest zrobiony: [PLAN-rozbudowy.md](docs/plany/archiwum/PLAN-rozbudowy.md) – tam sekcja „Jak pracować” (wzór dla kolejnych planów i automatu `scripts/plan.sh`).
- Testy i wdrożenie: `panel/tests/deploy.sh` (niżej „Testy”). Lokalnego stosu testowego nie stawiaj, chyba że użytkownik o to poprosi.

## Niepodważalne zasady

### Wspólne dla pakietu

<!-- pakiet:B -->
- **Emisja nie pada przez nasz błąd.** Błędy przez `Result`, limity czasu na połączeniach, blokady odporne na zatrucie; awaria grafiki, panelu albo zapisu na dysk nie zatrzymuje anteny ani kanału.
- **Dane z zewnątrz są obce.** Wejście z sieci, plików, ErsatzTV i sąsiednich aplikacji – walidacja i limity; tekst do DOM przez `textContent`, a HTML tylko szablonem `html` z `dom.js` (escapuje wstawiane wartości; `setHtml` to jedyne miejsce z `innerHTML`; wyjątek: zamrożony `/artist` CG System); bez `eval`; sekrety (tokeny) nie trafiają do przeglądarki ani do logów.
- **Zależności tylko z listy.** Serwery pakietu (`vizrs`, `vizrs-live`) nie mają zależności zewnętrznych; front-end to czysty JS (moduły ES) bez bundlera, bibliotek i CDN. Wyjątki (okna Tauri, renderer natywny, sidecar) – tylko crate'y wymienione w nagłówku ich `Cargo.toml`; nowa zależność to decyzja użytkownika.
- **Kod wspólny tylko przez synchronizację.** Kod, wygląd (`vizrs-ui.css`), moduły okna (`gpu.rs`, `scale.rs`, `screen.rs`, `conn_url.rs`) i zasady pakietu mają źródło w VizRS CG System; kopie aktualizują skrypty (`tools/sync-cg.sh`, `scripts/sync-ui.sh`, `tools/sync-zasady.sh`) ze sprawdzaniem `--check` w CI – kopii nie edytuje się ręcznie. Cudzy kod tylko na licencji zgodnej z MIT i z wpisem w `THIRD-PARTY.md`. ErsatzTV – wyłącznie przez jego interfejsy (element „script”, ścieżka FFmpeg, startup hook mostu, komendy MediatR), bez kopiowania i bez zmian w jego kodzie.
- **Licencja: MIT** (`LICENSE`, `Copyright (c) 2026 sdorpl`; pole `license` w manifestach); składniki zewnętrzne – `THIRD-PARTY.md`.
- **Numer wersji: rok.miesiąc.rewizja** (`26.11.1` = listopad 2026, pierwsze wydanie w miesiącu; kolejne `26.11.2`, w grudniu od `26.12.1`), bez zer wiodących (semver: Cargo, paczki, obrazy). Znacznik `v<wersja>` = wersja we wszystkich manifestach projektu (lista – „Tego projektu”). Wydania z gałęzi `main`; `main-oldstable` to stan sprzed 26.11.1.
- **Gdzie co opisywać:** README tylko wprowadza i odsyła; opis działania, opcje i zmienne tylko w `docs/DOKUMENTACJA.md` (nie powielaj w README, planach ani STATUS); API, którego aplikacja jest dostawcą – `docs/API.md` (albo sekcja wskazana w „Tego projektu”); praca użytkownika – `docs/INSTRUKCJA*.md`; wdrożenie i awarie – `docs/RUNBOOK.md`; plany – `docs/plany/` + wiersz w `docs/plany/README.md`; opis całego pakietu – sdorpl/VizRS.
- **Dokumentacja razem z kodem:** commit zmieniający zachowanie (UI, API, format, opcje, obrazy) aktualizuje w tym samym commicie `docs/DOKUMENTACJA.md`, a gdy dotyczy codziennej pracy – także instrukcję; API – `docs/API.md`; wdrożenie – `docs/RUNBOOK.md`; zmiany wydania – `CHANGELOG.md` (sekcja „Niewydane”); zamknięta luka – usuń wiersz z `docs/LUKI.md`; zmiana etapu – `docs/STATUS.md`.
- **Produkcji i danych użytkownika nie ruszaj bez prośby.** Testy na kopii danych, innym porcie albo stosie testowym; działających serwerów, stosów i kontenerów nie zatrzymuj (mogą nadawać).
- **Zobacz wynik.** Przy pracy nad UI zrzut ekranu (Playwright; pliki PNG poza repo) i obejrzenie go przed commitem.
<!-- /pakiet:B -->

### Tego projektu

- **Manifesty wersji:** znacznik `v<wersja>` = `version` w `Cargo.toml` i `package.json`.
- **Z VizRS CG System tylko przez protokół:** TCP 6100 (`AUTH`, `STATE`, `TAKE`, `OUT`, …), `GET /api/show`, strona `/output` i proces `vizrs-render stream` / `bounds` / `thumb` (renderer natywny, protokół klatek w VizRS CG System DOKUMENTACJA §10.1; jego kod zostaje w CG System – obraz `vizrs-cg` buduje go z tagu `VIZRS_REF` w `Dockerfile`).
- **ErsatzTV bez zmian w kodzie**: integracja tylko przez element „script” (`etv/templates/graphics-elements/script/vizrs.yml`) z klientem `vizrs-etv-client` i nakładkę ffmpeg `vizrs-ffmpeg` ustawioną jako ścieżka FFmpeg – obie to statyczne binarki musl z tego crate'a, budowane w obrazie sidecara na amd64 i arm64 i instalowane przez sidecara (obraz ErsatzTV nie ma Pythona na arm64 i niczego nie doinstalowujemy). Nakładka zmienia wyłącznie wejście płótna grafiki (z nim znaki wodne ErsatzTV tej pozycji: wejścia obrazów i ich nałożenie nad grafiką, wybór i położenie z mostu `GET /vizrs-bridge/watermarks/…`; plus jawnie włączone `VIZRS_FFMPEG_FPS`/`VIZRS_FFMPEG_PRESET`/`VIZRS_FFMPEG_X264_PARAMS` (z `VIZRS_FFMPEG_NVENC_BF` tylko pierwsze wypełnienie `encoder.json`: presety x264/NVENC/QSV, profil H.264, opcje x264 i klatki B koderów sprzętowych ustawia VizRS Live)/`VIZRS_FFMPEG_READRATE_CATCHUP`/`VIZRS_FFMPEG_FPS_BLEND`/`VIZRS_FFMPEG_CUDA`/`VIZRS_FFMPEG_NVENC_BF`/`VIZRS_FFMPEG_NO_SHORTEST`/`VIZRS_FFMPEG_SYNC_RAM`/`VIZRS_FFMPEG_NO_DISCARDCORRUPT`/`VIZRS_FFMPEG_INGEST` – ten ostatni dzieli kawałek na przebiegi kawałek/live do tej samej playlisty, tylko gdy sidecar ma sygnał live kanału); w płótnie z jednym `-init_hw_device cuda` grafika idzie przez `hwupload` zamiast `hwupload_cuda` (bez osobnego kontekstu CUDA na strefę); każde inne wywołanie i każdy błąd = oryginalna komenda.
- **Grafika nigdy nie zatrzymuje emisji**: komendy w tle z limitami czasu, błędy tylko do logu, awaria Chromium = czyste klatki i restart, brak sidecara = klip bez grafiki.
- **Bez GPU na ścieżce grafiki**: Chromium / vizrs-render renderuje programowo; na Oracle Cloud Ampere A1 (arm64) kompilacja binarek (`vizrs-cg`, `vizrs-ffmpeg`, `vizrs-etv-client`, `vizrs-render`) wymusza flagi mikroarchitektury `RUSTFLAGS="-C target-cpu=neoverse-n1 -C opt-level=3"` oraz dostrojenie .NET CLR (`DOTNET_gcServer=0`, limit sterty 1.5 GB). Obraz buduje się na amd64 i arm64 bez emulacji QEMU.
- **Mało zależności**: zależności runtime to tylko `png` i `regex` (bez domyślnych funkcji; wersje w `Cargo.lock`, budowa z `--locked`). Nowej nie dodawaj bez potrzeby – JSON, HTTP, base64, DevTools i skalowanie są własne. Testy kontraktu w Node.js – tylko biblioteka standardowa.
- **Gdzie co opisywać (uzupełnienie):** Live API, którego to repo jest dostawcą (`/api/live/*`, `/health`, `/etv/pages`, `/ingest/*`) – `docs/API.md` §1 (w DOKUMENTACJI §1 tylko odsyłacz). Nowa opcja = `src/config.rs` + `config.example.json` (gdy dotyczy) + `docker-compose.yml` + DOKUMENTACJA §5 + test.


#### Panel (ścieżki kodu względem `panel/`; dokumentacja panelu – DOKUMENTACJA §9–§15)

- **Legacy bez zmian.** Nie forkujemy ErsatzTV i nic do niego nie zgłaszamy. Most ładuje się do oficjalnego obrazu przez `DOTNET_STARTUP_HOOKS` + `ASPNETCORE_HOSTINGSTARTUPASSEMBLIES`. **Żadnych zapisów wprost do bazy legacy** – tylko komendy legacy (MediatR), te same, których używa jego UI.
- **C# tylko w moście** (`bridge/`), jak najmniej: ogólny dyspozytor `POST /vizrs-bridge/command/{Nazwa}` z białą listą. Nowa funkcja panelu = wiersz w `bridge/commands.txt` + kod w Ruście/JS. Nowy kod C# tylko, gdy dyspozytor nie wystarcza (jak wgrywanie obrazu, R5) – wtedy mały, ogólny endpoint, opisany w DOKUMENTACJA §10 i `docs/API.md` §3.
- **Biała lista jest granicą bezpieczeństwa.** Nie dopisuj komend, które zwracają sekret w innym kształcie niż pole (`Get{Jellyfin,Emby,Plex}ConnectionParameters`), komend wewnętrznych (`TryCompletePlexPinFlow`, `RebuildSearchIndex`) ani takich, które legacy psuje (DOKUMENTACJA §13).
- **Zero zależności w serwerze.** `server/Cargo.toml` ma pustą sekcję `[dependencies]` – własny HTTP/1.1 (`http.rs`), klient mostu (`bridge.rs`), JSON. Nie dodawaj crate'ów. UI to czysty JS (moduły ES) bez bundlera, bibliotek i CDN.
- **Wyjątek: aplikacja `desktop/`** (`vizrs-live-desktop`, Tauri 2, decyzja użytkownika 2026-11-12, plan `../VizRS CG System/docs/plany/PLAN-wyglad-live-i-tauri.md` T2) – osobny workspace z własnym `Cargo.lock`; dozwolone tylko crate'y z nagłówka `desktop/Cargo.toml` (tauri, tauri-plugin-dialog, tauri-plugin-single-instance, serde_json, ureq/rustls, getrandom, webkit2gtk; tauri-build). **Natywny klient panelu na serwerze** (decyzja użytkownika 2026-11-15): nie wkompilowuje ani nie uruchamia panelu – łączy się z panelem przez pośrednika (`desktop/src/proxy.rs`), który dokleja token – **token nie trafia do strony**. Strona → okno tylko przez białą listę `desktop/src/bridge.rs` (komenda `ipc`, uprawnienie tylko dla adresu bieżącego panelu). Test: `node desktop/tests/okno.cjs` (otwiera okna).
- **UI jest wbudowane w binarkę** (`include_str!`, tablica `STATIC` w `server/src/routes.rs`). Wniosek:
  - zmiana w `server/web/*` nie działa w uruchomionym panelu, dopóki nie przebudujesz binarki albo nie uruchomisz z `--web server/web`;
  - nowy plik w `server/web/` trzeba dopisać do `STATIC` (test `module_imports_resolve` pilnuje importów).
- **Dane z legacy i CG są obce.** Tekst do DOM tylko przez `textContent` (żadnego `innerHTML`/`eval`), bez stylów i skryptów inline (CSP na każdej odpowiedzi). Komunikat błędu legacy pokaż operatorowi (toast), z polskim wstępem.
- **Sekrety zostają w serwerze.** Token mostu i token CG Playout nie trafiają do przeglądarki; pola `…apikey/token/password/secret` w odpowiedziach maskuje `mask_secrets`. Każde żądanie zmieniające wymaga `X-VizRS-Live: 1` (CSRF).
- **CG Playout tylko przez jego API**, przez pośrednika `/api/cg/…` z białą listą ścieżek (DOKUMENTACJA §11). Panel nie zapisuje plików CG ani legacy (decyzja R2: YAML, skrypty i JSON leżą na wolumenie `/config`).
- **Lokalny stos użytkownika (`virtrs-*`) tylko przez `tests/deploy.sh`** (decyzja użytkownika 2026-11-07: „wdrażaj od razu, bez stosów do testów”): skrypt podmienia sam panel (`live`), przejście po ekranach jest tylko do odczytu, a błąd = powrót do poprzedniego obrazu. Legacy, CG Playout, VizRS i proxy nie restartuj, nie zapisuj nic do legacy ani CG poza tym, o co prosi użytkownik.
- **Gdzie co opisywać (uzupełnienie):** działanie panelu i mostu – DOKUMENTACJA §9–§15, ich API – `docs/API.md` §2–§3 (dostawca: most `/vizrs-bridge/*`, panel `/api/*`); praca operatora – `docs/INSTRUKCJA.md`; wdrożenie – `docs/RUNBOOK.md`; nowy plan lub punkt – w `docs/plany/PLAN-mcr.md` (jeden dokument planów panelu).
- **Manifesty wersji:** znacznik `v<wersja>` = `server/Cargo.toml`, `desktop/Cargo.toml`, `desktop/tauri.conf.json`, `bridge/VizRS.Bridge.csproj` i stała `Version` w `bridge/Bridge.cs`.
- **Dokumentacja razem z kodem (uzupełnienie):** dotyczy też ekranów (DOKUMENTACJA §12 tabela ekranów), białej listy mostu i ograniczeń legacy (§13) oraz obrazów.

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

- **Kod (tylko Claude):** `src/**`, `tests/**` (wektory, kontrakt), `Cargo.toml`, `Cargo.lock`, `package.json`, `Dockerfile`, `docker-compose*.yml`, `.release`, `etv/`, `seed/`, `bench/`, `.github/`, `config.example.json`, `tools/sync-zasady.sh`; plany w `docs/plany/`.
- **Dokumentacja (Gemini – tylko pliki wskazane w zleceniu):** `README.md`, `CHANGELOG.md`, `docs/DOKUMENTACJA.md`, `docs/API.md`, `docs/INSTRUKCJA.md`, `docs/RUNBOOK.md`, `docs/STATUS.md`, `docs/LUKI.md`.
- **Bramka (Gemini może uruchomić i streścić):** `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo build --release`, `npm test`, `tools/sync-zasady.sh --check`.
- **Zawsze:** stosów produkcyjnych (Portainer, serwery emisji) i lokalnego stosu użytkownika nie ruszaj; po testach na Dockerze sprzątaj tylko to, co powstało na potrzeby testu – bez `down -v` i `volume prune`.


#### Panel (ścieżki kodu względem `panel/`; dokumentacja panelu – DOKUMENTACJA §9–§15)

- **Kod (tylko Claude):** `server/**` (`*.rs`, `server/web/*`), `bridge/**` (`*.cs`, `*.csproj`) i **`bridge/commands.txt`** (biała lista – granica bezpieczeństwa), `desktop/**`, `tests/**`, `scripts/**`, `docker/**`, `compose*.yml`, `.env.example`, workflowy `panel-*.yml` w `.github/workflows/`; plan panelu `docs/plany/PLAN-mcr.md`.
- **Dokumentacja (Gemini – tylko pliki wskazane w zleceniu):** wspólna z sidecarem (lista wyżej) oraz `docs/API.md`, `docs/INSTRUKCJA.md`, `docs/RUNBOOK.md`.
- **Bramka (Gemini może uruchomić i streścić):** `cargo fmt --check --manifest-path server/Cargo.toml`, `cargo clippy`, `cargo test` (server, desktop), `tests/gate.sh`, `tests/stack.sh up|seed|test|down`, `tools/sync-zasady.sh --check`.
- **Zawsze:** produkcji nie ruszaj (stosy `virtrs-*`, serwery emisji, port 8409) – żadnego `docker compose` produkcji, zapisów w `etv-config/`, push ani tagów.

## Uruchamianie

```bash
COMPOSE_PROFILES=grafika docker compose up -d   # VizRS :8788, sidecar :7700, ErsatzTV :8409 (bez profilu: sam ErsatzTV)
COMPOSE_PROFILES=grafika docker compose -f docker-compose.yml -f docker-compose.live.yml up -d   # + panel :8090
cargo run --release -- config.json         # bez Dockera (Rust 1.87+, Chromium w PATH albo VIZRS_BROWSER)
```

Profile compose: `grafika` (`seed` jest zawsze; `vizrs`, `cg` tylko z profilem), `ingest` (wymaga `grafika`); w Portainerze zmienna `COMPOSE_PROFILES` stosu. Nakładki: `docker-compose.live.yml` (panel: legacy z mostem, `proxy`, `live`; Caddyfile w `configs:`), `panel/compose.build.yml` (lokalna budowa obrazów panelu, `tests/deploy.sh`), `docker-compose.ampere.yml` (Oracle Ampere A1), `docker-compose.nvidia.yml` (NVENC w ErsatzTV), `docker-compose.vaapi.yml` (VAAPI w ErsatzTV: Intel, AMD).


### Panel (ścieżki kodu względem `panel/`; dokumentacja panelu – DOKUMENTACJA §9–§15)

```bash
cp .env.example .env && docker compose up -d --build     # produkcja / własny serwer (RUNBOOK §2)
cd server && cargo build --release                        # sam panel
./target/release/vizrs-live --bridge http://127.0.0.1:18409 --bridge-token … --web web   # UI z dysku
```

| Adres | Co |
|---|---|
| `http://<serwer>:8090/` | panel (konta operatorów: login i hasło, pierwsze konto kodem z logu kontenera; token awaryjny `VIZRS_LIVE_TOKEN`) |
| `http://<serwer>:8409/iptv/channels.m3u`, `/iptv/xmltv.xml` | widzowie – przez proxy Caddy (web GUI legacy i most z zewnątrz = 403) |

Stos testowy (`tests/stack.sh up|seed|test|down|reset|mem`, dane w `$VIZRS_LIVE_TEST_DIR`, domyślnie `~/.cache/vizrs-live-test` – nie w `/tmp`, bo to często tmpfs, czyli RAM; limity pamięci: DOKUMENTACJA §7 „Testy panelu”):

| Port (127.0.0.1) | Co |
|---|---|
| 18409 | legacy z mostem (`/vizrs-bridge/health`) |
| 18410 | proxy dla widzów |
| 18090 | panel testowy (z atrapą CG Playout) |
| 18096, 18700 | atrapy z `tests/e2e.cjs`: Jellyfin, VizRS CG Playout |
| 18196, 18896, 18324, 18701, 18092 | `tests/real.sh`: prawdziwe Jellyfin, Emby, Plex, CG Playout i drugi panel (ręcznie, poza bramką; przed `tests/stack.sh test` zawsze `tests/real.sh down`) |

Źródło legacy do podglądu: `git clone --depth 1 https://github.com/ErsatzTV/legacy` do katalogu tymczasowego (poza repo). Komendy ekranu legacy: `ErsatzTV/Pages/<Ekran>.razor` (`Mediator.Send(`, `new <Komenda>(`), definicje w `ErsatzTV.Application/**/Commands|Queries/*.cs`; parametry i wyliczenia daje działający most: `GET /api/commands` (konsola w Diagnostyce).

## Testy (bramka przed commitem)

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings
cargo test                                 # moduły, wektory tests/vectors/, Chromium (pomijany bez przeglądarki)
cargo build --release && npm test          # kontrakt: binarka jako czarna skrzynka (tests/contract/)
tools/sync-zasady.sh --check                # zasady pakietu = CG System (wersja w docs/ZASADY_CG_SYSTEM)
```

Wektory `tests/vectors/*.json` to zachowanie sidecara Node.js sprzed migracji (generator w historii gita, commit 0c54143); po zamierzonej zmianie zachowania popraw wektor w tym samym commicie. Testy kontraktu (udawany VizRS, klient `vizrs-etv-client`, nakładka `vizrs-ffmpeg`; `VIRTRS_CG_BIN`/`VIZRS_ETV_CLIENT_BIN`/`VIZRS_FFMPEG_BIN` wskazują inne binarki) obowiązują każdą zmianę protokołu.

Po testach na stosie Dockera zawsze sprzątaj to, co powstało na potrzeby testu (kontenery, obrazy, wolumeny, pliki), ale **dane działających instancji tego projektu zostawiaj** – bez `down -v` i `volume prune`. Szczegóły: DOKUMENTACJA §7.


### Panel (ścieżki kodu względem `panel/`; dokumentacja panelu – DOKUMENTACJA §9–§15)

```bash
tests/deploy.sh   # lokalnie, po każdej zmianie (ok. 20–60 s): cargo fmt/clippy/test, node --check, obraz panelu,
                  # podmiana `live` w stosie CG Playout, tests/prod-smoke.cjs (wszystkie ekrany, tylko odczyt), przy błędzie powrót
DEPLOY_SHOTS=<katalog poza repo> tests/deploy.sh   # to samo + zrzut każdego ekranu (przy pracy nad UI – obejrzyj)
tests/gate.sh     # pełna bramka ze stosem testowym i e2e – uruchamia ją CI (ci.yml) po pushu; lokalnie tylko na prośbę
tools/sync-zasady.sh --check   # zasady pakietu = CG System (wersja w docs/ZASADY_CG_SYSTEM)
```

`tests/deploy.sh` czyta `VIZRS_CG_DIR` (katalog stosu CG Playout, domyślnie korzeń tego repozytorium) i `PLAYWRIGHT_DIR` z `tests/local.env` (poza repo); token panelu bierze z `.env` CG Playout. Bezpiecznik `prod-smoke.cjs`: przeglądarka przepuszcza tylko GET, logowanie i komendy `Get…/Query…/Search…/Count…` + `NewFFmpegProfile` – każda inna próba oblewa przebieg.
Nowa funkcja = nowe przypadki w `tests/e2e.cjs` (sprawdzi je CI; w razie potrzeby dane w `tests/stack.sh seed`). Przy pracy nad UI **zobacz wynik** – zrzuty z `DEPLOY_SHOTS`, pliki PNG poza repo.

## Struktura

| Ścieżka | Rola |
|---|---|
| `src/main.rs` | start: konfiguracja, serwer, `SIGTERM` (zamyka przeglądarki) |
| `src/config.rs` | domyślne opcje, plik JSON, zmienne środowiskowe |
| `src/server.rs` | HTTP: `POST /etv?channel=N` (strumień pakietów), `GET /etv/pages` (strony VizRS), `GET /ffmpeg`, `GET /ffmpeg/frames` (nakładka ffmpeg), `GET /health`; przechwytywanie na rozmiar, przejmowanie sesji |
| `src/http.rs`, `src/json.rs` | własny minimalny HTTP/1.1 (strumienie chunked jak w Node; HTTP/1.0 dla nakładki) i JSON |
| `src/template.rs`, `src/time.rs` | kontekst klipu z danych szablonu ErsatzTV, symbole `{title}`, `{next_*}`…, wykrywanie reklam (`regex`), daty ISO |
| `src/vizrs.rs` | klient TCP VizRS (`AUTH`, `STATE`, komendy), zdarzenia kanału w kolejności, zdejmowanie/przywracanie stron przy reklamach |
| `src/capture/` (`tape.rs` – taśma crawla), `src/devtools.rs`, `src/sys.rs` | bezgłowy Chromium (screencast DevTools albo klatki na żądanie `beginFrame`, `capture_mode`, przez `--remote-debugging-pipe`), restart po awarii, `PR_SET_PDEATHSIG`, sygnały (też dla nakładki), zegar monotoniczny |
| `src/native.rs` | renderer natywny VizRS (`renderer: native\|auto`): proces `vizrs-render stream` na warstwę show (`?layers=`, `native_layers`; kolejność `layerZ`, mieszanie nachodzących warstw) albo jeden na całą antenę, rekordy klatek, prostokąty dla historii (DOKUMENTACJA §1) |
| `src/frames.rs`, `src/png.rs`, `src/rects.rs`, `src/scale.rs`, `src/area.rs`, `src/footprint.rs`, `src/bounds.rs` | historia klatek, dekodowanie PNG, widoczne prostokąty, skalowanie jak libvips, obszar grafiki (`area`), ślady stron i strefy `area: auto` (komórki 16 px, PLAN-strefy), granice stron z VizRS (`vizrs-render bounds`, PLAN-strefy S6) |
| `src/packets.rs`, `src/stream.rs`, `src/yuva.rs`, `src/delta.rs` | pakiety ETV (`Rectangles`/`Repeat`; piksele `Arc`, wspólne między obrazami), tempo strumienia i `lead_seconds`, wstrzymanie strumienia przed obrazem spoza stref, klatki `yuva420p` dla nakładki ffmpeg (z poprzedniej klatki, liczby całkowite, wątki przy dużej zmianie), delta klatek do `vizrs-ffmpeg` (tylko zmienione wiersze) |
| `src/etv_client.rs`, `src/bin/vizrs-etv-client.rs` | klient elementu „script” w kontenerze ErsatzTV: stdin → sidecar → stdout, w tle synchronizacja elementów stron |
| `src/etv_sync.rs` | sidecar instaluje programy ErsatzTV i `vizrs.yml`, a cyklicznie synchronizuje kanały i elementy stron (DOKUMENTACJA §4) |
| `src/sync.rs` | strony VizRS → elementy `page-<ID>.yml` ErsatzTV (także `vizrs-cg sync`); plików nie kasuje, bo ErsatzTV usunąłby przypisania (DOKUMENTACJA §3) |
| `src/net.rs` | wspólny klient HTTP programów ErsatzTV (URL, połączenia z limitami czasu) |
| `src/overlay/`, `src/bin/vizrs-ffmpeg.rs` | nakładka ffmpeg ErsatzTV (binarka `vizrs-ffmpeg`): płótno grafiki zastąpione klatkami `yuva420p` z sidecara (DOKUMENTACJA §1); `settings.rs`, `args.rs` (argumenty ffmpeg), `cuda.rs`, `cpu.rs`, `ram_sync.rs`, `canvas.rs`, `zones.rs`, `relay.rs` (strumień klatek), `run.rs` (przebieg, przejęcie anteny) |
| `src/ingest.rs` | live ingest w sidecarze: `POST /ingest` (MPEG-TS z odbiornika, token), `GET /ingest` (strumień od ostatniej klatki kluczowej), `GET /ingest/state`; parser PAT/PMT/klatek kluczowych |
| `src/listener.rs` | odbiorniki live ingest (`vizrs-ffmpeg vizrs-ingest`, usługa `ingest` w profilu compose): ffmpeg `-listen` RTMP/SRT → `-c copy` MPEG-TS → sidecar |
| `src/live_shows.rs`, `src/standby.ts` | audycje na żywo w ramówce: lista `<etv_config>/vizrs-live/shows.json` (z `live_shows` tylko pierwszy raz), definicje Remote Stream ErsatzTV obok; sekunda czerni MPEG-TS slotu bez sygnału (dla ffprobe ErsatzTV) |
| `src/panel.rs`, `src/etv_api.rs`, `src/encoder.rs` | live API dla VizRS Live (`/api/live/*`, `panel_token`; bez własnej strony – dawny Live Panel `/live` usunięty 2026-11-07): stan kanałów, ZDEJMIJ / WPUŚĆ (`control.json`), koder nakładki (`encoder.json`: presety x264/NVENC/QSV, profil H.264, opcje x264, klatki B sprzętowe; czytane też przez `vizrs-ffmpeg`; `.env` tylko na start), kreator audycji LIVE (plik, skan, Smart Collection `LIVE · …`, reset playoutu); klient API ErsatzTV i parser XMLTV |
| `src/thumbs.rs` | miniatury stron VizRS dla VizRS Live (`GET /api/live/thumbs/<numer>.png`): `vizrs-render thumb`, pamięć podręczna wg wersji strony w `<etv_config>/vizrs-live/thumbs/` |
| `src/takeover.rs` | przejęcie kawałka przez live w nakładce (`VIZRS_FFMPEG_INGEST`): przebiegi kawałek/live do tej samej playlisty HLS, `-ss`/`-t`/`-output_ts_offset` z EXTINF, komenda live z koderem i HLS ErsatzTV, przełączanie pipe'ów grafiki; restart przebiegu przy strefach poszerzonych w środku kawałka (`area: auto`, PLAN-strefy S3b) |
| `etv/templates/…/vizrs.yml` | definicja elementu grafiki „VizRS” dla ErsatzTV |
| `Dockerfile` | obraz sidecara: binarka i `vizrs-render` z wydania VizRS (`VIZRS_REF`; arm64 krzyżowo) + `chromium-headless-shell` |
| `tests/*.rs`, `tests/vectors/` | testy Rusta, wektory referencyjne |
| `tests/contract/` | testy kontraktu w Node.js (`npm test`) |
| `seed/` | dane startowe stosu: show VizRS, baza ErsatzTV (kanał 1), media testowe; obraz `vizrs-seed` z wydania – `init.sh` wypełnia puste wolumeny i daje sidecarowi dostęp, stos bez plików z hosta (Portainer z GitHuba; DOKUMENTACJA §4) |
| `bench/` | benchmark sidecara (`sidecar.mjs`, scena `scene.html`, obraz i compose, DOKUMENTACJA §8) |
| `docs/` | dokumentacja produktu: `DOKUMENTACJA.md` (sidecar §1–§8, panel §9–§15), `API.md` (Live API, API panelu i mostu), `INSTRUKCJA.md` (operator), `RUNBOOK.md` (wdrożenie, kopie, wydanie), `STATUS.md`, `LUKI.md`; `plany/` – plany prac (spis: `plany/README.md`; `PLAN-mcr.md` – jedyny plan rozwoju panelu, `native.md`, `PLAN-strefy.md`; zakończone w `plany/archiwum/`) |
| `.github/workflows/ci.yml` | testy, binarka jako artefakt |
| `.github/workflows/release.yml` | po znaczniku `v*` – jedno wydanie produktu: archiwa sidecara i panelu (Linux x86_64/arm64), obrazy `vizrs-cg`, `vizrs-seed`, `vizrs-live`, `vizrs-live-legacy` (amd64 + arm64), AppImage okna panelu, wydanie GitHub; sprawdza jedną wersję (Cargo.toml, panel, okno, most) |


### Panel (ścieżki kodu względem `panel/`; dokumentacja panelu – DOKUMENTACJA §9–§15)

| Ścieżka | Rola |
|---|---|
| `bridge/Bridge.cs` | VizRS Bridge (C#, .NET 10): hak startowy, `/vizrs-bridge/{health,commands,command/…,upload/…}`, refleksja po `ErsatzTV.Application`, serializacja wyników |
| `bridge/commands.txt` | biała lista komend legacy (jedna w wierszu, `#` – komentarz) |
| `server/src/main.rs` | serwer panelu: start, połączenia, healthcheck, stan aplikacji |
| `server/src/config.rs`, `session.rs` | opcje i `VIZRS_LIVE_*`; ciasteczko, logowanie (konto, token, pierwsze konto), limit prób, autoryzacja, API kont `/api/users` |
| `server/src/accounts.rs`, `crypto.rs` | konta i sesje w katalogu danych (`users.json`, `sessions.json`, role); SHA-256, HMAC, PBKDF2 bez crate'ów |
| `server/src/routes.rs` | routing `/api/*`, komendy ErsatzTV, stan, `STATIC` (CSRF/CSP – `http.rs`) |
| `server/src/proxy.rs`, `upload.rs`, `secrets.rs` | pośrednik ErsatzTV i CG (`/api/cg/…`, strumienie, podgląd, miniatury); przesyłanie obrazów; `mask_secrets` |
| `server/src/bridge.rs` | klient HTTP mostu i CG Playout (sieć wewnętrzna) |
| `server/src/http.rs` | minimalny HTTP/1.1: jedno żądanie na połączenie, limity rozmiaru i czasu, nagłówki bezpieczeństwa |
| `desktop/` | aplikacja VizRS CG Playout – natywny klient panelu (Tauri 2): `src/main.rs` (opcje, stan, start), `src/window.rs` (okna), `src/menu.rs`, `src/ipc.rs` (komenda `ipc`, okno połączenia); `src/gpu.rs`, `scale.rs`, `screen.rs`, `conn_url.rs` (adres i token serwera) – kopie z VizRS CG System (`scripts/sync-ui.sh`, `--check` w CI), `src/proxy.rs` (pośrednik z tokenem), `src/connect.rs` + `ui/connect.html` (okno połączenia), `src/bridge.rs` (biała lista), `tests/okno.cjs`; `tauri.conf.json` (paczki: sam klient) |
| `server/web/desktop.js` | most panelu z aplikacją `desktop/` (tytuł okna, alarm); w przeglądarce nic nie robi |
| `server/web/app.js`, `core.js` | wejście i pasek górny; DOM (`h`, `table`…), `api`/`cmd`, toast, dialog, router (`VIEWS`), etykiety `PL`, kontrola białej listy |
| `server/web/form.js`, `parts.js`, `when.js`, `tasks.js`, `check.js` | formularz z metadanych komend; `crudView`, `itemsEditor`, `contentPicker`, `dayBar`; „kiedy obowiązuje”; wyniki zadań w tle z logu legacy; kontrola ramówki |
| `server/web/views/*.js` | ekrany (po jednym pliku na grupę menu albo duży ekran; spis: DOKUMENTACJA §11 „UI”) |
| `docker/` | `legacy-bridge.Dockerfile` (oficjalny legacy + DLL + `commands.txt` + workstation GC; bez limitu sterty – strojenie Ampere A1 w nakładce CG Playout), `live.Dockerfile` (`scratch`, musl, kompilacja krzyżowa z `-C target-cpu=neoverse-n1` dla arm64), `proxy.Caddyfile` |
| `.env.example` | zmienne panelu (`VIZRS_LIVE_*`, `LEGACY_IMAGE`); wdrożenie panelu – `docker-compose.live.yml` w korzeniu repo, karta graficzna – `docker-compose.{vaapi,nvidia}.yml` |
| `compose.build.yml` | nakładka do lokalnej budowy obrazów panelu i legacy z mostem (`tests/deploy.sh`); Portainer jej nie używa |
| `scripts/check-caddy.sh` | `docker/proxy.Caddyfile` = kopia w `configs:` nakładki `docker-compose.live.yml` (CI, `deploy.sh`) |
| `tests/` | `deploy.sh` (szybkie testy + wdrożenie panelu na lokalny stos + `prod-smoke.cjs`, przejście tylko do odczytu), `stack.sh` (stos testowy, smoke, seed), `e2e.cjs` (Playwright + atrapy), `gate.sh` (pełna bramka, CI), `real.sh` (prawdziwe usługi), `mock-bridge.cjs` (atrapa mostu do zrzutów UI bez Dockera), `hwaccel.sh` (akceleracja Intel/AMD/NVIDIA w obrazie legacy – CI i wydanie) |
| `scripts/plan.sh` | automat planu: jeden punkt = jedna świeża sesja `claude -p`; dzienniki w `.plan-logs/` (poza repo) |
| `../.github/workflows/` | `panel-ci.yml` (bramka panelu), `panel-legacy-latest.yml` (co tydzień najnowsze legacy); wydanie panelu w `release.yml` produktu (jeden tag `v*`: binarka, obrazy `vizrs-live`, `vizrs-live-legacy`, AppImage) |

**Model panelu:** Przeglądarka → `POST /api/cmd/{Nazwa}` (panel, ciasteczko + `X-VizRS-Live`) → `POST /vizrs-bridge/command/{Nazwa}` (token mostu, biała lista) → komenda legacy przez MediatR. Wynik: `{"ok":true,"result":…}`, odrzucenie legacy `{"ok":false,"error":…}`, zadanie w tle `queued` (202), wynik nieodczytywalny `warning`. Formularze budują się z metadanych `GET /api/commands` (typy parametrów, wyliczenia); wyliczeń typów zagnieżdżonych most nie opisuje – listy są wtedy w pliku ekranu. Zmiany ramówki wchodzą na antenę dopiero po przebudowie playoutu. Szczegóły: [docs/DOKUMENTACJA.md](docs/DOKUMENTACJA.md) §10–§12, trasy – [docs/API.md](docs/API.md).

## Commity

<!-- pakiet:D -->
- Jedna spójna zmiana = jeden commit, po polsku, opisowo (styl historii repo). Tylko zielone stany (bramka z „Testy”).
- Pliki dodawaj jawnie (`git add ścieżka`), nie `git add -A`. Nie commituj katalogów `target/`, zrzutów, logów, plików tymczasowych, `.env` ani sekretów.
- Commit i push tylko na prośbę użytkownika.
<!-- /pakiet:D -->
- Nie commituj `config.json`, `media/` (własne media; testowe są w `seed/media/`) ani kopii `.env*`. W `seed/` nigdy sekretów ErsatzTV (`*-secrets.json`, `data-protection`) ani tokenów.
- Panel: Styl historii: „Sloty na żywo: jak nadawać – serwer i klucz RTMP, adres SRT z hasłem”; punkt planu: „R10: …”, „M2: …”. Zielony stan = `tests/deploy.sh`.
- Panel: Wyjątek: sesja automatu `scripts/plan.sh` robi lokalny commit, nigdy push.
- Panel: Nie commituj `server/target/`, `bridge/bin|obj/`, `.env`, `etv-config/`, `media/`, `tests/local.env` ani `.plan-logs/`.
