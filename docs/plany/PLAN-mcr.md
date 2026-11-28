# Plan rozwoju VizRS Live (PLAN-mcr) – braki wobec legacy, wzorzec Viz Channel Branding, legacy czy Next

> **Stan:** gałąź `next-v`, wydanie testowe 1.3.0-test.1 (2026-11-10): K1–K11, K18 zrobione; K12–K14, K17 – panel gotowy, czeka na VizRS CG Playout; K15–K16 – kontrakt; Q1 ręcznie; decyzja legacy/Next: **A – legacy** (§6). Od 2026-11-28 do zrobienia K22–K23 (nagła emisja live, §5); K24 – po K23, droga W1 (decyzja 2026-11-28). Stan prac: [../STATUS.md](../STATUS.md), znane braki: [../LUKI.md](../LUKI.md), opis działania: [../DOKUMENTACJA.md](../DOKUMENTACJA.md).
>
> **Jedyny dokument planów rozwoju VizRS Live** (decyzja użytkownika 2026-11-09): nowe plany i punkty panelu dopisuj tutaj – nowy punkt w tabeli §4 (w miejscu kolejki, które mu odpowiada) i opis w §5, nowe analizy jako kolejne sekcje – zamiast zakładać nowe pliki `PLAN-*.md`. Zakończone plany [PLAN-rozbudowy.md](archiwum/PLAN-rozbudowy.md) i [PLAN-makieta.md](archiwum/PLAN-makieta.md) zostają jako archiwum.
>
> Treść: §1 skąd wiadomo, §2 czego jeszcze brakuje wobec web GUI legacy, §3 czego uczy Vizrt (tabela A–J), §4 jeden przebieg (luki + plan), §5 punkty, §6 legacy czy Next – decyzja i analiza, §7 harmonogram w stylu ffplayout – analiza (propozycja K19–K20, do decyzji), §8 przepisanie legacy do Rusta – VizRS Live jako GUI i planista dla Next (analiza, propozycja N0–N7, do decyzji).

## 1. Źródła i ograniczenia rozpoznania

