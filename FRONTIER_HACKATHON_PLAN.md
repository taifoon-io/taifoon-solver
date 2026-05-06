# Taifoon × Frontier Hackathon — Full E2E Delivery Plan

**Date:** 2026-05-05
**Owner:** MB ([@yawningmonsoon](mailto:maciej@t3rn.io))
**Status:** Active. Supersedes `HACKATHON_COLOSSEUM_PLAN.md`.
**Operating model:** Loopback agent system (coding + review subagents per phase).

---

## 0. North Star

A live, mainnet-broadcasting solver that fills real cross-chain intents on **EVM↔Solana** through **Across, deBridge, LiFi (meta-router), and Mayan**, with a **per-intent trace + running P&L dashboard** visible during the demo. Every claim is backed by an on-chain mainnet tx hash recorded in the outcome log.

The plan is structured for **autonomous delivery in loops**: each phase is a self-contained unit a coding agent can implement, a review agent can verify, and the user can rubber-stamp on a real chain.

---

## 1. Scope (frozen 2026-05-05)

| Decision | Value |
|---|---|
| Target | Mainnet, real funds, capped notional `MAX_NOTIONAL_USD=200`, profit gate `MIN_PROFIT_USD=0.50` |
| Solana role | EVM ↔ SOL bidirectional |
| Bridges | Across V3, deBridge DLN, LiFi (meta-router → underlying), Mayan Swift (Solana side) |
| Trace | rusqlite outcome_log + live web dashboard with running P&L |
| Time budget | A few days, demo-grade |

Out of scope: Stargate, Connext, Hop, Wormhole, Synapse, T3RN Operator V5 proof path (chain wiring uses `operator=0x0` everywhere → all Across fills are direct-SpokePool), Mayan EVM (gated until MB completes Mayan Discord registration).

---

## 2. Current state (audited 2026-05-05 against `HEAD = 83243c5`)

| Component | Status |
|---|---|
| `lambda_controller::lambda_execute` (live broadcast path) | READY, branches per protocol |
| Across V3 direct-fill (`SpokePool.fillV3Relay`) | READY: spread guard, deadline guard, message-hook skip, Solana-source skip |
| deBridge `DlnDestination.fulfillOrder` | READY: estimate calldata aligned with broadcast, spread guard for cross-decimal orders |
| LiFi meta-router (`LiFiMetaRouter::project_to_child`) | READY: dispatches to Across/deBridge/Mayan child intent, li.quest sending-tx fallback |
| Mayan Solana broadcast (`SolanaBroadcaster::send_fulfill`) | READY but unverified live: ed25519 signing, raw JSON-RPC, VAA retry to 24 attempts |
| Mayan EVM | GATED at `mayan_evm_solver_not_registered` until MB registers with Mayan |
| `OutcomeLog::recent()` + `pnl_summary()` | DONE in this session (2026-05-05) |
| `MAX_NOTIONAL_USD` hard cap in lambda controller | DONE in this session (2026-05-05) |
| `run-mainnet.sh` runbook | DONE in this session (2026-05-05) |
| `/api/solver/outcomes` + `/api/solver/pnl` HTTP routes | TODO |
| Dashboard `<LivePnL/>` panel | TODO |
| Mainnet first-fill verification gates | TODO (this plan) |

---

## 3. Loopback agent operating model

Each phase below is delivered by a **coding ↔ review agent pair**. The user is the third agent in the loop — they alone run the on-chain mainnet verification (because the user holds the keys; coding agents do not move funds).

```
┌──────────────────────────┐    diff/edit     ┌──────────────────────────┐
│   coding-agent (Sonnet)  │ ───────────────▶ │   review-agent (Opus)    │
│   reads phase scope.md   │                  │   runs gate check.sh     │
│   writes/edits files     │ ◀─────────────── │   PASS / FAIL + diag     │
└──────────────────────────┘    verdict       └──────────────────────────┘
              ▲                                            │
              │ FAIL → re-prompt                           │ PASS
              │                                            ▼
              │                              ┌────────────────────────────┐
              └──────────────────────────────│   user (MB) — broadcast    │
                                             │   the verification tx,     │
                                             │   paste hash, close phase  │
                                             └────────────────────────────┘
```

Loop driver, prompt templates, and gate scripts live under `tools/loops/` (Phase 0 deliverable). Every phase exits only when its **on-chain verification gate** has a recorded mainnet tx hash in `outcomes/verification_gates.jsonl`.

---

## 4. Phases

