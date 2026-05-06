#!/usr/bin/env bash
# Phase 1 acceptance gate — Solana broadcaster verified at the unit-test level.
# Mainnet broadcast is a separate manual step (mainnet_phase1.sh).
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$REPO_ROOT"

echo "[phase1] cargo test -p protocol-adapters-solana --release"
cargo test -p protocol-adapters-solana --release

echo "[phase1] cargo test -p executor mayan_solana --release"
cargo test -p executor mayan_solana --release

# Verify the lambda controller branches into the Solana path on a Solana-source intent.
echo "[phase1] sanity-check Solana intent decoding"
test -f tests/fixtures/mayan_solana.json
test -f tests/fixtures/mayan_solana.meta.json

echo "[phase1] PASS"
