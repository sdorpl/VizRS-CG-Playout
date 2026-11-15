#!/bin/sh
# Zasady pakietu VizRS wspólne dla wszystkich repozytoriów – źródło: VizRS CG System.
#   tools/sync-zasady.sh [KATALOG-CG-SYSTEM]          – skopiuj zasady ze źródła do tego repo
#   tools/sync-zasady.sh --check [KATALOG-CG-SYSTEM]  – sprawdź zgodność (CI, bramka); kod 1 = rozjazd
# Domyślnie ../VizRS CG System. Kopiowane: bloki <!-- pakiet:X --> … <!-- /pakiet:X --> w CLAUDE.md (A, S, B, C, D),
# całe GEMINI.md, docs/AGENCI.md i ten skrypt; AGENTS.md = kopia CLAUDE.md (nie dowiązanie). Wersja źródła:
# docs/ZASADY_CG_SYSTEM (commit CG System – CI pobiera go do --check). W samym VizRS CG System skrypt pilnuje
# tylko AGENTS.md = CLAUDE.md. Zmiana zasad pakietu: najpierw CG System, potem ten skrypt w pozostałych repo.
set -e
CHECK=
[ "$1" = --check ] && { CHECK=1; shift; }
TOP="$(cd "$(dirname "$0")/.." && pwd)"
DEF="$TOP/../VizRS CG System"
# w samym VizRS CG System (także w CI, bez katalogu obok) źródłem jest to repo
grep -q '^name = "vizrs"$' "$TOP/source/Cargo.toml" 2>/dev/null && DEF="$TOP"
CG="$(cd "${1:-$DEF}" 2>/dev/null && pwd)" || { echo "Brak źródła CG System: ${1:-$DEF}" >&2; exit 2; }
BLOCKS="A S B C D"
FILES="GEMINI.md docs/AGENCI.md tools/sync-zasady.sh"
[ -f "$CG/CLAUDE.md" ] && [ -f "$CG/source/Cargo.toml" ] || { echo "To nie jest VizRS CG System: $CG" >&2; exit 2; }

# blok X (z liniami znaczników) z pliku $1
block() { awk -v k="$2" 'index($0, "<!-- pakiet:" k " ")==1 || $0=="<!-- pakiet:" k " -->" {p=1} p {print} $0=="<!-- /pakiet:" k " -->" {exit}' "$1"; }

# CLAUDE.md z $1 z blokami podmienionymi na bloki ze źródła → stdout
merged() {
  m="$1"
  for k in $BLOCKS; do
    src=$(mktemp); block "$CG/CLAUDE.md" "$k" > "$src"
    [ -s "$src" ] || { echo "Źródło bez bloku pakiet:$k" >&2; rm -f "$src"; exit 2; }
    grep -q "^<!-- /pakiet:$k -->\$" "$m" || { echo "$TOP/CLAUDE.md: brak bloku pakiet:$k (wstaw znaczniki w miejscu bloku)" >&2; rm -f "$src"; exit 2; }
    out=$(mktemp)
    awk -v k="$k" -v src="$src" '
      index($0, "<!-- pakiet:" k " ")==1 || $0=="<!-- pakiet:" k " -->" { while ((getline l < src) > 0) print l; skip=1; next }
      skip && $0=="<!-- /pakiet:" k " -->" { skip=0; next }
      !skip { print }' "$m" > "$out"
    rm -f "$src"; [ "$m" = "$1" ] || rm -f "$m"; m="$out"
  done
  cat "$m"; [ "$m" = "$1" ] || rm -f "$m"
}

bad=0
fail() { echo "ROZJAZD: $1" >&2; bad=1; }

if [ "$TOP" = "$CG" ]; then
  # źródło: tylko AGENTS.md = CLAUDE.md
  if [ -n "$CHECK" ]; then
    { [ -f "$TOP/AGENTS.md" ] && [ ! -L "$TOP/AGENTS.md" ] && cmp -s "$TOP/CLAUDE.md" "$TOP/AGENTS.md"; } || fail "AGENTS.md ≠ CLAUDE.md (kopia, nie dowiązanie)"
  else
    rm -f "$TOP/AGENTS.md"; cp "$TOP/CLAUDE.md" "$TOP/AGENTS.md"
  fi
else
  tmp=$(mktemp); merged "$TOP/CLAUDE.md" > "$tmp"
  if [ -n "$CHECK" ]; then
    cmp -s "$tmp" "$TOP/CLAUDE.md" || fail "CLAUDE.md – bloki pakietu różnią się od CG System (tools/sync-zasady.sh)"
    { [ -f "$TOP/AGENTS.md" ] && [ ! -L "$TOP/AGENTS.md" ] && cmp -s "$TOP/CLAUDE.md" "$TOP/AGENTS.md"; } || fail "AGENTS.md ≠ CLAUDE.md (kopia, nie dowiązanie)"
    for f in $FILES; do cmp -s "$CG/$f" "$TOP/$f" || fail "$f ≠ CG System"; done
  else
    cat "$tmp" > "$TOP/CLAUDE.md"
    rm -f "$TOP/AGENTS.md"; cp "$TOP/CLAUDE.md" "$TOP/AGENTS.md"
    for f in $FILES; do mkdir -p "$(dirname "$TOP/$f")"; cp "$CG/$f" "$TOP/$f"; done
    chmod +x "$TOP/tools/sync-zasady.sh"
    rev=$(git -C "$CG" rev-parse HEAD 2>/dev/null || true)
    [ -n "$rev" ] && echo "$rev" > "$TOP/docs/ZASADY_CG_SYSTEM"
    echo "Zasady pakietu skopiowane z $CG${rev:+ ($rev)}"
  fi
  rm -f "$tmp"
fi
[ $bad = 0 ] || exit 1
[ -n "$CHECK" ] && echo "Zasady pakietu zgodne z VizRS CG System"
exit 0
