# Plany

Dokumenty projektowe: zakres punktów, decyzje i ich uzasadnienie. Bieżący stan prac jest w [../STATUS.md](../STATUS.md), znane braki w [../LUKI.md](../LUKI.md), opis działania w [../DOKUMENTACJA.md](../DOKUMENTACJA.md) (sidecar §1–§8, panel §9–§15).

## Bieżące

| Dokument | Stan | Zakres |
|---|---|---|
| [PLAN-mcr.md](PLAN-mcr.md) | K1–K21 zrobione i wydane (ostatnie: K15, K16, K20 w 26.11.29); K22–K24 (nagła emisja live, K24 drogą W1) wydane w 26.11.36; otwarte Q1 (ręcznie) i propozycje §7.3 bez decyzji; §8 (próba Next) odrzucona 2026-11-28 | **jedyny plan rozwoju panelu**: jeden przebieg luk i planu (K1–K18, Q1): G1 (sesje, CSP), braki wobec web GUI legacy, panel na wzór Viz Channel Branding (kontrola ramówki, mozaika, import ramówki, warstwy, TAKE/OUT, as-run, zdarzenia wtórne, promo, alarmy); decyzja: zostajemy przy legacy (§6) |
| [native.md](native.md) | w `main`: `renderer: auto` z `vizrs-render` z VizRS CG System 26.11.13 (od 26.11.30 – ruch 2D, WebP); stan etapów – PLAN-native w CG System | natywny renderer VizRS (`vizrs-render`) zamiast Chromium |
| [PLAN-strefy.md](PLAN-strefy.md) | S0–S6 zrobione | strefy grafiki liczone z tego, co jest na antenie (`VIZRS_AREA=auto`) |

**Plany panelu – jeden dokument** (decyzja użytkownika 2026-11-09): [PLAN-mcr.md](PLAN-mcr.md). Nowe punkty i analizy dopisuj tam (tabela z kolumnami „Punkt | Zakres | Wymaga | Stan”, opis punktu tak, żeby dało się go zrobić w świeżej sesji); nowych plików `PLAN-*.md` dla panelu nie zakładaj. Automat: `panel/scripts/plan.sh status|run` (domyślnie PLAN-mcr; pomija punkty „ręcznie” i ze znacznikiem „CG”; inny plan: `PLAN_FILE=../docs/plany/PLAN-<temat>.md`, ścieżka względem `panel/`).

Plan połączenia z VizRS Live i porządków pakietu: [PLAN-PORZADKOWANIA.md w sdorpl/VizRS](https://github.com/sdorpl/VizRS/blob/main/PLAN-PORZADKOWANIA.md).

## Archiwum (zakończone)

| Dokument | Zakres |
|---|---|
| [archiwum/PLAN-rozbudowy.md](archiwum/PLAN-rozbudowy.md) | panel: od prototypu 0.1.0 do panelu, który zastępuje web GUI legacy (R0–R12); sekcja „Jak pracować” – wzór dla kolejnych planów i automatu `plan.sh` |
| [archiwum/PLAN-makieta.md](archiwum/PLAN-makieta.md) | panel na wzór artefaktu „Makieta UI VizRS Live” (M0–M9) |
| [archiwum/PLAN-live-panel.md](archiwum/PLAN-live-panel.md) | dawny Live Panel sidecara; UI przeniesione do VizRS Live (dziś panel), tu zostało live API |
| [archiwum/PLAN-rust.md](archiwum/PLAN-rust.md) | migracja sidecara z Node.js do Rusta |
| [archiwum/PLAN-plynnosc.md](archiwum/PLAN-plynnosc.md) | płynność animacji grafiki |
| [archiwum/PLAN-ampere.md](archiwum/PLAN-ampere.md) | emisja w 2 OCPU (Oracle Ampere A1) |
| [archiwum/PLAN-p400.md](archiwum/PLAN-p400.md) | lokalny stos na Quadro P400: pamięć karty, NVENC, `VIZRS_FFMPEG_CUDA=auto` |

## Poza repozytorium

| Dokument | Zakres |
|---|---|
| [PLAN-vizrs-live.md](../../../VizRS%20CG%20System/docs/plany/PLAN-vizrs-live.md) (VizRS CG System) | VizRS Live w rodzinie VizRS: założenia, decyzje, powiązanie z grafiką VizRS i VizRS CG Playout |
