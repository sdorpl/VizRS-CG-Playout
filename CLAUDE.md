# VizRS CG Playout

Sidecar (jeden plik binarny w Ruście + bezgłowy Chromium) łączący **ErsatzTV** z silnikiem grafiki **VizRS**: komendy TCP na start klipu,
reklamy bez grafiki i wtapianie `/output` – przez nakładkę ffmpeg (grafika wprost do ffmpeg ErsatzTV) albo element grafiki „script” ErsatzTV –
oraz live ingest (RTMP/SRT przejmuje kanał). Kod ErsatzTV zostaje niezmieniony.

Kod, komentarze i komunikaty logu są po angielsku; dokumentacja i rozmowa po polsku.

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
- Przy komendach VizRS sprawdź składnię w `../VizRS CG System/docs/API.md` (sekcja TCP).

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

- **Manifesty wersji:** znacznik `v<wersja>` = `version` w `Cargo.toml` i `package.json`.
- **Z VizRS CG System tylko przez protokół:** TCP 6100 (`AUTH`, `STATE`, `TAKE`, `OUT`, …), `GET /api/show`, strona `/output` i proces `vizrs-render stream` / `bounds` / `thumb` (renderer natywny, protokół klatek w VizRS CG System DOKUMENTACJA §10.1; jego kod zostaje w CG System – obraz `vizrs-cg` buduje go z tagu `VIZRS_REF` w `Dockerfile`).
- **ErsatzTV bez zmian w kodzie**: integracja tylko przez element „script” (`etv/templates/graphics-elements/script/vizrs.yml`) z klientem `vizrs-etv-client` i nakładkę ffmpeg `vizrs-ffmpeg` ustawioną jako ścieżka FFmpeg – obie to statyczne binarki musl z tego crate'a, budowane w obrazie sidecara na amd64 i arm64 i instalowane przez sidecara (obraz ErsatzTV nie ma Pythona na arm64 i niczego nie doinstalowujemy). Nakładka zmienia wyłącznie wejście płótna grafiki (plus jawnie włączone `VIZRS_FFMPEG_FPS`/`VIZRS_FFMPEG_PRESET`/`VIZRS_FFMPEG_X264_PARAMS` (z `VIZRS_FFMPEG_NVENC_BF` tylko pierwsze wypełnienie `encoder.json`: presety x264/NVENC/QSV, profil H.264, opcje x264 i klatki B koderów sprzętowych ustawia VizRS Live)/`VIZRS_FFMPEG_READRATE_CATCHUP`/`VIZRS_FFMPEG_FPS_BLEND`/`VIZRS_FFMPEG_CUDA`/`VIZRS_FFMPEG_NVENC_BF`/`VIZRS_FFMPEG_NO_SHORTEST`/`VIZRS_FFMPEG_SYNC_RAM`/`VIZRS_FFMPEG_NO_DISCARDCORRUPT`/`VIZRS_FFMPEG_INGEST` – ten ostatni dzieli kawałek na przebiegi kawałek/live do tej samej playlisty, tylko gdy sidecar ma sygnał live kanału); w płótnie z jednym `-init_hw_device cuda` grafika idzie przez `hwupload` zamiast `hwupload_cuda` (bez osobnego kontekstu CUDA na strefę); każde inne wywołanie i każdy błąd = oryginalna komenda.
- **Grafika nigdy nie zatrzymuje emisji**: komendy w tle z limitami czasu, błędy tylko do logu, awaria Chromium = czyste klatki i restart, brak sidecara = klip bez grafiki.
- **Bez GPU na ścieżce grafiki**: Chromium / vizrs-render renderuje programowo; na Oracle Cloud Ampere A1 (arm64) kompilacja binarek (`vizrs-cg`, `vizrs-ffmpeg`, `vizrs-etv-client`, `vizrs-render`) wymusza flagi mikroarchitektury `RUSTFLAGS="-C target-cpu=neoverse-n1 -C opt-level=3"` oraz dostrojenie .NET CLR (`DOTNET_gcServer=0`, limit sterty 1.5 GB). Obraz buduje się na amd64 i arm64 bez emulacji QEMU.
- **Mało zależności**: zależności runtime to tylko `png` i `regex` (bez domyślnych funkcji; wersje w `Cargo.lock`, budowa z `--locked`). Nowej nie dodawaj bez potrzeby – JSON, HTTP, base64, DevTools i skalowanie są własne. Testy kontraktu w Node.js – tylko biblioteka standardowa.
- **Gdzie co opisywać (uzupełnienie):** Live API, którego to repo jest dostawcą (`/api/live/*`, `/health`, `/etv/pages`, `/ingest/*`) – DOKUMENTACJA §1 „Live API” (docelowo `docs/API.md`). Nowa opcja = `src/config.rs` + `config.example.json` (gdy dotyczy) + `docker-compose.yml` + DOKUMENTACJA §5 + test.

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
- **Dokumentacja (Gemini – tylko pliki wskazane w zleceniu):** `README.md`, `CHANGELOG.md`, `docs/DOKUMENTACJA.md`, `docs/STATUS.md`, `docs/LUKI.md`.
- **Bramka (Gemini może uruchomić i streścić):** `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo build --release`, `npm test`, `tools/sync-zasady.sh --check`.
- **Zawsze:** stosów produkcyjnych (Portainer, serwery emisji) i lokalnego stosu użytkownika nie ruszaj; po testach na Dockerze sprzątaj tylko to, co powstało na potrzeby testu – bez `down -v` i `volume prune`.

