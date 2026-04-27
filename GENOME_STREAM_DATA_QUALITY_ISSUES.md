# Genome Stream Data Quality Issues - Critical Findings

**Date**: 2026-04-27
**Status**: 🚨 MULTIPLE CRITICAL BUGS DISCOVERED

---

## Executive Summary

**Discovery**: The Li.Fi `reference` (tx_hash) bug is **NOT an isolated issue**.

**MASSIVE DATA QUALITY PROBLEM** affecting multiple protocols:

1. **Missing `input_amount`** - Affects MANY protocols (high-frequency failures)
2. **Missing `src_token`** - Affects some protocols (moderate frequency)
3. **Missing `reference`** - Li.Fi (FIXED in commit 55ec52c)

**Impact**: **Unknown percentage of ALL intents being silently dropped** across multiple protocols.

---

## Evidence from Live Logs

**Observation Period**: 47 minutes (09:44 - 10:31 UTC)

**Failure Count**: 200+ intent parsing failures

**Failure Types**:
- `"Missing input_amount in genome event"` - ~190 failures (~95%)
- `"Missing src_token in genome event"` - ~15 failures (~7.5%)

---

## Critical Findings

### 1. Missing `input_amount` - SEVERE ISSUE

**Frequency**: ~4 failures per minute (very high)

**Impact**: Intents cannot be processed without amount - **100% drop rate** for affected events

**Code Location**: `/Users/mbultra/projects/taifoon-solver/crates/genome-client/src/lib.rs:86`

**Current Code** (BUGGY):
```rust
let amount = event.input_amount.clone().context("Missing input_amount in genome event")?;
```

**Problem**: `.context()?` fails fast when `input_amount` is null/missing

**Which protocols affected?** UNKNOWN - need to correlate logs with protocol names

---

### 2. Missing `src_token` - MODERATE ISSUE

**Frequency**: ~0.3 failures per minute (moderate)

**Impact**: Cannot determine token type - **100% drop rate** for affected events

**Code Location**: `/Users/mbultra/projects/taifoon-solver/crates/genome-client/src/lib.rs:122`

**Current Code** (BUGGY):
```rust
let src_token = event.src_token.context("Missing src_token in genome event")?;
```

**Problem**: `.context()?` fails fast when `src_token` is null/missing

---

### 3. Missing `reference` (tx_hash) - Li.Fi ONLY

**Frequency**: Unknown (Li.Fi specific)

**Impact**: **100% drop rate for Li.Fi intents**

**Status**: ✅ **FIXED** in commit 55ec52c with synthetic hash generation

---

## Root Cause Analysis

**Why is this happening?**

### Hypothesis 1: Spinner Genome API Data Incompleteness

**Theory**: Genome stream is emitting events with NULL/missing required fields

**Evidence**:
- Consistent pattern of missing fields across multiple protocols
- Different protocols missing different fields
- High frequency (~4/min) suggests systematic issue

**Likely cause**:
- Spinner genome API not validating required fields before emitting events
- Different protocol collectors providing incomplete data
- Schema migration incomplete (old events with old field names)

### Hypothesis 2: Field Name Changes

**Theory**: Genome API changed field names but solver still uses old names

**Evidence**:
- Li.Fi fix shows we're using `input_amount` (new) vs `amount` (old)
- Code has fallback logic: `event.input_amount.clone().context(...)?`
- But other fields don't have fallbacks

**Example from code**:
```rust
// Support both input_amount (new) and amount (old)
let amount = event.input_amount.clone().context("Missing input_amount in genome event")?;
```

**But this is STILL using `.context()?` which fails!**

---

## Affected Protocols (Unknown)

**Problem**: Logs show errors but NOT which protocols are affected

**Log Format**:
```
🎯 New intent detected: orbiter_finance (252 → 250)
Failed to convert genome event to intent: Missing input_amount in genome event
```

**Need**: Correlation analysis to determine which protocols have which missing fields

---

## Proposed Fix Strategy

### Option 1: Make ALL Fields Optional (SAFEST)

```rust
// Make fields optional in GenomeEvent struct
pub struct GenomeEvent {
    pub input_amount: Option<String>,  // Currently required
    pub src_token: Option<String>,      // Currently required
    pub reference: Option<String>,      // Currently optional (Li.Fi fix)
    // ... other fields
}

// Then in Intent::from_genome_event():
let amount = event.input_amount
    .clone()
    .or_else(|| event.amount.clone())  // Fallback to old field name
    .unwrap_or_else(|| {
        warn!("⚠️  Missing amount in genome event, using 0");
        "0".to_string()  // Or skip intent entirely
    });
```

