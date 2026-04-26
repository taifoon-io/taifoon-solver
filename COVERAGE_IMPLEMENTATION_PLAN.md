# Taifoon Solver - Full Coverage Implementation Plan
**Generated:** 2026-04-25
**Objective:** Extend Razor gas coverage and protocol support using fixture-first approach

---

## Executive Summary

### Current State (as of 2026-04-25 10:41 UTC)
- **Solver Status:** ✅ Running with 100 live intents
- **Warmbed Gas API:** 🟡 Spinner pod restarting (0/1 Ready) — was collecting: 252, 1284, 59144, 100, 42161, 400
- **Protocol Coverage:** 9/31 protocols active with live data
- **Cross-VM Bridging:** 24+ EVM ↔ Solana routes detected!
- **Critical Discovery:** Solana (200) has 20+ live intents — **highest priority**

### Architecture Understanding

```
┌─────────────────────────────────────────────────────────────────┐
│                     WARMBED GAS ORACLE FLOW                      │
└─────────────────────────────────────────────────────────────────┘

 RPC Endpoints
      ↓
 HeaderCollector (Spinner)
      ↓
 store_header() → CF_GAS_METRICS (RocksDB)
      ↓
 GET /api/gas/latest/:chain_id (DA API)
      ↓
 Solver API: fetch_razor_for_chain()
      ↓
 Dashboard: Razor Gas Panel


📊 Data Flow:
- Source: Live RPC endpoints (configured per-chain)
- Storage: /opt/spinner/spinner-data/CF_GAS_METRICS
- API: http://46.4.96.124:30081/api/gas/latest
- Solver: http://localhost:8082/api/solver/razor
```

---

## Phase 1: Infrastructure Validation (IMMEDIATE)

### 1.1 Wait for Spinner Pod Readiness
```bash
# Monitor pod until 1/1 Ready
ssh root@46.4.96.124 "kubectl get pods -n spinner -l app=spinner -w"

# Test gas API endpoint
curl -s "http://46.4.96.124:30081/api/gas/latest" | jq '.chains'
```

**Expected:** Returns gas data for all chains Spinner is currently tracking
**Blocker:** Without this, Razor endpoint will continue to fail

### 1.2 Validate Production API Nginx Routing
```bash
# Check nginx config for /api/gas/* routing
ssh root@46.4.96.124 "grep -A 5 '/api/gas' /etc/nginx/sites-enabled/*"

# Test through public API
curl -s "https://api.taifoon.dev/api/gas/latest/1" | jq '.'
```

**Issue:** Currently returning 502 Bad Gateway
**Fix:** Verify nginx proxy_pass points to http://localhost:30081

---

## Phase 2: Identify Current Coverage (FIXTURE-FIRST)

### 2.1 Query Live Gas Data from Spinner
```bash
# Get all chains with gas data
curl -s "http://46.4.96.124:30081/api/gas/latest" | \
  jq '.data[] | {chain_id, block_number, gas_price_gwei}'

# Check specific chains from live intents
for chain_id in 1 10 56 100 137 200 252 324 534352 8453 42161 59144; do
  echo "Chain $chain_id:"
  curl -s "http://46.4.96.124:30081/api/gas/latest/$chain_id" | jq '.gas_price_gwei // "MISSING"'
done
```

**Goal:** Create baseline of what's already working vs. what needs to be added

### 2.2 Analyze 100 Live Intents
```bash
# Get all intents and extract unique chain IDs
curl -s "http://localhost:8082/api/solver/intents" | \
  jq -r '.intents[] | "\(.src_chain) \(.dst_chain)"' | \
  tr ' ' '\n' | sort -nu

# Group intents by protocol
curl -s "http://localhost:8082/api/solver/intents" | \
  jq -r '.intents[] | .protocol' | sort | uniq -c | sort -rn
```

**Expected Output:**
- List of all chain IDs with live intent activity
- Protocol distribution (e.g., Stargate V2: 15, LI.FI: 12, Mayan: 8...)

---

