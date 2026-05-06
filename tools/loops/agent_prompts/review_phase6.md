# Review Agent — Phase 6

```
VERDICT: PASS|FAIL
<diagnostic>
```

## PASS
- `phase6.sh` ends with `[phase6] PASS` (verifies all prior phases have ledger entries)
- `tools/loops/demo_script.md` exists and includes:
  - explicit timing
  - copy-pastable terminal commands
  - mainnet fallback story
- The pre-flight checklist names every closed phase by tx hash + explorer link

## FAIL
- Any prior phase missing from the ledger
- Demo script lacks timing or copy-pastable commands
- New Rust code edits in this phase (Phase 6 is rehearsal-only — code changes
  must be triaged back to their owning phase)

When PASS, end the diagnostic with: "Demo ready. MB to record final walkthrough
and run `gh release create frontier-v0.1` with FRONTIER_HACKATHON_PLAN.md as
release notes."
