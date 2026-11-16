# PLAN: migracja sidecara VizRS CG Playout z Node.js do Rusta

> Status (2026-10-25): **etapy 0–9 zrobione na `rust-edit`** – sidecar jest w Ruście w korzeniu repozytorium, kod Node.js usunięty, Node.js zostaje tylko do testów kontraktu i benchmarku. Otwarte: pomiar na Ampere A1 (`bench/`, DOKUMENTACJA §8), scalenie do `main` i wydanie.
> Gałąź robocza: **`rust-edit`**. `main` do etapu 8 zostaje wydawalny na Node.js.
> Powiązane: [DOKUMENTACJA.md](../../DOKUMENTACJA.md) (opis działania – kontrakt, którego migracja nie zmienia).

## 0. Cel

Sidecar (`bin/` + `src/*.js`, ok. 1700 linii, zależność `sharp`) staje się jednym plikiem binarnym w Ruście, tak jak silnik VizRS CG System. Zyski:

- mniej CPU i pamięci obok Chromium, co ma znaczenie na Oracle Ampere A1 (dziś Node ma ok. 60–80 MB RSS, cel to ok. 10 MB);
- znika natywna zależność libvips (`sharp`) i cały runtime Node w obrazie;
- przetwarzanie klatek (`visibleRects`, `yuva420p`, skalowanie) w kodzie kompilowanym zamiast pętli JS. Dla nakładki ffmpeg przy 50 kl./s i 1080p to najdroższy fragment poza Chromium.

## 1. Kontrakt – bez zmian

| Element | Gdzie opisany |
|---|---|
| `POST /etv?channel=N[&render=…]`, `GET /ffmpeg?channel=N[&wait=ms]`, `GET /ffmpeg/frames?session=N[&fps=…]`, `GET /health` | DOKUMENTACJA §1, `src/server.js` |
| pakiety ETVG (`Clear`, `Repeat`, `Rectangles`) i klatki `yuva420p` – bajt w bajt | `src/packets.js`, `src/yuva.js` |
| opcje `config.json`, zmienne środowiskowe (`VIZRS_ON_*` z `\n`), domyślne wartości | DOKUMENTACJA §5, `src/config.js` |
| symbole zastępcze, wykrywanie reklam, zdejmowanie i przywracanie stron | DOKUMENTACJA §3, `src/template.js`, `src/vizrs.js` |
| format logu (`<ISO> INFO …`, `DEBUG=1`) | `bin/virtrs-cg.js` |
| `client/*.py`, `etv/…/vizrs.yml`, `seed/`, compose (poza obrazem sidecara) | bez zmian – działają w kontenerze ErsatzTV |

## 2. Decyzje techniczne

| Obszar | Node.js | Rust | Uzasadnienie |
|---|---|---|---|
| współbieżność | pętla zdarzeń | `std::thread`, `mpsc`, `Condvar`; bez tokio | kilka kanałów i sesji; blokujący zapis daje backpressure |
| HTTP | `node:http` | własny minimalny HTTP/1.1 na `TcpListener` | 4 trasy; strumienie jak w Node: na HTTP/1.1 chunked (`http.client` Pythona gubi gniazdo przy odpowiedzi kończonej rozłączeniem, co sprawdziłem w etapie 3), na HTTP/1.0 (nakładka) surowe bajty i `Connection: close`; nagłówki wysyłane od razu |
| DevTools | WebSocket + `DevToolsActivePort` | `--remote-debugging-pipe` (fd 3/4, JSON zakończony `\0`) | bez klienta WebSocket i odpytywania pliku |
| koniec Chromium | `SIGKILL` przy zamykaniu | `PR_SET_PDEATHSIG` w `pre_exec` | przeglądarka kończy się z sidecarem bez obsługi sygnałów |
| PNG | `sharp` (libvips) | crate `png` | czysty Rust, szybki inflate (fdeflate) |
| skalowanie `render_size` | `sharp`, Lanczos z premultiplikacją | własny rozdzielny Lanczos3 z premultiplikacją | znika ostatnia zależność natywna |
| `ad_match` | `RegExp(…, "i")` | crate `regex` bez domyślnych funkcji (`std`, `unicode-case`, `unicode-perl`; zależy tylko od `regex-automata` i `regex-syntax`) | `regex-lite` zamienia wielkość liter tylko w ASCII, a `/ogłoszenia/i` w JS pasuje do „OGŁOSZENIA” (wektor w `template.json`) |
| JSON | wbudowany | własny moduł (parsowanie + zapis) | jak w VizRS CG System, ale napisany od nowa – bez kopiowania kodu |
| base64 | `Buffer` | własna funkcja | ok. 20 linii |

