# VizRS CG Playout — grafika VizRS na kanałach ErsatzTV

> Do 2026-10-31 projekt nazywał się **VirtRS CG Playout** (repozytorium `sdorpl/VirtRS-CG-Playout`, GitHub przekierowuje stary adres). Nazwy techniczne zostają bez zmian: binarka `vizrs-cg`, obraz `ghcr.io/sdorpl/vizrs-cg`, zmienne `VIRTRS_*`.

Część rodziny **VizRS**: grafika z [VizRS CG System](../VizRS%20CG%20System)
na kanałach [ErsatzTV](https://ersatztv.org). Mały serwis pomocniczy (sidecar)
obok niezmienionego ErsatzTV:

- **Komendy na start klipu** – gdy zaczyna się pozycja playoutu, do VizRS idą
  komendy TCP (`TAKE`, `UPDATE`, `OUT`, `MACRO`, …) z danymi klipu
  i następnego programu (`{title}`, `{next_title}`, `{next_start}`, …).
- **Reklamy bez grafiki** – na reklamie wszystkie warstwy na antenie schodzą;
  strony wracają jednym `TAKE` na pierwszym zwykłym klipie.
- **Wtapianie (burn-in)** – strona `/output` VizRS renderowana bezgłowym
  Chromium (bez GPU) i nakładana na obraz w ffmpeg ErsatzTV (nakładka
  ffmpeg) albo przez element grafiki ErsatzTV.

Sidecar to jeden plik binarny w Ruście (`vizrs-cg`) obok bezgłowego Chromium;
wydania: obraz `ghcr.io/sdorpl/vizrs-cg` (amd64, arm64) i archiwa dla Linuksa
x86_64 i arm64 (DOKUMENTACJA §4, §7).

## Rodzina VizRS

| Repozytorium | Rola |
|---|---|
| [VizRS CG System](../VizRS%20CG%20System) | silnik grafiki: Artist (sceny), Trio (playout stron), `/output`, API TCP 6100 / HTTP / SSE |
| [VizRS ffPlayout System](../VizRS%20ffPlayout%20System) | **archiwum** (od 2026-10-24): porzucony playout 24/7 (fork ffplayout, GPL-3.0), wzorzec integracji |
| **VizRS CG Playout** (to repo) | ta sama integracja VizRS dla kanałów ErsatzTV, bez zmian w ErsatzTV |

Aplikacje łączy tylko protokół VizRS (TCP 6100, `/output`), opisany w
[API.md](../VizRS%20CG%20System/docs/API.md) systemu VizRS. Konfiguracja
celowo odpowiada ustawieniom grafiki w VizRS ffPlayout System (dziś
archiwum i wzorzec) – szablony komend przenosi się bez zmian.

## Szybki start

```sh
docker compose up -d
```

uruchamia VizRS (`:8788`, Trio pod `/trio`), sidecar (`:7700/health`)
i ErsatzTV (`:8409`). Pierwszy start wypełnia puste wolumeny danymi z `seed/`:
show VizRS i kanał 1 ErsatzTV z grafiką VizRS na odcinkach testowych.
Kolejne kanały: element grafiki **VizRS** w pozycji harmonogramu albo w **Deco**
kanału, potem reset playoutu. Przed otwarciem portów zmień `VIZRS_TOKEN`
i `VIZRS_OUTPUT_TOKEN`.

## Dokumentacja

| Dokument | Treść |
|---|---|
| [docs/DOKUMENTACJA.md](docs/DOKUMENTACJA.md) | działanie, konfiguracja, symbole zastępcze, Docker (Ampere A1, NVENC, VAAPI), awarie, testy, benchmark przechwytywania grafiki (Portainer) |
| [CLAUDE.md](CLAUDE.md) | dla programistów i agentów: zasady, struktura, testy, commity |
