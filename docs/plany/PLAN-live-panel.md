# PLAN: VizRS Live Panel – sterowanie transmisjami na żywo kanałów ErsatzTV

> Stan: **propozycja** (2026-11-05), gałąź `live-panel`. Etapy L0–L6 (§9). Planowanie zostaje w ErsatzTV, panel prowadzi do niego kreatorem (§6a).
> Powiązane: DOKUMENTACJA §1 „Live ingest” i „Audycje na żywo w ramówce”,
> VizRS CG System `docs/plany/PLAN-kanal.md` (rola emisji, MCR).

## 0. Streszczenie

Live ingest (2.8–2.9) działa, ale nie ma interfejsu: audycje na żywo definiuje
się w `.env` (`VIZRS_LIVE_SHOWS`), stan widać tylko w `curl` i logach, a
operator nie może zdjąć studia z anteny inaczej niż wyłączając OBS. ErsatzTV
nie pomaga: Remote Streams są w nim tylko listą do przeglądania (plik `.yml`
na dysku, bez edytora), a ramówką nie steruje się na żywo.

**VizRS Live Panel** to czwarta aplikacja pakietu: stanowisko operatora
emisji (MCR) dla sygnałów na żywo. Pokazuje, co przychodzi ze studia i co
idzie na antenę, pozwala wpuścić albo zdjąć studio, prowadzi listę audycji na
żywo i pokazuje najbliższe z nich w ramówce z odliczaniem.

## 1. Miejsce w pakiecie VizRS

| Aplikacja | Repozytorium | Rola | Wzór (Vizrt) |
|---|---|---|---|
| VizRS CG System (`vizrs`) | VizRS CG System | silnik grafiki: Artist (sceny), Trio (playout stron), `/output`, TCP 6100 | Viz Artist, Viz Trio, Viz Engine |
| VizRS CG Playout (`vizrs-cg`, `vizrs-ffmpeg`, `vizrs-etv-client`) | VizRS CG Playout | grafika VizRS na kanałach ErsatzTV, live ingest (odbiór, przejęcie kanału) | Viz Multichannel + integracja z automatyką |
| ErsatzTV | (zewnętrzne) | playout wideo, harmonogram, EPG | automatyka emisji |
| **VizRS Live Panel** (nowy) | VizRS CG Playout, gałąź `live-panel` | sterowanie sygnałami na żywo i audycjami na żywo kanałów | panel MCR / router sygnałów |

Podział odpowiedzialności zostaje: grafikę zmienia Trio, ramówkę ErsatzTV,
obraz kanału nakładka ffmpeg. Live Panel steruje tylko tym, **czy i kiedy
studio jest na antenie**, i definicjami audycji na żywo.

## 2. Gdzie działa – decyzja

| Wariant | Za | Przeciw |
|---|---|---|
| **A. Moduł sidecara `vizrs-cg`**: UI pod `/live`, API pod `/api/live/*`, w tej samej binarce (zalecany) | stan sygnałów, przejęcia i audycji już jest w sidecarze (`src/ingest.rs`, `src/live_shows.rs`); zero nowych usług w stosie, S1/S2 dostają panel z aktualizacją obrazu `cg`; tak jak Trio jest częścią binarki `vizrs` | panel restartuje się razem z sidecarem (restart i tak kończy przebieg live) |
| B. Osobna binarka `vizrs-live` (port 7710), rozmawia z sidecarem przez HTTP | niezależny cykl życia, własny obraz | dubluje stan albo wymaga nowego API sterowania w sidecarze i tak; nowa usługa w compose, Portainer, porty |
| C. Panel w VizRS Trio | operator ma wszystko w jednym oknie | Trio nie zna ErsatzTV ani ingest; zmiany w dwóch repo, zasada zera zależności rdzenia VizRS i sterowanie emisją w aplikacji grafiki |

