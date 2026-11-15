# VizRS Live – panel kanału nad ErsatzTV legacy

Część rodziny **VizRS**. Polski panel do prowadzenia kanałów telewizyjnych **ErsatzTV**: kanały, ramówka, playouty, EPG, diagnostyka – bez używania web GUI ErsatzTV legacy. Logika (baza, skaner, planowanie, nadawanie przez ErsatzTV Next) zostaje w legacy; VizRS Live ją pokazuje i nią steruje.

> **Stan: 26.11.10** (najnowsze ErsatzTV legacy, sprawdzone z v26.12.0; operator: [docs/INSTRUKCJA.md](docs/INSTRUKCJA.md), administrator: [docs/RUNBOOK.md](docs/RUNBOOK.md)). Działa end-to-end: pełna edycja kanałów, harmonogramów (ramówka klasyczna z pozycjami), playoutów (wszystkie rodzaje, harmonogramy zastępcze, kalendarz tygodnia playoutu blokowego), bloków i szablonów dnia, deco i szablonów deco, bibliotek i mediów (przeglądarka, kosz), kolekcji (zwykłe, inteligentne, wielo-, powtórki), playlist z podglądem, profili kodowania, znaków wodnych, wypełniaczy i logo kanału, ustawień legacy; logi, konserwacja, test odtwarzania i podgląd strumienia kanału, podgląd ramówki i EPG, diagnostyka, konsola komend. Grafika VizRS i sloty na żywo (przez VizRS CG Playout); web GUI legacy zamknięte za proxy (R10, sprawdzone też z legacy v26.12.0); stan prac: [docs/STATUS.md](docs/STATUS.md). Plan: [PLAN-vizrs-live.md](../VizRS%20CG%20System/docs/plany/PLAN-vizrs-live.md) w VizRS CG System. Plan: VizRS Live zostanie połączony z VizRS CG Playout w jeden produkt „VizRS CG Playout” ([sdorpl/VizRS](https://github.com/sdorpl/VizRS)).

## Pakiet VizRS

VizRS Live to jedna z kilku aplikacji pakietu VizRS, które mają być ze sobą **zawsze zgodne**: [VizRS CG System](../VizRS%20CG%20System) (silnik grafiki, Trio, `/output`), [VizRS CG Playout](../VirtRS%20CG%20Playout) (grafika i sloty na żywo na kanałach ErsatzTV), **VizRS Live** (to repo) i [VizRS CG Artist](../VizRS%20CG%20Artist) (edytor grafik i animacji). Styki i zasady zgodności: [DOKUMENTACJA §1](docs/DOKUMENTACJA.md#pakiet-vizrs--zgodność-aplikacji).

## Jak to działa

```
 przeglądarka ──► vizrs-live (Rust, :8090) ──► ErsatzTV legacy (sieć wewnętrzna Dockera)
                  UI panelu + pośrednik         └─ VizRS Bridge (C#, w procesie legacy)
                                                    └─ komendy legacy (MediatR) ──► baza, playout, Next
 widzowie ──► proxy (Caddy, :8409) ──► legacy: tylko /iptv/*, /artwork/*, HDHomeRun (reszta 403)
```

- **VizRS Bridge** (`bridge/`, C#, ~500 linii): wtyczka ładowana do niezmienionego legacy przez `DOTNET_STARTUP_HOOKS` i `ASPNETCORE_HOSTINGSTARTUPASSEMBLIES`. Jeden ogólny dyspozytor: `POST /vizrs-bridge/command/{Nazwa}` wywołuje tę samą komendę legacy, której używa jego UI. Dozwolone komendy wymienia **biała lista** [`bridge/commands.txt`](bridge/commands.txt) – nowa funkcja panelu to zwykle nowy wiersz, bez zmiany kodu C#.
- **vizrs-live** (`server/`, Rust bez zależności): podaje UI panelu i przekazuje komendy do mostu. Token mostu nie trafia do przeglądarki.
- **UI** (`server/web/`, czysty JS bez bibliotek): ekrany na wzór web GUI legacy, po polsku, w stylu VizRS.

## Uruchomienie (Docker)

```bash
cp .env.example .env          # ustaw VIZRS_BRIDGE_TOKEN i VIZRS_LIVE_TOKEN
docker compose up -d --build
```

Panel: `http://<serwer>:8090/` (logowanie tokenem `VIZRS_LIVE_TOKEN`). Widzowie: `http://<serwer>:8409/iptv/channels.m3u` (przez proxy; web GUI legacy z zewnątrz = 403).

Szczegóły, opcje, bezpieczeństwo i ograniczenia: [docs/DOKUMENTACJA.md](docs/DOKUMENTACJA.md).

## Dokumentacja

| Dokument | Dla kogo |
|---|---|
| [Instrukcja operatora](docs/INSTRUKCJA.md) | operator kanału – codzienna praca, przebudowa ramówki, awarie |
| [Pełna dokumentacja](docs/DOKUMENTACJA.md) | wszyscy – most, panel, ekrany, ograniczenia legacy, testy, bezpieczeństwo |
| [Runbook](docs/RUNBOOK.md) | administrator – wdrożenie, kopie, aktualizacja legacy, odtworzenie |
| [Historia zmian](CHANGELOG.md) | wszyscy |
| [Spis wszystkich dokumentów](docs/README.md) | w tym stan prac, znane braki, plany |

## Praca nad kodem

```bash
tests/stack.sh up          # stos testowy na 18409/18410/18090 (nie rusza produkcji) + smoke mostu i proxy
tests/stack.sh seed        # wideo testowe i kanał INFO24 z ramówką – wyłącznie przez panel
PLAYWRIGHT_DIR=… node tests/e2e.cjs     # e2e panelu
tests/stack.sh down
```

Programiści i agenci: [CLAUDE.md](CLAUDE.md) (zasady, struktura, testy, commity; Gemini CLI: [GEMINI.md](GEMINI.md)).

## Licencja

MIT – [LICENSE](LICENSE). Składniki zewnętrzne (fonty, biblioteki, obrazy): [THIRD-PARTY.md](THIRD-PARTY.md). Opis całego pakietu VizRS: [sdorpl/VizRS](https://github.com/sdorpl/VizRS).
Współpracuje z [ErsatzTV](https://ersatztv.org) (legacy: Zlib, Next: MIT) bez zmian w jego kodzie – powered by ErsatzTV. Obraz `vizrs-live-legacy` dokłada tylko plik `VizRS.Bridge.dll` do oficjalnego obrazu.
