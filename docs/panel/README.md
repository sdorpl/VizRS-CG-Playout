# Dokumentacja VizRS Live

## Użytkownicy

| Dokument | Dla kogo | Zawartość |
|---|---|---|
| [INSTRUKCJA.md](INSTRUKCJA.md) | operator kanału | wejście do panelu, codzienna praca, przebudowa ramówki, usuwanie, komunikaty, awarie |
| [DOKUMENTACJA.md](DOKUMENTACJA.md) | wszyscy | pełny opis: składniki, most i jego API, panel (opcje, API, UI), ekrany i decyzje, ograniczenia legacy, aktualizacja legacy, testy, proxy i lista pokrycia, wydanie i bezpieczeństwo |
| [../CHANGELOG.md](../CHANGELOG.md) | wszyscy | historia zmian |

## Wdrożenie i administracja

| Dokument | Dla kogo | Zawartość |
|---|---|---|
| [RUNBOOK.md](RUNBOOK.md) | administrator, dyżur techniczny | składniki i porty, wdrożenie, kopie `etv-config`, aktualizacja panelu i legacy, odtworzenie, awarie, wydanie nowej wersji |
| [../.env.example](../.env.example) | administrator | tokeny, przypięte wersje obrazów, adresy, katalogi |

## Zespół i rozwój

| Dokument | Zawartość |
|---|---|
| [STATUS.md](STATUS.md) | bieżący etap, następne zadanie, kroki ręczne, blokery |
| [LUKI.md](LUKI.md) | znane braki panelu, uszeregowane G1 → G2 → G3 |
| [plany/](plany/README.md) | plany (rozbudowa R0–R12, wzór pracy „jeden punkt = jedna sesja”) |
| [../CLAUDE.md](../CLAUDE.md), [../GEMINI.md](../GEMINI.md) | zasady pracy z kodem, struktura, testy, commity (programiści i agenci; `GEMINI.md` wczytuje `CLAUDE.md`) |
| [AGENCI.md](AGENCI.md) | podział pracy agentów: Claude dowodzi i pisze kod, Gemini – dokumentacja, kompilacja, testy, streszczenia; zlecenie, raport, przegląd |
