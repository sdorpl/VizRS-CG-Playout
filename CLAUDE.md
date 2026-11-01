# VizRS CG Playout

Sidecar (jeden plik binarny w Ruście + bezgłowy Chromium) łączący **ErsatzTV** z silnikiem grafiki **VizRS**: komendy TCP na start klipu,
reklamy bez grafiki i wtapianie `/output` – przez nakładkę ffmpeg (grafika wprost do ffmpeg ErsatzTV) albo element grafiki „script” ErsatzTV. Kod ErsatzTV zostaje niezmieniony.

Rodzina VizRS (repozytoria obok, `../`):
- **VizRS CG System** (`../VizRS CG System`) – silnik grafiki (Rust): Artist, Trio, `/output`, API TCP 6100 / HTTP / SSE. Plan kanału: `docs/plany/PLAN-kanal.md`.
- **VizRS ffPlayout System** (`../VizRS ffPlayout System`) – **archiwum od 2026-10-24**: porzucony własny playout 24/7 (fork ffplayout, GPL-3.0). Zostaje jako wzorzec integracji VizRS, nie jest rozwijany.
- **VizRS CG Playout** (to repo, do 2026-10-31 „VirtRS CG Playout”) – ta integracja dla ErsatzTV. Zmieniona tylko nazwa w tekstach; binarki (`virtrs-cg`, `vizrs-etv-client`, `vizrs-ffmpeg`), crate, obrazy, usługi compose, zmienne `VIRTRS_*` bez zmian; repozytorium GitHub: `sdorpl/VizRS-CG-Playout`.

Język pracy z użytkownikiem i dokumentacji: **polski**. Kod, komentarze i komunikaty logu są po angielsku — pisz zgodnie z otoczeniem.

## Zacznij każdą sesję tutaj

1. `git log -5`, `git status`.
2. Przy zmianie zachowania przeczytaj właściwą sekcję [docs/DOKUMENTACJA.md](docs/DOKUMENTACJA.md) — w tym samym commicie ją zaktualizujesz.
3. Przy komendach VizRS sprawdź składnię w `../VizRS CG System/docs/API.md` (sekcja TCP).

## Niepodważalne zasady

