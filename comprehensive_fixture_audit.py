#!/usr/bin/env python3
"""
Comprehensive Fixture Audit Script
Analyzes ALL fixture files and identifies EVERY issue systematically
"""

import json
import os
from pathlib import Path
from collections import defaultdict
from decimal import Decimal

# Configuration
FIXTURES_DIR = "fixtures"
U128_MAX = 340282366920938463463374607431768211455  # 2^128 - 1
MAX_REASONABLE_AMOUNT_USD = 10_000_000  # $10M
MIN_REASONABLE_AMOUNT_USD = 0.000001  # $0.000001

# Token prices (simplified)
TOKEN_PRICES = {
    "ETH": 3000.0,
    "WETH": 3000.0,
    "USDC": 1.0,
    "USDT": 1.0,
    "DAI": 1.0,
    "native": 3000.0,  # Assume ETH
}

# Chain names for better reporting
CHAIN_NAMES = {
    1: "Ethereum",
    10: "Optimism",
    56: "BSC",
    100: "Gnosis",
    137: "Polygon",
    143: "Monad",
    250: "Fantom",
    252: "Fraxtal",
    1284: "Moonbeam",
    1868: "Lisk",
    2222: "Kava",
    8453: "Base",
    42161: "Arbitrum",
    43114: "Avalanche",
    59144: "Linea",
}

