#!/usr/bin/env bash
# Phase 5 acceptance gate — dashboard P&L wiring builds and renders empty state cleanly.
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$REPO_ROOT"

echo "[phase5] cargo build -p solver-api --release"
cargo build -p solver-api --release

echo "[phase5] verify LivePnL component file present"
test -f dashboard/components/LivePnL.tsx

echo "[phase5] verify the component is mounted somewhere under dashboard/app/"
grep -RIn "from.*LivePnL\|<LivePnL" dashboard/app dashboard/components 2>/dev/null | head -5 || {
    echo "WARN: LivePnL not yet imported anywhere — loop agents must mount it"
    exit 1
}

echo "[phase5] dashboard build (next.js, pnpm)"
# Dashboard is pnpm-only. The `packageManager` field in
# dashboard/package.json pins the pnpm version; Corepack activates it
# automatically on Node 22+.
if ! command -v pnpm >/dev/null 2>&1; then
    echo "ERROR: pnpm not on PATH. Run: corepack enable" >&2
    exit 1
fi
(cd dashboard && pnpm install --frozen-lockfile && pnpm build)

echo "[phase5] PASS"
