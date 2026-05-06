#!/usr/bin/env bash
# Run solver in LIVE MAINNET mode against the four configured bridges
# (Across, deBridge, LiFi, Mayan) on EVM + Solana.
#
# This is the Frontier Hackathon demo runbook. It:
#   - Loads SOLVER_PRIVATE_KEY (EVM) + SOLANA_PRIVATE_KEY (Solana) from keychain
#   - Refuses to start if any required env is missing
#   - Prints solver address + balance on every wired chain BEFORE subscribing
#   - Defaults to DRY_RUN=true (no broadcasts) — must set DRY_RUN=false explicitly
#   - Hard caps every fill at MAX_NOTIONAL_USD (default $200)
#   - Persists a fresh outcome SQLite under ./outcomes/ for per-run trace
#
# Usage:
#   ./run-mainnet.sh                 # dry-run (default — safe)
#   DRY_RUN=false ./run-mainnet.sh   # broadcasts on confirmation
#   DRY_RUN=false MAX_NOTIONAL_USD=50 ./run-mainnet.sh
#
# Kill switch: pkill -SIGTERM taifoon-solver
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")" && pwd)"

# --- Defaults (safe) ---
DRY_RUN="${DRY_RUN:-true}"
SIMULATION_MODE="${SIMULATION_MODE:-$DRY_RUN}"
MAX_NOTIONAL_USD="${MAX_NOTIONAL_USD:-200}"
MIN_PROFIT_USD="${MIN_PROFIT_USD:-0.50}"
PROTOCOL_FILTER="${PROTOCOL_FILTER:-across,debridge,lifi,mayan}"
# Rebalancer + deBridge claim-retry run inside solver-main every SIDECAR_INTERVAL_SECS.
SIDECAR_INTERVAL_SECS="${SIDECAR_INTERVAL_SECS:-300}"
GENOME_SSE_URL="${GENOME_SSE_URL:-https://api.taifoon.dev/api/genome/subscribe/sse}"
SPINNER_API_URL="${SPINNER_API_URL:-https://api.taifoon.dev}"
SOLANA_RPC_URL="${SOLANA_RPC_URL:-https://mainnet.helius-rpc.com/?api-key=b8dca1eb-aec9-4399-8906-c496da99db29}"
OUTCOME_DB_PATH="${OUTCOME_DB_PATH:-$REPO_ROOT/outcomes/mainnet_$(date +%Y%m%d_%H%M%S).sqlite}"
WALLET_DB_PATH="${WALLET_DB_PATH:-$REPO_ROOT/outcomes/wallet_mainnet.sqlite}"
RUST_LOG="${RUST_LOG:-taifoon_solver=info,executor=info,genome_client=info,warn}"

# --- Load EVM key from keychain if not set ---
if [[ -z "${SOLVER_PRIVATE_KEY:-}" ]]; then
    SOLVER_PRIVATE_KEY=$(security find-generic-password -s mamba-messiah-key -w 2>/dev/null || true)
    if [[ -z "$SOLVER_PRIVATE_KEY" ]]; then
        echo "ERROR: SOLVER_PRIVATE_KEY not set and 'mamba-messiah-key' not in macOS keychain." >&2
        echo "  Add with: security add-generic-password -a \$USER -s mamba-messiah-key -w 0xYOUR_KEY" >&2
        exit 1
    fi
fi

# --- Load Solana key from keychain if not set ---
if [[ -z "${SOLANA_PRIVATE_KEY:-}" ]]; then
    SOLANA_PRIVATE_KEY=$(security find-generic-password -s mamba-messiah-solana-key -w 2>/dev/null || true)
    if [[ -z "$SOLANA_PRIVATE_KEY" ]]; then
        echo "WARN: SOLANA_PRIVATE_KEY not set and 'mamba-messiah-solana-key' not in keychain." >&2
        echo "      Mayan-Solana fills will be skipped at the broadcaster step." >&2
        SOLANA_PRIVATE_KEY=""
    fi
fi

# --- Resolve solver EVM address (informational; cast is optional) ---
ADDR=""
if command -v cast >/dev/null 2>&1; then
    ADDR=$(cast wallet address "$SOLVER_PRIVATE_KEY" 2>/dev/null || true)
