---
name: taifoon-solver
description: Use this skill whenever working in the /Users/mbaj/projects/taifoon-solver/ repo. It's a Rust workspace + Next.js dashboard for a cross-chain intent solver (Across, deBridge, Mayan, LiFi, Wormhole NTT — both EVM and Solana). The skill orients you to the donut attestation flow (Spinner is the registered adapter owner; per-fill inflow is redistributed internally 70/20/10 to adapter_builder / adapter_reviewers / adapter_ecosystem), the Spinner / Solver / Builder hierarchy, the onboarding flow, and the test rigs. Triggers on any mention of Taifoon, Spinner, donut, adapter_registry, solver-api, donut-adjudicator, or files under crates/ in this repo.
---

# Taifoon Solver — Orientation for Agents

Read this entirely before making changes. The repo has tight semantics
that don't fit the obvious mental model.

## What this is

A solver that watches a cross-chain intent stream (Genome SSE), decides
which intents to fill, and broadcasts fills through protocol adapters
(Across, deBridge, Mayan Swift/Flash, LiFi, Wormhole NTT) on EVM and
Solana. The Spinner is registered with the upstream order contract /
fee distributor as the **adapter owner**; on each fill the Spinner's
wallet receives a per-fill **inflow** as the adapter-owner share. Every
confirmed fill emits a **signed donut attestation** that records how the
Spinner redistributed that inflow internally to three purposes —
adapter_builder (70%), adapter_reviewers (20%), adapter_ecosystem (10%).

## Three actors you must keep straight

- **Spinner** = operator pod. Owns keys (Keychain on macOS), owns the
  binary process, owns the capital, and is the registered adapter
  owner with the upstream order contract. One Spinner can host many
  solver modules. Registered in
  `crates/solver-api/src/hosting.rs::HostedSolver`.
- **Solver module** = a protocol-family fill path inside the Spinner's
  binary. Code lives in `crates/executor/`.
- **adapter_builder** = developer who shipped an adapter integration.
  Receives the 70% share of the redistributed inflow. NOT the Spinner
  (usually). Configured in `config/adapter_registry.json`.

adapter_reviewers (20%) are the open-mamba code-review agents
registered for an adapter; the addresses live under `reviewers` in
`config/adapter_registry.json`. adapter_ecosystem (10%) goes to a
single address — the catch-all + fail-closed absorber.

## The donut math — fix the most common misconception first

**The donut base is the upstream adapter-owner inflow, NOT realised
profit and NOT the protocol fee itself.** The Spinner is the adapter
owner with the upstream order contract. On each fill the upstream fee
distributor routes a per-fill inflow to the Spinner's wallet; the donut
attestation records how the Spinner redistributes that inflow
internally. Under the current arrangement the inflow equals the SSE-
decoded fee component on the intent.

```
donut_take_usd_micro = max(0, inflow_usd_micro) × split_num / split_den
recipients["adapter_builder"]    = donut × 70 / 100   ──► adapter_builder
recipients["adapter_reviewers"]  = donut × 20 / 100   ──► reviewers (split equally)
recipients["adapter_ecosystem"]  = donut − builder − reviewers (residual)
```

`split_num / split_den` defaults to `1 / 1` (100% of inflow
redistributed). Per-adapter overrides live in
`config/adapter_registry.json` (optional `donut_bps_num` /
`donut_bps_den` fields — the names predate the inflow framing). The
70 / 20 / 10 internal split is fixed and uniform across all adapters
this Spinner runs.

All money is **i64 micro-USD** ($1.00 = 1_000_000). Floats only appear
at the boundary (USD inputs from `OutcomeRecord.actual_profit_usd` /
`fee_usd`) and are converted via `usd_to_micro` immediately.

## File map you'll actually need

Start with the architecture doc — it's exhaustive: `docs/donut_flow.md`.

**Hot files** (touch most often):

- `crates/donut-adjudicator/src/lib.rs` — math, signing, adapter-id
  resolver, registry loader, policy + view structs.
- `crates/solver-api/src/hosting.rs` — provision flow, SIWE,
  `persist_attestation`, ledger reads. SQLite schema lives here.
- `crates/solver-api/src/lib.rs` — Axum router. All `/api/donut/*` and
  `/api/hosting/*` handlers.
- `crates/executor/src/outcome_log.rs` — `OutcomeRecord` struct. The
  upstream type every attestation derives from.
- `crates/executor/src/lambda_controller.rs` — the live fill loop. Each
  protocol has its own branch ending in `append_outcome` (EVM) or
  `append_solana_confirmed` (Solana).
- `crates/solver-main/src/main.rs` — the binary entrypoint. Loads keys
  from Keychain via `messiah::load_messiah_signer`.
- `config/adapter_registry.json` — adapter_id → builder + reviewers + bps.
- `tests/integration/test_user_journey.py` — Python rig. Reads as the
  user spec: "can I onboard / are my funds safe / are my funds working /
  is the split right".

**Cold files** (read once for orientation, rarely modify):

- `crates/genome-client/` — SSE poller, intent decoder.
- `crates/protocol-adapters-solana/` — Mayan Swift, Mayan Flash, NTT,
  DLN-Solana program clients.
