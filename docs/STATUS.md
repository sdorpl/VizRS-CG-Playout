# STATUS – stan prac

> Aktualizuj w tym samym commicie, który zmienia stan. Znane braki: [LUKI.md](LUKI.md), opis działania: [DOKUMENTACJA.md](DOKUMENTACJA.md), zmiany wydań: [../CHANGELOG.md](../CHANGELOG.md).
>
> **Ostatnia aktualizacja:** 2026-11-15

## Bieżący etap

**Wydanie 26.11.10** (2026-11-14): logo na przejęcie kanału przez live ingest (`on_live`), grafika ramówki wstrzymana na czas przejęcia. Stosy s1 i s2 (Portainer GitOps, `.release`) – obrazy `vizrs-cg` 26.11.10 i `vizrs` z wydań VizRS CG System.

## Następne

1. **Połączenie z VizRS Live** w jeden produkt „VizRS CG Playout” – plan: [sdorpl/VizRS](https://github.com/sdorpl/VizRS) `PLAN-PORZADKOWANIA.md` §3 (panel, most i okno w `panel/`, jedna nakładka compose, jedna wersja, profil compose `grafika` dla trybu bez grafiki). Zmiany compose wdrażane na s1/s2 tylko w oknie ustalonym przez użytkownika.
2. Po połączeniu: podział `DOKUMENTACJA.md` na `API.md` (Live API, ETV, ffmpeg, ingest), `RUNBOOK.md` (compose, GPU, Portainer, `.release`, proxy nginx z TLS dla panelu) i opis działania.
