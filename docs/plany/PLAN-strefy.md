# PLAN-strefy – strefy grafiki liczone z tego, co jest na antenie

> Stan: S0–S6 zrobione (S0–S3a, S4, S5 w CG Playout – 2026-11-07; S3b – 2026-11-11; wiersz S5 w VizRS Live – wydanie 1.2.2; `area: auto` domyślnie – 26.11.2, 2026-11-12; S6 – 2026-11-13, czeka na wydania, niżej „Wdrożenie S6”). Opis działania – `docs/DOKUMENTACJA.md` §1 („Obszar grafiki”).
>
> Poprawki 2026-11-11 (lokalny stos: strefy zawsze „cały kanał”, sidecar 120% CPU): (1) ślady uczone z komórek siatki 16 px i czasu ich pojawienia się zamiast z widocznych prostokątów – prostokąty łączyły grafiki ułożone jedna nad drugą (QR `ipg1` miał ślad 1712x560 razem z tytułem, ticker 1920x568), a ślad nigdy się nie kurczy; `footprints.json` `version: 2`, starszy plik = nauka od nowa; (2) nakładka zdolna poszerzać strefy (S3b) mówi to w `/ffmpeg` (`follow=1`) i dostaje tylko ślady stron na antenie i z reguł – ślad raz widzianego wipe'a (`ti_pg_wipe`, 70% kadru) nie zabiera stref każdemu przebiegowi; S3a („wszystkie znane strony”) zostaje dla nakładki bez przebiegów; (3) klatki `yuva420p` budowane z poprzedniej (tylko zmienione wiersze) i liczone na liczbach całkowitych – DOKUMENTACJA §1.

> Poprawki 2026-11-12, emisja s1/s2 (wydanie 26.11.2): (1) `area: auto` domyślnie (compose `${VIZRS_AREA:-auto}`; cały kanał: `full`); (2) strony z `on_clip`/`on_end` i reguł audycji liczą się w strefach tylko na kanale, na którym od startu ruszyła sesja ogólnego elementu VizRS – na s1 i s2 (same elementy stron) domyślne `TAKE 4001` z compose nigdy nie wchodziło na antenę i dawało każdemu przebiegowi cały kanał.
>
> Poprawki 2026-11-12 (grafika ucięta zaraz po wejściu): (1) uczenie i wykrywanie patrzyły na jeden obraz co 0,5 s, więc wjazd krótszy niż to nie trafiał do śladu i był ucinany przy każdym wejściu – teraz liczą się komórki wszystkich obrazów wysłanych przez sesje kanału; (2) strumień nakładki sprawdza każdy obraz przed wysłaniem: obraz poza strefami wstrzymuje przebieg (najwyżej 3 s, w zapasie `lead_seconds`) i od razu poszerza strefy, nakładka zamyka staremu ffmpeg rurę grafiki i pyta o strefy co 100 ms – nowy przebieg pokazuje wejście grafiki od pierwszej klatki (DOKUMENTACJA §1).

## Problem

Obszar grafiki (`area`, `VIZRS_AREA`) jest dziś **stały**: ustawia się go w konfiguracji raz dla kanału. Bez niego renderer rysuje, sidecar dekoduje, a nakładka ffmpeg wysyła do ErsatzTV i nakłada **pełną klatkę** `yuva420p` (1080p: ok. 5 MB). Przy `VIZRS_FFMPEG_FPS=50` to ok. 260 MB/s przez pipe, a przy NVENC – `hwupload_cuda` i `overlay_cuda` pełnej klatki z pulą buforów w pamięci GPU. Na lokalnej produkcji (Quadro P400, 2 GB, dzielone z pulpitem) właśnie ta pamięć się kończy (`Error while filtering: Cannot allocate memory`, 2026-11-07).

Tymczasem grafika zwykle zajmuje mały kawałek ekranu i zmienia się w czasie: belka tytułowa na start pozycji, „Za chwilę” przed końcem, ticker, logo. Stały obszar musi objąć sumę wszystkiego, co *kiedykolwiek* może się pojawić, więc albo jest za duży (mały zysk), albo za mały (grafika ucięta).

## Cel

