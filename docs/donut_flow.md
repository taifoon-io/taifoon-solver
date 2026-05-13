# Donut Flow — End-to-End Architecture

This is the canonical architecture document for the Taifoon donut
attestation flow: how a Spinner operator runs the solver, how every fill
produces a signed attestation, and how the **internal redistribution of
the upstream adapter-owner inflow** routes 70 / 20 / 10 to the right
addresses for every adapter this Spinner runs.

Treat this file as the single source of truth. The dashboard onboarding
flow, the `SECURITY_ONBOARDING.md` runbook, and the per-crate doc
comments all derive from this.

---

## 1. The actors

| Actor | Role | Owns |
|---|---|---|
| **Spinner** | Operator pod running the solver binary | Their own EVM + Solana keys (Keychain), the wallet capital, the binary process. **Also the registered adapter owner with the upstream order contract.** |
| **Solver module** | A protocol-specific fill path | Inside the Spinner's binary — one module per protocol family |
| **adapter_builder** | Author of an adapter integration | The 70% share of the redistributed inflow at their EVM address |
| **adapter_reviewers** | Open-mamba code-review agents registered for the adapter | The 20% share, split equally between addresses listed in the registry |
| **adapter_ecosystem** | Catch-all ecosystem treasury | The 10% share AND any fail-closed routing |

No one centrally hosts a Spinner. Each operator runs their own binary on
their own machine, holding their own keys. The Taifoon hosting registry
at `solver.taifoon.dev/api/hosting/*` is a *directory* (fleet visibility +
API-token issuance), not a custody service.

---

## 2. Where the donut amount comes from

**Critical point that anchors everything else**: the donut base is the
**upstream adapter-owner inflow** that the Spinner's wallet receives on
each fill, NOT the protocol fee itself and NOT the realised profit.

The Spinner is registered with the upstream order contract / fee
distributor as the adapter owner. On each fill, the upstream fee
distributor splits the protocol-level fee (its own concern) and routes a
small per-fill **inflow** to the Spinner's wallet as the adapter-owner
share. The donut attestation records how the Spinner **redistributes that
inflow internally** to the three recipient purposes.

Under the current arrangement the inflow equals the SSE-decoded fee
component on the intent — the Spinner's executor reads it from the
Genome SSE feed and writes it into `OutcomeRecord.fee_usd`. Per-protocol
decoding rules live on the `OutcomeRecord::fee_usd` doc comment in
`crates/executor/src/outcome_log.rs:53-71`:

| Protocol | Source of `fee_usd` (= inflow under current arrangement) |
|---|---|
| Across V3 | `inputAmount − outputAmount` × token-USD-price |
| deBridge DLN | `giveAmount − takeAmount` × token-USD-price |
| Mayan Swift | auction-winning fee declared in the intent |
| LiFi | embedded relay fee in the calldata |
| Wormhole NTT | bridge-fee field on the NTT message |

The redistribution fraction defaults to `(1, 1)` = 100% of inflow gets
redistributed. Adapters that need to retain operational margin can
declare a per-adapter override in `config/adapter_registry.json` via the
optional `donut_bps_num` / `donut_bps_den` fields. The 70 / 20 / 10
split applies to whatever donut amount results — that fraction stays
uniform across every adapter this Spinner runs.

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
   │              AttestationPump                                       │
   │                ─ polls solver_outcomes for new executed rows       │
   │                ─ resolves adapter_id_for_outcome(&row)             │
   │                ─ looks up adapter_builder + adapter_reviewers      │
   │                  in AdapterRegistry (loaded from                   │
   │                  ./config/adapter_registry.json)                   │
   │                ─ CanonicalAdjudicator.attest(...)                  │
   │                  · canonical-JSON, EIP-191 signed                  │
   │                  · math: donut = max(0,inflow) ×                   │
   │                                  split_num / split_den             │
   │                          recipients =                              │
   │                            adapter_builder    (70%)                │
   │                            adapter_reviewers  (20%)                │
   │                            adapter_ecosystem  (10%, residual)      │
   │                ─ POST /api/donut/attest with Bearer token          │
   │                ─ chains attestations via prev_hash                 │
   └─────────────────────────────────┬──────────────────────────────────┘
                                     │
   ┌─────────────────────────────────▼──────────────────────────────────┐
   │                  PUBLIC AUDIT TRAIL (no auth)                       │
   │ ─────────────────────────────────────────────────────────────────── │
   │   GET /api/donut/policy                                             │
   │       → { split_num: 1, split_den: 1,                               │
   │           builder_num: 70, reviewers_num: 20, ecosystem_num: 10,    │
   │           split_share_den: 100,                                     │
   │           applies_to: "all_provisioned_adapter_inflows",            │
   │           adjudicator_version: "canonical-v2" }                     │
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
| Canonical math (integer micro-USD, per-adapter redistribution fraction) | `compute_redistribution_micro(inflow_micro, split_num, split_den)` | `src/lib.rs` |
| Canonical default (100% redistribution) | `compute_redistribution_micro_default(inflow_micro)` | `src/lib.rs` |
| USD → micro-USD boundary | `usd_to_micro(f64) -> i64` | `src/lib.rs` |
| Adapter-id resolver | `adapter_id_for_outcome(&OutcomeRecord)` | `src/lib.rs` |
| Sign + verify (EIP-191, low-`s`, hash chain) | `CanonicalAdjudicator::{attest, verify}` | `src/lib.rs` |
| Recipient share struct | `RecipientShare { purpose, addresses, share_usd_micro, share_num, share_den, is_residual }` | `src/lib.rs` |
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
| AttestationPump (background reconciler) | `attestation_pump::spawn_attestation_pump` | `crates/solver-main/src/attestation_pump.rs` |
| Outcome log (SQLite, append-only) | `OutcomeLog`, `OutcomeRecord` | `crates/executor/src/outcome_log.rs` |

