# Review Agent — Phase 1

Output format:

```
VERDICT: PASS|FAIL
<one paragraph diagnostic, naming files/tests/lines>
```

## PASS criteria
- `phase1.sh` ends with `[phase1] PASS`
- `protocol-adapters-solana` tests all pass; the new fixture-decoding tests are
  present and exercise BOTH `mayan_solana.json` and `mayan_solana_live.json`
  (or document why the latter is absent)
- The coding agent did NOT modify `SolanaBroadcaster::send_fulfill` or any of
  the broadcast logic in `lambda_controller.rs`'s Mayan-Solana branch
- A `tools/capture_intent.sh` exists OR a clear note explains why the live
  fixture is unchanged

## FAIL triggers
- Any test was disabled/skipped/`#[ignore]`d to make the gate pass
- The coding agent edited the broadcast path (Phase 1 is verification only —
  broadcast logic changes belong to a debug session under MB's direct review)
- Fixture data was hand-edited in a way that masks a real-world decode failure

## Diagnostic content
When FAIL, name the offending file and line. Examples:
- "FAIL: `crates/protocol-adapters-solana/src/mayan_solana.rs:412` — `from_intent` panics on the live fixture because field `swift_program_id` is `null`. Expected fallback to `DEFAULT_MAYAN_SWIFT_PROGRAM`."
- "FAIL: `mayan_solana_integration.rs::test_simulate_classifies_insufficient_lamports_as_green` — classifies as Yellow instead of Green; check `simulate.rs::classify_solana_simulate_result` for the lamports error string."
