#!/usr/bin/env python3
"""
Automated Fixture Validator
Validates all protocol fixtures against the Spinner API with real gas prices.
Catches: >$1 profits, None values, 0 gas values, unrealistic costs.
"""

import json
import requests
from pathlib import Path
from typing import Dict, List, Optional
from dataclasses import dataclass

SPINNER_GAS_API = "http://46.4.96.124:30081/api/gas/latest"
FIXTURES_DIR = Path("fixtures")

@dataclass
class ValidationResult:
    intent_id: str
    protocol: str
    src_chain: int
    dst_chain: int
    raw_profit: Optional[float]
    gas_cost_usd: Optional[float]
    src_gas_gwei: Optional[float]
    dst_gas_gwei: Optional[float]
    issues: List[str]
    status: str  # "PASS", "FAIL", "WARNING"

def fetch_gas_price(chain_id: int) -> Optional[Dict]:
    """Fetch gas price for a chain from Spinner API"""
    try:
        url = f"{SPINNER_GAS_API}/{chain_id}"
        response = requests.get(url, timeout=5)
        if response.status_code == 200:
            return response.json()
        else:
            print(f"    ⚠️  Chain {chain_id}: HTTP {response.status_code}")
            return None
    except Exception as e:
        print(f"    ⚠️  Chain {chain_id}: {str(e)[:50]}")
        return None

def validate_intent(intent: Dict, gas_cache: Dict[int, Optional[Dict]]) -> ValidationResult:
    """Validate a single intent"""
    intent_id = intent.get("id", "unknown")
    protocol = intent.get("protocol", "unknown")
    src_chain = intent.get("src_chain", 0)
    dst_chain = intent.get("dst_chain", 0)
    raw_profit = intent.get("profit_usd")

    issues = []
    status = "PASS"

    # Fetch gas prices (with caching)
    if src_chain not in gas_cache:
        gas_cache[src_chain] = fetch_gas_price(src_chain)
    if dst_chain not in gas_cache:
        gas_cache[dst_chain] = fetch_gas_price(dst_chain)

    src_gas = gas_cache.get(src_chain)
    dst_gas = gas_cache.get(dst_chain)

    src_gas_gwei = src_gas.get("gas_price_gwei") if src_gas else None
    dst_gas_gwei = dst_gas.get("gas_price_gwei") if dst_gas else None

    # Check for missing gas prices
    if src_gas is None:
        issues.append(f"Missing gas price for src chain {src_chain}")
        status = "WARNING"
    if dst_gas is None:
        issues.append(f"Missing gas price for dst chain {dst_chain}")
        status = "WARNING"

    # Check for zero gas prices
    if src_gas_gwei is not None and src_gas_gwei == 0:
        issues.append(f"Zero gas price for src chain {src_chain}")
        status = "FAIL"
    if dst_gas_gwei is not None and dst_gas_gwei == 0:
        issues.append(f"Zero gas price for dst chain {dst_chain}")
        status = "FAIL"

    # Calculate total gas cost (very rough estimate: 60k gas per tx)
    GAS_LIMIT = 60_000
    src_gas_cost = None
    dst_gas_cost = None

    if src_gas_gwei is not None:
        # Convert gwei to USD (assuming native token price, this is simplified)
        # For Ethereum: ~$3000/ETH, for Polygon: ~$0.9/POL, etc.
        token_prices = {
            1: 3000,    # ETH
            10: 3000,   # ETH (OP)
            8453: 3000, # ETH (Base)
            42161: 3000,# ETH (Arbitrum)
            137: 0.9,   # POL
            56: 600,    # BNB
        }
        token_price = token_prices.get(src_chain, 1)  # Default to $1
        src_gas_cost = (src_gas_gwei / 1e9) * GAS_LIMIT * token_price

    if dst_gas_gwei is not None:
        token_price = token_prices.get(dst_chain, 1)
        dst_gas_cost = (dst_gas_gwei / 1e9) * GAS_LIMIT * token_price

    total_gas_cost = None
    if src_gas_cost is not None and dst_gas_cost is not None:
        total_gas_cost = src_gas_cost + dst_gas_cost

        # Check if gas cost > $1 (likely unprofitable)
        if total_gas_cost > 1.0:
            issues.append(f"Gas cost alone is ${total_gas_cost:.4f} (likely unprofitable)")
            status = "FAIL"

    # Check raw profit
    if raw_profit is not None:
        if raw_profit > 1.0:
            issues.append(f"Unrealistic profit: ${raw_profit:.2f} (should be < $1)")
            status = "FAIL"
        if raw_profit < -2.0:
            issues.append(f"Unrealistic loss: ${raw_profit:.2f} (should be > -$2)")
            status = "FAIL"

    return ValidationResult(
        intent_id=intent_id,
        protocol=protocol,
        src_chain=src_chain,
        dst_chain=dst_chain,
        raw_profit=raw_profit,
        gas_cost_usd=total_gas_cost,
        src_gas_gwei=src_gas_gwei,
        dst_gas_gwei=dst_gas_gwei,
        issues=issues,
        status=status
    )

