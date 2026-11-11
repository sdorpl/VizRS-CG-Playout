# Lokalny stos emisyjny na Quadro P400

Plan z 2026-11-11. Cel: kanał 1080p50 z grafiką (`area: auto`, renderer
natywny) i live ingest działa na lokalnej stacji stabilnie, także przy zmianie
pozycji i z otwartym pulpitem, z zapasem na drugi kanał. Kod ErsatzTV bez
zmian, a nowe zachowanie nakładki tylko jako jawna opcja `VIZRS_FFMPEG_*`
(CLAUDE.md, „Niepodważalne zasady”).

## Stan wyjściowy (pomiar 2026-11-11)

Sprzęt: Xeon E3-1270 v3 (4 rdzenie / 8 wątków, **bez iGPU**), 16 GB RAM,
Quadro P400 (Pascal GP107: 2 GB GDDR5, 1 NVENC 6. gen., 1 NVDEC, sterownik
580.178.04, persistence mode włączony). P400 to **jedyna karta graficzna**,
więc obsługuje też pulpit KDE (Wayland).

Stos `virtrs`: `docker-compose.yml` + `docker-compose.nvidia.yml` + VizRS
Live, `.env`: `VIZRS_FFMPEG_FPS=50`, `VIZRS_AREA=auto`,
`VIZRS_FFMPEG_CUDA=overlay`, `VIZRS_FFMPEG_NVENC_BF=0`,
`VIZRS_BROWSER_GPU=1`, profil `ingest`.

| Zasób | Użycie | Uwagi |
|---|---|---|
| vRAM razem | 1495 / 2048 MiB | zostaje ok. 550 MiB |
| – pulpit i aplikacje | ok. 1210 MiB | plasmashell 181, Firefox 213, Electron ok. 390, maliit-keyboard 141, VLC 113, monitor systemu 77, kwin 56 |
| – ffmpeg kanału | 137 MiB | `overlay_cuda`, `-readrate 1`, `-bf:v 0`, 3 strefy |
| – Chromium sidecara | ok. 67 MiB (szac.) | `VIZRS_BROWSER_GPU=1`; `nvidia-smi` go nie wypisuje |
| NVENC | 15% | jeden kanał 1080p50 6 Mb/s, preset domyślny (`p4`) |
| NVDEC | 0% | dekodowanie materiału na CPU (tryb `overlay` zdejmuje `-hwaccel`) |
| CPU | ersatztv 62%, cg 38% rdzenia | ffmpeg 0,6 rdzenia (dekoder `-threads 1`, `loudnorm`) |
| RAM | 4 GB dostępne | ersatztv 1,3 GB |
| PCIe | gen 2 ×16 w P0 | P400 i C226 mogą gen 3 – do sprawdzenia |

Wnioski:

1. **Wąskim gardłem jest pamięć karty, nie moc.** NVENC ma 85% zapasu, a CPU
   ponad połowę. Stos zajmuje ok. 200 MiB, pulpit sześć razy więcej.
2. Przy zmianie pozycji dwa procesy kanału działają chwilę naraz (2 × ok.
   160 MiB), a nadrabianie bez `-readrate` (387 MiB) jest już zablokowane.
   Obecny zapas (ok. 550 MiB) wystarcza na jeden kanał. Drugi kanał albo nowa
   karta w przeglądarce może go wyczerpać → `CUDA_ERROR_OUT_OF_MEMORY` →
   plansza zastępcza.
3. Ścieżka klatek: CPU dekoduje → `hwupload` → `scale_cuda` + `overlay_cuda`
   → `hwdownload` → NVENC (wysyła z powrotem). Podróż w obie strony to ok.
   2 × 155 MB/s przy 1080p50, czyli tanio. `hwdownload` musi zostać, bo kolejka
   przed muxerem (`loudnorm`, ok. 3 s) inaczej trzyma klatki na karcie.

## Stan etapów (2026-11-11)