Zależności runtime: **`png` + `regex`** (wersje przypięte w `Cargo.lock`, CI buduje z `--locked`). Zasada „Mało zależności” w CLAUDE.md zmienia się przy scaleniu.

Model wątków:
- **kanał** (`ChannelEvents`): wątek z kolejką `mpsc` w miejsce łańcucha `Promise` – ta sama kolejność komend; przerwa reklamowa (`GAP_GRACE`) przez `recv_timeout`;
- **przechwytywanie**: wątek czytający rurę DevTools i wątek dekodujący. Skrzynka trzyma tylko najnowszy PNG (jak `pendingPng`). Historia to `Mutex<VecDeque<Arc<Frame>>>`, pakiet i `yuva` w `OnceLock` na klatce;
- **sesja**: wątek na połączenie, tempo z `Instant`; wstrzymanie i wznowienie (przejęcie przez nakładkę) przez `Condvar`.

## 3. Testy: najpierw kontrakt

1. **Czarna skrzynka.** `test/integration.test.mjs` uruchamia binarkę z `VIRTRS_CG_BIN` (domyślnie `node bin/virtrs-cg.js`) zamiast wołać `createServer()` w procesie. Rozmiary renderowania i klatkaż sprawdza przez `/health` zamiast atrapy przechwytywania. Przeglądarką jest `/bin/false`, więc klatki są przezroczyste. Treść grafiki sprawdza `chromium.test.mjs` na prawdziwym Chromium: pakiety ETV i `yuva420p` przez proces sidecara. Wymagania wobec budowy Rusta: `/health` z polami `sessions`, `captures[].size`, `render`, `fps`, `status`, `seq`; ukryta opcja `gap_grace_ms` w pliku konfiguracji; nieudany start przeglądarki = przezroczyste klatki i ponowna próba.
2. **Wektory referencyjne.** `test/vectors/generate.mjs` zapisuje `test/vectors/*.json` z obecnej implementacji: `template` (kontekst klipu, komendy, TimeSpan, zegar, reklamy), `vizrs` (`onAir`), `config` (domyślne, plik, zmienne), `server` (`parseRate`, `renderSize`, `everyNthFrame`), `frames` (prostokąty, pakiety, `yuva`), `png`, `scale`. Obrazy są opisane wypełnieniami (`paint`), więc Rust buduje to samo RGBA. `cargo test` porównuje bajt w bajt, a skalowanie z tolerancją ±2 na kanał.
3. **CI:** `cargo test`, potem `npm test` z `VIRTRS_CG_BIN=rust/target/release/virtrs-cg`.

## 4. Etapy

Każdy etap kończy się zielonym commitem na `rust-edit`.

