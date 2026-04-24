# Complete Master Plan: Taifoon Solver Integration

**Date**: 2026-04-23  
**Status**: 🟡 Data Source Identified, Integration Needed

---

## Executive Summary

**THE SITUATION:**
- ✅ Spinner HAS protocol scanning (10,000 t3rn orders tracked)
- ✅ Spinner HAS protocols.xml with 31+ bridge protocols  
- ✅ Spinner HAS protocol_event_processor infrastructure
- ❌ Genome SSE stream does NOT publish protocol events (only blocks/gas)
- ❌ Solver is waiting for SSE protocol events that don't come
- ❌ Dashboard shows zero intents because solver has no data

**THE ROOT ISSUE:**
The genome stream publishes blocks but NOT decoded protocol events.  
The solver expects `entity: "proto"` events but stream only sends `entity: "block"`.

**THE SOLUTION:**
Two paths forward:

1. **Path A (Quick Win)**: Solver polls Spinner DA API for t3rn orders instead of SSE
2. **Path B (Proper Fix)**: Enable protocol event publishing to genome SSE stream

---

## Current System Architecture (AS IS)

###

 Data Flow TODAY:

```
Spinner (46.4.96.124)
  ├─ Header Collector → Blocks ingested
  ├─ Protocol Scanner → t3rn orders decoded (10,000 in DB)
  ├─ Genome SSE → Publishes blocks/gas ONLY
  └─ DA API (port 30081) → Has protocol order data ✅
       └─ /api/lambda/t3rn/orders/stats (10,000 orders)
       └─ /api/lambda/t3rn/orders/pending (0 currently pending)

Genome Stream (SSE)
  └─ https://api.taifoon.dev/api/genome/subscribe/sse
       ├─ entity: "block" ✅ Published
       ├─ entity: "gas" ✅ Published  
       └─ entity: "proto" ❌ NOT published

Solver (localhost:8082)
  ├─ Genome Client → Connected ✅
  ├─ Filters for entity="proto" → NO MATCHES ❌
  ├─ total_intents: 0 ← NO DATA
  └─ Dashboard SSE → Broadcasting empty data

Dashboard (localhost:3000)
  └─ Shows 0 intents ← Waiting for solver data
```

---

## What EXISTS Right Now

### ✅ In Spinner (`/Users/mbultra/projects/spinner`)

**Protocol Infrastructure:**
- `rust/crates/header-collector/protocols.xml` - 31+ protocols defined
- `rust/crates/header-collector/src/protocol_event_processor.rs` - Decoder
- `rust/crates/header-collector/src/evm_event_decoder.rs` - EVM events
- `rust/crates/header-collector/src/unified_protocol_event.rs` - Unified format
- `rust/crates/da-api/src/protocol_registry.rs` - Protocol registry

**Data Stored:**
- 10,000 t3rn lambda orders (Executed/Claimed/Expired)
- Orders across 14 chains (Linea: 2657, Optimism: 1668, Unichain: 930, etc.)
- Accessible via `/api/lambda/t3rn/orders/*` endpoints

**Genome Stream:**
- Publishes via SSE: `https://api.taifoon.dev/api/genome/subscribe/sse`
- Currently: blocks + gas snapshots
- Missing: protocol events (`entity: "proto"`)

### ✅ In Solver (`/Users/mbultra/projects/taifoon-solver`)

**Components Built:**
- Genome client (SSE consumer) - CONNECTED ✅
- Profit calculator - Ready ✅
- Executor (3-tier liquidity waterfall) - Ready ✅
- T3RN sidecar - Ready ✅
- Dashboard (Next.js 15 + SSE) - Running ✅
- Solver API (port 8082) - Running ✅

**Missing:**
- Data source connection (genome SSE has no protocol events)

### ✅ In TamTam (`/Users/mbultra/projects/tamtam`)

**Registered:**
- Project: `taifoon-solver` (enabled, high priority)
- 7 Agents: Orchestrator + 6 delivery agents
- 7 Skills: Complete delivery pipeline defined

---

## The Two Paths Forward

### Path A: Quick Win - Poll DA API (RECOMMENDED FIRST)

**What**: Modify solver to poll Spinner DA API instead of SSE

**Pros:**
- Uses existing data (10,000 orders available)
- No Spinner changes needed
- Can work TODAY
- Tests entire solver→dashboard flow

**Cons:**
- Polling (not real-time SSE)
- Only t3rn data (not all 31 protocols)

**Implementation:**
1. Add DA API client to solver
2. Poll `/api/lambda/t3rn/orders/stats` every 5-10s
3. Convert orders → intents
4. Feed to profit calculator → executor → dashboard
5. VERIFY end-to-end flow works

**Time**: 2-3 hours (TamTam Agent can do this)

### Path B: Proper Fix - Enable Protocol Events in SSE

**What**: Modify Spinner to publish protocol events to genome stream

**Pros:**
- Real-time SSE (no polling)
- All protocols (not just t3rn)
- Scalable architecture

**Cons:**
- Requires Spinner code changes
- Requires deploy to 46.4.96.124
- More complex

**Implementation:**
1. Find where genome SSE publishes events in Spinner
2. Hook protocol_event_processor into SSE publisher
3. Publish `entity: "proto"` events with required fields
4. Deploy via `/admin/deploy` endpoint
5. Verify solver receives events

**Time**: 4-6 hours (requires Spinner expertise)

---

## TamTam Orchestration Plan (REVISED)

### Phase 0: Data Integration (NEW!)