def main():
    print("=" * 70)
    print("TAIFOON FIXTURE VALIDATOR")
    print("=" * 70)
    print()

    protocols = [
        "across_v3_intents",
        "allbridge_intents",
        "hyperlane_intents",
        "layerzero_v2_intents",
        "lifi_v2_intents",
        "orbiter_finance_intents",
        "squid_router_intents",
        "stargate_v2_intents",
        "t3rn_lwc_intents",
    ]

    total = 0
    passed = 0
    failed = 0
    warnings = 0

    results: List[ValidationResult] = []
    gas_cache: Dict[int, Optional[Dict]] = {}

    for protocol in protocols:
        fixture_file = FIXTURES_DIR / f"{protocol}.json"

        if not fixture_file.exists():
            print(f"⚠️  Skipping {protocol} (file not found)")
            continue

        print(f"🔍 Testing {protocol} fixtures...")

        # Parse NDJSON (newline-delimited JSON)
        intents = []
        with open(fixture_file) as f:
            for line_num, line in enumerate(f, 1):
                line = line.strip()
                if line:
                    try:
                        intents.append(json.loads(line))
                    except json.JSONDecodeError as e:
                        print(f"    ⚠️  Skipping malformed JSON at line {line_num}: {str(e)[:50]}")

        for intent in intents:
            result = validate_intent(intent, gas_cache)
            results.append(result)
            total += 1

            if result.status == "PASS":
                passed += 1
            elif result.status == "FAIL":
                failed += 1
            else:
                warnings += 1

        print(f"   ✓ Validated {len(intents)} intents")

    # Print summary
    pass_rate = (passed / total * 100) if total > 0 else 0

    print()
    print("=" * 70)
    print("TEST SUMMARY")
    print("=" * 70)
    print(f"Total Intents:  {total}")
    print(f"✅ PASS:        {passed} ({pass_rate:.1f}%)")
    print(f"❌ FAIL:        {failed}")
    print(f"⚠️  WARNING:    {warnings}")
    print("=" * 70)
    print()

    # Print failures
    failures = [r for r in results if r.status == "FAIL"]
    if failures:
        print("=" * 70)
        print(f"FAILURES ({len(failures)})")
        print("=" * 70)

        for r in failures:
            print(f"\n❌ {r.intent_id}")
            print(f"   Protocol: {r.protocol}")
            print(f"   Route: {r.src_chain} → {r.dst_chain}")
            if r.raw_profit is not None:
                print(f"   Raw Profit: ${r.raw_profit:.6f}")
            if r.gas_cost_usd is not None:
                print(f"   Gas Cost: ${r.gas_cost_usd:.6f}")
            if r.src_gas_gwei is not None:
                print(f"   Src Gas: {r.src_gas_gwei} gwei")
            if r.dst_gas_gwei is not None:
                print(f"   Dst Gas: {r.dst_gas_gwei} gwei")
            print("   Issues:")
            for issue in r.issues:
                print(f"     • {issue}")
        print()

    # Print warnings (first 10)
    warn_results = [r for r in results if r.status == "WARNING"][:10]
    if warn_results:
        print("=" * 70)
        print(f"WARNINGS ({len([r for r in results if r.status == 'WARNING'])}) - showing first 10")
        print("=" * 70)

        for r in warn_results:
            print(f"\n⚠️  {r.intent_id}")
            print(f"   Protocol: {r.protocol}")
            print(f"   Route: {r.src_chain} → {r.dst_chain}")
            if r.raw_profit is not None:
                print(f"   Raw Profit: ${r.raw_profit:.6f}")
            print("   Issues:")
            for issue in r.issues:
                print(f"     • {issue}")
        print()

    # Protocol breakdown
    protocol_stats = {}
    for r in results:
        if r.protocol not in protocol_stats:
            protocol_stats[r.protocol] = {"pass": 0, "fail": 0, "warn": 0}

        if r.status == "PASS":
            protocol_stats[r.protocol]["pass"] += 1
        elif r.status == "FAIL":
            protocol_stats[r.protocol]["fail"] += 1
        else:
            protocol_stats[r.protocol]["warn"] += 1

    print("=" * 70)
    print("PROTOCOL BREAKDOWN")
    print("=" * 70)
    print(f"{'Protocol':<20} {'Pass':>8} {'Fail':>8} {'Warn':>8} {'Pass Rate':>10}")
    print("-" * 70)

    for protocol in sorted(protocol_stats.keys()):
        stats = protocol_stats[protocol]
        total_protocol = stats["pass"] + stats["fail"] + stats["warn"]
        pass_rate_protocol = (stats["pass"] / total_protocol * 100) if total_protocol > 0 else 0
        print(f"{protocol:<20} {stats['pass']:>8} {stats['fail']:>8} {stats['warn']:>8} {pass_rate_protocol:>9.1f}%")

    print()

    # Exit with failure if any failures found
    if failed > 0:
        print("❌ TEST FAILED: {} failing intents (gas pricing bugs still present)".format(failed))
        return 1
    else:
        print("=" * 70)
        print(f"✅ ALL TESTS PASSED (Pass rate: {pass_rate:.1f}%)")
        print("=" * 70)
        print()
        return 0

if __name__ == "__main__":
    exit(main())
