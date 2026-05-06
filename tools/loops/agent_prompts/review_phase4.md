# Review Agent — Phase 4

```
VERDICT: PASS|FAIL
<diagnostic>
```

## PASS
- `phase4.sh` ends with `[phase4] PASS`
- Resolution rate is >70% for the replay window OR coding agent justifies
  why it's lower (e.g. live stream currently dominated by exotic bridges)
- Child-intent projection tests cover Across, deBridge, AND Mayan (all three
  underlying types we accept)
- The debug log line is wired into `lifi_meta_router::estimate` and emits at
  level INFO (not TRACE — judges will glance at the live log during demo)

## FAIL
- Resolution rate <70% with no explanation
- Projection test missing for any of the three underlying types
- Debug log line is at TRACE/DEBUG level (won't show in default RUST_LOG=info)

Diagnostic must name the LiFi tool/bridge values that failed to resolve.
