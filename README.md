# Taifoon Solver

**Production cross-chain intent solver — Across V3 · deBridge DLN · Mayan Swift · LiFi**

Live on Base · Arbitrum · Optimism (EVM) and Solana mainnet. First fill confirmed
[2026-04-28 on Base](https://basescan.org/tx/0x262b9d65d30a973107775d5f94f7ea6a0101593e27b3d6a5869b24edca64180c).

---

## What it does

Taifoon is a Rust solver that watches intent streams from Across, deBridge, Mayan Swift, and
LiFi, evaluates profitability in real time, and fires fills on-chain — earning the spread.

```
Genome SSE stream ──► intent filter ──► profit check ──► lambda executor
                                                              │
                                              Across fillRelay│
                                           DLN fulfillOrder   │
                                       Mayan fulfillSimple    │
                                Mayan Solana sendTransaction  │
                                                              ▼
                                                     outcome SQLite
                                                         │
                                                  REST API + dashboard
```

Four real fill paths are live today:

| Protocol | Path | Status |
|----------|------|--------|
| Across V3 | `SpokePool.fillRelay` (EVM) | ✅ live fills |
| deBridge DLN | `DlnDestination.fulfillOrder` (EVM) | ✅ live fills |
| Mayan Swift | `fulfillSimple` / `fulfillOrder` + Wormhole VAA (EVM) | ✅ live fills |
| Mayan Swift | ed25519 `sendTransaction` (Solana) | ✅ live fills |
| LiFi | meta-router → underlying Across / deBridge / Mayan | ✅ live fills |

### LiFi-supported underlying protocols

LiFi is an aggregator. When a `LiFiTransferStarted` event arrives, we resolve the underlying
bridge from li.quest (authoritative) then dispatch to the matching fill adapter:

| LiFi `bridge` slug | Resolved as | Fill contract | Enrichment path |
|--------------------|-------------|---------------|-----------------|
| `across`, `across_v3` | Across V3 | `SpokePool.fillRelay` | V3FundsDeposited from tx receipt |
| `debridge`, `dln`, `debridge_dln` | deBridge DLN | `DlnDestination.fulfillOrder` | maker_order_nonce from order payload |
| `mayan`, `mayan_swift`, `mayanswift` | Mayan Swift | `MayanSwift.fulfillSimple` | order_id from Mayan poller |
| anything else (`stargate`, `hop`, …) | — | not filled | soft-skip → `RouteNotImplemented` |

**API lifecycle for a LiFi intent:**

```
1. Genome SSE emits LiFiTransferStarted (protocol="lifi", bridge="across")
2. execute.rs resolves bridge via genome metadata
3. li.quest /v1/status?txHash= → authoritative bridge slug + sending tx hash
      if Pending → retry next tick
      if NotRoutable → skip
4. LiFiMetaRouter::project_to_child(intent, "across")
      → rewrites protocol="across_v3", deposits sending_tx_hash as tx_hash
5. lambda_execute(child_intent) → full Across/deBridge/Mayan pipeline
6. Outcome logged under lifi→across:<tx_hash>
```

**Readiness checklist:**

| Check | Command | Expected |
|-------|---------|----------|
| li.quest resolution | `curl 'https://li.quest/v1/status?txHash=<tx>'` | `bridge: "across"` etc. |
| Across child fill | `PROTOCOL_FILTER=lifi DRY_RUN=true ./run-mainnet.sh` | `🔀 LiFi→across projection` in logs |
| deBridge child fill | same with debridge intent | `🔀 LiFi→debridge projection` |
| Mayan child fill | same with mayan intent | `🔀 LiFi→mayan projection` |
| Unknown bridge skip | — | `⏭️ LiFi bridge '...' not routable` |
| API outcome visible | `GET /api/solver/outcomes` | entry with `protocol="lifi→across"` |

---

## Colosseum Hackathon — What we built

### EVM ↔ Solana bidirectional fills

The Solana path is the core hackathon story. Mayan Swift orders that originate on Solana
(Solana → EVM) are detected by a polling loop, routed to a native-Rust ed25519 signer
loaded from macOS Keychain, and broadcast via `sendTransaction`. The fill lands on the
EVM destination chain within the Mayan fill window.

Matching path for EVM → Solana: the solver constructs a Mayan Swift fulfill instruction
(Anchor discriminator + account meta layout), signs it, and sends it via Solana JSON-RPC.

### Autonomous portfolio management

A portfolio sidecar runs alongside the solver. It monitors per-chain stablecoin and gas
balances, and when a fill chain falls below threshold it automatically fires an Across V3
bridge to top it up — no human in the loop.

```
Solver fills Base ──► Base USDC drains ──► sidecar detects LOW_FUNDS
                                                │
                                         Across depositV3
                                    (Arbitrum → Base, auto)
                                                │
                                       Base USDC restored
                                                │
                                         Solver resumes
```

If Across fails (e.g. fill window expired) the rebalancer falls back automatically to
deBridge DLN for the same route.

### Intent lifecycle — full state machine

Every intent is tracked through a typed state machine committed to SQLite:

```
DETECTED → PROFIT_CHECK → CALLDATA_BUILD → BROADCAST
                │                 │              │
         SKIP_UNPROFITABLE   CALLDATA_ERROR   PENDING
                                               │    │
                                          CONFIRMED  REVERTED
```

Claim tracking for deBridge is separate: `claim_tx_hash` and `claim_fee_usd` are written
back to the outcome record after the unlock transaction confirms.

### Key management — no tempfiles

EVM key: `messiah.rs` reads from macOS Keychain entry `mamba-messiah-key` into an
`alloy::PrivateKeySigner` and immediately `drop()`s the raw string.

Solana key: `keychain.rs` (new this session) mirrors the same pattern for
`mamba-messiah-solana-key` — native Rust `std::process::Command::output()`, no tempfiles,
no `std::fs` writes.

---

## Quick start

### Dependencies

- Rust 1.78+ (`rustup update stable`)
- Node 20+ and npm (for dashboard)
- macOS Keychain entries `mamba-messiah-key` (EVM) and `mamba-messiah-solana-key` (Solana)
  — see [SECURITY_ONBOARDING.md](./SECURITY_ONBOARDING.md) for setup

### Build

```bash
git clone https://github.com/yawningmonsoon/taifoon-solver
cd taifoon-solver
cargo build --release
```

### Dry run (safe — no broadcasts)

```bash
./run-mainnet.sh
# DRY_RUN=true by default — reads live genome stream, evaluates intents, logs decisions
```

### Live mode

```bash
DRY_RUN=false MAX_NOTIONAL_USD=50 ./run-mainnet.sh
```

### Dashboard

```bash
cd dashboard
npm install
npm run dev          # http://localhost:3000
```

The API server starts automatically with the solver on port 8082.

---

## Repository layout

```
crates/
  genome-client/           SSE stream consumer — Across poller, deBridge DLN poller,
                           Mayan Swift poller, LiFi meta-router
  executor/
    lambda_controller.rs   Intent state machine + all fill calldata builders
    outcome_log.rs         SQLite outcome + claim tracking
    across_executor.rs     Across V3 direct fill path
    mayan_evm_estimate.rs  Mayan EVM gas estimate + calldata
    mayan_solana_estimate.rs  Mayan Solana instruction builder
    skip_rules.rs          Rule engine for skipping intents
  portfolio-sidecar/
    rebalancer.rs          Across-primary / deBridge-fallback bridge dispatcher
    inventory.rs           Per-chain balance + Solana gas status classification
    tx_guard.rs            Pre-flight allowlist — only known contracts allowed
  protocol-adapters-solana/
    keychain.rs            Solana key loader (Keychain → in-memory, no tempfile)
    mayan_solana.rs        Solana broadcast path (sign + sendTransaction)
  solver-api/              Axum REST API — portfolio, rebalance, claims, outcomes
  solver-main/
    main.rs                Solve loop — genome events → lambda executor
    lifi_resolver.rs       LiFi status API fallback with retry backoff
    messiah.rs             EVM key loader (Keychain → in-memory)
  wallet-manager/          SQLite wallet state + deBridge claim list
  t3rn-sidecar/            T3RN LWC V4 sidecar (backup liquidity)
  taifoon-cli/             CLI — wallet, monitor, execute, sidecar subcommands
dashboard/
  app/portal/              Live P&L dashboard (SSE feed + REST polling)
  components/
    PortfolioPanel.tsx     Per-chain inventory view (USDC/WETH/gas, color-coded)
    ClaimsPanel.tsx        deBridge pending claims + manual retry
    LivePnL.tsx            Real-time fill stream
run-mainnet.sh             Production launcher (keychain, dry-run guard, balance check)
SECURITY_ONBOARDING.md    Key management, CI secrets patterns, fee docs
TESTNET_ONBOARDING.md     Base Sepolia + Solana Devnet setup
```

---

## API

All `/api/solver/*` endpoints require `Authorization: Bearer $SOLVER_API_TOKEN`.

```bash
GET  /api/solver/portfolio          # per-chain balances + Solana gas status
POST /api/solver/rebalance          # trigger manual rebalance cycle
GET  /api/solver/rebalancer/status  # last rebalance decision
GET  /api/solver/outcomes           # fill history (limit, offset)
GET  /api/solver/pnl                # P&L summary by protocol
GET  /api/solver/claims             # deBridge pending claims
POST /api/solver/claims/:id/retry   # retry a specific claim
GET  /health                        # unauthenticated healthcheck
```

---

## Tests

```bash
cargo test --workspace   # 74 tests, 0 failures
```

Key test coverage:

- Across calldata round-trip (selector, amounts, determinism)
- Lambda controller skip rules (12 scenarios)
- LiFi status body parser (7 scenarios including malformed hash)
- Solana gas classification (4 thresholds)
- deBridge claim row SQL queries (2 tests)
- Profit calculator (spread, fee, gas math)
- LWC sandbox integration (33 tests)

---

## Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `DRY_RUN` | `true` | No broadcasts when true |
| `MAX_NOTIONAL_USD` | `200` | Hard cap per fill |
| `MIN_PROFIT_USD` | `0.50` | Skip below this threshold |
| `PROTOCOL_FILTER` | `across,lifi` | Comma-separated protocol allow-list |
| `DST_CHAIN_FILTER` | `8453` | Only fill on these chains |
| `MAX_INPUT_USD` | `15` | Drop large intents above wallet capacity |
| `SOLVER_API_TOKEN` | — | Bearer token for API auth |
| `SOLANA_RPC_URL` | public endpoint | Helius or equivalent for production |
| `GENOME_SSE_URL` | `https://api.taifoon.dev/...` | Intent stream |
| `SIDECAR_INTERVAL_SECS` | `300` | Portfolio rebalance cadence |

Override per-chain inventory targets:
```bash
SIDECAR_MIN_STABLE_8453=100    # Base USDC floor
SIDECAR_TARGET_STABLE_42161=200
SIDECAR_MIN_GAS_8453=0.005
```

---

## Security

Private keys never touch disk. See [SECURITY_ONBOARDING.md](./SECURITY_ONBOARDING.md) for:
- macOS Keychain setup for both EVM and Solana keys
- GitHub Actions / HashiCorp Vault / AWS Secrets Manager patterns for CI/Linux
- deBridge manual claim recovery (`cast send claimUnlock`)
- Per-protocol fee structures and claim latencies

Testnet setup: [TESTNET_ONBOARDING.md](./TESTNET_ONBOARDING.md)

---

## User testing flow

Follow this sequence to validate the full solver from a fresh clone, mirroring the
solver.taifoon.dev installation guide:

### 1. Install

```bash
git clone https://github.com/yawningmonsoon/taifoon-solver
cd taifoon-solver
cargo build --release
```

### 2. Set keys (macOS Keychain)

```bash
security add-generic-password -a mamba-messiah-key -s mamba-messiah-key \
  -w "0x<your-evm-private-key>"
security add-generic-password -a mamba-messiah-solana-key -s mamba-messiah-solana-key \
  -w "<your-solana-base58-key>"
```

Skip the Solana key if not testing Mayan Solana fills.

### 3. Select chains and assets

Default inventory targets (overridable via env):

| Chain | Chain ID | Asset | Floor | Target |
|-------|----------|-------|-------|--------|
| Base | 8453 | USDC | $100 | $500 |
| Arbitrum | 42161 | USDC | $100 | $300 |
| Optimism | 10 | USDC | $50 | $200 |
| Ethereum | 1 | USDC | $50 | $100 |
| Polygon | 137 | USDC | $30 | $100 |

To override a chain's target:
```bash
export SIDECAR_MIN_STABLE_8453=200      # Base floor $200
export SIDECAR_TARGET_STABLE_42161=500  # Arb target $500
export DST_CHAIN_FILTER=8453,42161      # Only fill on Base and Arbitrum
```

### 4. Select protocols

```bash
export PROTOCOL_FILTER=across,lifi      # default — safest to start
# or:
export PROTOCOL_FILTER=all              # Across + deBridge + Mayan + LiFi
# or:
export PROTOCOL_FILTER=debridge         # deBridge only (requires manual claim monitoring)
```

### 5. Dry run — validate intent stream

```bash
DRY_RUN=true ./run-mainnet.sh
```

Expected log output within 30 s:
```
📡 Monitoring genome stream...
📥 across_v3:dep:12345 ... amt=50000000
⏭️  skipped: below min_profit_usd
```

If no intents appear in 60 s, check `GENOME_SSE_URL` and network access.

### 6. Check portfolio baseline

```bash
curl -s -H "Authorization: Bearer $SOLVER_API_TOKEN" \
  http://localhost:8082/api/solver/portfolio | jq .
```

Expected: per-chain `stable_usd`, `gas_eth`, `status` fields. All fill chains should
show `status: "healthy"` before enabling live mode.

### 7. Live mode — first fill

```bash
DRY_RUN=false MIN_PROFIT_USD=0.10 MAX_NOTIONAL_USD=20 ./run-mainnet.sh
```

Watch for:
```
🎉 CONFIRMED: across_v3:dep:12345 — tx 0x...
```

Verify on-chain: the logged tx hash should appear on the destination chain scanner.

### 8. Dashboard

```bash
cd dashboard && npm install && npm run dev
# open http://localhost:3000
```

Panels to verify: Live P&L stream, Portfolio (color-coded chain status), Claims (deBridge).

### 9. Manual rebalance

```bash
curl -s -X POST -H "Authorization: Bearer $SOLVER_API_TOKEN" \
  http://localhost:8082/api/solver/rebalance | jq .
```

Expected: `{"triggered":true,"actions":[...]}` — lists any bridge actions fired.

---

## Taifoon Intelligence — LLM role in the solver

The solver runs fully autonomously today: deterministic skip rules, numeric profit checks,
on-chain gas estimates. The question is where an LLM adds signal without adding latency.

### Recommended role: async hint engine (not inline decision-maker)

Keep the hot path (genome event → fill → on-chain) fully deterministic. LLM runs on a
separate goroutine/tick and writes advisory signals to a shared state store. The solver
reads from that store on each intent evaluation.

```
genome stream ──► intent filter ──► profit check ──► fill
                                         ▲
                                    advisory state
                                         ▲
                               LLM hint tick (async, ~30 s lag)
                                    ▲         ▲
                               market data   intent history
```

### Concrete LLM hints the solver can act on today

| Signal | How the solver uses it | Fallback if LLM is slow |
|--------|------------------------|-------------------------|
| `protocol_congestion: {debridge: high}` | Raise `min_profit_usd` for deBridge by 2× | Use static config |
| `chain_gas_spike: {arbitrum: 3x}` | Skip Arbitrum fills for next N seconds | Static gas cap |
| `fill_window_risk: {across: dep:1234}` | Boost urgency, skip gas-price gate | Time-based deadline |
| `route_quality: {lifi→debridge: 0.6}` | Raise min profit for low-quality routes | Route always permitted |
| `claim_stuck: [dep:1234, dep:5678]` | Trigger manual claim retry | outcome_log already tracks |

### What NOT to put in the hot path

Avoid LLM calls inline — a 300ms LLM response would time-out fill windows. The LLM
should never gate a single fill; it sets environment-level priors that decay with a TTL.

### Bootstrap config

```bash
export TAIFOON_INTEL_URL=http://localhost:8099  # hint service endpoint
export TAIFOON_INTEL_TTL_SECS=60               # stale hint expiry
export TAIFOON_INTEL_ENABLED=false             # off by default until tested
```

---

## License

MIT
