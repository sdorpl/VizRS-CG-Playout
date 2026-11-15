# PLAN-makieta – panel na wzór makiety UI

> Wzorzec: artefakt **„Makieta UI VizRS Live”** – https://claude.ai/artifact/9ajuiXeKEZD6ntYLxtAzbM (sekcje = ekrany, kotwice `#antena`, `#epg`, `#harmonogramy`…; czytać narzędziem Artifact `read`, nie przez WebFetch). Stan prac: [../STATUS.md](../STATUS.md).

## Założenie

Panel **nie jest przepisywany od zera**. Porównanie zrzutów (2026-11-07) z makietą: wygląd (`style.css` – kolory, pasek górny, menu z grupami, panele, „lista + edytor”, podpis „Pod spodem”) już jest makietą. Różnice są w treści ekranów – co pokazują tabele, karty i podglądy. Każdy punkt przenosi z makiety jeden ekran albo grupę i **tylko to, co ma pokrycie w danych** (legacy przez most, CG Playout, VizRS). Przykładowe dane makiety (np. „Next: 3/3 nadaje”, „dysk 91 %”) nie stają się zmyślonymi wartościami w panelu – brak źródła = pomijamy albo zapisujemy w [../LUKI.md](../LUKI.md).

Zrobione wcześniej (poza planem): **Harmonogramy** – start „06:00 stała / po poprzedniej”, wypełniacze, znaki i grafika w tabeli pozycji, „Podgląd 24 h” (commit 3085509).

## Jak pracować z tym planem

Jak w [PLAN-rozbudowy.md](PLAN-rozbudowy.md) („Jak pracować”): jeden punkt = jedna sesja, kolejność od góry, warunki „gotowe”. **Testy bez stosu testowego:** `DEPLOY_SHOTS=<katalog poza repo> tests/deploy.sh` (szybkie testy, wdrożenie panelu na lokalny stos, przejście po ekranach tylko do odczytu) zielony, **zrzut obejrzany obok sekcji makiety**, nowe przypadki w `tests/e2e.cjs` (uruchamia CI), DOKUMENTACJA §4, CHANGELOG, kolumna Stan, commit. Automat: `PLAN_FILE=docs/plany/PLAN-makieta.md scripts/plan.sh run` (wymaga czystego drzewa; kontrola po sesji: `tests/deploy.sh`). Na początku sesji przeczytaj sekcję makiety swojego ekranu i zrób zrzut obecnego ekranu – porównanie obu to punkt wyjścia.

## Tabela

| Punkt | Zakres | Wymaga | Stan |
|---|---|---|---|
| M0 | Okno (karta pozycji, potwierdzenia) zamyka się przy zmianie ekranu – zostawało nad nowym ekranem | – | zrobione (2026-11-07) |
| M1 | Pasek górny i menu: liczniki w menu, wybór kanału w pasku | M0 | zrobione (2026-11-07) |
| M2 | Na antenie: karta kanału z ramką VizRS, panel „Uwagi”, wspólna oś zdarzeń | M1 | zrobione (2026-11-07) |
| M3 | Przewodnik EPG: wszystkie kanały, wybór okna czasu, sklejanie krótkich pozycji | M1 | zrobione (2026-11-07) |
| M4 | Media: kafle w Przeglądarce, „Zapisz jako kolekcję inteligentną” | M0 | zrobione (2026-11-07) |
| M5 | Kolekcje: jedna lista z pigułką rodzaju, podgląd zawartości w edytorze | M4 | zrobione (2026-11-07; bez łącznego czasu – brak danych w legacy) |
| M6 | Playouty: „Zbudowany do” i „Przebuduj” w wierszu, najbliższe pozycje z blokiem i LIVE | M0 | zrobione (2026-11-07; „Od nowa o” i „Budowa” wchłonięte przez „Zbudowany do”) |
| M7 | Bloki: kalendarz tygodnia jako siatka godzin × dni | M6 | zrobione (2026-11-07) |
| M8 | Grafika kanału i Sloty na żywo: reguły „Kiedy / Co / Ile”, tabela wejść ze stanem | M2 | zrobione (2026-11-07; audycje LIVE były już tabelą na ekranie Audycje LIVE; „Bez sygnału” (plansza) i bitrate wejścia – brak danych w CG Playout) |
| M9 | Ustawienia i Diagnostyka: panele obok siebie, „Stan” stosu na górze, logi ze źródłem | M0 | zrobione (2026-11-07; logi CG Playout = dziennik zdarzeń z /api/live/state – CG nie wystawia log.jsonl przez API; VizRS bez sprawdzania łączności z serwera – adres i podgląd) |

## Punkty

### M0 – okno zamyka się przy zmianie ekranu
Błąd znaleziony przy zrzutach: karta pozycji otwarta w Przeglądarce (albo Playoutach) zostawała otwarta po kliknięciu w menu, nad kolejnymi ekranami. `dialog()` w `core.js` zamyka się teraz na `hashchange` jak „Anuluj” (obietnica dostaje `false`). e2e: karta pozycji → inny ekran → `#overlay` ukryty.

