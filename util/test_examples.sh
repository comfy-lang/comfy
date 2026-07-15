#!/usr/bin/env bash
# Compiles and runs every example, checking its exit code against the
# expected value documented in a trailing "// exit N" comment. Meant to be
# run inside `nix develop`.
#
# Deliberately doesn't use `set -e` - a single failing example shouldn't
# abort the whole run, since we want a full pass/fail report at the end.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

# path|expected -- "error" means it must fail to compile, "skip" means the
# exit code is nondeterministic and shouldn't be checked at all.
EXAMPLES=(
  "examples/exit_code.cfy|42"
  "examples/functions/basic_math.cfy|125"
  "examples/logic/if_while.cfy|1"
  "examples/logic/short_circuit.cfy|7"
  "examples/math/arithmetic_folding.cfy|43"
  "examples/math/div_by_zero.cfy|error"
  "examples/pointers/read_write.cfy|42"
  "examples/showcase.cfy|39"
  "examples/structs/array_field.cfy|139"
  "examples/structs/nested.cfy|32"
  "examples/variables/basic_arrays.cfy|169"
  "examples/variables/const_assign.cfy|error"
  "examples/variables/mutability.cfy|41"
  "examples/variables/syscall_expr.cfy|0"
  "examples/variables/syscall_expr_pid.cfy|skip"
  "examples/optimization/opt_control_flow.cfy|42"
)

echo "Building comfyc..."
RUSTFLAGS="-Awarnings" cargo build --quiet

PASS=0
FAIL=0
SKIP=0

for ENTRY in "${EXAMPLES[@]}"; do
  FILE="${ENTRY%%|*}"
  EXPECTED="${ENTRY##*|}"

  if [ "$EXPECTED" = "skip" ]; then
    echo "SKIP  $FILE (nondeterministic)"
    SKIP=$((SKIP + 1))
    continue
  fi

  COMPILE_OUT="$(RUSTFLAGS="-Awarnings" cargo run --quiet -- "$FILE" 2>&1)"
  COMPILE_STATUS=$?

  if [ "$COMPILE_STATUS" -ne 0 ]; then
    if [ "$EXPECTED" = "error" ]; then
      echo "PASS  $FILE (compile error, as expected)"
      PASS=$((PASS + 1))
    else
      echo "FAIL  $FILE - expected exit $EXPECTED, but compilation failed:"
      echo "$COMPILE_OUT" | sed 's/^/         /'
      FAIL=$((FAIL + 1))
    fi
    continue
  fi

  if [ "$EXPECTED" = "error" ]; then
    echo "FAIL  $FILE - expected a compile error, but it compiled successfully"
    FAIL=$((FAIL + 1))
    continue
  fi

  OUT_S="$(echo "$COMPILE_OUT" | grep -oP '(?<=Wrote )\S+\.s')"
  "$SCRIPT_DIR/link_compile.sh" "$OUT_S" >/dev/null
  BIN="build/$(basename "$OUT_S" .s)"

  case "$(uname -m)" in
    arm|armv6l|armv7l)
      "$BIN" >/dev/null 2>&1
      ACTUAL=$?
      ;;
    *)
      QEMU_BIN="${QEMU_ARM:-qemu-arm}"
      "$QEMU_BIN" "$BIN" >/dev/null 2>&1
      ACTUAL=$?
      ;;
  esac

  if [ "$ACTUAL" -eq "$EXPECTED" ]; then
    echo "PASS  $FILE (exit $ACTUAL)"
    PASS=$((PASS + 1))
  else
    echo "FAIL  $FILE - expected exit $EXPECTED, got $ACTUAL"
    FAIL=$((FAIL + 1))
  fi
done

echo
echo "$PASS passed, $FAIL failed, $SKIP skipped"
[ "$FAIL" -eq 0 ]
