# Plan rozbudowy VizRS Live (po prototypie 0.1.0)

Cel końcowy: **operator nie korzysta z web GUI ErsatzTV legacy** – wszystko, co daje web GUI legacy, robi się w VizRS Live, a web GUI legacy jest zamknięte dla operatorów (R10).

> **Stan:** wdrożony – R0–R11 w wydaniu 1.0.0, R12 w „Niewydane”. Dalsze prace: [../STATUS.md](../STATUS.md) i [../LUKI.md](../LUKI.md); ta sekcja „Jak pracować” jest wzorem dla kolejnych planów.

## Jak pracować z tym planem

- **Jeden punkt = jedna sesja.** Każdy punkt jest opisany tak, żeby dało się go zrobić w świeżej sesji bez historii rozmowy. Nie łącz punktów w jednej sesji; po skończeniu punktu zakończ sesję.
- **Kolejność:** od góry tabeli. Punkt można zacząć, gdy jego „Wymaga” jest zrobione.
- **Automat:** `scripts/plan.sh run` bierze następny punkt (pierwszy „do zrobienia” ze spełnionym „Wymaga”) i uruchamia dla niego nową sesję `claude -p` bez operatora; `-n N` – kilka punktów po kolei, `-p Rk` – wskazany punkt, `--dry-run` – tylko pokaż. Po sesji automat sam sprawdza: Stan = „zrobione”, nowy commit, czyste drzewo, `tests/gate.sh` zielony; inaczej się zatrzymuje. Sesja, która nie może dokończyć punktu, ustawia Stan = „przerwane: powód” – taki punkt czeka na człowieka. Dzienniki: `.plan-logs/`. Koszt i czas: `PLAN_BUDGET_USD`, `PLAN_TIMEOUT`.
- **Na początku sesji:**
  1. Przeczytaj `CLAUDE.md`, `docs/STATUS.md`, ten plik (tabelę i swój punkt), `docs/DOKUMENTACJA.md` (zwłaszcza §2 most, §4 ekrany, §5 ograniczenia), `git log -5`.
  2. Uruchom stos testowy: `tests/stack.sh up && tests/stack.sh seed` (porty 18409/18090, produkcyjnego ErsatzTV `virtrs-*` nie ruszaj).
  3. Źródło legacy do podglądu: `git clone --depth 1 https://github.com/ErsatzTV/legacy` do katalogu tymczasowego (poza repo). Wersja w obrazie: `GET /vizrs-bridge/health` → `legacy`.
- **Jak znaleźć komendy ekranu legacy:** w `ErsatzTV/Pages/<Ekran>.razor` (i `ErsatzTV/ViewModels/*EditViewModel.cs`) szukaj `Mediator.Send(` i `new <Komenda>(`; definicje w `ErsatzTV.Application/**/Commands|Queries/*.cs`. Parametry i wartości wyliczeń daje też działający most: `GET /api/commands` (konsola w Diagnostyce).
- **Zasada mostu:** nowa funkcja = wiersze w `bridge/commands.txt` + kod w Ruście/JS. Nowy kod C# tylko, gdy ogólny dyspozytor nie wystarcza (np. wgrywanie pliku – R5); wtedy mały, ogólny endpoint, opisany w DOKUMENTACJA §2.
- **Na końcu sesji (warunki „gotowe”):**
  - bramka zielona: `tests/gate.sh` (`cargo fmt/clippy/test/build`, `tests/stack.sh up/seed`, e2e);
  - nowe przypadki e2e dla punktu w `tests/e2e.cjs` (i w razie potrzeby dane w `tests/stack.sh seed`);
  - zrzut ekranu nowych widoków obejrzany (Playwright, pliki PNG poza repo);
  - `docs/DOKUMENTACJA.md` (§4 tabela ekranów, §5 ograniczenia legacy), `CHANGELOG.md` (Niewydane), kolumna **Stan** w tabeli niżej;
  - commit (jeden punkt = jeden commit albo kilka spójnych), push tylko na prośbę.
