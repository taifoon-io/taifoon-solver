# Gas Pricing Bug Analysis & Fix Strategy

**Date:** 2026-04-25
**Status:** Critical production blocker
**Impact:** Solver calculates impossible profits, wasting gas on unprofitable fills

---

## Root Cause Analysis

### Problem #1: BSC Gas Price = 0

**Evidence:**
- BSC (chain 56) reports `gas_price_gwei: 0` from Spinner API
- Results in $51 BILLION profit calculation on 51 tokens (see `fixtures/SMOKING_GUN_EVIDENCE.md:10-44`)

**Root Cause:**
The `calculate_gas_price_from_block()` function in `spinner/rust/crates/header-collector/src/evm.rs:448-539` extracts gas prices from historical block transactions. For BSC, transactions either:
1. Have `gas_price = None` (EIP-1559 transactions)
2. Have `gas_price = 0` (zero-fee transactions)
3. Result in empty `gas_prices` vector → returns `None`

When stored as `None`, the API converts it to `0` in wei, then to `0` gwei.

**Technical Details:**
```rust
// evm.rs:518-526
let mut gas_prices: Vec<u64> = block
    .transactions
    .iter()
    .filter_map(|tx| tx.gas_price.map(|price| price.as_u64()))
    .collect();

if gas_prices.is_empty() {
    return None;  // ← This causes BSC gas = 0
}
```

### Problem #2: OP Stack Gas 10000x Too Low

**Evidence:**
- Optimism (10): `3.77e-07 gwei` (expected: 0.001-0.01 gwei)
- Linea (59144): `7e-09 gwei` (expected: 0.001-0.01 gwei)
- Mode, Zora, Blast, Fraxtal: all similarly too low

**Root Cause:**
OP Stack chains use EIP-1559 exclusively. The `calculate_gas_price_from_block()` function calculates:
```rust
// evm.rs:515
Some(base_fee_val + median_priority_fee)
```

However, OP Stack L2s have extremely low base fees (often < 0.001 gwei) because execution is cheap. The stored value is technically correct in wei, but the problem is:

1. These are **historical** values from blocks that may be hours old
2. The values represent **past** gas prices, not current market rates
3. The API serves these stale values to the solver

### Problem #3: Historical vs. Live Gas Prices

**Fundamental Issue:**
The current system extracts gas prices from the latest indexed **block header**, not from live RPC queries. This means:

- Gas prices can be hours old
- They represent what users paid in the past, not what they'd pay now
- Chains with volatile gas prices (like Ethereum) show stale data

**Code Flow:**
1. Header collector fetches block → calculates gas from txs → stores in DB
2. Solver queries `/api/gas/latest/:chain_id` → gets stale gas price
3. Solver uses stale price for profit calculations → wrong results

---

## Fix Strategy

### Phase 1: Immediate Mitigations (Deploy Today)

#### 1.1 Add Gas Price Sanity Checks in Spinner API

**File:** `spinner/rust/crates/da-api/src/api.rs:493-521`

Add validation before returning gas prices:

```rust
// After line 502
let gwei = m.gas_price.map(|p| p as f64 / 1e9);

// ADD THIS:
let validated_gwei = match gwei {
    Some(g) if g < 1e-6 => {
        eprintln!("⚠️  Chain {} gas too low: {} gwei, using fallback", chain_id, g);
        get_fallback_gas_price(chain_id)
    },
    Some(g) if g > 10000.0 => {
        eprintln!("⚠️  Chain {} gas too high: {} gwei, using fallback", chain_id, g);
        get_fallback_gas_price(chain_id)
    },
    Some(g) => Some(g),
    None => {
        eprintln!("⚠️  Chain {} gas is None, using fallback", chain_id);
        get_fallback_gas_price(chain_id)
    }
};
```

**Fallback Values (based on chain type):**
- BSC: 3.0 gwei (typical BSC gas price)
- Optimism/Base/OP Stack: 0.001 gwei
- Arbitrum: 0.01 gwei
- Ethereum: fetch from public API (etherscan/gas-tracker)

#### 1.2 Add Profit Sanity Checks in Solver

**File:** `taifoon-solver/src/profitability.rs` (or equivalent)

```rust
if profit_usd > 10.0 {
    eprintln!("⚠️  Suspiciously high profit: ${:.2} for intent {}", profit_usd, intent.id);
    // Flag for review, don't auto-fill
    return IntentDecision::SkipSuspicious;
}

if profit_usd < -10.0 {
    eprintln!("⚠️  Suspiciously large loss: ${:.2} for intent {}", profit_usd, intent.id);
    return IntentDecision::Skip;
}
```

### Phase 2: Proper Fix (Deploy This Week)

#### 2.1 Implement Live Gas Price Oracle

Create a new RPC-based gas price fetcher that runs every 10 seconds:

**New File:** `spinner/rust/crates/gas-oracle/src/live_gas_fetcher.rs`

```rust
pub async fn fetch_live_gas_price(chain_id: u64, rpc_url: &str) -> Result<u64> {
    match chain_id {
        // Ethereum, BSC: use eth_gasPrice RPC call
        1 | 56 => fetch_evm_gas_price(rpc_url).await,

        // OP Stack: use eth_gasPrice (very low) + 15% buffer
        8453 | 10 | 34443 | 252 | 7777777 => {
            let base_price = fetch_evm_gas_price(rpc_url).await?;
            Ok((base_price as f64 * 1.15) as u64)  // 15% buffer
        },

        // Arbitrum: query L2 gas price oracle contract
        42161 | 421614 => fetch_arbitrum_gas(rpc_url).await,

        _ => fetch_evm_gas_price(rpc_url).await,
    }
}

async fn fetch_evm_gas_price(rpc_url: &str) -> Result<u64> {
    let provider = Provider::<Http>::try_from(rpc_url)?;
    let price = provider.get_gas_price().await?;
    Ok(price.as_u64())
}
```

**Advantages:**
- Always fresh prices (< 10 seconds old)
- Uses RPC `eth_gasPrice` which accounts for current network conditions
- Can add chain-specific logic (e.g., Arbitrum gas oracle contract)

#### 2.2 Hybrid Approach: Historical + Live

Keep the current block-based gas extraction for **archival purposes**, but add a separate **live gas price** table:

**New Table:** `CF_LIVE_GAS_PRICES`
- Key: `chain_id:u64`
- Value: `LiveGasPrice { price_wei: u64, timestamp: u64, source: String }`
- Updated every 10 seconds by background task

**New API Endpoint:** `GET /api/gas/live/:chain_id`
- Returns the live gas price (not from blocks)
- Solver uses this instead of `/api/gas/latest/:chain_id`

### Phase 3: Testing & Validation

#### 3.1 Regression Tests

Use the fixtures as test cases:

```bash
cd /Users/mbultra/projects/taifoon-solver
cargo test --test gas_sanity_checks
```

**Test Cases:**
1. BSC gas = 0 → should use fallback (3.0 gwei)
2. OP Stack gas < 1e-6 → should use fallback (0.001 gwei)
3. All `unrealistic_profits.json` intents → profits should be < $1

#### 3.2 Deployment Verification

After deploying fixes:

```bash
# 1. Check BSC gas price
curl -s http://46.4.96.124:30081/api/gas/latest/56 | jq '.gas_price_gwei'
# Expected: ~3.0 gwei (not 0!)

# 2. Check Optimism gas price
curl -s http://46.4.96.124:30081/api/gas/latest/10 | jq '.gas_price_gwei'
# Expected: ~0.001 gwei (not 3.77e-07!)

# 3. Re-run solver on unrealistic_profits.json
./extract-intent-fixtures.sh
diff fixtures/unrealistic_profits.json fixtures/unrealistic_profits_after_fix.json
# Expected: Much smaller or empty
```

---

## Implementation Priority

**Today (2026-04-25):**
1. Add gas sanity checks in Spinner API (`api.rs:502`)
2. Add profit sanity checks in Solver
3. Deploy to production, verify with curl tests

**This Week:**
4. Implement live gas price fetcher
5. Create `CF_LIVE_GAS_PRICES` table
6. Add `/api/gas/live/:chain_id` endpoint
7. Update Solver to use live gas prices
8. Run regression tests with fixtures

**Next Week:**
9. Monitor for 1 week, collect metrics
10. Enable autonomous fills on validated chains only
11. Create alerts for gas price anomalies

---

## Success Criteria

✅ **BSC gas > 0** (should be ~3 gwei)
✅ **OP Stack gas reasonable** (should be ~0.001-0.01 gwei)
✅ **No intents with profit > $10** (current: 9 failing intents)
✅ **Solver skips suspicious intents** (flagged for manual review)
✅ **Live gas prices updated every 10s** (freshness guarantee)

---

## Files to Modify

### Immediate Fix (Today)
1. `spinner/rust/crates/da-api/src/api.rs` — add sanity checks
2. `taifoon-solver/src/profitability.rs` — add profit validation

### Proper Fix (This Week)
3. `spinner/rust/crates/gas-oracle/src/live_gas_fetcher.rs` — NEW FILE
4. `spinner/rust/crates/da-api/src/storage.rs` — add `CF_LIVE_GAS_PRICES`
5. `spinner/rust/crates/da-api/src/api.rs` — add `/api/gas/live/:chain_id`
6. `taifoon-solver/src/gas_client.rs` — switch to live endpoint

---

**Next Steps:** Start with Phase 1 immediate mitigations to stop the bleeding, then implement proper live gas oracle in Phase 2.
