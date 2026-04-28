# Solver Registration Flows

Three trigger paths converge on the same backend webhook, which calls
`k8s/register-webhook.sh` to provision a per-solver sidecar pod in
namespace `solvers`. Every path produces an `onboard:<solver_id>` genome
event so the lifecycle is auditable end-to-end in open-mamba.

```
                +--------------------+
                | k8s/register-      |
                | webhook.sh         |
                |                    |
   (1) Web UI ->|  POST /register    |
   (2) LLM    ->|  kubectl secret    |-> Deployment solver-<id> running
   (3) API    ->|  kubectl apply     |   in namespace `solvers`
                +--------------------+
```

## 1. Web UI (taifoon.dev)

Highest priority. Human operator drives the flow.

1. User fills the register form on `taifoon.dev/solvers/new`.
2. Frontend `POST`s to `${TAIFOON_API_BASE}/api/solvers/register` with
   `{wallet, chains}`.
3. `taifoon-mamba` backend mints a `solver_id`, persists the row, and
   ingests an `onboard` event into open-mamba with
   `{project: "solvers", payload: {solver_id, wallet, chains}}`.
4. The mamba worker exports `SOLVER_ID`, `SOLVER_PRIVATE_KEY`,
   `CHAIN_WIRING_JSON`, `SOLVER_WALLET_ADDR` and runs
   `k8s/register-webhook.sh`.
5. Pod `solver-<id>` is running within ~60s.

## 2. taifoon-intel LLM agent

Lower-priority queue. The LLM hand-registers a solver as part of a
larger autonomous flow.

1. `taifoon-intel` calls `POST /api/solvers/register` with an LLM-issued
   wallet and chain wiring.
2. The backend tags the row `source = "intel"` so the dashboard can
   surface non-human registrations separately.
3. Same webhook path as (1) from there on. The LLM never touches
   `kubectl` directly.

## 3. Direct API integration

For operators wiring their own infra.

1. Operator runs `taifoon-cli onboard` locally (Phase 1a).
2. CLI prints `SOLVER_PRIVATE_KEY` + `CHAIN_WIRING_JSON` env snippets and
   writes `~/.taifoon/solver.toml`.
3. Operator (or their CI) calls
   `POST ${TAIFOON_API_BASE}/api/solvers/register` directly with their
   wallet, then runs `k8s/register-webhook.sh` against their own
   cluster — or hands the solver off to the hosted sidecar by reusing
   the returned `solver_id`.

## Manifest contract

`k8s/solver-sidecar.yaml` is a template — `${SOLVER_ID}` is substituted
by the webhook before `kubectl apply`. Each pod consumes a Secret named
`solver-<solver_id>` carrying `SOLVER_PRIVATE_KEY` and
`CHAIN_WIRING_JSON`. The Deployment is labelled `solver_id=<id>` so
dashboards and `kubectl get -l solver_id=...` can target a single
operator.

## Notes

- The template assumes the `taifoon-solver:latest` image has been
  pushed to a registry the cluster can reach (or pre-loaded for local
  Kind/Minikube clusters).
- No cluster access is required to ship the manifests themselves — the
  webhook script is the integration seam between mamba workers and the
  cluster.
