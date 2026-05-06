#!/usr/bin/env bash
# Mainnet verification gate — Phase 1 (Mayan-Solana first-fill).
# Run by MB. Requires SOLVER_PRIVATE_KEY + SOLANA_PRIVATE_KEY in keychain
# and a funded Messiah Solana keypair.
#
# This script:
#   1. Starts run-mainnet.sh with PROTOCOL_FILTER=mayan, MAX_NOTIONAL_USD=50
#   2. Tails the outcome SQLite for one row with decision='confirmed' AND protocol matching mayan AND tx_hash NOT NULL
#   3. Appends an entry to outcomes/verification_gates.jsonl
#
# Abort: Ctrl-C cleanly stops the solver and the tail.
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
LEDGER="$REPO_ROOT/outcomes/verification_gates.jsonl"
mkdir -p "$REPO_ROOT/outcomes"

DRY_RUN=false MAX_NOTIONAL_USD="${MAX_NOTIONAL_USD:-50}" PROTOCOL_FILTER=mayan \
    "$REPO_ROOT/run-mainnet.sh" &
SOLVER_PID=$!
trap "kill -SIGTERM $SOLVER_PID 2>/dev/null || true" EXIT

# Wait up to 30 minutes for the first confirmed Mayan-Solana fill.
DEADLINE=$(( $(date +%s) + 1800 ))
while (( $(date +%s) < DEADLINE )); do
    DB=$(ls -t "$REPO_ROOT"/outcomes/mainnet_*.sqlite 2>/dev/null | head -1 || true)
    if [[ -n "$DB" ]]; then
        ROW=$(sqlite3 "$DB" "select tx_hash, actual_profit_usd, intent_id from solver_outcomes where decision='confirmed' and protocol like '%mayan%' and tx_hash is not null order by ts desc limit 1" || true)
        if [[ -n "$ROW" ]]; then
            tx=$(echo "$ROW" | awk -F'|' '{print $1}')
            profit=$(echo "$ROW" | awk -F'|' '{print $2}')
            intent=$(echo "$ROW" | awk -F'|' '{print $3}')
            ts=$(date -u +%FT%TZ)
            echo "==> Phase 1 mainnet gate PASS: tx=$tx profit=$profit"
            printf '{"phase":1,"tx":"%s","explorer":"https://solscan.io/tx/%s","ts":"%s","intent_id":"%s","realized_usd":%s}\n' \
                "$tx" "$tx" "$ts" "$intent" "${profit:-null}" >> "$LEDGER"
            kill -SIGTERM "$SOLVER_PID" 2>/dev/null || true
            exit 0
        fi
    fi
    sleep 5
done

echo "==> Phase 1 mainnet gate TIMED OUT after 30 minutes"
echo "    No confirmed Mayan-Solana fill recorded."
exit 1
