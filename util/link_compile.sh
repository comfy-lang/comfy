#!/usr/bin/env bash
# Assembles and links a comfyc-generated .s file into an ARM32 ELF binary.
#
# Uses the cross toolchain provided by the flake (`nix develop`), which sets
# $COMFYC_CROSS_PREFIX to the right `as`/`ld` prefix for the host running
# this script. Falls back to the common Debian/Ubuntu package prefix if run
# outside the dev shell.
set -euo pipefail

if [ -z "${1:-}" ]; then
  echo "Usage: $0 <source_file.s>" >&2
  exit 1
fi

SRC="$1"
PREFIX="${COMFYC_CROSS_PREFIX:-arm-linux-gnueabihf-}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BUILD_DIR="${BUILD_DIR:-$REPO_ROOT/build}"
mkdir -p "$BUILD_DIR"

BASENAME="$(basename "$SRC" .s)"

"${PREFIX}as" -g -o "$BUILD_DIR/${BASENAME}.o" "$SRC"
"${PREFIX}ld" -o "$BUILD_DIR/$BASENAME" "$BUILD_DIR/${BASENAME}.o" \
  -Ttext=0x10000 --no-dynamic-linker -nostdlib

echo "Built $BUILD_DIR/$BASENAME (prefix: ${PREFIX})"
