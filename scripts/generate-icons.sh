#!/usr/bin/env bash
set -euo pipefail

# Requires ImageMagick and macOS iconutil; generated assets are checked in.
mkdir -p target/packaging
work="$(mktemp -d "$PWD/target/packaging/icons.XXXXXX")"
iconset="$work/ansiedad.iconset"
mkdir -p "$iconset"
magick -background none assets/ansiedad.svg -resize 256x256 PNG32:assets/ansiedad.png
magick -background none assets/ansiedad.svg \
  -define icon:auto-resize=256,128,64,48,32,16 assets/ansiedad.ico
for size in 16 32 128 256 512; do
  magick -background none assets/ansiedad.svg -resize "${size}x${size}" \
    "PNG32:$iconset/icon_${size}x${size}.png"
  double=$((size * 2))
  magick -background none assets/ansiedad.svg -resize "${double}x${double}" \
    "PNG32:$iconset/icon_${size}x${size}@2x.png"
done
iconutil -c icns "$iconset" -o assets/ansiedad.icns
