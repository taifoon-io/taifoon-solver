# Spinner SYNC Sandbox Setup — Base Sepolia first, Solana Devnet next

Purpose: stand up a **local** spinner (genome SSE + V5 proof API) on Base Sepolia
and point `solver-main` at it so the full estimate-pipeline (col-p1) and lambda
controller (col-p4) can be exercised end-to-end without touching the production
node at `46.4.96.124:30081`.

The default production URL is fine when you want to consume real protocol
events. For sandbox runs — especially anything that posts attempt-bundles, runs
test-fills, or claims revenue — use a **local** spinner so you don't pollute
real ledgers.

---

## 1) Start spinner locally (Base Sepolia)

Spinner is split into two binaries; both must be running.

### 1a. Header collector / genome broadcaster — `spinner-bin`

The writer process. Collects Base Sepolia headers + transactions, writes RocksDB,
runs the MMR indexer, and broadcasts protocol events on the genome SSE channel.

```bash
cd /Users/mbultra/projects/spinner/rust

# Sandbox config — Base Sepolia only. Create configs/spinner-base-sepolia.json
# from an existing base config in spinner/rust (search for chain_id 84532), or
# copy and edit configs/spinner-config.json down to a single chain entry.
cargo run --release -p spinner-bin -- \
    --config configs/spinner-base-sepolia.json \
    --storage-path ./sandbox-data \
    --api-port 30081 \
    --metrics-port 9091
```

Notes:
- `--api-port 30081` enables the in-process DA API (genome SSE + V5 proof) so a
  separate `da-api-server` process is **not** required for the sandbox path.
  Skip step 1b unless you intentionally want to run them split.
- `--storage-path ./sandbox-data` keeps the RocksDB out of `/tmp/taifoon-rocksdb`
  so the prod node's data is never touched if you happen to be on the same host.
- Confirm Base Sepolia is wired in `configs/spinner-base-sepolia.json` —
  network entry with `chain_id: 84532`, an RPC endpoint, and `chain_type: "evm"`.

### 1b. (optional) Standalone read-only DA API — `da-api-server`

Only needed if you want to read the same RocksDB from a second process while
spinner-bin keeps the writer lock. The server opens RocksDB **read-only** so it
can coexist with a running spinner-bin.

```bash
cd /Users/mbultra/projects/spinner/rust
cargo run --release -p da-api --bin da-api-server -- \
    --storage-path ./sandbox-data \
    --bind-addr 127.0.0.1:30082
```

If you do this, `SPINNER_API_URL` for solver-main can point at either port — the
in-process one (30081) serves both writes and reads; the standalone one (30082)
is read-only.

### 1c. Smoke-test the local endpoints

```bash
# Health
curl -s http://127.0.0.1:30081/health

# Genome SSE — should hold open and stream events as Base Sepolia advances
curl -N http://127.0.0.1:30081/api/genome/subscribe/sse | head -c 4096

# V5 proof bundle (requires a real tx_hash + log_index from a Base Sepolia event)
curl -s -X POST http://127.0.0.1:30081/api/v5/proof/bundle \
    -H 'content-type: application/json' \
    -d '{"protocol":"across","order_id":"sandbox-1","src_chain_id":84532,
         "tx_hash":"0x...","log_index":0}' | jq .
```

If the `/api/v5/proof/bundle` POST returns `404 Order not found` or fails on
`Failed to fetch transaction`, the spinner has not yet collected/finalized the
referenced tx. Wait for the genome SSE to surface a fresh Base Sepolia order
event, then re-issue with that tx_hash.

---

## 2) Point solver-main at the local spinner

`crates/solver-main/src/main.rs` reads three URL/env vars at startup
(`main.rs:33-49`):

| Env var               | Default (production)                                            | Sandbox value                                  |
|-----------------------|-----------------------------------------------------------------|------------------------------------------------|
| `GENOME_SSE_URL`      | `http://46.4.96.124:30081/api/genome/subscribe/sse`             | `http://127.0.0.1:30081/api/genome/subscribe/sse` |
| `SPINNER_API_URL`     | `http://46.4.96.124:30081`                                      | `http://127.0.0.1:30081`                       |
| `PROTOCOL_FILTER`     | `across`                                                        | `across` (Base Sepolia has Across deployments) |
| `MIN_PROFIT_USD`      | `0.10`                                                          | `0.0` (sandbox — accept tiny synthetic fills)  |
| `DRY_RUN`             | `true`                                                          | `true` until col-p4 has been validated end-to-end |
| `OUTCOME_DB_PATH`     | `/tmp/taifoon_solver_outcomes.sqlite`                           | `./sandbox-data/solver_outcomes.sqlite`        |
| `MAMBA_LAKE_URL`      | (unset → no skip-rules)                                         | leave unset                                    |
| `ETH_RPC_URL`         | (used by tests / estimate path against Ethereum mainnet)        | `https://sepolia.base.org` for Base Sepolia    |
| `MESSIAH_KEY` source  | macOS keychain entry `mamba-messiah-key`                        | a **separate** sandbox keychain entry — see below |

