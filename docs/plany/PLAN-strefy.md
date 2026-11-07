# PLAN-strefy – strefy grafiki liczone z tego, co jest na antenie

> Stan: propozycja (2026-11-07). Opis działania po wdrożeniu – `docs/DOKUMENTACJA.md` §1 („Obszar grafiki”).

## Problem

Obszar grafiki (`area`, `VIZRS_AREA`) jest dziś **stały**: ustawia się go w konfiguracji raz dla kanału. Bez niego renderer rysuje, sidecar dekoduje, a nakładka ffmpeg wysyła do ErsatzTV i nakłada **pełną klatkę** `yuva420p` (1080p: ok. 5 MB). Przy `VIZRS_FFMPEG_FPS=50` to ok. 260 MB/s przez pipe, a przy NVENC – `hwupload_cuda` i `overlay_cuda` pełnej klatki z pulą buforów w pamięci GPU. Na lokalnej produkcji (Quadro P400, 2 GB, dzielone z pulpitem) właśnie ta pamięć się kończy (`Error while filtering: Cannot allocate memory`, 2026-11-07).

Tymczasem grafika zwykle zajmuje mały kawałek ekranu i zmienia się w czasie: belka tytułowa na start pozycji, „Za chwilę” przed końcem, ticker, logo. Stały obszar musi objąć sumę wszystkiego, co *kiedykolwiek* może się pojawić, więc albo jest za duży (mały zysk), albo za mały (grafika ucięta).

## Cel

Strefy przebiegu ffmpeg wynikają z grafiki, którą system **właśnie odpalił** (i odpali w tym kawałku). Każda strona VizRS ma swój ślad na ekranie, a przebieg dostaje sumę śladów stron na antenie, nie cały ekran. Gdy pojawi się grafika spoza stref, strefy się poszerzają, a grafika nigdy nie jest trwale ucięta.

## Zasady (z niepodważalnych zasad repozytorium)

- **VizRS bez zmian** w podstawowej wersji: ślady liczymy z pikseli klatek (`visible_rects`, już używane dla pakietów ETV), a wiedza o tym, co jest na antenie, pochodzi z protokołu (TCP `STATE`, komendy, które sidecar sam wysyła). Dokładne granice z VizRS to osobny, opcjonalny punkt (S6, w repo VizRS).
- **Grafika nigdy nie zatrzymuje emisji**: każdy błąd mechanizmu oznacza powrót do obecnego zachowania (cały ekran albo stały `VIZRS_AREA`).
- **Zgodność**: `VIZRS_AREA=<obszary>` działa jak dziś i ma pierwszeństwo. Nowe zachowanie włącza `VIZRS_AREA=auto`, domyślnie wyłączone.

## Mechanizmy

### 1. Ślady stron (S1)

Rejestr śladów: dla każdej strony (show, numer strony, rozmiar renderu) zestaw prostokątów w pikselach kanału.

- **Uczenie z klatek.** Gdy na antenie kanału (bus `ch-N` albo główna antena) jest dokładnie jeden zestaw stron, a sidecar wie, które to strony (sam je wziął komendą `TAKE` z reguł albo widzi je w `STATE`), `visible_rects` każdej klatki dopisuje się do śladu tych stron. Ślad tylko **rośnie** (suma), więc obejmuje też skrajne fazy animacji (wjazd z boku, rozwinięcie).
- **Normalizacja.** Prostokąty wyrównane do siatki 16 px, z marginesem 16 px, a sąsiednie (odstęp < 64 px) sklejane. Limit 4 prostokątów na stronę; nadmiar = ich obwiednia.
- **Wiele stron naraz** (logo + belka): piksel przypisujemy stronie, której istniejący ślad go obejmuje; reszta trafia do strony wziętej najpóźniej. Niejednoznaczne klatki (dwa nowe TAKE w tej samej chwili) nie uczą nikogo.
- **Trwałość.** Plik `<data>/footprints.json` (jak `control.json` w live API), unieważniany przy zmianie show VizRS (nazwa / `rev` ze `STATE`). Strona bez śladu = **nieznana**.

### 2. Strefy przebiegu (S2)

Przy starcie każdego przebiegu (`GET /ffmpeg?channel=N&area=1`) sidecar liczy zestaw stref zamiast brać stały:

```
strefy = sklej( ślady stron na antenie kanału teraz
              ∪ ślady stron, które reguły mogą wziąć w tym kawałku:
                  on_clip, on_end (i reguły audycji/kanału, które pasują do tytułu kawałka) )
```

