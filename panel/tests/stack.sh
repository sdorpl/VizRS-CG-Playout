#!/usr/bin/env bash
# Stos testowy VizRS Live na osobnych portach (nie rusza produkcyjnego ErsatzTV):
#   legacy z mostem: 127.0.0.1:18409, proxy dla widzów (R10): 127.0.0.1:18410, panel: 127.0.0.1:18090, dane: $VIZRS_LIVE_TEST_DIR (domyślnie ~/.cache/vizrs-live-test)
#
#   tests/stack.sh up      # buduje obraz mostu i panel, uruchamia, sprawdza most (smoke)
#   tests/stack.sh seed    # wideo testowe + kanał INFO24 z ramówką – wszystko komendami przez panel (drugi raz nic nie dubluje)
#   tests/stack.sh test    # e2e UI (tests/e2e.cjs) na działającym stosie; Playwright: PLAYWRIGHT_DIR albo tests/local.env
#   tests/stack.sh down    # zatrzymuje (dane zostają)
#   tests/stack.sh reset   # zatrzymuje i usuwa dane testowe ($VIZRS_LIVE_TEST_DIR)
#   tests/stack.sh mem     # ile RAM zajmuje stos testowy (kontenery, panele, Chromium, dane w tmpfs)
# Domyślnie najnowsze ghcr.io/ersatztv/legacy (up pobiera nowsze). Inna wersja: LEGACY_IMAGE=ghcr.io/ersatztv/legacy:<wersja> tests/stack.sh up
#   (najlepiej z osobnym VIZRS_LIVE_TEST_DIR – nowsze legacy migruje bazę i starsze już jej nie otworzy)
# Pełna bramka przed commitem: tests/gate.sh
# Pamięć: dane na dysku, nie w /tmp (często tmpfs = RAM); legacy z workstation GC i limitem VIZRS_LIVE_TEST_MEM (domyślnie 1536m – e2e ogląda kanał,
#   a ffmpeg legacy przy 1080p to ok. 775 MB;
#   legacy ma w runtimeconfig Server GC – sterta na każdy rdzeń); VIZRS_LIVE_TEST_MEM=0 – bez limitu.
set -euo pipefail
cd "$(dirname "$0")/.."
# lokalne ustawienia testów (nie w repo), np. PLAYWRIGHT_DIR=/ścieżka/z/node_modules/playwright
[ -f tests/local.env ] && . tests/local.env

DIR="${VIZRS_LIVE_TEST_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/vizrs-live-test}"
MEM="${VIZRS_LIVE_TEST_MEM:-1536m}"
# limit pamięci kontenera (docker run …): pusty przy VIZRS_LIVE_TEST_MEM=0
mem_limit() { [ "$MEM" = 0 ] || printf -- '--memory=%s --memory-swap=%s' "$1" "$1"; }
TOKEN="test-token-0123456789"
# VizRS CG Playout w testach: atrapa z tests/e2e.cjs na 127.0.0.1:18700 (R9)
CG_TOKEN="test-cg-token-0123456789"
NAME="vizrs-live-test-legacy"
PROXY_NAME="vizrs-live-test-proxy"
NET="vizrs-live-test-net"
PROXY="http://127.0.0.1:18410"
BRIDGE="http://127.0.0.1:18409/vizrs-bridge"
PANEL="http://127.0.0.1:18090"

wait_bridge() {
  for _ in $(seq 1 90); do
    if curl -sf -o /dev/null -H "X-VizRS-Token: $TOKEN" "$BRIDGE/health"; then return 0; fi
    sleep 2
  done
  echo "most nie wstał – docker logs $NAME" >&2
  return 1
}

cmd() { # cmd NAZWA [JSON]
  local body="${2:-}"
  [ -n "$body" ] || body='{}'
  curl -s -X POST -H 'X-VizRS-Live: 1' "$PANEL/api/cmd/$1" -d "$body"
  echo
}

expect() { # expect OPIS KOD_HTTP polecenie...
  local what="$1" want="$2"; shift 2
  local got
  got=$("$@" -o /dev/null -w '%{http_code}')
  if [ "$got" = "$want" ]; then echo "ok   $what ($got)"; else echo "BŁĄD $what: $got zamiast $want" >&2; exit 1; fi
}

