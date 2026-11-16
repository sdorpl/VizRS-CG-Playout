# Współpraca agentów: Claude i Gemini (pakiet VizRS)

Zasady podziału pracy między Claude Code i Gemini (Antigravity CLI) w pakiecie VizRS. Cel: tokeny Claude idą na myślenie i kod, a zadania lekkie i wykonawcze (dokumentacja, kompilacja, długie wyjścia) bierze Gemini. Plik jest wspólny dla wszystkich repozytoriów pakietu VizRS (VizRS CG System, VizRS CG Artist, VizRS CG Playout z panelem – dawnym VizRS Live), identyczny w każdym; źródłem jest VizRS CG System, a kopie aktualizuje `tools/sync-zasady.sh` (nie edytuje się ich ręcznie). Zasady ogólne repozytorium są w [../CLAUDE.md](../CLAUDE.md) i obowiązują obu agentów. Ten plik dodaje tylko podział ról.

## 1. Role

| | **Claude – dowódca** | **Gemini – wykonawca** |
|---|---|---|
| Decyzje | planuje, wybiera rozwiązanie, ustala zakres zadania, rozstrzyga wątpliwości | nie podejmuje decyzji projektowych; przy wątpliwości pyta w raporcie |
| Kod | **jedyny, który pisze i zmienia kod** | **nie pisze i nie zmienia kodu**, nawet „małej poprawki” |
| Dokumentacja | wyznacza, co i gdzie opisać; przegląda wynik | pisze i redaguje tekst `.md` według zlecenia |
| Kompilacja, testy | zleca, czyta raport, naprawia | uruchamia, streszcza wynik, wskazuje miejsce błędu |
| Commit | przegląda zmiany, robi commit (na prośbę użytkownika) | nie commituje, nie pushuje, nie taguje |

**Ostatnie słowo ma Claude** (a nad nim użytkownik). Wynik Gemini jest propozycją, dopóki Claude go nie przejrzy.

## 2. Co jest kodem (tylko Claude)

Kodem jest wszystko, co wpływa na działanie albo budowanie. Co jest kodem w danym repozytorium, określa `CLAUDE.md` → „Współpraca z Gemini” → „Ten projekt” (sekcja „Kod”).

Część wspólna dla wszystkich repozytoriów pakietu:

- pliki zasad (`CLAUDE.md`, `AGENTS.md`, `GEMINI.md`, `docs/AGENCI.md`),
- `.github/**`,
- pliki Dockera i compose,
- skrypty, testy,
- manifesty (`Cargo.toml`, `Cargo.lock`, `*.csproj`, `package.json`, `tauri.conf.json`),
- pliki licencji (`LICENSE`, `THIRD-PARTY.md`),
- białe listy (granice bezpieczeństwa),
- decyzje w planach (`docs/plany/` – nowy plan albo decyzję pisze Claude; Gemini może redagować wskazany plan, gdy zlecenie podaje fakty),
- cokolwiek w sąsiednich repozytoriach pakietu – tylko do czytania.

Gemini może te pliki **czytać**. Gdy zobaczy w nich błąd, opisuje go w raporcie (`plik:wiersz`, objaw, propozycja słowami) – nie wysyła łatki.

## 3. Co może Gemini

| Zadanie | Zakres |
|---|---|
| Dokumentacja | pliki z listy „Dokumentacja” w `CLAUDE.md` → „Ten projekt” – tylko pliki wskazane w zleceniu, zgodnie z zasadą „Gdzie co opisywać” w `CLAUDE.md`; redagowanie wskazanego planu w `docs/plany/` na podstawie podanych faktów |
| Kompilacja i bramka | polecenia z listy „Bramka” w `CLAUDE.md` → „Ten projekt”; `cargo fmt` tylko z `--check` (bez zapisu) – uruchomić i streścić |
| Długie wyjścia | streszczenie logów serwera, okna, kontenerów, wyników testów (w tym E2E), narzędzi porównawczych |
| Wyszukiwanie | przegląd źródeł w repozytorium i sąsiednich repozytoriach pakietu, grep po repo – raport z odnośnikami `plik:wiersz` |
| Kontrola tekstu | spójność dokumentacji z kodem (menu, skróty, opcje CLI, trasy API), literówki, martwe odnośniki – zmiany tylko w plikach dokumentacji projektu |
| Zrzuty UI | opis zrzutów ekranu, które wykonał Claude (pliki PNG poza repo) |

