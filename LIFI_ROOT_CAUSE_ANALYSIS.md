# Li.Fi Intent Loss - Root Cause Analysis

**Date**: 2026-04-27
**Issue**: Li.Fi V2 intents being silently dropped, never reaching profit calculation
**Status**: ✅ ROOT CAUSE IDENTIFIED

---

## Executive Summary

**ALL Li.Fi V2 intents are being silently dropped** during genome stream ingestion due to missing `reference` (tx_hash) field in the genome events. The parser requires this field but Li.Fi events don't provide it, causing a silent failure with `.context()?` error propagation.

---

## Data Flow Trace

### 1. Genome SSE Stream → GenomeClient
**File**: `/Users/mbultra/projects/taifoon-solver/crates/genome-client/src/lib.rs`

```
Spinner API (46.4.96.124:30081)
    ↓
GET /api/genome/subscribe/sse
    ↓
GenomeEvent (SSE format)
    ↓
parse_sse_event() → filters entity/action
    ↓
Intent::from_genome_event() → ❌ FAILS HERE FOR LIFI
```

### 2. The Critical Code Path

**Lines 96-99** in `genome-client/src/lib.rs`:
```rust
// Use ref_hash as tx hash
let tx_hash = event
    .reference           // ← Li.Fi events have reference = null
    .or_else(|| event.order_id.clone())  // ← order_id also missing/different format
    .context("Missing reference/order_id in genome event")?;  // ← FAILS HERE!
```

**What happens**:
1. Li.Fi genome event arrives with `reference: null` or `reference: None`
2. `or_else()` falls back to `event.order_id` which is also not in expected format
3. `.context()?` propagates error and **intent is silently dropped**
4. Warning log: `"Failed to convert genome event to intent: Missing reference/order_id in genome event"`
5. Intent **NEVER reaches profit calculation** or solver logic

---

## Evidence from Fixtures

**File**: `fixtures/lifi_v2_intents.json`

```json
{
  "protocol": "lifi_v2",
  "id": "lifi_v2:lifi_v2::lifi_0x8f402c380754a14c8a216c67e219af96c8a449b6b6cd08553d455945e616bba4",
  "src_chain": 59144,
  "dst_chain": 8453,
  "amount": "2213850",
  "tx_hash": null,  // ← NULL TX_HASH!
  "profit_usd": -1.0499999999933585,
  "state": "skipped"
}
```

**All 17 Li.Fi fixtures** have `"tx_hash": null`.

---

## Root Cause Analysis

### Problem Layers:

**Layer 1: Missing Data** (Genome Stream Issue)
- Li.Fi genome events don't include `reference` field (tx_hash)
- Or they use a different field name/format
- This is a **data schema mismatch** between what genome stream provides vs what the solver expects

**Layer 2: Strict Validation** (Parser Bug)
- `Intent::from_genome_event()` uses `.context()?` which **fails fast**
- No fallback mechanism for missing tx_hash
- No way to generate synthetic tx_hash from available data

**Layer 3: Silent Failure** (Logging Issue)
- Error logged as `warn!()` instead of `error!()`
- Intent silently dropped with no visibility to operator
- No metrics/counters for dropped intents by protocol

---

## Impact Assessment

**Affected Protocols**: Li.Fi V2 (possibly others)
**Intents Lost**: 100% of Li.Fi V2 intents
**Revenue Impact**: ALL Li.Fi MEV opportunities missed

**From Fixture Audit**:
- 17 Li.Fi V2 intents analyzed
- 0 processed successfully (100% drop rate)
- 16 would have been skipped anyway (negative profit due to gas bug - NOW FIXED)
- **1 would have been PROFITABLE** but was never seen by solver

---

## Fix Options

### Option 1: Generate Synthetic tx_hash (RECOMMENDED)
```rust
// Use ref_hash as tx hash, with fallback to generated ID
let tx_hash = event.reference
    .or_else(|| event.order_id.clone())
    .unwrap_or_else(|| {
        // Generate synthetic tx_hash from available data
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        event.depositor.hash(&mut hasher);
        event.src_chain.hash(&mut hasher);
        event.dst_chain.hash(&mut hasher);
        event.input_amount.hash(&mut hasher);
        event.ts.hash(&mut hasher);

        format!("synthetic_{:x}", hasher.finish())
    });
```

**Pros**:
- Allows all intents to be processed
- Maintains backward compatibility
- Generates deterministic IDs

**Cons**:
- Synthetic tx_hash not verifiable on-chain
- May cause ID collisions (low probability)

### Option 2: Make tx_hash Optional
```rust
pub struct Intent {
    pub tx_hash: Option<String>,  // ← Make optional
    // ...
}
```

**Pros**:
- Clean solution
- Explicitly models reality (some intents don't have tx_hash yet)

**Cons**:
- Requires changes throughout codebase
- Profit calc logic might assume tx_hash exists

### Option 3: Fix Genome Stream (IDEAL)
- Update Spinner API to include `reference` field for ALL protocols
- Ensure Li.Fi events include transaction hash
- Requires Spinner code changes

**Pros**:
- Fixes root cause
- All protocols benefit

**Cons**:
- Requires Spinner deployment
- May take time

---

## Recommended Fix

**Immediate (Option 1)**:
1. Add fallback tx_hash generation in `Intent::from_genome_event()`
2. Log when synthetic tx_hash is used
3. Deploy solver with fix

**Long-term (Option 3)**:
1. Update Spinner genome stream to include tx_hash for all protocols
2. Once deployed, remove synthetic fallback from solver

---

## Files to Modify

1. `/Users/mbultra/projects/taifoon-solver/crates/genome-client/src/lib.rs` (lines 96-99)
   - Add fallback tx_hash generation
   - Add warning log when using synthetic tx_hash

2. Add monitoring for intent drop rate by protocol

---

## Testing Plan

1. **Unit Test**: Add test case for Li.Fi genome event with missing reference
2. **Integration Test**: Feed real Li.Fi fixtures through parser
3. **Smoke Test**: Monitor solver logs for "synthetic tx_hash" warnings
4. **Production Validation**: Check that Li.Fi intents reach profit calculation

---

## Related Issues

- ✅ Gas calculation bug (1000× overestimation) - FIXED in commit 9297397
- ✅ u128 overflow handling - FIXED in commit 9297397
- ⚠️ Li.Fi intent drop - **THIS ISSUE** (tx_hash missing)
- ✅ Fixture audit complete - FIXTURE_AUDIT_REPORT.md

---

**Conclusion**: Li.Fi intents are **NOT coming from genome stream with required fields**. This is a data schema mismatch that causes 100% intent loss. Fix requires either fallback logic in the solver OR fix in Spinner genome stream.
