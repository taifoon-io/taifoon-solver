#!/usr/bin/env bash
# lwc_dry_run.sh — end-to-end dry-run validation for the LWC fill path.
#
# Usage:
#   scripts/lwc_dry_run.sh
#
# Runs taifoon-solver with LWC enabled + DRY_RUN and validates that:
#   1. T3RN_LWC_ENABLED is picked up
#   2. [LWC] scan log lines appear
#   3. Reports how many chains were scanned
#   4. Reports whether any chain returned non-zero pool_available_usd
#
# Exit 0 on success, 1 on failure.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# ── Environment ──────────────────────────────────────────────────────────────
export T3RN_LWC_ENABLED=true
export DRY_RUN=true
export SOLVER_PRIVATE_KEY="0x0000000000000000000000000000000000000000000000000000000000000001"
export GENOME_SSE_URL="https://genome.taifoon.dev/api/genome/subscribe/sse"
export SPINNER_API_URL="https://api.taifoon.dev"

# Point at the config directory so lwc_manager can find lwc_deployments.json.
export LWC_DEPLOYMENTS_PATH="${WORKSPACE_ROOT}/config/lwc_deployments.json"

echo "[lwc_dry_run] Starting taifoon-solver with:"
echo "  T3RN_LWC_ENABLED=${T3RN_LWC_ENABLED}"
echo "  DRY_RUN=${DRY_RUN}"
echo "  LWC_DEPLOYMENTS_PATH=${LWC_DEPLOYMENTS_PATH}"
echo ""

# ── Run solver (capture first 100 lines of output) ───────────────────────────
cd "${WORKSPACE_ROOT}"

OUTPUT_FILE="$(mktemp /tmp/lwc_dry_run_XXXXXX.log)"
trap 'rm -f "${OUTPUT_FILE}"' EXIT

echo "[lwc_dry_run] Building taifoon-solver..."
cargo build --bin taifoon-solver 2>&1 | tail -5

echo "[lwc_dry_run] Running solver (collecting up to 100 lines or 30s)..."
timeout 30 cargo run --bin taifoon-solver 2>&1 | head -100 > "${OUTPUT_FILE}" || true

echo ""
echo "──────────────── Solver output ────────────────"
cat "${OUTPUT_FILE}"
echo "────────────────────────────────────────────────"
echo ""

# ── Analysis ─────────────────────────────────────────────────────────────────

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

# 1. T3RN_LWC_ENABLED was picked up (look for "T3RN LWC enabled" or "[LWC]" lines).
lwc_picked_up=0
if grep -qiE "(T3RN LWC enabled|LWC_ENABLED|lwc_manager)" "${OUTPUT_FILE}" 2>/dev/null; then
    lwc_picked_up=1
fi
check "T3RN_LWC_ENABLED picked up" "${lwc_picked_up}"

# 2. Any [LWC] scan log lines appear.
lwc_scan_lines=$(grep -c "\[LWC\]" "${OUTPUT_FILE}" 2>/dev/null || true)
check "[LWC] scan lines present (found ${lwc_scan_lines})" "$( [ "${lwc_scan_lines}" -gt 0 ] && echo 1 || echo 0 )"

# 3. Count how many chains were scanned (lines containing "well chain=").
chains_scanned=$(grep -c "well chain=" "${OUTPUT_FILE}" 2>/dev/null || true)
echo ""
echo "[lwc_dry_run] Chains scanned: ${chains_scanned}"

# 4. Any chain returned non-zero pool_available_usd?
# Log lines look like: "avail=$NN.NN" — match anything > $0.00.
non_zero_pools=$(grep -oE 'avail=\$[0-9]+\.[0-9]+' "${OUTPUT_FILE}" 2>/dev/null \
    | grep -v 'avail=\$0\.00' | wc -l | tr -d ' ' || true)
echo "[lwc_dry_run] Chains with non-zero pool_available_usd: ${non_zero_pools}"

# ── Summary ───────────────────────────────────────────────────────────────────
echo ""
echo "──────────────── Result ─────────────────────────"
echo "  PASS: ${PASS}"
echo "  FAIL: ${FAIL}"
echo "  Chains scanned:       ${chains_scanned}"
echo "  Non-zero pools found: ${non_zero_pools}"
echo "─────────────────────────────────────────────────"
echo ""

if [ "${FAIL}" -gt 0 ]; then
    echo "[lwc_dry_run] FAILED (${FAIL} check(s) did not pass)"
    exit 1
fi

echo "[lwc_dry_run] All checks passed"
exit 0