- Reklama nie ma grafiki (zachowanie z ffPlayout): strefy z `on_ad` się nie liczą.
- **Którakolwiek strona nieznana** → cały ekran dla tego przebiegu (bezpiecznie; przy okazji uczy się jej śladu).
- **Sklejanie stref**: każda strefa to osobne `overlay` w grafie ffmpeg (`placed_areas`), więc łączymy je, dopóki suma pikseli rośnie o mniej niż 25%, najwyżej do 4 stref. Strefy zajmujące ponad 60% ekranu → cały ekran (atlas nie daje wtedy zysku).
- Odpowiedź `/ffmpeg` niesie zestaw tak jak dziś (`areas`, rozmiar atlasu), więc nakładka (`vizrs-ffmpeg`) nie zmienia formatu, a tylko dostaje inne liczby.

### 3. Poszerzenie w trakcie przebiegu (S3)

Zestaw stref jest stały na przebieg (graf ffmpeg się nie zmienia). Gdy w trakcie pojawi się grafika spoza stref:

- **Wyprzedzenie (zanim narysuje się piksel):** sidecar wie o każdym `TAKE`, który sam wysyła (`on_end` przed końcem, reguły), i widzi w `STATE` strony wzięte przez operatora (Trio). Jeśli ślad nowej strony nie mieści się w strefach, albo strona jest nieznana, ustawia dla kanału flagę „poszerz”.
- **Wykrycie (zabezpieczenie):** piksele `visible_rects` poza strefami przez 2 kolejne klatki → ta sama flaga.
- **Nowy przebieg:** nakładka odpytuje flagę (jak stan live w `takeover.rs`, co 250 ms) i robi to samo, co przy przejęciu live: kończy przebieg `SIGINT` (z eskalacją `SIGTERM`/`SIGKILL` z 2.10.0) i startuje kolejny od miejsca, w którym jest kawałek (`-ss`, `-output_ts_offset` z EXTINF), z nowym zestawem stref. To ta sama ścieżka przebiegów co ingest, więc HLS jest ciągły, a przełączenie wypada na granicy segmentu.
- **Koszt i limit:** między wykryciem a nowym przebiegiem (do ok. 1 s) grafika spoza stref jest ucięta, czego wyprzedzenie zwykle unika. Najwyżej 1 poszerzenie na 10 s; drugie w tym oknie → cały ekran do końca kawałka (koniec z „mruganiem” przebiegów).

### 4. Zwężanie (S4)

Strefy maleją tylko wtedy, gdy przebieg i tak startuje od nowa: przy nowym kawałku, po live albo po poszerzeniu. W środku kawałka nie restartujemy ffmpeg tylko po to, żeby zmniejszyć strefy (histereza: zysk mniejszy niż koszt przełączenia).

### 5. Obserwowalność (S5)

- `/health`: dla przechwytywania `areas` (bieżące strefy przebiegu), `area_mode` (`auto|static|whole`), `area_pixels` (% ekranu), `widenings` (licznik poszerzeń), `unknown_pages`.
- Log: start przebiegu ze strefami i ich udziałem w ekranie, każde poszerzenie z przyczyną (`take 4001 bez śladu`, `piksele poza strefami`).
- VizRS Live (osobne repo): wiersz „Grafika: strefy N, X % ekranu” w panelu „Stan” Diagnostyki – tylko z `/health`, bez nowych endpointów.

### 6. Granice stron z VizRS (S6, opcjonalnie, repo VizRS CG System)

VizRS zna układ szablonu (pozycje i rozmiary elementów, zakres animacji), więc może podać ślad dokładnie, bez uczenia: `GET /pages/<n>/bounds?w=1920&h=1080` → prostokąty maksymalnego zasięgu strony. Sidecar używa ich zamiast śladu z pikseli, gdy są (strona nigdy nie jest „nieznana”), a uczenie zostaje jako zapas dla starszych wersji VizRS. To zmiana w repo VizRS, osobny plan tam; tu tylko odczyt przez protokół HTTP (zgodnie z zasadą „Z VizRS tylko przez protokół”).

## Szacowany zysk

Typowy kanał informacyjny: belka 1920×300 u dołu + logo 300×150 w rogu ≈ 16% ekranu, czyli ok. 6× mniej danych grafiki na klatkę (5 MB → ok. 0,85 MB). Proporcjonalnie maleje ruch przez pipe, `hwupload_cuda` i pule buforów `overlay_cuda` w pamięci GPU, a renderer (DevTools `viewport` / natywny) rysuje mniejszy obszar. Bez grafiki na antenie (np. reklamy) strefa jest pusta: przebieg dostaje minimalną strefę 16×16 albo w ogóle nie dostaje nakładania – do decyzji w S2.

