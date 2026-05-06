#!/usr/bin/env bash
# Phase 3 acceptance gate — deBridge fulfillOrder ready.
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$REPO_ROOT"

echo "[phase3] cargo test -p executor debridge --release"
cargo test -p executor debridge --release

echo "[phase3] cargo test -p protocol-adapters debridge --release"
cargo test -p protocol-adapters debridge --release

echo "[phase3] PASS"
