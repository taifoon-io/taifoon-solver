#!/usr/bin/env python3
"""
Swap USDT → ETH on Base using Uniswap V3 to fund deBridge protocol fee.

After the swap, the solver will have ~0.001 ETH on Base — enough to pay
the deBridge createOrder fixed fee and run a self-fill test.

Usage:
    python3 tools/swap_usdt_to_eth.py --dry-run     # preview without broadcast
    python3 tools/swap_usdt_to_eth.py --approve     # approve USDT to SwapRouter
    python3 tools/swap_usdt_to_eth.py               # execute swap

Environment:
    SOLVER_PRIVATE_KEY  or macOS keychain 'mamba-messiah-key'
"""

import subprocess
import os
import sys
import json
import time

BASE_RPC = "https://base-rpc.publicnode.com"
BASE_USDT = "0xfde4C96c8593536E31F229EA8f37b2ADa2699bb2"
BASE_WETH = "0x4200000000000000000000000000000000000006"
# Uniswap V3 SwapRouter02 on Base
SWAP_ROUTER = "0x2626664c2603336E57B271c5C0b26F421741e481"
# Uniswap V3 pool fee: 0.3% (fee=3000) - highest liquidity USDT/WETH pool on Base
POOL_FEE = 3000
# Swap $3 USDT
AMOUNT_IN = 3_000_000  # raw (6 decimals)
# Minimum ETH out: 0.001 ETH (2x safety on top of deBridge 0.001 ETH fee)
MIN_AMOUNT_OUT = 1_000_000_000_000_000  # 0.001 ETH in wei
# Deadline: 10 minutes from now
DEADLINE_SECS = 600


def run(cmd, **kwargs):
    return subprocess.run(cmd, capture_output=True, text=True, **kwargs)


def get_private_key() -> str:
    pk = os.environ.get("SOLVER_PRIVATE_KEY", "")
    if not pk:
        r = run(["security", "find-generic-password", "-s", "mamba-messiah-key", "-w"])
        pk = r.stdout.strip()
    if not pk:
        print("ERROR: no private key", file=sys.stderr)
        sys.exit(1)
    return pk


def get_solver_address(pk: str) -> str:
    r = run(["cast", "wallet", "address", pk])
    return r.stdout.strip()


def parse_cast_uint(s: str) -> int:
    return int(s.strip().split("\n")[0].split("[")[0].strip() or "0")


def get_eth_balance(solver: str) -> int:
    r = run(["cast", "balance", solver, "--rpc-url", BASE_RPC])
    return int(r.stdout.strip() or "0")


def get_usdt_balance(solver: str) -> int:
    r = run(["cast", "call", BASE_USDT, "balanceOf(address)(uint256)", solver,
             "--rpc-url", BASE_RPC])
    return parse_cast_uint(r.stdout)


def get_usdt_allowance(solver: str) -> int:
    r = run(["cast", "call", BASE_USDT, "allowance(address,address)(uint256)",
             solver, SWAP_ROUTER, "--rpc-url", BASE_RPC])
    return parse_cast_uint(r.stdout)


def get_quote() -> int:
    """Get ETH output quote for AMOUNT_IN USDT via QuoterV2."""
    QUOTER = "0x3d4e44Eb1374240CE5F1B871ab261CD16335B76a"
    r = run(["cast", "call", QUOTER,
             "quoteExactInputSingle((address,address,uint256,uint24,uint160))(uint256,uint160,uint32,uint256)",
             f"({BASE_USDT},{BASE_WETH},{AMOUNT_IN},{POOL_FEE},0)",
             "--rpc-url", BASE_RPC])
    if r.returncode != 0:
        print(f"Quote failed: {r.stderr[:200]}")
        return 0
    # Output: "1274407468882238 [1.274e15]\n..."
    return parse_cast_uint(r.stdout.split("\n")[0])


