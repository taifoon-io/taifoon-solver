# Taifoon Solver - Complete Delivery Plan

**Goal**: Full profitable solver with 1-page dashboard for all protocols in protocols.xml

**Timeline**: 1 session (dashboard + API) + 2-3 days (executor)

---

## Phase 1: Solver API + Dashboard (THIS SESSION)

### 1.1 Solver API Crate

**Location**: `crates/solver-api/`

**Purpose**: Expose solver internals via HTTP + SSE for dashboard consumption

**Dependencies**:
```toml
[dependencies]
axum = "0.7"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tower-http = { version = "0.5", features = ["cors"] }
anyhow = "1"
tracing = "0.1"
```

**Endpoints**:

```rust
// GET /api/solver/stream - SSE feed of all intents
// Returns: Server-Sent Events stream
{
  "event": "intent_detected",
  "data": {
    "id": "lifi_v2:0xabc...",
    "protocol": "lifi_v2",
    "src_chain": 1,
    "dst_chain": 42161,
    "amount": "10000000000",
    "token": "0xA0b86991...",
    "timestamp": "2026-04-23T12:34:56Z"
  }
}

{
  "event": "intent_attempted",
  "data": {
    "id": "lifi_v2:0xabc...",
    "profitable": true,
    "profit_usd": 43.90,
    "protocol_fee_usd": 49.00,
    "gas_cost_usd": 5.10,
    "decision": "execute"
  }
}

{
  "event": "intent_solved",
  "data": {
    "id": "lifi_v2:0xabc...",
    "tx_hash": "0xdef...",
    "actual_profit_usd": 42.15,
    "gas_used": "145234"
  }
}

// GET /api/solver/stats - Current statistics
{
  "status": "live",
  "net_profit_today_usd": 432.18,
  "latency_ms": 127,
  "success_rate": 0.942,
  "total_intents": 1247,
  "profitable_intents": 823,
  "skipped_intents": 424,
  "executed_fills": 776,
  "failed_fills": 47
}

// GET /api/solver/intents?limit=50 - Recent intents
{
  "intents": [
    {
      "id": "lifi_v2:0xabc...",
      "protocol": "lifi_v2",
      "timestamp": "2026-04-23T12:34:56Z",
      "state": "solved",
      "profit_usd": 43.90,
      "tx_hash": "0xdef..."
    }
  ]
}

// GET /api/solver/protocols - Protocol breakdown
{
  "protocols": [
    {
      "name": "lifi_v2",
      "fills": 432,
      "volume_usd": 4320000,
      "profit_usd": 2156.80,
      "fee_bps": 49
    },
    {
      "name": "stargate_v2",
      "fills": 89,
      "volume_usd": 890000,
      "profit_usd": 178.00,
      "fee_bps": 2
    }
  ]
}

// GET /api/solver/money-flow - P&L breakdown
{
  "period": "24h",
  "protocol_fees_usd": 12458.32,
  "gas_costs_usd": -2341.12,
  "liquidity_costs_usd": -124.00,
  "net_profit_usd": 9993.20,
  "roi": 4.26
}
```

**Implementation**:

```rust
// crates/solver-api/src/lib.rs
use axum::{
    Router,
    routing::get,
    response::sse::{Event, Sse},
    Json,
};
use tokio::sync::broadcast;
use std::sync::Arc;

pub struct SolverApi {
    event_tx: broadcast::Sender<SolverEvent>,
    stats: Arc<RwLock<SolverStats>>,
}

#[derive(Clone, Debug, Serialize)]
pub enum SolverEvent {
    IntentDetected(Intent),
    IntentAttempted(ProfitResult),
    IntentSolved(FillResult),
}

pub struct SolverStats {
    pub net_profit_today_usd: f64,
    pub latency_ms: u64,
    pub success_rate: f64,
    pub total_intents: u64,
    pub profitable_intents: u64,
    pub skipped_intents: u64,
    pub executed_fills: u64,
    pub failed_fills: u64,
}

impl SolverApi {
    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel(1000);
        Self {
            event_tx,
            stats: Arc::new(RwLock::new(SolverStats::default())),
        }
    }

    pub fn router(&self) -> Router {
        Router::new()
            .route("/api/solver/stream", get(stream_handler))
            .route("/api/solver/stats", get(stats_handler))
            .route("/api/solver/intents", get(intents_handler))
            .route("/api/solver/protocols", get(protocols_handler))
            .route("/api/solver/money-flow", get(money_flow_handler))
            .layer(tower_http::cors::CorsLayer::permissive())
    }

    pub fn emit_event(&self, event: SolverEvent) {
        let _ = self.event_tx.send(event);
    }
}

async fn stream_handler() -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // SSE implementation
}
```

### 1.2 Wire Solver API into Main Binary

**Location**: `crates/solver-main/src/main.rs`

```rust
#[tokio::main]
async fn main() -> Result<()> {
    // Initialize solver API
    let solver_api = SolverApi::new();

    // Spawn API server on port 8082
    let api_router = solver_api.router();
    tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind("0.0.0.0:8082").await.unwrap();
        axum::serve(listener, api_router).await.unwrap();
    });

    // Existing genome client + profit calculator
    let genome_client = GenomeClient::new(&genome_url);
    let profit_calc = ProfitCalculator::new(min_profit, solver_intel);

    // Intent processing loop with API events
    while let Some(intent) = intent_rx.recv().await {
        // Emit: intent detected
        solver_api.emit_event(SolverEvent::IntentDetected(intent.clone()));

        // Calculate profit
        let profit_result = profit_calc.calculate(&intent).await?;

        // Emit: intent attempted
        solver_api.emit_event(SolverEvent::IntentAttempted(profit_result.clone()));

        if profit_result.is_profitable {
            // TODO: Execute fill (Phase 3)
            // Emit: intent solved
            // solver_api.emit_event(SolverEvent::IntentSolved(fill_result));
        }
    }
}
```

### 1.3 Dashboard (Next.js)

**Location**: `dashboard/` (new subdirectory in taifoon-solver repo)

**Stack**:
- Next.js 15 (App Router)
- Tailwind CSS
- Server-Sent Events for real-time updates
- No external dependencies for charts (use CSS for simple bars)

**Structure**:
```
dashboard/
├── app/
│   ├── layout.tsx       # Root layout with Inter font
│   ├── page.tsx         # Main 1-page dashboard
│   └── globals.css      # Tailwind + custom CSS
├── components/
│   ├── IntentsStream.tsx    # Live intent feed
│   ├── PerformanceStats.tsx # Latency, success rate
│   ├── ProtocolBreakdown.tsx # Protocol fills
│   ├── MoneyFlow.tsx        # P&L breakdown
│   └── TopIntents.tsx       # Top 10 by profit
├── hooks/
│   └── useSolverEvents.ts   # SSE hook
├── package.json
├── tailwind.config.ts
├── tsconfig.json
└── next.config.js
```

**Main Page** (`app/page.tsx`):