class FixtureAuditor:
    def __init__(self):
        self.issues = defaultdict(list)
        self.stats = defaultdict(int)
        self.protocol_stats = defaultdict(lambda: {
            "total": 0,
            "valid": 0,
            "issues": defaultdict(int)
        })

    def get_token_symbol(self, token_addr):
        """Infer token symbol from address"""
        if not token_addr or token_addr == "0x0000000000000000000000000000000000000000":
            return "ETH"
        if "native" in token_addr.lower():
            return "native"
        # Common token addresses (simplified)
        return "UNKNOWN"

    def estimate_token_price(self, token_addr, chain_id):
        """Estimate token price"""
        symbol = self.get_token_symbol(token_addr)
        return TOKEN_PRICES.get(symbol, 3000.0)  # Default to ETH price

    def check_u128_overflow(self, intent, protocol):
        """Check if amount overflows u128"""
        try:
            amount_str = intent.get("amount", "0")
            amount_int = int(amount_str)

            if amount_int > U128_MAX:
                self.issues[protocol].append({
                    "type": "U128_OVERFLOW",
                    "severity": "CRITICAL",
                    "id": intent.get("id", "unknown"),
                    "amount": amount_str,
                    "digits": len(amount_str),
                    "u128_max": U128_MAX,
                    "message": f"Amount {amount_str} ({len(amount_str)} digits) overflows u128 max (~10^38)"
                })
                self.protocol_stats[protocol]["issues"]["u128_overflow"] += 1
                return False
            return True
        except (ValueError, TypeError) as e:
            self.issues[protocol].append({
                "type": "PARSE_ERROR",
                "severity": "HIGH",
                "id": intent.get("id", "unknown"),
                "amount": intent.get("amount", "MISSING"),
                "error": str(e),
                "message": f"Cannot parse amount: {intent.get('amount')}"
            })
            self.protocol_stats[protocol]["issues"]["parse_error"] += 1
            return False

    def check_zero_amount(self, intent, protocol):
        """Check for zero amounts"""
        try:
            amount_str = intent.get("amount", "0")
            amount_int = int(amount_str)

            if amount_int == 0:
                self.issues[protocol].append({
                    "type": "ZERO_AMOUNT",
                    "severity": "HIGH",
                    "id": intent.get("id", "unknown"),
                    "src_chain": intent.get("src_chain"),
                    "dst_chain": intent.get("dst_chain"),
                    "message": "Intent has zero amount"
                })
                self.protocol_stats[protocol]["issues"]["zero_amount"] += 1
                return False
            return True
        except:
            return True  # Already caught by parse_error

    def check_negative_profit(self, intent, protocol):
        """Check for negative profits (gas calculation bug indicator)"""
        profit = intent.get("profit_usd", 0)

        if profit < 0:
            self.issues[protocol].append({
                "type": "NEGATIVE_PROFIT",
                "severity": "MEDIUM",
                "id": intent.get("id", "unknown"),
                "profit": profit,
                "amount": intent.get("amount"),
                "src_chain": f"{intent.get('src_chain')} ({CHAIN_NAMES.get(intent.get('src_chain'), '?')})",
                "dst_chain": f"{intent.get('dst_chain')} ({CHAIN_NAMES.get(intent.get('dst_chain'), '?')})",
                "state": intent.get("state", "unknown"),
                "message": f"Negative profit ${profit:.2f} (likely gas calculation bug)"
            })
            self.protocol_stats[protocol]["issues"]["negative_profit"] += 1
            return False
        return True

    def check_unrealistic_profit(self, intent, protocol):
        """Check for unrealistic profit margins"""
        try:
            amount_str = intent.get("amount", "0")
            amount_int = int(amount_str)
            profit = intent.get("profit_usd", 0)

            if amount_int == 0 or profit <= 0:
                return True  # Skip if invalid data

            # Estimate amount in USD (rough calculation)
            # Assume 18 decimals for now
            amount_eth = amount_int / 1e18
            estimated_usd = amount_eth * 3000  # Rough ETH price

            if estimated_usd > 0:
                profit_margin = (profit / estimated_usd) * 100

                # Profit margin > 10% is suspicious
                if profit_margin > 10:
                    self.issues[protocol].append({
                        "type": "UNREALISTIC_PROFIT",
                        "severity": "LOW",
                        "id": intent.get("id", "unknown"),
                        "profit": profit,
                        "estimated_amount_usd": estimated_usd,
                        "profit_margin_pct": round(profit_margin, 2),
                        "message": f"Profit margin {profit_margin:.1f}% seems unrealistic (>${profit:.2f} on ~${estimated_usd:.2f})"
                    })
                    self.protocol_stats[protocol]["issues"]["unrealistic_profit"] += 1
                    return False
            return True
        except:
            return True

    def check_missing_fields(self, intent, protocol):
        """Check for missing required fields"""
        required_fields = ["id", "protocol", "src_chain", "dst_chain", "amount", "state"]
        missing = [f for f in required_fields if f not in intent or intent[f] is None]

        if missing:
            self.issues[protocol].append({
                "type": "MISSING_FIELDS",
                "severity": "HIGH",
                "id": intent.get("id", "unknown"),
                "missing_fields": missing,
                "message": f"Missing required fields: {', '.join(missing)}"
            })
            self.protocol_stats[protocol]["issues"]["missing_fields"] += 1
            return False
        return True

    def check_invalid_chains(self, intent, protocol):
        """Check for invalid chain IDs"""
        src_chain = intent.get("src_chain", 0)
        dst_chain = intent.get("dst_chain", 0)

        if src_chain == dst_chain:
            self.issues[protocol].append({
                "type": "SAME_CHAIN",
                "severity": "HIGH",
                "id": intent.get("id", "unknown"),
                "chain": src_chain,
                "message": f"Source and destination are the same chain ({src_chain})"
            })
            self.protocol_stats[protocol]["issues"]["same_chain"] += 1
            return False

        if src_chain == 0 or dst_chain == 0:
            self.issues[protocol].append({
                "type": "INVALID_CHAIN",
                "severity": "MEDIUM",
                "id": intent.get("id", "unknown"),
                "src_chain": src_chain,
                "dst_chain": dst_chain,
                "message": f"Chain ID 0 is invalid (src: {src_chain}, dst: {dst_chain})"
            })
            self.protocol_stats[protocol]["issues"]["invalid_chain"] += 1
            return False

        return True

    def audit_intent(self, intent, protocol):
        """Run all checks on a single intent"""
        self.protocol_stats[protocol]["total"] += 1

        checks = [
            self.check_missing_fields,
            self.check_u128_overflow,
            self.check_zero_amount,
            self.check_invalid_chains,
            self.check_negative_profit,
            self.check_unrealistic_profit,
        ]

        all_passed = True
        for check in checks:
            if not check(intent, protocol):
                all_passed = False

        if all_passed:
            self.protocol_stats[protocol]["valid"] += 1

        return all_passed

    def audit_fixture_file(self, filepath):
        """Audit a single fixture file"""
        protocol = Path(filepath).stem.replace("_intents", "")

        print(f"Auditing {filepath}...")

        try:
            with open(filepath, 'r') as f:
                # Try to parse as NDJSON first
                intents = []
                for line_num, line in enumerate(f, 1):
                    line = line.strip()
                    if not line:
                        continue
                    try:
                        intent = json.loads(line)
                        intents.append(intent)
                    except json.JSONDecodeError as e:
                        self.issues[protocol].append({
                            "type": "JSON_PARSE_ERROR",
                            "severity": "CRITICAL",
                            "line": line_num,
                            "error": str(e),
                            "message": f"Line {line_num}: Invalid JSON"
                        })
                        continue

            if not intents:
                print(f"  ⚠️  No valid intents found in {filepath}")
                return

            print(f"  Found {len(intents)} intents")

            for intent in intents:
                self.audit_intent(intent, protocol)

        except Exception as e:
            print(f"  ❌ Error reading {filepath}: {e}")
            self.issues[protocol].append({
                "type": "FILE_ERROR",
                "severity": "CRITICAL",
                "error": str(e),
                "message": f"Cannot read file: {e}"
            })

    def generate_report(self):
        """Generate comprehensive report"""
        report = []
        report.append("=" * 100)
        report.append("COMPREHENSIVE FIXTURE AUDIT REPORT")
        report.append("=" * 100)
        report.append("")

        # Summary statistics
        report.append("SUMMARY")
        report.append("-" * 100)
        total_intents = sum(stats["total"] for stats in self.protocol_stats.values())
        total_valid = sum(stats["valid"] for stats in self.protocol_stats.values())
        total_issues = total_intents - total_valid

        report.append(f"Total Intents: {total_intents}")
        report.append(f"Valid Intents: {total_valid} ({100 * total_valid / total_intents if total_intents > 0 else 0:.1f}%)")
        report.append(f"Intents with Issues: {total_issues} ({100 * total_issues / total_intents if total_intents > 0 else 0:.1f}%)")
        report.append("")

        # Protocol breakdown
        report.append("PROTOCOL BREAKDOWN")
        report.append("-" * 100)
        for protocol in sorted(self.protocol_stats.keys()):
            stats = self.protocol_stats[protocol]
            report.append(f"\n{protocol.upper()}:")
            report.append(f"  Total: {stats['total']}")
            report.append(f"  Valid: {stats['valid']} ({100 * stats['valid'] / stats['total'] if stats['total'] > 0 else 0:.1f}%)")

            if stats['issues']:
                report.append(f"  Issues:")
                for issue_type, count in sorted(stats['issues'].items()):
                    report.append(f"    - {issue_type}: {count}")

        report.append("")
        report.append("")

        # Detailed issues
        report.append("DETAILED ISSUES BY PROTOCOL")
        report.append("=" * 100)

        for protocol in sorted(self.issues.keys()):
            protocol_issues = self.issues[protocol]
            if not protocol_issues:
                continue

            report.append(f"\n{protocol.upper()} - {len(protocol_issues)} issues")
            report.append("-" * 100)

            # Group by severity
            by_severity = defaultdict(list)
            for issue in protocol_issues:
                by_severity[issue.get("severity", "UNKNOWN")].append(issue)

            for severity in ["CRITICAL", "HIGH", "MEDIUM", "LOW"]:
                if severity not in by_severity:
                    continue

                report.append(f"\n{severity} ({len(by_severity[severity])} issues):")
                report.append("")

                for issue in by_severity[severity]:
                    report.append(f"  [{issue['type']}] {issue['message']}")
                    report.append(f"    ID: {issue.get('id', 'N/A')}")
                    for key, value in issue.items():
                        if key not in ['type', 'severity', 'message', 'id']:
                            report.append(f"    {key}: {value}")
                    report.append("")

        report.append("")
        report.append("=" * 100)
        report.append("END OF REPORT")
        report.append("=" * 100)

        return "\n".join(report)

def main():
    auditor = FixtureAuditor()

    # Protocol-specific fixture files
    protocol_fixtures = [
        "across_v3_intents.json",
        "allbridge_intents.json",
        "hyperlane_intents.json",
        "layerzero_v2_intents.json",
        "lifi_v2_intents.json",
        "orbiter_finance_intents.json",
        "squid_router_intents.json",
        "stargate_v2_intents.json",
        "t3rn_lwc_intents.json",
    ]

    for fixture in protocol_fixtures:
        filepath = os.path.join(FIXTURES_DIR, fixture)
        if os.path.exists(filepath):
            auditor.audit_fixture_file(filepath)

    # Generate and save report
    report = auditor.generate_report()

    # Print to console
    print("\n" + report)

    # Save to file
    with open("FIXTURE_AUDIT_REPORT.md", "w") as f:
        f.write(report)

    print(f"\n✅ Report saved to FIXTURE_AUDIT_REPORT.md")

if __name__ == "__main__":
    main()