- **Web GUI legacy (port 8409).** Rozpoznanie z kodu, bez działającego GUI: GUI jest zamknięte za proxy (DOKUMENTACJA §8), a sesja, w której powstał plan, nie miała dostępu do lokalnego stosu. Źródło: `ErsatzTV/legacy` na `main`, commit `e51ac60` z 2026-11-09 (po v26.12.0), strony `ErsatzTV/Pages/**/*.razor`, menu `Shared/MainLayout.razor`. Porównanie: 259 rekordów komend i zapytań w `ErsatzTV.Application` z białą listą `bridge/commands.txt` (241 wierszy) i listą pokrycia (DOKUMENTACJA §8).
- **ErsatzTV Next.** `ErsatzTV/next` commit `c5cb065` (v0.2.5, 2026-11-06): README, `CHANGELOG.md`, `schema/*.json`, trasy `crates/ersatztv`. Wcześniejsze rozpoznanie (2026-11-06): `../../../VizRS CG System/docs/plany/PLAN-vizrs-live.md` §2.
- **Vizrt.** Kanałową grafikę i playout Vizrt robi dziś **Viz Channel Branding** (5.x, następca **Viz Multichannel** 4.x); studio – Viz Mosart (poza zakresem). Opis pochodzi z wyników wyszukiwarki i stron produktu, **nie z pełnej lektury przewodników**. Prośba o dodanie `docs.vizrt.com` i `documentation.vizrt.com` do dozwolonych hostów sesji została odrzucona (2026-11-09). Do uzupełnienia, gdy hosty będą dozwolone: przewodnik VCB 5.5/5.6 (rozdziały User Interface, Playlists, Channel Deck, Integrations Hub REST v2), Multichannel 4.2 (Master Control Workflow, Import/Export Settings).
  Strony: [VCB – przegląd](https://www.vizrt.com/product-updates/viz-channel-branding-overview/), [VCB 5.4](https://www.vizrt.com/product-updates/viz-channel-branding/viz-channel-branding-5-4/), [release notes 5.4](https://docs.vizrt.com/viz-channelbranding-rn/5.4/Viz_Channel_Branding_Release_Notes.html), [5.5 (PDF)](https://docs.vizrt.com/viz-channelbranding-rn-5.5.pdf), [User Interface 5.5](https://docs.vizrt.com/viz-channelbranding-guide/5.5/User_Interface.html), [Basic Settings 5.0](https://documentation.vizrt.com/viz-channelbranding-guide/5.0/Basic_Settings.html), [Playlists 5.3](https://docs.vizrt.com/viz-channelbranding-guide/5.3/Playlists.html), [Clip Bank 5.2](https://docs.vizrt.com/viz-channelbranding-guide/5.2/Clip_Bank.html), [Multichannel 4.2 – Working with Playlists](https://docs.vizrt.com/viz-multichannel-guide/4.2/Working_with_Playlists.html), [Import/Export Settings](https://docs.vizrt.com/viz-multichannel-guide/4.2/Import___Export_Settings.html), [Master Control Workflow](https://docs.vizrt.com/viz-multichannel-guide/4.2/Master_Control_Workflow.html).

## 2. Web GUI legacy – co jeszcze nie ma odpowiednika

Pokrycie jest prawie pełne (DOKUMENTACJA §8). Nowe braki, których nie było w LUKI (dopisane jako #23–#27):

| Element legacy | Komendy | Uwagi |
|---|---|---|
| **Edit Channel Numbers** (`/channels/numbers`, przycisk na liście kanałów) – zmiana numeracji wszystkich kanałów naraz | `GetAllChannelsForSort`, `UpdateChannelNumbers` | wiersze w `commands.txt` + UI w `views/kanaly.js` → **K3** |
| **Własne rozdzielczości** (Settings → FFmpeg) | `CreateCustomResolution`, `DeleteCustomResolution` | → **K4** |
| **Walidator YAML harmonogramu sekwencyjnego** (`/system/troubleshooting/sequential-schedule`) | `ISequentialScheduleValidator` – poza MediatR | dyspozytor go nie wywoła; razem z LUKI #14 (pliki YAML) |
| **Diagnostyka playoutu blokowego** (`/system/troubleshooting/block-playout`) | `GetAllBlockPlayouts`, `DecodePlayoutHistory` | obok LUKI #13 → **K7** |
| **Wyodrębnianie napisów osadzonych** | `ExtractEmbeddedSubtitles`, `ExtractEmbeddedShowSubtitles` | zadanie w tle → **K8** |
| **Paczka diagnostyczna** (Troubleshooting → Upload Results) | `ArchiveTroubleshootingResults`, `ArchiveMediaSample` | przydatna przy zgłoszeniu awarii (RUNBOOK) → **K5** |
| **Stan silnika Next** | `GetNextProbe`, `RefreshNextProbe`, `GetNextDescribe`, `GetNextVersion` | już LUKI #11 → **K5** |

Świadomie pominięte (bez zmian): Trakt (LUKI #16), Docs/GitHub, UI settings, `RebuildSearchIndex`, `TryCompletePlexPinFlow`, `Get{Jellyfin,Emby}ConnectionParameters`.

## 3. Wzorzec Vizrt – co z niego brać

Model Viz Channel Branding (VCB) i Multichannel, w skrócie:

- **Playlista kanału** = **zdarzenia pierwotne** (programy z ramówki) i przypięte do nich **zdarzenia wtórne** (strony grafiki i akcje) z **przesunięciem** (offset) względem programu i czasem trwania. Playlista dobowa albo ciągła.
- **Import ramówki z systemu traffic**: CSV/TXT, XML, Excel, Harris ADC‑100, Omnibus; folder obserwowany, pliki oznaczane `.done`/`.fail`, okno importu, „pełna aktualizacja” (scalenie bez kasowania) albo „tylko czasy”; od 5.x także REST (Integrations Hub v2).
- **Automatyczna kontrola playlisty**: nakładające się animacje, niezgodności z ramówką, inne odstępstwa; zdarzenie z błędem na czerwono z opisem w podpowiedzi.
- **Run Status**: stan wykonania każdego zdarzenia, opisany przez Media Sequencer.
- **Channel Deck**: jeden widok wszystkich kanałów – obraz (NDI), co gra teraz, co zaplanowano, alarmy (brak reguły, brak metadanych do szablonu).
- **Page pool i Quick Take**: strony gotowe do ręcznego wprowadzenia; **Take / Continue / Take Out** przy automatyce.
- **Clip Bank**: wcześniej przygotowane zdarzenia LIVE do szybkiego wstawienia w bieżącą playlistę.
- **Dane w stronach**: pola z bazy, pliku, playlisty; reguły aktywacji (pozycja aktywna tylko, gdy dane są).
- **Kampanie** (5.4): zatwierdzanie stron (Planning → MCR), czas życia (TTL) stron kampanii.
- **Wyzwalanie**: czasowe przez Media Sequencer albo z automatyki (TCP/IP, Intelligent Interface, VDCP, GPI, REST).

Odpowiedniki w rodzinie VizRS: playlista = ramówka legacy (playout), zdarzenia wtórne = reguły grafiki CG Playout (`/api/live/graphics`: start pozycji ± s, przed końcem, przerwa reklamowa, slot na żywo), Media Sequencer = CG Playout + VizRS (TCP 6100), Clip Bank ≈ Audycje LIVE, Page pool ≈ strony VizRS (`/etv/pages`), Channel Deck ≈ Na antenie.

### Funkcje Vizrt, których brakuje w panelu (A–J)

Sprawdzone w `server/web` (2026-11-09). Kolumna „Punkt” wskazuje punkt tabeli w §4.

| # | Funkcja (Vizrt) | Co by to było w VizRS | Gdzie zmiana | Punkt |
|---|---|---|---|---|
| A | **Zdarzenia wtórne** przy pozycji ramówki – strony i akcje z przesunięciem (offset) względem programu | reguła grafiki przypięta do konkretnej audycji/kolekcji/pozycji, z offsetem od startu albo końca i czasem trwania; kilka zdarzeń na pozycję (np. „Za chwilę” 30 s przed końcem, belka tytułowa po 10 s) | CG Playout (model reguł) + panel; legacy bez zmian | K15 |
| B | **Kontrola playlisty** – nakładające się animacje, niezgodności z ramówką, błąd na czerwono z opisem | „Sprawdź ramówkę”: dziury, playout za krótki, pliki w koszu, audycja LIVE bez wejścia studia, zdjęta grafika – czerwone znaczniki w EPG, Playoutach i na karcie kanału | panel (dane z legacy i CG) | K9 |
| C | **Run Status / as-run** – stan wykonania każdego zdarzenia | dziennik emisji: co faktycznie poszło na antenę (pozycja, start, koniec, strony grafiki, przejęcia live), eksport CSV | CG Playout (zapis, `GET /api/live/asrun`) + panel (ekran, CSV) | K14 |
| D | **Channel Deck** – jeden widok „czy branding gra poprawnie” na wszystkich kanałach | „Na antenie” z warstwami grafiki VizRS (pigułki „W10 · belka 101”) i lampką zgodności grafiki z regułą | najpierw API CG Playout (`/api/live/state`), potem panel | K12 (LUKI #22) |
| E | **Import ramówki z systemu traffic** (CSV, XML, Excel, Harris, Omnibus; folder obserwowany, `.done`/`.fail`; pełna aktualizacja albo tylko czasy) | import CSV/XML do playlisty legacy z podglądem dopasowania do mediów; dokładne godziny – Scripted Schedule (§6, C2, po zmianie decyzji R2) | panel (`CreatePlaylist`, `ReplacePlaylistItems` już na białej liście) | K11 |
| F | **Dane w stronach** z bazy, RSS albo pliku; reguły aktywacji (pozycja aktywna tylko, gdy dane są) | pola stron VizRS z EPG legacy (CG Playout ma już `{title}`, `{next_*}`); źródło RSS/JSON dla tickera; reguła „pomiń stronę, gdy pole puste” | VizRS CG System i CG Playout | poza planem panelu (do planów CG) |
| G | **Kampanie** (VCB 5.4): zatwierdzanie stron (Planning → MCR) i czas życia (TTL) stron kampanii | strona promo z datami „od–do” i znacznikiem „zatwierdzona”; reguła gaśnie po terminie | CG Playout (reguły) + panel | K16 |
| H | **Take / Continue / Take Out** i **Quick Take** z page pool przy automatyce | na karcie kanału: wprowadź stronę, dalej, zdejmij jedną warstwę (dziś jest tylko „Zdejmij całą grafikę”) | CG Playout (TCP VizRS: `TAKE`/`CONTINUE`/`OUT`) + panel | K13 |
| I | **Wyzwalanie z automatyki** (REST, TCP/IP, GPI, VDCP) | REST już jest (`/api/live/*`); dodać webhook i alarm przy utracie sygnału, pustej ramówce, zatrzymanej emisji, długo zdjętej grafice | panel (banner, dźwięk) + CG Playout (webhook) | K17 |
| J | **Monitorowanie wszystkich kanałów** z jednego klienta (obraz NDI w Channel Deck) | mozaika podglądów (multiview) w „Na antenie”; uwaga: każdy podgląd to sesja Next, która transkoduje – limit albo miniatury | panel (`player.js`, `mux.js`) | K10 |

## 4. Jeden przebieg: luki + plan

**Decyzja użytkownika (2026-11-09): wariant A – zostajemy przy ErsatzTV legacy** (§6). Realizacja na gałęzi **`next-v`**.

Przebieg łączy [LUKI.md](../LUKI.md) i punkty z §2–§3 w jedną kolejkę: najpierw G1 (bezpieczeństwo), potem szybkie G2 wobec legacy (tylko panel i biała lista), potem funkcje MCR z danych, które panel już ma, na końcu punkty wymagające zmian w API VizRS CG Playout i G3. Luki spoza przebiegu – na końcu tej sekcji, z powodem.

**Jak pracować.** Jak w [PLAN-rozbudowy.md](archiwum/PLAN-rozbudowy.md) („Jak pracować”): jeden punkt = jedna sesja, pierwszy „do zrobienia” ze spełnionym „Wymaga”, warunki „gotowe”, commit „Kn: …”. Testy wg [PLAN-makieta.md](archiwum/PLAN-makieta.md): `DEPLOY_SHOTS=<katalog poza repo> tests/deploy.sh` zielony, zrzut obejrzany, nowe przypadki w `tests/e2e.cjs` (sprawdzi je CI). Zamknięta luka: wiersz usunięty z LUKI i wpis w `CHANGELOG.md` („Niewydane”). Punkty **CG** zmieniają API VizRS CG Playout – zasada „pakiet zawsze zgodny”: najpierw CG Playout (kod, testy kontraktu, jego DOKUMENTACJA §1 „Live API”), potem panel, w tym samym cyklu; starszy CG Playout nie może psuć panelu (brak pola = brak elementu UI). Punkt **ręcznie** robi się z użytkownikiem – automat go pomija.

| Punkt | Zakres | Wymaga | Stan |
|---|---|---|---|
| K1 | Sesje panelu: losowy identyfikator sesji zamiast tokenu w ciasteczku, wylogowanie, wygasanie (LUKI #2, G1) | – | zrobione (2026-11-09) |
| K2 | CSP zawężone do znanych źródeł obrazów i ramek zamiast `http:`/`https:` (LUKI #6, G1) | – | zrobione (2026-11-09) |
| K3 | Numeracja kanałów naraz (LUKI #23) | – | zrobione (2026-11-09) |
| K4 | Własne rozdzielczości w Profilach (LUKI #24) | – | zrobione (2026-11-09) |
| K5 | Silnik Next w Profilach i Diagnostyce, paczka diagnostyczna (LUKI #11, #27) | – | zrobione (2026-11-09; #27 zostaje – DOKUMENTACJA §4) |
| K6 | Wyniki zadań w tle (nakładanie pozycji, konserwacja) w panelu, nie tylko w Logach (LUKI #10) | – | zrobione (2026-11-09) |
| K7 | Diagnostyka i historia playoutu blokowego (LUKI #13, część #25) | – | zrobione (2026-11-09) |
| K8 | Biblioteki: czas folderów obrazów, skan jednego serialu, wyodrębnianie napisów (LUKI #18, #26) | – | zrobione (2026-11-09) |
| K9 | Kontrola ramówki: błędy na czerwono w Na antenie, EPG i Playoutach (Vizrt B) | K6 | zrobione (2026-11-09) |
| K10 | Mozaika kanałów (multiview) w Na antenie (Vizrt J) | K9 | zrobione (2026-11-09) |
| K11 | Import ramówki z pliku CSV/XML do playlisty legacy (Vizrt E) | – | zrobione (2026-11-09) |
| Q1 | Sloty na żywo z prawdziwym sygnałem OBS → CG Playout, Plex z prawdziwym kontem (LUKI #3, G1) | – | ręcznie (z użytkownikiem) |
| K12 | Warstwy i strony VizRS na antenie w karcie kanału (LUKI #22, Vizrt D; CG) | – | zrobione (panel + CG Playout 26.11.x) |
| K13 | Ręczne TAKE / CONTINUE / OUT pojedynczej strony (Vizrt H; CG) | K12 | zrobione (panel + CG Playout 26.11.x) |
| K14 | Dziennik emisji (as-run) z eksportem CSV (Vizrt C; CG) | K12 | zrobione (panel + CG Playout 26.11.x) |
| K15 | Zdarzenia wtórne przypięte do audycji/kolekcji, oś pozycji z grafiką (Vizrt A; CG) | K12 | zrobione (2026-11-24, wydane w 26.11.29; cel tylko po tytule – niżej) |
| K16 | Ważność (od–do) i zatwierdzanie stron promo (Vizrt G; CG) | K15 | zrobione (2026-11-24, wydane w 26.11.29) |
| K17 | Alarmy: banner, dźwięk, webhook (Vizrt I; webhook w CG) | K9, K14 | zrobione (panel + CG Playout 26.11.x) |
| K18 | Podgląd kanału: HEVC w `mux.js`, dźwięk inny niż AAC – sprawdzić wykonalność (LUKI #21, G3) | – | zrobione (2026-11-09) |
| K19 | Konta operatorów: login i hasło zamiast tokenu, konfiguracja z panelu (pierwsze konto kodem z logu, Użytkownicy, Moje konto, role admin/operator, sesje na wolumenie) | – | zrobione (2026-11-18, gałąź `live-ingest-new`) |
| K20 | Okno AppImage logowane kontem (pośrednik z sesją konta zamiast `Bearer`) | K19 | zrobione (2026-11-24, wydane w 26.11.29) |
| K21 | Jedno konto w pakiecie (opcja): konta panelu logują do VizRS Trio i Artist (przełącznik w Użytkownicy, `/api/sso`, CG System `--panel-login`) | K19 | zrobione (2026-11-18, `live-ingest-new` + CG System `konta-panelu`) |
| K22 | Przebudowa ramówki bez kasowania postępu serii: Refresh zamiast Reset po zapisie emisji audycji i na karcie kanału | – | do zrobienia |
| K23 | Nagła emisja na żywo z datą: zaplanowane okno przejęcia kanału w CG Playout (wariant B, decyzja użytkownika 2026-11-28), wybór losu pozycji pod spodem przy dodawaniu (CG) | K22 | do zrobienia |
| K24 | Zachowanie odcinka pozycji zastąpionej przez nagłą emisję (wybór „zachowaj odcinek” z K23): rozpoznanie w legacy, potem wykonanie | K23 | do zrobienia po K23 – droga W1 (decyzja użytkownika 2026-11-28, §5) |

**Poza przebiegiem** (wiersze zostają w LUKI):

| LUKI | Dlaczego nie w przebiegu |
|---|---|
| #1 TLS | zamierzone: serwer bez zależności, TLS w zewnętrznym proxy (RUNBOOK) |
| #5 blokada biblioteki, #8 test odtwarzania, #9 postęp skanu, #12 stan Plex | poza MediatR – wymagałyby nowego kodu C# w moście; korzyść mała |
| #14 pliki YAML/skrypty, #20 elementy graficzne, walidator YAML z #25 | decyzja R2 (panel nie zapisuje na `/config`); zmiana tylko decyzją użytkownika – wtedy także Scripted Schedule (§6, C2) |
| #15, #17 | zamierzone albo ograniczenie legacy |
| #16 Trakt, #19 głęboki skan | pominięte świadomie (klucz API Trakt; rzadko potrzebny) |
| Vizrt F (dane w stronach) | praca w VizRS CG System i CG Playout, nie w panelu |

## 5. Punkty

### K1 – sesje panelu (LUKI #2)
Dziś ciasteczko `vizrs_live` niesie sam token panelu (`server/src/main.rs`, `session_cookie`), więc wymiana tokenu unieważnia wszystkie sesje, a wyciek ciasteczka = wyciek tokenu. Zrobić: po udanym logowaniu losowy identyfikator sesji (32 bajty z `/dev/urandom` – bez crate'ów; obraz `scratch` ma `/dev` od środowiska kontenera), tablica sesji w pamięci (`Mutex<HashMap>`, limit liczby, wygasanie po 30 dniach bezczynności), „Wyloguj” usuwa sesję, restart panelu = ponowne logowanie (opisać). Token panelu dalej działa w nagłówku dla skryptów, jeśli dziś działa (sprawdzić). Testy jednostkowe (wygasanie, limit, wylogowanie), e2e logowania. DOKUMENTACJA §3 i §9, RUNBOOK §6. Zamyka LUKI #2.

### K2 – CSP zawężone (LUKI #6)
`server/src/http.rs`: `CSP` jest stałą z `img-src … http: https:` i `frame-src http: https:`. Zbudować CSP przy starcie z originów, które panel zna: `--stream-url` (obrazy legacy `/artwork`) i adres VizRS (`/output` w ramce) – `img-src 'self' data: <stream-origin>`, `frame-src <vizrs-origin>`. Brak opcji = brak źródła. Test jednostkowy nagłówka; przejście po ekranach bez błędów CSP w konsoli (`prod-smoke.cjs` zbiera błędy konsoli – sprawdzić). Zamyka LUKI #6.

### K3 – numeracja kanałów naraz (LUKI #23)
Legacy: `ChannelNumbers.razor` – lista kanałów (`GetAllChannelsForSort`), zapis jednym `UpdateChannelNumbers`. Panel: przycisk „Numeracja” na ekranie Kanały, okno z tabelą (numer – pole, nazwa), kontrola duplikatów przed wysłaniem, błąd legacy w tostach. Wiersze w `bridge/commands.txt`; DOKUMENTACJA §4 (Kanały), §8.

### K4 – własne rozdzielczości (LUKI #24)
Legacy: Settings → FFmpeg: `GetAllResolutions` (jest), `CreateCustomResolution` (nazwa, szerokość, wysokość), `DeleteCustomResolution` (tylko własne – `IsCustom`). Panel: lista na ekranie Profile kodowania pod wyborem rozdzielczości albo w Ustawieniach → FFmpeg.

### K5 – silnik Next, paczka diagnostyczna (LUKI #11, #27)
`GetNextVersion`, `GetNextDescribe`, `GetNextProbe`, `RefreshNextProbe` (legacy ≥ 26.12.0; starsze – brak komendy = brak sekcji). Profile kodowania: przy wybranej akceleracji „działa / nie działa i dlaczego”, kodeki dekodowania i kodowania sprzętowego (jak edytor profilu legacy 26.12.0). Diagnostyka: karta „Silnik Next” z wersją, akceptowanymi wersjami schematów i „Sprawdź ponownie”. Paczka diagnostyczna (`ArchiveTroubleshootingResults`): sprawdzić, co zwraca; jeśli plik da się podać przez dyspozytor albo istniejący endpoint mostu – przycisk „Pobierz paczkę diagnostyczną”, w przeciwnym razie zostawić #27 z opisem powodu.
*Zrobione 2026-11-09:* ZIP powstaje tylko w katalogu tymczasowym legacy (`ArchiveTroubleshootingResults` zwraca ścieżkę) – bez nowego kodu C# nie do pobrania; #27 zostaje (DOKUMENTACJA §4).

### K6 – wyniki zadań w tle (LUKI #10)
`CheckForOverlappingPlayoutItems` i zadania konserwacji wracają jako `queued`, a wynik jest tylko w logu legacy. Po zleceniu panel czyta `GetRecentLogEntries` przez ≤ 60 s i pokazuje w oknie wpisy z czasem po zleceniu, pasujące do zadania (wzorce komunikatów z kodu legacy – wypisać w DOKUMENTACJA §4); brak wpisu = „wynik w Logach”. Potrzebne dla K9 (nakładanie pozycji jako błąd ramówki).

### K7 – playout blokowy: diagnostyka i historia (LUKI #13, część #25)
Legacy: `BlockPlayoutTroubleshooting.razor` i `BlockPlayoutHistory.razor` – `GetAllBlockPlayouts`, `GetBlockPlayoutHistory`, `DecodePlayoutHistory`. Panel: w edytorze playoutu blokowego zakładka „Historia” (tylko odczyt: blok, pozycja, ostatnia emisja, zdekodowany stan). Przed dopisaniem do białej listy sprawdzić, czy wyniki nie niosą obiektów wewnętrznych (DOKUMENTACJA §5); wtedy uwaga w `commands.txt` jak dziś przy `GetBlockPlayoutHistory`. Walidator YAML z #25 – poza przebiegiem (R2).

### K8 – biblioteki: obrazy, serial, napisy (LUKI #18, #26)
Komendy na białej liście bez UI: `GetImageFolders`, `UpdateImageFolderDuration` (czas wyświetlania obrazów w folderze), `QueueShowScanByLibraryId` (skan jednego serialu – w karcie serialu w Przeglądarce). Nowe: `ExtractEmbeddedSubtitles`, `ExtractEmbeddedShowSubtitles` (zadanie w tle; przycisk w karcie pozycji / serialu, wynik jak w K6).

### K9 – kontrola ramówki (Vizrt B)
Na wzór kontroli playlisty Vizrt, tylko z danych, które panel już ma: `GetFuturePlayoutItemsById` (dziury między pozycjami, playout kończący się w ciągu < 24 h, kanał bez playoutu), `GetPlayoutWarningsCount`, wynik nakładania pozycji (K6), pozycje z plikiem w koszu (`QuerySearchIndex…` `state:FileNotFound` dla pozycji w najbliższych 24 h), CG Playout `/api/live/state` (audycja LIVE w ramówce bez wejścia studia, zdjęta grafika). Wynik: lista „Uwagi” w Na antenie (jest – rozszerzyć), czerwone znaczniki pozycji w EPG i Playoutach z opisem w podpowiedzi. Liczone w przeglądarce przy odświeżeniu ekranu, bez pętli w serwerze. Bez zapisu.

### K10 – mozaika kanałów (Vizrt J)
Na antenie: przełącznik „Mozaika” – siatka podglądów (`player.js`/`mux.js`), wyciszone, dźwięk tylko w wybranym, czerwona ramka kanału z błędem z K9. **Uwaga kosztowa:** każdy podgląd to sesja Next, która transkoduje (na Ampere A1 2 OCPU może nie wystarczyć) – limit podglądów (opcja `VIZRS_LIVE_MULTIVIEW_MAX`, domyślnie 4) i ostrzeżenie; rozważyć miniatury co N s. DOKUMENTACJA §3 „UI”.

### K11 – import ramówki z pliku (Vizrt E)
Plik CSV (start, tytuł albo ścieżka pliku, długość) lub XML wgrany w przeglądarce, parsowany w JS. Dopasowanie do mediów legacy (`QuerySearchIndexAllItems` po ścieżce, potem po tytule), podgląd: dopasowane / niejednoznaczne / brak. Zapis jako **playlista legacy** (`CreatePlaylist`, `ReplacePlaylistItems` – są na liście) do użycia w harmonogramie. Bez zapisu plików na `/config` (R2). Godziny emisji wymusza harmonogram (pozycja „stała godzina”). Harris/Omnibus i godziny co do sekundy – poza punktem (§6).

### Q1 – prawdziwy sygnał i Plex (LUKI #3, ręcznie)
Z użytkownikiem, na jego stosie: OBS → RTMP/SRT → CG Playout → przejęcie kanału, ZDEJMIJ/WPUŚĆ, audycja LIVE z ramówki; Plex – logowanie PIN i synchronizacja. Wynik w DOKUMENTACJA §7 i STATUS. Najlepiej przed K12–K17, które rozbudowują część live.

### K12 – warstwy VizRS na antenie (LUKI #22; CG)
CG Playout: w `/api/live/state` dla kanału `layers`: `[{layer, page, scene, since}]` – CG zna je z `STATE` VizRS (woła je już przy „Zdejmij grafikę”). Panel: pigułki „W10 · belka 101” w karcie kanału (makieta M2).

### K13 – TAKE / CONTINUE / OUT (CG)
CG Playout: `POST /api/live/channels/<n>/take` `{page}`, `/continue` `{layer}`, `/out` `{layer}` → komendy TCP VizRS na antenie kanału, w tle z limitem czasu („grafika nigdy nie zatrzymuje emisji”), wpis w dzienniku z `X-Operator`. Panel: przy pigułkach z K12 „Dalej”/„Zdejmij”, w karcie kanału „Wprowadź stronę…” (lista z `/etv/pages`) – odpowiednik Quick Take. Ręcznie wprowadzona strona ma pierwszeństwo przed regułą do czasu zdjęcia (ustalić w CG).

### K14 – dziennik emisji (as-run) (CG)
CG Playout zapisuje start i koniec każdej pozycji kanału (z danych szablonu ErsatzTV), wprowadzone i zdjęte strony, przejęcia live (ZDEJMIJ/WPUŚĆ), zdjęcie całej grafiki – JSONL z rotacją jak `log.jsonl`, `GET /api/live/asrun?channel=&from=&to=`. Panel: ekran „Dziennik emisji” (filtr kanału i dnia, tabela), eksport CSV w przeglądarce.

### K15 – zdarzenia wtórne (CG)
Dziś reguły grafiki są ogólne (każda pozycja, przerwa, slot). Dodać regułę z celem: audycja (tytuł), kolekcja albo ID pozycji legacy, z listą zdarzeń `{page, offset_s od startu|końca, duration_s, values}` – kilka zdarzeń na pozycję („Za chwilę” 30 s przed końcem, belka tytułowa po 10 s). Walidacja w CG (nakładające się warstwy = błąd). Panel: w karcie pozycji (EPG, Playouty) oś pozycji z zaplanowanymi zdarzeniami i „Dodaj zdarzenie”.

**Zrobione 2026-11-24:** CG Playout `src/secondary.rs` (planowanie przez `timed()` z czasem trwania), panel `views/zdarzenia.js` (Grafika kanału, karta pozycji w EPG). **Odstępstwo:** cel tylko `show_title`/`title` (regex) – ErsatzTV nie podaje elementowi „script” kolekcji ani ID pozycji playoutu, więc `collection`/`item_id` CG odrzuca z komunikatem; dałoby się je dodać zapytaniem mostu o pozycję (nowy kod C#) – osobna decyzja. Nakładanie sprawdzane dla tej samej strony (warstw stron sidecar nie zna). Oś pozycji w EPG, w Playoutach – nie (karta tylko w EPG).

### K16 – ważność i zatwierdzanie stron promo (CG)
Reguła/zdarzenie dostaje `valid_from`, `valid_to` i `approved` (kto, kiedy); CG pomija niezatwierdzone i przeterminowane. Panel: pigułki „czeka na zatwierdzenie” / „wygasa jutro”, filtr. Wzór: VCB 5.4.

**Zrobione 2026-11-24:** brak klucza `approved` = bez zatwierdzania (zwykłe zdarzenia nie wymagają klikania), `null` = czeka, `{by, at}` = zatwierdzone; daty – data lokalna startu pozycji. Panel: zatwierdza konto zalogowanego operatora; zmiana treści zatwierdzonej reguły cofa zatwierdzenie. Od 26.11.30 zatwierdza tylko administrator (UI i strażnik w serwerze panelu); cel `path`/`start` zamiast kolekcji i ID pozycji; karta pozycji także w Playoutach.

### K17 – alarmy
Panel (przeglądarka): banner i opcjonalny dźwięk, gdy K9 znajdzie błąd w najbliższych 15 min, slot LIVE bez sygnału, emisja kanału zatrzymana, grafika zdjęta > N min. Webhook (poza przeglądarką) – w CG Playout, który ma pętlę zdarzeń; opcja `VIZRS_ALERT_WEBHOOK` po stronie CG, źródło zdarzeń z K14.

### K18 – podgląd HEVC i inne dźwięki (LUKI #21)
`mux.js` przepakowuje tylko H.264 + AAC. Sprawdzić koszt `hvcC` (HEVC) i dźwięku AC-3/E-AC-3 w MSE (`MediaSource.isTypeSupported`); jeśli wykonalne bez bibliotek – dodać, inaczej opisać w DOKUMENTACJA §3 i zostawić lukę. Najniższy priorytet.

### K19 – konta operatorów (decyzje użytkownika 2026-11-18)
Logowanie tokenem przez www jest uciążliwe; konfiguracja tylko z poziomu www, bez edycji plików i zmiennych. Decyzje: role **administrator + operator**; stary token **zostaje awaryjnie** (skrypty, przycisk „Token awaryjny”, pierwsze konto); konta **osobne w każdej aplikacji pakietu** – Trio bywa uruchamiane samo jako sterownik grafiki dla OBS, bez panelu i ErsatzTV, więc nie może zależeć od kont panelu. Zrobione: DOKUMENTACJA §11 „Konta operatorów”, API.md §2.

### K20 – okno AppImage z kontem
Pośrednik okna (`desktop/src/proxy.rs`) dokleja dziś `Bearer <token>`. Z kontami: okno „Połącz z panelem” pyta o login i hasło, samo woła `POST /api/login`, trzyma identyfikator sesji (nie hasło) w `live-connections.json` i dokleja ciasteczko `vizrs_live`; 401 → okno połączenia. Token zostaje jako druga ścieżka.

**Zrobione 2026-11-24 – odstępstwo od szkicu:** login i hasło wpisuje się na ekranie logowania panelu w oknie (ten sam co w przeglądarce: pierwsze konto, „Token awaryjny”), nie w oknie połączenia – pośrednik zatrzymuje `Set-Cookie: vizrs_live` u siebie, dokleja sesję (pierwszeństwo przed tokenem) i zapisuje ją w `live-connections.json`. 401 przy sesji = sesja zapomniana i ekran logowania panelu zamiast okna połączenia. Dzięki temu wylogowanie, zmiana konta i zakładanie pierwszego konta działają w oknie bez osobnego kodu. Okna natywne VizRS CG System (`vizrs-shell`) i Artist – dalej tokenem (K21, „Otwarte”).

### K21 – jedno konto w pakiecie (decyzja użytkownika 2026-11-18)
„Pozwól na logowanie jednym kontem, ale jako opcję, a nie konieczność” → jedno konto w całym pakiecie: panel jest źródłem kont, VizRS CG System przyjmuje je opcjonalnie. Przełącznik w panelu (domyślnie wyłączony), adres panelu w compose (`VIZRS_PANEL_LOGIN`); Trio samo (OBS) bez zmian – token. Zamiast osobnych kont w CG System (dawny pomysł K21) – CG System nie trzyma haseł. Zrobione: DOKUMENTACJA §11 „Logowanie w pakiecie”, API.md §2 `/api/sso…`, CG System DOKUMENTACJA §12. Otwarte: okna natywne (`vizrs-shell`, Artist) dalej tokenem – razem z K20.

### Nagła emisja na żywo „dziś” – K22–K24 (analiza 2026-11-26, decyzje użytkownika 2026-11-28)
Przypadek: emisja LIVE ogłoszona tego samego dnia (np. dziś 22:00 na S1 ch1), o godzinie, którą zajmuje stała pozycja harmonogramu (Wojtek). Dziś działa tylko ścieżka ręczna: `ingest_anytime=1` kanału i WPUŚĆ w Slotach na żywo. Ustalenia analizy: legacy ma pozycje bez daty (najwyżej harmonogram alternatywny z zakresem dat – z rokiem albo bez, poprawka z rozpoznania K24); `ReplaceProgramScheduleItems` sam uruchamia Refresh, a Refresh i Reset budują od „teraz”; Refresh zachowuje postęp serii, Reset go kasuje. Decyzje: **wariant B** – nagła emisja jako okno przejęcia w CG Playout, legacy i ramówka bez zmian (warianty odrzucone: A – jednodniowy harmonogram alternatywny, C – edycja pozycji playoutu, nadpisywana przez Refresh); **los pozycji pod spodem wybiera operator przy dodawaniu emisji**; przycisk przebudowy – bez zdania użytkownika, przyjęte: Refresh domyślnie (K22).

### K22 – przebudowa bez kasowania postępu
Po zapisie emisji audycji (`views/live.js` `saveAirings` → `rebuildStep`, wiersz `steps.push(await rebuildStep(…))`) panel woła `live/channels/N/rebuild`, a CG Playout robi Reset playoutu (`src/panel.rs` trasa `rebuild` → `src/etv_api.rs` `reset_playout` → `POST /api/channels/N/playout/reset`) – zbędnie, bo legacy już zrobił Refresh, i szkodliwie, bo Reset kasuje postęp odcinków. To samo robi przycisk „⟲ Przebuduj ramówkę” na karcie kanału i przyciski kanałów w oknie audycji.
Zrobić: (1) sprawdzić w kodzie legacy, czy REST ma przebudowę w trybie Refresh; jeśli tak – trasa `rebuild` z `?mode=refresh|reset`, domyślnie `refresh`, wpis w dzienniku zmian CG z trybem; jeśli nie – panel woła przez most `BuildPlayout` z `Mode: 'Refresh'` (jak `views/playouty.js` i `views/antena.js`; potrzebny `GetPagedPlayouts` do `PlayoutId`), a `rebuild` w CG zostaje Resetem opisanym jako „od nowa”. (2) Po zapisie emisji: Refresh albo nic, gdy zapisano tylko harmonogram (legacy już przebudował) – zostawić Refresh, gdy zmieniły się dane audycji widoczne w EPG. (3) Przycisk na karcie kanału: Refresh; Reset tylko w Playoutach z potwierdzeniem „kasuje postęp odcinków”. Gotowe: test jednostkowy trasy (tryb, dziennik) albo e2e z atrapą mostu (wołany `BuildPlayout` z `Refresh`, brak `playout/reset`), DOKUMENTACJA §1 „Live API”, API.md, INSTRUKCJA (co robi przycisk).

### K23 – nagła emisja z datą (okno przejęcia, CG)
**CG Playout.** Nowy obiekt kanału: okno nagłej emisji `{id, channel, start (ISO z strefą), end, input|null, title, under: "replace"|"keep", created_by}`. W oknie kanał zachowuje się jak przy `anytime=1` i bez `hold` (źródło przejmuje antenę od pierwszej klatki kluczowej, `ChannelControl` operatora ma pierwszeństwo tylko przez jawne WSTRZYMAJ), wejście `input` – jak wybór wejścia w Slotach; po `end` wraca stan sprzed okna. Okna trzeba zapisywać na wolumenie razem ze stanem kanałów (`<etv_config>/vizrs-live/control.json`, `src/panel.rs`, zapis atomowy) – restart sidecara przed 22:00 nie może zgubić emisji; okna po `end` usuwać przy starcie. Walidacja: `end > start`, start nie w przeszłości (z tolerancją minuty), bez nakładania okien jednego kanału. API: `GET/POST /api/live/channels/N/windows`, `DELETE …/windows/<id>`; stan kanału (`/ingest/state`, `/api/live/state`) z polem `window` (bieżące albo najbliższe). Dziennik zmian (`record`) przy dodaniu i usunięciu, dziennik emisji (as-run) – początek i koniec okna z tytułem. Grafika: jak dziś przy przejęciu (`on_live`, `live_watermark`); brak sygnału w oknie = ramówka gra dalej (jak dziś przy przejęciu bez źródła) i alarm K17 „okno bez sygnału” po np. 60 s.
**Panel.** Na karcie kanału w Slotach na żywo „Nagła emisja”: data (domyślnie dziś), od–do, wejście, tytuł; podgląd, co ramówka ma pod spodem w tym czasie (`GetFuturePlayoutItemsById`), i wybór losu tej pozycji: **„zastąp – odcinek leci pod spodem i przepada”** (`replace`, bez zmian w legacy) albo **„zachowaj odcinek”** (`keep` – pole widoczne dopiero po K24; brak w CG = brak opcji). Lista zaplanowanych okien z usuwaniem. Starszy CG Playout bez `windows` – brak przycisku.
**Znane ograniczenie wariantu B (przyjęte):** EPG/XMLTV z legacy pokazuje pozycję spod okna, nie nagłą emisję. Ewentualna poprawka XMLTV po stronie CG – osobny punkt po K23, jeśli użytkownik zechce.
Gotowe: testy jednostkowe okna (start/koniec, nakładanie, zapis i odczyt po restarcie, `hold` w oknie), test kontraktu API, e2e panelu (dodanie, lista, usunięcie, brak przycisku przy starym CG), DOKUMENTACJA §1 „Live ingest”/„Live API”, API.md, INSTRUKCJA, RUNBOOK (co zrobić, gdy okno nie przejęło anteny). Ręcznie na S1 z użytkownikiem: okno na kanale testowym z prawdziwym OBS (jak Q1).

### K24 – zachowanie odcinka pod nagłą emisją
W wariancie B pozycja legacy gra pod oknem, więc postęp jej serii idzie dalej i odcinek przepada. Opcja `keep` z K23 wymaga, żeby odcinek poleciał w następnej emisji. Rozpoznanie (najpierw, bez kodu): w kodzie legacy – gdzie żyje postęp pozycji (punkty kontrolne playoutu klasycznego), czy da się go cofnąć komendą z MediatR (most, biała lista) bez Resetu; jeśli nie – czy okno może zamiast tego wstrzymać pozycję w legacy tylko na ten dzień bez łamania R2 i bez harmonogramu alternatywnego (wariant A odrzucony). Wynik rozpoznania z kosztami do decyzji użytkownika; bez wykonalnej drogi – opcja `keep` nie powstaje, a K23 zostaje z samym `replace`.

**Rozpoznanie (2026-11-28).** Źródło: `ErsatzTV/legacy` `main` `d508bec` (2026-11-25); od `84caf12` w tych miejscach zmienił się tylko podręczny zbiór pozycji kolekcji (`CollectionItemsCache`), logika ta sama. Pliki: `ErsatzTV.Core/Scheduling/PlayoutBuilder.cs` (`RefreshPlayout`, `ResetPlayout`, `SaveCheckpointAnchors`, `BuildProgramScheduleAnchors`, `GetMediaCollectionEnumerator`), `Core/Domain/{PlayoutProgramScheduleAnchor,CollectionEnumeratorState,ProgramScheduleAlternate}.cs`, `Core/Scheduling/AlternateScheduleSelector.cs`, `Application/Playouts/Commands/{ReplacePlayoutAlternateScheduleItemsHandler,UpdateOnDemandCheckpointHandler}.cs`, `Application/Scheduling/Commands/ErasePlayoutHistoryHandler.cs`, `Application/ProgramSchedules/Commands/ReplaceProgramScheduleItemsHandler.cs`.
- **Gdzie żyje postęp.** Wiersze `PlayoutProgramScheduleAnchor` playoutu, jeden na klucz kolekcji (kolekcja, kolekcja inteligentna, multi, playlista, zapytanie, pojedyncza pozycja). Każdy ma `CollectionEnumeratorState {Seed, Index, Started}`. Wiersz z `AnchorDate = null` to stan bieżący (koniec zbudowanej ramówki). Wiersze z datą to punkty kontrolne: `SaveCheckpointAnchors` zapisuje je w czasie budowy przy każdym przejściu przez lokalną północ, najwyżej jeden na klucz i dzień.
- **Refresh** usuwa stany bieżące i punkty kontrolne inne niż dzisiejszy. Potem buduje od dzisiejszego punktu kontrolnego, czyli od początku doby, z tym samym ziarnem: dzisiejsze pozycje wychodzą takie same, a punkt kontrolny jutra liczy się od nowa. **Reset** kasuje wszystkie wiersze i losuje nowe ziarno. Postęp to więc to, co **zaplanowano**, nie to, co poleciało: legacy nie wie, że pozycję przykryło okno CG.
- **Komendy MediatR, które ruszają postęp:** tylko `BuildPlayout` (Continue/Refresh/Reset), `ErasePlayoutHistory` (kasuje stany, pozycje i historię – więcej niż Reset) i `UpdateOnDemandCheckpoint` (tylko kanały na żądanie, przesuwa czas, nie `Index`). **Nie ma komendy, która ustawia albo cofa `Index` jednej kolekcji.** Cofnięcie postępu wymaga zmiany kodu legacy albo zapisu w jego bazie, a zasady pakietu wykluczają jedno i drugie. Ta droga odpada.
- **Co zostaje:** odcinek się zachowa, gdy harmonogram aktywny **tego dnia** nie ma pozycji spod okna. Dzisiejsza budowa nie zużyje wtedy odcinka, a punkt kontrolny jutra zapisze stan bez niego. Legacy wybiera harmonogram raz na dobę (`AlternateScheduleSelector.GetScheduleForDate` w każdym przejściu budowy). **Poprawka do analizy z 2026-11-26:** harmonogram alternatywny ma `StartYear`/`EndYear` (`LimitToDateRange`, zakres włącznie). Komenda `ReplacePlayoutAlternateScheduleItems` je przyjmuje, a panel już je obsługuje (`web/when.js`). Jeden dzień z rokiem jest więc możliwy i nie wraca za rok.

**Drogi do decyzji:**

| | Jak | Koszt | Ryzyka i ograniczenia |
|---|---|---|---|
| **W1 – jednodniowy harmonogram alternatywny tylko dla `keep`** (**wybrana** 2026-11-28) | Przy dodaniu okna z `keep` panel przez most: `CopyProgramSchedule` (kopia harmonogramu kanału na ten dzień, nazwa z datą i kanałem). W kopii zdejmuje pozycję spod okna (`GetProgramScheduleItems` → `ReplaceProgramScheduleItems`). Potem `ReplacePlayoutAlternateScheduleItems`: nowy wiersz z `LimitToDateRange`, rok = data okna, najwyższy priorytet. Legacy sam robi Refresh, gdy zmienia się harmonogram dnia. CG zapisuje w oknie odnośnik (id kopii i wiersza). Usunięcie okna albo dzień po oknie: zdjęcie wiersza i `DeleteProgramSchedule` kopii. Okno samo w sobie zostaje wariantem B. | Jedna sesja: panel, pole odnośnika w oknie CG z K23, e2e z atrapą mostu. Wszystkie komendy są już na białej liście, więc bez nowych wierszy w `bridge/commands.txt`. | Otwiera ponownie odrzucony wariant A, ale tylko jako wykonanie `keep`. Zmiana harmonogramu głównego po dodaniu okna (Audycje, Harmonogramy) nie trafia w ten dzień do kopii, więc panel musi ostrzec albo odświeżyć kopię. Działa tylko w playoucie klasycznym. Pozycji, która już się zaczęła, `keep` nie obejmuje. Odcinek poleci w następnej emisji tej **kolekcji** – także pod inną godziną, jeśli kolekcja jest w kilku pozycjach. Pozostałość po awarii sprzątania jest nieszkodliwa (rok w zakresie) i widać ją w Playoutach. Przy okazji: w kopii można wstawić w miejsce pozycji kolekcję `LIVE · …` nagłej emisji, wtedy EPG pokazuje emisję (znane ograniczenie B). Wymaga to kolekcji z ważną pozycją, bo pusta zatrzymuje budowę kanału. |
| W2 – tymczasowa zmiana pozycji w harmonogramie głównym do lokalnej północy, potem przywrócenie | `ReplaceProgramScheduleItems` przed oknem i po północy (Refresh po każdej zmianie robi legacy) | podobny jak W1 plus zapis zamiaru w `control.json` i przywracanie przy starcie | Gorsza od W1: harmonogram bywa wspólny dla kilku kanałów (`ProgramSchedule.Playouts`), więc zmiana trafia we wszystkie. Brak pozycji jest we wszystkich zbudowanych dniach do przywrócenia. Przywrócenie po awarii CG po następnej emisji puszcza dziurę bez okna. Edycje harmonogramu w tym czasie się nadpisują. **Odradzana.** |
| **W3 – bez `keep`** | K23 tylko z `replace`, w panelu brak opcji | zero | Odcinek spod okna przepada, jak dziś przy ręcznym WPUŚĆ. |

**Decyzja użytkownika (2026-11-28): W1.** K24 wykonuje się po K23 według wiersza W1; gotowe – e2e panelu (kopia, wiersz alternatywny z rokiem, sprzątanie, ostrzeżenie przy zmianie harmonogramu), test CG (odnośnik w oknie przeżywa restart), DOKUMENTACJA §1 i §12, INSTRUKCJA.

## 6. Legacy czy Next

**Decyzja (2026-11-09): A – zostajemy przy ErsatzTV legacy.** B (VizRS jako planista dla samego Next) i C1 (osobny lineup Next) – odrzucone. C2 (Scripted Schedule w legacy, ramówka co do sekundy) zostaje możliwy w ramach A, ale tylko po zmianie decyzji R2 (LUKI #14) – wtedy nowy punkt w §4. Do rozważenia B wrócić dopiero, gdy Next wyda 1.0 z API zarządzania albo legacy przestanie być rozwijane (sygnał: brak wydań > 2 miesiące, baner końca wsparcia).

Analiza, na której oparto decyzję:

### Stan faktyczny (2026-11-09)

- Od **legacy v26.12.0** silnik nadawania legacy jest usunięty: **wszystkie kanały nadaje już ErsatzTV Next** (legacy przenosi kanały przy starcie, adresy `ts-legacy`/`hls-direct` przekierowuje). VizRS Live **już korzysta z Next** – legacy jest dla niego planistą, bazą, skanerem i dostawcą EPG.
- **Next 0.2.5** (Rust, MIT, przed 1.0 – `0.B` = zmiana niezgodna) z założenia **nie** ma bibliotek, metadanych, harmonogramów ani tworzenia playoutów; **nie ma API zarządzania** – tylko pliki JSON z wersją (`lineup.json`, `channel.json`, katalog playoutu `{start}_{finish}.json`, wczytywany ponownie po zmianie) i HTTP: `/channel/{plik}` (HLS), `/channels.m3u`, `/xmltv.xml` (XMLTV z plików, które pisze planista). Bez uwierzytelniania. HDHomeRun, logo i okładki EPG, Plex/Jellyfin/Emby – tylko w legacy.
- Mocne strony Next dla VizRS: źródło **`dynamic`** (pozycja rozwiązywana zapytaniem HTTP w chwili emisji), źródła `http`/`rtsp` z `is_live`, **warstwy grafiki** w playoucie (`graphics[]`, w tym `canvas` – wideo z alfą z HTTP z nagłówkami offsetu; tak legacy podaje elementy graficzne, a CG Playout element „script” VizRS), **fallback** z obrazem tła i opisem błędu, `describe --probe`.

### Opcje

| | A. Zostać na legacy – **wybrane** | B. Porzucić legacy: VizRS jako planista dla Next | C. Hybryda: legacy zostaje, VizRS przejmuje wybrane kanały lub ramówki |
|---|---|---|---|
| Co robimy | most i panel jak dziś, przebieg z §4 | piszemy: bazę mediów i skaner (ffprobe), kolekcje, harmonogramy/bloki/wypełniacze, budowanie playoutu, XMLTV, uruchamianie `ersatztv`, proxy z autoryzacją | C1: kanały „traffic” jako **osobny lineup Next** obok legacy; C2: ramówka z pliku wewnątrz legacy przez **Scripted Schedule** (playout `Scripted`, skrypt woła `add_*`, `wait_until_exact`, `pad_until_exact`, `graphics_on/off`) |
| Koszt | najniższy | bardzo wysoki – w praktyce przepisanie legacy (~157 tys. linii C# według PLAN-vizrs-live §4) | C1 średni; C2 niski–średni (skrypt na `/config` – LUKI #14) |
| Zyski | pełne funkcje legacy (biblioteki, Plex/Jellyfin/Emby, bloki, deco, EPG, HDHR) | jeden język (Rust), brak .NET (sterta legacy 1,5 GB na Ampere A1), brak zależności od wewnętrznych komend legacy, pełna kontrola nad zdarzeniami wtórnymi i as-run, LIVE przez `dynamic`/`http is_live` bez nakładki `vizrs-ffmpeg` | dokładne godziny emisji z traffic bez przepisywania legacy; C1 – grafika i LIVE natywnie w playoucie Next |
| Ryzyka | most na komendach wewnętrznych: każde wydanie legacy (co tydzień) = test (`commandsMissing`, e2e); .NET na Ampere | Next przed 1.0 (0.2.0 → 0.2.5 w miesiąc), bez API i bez autoryzacji; utrata Plex/Jellyfin/Emby, HDHR; ogromna praca, zanim panel znów zrobi to, co dziś | C1: dwa lineupy (dwa M3U/XMLTV), własny obraz Next do utrzymania; C2: łamie decyzję R2 |

Niezależnie od decyzji: CG Playout może sprawdzić, czy element graficzny legacy z warstwą `canvas` Next zastąpi nakładkę `vizrs-ffmpeg` (PLAN-vizrs-live §4, „Na później”) – praca w repo CG Playout, poza tym planem.

## 7. Harmonogram w stylu ffplayout – analiza (2026-11-11)

**Pytanie użytkownika:** czy sposób układania ramówki z [ffplayout](https://github.com/ffplayout/ffplayout) da się dodać do VizRS Live? **Krótka odpowiedź:** tak, ale tylko jego część „ramówka dnia jako lista plików z wyliczonymi godzinami” i „generator z szablonu”. Da się to zbudować w samym panelu, na komendach z białej listy, bez nowego kodu C#. Nie przeniesiemy tego, co w ffplayout robi silnik: punktów wejścia i wyjścia klipu, cięcia klipu, przeskoku w czasie i edycji tego, co właśnie leci, ze skutkiem od razu. Ani legacy, ani Next tego nie umieją.

**Źródła:** `ffplayout/ffplayout` na `main`, commit `5482009` (2026-11-09): README, `docs/playlist_gen.md`, `docs/folder_mode.md`, `crates/app/src/utils/generator.rs`, `crates/app/src/player/input/playlist.rs`, `frontend/src/views/PlayerView.vue`, `components/player/PlaylistGenerator.vue`, `composables/helper.ts` (`processPlaylist`). Legacy: `ErsatzTV/legacy` `main` (2026-11-11), `ErsatzTV.Core/Domain/{ProgramScheduleItem,ProgramScheduleAlternate,CollectionType}.cs`, `Scheduling/PlayoutTemplate.cs`.

### 7.1 Jak układa ramówkę ffplayout

- **Jedna playlista na dzień i kanał:** plik JSON `playlists/RRRR/MM/RRRR-MM-DD.json`, w nim `program: [{ in, out, duration, source, title?, audio?, ad?, custom_filter?, graphics? }]`. Godzin nie zapisuje się w pliku. Start pozycji = `day_start` (np. 06:00) + suma `out − in` pozycji wcześniejszych. Doba trwa `length` (zwykle 24 h).
- **Edytor dnia** (strona Player): wybór daty, przeciąganie plików z przeglądarki mediów na listę, zmiana kolejności, okno pozycji (`in`/`out`, osobna ścieżka audio, znacznik reklamy, grafika), podział klipu na N części, „zapętl”, czyli powielanie listy do pełnej doby, pozycje poza dobą na czerwono (`overtime`), kopiowanie dnia na inną datę, usuwanie, import TXT/M3U z kontrolą plików, podgląd klipu. Zapis jest jawny, przyciskiem. Gdy ktoś inny zmienił ten dzień, ffplayout ostrzega o niezapisanych zmianach.
- **Generator:** prosty (wszystkie albo wybrane foldery, po kolei albo losowo) i zaawansowany, z szablonu bloków `{ start, duration, shuffle, paths[] }`. Każdy blok wypełnia się plikami z folderów, bez powtórzenia klipu tuż po sobie i z limitem powtórzeń, a resztę bloku dopełnia wypełniacz. Generator pisze JSON dla zakresu dat i pomija dni, które już istnieją. Wywołanie z CLI albo przez `POST /api/playlist/{kanał}/generate/{data}`.
- **Silnik:** wybiera pozycję po zegarze (ta, w której mieści się „teraz”) i przewija ją do bieżącego miejsca. Ostatni klip przycina do końca doby, brakujący czas wypełnia wypełniaczem (plik albo folder w pętli), a gdy nie ma playlisty, gra sam wypełniacz. Pilnuje zgodności z zegarem (`stop_threshold`, wykrywa zmianę czasu letni/zimowy). Operator może przeskoczyć w przód lub w tył (`time_shift` do końca dnia). Inne tryby: `infinit` (jedna playlista w pętli) i tryb folderu (obserwowany katalog, po kolei albo losowo).

### 7.2 Jak to się ma do legacy

| ffplayout | Odpowiednik w legacy (komendy na białej liście) | Ocena |
|---|---|---|
| Szablon bloków `start/duration/shuffle/paths` | **harmonogram klasyczny**: pozycja `StartType=Fixed`, `PlayoutMode=Duration`, `PlaybackOrder=Shuffle`/`Chronological`, ogon `TailMode=Filler` (`ReplaceProgramScheduleItems`). Albo **blok** o długości N min z szablonem dnia (`ReplaceBlockItems`, `ReplaceTemplateItems`) | jest, w legacy bogatsze (kolekcje, inteligentne kolekcje, `SearchQuery`, seriale, wypełniacze przed, w trakcie i po). Brakuje tylko prostego kreatora w stylu ffplayout |
| Generowanie zakresu dat | playout klasyczny lub blokowy buduje się N dni naprzód (`BuildPlayout`), dni różnią się harmonogramami zastępczymi albo kalendarzem szablonów | jest (dynamicznie, bez plików na dzień) |
| **Konkretna lista plików na konkretny dzień** z godzinami wyliczonymi z długości | brak gotowego ekranu. Da się to złożyć: harmonogram „Dzień RRRR-MM-DD” z pozycjami, z których każda to **jeden element mediów** (`CollectionType` = `OtherVideo`/`Movie`/`Episode`…, `MediaItemId`, `PlayoutMode=One`). Pierwsza pozycja `Fixed` o `day_start`, reszta `Dynamic`, ogon z wypełniacza. Harmonogram dołączony jako **zastępczy na jedną datę** (`ProgramScheduleAlternate`: `LimitToDateRange` + `StartYear/EndYear` = ta sama data; `ReplacePlayoutAlternateScheduleItems`) | **wykonalne** w panelu, bez C# i bez plików na `/config` (R2) – to propozycja **K19** |
| `in`/`out`, podział klipu | brak (pozycja legacy gra cały plik) | **niewykonalne** bez zmian w legacy |
| Przeskok w czasie (`time_shift`), „pomiń klip” | brak: Next nie ma sterowania odtwarzaniem | **niewykonalne**; zastępstwo: zmiana ramówki + `BuildPlayout` (Next wczytuje playout po zmianie) |
| Przycięcie ostatniego klipu do końca doby | legacy nie tnie. O styku z następną stałą godziną decyduje `FixedStartTimeBehavior` (ścisła albo elastyczna) i ogon wypełniacza | częściowo; panel pokaże przekroczenie na czerwono, tak jak `overtime` w ffplayout |
| Wypełniacz przy braku playlisty | `FallbackFiller`, kanał bez playoutu = plansza Next | jest |
| Znacznik reklamy `ad`, grafika przy klipie | wypełniacz w trakcie / przerwa reklamowa i reguły grafiki CG Playout. Zdarzenia przy pozycji to K15 | przez CG (K15), nie w ramówce legacy |
| Import TXT/M3U | K11 (CSV/XML → playlista legacy) | jest; K19 może przyjmować ten sam import jako źródło dnia |
| Tryb folderu (obserwowany katalog) | biblioteka lokalna ze skanem + kolekcja albo `SearchQuery` + pozycja `Flood` | jest |
| Dokładne godziny „co do sekundy” z systemu emisyjnego | Scripted Schedule (`wait_until_exact`, `pad_until_exact`) – §6, C2 | tylko po zmianie decyzji R2 |

**Wniosek:** model ffplayout i model legacy różnią się zasadniczo. ffplayout trzyma **gotową listę na każdy dzień**, legacy **opisuje regułę i co dzień buduje z niej listę**. Panel nie powinien podmieniać modelu legacy. Może dać operatorowi **widok i edytor w stylu ffplayout** jako drugi sposób zapisu tej samej ramówki, z zapisem przez komendy legacy.

### 7.3 Propozycja (do decyzji użytkownika – po akceptacji wiersze trafią do §4)

> Uwaga (2026-11-28): numery K19 i K20 z tej propozycji zajęły później konta operatorów (K19) i okno logowane kontem (K20) w §4. „Ramówka dnia” i „Kreator szablonu dnia” nie są zrobione – przy akceptacji dostaną nowe numery (od K22).

**K19 – Ramówka dnia (edytor listy na datę, wzorzec ffplayout).** Nowy ekran albo karta w Playoutach dla kanału z playoutem **klasycznym**:
- wybór kanału i daty, lista pozycji z wyliczonymi godzinami (start doby + suma długości z metadanych legacy), pasek zapełnienia doby i pozycje po końcu doby na czerwono;
- dodawanie z Przeglądarki i wyszukiwarki (jak `contentPicker`), zmiana kolejności, usuwanie, „zapętl do pełnej doby”, „kopiuj dzień na datę…”, źródło z importu K11;
- zapis: `CreateProgramSchedule` „VizRS dzień RRRR-MM-DD · kanał”, potem `ReplaceProgramScheduleItems` (pierwsza pozycja `Fixed`, ścisła, pozostałe `Dynamic`, każda to jeden element mediów, ogon i zapas z wybranego wypełniacza), potem `ReplacePlayoutAlternateScheduleItems` (dopisany zastępczy na tę datę, istniejące zastępcze bez zmian, kolejność wg `Index`), na końcu `BuildPlayout` (Refresh). Kontrola K9 porównuje wynik z planem;
- odczyt dnia: `GetProgramScheduleItems` harmonogramu dnia, a dla porównania faktyczne pozycje z EPG;
- sprzątanie: „usuń dni starsze niż N” (`DeleteProgramSchedule` + zastępcze), ręcznie, z potwierdzeniem.
- **Do sprawdzenia przed startem, na stosie testowym (automat nie wdraża):** (1) czy dopasowanie zastępczego po `StartYear/EndYear` działa w bieżącym legacy; (2) zachowanie pozycji `Fixed` ścisłej, gdy poprzedni dzień przekroczył dobę (legacy może przesunąć start na następny dzień); (3) czy legacy przyjmuje `ReplaceProgramScheduleItems` z pozycją pojedynczego elementu mediów (`MediaItemId`) – jeśli nie, zapasowo jedna playlista legacy na dzień (`CreatePlaylist` + `ReplacePlaylistItems`, jak w K11) i jedna pozycja harmonogramu z tą playlistą; (4) ile harmonogramów zastępczych znosi playout (wydajność budowania).
- **Wymaga:** – (wszystkie komendy są już na białej liście). **Koszt:** średni, wyłącznie JS w panelu (`views/dzien.js`, wpis w `STATIC`), e2e w `tests/e2e.cjs`.

**K20 – Kreator szablonu dnia (generator ffplayout „zaawansowany”).** Okno w Harmonogramach: wiersze `start · czas · źródło (kolekcja, inteligentna kolekcja, zapytanie) · losowo/po kolei · wypełniacz` → komplet pozycji `Fixed` + `Duration` + `TailMode=Filler` w nowym albo wybranym harmonogramie (`ReplaceProgramScheduleItems`). Panel sprawdza, czy bloki się nie nakładają i czy dają razem pełną dobę. Wariant dla playoutu blokowego: bloki o tej długości i szablon dnia. **Koszt:** niski (logika jest w `views/harmonogramy.js`, kreator tylko składa pozycje). **Wymaga:** –.

**Odrzucone (z powodem):** `in`/`out` i podział klipu, przeskok w czasie, edycja tego, co właśnie leci, ze skutkiem od razu (legacy i Next tego nie umieją, a zmiana wymagałaby forka – zasada „legacy bez zmian”); własne pliki JSON dnia, czytane przez Next albo przez ExternalJson legacy (zapis na `/config` – R2, a ExternalJson przyjmuje format dizqueTV, nie listę plików z czasami).

## 8. Przepisanie legacy do Rusta – VizRS Live jako GUI i planista dla Next (analiza 2026-11-11)

**Pytanie użytkownika:** jaki jest plan i sens przepisania ErsatzTV legacy do Rusta, tak żeby VizRS Live stało się GUI (i planistą) dla ErsatzTV Next? To wariant **B** z §6, odrzucony 2026-11-09. Ta sekcja nie zmienia tej decyzji – rozpisuje B na etapy, żeby można było podjąć ją świadomie.

**Krótka odpowiedź:** jako **cel długoterminowy ma to sens**. Jako **jednorazowe przepisanie teraz – nie**: to ok. 118 tys. linii logiki C# (8.2), a Next jest przed 1.0 i nie ma API. Proponowana droga to **stopniowe przejmowanie** (wzorzec *strangler*). VizRS Live najpierw samo pisze playouty Next dla wybranych kanałów: ramówka dnia co do sekundy, LIVE, grafika. Legacy zostaje dla pozostałych kanałów i jako biblioteka mediów. Każdy etap daje wartość sam w sobie i kończy się bramką „idziemy dalej / zostajemy”. Pierwsze dwa etapy (N0–N1) dają to, czego wariant A nie da nigdy, bez utraty niczego z legacy.

**Źródła:** `ErsatzTV/legacy` `main` `c41969b` (2026-11-10) – liczby wierszy `*.cs` bez migracji i testów; `ErsatzTV/next` `c5cb065` (v0.2.5, 2026-11-06) – `schema/playout.json`, trasy `crates/ersatztv/src/main.rs` (`/channel/{plik}`, `/channels.m3u`, `/xmltv.xml` – nic więcej).

### 8.1 Sens – co by to dało

| | Dziś (A: legacy + most) | Po przejęciu przez VizRS Live |
|---|---|---|
| Ramówka co do sekundy, punkty wejścia i wyjścia, podział klipu | niewykonalne (§7: K19 bez `in/out`, odrzucone) | Next ma `in_point_ms`/`out_point_ms` w źródle `local` i `http`; ramówka dnia w stylu ffplayout (§7.1) wprost jako plik playoutu |
| Audycje LIVE | CG Playout zakłada Remote Stream + Smart Collection, pozycję harmonogramu operator dodaje ręcznie (LUKI #15) | pozycja `http` z `is_live` albo `dynamic` (rozwiązywana w chwili emisji – przejęcie kanału bez nakładki) |
| Grafika | reguły CG Playout + nakładka `vizrs-ffmpeg` | warstwa `canvas` / `graphics[]` w samej pozycji playoutu; zdarzenia wtórne (K15) i as-run (K14) z naszych plików, nie z odgadywania |
| Odporność na wydania | legacy co tydzień; most na wewnętrznych komendach (`commandsMissing`, e2e) i hakach startowych .NET | jeden kontrakt: schematy Next z wersją (`describe` podaje akceptowane wersje) |
| Zasoby | sterta .NET legacy ok. 1,5 GB na Ampere A1 (2 OCPU) | Rust: dziesiątki MB; ten sam Next do kodowania |
| Luki G2 | wynikają z tego, że legacy trzyma stan poza MediatR (#5, #8, #9, #12, #27) albo w plikach (#14, #20, #25) | znikają – to nasz kod i nasz magazyn |
| Zasady repo | C# w moście, biała lista jako granica bezpieczeństwa | jeden język, bez C#; most i `commands.txt` do usunięcia na końcu |

**Koszt sensu:** tracimy darmowe poprawki planisty, skanera i integracji, które legacy wydaje co tydzień. Od tej chwili każdy błąd planowania jest nasz.

### 8.2 Ile tego jest

| Część legacy | Wiersze C# | W VizRS Live |
|---|---|---|
| UI Blazor (`ErsatzTV/`) | 11,9 tys. | **już jest** – panel (ok. 10 tys. wierszy Rust + JS) |
| Stary silnik: `ErsatzTV.FFmpeg`, `Infrastructure/Streaming` | 16,6 tys. | **nie przepisujemy** – robi to Next |
| Planista: `Core/Scheduling` (klasyczny, blokowy 1,8, YAML 3,8, silnik skryptów 2,2) + `Infrastructure/Scheduling` | 17,2 tys. | **rdzeń** – klasyczny i blokowy; YAML i skrypty pominięte (8.6) |
| Skaner: `ErsatzTV.Scanner` (metadane 9 tys.: NFO, ffprobe, okładki) | 15,2 tys. | lokalny – tak; Plex/Jellyfin/Emby – osobny etap (N6) |
| Plex, Jellyfin, Emby (`Infrastructure`, poza skanerem) | 5,8 tys. | opcja (N6) |
| Dane: EF Core (`Infrastructure/Data`) | 15,2 tys. | własny magazyn (8.3) – znacznie mniej kodu |
| Wyszukiwarka Lucene (`Infrastructure/Search`) | 3,4 tys. | prosty indeks w pamięci + język zapytań inteligentnych kolekcji |
| `ErsatzTV.Application` (handlery MediatR) | 36,7 tys. | w większości znika: handler = trasa API panelu |
| Synchronizacja z Next (`Core/Next`) | 3,4 tys. | **przepisać jako pierwsze** – to styk z Next |

**Szacunek:** 35–55 tys. wierszy Rusta. Przy zasadzie „jeden punkt = jedna sesja” to rząd **40–60 sesji**. Szacunek jest zgrubny – największą niewiadomą jest wierność planisty blokowego i deco.

### 8.3 Twarde przeszkody

1. **Zero zależności w serwerze** (zasada repo) koliduje z:
   - bazą – własny magazyn: plik JSON na kolekcję encji + dziennik zapisów, zapis atomowy (`rename`), wszystko w pamięci (skala: dziesiątki tysięcy pozycji mediów – mieści się);
   - **TLS** – Plex (`plex.tv`), Jellyfin/Emby przez https. Do wyboru: `curl` jako proces, Caddy jako pośrednik wychodzący albo wyjątek od zasady. **Decyzja użytkownika**;
   - NFO (XML) – mały parser; YAML – pomijamy razem z harmonogramami YAML;
   - miniatury okładek – `ffmpeg` jako proces (jest w obrazie Next).
2. **Next przed 1.0** – 0.2.0 → 0.2.5 w miesiąc, zmiana `0.B` = niezgodność. Zabezpieczenie: generator playoutu sprawdza `describe` (akceptowane wersje schematów), a w CI test zgodności z najnowszym obrazem `ersatztv/next` co tydzień (jak `legacy-latest.yml`).
3. **Next bez API i bez autoryzacji** – sterowanie wyłącznie plikami: `lineup.json`, `channel.json`, katalog playoutu `{start}_{finish}.json` (wczytywany ponownie po zmianie) i XMLTV `{tvg_id}.xml`. Panel potrzebuje **wspólnego wolumenu z Next** – to nowy rodzaj zapisu. Nie dotyczy `/config` legacy ani CG, ale wymaga jawnej decyzji (rozszerzenie R2). Widzowie dalej tylko przez Caddy.
4. **Migracja danych z legacy** – bez czytania bazy legacy (zasada repo, a SQLite i tak wymagałby zależności): **eksport przez most**, czyli te same komendy `Get…/Query…`, które panel już ma na białej liście, do JSON-a, potem import do magazynu. Most przydaje się do samego końca.
5. **Wierność planisty** – semantyka harmonogramów, bloków, deco, wypełniaczy i inteligentnych kolekcji musi dać tę samą ramówkę. Zabezpieczenie: **testy wzorcowe** – te same dane, playout legacy (`GetFuturePlayoutItemsById` przez most) porównany z naszym, pozycja po pozycji.
6. **ErsatzTV-ffmpeg** – Next nie działa ze zwykłym ffmpeg. Używamy oficjalnego obrazu `ersatztv/next` (amd64, arm64), nie budujemy własnego.

### 8.4 Architektura docelowa

```
Przeglądarka ── /api/* ──▶ VizRS Live (Rust, bez zależności)
                            ├─ magazyn (JSON + dziennik)     ├─ skaner (ffprobe, NFO, okładki przez ffmpeg)
                            ├─ planista (klasyczny, bloki)   ├─ EPG / M3U / HDHomeRun
                            ├─ generator Next ──pliki──▶ wolumen: lineup.json, channel/*.json,
                            │                                    playout/{kanał}/{start}_{finish}.json, xmltv/*.xml
                            ├─ /next/dynamic/{kanał} ◀── źródło `dynamic` Next (LIVE w chwili emisji)
                            └─ pośrednik /api/cg/… ──▶ VizRS CG Playout (grafika, sygnały studia)
ErsatzTV Next (oficjalny obraz) ── /channel/*.m3u8 ──▶ Caddy ──▶ widzowie
```

Nowe moduły w `server/src/`: `store.rs`, `scan.rs`, `sched/` (klasyczny, blokowy, wypełniacze), `next.rs` (pliki Next ze schematów), `epg.rs`, `hdhr.rs`. Do sprawdzenia w repo CG Playout: czy VizRS może podać warstwę `canvas` (wideo z alfą przez HTTP z nagłówkami `x-etv-offset-ms`), żeby zastąpić nakładkę `vizrs-ffmpeg`.

### 8.5 Etapy (propozycja N0–N7, do decyzji – po akceptacji wiersze trafią do §4)

| Punkt | Zakres | Bramka „idziemy dalej” |
|---|---|---|
| **N0** – próba (1–2 sesje) | `next.rs`: playout jednego kanału z danych mostu (`GetFuturePlayoutItemsById` → plik Next) w samodzielnym kontenerze Next na stosie testowym, obok legacy | Next gra nasz plik; przejścia o zadanej sekundzie; `in/out` działa; zmiana pliku w trakcie emisji wchodzi bez restartu |
| **N1** – kanały VizRS (C1 z §6 w nowej wersji) | drugi lineup Next obok legacy dla kanałów „ramówka dnia”: K19 natywnie (lista plików na datę z `in/out`, kopiuj dzień, zapętl), LIVE jako `http is_live` / `dynamic`, grafika w pozycji. Media czytane z legacy przez most. Własny magazyn tylko dla ramówek dnia. **Jedno M3U i jeden XMLTV** dla widza – panel albo Caddy scala oba lineupy (usuwa główne ryzyko C1) | tydzień na stosie `virtrs` bez dziury i bez fallbacku |
| **N2** – skaner lokalny + magazyn mediów | ffprobe, NFO, okładki; legacy skanuje równolegle, porównanie liczby i długości pozycji | rozbieżność < 1% pozycji, wyjaśniona |
| **N3** – planista klasyczny | harmonogramy, kolekcje (4 rodzaje), inteligentne kolekcje (język zapytań), wypełniacze, harmonogramy zastępcze, import starych ramówek przez most | testy wzorcowe zgodne z legacy na danych z `tests/stack.sh seed` i kopii produkcji |
| **N4** – bloki, szablony, deco, kalendarz | jak w legacy (R6) | testy wzorcowe jak w N3 |
| **N5** – przeniesienie wszystkich kanałów | EPG, M3U, HDHomeRun, logo, znaki wodne (`watermark` Next); legacy już tylko do eksportu | dwa tygodnie produkcji wyłącznie na planiście VizRS |
| **N6** – Plex / Jellyfin / Emby | tylko po decyzji o TLS (8.3); inaczej świadome porzucenie | jak N2 dla każdego źródła |
| **N7** – wyłączenie legacy | usunięcie `bridge/`, `commands.txt`, `legacy-bridge.Dockerfile`; zasady w CLAUDE.md i DOKUMENTACJA przepisane; wydanie 2.0 | – |

Każdą bramkę można zakończyć decyzją „zostajemy”. Po N1 stan jest trwały i użyteczny: legacy plus kanały VizRS w Next.

### 8.6 Co tracimy (świadomie)

Harmonogramy YAML i Scripted Schedule (C2), ExternalJson, Trakt, Plex/Jellyfin/Emby do czasu N6. Do tego darmowe poprawki legacy i jego społeczność. Kanały lustrzane, przesunięcia czasu, preferencje języka ścieżek i selektor strumieni trzeba przepisać (N3–N5), bo inaczej znikną.

### 8.7 Rekomendacja i decyzje do podjęcia

- **Nie zaczynać pełnego przepisania teraz.** Zrobić **N0** (tanie, rozstrzyga technikę) i – jeśli się uda – **N1**. Tak dostajemy ramówkę co do sekundy, `in/out`, LIVE i grafikę w Next bez utraty legacy. Po N1 decyzja §6 zmienia się z „A” na „A + kanały VizRS w Next”.
- **Warunki przejścia do N2 i dalej** (wystarczy jeden): Next 1.0 albo stabilny schemat playoutu przez ≥ 2 miesiące; legacy przestaje być rozwijane (sygnał z §6); N1 przez miesiąc bez awarii, a koszt utrzymania mostu rośnie.
- **Decyzje użytkownika potrzebne przed N0:** (1) zgoda na N0/N1 mimo decyzji A z 2026-11-09; (2) panel zapisuje pliki na **własny wolumen Next** (rozszerzenie R2 – nie dotyczy `/config` legacy ani CG); (3) przed N6: TLS a zasada zero zależności; (4) czy Plex/Jellyfin/Emby są w ogóle potrzebne na kanałach VizRS.
- Zasady „legacy bez zmian” i „żadnych zapisów do bazy legacy” zostają w mocy przez cały czas – przejmowanie odbywa się obok legacy, nie w nim.

## 9. Zdublowane funkcje w pakiecie VizRS – analiza i porządki (2026-11-11)

Przegląd VizRS Live wobec VizRS CG Playout i VizRS CG System. Kodu przeniesionego między repozytoriami nie ma (własne HTTP i JSON w każdym to zasada „zero zależności”), ale te same czynności dało się wykonać w kilku miejscach.

| # | Dublowanie | Decyzja / stan |
|---|---|---|
| D1 | Ręczne TAKE / CONTINUE / OUT na antenie kanału w Trio (CG System) i w Live (K13); dziennik emisji widział tylko komendy z Live | **Live zachowuje sterowanie grafiką** (decyzja użytkownika 2026-11-11 – role Trio i Live nie są rozdzielane). Dziennik pełny: CG Playout wykrywa zmiany warstw, których sam nie wysłał (Trio, `/mobile`, makra, karuzela, Companion) i zapisuje `take`/`out` z operatorem „VizRS (poza panelem)”. Przy okazji naprawiona flaga `manual` (K13). **Zrobione** (CG Playout `panel.rs`, `vizrs.rs`) |
| D2 | „Kiedy grafika wchodzi” w trzech miejscach: czas strony w Trio (⏱ kanał), reguły CG Playout (`on_clip`, `on_end`, `shows`, `channels`), elementy `page-<ID>.yml` w ramówce legacy | Jeden widok w Live: Grafika kanału → „Kiedy strona wchodzi na antenę” (trzy źródła przy każdej stronie, ostrzeżenie „może wejść dwa razy”). **Zrobione.** Otwarte: który mechanizm ma być główny (proponowany: element w ramówce + czas z Trio; reguły CG do pasków stałych i reklam) – do decyzji użytkownika |
| D3 | Zapisy do legacy dwiema drogami: CG Playout `etv_api.rs` (Smart Collections `LIVE · …`, skan biblioteki, reset playoutu, XMLTV) i Live przez most | Kolekcje `LIVE · …` w Live tylko do odczytu, z pigułką „CG Playout”, nazwa zarezerwowana. **Zrobione.** Otwarte: CG przestaje pisać do legacy, a kroki audycji LIVE (kolekcja, skan, przebudowa) wykonuje Live przez most – zmiana styku po obu stronach, osobny punkt |
| D4 | Wdrożenie opisane dwa razy: `compose.cg-playout.yml` (lokalnie) i `docker-compose.live.yml` CG (Portainer), kopia Caddyfile, `compose.vaapi.yml` | `scripts/check-cg-overlay.sh` (na początku `tests/deploy.sh`) wymusza zgodność; wyrównany rozjazd `TZ`. **Zrobione.** Otwarte: jedna nakładka (lokalnie `docker-compose.live.yml` CG + cienka nakładka Live tylko z budową mostu) – wymaga zmiany `COMPOSE_FILE` w `.env` lokalnego stosu i odtworzenia proxy, więc tylko na prośbę użytkownika |
| D5 | Ramówka czytana dwa razy: XMLTV w CG Playout i `GetFuturePlayoutItemsById` w Live | Bez zmian: małe ryzyko (EPG legacy odświeża się z opóźnieniem); do obserwacji |
| D6 | Alarmy: webhook CG System (`--webhook`), `alert_webhook` CG Playout, alarmy w przeglądarce Live; stan systemu w Trio i w Live | Bez zmian: uzupełniają się (różne zdarzenia) |
| D7 | „Show/audycja” w trzech znaczeniach (paczka grafiki VizRS, `shows` w regułach CG, Audycje LIVE) | Tylko pojęcia: w UI Live używać „show VizRS”, „reguła audycji”, „audycja LIVE” |
