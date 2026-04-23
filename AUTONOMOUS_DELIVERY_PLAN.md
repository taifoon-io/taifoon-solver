---
title: Autonomous Delivery Plan - Complete E2E Solver with Dashboard
description: TamTam-orchestrated autonomous delivery of full profitable solver system
---

# Autonomous Delivery Plan

**Goal**: Complete end-to-end profitable solver with dashboard, supporting ALL protocols from protocols.xml, with T3RN LWC as optional liquidity sidecar.

**Method**: TamTam agent orchestration with 6 specialized agents working in sequence.

**Timeline**: 4-6 hours autonomous execution

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                     TamTam Orchestration                      │
│                    (localhost:1337)                           │
└─────────────────────────────────────────────────────────────┘
                              │
              ┌───────────────┼───────────────┐
              │               │               │
        ┌─────▼─────┐   ┌────▼────┐   ┌─────▼─────┐
        │  Agent 1  │   │ Agent 2 │   │  Agent 3  │
        │ Protocol  │   │   T3RN  │   │ Dashboard │
        │ Analysis  │   │ Sidecar │   │  Builder  │
        └───────────┘   └─────────┘   └───────────┘
              │               │               │
        ┌─────▼─────┐   ┌────▼────┐   ┌─────▼─────┐
        │  Agent 4  │   │ Agent 5 │   │  Agent 6  │
        │ Executor  │   │  E2E    │   │  Deploy   │
        │  Builder  │   │  Test   │   │  & Push   │
        └───────────┘   └─────────┘   └───────────┘
              │
        ┌─────▼─────────────────────────────────┐
        │    Complete Profitable Solver          │
        │  + Dashboard + T3RN Sidecar            │
        │  + Support for ALL protocols.xml       │
        └────────────────────────────────────────┘
