# VizRS CG Playout – dokumentacja

> Do 2026-10-31 projekt nazywał się **VirtRS CG Playout** (repozytorium `sdorpl/VirtRS-CG-Playout`, GitHub przekierowuje stary adres). Nazwy techniczne zostają bez zmian: binarka `virtrs-cg`, obraz `ghcr.io/sdorpl/virtrs-cg`, zmienne `VIRTRS_*`.

1. [Jak to działa](#1-jak-to-działa)
2. [Odpowiedniki w VizRS ffPlayout System](#2-odpowiedniki-w-vizrs-ffplayout-system)
3. [Komendy i symbole zastępcze](#3-komendy-i-symbole-zastępcze)
4. [Uruchamianie w Dockerze](#4-uruchamianie-w-dockerze)
5. [Uruchamianie bez Dockera, konfiguracja](#5-uruchamianie-bez-dockera-konfiguracja)
6. [Awarie](#6-awarie)
7. [Testy](#7-testy)
8. [Benchmark przechwytywania grafiki](#8-benchmark-przechwytywania-grafiki)

## 1. Jak to działa

```
ErsatzTV ── element grafiki „script” ──► vizrs-etv-client ──HTTP──► virtrs-cg (sidecar)
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
| `auto` (domyślnie) | renderer natywny, dopóki wszystko na antenie umie narysować; gdy na antenę wejdzie coś, czego nie umie (np. ruch na wzór BTB, obraz WebP albo wideo, font spoza wbudowanych; renderer 0.2 rysuje całe INFO24 i R7), w tej samej klatce rysuje Chromium, a po zejściu tej grafiki znów renderer natywny |

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

Przy kilku obszarach `VIZRS_AREA` jeden proces `vizrs-render` (od 0.3) dostaje
je wszystkie (`--area` powtórzone) i rysuje każdy osobno, nie ich obwiednię.
Pomiar na A1 (s2, 1600×900 25 kl./s, dolny pas i róg, sam renderer): obwiednia
22,3% rdzenia, osobne obszary w dwóch procesach 12,5%. Sidecar z tą wersją
wymaga renderera 0.3: starszy wziąłby tylko ostatni `--area`.

Koszt (x86, kanał 1280×720 25 kl./s, crawl VizRS, sidecar razem z rysującym):
Chromium ok. 56% rdzenia, renderer natywny ok. 14%. Pomiar na Ampere A1
i stan obsługiwanych elementów: VizRS `docs/plany/PLAN-native.md`.
Obraz Dockera zawiera `vizrs-render`: `Dockerfile` buduje go z wydania VizRS CG
System wskazanego przez `VIZRS_REF` (na arm64 także krzyżowo).

Pomiar: §8 (Taśma crawla).

Sidecar to jeden plik binarny `virtrs-cg` (Rust) obok bezgłowego Chromium:

| Adres | Co |
|---|---|
| `POST /etv?channel=1[&render=720p][&area=1920x140+0+930]` | treść: dane szablonu ErsatzTV; odpowiedź: niekończący się strumień pakietów ETV; `render` – rozmiar renderowania grafiki, `area` – obszar grafiki (§1) |
| `GET /ffmpeg?channel=1[&wait=2000][&area=1]` | nakładka ffmpeg przejmuje wolną sesję elementu kanału; odpowiedź: `session`, `width`, `height`, `fps` (404 = brak sesji); z `area=1` (nakładka umie ustawić grafikę) rozmiar i położenie `x`, `y` obszaru grafiki |
| `GET /ffmpeg/frames?session=N[&fps=25/1][&area=1]` | surowe klatki `yuva420p` przejętej sesji dla ffmpeg, z `area=1` w rozmiarze obszaru grafiki (Nakładka ffmpeg, niżej) |
| `POST /etv/channels` | numery i nazwy kanałów ErsatzTV od klienta, dla VizRS (§3) |
| `GET /etv/pages[?channel=1]` | strony VizRS kanału (lista elementów, §3) |
| `GET /health` | liczba sesji, stan przechwytywania (`size` kanału, `render`, `area`, `fps`, `mode`: `screencast` albo `frames`, `status`: `capturing`, `paused`, `restarting`…, `seq`) i kanały z nakładką ffmpeg (`overlay`) |

Chromium wysyła PNG tylko przy zmianie strony. Sidecar dekoduje klatkę
dopiero wtedy, gdy któraś sesja ją pokazuje (klatki, których żadna klatka
wyjścia nie bierze, zostają niezdekodowanym PNG), przycina ją do widocznych obszarów i wysyła jako jeden pakiet
`Rectangles` z osobnym prostokątem na każdą grupę grafik (np. logo w rogu
i pasek na dole to dwa małe prostokąty, a nie jeden na cały ekran;
przerwy węższe niż 32 px zostają w jednym prostokącie). Niezmienione klatki idą jako `Repeat`, więc statyczna grafika
prawie nic nie kosztuje. Animacje pod obciążeniem gubią klatki, ale nie
zostają w tyle.

Płynność animacji (rozwiązania z VizRS ffPlayout System):

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

Nakładka `vizrs-ffmpeg` ([src/overlay.rs](../src/overlay.rs)) omija ten
łańcuch, tak jak VizRS ffPlayout System mieszał grafikę we własnym ffmpeg. To
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
   `VIZRS_FFMPEG_FPS`, `VIZRS_FFMPEG_PRESET`, `VIZRS_FFMPEG_READRATE_CATCHUP`,
   `VIZRS_FFMPEG_FPS_BLEND` niżej). Płótno z parametrem
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
4. ffmpeg czyta wejście grafiki z gniazda `GET /ffmpeg/frames` przekazanego
   jako `pipe:N`. Są to surowe klatki `yuva420p` w rozmiarze kanału,
   w tempie kanału z `lead_seconds` zapasu (§1). Gdy kanał ma obszar grafiki
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

Warunki i różnice:

- Numer w `channel=` adresu elementu, jeśli jest podany, musi być numerem
  kanału ErsatzTV (bez niego klient sam go ustala, §4). Nakładka zna kanał tylko
  z adresu płótna.
- Z nakładką ErsatzTV nie rysuje własnych elementów grafiki tej pozycji
  (tekst, obraz, watermarki z płótna). Logo i napisy daj do VizRS.
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
- Na wejściu surowe klatki kosztują pasmo, ale nie CPU: 1080p to ok. 5 MB
  na klatkę, ok. 130 MB/s przy 25 kl./s. Idą lokalnie przez sieć Dockera.

Zmienne środowiskowe nakładki (w kontenerze ErsatzTV):

| Zmienna | Domyślnie | Znaczenie |
|---|---|---|
| `VIZRS_CG_URL` | – (compose: `http://cg:7700`) | adres sidecara; puste = nakładka tylko przekazuje wywołania do ffmpeg |
| `VIZRS_FFMPEG` | `/usr/local/bin/ffmpeg` | prawdziwy ffmpeg |
| `VIZRS_FFMPEG_FPS` | – | wymuszony klatkaż kanału, `50` albo `25` (także cel **Normalize Framerate**); puste = klatkaż materiałów |
| `VIZRS_FFMPEG_PRESET` | – (compose A1: `veryfast`) | preset kodera programowego (libx264, libx265), np. `superfast`; puste = preset ErsatzTV |
| `VIZRS_FFMPEG_READRATE_CATCHUP` | – (compose: `2`) | tempo nadrabiania strumienia MPEG-TS kanału po przestoju (`-readrate_catchup`); puste = bez nadrabiania |
| `VIZRS_FFMPEG_FPS_BLEND` | – (compose: `1`) | `1` = zmiana klatkażu materiału szybszego od kanału z mieszaniem klatek (`framerate` zamiast `fps`; klatkaż z `ffprobe`); puste = `fps` |

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
wznawia je i od razu dostaje świeżą klatkę. Przerwa między pozycjami
harmonogramu jest dużo krótsza, więc w trakcie emisji pauza nie występuje.

## 2. Odpowiedniki w VizRS ffPlayout System

VizRS ffPlayout System jest od 2026-10-24 archiwum: nie jest rozwijany i
zostaje jako wzorzec. Tabela pokazuje, skąd pochodzą opcje i jak przenieść
szablony komend; nowe funkcje VizRS CG Playout nie mają już odpowiednika tam.

| ffPlayout | ErsatzTV / VizRS CG Playout |
|---|---|
| Ustawienia → grafika VizRS: adres, token | `vizrs.address`, `vizrs.token` |
| komendy klipu / reklamy | `on_clip` / `on_ad` |
| URL wtapiania | `output_url` |
| reklama = `"category": "advertisement"` | `ad_match.path` / `ad_match.title` (wyrażenia regularne na ścieżce i tytule) |
| wtapianie w łańcuchu filtrów ffmpeg | nakładka ffmpeg (§1), bez niej element grafiki „script” ErsatzTV, `z_index: 100` |
| reklama zaznaczona na pozycji | pozycja harmonogramu bez elementu **VizRS** (`ad_without_element`) |
| lista `vizrs` na pozycji playlisty | brak (ErsatzTV nie ma pól na pozycję) |
| `{next_*}` z playlisty | z wpisów EPG ErsatzTV (`epg_entries: 3`) |
| przechwytywanie co n-tą klatkę Chromium, wtapianie aktualnej grafiki w każdą klatkę | to samo (§1, płynność animacji), plus zrzut stanu końcowego |
| brak – grafika zawsze w rozmiarze kanału | `render_size` / `&render=` (rozmiar renderowania grafiki, §1) |

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
| `{next_start}` / `{next_start_full}` | start następnego, `hh:mm` / `hh:mm:ss` |
| `{next_duration}` | długość następnego `hh:mm:ss` |

`|` w wartościach zamienia się na `/`, a znaki końca linii są usuwane.
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

Przy `channel_buses` klient `vizrs-etv-client` odczytuje kanały z API ErsatzTV
(klucz z `api-secrets.json` zostaje w jego kontenerze) i podaje numery i nazwy
sidecarowi (`POST /etv/channels`), a ten zakłada je w VizRS. Lista **Antena** w Trio
pokazuje więc kanały ErsatzTV z ich nazwami. Port API to 8409.

Obiekt `channels` nadpisuje ustawienia kanału: komendy, `shows`, a także własny
VizRS (kanał z `vizrs.address` nie używa wtedy anteny kanału). Czego nie podasz,
zostaje z ustawień głównych:

```json
"channels": {
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
- Pierwsza pasująca reguła z listy `shows` zastępuje domyślne `on_clip`, `on_ad`
  i/lub `ad_match` (pola pominięte przyjmują wartości globalne z konfiguracji).
- Jeśli audycja nie pasuje do żadnego wpisu z `shows`, stosowane są ustawienia
  globalne.

### Osobne elementy grafiki w ErsatzTV (`page=<ID>` / `take=<ID>`) i synchronizacja (`virtrs-cg sync`)

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
| strona znika (usunięta, inny show) | zostaje z nazwą `[brak w VizRS] Ticker (3001)`; `TAKE` da `ERR` w logu; znacznik znika, gdy strona wróci |

**Pliki nie są kasowane**, bo ErsatzTV usuwa element, którego plik brakuje przy
odświeżeniu, a z nim – kaskadowo i bezpowrotnie – przypisania w harmonogramach,
Deco i pozycjach playoutu. Z tego samego powodu pliki podmieniane są atomowo
(`rename`), bez chwili, w której pliku nie ma. Nazwa pliku nie ma znaczenia:
stroną elementu jest parametr `page=` (lub `take=`) w jego URL. Plik bez
pierwszej linii `# Generated from VizRS CG System page …` jest ręczny:
synchronizacja go nie zmienia, a jego strona nie dostaje drugiego elementu.

Automat zostawia URL istniejącego pliku, więc element raz utworzony trzyma swój
kanał. Elementy są wspólne dla całego ErsatzTV: przy kilku kanałach na jednym
sidecarze `page-<ID>.yml` należy do kanału, który utworzył go pierwszy, a dla
pozostałych skopiuj plik z innym `channel=` (bez pierwszej linii). Elementy
utworzone przez klienta bez `channel=` są go pozbawione i działają na każdym kanale.

Ręcznie, np. żeby przenieść elementy na inny kanał, służy polecenie:

```sh
virtrs-cg sync [--url http://vizrs:8088] [--token <TOKEN>] [--channel 1] [--cg-url http://cg:7700] [--output-dir <DIR>] [--prune]
# albo z pliku projektu show.json:
virtrs-cg sync --file seed/vizrs-data/show.json --output-dir <DIR>
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
następnej). Oś bierze się z pierwszej sesji pozycji: czas emisji
(`MediaItem_Start` + `MediaItem_StreamSeek`), gdy sesja przyszła najwyżej
`lead_seconds` + 2 s przed nim, inaczej chwila startu sesji – tak jak tempo
strumienia klatek.

Kolejne sesje tej samej pozycji (ErsatzTV transkoduje ją kawałkami, widzowie
dołączają) nie wysyłają komend ponownie; kilka elementów na jednej pozycji
planuje się niezależnie. Pozycja krótsza niż `in_seconds` + `out_seconds`
albo już w ostatnich `out_seconds` nie dostaje grafiki wcale. Bez czasów
pozycji w danych ErsatzTV komendy idą od razu, bez `OUT`. Reklama (`ad_match`,
pozycja bez elementu) dostaje `on_ad` od razu, a grafika zaplanowana przed nią
nie jest już wysyłana. `out_seconds` ujemne = bez `OUT` (grafika zostaje do
reklamy); razem z `in_seconds: 0` to dawne zachowanie (komendy od razu, bez
zejścia).

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

Obraz sidecara `ghcr.io/sdorpl/virtrs-cg` (amd64 i arm64, zmienna
`CG_IMAGE`) buduje workflow wydania po znaczniku `v*`
(`.github/workflows/release.yml`); pakiet
w ghcr musi być publiczny albo maszyna zalogowana (`docker login ghcr.io`).
Bez opublikowanego obrazu dodaj `--build` do `up` – zbuduje go sama A1
(kilka minut). Klient elementu i nakładka ffmpeg (`vizrs-etv-client`,
`vizrs-ffmpeg`, §1) powstają zawsze przy budowie obrazu `seed`: na A1
natywnie dla arm64, statycznie (musl), więc działają w obrazie ErsatzTV arm64
bez dodatkowych pakietów. Na x86 binarka arm64 kompiluje się krzyżowo (ok. 40 s), przez
QEMU idzie tylko instalacja pakietów obrazu.
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
[docs/plany/PLAN-ampere.md](plany/PLAN-ampere.md). Oracle
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
Chromium nie potrzebują GPU. ErsatzTV wysyła je na kartę (`format=yuva420p,hwupload_cuda`)
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
wynik w `target/release/virtrs-cg`, programy ErsatzTV w
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
virtrs-cg config.json      # albo CONFIG=config.json virtrs-cg
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
  "lead_seconds": 4,
  "render_size": "",
  "area": "",
  "max_fps": 0,
  "capture_mode": "frames",
  "clock_offset_seconds": 0,
  "tape": false,
  "renderer": "chromium",
  "renderer_bin": ""
}
```

Obiekt `channels` (opcjonalny, env `VIZRS_CHANNELS` z tym samym JSON-em; błędny
JSON w env trafia do logu i jest pomijany) daje kanałowi własny VizRS (§3).

Tablica `shows` (opcjonalna) pozwala nadpisać `on_clip`, `on_ad` i `ad_match`
dla wybranych audycji (§3, Elementy graficzne per audycja).
Pusty `output_url` = tylko komendy, bez wtapiania. Wzór: `config.example.json`.

Zmienne środowiskowe (nadpisują plik): `LISTEN`, `VIZRS_ADDRESS`,
`VIZRS_TOKEN`, `VIZRS_OUTPUT_URL`, `VIZRS_ON_CLIP`, `VIZRS_ON_AD`
(`\n` rozdziela linie), `VIZRS_AD_PATH`, `VIZRS_AD_TITLE`, `VIZRS_AD_WITHOUT_ELEMENT=0`
(wyłącza reklamę po braku elementu), `VIZRS_IN_SECONDS`,
`VIZRS_OUT_SECONDS` (wejście i zejście grafiki, §3), `LEAD_SECONDS`, `VIZRS_RENDER_SIZE`
(`720p`, `1280x720`, puste = rozmiar kanału; §1), `VIZRS_AREA` (obszary
grafiki `SZERxWYS+X+Y` po przecinku, np. `1920x140+0+930,160x160+1690+30`,
puste = cały kanał; §1), `VIZRS_MAX_FPS`
(górna granica tempa przechwytywania, np. `25`; puste albo `0` = klatkaż
kanału), `VIZRS_CAPTURE_MODE` (`frames` albo `screencast`, niżej),
`VIZRS_CLOCK_OFFSET` (sekundy dodane do zegara strony grafiki, niżej),
`VIZRS_TAPE=1` (taśmę crawla przesuwa sidecar, §1; `0` wyłącza),
`VIZRS_RENDERER` (`chromium`, `native` albo `auto` – kto rysuje grafikę, §1;
błędna wartość = `chromium` z ostrzeżeniem), `VIZRS_RENDERER_BIN` (program
`vizrs-render`; puste = obok sidecara albo w `PATH`),
`VIZRS_BROWSER`, `VIZRS_BROWSER_NO_SANDBOX=1` (root / kontenery),
`VIZRS_BROWSER_ARGS` (dodatkowe flagi Chromium, rozdzielone spacją),
`DEBUG=1`.

Nieznana opcja w pliku trafia do logu jako ostrzeżenie. Zły typ wartości
(np. `"no_sandbox": "yes"`) zatrzymuje start z nazwą opcji; liczbę można
podać także jako tekst (`"lead_seconds": "6"`). `LEAD_SECONDS`,
`VIZRS_IN_SECONDS`, `VIZRS_OUT_SECONDS`, `VIZRS_MAX_FPS` i `VIZRS_CLOCK_OFFSET`, które nie są liczbą, dają ostrzeżenie i zostawiają
poprzednią wartość.

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

**Bezpiecznik płynności** (tryb `frames`): gdy w oknie 5 s Chromium nie zdąży
narysować ponad 10% klatek, rysuje połowę klatkażu, nie mniej niż 24 kl./s
(50 → 25, 60 → 30; kanału 25 nie obniża). Połowa dzieli się całkowicie, więc
ruch zostaje równy: każdy obraz trwa dwie klatki. Gdy przez 60 s rysowanie
zajmuje mniej niż 40% czasu, klatkaż wraca; nieudany powrót podwaja ten czas
(do 1 h). `/health`: `drawn_fps`, w logu `too slow for the channel's rate`.
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

## 7. Testy

```sh
cargo test                 # moduły i wektory referencyjne, Chromium (pomijany bez przeglądarki)
cargo build --release
npm test                   # testy kontraktu na target/release/virtrs-cg, vizrs-etv-client i vizrs-ffmpeg
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
  przeglądarki pomijany. Inną binarkę wskazuje `VIRTRS_CG_BIN`, przy
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
(`git tag v2.0.6 && git push origin v2.0.6`) albo ręcznie (Actions → Release
→ Run workflow, z wersją; powstaje wtedy szkic wydania). Buduje:

- archiwa `VizRS-CG-Playout-<wersja>-linux-x86_64.tar.gz` i `…-linux-arm64.tar.gz`
  (binarka `virtrs-cg`, `client/` z `vizrs-etv-client` i `vizrs-ffmpeg` –
  statycznymi, musl –, `etv/`, `config.example.json`), arm64 natywnie na runnerze arm,
- obraz `ghcr.io/<właściciel>/virtrs-cg:<wersja>` i `:latest` (amd64 i arm64),
- wydanie na GitHubie z archiwami i `SHA256SUMS.txt`.

Wersja z łącznikiem (`v2.1.0-rc1`) to wydanie wstępne: obraz dostaje tylko
swój znacznik, `:latest` zostaje bez zmian.

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
   w **Images** `virtrs-cg-bench:local`.

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
  kl./s): [docs/plany/PLAN-plynnosc.md](plany/PLAN-plynnosc.md) §4.

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