| Etap | Stan |
|---|---|
| E0 pomiar | doraźne pomiary w tym planie; skrypt `bench/` – nie zrobiony |
| E1 pulpit | odrzucony (decyzja użytkownika) |
| E2 `.env` | **wdrożony lokalnie**: `VIZRS_BROWSER_GPU=0` (karta zajęta 1495 → 1270 MiB, razem z wahaniami pulpitu) |
| E3 jakość NVENC | odłożony – poprawia jakość, nie wydajność karty |
| E4 `VIZRS_FFMPEG_CUDA=auto` | **zrobiony** (`src/overlay.rs`, test jednostkowy i kontraktu, DOKUMENTACJA §5) i wdrożony lokalnie; kawałek przy 715 MiB wolnych = `overlay` |
| E5 NVDEC | zmierzony, **odrzucony** (niżej) |
| E6 drugi kanał | nie zrobiony |
| E7, E8 | pominięte (decyzja użytkownika) |
| Lagi 1080p50 | **naprawione**: nie karta (SM 30–40%, głównie pulpit; NVENC 12%), tylko `-readrate 1` trybu `overlay` bez zrywu – HLS 0,93×. Zryw = grafika gotowa w sidecarze (`ahead` w `/ffmpeg`): HLS 1,04× do dogonienia ramówki, ffmpeg 167 → 277 MiB, `VIZRS_FFMPEG_CUDA_MIN_FREE` 300 → 400 (DOKUMENTACJA, „Pamięć karty przy NVENC”) |

## Etapy

Kolejność według stosunku zysku do ryzyka. Każdy etap kończy się pomiarem
w tej samej procedurze (E0), wpisem do DOKUMENTACJI (§4 NVIDIA, „Pamięć karty
przy NVENC”) i – gdy zmienia kod – testem.

### E0. Procedura pomiaru (bez zmian w kodzie)

Skrypt w `bench/` (np. `bench/gpu-p400.sh`, tylko `nvidia-smi`, `docker
stats`, `ffprobe`), który przez 10 min anteny zapisuje co sekundę:

- vRAM: łącznie, na proces (`--query-compute-apps`), **szczyt przy zmianie
  pozycji** (dwa procesy naraz) i przy przejęciu przez live (`takeover`),
- `utilization.encoder/decoder/gpu`, zegary, P-state, generację PCIe,
- CPU kontenerów `ersatztv`, `cg`, `ingest`,
- tempo kawałków (czas kawałka 44 s vs czas ścienny) i opóźnienie HLS.

Do tego test pojemności NVENC: N równoległych `testsrc2` 1080p50 →
`h264_nvenc -bf 0` (N = 1…8), żeby zmierzyć realny limit sesji i vRAM na
sesję. Dla Quadro limitu sesji ma nie być, ale potwierdzamy to na tym
sterowniku.

Kryterium „stabilnie”: 0 × `OUT_OF_MEMORY` i 0 × plansza zastępcza w 24 h
oraz zapas vRAM ≥ 300 MiB w szczycie zmiany pozycji.

### E1. ~~Odchudzenie pulpitu~~ – odrzucone

Decyzja użytkownika 2026-11-11: pulpitu nie ruszamy. Pulpit zostaje więc
zmienną, na którą stos nie ma wpływu (od ok. 1,2 GB w górę), a cały zysk musi
przyjść ze stosu (E2–E5) i z bezpiecznika (E4). Dlatego E4 ma pierwszeństwo.
Wariant sprzętowy, który rozwiązuje to bez zmian na pulpicie, opisuje E8.

### E2. Ustawienia stosu (tylko `.env`)

- `VIZRS_BROWSER_GPU=0`: renderer natywny (`auto`) i tak rysuje na CPU, a
  Chromium na GPU pomaga tylko scenom rysowanym przez przeglądarkę (BTB).
  Zysk ok. 67 MiB. Włączać tylko pod show z takimi scenami.
- Zostają: `VIZRS_FFMPEG_CUDA=overlay`, `VIZRS_FFMPEG_NVENC_BF=0`,
  `VIZRS_AREA=auto`.
- `DOTNET_gcServer=0` i limit sterty dla ErsatzTV (jak w
  `docker-compose.ampere.yml`): mniej RAM, bez wpływu na kartę. Sprawdzić,
  czy zapisać to w nakładce `docker-compose.nvidia.yml`.
- PCIe gen 2: sprawdzić `nvidia-smi -q` pod obciążeniem. Gen 2 bez zmian
  wystarcza (8 GB/s wobec ok. 0,3 GB/s potrzebnych), więc tylko notatka.

### E3. Jakość NVENC przy tym samym bitrate (opcja nakładki)

NVENC ma zapas, a P400 nie ma klatek B w HEVC, więc zostajemy przy H.264
i podnosimy jakość tanimi narzędziami Pascala. Pamięć karty już zmierzona
(2026-11-11, sam koder: `testsrc2` 1080p50, 6 Mb/s, `-bf:v 0`, ffmpeg 8.1.2
z obrazu ErsatzTV). Do zmierzenia zostaje jakość (SSIM/VMAF na 3 materiałach
przy tym samym bitrate):