fi

# --- Pre-flight: print balances on every wired EVM chain ---
echo "==================== Taifoon Solver — MAINNET ===================="
echo " Mode:           $([[ "$DRY_RUN" == "true" ]] && echo 'DRY-RUN (no broadcasts)' || echo 'LIVE — WILL BROADCAST')"
echo " Solver (EVM):   ${ADDR:-<install foundry to decode>}"
echo " Solana key:     $([[ -n "$SOLANA_PRIVATE_KEY" ]] && echo 'loaded' || echo 'MISSING — Solana fills will skip')"
echo " Max notional:   \$$MAX_NOTIONAL_USD per fill"
echo " Min profit:     \$$MIN_PROFIT_USD per fill"
echo " Protocols:      $PROTOCOL_FILTER"
echo " Outcome DB:     $OUTCOME_DB_PATH"
echo " Genome SSE:     $GENOME_SSE_URL"
echo "==================================================================="

if [[ -n "$ADDR" && "$DRY_RUN" == "false" ]]; then
    echo
    echo "Pre-flight balances (mainnet):"
    declare -A RPC=(
        ["Ethereum"]="https://ethereum-rpc.publicnode.com"
        ["Optimism"]="https://mainnet.optimism.io"
        ["Polygon"]="https://polygon-bor-rpc.publicnode.com"
        ["Base"]="https://base-rpc.publicnode.com"
        ["Arbitrum"]="https://arb1.arbitrum.io/rpc"
    )
    for chain in "${!RPC[@]}"; do
        bal=$(cast balance "$ADDR" --rpc-url "${RPC[$chain]}" 2>/dev/null || echo "?")
        echo "  $chain: $bal wei"
    done
    echo
    echo "*** This will broadcast LIVE TRANSACTIONS on mainnet."
    echo "*** Fills are capped at \$$MAX_NOTIONAL_USD notional, profit-gated at \$$MIN_PROFIT_USD."
    if [[ "${YES_I_AM_SURE:-}" == "1" ]]; then
        echo "Auto-confirmed (YES_I_AM_SURE=1)."
    else
        read -r -p "Type 'GO LIVE' to confirm: " confirm
        if [[ "$confirm" != "GO LIVE" ]]; then
            echo "Aborted. Re-run without DRY_RUN=false to dry-run instead." >&2
            exit 1
        fi
    fi
fi

# --- Ensure outcomes directory exists ---
mkdir -p "$(dirname "$OUTCOME_DB_PATH")"

# --- Verify binary exists ---
SOLVER_BIN="$REPO_ROOT/target/release/taifoon-solver"
if [[ ! -x "$SOLVER_BIN" ]]; then
    echo "ERROR: $SOLVER_BIN not found. Run: cargo build --workspace --release" >&2
    exit 1
fi

# --- Launch ---
exec env \
    SOLVER_PRIVATE_KEY="$SOLVER_PRIVATE_KEY" \
    SOLANA_PRIVATE_KEY="$SOLANA_PRIVATE_KEY" \
    DRY_RUN="$DRY_RUN" \
    SIMULATION_MODE="$SIMULATION_MODE" \
    MAX_NOTIONAL_USD="$MAX_NOTIONAL_USD" \
    MIN_PROFIT_USD="$MIN_PROFIT_USD" \
    PROTOCOL_FILTER="$PROTOCOL_FILTER" \
    SIDECAR_INTERVAL_SECS="$SIDECAR_INTERVAL_SECS" \
    GENOME_SSE_URL="$GENOME_SSE_URL" \
    SPINNER_API_URL="$SPINNER_API_URL" \
    SOLANA_RPC_URL="$SOLANA_RPC_URL" \
    CHAIN_WIRING_FILE="$REPO_ROOT/config/chain_wiring.json" \
    OUTCOME_DB_PATH="$OUTCOME_DB_PATH" \
    WALLET_DB_PATH="$WALLET_DB_PATH" \
    RUST_LOG="$RUST_LOG" \
    "$SOLVER_BIN"