- `dashboard/app/onboard/` — Next.js wizard.

## Run the tests

The test sandbox boots `solver-api-testbin` (in
`crates/solver-api/src/bin/testbin.rs`) in a subprocess against a temp
SQLite + random API token, then drives the full HTTP surface from
Python:

```bash
cd /Users/mbaj/projects/taifoon-solver
cargo test -p donut-adjudicator
cargo test -p solver-api
cargo test -p solver-sandbox --test solana_attestation_sandbox
python3 -m pip install -r tests/integration/requirements.txt
python3 -m pytest tests/integration -v
```

The Python rig delegates attestation signing to the Rust testbin via
`solver-api-testbin sign-attestation` (stdin JSON spec → signed
DonutAttestation JSON on stdout) — keeps Python from having to reproduce
serde_json's float formatter byte-for-byte.

## Common tasks — the right entry points

### "Add a new protocol adapter"

1. Add a Rust adapter under `crates/protocol-adapters/` or
   `crates/protocol-adapters-solana/`.
2. Wire it into `crates/executor/src/lambda_controller.rs` (new branch)
   so a confirmed fill emits an `OutcomeRecord` with the correct
   `protocol`, `src_chain`, `dst_chain`, and `fee_usd` (decoded from the
   SSE intent).
3. Extend `donut_adjudicator::default_adapter_id` to recognise the new
   protocol string.
4. Add an entry to `config/adapter_registry.json` with the Builder
   address, reviewer set, and (if non-default) a per-adapter bps
   override.
5. Add a Rust test in
   `crates/solver-sandbox/tests/solana_attestation_sandbox.rs` (for
   Solana) or in `crates/donut-adjudicator/src/lib.rs` (for EVM) that
   asserts the adapter_id resolves correctly and the math invariants
   hold.

### "Add a new API route"

Routes live in `crates/solver-api/src/lib.rs::SolverApi::router()`.
Mutation routes go inside the `solver_api` group (gated by
`require_solver_api_token`). Read-only public routes go in `public_api`
or, for donut-policy reads, in `donut_policy`.

### "Run a Spinner locally"

See `SECURITY_ONBOARDING.md` and `run-mainnet.sh`. Default is
`DRY_RUN=true`; flip to `DRY_RUN=false` only after a successful dry-run.
The key goes in macOS Keychain (`security add-generic-password -s
mamba-messiah-key`). On Linux/CI use env vars from a secrets manager.

### "Modify the math"

Change only `compute_redistribution_micro` in
`crates/donut-adjudicator/src/lib.rs`. Bump
`DonutPolicy::canonical()`'s `adjudicator_version` string. Update every
test in:

- `crates/donut-adjudicator/src/lib.rs` (unit tests)
- `crates/solver-sandbox/tests/solana_attestation_sandbox.rs` (cross-protocol)
- `crates/solver-api/src/hosting.rs` (persist round-trip)
- `tests/integration/test_user_journey.py` (HTTP round-trip)
- `tests/integration/helpers.py::compute_split_micro` (Python mirror)

If you change the math without updating the version string, the public
audit trail loses its anchor — every old attestation becomes
unverifiable.

## What to NEVER do

- **Never log a raw private key, raw api_token, or raw signature
  material.** Log only the recovered public address or the first 8 chars
  of the token. The Keychain bootstrap
  (`messiah::load_messiah_signer`) drops the raw key string within
  microseconds; preserve that pattern.
- **Never use float math for money** anywhere downstream of
  `usd_to_micro`. The whole point of the i64 migration was byte-stable
  signatures across platforms.
- **Never silently route the adapter_builder share to the Spinner** when
  an adapter isn't registered. The adjudicator routes both 70% and 20%
  to the ecosystem treasury as fail-closed behaviour. Tests cover this.
- **Never use `futures::executor::block_on` inside an axum handler.**
  SIWE verify is `async fn`; await it directly. Nested executors
  deadlock current-thread tokio.
- **Never weaken the spinner_id binding in `persist_attestation`.** The
  body's `spinner_id` MUST equal
  `donut_adjudicator::spinner_id_from_addr(att.spinner_addr)`. Without
  that check, a Spinner with a valid API token can pollute another
  Spinner's ledger.

## Open work (good first issues)

- **On-chain settlement reconciler** — when an on-chain contract
  publishes for redistribution, the attestation flow stays and becomes
  the off-chain reconciler against on-chain settlement.
- **Per-platform RPC failover** in
  `crates/protocol-adapters-solana/src/send.rs` — current code uses one
  Solana RPC and retries on rate-limit; failover to a secondary endpoint
  would harden Mayan-Solana fills.
- **Deploy box** — `deploy/` contains a containerised Spinner build for
  operators who want to run an adapter operator pod without managing the
  Rust toolchain locally.

## When in doubt

- Read `docs/donut_flow.md` — the architecture doc covers every box in
  the runtime diagram.
- Read `SECURITY_ONBOARDING.md` — operator-facing key handling, env
  vars, kill switches.
- The Python tests are written as **user questions** ("can I onboard?",
  "are my funds safe?"). They double as the most accurate spec of
  expected behaviour.