| Opcja | vRAM kodera | Uwagi |
|---|---|---|
| bez opcji (`p4`) | 79 MiB | baza |
| `-preset p1` / `p5` | 79 / 79 MiB | `p5` za darmo w pamięci – pierwszy kandydat |
| `-spatial-aq 1` | 85 MiB | lepsze płaskie tła i gradienty grafiki; PSNR zwykle spada, liczy się ocena wzrokowa / VMAF |
| `-temporal-aq 1` | 85 MiB | **P400 przyjmuje** (bez ostrzeżenia), choć HandBrake podaje „od Turinga” – sprawdzić, czy realnie działa |
| `-preset p6` (też z `-multipass disabled`) | 154 MiB | p6/p7 same włączają bufor wyprzedzenia – **odpada** |
| `-rc-lookahead 16` | 154 MiB | odpada (+75 MiB na proces, ×2 przy zmianie pozycji) |
| `-preset p7 -multipass fullres` | 203 MiB | odpada |

Wdrożenie, gdy pomiar pokaże zysk: jedna opcja `VIZRS_FFMPEG_NVENC_OPTS`
(dopisywana po nazwie kodera `*_nvenc` tak jak `-bf:v`, tylko bez opcji już
podanych przez ErsatzTV). Pełny zestaw: `src/overlay.rs`, `src/config.rs`
(jeśli dotyczy), `docker-compose.yml`, DOKUMENTACJA §5, test kontraktu.

### E4. Bezpiecznik pamięci karty: `VIZRS_FFMPEG_CUDA=auto`

Teraz tryb wybiera się na stałe. Przy pełnej karcie (pulpit, drugi kanał)
`overlay` kończy się planszą zastępczą, a `encode` (ok. 120 MiB, +1 rdzeń
CPU) by przeszedł. Propozycja:

- `auto` = przed transkodowaniem nakładka czyta wolną pamięć karty
  (`nvidia-smi --query-gpu=memory.free`, jest w kontenerze dzięki
  `NVIDIA_DRIVER_CAPABILITIES=utility`; limit czasu 1 s): wolne ≥ próg
  (`VIZRS_FFMPEG_CUDA_MIN_FREE`, domyślnie np. 400 MiB) → `overlay`,
  mniej → `encode`. Brak `nvidia-smi`, błąd albo przekroczony czas →
  `overlay` (dzisiejsze zachowanie). Wybór trafia do logu.
- Ostatnia linia obrony, osobno do rozważenia: gdy przebieg `overlay` padnie
  w pierwszych sekundach z `CUDA_ERROR_OUT_OF_MEMORY`, nakładka powtarza go
  raz w trybie `encode`, zanim ErsatzTV pokaże planszę. Wymaga ostrożności
  z HLS (append do tej samej playlisty, jak w `takeover.rs`).

Test kontraktu: atrapa `nvidia-smi` w `PATH` z zadaną wolną pamięcią → oba
tryby i wariant błędu.

### E5. Dekodowanie ciężkich materiałów na NVDEC (opcja, po pomiarze)

ErsatzTV daje dekoderowi `-threads 1`. Materiał H.264 1080p to dla Haswella
na jednym wątku za mało, żeby nadrabiać, a HEVC 1080p50/4K może nie
utrzymać 1×. NVDEC stoi bezczynny. Pomiar: tempo i CPU dla HEVC 1080p, H.264
1080p60 o wysokim bitrate, 4K. Jeśli któryś nie trzyma 1×:

- `overlay` zostawia `-hwaccel cuda` (bez `-hwaccel_output_format cuda`,
  albo z nim i ograniczeniem `-extra_hw_frames`) dla kodeków i rozdzielczości
  z listy (np. `hevc`, > 1080p), rozpoznanych przez `ffprobe` tak jak
  w `VIZRS_FFMPEG_FPS_BLEND`. Koszt vRAM dekodera (pule ramek) zmierzyć –
  dlatego tylko dla materiałów, które tego potrzebują. Pula NVDEC w ffmpeg
  to DPB + `extra_hw_frames` + liczba wątków, z nadmiarem zgłoszonym w trac
  #9285 (otwarte). `-threads 1` ErsatzTV działa tu na korzyść.
