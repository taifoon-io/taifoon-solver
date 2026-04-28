#!/usr/bin/env python3
"""
Failure Analysis Script
Analyzes all failing intents to identify data quality issues.
"""

import json
from pathlib import Path
from typing import Dict, List
from dataclasses import dataclass

FIXTURES_DIR = Path("fixtures")

@dataclass
class FailureAnalysis:
    intent_id: str
    protocol: str
    src_chain: int
    dst_chain: int
    profit_usd: float
    amount: str
    state: str
    issues: List[str]
    severity: str  # "CRITICAL", "HIGH", "MEDIUM", "LOW"

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
            except json.JSONDecodeError as e:
                break

    return intents

def analyze_intent(intent: Dict) -> FailureAnalysis:
    """Analyze a single intent for data quality issues"""
    intent_id = intent.get("id", "unknown")
    protocol = intent.get("protocol", "unknown")
    src_chain = intent.get("src_chain", 0)
    dst_chain = intent.get("dst_chain", 0)
    profit_usd = intent.get("profit_usd", 0.0)
    amount = intent.get("amount", "0")
    state = intent.get("state", "unknown")

    issues = []
    severity = "LOW"

    # Check for unrealistic profit values
    if profit_usd > 1000:
        issues.append(f"EXTREME profit: ${profit_usd:,.2f} (likely test data error)")
        severity = "CRITICAL"
    elif profit_usd > 100:
        issues.append(f"Very high profit: ${profit_usd:,.2f} (likely unrealistic)")
        severity = "HIGH"
    elif profit_usd > 10:
        issues.append(f"High profit: ${profit_usd:,.2f} (unusual for cross-chain)")
        severity = "MEDIUM"

    # Check for unrealistic losses
    if profit_usd < -10:
        issues.append(f"Large loss: ${profit_usd:.2f} (likely unprofitable)")
        severity = "HIGH"

    # Check for zero or very small amounts
    try:
        amount_int = int(amount)
        if amount_int == 0:
            issues.append("Zero amount (invalid intent)")
            severity = "HIGH"
        elif amount_int < 1000:
            issues.append(f"Very small amount: {amount_int} (dust transfer)")
            severity = "MEDIUM"
    except ValueError:
        issues.append(f"Invalid amount format: {amount}")
        severity = "HIGH"

    # Check state
    if state == "attempted" and profit_usd > 100:
        issues.append("Attempted with unrealistic profit (should be skipped)")
        severity = "CRITICAL"

    return FailureAnalysis(
        intent_id=intent_id,
        protocol=protocol,
        src_chain=src_chain,
        dst_chain=dst_chain,
        profit_usd=profit_usd,
        amount=amount,
        state=state,
        issues=issues,
        severity=severity
    )

