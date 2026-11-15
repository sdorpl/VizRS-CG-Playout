# LUKI – znane braki

> Uszeregowane G1 (niezawodność i bezpieczeństwo) → G2 (wierność) → G3 (funkcje). Zamknięta luka = usunięty wiersz (w commicie, który ją zamyka).

| # | Grupa | Luka | Stan / uwagi |
|---|---|---|---|
| 1 | G1 | Po restarcie ErsatzTV oś czasu kanału bywa ok. 12 s za zegarem – więcej niż `LIVE_BEHIND_MAX` (10 s, `src/takeover.rs`), więc nakładka nadrabia ramówkę i nie wpuszcza live, choć studio nie jest wstrzymane | obserwacja 2026-11-14 (kanał z przejęciem live ingest); do analizy |
| 2 | G2 | Jeden sygnał live na kanał: drugi dostaje 409, aktywne przejęcie nie jest wywłaszczane – brak priorytetów odbiorników | DOKUMENTACJA §1 „Live ingest” |
| 3 | G2 | Renderer natywny (`vizrs-render`) nie rysuje wszystkiego (np. ruch `node.motion`, WebP, wideo, fonty spoza wbudowanych) – w trybie `auto` rysuje wtedy Chromium (więcej CPU) | DOKUMENTACJA §1 „Renderer natywny VizRS” |
| 4 | G2 | Dokładne granice stron (`vizrs-render bounds`) wymagają VizRS CG System ≥ 26.11.2; ze starszym strefy grafiki są uczone z pikseli klatek | DOKUMENTACJA §1 „Obszar grafiki (`area`)” |
| 5 | G3 | Dokumentacja w jednym pliku (`DOKUMENTACJA.md`, ok. 2800 wierszy) – brak osobnych `API.md` i `RUNBOOK.md` | plan połączenia z VizRS Live (STATUS „Następne”) |