```tsx
'use client'

import { useSolverEvents } from '@/hooks/useSolverEvents'
import IntentsStream from '@/components/IntentsStream'
import PerformanceStats from '@/components/PerformanceStats'
import ProtocolBreakdown from '@/components/ProtocolBreakdown'
import MoneyFlow from '@/components/MoneyFlow'
import TopIntents from '@/components/TopIntents'

export default function Dashboard() {
  const { intents, stats, protocols, moneyFlow } = useSolverEvents()

  return (
    <div className="min-h-screen bg-[#1A1A1A] text-white">
      {/* Header */}
      <header className="border-b border-gray-800 px-6 py-4 flex justify-between items-center">
        <h1 className="text-2xl font-bold">Taifoon Solver</h1>
        <div className="flex items-center gap-4">
          <div className="flex items-center gap-2">
            <div className="w-2 h-2 bg-[#00FF88] rounded-full animate-pulse" />
            <span className="text-sm text-gray-400">LIVE</span>
          </div>
          <div className="text-xl font-bold text-[#00FF88]">
            Net: ${stats.net_profit_today_usd.toFixed(2)}
          </div>
        </div>
      </header>

      {/* Main Grid */}
      <div className="grid grid-cols-3 gap-6 p-6">
        {/* Left Column: Intents Stream */}
        <div className="col-span-2">
          <IntentsStream intents={intents} />
        </div>

        {/* Right Column: Stats */}
        <div className="space-y-6">
          <PerformanceStats stats={stats} />
          <ProtocolBreakdown protocols={protocols} />
          <TopIntents intents={intents} />
        </div>

        {/* Bottom: Money Flow */}
        <div className="col-span-3">
          <MoneyFlow flow={moneyFlow} />
        </div>
      </div>
    </div>
  )
}
```

**SSE Hook** (`hooks/useSolverEvents.ts`):

```ts
import { useEffect, useState } from 'react'

export function useSolverEvents() {
  const [intents, setIntents] = useState([])
  const [stats, setStats] = useState(null)

  useEffect(() => {
    // Connect to SSE stream
    const eventSource = new EventSource('http://localhost:8082/api/solver/stream')

    eventSource.addEventListener('intent_detected', (e) => {
      const intent = JSON.parse(e.data)
      setIntents(prev => [intent, ...prev].slice(0, 50))
    })

    eventSource.addEventListener('intent_attempted', (e) => {
      const result = JSON.parse(e.data)
      // Update intent with profit info
    })

    eventSource.addEventListener('intent_solved', (e) => {
      const result = JSON.parse(e.data)
      // Update intent with tx hash
    })

    // Fetch stats every 5 seconds
    const statsInterval = setInterval(async () => {
      const res = await fetch('http://localhost:8082/api/solver/stats')
      setStats(await res.json())
    }, 5000)

    return () => {
      eventSource.close()
      clearInterval(statsInterval)
    }
  }, [])

  return { intents, stats, protocols: [], moneyFlow: null }
}
```

**Intents Stream Component** (`components/IntentsStream.tsx`):

```tsx
export default function IntentsStream({ intents }) {
  return (
    <div className="bg-[#0A0A0A] border border-gray-800 rounded-lg p-6">
      <h2 className="text-lg font-bold mb-4">INTENTS STREAM (Real-time)</h2>
      <div className="space-y-3 max-h-[600px] overflow-y-auto">
        {intents.map(intent => (
          <div
            key={intent.id}
            className="bg-[#1A1A1A] border border-gray-700 rounded p-4"
          >
            <div className="flex justify-between items-start mb-2">
              <div className="flex items-center gap-2">
                <span className="text-2xl">📥</span>
                <span className="font-mono text-sm text-[#00D9FF]">
                  {intent.protocol} #{intent.id.slice(-8)}
                </span>
              </div>
              <div className="text-right">
                {intent.state === 'solved' && (
                  <span className="text-[#00FF88]">✅ EXECUTED</span>
                )}
                {intent.state === 'skipped' && (
                  <span className="text-gray-500">⏭️ SKIP</span>
                )}
              </div>
            </div>
            <div className="text-sm text-gray-400 space-y-1">
              <div>
                Chain {intent.src_chain} → {intent.dst_chain} •
                {(parseInt(intent.amount) / 1e6).toFixed(2)} USDC
              </div>
              {intent.profit_usd && (
                <>
                  <div className="text-white font-bold">
                    Profit: ${intent.profit_usd.toFixed(2)}
                  </div>
                  <div className="text-xs">
                    Protocol Fee: ${intent.protocol_fee_usd.toFixed(2)} •
                    Gas: ${intent.gas_cost_usd.toFixed(2)}
                  </div>
                </>
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}
```