## Uruchamianie

```bash
docker compose up -d                       # VizRS :8788, sidecar :7700, ErsatzTV :8409
cargo run --release -- config.json         # bez Dockera (Rust 1.87+, Chromium w PATH albo VIZRS_BROWSER)
```

Nakładki: `docker-compose.ampere.yml` (Oracle Ampere A1), `docker-compose.nvidia.yml` (NVENC w ErsatzTV), `docker-compose.vaapi.yml` (VAAPI w ErsatzTV: Intel, AMD).

## Testy (bramka przed commitem)

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings
cargo test                                 # moduły, wektory tests/vectors/, Chromium (pomijany bez przeglądarki)
cargo build --release && npm test          # kontrakt: binarka jako czarna skrzynka (tests/contract/)
tools/sync-zasady.sh --check                # zasady pakietu = CG System (wersja w docs/ZASADY_CG_SYSTEM)
```

Wektory `tests/vectors/*.json` to zachowanie sidecara Node.js sprzed migracji (generator w historii gita, commit 0c54143); po zamierzonej zmianie zachowania popraw wektor w tym samym commicie. Testy kontraktu (udawany VizRS, klient `vizrs-etv-client`, nakładka `vizrs-ffmpeg`; `VIRTRS_CG_BIN`/`VIZRS_ETV_CLIENT_BIN`/`VIZRS_FFMPEG_BIN` wskazują inne binarki) obowiązują każdą zmianę protokołu.

Po testach na stosie Dockera zawsze sprzątaj to, co powstało na potrzeby testu (kontenery, obrazy, wolumeny, pliki), ale **dane działających instancji tego projektu zostawiaj** – bez `down -v` i `volume prune`. Szczegóły: DOKUMENTACJA §7.

## Struktura

| Ścieżka | Rola |
|---|---|
| `src/main.rs` | start: konfiguracja, serwer, `SIGTERM` (zamyka przeglądarki) |
| `src/config.rs` | domyślne opcje, plik JSON, zmienne środowiskowe |
| `src/server.rs` | HTTP: `POST /etv?channel=N` (strumień pakietów), `GET /etv/pages` (strony VizRS), `GET /ffmpeg`, `GET /ffmpeg/frames` (nakładka ffmpeg), `GET /health`; przechwytywanie na rozmiar, przejmowanie sesji |
| `src/http.rs`, `src/json.rs` | własny minimalny HTTP/1.1 (strumienie chunked jak w Node; HTTP/1.0 dla nakładki) i JSON |
| `src/template.rs`, `src/time.rs` | kontekst klipu z danych szablonu ErsatzTV, symbole `{title}`, `{next_*}`…, wykrywanie reklam (`regex`), daty ISO |
| `src/vizrs.rs` | klient TCP VizRS (`AUTH`, `STATE`, komendy), zdarzenia kanału w kolejności, zdejmowanie/przywracanie stron przy reklamach |
| `src/capture.rs`, `src/devtools.rs`, `src/sys.rs` | bezgłowy Chromium (screencast DevTools albo klatki na żądanie `beginFrame`, `capture_mode`, przez `--remote-debugging-pipe`), restart po awarii, `PR_SET_PDEATHSIG`, sygnały (też dla nakładki), zegar monotoniczny |
| `src/native.rs` | renderer natywny VizRS (`renderer: native\|auto`): proces `vizrs-render stream` na warstwę show (`?layers=`, `native_layers`; kolejność `layerZ`, mieszanie nachodzących warstw) albo jeden na całą antenę, rekordy klatek, prostokąty dla historii (DOKUMENTACJA §1) |
| `src/frames.rs`, `src/png.rs`, `src/rects.rs`, `src/scale.rs`, `src/area.rs`, `src/footprint.rs`, `src/bounds.rs` | historia klatek, dekodowanie PNG, widoczne prostokąty, skalowanie jak libvips, obszar grafiki (`area`), ślady stron i strefy `area: auto` (komórki 16 px, PLAN-strefy), granice stron z VizRS (`vizrs-render bounds`, PLAN-strefy S6) |
| `src/packets.rs`, `src/stream.rs`, `src/yuva.rs`, `src/delta.rs` | pakiety ETV (`Rectangles`/`Repeat`; piksele `Arc`, wspólne między obrazami), tempo strumienia i `lead_seconds`, wstrzymanie strumienia przed obrazem spoza stref, klatki `yuva420p` dla nakładki ffmpeg (z poprzedniej klatki, liczby całkowite, wątki przy dużej zmianie), delta klatek do `vizrs-ffmpeg` (tylko zmienione wiersze) |
| `src/etv_client.rs`, `src/bin/vizrs-etv-client.rs` | klient elementu „script” w kontenerze ErsatzTV: stdin → sidecar → stdout, w tle synchronizacja elementów stron |
| `src/etv_sync.rs` | sidecar instaluje programy ErsatzTV i `vizrs.yml`, a cyklicznie synchronizuje kanały i elementy stron (DOKUMENTACJA §4) |
| `src/sync.rs` | strony VizRS → elementy `page-<ID>.yml` ErsatzTV (także `vizrs-cg sync`); plików nie kasuje, bo ErsatzTV usunąłby przypisania (DOKUMENTACJA §3) |
| `src/net.rs` | wspólny klient HTTP programów ErsatzTV (URL, połączenia z limitami czasu) |
| `src/overlay.rs`, `src/bin/vizrs-ffmpeg.rs` | nakładka ffmpeg ErsatzTV (binarka `vizrs-ffmpeg`): płótno grafiki zastąpione klatkami `yuva420p` z sidecara (DOKUMENTACJA §1) |
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
| `docs/plany/` | plany prac (`PLAN-rust.md` – migracja z Node.js, `PLAN-plynnosc.md` – płynność animacji, `native.md` – natywny renderer VizRS zamiast Chromium, `PLAN-ampere.md` – emisja w 2 OCPU, `PLAN-live-panel.md` – dawny VizRS Live Panel, zamknięty: UI przeniesione do VizRS Live, tu zostaje live API, `PLAN-strefy.md` – strefy grafiki liczone z tego, co jest na antenie, `VIZRS_AREA=auto`, `PLAN-p400.md` – lokalny stos na Quadro P400: pamięć karty, NVENC, `VIZRS_FFMPEG_CUDA=auto`) |
| `.github/workflows/ci.yml` | testy, binarka jako artefakt |
| `.github/workflows/release.yml` | po znaczniku `v*`: archiwa Linux x86_64/arm64, obraz amd64 + arm64 do ghcr, wydanie GitHub |

## Commity

<!-- pakiet:D -->
- Jedna spójna zmiana = jeden commit, po polsku, opisowo (styl historii repo). Tylko zielone stany (bramka z „Testy”).
- Pliki dodawaj jawnie (`git add ścieżka`), nie `git add -A`. Nie commituj katalogów `target/`, zrzutów, logów, plików tymczasowych, `.env` ani sekretów.
- Commit i push tylko na prośbę użytkownika.
<!-- /pakiet:D -->
- Nie commituj `config.json`, `media/` (własne media; testowe są w `seed/media/`) ani kopii `.env*`. W `seed/` nigdy sekretów ErsatzTV (`*-secrets.json`, `data-protection`) ani tokenów.
