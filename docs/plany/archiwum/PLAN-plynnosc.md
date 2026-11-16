# PLAN: płynność animacji grafiki (ticker)

> Status (2026-10-25): **etapy 1–4 i 6 zrobione** (6: obszary grafiki, także kilka na kanał; gałąź `lzejsza-grafika`) – miara równych kroków w benchmarku, tryb `capture_mode: frames` (domyślnie wyłączony), pomiary na x86 (§4), dokumentacja. Otwarte: pomiar na Ampere A1 i ewentualne włączenie `frames` w `docker-compose.ampere.yml`. Etap 5 nie podjęty (warunek niespełniony).
> Powiązane: [DOKUMENTACJA.md](../../DOKUMENTACJA.md) §1 (Płynność animacji, Klatkaż kanału), §8 (benchmark).

## 0. Cel

Przewijany ticker i inne ciągłe animacje mają się poruszać równymi krokami w każdej klatce wyjścia: 25 i 50 kl./s przy materiałach europejskich, bez dokładania CPU (Ampere A1, 2 CPU sidecara).

## 1. Diagnoza: skąd szarpanie

### 1.1 Dudnienie częstotliwości Chromium i kanału (główna przyczyna)

Chromium składa stronę w 60 Hz (`BROWSER_FPS`). Screencast bierze co n-tą klatkę (`every_nth_frame`, `src/capture.rs`). Klatka wyjścia pokazuje ostatnią klatkę Chromium sprzed swojej chwili (minus 100 ms):

| Kanał | Przechwytywanie | Na 1 s | Skutek |
|---|---|---|---|
| 25 kl./s (też A1 z `VIZRS_MAX_FPS=25`) | co 2. = 30/s | 6 źródła na 5 wyjścia | podwójny krok 5×/s |
| 50 kl./s (`VIZRS_FFMPEG_FPS=50`) | każda = 60/s | 6 na 5 | podwójny krok 10×/s |
| 30 kl./s | co 2. = 30/s | 1:1 | równo |
| 60 kl./s | każda = 60/s | 1:1 | równo |

To zachowanie deterministyczne, niezależne od obciążenia. Przy równym przewijaniu tekstu widać je najbardziej. Pomiar (§4) pokazał dodatkowo, że pod obciążeniem Chromium nie rysuje równo co 1/60 s, więc także 30 i 60 kl./s (1:1 w tabeli) nie dają równego ruchu.

### 1.2 Koszt klatki przy ciągłej animacji

Każda klatka tickera jest nowa. Chromium koduje PNG **całego okna**, bo screencast nie ma przycinania. Sidecar ją dekoduje, wyznacza prostokąty, skaluje (`render_size`) i robi `yuva420p`. Pod limitem CPU klatki wypadają (pomiary A1, DOKUMENTACJA §4). To druga przyczyna, ważna głównie na A1.

## 2. Rozważone kierunki

### 2.1 Ticker w osobnym procesie (osobna przeglądarka) – nie teraz

Sam osobny proces nie usuwa dudnienia z 1.1, bo każda przeglądarka ma te same 60 Hz. Pomaga tylko na koszt z 1.2 i tylko wtedy, gdy ticker ma własne, małe okno (np. 1920×80):

- plus: kilkanaście razy mniej pikseli PNG na klatkę; stojąca reszta grafiki idzie jako `Repeat`; praca dzieli się na rdzenie;
- warunek: VizRS musi wystawiać `/output` z jedną warstwą albo elementem. Integracja idzie tylko przez protokół; sprawdzić w `../VizRS CG System/docs/API.md`;
- minus: rozjazd wejść i wyjść grafik między dwiema stronami (dwa zegary, różne opóźnienia); mieszanie alfy dwóch źródeł w sidecarze (pakiety ETV i `yuva420p`; dziś jedna przeglądarka na klucz rozmiaru, `--renderer-process-limit=1`); ok. 150–250 MB RAM i stały koszt drugiej przeglądarki przy 2 CPU na A1.