**Propozycja: A.** Live Panel to aplikacja z własną nazwą i adresem
(`http://<serwer>:7700/live`), wbudowana w `vizrs-cg` jak Trio w `vizrs`.
Trio dostaje później tylko wskaźnik „studio na antenie” (L6) z tego samego API.
Gdyby panel urósł (wiele stosów, wielu operatorów), API z §5 pozwala wydzielić
go do wariantu B bez zmian w UI.

## 3. Architektura

```
 OBS / koder studia ── RTMP/SRT ──► ingest (vizrs-ffmpeg vizrs-ingest)
                                      │  MPEG-TS, POST /ingest            │ heartbeat + miniatura (L2, L4)
                                      ▼                                   ▼
┌──────────────────────────── vizrs-cg (sidecar, :7700) ─────────────────────────────┐
│  ingest.rs   źródła kanałów, bufor od klatki kluczowej, stan live                   │
│  control.rs  (nowy) decyzje operatora: wstrzymaj / wpuść / zdejmij, tryb anytime    │
│  live_shows.rs  audycje na żywo: shows.json → Remote Streams .yml                   │
│  panel.rs    (nowy) /live (UI), /api/live/* (REST), /api/live/events (SSE)          │
│  etv_api.rs  (nowy) ErsatzTV: skan biblioteki, kanały, EPG (xmltv), reset playoutu  │
└──────────────┬───────────────────────────────┬─────────────────────────────────────┘
               │ /ingest/state (live = źródło  │ /ingest?…&slot=1
               │ ∧ nie wstrzymane)             │
               ▼                               ▼
      vizrs-ffmpeg (nakładka w ErsatzTV) ── przebiegi kawałek / live / plansza ──► HLS kanału
               ▲
 ErsatzTV ─────┘ (harmonogram, EPG; audycje na żywo z biblioteki /config/vizrs-live)

 Przeglądarka operatora ──► /live?token=… (UI, SSE)      Trio ──► /api/live/state (L6)
```

Najważniejsza własność: **nakładka nie zmienia się**. Już teraz pyta co
250 ms o `/ingest/state` i przechodzi na live albo z powrotem według pola
`live`, a poza audycjami na żywo także `anytime`. Decyzje operatora wchodzą
w te dwa pola: `live = sygnał obecny ∧ kanał nie wstrzymany`, `anytime` =
ustawienie kanału z panelu (zamiast jednego `VIZRS_INGEST_ANYTIME`). Zdjęcie
studia w panelu działa więc w ok. 0,3 s plus opóźnienie bufora odtwarzacza.

## 4. Dane i stan

| Co | Gdzie | Trwałość |
|---|---|---|
| Audycje na żywo (kanał, tytuł, opis, długość, plik) | `<etv_config>/vizrs-live/shows.json`, źródło prawdy; `.yml` generowane z niego | dysk (wolumen konfiguracji ErsatzTV, już zapisywalny dla 10001 od 2.9.1) |
| `VIZRS_LIVE_SHOWS` | tylko pierwsze wypełnienie `shows.json`, gdy go nie ma; potem w logu informacja, że env jest pomijany | – |
| Tryb kanału: `anytime`, wstrzymanie (`hold`) | `<etv_config>/vizrs-live/control.json` | dysk: wstrzymanie musi przeżyć restart `cg`, inaczej restart wpuściłby studio |
| Źródła, stan live, czas trwania, kodeki | pamięć `ingest.rs` | ulotne |
| Odbiorniki (adresy, ostatni nadawca, błędy) | heartbeat odbiorników → pamięć | ulotne, odnawiane co 5 s |
| Miniatura sygnału | ostatni JPEG z odbiornika → pamięć | ulotne |
| ID biblioteki „Na żywo” w ErsatzTV | `control.json` (ustawienie panelu) | dysk |
| Log operacji (kto, co, kiedy) | `<etv_config>/vizrs-live/log.jsonl`, rotacja | dysk |

## 5. API (`/api/live/*`, token panelu)

