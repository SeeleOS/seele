#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -lt 2 ] || [ "$#" -gt 6 ]; then
    echo "usage: $0 <input> <output> [opacity] [r] [g] [b]" >&2
    echo "example: $0 misc/wallpaper.png misc/wallpaper-blended.png 0.45 30 34 51" >&2
    exit 1
fi

input=$1
output=$2
opacity=${3:-0.65}
r=${4:-30}
g=${5:-34}
b=${6:-51}

ffmpeg -y \
    -i "$input" \
    -f lavfi -i "color=c=0x$(printf '%02X%02X%02X' "$r" "$g" "$b"):s=16x16" \
    -filter_complex "[1:v][0:v]scale2ref[overlay][base];[base][overlay]blend=all_mode=normal:all_opacity=${opacity}" \
    -update 1 \
    -frames:v 1 \
    "$output"
