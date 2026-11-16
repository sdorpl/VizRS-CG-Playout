# Zmiany

Wydania VizRS CG Playout w numeracji rok.miesiąc.rewizja od wersji 26.11.1, publikowane jako obraz kontenera `ghcr.io/sdorpl/vizrs-cg`. Wcześniejsze wydania (2.x) znajdują się w historii gita oraz w tagach `v2.*`.

## Niewydane

- **Połączenie z VizRS Live:** panel kanału, most VizRS Bridge i aplikacja okienna w jednym repozytorium (`panel/`); wspólna numeracja wersji całego produktu i jedno wydanie w `.github/workflows/release.yml`.
- **Profile compose `grafika` i `ingest`:** jeden plik `docker-compose.yml` w korzeniu repozytorium; bez profilu startuje sam ErsatzTV (np. z panelem), profil `grafika` włącza usługi `vizrs` i `cg`, a profil `ingest` – usługę `ingest` (uwaga: stosy Portainera z grafiką muszą mieć `COMPOSE_PROFILES=grafika` albo `grafika,ingest` przed wdrożeniem tej wersji – RUNBOOK panelu §2).
- **Jedna nakładka `docker-compose.live.yml`:** ujednolicone wdrożenie panelu w Portainerze i lokalnie (usunięto `panel/compose*.yml`), konfiguracja Caddy wbudowana w `configs:` nakładki z weryfikacją przez `panel/scripts/check-caddy.sh`; lokalna budowa obrazów przez nakładkę `panel/compose.build.yml`.
- **Nazwa „VizRS CG Playout” w panelu i oknie:** ujednolicona nazwa interfejsu panelu i aplikacji okiennej z dopiskiem „Powered by ErsatzTV” w oknie „O programie”.
- **Aplikacja okienna (AppImage):** wyłącznie natywny klient panelu działającego na serwerze produkcyjnym (usunięty tryb panelu lokalnego); połączenie przez HTTPS/TLS i token panelu.
- **Stos z VizRS Live:** aktualizacja obrazów do 26.11.8 (logo na przejęcie kanału przez live ingest).

## 26.11.10 (2026-11-14)

- **Grafika podczas przejęcia przez live ingest (`on_live`):** przejęcie kanału wstrzymuje grafikę ramówki; na antenę wprowadzana jest grafika live kanału (`on_live`).
- **Stos z VizRS Live:** aktualizacja obrazów do 26.11.6 (Dziennik emisji – logi live ingest, przejęć i CG).

## 26.11.9 (2026-11-14)

- **Logi emisji dla panelu VizRS Live:** endpoint `GET /api/live/logs` udostępniający logi sidecara, odbiorników live ingest oraz przejęć kanału.

## 26.11.8 (2026-11-13)

- **Logi w plikach na woluminie:** zapis logów sidecara (`vizrs-cg.log`) i odbiorników live ingest (`vizrs-ingest.log`); tempo przebiegów live w śladzie przejęć, ostrzeżenie o odpadnięciu kursora oraz krótki ogon kawałka bez oceny źródła.
- **Ujednolicenie konfiguracji stosów s1 i s2:** usunięcie osobnych plików `docker-compose.s2-stack.yml` i `docker-compose.s2.yml` – stos s2 korzysta z tych samych plików co s1 (`docker-compose.yml` + `.ampere` + `.live`) z konfigurowalnymi nazwami woluminów, konfiguracją ErsatzTV i mediami.

## 26.11.7 (2026-11-13)

- **Oś czasu przebiegu nakładki:** przy czasie emisji kawałka niezgodnym z końcem poprzedniego przebiegu (różnica ponad 3 s, np. po restarcie ErsatzTV), oś czasu kontynuowana jest od końca poprzedniego.
- **Wdrożenie:** pojedynczy plik `docker-compose.portainer.yml` generowany z `docker-compose.yml`, `.ampere` i `.live`.
- **Stos S2:** domyślne włączenie `VIZRS_FFMPEG_SYNC_RAM=1` (tak jak na Ampere S1).

## 26.11.6 (2026-11-13)

