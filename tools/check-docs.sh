#!/bin/sh
# Kontrola dokumentacji stanu – wspólna dla pakietu VizRS (źródło: VizRS CG System, kopie: tools/sync-zasady.sh).
#   tools/check-docs.sh   – w katalogu repo albo z dowolnego miejsca; kod 1 = błąd (CI, bramka przed commitem)
# Sprawdza (zasady bloku A „Dokumentacja stanu”):
#   1. CHANGELOG.md nie jest pusty, ma sekcję „## Niewydane” i nagłówek wersji z manifestu (`## 26.11.13 …`);
#   2. CHANGELOG.md nie zgubił nagłówka żadnej wersji z poprzedniego commita (HEAD~1, gdy jest w historii);
#   3. docs/STATUS.md: „Ostatnia aktualizacja” nie starsza niż data wydania bieżącej wersji w CHANGELOG.
# Wersja: pierwsze `version = "…"` z source/Cargo.toml, a bez niego z Cargo.toml.
set -e
TOP="$(cd "$(dirname "$0")/.." && pwd)"
cd "$TOP"
bad=0
fail() { echo "DOKUMENTACJA: $1" >&2; bad=1; }

CL=CHANGELOG.md
MAN=source/Cargo.toml
[ -f "$MAN" ] || MAN=Cargo.toml
VER=$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' "$MAN" 2>/dev/null | head -n 1)
[ -n "$VER" ] || { echo "Brak wersji w $MAN" >&2; exit 2; }

# nagłówki wersji („## 26.11.13 (data)”, „## 26.11.12 – data”, „## v2.0.1 …”) – sama wersja, po jednej w wierszu
versions() { sed -n 's/^## v\{0,1\}\([0-9][0-9]*\.[0-9][0-9.]*[0-9A-Za-z.-]*\).*/\1/p'; }

if [ ! -s "$CL" ]; then
  fail "$CL jest pusty albo go brak – przywróć historię (git log -p -- $CL)"
else
  grep -q '^## Niewydane' "$CL" || fail "$CL bez sekcji „## Niewydane” na zmiany przed wydaniem"
  esc=$(printf '%s' "$VER" | sed 's/\./\\./g')
  line=$(grep -m 1 -E "^## v?$esc( |$)" "$CL" || true)
  [ -n "$line" ] || fail "$CL bez nagłówka bieżącej wersji $VER z $MAN"
  # 2. nagłówki z poprzedniego commita
  if git rev-parse -q --verify HEAD~1 >/dev/null 2>&1 && git cat-file -e "HEAD~1:$CL" 2>/dev/null; then
    now=$(mktemp); versions < "$CL" | sort -u > "$now"
    for v in $(git show "HEAD~1:$CL" | versions | sort -u); do
      grep -qxF "$v" "$now" || fail "$CL zgubił wpis wersji $v (był w HEAD~1)"
    done
    rm -f "$now"
  fi
  # 3. STATUS nie starszy niż wydanie bieżącej wersji
  rel=$(printf '%s' "$line" | grep -oE '[0-9]{4}-[0-9]{2}-[0-9]{2}' | head -n 1 || true)
  if [ -f docs/STATUS.md ] && [ -n "$rel" ]; then
    st=$(grep -m 1 'Ostatnia aktualizacja' docs/STATUS.md | grep -oE '[0-9]{4}-[0-9]{2}-[0-9]{2}' | head -n 1 || true)
    if [ -z "$st" ]; then
      fail "docs/STATUS.md bez daty „Ostatnia aktualizacja: RRRR-MM-DD”"
    elif [ "$(printf '%s\n%s\n' "$st" "$rel" | sort | head -n 1)" != "$rel" ]; then
      fail "docs/STATUS.md z $st, a wydanie $VER z $rel – uaktualnij bieżący etap i „Następne”"
    fi
  fi
fi

[ $bad = 0 ] || exit 1
echo "Dokumentacja stanu zgodna z wersją $VER"
