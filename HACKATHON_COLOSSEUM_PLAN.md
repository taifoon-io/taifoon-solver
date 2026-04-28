# Taifoon × Solana Colosseum Hackathon — Master Plan

**Theme**: One big demo of Taifoon powers — end-to-end solver lifecycle on Solana + Base, tracked under open-mamba.

**Phases**: 6 sequential, each tracked as a mamba job.
**Testnet-first scope**: Base Sepolia ↔ Solana Devnet only, until V5 proofs are verified per-network.

---

## Phase 1 — Solver Onboarding (CLI + Sidecar k8s)

### 1a. CLI Onboarding Tool

Every solver operator starts here. Full traceability from first command.

**Deliverables:**
- `taifoon-cli onboard` command that:
  - Generates or imports a solver wallet (keypair)
  - Registers the wallet on-chain with the Taifoon Registry (Base Sepolia + Solana Devnet)
  - Prints a `solver_id`, a `SOLVER_PRIVATE_KEY` env var, and a `CHAIN_WIRING_JSON` snippet ready to paste
  - Writes a `~/.taifoon/solver.toml` config file
  - Emits a traceable `onboard` genome event (feed back into open-mamba as job `onboard:<solver_id>`)

**Status of existing `taifoon-cli`**: compiles, has `wallet` + `execute` + `monitor` subcommands; `onboard` is missing. Add it.

**File**: `crates/taifoon-cli/src/commands/onboard.rs`

### 1b. K8s Sidecar at solver.taifoon.dev

Each registration spawns an individual solver pod.

**Trigger sources** (priority order):
1. **taifoon.dev web UI** — user registers via frontend → webhook → open-mamba → k8s job
2. **taifoon-intel LLM** — LLM agent registers a solver hand (lower priority queue)
3. **Direct integration** — operator calls `POST /api/solvers/register` directly → registers

**K8s manifest**: one `Deployment` per solver, namespace `solvers`, labeled `solver_id=<id>`.
Each pod runs `solver-main` with env vars `SOLVER_PRIVATE_KEY`, `CHAIN_WIRING_JSON`, `GENOME_SSE_URL`.

**Webhook flow**:
```
taifoon.dev register form
  → POST /api/solvers/register (taifoon-mamba backend)
  → open-mamba ingest {project: "solvers", payload: {solver_id, wallet, chains}}
  → mamba worker dispatches k8s apply
  → pod running within 60s
```

**Reference files:**
- `spinner/K8S_DEPLOYMENT_STATUS.md`
- `spinner/AGENT_DEPLOYMENT.md`

---

## Phase 2 — Wallet Lifecycle Manager

**This is the first concrete build item.** Everything downstream depends on knowing the state of in-progress intents.

### State Machine

```
INTENT_DETECTED
  → PROFITABILITY_CHECK (spinner test-run)
      → SKIP_UNPROFITABLE
      → PROOF_FETCH (spinner v5 proof bundle)
          → PROOF_MISSING
          → CALLDATA_BUILD
              → CALLDATA_ERROR
              → BROADCAST
                  → PENDING_CONFIRMATION
                      → CONFIRMED ✓  (success)
                      → REVERTED ✗   (retry or abandon)
                          → RETRY_QUEUED
```

### Wallet State Tracker

**New crate**: `crates/wallet-manager/`

Responsibilities:
- Tracks USDC + native token balance per chain (Base Sepolia, Solana Devnet)
- Tracks in-flight intents: one SQLite row per intent keyed by `intent_id`
- Exposes `GET /api/wallet/status` → balances + in-flight count
- Exposes `GET /api/wallet/intents` → full lifecycle history
- Emits balance-low warnings (< $50 USDC) so the operator can refill

**Columns in `intents` table**:
```sql
intent_id TEXT PRIMARY KEY,
protocol  TEXT,
src_chain INTEGER,
dst_chain INTEGER,
amount_usd REAL,
state     TEXT,  -- matches state machine above
created_at DATETIME,
updated_at DATETIME,
tx_hash   TEXT,
error     TEXT
```