### 1.4 Deployment

**Docker Update** (`k8s/Dockerfile`):

```dockerfile
FROM ubuntu:24.04

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Copy solver binary
COPY target/release/taifoon-solver /usr/local/bin/taifoon-solver
COPY config/solver_intel.json /app/config/solver_intel.json

WORKDIR /app

ENV RUST_LOG=info
ENV GENOME_SSE_URL=https://api.taifoon.dev/api/genome/subscribe/sse
ENV MIN_PROFIT_USD=1.0

# Expose ports
EXPOSE 8082

CMD ["/usr/local/bin/taifoon-solver"]
```

**K8s Deployment** (`k8s/deployment.yaml`):

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: taifoon-solver
  namespace: default
  labels:
    app: taifoon-solver
spec:
  replicas: 1
  selector:
    matchLabels:
      app: taifoon-solver
  template:
    metadata:
      labels:
        app: taifoon-solver
    spec:
      containers:
      - name: taifoon-solver
        image: taifoon-solver:latest
        imagePullPolicy: Never
        env:
        - name: RUST_LOG
          value: "info"
        - name: GENOME_SSE_URL
          value: "https://api.taifoon.dev/api/genome/subscribe/sse"
        - name: MIN_PROFIT_USD
          value: "1.0"
        ports:
        - containerPort: 8082
          name: api
        resources:
          requests:
            memory: "256Mi"
            cpu: "200m"
          limits:
            memory: "1Gi"
            cpu: "1000m"
---
apiVersion: v1
kind: Service
metadata:
  name: taifoon-solver
  namespace: default
spec:
  selector:
    app: taifoon-solver
  ports:
  - port: 8082
    targetPort: 8082
    nodePort: 30082
  type: NodePort
```

**Dashboard Deployment** (Vercel or self-hosted):

```bash
# Option 1: Vercel
cd dashboard
vercel --prod

# Option 2: Self-hosted (alongside solver)
cd dashboard
npm run build
npm run start  # Runs on port 3000
```

---

## Phase 2: Money Flow & Rewards Tracking

### 2.1 Money Flow Data Model

**Storage**: In-memory (for now), persist to SQLite later

```rust
pub struct MoneyFlowTracker {
    pub protocol_fees_earned: f64,
    pub gas_costs_paid: f64,
    pub liquidity_costs_paid: f64,
    pub fills: Vec<FillRecord>,
}

pub struct FillRecord {
    pub intent_id: String,
    pub protocol: String,
    pub timestamp: DateTime<Utc>,
    pub amount_usd: f64,
    pub protocol_fee_usd: f64,
    pub gas_cost_usd: f64,
    pub liquidity_cost_usd: f64,
    pub net_profit_usd: f64,
    pub tx_hash: Option<String>,
}

impl MoneyFlowTracker {
    pub fn record_fill(&mut self, fill: FillRecord) {
        self.protocol_fees_earned += fill.protocol_fee_usd;
        self.gas_costs_paid += fill.gas_cost_usd;
        self.liquidity_costs_paid += fill.liquidity_cost_usd;
        self.fills.push(fill);
    }

    pub fn net_profit(&self) -> f64 {
        self.protocol_fees_earned - self.gas_costs_paid - self.liquidity_costs_paid
    }

