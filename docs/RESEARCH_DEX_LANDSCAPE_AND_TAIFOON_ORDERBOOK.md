# DEX Landscape Research — and a Plan to Turn Taifoon-Solver into an Order Book Surface

_Author: research pass, May 2026_
_Audience: yawningmonsoon / Taifoon core_
_Scope: open-source DEX repos on Solana & EVM, gap analysis vs taifoon-solver, complexity of (a) hosting an EVM order book at `rpc.taifoon.dev` and (b) running our own Solana devnet, B2B pipeline for the builders program._

---

## 0. TL;DR

1. **What we have today.** Taifoon-solver is a high-quality **cross-chain intent solver** — it listens to Across / deBridge / Mayan / LiFi via the Genome SSE stream, prices fills against Warmbed gas, races to fill them, anchors the settlement in a V5 MMR proof, and redistributes fees via the 70/20/10 donut. It is _not_ a DEX. There is no order book, no matching engine, no Anchor program of our own, no signed-order ingestion API, no on-chain commit-reveal, no oracle layer, no margin/collateral.
2. **Best-in-class references.**
   - Solana CLOBs: **Phoenix v1** (crankless, atomic settle, FIFO) and **OpenBook v2** (event-heap, hybrid crank, Mango-derived).
   - Solana perps: **Drift v2** (JIT auction proxy, cross-margin, multi-liquidity).
   - EVM intent settlement: **CoW Protocol Services** (Rust, batch auctions every ~30 s), **UniswapX** (exclusive Dutch reactor + RFQ), **1inch Fusion** (resolver staking via Unicorn Power).
   - Universal intent: **Aori** (LayerZero-paired contracts), **dYdX v4** (Cosmos SDK off-chain in-memory orderbook in validators), **Hyperliquid** (HyperBFT L1 with native on-chain CLOB).
3. **Top three things taifoon-solver is missing relative to a real Solana DEX.** (a) An Anchor program defining `place_order / cancel / fill / settle` instructions with strict PDA + signer validation; (b) an in-memory matching engine driven by a fill bus that mirrors on-chain state (Phoenix-style FIFO or OpenBook v2 event-heap); (c) a signed-intent ingestion surface (EIP-712 on EVM, ed25519-detached-sig on Solana) with replay protection and bonded solver registration.
4. **`rpc.taifoon.dev` as an EVM order book is feasible in ~3–6 engineering-months.** We can ship a CoW-style batch auction + UniswapX-style Dutch reactor by **forking `cowprotocol/services` (Rust, same stack as us)**, swapping out the autopilot for our Spinner V5 proof pipeline, and deploying a `TaifoonReactor.sol` (≈600 LOC) inheriting from UniswapX's `IReactor`. The hard parts are not code — they are **solver bonding, dispute resolution, and MEV protection**.
5. **A Taifoon-operated Solana devnet is a 1–2 month effort** if we run a **fork of public devnet with a 3–5 validator private cluster + indexer + faucet + explorer**, hosting our own Anchor order-book program. Real cost is bandwidth and op-load (≈$1.5–4k/mo). Localnet is free but useless for competition because it has no continuity.
6. **The builders-program B2B pipeline is the actual product.** Token issuers + L1/L2s + market-makers pay Taifoon to be listed on the order book; agent teams pay (or trade for share of donut) to compete against each other on real liquidity; reviewer agents and adapter authors get paid out of every fill. The same 70/20/10 donut we already use for adapters works for listed markets — we just need a `ListedMarketsRegistry` Anchor account + EVM contract on top of the existing `BuildersRegistry`.

---

## 1. Solana DEX Repo Catalog

