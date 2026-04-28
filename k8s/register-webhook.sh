#!/usr/bin/env bash
# register-webhook.sh — register a solver and deploy its sidecar pod.
#
# Called by:
#   1. taifoon.dev web UI (via taifoon-mamba backend webhook)
#   2. taifoon-intel LLM agent
#   3. Direct API consumers (curl)
#
# Flow:
#   POST /api/solvers/register   -> mints solver_id, persists wallet
#   kubectl create secret        -> SOLVER_PRIVATE_KEY + CHAIN_WIRING_JSON
#   kubectl apply (envsubst)     -> per-solver Deployment in namespace `solvers`
#
# Required env:
#   TAIFOON_API_BASE      e.g. https://api.taifoon.dev
#   SOLVER_PRIVATE_KEY    hex-encoded private key for the solver wallet
#   CHAIN_WIRING_JSON     chain wiring config blob (single line)
#   SOLVER_WALLET_ADDR    wallet pubkey/address derived from SOLVER_PRIVATE_KEY
#
# Optional env:
#   SOLVER_ID             skip /api/solvers/register and use this id directly
#   CHAINS                comma list (default: base-sepolia,solana-devnet)
#   MANIFEST              path to template (default: k8s/solver-sidecar.yaml)
set -euo pipefail

API_BASE="${TAIFOON_API_BASE:-https://api.taifoon.dev}"
CHAINS="${CHAINS:-base-sepolia,solana-devnet}"
MANIFEST="${MANIFEST:-$(dirname "$0")/solver-sidecar.yaml}"

: "${SOLVER_PRIVATE_KEY:?SOLVER_PRIVATE_KEY env var required}"
: "${CHAIN_WIRING_JSON:?CHAIN_WIRING_JSON env var required}"
: "${SOLVER_WALLET_ADDR:?SOLVER_WALLET_ADDR env var required}"

# 1. Register with the Taifoon backend (skip if SOLVER_ID already set).
if [[ -z "${SOLVER_ID:-}" ]]; then
  echo "[register-webhook] POST ${API_BASE}/api/solvers/register"
  resp=$(curl -fsS -X POST "${API_BASE}/api/solvers/register" \
    -H 'Content-Type: application/json' \
    -d "$(printf '{"wallet":"%s","chains":"%s"}' "${SOLVER_WALLET_ADDR}" "${CHAINS}")")
  SOLVER_ID=$(printf '%s' "${resp}" | sed -n 's/.*"solver_id"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
  if [[ -z "${SOLVER_ID}" ]]; then
    echo "[register-webhook] register failed; response: ${resp}" >&2
    exit 1
  fi
fi
echo "[register-webhook] solver_id=${SOLVER_ID}"
export SOLVER_ID

# 2. Ensure namespace exists, then create/replace Secret.
kubectl get ns solvers >/dev/null 2>&1 || kubectl create ns solvers
kubectl -n solvers create secret generic "solver-${SOLVER_ID}" \
  --from-literal=SOLVER_PRIVATE_KEY="${SOLVER_PRIVATE_KEY}" \
  --from-literal=CHAIN_WIRING_JSON="${CHAIN_WIRING_JSON}" \
  --dry-run=client -o yaml | kubectl apply -f -

# 3. Render the manifest template and apply.
envsubst '${SOLVER_ID}' < "${MANIFEST}" | kubectl apply -f -

# 4. Wait briefly for the deployment to become available (best-effort).
kubectl -n solvers rollout status "deployment/solver-${SOLVER_ID}" --timeout=60s || true

echo "[register-webhook] solver-${SOLVER_ID} deployed"