```

---

## Agent Pipeline

### 🤖 Agent 1: Protocol Registry Analyzer

**Name**: `Protocol XML Analyzer`
**Skill**: `analyze-protocols-xml.md`
**Model**: Sonnet 4.5
**Estimated Time**: 15 minutes

**Task**:
1. Parse `/Users/mbultra/projects/spinner/rust/crates/header-collector/protocols.xml`
2. Extract all protocol information (names, chains, contracts, events)
3. Create `config/protocols_registry.json` with complete protocol data
4. Create `PROTOCOL_SUPPORT.md` with support matrix
5. Merge with existing `solver_intel.json` fee data

**Outputs**:
- ✅ `config/protocols_registry.json` (25+ protocols)
- ✅ `PROTOCOL_SUPPORT.md` (support matrix)
- ✅ Updated `config/solver_intel.json`

**Success Criteria**:
- All protocols from XML captured
- Contract addresses per chain extracted
- Event signatures documented
- JSON validates

---

### 🤖 Agent 2: T3RN LWC Sidecar Builder

**Name**: `T3RN Sidecar Implementor`
**Skill**: `build-t3rn-sidecar.md`
**Model**: Sonnet 4.5
**Estimated Time**: 30 minutes
**Depends On**: Agent 1 (protocols registry)

**Task**:
1. Create `crates/t3rn-sidecar/` with LWC integration
2. Copy ABIs from `/Users/mbultra/projects/t3rn-guardian`
3. Implement order creation, monitoring, and liquidity checks
4. Add testnet configuration (Base Sepolia ↔ Optimism Sepolia)
5. Prepare for mainnet deployment

**Outputs**:
- ✅ `crates/t3rn-sidecar/` (new crate)
- ✅ LWC ABI definitions
- ✅ Configuration for testnet + mainnet
- ✅ Integration points documented

**Success Criteria**:
- `cargo build --release` succeeds with new crate
- LWC order creation works on testnet
- Fallback logic implemented (Priority 3)

---

### 🤖 Agent 3: Dashboard Builder

**Name**: `Dashboard Frontend Architect`
**Skill**: `build-dashboard.md`
**Model**: Sonnet 4.5
**Estimated Time**: 45 minutes
**Depends On**: Agent 1 (protocol data for breakdown view)

**Task**:
1. Initialize Next.js 15 app in `dashboard/`
2. Implement all components (IntentsStream, PerformanceStats, ProtocolBreakdown, MoneyFlow)
3. Create SSE hook for real-time events
4. Configure Tailwind CSS with BRAND.md colors
5. Add protocol breakdown using protocols_registry.json
6. Test with mock data

**Outputs**:
- ✅ `dashboard/` (complete Next.js app)
- ✅ All components implemented
- ✅ SSE integration working
- ✅ Protocol breakdown with ALL 25+ protocols

**Success Criteria**:
- `npm run dev` starts successfully
- Dashboard loads at localhost:3000
- SSE connection shows "LIVE" status
- All protocols displayed in breakdown

---

### 🤖 Agent 4: Multi-Protocol Executor Builder

**Name**: `Executor Implementation Master`
**Skill**: `build-executor.md`
**Model**: Sonnet 4.5 (extended context)
**Estimated Time**: 60 minutes
**Depends On**: Agent 1 (protocols), Agent 2 (T3RN sidecar)

**Task**:
1. Implement executor in `crates/executor/src/lib.rs`
2. Add protocol-specific fill logic (LiFi, Stargate, Across first)
3. Integrate T3RN sidecar as Priority 3 liquidity source
4. Add safety features (SIMULATION_MODE, balance checks, gas limits)
5. Implement liquidity waterfall (own funds → flash loans → LWC)
6. Wire into solver-main with event emission

**Outputs**:
- ✅ `crates/executor/` (complete implementation)
- ✅ Multi-protocol support (LiFi, Stargate, Across ready)
- ✅ T3RN sidecar integration
- ✅ Safety features enforced
- ✅ solver-main updated with executor calls

**Success Criteria**:
- `cargo build --release` succeeds
- SIMULATION_MODE=true by default
- Events emitted for IntentSolved
- T3RN fallback logic works
- Logs show liquidity source decisions

---

### 🤖 Agent 5: E2E Integration Tester

**Name**: `Integration Test Orchestrator`
**Skill**: `test-e2e-solver.md` (to be created)
**Model**: Sonnet 4.5
**Estimated Time**: 30 minutes
**Depends On**: Agents 1-4 (all components)

**Task**:
1. Start solver in simulation mode
2. Start dashboard
3. Verify SSE connection
4. Send test intents via genome stream
5. Verify profit calculation
6. Verify executor attempts (simulation)
7. Verify dashboard updates in real-time
8. Test T3RN sidecar fallback logic
9. Generate test report

**Outputs**:
- ✅ `E2E_TEST_REPORT.md` with results
- ✅ Screenshots of working dashboard
- ✅ Simulation logs showing all components working

**Success Criteria**:
- Solver starts without errors
- Dashboard connects via SSE
- Test intents processed correctly
- All 3 liquidity priorities tested
- Dashboard shows real-time updates

---

### 🤖 Agent 6: Deployment & Documentation

**Name**: `Deployment Coordinator`
**Skill**: `deploy-and-document.md` (to be created)
**Model**: Sonnet 4.5
**Estimated Time**: 20 minutes
**Depends On**: Agent 5 (tests passing)

**Task**:
1. Update README.md with complete feature list
2. Update DELIVERY_PLAN.md with completion status
3. Create DEPLOYMENT.md with production instructions
4. Generate changelog
5. Git commit all changes
6. Push to GitHub
7. Create Docker deployment config
8. Generate operator runbook

**Outputs**:
- ✅ Updated documentation
- ✅ Git commit with comprehensive message
- ✅ Pushed to GitHub
- ✅ `DEPLOYMENT.md` (production guide)
- ✅ `RUNBOOK.md` (operator guide)

**Success Criteria**:
- All docs updated
- Clean git history
- Pushed to origin/master
- Production deployment ready

---

## Execution Plan

### Option A: Sequential Execution (Recommended for First Run)

Run agents one at a time, verify each before proceeding:

```
Agent 1 → Verify protocols_registry.json created
    ↓
