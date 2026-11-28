# VizRS CG Playout – dokumentacja

> Do 2026-10-31 projekt nazywał się **VirtRS CG Playout** (repozytorium `sdorpl/VirtRS-CG-Playout`, GitHub przekierowuje stary adres). Nazwy techniczne bez zmian – binarka `vizrs-cg`, obraz `ghcr.io/sdorpl/vizrs-cg`; zmienne środowiskowe `VIZRS_*`.

Dokumentacja obejmuje część sidecara grafiki i live ingest (§1–§8) oraz część panelu kanału nad ErsatzTV legacy (§9–§15). Pełny opis punktów końcowych HTTP znajduje się w [API.md](API.md), wdrożenie i procedury administratora w [RUNBOOK.md](RUNBOOK.md), a codzienna praca operatora w [INSTRUKCJA.md](INSTRUKCJA.md).

1. [Jak to działa](#1-jak-to-działa) – w tym [pakiet VizRS i zgodność](#pakiet-vizrs--zgodność-aplikacji)
3. [Komendy i symbole zastępcze](#3-komendy-i-symbole-zastępcze)
4. [Uruchamianie w Dockerze](#4-uruchamianie-w-dockerze)
5. [Uruchamianie bez Dockera, konfiguracja](#5-uruchamianie-bez-dockera-konfiguracja)
6. [Awarie](#6-awarie)
7. [Testy](#7-testy) – w tym [testy panelu](#testy-panelu)
8. [Benchmark przechwytywania grafiki](#8-benchmark-przechwytywania-grafiki)
9. [Panel kanału – składniki](#9-panel-kanału--składniki)
10. [Most (VizRS Bridge)](#10-most-vizrs-bridge)
11. [Panel (vizrs-live)](#11-panel-vizrs-live)
12. [Ekrany panelu](#12-ekrany-panelu)
13. [Znane ograniczenia (legacy)](#13-znane-ograniczenia-legacy-v26120)
14. [Aktualizacja legacy](#14-aktualizacja-legacy)
15. [Zamknięcie web GUI legacy](#15-zamknięcie-web-gui-legacy-r10)

## 1. Jak to działa

```
ErsatzTV ── element grafiki „script” ──► vizrs-etv-client ──HTTP──► vizrs-cg (sidecar)
  ▲  stdin: dane szablonu (JSON)                                         │  TCP 6100 ─► VizRS
  └──────── stdout: pakiety grafiki ETV ◄────────────────────────────────┘  Chromium ◄─ /output
```

ErsatzTV uruchamia element „script” dla każdej pozycji playoutu, podaje na
stdin dane szablonu (ścieżka, tytuł, start/stop, wpisy EPG, rozdzielczość,
klatkaż) i nakłada surowe klatki czytane ze stdout (`format: packet`).
Klient `vizrs-etv-client` ([src/etv_client.rs](../src/etv_client.rs)) tylko
przekazuje dane w obie strony (`POST /etv` po HTTP/1.0, pakiety jako surowe
bajty). To statyczna binarka (musl) budowana w obrazie `seed` (§4), więc
w kontenerze ErsatzTV na amd64 i arm64 nie potrzeba Pythona ani niczego
innego. Przeglądarka działa więc w sidecarze, jedna na rozmiar kanału
(i rozmiar renderowania, niżej), także między pozycjami.

### Rozdzielczość warstwy grafiki

ErsatzTV zawsze nakłada grafikę na płótno w rozmiarze kanału. Domyślnie
Chromium renderuje `/output` w tym samym rozmiarze (`Resolution` z danych
ErsatzTV), także w compose. To najpłynniejszy i najtańszy wariant: kanał
1600×900 z renderem 1080p dał 78% równego ruchu i 72% CPU sidecara, a w
rozmiarze kanału 99% i 29% (§8). Opcja `render_size`
(`VIZRS_RENDER_SIZE`) albo parametr `render` w adresie elementu
(`http://cg:7700/etv?channel=1&render=720p`, ma pierwszeństwo) każe
renderować w innym rozmiarze, a sidecar skaluje widoczne prostokąty do
rozmiaru kanału tak jak libvips: przy zmniejszaniu filtr Lanczos3, przy
powiększaniu bicubic, z premultiplikacją alfy (piksele w ±2 od libvips;
obliczenia w pojedynczej precyzji). Skalowane są tylko widoczne prostokąty,
nie cała klatka. Przy ciągłej animacji (przewijany ticker) każda klatka jest
nowa i przechodzi przez skalowanie: na Ampere A1 klatka z tickerem i belką
720p → 1080p to ok. 17 ms skalowania (do wersji 2.1.3 ok. 42 ms) i ok. 32 ms
całej obróbki w sidecarze.

| Wartość | Rozmiar renderowania |
|---|---|
| puste, `auto` | rozmiar kanału (bez skalowania) |
| `720p`, `720`, `540p`… | ta wysokość w proporcjach kanału (1080p → 1280×720) |
| `1280x720` | dokładnie ten rozmiar (zachowaj proporcje kanału, inaczej obraz się rozciągnie) |

Mniejszy rozmiar renderowania oznacza mniej pracy Chromium przy każdej
zmienionej klatce kosztem ostrości tekstu, a większy niż kanał daje
supersampling. Na grafikę statyczną nie ma wpływu, bo nie generuje klatek.
Pomiar na x86 dla kanału 1080p i animacji ciągłej (przewijany ticker,
mrugające „Na żywo”), CPU Chromium i sidecara na jedną klatkę na sekundę:
1080p ok. 7,8%, `720p` ok. 5,6% (−30%), `540p` ok. 4,3% (−45%).
Błędna wartość trafia do logu, a grafika renderuje się w rozmiarze kanału.
Kanały o tym samym rozmiarze, rozmiarze renderowania i obszarze grafiki
(niżej) dzielą jedną przeglądarkę.

### Obszar grafiki (`area`)

Grafika zwykle zajmuje mały kawałek ekranu (pasek z tickerem, logo w rogu),
a mimo to Chromium co klatkę rysuje i koduje do PNG cały ekran kanału, sidecar
dekoduje cały PNG, a nakładka ffmpeg (niżej) dostaje i nakłada pełną klatkę
`yuva420p` (1080p: ok. 5 MB). Opcja `area` (`VIZRS_AREA`) albo parametr
`area` w adresie elementu (`http://cg:7700/etv?channel=1&area=1920x140+0+930`,
ma pierwszeństwo) podaje obszar grafiki w pikselach kanału, w zapisie
`SZERxWYS+X+Y` (w adresie `+` może być też zakodowany jako `%2B`):

- Chromium rysuje tylko ten obszar strony (DevTools `viewport`; strona dalej
  układa się w pełnym rozmiarze, więc `/output` VizRS nie wymaga zmian), a PNG
  ma rozmiar obszaru (w rozmiarze renderowania, gdy jest `render_size`);
- sidecar dekoduje i skaluje tylko obszar; pakiety ETV mają współrzędne
  kanału, więc element „script” działa bez zmian;
- nakładka ffmpeg dostaje klatki w rozmiarze obszaru i nakłada je w jego
  miejscu (`overlay=x=X:y=Y`, Nakładka ffmpeg, niżej).

Obszarów może być kilka, rozdzielonych przecinkami, np. pasek i logo:
`area=1920x140+0+930,160x160+1690+30`.

**Domyślnie `auto`** (od 2026-11-12, w `config.json` i w compose:
`${VIZRS_AREA:-auto}`). Cały kanał daje `full` (`VIZRS_AREA=full`), a poza
compose także puste `VIZRS_AREA` albo `"area": ""`. W compose puste
`VIZRS_AREA` oznacza `auto`, więc tam strefy wyłącza tylko `full`. Pomiar
(s2, A1, materiał 1600×900 25 kl./s, x264 `ultrafast` 3000 kb/s, sam ffmpeg
kawałka): bez grafiki 0,38 rdzenia, nałożenie pełnej klatki 1600×900 0,71,
pasek 1600×160 i logo 256×128 0,46 – nałożenie pełnej klatki kosztowało tyle
co samo kodowanie. Dopóki strony na antenie nie mają śladów, przebieg dostaje
cały kanał, czyli tyle co bez stref.

**`area: auto`** (`VIZRS_AREA=auto`, PLAN-strefy) – strefy mają wynikać
z grafiki na antenie zamiast ze stałego ustawienia. Renderer rysuje cały
kanał, a sidecar uczy się **śladów stron** i z nich składa strefy każdego
przebiegu nakładki ffmpeg. Co 0,5 s dla każdego kanału z przechwytywaniem
bierze ostatni obraz i strony na jego antenie (`STATE`, z prefiksem `@N` przy
`channel_buses`), razem z komórkami **wszystkich** obrazów, które sesje
kanału wysłały od poprzedniego kroku (szybka animacja wejścia, krótsza niż
0,5 s, też trafia do śladu strony, więc przy kolejnym wejściu strona mieści
się w strefach od początku). Obraz czyta jako **komórki siatki 16 px** (komórka jest
narysowana, gdy którykolwiek jej piksel ma alfę), a nie jako widoczne
prostokąty: grafiki ułożone jedna nad drugą (QR obok tytułu nad tickerem)
dają jeden prostokąt, ale osobne komórki. Jest jedna strona – wszystkie
komórki są jej. Kilka stron:

1. komórka, która pojawiła się do 3 s po wejściu strony (`since` w `STATE`)
   i nie była narysowana przez 2 s przed nim, jest tej strony – także w śladzie
   innej (pasek wipe'a na pasku tytułu); wejścia w odstępie do 0,5 s to jedna
   chwila i z nich nikt się nie uczy (np. tytuł z `on_clip` i strona wzięta
   razem z nim);
2. komórka w śladzie strony na antenie jest już znana;
3. gdy wszystkie strony na antenie mają ślady, komórka tuż przy śladzie
   (do 32 px) dokładnie jednej z nich jest tej strony: jej animacja idzie
   dalej (rozwijana ramka, cyfry odliczania);
4. komórka, która pojawiła się do 15 s po ostatnim wejściu, z tą samą ciszą
   przed nim, jest strony wziętej wtedy; litery przesuwanego tickera, które
   znikają i wracają, nie są nowej strony;
5. komórka narysowana dłużej niż 15 s idzie do jedynej strony na antenie bez
   śladu.

W każdym innym wypadku nikt się nie uczy – błędny ślad nigdy się nie kurczy,
a brak śladu kosztuje cały kanał. Strona, której wejście sidecar widział
i która przez 15 s nie narysowała nic swojego (wszystko w śladach innych),
dostaje pusty ślad: jej piksele są w strefach, dopóki tamte strony są na
antenie, a bez nich piksele poza strefami poszerzają przebieg. Przez 3 s po
zejściu strony z anteny (jej animacja wyjścia jeszcze trwa, a `STATE` już jej
nie ma) nikt się nie uczy, a przez 30 s liczy się tylko ostatni obraz
przechwytywania (zgodny w czasie ze `STATE`), bez komórek strumieni wysłanych
od poprzedniego kroku: nakładka czyta historię nawet `lead_seconds` + 12 s za
zegarem (S1: 17–22 s), więc jej strumienie niosą jeszcze stronę, która zeszła
(2026-11-13 pasek crawla po jego OUT trafił do śladu logo i strefy kanału
urosły z 2% do 11%). Obraz jest brany przed `STATE`, więc strona wzięta
między nimi jest na liście, ale jeszcze nie na obrazie. Uczenie trwa tylko,
gdy kanał ma sesję z przechwytywaniem (ErsatzTV transkoduje); komórki
pierwszego obrazu po przerwie (ponad 5 s bez obrazu) pojawiły się w nieznanej
chwili, więc nie są „świeże” po żadnym wejściu. Każdy wzrost
śladu trafia do logu (`footprints: <strona> draws <prostokąt> (<reguła>)`).
Komórki strony łączą się w prostokąty (grupy sąsiadujących
komórek), wyrównane do siatki 16 px z marginesem 16 px, sklejane przy
odstępie mniejszym niż 64 px, najwyżej 4 na stronę (więcej – ich obwiednia).
Ślad tylko rośnie, więc obejmuje też skrajne fazy animacji. Klucz to `id`
i numer strony VizRS oraz rozmiar kanału (inne show = inne `id` = puste
ślady). Ślady zapisują się co 10 s w `<etv_config>/vizrs-live/footprints.json`
(`version: 2`); brak, uszkodzony plik albo plik starszej wersji (ślady
z widocznych prostokątów, często zawyżone) oznacza brak śladów i naukę od nowa.

**Granice stron z VizRS** (PLAN-strefy S6): przy `area: auto` sidecar w tle
(wątek co 5 s) pyta VizRS o `rev` show (`STATE`); przy nowej rewizji albo nowym
rozmiarze kanału z przechwytywaniem pobiera show (`GET /api/show` na serwerze
z adresu `output_url`, token operatora `vizrs.token`) i uruchamia
`vizrs-render bounds --show <plik tymczasowy> --url <output_url> --size <rozmiar kanału> --page N …`
dla stron, których `rev` strony albo jej sceny się zmieniły (VizRS
DOKUMENTACJA §10.1); granice (komórki 16 px, przyciągnięte do siatki
z marginesem 16 px jak ślady) czynią stronę znaną bez uczenia – także strony
wzięte w tej samej chwili, z których ślad się nie uczy (na s2 trzy strony
kanału 4 wchodziły razem i kanał zostawał na całym ekranie); wyuczony ślad
dokłada się do nich (np. dłuższy tekst niż w wartościach strony), poszerzanie
stref działa jak dotąd; strona, której renderer natywny nie rysuje w całości
(`issues`), nie dostaje granic; granice nie trafiają do `footprints.json`
(liczone od nowa po starcie); brak `vizrs-render` z podkomendą `bounds` (VizRS
starszy niż 26.11.2), błąd HTTP albo brak tokenu: jedno
ostrzeżenie w logu, ponowna próba co 60 s, strefy tylko ze śladów; log udanego
przebiegu: `graphics zones: bounds of N pages from VizRS at WxH in S s (M drawn natively)`.

Przy przejęciu sesji przez nakładkę (`GET /ffmpeg?…&area=1`, start przebiegu)
strefy przebiegu to sklejone ślady stron na antenie kanału (`STATE`), stron,
które zeszły z niej w ostatnich 5 s (ich animacja wyjścia jeszcze się rysuje,
np. na starcie nowej audycji; strona bez śladu niczego tu nie dodaje), i stron,
które mogą wziąć reguły (`TAKE` z `on_clip`, `on_end` i reguł audycji, po
zastosowaniu reguł z live API) – tylko na kanale, na którym od startu sidecara
ruszyła sesja ogólnego elementu **VizRS** (bez `page=`/`take=`/`cmd=`), bo
tylko taki element wysyła reguły. Kanał z samymi elementami pojedynczych
stron nie czeka na stronę z `on_clip`, która nigdy nie wejdzie na antenę (na
s1 i s2 domyślne `TAKE 4001` z compose przez cały czas dawało każdemu
przebiegowi cały kanał; poprawione 2026-11-12). Pierwsza pozycja po starcie
sidecara może zacząć się przed sesją ogólnego elementu; strona reguły wzięta
wtedy poza strefy poszerza je (niżej). Do tego strony, które pozycja może wziąć
poza regułami: zdjęte na czas reklamy (wracają z pierwszym zwykłym klipem;
liczone do 10 s po powrocie, bo ich `TAKE` może jeszcze nie być w `STATE`)
i strony brane w ostatnich 10 min na tym kanale przez elementy pojedynczych
stron (`page=` / `take=`, §3) – ErsatzTV uruchamia sesję takiego elementu
razem z płótnem, często ułamek milisekundy po przejęciu sesji przez
nakładkę, a bez tego pierwszy kawałek po reklamach poszerzał strefy do
całego kanału (`page … outside the zones`, restart przebiegu i grafika
1080p50 rysowana w 25 kl./s; pomiar 2026-11-11). Nakładka, której kawałek idzie przez
przebiegi zdolne poszerzyć strefy w środku kawałka (niżej), mówi to
parametrem `follow=1` i dostaje tylko te ślady: stronę wziętą później
(np. pełnoekranowe przejście) obsłuży poszerzenie, więc raz widziany wipe nie
zabiera stref każdemu przebiegowi. Nakładka bez `follow=1` dostaje do tego
ślady **wszystkich znanych stron** dla rozmiaru kanału, żeby znana strona
wzięta przez operatora w trakcie przebiegu nie była ucięta. Nakładka dostaje
strefy tak jak stały `area` (`areas`, atlas), a `resume` niesie wyliczone
strefy, nie `auto` (nakładka po restarcie sidecara dostaje ten sam atlas, co
jej graf ffmpeg). Cały kanał, gdy którakolwiek z tych stron nie ma jeszcze
śladu, gdy nic nie jest znane (z `follow=1`: także gdy nic nie ma na antenie,
nie schodzi z niej ani nie ma w regułach) albo gdy strefy zajmują ponad 60% kanału. Log: `graphics zones
<strefy> (N% of the channel)` albo `graphics zones: the whole channel`.

Strefy są stałe przez przebieg ffmpeg (jego graf się nie zmienia). Gdy
grafika wychodzi poza nie w środku kawałka, **strefy się poszerzają**
(PLAN-strefy S3b). Co 0,5 s, po uczeniu śladów, sidecar sprawdza każdy przebieg
ze strefami. Powód poszerzenia to: strona na antenie bez śladu (TAKE
operatora z Trio albo z panelu, reguła – zanim narysuje pierwszy piksel),
strona, której ślad wychodzi poza strefy, albo piksele obrazu poza strefami
(animacja szersza niż dotychczasowy ślad). Nowe strefy to te, które dostałby
przebieg startujący teraz, jeśli obejmują to, co jest na antenie i co
narysowane; inaczej – cały kanał. Drugie poszerzenie w ciągu 10 s daje cały
kanał do końca kawałka (bez „mrugania” przebiegów). Log: `graphics zones
widened to <strefy> (<powód>)` albo `graphics zones: the whole channel
(<powód>)`.

Nakładka, która dostała w `/ffmpeg` pole `zones_rev`, co 250 ms pyta o
strefy swojej sesji (`GET /ffmpeg/zones?session=N`: `rev`, `zones`, rozmiar i
`areas` klatek, `resume`). Gdy `rev` rośnie, kończy bieżący przebieg kawałka
(`SIGINT`, jak przy przejęciu live, z eskalacją `SIGTERM`/`SIGKILL`) i startuje
następny od miejsca, w którym kawałek jest (`-ss`, `-t`, `-output_ts_offset`
z EXTINF – ta sama playlista HLS, ciągłe znaczniki czasu). Nowy przebieg
dostaje nowy strumień klatek tej samej sesji
(`/ffmpeg/frames?session=N&rev=R&zones=…&at=S&area=1`: klatki w podanych
strefach, klatka 0 w sekundzie S kawałka), a poprzedni strumień znika – sesja
elementu kończy się dopiero z ostatnim. Dotyczy to także przebiegu live
(przejęcie, §1 Live ingest): po poszerzeniu stref kończy się i rusza od nowa
z nowymi strefami, a sidecar dokłada sygnał od miejsca, w którym przebieg
stanął (`take`). Do 2026-11-18 restartował się tylko przebieg ramówki: live
jechał na starym strumieniu klatek, który sidecar wstrzymywał (niżej) – w
miejscu belek wychodziły poszarpane, kolorowe pasy, a zatrzymanie takiego
przebiegu trwało ok. 3 s. Typowo na starcie kawałka po ręcznym TAKE z panelu:
strona bez śladu poszerzała strefy w ciągu 0,1 s. W logu nakładki: `live run
restarted for new graphics zones`.

**Grafika nie jest ucinana na wejściu.** Strumień klatek nakładki sprawdza
każdy nowy obraz, zanim zamieni go na klatkę: gdy którakolwiek narysowana
komórka (16 px) leży poza strefami przebiegu, ta klatka nie wychodzi.
Strumień **wstrzymuje się**, a sidecar od razu poszerza strefy (powód
`graphics outside the zones, frames held`; gdy strefy poszerzyły się już
wcześniej i obejmują obraz – tylko czeka). ffmpeg czeka na grafikę, ale
strumień wyprzedza zegar o `lead_seconds`, więc widzowie tego nie widzą.
Nakładka pyta o strefy co 100 ms. Przed SIGINT zamyka staremu przebiegowi rurę
grafiki (ffmpeg czeka na nią i inaczej nie skończyłby się od razu). Nowy
przebieg zaczyna od chwili kawałka, w której stary stanął, i pokazuje
wejście grafiki od pierwszej klatki. Wstrzymanie trwa najwyżej 3 s; potem
strumień idzie dalej (grafika poza strefami ucięta, jak przed tą zmianą).
Wstrzymuje się tylko strumień nakładki, która śledzi strefy (`follow=1`,
pytania o `/ffmpeg/zones`), i tylko raz na strumień.

Przebiegi wymagają `VIZRS_FFMPEG_INGEST=1` (domyślnie w compose) i wyjścia HLS
z `-t`, jak przy live ingest (§1, Live ingest); kawałek idzie wtedy przez
przebiegi także bez live ingestu w sidecarze, ale live go nie przejmuje.
Tylko wtedy nakładka wysyła `follow=1`. W innym przypadku (oraz ze starszą nakładką, która nie pyta o strefy)
zostaje dawne zachowanie: grafika poza strefami jest ucięta do następnego
przebiegu i sidecar ostrzega `graphics outside the zones of this run`.

`/health` ma przy `area: auto` pole `zones`: `pages` (liczba śladów), `bounds` (liczba granic stron z VizRS) i `runs` –
dla każdego przebiegu nakładki `channel`, `zones` (strefy albo `null` = cały
kanał), `share_percent`, `outside` (obrazy z grafiką poza strefami),
`widenings` (poszerzenia w tym kawałku).

- W trybie `capture_mode: frames` każdy obszar to osobna karta Chromium
  z tą samą stroną i stałym `viewport` równym obszarowi; w każdej klatce
  sidecar rysuje każdą kartę z tym samym czasem klatki, a obraz składa się
  z jednego PNG na obszar. Karta nie zmienia `viewport`, więc Chromium mówi,
  czy jej obszar się zmienił: stojący obszar (logo, odliczanie między
  zmianami) nie kosztuje zrzutu. Strona działa w każdej karcie osobno
  (JavaScript i układ tyle razy, ile obszarów; jedno połączenie zdarzeń
  VizRS na kartę), rysowany jest tylko jej obszar.
- Screencast ma jeden `viewport`: Chromium rysuje prostokąt obejmujący
  wszystkie obszary, a sidecar przycina go do nich. Przy obszarach w różnych
  rogach to prawie cały ekran, więc kilka obszarów opłaca się tylko z `frames`.
- Nakładka ffmpeg dostaje jedną klatkę z obszarami ułożonymi jeden pod drugim
  (szerokość najszerszego, wysokość wszystkich razem). W grafie rozdziela je
  (`split`, `crop`), każdy przygotowuje tak jak ErsatzTV przygotowuje grafikę
  (`format`, `hwupload`…) i nakłada w jego miejscu, jeden po drugim.
- Nakładające się obszary sidecar łączy w jeden (ich prostokąt obejmujący),
  żeby żaden piksel nie był nakładany dwa razy.

Wszystko poza obszarami jest obcinane – grafika, która ma się pokazać gdzie
indziej (lower third wyżej), musi się w którymś mieścić. Pozycja i rozmiar są wyrównywane do parzystych
pikseli (podpróbkowanie chrominancji 4:2:0) i przycinane do kanału; puste albo
`full` = cały kanał. Błędna wartość trafia do logu, a grafika idzie z całego
kanału.

Pomiar (§8, Xeon 2,8 GHz, 4 vCPU, kanał 1080p, przewijany ticker, obszar
`1920x140+0+930`): z `capture_mode: frames` 96–99,5% klatek z równym krokiem
także przy 50 kl./s (cała klatka: 0–38%), CPU przeglądarki 45% zamiast 118%
(85% przy 50 kl./s), sidecar w trybie nakładki 19% zamiast 73% CPU i 17 MB/s
zamiast 131 MB/s surowych klatek. Screencast też zyskuje (20–41% zamiast
4–10%), ale równy ruch daje dopiero połączenie z `frames`. `render_size`
mniejszy od kanału nic tu już nie dodaje: obszar jest mały, a skalowanie
podnosi CPU sidecara (38–45%). Tabela: §8.

### Taśma crawla rysowana przez sidecar (`tape`)

Przewijany ticker (crawl VizRS) zmienia się w każdej klatce, więc Chromium
rysuje go, koduje PNG, a sidecar dekoduje ten PNG raz na klatkę kanału, choć
sama taśma się nie zmienia, tylko przesuwa. Z opcją `tape` (`VIZRS_TAPE=1`,
tylko `capture_mode: frames`) taśmę przesuwa sidecar:

- sidecar co sekundę pyta stronę `/output` przez DevTools
  (`window.vizrsTape.state()`, VizRS DOKUMENTACJA §10) o crawle na antenie:
  okno crawla w pikselach strony i przesunięcie taśmy `s`. Wybiera najszerszy
  crawl mieszczący się w całości w jednym obszarze grafiki i prosi stronę,
  żeby oddała mu taśmę (`external`);
- strona ukrywa wtedy swoją taśmę, ale tylko gdy crawl stoi: to samo
  położenie co w poprzedniej klatce, przodkowie z pełnym kryciem, bez
  filtra, maski i przycięcia, i żaden element nie maluje nad oknem crawla
  (tekst liczy się według liter, nie pola – napis nad belką nie przeszkadza;
  cienie sąsiednich pól też nie).
  Wejście i zejście grafiki, ruch belki albo element na tickerze = strona
  rysuje taśmę sama, jak bez tej opcji, i bierze ją z powrotem bez przerwy
  w ruchu;
- gdy taśma jest ukryta, Chromium raz na okno taśmy (szerokość okna crawla,
  przy 1540 px i 150 px/s co ok. 10 s) rysuje „film”: jedną klatkę z samą
  taśmą przesuniętą o zamówione `S`, reszta strony ukryta, a zaraz potem
  zwykłą klatkę. Sidecar wycina okno crawla i dokłada je do poprzednich;
- w każdej klatce kanału sidecar bierze ostatni obraz strony (bez taśmy –
  stoi, więc Chromium nie koduje nic nowego) i nakłada na niego taśmę
  z filmów przesuniętą o `s` zaokrąglone do piksela (kolumna X okna to
  punkt X + s taśmy). Mieszanie alfy jak w kompozytorze Chromium, krawędzie
  okna przycięte tak samo.

Zmiana wyglądu taśmy (styl, kolory, font, rozmiar) zmienia jej klucz
(`key`) i sidecar kręci filmy od nowa; nowe wiadomości dochodzą na koniec
taśmy jak zwykle (najwyżej pół okna później niż bez opcji, bo filmy
wyprzedzają ekran). Gdy filmu nie da się zrobić, sidecar oddaje crawl
stronie i zapisuje ostrzeżenie. Inne animacje (zegar, błysk etykiety)
rysuje dalej Chromium: zmiana w obszarze to nowy obraz strony pod taśmą.
Stan widać w `/health`: `tape.crawl` (crawl rysowany przez sidecar, `null`
gdy żaden) i `tape.films` (filmy od startu przeglądarki). Opcja wymaga VizRS
2.0.7 lub nowszego; ze starszym (strona bez `window.vizrsTape`) niczego nie
zmienia, tylko co sekundę pyta o crawle.

Przy kilku obszarach crawl liczy się w karcie obszaru, w którym leży
w całości; taśmę ukrywa i filmuje tylko ta karta.

### Renderer natywny VizRS (`renderer`)

Zamiast Chromium grafikę może rysować renderer natywny VizRS – program
`vizrs-render` z repozytorium VizRS CG System (VizRS DOKUMENTACJA §10.1,
plan [plany/native.md](plany/native.md)). Odtwarza sceny tą samą logiką co
strona `/output`, ale bez przeglądarki: sam czyta zdarzenia anteny
(`/events`), fonty i pliki z adresu `output_url` (token, `layers`, `bus`,
`w`, `h`, `fx=step` działają jak na stronie) i rysuje na CPU w rozmiarze
kanału, bez `render_size`, PNG i skalowania. Sidecar uruchamia go jako
proces podrzędny (`vizrs-render stream --url <output_url> --size SZERxWYS
[--area …]`) i w każdej klatce kanału prosi o obraz z czasem tej klatki;
odpowiedź „bez zmian” nie kosztuje nic, a nowy obraz to tylko prostokąt
z grafiką (pasek tickera, nie cały kanał).

| `renderer` | Kto rysuje |
|---|---|
| `chromium` | bezgłowy Chromium, jak dotąd |
| `native` | tylko renderer natywny, Chromium się nie uruchamia; czego renderer nie umie, nie ma na obrazie (lista w logu) |
| `auto` (domyślnie) | renderer natywny, dopóki wszystko na antenie umie narysować; gdy na antenę wejdzie coś, czego nie umie (np. ruch `node.motion`, obraz WebP albo wideo, font spoza wbudowanych; renderer 0.2 rysuje całe INFO24 i R7), w tej samej klatce rysuje Chromium, a po zejściu tej grafiki znów renderer natywny |

W trybie `auto` Chromium działa obok w trybie `frames`, ale nie rysuje, dopóki
rysuje renderer natywny (prawie nie zużywa CPU). Przejście na Chromium daje
od razu świeży obraz strony; animacje, które trwały w czasie rysowania
natywnego (crawl, zegar), Chromium wznawia od miejsca, w którym sam je
zostawił, więc przy przejściu przewijany pasek może przeskoczyć. Awaria albo
brak renderera natywnego = Chromium (w `auto`) albo czyste klatki i restart
po 3 s (w `native`); renderer, który nie odda klatki w 2 s, jest
zatrzymywany. `/health`: `renderer` (ustawienie) i `drawn_by` (`native` albo
`chromium` – kto narysował ostatnią klatkę). Zmiany listy rzeczy, których
renderer nie umie, trafiają do logu (`vizrs-render: …`).

**Łaty renderera natywnego** (2026-11-11, VizRS `vizrs-render` od wydania
26.11.1). Sidecar dopisuje do każdego żądania klatki flagę `p`. Renderer, który
ją zna, wysyła przy niezmienionym prostokącie z grafiką tylko zmienione piksele
(rekord 6, VizRS DOKUMENTACJA §10.1). Sidecar trzyma ostatni obraz renderera,
nakłada na niego łatę i dalej postępuje jak z całym obrazem. Przewijany crawl
pod planszą to wtedy ok. 0,6 MB na klatkę zamiast ok. 4 MB, a kanał 1080p50
utrzymuje crawl w 50 kl./s (wcześniej bezpiecznik płynności schodził do 25).
Starszy renderer flagę ignoruje i wysyła całe obrazy. Łata bez obrazu albo
poza nim to błąd renderera: sidecar go zatrzymuje, a rysuje Chromium (jak przy
każdym błędzie renderera).

**Proces na warstwę** (`native_layers`, `VIZRS_NATIVE_LAYERS`, domyślnie
włączone; 2026-11-11). Sidecar uruchamia osobny `vizrs-render` dla każdej
warstwy show (`?layers=<warstwa>` w adresie `/output`, jak osobne źródła
w OBS) i składa ich obrazy sam. Każde żądanie klatki idzie do wszystkich
procesów naraz, więc warstwy rysują się równolegle na różnych rdzeniach,
a warstwa, która stoi, odpowiada „bez zmian” i nic nie kosztuje. Z jednym
procesem przewijany ticker zmuszał renderer do narysowania i wysłania całego
prostokąta z grafiką (tytuł, QR i pasek naraz) w każdej klatce. Pomiar
2026-11-11 (lokalny stos, kanał 1080p, ticker na `back`, tytuł na `main`, QR,
odliczanie): sam renderer przy 50 kl./s – jeden proces 40% rdzenia i ok.
74 MB/s przez pipe, procesy na warstwy razem 11% i ok. 6 MB/s; cały kontener
`cg` w emisji (25 kl./s, strefy 58%) – jeden proces 119% (renderer 80%,
sidecar 39%), proces na warstwę 21,5% (renderery 10%, sidecar 11%).

- Listę warstw sidecar czyta z show VizRS (`GET /api/show`: warstwa każdej
  sceny, z tokenem `VIZRS_TOKEN`) w tle zaraz po starcie przechwytywania,
  potem co 30 s; co 2 s porównuje ją z warstwami na antenie (`STATE`) i nowa
  warstwa (scena przeniesiona na inną) czyta show od razu. Do pierwszego
  odczytu, bez dostępu do API i przy kilku obszarach `area` rysuje jeden
  proces, jak dotąd.
- Kolejność jak na stronie `/output`: `back`, potem inne nazwy, `main`,
  `z<N>_nazwa` (N 0–99), `front`; warstwy o tym samym miejscu w kolejności
  z show. Prostokąty różnych warstw, które się nie nakładają, idą do obrazu
  takie, jakie są (piksele wspólne z poprzednim obrazem, więc klatka
  `yuva420p` je pomija, §1 „Klatka z poprzedniej”). Nakładające się grupy
  sidecar miesza w ich obwiedni (alfa „over”, bez przemnożenia, jak
  przeglądarka) i zapamiętuje wynik, dopóki ich piksele się nie zmienią.
- `auto`: gdy którakolwiek warstwa wymaga Chromium (rekord „nie
  narysowano”), całą klatkę rysuje Chromium, jak przy jednym procesie.
- Klatkę następnego slotu sidecar zamawia zaraz po odebraniu bieżącej, więc
  renderery rysują ją, kiedy przechwytywanie czeka na slot. Bez tego każda
  klatka czekała na najwolniejszy z procesów: przy 7 warstwach i 50 kl./s
  7,6% klatek po czasie (p99 133 ms), z wyprzedzeniem 0,1% (p99 7 ms; pomiar
  2026-11-11 sondą na lokalnym stosie). Zamówiona klatka, której slot
  przepadł, i tak trafia do obrazu (łata potrzebuje swojej bazy), a sidecar
  zamawia właściwą.
- Przy kilku obszarach (`VIZRS_AREA`, strefy przebiegu) proces warstwy
  dostaje jeden `--area`: ich obwiednię. Odpowiada wtedy regionem i łatkami
  jak przy jednym obszarze, a sidecar przycina prostokąty do obszarów.
  Wcześniej przy kilku obszarach rysował jeden proces na wszystkie warstwy
  i przy każdej zmianie wysyłał wszystkie obszary w całości (20–75 ms na
  klatkę przy dwóch strefach 1080p).
- Każdy proces to 12–35 MB pamięci (pusta warstwa najmniej); show z 7
  warstwami to ok. 130 MB. `VIZRS_NATIVE_LAYERS=0` wraca do jednego procesu.
- `/health`: `captures[].layers` – warstwy rysowane osobno (pusta lista =
  jeden proces albo Chromium).
- Powiązania (`dock`) między warstwami działają także przy procesie na
  warstwę: od `vizrs-render` z VizRS CG System 26.11.9 (`VIZRS_REF`; obrys celu z fontem z zasobów – od 26.11.10) każdy
  proces zna obecność wszystkich warstw anteny i liczy przesunięcia jak
  `/output` (reguły krawędzi z obrysu celu, łańcuch, powrót po OUT celu –
  VizRS CG System DOKUMENTACJA §4.7). Starszy renderer nie widział celów
  z innych warstw – obejściem było `VIZRS_NATIVE_LAYERS=0`, teraz zbędne.
  Diagnostyka: `VIZRS_RENDER_DOCK=1` w środowisku kontenera `cg`.

Przy kilku obszarach `VIZRS_AREA` jeden proces `vizrs-render` (od 0.3; przy
procesach na warstwę – obwiednia, wyżej) dostaje je wszystkie (`--area` powtórzone) i rysuje każdy osobno, nie ich obwiednię.
Pomiar na A1 (s2, 1600×900 25 kl./s, dolny pas i róg, sam renderer): obwiednia
22,3% rdzenia, osobne obszary w dwóch procesach 12,5%. Sidecar z tą wersją
wymaga renderera 0.3: starszy wziąłby tylko ostatni `--area`.

Koszt (x86, kanał 1280×720 25 kl./s, crawl VizRS, sidecar razem z rysującym):
Chromium ok. 56% rdzenia, renderer natywny ok. 14%. Pomiar na Ampere A1
i stan obsługiwanych elementów: VizRS `docs/plany/PLAN-native.md`.
Obraz Dockera zawiera `vizrs-render`: `Dockerfile` buduje go z wydania VizRS CG
System wskazanego przez `VIZRS_REF` (na arm64 także krzyżowo).

Pomiar: §8 (Taśma crawla).

Sidecar to jeden plik binarny `vizrs-cg` (Rust) obok bezgłowego Chromium:

| Adres | Co |
|---|---|
| `POST /etv?channel=1[&render=720p][&area=1920x140+0+930]` | treść: dane szablonu ErsatzTV; odpowiedź: niekończący się strumień pakietów ETV; `render` – rozmiar renderowania grafiki, `area` – obszar grafiki (§1) |
| `GET /ffmpeg?channel=1[&wait=2000][&area=1][&follow=1]` | nakładka ffmpeg przejmuje wolną sesję elementu kanału; odpowiedź: `session`, `width`, `height`, `fps` (404 = brak sesji); z `area=1` (nakładka umie ustawić grafikę) rozmiar i położenie `x`, `y` obszaru grafiki; `follow=1` – kawałek idzie przez przebiegi, które poszerzają strefy w środku kawałka, więc przy `area: auto` strefy to tylko ślady stron na antenie i z reguł (§1, Obszar grafiki) |
| `GET /ffmpeg/frames?session=N[&fps=25/1][&area=1]` | surowe klatki `yuva420p` przejętej sesji dla ffmpeg, z `area=1` w rozmiarze obszaru grafiki (Nakładka ffmpeg, niżej) |
| `GET /etv/pages[?channel=1]` | strony VizRS kanału (lista elementów, §3) |
| `GET /ffmpeg/resume?channel=1&width=…&height=…&fps=…` | klatki dla nakładki po restarcie sidecara, bez sesji elementu; parametry z pola `resume` odpowiedzi `/ffmpeg` |
| `GET /ffmpeg/zones?session=N` | przy `area: auto`: bieżące strefy przejętej sesji (`rev` rośnie z każdym poszerzeniem w środku kawałka), dla następnego przebiegu nakładki (§1, Obszar grafiki) |
| `GET /ffmpeg/frames?session=N&rev=R&zones=…&at=S&area=1` | kolejny strumień klatek tej sesji w podanych strefach (puste = cały kanał), klatka 0 w sekundzie S kawałka; zastępuje poprzedni |
| `GET /health` | liczba sesji, stan przechwytywania (`size` kanału, `render`, `area`, `fps`, `mode`: `screencast` albo `frames`, `status`: `capturing`, `paused`, `restarting`…, `seq`) i kanały z nakładką ffmpeg (`overlay`) |

Chromium wysyła PNG tylko przy zmianie strony. Sidecar dekoduje klatkę
dopiero wtedy, gdy któraś sesja ją pokazuje (klatki, których żadna klatka
wyjścia nie bierze, zostają niezdekodowanym PNG), przycina ją do widocznych obszarów i wysyła jako jeden pakiet
`Rectangles` z osobnym prostokątem na każdą grupę grafik (np. logo w rogu
i pasek na dole to dwa małe prostokąty, a nie jeden na cały ekran;
przerwy węższe niż 32 px zostają w jednym prostokącie). Niezmienione klatki idą jako `Repeat`, więc statyczna grafika
prawie nic nie kosztuje. Animacje pod obciążeniem gubią klatki, ale nie
zostają w tyle.

Płynność animacji:

- **Przechwytywanie nie szybsze niż klatkaż kanału.** Chromium składa
  stronę 60 razy na sekundę; przy 24–30 fps screencast bierze co drugą
  klatkę (30 PNG/s zamiast 60), przy 50–60 fps każdą. To połowa pracy
  z PNG w Chromium i sidecarze, więc pod obciążeniem klatki przychodzą
  równo. Pojedyncza zmiana (strona statyczna, ostatnia klatka animacji)
  mogłaby wypaść między branymi klatkami, dlatego po 150 ms bez klatek
  sidecar robi jeden zrzut strony (`Page.captureScreenshot`) i obraz zawsze
  dochodzi do stanu końcowego.
- **Każda klatka wyjścia pokazuje grafikę ze swojej chwili.** Sidecar trzyma
  klatki z ostatnich `lead_seconds` + 1,6 s (jako PNG) z czasem, w którym Chromium je narysował (znacznik
  screencastu, nie chwila końca dekodowania), a strumień bierze dla klatki
  wyjścia tę, która była aktualna w jej chwili, nie w chwili zapisu. Klatka
  wyjścia patrzy 100 ms wstecz: tyle mieści się przesył i dekodowanie PNG,
  więc każda zastaje już swój obraz, a rozrzut czasu dekodowania nie zamienia
  ruchu w powtórzenia i podwójne kroki (opóźnienie grafiki rośnie o 0,1 s).
  Gdy ErsatzTV na moment przestaje czytać (zator, obciążony CPU), zaległe
  klatki idą jako kolejne obrazy animacji, a nie jeden obraz powtórzony i skok.
  Czytelnik ErsatzTV spóźniony o ponad 1 s względem osi czasu przeskakuje do
  przodu. Nakładka ffmpeg (§1) osi nie przesuwa: spóźniony ffmpeg dostaje
  zaległe klatki od razu i nadrabia, tak jak ErsatzTV bez grafiki. Bez tego
  kawałek, którego materiał długo startuje (np. seek w długim filmie
  z Jellyfin, ok. 14 s), trwał dłużej niż 44 s, a kanał z każdym kawałkiem
  coraz bardziej odstawał od harmonogramu. Obrazy takiego ffmpeg idą po kolei:
  klatka spoza historii nie bierze „najstarszego obrazu w chwili odczytu”
  (ffmpeg czyta po kilka klatek naraz, więc obrazy by się powtarzały
  i przeskakiwały). Klatki przesuwają się raz o stałą wartość w głąb historii
  (z zapasem 0,3 s), a przesunięcie maleje, gdy ffmpeg nadrabia.
  Pamięć historii: obraz taki sam jak poprzedni (stojąca grafika) współdzieli
  jego piksele, a klatka `yuva420p` dla nakładki (ok. 5 MB przy 1080p) żyje
  tylko, dopóki sesja ją wysyła, nie przez całą historię. Wcześniej 5,6 s
  historii przy 50 kl./s i ruchomej grafice trzymało ok. 3 GB; teraz sidecar
  z jednym kanałem 1080p50 zajmuje ok. 250 MB.
- **Klatka `yuva420p` z poprzedniej.** Gdy prostokąty obrazu leżą tam, gdzie
  w poprzednim, sidecar zmienia płótno w miejscu i tylko w prostokątach
  z nowymi pikselami (prostokąt stojącej warstwy ma te same piksele i jest
  pomijany bez porównywania); duża zmiana (ponad 150 tys. pikseli) jest
  liczona na kilku wątkach (do 4). Pomiar (1080p, tytuł, QR i ramka stoją,
  ticker 1920×104 się przesuwa): 1,8 ms na klatkę; dawny kod 12–17 ms.
  Strumień klatek nakładki składa każdy
  obraz na płótnie RGBA klatki i przelicza na `yuva420p` tylko pary wierszy
  (jeden wiersz chromy), których piksele zmieniły się od poprzedniego obrazu;
  resztę bierze z poprzedniej klatki. Przesuwany ticker pod stojącym tytułem
  kosztuje wiersze tickera, a nie całą klatkę, a obraz równy poprzedniemu
  (nowy zrzut stojącej grafiki) daje tę samą klatkę. Konwersja liczy na
  liczbach całkowitych (współczynniki macierzy mają cztery miejsca po
  przecinku), a chromę bloku 2×2 z sum kanałów; wynik jest ten sam co
  dawnego sidecara Node.js (wektory `tests/vectors/frames.json`), poza
  sumami dokładnie całkowitymi (ok. 1 na 10 tys.), gdzie liczby zmiennoprzecinkowe
  dawały o 1 mniej.
- **Klatki na żądanie (`capture_mode: frames`, §5).** Screencast bierze
  klatki z zegara Chromium (60 Hz), więc przy 25 kl./s kanału na 5 klatek
  wyjścia przypada 6 klatek źródła i ruch co jakiś czas przeskakuje
  o podwójny krok; pod obciążeniem Chromium nie trzyma też równych 60 Hz.
  W trybie `frames` sidecar każe Chromium narysować klatkę
  (`HeadlessExperimental.beginFrame`) dokładnie raz na klatkę kanału,
  z czasem tej klatki, i datuje obraz tym czasem. Animacja idzie o jeden
  krok na klatkę wyjścia, a strona stojąca kosztuje tylko próbę rysowania bez
  PNG (co sekundę zrzut kontrolny). Pomiar (§8, Xeon 2,8 GHz, 4 vCPU,
  przewijany ticker): kanał 720p25 – 95–97% klatek z równym krokiem
  zamiast 30–60%, CPU przeglądarki ok. 95% zamiast ok. 135–150%; kanał
  1080p25 renderowany w 720p – 87–93% zamiast 30–64%. Warunek: host musi
  narysować każdą klatkę w czasie klatki. Gdy nie nadąża (tamże: 1080p
  w pełnym rozmiarze, 50 albo 60 kl./s), klatki wypadają nieregularnie
  i ruch bywa gorszy niż ze screencastu – wtedy `max_fps` (np. `25` przy
  kanale 50 kl./s: 98,8% równych kroków w 720p) albo `render_size`.
  Działa tylko z `chromium-headless-shell`. To tryb domyślny; inna
  przeglądarka sama wraca do screencastu (§5).

Strumień wyprzedza zegar najwyżej o `lead_seconds`
(domyślnie 4 s) – to bufor, który ErsatzTV buduje początkowym wyrzutem,
i zarazem opóźnienie zmiany z VizRS na obrazie. Mniej = szybsza grafika,
więcej = gdy odtwarzanie się zacina. Grafika idzie `lead_seconds` za
zegarem, więc także początkowy wyrzut pokazuje obrazy z sekund przed
startem (animacja się rusza), a nie jeden obraz powtórzony.

Oś czasu strumienia to czas emisji pozycji: start w harmonogramie
(`start` z danych szablonu) plus przesunięcie (`seek`). ErsatzTV transkoduje
pozycję kawałkami po 44 s, każdy w nowej sesji, albo jednym ffmpeg na całą
pozycję (`-t` długości audycji; tak szedł film 2 h 17 min na s2, 2026-10-30).
Przy kawałkach kolejny ciągnie animację dokładnie od miejsca, w którym
skończył się poprzedni, bez skoku ani powtórki. Gdy ErsatzTV wyprzedza harmonogram (pierwsza klatka
w emisji później niż koniec wyrzutu, do 2 s), strumień czeka; czas emisji
ponad 2 s po końcu wyrzutu albo ponad 10 s w przeszłości to zły zegar lub
stare dane – wtedy oś czasu liczy się od startu strumienia, jak bez czasu
emisji.

Kolejny przebieg kanału (do 10 s po poprzednim) przejmuje jego oś czasu:
przesunięcie względem czasu emisji i miejsce, w którym poprzedni skończył
(klatka 0 następnego = klatka po ostatniej wysłanej). Gdy start z czasu
emisji odbiega od tego miejsca o ponad 3 s, czasy emisji kolejnych kawałków
do siebie nie pasują, a przebieg idzie dalej od końca poprzedniego
(`graphics stream: the air time is … s off the end of the run before`).
Tak było po restarcie ErsatzTV na S2 (2026-11-13): dwa kawałki jednej
pozycji miały czasy emisji przesunięte o 34 s, a grafika czytana 35 s po
swojej chwili przeskoczyła o 18 s. Margines 3 s pokrywa klatki, które
ffmpeg nakładki przeczytał z wyprzedzeniem.

### Nakładka ffmpeg

Element „script” ma w ErsatzTV stały koszt, niezależny od grafiki: dla
każdej klatki silnik grafiki ErsatzTV (.NET) rysuje płótno w rozmiarze kanału,
koduje je do FFV1 w osobnym ffmpeg, a główny ffmpeg je dekoduje
i konwertuje. Pomiar na Xeon E3-1270 v3 z NVENC, kanał 1080p25: przez ten
łańcuch przechodzi ok. 14 kl./s, transkodowanie idzie 0,5–0,57× czasu
rzeczywistego (segment 4 s co ok. 7 s), a widz stale czeka na bufor. Ani
sidecar (równe 25 kl./s), ani główny ffmpeg (ok. 5× z grafiką z pliku) tego
nie ograniczają, a `render_size` nie pomaga, bo płótno ErsatzTV zawsze ma
rozmiar kanału.

Nakładka `vizrs-ffmpeg` ([src/overlay/](../src/overlay/mod.rs)) omija ten
łańcuch. To
też statyczna binarka (musl), więc działa w obrazie ErsatzTV na amd64
i arm64 bez względu na jego glibc; `seed` kładzie ją do `/vizrs` (§4). W ErsatzTV ustawia się ją jako ścieżkę FFmpeg
(**Settings → FFmpeg → FFmpeg Path** = `/vizrs/vizrs-ffmpeg`); kod ErsatzTV
zostaje bez zmian.

```
ErsatzTV ── ffmpeg ──► vizrs-ffmpeg ── -i pipe:N (yuva420p) ◄── GET /ffmpeg/frames ── sidecar
              │            └─ GET /internal/graphics/… (płótno: start elementu „script”, dane pozycji)
              └─ każde inne wywołanie: prawdziwy ffmpeg bez zmian
```

1. Wywołanie bez wejścia płótna grafiki (`-i http://…/internal/graphics/<kanał>/<pozycja>`)
   idzie do prawdziwego ffmpeg bez zmian (`exec`; poza jawnie włączonymi
   `VIZRS_FFMPEG_FPS`, `VIZRS_FFMPEG_PRESET`, `VIZRS_FFMPEG_X264_PARAMS`, `VIZRS_FFMPEG_READRATE_CATCHUP`,
   `VIZRS_FFMPEG_FPS_BLEND`, `VIZRS_FFMPEG_CUDA`, `VIZRS_FFMPEG_NVENC_BF`, `VIZRS_FFMPEG_NO_SHORTEST`, `VIZRS_FFMPEG_SYNC_RAM`, `VIZRS_FFMPEG_NO_DISCARDCORRUPT` niżej). Płótno z parametrem
   `region` (same watermarki, bez elementu VizRS) też, podobnie płótno po
   `https` (ErsatzTV podaje `http://localhost`).
2. Nakładka sama otwiera płótno z nagłówkami ErsatzTV i odczytuje je do
   końca. ErsatzTV uruchamia więc element „script” jak zwykle: klient
   przekazuje dane pozycji, a komendy, reklamy i `{next_*}` działają bez zmian.
3. `GET /ffmpeg?channel=<kanał>` przejmuje sesję tego elementu (czeka na nią
   do 2 s). Sidecar przestaje wysyłać do niej pakiety, a silnik grafiki
   ErsatzTV czeka na następny pakiet, nie zużywając CPU. Przy kilku
   elementach VizRS na pozycji nakładka dostaje tylko sesję, która niesie
   grafikę; pozostałe (przezroczyste) zostają przy ErsatzTV, nawet gdy
   przyjdą pierwsze. Gdyby `/ffmpeg/frames` jednak nie dało klatek, sidecar
   loguje `ffmpeg overlay: no frames for session …`, a nakładka uruchamia
   oryginalną komendę.
4. ffmpeg czyta wejście grafiki przekazane jako `pipe:N`: nakładka przepisuje
   do niego strumień `GET /ffmpeg/frames` przez parę gniazd uniksowych
   (`socketpair`), nie przez potok. Potoki liczą się do pamięci potoków
   użytkownika – wspólnej dla wszystkich kontenerów działających jako root
   i dla `docker-proxy` (potoki po 1 MiB na każde przekazywane połączenie
   widza). Po przekroczeniu `fs.pipe-user-pages-soft` (64 MiB) jądro daje
   nowemu potokowi w kontenerze 8 KiB, klatka 1600×900 (3,6 MB) przechodzi
   wtedy w ok. 450 porcjach, a kanał nie nadąża za czasem rzeczywistym
   (zmierzone 0,96× przy ok. 45 widzach: lagi i klatkowanie narastające
   z czasem oglądania). Bufor gniazda to `net.core.wmem_default` (ok. 208 KiB)
   bez względu na potoki. Same potoki ErsatzTV (koder płótna) zostają, więc
   na serwerze z widzami przez opublikowane porty warto i tak wyłączyć
   `userland-proxy` Dockera albo podnieść `fs.pipe-user-pages-soft` (§4).
   Są to surowe klatki `yuva420p` w rozmiarze kanału,
   w tempie kanału z `lead_seconds` zapasu (§1). Między sidecarem a nakładką
   idą jako **delta**: nakładka prosi o nią (`delta=1` w `/ffmpeg/frames`
   i `/ffmpeg/resume`), sidecar potwierdza nagłówkiem `X-VizRS-Frames: delta`
   i wysyła dla każdej klatki tylko pasy wierszy (osobno w płaszczyznach Y, U,
   V, A), które zmieniły się od poprzedniej. Stojąca grafika to 8 bajtów na
   klatkę, przewijany ticker – wiersze jego paska. Nakładka trzyma klatkę,
   nakłada na nią zmiany i ffmpeg dostaje całą klatkę jak dawniej (wejście
   `rawvideo` potrzebuje każdej). Pierwsza klatka strumienia, także po
   powrocie sidecara, idzie w całości. Sidecar bez delty (bez nagłówka) albo
   starsza nakładka (bez `delta=1`) – całe klatki. Przy 1080p50 i całym kadrze
   ruch spada z ok. 260 MB/s do ok. 25 MB/s (§8, Uwagi i wyniki). Gdy kanał ma obszar grafiki
   (§1), klatki mają jego rozmiar, a nakładka ustawia jego położenie
   w filtrze nakładania grafiki (`x=`, `y=` w `overlay`, `overlay_cuda`,
   `overlay_vaapi`…; inne opcje zostają). Kilka obszarów przychodzi ułożonych
   w jednej klatce; nakładka rozdziela je (`split`, `crop`) i nakłada każdy
   osobno w jego miejscu (§1, Obszar grafiki). Jeśli w grafie tego zrobić się nie
   da (grafika skalowana po drodze, opcje pozycyjne, brak nakładania wejścia
   płótna), nakładka prosi o pełne klatki i nie zmienia filtra. Opcje płótna (`-headers`,
   `-protocol_whitelist`, kodek, format) znikają, a `-t` i reszta komendy
   zostają, łącznie z filtrami sprzętowymi i filtrem nakładania ErsatzTV
   (`overlay`, `overlay_cuda` przy NVENC, `overlay_vaapi` itd., §4).
5. Gdy ffmpeg kończy kawałek, sidecar kończy sesję elementu, a nakładka po
   0,5 s zamyka płótno. ErsatzTV kończy wtedy element „script” jak bez
   nakładki.

Kanał, którego sesję przejęła nakładka, sidecar zapamiętuje. Kolejne sesje
tego kanału od razu nic nie wysyłają do ErsatzTV. Jeśli nakładka nie
przejmie sesji w 3 s, sidecar zapomina kanał i pakiety idą do ErsatzTV jak
dawniej.

Ten sam pomiar z nakładką: transkodowanie 1× (na początku kawałka szybciej,
o zapas `lead_seconds`), CPU kontenera ErsatzTV ok. 20–30% zamiast ok. 360%,
sidecar ok. 15%.

Restart sidecara nie przerywa kanału. `vizrs-ffmpeg` trwa przez całą pozycję i
przekazuje klatki przez rurę do ffmpeg. Gdy strumień zniknie, dopełnia klatkę,
wysyła przezroczyste klatki w tempie klatkażu kanału i co sekundę łączy się ponownie (`/ffmpeg/resume`),
po czym grafika wraca na tym samym ffmpeg. Tak działa aktualizacja `cg` bez
restartu ErsatzTV; komendy kanału wrócą z następną pozycją, a stan anteny zostaje w
VizRS. Po powrocie obraz idzie z czasu bieżącego, nie z osi czasu pozycji.

Warunki i różnice:

- Numer w `channel=` adresu elementu, jeśli jest podany, musi być numerem
  kanału ErsatzTV (bez niego klient sam go ustala, §4). Nakładka zna kanał tylko
  z adresu płótna.
- Z nakładką ErsatzTV nie rysuje własnych elementów grafiki tej pozycji
  (tekst, obraz z płótna). Logo i napisy daj do VizRS. **Znaki wodne** rysuje
  za to sama nakładka – niżej.

#### Znaki wodne ErsatzTV

ErsatzTV Next wkłada znaki wodne pozycji z elementem grafiki do tego samego
płótna co element, więc po zastąpieniu płótna klatkami sidecara znikały
z anteny, gdy tylko pozycja miała grafikę VizRS (pozycja bez elementów
grafiki ma znaki jako zwykłe warstwy obrazu Next – nakładka ich nie dotyka).
Teraz nakładka, przejmując płótno, pyta most VizRS Bridge w tym samym
ErsatzTV (host i port z adresu płótna, `GET /vizrs-bridge/watermarks/<kanał>/<pozycja>`,
§10) o znaki wodne pozycji – te same, które ErsatzTV narysowałby w płótnie,
z jego położeniem i rozmiarem – i dokłada je do komendy:

- każdy znak to wejście obrazu po ostatnim wejściu komendy (`-loop 1
  -framerate <klatkaż> -i <obraz>`, GIF: `-ignore_loop 0`, animowany GIF się
  zapętla; animowany PNG – pierwsza klatka), więc numery dotychczasowych
  wejść się nie zmieniają;
- w grafie: skalowanie do rozmiaru z mostu, krycie (stałe – `colorchannelmixer`;
  przerywane – krzywa ErsatzTV: 1 s pojawiania, czas pokazu, 1 s znikania, co
  N minut wg godziny ErsatzTV – `geq`), konwersja do `yuva420p` w macierzy
  klatek grafiki, potem to samo przygotowanie co grafiki (`format`,
  `hwupload`…) i nałożenie tym samym filtrem co grafika (`overlay`,
  `overlay_cuda`, `overlay_vaapi`…) w miejscu z mostu – **zaraz po grafice
  VizRS, zawsze na wierzchu**: znak przykrywa grafikę, także pełnoekranową
  (dalsze filtry ErsatzTV po nałożeniu grafiki, np. `format=nv12|vaapi,hwupload`,
  idą po ostatnim znaku);
- obszary grafiki (§1, Obszar grafiki), synchronizacja w RAM
  (`VIZRS_FFMPEG_SYNC_RAM`) i tryby `VIZRS_FFMPEG_CUDA` działają bez zmian
  (filtry znaków przechodzą na CPU razem z resztą grafu).

Warunki: ErsatzTV z mostem (obraz `vizrs-live-legacy`, nakładka
`docker-compose.live.yml`) i `VIZRS_BRIDGE_TOKEN` w środowisku ErsatzTV (ta
sama zmienna, z której korzysta most; nakładka dziedziczy ją jako proces
potomny). Most pyta się równolegle z przejęciem sesji, najwyżej 1,5 s. Bez
tokenu, bez mostu (sam `docker-compose.yml`, starszy most), z odmową albo bez
odpowiedzi w czasie – pozycja idzie jak dotąd, bez znaków wodnych (wpis
`vizrs-ffmpeg: watermarks: …` w logu ErsatzTV). Różnice wobec płótna ErsatzTV:
znak zawsze nad grafiką VizRS (bez względu na `ZIndex` – w płótnie ErsatzTV
warstwy szły wg `ZIndex`, a element VizRS ma 100); tryb „wyrażenie
krycia” ma stałe krycie znaku; przebiegi live (przejęcie kanału przez live
ingest, audycje LIVE) – bez znaków wodnych.
- Kolory: klatki są w BT.709 (od 720 linii) albo BT.601, w zakresie
  ograniczonym, z alfą prostą (nie premultiplikowaną). Ścieżka ErsatzTV
  konwertowała zawsze w BT.601, więc w HD kolory grafiki są teraz wierniejsze.
  Sidecar podaje macierz w odpowiedzi `/ffmpeg` (`matrix`), a nakładka deklaruje
  ją na wejściu klatek (`-colorspace`, `-color_range tv`). Bez tej deklaracji
  `overlay` wynegocjowałby nieznaną przestrzeń kolorów dla całego łańcucha,
  a `scale` w ffmpeg przepuszczałby każdą skalowaną klatkę materiału przez
  RGB (BT.709 → BT.601): ok. 0,3 rdzenia A1, przesunięte kolory filmu i brak
  `color_space` w strumieniu. Pomiar 2026-10-31 (A1, 1600×900@25, x264
  `superfast`, 30 s): materiał 1080p30 12,6 Mb/s 1,50 → 1,21 rdzenia
  i +0,47 dB SSIM, 1080p60 1,46 → 1,21, 720p30 0,82 → 0,70, materiał już
  w 1600×900 0,95 → 0,90. Materiał w innej macierzy niż kanał nadal idzie
  przez RGB, bo tam konwersja jest potrzebna.
- Kanał musi mieć silnik strumieniowania, który podaje ffmpeg płótno grafiki
  przez HTTP (`/internal/graphics/…`) – jak kanał 1 z bazy startowej
  (`Channel.StreamingEngine = 1`). Przy drugim silniku ErsatzTV podaje
  grafikę na stdin (`-f rawvideo -pix_fmt bgra … -i -`): nakładka jej nie
  rozpoznaje i puszcza oryginalną komendę, więc grafika idzie wolniejszą
  ścieżką ErsatzTV, a `/health` sidecara pokazuje `"overlay":[]` mimo
  `"sessions":1`. Kanał założony w ErsatzTV od zera trzeba w tym sprawdzić.
- Tempo transkodowania zależy od tempa grafiki: ffmpeg czeka na klatki
  z sidecara. Sidecar dławiony limitem CPU (przy ciągłej animacji) spowalnia
  więc cały kanał poniżej czasu rzeczywistego – odtwarzacz się buforuje.
  Daj sidecarowi zapas CPU (`CG_CPUS` na A1, §4). Pamięć klatek `yuva420p`
  sidecar używa ponownie (mała pula), proces zajmuje przez to o kilkanaście MB
  więcej.
- Pozycja z innymi elementami grafiki, ale bez elementu VizRS, startuje
  o 2 s później (nakładka czeka na sesję) i leci ścieżką ErsatzTV.
- **Grafika na pozycji bez elementu VizRS** (od 26.11.32): bez
  elementu ErsatzTV nie daje płótna grafiki, więc wcześniej nakładka puszczała
  komendę bez zmian i grafika wprowadzona na kanale (ręcznie w panelu –
  „Wprowadź stronę…” na ekranie Na antenie – albo z Trio) szła do VizRS, ale
  nie na obraz (S1 2026-11-26: ramówka kanału 1 tylko ze znakiem wodnym).
  Teraz, z `VIZRS_FFMPEG_INGEST=1` i sidecarem, który zna grafikę kanału
  (`graphics` w `/ingest/state`), taki kawałek idzie przebiegami: póki na
  antenie kanału nie ma grafiki – komenda ErsatzTV jak jest; gdy się pojawi
  (sprawdzane co sekundę, warstwy w sidecarze co 2 s – razem do kilku sekund),
  nakładka zakłada własną sesję grafiki kanału (`POST /etv?…&live=1`, jak przy
  przejęciu przez studio; bez reguł ramówki) i restartuje przebieg od miejsca,
  w którym jest (jak przy poszerzeniu stref – nowy przebieg tej samej
  playlisty), z klatkami grafiki jako dodatkowym wejściem nałożonym na obraz
  **pod** znakami wodnymi ErsatzTV (wejścia obrazów nakładane na końcu łańcucha
  obrazu zostają na wierzchu), w strefach stron jak przy elemencie. Grafika
  zostaje do końca kawałka także po zdjęciu stron; następny kawałek bez
  grafiki na antenie rusza zwykłą komendą. Kawałek, który zaczyna się przy
  grafice już na antenie, czeka na sesję najwyżej 1,5 s i ma ją od początku.
  Bez pola `graphics` (starszy sidecar, live API wyłączone, kanał nieznany),
  bez `VIZRS_FFMPEG_INGEST`, przy audycji LIVE z ramówki (ma własną ścieżkę)
  i przy przejęciu przez studio (wyżej) – jak dotąd. Komenda, na którą grafiki
  nie da się bezpiecznie nałożyć (klatki sprzętowe w grafie: `hwupload`,
  filtry `_cuda`/`_vaapi`/`_qsv`; obraz nie z grafu filtrów; strefy nie do
  rozmieszczenia), idzie bez grafiki do końca kawałka (log nakładki „the
  graphics cannot go over this piece's command; the piece goes without
  them”); udane nałożenie – „channel N: VizRS graphics over the piece from
  X s (no VizRS element on the item)”, sidecar: „ffmpeg overlay took over the
  graphics of live ingest”. Koszt jak przy pozycji z elementem, tylko gdy
  grafika jest na antenie.
  **Studio gotowe w każdej chwili (`ingest_anytime`, od 26.11.34):** kawałek
  takiego kanału idzie ścieżką przejęcia przez studio; dopóki studio nie
  nadaje, grafika kanału wchodzi na niego tak samo jak wyżej (restart przebiegu
  od bieżącego miejsca, pod znakami wodnymi), a gdy studio wejdzie – przejęcie
  jak dotąd. Do 26.11.33 grafika wprowadzona na takim kanale bez studia nie
  trafiała na obraz (S1 2026-11-27, kanał 1 BTB). Sesję grafiki kanału taki
  kawałek zakłada od razu (gotowość na studio), więc „ffmpeg overlay took
  over the graphics of live ingest” w logu sidecara jest przy każdym kawałku.
  **Ciągłość grafiki (od 26.11.33):** sesja nakładki dostaje czas anteny
  kawałka (`MediaItem_Start` = koniec osi ErsatzTV w playliście, czyli
  `PROGRAM-DATE-TIME` + długość ostatniego segmentu, odczytany przed
  pierwszym przebiegiem), a każdy przebieg z grafiką otwiera strumień klatek
  od swojego miejsca w kawałku (`/ffmpeg/frames?…&rev=…&zones=…&at=<s>`, jak
  przy poszerzeniu stref). Sidecar bierze wtedy obrazy osi czasu kanału z tego
  miejsca (log „frame 0 on air … (air time)” albo „(the channel's timeline,
  from the run before)”), więc po restarcie (wejście grafiki, poszerzenie
  stref, granica kawałka) animacja idzie dalej. W 26.11.32 każdy nowy
  strumień zaczynał od obrazu sprzed `lead_seconds` („no air time: from
  now”): ticker cofał się o kilka klatek do kilku sekund przy każdym wejściu
  grafiki (S1 2026-11-26). Playlista bez dat albo czas anteny poza osią
  zegara – jak wcześniej, od teraz. To samo dotyczy grafiki przebiegów live
  pozycji bez elementu (logo `on_live`). Trwała grafika (logo, reguły `on_clip`) nadal
  wymaga elementu „VizRS” na pozycjach ramówki.
- Na wejściu surowe klatki kosztują pasmo, ale nie CPU: 1080p to ok. 5 MB
  na klatkę, ok. 130 MB/s przy 25 kl./s. Idą lokalnie przez sieć Dockera.

Zmienne środowiskowe nakładki (w kontenerze ErsatzTV):

| Zmienna | Domyślnie | Znaczenie |
|---|---|---|
| `VIZRS_CG_URL` | – (compose: `http://cg:7700`) | adres sidecara; puste = nakładka tylko przekazuje wywołania do ffmpeg |
| `VIZRS_FFMPEG` | `/usr/local/bin/ffmpeg` | prawdziwy ffmpeg |
| `VIZRS_FFMPEG_FPS` | – (`docker-compose.ampere.yml`: `25`, gdy zmiennej nie ma) | wymuszony klatkaż wszystkich kanałów serwera, np. `25`, `30`, `50` (także cel **Normalize Framerate**); wygrywa z klatkażem profilu kodowania; puste = klatkaż z profilu kanału (VizRS Live: Profile kodowania → Klatkaż), a bez niego – materiałów. Na Ampere ustaw pustą wartość w stosie, żeby klatkaż z panelu działał (S1 od 2026-11-20: kanał retransmisji YouTube 60 kl./s na 30 kl./s – z 25 kl./s ruch szarpał, §1 „Płynna zmiana klatkażu”) |
| `VIZRS_FFMPEG_PRESET` | – (compose A1: `veryfast`) | preset kodera programowego (libx264, libx265), np. `superfast`; puste = preset ErsatzTV. Tylko dopóki nie ma `encoder.json` – potem koder ustawia panel (niżej) |
| `VIZRS_FFMPEG_X264_PARAMS` | – | opcje libx264 (`-x264-params:v`), np. `bframes=0:subme=0` (niżej, „Opcje x264”); puste = bez. Tylko dopóki nie ma `encoder.json` |
| `VIZRS_FFMPEG_SETTINGS` | `/config/vizrs-live/encoder.json` | plik kodera z panelu (preset x264, profil H.264, opcje x264, presety NVENC i QSV, klatki B koderów sprzętowych; `PUT /api/live/encoder`) – każdy klucz pliku zastępuje odpowiednią zmienną; puste = tylko zmienne środowiskowe |
| `VIZRS_FFMPEG_READRATE_CATCHUP` | – (compose: `2`) | tempo nadrabiania strumienia MPEG-TS kanału po przestoju (`-readrate_catchup`); puste = bez nadrabiania |
| `VIZRS_FFMPEG_FPS_BLEND` | – (compose: `1`) | `1` = zmiana klatkażu materiału szybszego od kanału z mieszaniem klatek (`framerate` zamiast `fps`; klatkaż z `ffprobe`); puste = `fps` |
| `VIZRS_FFMPEG_STANDBY` | – | obraz audycji na żywo bez sygnału studia (§1, Audycje na żywo w ramówce); puste = czerń |
| `VIZRS_FFMPEG_CUDA` | – | `encode` = przy koderze NVENC (`*_nvenc`) na karcie zostaje tylko koder: nakładka usuwa z komendy ErsatzTV dekodowanie CUDA (`-hwaccel cuda`, `-hwaccel_output_format`, `-init_hw_device cuda`, `-filter_hw_device`) i wysyłkę na kartę (`hwupload`, `hwupload_cuda`), a `scale_cuda`, `overlay_cuda`, `pad_cuda`, `yadif_cuda`, `bwdif_cuda` zamienia na filtry CPU (`scale`+`format`, `overlay=…:format=auto`, `pad`, `yadif`, `bwdif`). Proces bierze ok. połowę pamięci GPU (720p: 118 MiB zamiast 218 MiB, pomiar 2026-11-07 na Quadro P400) kosztem CPU (ok. 0,3 rdzenia na 720p). Dla karty dzielonej z pulpitem: dwie pozycje kanału naraz (kończąca się i następna) nie kończą się `CUDA_ERROR_OUT_OF_MEMORY` / `Cannot allocate memory` (zastępczą planszą). Inny koder, komenda bez CUDA albo nieznany filtr `*_cuda` = komenda bez zmian. `overlay` = przy koderze NVENC i dekodowaniu CUDA (`-hwaccel cuda`) nakładka usuwa tylko dekodowanie (`-hwaccel`, `-hwaccel_output_format`, `-hwaccel_device`); zdekodowane klatki trafiają na kartę (`format=nv12\|yuv420p\|…,hwupload`) tam, gdzie filtry ErsatzTV ich tam oczekują (pierwszy `hwupload` albo filtr `*_cuda` łańcucha z wejścia), skalowanie i nakładanie grafiki (`overlay_cuda`) zostają na karcie, a klatki dla kodera (łańcuch z `-map`) wracają do RAM (`hwdownload`). Bez tego klatki czekające na muxer – dźwięk ErsatzTV idzie przez `loudnorm` z ok. 3 s wyprzedzenia – zapełniały pamięć karty w kilka sekund (`CUDA_ERROR_OUT_OF_MEMORY`, plansza zastępcza; pomiar 2026-11-11). Pomiar 2026-11-11 na Quadro P400 (materiał 720p25 do 1080p50 z paskiem grafiki): 0,33 rdzenia zamiast 1,40 przy `encode`, pamięć GPU podobna (213 MiB zamiast 206). W transkodowaniu z płótnem grafiki nakładanej przez `overlay_cuda` materiał bez własnego `-readrate` dostaje `-readrate 1` z początkowym zrywem równym grafice gotowej w sidecarze (niżej, Pamięć karty przy NVENC). Komenda bez dekodowania CUDA albo z innym koderem = bez zmian. `auto` = przed każdym transkodowaniem z koderem NVENC i dekodowaniem albo filtrami CUDA nakładka pyta `nvidia-smi` o wolną pamięć karty (najwyżej 1 s): co najmniej `VIZRS_FFMPEG_CUDA_MIN_FREE` MiB = `overlay`, mniej = `encode` (wpis w logu ErsatzTV: `GPU memory … MiB free`). Brak `nvidia-smi`, błąd albo przekroczony czas = `overlay`. Dla karty dzielonej z pulpitem, którego zużycia stos nie kontroluje: pełna karta kosztuje kawałek z grafiką nakładaną na CPU (ok. 80 zamiast ok. 140 MiB przy 1080p50 z `-bf:v 0`, więcej CPU) zamiast planszy zastępczej. Puste = komenda ErsatzTV |
| `VIZRS_FFMPEG_CUDA_MIN_FREE` | `400` | wolna pamięć karty w MiB, przy której `VIZRS_FFMPEG_CUDA=auto` wybiera `overlay`. Kawałek `overlay` 1080p50 zajmuje ok. 280 MiB (pule klatek po początkowym zrywie; 160 MiB bez niego), a przy zmianie pozycji poprzedni działa jeszcze chwilę |
| `VIZRS_FFMPEG_NVENC_BF` | – | liczba klatek B koderów sprzętowych (NVENC `*_nvenc`, QSV `*_qsv`, VAAPI `*_vaapi`) bez własnego `-bf`, np. `0`; nakładka dopisuje `-bf:v N` po nazwie kodera w każdym wywołaniu. Tylko dopóki `encoder.json` nie ma klucza `bframes` – potem ustawia je panel. Klatki B trzymają na karcie klatki odniesienia: koder 1080p50 zajmuje 139 MiB przy domyślnych ffmpeg, 99 MiB przy `1`, 79 MiB przy `0` (pomiar 2026-11-11, Quadro P400) – dla karty dzielonej z pulpitem, kosztem kilku procent bitrate przy tej samej jakości; puste = ffmpeg |
| `VIZRS_FFMPEG_NO_SHORTEST` | – | `1` = nakładka usuwa `-shortest` z każdego wywołania, którego wyjście i tak kończy `-t` (ErsatzTV podaje oba). Dźwięk ErsatzTV idzie przez `apad` i `loudnorm` z ok. 3 s wyprzedzenia, a `-shortest` trzyma surowe klatki obrazu w kolejce, aż dojdzie dźwięk tej samej chwili: przy 1080p50 ok. 150 klatek, czyli RSS ffmpeg 657 MB zamiast 268 MB, i drugi raz przy zmianie pozycji, gdy działają dwa procesy (pomiar 2026-11-11, materiał 1080p, NVENC). Długość wyjścia bez zmian. Wywołanie bez `-t` wyjścia – bez zmian; puste = komenda ErsatzTV |
| `VIZRS_FFMPEG_SYNC_RAM` | – | `1` = w trybie `overlay` (także wybranym przez `auto`) klatki materiału czekają na grafikę swojej chwili w RAM, zanim trafią na kartę: w grafie z `overlay_cuda` jedyny łańcuch materiału z `hwupload` dostaje na początku synchronizację z wejściem grafiki (`pipe:N`) na CPU – wycinek grafiki 2×2 px nałożony poza kadrem (`overlay=x=main_w:y=main_h`, nic nie rysuje), z formatem bez alfy; wejście grafiki rozdziela `split`. Bez tego klatki zdekodowane z wyprzedzeniem (ok. 3 s `loudnorm`, zryw, brak `-readrate`) czekają na karcie przed `overlay_cuda` (niżej, Pamięć karty przy NVENC). Graf z synchronizacją nie dostaje `-readrate` ani zrywu: tempo materiału wyznacza grafika z sidecara (najwyżej `lead_seconds` przed zegarem), a klatki zdekodowane wcześniej czekają w RAM – kanał nie zostaje za ramówką i grafika nie przeskakuje (niżej). Graf bez jednoznacznego łańcucha materiału albo z grafiką użytą dwa razy = bez zmian (wtedy `-readrate` jak dotąd). **Nakładka na CPU** (`overlay`, np. Ampere: S1, S2) – `overlay` i tak trzyma klatki materiału w RAM do grafiki ich chwili, więc wejście materiału traci tylko `-readrate` ErsatzTV (z `-readrate_initial_burst` i `-readrate_catchup`): pozycja, która ruszyła późno (oś czasu kanału 10–15 s za zegarem, kilka sekund otwierania materiału), nadrabia start, zamiast czytać grafikę 17–22 s po jej chwili – starszą niż historia, grafika przeskakiwała o 0,5–4,9 s (S1, 2026-11-13: 4 skoki na 48 przebiegów). Pomiar (ffmpeg ErsatzTV, CPU, 1600×900@25, grafika w tempie zegara): 30 s wyjścia w 30,4 s bez `-readrate`, 33,5 s z nim; szczyt RSS bez zmian (ok. 425 MB). Graf na karcie (`overlay_cuda`, `overlay_vaapi`, `overlay_qsv`) – bez tej zmiany. Gdy sidecar zniknie, nakładka podaje przezroczyste klatki w tempie klatkażu kanału (nie tak szybko, jak ffmpeg czyta), więc materiał bez `-readrate` nie ucieka do końca pozycji. `docker-compose.ampere.yml` włącza ją domyślnie; puste = wyłączone |
| `VIZRS_FFMPEG_NO_DISCARDCORRUPT` | – | `1` = kopia strumienia (`-c copy`) z wejścia HLS (`….m3u8`) – tak ErsatzTV składa strumień MPEG-TS kanału (`/iptv/channel/N.ts`) z własnego HLS – traci `discardcorrupt` z `-fflags` tego wejścia (puste `-fflags` znika). Każdy kawałek kanału to nowy ffmpeg, którego liczniki ciągłości MPEG-TS zaczynają się od 0, więc demuxer oznaczał ostatnią klatkę wideo kawałka przed granicą jako uszkodzoną, a flaga ją wyrzucała: 40 ms zatrzymania obrazu co kawałek (44 s), choć klatka dekoduje się bez błędów (ta sama suma MD5 co w segmencie; pomiar 2026-11-12: 399 zamiast 400 klatek na dwóch segmentach z granicy). Segmenty HLS same są ciągłe – dotyczy tylko strumienia `.ts`. Flagi zapisane inaczej niż `+…` = bez zmian; puste = wyłączone |
| `VIZRS_FFMPEG_INGEST` | – (compose: `1`) | `1` = sygnał live kanału z sidecara przejmuje transkodowanie (§1, Live ingest); przy wyłączonym ingest w sidecarze jedno zapytanie `/ingest/state` na kawałek i nic więcej; puste = nigdy |

#### Pamięć karty przy NVENC

Pamięci karty (vRAM) nie da się przenieść do RAM ani tmpfs: dekoder, filtry
`*_cuda` i koder NVENC trzymają klatki na karcie, a ich pliki wynikowe
(`/transcode`) i tak leżą w tmpfs. Da się zmniejszyć to, co na karcie siedzi.
Pomiar 2026-11-11 na Quadro P400 (2 GB, dzielona z pulpitem), materiał 720p25
do 1080p50, dwie strefy grafiki, `VIZRS_FFMPEG_CUDA=overlay`:

| Komenda | vRAM procesu |
|---|---|
| grafika przez `hwupload_cuda` (wcześniej) | 249 MiB |
| grafika przez `hwupload` (teraz zawsze) | 193 MiB |
| oraz `VIZRS_FFMPEG_NVENC_BF=0` | 133 MiB |

To pomiar w czasie rzeczywistym (`-re`, bez dźwięku). Na antenie silnik next
ErsatzTV często puszcza kawałki **bez** `-readrate` (nadrabia bufor). Grafika
z sidecara płynie jednak w czasie kanału (z `lead_seconds` zapasu), więc
materiał dekodowany z pełną prędkością tylko czeka na nią w kolejce przed
`overlay_cuda` – już na karcie. Pełna komenda z anteny (dźwięk z `loudnorm`,
HLS, 3 strefy, `-bf:v 0`; pomiar 2026-11-11):

| Wejście materiału | `hwupload_cuda` | `hwupload` |
|---|---|---|
| bez `-readrate` | 473 MiB | 387 MiB |
| `-readrate 1` | 253 MiB | 159 MiB |
| `-readrate 1 -readrate_initial_burst 2` | – | 271 MiB |

Dlatego w trybie `VIZRS_FFMPEG_CUDA=overlay` (bez `VIZRS_FFMPEG_SYNC_RAM`, niżej) transkodowanie z płótnem
grafiki nakładanej na karcie (`overlay_cuda`, klatki z `pipe:N`) czyta
materiał (pierwsze wejście) z `-readrate 1`, gdy ErsatzTV nie dał własnego
tempa, oraz z `-readrate_initial_burst` równym grafice, którą sidecar ma od
razu gotową: pole `ahead` w odpowiedzi `/ffmpeg` to `lead_seconds` plus
spóźnienie kawałka względem czasu emisji (zaległe klatki grafiki idą od
razu), w nakładce powiększone o czas od przejęcia, a dla przebiegu od
połowy kawałka (strefy poszerzone, live) pomniejszone o jego początek;
najmniej 0,5 s (domyślny zryw ffmpeg), najwięcej 30 s. Materiał wyprzedza
więc grafikę najwyżej o ułamek sekundy, a kawałek, który wystartował później
(start ffmpeg, przejęcie sesji, restart przebiegu), nadrabia ten czas jak
zrywem ErsatzTV. Sidecar bez pola `ahead` = samo `-readrate 1`.

Samo `-readrate 1` (bez zrywu, 2026-11-11 przez kilka godzin) gubiło przy
każdej zmianie kawałka ok. 3,5 s: przerwa między segmentami HLS 7,2–7,7 s
zamiast 4 s, produkcja 44 s obrazu w 47,4 s (0,93×), więc bufor
odtwarzacza wyczerpywał się po kilku kawałkach (lagi przy 1080p50 z NVENC,
choć NVENC miał 85% zapasu). Ze zrywem (pomiar 2026-11-11, P400, 1080p50,
`overlay`): przerwa ok. 5,3 s, produkcja 1,04× do dogonienia ramówki, potem
zryw 2,3–4 s; ffmpeg 277 MiB karty zamiast 167 (pule klatek rosną przy
dekodowaniu szybszym niż czas rzeczywisty), stąd `VIZRS_FFMPEG_CUDA_MIN_FREE`
400.

Te pule to klatki materiału czekające na grafikę: `loudnorm` oddaje dźwięk
z ok. 3 s opóźnieniem, więc ffmpeg dekoduje materiał tyle naprzód, a każda
klatka po `hwupload` czeka w kolejce `overlay_cuda` na grafikę, która płynie
w czasie kanału – ok. 3 MiB na klatkę 1080p, `-readrate` tego nie ogranicza
(wyprzedzenie wymusza dźwięk), a zryw dokłada (PLAN-p400, E9).
`VIZRS_FFMPEG_SYNC_RAM=1` przenosi tę kolejkę do RAM: materiał synchronizuje
się z grafiką na CPU przed wysyłką na kartę. Pomiar 2026-11-12, ffmpeg 8.1
z obrazu ErsatzTV, Quadro P400, materiał 720p25 do 1080p50 z dźwiękiem
(`loudnorm`), dwie strefy grafiki z pipe w czasie rzeczywistym, `-readrate 1
-readrate_initial_burst 2`: 359 MiB karty bez opcji, **111 MiB** z nią; ten
sam obraz na wyjściu. Na antenie lokalnego stosu (5 min, 8 kawałków):
ffmpeg kawałka stale 139 MiB zamiast 259–463 MiB, karta najwyżej 991 zamiast
1289 MiB. Koszt: klatki czekają w RAM (na antenie RSS ffmpeg 429–566 MB
zamiast do 524 MB; w próbie z samym 1080p do +430 MB) i ok. 0,09 rdzenia
(CPU `overlay` kopiuje klatkę).

**Tempo materiału przy `VIZRS_FFMPEG_SYNC_RAM=1`.** Zryw `-readrate` nie
nadrabiał startu kawałka: przewinięcie `-ss` czyta od klatki kluczowej przed
punktem (do długości GOP, np. 5 s) i to zjada zryw, a do tego ok. 3,5 s
rozruchu ffmpeg (otwarcie materiału po HTTP, przewinięcie). Kanał ustalał się
ok. 8 s za ramówką (`graphics stream: frame 0 on air -8.5 s from now`),
pierwsze klatki kawałka nakładka czytała 16–21 s po ich chwili – dalej, niż
sięga historia sidecara (`lead_seconds` + 12 s) – i grafika przeskakiwała
o 0,4–5 s (`graphics stream: frame … read … s late; the graphics jump …`),
ucinając animacje wejścia. Z synchronizacją w RAM nakładka nie dodaje
`-readrate`: materiał idzie tak szybko, jak przychodzi grafika. Pomiar
2026-11-12 (lokalny stos, kanał 1, 9 kawałków): kanał ok. 3 s **przed**
ramówką zamiast 8 s za nią, zero przeskoków, ffmpeg 117–135 MiB karty i do
565 MB RSS (kolejka ograniczona); animacje wyjścia i wejścia strony (`OUT`
i `TAKE` z live API) w obrazie HLS klatka po klatce, bez skoków.

**Klatka 0 kawałka na siatce grafiki (`[vz_grid]`).** Przewinięcie `-ss`
zostawia pierwszą klatkę materiału trochę po zerze (0,003–0,024 s w pomiarze
2026-11-13), więc `fps` ErsatzTV kładzie ją na klatce 1. Nakładka dawała jej
grafikę klatki 1, a wyjście (`-fps_mode cfr`) wypełniało klatkę 0 jej kopią.
Grafika na każdej granicy kawałka (co 44 s) szła więc 1 1 2 3… zamiast
0 1 2 3…: przeskok o klatkę i postój, widoczny na crawlu jako drgnięcie.
Nakładka (każda: CPU, VAAPI, CUDA, synchronizacja w RAM) przepuszcza główne
wejście pierwszego `overlay` w grafie przez `fps=<klatkaż grafiki>:start_time=0`
(łańcuch `[vz_grid]`). Materiał ma tu już klatkaż kanału, po `fps`, płynnych
przejściach i `-r`. Klatka 0 dostaje grafikę klatki 0, a czasy klatek
materiału się nie zmieniają (bez przesunięcia względem dźwięku). Pomiar
crawla na lokalnym stosie (kanał 1, 50 kl./s, 4 granice kawałków w 186 s):
taśma o 3 px w każdej klatce poza animacją wejścia (wcześniej para „+6 px, 0”
na każdej granicy).

Każdy `hwupload_cuda` otwiera własny kontekst CUDA (ok. 28 MiB), a nakładka
wstawia go dla każdej strefy grafiki (`area: auto`). W transkodowaniu z płótnem
grafiki i jednym urządzeniem `-init_hw_device cuda` nakładka zamienia więc
`hwupload_cuda` bez opcji na `hwupload`: wysyłka idzie przez to urządzenie,
wspólne z filtrami ErsatzTV. Z dekodowaniem CUDA (bez `VIZRS_FFMPEG_CUDA`)
to samo daje 223 → 197 MiB przy jednej strefie. Sam kontekst CUDA
ffmpeg to ok. 30 MiB (kernele ładowane leniwie, `CUDA_MODULE_LOADING` nic nie
zmienia), koder NVENC 1080p50 – 139 MiB z domyślnymi klatkami B (`-surfaces`,
`-delay`, `-preset`, `-tune` nie zmieniają tego istotnie). Przy zmianie
pozycji dwa procesy kanału działają chwilę naraz, więc zapas karty musi
mieścić dwa. Chromium sidecara z `VIZRS_BROWSER_GPU=1` też zajmuje kartę
(EGL; `nvidia-smi` go nie wypisuje, bo proces jest w kontenerze bez CUDA):
przy rendererze natywnym (`renderer: native`) rysuje mało, więc na małej
karcie zostaw go na CPU.

#### Nadrabianie strumienia MPEG-TS (`VIZRS_FFMPEG_READRATE_CATCHUP`)

W trybie kanału MPEG-TS (TS hybrid) ErsatzTV czyta własne HLS kanału osobnym
ffmpeg: `-readrate 1.0 -readrate_initial_burst 6 -i …/iptv/channel/N.m3u8?mode=segmenter
-c copy -f mpegts`. ffmpeg 8 bez `-readrate_catchup` nie nadrabia przestoju:
gdy czytnik dojdzie do krawędzi playlisty, stracony czas przepada na zawsze.
Silnik next dzieli pozycje na kawałki po 44 s, a każdy kawałek startuje kilka
sekund bez nowego segmentu (start ffmpeg, przewinięcie materiału), więc
strumień `.ts` zostaje coraz bardziej za czasem rzeczywistym, a bufor
odtwarzacza się wyczerpuje – widz widzi lagi i buforowanie, choć transkodowanie
idzie 1×. Z `VIZRS_FFMPEG_READRATE_CATCHUP=2` nakładka dopisuje
`-readrate_catchup 2` za `-readrate` wejścia, które jest playlistą HLS
(`.m3u8`); po przestoju czytnik nadrabia do 2× i wraca do zapasu z
`-readrate_initial_burst`. Transkodowanie materiałów (`-readrate` wejścia
pliku) i komenda z własnym `-readrate_catchup` zostają bez zmian. Opcja wymaga
ffmpeg 7.1 lub nowszego (obraz ErsatzTV ma 8.1). Pomiar 2026-10-26 (A1,
kanał 1600×900@25, `4.ts`): bez nadrabiania strumień szedł 0,86–0,92× i po
150 s był 11 s za zegarem; z `2` trzymał stały zapas +6 s (+4 s na chwilę
przy zmianie kawałka).

#### Płynna zmiana klatkażu (`VIZRS_FFMPEG_FPS_BLEND`)

Kanał 25 kl./s z materiałem 60 kl./s (albo 30, 29,97) dostaje od ErsatzTV
(**Normalize Framerate**) filtr `fps=25`: bierze on na przemian co 2. i co 3.
klatkę źródła, więc klatki wychodzą równo co 40 ms, ale ruch skacze o 33
i 50 ms – przy ruchu kamery obraz wyraźnie „laguje” (judder), choć strumień
jest technicznie równy. Z `VIZRS_FFMPEG_FPS_BLEND=1` nakładka zmienia
w transkodowaniu do HLS pierwszy filtr łańcucha wideo materiału
`[0:0]fps=25,…` na `[0:0]framerate=fps=25,…`: klatka wyjściowa leżąca między
dwiema klatkami źródła jest ich mieszanką ważoną czasem, więc ruch jest
równy (z lekką smugą przy szybkim ruchu). Klatki, które trafiają dokładnie
w klatkę wyjścia (materiał 25 albo 50 kl./s), i cięcia scen przechodzą bez
mieszania. Inne użycia `fps` (np. to z `VIZRS_FFMPEG_FPS` przed nakładaniem
grafiki) i `fps` z opcjami zostają bez zmian.

Materiał nie szybszy od wyjścia (25 kl./s na kanale 50 albo 25) zostaje przy
`fps`: jego klatki są powtarzane, nie mieszane – mieszanie co drugiej klatki
dałoby podwójne krawędzie w ruchu. Klatkaż materiału (`avg_frame_rate`,
inaczej `r_frame_rate`) podaje `ffprobe` leżący obok ffmpeg (`VIZRS_FFMPEG`
z nazwą `ffprobe`). Przez pośrednika ErsatzTV (Jellyfin, Plex, Emby) sprawdzenie
trwa kilka sekund (s2: 6–7 s), więc nie czeka na nie żaden kawałek:
pierwszy kawałek materiału uruchamia `ffprobe` w tle i idzie z mieszaniem jak
dotąd, a wynik trafia do pamięci podręcznej (`$TMPDIR/vizrs-ffmpeg-rates/`,
klucz: adres wejścia bez parametrów zapytania) dla następnych kawałków
(co 44 s) i powtórek tego samego pliku (np. identów). Sprawdzenie bez wyniku
(brak `ffprobe`, błąd) jest powtarzane po 60 s; do tego czasu działa
mieszanie. Pomiar 2026-10-26 (A1,
20 s materiału 1080p60 → 1600×900@25, x264 `superfast`): 23,2 s CPU z `fps`,
23,6 s z `framerate` (+2%).

#### Sygnał 60 kl./s na kanale 25 albo 50 (`decimate`)

Stacja nadająca w 50 Hz trafia na YouTube jako 60 kl./s przez powtórzenie co
szóstej klatki (TVP Info, S1 2026-11-20: 91 kopii co 6 klatek na 10 s). Sam
`fps=25` bierze wtedy klatki o fazach ruchu 0, 2, 4, 6, 9, 10… (kroki 2-2-2-3-1),
a `fps=30` – kroki 2-2-1 z okazjonalną kopią: ruch nierówny. Przebieg live
dostaje więc, gdy sidecar mierzy sygnał ok. 60 kl./s (59–61, pole `fps`
w `/ingest/state`), a kanał ma 25 albo 50 kl./s, filtr `decimate=cycle=6`
przed skalowaniem: z każdych 6 klatek odpada najbardziej podobna do
poprzedniej – właśnie powtórzona – i do `fps` idzie równe 50 kl./s (25 =
co druga, kroki stale 2). Prawdziwe 60 kl./s traci najspokojniejszą klatkę
z sześciu – nie gorzej niż sam `fps`. Inne klatkaże i starszy sidecar (bez
`fps`) – bez zmian. Ślad przejęć podaje klatkaż źródła („live from … (source
60 fps)”). Koszt na A1: 10 s 1080p60 przez `decimate` z dekodowaniem ok. 2 s
jednego wątku (dekodowanie przebieg live robi i tak). Dla takiego źródła
lepiej więc zostawić kanał na 25 kl./s niż przestawiać go na 30.

#### Preset kodera (`VIZRS_FFMPEG_PRESET`)

Silnik strumieniowania next (`Channel.StreamingEngine = 1`, wymagany przez
nakładkę) nie przekazuje do ffmpeg presetu ani profilu z profilu FFmpeg
ErsatzTV. Bez sprzętowego kodera x264 koduje wtedy w presecie `medium`, za
wolnym na A1 (§4). Z `VIZRS_FFMPEG_PRESET=veryfast` nakładka dopisuje
`-preset:v veryfast` zaraz po `-vcodec libx264` (albo `libx265`) w każdym
wywołaniu, także bez grafiki VizRS. Komenda, która ma już `-preset`, i kodery
sprzętowe (NVENC, VAAPI) zostają bez zmian. Sprawdzenie: nagłówek x264
w strumieniu (`grep -a -o "x264 - core.\{0,400\}"` na nagranym `.ts`) pokazuje
`subme=7 … trellis=1` przy `medium`, `subme=2` przy `veryfast`, `subme=1` przy `superfast`.

Preset (razem z profilem H.264 i opcjami x264) ustawia się w panelu; zmienna
wypełnia tylko pierwszy raz `encoder.json` (niżej, „Koder z panelu”).

#### Opcje x264 (`VIZRS_FFMPEG_X264_PARAMS`) i koder z panelu

Z `VIZRS_FFMPEG_X264_PARAMS=bframes=0:subme=0` nakładka dopisuje
`-x264-params:v bframes=0:subme=0` po `libx264` (przed nim preset) w każdym
wywołaniu. Gdy ErsatzTV podał już `-x264-params`, opcje nakładki trafiają na
jego koniec (`<ErsatzTV>:<nakładka>`), więc wygrywają przy tym samym kluczu.
Inne kodery (libx265, NVENC, VAAPI, QSV) zostają bez zmian.

Pomiar 2026-10-31 ([plany/archiwum/PLAN-ampere.md](plany/archiwum/PLAN-ampere.md), s2: Neoverse-N1,
1600×900@25, x264 `superfast`, 3000 kb/s; CPU / SSIM Y względem samego
presetu; materiał Gothic reprezentuje antenę, Warmianin z mocnym ziarnem nie):

| Opcje | CPU Warmianin / Gothic | SSIM Warmianin / Gothic |
|---|---|---|
| `subme=0` | −6% / −8% | 0,00 / −0,39 dB |
| `bframes=0` | −11% / −15% | −0,10 / −0,77 dB |
| `bframes=0:subme=0` | −18% / −20% | −0,11 / −1,44 dB |

To wymiana jakości na CPU: włączaj, gdy kanał nie trzyma tempa 1× albo trzeba
zmieścić więcej kanałów. `ultrafast` (−31…−36% CPU, −3,4…−5,0 dB) odrzucono.

**Koder z panelu.** Ustawienia kodera wybiera panel (ekran „Profile
kodowania”, `PUT /api/live/encoder`, [API.md §1](API.md#1-live-api-sidecara-apilive-etv-ffmpeg-ingest-health)). Silnik next ErsatzTV nie przekazuje do
ffmpeg ani presetu, ani profilu z profilu FFmpeg, także koderom sprzętowym
(np. `-vcodec h264_nvenc -b:v 6000k` bez `-preset`/`-profile`), więc dokłada je
nakładka. Sidecar trzyma je w `<etv_config>/vizrs-live/encoder.json`; ten sam
wolumen jest w kontenerze ErsatzTV pod `/config`, a nakładka czyta plik
(`VIZRS_FFMPEG_SETTINGS`) przy każdym uruchomieniu – zmiana działa od
następnego ffmpeg, bez restartu.

| Klucz | Kodery | Dopisuje | Wartości (puste = domyślne kodera) |
|---|---|---|---|
| `preset` | libx264, libx265 | `-preset:v` | `ultrafast` … `placebo` (domyślnie ErsatzTV: `medium`) |
| `profile` | libx264, `h264_nvenc`, `h264_qsv`, `h264_vaapi` | `-profile:v` | `baseline`, `main`, `high`; VAAPI: `baseline` → `constrained_baseline`. Domyślnie x264 `high`, **NVENC `main`**, QSV i VAAPI – sterownik |
| `x264_params` | libx264 | `-x264-params:v` | opcje z białej listy (niżej) |
| `nvenc_preset` | `*_nvenc` | `-preset:v` | `p1` (najszybszy) … `p7` (najlepsza jakość); domyślnie `p4` |
| `qsv_preset` | `*_qsv` | `-preset:v` | `veryfast` … `veryslow`; domyślnie sterownik |
| `bframes` | `*_nvenc`, `*_qsv`, `*_vaapi` | `-bf:v` | 0–4; mniej klatek B = mniej pamięci karty (NVENC 1080p50: 139 MiB domyślnie, 79 MiB przy `0`) |

Komenda z własnym `-preset`, `-profile` albo `-bf` zostaje przy swoim. Kodery
HEVC nie dostają profilu (inne nazwy). VAAPI nie ma presetu (`-quality` działa
inaczej w każdym sterowniku), dostaje tylko profil i klatki B.

- **Klucz w pliku obowiązuje, nawet pusty.** Pusty = domyślne kodera, nie
  wartość z `.env`. Klucza, którego w pliku nie ma (plik starszej wersji),
  i całego pliku, gdy go brak (np. bez `etv_config`), dotyczą zmienne
  środowiskowe kontenera ErsatzTV. Panel zapisuje zawsze wszystkie klucze.
- **`.env` tylko na start.** Sidecar przy starcie, gdy pliku jeszcze nie ma,
  zapisuje go raz z `VIZRS_FFMPEG_PRESET`, `VIZRS_FFMPEG_X264_PARAMS` i
  `VIZRS_FFMPEG_NVENC_BF` (compose podaje je też usłudze `cg`) – jak
  `inputs.json` z `VIZRS_INGEST`. Potem zmiana `.env` nic nie robi.
- **Walidacja.** Wartości spoza list i opcja x264 spoza białej listy
  (`src/encoder.rs`: jakość i szybkość – `bframes`, `subme`, `ref`, `me`,
  `trellis`, `aq-*`, `psy-rd`, `rc-lookahead` …; bez `keyint`/`scenecut`, wątków,
  sterowania bitrate i opcji z plikiem) są odrzucane przy zapisie i pomijane
  przy odczycie.

#### Klatkaż kanału (`VIZRS_FFMPEG_FPS`)

ErsatzTV nie ma ustawienia klatkażu kanału. Bierze klatkaż materiałów, a przy
**Normalize Framerate** w profilu FFmpeg najniższy z nich. Kanał z materiałami
25 kl./s ma więc 25 kl./s. Z `VIZRS_FFMPEG_FPS=50` nakładka zmienia każde
transkodowanie do HLS:

- zmianę klatkażu materiału z **Normalize Framerate** (`[0:0]fps=25,…`)
  kieruje na klatkaż kanału (`[0:0]fps=50,…`): materiał 30 i 60 kl./s idzie
  wprost do 50, zamiast przez 25 z każdą klatką powtórzoną, a z
  `VIZRS_FFMPEG_FPS_BLEND` jest mieszany (niżej); `VIZRS_FFMPEG_FPS=25`
  ujednolica kanał do 25 kl./s także wtedy, gdy najwolniejszy materiał
  ma 30;
- do wideo przed pierwszym filtrem nakładania wstawia `fps=50`
  (`[v_m0][v_s0]overlay…` → `[v_m0]fps=50[v_m0_fps];[v_m0_fps][v_s0]overlay…`),
  więc grafika jest nakładana i porusza się w 50 kl./s, a nie tylko dublowana
  na wyjściu;
- przed `-f hls` dodaje `-r 50`, także w pozycjach bez grafiki VizRS (reklamy,
  komenda oryginalna), żeby cały kanał miał jeden klatkaż;
- surowe klatki grafiki idą w 50 kl./s, a sidecar przestawia przechwytywanie
  Chromium na każdą klatkę (`/health`: `fps` przechwytywania).

Inne wywołania (koder płótna ErsatzTV, kopiowanie strumienia `-c copy`,
remuks do MPEG-TS) zostają bez zmian. Obraz materiału 25 kl./s nie robi się
płynniejszy, bo każda jego klatka jest powtórzona; płynniejsza jest grafika.

Klatkaż 30 albo 60 kl./s (`VIZRS_FFMPEG_FPS=30`/`60`) ma sens tylko przy
materiałach 30 albo 60 kl./s. Przy materiałach 25 kl./s psuje obraz: przy 30
co piąta klatka filmu jest powtórzona, przy 60 klatki filmu trwają na
przemian 2 i 3 klatki wyjścia (60/25 = 2,4), więc ruch w materiale robi się
nierówny. Grafiki ze screencastu sam klatkaż nie wyrównuje, choć 30 i 60
dzielą 60 Hz Chromium: pomiar (§8) przy 30 i 60 kl./s dał 34–57% równych
kroków, tyle co przy 25 i 50, bo pod obciążeniem Chromium nie rysuje równo
co 1/60 s. Równą grafikę przy każdym klatkażu daje tryb `frames` (§1,
Płynność animacji).
Koszt: animacja grafiki to dwa razy więcej PNG w Chromium i sidecarze (pomiar
na Xeon E3-1270 v3, 1080p50: ok. 170% CPU sidecara w trakcie animacji, kilka
procent przy grafice stojącej), surowe klatki 1080p to ok. 260 MB/s, a koder
dostaje dwa razy więcej klatek (`-g 50` z profilu oznacza wtedy GOP 1 s).

Gdy przez 30 s nie ma żadnej sesji danego rozmiaru (ErsatzTV transkoduje
kanał tylko przy widzach), przechwytywanie przechodzi w `paused`: Chromium
zostaje uruchomiony ze stroną `/output`, ale nie renderuje klatek (przy
animowanym pasku 1080p z ~200% CPU do kilku procent). Pierwsza sesja
wznawia je i od razu dostaje świeżą klatkę. Obrazy sprzed pauzy (nawet
sprzed wielu minut: inna plansza, stara godzina zegara) są wtedy
zapominane: strumień czeka na pierwszy nowy obraz (najwyżej 1,5 s) i to on
idzie także jako zapas `lead_seconds`. Wcześniej te sekundy zapasu brały
ostatni obraz sprzed pauzy, więc po wznowieniu kanał przez ok. 4 s
pokazywał nieaktualną grafikę. Przerwa między pozycjami
harmonogramu jest dużo krótsza, więc w trakcie emisji pauza nie występuje.

### Live ingest

Kanał można przełączyć z harmonogramu na
przychodzący sygnał na żywo (RTMP albo SRT, YouTube albo strumień z linku –
HLS, RTMP, SRT, RTSP). ErsatzTV zostaje bez zmian:
sygnał przyjmuje osobna usługa, sidecar trzyma jego stan, a podmianę obrazu
robi nakładka ffmpeg.

```
OBS/koder ── RTMP/SRT ──► vizrs-ffmpeg vizrs-ingest ── POST /ingest (MPEG-TS) ──► sidecar
YouTube ◄── yt-dlp, ffmpeg ┤(ffmpeg -listen, -c copy)
HLS/RTMP/SRT/RTSP ◄── ffmpeg┘                              │
ErsatzTV ── ffmpeg kawałka ──► vizrs-ffmpeg ── GET /ingest/state, GET /ingest ◄────┘
                                  └─ ta sama playlista HLS: kawałek │ live │ kawałek
```

1. **Odbiór** (`vizrs-ffmpeg vizrs-ingest`, [src/listener.rs](../src/listener.rs)):
   dla każdego wpisu `kanał=URL` z `VIZRS_INGEST` ffmpeg czeka na nadawcę
   (RTMP `-listen 1`, SRT `mode=listener`), przepakowuje strumień do MPEG-TS
   bez transkodowania (`-c copy`; pierwszy obraz i pierwszy dźwięk) i wysyła
   go do sidecara (`POST /ingest?channel=N&input=<id wejścia>`, `Authorization: Bearer
   <VIZRS_INGEST_TOKEN>`). Nadawca, który przez `VIZRS_INGEST_IDLE_MS`
   (domyślnie 15000 ms) nic nie wyśle, jest rozłączany, a ffmpeg czeka na
   następnego. Próg jest celowo dłuższy niż cisza, po której sidecar
   kończy live (`ingest_idle_seconds`, domyślnie 8 s, niżej): krótsza przerwa zdejmuje tylko sygnał z anteny,
   nadawca zostaje połączony, a gdy dane wrócą, odbiornik w ciągu ok. sekundy
   łączy się z sidecarem ponownie i live wraca od klatki kluczowej. Rozłączony nadawca musi połączyć się od nowa – przekaźnik
   czytający HLS (np. datarhei Restreamer: segmenty po 2 s wysyłane paczkami)
   robi to dopiero po kilkunastu sekundach. Dlatego próg nie powinien być
   krótszy niż `ingest_idle_seconds` sidecara (przy 2000 ms przekaźnik HLS był rozłączany co godzinę
   z ok. 19 s przerwy). Sidecar nieosiągalny = dane przepadają, a odbiornik łączy się
   ponownie co sekundę (nadawca zostaje). W RTMP ostatni człon ścieżki to
   klucz strumienia: ffmpeg przyjmuje każdą nazwę i tylko ostrzega, więc
   odbiornik sam rozłącza nadawcę z innym kluczem (`wrong stream key` w logu).
   SRT przyjmuje MPEG-TS (nadawca `srt://serwer:9000?mode=caller`); hasło
   ustawia się w adresie (`passphrase=…`, 10–79 znaków, ten sam u nadawcy).
   MPEG-TS ffmpeg analizuje 2 s (`-analyzeduration`, domyślnie 5 s – o tyle
   późniejsze przejęcie); ścieżka, która zacznie się później, przepada
   (dźwięk: cisza do końca sygnału). Każdy pakiet idzie od razu (`-flush_packets 1`).
   Do 16 odbiorników, każdy z innym adresem; kilka może zasilać jeden kanał.
   **Które wejście bierze kanał:** sidecar przyjmuje na kanał jedno źródło
   naraz. Bez wyboru (automatycznie) wygrywa pierwsze z sygnałem, następne
   dostaje 409 i próbuje co sekundę – gdy pierwsze zejdzie, przejmuje kanał.
   Operator może wybrać wejście kanału (Live API `PUT /api/live/channels/<n>`
   `{"input": "<id>"}`, panel: lista „Wejście” na karcie kanału w Sloty na
   żywo; zapis w `control.json`, przeżywa restart): wtedy kanał bierze tylko
   to wejście – źródło z innego schodzi od razu (sidecar zamyka jego
   połączenie), a pozostałe są odrzucane, choćby wybrane nie miało sygnału
   (kanał wraca wtedy do ramówki). Odbiorniki RTMP/SRT dalej przyjmują
   nadawców, więc po przełączeniu na nie sygnał wchodzi w ciągu ok. 1 s;
   wejścia YouTube i z linku nie wybrane znikają z listy dla usługi `ingest`
   (`GET /ingest/listeners`) i nie pobierają strumienia – po przełączeniu na
   nie usługa uruchamia je w ciągu ok. 5 s (YouTube: plus zapytanie yt-dlp).
   Stan każdego wejścia w Live API: nadaje, sygnał czeka (odrzucony w
   ostatnich 15 s – odbiornik ponawia, gdy przyjdą dane, a HLS przysyła segmenty do 10 s naraz; w dzienniku zdarzeń odmowa raz, gdy wejście zaczyna czekać), nie pobiera (wybrane inne), brak. Wybrane wejście, które
   zniknie z listy (zmiana wejść, koniec filmu YouTube), przywraca
   „automatycznie” (zdarzenie w dzienniku operacji).
   **Lista odbiorników z sidecara:** co 5 s usługa pyta `GET /ingest/listeners`
   (ten sam token; odpowiedź w formacie `VIZRS_INGEST`) i sama uruchamia nowe
   oraz zatrzymuje zmienione odbiorniki – bez restartu, pozostałe trwają.
   Sidecar podaje wejścia ustawione w panelu (`PUT /api/live/inputs`, [API.md §1](API.md#1-live-api-sidecara-apilive-etv-ffmpeg-ingest-health),
   plik `<etv_config>/vizrs-live/inputs.json`), a bez pliku – swoje `VIZRS_INGEST`.
   Starszy sidecar (404) albo nieosiągalny przy starcie = `VIZRS_INGEST` usługi.
   Wejścia z panelu używają tylko portów, które compose wystawia: RTMP
   1936–1943, SRT 9000–9007 (UDP), po jednym odbiorniku na port.
   **Wejście YouTube** (wpis `kanał=https://www.youtube.com/…` albo
   `https://youtu.be/…`, bez portu): usługa sama pobiera strumień zamiast
   czekać na nadawcę. `yt-dlp` (`VIZRS_YTDLP`, domyślnie `/vizrs/yt-dlp` –
   samodzielna binarka z Pythonem w środku, przypięta w `Dockerfile` do wersji
   i sum SHA-256, instalowana przez sidecara na wolumen `vizrs-client` obok
   `vizrs-ffmpeg`) znajduje adresy obrazu i dźwięku: H.264 do 1080p i AAC
   (VP9, AV1 i Opus nie przechodzą do MPEG-TS bez transkodowania), najwyżej
   90 s na zapytanie. Zadania JavaScript YouTube („n challenge” – bez nich
   klienci `web`/`web_embedded`, których yt-dlp używa z ciasteczkami konta,
   nie dają adresów: „No video formats found!”) rozwiązuje **Deno**
   (`VIZRS_DENO`, domyślnie `/vizrs/deno` – oficjalna binarka, przypięta
   w `Dockerfile` do wersji i sum SHA-256 archiwów, ok. 96 MB, na tym samym
   wolumenie; yt-dlp dostaje `--js-runtimes deno:/vizrs/deno`, gdy plik
   jest – starszy sidecar go nie instaluje i yt-dlp idzie bez niego).
   Sidecar porównuje pliki klienta kawałkami, nie wczytuje ich w całości.
   ffmpeg czyta je (`-reconnect`) i przepakowuje do
   MPEG-TS jak strumień nadawcy – dalej wszystko jak przy RTMP/SRT (przejęcie,
   `hold`, `anytime`, grafika `on_live`). Rodzaje adresu:
   - **strona live kanału** (`…/@nazwa/live`, `…/channel/<ID>/live`) – stałe
     wejście: każda transmisja kanału przejmuje antenę, bez transmisji
     odbiornik pyta ponownie co 30 s;
   - **jeden film** (`watch?v=…`, `youtu.be/…`, `/live/<ID>`) – transmisja do
     jej końca (zapowiedziana: pytanie co 30 s, aż ruszy) albo nagranie raz,
     od początku, w czasie rzeczywistym (`-re`; przerwany odczyt idzie dalej
     od miejsca przerwania). Potem odbiornik zgłasza koniec
     (`POST /ingest/listeners/done?channel=N&url=…`, ten sam token; 200 także,
     gdy wpisu już nie ma, 400 dla strony live kanału),
     a sidecar usuwa wpis z listy i z `inputs.json` (zdarzenie w dzienniku
     operacji) – restart usługi nie odtworzy go ponownie. Starszy sidecar
     (404) wpisu nie usuwa: film nie wraca do restartu usługi `ingest`.
   Odmowa YouTube (np. „Sign in to confirm you're not a bot” z adresów
   serwerowni – tak odpowiada S1 i S2 w Oracle Cloud) trafia do logu usługi
   z podpowiedzią i jest ponawiana co 30 s; wtedy pomagają **ciasteczka
   zalogowanego konta**. Wgrywa je VizRS Live (Sloty na żywo → „Ciasteczka
   YouTube…”, `PUT /api/live/youtube-cookies`, [API.md §1](API.md#1-live-api-sidecara-apilive-etv-ffmpeg-ingest-health)):
   sidecar sprawdza plik (format Netscape `cookies.txt` albo JSON z
   rozszerzenia przeglądarki; ciasteczka youtube.com, do 512 KB), zapisuje go
   jako Netscape w `<etv_config>/vizrs-live/youtube-cookies.txt` z prawami
   0600, a do panelu oddaje tylko liczbę ciasteczek, czy konto jest
   zalogowane (`SID`, `__Secure-1PSID`, `__Secure-3PSID`, `SAPISID`,
   `LOGIN_INFO` youtube.com) i najbliższą datę wygaśnięcia – wartości nie
   wychodzą z serwera ani do logu (dziennik operacji ma tylko „ciasteczka
   YouTube wgrane/usunięte”). Usługa `ingest` widzi ten plik na tym samym
   wolumenie (`VIZRS_YTDLP_PANEL_COOKIES`, domyślnie
   `/etv-config/vizrs-live/youtube-cookies.txt`) i przed każdym pytaniem
   YouTube daje yt-dlp jego prywatną kopię na wejście (`$TMPDIR`, 0600 –
   yt-dlp zapisuje odświeżone ciasteczka do pliku, który czyta, a dwa wejścia
   naraz nie mogą pisać do jednego); kopię robi od nowa, gdy plik z panelu
   się zmieni, i wtedy wejście czekające na ponowienie pyta od razu, bez 30 s.
   YouTube co jakiś czas wymienia ciasteczka sesji, a stare użyte ponownie
   bierze za wykradzione i kończy sesję (yt-dlp: „The provided YouTube account
   cookies are no longer valid”). Dlatego po każdym pytaniu odbiornik zapisuje
   ciasteczka odświeżone przez yt-dlp z powrotem do pliku z panelu (obok
   i zmiana nazwy, 0600, właściciel jak katalogu – sidecar dalej go czyta),
   o ile plik nie zmienił się od zrobienia kopii (nowe wgranie wygrywa)
   i o ile yt-dlp nie stracił sesji: gdy YouTube ją zakończy, kasuje
   ciasteczka logowania (`LOGIN_INFO`, `SID`, `SAPISID`…, zostaje m.in.
   `__Secure-3PSID`) – wtedy plik z panelu zostaje, jak był, odbiornik raz
   loguje „YouTube ended the account's session … upload new cookies”
   i zostawia znacznik `youtube-cookies.ended` (czas), który panel pokazuje
   na czerwono („YouTube zakończył sesję …”), a nowe wgranie kasuje (S1
   2026-11-20: po 5 h nadawania sesja wygasła, a okrojony plik nadpisał
   wgrany – live stanął z „Sign in to confirm you're not a bot”);
   następna kopia (ponowny wybór wejścia, restart, inne wejście) zaczyna od
   najnowszych. Te same ciasteczka na dwóch serwerach – jeden wymienia,
   drugi używa starych – też kończą sesję: każdy serwer potrzebuje własnego
   eksportu. Log przy unieważnieniu: „the cookies are no longer valid … –
   upload new ones”. yt-dlp zapisuje ciasteczka sesyjne bez daty – sidecar
   je przyjmuje (data 0).
   **Podtrzymanie sesji** (od 26.11.25): wejścia pytają YouTube tylko przy
   starcie i gdy adres transmisji wygaśnie (ok. 6 h), a sesja zostawiona na
   godziny bez ruchu padała (S1 2026-11-21: między 09:06 a 13:22 UTC, wyszło
   dopiero przy ponownym wyborze wejścia). Dlatego usługa `ingest` co
   `VIZRS_YTDLP_KEEPALIVE_MIN` minut (domyślnie 30; 0 = wyłączone; pierwszy
   raz 2 min po starcie) – także bez żadnego wejścia YouTube – daje yt-dlp
   kopię ciasteczek z panelu i każe przeczytać jedną pozycję strony
   widocznej tylko dla zalogowanego konta (`https://www.youtube.com/feed/history`,
   bez pobierania i bez Deno). Odświeżone ciasteczka wracają do pliku
   z panelu jak po pytaniu wejścia. Sesja żywa = znacznik
   `youtube-cookies.checked` (czas, panel: „Sesja sprawdzona i podtrzymana
   …”); sesja zakończona (yt-dlp: „The provided YouTube account cookies are
   no longer valid” – kod wyjścia 0, tylko ostrzeżenie – albo „Login details
   are needed”, albo zniknięcie ciasteczek logowania) = od razu znacznik
   `youtube-cookies.ended`, alarm w panelu i koniec podtrzymywania do nowego
   wgrania; inna odmowa (sieć, odmowa adresu) – tylko wiersz logu
   („YouTube session keep-alive: …”, bez powtórzeń) i próba za interwał.
   yt-dlp z ciasteczkami z panelu działa zawsze pojedynczo (blokada na
   wejścia i podtrzymanie): dwa naraz odświeżałyby tę samą sesję i jeden
   zostałby z wymienionymi ciasteczkami. Ostrzeżenie „cookies are no longer
   valid” przy pytaniu wejścia też stawia znacznik. Własny plik
   (`VIZRS_YTDLP_COOKIES`) nie jest podtrzymywany.
   Usunięcie w panelu = wejścia bez ciasteczek. `VIZRS_YTDLP_COOKIES`
   (własny plik, zapisywalny) wygrywa z plikiem z panelu. Ciasteczka wygasają
   albo YouTube je unieważnia (wylogowanie, eksport z otwartej dalej
   przeglądarki) – wtedy log mówi „with the cookies: they may have expired”
   i trzeba wgrać nowe; najlepiej z osobnego konta, bo YouTube może je
   zablokować za pobieranie. Wejście YouTube nie potrzebuje portów
   ani klucza: wchodzi na antenę jak sygnał ze studia, więc poza audycjami
   LIVE tylko przy `anytime` kanału. Pobieranie treści z YouTube podlega jego
   regulaminowi i prawom autorów – za uprawnienia do emisji odpowiada nadawca.
   **Wejście z linku** (wpis `kanał=<adres strumienia>`, bez portu): usługa
   sama pobiera strumień spod adresu – HLS (`https://…/index.m3u8`) i inny
   strumień HTTP(S) (np. MPEG-TS), RTMP(S) (`rtmp://serwer/app/klucz`), SRT
   jako caller (`srt://serwer:port`, `mode=caller` dopisywany, gdy brak
   `mode=`) albo RTSP(S) (`rtsp://kamera/…`, transport TCP). `rtmp://` i
   `srt://` na `0.0.0.0`, `[::]` albo adresie lokalnym (`127.*`, `[::1]`, `localhost`) to odbiornik, każdy inny host – link;
   adres `https://` YouTube to wejście YouTube. ffmpeg czyta strumień
   (`-c copy`, pierwszy obraz i pierwszy dźwięk; obraz H.264 i dźwięk AAC
   jak od nadawcy) i dalej wszystko jak przy RTMP/SRT. Strumień HTTP idzie w
   czasie rzeczywistym (`-re` – nagranie albo plik nie przeleci w sekundę;
   na żywo nie zmienia nic), z `-reconnect`; każdy rodzaj ma limit odczytu
   i połączenia 15 s (`-rw_timeout`/`-timeout`), więc milczący serwer kończy
   odczyt. Lista protokołów ffmpeg jest zawężona do schematu linku
   (`-protocol_whitelist`, np. dla HTTP: `http,https,tcp,tls,crypto,httpproxy`)
   – lista HLS nie wskaże pliku lokalnego. Z listy wariantów HLS (master)
   ffmpeg bierze pierwszy wariant – inny wybiera się linkiem do jego listy.
   Wejście z linku jest stałe: po końcu albo zerwaniu strumienia odbiornik
   czyta go ponownie po 1 s, a gdy nic nie przyszło – co 10 s (w logu raz,
   do pierwszego udanego odczytu). Adres do 1000 znaków; tokeny w adresie
   ukrywa log (jak klucz RTMP) i tabela wejść panelu. Link wskazuje serwer, z
   którym łączy się usługa `ingest` – także w sieci wewnętrznej, więc
   wejścia ustawia tylko operator panelu. Jak YouTube – poza audycjami LIVE
   na antenę tylko przy `anytime` kanału.
2. **Stan w sidecarze** ([src/ingest.rs](../src/ingest.rs)): kanał jest „live”
   od pierwszej klatki kluczowej obrazu (flaga *random access* w MPEG-TS albo
   IDR/SPS H.264, IRAP/VPS/SPS HEVC) do końca połączenia albo ciszy dłuższej
   niż `ingest_idle_seconds` (env `VIZRS_INGEST_IDLE_SECONDS`, domyślnie 8 s,
   co najmniej 1 s). Do 26.11.1 było to stałe 5 s: przekaźnik czytający HLS
   (datarhei Restreamer, segmenty 2 s z kawałków 4 s ErsatzTV) potrafi milczeć
   ok. 4 s, a skok znaczników czasu w źródle z `-re` nawet dłużej, więc przy
   5 s sygnał co chwilę spadał z anteny i wracał.
   Sidecar trzyma ostatnie PAT i PMT oraz strumień od ostatniej klatki
   kluczowej (do 32 MB), więc czytelnik dołącza od razu od obrazu. Jeden sygnał
   na kanał: drugi, gdy pierwszy trwa, dostaje 409 (aktywne przejęcie nie jest
   wywłaszczane). Bez `ingest_token` (`VIZRS_INGEST_TOKEN`)
   ingest jest wyłączony, a `POST /ingest` odpowiada 404.
   `GET /ingest/state?channel=N` → `{"enabled":true,"anytime":true,"hold":false,
   "live":true,"audio":true,"video":"h264","fps":60,"seconds":12}` (`fps` –
   klatkaż sygnału z PTS ostatnich 64 obrazów: mediana kroku po posortowaniu,
   więc klatki B nie przeszkadzają; `null` przed 16 obrazami i bez sygnału;
   `live_watermark` – znaki wodne ErsatzTV przejęcia; `graphics` – czy na
   antenie kanału jest teraz grafika VizRS (warstwy `STATE` czytane przez live
   API co 2 s), dla nakładki pozycji bez elementu VizRS (niżej „Grafika na
   pozycji bez elementu VizRS”); pola nie ma, gdy nikt warstw nie czyta: live
   API wyłączone, brak adresu VizRS albo kanał nieznany sidecarowi),
   `/health` → `ingest`
   (lista kanałów z sygnałem, `null` przy wyłączonym ingest). Kanał wstrzymany
   w live API (niżej) ma `"hold":true` i `"live":false` mimo sygnału, a jego
   `GET /ingest` odpowiada jak bez sygnału (bieżący czytelnik kończy się
   w ciągu sekundy).
   **Ciągłość przebiegów live:** sidecar trzyma ostatnie 20 s sygnału (całe
   grupy obrazów, wg PTS; najwyżej 32 MiB) i kursor kanału. Czytelnik z `take`
   (przebieg live nakładki) zaczyna od klatki kluczowej najbliższej kursorowi,
   który zostawił poprzedni przebieg (jego klatka startowa + `take`), a kursor
   przesuwa się o `take`. ffmpeg czyta wtedy od razu sekundy stracone na start
   i kanał trzyma czas rzeczywisty przez kolejne kawałki ErsatzTV (44 s). Bez
   tego każdy przebieg zaczynał od bieżącej chwili, a start nowego ffmpeg
   (ok. 1 s) i przerwa ErsatzTV między kawałkami (ok. 0,6 s) przepadały:
   zmierzone 0,96× czasu rzeczywistego, zapas HLS kanału topniał i odtwarzacze
   z czasem coraz częściej stawały. Kursor dalej niż 12 s za sygnałem (kanał
   wstrzymany, przebiegi zbyt wolne), brak kursora (pierwszy przebieg, nowy
   nadawca) i źródło bez PTS = bieżąca chwila, jak każdy inny czytelnik
   (`GET /ingest`, slot). Log sidecara: `ingest: channel N: live run of 44.0 s
   from 1.2 s behind the source`. Live na kanale jest przez to o te sekundy
   (zwykle 1–2 s) dalej od źródła. Nadrobienie wymaga jeszcze dwóch rzeczy
   w nakładce: strumień grafiki kawałka między przebiegami czeka na następny
   przebieg zamiast wyrzucać klatki (sidecar odmierza je zegarem od startu
   kawałka i zaległe wysyła naraz), a wejście live ma `-t` pozostałego czasu
   kawałka (niżej). Pomiar na lokalnym stosie (nadawca RTMP 1280×720, widz
   HLS, 10 min live): 1,001× czasu rzeczywistego, kawałki 44 s co 43,9–44,3 s,
   żaden przebieg nie zatrzymany po czasie; wcześniej 0,963× (od bieżącej
   chwili) i 0,937× (sama ciągłość: przebiegi nadrabiały, ale stały do
   `LIVE_OVERRUN`).
3. **Przejęcie** (`VIZRS_FFMPEG_INGEST=1` w ErsatzTV, [src/takeover.rs](../src/takeover.rs)):
   nakładka ffmpeg pyta sidecara o stan kanału przy każdym transkodowaniu
   kawałka do HLS (`-f hls` z `-t`, wideo kodowane, nie kopiowane). Kanał to
   kanał płótna grafiki, a bez płótna (reklama bez elementu VizRS) katalog
   playlisty (`/transcode/<kanał>/`). Przy włączonym ingest kawałek idzie jako
   ciąg przebiegów ffmpeg do tej samej playlisty (ErsatzTV pisze ją z
   `append_list`):
   - komenda ErsatzTV kawałka; gdy kanał przechodzi na live, nakładka kończy ją
     `SIGINT` (ffmpeg zamyka i dopisuje bieżący segment, co ok. 250 ms
     sprawdzenie stanu); gdy nie skończy się w 3 s – `SIGTERM`, po kolejnych
     2 s `SIGKILL` (zawieszony ffmpeg, np. CUDA z pipe'em grafiki, nie może
     trzymać live poza anteną ani pamięci GPU – wcześniej nakładka czekała bez
     końca, a ErsatzTV Next restartował sesję po „ffmpeg stall”);
   - przebieg live: wejście `GET /ingest?channel=N&take=<pozostały czas kawałka, s>`
     (ciągłość niżej), skalowanie z zachowaniem
     proporcji i czarne pasy do rozmiaru kanału, klatkaż kanału, dźwięk 48 kHz
     stereo (bez ścieżki dźwięku: cisza), grafika VizRS nałożona jak w kawałku
     (te same obszary, §1, Obszar grafiki), a koder, jego opcje, `-r` i opcje
     HLS z komendy ErsatzTV; trwa do końca sygnału albo kawałka. Źródło live
     nie kończy się samo, a ffmpeg po osiągnięciu `-t` potrafi czekać na swoje
     wejścia zamiast wyjść – ErsatzTV widział wtedy kawałek stojący ok. minuty
     („ffmpeg stalled”), a odtwarzacze widzów zastygały. Dlatego wejście live
     ma `-t` pozostałego czasu kawałka: koniec wejścia kończy nakładanie
     (`shortest=1`) i przebieg, gdy tylko kawałek jest gotowy – także wtedy,
     gdy przebieg nadrobił start i doszedł do końca przed zegarem. Gdyby mimo
     to żył dłużej niż pozostały czas kawałka plus 3 s, nakładka kończy go
     `SIGINT` (dalej jak wyżej: `SIGTERM`, `SIGKILL`);
   - po zejściu sygnału znowu komenda kawałka od miejsca, w którym jest już
     harmonogram (`-ss` przesunięte o czas, który kawałek wyprodukował).
   Każdy przebieg dostaje `-t` z pozostałym czasem i `-output_ts_offset`
   zwiększony o wyprodukowany czas (z EXTINF nowych segmentów playlisty, tak
   jak liczy go ErsatzTV), więc znaczniki czasu idą dalej jak z jednego
   ffmpeg, a kawałek kończy się dokładnie wtedy, kiedy ErsatzTV tego oczekuje.
   Ślad przebiegów: każdy kawałek, przejście na live, koniec przebiegu
   i zatrzymanie (`SIGINT`/`SIGTERM`/`SIGKILL`) trafia – poza stderr, którego
   ErsatzTV Next nie zachowuje – do pliku
   `$ETV_CONFIG_FOLDER/vizrs-live/logs/vizrs-takeover-<kanał>.log` (obok logów
   sidecara i odbiorników; bez tej zmiennej obok playlisty): czas UTC, PID nakładki, komunikat i stan playlisty (media
   sequence, liczba segmentów, ostatni z `PROGRAM-DATE-TIME`, pliki `.ts`
   na dysku). Plik powyżej 2 MB przechodzi w `….1` (jeden stary zostaje).
   Do 26.11.16 ślad leżał w folderze logów ErsatzTV (`logs/`) – a ekran logów
   legacy (`GetRecentLogEntries`, panel: Logi) czyta „najnowszy” plik tego
   folderu wg nazwy, czyli `vizrs-takeover-…` zamiast `ersatztv<data>.log`,
   i pokazywał zero wpisów. Nakładka przy pierwszym śladzie przenosi stare
   pliki `vizrs-takeover-*` z `logs/` do `vizrs-live/logs/` (dopisując do
   istniejących); sidecar czyta oba miejsca.
   Koniec przebiegu live podaje też czas zegara i tempo (`live run ended after
   40.9 s in 47.0 s (0.87x, exit 255)`), a przebieg dłuższy niż 2 min co 2 min
   dopisuje `live run made … s in … s (…x)`. Tempo poniżej 1,00x oznacza, że
   kanał zostaje za zegarem: bufor HLS (`ts on disk`) topnieje, aż odtwarzacze
   dostaną pustą playlistę (S1, 2026-11-13: ok. 0,9x przez pół godziny, od
   35 segmentów do zera). Po stronie sidecara ślad zostawia kursor ciągłości:
   gdy poprzednie przebiegi zostawiły go dalej niż 12 s za źródłem, sidecar
   zaczyna od bieżącej chwili i pisze ostrzeżenie `live run of … s from the
   live edge: the runs before left it … s behind`.
   Live zastępuje harmonogram, nie przesuwa go: program po powrocie idzie
   od bieżącego miejsca, a ominięta część przepada.
   Kawałek, który zaczyna się w trakcie live, od razu idzie
   z sygnału.

Rozmiar kanału dla przebiegu live, po kolei: rozdzielczość profilu FFmpeg
kanału w ErsatzTV (`GET /api/channels` → `ffmpegProfile`, `GET /api/ffmpeg/profiles`
→ `resolution`; pod `VIZRS_BRIDGE_URL`, klucz z `api-secrets.json`, raz na proces,
najwyżej 1,5 s), `pad`/`crop`, a potem `scale` z filtrów ErsatzTV, klatki grafiki
całego kanału, najnowszy segment w katalogu playlisty (`ffprobe`),
w ostateczności 1920×1080. Segment jest na końcu, bo to wynik poprzedniego
przebiegu: przebieg live w złym rozmiarze utrwalał go w kolejnych. ErsatzTV nie
skaluje kawałka, gdy materiał pozycji ma rozmiar kanału – tak bywa w slocie
audycji LIVE, którego sygnał przy sondowaniu miał rozmiar kanału; po restarcie
ErsatzTV nie było jeszcze segmentu i przebieg wychodził w 1920×1080 z grafiką
1600×900 w lewym górnym rogu (S1, 2026-11-22). Wybrany rozmiar i jego źródło
są w śladzie przejęć kanału (`channel N: live picture SZERxWYS from …`). Klatkaż: `VIZRS_FFMPEG_FPS`, `-r` wyjścia, klatki
grafiki, 25. Przy koderze VAAPI i Vulkan obraz jest wysyłany do karty
(`format=nv12,hwupload` z urządzeniem z komendy ErsatzTV), przy QSV podany
w NV12; NVENC i kodery programowe biorą go wprost. Opcje dekodera materiału
(`-hwaccel` itd.) przebieg live pomija: sygnał dekoduje się programowo.

Opóźnienie: przejęcie widać po czasie, o który ErsatzTV wyprzedza zegar
(bufor odtwarzacza, `lead_seconds`), plus dołączenie od ostatniej klatki
kluczowej sygnału (GOP nadawcy, ustaw 1–2 s). Kawałek z kopiowanym wideo
(`-c:v copy`, tryb kopiowania ErsatzTV) live nie przejmuje.

Uwaga: sygnał live wchodzi na antenę bez potwierdzenia operatora, więc
porty odbiorników otwieraj tylko z kluczem RTMP albo hasłem SRT (w compose
przykładowy klucz `change-me`), najlepiej tylko dla zaufanych adresów.
`GET /ingest` i `/ingest/state` nie wymagają tokenu, jak reszta API sidecara
(`/ffmpeg/frames`): portu 7700 nie wystawiaj publicznie.

#### Grafika podczas przejęcia (`on_live`)

Gdy live ingest przejmuje kanał (nakładka `vizrs-ffmpeg` z `VIZRS_FFMPEG_INGEST`,
[src/takeover.rs](../src/takeover.rs)), ErsatzTV w tle nadal przetwarza kolejne
kawałki harmonogramu i wywołuje `POST /etv`. Aby grafika ramówki (`on_clip`,
`on_end`, `on_ad`, reguły `shows` i zaplanowane czasy stron) nie trafiała na sygnał
ze studia, sidecar wstrzymuje komendy ramówki na czas przejęcia i opcjonalnie
wprowadza dedykowaną grafikę kanału zdefiniowaną w polu `on_live`
(np. `"on_live": "TAKE 7510"` – logo kanału z dopiskiem „NA ŻYWO”).

- **Definicja**: `on_live` występuje **wyłącznie per kanał** w obiekcie
  `channels.<N>.on_live` (w `config.json` oraz w `graphics.json` przez
  `PUT /api/live/graphics`). Nie ma globalnego `on_live`. Składnia komend jest
  taka sama jak w `on_clip` (wiersze, `#` jako komentarz), natomiast symbole
  ramówki `{…}` są puste, ponieważ przejęcie nie ma pozycji harmonogramu.
- **Grafika wejścia**: wejście z panelu może mieć własne `on_live`
  (`PUT /api/live/inputs`, pole `on_live` wejścia; w `inputs.json` obiekt
  `"on_live": {"<kanał>=<adres>": "TAKE 7510"}` obok `listeners`). Gdy kanał
  przejmuje sygnał z tego wejścia, zamiast `on_live` kanału wchodzi grafika
  wejścia – np. inne logo NA ŻYWO dla transmisji z YouTube niż dla studia.
  Wejście bez własnej grafiki = `on_live` kanału. Sidecar wie, z którego wejścia
  jest sygnał, bo odbiornik podaje je w `POST /ingest?channel=N&input=<id>`
  (`id` – skrót 64-bitowy wpisu `kanał=adres`, bez klucza i hasła w adresie
  żądania); starszy odbiornik bez `input` = `on_live` kanału. Zmiana grafiki
  wejścia działa od następnego przejęcia.
- **Kiedy zachodzi przejęcie**: kanał ma sygnał live (dostępny, niewstrzymany
  przez `hold`/ZDEJMIJ), a nakładka czyta go w przebiegu live
  (`GET /ingest?channel=N&take=…`), bądź taki przebieg zakończył się nie dawniej
  niż 15 s temu (`TAKEOVER_GRACE` – przerwa między kolejnymi kawałkami
  ErsatzTV). Sidecar monitoruje ten stan co 250 ms (wątek `takeovers`). Bez
  nakładki z `VIZRS_FFMPEG_INGEST` przejęcie nie występuje, więc grafika nie ulega
  zmianie. **Audycje LIVE z ramówki** (sloty `live_shows`, Remote Stream
  `/ingest?slot=1`) **nie są przejęciem** – tam grafiką sterują standardowe reguły
  i ustawienia pozycji audycji w harmonogramie (element VizRS pozycji, np. logo
  audycji, reguły `shows` dopasowane do tytułu) oraz jej znaki wodne ErsatzTV.
  Przebieg live slotu czyta studio z `&programme=1` (`GET /ingest?…&take=…&programme=1`),
  którego sidecar nie liczy jako przejęcia – do 26.11.25 liczył i podczas audycji
  wchodziło `on_live` kanału zamiast grafiki audycji (S1 2026-11-21, „BTB Info”
  03:30: `TAKE 7510` zamiast logo audycji 7507). **Znaki wodne audycji**
  (`WatermarkIds` pozycji, np. z okna Audycje LIVE) nakładka kładzie w przebiegach
  live slotu – ze studiem i na planszy bez studia – na wierzchu, jak ErsatzTV:
  przy elemencie VizRS z mostu (jak przebiegi ramówki), bez niego – z mostu wg
  pozycji płótna regionu ErsatzTV (`/internal/graphics/<kanał>/<pozycja>?region=…`;
  log nakładki „N ErsatzTV watermark(s) of the live programme on its live runs”).
  Przy **przejęciu** (studio poza ramówką) znaki wodne pozycji schodzą – na
  antenie jest grafika studia (`on_live` kanału albo wejścia) i jej znak wodny
  ErsatzTV (`live_watermark`, niżej).
- **Początek przejęcia**: sidecar wykonuje `STATE` → `OUT` wszystkich warstw
  kanału w VizRS (zapamiętując aktywne strony), a następnie wysyła komendy
  `on_live` danego kanału (np. `TAKE 7510`). Bez zdefiniowanego `on_live` grafika
  ramówki i tak jest wstrzymywana na czas przejęcia (antena zostaje czysta, nic nie
  wchodzi w jej miejsce).
- **W trakcie przejęcia**: komendy ramówki (start pozycji, `on_end`, reklamy, czasy
  wejścia i wyjścia stron) nie trafiają do VizRS – sidecar śledzi w pamięci,
  które strony ramówki byłyby aktualnie na antenie. Ręczne komendy
  TAKE / CONTINUE / OUT z panelu (`/api/live/channels/<n>/take…`, [API.md §1](API.md#1-live-api-sidecara-apilive-etv-ffmpeg-ingest-health)) działają
  normalnie także w trakcie przejęcia.
- **Koniec przejęcia** (odłączenie nadawcy, cisza powyżej `ingest_idle_seconds`
  lub ZDEJMIJ studia): sidecar zdejmuje strony z `on_live` (`OUT`), po czym
  wysyła jedno `TAKE` dla stron ramówki, które w tym momencie powinny być na
  antenie według śledzonych komend ramówki.
- **ZDEJMIJ / PRZYWRÓĆ GRAFIKĘ**: `POST /api/live/graphics/off` w trakcie
  przejęcia zdejmuje również grafikę live (`on_live`). Przywrócenie grafiki
  w trakcie trwania przejęcia przywraca grafikę live, a nie ramówkę. Jeżeli
  przejęcie zakończy się przy zdjętej grafice, po jej przywróceniu powraca
  aktualna ramówka.
- **Strefy grafiki (`area: auto`)**: gdy kanał ma sygnał live, strony z `on_live`
  są uwzględniane w strefach przebiegu nakładki (logo live nie zostanie ucięte).
- **Znak wodny ErsatzTV przejęcia** (`live_watermark`, od 26.11.26): obok
  `on_live` reguła kanału może mieć znaki wodne ErsatzTV na czas przejęcia
  (`channels.<N>.live_watermark`: numery znaków legacy, `"3"` albo `"3,5"`, do
  8 – w `config.json` i w `graphics.json`, panel: Grafika kanału, reguła kanału),
  a wejście – własne (`inputs.json`: `"live_watermark": {"<kanał>=<adres>": "3"}`,
  panel: Ustaw wejścia…); wejście bez własnego = znak kanału. Sidecar podaje
  nakładce znaki przejęcia w `GET /ingest/state` (`"live_watermark":"3,5"` –
  wejścia, z którego jest sygnał, albo kanału), a nakładka przy każdym przebiegu
  live przejęcia pobiera je z mostu (`GET /vizrs-bridge/watermarks/<kanał>?ids=…`,
  adres mostu `VIZRS_BRIDGE_URL` w kontenerze ErsatzTV, domyślnie
  `http://127.0.0.1:8409`, token `VIZRS_BRIDGE_TOKEN`; ponownie tylko przy innych
  numerach, np. po zmianie wejścia) i kładzie je na wierzchu obrazu studia
  i grafiki, w miejscu i z kryciem jak ErsatzTV. Znaki pozycji ramówki na ten
  czas schodzą. Bez tokenu, bez odpowiedzi w 1,5 s albo przy błędzie – przejęcie
  bez znaku (log nakładki „watermarks of the live takeover (…): …; none
  drawn”); udane – „N ErsatzTV watermark(s) of the live takeover (3)”.
- **Pozycja bez elementu VizRS** (od 26.11.26): klatki grafiki VizRS nakładka
  dostaje z sesji elementu „script” pozycji – pozycja bez elementu VizRS jej
  nie ma, więc wcześniej przebieg live szedł bez grafiki i `on_live` (np. logo
  NA ŻYWO) trafiało do VizRS, ale nie na obraz (S1 2026-11-21: pozycje
  „wypełnij” bez elementu). Teraz nakładka przy pierwszym przebiegu live
  takiego kawałka zakłada w sidecarze własną sesję grafiki kanału
  (`POST /etv?channel=N&live=1`, rozmiar i klatkaż kanału) i przejmuje ją
  (`GET /ffmpeg?channel=N&…&live=1`); jej klatki idą do każdego przebiegu live
  tego kawałka (strefy jak przy elemencie: nakładka przejmuje sesję z `follow=1`, więc sidecar daje strefy stron przejęcia – np. samego logo `on_live` – a poszerzenie w trakcie kawałka, gdy operator wprowadzi inną stronę, restartuje przebieg live w nowych strefach; do 26.11.26 `follow=0` i strefa obejmowała wszystkie strony show, czyli cały kadr – lokalnie ok. 0,2 rdzenia więcej przy 1600×900), a przebiegi ramówki zostają, jak je
  zrobił ErsatzTV (z jego znakami wodnymi). Sesja zakłada się w tle – przebieg
  live na nią nie czeka: rusza bez grafiki i jest restartowany z grafiką, gdy
  sesja jest gotowa (jak przy poszerzeniu stref); kawałek, który zaczyna się
  przy studiu już na antenie (live trwa z poprzedniego kawałka), czeka na nią
  najwyżej 1,5 s, żeby logo nie mrugało co kawałek. Sesja live nie uruchamia reguł
  ramówki (bez `on_clip`, końca pozycji i wykrywania reklam), a zwykła
  nakładka jej nie przejmie (i odwrotnie: nakładka live nie weźmie sesji
  pozycji ErsatzTV). Kończy się z kawałkiem. Błąd sesji = przebieg live bez
  grafiki, jak wcześniej (log nakładki: „graphics of channel N for the runs
  of the piece: …; the runs without them”); udana – „graphics of channel N
  for the runs of the piece (no VizRS element on the item)”, sidecar: „ffmpeg
  overlay took over the graphics of live ingest”.
- **Logi sidecara**: zdarzenia odnotowywane są wpisami `channel N: live takeover:
  schedule graphics held, live graphics: TAKE 7510`, `channel N: live takeover
  over: schedule graphics back`, a odpowiedzi VizRS na komendy przejęcia mają
  dopisek `(live takeover)`.

### Audycje na żywo w ramówce (`live_shows`)

Studio może mieć w ramówce ErsatzTV własne pozycje: audycja na żywo ma tytuł
i opis w EPG, stałą godzinę i długość, a w tym czasie kanał nadaje sygnał
ze studia (live ingest, wyżej). Bez sygnału idzie plansza zastępcza, a nie
program. Pozycja to **Remote Stream** ErsatzTV, którego adres wskazuje
sidecara:

```yaml
# /config/vizrs-live/1-wiadomosci-na-zywo.yml – pisze go sidecar
title: "Wiadomości na żywo"
plot: "Serwis ze studia"
content_rating: "12"
url: "http://cg:7700/ingest?channel=1&slot=1"
is_live: true
duration: "00:30:00"
```

1. **Definicje** ([src/live_shows.rs](../src/live_shows.rs)): lista audycji
   to `<etv_config>/vizrs-live/shows.json`, którą edytuje panel przez Live API ([API.md §1](API.md#1-live-api-sidecara-apilive-etv-ffmpeg-ingest-health)).
   `live_shows` (`VIZRS_LIVE_SHOWS`, JSON) tylko ją zakłada: gdy pliku nie ma,
   sidecar przepisuje do niego wpisy z konfiguracji (w logu `moved to …
   shows.json`), a potem konfigurację pomija. Każdą audycję sidecar zapisuje
   przy starcie, co `sync_seconds` i po zmianie w panelu do
   `<etv_config>/vizrs-live/<kanał>-<tytuł>.yml` (albo `<file>.yml`), tylko gdy
   plik się zmienił. Pól `channel`, `title`,
   `duration` (`"HH:MM:SS"`, `"MM:SS"` albo sekundy, do 23:59:59) nie można
   pominąć; `plot`, `content_rating` (kategoria wiekowa w EPG, do 20 znaków, np. `12`) i `file` są opcjonalne. Nazwa pliku powstaje raz, przy
   dodaniu: zmiana tytułu w panelu zostawia plik, więc pozycja w ErsatzTV i jej
   miejsca w harmonogramach zostają. Adres sidecara pochodzi z adresu
   elementu `vizrs.yml` (domyślnie `http://cg:7700`). Synchronizacja nie kasuje
   plików (ErsatzTV usunąłby pozycję z harmonogramów); usuwa je tylko „Usuń”
   w panelu albo operator ręcznie. Własne pliki `.yml` w tym formacie też działają.
2. **Biblioteka w ErsatzTV**, raz: **Media → Local Libraries → Add Local
   Library**, rodzaj **Remote Streams**, folder `/config/vizrs-live`, skan.
   Audycje są potem zwykłymi elementami biblioteki: kolekcja, pozycja
   harmonogramu (np. **Fixed Start** 19:30, **Playout Mode: One**) i reset
   playoutu. ErsatzTV bierze długość z `duration`, bo próbkowanie adresu nie
   daje długości (niżej), a tytuł i opis idą do EPG i do `{title}` komend
   grafiki (§3). Element **VizRS** na pozycji działa jak na każdej innej.
3. **Adres slotu** (`GET /ingest?channel=N&slot=1`): gdy studio nadaje,
   sidecar podaje sygnał tak jak `GET /ingest`. Bez sygnału odpowiada sekundą
   czerni i ciszy w MPEG-TS (H.264, AAC), bez `Content-Length`, więc ffprobe
   ErsatzTV przy skanowaniu i odtwarzaniu widzi obraz i dźwięk, ale nie
   długość.
4. **Odtwarzanie** (nakładka ffmpeg, `VIZRS_FFMPEG_INGEST=1`): ErsatzTV Next
   gra pozycję live jednym ffmpeg na cały czas audycji (`-t` do jej końca, bez
   przewijania). Nakładka rozpoznaje wejście slotu i nigdy nie uruchamia tej
   komendy. Przez cały czas idą przebiegi jak przy przejęciu (§1, Live
   ingest): studio, gdy nadaje, a gdy nie – plansza: `VIZRS_FFMPEG_STANDBY`
   (obraz w kontenerze ErsatzTV, np. `/media/plansza.png`, skalowany do
   kanału; brak pliku = czerń z wpisem w logu) albo czerń, z ciszą i grafiką
   VizRS. Studio może więc zacząć później i skończyć wcześniej: przed
   wejściem i po zejściu idzie plansza, a o czasie audycji decyduje ramówka.
   Slot działa także przy sidecarze nieosiągalnym na starcie pozycji
   (plansza, stan sprawdzany dalej) i bez względu na `ingest_anytime`.

`ingest_anytime` (`VIZRS_INGEST_ANYTIME`, domyślnie `true`): czy sygnał
przejmuje kanał także poza audycjami na żywo (każdy
kawałek). `false` = studio wchodzi na antenę tylko w swoich pozycjach
ramówki, a w pozostałym czasie jego sygnał niczego nie zmienia (`/ingest/state`:
`"anytime"`). Przełącznik „poza audycjami” (Live API, panel) nadpisuje to dla
jednego kanału.

**Nagła emisja (okno przejęcia, K23).** Emisja na żywo ogłoszona tego samego
dnia o godzinie, którą zajmuje ramówka: operator planuje w panelu okno (data,
od–do, wejście, tytuł), a sidecar w tym czasie traktuje kanał jak przy
`anytime` i bez wstrzymania – sygnał studia przejmuje antenę od pierwszej
klatki kluczowej, bez sygnału gra ramówka. ZDEJMIJ w oknie działa do jego
końca; po końcu kanał wraca do stanu sprzed okna. ErsatzTV, harmonogram i EPG
zostają bez zmian (EPG pokazuje pozycję spod okna, a jej odcinek przepada).
`/ingest/state` ma wtedy `"window": "<tytuł>"` (poza oknem `null`), a
`"anytime"` i `"hold"` już uwzględniają okno. Okna są w `control.json`, więc
restart sidecara przed emisją ich nie gubi. API i walidacja – [API.md §1](API.md).

Bez nakładki (zwykły ffmpeg) ErsatzTV odtworzyłby adres slotu wprost: przy
nadającym studiu działa to jak zwykły Remote Stream (bez grafiki VizRS przez
nakładkę), ale bez studia sekunda czerni kończy źródło i ErsatzTV pokazuje
do końca pozycji własną planszę awaryjną. Audycje na żywo wymagają więc
nakładki z `VIZRS_FFMPEG_INGEST=1`.

### Live API (`/api/live/*`) dla panelu

Sidecar udostępnia Live API sygnałów na żywo i audycji LIVE ([src/panel.rs](../src/panel.rs)), z którego korzysta panel kanału (ekrany „Sloty na żywo”, „Audycje LIVE”, „Grafika kanału”, „Dziennik emisji”). Dostęp zabezpiecza token `panel_token` (zmienna środowiskowa `VIZRS_PANEL_TOKEN` w sidecarze, `VIZRS_LIVE_CG_TOKEN` w panelu); puste = API wyłączone. Pełny opis punktów końcowych, parametrów i formatów odpowiedzi znajduje się w [API.md §1](API.md#1-live-api-sidecara-apilive-etv-ffmpeg-ingest-health).

### Pakiet VizRS – zgodność aplikacji

VizRS CG Playout to jedna z aplikacji pakietu VizRS, które mają być **zawsze ze sobą zgodne** (bieżące wersje działają razem). Opis całego pakietu: [sdorpl/VizRS](https://github.com/sdorpl/VizRS).

| Repozytorium (katalogi obok, `../`) | Rola | Styk z tą aplikacją |
|---|---|---|
| [VizRS CG System](../../VizRS%20CG%20System) | silnik grafiki (Rust): Trio (playout stron), wyjście `/output` (też anteny kanałów `bus=ch-N`), API TCP 6100 / HTTP / SSE (opis: `docs/API.md`), serwer `vizrs` | protokół VizRS: TCP 6100 (`AUTH`, `STATE`, `TAKE`, `OUT`…), strona `/output`, renderer `vizrs-render stream` (dostawca styku: VizRS CG System, `docs/API.md`) |
| **VizRS CG Playout** (to repo) | grafika VizRS i sloty na żywo (live ingest RTMP/SRT) na kanałach ErsatzTV (sidecar `cg`, bez zmian w ErsatzTV) oraz panel kanału (`panel/`) nad ErsatzTV legacy | – |
| [VizRS CG Artist](../../VizRS%20CG%20Artist) | edytor scen i animacji (Tauri 2) | brak bezpośredniego – sceny z Artist trafiają na serwer `vizrs` (VizRS CG System), a stamtąd na kanał |

Zasady zgodności:

- **Styk zmienia się po obu stronach w tym samym cyklu.** Zmiana API, pól odpowiedzi, parametrów lub zmiennych nakładki `docker-compose.live.yml` po jednej stronie wymaga dostosowania drugiej strony i jej dokumentacji, zanim wejdzie do wydania. Opis styku jest po stronie dostawcy: dla Live API sidecara dostawcą jest ta aplikacja ([API.md §1](API.md#1-live-api-sidecara-apilive-etv-ffmpeg-ingest-health)), dla API mostu – most ([API.md §3](API.md#3-api-mostu-vizrs-bridge)), a dla protokołu VizRS (TCP 6100, `/output`) dostawcą jest VizRS CG System (`docs/API.md`).
- **Starsza wersja sąsiada nie psuje aplikacji ani emisji.** Brak nowego pola lub ścieżki to „niedostępne w tej wersji”, nie błąd. Rozszerzenia API w CG Playout zachowują kompatybilność wsteczną ze starszą wersją panelu; z kolei przy współpracy z serwerem VizRS CG System brak nowych możliwości serwera grafiki nie może powodować błędu krytycznego ani przerywać emisji kanału.
- **Wydania razem.** Przed wydaniem sprawdź z bieżącymi wersjami pozostałych aplikacji pakietu (VizRS CG System i VizRS CG Artist); zmianę wymagającą nowszej wersji sąsiada odnotuj w `CHANGELOG.md` z minimalną wersją.

## 3. Komendy i symbole zastępcze

Jedna komenda na linię (składnia: [API VizRS, TCP](../../VizRS%20CG%20System/docs/API.md));
puste linie i komentarze `#` są pomijane. Przy tokenie sidecar najpierw
wysyła `AUTH <token>`.

| Symbol | Wartość |
|---|---|
| `{title}` | tytuł, tytuł serialu albo nazwa pliku |
| `{show_title}` | tytuł serialu |
| `{source}` | ścieżka pliku |
| `{duration}` | długość klipu `hh:mm:ss` |
| `{next_title}` | tytuł następnego programu |
| `{next_start}` / `{next_start_full}` | start następnego, `hh:mm` / `hh:mm:ss`, w strefie `TZ` (niżej) |
| `{next_start_iso}` | start następnego jako czas UTC co do sekundy (`2026-11-02T19:00:00Z`) – pole `target` zegara VizRS w trybie `target` (odliczanie do następnej audycji, np. `TAKE 1407 target={next_start_iso}`); oba renderery VizRS czytają `Z` i przesunięcie strefy. Transkodowanie wyprzedza antenę – zegar strony przesuwa o to `clock_offset_seconds` (`VIZRS_CLOCK_OFFSET`, §5), inaczej odliczanie na antenie spóźnia się o wyprzedzenie; pusty bez EPG |
| `{next_duration}` | długość następnego `hh:mm:ss` |
| `{next2_title}`, `{next2_start}` / `{next2_start_full}`, `{next2_start_iso}`, `{next2_duration}` | to samo dla programu po następnym (trzeci wpis EPG, `epg_entries: 3`) |

`|` w wartościach zamienia się na `/`, a znaki końca linii są usuwane.
Godziny programów ErsatzTV podaje w UTC (`…Z`, `…+00:00`), niezależnie od własnej
strefy. Takie przelicza się na strefę procesu VizRS CG Playout (`TZ`, w stosie
`Europe/Warsaw`). Godzina z inną strefą (`…+01:00`) zostaje taka, jak w danych.
Pola strony można też podać jako JSON (`TAKE 1406 {"programs":"{next_title}\n{next2_title}"}`,
składnia VizRS): tylko tak wartość ma kilka linii, np. dla rotacji wierszy
(`cycle`) w tekście sceny. W takiej linii (`TAKE`/`UPDATE`/`LIVE`, także z `@N`)
wartości symboli są wstawiane ze znakami JSON (`"` → `\"`), więc tytuł
z cudzysłowem nie psuje komendy.
Gdy kilku widzów dołącza do tej samej pozycji, ErsatzTV uruchamia skrypt
ponownie – komendy nie są wysyłane drugi raz.

Na reklamie sidecar odczytuje `STATE`, zdejmuje wszystkie warstwy na antenie
i wysyła `on_ad`. Pierwszy zwykły klip przywraca zdjęte strony jednym `TAKE`
przed własnymi komendami `on_clip`.

Reklamę można też wskazać w harmonogramie, jak znak wodny: element **VizRS**
dodaj w **Schedules → pozycja → Graphics Elements** przy pozycjach z grafiką,
a przy reklamach go nie zaznaczaj (wtedy nie dodawaj go do **Deco**).
ErsatzTV nie uruchamia wtedy skryptu, więc sidecar nie dostaje sesji. Gdy
żadna sesja kanału nie trwa w chwili końca poprzedniej pozycji
(`MediaItem_Stop`) plus 2 s, sidecar traktuje to jako reklamę: zdejmuje
warstwy na antenie (`STATE` → `OUT`) i wysyła `on_ad` (symbole klipu są wtedy
puste). Pierwsza pozycja z elementem przywraca strony. Wyłącza to
`ad_without_element: false`. Uwaga: zatrzymanie kanału w ErsatzTV (brak
widzów) wygląda tak samo i też zdejmuje grafikę – wraca przy kolejnej sesji.

### Osobne grafiki na kanał (`channels`)

Jeden VizRS ma osobną antenę na kanał: komendy TCP `@N …` i wyjście
`/output?bus=ch-N` (API VizRS). Opcja `channel_buses` (env `VIZRS_CHANNEL_BUSES=1`)
kieruje na nie każdy kanał ErsatzTV bez konfiguracji: nowy kanał z elementem VizRS
od razu ma własną grafikę. Wymaga VizRS z antenami kanałów. Strony i show są
wspólne, a różnią się tym, co kanał bierze na antenę. Operator wybiera antenę
kanału w Trio (lista **Antena**), a główna antena przy tej opcji nie idzie na emisję. Karuzela, rundown, makra i dock działają na wybranej antenie.

Przy `channel_buses` sidecar co `sync_seconds` czyta kanały z API ErsatzTV i
zakłada je z nazwami w VizRS (§4), więc lista **Antena** w Trio pokazuje kanały
ErsatzTV, zanim pójdzie na nich pierwsza grafika.

Obiekt `channels` nadpisuje ustawienia kanału: komendy, `shows`, a także własny
VizRS (kanał z `vizrs.address` nie używa wtedy anteny kanału). Czego nie podasz,
zostaje z ustawień głównych:

```json
"channels": {
  "1": {
    "on_live": "TAKE 7510"
  },
  "2": {
    "on_clip": "TAKE 4001 headline={title}",
    "on_ad": "",
    "shows": [{ "show_title": "^Kino", "on_clip": "TAKE 5001" }],
    "vizrs": { "address": "vizrs2:6100" },
    "output_url": "http://vizrs2:8088/output?token=…"
  }
}
```

- Element `vizrs.yml` bez `channel=` działa na każdym kanale (§4). Tylko gdy dwa
  kanały mogą grać ten sam materiał w jednej chwili, daj kanałowi kopię elementu
  z `http://cg:7700/etv?channel=N`, przypisaną w **Deco**. Element z cudzym
  `channel=` pokaże grafikę tamtego kanału.
- Każde `/output` ma własne przechwytywanie, więc CPU rośnie z liczbą kanałów
  (§1); kanały jednej instancji nadal dzielą przechwytywanie.
- Klient z jawnym `channel=` synchronizuje strony VizRS tego kanału i tylko jego
  elementy (§3). Numer zajęty przez inny kanał dostaje plik `page-<ID>-ch<N>.yml`.
- `shows` podane w kanale zastępuje całą główną listę `shows` dla tego kanału;
  bez niego obowiązuje lista główna. `ad_match` poza `shows` jest wspólne.
- `on_live` (opcjonalne, np. `"on_live": "TAKE 7510"`) – komendy grafiki wysyłane
  w chwili przejęcia kanału przez live ingest (§1, [Grafika podczas przejęcia (`on_live`)](#grafika-podczas-przejęcia-on_live)).
  Pole występuje **wyłącznie per kanał** (nie ma globalnego `on_live`); w trakcie
  przejęcia grafika ramówki zostaje wstrzymana.

### Elementy graficzne per audycja (`shows`)

Tak jak w przypadku watermarków przypisywanych do pozycji w ErsatzTV, sidecar
umożliwia zdefiniowanie odrębnych szablonów komend (`on_clip`, `on_ad`) oraz
reguł wykrywania reklam (`ad_match`) dla konkretnych audycji.
Służy do tego tablica `shows` w pliku konfiguracyjnym:

```json
{
  "on_clip": "TAKE 4001 headline={title}",
  "on_ad": "",
  "shows": [
    {
      "show_title": "^(Wiadomości|Serwis)",
      "on_clip": "TAKE 1001 headline={title}|styl=pilne",
      "on_ad": "TAKE 3001 items=Wracamy po reklamie",
      "ad_match": { "path": "/reklamy_news/", "title": "" }
    },
    {
      "show_title": "^Sport",
      "on_clip": "TAKE 5001"
    }
  ]
}
```

- `show_title` to wyrażenie regularne (case-insensitive) dopasowywane do
  tytułu audycji (`MediaItem_ShowTitle` z danych ErsatzTV, symbol `{show_title}`).
- Pierwsza pasująca reguła z listy `shows` zastępuje domyślne `on_clip`, `on_ad`,
  `ad_match`, `on_end` i/lub `end_seconds` (pola pominięte przyjmują wartości
  globalne z konfiguracji; `"on_end": ""` wyłącza belkę przed końcem tej audycji).
- Jeśli audycja nie pasuje do żadnego wpisu z `shows`, stosowane są ustawienia
  globalne.

### Zdarzenia wtórne (`secondary`, K15) z ważnością i zatwierdzaniem (K16)

Reguła zdarzeń wtórnych wybiera pozycje po tytule audycji albo pozycji i wprowadza
na nich strony VizRS w zaplanowanych chwilach – kilka zdarzeń na pozycję, np. belka
tytułowa 10 s po starcie i „Za chwilę” 30 s przed końcem (`src/secondary.rs`).
Działa niezależnie od `on_clip`/`on_end` i od elementów grafiki w ErsatzTV:

```json
"secondary": [
  {
    "name": "Wiadomości – belki",
    "channel": "1",
    "target": { "show_title": "^Wiadomości" },
    "events": [
      { "page": "7501", "anchor": "start", "offset_s": 10, "duration_s": 8 },
      { "page": "7510", "anchor": "end", "offset_s": 30, "duration_s": 25,
        "values": { "f0": "{next_title}" } }
    ],
    "valid_from": "2026-11-24", "valid_to": "2026-12-31",
    "approved": { "by": "jan", "at": "2026-11-24T10:00:00Z" }
  }
]
```

- `target.show_title` / `target.title` / `target.path` – wyrażenia regularne (bez
  rozróżniania wielkości liter) dla `{show_title}`, `{title}` i ścieżki pliku pozycji
  (`{source}`, np. folder kolekcji `/media/seriale/`); `target.start` – jedna pozycja
  ramówki: jej start (ISO, ±2 s, wymaga `channel`). Podane kilka – muszą pasować
  wszystkie. `target.collection` i `target.item_id` są odrzucane (400 przy zapisie,
  błąd przy starcie): ErsatzTV nie podaje sidecarowi kolekcji ani pozycji playoutu
  klipu – zamiast nich `path` i `start`. `channel` – tylko ten kanał (bez niego – każdy).
- Zdarzenie: `page` (numer albo nazwa strony), `anchor` – `start` (od startu
  pozycji) albo `end` (przed końcem – ten sam koniec co `on_end`, `end_ref`),
  `offset_s` (0–86400), `duration_s` (> 0) – `OUT` strony po tym czasie, najpóźniej
  na końcu pozycji; `values` – pola strony (linia `TAKE <strona> {json}`, symbole
  `{title}`, `{next_title}`… jak w komendach). Do 200 reguł, do 20 zdarzeń w regule.
- Każde zdarzenie idzie raz na pozycję, tak jak `on_clip` po czasie klatek
  (`lead_seconds`, oś pozycji). Pozycja krótsza niż zdarzenie – zdarzenie przepada;
  reklama – bez zdarzeń; przerwa w środku zdejmuje strony jak inne.
- **Nakładanie:** dwa zdarzenia reguły z tą samą stroną od tego samego punktu
  w zachodzących na siebie czasach = błąd (drugi `TAKE` uciąłby pierwszy). Warstw
  stron sidecar nie zna (zna je VizRS), więc różne strony na jednej warstwie
  sprawdza operator.
- **Ważność i zatwierdzanie (K16):** `valid_from` / `valid_to` (RRRR-MM-DD, data
  lokalna startu pozycji, strefa `TZ` sidecara; obie włącznie) i `approved` –
  w regule i w pojedynczym zdarzeniu. Bez klucza `approved` wpis nie wymaga
  zatwierdzenia; `"approved": null` – czeka (nie idzie na antenę);
  `{ "by", "at" }` – zatwierdzony. Wpis poza ważnością albo niezatwierdzony sidecar
  pomija bez ostrzeżenia.
- Reguły są częścią reguł grafiki: `config.json` albo – po pierwszym zapisie
  z panelu – `graphics.json` (`GET/PUT /api/live/graphics`, [API.md §1](API.md)).
  W `/api/live/state` cechy `events` i `approval`.

### Osobne elementy grafiki w ErsatzTV (`page=<ID>` / `take=<ID>`) i synchronizacja (`vizrs-cg sync`)

Drugim sposobem – analogicznym do watermarków w ErsatzTV – jest dodanie
poszczególnych grafik VizRS jako osobnych elementów graficznych **script**
w ErsatzTV (pliki `.yml` w katalogu `etv/templates/graphics-elements/script/`).
Każdy taki plik odpowiada stronie z VizRS CG System:
- format nazwy elementu: `[Nazwa strony] (ID)`, np. `Ticker (3001)`
- adres URL z parametrem strony: `http://cg:7700/etv?channel=1&page=3001` (lub opcjonalnie `take=...` z wartościami pól `fields=...`).

Elementy stron powstają same. Każda sesja elementu VizRS (także samego
**VizRS** z `vizrs.yml`) po cichu, w tle, wyrównuje katalog
`/config/templates/graphics-elements/script/` (`ETV_CONFIG_FOLDER`, gdy ErsatzTV
ma inny) ze stronami bieżącego show. Robi to `vizrs-etv-client`, bo tylko on
działa w kontenerze ErsatzTV i zna kanał elementu: listę bierze z `GET
/etv/pages` sidecara, a sidecar z `GET /api/show` VizRS (adres z `output_url`,
token z `vizrs.token`; bez `output_url` synchronizacji nie ma). Błąd trafia
tylko do logu i nie wpływa na grafikę. ErsatzTV odświeża listę elementów przy
starcie każdej sesji i co godzinę, więc nowa strona pojawia się w **Schedules
→ pozycja → Graphics Elements** najpóźniej po zmianie pozycji i kolejnym
odświeżeniu; wymaga to, by na kanale grał jakikolwiek element VizRS. Na
świeżym stosie elementy stron pojawiają się po pierwszej emisji kanału
(obraz `seed` ich nie zawiera, bo wzorcowy show nie musi być tym działającym).

| Zmiana w VizRS | Element w ErsatzTV |
|---|---|
| nowa strona | nowy plik `page-<ID>.yml`: URL sesji, która go utworzyła (kanał, `render`, `area`) plus `page=<ID>` |
| zmieniona nazwa | zmienia się tylko `name:`, plik i przypisania zostają |
| czas strony na kanale (Trio, `playout` strony) | w URL zmieniają się tylko `at=`, `out=`, `ref=` (§3, Grafika przed końcem audycji) |
| strona znika (usunięta, inny show) | zostaje z nazwą `[brak w VizRS] Ticker (3001)`; `TAKE` da `ERR` w logu; znacznik znika, gdy strona wróci |

**Pliki nie są kasowane**, bo ErsatzTV usuwa element, którego plik brakuje przy
odświeżeniu, a z nim – kaskadowo i bezpowrotnie – przypisania w harmonogramach,
Deco i pozycjach playoutu. Z tego samego powodu pliki podmieniane są atomowo
(`rename`), bez chwili, w której pliku nie ma. Nazwa pliku nie ma znaczenia:
stroną elementu jest parametr `page=` (lub `take=`) w jego URL. Plik bez
pierwszej linii `# Generated from VizRS CG System page …` jest ręczny:
synchronizacja go nie zmienia, a jego strona nie dostaje drugiego elementu.

Automat zostawia URL istniejącego pliku (poza czasem strony: `at`, `out`,
`ref`), więc element raz utworzony trzyma swój kanał. Elementy są wspólne dla całego ErsatzTV: przy kilku kanałach na jednym
sidecarze `page-<ID>.yml` należy do kanału, który utworzył go pierwszy, a dla
pozostałych skopiuj plik z innym `channel=` (bez pierwszej linii). Elementy
utworzone przez klienta bez `channel=` są go pozbawione i działają na każdym kanale.

Ręcznie, np. żeby przenieść elementy na inny kanał, służy polecenie:

```sh
vizrs-cg sync [--url http://vizrs:8088] [--token <TOKEN>] [--channel 1] [--cg-url http://cg:7700] [--output-dir <DIR>] [--prune]
# albo z pliku projektu show.json:
vizrs-cg sync --file seed/vizrs-data/show.json --output-dir <DIR>
```

Działa tak samo, ale ustawia też URL plików generowanych (`--channel`,
`--cg-url`). `--prune` usuwa pliki stron, których nie ma w VizRS, **razem z ich
przypisaniami w ErsatzTV**; przy pustej liście stron nie usuwa niczego.

Na jednej pozycji można zaznaczyć kilka elementów VizRS (np. **VizRS**
i **Ticker wipe z kategoriami (3003)**). Klatki `/output` niesie wtedy tylko
jedna sesja danego kawałka pozycji (kanał, `MediaItem_Start`,
`MediaItem_StreamSeek`), pozostałe wysyłają przezroczyste – inaczej ErsatzTV
albo nakładka ffmpeg rysowałyby tę samą grafikę kilka razy. Komendy każdy
element wysyła swoje.

### Wejście i zejście grafiki (`in_seconds`, `out_seconds`)

Grafika każdego elementu VizRS (ogólny **VizRS** z `on_clip` i elementy stron
`page=`/`take=`/`cmd=`) jest wiązana z czasem pozycji w harmonogramie
(`MediaItem_Start`, `MediaItem_Stop`):

- **IN**: komendy idą `in_seconds` (domyślnie 5 s) po starcie pozycji; widz
  dołączający później wywołuje je od razu,
- **OUT**: `OUT` stron z linii `TAKE` tych komend (np. `TAKE 4001 headline=…`
  → `OUT 4001`) idzie `out_seconds` (domyślnie 5 s) przed jej końcem.
  Komendy bez `TAKE` (np. samo `UPDATE`) nie mają `OUT`.

Czasy liczą się w osi wideo pozycji, nie według zegara. ErsatzTV transkoduje
pozycję z wyprzedzeniem (na s2 ok. 30 s przed startem w harmonogramie), a jej
klatki pokazują grafikę z chwili, w której powstają (`lead_seconds` wstecz,
§1). Sidecar wysyła więc `TAKE` wtedy, gdy transkodowanie dochodzi do
`in_seconds` pozycji, a `OUT` – gdy dochodzi do `out_seconds` przed jej końcem.
Animacje wejścia i zejścia trafiają w te miejsca wideo, zamiast wypaść poza
nie (grafika ucięta na końcu pozycji i wskakująca na ostro na początku
następnej). Oś bierze się z sesji pozycji: czas emisji
(`MediaItem_Start` + `MediaItem_StreamSeek`), gdy sesja przyszła najwyżej
`lead_seconds` + 2 s przed nim, inaczej chwila startu sesji – tak jak tempo
strumienia klatek. ErsatzTV transkoduje pozycję kawałkami (na s2 po ok. 44 s,
każdy to osobna sesja), a wyprzedzenie zmienia się między nimi: na starcie
pozycji do ok. 30 s, później transkodowanie dogania emisję. Zaplanowany, a
jeszcze niewysłany `TAKE` lub `OUT` idzie więc według osi **najnowszej** sesji
pozycji, czyli kawałka, który robi klatki tej chwili. Z osią tylko pierwszej
sesji (do 2.7.1) belka przed końcem długiej pozycji mijała się z obrazem
o kilkadziesiąt sekund.

Kolejne sesje tej samej pozycji (ErsatzTV transkoduje ją kawałkami, widzowie
dołączają) nie wysyłają komend ponownie; kilka elementów na jednej pozycji
planuje się niezależnie. Pozycja krótsza niż `in_seconds` + `out_seconds`
albo już w ostatnich `out_seconds` nie dostaje grafiki wcale. Bez czasów
pozycji w danych ErsatzTV komendy idą od razu, bez `OUT`. Reklama (`ad_match`,
pozycja bez elementu) dostaje `on_ad` od razu, a grafika zaplanowana przed nią
nie jest już wysyłana. `out_seconds` ujemne = bez `OUT` (grafika zostaje do
reklamy); razem z `in_seconds: 0` to dawne zachowanie (komendy od razu, bez
zejścia).

### Grafika przed końcem audycji (`on_end`, `at=-N`)

Belka typu „Za chwilę: *następna audycja* o *godzina*” wchodzi określoną liczbę
sekund **przed końcem** audycji i schodzi tuż przed nim. Czas liczy się tak
jak `in_seconds`/`out_seconds` – w osi wideo pozycji, z poprawką na transkodowanie
z wyprzedzeniem.

**Dla każdej audycji (`on_end`).** Elementy ogólne **VizRS** (bez `page=`)
wysyłają w każdej zwykłej pozycji, niezależnie od `on_clip`:

```json
"on_end": "TAKE 1405 next=Za chwilę|programs={next_title}|times={next_start}",
"end_seconds": 60,
"end_out_seconds": 5,
"end_ref": "programme",
"shows": [
  { "show_title": "^Wiadomości", "end_seconds": 30 },
  { "show_title": "^Zapowiedź", "on_end": "" }
]
```

- `on_end` – komendy (symbole jak w `on_clip`), puste = brak; nadpisywane w
  `shows` i `channels` (tam też `end_seconds`);
- `end_seconds` (60) – `TAKE` tyle sekund przed końcem;
- `end_out_seconds` (5) – `OUT` stron z linii `TAKE` tyle sekund przed końcem;
- `end_ref` – od którego końca: `programme` (domyślnie) – koniec wpisu EPG, w
  którym zaczyna się pozycja (`Epg[].Stop`), czyli całej audycji, także gdy
  przerwy dzielą ją na kilka pozycji; bez EPG – koniec pozycji; `item` – koniec
  pozycji (`MediaItem_Stop`).

**Dla wybranych pozycji (element strony).** Adres elementu strony przyjmuje czas:

| Parametr | Znaczenie |
|---|---|
| `at=10` / `at=-60` | `TAKE` 10 s po starcie pozycji / 60 s przed końcem (domyślnie `in_seconds` po starcie) |
| `out=5` | `OUT` 5 s przed końcem (domyślnie `out_seconds`, a przy `at=-N` – `end_out_seconds`); `out=-1` – bez `OUT` (nie dla `at=-N`) |
| `ref=programme` / `ref=item` | koniec audycji z EPG albo pozycji (domyślnie: `item`, a przy `at=-N` – `end_ref`) |

np. `http://cg:7700/etv?channel=1&page=1405&at=-60&out=5`. Element zaznacza się
w ErsatzTV tylko przy wybranych pozycjach. Czas można też ustawić w **Trio**
(edytor strony → **⏱ kanał**): zapisuje się w stronie (`playout`), a
synchronizacja elementów (§3) przepisuje go do URL pliku `page-<ID>.yml`.
Błędne wartości trafiają do logu jako ostrzeżenie i są pomijane.

Zasady:

- moment `TAKE` przed końcem musi wypaść w tej pozycji: w audycji pociętej
  przerwami belkę dostaje tylko część, w której wypada (inne nic nie wysyłają),
  a pozycja krótsza niż `end_seconds` (bez EPG) – wcale;
- grafika przed końcem **zawsze ma `OUT`**, najpóźniej `out` przed końcem
  pozycji – inaczej przerwa zdjęłaby ją przez `STATE`, a pierwsza pozycja
  następnej audycji przywróciłaby ją;
- widz dołączający między `TAKE` a `OUT` wywołuje `TAKE` od razu, później nic;
- reklama po zaplanowaniu anuluje plan tak jak przy `on_clip`;
- belka na tej samej warstwie co strona z `on_clip` ją zastępuje (późniejsze
  `OUT` tamtej strony daje tylko `ERR` w logu); osobna warstwa – obie naraz.

Zapowiedź w dwóch „slajdach” („Za chwilę: *następny*”, potem „Następnie:
*kolejny*”) daje scena z dwoma tekstami z `cycle` (rotacja co N s,
zsynchronizowana w scenie) i pola po dwie linie, np. strona 1406
„Za chwilę – belka” na kanale 5:

```json
"channels": { "5": { "on_end": "TAKE 1406 {\"heads\":\"Za chwilę\\nNastępnie\",\"programs\":\"{next_title}\\n{next2_title}\",\"times\":\"{next_start}\\n{next2_start}\"}" } }
```

Bez trzeciego wpisu EPG `{next2_*}` są puste, a VizRS pomija pustą linię:
program stoi, a nagłówki dalej się zmieniają.

Czas na ekranie = moment komendy + animacja wejścia sceny (Artist: `delay`,
`dur` presetu IN). Animacja wyjścia powinna być krótsza niż `end_out_seconds`.

## 4. Uruchamianie w Dockerze

```sh
docker compose up -d
```

uruchamia VizRS (`:8788`), sidecar (`:7700`, `/health`) i ErsatzTV (`:8409`,
media testowe z wolumenu `seed-media` albo `MEDIA_DIR`). Za reverse proxy
na hoście ustaw `ETV_PORT=127.0.0.1:8409`: ErsatzTV przestaje być publiczny, ale
tracą go też kontenery spoza stosu łączące się przez adres hosta Dockera
(np. Restreamer). Stos nie montuje
żadnych plików z hosta, więc działa tak samo z klonu repozytorium i w
Portainerze wdrażającym stos z GitHuba (**Stacks → Add stack → Repository**).
Usługa `seed` (niżej) kopiuje
[vizrs.yml](../etv/templates/graphics-elements/script/vizrs.yml) do
`/config/templates/graphics-elements/script/` ErsatzTV, a programy
`vizrs-etv-client` i `vizrs-ffmpeg` do wolumenu `vizrs-client`, montowanego
w ErsatzTV jako `/vizrs`.
W ErsatzTV dodaj element **VizRS** do pozycji harmonogramu
(**Schedules → pozycja → Graphics Elements**) albo do **Deco** przypisanego
do playoutu i zresetuj playout – bez tego ErsatzTV nie uruchamia skryptu.
Numeru kanału nie trzeba ustawiać: `vizrs-etv-client` bez `channel=` w adresie
bierze go z ErsatzTV. Skrypt nie dostaje numeru, ale ErsatzTV uruchamia go, gdy
ffmpeg otwiera płótno `/internal/graphics/<kanał>/<pozycja>`, więc klient czyta
numer z takiego procesu w `/proc` kontenera: z tym samym plikiem wejściowym co
`MediaItem_Path` pozycji, a bez takiego z najmłodszego. Ten sam element działa
więc na wielu kanałach. Pomylić się może przy tym samym materiale na dwóch
kanałach w jednej chwili albo materiale podanym adresem innym niż ta ścieżka;
adres z `channel=` zawsze wygrywa (§3, `channels`).
Aktualizacja stosu zmienia resztę `vizrs.yml`, ale zachowuje jego adres (także
`render`, `area`), a istniejące `channel=` zostaje, dopóki go nie usuniesz. Elementy
poszczególnych stron powstają same (§3).

W `/vizrs` jest też nakładka ffmpeg `vizrs-ffmpeg` (§1), zbudowana w obrazie
`seed` dla architektury stosu; compose ustawia ErsatzTV
`VIZRS_CG_URL=http://cg:7700`. Baza startowa (`seed/`) ma już ścieżkę FFmpeg
`/vizrs/vizrs-ffmpeg`. W istniejącej instancji ustaw ją w **Settings → FFmpeg
→ FFmpeg Path**. Powrót do ścieżki
grafiki ErsatzTV: `/usr/local/bin/ffmpeg` albo `VIZRS_CG_URL=` (puste).

Programy w `/vizrs` to statyczne binarki zbudowane dla architektury stosu:
obraz ErsatzTV nie potrzebuje Pythona ani dodatkowych pakietów.

**Aktualizacja z wersji sprzed 2.1.3 (klienci w Pythonie):** od wersji 2.1.7
`seed` nie tworzy już w `/vizrs` plików `vizrs-ffmpeg.py`
i `vizrs-etv-client.py`. Przed aktualizacją ustaw **FFmpeg Path** na
`/vizrs/vizrs-ffmpeg` – ścieżka `.py` po aktualizacji nie istnieje i ErsatzTV
przestałby transkodować kanał. W kopiach elementu ze starego `vizrs.yml`
(`command: python3`, `/vizrs/vizrs-etv-client.py`) zmień `command` na
`/vizrs/vizrs-etv-client`, a z `args` usuń ścieżkę `.py`; bez tego pozycja
leci bez grafiki.

Przed otwarciem portów zmień `VIZRS_TOKEN` i `VIZRS_OUTPUT_TOKEN`.
Ustawienia stałe dla danej maszyny (nazwa projektu, nakładki, porty, tokeny)
trzymaj w pliku `.env` obok compose (poza gitem), np.
`COMPOSE_PROJECT_NAME=virtrs`, `COMPOSE_FILE=docker-compose.yml:docker-compose.nvidia.yml`
(albo `…:docker-compose.vaapi.yml`),
`VIZRS_PORT=8789` – wtedy samo `docker compose pull && docker compose up -d --build`
odtwarza ten sam stos.
Chromium wymaga `shm_size: 512mb`. Obraz buduje się na amd64 i arm64.

Strefa czasowa jest wymuszona: każda usługa stosu (`seed`, `vizrs`, `cg`,
`ersatztv`) dostaje `TZ=Europe/Warsaw` z kotwicy `x-timezone` w compose, bez
nadpisywania z `.env` ani zmiennych hosta. Dotyczy to zegarów w grafice
(JavaScript w Chromium), VizRS, harmonogramu i EPG ErsatzTV oraz logów.
Obraz sidecara ma `tzdata` i sam ustawia `TZ=Europe/Warsaw`, więc ta sama
strefa obowiązuje też poza tym compose (np. `docker run`). Inną strefę daje
tylko zmiana kotwicy w compose.

### Live ingest w Dockerze

Odbiorniki (§1, Live ingest) to usługa `ingest` w profilu `ingest`: obraz
ErsatzTV (jego ffmpeg, bez dodatkowego pobierania) z programem
`/vizrs/vizrs-ffmpeg vizrs-ingest` z wolumenu `vizrs-client`. W `.env`:

```sh
VIZRS_INGEST_TOKEN=<długi losowy tekst>              # sidecar i odbiorniki
VIZRS_INGEST=1=rtmp://0.0.0.0:1936/live/<klucz>      # kilka: "1=rtmp://…; 2=srt://0.0.0.0:9000?passphrase=…; 3=https://www.youtube.com/@kanal/live"
# VIZRS_YTDLP_COOKIES=/etv-config/moje-cookies.txt  – własny plik cookies dla yt-dlp zamiast wgranego w VizRS Live (Ciasteczka YouTube)
# VIZRS_YTDLP_KEEPALIVE_MIN=30  – co ile minut podtrzymać sesję ciasteczek z VizRS Live (0 = nigdy)
# INGEST_RTMP_PORT=1936, INGEST_SRT_PORT=9000 (UDP) – porty na hoście
```

potem `docker compose --profile ingest up -d`. Nadawca (OBS: **Ustawienia →
Strumień → Własny**): serwer `rtmp://<serwer>:1936/live`, klucz `<klucz>`;
GOP 1–2 s. ErsatzTV ma w compose `VIZRS_FFMPEG_INGEST=1`, a ścieżką FFmpeg
musi być nakładka (`/vizrs/vizrs-ffmpeg`, wyżej). Bez tokenu sidecar ma ingest
wyłączony, a odbiorniki nie startują (`VIZRS_INGEST_TOKEN is empty` w logu).
Pierwszy start po aktualizacji może zastać w wolumenie starą nakładkę bez
trybu `vizrs-ingest` – usługa restartuje się, aż `cg` ją podmieni (§4,
Synchronizacja z ErsatzTV). Gdy `cg` nowego wydania podmieni nakładkę w
wolumenie, usługa kończy się sama w ciągu ok. 5 s (log: `a new vizrs-ffmpeg in
the volume … restarting`) i kontener startuje z nową wersją – trwający sygnał
na chwilę się urywa i nadawca łączy się ponownie; tak samo przy pierwszym wejściu YouTube może
brakować `/vizrs/yt-dlp` (log: `/vizrs/yt-dlp: No such file…`, ponawiane co
30 s). Odbiornik tylko przepakowuje strumień (ułamek
rdzenia); CPU kosztuje przebieg live w ErsatzTV: dekodowanie sygnału
w miejsce dekodowania materiału.

Live API dla panelu ([API.md §1](API.md#1-live-api-sidecara-apilive-etv-ffmpeg-ingest-health)): w `.env` `VIZRS_PANEL_TOKEN=<długi losowy tekst>`,
ten sam token w panelu (`VIZRS_LIVE_CG_TOKEN`). Audycje na żywo w ramówce ([API.md §1](API.md#1-live-api-sidecara-apilive-etv-ffmpeg-ingest-health))
zakłada się w panelu (Audycje LIVE); `VIZRS_LIVE_SHOWS`, np.
`VIZRS_LIVE_SHOWS=[{"channel":"1","title":"Wiadomości","duration":"00:30:00"}]`,
tylko pierwszy raz wypełnia ich listę
(sidecar pisze je do wolumenu konfiguracji ErsatzTV, `/config/vizrs-live`),
opcjonalnie `VIZRS_INGEST_ANYTIME=0` i `VIZRS_FFMPEG_STANDBY=/media/plansza.png`
(plik w mediach ErsatzTV). Bibliotekę **Remote Streams** z folderem
`/config/vizrs-live` zakłada się raz w ErsatzTV.

### Serwer z widzami: `userland-proxy` i pamięć potoków

Docker domyślnie przekazuje każde połączenie na opublikowany port przez
`docker-proxy`, a ten trzyma dwa potoki po 1 MiB na połączenie – jako root, tak
jak procesy w kontenerach (ffmpeg, ErsatzTV). Wszystkie te potoki dzielą jeden
limit `fs.pipe-user-pages-soft` (domyślnie 16384 strony = 64 MiB); po jego
przekroczeniu nowe potoki w kontenerach mają tylko 8 KiB, a kanał z grafiką
1600×900 przestaje nadążać za czasem rzeczywistym (§1, nakładka ffmpeg, pkt 4).
Nakładka podaje grafikę przez gniazdo, ale potoki ErsatzTV zostają, więc na
serwerze z widzami (s1: ok. 45 połączeń = 90 MiB potoków `docker-proxy`)
wyłącza się `userland-proxy` – porty obsługuje wtedy sam iptables (NAT), bez
procesów i potoków:

```bash
# /etc/docker/daemon.json (scalić z istniejącymi kluczami)
{ "userland-proxy": false }
sudo systemctl restart docker        # restart wszystkich kontenerów: krótka przerwa w emisji
```

Bez restartu Dockera wystarczy podnieść limit (od razu, dla nowych potoków):
`echo 'fs.pipe-user-pages-soft = 262144' | sudo tee /etc/sysctl.d/90-vizrs-pipes.conf && sudo sysctl --system`.
Stan: suma rozmiarów potoków (`F_GETPIPE_SZ`) procesów `docker-proxy` i liczba
potoków 8 KiB procesów `ffmpeg`.

### Synchronizacja z ErsatzTV

Sidecar sam utrzymuje integrację, więc jego wersja zawsze zgadza się z klientami:

- na starcie kopiuje `vizrs-etv-client` i `vizrs-ffmpeg` z obrazu do wolumenu
  (`VIZRS_CLIENT_SOURCE`, `VIZRS_CLIENT_DIR`) podmianą pliku, bez przerywania
  działającego ffmpeg, i aktualizuje `vizrs.yml`, zachowując jego adres;
- co `VIZRS_SYNC_SECONDS` (domyślnie 60 s) przez API ErsatzTV (`VIZRS_ETV_URL`,
  klucz z `api-secrets.json` w `VIZRS_ETV_CONFIG`) zakłada anteny i nazwy kanałów,
  a ze stron VizRS robi elementy `page-<ID>.yml`.

Puste `VIZRS_ETV_CONFIG` wyłącza synchronizację, a wtedy elementy stron robi jak
dotąd klient (§3). `seed` wypełnia tylko puste wolumeny i przy każdym starcie
stosu daje użytkownikowi 10001 zapis do klientów, elementów i audycji na żywo (`vizrs-live`, §1) oraz odczyt klucza
API. Błędy trafiają tylko do logu.

### Stosy s1 i s2

s1 i s2 mają ten sam stos z repozytorium: w Portainerze **Repository → Compose
path** `docker-compose.yml`, **Additional paths** `docker-compose.ampere.yml` i
`docker-compose.live.yml` (po złożeniu nic się nie buduje – `ampere` zeruje
`build`, więc działa też przez agenta Portainera na s2) oraz
`COMPOSE_PROFILES=grafika,ingest`. Ustawienia są wspólne – domyślne dla Ampere: preset
`ultrafast`, `cg` 6 GB, bez płynnych przejść, synchronizacja w RAM, 25 kl./s,
renderer `auto`; obie stacje mają też `VIZRS_CHANNEL_BUSES=1`. Stos ustawia tylko
to, co się różni:

| Zmienna | s1 (`vizplay`) | s2 (`vizrtplay-s2`) |
|---|---|---|
| `STACK_PREFIX` (nazwy woluminów i sieci; domyślnie nazwa projektu) | – | `repo` |
| `ERSATZTV_CONFIG` (konfiguracja ErsatzTV: wolumin albo ścieżka na hoście) | – (wolumin) | `/etc/ersatz` |
| `MEDIA_DIR` / `MEDIA_MOUNT` (media na hoście / w ErsatzTV i panelu) | `/media` / – | `/media/tv` / `/media/tv` |
| `VIZRS_INGEST`, `VIZRS_INGEST_PUBLIC`, `INGEST_RTMP_PORT` | kanał 1, `s1.btb.j00r.us`, 1936 | kanał 4, `s2.btb.j00r.us`, 1944 (1936 zajmuje Restreamer) |
| `VIZRS_LIVE_STREAM_URL`, `VIZRS_LIVE_VIZRS_URL` | `emisja.`, `s1.` | `emisja2.`, `s2.` |

oraz tokeny (`VIZRS_PANEL_TOKEN`, `VIZRS_BRIDGE_TOKEN`, `VIZRS_LIVE_TOKEN`,
`VIZRS_INGEST_TOKEN`). `STACK_PREFIX`, `ERSATZTV_CONFIG` i `MEDIA_MOUNT`
pozwalają stosowi użyć danych starszej instalacji bez ich przenoszenia.
Dawne `docker-compose.s2-stack.yml` (kopia, która rozjechała się ze źródłami:
brak strojenia .NET, ingestu, `VIZRS_FFMPEG_SYNC_RAM`) i `docker-compose.s2.yml`
zastąpił ten układ 2026-11-13.

### Automatyczna aktualizacja po wydaniu

Stosy s1 i s2 używają obrazów z wydań: `vizrs-cg`, `vizrs` i `vizrs-seed`.
Watchtower ich nie dotyka, bo odtworzenie jednego kontenera pomija `seed` i
zależności. Aktualizuje Portainer: workflow `stack-refresh.yml` co 15 minut
porównuje najnowsze wydania obu repozytoriów z plikiem `.release` i przy zmianie
zapisuje go na `main`. Stos w Portainerze ma włączone **GitOps updates** z
**Re-pull image**, więc po tym commicie pobiera obrazy, a nowy `cg` przynosi
programy ErsatzTV.
Odtworzenie `cg` zamraża grafikę do końca pozycji; naprawą jest restart ErsatzTV.

### Panel kanału zamiast web GUI ErsatzTV (`docker-compose.live.yml`)

Nakładka [docker-compose.live.yml](../docker-compose.live.yml) wpina w stos panel
**VizRS CG Playout** (katalog `panel/`, dawny VizRS Live), w którym operator
pracuje zamiast w web GUI ErsatzTV. Nie montuje plików z hosta i niczego nie
buduje, więc działa także w Portainerze: **Additional paths** →
`docker-compose.live.yml` (na Ampere po `docker-compose.ampere.yml`).

- `ersatztv` dostaje obraz `ghcr.io/sdorpl/vizrs-live-legacy` – oficjalne legacy
  z mostem VizRS Bridge (komendy legacy dla panelu, tylko sieć Dockera); wolumeny
  te same, port 8409 nie jest już wystawiony wprost;
- `proxy` (Caddy) wystawia `:8409` tylko dla widzów: `/iptv/*`, `/artwork/*`,
  HDHomeRun; web GUI i API legacy oraz `/vizrs-bridge/*` odpowiadają 403;
- `live` – panel na `:8090` (`VIZRS_LIVE_PORT`), z sidecarem przez `http://cg:7700`
  (gdy włączony jest profil `grafika`; bez profilu `grafika` panel działa nad samym ErsatzTV,
  a `cg` ma `required: false`).

Wymagane zmienne (w Portainerze **Environment variables**; min. 16 znaków):
`COMPOSE_PROFILES=grafika` (lub `grafika,ingest` dla live ingest; wymagane przed aktualizacją!),
`VIZRS_BRIDGE_TOKEN` (most), opcjonalnie `VIZRS_LIVE_TOKEN` (token awaryjny panelu i okna AppImage; operatorzy logują się kontami zakładanymi w panelu, konta na wolumenie `live-data`),
`VIZRS_PANEL_TOKEN` (Live API sidecara, [API.md §1](API.md#1-live-api-sidecara-apilive-etv-ffmpeg-ingest-health)); opcjonalnie
`VIZRS_LIVE_STREAM_URL=http://<serwer>:8409`. Panel (`LIVE_IMAGE`), ErsatzTV z mostem
(`LIVE_LEGACY_IMAGE`) idą z `:latest`, jak sidecar (`vizrs-cg`) – decyzja użytkownika 2026-11-16; wydanie produktu przesuwa `:latest` wszystkich obrazów naraz, a stos pobiera je przy każdym przeładowaniu stosu (Portainer „Re-pull image”, np. po commicie `.release` ze `stack-refresh`); konkretną wersję daje zmienna stosu `LIVE_IMAGE`/`LIVE_LEGACY_IMAGE`; proxy to zawsze najnowsze `caddy:alpine`.
Caddyfile jest wpisany bezpośrednio w sekcję `configs:` nakładki `docker-compose.live.yml`
(Portainer ze stosu GitHuba nie montuje plików z repozytorium). Zgodność kopii z `panel/docker/proxy.Caddyfile`
sprawdza `panel/scripts/check-caddy.sh` w CI (`panel-ci.yml`) i `panel/tests/deploy.sh`.
Do lokalnej budowy obrazów panelu służy nakładka `panel/compose.build.yml` (używana przez `panel/tests/deploy.sh`; Portainer jej nie używa).
Cofnięcie: usunięcie nakładki ze stosu i redeploy (ErsatzTV wraca na oficjalny obraz z tą samą bazą).

### Dane startowe (`seed/`)

Pierwszy start stosu dostaje gotową konfigurację instancji wzorcowej. Usługa
`seed` to mały obraz budowany z repozytorium ([seed/Dockerfile](../seed/Dockerfile):
`seed/`, `vizrs.yml` oraz `vizrs-etv-client` i `vizrs-ffmpeg` kompilowane
w etapie budowy – na x86 krzyżowo dla arm64, na A1 natywnie); przy każdym
starcie stosu kopiuje dane startowe do **pustych** wolumenów i kończy pracę, a VizRS i ErsatzTV startują
po niej. Wolumen z danymi zostaje nietknięty (w logu `seed: … present, left
alone`), więc `docker compose up -d` na działającej instancji niczego nie
nadpisuje. Programy ErsatzTV i `vizrs.yml` `seed` podmienia zawsze – nowa
wersja repozytorium przynosi je po `docker compose up -d --build` (w
Portainerze: **Pull and redeploy**). Pusty katalog `vizrs.yml`, który
zostawiało w wolumenie starsze compose (montowało plik z hosta), `seed`
zastępuje plikiem.

| Katalog | Zawartość | Trafia do |
|---|---|---|
| `seed/vizrs-data/` | show „Newsroom INFO24” (`shows/newsroom-info24.json`, aktywny), `show.json`, tła z `assets/` | wolumen `vizrs-data` (`/data` VizRS) |
| `seed/ersatztv-config/ersatztv.sqlite3` | baza ErsatzTV: kanał 1 z elementem **VizRS**, harmonogram, biblioteka `/media`, profil FFmpeg 1080p x264, ścieżka FFmpeg = nakładka (§1) | wolumen `ersatztv-config` (`/config`) |
| `seed/media/` | odcinki testowe i reklama (`reklamy/` – bez grafiki) | wolumen `seed-media` (`/media` ErsatzTV, gdy brak `MEDIA_DIR`) |
| `vizrs-etv-client`, `vizrs-ffmpeg`, `etv/…/vizrs.yml` | klient elementu i nakładka ffmpeg (binarki z etapu budowy) oraz element grafiki „VizRS” (zawsze nadpisywane) | wolumen `vizrs-client` (`/vizrs`), `/config/templates/graphics-elements/script/` |

Bez sekretów: nie ma tu plików `*-secrets.json`, kluczy `data-protection`,
logów, cache ani indeksu wyszukiwania (ErsatzTV buduje go przy pierwszym
starcie – ostrzeżenie `Search index failed to initialize` jest wtedy
normalne). Profil FFmpeg koduje programowo (Hardware Acceleration: None),
żeby kanał ruszył na każdej maszynie; z kartą NVIDIA przełącz go na Nvidia,
z Intel albo AMD na Vaapi (niżej). Tokeny VizRS ustawiasz jak zwykle w `.env`.

Odświeżenie danych startowych z działającej instancji: zatrzymaj ErsatzTV
(`docker compose stop ersatztv`, baza bez otwartego WAL), skopiuj z wolumenów
`show.json`, `shows/`, `assets/` i `ersatztv.sqlite3` do `seed/`, w kopii
bazy ustaw `FFmpegProfile.HardwareAcceleration = 0` i `ffmpeg.ffmpeg_path =
/vizrs/vizrs-ffmpeg` w `ConfigElement`, usuń wpis
`search_index.version` z `ConfigElement`, potem `docker compose start ersatztv`.
Własne media trzymaj w `media/` (poza gitem) i wskaż je przez
`MEDIA_DIR=./media` (z klonu repozytorium) albo ścieżką bezwzględną na
hoście, np. `MEDIA_DIR=/srv/media` (Portainer: ścieżka względna wskazywałaby
katalog wewnątrz Portainera); ścieżki w ErsatzTV zaczynają się od `/media`.

### Wersje ErsatzTV, Rusta i Node.js

Zasady:

- **ErsatzTV** – zawsze najnowsza wersja z
  [github.com/ErsatzTV/legacy](https://github.com/ErsatzTV/legacy.git);
  compose używa obrazu `ghcr.io/ersatztv/legacy:latest` (amd64 i arm64).
- **Rust** – stabilny; obraz buduje się na `rust:1.97-bookworm`, minimum
  1.87 (`rust-version` w `Cargo.toml`). Zależności runtime: `png` i `regex`,
  przypięte w `Cargo.lock`.
- **Node.js** – tylko do testów kontraktu i benchmarku (§7, §8), bez
  zależności z npm; zawsze najnowsza wersja z
  [github.com/nodejs/node](https://github.com/nodejs/node.git).

Integrację, testy i obrazy sprawdzamy na tych wersjach; zgłoszenia na
starszych najpierw powtarzamy po aktualizacji.

### Oracle Cloud Ampere A1

Na maszynie Ampere A1 (arm64, bez GPU) z nakładką:

```sh
docker compose -f docker-compose.yml -f docker-compose.ampere.yml pull
docker compose -f docker-compose.yml -f docker-compose.ampere.yml up -d
```

Obraz sidecara `ghcr.io/sdorpl/vizrs-cg` (amd64 i arm64, zmienna
`CG_IMAGE`) buduje workflow wydania po znaczniku `v*`
(`.github/workflows/release.yml`); pakiet
w ghcr musi być publiczny albo maszyna zalogowana (`docker login ghcr.io`).
Bez opublikowanego obrazu dodaj `--build` do `up` – zbuduje go sama A1
(kilka minut). Klient elementu i nakładka ffmpeg (`vizrs-etv-client`,
`vizrs-ffmpeg`, §1) powstają zawsze przy budowie obrazu `seed`: na A1
natywnie dla arm64, statycznie (musl), więc działają w obrazie ErsatzTV arm64
bez dodatkowych pakietów. Na x86 binarka arm64 kompiluje się krzyżowo (ok. 40 s), przez
QEMU idzie tylko instalacja pakietów obrazu.

Binarki arm64 (`vizrs-cg`, `vizrs-ffmpeg`, `vizrs-etv-client`, `vizrs-render`, panel `vizrs-live`) kompilują się z
`RUSTFLAGS="-C target-cpu=neoverse-n1 -C opt-level=3"`. Nakładka `docker-compose.ampere.yml` stroi .NET legacy:
`DOTNET_gcServer=0`, `DOTNET_GCHeapHardLimit=0x60000000` (1,5 GB), `DOTNET_EnableWriteXorExecute=0`.
Obraz VizRS ma wariant arm64.

Nakładka ogranicza sidecar do 2 CPU i 2 GB (`CG_CPUS`, `CG_MEMORY`), uruchamia
Chromium z dwoma wątkami rastra (`VIZRS_BROWSER_ARGS`), daje ErsatzTV
wyższy priorytet CPU, trzyma `/transcode`
w tmpfs 4 GB (`TRANSCODE_TMPFS`) i podnosi `LEAD_SECONDS` do 5. Rozdzielczość i klatkaż
grafiki dziedziczy po ErsatzTV jak każdy stos (§1, §5; przy braku mocy
najpierw obszar grafiki, a kanał 50 kl./s sam zejdzie do 25). Brak
sprzętowego kodera: w profilu FFmpeg ErsatzTV ustaw **Hardware Acceleration:
None**, libx264. Preset ustawia nakładka (`VIZRS_FFMPEG_PRESET`, w compose
A1 domyślnie `veryfast`; §1), bo silnik next pomija preset z profilu.
`superfast` daje przy kilku kanałach, materiałach 1080p50/60 albo ciągłym
tickerze. Przy materiałach o różnym klatkażu (np. 25 i 60 kl./s) włącz
w profilu **Normalize Framerate**: silnik next wstawia wtedy `fps` przed
skalowaniem i nie skaluje zbędnych klatek. Na darmowym kształcie 4 OCPU
realnie jeden kanał 1080p albo dwa 720p transkodowane jednocześnie. Otwórz porty w liście bezpieczeństwa VCN
i w zaporze maszyny (`firewalld`/`iptables`).

Grafika na A1 kosztuje głównie przy animacji: przesuwający się pasek 1080p
to ok. 2 rdzenie Chromium (pomiar na x86); obraz statyczny (logo, plansza,
zegar co sekundę) prawie nic nie kosztuje. Z nakładką ffmpeg ciągła animacja
przy zbyt niskim `CG_CPUS` spowalnia transkodowanie (ffmpeg czeka na klatki
grafiki, §1). Pomiar na 4 OCPU, kanał 1080p25, przewijany ticker w VizRS,
render 720p: z presetem x264 `medium` kanał szedł ok. 0,5× czasu
rzeczywistego, z `superfast` i sidecarem 1,5 CPU ok. 0,8–1× (sidecar dławiony
ok. 30% czasu); stąd domyślne 2 CPU i szybsze skalowanie od wersji 2.1.4.
Pomiar 2026-10-26 (kanał 1600×900@25 na silniku next, materiały 720p25
i 1080p60, ticker, render 720p): `medium` (preset z profilu pominięty)
0,7–0,85×, ErsatzTV ok. 2,5 rdzenia; `VIZRS_FFMPEG_PRESET=veryfast`
0,92–1×, 1,6 rdzenia; `superfast` z Normalize Framerate ≥1×, 1,3 rdzenia. Bez widzów przechwytywanie stoi (§1). Lżejsze
`/output` (mniej ciągłych animacji, mniejsze rozmiary kanału) daje więcej
niż podnoszenie `CG_CPUS`.

Render 1080p kosztuje Chromium ok. 40% więcej CPU na klatkę niż 720p (§1),
a na x86 pełne 1080p w 25 kl./s nie zawsze rysowało się w czasie klatki
(§8). Na A1 sprawdź to benchmarkiem (§8, `BENCH_MAX_FPS=25`, dla porównania
`BENCH_RENDER=720p`); `even_percent` wyraźnie poniżej 90% albo ostrzeżenia
o nienarysowanych klatkach w logu oznaczają, że A1 nie nadąża – wtedy
`VIZRS_AREA` (§1; tylko pasek zamiast całego kanału, ostrość bez zmian),
wyższe `CG_CPUS`, niższe `VIZRS_MAX_FPS`, `VIZRS_RENDER_SIZE=720p` albo
`VIZRS_CAPTURE_MODE=screencast`.

Limity darmowego planu (Always Free, dokumentacja Oracle, stan 2026-10-31):
1500 OCPU-godzin i 9000 GB-godzin A1 miesięcznie, czyli 2 OCPU i 12 GB łącznie
na wszystkie instancje (do czerwca 2026: 4 OCPU i 24 GB), 200 GB dysków, 10 TB
ruchu wychodzącego miesięcznie – średnio
ok. 31 Mb/s, czyli ok. 7 widzów HLS 4 Mb/s oglądających non stop. Instancji
większej niż limit po usunięciu może nie dać się odtworzyć. Budżet na 2 OCPU,
pomiary i odrzucone wymiany składników:
[docs/plany/PLAN-ampere.md](plany/archiwum/PLAN-ampere.md). Oracle
odzyskuje darmowe instancje bezczynne przez 7 dni (jednocześnie CPU
w 95. percentylu, sieć i pamięć poniżej 20%); kanał na żądanie bez widzów
tak wygląda. Konto Pay As You Go z tymi samymi
darmowymi limitami temu nie podlega.

### NVIDIA NVENC

Koduje ErsatzTV; jego ffmpeg ma `h264_nvenc`, `hevc_nvenc` i filtry CUDA.
Ze sterownikiem NVIDIA i
[NVIDIA Container Toolkit](https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/latest/install-guide.html):

```sh
docker compose -f docker-compose.yml -f docker-compose.nvidia.yml up -d
```

potem **Settings → FFmpeg Profiles → Hardware Acceleration** = **Nvidia**.
Karta trafia do kontenera przez CDI (`driver: cdi`, `nvidia.com/gpu=…`), więc
Docker nie potrzebuje runtime'u `nvidia` w `daemon.json` (Docker 25+). Specyfikację
`/var/run/cdi/nvidia.yaml` toolkit (1.18+) generuje sam; bez niej:
`sudo nvidia-ctk cdi generate --output=/var/run/cdi/nvidia.yaml`. Karty pokazuje
`nvidia-ctk cdi list`, a `NVIDIA_GPU` wybiera jedną (indeks albo UUID, domyślnie `all`).
Sprawdzenie: `docker compose exec ersatztv nvidia-smi` i
`docker compose exec ersatztv ffmpeg -hide_banner -f lavfi -i testsrc2 -t 3 -c:v h264_nvenc -f null -`.
Komunikat `could not select device driver "nvidia"` oznacza starą wersję nakładki
albo brak specyfikacji CDI. Grafika się nie
zmienia – trafia do ffmpeg ErsatzTV jako surowe klatki (§1), więc sidecar i jego
Chromium nie potrzebują GPU. Nakładka daje kartę także kontenerowi `cg`:
z `VIZRS_BROWSER_GPU=1` Chromium rysuje na GPU (EGL przez ANGLE: `--use-gl=angle
--use-angle=gl-egl` zamiast `--disable-gpu`; sterownik EGL NVIDIA wstrzykuje CDI,
`libEGL.so.1` jest w obrazie; Vulkan w ANGLE nie działa bez okna). Pomiar
2026-11-11 na Quadro P400 (zrzuty PNG 1920×1080 strony z paskiem, crawlem,
rozmyciem i zegarem): 66 ms CPU na klatkę zamiast 179 ms, za ok. 67 MiB pamięci
GPU. Renderer natywny (`renderer: auto|native`) zawsze rysuje na CPU, więc
przełącznik działa tylko na sceny rysowane przez Chromium (np. ruch `node.motion`).
Sprawdzenie: w logu Chromium (`DEBUG=1`) brak `GLDisplayEGL::Initialize failed`,
a `nvidia-smi` pokazuje proces `chromium-headless-shell`. ErsatzTV wysyła je na kartę (`format=yuva420p,hwupload_cuda`)
i nakłada `overlay_cuda`; `VIZRS_FFMPEG_FPS` działa też na klatkach CUDA. Karty konsumenckie ograniczają liczbę
równoległych sesji NVENC, czyli kanałów transkodowanych naraz.

### VAAPI (Intel, AMD)

Koduje ErsatzTV; jego obraz ma `vainfo` i sterowniki `iHD`, `i965`
i `radeonsi`. Na hoście wystarczy sterownik jądra (`/dev/dri/renderD128`):

```sh
docker compose -f docker-compose.yml -f docker-compose.vaapi.yml up -d
```

potem w **Settings → FFmpeg Profiles**: **Hardware Acceleration** = **Vaapi**,
**VAAPI Device** = `/dev/dri/renderD128`, **VAAPI Driver** = `iHD` (Intel od
Broadwella), `i965` (starsze Intel) albo `RadeonSI` (AMD). Przy kilku kartach
`VAAPI_DEVICE` wskazuje węzeł hosta (`ls -l /dev/dri/by-path`); w kontenerze
to zawsze `/dev/dri/renderD128`. ErsatzTV działa w kontenerze jako root, więc
grupa `render` nie jest potrzebna.
Sprawdzenie: `docker compose exec ersatztv vainfo --display drm --device /dev/dri/renderD128`
i `docker compose exec ersatztv ffmpeg -hide_banner -vaapi_device /dev/dri/renderD128 -f lavfi -i testsrc2 -t 3 -vf format=nv12,hwupload -c:v h264_vaapi -f null -`.

Z grafiką ErsatzTV pobiera obraz z karty (`hwdownload`), nakłada grafikę
programowo (`overlay`) i wysyła z powrotem do kodera (`hwupload`); przy
napisach używa `overlay_vaapi`. Nakładka ffmpeg (§1) działa w obu wariantach
tak samo jak z NVENC: podmienia tylko wejście grafiki, a `VIZRS_FFMPEG_FPS`
wstawia `fps` przed filtrem nakładania. Sidecar nie potrzebuje GPU.

## 5. Uruchamianie bez Dockera, konfiguracja

Binarka z archiwum wydania (`VizRS-CG-Playout-<wersja>-linux-x86_64.tar.gz` albo
`…-linux-arm64`, §7) albo zbudowana ze źródeł (`cargo build --release`,
wynik w `target/release/vizrs-cg`, programy ErsatzTV w
`target/release/vizrs-etv-client` i `vizrs-ffmpeg`; statycznie: `--bin
vizrs-etv-client --bin vizrs-ffmpeg --target <arch>-unknown-linux-musl`) i Chromium (`chromium-headless-shell`,
`chromium`, `google-chrome`, … w `PATH` – w tej kolejności – albo
`VIZRS_BROWSER`). Obraz Dockera używa `chromium-headless-shell` (Debian): ten
sam silnik bez warstwy interfejsu, przy animacji ok. połowa CPU pełnego
`chromium`. W Debianie wskaż samą przeglądarkę,
`VIZRS_BROWSER=/usr/lib/chromium/chromium-headless-shell` – skrypt
`/usr/bin/chromium-headless-shell` uruchamia ją jako proces potomny powłoki,
więc przy zamykaniu sidecara mogłaby zostać osierocona.

```sh
vizrs-cg config.json      # albo CONFIG=config.json vizrs-cg
```

```json
{
  "listen": "0.0.0.0:7700",
  "vizrs": { "address": "127.0.0.1:6100", "token": "" },
  "output_url": "http://127.0.0.1:8088/output",
  "on_clip": "TAKE 4001 headline={title}|sub=Potem: {next_title} o {next_start}",
  "on_ad": "",
  "ad_match": { "path": "/reklamy/", "title": "" },
  "shows": [],
  "ad_without_element": true,
  "in_seconds": 5,
  "out_seconds": 5,
  "on_end": "",
  "end_seconds": 60,
  "end_out_seconds": 5,
  "end_ref": "programme",
  "lead_seconds": 4,
  "render_size": "",
  "area": "",
  "max_fps": 0,
  "capture_mode": "frames",
  "clock_offset_seconds": 0,
  "tape": false,
  "renderer": "chromium",
  "renderer_bin": "",
  "native_layers": true,
  "ingest_token": ""
}
```

`ingest_token` (env `VIZRS_INGEST_TOKEN`; ustawiona pusta zmienna wyłącza
token z pliku): wspólny sekret odbiorników live ingest; puste = live ingest
wyłączony (§1, Live ingest). `ingest_idle_seconds` (env
`VIZRS_INGEST_IDLE_SECONDS`, domyślnie 8, co najmniej 1; błędna wartość w env
trafia do logu i zostaje domyślna): po tylu sekundach ciszy źródło kończy live
(§1, Live ingest); `VIZRS_INGEST_IDLE_MS` usługi `ingest` musi być dłuższe.
`ingest_anytime` (env `VIZRS_INGEST_ANYTIME`,
`0`/`1`, domyślnie `true`) i `live_shows` (tablica, env `VIZRS_LIVE_SHOWS` z tym
samym JSON-em; błędny JSON w env trafia do logu i jest pomijany, błędny wpis
w pliku zatrzymuje start z nazwą pola): §1, Audycje na żywo w ramówce.
`panel_token` (env `VIZRS_PANEL_TOKEN`; ustawiona pusta zmienna wyłącza
token z pliku): token Live API dla panelu, puste = API wyłączone ([API.md §1](API.md#1-live-api-sidecara-apilive-etv-ffmpeg-ingest-health)).
`alert_webhook` (env `VIZRS_ALERT_WEBHOOK`): adres, pod który idą alarmy
dziennika emisji (utrata sygnału, wstrzymane studio, zdjęta grafika); puste =
bez alarmów poza przeglądarką ([API.md §1](API.md#1-live-api-sidecara-apilive-etv-ffmpeg-ingest-health)).
`ingest` (env `VIZRS_INGEST`, ta sama wartość co w usłudze odbiorników; tylko
pierwsze wypełnienie – po zapisie wejść w panelu obowiązuje `inputs.json`),
`ingest_public` (env `VIZRS_INGEST_PUBLIC`: host, pod którym nadawcy widzą
odbiorniki, np. `studio.example.pl` albo IP serwera; puste = host, z którego
operator otwiera panel) i porty na hoście `VIZRS_INGEST_RTMP_PORT`,
`VIZRS_INGEST_SRT_PORT` (compose podaje `INGEST_RTMP_PORT`, `INGEST_SRT_PORT`;
bez nich port z adresu odbiornika): sidecar sam niczego nie odbiera – z tego
Live API podaje w `/api/live/state` każdemu kanałowi `inputs`
(`[{"kind":"rtmp"|"srt","host","port","app","key","passphrase","streamid"}]`),
czyli serwer i klucz dla OBS albo adres SRT z hasłem. To sekrety nadawania:
widzi je tylko ktoś z tokenem Live API.

Opcje `etv_config`, `etv_url`, `client_source`, `client_dir`, `sync_seconds`
(env `VIZRS_ETV_CONFIG`, `VIZRS_ETV_URL`, `VIZRS_CLIENT_SOURCE`, `VIZRS_CLIENT_DIR`,
`VIZRS_SYNC_SECONDS`): synchronizacja z ErsatzTV (§4).

Obiekt `channels` (opcjonalny, env `VIZRS_CHANNELS` z tym samym JSON-em; błędny
JSON w env trafia do logu i jest pomijany) daje kanałowi własny VizRS, własne reguły
grafiki oraz pole `on_live` (komendy grafiki na czas przejęcia kanału przez live
ingest, §1, §3; `on_live` występuje wyłącznie w obiekcie kanału, bez osobnej zmiennej środowiskowej).

Tablica `shows` (opcjonalna) pozwala nadpisać `on_clip`, `on_ad`, `ad_match`,
`on_end` i `end_seconds` dla wybranych audycji (§3, Elementy graficzne per audycja).

`on_end`, `end_seconds`, `end_out_seconds`, `end_ref`: belka przed końcem
audycji (§3, Grafika przed końcem audycji). `end_seconds` i `end_out_seconds`
to 0–86 400 s, `end_ref` – `programme` albo `item`; inna wartość w pliku
zatrzymuje start z nazwą opcji.
Pusty `output_url` = tylko komendy, bez wtapiania. Wzór: `config.example.json`.

Zmienne środowiskowe (nadpisują plik): `LISTEN`, `VIZRS_ADDRESS`,
`VIZRS_TOKEN`, `VIZRS_OUTPUT_URL`, `VIZRS_ON_CLIP`, `VIZRS_ON_AD`
(`\n` rozdziela linie), `VIZRS_AD_PATH`, `VIZRS_AD_TITLE`, `VIZRS_AD_WITHOUT_ELEMENT=0`
(wyłącza reklamę po braku elementu), `VIZRS_IN_SECONDS`,
`VIZRS_OUT_SECONDS` (wejście i zejście grafiki, §3), `VIZRS_ON_END`
(`\n` rozdziela linie), `VIZRS_END_SECONDS`, `VIZRS_END_OUT_SECONDS`,
`VIZRS_END_REF` (grafika przed końcem audycji, §3), `LEAD_SECONDS`, `VIZRS_RENDER_SIZE`
(`720p`, `1280x720`, puste = rozmiar kanału; §1), `VIZRS_AREA` (obszary
grafiki `SZERxWYS+X+Y` po przecinku, np. `1920x140+0+930,160x160+1690+30`,
domyślnie `auto` = ślady stron na antenie, `full` albo puste = cały kanał; §1), `VIZRS_MAX_FPS`
(górna granica tempa przechwytywania, np. `25`; puste albo `0` = klatkaż
kanału), `VIZRS_CAPTURE_MODE` (`frames` albo `screencast`, niżej),
`VIZRS_CLOCK_OFFSET` (sekundy dodane do zegara strony grafiki, niżej),
`VIZRS_TAPE=1` (taśmę crawla przesuwa sidecar, §1; `0` wyłącza),
`VIZRS_RENDERER` (`chromium`, `native` albo `auto` – kto rysuje grafikę, §1;
błędna wartość = `chromium` z ostrzeżeniem), `VIZRS_RENDERER_BIN` (program
`vizrs-render`; puste = obok sidecara albo w `PATH`), `VIZRS_NATIVE_LAYERS`
(`0` = jeden proces renderera natywnego na całą antenę; domyślnie proces na
warstwę show, §1 „Renderer natywny VizRS”),
`VIZRS_INGEST_TOKEN`, `VIZRS_INGEST_ANYTIME`, `VIZRS_INGEST_IDLE_SECONDS`, `VIZRS_LIVE_SHOWS` (live ingest, §1),
`VIZRS_PANEL_TOKEN` (Live API dla panelu, [API.md §1](API.md#1-live-api-sidecara-apilive-etv-ffmpeg-ingest-health)),
`VIZRS_BROWSER`, `VIZRS_BROWSER_NO_SANDBOX=1` (root / kontenery),
`VIZRS_BROWSER_ARGS` (dodatkowe flagi Chromium, rozdzielone spacją),
`VIZRS_BROWSER_GPU=1` / `"browser_gpu": true` (Chromium rysuje na GPU, §4 NVIDIA; `0` wyłącza),
`DEBUG=1`.

Nieznana opcja w pliku trafia do logu jako ostrzeżenie. Zły typ wartości
(np. `"no_sandbox": "yes"`) zatrzymuje start z nazwą opcji; liczbę można
podać także jako tekst (`"lead_seconds": "6"`). `LEAD_SECONDS`,
`VIZRS_IN_SECONDS`, `VIZRS_OUT_SECONDS`, `VIZRS_MAX_FPS` i `VIZRS_CLOCK_OFFSET`, które nie są liczbą, dają ostrzeżenie i zostawiają
poprzednią wartość; tak samo `VIZRS_END_SECONDS` i `VIZRS_END_OUT_SECONDS`
spoza 0–86 400 s oraz `VIZRS_END_REF` inne niż `programme`/`item`.

`max_fps` ogranicza, ile klatek na sekundę Chromium wysyła ze screencastu
(co n-ta klatka z 60 na sekundę, §1): przy `25` kanał 50 kl./s dostaje
animacje w 25 kl./s (każda klatka grafiki dwa razy), a Chromium koduje
i sidecar dekoduje o połowę mniej PNG. Kanały 24–30 kl./s i tak dostają
co drugą klatkę, więc `25` ich nie zmienia. W trybie `frames` (niżej)
Domyślnie klatkaż grafiki to klatkaż ErsatzTV: `FrameRate` z danych elementu,
a przy nakładce ffmpeg jej `x-etv-frame-rate` albo `VIZRS_FFMPEG_FPS`. Przy
**Normalize Framerate** to klatkaż wspólny kanału, bez niego klatkaż bieżącego
materiału, więc przechwytywanie idzie za nim w górę i w dół (przy kilku
sesjach najwyższy; `/health`: `fps`).

**Bezpiecznik płynności** (tryb `frames`): gdy w dwóch kolejnych oknach 5 s
Chromium albo renderer natywny nie zdąży narysować ponad 10% klatek, rysuje
połowę klatkażu, nie mniej niż 24 kl./s
(50 → 25, 60 → 30; kanału 25 nie obniża). Połowa dzieli się całkowicie, więc
ruch zostaje równy: każdy obraz trwa dwie klatki. Gdy przez 60 s rysowanie
zajmuje mniej niż 40% czasu, klatkaż wraca; nieudany powrót podwaja ten czas
(do 1 h). `/health`: `drawn_fps`, w logu `too slow for the channel's rate`.
Jedno złe okno to seria, nie przeciążenie: wejście dużej planszy kosztuje
renderer natywny kilka klatek po 50–200 ms, a połowa klatkażu tego nie
skraca. Z jednym oknem kanał 1080p50 spadał na 25 kl./s co minutę na
kolejną minutę; z dwoma oknami trzyma 50 kl./s (pomiar 2026-11-11, Quadro
P400, renderer na warstwę, ok. 1% klatek z opóźnieniem).
Stały `max_fps` nie jest więc potrzebny, a taki, który nie dzieli klatkażu
kanału, psuje ruch: 30 na kanale 50 dało 80% równo wobec 99,7% bez limitu.
Bezpiecznik jest ochroną, a nie celem: lepiej ustawić obszar grafiki (§1).

`max_fps` to tempo, w jakim Chromium rysuje: przy `25` kanał 50 kl./s
dostaje każdą klatkę grafiki równo dwa razy.

`capture_mode` (`VIZRS_CAPTURE_MODE`) wybiera sposób przechwytywania
Chromium (§1, Płynność animacji):

| Wartość | Działanie |
|---|---|
| `frames` (domyślnie, też puste) | Chromium rysuje tylko na żądanie (`HeadlessExperimental.beginFrame`), raz na klatkę kanału (albo `max_fps`), z czasem tej klatki |
| `screencast` | Chromium składa stronę 60 razy na sekundę, screencast bierze co n-tą klatkę (zachowanie do wersji 2.1) |

`frames` działa tylko z `chromium-headless-shell` (obraz Dockera go ma).
Inna przeglądarka (pełny `chromium`, `google-chrome`) nie ma tej komendy:
sidecar zapisuje ostrzeżenie, uruchamia przeglądarkę ponownie po 3 s i dalej
używa screencastu (`/health`: `mode` przechwytywania). Błędna wartość trafia
do logu, a przechwytywanie idzie trybem `frames`. Gdy host nie nadąża rysować
każdej klatki, co minutę w logu pojawia się ostrzeżenie z liczbą
nienarysowanych klatek – obniż wtedy `max_fps` albo `render_size`.

`clock_offset_seconds` (`VIZRS_CLOCK_OFFSET`, domyślnie `0`) przesuwa `Date`
strony grafiki w Chromium do przodu. ErsatzTV koduje obraz z wyprzedzeniem
(na s2 ok. 30 s), a klatka pokazuje zegar z chwili, w której powstała, więc
zegar i odliczanie na antenie spóźniają się. Wartość to zmierzona różnica
między zegarem na wyjściu a czasem rzeczywistym (na s2: 17 s). Animacje nie
używają `Date`, a stała wartość nie goni zmian wyprzedzenia ErsatzTV.

`ad_match.path` i `ad_match.title` to wyrażenia regularne składni
[`regex`](https://docs.rs/regex/latest/regex/#syntax), bez rozróżniania
wielkości liter, także w polskich znakach (`ogłoszenia` pasuje do
„OGŁOSZENIA”). Nie ma w niej lookahead (`(?=…)`) ani odwołań wstecznych
(`\1`); `\d` i `\w` obejmują też znaki spoza ASCII. Wzorzec, który się nie
kompiluje, daje ostrzeżenie przy starcie i niczego nie dopasowuje – klip
leci wtedy jak zwykły, a sidecar działa dalej.

Daty z danych ErsatzTV (`MediaItem_Start`, `Stop`, EPG) mają offset strefy
(`2026-10-24T20:00:00.0000000+02:00`); data i czas bez offsetu są czytane
jako UTC.

## 6. Awarie

Log sidecara trafia, poza stdout/stderr, do pliku
`<etv_config>/vizrs-live/logs/vizrs-cg.log` (gdy `etv_config` jest ustawione;
powyżej 4 MB plik przechodzi w `….1`, jeden stary zostaje). `docker logs`
znika razem z kontenerem, więc po nocnym wdrożeniu stosu nie było czym
sprawdzić, co działo się z live wcześniej. Folder, którego nie da się
założyć, zostawia log tylko na stdout (ostrzeżenie przy starcie).
Obok leży `vizrs-ingest.log` odbiorników live ingest (usługa `ingest`,
`VIZRS_INGEST_LOG`; ta sama rotacja): nadawcy przychodzący i odchodzący
(`publisher on` / `publisher gone`) i błędy ich ffmpeg, z czasem. Na stderr
odbiorniki piszą jak dotąd, bez czasu. Ślad przejęć kanału (przebiegi live,
ich tempo) jest obok, w `vizrs-takeover-<kanał>.log` (do 26.11.16 w
`<etv_config>/logs/`). Do folderu `<etv_config>/logs/` pisze tylko ErsatzTV:
ekran logów legacy bierze z niego ostatni plik wg nazwy.

Grafika nigdy nie zatrzymuje playoutu. Komendy idą w tle z limitem 2 s na
połączenie i każdą odpowiedź; błędy i odpowiedzi `ERR` trafiają do logu.
Nieudane przywrócenie stron po reklamie jest ponawiane na kolejnym klipie.
Po awarii przeglądarki grafika jest czyszczona, a Chromium wraca po 3 s.
Przeglądarka ma 30 s na start i otwarcie `/output` (pierwszy start na zimnej
maszynie, np. małej VM ARM, potrafi trwać 15–20 s); potem jest restartowana.
Gdy sidecar jest nieosiągalny, klient kończy pracę i pozycja leci bez grafiki.
Nieudana synchronizacja elementów stron (§3) zostawia pliki bez zmian.

Przeglądarka ginie razem z sidecarem, także przy `kill -9`
(`PR_SET_PDEATHSIG`); `SIGTERM` (`docker stop`) zamyka przeglądarki i usuwa
ich profile. Procesy pomocnicze zabitej przeglądarki zbiera init kontenera
(`init: true` w compose); bez niego zostawałyby jako zombie przy PID 1.

Nakładka ffmpeg uruchamia oryginalną komendę ErsatzTV (z płótnem grafiki
ErsatzTV), gdy sidecar nie odpowiada, nie ma sesji elementu VizRS albo
strumień klatek się nie otwiera. Do logu ErsatzTV trafia wtedy linia
`vizrs-ffmpeg: …; ErsatzTV graphics instead`. Przerwany strumień klatek
zostawia ostatni obraz grafiki do końca procesu ffmpeg, bo ffmpeg ErsatzTV
przy końcu wejścia nakładki powtarza jego ostatnią klatkę; obraz programu
leci dalej, a nakładka nie łączy się ponownie. To najwyżej kilkadziesiąt
sekund przy kawałkach po 44 s, ale do końca pozycji, gdy ffmpeg koduje ją
w całości: **restart lub wdrożenie sidecara w trakcie emisji zamraża wtedy
zegary i tickery** (s2, 2026-10-30: stałyby ponad godzinę). `/health` pokazuje
to jako `"sessions":0`, `"overlay":[]` przy działającym kanale. Grafikę
przywraca następna pozycja albo restart ErsatzTV (`docker compose restart
ersatztv`: kanał przerywa się na ok. minutę, pierwsza próba pozycji kończy się
czernią, potem ErsatzTV wznawia ją od właściwego miejsca). Sidecar, który
znika bez zamknięcia połączenia (kontener usunięty z danymi w kolejce TCP),
też nie zatrzymuje kanału: keepalive gniazda klatek zrywa je po ok. 11 s. Gdy ErsatzTV zabija
nakładkę, ffmpeg dostaje `SIGTERM` (`PR_SET_PDEATHSIG`); sygnały zamknięcia
(`SIGTERM`, `SIGINT`, …) nakładka przekazuje ffmpeg. ffmpeg startuje z pustą
maską sygnałów: nakładka blokuje u siebie przekazywane sygnały, a proces
potomny dziedziczy maskę, więc bez jej wyczyszczenia ffmpeg nie przyjmowałby
`SIGTERM` i po zabiciu nakładki zostawałby, dopisując segmenty do playlisty
kanału (do wersji z tą poprawką kończył go dopiero `SIGKILL`).

Live ingest nie zatrzymuje kanału: przebieg live, który nie da obrazu
(sygnał zniknął między sprawdzeniem stanu a startem, uszkodzony strumień),
kończy się, a kawałek leci dalej z harmonogramu; ten sam sygnał jest pomijany
przez 30 s albo do jego zejścia (`the live source gave no picture` w logu
ErsatzTV). Ocenia się tylko przebieg, który miał przed sobą co najmniej 5 s
kawałka: ogon kawałka po przebiegu zatrzymanym za przekroczenie czasu (zwykle
0,1–3 s) jest krótszy niż start ffmpeg, a jego ocena wstawiała na ten ogon
harmonogram (S1, 2026-11-13). Sidecar nieosiągalny = nakładka widzi kanał bez live. Przebieg
programu, który sam skończy się błędem, kończy kawałek z tym kodem jak bez
ingest. Restart sidecara w trakcie live kończy przebieg live (koniec
strumienia), kawałek wraca do harmonogramu, a odbiornik łączy się ponownie
i kanał wraca na live z następnym sprawdzeniem stanu.

Live nie może zostawić osi czasu kanału za zegarem. ErsatzTV (Next engine,
`ersatztv-channel`) liczy czas segmentów z sumy wyprodukowanego materiału, a nie
z zegara, a segmenty starsze niż 2 min kasuje od razu po zapisaniu. Zakłada, że
zwykłe pozycje transkoduje szybciej niż w czasie rzeczywistym i zawsze nadgoni.
Kawałek wypełniany przebiegami live nadgonić nie może: źródło w tempie 0,3×
(S1, 2026-11-13 20:30 UTC, przekaźnik z sesji `.ts` ErsatzTV na S2) rozciągnęło
kawałki 2–3 razy, oś czasu spadła ponad 2 min za zegar, a live w tempie 1,0×
nigdy tego nie odrobił: playlista widzów była pusta przez 3,5 h, aż do restartu
ErsatzTV. Nakładka przed kawałkiem czyta z playlisty (ErsatzTV nadpisuje ją
własną osią czasu przed każdym kawałkiem), o ile kanał jest za zegarem, a w
trakcie przebiegu live co sekundę dolicza czas zegara minus wyprodukowany
materiał. Dwa progi (`src/takeover.rs`):
- **przebieg wolniejszy od zegara** – gdy przebieg live od swojego startu
  stracił do zegara ponad 10 s (`LIVE_LAG_MAX`), źródło jest za wolne:
  przebieg jest kończony, a live wraca najwcześniej po 30 s (w logu `the live
  run lost … s against the clock (the source is slower than the clock)`);
- **oś czasu daleko za zegarem** – powyżej 90 s (`LIVE_BEHIND_MAX`, 30 s
  przed 2 min, po których ErsatzTV kasuje segmenty) przebieg live jest kończony
  i leci harmonogram; nowy przebieg live nie rusza, dopóki oś czasu jest dalej
  niż 75 s za zegarem (`LIVE_BEHIND_START`; do 2026-11-19 progi 60 i 45 s –
  kanał S1 po restarcie ErsatzTV 64 s za zegarem nie wracał na live) – na starcie kawałka i po
  nadgonieniu. Harmonogram nadgania tylko wtedy, gdy ErsatzTV robi kawałki
  szybciej niż w czasie rzeczywistym; z `VIZRS_FFMPEG_SYNC_RAM` idzie w tempie
  zegara i oś czasu zostaje tam, gdzie była.

Do 2026-11-18 był jeden próg 10 s, w trakcie przebiegu i na starcie, a live
wracał dopiero przy osi czasu równej zegarowi. Jeden wolny kawałek ramówki
(np. restart przebiegu dla stref grafiki) zostawiał kanał 10–14 s za zegarem;
sam start przebiegu live (ok. 1 s bez materiału) przekraczał wtedy próg, więc
każdy kawałek dawał ok. 1 s live, a potem harmonogram – na antenie klatkowanie
i skoki obrazu, a grafika przejęcia wchodziła i schodziła co 44 s (lokalny
stos, wejście YouTube; także LUKI z 2026-11-14 po restarcie ErsatzTV). W logu
ErsatzTV: `live leaves the channel's timeline … s behind the clock; the
schedule takes over`, `the channel's timeline is … s behind the clock: live
may come back`, a na starcie kawałka
`the channel's timeline is … s behind the clock`. Playlista samych skasowanych
segmentów (sekwencja powyżej 0, nic na liście) liczy się jako 2 min za zegarem.

## 7. Testy

```sh
cargo test                 # moduły i wektory referencyjne, Chromium (pomijany bez przeglądarki)
cargo build --release
npm test                   # testy kontraktu na target/release/vizrs-cg, vizrs-etv-client i vizrs-ffmpeg
```

- `cargo test` – testy modułów (`src/*.rs`) oraz `tests/*.rs`:
  - **wektory referencyjne** `tests/vectors/*.json`: wejścia i dokładne
    wyniki sidecara Node.js sprzed migracji (kontekst klipu, komendy, daty,
    reklamy, konfiguracja, `STATE`, prostokąty, pakiety ETV, `yuva420p`,
    PNG; skalowanie z tolerancją ±2). Generator jest w historii gita
    (`test/vectors/generate.mjs`, commit 0c54143). Po zamierzonej zmianie
    zachowania wektor poprawia się ręcznie w tym samym commicie;
  - **Chromium** (`tests/chromium.rs`): widoczny prostokąt strony,
    wstrzymanie i wznowienie bez sesji, renderowanie w mniejszym rozmiarze
    ze skalowaniem, tempo animacji.
- `npm test` – **kontrakt** (`tests/contract/`, sama biblioteka standardowa
  Node.js): sidecar jako czarna skrzynka, osobny proces
  (`tests/contract/sidecar.mjs`) z plikiem konfiguracji, udawany VizRS
  (TCP), prawdziwy klient elementu (`target/release/vizrs-etv-client`, inna
  binarka: `VIZRS_ETV_CLIENT_BIN`) i nakładka ffmpeg
  (`target/release/vizrs-ffmpeg`, `VIZRS_FFMPEG_BIN`) z udawanym ffmpeg
  (skrypt Node.js). Przeglądarką jest
  `/bin/false`, więc przechwytywanie istnieje, ale klatki są przezroczyste,
  jak po awarii Chromium; testy sprawdzają protokół (`/etv`, `/ffmpeg`,
  `/ffmpeg/frames`, `/health`) i komendy VizRS. Wtapianie przez prawdziwy
  Chromium (pakiety ETV i `yuva420p`) – `chromium.test.mjs`, bez
  przeglądarki pomijany. Live ingest – `ingest.test.mjs`: API `/ingest`
  z ręcznie złożonym MPEG-TS oraz pełna ścieżka z prawdziwym ffmpeg
  (`VIZRS_TEST_FFMPEG` albo `ffmpeg` w `PATH`, bez niego pomijana): nadawca RTMP,
  odbiornik, kawałek z płótnem grafiki do HLS, w środku live; sprawdza długość
  playlisty, rozmiar kanału, ciągłość znaczników czasu wideo i obraz w segmentach;
  audycja na żywo w ramówce: plik `.yml`, odpowiedź slotu dla ffprobe, plansza
  przed i po studiu, zwykły kawałek nieprzejęty przy `ingest_anytime: false`.
  Live API – `live-panel.test.mjs` z udawanym ErsatzTV (Node.js): token
  (bez strony `/live` i ciasteczka), odrzucanie żądań z obcych stron, kreator
  audycji (plik, skan, Smart Collection, zmiana nazwy, usunięcie, dziennik),
  wstrzymanie kanału w `/ingest/state` także po restarcie, przeniesienie
  `VIZRS_LIVE_SHOWS` do `shows.json`.
  Inną binarkę wskazuje `VIRTRS_CG_BIN`, przy
  `TRACE=1` log sidecara każdego testu trafia na stderr.

### Sprzątanie po testach w Dockerze

Po testach na stosie Dockera zawsze posprzątaj: usuń kontenery, obrazy,
wolumeny, sieci i pliki (zrzuty PNG, logi), które powstały tylko na
potrzeby testu. **Nie ruszaj danych działających instancji tego projektu**
– wolumenów podpiętych do jego obecnie działających kontenerów – żeby nie
trzeba było ich bez powodu kopiować i konfigurować od nowa.

Przed usunięciem wolumenu sprawdź, czy nie używa go działający kontener:

```sh
docker ps --filter volume=<wolumen>    # pusto = wolumen niczyj, można usunąć
```

Nie używaj `docker compose down -v`, `docker volume prune` ani `docker
system prune --volumes` – usuwają dane bez takiego sprawdzenia.

CI (`.github/workflows/ci.yml`) przy każdym wypchnięciu na `main` i w PR
sprawdza `cargo fmt`, `cargo test`, `cargo clippy` (bez ostrzeżeń), buduje
binarkę, uruchamia `npm test` i zostawia binarkę (Linux x86_64) jako
artefakt przebiegu.

Wydanie (`.github/workflows/release.yml`) rusza po znaczniku `v*`
(`git tag v26.11.11 && git push origin v26.11.11`) albo ręcznie (Actions → Release
→ Run workflow, z wersją; bez `publish` powstaje szkic wydania, z `publish` – wydanie i znacznik
na wybranym commicie). Workflow sprawdza
jedną wspólną wersję produktu (`Cargo.toml` sidecara, `panel/server/Cargo.toml`,
`panel/desktop/Cargo.toml`, `panel/desktop/tauri.conf.json`, `panel/bridge/VizRS.Bridge.csproj`
i stała `Version` w `panel/bridge/Bridge.cs`) i wywołuje oba CI tego commita (`ci.yml` – sidecar z testami
kontraktu, `panel-ci.yml` – serwer panelu, okno na trzech systemach, pełna bramka `panel/tests/gate.sh`,
weryfikacja akceleracji sprzętowej `panel/tests/hwaccel.sh` na obrazie legacy, AppImage). Obrazy i wydanie
powstają dopiero po zielonym CI; znaczniki nie uruchamiają CI osobno. Znacznik zawsze `v<wersja>`. Buduje:

- archiwa sidecara `VizRS-CG-Playout-<wersja>-linux-x86_64.tar.gz` i `…-linux-arm64.tar.gz`
  (binarka `vizrs-cg`, `client/` z `vizrs-etv-client` i `vizrs-ffmpeg` –
  statycznymi, musl –, `etv/`, `config.example.json`), arm64 natywnie na runnerze arm,
- archiwa panelu `VizRS-CG-Playout-panel-<wersja>-linux-x86_64.tar.gz` i `…-linux-arm64.tar.gz`
  (statyczna binarka `vizrs-live` z `panel/server`),
- obrazy `ghcr.io/<właściciel>/vizrs-cg:<wersja>`, `vizrs-seed:<wersja>`,
  `vizrs-live:<wersja>` oraz `vizrs-live-legacy:<wersja>[-legacy<wersja>]` (amd64 i arm64),
- aplikację okienną: AppImage natywnego klienta panelu `VizRS-CG-Playout-<wersja>-x86_64.AppImage`
  (z joba `appimage` CI panelu – wtyczki GStreamera podglądu sprawdzone przez `panel/scripts/check-appimage.sh`),
- wydanie na GitHubie z archiwami i `SHA256SUMS.txt`.

Publikacja wersji bez łącznika (znacznik albo ręcznie z `publish`) przesuwa tag `:latest` (dla obrazów `vizrs-cg`,
`vizrs-seed`, `vizrs-live` oraz `vizrs-live-legacy`). Wersja z łącznikiem (`v26.11.11-rc1`) to wydanie wstępne, a szkic
(ręcznie bez `publish`) – próba: obrazy dostają tylko swój znacznik wersji, a `:latest` zostaje bez zmian. Nazwy obrazów oraz zmienne środowiskowe `VIZRS_LIVE_*`
pozostają bez zmian (stosy produkcyjne ich używają).

### Testy panelu

| Test | Polecenie |
|---|---|
| **lokalnie po każdej zmianie** | `panel/tests/deploy.sh` – `cargo fmt --check`, `clippy`, `cargo test`, `node --check` plików JS; obraz panelu z drzewa (poprzedni jako `vizrs-live:poprzedni`); `docker compose up -d --no-deps live` w stosie CG Playout (`VIZRS_CG_DIR`, domyślnie korzeń repozytorium; wymaga `LIVE_IMAGE=vizrs-live:local` w jego `.env`), czekanie na `healthy` i `/health`; `panel/tests/prod-smoke.cjs` – logowanie tokenem z `.env` CG, każdy ekran z menu (nagłówek ≠ „Błąd”, bez wyjątków JS i odpowiedzi 5xx), `DEPLOY_SHOTS` – zrzuty. **Tylko odczyt:** przeglądarka przerywa każde żądanie poza GET, logowaniem i komendami `Get…/Query…/Search…/Count…` + `NewFFmpegProfile` (rodzaj z `/api/commands` nie wystarcza – most oznacza jako „query” tylko przestrzeń `*.Queries`) i oblewa wtedy przebieg. Błąd po podmianie = automatyczny powrót do poprzedniego obrazu. `DEPLOY_NO_SMOKE=1` – bez przejścia |
| serwer (Rust) | `cd panel/server && cargo clippy --all-targets -- -D warnings && cargo test` |
| most + panel (smoke) | `panel/tests/stack.sh up` |
| zrzuty UI bez Dockera (atrapa mostu) | `LEGACY_SRC=<klon ErsatzTV/legacy> node panel/tests/mock-bridge.cjs` (127.0.0.1:18409, token `test-token-0123456789`; metadane komend z rekordów MediatR źródeł legacy jak `/vizrs-bridge/commands`, stałe kanały 24–27, profile, ramówka z dziurą, stan Next, logi; zapisy tylko w pamięci; `MOCK_CG=1` – atrapa CG Playout, `MOCK_NO_HW=1` – maszyna bez akceleracji sprzętowej), panel `--bridge http://127.0.0.1:18409 --web panel/server/web`, potem `PANEL_TOKEN=… node panel/tests/prod-smoke.cjs <panel> <katalog zrzutów>`. Tylko do oglądania ekranów – nie zastępuje e2e na prawdziwym legacy |
| ramówka przez panel | `panel/tests/stack.sh seed` (drugi raz nic nie dubluje; od zera: `panel/tests/stack.sh reset`) |
| e2e UI | `panel/tests/stack.sh test` (Playwright: `PLAYWRIGHT_DIR` albo `panel/tests/local.env`; zrzuty nowych widoków: `E2E_SHOTS=<katalog poza repo>`) |
| proxy (smoke) | w `panel/tests/stack.sh up`: proxy na 127.0.0.1:18410 (`panel/docker/proxy.Caddyfile`, sieć `vizrs-live-test-net`): `/`, `/channels`, `/api/…`, `/vizrs-bridge/…`, `POST` = 403; M3U, XMLTV, `discover.json` = 200. Panel testowy podaje widzom adres proxy (`--stream-url http://127.0.0.1:18410`) |
| **pełna bramka (CI)** | `panel/tests/gate.sh` – wszystko poniżej „lokalnie” po kolei, ze stosem testowym; uruchamia ją `panel-ci.yml` po pushu (z `GATE_BEZ_SERWERA=1` – krok serwera robi tam job `serwer`), lokalnie tylko na prośbę; stos, który sama uruchomiła, po zielonym przebiegu zatrzymuje (`GATE_KEEP_STACK=1` – zostaw), przy błędzie zostawia do debugowania |
| pamięć | `panel/tests/stack.sh mem` – RAM kontenerów stosu, paneli i Chromium z Playwright, katalog danych i jego system plików. Dane domyślnie w `~/.cache/vizrs-live-test` (dysk; `/tmp` bywa tmpfs = RAM). Legacy w stosie z `DOTNET_gcServer=0` (obraz ma w runtimeconfig Server GC – sterta na każdy rdzeń; workstation GC to ok. połowa RAM) i limitem `VIZRS_LIVE_TEST_MEM` (domyślnie `1536m`, `0` – bez limitu; e2e ogląda kanał 24 w podglądzie, a ffmpeg legacy kodujący 1080p zajmuje ok. 775 MB – razem z legacy ok. 1,12 GB, w limicie 1 GB jądro ubija ffmpeg i sesja kanału nie startuje), proxy 128 MB; `panel/tests/real.sh` – Jellyfin, Emby, Plex po `VIZRS_LIVE_REAL_MEM` (domyślnie `1g`). Pomiar 2026-11-07 (up + seed + e2e): legacy ok. 850 → 420–480 MB bez oglądania kanału, dane w tmpfs 300 MB → 0. Produkcji to nie dotyczy (`docker-compose.live.yml` bez zmian) |
| CI | `.github/workflows/panel-ci.yml` (push na `main`, PR, ręcznie; wywoływany też przez `release.yml`): serwer (`--locked`, binarka jako artefakt `vizrs-live-linux-x86_64`), krzyżowa budowa obu obrazów na arm64, obraz panelu, `panel/tests/gate.sh` z Playwright 1.58.2 (akcja `.github/actions/playwright`: `PLAYWRIGHT_DIR` = `$RUNNER_TEMP/pw` przez `GITHUB_ENV`) i ffmpeg z libx265 (K18), po bramce krok `panel/tests/hwaccel.sh` (sprawdzenie akceleracji Intel/AMD/NVIDIA w obrazie legacy); okno na trzech systemach; AppImage z GStreamerem (`panel/scripts/check-appimage.sh`, artefakt `vizrs-cg-playout-appimage` dla wydania); `panel-legacy-latest.yml` (poniedziałek 04:17 UTC i ręcznie): stos na `ghcr.io/ersatztv/legacy:latest` + seed + e2e |

**Prawdziwe usługi zamiast atrap (`panel/tests/real.sh`, ręcznie, poza bramką).** e2e testuje Jellyfin i CG Playout na atrapach; `panel/tests/real.sh up` stawia obok stosu testowego prawdziwe: **Jellyfin** (127.0.0.1:18196), **Emby** (18896), **Plex** (18324) – w sieci `vizrs-live-test-net`, z wideo testowym seeda pod `/media/info24` – oraz **VizRS CG Playout** (binarka, 127.0.0.1:18701, live API na danych stosu) i drugi panel (**127.0.0.1:18092**) podłączony do niego (panel 18090 zostaje na atrapie e2e). Jellyfin i Emby konfiguruje sam (konto `vizrs`, biblioteka „Filmy E2E”, klucz API w `$VIZRS_LIVE_TEST_DIR/real/*.key`); `panel/tests/real.sh connect` łączy z nimi legacy przez panel; `status`, `down` (najpierw odłącza legacy od Jellyfin i Emby – ich pozycje zmieniają liczby, których oczekuje e2e, więc przed `panel/tests/stack.sh test` zawsze `down`), `reset` (usuwa też dane usług). Plex: logowanie PIN przez plex.tv – ręcznie w panelu (Źródła zewnętrzne → Plex), opcjonalnie `PLEX_CLAIM`. Sprawdzone 2026-11-07: legacy ↔ Jellyfin 10.10.7 i Emby 4.8.11 (połączenie, klucz zamaskowany, biblioteki, zamiana ścieżki `/media/info24` → `/config/media/info24`, synchronizacja – film „Pogoda” w indeksie legacy); reguły grafiki na prawdziwym CG Playout (odczyt, błędny regex odrzucony, zapis `graphics.json`). Bez silnika VizRS (komendy grafiki idą w próżnię) i bez Plex z kontem.

Plan rozbudowy realizuje automat `panel/scripts/plan.sh` (opis w [plany/archiwum/PLAN-rozbudowy.md](plany/archiwum/PLAN-rozbudowy.md), „Jak pracować”).

## 8. Benchmark przechwytywania grafiki

`bench/sidecar.mjs` mierzy na maszynie docelowej, ile kosztuje wtapianie
grafiki bez GPU: binarka sidecara renderuje stronę przez jedną sesję,
a pomiar obejmuje CPU i pamięć samego procesu sidecara (dekodowanie PNG,
prostokąty, pakiety albo klatki `yuva420p`; przeglądarka liczy się osobno)
oraz tempo nowych obrazów. Tryby (`BENCH_MODES`):

- **`etv`** – pakiety ETV dla elementu „script” ErsatzTV,
- **`ffmpeg`** – surowe klatki `yuva420p` dla nakładki ffmpeg
  (`BENCH_FFMPEG_FPS`, domyślnie 50; sesja skryptu zostaje otwarta, jak
  w ErsatzTV).

Strona to `bench/scene.html`, podobna do grafiki VizRS: przezroczyste tło,
zegar, logo obracane w 3D, lower third z gradientem, `drop-shadow`
i `blur`, wysuwany co 5 s, tekst przez `EventSource`, ticker na
`requestAnimationFrame` (scena `animated`; `static` ma tylko zegar
i tekst). Zamiast sceny można zmierzyć prawdziwe `/output` VizRS
(`BENCH_URL`; grafika musi być na antenie).

Scena `animated` rysuje w lewym górnym rogu znacznik: 16 pól 8×8 px, białe
= 1, czarne = 0, numer kroku animacji (czas `requestAnimationFrame` przez
długość klatki). Benchmark czyta go z każdej klatki wyjścia, tak jak bierze
ją ErsatzTV (pakiety ETV z `Repeat`) albo ffmpeg (klatki `yuva420p`),
i liczy, czy ruch poszedł dokładnie o jeden krok, stał, czy przeskoczył.
Scena robi krok raz na klatkę wyjścia, a przy `BENCH_MAX_FPS` niższym od
klatkażu raz na kilka (np. 50 kl./s przy `25`: równo = krok co drugą klatkę).

| Kolumna | Znaczenie |
|---|---|
| `render`, `area`, `fps`, `capture`, `max_fps` | rozmiar renderowania, obszar grafiki, klatkaż wyjścia trybu, tryb przechwytywania (`/health`), `max_fps` |
| `tape` | z `BENCH_TAPE=1`: crawl rysowany przez sidecar na końcu pomiaru (`/health`, `–` = żaden) i filmy taśmy zrobione w czasie pomiaru |
| `cpu_percent` | CPU procesu sidecara, % jednego rdzenia |
| `browser_cpu_percent` | CPU przeglądarki (procesy potomne sidecara), % jednego rdzenia |
| `rss_mb` | pamięć procesu sidecara (głównie klatki: historia PNG na `lead_seconds` + 1,6 s i obrazy pokazywane przez sesje) |
| `pictures_per_s` | nowe obrazy grafiki na sekundę (animacja przy 25 fps: ok. 25) |
| `mb_per_s` | strumień do ErsatzTV albo ffmpeg |
| `even_percent`, `stood_percent`, `skipped_percent` | klatki wyjścia z ruchem równym (ideał 100%), stojącym i z przeskokiem (scena `animated`) |
| `ffmpeg_cpu_percent` | z `BENCH_GPU=1`: CPU procesu ffmpeg nakładającego grafikę, % jednego rdzenia |
| `gpu_mib`, `gpu_card_mib` | z `BENCH_GPU=1`: najwyższa pamięć GPU tego ffmpeg i całej karty w czasie pomiaru (`nvidia-smi`, co 1 s), MiB |
| `ffmpeg_error` | z `BENCH_GPU=1`: ffmpeg zakończył się w czasie pomiaru (ostatnie linie jego błędów) |

Zmienne środowiskowe (wszystkie opcjonalne):

| Zmienna | Domyślnie | Znaczenie |
|---|---|---|
| `BENCH_MODES` | `etv,ffmpeg` | tryby |
| `BENCH_SCENE` | `animated` | `animated` albo `static` |
| `BENCH_URL` | – | adres do zmierzenia, np. `http://vizrs:8088/output?token=…` |
| `BENCH_SIZE` | `1920x1080` | rozmiar kanału |
| `BENCH_FPS` | `25` | klatkaż kanału |
| `BENCH_FFMPEG_FPS` | `50` | klatkaż nakładki ffmpeg |
| `BENCH_CAPTURE` | `frames` | `capture_mode` sidecara (`screencast`, §5) |
| `BENCH_MAX_FPS` | `0` | `max_fps` sidecara |
| `BENCH_SCENE_FPS` | `0` | tempo sceny i oceny równości (przy bezpieczniku), `0` = klatkaż wyjścia |
| `BENCH_RENDER` | – | `render_size` sidecara, np. `720p` |
| `BENCH_AREA` | – | `area` sidecara, np. `1920x140+0+930` (pasek z tickerem sceny) albo z logo `1920x140+0+930,160x160+1690+30`; znacznik kroku trafia w lewy górny róg pierwszego obszaru |
| `BENCH_TAPE` | – | `1` = `tape` sidecara (§1; z `BENCH_URL` i crawlem VizRS na antenie) |
| `BENCH_DELTA` | – | `1` = tryb `ffmpeg` czyta klatki jako deltę (`delta=1`, jak `vizrs-ffmpeg`, §1 Nakładka ffmpeg), składane z powrotem w całe klatki do oceny ruchu |
| `BENCH_GPU` | – | `1` = tryb `ffmpeg` przekazuje klatki też do prawdziwego ffmpeg, który nakłada je tak jak `vizrs-ffmpeg` przy `VIZRS_FFMPEG_CUDA=overlay` (na każdy obszar `crop`, `hwupload`, `overlay_cuda` w jego miejscu, na obraz testowy `testsrc2` kanału; klatki wracają do RAM dla kodera), i mierzy CPU oraz pamięć GPU tego ffmpeg; tylko NVIDIA, poza Dockerem (`nvidia-smi`, ffmpeg z CUDA i NVENC) |
| `BENCH_GPU_ENCODER` | `h264_nvenc` | koder ffmpeg z `BENCH_GPU=1` (NVENC: `-preset p1 -bf 0`) |
| `BENCH_GPU_MIN_FREE` | `400` | z `BENCH_GPU=1`: najmniej wolnej pamięci karty [MiB] przed startem ffmpeg, inaczej pomiar się nie zaczyna (karta bywa dzielona z działającym kanałem) |
| `BENCH_FFMPEG` | `ffmpeg` | ffmpeg dla `BENCH_GPU=1` |
| `BENCH_WARMUP` | `10` | rozgrzewka przed pomiarem [s] |
| `BENCH_DURATION` | `30` | pomiar [s] |

Poza Dockerem (Node.js 22+, Chromium w `PATH` albo `VIZRS_BROWSER`):

```sh
cargo build --release && npm run bench
```

Mierz na maszynie docelowej i bez innego obciążenia (zatrzymaj na ten czas
sidecar, jeśli działa na tym samym hoście) – wyniki z różnych maszyn nie są
porównywalne.

### Instalacja w Portainerze

Obraz buduje się z repozytorium na maszynie docelowej (amd64 albo arm64,
np. Ampere A1); pierwszy build trwa kilka minut.

1. **Stacks → Add stack**, nazwa np. `virtrs-bench`.
2. **Build method: Repository**:
   - Repository URL: `https://github.com/sdorpl/VizRS-CG-Playout`
   - Repository reference: `refs/heads/main`
   - Compose path: `bench/docker-compose.yml`
   - (repozytorium jest publiczne – bez uwierzytelniania)
3. **Environment variables** (opcjonalnie): np. `BENCH_DURATION=60`, a do
   pomiaru prawdziwej grafiki `BENCH_URL` z adresem `/output` osiągalnym
   z kontenera (adres hosta, nie `localhost`).
4. **Deploy the stack**. Wyniki: **Containers → virtrs-bench-bench-1 →
   Logs** (tabela i linia JSON); kontener kończy pracę po pomiarze.
5. Kolejny pomiar: **Containers → virtrs-bench-bench-1 → Start**.
6. Po zakończeniu sprzątanie (§7): **Stacks → virtrs-bench → Delete**, potem
   w **Images** `vizrs-cg-bench:local`.

Bez Portainera, z katalogu repozytorium:

```sh
docker compose -f bench/docker-compose.yml -p virtrs-bench up --build
# sprzątanie
docker compose -f bench/docker-compose.yml -p virtrs-bench down --rmi local
```

### Uwagi i wyniki

- **Równość ruchu, screencast wobec `frames`** (Xeon 2,8 GHz, 4 vCPU
  w chmurze, Chromium 153 `chrome-headless-shell`, scena `animated`, 15 s;
  równo / stoi / przeskok, CPU przeglądarki):

  | Kanał | Tryb sidecara | `screencast` | `frames` |
  |---|---|---|---|
  | 720p25 | `etv` | 60% / 16% / 24%, 150% | 95% / 3% / 3%, 97% |
  | 720p25 | `ffmpeg` | 31% / 32% / 37%, 134% | 97% / 2% / 2%, 95% |
  | 720p30 | `ffmpeg` | 55% / 18% / 27%, 151% | 86% / 7% / 7%, 109% |
  | 720p50 | `ffmpeg` | 48% / 24% / 28%, 178% | 27% / 39% / 34%, 115% (31 obrazów/s) |
  | 720p50, `max_fps` 25 | `ffmpeg` | 62% / 16% / 22%, 146% | 99% / 1% / 0%, 92% |
  | 1080p25 | `etv` | 10% / 47% / 43%, 152% | 41% / 30% / 29%, 118% (17,5 obrazu/s) |
  | 1080p25, render 720p | `etv` | 64% / 14% / 22%, 145% | 93% / 3% / 3%, 97% |
  | 1080p25, render 720p | `ffmpeg` | 30% / 32% / 38%, 131% | 87% / 4% / 9%, 95% |

- **Dziedziczenie po ErsatzTV** (Xeon E3-1270 v3, 8 wątków, obok działającego
  stosu, `frames`, tryb `ffmpeg`, scena `animated`; równo / stoi / przeskok,
  CPU sidecara / przeglądarki):

  | Kanał | Wariant | Wynik |
  |---|---|---|
  | 900p25 | render 1080p | 78 / 11 / 11, 72% / 114% |
  | 900p25 | rozmiar kanału | 99 / 0,3 / 0,3, 29% / 96% |
  | 1080p50, cała klatka | `max_fps` 30 | 54 / 20 / 26, 47% / 122% |
  | 1080p50, cała klatka | bez limitu, bez bezpiecznika | 8,5 / 53 / 39, 49% / 121% |
  | 1080p50, cała klatka | bezpiecznik (25) | 84 / 10 / 6, 45% / 114% |
  | 1080p50, obszar | `max_fps` 30 | 80 / 0,2 / 20, 10% / 38% |
  | 1080p50, obszar | klatkaż kanału | 99,7 / 0,2 / 0,2, 16% / 61% |

  `BENCH_SCENE_FPS` ustawia tempo sceny na klatkaż, do którego zejdzie
  bezpiecznik; bez tego pomiar liczy kroki dla pełnego klatkażu.

- **Delta klatek nakładki** (2026-11-11, `BENCH_MODES=ffmpeg`, 1080p50, cały
  kanał, host z działającym lokalnym stosem – Chromium dawał tylko 11–23
  obrazów/s): strumień 259,5 MB/s całymi klatkami, 21–28 MB/s deltą (−90%).
  CPU sidecara podobne w obu trybach (36–49%, zależnie od liczby obrazów do
  zamiany); zysk jest po stronie odbiorcy i jądra (pętla TCP), którego ten
  pomiar nie obejmuje. Na lokalnym stosie (renderer natywny na warstwę,
  `area: auto`) po wdrożeniu delty, wstrzymania i poprawionych śladów:
  kontener `cg` 15% zamiast 30–39%, `vizrs-ffmpeg` 5% zamiast 22% – przy
  strefach 44% kadru zamiast 57% i całego kadru, więc nie cały spadek to
  delta.

- **Obszar grafiki** (`BENCH_AREA=1920x140+0+930`, ten sam host, kanał
  1080p, render 1080p; równo / stoi / przeskok, CPU sidecara / przeglądarki,
  strumień):

  | Tryb sidecara | `capture_mode` | cały kanał | obszar |
  |---|---|---|---|
  | `etv` 25 | `screencast` | 4 / 50 / 47, 21% / 149%, 18 MB/s | 15 / 39 / 46, 5% / 62%, 12 MB/s |
  | `ffmpeg` 25 | `screencast` | 10 / 48 / 41, 71% / 153%, 131 MB/s | 20 / 39 / 41, 18% / 63%, 17 MB/s |
  | `ffmpeg` 50 | `screencast` | 9 / 39 / 53, 89% / 197%, 162 MB/s | 41 / 26 / 32, 34% / 127%, 34 MB/s |
  | `etv` 25 | `frames` | 35 / 34 / 32, 27% / 118%, 23 MB/s | 99,5 / 0,3 / 0,3, 7% / 45%, 17 MB/s |
  | `ffmpeg` 25 | `frames` | 38 / 32 / 30, 73% / 119%, 131 MB/s | 98 / 2 / 1, 19% / 45%, 17 MB/s |
  | `ffmpeg` 50 | `frames` | 0 / 66 / 34, 99% / 116%, 184 MB/s | 96,5 / 2 / 2, 34% / 85%, 34 MB/s |

  Z renderem 720p obszar daje te same 99,5% (`frames`), ale CPU sidecara
  rośnie do 38–45% przez skalowanie.

- **Kilka obszarów** (`frames`, ten sam host, 1080p; równo / stoi /
  przeskok, CPU sidecara / przeglądarki, obrazy na sekundę, strumień):

  | Obszar | `etv` 25 | `ffmpeg` 50 |
  |---|---|---|
  | cały kanał | 33 / 35 / 32, 26% / 117%, 16,3/s | 0 / 66 / 34, 100% / 117%, 16,5/s, 189 MB/s |
  | pasek `1920x140+0+930` | 99,5 / 0,3 / 0,3, 7% / 46%, 24,9/s | 94 / 4 / 2, 36% / 87%, 48/s, 34 MB/s |
  | pasek + logo `…,160x160+1690+30` | 98,9 / 0,5 / 0,5, 10% / 86%, 24,9/s | 41 / 30 / 29, 57% / 117%, 35/s, 72 MB/s |
  | ten sam pasek i logo jako jeden prostokąt `1920x1050+0+30` | 36 / 33 / 31, 27% / 119%, 16,8/s | 0 / 66 / 34, 100% / 117%, 184 MB/s |

  Tabelę zmierzono przy jednej karcie przełączającej `viewport` między
  obszarami: dwa obszary to były dwa rysowania i dwa zrzuty na klatkę, przy
  50 kl./s za dużo na 20 ms. Z kartą na obszar (§1; Xeon E3-1270 v3, scena
  `animated`, 15 s) pasek + logo dają 98,9 / 0,5 / 0,5, 13% / 73%, 25/s
  w `etv` 25 i 99,5 / 0,3 / 0,3, 37% / 88%, 50/s, 72 MB/s w `ffmpeg` 50
  (sam pasek na tym hoście: 99,5% równo, przeglądarka 75%).

  Ten host nie rysuje 1080p w pełnym rozmiarze w 25 kl./s ani 720p w 50
  kl./s; tam oba tryby są słabe, a `frames` przy 50/60 kl./s bez `max_fps`
  gorszy. CPU sidecara w obu trybach podobne. Pełna macierz (też 30 i 60
  kl./s): [docs/plany/PLAN-plynnosc.md](plany/archiwum/PLAN-plynnosc.md) §4.

- **Taśma crawla** (`BENCH_TAPE`, §1; Xeon E3-1270 v3, 8 wątków,
  `chrome-headless-shell` 146, `frames`, 1080p, prawdziwe `/output` VizRS
  ze stroną 3001 „Ticker” z `seed/` na antenie, 20 s; CPU sidecara /
  przeglądarki, obrazy na sekundę, filmy w czasie pomiaru):

  | Obszar | Tryb sidecara | bez `tape` | z `tape` |
  |---|---|---|---|
  | cały kanał | `ffmpeg` 50 | 70% / 121%, 34,5/s | 61% / 14%, 49,8/s, 2 filmy |
  | `1920x80+0+1000` | `etv` 25 | 6% / 30%, 25/s | 5% / 10%, 25/s, 2 filmy |
  | `1920x80+0+1000` | `ffmpeg` 50 | 22% / 49%, 50/s | 23% / 18%, 50/s, 1 film |

  Ampere A1 (s2, Neoverse-N1, 4 rdzenie, obok działającego kanału 4 –
  load 6–12, więc liczby są zawyżone; Chromium z Debiana
  `chromium-headless-shell`, 1600×900, 25 kl./s, `max_fps` 25, nakładka
  ffmpeg 25, strony 3004 „Ticker przewijany z kategoriami” i 2101
  „Odliczanie” z show kanału 4; CPU sidecara / przeglądarki, obrazy na
  sekundę):

  | Obszar | Tryb sidecara | bez `tape` | z `tape` |
  |---|---|---|---|
  | cały kanał | `etv` 25 | 18% / 61%, 24,3/s | 14% / 16%, 25/s |
  | cały kanał | `ffmpeg` 25 | 34% / 57%, 23,1/s | 32% / 16%, 24,6/s |
  | `1600x110+0+790` | `etv` 25 | 4% / 21%, 25/s | 3% / 12%, 25/s |
  | `1600x110+0+790` | `ffmpeg` 25 | 7% / 21%, 25/s | 6% / 13%, 25/s |
  | `1600x110+0+790,360x94+1200+82` | `etv` 25 | 6% / 43%, 25/s | 4% / 49%, 24,9/s |
  | `1600x110+0+790,360x94+1200+82` | `ffmpeg` 25 | 8% / 44%, 24,3/s | 9% / 48%, 24,8/s |

  Wiersze z dwoma obszarami (ustawienie kanału 4) zmierzono jeszcze przy
  jednej karcie przełączającej `viewport` między obszarami: każda zmiana
  `viewport` to zrzut obu obszarów w każdej klatce, więc taśma nic nie dawała.
  Z kartą na obszar (§1) na Xeonie E3-1270 v3 (te same strony, 1600×900,
  15 s) przeglądarka bierze 54% bez opcji i 25–27% z nią (z jedną kartą
  71–88% i 73–97%); sam pasek: 42% i 16%.

  Chromium przy stojącym crawlu rysuje już tylko zegar (raz na sekundę)
  i filmy, więc jego CPU spada 3–9 razy, a cały kanał nadąża z pełnym
  tempem obrazów. CPU sidecara się nie zmienia: składanie taśmy z filmów
  kosztuje tyle co dekodowanie PNG. Na zrzutach obraz z taśmą sidecara
  wygląda jak rysowany przez stronę (ten sam tekst, krawędzie okna
  przycięte tak samo).

- **Sidecar w Ruście wobec Node.js** (Xeon E3-1270 v3, 1080p, scena
  `animated`, 25 s): tryb `etv` – Rust 28% CPU i 93–96 MB, Node 53–56%
  i 263–280 MB; tryb `ffmpeg` (50 kl./s) – Rust 79–93% i 161–187 MB, Node
  111–140% i 351–390 MB; tempo obrazów takie samo (21–38/s).
- Wyniki niżej pochodzą ze starego benchmarku silników (`bench/bench.mjs`,
  usunięty przy migracji do Rusta, w historii gita – commit 0c54143):
  Chromium jak w sidecarze, Chromium zsynchronizowany z klatkami
  (`chromium-frames`) i WPE WebKit.

- **Ampere A1** (Neoverse-N1, 4 rdzenie, 1080p, 25 fps, 60 s): WPE przy
  animacji robi tylko 12,2 kl./s przy 243% CPU i 366 MB, Chromium 23,4
  kl./s przy 209% CPU i 223 MB; przy scenie statycznej oba ok. 36% CPU.
  WPE odpada.
- **`chromium-frames`** (Xeon E3-1270 v3 w Dockerze, Chromium 156, 12 s):
  ruch na wyjściu przy 720p 99% równych kroków wobec 65% w `chromium`, przy
  130% CPU wobec 149%; przy 1080p 71–92% wobec 65% przy podobnym CPU – tam
  ten procesor nie nadąża z PNG w każdym slocie. Scena statyczna kosztuje
  tyle co w `chromium`.
- Pułapki `beginFrame` w Chromium 156: z flagą
  `--disable-new-content-rendering-timeout` każda klatka zgłasza zmianę;
  żądanie zrzutu zawsze ją zgłasza, a klatka po zrzucie też – pochłania ją
  pusta klatka zaraz po zrzucie, a na stojącej stronie co sekundę idzie
  zrzut kontrolny.
- WebKit, podobnie jak Chromium, nie uruchomi w kontenerze własnej
  piaskownicy; benchmark ją wyłącza
  (`WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1`) – to odpowiednik
  `--no-sandbox`. WPE nie załadował `FontFace` z `local()`.


## 9. Panel kanału – składniki

| Składnik | Gdzie | Rola |
|---|---|---|
| VizRS Bridge | `panel/bridge/` (C#, .NET 10) | most REST w procesie ErsatzTV legacy: ogólny dyspozytor komend z białą listą ([API.md §3](API.md#3-api-mostu-vizrs-bridge)) |
| vizrs-live | `panel/server/` (Rust, bez zależności) | serwer panelu: UI, logowanie, pośrednik do mostu i CG Playout ([API.md §2](API.md#2-api-panelu-api)) |
| UI panelu | `panel/server/web/` (moduły ES, bez bibliotek) | ekrany na wzór web GUI legacy, po polsku; wbudowane w binarkę (`include_str!`), budowa w [§11](#11-panel-vizrs-live) „UI” |
| okno panelu | `panel/desktop/` (Tauri 2) | natywny klient panelu na serwerze (`VizRS-CG-Playout-<wersja>-x86_64.AppImage`) |
| obraz legacy z mostem | `panel/docker/legacy-bridge.Dockerfile` | `FROM ghcr.io/ersatztv/legacy` + `VizRS.Bridge.dll` + `commands.txt` + 3 zmienne środowiskowe ([§10](#10-most-vizrs-bridge)) |
| obraz panelu | `panel/docker/live.Dockerfile` | statyczna binarka musl w `scratch` |
| proxy dla widzów | `panel/docker/proxy.Caddyfile` | na zewnątrz tylko strumienie, M3U/XMLTV, obrazy i HDHomeRun; w nakładce wpisany w `configs:`, sprawdzany przez `panel/scripts/check-caddy.sh` ([§15](#15-zamknięcie-web-gui-legacy-r10)) |
| wdrożenie panelu | `docker-compose.live.yml` | nakładka na `docker-compose.yml` VizRS CG Playout: `ersatztv` → legacy z mostem bez portu, `proxy`, `live` z `http://cg:7700` ([RUNBOOK.md §2a](RUNBOOK.md#2a-wdrożenie-w-portainerze-gitops-i-konfiguracja-proxy)); lokalna budowa: `panel/compose.build.yml` |
| akceleracja sprzętowa | `docker-compose.vaapi.yml`, `docker-compose.nvidia.yml` | nakładki przekazujące kartę graficzną (Intel/AMD VAAPI/QSV, NVIDIA NVENC przez CDI) do usługi `ersatztv` ([RUNBOOK.md §2b](RUNBOOK.md#2b-akceleracja-sprzętowa-intel-amd-nvidia)) |
| CI i wydanie | `.github/workflows/panel-ci.yml`, `.github/workflows/release.yml` | bramka na GitHub Actions; jedno wydanie całego produktu (sidecar, panel, most, AppImage) po tagu `v*` ([RUNBOOK.md §7](RUNBOOK.md#7-wydanie-nowej-wersji-opiekun-repozytorium)) |

### Pakiet VizRS – zgodność aplikacji

Zasady zgodności i opis pakietu VizRS (trzy aplikacje: VizRS CG System, VizRS CG Playout z panelem, VizRS CG Artist) znajdują się w [§1](#pakiet-vizrs--zgodność-aplikacji); opis całego pakietu: [sdorpl/VizRS](https://github.com/sdorpl/VizRS).

## 10. Most (VizRS Bridge)

### Wpięcie bez zmiany legacy

- `DOTNET_STARTUP_HOOKS=/vizrs-bridge/VizRS.Bridge.dll` – hak startowy .NET; rejestruje rozwiązywanie zestawu `VizRS.Bridge` (nie ma go w `ErsatzTV.deps.json`).
- `ASPNETCORE_HOSTINGSTARTUPASSEMBLIES=VizRS.Bridge` – ASP.NET Core ładuje `BridgeHostingStartup`, który dokłada oprogramowanie pośrednie na początek potoku legacy (`IStartupFilter`).
- `DOTNET_gcServer=0` – workstation GC zamiast server GC z runtimeconfig legacy (ok. połowa RAM, [§7](#testy-panelu)). Obraz **nie** ustawia limitu sterty (`DOTNET_GCHeapHardLimit`): po jego przekroczeniu legacy pada, a z nim emisja wszystkich kanałów; limit i `DOTNET_EnableWriteXorExecute=0` daje tylko nakładka Ampere A1 VizRS CG Playout (`docker-compose.ampere.yml`). Segmenty HLS (`/transcode`) – tmpfs w `docker-compose.yml` i `docker-compose.live.yml`.
- Most kompiluje się tylko przeciw ASP.NET Core i `MediatR.dll` **z obrazu legacy** (wersja zawsze zgodna). Komendy legacy wyszukuje w czasie działania (refleksja po `ErsatzTV.Application`), więc nie wiąże się z ich wersją przy kompilacji.

### Znaki wodne płótna grafiki

`GET /vizrs-bridge/watermarks/{kanał}/{IdPozycji}` ([API.md §3](API.md#3-api-mostu-vizrs-bridge)) podaje nakładce `vizrs-ffmpeg` znaki wodne pozycji z elementem VizRS (§1, „Znaki wodne ErsatzTV”). Most liczy je tak jak legacy przy płótnie grafiki: tę samą pozycję i kanał (`GetGraphicsCanvasStreamHandler.LoadCanvasItem`, także kanał-lustro), ten sam wybór (`IWatermarkSelector.SelectWatermarks`: deco szablonu i playoutu, znaki pozycji, kanału i globalny, wyłączenie na czas wypełniaczy) i to samo położenie (`ImageElementBase.CalculateBounds` z rozmiarem obrazu i kadrem treści). Wszystko przez refleksję, bez kopiowania kodu legacy i bez zapisów; legacy w innej postaci (zmieniona nazwa albo sygnatura) = 500 z opisem, a nakładka zostaje bez znaków wodnych, jak dawniej. Obraz, którego nie da się odczytać, most pomija (wpis w logu legacy).

`GET /vizrs-bridge/watermarks/{kanał}?ids=3,5` (od 26.11.26) podaje znaki wodne wybrane dla przejęcia przez live ingest (`live_watermark` reguły kanału albo wejścia, §1 „Grafika podczas przejęcia”): encje kanału (z profilem i rozdzielczością) i znaków przez `DbContext.Set<T>()` z `Include` po nazwie, obraz przez `IWatermarkSelector.GetWatermarkOptions` (znak jako znak pozycji, bez globalnego), położenie tym samym `CalculateBounds` w całym kadrze kanału. Tylko odczyt; legacy w innej postaci = 500, a przejęcie idzie bez znaku.

### API mostu

Most VizRS Bridge udostępnia API REST w procesie ErsatzTV legacy (`/vizrs-bridge/*`, port 8409 w sieci wewnętrznej, autoryzacja nagłówkiem `X-VizRS-Token`). Pełny opis punktów końcowych, dyspozytora komend MediatR, formatów odpowiedzi oraz endpointów specjalnych znajduje się w [API.md §3](API.md#3-api-mostu-vizrs-bridge).

### Konfiguracja mostu

| Zmienna | Znaczenie |
|---|---|
| `VIZRS_BRIDGE_TOKEN` | wymagany, min. 16 znaków; bez niego most odpowiada 503 i nic nie wykonuje |
| `VIZRS_BRIDGE_COMMANDS` | ścieżka białej listy (domyślnie `commands.txt` obok DLL) |
| `VIZRS_BRIDGE_DATA` | katalog danych mostu (klatkaż profili, `framerates.json`); domyślnie `/config/vizrs-bridge` |

Biała lista: jedna nazwa komendy w wierszu, `#` – komentarz. Nazwy nieznane w danej wersji legacy pokazuje `GET /health` (`commandsMissing`) – to pierwszy sygnał zmiany w nowym wydaniu legacy.

## 11. Panel (vizrs-live)

### Opcje

| Opcja | Zmienna | Domyślnie |
|---|---|---|
| `--bind` | `VIZRS_LIVE_BIND` | `127.0.0.1` (w obrazie `0.0.0.0`) |
| `--port` | `VIZRS_LIVE_PORT` | `8090` |
| `--bridge` | `VIZRS_LIVE_BRIDGE` | `http://127.0.0.1:8409` |
| `--bridge-token` | `VIZRS_LIVE_BRIDGE_TOKEN` | wymagany (= `VIZRS_BRIDGE_TOKEN`) |
| `--token` | `VIZRS_LIVE_TOKEN` | brak; token awaryjny (min. 16 znaków): skrypty (`Authorization: Bearer`), logowanie awaryjne i kod pierwszego konta. Poza 127.0.0.1 wymagany token albo `--data` |
| `--data KATALOG` | `VIZRS_LIVE_DATA` | brak (w obrazie `/data`, w compose wolumen `live-data`); konta operatorów i sesje (`users.json`, `sessions.json`); bez niego konta są wyłączone, a sesje tylko w pamięci |
| `--stream-url` | `VIZRS_LIVE_STREAM_URL` | brak; adres legacy dla widzów (odnośniki „Strumień”, M3U, XMLTV) |
| `--vizrs-url` | `VIZRS_LIVE_VIZRS_URL` | brak; VizRS CG System **widziany z przeglądarki operatora** (podgląd `/output`, odnośniki do Trio): `http(s)://host:port` albo sam port `:8788` – wtedy host, pod którym otwarto panel (ciasteczko VizRS `SameSite=Strict` działa w ramce tylko w tej samej witrynie; w `docker-compose.live.yml` domyślnie `:${VIZRS_PORT}`) |
| `--vizrs-output-token` | `VIZRS_LIVE_VIZRS_OUTPUT_TOKEN` | brak; token wyjścia VizRS (`VIZRS_OUTPUT_TOKEN`, tylko odczyt – jak w OBS) dla ramki `/output`; **trafia do przeglądarki** (`/api/status`, adres ramki). Bez niego VizRS z tokenem pokazuje w ramce „wymagany token”, chyba że przeglądarka jest zalogowana w Trio. Wymaga `--vizrs-url` |
| `--vizrs-channels` | `VIZRS_LIVE_VIZRS_CHANNELS=1` | wyłączone; VizRS z antenami kanałów (CG Playout `channel_buses` / `VIZRS_CHANNEL_BUSES=1`; w `docker-compose.live.yml` brane stamtąd): podgląd `/output?bus=ch-N`, Trio `/trio?channel=N` |
| `--multiview-max N` | `VIZRS_LIVE_MULTIVIEW_MAX` | 4; najwyżej tyle podglądów w mozaice Na antenie (K10), 0–16, 0 = bez mozaiki (przycisku nie ma). Każdy podgląd to sesja segmentera Next kanału, która koduje – limit chroni maszynę (Ampere A1 2 OCPU: 4 albo mniej); `GET /api/status` podaje `multiviewMax` |
| `--cg-url` | `VIZRS_LIVE_CG_URL` | brak; VizRS CG Playout (sidecar `cg`, np. `http://cg:7700`) – Grafika kanału i Sloty na żywo; tylko `http://` w sieci wewnętrznej |
| `--cg-token` | `VIZRS_LIVE_CG_TOKEN` | wymagany przy `--cg-url`; = `panel_token` (`VIZRS_PANEL_TOKEN`) CG Playout; zostaje w serwerze |
| `--web KATALOG` | – | UI z dysku zamiast wbudowanego |
| `--healthcheck`, `--version`, `--help` | – | |

### API panelu

Serwer panelu udostępnia wewnętrzne API HTTP (`/api/*`) dla interfejsu użytkownika i aplikacji okiennej oraz pośredniczy w komunikacji z mostem VizRS Bridge i CG Playout. Pełny opis wszystkich tras HTTP, kodów odpowiedzi, nagłówka CSRF `X-VizRS-Live` oraz logowania znajduje się w [API.md §2](API.md#2-api-panelu-api).

**Sekrety tylko do zapisu (R8).** W każdej odpowiedzi `/api/cmd/*` serwer panelu zamienia tekstowe wartości pól, których nazwa kończy się na `apikey`, `token`, `password` albo `secret` (bez względu na wielkość liter), na `"********"` (pusty tekst zostaje pusty) – `mask_secrets` w `panel/server/src/secrets.rs`. Dotyczy to też konsoli komend w Diagnostyce. Komend, które zwracają sekret w innym kształcie (`Get{Jellyfin,Emby,Plex}ConnectionParameters` – gotowy nagłówek autoryzacji), nie ma na białej liście.

**Bezpieczeństwo sesji i nagłówki:** Każde żądanie zmieniające (`POST`, `PUT`, `DELETE`) wymaga nagłówka `X-VizRS-Live: 1` (obce strony nie mogą go wysłać bez CORS – ochrona przed CSRF). Gdy panel ma konta (`--data`) albo token awaryjny, `/api/*` (poza `/api/auth`, `/api/login`, `/api/setup`, `/api/logout`) wymaga ciasteczka ważnej sesji albo `Authorization: Bearer <token awaryjny>` (skrypty). Po udanym logowaniu serwer losuje identyfikator sesji (32 bajty z `/dev/urandom`, 64 znaki szesnastkowe – bez crate'ów; obraz `scratch` dostaje `/dev` od środowiska kontenera) i zapisuje w magazynie sesji (najwyżej 256 sesji – przy nadmiarze odpadają wygasłe, potem najdawniej używana; wygasanie po 30 dniach bezczynności, każde żądanie odświeża czas; z katalogiem danych w `sessions.json` – tylko skrót SHA-256 identyfikatora, plik z prawami 0600, odświeżenie czasu użycia zapisywane najwyżej co 10 min). Ciasteczko niesie tylko identyfikator, nie token – wyciek ciasteczka nie ujawnia tokenu, a „Wyloguj” unieważnia tę jedną sesję. Z katalogiem danych sesje przeżywają restart panelu i nowy obraz; bez niego restart = ponowne logowanie w każdej przeglądarce. **Każda** odpowiedź (też API i UI z `--web`) ma CSP (`http::csp()`: bez skryptów i stylów inline, `img-src 'self' data:` plus origin `--stream-url` (okładki i logo legacy `/artwork`), `frame-src` tylko origin `--vizrs-url` (ramka `/output`; przy samym porcie `:N` – `http://*:N https://*:N`, bo host to adres, pod którym otwarto panel), bez tych opcji `frame-src 'none'`; `media-src 'self' blob:` – odtwarzacz MSE podglądu, `object-src 'none'`, `base-uri 'none'`, `form-action 'self'`, `frame-ancestors 'none'`), `X-Frame-Options: DENY`, `X-Content-Type-Options: nosniff`, `Cache-Control: no-store`. Logowanie: najwyżej 10 nieudanych prób w oknie 60 s na adres IP klienta (bezpiecznik łączny: 100/60 s dla wszystkich), potem 429 do końca okna – także dla dobrego tokenu; każda nieudana próba czeka 400 ms i trafia do dziennika (`login: zły token (<adres IP>)`, `login: złe hasło albo login <login> (<adres IP>)`). Ciasteczko dostaje `Secure`, gdy panel stoi za proxy TLS (`X-Forwarded-Proto: https`).

**Konta operatorów (logowanie loginem i hasłem).** Z katalogiem danych (`--data`, w obrazie `/data` na wolumenie `live-data`) operatorzy logują się kontem zamiast wspólnego tokenu, a konta zakłada się w panelu – bez edycji plików i zmiennych. Pliki (`accounts.rs`): `users.json` – login, rola (`admin` / `operator`), czas założenia i skrót hasła PBKDF2-HMAC-SHA256 (sól 16 bajtów, 310 000 iteracji – liczba zapisana przy haśle, więc późniejsza zmiana nie unieważnia starych haseł; SHA-256 i PBKDF2 napisane w panelu bez crate'ów, `crypto.rs`, testy na wektorach FIPS 180-4, RFC 4231 i RFC 7914) oraz `sessions.json`. Zapis atomowy (plik tymczasowy + `rename`), prawa 0600; najpierw plik, potem pamięć – nieudany zapis nie zmienia kont. **Pierwsze uruchomienie:** bez kont panel przy starcie wypisuje w logu kontenera `kod pierwszej konfiguracji: XXXX-XXXX-XXXX` (12 znaków bez mylących 0/O i 1/I, nowy przy każdym starcie, ważny do założenia pierwszego konta); przeglądarka pokazuje okno „pierwsze uruchomienie” (kod, login, hasło) – zamiast kodu można wpisać token awaryjny, jeśli jest. Zły kod liczy się do limitu logowań. **Role:** administrator – wszystko, w tym ekran Użytkownicy (`/api/users`); operator – wszystko poza kontami. Jedynemu administratorowi nie da się odebrać roli ani usunąć konta. Zmiana roli działa od razu w otwartych sesjach, usunięcie konta kończy jego sesje. Nieznany login liczy skrót tak samo długo jak złe hasło (bez wycieku istnienia konta po czasie odpowiedzi); skrót liczy się poza blokadą magazynu. **Token awaryjny** (`--token`) zostaje: skrypty (`Bearer`), logowanie przyciskiem „Token awaryjny” w oknie logowania (sesja = administrator bez konta) i odzyskanie dostępu, gdy nikt nie pamięta hasła administratora. **Plik kont z błędem** (albo katalog tylko do odczytu) wyłącza konta: panel działa, loguje się tylko tokenem, powód jest w logu (`KONTA WYŁĄCZONE`) i w oknie logowania; pliku z błędem panel nie nadpisuje. **Dziennik CG Playout:** podpis operacji z panelu to konto i stanowisko z paska górnego – `VizRS Live · jan (Studio 1)`; bez konta jak dotąd `VizRS Live · stanowisko`. Natywny klient panelu (AppImage) łączy się dalej tokenem awaryjnym (pośrednik dokleja `Bearer`).

**Logowanie w pakiecie (opcja).** Przełącznik na ekranie Użytkownicy („Konta panelu logują też do VizRS Trio i Artist”, `settings.json`, domyślnie wyłączony) pozwala VizRS CG System w tym stosie przyjmować konta panelu – jedno konto w całym pakiecie. Serwer `vizrs` dostaje adres panelu z compose (`VIZRS_PANEL_LOGIN=http://live:8090` w nakładce `docker-compose.live.yml` – nikt go nie wpisuje) i korzysta z `/api/sso…` ([API.md §2](API.md#2-api-panelu-api)): login i hasło z ekranu logowania Trio sprawdza panel i zakłada sesję konta, którą CG System sprawdza co 15 s; zalogowany w panelu pod tym samym hostem wchodzi do Trio bez ponownego logowania (ciasteczko `vizrs_live` jedzie też na port VizRS). Wyłączenie przełącznika, usunięcie konta albo wylogowanie jego sesji (Użytkownicy → Wyloguj) kończy sesję w Trio w ciągu 15 s; panel nieosiągalny Trio nie wylogowuje. Trio uruchomione samo (np. dla OBS, bez panelu) loguje się swoim tokenem. Opis po stronie Trio: VizRS CG System DOKUMENTACJA §12.

**Alarmy (K17, 2026-11-09).** Na każdym ekranie, pod paskiem górnym, banner „ALARM” (`panel/server/web/alarms.js`, co 60 s, tylko odczyt): przerwa w ramówce teraz, dziura w ciągu 15 min, playout kończący się w ciągu 15 min (pozycje z `GetFuturePlayoutItemsById`, 30 na kanał), slot LIVE za ≤ 5 min bez sygnału studia, emisja zatrzymana (kanał nadawany przez CG Playout bez sesji legacy – ostrzeżenie), cała grafika zdjęta > 5 min (`graphics_off_since` z CG albo czas pierwszego zauważenia w panelu), YouTube zakończył sesję ciasteczek (`ended_at` z `/api/live/youtube-cookies`, gdy CG Playout ma cechę `youtube_cookies`; od 26.11.25). Klik w alarm prowadzi do ekranu; „Dźwięk: wł./wył.” (krótki sygnał WebAudio przy nowym alarmie, wybór pamiętany w przeglądarce – przeglądarka wymaga kliknięcia, zanim zagra), „Ukryj na 10 min” (nowy błąd i tak pokaże banner). Webhook poza przeglądarką – w CG Playout ([API.md §1](API.md#1-live-api-sidecara-apilive-etv-ffmpeg-ingest-health)).

### UI (`panel/server/web/`)

| Plik | Zawartość |
|---|---|
| `desktop.js` | most panelu z aplikacją okienną (`panel/desktop/`): tytuł okna i alarm, bez skrótów przeglądarki; w przeglądarce nic nie robi (niżej „Aplikacja okienna panelu”) |
| `app.js` | wejście: stan mostu w pasku górnym, licznik playoutów z błędem budowy (`GetPlayoutWarningsCount`, co 15 s; odnośnik do Playoutów), metadane komend (`GET /api/commands`), start routera; **liczniki w menu** (Kanały, Profile, Znaki, Biblioteki, Kolekcje – suma czterech rodzajów, Playlisty, Wypełniacze, Harmonogramy, Bloki, Playouty) z komend listujących – wszystkie po starcie, potem licznik ekranu po każdym jego przerysowaniu (wejście, zapis); bez komendy na białej liście licznika nie ma; **wybór kanału w pasku górnym** (`GetAllChannels`, pamiętany w `localStorage`, `S.channel`, `onChannel()` w `core.js`): filtruje Na antenie (karty, najbliższe pozycje, anteny VizRS), EPG, listę Playoutów i Sloty na żywo (karty kanałów, ramówka), a w Grafice kanału ustawia domyślny kanał stron VizRS; „Wszystkie kanały” = bez filtra; kanał, którego już nie ma, = wszystkie |
| `core.js` | DOM (`h`, `table`, `panel`…), `api`/`cmd`/`argsFor`, `upload` (obraz do legacy), `legacyUrl` (adres pliku legacy przez `--stream-url`), toast, dialog, router (`VIEWS`, `route`), etykiety `PL`, kontrola białej listy (`missing`, `missingNote`, `needs`) |
| `player.js`, `mux.js` | **podgląd strumienia kanału**: okno w prawym dolnym rogu (`openMonitor`; przycisk „Podgląd” w pasku górnym, „Podgląd ▶” na kartach Na antenie, „Odtwórz w panelu” w Diagnostyce), zostaje przy zmianie ekranów; wybór kanału (ostatni pamiętany w `localStorage`), dźwięk (domyślnie wyłączony), „Większy”, pełny ekran (w przeglądarce – pełny ekran wideo; w oknie aplikacji – podgląd wypełnia okno, a okno przechodzi w pełny ekran przez most okna `{t:'fullscreen'}`, Esc albo „Wyjdź” wraca: WebKitGTK z paczki AppImage, budowanej na Ubuntu 22.04, przerywa proces przy `requestFullscreen()` na `<video>`, a systemowy WebKitGTK 2.52 nie – sprawdzone 2026-11-26). `mux.js` przepakowuje segmenty MPEG-TS (H.264 + AAC ADTS) na fragmenty MP4 dla Media Source Extensions – bez dekodowania i kodowania. **HEVC i AC-3 (K18, 2026-11-09):** gdy przeglądarka dekoduje je w MSE (`MediaSource.isTypeSupported('video/mp4; codecs="hvc1…"')` / `'audio/mp4; codecs="ac-3"'` – Safari, Chrome/Edge ze sprzętowym HEVC; Chrome i Firefox bez AC-3), `mux.js` przepakowuje też HEVC (VPS/SPS/PPS → `hvcC`, nazwa kodeka wg ISO/IEC 14496-15, np. `hvc1.1.6.L93.B0`, klatki kluczowe BLA/IDR/CRA, Main i Main 10) i AC-3 (ramki z nagłówka 0B77 → `dac3`, 1536 próbek na ramkę, do 5.1); sprawdzone ffprobe/ffmpeg na próbkach z libx265 i ac3. E-AC-3 i AV1 – nie (E-AC-3 w MSE prawie nigdzie, AV1 w TS niestandardowy); bez obsługi HEVC zostaje natywny HLS przeglądarki, bez AC-3 – obraz bez dźwięku z opisem w stopce podglądu. **Stałe opóźnienie za ramówką (zwykle 3–5 s):** segmenty mają `PROGRAM-DATE-TIME`, ale kanał z nakładką grafiki CG Playout powstaje w tempie zegara (wyprzedza go najwyżej o `LEAD_SECONDS`, domyślnie 4 s), więc najnowszy segment kończy się ok. „teraz”; podgląd grający dokładnie „teraz” stawał co segment (pomiar 2026-11-11: 29 przestojów na minutę). Odtwarzacz mierzy przy każdym odczycie playlisty, o ile koniec najnowszego segmentu jest za zegarem serwera (`X-VizRS-Now`), i gra o najgorszy wynik z ostatniej minuty + 2 s za zegarem anteny (1–30 s; segment bez własnego PDT dostaje czas poprzedniego + jego długość). Zaczyna od segmentu, w którym wypada ten czas, i trzyma go (różnica < 2 s – tempo 0,94–1,06, większa – skok); stopka pokazuje „N s za anteną”. Bufor do 8 s naprzód. Bez `PROGRAM-DATE-TIME` – krawędź na żywo playlisty. Bufor za obrazem do ok. 30 s (starsze dane usuwane), karta w tle nic nie pobiera (po powrocie start od nowa), błędy – do 5 ponowień co 3 s. HEVC/AV1 albo brak MSE: natywny HLS przeglądarki (Safari, nowy Chrome; opóźnienie zależne od przeglądarki), inaczej komunikat. Dźwięk AC-3/LATM: obraz bez dźwięku |
| `form.js` | **formularz z metadanych**: rodzaj kontrolki z typu parametru komendy (tekst, liczba, tak/nie, wyliczenie z wartościami z mostu, `TimeSpan` gg:mm:ss, data/godzina, wybór id z listy, lista id – do 6 pozycji pola tak/nie, przy dłuższej liście wyszukiwarka: wybrane jako etykiety z „×”, lista podpowiedzi zawężana pisaniem (bez polskich znaków też trafia), klawiatura ↑/↓, Enter, Esc, Backspace; pozycje mogą mieć miniaturę – obraz znaku wodnego (napis „logo” dla logo kanału), obraz strony VizRS dla elementu `page-<nr>.yml` (pośrednik `live/thumbs`, bez CG Playout albo miniatury – napis „VizRS n”), inny element graficzny – rodzaj z katalogu pliku; najechanie wskaźnikiem na obraz miniatury pokazuje jego powiększenie w pływającym okienku, bez przechwytywania kliknięć – tylko urządzenia ze wskaźnikiem); opis pola daje etykietę PL, sekcję, podpowiedź (tekst albo funkcja wartości), pole warunkowe (`show`; ukryta grupa ukrywa swoje pola), kontrolkę własną (`render`); `read()` zbiera wartości i opisuje złe pole |
| `parts.js` | `crudView` – ekran „lista + edytor” (+ Nowy, Zapisz, Usuń z potwierdzeniem, Kopiuj; `createCmd` – komenda zakładania zależna od danych); `listView` – lista tylko do odczytu; `itemsEditor` – lista pozycji (w górę/w dół, dodaj, usuń, edycja wiersza w panelu bocznym; poniżej 760 px wiersz to karta „etykieta: wartość” – komórki z `data-label` nagłówka, klasa `.tw.stack` – zamiast tabeli szerszej niż ekran, np. pozycje harmonogramu na telefonie); `contentPicker` – wybór treści; `groupActions` – „+ Grupa” / „Usuń grupę…”; `dayBar` – oś doby 24 h z segmentami |
| `when.js` | „kiedy obowiązuje”: dni tygodnia, dni miesiąca, miesiące, zakres dat (`whenEditor`), dopasowanie dnia jak w legacy (`matchesDate`, `pickFor`) – wspólne dla harmonogramów zastępczych i kalendarza playoutu blokowego |
| `clip.js` | **odtwarzanie pozycji w karcie Przeglądarki**: `playSection` – domyślnie plik wprost (`<video>` z `/api/vod/…`, bez legacy i transkodowania, pełne przewijanie; `directSupport` sprawdza kodek obrazu i dźwięku z `GetMediaItemInfo` – HEVC tylko tam, gdzie przeglądarka go dekoduje, kontenery avi/ts/mpg – nie), przy niewspieranym kodeku albo błędzie odtwarzacza – test odtwarzania legacy; przyciski „Przez legacy (transkodowanie)” / „‹ Wprost z pliku”; zapasowo `ClipPlayer` – `POST /api/play/…` co 30 s (następny fragment, gdy w buforze zostaje < 12 s), segmenty TS z `/api/stream/session/.troubleshooting/` przez `Transmuxer` (`mux.js`) do jednego `MediaSource`; kolejne fragmenty jako nieciągłość jednej osi, `timestampOffset` = sekunda startu, więc `currentTime` = miejsce w materiale; `clipSection` – „▶ Odtwórz od g:mm:ss”, „‹ 30 s” / „30 s ›”, „■”, profil kodowania (pamiętany w `localStorage` `vizrs-live-clip-profile`) |
| `cmdedit.js` | **edytor komend VizRS w regułach grafiki** (Grafika kanału): tekst komend (`on_clip`, `on_end`, `on_ad`) jako lista kroków – akcja (Wprowadź `TAKE`, Dalej `CONTINUE`, Zdejmij `OUT`, Podmień dane `UPDATE`), strony z show VizRS (lista z `/etv/pages` wszystkich kanałów CG Playout; strona spoza show – czerwona pigułka), pola strony `nazwa = wartość` z listą symboli ramówki (`{title}`, `{next_title}`, `{next_start}`…; nazwy pól podpowiada `fields` strony, gdy CG Playout je poda, inaczej pola użyte w bieżących regułach). Zapis w tej samej składni co `config.json` CG Playout (`k=v\|k=v`, JSON pól, gdy wartość ma `\|` albo koniec linii albo tak było w źródle); linia, której edytor nie rozumie (komentarz `#`, `@N`, inna komenda), zostaje krokiem „Komenda tekstowa” bez zmian. Tryb pola: „jak globalnie” (tylko reguła audycji/kanału), „wyłączone” / „tylko zdejmij grafikę” (przerwa), „grafiki VizRS”; przycisk „Tekst” – te same komendy jako tekst. Krok bez strony zatrzymuje zapis |
| `views/*.js` | ekrany, po jednym pliku na grupę menu (`antena`, `kanaly`, `media` – biblioteki, przeglądarka, kosz, `zrodla` – Jellyfin, Emby, Plex, strumienie zdalne, `listy`, `grafika`, `system`), duże ekrany we własnym pliku (`harmonogramy`, `playouty`, `kalendarz` – kalendarz tygodnia playoutu blokowego, `bloki` – bloki i szablony dnia, `deco` – deco i szablony deco, `profile`, `znaki` – też pole obrazu `imageField` dla logo kanału, `wypelniacze`); każdy dopisuje się do `VIEWS` |
| `views/uzytkownicy.js` | ekran System → Użytkownicy (konta, tylko administrator) |
| `index.html`, `vizrs-ui.css`, `style.css` | szkielet (pasek górny, menu, okno, toast) i arkusze stylów: `vizrs-ui.css` – **wspólny rdzeń wyglądu rodziny VizRS** (tokeny, przyciski, pola, pigułki, pasek górny, menu i szuflada, panele, tabele, formularze, okna, karty); źródło w VizRS CG System (`source/web/vizrs-ui.css`), tu identyczna kopia – `panel/scripts/sync-ui.sh` kopiuje i zapisuje wersję źródła w `panel/server/web/CG_SYSTEM`, `panel/scripts/sync-ui.sh --check` (CI, `panel/tests/deploy.sh`) pilnuje zgodności; nie edytuj kopii. `style.css` (ładowany po rdzeniu) – tylko style własne panelu. Kolory, promienie i cienie tylko przez zmienne z `:root` (tokeny: powierzchnie `--bg…--panel3`, linie, akcent, stany `--ok/bad/warn/info-bg`). Ikony menu – maski SVG w `data:` (dozwolone przez CSP `img-src data:`), przypisane po `href` odnośnika. Przycisk ☰ (`#nav-toggle`, `app.js`): na szerokim ekranie zwija menu do paska ikon (klasa `nav-min` na `.app`, pamiętane w `localStorage` `vizrs-live-nav-min`; nazwa ekranu w podpowiedzi), poniżej 760 px otwiera menu jako szufladę (`nav-open`; zamyka ją wybór ekranu, klik obok i Esc). `prefers-reduced-motion` wyłącza animacje |

Nowy plik w `panel/server/web/` trzeba dopisać do `STATIC` w `panel/server/src/routes.rs`; test `module_imports_resolve` pilnuje, że każdy `import` wskazuje plik ze `STATIC`.

**Brakujące komendy.** Ekran (albo rodzaj treści w wyborze treści) sprawdza, czy jego komendy są w `GET /api/commands`; jeśli nie – pokazuje „ta funkcja wymaga komendy X na białej liście mostu” zamiast pustego ekranu albo błędu 403.

**Wybór treści** (`contentPicker`) daje pola treści pozycji legacy (`CollectionType` + `CollectionId` / `SmartCollectionId` / `MultiCollectionId` / `PlaylistId` / `MediaItemId` / `RerunCollectionId` albo `SearchTitle` + `SearchQuery`). Wyszukiwanie: `SearchCollections`, `SearchSmartCollections`, `SearchMultiCollections`, `SearchTelevisionShows`, `SearchTelevisionSeasons`, `SearchArtists` (legacy szuka `LIKE %tekst%`, najwyżej 10 wyników; pusty tekst = pierwsze 10), playlisty przez `GetAllPlaylistGroups` + `GetPlaylistsByPlaylistGroupId` (filtr po nazwie w przeglądarce). Powtórki wymagają `SearchRerunCollections` (dojdzie w R4).

### Aplikacja okienna panelu (`panel/desktop/`, Tauri 2)

**Natywna aplikacja sterowania: klient panelu działającego na serwerze** (w stosie z ErsatzTV legacy i mostem) – ten sam panel w oknie programu (Linux – WebKitGTK, Windows – WebView2, macOS – WKWebView); panel w przeglądarce działa bez zmian. Okno niczego nie uruchamia lokalnie (panel bez ErsatzTV i mostu nie ma czym sterować; do 26.11.10 domyślny „panel lokalny” wywracał się na stacji operatora brakiem tokenu mostu). Osobny workspace `panel/desktop/` z zależnościami (tauri, tauri-plugin-dialog, tauri-plugin-single-instance, serde_json, ureq/rustls, getrandom; Linux – webkit2gtk) – wyjątek od „zero zależności” tylko tam, serwer `vizrs-live` dalej ich nie ma. Wzór: okno Trio VizRS CG System (`source/shell/`) i VizRS CG Artist – bez wspólnego crate'a.

```bash
vizrs-live-desktop                                   # zapisany panel „przy uruchomieniu”, bez niego okno „Połącz z panelem”
vizrs-live-desktop --connect https://live.example.pl --token …               # wskazany panel (też http://serwer:8090)
vizrs-live-desktop --connect https://live.example.pl                         # panel z kontami: logowanie kontem w oknie
vizrs-live-desktop connect                           # najpierw okno „Połącz z panelem”
```

- **Połączenie z panelem:** okno łączy się z panelem na serwerze przez **pośrednika** w programie (`panel/desktop/src/proxy.rs`, `127.0.0.1`, losowy port): każde żądanie strony idzie do panelu z sesją konta (ciasteczko `vizrs_live`) albo z `Authorization: Bearer <token>` (token z `--token`, `VIZRS_LIVE_TOKEN` albo okna połączenia), więc **ani token, ani sesja nie trafiają do strony**. Pośrednik przyjmuje tylko okno programu (sekret z pierwszego adresu → ciasteczko `HttpOnly`), sprawdza `Host` i `Origin`, nie przekazuje ciasteczek strony ani `Set-Cookie` panelu, odpowiedzi (podgląd HLS, odtwarzanie z `Range`) przesyła strumieniem; treść żądania do 64 MB, `http://` i `https://` (rustls).
- **Logowanie kontem (K20, od 26.11.29):** panel z kontami operatorów nie potrzebuje tokenu w oknie. Bez tokenu okno pokazuje ekran logowania panelu (login i hasło, „Token awaryjny”, pierwsze konto kodem z logu) – ten sam co w przeglądarce. `Set-Cookie: vizrs_live=…` z odpowiedzi panelu zostaje w pośredniku: dokleja on sesję do kolejnych żądań (sesja ma pierwszeństwo przed tokenem – inaczej panel widziałby token, nie konto) i zapisuje jej identyfikator – nie hasło – w `live-connections.json` (pole `session`), więc po ponownym uruchomieniu logowanie nie wraca, dopóki sesja żyje w panelu (30 dni bez użycia). „Wyloguj” w panelu, zmiana hasła albo usunięcie konta kasują sesję (panel odpowiada 401 – pośrednik ją zapomina, strona pokazuje logowanie). Wpisany w oknie połączenia token kasuje zapamiętaną sesję tego panelu (logowanie tokenem). Panel bez kont i bez tokenu w oknie – komunikat „Panel wymaga tokenu”.
- **Połącz z panelem** (Plik → Połącz z panelem…, Ctrl+Shift+K): adres i token panelu (opcjonalny przy kontach), sprawdzenie przez `GET /api/status` i – bez tokenu – `GET /api/auth` (zły token, brak panelu, panel bez kont – komunikat), lista zapisanych w `live-connections.json` obok listy serwerów VizRS (`~/.config/vizrs/`, `%APPDATA%\VizRS\`, `~/Library/Application Support/VizRS/`; prawa tylko dla właściciela; token tylko przy „zapamiętaj token”), „łącz z tym panelem przy uruchomieniu”. Panel musi być osiągalny ze stacji operatora (port panelu `8090` albo adres przez proxy TLS / VPN; [RUNBOOK.md](RUNBOOK.md)). Zapisany panel nieosiągalny przy starcie – okno połączenia z komunikatem. Po połączeniu wszystkie okna przechodzą na wybrany panel (tytuł z nazwą hosta).
- **Most strony:** `panel/server/web/desktop.js` (protokół okien VizRS: `window.__vizrsPost('app:' + JSON)` → komenda Tauri `ipc`) zgłasza tytuł okna (ekran, `● ALARM` przy alarmie K17) i alarm – okno w tle prosi wtedy system o uwagę; wyłącza też skróty przeglądarki (F3, F7, F12, Ctrl+F/G/H/J/P/R). Aplikacja przyjmuje tylko `{t:'state', title ≤ 200 znaków, alert}` i `{t:'fullscreen', on}` – pełny ekran własnego okna dla podglądu kanału (od 26.11.33) (`panel/desktop/src/bridge.rs`) i tylko od strony bieżącego panelu (uprawnienie dodawane w działaniu dla jego adresu); okno połączenia – strona wbudowana w program (`panel/desktop/ui/connect.html`). Własny moduł zamiast `native.js` CG System, bo tamten buduje okno przez `innerHTML` i style w wierszu (CSP panelu).
- **Menu:** Plik – Połącz z panelem…, Nowe okno (Ctrl+Shift+N), Zamknij okno (Ctrl+W), Zakończ (Ctrl+Q; Windows Alt+F4); Widok – Odśwież (F5), Pełny ekran (F11), Powiększ / Pomniejsz / 100%, Narzędzia deweloperskie; Pomoc – Sprawdź aktualizacje, O programie (macOS: menu programu, Edycja, Okno). Odnośniki spoza panelu (Trio VizRS, dokumentacja) otwiera przeglądarka systemowa. Jedna instancja (wtyczka Tauri) – kolejne uruchomienie wysuwa okno (`connect` – okno połączenia).
- **Akceleracja sprzętowa (Linux, WebKitGTK)** – `panel/desktop/src/gpu.rs`, ten sam plik w VizRS CG Artist, VizRS CG System (`source/shell`) i panelu (`panel/desktop/`): przy starcie, przed GTK, okno ustawia zmienne WebKitGTK. Sterownik własnościowy **NVIDIA** (węzeł `renderD*` sterownika `nvidia`, `nvidia-drm.modeset` nie „N”): `WEBKIT_FORCE_DMABUF_RENDERER=1` i `__NV_DISABLE_EXPLICIT_SYNC=1` – bez tego WebKitGTK sam wyłącza akcelerację („Policy: never” w `webkit://gpu`) i rysuje na CPU (pomiar 2026-11-13, Quadro P400, sterownik 580, KDE Wayland: animacja 5 → 61 kl./s; samo wymuszenie DMA-BUF na Waylandzie kończy się błędem protokołu 71). Przy NVIDIA także `WEBKIT_SKIA_ENABLE_CPU_RENDERING=1`: warstwy rasteryzuje Skia na CPU, składa je GPU – rasteryzacja Skia na GPU wywracała proces strony w `libnvidia-eglcore` (zgłoszenie 2026-11-14, sterownik 580, WebKitGTK 2.52: pusta albo nieaktualna treść okna po kilku sekundach lub przy zmianie rozmiaru; pomiar: 3 awarie w 3 uruchomieniach → 0, animacja dalej 61–62 kl./s). Gdy proces strony mimo to padnie, okno wczytuje stronę od nowa (najwyżej 3 razy na minutę; komunikat `proces strony zakończył się …` na wyjściu błędów). Inne karty (Mesa: Intel, AMD, nouveau) i maszyny bez GPU – wybór zostaje przy WebKitGTK. Zmienne `WEBKIT_*` ustawione przez użytkownika mają pierwszeństwo. `VIZRS_GPU`: `auto` (domyślnie; `on` – to samo), `off` – bez akceleracji (zepsuty sterownik), `x11` – okno przez XWayland (`GDK_BACKEND=x11`) z akceleracją, `wayland` – bez przełączania na XWayland przy skali ułamkowej, `system` – bez zmian. Decyzja trafia na standardowe wyjście błędów (`GPU: …`). Windows (WebView2) i macOS (WKWebView) – akceleracja systemu, bez zmian.
- **Skala ułamkowa (KDE Plasma Wayland)** – `panel/desktop/src/scale.rs` (ten sam plik w VizRS CG Artist, VizRS CG System i panelu): GTK 3 / WebKitGTK 4.1 nie zna skalowania ułamkowego Waylanda – przy 125 % zaokrągla do 2, rysuje stronę w buforze ×2, a kompozytor zmniejsza ją ×0,625 (nierówne cienkie linie, 2,56× więcej pikseli; pomiar 2026-11-14, 1920×1080 przy 125 %, Quadro P400: animacja 5 kl./s, przez XWayland 18). W trybie `auto` sesja Wayland z niecałkowitą skalą programów X11 w `kwinrc` (`[Xwayland] Scale`, np. 1.25) otwiera okno przez XWayland: natywne piksele, interfejs tej samej wielkości (WebKitGTK powiększa stronę o `Xft.dpi / 96`). Wyłączenie: `VIZRS_GPU=wayland` albo `GDK_BACKEND=wayland`; innych pulpitów okno nie przełącza.
- **Rozmiar okna a skalowanie ekranu** – `panel/desktop/src/screen.rs` (ten sam plik w VizRS CG Artist, VizRS CG System i panelu): okno otwierane bez maksymalizacji dostaje najwyżej 92 % szerokości i 90 % wysokości obszaru roboczego monitora w pikselach logicznych – przy skalowaniu ułamkowym (1920×1080 przy 125 % = 1536×864) domyślne 1600×900 albo rozmiar zapisany na innym ekranie nie wychodzi poza krawędź.
- **Budowa i paczki:** `cd panel/desktop && cargo build --release` (Linux do budowy: `libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libxdo-dev`). Paczki natywnego klienta (`.deb`, `.rpm`, AppImage, `.msi`/NSIS, `.dmg`): `cd panel/desktop && npx @tauri-apps/cli@2 build`. AppImage do wydania (`VizRS-CG-Playout-<wersja>-x86_64.AppImage`) buduje akcja Release na Ubuntu 22.04 (zadanie `appimage`).
- **Test w działaniu:** `node panel/desktop/tests/okno.cjs` (otwiera okna; atrapa mostu `panel/tests/mock-bridge.cjs`): start bez zapisanego panelu (okno połączenia, bez uruchamiania `vizrs-live`), dawny `local` odrzucony, panel z tokenem przez pośrednika (strona bez tokenu), okno połączenia ze złym i dobrym tokenem, panel bez kont bez tokenu, logowanie kontem na stronie (sesja w pośredniku, na liście zapisanych bez hasła, po ponownym uruchomieniu bez logowania, po wylogowaniu – znów logowanie); CI – zadanie `desktop` (Linux pod Xvfb; Windows i macOS – kompilacja i testy jednostkowe).


## 12. Ekrany panelu

| Ekran | Komendy legacy | Prototyp 0.1 |
|---|---|---|
| Przewodnik EPG | `GetAllChannels`, `GetPagedPlayouts`, `GetFuturePlayoutItemsById`; CG Playout `GET /api/live/state` (`guide`) | wszystkie włączone kanały w wierszach (bez playoutu – „brak playoutu” z odnośnikiem), linijka godzin, linia „teraz”, bieżąca pozycja wyróżniona, wypełniacze przygaszone; okno 4 h: „teraz”, „4 h →”, „← 4 h” (wstecz tylko do „teraz” – legacy podaje tylko przyszłe pozycje), data i godziny okna w nagłówku; pozycje krótsze niż 1,5 % okna (3,6 min) sklejone w bloki „pierwsza +N” do 30 min, pełna lista w podpowiedzi; audycje LIVE (pozycja legacy, której start ±90 s zgadza się z `live` w ramówce CG) – czerwona ramka i „●”; **kontrola ramówki** (K9, niżej); **karta pozycji** (klik w pozycję, nie w blok i nie w wypełniacz; ta sama karta z tabeli „najbliższe pozycje” w Playoutach): tytuł, kanał, godziny, zdarzenia wtórne pasujące do pozycji na osi o jej prawdziwej długości (K15; wzorce `show_title` i `title` panel sprawdza na tytule pozycji legacy – CG Playout porównuje `show_title` z tytułem audycji, opis w karcie; reguły po ścieżce pliku pomija z liczbą – legacy nie podaje ścieżki pozycji), „Zdarzenie: tylko ta pozycja” (nowa reguła z kanałem i `target.start` pozycji) i „Zdarzenie: każda emisja tytułu” (kanał i `^tytuł$`) na ekranie Grafika kanału |
| Na antenie | `GetAllChannels`, `GetPagedPlayouts`, `GetFuturePlayoutItemsById`, `BuildPlayout`; most: sesje (`/api/sessions`, `/api/session/…`); CG Playout `GET /api/live/state`, `POST /api/live/graphics/off\|on` | układ wg makiety UI (2026-11-08): w nagłówku licznik „N/M kanałów w ramówce” (czerwony, gdy wszystkie mają bieżącą pozycję), **przełącznik całej grafiki** (2026-11-09, przy podłączonym CG Playout: pigułka „grafika na antenie” / „GRAFIKA ZDJĘTA” ze stanu `settings.graphics_off` i przycisk „Zdejmij grafikę” – z potwierdzeniem – albo „Przywróć grafikę”; „Zdejmij” wyzwala w VizRS animacje wyjścia (`OUT` warstw na antenie każdego kanału), a po nich CG Playout wysyła przezroczyste klatki; „Przywróć” wprowadza te same strony z animacją wejścia (`TAKE`), z uwzględnieniem zmian ramówki w tym czasie – szczegóły w docs/API.md §1; stan przetrwa restart CG; zdjęta grafika – czerwona uwaga) i „Otwórz Trio ↗” (antena główna; przy antenach kanałów Trio jest na kartach); **emisja kanału** (2026-11-09): na karcie pigułka „emisja: <stan sesji legacy>” albo „emisja zatrzymana” i przyciski „Uruchom emisję” (gdy sesji nie ma), „Restart emisji” i „Zatrzymaj emisję” (oba z potwierdzeniem; docs/API.md §3 `/vizrs-bridge/session/…`); most bez `/sessions` (wydania do 1.2.4) – pigułki brak, uwaga „stan sesji niedostępny”; CG Playout bez `graphics_off` w stanie (starszy niż 2026-11-09) – przełącznika grafiki brak; karta kanału: stan w nagłówku (NA ANTENIE – jest bieżąca pozycja, ZASTĘPCZY – przerwa w ramówce, leci wypełniacz zapasowy, BRAK PLAYOUTU), **ramka VizRS `/output` na antenie kanału** z plakietką „NA ŻYWO”, gdy studio jest na antenie (`on_air` z CG; przy `--vizrs-channels` antena kanału; bez anten kanałów główna antena – w karcie, gdy na ekranie jest jedna karta, przy wielu kartach jedna wspólna ramka z wyborem po prawej), teraz (pigułka „wypełniacz” przy pozycji wypełniacza) / następnie z postępem, **wiersz stanu studia z CG** (studio NA ANTENIE, sygnał studia, studio wstrzymane, studio także poza audycjami, najbliższy slot LIVE z godziną – w makiecie były tu warstwy grafiki, ale CG Playout ich nie podaje), przebudowa i reset playoutu; **„Najbliższe zdarzenia”** – godzina z sekundami, pozycje ramówki wszystkich kanałów (dopisek „wypełniacz”) i najbliższe audycje LIVE z CG (`next_live`, pigułka LIVE); **„Uwagi”** – kanał bez playoutu, błąd budowy playoutu (komunikat legacy), przerwa w ramówce (leci wypełniacz zapasowy), CG Playout nie odpowiada, studio wstrzymane, slot na żywo w ciągu godziny bez sygnału studia – z odnośnikiem do Playoutów albo Slotów. Odświeżanie co 30 s podmienia tylko treść kart, list i uwag (karty i ramki VizRS zostają – bez przeładowania grafiki); inny zestaw kanałów = przerysowanie ekranu. **„Podgląd ▶”** – strumień kanału w oknie podglądu (`/api/stream/…`, docs/API.md §2); **kontrola ramówki** (K9, niżej) |
| Kanały | `GetAllChannels`, `CreateChannel`, `UpdateChannel`, `DeleteChannel`, `SaveArtworkToDisk` (logo, przez `/api/upload/Logo`), `GetAllChannelsForSort`, `UpdateChannelNumbers` (+ profile, znaki, wypełniacze do list wyboru) | **pełna edycja** (wzorzec „lista + edytor”, formularz z metadanych; „Kanał lustrzany” tylko dla źródła Mirror); logo: wgrany obraz albo adres URL, z podglądem; **„Numeracja…”** (K3, odpowiednik Edit Channel Numbers legacy): okno z tabelą numer – kanał – poprzedni numer; panel przed wysłaniem odrzuca zły numer (liczba, opcjonalnie z kropką: `12`, `12.1` – legacy liczy z niego `SortNumber`) i duplikaty, potem wysyła jednym `UpdateChannelNumbers` tylko zmienione kanały (`{Id, Number, Name, OriginalNumber}`); legacy nadaje numery w jednej transakcji (najpierw tymczasowe `-Id`), więc zamiana numerów miejscami działa; odświeża listę kanałów i XMLTV sam (zadania w tle) |
| Profile kodowania | `GetAllFFmpegProfiles`, `NewFFmpegProfile`, `CreateFFmpegProfile`, `UpdateFFmpegProfile`, `CopyFFmpegProfile`, `DeleteFFmpegProfile`, `GetAllResolutions`, `GetSupportedHardwareAccelerationKinds`, `CreateCustomResolution`, `DeleteCustomResolution`, `GetNextProbe` (+ kanały) | **pełna edycja**: rozdzielczość, obraz, akceleracja z informacją o sprzęcie, dźwięk z normalizacją głośności (−23 LUFS), nowy z wartościami domyślnymi legacy, kopiuj, usuń (z listą kanałów); **akceleracja wg silnika Next** (K5, legacy ≥ 26.12.0): podpowiedź „działa / nie działa i dlaczego” – ta sama logika co `NextHardware.Check` w edytorze profilu legacy (akceleracja → nazwa Next: NVENC = `cuda`; VAAPI – urządzenie i sterownik muszą pasować do sprawdzonego wpisu; kodowanie sprzętowe = wpis `Available` z kodekiem i głębią profilu), z kodekami dekodowania i kodowania sprzętowego oraz innymi działającymi wpisami; bez `GetNextProbe` (starsze legacy) – dawna podpowiedź z listy ffmpeg; **„Rozdzielczości”** pod listą (K4): wbudowane i własne, liczba profili, „+ Dodaj rozdzielczość” (szerokość × wysokość, panel sprawdza liczby parzyste 16–7680 × 16–4320 i duplikat; legacy nadaje nazwę `SZERxWYS`), „Usuń” tylko przy własnych (`IsCustom`); **„Klatkaż”** (most `/api/framerates`, §10) zamiast przełącznika „stała liczba klatek”: „jak materiał” (`NormalizeFramerate` = nie), „automatycznie – najniższy z ramówki” (wyliczenie legacy), albo stały 25, 50, 23,976, 24, 29,97, 30, 59,94, 60 kl./s (ustawienie mostu + `NormalizeFramerate` = tak); zapis po zapisie profilu w legacy, usunięcie profilu kasuje jego ustawienie, kopia profilu go nie przenosi; kolumna „Klatki” na liście; most bez filtra (starszy albo `problem`) – zostaje przełącznik legacy z uwagą; **bez akceleracji sprzętowej** (sonda Next: żadna akceleracja nie koduje; starsze legacy – lista ffmpeg bez sprzętu): nowy profil startuje z „brak (programowo)”, akceleracje na liście z dopiskiem „brak na tej maszynie (programowo)”, w kolumnie „Akceleracja” ostrzeżenie „… – brak, programowo” przy profilach, które i tak kodują na procesorze; panel kodera mówi, że obowiązuje libx264 (domyślny profil „x264: high”, podpowiedź presetu), a presety NVENC/QSV i klatki B zwija w „Ustawienia koderów sprzętowych” – zapis kodera zawsze odsyła je bez zmian (PUT zastępuje wszystkie pola); **„Koder (nakładka VizRS CG)”** pod listą (CG Playout `GET/PUT /api/live/encoder`, 2026-11-12): pola z wartościami w użyciu i źródłem w nagłówku („ustawione w panelu”, „z .env serwera” – plik jeszcze nie istnieje, „domyślne”): **profil H.264** dla wszystkich koderów H.264 – libx264, NVENC, QSV, VAAPI (baseline, main, high; puste = domyślny kodera: x264 `high`, NVENC `main`); **programowo**: preset x264 (ultrafast…placebo, z uwagami z pomiarów A1; puste = ErsatzTV `medium`) i opcje x264 – zmierzone warianty `subme=0`, `bframes=0`, `bframes=0:subme=0` z kosztem CPU i SSIM, „bez opcji” albo „własne…” (białą listę kluczy sprawdza CG); **sprzętowo**: preset NVENC (p1–p7, puste = p4), preset QSV (veryfast…veryslow), klatki B koderów NVENC/QSV/VAAPI (0–4, z pamięcią karty przy 0 i 1). Który zestaw działa, wynika z akceleracji w profilu kanału; VAAPI nie ma presetu. Zapis wysyła wszystkie pola; ustawienia panelu zastępują `.env` serwera (`VIZRS_FFMPEG_PRESET`, `_X264_PARAMS`, `_NVENC_BF`), który wypełnia je tylko przy pierwszym starcie CG. Silnik Next pomija preset i profil z profilu kodowania (także przy akceleracji), więc dokłada je nakładka `vizrs-ffmpeg` (DOKUMENTACJA §1 „Koder z panelu”); działa od następnego startu ffmpeg. Bez CG Playout panelu nie ma, starszy CG – uwaga, że zostaje .env (CG bez pól sprzętowych – tylko pola programowe); CG bez `etv_config` – wartości tylko do odczytu |
| Znaki wodne | `GetAllWatermarks`, `CreateWatermark`, `UpdateWatermark`, `CopyWatermark`, `DeleteWatermark`, `SaveArtworkToDisk` (przez `/api/upload/Watermark`) (+ kanały) | **pełna edycja**: wgrany obraz albo logo kanału, tryb (stały, przerywany, wyrażenie krycia), 9 położeń, rozmiar, marginesy, krycie, warstwa; podgląd na makiecie kadru 16:9; miniatury na liście |
| Wypełniacze | `GetAllFillerPresets`, `CreateFillerPreset`, `UpdateFillerPreset`, `DeleteFillerPreset` (+ kolekcje do nazw treści) | **pełna edycja**: miejsce, tryb (czas, liczba, losowa liczba, dopełnienie do pełnych minut), treść (wybór treści), znaki wodne, wyrażenie przerw, rozdziały |
| Harmonogramy | `GetAllProgramSchedules`, `CreateProgramSchedule`, `UpdateProgramSchedule`, `CopyProgramSchedule`, `DeleteProgramSchedule`, `GetProgramScheduleItems`, `ReplaceProgramScheduleItems` (+ wypełniacze, znaki wodne, elementy graficzne, kody języków, playouty do list) | **pełna edycja**: ustawienia i pozycje (edytor pozycji: start, treść, kolejność, maraton, tryb, wypełniacze, znaki i grafika, audio/napisy), kopiowanie, usuwanie (z ostrzeżeniem o playoutach); jeden „Zapisz” zapisuje ustawienia, potem pozycje |
| Playouty | `GetPagedPlayouts`, `CreateClassicPlayout`, `CreateBlockPlayout`, `CreateSequentialPlayout`, `CreateScriptedPlayout`, `CreateExternalJsonPlayout`, `UpdatePlayout`, `UpdateSequentialPlayout`, `UpdateScriptedPlayout`, `UpdateExternalJsonPlayout`, `DeletePlayout`, `BuildPlayout`, `ResetAllPlayouts`, `ErasePlayoutItems`, `ErasePlayoutHistory`, `CheckForOverlappingPlayoutItems`, `GetPlayoutAlternateSchedules`, `ReplacePlayoutAlternateScheduleItems`, `GetFuturePlayoutItemsById`; blokowy: `GetPlayoutTemplates`, `ReplacePlayoutTemplateItems`, `GetDecoByPlayoutId`, `UpdateDefaultDeco` (+ szablony, bloki, deco do kalendarza) (+ kanały, harmonogramy; `GetPlayoutWarningsCount` w pasku górnym) | **pełna obsługa**: zakładanie wszystkich rodzajów (tylko kanały bez playoutu), harmonogram główny, ścieżka pliku/skryptu, codzienna przebudowa, **harmonogramy zastępcze** (dni tygodnia/miesiąca, miesiące, zakres dat, kolejność), **kalendarz tygodnia playoutu blokowego** (szablony dni z szablonem deco, deco domyślne, widok siatki godziny × dni albo pasków doby z przełącznikiem Siatka / Paski, pigułki przypisań szablony → dni, linia „teraz”, klik w blok – jego pozycje i deco), stan ostatniej budowy, przebudowa / od nowa / wyczyszczenie pozycji i historii / sprawdzenie nakładania, „Resetuj wszystkie”, usuwanie z potwierdzeniem; **lista**: rodzaj w pigułce, „Zbudowany do” (koniec ostatniej przyszłej pozycji – `GetFuturePlayoutItemsById` z `TotalCount`, dwa wywołania na playout; stan budowy tylko przy błędzie, ↻ godzina codziennej budowy) i „Przebuduj” (klasyczny, blokowy) / „Od nowa” (pozostałe) w wierszu; **najbliższe pozycje**: wypełniacze przygaszone z rodzajem (przed, w trakcie, ogon…), audycje LIVE z ramówki CG Playout w pigułce (ta sama godzina startu co pozycja), w blokowym kolumna „Blok” (`BlockId` z `SchedulingContext` pozycji + `GetAllBlocks`; wypełniacz bez kontekstu – blok poprzedniej pozycji) |
| Biblioteki | `GetAllLocalLibraries`, `CountMediaItemsByLibrary`, `GetLocalLibraryPaths`, `CountMediaItemsByLibraryPath`, `CreateLocalLibrary`, `UpdateLocalLibrary`, `MoveLocalLibraryPath`, `DeleteLocalLibrary`, `QueueLibraryScanByLibraryId` | **pełna obsługa bibliotek lokalnych**: rodzaj, nazwa, ścieżki w kontenerze legacy (dodaj / usuń z potwierdzeniem / przenieś do innej biblioteki tego rodzaju), liczba pozycji (biblioteka i ścieżka), „Skanuj teraz” (też od razu po założeniu), usuwanie z potwierdzeniem; **„Foldery obrazów”** – czas obrazu na folder (K8, niżej) |
| Źródła zewnętrzne | Jellyfin / Emby (`K` = `Jellyfin` albo `Emby`): `GetAllKMediaSources`, `GetKSecrets`, `SaveKSecrets`, `DisconnectK`, `SynchronizeKMediaSources`, `SynchronizeKLibraries`, `GetKLibrariesBySourceId`, `UpdateKLibraryPreferences`, `SynchronizeKLibraryByIdIfNeeded`, `ForceSynchronizeKLibraryById`, `SynchronizeKCollections`, `GetKPathReplacementsBySourceId`, `UpdateKPathReplacements`; Plex: `GetAllPlexMediaSources`, `StartPlexPinFlow`, `SignOutOfPlex`, `DeletePlexMediaSource`, `SynchronizePlexMediaSources`, `SynchronizePlexLibraries`, `GetPlexLibrariesBySourceId`, `UpdatePlexLibraryPreferences`, `SynchronizePlexLibraryByIdIfNeeded`, `ForceSynchronizePlexLibraryById`, `SynchronizePlexNetworks`, `SynchronizePlexCollections`, `GetPlexPathReplacementsBySourceId`, `UpdatePlexPathReplacements`; strumienie: `GetAllLocalLibraries` | zakładki **Jellyfin**, **Emby** (adres i klucz API – klucz tylko do zapisu, połącz / połącz ponownie, rozłącz z potwierdzeniem), **Plex** (logowanie kodem PIN na stronie Plex, inne konto, wyloguj, usuń serwer); dla każdego serwera: **biblioteki** do synchronizacji (zapis wyboru, synchronizacja biblioteki, odświeżenie listy, synchronizacja kolekcji) i **zamiany ścieżek**; **strumienie zdalne** – opis pliku .yml i biblioteki tego rodzaju |
| Przeglądarka | `QuerySearchIndexAllItems`, `QuerySearchIndex{OtherVideos,Movies,Shows,Seasons,Episodes,MusicVideos,Artists,Songs,Images,RemoteStreams}`, `GetMediaItemInfo`, `CreateSmartCollection`, `AddItemsToCollection`, `AddItemsToPlaylist` | zakładki rodzajów z liczbą pozycji, wyszukiwarka (składnia legacy, podpowiedź pól), stronicowanie po 50, karta pozycji (stan, czas, obraz, strumienie, rozdziały), **„Dodaj zaznaczone do…”** kolekcji zwykłej albo playlisty; **widok Tabela / Kafle** (przełącznik w nagłówku panelu, pamiętany w `localStorage`): kafel = okładka legacy (`/artwork/posters/…` dla filmów, seriali i sezonów, `/artwork/thumbnails/…` dla reszty, spod `--stream-url`; bez okładki albo przy błędzie – rodzaj pozycji), tytuł, plik, stan, pole zaznaczenia; klik albo Enter – karta pozycji; **„Zapisz jako kolekcję inteligentną”** (przy niepustym zapytaniu, `CreateSmartCollection`): bieżące zapytanie bez ograniczenia do zakładki, nazwa w oknie, potem ekran Kolekcje; **karta serialu i sezonu bez `GetMediaItemInfo`** (§13): sezony i odcinki z indeksu (`QuerySearchIndexSeasons`, `QuerySearchIndexEpisodes` z `show_title:"…"`, przefiltrowane po `ShowId` / `SeasonId`, bo tytuł może mieć kilka seriali), odcinki w dwóch kolumnach (numer ze stanem; tytuł z nazwą pliku, gdy różni się od tytułu; bez numeru w nazwie pliku – E00 przygaszone, kolejność naturalna po tytule), klik w sezon – karta sezonu, w odcinek – karta pozycji, „‹ Wróć” – karta nadrzędna; karta serialu: „Skanuj serial”, „Wyodrębnij napisy” (K8); karta artysty – bez zapytania do legacy; **okładki** filmów, seriali i sezonów (miniatura w tabeli, plakat w karcie; odcinek – plakat serialu) przez `/api/artwork/…`; **„Odtwarzanie”** w karcie pozycji z plikiem (bez obrazów): wprost z Jellyfin albo biblioteki lokalnej (`/api/vod/…`, ścieżka z karty odcinka, `GetMovieById` albo względna z karty „Inne”), Emby, Plex i strumienie zdalne – przez test odtwarzania legacy po 30 s (§13, `GetAllFFmpegProfiles`); rozdziały ponad 12 zwinięte (Jellyfin potrafi wygenerować setki) |
| Kosz | `QuerySearchIndex…` (`state:FileNotFound`), `DeleteItemsFromDatabase`, `EmptyTrash` | pozycje bez pliku ze wszystkich rodzajów, usuwanie zaznaczonych z bazy, „Opróżnij kosz” – z potwierdzeniem |
| Kolekcje | zwykłe: `GetAllCollections`, `GetCollectionCards`, `CreateCollection`, `UpdateCollection`, `DeleteCollection`, `RemoveItemsFromCollection`; inteligentne: `GetAllSmartCollections`, `CreateSmartCollection`, `UpdateSmartCollection`, `DeleteSmartCollection`, `QuerySearchIndexAllItems`; wielokolekcje: `GetAllMultiCollections`, `GetMultiCollectionById`, `CreateMultiCollection`, `UpdateMultiCollection`, `DeleteMultiCollection`; powtórki: `GetPagedRerunCollections`, `GetRerunCollectionById`, `CreateRerunCollection`, `UpdateRerunCollection`, `DeleteRerunCollection` | **pełna edycja** w czterech zakładkach: zwykła z listą pozycji i usuwaniem zaznaczonych; inteligentna z zapytaniem i liczbą wyników („Sprawdź zapytanie”); wielokolekcja ze składnikami (zwykłe/inteligentne, „jako grupa”); powtórki (treść, kolejność premier i powtórek); **zakładka „Wszystkie”** (domyślna): kolekcje czterech rodzajów w jednej tabeli z pigułką rodzaju (zwykła / inteligentna / wielokolekcja / powtórki), liczbą pozycji (wielokolekcja – składników) i zapytaniem albo źródłem; klik – edytor w zakładce rodzaju (`crudView().select(id)`), „+ Nowa …” dla każdego rodzaju. Łącznego czasu kolekcji (makieta) nie ma – karty legacy nie podają długości pozycji; **kolekcje audycji LIVE** (nazwy `collection_name` z CG Playout `/api/live/shows`, bez CG – przedrostek `LIVE · `) mają pigułkę „CG Playout” i są tylko do odczytu (uwaga z odnośnikiem do Audycji LIVE zamiast Zapisz / Usuń, opcja `locked` w `crudView`), bo CG Playout zakłada je, zmienia i usuwa razem z audycją; nowej kolekcji z nazwą `LIVE · …` panel nie zapisze |
| Playlisty | `GetAllPlaylistGroups`, `GetPlaylistsByPlaylistGroupId`, `GetPlaylistItems`, `CreatePlaylistGroup`, `DeletePlaylistGroup`, `CreatePlaylist`, `DeletePlaylist`, `ReplacePlaylistItems`, `PreviewPlaylistPlayout` | **pełna edycja**: grupy (załóż, usuń), playlisty w grupach, edytor pozycji (treść przez wybór treści, kolejność, ile / wszystkie, EPG; przesuwanie, kopiowanie), pojedyncze pliki z Przeglądarki, **podgląd playoutu**; **„Import ramówki z pliku…”** – CSV/XML → playlista (K11, niżej) |
| Bloki i szablony | bloki: `GetAllBlocks`, `GetAllBlockGroups`, `CreateBlockGroup`, `DeleteBlockGroup`, `CreateBlock`, `CopyBlock`, `DeleteBlock`, `GetBlockItems`, `ReplaceBlockItems`, `PreviewBlockPlayout`; szablony dnia: `GetAllTemplates`, `GetAllTemplateGroups`, `CreateTemplateGroup`, `DeleteTemplateGroup`, `CreateTemplate`, `CopyTemplate`, `DeleteTemplate`, `GetTemplateItems`, `ReplaceTemplateItems` (+ znaki wodne, elementy graficzne) | **pełna edycja** w dwóch zakładkach: bloki w grupach (nazwa, długość co 5 min, koniec bloku, pozycje: treść, kolejność, EPG, znaki wodne, grafika; kopiuj; **podgląd wypełnienia bloku**); szablony dnia w grupach (bloki o godzinach startu na **osi doby**, nakładanie na czerwono; kopiuj) |
| Wystrój (deco) | deco: `GetAllDecoGroups`, `GetDecosByDecoGroupId`, `GetDecoById`, `CreateDecoGroup`, `DeleteDecoGroup`, `CreateDeco`, `UpdateDeco`, `DeleteDeco`; szablony deco: `GetAllDecoTemplateGroups`, `GetDecoTemplatesByDecoTemplateGroupId`, `CreateDecoTemplateGroup`, `DeleteDecoTemplateGroup`, `CreateDecoTemplate`, `DeleteDecoTemplate`, `GetDecoTemplateItems`, `ReplaceDecoTemplateItems` | **pełna edycja** w dwóch zakładkach: deco (znak wodny i grafika: dziedzicz / wyłącz / zastąp / scal, „także w wypełniaczu”; przerwy: playlista na starcie bloku; wypełniacz domyślny z przycinaniem; martwe powietrze); szablony deco (deco w przedziałach godzin na osi doby) |
| Grafika kanału | `GetAllGraphicsElements`, `RefreshGraphicsElements`; na żądanie `GetAllProgramSchedules`, `GetProgramScheduleItems`, `GetAllBlocks`, `GetBlockItems`, `GetAllDecoGroups`, `GetDecosByDecoGroupId`, `GetDecoById`; CG Playout: `/health`, `/etv/pages`, `/api/live/graphics` | stan VizRS CG Playout (sesje elementu VizRS, nakładka ffmpeg, live ingest, przechwytywanie grafiki per kanał), **„Kiedy strona wchodzi na antenę”** – strony VizRS kanału z trzema źródłami naraz: czas z Trio (⏱ kanał, `playout` strony z `/etv/pages`: `at`, `out`, `ref`), reguły CG Playout z `TAKE` tej strony (globalne, audycji, kanału) i element legacy strony (`page-<nr>.yml` albo nazwa „… (nr)”) z przypisaniami w harmonogramach, blokach i deco (przycisk „Sprawdź przypisania w ramówce”, wynik pamiętany do przeładowania strony); kolumna **Skutek**: reguła CG, ramówka (z czasem z Trio), „nie wchodzi sama” albo ostrzeżenie „reguła i ramówka – może wejść dwa razy”, podgląd anteny VizRS `/output` (`--vizrs-url`, token wyjścia, antena kanału `bus=ch-N` przy `--vizrs-channels`) z odnośnikiem do Trio, elementy graficzne legacy i „Wczytaj pliki elementów”; **tabela reguł „Kiedy / Co / Ile”** (streszczenie reguł globalnych, audycji i wybranego kanału po polsku – akcja, strona z nazwą, pola, wiersz „przejęcie przez live ingest (grafika ramówki wstrzymana)” z czasem „na czas przejęcia”; tekst komendy w podpowiedzi; klik przenosi kursor do edytora) i pod nią **edytor reguł** z wyborem grafik zamiast komend tekstowych (`cmdedit.js`; reguły audycji i kanałów jako zwijane karty; zapis zostawia listę `secondary` bez zmian; w karcie reguły kanału wiersz „Na przejęcie przez live ingest” na komendy `on_live`, pokazywany gdy CG Playout ma cechę `on_live` w `features`, puste pole = pominięcie klucza przy zapisie; pod nim „Znak wodny ErsatzTV na przejęcie” – wybór znaków wodnych legacy (`GetAllWatermarks`, zapis `live_watermark` jako numery „3,5”, pokazywany przy cesze `live_watermark`; bez niej wartość zostaje bez zmian); w „Ustaw wejścia…” Slotów na żywo to samo przy wejściu – „Znak wodny ErsatzTV wejścia” (puste = znak kanału); zapis PUT `/api/live/graphics`); **Zdarzenia wtórne** (K15/K16, `views/zdarzenia.js`, przy cesze `events`; bez niej – uwaga o aktualizacji CG Playout): reguły jako zwijane karty – nazwa, kanał, wzorce tytułu audycji, pozycji i ścieżki pliku, „Tylko pozycja o starcie”, zdarzenia (strona z show albo numer, „po starcie”/„przed końcem”, sekundy, czas na antenie, pola `nazwa=wartość`), oś pozycji (lewa część – od startu, prawa – przed końcem, środek umowny; zdarzenie, które nie wejdzie, szare), przy cesze `approval` „Ważna od/do” i zatwierdzanie („Wymagaj zatwierdzenia” → `approved: null`, „Zatwierdź” – tylko administrator (panel bez kont i token awaryjny = administrator) → `{by: konto, at}`; operator widzi „zatwierdza administrator”; zmiana treści zatwierdzonej reguły przy zapisie cofa zatwierdzenie; serwer panelu przy `PUT /api/cg/live/graphics` od operatora porównuje zatwierdzenia z bieżącymi w CG Playout – nowe albo przeniesione na inną treść = 403, cofnięcie i usunięcie wolno), pigułki „czeka na zatwierdzenie”, „wygasa dziś/jutro”, „wygasła”, liczniki w nagłówku i filtr (czekające, wygasające w 7 dni, nie wchodzą); zdarzenia z własną ważnością albo zatwierdzeniem (z `config.json`) przechodzą bez zmian; zapis PUT `/api/live/graphics` z całym dokumentem i nową listą `secondary`, sprawdzenie przed wysłaniem (cel, wzorce, strona, czasy) |
| Sloty na żywo | CG Playout: `/api/live/state`, `/api/live/channels/…`, `/api/live/inputs`, `/api/live/youtube-cookies` | stanowisko MCR (dawny VizRS Live Panel z CG Playout `/live`, przeniesiony 2026-11-07; strona `/live` usunięta z CG Playout): lampki CG Playout / ErsatzTV / odbiór sygnałów; karta kanału: tally NA ANTENIE / WSTRZYMANY / RAMÓWKA, **sygnał studia** z licznikiem czasu (zegar CG), kodek i dźwięk, tryb studia (konfiguracja CG / panel), wiersz **„Grafika”** – „live – ramówka wstrzymana” (podczas przejęcia) albo „z ramówki” (gdy pole `takeover` w stanie kanału z CG Playout), linia „Teraz” z **ostrzeżeniem**, gdy audycja LIVE trwa albo zaczyna się w ciągu 10 min bez studia (odliczanie); **■ ZDEJMIJ** dwoma kliknięciami (pierwsze uzbraja „POTWIERDŹ ZDJĘCIE” na 3 s) / **▶ WPUŚĆ**, „studio także poza audycjami” (do zapisu pole zablokowane, a karta trzyma kliknięty stan, aż CG go potwierdzi – najdłużej 10 s; wcześniej odświeżanie co 1 s cofało pole i operator klikał kilka razy), **⟲ Przebuduj ramówkę** (K22: Refresh przez most – `GetPlayoutIdByChannelNumber` → `BuildPlayout` z `Mode: Refresh`; postęp serii zostaje; most bez tych komend – komunikat „przebuduj w Playoutach”), zwijane „Jak nadawać”; **Nagła emisja…** (K23, CG Playout z cechą `windows`; starszy – bez przycisku): okno z datą (domyślnie dziś), od–do (koniec przed początkiem = po północy), wejściem (domyślnie jak w kanale), tytułem, podglądem pozycji ramówki pod oknem (`GetPlayoutIdByChannelNumber` → `GetFuturePlayoutItemsById`) i losem tej pozycji („zastąp – odcinek pod spodem przepada”; „zachowaj odcinek” tylko z cechą `windows_keep`); zaplanowane okna na karcie kanału (data i godziny albo „NAGŁA EMISJA – trwa”/„– wstrzymana”, tytuł, los, 🗑 z potwierdzeniem; usunięcie trwającego od razu oddaje kanał); alarm „nagła emisja … – brak sygnału studia, leci ramówka”, gdy okno trwa minutę bez sygnału (K17, baner); w Dzienniku emisji `window_start` / `window_end`; **Ramówka** (EPG z CG, zakładki kanałów, bieżąca pozycja, znacznik LIVE z godziną; wybrany kanał pamiętany w przeglądarce); panel **Wejścia studia** (tabela wejść RTMP/SRT: kanał, wejście, adres bez sekretów, stan i parametry obrazu; **Ustaw wejścia…** – edytor wejść, niżej; **Ciasteczka YouTube…** – ciasteczka zalogowanego konta dla wejść YouTube, niżej); **Zdarzenia** (40 ostatnich). Stan co 1 s, przebudowa kart tylko po zmianie treści |
| Audycje LIVE | CG Playout: `/api/live/shows`, `/api/live/settings`; `GetAllLocalLibraries`, `GetLocalLibraryPaths` | tabela audycji: kanał, tytuł, długość, stan w ErsatzTV (plik / biblioteka / kolekcja), kolekcja do harmonogramu, emisja w ramówce („dziś 18:00 · +2”, „● teraz”, „nie ma w ramówce”) z godzinami z harmonogramów, pod tytułem kolekcja `LIVE · …` i kategoria wiekowa; nowa / zmiana / usunięcie (z potwierdzeniem), skan biblioteki; **okno audycji** (nowa i „Edytuj…”) – w jednym oknie dane w EPG (tytuł, długość, kategoria wiekowa `content_rating`, opis; zapis w CG Playout, `PUT` tylko przy zmianie) i **emisje**: wiersz = godzina (z podglądem końca z długości), dni = harmonogram playoutu kanału (główny „pozostałe dni” / „codziennie” i alternatywne z dniami z `GetPlayoutAlternateSchedules`), start ściśle / elastycznie (`FixedStartTimeBehavior`), własny tytuł emisji w EPG (`CustomTitle`), znak wodny (`WatermarkIds`) i grafika (`GraphicsElementIds` – element graficzny legacy jak w Harmonogramach, strony VizRS CG System pierwsze z numerem strony; w oknie pierwszy z listy, niezmieniony zachowuje całą listę dawnej pozycji); na liście godziny emisji z harmonogramów (`emisje: 20:00, 10:00 (sob, nd)`), a gdy są w harmonogramie, ale nie w EPG – „brak w EPG – przebuduj ramówkę”. Zapis emisji: w każdym harmonogramie kanału panel zdejmuje dotychczasowe pozycje audycji i wstawia te z okna (z resztą pól dawnej pozycji, np. wypełniaczami i elementami graficznymi ustawionymi w Harmonogramach) przez `GetProgramScheduleItems` → `ReplaceProgramScheduleItems` (jak ekran Harmonogramy); harmonogram bez zmian nie jest zapisywany, a gdy nic się nie zmieniło – bez przebudowy; zapis harmonogramu legacy sam kończy przebudową Refresh (`ReplaceProgramScheduleItems` → `BuildPlayout` Refresh każdego playoutu harmonogramu), więc panel nie przebudowuje drugi raz (krok kreatora „Ramówka kanału przebudowana” z tym opisem); po zmianie danych audycji widocznych w EPG (tytuł, długość, opis, kategoria) – Refresh przez most (K22). Reset (`/rebuild` CG Playout, `POST /api/channels/<n>/playout/reset` legacy – kasuje postęp odcinków) panel woła już tylko jako „Od nowa” w Playoutach i Na antenie, z potwierdzeniem. Pozycja (kolekcja inteligentna `LIVE · …` z `collection_id`, tryb One) trafia do bloku stałej godziny, w który wpada (za jego pozycjami „po poprzedniej”, przed następną stałą godziną); gdy zaraz po audycji jest następna stała godzina, a przed audycją treść „wypełnij”, za audycją wraca ta sama treść (kopia „po poprzedniej”, zdejmowana razem z emisją) – inaczej kreator ostrzega o wypełniaczu zapasowym / planszy do tej godziny; odmowa: playout nieklasyczny (wtedy ręcznie), harmonogram tasujący pozycje, stała godzina już zajęta, zły zapis godziny; **kreator** po zapisie: kroki CG Playout i panelu (✓/!); bez komend harmonogramu w moście – instrukcja ręczna i „Otwórz Harmonogramy”; **biblioteka audycji** wybierana z bibliotek Remote Streams legacy (zamiast wpisywania ID) z kontrolą folderu `/config/vizrs-live`: biblioteka bez niego = czerwone ostrzeżenie (jakie foldery ma) i przycisk **„Dodaj folder /config/vizrs-live”** (`UpdateLocalLibrary` z wszystkimi dotychczasowymi ścieżkami – legacy zastępuje listę, a pominięta ścieżka znika z bazy razem z pozycjami – po potwierdzeniu, potem skan przez CG Playout); kreator nie wstawia wtedy emisji do harmonogramu (krok „Godzina emisji” z powodem) – pusta kolekcja `LIVE · …` w harmonogramie zatrzymuje budowę ramówki całego kanału („SmartCollection … has no valid items”, S1 2026-11-21: biblioteka 7 z samym `/media/tv/Live`); zdjęcie emisji działa zawsze; przebudowa ramówki per kanał (Refresh przez most, jak na karcie Slotów) |
| Dziennik emisji | CG Playout: `GET /api/live/state` (`features`), `GET /api/live/asrun?channel=&day=`, `GET /api/live/logs?source=&channel=&day=`; most: `GetRecentLogEntries` | centralne miejsce logów systemu emisji kanału: wybór źródła – „antena (dziennik emisji)” (domyślne), „live ingest – odbiorniki”, „przejęcia kanału (ErsatzTV ↔ live)”, „sidecar VizRS CG” (te trzy z `logs` w `features` CG), „ErsatzTV legacy” (`GetRecentLogEntries`: strony po 200 od najnowszych, najwyżej 10, wpisy z wybranego dnia UTC – legacy nie dzieli logu na kanały) i „wszystko na jednej osi czasu” (źródła równolegle, posortowane po czasie, kolumna „Źródło”; źródło, które nie odpowiedziało, daje jeden wiersz błędu zamiast przerywać widok); logi: tabela Czas / Poziom (info, uwaga, błąd) / Wpis, próg „od: uwaga” / „tylko błędy”, filtr tekstu; CG bez `logs` – zostaje antena i legacy z uwagą o aktualizacji CG; eksport CSV bieżącego widoku (logi: czas, źródło, poziom, wpis). Antena – K14: kanał, dzień, filtr tekstu, tabela (czas, zdarzenie, pozycja, strona/warstwa, operator), etykiety zdarzeń w tym przejęcia przez live ingest: „przejęcie live – grafika ramówki wstrzymana” (`live_graphics_on`) i „koniec przejęcia – grafika ramówki wraca” (`live_graphics_off`), eksport CSV; zmiany anteny spoza panelu i reguł (Trio, `/mobile`, makra, karuzela VizRS, Companion) CG Playout wykrywa z odczytów warstw i zapisuje jako `take`/`out` z operatorem „VizRS (poza panelem)”; CG Playout bez `asrun` w `features` – opis, że ta wersja nie prowadzi dziennika (kontrakt, DOKUMENTACJA §7) |
| Ustawienia | `GetFFmpegSettings`, `UpdateFFmpegSettings`, `GetMpegTsScripts`, `GetPlayoutSettings`, `UpdatePlayoutSettings`, `GetXmltvSettings`, `UpdateXmltvSettings`, `GetLibraryRefreshInterval`, `UpdateLibraryRefreshInterval`, `GetHDHRTunerCount`, `UpdateHDHRTunerCount`, `GetHDHRUUID`, `GetLoggingSettings`, `UpdateLoggingSettings`, `GetUiSettings`, `UpdateUiSettings` (+ profile, znaki wodne, wypełniacze do list) | **pełna edycja ustawień legacy** w siatce paneli obok siebie (wczytywanych równolegle, każdy z osobnym „Zapisz” w nagłówku): FFmpeg i strumienie, Playouty, Przewodnik XMLTV, Skaner i HDHomeRun, Poziomy logów, Web GUI legacy; pod nimi panele „Panel” i „Most”; błąd legacy izolowany w panelu danej sekcji |
| Logi | `GetRecentLogEntries`; CG Playout: `/api/live/state` | wybór źródła: „ErsatzTV legacy” (najnowsze wpisy logu legacy, próg poziomu, wyszukiwanie w treści, stronicowanie po 200, odświeżanie co 5 s) albo „VizRS CG Playout” (dziennik zdarzeń z `/api/live/state`: czas, kanał, wpis; filtr tekstem; CG nie wystawia pliku log.jsonl przez API) |
| Diagnostyka | `GetAllHealthCheckResults` (też z `Refresh`), `GetTroubleshootingInfo`, `GetNextVersion`, `GetNextDescribe`, `GetNextProbe`, `RefreshNextProbe` (karta **„Silnik Next”**, K5: wersja, schematy playoutu i kanału `breaking.compatible`, segment i klatka kluczowa, obsługiwane kodeki i akceleracje, czas ostatniego sprawdzenia sprzętu z wersją ffmpeg, tabela akceleracji – stan, urządzenie, sterownik, kodeki dekodowania/kodowania, błąd – i „Sprawdź ponownie”; bez tych komend karty nie ma), `RefreshFFmpegCapabilities`, `GetPlayoutWarningsCount`, `EmptyTrash`, `DeleteOrphanedArtwork`, `DeleteOrphanedSubtitles`, `ReleaseMemory`, `GetAllChannels` + wszystkie z listy, zapytania wyboru treści; CG Playout: `/health` (też `zones` przy `area: auto`), `/api/live/state` | panel **Stan** na górze (wiersz „strefy grafiki”, gdy CG Playout ma `VIZRS_AREA=auto`: liczba śladów stron i granic stron z VizRS – `bounds` w `/health`, CG Playout od 26.11.3 – strefy przebiegów nakładki, % ekranu, grafika poza strefami → UWAGA) (tabela Stan / Składnik / Opis, pigułki OK / UWAGA / BŁĄD / INFO, „Sprawdź ponownie”: wersja legacy z mostu, most i biała lista z ostrzeżeniem o nieznanych komendach, podsumowanie kontroli legacy, błędy budowy playoutów, Next i ffmpeg, stan CG Playout oraz adres VizRS), kontrole legacy („Sprawdź ponownie”), wersje i sprzęt (legacy, ffmpeg, procesor, karty grafiki), konserwacja z potwierdzeniami i oknem wyniku z logu legacy (K6), test odtwarzania („Odtwórz w panelu” – okno podglądu; odnośniki do strumienia HLS/TS kanału przy `--stream-url`), konsola komend, wybór treści; **web GUI legacy dla widzów** (R10: wynik `/api/exposure` – zamknięte / widoczne / nie sprawdzono) i **pokrycie web GUI legacy** (lista kontrolna §15 z odnośnikami do ekranów) |
| Użytkownicy | – (panel: `/api/users`) | konta operatorów panelu (widoczne w menu tylko dla administratora z kontami włączonymi): tabela login / rola (zmiana listą od razu) / utworzone / ostatnio w panelu / liczba sesji, „Nowe hasło” (wylogowuje konto), „Wyloguj” (wszystkie sesje konta), „Usuń” (z potwierdzeniem); panel „Nowe konto” (login, hasło, rola); panel „Logowanie w pakiecie VizRS” – przełącznik kont panelu w Trio i Artist. Pasek górny: „konto: login · stanowisko” – okno **Moje konto** (stanowisko do dziennika CG, zmiana własnego hasła) |

Na białej liście są też komendy bez ekranu, używane przez `panel/tests/stack.sh seed` i testy oraz dostępne w konsoli: `CreateLocalLibrary`, `QueueLibraryScanByLibraryId`, `GetProgramScheduleById`, `GetPlayoutIdByChannelNumber`.

**Harmonogramy – szczegóły.** Listy wyboru pozycji (start, tryb, kolejność, maraton, reszta czasu…) są w `panel/server/web/views/harmonogramy.js`, bo most nie opisuje wyliczeń typu zagnieżdżonego; ograniczenia jak w `ScheduleItemsEditor.razor` legacy: kolejność zależna od rodzaju treści, „wypełniaj grupami” tylko dla kilku/czasu i kolekcji, przy tasowaniu pozycji bez stałej godziny i trybu „wypełnij” (sprawdzane też przed zapisem). Wypełniacze w polach pozycji filtrowane po rodzaju (`FillerKind`). Pozycje wysyłane z `Index` = 1, 2, … w kolejności z tabeli; `GetProgramScheduleItems` nie zwraca ich posortowanych – panel sortuje po `Index`. Walidację robi legacy przy `ReplaceProgramScheduleItems` (np. „Tail Filler is required with tail mode Filler”, „A collection must not use multiple playback orders”) – komunikat zostaje po angielsku, z polskim wstępem; ustawienia harmonogramu są wtedy już zapisane, pozycje nie. Tabela pozycji: start („06:00 stała”, z nadpisaniem zachowania „ściśle”/„elastycznie”, albo „po poprzedniej”), treść, tryb, kolejność, wypełniacze w skrócie („przed: … · ogon: …”) i etykiety znaków wodnych oraz elementów graficznych. **Podgląd 24 h** (pasek doby pod pozycjami, odświeżany przy każdej zmianie listy): każda pozycja o stałej godzinie trwa do następnej stałej, z liczbą pozycji „po poprzedniej” za nią (`+2`, pełna lista w podpowiedzi); to szkielet – dokładne godziny zależą od długości plików i liczy je legacy przy budowie playoutu; bez stałych godzin – sam komunikat. Kolumna „Kanały”, uwaga w edytorze i potwierdzenie usunięcia: playouty klasyczne z `ScheduleName` = nazwa harmonogramu (nazwy są w legacy unikalne).

**Import ramówki z pliku (K11, 2026-11-09).** Playlisty → „Import ramówki z pliku…” (wzorzec Vizrt E, `panel/server/web/views/import.js`): plik CSV (kolumny start, tytuł albo ścieżka pliku, długość – po nagłówku po polsku/angielsku albo w tej kolejności; separator `;`, `,` albo tabulator, cudzysłowy jak w CSV) albo XML (każdy element z dzieckiem lub atrybutem `title`/`tytul`/`path`/`plik`, np. `<event>`), do 2 MB i 500 pozycji, czytany w przeglądarce – nic nie trafia na `/config` (R2). Dopasowanie do mediów legacy przez wyszukiwarkę (`QuerySearchIndex{OtherVideos,Movies,Episodes,MusicVideos,Songs,Images}`, zapytanie `title:"…"`, 10 wyników na rodzaj, po 4 wiersze naraz): dokładny tytuł bez wielkości liter = „dopasowane”, kilka = „niejednoznaczne” (gdy w pliku jest ścieżka, wygrywa pozycja z tą samą nazwą pliku w opisie karty), tylko podobne = „podobne” (bez wyboru domyślnego), nic = „brak”. Ścieżka bez tytułu: najpierw nazwa pliku bez rozszerzenia (tytuł legacy bez metadanych), potem z `_` `.` `-` zamienionymi na spacje (indeks wyszukiwania legacy nie ma pola ścieżki). Podgląd w oknie z wyborem pozycji legacy przy każdym wierszu; „Utwórz playlistę” zakłada playlistę w wybranej grupie (`CreatePlaylist`) i zapisuje dopasowane pozycje w kolejności pliku (`ReplacePlaylistItems`, pojedyncze pozycje, w EPG), niedopasowane pomija. **Godziny z pliku są tylko informacją** – emisję o stałej godzinie ustawia pozycja harmonogramu („stała godzina”); dokładne co do sekundy godziny, Harris/Omnibus, folder obserwowany – poza punktem ([plany/PLAN-mcr.md](plany/PLAN-mcr.md) §6, C2).

**Mozaika kanałów (K10, 2026-11-09).** Na antenie → przycisk „Mozaika” (stan pamiętany w przeglądarce): nad kartami siatka podglądów kanałów (`Preview` z `player.js`, ta sama ścieżka co „Podgląd ▶”: `/api/stream/{kanał}.m3u8`), wyciszone; klik (albo Enter) w kafel włącza dźwięk tylko z niego. Pierwsze N kanałów z ekranu (wybór kanału w pasku górnym zawęża listę), N = `--multiview-max` (domyślnie 4) – przy większej liczbie kanałów uwaga z limitem. Czerwona ramka i pasek z pierwszym błędem, gdy kontrola ramówki (K9) znalazła błąd kanału. Zmiana ekranu zatrzymuje wszystkie podglądy (`S.cleanup` w `route()`), a legacy kończy nieoglądane sesje po czasie bezczynności. **Koszt:** każdy kafel to sesja Next, która koduje kanał (jak widz); miniatury co N s rozważone i odrzucone – legacy nie wystawia klatek kanału bez sesji, więc nie byłyby tańsze.

**Kontrola ramówki (K9, 2026-11-09).** Na wzór automatycznej kontroli playlisty Vizrt, tylko z danych, które panel już ma, liczona w przeglądarce (`panel/server/web/check.js`), bez zapisu i bez pętli w serwerze. Reguły: **dziura** między pozycjami dłuższa niż 5 s (leci wypełniacz zapasowy), **nakładanie** – pozycja zaczyna się ponad 5 s przed końcem poprzedniej, oraz wpis legacy o nakładaniu z ostatnich 24 h (K6, `overlapWarnings`), **koniec playoutu** w ciągu 24 h albo brak przyszłych pozycji (ostatnia pozycja z `TotalCount` – osobne zapytanie o ostatnią stronę), **plik w koszu** – tytuły pozycji legacy `state:FileNotFound` (`QuerySearchIndexAllItems` i zapytania rodzajów, do 100 na rodzaj) porównane z tytułami pozycji na 24 h (pozycja playoutu nie niesie id pliku – porównanie po tytule, możliwe fałszywe trafienia przy powtarzających się tytułach), **audycja LIVE w ciągu 24 h bez wejścia studia** (CG Playout: `next_live` kanału i `live/inputs`), kanał bez playoutu; zdjęta grafika i błąd budowy – jak dotąd w „Uwagach”. Gdzie: **Na antenie** – przy wejściu, co 5 min i przyciskiem „Sprawdź ramówkę” (pełna kontrola, do 300 pozycji na kanał), pigułka „BŁĄD RAMÓWKI ×n” z listą w podpowiedzi i czerwona ramka karty, wpisy w „Uwagach”; **EPG** – dziury jako czerwone pola „dziura”, błędne pozycje na czerwono z „⚠” i opisem w podpowiedzi; **Playouty** – pigułka „kontrola: n uwag” w nagłówku najbliższych pozycji, dziury nad tabelą, kolumna „Kontrola”.

**Biblioteki: obrazy, serial, napisy (K8, 2026-11-09).** Biblioteki: gdy jest biblioteka rodzaju „Obrazy”, pod edytorem panel **„Foldery obrazów”** (odpowiednik ekranu Images legacy): foldery najwyższego poziomu ze wszystkich bibliotek obrazów (`GetImageFolders` bez folderu), wejście w podfoldery z okruszkami, liczba podfolderów i obrazów, czas wyświetlania jednego obrazu (s; puste = domyślny z ustawień legacy) zapisywany od razu `UpdateImageFolderDuration` (panel sprawdza 0–86400 s). Przeglądarka → zakładka Seriale → karta serialu: **„Skanuj serial”** (`GetTelevisionShowById` daje bibliotekę, potem `QueueShowScanByLibraryId` bez głębokiego skanu; `false` od legacy = nie zlecił, np. biblioteka zewnętrzna bez synchronizacji pozycji) i **„Wyodrębnij napisy”** (`ExtractEmbeddedShowSubtitles`); Playouty → konserwacja: **„Wyodrębnij napisy”** (`ExtractEmbeddedSubtitles` z playoutem – napisy osadzone pozycji tego playoutu). Napisy to zadania w tle – wynik jak w K6 (okno z wpisami logu pasującymi do „subtitle”, do 30 s; legacy pisze je zwykle tylko przy błędzie albo na poziomie Debug).

**Historia bloków (K7, 2026-11-09).** W edytorze playoutu blokowego panel „Historia bloków” (tylko odczyt, odpowiednik Troubleshooting → Block Playout legacy): wybór bloku z szablonów dnia playoutu (`GetAllBlocksForPlayout`), tabela wpisów historii (`GetBlockPlayoutHistory`, strony po 20, najnowsze legacy podaje w swojej kolejności) z kluczem stanu i „Pokaż” → `DecodePlayoutHistory` (kolejność odtwarzania, kolekcja, ostatnia pozycja). Legacy pamięta w historii stan losowania i postęp kolekcji między budowami – „Wyczyść pozycje i historię” go zeruje. Wyniki to proste rekordy (`PlayoutHistoryViewModel`, `PlayoutHistoryDetailsViewModel`, `BlockViewModel`) – bez obiektów wewnętrznych, więc komendy weszły na białą listę (wcześniej pominięte). Walidator YAML harmonogramu sekwencyjnego (LUKI #25) – poza MediatR, poza przebiegiem.

**Playouty – szczegóły.** Zakładanie: rodzaj → komenda `Create<Rodzaj>Playout` (legacy od razu buduje playout od nowa w tle). Klasyczny wymaga harmonogramu z pozycjami; sekwencyjny (YAML), skryptowy i zewnętrzny JSON – ścieżki pliku **w kontenerze legacy**, który musi już istnieć (legacy sprawdza; dla skryptu pierwsze słowo polecenia, reszta to argumenty). Blokowy startuje pusty – szablony dni i deco przypisuje się w jego kalendarzu tygodnia (niżej). Edycja: „Codziennie od nowa o” (`UpdatePlayout`, puste = wyłączone; legacy co dzień o tej godzinie robi `BuildPlayout` Reset), ścieżka pliku (`Update<Rodzaj>Playout`, tylko gdy zmieniona). **Harmonogramy zastępcze** (tylko klasyczny i nie w trybie „na żądanie”, jak w legacy): `GetPlayoutAlternateSchedules` zwraca też harmonogram główny jako ostatnią pozycję (`Id` = −1, najwyższy `Index`), a `ReplacePlayoutAlternateScheduleItems` zapisuje pozycję o najwyższym `Index` jako harmonogram główny – panel pokazuje go jako osobne pole „Harmonogram główny”, a przy zapisie dokleja na koniec listy. Legacy wybiera dla dnia pierwszą pasującą pozycję (wg kolejności); pusta lista dni/miesięcy nigdy nie pasuje, więc panel jej nie przyjmuje. Zapis tylko przy zmianie; legacy sam odświeża playout, gdy zmienia się harmonogram któregoś z już zbudowanych dni. Przebudowa: klasyczny i blokowy – „Przebuduj” (Refresh) i „Od nowa” (Reset), pozostałe tylko „Od nowa”. Konserwacja: „Wyczyść pozycje” (`ErasePlayoutItems`, blokowy/sekwencyjny/skryptowy – przyszłe pozycje, historia do teraz zostaje), „Wyczyść pozycje i historię” (`ErasePlayoutHistory`, wszystkie poza JSON), „Sprawdź nakładanie pozycji” (`CheckForOverlappingPlayoutItems` – zadanie w tle, wynik tylko w logach legacy). Licznik w pasku górnym: liczba playoutów, których ostatnia budowa się nie udała; komunikat błędu legacy jest w edytorze playoutu. Historia bloku – panel „Historia bloków” (K7, niżej).

**Bloki, szablony, deco i kalendarz tygodnia – szczegóły (R6).** Legacy nie ma `UpdateBlock` ani `UpdateTemplate`: nazwę, długość (wielokrotność 5 min, do 24 h – legacy sprawdza) i „koniec bloku” zapisuje `ReplaceBlockItems` razem z pozycjami, nazwę szablonu dnia – `ReplaceTemplateItems`. Nowy blok: `CreateBlock` (nazwa, grupa), zaraz potem `ReplaceBlockItems` z długością. Pozycje bloku jak w Block Editor legacy: kolekcja, serial, sezon, inteligentna, wyszukiwanie; kolejności zależne od rodzaju (z „losową rotacją”). Podgląd bloku (`PreviewBlockPlayout`) liczy się z pozycji wysłanych w komendzie – działa bez zapisu. Szablon dnia: pozycje to blok + godzina startu (`TimeSpan`); legacy odrzuca nakładające się bloki – panel zaznacza je wcześniej na czerwono na osi doby. Godziny szablonów i deco są w **strefie czasowej kontenera legacy** (`TimeZoneInfo.Local`; obraz testowy działa w UTC) – ustaw `TZ` kontenera legacy na strefę kanału. Deco: tryby jak w Deco Editor legacy (scal tylko dla znaku wodnego i grafiki; „także w wypełniaczu” tylko przy „zastąp”); treść przerw – tylko playlisty na starcie bloku (web GUI legacy 26.11 blokuje inne rodzaje i miejsca); wypełniacz domyślny i martwe powietrze – kolekcja, serial, sezon, artysta, wielokolekcja, inteligentna. Treść ustawiona przy innym trybie niż „zastąp” zostaje w legacy bez zmian. Szablon deco: deco od–do (koniec wcześniejszy niż start = przez północ). Kalendarz playoutu blokowego: `ReplacePlayoutTemplateItems` (pozycje `Id` 0 = nowe, `Index` wg tabeli) i `UpdateDefaultDeco`, zapis razem z playoutem, tylko przy zmianie; legacy wybiera szablon dnia tak jak harmonogram zastępczy (pierwsza pasująca pozycja), a podgląd tygodnia liczy to samo w przeglądarce (`when.js`). Dwa widoki podglądu (przełącznik „Siatka” / „Paski” w nagłówku panelu, pamiętany w `localStorage`): domyślna siatka 7 dni × godziny (bloki w 4 stałych kolorach, deco z prawej strony kolumny, czerwona linia „teraz”, pigułki przypisań szablonów do dni i ostrzeżenie o braku szablonu; tytuł z numerem tygodnia ISO; zakres godzin z bloków tygodnia, min. 4 h – ogon bloku z poprzedniego dnia sprzed tego zakresu jako znacznik „↑ … do HH:MM” w nagłówku dnia; klik albo Enter na bloku – szczegóły pod siatką: długość, godziny, pozycje bloku `GetBlockItems` (wczytane raz na blok), deco w tej godzinie i odnośniki do Bloków i Deco; poziome przewijanie na wąskim ekranie) oraz dotychczasowe paski doby 24 h jeden pod drugim. **Zmiany wchodzą na antenę dopiero po przebudowie** (legacy jej nie zleca) – panel o tym przypomina.

**Ustawienia, logi, konserwacja – szczegóły (R7, M9).** Każda sekcja ustawień to para `Get…`/`Update…`; `Update…Settings` przyjmuje cały obiekt sekcji (np. `{"XmltvSettings": {...}}`), więc panel wysyła to, co przeczytał, z polami formularza na wierzchu. Zamiast zakładek wszystkie sekcje legacy (FFmpeg i strumienie, Playouty, Przewodnik XMLTV, Skaner i HDHomeRun, Poziomy logów, Web GUI legacy) są wyświetlane w siatce paneli obok siebie (`cols`, wczytywane równolegle), każdy z osobnym przyciskiem „Zapisz” w nagłówku zapisującym wyłącznie tę sekcję; pod nimi znajdują się panele „Panel” i „Most” (dawna zakładka „Panel i most”). Błąd odczytu sekcji z legacy pojawia się tylko wewnątrz jej panelu, nie blokując pozostałych. Most nie opisuje wyliczeń w obiektach zagnieżdżonych – listy wyboru (poziomy logów, strefa XMLTV, format HLS Direct) są w `panel/server/web/views/system.js`. Skaner i HDHomeRun to osobne komendy (`UpdateLibraryRefreshInterval`, `UpdateHDHRTunerCount`), wysyłane tylko przy zmianie. Zakresy pól sprawdza panel tak jak web GUI legacy (dni playoutu i XMLTV ≥ 1, limit skryptu ≥ 10 s, segmenter bezczynny ≥ 30 s, segmenty na start ≥ 1, skan co 0–999999 godz., tunery ≥ 1). Poziomy logów działają od razu, bez restartu legacy. Logi: wybór źródła na początku paska filtrów – „ErsatzTV legacy” (`GetRecentLogEntries` czyta najnowszy plik logu legacy od najnowszych wpisów i filtruje tekstem; próg poziomu panel nakłada na stronę 200 wpisów; stronicowanie i auto-odświeżanie co 5 s) albo „VizRS CG Playout” (tylko gdy panel ma CG: dziennik zdarzeń z pola `events` w `/api/live/state` – przejęcia kanału, ZDEJMIJ/WPUŚĆ, przebudowy ramówki, audycje LIVE od startu CG Playout; kolumny Czas / Kanał / Wpis oraz filtr tekstem; CG Playout nie wystawia pliku `log.jsonl` przez API). Diagnostyka: na górze panel „Stan” (tabela Stan / Składnik / Opis, pigułki OK / UWAGA / BŁĄD / INFO, „Sprawdź ponownie”) zbierający stan stosu tylko z danych, które panel ma: wersja legacy z mostu (BŁĄD przy braku łączności), wersja mostu i biała lista (UWAGA z listą, gdy komendy z białej listy są nieznane w tej wersji legacy), podsumowanie kontroli legacy `GetAllHealthCheckResults` (błędy → BŁĄD, ostrzeżenia → UWAGA, tabela niżej), błędy ostatniej budowy playoutów z `GetPlayoutWarningsCount` (UWAGA), wersje Next i ffmpeg z `GetTroubleshootingInfo` (INFO), stan CG Playout z `/health` i `/api/live/state` (połączony, wersja, liczba kanałów, odbiór sygnałów; „—” gdy nie ustawiony, BŁĄD przy błędzie) oraz adres VizRS z `--vizrs-url` (INFO; łączność widać w podglądzie `/output` w Grafice kanału; „—” gdy nie ustawiony). Reszta ekranu bez zmian: kontrole legacy, wersje i sprzęt, ekspozycja web GUI legacy, konserwacja (zadania w tle `queued` – wynik w logach; `RebuildSearchIndex` pominięta – legacy obsługuje ją tylko przy starcie), test odtwarzania („Odtwórz w panelu” – okno podglądu, docs/API.md §2; odnośniki `--stream-url` + `/iptv/channel/{numer}.m3u8|.ts`), „Odśwież możliwości ffmpeg” (`RefreshFFmpegCapabilities`) i konsola komend.

**Źródła zewnętrzne – szczegóły i decyzje (R8, 2026-11-07).** Jellyfin i Emby mają w legacy po jednym serwerze (adres + klucz API w magazynie sekretów legacy). `Save{K}Secrets` najpierw łączy się z serwerem (`/System/Info`), a dopiero potem zapisuje i w tle pobiera listę bibliotek – błąd połączenia wraca do panelu (toast), nic nie jest zapisane. **Klucz API jest tylko do zapisu** (decyzja zachowawcza): serwer panelu go maskuje (§11), więc panel nie umie wysłać „starego” klucza – każda zmiana adresu wymaga ponownego wpisania klucza (tak关心 tak samo jest bezpieczniej niż w web GUI legacy, które pokazuje klucz). „Rozłącz” (`Disconnect{K}`) usuwa serwer, biblioteki, pozycje i klucz – z potwierdzeniem. Biblioteki: legacy pobiera tylko zaznaczone („synchronizuj”); po zapisie wyboru panel – jak Libraries Editor legacy – od razu zleca synchronizację nowo włączonych (`Synchronize{K}Libraries` + `…LibraryByIdIfNeeded`). „Synchronizuj” przy bibliotece = `Synchronize{K}Libraries` + `ForceSynchronize{K}LibraryById` bez głębokiego skanu (dla Plex też `SynchronizePlexNetworks`), jak ekran Libraries legacy; głęboki skan pominięty (rzadko potrzebny, długo trwa). Wszystkie synchronizacje idą do kolejki skanera legacy (`queued`) – postęp i błędy w Logach. Web GUI legacy przed zleceniem bierze blokadę biblioteki w procesie (`IEntityLocker`, poza MediatR); panel jej nie bierze – drugie zlecenie tej samej biblioteki trafia po prostu do kolejki. Zamiany ścieżek (ścieżka na serwerze mediów → ścieżka w kontenerze legacy) zapisuje jeden `Update{K}PathReplacements` z całą listą (`Id` 0 = nowa). **Plex**: `StartPlexPinFlow` zwraca adres `https://app.plex.tv/auth#…` i sam kolejkuje `TryCompletePlexPinFlow` (legacy przez 2 minuty sprawdza PIN i po zalogowaniu dopisuje serwery konta) – panel otwiera ten adres w nowej karcie, pokazuje go też jako odnośnik (gdy przeglądarka zablokuje okno) i przez ~2 minuty odświeża listę serwerów. Token Plex zostaje w legacy. Czy konto jest zalogowane, legacy mówi tylko przez magazyn sekretów (poza MediatR) – panel pokazuje „Wyloguj” tylko, gdy są serwery. `TryCompletePlexPinFlow` (wewnętrzny PIN) nie jest na białej liście. **Strumienie zdalne** to w legacy pliki `.yml` (`url` albo `script`, `title`, `duration`, `is_live`, `fallback_query`…) w bibliotece lokalnej rodzaju „strumienie zdalne” – obsługują je już Biblioteki i Przeglądarka (R3); zgodnie z decyzją R2 panel nie zapisuje tych plików (krok ręczny: plik na wolumen `/config`, potem „Skanuj teraz”). Testy: e2e łączy legacy z atrapą serwera Jellyfin (mały serwer HTTP w `panel/tests/e2e.cjs`, port 18096, legacy widzi go przez bramę sieci Dockera; inny adres: `E2E_FAKE_HOST`), Plex – adres PIN podstawiony w przeglądarce (bez plex.tv). Na prawdziwym Jellyfin/Emby/Plex nie sprawdzone (krok ręczny przed wdrożeniem: połączyć, zaznaczyć bibliotekę, sprawdzić pozycje w Przeglądarce).

**VizRS Live Panel wchłonięty i usunięty (2026-11-07).** Wszystko, co dawał VizRS Live Panel CG Playout (`/live`: zakładki „Na żywo” i „Audycje LIVE”), jest w panelu: ekrany Sloty na żywo i Audycje LIVE (§12), wskaźnik w pasku górnym (`studio na antenie: 24` / `studio wstrzymane` / `CG Playout: połączony` – zielony, studio: ramówka; `CG Playout: brak połączenia`; co 5 s, klik → Sloty) i **nazwa stanowiska** (przycisk „operator: …” w pasku górnym, pamiętana w przeglądarce). Logika zostaje w CG Playout (odbiór sygnału, przejęcie kanału, pliki audycji, `control.json`) – panel woła jego API przez pośrednika. Nazwa stanowiska idzie w nagłówku `X-Operator` (przeglądarka koduje `encodeURIComponent`, serwer dekoduje, usuwa znaki sterujące, tnie do 40 znaków) do CG Playout jako `VizRS Live · nazwa` – trafia do zdarzeń i dziennika CG (`<etv_config>/vizrs-live/log.jsonl`); bez nazwy – `VizRS Live`. Różnice wobec `/live` CG: zamiast odnośników do web GUI legacy – Harmonogramy panelu; biblioteka z listy legacy zamiast ID; ZDEJMIJ jak w `/live` (dwa kliknięcia). Strony `/live` w CG Playout już nie ma (usunięta 2026-11-07 razem z logowaniem i ciasteczkiem `vizrs_panel`; `/live` = 404) – panel jest jedynym stanowiskiem operatora; CG Playout daje tylko API `/api/live/*` z tokenem w nagłówku (docs/API.md §1). Ustawienie `etv_public` (odnośniki strony `/live` do web GUI legacy) zniknęło razem z nią.

**Sloty na żywo – jak nadawać (2026-11-07).** Przy każdym kanale panel pokazuje dane dla nadawcy z pola `inputs` w `/api/live/state` VizRS CG Playout: **RTMP** – Serwer (`rtmp://host:port/app`) i Klucz strumienia osobno, jak pyta o nie OBS (Ustawienia → Strumień → „Własny…”); **SRT** – pełny adres `srt://host:port?mode=caller&passphrase=…` i hasło. Klucz, adres SRT i hasło są ukryte (pole hasła) z „Pokaż” i „Kopiuj” – to sekrety nadawania, a CG podaje je tylko z tokenem panelu. Host: `VIZRS_INGEST_PUBLIC` w CG Playout, a gdy pusty – host, z którego operator otwiera panel. Wejścia zna CG Playout z `VIZRS_INGEST` (pierwsze wypełnienie) albo z listy ustawionej w panelu (niżej); kanał bez wejścia ma przycisk „+ Dodaj wejście”.

**Sloty na żywo – ustawianie wejść (2026-11-08).** „Ustaw wejścia…” (panel Wejścia studia) i „+ Dodaj wejście” (kanał bez wejścia) otwierają edytor całej listy wejść CG Playout (`GET/PUT /api/live/inputs`, VizRS CG Playout z 2026-11-08): wiersz = kanał, RTMP albo SRT, port, klucz strumienia (RTMP) albo hasło (SRT, 10–79 znaków, puste = bez hasła), „Losuj” (24 znaki szesnastkowe z `crypto.getRandomValues`), „×”. Porty tylko z zakresu, który compose CG wystawia na hoście (RTMP 1936–1943, SRT 9000–9007/UDP; CG podaje go w `ports`), po jednym wejściu na port. Panel składa adresy odbiorników (`rtmp://0.0.0.0:<port>/live/<klucz>`, `srt://0.0.0.0:<port>?passphrase=<hasło>`; inne parametry SRT z istniejącego adresu zostają) i wysyła całą listę; CG Playout ją sprawdza (błąd = toast z jego komunikatem, nic się nie zmienia), zapisuje w `<etv_config>/vizrs-live/inputs.json` – od tej chwili lista z panelu wygrywa z `VIZRS_INGEST` – a usługa odbiorników przejmuje ją w ciągu ok. 5 s bez restartu (zmienione odbiorniki startują od nowa, trwający na nich sygnał się urywa; reszta działa dalej). Adres dla nadawców (`VIZRS_INGEST_PUBLIC`) zostaje w konfiguracji CG Playout – to ustawienie serwera, nie kanału. Starsze CG Playout bez `/api/live/inputs`: toast, że wejścia ustawia się tam w `VIZRS_INGEST`. Pod kartami kanałów panel „Wejścia studia” zbiera w tabeli wszystkie wejścia (RTMP, SRT) z adresem serwera bez sekretów, bieżącym stanem („nadaje · na antenie”, „nadaje”, „wstrzymany”, „czeka”) oraz parametrami obrazu i dźwięku. **Wejście YouTube** (2026-11-18): rodzaj „YouTube” zamiast portu i klucza ma pole adresu (`https://www.youtube.com/…`, `https://youtu.be/…`; panel sprawdza go przed zapisem i pod polem mówi, czy to strona live kanału, czy jeden film), w „Jak nadawać” karta „YouTube (CG Playout pobiera sam)” z adresem, w tabeli rodzaj YouTube; działanie – §1, Live ingest. Starszy CG Playout odrzuca adres YouTube (toast z jego komunikatem). **Wejście z linku** (2026-11-18): rodzaj „Link (HLS, RTMP, SRT, RTSP)” ma pole adresu strumienia (`http(s)://`, `rtmp(s)://`, `srt://`, `rtsp(s)://` z hostem spoza tej maszyny (nie `0.0.0.0`, `[::]`, `127.*`, `localhost`); zły adres – podpowiedź pod polem i toast przy zapisie, bez wysyłki); w „Jak nadawać” karta „Link do strumienia (CG Playout pobiera sam)” z adresem ukrytym jak klucz („Pokaż”, „Kopiuj” – link bywa z tokenem), w tabeli rodzaj Link i adres bez loginu, zapytania i klucza RTMP (`?***`, `/***`); działanie – §1, Live ingest. Starszy CG Playout odrzuca link (toast z jego komunikatem). **Wybór wejścia kanału** (2026-11-18, CG Playout z cechą `input_select`): karta kanału z co najmniej dwoma wejściami ma listę „Wejście:” – „automatycznie – pierwsze z sygnałem” i wejścia kanału po nazwie z CG Playout (`label`, bez sekretów) ze stanem (● nadaje, ● sygnał czeka, ○ nie pobiera); zmiana od razu wysyła `PUT /api/live/channels/<n>` `{"input"}` i odświeża widok, a toast mówi, co wybrano. W tabeli „Wejścia studia” stan każdego wejścia osobno (nadaje · na antenie, sygnał czeka, nie wybrane, czeka) i znaczek „wybrane”; obraz i dźwięk tylko przy wejściu, które nadaje. Starszy CG Playout (bez `state` przy wejściach) – lista się nie pokazuje, tabela jak dotąd. Dziennik emisji pokazuje `input` jako „wybór wejścia”; działanie – §1, Live ingest.

**Sloty na żywo – ciasteczka YouTube (2026-11-20).** „Ciasteczka YouTube…” w nagłówku panelu Wejścia studia (CG Playout z cechą `youtube_cookies`; starszy – bez przycisku) otwiera okno ciasteczek dla yt-dlp (`GET/PUT/DELETE /api/live/youtube-cookies`, pośrednik `live/youtube-cookies`). Na górze stan z CG Playout: brak ciasteczek, zły zapisany plik albo „Wgrane <data>: n z youtube.com (razem m)” z „Konto zalogowane, ważne do <data>” (zielono; żółto, gdy zostało mniej niż 7 dni; czerwono po terminie albo bez ciasteczek zalogowanego konta) i – gdy CG Playout podaje `checked_at` (26.11.25) – „Sesja sprawdzona i podtrzymana <data>” (ostatnie udane podtrzymanie przez usługę `ingest`, §1); sesja zakończona przez YouTube – na czerwono, do tego alarm w banerze (K17). Niżej krótka instrukcja (osobne konto Google, okno prywatne, eksport rozszerzeniem przeglądarki w formacie cookies.txt / Netscape albo JSON, zamknięcie okna bez wylogowania), wybór pliku (`.txt`, `.json`, do 512 KB; treść trafia do pola tekstowego) albo wklejenie treści. **Zapisz** wysyła treść do CG Playout, który ją sprawdza (błąd = toast z jego komunikatem, stary plik zostaje); toast mówi, ile ciasteczek youtube.com przyjęto i czy konto jest zalogowane. **Usuń ciasteczka** (tylko gdy są) – po potwierdzeniu. Panel nie dostaje wartości ciasteczek z powrotem; działanie – §1, Live ingest.

**Grafika VizRS i sloty na żywo – szczegóły i decyzje (R9, 2026-11-07).** Panel rozmawia z VizRS CG Playout wyłącznie przez jego API (decyzja planu R9), przez pośrednika w serwerze (`/api/cg/…`, docs/API.md §2) – adres i token CG nie trafiają do przeglądarki. Spis API CG Playout, z którego panel korzysta: `/health`, `/etv/pages`, `/ingest/state` (pośrednik gotowy, UI używa stanu z `/api/live/state`, który zawiera to samo per kanał) i live API CG (`/api/live/*`, wymaga `panel_token` w CG Playout; docs/API.md §1). Decyzje: (1) **Reguły grafiki** (`on_clip`, `on_end`/`end_seconds`, `on_ad`/`ad_match`, `shows`, `channels` – kiedy która strona VizRS wchodzi, z polami `{title}`, `{next_title}`…) panel czyta i zapisuje przez API CG Playout `GET/PUT /api/live/graphics` (dodane w CG Playout 2026-11-07, docs/API.md §1; pośrednik: `live/graphics`, GET i PUT). Zapis wysyła cały dokument reguł; CG Playout go waliduje (błędny regex = 400, panel pokazuje komunikat, nic się nie zmienia), zapisuje w `<etv_config>/vizrs-live/graphics.json` – od tej chwili plik wygrywa z regułami z `config.json` (w panelu „z panelu” / „z config.json”) – i stosuje od następnej pozycji, bez restartu. W regułach audycji i kanałów tryb pola „jak globalnie” = pole pominięte, „wyłączone” = pusty tekst w CG (dawne „-” z pola tekstowego panel czyta jako wyłączone); w karcie reguły kanału pole „Na przejęcie przez live ingest” (`on_live`, np. `TAKE 7510` dla logo „NA ŻYWO”) definiuje komendy na czas przejęcia kanału przez live ingest nakładki ffmpeg – grafika ramówki jest wtedy wstrzymywana, a po zejściu studia wraca automatycznie (pole pokazywane, gdy CG Playout ma cechę `on_live` w `features`; starszy CG ukrywa pole, a zapisane wcześniej `on_live` zachowuje; puste pole = pominięcie klucza); komendy edytuje się jako wybór grafik (`cmdedit.js`, §11 UI) – zapis w tej samej składni tekstowej; reguły audycji przypisane do kanału i `ad_match` audycji panel przenosi bez zmian (nie ma ich w edytorze). Na górze ekranu reguły streszcza tabela „Kiedy / Co / Ile” (globalne, audycji i kanałów z komendami opisanymi po polsku – akcja, strona, pola, wiersz „przejęcie przez live ingest (grafika ramówki wstrzymana)” z czasem „na czas przejęcia” – lub pigułką „wyłączone” / „OUT” oraz czasem trwania), z której kliknięcie przenosi kursor wprost do edytora pod nią (karta reguły się rozwija). Adres, token i `output_url` kanału zostają w `config.json` CG Playout. Starsze CG Playout bez tego API: panel pokazuje, że reguły zmienia się w `config.json`. (2) **Audycje na żywo jako pozycje ramówki**: CG Playout zakłada plik Remote Stream i Smart Collection `LIVE · tytuł` w legacy; pozycję harmonogramu (start Fixed, Strict, Smart Collection, One) operator dodaje w panelu w Harmonogramach (R1) – panel tego nie automatyzuje, bo CG zmienia legacy przez własne API, a zapis harmonogramu z dwóch miejsc groziłby nadpisaniem. (3) **Elementy graficzne legacy** (pliki YAML) – tylko lista i `RefreshGraphicsElements` (legacy ponownie czyta katalog elementów); element VizRS (`vizrs.yml`) zakłada CG Playout przy synchronizacji, więc zapis plików z panelu nie był potrzebny i ogólnego endpointu zapisu w moście nie dodano (decyzja R2 bez zmian). (4) **Podgląd `/output` i Trio** – ramka i odnośniki z `--vizrs-url` (adres VizRS widoczny dla przeglądarki operatora; CG Playout nie podaje swojego `output_url` przez API). VizRS z tokenem odrzuca wejście bez tokenu (401), więc ramka dostaje token wyjścia (`--vizrs-output-token`, tylko odczyt; poprawka 2026-11-07 – wcześniej ramka i Trio dawały na produkcji „wymagany token”). Tokenu operatora VizRS panel nie zna i nie przekazuje: przy pierwszym wejściu do Trio w danej przeglądarce operator dopisuje `?token=…` (podpowiedź na odnośniku), potem VizRS pamięta go w ciasteczku. Przy antenach kanałów (`--vizrs-channels`) ramka idzie za wybranym kanałem (w Grafice kanału – lista stron VizRS, na Antenie – lista w nagłówku ramki, wybór przeżywa odświeżenie), a „Trio ↗” na karcie kanału otwiera jego antenę. Ustawienia live API CG: biblioteka Remote Streams – w Audycjach LIVE (pośrednik `PUT live/settings`, od 2026-11-07); publicznego adresu legacy (`etv_public`) CG Playout już nie ma – służył tylko odnośnikom strony `/live` do web GUI legacy. Testy: atrapa CG Playout w `panel/tests/e2e.cjs` (127.0.0.1:18700; panel testowy startuje z `--cg-url`); na prawdziwym CG Playout nie sprawdzone – krok ręczny przed wdrożeniem: ustawić `panel_token` w CG i `VIZRS_LIVE_CG_URL`/`VIZRS_LIVE_CG_TOKEN` w panelu, otworzyć Grafikę kanału (stan „połączony”, strony VizRS) i Sloty na żywo (sygnał z OBS, ZDEJMIJ/WPUŚĆ, audycja LIVE).

**Pliki YAML, skrypty i JSON – decyzja (R2, 2026-11-06): panel ich nie edytuje.** Operator kładzie pliki na wolumen konfiguracji legacy (`ETV_CONFIG` → `/config` w kontenerze; proponowany katalog `/config/schedules/`, skrypty `/config/scripts/`) i wpisuje w panelu ścieżkę w kontenerze. Zapis plików z panelu wymagałby nowego kodu C# w moście (zapis pliku w kontenerze legacy – poza ogólnym dyspozytorem) albo wspólnego wolumenu z panelem; oba rozwiązania otwierają zapis plików przez sieć, a edytor YAML bez bibliotek nie daje walidacji. Wracamy do tego, jeśli R9 (elementy graficzne legacy w YAML) będzie potrzebować zapisu plików – wtedy jeden ogólny endpoint mostu ograniczony do jednego katalogu (np. `/config/vizrs/`), z limitem rozmiaru, wspólny dla playoutów i grafiki.

**Media – szczegóły.** Ścieżki bibliotek to ścieżki **w kontenerze legacy** (np. `/config/media/…` na wolumenie konfiguracji albo zamontowany katalog) – panel to mówi przy polu i wymaga ścieżki bezwzględnej. Edycja biblioteki to jeden `UpdateLocalLibrary` z całą listą ścieżek (`Id` 0 = nowa); usunięta ścieżka znika z legacy razem ze swoimi pozycjami (pliki zostają), dlatego ✕ przy ścieżce z pozycjami pyta o potwierdzenie. Rodzaju biblioteki legacy nie pozwala zmienić. Przeniesienie ścieżki (`MoveLocalLibraryPath`) działa od razu, tylko między bibliotekami tego samego rodzaju. Postępu skanu legacy nie udostępnia zapytaniem (idzie zdarzeniami Blazora) – po „Skanuj teraz” panel przez pół minuty odświeża liczby pozycji. Wyszukiwarka dokleja `type:<rodzaj> AND (…)` jak legacy; liczby na zakładkach z `QuerySearchIndexAllItems` (pusty tekst = `*`). Kosz to wyszukiwanie `state:FileNotFound` (jak ekran Trash legacy). `RemoveMediaItems` (w planie R3) jest wewnętrzną komendą indeksu wyszukiwania – nie ma jej na białej liście; `UpdateLocalLibraryPath` to typ ścieżki w `UpdateLocalLibrary`, nie komenda. Czas wyświetlania folderów obrazów (`GetImageFolders`, `UpdateImageFolderDuration`) i skan pojedynczego serialu (`QueueShowScanByLibraryId`) są na liście, bez UI (R4 ich nie potrzebował – do dołożenia przy Bibliotekach). Zaznaczone pozycje bieżącej zakładki trafiają do kolekcji zwykłej albo playlisty („Dodaj zaznaczone do…”; zaznaczenie znika po zmianie zakładki lub zapytania).

**Kolekcje i playlisty – szczegóły (R4).** Pozycje kolekcji zwykłej: `GetCollectionCards` (w planie „GetCollectionItems” – takiej komendy nie ma), karty wszystkich rodzajów w jednej tabeli; dodaje się je w Przeglądarce (`AddItemsToCollection` z listą id jednego rodzaju – pojedyncze `Add{Movie,Show,…}ToCollection` są zbędne i nie są na liście). Własna kolejność (`UpdateCollectionCustomOrder`) – legacy pozwala ją tylko w kolekcji z samych filmów; nie wdrożone. Inteligentna: liczba wyników to suma list z `QuerySearchIndexAllItems` dla zapytania. Wielokolekcja: legacy trzyma składniki zwykłe i inteligentne osobno (`Items`, `SmartItems`), bez wspólnej kolejności; składnik „jako grupa” gra chronologicznie (jak w Multi Collection Editor legacy, panel wysyła `PlaybackOrder` = `Chronological`); `GetAllMultiCollections` nie zawsze zwraca składniki – edytor czyta `GetMultiCollectionById`. Powtórki: lista z `GetPagedRerunCollections` (bez nazw treści), edytor z `GetRerunCollectionById`; kolejności jak w Rerun Collection Editor (wielokolekcja – tylko tasowanie). Playlista: nazwę i pozycje zapisuje jeden `ReplacePlaylistItems` (`Index` 0, 1, … wg tabeli; „wszystkie” wysyła `Count` = `null`); pojedyncze pliki (film, inne wideo, utwór…) dodaje Przeglądarka (`AddItemsToPlaylist`), w edytorze można je przesuwać i ustawiać. Playlist systemowych (grupa „Trakt Lists”) legacy nie pozwala edytować. Usunięcie grupy usuwa jej playlisty. Trakt (`AddTraktList`, `DeleteTraktList`) – nie wdrożone (opcjonalne, wymaga klucza API Trakt).

**Wyniki zadań w tle (K6, 2026-11-09).** Zadania w tle legacy (most zwraca `queued`) nie mają wyniku w odpowiedzi – legacy zapisuje go tylko w swoim logu. Po zleceniu „Sprawdź nakładanie pozycji” (Playouty) i zadań konserwacji (Diagnostyka) panel (`panel/server/web/tasks.js`, `followTask`) co 2,5 s czyta `GetRecentLogEntries` (strona 200 wpisów, filtr tekstu legacy) i bierze wpisy z czasem po zleceniu (zapas 5 s na różnicę zegarów), pasujące do zadania; pierwszy pasujący kończy czekanie, a wynik pokazuje okno (czas, poziom, wpis legacy). Czekanie: 20 s (szybkie zadania), najwyżej 60 s; przejście na inny ekran przerywa czekanie (toast „wynik w System → Logi”, bez okna nad cudzym ekranem). Wzorce komunikatów z kodu legacy: nakładanie – `Playout for channel {kanał} has overlapping playout items; this may be a bug.` (Warning, `CheckForOverlappingPlayoutItemsHandler`; legacy pisze go **tylko** przy znalezionym nakładaniu – brak wpisu = ramówka bez nakładania, tak też mówi okno); konserwacja – `Deleted {n} orphaned artwork|actors`, `Could not delete artwork file`, `… garbage collection` (poziom Debug – widać je przy progu logów Debug; przy domyślnym Information okno mówi, że wpisu nie ma i gdzie zmienić próg). `overlapWarnings(od)` – te same wpisy dla kontroli ramówki (K9).

**Paczka diagnostyczna legacy (LUKI #27, decyzja K5 2026-11-09):** `ArchiveTroubleshootingResults` tworzy plik ZIP w katalogu tymczasowym kontenera legacy i zwraca tylko jego ścieżkę, a `ArchiveMediaSample` – tak samo dla próbki pliku. Przez dyspozytor da się zlecić utworzenie, ale nie pobrać pliku: potrzebny byłby nowy endpoint C# w moście czytający dowolną ścieżkę z dysku legacy (ryzyko większe niż korzyść). Komendy nie są na białej liście; paczkę zbiera się ręcznie (RUNBOOK §6 – `docker compose exec ersatztv` i `etv-config/logs`).

**Profile, znaki wodne, wypełniacze – szczegóły (R5).** Profil: panel pokazuje pola silnika Next; pola tylko starego silnika legacy (wątki, tryb pasów, profil i preset H.264, B-klatki, wyświetlacz VAAPI, dodatkowe ramki QSV, normalizacja kolorów) zostają bez zmian – przy nowym profilu z `NewFFmpegProfile`. Informacja o sprzęcie to lista z `GetSupportedHardwareAccelerationKinds` (co zgłasza ffmpeg legacy); dokładniejszy stan wg Next (`GetNextProbe`, legacy ≥ 26.12.0) – podpowiedź K5 w tabeli §12. Akcelerację V4L2 panel pomija (legacy jej nie włącza). Znak wodny: obraz zapisany w legacy panel pokazuje przez `--stream-url` (`/artwork/watermarks/…`); bez tej opcji widać tylko świeżo wgrany plik. Makieta kadru jest przybliżona (rozmiar oryginalny liczony względem 1920 px). Usunięcie znaku: legacy odpina go od kanałów. Wypełniacz: jak w legacy – miejsca nie da się zmienić po założeniu, ogon i zapasowy nie mają trybu ani playlisty, playlista wymusza tryb „liczba”, wartości spoza trybu są wysyłane jako `null`. Wgrany, a niezapisany obraz zostaje w pamięci obrazów legacy; sprząta go „Usuń osierocone grafiki” (Diagnostyka → Konserwacja; `DeleteOrphanedArtwork` usuwa z katalogu pamięci obrazów pliki, do których nie odwołuje się żadna pozycja ani znak wodny).

## 13. Znane ograniczenia (legacy v26.12.0)

- `AddProgramScheduleItem` zapisuje pozycję, ale legacy rzuca wyjątek przy budowaniu odpowiedzi (`Mapper.ProjectToViewModel`, niezaładowane znaki wodne pozycji). Nie ma jej na białej liście; panel używa `ReplaceProgramScheduleItems`, jak UI legacy.
- `ReplaceProgramScheduleItems` zwraca leniwą projekcję, której nie da się zserializować – most zgłasza sukces z ostrzeżeniem (`warning`); panel traktuje to jako sukces i przeładowuje pozycje.
- `DeleteProgramSchedule` usuwa kaskadowo playouty, które używają harmonogramu (klucz obcy `OnDelete(Cascade)` w legacy) – panel wymienia te kanały w potwierdzeniu usunięcia.
- `GetMediaItemInfo` dla pozycji zbiorczej bez pliku (serial, sezon, artysta) rzuca wyjątek (`MediaItemExtensions.GetHeadVersion`, „Specified argument was out of the range”) – Przeglądarka nie woła go dla tych rodzajów (§12). Komunikaty legacy z wyjątkiem mają stos wywołań .NET – toast pokazuje tekst do pierwszego „at Przestrzeń.Typ…” (`shortError` w `core.js`), pełny w podpowiedzi i w konsoli przeglądarki.
- `ReplacePlayoutAlternateScheduleItems` z pustą listą legacy odrzuca („must not be empty”) – panel zawsze wysyła co najmniej harmonogram główny.
- Część wyników legacy to encje bazy (np. `BuildStatus` w `GetPagedPlayouts`) – most serializuje je z pominięciem cykli; mogą być duże.
- Web GUI legacy i strumienie są na tym samym porcie 8409 – legacy nie umie ich rozdzielić. Rozdziela proxy przed legacy (R10, §15); bez proxy web GUI jest dostępne dla każdego, kto widzi port.
- (v26.12.0) `ReplacePlayoutTemplateItems`, `UpdateDefaultDeco`, `UpdateDeco`, `DeleteWatermark`… same zlecają synchronizację playoutów Next (`QueueSyncNextPlayouts`) – to odświeża serializację, nie przebudowuje playoutu; „Przebuduj” dalej potrzebne po zmianie kalendarza.
- `PreviewPlaylistPlayout` buduje podgląd z pozycji z bazy (handler ustawia `PlaylistId`, a budowniczy playoutu czyta playlistę po id), nie z pozycji wysłanych w komendzie – podgląd niezapisanej playlisty daje „has no valid items”. Panel przed podglądem zapisuje pozycje („Zapisz i pokaż podgląd”).
- Zagnieżdżone obiekty w komendach powtórek (`CreateRerunCollection`, `UpdateRerunCollection`: `MediaCollectionViewModel`, `MultiCollectionViewModel`…) most deserializuje tylko z kompletem pól konstruktora – panel wysyła pełne obiekty z pustymi nazwami (legacy czyta z nich tylko `Id`).
- `GetDecosByDecoGroupId` zwraca deco bez treści (kolekcje wypełniacza i martwego powietrza, playlisty przerw = `null`, tylko id w bazie). Zapis `UpdateDeco` na podstawie tej listy skasowałby treść – edytor deco czyta i zapisuje na podstawie `GetDecoById` (test e2e pilnuje ponownego zapisu).
- `ReplacePlayoutTemplateItems` i `UpdateDefaultDeco` nie przebudowują playoutu – trzeba „Przebuduj” albo „Od nowa”.
- Komendy `Update…Settings` nie sprawdzają zakresów (np. `UpdateXmltvSettings` przyjmuje 0 dni) – robi to tylko web GUI legacy, więc panel sprawdza sam przed zapisem. Wyjątek: dni XMLTV większe niż dni playoutu legacy odrzuca („cannot be greater than Playout days to build”).
- `RebuildSearchIndex` legacy uruchamia tylko przy starcie; wysłana przez most trafia do kolejki skanera, która jej nie obsługuje („Unsupported request type”) – nie ma jej na białej liście. Przebudowa indeksu: restart kontenera legacy.
- Odtwarzanie w Przeglądarce idzie wprost z pliku (`/api/vod`, docs/API.md §2); test odtwarzania legacy jest zapasowy. Test odtwarzania legacy (`PrepareTroubleshootingPlayback` → `StartTroubleshootingPlayback`) przekazuje między krokami wewnętrzne obiekty (`PlayoutItemResult`, proces ffmpeg) – przez ogólny dyspozytor się nie da. Panel woła wprost jego endpoint HTTP `/api/troubleshoot/playback.m3u8` (`POST /api/play`, docs/API.md §2; autoryzacja ciasteczkiem web GUI – bez logowania w legacy przechodzi). Ograniczenia legacy: fragment ma najwyżej 30 s (`PrepareTroubleshootingPlaybackHandler`), proces ffmpeg najwyżej 2 min, **jeden test naraz w całym legacy** (blokada; nowy czyści katalog `.troubleshooting`), legacy nie podaje całego pliku. Na stosie z profilem NVENC (legacy v26.12.0) część źródeł Jellyfin kończy się błędem filtra legacy `hwdownload … Invalid output format nv12` (kod 234) – pomaga profil bez akceleracji sprzętowej; pliki lokalne z tym samym profilem grają (pomiar 2026-11-12).
- Emisja kanału w legacy jest na żądanie: „Zatrzymaj emisję” kończy sesję, ale następne żądanie widza (odtwarzacz, tuner, VizRS CG Playout) uruchamia ją znowu, a bez widzów legacy sam zamyka sesję po czasie bezczynności. Trwałe zdjęcie kanału z anteny = wyłączenie kanału (Kanały, „Włączony”).
- `GetAllLanguageCodes` w obrazie legacy zwraca tylko `und` (brak danych kultur .NET) – preferowany język dźwięku wpisuje się jako kod ISO 639-2.
- Logowanie do Plex: `StartPlexPinFlow` działa przez most (adres app.plex.tv), dokończenie logowania prawdziwym kontem nie sprawdzone w testach. Stan „zalogowany” i blokady bibliotek legacy trzyma poza MediatR (`IPlexSecretStore`, `IEntityLocker`) – panel ich nie widzi.
- `Save{Jellyfin,Emby}Secrets` i `Get…Secrets` operują jednym serwerem na rodzaj (tak działa legacy); kilku serwerów Jellyfin nie da się podłączyć.

## 14. Aktualizacja legacy

1. Na stosie testowym, z osobnym katalogiem danych (nowsze legacy migruje bazę i starsze już jej nie otworzy):
   `export VIZRS_LIVE_TEST_DIR=~/.cache/vizrs-live-test-<wersja> LEGACY_IMAGE=ghcr.io/ersatztv/legacy:<wersja>`
2. `panel/tests/stack.sh up` – buduje obraz mostu na tej wersji, smoke mostu i proxy; smoke sam sprawdza, że `commandsMissing` z `GET /vizrs-bridge/health` jest puste.
3. `panel/tests/stack.sh seed && panel/tests/stack.sh test` – ramówka od zera wyłącznie przez panel i e2e.
4. Przejrzyj zmiany komend: `git diff v<stara>..v<nowa> -- ErsatzTV.Application` w źródle legacy (zmienione rekordy `Commands/*.cs`, `Queries/*.cs` = zmienione parametry; nowe pliki = nowe możliwości). Zmiany zachowania opisz w §13.
5. Dopiero potem: kopia `etv-config` i `docker compose up -d` na produkcji.

**Domyślnie najnowsze (decyzja 2026-11-07).** `panel/docker/legacy-bridge.Dockerfile`, `docker-compose.live.yml`, `.env.example` i stos testowy biorą `ghcr.io/ersatztv/legacy:latest` – tak jak `ersatztv` i `ingest` w CG Playout, żeby cały stos był na jednej wersji legacy. Obraz z mostem (`vizrs-live-legacy:local`) buduje się na miejscu (w lokalnym stosie przez `panel/compose.build.yml`): `pull_policy: build` i `build.pull: true` – każde `docker compose up` sprawdza nowsze legacy (`panel/tests/stack.sh up` – `docker build --pull`). Konkretna wersja (powrót po złej aktualizacji, test przed wdrożeniem): `LEGACY_IMAGE=ghcr.io/ersatztv/legacy:v<wersja>`. Ryzyko: nowe legacy wchodzi bez testu mostu i migruje bazę – stąd kopia `etv-config` przed `up` (RUNBOOK §4) i `commandsMissing` w Diagnostyce. Akcelerację sprzętową GPU dla usługi `ersatztv` włączają nakładki `docker-compose.vaapi.yml` (Intel, AMD) oraz `docker-compose.nvidia.yml` (NVIDIA) w korzeniu repozytorium – szczegóły w [RUNBOOK.md](RUNBOOK.md) §2b.

Wczesne ostrzeżenie: CI `panel-legacy-latest.yml` co tydzień robi kroki 1–3 na `ghcr.io/ersatztv/legacy:latest` (od 2026-11-07 to samo robi `ci.yml` przy każdym push, bo latest jest domyślne).

**Przejście 26.11.6 → 26.12.0 (R10, 2026-11-07):** smoke, `commandsMissing = []`, seed na świeżej bazie i całe e2e – zielone bez zmian w moście i panelu. Rekordy komend bez zmian; nowe: `GetNextProbe`, `RefreshNextProbe`, `GetNextDescribe` (stan silnika Next – do dopisania w Profilach, zob. R5 w §12; nie na białej liście). Kilka komend deco/szablonów/znaków sama zleca synchronizację playoutów Next (§13). Domyślny `LEGACY_IMAGE` zostaje `latest` – przypięcie wersji w `.env.example` robi wydanie 1.0 (R11).

## 15. Zamknięcie web GUI legacy (R10)

Operatorzy pracują wyłącznie w panelu. Legacy nie ma portu na zewnątrz (`expose`, nie `ports` w `docker-compose.live.yml`); panel i most rozmawiają w sieci wewnętrznej Dockera (`http://ersatztv:8409`). Na zewnątrz port 8409 wystawia **proxy** (Caddy, `panel/docker/proxy.Caddyfile`):

| Na zewnątrz (tylko `GET`/`HEAD`) | Po co |
|---|---|
| `/iptv/*` | strumienie (HLS, TS, HLS Direct, segmenty sesji), `channels.m3u`, `xmltv.xml`, logo kanałów i okładki EPG |
| `/artwork/*` | obrazy legacy tylko do odczytu – podgląd znaków wodnych i logo w panelu (przeglądarka operatora ładuje je spod `--stream-url`) |
| `/discover.json`, `/device.xml`, `/lineup.json`, `/lineup_status.json` | HDHomeRun (Plex, Jellyfin, Emby jako tuner) |
| wszystko inne, też `POST` do powyższych | **403**: web GUI (Blazor), `/api/*` legacy, `/vizrs-bridge/*` |

**Decyzje (2026-11-07, sesja automatu – zachowawczo):**
- **Caddy, nie nginx** – jeden krótki plik, dopasowanie ścieżek i metod bez warunków `if`, `flush_interval -1` dla strumieni; obraz `caddy:alpine` – najnowszy (do zmiany `PROXY_IMAGE`). TLS proxy nie robi (`auto_https off`) – jak dotąd port 8409 był zwykłym HTTP; certyfikat to osobna decyzja wdrożenia (R11).
- **`/artwork/*` otwarte** – to tylko obrazy (plakaty, miniatury, znaki wodne, adresy z identyfikatorem/skrótem), a panel z nich korzysta; zamknięcie zepsułoby podgląd w Znakach wodnych. `/api/*` legacy (np. `/api/channels`, `/api/sessions`) zamknięte – nic z zewnątrz ich nie potrzebuje.
- **HDHomeRun otwarty** – to funkcja dla widzów (tuner), nie GUI.
- Panel nie dostał osobnego adresu „wewnętrznego” legacy do obrazów: `--stream-url` = adres proxy dla widzów.
- **Sprawdzanie w panelu** (Diagnostyka → „Web GUI legacy dla widzów”): serwer panelu pyta adres `--stream-url` o `/`, `/channels`, `/vizrs-bridge/health` (oczekiwane 403/404) i `/iptv/channels.m3u` (200). Sprawdza ze swojej sieci – gdy `--stream-url` to `localhost` albo adres publiczny niewidoczny z kontenera, wynik to „nie sprawdzono”, a sprawdzić trzeba z zewnątrz (`curl -I http://<serwer>:8409/` → 403).

**Pierwsze uruchomienie legacy bez GUI.** Legacy nie wymaga konfiguracji startowej: na pustym `/config` tworzy bazę i domyślny profil kodowania (id 1), a całą resztę (biblioteka, skan, kolekcja, harmonogram, kanał, playout) robi `panel/tests/stack.sh seed` wyłącznie komendami przez panel – sprawdzone na świeżej bazie v26.11.6 i v26.12.0. Jedyne, czego trzeba przy pierwszym starcie: `xmltv.xml` pojawia się kilka–kilkanaście sekund po starcie (wcześniej legacy zwraca 500 – smoke czeka).

**Lista kontrolna pokrycia** (menu `ErsatzTV/Shared/MainLayout.razor`; ta sama lista w UI: Diagnostyka → „Pokrycie web GUI legacy”, `COVERAGE` w `panel/server/web/views/system.js`; e2e pilnuje, że każdy wiersz ma ekran albo uzasadnione pominięcie):

| Legacy | Panel |
|---|---|
| Channels (z Edit Channel Numbers) | Kanały („Numeracja…”) |
| FFmpeg Profiles | Profile kodowania |
| Watermarks | Znaki wodne |
| Media Sources → Local / Libraries | Biblioteki |
| Media Sources → Emby / Jellyfin / Plex | Źródła zewnętrzne |
| Media → Trash | Kosz |
| Media → TV Shows, Movies, Music, Other Videos, Songs, Images, Remote Streams | Przeglądarka |
| Lists → Collections, Smart, Multi, Rerun | Kolekcje |
| Lists → Playlists | Playlisty |
| Lists → Trakt Lists | **pominięte**: wymaga klucza API Trakt; listy założone wcześniej widać jako playlisty systemowe (tylko odczyt) |
| Lists → Filler Presets | Wypełniacze |
| Scheduling → Schedules / Blocks, Templates / Decos, Deco Templates / Playouts | Harmonogramy / Bloki i szablony / Wystrój (deco) / Playouty |
| Settings → FFmpeg, Logging, HDHomeRun, Scanner, Playout, XMLTV, UI | Ustawienia (UI legacy – tylko wygląd web GUI, bez znaczenia po zamknięciu) |
| Support → Health Checks / Logs | Diagnostyka / Logi |
| Support → Troubleshooting | **częściowo**: wersje i sprzęt, odnośniki do strumienia; test pliku – „Odtwarzanie” w karcie pozycji Przeglądarki (po 30 s, §13) |
| pasek: M3U, XMLTV | odnośniki w Kanałach, Na antenie, Przewodniku EPG |
| pasek: Docs, pomoc, GitHub | **pominięte**: strony projektu ErsatzTV |

**Kroki ręczne (poza repozytorium / poza sesją automatu):**
1. Produkcja: po wdrożeniu `docker-compose.live.yml` legacy traci port 8409, a dostaje go proxy – `docker compose up -d` przestawia to bez zmian w `etv-config`. Jeśli coś (np. Plex/Jellyfin jako tuner HDHomeRun, odtwarzacze) korzystało dotąd z innych ścieżek niż powyżej, sprawdzić po przejściu (`curl -I`).
2. Administrator, który wyjątkowo potrzebuje web GUI legacy: tunel SSH do sieci Dockera albo tymczasowo `ports: ["127.0.0.1:8410:8409"]` przy usłudze `ersatztv` – nie dla operatorów.
3. CI zacznie działać po wypchnięciu `.github/workflows/` na GitHub (sesja automatu nie robi push); w ustawieniach repozytorium Actions muszą być włączone. `panel-legacy-latest.yml` nie wymaga sekretów (obraz legacy jest publiczny). Krok `panel/tests/hwaccel.sh` w `panel-ci.yml` i `release.yml` weryfikuje akcelerację sprzętową w obrazie legacy przed publikacją.
