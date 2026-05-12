# Donut Flow — End-to-End Architecture

This is the canonical architecture document for the Taifoon donut fee-split
flow: how a Spinner operator runs the solver, how every fill produces a
signed attestation, and how the 70 / 20 / 10 split routes to the right
addresses across **all provisioned builders**.

Treat this file as the single source of truth. The dashboard onboarding
flow, the `SECURITY_ONBOARDING.md` runbook, and the per-crate doc
comments all derive from this.

---

## 1. The actors

| Actor | Role | Owns |
|---|---|---|
| **Spinner** | Operator pod running the solver binary | Their own EVM + Solana keys (Keychain), the wallet capital, the binary process |
| **Solver module** | A protocol-specific fill path | Inside the Spinner's binary — one module per protocol family |
| **Builder** | Author of an adapter integration | The 70% donut creator-share at their EVM address |
| **Reviewer set** | Open-mamba code-review agents in the upstream Spinner OS | The 20% reviewer-share, split equally |
| **Ecosystem treasury** | Catch-all bucket | The 10% ecosystem-share AND any fail-closed routing |

No one centrally hosts a Spinner. Each operator runs their own binary on
their own machine, holding their own keys. The Taifoon hosting registry
at `solver.taifoon.dev/api/hosting/*` is a *directory* (fleet visibility +
API-token issuance), not a custody service.

---

## 2. Where the donut amount comes from

**Critical point that anchors everything else**: the donut base is the
**SSE-decoded fee** that the Spinner collects for filling an intent, NOT
the realised profit after gas. The donut is a tax on revenue (fees), not
a tax on margin.

Why: Across-style relayer fees, deBridge spreads, Mayan auction premia,
LiFi embedded fees — these are all **declared in the intent at submission
time** and stream over the Genome SSE feed. The Spinner reads them
before broadcasting the fill. The donut is `(per-adapter bps) × fee`, the
Spinner pays gas out of their net.

Per-protocol decoding rules live on the `OutcomeRecord::fee_usd` doc
comment in `crates/executor/src/outcome_log.rs:53-71`:

| Protocol | Source of `fee_usd` |
|---|---|
| Across V3 | `inputAmount − outputAmount` × token-USD-price |
| deBridge DLN | `giveAmount − takeAmount` × token-USD-price |
| Mayan Swift | auction-winning fee declared in the intent |
| LiFi | embedded relay fee in the calldata |
| Wormhole NTT | bridge-fee field on the NTT message |

The `49 bps` we used to advertise is **the canonical default** — applied
when an adapter doesn't declare its own rate. Protocols with different
economics (auctions vs. relays vs. meta-aggregators) can declare a
per-adapter rate in `config/adapter_registry.json` via the optional
`donut_bps_num` / `donut_bps_den` fields. The 70 / 20 / 10 split applies
to whatever donut amount results — that fraction stays uniform.

---

## 3. The end-to-end flow