Zawsze obowiązuje: serwery i okna uruchamiaj tylko na kopii danych i innym porcie; nie zatrzymuj działających usług, stosów ani kontenerów (mogą nadawać na antenę); lista ograniczeń projektu – „Zawsze” w `CLAUDE.md` → „Współpraca z Gemini” → „Ten projekt”; żadnego `git push`, tagów ani instalacji pakietów.

## 4. Kiedy Claude zleca, a kiedy robi sam

Tokeny Claude zużywa to, co trafia do jego kontekstu (i jest potem ponownie wysyłane w każdym kolejnym kroku sesji), oraz tekst, który Claude pisze. Najtańszy jest więc **filtr w powłoce**, nie agent.

**Logi zawsze przez plik i filtr (Claude, bez Gemini):**

```bash
<bramka projektu> > "$S/bramka.log" 2>&1; echo "kod: $?"; tail -3 "$S/bramka.log"   # zielono: kod i ogon
grep -n -A3 '^error\|^warning\|FAILED\|panicked\|^FAIL' "$S/bramka.log" | head -80   # czerwono: same błędy
```

Do kontekstu trafia tylko kod wyjścia, ogon albo odfiltrowane błędy – nigdy cały log. Błędy kompilacji i testów Claude czyta sam (i tak musi zobaczyć każde miejsce, żeby je poprawić), streszczenie „pierwszy błąd” nie wystarcza.

**Zlecaj Gemini**, gdy zadanie daje dużo tekstu przy małej liczbie decyzji i nie da się go zastąpić filtrem:

- **proza dokumentacji** do gotowej zmiany kodu (Claude podaje fakty, Gemini pisze, Claude przegląda tylko diff) – oszczędza drogie tokeny wyjściowe Claude; dokumentacja bywa długa, więc zysk jest tu największy,
- **przeszukiwanie dużych źródeł**, gdy Claude musiałby czytać dziesiątki plików – raport ze ścisłym formatem,
- **długie logi bez struktury**, których nie da się sensownie przefiltrować (logi WebView/przeglądarki, serwera, kontenerów) – Claude zapisuje log do pliku, Gemini tylko czyta.

**Rób sam**, gdy:

- wystarczy jedno, dwa wywołania narzędzi albo `grep` po repo (zlecenie, czekanie i przegląd kosztują więcej),
- trzeba zdecydować (projekt, architektura, bezpieczeństwo, białe listy, formaty danych, API),
- wynik trafia do kodu.

**Pomiar (2026-11-07, `agy`, ok. 3,3 znaku na token):**

| Zadanie | Claude sam, cały wynik | Przez Gemini | Claude sam, z filtrem |
|---|---|---|---|
| bramka zielona (64 s) | 11,6 KB ≈ 3,5 tys. tok. | ≈ 300 tok. (zlecenie + raport) | `tail -3`: ≈ 15 tok. |
| clippy czerwony (14 błędów po zmianie sygnatury) | 7,7 KB ≈ 2,3 tys. tok. | streszczenie niewystarczające do poprawki | `grep -A2 '^error'`: ≈ 450 tok. |
| zgodność DOKUMENTACJA §4 z `VIEWS` | 1 wywołanie, ≈ 1,4 tys. tok., ~1 s | raport ≈ 1,5 tys. tok., **263 s**, 13 z 14 wierszy to szum | – |

Wniosek: przy logach filtr jest tańszy od zlecenia. Zysk z Gemini to proza dokumentacji i duże przeszukiwania – rzędu kilku procent tokenów sesji. Każde zlecenie ma koszt stały (ten plik ≈ 2,4 tys. tok., zlecenie, przegląd diffu) i trwa od 10 s do kilku minut. Raport sprawdzaj wyrywkowo – bywa zaszumiony.