Strefy przebiegu ffmpeg wynikają z grafiki, którą system **właśnie odpalił** (i odpali w tym kawałku). Każda strona VizRS ma swój ślad na ekranie, a przebieg dostaje sumę śladów stron na antenie, nie cały ekran. Gdy pojawi się grafika spoza stref, strefy się poszerzają, a grafika nigdy nie jest trwale ucięta.

## Zasady (z niepodważalnych zasad repozytorium)

- **VizRS bez zmian** w podstawowej wersji: ślady liczymy z pikseli klatek (`visible_rects`, już używane dla pakietów ETV), a wiedza o tym, co jest na antenie, pochodzi z protokołu (TCP `STATE`, komendy, które sidecar sam wysyła). Dokładne granice z VizRS to osobny, opcjonalny punkt (S6, w repo VizRS).
- **Grafika nigdy nie zatrzymuje emisji**: każdy błąd mechanizmu oznacza powrót do obecnego zachowania (cały ekran albo stały `VIZRS_AREA`).
- **Zgodność**: `VIZRS_AREA=<obszary>` działa jak dziś i ma pierwszeństwo. Początkowo `auto` było domyślnie wyłączone; od 26.11.2 jest domyślne, a cały kanał daje `VIZRS_AREA=full`.

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

### 6. Granice stron z VizRS (S6)

Uczenie z pikseli nie radzi sobie ze stronami wziętymi w tej samej chwili (zasada „niejednoznaczne klatki nie uczą nikogo”): na s2 trzy strony kanału 4 (ticker, kafel programu, odliczanie) wchodzą razem z pozycją, więc żadna nie dostała śladu i kanał stał na całym ekranie. VizRS może podać ślad bez uczenia.

**Wykonanie inne niż pierwotny projekt.** Plan zakładał `GET /pages/<n>/bounds` w serwerze `vizrs`. Serwer (Rust bez zależności) nie liczy jednak geometrii ani nie ma metryk fontów, a zasięg strony zależy od tekstu, cieni, ruchu (`node.motion`: wjazdy o ±1920 px, skala) i pętli. Ma to wszystko renderer natywny (`source/render`: `Bounds`, `text_bounds`), który sidecar już uruchamia. Dlatego S6 to podkomenda **`vizrs-render bounds`** (VizRS DOKUMENTACJA §10.1): odtwarza stronę jak na antenie (TAKE, stop pointy ze spoczynkiem 4 s na pętle, tickery i zegary, OUT) w połowie rozdzielczości i zwraca komórki 16 px z alfą jako prostokąty w px kanału; strona nie w pełni natywna (`issues`) granic nie dostaje.

Sidecar (`src/bounds.rs`, wątek `bounds`): co 5 s `rev` show ze `STATE`; przy nowej rewizji albo nowym rozmiarze kanału `GET /api/show` (token operatora) i `vizrs-render bounds --page …` tylko dla stron, którym zmieniło się `rev` strony lub sceny. Granice trzyma obok śladów (`Footprints::bounds`, nie w pliku): strona z granicami jest znana, wyuczony ślad dokłada się do nich, poszerzanie (S3b) zostaje zabezpieczeniem. Brak podkomendy (starszy `vizrs-render`), błąd HTTP: ostrzeżenie i strefy tylko ze śladów. `/health` → `zones.bounds`, w VizRS Live wiersz „strefy grafiki” podaje tę liczbę.

Pomiar: INFO24 (22 strony) przy 1600x900 – 3,7 s (x86); granice pasują do grafik (belki – pasy u dołu, logo i odliczanie – rogi, tickery – pas 1600x68, tytuł 4001 – środek kadru).

## Wynik na A1 (s1, s2; 2026-11-12)

Bez GPU zyskiem jest przede wszystkim **CPU ffmpeg kawałka**, nie pamięć karty. Pomiar na s2 (Neoverse-N1, materiał 1600x900 25 kl./s, x264 `ultrafast` 3000 kb/s, 20 s, `nice 19`): bez grafiki 0,38 rdzenia, nałożenie pełnej klatki 1600x900 0,71, pasek 1600x160 + logo 256x128 0,46 – nałożenie pełnej klatki kosztowało tyle co kodowanie. Po wydaniu 26.11.2 na s1: strefy 1% (kanał 1, logo 144x144) i 4% ekranu (kanał 2, logo i odliczanie), ErsatzTV 1,16 zamiast 1,49 rdzenia (−22%), host 1,41 zamiast 1,69 z 4 rdzeni, CPU PSI ok. 14% zamiast ok. 24%. s2 został na całym ekranie (strony wzięte razem) – stąd S6.

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