```
                          ┌───────────────────────────────────────────┐
                          │     solver.taifoon.dev/onboard            │
                          │  (dashboard/app/onboard/page.tsx)          │
                          └───────────────────────────────────────────┘
                                            │
                          ┌─── 1. CONNECT WALLET ───┐
                          │  wagmi + viem injected  │
                          │  + WalletConnect v2     │
                          └───────────┬─────────────┘
                                      │
                          ┌─── 2. SIWE NONCE ───────┐
                          │ POST /api/hosting/      │
                          │      siwe-nonce         │
                          │ → 32-byte nonce, 5-min  │
                          │   TTL, single-use       │
                          └───────────┬─────────────┘
                                      │
                          ┌─── 3. SIGN SIWE ────────┐
                          │ EIP-4361 message,       │
                          │ EIP-55 address,         │
                          │ pinned domain + chain,  │
                          │ personal_sign           │
                          └───────────┬─────────────┘
                                      │
                          ┌─── 4. PROVISION ────────┐
                          │ POST /api/hosting/      │
                          │      provision          │
                          │ → solver_id +           │
                          │   api_token (shown      │
                          │   once, 24 random       │
                          │   bytes via OsRng)      │
                          └───────────┬─────────────┘
                                      │
                          ┌─── 5. INSTALL ──────────┐
                          │ cargo install …         │
                          │   taifoon-solver        │
                          │ security add-           │
                          │   generic-password      │
                          │   -s mamba-messiah-key  │
                          │ + config/adapter_       │
                          │   registry.json fetch   │
                          └───────────┬─────────────┘
                                      │
                  ┌───────────────────▼───────────────────┐
                  │      OPERATOR'S MACHINE (Spinner)      │
                  │      ─────────────────────────────     │
                  │      run-mainnet.sh                    │
                  │      DRY_RUN=false                     │
                  │      MAX_NOTIONAL_USD=10               │
                  │      SPINNER_API_TOKEN=…               │
                  │      ADAPTER_REGISTRY_PATH=…           │
                  └───────────────────┬────────────────────┘
                                      │
   ┌──────────────────────────────────▼─────────────────────────────────┐
   │                       RUNTIME LOOP                                  │
   │ ─────────────────────────────────────────────────────────────────── │
   │   Genome SSE ─► intent_filter ─► profit_calc ─► lambda_controller   │
   │                                                       │             │
   │              ┌────────────────┬───────────────────┬───┴───────┬──┐ │
   │              ▼                ▼                   ▼           ▼  │ │
   │        Across EVM       Mayan Solana         DLN Solana   Mayan… │ │
   │              │                │                   │           │  │ │
   │              └────────────────┼───────────────────┴───────────┘  │ │
   │                               ▼                                    │
   │              append_outcome / append_solana_confirmed              │
   │              writes OutcomeRecord with:                            │
   │                ─ fee_usd        (decoded from SSE intent)          │
   │                ─ actual_profit_usd (fee − gas)                     │
   │              to ./outcomes/mainnet_<ts>.sqlite                     │
   │                               │                                    │
   │                               ▼                                    │
   │              AttestationPump  (TODO: planned crate)                │
   │                ─ polls solver_outcomes for new executed rows       │
   │                ─ resolves adapter_id_for_outcome(&row)             │
   │                ─ looks up builder + reviewers + bps in             │
   │                  AdapterRegistry (loaded from                      │
   │                  ./config/adapter_registry.json)                   │
   │                ─ CanonicalAdjudicator.attest(...)                  │
   │                  · canonical-JSON, EIP-191 signed                  │
   │                  · math: donut = fee × bps_num / bps_den           │
   │                          70/20/10 of that donut                    │
   │                ─ POST /api/donut/attest with Bearer token          │
   │                ─ chains attestations via prev_hash                 │
   └─────────────────────────────────┬──────────────────────────────────┘
                                     │
   ┌─────────────────────────────────▼──────────────────────────────────┐
   │                  PUBLIC AUDIT TRAIL (no auth)                       │
   │ ─────────────────────────────────────────────────────────────────── │
   │   GET /api/donut/policy                                             │
   │       → { donut_bps_num: 49, donut_bps_den: 10000,                  │
   │           creator_num: 70, reviewer_num: 20, ecosystem_num: 10,     │
   │           applies_to: "all_provisioned_adapters",                   │
   │           adjudicator_version: "canonical-v1" }                     │
   │                                                                     │
   │   GET /api/donut/registry                                           │
   │       → { ecosystem, adapters: {                                    │
   │             "mayan-solana-swift-v1": { builder, reviewers, [bps] }, │
   │             "mayan-flash-solana-v1": { … },                         │
   │             "wormhole-ntt-solana-v1": { … },                        │
   │             "debridge-dln-solana-v1": { … },                        │
   │             … } }                                                   │
   │                                                                     │
   │   GET /api/donut/ledger/:spinner_id                                 │
   │       → signed attestation chain, hash-linked, oldest first         │
   │                                                                     │
   │   GET /api/donut/ledger/:spinner_id/head                            │
   │       → { prev_hash, count } — what the next attestation must use   │
   └─────────────────────────────────────────────────────────────────────┘
```

---

## 4. Component → file map

The map is exhaustive. Every box in the diagram has a code home.

### Onboarding (`dashboard/`)

