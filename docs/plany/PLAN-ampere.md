# Emisja na Ampere A1 w budżecie 2 OCPU

Pomiary 2026-10-31 na s2 (Neoverse-N1, 4 OCPU, obok kanału 4 na antenie).
Łańcuch jak na antenie: 1600×900@25, x264 `superfast`, 3000 kb/s, dwa obszary
grafiki, tempo 1×, 30 s, bez audio. CPU w rdzeniach, SSIM dla Y w dB (różnica
względem obecnego łańcucha). Kontrolne powtórki bazy różniły się o ok. 1%.

## Budżet

Pomiar 60 s, film z tickerem: `ffmpeg` ok. 0,96, Chromium 0,22, sidecar 0,08,
ErsatzTV 0,08, reszta (nginx, docker-proxy, core) ok. 0,06, razem ok. 1,4.
Czytniki MPEG-TS `.ts`: ok. 0,3% rdzenia na proces. W `ffmpeg` libx264 to 72%.
Wyjścia Restreamera nie są w tym budżecie; nie muszą zostać na tej maszynie.

| Materiał | Teraz | Po poprawce kolorów | Cały kanał po poprawce |
|---|---|---|---|
| 1080p30 12,6 Mb/s (Warmianin) | 1,50 | 1,21 | ok. 1,7 |
| 1080p60 (STALIN) | 1,46 | 1,21 | ok. 1,7 |
| 720p30 (Federacja) | 0,82 | 0,70 | ok. 1,2 |
| 900p30 1,9 Mb/s (Gothic) | 0,95 | 0,90 | ok. 1,4 |

Po poprawce najcięższe materiały trzymają 1× w kontenerze `--cpus 1.3`. Zapas
na nadrabianie po awarii i start pozycji jest mały; pełnego stosu w 2 OCPU nie
sprawdzano.

## Poprawka: deklaracja kolorów grafiki

`overlay` z nieoznaczonym wejściem surowych klatek negocjował `csp:unknown` dla
całego łańcucha, więc `scale` przepuszczał każdą skalowaną klatkę przez RGB
(BT.709 → BT.601). Profil: libx264 bez zmian (0,82), `ffmpeg` 0,66 zamiast 0,35,
gorąca pętla to konwersja YUV → BGR24 w C. Koszt nie zależał od rozmiaru ani
treści grafiki (16×16 i przezroczysta to ten sam wynik). SSIM po poprawce:
Warmianin +0,47 dB, Gothic +0,03 dB (wszystkie płaszczyzny +0,26 dB). Strumień
ma `color_space=bt709` zamiast `unknown`. Opis: DOKUMENTACJA §1.

## Tickery

Pomiar 2026-10-31 (s2, kopia `show.json`, własny VizRS, ta sama binarka
i Chromium co na antenie, kanał 1600×900@25, `frames`, obszar
`1600x110+0+790,360x94+1200+82`, 20 s). CPU w % jednego rdzenia: Chromium /
sidecar; poza ostatnimi wierszami stan z `VIZRS_TAPE=1`.

| Grafika | Strony | Bez taśmy | Z taśmą | Obrazy/s |
|---|---|---|---|---|
| nic na antenie | – | 4,6 / 1,2 | – | 0 |
| odliczanie | 2101 | 8,9 / 1,4 | – | 1 |
| góra-dół (`roll`) | 3002, 5004, 5007 | 12 / 2 | bez zmian | 2,7–3,4 |
| wipe | 3003, 5008 | 13–14 / 2,3 | bez zmian | 5,5 |
| Ticker OPilne Wiper | 5005, 5006 | 14,5 / 2,8 | bez zmian | 5,5 |
| przewijany (`crawl`) | 3001, 3004, 5002, 5003, 5009 | 24–25 / 7 | 16 / 8 | 25 |
| Ticker kopia | 5010 | 28,6 / 9,2 | 16,5 / 8 | 25 |
| Ticker kopia kopia | 5011 | 26,8 / 8,7 | 27,3 / 8,6 (taśma się nie włącza) | 25 |
| 3004 + 2101 | – | – | 18,4 / 7,3 | 25 |
| 3003 + 2101 | – | – | 16 / 3 | 6,3 |
| 3004, cała klatka | – | 57 / 34 | 12 / 31 | 25 |
| 3003, cała klatka | – | 19,5 / 10,7 | – | 5,5 |

Taśma działa w `crawl`; w `roll` i `wipe` obraz i tak stoi między zmianami.
Przyczyny braku taśmy w 5011 nie ustalono (scena ma węzły inne niż 5010).
Zmiana na antenie z 3004 na 3003 oszczędza ok. 0,07 rdzenia. Obszar daje tu
więcej niż taśma: pełna klatka to ok. 4× więcej CPU razem. Natywny renderer
miałby do zdobycia najwyżej ok. 0,2 rdzenia.

## Rezerwa, gdy poprawka nie wystarczy

Wymaga nowej opcji nakładki (parametry x264), bez niej niedostępne.

| Opcje x264 | CPU Warmianin / Gothic | SSIM Warmianin / Gothic |
|---|---|---|
| `subme=0` | −6% / −8% | 0,00 / −0,39 |
| `bframes=0` | −11% / −15% | −0,10 / −0,77 |
| `bframes=0:subme=0` | −18% / −20% | −0,11 / −1,44 |

Warmianin (mocne ziarno) nie reprezentuje anteny: decyduj po Gothicu.

## Odrzucone

| Składnik | Wynik | Powód |
|---|---|---|
| `ultrafast` | −31…−36% CPU, −3,4…−5,0 dB | jakość |
| `partitions=none`, bez CABAC | −5% CPU, −0,4…−1,0 dB | gorsze od `subme=0` |
| GOP 4 s | 0% CPU, ok. 0 dB | bez efektu |
| THP (`glibc.malloc.hugetlb=1`) | 0% | bez efektu |
| x264 master z DotProd, GCC/Clang/PGO | −2,5…−4% | zysk nie wart utrzymania własnej biblioteki |
| własny `ffmpeg` pod N1 | 0% względem samego PGO x264 | bez efektu |
| x265 `ultrafast` | ok. 7–8× drożej | nie mieści się w czasie rzeczywistym |
| OpenH264 2.6 | −20…−22% CPU, −0,31…−1,49 dB | jak dostrojony x264, wymaga własnego `ffmpeg` |
| 720p przy 3000 kb/s | Warmianin −27%, Gothic 0% | nie jest darmowym zyskiem |
| mezzanine (film raz w 1600×900@25) | Warmianin 1,10–1,15, Gothic 0,99–1,04; −0,4…−1,1 dB | po poprawce kolorów zysk znikomy lub ujemny, ok. 4 s CPU na sekundę filmu i 5–11 Mb/s na dysku |

Kvazaar nie został zmierzony (błąd komendy). Liczby PGO, własnego `ffmpeg`,
OpenH264 i mezzanine zebrano przed poprawką kolorów. Nie sprawdzano SVT-AV1,
VP9 ani GStreamera (te same biblioteki). Blitz nie wykona JavaScriptu `/output`.

## Wdrożenie

1. Poprawka na s2 i s1: restart `cg` zamraża grafikę do końca pozycji.
2. Pomiar pełnego kanału na antenie (`perf`, CPU, kolory obrazu).
3. Dopiero potem decyzja o zmniejszeniu instancji. Nie usuwać instancji A1
   w ramach tego zadania: odtworzenie ponad limit może być niemożliwe.
