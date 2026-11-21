# Zmiany

Wydania VizRS CG Playout w numeracji rok.miesiąc.rewizja od wersji 26.11.1, publikowane jako obraz kontenera `ghcr.io/sdorpl/vizrs-cg`. Wcześniejsze wydania (2.x) znajdują się w historii gita oraz w tagach `v2.*`.

## Niewydane

## 26.11.26 (2026-11-21)

- **Audycja LIVE z harmonogramu ma swoją grafikę i znak wodny, studio poza ramówką – swoje.** Do tej pory przebieg live audycji ze slotu liczył się w sidecarze jako przejęcie: grafika audycji (element VizRS pozycji) była wstrzymywana, a wchodziło `on_live` kanału (S1, „BTB Info” 03:30: `TAKE 7510` zamiast logo 7507). Teraz przebieg slotu czyta studio z `&programme=1` i nie jest przejęciem – działają element i reguły pozycji audycji, a jej znaki wodne ErsatzTV (`WatermarkIds`, np. z okna Audycje LIVE) nakładka kładzie w przebiegach live slotu, ze studiem i na planszy bez studia (z elementem VizRS i bez niego). Przejęcie poza ramówką zostaje przy grafice studia (`on_live` kanału albo wejścia).
- **Logo NA ŻYWO wejścia studia także na pozycji bez elementu VizRS.** Przy przejęciu nakładka miała klatki grafiki VizRS tylko z sesji elementu „script” pozycji: na pozycji bez elementu (S1: pozycje „wypełnij” kanału 1) `on_live` (`TAKE 7510`) szło do VizRS, a na obraz nie – także po WPUŚĆ. Teraz nakładka zakłada na przebiegi live takiego kawałka własną sesję grafiki kanału w sidecarze (`POST /etv?…&live=1`, przejmowana tylko przez `/ffmpeg?…&live=1`, bez reguł ramówki), w tle – przebieg live na nią nie czeka.
- **Znak wodny ErsatzTV na przejęcie przez live ingest.** Obok grafiki VizRS studia (`on_live`) reguła kanału i wejście studia mogą mieć znak wodny ErsatzTV na czas przejęcia (`live_watermark`, wybór w Grafice kanału i w „Ustaw wejścia…”; wejście bez własnego = znak kanału). Nakładka pobiera go z mostu (nowe `GET /vizrs-bridge/watermarks/<kanał>?ids=…`) i kładzie na wierzchu obrazu studia, jak ErsatzTV; znaki pozycji ramówki na ten czas schodzą. Nowa zmienna nakładki `VIZRS_BRIDGE_URL` (domyślnie `http://127.0.0.1:8409`).
- Sidecar, nakładka i most (obraz `vizrs-live-legacy`) muszą być z tego wydania (nakładka instaluje się z obrazu `vizrs-cg`). DOKUMENTACJA §1 „Grafika podczas przejęcia”, §10, §12, API.md.

## 26.11.25 (2026-11-21)

- **Ciasteczka YouTube: podtrzymanie sesji.** Na S1 sesja konta znów padła (między 09:06 a 13:22 UTC), a wyszło to dopiero przy ponownym wyborze wejścia YouTube: wejścia pytają YouTube tylko przy starcie i co ok. 6 h, a przeglądarka odświeża sesję co kilka minut. Usługa `ingest` co 30 min (`VIZRS_YTDLP_KEEPALIVE_MIN`, 0 = wyłączone), także bez wybranego wejścia YouTube, odświeża ciasteczka z panelu przez yt-dlp (jedna pozycja strony widocznej tylko po zalogowaniu, bez pobierania) i zapisuje je z powrotem. Zakończona sesja jest widoczna od razu: znacznik `ended`, czerwony stan w oknie ciasteczek i **alarm w banerze** panelu. Udane podtrzymanie: „Sesja sprawdzona i podtrzymana …” (`checked_at` w `/api/live/youtube-cookies`). yt-dlp z tymi ciasteczkami działa pojedynczo, a ostrzeżenie „cookies are no longer valid” przy pytaniu wejścia też oznacza koniec sesji. Ciasteczek, które YouTube już unieważnił, to nie naprawi – trzeba wgrać nowe. DOKUMENTACJA §1, §11, API.md, INSTRUKCJA.

## 26.11.24 (2026-11-21)

- **Harmonogramy na telefonie: pozycje nie są ucinane.** Tabela pozycji (Start, Treść, Tryb, Kolejność, Wypełniacze, Znaki i grafika) miała ok. 820 px i na wąskim ekranie kolumny od „Kolejność” wypadały poza ekran. Poniżej 760 px każda pozycja jest kartą „etykieta: wartość” z przyciskami (kopiuj, w górę, w dół, usuń) na dole – dotyczy wspólnej listy pozycji, więc także bloków, playlist, deco, kalendarza i harmonogramów zastępczych w Playoutach. DOKUMENTACJA §11.

## 26.11.23 (2026-11-21)

- **Audycje LIVE: grafika przy emisji.** W oknie audycji każda emisja ma obok znaku wodnego wybór elementu graficznego (`GraphicsElementIds` pozycji harmonogramu), jak w Harmonogramach – strony VizRS CG System na początku listy z numerem strony. Gdy pozycja ma kilka elementów z Harmonogramów, a wybór w oknie się nie zmienił, wszystkie zostają (tak samo znaki wodne). DOKUMENTACJA §12, INSTRUKCJA.

## 26.11.22 (2026-11-21)

- **Audycje LIVE: biblioteka bez folderu `/config/vizrs-live` nie psuje już ramówki.** Na S1 biblioteka „Remote Streams” miała tylko `/media/tv/Live`; plik audycji leżał poza nią, kolekcja `LIVE · …` była pusta, a emisja w harmonogramie zatrzymała budowę playoutu kanału („SmartCollection … has no valid items”). Panel pokazuje teraz czerwone ostrzeżenie z przyciskiem „Dodaj folder /config/vizrs-live” (dotychczasowe ścieżki biblioteki zostają, potem skan), a kreator nie wstawia emisji, dopóki folderu nie ma. DOKUMENTACJA §12.

## 26.11.21 (2026-11-21)

- **Zakończona sesja YouTube widoczna w panelu:** gdy YouTube kończy sesję konta, kasuje ciasteczka logowania; odbiornik zapisywał wtedy okrojony plik w miejsce wgranego, a panel dalej pokazywał „Konto zalogowane” (S1 2026-11-20: live z YouTube stanął po 5 h). Teraz plik z panelu zostaje, panel pokazuje na czerwono „YouTube zakończył sesję … – wgraj nowe ciasteczka” (`ended_at` w `/api/live/youtube-cookies`), a „zalogowane” liczy się jak w yt-dlp (`LOGIN_INFO` + `SAPISID`/`__Secure-*PAPISID`). DOKUMENTACJA §1, API.md.

## 26.11.20 (2026-11-21)

- **Płynny live z YouTube 60 kl./s na kanale 25 kl./s:** stacje 50 Hz (np. TVP Info) trafiają na YouTube jako 60 kl./s z powtórzoną co szóstą klatką; dotąd przejście na 25 albo 30 kl./s dawało nierówny ruch. Sidecar mierzy klatkaż sygnału (`fps` w `/ingest/state`), a przebieg live przy ~60 kl./s na kanale 25/50 usuwa powtórzoną klatkę (`decimate=cycle=6`) przed `fps` – ruch równy. Starszy sidecar: jak dotąd. DOKUMENTACJA §1.
- **Klatkaż z panelu na Ampere:** `docker-compose.ampere.yml` wymuszał 25 kl./s także przy pustym `VIZRS_FFMPEG_FPS` – klatkaż ustawiony w profilu kodowania (panel) nie działał (S1: profil 30 kl./s, a kanał dalej 25). Teraz pusta wartość w stosie = klatkaż z profilu kanału; brak zmiennej = 25 jak dotąd. DOKUMENTACJA §5.

## 26.11.19 (2026-11-20)

