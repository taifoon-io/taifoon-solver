#!/usr/bin/env bash
# Loopback driver — orchestrates coding-agent ↔ review-agent ↔ user-mainnet-gate
# for one phase of the Frontier Hackathon plan.
#
# Usage:
#   ./tools/loops/loop_driver.sh <phase>             # run phase
#   ./tools/loops/loop_driver.sh --self-test         # dry-run gate scripts only
#   ./tools/loops/loop_driver.sh --list              # list phases
#
# Env:
#   MAX_LOOP_ITERATIONS  default 6  (safety belt for runaway loops)
#   CLAUDE_BIN           default `claude` on PATH (the CLI that spawns agents)
#   AGENT_MODEL          default `claude-sonnet-4-6` for coding, `claude-opus-4-6` for review
#   DRY_RUN_LOOP         when true, prints what it would do but skips agent invocation
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
LOOPS_DIR="$REPO_ROOT/tools/loops"
GATE_LEDGER="$REPO_ROOT/outcomes/verification_gates.jsonl"
mkdir -p "$REPO_ROOT/outcomes"
[[ -f "$GATE_LEDGER" ]] || : > "$GATE_LEDGER"

MAX_LOOP_ITERATIONS="${MAX_LOOP_ITERATIONS:-6}"
CLAUDE_BIN="${CLAUDE_BIN:-claude}"
DRY_RUN_LOOP="${DRY_RUN_LOOP:-false}"

# --- Phase metadata (kept in sync with FRONTIER_HACKATHON_PLAN.md §4) ---
declare -A PHASE_TITLE=(
    [0]="Bench: build green, tests green, loops armed"
    [1]="Solana broadcast verified live (Mayan-Solana)"
    [2]="Across mainnet first-fill"
    [3]="deBridge mainnet first-fill"
    [4]="LiFi-via-X mainnet first-fill"
    [5]="Dashboard P&L wiring"
    [6]="Demo dry-run rehearsal"
)

usage() {
    echo "Usage: $0 <phase|--self-test|--list>"
    echo "Phases:"
    for p in 0 1 2 3 4 5 6; do
        printf "  %d  %s\n" "$p" "${PHASE_TITLE[$p]}"
    done
}

if [[ $# -eq 0 ]]; then usage; exit 1; fi

case "$1" in
    --list) usage; exit 0 ;;
    --self-test)
        echo "==> Running all phase acceptance gates (no agents, no keys)…"
        for p in 0 1 2 3 4 5 6; do
            gate="$LOOPS_DIR/gates/phase${p}.sh"
            if [[ -x "$gate" ]]; then
                echo "  -- phase $p: $gate"
                "$gate" || echo "     (returned non-zero — expected for unfinished phases)"
            else
                echo "  -- phase $p: gate script missing"
            fi
        done
        exit 0
        ;;
esac

PHASE="$1"
if [[ -z "${PHASE_TITLE[$PHASE]:-}" ]]; then
    echo "ERROR: unknown phase '$PHASE'. Run with --list to see options." >&2
    exit 1
fi

CODING_PROMPT="$LOOPS_DIR/agent_prompts/coding_phase${PHASE}.md"
REVIEW_PROMPT="$LOOPS_DIR/agent_prompts/review_phase${PHASE}.md"
ACCEPTANCE_GATE="$LOOPS_DIR/gates/phase${PHASE}.sh"
MAINNET_GATE="$LOOPS_DIR/gates/mainnet_phase${PHASE}.sh"   # may not exist for non-mainnet phases

for f in "$CODING_PROMPT" "$REVIEW_PROMPT" "$ACCEPTANCE_GATE"; do
    [[ -f "$f" ]] || { echo "ERROR: missing $f" >&2; exit 1; }
done
chmod +x "$ACCEPTANCE_GATE" 2>/dev/null || true
[[ -f "$MAINNET_GATE" ]] && chmod +x "$MAINNET_GATE" 2>/dev/null || true

echo "==================================================================="
echo " Phase $PHASE — ${PHASE_TITLE[$PHASE]}"
echo " Max iterations: $MAX_LOOP_ITERATIONS"
echo " Acceptance gate: $ACCEPTANCE_GATE"
[[ -f "$MAINNET_GATE" ]] && echo " Mainnet gate:    $MAINNET_GATE" || echo " Mainnet gate:    (none — local-only phase)"
echo "==================================================================="

iteration=0
last_diag=""

