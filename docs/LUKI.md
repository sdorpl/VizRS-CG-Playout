# LUKI – znane braki

> Uszeregowane G1 (niezawodność i bezpieczeństwo) → G2 (wierność) → G3 (funkcje). Zamknięta luka = usunięty wiersz (w commicie, który ją zamyka). Opis działania i uzasadnienia decyzji są w [DOKUMENTACJA.md](DOKUMENTACJA.md) i [API.md](API.md). Plan rozwoju panelu: [plany/PLAN-mcr.md](plany/PLAN-mcr.md).
>
> Ograniczenia samego legacy, których panel nie obejdzie bez zmiany legacy (np. jeden serwer Jellyfin/Emby, `GetAllLanguageCodes` = `und`), są w [DOKUMENTACJA.md §13](DOKUMENTACJA.md#13-znane-ograniczenia-legacy-v26120), nie tutaj.

## Sidecar

| # | Grupa | Luka | Stan / uwagi |
|---|---|---|---|
| 2 | G2 | Jeden sygnał live na kanał: drugi dostaje 409, aktywne przejęcie nie jest wywłaszczane – brak priorytetów odbiorników | [DOKUMENTACJA.md §1](DOKUMENTACJA.md#1-jak-to-działa) oraz [API.md §1](API.md) |
| 3 | G2 | Renderer natywny (`vizrs-render`) nie rysuje wszystkiego (np. ruch 3D, maska ukośna, żaluzja, efekty tekstu, pętle i tło `node.motion`, glitch/RGB, WebP animowane, wideo, fonty spoza wbudowanych; ruch 2D i WebP nieruchome – od 26.11.30) – w trybie `auto` rysuje wtedy Chromium (więcej CPU) | [DOKUMENTACJA.md §1](DOKUMENTACJA.md#1-jak-to-działa) |
| 4 | G2 | Dokładne granice stron (`vizrs-render bounds`) wymagają VizRS CG System ≥ 26.11.2; ze starszym strefy grafiki są uczone z pikseli klatek | [DOKUMENTACJA.md §1](DOKUMENTACJA.md#1-jak-to-działa) |

## G1 – niezawodność i bezpieczeństwo (panel)

| # | Luka | Uwagi |
|---|---|---|
| 1 | Brak TLS w panelu i w proxy dla widzów | zamierzone (serwer bez zależności); LAN/VPN albo zewnętrzne proxy TLS – [RUNBOOK.md §2](RUNBOOK.md#2-wdrożenie); ciasteczko dostaje `Secure` przy `X-Forwarded-Proto: https` ([RUNBOOK.md §7](RUNBOOK.md#7-wydanie-nowej-wersji-opiekun-repozytorium)) |
| 5 | Panel nie bierze blokady biblioteki legacy (`IEntityLocker`, poza MediatR) | drugie zlecenie synchronizacji tej samej biblioteki trafia do kolejki ([DOKUMENTACJA.md §12](DOKUMENTACJA.md#12-ekrany-panelu), R8) |

## G2 – wierność wobec web GUI legacy (panel)

| # | Luka | Uwagi |
|---|---|---|
| 8 | Test odtwarzania pliku z wyborem profilu (jak w Blazorze legacy) – panel ma tylko podgląd kanału | legacy przekazuje między krokami obiekty wewnętrzne ([DOKUMENTACJA.md §13](DOKUMENTACJA.md#13-znane-ograniczenia-legacy-v26120)) |
| 9 | Brak postępu skanu biblioteki | legacy podaje go tylko zdarzeniami Blazora; panel przez 30 s odświeża liczby pozycji ([DOKUMENTACJA.md §12](DOKUMENTACJA.md#12-ekrany-panelu) „Media”) |
| 12 | Plex: panel nie wie, czy konto jest zalogowane | legacy trzyma to w `IPlexSecretStore` poza MediatR; „Wyloguj” tylko, gdy są serwery |
| 25 | Walidator YAML harmonogramu sekwencyjnego | poza MediatR (`ISequentialScheduleValidator`); razem z #14 (diagnostyka playoutu blokowego – zrobiona w K7) |
| 27 | Paczka diagnostyczna legacy (`ArchiveTroubleshootingResults`, `ArchiveMediaSample`) | ZIP powstaje tylko na dysku legacy, pobranie wymagałoby nowego endpointu C# ([DOKUMENTACJA.md §12](DOKUMENTACJA.md#12-ekrany-panelu), decyzja K5) |

## G3 – funkcje (panel)

| # | Luka | Uwagi |
|---|---|---|
| 14 | Panel nie edytuje plików YAML, skryptów i JSON playoutów ani `.yml` strumieni zdalnych | decyzja R2; plik na wolumen `/config`, w panelu ścieżka; ewentualnie jeden ogólny endpoint zapisu w moście ograniczony do jednego katalogu ([DOKUMENTACJA.md §12](DOKUMENTACJA.md#12-ekrany-panelu)) |
| 15 | Pozycję harmonogramu audycji LIVE operator dodaje ręcznie w Harmonogramach | zamierzone (zapis harmonogramu z dwóch miejsc groziłby nadpisaniem); kreator Audycji LIVE prowadzi krok po kroku |
| 16 | Trakt Lists (`AddTraktList`, `DeleteTraktList`) | wymaga klucza API Trakt; pominięte w liście pokrycia ([DOKUMENTACJA.md §15](DOKUMENTACJA.md#15-zamknięcie-web-gui-legacy-r10)) |
| 17 | Własna kolejność kolekcji (`UpdateCollectionCustomOrder`) | legacy pozwala tylko dla kolekcji z samych filmów ([DOKUMENTACJA.md §12](DOKUMENTACJA.md#12-ekrany-panelu) R4) |
| 19 | Głęboki skan źródeł zewnętrznych | pominięty (rzadko potrzebny, długo trwa) – [DOKUMENTACJA.md §12](DOKUMENTACJA.md#12-ekrany-panelu) R8 |
| 20 | Elementy graficzne legacy (YAML): tylko lista i „Wczytaj pliki elementów” | element VizRS zakłada CG Playout; patrz #14 |