| Repo | Type | License | Why it matters for us | Stars / activity |
|---|---|---|---|---|
| [`Ellipsis-Labs/phoenix-v1`](https://github.com/Ellipsis-Labs/phoenix-v1) | On-chain CLOB program | BUSL-1.1 | The cleanest, most-cited Solana CLOB. Crankless — orders settle atomically inside the matching instruction. FIFO price-time priority. Audited by OtterSec. **This is the architecture we should clone for the Taifoon Solana book.** | Heavily referenced; ≈$75B cumulative volume cited by Ellipsis. |
| [`Ellipsis-Labs/phoenix-sdk`](https://github.com/Ellipsis-Labs/phoenix-sdk) | Rust + TS SDK | Open | The reference for how clients sign and submit place/cancel/match instructions. | Used by every Phoenix integrator. |
| [`openbook-dex/openbook-v2`](https://github.com/openbook-dex/openbook-v2) | Monorepo: Solana program + TS client | MIT + GPL (the program parts) | Event-heap design — matching emits fill events that are settled later by a crank instruction (`consume_events`). More flexible than Phoenix but with crank latency. Spot only. Order types: limit, market, IOC, FOK, post-only. | Integrated by Jupiter, Mango, Raydium routing. |
| [`drift-labs/protocol-v2`](https://github.com/drift-labs/protocol-v2) | Perps DEX + spot + lending | Open | Cross-margin risk engine, **JIT auction via `jit-proxy`** (stateless permissionless program where market-makers respond to auctions in real time). The blueprint for our "agents on real books" idea — JIT proxy is literally agent vs. agent. | Largest perps DEX by volume on Solana. |
| [`jup-ag/*`](https://github.com/jup-ag) | Aggregator + Limit Order + DCA + Trigger | Open SDK / closed router internals | Jupiter's Trigger API is the de-facto Solana intent surface for swaps. Their Limit Order book lives off-chain with on-chain settlement. **Closest analog to what we'd build at `rpc.taifoon.dev` for Solana.** | Largest aggregator on Solana. |
| [`solana-labs/perpetuals`](https://github.com/solana-labs/perpetuals) | Solana Labs reference perps | Apache | Reference implementation; less production-grade than Drift but instructive for the account/PDA layout. | Reference. |
| [`raydium-io/raydium-clmm`](https://github.com/raydium-io/raydium-clmm) | Concentrated-liquidity AMM | Open | Not a CLOB, but the canonical Solana CLMM. Useful if we want a hybrid book+pool. | Raydium-maintained. |
| Lifinity | Oracle-based proactive MM | Closed source | Pioneer of Pyth-driven AMM on Solana. Pattern (not code) to copy if we want oracle pricing alongside the book. | — |
| Zeta Markets | Perps with CLOB | Closed; **discontinued** (stopped operations) | Cautionary tale; **don't** treat it as a live competitor. | Dead. |

### What "good" looks like — Solana CLOB code practices

Distilled from Phoenix and OpenBook v2:

- **One instruction does one thing.** `place_limit_order`, `cancel_order`, `consume_events`, `settle_funds` are separate. Taifoon's current monolithic `execute_fill` would be ~5 instructions on Solana.
- **Strict `Accounts<'info>` validation.** Every account is typed with `seeds = […], bump`, `mut`, `has_one = market`, `token::mint`, `token::authority`. Phoenix's `MarketHeader` and `Side` enums are PDA-protected against substitution.
- **No `UncheckedAccount` without a `/// CHECK:` comment.** Helius's security guide explicitly calls this out — most Anchor exploits come from `AccountInfo` bypasses.
- **Event-heap decoupling (OpenBook).** Match emits fill events to a ring buffer; settlement is a separate instruction. Lets the matcher stay deterministic and bounded in compute units. **This is the pattern we should adopt** — it maps cleanly onto our V5-proof-then-settle flow.
- **Canonical bump derivation.** Always store `bump` in the account and re-derive with `Pubkey::create_program_address` rather than `find_program_address` in hot paths.
- **Unique seed prefixes per account family** (`b"market"`, `b"order"`, `b"open_orders"`) to make collisions impossible.
- **Compute-unit budgeting up front** (`ComputeBudget::SetComputeUnitLimit`) — taifoon-solver already does this in `mayan_solana.rs:1–100`, good.
- **Fixed-point math (e.g. `fixed::types::I80F48`)** for prices and quantities — never floats on-chain. Drift uses I80F48 from the `fixed` crate.
- **Deterministic match logic** so all validators arrive at the same fill order regardless of crank caller (dYdX v4 paid hard for this).

Sources: [Helius — Hitchhiker's Guide to Solana Program Security](https://www.helius.dev/blog/a-hitchhikers-guide-to-solana-program-security), [Vultbase — Anchor Program Security](https://www.vultbase.com/articles/anchor-program-security-solana), [OpenBook v2 DeepWiki](https://deepwiki.com/openbook-dex/openbook-v2), [Drift v2 README](https://github.com/drift-labs/protocol-v2/blob/master/README.md).

---

## 2. EVM DEX Repo Catalog

| Repo | Type | License | Why it matters | Notes |
|---|---|---|---|---|
| [`cowprotocol/services`](https://github.com/cowprotocol/services) | **Rust off-chain services** (orderbook, autopilot, driver) | LGPL-3.0 | **Same Rust stack as taifoon-solver.** Three crates that map almost 1:1 to what we'd build: `orderbook` (HTTP API + DB), `autopilot` (batch organizer), `driver`/`solver` (matcher). | The most useful single reference for `rpc.taifoon.dev`. |
| [`cowprotocol/simple-solver-template`](https://github.com/cowprotocol/simple-solver-template) | Minimal solver template | Open | The "hello world" for solver competition. Useful for builders-program onboarding. | — |
| [`Uniswap/UniswapX`](https://github.com/Uniswap/UniswapX) | Solidity reactor + filler examples | GPL-2.0 / MIT | `ExclusiveDutchOrderReactor.sol`, `IReactor` interface, `SwapRouter02Executor` filler. The clean abstraction for **signed orders → reactor → filler → settle**. ≈600 LOC for a reactor + ~300 LOC for a filler. | Pattern to mirror in `TaifoonReactor.sol`. |
| [`Uniswap/v4-core`](https://github.com/Uniswap/v4-core) | Singleton `PoolManager.sol` + hooks | BUSL-1.1 | Not an order book, but the hooks pattern is how we plug **per-token-launch logic** into the same settlement contract. A "TaifoonListingHook" could enforce KYC/whitelist/fee schedule per market. | Adopt the hooks pattern in our reactor. |
| [`1inch/fusion-resolver-example`](https://github.com/1inch/fusion-resolver-example) | Solidity resolver example | MIT | Shows the **staked-resolver** model (Unicorn Power, 1INCH stake). Useful for solver bonding design. | Pattern, not code. |
| [`1inch/fusion-sdk`](https://github.com/1inch/fusion-sdk) | TS SDK | MIT | Reference client for signing Fusion intents. | — |
| [`1inch/solana-fusion-sdk`](https://github.com/1inch/solana-fusion-sdk) | TS SDK | MIT | **1inch already brought Fusion to Solana** — proves the cross-VM precedent for what we're trying to do. | — |
| [`aori-io/aori`](https://github.com/aori-io/aori) | Universal intent settlement | Open | Paired contracts across chains via LayerZero messaging; user-signed intent on src chain → solver fulfils on dst chain. **Closest existing protocol to what Taifoon-solver routes _through_** — could be an integration target rather than a competitor. | Cross-chain intents. |
| [`dydxprotocol/v4-chain`](https://github.com/dydxprotocol/v4-chain) | Cosmos SDK chain with custom orderbook module | AGPL-3.0 | **Validators run an in-memory CLOB**; only fills (not orders) hit consensus. Pattern for keeping order placement gas-free at scale. | Tendermint-only path; harder to fork. |
| [`hyperliquid-dex/order_book_server`](https://github.com/hyperliquid-dex/order_book_server) + [`hyperliquid-rust-sdk`](https://github.com/hyperliquid-dex/hyperliquid-rust-sdk) | Order book server + Rust SDK | Open | HyperCore's L4 book snapshot + diff streaming. **Excellent feed-shape reference** for our `rpc.taifoon.dev` WebSocket API. | Apache/MIT. |
| `0x` Protocol v4 | RFQ + meta-tx EVM | Open | Long-running RFQ standard; useful for EIP-712 order layout. | — |

### What "good" looks like — EVM order book / solver code practices

Distilled from CoW services, UniswapX, and 1inch Fusion:

- **Separation of concerns into three layers** (CoW): off-chain order book + auctioneer + solver. Maps onto Taifoon as `solver-api` (book) + new `auctioneer` crate (autopilot) + existing `executor` (solver).
- **All orders are EIP-712 signed structs** with `nonce`, `deadline`, `inputToken`, `outputToken`, `inputAmount`, `outputAmount`, plus protocol-specific extensions (Dutch decay parameters, exclusivity period).
- **Reactor pattern (UniswapX).** The settlement contract is a thin generic reactor; pluggable `IFiller` callbacks let solvers route through any liquidity. This is structurally the same as Taifoon's `ProtocolAdapter` trait — but **on-chain**.
- **Batched auctions with an explicit "auction" entity** (CoW). Each auction has an ID, a frozen set of orders, a deadline, a winning solver, a score. We already log "outcomes" per intent; we need to log _auctions_ as a first-class object.
- **Solver bonding** (CoW + 1inch). Solvers post collateral and can be slashed for bad behaviour. This is the cleanest way to make our donut economy enforceable.
- **Driver/solver split** (CoW). The driver handles RPC, signature recovery, settlement; the solver only computes the match. Lets us run third-party solvers safely.
- **Indexer service** as a separate process. CoW's autopilot watches chain events and reconciles. Our outcome log fills this role partially; needs to be authoritative for "did the fill land?".
- **No state in the settlement contract beyond the bare minimum.** Phoenix's atomic settlement + UniswapX's nonce-only state both demonstrate this.

Sources: [CoW services repo](https://github.com/cowprotocol/services), [UniswapX architecture docs](https://docs.uniswap.org/contracts/uniswapx/architecture), [Modern DEXes — 1inch](https://mixbytes.io/blog/modern-dex-es-how-they-re-made-1inch-limit-order-protocols), [Modern DEXes — CoW](https://mixbytes.io/blog/modern-dex-es-how-they-re-made-cow-protocol).

---

## 3. Gap Analysis: Taifoon-Solver vs Production Solana DEX Practices

The repo today (architecture summary):

```
crates/
  genome-client/         SSE poller — pulls Across/deBridge/Mayan/LiFi intents from api.taifoon.dev
  profit-calc/           Fee + gas + spread → ProfitResult; Warmbed gas client; 30s cache
  protocol-adapters/     Trait `ProtocolAdapter`; AcrossAdapter, DeBridgeAdapter, MayanAdapter, LiFiAdapter, OrbiterAdapter
  protocol-adapters-solana/  Mayan Swift / DLN-Solana / Wormhole NTT adapters (call EXTERNAL programs)
  executor/              Execution waterfall (OwnFunds → FlashLoan → T3RN); SQLite outcome log; SpinnerClient
  solver-api/            Axum HTTP+SSE; SIWE auth on /provision; intents/outcomes streams
  solver-main/           Binary; tokio main; tracing JSON
  t3rn-sidecar/          LWC bridge controller
  donut-adjudicator/     70/20/10 attestations; thiserror + sha2 + async_trait
  portfolio-sidecar/     Balance tracking + rebalancer (Kamino integration stub)
  wallet-manager/        MESSIAH macOS keychain integration
  solver-registry/, solver-sandbox/, taifoon-arb-bridge/, taifoon-cli/, mempool-monitor/
```

### 3.1 The honest verdict

Taifoon-solver is **an intent-fulfilment fleet, not an exchange**. Everything it does is _reactive_ — wait for someone else's order book event, race to fill it, anchor in a V5 proof. There is no notion of "we hold a book", "we match buyers and sellers", "we publish quotes". To become `rpc.taifoon.dev` as a venue, three deep changes are required.

### 3.2 The top three structural gaps (with proposed code changes)

#### Gap #1 — No signed-intent ingestion surface

**Problem.** Today, intents enter via `genome_client` SSE from external protocols. There is no public endpoint where a wallet (or an AI agent) can post `"I want to buy 5 SOL for ≤$1,250 by 17:00 UTC, here is my signature"`. Without that we can't run our own book.

**Reference patterns.** UniswapX `ExclusiveDutchOrder` struct (EIP-712); CoW `services/crates/orderbook/openapi`; Aori signed-intent flow.

**Proposed change.** Add a new crate `taifoon-intents/` that defines:

```rust
// crates/taifoon-intents/src/lib.rs (NEW)
use alloy::primitives::{Address, U256, B256};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Chain { Evm(u64), Solana }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitOrder {
    pub maker: String,                 // EVM address or Solana pubkey, chain-tagged
    pub chain: Chain,
    pub input_token: String,
    pub output_token: String,
    pub input_amount: U256,
    pub min_output_amount: U256,       // limit price = output / input
    pub start_time: u64,
    pub deadline: u64,
    pub nonce: U256,
    pub exclusivity: Option<Exclusivity>,
    pub decay: Option<DutchDecay>,     // optional: makes this a UniswapX-style auction
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Exclusivity { pub filler: String, pub override_bps: u16, pub period_secs: u32 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DutchDecay { pub start_amount: U256, pub end_amount: U256 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedOrder {
    pub order: LimitOrder,
    pub signature: String,             // 0x... for EVM (EIP-712), or base58 for Solana ed25519
    pub order_hash: B256,              // canonical hash used for replay protection
}

pub fn eip712_typed_data_hash(order: &LimitOrder, domain_separator: B256) -> B256 { /* ... */ }
pub fn verify(signed: &SignedOrder) -> Result<Address, OrderError> { /* recover + match */ }
```

Then expose it in `solver-api`:

```rust
// crates/solver-api/src/routes/orders.rs (NEW)
// POST /api/v1/orders            — submit a signed order
// GET  /api/v1/orders            — list open orders (paginated)
// GET  /api/v1/orders/:hash      — read one
// POST /api/v1/orders/:hash/cancel — owner-signed cancel
// GET  /api/v1/book/:market      — full book snapshot
// GET  /api/v1/book/:market/sse  — live diffs (Hyperliquid-style L4 feed)
```

Wire into `Cargo.toml` workspace members. Estimate: **~2 weeks** for one engineer.

#### Gap #2 — No matching engine + no on-chain settlement program

**Problem.** Profit-calc reasons about a single intent; there is no in-memory price-time priority book and no Anchor program to atomically settle a match on Solana (or a Solidity reactor on EVM).

**Reference patterns.** Phoenix v1 `FIFOMarket` instruction (`Ellipsis-Labs/phoenix-v1`); OpenBook v2 event-heap; UniswapX `BaseReactor.execute()` callback model.

**Proposed change.** Two pieces.

(a) **Matching engine** as a new crate `taifoon-matcher/`:

```rust
// crates/taifoon-matcher/src/lib.rs (NEW)
use std::collections::BTreeMap;
use taifoon_intents::SignedOrder;

pub struct Book {
    pub bids: BTreeMap<Price, Level>,  // price descending (use Reverse<Price>)
    pub asks: BTreeMap<Price, Level>,  // price ascending
    pub orders: HashMap<OrderHash, Arc<SignedOrder>>,
}

pub struct Level { pub queue: VecDeque<OrderHash>, pub total_size: U256 }

#[derive(Debug, Clone)]
pub struct Fill {
    pub maker_order: OrderHash, pub taker_order: OrderHash,
    pub price: Price, pub size: U256, pub timestamp_ms: u64,
}

impl Book {
    pub fn place(&mut self, order: SignedOrder) -> Vec<Fill> { /* price-time priority */ }
    pub fn cancel(&mut self, hash: OrderHash, signed_by: Address) -> Result<()> { /* owner check */ }
    pub fn snapshot(&self) -> BookSnapshot { /* for SSE clients */ }
}
```

Property tests in `crates/taifoon-matcher/tests/`:
- **price-time priority** invariant (no order skips a same-price earlier order),
- **conservation of size** (sum of fills equals min of crossing orders' remaining sizes),
- **no self-trades** unless explicitly allowed,
- **monotone book state** after a cancel.

(b) **On-chain settlement.** Two new repos / paths:

- `contracts-evm/TaifoonReactor.sol` — inherits UniswapX `BaseReactor`, validates the EIP-712 signature, calls a pluggable `IFiller` (which is a Taifoon adapter or third-party solver). ≈600 LOC + tests. Reuses `Uniswap/UniswapX/src/lib/OrderInfoLib.sol` patterns.
- `programs/taifoon_book/` — new Anchor program defining `place_order / cancel_order / consume_fills / settle_funds`. Phoenix-style FIFO matching, OpenBook-style event heap for off-chain crank. ≈1,500–2,500 LOC of Rust + Anchor.

This is the **single biggest piece of work**: realistically 6–10 engineering-weeks for a senior Anchor dev, with audit on top.

**Why we don't fork Phoenix verbatim.** Phoenix is BUSL-1.1 — non-commercial until 2027. We can study it, but the Anchor program needs to be original. OpenBook v2 (MIT) is cleaner license-wise; **recommend forking openbook-v2 and modifying** if we want to ship in <3 months.

#### Gap #3 — No solver bonding + no auction-as-an-entity

**Problem.** The donut split (70/20/10) only fires when a solver _claims_ a fill. There's no commitment to fill, no slashing if a solver loses an auction and then front-runs, and no way to penalise an agent that stalls a market. A real venue needs solvers to post collateral and auctions to be first-class objects.

**Reference patterns.** CoW solver bonding (must hold COW + ETH collateral); 1inch Unicorn Power staking; Drift's `jit-proxy` market-maker registration.

**Proposed change.**

```rust
// crates/donut-adjudicator/src/auction.rs (NEW)
pub struct Auction {
    pub id: AuctionId,
    pub market: MarketId,
    pub orders: Vec<OrderHash>,
    pub opened_at: u64, pub closes_at: u64,
    pub winning_solver: Option<SolverId>,
    pub winning_score: Option<Score>,
    pub settlement_tx: Option<TxRef>,
    pub status: AuctionStatus,           // Open, Won, Settled, Disputed, Slashed
}

pub trait AuctionAdjudicator: Send + Sync {
    async fn open(&self, market: MarketId, orders: &[OrderHash]) -> Result<AuctionId>;
    async fn submit_bid(&self, a: AuctionId, solver: SolverId, score: Score) -> Result<()>;
    async fn settle(&self, a: AuctionId, settlement: TxRef) -> Result<DonutSplit>;
    async fn dispute(&self, a: AuctionId, evidence: V5ProofBlob) -> Result<()>;
}
```

Extend `solver-registry/` to track collateral, slash conditions, and reputation. EVM side: a new `SolverBondRegistry.sol` (≈300 LOC) staking USDC or our token; slashing controlled by a multisig in v1, by a governance module in v2.

Estimate: **~3–4 weeks** including contract + adjudicator.

### 3.3 Secondary gaps (worth listing, not blocking)

| Gap | Where it lives in repo | Suggested fix |
|---|---|---|
| Env-only config; no typed config files | `solver-main/src/main.rs:79–100` | Adopt `figment` or `config` crate (already in workspace deps!) for layered config |
| No Prometheus metrics | `solver-main/src/main.rs:66–77` | Add `prometheus-client` + `/metrics` endpoint in `solver-api` |
| No property/invariant tests | `tests/fixture_validator.rs` | Add `proptest` to `taifoon-matcher` + a `kani` proof for the donut split math |
| `UncheckedAccount`-style usage in Solana adapters (sigVerify=false at estimate time only — OK; flag for audit) | `protocol-adapters-solana/src/mayan_solana.rs` | Add a comment + lint that real-send always re-verifies; this is fine for estimate but document it |
| Float arithmetic in profit calc | `profit-calc/src/lib.rs:64–150` | Move price math to `fixed::I80F48` or `rust_decimal` for the matching engine path |
| Single SQLite outcome log; no replay log | `executor/src/outcome_log.rs` | Add a write-ahead JSONL log per auction for forensic replay |
| Dashboard reads SQLite-shaped data, no order book widget | `dashboard/app/analytics/page.tsx` | Add `/book/:market` page consuming the new SSE feed; reuse Hyperliquid L4 feed shape |

---

## 4. `rpc.taifoon.dev` — EVM Order Book Complexity Assessment

### 4.1 What we mean by "EVM order book at rpc.taifoon.dev"

A public endpoint where:
- users post EIP-712-signed limit / Dutch / RFQ orders;
- solvers (including our own + the builders-program participants) compete in batched auctions;
- settlement lands atomically on chain via a single reactor contract;
- every fill is anchored in a V5 MMR proof (we already do this — it stays);
- the donut split runs on every settled auction.

### 4.2 Architecture (proposed)

```
                          ┌─────────────────────────────────────────────────────────┐
   User wallet ──────────▶│  rpc.taifoon.dev — solver-api (Axum)                    │
   AI agent  ─────────────│  POST /v1/orders, GET /v1/book, WS /v1/book/sse         │
                          └────────┬──────────────────────────────────────┬─────────┘
                                   │ submit                               │ subscribe
                                   ▼                                      │
                          ┌──────────────────────────┐                    │
                          │  taifoon-intents         │                    │
                          │  EIP-712 / ed25519 verify│                    │
                          └────────┬─────────────────┘                    │
                                   ▼                                      │
                          ┌──────────────────────────┐                    │
                          │  taifoon-matcher         │  fills──▶ SSE ─────┘
                          │  in-memory book          │
                          └────────┬─────────────────┘
                                   ▼
                          ┌──────────────────────────┐
                          │  auctioneer              │  every Δt seconds
                          │  freezes batch, broadcasts│
                          │  to solvers              │
                          └────────┬─────────────────┘
                                   ▼
                          ┌──────────────────────────┐
       3rd-party solvers─▶│  driver (per solver)     │── proposed solution ──┐
       Taifoon solver ───▶│  (CoW-style)             │                       │
                          └──────────────────────────┘                       │
                                                                             ▼
                                                              ┌──────────────────────────┐
                                                              │  Spinner V5 proof bundle │
                                                              │  + TaifoonReactor.sol    │
                                                              │  + BuildersRegistry.sol  │
                                                              │  → settle on chain        │
                                                              └──────────────────────────┘
```

### 4.3 Effort estimate

Working backwards from `cowprotocol/services` (which is ~85k LOC of Rust) and the UniswapX contracts (~3k LOC of Solidity), and crediting what we already have:

| Component | New / existing | Estimate |
|---|---|---|
| `taifoon-intents` crate (EIP-712 + ed25519 + replay) | New | 2 weeks |
| `taifoon-matcher` crate + property tests | New | 3 weeks |
| `auctioneer` crate (CoW-style batch organiser) | New | 3 weeks |
| `solver-api` routes for orders + book + WS feed | Extend existing | 2 weeks |
| `TaifoonReactor.sol` + filler interface | New | 3 weeks + audit |
| `SolverBondRegistry.sol` + adjudicator integration | New | 2 weeks + audit |
| Indexer (chain events → outcome log) | New | 2 weeks |
| Dashboard: book widget, auction history, solver leaderboard | Extend `dashboard/` | 2 weeks |
| 3rd-party solver template (`taifoon-solver-template-rs`) | New | 1 week |
| Internal load testing + canary on Sepolia | — | 2 weeks |
| Audit + remediation | Outside | 4–6 weeks calendar |

**Realistic shipping window: 3–6 engineering-months with two senior engineers (1 Rust + 1 Solidity)**, plus an external audit overlapping the last six weeks. **6–9 months calendar to mainnet.**

### 4.4 Risks (not addressed by code alone)

1. **Solver collusion / coordination.** CoW has fought this for two years. Solution patterns: rotating exclusivity, blind-auction (commit-reveal), Flashbots SUAVE-style sealed bids. Pick one and accept tradeoffs.
2. **MEV on the reactor itself.** UniswapX uses exclusivity + Dutch decay to make front-running unprofitable. We can adopt the same primitive.
3. **Order-book DoS** (cheap signed orders flooding the matcher). Rate-limit by maker address + require a small deposit per open order, or a `nonce` window per maker.
4. **Solver bonding economics.** Too cheap → no skin in game. Too expensive → no third-party solvers. CoW lands at ~$10k-equivalent + reputation; we should probably start there.
5. **Cross-chain settlement.** Our V5 proof works _today_ for source→destination intents. For an order book where both legs are on EVM, we don't strictly need cross-chain proofs — but if we expose a Solana-side book, we need a bridged settlement which adds another 4–8 weeks.

### 4.5 Sequencing — what to build first

1. (Week 1–4) Ship `taifoon-intents` + `taifoon-matcher` + the new `solver-api` routes. Run a **testnet-only book** with simulated fills.
2. (Week 5–10) Add the reactor + bond contracts on Sepolia. Settle real (testnet-USDC) fills. Onboard 3 internal solvers.
3. (Week 11–14) Run a public testnet competition. **This is the builders-program v1.**
4. (Week 15–18) Mainnet on Base or Arbitrum (cheapest reactor gas; both are EVM-equivalent for our purposes). One pair: WETH/USDC.
5. (Week 19+) Add markets, integrate Solana side via the devnet path in §5.

---

## 5. Solana Devnet Feasibility & Plan

### 5.1 Options

| Option | What we run | When it makes sense | Cost / mo |
|---|---|---|---|
| **A. Use public devnet** | Just deploy our Anchor book program to `api.devnet.solana.com` | Cheapest path to "agents trade on Solana with our program". Free SOL faucet, no infra. | **$0** (program deploy is one-time ~5 SOL on devnet) |
| **B. `solana-test-validator` in CI/local** | Per-developer / per-CI-job ephemeral validator | Development + integration tests only. No multi-party competition possible — no continuity. | $0 |
| **C. Taifoon-operated public testnet** (recommended for competitions) | 3–5 validators + 1 RPC node + 1 indexer + faucet + explorer fork | We want guarantees: uptime SLAs, custom genesis, our tokens pre-listed, controlled feature gates. **Builders-program competition surface lives here.** | $1,500–4,000/mo on Hetzner/Equinix; ≈1 mo to stand up |
| **D. Fork-and-rebrand devnet** | Snapshot devnet, restart with own validators | Maximum control; not really worth the operational burden | $4,000+/mo |

**Recommendation: A for v1, C for builders-program v2 (when we want to gate tokens and run scored competitions).**

### 5.2 Option C — engineering tasks

1. Anchor program `taifoon_book` deployed and audited (≈6–10 weeks, see §3.2).
2. Validator infra: 3 voting validators + 1 RPC + 1 archival, on hardware spec'd per Solana validator FAQ (≥256GB RAM, 2TB NVMe, 1Gbps symmetric).
3. Custom genesis with:
   - Pre-loaded SPL tokens for `BLD-1`, `BLD-2`, ... (builders-program-listed tokens),
   - Pre-funded faucet account,
   - Feature gate matched to current mainnet (avoid testing on stale runtime).
4. Indexer: fork [`dboures/openbook-candles`](https://github.com/dboures/openbook-candles) for our event format, write candles to TimescaleDB (we already have rusqlite + outcome log — keep that for solver attribution and add Timescale for market data).
5. Faucet API + explorer (fork solana-explorer; small SVG/banner change).
6. Public docs at `docs.taifoon.dev/devnet`.
7. On-call rotation. **This is the real cost.**

### 5.3 Tokens listed at launch

The point of the devnet is that **the builders program lists new tokens on it before mainnet**. So the listing pipeline (§6) needs to be live before the devnet has users.

---

## 6. B2B Pipeline: Builders Program → Live Trading Surface

### 6.1 Who pays whom

```
   ┌──────────────────────┐         ┌─────────────────────────────────────────────┐
   │ Token issuer / L1+L2 │── $$$ ─▶│ Taifoon listing fee + market-making bounty  │
   └──────────────────────┘         └──────────────────────────────────────────────┘
                                                       │
                                                       ▼
                                        ┌──────────────────────────────┐
   ┌──────────────────────┐             │  ListedMarketsRegistry        │
   │ Agent team           │── stake ───▶│  (donut economics applied to   │
   │ (AI trader / fund)   │             │   listings + fills)            │
   └──────────────────────┘             └──────────────────────────────┘
                                                       │
                                          70 / 20 / 10 │
                                                       ▼
                                ┌────────────────┬───────────────┬──────────────┐
                                │ 70% builder    │ 20% reviewer  │ 10% ecosystem│
                                │  (token issuer │  agents       │  treasury    │
                                │   + adapter)   │               │              │
                                └────────────────┴───────────────┴──────────────┘
```

### 6.2 The listing pipeline (concrete)

1. **Submit.** Issuer posts a `ListingProposal` to `solver-api`: token metadata, mint address (Solana) or ERC-20 (EVM), liquidity commitment, market-making bounty (in USDC), KYC attestation if applicable.
2. **Review.** Two reviewer agents independently sign verdicts. We already have this loop in `donut-adjudicator` for adapter merges — reuse the trait.
3. **Bond.** Issuer posts a slashable bond (e.g. $5k) against the market-making bounty.
4. **Deploy.** Anchor program `taifoon_book` initialises a new `Market` PDA; reactor on EVM gets a new `marketId`.
5. **Seed liquidity.** Either the issuer themselves, or a Taifoon-blessed market-maker, posts initial orders.
6. **Open competition.** Builders-program agents can now route to this market via `rpc.taifoon.dev`.
7. **Earn donut.** Every fill pays the donut: 70% to the issuer + the adapter author (split by their pre-declared ratio), 20% to the reviewers, 10% to ecosystem.
8. **Slashing.** If liquidity is withdrawn before the bonded period ends, or if the token turns out to be malicious, bond is slashed and distributed to filled traders.

### 6.3 Builders-program participant tracks

- **Adapter authors.** Write `ProtocolAdapter` impls (we already pay them 70% donut). Now also: write `Filler` impls for the new reactor.
- **Solver / agent authors.** Compete in auctions, earn solver share. Sandbox keys + dry-run mode + `taifoon-solver-template-rs` to onboard.
- **Market-maker authors.** Pre-deploy resting liquidity bots; earn maker rebate + portion of donut.
- **Reviewer authors.** Audit adapters and listings; paid out of the 20% slice. Already exists in our pipeline.
- **Token issuers.** Pay listing fee + bounty; receive the 70% builder slice on their own market.

### 6.4 B2B counterparties to start conversations with

- **L1/L2s wanting a Solana-VM equivalent listing surface** (Eclipse, Sonic SVM, Bonk, MagicBlock). Sell them: "we'll be your DEX surface on day one".
- **Issuance platforms** (Streamflow, Meteora DLMM, Solana Foundation grants pipeline). Plug into their token-launch UX.
- **AI agent frameworks** (Virtuals, Wayfinder, ElizaOS, AgentLayer). Sell them: "your agents can trade real money on real books, in our sandbox first".
- **Market makers** (Wintermute Solana desk, GSR, Auros). Sell them: solver slots + flow.
- **L1 wallets** (Phantom, Backpack, Rabby) for default-routing user intents.

---

## 7. Risks, Open Questions, and What I Need You to Decide

1. **License posture.** Phoenix v1 is BUSL-1.1 — we cannot directly fork it commercially. OpenBook v2 is MIT + GPL. **Decision needed:** fork OpenBook v2, or write fresh under MIT (slower but cleaner IP).
2. **EVM-first or Solana-first.** Recommendation: **EVM first** (3–6 months) because we already have alloy, Spinner V5 proofs target EVM today, and CoW services is a fork-able Rust codebase. Solana follows in months 6–10.
3. **Solver bonding token.** Use stablecoin (boring, works), or the Taifoon token (creates demand, but ties bond size to token volatility). CoW chose ETH+COW hybrid for this reason.
4. **MEV stance.** UniswapX-style exclusive Dutch is the lowest-effort answer that already has user trust. Sealed-bid is more interesting but doubles the engineering scope.
5. **How do we want third-party solvers to authenticate?** SIWE is already wired in `solver-api/src/lib.rs:43–44`. Reuse it for solver registration + sign every auction bid. Trivial extension.
6. **Devnet operations team.** None today. If we go Option C in §5, we need an on-call (or pay Triton / Helius to host the validators under our brand). **Decision needed.**
7. **Token issuer KYC.** Are we comfortable being the listing authority for unknown tokens? If yes, we move fast. If no, every listing needs a reviewer-agent attestation step (already supported by donut-adjudicator). Recommend the latter.

---

## 8. One-page recommended sequencing

| Quarter | What ships | Who builds | Outcome |
|---|---|---|---|
| **Q2 2026** | `taifoon-intents` + `taifoon-matcher` + new `solver-api` routes; **TaifoonReactor.sol on Sepolia**; 3 internal solvers; testnet-only book | 1 Rust + 1 Solidity senior | Internal demo, no real money |
| **Q3 2026** | Solver bond registry; auction-as-entity; dashboard book widget; **public testnet competition (builders-program v1)** | + 1 product | First external solvers; first listed test-tokens; press moment |
| **Q4 2026** | Audit + remediation; **Base mainnet, WETH/USDC pair**; first paid listing | + audit firm | Real revenue from listing fees and donut |
| **Q1 2027** | Anchor `taifoon_book` program audited; **Solana devnet (Option C) live**; first SVM listings; cross-VM intents via Aori/LayerZero | + 1 Anchor senior | Cross-chain venue; agent competition runs on real Solana liquidity |
| **Q2 2027** | Mainnet Solana market; multi-market reactor; sealed-bid v2 | full team | True open-market DEX, ours |

---

## Sources

Solana side:
- [openbook-dex/openbook-v2 (GitHub)](https://github.com/openbook-dex/openbook-v2)
- [openbook-v2 DeepWiki](https://deepwiki.com/openbook-dex/openbook-v2)
- [Ellipsis-Labs/phoenix-v1 (GitHub)](https://github.com/Ellipsis-Labs/phoenix-v1)
- [Ellipsis-Labs/phoenix-sdk (GitHub)](https://github.com/Ellipsis-Labs/phoenix-sdk)
- [Ellipsis Labs — Phoenix Perpetuals announcement](https://www.ellipsislabs.xyz/blog-posts/introducing-phoenix-perpetuals)
- [drift-labs/protocol-v2 (GitHub)](https://github.com/drift-labs/protocol-v2)
- [Inside Drift: Architecting a High-Performance Orderbook on Solana](https://extremelysunnyyk.medium.com/inside-drift-architecting-a-high-performance-orderbook-on-solana-612a98b8ac17)
- [Jupiter GitHub org](https://github.com/jup-ag)
- [solana-labs/perpetuals (GitHub)](https://github.com/solana-labs/perpetuals)
- [raydium-io/raydium-clmm (GitHub)](https://github.com/raydium-io/raydium-clmm)
- [Lifinity protocol docs](https://docs.lifinity.io/)
- [Solana Compass — orderbook trading apps](https://solanacompass.com/projects/category/defi/orderbook-trading)
- [Helius — A Hitchhiker's Guide to Solana Program Security](https://www.helius.dev/blog/a-hitchhikers-guide-to-solana-program-security)
- [Vultbase — Anchor Program Security](https://www.vultbase.com/articles/anchor-program-security-solana)
- [solana-foundation/anchor — PDA docs](https://github.com/solana-foundation/anchor/blob/master/docs/content/docs/basics/pda.mdx)
- [Solana — Clusters reference](https://solana.com/docs/references/clusters)
- [Solana Test Validator Guide](https://solana.com/developers/guides/getstarted/solana-test-validator)
- [QuickNode — Start a Solana local validator](https://www.quicknode.com/guides/solana-development/getting-started/start-a-solana-local-validator)
- [Cherry Servers — Cost to run a Solana node](https://www.cherryservers.com/blog/solana-node-cost)
- [sol-tutorials/solana-validator-faq.md](https://github.com/agjell/sol-tutorials/blob/master/solana-validator-faq.md)

EVM side:
- [cowprotocol/services (GitHub)](https://github.com/cowprotocol/services)
- [cowprotocol/simple-solver-template (GitHub)](https://github.com/cowprotocol/simple-solver-template)
- [CoW Protocol — Solvers docs](https://docs.cow.fi/cow-protocol/concepts/introduction/solvers)
- [MixBytes — Modern DEXes: CoW Protocol](https://mixbytes.io/blog/modern-dex-es-how-they-re-made-cow-protocol)
- [MetaLamp — CoW Protocol Batch Auctions Explained](https://metalamp.io/magazine/article/cow-protocol-batch-auctions-how-orderbook-autopilot-and-solvers-ensure-fair-trading)
- [Uniswap/UniswapX (GitHub)](https://github.com/Uniswap/UniswapX)
- [Uniswap docs — UniswapX architecture](https://docs.uniswap.org/contracts/uniswapx/architecture)
- [OpenZeppelin — UniswapX audit](https://www.openzeppelin.com/news/uniswapx-audit)
- [Uniswap/v4-core (GitHub)](https://github.com/Uniswap/v4-core)
- [Uniswap v4 — PoolManager docs](https://docs.uniswap.org/contracts/v4/concepts/PoolManager)
- [1inch/fusion-resolver-example (GitHub)](https://github.com/1inch/fusion-resolver-example)
- [1inch/fusion-sdk (GitHub)](https://github.com/1inch/fusion-sdk)
- [1inch/solana-fusion-sdk (GitHub)](https://github.com/1inch/solana-fusion-sdk)
- [MixBytes — Modern DEXes: 1inch](https://mixbytes.io/blog/modern-dex-es-how-they-re-made-1inch-limit-order-protocols)
- [aori-io/aori (GitHub)](https://github.com/aori-io/aori)
- [Aori — Universal intent settlement](https://www.aori.io/)
- [dydxprotocol/v4-chain (GitHub)](https://github.com/dydxprotocol/v4-chain)
- [dYdX — v4 technical architecture](https://www.dydx.xyz/blog/v4-technical-architecture-overview)
- [hyperliquid-dex/order_book_server (GitHub)](https://github.com/hyperliquid-dex/order_book_server)
- [hyperliquid-dex/hyperliquid-rust-sdk (GitHub)](https://github.com/hyperliquid-dex/hyperliquid-rust-sdk)
- [Flashbots — Illuminating Ethereum's Order Flow Landscape](https://writings.flashbots.net/illuminate-the-order-flow)
- [Quantifying Price Improvement in Order Flow Auctions (Uniswap blog PDF)](https://blog.uniswap.org/UniswapX_PI.pdf)