| Metoda | Ścieżka | Działanie |
|---|---|---|
| GET | `/api/live/state` | kanały: źródło (jest, od kiedy, kodek, dźwięk, nadawca), na antenie, wstrzymanie, anytime, bieżąca i następna audycja na żywo |
| GET | `/api/live/events` | SSE: zmiany stanu (jak `/events` w VizRS), zamiast odpytywania |
| POST | `/api/live/channels/<n>/hold` · `/release` | wstrzymaj studio (zdejmij / nie wpuszczaj) · wpuść |
| PUT | `/api/live/channels/<n>` | `{"anytime": bool}` |
| GET/POST | `/api/live/shows` | lista (ze stanem w ErsatzTV i najbliższą emisją) · dodaj audycję i utwórz ją w ErsatzTV |
| PUT/DELETE | `/api/live/shows/<id>` | zmień · usuń (z potwierdzeniem w UI: ErsatzTV zdejmie ją z harmonogramów) |
| POST | `/api/live/shows/scan` | skan biblioteki „Na żywo” w ErsatzTV (`POST /api/libraries/<id>/scan`) |
| POST | `/api/live/channels/<n>/rebuild` | przebudowa playoutu kanału (`POST /api/channels/<n>/playout/reset`) |
| GET | `/api/live/guide?channel=<n>` | najbliższe pozycje z EPG ErsatzTV (`/iptv/xmltv.xml`), oznaczone, które są audycjami na żywo |
| GET | `/api/live/preview/<n>.jpg` | miniatura sygnału studia |

Wewnętrzne, dla odbiorników (token ingest): `POST /ingest/heartbeat`,
`POST /ingest/thumb?channel=<n>`. Istniejące `/ingest*` i `/health` bez zmian.

## 6. Interfejs

Makieta: [live-panel-makieta.html](live-panel-makieta.html) (statyczna; `#edycja` w adresie pokazuje okienko audycji).

Czysty JS (moduły ES), bez bibliotek i CDN, działa offline; wygląd z
`ui.css` VizRS (te same kolory, przyciski, panele), polskie etykiety.

```
┌ VizRS Live Panel ────────────────────────────── 19:58:12 ● połączono ─┐
│ KANAŁY                                                                │
│ ┌ 1  BTB 24 ──────────────────┐  ┌ 4  BTB Info ────────────────────┐  │
│ │ [miniatura studia]           │  │ [brak sygnału]                  │  │
│ │ ● SYGNAŁ 00:12:41 h264 ♪     │  │ ○ odbiornik rtmp …/live/…       │  │
│ │ ■ NA ANTENIE                 │  │ □ ramówka                       │  │
│ │ [ZDEJMIJ]  anytime ☐         │  │ [WPUŚĆ]   anytime ☑             │  │
│ │ Teraz: Wiadomości (do 20:30) │  │ Następna live: 21:00 Rozmowa…   │  │
│ └──────────────────────────────┘  └─────────────────────────────────┘  │
│ AUDYCJE NA ŻYWO                                         [+ Dodaj]     │
│  kanał  tytuł              długość  opis                   ✎  🗑       │
│  1      Wiadomości          30:00   Serwis ze studia                   │
│ RAMÓWKA (kanał 1)  20:00 Wiadomości ● LIVE · 20:30 Film …             │
└───────────────────────────────────────────────────────────────────────┘
```

- Duże, jednoznaczne przyciski (ZDEJMIJ czerwony, WPUŚĆ zielony),
  potwierdzenie przytrzymaniem albo drugim kliknięciem, jak TAKE w Trio.
- Ostrzeżenie „audycja za 2 min, brak sygnału ze studia” (kanał na żółto).
- Edycja audycji w okienku: kanał (lista z ErsatzTV), tytuł, opis, długość
  `HH:MM:SS`; po zapisie skan biblioteki i informacja „dodaj ją do
  harmonogramu w ErsatzTV” z odnośnikiem.
