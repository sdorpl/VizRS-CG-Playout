# PLAN: natywny renderer grafiki VizRS (bez Chromium)

> Stan (2026-11-28): renderer w `main` obu repozytoriów; obraz `vizrs-cg` od 26.11.30 wkompilowuje `vizrs-render` z VizRS CG System 26.11.13 (`ARG VIZRS_REF`) – N1, N2 oraz ruch `node.motion` 2D i WebP nieruchome; reszta N3/N4 i N5 – VizRS `docs/plany/PLAN-native.md`.
>
> Pierwotnie (2026-10-31): **etap N1 rozpoczęty** na gałęziach `render-native` obu repozytoriów: renderer `vizrs-render` 0.1
> (VizRS `source/render/`, stan etapów: VizRS `docs/plany/PLAN-native.md`) i opcja `renderer: chromium|native|auto`
> w sidecarze (DOKUMENTACJA §1, §5). Decyzje z §8 podjęte wg propozycji planu (niżej, „Decyzje – stan”).
> Dotyczy dwóch repozytoriów: **VizRS CG System** (renderer, nowy kanał protokołu) i **VizRS CG Playout** (odbiór klatek w sidecarze).
> Powiązane: [DOKUMENTACJA.md](../DOKUMENTACJA.md) §1 (taśma crawla, obszar grafiki), §4 (Ampere A1), §8 (benchmark);
> [PLAN-plynnosc.md](archiwum/PLAN-plynnosc.md); w VizRS: `docs/plany/PLAN-lekkie-output.md`, `docs/DOKUMENTACJA.md` §4, §6, §7.

## 0. Cel

Grafika kanału na Oracle Cloud Ampere A1 (arm64, bez GPU) ma kosztować ułamek obecnego CPU przy ciągłej animacji
(ticker crawl, „Na żywo”). Wygląd, animacje i parametry kanału zostają zachowane. Dziś przewijany pasek 1080p
to ok. 2 rdzenie Chromium i sidecara.

## 1. Rozważone i odrzucone kierunki

| Kierunek | Wniosek |
|---|---|
| **GStreamer z `x264enc`** zamiast ffmpeg | Brak zysku. `x264enc` to ten sam libx264 (NEON na arm64), który ffmpeg ErsatzTV woła jako `-vcodec libx264`. Koszt zależy od presetu, nie od frameworka (pomiar 2026-10-26: `medium` ok. 2,5 rdzenia, `veryfast` 1,6, `superfast` z Normalize Framerate 1,3). Dekodowanie (`avdec_h264` = libavcodec) i nakładanie (`compositor` wobec `overlay`) kosztują podobnie. Wymagałoby tłumaczenia każdej komendy ffmpeg ErsatzTV na pipeline GStreamera i doinstalowania bibliotek do obrazu ErsatzTV, co łamie zasady projektu. |
| **WPE WebKit** zamiast Chromium | Zmierzony na A1 (DOKUMENTACJA §8): przy animacji 1080p25 12,2 kl./s przy 243% CPU, Chromium 23,4 kl./s przy 209%. Nie ładował `FontFace` z `local()`. Odrzucony. |
| **Firefox headless** | Bez odpowiednika screencastu DevTools i `beginFrame`. Zostają zrzuty przez WebDriver BiDi, wolniejsze i bez taktowania klatkami. |
| **Servo** | Niedojrzały dla CSS i animacji używanych przez `/output`. Za duże ryzyko przy emisji 24/7. |
| **Ultralight** | Zamknięty, z własną licencją, linkowany. Sprzeczny z zasadą małej liczby zależności. |

Chromium w trybie `frames` (`beginFrame`, `chromium-headless-shell`) to najtańszy znany wariant przeglądarkowy.
Dalszy zysk wymaga rezygnacji z HTML na ścieżce emisji.

## 2. Skąd bierze się koszt dziś

Przy ciągłej animacji na każdą klatkę płacimy za:

1. rastrowanie DOM i CSS na CPU w Chromium,
2. kodowanie PNG w Chromium,
3. dekodowanie PNG, wyznaczenie prostokątów, skalowanie (`render_size`) i `yuva420p` w sidecarze.

Kroki 2 i 3 to podatek za przeglądarkę jako proces zewnętrzny. Taśma crawla (`VIZRS_TAPE`, DOKUMENTACJA §1)
już pokazała, że przesuwanie raz wyrenderowanej taśmy bez Chromium obniża CPU przeglądarki kilkukrotnie.

## 3. Co rysuje `/output` i co musiałby odtworzyć renderer natywny

`/output` rysuje `ScenePlayer` z `web/renderer.js` (ok. 1800 linii) przez DOM i CSS (`transform`, `filter`,
`mask`, gradienty), ruch liczy `web/motion.js`, a układ tekstu `web/text-model.js`.