## 5. Zlecenie (Claude → Gemini)

„Gemini” to **Antigravity CLI** (`agy`), zainstalowany w `~/.gemini/bin/agy` i **nie ma go w `PATH`**. Działa na koncie Google użytkownika. Claude uruchamia `agy` bez trybu interaktywnego (`-p`) z katalogu repozytorium.

**Gemini CLI (`gemini`) nie używamy.** Od 2026-06-18 Google nie obsługuje w nim kont prywatnych (darmowe, AI Pro, AI Ultra – logowanie kończy się `UNSUPPORTED_CLIENT`); zostały tylko płatne klucze API z przedpłatą. Decyzja użytkownika (2026-11-07): korzystamy wyłącznie z wykupionej subskrypcji Google, bez dopłat i kredytów API – więc jedynym wykonawcą jest `agy`. Model: domyślnie Gemini 3.8 Flash (High) (`--model "Gemini 3.8 Flash (High)"`); dostępne modele: `agy models`. Poziomu `pro` wtyczki antigravity w tej subskrypcji nie ma (2026-11-15). Wybór na sesję: `--model <id>`. Streszczenia i proste raporty: `--model gemini-3.8-flash-low` (pomiar: ten sam poprawny wynik, 10,5 s zamiast 25,4 s na High); proza dokumentacji i przeszukiwanie źródeł: domyślny High.

```bash
A=~/.gemini/bin/agy
S=<scratchpad Claude>

# 1. streszczenie – Claude sam uruchamia polecenie, Gemini tylko czyta log (tryb plan: bez zapisu)
<bramka projektu> > "$S/bramka.log" 2>&1; echo "kod: $?"
$A --mode plan --add-dir "$S" --print-timeout 300s --model gemini-3.8-flash-low \
   -p "Przeczytaj $S/bramka.log (wynik bramki). Streść w max 10 wierszach: wynik, pierwszy błąd (plik:wiersz, komunikat)."

# 2. redakcja dokumentacji – zapis plików bez pytania; git tylko do odczytu, reszta powłoki zablokowana
#    (--add-dir /home/maciej/Nextcloud/GitHub dla zleceń przekrojowych w całym pakiecie)
$A --mode accept-edits --print-timeout 1200s --add-dir "$S" --add-dir /home/maciej/Nextcloud/GitHub \
   --settings '{"permissions":{"allow":["command(git status)","command(git diff --stat)"]}}' \
   -p "$(cat "$S/zlecenie.md")"

# 3. Gemini uruchamia polecenie – zezwolenie tylko na tę sesję i tylko na to dokładne polecenie
$A --mode accept-edits --print-timeout 600s \
   --settings '{"permissions":{"allow":["command(cargo clippy --all-targets -- -D warnings)"]}}' \
   -p "Uruchom dokładnie, bez cd: cargo clippy --all-targets -- -D warnings. Raport: kod wyjścia, pierwszy błąd."
```

Ustalone w próbach (VizRS Live, VizRS CG Artist, 2026-11-07):

