#!/usr/bin/env bash
# Continuous solver runner with auto-restart and log rotation.
#
# Runs the solver in an infinite loop, restarting on exit (crash or kill).
# Logs to ./outcomes/solver_<date>.log with the last 5000 lines kept per run.
#
# Usage:
#   tools/run_continuous.sh                   # dry-run (default)
#   YES_I_AM_SURE=1 DRY_RUN=false tools/run_continuous.sh
#   YES_I_AM_SURE=1 DRY_RUN=false MAX_NOTIONAL_USD=5 PROTOCOL_FILTER=across tools/run_continuous.sh
#
# Stop: Ctrl+C or: pkill -f run_continuous
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SOLVER_BIN="$REPO_ROOT/target/release/taifoon-solver"
LOG_DIR="$REPO_ROOT/outcomes"

DRY_RUN="${DRY_RUN:-true}"
MAX_NOTIONAL_USD="${MAX_NOTIONAL_USD:-200}"
MIN_PROFIT_USD="${MIN_PROFIT_USD:-0.50}"
PROTOCOL_FILTER="${PROTOCOL_FILTER:-across,debridge,lifi,mayan}"
RESTART_DELAY="${RESTART_DELAY:-5}"

mkdir -p "$LOG_DIR"

if [[ ! -x "$SOLVER_BIN" ]]; then
    echo "ERROR: $SOLVER_BIN not found. Run: cargo build --workspace --release" >&2
    exit 1
fi

SOLVER_PRIVATE_KEY="${SOLVER_PRIVATE_KEY:-$(security find-generic-password -s mamba-messiah-key -w 2>/dev/null || true)}"
if [[ -z "$SOLVER_PRIVATE_KEY" ]]; then
    echo "ERROR: no private key" >&2; exit 1
fi
SOLANA_PRIVATE_KEY="${SOLANA_PRIVATE_KEY:-$(security find-generic-password -s mamba-messiah-solana-key -w 2>/dev/null || true)}"
SOLANA_ADDRESS="${SOLANA_ADDRESS:-DUDgHSeM1KU9W8WyiMpEP7HtQKY22fRmpjxKViLEBQQF}"
LIFI_API_KEY="${LIFI_API_KEY:-$(security find-generic-password -s lifi-solver-key -w 2>/dev/null || true)}"

RUN=0
trap 'echo "Stopping continuous runner..."; exit 0' INT TERM

while true; do
    RUN=$((RUN + 1))
    LOG="$LOG_DIR/solver_$(date +%Y%m%d_%H%M%S)_run${RUN}.log"
    DB="$LOG_DIR/mainnet_$(date +%Y%m%d_%H%M%S)_run${RUN}.sqlite"

    echo "[$(date -u '+%H:%M:%S UTC')] Starting run #$RUN (DRY_RUN=$DRY_RUN, protocol=$PROTOCOL_FILTER, notional=$MAX_NOTIONAL_USD)"
    echo "  Log: $LOG"

    env \
        SOLVER_PRIVATE_KEY="$SOLVER_PRIVATE_KEY" \
        SOLANA_PRIVATE_KEY="$SOLANA_PRIVATE_KEY" \
        SOLANA_ADDRESS="$SOLANA_ADDRESS" \
        LIFI_API_KEY="$LIFI_API_KEY" \
        DRY_RUN="$DRY_RUN" \
        SIMULATION_MODE="$DRY_RUN" \
        MAX_NOTIONAL_USD="$MAX_NOTIONAL_USD" \
        MIN_PROFIT_USD="$MIN_PROFIT_USD" \
        PROTOCOL_FILTER="$PROTOCOL_FILTER" \
        T3RN_LWC_ENABLED=false \
        CHAIN_WIRING_FILE="$REPO_ROOT/config/chain_wiring.json" \
        OUTCOME_DB_PATH="$DB" \
        WALLET_DB_PATH="$LOG_DIR/wallet_mainnet.sqlite" \
        SOLANA_RPC_URL="${SOLANA_RPC_URL:-https://api.mainnet-beta.solana.com}" \
        RUST_LOG="${RUST_LOG:-genome_client=info,executor=info,warn}" \
        "$SOLVER_BIN" 2>&1 | tee "$LOG" || true

    EXIT=$?
    echo "[$(date -u '+%H:%M:%S UTC')] Run #$RUN exited (code $EXIT). Restarting in ${RESTART_DELAY}s..."

    # Show last fill/outcome from the log
    tail -20 "$LOG" | grep -E "(confirmed|filled|FILL|profit|error|Error)" | tail -5 || true

    sleep "$RESTART_DELAY"
done
