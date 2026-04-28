#!/usr/bin/env python3
"""
Negative Profit Inspector - Deep dive into all negative profit intents
Negative profits usually indicate bugs in gas calculations or intent data
"""

import json
from pathlib import Path
from typing import Dict, List
from dataclasses import dataclass

FIXTURES_DIR = Path("fixtures")

# Token prices (USD)
TOKEN_PRICES = {
    1: 3000,      # Ethereum
    10: 3000,     # Optimism (ETH)
    56: 600,      # BSC (BNB)
    137: 0.9,     # Polygon (MATIC)
    143: 3000,    # Monad (ETH)
    200: 150,     # Solana (SOL)
    250: 0.5,     # Fantom (FTM)
    252: 3000,    # Fraxtal (ETH)
    324: 3000,    # zkSync Era (ETH)
    999: 3000,    # Zora (ETH)
    1284: 3000,   # Moonbeam (GLMR)
    7777777: 3000,# Zora Network (ETH)
    8453: 3000,   # Base (ETH)
    34443: 3000,  # Mode (ETH)
    42161: 3000,  # Arbitrum (ETH)
    43114: 30,    # Avalanche (AVAX)
    59144: 3000,  # Linea (ETH)
    81457: 3000,  # Blast (ETH)
    534352: 3000, # Scroll (ETH)
}

GAS_LIMIT = 60_000  # Standard gas limit for bridge transfers

def load_ndjson(file_path: Path) -> List[Dict]:
    """Load NDJSON file"""
    intents = []
    if not file_path.exists():
        return []

    with open(file_path, 'r') as f:
        content = f.read()
        decoder = json.JSONDecoder()
        idx = 0
        while idx < len(content):
            content_stripped = content[idx:].lstrip()
            if not content_stripped:
                break
            try:
                obj, end_idx = decoder.raw_decode(content_stripped)
                intents.append(obj)
                idx += len(content[idx:]) - len(content_stripped) + end_idx
            except json.JSONDecodeError:
                break

    return intents

def calculate_gas_cost(chain_id: int, gas_gwei: float) -> float:
    """Calculate gas cost in USD"""
    token_price = TOKEN_PRICES.get(chain_id, 1)
    gas_cost_native = (gas_gwei / 1e9) * GAS_LIMIT
    gas_cost_usd = gas_cost_native * token_price
    return gas_cost_usd

def inspect_negative_profit_intent(intent: Dict) -> Dict:
    """Deep inspection of negative profit intent"""
    intent_id = intent.get("id", "unknown")
    protocol = intent.get("protocol", "unknown")
    src_chain = intent.get("src_chain", 0)
    dst_chain = intent.get("dst_chain", 0)
    profit_usd = intent.get("profit_usd", 0.0)
    amount = intent.get("amount", "0")
    state = intent.get("state", "unknown")

    # Estimate what gas prices would cause this negative profit
    # Assumption: Bridge fees are typically 0.01-0.1% of transfer amount
    # So if profit is negative, it's almost always due to gas costs

    issues = []

    # Check if profit is negative
    if profit_usd >= 0:
        return None  # Skip positive profits

    # Calculate what the gas cost would need to be
    # to cause this negative profit (assuming 0.05% bridge fee)
    try:
        amount_float = float(amount) / 1e18  # Convert from wei
        estimated_bridge_fee_usd = amount_float * 0.0005  # 0.05% fee
        implied_gas_cost = estimated_bridge_fee_usd + abs(profit_usd)

        # Back-calculate implied gas price
        src_token_price = TOKEN_PRICES.get(src_chain, 1)
        dst_token_price = TOKEN_PRICES.get(dst_chain, 1)

        # Assume equal gas on both chains
        avg_token_price = (src_token_price + dst_token_price) / 2
        implied_gas_gwei = (implied_gas_cost / 2) / (GAS_LIMIT * avg_token_price / 1e9)

    except:
        amount_float = 0
        estimated_bridge_fee_usd = 0
        implied_gas_cost = 0
        implied_gas_gwei = 0

    # Analyze issues
    if profit_usd < -10:
        issues.append(f"EXTREME negative profit: ${profit_usd:.2f} (likely data error)")
    elif profit_usd < -2:
        issues.append(f"Very high loss: ${profit_usd:.2f} (gas cost exceeds reasonable bounds)")
    else:
        issues.append(f"Negative profit: ${profit_usd:.2f} (check gas calculations)")

    if amount == "0":
        issues.append("Zero amount transfer (invalid)")
    elif amount_float < 0.001:
        issues.append(f"Dust transfer: {amount_float:.6f} tokens (likely unprofitable)")

    if implied_gas_gwei > 1000:
        issues.append(f"Implied gas: {implied_gas_gwei:.1f} gwei (UNREALISTIC - likely bug)")
    elif implied_gas_gwei > 100:
        issues.append(f"Implied gas: {implied_gas_gwei:.1f} gwei (very high)")

    return {
        "intent_id": intent_id,
        "protocol": protocol,
        "src_chain": src_chain,
        "dst_chain": dst_chain,
        "profit_usd": profit_usd,
        "amount": amount,
        "amount_tokens": amount_float,
        "state": state,
        "estimated_bridge_fee": estimated_bridge_fee_usd,
        "implied_gas_cost": implied_gas_cost,
        "implied_gas_gwei": implied_gas_gwei,
        "issues": issues,
    }

