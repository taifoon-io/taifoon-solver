#!/usr/bin/env bash
# Mainnet verification gate — Phase 4 (lifi first-fill).
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
LEDGER="$REPO_ROOT/outcomes/verification_gates.jsonl"
mkdir -p "$REPO_ROOT/outcomes"

DRY_RUN=false MAX_NOTIONAL_USD="${MAX_NOTIONAL_USD:-200}" PROTOCOL_FILTER=lifi \
    "$REPO_ROOT/run-mainnet.sh" &
SOLVER_PID=$!
trap "kill -SIGTERM $SOLVER_PID 2>/dev/null || true" EXIT

WALLET_DB="$REPO_ROOT/outcomes/wallet_mainnet.sqlite"
DEADLINE=$(( $(date +%s) + 1800 ))
while (( $(date +%s) < DEADLINE )); do
    if [[ -f "$WALLET_DB" ]]; then
        # intents table: state=CONFIRMED, protocol contains lifi, tx_hash present.
        ROW=$(sqlite3 "$WALLET_DB" \
            "SELECT tx_hash, intent_id, dst_chain FROM intents \
             WHERE state='CONFIRMED' AND protocol LIKE '%lifi%' AND tx_hash IS NOT NULL \
             ORDER BY updated_at DESC LIMIT 1" 2>/dev/null || true)
        if [[ -n "$ROW" ]]; then
            tx=$(echo "$ROW" | awk -F'|' '{print $1}')
            intent=$(echo "$ROW" | awk -F'|' '{print $2}')
            chain=$(echo "$ROW" | awk -F'|' '{print $3}')
            ts=$(date -u +%FT%TZ)
            case "$chain" in
                1) explorer="https://etherscan.io/tx/$tx";;
                10) explorer="https://optimistic.etherscan.io/tx/$tx";;
                137) explorer="https://polygonscan.com/tx/$tx";;
                8453) explorer="https://basescan.org/tx/$tx";;
                42161) explorer="https://arbiscan.io/tx/$tx";;
                59144) explorer="https://lineascan.build/tx/$tx";;
                *) explorer="https://etherscan.io/tx/$tx";;
            esac
            echo "==> Phase 4 mainnet gate PASS: tx=$tx chain=$chain"
            echo "    Explorer: $explorer"
            printf '{"phase":4,"tx":"%s","explorer":"%s","ts":"%s","intent_id":"%s","dst_chain":%s}\n' \
                "$tx" "$explorer" "$ts" "$intent" "$chain" >> "$LEDGER"
            kill -SIGTERM "$SOLVER_PID" 2>/dev/null || true
            exit 0
        fi
    fi
    sleep 5
done

echo "==> Phase 4 mainnet gate TIMED OUT after 30 minutes"
exit 1