| Box | File |
|---|---|
| Onboard wizard page | `dashboard/app/onboard/page.tsx` |
| Connect-wallet step | `dashboard/components/onboard/WalletConnectStep.tsx` |
| Provisioned-solver result | `dashboard/components/onboard/ProvisionedSolver.tsx` |
| Wagmi config | `dashboard/lib/wagmi.ts` |
| Providers wrapper | `dashboard/lib/providers.tsx` |

### Hosting API (`crates/solver-api/`)

| Box | Symbol | Location |
|---|---|---|
| `POST /api/hosting/siwe-nonce` | `siwe_nonce_handler` | `src/hosting.rs` |
| `POST /api/hosting/provision` | `provision_handler` | `src/lib.rs` / `src/hosting.rs::provision` |
| `GET  /api/hosting/solvers/:id` | `get_solver_handler` | `src/hosting.rs` |
| SIWE verifier (async, EIP-55, clock-skew, single-use nonce, hard-capped store) | `verify_siwe` + `issue_siwe_nonce` + `consume_siwe_nonce` | `src/hosting.rs` |
| API-token Bearer middleware | `require_solver_api_token` | `src/lib.rs` |

### Donut API (`crates/solver-api/`)

| Box | Symbol | Location |
|---|---|---|
| `POST /api/donut/attest` | `donut_attest_handler` (Bearer-gated) | `src/lib.rs` |
| `GET  /api/donut/ledger/:spinner_id` | `donut_ledger_handler` (public) | `src/lib.rs` |
| `GET  /api/donut/ledger/:spinner_id/head` | `donut_head_handler` (public) | `src/lib.rs` |
| `GET  /api/donut/policy` | `donut_policy_handler` (public) | `src/lib.rs` |
| `GET  /api/donut/registry` | `donut_registry_handler` (public) | `src/lib.rs` |
| `persist_attestation` (transactional INSERT + UPDATE, spinner_id ↔ recovered-addr binding) | `HostingRegistry::persist_attestation` | `src/hosting.rs` |

### Donut adjudicator (`crates/donut-adjudicator/`)

| Box | Symbol | Location |
|---|---|---|
| Canonical math (integer micro-USD, per-adapter bps) | `compute_split_micro(fee_micro, bps_num, bps_den)` | `src/lib.rs` |
| Canonical default | `compute_split_micro_default(fee_micro)` | `src/lib.rs` |
| USD → micro-USD boundary | `usd_to_micro(f64) -> i64` | `src/lib.rs` |
| Adapter-id resolver | `adapter_id_for_outcome(&OutcomeRecord)` | `src/lib.rs` |
| Sign + verify (EIP-191, low-`s`, hash chain) | `CanonicalAdjudicator::{attest, verify}` | `src/lib.rs` |
| Policy struct | `DonutPolicy::canonical()` | `src/lib.rs` |
| Registry loader | `AdapterRegistry::load_default()` / `load_from_path` | `src/lib.rs` |
| Registry view (public JSON) | `AdapterRegistryView`, `AdapterRegistryEntry` | `src/lib.rs` |

### Solver runtime (`crates/executor/`, `crates/solver-main/`)

| Box | Symbol | Location |
|---|---|---|
| Intent stream (SSE poller) | `genome_client::*` | `crates/genome-client/` |
| EVM fill path | `LambdaController::lambda_execute` + `AcrossExecutor::execute` | `crates/executor/src/lambda_controller.rs`, `across_executor.rs` |
| Solana fill paths (Mayan Swift, Mayan Flash, Wormhole NTT, deBridge DLN) | branches inside `lambda_execute` | `crates/executor/src/lambda_controller.rs` |
| OutcomeRecord write on confirmed Solana fill | `LambdaController::append_solana_confirmed` | `crates/executor/src/lambda_controller.rs` |
| MESSIAH key bootstrap (Keychain → secp256k1 signer) | `messiah::load_messiah_signer` | `crates/solver-main/src/messiah.rs` |
| Solana key bootstrap | `keychain::load_solana_signer` | `crates/protocol-adapters-solana/src/keychain.rs` |
| Outcome log (SQLite, append-only) | `OutcomeLog`, `OutcomeRecord` | `crates/executor/src/outcome_log.rs` |

### Config

| File | Purpose |
|---|---|
| `config/adapter_registry.json` | adapter_id → builder, reviewer set, optional per-adapter bps override |
| `config/chain_wiring.json` | chain ID → RPC URL + contract addresses |
| `config/protocols_registry.json` | per-protocol metadata used by the genome decoder |