### Config

| File | Purpose |
|---|---|
| `config/adapter_registry.json` | adapter_id → adapter_builder, adapter_reviewers, optional per-adapter redistribution-fraction override |
| `config/chain_wiring.json` | chain ID → RPC URL + contract addresses |
| `config/protocols_registry.json` | per-protocol metadata used by the genome decoder |

### Tests

| File | What it covers |
|---|---|
| `crates/donut-adjudicator/src/lib.rs` (`#[cfg(test)]`) | Math, signature, hash chain, adapter_id mapping, low-`s` rejection, fail-closed routing, residual-recipient invariant |
| `crates/solver-sandbox/tests/solana_attestation_sandbox.rs` | All four Solana protocols routed through the adjudicator with the same fixture pattern |
| `crates/solver-api/src/hosting.rs` (`#[cfg(test)]`) | SIWE nonce, EIP-55 casing, clock skew grace, persist round-trip, duplicate fill_id, bad chain link, spinner_id binding |
| `tests/integration/test_user_journey.py` | End-to-end HTTP rig: onboarding, fund safety, active fills across all Solana protocols, 70/20/10 redistribution, fail-closed, spinner_id binding regression |

---

## 5. What enforces "uniform across all adapters"

Three layers, in increasing order of trust required:

1. **Code layer.** The same `compute_redistribution_micro` is called for
   every attestation in every solver-main process. The 70 / 20 / 10
   share constants are `pub const` integer values. The per-adapter
   redistribution fraction is per-adapter via `AdapterRegistry::bps_for`
   — but it's pinned on each attestation (`split_num`, `split_den`) and
   re-verified by every reader. A Spinner cannot quietly change the
   policy on their own fills.

2. **Attestation layer.** Every fill produces a signed `DonutAttestation`
   that carries `inflow_usd_micro`, `split_num`, `split_den`,
   `donut_take_usd_micro`, and the full `recipients` map. The signature
   recovers to the Spinner's EVM address. `verify()` re-derives the
   math from `max(0, inflow_usd_micro) × split_num / split_den`, asserts
   the recipients sum to that exactly, and asserts exactly one recipient
   carries `is_residual: true`. The hash chain stops a Spinner from
   silently re-writing history.

3. **Public-audit layer.** `GET /api/donut/policy` publishes the
   canonical constants. `GET /api/donut/registry` publishes the
   adapter → builder map (with per-adapter override where applicable).
   `GET /api/donut/ledger/:spinner_id` publishes every attestation,
   signed and chained, for any auditor to replay. The claim
   "uniform across all adapters" is verifiable without trusting the
   Spinner — any reader can re-compute the math from
   `inflow × split_num / split_den × {70, 20, 10}` and confirm.

### How the 70/20/10 fits the self-hosted model

Every Spinner runs the binary on their own machine. The internal
redistribution defined here is **uniform across all adapters this
Spinner runs**. The upstream order contract / fee distributor handles
the protocol-level split (the Spinner's adapter-owner inflow vs. the
protocol's other downstream recipients) independently of this
attestation — that split is not the Spinner's concern and not encoded
in the donut math.

A future Taifoon-hosted-box service (with a 2FA-decryption + keyless-
signing project as a separate isolated provision layer) is researched
but **not part of this repo** — today every operator self-hosts.

---

## 6. Open work

- **On-chain settlement reconciler** — referenced in `LICENSE.md`. When
  an on-chain settlement contract for the redistribution publishes, the
  attestation flow stays — it becomes the *off-chain reconciler* for the
  on-chain settlement.
- **Per-platform RPC failover** in
  `crates/protocol-adapters-solana/src/send.rs` — current code uses one
  Solana RPC and retries on rate-limit; failover to a secondary endpoint
  would harden Mayan-Solana fills.

---

## 7. Where to start, depending on what you're doing

| You want to … | Start here |
|---|---|
| Onboard as a Spinner | `solver.taifoon.dev/onboard`, then `SECURITY_ONBOARDING.md` |
| Run a local dev rig | `python3 -m pytest tests/integration -v` (boots the test-server subprocess) |
| Read the math | `crates/donut-adjudicator/src/lib.rs::compute_redistribution_micro` |
| Add a new protocol adapter | `donut_adjudicator::default_adapter_id` + `config/adapter_registry.json` |
| Audit a Spinner's claimed redistribution | `GET /api/donut/ledger/<spinner_id>` then re-verify with `CanonicalAdjudicator::verify` |
| Verify the canonical policy | `GET /api/donut/policy` |
| Deploy a Spinner as a containerised box | `deploy/README.md` |