case "${1:-}" in
  up)
    mkdir -p "$DIR/config"
    docker build -q --pull -f docker/legacy-bridge.Dockerfile ${LEGACY_IMAGE:+--build-arg LEGACY_IMAGE="$LEGACY_IMAGE"} -t vizrs-live-legacy:dev . >/dev/null
    (cd server && cargo build --release -q)
    docker rm -f "$NAME" "$PROXY_NAME" >/dev/null 2>&1 || true
    docker network inspect "$NET" >/dev/null 2>&1 || docker network create "$NET" >/dev/null
    # shellcheck disable=SC2046
    docker run -d --name "$NAME" --network "$NET" -p 127.0.0.1:18409:8409 $(mem_limit "$MEM") -e DOTNET_gcServer=0 \
      -e VIZRS_BRIDGE_TOKEN="$TOKEN" -v "$DIR/config:/config" vizrs-live-legacy:dev >/dev/null
    # proxy jak w compose.yml (docker/proxy.Caddyfile); panel podaje widzom adres proxy
    # shellcheck disable=SC2046
    docker run -d --name "$PROXY_NAME" --network "$NET" -p 127.0.0.1:18410:8409 $(mem_limit 128m) -e LEGACY_UPSTREAM="$NAME:8409" \
      -v "$PWD/docker/proxy.Caddyfile:/etc/caddy/Caddyfile:ro" "${PROXY_IMAGE:-caddy:2.10.2-alpine}" >/dev/null
    wait_bridge
    pkill -f "[v]izrs-live --port 18090" 2>/dev/null || true
    nohup server/target/release/vizrs-live --port 18090 --bridge http://127.0.0.1:18409 --bridge-token "$TOKEN" \
      --stream-url "$PROXY" --cg-url http://127.0.0.1:18700 --cg-token "$CG_TOKEN" \
      --vizrs-url :18700 --vizrs-output-token e2e-vizrs-output --vizrs-channels >"$DIR/panel.log" 2>&1 &
    sleep 1
    # smoke: most i jego zabezpieczenia
    expect "most: zdrowie" 200 curl -s -H "X-VizRS-Token: $TOKEN" "$BRIDGE/health"
    missing=$(curl -s -H "X-VizRS-Token: $TOKEN" "$BRIDGE/health" | grep -o '"commandsMissing":\[[^]]*\]')
    if [ "$missing" = '"commandsMissing":[]' ]; then echo "ok   most: wszystkie komendy z białej listy są w legacy"; else echo "BŁĄD most: brak komend w legacy: $missing" >&2; exit 1; fi
    expect "most: zły token" 401 curl -s -H "X-VizRS-Token: zly" "$BRIDGE/health"
    expect "most: komenda spoza listy" 403 curl -s -X POST -H "X-VizRS-Token: $TOKEN" "$BRIDGE/command/SaveConfigElementByKey" -d '{}'
    expect "most: zapytanie" 200 curl -s -X POST -H "X-VizRS-Token: $TOKEN" "$BRIDGE/command/GetAllChannels" -d '{}'
    expect "panel: status" 200 curl -s "$PANEL/api/status"
    expect "panel: POST bez X-VizRS-Live" 403 curl -s -X POST "$PANEL/api/cmd/GetAllChannels" -d '{}'
    # proxy (R10): web GUI legacy i most zamknięte, strumienie i M3U/XMLTV otwarte
    # świeża baza: legacy tworzy xmltv.xml chwilę po starcie (wcześniej 500)
    for _ in $(seq 1 30); do curl -sf -o /dev/null "$PROXY/iptv/xmltv.xml" && break; sleep 1; done
    expect "proxy: web GUI legacy /" 403 curl -s "$PROXY/"
    expect "proxy: web GUI legacy /channels" 403 curl -s "$PROXY/channels"
    expect "proxy: API legacy" 403 curl -s "$PROXY/api/channels"
    expect "proxy: most" 403 curl -s -H "X-VizRS-Token: $TOKEN" "$PROXY/vizrs-bridge/health"
    expect "proxy: POST do /iptv" 403 curl -s -X POST "$PROXY/iptv/channels.m3u"
    expect "proxy: M3U" 200 curl -s "$PROXY/iptv/channels.m3u"
    expect "proxy: XMLTV" 200 curl -s "$PROXY/iptv/xmltv.xml"
    expect "proxy: HDHomeRun" 200 curl -s "$PROXY/discover.json"
    echo "panel: $PANEL/  ·  widzowie (proxy): $PROXY/iptv/channels.m3u  ·  legacy: http://127.0.0.1:18409/  ·  dane: $DIR"
    ;;
  seed)
    if cmd GetAllChannels | grep -q '"Number":"24"'; then
      echo "seed: kanał 24 już jest – dane testowe gotowe (od zera: tests/stack.sh reset && tests/stack.sh up && tests/stack.sh seed)"
      exit 0
    fi
    docker exec "$NAME" sh -c 'mkdir -p /config/media/info24 && cd /config/media/info24 && for t in Wiadomosci Pogoda Reportaz; do [ -f "$t.mp4" ] || ffmpeg -loglevel error -y -f lavfi -i "testsrc2=size=1280x720:rate=25" -f lavfi -i "sine=frequency=440:sample_rate=48000" -t 120 -c:v libx264 -preset ultrafast -c:a aac "$t.mp4"; done'
    cmd CreateChannel '{"Name":"INFO24","Number":"24","Group":"Informacje","Categories":"wiadomości","FFmpegProfileId":1,"Logo":{"Path":"","ContentType":""},"StreamingEngine":"Next","StreamingMode":"HttpLiveStreamingSegmenter","PlayoutSource":"Generated","PlayoutMode":"Continuous","StreamSelectorMode":"Default","IdleBehavior":"StopOnDisconnect","SubtitleMode":"None","IsEnabled":true,"ShowInEpg":true}'
    lib=$(cmd CreateLocalLibrary '{"Name":"Archiwum INFO24","MediaKind":"OtherVideos","Paths":["/config/media/info24"]}' | sed -n 's/.*"Id":\([0-9]*\).*/\1/p')
    cmd QueueLibraryScanByLibraryId "{\"LibraryId\":$lib}"
    echo "czekam na skan…"; sleep 25
    sc=$(cmd CreateSmartCollection '{"Name":"Archiwum INFO24","Query":"type:other_video"}' | sed -n 's/.*"Id":\([0-9]*\).*/\1/p')
    ps=$(cmd CreateProgramSchedule '{"Name":"INFO24 dzień","FixedStartTimeBehavior":"Strict"}' | sed -n 's/.*"ProgramScheduleId":\([0-9]*\).*/\1/p')
    cmd ReplaceProgramScheduleItems "{\"ProgramScheduleId\":$ps,\"Items\":[{\"Index\":1,\"StartType\":\"Dynamic\",\"PlayoutMode\":\"Flood\",\"CollectionType\":\"SmartCollection\",\"SmartCollectionId\":$sc,\"PlaybackOrder\":\"Chronological\",\"MultipleCount\":\"1\",\"SearchTitle\":\"\",\"SearchQuery\":\"\",\"CustomTitle\":\"\",\"WatermarkIds\":[],\"GraphicsElementIds\":[]}]}"
    ch=$(cmd GetAllChannels | grep -o '"Id":[0-9]*,"Number":"24"' | sed 's/"Id":\([0-9]*\).*/\1/')
    po=$(cmd CreateClassicPlayout "{\"ChannelId\":$ch,\"ProgramScheduleId\":$ps}" | sed -n 's/.*"PlayoutId":\([0-9]*\).*/\1/p')
    cmd BuildPlayout "{\"PlayoutId\":$po,\"Mode\":\"Reset\"}"
    echo "gotowe: kanał 24 · INFO24 z ramówką (playout $po). Panel: $PANEL/#antena"
    ;;
  test)
    if [ -z "${PLAYWRIGHT_DIR:-}" ]; then
      echo "brak PLAYWRIGHT_DIR: ustaw zmienną albo wpisz PLAYWRIGHT_DIR=… do tests/local.env (katalog z node_modules/playwright)" >&2
      exit 2
    fi
    PLAYWRIGHT_DIR="$PLAYWRIGHT_DIR" node tests/e2e.cjs "$PANEL"
    ;;
  reset)
    case "$DIR" in /|/tmp|/home|"$HOME"|"$HOME/.cache"|"") echo "reset: odmawiam usunięcia '$DIR'" >&2; exit 1 ;; esac
    pkill -f "[v]izrs-live --port 18090" 2>/dev/null || true
    docker rm -f "$NAME" "$PROXY_NAME" >/dev/null 2>&1 || true
    docker network rm "$NET" >/dev/null 2>&1 || true
    # pliki w config/ tworzy legacy w kontenerze (root) – usuwa je kontener
    [ -d "$DIR" ] && docker run --rm -v "$DIR:/d" alpine sh -c 'rm -rf /d/* /d/.[!.]*' >/dev/null 2>&1 || true
    rm -rf "$DIR"
    echo "usunięte: $DIR"
    ;;
  down)
    pkill -f "[v]izrs-live --port 18090" 2>/dev/null || true
    docker rm -f "$NAME" "$PROXY_NAME" >/dev/null 2>&1 || true
    echo "zatrzymane; dane: $DIR"
    ;;
  mem)
    docker stats --no-stream --format '{{.Name}}\t{{.MemUsage}}' | grep '^vizrs-live-test' || echo "kontenery stosu: nie działają"
    for p in 18090 18092; do
      pid=$(pgrep -f "[v]izrs-live --port $p" | head -1) || true
      [ -n "$pid" ] && printf 'panel %s\t%s MiB\n' "$p" "$(( $(ps -o rss= -p "$pid") / 1024 ))"
    done
    printf 'Chromium z Playwright\t%s MiB\n' "$(ps -eo rss=,args= | grep '[m]s-playwright' | awk '{s+=$1} END {print int(s/1024)}')"
    fs=$(df --output=fstype "$DIR" 2>/dev/null | tail -1) || true
    printf 'dane %s (%s)\t%s\n' "$DIR" "${fs:-brak}" "$(du -sh "$DIR" 2>/dev/null | cut -f1)"
    [ "$fs" = tmpfs ] && echo "UWAGA: $DIR leży w tmpfs – dane testowe zajmują RAM" >&2
    true
    ;;
  *)
    echo "użycie: tests/stack.sh up|seed|test|down|reset|mem" >&2
    exit 2
    ;;
esac
