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

## License

MIT
