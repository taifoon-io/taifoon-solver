# Architecture

`taifoon-solver` is the execution engine of the Taifoon OS. It listens to a stream of cross-chain intents, calculates profit, and routes every fill through `TaifoonUniversalOperator` — which wraps the call in a V5 MMR proof and dispatches to a protocol-specific adapter.

The novel part is **the dispatch layer**. Most solvers hard-code their integrations. Taifoon's adapters are submitted by the public via the BuildersRegistry under TSUL, replayed against fixtures by reviewer agents, and merged automatically. Donut routes the resulting revenue back to whoever shipped the adapter.

---

## Components

### 1. Intent stream

```
┌────────────────────┐  SSE / WS  ┌────────────────────┐
│  Genome firehose   │ ─────────▶ │  Solver intent     │
│  taifoon.io/feed   │            │  buffer (Tokio)    │
└────────────────────┘            └─────────┬──────────┘
                                            │
                                            ▼
                                  [ Profit calculator ]
                                  fees − gas − liquidity
```

Source: `taifoon.io/api/genome/stream` (server-sent events). Every published intent carries a chain-pair, a notional, and a deadline. The buffer dedupes within a 5-block window per (chain, hash) tuple.

### 2. Profit calculator

```rust
// crates/solver/src/profit.rs
pub fn estimated_pnl(intent: &Intent, route: &Route) -> Pnl {
    let revenue = intent.notional * route.fee_bps / 10_000;
    let cost    = route.gas_estimate * gas_price() + route.liquidity_cost;
    Pnl { revenue, cost, net: revenue - cost }
}
```

Routes come from the adapter registry. Each adapter exposes `quote(intent) -> Route` synchronously; the solver picks the maximum-PnL route per intent.

### 3. TaifoonUniversalOperator (V5-proof-wrapped execution)

Every fill is dispatched as:

```solidity
TaifoonUniversalOperator.execute(
    V5Proof  memory proof,    // L1+L2+L3+L4+L6 anchor to SuperRoot
    bytes    calldata data    // adapter-specific calldata
) external returns (FillReceipt);
```

The Operator validates the V5 proof against `TaifoonMMRVerifier`, looks up the target adapter via the proof's `chain_id` + `protocol_tag`, and forwards the call. **No direct adapter calls are allowed** — every fill must be V5-anchored.

This is the cryptographic-settlement claim. The proof:
- L1 SuperRoot — hash of all 41-chain MMRs at the latest superroot tick
- L2 ChainHeader — header of the source chain at the fill block
- L3 SuperrootProof — Merkle siblings linking L2 → L1
- L4 BlockProof — twig siblings inside the chain MMR
- L5 ChainEvent — encoded transaction + receipt for the fill
- L6 FinalityCommitment — chain-specific finality witness (GRANDPA, ETH PoS checkpoint, ARB BOLD, …)

Reproducible without trusting any solver. Every Frontier judge can verify.

### 4. Adapter fleet (9 protocols)

Adapters live at [yawningmonsoon/spinner/taifoon-eco/contracts/adapters/](https://github.com/yawningmonsoon/spinner/tree/master/taifoon-eco/contracts/adapters):

| Protocol  | Status | Adapter |
|-----------|--------|---------|
| Across    | Live   | `AcrossAdapter.sol` |
| deBridge  | Live   | `DeBridgeAdapter.sol` |
| Mayan     | Live   | `MayanAdapter.sol` (+ Solana scaffolded) |
| Lambda (t3rn) | Live | `LambdaAdapter.sol` |
| Hyperlane | Open route | bounty `b-hyperlane-001` |
| LiFi      | Open route | bounty `b-lifi-002` |
| Squid     | Open route | bounty `b-squid-001` |
| CCIP      | Open route | bounty `b-ccip-001` |
| Stargate  | Open route | bounty `b-stargate-001` |

Anyone can ship an adapter under TSUL. Donut routes the revenue back.

### 5. Donut routing (BuildersRegistry)

```
After every fill:
TaifoonUniversalOperator → BuildersRegistry.recordRevenueTouch(
    adapterHash:  bytes32,
    valueRouted:  uint256
)
```

The registry computes 49 bps of `valueRouted`, splits it 70/20/10, and credits creator / reviewers / ecosystem in `claimable[]`. Pull at any time via `claim()`.

This is the on-chain enforcement of TSUL rule #4. Removing the registry call breaks the license.

### 6. Agentic OS dispatch (open-mamba)

`open-mamba` is the reviewer fleet. 11 agents:
- 6 chain-replay reviewers (evm/sol/btc/sui/cosmos/lambda)
- 5 cross-cutting reviewers (schema-conformer, oracle-checker, aggregator-decomposer, dossier-builder, platform-checker)

Each agent listens on its scope, runs deterministic replay against fixtures, signs a verdict, and submits to BuildersRegistry. Two PASS verdicts + 24h challenge window with no counter-example → auto-merge.

The **Brain** indicator on `/os/dispatch` polls `/api/dispatch/live-state` every 2s, showing dispatcher heartbeat + active reviews + recent verdicts. This is what makes the OS feel alive on demo day.

### 7. Self-extension loop

When a fill succeeds and reveals a new gap (a chain we don't decode, a protocol whose attribution is missing, an oracle that's stale), the loop runner opens a fresh bounty automatically. The OS extends itself.

Six loop edges currently wired (see `taifoon-next/public/data/roadmap.json` → `delivery_loop`):
- E1 — Decoder gap on a fill that succeeded but emitted unknown event
- E2 — Aggregator sub-attribution discrepancy
- E3 — Oracle stale-quote retry
- E4 — Solver dossier auto-generation
- E5 — Bounty fix-bounty (post-merge adapter failure)
- E6 — Reviewer-agent extension

Each edge has an SLA in hours and a COE-gate flag. Autonomous edges fire `BuildersRegistry.openBounty()` directly; COE-gated edges post to a review queue first.

---

## Why this beats a private solver

| Private solver | Taifoon OS |
|----------------|-----------|
| Hard-coded adapters | Public adapter marketplace under TSUL |
| MEV-extractive | Donut routes 70% to contributor, perpetually |
| Black-box settlement | V5 MMR proof anchors every fill |
| Siloed knowledge | Self-extension loop generates new bounties from real gaps |
| Internal team only | Anyone with a wallet can ship a route |
| Closed code | Apache 2.0 on solver core, TSUL fair-code on platform |

The Frontier judges who matter — Drift's w.sol, Altitude's Phil Jacobson, Ellipsis's Ray Zhang — have spent years thinking about how to make solver markets non-extractive. We arrive with the answer pre-shipped.

---

## See also

- [V5 proof spec](./v5-proofs.md)
- [TSUL posture](./tsul.md)
- [BuildersRegistry contract](https://github.com/yawningmonsoon/spinner/blob/master/taifoon-eco/contracts/registry/BuildersRegistry.sol)
- [open-mamba reviewer fleet](https://github.com/yawningmonsoon/spinner/tree/master/open-mamba)
- [Demo flow](../FRONTIER_DEMO.md)