**Agent 0A: DA API Poller** (Path A - Quick Win)
- Modify genome client to poll DA API
- Convert t3rn orders → intents
- Test solver→dashboard flow
- **Deliverable**: Intents visible in dashboard ✅

**Agent 0B: SSE Protocol Publisher** (Path B - Proper Fix)  
- Modify Spinner genome publisher
- Enable protocol event streaming
- Deploy to production server
- **Deliverable**: Real-time protocol events in SSE ✅

### Phase 1: Foundation (Agents 1-2)

**Agent 1: Protocol XML Analyzer**
- Parse protocols.xml (already exists in Spinner)
- Create protocols_registry.json
- Document 31+ protocols

**Agent 2: T3RN Sidecar**  
- Already built ✅
- Ready for integration testing

### Phase 2: UI & Execution (Agents 3-4)

**Agent 3: Dashboard** - Already built ✅
**Agent 4: Executor** - Already built ✅

### Phase 3: Integration & Docs (Agents 5-6)

**Agent 5: E2E Testing**
- Verify data flow end-to-end
- Test with real t3rn orders  
- Validate dashboard updates

**Agent 6: Deployment & Docs**
- DEPLOYMENT.md creation
- README.md updates
- Production deployment guides

---

## Immediate Next Steps (IN ORDER)

### 1. Choose Path (A or B or Both)

**Recommendation**: Start with Path A (DA API polling)
- Gets data flowing TODAY
- Proves solver logic works
- Validates dashboard integration
- Then do Path B for real-time SSE

### 2. Execute Agent 0A (DA API Integration)

**Task**: Create genome client variant that polls DA API

**Code changes needed:**
```rust
// In taifoon-solver/crates/genome-client/src/lib.rs
// Add DA API poller alongside SSE client

pub struct DaApiPoller {
    base_url: String,
    client: reqwest::Client,
}

impl DaApiPoller {
    pub async fn poll_orders(&self) -> Result<Vec<T3rnOrder>> {
        let url = format!("{}/api/lambda/t3rn/orders/stats", self.base_url);
        // ... fetch and convert to Intent
    }
}
```

**Integration**:
- Solver main.rs: Start DA poller in parallel with SSE client
- Convert T3rn orders → Intent format
- Feed to existing profit calc → executor → dashboard SSE

### 3. Verify Dashboard Shows Data

Once Agent 0A is done:
- Open http://localhost:3000
- Should see t3rn orders as intents
- Protocol breakdown shows t3rn
- Money flow tracks profitability

### 4. Execute Remaining Agents (1-6)

With data flowing, complete the delivery pipeline.

---

## Success Criteria

### Phase 0 Complete When:
- ✅ Solver detects t3rn orders from DA API
- ✅ Dashboard shows live intent data
- ✅ total_intents > 0 in /api/solver/stats
- ✅ IntentsStream component displays orders
- ✅ ProtocolBreakdown shows t3rn entries

### Full Delivery Complete When:
- ✅ All 31+ protocols supported (protocols_registry.json)
- ✅ Real-time SSE protocol events (Path B complete)
- ✅ Executor tests profitable fills
- ✅ T3RN LWC fallback works
- ✅ E2E testing passes
- ✅ Documentation complete (DEPLOYMENT.md, README.md)

---

## API Endpoints Reference

### Spinner (https://api.taifoon.dev)

**Health:**
- `GET /health` - System status

**Networks:**
- `GET /api/spinner/networks` - 41 chains tracked

**Protocol Orders (t3rn):**
- `GET /api/lambda/t3rn/orders/stats` - 10,000 orders summary
- `GET /api/lambda/t3rn/orders/pending` - Pending orders
- `GET /v5/order/:order_id` - Single order details

**Genome Stream:**
- `GET /api/genome/subscribe/sse` - SSE stream (blocks/gas only currently)

### Solver (http://localhost:8082)

**Stats:**
- `GET /api/solver/stats` - Solver statistics
- `GET /api/solver/intents` - Intent history
- `GET /api/solver/protocols` - Protocol performance
- `GET /api/solver/money-flow` - Profit breakdown

**Stream:**
- `GET /api/solver/stream` - SSE event stream to dashboard

---

## Files Created This Session

1. `TAMTAM_ORCHESTRATION.md` - TamTam pipeline overview
2. `GENOME_STREAM_INTEGRATION.md` - Data flow analysis
3. `COMPLETE_MASTER_PLAN.md` - This file (master plan)
4. `/Users/mbultra/projects/tamtam/data/skills/taifoon-solver/` - 7 agent skills
5. TamTam database: 7 agents registered and enabled

---

## Key Insights

1. **Spinner DOES have protocol data** (10,000 t3rn orders)
2. **Genome SSE is incomplete** (blocks only, no protocols)
3. **DA API is the workaround** (has order data via REST)
4. **Solver is correctly built** (just needs data source connection)
5. **TamTam is ready** (all agents registered and waiting)

---

**Next Command to Run:**

```bash
# Start with Path A - create Agent 0A for DA API polling
curl -X POST http://localhost:1337/api/agents/orchestrator/run \
  -H "Content-Type: application/json" \
  -d '{"prompt": "Execute Agent 0A: Integrate DA API polling to get t3rn orders into solver, then verify dashboard shows live data"}'
```

---

**Status**: 🟢 Ready to Execute  
**Blocker Identified**: Genome SSE missing protocol events  
**Workaround Available**: DA API has data  
**Path Forward**: Clear and actionable