- **Synchronizacja w RAM dla nakładki CPU (S1, S2):** odtwarzanie mediów bez `-readrate` ErsatzTV z tempem nadawanym przez grafikę; generowanie przezroczystych klatek w tempie kanału w przypadku utraty połączenia z sidecarem.
- **Ślady stron:** przez 30 s po zejściu strony z anteny utrzymywany jest wyłącznie ostatni obraz przechwytywania (bez komórek strumieni nakładki).

## 26.11.5 (2026-11-13)

- **Płynność grafiki nakładki:** klatka 0 kawałka ustawiona na siatce grafiki (`fps=R:start_time=0` przed pierwszym filtrem overlay), eliminująca drgnięcie grafiki na granicach kawałków ErsatzTV.
- **Stos z VizRS Live:** aktualizacja obrazów panelu i legacy z mostem do wersji 26.11.5 (wersje 26.11.2, 26.11.3 i 26.11.4 nieopublikowane).

## 26.11.4 (2026-11-13)

- **Miniatury stron VizRS dla VizRS Live:** endpoint `GET /api/live/thumbs/<numer>.png` wykorzystujący renderer natywny z VizRS CG System 26.11.3 (`vizrs-render thumb`) oraz pamięć podręczną miniatur wg wersji strony.

## 26.11.3 (2026-11-13)

- **Granice stron ze statycznej analizy scen:** pobieranie granic stron z VizRS (`vizrs-render bounds`, PLAN-strefy S6) przy użyciu renderera natywnego z VizRS CG System 26.11.2.

## 26.11.2 (2026-11-12)

- **Strefy grafiki domyślnie (`area: auto`):** automatyczne wyznaczanie stref; strony z reguł uwzględniane tylko przy ogólnym elemencie graficznym.
- **Konfigurowalny czas ciszy źródła live ingest:** parametr `ingest_idle_seconds` (domyślnie 8 s).
- **Testy kontraktu:** domyślny tryb pełnego kanału (`area: full`), chyba że test jawnie definiuje obszar.
- **Stos z VizRS Live:** przypięcie panelu i mostu w `docker-compose.live.yml` do wersji 26.11.1 (wersja mostu = wersja panelu; most legacy 1.2.7 do czasu wydania obrazów VizRS Live 26.11.1).

## 26.11.1 (2026-11-12)

- **Nowa numeracja i organizacja repozytorium:** przejście na numerację rok.miesiąc.rewizja (`26.11.1`); połączenie gałęzi `main` z `next-v` (poprzedni stan zachowany jako `main-oldstable`).
- **Optymalizacje pamięci vRAM i synchronizacja nakładki ffmpeg (PLAN-p400):**
  - buforowanie klatek materiału w RAM (`VIZRS_FFMPEG_SYNC_RAM`), stabilizujące zużycie pamięci karty przez ffmpeg na poziomie 139 MiB,
  - `VIZRS_FFMPEG_CUDA=auto` (overlay przy wolnej pamięci karty, encode przy pełnej) oraz `VIZRS_FFMPEG_CUDA=overlay` (redukcja vRAM z 387 do 159 MiB),
  - eliminacja przeskoków grafiki i lagów 1080p50 z NVENC dzięki synchronizacji w RAM bez `-readrate` oraz synchronizacji startu materiału z gotową grafiką w sidecarze.
- **Płynność i spójność strumienia:** eliminacja gubienia klatek w strumieniu `.ts` na granicy kawałków (`VIZRS_FFMPEG_NO_DISCARDCORRUPT`), konfiguracja kodera z VizRS Live (`encoder.json`), opcje x264 i usunięcie `-shortest`.
- **Renderer natywny:** integracja `vizrs-render` z VizRS CG System 26.11.1 (łaty klatek, jeden renderer na strefy).
- **Strefy grafiki:** uwzględnianie stron wracających po reklamach oraz stron elementów `page=` / `take=` przy przejęciu kanału.
- **Wdrożenie i benchmark:** zamontowanie wolumenu mediów w `docker-compose.live.yml` dla odtwarzania w Przeglądarce VizRS Live oraz benchmark pamięci GPU (`BENCH_GPU=1`).
