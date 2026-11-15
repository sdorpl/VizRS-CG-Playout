#!/usr/bin/env bash
# Prawdziwe usługi obok stosu testowego (tests/stack.sh) – do ręcznego sprawdzenia tego, co e2e testuje na atrapach:
#   Jellyfin  127.0.0.1:18196   (w sieci stosu: http://vizrs-live-test-jellyfin:8096)
#   Emby      127.0.0.1:18896   (http://vizrs-live-test-emby:8096)
#   Plex      127.0.0.1:18324   (http://vizrs-live-test-plex:32400; logowanie PIN przez plex.tv – ręcznie)
#   VizRS CG Playout 127.0.0.1:18701 (binarka z ../VirtRS CG Playout, live API z tokenem, etv_config = dane stosu)
#   drugi panel VizRS Live 127.0.0.1:18092 podłączony do prawdziwego CG Playout (panel 18090 zostaje na atrapie e2e)
#
#   tests/stack.sh up && tests/stack.sh seed   # najpierw stos (legacy, wideo testowe)
#   tests/real.sh up       # uruchamia usługi; Jellyfin i Emby konfiguruje sam (konto, biblioteka z wideo testowym, klucz API)
#   tests/real.sh connect  # łączy legacy z Jellyfin i Emby przez panel (klucze z up), z zamianą ścieżek
#   tests/real.sh status   # adresy, klucze, stan
#   tests/real.sh down     # odłącza legacy od Jellyfin i Emby, zatrzymuje i usuwa kontenery (dane w $DIR/real zostają); reset – też dane
# Przed tests/stack.sh test zrób tests/real.sh down: pozycje z Jellyfin i Emby zmieniają liczby, których oczekuje e2e.
#
# Pamięć: każda usługa z limitem VIZRS_LIVE_REAL_MEM (domyślnie 1g; 0 – bez limitu), Jellyfin i Emby z workstation GC.
# Produkcji (virtrs-*, port 8409) nie dotyka. Obrazy: JELLYFIN_IMAGE, EMBY_IMAGE, PLEX_IMAGE; Plex: PLEX_CLAIM=claim-… (https://plex.tv/claim) – opcjonalnie.
set -euo pipefail
cd "$(dirname "$0")/.."
[ -f tests/local.env ] && . tests/local.env

DIR="${VIZRS_LIVE_TEST_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/vizrs-live-test}"
REAL_MEM="${VIZRS_LIVE_REAL_MEM:-1g}"
R="$DIR/real"
NET="vizrs-live-test-net"
MEDIA="$DIR/config/media/info24"
JELLYFIN_IMAGE="${JELLYFIN_IMAGE:-jellyfin/jellyfin:10.10.7}"
EMBY_IMAGE="${EMBY_IMAGE:-emby/embyserver:4.8.11.0}"
PLEX_IMAGE="${PLEX_IMAGE:-plexinc/pms-docker:latest}"
CG_REPO="${CG_REPO:-../VirtRS CG Playout}"
CG_TOKEN="real-cg-token-0123456789"
TOKEN="test-token-0123456789"
USER_NAME="vizrs"
USER_PW="vizrs-test-haslo"
PANEL="http://127.0.0.1:18092"

say() { printf '%s\n' "$*"; }
wait_http() { # wait_http URL [prób]
  for _ in $(seq 1 "${2:-90}"); do curl -sf -o /dev/null "$1" && return 0; sleep 2; done
  say "nie odpowiada: $1" >&2; return 1
}

