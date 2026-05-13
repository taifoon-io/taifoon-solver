# Deployable adapter box

A containerised Spinner pod that **the operator runs on their own
hardware or cloud instance**. Taifoon does not host this for you — the
hosting registry at `solver.taifoon.dev` is a directory that tracks
operators by their registered address, not a custody or compute service.
You own the VM, you own the key, you own the SQLite outcome log.

The image ships `taifoon-solver` (the same binary the local dev rig
runs) wired against the live `api.taifoon.dev` API surface for intent
intake, V5 proofs, and attestation submission.

## 1. What the box does

- Runs `taifoon-solver` as a non-root process inside an Ubuntu 24.04
  container.
- Subscribes to `https://api.taifoon.dev/api/genome/subscribe/sse` for
  cross-chain intents.
- Decides which intents to fill (profit gating + max-notional cap).
- Broadcasts fills through the protocol adapters compiled into the
  binary (Across, deBridge DLN, Mayan Swift, Mayan Flash, Wormhole NTT,
  LiFi — both EVM and Solana paths).
- For every confirmed fill, signs a `DonutAttestation` recording the
  internal redistribution of the upstream adapter-owner inflow
  (`adapter_builder` 70% / `adapter_reviewers` 20% /
  `adapter_ecosystem` 10%) and POSTs it to the solver-api at
  `https://api.taifoon.dev/api/donut/attest` using the bearer token
  provisioned at `solver.taifoon.dev/onboard`.
- Exposes `/health` for liveness probes; mounts SQLite under
  `/data/outcomes` so an operator can `docker cp` the outcome log out
  at any time.

The box is **stateful but non-custodial** — your private key is mounted
read-only and never persisted outside the host filesystem you choose.

## 2. Env vars you must set

Required (no defaults):

| Variable | What it's for |
|---|---|
| `SOLVER_API_TOKEN` | Bearer token issued by `solver.taifoon.dev/onboard` |
| `SOLVER_PRIVATE_KEY` | 0x-prefixed 32-byte EVM secp256k1 hex |

Optional (defaults shown):

| Variable | Default | Purpose |
|---|---|---|
| `PORT` | `8082` | Host port to expose for `/health` + the local API |
| `DRY_RUN` | `true` | When `true`, the solver decides but does not broadcast |
| `MAX_NOTIONAL_USD` | `10` | Per-fill USD notional cap |
| `SPINNER_API_URL` | `https://api.taifoon.dev` | Upstream solver-api base URL |
| `GENOME_SSE_URL` | `https://api.taifoon.dev/api/genome/subscribe/sse` | SSE intent feed |
| `ADAPTER_REGISTRY_PATH` | `/config/adapter_registry.json` | Adapter → builder map inside the container |
| `SIWE_DOMAIN` | `solver.taifoon.dev` | Domain pinning for SIWE signatures |
| `RUST_LOG` | `info` | Log verbosity |

## 3. Bring it up

```bash
export SOLVER_API_TOKEN="…token from onboarding flow…"
export SOLVER_PRIVATE_KEY="0x…32 bytes hex…"
# Stay in DRY_RUN for the first run.
export DRY_RUN=true

./deploy/run.sh up
```

`run.sh` validates the required env, builds the image (first run only,
~6–8 min), then runs `docker compose -f deploy/docker-compose.yml up -d`.
On subsequent invocations the image is cached.

## 4. Confirm it's running

Three checks, increasing in depth:

1. **Container alive**
   ```bash
   ./deploy/run.sh status
   # → taifoon-solver  Up (healthy)
   ```
2. **Local health endpoint reachable**
   ```bash
   curl http://127.0.0.1:${PORT:-8082}/health
   # → {"status":"ok","service":"taifoon-solver-api"}
   ```
3. **Attestations flowing to the upstream solver-api**
   - First derive your `spinner_id`: the first 8 hex chars (lowercase,
     no `0x`) of the EVM address that corresponds to
     `SOLVER_PRIVATE_KEY`.
   - Then query the public ledger endpoint:
     ```bash
     SPINNER_ID="$(cast wallet address --private-key "$SOLVER_PRIVATE_KEY" \
       | tr 'A-Z' 'a-z' | sed 's/^0x//' | cut -c 1-8)"
     curl "https://api.taifoon.dev/api/donut/ledger/${SPINNER_ID}/head"
     # → {"spinner_id":"…","prev_hash":"0x…","count":N}
     ```
   - After your first successful fill (still in `DRY_RUN=true` you'll
     see decisions but no fills — flip to `DRY_RUN=false` to go live),
     `count` increments and `prev_hash` advances.

When `count` matches the number of fills you've observed in the
container logs, the box is fully wired end-to-end.

## 5. Going live

Once a few `DRY_RUN=true` cycles look right in the logs:

```bash
./deploy/run.sh down
export DRY_RUN=false
export MAX_NOTIONAL_USD=25   # raise gradually
./deploy/run.sh up
```

Watch logs with `./deploy/run.sh logs`. The first live attestation is
the green light — verify it appears in
`https://api.taifoon.dev/api/donut/ledger/<your_spinner_id>`.

## 6. Troubleshooting

- **Image build fails on `cargo build`** — host needs at least 8 GB RAM
  free; the LTO link step is the peak. Re-run; cached layers shorten
  retries to a few minutes.
- **`SOLVER_PRIVATE_KEY must be set` from `run.sh`** — verify the var is
  exported in the same shell session that calls `run.sh` (or wire it
  into a `.env` file Docker Compose picks up via `--env-file`).
- **`/health` returns 200 but no attestations appear** — confirm
  `SOLVER_API_TOKEN` matches what `solver.taifoon.dev/onboard`
  displayed; rotate via re-provision if you lost it.
- **`adapter_registry not loadable …` warning** — mount your own
  registry at `/config/adapter_registry.json` (override the default
  shipped in the image with a bind-mount in `docker-compose.override.yml`).
