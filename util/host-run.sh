#!/usr/bin/env bash
# Runs an ARM32 comfyc binary regardless of the host's CPU architecture:
# native execution on an ARM32 host, transparent emulation via `qemu-arm`
# everywhere else (as provided by `nix develop`).
set -euo pipefail

if [ -z "${1:-}" ]; then
  echo "Usage: $0 <binary_path> [args...]" >&2
  exit 1
fi

BIN="$1"
shift || true

case "$(uname -m)" in
  arm|armv6l|armv7l)
    exec "$BIN" "$@"
    ;;
  *)
    QEMU_BIN="${QEMU_ARM:-qemu-arm}"
    if ! command -v "$QEMU_BIN" >/dev/null 2>&1; then
      echo "error: $QEMU_BIN not found. Enter the dev shell first: nix develop" >&2
      exit 1
    fi
    exec "$QEMU_BIN" "$BIN" "$@"
    ;;
esac