Wracamy do tego dopiero, gdy po etapie 2 (BeginFrame) na A1 nadal brakuje CPU.

### 2.2 Klatkaż kanału 30 albo 60 kl./s – tylko przy takich materiałach

Przy 30/60 kl./s grafika idzie 1:1 (tabela 1.1). ErsatzTV bierze jednak klatkaż z materiałów, a kanał zmienia tylko `VIZRS_FFMPEG_FPS` (nakładka ffmpeg). Przy materiałach 25 kl./s:

- 30 kl./s: co piąta klatka filmu powtórzona, więc szarpanie przechodzi z grafiki na wideo;
- 60 kl./s: klatki filmu na przemian po 2 i 3 klatki wyjścia (60/25 = 2,4), ruch w materiale nierówny.

Do sprawdzenia także przy 30/60: faza (zegar Chromium nie jest zgrany z klatkami wyjścia; przy rozrzucie znaczników czasu możliwy krok podwójny i zaraz zerowy) i dryf (Chromium np. 59,9 Hz zamiast 60 albo 29,97 wobec 30 daje przeskok co kilka do kilkudziesięciu sekund).

Koszt: 30 kl./s prawie jak dziś (20% więcej klatek dla kodera). 60 kl./s to dwa razy więcej PNG (1080p50 na Xeonie ok. 170% CPU sidecara), ok. 310 MB/s surowych klatek 1080p i dwa razy więcej klatek dla kodera; na A1 nierealne. Na A1 z `VIZRS_MAX_FPS=25` kanał 60 kl./s dostaje każdą klatkę grafiki równo podwojoną (ruch równy, ale 30 Hz), płaci głównie x264.

**Wniosek (przed pomiarem):** materiały 30/60 kl./s → `VIZRS_FFMPEG_FPS=30` (albo 60, gdy CPU pozwala), opisać w DOKUMENTACJA §1. Materiały 25 kl./s → zostać przy 25/50 i robić etap 2.

**Po pomiarze (§4):** screencast przy 30 i 60 kl./s dał 34–57% równych kroków (720p), tyle co przy 25 i 50 – założenie o równych 60 Hz Chromium się nie potwierdziło. Klatkaż 30/60 zostaje więc tylko sprawą materiałów (DOKUMENTACJA §1, Klatkaż kanału), a równą grafikę daje `frames`.

### 2.3 Klatki Chromium sterowane przez sidecar (BeginFrame) – rekomendowane

Chromium rysuje klatkę dokładnie w chwili klatki wyjścia (25 albo 50 Hz). Dudnienie z 1.1 znika, a klatek niepotrzebnych kanałowi nie ma, więc i CPU spada. Stary benchmark (`chromium-frames`, DOKUMENTACJA §8, commit 0c54143) mierzył już: przy 720p 99% równych kroków zamiast 65%, CPU 130% wobec 149%. Przy 1080p na Xeonie wyszło 71–92%, bo procesor nie nadążał z PNG w każdym slocie. Obraz ma `chromium-headless-shell`, który obsługuje `--enable-begin-frame-control`. Nie trzeba niczego zmieniać w VizRS.

Znane pułapki (DOKUMENTACJA §8): z `--disable-new-content-rendering-timeout` każda klatka zgłasza zmianę; żądanie zrzutu zawsze ją zgłasza, a klatka po zrzucie też (pochłania ją pusta klatka zaraz po zrzucie); na stojącej stronie co sekundę idzie zrzut kontrolny.

### 2.4 Ticker przewijany przez sidecar – pomysł na później

Chromium rysuje raz cały pasek tekstu, a sidecar przesuwa go o piksele wyliczone z czasu klatki wyjścia. Płynność idealna przy minimalnym CPU. To jednak duża zmiana projektu: VizRS musiałby podawać przez protokół treść, tempo i stan tickera, a część grafiki przestałaby pochodzić z `/output`.

## 3. Etapy