### M1 – pasek górny i menu
Makieta: liczniki przy pozycjach menu (`Kanały 3`, `Kolekcje 14`, `Playouty 3`…), wybór kanału w pasku („Wszystkie kanały (3)” / „1 · INFO24”), pigułka stanu CG Playout („połączony”).
- Liczniki z lekkich komend listujących, pobierane raz po starcie i po zapisie na danym ekranie (bez odpytywania w pętli); brak komendy na białej liście = brak licznika.
- Wybór kanału: pamiętany w przeglądarce (`localStorage`, w `try`), filtruje ekrany kanałowe (Na antenie, EPG, Playouty, Grafika kanału, Sloty); „Wszystkie” = jak dziś.
- Pigułka CG Playout: dziś tylko błąd połączenia – dodać stan „połączony”.

### M2 – Na antenie
Makieta: karta kanału = ramka obrazu (grafika VizRS na antenie kanału), teraz/następnie z paskiem postępu, pigułki warstw CG, przyciski; obok „Najbliższe zdarzenia” (pozycje ramówki + zdarzenia CG, np. slot na żywo) i „Uwagi”.
- Ramka VizRS w karcie kanału, gdy `--vizrs-channels` (antena `bus=ch-N`); bez anten kanałów – jedna wspólna ramka jak dziś.
- „Uwagi”: playouty z błędem budowy (`GetPlayoutWarningsCount` + komunikat z playoutu), kanał bez playoutu, slot na żywo w najbliższej godzinie bez sygnału (stan CG `/api/live/state`).
- Najbliższe zdarzenia: pozycje legacy wszystkich kanałów + najbliższe audycje LIVE z CG (`next_live`), posortowane, z pigułką kanału.
- Warstwy CG (pigułki „W10 belka 101”) tylko jeśli CG Playout je wystawia – sprawdzić API; jeśli nie, wiersz w LUKI.

### M3 – Przewodnik EPG
Makieta: wszystkie kanały w wierszach, linijka godzin, linia „teraz”, bieżąca pozycja wyróżniona, LIVE na czerwono, data w pigułce.
- Kanały bez playoutu jako pusty wiersz z odnośnikiem do Playoutów.
- Wybór okna: ← / teraz / → (po 4 h) i data w pigułce.
- Sąsiednie pozycje krótsze niż ~1 % okna sklejane w „N pozycji” (podpowiedź z listą) – dziś krótkie pozycje dają nieczytelny grzebień.
- Pozycje audycji LIVE (z CG `guide`) wyróżnione.

### M4 – Media
Makieta: kafle z czasem trwania na miniaturze, tytuł i opis pod spodem; pole zapytania z „Zapisz jako kolekcję inteligentną”; wynik „18 pozycji”.
- Przełącznik tabela / kafle (pamiętany w przeglądarce); kafle bez miniatur legacy, chyba że da się je podać bez otwierania legacy na zewnątrz (`/artwork/*` przez proxy – sprawdzić; inaczej czas trwania na tle jak w makiecie).
- „Zapisz jako kolekcję inteligentną” z bieżącego zapytania (`CreateSmartCollection`, nazwa w oknie), potem przejście do Kolekcji.

### M5 – Kolekcje
Makieta: jedna tabela wszystkich rodzajów z pigułką rodzaju (zwykła / inteligentna / wielokolekcja / powtórki) i liczbą pozycji; edytor z „Podglądem” (liczba pozycji, łączny czas, najnowsza).
- Zakładki zostają jako filtr („Wszystkie” domyślnie), lista łączona.
- Podgląd w edytorze z danych, które legacy daje (pozycje kolekcji / wynik zapytania inteligentnej).

### M6 – Playouty
Makieta: kolumny „Źródło”, „Zbudowany do”, przycisk „Przebuduj” w wierszu; po prawej najbliższe pozycje z kolumną „Blok”, wypełniacze przygaszone, LIVE w pigułce.
- „Zbudowany do” = koniec ostatniej pozycji playoutu (jeśli komenda listy go nie daje – z `GetFuturePlayoutItemsById`, tylko dla zaznaczonego).
- Najbliższe pozycje: wypełniacze `dim`, pozycje audycji LIVE z pigułką.

### M7 – Bloki: kalendarz tygodnia
Makieta: siatka – kolumny dni, wiersze godzin, bloki jako prostokąty w kolorach rodzaju, nagłówek z tygodniem i pigułkami szablonów („Dzień roboczy: pn–pt”). Dziś: paski doby jeden pod drugim. Siatka jako drugi widok (przełącznik), pozycje przez `el.style` (CSP), poziome przewijanie na wąskim ekranie.

### M8 – Grafika kanału i Sloty na żywo
Makieta: reguły w tabeli „Kiedy / Co / Ile” z edytorem obok i podglądem strony; wejścia studia w tabeli (adres, stan, obraz), audycje w tabeli (kiedy, wejście, bez sygnału).
- Tabela reguł jako streszczenie obecnego edytora reguł CG (bez zmiany zapisu `/api/live/graphics`).
- Wejścia: stan i parametry sygnału ze `/api/live/state` (`source`), adresy jak dziś.

### M9 – Ustawienia i Diagnostyka
Makieta: Ustawienia – sekcje legacy jako panele obok siebie (`cols`) zamiast zakładek; Diagnostyka – „Stan” stosu na górze (legacy, most, CG Playout, VizRS – OK/UWAGA z wersją), logi z wyborem źródła.
- Ustawienia: panele w siatce, każdy z własnym „Zapisz” (jak dziś zakładki).
- Diagnostyka: wiersz „Stan” z danych, które panel już ma (`/api/status`, kontrole legacy, CG, VizRS); logi – źródło legacy / CG Playout (dziennik `log.jsonl` przez API CG, jeśli dostępne).
