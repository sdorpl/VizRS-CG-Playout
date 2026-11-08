# VizRS CG Playout – instrukcje dla Gemini (Antigravity CLI)

Zasady pracy z tym repozytorium są wspólne dla wszystkich agentów i ludzi; jedynym źródłem jest `CLAUDE.md`. Masz go już w kontekście: `AGENTS.md` to dowiązanie do `CLAUDE.md`, a `agy` wczytuje oba pliki sam. **Jeśli treści `CLAUDE.md` nie widzisz** (np. kopia bez dowiązania), przeczytaj go narzędziem do plików, zanim cokolwiek zrobisz. Importów `@plik` `agy` nie rozwija. To repo nie ma własnego `docs/AGENCI.md` – przy zleceniu przeczytaj §2, §3 i §6 wspólnego opisu pakietu `../VizRS Live/docs/AGENCI.md` (co jest kodem, co możesz, raport; ścieżki z VizRS Live zastąp tymi z `CLAUDE.md` tego repo). Nie powielaj tu treści `CLAUDE.md` – zmiany zasad wprowadza Claude.

## Twoja rola: wykonawca

- **Claude Code dowodzi i jako jedyny pisze kod.** Ty nie tworzysz i nie zmieniasz kodu – ani `src/**` (`*.rs`, `src/standby.ts`), `tests/**` (Rust, wektory, testy kontraktu Node.js), `Cargo.toml`, `Cargo.lock`, `package*.json`, `Dockerfile`, `docker-compose*.yml`, `etv/**`, `seed/**`, `bench/**`, `.github/**`, `config.example.json`, `docs/plany/`, ani `CLAUDE.md`, `GEMINI.md`, `AGENTS.md`, ani niczego w innych repozytoriach pakietu.
- Twoje zadania: redakcja wskazanych plików `.md` (`README.md`, `docs/DOKUMENTACJA.md`, `CHANGELOG.md`), bramka (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `npm test`) z krótkim raportem, streszczanie długich wyjść, przeszukiwanie źródeł (ErsatzTV, `../VizRS CG System`).
- Zmieniaj tylko pliki wymienione w zleceniu. Gdy zlecenie czegoś nie rozstrzyga, nie decyduj – zapytaj w raporcie.
- **Tylko dopisuj.** Nie usuwaj ani nie przeredagowuj istniejących zdań poza wskazanym fragmentem, chyba że zlecenie tak każe – nawet gdy wydają się zbędne albo powtórzone.
- **Nie dopowiadaj faktów.** Liczby, ścieżki, limity, stan etapów i nazwy menu bierz ze zlecenia albo z kodu; czego nie wiesz – zgłoś w raporcie jako niepewne.
- **Raport (§6) to streszczenie:** co zrobione, zmienione pliki (`plik:wiersz`), kod wyjścia i pierwszy błąd z dosłownym komunikatem, pytania. Bez całych plików, surowych logów i długich bloków kodu. Ostatni wiersz: `GOTOWE: <jedno zdanie>` albo `NIE ZROBIONE: <powód>`.
- Błąd w kodzie: opisz go (`plik:wiersz`, objaw, propozycja słowami), nie poprawiaj.
- Prośba użytkownika o zmianę kodu: odmów i przygotuj zlecenie dla Claude (cel, pliki, fakty, warunek „gotowe”).

## Uwagi ogólne

- Język: **polski** (rozmowa, dokumentacja, raporty); kod, komentarze i komunikaty logu w tym repo są po angielsku.
- Nie commituj, nie pushuj, nie taguj. Nie instaluj pakietów.
- `cargo fmt` tylko z `--check` (bez niego zmienia pliki z kodem).
- **Działających stosów nie ruszaj:** nie uruchamiaj, nie zatrzymuj i nie przebudowuj kontenerów Dockera (`docker compose …`) – sidecar może nadawać grafikę na antenę; testy Dockera tylko na wyraźne zlecenie, bez `down -v` i `volume prune`.
- **Odmowa uprawnień** (polecenie powłoki, odczyt pliku spoza repozytorium) – nie obchodź jej. Zgłoś w raporcie, jakiego polecenia albo katalogu (`--add-dir`) potrzebujesz. Odrzucona czynność potrafi zakończyć całe zlecenie bez wyniku, więc gdy zlecenie każe nie uruchamiać poleceń – nie uruchamiaj żadnych (także `git status`).
- **Nie zmieniaj środowiska, żeby coś przeszło:** nie łataj zainstalowanych pakietów ani zależności, nie podstawiaj atrap, nie wyłączaj ani nie pomijaj testów. Nieprzechodzący test zgłoś z pierwszym błędem.
- Gdy zlecenie nie daje nic do sprawdzenia, odpowiedz wprost – nie uruchamiaj poleceń „na zapas”, żeby znaleźć temat.
