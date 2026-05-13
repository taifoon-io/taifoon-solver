# DEX Landscape Research & Taifoon Integration Paths

_Author: research pass, May 2026_
_Audience: builders, market makers, agent teams, and partner protocols evaluating Taifoon as a trading-integration target_
_Scope: a survey of leading open-source DEX repositories on Solana and EVM, with a map of how external participants integrate with Taifoon's trading surfaces._

This document is intentionally bounded to **public research** and **integration paths**. Taifoon's own implementation roadmap, contract specifics, validator topology, effort estimates, and sequencing are not described here.

---

## 1. Why this document exists

If you are a market maker, a token issuer, a trading-agent team, or a protocol thinking about routing flow through Taifoon, you want two things:

1. **A map of the space you are already operating in** — which open-source DEX codebases set the standards for spot CLOBs, perps, intent settlement, and solver auctions on Solana and EVM.
2. **A clear answer to "how do I plug in?"** — what interfaces exist, what data we publish, what credentials we accept, what economic model governs participation.

This document covers both. It does not describe what Taifoon is building internally; for that, see `taifoon.io` and our docs at `taifoon.io/docs`, which surface what is ready for public consumption.

---

## 2. Solana DEX repos worth knowing

| Repo | Type | License | One-liner |
|---|---|---|---|
| [`drift-labs/protocol-v2`](https://github.com/drift-labs/protocol-v2) | Perps + spot + lending | Apache | Most actively-maintained on-chain perps DEX on Solana. Cross-margin risk engine, JIT-proxy auction primitive, Pyth/Switchboard oracle integration. |
| [`drift-labs/drift-rs`](https://github.com/drift-labs/drift-rs) | Rust SDK | Apache | Reference Rust client for Drift v2 — `DriftClient`, `jit_client`, `marketmap`, `oraclemap`. |
| [`drift-labs/jit-proxy`](https://github.com/drift-labs/jit-proxy) | Solana program | Apache | Permissionless market-maker JIT auction responder. Pattern for agent-vs-agent fillable auctions. |
| [`Ellipsis-Labs/phoenix-v1`](https://github.com/Ellipsis-Labs/phoenix-v1) | On-chain CLOB | BUSL-1.1 | Crankless spot book, atomic settle, FIFO price-time priority. Audited by OtterSec. License blocks commercial forking until 2027 — study, do not fork. |
| [`openbook-dex/openbook-v2`](https://github.com/openbook-dex/openbook-v2) | On-chain CLOB | MIT + GPL | Event-heap design — match emits fill events, settled out-of-band by `consume_events`. Spot only. Integrated by Jupiter, Mango, Raydium routing. |
| [`jup-ag/*`](https://github.com/jup-ag) | Aggregator + Limit Order + DCA | Open SDK | Solana's dominant aggregator. Limit-order book is off-chain with on-chain settlement. |
| [`solana-labs/perpetuals`](https://github.com/solana-labs/perpetuals) | Reference perps | Apache | Solana Labs' reference implementation; less production-grade than Drift but instructive for account/PDA layout. |
| [`raydium-io/raydium-clmm`](https://github.com/raydium-io/raydium-clmm) | Concentrated-liquidity AMM | Open | Canonical Solana CLMM. Useful baseline if you compare AMM-vs-book design tradeoffs. |

### What's _good_ in Solana CLOB code, distilled

- **One instruction does one thing.** `place_order`, `cancel_order`, `consume_events`, `settle_funds` are separate.
- **Strict `Accounts<'info>` validation** with typed `seeds = […], bump`, `mut`, `has_one`, `token::mint`, `token::authority` constraints.
- **No `UncheckedAccount` without a `/// CHECK:` justification.** Most Anchor exploits originate from raw `AccountInfo` bypasses.
- **Event-heap decoupling** (OpenBook v2): matching emits to a ring buffer; settlement is a separate instruction. Keeps matching bounded in compute units.
- **Canonical bump derivation** — store the bump on the account, re-derive with `Pubkey::create_program_address` in hot paths.
- **Fixed-point math** (`fixed::types::I80F48`) for prices and quantities. Never floats on-chain.
- **Deterministic match logic** so every validator/observer arrives at the same fill order regardless of crank caller.
- **Compute-unit budgeting up front** via `ComputeBudgetInstruction::SetComputeUnitLimit` and `SetComputeUnitPrice`.

References: [Helius — A Hitchhiker's Guide to Solana Program Security](https://www.helius.dev/blog/a-hitchhikers-guide-to-solana-program-security), [Anchor PDA docs](https://github.com/solana-foundation/anchor/blob/master/docs/content/docs/basics/pda.mdx), [Inside Drift: Architecting a High-Performance Orderbook on Solana](https://extremelysunnyyk.medium.com/inside-drift-architecting-a-high-performance-orderbook-on-solana-612a98b8ac17).

---

## 3. EVM DEX / intent-settlement repos worth knowing

| Repo | Type | License | One-liner |
|---|---|---|---|
| [`cowprotocol/services`](https://github.com/cowprotocol/services) | Off-chain Rust services | LGPL-3.0 | Three-layer orderbook + autopilot + driver/solver. Batch auctions every ≈30s. Reference shape for intent-style books. |
| [`cowprotocol/simple-solver-template`](https://github.com/cowprotocol/simple-solver-template) | Solver template | Open | Minimal solver scaffold. Useful starting point for builders. |
| [`Uniswap/UniswapX`](https://github.com/Uniswap/UniswapX) | Solidity reactor + filler examples | GPL-2.0 / MIT | `ExclusiveDutchOrderReactor.sol`, `IReactor`, `SwapRouter02Executor`. Clean abstraction for signed-orders → reactor → filler → settle. |
| [`Uniswap/v4-core`](https://github.com/Uniswap/v4-core) | Singleton `PoolManager.sol` + hooks | BUSL-1.1 | Not an order book, but the hooks pattern is the cleanest way to plug per-listing logic into one settlement contract. |
| [`1inch/fusion-resolver-example`](https://github.com/1inch/fusion-resolver-example) | Solidity resolver example | MIT | Reference for the staked-resolver model. |
| [`1inch/fusion-sdk`](https://github.com/1inch/fusion-sdk) | TS SDK | MIT | Reference for signing Fusion intents from a wallet. |
| [`1inch/solana-fusion-sdk`](https://github.com/1inch/solana-fusion-sdk) | TS SDK | MIT | Demonstrates cross-VM precedent for intent-style settlement. |
| [`aori-io/aori`](https://github.com/aori-io/aori) | Universal intent settlement | Open | Paired contracts across chains via LayerZero messaging. |
| [`dydxprotocol/v4-chain`](https://github.com/dydxprotocol/v4-chain) | Cosmos SDK chain with custom orderbook | AGPL-3.0 | Validators run an in-memory CLOB; only fills hit consensus. Pattern for keeping order placement gas-free at scale. |
| [`hyperliquid-dex/order_book_server`](https://github.com/hyperliquid-dex/order_book_server) + [`hyperliquid-rust-sdk`](https://github.com/hyperliquid-dex/hyperliquid-rust-sdk) | Order book server + Rust SDK | Open | HyperCore's L4 book snapshot + diff streaming — excellent feed-shape reference. |

### What's _good_ in EVM order-book / solver code, distilled

- **Three-layer split** (CoW): off-chain order book + auctioneer + solver. Cleanest separation we have seen.
- **All orders are EIP-712 signed structs** with `nonce`, `deadline`, `inputToken`, `outputToken`, `inputAmount`, `outputAmount` plus per-style extensions (Dutch decay, exclusivity).
- **Reactor pattern** (UniswapX): settlement is a thin generic contract; pluggable `IFiller` callbacks let solvers route through any liquidity.
- **Batched auctions as first-class entities** (CoW). Each auction has an id, frozen orders, a deadline, a winning solver, a score.
- **Solver bonding** (CoW + 1inch). Collateral + slashing make the donut-style economy enforceable.
- **Driver / solver split** (CoW). Driver handles RPC, signature recovery, settlement; solver only computes the match.
- **Indexer as a separate service** that watches chain events and reconciles state.
- **Minimal settlement-contract state.** Atomic settlement + nonce-only state.

References: [CoW Protocol Solvers docs](https://docs.cow.fi/cow-protocol/concepts/introduction/solvers), [UniswapX Architecture](https://docs.uniswap.org/contracts/uniswapx/architecture), [Modern DEXes — 1inch Limit Order Protocols](https://mixbytes.io/blog/modern-dex-es-how-they-re-made-1inch-limit-order-protocols), [Modern DEXes — CoW Protocol](https://mixbytes.io/blog/modern-dex-es-how-they-re-made-cow-protocol).

---

## 4. Taifoon integration paths for partners

`taifoon.io` defines five Integration Capability Paths (ICPs). For trading-shaped integration, the two relevant ones are **ICP-01 (DeFi Protocols)** and **ICP-02 (Institutional & Trading)**. What follows is the integration map for each participant type.

### 4.1 If you are a market maker or a trading-agent team

You want to fill orders and earn rebates + share of the fee donut. The integration path:

1. **Read the Hand interface.** Taifoon's trading SDK exposes a uniform `Hand` trait so a single client can quote and trade against centralized exchanges and on-chain DEXs without per-venue code. The trait shape mirrors `gmosx/kraken-sdk-rust`, extended with Drift-style JIT and UniswapX-style Dutch primitives. Operators can plug in additional venues out-of-tree by implementing the same trait.
2. **Pick a venue.** You can already trade against Drift v2 today using `drift-labs/drift-rs`, against Kraken using `gmosx/kraken-sdk-rust`, and against Spinner's cross-chain settlement surface via `api.taifoon.dev`. Any future venue plugs into the same trait — code does not change.
3. **Authenticate via SIWE.** `solver-api` (the Taifoon back-end at `api.taifoon.dev`) accepts SIWE (EIP-4361) for solver / agent provisioning. Sandbox keys are issued through `/api/hosting/provision`. The matching solver-side HTTP surface is at `/api/hand/*` — see `taifoon-solver/crates/solver-api/src/hand.rs`.
4. **Subscribe to the signals layer.** `taifoon.io/docs/oracle` and `/docs/v5-proof-api` document the real-time gRPC and SSE feeds that drive most institutional-grade strategies on Taifoon: gas oracle, finality oracle, sniper signals, cross-chain inclusion proofs.
5. **Earn from the donut.** Every fill attributable to a Taifoon-managed path contributes to the 70 / 20 / 10 split (builder + reviewer + ecosystem). See `taifoon.io/builders` and the Builders Programme docs.

### 4.2 If you are a token issuer or an L1/L2

You want exposure on Taifoon's discovery and routing surfaces so trading agents and market makers can discover and quote your asset.

1. **Submit a `ListingProposal`** to `solver-api`. Required: mint or ERC-20 address, metadata, liquidity-commitment, market-making bounty (USDC).
2. **Pass reviewer-agent attestation.** Two independent reviewer agents sign verdicts. Reuses the existing `donut-adjudicator` review loop that already governs adapter merges.
3. **Post a slashable bond.** Protects fillers and agents against rug-pulls and undisclosed transfer behaviour. The bond is held by a registry; slashed proceeds flow to affected fillers, reviewers, and the ecosystem treasury.
4. **Seed liquidity.** Either you, or a Taifoon-blessed market maker via the Hand SDK.
5. **Live on the routing surface.** Trading agents in the builders programme can now route orders to your market through Taifoon-managed paths.

For L1s and rollups wanting their tokens or their chain itself to be a first-class trading surface on Taifoon, **ICP-05 (Chains & Rollups)** is the right path — see `taifoon.io/for/rollups`.

### 4.3 If you are an adapter author or protocol integrator

You want to register a new cross-chain protocol or a new venue adapter and earn the builder share of every fill that flows through it.

1. **Write a `ProtocolAdapter` impl** (Rust) under TSUL license against the existing trait in `taifoon-solver/crates/protocol-adapters/`. Existing adapters (Across, deBridge, LiFi, Mayan, Orbiter) are the reference shape.
2. **Submit via the OS dispatch** at `taifoon.io/os/submit-job` (SIWE-gated). Bounty is paid out of the donut.
3. **Pass two reviewer-agent verdicts.** Automatic merge after both verdicts land.
4. **Earn 70%** of the donut split on every fill routed through your adapter, for the bonded period.

### 4.4 If you are a messaging / interop protocol

ICP-04 — see `taifoon.io/for/messaging`. Taifoon's V5 MMR proofs provide a parallel verification path for your cross-chain messages (dual-validation pattern). No trading integration required.

### 4.5 If you are an infrastructure / data provider

ICP-03 — see `taifoon.io/for/infrastructure`. You contribute headers or compute (Spinners); you receive execution-fee share from the autonomous DeFi spinner economy. No trading-side integration required.

---

## 5. The economic loop, briefly

```
   Token issuer / L1     ──────── listing fee + bounty ──────▶  Taifoon registry
   Agent / MM team       ──────── bond + Grip          ──────▶  Sandboxed Trader
   Adapter author        ──────── adapter + reviews    ──────▶  Live route
                                                                     │
                                                            70 / 20 / 10 donut
                                                                     │
                                                    ┌────────────────┼────────────────┐
                                                    ▼                ▼                ▼
                                            70% Builder        20% Reviewers     10% Ecosystem
                                            (issuer + adapter   (attestation       (treasury,
                                             author)             pool)              spinners)
```

Every adapter-routed cross-chain intent, every reviewed listing, every Taifoon-attributable fill — same split, same registry, same SuperRoot-anchored attestation trail.

---

## 6. What to do next

- **Trade today:** point your existing client at Drift v2 or Kraken — both are supported by the Taifoon trading SDK at launch, through the same `Hand` trait that any Taifoon-integrated venue uses.
- **List today:** open a thread in `#integrations` at `t.me/taifoon_network` and we will start the listing-proposal flow with you.
- **Build today:** read `taifoon.io/builders` and submit at `taifoon.io/os/submit-job`.
- **Stay in the loop:** the Taifoon Builders channel at `t.me/taifoon_network/9` is the most current source of integration news.

---

## Sources

Solana side:
- [drift-labs/protocol-v2 (GitHub)](https://github.com/drift-labs/protocol-v2)
- [drift-labs/drift-rs (GitHub)](https://github.com/drift-labs/drift-rs)
- [drift-labs/jit-proxy (GitHub)](https://github.com/drift-labs/jit-proxy)
- [drift-labs/keeper-bots-v2 (GitHub)](https://github.com/drift-labs/keeper-bots-v2)
- [drift-rs on docs.rs](https://docs.rs/drift-rs/latest/drift_rs/)
- [Drift Market-Maker Participation docs](https://docs.drift.trade/market-makers/market-maker-participation)
- [openbook-dex/openbook-v2 (GitHub)](https://github.com/openbook-dex/openbook-v2)
- [openbook-v2 DeepWiki](https://deepwiki.com/openbook-dex/openbook-v2)
- [Ellipsis-Labs/phoenix-v1 (GitHub)](https://github.com/Ellipsis-Labs/phoenix-v1)
- [Ellipsis-Labs/phoenix-sdk (GitHub)](https://github.com/Ellipsis-Labs/phoenix-sdk)
- [Jupiter GitHub org](https://github.com/jup-ag)
- [solana-labs/perpetuals (GitHub)](https://github.com/solana-labs/perpetuals)
- [raydium-io/raydium-clmm (GitHub)](https://github.com/raydium-io/raydium-clmm)
- [Helius — Hitchhiker's Guide to Solana Program Security](https://www.helius.dev/blog/a-hitchhikers-guide-to-solana-program-security)
- [Anchor PDA docs](https://github.com/solana-foundation/anchor/blob/master/docs/content/docs/basics/pda.mdx)

EVM side:
- [cowprotocol/services (GitHub)](https://github.com/cowprotocol/services)
- [cowprotocol/simple-solver-template (GitHub)](https://github.com/cowprotocol/simple-solver-template)
- [CoW Protocol Solvers docs](https://docs.cow.fi/cow-protocol/concepts/introduction/solvers)
- [Uniswap/UniswapX (GitHub)](https://github.com/Uniswap/UniswapX)
- [UniswapX Architecture](https://docs.uniswap.org/contracts/uniswapx/architecture)
- [Uniswap/v4-core (GitHub)](https://github.com/Uniswap/v4-core)
- [1inch/fusion-resolver-example (GitHub)](https://github.com/1inch/fusion-resolver-example)
- [1inch/fusion-sdk (GitHub)](https://github.com/1inch/fusion-sdk)
- [1inch/solana-fusion-sdk (GitHub)](https://github.com/1inch/solana-fusion-sdk)
- [MixBytes — Modern DEXes: 1inch](https://mixbytes.io/blog/modern-dex-es-how-they-re-made-1inch-limit-order-protocols)
- [MixBytes — Modern DEXes: CoW](https://mixbytes.io/blog/modern-dex-es-how-they-re-made-cow-protocol)
- [aori-io/aori (GitHub)](https://github.com/aori-io/aori)
- [dydxprotocol/v4-chain (GitHub)](https://github.com/dydxprotocol/v4-chain)
- [hyperliquid-dex/order_book_server (GitHub)](https://github.com/hyperliquid-dex/order_book_server)
- [hyperliquid-dex/hyperliquid-rust-sdk (GitHub)](https://github.com/hyperliquid-dex/hyperliquid-rust-sdk)
- [Flashbots — Illuminating Ethereum's Order Flow Landscape](https://writings.flashbots.net/illuminate-the-order-flow)

Taifoon:
- [Taifoon.io](https://taifoon.io)
- [Taifoon Docs](https://taifoon.io/docs)
- [Builders Programme](https://taifoon.io/builders)
- [V5 Proof API](https://taifoon.io/docs/v5-proof-api)
- [Gas Price Oracle](https://taifoon.io/docs/oracle)
- [Pricing](https://taifoon.io/pricing)
