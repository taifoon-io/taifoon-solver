#!/usr/bin/env bash
# Phase 6 acceptance gate — demo rehearsal prerequisites.
# Asserts that earlier phases all closed (entries in verification_gates.jsonl)
# and the local environment is ready for the demo.
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$REPO_ROOT"

LEDGER="$REPO_ROOT/outcomes/verification_gates.jsonl"
[[ -f "$LEDGER" ]] || { echo "[phase6] FAIL: verification_gates.jsonl missing"; exit 1; }

echo "[phase6] earlier phases must each have at least one entry in the gate ledger:"
for p in 1 2 3 4 5; do
    if grep -q "\"phase\":${p}\b" "$LEDGER"; then
        echo "  phase $p: present"
    else
        echo "  phase $p: MISSING — run ./tools/loops/loop_driver.sh $p first"
        exit 1
    fi
done

echo "[phase6] cargo build --workspace --release"
cargo build --workspace --release

echo "[phase6] check run-mainnet.sh present + executable"
[[ -x "$REPO_ROOT/run-mainnet.sh" ]]

echo "[phase6] PASS — ready for demo rehearsal"
