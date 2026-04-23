# Taifoon Solver Brand

**Tagline**: The best solver in the industry for all cross-chain bridge protocols

## Identity

**Name**: Taifoon Solver
**Owner**: yawningmonsoon
**Repository**: https://github.com/yawningmonsoon/taifoon-solver
**Mission**: Execute profitable cross-chain intents across 25+ protocols with millisecond latency

## Visual Identity

### Colors
- **Primary**: `#00D9FF` (Cyan) - Speed, precision, liquidity
- **Secondary**: `#1A1A1A` (Dark) - Professional, serious
- **Accent**: `#00FF88` (Green) - Profit, success
- **Warning**: `#FFB800` (Amber) - Caution, gas costs
- **Error**: `#FF3366` (Red) - Loss, failure

### Typography
- **Headings**: Inter Bold
- **Body**: Inter Regular
- **Code**: JetBrains Mono

## Dashboard Philosophy

**Principle**: Transparency through simplicity

The solver is a black box to the outside world, but a glass box to the operator.
Every intent, every calculation, every decision is visible in real-time.

### 1-Page Dashboard Layout

```
┌─────────────────────────────────────────────────────────────┐
│  Taifoon Solver                      🟢 LIVE    Net: $432   │
├─────────────────────────────────────────────────────────────┤
│                                                               │
│  INTENTS STREAM (Real-time)          PERFORMANCE             │
│  ┌────────────────────────────┐     ┌──────────────────┐    │
│  │ 📥 lifi_v2 #4521           │     │ Latency: 127ms   │    │
│  │ ETH → Arb • 10K USDC       │     │ Success: 94.2%   │    │
│  │ Profit: $43.90 ✅          │     │ Total: 1,247     │    │
│  │ Status: EXECUTED           │     │ Profitable: 823  │    │
│  │ Gas: $5.10 • Fee: $49.00   │     │ Skipped: 424     │    │
│  ├────────────────────────────┤     └──────────────────┘    │
│  │ 📥 stargate_v2 #4522       │                              │
│  │ Base → OP • 50K USDC       │     PROTOCOL BREAKDOWN       │
│  │ Profit: $0.83 ⏭️           │     ┌──────────────────┐    │
│  │ Status: SKIP (< $1)        │     │ LiFi: 432 fills  │    │
│  │ Gas: $0.15 • Fee: $1.00    │     │ Stargate: 89     │    │
│  └────────────────────────────┘     │ Across: 12       │    │
│                                      │ T3RN LWC: 7      │    │
│  MONEY FLOW                          └──────────────────┘    │
│  ┌────────────────────────────┐                              │
│  │ Protocol Fees: +$12,458    │     TOP INTENTS (24h)        │
│  │ Gas Costs: -$2,341         │     ┌──────────────────┐    │
│  │ Liquidity Costs: -$124     │     │ 1. $143 (lifi)   │    │
│  │ Net Profit: +$9,993        │     │ 2. $89 (across)  │    │
│  │ ROI: 426%                  │     │ 3. $67 (lifi)    │    │
│  └────────────────────────────┘     └──────────────────┘    │
│                                                               │
└─────────────────────────────────────────────────────────────┘
```

### Data Sections

1. **Header**: Status indicator, net profit today
2. **Intents Stream**: Live feed of all intents (read/attempted/solved)
3. **Performance**: Latency, success rate, totals
4. **Protocol Breakdown**: Fills per protocol
5. **Money Flow**: Detailed P&L breakdown
6. **Top Intents**: Highest profit opportunities in last 24h

## Tech Stack

### Dashboard
- **Framework**: Next.js 15 (App Router)
- **Styling**: Tailwind CSS
- **Real-time**: Server-Sent Events (SSE)
- **Charts**: Recharts (minimal, only for profit trends)
- **Deployment**: Vercel (or self-hosted alongside solver)

### Solver API
- **Language**: Rust (Axum)
- **Port**: 8082 (solver API, separate from DA API)
- **Endpoints**:
  - `GET /api/solver/stream` - SSE feed of all intents
  - `GET /api/solver/stats` - Current stats (profit, success rate, etc.)
  - `GET /api/solver/intents` - Recent intents list
  - `GET /api/solver/protocols` - Protocol breakdown
  - `GET /api/solver/money-flow` - P&L breakdown

## Key Metrics

### Intent States
1. **READ**: Intent detected from genome stream
2. **ATTEMPTED**: Profitability calculated, decision made
3. **SOLVED**: Successfully executed on-chain

### Success Criteria
- **Latency**: < 200ms from intent detection to execution decision
- **Success Rate**: > 95% of attempted fills succeed
- **Profitability**: > 80% of detected intents are profitable (> $1)
- **Net P&L**: Positive every 24h period

## Voice and Tone

**Professional**: This is a money-making machine, not a toy
**Transparent**: Every decision is logged and visible
**Confident**: "The best solver in the industry" - backed by data
**Precise**: Numbers matter - show decimals, show gas costs, show everything

## Launch Checklist

- [ ] BRAND.md documented
- [ ] Dashboard deployed (1-page, real-time)
- [ ] Solver API live (port 8082)
- [ ] SSE stream working
- [ ] First profitable fill executed
- [ ] Net positive P&L for 24h
- [ ] README updated with dashboard URL
- [ ] Screenshot added to repo

---

**Status**: Ready to implement
**Timeline**: 1 session (dashboard) + 2-3 days (executor)