# Jellyfin i Emby mają to samo API kreatora: konto, ustawienia, biblioteka, klucz API.
setup_mb() { # setup_mb NAZWA PORT PREFIKS_NAGŁÓWKA
  local name="$1" base="http://127.0.0.1:$2" scheme="$3"
  local keyfile="$R/$name.key"
  [ -s "$keyfile" ] && { say "$name: już skonfigurowany (klucz w $keyfile)"; return 0; }
  wait_http "$base/System/Info/Public" 120
  local auth="$scheme Client=\"VizRS Live test\", Device=\"real.sh\", DeviceId=\"vizrs-real-sh\", Version=\"1.0\""
  local j=(-sf -H 'Content-Type: application/json' -H "X-Emby-Authorization: $auth")
  if [ "$(curl -s "$base/System/Info/Public" | sed -n 's/.*"StartupWizardCompleted":\(true\|false\).*/\1/p')" != "true" ]; then
    curl "${j[@]}" -X POST "$base/Startup/Configuration" -d '{"UICulture":"pl","MetadataCountryCode":"PL","PreferredMetadataLanguage":"pl"}' >/dev/null
    curl "${j[@]}" "$base/Startup/User" >/dev/null || true
    curl "${j[@]}" -X POST "$base/Startup/User" -d "{\"Name\":\"$USER_NAME\",\"Password\":\"$USER_PW\"}" >/dev/null
    curl "${j[@]}" -X POST "$base/Startup/RemoteAccess" -d '{"EnableRemoteAccess":true,"EnableAutomaticPortMapping":false}' >/dev/null || true
    curl "${j[@]}" -X POST "$base/Startup/Complete" >/dev/null
  fi
  local login token
  login=$(curl "${j[@]}" -X POST "$base/Users/AuthenticateByName" -d "{\"Username\":\"$USER_NAME\",\"Pw\":\"$USER_PW\"}")
  token=$(printf '%s' "$login" | sed -n 's/.*"AccessToken":"\([^"]*\)".*/\1/p')
  [ -n "$token" ] || { say "$name: logowanie nie wyszło: $login" >&2; return 1; }
  local a=(-sf -H 'Content-Type: application/json' -H "X-Emby-Token: $token")
  curl "${a[@]}" -X POST "$base/Library/VirtualFolders?name=Filmy%20E2E&collectionType=movies&refreshLibrary=true&paths=%2Fmedia%2Finfo24" \
    -d '{"LibraryOptions":{"PathInfos":[{"Path":"/media/info24"}]}}' >/dev/null || say "$name: biblioteka już jest?"
  curl "${a[@]}" -X POST "$base/Auth/Keys?App=VizRSLive&app=VizRSLive" >/dev/null
  local key
  key=$(curl "${a[@]}" "$base/Auth/Keys" | grep -o '"AccessToken":"[^"]*"' | tail -1 | cut -d'"' -f4)
  [ -n "$key" ] || { say "$name: brak klucza API" >&2; return 1; }
  printf '%s' "$key" >"$keyfile"
  say "ok   $name: konto $USER_NAME / $USER_PW, biblioteka „Filmy E2E” (/media/info24), klucz API w $keyfile"
}

cmd() { # cmd NAZWA [JSON]
  local body="${2:-}"
  [ -n "$body" ] || body='{}'
  curl -s -X POST -H 'X-VizRS-Live: 1' "$PANEL/api/cmd/$1" -d "$body"
  echo
}

case "${1:-}" in
  up)
    [ -d "$MEDIA" ] || { say "brak $MEDIA – najpierw: tests/stack.sh up && tests/stack.sh seed" >&2; exit 1; }
    docker network inspect "$NET" >/dev/null 2>&1 || { say "brak sieci $NET – najpierw tests/stack.sh up" >&2; exit 1; }
    mkdir -p "$R"/{jellyfin,emby,plex}/{config,cache} "$R/cg"
    run() { # run NAZWA PORT_HOSTA:PORT obraz [argumenty docker]
      local n="vizrs-live-test-$1"; shift
      docker ps -q -f "name=^$n$" | grep -q . && { say "$n: działa"; return; }
      docker rm -f "$n" >/dev/null 2>&1 || true
      local ports="$1" image="$2"; shift 2
      local lim=()
      [ "$REAL_MEM" = 0 ] || lim=(--memory="$REAL_MEM" --memory-swap="$REAL_MEM")
      docker run -d --name "$n" --network "$NET" -p "127.0.0.1:$ports" "${lim[@]}" -v "$MEDIA:/media/info24:ro" "$@" "$image" >/dev/null
      say "ok   $n ($image) na 127.0.0.1:${ports%%:*}"
    }
    run jellyfin 18196:8096 "$JELLYFIN_IMAGE" -e DOTNET_gcServer=0 --user "$(id -u):$(id -g)" -v "$R/jellyfin/config:/config" -v "$R/jellyfin/cache:/cache"
    run emby 18896:8096 "$EMBY_IMAGE" -e DOTNET_gcServer=0 -e UID="$(id -u)" -e GID="$(id -g)" -v "$R/emby/config:/config"
    run plex 18324:32400 "$PLEX_IMAGE" -e PLEX_UID="$(id -u)" -e PLEX_GID="$(id -g)" -e TZ=Europe/Warsaw ${PLEX_CLAIM:+-e PLEX_CLAIM="$PLEX_CLAIM"} \
      -v "$R/plex/config:/config" -v "$R/plex/cache:/transcode"
    # VizRS CG Playout z repozytorium obok (bez Dockera); live API na danych stosu
    cg_bin="$CG_REPO/target/release/vizrs-cg"
    (cd "$CG_REPO" && cargo build --release -q --bin vizrs-cg)
    if ! curl -sf -o /dev/null http://127.0.0.1:18701/health; then
      cat >"$R/cg/config.json" <<JSON
{ "listen": "127.0.0.1:18701", "vizrs": { "address": "127.0.0.1:6199", "token": "" }, "output_url": "",
  "on_clip": "TAKE 4001 headline={title}", "etv_url": "http://127.0.0.1:18409", "etv_config": "$DIR/config",
  "panel_token": "$CG_TOKEN", "ingest_token": "real-ingest", "sync_seconds": 3600 }