## Wynik S0 – powtórka z pamięcią GPU (2026-11-12)

Ten sam host (Quadro P400, 2 GB, emisja kanału i pulpit w tle), binarka sidecara z działającego kontenera `cg`, `bench/sidecar.mjs` z `BENCH_GPU=1`: klatki idą też do prawdziwego ffmpeg z grafem nakładki przy `VIZRS_FFMPEG_CUDA=overlay` (`crop` + `hwupload` + `overlay_cuda` na strefę, `h264_nvenc`), pamięć GPU z `nvidia-smi` co 1 s (najwyższa w 30 s pomiaru). Chrome rysował tym razem animację (20–30 obrazów/s).

| Klatkaż | Strefy | MB/s do ffmpeg | CPU sidecara | CPU ffmpeg | Pamięć GPU ffmpeg |
|---|---|---|---|---|---|
| 50 | cały ekran | 206 | 43% | 55% | 111 MiB |
| 50 | belka `1920x300+0+780` + logo `320x160+1580+30` | 110 | 49% | 53% | 111 MiB |
| 25 | cały ekran | 130 | 31% | 28% | 107 MiB |
| 25 | belka + logo | 55 | 28% | 23% | 101 MiB |

Dla porównania sam ffmpeg tym grafem bez grafiki (`hwupload`, `hwdownload`, NVENC, 1080p50) ma 83 MiB, a z grafiką pełnoekranową z `lavfi` (bez czekania na pipe) – 123 MiB.

Wnioski:

- **Strefy prawie nie zmieniają pamięci GPU** (0–6 MiB z ok. 110 MiB na proces). Większość to kontekst CUDA i koder; grafika pełnoekranowa dokłada najwyżej ok. 40 MiB. Brak pamięci karty z 2026-11-07 brał się z klatek materiału czekających na muxer (poprawione 2026-11-11 w `VIZRS_FFMPEG_CUDA=overlay`), nie z rozmiaru grafiki.
- Strefy dają to, co w pierwszym S0: **ok. 2× mniej danych przez pipe** (206 → 110 MB/s przy 50 kl./s, 130 → 55 przy 25) i trochę mniej CPU ffmpeg przy 25 kl./s (28% → 23%). CPU sidecara się nie zmienia, bo renderer przy `auto` i tak rysuje cały kanał (różnice w granicach szumu: inna liczba obrazów na sekundę).
- Dla pozostałych prac: zawyżony ślad strony (np. po przeniesieniu strony w Artist) kosztuje pasmo pipe, a nie pamięć karty, więc unieważnianie śladu po zmianie sceny ma niski priorytet.
- Równość ruchu (`even_percent` 3–35%) nie jest tu miarodajna: Chrome z pulpitu dzielił CPU i GPU z emisją.

## Punkty