| # | Etap | Stan |
|---|---|---|
| 1 | miara równych kroków (`bench/sidecar.mjs`, znacznik w `bench/scene.html`), CPU przeglądarki, `BENCH_CAPTURE`/`BENCH_MAX_FPS`/`BENCH_RENDER` | ✅ |
| 2 | `capture_mode: frames` (`src/capture.rs`, `src/config.rs`, `src/sys.rs` – zegar monotoniczny), powrót do screencastu bez begin frames, ostrzeżenie o nienarysowanych klatkach, `/health` `mode`, testy Rusta i kontraktu | ✅ |
| 3 | decyzja: domyślnie `screencast` (niżej); później zmieniona na `frames` (§5) | ✅ |
| 4 | DOKUMENTACJA §1 (Płynność animacji, Klatkaż kanału), §4 (A1), §5, §8 | ✅ |
| 5 | osobny proces na ticker | nie podjęty: brak pomiaru na A1, a `frames` usuwa główną przyczynę bez zmian w VizRS |

Opis etapów z planowania:

1. **Miara równych kroków w benchmarku.** `bench/sidecar.mjs` podaje dziś tylko `pictures_per_s`, więc dudnienia nie pokaże. Trzeba dodać odsetek klatek wyjścia, w których ticker przesunął się o jeden krok (wzorzec: stary `bench/bench.mjs`, commit 0c54143). Zmierzyć stan obecny: 25, 30, 50 i 60 kl./s, x86 i A1, 720p i 1080p. Przy okazji sprawdzić rzeczywistą częstotliwość Chromium (60 Hz, dryf).
2. **Przechwytywanie BeginFrame za opcją** (np. `capture_mode`; `src/config.rs`, `config.example.json`, `docker-compose.yml`, DOKUMENTACJA §5, test). Domyślnie wyłączone, dopóki pomiary z etapu 1 nie potwierdzą zysku na A1. Obsłużyć pułapki z 2.3, zrzut stanu końcowego, pauzę bez sesji i restart po awarii (zasada „grafika nigdy nie zatrzymuje emisji”).
3. **Decyzja o domyślnym trybie** na podstawie pomiarów (x86 i A1). Aktualizacja DOKUMENTACJA §1 (Płynność animacji) i §4 (A1).
4. **Opis klatkażu 30/60** w DOKUMENTACJA §1 (Klatkaż kanału): kiedy pomaga, a kiedy szkodzi (2.2).
5. **Osobny proces na ticker (2.1)** tylko wtedy, gdy po etapie 3 na A1 brakuje CPU i VizRS umie wystawić pojedynczą warstwę.

## 4. Wyniki (etapy 1 i 3)

Host: Intel Xeon 2,8 GHz, 4 vCPU (maszyna w chmurze, współdzielona – liczby niosą szum kilku punktów), Chromium 153 `chrome-headless-shell`, scena `animated`, rozgrzewka 5 s, pomiar 15 s. Kolumny: CPU sidecara / CPU przeglądarki (% rdzenia), nowe obrazy na sekundę, klatki wyjścia równo / stoi / przeskok.

