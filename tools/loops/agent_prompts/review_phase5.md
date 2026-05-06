# Review Agent — Phase 5

```
VERDICT: PASS|FAIL
<diagnostic>
```

## PASS
- `phase5.sh` ends with `[phase5] PASS`
- Both endpoints respond on `localhost:8082` (or whatever `API_PORT` is set to)
- LivePnL is imported in exactly ONE place under `dashboard/app/` (multiple
  mounts would cause duplicate fetch storms — flag if so)
- Dashboard `pnpm build` produces no TypeScript errors

## FAIL
- LivePnL added to `dashboard/components/` but never mounted
- `solver-api` build fails (typically due to executor crate dependency cycle —
  check that `executor` is in `solver-api/Cargo.toml` deps; do NOT add the
  reverse edge)
- Polling interval was changed from 3s without justification (the spec says 3s)

Name the file/line in the diagnostic.
