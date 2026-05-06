#!/usr/bin/env bash
# Mainnet verification gate — Phase 4 (lifi first-fill).
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
LEDGER="$REPO_ROOT/outcomes/verification_gates.jsonl"
mkdir -p "$REPO_ROOT/outcomes"

DRY_RUN=false MAX_NOTIONAL_USD="${MAX_NOTIONAL_USD:-50}" PROTOCOL_FILTER=lifi \
    "$REPO_ROOT/run-mainnet.sh" &
SOLVER_PID=$!
trap "kill -SIGTERM $SOLVER_PID 2>/dev/null || true" EXIT

DEADLINE=$(( $(date +%s) + 1800 ))
while (( $(date +%s) < DEADLINE )); do
    DB=$(ls -t "$REPO_ROOT"/outcomes/mainnet_*.sqlite 2>/dev/null | head -1 || true)
    if [[ -n "$DB" ]]; then
        ROW=$(sqlite3 "$DB" "select tx_hash, actual_profit_usd, intent_id, dst_chain from solver_outcomes where decision='confirmed' and protocol like '%lifi%' and tx_hash is not null order by ts desc limit 1" || true)
        if [[ -n "$ROW" ]]; then
            tx=$(echo "$ROW" | awk -F'|' '{print $1}')
            profit=$(echo "$ROW" | awk -F'|' '{print $2}')
            intent=$(echo "$ROW" | awk -F'|' '{print $3}')
            chain=$(echo "$ROW" | awk -F'|' '{print $4}')
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
            echo "==> Phase 4 mainnet gate PASS: tx=$tx profit=$profit chain=$chain"
            printf '{"phase":4,"tx":"%s","explorer":"%s","ts":"%s","intent_id":"%s","dst_chain":%s,"realized_usd":%s}\n' \
                "$tx" "$explorer" "$ts" "$intent" "$chain" "${profit:-null}" >> "$LEDGER"
            kill -SIGTERM "$SOLVER_PID" 2>/dev/null || true
            exit 0
        fi
    fi
    sleep 5
done

echo "==> Phase 4 mainnet gate TIMED OUT after 30 minutes"
exit 1