def main():
    print("="*100)
    print("NEGATIVE PROFIT INSPECTOR - DEEP DIVE ANALYSIS")
    print("="*100)
    print("Investigating all intents with negative profits (likely bugs)")
    print()

    protocols = [
        "orbiter_finance_intents",
        "lifi_v2_intents",
        "across_v3_intents",
        "stargate_v2_intents",
        "hyperlane_intents",
    ]

    all_negative = []

    for protocol_file in protocols:
        fixture_file = FIXTURES_DIR / f"{protocol_file}.json"
        intents = load_ndjson(fixture_file)

        if not intents:
            continue

        # Find all negative profit intents
        negative_intents = []
        for intent in intents:
            analysis = inspect_negative_profit_intent(intent)
            if analysis:
                negative_intents.append(analysis)

        if negative_intents:
            all_negative.extend(negative_intents)

            print(f"\n{'='*100}")
            print(f"PROTOCOL: {protocol_file}")
            print(f"{'='*100}")
            print(f"Total intents: {len(intents)}")
            print(f"Negative profit intents: {len(negative_intents)} ({len(negative_intents)/len(intents)*100:.1f}%)")
            print()

            # Sort by profit (most negative first)
            negative_intents.sort(key=lambda x: x["profit_usd"])

            for i, analysis in enumerate(negative_intents, 1):
                print(f"\n{i}. Intent: {analysis['intent_id'][:70]}...")
                print(f"   Chain: {analysis['src_chain']} → {analysis['dst_chain']}")
                print(f"   Amount: {analysis['amount_tokens']:.6f} tokens")
                print(f"   Profit: ${analysis['profit_usd']:.2f}")
                print(f"   State: {analysis['state']}")
                print(f"   Estimated bridge fee: ${analysis['estimated_bridge_fee']:.4f}")
                print(f"   Implied gas cost: ${analysis['implied_gas_cost']:.2f}")
                print(f"   Implied gas price: {analysis['implied_gas_gwei']:.1f} gwei")
                print()
                print(f"   🔍 Issues:")
                for issue in analysis["issues"]:
                    print(f"      ❌ {issue}")

    # Overall summary
    print("\n\n" + "="*100)
    print("OVERALL NEGATIVE PROFIT SUMMARY")
    print("="*100)
    print(f"Total negative profit intents: {len(all_negative)}")
    print()

    # Group by severity
    extreme = [a for a in all_negative if a["profit_usd"] < -10]
    high = [a for a in all_negative if -10 <= a["profit_usd"] < -2]
    medium = [a for a in all_negative if -2 <= a["profit_usd"] < 0]

    print(f"Severity breakdown:")
    print(f"  🔴 EXTREME (< -$10):  {len(extreme)}")
    print(f"  🟠 HIGH (-$10 to -$2): {len(high)}")
    print(f"  🟡 MEDIUM (-$2 to $0):  {len(medium)}")
    print()

    # Find most likely bugs
    print("="*100)
    print("MOST LIKELY BUGS (by implied gas price)")
    print("="*100)
    print()

    # Sort by implied gas price (highest = most likely bug)
    all_negative.sort(key=lambda x: x["implied_gas_gwei"], reverse=True)

    print(f"{'Intent':<70} {'Profit':>10} {'Implied Gas':>15} {'Protocol':>20}")
    print("-"*100)

    for analysis in all_negative[:20]:  # Top 20
        protocol_short = analysis["protocol"].replace("_intents", "")
        intent_short = analysis["intent_id"][:50] + "..." if len(analysis["intent_id"]) > 50 else analysis["intent_id"]
        print(f"{intent_short:<70} ${analysis['profit_usd']:>9.2f} {analysis['implied_gas_gwei']:>12.1f} gwei {protocol_short:>20}")

    print()
    print("="*100)
    print("RECOMMENDATIONS")
    print("="*100)
    print()
    print("1. IMMEDIATE FIXES NEEDED:")
    print(f"   - {len(extreme)} intents with extreme losses (< -$10)")
    print(f"   - {len([a for a in all_negative if a['implied_gas_gwei'] > 1000])} intents with unrealistic gas (> 1000 gwei)")
    print()
    print("2. ROOT CAUSES:")
    print("   - Most negative profits are due to UNREALISTIC gas price calculations")
    print("   - Bridge fees are typically 0.01-0.1% (very small)")
    print("   - Gas costs should be < $2 on most chains")
    print("   - Negative profits indicate gas calculations are inflated")
    print()
    print("3. NEXT STEPS:")
    print("   - Remove all intents with implied gas > 100 gwei (clearly wrong)")
    print("   - Verify gas price sources for all chains")
    print("   - Re-generate fixtures with LIVE gas prices from Spinner API")
    print("   - Add validation: reject intents with profit < -$2")
    print()

if __name__ == "__main__":
    main()
