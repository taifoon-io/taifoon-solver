# Review Agent — Phase 2

```
VERDICT: PASS|FAIL
<diagnostic>
```

## PASS
- `phase2.sh` ends with `[phase2] PASS`
- New `tools/intent_replay.sh` exists, runs, and the coding agent reported a histogram
- New `tests/across_calldata.rs` exists and passes
- No skip reason fires on >50% of replay intents (or it does, and the coding
  agent has documented WHY in the report — e.g. "all 50 intents had Solana
  source; this is the live stream's current shape, not a bug")

## FAIL
- Tests pass but the histogram shows a >50% skip rate without explanation
- Calldata round-trip tests are absent or weakly assert (e.g. just length check)
- Any change to lambda controller broadcast branches

Name the file/test/skip-reason in the diagnostic.