Every state transition is written here before the external call is made → crash-safe.

---

## Phase 3 — taifoon-arb Portfolio + Cross-Chain Consolidation

**Connects to `taifoon-arb` project** (consolidate feature).

- After a fill is confirmed on the destination chain, the solver's USDC sits on that chain.
- `taifoon-arb` consolidation: automatically bridges idle USDC back from Base Sepolia → Solana Devnet (or vice versa) when balance on one side exceeds threshold.
- Trigger: `wallet-manager` emits `balance_high` event → open-mamba job → taifoon-arb executes bridge tx.
- For hackathon demo: Solana ↔ Base only, manual threshold set in config.

**Key dependency**: taifoon-arb must have a working bridge call for Solana ↔ Base. Check `taifoon-arb/` for existing bridge adapter.

---

## Phase 4 — Lambda Controller (Intent Lifecycle State Machine)

**The core of the hackathon demo**.

Implements the full order lifecycle referenced in "Protocol Solver Integration Research":

```
lambda_execute(intent_id):
  1. fetch intent from genome SSE
  2. wallet_manager.reserve(intent.amount)
  3. spinner.test_run(protocol, intent) → go/no-go
  4. spinner.fetch_v5_proof(intent_id) → proof_blob
  5. build_adapter_calldata(intent) → calldata
  6. broadcast executeWithProof(proof_blob, adapter, calldata)
  7. wait_for_receipt → CONFIRMED | REVERTED
  8. if CONFIRMED: wallet_manager.release(intent.amount) + emit genome feedback event

lambda_claim(intent_id):
  1. check CONFIRMED state in wallet_manager
  2. call claim() on Taifoon Universal Operator
  3. credits solver wallet with solver fee
  4. wallet_manager.record_revenue(fee_usd)
```

**Files to create/modify:**
- `crates/executor/src/lambda_controller.rs` — the state machine
- `crates/executor/src/across_executor.rs` — fix ABI mismatch (see below)
- `crates/solver-main/src/main.rs` — wire lambda_controller in place of legacy executor

### Critical Bug Fixes Required First (from STATE_OF_THE_UNION)

| Bug | File | Fix |
|-----|------|-----|
| ABI mismatch `uint32` vs `int64` depositId | `executor/src/across_executor.rs:36-42` | Change to `int64 depositId` in sol! block |
| `parse_deposit_id` returns wrong type | `across_executor.rs:326` | Return `Option<i64>` |
| `outputAmount` set to `inputAmount` | `across_executor.rs:307` | Read from genome event field |
| genome-client test fixture stale | `genome-client/src/lib.rs:301-315` | Rename `ref`→`ref_hash`, `token`→`src_token`, etc. |
| `Intent` missing `deposit_id` field | `genome-client/src/lib.rs:49-73` | Add `pub deposit_id: Option<i64>` |

---

## Phase 5 — Spinner SYNC + V5 Proof Feedback Loop

**Sandboxed testing setup** — run spinner + solver together locally or on `46.4.96.124`.

### Sandbox Setup

```bash
# Terminal 1: spinner (genome SSE + v5 proof API)
cd ~/projects/spinner
cargo run --bin spinner-api -- --chains base-sepolia,solana-devnet

# Terminal 2: solver (listens to spinner SSE)
cd ~/projects/taifoon-solver
GENOME_SSE_URL=http://localhost:30081/api/genome/subscribe/sse \
SOLVER_PRIVATE_KEY=<test-key> \
DRY_RUN=false \
cargo run --bin solver-main
```

### V5 Proof Feedback Loop

After a successful `executeWithProof` broadcast:
1. The tx receipt contains the V5 proof commitment hash
2. Spinner's SYNC operation picks it up and creates a genome feedback event:
   `{event: "genome", data: {entity: "proof", action: "confirmed", ref_hash: "<intent_id>"}}`