- **Wspólne zasady UI:** po polsku, styl VizRS, tekst do DOM tylko przez `textContent`, bez stylów/skryptów inline (CSP), potwierdzenie przed usunięciem, błąd legacy pokazany operatorowi (toast), wartości wyliczeń z `GET /api/commands` + etykiety PL w `PL`.

## Tabela

| Punkt | Zakres | Wymaga | Stan |
|---|---|---|---|
| R0 | Fundament UI: moduły, formularz z metadanych, wspólne edytory | – | zrobione (2026-11-06) |
| R1 | Harmonogramy (klasyczna ramówka): pełna edycja z pozycjami | R0 | zrobione (2026-11-06) |
| R2 | Playouty: zakładanie wszystkich rodzajów, edycja, usuwanie, harmonogramy zastępcze | R1 | zrobione (2026-11-06) |
| R3 | Media: biblioteki lokalne, skan, przeglądarka i wyszukiwarka, kosz | R0 | zrobione (2026-11-06) |
| R4 | Kolekcje (zwykłe, inteligentne, wielo-, powtórki) i playlisty | R3 | zrobione (2026-11-06) |
| R5 | Profile kodowania, znaki wodne (z wgrywaniem obrazu), wypełniacze, logo kanału | R0 | zrobione (2026-11-06) |
| R6 | Bloki, szablony, deco: edycja i kalendarz tygodnia | R4, R5 | zrobione (2026-11-07) |
| R7 | Ustawienia legacy, logi, konserwacja, test odtwarzania | R0 | zrobione (2026-11-07) |
| R8 | Źródła zewnętrzne: Jellyfin, Emby, Plex (PIN), strumienie zdalne | R3 | zrobione (2026-11-07) |
| R9 | Grafika VizRS i sloty na żywo (VizRS CG Playout) | R2 | zrobione (2026-11-07) |
| R10 | Zamknięcie web GUI legacy, proxy, aktualizacje legacy, CI | R1–R8 | zrobione (2026-11-07) |
| R11 | Wydanie 1.0: instrukcja operatora, obrazy, przegląd bezpieczeństwa | R10 | zrobione (2026-11-07) |
| R12 | VizRS Live Panel (CG Playout `/live`) wchłonięty: Sloty na żywo (MCR), Audycje LIVE, operator i studio w pasku górnym | R9 | zrobione (2026-11-07) |

---

## R0 – Fundament UI

**Cel:** przygotować `server/web/` na kilkanaście pełnych edytorów, zanim powstaną.

**Zakres:**
- Podział `app.js` (dziś jeden plik) na moduły ES: `web/core.js` (DOM `h()`, `api`, `cmd`, `argsFor`, toast, dialog, router, `PL`), `web/views/<obszar>.js` (po jednym na grupę menu). Każdy nowy plik dopisać do `STATIC` w `server/src/main.rs` (i do testu tras, że każdy plik z `STATIC` zwraca 200).
- **Formularz z metadanych:** funkcja, która z `GET /api/commands` (typ, opcjonalność, wartości wyliczeń) i opisu pól (etykieta PL, sekcja, rodzaj kontrolki, źródło listy wyboru) buduje formularz, zbiera wartości i wywołuje komendę. Typy: tekst, liczba, tak/nie, wyliczenie, `TimeSpan` (gg:mm:ss), data/godzina, wybór id z listy (np. kolekcja), lista id (wiele).
- **Wzorzec ekranu „lista + edytor”** (jak Kanały): lista z zaznaczeniem, edytor, „+ Nowy”, „Zapisz”, „Usuń” z potwierdzeniem, „Kopiuj” (gdy legacy ma `Copy…`). Przepisać na niego ekran Kanały bez zmiany zachowania.
- **Edytor listy pozycji** (wiersze przesuwane w górę/dół, dodaj/usuń, edycja wiersza w panelu bocznym) – użyją go R1, R4, R6.
- **Wybór treści:** komponent „kolekcja / inteligentna / wielokolekcja / playlista / serial / sezon / artysta / wyszukiwanie” z wyszukiwaniem (`SearchCollections`, `SearchSmartCollections`, `SearchMultiCollections`, `SearchTelevisionShows`, `SearchTelevisionSeasons`, `SearchArtists`, playlisty przez `GetAllPlaylistGroups` + `GetPlaylistsByPlaylistGroupId`) – dodać te zapytania do białej listy.
- Kontrola w UI: komendy potrzebne ekranowi, których nie ma w `GET /api/commands`, dają czytelny komunikat („ta funkcja wymaga komendy X na białej liście mostu”), a nie pusty ekran.

