# Critical Bugs Found in Taifoon Solver
**Date:** 2026-04-25
**Severity:** HIGH - Affects data integrity

---

## Bug #1: Chain ID 0 = Decoder Failure (NOT t3rn Lambda) 🚨

### Evidence
```json
{
  "protocol": "layerzero_v2",
  "src_chain": 10,
  "dst_chain": 0,  ← DECODER BUG
  "amount": "0",    ← DECODER BUG
  "profit_usd": -1.05,
  "state": "skipped"
}
```

### Root Cause
Protocol decoders are **failing to extract destination chain** and defaulting to `0` when decoding fails.

### Affected Protocols
- **LayerZero V2** - confirmed (multiple intents with dst_chain: 0)
- **t3rn Lambda** - likely (the "Lambda chain 0" was probably also a decoding failure)
- **Squid Router** - suspect (saw Fantom → 0 routes)

### Impact
- **Data integrity:** Unknown how many "real" chains are being misidentified as chain 0
- **Profitability:** Misidentified chains → wrong gas costs → wrong profit calculations
- **Autonomous delivery:** Cannot safely fill intents with dst_chain: 0

### Fix Location
Need to investigate protocol decoders in:
- `spinner/rust/crates/header-collector/` - protocol XML definitions
- `spinner/rust/crates/genome/` - event decoding logic

### Recommended Action
1. Add logging for decoder failures (don't silently default to 0)
2. Mark intents with dst_chain: 0 as "decoding_error" instead of "detected"
3. Review LayerZero V2 decoder - why is it failing?

---

## Bug #2: OP Stack Gas Prices (Decimals/Units Issue) 🚨

### Evidence
From Razor endpoint test:
```
Optimism (10):   0.00000041 gwei  ← 9 orders of magnitude too low
Linea (59144):   0.000000007 gwei ← 9 orders of magnitude too low
BSC (56):        0 gwei           ← Complete failure
```

### Expected Values
Optimism should be ~0.001-0.01 gwei (L2 is cheap, but not THAT cheap)

### Root Cause (Hypothesis)
**Decimals mismatch** - OP Stack chains may report gas in **wei** but Warmbed is treating it as **gwei**.

Conversion error:
- OP Stack reports: `413 wei`
- Warmbed treats as: `413 gwei`
- Then converts to gwei: `413 / 1e9 = 0.000000413 gwei`

**Correct flow should be:**
- OP Stack reports: `413 wei`
- Convert wei → gwei: `413 / 1e9 = 0.000000413 gwei` ✅ (if source is wei)
- OR OP Stack reports: `0.413 gwei` directly ✅ (if source is already gwei)

The fact that the numbers are ~1e-7 to 1e-9 suggests a **double conversion** or **wrong source unit assumption**.

### Fix Location
`spinner/rust/crates/da-api/src/api.rs` - gas endpoint (lines 489-556)

Check:
1. What unit does RPC return? (wei vs gwei)
2. What unit does Warmbed assume? (wei vs gwei)
3. Is there a double conversion happening?

### Recommended Action
1. Log raw RPC gas price responses for OP Stack chains
2. Compare with Ethereum (which works correctly)
3. Add chain-specific unit handling if needed

---

## Summary

| Bug | Severity | Impact | Status |
|-----|----------|--------|--------|
| Chain ID 0 decoding failure | 🔴 CRITICAL | Data corruption | Not fixed |
| OP Stack gas decimals | 🔴 CRITICAL | Wrong profit calculations | Not fixed |

**Both bugs must be fixed before autonomous delivery can be enabled.**

---

## Next Steps

1. **Immediate:** Add error logging for chain ID 0 intents
2. **Immediate:** Investigate LayerZero V2 decoder
3. **Immediate:** Check OP Stack gas unit assumptions
4. **Short-term:** Add decoder validation tests
5. **Short-term:** Add gas price sanity checks (reject values < 0.001 gwei or > 10000 gwei)

---

**Status:** 🚨 BLOCKERS IDENTIFIED - DO NOT DEPLOY TO PRODUCTION
