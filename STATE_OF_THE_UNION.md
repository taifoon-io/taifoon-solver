# State of the Union — taifoon-solver
Date: 2026-04-27

## TL;DR

- The repo is **not vaporware**. `cargo build --workspace` is **clean** and `cargo test --workspace` is **almost green**: 1 unit test fails (`genome-client::test_parse_genome_event` — the test fixture uses old field names `ref/token/amount/timestamp` while the code now requires `input_amount`/`src_token`/`ts`/`ref_hash`). Everything else compiles and tests.
- The Across path is the **only** end-to-end fill path. It has a real signer, real `executeWithProof` calldata builder, real receipt-handling, and a real outcome log. **It has never been broadcast** because (a) `SOLVER_PRIVATE_KEY` is not set in any default config and (b) `DRY_RUN` defaults to true.
- The other four protocol adapters (deBridge, Mayan, LiFi, Stargate) compile but are **adapter-shaped stubs** that build calldata of varying realism and then return `Err("Live X execution not yet implemented")` from `execute_fill`. They are **not** wired into the Across executor's `executeWithProof` pipeline; they hang off the legacy `Executor` which itself goes nowhere.
- A real-world Across deposit hitting the live SSE today would, with default config, get to `[DRY-RUN] Would broadcast executeWithProof` and stop. With `SOLVER_PRIVATE_KEY`+`CHAIN_WIRING_JSON`+`DRY_RUN=false` it would actually broadcast — but there is a **silent ABI mismatch** between the Rust struct (`uint32 depositId`) and the Solidity adapter (`int64 depositId`), so the Operator-side `abi.decode` would revert.
- The genome SSE host (46.4.96.124:30081) is **not reachable** from this machine — both the SSE endpoint and the V5 proof endpoint time out. So real-stream verification was not possible; this audit is from source.

## Section 1: Crate-by-crate status

Workspace members (from `Cargo.toml`):

| Crate | LOC | Status | Notes |
|---|---:|---|---|
| `genome-client` | 326 | **COMPILES_AND_RUNS** (1 unit test failing) | Real reqwest+chunked SSE parser, real reconnect-loop. Parses `event: genome` / `event: genome_entry`. Per-event JSON → `Intent`. Test fixture stale vs. struct (uses old keys). |
| `profit-calc` | 432 | **COMPILES_AND_RUNS** | Real Warmbed gas-price fetch w/ 30s cache + fallback table, real fee-bps loader from `solver_intel.json`, USDC/USDT decimal detection by hardcoded address list. ETH price is a hard-coded $3000 (cf. line 86). Spread is hard-coded 0. Liquidity cost hard-coded 0. |
| `protocol-adapters` | ~1100 | **MIXED** | See Section 3. Trait + factory are real; per-protocol implementations vary from STUB_ONLY (Stargate) to IMPLEMENTED-but-not-broadcast-capable (Across, deBridge). |
| `executor` | ~900 (5 files) | **COMPILES_AND_RUNS** | `AcrossExecutor` is the only real path — full pipeline: Spinner test-run → fetch proof bundle → build adapter calldata → wrap in `executeWithProof` → sign+broadcast via alloy → receipt → outcome log. The legacy `Executor` (the original one wired to `protocol-adapters`) is COMPILES_BUT_STUB and only emits simulated tx hashes. |
| `solver-main` | 342 | **COMPILES_AND_RUNS** | Wires SSE → skip-rules → Across executor (if `SOLVER_PRIVATE_KEY` set) → fallback to legacy executor. Spawns axum SSE event API on :8082. |
| `solver-api` | 474 | **COMPILES_AND_RUNS** | Axum router that streams `IntentDetected/IntentAttempted/IntentSolved` events to a dashboard. Defensible. |
| `t3rn-sidecar` | 132 (2 files) | **COMPILES_BUT_STUB** | LWC interface; `create_order` and `can_provide_liquidity` are placeholders — actual order creation isn't implemented. |
| `mempool-monitor` | 420 | **COMPILES_AND_RUNS** (not wired in) | Built but `solver-main` never instantiates it. Dead in-tree. |
| `taifoon-cli` | ~1000 (5 files) | **COMPILES_AND_RUNS** | Standalone CLI with execute/monitor/wallet/test_mode subcommands. 21 dead-code warnings. Independent of solver-main. |

`cargo build --workspace`: **OK** (with warnings, ~12s).
`cargo test --workspace`: **1 failure** (`genome-client::test_parse_genome_event`, fixture rot — see fix in Section 7). Everything else passes.

## Section 2: Genome SSE

`crates/genome-client/src/lib.rs` is **real, not a stub**:

- `GenomeClient::subscribe()` (line 187): outer reconnect-on-failure loop with fixed 5s sleep (no exponential backoff — should be addressed eventually but not blocking).
- `subscribe_internal` (line 204): chunked HTTP read; manual SSE framing on `\n\n`; multiline parse pulls `event:` and `data:` from each frame.
- Accepts `event: genome` and `event: genome_entry`.
- Filters to `entity in {proto, order}` and `action in {deposit, placed, executed}`.
- `Intent::from_genome_event` (line 77) does field-by-field plumbing with reasonable fallbacks (synthetic tx_hash via DefaultHasher when `ref_hash` is absent, native-zero fallback for missing `src_token`).

**Live test status**: I attempted `curl -sS --max-time 8 -I http://46.4.96.124:30081/api/genome/subscribe/sse` and `curl http://46.4.96.124:30081/api/v5/proof/bundle/across/test` from this machine — both timed out after 8s. Either the host is firewalled from this IP or down. The source itself is sound; the question is whether the host is reachable from the actual solver runtime. **Verify externally before declaring this section healthy.**

**Real bug in test, not in code**: `tests::test_parse_genome_event` uses `"ref"`, `"token"`, `"amount"`, `"timestamp"` — but the struct expects `"ref_hash"`, `"src_token"`, `"input_amount"`, `"ts"`. Test panics on `unwrap()` for `Missing input_amount`. **Trivial one-line fix**, but it's the only red light in the workspace.

## Section 3: Protocol decoder coverage

Cross-referencing `protocols.xml` (the spinner catalog has ~30 protocol IDs) against `taifoon-solver/crates/protocol-adapters`:

| protocols.xml id | taifoon-solver adapter | Decoder status | On-chain adapter (taifoon-eco) |
|---|---|---|---|
| `across` | `across.rs` | **IMPLEMENTED** — real `fillV3Relay` calldata, real spoke-pool address table for chains 1/10/137/8453/42161 | `contracts/adapters/AcrossAdapter.sol` exists |
| `debridge` | `debridge.rs` | **IMPLEMENTED** — real `fulfillOrder` calldata, real DLN address (single 0xeF4f… for all chains), but `Order` struct is filled with bytes-encoded addresses where the source intent doesn't actually carry a `makerOrderNonce` (extracted from intent ID by parsing trailing digits — **brittle**) | `contracts/adapters/DeBridgeAdapter.sol` exists |
| `mayan_swift` | `mayan.rs` | **STUB_ONLY** — `build_fill_tx` produces `data: hex("fulfill()")` literal bytes (line 62). Will revert on-chain. | `contracts/adapters/MayanAdapter.sol` exists |
| `lifi` | `lifi.rs` | **STUB_ONLY** — `build_fill_tx` produces `data: hex("lifi_delegate_to_underlying")` literal bytes (line 203). Will revert on-chain. The protocol is correctly identified as a meta-aggregator but no underlying-protocol routing is implemented. | none |
| `stargate` | `stargate.rs` | **NOT_STARTED** — every method returns `Err(anyhow!("Stargate adapter not yet implemented"))` | none |
| `connext`, `hop`, `relay`, `axelar`, `cctp`, `ccip`, `wormhole`, `synapse`, `symbiosis`, `meson`, `socket`, `squid`, `rango`, `router_protocol`, `allbridge`, `orbiter_finance`, `multichain`, `celer`, `hyperlane`, `t3rn`, `1inch`, `arbitrum_bridge`, `optimism_bridge`, `bancor`, `uniswap_v2/v3/v4` | — | **NOT_STARTED** | none |

So: **GOLD_TIER (production-quality, end-to-end)**: 0. **IMPLEMENTED**: 2 (Across, deBridge). **STUB_ONLY**: 2 (Mayan, LiFi). **NOT_STARTED**: 1 in-tree (Stargate) + ~24 catalog protocols not even sketched.

Important: only Across has a corresponding executor pathway (`AcrossExecutor`). The other adapters live behind the legacy `Executor::execute_with_own_funds`, which never broadcasts.

## Section 4: End-to-end fill path trace (the core section)

Hypothetical: real Across deposit hits live genome SSE **right now**, default `solver-main` config, `SOLVER_PRIVATE_KEY` SET, `CHAIN_WIRING_JSON` SET, `DRY_RUN=false`, `PROTOCOL_FILTER=across`.

