#!/usr/bin/env bash
# Phase 6 acceptance gate — all-adapter test coverage.
# Verifies every adapter the solver can dispatch has passing tests.
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$REPO_ROOT"

echo "[phase6] cargo build --workspace --release"
cargo build --workspace --release

echo "[phase6] cargo test -p executor across_v3 --release"
cargo test -p executor across_v3 --release

echo "[phase6] cargo test -p executor debridge_dln --release"
cargo test -p executor debridge_dln --release

echo "[phase6] cargo test -p executor mayan_swift --release"
cargo test -p executor mayan_swift --release

echo "[phase6] cargo test -p executor mayan_solana --release"
cargo test -p executor mayan_solana --release

echo "[phase6] cargo test -p executor lifi_meta --release"
cargo test -p executor lifi_meta --release

echo "[phase6] cargo test -p executor --test lifi_projection --release"
cargo test -p executor --test lifi_projection --release

echo "[phase6] PASS"