3. This event re-enters the genome SSE → solver ignores it (not a fill intent) but logs it
4. **For testnet**: v5 proof submitted to Taifoon Devnet (not mainnet) to avoid gas costs
5. Universal Operator tested separately after per-network sandbox validation passes

### Per-Network Validation Order
1. **Base Sepolia** — EVM, most similar to existing Across path, lowest risk
2. **Solana Devnet** — different execution model, test Mayan/Wormhole adapters here
3. Mainnet only after both devnets pass

---

## Phase 6 — Diligent Tracking Under open-mamba

**Every phase above is a mamba job.** No phase is "done" until its mamba job is `CONFIRMED`.

### Job Schema

```json
{
  "project": "hackathon-colosseum",
  "assigned_agent": "coder",
  "model": "claude-opus-4-7",
  "payload": {
    "phase": "P1a",
    "description": "CLI onboard command",
    "deliverable": "taifoon-cli onboard subcommand compiles + registers wallet on Base Sepolia devnet",
    "success_criteria": "cargo test --workspace green + demo tx hash from testnet"
  },
  "priority": 1
}
```

### Mamba Schedule

| Job ID | Phase | Description | Depends On |
|--------|-------|-------------|------------|
| `col-p1a` | 1a | CLI onboard command | — |
| `col-p1b` | 1b | K8s sidecar + webhook registration | col-p1a |
| `col-p2` | 2 | Wallet lifecycle manager + state machine | col-p1a |
| `col-p2-bugs` | 4 prereq | Fix 5 critical bugs in across_executor | — |
| `col-p3` | 3 | taifoon-arb consolidation (Solana↔Base) | col-p2 |
| `col-p4` | 4 | Lambda controller (execute + claim) | col-p2-bugs, col-p2 |
| `col-p5` | 5 | Spinner SYNC sandbox + V5 feedback loop | col-p4 |
| `col-p5-sol` | 5 | Solana Devnet per-network validation | col-p5 |
| `col-p6` | 6 | Mamba tracking + dashboard | col-p1a |

### Ingest Command (run once to register all jobs)

```bash
for phase in col-p1a col-p2-bugs col-p6; do
  curl -X POST http://localhost:1337/ingest \
    -H 'content-type: application/json' \
    -d "{\"project\":\"hackathon-colosseum\",\"assigned_agent\":\"coder\",\"model\":\"claude-opus-4-7\",\"payload\":\"$phase\",\"priority\":1}"
done
```

---

## Testnet Token Checklist

Before anything broadcasts:

- [ ] Base Sepolia ETH — faucet: `https://www.alchemy.com/faucets/base-sepolia`
- [ ] Base Sepolia USDC — faucet or mint from Circle testnet USDC
- [ ] Solana Devnet SOL — `solana airdrop 2 <wallet> --url devnet`
- [ ] Solana Devnet USDC — `spl-token create-token` or Circle devnet faucet
- [ ] Confirm Across SpokePool deployed on Base Sepolia (`0x...`)
- [ ] Confirm Taifoon Universal Operator V5 deployed on Base Sepolia + Solana Devnet

---

## Demo Script (Hackathon Submission)

1. `taifoon-cli onboard` → solver registered, pod spawned on solver.taifoon.dev
2. Open-mamba dashboard at `localhost:1337` — shows solver job queue live
3. Submit a test intent (Base Sepolia → Solana Devnet, 10 USDC via Across/Mayan)
4. Genome SSE picks it up → lambda controller runs the state machine → V5 proof fetched → executeWithProof broadcast
5. Receipt confirmed → claim() called → solver wallet receives fee
6. taifoon-arb consolidation: 10 USDC auto-bridged back to starting chain
7. Wallet lifecycle dashboard shows full intent history with tx hashes

**The demo is the grant application made executable.**