**Gotowe, gdy:** wszystkie dotychczasowe ekrany działają jak przed zmianą (e2e zielone), Kanały korzystają z nowego wzorca, jest test e2e dla komponentu wyboru treści.

## R1 – Harmonogramy (ramówka klasyczna)

**Cel:** ułożyć ramówkę klasyczną w panelu (odpowiednik Schedules + Schedule Items Editor).

**Komendy:** `GetPagedProgramSchedules`/`GetAllProgramSchedules`, `GetProgramScheduleById`, `CreateProgramSchedule`, `UpdateProgramSchedule`, `CopyProgramSchedule`, `DeleteProgramSchedule`, `GetProgramScheduleItems`, `ReplaceProgramScheduleItems` (nie `AddProgramScheduleItem` – błąd legacy, DOKUMENTACJA §5); listy wyboru: wypełniacze, znaki wodne, elementy graficzne, kody języków.

**UI:** lista harmonogramów; edytor ustawień (tasowanie, losowy start, części odcinków razem, kolekcje jak seriale, zachowanie godzin stałych); **edytor pozycji** (R0): start dynamiczny/stały z godziną, tryb (One / Multiple + liczba lub tryb / Duration + czas + ogon / Flood), treść (komponent R0), kolejność odtwarzania, maraton, wypełniacze pre/mid/post/tail/fallback, znaki wodne i grafika, tytuł własny, tryb przewodnika, preferencje audio/napisów. Zapis całej listy jednym `ReplaceProgramScheduleItems` (most zwraca wtedy `warning` – traktować jako sukces i przeładować pozycje).

**Testy e2e:** nowy harmonogram z dwiema pozycjami (stała 06:00 Flood + dynamiczna One), zmiana kolejności, zapis, ponowne otwarcie = te same wartości; usunięcie.

**Do sprawdzenia:** jak legacy waliduje sprzeczne pozycje (komunikaty po angielsku – zostawić tekst legacy, dodać polski wstęp).

## R2 – Playouty

**Cel:** zakładać i prowadzić playouty bez legacy (odpowiednik Playouts + edytory playoutów).

**Komendy:** `CreateClassicPlayout`, `CreateBlockPlayout`, `CreateSequentialPlayout`, `CreateScriptedPlayout`, `CreateExternalJsonPlayout`, `UpdatePlayout`, `UpdateSequentialPlayout`, `UpdateScriptedPlayout`, `UpdateExternalJsonPlayout`, `DeletePlayout`, `GetPlayoutById`, `BuildPlayout` (Continue/Refresh/Reset), `ResetAllPlayouts`, `ErasePlayoutItems`, `ErasePlayoutHistory`, `GetPlayoutAlternateSchedules`, `ReplacePlayoutAlternateSchedule`, `ReplacePlayoutAlternateScheduleItems`, `GetPlayoutWarningsCount`, `CheckForOverlappingPlayoutItems`, `GetBlockPlayoutHistory` (podgląd).

**UI:** „+ Playout” z wyborem rodzaju i kanału (tylko kanały bez playoutu); dla sekwencyjnego/skryptowego/JSON – ścieżka pliku w kontenerze legacy (i podpowiedź, gdzie go położyć); edycja (np. dzienna godzina przebudowy); **harmonogramy zastępcze** (dni tygodnia/miesiąca → harmonogram) z kolejnością; ostrzeżenia playoutów jako licznik w pasku górnym; „Resetuj wszystkie” z potwierdzeniem.