| Element | Natywnie | Wierność |
|---|---|---|
| `rect`, `ellipse`, gradient, zaokrąglenie, ramka, krycie, `x/y/w/h`, `rotation`, `scale`, `skewX` | proste (rasteryzer 2D) | praktycznie identyczna |
| maski `clipL/R/T/B`, wipe, maska ukośna, żaluzja, rosnący pasek | proste (przycinanie ścieżką) | identyczna |
| klucze animacji, krzywe (`inout`, `back`, `hold`, nazwy GSAP, `cubic-bezier`), stop pointy, presety ruchu, pętle uwagi, `blink`, `dock`, autofollow | matematyka z `motion.js`, do przepisania 1:1 | identyczna, a ruch równiejszy (czas = numer klatki kanału) |
| `qr`, `clock` (formaty, strefa IANA, domyślnie `Europe/Warsaw`) | proste; strefy wymagają bazy tzdata | identyczna |
| `shadow`, `blur`, rozmycia w presetach (20 px całości, 14 px jednostek) | rozmycie gaussowskie na CPU | bardzo bliska; duże rozmycie co klatkę kosztuje też natywnie |
| odwrócenia X/Y (`perspective`, `rotateX/Y`) | przekształcenie perspektywiczne bitmapy warstwy | bliska, inne wygładzanie krawędzi |
| rozdzielenie RGB, glitch, `shimmer`, `glow`, `scanline` | ręcznie, każdy efekt osobno | bliska, zależnie od staranności |
| **tekst**: fonty TTF/OTF/WOFF/WOFF2 z assets, kerning, polskie znaki, `squeeze*`, `shrink`, łamanie `balance`/`fill`, twarde spacje, `[[wyróżnienia]]`, `{b}{i}{u}{c=}{sz=}`, `upper`, `spacing`, efekty na znaki, słowa i linie, `scramble`, licznik | **najtrudniejsze**: kształtowanie tekstu, rasteryzacja glifów, pomiar szerokości i cały układ (`textFit`, łamanie) z tymi samymi podziałami co przeglądarka | układ zgodny przy tych samych metrykach; glify minimalnie inne (wygładzanie, hinting inne niż Skia w Chromium), na kanale H.264 praktycznie niewidoczne |
| `image`: PNG, JPEG | dekoder obrazów | identyczna |
| `image`: wideo (`.mp4`, `.webm`, `.mov`) | wymaga dekodera wideo (ffmpeg, libav), w rdzeniu VizRS nierealne | na start bez wideo: scena wraca do Chromium (§6). Na kanale wypalanym i tak odradzane (PLAN-lekkie-output L4) |
| fonty z Google Fonts z internetu | odpada | fonty trzeba wgrać do assets (na antenie i tak zalecane) |

**Wniosek:** belki, podpisy, tickery, zegary, „Na żywo”, QR i logo wyglądają i ruszają się tak samo. Różnica
w wygładzaniu glifów nie będzie widoczna po kodowaniu. Odstępstwa zostają przy wideo w grafice i przy efektach
specjalnych, dopóki nie zostaną starannie odtworzone.

## 4. Parametry kanału

- **Rozdzielczość i klatkaż:** renderer rysuje dokładnie w rozmiarze i rytmie kanału. Znika `render_size`,
  skalowanie i dudnienie 60 Hz (PLAN-plynnosc §1.1). Równy ruch wynosi 100% z definicji (czas klatki `n / fps`).
- **Alfa i `yuva420p`:** renderer oddaje RGBA albo od razu `yuva420p`. Znika kodowanie i dekodowanie PNG.
- **`area`:** naturalne, rysuje się tylko obszar paska.
- **Grafika stojąca:** zero kosztu, renderer nie wysyła klatki (odpowiednik `Repeat`).
- **Kolory:** BT.709 (od 720 linii) albo BT.601, zakres ograniczony, alfa prosta, jak dziś w nakładce (DOKUMENTACJA §1).

## 5. Zysk CPU (szacunek, niezmierzony)

Natywnie crawl 1080p to przesunięcie raz wyrenderowanego tekstu i złożenie kilku warstw. Szacunek: kilka do
kilkunastu procent jednego rdzenia zamiast ok. 2 rdzeni. Wyjątki to duże `blur`, cienie i glitch w każdej klatce.
Szacunek trzeba potwierdzić prototypem na A1 (§7, etap N0).

## 6. Wymagania

### 6.1 VizRS CG System

1. **Zasada „Jeden renderer”** (`renderer.js` dla OBS, Trio i Artist). Drugi renderer musi mieć tę samą semantykę.
   Zgodność pilnują wspólne testy: zrzuty wzorcowe Chromium i renderera natywnego z tolerancją, na wzór
   `tools/engines.js` i `tests/e2e/golden.js`, oraz wektory ruchu (`tests/motion.js`) liczone przez obie strony.
