#!/usr/bin/env python3
"""
Fixture Filter - Remove Unsupported Chains
Filters all protocol fixtures to only include supported chains.
Removes invalid chain IDs (0, corrupted values, unsupported chains).
"""

import json
from pathlib import Path
from typing import Dict, List, Set

FIXTURES_DIR = Path("fixtures")
BACKUP_DIR = FIXTURES_DIR / "backup"

# Supported chains (have gas data + active collectors in Spinner)
SUPPORTED_CHAINS = {
    1,      # Ethereum
    10,     # Optimism
    56,     # BSC
    137,    # Polygon
    143,    # Monad
    200,    # Solana
    250,    # Fantom
    252,    # Frax
    324,    # zkSync Era
    999,    # Zora
    1101,   # Polygon zkEVM
    1284,   # Moonbeam
    7777777,# Zora
    8453,   # Base
    34443,  # Mode
    42161,  # Arbitrum
    43114,  # Avalanche
    59144,  # Linea
    81457,  # Blast
    534352, # Scroll
}

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

def load_ndjson(file_path: Path) -> List[Dict]:
    """Load NDJSON file"""
    intents = []
    if not file_path.exists():
        return []

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
    except Exception as e:
        print(f"    ⚠️  Error reading {file_path}: {e}")
        return []

    return intents

def save_ndjson(file_path: Path, intents: List[Dict]):
    """Save NDJSON file"""
    with open(file_path, 'w') as f:
        for intent in intents:
            f.write(json.dumps(intent) + '\n')

def is_valid_chain(chain_id: int) -> bool:
    """Check if chain ID is valid and supported"""
    if chain_id == 0:
        return False
    if chain_id > 10_000_000_000:  # Corrupted overflow values
        return False
    if chain_id not in SUPPORTED_CHAINS:
        return False
    return True

def filter_protocol_intents(protocol: str) -> Dict:
    """Filter intents for a protocol"""
    fixture_file = FIXTURES_DIR / f"{protocol}.json"

    if not fixture_file.exists():
        return {
            "protocol": protocol,
            "original": 0,
            "filtered": 0,
            "removed": 0,
            "issues": ["File not found"]
        }

    intents = load_ndjson(fixture_file)
    original_count = len(intents)

    # Filter valid intents
    valid_intents = []
    removed_reasons = {
        "chain_0": 0,
        "corrupted_chain": 0,
        "unsupported_src": 0,
        "unsupported_dst": 0,
    }

    for intent in intents:
        src_chain = intent.get("src_chain", 0)
        dst_chain = intent.get("dst_chain", 0)

        # Check for chain ID 0
        if src_chain == 0 or dst_chain == 0:
            removed_reasons["chain_0"] += 1
            continue

        # Check for corrupted chain IDs (overflow)
        if src_chain > 10_000_000_000 or dst_chain > 10_000_000_000:
            removed_reasons["corrupted_chain"] += 1
            continue

        # Check for unsupported chains
        if src_chain not in SUPPORTED_CHAINS:
            removed_reasons["unsupported_src"] += 1
            continue

        if dst_chain not in SUPPORTED_CHAINS:
            removed_reasons["unsupported_dst"] += 1
            continue

        # Intent is valid
        valid_intents.append(intent)

    filtered_count = len(valid_intents)
    removed_count = original_count - filtered_count

    # Backup original file
    BACKUP_DIR.mkdir(exist_ok=True)
    backup_file = BACKUP_DIR / f"{protocol}.json.backup"
    if fixture_file.exists():
        import shutil
        shutil.copy2(fixture_file, backup_file)

    # Save filtered intents
    if valid_intents:
        save_ndjson(fixture_file, valid_intents)
        print(f"  ✅ {protocol}: {original_count} → {filtered_count} intents ({removed_count} removed)")
    else:
        print(f"  ⚠️  {protocol}: ALL {original_count} intents removed (no supported chains)")
        # Keep file but make it empty
        fixture_file.write_text("")

    # Build issues list
    issues = []
    for reason, count in removed_reasons.items():
        if count > 0:
            issues.append(f"{reason}: {count}")

    return {
        "protocol": protocol,
        "original": original_count,
        "filtered": filtered_count,
        "removed": removed_count,
        "removal_reasons": removed_reasons,
        "issues": issues
    }

def main():
    print("="*80)
    print("FIXTURE FILTER - REMOVE UNSUPPORTED CHAINS")
    print("="*80)
    print()
    print(f"Supported chains: {len(SUPPORTED_CHAINS)}")
    print(f"Protocols to filter: {len(PROTOCOLS)}")
    print()

    results = []
    total_original = 0
    total_filtered = 0
    total_removed = 0

    for protocol in PROTOCOLS:
        result = filter_protocol_intents(protocol)
        results.append(result)
        total_original += result["original"]
        total_filtered += result["filtered"]
        total_removed += result["removed"]

    print()
    print("="*80)
    print("FILTERING SUMMARY")
    print("="*80)
    print(f"Total intents (original):  {total_original}")
    print(f"Total intents (filtered):  {total_filtered}")
    print(f"Total removed:             {total_removed}")
    print(f"Retention rate:            {(total_filtered/total_original*100):.1f}%" if total_original > 0 else "N/A")
    print("="*80)
    print()

    # Print protocol breakdown
    print("PROTOCOL BREAKDOWN")
    print("-"*80)
    print(f"{'Protocol':<30} {'Original':>10} {'Filtered':>10} {'Removed':>10} {'Rate':>8}")
    print("-"*80)

    for result in results:
        rate = (result["filtered"] / result["original"] * 100) if result["original"] > 0 else 0
        print(f"{result['protocol']:<30} {result['original']:>10} {result['filtered']:>10} {result['removed']:>10} {rate:>7.1f}%")

    print()

    # Print removal reasons
    print("REMOVAL REASONS")
    print("-"*80)

    reason_totals = {
        "chain_0": 0,
        "corrupted_chain": 0,
        "unsupported_src": 0,
        "unsupported_dst": 0,
    }

    for result in results:
        for reason, count in result.get("removal_reasons", {}).items():
            reason_totals[reason] += count

    for reason, count in reason_totals.items():
        if count > 0:
            print(f"  {reason:.<40} {count:>5}")

    print()
    print("✅ Filtering complete!")
    print(f"📁 Backups saved to: {BACKUP_DIR}")
    print()
    print("Next steps:")
    print("  1. Run autonomous_monitor.py to validate filtered fixtures")
    print("  2. Check that protocols now have better pass rates")
    print()

if __name__ == "__main__":
    main()
