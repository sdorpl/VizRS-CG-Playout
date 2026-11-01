#!/bin/sh
# Every start of the stack (the image of seed/Dockerfile): copies the reference
# instance into empty volumes – the VizRS show with its backgrounds, the
# ErsatzTV channel database and the test media. Volumes that already hold data
# are left alone. The ErsatzTV programs and elements come from the sidecar.
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

# The sidecar (user 10001) keeps these up to date itself: programs, elements
# and read access to the ErsatzTV API key.
elements=/ersatztv-config/templates/graphics-elements/script
mkdir -p "$elements"
chown -R 10001:10001 /vizrs-client "$elements"
if [ -e /ersatztv-config/api-secrets.json ]; then
  chown root:10001 /ersatztv-config/api-secrets.json
  chmod 640 /ersatztv-config/api-secrets.json
fi
chgrp 10001 /ersatztv-config
chmod g+x /ersatztv-config /ersatztv-config/templates /ersatztv-config/templates/graphics-elements
echo "seed: access for the sidecar set"
