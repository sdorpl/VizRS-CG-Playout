#!/usr/bin/env bash
# Bramka przed commitem (CLAUDE.md): serwer (fmt, clippy, testy, wydanie), stos testowy (smoke + dane), e2e UI.
# Kod wyjścia 0 = zielono. Playwright: PLAYWRIGHT_DIR albo tests/local.env.
# Stos, który bramka sama uruchomiła, po zielonym przebiegu zatrzymuje (oddaje RAM); przy błędzie zostaje do debugowania.
# GATE_KEEP_STACK=1 – zostaw stos uruchomiony.
set -euo pipefail
cd "$(dirname "$0")/.."
step() { printf '\n== %s\n' "$*"; }
was_up=$(docker ps -q -f name='^vizrs-live-test-legacy$')

step "serwer: cargo fmt --check, clippy, test, build --release"
(cd server && cargo fmt --check && cargo clippy --all-targets -q -- -D warnings && cargo test -q && cargo build --release -q)

step "stos testowy: up (smoke mostu i panelu)"
tests/stack.sh up

step "stos testowy: seed"
tests/stack.sh seed

step "e2e UI"
tests/stack.sh test

if [ -z "$was_up" ] && [ -z "${GATE_KEEP_STACK:-}" ]; then
  step "stos testowy: down (uruchomiony przez bramkę; GATE_KEEP_STACK=1 – zostaw)"
  tests/stack.sh down
fi

printf '\nbramka: zielono\n'