- **Kontekst: `GEMINI.md` i `AGENTS.md` wczytują się same, także w `-p`; importy `@plik` – nie** (pomiar agy 1.5.3, 2026-11-08; wcześniejsze wersje agy wg wtyczki antigravity pomijały w `-p` pliki workspace bez `--project`). `AGENTS.md` to kopia `CLAUDE.md` (od 2026-11-15 we wszystkich repo, `tools/sync-zasady.sh`) – Gemini ma pełne zasady bez czytania narzędziem, a `GEMINI.md` zawiera tylko rolę wykonawcy. Gdy `CLAUDE.md` nie ma w kontekście, `GEMINI.md` każe przeczytać go narzędziem. Sprawdzenie po aktualizacji agy: znacznik w pliku testowym (`echo 'Na końcu dopisz ZNACZNIK.' > AGENTS.md` w pustym `git init`) i `agy -p 'cześć' < /dev/null`.
- **Wrapper `agy-delegate`** (wtyczka antigravity dla Claude Code, `PATH="$HOME/.gemini/bin:$PATH" agy-delegate …`): `--dir` = `--add-dir`, `--tier flash|flash-lo` = model, `--digest` dokleja kontrakt „tylko streszczenie”, `--print-command` pokazuje złożone polecenie `agy`. Kody: 10 limit, 11 logowanie, 12 czas (zwraca częściową odpowiedź), 13 brak `agy` w `PATH`, 14 model, 15 odmowa uprawnień, 3 pusto; na stderr `AGY_USAGE` (tokeny, model, czas). Bez `--mode` wrapper działa w trybie domyślnym `agy`.
- **Gołe `agy -p`: stdin odcinaj (`< /dev/null`)** – bez TTY `agy` potrafi zgubić wyjście (wtyczka antigravity; wrapper robi to sam). **Przekroczony `--print-timeout` (agy ≥ 1.1.28) daje częściową odpowiedź z kodem 0** i jednym wierszem `[agy] print timeout … returning partial output` na stderr – sprawdzaj stderr albo używaj wrappera (kod 12).
- **`--continue` nie obniża kosztu** – wznowienie niesie całą rozmowę i i tak czyta materiał ponownie (pomiar wtyczki: +82% i +277% wobec świeżego wywołania). Tylko do dokończenia po błędzie (limit, czas).
- **Stdin jest ignorowany** (`-p … < log` daje „stdin jest pusty”). Log przekazuj jako plik do przeczytania (wzór 1, katalog spoza repo przez `--add-dir`) albo wklej do treści promptu, gdy jest krótki.
- **Kolejność przełączników:** `-p` bierze następny argument jako prompt, więc stawiaj go na końcu (albo `-p='…'`).
- **Powłoka w trybie `-p` jest domyślnie zablokowana** – każde polecenie odrzucane bez pytania. Zezwolenie daje `--settings` z regułą `command(<polecenie>)`; działa przy **dokładnie** tym samym tekście polecenia (bez `cd … &&`, bez `eval`). Zezwolenia nie zapisuj na stałe w ustawieniach `agy`.
- **Odrzucone polecenie kończy całe zlecenie bez wyniku** („no output produced … auto-denied”). Gemini lubi zacząć od `git status` / `git diff` – zezwól na nie przy redakcji dokumentacji (wzór 2: `--settings '{"permissions":{"allow":["command(git status)","command(git diff --stat)"]}}'`) albo napisz w zleceniu, że ma nie uruchamiać poleceń. **Pomiar 2026-11-15:** mimo zezwolenia na `git status` i `git diff --stat` 3 z 5 zleceń redakcji w pakiecie przepadły bez wyniku – Gemini sięgał po inne polecenia (`grep`, `ls`). Pewniejsze: w zleceniu redakcji wprost „NIE uruchamiaj ŻADNYCH poleceń powłoki – czytaj i edytuj wyłącznie narzędziami do plików” (ponowione zlecenia z tym zdaniem przeszły).
- **Gemini dopowiada fakty**, których nie ma w zleceniu ani w kodzie (2026-11-07: zmyślone ścieżki na Windows/macOS, inny limit czasu klienta, „etapy wykonane”, których nie wykonano). W zleceniu podawaj fakty wprost, a przy przeglądzie sprawdzaj każdą liczbę, ścieżkę i stan etapu z kodem.
- **Odczyt poza katalogiem roboczym i `--add-dir` też jest odrzucany** – zlecenie kończy się bez wyniku („a tool required the "read_file" permission … auto-denied”). Reguła `--settings` `read_file` bez celu nie pomaga. Dodaj `--add-dir` dla każdego katalogu, który Gemini ma czytać: wzory z innego repo, sąsiednie repozytoria przy odnośnikach (zlecenia dla całego pakietu VizRS: `--add-dir /home/maciej/Nextcloud/GitHub`).
- **Kod wyjścia 0 nie oznacza wykonania.** Przy „no output produced” `agy` też kończy się kodem 0 – zawsze sprawdzaj raport (`grep -l 'no output produced' "$S"/raport-*`) i `git status`.
- **To samo zlecenie dla kilku repozytoriów** (np. wspólna sekcja w całym pakiecie): jeden plik zlecenia w `$S`, jedno `agy` na repo uruchomione z katalogu tego repo, równolegle (`( cd "$repo" && $A … ) &` … `wait`) – 3 repozytoria ok. 2–4 min razem. Fakty o styku podaj w zleceniu dla wszystkich stron naraz; każde repo przejrzyj i commituj osobno.
- **Przy uzupełnianiu sekcji Gemini usuwa sąsiednie zdania**, które uzna za zbędne (2026-11-08: zdanie o protokole VizRS w README CG Playout). W zleceniu pisz „nie usuwaj istniejących zdań, tylko dopisuj”, a w przeglądzie czytaj wiersze usunięte: `git diff | grep '^-[^-]'`.
- **`--dangerously-skip-permissions` nie używamy** (to odpowiednik `--yolo`).
- **Odmowa zmiany kodu to zachowanie modelu, nie blokada narzędzia** (w próbie w trybie `accept-edits` prośba o zmianę pliku kodu skończyła się odmową i przygotowaniem zlecenia dla Claude wg §8 – plik pozostał nietknięty) – dlatego przegląd z §7 jest obowiązkowy.
- **Czas zlecenia:** jedno zlecenie trwa od ok. 10 s do kilku minut (proste streszczenie 10–35 s).

