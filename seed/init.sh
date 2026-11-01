#!/bin/sh
# Every start of the stack (the image of seed/Dockerfile): copies the reference
# instance into empty volumes – the VizRS show with its backgrounds, the
# ErsatzTV channel database and the test media. Volumes that already hold data
# are left alone. The ErsatzTV programs and the graphics element are ours and
# are always replaced (the element keeps its address), so a new version of the
# stack brings them along.
set -eu

# A new volume created here belongs to root; VizRS runs as 10001.
chown 10001:10001 /vizrs-data
if [ -e /vizrs-data/show.json ] || [ -d /vizrs-data/shows ]; then
  echo "seed: VizRS data present, left alone"
else
  cp -R /seed/vizrs-data/. /vizrs-data/
  chown -R 10001:10001 /vizrs-data
  echo "seed: VizRS show and assets copied"
fi

if [ -e /ersatztv-config/ersatztv.sqlite3 ]; then
  echo "seed: ErsatzTV database present, left alone"
else
  cp /seed/ersatztv-config/ersatztv.sqlite3 /ersatztv-config/
  echo "seed: ErsatzTV channel database copied"
fi

if [ -n "$(ls -A /media)" ]; then
  echo "seed: test media present, left alone"
else
  cp -R /seed/media/. /media/
  echo "seed: test media copied"
fi

rm -rf /vizrs-client/*
cp -R /seed/client/. /vizrs-client/
chmod 755 /vizrs-client/*
mkdir -p /ersatztv-config/templates/graphics-elements/script
# The general element; the page elements come from the live VizRS show, written
# by vizrs-etv-client (src/sync.rs).
elements=/ersatztv-config/templates/graphics-elements/script
# The element's address is per instance (channel=, area=): a new version keeps it.
url=$(grep -m1 '^  - http' "$elements/vizrs.yml" 2>/dev/null || true)
cp -R /seed/templates/. "$elements/"
if [ -n "$url" ]; then
  URL="$url" awk '/^  - http/ { print ENVIRON["URL"]; next } { print }' \
    /seed/templates/vizrs.yml > "$elements/vizrs.yml"
fi
echo "seed: ErsatzTV clients and VizRS graphics elements updated"
