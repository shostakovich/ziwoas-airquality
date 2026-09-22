#!/usr/bin/env bash
# Build the SEN66 firmware with the pinned profile from sketch.yaml.
#
# Usage:
#   firmware/build.sh                               compile only
#   firmware/build.sh --upload -p /dev/cu.usbmodemXXXX   compile and flash (Linux: /dev/ttyACM0)
#
# Any further arguments are passed to `arduino-cli compile`.
# Output (.uf2/.bin/.elf): build/firmware/ in the repository root.
# If upload over the serial port fails, hold BOOTSEL while plugging in the board and copy
# build/firmware/firmware.ino.uf2 to the RPI-RP2 drive.
set -euo pipefail

sketch_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_dir="$(cd "$sketch_dir/.." && pwd)"
out_dir="$repo_dir/build/firmware"

version="$(git -C "$repo_dir" describe --always --dirty --tags 2>/dev/null || true)"
# Keep the macro a plain string literal whatever git prints.
version="$(printf '%s' "${version:-dev}" | tr -c 'A-Za-z0-9._+-' '_')"
echo "FIRMWARE_VERSION=$version"

exec arduino-cli compile \
  --profile feather \
  --warnings all \
  --output-dir "$out_dir" \
  --build-property "compiler.cpp.extra_flags=-DFIRMWARE_VERSION=\"$version\"" \
  "$@" \
  "$sketch_dir"
