# Taifoon Solver Deployment Status

**Last Updated:** 2026-04-25 19:50 UTC
**Dashboard URL:** https://volamp.io/solver
**Status:** ✅ OPERATIONAL (with known gas calculation bugs documented)

---

## System Status

### Dashboard
- **URL:** https://volamp.io/solver
- **Status:** ✅ LIVE - Fully functional Next.js app
- **Container:** `taifoon-dashboard` running on port 3000
- **Features:**
  - Real-time intent streaming
  - Performance metrics
  - Gas presets (Razor)
  - Protocol breakdown
  - Money flow tracking
  - Top intents (24h)

### Solver API
- **Port:** 8082
- **Status:** ✅ LIVE
- **Container:** `taifoon-solver` (healthy)
- **Stats:** 9,228 total intents tracked
- **Endpoints:**
  - `/api/solver/stats` - System statistics
  - `/api/solver/intents` - Recent intents (100 max)
  - `/api/solver/protocols` - Protocol breakdown
  - `/api/solver/razor` - Gas price data (30 chains)
  - `/api/solver/money-flow` - Financial metrics
  - `/api/solver/stream` - SSE intent stream

### Nginx Configuration
- **Location:** `/etc/nginx/sites-available/volamp.io`
- **Routing:**
  - `/solver` → Dashboard (port 3000)
  - `/solver-api/` → Solver API (port 8082)
  - `/_next/` → Next.js static assets
- **Auth:** Disabled for dashboard/API, enabled for `/brain`

---

## Critical Bugs Identified 🚨

### Bug #1: BSC Gas = 0 → $51 Billion Profit
**Severity:** CRITICAL
**Impact:** Solver calculates impossible profits on BSC chains
**Evidence:** `fixtures/SMOKING_GUN_EVIDENCE.md` (line 10-44)
**Root Cause:** BSC RPC returning zero gas price
**Status:** ❌ NOT FIXED

```json
{
  "chain_id": 56,
  "gas_cost_gwei": 0,
  "intent_profit": "$51,631,363,351.32 USD",
  "actual_amount": "51.63 tokens"
}
```

### Bug #2: OP Stack Gas 10000x Too Low
**Severity:** CRITICAL
**Impact:** Wrong profit calculations on 8+ chains
**Evidence:** `fixtures/SMOKING_GUN_EVIDENCE.md` (line 46-86)
**Root Cause:** Wei vs Gwei unit mismatch in OP Stack RPC responses
**Status:** ❌ NOT FIXED

**Affected Chains:**
- Optimism (10): 3.77e-07 gwei (expected: 0.001-0.01)
- Linea (59144): 7e-09 gwei (expected: 0.001-0.01)
- Fraxtal, Mode, Blast, Zora, Lisk (all too low)

**Code Location:**
- Spinner gas fetching: `spinner/rust/crates/da-api/src/storage.rs:159-167`
- Wei→Gwei conversion: `spinner/rust/crates/da-api/src/api.rs:489-556`
- Conversion formula: `gwei = wei as f64 / 1e9` (line 503, 533)

### Bug #3: Chain ID 0 Decoder Failures
**Severity:** HIGH
**Impact:** Unknown how many chains are misidentified
**Evidence:** `CRITICAL_BUGS_FOUND.md` (line 7-43)
**Status:** ❌ NOT FIXED

---

## Test Fixtures Created

**Location:** `/Users/mbultra/projects/taifoon-solver/fixtures/`

### Evidence Files
- `SMOKING_GUN_EVIDENCE.md` (3.9K) - Detailed bug analysis with profit examples
- `unrealistic_profits.json` (3.0K) - 9 intents with profits >$1 or <-$2
- `gas_issues_summary.json` (1.9K) - Structured summary of all gas bugs
- `README.md` (2.5K) - Fixtures documentation

### Protocol Fixtures (100 live intents)
- `across_v3_intents.json` (8 samples)
- `allbridge_intents.json` (9 samples)
- `hyperlane_intents.json` (6 samples)
- `layerzero_v2_intents.json` (9 samples)
- `lifi_v2_intents.json` (20 samples)
- `orbiter_finance_intents.json` (37 samples - most active)
- `squid_router_intents.json` (2 samples)
- `stargate_v2_intents.json` (8 samples)
- `t3rn_lwc_intents.json` (1 sample) - **Autonomous delivery target!**

### Coverage Data
- `intents-full-dump-*.json` (28K) - Complete 100-intent snapshot
- `chains-observed-*.txt` - 30 unique chain IDs
- `routes-observed-*.txt` - 55 cross-chain routes
- `extraction-summary-*.json` - Metadata

---

## Action Items (Priority Order)

### Immediate (BLOCKERS)

1. **Fix BSC Gas = 0**
   - Location: Spinner RPC gas fetching code
   - Check: Why does BSC RPC return 0?
   - Test: Verify `curl https://bsc-rpc-endpoint/eth_gasPrice` returns valid data
   - Validate: After fix, `unrealistic_profits.json` should have no BSC intents