## Ryzyka

| Ryzyko | Ograniczenie |
|---|---|
| Animacja wychodzi poza dotychczasowy ślad (rzadka faza) | ślad tylko rośnie, margines 16 px, wykrycie pikseli poza strefami (S3) |
| Operator bierze w Trio dowolną stronę | `STATE` → wyprzedzenie (S3); nieznana strona → cały ekran |
| Grafika pełnoekranowa | > 60% ekranu → cały ekran; brak zysku, ale poprawny obraz |
| Częste przełączanie przebiegów | limit 1 poszerzenia / 10 s, potem cały ekran do końca kawałka; zwężanie tylko przy naturalnym starcie (S4) |
| Wiele `overlay_cuda` w grafie | najwyżej 4 strefy (S2) |
| Błąd rejestru / pliku śladów | brak śladów = wszystkie strony nieznane = cały ekran |

## Wynik S0 (2026-11-07)

Benchmark `bench/sidecar.mjs`, tryb `ffmpeg`, 1920×1080, host lokalnej produkcji (Xeon E3-1270 v3) pod obciążeniem emisji; Chrome z pulpitu nie rysował animacji sceny (`pictures_per_s` ≈ 0,3), więc miarodajne są tylko strumień i CPU sidecara:

| Klatkaż | Strefy | MB/s do ffmpeg | CPU sidecara |
|---|---|---|---|
| 50 | cały ekran | 252 | 34% |
| 25 | cały ekran | 130 | 24% |
| 50 | belka `1920x300+0+780` + logo `320x160+1580+30` | 111 | 33% |
| 25 | belka + logo | 55 | 16% |

Wniosek dla S2: atlas układa strefy jedna pod drugą z szerokością najszerszej, więc logo 320 px zajmuje w nim 1920 px szerokości (43% ekranu zamiast 30% pikseli stref). S2 powinno układać strefy obok siebie, gdy się mieszczą (pakowanie półkowe), albo sklejać je tylko przy zysku.

## Punkty

| Punkt | Zakres | Wymaga | Stan |
|---|---|---|---|
| S0 | Pomiar wyjściowy na lokalnym stosie: CPU sidecara i ErsatzTV, pamięć GPU, ruch pipe przy pełnym ekranie (benchmark `bench/`, `VIZRS_FFMPEG_FPS=25` i `50`) | – | zrobione (2026-11-07, wyniki niżej) |
| S1 | Rejestr śladów stron: uczenie z `visible_rects` przy znanych stronach na antenie, normalizacja, `footprints.json`, testy jednostkowe (przypisanie pikseli, sklejanie, unieważnienie przy zmianie show) | S0 | zrobione (2026-11-07; `src/footprint.rs`, uczenie co 0,5 s w wątku sidecara, `VIZRS_AREA=auto`) |
| S2 | `VIZRS_AREA=auto`: zestaw stref przebiegu przy `GET /ffmpeg` (strony na antenie + reguły kawałka), sklejanie, nieznana strona → cały ekran; test kontraktu nakładki z atlasem zmiennym między przebiegami | S1 | do zrobienia |
| S3 | Poszerzenie w trakcie przebiegu: flaga z wyprzedzenia (`TAKE`, `STATE`) i z pikseli poza strefami, nowy przebieg przez ścieżkę przebiegów `takeover.rs`, limit 1/10 s | S2 | do zrobienia |
| S4 | Zwężanie przy naturalnym starcie przebiegu (histereza) | S3 | do zrobienia |
| S5 | `/health` i log (strefy, % ekranu, poszerzenia), DOKUMENTACJA §1 i §5 (`VIZRS_AREA=auto`); w VizRS Live – wiersz w „Stan” Diagnostyki | S2 | do zrobienia |
| S6 | (opcjonalnie, repo VizRS CG System) `GET /pages/<n>/bounds` – granice stron z układu szablonu; sidecar używa ich przed śladem z pikseli | S2 | do zrobienia |

Kolejność S0 → S5 daje działającą funkcję bez zmian w VizRS. Każdy punkt ma testy (`cargo test`, wektory, kontrakt `npm test`) i kończy się zielonym `cargo fmt --check && cargo clippy -- -D warnings && cargo test && npm test`.
