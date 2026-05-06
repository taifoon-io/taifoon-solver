# Coding Agent — Phase 4 (LiFi-via-X mainnet first-fill prep)

LiFi is a meta-aggregator: every LiFi event names its underlying bridge in the
`bridge` (or fallback `tool`) field. The fill goes against the underlying
bridge's destination contract, not the LiFi Diamond.

## Acceptance gate
`./tools/loops/gates/phase4.sh` exits 0.

## What to do
1. **Resolution-rate tracker.** Add a debug log line in
   `crates/executor/src/lifi_meta_router.rs::estimate` that records the
   resolved underlying bridge for every LiFi intent. Then run the intent
   replay against `PROTOCOL_FILTER=lifi` and report:
   - resolution rate (intents with a usable underlying bridge / total)
   - distribution of underlying bridges (across/debridge/mayan/other)
   - any `RouteNotImplemented(<other>)` cases — these are soft skips, not bugs

2. **Child-intent projection test.** In `crates/executor/tests/lifi_projection.rs`,
   for each fixture `tests/fixtures/lifi*.json`:
   - Resolve the underlying bridge via `LiFiMetaRouter::resolve_bridge`
   - Project to a child intent via `LiFiMetaRouter::project_to_child`
   - Assert `child.protocol` matches the expected canonical name
     (`across_v3`, `debridge`, `mayan_swift`)
   - Assert chain/token/amount/depositor/recipient are inherited unchanged

3. **li.quest fallback path test.** When the LiFi event lacks a `tool` field,
   confirm the `resolve_lifi_bridge` path in `solver-main/src/main.rs` is
   exercised (mocked HTTP) and returns `LifiBridgeResult::Resolved` for a
   canned response.

## Report
- Resolution-rate breakdown for the replay window
- Test outcomes
- Any LiFi events seen with `bridge` values not in {across, debridge, mayan} —
  list them; they're future work but worth tracking