- Później: wersja na telefon (jak `/mobile` w VizRS) i okno `vizrs-shell`.

## 6a. Audycje LIVE w ramówce ErsatzTV – kreator

Ramówkę układa się dalej w ErsatzTV; panel ma tylko maksymalnie skrócić
drogę od pomysłu na audycję do pozycji w harmonogramie. Co da się zrobić z
zewnątrz (sprawdzone w kodzie ErsatzTV):

| Krok | API ErsatzTV | Kto robi |
|---|---|---|
| definicja audycji (`.yml` Remote Stream) | – (plik na dysku) | panel |
| skan biblioteki „Na żywo” | `POST /api/libraries/<id>/scan` | panel |
| kolekcja z audycją | `POST /api/collections/smart/new` (Smart Collection, zapytanie po bibliotece i dokładnym tytule) | panel, nazwa `LIVE · <tytuł>` |
| **pozycja w harmonogramie** (godzina, One) | brak API dla Classic/Block; Scripted/ExternalJson/YAML przejmują cały kanał | operator w ErsatzTV |
| przebudowa playoutu | `POST /api/channels/<n>/playout/reset` | panel (przycisk) |

Zapis do bazy ErsatzTV (`ersatztv.sqlite3`) odrzucony: zapis pod działającą
aplikacją, schemat zmienny między wersjami, ryzyko uszkodzenia ramówki S1/S2.

Przebieg w panelu (zakładka **Audycje LIVE**):

1. **＋ Nowa audycja LIVE**: kanał, tytuł (EPG, `{title}`), długość, opis,
   plansza bez studia → **Utwórz w ErsatzTV**.
2. Panel: zapis `.yml`, skan, czekanie na pozycję w bibliotece, Smart
   Collection `LIVE · <tytuł>`; postęp krok po kroku.
3. Okienko „ostatni krok” z gotowymi wartościami do wybrania w ErsatzTV
   (Fixed, Strict, Smart Collection, One, opcjonalnie element VizRS) i
   odnośnikiem do harmonogramu kanału (`/schedules`).
4. **Przebuduj ramówkę** (reset playoutu kanału).
5. Tabela audycji pokazuje stan w ErsatzTV (plik, biblioteka, kolekcja) i
   najbliższą emisję z EPG (`/iptv/xmltv.xml`); audycja bez emisji w oknie EPG
   = „nie ma w ramówce” z instrukcją.

Zmiana tytułu przemianowuje kolekcję (`/api/collections/smart/update`) i
wymaga skanu; usunięcie usuwa plik i kolekcję po potwierdzeniu (ErsatzTV
zdejmie pozycje z harmonogramów, które jej używają).

Makieta: zakładka **Audycje LIVE** (`#audycje`), okienka `#nowa`, `#utworzona`, `#edycja`.

## 7. Bezpieczeństwo

- Własny token panelu `VIZRS_PANEL_TOKEN` (`panel_token`); puste = panel
  wyłączony (404). Token ingest zostaje dla maszyn (odbiorniki).
- Model z VizRS `guard.rs`: token w nagłówku albo `?token=` → ciasteczko,
  ekran logowania bez tokenu, odrzucanie żądań z obcych witryn
  (`Sec-Fetch-Site`, `Origin`), porównanie w stałym czasie, spowalnianie
  złych prób. Kopia zasad, nie kodu (VizRS nie ma zależności między repo).
- Port 7700 nadal nie powinien być publiczny; panel do sieci operatorów albo
  za reverse proxy z HTTPS.
- Każda zmiana (wstrzymanie, audycje) do logu operacji z nazwą operatora.

## 8. Integracje