| Punkt | Zakres | Wymaga | Stan |
|---|---|---|---|
| S0 | Pomiar wyjściowy na lokalnym stosie: CPU sidecara i ErsatzTV, pamięć GPU, ruch pipe przy pełnym ekranie (benchmark `bench/`, `VIZRS_FFMPEG_FPS=25` i `50`) | – | zrobione (2026-11-07, wyniki niżej) |
| S1 | Rejestr śladów stron: uczenie z `visible_rects` przy znanych stronach na antenie, normalizacja, `footprints.json`, testy jednostkowe (przypisanie pikseli, sklejanie, unieważnienie przy zmianie show) | S0 | zrobione (2026-11-07; `src/footprint.rs`, uczenie co 0,5 s w wątku sidecara, `VIZRS_AREA=auto`) |
| S2 | `VIZRS_AREA=auto`: zestaw stref przebiegu przy `GET /ffmpeg` (strony na antenie + reguły kawałka), sklejanie, nieznana strona → cały ekran; test kontraktu nakładki z atlasem zmiennym między przebiegami | S1 | zrobione (2026-11-07; renderer dalej rysuje cały kanał – oszczędność na strumieniu, wysyłce na GPU i nakładaniu; pakowanie atlasu obok siebie – osobno) |
| S3a | Strefy przebiegu = ślady wszystkich znanych stron rozmiaru kanału (znana strona wzięta przez operatora nie jest ucięta); piksele poza strefami: ostrzeżenie i licznik `outside` | S2 | zrobione (2026-11-07) |
| S3b | Poszerzenie w trakcie przebiegu: flaga z wyprzedzenia (`TAKE`, `STATE`) i z pikseli poza strefami, nowy przebieg przez ścieżkę przebiegów `takeover.rs`, limit 1/10 s. **Wymaga zmiany nakładki:** strumień klatek grafiki jest dziś jeden na kawałek ze stałym układem (`Sink::new(frame)` w `overlay::with_graphics`), więc poszerzenie potrzebuje drugiego strumienia `/ffmpeg/frames` tej samej sesji (sidecar dziś odmawia), nowego `Sink` z innym rozmiarem klatki i ogólnego wyzwalacza restartu przebiegu w `takeover::sequence` (dziś tylko live); działa tylko dla kawałków idących przez sekwencję (`VIZRS_FFMPEG_INGEST`) | S3a | zrobione (2026-11-11; wyprzedzenie z `STATE` co 0,5 s – strona bez śladu albo ze śladem poza strefami, bez osobnej ścieżki dla `TAKE` sidecara; `/ffmpeg/zones` + kolejny strumień `/ffmpeg/frames?rev=…`; kawałek ze strefami `auto` idzie przez przebiegi także bez live ingestu w sidecarze; test `tests/contract/zones.test.mjs`; poprawka 2026-11-11: przy przejęciu także strony zdjęte na reklamę i strony elementów `page=`/`take=` kanału z ostatnich 10 min – ich sesja przychodzi po przejęciu, a pierwszy kawałek po reklamach poszerzał się do całego kanału) |
| S4 | Zwężanie przy naturalnym starcie przebiegu (histereza) | S3a | zrobione z założenia (strefy liczone tylko przy starcie przebiegu, S2) |
| S5 | `/health` i log (strefy, % ekranu, poszerzenia), DOKUMENTACJA §1 i §5 (`VIZRS_AREA=auto`); w VizRS Live – wiersz w „Stan” Diagnostyki | S2 | zrobione (CG Playout 2026-11-07, `/health` → `zones`; VizRS Live – wiersz „strefy grafiki”, commit 207ba03, wydanie 1.2.2; liczba granic S6 – 2026-11-13) |
| S6 | Granice stron z VizRS: `vizrs-render bounds` (repo VizRS CG System) zamiast pierwotnego `GET /pages/<n>/bounds`; sidecar pobiera show, liczy granice stron zmienionych od ostatniej rewizji i używa ich razem ze śladem z pikseli | S2 | zrobione (2026-11-13; testy: `bounds` w `source/render`, `footprint`/`bounds` i kontrakt `native.test.mjs` w CG Playout); wdrożenie – niżej |

## Wdrożenie S6

1. Wydanie VizRS CG System z `vizrs-render bounds` (renderer trafia do obrazu sidecara z wydania VizRS).
2. CG Playout: `VIZRS_REF` w `Dockerfile` na to wydanie, wydanie CG Playout (sidecar z `src/bounds.rs`).
3. Na s1 i s2: log `graphics zones: bounds of N pages from VizRS…`, `/health` → `zones.bounds`, strefy kanału 4 na s2 zamiast całego ekranu; pomiar CPU jak w „Wynik na A1”.
4. VizRS Live: wydanie z liczbą granic w Diagnostyce (bez niej działa – starszy panel pokazuje tylko `pages`).

Kolejność S0 → S5 daje działającą funkcję bez zmian w VizRS. Każdy punkt ma testy (`cargo test`, wektory, kontrakt `npm test`) i kończy się zielonym `cargo fmt --check && cargo clippy -- -D warnings && cargo test && npm test`.