| # | Etap | Zakres | Stan |
|---|---|---|---|
| 0 | Przygotowanie | praca na `main` zacommitowana, gałąź `rust-edit`, crate w `rust/` (bo `src/` zajmuje JS), job `rust` w CI | ✅ |
| 1 | Kontrakt | testy czarnej skrzynki (`test/sidecar.mjs`, przeglądarka `/bin/false` = przezroczyste klatki; treść grafiki w `chromium.test.mjs`) i wektory `test/vectors/*.json` z generatorem, nadal na JS | ✅ |
| 2 | Szkielet | `json.rs` (parsowanie i zapis jak `JSON.stringify`), `config.rs` (plik + zmienne, zgodny z `test/vectors/config.json`), `log.rs`, `http.rs` (jedno żądanie na połączenie, wątek na połączenie, `Content-Length` lub chunked, limit 4 MB), `server.rs` z `/health`, `main.rs` (argument `config.json`/`CONFIG`); pozostałe trasy na razie 404 | ✅ |
| 3 | Logika bez grafiki | `time.rs` (formy ISO `Date.parse`, wektor `dates`), `template.rs` (kontekst klipu, symbole, `AdMatcher`), `vizrs.rs` (TCP z limitami 2 s, `AUTH`/`STATE`/`OUT`/`TAKE`; stan kanału pod mutexem, wysyłki po kolei w wątku kanału, zegar przerwy jako wątek z licznikiem pokoleń), `packets.rs` (budowa pakietów), `stream.rs` (tempo, wyprzedzenie, `Repeat`, wstrzymanie/wznowienie), `/etv` z przezroczystymi klatkami | ✅ |
| 4 | Klatki | `frames.rs` (klatka z czasem, pakiet i `yuva` liczone raz; historia `setFrame`/`frameAt`), `rects.rs`, `png.rs` (crate `png`), `yuva.rs` (te same działania na `f64` co JS, więc bajt w bajt), `scale.rs` (jak libvips: zmniejszanie Lanczos3 jak `vips_reduce` – najpierw pion, każde przejście w 8 bitach; powiększanie bicubic z próbkowaniem w `x/skala − 0,5`; kolor zerowany, gdy alfa = 0), format `Yuva` w `stream.rs` | ✅ |
| 5 | Chromium | `capture.rs` (flagi i dekoder najnowszej klatki jak w `vizrs_capture.rs`; screencast z `everyNthFrame`, zrzut po 150 ms ciszy, czas z `metadata.timestamp`, pauza po 30 s, `raiseFps`, restart po 3 s z czystymi klatkami), `devtools.rs` (CDP przez `--remote-debugging-pipe`, sprawdzone na Chrome), `sys.rs` (rury na fd 3/4, `PR_SET_PDEATHSIG` – przeglądarka ginie nawet przy `kill -9`; SIGTERM sprząta przeglądarki i profile), `base64.rs`; testy Chromium w `rust/tests/chromium.rs` | ✅ |
| 6 | Nakładka ffmpeg | `/ffmpeg` (oczekiwanie do 10 s, przejęcie sesji), `/ffmpeg/frames` (surowe `yuva420p`, `raiseFps`), wstrzymanie 3 s (`HOLD`), powrót do ErsatzTV po 10 s bez strumienia (`FRAMES_WAIT`), kanały z nakładką w `/health` | ✅ |
| 7 | Obraz i CI | wieloetapowy `Dockerfile`: binarka budowana na platformie hosta, arm64 kompilacją krzyżową (`aarch64-unknown-linux-gnu`, ok. 40 s zamiast emulacji), obraz `debian:bookworm-slim` + `chromium-headless-shell`; `VIZRS_BROWSER` wskazuje samą przeglądarkę (`/usr/lib/chromium/…`), bo skrypt Debiana w `/usr/bin` uruchamia ją jako dziecko `sh` – zatrzymanie i `PDEATHSIG` trafiały w `sh`; `init: true` w compose zbiera procesy pomocnicze po awarii przeglądarki (wcześniej zombie przy PID 1 – także w obrazie Node); bez zmiennych libvips i Node w nakładce Ampere. `ci.yml` (artefakt binarki) i `release.yml` (archiwa x86_64/arm64, obraz, wydanie) – commit autora 939643b | ✅ |
| 8 | Odbiór | stos Docker obok działającego (osobny projekt i porty): ErsatzTV z nakładką ffmpeg przejmuje sesję, grafika VizRS (`on_clip` z `{title}`, `{next_title}`) jest w obrazie HLS, przerwa reklamowa zdejmuje i przywraca stronę 4001, zabita przeglądarka wraca po ok. 3,5 s bez zombie, bez sidecara kanał nadaje (nakładka w Pythonie, bez zmian). Benchmark `bench/sidecar.mjs` (tabela niżej). Ampere A1: do zmierzenia | ✅ (amd64) |
| 9 | Sprzątanie | usunięte `bin/`, `src/*.js`, `package-lock.json`, testy jednostkowe JS, `docker/sidecar.Dockerfile` i stary benchmark silników (`bench.mjs`, WPE – decyzja autora; wnioski w DOKUMENTACJI §8); crate w korzeniu (`Cargo.toml`, `src/`, `tests/*.rs`), testy kontraktu w `tests/contract/` (`npm test`, bez zależności), wektory zamrożone w `tests/vectors/`; nowy obraz i compose benchmarku `bench/sidecar.mjs`; DOKUMENTACJA (§1, §4–§8), README, CLAUDE.md, workflow. Scalenie do `main` i numer wersji – decyzja autora | ✅ (bez scalenia) |