**Testy e2e:** założenie playoutu klasycznego dla nowego kanału z R1, przebudowa, pozycje widoczne w „Na antenie”; harmonogram zastępczy na sobotę; usunięcie playoutu.

**Do sprawdzenia:** playouty sekwencyjne (YAML) – czy panel ma też edytować sam plik YAML (wymagałoby zapisu pliku w kontenerze legacy: osobny endpoint mostu albo wspólny wolumen) – decyzja w tej sesji, zapisać w DOKUMENTACJA.

**Decyzja (2026-11-06):** panel nie edytuje plików YAML/skryptów/JSON; pliki na wolumenie konfiguracji legacy (`/config/schedules/`, `/config/scripts/`), w panelu ścieżka w kontenerze. Zapis plików – dopiero gdy R9 będzie go potrzebować, jednym ogólnym endpointem mostu ograniczonym do jednego katalogu (DOKUMENTACJA §4).

## R3 – Media

**Cel:** biblioteki i przeglądanie treści (Media Sources: Local, Libraries, ekrany mediów, Search, Trash).

**Komendy:** `GetAllLocalLibraries`, `GetLocalLibraryById`, `GetLocalLibraryPaths`, `CreateLocalLibrary`, `UpdateLocalLibrary`, `UpdateLocalLibraryPath`, `MoveLocalLibraryPath`, `DeleteLocalLibrary`, `CountMediaItemsByLibrary`, `CountMediaItemsByLibraryPath`, `QueueLibraryScanByLibraryId`, `QueueShowScanByLibraryId`, `QuerySearchIndexAllItems` i `QuerySearchIndex{Movies,Shows,Seasons,Episodes,OtherVideos,Songs,MusicVideos,Artists,Images,RemoteStreams}`, `QuerySearchTargets`, `GetMediaItemInfo`, `GetMovieById`, `GetTelevisionShowById`, `GetTelevisionSeasonById`, `GetRemoteStreamById`, `GetImageFolders`, `UpdateImageFolderDuration`, `RemoveMediaItems`, `EmptyTrash`, `DeleteItemsFromDatabase`, `GetLibraryRefreshInterval`.

**UI:** biblioteki lokalne z rodzajem i ścieżkami (dodaj/usuń/przenieś ścieżkę), liczba pozycji, „Skanuj”; przeglądarka z zakładkami rodzajów i wyszukiwarką (składnia legacy, podpowiedź pól), stronicowanie, karta pozycji (`GetMediaItemInfo`: strumienie, rozdziały, czas), dodawanie zaznaczonych do kolekcji/playlisty (po R4 – na razie przycisk ukryty); kosz.

**Testy e2e:** biblioteka z `tests/stack.sh seed` widoczna z liczbą 3, wyszukiwanie „Pogoda” zwraca 1 pozycję, karta pozycji pokazuje H.264.

**Do sprawdzenia:** ścieżki to ścieżki **w kontenerze legacy** – panel powinien to jasno mówić; postęp skanu (czy legacy ma zapytanie o stan skanu – jeśli nie, odświeżanie liczby pozycji).

## R4 – Kolekcje i playlisty

**Komendy:** `GetPagedCollections`/`GetAllCollections`, `GetCollectionById`, `GetCollectionItems` (sprawdzić nazwę), `CreateCollection`, `UpdateCollection`, `DeleteCollection`, `AddItemsToCollection`, `RemoveItemsFromCollection`, `Add{Movie,Show,Season,Episode,Artist,MusicVideo,OtherVideo,Song,Image,MediaItem}ToCollection`; `GetPagedSmartCollections`, `CreateSmartCollection`, `UpdateSmartCollection`, `DeleteSmartCollection`; `GetPagedMultiCollections`, `GetMultiCollectionById`, `CreateMultiCollection`, `UpdateMultiCollection`, `DeleteMultiCollection`; `GetPagedRerunCollections`, `CreateRerunCollection`, `UpdateRerunCollection`, `DeleteRerunCollection`; playlisty: `GetAllPlaylistGroups`, `GetPlaylistsByPlaylistGroupId`, `GetPlaylistById`, `GetPlaylistItems`, `CreatePlaylistGroup`, `DeletePlaylistGroup`, `CreatePlaylist`, `DeletePlaylist`, `ReplacePlaylistItems`, `AddItemsToPlaylist`, `PreviewPlaylistPlayout`; Trakt (`AddTraktList`, `DeleteTraktList`) – opcjonalnie.

