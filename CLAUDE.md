# VizRS CG Playout

Sidecar (jeden plik binarny w Ruście + bezgłowy Chromium) łączący **ErsatzTV** z silnikiem grafiki **VizRS**: komendy TCP na start klipu,
reklamy bez grafiki i wtapianie `/output` – przez nakładkę ffmpeg (grafika wprost do ffmpeg ErsatzTV) albo element grafiki „script” ErsatzTV –
oraz live ingest (RTMP/SRT przejmuje kanał, jak w ffPlayout). Kod ErsatzTV zostaje niezmieniony.

Pakiet VizRS (repozytoria obok, `../`) – aplikacje pakietu mają być **zawsze ze sobą zgodne** (DOKUMENTACJA §1 „Pakiet VizRS”); zmiana styku = dostosowanie drugiej strony w tym samym cyklu:
- **VizRS CG System** (`../VizRS CG System`) – silnik grafiki (Rust): Trio, `/output`, API TCP 6100 / HTTP / SSE. Plan kanału: `docs/plany/PLAN-kanal.md`.
- **VizRS CG Playout** (to repo, do 2026-10-31 „VirtRS CG Playout”) – ta integracja dla ErsatzTV. Zmieniona tylko nazwa w tekstach; od 2.6.0 binarka, crate i obraz to `vizrs-cg` (wcześniej `virtrs-cg`); usługi compose i zmienne `VIRTRS_*` bez zmian; repozytorium GitHub: `sdorpl/VizRS-CG-Playout`.
- **VizRS Live** (`../VizRS Live`) – panel kanału nad ErsatzTV legacy; korzysta z live API tego repo (`/api/live/*`, `/health`, `/etv/pages`, `/ingest/state`) i ma nakładkę `compose.cg-playout.yml` na stos stąd.
- **VizRS CG Artist** (`../VizRS CG Artist`) – edytor grafik i animacji; sceny wysyła na serwer VizRS CG System, z tym repo nie styka się bezpośrednio.
- **VizRS ffPlayout System** (`../VizRS ffPlayout System`) – **archiwum od 2026-10-24**: porzucony własny playout 24/7 (fork ffplayout, GPL-3.0). Zostaje jako wzorzec integracji VizRS, nie jest rozwijany.

Język pracy z użytkownikiem i dokumentacji: **polski**. Kod, komentarze i komunikaty logu są po angielsku — pisz zgodnie z otoczeniem.

## Zacznij każdą sesję tutaj

1. `git log -5`, `git status`.
2. Przy zmianie zachowania przeczytaj właściwą sekcję [docs/DOKUMENTACJA.md](docs/DOKUMENTACJA.md) — w tym samym commicie ją zaktualizujesz.
3. Przy komendach VizRS sprawdź składnię w `../VizRS CG System/docs/API.md` (sekcja TCP).

## Niepodważalne zasady