| Kanał | Tryb | `screencast` | `frames` |
|---|---|---|---|
| 720p25 | etv | 19 / 150, 25,9/s, 60 / 16 / 24 | 21 / 97, 24,4/s, 95 / 3 / 3 |
| 720p25 | ffmpeg | 34 / 134, 20,3/s, 31 / 32 / 37 | 32 / 95, 24,7/s, 97 / 2 / 2 |
| 720p30 | etv | 17 / 136, 21,5/s, 34 / 32 / 34 | 22 / 109, 27,5/s, 83 / 9 / 9 |
| 720p30 | ffmpeg | 40 / 151, 25,9/s, 55 / 18 / 27 | 39 / 109, 28,0/s, 86 / 7 / 7 |
| 720p50 | ffmpeg | 59 / 178, 46,7/s, 48 / 24 / 28 | 60 / 115, 30,6/s, 27 / 39 / 34 |
| 720p60 | ffmpeg | 72 / 186, 50,1/s, 57 / 20 / 23 | 71 / 115, 30,1/s, 9 / 50 / 41 |
| 720p50, `max_fps` 25 | ffmpeg | 61 / 146, 26,1/s, 62 / 16 / 22 | 59 / 92, 24,8/s, 99 / 1 / 0 |
| 1080p25 | etv | 21 / 152, 14,3/s, 10 / 47 / 43 | 28 / 118, 17,5/s, 41 / 30 / 29 |
| 1080p25 | ffmpeg | 70 / 151, 13,7/s, 9 / 48 / 43 | 71 / 118, 16,5/s, 33 / 34 / 33 |
| 1080p30 | etv | 21 / 152, 13,7/s, 3 / 56 / 41 | 28 / 118, 17,7/s, 24 / 41 / 35 |
| 1080p30 | ffmpeg | 84 / 153, 13,5/s, 1 / 56 / 43 | 84 / 117, 16,6/s, 17 / 45 / 38 |
| 1080p50 | ffmpeg | 92 / 191, 26,5/s, 5 / 45 / 50 | 99 / 117, 16,7/s, 0 / 66 / 34 |
| 1080p60 | ffmpeg | 92 / 189, 25,7/s, 2 / 46 / 52 | 100 / 117, 16,1/s, 0 / 67 / 33 |
| 1080p50, `max_fps` 25 | ffmpeg | 95 / 148, 13,0/s, 26 / 49 / 25 | 99 / 116, 16,5/s, 45 / 33 / 22 |
| 1080p25, render 720p | etv | 66 / 145, 26,0/s, 64 / 14 / 22 | 71 / 97, 24,3/s, 93 / 3 / 3 |
| 1080p25, render 720p | ffmpeg | 98 / 131, 20,8/s, 30 / 32 / 38 | 99 / 95, 24,3/s, 87 / 4 / 9 |

Wnioski:

- Gdy host rysuje każdą klatkę w czasie klatki (720p przy 25–30 kl./s, 1080p renderowane w 720p), `frames` daje 83–99% równych kroków zamiast 30–64% i ok. jedną trzecią mniej CPU przeglądarki. To potwierdza wynik starego benchmarku (`chromium-frames`, 99% wobec 65% przy 720p).
- Gdy nie nadąża (tu 1080p w pełnym rozmiarze, 720p przy 50/60 kl./s), oba tryby są słabe, a `frames` bez `max_fps` przy 50/60 kl./s gorszy od screencastu: rysowanie jest szeregowe (`beginFrame` czeka na PNG), klatki wypadają nieregularnie. Lekarstwo: `max_fps` (720p50 z `max_fps` 25: 99%) albo `render_size`.
- Hipoteza 2.2 (30/60 kl./s wyrównują screencast) się nie potwierdziła (34–57% przy 720p).

Decyzja (etap 3, zastąpiona – patrz „Domyślnie `frames`” w §5): **domyślnie `screencast`**. Powody: `frames` wymaga `chromium-headless-shell` (z innym Chromium tylko powrót do screencastu po restarcie przeglądarki) i bez dobranego `max_fps`/`render_size` na zbyt wolnym hoście bywa gorszy; Ampere A1 nie został zmierzony. Zalecenie w DOKUMENTACJI §1 i §4: `frames` przy renderowaniu, które host rysuje w czasie klatki – sprawdzić benchmarkiem na maszynie docelowej. Na A1 nakładka `docker-compose.ampere.yml` ma już render 720p i `max_fps` 25, czyli warunki, w których `frames` wypadł najlepiej; włączenie po pomiarze na A1.

Możliwe dalsze kroki: kilka przeglądarek rysujących na przemian (stary `frames-worker.mjs` miał `pages`), żeby PNG kodowały się równolegle przy 50 kl./s; automatyczne obniżanie tempa (co druga klatka), gdy host nie nadąża.

## 5. Etap 6: obszar grafiki (`area`)

Cel: lżejsze wstrzykiwanie grafiki do ErsatzTV – nie cały ekran kanału, tylko część zajęta przez grafikę (np. pasek z tickerem `1920x140+0+930`).