### MESSIAH key (sandbox)

The signing key is loaded only via the macOS keychain helper in solver-main
(`load_messiah_key`). Do **not** reuse `mamba-messiah-key` for the sandbox.
Create a dedicated entry funded only with Base Sepolia ETH:

```bash
# Generate a fresh sandbox EOA externally (cast wallet new, etc.), then:
security add-generic-password -a "$USER" -s mamba-messiah-key-sandbox \
    -w '<sandbox-private-key>'
```

Then patch `load_messiah_key` to read `mamba-messiah-key-sandbox` when
`SANDBOX_MODE=true`, OR temporarily rename the prod entry while sandbox-testing
(safer: use a sandbox-only mac user account). Never write the sandbox key to a
file or env var.

### One-shot launch

```bash
cd /Users/mbultra/projects/taifoon-solver
GENOME_SSE_URL=http://127.0.0.1:30081/api/genome/subscribe/sse \
SPINNER_API_URL=http://127.0.0.1:30081 \
PROTOCOL_FILTER=across \
MIN_PROFIT_USD=0.0 \
DRY_RUN=true \
OUTCOME_DB_PATH=./sandbox-data/solver_outcomes.sqlite \
ETH_RPC_URL=https://sepolia.base.org \
cargo run --release -p solver-main
```

---

## 3) V5 proof feedback loop

End-to-end shape (anchor in code, not invented):

1. **Event in** — `GenomeClient` (`crates/genome-client`) subscribes to
   `GENOME_SSE_URL` and surfaces `GenomeEvent` records.
2. **Intent build** — `Intent::from_genome_event` extracts the protocol-specific
   fields the estimate adapter needs (`across`: `deposit_id`, `output_amount`;
   `debridge`: `maker_order_nonce`, `give_amount`, `take_amount`, `order_id`;
   `mayan_solana`: `swift_program_id`, `state_account`, `vault_account`,
   `compute_units_estimate`, `is_solana_source`).
3. **Estimate** — `EstimateAdapter::estimate(intent)` runs the protocol-specific
   simulator (`AcrossEstimateAdapter`, `MayanEvmEstimateAdapter`,
   `MayanSolanaEstimateAdapter`, …) and returns one of:
   - `EstimateOutcome::OkGas(u64)` / `OkComputeUnits(u64)` — calldata is correct
     (GREEN; calldata reached the validator/RPC and got a positive estimate)
   - `EstimateOutcome::InsufficientFundsLike(String)` /
     `InsufficientLamports(String)` — wallet underfunded (GREEN; calldata is
     correct, the only issue is balance)
   - `EstimateOutcome::Reverted(String)` — protocol-level reject; ABI mismatch
     or stale order (RED — real bug)
   - `EstimateOutcome::AbiInvalid(String)` — couldn't even encode calldata
     (RED — Rust-side type mismatch)
4. **Attempt-bundle write** — every adapter calls
   `executor::estimate::write_attempt_bundle(spinner_base, &bundle)` with a
   serialized `AttemptBundle` (intent_id, protocol, outcome, gas_or_error,
   calldata_hex, ts). This is best-effort: the helper logs `tracing::warn` and
   continues if the spinner is unreachable or the endpoint 404s.
5. **(spinner side, TODO)** — the corresponding
   `POST /api/v5/proof/bundle/attempt` endpoint **is not yet implemented**
   server-side. Code reference and confirmed gap:
   - Solver writer:
     `crates/executor/src/estimate.rs:177` (`write_attempt_bundle` posts to
     `{spinner_base}/api/v5/proof/bundle/attempt`).
     `crates/executor/src/estimate.rs:14` documents the gap as
     `TODO(spinner): add /api/v5/proof/bundle/attempt endpoint server-side`.
   - Spinner reader: `spinner/rust/crates/da-api/src/api.rs:3906-3907` only
     mounts `/api/v5/proof/bundle` (POST, generator) and
     `/api/v5/proof/bundle/:protocol/:order_id` (GET, by-order). No `/attempt`
     route exists, which is why the writer is best-effort.
6. **Forward path (col-p4)** — once an estimate returns OkGas/OkComputeUnits,
   `lambda_execute(intent)` will:
   - `wallet_manager.reserve(intent.amount_usd)` (`crates/wallet-manager`,
     col-p2 — already implemented),
   - `spinner.test_run(intent)` — issue a dry-run fill against the local
     spinner,
   - `spinner.fetch_v5_proof(intent_id)` — call
     `GET /api/v5/proof/bundle/:protocol/:order_id` (the existing endpoint at
     `solver_proof_api.rs:409`) to get the V5 proof blob,
   - `build_adapter_calldata(proof, intent)` — adapter-specific (see
     `crates/protocol-adapters` and `crates/executor/src/across_executor.rs`),
   - broadcast `executeWithProof(proof, adapter, calldata)` against the
     `TaifoonUniversalOperator`,
   - on `CONFIRMED` receipt: `wallet_manager.release(intent_id)` and emit a
     genome feedback event,
   - `lambda_claim(intent_id)` — call `claim()` on the Universal Operator and
     `wallet_manager.record_revenue(fee_usd)`.

