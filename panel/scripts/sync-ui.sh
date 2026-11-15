#!/bin/sh
# Wspólny rdzeń wyglądu rodziny VizRS (vizrs-ui.css) – kopia ze źródła w VizRS CG System (source/web/vizrs-ui.css).
#   scripts/sync-ui.sh [KATALOG-CG-SYSTEM]          – skopiuj vizrs-ui.css do server/web/ i zapisz wersję źródła
#   scripts/sync-ui.sh --check [KATALOG-CG-SYSTEM]  – sprawdź, że server/web/vizrs-ui.css jest identyczny ze źródłem (CI, deploy.sh)
# Domyślnie ../VizRS CG System. Wersja źródła: server/web/CG_SYSTEM (commit CG System; CI pobiera ten stan do --check).
# Zmiana wyglądu wspólnego zawsze najpierw w CG System, potem tu ten skrypt; style własne panelu – server/web/style.css.
set -e
CHECK=
[ "$1" = --check ] && { CHECK=1; shift; }
TOP="$(cd "$(dirname "$0")/.." && pwd)"
CG="${1:-$TOP/../VizRS CG System}"
SRC="$CG/source/web/vizrs-ui.css"
DST="$TOP/server/web/vizrs-ui.css"
[ -f "$SRC" ] || { echo "sync-ui: brak źródła $SRC" >&2; exit 2; }
if [ -n "$CHECK" ]; then
  cmp -s "$SRC" "$DST" || { echo "sync-ui: server/web/vizrs-ui.css różni się od CG System – uruchom scripts/sync-ui.sh" >&2; exit 1; }
  echo "server/web/vizrs-ui.css zgodny z CG System ($(git -C "$CG" describe --tags --always 2>/dev/null || echo '?'))"
  exit 0
fi
cp "$SRC" "$DST"
git -C "$CG" rev-parse HEAD > "$TOP/server/web/CG_SYSTEM"
echo "Skopiowano vizrs-ui.css z CG System $(git -C "$CG" describe --tags --always 2>/dev/null || echo '?') – sprawdź: git diff --stat server/web/"