- **Z VizRS tylko przez protokół**: TCP 6100 (`AUTH`, `STATE`, `TAKE`, `OUT`, …), strona `/output` i proces `vizrs-render stream` (renderer natywny VizRS, protokół klatek w VizRS DOKUMENTACJA §10.1; jego kod zostaje w VizRS). Nie kopiuj kodu między repozytoriami rodziny (różne licencje; ffPlayout jest GPL-3.0). Wyjątek (zgoda autora, 2026-10-25): wolno przenosić kod z modułów VizRS w ffPlayout napisanych wyłącznie przez autora (sdorpl) – `crates/app/src/utils/vizrs.rs`, `vizrs_capture.rs`, `crates/engine/src/compositor/graphics.rs` – sprawdź `git log` pliku. Kodu upstream ffplayout (inni autorzy) nadal nie.
- **Wzorzec VizRS ffPlayout System (archiwum)**: opcje (`on_clip`, `on_ad`, `output_url`, adres/token), symbole zastępcze i zachowanie reklam pochodzą z ffPlayout i zostają zgodne, żeby szablony komend dało się przenieść. ffPlayout nie jest już rozwijany: nowe funkcje projektuj tutaj, niczego tam nie dopasowuj (tabela w DOKUMENTACJA §2 to mapa pojęć).
- **ErsatzTV bez zmian w kodzie**: integracja tylko przez element „script” (`etv/templates/graphics-elements/script/vizrs.yml`) z klientem `vizrs-etv-client` i nakładkę ffmpeg `vizrs-ffmpeg` ustawioną jako ścieżka FFmpeg – obie to statyczne binarki musl z tego crate'a, budowane w obrazie sidecara na amd64 i arm64 i instalowane przez sidecara (obraz ErsatzTV nie ma Pythona na arm64 i niczego nie doinstalowujemy). Nakładka zmienia wyłącznie wejście płótna grafiki (plus jawnie włączone `VIZRS_FFMPEG_FPS`/`VIZRS_FFMPEG_PRESET`/`VIZRS_FFMPEG_READRATE_CATCHUP`/`VIZRS_FFMPEG_FPS_BLEND`); każde inne wywołanie i każdy błąd = oryginalna komenda.
- **Grafika nigdy nie zatrzymuje emisji**: komendy w tle z limitami czasu, błędy tylko do logu, awaria Chromium = czyste klatki i restart, brak sidecara = klip bez grafiki.
- **Bez GPU na ścieżce grafiki**: Chromium renderuje programowo; docelowo także Oracle Cloud Ampere A1 (arm64) – pamiętaj o budżecie CPU. Obraz buduje się na amd64 i arm64.
- **Mało zależności**: zależności runtime to tylko `png` i `regex` (bez domyślnych funkcji; wersje w `Cargo.lock`, budowa z `--locked`). Nowej nie dodawaj bez potrzeby – JSON, HTTP, base64, DevTools i skalowanie są własne. Testy kontraktu w Node.js – tylko biblioteka standardowa.
- **Gdzie co opisywać**: README tylko wprowadza i odsyła; opis działania, opcje i zmienne środowiskowe tylko w `docs/DOKUMENTACJA.md`. Nowa opcja = `src/config.rs` + `config.example.json` (gdy dotyczy) + `docker-compose.yml` + DOKUMENTACJA §5 + test.

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
| `src/native.rs` | renderer natywny VizRS (`renderer: native\|auto`): proces `vizrs-render stream`, rekordy klatek, prostokąty dla historii (DOKUMENTACJA §1) |
| `src/frames.rs`, `src/png.rs`, `src/rects.rs`, `src/scale.rs`, `src/area.rs` | historia klatek, dekodowanie PNG, widoczne prostokąty, skalowanie jak libvips, obszar grafiki (`area`) |
| `src/packets.rs`, `src/stream.rs`, `src/yuva.rs` | pakiety ETV (`Rectangles`/`Repeat`), tempo strumienia i `lead_seconds`, klatki `yuva420p` dla nakładki ffmpeg |
| `src/etv_client.rs`, `src/bin/vizrs-etv-client.rs` | klient elementu „script” w kontenerze ErsatzTV: stdin → sidecar → stdout, w tle synchronizacja elementów stron |
| `src/etv_sync.rs` | sidecar instaluje programy ErsatzTV i `vizrs.yml`, a cyklicznie synchronizuje kanały i elementy stron (DOKUMENTACJA §4) |
| `src/sync.rs` | strony VizRS → elementy `page-<ID>.yml` ErsatzTV (także `virtrs-cg sync`); plików nie kasuje, bo ErsatzTV usunąłby przypisania (DOKUMENTACJA §3) |
| `src/net.rs` | wspólny klient HTTP programów ErsatzTV (URL, połączenia z limitami czasu) |
| `src/overlay.rs`, `src/bin/vizrs-ffmpeg.rs` | nakładka ffmpeg ErsatzTV (binarka `vizrs-ffmpeg`): płótno grafiki zastąpione klatkami `yuva420p` z sidecara (DOKUMENTACJA §1) |
| `etv/templates/…/vizrs.yml` | definicja elementu grafiki „VizRS” dla ErsatzTV |
| `Dockerfile` | obraz sidecara: binarka i `vizrs-render` z wydania VizRS (`VIZRS_REF`; arm64 krzyżowo) + `chromium-headless-shell` |
| `tests/*.rs`, `tests/vectors/` | testy Rusta, wektory referencyjne |
| `tests/contract/` | testy kontraktu w Node.js (`npm test`) |
| `seed/` | dane startowe stosu: show VizRS, baza ErsatzTV (kanał 1), media testowe; obraz `vizrs-seed` z wydania – `init.sh` wypełnia puste wolumeny i daje sidecarowi dostęp, stos bez plików z hosta (Portainer z GitHuba; DOKUMENTACJA §4) |
| `bench/` | benchmark sidecara (`sidecar.mjs`, scena `scene.html`, obraz i compose, DOKUMENTACJA §8) |
| `docs/plany/` | plany prac (`PLAN-rust.md` – migracja z Node.js, `PLAN-plynnosc.md` – płynność animacji, `native.md` – natywny renderer VizRS zamiast Chromium, `PLAN-ampere.md` – emisja w 2 OCPU) |
| `.github/workflows/ci.yml` | testy, binarka jako artefakt |
| `.github/workflows/release.yml` | po znaczniku `v*`: archiwa Linux x86_64/arm64, obraz amd64 + arm64 do ghcr, wydanie GitHub |

## Commity

- Jedna spójna zmiana = jeden commit, po polsku, opisowo. Tylko zielone stany.
- Pliki dodawaj jawnie (`git add ścieżka`). Nie commituj `config.json`, `target/`, `media/` (własne media; testowe są w `seed/media/`), zrzutów PNG ani logów. W `seed/` nigdy sekretów ErsatzTV (`*-secrets.json`, `data-protection`) ani tokenów.
- Commit i push tylko na prośbę użytkownika.
