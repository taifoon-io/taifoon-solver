#!/usr/bin/env bash
# Phase 0 acceptance gate — bench: build green, tests green, loops armed.
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$REPO_ROOT"

echo "[phase0] cargo build --workspace --release"
cargo build --workspace --release

echo "[phase0] cargo test --workspace"
cargo test --workspace

echo "[phase0] verify loop scaffolding"
[[ -d tools/loops/agent_prompts ]]
[[ -d tools/loops/gates ]]
[[ -x tools/loops/loop_driver.sh ]]
[[ -f outcomes/verification_gates.jsonl ]] || : > outcomes/verification_gates.jsonl

echo "[phase0] PASS"
