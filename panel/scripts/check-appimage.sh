#!/usr/bin/env bash
# Sprawdzenie AppImage okna panelu: podgląd strumienia potrzebuje w paczce wtyczek GStreamera – demuxera MP4 (isomp4),
# dekoderów H.264/AAC (libav), h264parse (videoparsersbad), appsrc i playbin; potem start z --version.
#   panel/scripts/check-appimage.sh <plik.AppImage>
# Uruchamia je CI panelu (panel-ci.yml, job appimage), którego AppImage trafia do wydania.
set -euo pipefail
a=$(realpath "$1")
[ -f "$a" ] || { echo "::error::brak $1"; exit 1; }
chmod +x "$a"
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
(cd "$tmp" && APPIMAGE_EXTRACT_AND_RUN=1 "$a" --appimage-extract >/dev/null)
for w in libgstisomp4.so libgstlibav.so libgstvideoparsersbad.so libgstapp.so libgstplayback.so; do
  find "$tmp/squashfs-root" -name "$w" | grep -q . || { echo "::error::AppImage bez wtyczki GStreamera $w"; exit 1; }
done
APPIMAGE_EXTRACT_AND_RUN=1 "$a" --version
echo "AppImage: wtyczki GStreamera w paczce, --version działa"