## Phase 3: Add Missing Chains to Spinner (CRITICAL)

### Priority 1: Solana (Chain 200) — URGENT
**Reason:** 20+ live intents detected in coverage.xml

```rust
// File: /Users/mbultra/projects/spinner/deployments/parachain-registry.json
// Add Solana configuration:

{
  "chain_id": 200,
  "name": "Solana Mainnet",
  "rpc_endpoints": [
    "https://api.mainnet-beta.solana.com",
    "https://solana-api.projectserum.com"
  ],
  "chain_type": "Solana",
  "finality_type": "SOL_TOWER_BFT",
  "finality_blocks": 31
}
```

**Verification:**
```bash
# After adding, check Spinner logs
ssh root@46.4.96.124 "kubectl logs -n spinner spinner-0 --tail=100 | grep 'Chain 200'"

# Test gas endpoint
curl -s "http://46.4.96.124:30081/api/gas/latest/200" | jq '.'
```

### Priority 2: BSC (56), Avalanche (43114), Linea (59144)
**Reason:** Live intents detected in coverage.xml

| Chain ID | Name | RPC Endpoint | Status |
|----------|------|--------------|--------|
| 56 | BSC | https://bsc-dataseed1.binance.org | ✅ Has fallback in warmbed_rpc_resolver.rs:199 |
| 43114 | Avalanche C-Chain | https://avalanche-c-chain-rpc.publicnode.com | ✅ Has fallback in warmbed_rpc_resolver.rs:203 |
| 59144 | Linea | https://rpc.linea.build | ✅ ALREADY COLLECTING (seen in logs!) |

**Action:** Verify BSC and Avalanche are configured in parachain-registry.json

### Priority 3: Resolve Unknown Chain IDs
From coverage.xml, these chains appeared with intent activity but are not in fallback_rpc_url():

| Chain ID | Possible Identity | Next Step |
|----------|-------------------|-----------|
| 130 | Unichain (confirmed in warmbed_rpc_resolver.rs:216) | ✅ Already configured |
| 252 | Frax (confirmed in warmbed_rpc_resolver.rs:215) | ✅ ALREADY COLLECTING (seen in logs!) |
| 999 | HyperLiquid EVM (warmbed_rpc_resolver.rs:234) | ✅ Already configured |
| 1689 | ? | Research chainlist.org |
| 1868 | ? | Research chainlist.org |
| 33139 | ApeChain (warmbed_rpc_resolver.rs:230) | ✅ Already configured |
| 57073 | ? | Research chainlist.org |
| 167000 | ? | Research chainlist.org |

**Action:** Use chainlist.org to identify unknowns, add to parachain-registry.json

---

## Phase 4: Fixture-Based Testing (SYSTEMATIC VALIDATION)

### 4.1 Create Intent Fixture Extractor
```bash
# Script: extract-intent-fixtures.sh
#!/bin/bash

echo "Extracting fixture data from 100 live intents..."

# Get all intents
INTENTS_JSON=$(curl -s "http://localhost:8082/api/solver/intents")

# Save full dump
echo "$INTENTS_JSON" > /tmp/intents-full-dump-$(date +%Y%m%d-%H%M%S).json

# Extract fixture samples per protocol
for protocol in $(echo "$INTENTS_JSON" | jq -r '.intents[].protocol' | sort -u); do
  echo "Extracting fixtures for: $protocol"

  echo "$INTENTS_JSON" | \
    jq ".intents[] | select(.protocol == \"$protocol\") | {
      id,
      protocol,
      src_chain,
      dst_chain,
      amount,
      timestamp,
      state
    }" | \
    head -100 > "fixtures/${protocol}_intents.json"
done

echo "Fixtures saved to fixtures/ directory"
```

