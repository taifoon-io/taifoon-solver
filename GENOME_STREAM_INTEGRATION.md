# Genome Stream Integration - The Missing Link

## Problem Identified

✅ Genome Stream: **CONNECTED**  
✅ Stream Data: Block/gas events flowing  
❌ **Protocol Events: MISSING**  
❌ Intent Detection: 0 intents (waiting for protocol events)

## Root Cause

The genome stream at `https://api.taifoon.dev/api/genome/subscribe/sse` currently publishes:
- ✅ Block ingestion events (`entity: "block"`)
- ✅ Gas snapshots (`entity: "gas"`)
- ❌ **Protocol events (`entity: "proto"`) - NOT PUBLISHING**

The solver's genome client filters for `entity: "proto"` events with these fields:
- `id`: Protocol name (e.g., "lifi_v2", "stargate_v2")
- `action`: "deposit" or "fill"  
- `src_chain`, `dst_chain`: Cross-chain route
- `depositor`, `recipient`: Addresses
- `amount`, `token`: Transfer details
- `reference`: Transaction hash

## The Missing Component: Protocol Scanner

The genome stream needs a **Protocol Scanner** that:
1. Reads blocks from the stream
2. Decodes protocol-specific events (using protocols.xml contracts/ABIs)
3. Publishes protocol events back to genome stream
4. Enables solver to detect bridge intents

## Current Architecture (Broken)

```
Spinner Header Collector
  ↓
Genome Stream (SSE)
  ├─ Publishes: blocks, gas
  └─ Missing: protocol events
       ↓
Solver (waiting...)
  └─ Filters for entity="proto"
  └─ total_intents: 0 ← NO DATA
```

## Target Architecture (Working)

```
Spinner Header Collector
  ↓
Protocol Scanner (NEW!)
  ├─ Reads: blocks from genome
  ├─ Decodes: Across, Stargate, LiFi events
  └─ Publishes: entity="proto" to genome
       ↓
Genome Stream (SSE)
  └─ Publishes: blocks, gas, PROTOCOLS
       ↓
Solver
  ├─ Receives: protocol events
  ├─ Calculates: profitability
  └─ Executes: fills
```

## Solution Options

### Option 1: Enable Spinner Protocol Scanning (Best)

**Location**: `/Users/mbultra/projects/spinner`  
**File**: `rust/crates/header-collector/protocols.xml`

The spinner project already has:
- ✅ protocols.xml with 31+ protocols
- ✅ Header collector reading blocks
- ❌ Protocol event decoder (needs implementation)

**Action**: Implement protocol scanner in spinner that:
1. Uses protocols.xml to know what events to watch
2. Decodes events from collected blocks
3. Publishes to genome stream with `entity: "proto"`

**TamTam Agent Needed**: "Spinner Protocol Scanner Builder"
- Parse protocols.xml
- Generate event decoders for each protocol
- Integrate with genome stream publisher
- Deploy to spinner on 46.4.96.124

### Option 2: Mock Protocol Events (Testing Only)

Create a mock generator that publishes fake protocol events to test the solver.

**Not recommended**: Doesn't solve real data problem.

### Option 3: Query Spinner DA API Directly (Workaround)

Instead of genome stream SSE, query spinner's DA API for protocol receipts.

**Endpoint**: `http://46.4.96.124:30081/api/lambda/t3rn/orders/pending`

This gives real protocol data but loses real-time SSE benefits.

## Recommended Next Steps

### Immediate (Fix Data Flow):

1. **Check if spinner has protocol scanning**:
   ```bash
   ssh root@46.4.96.124 "cd /root/spinner && grep -r 'protocol' rust/crates/*/src/*.rs | grep -i event"
   ```

2. **Query DA API for existing protocol data**:
   ```bash
   curl http://46.4.96.124:30081/api/lambda/t3rn/orders/pending
   curl http://46.4.96.124:30081/api/lambda/t3rn/orders/stats
   ```

3. **If protocol data exists**: Modify solver to poll DA API instead of SSE
4. **If no protocol data**: TamTam must deliver "Protocol Scanner" agent

### TamTam Orchestration Update:

Add **Agent 0: Spinner Protocol Scanner**  
- **Before** Agent 1-6 delivery
- **Purpose**: Enable protocol event publishing to genome stream
- **Location**: `/Users/mbultra/projects/spinner` (not taifoon-solver)
- **Deliverable**: Protocol events flowing to genome SSE

## Testing Protocol Events

Once protocol scanning is enabled:

```bash
# Should see entity="proto" events
curl -N https://api.taifoon.dev/api/genome/subscribe/sse | grep 'entity":"proto'

# Solver should detect intents
curl http://localhost:8082/api/solver/stats
# total_intents: > 0 ✅

# Dashboard should show live data
open http://localhost:3000
# IntentsStream: showing real-time intents ✅
```

## Current Workaround

While protocol scanning is being implemented, the solver can:

1. **Poll spinner DA API** for protocol orders:
   ```rust
   // Replace genome SSE client with DA API poller
   let orders = reqwest::get("http://46.4.96.124:30081/api/lambda/t3rn/orders/pending")
       .await?
       .json::<Vec<Order>>()
       .await?;
   ```

2. **Convert orders → intents** and process normally

This gets real data flowing while genome stream enhancement is in progress.

---

**Status**: 🔴 Data flow blocked - needs protocol scanning  
**Blocker**: Genome stream missing protocol events  
**Solution**: Implement protocol scanner in spinner OR poll DA API directly
