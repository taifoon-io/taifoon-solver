#!/usr/bin/env python3
"""
Create a tiny Across V3 native-ETH self-fill: deposit ETH on Optimism, fill on Base.

We have:
  - Optimism: ~0.000109 ETH (source, lock 0.00005 ETH)
  - Base:     ~0.000232 ETH (destination fill side)

Calls SpokePool.depositV3 with:
  - inputToken / outputToken = WETH sentinel (0x0000...0000 = native ETH)
  - inputAmount = 0.00005 ETH
  - outputAmount = 0.000045 ETH (90% of input — solver earns the spread)
  - destinationChainId = 8453 (Base)
  - quoteTimestamp = now
  - fillDeadline = now + 600s
  - depositor = solver address
  - recipient = solver address (receive the fill on Base)

After deposit, AcrossPoller will pick up the V3FundsDeposited event on Base SpokePool
within ~20s and the solver fills it automatically (if balance check passes).

Usage:
    python3 tools/create_across_eth_self_fill.py --dry-run   # show tx data, no broadcast
    python3 tools/create_across_eth_self_fill.py             # broadcast deposit
"""

import os
import sys
import time
import subprocess

# Across V3 SpokePool addresses
SPOKE_OPT   = "0x6f26Bf09B1C792e3228e5467807a900A503c0281"   # Optimism
SPOKE_BASE  = "0x09aea4b2242abC8bb4BB78D537A67a245A7bEC64"   # Base
RPC_OPT     = "https://mainnet.optimism.io"
RPC_BASE    = "https://mainnet.base.org"
CHAIN_OPT   = 10
CHAIN_BASE  = 8453

NATIVE_ETH  = "0x0000000000000000000000000000000000000000"
INPUT_WEI   = 10_000_000_000_000      # 0.00001 ETH (fits our 0.000029 ETH Optimism balance)
OUTPUT_WEI  = 9_000_000_000_000       # 0.000009 ETH (solver earns 10% spread)
FILL_DEADLINE_OFFSET = 600            # 10 minutes

# depositV3 ABI selector + encoding
# depositV3(address depositor, address recipient, address inputToken, address outputToken,
#            uint256 inputAmount, uint256 outputAmount, uint256 destinationChainId,
#            address exclusiveRelayer, uint32 quoteTimestamp, uint32 fillDeadline,
#            uint32 exclusivityDeadline, bytes calldata message)
DEPOSIT_V3_SEL = "0x7b939232"


def load_key():
    key = os.environ.get("SOLVER_PRIVATE_KEY", "")
    if not key:
        result = subprocess.run(
            ["security", "find-generic-password", "-s", "mamba-messiah-key", "-w"],
            capture_output=True, text=True
        )
        key = result.stdout.strip()
    if not key:
        sys.exit("ERROR: SOLVER_PRIVATE_KEY not set and keychain entry not found")
    return key


def cast_call(cmd_args, rpc):
    result = subprocess.run(
        ["cast"] + cmd_args + ["--rpc-url", rpc],
        capture_output=True, text=True
    )
    return result.stdout.strip()


def solver_address(key):
    r = subprocess.run(["cast", "wallet", "address", key], capture_output=True, text=True)
    return r.stdout.strip()


def get_eth_balance(addr, rpc):
    r = subprocess.run(["cast", "balance", addr, "--rpc-url", rpc], capture_output=True, text=True)
    try:
        return int(r.stdout.strip())
    except Exception:
        return 0


def get_block_timestamp(rpc):
    r = subprocess.run(
        ["cast", "block", "latest", "--field", "timestamp", "--rpc-url", rpc],
        capture_output=True, text=True
    )
    try:
        return int(r.stdout.strip(), 16) if r.stdout.strip().startswith("0x") else int(r.stdout.strip())
    except Exception:
        return int(time.time())