**UI:** zakładki rodzajów kolekcji; kolekcja zwykła z listą pozycji i usuwaniem; inteligentna z edycją zapytania i podglądem liczby wyników (R3 wyszukiwarka); wielokolekcja ze składnikami i kolejnością; powtórki; playlisty w grupach z edytorem pozycji (R0) i podglądem playoutu; w przeglądarce R3 włączyć „Dodaj do kolekcji/playlisty”.

**Testy e2e:** kolekcja zwykła z 2 pozycjami z przeglądarki, inteligentna z zapytaniem, playlista z 3 pozycjami i podglądem.

## R5 – Profile kodowania, znaki wodne, wypełniacze, logo

**Komendy:** `GetAllFFmpegProfiles`, `GetFFmpegProfileById`, `NewFFmpegProfile` (wartości domyślne), `CreateFFmpegProfile`, `UpdateFFmpegProfile`, `CopyFFmpegProfile`, `DeleteFFmpegProfile`, `GetAllResolutions`, `GetSupportedHardwareAccelerationKinds`, `GetNextDescribe`/`RefreshNextProbe` (co umie sprzęt); `GetWatermarkById`, `CreateWatermark`, `UpdateWatermark`, `CopyWatermark`, `DeleteWatermark`; `GetPagedFillerPresets`, `GetFillerPresetById`, `CreateFillerPreset`, `UpdateFillerPreset`, `DeleteFillerPreset`; `SaveArtworkToDisk`.

**UI:** profil (obraz, dźwięk, akceleracja z informacją, co sprzęt naprawdę umie, normalizacja głośności – podpowiedź −23 LUFS dla TV w Polsce); znak wodny (obraz, 9 położeń, rozmiar, krycie, tryb stały/przerywany) z podglądem na makiecie kadru; wypełniacz (rodzaj, tryb, treść przez komponent R0); w edytorze kanału – wgrywanie logo.

**Nowy kod C# (wyjątek):** `SaveArtworkToDisk` przyjmuje `Stream`, więc ogólny dyspozytor go nie obsłuży. Dodać ogólny endpoint mostu `POST /vizrs-bridge/upload?kind=<ArtworkKind>` (treść = bajty pliku, `Content-Type`, limit np. 10 MB, tylko typy obrazów) → `SaveArtworkToDisk` → zwraca ścieżkę; w panelu `POST /api/upload` z tym samym limitem. Opisać w DOKUMENTACJA §2 i przetestować (zły typ, za duży plik).

**Testy e2e:** nowy profil 720p, kopia, usunięcie; znak wodny z wgranym PNG przypisany do kanału; wypełniacz „dopełnij do 30 min”.

## R6 – Bloki, szablony, deco

**Komendy:** `GetAllBlockGroups`, `CreateBlockGroup`, `DeleteBlockGroup`, `GetBlocksByBlockGroupId`, `GetBlockById`, `CreateBlock`, `CopyBlock`, `DeleteBlock`, `GetBlockItems`, `ReplaceBlockItems`, `PreviewBlockPlayout`; `GetAllTemplateGroups`, `CreateTemplateGroup`, `DeleteTemplateGroup`, `GetTemplatesByTemplateGroupId`, `GetTemplateById`, `CreateTemplate`, `CopyTemplate`, `DeleteTemplate`, `GetTemplateItems`, `ReplaceTemplateItems`, `GetPlayoutTemplates`, `ReplacePlayoutTemplateItems`; `GetAllDecoGroups`, `CreateDecoGroup`, `DeleteDecoGroup`, `GetDecosByDecoGroupId`, `GetDecoById`, `CreateDeco`, `UpdateDeco` (+ `UpdateDecoBreakContent` i pokrewne – sprawdzić w `DecoEditor.razor`), `DeleteDeco`, `GetDecoTemplateTree`, `CreateDecoTemplate*`, `GetDecoTemplateItems`, `ReplaceDecoTemplateItems`, `GetDecoByPlayoutId`.

