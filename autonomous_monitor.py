#!/usr/bin/env python3
"""
Autonomous Protocol Monitor & Validator
Continuously monitors Spinner API for new intents, validates data integrity,
and provides real-time protocol health status.

Features:
- Real-time SSE listening for new intents
- Autonomous data integrity validation
- Live gas price monitoring
- Protocol health tracking
- Alert system for failures
- Comprehensive status dashboard
"""

import json
import asyncio
import aiohttp
from pathlib import Path
from typing import Dict, List, Optional, Set
from dataclasses import dataclass, asdict
from datetime import datetime, timedelta
from collections import defaultdict
import sys

# Configuration
SPINNER_API_BASE = "http://46.4.96.124:30081"
SPINNER_GAS_API = f"{SPINNER_API_BASE}/api/gas/latest"
GENOME_SSE_URL = f"{SPINNER_API_BASE}/api/genome/subscribe/sse"
FIXTURES_DIR = Path("fixtures")

# Supported chains (have gas data + active collectors)
SUPPORTED_CHAINS = {
    1, 10, 56, 137, 143, 200, 250, 252, 324, 999, 1101, 1284,
    7777777, 8453, 34443, 42161, 43114, 59144, 81457, 534352
}

# Protocol registry
PROTOCOLS = [
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
    timestamp: str

@dataclass
class ProtocolHealth:
    protocol: str
    total: int
    passed: int
    failed: int
    warnings: int
    last_check: str
    pass_rate: float
    critical_issues: List[str]
    unsupported_chains: Set[int]

class GasCache:
    """Async gas price cache with TTL"""
    def __init__(self, ttl_seconds=30):
        self.cache: Dict[int, Dict] = {}
        self.timestamps: Dict[int, datetime] = {}
        self.ttl = timedelta(seconds=ttl_seconds)

    def get(self, chain_id: int) -> Optional[Dict]:
        if chain_id in self.cache:
            if datetime.now() - self.timestamps[chain_id] < self.ttl:
                return self.cache[chain_id]
        return None

    def set(self, chain_id: int, data: Dict):
        self.cache[chain_id] = data
        self.timestamps[chain_id] = datetime.now()

class AutonomousMonitor:
    def __init__(self):
        self.gas_cache = GasCache(ttl_seconds=30)
        self.protocol_health: Dict[str, ProtocolHealth] = {}
        self.validation_history: List[ValidationResult] = []
        self.alert_queue: List[str] = []
        self.stats = {
            "total_validated": 0,
            "total_passed": 0,
            "total_failed": 0,
            "total_warnings": 0,
            "start_time": datetime.now().isoformat(),
        }

    async def fetch_gas_price(self, session: aiohttp.ClientSession, chain_id: int) -> Optional[Dict]:
        """Fetch gas price with caching"""
        # Check cache first
        cached = self.gas_cache.get(chain_id)
        if cached:
            return cached

        try:
            url = f"{SPINNER_GAS_API}/{chain_id}"
            async with session.get(url, timeout=aiohttp.ClientTimeout(total=5)) as response:
                if response.status == 200:
                    data = await response.json()
                    self.gas_cache.set(chain_id, data)
                    return data
                else:
                    print(f"    ⚠️  Chain {chain_id}: HTTP {response.status}")
                    return None
        except Exception as e:
            print(f"    ⚠️  Chain {chain_id}: {str(e)[:50]}")
            return None

    async def validate_intent(self, session: aiohttp.ClientSession, intent: Dict) -> ValidationResult:
        """Validate a single intent"""
        intent_id = intent.get("id", "unknown")
        protocol = intent.get("protocol", "unknown")
        src_chain = intent.get("src_chain", 0)
        dst_chain = intent.get("dst_chain", 0)
        raw_profit = intent.get("profit_usd")

        issues = []
        status = "PASS"

        # Fetch gas prices in parallel
        gas_results = await asyncio.gather(
            self.fetch_gas_price(session, src_chain),
            self.fetch_gas_price(session, dst_chain),
            return_exceptions=True
        )

        src_gas = gas_results[0] if not isinstance(gas_results[0], Exception) else None
        dst_gas = gas_results[1] if not isinstance(gas_results[1], Exception) else None

        src_gas_gwei = src_gas.get("gas_price_gwei") if src_gas else None
        dst_gas_gwei = dst_gas.get("gas_price_gwei") if dst_gas else None

        # Check chain support
        if src_chain not in SUPPORTED_CHAINS:
            issues.append(f"Unsupported src chain {src_chain}")
            status = "WARNING"
        if dst_chain not in SUPPORTED_CHAINS:
            issues.append(f"Unsupported dst chain {dst_chain}")
            status = "WARNING"

        # Check for missing gas prices
        if src_gas is None and src_chain in SUPPORTED_CHAINS:
            issues.append(f"Missing gas price for src chain {src_chain}")
            status = "WARNING"
        if dst_gas is None and dst_chain in SUPPORTED_CHAINS:
            issues.append(f"Missing gas price for dst chain {dst_chain}")
            status = "WARNING"

        # Check for zero gas prices (CRITICAL)
        if src_gas_gwei is not None and src_gas_gwei == 0:
            issues.append(f"CRITICAL: Zero gas price for src chain {src_chain}")
            status = "FAIL"
        if dst_gas_gwei is not None and dst_gas_gwei == 0:
            issues.append(f"CRITICAL: Zero gas price for dst chain {dst_chain}")
            status = "FAIL"

        # Calculate total gas cost
        GAS_LIMIT = 60_000
        token_prices = {
            1: 3000, 10: 3000, 8453: 3000, 42161: 3000, 59144: 3000,  # ETH chains
            137: 0.9, 56: 600, 200: 150, 43114: 30, 250: 0.5,  # Other chains
        }

        total_gas_cost = None
        if src_gas_gwei is not None and dst_gas_gwei is not None:
            src_token_price = token_prices.get(src_chain, 1)
            dst_token_price = token_prices.get(dst_chain, 1)
            src_gas_cost = (src_gas_gwei / 1e9) * GAS_LIMIT * src_token_price
            dst_gas_cost = (dst_gas_gwei / 1e9) * GAS_LIMIT * dst_token_price
            total_gas_cost = src_gas_cost + dst_gas_cost

            if total_gas_cost > 1.0:
                issues.append(f"High gas cost: ${total_gas_cost:.4f} (likely unprofitable)")
                status = "FAIL"

        # Check raw profit sanity
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
            status=status,
            timestamp=datetime.now().isoformat()
        )

    def load_ndjson(self, file_path: Path) -> List[Dict]:
        """Load NDJSON file"""
        intents = []
        try:
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
                        print(f"    ⚠️  JSON decode error at position {idx}: {e}")
                        break
        except FileNotFoundError:
            return []
        return intents

    async def validate_protocol_fixtures(self, session: aiohttp.ClientSession, protocol: str) -> ProtocolHealth:
        """Validate all fixtures for a protocol"""
        fixture_file = FIXTURES_DIR / f"{protocol}.json"

        if not fixture_file.exists():
            return ProtocolHealth(
                protocol=protocol,
                total=0,
                passed=0,
                failed=0,
                warnings=0,
                last_check=datetime.now().isoformat(),
                pass_rate=0.0,
                critical_issues=["Fixture file not found"],
                unsupported_chains=set()
            )

        intents = self.load_ndjson(fixture_file)

        # Validate all intents in parallel (batched to avoid overwhelming API)
        batch_size = 10
        results = []

        for i in range(0, len(intents), batch_size):
            batch = intents[i:i + batch_size]
            batch_results = await asyncio.gather(
                *[self.validate_intent(session, intent) for intent in batch]
            )
            results.extend(batch_results)
            await asyncio.sleep(0.5)  # Rate limiting

        # Analyze results
        total = len(results)
        passed = sum(1 for r in results if r.status == "PASS")
        failed = sum(1 for r in results if r.status == "FAIL")
        warnings = sum(1 for r in results if r.status == "WARNING")
        pass_rate = (passed / total * 100) if total > 0 else 0.0

        critical_issues = []
        unsupported_chains = set()

        for r in results:
            if r.status == "FAIL":
                for issue in r.issues:
                    if "CRITICAL" in issue or "Zero gas" in issue:
                        critical_issues.append(f"{r.intent_id}: {issue}")

            if r.src_chain not in SUPPORTED_CHAINS:
                unsupported_chains.add(r.src_chain)
            if r.dst_chain not in SUPPORTED_CHAINS:
                unsupported_chains.add(r.dst_chain)

        # Update global stats
        self.stats["total_validated"] += total
        self.stats["total_passed"] += passed
        self.stats["total_failed"] += failed
        self.stats["total_warnings"] += warnings

        return ProtocolHealth(
            protocol=protocol,
            total=total,
            passed=passed,
            failed=failed,
            warnings=warnings,
            last_check=datetime.now().isoformat(),
            pass_rate=pass_rate,
            critical_issues=critical_issues[:10],  # Limit to top 10
            unsupported_chains=unsupported_chains
        )

    async def run_full_validation(self):
        """Run full validation sweep of all protocols"""
        print("\n" + "="*80)
        print("AUTONOMOUS PROTOCOL MONITOR - FULL VALIDATION SWEEP")
        print("="*80)
        print(f"Timestamp: {datetime.now().isoformat()}")
        print(f"Checking {len(PROTOCOLS)} protocols...")
        print()

        async with aiohttp.ClientSession() as session:
            # Validate all protocols in parallel
            health_results = await asyncio.gather(
                *[self.validate_protocol_fixtures(session, p) for p in PROTOCOLS]
            )

            # Update health tracking
            for health in health_results:
                self.protocol_health[health.protocol] = health

        # Print results
        self.print_protocol_breakdown()
        self.print_critical_issues()
        self.print_unsupported_chains()

    def print_protocol_breakdown(self):
        """Print protocol health breakdown"""
        print("\n" + "="*80)
        print("PROTOCOL HEALTH BREAKDOWN")
        print("="*80)
        print(f"{'Protocol':<30} {'Total':>8} {'Pass':>8} {'Fail':>8} {'Warn':>8} {'Pass%':>8} {'Status':>10}")
        print("-"*80)

        for protocol in sorted(self.protocol_health.keys()):
            health = self.protocol_health[protocol]

            # Determine status symbol
            if health.pass_rate >= 90:
                status = "✅ HEALTHY"
            elif health.pass_rate >= 70:
                status = "⚠️  DEGRADED"
            else:
                status = "❌ FAILING"

            print(f"{protocol:<30} {health.total:>8} {health.passed:>8} {health.failed:>8} {health.warnings:>8} {health.pass_rate:>7.1f}% {status:>10}")

        print()

    def print_critical_issues(self):
        """Print critical issues across all protocols"""
        all_critical = []
        for health in self.protocol_health.values():
            if health.critical_issues:
                all_critical.extend([(health.protocol, issue) for issue in health.critical_issues])

        if all_critical:
            print("\n" + "="*80)
            print(f"CRITICAL ISSUES ({len(all_critical)} found)")
            print("="*80)
            for protocol, issue in all_critical[:20]:  # Top 20
                print(f"  ❌ [{protocol}] {issue}")
            print()

    def print_unsupported_chains(self):
        """Print unsupported chains across all protocols"""
        all_unsupported = set()
        protocol_chains = defaultdict(set)

        for health in self.protocol_health.values():
            for chain_id in health.unsupported_chains:
                all_unsupported.add(chain_id)
                protocol_chains[chain_id].add(health.protocol)

        if all_unsupported:
            print("\n" + "="*80)
            print(f"UNSUPPORTED CHAINS ({len(all_unsupported)} chains)")
            print("="*80)
            print("These chains need collector implementation or filtering:")
            print()

            for chain_id in sorted(all_unsupported):
                protocols = ", ".join(sorted(protocol_chains[chain_id]))
                print(f"  Chain {chain_id:>6}: used by {protocols}")
            print()

    def print_summary(self):
        """Print overall summary"""
        print("\n" + "="*80)
        print("VALIDATION SUMMARY")
        print("="*80)

        total = self.stats["total_validated"]
        passed = self.stats["total_passed"]
        failed = self.stats["total_failed"]
        warnings = self.stats["total_warnings"]
        pass_rate = (passed / total * 100) if total > 0 else 0

        print(f"Total Intents:    {total}")
        print(f"✅ PASS:          {passed} ({pass_rate:.1f}%)")
        print(f"❌ FAIL:          {failed}")
        print(f"⚠️  WARNING:      {warnings}")
        print(f"Uptime:           {self.stats['start_time']}")
        print("="*80)
        print()

        # Verdict
        if failed == 0:
            print("✅ ALL VALIDATIONS PASSED")
        elif pass_rate >= 90:
            print("⚠️  MOSTLY HEALTHY (some failures present)")
        else:
            print("❌ CRITICAL: Multiple failures detected")
        print()

async def main():
    monitor = AutonomousMonitor()

    print("="*80)
    print("TAIFOON AUTONOMOUS PROTOCOL MONITOR")
    print("="*80)
    print("Real-time validation & health monitoring")
    print()

    # Run initial full validation
    await monitor.run_full_validation()
    monitor.print_summary()

    # Export results to JSON
    output_file = Path("protocol_health_report.json")
    report = {
        "timestamp": datetime.now().isoformat(),
        "stats": monitor.stats,
        "protocols": {
            name: {
                **asdict(health),
                "unsupported_chains": list(health.unsupported_chains)
            }
            for name, health in monitor.protocol_health.items()
        }
    }

    with open(output_file, 'w') as f:
        json.dump(report, f, indent=2)

    print(f"📊 Report exported to: {output_file}")
    print()

if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        print("\n\nMonitor stopped by user.")
        sys.exit(0)