Agent 2 → Verify t3rn-sidecar compiles
    ↓
Agent 3 → Verify dashboard runs
    ↓
Agent 4 → Verify executor compiles
    ↓
Agent 5 → Verify E2E test passes
    ↓
Agent 6 → Verify push to GitHub succeeds
```

**Total Time**: ~3-4 hours

### Option B: Parallel Execution (Advanced)

Run independent agents in parallel:

```
┌─ Agent 1 (Protocol Analysis) → Agent 4 (Executor)
│
├─ Agent 2 (T3RN Sidecar) ──────→ Agent 4 (Executor)
│
└─ Agent 3 (Dashboard) ──────────→ Agent 5 (E2E Test)
                                        ↓
                                   Agent 6 (Deploy)
```

**Total Time**: ~2 hours (with parallel execution)

---

## TamTam Configuration

### Create All Agents in TamTam

**Step 1**: Start TamTam
```bash
cd /Users/mbultra/projects/tamtam
pnpm dev
# Open http://localhost:1337
```

**Step 2**: Create Agents (via TamTam UI)

1. **Agent 1**: Protocol XML Analyzer
   - Project: taifoon-solver
   - Model: sonnet
   - Skill: Analyze protocols.xml for Solver Integration
   - Prompt: "Analyze spinner's protocols.xml and create complete protocol registry for solver"

2. **Agent 2**: T3RN Sidecar Implementor
   - Project: taifoon-solver
   - Model: sonnet
   - Skill: Build T3RN LWC Sidecar
   - Prompt: "Build T3RN LiquidityWellCompact sidecar as Priority 3 liquidity source"

3. **Agent 3**: Dashboard Frontend Architect
   - Project: taifoon-solver
   - Model: sonnet
   - Skill: Build Taifoon Solver Dashboard
   - Prompt: "Build complete Next.js dashboard with SSE integration and protocol breakdown"

4. **Agent 4**: Executor Implementation Master
   - Project: taifoon-solver
   - Model: sonnet
   - Skill: Build Taifoon Solver Executor
   - Prompt: "Implement multi-protocol executor with T3RN sidecar integration and safety features"

5. **Agent 5**: Integration Test Orchestrator
   - Project: taifoon-solver
   - Model: sonnet
   - Prompt: "Run complete E2E test: solver + dashboard + T3RN sidecar in simulation mode"

6. **Agent 6**: Deployment Coordinator
   - Project: taifoon-solver
   - Model: sonnet
   - Prompt: "Update all docs, commit, push to GitHub, and create deployment guide"

**Step 3**: Execute Pipeline

**Sequential**:
1. Run Agent 1 → Wait for completion → Verify output
2. Run Agent 2 → Wait → Verify
3. Run Agent 3 → Wait → Verify
4. Run Agent 4 → Wait → Verify
5. Run Agent 5 → Wait → Verify
6. Run Agent 6 → Done!

**Parallel** (advanced):
1. Run Agents 1, 2, 3 simultaneously
2. Wait for all to complete
3. Run Agent 4
4. Run Agent 5
5. Run Agent 6

---

## Monitoring & Verification

### TamTam Dashboard Views

1. **Terminal View**: Real-time token streaming from Claude
2. **Run History**: Track each agent's execution
3. **Logs**: Detailed output from each step
4. **Notifications**: Get alerts when agents complete

### Verification Checklist

After each agent:

**Agent 1**:
- [ ] `config/protocols_registry.json` exists
- [ ] Contains 25+ protocols
- [ ] `PROTOCOL_SUPPORT.md` created

**Agent 2**:
- [ ] `crates/t3rn-sidecar/` directory exists
- [ ] `cargo build --release` succeeds
- [ ] LWC ABI present

**Agent 3**:
- [ ] `dashboard/` directory exists
- [ ] `npm run dev` starts
- [ ] Loads at localhost:3000

**Agent 4**:
- [ ] `crates/executor/src/lib.rs` updated
- [ ] `cargo build --release` succeeds
- [ ] T3RN integration present

**Agent 5**:
- [ ] `E2E_TEST_REPORT.md` created
- [ ] All tests passing
- [ ] Screenshots generated

**Agent 6**:
- [ ] Git commit successful
- [ ] Pushed to GitHub
- [ ] Docs updated

---

## Expected Deliverables

### Code

```
taifoon-solver/
├── crates/
│   ├── genome-client/       ✅ Existing
│   ├── profit-calc/         ✅ Existing
│   ├── solver-api/          ✅ Existing
│   ├── t3rn-sidecar/        🆕 NEW (Agent 2)
│   ├── executor/            🆕 COMPLETE (Agent 4)
│   └── solver-main/         🔄 UPDATED (Agent 4)
├── dashboard/               🆕 NEW (Agent 3)
│   ├── app/
│   ├── components/
│   └── hooks/
├── config/
│   ├── solver_intel.json    ✅ Existing
│   └── protocols_registry.json 🆕 NEW (Agent 1)
├── BRAND.md                 ✅ Existing
├── DELIVERY_PLAN.md         ✅ Existing
├── PROTOCOL_SUPPORT.md      🆕 NEW (Agent 1)
├── E2E_TEST_REPORT.md       🆕 NEW (Agent 5)
├── DEPLOYMENT.md            🆕 NEW (Agent 6)
└── RUNBOOK.md               🆕 NEW (Agent 6)
```

### Features

**✅ Phase 1**: Genome stream consumer (COMPLETE)
**✅ Phase 2**: Profit calculator (COMPLETE)
**✅ Phase 2.5**: Solver API + SSE (COMPLETE)
**🆕 Phase 3**: Multi-protocol executor (DELIVERED by Agent 4)
**🆕 Phase 4**: Dashboard frontend (DELIVERED by Agent 3)
**🆕 Phase 5**: T3RN LWC sidecar (DELIVERED by Agent 2)
**🆕 Phase 6**: E2E testing (DELIVERED by Agent 5)
**🆕 Phase 7**: Production deployment (DELIVERED by Agent 6)

### Protocol Support

- **Total Protocols**: 25+ (from protocols.xml)
- **Tier 1 Ready**: LiFi V2, Stargate V2, Across V3
- **Tier 2 Planned**: deBridge, Hop, Synapse, +9 more
- **Tier 3 Future**: Remaining protocols
- **T3RN LWC**: As liquidity fallback for ALL protocols

---

## Success Metrics

### Technical

- [ ] All 6 agents complete successfully
- [ ] `cargo build --release` succeeds
- [ ] `npm run dev` works (dashboard)
- [ ] E2E test passes
- [ ] Git push succeeds

### Functional

- [ ] Solver detects intents from genome stream
- [ ] Profit calculated for ALL protocols
- [ ] Executor attempts fills (simulation)
- [ ] T3RN sidecar available as fallback
- [ ] Dashboard shows real-time updates
- [ ] All 25+ protocols visible in dashboard

### Production-Ready

- [ ] SIMULATION_MODE enabled by default
- [ ] Safety features enforced
- [ ] Documentation complete
- [ ] Deployment guide ready
- [ ] Operator runbook created

---

## Next Steps After Autonomous Delivery

1. **Testnet Testing** (1-2 days)
   - Deploy to Base Sepolia + Optimism Sepolia
   - Execute real fills with T3RN LWC
   - Verify profit tracking

2. **Mainnet Rollout** (1 week)
   - Start with SIMULATION_MODE=true
   - Monitor for 24h
   - Switch to real execution with $1 min profit
   - Gradually increase limits

3. **Optimization** (ongoing)
   - Add flash loan integration (Aave, Uniswap)
   - Optimize gas estimation
   - Add MEV protection
   - Expand protocol support to Tier 2+

---

**Total Autonomous Delivery Time**: 2-4 hours (parallel) or 3-6 hours (sequential)

**Result**: Complete, profitable, production-ready solver with dashboard and T3RN sidecar, supporting ALL protocols from protocols.xml!

🚀 **Let TamTam do the work. You monitor the dashboard.**