**Do sprawdzenia na starcie:** zmiana nazwy/długości bloku i szablonu – w legacy nie ma `UpdateBlock`/`UpdateTemplate`; ustalić w `BlockEditor.razor`/`TemplateEditor.razor`, którą komendą to idzie (prawdopodobnie `ReplaceBlockItems` niesie też nazwę i długość). Jeśli zapis omija MediatR – odnotować w §5 i nie obchodzić.

**UI:** grupy i bloki (pozycje bloku edytorem R0, podgląd bloku); szablon dnia jako oś 24 h z blokami; **kalendarz tygodnia** playoutu blokowego (jak w makiecie): szablony przypisane do dni, deco na bloku; deco: znak wodny, grafika, przerwy, wypełniacz, martwe powietrze z trybami dziedzicz/scal/zastąp/wyłącz.

**Testy e2e:** blok 30 min z 3 pozycjami, szablon dnia z dwoma blokami, playout blokowy na tydzień, deco z przerwą reklamową; pozycje widoczne w EPG.

## R7 – Ustawienia, logi, konserwacja, test odtwarzania

**Komendy:** `GetFFmpegSettings`/`UpdateFFmpegSettings`, `GetLoggingSettings`/`UpdateLoggingSettings`, `GetPlayoutSettings`/`UpdatePlayoutSettings`, `GetXmltvSettings`/`UpdateXmltvSettings`, `GetUiSettings`/`UpdateUiSettings`, `GetLibraryRefreshInterval`/`UpdateLibraryRefreshInterval`, `GetHDHRTunerCount`/`UpdateHDHRTunerCount`, `GetHDHRUUID`, `GetRecentLogEntries`, `GetAllHealthCheckResults` (z `Refresh`), `EmptyTrash`, `DeleteOrphanedArtwork`, `DeleteOrphanedSubtitles`, `ReleaseMemory`, `RebuildSearchIndex`, `GetTroubleshootingInfo`, `PrepareTroubleshootingPlayback`/`StartTroubleshootingPlayback` (sprawdzić, czy da się bez Blazora), `GetNextVersion`, `GetNextDescribe`.

**UI:** Ustawienia w sekcjach jak w legacy (FFmpeg, playout, XMLTV, skaner, HDHomeRun, logi); Diagnostyka: logi z filtrem poziomu i odświeżaniem, konserwacja z potwierdzeniami, informacje o sprzęcie i wersjach, test odtwarzania (jeśli wykonalny – podgląd HLS wymagałby odtwarzacza; bez bibliotek z CDN: zostawić odnośnik do strumienia).

**Testy e2e:** zmiana dni XMLTV i przywrócenie; logi zawierają wpis po akcji z panelu.

## R8 – Źródła zewnętrzne

**Komendy:** Jellyfin: `GetAllJellyfinMediaSources`, `GetJellyfinSecrets`/`SaveJellyfinSecrets`, `SynchronizeJellyfinMediaSources`, `GetJellyfinLibrariesBySourceId`, `UpdateJellyfinLibraryPreferences`, `ForceSynchronizeJellyfinLibraryById`, `GetJellyfinPathReplacementsBySourceId`/`UpdateJellyfinPathReplacements`, `DisconnectJellyfin`; Emby – odpowiedniki `…Emby…`; Plex: `StartPlexPinFlow`/`TryCompletePlexPinFlow` (kod PIN do wpisania na plex.tv/link), `GetAllPlexMediaSources`, `SynchronizePlexMediaSources` (sprawdzić nazwę), `GetPlexLibrariesBySourceId`, `UpdatePlexLibraryPreferences`, `ForceSynchronizePlexLibraryById`, `…PathReplacements…`, `SignOutOfPlex`, `DeletePlexMediaSource`.

