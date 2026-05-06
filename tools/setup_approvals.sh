#!/usr/bin/env bash
# One-time setup: approve protocol contracts to spend solver's tokens.
#
# Must be run once before live fills. Approves max uint256 to:
#   - Across V3 SpokePool on Optimism, Base, Arbitrum (for USDC fills)
#   - deBridge DlnDestination on all supported chains (for USDC fills)
#
# Usage:
#   ./tools/setup_approvals.sh --dry-run      # preview, no broadcasts
#   ./tools/setup_approvals.sh                # broadcast approvals

set -euo pipefail

DRY_RUN="${1:-}"
MAX_UINT256="115792089237316195423570985008687907853269984665640564039457584007913129639935"

# Load private key from keychain
PK=$(security find-generic-password -s mamba-messiah-key -w 2>/dev/null || true)
if [[ -z "$PK" ]]; then
    PK="${SOLVER_PRIVATE_KEY:-}"
fi
if [[ -z "$PK" ]]; then
    echo "ERROR: No private key. Set SOLVER_PRIVATE_KEY or add 'mamba-messiah-key' to keychain." >&2
    exit 1
fi

SOLVER=$(cast wallet address "$PK" 2>/dev/null || echo "unknown")
echo "Solver: $SOLVER"
echo "Mode: $([[ "$DRY_RUN" == "--dry-run" ]] && echo 'DRY-RUN (no broadcasts)' || echo 'LIVE')"
echo ""

# Protocol contract addresses
DLN_DEST="0xE7351Fd770A37282b91D153Ee690B63579D6dd7f"  # deBridge DlnDestination (all chains)

declare -A SPOKE_POOLS=(
    [10]="0x6f26Bf09B1C792e3228e5467807a900A503c0281"    # Optimism
    [8453]="0x09aea4b2242abC8bb4BB78D537A67a245A7bEC64"  # Base
    [42161]="0xe35e9842fceaCA96570B734083f4a58e8F7C5f2A"  # Arbitrum
)

declare -A USDC_ADDRS=(
    [10]="0x0b2C639c533813f4Aa9D7837CAf62653d097Ff85"    # Optimism native USDC
    [8453]="0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"  # Base USDC
    [42161]="0xaf88d065e77c8cC2239327C5EDb3A432268e5831"  # Arbitrum native USDC
)

declare -A RPCS=(
    [10]="https://mainnet.optimism.io"
    [8453]="https://base-rpc.publicnode.com"
    [42161]="https://arb1.arbitrum.io/rpc"
)

approve() {
    local chain=$1 token=$2 spender=$3 label=$4
    local rpc="${RPCS[$chain]}"
    local usdc="${USDC_ADDRS[$chain]}"

    if [[ "$token" != "$usdc" ]]; then return; fi  # only USDC for now

    # Check current allowance (cast may include "[1.157e77]" annotation — strip it)
    local allow
    allow=$(cast call "$token" "allowance(address,address)(uint256)" "$SOLVER" "$spender" \
        --rpc-url "$rpc" 2>/dev/null | head -1 | awk '{print $1}' || echo "0")

    echo "Chain $chain ($label): allowance=$allow"

    # Consider already-approved if allowance > 10^30 (well above any realistic fill).
    # Exact MAX_UINT256 check fails when tokens have been spent from the allowance.
    if python3 -c "import sys; sys.exit(0 if int('$allow' or '0') > 10**30 else 1)" 2>/dev/null; then
        echo "  ✅ Already max-approved (residual allowance)"
        return
    fi

    if [[ "$DRY_RUN" == "--dry-run" ]]; then
        echo "  [DRY-RUN] Would approve $token → $spender on chain $chain"
        return
    fi

    echo "  Approving $token → $spender on chain $chain..."
    cast send "$token" "approve(address,uint256)" "$spender" "$MAX_UINT256" \
        --rpc-url "$rpc" \
        --private-key "$PK" 2>&1 | grep -E "transactionHash|status|error" || true
    echo "  ✅ Approved"
}

echo "=== Across SpokePool approvals ==="
for chain in 10 8453 42161; do
    usdc="${USDC_ADDRS[$chain]}"
    spoke="${SPOKE_POOLS[$chain]}"
    approve "$chain" "$usdc" "$spoke" "Across SpokePool"
done

echo ""
echo "=== deBridge DlnDestination approvals ==="
for chain in 10 8453 42161; do
    usdc="${USDC_ADDRS[$chain]}"
    approve "$chain" "$usdc" "$DLN_DEST" "deBridge DlnDest"
done

echo ""
echo "Done. Re-run with --dry-run to verify allowances."