while (( iteration < MAX_LOOP_ITERATIONS )); do
    iteration=$((iteration + 1))
    echo
    echo "----------------- Iteration $iteration of $MAX_LOOP_ITERATIONS -----------------"

    # 1. Coding agent turn
    echo "==> Spawning coding agent…"
    if [[ "$DRY_RUN_LOOP" != "true" ]]; then
        if command -v "$CLAUDE_BIN" >/dev/null 2>&1; then
            # Pipe coding prompt + previous review diagnostic (if any) into claude.
            {
                cat "$CODING_PROMPT"
                if [[ -n "$last_diag" ]]; then
                    echo
                    echo "## Previous review verdict (FAIL)"
                    echo "$last_diag"
                fi
            } | "$CLAUDE_BIN" --print --model "${AGENT_MODEL:-claude-sonnet-4-6}" \
                --append-system-prompt "You are the coding subagent for phase $PHASE. Apply edits, then exit." \
                || echo "WARN: claude CLI returned non-zero; continuing to gate"
        else
            echo "WARN: '$CLAUDE_BIN' not on PATH — cannot spawn coding agent automatically."
            echo "      Open your editor / Claude Code session, paste the prompt below, do the edits, then press Enter:"
            echo
            cat "$CODING_PROMPT"
            read -r -p "Press Enter when the coding turn is complete… "
        fi
    else
        echo "    (DRY_RUN_LOOP=true; skipping agent invocation)"
    fi

    # 2. Acceptance gate
    echo "==> Running acceptance gate: $ACCEPTANCE_GATE"
    gate_output=$("$ACCEPTANCE_GATE" 2>&1) || true
    gate_status=$?
    echo "$gate_output" | tail -40
    if (( gate_status == 0 )); then
        echo "==> Acceptance gate PASS"
    else
        echo "==> Acceptance gate FAIL (exit $gate_status)"
    fi

    # 3. Review agent turn
    echo "==> Spawning review agent…"
    review_verdict=""
    if [[ "$DRY_RUN_LOOP" != "true" ]] && command -v "$CLAUDE_BIN" >/dev/null 2>&1; then
        review_verdict=$({
            cat "$REVIEW_PROMPT"
            echo
            echo "## Acceptance gate output (exit $gate_status)"
            echo '```'
            echo "$gate_output"
            echo '```'
        } | "$CLAUDE_BIN" --print --model "${AGENT_MODEL_REVIEW:-claude-opus-4-6}" \
            --append-system-prompt "You are the review subagent for phase $PHASE. Respond ONLY with 'VERDICT: PASS' or 'VERDICT: FAIL', followed by a one-paragraph diagnostic." \
            2>&1 || true)
        echo "$review_verdict"
    else
        if (( gate_status == 0 )); then
            review_verdict="VERDICT: PASS (no review agent — gate alone)"
        else
            review_verdict="VERDICT: FAIL (no review agent — gate alone)"
        fi
        echo "$review_verdict"
    fi

    # 4. Decide next step
    if echo "$review_verdict" | grep -qi "VERDICT: PASS"; then
        echo
        echo "==================================================================="
        echo " ✅ Phase $PHASE acceptance + review PASS"
        echo "==================================================================="
        if [[ -f "$MAINNET_GATE" ]]; then
            echo
            echo "Next: mainnet verification gate. Run when you (MB) are ready:"
            echo "  $MAINNET_GATE"
            echo
            echo "On success, the mainnet gate appends one line to:"
            echo "  $GATE_LEDGER"
            echo "Phase $PHASE is closed when that line lands."
        else
            echo "No mainnet gate for phase $PHASE — closing locally."
            tmpfile=$(mktemp)
            printf '{"phase":%d,"closed_at":"%s","local_only":true}\n' \
                "$PHASE" "$(date -u +%FT%TZ)" >> "$GATE_LEDGER"
            rm -f "$tmpfile"
        fi
        exit 0
    fi

    last_diag="$review_verdict"
    echo "==> Loop continues — feeding diagnostic back to coding agent."
done

echo
echo "==================================================================="
echo " ❌ Phase $PHASE exhausted $MAX_LOOP_ITERATIONS iterations without PASS"
echo "==================================================================="
echo "Last diagnostic:"
echo "$last_diag"
echo
echo "Manual triage required. Suggested next steps:"
echo "  - Read the latest acceptance gate output above"
echo "  - Edit the relevant file by hand"
echo "  - Re-run: $0 $PHASE"
exit 2