| Step | File:line | Outcome |
|---|---|---|
| 1. SSE chunk arrives, parsed into `Intent` | `genome-client/src/lib.rs:204-237` | ✅ succeeds (real code) |
| 2. Pushed onto `mpsc<Intent>` to main loop | `solver-main/src/main.rs:131` | ✅ |
| 3. `solver_api.emit_event(IntentDetected)` | `solver-main:136` | ✅ (dashboard sees it) |
| 4. `skip_rules.evaluate` | `solver-main:153` | ✅ (no rules loaded if `MAMBA_LAKE_URL` unset; passes through) |
| 5. Routes into Across branch | `solver-main:192` | ✅ |
| 6. `AcrossExecutor::fill` enters | `executor/across_executor.rs:105` | ✅ |
| 7. `spinner.test_run(protocol, order_id)` POST `/api/solver/test-run` | `executor/spinner_solver.rs:28-51` | ⚠️ depends on spinner being reachable; on this machine the host is unreachable. Tolerant of multiple JSON shapes. |
| 8. Profitability gate checked vs `profit_threshold_usd` | `across_executor.rs:116-162` | ✅ skips with `OutcomeRecord{decision="skip_unprofitable"}` if not profitable; logs and returns `Ok(None)` |
| 9. `chains.get(&intent.dst_chain)` | `across_executor.rs:165-169` | ⚠️ if chain not in `CHAIN_WIRING_JSON` → `Err("no chain wiring for dst N")`. Operator must pre-configure. |
| 10. `fetch_across_proof_bundle(order_id)` GET `/api/v5/proof/bundle/across/<id>` | `spinner_solver.rs:55-88` | ⚠️ spinner-side (see Section 6). Tolerant of `application/octet-stream`, `proof_blob_hex`, `proof`, `blob`, or raw JSON. |
| 11. `build_across_adapter_calldata(intent)` | `across_executor.rs:278-323` | 🛑 **HARD ISSUE**. Two problems: (a) `parse_deposit_id` extracts the trailing integer from `intent.id` or `intent.tx_hash` — for genome events whose `addr` is `T:1745678/proto:lifi_v2/deposit:1:0xabc123` the `id` becomes `lifi_v2:0xabc123` and there is no trailing integer → returns `Err("cannot parse depositId")`. For Across-specific events the fact pattern of `intent.id` would need to actually contain `:<u32>`; not guaranteed. (b) the V3RelayData struct in Rust uses `uint32 depositId` and `outputAmount = inputAmount` (the genome event doesn't actually carry the negotiated outputAmount, see `across_executor.rs:307`). So even if the deposit-id is parsed, the relay data has the wrong output amount and the fill will revert in the Across SpokePool because `outputAmount` doesn't match the protocol's prescribed output (Across enforces this on-chain). |
| 12. ABI encode `executeWithProof(v5ProofBlob, adapter, adapterCalldata)` | `across_executor.rs:183-188` | ✅ (assuming step 11 succeeded) |
| 13. Sign + broadcast via alloy provider | `across_executor.rs:217-230` | ✅ |
| 14. `provider.send_transaction → with_required_confirmations(1) → get_receipt` | `across_executor.rs:236-244` | ⚠️ if the on-chain call reverts (which it will — see step 15), the receipt has `status=false`. |
| 15. **On-chain `executeWithProof`** decodes `adapterCalldata` per `IAcrossAdapter.fill(uint32 depositId, bytes relayData, uint256 repaymentChainId)` | `across_executor.rs:36-42` | 🛑 **ABI MISMATCH**. The Solidity `AcrossAdapter.fill` signature in `taifoon-eco/contracts/adapters/AcrossAdapter.sol:104-108` is `fill(int64 depositId, bytes relayData, uint256 repaymentChainId)`. The Rust ABI uses `uint32`. Selector mismatch (`int64` vs `uint32` produces a different 4-byte selector) → call reverts. |
| 16. Outcome log append | `across_executor.rs:258-273` | ✅ records the receipt regardless |

**Verdict**: Even with everything wired correctly and the spinner reachable, the first real fill would revert on-chain due to the int64/uint32 selector mismatch. Fix: change the Rust `IAcrossAdapter.fill` `sol!` block to `int64 depositId` and re-derive `parse_deposit_id` to return `i64`, OR change the Solidity adapter to `uint32`. The Solidity side is what's deployed (most likely), so the Rust side should change.

## Section 5: Profitability gate

Two layers exist; only the second is consulted on the Across path.

1. `profit-calc/src/lib.rs::ProfitCalculator::calculate` (used only by the legacy executor, which never broadcasts):
   - Loads fee bps from `config/solver_intel.json` ✅
   - Fetches gas price from Warmbed API w/ 30s cache; falls back to per-chain hardcoded table on failure ✅
   - **ETH price is hard-coded $3000** (line 86). Stablecoin price is hard-coded $1. Token-decimal detection is hard-coded by address list (USDC/USDT only; everything else defaults to 18). Spread is hard-coded 0; liquidity cost is hard-coded 0.
   - So `is_profitable` is real arithmetic, not a `true` literal — but the inputs are stale enough that any non-USDC/non-USDT intent will be wildly mispriced.
2. `executor/across_executor.rs::fill` (the **only** path that actually broadcasts) **does not call `profit-calc` at all**. It defers the entire profit decision to `POST /api/solver/test-run` on the spinner side. So the on-host calculator is functionally moot for Across.

## Section 6: V5 proof bundle integration

Yes. `executor/spinner_solver.rs::fetch_across_proof_bundle` calls `GET /api/v5/proof/bundle/across/<order_id>` and feeds raw bytes into `executeWithProof` (the operator decodes with `TaifoonV5Codec` on-chain).

The spinner-side endpoint exists at `spinner/rust/crates/da-api/src/solver_proof_api.rs` (file header confirms it assembles 6-layer proof bundles, redis-cached). Integration is real on both ends; the question is whether the cached bundle is fresh and whether the host is reachable.

## Section 7: Path of least resistance — pick one protocol

**Across, on testnet, 5–7 working days**, with these concrete tasks:

1. **Fix the ABI mismatch**. `crates/executor/src/across_executor.rs:36-42` — change `uint32 depositId` → `int64 depositId` in both the `IAcrossAdapter` interface and the `V3RelayData` struct (line 54). Update `parse_deposit_id` (line 326) to return `i64`. Should match `taifoon-eco/contracts/adapters/AcrossAdapter.sol:14,99,104`.
2. **Fix `outputAmount` derivation**. `across_executor.rs:307` currently sets `outputAmount: input_amount`. The Across SpokePool enforces the negotiated output. Either (a) derive it from the genome event payload (the proto event for Across includes a `outputAmount` field if the decoder is doing its job — check spinner header-collector for `across` proto def) or (b) pull it from the V5 proof bundle (the proof's `L5_chain_event.encoded_tx` contains the original deposit calldata). Option (a) is faster.
3. **Fix the genome-client unit test fixture** (`crates/genome-client/src/lib.rs:301-315`). Rename `ref` → `ref_hash`, `token` → `src_token`, `amount` → `input_amount`, `timestamp` → `ts`. One-line fix per field. (The cleaner change is to add `#[serde(alias = "...")]` to `GenomeEvent` and make the client tolerant of both shapes.)
4. **Wire a Sepolia / Base-Sepolia chain in `CHAIN_WIRING_JSON`** with the testnet Across SpokePool. Set `SOLVER_PRIVATE_KEY` to a test wallet funded with test USDC and test ETH on the destination chain. Set `DRY_RUN=false`.
5. **Verify the `intent.id` format Across actually emits via the live SSE**. If the genome-client's synthetic `id = "<protocol>:<tx_hash>"` doesn't carry the depositId, the executor's `parse_deposit_id` cannot recover it. Fix: extend `Intent` with an optional `deposit_id: Option<i64>` and have `Intent::from_genome_event` populate it from the proto event payload (the source `proto:across/deposit` event from the genome stream should carry it directly).
6. **Smoke test against a known historical Across deposit** by mocking `SpinnerSolverClient` to return canned `test-run` and `proof-bundle` responses, then assert the calldata bytes match what `cast 4byte-decode` says they should. This catches the ABI mismatch in CI before any wallet ever signs.
7. After (1)–(6): broadcast one `executeWithProof` against a Sepolia deployment of `TaifoonUniversalOperatorV5` + `AcrossAdapter`. Confirm receipt status=success. Done.

What you do **not** need for testnet-Across: deBridge, Mayan, LiFi, Stargate, profit-calc, mempool-monitor, t3rn-sidecar, tlfr legacy executor. All of those can stay broken.

## Section 8: Reality vs claimed-state-in-docs

The repo has **38** markdown status documents at the root. Selectively cross-referenced:

- **`PROTOCOL_STATUS_REPORT.md`** (2026-04-26): claims "9 protocol integrations", "0% pass rate", and lists Hyperlane/Squid/T3rn/Allbridge/LayerZero V2/Stargate/Synapse/Connext/Wormhole as "validated". Reality: only Across (real), deBridge (partial), Mayan (stub), LiFi (stub) have any code in `protocol-adapters/`. The 9-protocol claim refers to fixture validation against spinner output, not solver-side decoding. **The doc is misleadingly optimistic about solver coverage.**
- **`CRITICAL_BUGS_FOUND.md`** (2026-04-25): correctly identifies (a) chain-id 0 fallback in protocol decoders (this is a **spinner**-side bug, not a solver-side one) and (b) OP-stack gas price unit confusion (also spinner-side). Both are real and would manifest as garbage profit calcs. Neither is fixed in the solver tree (because they shouldn't be — they belong to spinner).
- **`BUGFIX_SUMMARY.md`** is on disk but I didn't find a separate top-level summary worth quoting; the relevant fixes that landed are visible in `profit-calc/src/lib.rs` (verbose decoder logging, overflow detection, multiple-format `input_amount`/`amount` fallback).
- **`DEPLOYMENT_READINESS.md`**, **`FINAL_STATUS.md`**, **`SESSION_SUMMARY_2026-04-27.md`**: I did not read these in full but their volume itself is a smell — when a repo has a `FINAL_STATUS.md`, a `SESSION_COMPLETE_LIFI_FIX.md`, and a `DEPLOYMENT_STATUS.md` at the same root level, the doc directory is being used as a shipping log instead of a state-of-the-system reference. **Treat all *_STATUS / *_COMPLETE / *_SUMMARY docs as historical, not authoritative.** The authoritative state is the source.
- The integration with spinner's solver APIs (`/api/solver/test-run`, `/api/v5/proof/bundle/across/<id>`, `/api/solver/estimate-gas`) is **real on both sides** — the spinner has the matching crates (`da-api/src/solver_test_api.rs`, `solver_proof_api.rs`, `solver_gas_estimator.rs`). Whatever has been "stuck" is not at the API contract layer.

## Recommendation: Phase B brief (concrete, ~1 week of work)

**Goal**: One real Across fill on Sepolia + Base-Sepolia, broadcast and confirmed, with a tx hash you can show.

**Day 1 (½ day)** — Sweep the green-build floor:
- Fix `crates/genome-client/src/lib.rs:301-315` test fixture (or add serde aliases).
- Add a CI step that runs `cargo test --workspace` and gates on green.

**Day 1–2** — Fix the on-chain ABI mismatch:
- `crates/executor/src/across_executor.rs:36-58` change `uint32 depositId` → `int64 depositId` (both in `IAcrossAdapter` and `V3RelayData`).
- `crates/executor/src/across_executor.rs:326-330` change `parse_deposit_id` to return `Option<i64>`.
- Mirror the same change in `crates/protocol-adapters/src/across.rs:53` (this lives in the legacy adapter; less load-bearing but worth keeping consistent).
- Add a unit test that decodes the produced calldata back through alloy's ABI decoder and asserts the depositId field is preserved bit-for-bit.

**Day 2–3** — Plumb depositId end-to-end:
- Extend `Intent` (`crates/genome-client/src/lib.rs:49-73`) with `pub deposit_id: Option<i64>`.
- Update `Intent::from_genome_event` to read it from the genome event payload (need to confirm the field name with spinner — check `spinner/rust/crates/header-collector/protocols/across.xml` or equivalent). Backfill from `parse_deposit_id` for legacy events.
- `across_executor.rs:282` use `intent.deposit_id` first, fall back to parser only if `None`.

**Day 3–4** — Plumb outputAmount end-to-end:
- Same approach as depositId. The Across `V3FundsDeposited` event has `outputAmount` as a top-level field. The spinner decoder should already capture it; if it doesn't, that's a spinner fix (one line in the proto XML).
- Plumb into `Intent`, default `intent.amount` only if missing.

**Day 4–5** — Testnet smoke:
- Deploy `TaifoonUniversalOperatorV5` + `AcrossAdapter` on Sepolia + Base-Sepolia (probably already done — check `taifoon-eco/contracts/DEPLOYED_ADDRESSES.md`).
- Set up `CHAIN_WIRING_JSON` with `{11155111: {…sepolia…}, 84532: {…base-sepolia…}}`.
- Run `solver-main` against a private spinner test-deployment **first** (to avoid burning a real test-fee on a malformed proof bundle).
- Then point at the production spinner SSE.

**Day 5–7** — Buffer for the unforeseen (almost certainly: a missing field in the genome event payload, a stale spinner cache, or a mis-deployed adapter).

**What stays broken on purpose**: the legacy `Executor`, the dead `mempool-monitor`, the four stub adapters (deBridge/Mayan/LiFi/Stargate), and `profit-calc` for non-Across intents. Don't touch them — every hour spent there is an hour not spent on the one fill that matters.

**Stretch goal once Across is filling**: deBridge is closest to viable. It already has real `fulfillOrder` calldata. Same Phase-B structure applies (ABI sanity-check, plumb the missing fields, mainnet-fork test). Add ~1 week per protocol after Across.