### 4.2 Gas Oracle Fixture Test
```bash
# Script: test-gas-fixtures.sh
#!/bin/bash

# Test all chains from live intents
CHAINS=$(curl -s "http://localhost:8082/api/solver/intents" | \
         jq -r '.intents[] | "\(.src_chain) \(.dst_chain)"' | \
         tr ' ' '\n' | sort -nu)

echo "Testing gas data for chains with live intents:"
for chain_id in $CHAINS; do
  GAS_RESPONSE=$(curl -s "http://localhost:8082/api/solver/razor" | \
                 jq ".presets[] | select(.chain_id == $chain_id)")

  if [ -z "$GAS_RESPONSE" ]; then
    echo "❌ Chain $chain_id: MISSING in Razor response"
  else
    READY=$(echo "$GAS_RESPONSE" | jq -r '.ready')
    GAS_COST=$(echo "$GAS_RESPONSE" | jq -r '.gas_cost_gwei')

    if [ "$READY" = "true" ] && [ "$GAS_COST" != "null" ]; then
      # Validate reasonable range (0.001 to 10000 gwei)
      IS_REASONABLE=$(echo "$GAS_COST" | awk '{if ($1 > 0.001 && $1 < 10000) print "yes"; else print "no"}')

      if [ "$IS_REASONABLE" = "yes" ]; then
        echo "✅ Chain $chain_id: READY - Gas: ${GAS_COST} gwei"
      else
        echo "⚠️  Chain $chain_id: UNREASONABLE - Gas: ${GAS_COST} gwei"
      fi
    else
      echo "❌ Chain $chain_id: NOT READY"
    fi
  fi
done
```

### 4.3 Protocol Decoding Test
```bash
# Verify all 100 intents decode cleanly
curl -s "http://localhost:8082/api/solver/intents" | \
  jq '[.intents[] | select(.state == "error")] | length'

# Should output: 0
# If > 0, means there are decoding errors
```

---

## Phase 5: Cross-VM Route Testing

### 5.1 Identify EVM ↔ Solana Routes
From coverage.xml analysis, we found 24+ routes like:
- Ethereum (1) → Solana (200) via Mayan Swift
- Arbitrum (42161) → Solana (200) via Mayan Swift
- Base (8453) → Solana (200) via Mayan Swift

**Test Plan:**
```bash
# Extract all cross-VM routes
curl -s "http://localhost:8082/api/solver/intents" | \
  jq -r '.intents[] | select(
    (.src_chain == 200 and .dst_chain != 200) or
    (.src_chain != 200 and .dst_chain == 200)
  ) | "\(.src_chain) → \(.dst_chain) via \(.protocol)"' | \
  sort -u

# Verify Solana gas estimation
curl -s "http://46.4.96.124:30081/api/gas/latest/200" | jq '.'
```

**Expected:** Once Solana is added to Spinner, should return slot-based gas estimation

---

## Phase 6: Automated Coverage Updater

### 6.1 SSE Stream Watcher
```rust
// Purpose: Continuously update coverage.xml as new intents arrive
// File: crates/coverage-monitor/src/main.rs

use eventsource_client::Client;
use serde_json::Value;
use std::collections::HashSet;

#[tokio::main]
async fn main() {
    let sse_url = "http://46.4.96.124:30081/api/genome/subscribe/sse";
    let client = Client::new(sse_url).unwrap();

    let mut seen_chains = HashSet::new();
    let mut seen_protocols = HashSet::new();

    for event in client {
        if let Ok(evt) = event {
            if let Some(data) = evt.data {
                if let Ok(intent) = serde_json::from_str::<Value>(&data) {
                    let src = intent["src_chain"].as_u64().unwrap();
                    let dst = intent["dst_chain"].as_u64().unwrap();
                    let protocol = intent["protocol"].as_str().unwrap();

                    if seen_chains.insert(src) || seen_chains.insert(dst) {
                        println!("🆕 New chain discovered: {}", if seen_chains.contains(&src) { src } else { dst });
                        // TODO: Update coverage.xml
                    }

                    if seen_protocols.insert(protocol.to_string()) {
                        println!("🆕 New protocol discovered: {}", protocol);
                        // TODO: Update coverage.xml
                    }
                }
            }
        }
    }
}
```

---

## Phase 7: Deployment & Validation