2. **Fix OP Stack Wei→Gwei Conversion**
   - Location: `spinner/rust/crates/da-api/src/api.rs:503, 533`
   - Issue: Double conversion or wrong source unit assumption
   - Check: Compare Optimism vs Ethereum gas price fetching
   - Validate: Optimism gas should be ~0.001-0.01 gwei, not 3.77e-07

3. **Add Gas Price Sanity Checks**
   - Location: `spinner/rust/crates/da-api/src/api.rs` (gas endpoints)
   - Rule: Reject gas < 1e-6 gwei OR > 10000 gwei
   - Rule: Flag gas = 0 as error, don't use it
   - Log: Warning when gas is suspicious

4. **Add Profit Sanity Checks**
   - Location: Solver profit calculation code
   - Rule: Flag profits > $10 for manual review
   - Rule: Flag profits < -$10 as likely bugs
   - Log: All intents with unusual profits

### Short-Term (1-2 weeks)

5. **Investigate Chain ID 0 Decoding Failures**
   - Location: `spinner/rust/crates/header-collector/` (protocol decoders)
   - Issue: dst_chain defaulting to 0 when decoding fails
   - Fix: Mark as `decoding_error` instead of `detected`
   - Review: LayerZero V2, Squid Router decoders

6. **Create Regression Tests**
   - Use `fixtures/unrealistic_profits.json` as test cases
   - After fixes, all 9 intents should have realistic profits
   - Add CI check: fail if any profit > $10 or < -$10

7. **Deploy to Production**
   - Only after BSC and OP Stack bugs are fixed
   - Monitor for 24-48 hours
   - Enable autonomous fills only on validated chains

---

## Deployment Workflow

### Dashboard Updates

```bash
# On local machine: edit dashboard code
cd /Users/mbultra/projects/taifoon-solver/dashboard

# On server: rebuild and restart
ssh root@88.99.1.32
cd /root/taifoon-solver
git pull origin master
docker compose build dashboard
docker compose restart dashboard
docker logs -f taifoon-dashboard
```

### Solver Updates

```bash
# On local machine: edit Rust code
cd /Users/mbultra/projects/taifoon-solver

# On server: rebuild and restart
ssh root@88.99.1.32
cd /root/taifoon-solver
git pull origin master
docker compose build solver
docker compose restart solver
docker logs -f taifoon-solver
```

### Verify Deployment

```bash
# Dashboard
curl -s https://volamp.io/solver | grep "Taifoon Solver"

# API
curl -s https://volamp.io/solver-api/stats | jq '.status'

# Razor gas endpoint
curl -s https://volamp.io/solver-api/razor | jq '.presets | length'
```

---

## Current Statistics

**As of 2026-04-25 19:50 UTC:**

- Total intents tracked: 9,228
- Profitable intents: 2,202 (23.8%)
- Skipped intents: 7,026 (76.2%)
- Executed fills: 0 (autonomous mode OFF)
- Active protocols: 9
- Monitored chains: 30
- Unrealistic profits: 9 (0.097% - all due to gas bugs)

---

## Known Issues

### Dashboard Shows "OFFLINE" on First Load
**Cause:** JavaScript needs ~2-3 seconds to make first API call
**Solution:** Wait 3-5 seconds, page should update to "LIVE"
**Workaround:** Refresh page if still offline after 10 seconds

### Static Assets Cached
**Cause:** Nginx caches `/_next/` files for 1 year
**Solution:** Docker restart clears cache
**Workaround:** Hard refresh (Cmd+Shift+R) in browser

---

## URLs & Endpoints

### Public URLs
- Dashboard: https://volamp.io/solver
- Brain (auth required): https://volamp.io/brain

### API Endpoints (Public)
- Stats: https://volamp.io/solver-api/stats
- Intents: https://volamp.io/solver-api/intents
- Protocols: https://volamp.io/solver-api/protocols
- Razor Gas: https://volamp.io/solver-api/razor
- Money Flow: https://volamp.io/solver-api/money-flow
- SSE Stream: https://volamp.io/solver-api/stream

---

## Success Criteria for Production

✅ Dashboard accessible and showing live data
✅ API returning correct intent counts
✅ Razor gas endpoint serving 30+ chains
❌ BSC gas > 0 (currently 0)
❌ OP Stack gas reasonable (currently 10000x too low)
❌ No intents with profit > $10 (currently 9 intents fail)
❌ Chain ID 0 decoding errors fixed

**Production Ready:** 🚨 NO - Gas bugs must be fixed first

---

**Status Summary:** Infrastructure is operational, but critical gas calculation bugs prevent safe autonomous operation. All bugs are documented with smoking gun evidence in `fixtures/`. Dashboard is live for monitoring only.