    pub fn roi(&self) -> f64 {
        if self.gas_costs_paid + self.liquidity_costs_paid == 0.0 {
            return 0.0;
        }
        self.net_profit() / (self.gas_costs_paid + self.liquidity_costs_paid)
    }
}
```

### 2.2 Reward Claiming Flow

**For LiFi** (example - each protocol has different claim flow):

```rust
pub async fn claim_lifi_reward(
    provider: &Provider,
    wallet: &LocalWallet,
    intent_id: &str,
) -> Result<TxHash> {
    // LiFi automatically credits solver on destination chain
    // No separate claim needed - reward is in the fill transaction itself
    // Profit = (amount_out - amount_in) - gas_cost
    Ok(TxHash::default()) // No-op for LiFi
}
```

**For T3RN LWC**:

```rust
pub async fn claim_lwc_reward(
    provider: &Provider,
    wallet: &LocalWallet,
    order_id: &str,
) -> Result<TxHash> {
    // T3RN LWC rewards are claimed separately after execution
    let lwc_contract = LiquidityWellContract::new(LWC_ADDRESS, provider);

    // Call claimReward(order_id)
    let tx = lwc_contract
        .claimReward(order_id.into())
        .from(wallet.address())
        .send()
        .await?;

    Ok(tx.tx_hash())
}
```

---

## Phase 3: Executor (Next Session)

### 3.1 Executor Crate

**Location**: `crates/executor/`

**Purpose**: Execute profitable fills on-chain

**Key Functions**:

```rust
pub async fn execute_fill(intent: &Intent, profit: &ProfitResult) -> Result<FillResult> {
    match intent.protocol.as_str() {
        "lifi_v2" => execute_lifi_fill(intent, profit).await,
        "stargate_v2" => execute_stargate_fill(intent, profit).await,
        "across_v3" => execute_across_fill(intent, profit).await,
        "t3rn_lwc" => execute_lwc_fill(intent, profit).await,
        _ => Err(anyhow!("Unsupported protocol: {}", intent.protocol)),
    }
}

async fn execute_lifi_fill(intent: &Intent, profit: &ProfitResult) -> Result<FillResult> {
    // 1. Check balance on destination chain
    let balance = check_balance(intent.dst_chain, intent.token).await?;
    if balance < parse_amount(&intent.amount)? {
        return Err(anyhow!("Insufficient balance"));
    }

    // 2. Build fill transaction
    let fill_tx = build_lifi_fill_tx(intent).await?;

    // 3. Simulate
    let sim_result = simulate_tx(intent.dst_chain, &fill_tx).await?;
    if !sim_result.success {
        return Err(anyhow!("Simulation failed"));
    }

    // 4. Execute
    let tx_hash = send_tx(intent.dst_chain, fill_tx).await?;

    // 5. Wait for confirmation
    let receipt = wait_for_receipt(intent.dst_chain, tx_hash).await?;

    // 6. Calculate actual profit
    let actual_gas_cost = receipt.gas_used * receipt.effective_gas_price;
    let actual_profit = profit.net_profit_usd - (actual_gas_cost as f64 / 1e18);

    Ok(FillResult {
        tx_hash,
        actual_profit_usd: actual_profit,
        gas_used: receipt.gas_used,
    })
}
```

### 3.2 Integration with Solver Main

```rust
// In solver-main/src/main.rs
if profit_result.is_profitable {
    // Emit: intent attempted
    solver_api.emit_event(SolverEvent::IntentAttempted(profit_result.clone()));

    // Execute fill
    match executor::execute_fill(&intent, &profit_result).await {
        Ok(fill_result) => {
            // Record in money flow tracker
            money_flow.record_fill(FillRecord {
                intent_id: intent.id.clone(),
                protocol: intent.protocol.clone(),
                timestamp: Utc::now(),
                amount_usd: profit_result.amount_usd,
                protocol_fee_usd: profit_result.protocol_fee_usd,
                gas_cost_usd: fill_result.actual_gas_cost_usd,
                liquidity_cost_usd: 0.0, // TODO: Track for flash loans
                net_profit_usd: fill_result.actual_profit_usd,
                tx_hash: Some(fill_result.tx_hash),
            });

            // Emit: intent solved
            solver_api.emit_event(SolverEvent::IntentSolved(fill_result));

            tracing::info!(
                "✅ Filled {} - Profit: ${:.2}",
                intent.id,
                fill_result.actual_profit_usd
            );
        }
        Err(e) => {
            tracing::error!("❌ Failed to fill {}: {}", intent.id, e);
        }
    }
}
```

---

## Integration with protocols.xml

### Protocol Registry Sync

The existing `protocols.xml` in the spinner repo contains protocol event decoders.
We need to sync this data to `solver_intel.json`:

**Script**: `scripts/sync_protocols.sh`

```bash
#!/bin/bash
# Sync protocols.xml to solver_intel.json
# Run this whenever protocols.xml is updated

