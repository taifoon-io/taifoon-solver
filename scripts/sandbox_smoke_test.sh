#!/usr/bin/env bash
# sandbox_smoke_test.sh — smoke test for the taifoon-sandbox binary.
#
# Usage:
#   scripts/sandbox_smoke_test.sh
#
# Runs:
#   cargo run --bin taifoon-sandbox -- compete --solvers 2 --duration 10 --speed 100
#
# Asserts:
#   - Output contains "total_fills" (JSON leaderboard printed at end)
#
# Exit 0 on success, 1 on failure.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

echo "[sandbox_smoke] Building taifoon-sandbox..."
cd "${WORKSPACE_ROOT}"
cargo build --bin taifoon-sandbox 2>&1 | tail -5

echo "[sandbox_smoke] Running: taifoon-sandbox compete --solvers 2 --duration 10 --speed 100"
OUTPUT_FILE="$(mktemp /tmp/sandbox_smoke_XXXXXX.log)"
trap 'rm -f "${OUTPUT_FILE}"' EXIT

# Run with a safety timeout of 60 seconds.
timeout 60 cargo run --bin taifoon-sandbox -- compete \
    --solvers 2 \
    --duration 10 \
    --speed 100 \
    2>&1 | tee "${OUTPUT_FILE}"

echo ""
echo "──────────────── Smoke test checks ─────────────────"

PASS=0
FAIL=0

check() {
    local desc="$1"
    local result="$2"
    if [ "${result}" = "1" ]; then
        echo "  PASS  ${desc}"
        PASS=$((PASS + 1))
    else
        echo "  FAIL  ${desc}"
        FAIL=$((FAIL + 1))
    fi
}

# Assert: output contains "total_fills" (the JSON leaderboard field).
has_total_fills=0
if grep -q '"total_fills"' "${OUTPUT_FILE}" 2>/dev/null; then
    has_total_fills=1
fi
check '"total_fills" in output' "${has_total_fills}"

# Assert: output contains "solvers" array field.
has_solvers=0
if grep -q '"solvers"' "${OUTPUT_FILE}" 2>/dev/null; then
    has_solvers=1
fi
check '"solvers" array in output' "${has_solvers}"

# Bonus: report total fills value for information.
total_fills=$(grep -oE '"total_fills"\s*:\s*[0-9]+' "${OUTPUT_FILE}" | grep -oE '[0-9]+$' | head -1 || echo "?")
echo ""
echo "  total_fills reported: ${total_fills}"

echo "────────────────────────────────────────────────────"

if [ "${FAIL}" -gt 0 ]; then
    echo "[sandbox_smoke] FAILED (${FAIL} check(s) did not pass)"
    exit 1
fi

echo "[sandbox_smoke] All checks passed"
exit 0
