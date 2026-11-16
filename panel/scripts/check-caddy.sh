#!/usr/bin/env bash
# Caddyfile proxy dla widzów: panel/docker/proxy.Caddyfile (stos testowy panelu) = kopia wpisana w configs.live-proxy
# nakładki ../docker-compose.live.yml (Portainer nie montuje plików z repozytorium). Bramka panelu i CI.
# Użycie: scripts/check-caddy.sh   (z katalogu panel/)
set -euo pipefail
cd "$(dirname "$0")/.."
LIVE=../docker-compose.live.yml
# bez komentarzy i pustych wierszy; adres legacy ze zmiennej = stały adres w nakładce
ours=$(grep -v '^\s*#' docker/proxy.Caddyfile | sed 's/{\$LEGACY_UPSTREAM:ersatztv:8409}/ersatztv:8409/' | sed '/^\s*$/d')
theirs=$(awk '/^configs:/{c=1} c&&/content: \|/{f=1;next} f&&/^[^ \t]/{f=0} f' "$LIVE" | sed 's/^      //' | sed '/^\s*$/d')
if d=$(diff <(echo "$ours") <(echo "$theirs")); then
  echo "check-caddy: docker/proxy.Caddyfile = configs.live-proxy w docker-compose.live.yml"
else
  echo "check-caddy: Caddyfile różni się (< docker/proxy.Caddyfile, > configs.live-proxy w $LIVE):" >&2
  echo "$d" | sed 's/^/    /' >&2; exit 1
fi