cd ~/projects/spinner/rust/crates/header-collector
python3 << 'EOF'
import xml.etree.ElementTree as ET
import json

tree = ET.parse('protocols.xml')
root = tree.getroot()

protocols = {}
for protocol in root.findall('.//protocol'):
    name = protocol.get('name')
    # Extract fee info from protocol config
    # (protocols.xml doesn't have fees yet, but we can add them)
    protocols[name] = {
        "fee_bps": 0,  # TODO: Add to protocols.xml
        "chains": [],  # TODO: Extract from protocol events
    }

# Merge with existing solver_intel.json
with open('~/projects/taifoon-solver/config/solver_intel.json', 'r') as f:
    solver_intel = json.load(f)

# Update protocols while preserving manual fee entries
for name, data in protocols.items():
    if name not in solver_intel.get('protocols', {}):
        solver_intel.setdefault('protocols', {})[name] = data

with open('~/projects/taifoon-solver/config/solver_intel.json', 'w') as f:
    json.dump(solver_intel, f, indent=2)

print(f"Synced {len(protocols)} protocols to solver_intel.json")
EOF
```

---

## Summary: Delivery Phases

### ✅ ALREADY COMPLETE
- Phase 1A: Genome stream client
- Phase 1B: Profit calculator
- Repo structure, Docker, GitHub

### 🎯 THIS SESSION
- Phase 2A: Solver API (port 8082)
- Phase 2B: Next.js dashboard (1-page)
- Phase 2C: Money flow tracking
- Phase 2D: Deploy to 88.99.1.32

### 📋 NEXT SESSION (2-3 days)
- Phase 3A: Executor implementation (LiFi first)
- Phase 3B: Reward claiming logic
- Phase 3C: First testnet fill
- Phase 3D: First mainnet fill
- Phase 3E: Net positive P&L

### 🚀 FUTURE
- Phase 4A: Flash loan integration (Aave, Uniswap)
- Phase 4B: T3RN LWC as liquidity source
- Phase 4C: Multi-path routing
- Phase 4D: MEV protection
- Phase 4E: Real-time gas oracles

---

## Implementation Order (This Session)

1. **Add solver-api crate** (30 min)
   - Create crate structure
   - Implement endpoints
   - Add SSE stream handler

2. **Wire into solver-main** (15 min)
   - Spawn API server on port 8082
   - Emit events in intent processing loop

3. **Create dashboard** (45 min)
   - Initialize Next.js app
   - Build components
   - Implement SSE hook
   - Style with Tailwind

4. **Update Docker & K8s** (15 min)
   - Expose port 8082
   - Update deployment.yaml

5. **Deploy to 88.99.1.32** (15 min)
   - Build solver binary
   - Build Docker image
   - Deploy to K8s
   - Verify API + dashboard

**Total Time**: ~2 hours

---

## Success Criteria

### API
- [ ] `/api/solver/stream` returns SSE events
- [ ] `/api/solver/stats` returns current stats
- [ ] All endpoints respond < 50ms

### Dashboard
- [ ] Loads in browser
- [ ] Shows live intent stream
- [ ] Updates stats every 5s
- [ ] Mobile responsive

### Deployment
- [ ] Running on 88.99.1.32:30082
- [ ] Accessible from public internet
- [ ] Logs visible via `kubectl logs`

---

**Let's build it.**
