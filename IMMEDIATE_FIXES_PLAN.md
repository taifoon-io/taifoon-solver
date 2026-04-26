# Immediate Gas Pricing Fixes - Implementation Plan

**Status:** Ready to implement
**Priority:** CRITICAL - Production blocker
**Estimated Time:** 2-4 hours

---

## Overview

This document outlines the immediate fixes (Phase 1) to stop the gas pricing bugs that are causing impossible profit calculations in the Taifoon Solver.

## Files to Modify

### 1. Spinner API Gas Validation
**File:** `/Users/mbultra/projects/spinner/rust/crates/da-api/src/api.rs`

**Changes:**
- Add `get_fallback_gas_price_gwei(chain_id: u64) -> Option<f64>` helper function
- Add `validate_gas_price_gwei(raw_gwei: Option<f64>, chain_id: u64) -> (Option<f64>, bool)` validation function
- Modify `get_latest_gas_chain()` at line 493-521 to use validation
- Modify `get_latest_gas_all()` at line 526-556 to use validation

**Fallback Values:**
```rust
56 => Some(3.0),      // BSC
10 | 8453 | ... => Some(0.001),  // OP Stack
42161 | 421614 => Some(0.01),    // Arbitrum
59144 | 534352 | 1101 => Some(0.001),  // zkEVMs
1 => Some(30.0),      // Ethereum
```

**Validation Rules:**
- If gas < 1e-6 gwei → use fallback
- If gas > 10000 gwei → use fallback
- If gas is None → use fallback
- Log warnings to stderr for monitoring

### 2. Taifoon Solver Profit Validation
**Location:** Needs investigation - likely in profitability calculation code

**Changes:**
- Add profit sanity checks before auto-filling
- Flag intents with profit > $10 as suspicious
- Skip intents with profit < -$10 as likely bugs
- Log all suspicious intents for manual review

---

## Implementation Steps

### Step 1: Add Helper Functions to Spinner API

Insert before `get_latest_gas_chain()` function (before line 489):

```rust
/// Fallback gas prices for chains with known issues (in gwei)
fn get_fallback_gas_price_gwei(chain_id: u64) -> Option<f64> {
    match chain_id {
        56 => Some(3.0),  // BSC
        10 | 8453 | 34443 | 252 | 7777777 | 84532 | 957 | 1135 | 81457 => Some(0.001),  // OP Stack
        42161 | 421614 => Some(0.01),  // Arbitrum
        59144 | 534352 | 1101 => Some(0.001),  // zkEVMs
        1 => Some(30.0),  // Ethereum
        _ => None,
    }
}

/// Validate and sanitize gas price with fallback for suspicious values
fn validate_gas_price_gwei(raw_gwei: Option<f64>, chain_id: u64) -> (Option<f64>, bool) {
    const MIN_REASONABLE_GWEI: f64 = 1e-6;
    const MAX_REASONABLE_GWEI: f64 = 10000.0;

    match raw_gwei {
        Some(g) if g < MIN_REASONABLE_GWEI => {
            eprintln!("⚠️  Chain {} gas too low: {} gwei, using fallback", chain_id, g);
            (get_fallback_gas_price_gwei(chain_id), true)
        },
        Some(g) if g > MAX_REASONABLE_GWEI => {
            eprintln!("⚠️  Chain {} gas too high: {} gwei, using fallback", chain_id, g);
            (get_fallback_gas_price_gwei(chain_id), true)
        },
        Some(g) => (Some(g), false),
        None => {
            eprintln!("⚠️  Chain {} gas is None, using fallback", chain_id);
            (get_fallback_gas_price_gwei(chain_id), true)
        }
    }
}
```

### Step 2: Modify get_latest_gas_chain()

Replace lines 503-512 with:

```rust
            let raw_gwei = m.gas_price.map(|p| p as f64 / 1e9);
            let (validated_gwei, used_fallback) = validate_gas_price_gwei(raw_gwei, chain_id);

            let util = if m.gas_limit > 0 {
                Some((m.gas_used as f64 / m.gas_limit as f64) * 100.0)
            } else { None };
            Ok(Json(serde_json::json!({
                "chain_id":              m.chain_id,
                "block_number":          m.block_number,
                "timestamp":             m.timestamp,
                "base_fee_per_gas_wei":  m.gas_price,
                "gas_price_gwei":        validated_gwei,
                "gas_used":              m.gas_used,
                "gas_limit":             m.gas_limit,
                "utilization_pct":       util,
                "tx_count":              m.tx_count,
                "source":                "taifoon_header_collector",
                "used_fallback":         used_fallback,
                "raw_gas_price_gwei":    raw_gwei,
            })))
```

### Step 3: Modify get_latest_gas_all()

Replace lines 533-548 with:

```rust
    let entries: Vec<serde_json::Value> = all.iter().map(|m| {
        let raw_gwei = m.gas_price.map(|p| p as f64 / 1e9);
        let (validated_gwei, used_fallback) = validate_gas_price_gwei(raw_gwei, m.chain_id);

        let util = if m.gas_limit > 0 {
            Some((m.gas_used as f64 / m.gas_limit as f64) * 100.0)
        } else { None };
        serde_json::json!({
            "chain_id":              m.chain_id,
            "block_number":          m.block_number,
            "timestamp":             m.timestamp,
            "base_fee_per_gas_wei":  m.gas_price,
            "gas_price_gwei":        validated_gwei,
            "gas_used":              m.gas_used,
            "gas_limit":             m.gas_limit,
            "utilization_pct":       util,
            "tx_count":              m.tx_count,
            "source":                "taifoon_header_collector",
            "used_fallback":         used_fallback,
            "raw_gas_price_gwei":    raw_gwei,
        })
    }).collect();
```

---

## Testing Plan

### 1. Unit Tests (Before Deployment)

```bash
cd /Users/mbultra/projects/spinner/rust/crates/da-api
cargo test validate_gas_price
```

### 2. Integration Tests (Local)

```bash
# Check BSC gas price (should use fallback)
curl -s http://localhost:30081/api/gas/latest/56 | jq '{chain_id, gas_price_gwei, used_fallback}'

# Expected:
# {
#   "chain_id": 56,
#   "gas_price_gwei": 3.0,
#   "used_fallback": true
# }

# Check Optimism gas price (should use fallback)
curl -s http://localhost:30081/api/gas/latest/10 | jq '{chain_id, gas_price_gwei, used_fallback}'

# Expected:
# {
#   "chain_id": 10,
#   "gas_price_gwei": 0.001,
#   "used_fallback": true
# }
```

### 3. Production Tests (After Deployment)

```bash
# Check all chains
curl -s http://46.4.96.124:30081/api/gas/latest | jq '.data[] | select(.used_fallback == true) | {chain_id, gas_price_gwei, used_fallback}'

# Should show BSC, Optimism, and other problematic chains using fallbacks
```

### 4. Solver Validation

Re-run the solver on historical intents and verify profits are now reasonable:

```bash
cd /Users/mbultra/projects/taifoon-solver
./extract-intent-fixtures.sh

# Check unrealistic profits
jq '. | length' fixtures/unrealistic_profits.json
# Expected: 0 or much smaller than 9
```

---

## Deployment Workflow

### 1. Commit Changes (Local)

```bash
cd /Users/mbultra/projects/spinner
git add rust/crates/da-api/src/api.rs
git commit -m "fix(da-api): add gas price sanity checks with fallback values

- Add fallback gas prices for BSC (3 gwei), OP Stack (0.001 gwei), Arbitrum (0.01 gwei)
- Validate gas prices < 1e-6 or > 10000 gwei and use fallbacks
- Log warnings when fallbacks are used
- Add used_fallback and raw_gas_price_gwei fields to API response
- Prevents $51B profit calculations from zero gas prices

Fixes #<issue-number>"
```

### 2. Build and Deploy Spinner (Server)

```bash
ssh root@46.4.96.124

cd /root/spinner && git pull origin master
cd /root/spinner/rust && PATH=/root/.cargo/bin:$PATH cargo build --release --bin spinner

# Copy binary for Docker
cp /root/spinner/rust/target/release/spinner /tmp/spinner-binary
cd /tmp && docker build --no-cache -t spinner-monolith:latest -f Dockerfile.spinner-quick .
docker save spinner-monolith:latest | k3s ctr images import -

# Restart spinner pod
kubectl delete pod -n spinner spinner-0

# Verify
kubectl get pods -n spinner -l app=spinner
kubectl logs -n spinner spinner-0 -f | grep "⚠️"
```

### 3. Verify API Response

```bash
curl -s http://46.4.96.124:30081/api/gas/latest/56 | jq '.'
```

Expected output:
```json
{
  "chain_id": 56,
  "block_number": ...,
  "timestamp": ...,
  "base_fee_per_gas_wei": null,
  "gas_price_gwei": 3.0,
  "gas_used": ...,
  "gas_limit": ...,
  "utilization_pct": ...,
  "tx_count": ...,
  "source": "taifoon_header_collector",
  "used_fallback": true,
  "raw_gas_price_gwei": 0
}
```

---

## Success Criteria

✅ **BSC gas price** = 3.0 gwei (not 0!)
✅ **OP Stack gas prices** = 0.001 gwei (not 3.77e-07!)
✅ **Warnings logged** for all fallback usages
✅ **No intents with profit > $10** in solver
✅ **Solver still processes intents** (not broken by changes)

---

## Rollback Plan

If issues arise:

```bash
ssh root@46.4.96.124
cd /root/spinner && git checkout HEAD~1
cd /root/spinner/rust && PATH=/root/.cargo/bin:$PATH cargo build --release --bin spinner
# ... rebuild docker and restart pod ...
```

---

## Next Steps (After This Fix)

1. Monitor for 24-48 hours
2. Implement Phase 2: Live gas price fetcher
3. Create regression tests with unrealistic_profits.json
4. Enable autonomous fills on validated chains only

---

**Ready to implement:** Yes
**Requires code review:** Recommended
**Breaking changes:** No (adds new fields to API response)
**Estimated downtime:** ~30 seconds (pod restart)