Co powstało (`src/area.rs`, `src/capture.rs`, `src/yuva.rs`, `src/server.rs`, `src/overlay.rs`):

- `area` / `VIZRS_AREA` / `&area=` w adresie elementu, zapis `SZERxWYS+X+Y`, wyrównany do parzystych pikseli;
- Chromium rysuje tylko obszar (`Emulation.setDeviceMetricsOverride` z `viewport`; strona dalej układa się w pełnym rozmiarze), zrzut stanu końcowego z `clip` – screencast przy `viewport` nie wysyła klatki statycznej strony, więc zrzut idzie zawsze, także przy 50 kl./s;
- PNG, dekodowanie i `yuva420p` w rozmiarze obszaru; pakiety ETV we współrzędnych kanału;
- nakładka ffmpeg: `/ffmpeg?…&area=1` tylko wtedy, gdy umie ustawić grafikę (`x=`, `y=` w nakładaniu wejścia płótna; nie przy skalowaniu grafiki ani opcjach pozycyjnych), klatki obszaru i `overlay…=x=X:y=Y`; stara nakładka (bez `area=1`) dostaje pełne klatki;
- benchmark: `BENCH_AREA` (znacznik kroku w rogu obszaru).

Wynik (DOKUMENTACJA §1, §8): z `frames` 96–99,5% równych kroków na kanale 1080p przy 25 i 50 kl./s (cały kanał: 0–38%), CPU przeglądarki ok. 2,5× mniejsze, sidecar w trybie nakładki ok. 4× mniej CPU i 8× mniej danych. Koszt: grafika poza obszarem jest obcinana.

### Kilka obszarów na kanał (zrobione)

- `area=1920x140+0+930,160x160+1690+30`; nakładające się obszary łączone w prostokąt obejmujący.
- `frames`: w każdej klatce kolejne `setDeviceMetricsOverride` z `viewport` obszaru i `beginFrame` (czasy klatki o 0,004 ms od siebie); obraz = jeden PNG na obszar (`History::set_pngs`, dekoder z numerem PNG). Eksperyment: dwa obszary w jednej przeglądarce 25 kl./s przy 77% CPU, jeden prostokąt obejmujący 16 kl./s przy 115%.
- screencast: jeden `viewport` = prostokąt obejmujący, prostokąty przycinane do obszarów (`cut_to`).
- ffmpeg: obszary ułożone jeden pod drugim w jednej klatce (`Layout::atlas`); nakładka: `split=N`, `crop` każdego, łańcuch przygotowania ErsatzTV (`format`, `hwupload…`) powielony, `overlay…=x:y` po kolei. Sprawdzone prawdziwym ffmpeg (programowy `overlay`).
- `/ffmpeg?…&area=1` zwraca `areas` (x, y, w, h) oraz `x`, `y` pierwszego; nakładka bez `area=1` (np. z wydania 2.1.6) dostaje pełne klatki kanału.

Wynik (DOKUMENTACJA §8): pasek + logo przy 25 kl./s 98,9% równych kroków (jeden prostokąt obejmujący: 36%); przy 50 kl./s potrzebne `max_fps` 25 (100%).

Dalej: ciaśniejsze układanie obszarów w klatce ffmpeg (dziś wąskie logo pod szerokim paskiem marnuje ok. połowę klatki); obszar wykrywany automatycznie z widocznych prostokątów.

### Domyślnie `frames` (2026-10-25, decyzja autora)

`capture_mode` domyślnie `frames` – w kodzie, compose, nakładce A1 i benchmarku; screencast zostaje jawną opcją i automatycznym powrotem przeglądarki bez begin frames (pełny Chrome, CI). Zła wartość opcji = `frames`. Nagrania MP4 i klatki (25/30/50/60 kl./s): nowy wariant 99% (25), 91% (50, `max_fps` 25) równych kroków wobec 4–13% screencastu; przy 30/60 kl./s z dwoma obszarami 4 vCPU z równoległym x264 nie nadąża – liczba rysowań (obszary × tempo) ok. 50/s na takiej maszynie.
