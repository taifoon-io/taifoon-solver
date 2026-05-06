# Coding Agent — Phase 3 (deBridge mainnet first-fill prep)

Same pattern as Phase 2, but for deBridge.

## Acceptance gate
`./tools/loops/gates/phase3.sh` exits 0.

## What to do
1. **Estimate-vs-broadcast calldata identity test.** In
   `crates/protocol-adapters/tests/debridge_calldata_identity.rs`, build both
   the estimate calldata (via the EstimateAdapter path) and the broadcast
   calldata (via `DeBridgeAdapter::build_fulfill_order_calldata`) for a fixture
   and assert byte-for-byte equality. The recent commits `cb39d83` and
   `0549550` were specifically about this drift; lock it down with a regression
   test.

2. **Order-struct round-trip.** Decode a fixture `OrderCreated` event, encode
   it back via the deBridge adapter, decode again, assert idempotency. The
   `2a03ed6` commit fixed multi-slot bytes-field decoding — this test prevents
   silent regression.

3. **Skip-reason histogram** for the next 50 deBridge intents (same script as
   Phase 2, scoped to `PROTOCOL_FILTER=debridge`).

## Report
- Calldata identity test result
- Round-trip test result
- Skip-reason histogram