### Wyniki etapu 8 (Xeon E3-1270 v3, 1080p, scena `animated`, 25 s, dwa przebiegi)

| Tryb | Budowa | CPU sidecara | RSS | Nowe obrazy/s |
|---|---|---|---|---|
| `etv` (pakiety) | Node | 53–56% | 263–280 MB | 21–24 |
| `etv` | Rust | 28% | 93–96 MB | 24 |
| `ffmpeg` (`yuva420p`, 50 kl./s) | Node | 111–140% | 351–390 MB | 26–38 |
| `ffmpeg` | Rust | 79–93% | 161–187 MB | 31–38 |

Pamięć zajmują głównie klatki (historia 500 ms, w trybie `ffmpeg` pełne klatki `yuva420p` 1080p po 3 MB), a nie runtime. Zakładane w §0 ok. 10 MB dotyczy samego procesu bez grafiki.

## 5. Zamierzone różnice względem Node.js

Na scaleniu trafią do DOKUMENTACJI §5.

- Nieznana opcja w pliku konfiguracji daje ostrzeżenie w logu (Node pomijał ją po cichu).
- Zły typ opcji w pliku (np. `"no_sandbox": "yes"`) zatrzymuje start z komunikatem, która opcja jest zła. Node uruchamiał się i działał dalej z dziwną wartością. Liczba zapisana jako tekst (`"lead_seconds": "6"`) jest przyjmowana.
- `LEAD_SECONDS`, który nie jest liczbą, daje ostrzeżenie i zostawia poprzednią wartość (Node przyjmował `NaN`).
- Data i czas bez offsetu (`2026-10-24T20:00:00`) są czytane jako UTC. W JS to czas lokalny hosta, ale ErsatzTV zawsze wysyła offset. Inne formy dat mają wyniki identyczne z `Date.parse`, co pilnuje wektor `dates`.
- `\d` i `\w` w `ad_match` obejmują też cyfry i litery spoza ASCII (np. `\w` pasuje do „ż”). W JS tylko ASCII.
- Dane szablonu `null` dają pusty kontekst (Node odpowiadał 400).
- Nieprawidłowy wzorzec `ad_match` ma dawać ostrzeżenie i „nie reklama”. W Node wyjątek wywraca proces.

## 6. Wzorce z VizRS ffPlayout System

Autor pozwolił przenosić kod ze swoich modułów VizRS (zasada w CLAUDE.md). Kod upstream ffplayout jest wyłączony.

- `crates/app/src/utils/vizrs.rs` – kontekst klipu, symbole, `onAir`, wysyłanie komend: wzorzec dla etapu 3.
- `crates/app/src/utils/vizrs_capture.rs` – Chromium przez DevTools (WebSocket), dekodowanie w osobnym wątku, `every_nth_frame`: wzorzec dla etapu 5. W razie problemów z trybem rury to także zapasowa ścieżka WebSocket.

## 7. Ryzyka

- **`ad_match`.** `regex` nie ma lookahead ani odwołań wstecznych. Nieprawidłowy wzorzec daje ostrzeżenie przy starcie i „nie reklama”, nigdy błąd (zrobione w etapie 3). Trzeba to opisać w DOKUMENTACJI §5.
- **Daty.** Własny parser zastępuje `Date.parse`. Wektory zbieram z prawdziwych danych szablonu z `seed/`.
- **Sesja wstrzymana.** Nagłówki muszą wyjść od razu (dziś robi to `flushHeaders`), inaczej klient ETV czeka. Zrobione: `http::Stream::start`. Koniec sesji wykrywa wątek czytający połączenie, więc działa też dla sesji, która nic nie pisze.
- **Skalowanie.** Własny Lanczos odbiega od libvips o pojedyncze wartości. Test ma tolerancję.
- **Tryb rury DevTools.** Działa z Chrome (etap 5). Do potwierdzenia na `chromium-headless-shell` z Debiana (amd64 i arm64) w etapie 7. Zapasowo klient WebSocket jak w `vizrs_capture.rs`.
- **Skalowanie na CPU.** `scale.rs` liczy na `f64` w jednym wątku (libvips: SIMD i wątki). Pełnoekranowa grafika 720p→1080p to kilkadziesiąt ms na klatkę. Do sprawdzenia w benchmarku (etap 8), w razie potrzeby wersja całkowitoliczbowa.
