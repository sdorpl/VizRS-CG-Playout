#!/usr/bin/env bash
# Akceleracja sprzętowa w obrazie legacy z mostem (amd64): ffmpeg i sterowniki dla Intela (VAAPI, QSV),
# AMD (VAAPI) i NVIDII (NVENC/CUDA). Bez GPU – sprawdza tylko, że obraz to zawiera; przekazanie GPU
# do kontenera: compose.vaapi.yml, compose.nvidia.yml (RUNBOOK §2b). Biblioteki sterownika NVIDII
# (libcuda, libnvidia-encode) podaje przy starcie NVIDIA Container Toolkit – ich w obrazie być nie może.
#
# Użycie: tests/hwaccel.sh [obraz]   (domyślnie vizrs-live-legacy:dev – obraz ze stosu testowego)
set -euo pipefail
IMG="${1:-vizrs-live-legacy:dev}"
run() { docker run --rm --entrypoint sh "$IMG" -c "$1"; }

fail=0
ok() { echo "ok   $1"; }
bad() { echo "BŁĄD $1"; fail=1; }

FF=$(run 'command -v ffmpeg || echo /usr/local/bin/ffmpeg')
ACC=$(run "$FF -hide_banner -hwaccels 2>/dev/null" | tr '\n' ' ')
ENC=$(run "$FF -hide_banner -encoders 2>/dev/null")
DRI=$(run 'ls /usr/lib/x86_64-linux-gnu/dri/ 2>/dev/null')
LIB=$(run 'ls /usr/lib/x86_64-linux-gnu/ 2>/dev/null')

for a in vaapi qsv cuda; do
  [[ " $ACC " == *" $a "* ]] && ok "ffmpeg -hwaccels: $a" || bad "ffmpeg -hwaccels: brak $a"
done
for e in h264_vaapi hevc_vaapi h264_qsv hevc_qsv h264_nvenc hevc_nvenc; do
  grep -qw "$e" <<<"$ENC" && ok "koder $e" || bad "brak kodera $e"
done
for d in iHD_drv_video.so:'Intel (Broadwell i nowsze)' i965_drv_video.so:'Intel (starsze)' radeonsi_drv_video.so:'AMD'; do
  grep -qx "${d%%:*}" <<<"$DRI" && ok "sterownik VAAPI ${d%%:*} – ${d#*:}" || bad "brak sterownika VAAPI ${d%%:*} – ${d#*:}"
done
grep -qE '^libmfx-gen\.so|^libvpl\.so|^libmfxhw64\.so' <<<"$LIB" && ok "Intel QSV: środowisko oneVPL/Media SDK" || bad "brak środowiska Intel QSV (libmfx-gen / libvpl)"
grep -qE '^libva\.so' <<<"$LIB" && ok "libva" || bad "brak libva"

if ((fail)); then echo "hwaccel: $IMG – braki akceleracji"; exit 1; fi
echo "hwaccel: $IMG – Intel (VAAPI, QSV), AMD (VAAPI), NVIDIA (NVENC) w obrazie"
