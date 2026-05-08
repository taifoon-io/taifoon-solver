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
PROTOCOL_FILTER="${PROTOCOL_FILTER:-across,lifi}"

# Portfolio-capacity filters (narrow scope to what the wallet can actually fill).
#
# DST_CHAIN_FILTER — only process intents whose OUTPUT chain is in this list.
#   Current wallet: USDC on Base (8453) only.
#   Fill on Base for Across; Arbitrum (42161) / Optimism (10) when funded.
#   Empty string = accept all wired chains.
DST_CHAIN_FILTER="${DST_CHAIN_FILTER:-8453}"
#
# MAX_INPUT_USD — drop intents above this amount at intake (before li.quest API call).
#   Set to wallet's fillable capacity with a small safety margin.
#   Override: MAX_INPUT_USD=50 ./run-mainnet.sh
MAX_INPUT_USD="${MAX_INPUT_USD:-15}"
#
# MIN_INPUT_USD — ignore dust intents below this floor (saves enrichment RPC calls).
MIN_INPUT_USD="${MIN_INPUT_USD:-0.50}"

# Rebalancer + deBridge claim-retry run inside solver-main every SIDECAR_INTERVAL_SECS.
SIDECAR_INTERVAL_SECS="${SIDECAR_INTERVAL_SECS:-300}"
GENOME_SSE_URL="${GENOME_SSE_URL:-https://api.taifoon.dev/api/genome/subscribe/sse}"
SPINNER_API_URL="${SPINNER_API_URL:-https://api.taifoon.dev}"
SOLANA_RPC_URL="${SOLANA_RPC_URL:-https://api.mainnet-beta.solana.com}"

# Warn loudly if the operator is relying on the public Solana RPC. It works for
# dry-run and low-volume probing, but rate limits will throttle real fills.
if [[ "$SOLANA_RPC_URL" == "https://api.mainnet-beta.solana.com" ]]; then
    echo "WARN: Using public Solana RPC — rate limits apply. Set SOLANA_RPC_URL=https://mainnet.helius-rpc.com/?api-key=YOUR_KEY for production." >&2
fi
OUTCOME_DB_PATH="${OUTCOME_DB_PATH:-$REPO_ROOT/outcomes/mainnet_$(date +%Y%m%d_%H%M%S).sqlite}"
WALLET_DB_PATH="${WALLET_DB_PATH:-$REPO_ROOT/outcomes/wallet_mainnet.sqlite}"
RUST_LOG="${RUST_LOG:-taifoon_solver=info,executor=info,genome_client=info,warn}"

# --- Load EVM + Solana keys ---
# macOS: keys are read from Keychain (entries: mamba-messiah-key, mamba-messiah-solana-key).
# Linux / CI: inject via secrets manager before running this script, e.g.:
#   GitHub Actions : env: SOLVER_PRIVATE_KEY: ${{ secrets.SOLVER_PRIVATE_KEY }}
#   HashiCorp Vault: export SOLVER_PRIVATE_KEY="$(vault kv get -field=evm_key secret/taifoon-solver)"
#   AWS Secrets Mgr: export SOLVER_PRIVATE_KEY="$(aws secretsmanager get-secret-value --secret-id taifoon-solver/evm-key --query SecretString --output text)"
# See SECURITY_ONBOARDING.md §2.2.1 for full patterns.
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

# --- Derive SOLANA_ADDRESS from the keypair (extract pubkey = last 32 bytes of 64-byte keypair) ---
if [[ -z "${SOLANA_ADDRESS:-}" && -n "$SOLANA_PRIVATE_KEY" ]]; then
    KEYFILE_TMP=$(mktemp /tmp/sol_kp_XXXXXX.json)
    python3 -c "
import sys
ALPHA='123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'
k='$SOLANA_PRIVATE_KEY'.strip()
n=0
for c in k: n=n*58+ALPHA.index(c)
nb=(n.bit_length()+7)//8
raw=n.to_bytes(max(nb,1),'big')
if len(raw)==64: print('['+','.join(str(b) for b in raw)+']')
" > "$KEYFILE_TMP" 2>/dev/null
    if [[ -s "$KEYFILE_TMP" ]]; then
        SOLANA_ADDRESS=$(solana-keygen pubkey "$KEYFILE_TMP" 2>/dev/null || true)
    fi
    rm -f "$KEYFILE_TMP"
fi
# Fallback to the known solver Solana address
SOLANA_ADDRESS="${SOLANA_ADDRESS:-DUDgHSeM1KU9W8WyiMpEP7HtQKY22fRmpjxKViLEBQQF}"

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
echo " Solana address: ${SOLANA_ADDRESS:-not set}"
echo " Max notional:   \$$MAX_NOTIONAL_USD per fill"
echo " Min profit:     \$$MIN_PROFIT_USD per fill"
echo " Protocols:      $PROTOCOL_FILTER"
echo " Dst chains:     ${DST_CHAIN_FILTER:-all}"
echo " Amount range:   \$${MIN_INPUT_USD}–\$${MAX_INPUT_USD} input"
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
    SOLANA_ADDRESS="$SOLANA_ADDRESS" \
    DRY_RUN="$DRY_RUN" \
    SIMULATION_MODE="$SIMULATION_MODE" \
    MAX_NOTIONAL_USD="$MAX_NOTIONAL_USD" \
    MIN_PROFIT_USD="$MIN_PROFIT_USD" \
    PROTOCOL_FILTER="$PROTOCOL_FILTER" \
    DST_CHAIN_FILTER="$DST_CHAIN_FILTER" \
    MAX_INPUT_USD="$MAX_INPUT_USD" \
    MIN_INPUT_USD="$MIN_INPUT_USD" \
    SIDECAR_INTERVAL_SECS="$SIDECAR_INTERVAL_SECS" \
    GENOME_SSE_URL="$GENOME_SSE_URL" \
    SPINNER_API_URL="$SPINNER_API_URL" \
    SOLANA_RPC_URL="$SOLANA_RPC_URL" \
    CHAIN_WIRING_FILE="$REPO_ROOT/config/chain_wiring.json" \
    OUTCOME_DB_PATH="$OUTCOME_DB_PATH" \
    WALLET_DB_PATH="$WALLET_DB_PATH" \
    RUST_LOG="$RUST_LOG" \
    "$SOLVER_BIN"
