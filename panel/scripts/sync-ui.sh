#!/bin/sh
# Wspólny rdzeń wyglądu rodziny VizRS (vizrs-ui.css) – kopia ze źródła w VizRS CG System (source/web/vizrs-ui.css) –
# i moduły okna wspólne dla okien pakietu (desktop/src/gpu.rs, scale.rs, screen.rs ← source/shell/src/).
#   scripts/sync-ui.sh [KATALOG-CG-SYSTEM]          – skopiuj vizrs-ui.css do server/web/ i zapisz wersję źródła
#   scripts/sync-ui.sh --check [KATALOG-CG-SYSTEM]  – sprawdź, że server/web/vizrs-ui.css jest identyczny ze źródłem (CI, deploy.sh)
# Domyślnie ../../VizRS CG System (repo obok repozytorium VizRS CG Playout). Wersja źródła: server/web/CG_SYSTEM (commit CG System; CI pobiera ten stan do --check).
# Zmiana wyglądu wspólnego zawsze najpierw w CG System, potem tu ten skrypt; style własne panelu – server/web/style.css.
set -e
CHECK=
[ "$1" = --check ] && { CHECK=1; shift; }
TOP="$(cd "$(dirname "$0")/.." && pwd)"
CG="${1:-$TOP/../../VizRS CG System}"
SRC="$CG/source/web/vizrs-ui.css"
DST="$TOP/server/web/vizrs-ui.css"
[ -f "$SRC" ] || { echo "sync-ui: brak źródła $SRC" >&2; exit 2; }
WINDOW="gpu.rs scale.rs screen.rs"
if [ -n "$CHECK" ]; then
  cmp -s "$SRC" "$DST" || { echo "sync-ui: server/web/vizrs-ui.css różni się od CG System – uruchom scripts/sync-ui.sh" >&2; exit 1; }
  for f in $WINDOW; do
    cmp -s "$CG/source/shell/src/$f" "$TOP/desktop/src/$f" || { echo "sync-ui: desktop/src/$f różni się od CG System – uruchom scripts/sync-ui.sh" >&2; exit 1; }
  done
  echo "server/web/vizrs-ui.css i moduły okna zgodne z CG System ($(git -C "$CG" describe --tags --always 2>/dev/null || echo '?'))"
  exit 0
fi
cp "$SRC" "$DST"
for f in $WINDOW; do cp "$CG/source/shell/src/$f" "$TOP/desktop/src/$f"; done
git -C "$CG" rev-parse HEAD > "$TOP/server/web/CG_SYSTEM"
echo "Skopiowano vizrs-ui.css z CG System $(git -C "$CG" describe --tags --always 2>/dev/null || echo '?') – sprawdź: git diff --stat server/web/"
