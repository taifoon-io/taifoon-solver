#!/usr/bin/env bash
# P6 boot smoke test — solver-main wires HandBackend.
#
# Green-bar invariant (ROADMAP Phase 6): `cargo run -p solver-main` boots and
# `curl http://localhost:8082/api/hand/status` returns a non-503 JSON response
# with `.registered > 0`.
#
# This launches the `taifoon-solver` binary against the fixture hands.toml that
# registers a single "internal" venue, then asserts /api/hand/status and
# /api/hand/venues answer non-503 with at least one registered venue.
#
# Usage:
#   crates/solver-main/tests/hand_smoke.sh
# Requires: a built `taifoon-solver` binary (cargo build -p solver-main), curl, jq.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../../.." && pwd)"
FIXTURE="$HERE/fixtures/hands.toml"
PORT="${HAND_SMOKE_PORT:-8082}"

# Locate the binary (debug or release).
BIN=""
for cand in \
  "$REPO/target/debug/taifoon-solver" \
  "$REPO/target/release/taifoon-solver"; do
  if [[ -x "$cand" ]]; then BIN="$cand"; break; fi
done
if [[ -z "$BIN" ]]; then
  echo "SKIP: taifoon-solver binary not built (run: cargo build -p solver-main)" >&2
  exit 0
fi
for tool in curl jq; do
  command -v "$tool" >/dev/null 2>&1 || { echo "SKIP: $tool not installed" >&2; exit 0; }
done

WORKDIR="$(mktemp -d)"
cleanup() {
  [[ -n "${PID:-}" ]] && kill "$PID" 2>/dev/null || true
  rm -rf "$WORKDIR"
}
trap cleanup EXIT

# Boot the solver against the fixture. Keep it self-contained: dry-run, no
# external dependencies required for the hand routes to come up. SQLite paths
# point at the throwaway workdir.
HANDS_CONFIG_PATH="$FIXTURE" \
API_PORT="$PORT" \
DRY_RUN="true" \
SOLVER_API_TOKEN="smoke-test-token" \
OUTCOME_DB_PATH="$WORKDIR/outcomes.sqlite" \
WALLET_DB_PATH="$WORKDIR/wallet.sqlite" \
GENOME_SSE_URL="http://127.0.0.1:1/never" \
  "$BIN" > "$WORKDIR/solver.log" 2>&1 &
PID=$!

# Wait for the hand status route to come up (up to ~15s).
STATUS_JSON="$WORKDIR/status.json"
ok=0
for _ in $(seq 1 30); do
  if curl -fsS "http://localhost:$PORT/api/hand/status" -o "$STATUS_JSON" 2>/dev/null; then
    ok=1; break
  fi
  # Bail early if the process died.
  if ! kill -0 "$PID" 2>/dev/null; then
    echo "FAIL: solver process exited during boot" >&2
    tail -n 40 "$WORKDIR/solver.log" >&2 || true
    exit 1
  fi
  sleep 0.5
done

if [[ "$ok" != "1" ]]; then
  echo "FAIL: /api/hand/status never became reachable (likely still 503 or bind failure)" >&2
  tail -n 40 "$WORKDIR/solver.log" >&2 || true
  exit 1
fi

# Also fetch venues.
curl -fsS "http://localhost:$PORT/api/hand/venues" -o "$WORKDIR/venues.json"

echo "--- /api/hand/status ---"
cat "$STATUS_JSON"; echo

# Assert: registered > 0 (the non-503 green bar) and the internal venue is listed.
jq -e '.registered > 0' "$STATUS_JSON" >/dev/null \
  || { echo "FAIL: .registered not > 0" >&2; exit 1; }
jq -e 'any(.[]; .venue == "internal")' "$WORKDIR/venues.json" >/dev/null \
  || { echo "FAIL: 'internal' venue not in /api/hand/venues" >&2; exit 1; }

echo "PASS: /api/hand/status returned non-503 with registered > 0 and 'internal' venue present"
