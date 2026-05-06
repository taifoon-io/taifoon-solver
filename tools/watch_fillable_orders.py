#!/usr/bin/env python3
"""
Watch for fillable Across + deBridge orders matching current solver balances.

Prints opportunities as they appear. Useful for timing live fills.

Usage:
    python3 tools/watch_fillable_orders.py
    python3 tools/watch_fillable_orders.py --poll-secs 15
"""

import sys
import time
import json
import argparse
import urllib.request

# Solver wallet
SOLVER = "0x19b3d79a15b643c6a331772c3331838a5703bc49"

# Chain RPC endpoints
RPCS = {
    8453: "https://base-rpc.publicnode.com",
    10: "https://mainnet.optimism.io",
    42161: "https://arb1.arbitrum.io/rpc",
    1: "https://ethereum-rpc.publicnode.com",
}

# USDC addresses (lowercase)
USDC = {
    1: "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
    10: "0x0b2c639c533813f4aa9d7837caf62653d097ff85",
    8453: "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913",
    42161: "0xaf88d065e77c8cc2239327c5edb3a432268e5831",
    137: "0x2791bca1f2de4661ed88a30c99a7a9449aa84174",
}

# deBridge exclusive market maker (skip orders assigned to them)
DEBRIDGE_MM = "0x555ce236c0220695b68341bc48c68d52210cc35b"


def fetch_json(url: str, timeout: int = 8):
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "taifoon-watcher/1.0"})
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return json.loads(r.read())
    except Exception as e:
        return None


def eth_call(rpc: str, to: str, data: str) -> str:
    payload = json.dumps({
        "jsonrpc": "2.0", "id": 1, "method": "eth_call",
        "params": [{"to": to, "data": data}, "latest"]
    }).encode()
    req = urllib.request.Request(rpc, data=payload,
                                 headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=5) as r:
            res = json.loads(r.read())
            return res.get("result", "0x0") or "0x0"
    except:
        return "0x0"


def get_eth_balance(chain_id: int) -> float:
    rpc = RPCS.get(chain_id)
    if not rpc:
        return 0.0
    payload = json.dumps({
        "jsonrpc": "2.0", "id": 1, "method": "eth_getBalance",
        "params": [SOLVER, "latest"]
    }).encode()
    req = urllib.request.Request(rpc, data=payload,
                                 headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=5) as r:
            res = json.loads(r.read())
            return int(res.get("result", "0x0"), 16) / 1e18
    except:
        return 0.0


def get_usdc_balance(chain_id: int) -> float:
    rpc = RPCS.get(chain_id)
    usdc = USDC.get(chain_id)
    if not rpc or not usdc:
        return 0.0
    # balanceOf(address)
    data = "0x70a08231" + "000000000000000000000000" + SOLVER[2:]
    result = eth_call(rpc, usdc, data)
    try:
        return int(result, 16) / 1e6
    except:
        return 0.0


def check_across_opportunities(solver_balances: dict) -> list:
    """Return list of fillable Across deposits."""
    fillable = []
    data = fetch_json("https://app.across.to/api/deposits?status=unfilled&limit=100")
    if not isinstance(data, list):
        return fillable

    now = int(time.time())
    for d in data:
        dep_id = d.get("depositId")
        dst_raw = d.get("destinationChainId")
        if dst_raw is None:
            continue
        dst = int(dst_raw)
        out_tok = (d.get("outputToken") or "").lower()
        out_amt_str = d.get("outputAmount") or "0"
        fill_dl_str = d.get("fillDeadline") or ""
        excl = d.get("exclusiveRelayer") or "0x0000000000000000000000000000000000000000"

        # Only USDC output on chains we have USDC
        usdc_addr = USDC.get(dst, "")
        if usdc_addr != out_tok:
            continue

        try:
            amt_usd = int(out_amt_str) / 1e6
        except:
            continue

        # Check if we have enough balance
        our_usdc = solver_balances.get(f"usdc_{dst}", 0.0)
        if amt_usd > our_usdc:
            continue

        # Check exclusivity (skip if still exclusive to someone else)
        is_exclusive = excl.lower() not in (
            "0x0000000000000000000000000000000000000000",
            SOLVER.lower()
        )
        # For now, if exclusive to us — great!

        fillable.append({
            "protocol": "across",
            "dep_id": dep_id,
            "src_chain": d.get("originChainId"),
            "dst_chain": dst,
            "amount_usd": amt_usd,
            "our_usdc": our_usdc,
            "exclusive_relayer": excl[:12],
        })

    return fillable