Each phase has: **deliverable**, **acceptance gate** (reproducible without keys), **mainnet verification gate** (requires MB's keys on a real chain), **coding agent prompt**, **review agent prompt**.

### Phase 0 — Bench: build green, tests green, loops armed

**Deliverable:**
- `cargo build --workspace --release` clean
- `cargo test --workspace` clean
- `tools/loops/` directory containing: `loop_driver.sh`, `agent_prompts/coding_<phase>.md`, `agent_prompts/review_<phase>.md`, `gates/<phase>.sh`
- `outcomes/verification_gates.jsonl` initialized

**Acceptance gate:**
```bash
cd /Users/mbultra/projects/taifoon-solver
cargo build --workspace --release && cargo test --workspace && tools/loops/loop_driver.sh --self-test
```

**Mainnet verification gate:** *(none — this phase is local)*

**Coding agent prompt:** see `tools/loops/agent_prompts/coding_phase0.md`
**Review agent prompt:** see `tools/loops/agent_prompts/review_phase0.md`

---

### Phase 1 — Solana broadcast verified live (Mayan-Solana, smallest)

Smallest possible Mayan Solana fill broadcast. This is the differentiator — Solana-side fills are what makes the Frontier scope distinct. Doing this first burns down the highest-uncertainty risk first.

**Deliverable:**
- `SOLANA_PRIVATE_KEY` loaded from keychain `mamba-messiah-solana-key`
- `SolanaBroadcaster::send_fulfill` exercised against Helius mainnet RPC
- One real Mayan-Solana fill broadcast at notional ≤ $50 (or whatever the smallest live intent the genome stream provides)
- Fill recorded in outcome log with non-null `tx_hash` (Solana signature, base58)

**Acceptance gate (no keys required):**
```bash
cargo test -p protocol-adapters-solana --release
cargo test -p executor mayan_solana --release
```

**Mainnet verification gate (MB runs):**
```bash
DRY_RUN=false MAX_NOTIONAL_USD=50 PROTOCOL_FILTER=mayan ./run-mainnet.sh
# Wait for "🎉 Mayan Solana confirmed: …  sig=<base58>" in the log.
# Then:
sqlite3 outcomes/mainnet_*.sqlite \
  "select intent_id, decision, tx_hash, actual_profit_usd from solver_outcomes where decision='confirmed' and protocol like '%mayan%' order by ts desc limit 1"
# Expect: one row, decision=confirmed, tx_hash matches the log signature.
# Verify on Solscan: https://solscan.io/tx/<sig>
```

**Phase exits when:** an explorer link is appended to `outcomes/verification_gates.jsonl` as `{"phase":1,"tx":"<sig>","explorer":"https://solscan.io/tx/<sig>","ts":"…","notional_usd":…,"realized_usd":…}`.

**Coding agent prompt:** Drive `protocol-adapters-solana` test coverage to green. Confirm `MayanSolanaIntent::from_intent` decodes the Mayan-Solana payload shape currently emitted by the genome stream (capture a fixture with `tools/capture_intent.sh mayan_solana > tests/fixtures/mayan_solana_live.json` if the existing one is stale). Confirm `SolanaBroadcaster::from_env` reads the keychain-loaded env. Do not modify the broadcast logic itself unless the test or simulate path proves a defect.

**Review agent prompt:** Run the acceptance gate. Verify no regressions in other adapters' tests. If MB has run the mainnet gate, verify the explorer link resolves to a `Success` status and the tx signer matches the Messiah Solana pubkey.

---

### Phase 2 — Across mainnet first-fill (highest-volume, lowest-risk EVM)

**Deliverable:**
- One real Across V3 fill via direct `SpokePool.fillV3Relay` on Base or Arbitrum (smallest fees) at notional ≤ $50
- Fill recorded in outcome log with confirmed receipt + non-null `actual_profit_usd`

**Acceptance gate:**
```bash
cargo test -p executor across --release
cargo test -p protocol-adapters across --release
```

**Mainnet verification gate (MB runs):**
```bash
DRY_RUN=false MAX_NOTIONAL_USD=50 PROTOCOL_FILTER=across ./run-mainnet.sh
sqlite3 outcomes/mainnet_*.sqlite \
  "select tx_hash, src_chain, dst_chain, actual_profit_usd from solver_outcomes where decision='confirmed' and protocol like '%across%' order by ts desc limit 1"
# Verify on Basescan/Arbiscan and append to verification_gates.jsonl.
```

**Coding agent prompt:** Verify the spread guard, fill-deadline guard, and message-hook skip do not skip realistic intents. Capture a sample of the last 50 Across deposits seen on the genome stream (`tools/capture_intent.sh across --window 300`) and run them through `lambda_execute` in DRY_RUN mode; report skip-reason histograms. If >80% of intents skip for `across_no_deposit_id`, the genome enrichment is broken — escalate to MB. Otherwise proceed to mainnet.

**Review agent prompt:** Confirm `cargo test -p executor across` passes. If mainnet gate hit, confirm the receipt status is `1` (success) and gas used is in the expected range (60k-120k for direct fillV3Relay). If revert, capture the revert reason via `cast run <tx_hash> --rpc-url <chain_rpc>` and feed back to coding agent.

---

### Phase 3 — deBridge mainnet first-fill

**Deliverable:**
- One real deBridge `fulfillOrder` on Arbitrum or Base at notional ≤ $50
- Fill recorded with confirmed receipt + non-null `actual_profit_usd`

**Acceptance gate:**
```bash
cargo test -p executor debridge --release
cargo test -p protocol-adapters debridge --release
```

**Mainnet verification gate (MB runs):**
```bash
DRY_RUN=false MAX_NOTIONAL_USD=50 PROTOCOL_FILTER=debridge ./run-mainnet.sh
# Then verify on Arbiscan/Basescan.
```

**Coding agent prompt:** Re-confirm the recent deBridge fixes (commits `cb39d83`, `2a03ed6`, `cc6a98d`) are not regressed: estimate calldata matches broadcast calldata byte-for-byte (write a unit test that asserts `adapter.build_estimate_calldata == adapter.build_fulfill_order_calldata` for a fixture intent).

**Review agent prompt:** Confirm the unit test exists and passes. On mainnet gate, confirm the tx logs show the canonical `OrderFulfilled` event from `0xeF4f…`.

---

### Phase 4 — LiFi-via-X mainnet first-fill

LiFi is the meta-aggregator: a LiFi event names its underlying bridge (Across/deBridge/Mayan) in the `bridge`/`tool` field. The fill goes against the underlying bridge's destination contract.

**Deliverable:**
- One real LiFi-projected fill on the underlying — the live genome stream determines whether the underlying is Across, deBridge, or Mayan; record whichever fires first
- Fill recorded with `intent.protocol="lifi*"` and `tx_hash` on the underlying bridge's destination

**Acceptance gate:**
```bash
cargo test -p executor lifi_meta_router --release
```

**Mainnet verification gate (MB runs):**
```bash
DRY_RUN=false MAX_NOTIONAL_USD=50 PROTOCOL_FILTER=lifi ./run-mainnet.sh
sqlite3 outcomes/mainnet_*.sqlite \
  "select protocol, decision, tx_hash from solver_outcomes where protocol like '%lifi%' and decision='confirmed' order by ts desc limit 1"
```

**Coding agent prompt:** Add a debug log line in `lifi_meta_router::estimate` that records the resolved underlying bridge for every LiFi intent passed through the router. Verify resolution rate is >70% on a 50-intent sample (the rest may legitimately not be routable).

**Review agent prompt:** On mainnet gate, the recorded `tx_hash` should resolve to a tx on the underlying bridge's contract (Across SpokePool / DlnDestination / MayanSwift) — NOT the LiFi Diamond.

---

### Phase 5 — Dashboard P&L wiring

**Deliverable:**
- `solver-api` exposes `GET /api/solver/outcomes?limit=50` and `GET /api/solver/pnl`
- `dashboard/components/LivePnL.tsx` renders cumulative realized USD + per-protocol stacked bar + last 10 fills with explorer links
- Panel mounted at the top of `dashboard/app/page.tsx`

**Acceptance gate:**
```bash
cargo build -p solver-api --release
curl -s http://localhost:8082/api/solver/pnl | jq .realized_usd_total  # should be a number
curl -s http://localhost:8082/api/solver/outcomes?limit=5 | jq '. | length'
cd dashboard && pnpm build
```

**Mainnet verification gate (MB runs):**
After Phases 1-4 each have at least one confirmed fill in `outcomes/mainnet_*.sqlite`, point the dashboard at it and screenshot the panel showing realized USD > 0 + at least one fill per protocol. Save screenshot to `outcomes/phase5_screenshot.png` and commit.

**Coding agent prompt:** The `OutcomeLog::recent()` and `OutcomeLog::pnl_summary()` helpers are already in `crates/executor/src/outcome_log.rs`. Wire them into `solver-api` by: (a) extending `EventApi::new` to accept an optional `Arc<OutcomeLog>`, (b) adding two route handlers, (c) caching the `pnl_summary()` for 2 seconds to avoid hammering SQLite on every dashboard refresh. Do not add a websocket — these endpoints are pull-on-demand.

**Review agent prompt:** Verify `solver-api` does not gain a direct `rusqlite` dependency (the executor crate owns SQL). Verify dashboard build is green and the LivePnL component handles the empty-DB case gracefully (renders "$0.00 / 0 fills" with no errors).

---

### Phase 6 — Demo dry-run rehearsal

**Deliverable:**
- Recorded video walkthrough of the demo (fallback for demo day if mainnet flakes)
- Pre-flight checklist verified: balances funded, RPCs responsive, genome stream subscribed, dashboard rendering

**Acceptance gate:**
```bash
tools/loops/gates/phase6_rehearsal.sh
# Asserts: outcome DB has ≥1 confirmed row per protocol; dashboard responds 200;
# `which obs` and `which ffmpeg` available for recording.
```

**Mainnet verification gate:** Recorded video committed to `demo/frontier_walkthrough.mp4` (gitignored, but referenced by relative path in the README).

---

## 5. Mainnet runbook

```bash
# One-time setup (MB):
security add-generic-password -a $USER -s mamba-messiah-key       -w 0xYOUR_EVM_PRIVATE_KEY
security add-generic-password -a $USER -s mamba-messiah-solana-key -w YOUR_SOLANA_BASE58_PRIVATE_KEY
mkdir -p outcomes/

# Fund the EVM address shown by `cast wallet address $(security find-generic-password -s mamba-messiah-key -w)` on:
#   Ethereum, Optimism, Polygon, Base, Arbitrum  — each with ~$50-100 of native gas + the dst-token used by intents
# Fund the Solana address with ~0.1 SOL + the SPL tokens you intend to fulfill in.

# Build:
cargo build --workspace --release

# Dry-run smoke test (no funds at risk):
./run-mainnet.sh                              # default DRY_RUN=true

# Live (typed confirmation required):
DRY_RUN=false ./run-mainnet.sh

# Per-protocol scoping (recommended for first fill of each):
DRY_RUN=false PROTOCOL_FILTER=across   ./run-mainnet.sh
DRY_RUN=false PROTOCOL_FILTER=debridge ./run-mainnet.sh
DRY_RUN=false PROTOCOL_FILTER=lifi     ./run-mainnet.sh
DRY_RUN=false PROTOCOL_FILTER=mayan    ./run-mainnet.sh
```

### Kill switch

```bash
pkill -SIGTERM taifoon-solver   # clean shutdown — flushes outcome log, releases wallet reservations
```

The solver listens to SIGTERM via the tokio runtime and:
1. Stops accepting new SSE events
2. Awaits in-flight `lambda_execute` calls to reach a terminal state (or fail open)
3. Closes the rusqlite connection cleanly
4. Exits 0

If the solver is misbehaving (e.g., broadcasting in a loop), `pkill -SIGKILL` is safe — the on-chain state is the source of truth, and the next start will reconcile with the wallet ledger.

---

## 6. Demo narrative (5 minutes)

1. **Open dashboard at localhost:3000** — show the existing intent stream + protocol breakdown.
2. **Highlight the new Live P&L panel** — currently empty.
3. **Start solver:** `DRY_RUN=false ./run-mainnet.sh` — terminal prints balances on every chain, then SSE subscribes.
4. **First intent flows in:** dashboard `IntentsStream` shows the new event; logs show `attempt → estimate → broadcast → receipt`.
5. **First fill confirms:** P&L panel ticks up by realized USD.
6. **Trigger an EVM→Solana fill** (Mayan): same flow, but the receipt link goes to Solscan instead of Etherscan — that's the EVM↔SOL story made tangible.
7. **Open the SQLite directly** in a side terminal: `sqlite3 outcomes/mainnet_*.sqlite "select protocol, tx_hash, actual_profit_usd from solver_outcomes order by ts desc limit 5"` — judges see raw record matches the dashboard.

---

## 7. Open questions for MB (blocking specific phases)

| # | Question | Blocks |
|---|---|---|
| Q1 | Is `mamba-messiah-solana-key` already in the keychain with a funded mainnet SOL address? | Phase 1 |
| Q2 | Is the Mayan EVM solver registration done? If yes, we can flip Mayan EVM on for the demo (extra route in Phase 4). | Phase 4 (optional) |
| Q3 | What's the demo wallet funding budget? Drives `MAX_NOTIONAL_USD` and per-chain top-up amounts. | Phases 1-4 mainnet gates |
| Q4 | Genome SSE host: still `46.4.96.124:30081`, or has it moved? | All live phases |
| Q5 | Is there a reachable spinner `/api/solver/test-run` for profitability? Last `LIVE_OPS_LOG.md` capture (2026-04-28) was against a port-forward. | All EVM phases |

---

## 8. References

- Live broadcast surface: `crates/executor/src/lambda_controller.rs`
- LiFi meta-router: `crates/executor/src/lifi_meta_router.rs`
- Solana broadcaster: `crates/protocol-adapters-solana/src/{mayan_solana,send,simulate}.rs`
- Outcome log + P&L: `crates/executor/src/outcome_log.rs`
- Solver event API: `crates/solver-api/src/lib.rs`
- Chain wiring: `config/chain_wiring.json`
- Older plans (superseded but useful context): `HACKATHON_COLOSSEUM_PLAN.md`, `STATE_OF_THE_UNION.md`, `LIVE_OPS_LOG.md`
- Loop scripts and agent prompts: `tools/loops/` (Phase 0 deliverable)
- Verification gate ledger: `outcomes/verification_gates.jsonl` (append-only)
