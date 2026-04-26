# Taifoon Solver Test Fixtures

This directory contains real production data extracted from the Taifoon Solver for testing and validation.

## Files

### Evidence & Bug Reports

- **SMOKING_GUN_EVIDENCE.md** - Unrealistic profit calculations proving gas calculation bugs
- **unrealistic_profits.json** - 9 intents with profits >$1 or <-$2 (impossible in reality)

### Intent Data (from extract-intent-fixtures.sh)

Per-protocol intent samples:
- `across_v3_intents.json` (8 samples)
- `allbridge_intents.json` (9 samples)
- `hyperlane_intents.json` (6 samples)
- `layerzero_v2_intents.json` (9 samples)
- `lifi_v2_intents.json` (20 samples)
- `orbiter_finance_intents.json` (37 samples)
- `squid_router_intents.json` (2 samples)
- `stargate_v2_intents.json` (8 samples)
- `t3rn_lwc_intents.json` (1 sample) - **Autonomous delivery target!**

### Coverage Data

- `intents-full-dump-*.json` - Complete snapshot of 100 live intents
- `chains-observed-*.txt` - List of unique chain IDs seen
- `routes-observed-*.txt` - Cross-chain routes (55 unique)
- `extraction-summary-*.json` - Metadata about the extraction

## Usage

### For Testing

```bash
# Test protocol decoders
cat t3rn_lwc_intents.json | jq .

# Test profit calculation
cat unrealistic_profits.json | jq '.[0]'
```

### For Regression Testing

After fixing gas bugs, re-run extraction and compare:

```bash
./extract-intent-fixtures.sh
diff fixtures/unrealistic_profits.json fixtures/unrealistic_profits_after_fix.json
```

Expected: `unrealistic_profits_after_fix.json` should be EMPTY or much smaller.

## Critical Bugs Proven by Fixtures

See SMOKING_GUN_EVIDENCE.md for details:

1. **BSC Gas = 0** → $51 BILLION profit on 51 tokens
2. **OP Stack Gas Too Low** → $3-100 profits on small amounts
3. **Chain ID 0 Decoding** → Unknown how many real chains misidentified

**Status:** 🚨 DO NOT ENABLE AUTONOMOUS FILLS UNTIL THESE ARE FIXED

## Updating Fixtures

```bash
cd /Users/mbultra/projects/taifoon-solver
./extract-intent-fixtures.sh
```

Generates timestamped snapshots of:
- All protocols (separate JSON files)
- All chains (text list)
- All routes (text list)
- Full 100-intent dump

## Per-Chain Gas Data

The Razor endpoint provides gas estimates for 30 chains. Expected format:

```json
{
  "presets": [
    {
      "chain_id": 1,
      "chain_name": "Ethereum",
      "ready": true,
      "gas_cost_gwei": 0.464966278,
      "gas_cost_usd": 1.34,
      "symbol": "ETH"
    }
  ]
}
```

**Dashboard usage:** See `dashboard/components/RazorGasPresets.tsx`
