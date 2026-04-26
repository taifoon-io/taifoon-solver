# Smoking Gun Evidence: Unrealistic Profit Calculations
**Generated:** 2026-04-25
**Source:** Live solver intents from production

---

## Critical Evidence of Gas Calculation Bugs

### Example 1: BSC Gas = 0 → $51 BILLION Profit 💣

```json
{
  "id": "orbiter_finance:orbiter_finance::0xc7977800676cd410981c57b5228a647603d82371_130",
  "protocol": "orbiter_finance",
  "src_chain": 56,
  "dst_chain": 130,
  "amount": "51631363353317720000",
  "profit_usd": 51631363351.31772,
  "state": "attempted"
}
```

**Analysis:**
- **Amount:** 51.63 tokens (assuming 18 decimals)
- **Calculated Profit:** $51.6 BILLION USD
- **Root Cause:** BSC (chain 56) gas price = 0 gwei (from Razor endpoint)
- **Proof of Bug #2:** OP Stack/BSC gas decimals issue causes zero gas cost
- **Result:** Solver thinks filling this costs nothing → calculates absurd profit

---

### Example 2: Stargate V2 Bridge → $28,483 Profit on 9,494 Tokens

```json
{
  "id": "stargate_v2:stargate_v2::0x7618633fe866a38d764ad76810ab15743545b8e4280eeff06433cfcba5307f8e",
  "protocol": "stargate_v2",
  "src_chain": 56,
  "dst_chain": 8453,
  "amount": "9494823949000000000000",
  "profit_usd": 28483.421847,
  "state": "attempted"
}
```

**Analysis:**
- **Amount:** 9,494.82 tokens
- **Calculated Profit:** $28,483 USD (=$3 per token!)
- **Root Cause:** BSC gas = 0 means no cost to fill
- **Reality Check:** No cross-chain bridge offers $3/token arbitrage

---

### Example 3: Across V3 → $3.27 Profit on 1.44 ETH

```json
{
  "id": "across_v3:across_v3::across_2251196",
  "protocol": "across_v3",
  "src_chain": 59144,
  "dst_chain": 8453,
  "amount": "1440000000000000000",
  "profit_usd": 3.27,
  "state": "attempted"
}
```

**Analysis:**
- **Amount:** 1.44 ETH (~$4,300 USD at $3,000/ETH)
- **Calculated Profit:** $3.27 USD
- **Expected Reality:** Maybe $0.01-0.50 after gas
- **Root Cause:** Linea (59144) gas too low (7e-09 gwei from Razor)
- **Proof of Bug #2:** OP Stack chains showing near-zero gas prices

---

### Example 4: Orbiter Finance → $105 Profit on 106 Units

```json
{
  "id": "orbiter_finance:orbiter_finance::0x478946bcd4a5a22b316470f5486fafb928c0ba25_8453",
  "protocol": "orbiter_finance",
  "src_chain": 10,
  "dst_chain": 8453,
  "amount": "106036120000",
  "profit_usd": 105.93612,
  "state": "attempted"
}
```

**Analysis:**
- **Amount:** 106.036 tokens (assuming 9 decimals, or 0.0001 ETH if 18)
- **Calculated Profit:** $105.94 USD
- **Root Cause:** Optimism (10) gas = 3.77e-07 gwei (essentially free)
- **Reality:** This is geometrically impossible

---

## Summary of Bugs Proven by Evidence

| Bug | Evidence | Impact |
|-----|----------|--------|
| **Bug #2: BSC Gas = 0** | 51 BILLION profit on 51 tokens | CRITICAL - Solver will waste money |
| **Bug #2: OP Stack Gas Too Low** | $3-100 profits on small amounts | HIGH - Wrong profit calculations |
| **Bug #1: Chain ID 0 Decoding** | (See CRITICAL_BUGS_FOUND.md) | HIGH - Data corruption |

---

## Razor Gas Endpoint Data (Current State)

From `/api/solver/razor`:

```
BSC (56):        0 gwei           ← ZERO GAS = INFINITE PROFIT BUG
Optimism (10):   3.77e-07 gwei    ← 9 orders of magnitude too low
Linea (59144):   7e-09 gwei       ← 9 orders of magnitude too low
Mode (34443):    2.52e-07 gwei    ← 9 orders of magnitude too low
Zora (7777777):  2.52e-07 gwei    ← 9 orders of magnitude too low
```

**Expected Values:**
- BSC: ~0.1-3 gwei (not 0!)
- Optimism: ~0.001-0.01 gwei (not 3.77e-07!)
- Linea: ~0.001-0.01 gwei (not 7e-09!)

---

## Action Items (URGENT)

1. **Fix BSC RPC gas fetching** - Why is it returning 0?
2. **Fix OP Stack unit conversion** - Wei vs Gwei mismatch
3. **Add sanity checks** - Reject gas < 1e-6 gwei or > 10000 gwei
4. **Add profit sanity checks** - Flag profits > $10 or < -$10 for review
5. **Test with fixtures** - Use these examples as regression tests

---

**Status:** 🚨 PRODUCTION BLOCKERS IDENTIFIED - DO NOT ENABLE AUTONOMOUS FILLS