- **Numer wersji: rok.miesiąc.rewizja** (`26.11.1` = listopad 2026, pierwsze wydanie w miesiącu; kolejne `26.11.2`, w grudniu od `26.12.1`), bez zer wiodących (semver: Cargo, obrazy). Znacznik `v<wersja>` = `version` w `Cargo.toml`. Wydania idą z gałęzi `main`; `main-oldstable` to stan sprzed 26.11.1.
- **Z VizRS tylko przez protokół**: TCP 6100 (`AUTH`, `STATE`, `TAKE`, `OUT`, …), strona `/output` i proces `vizrs-render stream` (renderer natywny VizRS, protokół klatek w VizRS DOKUMENTACJA §10.1; jego kod zostaje w VizRS). Nie kopiuj kodu między repozytoriami rodziny (różne licencje; ffPlayout jest GPL-3.0). Wyjątek (zgoda autora, 2026-10-25): wolno przenosić kod z modułów VizRS w ffPlayout napisanych wyłącznie przez autora (sdorpl) – `crates/app/src/utils/vizrs.rs`, `vizrs_capture.rs`, `crates/engine/src/compositor/graphics.rs` – sprawdź `git log` pliku. Kodu upstream ffplayout (inni autorzy) nadal nie.
- **Wzorzec VizRS ffPlayout System (archiwum)**: opcje (`on_clip`, `on_ad`, `output_url`, adres/token), symbole zastępcze i zachowanie reklam pochodzą z ffPlayout i zostają zgodne, żeby szablony komend dało się przenieść. ffPlayout nie jest już rozwijany: nowe funkcje projektuj tutaj, niczego tam nie dopasowuj (tabela w DOKUMENTACJA §2 to mapa pojęć).
- **ErsatzTV bez zmian w kodzie**: integracja tylko przez element „script” (`etv/templates/graphics-elements/script/vizrs.yml`) z klientem `vizrs-etv-client` i nakładkę ffmpeg `vizrs-ffmpeg` ustawioną jako ścieżka FFmpeg – obie to statyczne binarki musl z tego crate'a, budowane w obrazie sidecara na amd64 i arm64 i instalowane przez sidecara (obraz ErsatzTV nie ma Pythona na arm64 i niczego nie doinstalowujemy). Nakładka zmienia wyłącznie wejście płótna grafiki (plus jawnie włączone `VIZRS_FFMPEG_FPS`/`VIZRS_FFMPEG_PRESET`/`VIZRS_FFMPEG_X264_PARAMS` (z `VIZRS_FFMPEG_NVENC_BF` tylko pierwsze wypełnienie `encoder.json`: presety x264/NVENC/QSV, profil H.264, opcje x264 i klatki B koderów sprzętowych ustawia VizRS Live)/`VIZRS_FFMPEG_READRATE_CATCHUP`/`VIZRS_FFMPEG_FPS_BLEND`/`VIZRS_FFMPEG_CUDA`/`VIZRS_FFMPEG_NVENC_BF`/`VIZRS_FFMPEG_NO_SHORTEST`/`VIZRS_FFMPEG_SYNC_RAM`/`VIZRS_FFMPEG_NO_DISCARDCORRUPT`/`VIZRS_FFMPEG_INGEST` – ten ostatni dzieli kawałek na przebiegi kawałek/live do tej samej playlisty, tylko gdy sidecar ma sygnał live kanału); w płótnie z jednym `-init_hw_device cuda` grafika idzie przez `hwupload` zamiast `hwupload_cuda` (bez osobnego kontekstu CUDA na strefę); każde inne wywołanie i każdy błąd = oryginalna komenda.
- **Grafika nigdy nie zatrzymuje emisji**: komendy w tle z limitami czasu, błędy tylko do logu, awaria Chromium = czyste klatki i restart, brak sidecara = klip bez grafiki.
- **Bez GPU na ścieżce grafiki**: Chromium / vizrs-render renderuje programowo; na Oracle Cloud Ampere A1 (arm64) kompilacja binarek (`vizrs-cg`, `vizrs-ffmpeg`, `vizrs-etv-client`, `vizrs-render`) wymusza flagi mikroarchitektury `RUSTFLAGS="-C target-cpu=neoverse-n1 -C opt-level=3"` oraz dostrojenie .NET CLR (`DOTNET_gcServer=0`, limit sterty 1.5 GB). Obraz buduje się na amd64 i arm64 bez emulacji QEMU.
- **Mało zależności**: zależności runtime to tylko `png` i `regex` (bez domyślnych funkcji; wersje w `Cargo.lock`, budowa z `--locked`). Nowej nie dodawaj bez potrzeby – JSON, HTTP, base64, DevTools i skalowanie są własne. Testy kontraktu w Node.js – tylko biblioteka standardowa.
- **Gdzie co opisywać**: README tylko wprowadza i odsyła; opis działania, opcje i zmienne środowiskowe tylko w `docs/DOKUMENTACJA.md`. Nowa opcja = `src/config.rs` + `config.example.json` (gdy dotyczy) + `docker-compose.yml` + DOKUMENTACJA §5 + test.

## Współpraca z Gemini

To repo nie ma własnego `docs/AGENCI.md` – obowiązuje wspólny podział pakietu VizRS opisany w `../VizRS Live/docs/AGENCI.md` (tam też „AGENCI §4/§5” niżej). W skrócie:

- **Claude dowodzi i jako jedyny pisze kod** (`src/**`, `tests/**`, `Cargo.toml`, `Dockerfile`, `docker-compose*.yml`, `etv/`, `seed/`, `bench/`, `.github/`, `config.example.json`, `CLAUDE.md`, `GEMINI.md`) oraz plany w `docs/plany/`.
- **Gemini wykonuje lekkie zadania:** redaguje wskazane pliki `.md`, uruchamia bramkę (`cargo fmt --check`, `cargo clippy`, `cargo test`, `npm test`) z krótkim raportem, streszcza długie wyjścia, przeszukuje źródła (także ErsatzTV i `../VizRS CG System`). Nie commituje i nie rusza działających stosów Dockera.
- **Kontekst Gemini:** `AGENTS.md` to kopia tego pliku (nie dowiązanie: Portainer GitOps odrzuca repozytorium z symlinkiem) – każdą zmianę CLAUDE.md przenieś do niej w tym samym commicie (`cp CLAUDE.md AGENTS.md`); `agy` sam wczytuje `AGENTS.md` i `GEMINI.md`, także w `-p` (pomiar agy 1.5.3, 2026-11-08: plik 19 KB w całości); importów `@plik` nie rozwija. Zasady zmieniaj tylko tutaj, `GEMINI.md` opisuje wyłącznie rolę wykonawcy i niczego stąd nie powiela.
- **Wywołanie:** `~/.gemini/bin/agy` (poza `PATH`), przełączniki – AGENCI §5. Wygodniej przez wrapper wtyczki antigravity: `PATH="$HOME/.gemini/bin:$PATH" agy-delegate [--tier flash|flash-lo] [--digest] [--mode plan] --dir . "<zlecenie>"`. Wrapper sam stawia `-p` na końcu i odcina stdin. Zamiast kodu 0 zwraca kod błędu: 10 – limit, 11 – logowanie, 12 – przekroczony czas, 14 – brak modelu, 15 – odmowa uprawnień, 3 – pusta odpowiedź. Na stderr wypisuje wiersz `AGY_USAGE` z tokenami i modelem. Tylko subskrypcja Google (bez kluczy API). `--yolo` / `--dangerously-skip-permissions` – nigdy: daje zgodę na cały komputer, a `--sandbox` jej nie ogranicza.
- **Próg opłacalności:** każde zlecenie ma koszt stały (zlecenie, czekanie, przegląd). Małe, jednorazowe i wymagające decyzji zadania rób sam – ich zlecanie to zmierzona strata. Zlecaj duże porcje tekstu przy niewielu decyzjach: prozę dokumentacji (też w kilku repozytoriach naraz, AGENCI §5), przeszukanie dużych źródeł, nieustrukturyzowane logi. Jedno duże zlecenie zamiast wielu małych. Bierz streszczenie (`--digest`), nie surowe wyjście; plików, które przerobił Gemini, nie czytaj ponownie – przeglądaj diff. Logi (bramka, kompilacja, testy) czytaj sam przez plik i filtr (`tail`, `grep`) – AGENCI §4.
- **Weryfikacja:** przed zleceniem migawka `git status` (i `git diff`, gdy pliki mają już zmiany), po zleceniu przegląd diffu – razem z usuniętymi wierszami. Kod 0 i raport „gotowe” niczego nie dowodzą. Bramkę uruchom sam, w czystym stanie – `agy` potrafi zmienić środowisko (połatać pakiety, podstawić atrapy), żeby test przeszedł. Każdą liczbę, ścieżkę i stan etapu sprawdź z kodem (Gemini dopowiada fakty).

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
| `src/frames.rs`, `src/png.rs`, `src/rects.rs`, `src/scale.rs`, `src/area.rs`, `src/footprint.rs` | historia klatek, dekodowanie PNG, widoczne prostokąty, skalowanie jak libvips, obszar grafiki (`area`), ślady stron i strefy `area: auto` (komórki 16 px, PLAN-strefy) |
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

- Jedna spójna zmiana = jeden commit, po polsku, opisowo. Tylko zielone stany.
- Pliki dodawaj jawnie (`git add ścieżka`). Nie commituj `config.json`, `target/`, `media/` (własne media; testowe są w `seed/media/`), zrzutów PNG ani logów. W `seed/` nigdy sekretów ErsatzTV (`*-secrets.json`, `data-protection`) ani tokenów.
- Commit i push tylko na prośbę użytkownika.
