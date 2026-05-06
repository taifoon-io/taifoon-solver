#!/usr/bin/env bash
# tools/intent_replay.sh — Phase 2/3 skip-reason histogram
#
# Runs the solver against the live genome SSE stream in DRY_RUN mode for a
# fixed window, captures every "⏭️  <intent_id> — <reason>" line emitted by
# the lambda controller and adapter pipelines, and prints a sorted histogram
# of skip reasons.
#
# This is the canonical command the Phase 2/3 coding-agent prompt expects you
# to run before flipping `DRY_RUN=false` for a real first-fill. If any single
# reason fires on >50% of intents (especially `across_no_deposit_id`,
# `across_solana_src_unsupported`, `across_message_hook_unsupported`, or
# `across_fill_deadline_expired`), genome enrichment is likely broken —
# escalate before broadcasting.
#
# Usage:
#   PROTOCOL_FILTER=across ./tools/intent_replay.sh                # 5-minute window
#   PROTOCOL_FILTER=across REPLAY_WINDOW_SECS=300 ./tools/intent_replay.sh
#   PROTOCOL_FILTER=debridge REPLAY_WINDOW_SECS=600 ./tools/intent_replay.sh
#
# Exit codes:
#   0  — capture completed and at least one intent observed
#   1  — solver binary missing or required env not set
#   2  — capture window expired with zero intents seen (genome stream blocked
#        or PROTOCOL_FILTER too narrow)
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

PROTOCOL_FILTER="${PROTOCOL_FILTER:-across}"
REPLAY_WINDOW_SECS="${REPLAY_WINDOW_SECS:-300}"
GENOME_SSE_URL="${GENOME_SSE_URL:-https://api.taifoon.dev/api/genome/subscribe/sse}"
SPINNER_API_URL="${SPINNER_API_URL:-https://api.taifoon.dev}"

SOLVER_BIN="$REPO_ROOT/target/release/taifoon-solver"
if [[ ! -x "$SOLVER_BIN" ]]; then
    echo "ERROR: $SOLVER_BIN not found. Run: cargo build --workspace --release" >&2
    exit 1
fi

# Outcome / wallet DBs are throwaway for replay — write under a tmp dir so
# we never collide with a real run-mainnet.sh artifact.
TMPDIR_REPLAY="$(mktemp -d -t intent_replay.XXXXXX)"
trap 'rm -rf "$TMPDIR_REPLAY"' EXIT
LOG_FILE="$TMPDIR_REPLAY/solver.log"
OUTCOME_DB="$TMPDIR_REPLAY/outcomes.sqlite"
WALLET_DB="$TMPDIR_REPLAY/wallet.sqlite"

# We run the solver in DRY_RUN mode so no funds are at risk. SOLVER_PRIVATE_KEY
# is still required (the binary refuses to start without it) — derive a throw-away
# from the keychain when present, otherwise let the binary fall back to its
# own env-resolution path.
if [[ -z "${SOLVER_PRIVATE_KEY:-}" ]]; then
    SOLVER_PRIVATE_KEY=$(security find-generic-password -s mamba-messiah-key -w 2>/dev/null || true)
fi

if [[ -z "${SOLVER_PRIVATE_KEY:-}" ]]; then
    echo "ERROR: SOLVER_PRIVATE_KEY not set and 'mamba-messiah-key' not in keychain." >&2
    echo "  Replay still safe (DRY_RUN=true), but the binary requires the env." >&2
    exit 1
fi

echo "==================== intent_replay.sh ===================="
echo " Protocol filter:    $PROTOCOL_FILTER"
echo " Capture window:     ${REPLAY_WINDOW_SECS}s"
echo " Genome SSE:         $GENOME_SSE_URL"
echo " Log file:           $LOG_FILE"
echo "=========================================================="

# Launch solver in DRY_RUN mode, capture stdout+stderr to log file.
# `timeout` sends SIGTERM after the window — the solver's tokio runtime
# handles SIGTERM cleanly and exits 0 (or 124 from `timeout` itself, which
# we ignore via `|| true`).
env \
    SOLVER_PRIVATE_KEY="$SOLVER_PRIVATE_KEY" \
    DRY_RUN=true \
    SIMULATION_MODE=true \
    MAX_NOTIONAL_USD="${MAX_NOTIONAL_USD:-200}" \
    MIN_PROFIT_USD="${MIN_PROFIT_USD:-0.50}" \
    PROTOCOL_FILTER="$PROTOCOL_FILTER" \
    GENOME_SSE_URL="$GENOME_SSE_URL" \
    SPINNER_API_URL="$SPINNER_API_URL" \
    CHAIN_WIRING_FILE="$REPO_ROOT/config/chain_wiring.json" \
    OUTCOME_DB_PATH="$OUTCOME_DB" \
    WALLET_DB_PATH="$WALLET_DB" \
    RUST_LOG="${RUST_LOG:-taifoon_solver=info,executor=info,genome_client=info,warn}" \
    timeout --foreground "$REPLAY_WINDOW_SECS" "$SOLVER_BIN" \
    > "$LOG_FILE" 2>&1 || true

# Tally observed intents (lines that mention an intent_id passing through the
# pipeline) vs skipped intents (the "⏭️" emoji prefix that lambda_controller +
# main loop both emit). The reason is everything after " — " on the skip line.
intents_observed=$(grep -cE '(detected intent|new intent|🎯|attempt)' "$LOG_FILE" || true)
skipped_lines=$(grep -F '⏭️' "$LOG_FILE" || true)

if [[ -z "$skipped_lines" && "$intents_observed" -eq 0 ]]; then
    echo "WARN: capture window expired with zero intents observed." >&2
    echo "      Genome SSE host may be unreachable from this network, or" >&2
    echo "      PROTOCOL_FILTER='$PROTOCOL_FILTER' has no live deposits." >&2
    tail -50 "$LOG_FILE" >&2
    exit 2
fi

# Aggregate: extract the reason after " — ", normalize numeric tails so
# `across_fill_deadline_expired:dl=1234<now=5678` and similar variants
# collapse into a single bucket.
echo
echo "==================== Skip-reason histogram ===================="
echo "$skipped_lines" \
    | sed -E 's/.*⏭️ +//' \
    | sed -E 's/^[^—]+— *//' \
    | sed -E 's/:[0-9]+(<[a-z]+=[0-9]+)?//g' \
    | sed -E 's/:[0-9]+\.[0-9]+pct.*$/:<pct>pct/' \
    | sed -E 's/:depositor=.*$/:depositor=<addr>/' \
    | sort \
    | uniq -c \
    | sort -rn

total_skips=$(printf '%s\n' "$skipped_lines" | grep -c '⏭️' || true)
echo
echo "Totals: $intents_observed observed / $total_skips skipped"
echo "Log file preserved at: $LOG_FILE  (note: tmpdir; copy out before script exits)"
echo "==============================================================="

# Surface the file outside the tmpdir trap so caller can inspect/preserve.
cp "$LOG_FILE" "$REPO_ROOT/tools/last_intent_replay.log"
echo "Log copied to: tools/last_intent_replay.log"
