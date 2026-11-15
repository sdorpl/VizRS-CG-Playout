#!/usr/bin/env bash
# Zgodność wdrożenia z VizRS CG Playout (pakiet VizRS, DOKUMENTACJA §1): te same rzeczy opisane w dwóch repozytoriach
# muszą być takie same, bo Portainer (stosy s1/s2) nie montuje plików z innego repo ani nie buduje z niego obrazów.
#
#   1. Caddyfile proxy: docker/proxy.Caddyfile  =  configs.live-proxy w docker-compose.live.yml CG Playout
#   2. środowisko usługi `live`: compose.cg-playout.yml  =  docker-compose.live.yml CG Playout (bez komunikatów :?…)
#   3. compose.vaapi.yml  =  docker-compose.vaapi.yml CG Playout (bez komentarzy)
#   4. wolumeny usługi `live` (media dla odtwarzania bez legacy): compose.cg-playout.yml  =  docker-compose.live.yml CG Playout
#
# Źródłem jest to repo (VizRS Live): rozjazd poprawia się po stronie CG Playout, chyba że zmiana wyszła stamtąd.
# Użycie: scripts/check-cg-overlay.sh [katalog CG Playout]   (domyślnie VIZRS_CG_DIR albo ../VirtRS CG Playout)
set -euo pipefail
cd "$(dirname "$0")/.."
CG_DIR="${1:-${VIZRS_CG_DIR:-$(cd .. && pwd)/VirtRS CG Playout}}"
CG_LIVE="$CG_DIR/docker-compose.live.yml"
[ -f "$CG_LIVE" ] || { echo "check-cg-overlay: brak $CG_LIVE – pomijam" >&2; exit 0; }
fail=0
report() { echo "check-cg-overlay: $1" >&2; echo "$2" | sed 's/^/    /' >&2; fail=1; }

# 1. Caddyfile: bez komentarzy i pustych wierszy; adres legacy ze zmiennej = stały adres w CG
ours=$(grep -v '^\s*#' docker/proxy.Caddyfile | sed 's/{\$LEGACY_UPSTREAM:ersatztv:8409}/ersatztv:8409/' | sed '/^\s*$/d')
theirs=$(awk '/^configs:/{c=1} c&&/content: \|/{f=1;next} f&&/^[^ \t]/{f=0} f' "$CG_LIVE" | sed 's/^      //' | sed '/^\s*$/d')
d=$(diff <(echo "$ours") <(echo "$theirs") || true)
[ -z "$d" ] || report "Caddyfile proxy różni się od kopii w $CG_LIVE (configs.live-proxy):" "$d"

# 2. środowisko usługi live: KLUCZ: wartość, bez komentarzy i bez treści komunikatu w ${X:?…}
envof() {
  awk '/^  live:/{s=1;next} s&&/^  [a-z]/{s=0} s&&/^    environment:/{e=1;next} e&&/^    [a-z]/{e=0} s&&e' "$1" \
    | grep -v '^\s*#' | sed 's/^\s*//; s/:?[^}]*}/}/g' | sed '/^$/d' | sort
}
d=$(diff <(envof compose.cg-playout.yml) <(envof "$CG_LIVE") || true)
[ -z "$d" ] || report "środowisko usługi live różni się (< compose.cg-playout.yml, > $CG_LIVE):" "$d"

# 2b. wolumeny usługi live
volof() {
  awk '/^  live:/{s=1;next} s&&/^  [a-z]/{s=0} s&&/^    volumes:/{e=1;next} e&&/^    [a-z]/{e=0} s&&e' "$1" | grep -v '^\s*#' | sed 's/^\s*//' | sed '/^$/d' | sort
}
d=$(diff <(volof compose.cg-playout.yml) <(volof "$CG_LIVE") || true)
[ -z "$d" ] || report "wolumeny usługi live różnią się (< compose.cg-playout.yml, > $CG_LIVE):" "$d"

# 3. VAAPI
if [ -f "$CG_DIR/docker-compose.vaapi.yml" ]; then
  d=$(diff <(grep -v '^\s*#' compose.vaapi.yml) <(grep -v '^\s*#' "$CG_DIR/docker-compose.vaapi.yml") || true)
  [ -z "$d" ] || report "compose.vaapi.yml różni się od docker-compose.vaapi.yml CG Playout:" "$d"
fi

[ "$fail" = 0 ] && echo "check-cg-overlay: wdrożenie zgodne z $CG_DIR"
exit "$fail"
