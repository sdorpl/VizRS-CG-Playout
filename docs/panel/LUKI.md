# LUKI – znane braki (uszeregowane: niezawodność → wierność → funkcje)

Kolejność realizacji luk razem z planem rozwoju: [plany/PLAN-mcr.md](plany/PLAN-mcr.md) §4 („Jeden przebieg”); luki spoza przebiegu są tam w tabeli z powodem. Zamykając pozycję, usuń wiersz i dopisz zmianę w `CHANGELOG.md` („Niewydane”). Nowe braki dopisuj od razu, z kolejnym wolnym numerem (numerów nie używamy ponownie). Opis działania i uzasadnienia decyzji są w [DOKUMENTACJA.md](DOKUMENTACJA.md) – tu tylko krótko i odsyłacz.

Ograniczenia samego legacy, których panel nie obejdzie bez zmiany legacy (np. jeden serwer Jellyfin/Emby, `GetAllLanguageCodes` = `und`), są w DOKUMENTACJA §5, nie tutaj.

## G1 – niezawodność i bezpieczeństwo

| # | Luka | Uwagi |
|---|---|---|
| 1 | Brak TLS w panelu i w proxy dla widzów | zamierzone (serwer bez zależności); LAN/VPN albo zewnętrzne proxy TLS – RUNBOOK; ciasteczko dostaje `Secure` przy `X-Forwarded-Proto: https` (DOKUMENTACJA §9) |
| 3 | Sloty na żywo nie sprawdzone z prawdziwym sygnałem (OBS → CG Playout), Plex nie sprawdzony z prawdziwym kontem | e2e na atrapach; `tests/real.sh` sprawdził Jellyfin, Emby i reguły grafiki na prawdziwym CG (DOKUMENTACJA §7); krok ręczny przed wdrożeniem – STATUS; PLAN-mcr Q1 (ręcznie) |
| 5 | Panel nie bierze blokady biblioteki legacy (`IEntityLocker`, poza MediatR) | drugie zlecenie synchronizacji tej samej biblioteki trafia do kolejki (DOKUMENTACJA §4, R8) |

## G2 – wierność wobec web GUI legacy

| # | Luka | Uwagi |
|---|---|---|
| 8 | Test odtwarzania pliku z wyborem profilu (jak w Blazorze legacy) – panel ma tylko podgląd kanału | legacy przekazuje między krokami obiekty wewnętrzne (DOKUMENTACJA §5) |
| 9 | Brak postępu skanu biblioteki | legacy podaje go tylko zdarzeniami Blazora; panel przez 30 s odświeża liczby pozycji (DOKUMENTACJA §4 „Media”) |
| 12 | Plex: panel nie wie, czy konto jest zalogowane | legacy trzyma to w `IPlexSecretStore` poza MediatR; „Wyloguj” tylko, gdy są serwery |
| 25 | Walidator YAML harmonogramu sekwencyjnego | poza MediatR (`ISequentialScheduleValidator`); razem z #14 (diagnostyka playoutu blokowego – zrobiona w K7) |
| 27 | Paczka diagnostyczna legacy (`ArchiveTroubleshootingResults`, `ArchiveMediaSample`) | ZIP powstaje tylko na dysku legacy, pobranie wymagałoby nowego endpointu C# (DOKUMENTACJA §4, decyzja K5) |

## G3 – funkcje

| # | Luka | Uwagi |
|---|---|---|
| 14 | Panel nie edytuje plików YAML, skryptów i JSON playoutów ani `.yml` strumieni zdalnych | decyzja R2; plik na wolumen `/config`, w panelu ścieżka; ewentualnie jeden ogólny endpoint zapisu w moście ograniczony do jednego katalogu (DOKUMENTACJA §4) |
| 15 | Pozycję harmonogramu audycji LIVE operator dodaje ręcznie w Harmonogramach | zamierzone (zapis harmonogramu z dwóch miejsc groziłby nadpisaniem); kreator Audycji LIVE prowadzi krok po kroku |
| 16 | Trakt Lists (`AddTraktList`, `DeleteTraktList`) | wymaga klucza API Trakt; pominięte w liście pokrycia (DOKUMENTACJA §8) |
| 17 | Własna kolejność kolekcji (`UpdateCollectionCustomOrder`) | legacy pozwala tylko dla kolekcji z samych filmów (DOKUMENTACJA §4 R4) |
| 19 | Głęboki skan źródeł zewnętrznych | pominięty (rzadko potrzebny, długo trwa) – DOKUMENTACJA §4 R8 |
| 20 | Elementy graficzne legacy (YAML): tylko lista i „Wczytaj pliki elementów” | element VizRS zakłada CG Playout; patrz #14 |
