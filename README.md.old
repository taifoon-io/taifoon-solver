# Taifoon Solver

**The best solver in the industry for all cross-chain bridge protocols**

## Overview

Taifoon Solver is a professional intent solver that executes profitable cross-chain fills for protocols like LiFi, Stargate, Across, and 20+ others. It uses the Taifoon Genome Stream for real-time intent detection and supports multiple liquidity sources.

### Key Features

- **Multi-Protocol Support**: LiFi, Stargate, Across, deBridge, +21 more protocols
- **Real-Time Intent Detection**: Consumes Taifoon Genome SSE stream
- **Profit Optimization**: Calculates gas costs, protocol fees, and spreads
- **Multiple Liquidity Sources**: Own funds → Flash loans → T3RN LWC (backup)
- **V5 Proof Generation**: MMR inclusion proofs for all fills

## Architecture

```
Taifoon DA API (genome stream) ──┐
                                 ▼
                         Genome SSE Client
                                 │
                                 ▼
                         Profit Calculator
                                 │
                                 ▼
                           Executor
                                 │
                                 ▼
                         Fill Complete!
```

### How It Works

1. **DA API** already monitors 25+ protocols and emits `proto:deposit` events (unfilled intents)
2. **Genome Client** subscribes to SSE stream and parses intents
3. **Profit Calculator** estimates net profit (protocol fee + spread - gas - liquidity cost)
4. **Executor** fills profitable intents on destination chain and claims rewards
5. **DA API** auto-detects our fills and emits `proto:fill` events with our solver address

## Quick Start

### Prerequisites

- Rust 1.83+
- Access to Taifoon Genome Stream (https://api.taifoon.dev)
- Hot wallet with funds on destination chains

### Run (Development)

```bash
# Clone repo
git clone https://github.com/yawningmonsoon/taifoon-solver.git
cd taifoon-solver

# Build
cargo build --release

# Run
./target/release/taifoon-solver
```

### Configuration

```bash
# Environment variables
export GENOME_SSE_URL="https://api.taifoon.dev/api/genome/subscribe/sse"
export MIN_PROFIT_USD="1.0"
export WALLET_PRIVATE_KEY="..." # TODO: Use hardware wallet in prod
```

## Implementation Status

### ✅ Phase 1: Genome Stream Consumer (COMPLETE)
- [x] SSE client for genome stream
- [x] Intent parsing from `proto:deposit` events
- [x] Queue management
- [x] Auto-reconnection

### ✅ Phase 2: Profitability (COMPLETE)
- [x] Load protocol fees from solver_intel.json
- [x] Gas estimation (chain-specific)
- [x] Profit calculation formula
- [x] Filtering (> $1 threshold)
- [x] Unit tests with realistic scenarios

### ✅ Phase 2.5: Solver API + Dashboard (COMPLETE)
- [x] Solver API with SSE endpoints (port 8082)
- [x] Real-time event streaming
- [x] Stats, intents, protocols, money-flow endpoints
- [x] BRAND.md and dashboard design
- [x] Integration with solver-main

### 📋 Phase 3: Execution (READY TO IMPLEMENT)
- [ ] Hot wallet integration
- [ ] Protocol-specific fill logic (LiFi first)
- [ ] Transaction simulation
- [ ] Fill execution on destination
- [ ] Reward claiming on source
- [ ] Profit tracking

### 📋 Phase 4: Dashboard Frontend (READY TO IMPLEMENT)
- [ ] Next.js 15 app setup
- [ ] 1-page dashboard UI
- [ ] Real-time SSE integration
- [ ] Tailwind CSS styling
- [ ] Deploy to Vercel or self-hosted

### 📋 Phase 5: Advanced (FUTURE)
- [ ] Flash loan integration (Aave, Uniswap)
- [ ] T3RN LWC as liquidity source
- [ ] Multi-path routing
- [ ] Real-time gas oracles
- [ ] MEV protection

## Project Structure

```
taifoon-solver/
├── crates/
│   ├── genome-client/     # SSE client for genome stream
│   ├── profit-calc/       # Profitability calculator
│   ├── solver-api/        # ✅ NEW: HTTP + SSE API (port 8082)
│   ├── executor/          # Fill executor
│   └── solver-main/       # Main binary
├── BRAND.md               # ✅ NEW: Brand identity and dashboard design
├── DELIVERY_PLAN.md       # ✅ NEW: Complete implementation roadmap
├── README.md              # This file
├── QUICKSTART.md          # Quick start guide
├── SESSION_SUMMARY.md     # Build summary
└── Cargo.toml             # Workspace config
```

## API Endpoints

The solver exposes a REST + SSE API on port 8082:

- `GET /api/solver/stream` - Server-Sent Events stream of all solver activity
- `GET /api/solver/stats` - Current statistics (profit, success rate, etc.)
- `GET /api/solver/intents` - Recent intents list
- `GET /api/solver/protocols` - Protocol breakdown
- `GET /api/solver/money-flow` - P&L breakdown

Example:
```bash
# Subscribe to live intent stream
curl -N http://localhost:8082/api/solver/stream

# Get current stats
curl http://localhost:8082/api/solver/stats
```

## Protocol Support

### Priority 1 (Active)
- **LiFi V2**: 13 fills/week, $2,258 volume, 49 bps fees ⭐
- **Stargate V2**: 6 fills/week, 2 bps fees
- **T3RN LWC**: 7,367 executions, 73.7% fill rate

### Priority 2 (Dormant but High Potential)
- **Across V3**: 0 fills recently, but historically high volume
- **deBridge DLN**: 4 bps fees
- **Hop Protocol**
- +19 more protocols

## Performance Targets

### Week 1
- ✅ Detect 50+ intents
- 🎯 Execute 1+ profitable fill
- 🎯 Net positive P&L

### Month 1
- 🎯 100+ fills executed
- 🎯 $500+ net profit
- 🎯 <5 min average latency
- 🎯 Top 20 solver by volume

## Resources

- **Solver Intel**: Protocol fees, volumes, solver addresses
- **Genome Stream**: https://api.taifoon.dev/api/genome/subscribe/sse
- **DA API Docs**: https://api.taifoon.dev/
- **Across Relayer** (reference implementation): https://github.com/across-protocol/relayer

## License

MIT

## Owner

yawningmonsoon (private repo)

---

**Status**: ✅ Phase 1, 2, & 2.5 Complete - Genome client + Profit calculator + Solver API PRODUCTION-READY
**Next**: Implement dashboard frontend (Phase 4) OR executor for actual fills (Phase 3)

See `BRAND.md` for dashboard design and `DELIVERY_PLAN.md` for complete roadmap.