def main():
    dry_run = "--dry-run" in sys.argv
    do_approve = "--approve" in sys.argv

    pk = get_private_key()
    solver = get_solver_address(pk)
    print(f"Solver: {solver}")

    eth = get_eth_balance(solver)
    usdt = get_usdt_balance(solver)
    print(f"Base ETH:  {eth/1e18:.8f} ETH")
    print(f"Base USDT: ${usdt/1e6:.4f}")

    if usdt < AMOUNT_IN:
        print(f"❌ Insufficient USDT: have ${usdt/1e6:.4f}, need ${AMOUNT_IN/1e6:.2f}")
        sys.exit(1)

    print(f"\nGetting quote for ${AMOUNT_IN/1e6:.2f} USDT → ETH (0.3% pool)...")
    eth_out = get_quote()
    if eth_out == 0:
        print("❌ Quote failed")
        sys.exit(1)
    print(f"Quote: {eth_out/1e18:.6f} ETH (${eth_out/1e18 * 2400:.2f})")

    if eth_out < MIN_AMOUNT_OUT:
        print(f"❌ Quote too low: {eth_out/1e18:.6f} ETH < {MIN_AMOUNT_OUT/1e18:.6f} ETH minimum")
        sys.exit(1)

    slippage_min = int(eth_out * 0.95)  # 5% slippage tolerance
    print(f"Min ETH out (5% slippage): {slippage_min/1e18:.6f} ETH")

    if do_approve:
        allowance = get_usdt_allowance(solver)
        print(f"\nCurrent USDT allowance for SwapRouter: ${allowance/1e6:.4f}")
        if allowance >= AMOUNT_IN:
            print("✅ Already approved.")
            return
        print(f"Approving ${AMOUNT_IN/1e6:.2f} USDT to SwapRouter on Base...")
        r = run(["cast", "send", BASE_USDT,
                 "approve(address,uint256)", SWAP_ROUTER, str(AMOUNT_IN),
                 "--rpc-url", BASE_RPC, "--private-key", pk])
        if r.returncode != 0:
            print(f"❌ Approve failed: {r.stderr[:300]}")
            sys.exit(1)
        print("✅ Approved")
        return

    if dry_run:
        print(f"\n[DRY RUN] Would swap ${AMOUNT_IN/1e6:.2f} USDT → ~{eth_out/1e18:.6f} ETH on Base")
        print(f"  Router: {SWAP_ROUTER}")
        print(f"  Pool:   USDT/WETH 0.3%")
        print(f"  After:  ~{(eth/1e18 + eth_out/1e18):.6f} ETH on Base")
        print(f"\nRun with --approve first, then without --dry-run to execute.")
        return

    # Check allowance
    allowance = get_usdt_allowance(solver)
    if allowance < AMOUNT_IN:
        print(f"\n⚠️  Need approval first. Run with --approve.")
        sys.exit(1)

    deadline = int(time.time()) + DEADLINE_SECS
    print(f"\nExecuting swap on Base (deadline={deadline})...")

    # exactInputSingle((tokenIn, tokenOut, fee, recipient, amountIn, amountOutMinimum, sqrtPriceLimitX96))
    # SwapRouter02 exactInputSingle
    r = run(["cast", "send", SWAP_ROUTER,
             "exactInputSingle((address,address,uint24,address,uint256,uint256,uint160))(uint256)",
             f"({BASE_USDT},{BASE_WETH},{POOL_FEE},{solver},{AMOUNT_IN},{slippage_min},0)",
             "--rpc-url", BASE_RPC, "--private-key", pk,
             "--gas-limit", "250000"])
    print("stdout:", r.stdout)
    print("stderr:", r.stderr[:300])

    if r.returncode == 0:
        new_eth = get_eth_balance(solver)
        print(f"\n✅ Swap successful!")
        print(f"   New ETH balance on Base: {new_eth/1e18:.8f} ETH")
        print(f"   deBridge fee requires:   0.001000 ETH")
        print(f"   Ready for deBridge self-fill: {new_eth/1e18 >= 0.001}")
        print(f"\nNext: python3 tools/create_debridge_usdc_self_fill.py --src 8453 --approve")
        print(f"Then: python3 tools/create_debridge_usdc_self_fill.py --src 8453")
    else:
        print("\n❌ Swap failed")
        sys.exit(1)


if __name__ == "__main__":
    main()
