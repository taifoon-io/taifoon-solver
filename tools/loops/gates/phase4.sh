#!/usr/bin/env bash
# Phase 4 acceptance gate — LiFi meta-router resolves to underlying.
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$REPO_ROOT"

echo "[phase4] cargo test -p executor lifi_meta_router --release"
cargo test -p executor lifi_meta_router --release

echo "[phase4] cargo test -p executor --test lifi_projection --release"
cargo test -p executor --test lifi_projection --release

echo "[phase4] cargo test -p solver-main --lib lifi_resolver --release"
cargo test -p solver-main --lib lifi_resolver --release

echo "[phase4] PASS"