Zlecenie zawiera zawsze:

1. **Cel** – jedno zdanie.
2. **Pliki, które wolno zmienić** – jawna lista (dla zadań „tylko raport”: „żadnych”).
3. **Fakty** – co zmieniło się w kodzie (commit, diff, `plik:wiersz`); Gemini nie zgaduje działania, opisuje to, co podał Claude albo co wyczytał z kodu.
4. **Warunek „gotowe”** i **format raportu** (długość, odnośniki `plik:wiersz`).

## 6. Raport (Gemini → Claude)

Krótko, po polsku:

- co zrobione; zmienione pliki,
- wynik poleceń: kod wyjścia, skrót, pierwszy błąd z `plik:wiersz` i dosłownym komunikatem (nie cały log),
- czego nie zrobiono i dlaczego; pytania do Claude.

Bez powtarzania zlecenia i bez prozy wokół wyniku.

## 7. Przegląd po zleceniu (Claude)

- Przed zleceniem zapisz migawkę `git status --porcelain` – drzewo robocze często ma niezacommitowaną pracę.
- Gdy pliki ze zlecenia mają już niezacommitowane zmiany, zapisz też `git diff > "$S/przed.diff"`: po zleceniu oddzielisz cudzą pracę od zmian Gemini, a commit obejmie tylko te drugie (sam fragment pliku: `git apply --cached` na wyciętym kawałku diffu, sprawdź `git diff --cached`).
- Po zleceniu porównaj z migawką: zmienione mogą być tylko pliki ze zlecenia. Zmianę w kodzie albo w nieprzewidzianym pliku przejrzyj i usuń ręcznie; **nie** cofaj na ślepo `git checkout`/`git restore` (zniszczyłoby cudzą pracę w tym samym pliku).
- Przejrzyj `git diff` dokumentacji: zgodność z kodem (§5 – dopowiedziane fakty), zasady z `CLAUDE.md` (zasada „Gdzie co opisywać”: opis działania tylko w `DOKUMENTACJA.md`, README tylko wprowadza).
- Wynik bramki z raportu przy commicie potwierdź – ostatni przebieg przed commitem musi być zielony.

## 8. Sesja Gemini uruchomiona przez użytkownika

Gdy użytkownik pracuje z Gemini bezpośrednio, obowiązuje ten sam podział. Na prośbę o zmianę kodu Gemini odmawia i zamiast tego przygotowuje **zlecenie dla Claude**: cel, pliki, fakty (`plik:wiersz`), warunek „gotowe” – do wklejenia w sesję Claude Code. Dokumentację, kompilację i testy wykonuje sam, wg §3.