2. **Zasada „Zero zależności w rdzeniu”.** Fonty (WOFF2 = Brotli), kształtowanie tekstu i rasteryzacja to dużo
   pracy bez crate'ów. Realnie: osobny workspace, np. `source/render/`, z wąską listą zależności w nagłówku
   `Cargo.toml`, jak wyjątek `source/shell/`. Serwer `vizrs` zostaje bez zależności.
3. **Nowy kanał protokołu**, np. strumień klatek po HTTP albo TCP: sidecar zamawia `scene/kanał, size, fps, area`,
   dostaje surowe klatki z czasem i znacznikiem „bez zmian”. Opis w `docs/API.md`.
4. **Zgłaszanie braku obsługi:** renderer natywny mówi, których scen (węzłów, efektów) nie umie narysować,
   żeby sidecar mógł użyć Chromium.

### 6.2 VizRS CG Playout

1. Odbiór klatek z nowego kanału zamiast przechwytywania Chromium, gdy wszystkie strony na antenie obsługuje
   renderer natywny. Nowa opcja (np. `renderer: chromium|native|auto`) = `src/config.rs`, `config.example.json`,
   `docker-compose.yml`, DOKUMENTACJA §5 i test.
2. **Grafika nigdy nie zatrzymuje emisji:** brak lub awaria renderera natywnego, nieobsłużona scena albo zerwane
   połączenie oznaczają przejście na Chromium (albo czyste klatki), błędy tylko do logu.
3. Testy kontraktu (`tests/contract/`) z udawanym strumieniem klatek VizRS.
4. Benchmark (§8 DOKUMENTACJI) z trybem natywnym do porównania na A1.

## 7. Etapy

| Etap | Zakres | Kryterium |
|---|---|---|
| N0 | Prototyp poza repo: crawl 1080p25 z jednym fontem, taśmą i prostokątem tła, wyjście `yuva420p`, pomiar na A1 | CPU co najmniej 5× niższe niż Chromium z `frames` i `tape`; inaczej plan zostaje zarzucony |
| N1 | **Hybryda**: natywnie tylko to, co ciągle się rusza (ticker `crawl`, `roll`, `wipe`, zegar); reszta strony, wejścia i wyjścia nadal w Chromium. Rozwinięcie taśmy sidecara | równy ruch 100%, CPU kanału na A1 w budżecie 2 CPU sidecara z zapasem |
| N2 | Prymitywy i ruch: wszystkie typy węzłów poza wideo, klucze, presety IN/OUT, pętle uwagi, maski | zrzuty wzorcowe w tolerancji dla `demo_show.json` |
| N3 | Tekst pełny: `textFit`, łamanie, tekst sformatowany, efekty tekstu | te same podziały wierszy i ściśnięcia co Chromium na wektorach z `text-model.js` |
| N4 | Efekty specjalne (3D, RGB, glitch, `bg`) | zrzuty w tolerancji; to, czego nie ma, zgłaszane wg §6.1 pkt 4 |
| N5 | Domyślne `renderer: auto` w `docker-compose.ampere.yml` | tydzień emisji testowej bez przejścia awaryjnego z winy renderera |

Sceny z wideo albo nieobsłużonym efektem zawsze wracają do Chromium, więc grafika nigdy nie znika.

## 8. Decyzje do podjęcia

**Stan (2026-10-31, gałąź `render-native`):** 1 – drugi renderer dopuszczony dla kanału; zgodność pilnuje
`tools/native-compare.js` (VizRS), INFO24 i R7 poniżej 1% różnych pikseli. 2 – osobny workspace `source/render/`
(tiny-skia, rustybuzz, wuff, serde_json, png, jpeg-decoder). 3 – renderer sam czyta `/events`, `/fonts`, `/assets`
jak `/output` (serwer VizRS bez zmian); klatki RGBA (prostokąt z grafiką) przez stdin/stdout procesu podrzędnego,
obszar liczy sidecar. 4 – otwarte, po pomiarze na A1. Etap N0 zastąpiony pomiarem na x86: crawl 720p25 ok. 14% rdzenia
(sidecar + renderer) wobec ok. 56% z Chromium; pomiar na A1 do zrobienia.

1. Czy VizRS dopuszcza drugi renderer (zasada „Jeden renderer”) i na jakich warunkach zgodności.
2. Wyjątek od „Zero zależności” dla osobnego workspace renderera i lista dozwolonych crate'ów.
3. Postać kanału protokołu (HTTP czy TCP, format klatek RGBA czy `yuva420p`, kto liczy obszar).
4. Czy po N1 iść dalej, czy hybryda wystarcza (zależnie od pomiaru na A1).