### Tests

| File | What it covers |
|---|---|
| `crates/donut-adjudicator/src/lib.rs` (`#[cfg(test)]`) | Math, signature, hash chain, adapter_id mapping, low-`s` rejection, fail-closed routing |
| `crates/solver-sandbox/tests/solana_attestation_sandbox.rs` | All four Solana protocols routed through the adjudicator with the same fixture pattern |
| `crates/solver-api/src/hosting.rs` (`#[cfg(test)]`) | SIWE nonce, EIP-55 casing, clock skew grace, persist round-trip, duplicate fill_id, bad chain link, spinner_id binding |
| `tests/integration/test_user_journey.py` | End-to-end HTTP rig: onboarding, fund safety, active fills across all Solana protocols, 70/20/10 split, fail-closed, spinner_id binding regression |

---

## 5. What enforces "uniform across all builders"

Three layers, in increasing order of trust required:

1. **Code layer.** The same `compute_split_micro` is called for every
   attestation in every solver-main process. The 70 / 20 / 10 fractions
   are `pub const` integer constants. The donut rate is per-adapter via
   `AdapterRegistry::bps_for` — but it's pinned on each attestation and
   re-verified by every reader. A Spinner cannot quietly change the
   policy on their own fills.

2. **Attestation layer.** Every fill produces a signed `DonutAttestation`
   that carries `donut_bps_num`, `donut_bps_den`, `creator_share_usd_micro`,
   `reviewer_share_usd_micro`, `ecosystem_share_usd_micro`. The signature
   recovers to the Spinner's EVM address. `verify()` re-derives the math
   from `fee_usd_micro × bps` and rejects any deviation. The hash chain
   stops a Spinner from silently re-writing history.

3. **Public-audit layer.** `GET /api/donut/policy` publishes the canonical
   constants. `GET /api/donut/registry` publishes the adapter → builder
   map (with per-adapter bps where applicable). `GET /api/donut/ledger/:spinner_id`
   publishes every attestation, signed and chained, for any auditor to
   replay. The TSUL claim "uniform across all builders" is verifiable
   without trusting the Spinner — any reader can re-compute the math from
   `fee_usd_micro × bps × {70, 20, 10}` and confirm.

What this stack does NOT enforce: the *actual on-chain payout* of the
70 % to the Builder. That requires the `BuildersRegistry` contract in
the upstream Spinner OS to actually deploy on-chain and intercept the
settlement leg. Today the attestation is the audit trail; the contract
is the planned next step (see `LICENSE.md` and the TSUL pointer).

---

## 6. Open work

What's NOT yet wired:

- **AttestationPump** — the background task in `solver-main` that reads
  `solver_outcomes` SQLite, signs attestations, POSTs to `/api/donut/attest`.
  The plumbing for it (outcome log, adjudicator, route, registry loader)
  is all in place; the loop binding them is not. See `crates/solver-main/src/main.rs`
  for where to hook it.
- **On-chain `BuildersRegistry`** — referenced in `LICENSE.md` and the
  upstream `yawningmonsoon/spinner` repo (currently private). When that
  contract publishes, the attestation flow stays — it becomes the
  *off-chain reconciler* for the on-chain settlement.
- **Dashboard `/policy` page** — reads `/api/donut/policy` + `/api/donut/registry`
  and renders the adapter → builder table for visitors.

---

## 7. Where to start, depending on what you're doing

| You want to … | Start here |
|---|---|
| Onboard as a Spinner | `solver.taifoon.dev/onboard`, then `SECURITY_ONBOARDING.md` |
| Run a local dev rig | `python3 -m pytest tests/integration -v` (boots the test-server subprocess) |
| Read the math | `crates/donut-adjudicator/src/lib.rs::compute_split_micro` |
| Add a new protocol adapter | `donut_adjudicator::default_adapter_id` + `config/adapter_registry.json` |
| Audit a Spinner's claimed donut | `GET /api/donut/ledger/<spinner_id>` then re-verify with `CanonicalAdjudicator::verify` |
| Verify the canonical policy | `GET /api/donut/policy` |
