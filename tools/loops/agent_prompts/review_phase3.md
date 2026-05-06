# Review Agent — Phase 3

```
VERDICT: PASS|FAIL
<diagnostic>
```

## PASS
- `phase3.sh` ends with `[phase3] PASS`
- Calldata-identity test exists and the bytes match for the test fixture
- OrderCreated round-trip test exists and round-trips
- Skip-reason histogram present in the coding agent's report

## FAIL
- Calldata bytes drift between estimate and broadcast (this is the bug the
  recent fixes targeted; if it returns it MUST block the mainnet gate)
- Round-trip test missing
- Any modification to `crates/protocol-adapters/src/debridge.rs` that doesn't
  have a corresponding test

Name the offending bytes (offset+length) when calldata drift is the cause.