The V5 proof response shape (from
`spinner/rust/crates/da-api/src/solver_proof_api.rs:202`):

```jsonc
{
  "protocol": "across",
  "order_id": "0x...",
  "proof": { /* full V5ProofBlob — L1..L6 layers */ },
  "transaction": {
    "chain_id": 84532,
    "block_number": 12345678,
    "tx_hash": "0x...",
    "tx_index": 3,
    "log_index": 0,
    "contract_address": "0x...",
    "topics": ["0x...", ...],
    "data": "0x..."
  },
  "metadata": {
    "superroot_hash": "0x...",
    "finality_type": "l2_output_root",
    "is_finalized": true,
    "finality_block": 12345700,
    "proof_size_bytes": 4321
  }
}
```

`metadata.is_finalized` MUST be `true` before lambda_execute submits the
on-chain `executeWithProof` — unfinalized bundles still pass the cache through
but reference an L6 commitment that may advance, which would invalidate the
proof at verification time.

---

## 4) Sandbox checklist

### Phase 1 — Base Sepolia (do this first)

- [ ] `configs/spinner-base-sepolia.json` exists with `chain_id: 84532` only
- [ ] `cargo run -p spinner-bin -- --config configs/spinner-base-sepolia.json --storage-path ./sandbox-data --api-port 30081` runs and reaches "Ready"
- [ ] `curl http://127.0.0.1:30081/health` returns 200
- [ ] `curl -N http://127.0.0.1:30081/api/genome/subscribe/sse` streams real Base Sepolia events
- [ ] Sandbox keychain entry created (separate from `mamba-messiah-key`); EOA funded with Base Sepolia ETH from a faucet
- [ ] solver-main launches with the env-var block in §2, log shows
      `📡 Genome SSE: http://127.0.0.1:30081/...` and
      `🔌 Spinner API: http://127.0.0.1:30081`
- [ ] `bin/estimate_one across tests/fixtures/across.json` against the sandbox prints `Outcome: ok_gas [GREEN]` or `insufficient_funds_like [GREEN]`
- [ ] `wallet-manager` `/api/wallet/status` and `/api/wallet/intents` reachable; an `IntentDetected` row lands when the genome surfaces a Base Sepolia Across deposit
- [ ] (col-p4) `lambda_execute` reaches `Broadcast` → `PendingConfirmation` → `Confirmed` against Base Sepolia
- [ ] (col-p4) `lambda_claim` records non-zero revenue in `wallet-manager` revenue ledger
- [ ] `cargo test --workspace` clean from the sandbox checkout

### Phase 2 — Solana Devnet (only after Base Sepolia is green)

- [ ] Devnet RPC env: `SOLANA_RPC_URL=https://api.devnet.solana.com`
- [ ] Spinner config extended with a Solana entry (chain_type: `solana`,
      cluster: `devnet`)
- [ ] Sandbox Solana key in keychain entry `mamba-messiah-solana-key-sandbox`
      (mirrors the `mamba-messiah-solana-key` loader path); funded with devnet
      SOL from a faucet
- [ ] `bin/estimate_one mayan_solana tests/fixtures/mayan_solana.json` against
      the sandbox prints `Outcome: ok_compute_units [GREEN]` OR
      `insufficient_lamports [GREEN]`
- [ ] Genome surfaces a real Mayan Swift order on Solana devnet, lambda_execute
      submits via `MayanSolanaSimulator` calldata, devnet tx confirms
- [ ] (deferred) Tighten the `Reverted` substring set in
      `mayan_solana_fixture_estimates_clean` before broadcast lane ships —
      flagged as non-blocking by code-reviewer at B.3

---

## Notes

- **Order matters.** Base Sepolia is EVM, mirrors the Across/deBridge/Mayan-EVM
  estimate paths that already exist in `crates/executor`. Solana Devnet
  exercises the bespoke `MayanSolanaSimulator` legacy-tx encoding and must come
  second so any Solana-specific surprises don't gate the EVM path.
- **Don't push from the sandbox.** Auto-push is on for the repo; commits made
  while pointed at the sandbox spinner can still leak into origin/master. Treat
  the sandbox as a runtime config switch, not a branch.
- **Attempt-bundle TODO.** Until the spinner-side `/api/v5/proof/bundle/attempt`
  endpoint lands (see §3 step 5), the writer's best-effort posts will surface
  as `tracing::warn` lines — these are expected, not a failure.