def build_calldata(solver_addr, quote_ts, fill_deadline):
    # depositV3(depositor, recipient, inputToken, outputToken,
    #           inputAmount, outputAmount, destinationChainId,
    #           exclusiveRelayer, quoteTimestamp, fillDeadline,
    #           exclusivityDeadline, message)
    pad = lambda x, bits=256: hex(x)[2:].zfill(bits // 4)
    addr = lambda a: a.lower().replace("0x", "").zfill(64)
    calldata = (
        DEPOSIT_V3_SEL[2:]
        + addr(solver_addr)           # depositor
        + addr(solver_addr)           # recipient (receive fill on Base)
        + addr(NATIVE_ETH)            # inputToken (native ETH)
        + addr(NATIVE_ETH)            # outputToken (native ETH on Base)
        + pad(INPUT_WEI)              # inputAmount
        + pad(OUTPUT_WEI)             # outputAmount
        + pad(CHAIN_BASE)             # destinationChainId
        + addr(NATIVE_ETH)            # exclusiveRelayer (none)
        + pad(quote_ts, 32)           # quoteTimestamp (uint32)
        + pad(fill_deadline, 32)      # fillDeadline (uint32)
        + pad(0, 32)                  # exclusivityDeadline (uint32)
        + pad(0x180, 256)             # message offset (pointing to empty bytes)
        + pad(0)                      # message length = 0
    )
    return "0x" + calldata


def main():
    dry_run = "--dry-run" in sys.argv
    key = load_key()
    solver = solver_address(key)
    print(f"Solver: {solver}")

    opt_bal = get_eth_balance(solver, RPC_OPT)
    base_bal = get_eth_balance(solver, RPC_BASE)
    print(f"Optimism ETH: {opt_bal} wei ({opt_bal/1e18:.6f} ETH)")
    print(f"Base ETH:     {base_bal} wei ({base_bal/1e18:.6f} ETH)")

    if opt_bal < INPUT_WEI + 5_000_000_000_000:
        sys.exit(f"ERROR: Insufficient ETH on Optimism. Have {opt_bal} wei, need {INPUT_WEI + 5_000_000_000_000} wei (deposit + gas)")
    if base_bal < OUTPUT_WEI + 5_000_000_000_000:
        sys.exit(f"ERROR: Insufficient ETH on Base to fill. Have {base_bal} wei, need {OUTPUT_WEI + 50_000_000_000_000} wei (output + gas)")

    quote_ts = get_block_timestamp(RPC_OPT)
    fill_deadline = quote_ts + FILL_DEADLINE_OFFSET
    calldata = build_calldata(solver, quote_ts, fill_deadline)

    print(f"\nDeposit params:")
    print(f"  SpokePool:       {SPOKE_OPT} (Optimism)")
    print(f"  inputToken:      ETH (native)")
    print(f"  outputToken:     ETH (native) on Base")
    print(f"  inputAmount:     {INPUT_WEI} wei ({INPUT_WEI/1e18:.5f} ETH)")
    print(f"  outputAmount:    {OUTPUT_WEI} wei ({OUTPUT_WEI/1e18:.5f} ETH)")
    print(f"  destinationChain: {CHAIN_BASE} (Base)")
    print(f"  quoteTimestamp:  {quote_ts}")
    print(f"  fillDeadline:    {fill_deadline} (+{FILL_DEADLINE_OFFSET}s)")
    print(f"  calldata:        {calldata[:50]}...")

    if dry_run:
        print("\n[DRY RUN] Would send the above deposit tx on Optimism.")
        print("Run without --dry-run to broadcast.")
        return

    print("\nBroadcasting depositV3 on Optimism...")
    result = subprocess.run([
        "cast", "send",
        SPOKE_OPT,
        calldata,
        "--value", str(INPUT_WEI),
        "--private-key", key,
        "--rpc-url", RPC_OPT,
        "--json"
    ], capture_output=True, text=True)

    if result.returncode != 0:
        print(f"ERROR: {result.stderr}")
        sys.exit(1)

    import json
    out = json.loads(result.stdout)
    tx_hash = out.get("transactionHash", "?")
    print(f"\n✅ Deposit tx: {tx_hash}")
    print(f"   Explorer: https://optimistic.etherscan.io/tx/{tx_hash}")
    print(f"\nNow run the solver — AcrossPoller will pick this up in ~20s:")
    print(f"  YES_I_AM_SURE=1 DRY_RUN=false MAX_NOTIONAL_USD=1 MIN_PROFIT_USD=0.001 PROTOCOL_FILTER=across ./run-mainnet.sh")


if __name__ == "__main__":
    main()