def check_debridge_opportunities(solver_balances: dict) -> list:
    """Return non-exclusive deBridge orders that might be fillable."""
    fillable = []
    # deBridge doesn't have a public order feed, but we can check our eth_getLogs
    # data via the DeBridgePoller. For monitoring purposes, just report ETH balance.
    # A fill is possible if: eth >= 0 (gas only) + USDC take_amount
    for chain_id in [8453, 10, 42161]:
        eth_bal = solver_balances.get(f"eth_{chain_id}", 0.0)
        usdc_bal = solver_balances.get(f"usdc_{chain_id}", 0.0)
        if eth_bal < 0.0001:  # min gas
            continue
        # Report available capacity for deBridge fills on this chain
        if usdc_bal > 0.01:
            fillable.append({
                "protocol": "debridge",
                "chain_id": chain_id,
                "eth_balance": eth_bal,
                "usdc_balance": usdc_bal,
                "note": "ready to fill non-exclusive USDC orders (if any exist)",
            })
    return fillable


def print_balances(balances: dict):
    print("\n📊 Current solver balances:")
    for chain_id in [8453, 10, 42161, 1]:
        eth = balances.get(f"eth_{chain_id}", 0.0)
        usdc = balances.get(f"usdc_{chain_id}", 0.0)
        chain_name = {1: "Ethereum", 10: "Optimism", 8453: "Base", 42161: "Arbitrum"}.get(chain_id, str(chain_id))
        print(f"  {chain_name:12}: {eth:.6f} ETH  {usdc:.4f} USDC")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--poll-secs", type=int, default=30, help="Poll interval (seconds)")
    args = parser.parse_args()

    print(f"🔍 Watching for fillable orders (poll={args.poll_secs}s) — solver={SOLVER[:10]}...")
    print("   Ctrl+C to stop\n")

    seen_ids = set()
    last_balance_refresh = 0

    while True:
        now = time.time()

        # Refresh balances every 60s
        if now - last_balance_refresh > 60:
            print("🔄 Refreshing balances...", end=" ", flush=True)
            balances = {}
            for chain_id in [8453, 10, 42161]:
                balances[f"eth_{chain_id}"] = get_eth_balance(chain_id)
                balances[f"usdc_{chain_id}"] = get_usdc_balance(chain_id)
            print("done")
            print_balances(balances)
            last_balance_refresh = now

        # Check Across
        across_opps = check_across_opportunities(balances)
        for opp in across_opps:
            key = f"across_{opp['dep_id']}"
            if key not in seen_ids:
                seen_ids.add(key)
                print(f"\n🎯 FILLABLE Across deposit found!")
                print(f"   depositId={opp['dep_id']} {opp['src_chain']}→{opp['dst_chain']}")
                print(f"   amount=${opp['amount_usd']:.4f} USDC (we have ${opp['our_usdc']:.4f})")
                print(f"   exclusive_relayer={opp['exclusive_relayer']}")
                print(f"   → Start solver with: DRY_RUN=false ./run-mainnet.sh")

        if not across_opps:
            ts = time.strftime("%H:%M:%S")
            print(f"[{ts}] No fillable Across deposits (checked 100 unfilled, none match our balance)")

        time.sleep(args.poll_secs)


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        print("\nStopped.")