**Bezpieczeństwo:** sekrety (klucze API) tylko zapis, nigdy nie wracają do przeglądarki w całości (maskowanie w panelu, a jeśli most zwraca je w wyniku – odfiltrować po stronie serwera Rust).

**Testy:** bez prawdziwego serwera mediów – test formularzy i błędów połączenia; z Jellyfin w kontenerze testowym, jeśli łatwo dostępny (opcjonalnie).

**Zrobione (2026-11-07) – decyzje:** klucz API tylko do zapisu (serwer panelu maskuje pola `…apikey/token/password/secret` w każdej odpowiedzi; zmiana połączenia = ponowne wpisanie klucza); `Get…ConnectionParameters` i `TryCompletePlexPinFlow` poza białą listą; Plex PIN – panel otwiera adres z `StartPlexPinFlow`, legacy sam kończy logowanie; głęboki skan pominięty; strumienie zdalne = biblioteki lokalne z plikami .yml (R3), plików panel nie zapisuje (decyzja R2). Testy: atrapa Jellyfin w `tests/e2e.cjs` zamiast kontenera Jellyfin; prawdziwe serwery – krok ręczny (DOKUMENTACJA §4).

## R9 – Grafika VizRS i sloty na żywo

**Cel:** to, czego legacy nie ma – grafika VizRS na kanale i audycje na żywo, z VizRS CG Playout (`../VirtRS CG Playout`, DOKUMENTACJA tamtego repo).

**Zakres:** przeczytać API CG Playout (konfiguracja kanałów, element VizRS, `/ingest`, `/ingest/state`); w panelu: **Grafika kanału** – reguły (start pozycji ± s, przed końcem audycji, przerwa reklamowa, slot na żywo → strona VizRS z polami `{title}`, `{next_title}`…) zapisane tam, gdzie CG Playout je trzyma; podgląd `/output` VizRS; **Sloty na żywo** – wejścia, stan sygnału, audycje na żywo jako pozycje ramówki (Remote Stream w legacy). Elementy graficzne legacy: `GetAllGraphicsElements`, `RefreshGraphicsElements` (pliki YAML – R2 zdecydował: panel nie zapisuje plików; jeśli R9 tego potrzebuje, dodaje jeden ogólny endpoint mostu do zapisu w jednym katalogu, wspólny dla playoutów i grafiki – DOKUMENTACJA §4).

**Decyzja (2026-11-06): panel łączy się z VizRS CG Playout przez jego API** – bez wspólnych plików konfiguracyjnych. Wzorem mostu: serwer panelu (Rust) jest pośrednikiem, adres i token CG Playout tylko po stronie serwera (nowe opcje `--cg-url`/`VIZRS_LIVE_CG_URL`, `--cg-token`/`VIZRS_LIVE_CG_TOKEN`), przeglądarka woła `/api/cg/...`. Na starcie sesji: spisać z dokumentacji CG Playout endpointy potrzebne panelowi (konfiguracja kanałów i reguł, `/ingest`, `/ingest/state`, stan połączenia z VizRS) i sprawdzić, czego API jeszcze nie ma – braki dopisać w CG Playout (osobne repo, osobny commit tam) albo jako punkt jego planu, nie obchodzić przez pliki.

**Zrobione (2026-11-07) – decyzje:** pośrednik `/api/cg/…` w serwerze (biała lista, token CG tylko w serwerze); Sloty na żywo na API Live Panelu CG (`/api/live/*`); Grafika kanału: stan CG, strony VizRS, podgląd `/output`, elementy legacy + `RefreshGraphicsElements`. **Reguły grafiki bez edycji** – CG Playout nie ma API reguł (są w jego `config.json`); dopisanie `GET/PUT` reguł w CG Playout to krok ręczny / punkt planu CG Playout (sesja automatu nie zmienia innych repo), potem edytor w panelu. **Uzupełnione (2026-11-07, po R11):** CG Playout ma `GET/PUT /api/live/graphics`, Grafika kanału ma edytor reguł (DOKUMENTACJA §4). Endpoint zapisu plików w moście niepotrzebny (element `vizrs.yml` zakłada CG). Pozycję harmonogramu audycji LIVE operator dodaje w Harmonogramach. Testy na atrapie CG (DOKUMENTACJA §4).

