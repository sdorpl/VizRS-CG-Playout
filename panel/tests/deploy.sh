#!/usr/bin/env bash
# Szybkie wdrożenie panelu na lokalny stos (VizRS Live w stosie VizRS CG Playout, nakładka compose.cg-playout.yml)
# zamiast lokalnego stosu testowego. Pełna bramka z e2e (tests/gate.sh) działa w CI po pushu.
#
#   0. scripts/check-cg-overlay.sh – Caddyfile, usługa live i VAAPI takie same jak w nakładce CG Playout;
#      scripts/sync-ui.sh --check – vizrs-ui.css taki sam jak w VizRS CG System (gdy jest obok)
#   1. szybkie testy: cargo fmt --check, clippy, cargo test (m.in. importy modułów UI), node --check plików JS
#   2. obraz panelu z drzewa roboczego (poprzedni zostaje jako vizrs-live:poprzedni)
#   3. podmiana tylko usługi `live` (legacy, CG Playout, VizRS i proxy działają dalej), czekanie na healthy
#   4. tests/prod-smoke.cjs – przejście po wszystkich ekranach tylko do odczytu
#   Błąd w 3. albo 4. = automatyczny powrót do vizrs-live:poprzedni.
#
# Zmienne (albo tests/local.env): VIZRS_CG_DIR – katalog stosu CG Playout (domyślnie ../VirtRS CG Playout),
# DEPLOY_SHOTS – katalog na zrzuty ekranów (poza repo), DEPLOY_NO_SMOKE=1 – bez przejścia po ekranach.
set -euo pipefail
cd "$(dirname "$0")/.."
step() { printf '\n== %s\n' "$*"; }
[ -f tests/local.env ] && set -a && . tests/local.env && set +a
CG_DIR="${VIZRS_CG_DIR:-$(cd .. && pwd)/VirtRS CG Playout}"
[ -f "$CG_DIR/.env" ] || { echo "deploy: brak $CG_DIR/.env – ustaw VIZRS_CG_DIR" >&2; exit 2; }
envval() { { grep -E "^$1=" "$CG_DIR/.env" || true; } | tail -1 | cut -d= -f2- | sed 's/^"//; s/"$//'; }
IMAGE="$(envval LIVE_IMAGE)"; IMAGE="${IMAGE:-vizrs-live:local}"
PORT="$(envval VIZRS_LIVE_PORT)"; PORT="${PORT:-8090}"
case "$IMAGE" in *:local) ;; *) echo "deploy: LIVE_IMAGE=$IMAGE w $CG_DIR/.env – obraz z rejestru, nie budowany lokalnie; ustaw LIVE_IMAGE=vizrs-live:local" >&2; exit 2 ;; esac
PREV="${IMAGE%:*}:poprzedni"

step "zgodność wdrożenia z CG Playout (Caddyfile, usługa live, VAAPI)"
scripts/check-cg-overlay.sh "$CG_DIR"
if [ -d "../VizRS CG System" ]; then scripts/sync-ui.sh --check; else echo "deploy: brak ../VizRS CG System – pomijam kontrolę vizrs-ui.css"; fi

step "szybkie testy: cargo fmt, clippy, test; składnia JS"
(cd server && cargo fmt --check && cargo clippy --all-targets -q -- -D warnings && cargo test -q)
find server/web tests -name '*.js' -o -name '*.cjs' | while read -r f; do node --check "$f"; done

step "obraz $IMAGE (poprzedni → $PREV)"
docker image inspect "$IMAGE" >/dev/null 2>&1 && docker tag "$IMAGE" "$PREV"
docker build -q -f docker/live.Dockerfile -t "$IMAGE" . >/dev/null

up() { (cd "$CG_DIR" && docker compose up -d --no-deps live >/dev/null 2>&1); }
healthy() {
  local id s
  for _ in $(seq 1 30); do
    id=$(cd "$CG_DIR" && docker compose ps -q live)
    s=$(docker inspect -f '{{.State.Health.Status}}' "$id" 2>/dev/null || echo brak)
    [ "$s" = healthy ] && return 0
    sleep 2
  done
  echo "deploy: panel nie jest healthy ($s)" >&2; return 1
}
rollback() {
  step "POWRÓT do $PREV"
  docker tag "$PREV" "$IMAGE" && up && healthy && echo "przywrócony poprzedni obraz"
  exit 1
}

step "podmiana usługi live w $CG_DIR"
up && healthy || rollback
curl -fsS -o /dev/null "http://127.0.0.1:$PORT/health" || rollback
echo "panel: http://127.0.0.1:$PORT/ (healthy)"

if [ -z "${DEPLOY_NO_SMOKE:-}" ]; then
  step "przejście po ekranach (tylko odczyt)"
  [ -n "${DEPLOY_SHOTS:-}" ] && mkdir -p "$DEPLOY_SHOTS"
  PANEL_TOKEN="$(envval VIZRS_LIVE_TOKEN)" node tests/prod-smoke.cjs "http://127.0.0.1:$PORT" "${DEPLOY_SHOTS:-}" || rollback
fi
printf '\ndeploy: zielono (wycofanie: docker tag %s %s && (cd "%s" && docker compose up -d --no-deps live))\n' "$PREV" "$IMAGE" "$CG_DIR"
