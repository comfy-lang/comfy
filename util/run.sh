#!/usr/bin/env bash
# Full pipeline for a single .cfy file: compile -> assemble -> link -> run.
# Meant to be run inside `nix develop`.
#
# Usage: util/run.sh examples/exit_code.cfy
set -euo pipefail

if [ -z "${1:-}" ]; then
  echo "Usage: $0 <file.cfy>" >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

OUTPUT="$(cargo run --quiet -- "$1")"
echo "$OUTPUT"

OUT_S="$(echo "$OUTPUT" | grep -oP '(?<=Wrote )\S+\.s')"
"$SCRIPT_DIR/link_compile.sh" "$OUT_S"

BIN="build/$(basename "$OUT_S" .s)"
"$SCRIPT_DIR/host-run.sh" "$BIN"