def main():
    print("="*100)
    print("FAILURE ANALYSIS - DATA QUALITY INVESTIGATION")
    print("="*100)
    print()

    protocols_to_analyze = {
        "orbiter_finance_intents": "Orbiter Finance (highest failure count)",
        "lifi_v2_intents": "LiFi V2",
        "across_v3_intents": "Across V3",
        "stargate_v2_intents": "Stargate V2",
    }

    all_analyses = {}

    for protocol_file, description in protocols_to_analyze.items():
        print(f"\n{'='*100}")
        print(f"ANALYZING: {description}")
        print(f"{'='*100}\n")

        fixture_file = FIXTURES_DIR / f"{protocol_file}.json"
        intents = load_ndjson(fixture_file)

        if not intents:
            print(f"  ⚠️  No intents found in {protocol_file}.json")
            continue

        # Analyze all intents
        analyses = [analyze_intent(intent) for intent in intents]

        # Filter to only problematic ones
        critical = [a for a in analyses if a.severity == "CRITICAL"]
        high = [a for a in analyses if a.severity == "HIGH"]
        medium = [a for a in analyses if a.severity == "MEDIUM"]

        all_analyses[protocol_file] = {
            "critical": critical,
            "high": high,
            "medium": medium,
            "total": len(intents)
        }

        print(f"Total intents: {len(intents)}")
        print(f"  🔴 CRITICAL: {len(critical)}")
        print(f"  🟠 HIGH:     {len(high)}")
        print(f"  🟡 MEDIUM:   {len(medium)}")
        print()

        # Print critical issues
        if critical:
            print("🔴 CRITICAL ISSUES:")
            print("-" * 100)
            for a in critical:
                print(f"\nIntent: {a.intent_id[:60]}...")
                print(f"  Chain: {a.src_chain} → {a.dst_chain}")
                print(f"  Profit: ${a.profit_usd:,.2f}")
                print(f"  Amount: {a.amount}")
                print(f"  State: {a.state}")
                for issue in a.issues:
                    print(f"  ❌ {issue}")

        # Print high priority issues
        if high:
            print("\n🟠 HIGH PRIORITY ISSUES:")
            print("-" * 100)
            for a in high[:5]:  # Top 5
                print(f"\nIntent: {a.intent_id[:60]}...")
                print(f"  Profit: ${a.profit_usd:.2f}, Amount: {a.amount}, Issues: {len(a.issues)}")
                for issue in a.issues:
                    print(f"  ⚠️  {issue}")

        # Print summary statistics
        if analyses:
            print("\n📊 PROFIT DISTRIBUTION:")
            print("-" * 100)
            profits = [a.profit_usd for a in analyses]
            print(f"  Min:    ${min(profits):,.2f}")
            print(f"  Max:    ${max(profits):,.2f}")
            print(f"  Avg:    ${sum(profits)/len(profits):,.2f}")
            print(f"  Median: ${sorted(profits)[len(profits)//2]:,.2f}")

            # Count by state
            states = {}
            for a in analyses:
                states[a.state] = states.get(a.state, 0) + 1

            print("\n📊 STATE DISTRIBUTION:")
            print("-" * 100)
            for state, count in sorted(states.items(), key=lambda x: -x[1]):
                print(f"  {state:15} {count:>5} ({count/len(analyses)*100:.1f}%)")

    # Print overall summary
    print("\n\n" + "="*100)
    print("OVERALL SUMMARY")
    print("="*100)

    total_critical = sum(len(a["critical"]) for a in all_analyses.values())
    total_high = sum(len(a["high"]) for a in all_analyses.values())
    total_medium = sum(len(a["medium"]) for a in all_analyses.values())
    total_intents = sum(a["total"] for a in all_analyses.values())

    print(f"\nTotal intents analyzed: {total_intents}")
    print(f"  🔴 CRITICAL issues: {total_critical}")
    print(f"  🟠 HIGH issues:     {total_high}")
    print(f"  🟡 MEDIUM issues:   {total_medium}")
    print()

    # Top issues by protocol
    print("TOP ISSUES BY PROTOCOL:")
    print("-" * 100)
    for protocol_file, data in all_analyses.items():
        critical_count = len(data["critical"])
        high_count = len(data["high"])
        if critical_count > 0 or high_count > 0:
            print(f"\n{protocol_file}:")
            print(f"  🔴 {critical_count} critical, 🟠 {high_count} high priority")

    print("\n\n" + "="*100)
    print("RECOMMENDATIONS")
    print("="*100)
    print()
    print("1. ORBITER FINANCE (CRITICAL):")
    print("   - Line 17: $74,572 profit intent (24857924377866246920000 amount)")
    print("   - Line 3: $262 profit intent (87985854817277730000 amount)")
    print("   - Line 10: $4.93 profit intent")
    print("   → ACTION: These are test data errors with unrealistic amounts/profits")
    print("   → FIX: Regenerate fixtures with realistic bridge transfer amounts")
    print()
    print("2. STARGATE V2:")
    print("   - Line 1: $10.91 profit (unusual but possible)")
    print("   → ACTION: Verify this is realistic for the bridge fee structure")
    print()
    print("3. LIFI V2:")
    print("   - Line 8: $3.48 profit (1825985822411639283 amount)")
    print("   → ACTION: Check if amount is realistic")
    print()
    print("4. ACROSS V3:")
    print("   - Line 5: -$5.09 profit (4308085947898706 amount)")
    print("   → ACTION: High loss suggests gas cost calculation issue")
    print()
    print("NEXT STEPS:")
    print("  1. Fix orbiter_finance test data (most critical)")
    print("  2. Validate all amounts are in proper decimals (wei for EVM)")
    print("  3. Re-run autonomous_monitor.py to verify fixes")
    print("  4. Consider generating fixtures from live bridge data")
    print()

if __name__ == "__main__":
    main()