### 7.1 Spinner Deployment Checklist
```bash
# 1. Update parachain-registry.json with new chains
# 2. Rebuild Spinner
ssh root@46.4.96.124 "cd /root/spinner && git pull origin master"
ssh root@46.4.96.124 "cd /root/spinner/rust && PATH=/root/.cargo/bin:$PATH cargo build --release --bin spinner"

# 3. Rebuild Docker image
ssh root@46.4.96.124 "cp /root/spinner/rust/target/release/spinner /tmp/spinner-binary"
ssh root@46.4.96.124 "cd /tmp && docker build --no-cache -t spinner-monolith:latest -f Dockerfile.spinner-quick ."
ssh root@46.4.96.124 "docker save spinner-monolith:latest | k3s ctr images import -"

# 4. Restart pod
ssh root@46.4.96.124 "kubectl delete pod -n spinner spinner-0"
ssh root@46.4.96.124 "kubectl get pods -n spinner -l app=spinner -w"

# 5. Verify
ssh root@46.4.96.124 "kubectl logs -n spinner spinner-0 --tail=100 | grep 'Chain 200'"
```

### 7.2 Solver Deployment Checklist
```bash
# 1. Already deployed with fixed gas calculation (COVERAGE_MATRIX.md line 167)
# 2. Verify 100 intents are being tracked
curl -s "http://localhost:8082/api/solver/intents" | jq '.intents | length'

# 3. Test Razor endpoint
curl -s "http://localhost:8082/api/solver/razor" | jq '.presets | length'
```

---

## Success Metrics

### Immediate (24 hours)
- [ ] Spinner pod fully ready (1/1)
- [ ] Gas API returning data for all 5 core chains (1, 10, 137, 8453, 42161)
- [ ] Solana (200) gas estimation live
- [ ] 502 error on https://api.taifoon.dev/api/gas/latest fixed

### Short-term (1 week)
- [ ] All chains from 100 live intents have gas coverage
- [ ] Zero decoding errors in intent processing
- [ ] Cross-VM routes (EVM ↔ Solana) fully tested
- [ ] coverage.xml auto-updates from SSE stream

### Long-term (1 month)
- [ ] All 31 protocols from protocols.xml have fixtures
- [ ] All unknown chain IDs resolved
- [ ] Automated CI/CD for coverage validation
- [ ] Full autonomous delivery readiness for t3rn Lambda

---

## Key Files Reference

### Warmbed Gas API
- **Endpoint Definition:** `spinner/rust/crates/da-api/src/api.rs:489-556`
- **Storage Layer:** `spinner/rust/crates/da-api/src/storage.rs` (CF_GAS_METRICS)
- **RPC Fallbacks:** `spinner/rust/crates/da-api/src/warmbed_rpc_resolver.rs:195-255`

### Solver API
- **Razor Handler:** `taifoon-solver/crates/solver-api/src/lib.rs:343-474`
- **Gas Calculation (FIXED):** Line 417 — changed from multiply to direct price

### Configuration
- **Chain Registry:** `spinner/deployments/parachain-registry.json`
- **Protocol Definitions:** `spinner/rust/crates/header-collector/protocols.xml`

### Testing
- **Coverage Test:** `taifoon-solver/test-coverage.sh`
- **Fixture Test:** `taifoon-solver/test-fixtures.sh`
- **Live Data:** `taifoon-solver/coverage.xml`

---

## Next Immediate Actions (in order)

1. **Wait for Spinner pod ready** (ETA: 2-3 minutes from restart)
2. **Test gas API:** `curl http://46.4.96.124:30081/api/gas/latest | jq .chains`
3. **Add Solana to parachain-registry.json**
4. **Rebuild & deploy Spinner**
5. **Run test-coverage.sh again to validate**
6. **Extract fixtures from 100 live intents**
7. **Document cross-VM routes**

---

**Status:** 🟡 Awaiting Spinner pod readiness
**Blocker:** Spinner restarting (59s age in last check)
**Critical Path:** Solana support → 20+ intents unlocked