JSON
      nohup "$cg_bin" "$R/cg/config.json" >"$R/cg/cg.log" 2>&1 &
      echo $! >"$R/cg/pid"
      wait_http http://127.0.0.1:18701/health 30
      say "ok   VizRS CG Playout na 127.0.0.1:18701 (dziennik $R/cg/cg.log; bez silnika VizRS – komendy grafiki idą w próżnię)"
    fi
    if ! curl -sf -o /dev/null "$PANEL/api/status"; then
      nohup server/target/release/vizrs-live --port 18092 --bridge http://127.0.0.1:18409 --bridge-token "$TOKEN" \
        --stream-url http://127.0.0.1:18410 --cg-url http://127.0.0.1:18701 --cg-token "$CG_TOKEN" >"$R/panel.log" 2>&1 &
      echo $! >"$R/panel.pid"
      wait_http "$PANEL/api/status" 30
      say "ok   panel VizRS Live z prawdziwym CG Playout: $PANEL/"
    fi
    setup_mb jellyfin 18196 MediaBrowser
    setup_mb emby 18896 Emby
    wait_http http://127.0.0.1:18324/identity 60 && say "ok   Plex odpowiada (serwer bez konta: logowanie PIN w panelu – Źródła zewnętrzne → Plex)"
    say "dalej: tests/real.sh connect (legacy ↔ Jellyfin, Emby)"
    ;;
  connect)
    for K in Jellyfin Emby; do
      k=$(echo "$K" | tr 'A-Z' 'a-z')
      key=$(cat "$R/$k.key")
      say "== $K"
      cmd "Save${K}Secrets" "{\"Secrets\":{\"Address\":\"http://vizrs-live-test-$k:8096\",\"ApiKey\":\"$key\"}}" | cut -c1-200
      cmd "Synchronize${K}MediaSources" | cut -c1-200
    done
    say "Legacy synchronizuje w tle; biblioteki i zamiany ścieżek (/media/info24 → /config/media/info24): $PANEL/#zrodla"
    ;;
  status)
    for n in jellyfin emby plex; do docker ps --format '{{.Names}} {{.Status}}' -f "name=vizrs-live-test-$n"; done
    for k in jellyfin emby; do [ -s "$R/$k.key" ] && say "$k klucz: $(cat "$R/$k.key")"; done
    curl -s http://127.0.0.1:18701/health | cut -c1-200; echo
    curl -s -H "X-Panel-Token: $CG_TOKEN" http://127.0.0.1:18701/api/live/graphics | cut -c1-200; echo
    say "panel: $PANEL/  ·  Jellyfin http://127.0.0.1:18196  ·  Emby http://127.0.0.1:18896  ·  Plex http://127.0.0.1:18324/web  ·  konto $USER_NAME / $USER_PW"
    ;;
  down | reset)
    # legacy z powrotem bez Jellyfin i Emby: ich pozycje psułyby liczby w e2e (tests/stack.sh test)
    if curl -sf -o /dev/null "$PANEL/api/status"; then
      for K in Jellyfin Emby; do cmd "Disconnect$K" >/dev/null; done
      say "legacy odłączony od Jellyfin i Emby"
    fi
    for f in "$R/cg/pid" "$R/panel.pid"; do [ -f "$f" ] && kill "$(cat "$f")" 2>/dev/null; rm -f "$f"; done
    docker rm -f vizrs-live-test-jellyfin vizrs-live-test-emby vizrs-live-test-plex >/dev/null 2>&1 || true
    # pliki usług należą do bieżącego użytkownika (UID w kontenerach)
    [ "$1" = reset ] && rm -rf -- "${DIR:?}/real"
    say "zatrzymane${1/down/}"
    ;;
  *) sed -n '2,17p' "$0"; exit 1 ;;
esac