- **AV1:** NVDEC Pascala nie dekoduje AV1, więc materiał AV1 zawsze dekoduje
  dav1d na CPU, w dodatku z `-threads 1` ErsatzTV. Zmierzyć 1080p50 AV1. Jeśli
  nie trzyma 1×, opcja nakładki podnosząca liczbę wątków tylko dla dekodera
  AV1 albo zalecenie transkodowania biblioteki do H.264.

### E7. Utrzymanie: sterownik 580 i kernele CUDA ffmpeg

- Gałąź **580 to ostatnia ze wsparciem Pascala** (także Maxwella i Volty).
  590 nie rozpozna już P400, a 580 dostaje poprawki bezpieczeństwa do 2028.
  Zablokować pakiety sterownika na gałęzi 580 (`apt-mark hold` / pin
  `nvidia-driver-580`), żeby aktualizacja systemu nie zabrała karty
  stosowi i pulpitowi naraz. Zapis w DOKUMENTACJI §4 NVIDIA.
- Filtry `*_cuda` w ffmpeg ErsatzTV (8.1.2-etv.8) są kompilowane przez
  `--enable-cuda-llvm` (clang → PTX). CUDA 13 nie kompiluje już kodu dla
  architektur starszych niż 7.5. Jeśli przyszły obraz ErsatzTV podniesie cel
  PTX, `scale_cuda`/`overlay_cuda` przestaną działać na P400, a NVENC
  zostanie. Po każdej aktualizacji obrazu ErsatzTV uruchomić test dymny
  (`testsrc2` → `hwupload` → `overlay_cuda` → `h264_nvenc`). Awaryjnie
  `VIZRS_FFMPEG_CUDA=encode` usuwa wszystkie filtry CUDA i zostawia tylko
  NVENC – to gotowe obejście.

### E8. Wariant sprzętowy (decyzja użytkownika, gdy E2–E4 nie wystarczą)

Pulpit zostaje na P400 bez zmian, a stos dostaje własną kartę (`NVIDIA_GPU`
albo `docker-compose.vaapi.yml`), o ile jest wolne gniazdo PCIe:

| Karta | Za | Przeciw |
|---|---|---|
| Intel Arc A310 (4 GB, low profile, bez wtyczki zasilania) | AV1/HEVC/H.264, dekodowanie AV1; stos ma już ścieżkę VAAPI | inny sterownik (i915/xe); nakładanie grafiki na CPU w ścieżce VAAPI (§4) |
| NVIDIA T400 4 GB / T1000 (TU117) | ten sam stos co dziś, wsparcie sterowników po 580 | NVENC 5. gen. – jakość jak Pascal, bez HEVC B |
| RTX A2000 | NVENC Turing/Ampere (lepsza jakość, HEVC B), 6–12 GB | cena, 70 W |

Druga P400 nic nie daje na przyszłość (koniec sterowników po 580).

**Pomiar E5 (2026-11-11, materiał z szumem, ffmpeg 8.1.2 z obrazu ErsatzTV,
tempo dekodowania bez kodera):**

| Materiał | CPU `-threads 1` | NVDEC |
|---|---|---|
| H.264 1080p50 20 Mb/s | 1,72× | 6,6× |
| HEVC 1080p50 10 Mb/s | 1,43× | 7,9× |
| AV1 1080p50 6 Mb/s | 2,28× | brak (Pascal) |
| HEVC 2160p25 30 Mb/s | **0,55×** (2 wątki 0,87×, 4 wątki 1,18×) | 4,0× |

Materiały do 1080p dekodują się na CPU w tempie, także AV1. Tylko 4K nie
nadąża, ale NVDEC 4K ze `scale_cuda` zajmuje 252 MiB (`-extra_hw_frames 0`
bez zmian). Z koderem daje to ok. 330 MiB na kawałek, ×2 przy zmianie
pozycji, czyli więcej, niż P400 dzielona z pulpitem ma wolne. Lokalna
biblioteka ma wyłącznie 720p. Wniosek: bez zmian w kodzie, a materiały 4K
przekodować w bibliotece do 1080p.

### E6. Drugi kanał (sprawdzenie pojemności)

Po E1–E4: dwa kanały 1080p50 (albo 1080p50 + 720p50) przez 2 h ze zmianami
pozycji i jednym przejęciem live. Oczekiwanie: NVENC ≤ 40%, vRAM stosu
≤ 2 × 2 × 160 MiB w szczycie, CPU ≤ 3 rdzenie. Wynik zapisać w DOKUMENTACJI
§4 jako pojemność P400.