## R10 – Zamknięcie web GUI legacy, aktualizacje, CI

**Zakres:**
- **Proxy przed legacy** (Caddy albo nginx w `compose.yml`): na zewnątrz tylko `/iptv/*` (strumienie, M3U, XMLTV, logo) i ewentualnie HDHomeRun (`/discover.json`, `/lineup*.json`, `/device.xml`); web GUI legacy i `/vizrs-bridge/*` tylko w sieci wewnętrznej Dockera. Test: z zewnątrz `/` i `/channels` legacy = 403/404, `/iptv/channels.m3u` = 200.
- **Lista kontrolna pokrycia:** każdy ekran z menu legacy (`ErsatzTV/Shared/MainLayout.razor`) → odpowiednik w panelu albo świadome pominięcie z uzasadnieniem (DOKUMENTACJA). Pierwsze uruchomienie legacy bez GUI (czy legacy wymaga jakiejś konfiguracji startowej).
- **Aktualizacja legacy:** przejść procedurę DOKUMENTACJA §6 na nowszym wydaniu niż 26.11.6; poprawić to, co się zmieni.
- **CI (GitHub Actions):** `cargo fmt/clippy/test`, budowa obu obrazów, `tests/stack.sh up && seed`, e2e z Playwright; zadanie cykliczne: najnowszy `ghcr.io/ersatztv/legacy:latest` + smoke (wczesne ostrzeżenie o zmianach komend).

**Zrobione (2026-11-07) – decyzje:** proxy = Caddy (`docker/proxy.Caddyfile`, usługa `proxy`), na zewnątrz GET/HEAD `/iptv/*`, `/artwork/*` (obrazy, potrzebne podglądowi w panelu) i HDHomeRun, reszta 403; legacy tylko `expose`; bez TLS (decyzja wdrożenia – R11). Stos testowy ma proxy na 18410, panel podaje widzom jego adres. Diagnostyka: sprawdzenie adresu dla widzów (`/api/exposure`) i lista pokrycia (Trakt i strony projektu – pominięte z uzasadnieniem, Troubleshooting – częściowo). Legacy nie wymaga konfiguracji startowej. Aktualizacja do v26.12.0 przeszła bez zmian (DOKUMENTACJA §6); domyślny `LEGACY_IMAGE` zostaje `latest`, przypięcie – R11. CI w `.github/workflows/` (nie uruchomione – wymaga push, krok ręczny, DOKUMENTACJA §8).

## R11 – Wydanie 1.0

**Zakres:** `docs/INSTRUKCJA.md` dla operatora (codzienna praca: kanał, ramówka, przebudowa, awarie), `docs/RUNBOOK.md` (wdrożenie, kopie `etv-config`, aktualizacja legacy, odtworzenie), obrazy na `ghcr.io/sdorpl/…`, wersje przypięte w `.env.example`, przegląd bezpieczeństwa (tokeny, CSRF, CSP, limity, sekrety R8, proxy R10), CHANGELOG 1.0.0.

**Zrobione (2026-11-07) – decyzje:** obrazy `ghcr.io/sdorpl/vizrs-live:1.0.0` i `vizrs-live-legacy:1.0.0-legacy26.12.0` (tag niesie wersję legacy, bez `latest`), publikacja przez `release.yml` po tagu – nie wykonana (wymaga push, krok ręczny, DOKUMENTACJA §9). Legacy przypięte do v26.12.0, Caddy do 2.10.2. TLS dalej poza panelem (RUNBOOK: LAN/VPN albo proxy TLS; `Secure` przy `X-Forwarded-Proto: https`). Przegląd bezpieczeństwa: dodany limit logowań i pełne CSP na każdej odpowiedzi; ryzyka przyjęte opisane w DOKUMENTACJA §9.
