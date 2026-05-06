# Coding Agent — Phase 2 (Across mainnet first-fill prep)

You are the coding subagent for Phase 2. Your job is to drive intent-validation
coverage on the Across path so MB can run `mainnet_phase2.sh` with confidence.

## Acceptance gate
`./tools/loops/gates/phase2.sh` exits 0.

## What to do
1. **Skip-reason histogram.** Write `tools/intent_replay.sh` that:
   - Captures the next 50-100 Across intents from the genome SSE stream
   - Runs each through `lambda_execute` in DRY_RUN mode (`PROTOCOL_FILTER=across`)
   - Tallies the skip reasons and prints a histogram
   Then run it once and paste the output into the report. If any single reason
   fires on >50% of intents (especially `across_no_deposit_id`,
   `across_solana_src_unsupported`, `across_message_hook_unsupported`, or
   `across_fill_deadline_expired`), escalate — that's likely a regression in
   genome enrichment, not normal traffic.

2. **Calldata round-trip test.** In `crates/executor/tests/across_calldata.rs`:
   - For each fixture in `tests/fixtures/across*.json`, build the
     `fillV3Relay` calldata via `build_across_spoke_pool_calldata_with_relayer`
     and assert (a) it starts with the right selector, (b) the decoded
     `outputAmount` ≤ the decoded `inputAmount`, (c) the destination chain
     matches `intent.dst_chain`.

3. **Spread guard sanity.** Add unit tests for the spread/deadline/message-hook
   guards in `lambda_controller.rs` exercising each skip path.

## What NOT to do
- Do not change broadcast logic. Phase 2 is preparation; broadcast happens at
  the mainnet gate.
- Do not modify `chain_wiring.json`.

## Report when done
- The skip-reason histogram (verbatim)
- New tests + outcomes
- Any genome-stream anomalies that block the mainnet gate
