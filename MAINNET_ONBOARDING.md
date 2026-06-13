# Mainnet onboarding — taifoon-solver × taifoon-arc × portal-2026

This is the live-fill flip. Read every section before you set `LIVE_FILL_OK=yes`.

## 0. Prereqs (must hold before you start)

- [ ] `taifoon-arc/contracts/donut/DonutHook.sol` deployed and verified on
      Ethereum, Optimism, Arbitrum, Base, Polygon. Addresses landed in
      `contracts/donut/DEPLOYMENTS.md`.
- [ ] `setAdapterContributor()` called once per slug on every chain
      (across, debridge, mayan_swift, lifi, relay).
- [ ] `ARC_OPERATOR_PRIVATE_KEY` provisioned in a hot wallet with enough
      gas on each chain to sign `recordFill()` per fill (≈80k gas × 5 chains).
- [ ] `SOLVER_PRIVATE_KEY` funded with at least $50 native per chain.
- [ ] `SOLANA_KEYPAIR_PATH` exists and is funded (Mayan A4/A5 fills).
- [ ] Dedicated RPCs (Alchemy / Infura / Quicknode) — never public endpoints
      for live fills. Public RPC rate-limits will leave fills mid-flight.

## 1. Boot the stack

```bash
# Terminal A — solver + sidecar + dashboard
cd taifoon-solver
docker compose -f docker-compose.yml -f docker-compose.mainnet.yml up

# Terminal B — arc-api
cd taifoon-arc
SIDECAR_URL=http://localhost:8090 \
  ARC_NETWORK=mainnet \
  ARC_OPERATOR_PRIVATE_KEY=$ARC_OPERATOR_PRIVATE_KEY \
  cargo run -p arc-api --release

# Terminal C — portal-2026
cd t3rn/portal-2026
NEXT_PUBLIC_ARC_API_URL=http://localhost:7088 pnpm dev
```

Confirm:

```bash
curl -s localhost:7088/v1/mode | jq .
# expect: { simulation: false, live_ok: false, network: "mainnet", sidecar_reachable: true }
```

`live_ok` is still `false` because `LIVE_FILL_OK` is unset. This is the
intended state — you can verify routing, donut math, and the wire without
spending real money.

## 2. The two-key flip

When you and at least one other person have inspected the smoke output:

```bash
export LIVE_FILL_OK=yes
docker compose -f docker-compose.yml -f docker-compose.mainnet.yml up -d sidecar
```

Re-check:

```bash
curl -s localhost:7088/v1/mode | jq '.simulation, .live_ok'
# expect: false, true
```

## 3. First live fill — Across Op→Base USDC, $5

`raw_data` must match the Across snake_case schema (see
`arc-protocols/src/across.rs` for the parser).

```bash
curl -s -X POST localhost:7088/v1/fill -H "content-type: application/json" -d '{
  "protocol_slug":"across","src_chain_id":10,"dst_chain_id":8453,
  "src_token":"0x0b2c639c533813f4aa9d7837caf62653d097ff85",
  "dst_token":"0x833589fcd6edb6e08f4c7c32d4f71b54bda02913",
  "input_amount":5000000,"output_amount":4998000,
  "recipient":"<YOUR_RECIPIENT>",
  "dry_run":false,
  "raw_data": { "...": "..." }
}' | jq .
```

Verify on-chain:

```bash
# DonutHook accrual on Base
cast call $DONUT_HOOK_BASE \
  "claimable(address,address)(uint256)" \
  0xac7055c00747b4117c0470a700000000000ac033 \
  0x833589fcd6edb6e08f4c7c32d4f71b54bda02913 \
  --rpc-url $BASE_RPC
# expect: non-zero (≈ value_routed_usd * 0.0049 * 0.7 in token units)
```

## 4. EVM↔Solana — Mayan A4 (Arb USDC → Solana USDC, $5)

Solana destinations use `dst_kind: "solana"` and a base58 recipient.

```bash
curl -s -X POST localhost:7088/v1/fill -H "content-type: application/json" -d '{
  "protocol_slug":"mayan_swift",
  "src_chain_id":42161,"dst_chain_id":0,"dst_kind":"solana",
  "src_token":"0xaf88d065e77c8cC2239327C5EDb3A432268e5831",
  "dst_token":"EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
  "input_amount":5000000,"output_amount":4995000,
  "recipient":"<YOUR_SOLANA_BASE58>",
  "dry_run":false,
  "raw_data": { "...": "..." }
}' | jq .
```

## 5. First donut claim

Once at least one fill has accrued:

```bash
cast send $DONUT_HOOK_BASE "claim(address)" $USDC_BASE \
  --from $CONTRIBUTOR_ADDR --private-key $CONTRIBUTOR_KEY \
  --rpc-url $BASE_RPC
# expect: USDC Transfer event from DonutHook → contributor
```

Take a screenshot of the Transfer event in basescan and add it to the
Phase 4 tracking issue ("Donut claim demo").

## 6. Killswitch

```bash
docker compose stop sidecar          # blocks new fills, in-flight fills continue
docker compose stop solver           # also stops the SSE loop
unset LIVE_FILL_OK                   # back to default no
```