- **Ciasteczka YouTube nie wygasają po restarcie:** YouTube wymienia ciasteczka sesji, a odbiornik przy każdym wyborze wejścia i restarcie brał kopię pierwotnego pliku z panelu – YouTube widział stare ciasteczka i kończył sesję („cookies are no longer valid”, potem znów „Sign in to confirm you're not a bot”; S1 2026-11-20). Teraz ciasteczka odświeżone przez yt-dlp wracają do pliku z panelu (gdy w międzyczasie nikt nie wgrał nowych), log przy unieważnieniu mówi wprost, że trzeba wgrać nowe, a okno panelu radzi osobny eksport na każdy serwer. Plik zapisany przez yt-dlp (ciasteczka sesyjne bez daty) panel czyta poprawnie. DOKUMENTACJA §1, Live ingest.

## 26.11.18 (2026-11-20)

- **Wejście YouTube z ciasteczkami konta:** po wgraniu ciasteczek na S1 YouTube już nie odmawiał, ale yt-dlp kończył się „No video formats found!” – klienci YouTube dla zalogowanego konta wymagają rozwiązania zadania JavaScript („n challenge”), a w kontenerze nie było środowiska JS. Obraz `vizrs-cg` zawiera teraz Deno 2.9.8 (przypięte sumami SHA-256), instalowane przez sidecara na wolumen `vizrs-client` obok yt-dlp; odbiornik podaje je yt-dlp (`--js-runtimes`, `VIZRS_DENO`). Sidecar porównuje pliki klienta kawałkami (Deno ma ok. 96 MB). DOKUMENTACJA §1, Live ingest; THIRD-PARTY.

## 26.11.17 (2026-11-20)

- **Poprawka: znaki wodne ErsatzTV razem z grafiką VizRS.** Gdy na antenie była pozycja z elementem grafiki VizRS, znak wodny kanału (albo deco, pozycji, globalny) znikał – ErsatzTV Next rysuje go w tym samym płótnie grafiki, które nakładka `vizrs-ffmpeg` zastępuje klatkami sidecara. Teraz nakładka pyta most VizRS Bridge o znaki wodne pozycji (nowy `GET /vizrs-bridge/watermarks/{kanał}/{IdPozycji}` – wybór i położenie liczone przez legacy) i nakłada je sama w ffmpeg, zawsze na wierzchu – nad grafiką VizRS: skala, położenie, krycie i tryb przerywany jak w ErsatzTV. Wymaga ErsatzTV z mostem tego wydania (`vizrs-live-legacy`) i `VIZRS_BRIDGE_TOKEN` w kontenerze ErsatzTV (nakładka `docker-compose.live.yml` już go ustawia); starszy most albo brak tokenu = jak dotąd, bez znaków. DOKUMENTACJA §1 (Znaki wodne ErsatzTV), §10; API.md §3.
- **Logi ErsatzTV legacy w panelu znów widać:** ekran Logi (źródło ErsatzTV legacy) pokazywał zero wpisów na S1 i S2. Legacy czyta „najnowszy” plik swojego folderu `logs/` wg nazwy, a nakładka zapisywała tam ślad przejęć `vizrs-takeover-<kanał>.log` – alfabetycznie po `ersatztv<data>.log`, bez wierszy w formacie legacy. Ślad przejęć leży teraz w `vizrs-live/logs/` (obok `vizrs-cg.log` i `vizrs-ingest.log`); nakładka przy pierwszym śladzie sama przenosi tam stare pliki, Dziennik emisji czyta oba miejsca. DOKUMENTACJA §1 i §6.
- **Ciasteczka YouTube z panelu:** YouTube odrzuca pobieranie z adresów serwerowni („Sign in to confirm you're not a bot” – tak na S1 i S2 w Oracle Cloud), więc wejście YouTube czekało bez końca. Teraz Sloty na żywo → **Ciasteczka YouTube…** przyjmuje plik cookies zalogowanego konta (Netscape `cookies.txt` albo JSON z rozszerzenia przeglądarki); CG Playout sprawdza go i trzyma w `<etv_config>/vizrs-live/youtube-cookies.txt` (0600), panel widzi tylko liczbę, zalogowanie i ważność. Usługa `ingest` daje yt-dlp jego prywatną kopię i po wgraniu pyta YouTube od razu; log przy odmowie podpowiada, co zrobić. Live API: `GET/PUT/DELETE /api/live/youtube-cookies`, cecha `youtube_cookies`; `VIZRS_YTDLP_COOKIES` nadal wygrywa. Starszy CG Playout – bez przycisku. DOKUMENTACJA §1, Live ingest i §12.

## 26.11.16 (2026-11-20)

- **Live ingest po aktualizacji:** usługa `ingest` sama uruchamia się ponownie, gdy sidecar nowego wydania podmieni program w wolumenie `vizrs-client`. Dotąd działała starą wersją do ręcznego restartu (Portainer odtwarza tylko kontenery ze zmienionym obrazem) – po wydaniu 26.11.15 na S1 i S2 linki HLS były odrzucane („only rtmp:// and srt:// listeners”). DOKUMENTACJA §4.
- **Live ingest: wyższe progi osi czasu** – przebieg live rusza przy osi czasu do 75 s za zegarem (było 45 s), kończy się powyżej 90 s (było 60 s). Kanał 1 na S1 po restarcie ErsatzTV został 64 s za zegarem i z `VIZRS_FFMPEG_SYNC_RAM` już tego nie nadrabiał – sygnał był, a na antenie cały czas ramówka. DOKUMENTACJA §1.

## 26.11.15 (2026-11-19)

- **Poprawka: Audycje LIVE – emisja po ostatniej stałej godzinie harmonogramu** (np. 20:00 w harmonogramie, który ma już emisję 07:15) trafia za nią; dotąd lądowała przed nią, z kopią pozycji „wypełnij” pomiędzy. Godziny emisji na liście audycji są po kolei. Wydanie 26.11.14 nie powstało (bramka e2e panelu na prawdziwym legacy zatrzymała je na tej usterce) – jego zmiany są tutaj.

## 26.11.14 (2026-11-19, niewydane – zob. 26.11.15)

- **VizRS CG System 26.11.11** (`VIZRS_REF`, obraz `vizrs`): Trio i Artist mogą logować kontami panelu (przełącznik „Logowanie w pakiecie VizRS”, domyślnie wyłączony), w Trio pole serwera na ekranie logowania i przycisk ⎋ Wyloguj.

- **Konta operatorów w panelu – logowanie loginem i hasłem zamiast tokenu.** Konta zakłada się w panelu, bez edycji plików i zmiennych: przy pierwszym uruchomieniu panel prosi o kod z logu kontenera (Portainer: kontener `live` → Logs) i zakłada konto administratora, a kolejne konta (role administrator / operator) – ekran **System → Użytkownicy** (nowe hasło, wylogowanie, usunięcie). Każdy zmienia swoje hasło w **Moje konto** (klik w „konto: …” w pasku górnym). Konta i sesje leżą na nowym wolumenie `live-data` (`VIZRS_LIVE_DATA=/data` w obrazie) – aktualizacja panelu już nikogo nie wylogowuje. Dziennik CG Playout podpisuje operacje kontem (`VizRS Live · jan (Studio 1)`). `VIZRS_LIVE_TOKEN` jest teraz opcjonalny i zostaje jako **token awaryjny**: skrypty (`Bearer`), przycisk „Token awaryjny” w oknie logowania, okno AppImage. API panelu: `/api/auth`, `/api/setup`, `/api/password`, `/api/users`; `/api/login` przyjmuje `{"user","password"}` (sam token w treści działa dalej). Stos: nakładka `docker-compose.live.yml` z wolumenem `live-data` – zaktualizuj stos (bez wolumenu panel działa jak dotąd, na token). DOKUMENTACJA §11, API.md §2, INSTRUKCJA, RUNBOOK §6.
- **Jedno konto w całym pakiecie (opcja):** przełącznik „Logowanie w pakiecie VizRS” na ekranie Użytkownicy – konta panelu logują też do VizRS Trio i Artist w tym stosie (zalogowany w panelu wchodzi do Trio bez ponownego logowania; usunięcie konta albo wyłączenie przełącznika zamyka jego sesje w Trio w ciągu 15 s). Domyślnie wyłączone; Trio samo (OBS) dalej na swoim tokenie. API panelu `/api/sso…`; nakładka `docker-compose.live.yml` podaje VizRS adres panelu (`VIZRS_PANEL_LOGIN`). Wymaga VizRS CG System z `--panel-login` (starszy VizRS = tylko token).
- **Wybór wejścia kanału:** kanał z kilkoma wejściami (np. studio RTMP, YouTube, link) bierze to, które operator wybierze na karcie kanału w **Sloty na żywo** (lista „Wejście”, ze stanem każdego wejścia: nadaje, sygnał czeka, nie pobiera); „automatycznie” działa jak dotąd – pierwsze z sygnałem. Przełączenie zdejmuje bieżące źródło od razu; YouTube i linki, których nie wybrano, nie pobierają strumienia. Wybór przeżywa restart (`control.json`), trafia do dziennika emisji (`input`) i do dziennika operacji; wybrane wejście usunięte z listy przywraca „automatycznie”. Live API: `PUT /api/live/channels/<n>` `{"input": id|null}`, w stanie kanału `input` i przy wejściach `id`, `label`, `state`; cecha `input_select`. Panel ze starszym CG Playout – bez listy.
- **Wejście z linku w live ingest:** wejście kanału może być linkiem do strumienia – HLS (`https://…/index.m3u8`) i inny strumień HTTP(S), RTMP(S) z innego serwera (`rtmp://serwer/app/klucz`), SRT jako caller (`srt://serwer:port`) albo RTSP. Usługa `ingest` sama pobiera strumień (ffmpeg, bez transkodowania: H.264 + AAC; HTTP w czasie rzeczywistym, limit odczytu 15 s, protokoły zawężone do schematu linku) i przejmuje nim kanał jak sygnałem ze studia; po końcu albo zerwaniu czyta go ponownie, wejście zostaje na liście. Panel: **Ustaw wejścia…** → rodzaj „Link (HLS, RTMP, SRT, RTSP)”; adres w karcie kanału ukryty jak klucz, w tabeli wejść bez tokenów. Live API: `inputs` kanału `{kind:"link", url, scheme}`. Zmiana rozpoznawania: `rtmp://` i `srt://` są odbiornikiem na `0.0.0.0`, `[::]` i adresie lokalnym (`127.*`, `[::1]`, `localhost`) – wpis `VIZRS_INGEST` z innym hostem (dotąd odbiornik na tym adresie) jest teraz linkiem.
- **Live ingest: koniec klatkowania na wejściu i zejściu live.** Kanał, którego oś czasu w ErsatzTV została 10–14 s za zegarem (jeden wolniejszy kawałek ramówki, restart ErsatzTV), wpadał w pętlę: w każdym kawałku ok. 1 s live, potem ramówka – obraz klatkował i skakał, a grafika przejęcia wchodziła i schodziła co 44 s. Nakładka ma teraz dwa progi z zapasem: przebieg live wolniejszy od zegara (ponad 10 s straty od swojego startu) jest kończony na 30 s, a oś czasu dalej niż 60 s za zegarem zatrzymuje live do powrotu poniżej 45 s (dotąd 10 s i powrót dopiero przy zegarze, którego ramówka z `VIZRS_FFMPEG_SYNC_RAM` nie nadgania). DOKUMENTACJA §6.
- **Live ingest: grafika bez poszarpanych pasów na sygnale live.** Gdy strefy grafiki (`area: auto`) poszerzały się w trakcie przebiegu live – np. na starcie kawałka po ręcznym TAKE z panelu – restartował się tylko przebieg ramówki, a live zostawał na wstrzymanym strumieniu klatek starych stref: w miejscu belek kolorowe, porwane pasy. Teraz przebieg live też rusza od nowa z nowymi strefami (sygnał od miejsca, w którym stanął). DOKUMENTACJA §1, Obszar grafiki.
- **Grafika wejścia na czas przejęcia:** wejście studia (RTMP, SRT, YouTube) może mieć własną grafikę `on_live` – np. logo z dopiskiem NA ŻYWO tylko dla transmisji z YouTube; bez niej wchodzi grafika live kanału. Panel: **Ustaw wejścia…** → „Na antenie z tego wejścia”; Live API: pole `on_live` w `/api/live/inputs`, cecha `input_on_live`; odbiornik podaje wejście w `POST /ingest?…&input=<id>` (starszy odbiornik = grafika kanału). DOKUMENTACJA §1, Grafika podczas przejęcia.
- **Audycje LIVE: jedno okno na EPG i emisje.** „+ Nowa audycja LIVE” i „Edytuj…” pokazują dane w EPG (tytuł, długość, nowa **kategoria wiekowa** – `content_rating` w pliku audycji, Live API `content_rating`) i **emisje w ramówce**: godzina, dni (harmonogram główny / alternatywny kanału), start ściśle albo elastycznie, własny tytuł emisji w EPG i znak wodny. Panel sam zapisuje pozycje w harmonogramie kanału (z powrotem treści „wypełnij” po audycji) i przebudowuje ramówkę; lista audycji pokazuje godziny emisji. Dotąd godzinę emisji trzeba było ułożyć ręcznie w Harmonogramach według pięciu kroków z kreatora.
- **Wejście YouTube w live ingest:** obok RTMP i SRT wejście kanału może być adresem YouTube – usługa `ingest` sama pobiera strumień (yt-dlp znajduje H.264 do 1080p i AAC, ffmpeg przepakowuje bez transkodowania) i przejmuje kanał jak sygnał ze studia. Strona live kanału (`…/@nazwa/live`) czeka na każdą transmisję; pojedynczy film gra raz (transmisja do końca, nagranie w czasie rzeczywistym), potem sidecar usuwa go z listy (`POST /ingest/listeners/done`). Panel: rodzaj „YouTube” w **Ustaw wejścia…**, w „Jak nadawać” i w tabeli wejść. Obraz `vizrs-cg` zawiera yt-dlp 2026.10.26 (przypięty sumami SHA-256), instalowany na wolumen `vizrs-client`; opcjonalnie `VIZRS_YTDLP_COOKIES`. DOKUMENTACJA §1, Live ingest.

## 26.11.13 (2026-11-17)

- **Renderer natywny z VizRS CG System 26.11.10** (`VIZRS_REF`): reguła krawędzi powiązań przelicza obrys celu po wczytaniu jego fontu z zasobów – kafel programu odsuwa się od dwuwierszowej belki o jej pełną wysokość (dotąd o wysokość zmierzoną bez tekstu, S2 kanał 4).

## 26.11.12 (2026-11-16)

- **Renderer natywny z VizRS CG System 26.11.9** (`VIZRS_REF`, dotąd 26.11.3): powiązania (`dock`) z celem na innej warstwie działają na antenie przy procesie na warstwę (`native_layers`, domyślnie) – dotąd proces warstwy nie widział celów z innych warstw i grafika stała w miejscu (obejście `VIZRS_NATIVE_LAYERS=0` przestaje być potrzebne). Do tego reguły krawędzi z obrysu celu (dynamiczna wysokość belki), łańcuch, krzywa `ease` i powrót po OUT celu, a także zmiany renderera z wydań 26.11.4–26.11.8 (m.in. wejście przy celu na antenie od razu na miejscu, maska warstwy, miniatury stron). DOKUMENTACJA §1.

## 26.11.11 (2026-11-16)

Pierwsze wspólne wydanie VizRS CG Playout z panelem kanału (dawny VizRS Live): sidecar, panel, most VizRS Bridge i aplikacja okienna w jednej wersji. Stosy z grafiką wymagają `COMPOSE_PROFILES=grafika` albo `grafika,ingest`.

- **Połączenie z VizRS Live:** panel kanału, most VizRS Bridge i aplikacja okienna w jednym repozytorium (`panel/`); wspólna numeracja wersji całego produktu i jedno wydanie w `.github/workflows/release.yml`.
- **Profile compose `grafika` i `ingest`:** jeden plik `docker-compose.yml` w korzeniu repozytorium; bez profilu startuje sam ErsatzTV (np. z panelem), profil `grafika` włącza usługi `vizrs` i `cg`, a profil `ingest` – usługę `ingest` (uwaga: stosy Portainera z grafiką muszą mieć `COMPOSE_PROFILES=grafika` albo `grafika,ingest` przed wdrożeniem tej wersji – RUNBOOK panelu §2).
- **Jedna nakładka `docker-compose.live.yml`:** ujednolicone wdrożenie panelu w Portainerze i lokalnie (usunięto `panel/compose*.yml`), konfiguracja Caddy wbudowana w `configs:` nakładki z weryfikacją przez `panel/scripts/check-caddy.sh`; lokalna budowa obrazów przez nakładkę `panel/compose.build.yml`.
- **Nazwa „VizRS CG Playout” w panelu i oknie:** ujednolicona nazwa interfejsu panelu i aplikacji okiennej z dopiskiem „Powered by ErsatzTV” w oknie „O programie”.
- **Aplikacja okienna (AppImage):** wyłącznie natywny klient panelu działającego na serwerze produkcyjnym (usunięty tryb panelu lokalnego); połączenie przez HTTPS/TLS i token panelu.
- **Obrazy panelu z `:latest`:** `docker-compose.live.yml` bierze `vizrs-live:latest` i `vizrs-live-legacy:latest` (wcześniej przypięte 26.11.8, logo na przejęcie kanału przez live ingest), jak `vizrs-cg` – stos odświeża je razem z wydaniem produktu.

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

## Historia panelu (VizRS Live do 26.11.10)

Do połączenia (2026-11-15) panel kanału, most VizRS Bridge i jego okno były osobną aplikacją VizRS Live (repozytorium `sdorpl/VizRS-Live-Playout`, obrazy `vizrs-live`, `vizrs-live-legacy`). Poniżej jej wydania; od 26.11.11 zmiany panelu są w sekcjach wydań wyżej.

### 26.11.10 – wydanie (2026-11-14)

- **Aplikacja VizRS Live – poprawka: puste, „rozjechane” okno na Linuksie z kartą NVIDIA** (zgłoszenie: KDE Plasma 125 %, Quadro P400, sterownik 580): rasteryzacja Skia na GPU wywracała proces strony WebKitGTK (`libnvidia-eglcore`) – po kilku sekundach albo przy zmianie rozmiaru okna treść znikała lub zostawała w starym rozmiarze. Przy NVIDIA warstwy rysuje teraz Skia na CPU, składa je dalej GPU (`WEBKIT_SKIA_ENABLE_CPU_RENDERING=1`). Gdy proces strony mimo to padnie, okno samo wczytuje stronę od nowa. Ten sam `gpu.rs` w VizRS CG Artist 26.11.8 i oknie Trio (VizRS CG System 26.11.7). DOKUMENTACJA §3. Panel (`server/`) bez zmian.

### 26.11.9 – wydanie (2026-11-14)

- **Aplikacja VizRS Live – poprawka skalowania ułamkowego na Linuksie** (zgłoszenie: 1920×1080, KDE Plasma Wayland 125 %, NVIDIA): WebKitGTK rysował stronę ×2 i kompozytor ją zmniejszał – nierówne cienkie linie, 2,56× więcej pikseli. W KDE przy niecałkowitej skali okno otwiera się teraz przez XWayland: ostre piksele, ta sama wielkość interfejsu. `VIZRS_GPU=wayland` – jak dawniej. Ten sam moduł (`gpu.rs`, `scale.rs`) w VizRS CG Artist 26.11.7 i oknie Trio. DOKUMENTACJA §3.
- **AppImage aplikacji VizRS Live** dołączony do wydania (panel lokalny `vizrs-live` w środku).

### 26.11.8 – wydanie (2026-11-14)

26.11.7 nieopublikowane (bramka wydania: test e2e wybierał pole w zwiniętej karcie reguły kanału).

- **Grafika podczas przejęcia przez live ingest (`on_live`):** w Grafice kanału w karcie reguły kanału pole „Na przejęcie przez live ingest” (`on_live`, np. `TAKE 7510` dla logo „NA ŻYWO”) pozwala zdefiniować grafikę wchodzącą na antenę podczas przejęcia kanału przez sygnał live ingest nakładki ffmpeg; grafika ramówki jest w tym czasie wstrzymywana i wraca automatycznie po powrocie do ramówki (w tabeli „Kiedy / Co / Ile” wiersz „przejęcie przez live ingest (grafika ramówki wstrzymana)”). W Slotach na żywo karta kanału pokazuje wiersz **Grafika: live – ramówka wstrzymana** (podczas przejęcia) albo „z ramówki” (pole `takeover`). W Dzienniku emisji (as-run) nowe rodzaje zdarzeń: `live_graphics_on` („przejęcie live – grafika ramówki wstrzymana”) i `live_graphics_off` („koniec przejęcia – grafika ramówki wraca”). **Wymaga VizRS CG Playout z cechą `on_live` w `features`** (starszy CG ukrywa pole w edytorze reguł, a zapisane wcześniej `on_live` zachowuje bez zmian). DOKUMENTACJA §4, INSTRUKCJA §2.

### 26.11.6 – wydanie (2026-11-14)

- **Dziennik emisji – centralne miejsce logów systemu emisji:** obok zapisu anteny (as-run) wybór źródła: **live ingest – odbiorniki** (nadawcy RTMP/SRT przychodzący i odchodzący, błędy ich ffmpeg), **przejęcia kanału** (przebiegi kawałek/live, tempo, oś czasu kanału względem zegara), **sidecar VizRS CG** (wiersze kanału i ogólne), **ErsatzTV legacy** (log legacy z wybranego dnia) oraz **wszystko na jednej osi czasu** z kolumną „Źródło”; próg poziomu (uwaga, błędy), filtr, eksport CSV każdego widoku. Klucze strumieni i hasła maskuje CG Playout. Logi VizRS CG włączają się, gdy CG Playout zgłosi `logs` w `features` – **wymaga VizRS CG Playout z tego cyklu** (`GET /api/live/logs`); pośrednik: `GET /api/cg/live/logs/{cg|ingest|takeover}/{kanał}/{RRRR-MM-DD}`. DOKUMENTACJA §3 i §4.

- **Aplikacja VizRS Live – rozmiar okna przy skalowaniu ułamkowym:** okno 1600×900 px logicznych wychodziło poza ekran 1920×1080 przy 125 % (1536×864); teraz jest przycinane do obszaru roboczego monitora (`desktop/src/screen.rs`, ten sam moduł w VizRS CG Artist i oknie Trio). DOKUMENTACJA „Aplikacja VizRS Live”.

- **Aplikacja VizRS Live (`desktop/`) – akceleracja sprzętowa na Linuksie z kartą NVIDIA** (sterownik własnościowy): okno wymusza renderer DMA-BUF WebKitGTK (`WEBKIT_FORCE_DMABUF_RENDERER=1`, `__NV_DISABLE_EXPLICIT_SYNC=1`) – dotąd WebKitGTK sam wyłączał akcelerację i panel rysował się na CPU (animacja 37 → 61 kl./s w oknie połączenia). Zmienna `VIZRS_GPU` (`auto`, `on`, `off`, `x11`, `system`); zmienne `WEBKIT_*` użytkownika mają pierwszeństwo. Moduł `desktop/src/gpu.rs` – ten sam w VizRS CG Artist i oknie Trio (VizRS CG System). DOKUMENTACJA „Aplikacja VizRS Live”.

### 26.11.5 – wydanie (2026-11-13)

Wydania 26.11.3 i 26.11.4 nie zostały opublikowane: bramka liczyła zamierzony 404 brakującej miniatury strony VizRS jako błąd JS, test powiększenia miniatury zależał od tego, co zasłania stronę, a `desktop/Cargo.lock` nie znał nowej wersji (poprawione). Zawiera zmiany 26.11.3 i poniższe.

- **Powiększenie miniatury po najechaniu:** w wyborze elementów graficznych i znaków wodnych (lista podpowiedzi, wybrane etykiety, pola tak/nie) najechanie myszą na miniaturę pokazuje jej większą wersję – łatwiej odróżnić podobne grafiki VizRS. Na ekranach dotykowych bez zmian.

### 26.11.3 – nieopublikowane (2026-11-13)

Zawartość 26.11.2 i poprawka routera niżej (26.11.2 nie zostało opublikowane – bramka wydania wykryła ten błąd). Razem z VizRS CG System 26.11.3, VizRS CG Artist 26.11.3 i VizRS CG Playout 26.11.4 (miniatury stron VizRS wymagają CG Playout ≥ 26.11.4).

- **Ekrany nie nadpisują się nawzajem:** wolny ekran (np. odświeżenie listy playoutów po usunięciu), który skończył się wczytywać już po przejściu na inny, nie zastępuje nowego widoku – wcześniej po szybkim przejściu można było zobaczyć poprzedni ekran.

- **Wybór znaków wodnych i elementów graficznych bez przewijania** (Harmonogramy – pozycje, Bloki, Deco): zamiast długiej listy pól tak/nie wyszukiwarka z podpowiedziami (wystarczy wpisać fragment nazwy, bez polskich znaków też trafia; ↑/↓ i Enter), wybrane pozycje jako etykiety z „×” i „wyczyść”. Każda pozycja ma miniaturę: znak wodny – swój obraz (logo kanału – napis „logo”), element graficzny będący stroną VizRS (`page-<nr>.yml`) – obraz strony narysowany przez renderer natywny VizRS w CG Playout (≥ 26.11.4; pośrednik `/api/cg/live/thumbs/<nr>.png`, bez niego napis „VizRS n”), inny element – rodzaj z katalogu pliku (tekst, obraz, ruch, skrypt). Krótkie listy (do 6) zostają polami tak/nie, też z miniaturą.

- **Aplikacja VizRS Live na Tauri 2** (`desktop/`, `vizrs-live-desktop`): panel lokalny (program uruchamia `vizrs-live` na `127.0.0.1` i zamyka go przy wyjściu) albo klient panelu zdalnego przez pośrednika w programie – token zostaje w programie, strona go nie dostaje. Okno „Połącz z panelem” z listą zapisanych, menu, jedna instancja, tytuł okna z ekranem i alarmem (okno w tle prosi o uwagę), paczki z bundlera Tauri z `vizrs-live` obok programu. Panel w przeglądarce bez zmian (nowy moduł `desktop.js` działa tylko w aplikacji). DOKUMENTACJA §3 „Aplikacja VizRS Live”.

- **Wspólny rdzeń wyglądu rodziny VizRS (`vizrs-ui.css`):** tokeny i kontrolki panelu (przyciski, pola, pigułki, pasek górny, menu z szufladą, tabele, formularze, okna, karty) wydzielone ze `style.css` do pliku wspólnego z VizRS CG System i VizRS CG Artist (źródło w CG System; `scripts/sync-ui.sh`, kontrola zgodności w CI i `tests/deploy.sh`). Wygląd panelu bez zmian.

- **Profile kodowania bez akceleracji sprzętowej:** na maszynie, na której żadna akceleracja nie koduje (sonda silnika Next), nowy profil startuje programowo, niedostępne akceleracje są oznaczone „brak na tej maszynie”, a lista profili ostrzega przy profilu, który i tak pójdzie programowo. Panel kodera pokazuje, że obowiązuje libx264 (profil domyślny „x264: high”, podpowiedź presetu dla procesora), a ustawienia NVENC/QSV/klatek B zwija. Poprawka: zapis kodera nie kasuje już ustawień sprzętowych, gdy ich pól nie widać. Atrapa mostu: `MOCK_NO_HW=1`.

- **Przeglądarka – karta pozycji na telefonie:** okno karty nie przewija się już w bok (szeroka tabela strumieni rozpychała całe okno – teraz przewija się sama tabela); na wąskim ekranie okno na całą szerokość, węższa kolumna opisów, mniejsza okładka nad danymi. Odcinki w karcie serialu i sezonu w dwóch kolumnach: numer ze stanem pod spodem, tytuł z nazwą pliku pod spodem – plik tylko, gdy różni się od tytułu. Odcinki bez numeru w nazwie pliku (legacy: E00) przygaszone z podpowiedzią i ułożone naturalnie po tytule (#2 przed #10). Czas pozycji bez ułamków sekund.

- Diagnostyka → Stan: wiersz „strefy grafiki” podaje też liczbę granic stron z VizRS (`bounds` w `/health` CG Playout, PLAN-strefy S6; starszy CG Playout – bez tej liczby).

### 26.11.1 – wydanie (2026-11-12)

Pierwsze wydanie w numeracji rok.miesiąc.rewizja (`26.11.1` = listopad 2026, wydanie 1); obejmuje zmiany z 1.3.0-test.1 i poniższe. Gałąź `main` = dawne `next-v`; poprzednia `main` zachowana jako `main-oldstable`. Most w obrazie `vizrs-live-legacy:26.11.1` (stos CG Playout przypina tę wersję).

- **Wydajność emisji – konfiguracja legacy:** `compose.yml` montuje `/transcode` jako tmpfs (segmenty HLS kanałów w RAM, jak w CG Playout) – wcześniej samodzielne wdrożenie zapisywało je na warstwę kontenera na dysku, a wolny dysk spóźniał segmenty widzom. Obraz `vizrs-live-legacy` nie ma już limitu sterty .NET (`DOTNET_GCHeapHardLimit=1,5 GB`) ani `DOTNET_EnableWriteXorExecute=0` na wszystkich platformach – limit groził upadkiem legacy (i całej emisji) przy dużych bibliotekach na serwerach z dużą ilością RAM; strojenie pod Ampere A1 zostaje w nakładce `docker-compose.ampere.yml` CG Playout. `DOTNET_gcServer=0` bez zmian. Działa po przebudowie obrazu legacy (`docker compose up -d --build ersatztv` – restart legacy przerywa emisję, robić w oknie serwisowym).
- **Przeglądarka – odtwarzanie bez legacy i bez transkodowania:** film, odcinek albo audycja grają prosto z pliku – z Jellyfin (adres i klucz z legacy zostają w serwerze panelu) albo z biblioteki lokalnej (media zamontowane w kontenerze panelu tak jak w legacy, tylko odczyt; nowy wolumen usługi `live` w `compose.yml`, `compose.cg-playout.yml` i `docker-compose.live.yml` CG Playout) – `POST /api/vod/open`, `GET /api/vod/{id}` z `Range`. Pełne przewijanie, start w 1–2 s, bez blokady „jedno odtwarzanie naraz”; działają też pozycje, których legacy z profilem NVENC nie kodował. Kodek nieznany przeglądarce = samo przełączenie na podgląd przez legacy. Długie listy rozdziałów w karcie zwinięte.
- **Profile kodowania – koder nakładki VizRS CG:** preset x264 (ultrafast…placebo), profil H.264 (baseline, main, high – dla libx264, NVENC, QSV i VAAPI), opcje libx264, a przy akceleracji sprzętowej preset NVENC (p1–p7), preset QSV i klatki B (NVENC/QSV/VAAPI) ustawia się teraz w panelu zamiast w `.env` serwera (silnik Next pomija preset i profil z profilu kodowania, także przy akceleracji; NVENC bez ustawienia koduje w `main`). Opcje: zmierzone warianty (`subme=0` −8% CPU, `bframes=0` −15%, `bframes=0:subme=0` −20% przy −0,4 / −0,8 / −1,4 dB SSIM) albo własne. `.env` (`VIZRS_FFMPEG_PRESET`, `VIZRS_FFMPEG_X264_PARAMS`, `VIZRS_FFMPEG_NVENC_BF`) wypełnia ustawienia tylko przy pierwszym starcie CG. Działa od następnego startu ffmpeg. **Wymaga VizRS CG Playout z `/api/live/encoder`** (ten sam cykl); starszy CG – uwaga, zostaje .env.
- **Przeglądarka – okładki i odtwarzanie:** okładki filmów, seriali i sezonów w tabeli i w karcie pozycji (odcinek – plakat serialu), podawane przez serwer panelu (`/api/artwork/…`) – działają też z telefonu, gdy adres dla widzów to `localhost`. W karcie filmu, odcinka czy audycji sekcja **Odtwarzanie**: od wybranego miejsca, ‹ 30 s / 30 s ›, wybór profilu; przez test odtwarzania legacy (`POST /api/play/…`), fragmenty po 30 s doklejane w tle. Na profilu NVENC część źródeł Jellyfin legacy nie zakoduje (błąd filtra legacy) – wtedy profil bez akceleracji.
- **Przeglądarka – serial i sezon:** klik w serial albo sezon nie kończy się już błędem legacy „Specified argument was out of the range” (`GetMediaItemInfo` nie działa dla pozycji bez pliku). Karta serialu pokazuje sezony i odcinki (z „Skanuj serial” i „Wyodrębnij napisy”), karta sezonu – jego odcinki; klik w odcinek otwiera kartę pozycji, „‹ Wróć” wraca. Komunikaty z wyjątkiem legacy bez stosu wywołań .NET i zawijane na telefonie.
- **Grafika kanału – edytor reguł z wyborem grafik:** zamiast pisać komendy `TAKE 4001 headline={title}` operator wybiera akcję (Wprowadź, Dalej, Zdejmij, Podmień dane), stronę z listy show VizRS i pola strony z listą symboli ramówki (`{title}`, `{next_title}`, `{next_start}`…); kroki w kolejności, podgląd komendy, tryb „Tekst” dla zaawansowanych. Reguły audycji i kanałów jako zwijane karty (kanał z listy CG Playout), tryb pola „jak globalnie” / „wyłączone” / „grafiki VizRS” zamiast pustego pola i „-”. Streszczenie „Kiedy / Co / Ile” opisuje komendy po polsku i stoi nad edytorem. Zapis w tej samej składni co dotąd – CG Playout bez zmian; nierozpoznana linia zostaje bez zmian jako „Komenda tekstowa”.
- **Nowy wygląd panelu:** odświeżony styl (karty z cieniem, zaokrąglenia, nowe przyciski i pola, przyklejone nagłówki tabel, pigułki stanu z kropką – „na antenie” pulsuje, animowane okna i komunikaty), ikony w menu, menu zwijane do paska ikon przyciskiem ☰ (pamiętane w przeglądarce), na telefonie menu jako szuflada. Bez zmian w działaniu ekranów; podpowiedź pola „Logo” nie jest już ściśnięta w wąskiej kolumnie.
- **Profile kodowania – „Klatkaż”:** stały klatkaż wyjściowy kanału (25, 50, 23,976, 29,97…), którego legacy nie pozwala wpisać – most nadpisuje wyliczenie legacy (`GetChannelFramerate`, najniższy klatkaż z ramówki) dla kanałów z tym profilem; silnik Next dopasowuje do niego każdy materiał. Do wyboru też „automatycznie” (jak legacy) i „jak materiał”. Wymaga mostu z tego wydania (obraz `vizrs-live-legacy`); starszy most – zostaje przełącznik legacy.
- **Podgląd kanału (też mozaika):** koniec zacinania co kilka sekund. Kanał z nakładką grafiki CG Playout powstaje w tempie zegara, więc najnowszy segment kończy się ok. „teraz”, a podgląd grał dokładnie „teraz” (pomiar: 29 przestojów i 14 skoków na minutę, tempo 1,0↔1,06). Teraz gra ze stałym, mierzonym opóźnieniem za anteną (najgorsze spóźnienie segmentu z ostatniej minuty + 2 s, zwykle 3–5 s; w stopce „N s za anteną”).
- **Sloty na żywo:** „studio także poza audycjami” nie wraca już do starego stanu tuż po kliknięciu (odświeżanie co 1 s odczytywało stan sprzed zapisu; operator klikał wtedy kilka razy, w dzienniku CG seria włącz/wyłącz) – pole zablokowane do końca zapisu, karta trzyma kliknięty stan do potwierdzenia przez CG.

- **Grafika kanału – „Kiedy strona wchodzi na antenę”:** przy każdej stronie VizRS kanału trzy źródła naraz – czas z Trio (⏱ kanał), reguły CG Playout z TAKE tej strony i element legacy z przypisaniami w harmonogramach, blokach i deco („Sprawdź przypisania w ramówce”); ostrzeżenie, gdy stronę wprowadza i reguła, i ramówka.
- **Dziennik emisji:** grafika wprowadzona i zdjęta w Trio, makrem, karuzelą VizRS albo z Companion trafia do dziennika (operator „VizRS (poza panelem)”) – wymaga VizRS CG Playout z tego cyklu; starszy CG zapisuje tylko komendy z panelu.
- **Kolekcje:** kolekcje audycji LIVE (`LIVE · …`) oznaczone pigułką „CG Playout” i tylko do odczytu – prowadzi je CG Playout razem z audycją; nazwa `LIVE · …` zarezerwowana.
- **Wdrożenie:** `scripts/check-cg-overlay.sh` (też na początku `tests/deploy.sh`) pilnuje, żeby Caddyfile, usługa `live` i nakładka VAAPI były takie same jak w `docker-compose.live.yml` CG Playout; nakładka `compose.cg-playout.yml` dostała `TZ: Europe/Warsaw` jak w CG.

### 1.3.0-test.1 – wydanie testowe (2026-11-10, gałąź `next-v`)

Wydanie wstępne z przebiegiem planu rozwoju [PLAN-mcr](docs/plany/PLAN-mcr.md) (K1–K18). Obrazy `ghcr.io/sdorpl/vizrs-live:1.3.0-test.1` i `vizrs-live-legacy:1.3.0-test.1` – bez przesuwania `:latest` (produkcja na `latest` bez zmian). Most: nowe wiersze białej listy (K3–K8) – do testu potrzebny obraz legacy z tej wersji. K12–K14 i webhook K17 zadziałają z VizRS CG Playout zgodnym z kontraktem (DOKUMENTACJA §3); starszy CG – te elementy UI się nie pokazują. Q1 (prawdziwy sygnał OBS, Plex) – ręcznie z użytkownikiem.

- **K12–K17:** VizRS CG Playout (gałąź `next-v`, 2026-11-10) ma już stronę serwerową kontraktu: `features`, warstwy na antenie, `take`/`continue`/`out`, dziennik emisji `GET /api/live/asrun`, `graphics_off_since` i webhook alarmów (`VIZRS_ALERT_WEBHOOK`). Panel i CG Playout wydaje się razem.
- **K12–K17 – strona panelu (kontrakt CG Playout w DOKUMENTACJA §3):** warstwy VizRS w karcie kanału (K12), „Dalej” / „Zdejmij” warstwy i „Wprowadź stronę…” (K13), ekran „Dziennik emisji” z eksportem CSV (K14), alarmy w przeglądarce – banner i dźwięk (K17); każda funkcja włącza się dopiero, gdy VizRS CG Playout zgłosi ją w `features` (starszy CG – bez zmian w UI). K15/K16 – kontrakt API. **Wymaga wydania VizRS CG Playout z tym kontraktem** (krok ręczny).

- **K18 – podgląd HEVC i AC-3:** `mux.js` przepakowuje HEVC (Main, Main 10) i AC-3 (do 5.1) na fragmenty MP4, gdy przeglądarka dekoduje je w MSE (Safari, Chrome/Edge ze sprzętowym HEVC); w pozostałych bez zmian (HEVC – natywny HLS, AC-3 – bez dźwięku). Zamyka LUKI #21.

- **K11 – import ramówki z pliku** (Vizrt E): Playlisty → „Import ramówki z pliku…” – CSV albo XML z systemu emisyjnego, dopasowanie pozycji do mediów legacy z podglądem (dopasowane / niejednoznaczne / brak), zapis jako nowa playlista legacy.

- **K10 – mozaika kanałów** (Vizrt J): przycisk „Mozaika” w Na antenie – siatka podglądów, dźwięk z jednego kafla, czerwona ramka przy błędzie ramówki; limit `VIZRS_LIVE_MULTIVIEW_MAX` (domyślnie 4, 0 = bez mozaiki). Atrapa mostu: `MOCK_HLS=1` – strumienie testowe ffmpeg.

- **K9 – kontrola ramówki** (Vizrt B): dziury, nakładanie, koniec playoutu w ciągu 24 h, pliki w koszu, audycja LIVE bez wejścia studia – „Sprawdź ramówkę” i „BŁĄD RAMÓWKI” w Na antenie, czerwone pozycje i dziury w EPG, kolumna „Kontrola” w Playoutach.

- **K8 – biblioteki:** „Foldery obrazów” w Bibliotekach (czas wyświetlania obrazu na folder), „Skanuj serial” i „Wyodrębnij napisy” w karcie serialu, „Wyodrębnij napisy” w konserwacji playoutu (`ExtractEmbeddedSubtitles`, `ExtractEmbeddedShowSubtitles` na białej liście). Zamyka LUKI #18 i #26.

- **K7 – historia bloków** w edytorze playoutu blokowego (tylko odczyt): bloki z szablonów, wpisy historii ze stronicowaniem, rozszyfrowanie wpisu (kolejność, kolekcja, ostatnia pozycja). Zamyka LUKI #13; z #25 zostaje walidator YAML (poza MediatR).

- **K6 – wyniki zadań w tle:** po „Sprawdź nakładanie pozycji” (Playouty) i zadaniach konserwacji (Diagnostyka) panel czyta log legacy do 20–60 s i pokazuje wynik w oknie (wpis legacy albo opis, co znaczy brak wpisu). Zamyka LUKI #10.

- **K4 – własne rozdzielczości** (Profile kodowania → „Rozdzielczości”): dodawanie (`CreateCustomResolution`, kontrola parzystości i duplikatu) i usuwanie własnych (`DeleteCustomResolution`). Zamyka LUKI #24.
- **K5 – silnik Next:** podpowiedź akceleracji w edytorze profilu wg `GetNextProbe` (ta sama logika co edytor legacy 26.12), karta „Silnik Next” w Diagnostyce (wersja, schematy, akceleracje z kodekami, „Sprawdź ponownie” – `RefreshNextProbe`). Zamyka LUKI #11; paczka diagnostyczna (#27) zostaje – plik ZIP powstaje tylko na dysku legacy, pobranie wymagałoby nowego kodu C# (DOKUMENTACJA §4).
- Atrapa mostu `tests/mock-bridge.cjs` (metadane komend ze źródeł legacy, przykładowe kanały i ramówka) – zrzuty UI bez Dockera i legacy.

- **K3 – numeracja kanałów naraz** (Kanały → „Numeracja…”): tabela numerów wszystkich kanałów, kontrola duplikatów w przeglądarce, zapis jednym `UpdateChannelNumbers` (na białej liście z `GetAllChannelsForSort`). Zamyka LUKI #23.

- **K2 – CSP zawężone:** `img-src` i `frame-src` tylko z originów `--stream-url` i `--vizrs-url` (bez opcji – brak źródła) zamiast `http:`/`https:`; `prod-smoke.cjs` oblewa przebieg przy naruszeniu CSP. Zamyka LUKI #6.

- **K1 – sesje panelu:** ciasteczko `vizrs_live` niesie losowy identyfikator sesji zamiast tokenu panelu; „Wyloguj” unieważnia sesję, sesje wygasają po 30 dniach bezczynności (limit 256), restart panelu wymaga ponownego logowania. Token w nagłówku `Authorization: Bearer` dla skryptów bez zmian. Zamyka LUKI #2.

### 1.2.8 – wydanie (2026-11-09)

- **Zdejmij · Przywróć grafikę z animacjami** (Na antenie): VizRS gra animacje wyjścia (`OUT`) i wejścia (`TAKE`) zamiast natychmiastowego cięcia; wymaga VizRS CG Playout z sekwencją OUT/TAKE (2.12.4).

### 1.2.7 – wydanie (2026-11-09)

- **Obrazy zawsze najnowsze:** domyślne `LIVE_IMAGE=ghcr.io/sdorpl/vizrs-live:latest` i `PROXY_IMAGE=caddy:alpine` w `compose.yml`, `compose.cg-playout.yml` i `.env.example` (wcześniej przypięte wersje); przypięcie – własna wartość w `.env`.
- Wydania 1.2.5 i 1.2.6 nie zostały opublikowane (bramka CI padła na testach e2e); ich zmiany wchodzą w 1.2.7.

### 1.2.5 – wydanie (2026-11-09)

- **Sterowanie anteną w Na antenie**: emisja kanału – „Uruchom emisję”, „Restart emisji”, „Zatrzymaj emisję” z pigułką stanu sesji legacy (most: `GET /vizrs-bridge/sessions`, `POST /vizrs-bridge/session/{kanał}/stop`; panel: `/api/sessions`, `/api/session/{kanał}/{start|stop|restart}`); **„Zdejmij grafikę” / „Przywróć grafikę”** – cała grafika VizRS ze wszystkich kanałów (pośrednik `live/graphics/off|on`). Wymaga mostu z tej wersji i VizRS CG Playout z `/api/live/graphics/off|on` (2026-11-09).

### 1.2.4 – wydanie (2026-11-09)

- **Optymalizacja pod Oracle Ampere A1 (ARM64, Neoverse-N1):**
  - Panel `vizrs-live` w `docker/live.Dockerfile`: kompilacja dla `linux/arm64` z flagami mikroarchitektury `RUSTFLAGS="-C target-cpu=neoverse-n1 -C opt-level=3"`.
  - Obraz mostu `vizrs-live-legacy` w `docker/legacy-bridge.Dockerfile`: konfiguracja środowiska .NET CLR z lżejszym Workstation GC (`DOTNET_gcServer=0`) i limitem sterty (`DOTNET_GCHeapHardLimit=0x60000000`), eliminującymi zatrzymania wątków i przycięcia enkodera wideo na maszynach 4 OCPU ARM.
  - Zaktualizowane domyślne obrazy w `compose.yml` i `compose.cg-playout.yml` na wersję 1.2.4.

### 1.2.3 – wydanie (2026-11-08)

- Wersja panelu i mostu 1.2.3 (obrazy `ghcr.io/sdorpl/vizrs-live:1.2.3`, `ghcr.io/sdorpl/vizrs-live-legacy:1.2.3` – amd64 + arm64); działa z VizRS CG Playout 2.12.0 (wejścia studia ustawiane z panelu).
- **Wejścia studia ustawiane w panelu**: Sloty na żywo → „Ustaw wejścia…” (i „+ Dodaj wejście” przy kanale bez wejścia) – kanał, RTMP/SRT, port, klucz albo hasło („Losuj”); zapis przez API VizRS CG Playout (`/api/live/inputs`, pośrednik `live/inputs`), odbiorniki przejmują zmianę w kilka sekund bez restartu. Wymaga VizRS CG Playout z `/api/live/inputs` (2026-11-08); `VIZRS_INGEST` zostaje pierwszym wypełnieniem.
- **Na antenie wg makiety UI**: licznik „N/M kanałów w ramówce” i „Otwórz Trio ↗” w nagłówku; karta kanału ze stanem NA ANTENIE / ZASTĘPCZY / BRAK PLAYOUTU, plakietką „NA ŻYWO” na ramce VizRS, gdy studio jest na antenie, pigułką „wypełniacz” i wierszem stanu studia z CG Playout (sygnał, wstrzymane, poza audycjami, najbliższy slot LIVE); przerwa w ramówce opisana jak w makiecie („Wypełniacz zapasowy” z tym, co następne); „Najbliższe zdarzenia” z sekundami i dopiskiem wypełniacza / audycji na żywo.

### 1.2.2 – wydanie (2026-11-08)

- Wersja panelu i mostu 1.2.2 (obrazy `ghcr.io/sdorpl/vizrs-live:1.2.2`, `ghcr.io/sdorpl/vizrs-live-legacy:1.2.2` – amd64 + arm64); działa z VizRS CG Playout 2.11.1 (strefy grafiki `VIZRS_AREA=auto`, `VIZRS_FFMPEG_CUDA=encode`).
- Diagnostyka → „Stan”: wiersz „strefy grafiki” – strefy przebiegów nakładki CG Playout przy `VIZRS_AREA=auto` (PLAN-strefy S5 w CG Playout), % ekranu, grafika poza strefami jako UWAGA.
- CI i publikacja działają (LUKI #7 zamknięta): `ci.yml` zielone od 2026-11-07, wydanie v1.2.1 opublikowane, pakiety ghcr publiczne.
### 1.2.1 – wydanie (2026-11-07)

- Wersja panelu i mostu 1.2.1 (obrazy `ghcr.io/sdorpl/vizrs-live:1.2.1`, `ghcr.io/sdorpl/vizrs-live-legacy:1.2.1` – amd64 + arm64); pierwsze opublikowane wydanie po 1.1.0 – zawiera też całą sekcję 1.2.0 niżej.
- **Akceleracja sprzętowa w wydaniu**: obraz legacy z mostem (amd64) zawiera akcelerację Intel VAAPI/QSV, AMD VAAPI i NVIDIA NVENC z oficjalnego obrazu upstream (sterowniki iHD, i965, radeonsi, libmfx, libva, kodery ffmpeg); nowe nakładki `compose.vaapi.yml` (przekazanie `/dev/dri/renderD128` dla Intela i AMD) oraz `compose.nvidia.yml` (karta przez CDI, `NVIDIA_DRIVER_CAPABILITIES`); nowy skrypt `tests/hwaccel.sh` sprawdza obecność akceleracji bez GPU i jest uruchamiany w bramce `ci.yml` oraz `release.yml` przed publikacją obrazów; nakładki dołączone do archiwów binarek wydania; opis wdrożenia i konfiguracji w profilach kodowania w RUNBOOK §2b.
- **Poprawki po pierwszym przebiegu bramki w CI** (tag v1.2.0 bez publikacji – bramka nie przeszła): przejściowy błąd legacy (500) przy zapytaniach tylko do odczytu serwer panelu ponawia raz, treść błędu w dzienniku; „Zbudowany do” w Playoutach odpytuje legacy po kolei, nie naraz; Na antenie – audycje LIVE z CG Playout zawsze w „Najbliższych zdarzeniach” (wcześniej wypychały je bliskie pozycje ramówki); Logi – zmiana źródła czyści filtr tekstu.
### 1.2.0 – tag bez publikacji (2026-11-07; bramka w CI nie przeszła – poprawki w 1.2.1)

- Wersja panelu i mostu 1.2.0 (obrazy `ghcr.io/sdorpl/vizrs-live:1.2.0`, `ghcr.io/sdorpl/vizrs-live-legacy:1.2.0` – amd64 + arm64, na najnowszym ErsatzTV legacy); panel na wzór makiety UI (PLAN-makieta M0–M9), limit logowań na adres klienta, poprawiona integracja z VizRS, legacy zawsze najnowsze, działające workflow wydań. Most bez zmian w działaniu.
- **Limit logowań na adres klienta** (LUKI #4): limit 10 nieudanych logowań w oknie 60 s liczony osobno dla adresu IP klienta (po przekroczeniu 429 tylko dla tego adresu do końca okna, inne logują się dalej); bezpiecznik łączny 100 błędów na 60 s ze wszystkich adresów razem blokuje logowanie przy ataku rozproszonym; panel nie ufa nagłówkowi X-Forwarded-For (za proxy lub z userland-proxy Dockera wspólny IP działa jak limit łączny); pamięć do 4096 adresów z czyszczeniem wygasłych okien; w dzienniku panelu przy złym tokenie adres IP klienta (`login: zły token (<IP>)`).
- **Workflow GitHub Actions na wzór VizRS CG System i CG Playout**: `release.yml` – bramka, potem obrazy `vizrs-live` i `vizrs-live-legacy` na amd64 i arm64 (buildx, `:latest` przy wydaniu bez łącznika), binarki panelu `VizRS-Live-<wersja>-linux-{x86_64,arm64}.tar.gz`, wydanie na GitHubie z `SHA256SUMS.txt`; ręczne uruchomienie = szkic. Nowy `drafts.yml` (usuwanie szkiców). `ci.yml`: `--locked`, binarka jako artefakt, krzyżowa budowa obrazów na arm64. Naprawione: wszystkie workflow były nieprawidłowe (`runner.temp` w `env` joba) i padały od razu. Dockerfile budują bez QEMU: panel krzyżowo z `rust-lld`, most na SDK platformy budującej.
- **Legacy zawsze najnowsze**: domyślny obraz bazowy to `ghcr.io/ersatztv/legacy:latest` (jak `ersatztv` i `ingest` w CG Playout) zamiast przypiętego v26.12.0 – `docker/legacy-bridge.Dockerfile`, `compose.yml`, `compose.cg-playout.yml`, `.env.example`, stos testowy. Obraz z mostem `vizrs-live-legacy:local` budowany na miejscu przy każdym `docker compose up` (`pull_policy: build`, `build.pull: true`), także w nakładce CG Playout (kontekst `VIZRS_LIVE_DIR`); `tests/stack.sh up` z `--pull`. Konkretna wersja: `LEGACY_IMAGE=ghcr.io/ersatztv/legacy:v<wersja>`. `release.yml` dokleja `-legacy<wersja>` do tagu tylko przy wersji przypiętej. Przed `up` – kopia `etv-config` (RUNBOOK §4).
- **Integracja z VizRS CG System (Trio, podgląd `/output`) naprawiona**: VizRS z tokenem dawał w ramce podglądu i po kliknięciu „Trio ↗” tylko „wymagany token”, a panel nie znał anten kanałów. Nowe opcje: `--vizrs-output-token` (`VIZRS_LIVE_VIZRS_OUTPUT_TOKEN`, token wyjścia VizRS, tylko odczyt – do ramki), `--vizrs-channels` (`VIZRS_LIVE_VIZRS_CHANNELS=1`, jak `channel_buses` w CG Playout: ramka `bus=ch-N`, Trio `?channel=N`), `--vizrs-url` przyjmuje sam port (`:8788` = host panelu w przeglądarce) i jest sprawdzany przy starcie. Ramka na Antenie i w Grafice kanału idzie za wybranym kanałem, „Trio ↗” na karcie kanału otwiera jego antenę, podpowiedź o tokenie operatora przy pierwszym wejściu. `compose.cg-playout.yml` ustawia wszystko z `.env` CG Playout (`VIZRS_PORT`, `VIZRS_OUTPUT_TOKEN`, `VIZRS_CHANNEL_BUSES`). Diagnostyka pokazuje nowe ustawienia; e2e z atrapą VizRS.
- **M9 – Ustawienia i Diagnostyka wg makiety**: Ustawienia – sekcje legacy (FFmpeg i strumienie, Playouty, Przewodnik XMLTV, Skaner i HDHomeRun, Poziomy logów, Web GUI legacy) oraz panele Panel i Most jako panele w siatce obok siebie (wczytywane równolegle), każdy z osobnym „Zapisz” w nagłówku; sekcja z błędem odczytu pokazuje błąd tylko w swoim panelu; Diagnostyka – na górze panel „Stan” stosu (tabela Stan / Składnik / Opis z pigułkami OK / UWAGA / BŁĄD / INFO i „Sprawdź ponownie”: wersje legacy i mostu, brakujące komendy z białej listy, podsumowanie kontroli legacy, ostrzeżenia budowy playoutów, wersje Next i ffmpeg, stan CG Playout oraz adres VizRS); Logi – wybór źródła na początku paska filtrów: „ErsatzTV legacy” (z progiem poziomu i stronicowaniem) albo „VizRS CG Playout” (dziennik zdarzeń z `/api/live/state`: przejęcia kanału, ZDEJMIJ/WPUŚĆ, przebudowy ramówki, audycje LIVE z filtrem tekstem; CG nie wystawia `log.jsonl` przez API).
- **M8 – Grafika kanału i Sloty na żywo wg makiety**: Grafika kanału – panele obok siebie: tabela reguł „Kiedy / Co / Ile” (streszczenie reguł globalnych, audycji i wybranego kanału z pierwszą linią komendy, pigułkami „wyłączone” / „OUT” i czasem trwania; klik przenosi kursor do edytora) oraz Edytor reguł; Sloty na żywo – panel „Wejścia studia” z tabelą wejść RTMP/SRT pod kartami kanałów (adres bez sekretów, stan „nadaje · na antenie” / „nadaje” / „wstrzymany” / „czeka”, kodek wideo i dźwięk lub brak sygnału; odświeżanie co 1 s).
- **M7 – Kalendarz tygodnia wg makiety**: widok siatki godziny × dni w kalendarzu playoutu blokowego (bloki w stałych kolorach, pasek deco z prawej strony dnia, czerwona linia „teraz”, pigułki przypisań szablonów do dni i ostrzeżenie o braku szablonu, numer tygodnia w tytule, zakres godzin z bloków – ogon bloku nocnego z poprzedniego dnia jako znacznik w nagłówku); klik w blok – szczegóły pod siatką (długość, godziny, pozycje z `GetBlockItems`, deco w tej godzinie); przełącznik „Siatka” / „Paski” w nagłówku panelu pamiętany w przeglądarce.
- **M6 – Playouty wg makiety**: w liście „Zbudowany do” i „Przebuduj” w wierszu, rodzaj w pigułce (stan budowy tylko przy błędzie); najbliższe pozycje z rodzajem wypełniacza, pigułką LIVE (audycje CG Playout) i kolumną „Blok” w playoucie blokowym.
- **M5 – Kolekcje wg makiety**: domyślna zakładka „Wszystkie” – kolekcje czterech rodzajów w jednej liście z pigułką rodzaju, klik otwiera edytor w zakładce rodzaju, „+ Nowa …” dla każdego rodzaju.
- **M4 – Przeglądarka mediów wg makiety**: widok kafli (okładka legacy albo rodzaj pozycji, tytuł, plik, stan), przełącznik Tabela / Kafle pamiętany w przeglądarce, „Zapisz jako kolekcję inteligentną” z bieżącego zapytania.
- **M3 – Przewodnik EPG wg makiety**: wszystkie kanały w wierszach (też bez playoutu), okno 4 h z „teraz / 4 h →”, data w nagłówku, krótkie pozycje sklejone w bloki „pierwsza +N” (koniec nieczytelnego grzebienia przy pozycjach 2-minutowych), audycje LIVE z CG wyróżnione.
- **M2 – Na antenie wg makiety**: ramka VizRS w karcie kanału (przy antenach kanałów), panel „Uwagi” (brak playoutu, błąd budowy, przerwa w ramówce, CG Playout, studio wstrzymane, slot na żywo bez sygnału), audycje LIVE z CG w „Najbliższych zdarzeniach”; odświeżanie co 30 s bez przebudowy kart, więc grafika VizRS się nie przeładowuje.
- **Testy bez lokalnego stosu testowego**: `tests/deploy.sh` – szybkie testy (Rust, składnia JS), obraz panelu, podmiana tylko usługi `live` w stosie CG Playout, `tests/prod-smoke.cjs` (wszystkie ekrany, tylko odczyt – bezpiecznik przerywa każdy zapis), przy błędzie powrót do poprzedniego obrazu; ok. 20 s przy obrazie w pamięci podręcznej. Pełna bramka z e2e zostaje w CI. `scripts/plan.sh`: punkty dowolnej litery (plan M), kontrola `PLAN_CHECK` (domyślnie `tests/deploy.sh`).
- **M1 – pasek górny i menu wg makiety**: liczniki w menu (kanały, profile, znaki, biblioteki, kolekcje, playlisty, wypełniacze, harmonogramy, bloki, playouty); wybór kanału w pasku górnym – filtr Na antenie, EPG, Playoutów i Slotów na żywo (pamiętany w przeglądarce); wskaźnik CG Playout pokazuje „połączony” zamiast „studio: ramówka”.
- **Plan „panel na wzór makiety”** (`docs/plany/PLAN-makieta.md`, M0–M9). M0: okno (karta pozycji, potwierdzenie) zamyka się przy zmianie ekranu – wcześniej zostawało otwarte nad kolejnymi ekranami.
- **Harmonogramy wg makiety UI**: tabela pozycji ze startem „06:00 stała” / „po poprzedniej”, kolumnami wypełniaczy oraz znaków wodnych i elementów graficznych; nowy panel „Podgląd 24 h” – szkielet doby z godzin stałych (dokładne godziny liczy legacy przy budowie playoutu).

### 1.1.0 – wydanie (2026-11-07)

- Wersja panelu i mostu 1.1.0 (obrazy `ghcr.io/sdorpl/vizrs-live:1.1.0`, `ghcr.io/sdorpl/vizrs-live-legacy:1.1.0-legacy26.12.0`); most bez zmian w działaniu.
- **Podgląd strumienia kanału w panelu**: okno podglądu (przycisk „Podgląd” w pasku górnym, „Podgląd ▶” na kartach Na antenie, „Odtwórz w panelu” w Diagnostyce), które zostaje przy zmianie ekranów. Własny lekki odtwarzacz bez bibliotek (`player.js`, `mux.js`: MPEG-TS → fragmenty MP4 dla MSE, bez kodowania) na tej samej sesji HLS legacy, co widzowie – bez dodatkowego obciążenia serwera; obraz równany do zegara anteny (`PROGRAM-DATE-TIME`), czyli opóźnienie ≈ 0 względem ramówki. Nowy pośrednik `GET /api/stream/…` (biała lista, bez tokenu mostu, przepisane adresy playlist), CSP `media-src 'self' blob:`. HEVC/AV1 – natywny HLS przeglądarki.
- **Stos testowy oszczędniejszy w RAM**: dane domyślnie w `~/.cache/vizrs-live-test` zamiast `/tmp` (tmpfs), legacy z workstation GC i limitem `VIZRS_LIVE_TEST_MEM` (1536m – e2e ogląda kanał w podglądzie), proxy 128 MB, Jellyfin/Emby/Plex w `tests/real.sh` po `VIZRS_LIVE_REAL_MEM` (1g); `tests/gate.sh` po zielonym przebiegu zatrzymuje stos, który sam uruchomił (`GATE_KEEP_STACK=1`); `tests/stack.sh mem` pokazuje zużycie. Legacy w stosie: ok. 850 → 450 MB.
- **Dokumentacja na wzór VizRS CG System**: nowe `docs/STATUS.md` (etap, następne zadanie, kroki ręczne), `docs/LUKI.md` (znane braki G1 → G2 → G3), `docs/README.md` (spis), `docs/plany/` (plan rozbudowy przeniesiony do `docs/plany/PLAN-rozbudowy.md`, `scripts/plan.sh` za nim); `CLAUDE.md` przebudowany (start sesji, niepodważalne zasady, porty stosu testowego, struktura, commity), nowy `GEMINI.md` dla Gemini CLI (wczytuje `CLAUDE.md`); spis treści w DOKUMENTACJA, tabela dokumentów w README.
- **VizRS Live Panel wchłonięty i usunięty** (CG Playout `/live` → VizRS Live; strona `/live`, logowanie i `etv_public` usunięte z CG Playout – zostaje tylko jego API `/api/live/*`): Sloty na żywo jako stanowisko MCR – lampki CG/ErsatzTV/odbioru, tally, licznik sygnału studia wg zegara CG, ostrzeżenie i odliczanie, gdy audycja LIVE trwa albo zbliża się bez studia, ZDEJMIJ dwoma kliknięciami, ramówka EPG z oznaczeniem LIVE, zdarzenia; stan co 1 s. Nowy ekran **Audycje LIVE**: stan plik/biblioteka/kolekcja, najbliższa emisja, kreator po zapisie (kroki CG + pozycja w Harmonogramach, przebudowa), wybór biblioteki Remote Streams z legacy (pośrednik `PUT /api/cg/live/settings`), przebudowa ramówki per kanał. Pasek górny: „studio na antenie” i nazwa stanowiska (`X-Operator` → dziennik CG jako „VizRS Live · nazwa”).

### 1.0.0 – wydanie (2026-11-07)

- **Edytor reguł grafiki** w Grafice kanału: reguły globalne (start pozycji, przed końcem audycji i ile sekund, przerwa reklamowa, rozpoznawanie reklam), reguły audycji i kanałów; zapis przez nowe API VizRS CG Playout `GET/PUT /api/live/graphics` (walidacja w CG, zmiana od następnej pozycji bez restartu). Pośrednik `/api/cg/live/graphics` (GET, PUT).
- **Sloty na żywo – jak nadawać**: przy każdym kanale serwer i klucz RTMP (jak w OBS) oraz adres SRT z hasłem, z „Pokaż” i „Kopiuj”, i krótki opis, kiedy studio przejmuje kanał. Dane z nowego pola `inputs` w `/api/live/state` VizRS CG Playout (`VIZRS_INGEST`, `VIZRS_INGEST_PUBLIC`); bez wejścia – podpowiedź, co ustawić.
- `tests/real.sh`: prawdziwe Jellyfin, Emby, Plex i VizRS CG Playout obok stosu testowego (Jellyfin i Emby konfigurowane automatycznie, `connect` łączy z nimi legacy) – ręczne sprawdzenie tego, co e2e testuje na atrapach.
- e2e stabilniejsze: „+ Dodaj zamianę” (R8), wgrywanie obrazu znaku wodnego (R5) i usuwanie deco (R6) czekają na przerysowanie widoku.

- **Dokumenty**: `docs/INSTRUKCJA.md` (operator: codzienna praca, przebudowa ramówki, awarie) i `docs/RUNBOOK.md` (wdrożenie, kopie `etv-config`, aktualizacja panelu i legacy, odtworzenie, awarie, wydanie).
- **Obrazy** `ghcr.io/sdorpl/vizrs-live:1.0.0` i `ghcr.io/sdorpl/vizrs-live-legacy:1.0.0-legacy26.12.0` (workflow `release.yml` po tagu `v*`; publikacja – krok ręczny, DOKUMENTACJA §9). `compose.yml` używa ich domyślnie (`docker compose pull`), budowa na miejscu dalej działa.
- **Wersje przypięte**: `.env.example` – `LIVE_IMAGE`, `LIVE_LEGACY_IMAGE`, `LEGACY_IMAGE=ghcr.io/ersatztv/legacy:v26.12.0`, `PROXY_IMAGE=caddy:2.10.2-alpine`; domyślne legacy w `legacy-bridge.Dockerfile` też v26.12.0.
- **Bezpieczeństwo** (przegląd, DOKUMENTACJA §9): limit nieudanych logowań (10/min, potem 429) z wpisem w dzienniku; CSP na wszystkich odpowiedziach (też API i `--web`) z `object-src 'none'`, `base-uri 'none'`, `form-action 'self'`, `frame-ancestors 'none'`; ciasteczko `Secure` za proxy TLS (`X-Forwarded-Proto: https`).
- Wersja panelu i mostu 1.0.0.
- e2e: osobny panel z tokenem – nagłówki CSP, 401 bez sesji, CSRF, logowanie w UI (zły i dobry token, atrybuty ciasteczka), limit logowań; wersja 1.0.0 panelu i mostu.

- **Web GUI legacy zamknięte** (R10): usługa `proxy` (Caddy, `docker/proxy.Caddyfile`) w `compose.yml` wystawia na 8409 tylko `/iptv/*`, `/artwork/*` (GET/HEAD) i HDHomeRun; web GUI, `/api` legacy i `/vizrs-bridge/*` = 403. Legacy bez portu na zewnątrz (`expose`). `.env.example`: `PROXY_IMAGE`.
- **Diagnostyka**: „Web GUI legacy dla widzów” (serwer panelu sprawdza adres `--stream-url`: GUI i most zamknięte, M3U otwarte; nowe `GET /api/exposure`) i **lista kontrolna pokrycia** menu legacy → ekrany panelu (pominięcia z uzasadnieniem).
- Sprawdzone z ErsatzTV legacy **v26.12.0** (smoke, seed na świeżej bazie, e2e) – bez zmian w moście; procedura aktualizacji w DOKUMENTACJA §6 rozpisana.
- Stos testowy: proxy na 127.0.0.1:18410 (sieć `vizrs-live-test-net`), smoke proxy (403/200), smoke sprawdza `commandsMissing = []`, `LEGACY_IMAGE=… tests/stack.sh up` dla innej wersji legacy.
- **CI** (GitHub Actions): `ci.yml` – fmt/clippy/test, obraz panelu, obraz legacy z mostem, stos, seed, e2e (Playwright); `legacy-latest.yml` – co tydzień najnowsze legacy + smoke, seed, e2e.
- e2e: proxy (kody ścieżek, XMLTV przez proxy), ocena w Diagnostyce, pokrycie (każdy ekran legacy ma odpowiednik albo pominięcie, odnośniki do menu panelu).

- **Grafika kanału** (R9): stan VizRS CG Playout (sesje elementu VizRS, nakładka ffmpeg, live ingest, przechwytywanie grafiki per kanał), **strony VizRS kanału**, podgląd anteny `/output`, elementy graficzne legacy z „Wczytaj pliki elementów” (`RefreshGraphicsElements`). Reguł grafiki panel nie edytuje – CG Playout nie ma do nich API (krok ręczny, DOKUMENTACJA §4).
- **Sloty na żywo** (R9): sygnał studia per kanał, NA ANTENIE / WSTRZYMANY / RAMÓWKA, **ZDEJMIJ** (z potwierdzeniem) / **WPUŚĆ**, „studio także poza audycjami”, przebudowa ramówki; **audycje LIVE** (nowa, zmiana, usunięcie, skan) ze stanem kolekcji w legacy i najbliższą emisją; zdarzenia.
- Serwer: opcje `--cg-url`/`VIZRS_LIVE_CG_URL` i `--cg-token`/`VIZRS_LIVE_CG_TOKEN`, pośrednik `/api/cg/…` z białą listą adresów CG Playout (token CG zostaje w serwerze); `X-VizRS-Live` wymagany dla każdego żądania zmieniającego (też `PUT`, `DELETE`); `/api/status` → `cg`.
- e2e: atrapa CG Playout (stan, strony, ZDEJMIJ/WPUŚĆ, tryb kanału, audycja LIVE z błędem walidacji, usuwanie, token tylko w serwerze, adres spoza listy odrzucony).

- **Źródła zewnętrzne** (R8, nowy ekran Media → Źródła zewnętrzne): **Jellyfin** i **Emby** (adres, klucz API, połącz / rozłącz), **Plex** (logowanie kodem PIN na stronie Plex, inne konto, wyloguj, usuń serwer); dla każdego serwera wybór bibliotek do synchronizacji, synchronizacja biblioteki i kolekcji, odświeżenie listy bibliotek, **zamiany ścieżek**; zakładka **strumienie zdalne** (format pliku .yml, biblioteki tego rodzaju).
- **Sekrety tylko do zapisu**: serwer panelu maskuje w odpowiedziach komend pola `…apikey`, `…token`, `…password`, `…secret` (`********`); klucz API nie wraca do przeglądarki.
- Biała lista: komendy Jellyfin, Emby i Plex (DOKUMENTACJA §4); bez `Get…ConnectionParameters` i `TryCompletePlexPinFlow`.
- Poprawka: edytor ścieżek biblioteki pokazywał tekst „null” nad ścieżkami.
- e2e: połączenie z atrapą Jellyfin (błędny adres, błąd połączenia, maskowanie klucza, wybór biblioteki, zamiana ścieżki, rozłączenie), logowanie Plex PIN (adres podstawiony), strumienie zdalne; stabilniejszy przypadek „Zwolnij pamięć” (R7).

### 0.4.0 – bloki, szablony, deco, ustawienia legacy, logi i konserwacja (2026-11-07)

- **Ustawienia legacy w panelu** (R7), zakładki jak Settings legacy: **FFmpeg i strumienie** (ścieżki ffmpeg/ffprobe, profil nowych kanałów, preferowany język dźwięku, globalny znak wodny i wypełniacz zapasowy, napisy, przeplot, raporty, segmenter HLS, format HLS Direct, skrypt MPEG-TS), **playouty** (dni do zbudowania, pomijanie brakujących plików, limit skryptu), **przewodnik XMLTV** (dni, strefa, bloki), **skaner i HDHomeRun** (co ile godzin skan, liczba tunerów, UUID), **poziomy logów** (sześć obszarów, działa od razu), **web GUI legacy** (motyw, język). Zakresy sprawdzane w panelu (legacy ich nie sprawdza).
- **Logi** (nowy ekran System → Logi): najnowsze wpisy logu legacy, próg poziomu, wyszukiwanie, stronicowanie, odświeżanie co 5 s.
- **Diagnostyka**: „Sprawdź ponownie” kontrole legacy, **wersje i sprzęt** (legacy, ffmpeg, procesor, karty grafiki), „Odśwież możliwości ffmpeg”, **konserwacja** z potwierdzeniami (opróżnij kosz, osierocone grafiki i napisy, zwolnij pamięć), **test odtwarzania** (odnośniki HLS/TS do strumienia wybranego kanału).
- Biała lista: `GetFFmpegSettings`, `UpdateFFmpegSettings`, `GetMpegTsScripts`, `GetLoggingSettings`, `UpdateLoggingSettings`, `GetPlayoutSettings`, `UpdatePlayoutSettings`, `GetXmltvSettings`, `UpdateXmltvSettings`, `GetUiSettings`, `UpdateUiSettings`, `UpdateLibraryRefreshInterval`, `GetHDHRTunerCount`, `UpdateHDHRTunerCount`, `GetHDHRUUID`, `GetRecentLogEntries`, `DeleteOrphanedArtwork`, `DeleteOrphanedSubtitles`, `ReleaseMemory`, `RefreshFFmpegCapabilities`, `GetTroubleshootingInfo`. `RebuildSearchIndex` świadomie pominięta (legacy obsługuje ją tylko przy starcie).
- e2e: zmiana dni XMLTV i przywrócenie (z walidacją panelu i legacy), wczytanie wszystkich sekcji ustawień, poziom logów Debug → „Zwolnij pamięć” → wpis widoczny w Logach → przywrócenie; wersje legacy i odnośnik testu odtwarzania.
- **Bloki i szablony – pełna edycja** (R6), zakładki: **bloki** w grupach (nazwa, długość co 5 min, koniec bloku; pozycje: kolekcja / serial / sezon / inteligentna / wyszukiwanie, kolejność, EPG, znaki wodne, grafika; kopiuj, usuń; **podgląd wypełnienia bloku** na pasku i w tabeli) i **szablony dnia** w grupach (bloki o godzinach startu na **osi doby 24 h**, nakładanie zaznaczone na czerwono; kopiuj, usuń).
- **Wystrój (deco) – pełna edycja** (R6), zakładki: **deco** (znak wodny i grafika: dziedzicz / wyłącz / zastąp / scal; treść przerw – playlisty na starcie bloku; wypełniacz domyślny z przycinaniem; martwe powietrze) i **szablony deco** (deco w przedziałach godzin na osi doby).
- **Playout blokowy: kalendarz tygodnia** – szablony dni (z szablonem deco) przypisane do dni tygodnia/miesiąca, miesięcy i zakresu dat, deco domyślne, podgląd tygodnia (bloki i deco każdego dnia na osiach doby, nawigacja po tygodniach); „Przebuduj” także dla blokowego.
- Grupy (bloków, szablonów, deco, szablonów deco): „+ Grupa”, „Usuń grupę…” – wspólny `groupActions`; pola „kiedy” (dni, miesiące, zakres dat) wydzielone do `when.js` dla harmonogramów zastępczych i kalendarza.
- Biała lista: `GetAllBlockGroups`, `CreateBlockGroup`, `DeleteBlockGroup`, `GetBlocksByBlockGroupId`, `GetBlockById`, `CreateBlock`, `CopyBlock`, `DeleteBlock`, `GetBlockItems`, `ReplaceBlockItems`, `PreviewBlockPlayout`, `GetAllTemplateGroups`, `CreateTemplateGroup`, `DeleteTemplateGroup`, `GetTemplatesByTemplateGroupId`, `GetTemplateById`, `CreateTemplate`, `CopyTemplate`, `DeleteTemplate`, `GetTemplateItems`, `ReplaceTemplateItems`, `CreateDecoGroup`, `DeleteDecoGroup`, `GetDecosByDecoGroupId`, `GetDecoById`, `CreateDeco`, `UpdateDeco`, `DeleteDeco`, `GetAllDecoTemplateGroups`, `CreateDecoTemplateGroup`, `DeleteDecoTemplateGroup`, `GetDecoTemplatesByDecoTemplateGroupId`, `GetDecoTemplateById`, `CreateDecoTemplate`, `DeleteDecoTemplate`, `GetDecoTemplateItems`, `ReplaceDecoTemplateItems`, `GetPlayoutTemplates`, `ReplacePlayoutTemplateItems`, `GetDecoByPlayoutId`, `UpdateDefaultDeco`.
- e2e: blok 30 min z 3 pozycjami, podgląd, kopia; szablon dnia z dwoma blokami (nakładanie odrzucone przez legacy); deco z przerwą (playlista) i martwym powietrzem, ponowny zapis nie gubi treści; szablon deco; playout blokowy z kalendarzem na cały tydzień i deco domyślnym, przebudowa, pozycje w EPG; sprzątanie przez UI. Zrzuty nowych widoków: `E2E_SHOTS=<katalog>`.

### 0.3.0 – playouty, media, kolekcje i playlisty, profile i znaki wodne (2026-11-06)

- **Kolekcje – pełna edycja** (R4), zakładki: **zwykłe** (nazwa, lista pozycji wszystkich rodzajów, usuwanie zaznaczonych z kolekcji), **inteligentne** (zapytanie wyszukiwarki legacy, liczba wyników na liście i „Sprawdź zapytanie” przed zapisem), **wielokolekcje** (składniki: kolekcje zwykłe i inteligentne, „planuj jako grupę”), **powtórki** (treść przez wybór treści, kolejność premier i powtórek wg rodzaju treści).
- **Playlisty – pełna edycja** (R4): grupy (załóż, usuń z playlistami), playlisty w grupach, edytor pozycji (kolekcja / serial / sezon / artysta / wielokolekcja / inteligentna, kolejność, liczba albo „wszystkie”, EPG; przesuwanie, kopiowanie, usuwanie), **podgląd playoutu** („Zapisz i pokaż podgląd” – legacy liczy podgląd z zapisanej playlisty).
- **Przeglądarka mediów: „Dodaj zaznaczone do…”** kolekcji zwykłej albo playlisty.
- Poprawki: kliknięcie pola wyboru w wierszu tabeli nie otwiera już karty pozycji; edytor pozycji nie wstawia tekstu „null” po zamknięciu panelu bocznego.
- Biała lista: `GetCollectionById`, `GetCollectionCards`, `CreateCollection`, `UpdateCollection`, `DeleteCollection`, `AddItemsToCollection`, `RemoveItemsFromCollection`, `GetSmartCollectionById`, `UpdateSmartCollection`, `DeleteSmartCollection`, `GetMultiCollectionById`, `CreateMultiCollection`, `UpdateMultiCollection`, `DeleteMultiCollection`, `GetPagedRerunCollections`, `GetRerunCollectionById`, `SearchRerunCollections`, `CreateRerunCollection`, `UpdateRerunCollection`, `DeleteRerunCollection`, `GetPlaylistById`, `GetPlaylistItems`, `CreatePlaylistGroup`, `DeletePlaylistGroup`, `CreatePlaylist`, `DeletePlaylist`, `ReplacePlaylistItems`, `AddItemsToPlaylist`, `PreviewPlaylistPlayout`. Wybór treści „powtórki” działa (wcześniej brak `SearchRerunCollections`).
- e2e: kolekcja zwykła z 2 pozycjami z przeglądarki (i usunięcie jednej), inteligentna z zapytaniem i liczbą 2, wielokolekcja z dwoma składnikami, powtórki, playlista z 3 pozycjami, podglądem i zmianą kolejności; sprzątanie przez UI.
- **Profile kodowania – pełna edycja** (R5): nowy (wartości domyślne legacy z `NewFFmpegProfile`), kopiuj, usuń (z listą kanałów, które go używają); rozdzielczość, dopasowanie obrazu, kodek, głębia, bitrate i bufor, akceleracja sprzętowa z informacją, co zgłasza ffmpeg legacy (`GetSupportedHardwareAccelerationKinds`; niezgłoszona oznaczona i z ostrzeżeniem), sterownik i urządzenie VAAPI, HDR → SDR, stała liczba klatek, usuwanie przeplotu; dźwięk z normalizacją głośności i podpowiedzią −23 LUFS (EBU R128). Pola tylko starego silnika legacy zostają bez zmian.
- **Znaki wodne – pełna edycja** (R5): wgrywanie obrazu (PNG/JPEG/GIF/WebP do 10 MB) albo logo kanału, tryb stały / przerywany (co ile minut, na ile sekund) / wyrażenie krycia, 9 położeń w siatce kadru, rozmiar, marginesy, krycie, warstwa; **podgląd na makiecie kadru 16:9**; miniatury na liście; kopiuj, usuń.
- **Wypełniacze – pełna edycja** (R5): miejsce (pre/mid/post-roll, ogon, zapasowy; po założeniu zablokowane jak w legacy), tryb (czas, liczba, losowa liczba, dopełnienie do 5/10/15/30 min), treść przez wybór treści, znaki wodne na wypełniaczu, wyrażenie przerw (mid-roll), rozdziały jako pozycje; pola spoza trybu czyszczone jak w legacy.
- **Logo kanału**: wgrywanie obrazu w edytorze kanału (albo adres URL), podgląd.
- **Most: `POST /vizrs-bridge/upload/{Logo|Watermark}`** – jedyny wyjątek od ogólnego dyspozytora (`SaveArtworkToDisk` przyjmuje `Stream`): tylko obrazy PNG/JPEG/GIF/WebP (typ i sygnatura pliku), do 10 MB, wymaga `SaveArtworkToDisk` na białej liście. Panel: `POST /api/upload/{Logo|Watermark}` z tymi samymi ograniczeniami (limit treści 10 MB tylko dla tej ścieżki).
- Biała lista: `GetFFmpegProfileById`, `NewFFmpegProfile`, `CreateFFmpegProfile`, `UpdateFFmpegProfile`, `CopyFFmpegProfile`, `DeleteFFmpegProfile`, `GetSupportedHardwareAccelerationKinds`, `GetWatermarkById`, `CreateWatermark`, `UpdateWatermark`, `CopyWatermark`, `DeleteWatermark`, `SaveArtworkToDisk`, `GetFillerPresetById`, `CreateFillerPreset`, `UpdateFillerPreset`, `DeleteFillerPreset`.
- e2e: profil 720p z −23 LUFS, kopia, usunięcie; znak wodny z wgranym PNG (podgląd na makiecie) przypisany do kanału; logo kanału wgrane; odrzucenie SVG, niezgodnej treści, pliku > 10 MB i złego rodzaju; wypełniacz „dopełnij do 30 min”.
- **Media** (R3): **Biblioteki** – biblioteki lokalne z rodzajem, nazwą i ścieżkami w kontenerze legacy (dodaj, usuń z potwierdzeniem, przenieś do innej biblioteki), liczba pozycji biblioteki i ścieżki, „Skanuj teraz” (też po założeniu) z odświeżaniem liczby pozycji, usuwanie z potwierdzeniem; **Przeglądarka** – zakładki rodzajów z liczbami, wyszukiwarka legacy z podpowiedzią pól, stronicowanie, karta pozycji (strumienie, rozdziały, czas, obraz); **Kosz** – pozycje bez pliku, usuwanie zaznaczonych z bazy, „Opróżnij kosz”.
- Biała lista: `GetLocalLibraryById`, `GetLocalLibraryPaths`, `UpdateLocalLibrary`, `MoveLocalLibraryPath`, `DeleteLocalLibrary`, `CountMediaItemsByLibrary`, `CountMediaItemsByLibraryPath`, `QueueShowScanByLibraryId`, `GetLibraryRefreshInterval`, `QuerySearchIndex*`, `QuerySearchTargets`, `GetMediaItemInfo`, `GetMovieById`, `GetTelevisionShowById`, `GetTelevisionSeasonById`, `GetRemoteStreamById`, `GetImageFolders`, `UpdateImageFolderDuration`, `EmptyTrash`, `DeleteItemsFromDatabase`.
- e2e: biblioteka z seed (3 pozycje, ścieżka), nowa biblioteka (walidacja ścieżki, zapis, usunięcie), wyszukiwanie „Pogoda”, karta pozycji z H.264, pusty kosz.
- **Playouty – pełna obsługa** (R2): „+ Nowy playout” z wyborem rodzaju (klasyczny, blokowy, sekwencyjny YAML, skryptowy, zewnętrzny JSON) i kanału (tylko kanały bez playoutu), harmonogram albo ścieżka pliku w kontenerze legacy z podpowiedzią; edycja: harmonogram główny, ścieżka pliku, codzienna przebudowa o godzinie; **harmonogramy zastępcze** (dni tygodnia ze skrótami robocze/weekend, dni miesiąca, miesiące, zakres dat, kolejność); stan ostatniej budowy z komunikatem legacy; przebudowa, od nowa, wyczyszczenie pozycji (i historii), sprawdzenie nakładania; „Resetuj wszystkie” z potwierdzeniem; usuwanie z potwierdzeniem; najbliższe pozycje pod edytorem.
- Pasek górny: licznik playoutów z błędem budowy (`GetPlayoutWarningsCount`), odnośnik do Playoutów.
- Decyzja: panel nie edytuje plików YAML/skryptów/JSON playoutów – pliki na wolumenie konfiguracji legacy (DOKUMENTACJA §4).
- `form.js`: podpowiedź pola może zależeć od wartości; ukryta grupa pól ukrywa też swoje pola przy odczycie. `crudView`: `createCmd` (komenda zakładania zależna od danych), `below` może zwrócić kilka paneli.
- Poprawka układu: długie widoki nie ściskają już paneli (obcięty edytor pozycji, gdy widok był wyższy niż ekran).
- Biała lista: `CreateBlockPlayout`, `CreateSequentialPlayout`, `CreateScriptedPlayout`, `CreateExternalJsonPlayout`, `UpdatePlayout`, `UpdateSequentialPlayout`, `UpdateScriptedPlayout`, `UpdateExternalJsonPlayout`, `DeletePlayout`, `ResetAllPlayouts`, `ErasePlayoutItems`, `ErasePlayoutHistory`, `CheckForOverlappingPlayoutItems`, `GetPlayoutWarningsCount`, `GetPlayoutAlternateSchedules`, `ReplacePlayoutAlternateScheduleItems`. Smoke mostu sprawdza odrzucenie komendy spoza listy na `SaveConfigElementByKey` (wcześniej `ResetAllPlayouts`, teraz dozwolona).
- e2e: playout klasyczny dla nowego kanału i harmonogramu, wybór tylko wolnych kanałów, błąd legacy dla brakującego pliku YAML, przebudowa i pozycje w „Na antenie”, harmonogram zastępczy na sobotę (walidacja dni), codzienna przebudowa, wyczyszczenie, „Resetuj wszystkie”, usunięcie.

### 0.2.0 – harmonogramy (2026-11-06)

- **Harmonogramy – pełna edycja** (R1): lista z kanałami, które używają harmonogramu; ustawienia (godziny stałe, tasowanie, losowy start, części odcinków, kolekcje jak seriale); nowy, kopiuj, usuń z potwierdzeniem (wymienia kanały, których playouty legacy usunie kaskadowo); **edytor pozycji**: start dynamiczny/o stałej godzinie, treść (wybór treści), kolejność odtwarzania zależna od rodzaju treści, maraton, tryb (wypełnij / jedna / kilka + liczba albo tryb / przez czas + reszta czasu + próby dopasowania), wypełnianie grupami, tytuł własny, tryb przewodnika, wypełniacze pre/mid/post/ogon/zapasowy, znaki wodne i elementy graficzne, preferencje audio i napisów; kopiowanie pozycji. Ustawienia i pozycje zapisuje jeden „Zapisz” (pozycje jednym `ReplaceProgramScheduleItems`); błąd legacy pokazany z polskim wstępem.
- **Most: abstrakcyjne typy legacy serializowane po typie rzeczywistym** – wcześniej pozycje harmonogramu (`GetProgramScheduleItems`) traciły pola podtypów (liczba trybu „kilka”, czas, reszta czasu, próby dopasowania), więc zapis z panelu skasowałby je w legacy.
- `crudView`: `loadOne` (dane zaznaczonej pozycji), `below` (panel pod edytorem), `beforeSave`/`afterSave`; `itemsEditor`: `commit()` (zatwierdza otwarty panel boczny przed zapisem) i kopiowanie wiersza (`copy`).
- Biała lista: `GetProgramScheduleById`, `UpdateProgramSchedule`, `CopyProgramSchedule`, `DeleteProgramSchedule`, `GetProgramScheduleItems`.
- e2e: harmonogram z dwiema pozycjami (06:00 „wypełnij” + dynamiczna „kilka”), zmiana kolejności, zapis, ponowne otwarcie, błąd walidacji legacy, kopia, usunięcie.
- **UI w modułach ES** (R0): `core.js`, `form.js`, `parts.js`, `views/<grupa menu>.js` zamiast jednego `app.js`; test serwera, że każdy plik ze `STATIC` jest podawany i każdy `import` wskazuje plik ze `STATIC`.
- **Formularz z metadanych komend** (`form.js`): kontrolki z typów parametrów mostu (tekst, liczba, tak/nie, wyliczenie, `TimeSpan`, data/godzina, wybór id, lista id), sekcje, pola warunkowe, opis złego pola.
- **Wspólne części ekranów** (`parts.js`): „lista + edytor” (Kanały przepisane na ten wzorzec bez zmiany zachowania; „Kanał lustrzany” widoczny tylko dla źródła Mirror), edytor listy pozycji, wybór treści z wyszukiwaniem w legacy (w Diagnostyce: pola treści do konsoli komend).
- Ekrany, którym brakuje komend na białej liście mostu, mówią, której komendy brakuje, zamiast pokazywać pusty ekran.
- Biała lista: `GetPlaylistsByPlaylistGroupId`, `SearchCollections`, `SearchSmartCollections`, `SearchMultiCollections`, `SearchTelevisionShows`, `SearchTelevisionSeasons`, `SearchArtists`.
- Testy: `tests/gate.sh` (cała bramka jednym poleceniem), `tests/stack.sh test` (e2e, Playwright z `tests/local.env`) i `reset`; `seed` nie dubluje danych przy ponownym uruchomieniu.
- `scripts/plan.sh`: automat planu rozbudowy – punkt po punkcie, każdy w nowej sesji Claude Code bez operatora, z niezależną kontrolą bramką.
- e2e: pole warunkowe i wyliczenia formularza, wybór treści (wynik, brak wyników, brakująca komenda, zapytanie), edytor listy pozycji; każda odpowiedź HTTP ≥ 400 w trakcie testu jest błędem.

### 0.1.0 – prototyp (2026-11-06)

- **VizRS Bridge** (C#): most REST w procesie niezmienionego ErsatzTV legacy (hak startowy + hosting startup), ogólny dyspozytor komend MediatR z białą listą, zadania w tle przez kolejkę legacy, wyniki `Either`/`Option` jako `{ok, result | error}`.
- **vizrs-live** (Rust, bez zależności): UI panelu, token panelu z ciasteczkiem, ochrona CSRF nagłówkiem, pośrednik do mostu, `/health` i `--healthcheck`.
- **Panel**: Na antenie, EPG, Kanały (pełna edycja), Playouty (przebudowa), widoki odczytu dla profili, znaków wodnych, bibliotek, kolekcji, playlist, wypełniaczy, harmonogramów, bloków, deco i elementów graficznych, Ustawienia, Diagnostyka z konsolą komend.
- Obrazy Docker (`vizrs-live-legacy`, `vizrs-live`), `compose.yml`, stos testowy `tests/stack.sh` (smoke + ramówka przez panel), e2e `tests/e2e.cjs`.
- Sprawdzone z ErsatzTV legacy v26.11.6: kanał zbudowany przez panel nadaje przez Next (HLS, H.264 1080p + AAC).
