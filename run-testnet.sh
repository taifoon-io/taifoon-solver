#!/usr/bin/env bash
# Run solver in LIVE testnet mode against Base Sepolia + Arb Sepolia.
# Requires: SOLVER_PRIVATE_KEY env var or mamba-messiah-key in keychain.
# Usage: ./run-testnet.sh [--dry-run]
set -euo pipefail

DRY_RUN="${DRY_RUN:-false}"
if [[ "${1:-}" == "--dry-run" ]]; then DRY_RUN=true; fi

# Load private key from keychain if not already set
if [[ -z "${SOLVER_PRIVATE_KEY:-}" ]]; then
    SOLVER_PRIVATE_KEY=$(security find-generic-password -s mamba-messiah-key -w 2>/dev/null || true)
    if [[ -z "$SOLVER_PRIVATE_KEY" ]]; then
        echo "ERROR: SOLVER_PRIVATE_KEY not set and mamba-messiah-key not found in keychain" >&2
        exit 1
    fi
fi

# Verify wallet has funds on Base Sepolia
ADDR=$(cast wallet address "$SOLVER_PRIVATE_KEY" 2>/dev/null || true)
echo "Solver address: $ADDR"
BASE_SEP_BAL=$(cast balance "$ADDR" --rpc-url https://sepolia.base.org 2>/dev/null || echo "0")
echo "Base Sepolia balance: $BASE_SEP_BAL wei"
if [[ "$BASE_SEP_BAL" == "0" && "$DRY_RUN" == "false" ]]; then
    echo "WARNING: Zero ETH on Base Sepolia — fills will fail unless you have testnet funds."
    echo "  Fund $ADDR on Base Sepolia, then retry."
    echo "  Or run: DRY_RUN=true ./run-testnet.sh"
fi

REPO_ROOT="$(cd "$(dirname "$0")" && pwd)"

SOLVER_PRIVATE_KEY="$SOLVER_PRIVATE_KEY" \
DRY_RUN="$DRY_RUN" \
GENOME_SSE_URL="http://127.0.0.1:30081/api/genome/subscribe/sse" \
SPINNER_API_URL="http://127.0.0.1:30081" \
CHAIN_WIRING_FILE="$REPO_ROOT/config/chain_wiring.json" \
PROTOCOL_FILTER="across" \
MIN_PROFIT_USD="0.0" \
OUTCOME_DB_PATH="/tmp/taifoon_testnet_outcomes.sqlite" \
WALLET_DB_PATH="/tmp/taifoon_testnet_wallet.sqlite" \
RUST_LOG="taifoon_solver=debug,executor=debug,genome_client=debug,warn" \
  "$REPO_ROOT/target/release/taifoon-solver" 2>&1