**Pros**:
- Prevents ALL intent drops from missing fields
- Graceful degradation
- Can log warnings for visibility

**Cons**:
- May process incomplete intents
- Requires validation logic throughout codebase

### Option 2: Enhanced Fallback Logic (RECOMMENDED)

Similar to Li.Fi fix but for ALL missing fields:

```rust
// For input_amount:
let amount = event.input_amount
    .clone()
    .or_else(|| event.amount.clone())  // Try old field name
    .context("Missing both input_amount and amount in genome event")?;

// For src_token:
let src_token = event.src_token
    .clone()
    .or_else(|| event.token.clone())  // Try old field name
    .or_else(|| event.src_token_address.clone())  // Try alternate name
    .unwrap_or_else(|| {
        warn!("⚠️  Missing src_token, inferring from chain native token");
        "0x0000000000000000000000000000000000000000".to_string()  // Assume ETH
    });
```

**Pros**:
- Handles field name migrations
- Provides fallback defaults
- Maintains intent processing rate

**Cons**:
- Assumptions may be wrong
- Complexity in fallback logic

### Option 3: Fix Spinner Genome API (IDEAL)

**Action**: Update Spinner to emit complete events with ALL required fields

**Pros**:
- Fixes root cause
- Benefits all consumers of genome stream
- Clean solution

**Cons**:
- Requires Spinner code changes
- Deployment coordination needed
- May take time

---

## Immediate Action Plan

1. ✅ **Li.Fi tx_hash fix deployed** (commit 55ec52c)

2. **URGENT: Analyze missing `input_amount` and `src_token` by protocol**
   - Add protocol name to error logs
   - Determine which protocols are affected
   - Calculate intent drop rate per protocol

3. **Implement fallback logic** for missing fields:
   - `input_amount`: Try old `amount` field
   - `src_token`: Try old `token` field, infer from chain

4. **Add monitoring**:
   - Count intent drops by protocol
   - Count intent drops by missing field
   - Alert on high drop rates

5. **Long-term: Fix Spinner genome stream**
   - Validate all required fields before emitting
   - Ensure schema consistency across protocols
   - Add integration tests

---

## Testing Plan

1. **Live Log Analysis**:
   - Capture 1000 genome events
   - Identify protocols with missing fields
   - Calculate drop rate by protocol

2. **Synthetic Testing**:
   - Create test events with missing fields
   - Verify fallback logic works
   - Ensure no crashes on incomplete data

3. **Production Validation**:
   - Deploy fix
   - Monitor intent drop rate (should decrease significantly)
   - Compare before/after metrics

---

## Metrics to Track

**Before Fix**:
- Intent drop rate: **UNKNOWN%** (massive)
- Errors per minute: ~4 (just for input_amount)
- Affected protocols: UNKNOWN

**After Fix** (Expected):
- Intent drop rate: <1%
- Errors per minute: 0
- All protocols processing correctly

---

## Related Issues

- ✅ Gas calculation bug (1000× overestimation) - FIXED
- ✅ u128 overflow handling - FIXED
- ✅ Li.Fi tx_hash missing - FIXED
- 🚨 **Missing `input_amount`** - **THIS ISSUE (NEW!)**
- 🚨 **Missing `src_token`** - **THIS ISSUE (NEW!)**

---

## Files to Investigate

1. `/Users/mbultra/projects/taifoon-solver/crates/genome-client/src/lib.rs`
   - Lines 86-149: Intent::from_genome_event()
   - Add fallback logic for missing fields

2. Spinner Genome API (separate repo)
   - Investigate why fields are missing
   - Add validation before emitting events

---

**Status**: 🚨 **CRITICAL - REQUIRES IMMEDIATE ACTION**

**Estimated Impact**: Potentially **50%+ of ALL intents being dropped** due to missing field issues across multiple protocols.

**Next Steps**:
1. Add protocol name to error logs to identify affected protocols
2. Implement fallback logic for `input_amount` and `src_token`
3. Deploy and monitor
4. Coordinate with Spinner team to fix root cause