- **ErsatzTV API** (klucz z `api-secrets.json`, już używany przez
  synchronizację): `GET /api/channels` (nazwy kanałów), `POST
  /api/libraries/<id>/scan` (po zmianie audycji), `GET /iptv/xmltv.xml`
  (ramówka, rozpoznanie audycji na żywo po tytule i kanale). ErsatzTV nie ma
  API listy bibliotek: ID biblioteki „Na żywo” operator podaje raz w
  ustawieniach panelu (widać je w adresie strony biblioteki).
- **Odbiorniki** (`src/listener.rs`): heartbeat co 5 s (adresy, nadawca,
  ostatni błąd, np. zły klucz) i miniatura z drugiego wyjścia ffmpeg
  (`fps=1/2`, 320 px, MJPEG) – bez dekodowania w sidecarze, który nie ma
  ffmpeg.
- **VizRS**: bez zmian w L0–L5. L6: w Trio wskaźnik „● STUDIO NA ANTENIE”
  kanału z `/api/live/state` (Trio zna już kanały playoutu, `⏱ kanał`).

## 9. Etapy

| Etap | Zakres | Gotowe, gdy |
|---|---|---|
| L0 | szkielet: `panel.rs`, `/live` (UI wbudowane `include_str!`), token panelu, logowanie, `/api/live/state` z obecnego stanu ingest | panel pokazuje kanały i sygnał, test kontraktu |
| L1 | audycje LIVE (§6a): `shows.json` (migracja z env), CRUD, `.yml`, skan biblioteki, Smart Collection `LIVE · …`, okienko „ostatni krok”, reset playoutu | nowa audycja z panelu = gotowa kolekcja w ErsatzTV, operator dodaje tylko pozycję w harmonogramie |
| L2 | sterowanie: `control.rs`, wstrzymaj / wpuść, `anytime` na kanał, trwałe w `control.json`, log operacji; SSE | ZDEJMIJ zdejmuje studio w < 1 s na stosie testowym |
| L3 | ramówka: EPG z ErsatzTV, najbliższa emisja audycji, „nie ma w ramówce”, odliczanie i ostrzeżenie o braku sygnału na karcie kanału | operator widzi „20:00 Wiadomości za 3 min, studio gotowe” |
| L4 | odbiorniki: heartbeat, miniatura, błędy nadawcy w panelu | widać obraz studia i zły klucz bez logów |
| L5 | dokumentacja (DOKUMENTACJA §1 „Live Panel”, §4 compose), compose (`VIZRS_PANEL_TOKEN`), wydanie | stos S2 z panelem |
| L6 | Trio: wskaźnik studia (VizRS CG System); telefon / `vizrs-shell` | operator Trio widzi stan studia |

## 10. Testy

- Rust: jednostkowe dla `control.rs` (decyzja `live`), `shows.json`
  (migracja, walidacja jak `validate.rs` w VizRS: limity długości tekstu,
  liczba audycji, czas 1 s–24 h), EPG (parser XMLTV).
- Kontrakt (`tests/contract/live-panel.test.mjs`, Node bez bibliotek): API,
  token, CSRF, CRUD, wstrzymanie zmienia `/ingest/state`, udawany ErsatzTV
  (skan, xmltv).
- UI: zrzut z Playwright (`PLAYWRIGHT_DIR` jak w VizRS), poza bramką CI.
- Na stosie lokalnym: OBS → panel → ZDEJMIJ / WPUŚĆ, audycja z panelu w ramówce.

## 11. Do decyzji

1. Wariant A (moduł sidecara) czy B (osobna binarka) – §2.
2. Osobny `VIZRS_PANEL_TOKEN` czy wspólny z tokenem operatora VizRS.
3. Czy ZDEJMIJ ma wstrzymywać do ręcznego WPUŚĆ, czy do końca bieżącej
   audycji / rozłączenia nadawcy (propozycja: do WPUŚĆ, z widocznym stanem).
4. Czy panel ma kiedyś układać ramówkę (dodawać pozycje w harmonogramie
   ErsatzTV przez API skryptowe), czy zostawiamy to ErsatzTV (propozycja:
   zostawiamy).
