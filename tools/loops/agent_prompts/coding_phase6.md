# Coding Agent — Phase 6 (Demo dry-run rehearsal)

You are the rehearsal coordinator. By Phase 6 every prior phase has at least
one entry in `outcomes/verification_gates.jsonl` (real mainnet txs).

## Acceptance gate
`./tools/loops/gates/phase6.sh` exits 0 (verifies prior-phase ledger entries +
runs cargo build).

## What to do
1. **Pre-flight checklist.** Print the contents of `outcomes/verification_gates.jsonl`
   nicely formatted (jq), and produce a 5-bullet summary:
   - phases closed ✓
   - txs in the ledger
   - per-protocol fill counts (cross-reference against the outcome SQLite)
   - dashboard URL + screenshot path
   - kill switch reminder

2. **Demo script.** Write `tools/loops/demo_script.md` containing the 5-minute
   walkthrough from `FRONTIER_HACKATHON_PLAN.md` §6, with explicit:
   - timing per beat (e.g. 0:00-0:30 open dashboard, 0:30-1:00 start solver)
   - terminal commands ready to copy-paste
   - fallback if mainnet flakes (point at the recorded video,
     `demo/frontier_walkthrough.mp4`)

3. **Recording prep.** Verify `obs` (or `ffmpeg` for headless) is available;
   if not, document the install command. Don't actually record — that's MB's
   call when ready.

## What NOT to do
- Don't broadcast new mainnet txs at this phase. Phase 6 is rehearsal, not
  fresh broadcast.
- Don't modify any Rust code — if Phase 6 reveals a code defect, it's a
  regression and goes back to the relevant earlier phase loop.

## Report
- The pre-flight checklist
- The demo script path
- Any environment gaps (e.g. `obs` not installed)
