# Coding Agent — Phase 5 (Dashboard P&L wiring)

The heavy lifting is already done in this repo as of 2026-05-05:
- `OutcomeLog::recent()` and `OutcomeLog::pnl_summary()` are implemented in
  `crates/executor/src/outcome_log.rs`
- `solver-api` exposes `/api/solver/outcomes` and `/api/solver/pnl` with a
  2-second cache on the P&L summary
- `dashboard/components/LivePnL.tsx` is written, polls every 3s, renders
  a stacked-bar breakdown + recent fills with explorer URLs
- `solver-main` injects the OutcomeLog into SolverApi via `set_outcome_log`

Your job is the last mile: **mount the component on the operator dashboard
page and verify the build+render.**

## Acceptance gate
`./tools/loops/gates/phase5.sh` exits 0:
- `cargo build -p solver-api --release`
- `dashboard/components/LivePnL.tsx` exists
- `LivePnL` is imported and rendered somewhere under `dashboard/app/`
- `cd dashboard && pnpm build` succeeds

## What to do
1. **Pick the right page.** The most likely target is
   `dashboard/app/portal/[solverId]/page.tsx` (the operator portal). If that
   doesn't exist yet, mount on `dashboard/app/page.tsx` above the existing
   `<DashboardPreview/>` section.

2. **Import + render.** One line at the top:
   ```tsx
   import LivePnL from '@/components/LivePnL'
   ```
   And one block in the JSX, with appropriate spacing:
   ```tsx
   <section className="container mx-auto my-8">
     <LivePnL />
   </section>
   ```

3. **Empty-state sanity.** Run the dashboard against a fresh solver
   (no fills in the DB) and confirm the panel renders "$0.00 / 0 fills"
   without errors. The component already handles this case but the manual
   smoke is worth it.

4. **Hash-link sanity.** Manually populate one row in the outcome SQLite via
   `sqlite3 outcomes/test.sqlite "INSERT INTO solver_outcomes VALUES (...)"`
   with a real Solscan tx hash and confirm the link opens the right page.

## What NOT to do
- Do not touch `LivePnL.tsx` itself unless the build fails — it's tested.
- Do not change the polling interval or add WebSocket complexity — the 3s
  poll is intentional, simpler than realtime, and dashboards typically only
  show this for seconds at a time during the demo.

## Report
- Which page got the mount
- Screenshot or describe the empty state and the populated state
- Any console errors in the browser