## Rozeznanie 2026-11-11 (sieć + szybkie pomiary)

- **Pojemność P400:** w benchmarku Livepeer karta trzymała 6 równoległych
  strumieni w czasie rzeczywistym i była ograniczona **wyłącznie pamięcią
  karty**. To potwierdza kierunek planu. Limit sesji NVENC dla Quadro
  poniżej x2000 nie jest jednoznaczny w źródłach (NVIDIA dzieli karty na
  „qualified” i „non-qualified”), więc test sesji w E0 zostaje.
- **Wspólny kontekst CUDA (`-init_hw_device cuda=cu,primary_ctx=1`):**
  sprawdzone, bez zysku (103 → 103 MiB z `overlay_cuda`, sam koder 79 → 79
  MiB). Koder NVENC i tak używa kontekstu urządzenia z komendy.
- **`CUDA_MODULE_LOADING=LAZY`:** domyślne od CUDA 12.3 i bez zmian
  w pomiarze („Pamięć karty przy NVENC”). Odpada.
- **Blokada zegarów (`nvidia-smi -lgc`):** Pascal zwykle jej nie obsługuje,
  a karta i tak pracuje w P0 na maksymalnym zegarze (1252 MHz). Odpada.
- Koszt samego `overlay_cuda` z jedną strefą 1080p to ok. +24 MiB nad koderem
  (103 vs 79 MiB). Na antenie (3 strefy, `scale_cuda`, `fps`) 137 MiB. Tryb
  `encode` w E4 będzie więc kosztował ok. 80 MiB zamiast ok. 140.

Źródła: [Livepeer – GTX 950 + Quadro P400](https://forum.livepeer.org/t/gtx-950-quadro-p400-benchmarks/1497),
[NVENC Application Note 12.2](https://docs.nvidia.com/video-technologies/video-codec-sdk/12.2/nvenc-application-note/index.html),
[ffmpeg trac #9285](https://fftrac-bg.ffmpeg.org/ticket/9285),
[ffmpeg: `primary_ctx`](https://lists.mplayerhq.hu/pipermail/ffmpeg-cvslog/2021-November/129825.html),
[CUDA Lazy Loading](https://docs.nvidia.com/cuda/archive/13.1.1/cuda-programming-guide/04-special-topics/lazy-loading.html),
[Tom's Hardware – 580 ostatnią gałęzią dla Pascala](https://www.tomshardware.com/pc-components/gpu-drivers/nvidia-to-axe-maxwell-pascal-and-volta-gpus-with-end-of-driver-support-580-series-drivers-will-be-the-last-to-support-gtx-900-and-1000-cards),
[NVIDIA – wsparcie architektur w CUDA](https://developer.nvidia.com/blog/navigating-gpu-architecture-support-a-guide-for-nvidia-cuda-developers),
[HandBrake – NVENC](https://handbrake.fr/docs/en/1.7.0/technical/video-nvenc.html),
[NVENC – generacje (TU117)](https://en.wikipedia.com/wiki/NVENC).

## Poza zakresem

- HEVC/AV1 na wyjściu: P400 nie ma klatek B w HEVC ani AV1, a odtwarzacze
  HLS i tak oczekują H.264.
- Grafika na GPU w sidecarze (renderer natywny na CUDA): sprzeczne z zasadą
  „Bez GPU na ścieżce grafiki”.
- Zmiany w ErsatzTV (np. `loudnorm` z krótszym wyprzedzeniem): tylko przez
  nakładkę i tylko jako jawna opcja – teraz niepotrzebne, bo `hwdownload`
  rozwiązuje problem kolejki.

## Kolejność i decyzje użytkownika

1. E0 + E2 + E7 – od razu, bez kodu w `src/` (E7: blokada sterownika na
   hoście – decyzja użytkownika).
2. E4 – pierwszy kod, bo pulpit zostaje bez zmian (E1 odrzucone), a E4
   chroni antenę niezależnie od tego, co jest na pulpicie.
3. E3 (`p5` + `spatial-aq`, ewentualnie `temporal-aq`), E5 – tylko jeśli
   pomiary jakości i tempa pokażą zysk.
4. E6 – potwierdzenie i liczby do dokumentacji.
5. E8 – tylko gdy E6 pokaże, że jedna P400 z pulpitem nie wystarcza.
