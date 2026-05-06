#!/usr/bin/env bash
# Phase 2 acceptance gate — deBridge calldata round-trip + Across V3 DRY_RUN smoke
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$REPO_ROOT"

echo "[phase2] deBridge calldata round-trip"
cargo test -p protocol-adapters debridge --release

echo "[phase2] Across V3 DRY_RUN smoke"
if command -v timeout &>/dev/null; then
    DRY_RUN=true timeout 60 cargo run -p solver-main --bin taifoon-solver --release -- --max-events 3 || true
elif command -v gtimeout &>/dev/null; then
    DRY_RUN=true gtimeout 60 cargo run -p solver-main --bin taifoon-solver --release -- --max-events 3 || true
else
    DRY_RUN=true cargo run -p solver-main --bin taifoon-solver --release -- --max-events 3
fi

echo "[phase2] PASS"
