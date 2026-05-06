# Coding Agent — Phase 1 (Solana broadcast verified)

You are the coding subagent for Phase 1. Your job is to take the Solana broadcast
path from "compiles" to "verified at the unit-test level," ready for MB to run
the mainnet gate (`tools/loops/gates/mainnet_phase1.sh`).

## Acceptance gate
`./tools/loops/gates/phase1.sh` must exit 0:
- `cargo test -p protocol-adapters-solana --release`
- `cargo test -p executor mayan_solana --release`
- Solana intent fixtures exist under `tests/fixtures/`

## What to do (lift)
1. Capture a fresh Mayan-Solana intent fixture from the live genome stream if
   possible. The recommended path: write a small `tools/capture_intent.sh` that
   subscribes to the SSE stream for 60 seconds, filters for `protocol=mayan_swift`
   AND `is_solana_source=true`, and writes the first matching event to
   `tests/fixtures/mayan_solana_live.json`. Don't invent fixture content —
   if no live intent appears, leave the existing fixture in place and note it.

2. Add **table-driven tests** to `crates/protocol-adapters-solana/src/mayan_solana.rs`:
   - `test_decode_live_fixture` — `MayanSolanaIntent::from_intent` returns Ok on
     `tests/fixtures/mayan_solana.json` AND `tests/fixtures/mayan_solana_live.json`
     (skip the latter if it doesn't exist).
   - `test_compute_units_estimate_in_range` — for the fixtures, the estimate is
     between 200_000 and 1_400_000 (sane range for Anchor programs).
   - `test_priority_fee_set` — the resulting transaction has a non-zero
     priority-fee compute-budget instruction.

3. Add an integration test under `crates/executor/tests/mayan_solana_integration.rs`:
   - Build a `MayanSolanaIntent` from the fixture
   - Call `SolanaSimulator::classify_solana_simulate_result` on a synthetic
     "insufficient lamports" RPC response and assert it classifies as
     `EstimateOutcome::Green` (the fund check is the LAST step before broadcast,
     so reaching it = the calldata + program ABI matched).

4. **Validation only** — do NOT modify `SolanaBroadcaster::send_fulfill`. If a
   test reveals a defect there, escalate by writing a clear FAIL diagnostic
   for the review agent and stop.

## What to leave for the mainnet gate
- Actual broadcast against Helius mainnet (requires SOLANA_PRIVATE_KEY in keychain)
- Real signature recorded in outcome log
- Solscan link

## Report when done
- New/changed files
- Test names + pass/fail
- The phase1.sh gate output
- Any anomalies in fixture decoding (e.g. fields that come from genome under
  unexpected names) — these are red flags worth surfacing to MB before the
  mainnet gate
