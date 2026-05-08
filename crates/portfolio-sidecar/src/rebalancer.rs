//! Bridge decision engine.
//!
//! Given a classified inventory snapshot, emits the minimum set of Across
//! bridge intents needed to make every fill chain healthy:
//!
//! 1. LOW_GAS / CRITICAL chains first:
//!    Send $GAS_TOPUP_USD of stables from the best-funded surplus chain
//!    via Across /api/swap with outputToken=0x0 (native gas token).
//!
//! 2. LOW_FUNDS / CRITICAL chains:
//!    Send `shortfall` USD of stables from surplus chain via Across depositV3.
//!
//! Surplus chain selection priority: highest stable_usd first, never below
//! its own min_stable_usd + what we're about to send.

use alloy::{
    network::EthereumWallet,
    primitives::{Address, Bytes, U256},
    providers::{Provider, ProviderBuilder},
    rpc::types::TransactionRequest,
    signers::local::PrivateKeySigner,
    sol,
    sol_types::SolCall,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{info, warn};

use crate::{
    inventory::{InventoryStatus, InventoryTarget},
    scanner::ChainSnapshot,
    tx_guard::TxGuard,
};

sol! {
    function depositV3(
        address depositor, address recipient,
        address inputToken, address outputToken,
        uint256 inputAmount, uint256 outputAmount,
        uint256 destinationChainId,
        address exclusiveRelayer,
        uint32 quoteTimestamp, uint32 fillDeadline, uint32 exclusivityDeadline,
        bytes calldata message
    ) external payable;

    function approve(address spender, uint256 amount) external returns (bool);
    function allowance(address owner, address spender) external view returns (uint256);
}


// MayanSwift V2 — createOrderWithToken selector 0xa3a30834
// Field order matches Mayan SDK ABI (@mayanfinance/swap-sdk v13.3.0)
sol! {
    interface MayanSwiftCreate {
        struct Order {
            uint8 payloadType;
            bytes32 trader;
            bytes32 destAddr;
            uint16 destChainId;
            bytes32 referrerAddr;
            bytes32 tokenOut;
            uint64 minAmountOut;
            uint64 gasDrop;
            uint64 cancelFee;
            uint64 refundFee;
            uint64 deadline;
            uint8 referrerBps;
            uint8 auctionMode;
            bytes32 random;
        }

        function createOrderWithToken(
            address tokenIn,
            uint256 amountIn,
            Order order,
            bytes calldata customPayload
        ) external;
    }
}

// MayanForwarder (0x337685fd) — routes user deposits through Wormhole so Mayan indexers
// can discover the order. Call forwardERC20 here with protocolData = MayanSwift calldata.
// ABI verified from deployed contract on Ethereum mainnet.
sol! {
    interface MayanForwarder {
        struct PermitParams {
            uint256 value;
            uint256 deadline;
            uint8 v;
            bytes32 r;
            bytes32 s;
        }

        /// Deposit ERC-20 via the Forwarder. The Forwarder will:
        ///   1. Pull `amountIn` tokenIn from msg.sender
        ///   2. Approve `mayanProtocol` (MayanSwift) for the amount
        ///   3. Call `mayanProtocol` with `protocolData` (encoded createOrderWithToken)
        ///   4. Emit a Wormhole message so Mayan's indexer can discover the order
        function forwardERC20(
            address tokenIn,
            uint256 amountIn,
            PermitParams permitParams,
            address mayanProtocol,
            bytes calldata protocolData
        ) external payable;
    }
}

sol! {
    interface WETH {
        function withdraw(uint256 wad) external;
        function deposit() external payable;
    }
    interface UniswapSwapRouter {
        struct ExactInputSingleParams {
            address tokenIn;
            address tokenOut;
            uint24 fee;
            address recipient;
            uint256 amountIn;
            uint256 amountOutMinimum;
            uint160 sqrtPriceLimitX96;
        }
        function exactInputSingle(ExactInputSingleParams params) external payable returns (uint256 amountOut);
        function unwrapWETH9(uint256 amountMinimum, address recipient) external payable;
        function multicall(bytes[] calldata data) external payable returns (bytes[] memory results);
    }
}

/// Spoke pool addresses per chain (Across V3).
fn spoke_pool(chain_id: u64) -> Option<Address> {
    use std::str::FromStr;
    match chain_id {
        1     => Address::from_str("0x5c7BCd6E7De5423a257D81B442095A1a6ced35C5").ok(),
        10    => Address::from_str("0x6f26Bf09B1C792e3228e5467807a900A503c0281").ok(),
        137   => Address::from_str("0x9295ee1d8C5b022Be115A2AD3c30C72E34e7F096").ok(),
        8453  => Address::from_str("0x09aea4b2242abC8bb4BB78D537A67a245A7bEC64").ok(),
        42161 => Address::from_str("0xe35e9842fceaCA96570B734083f4a58e8F7C5f2A").ok(),
        59144 => Address::from_str("0x7e63a5f1a8F0B4D0934B2f2327DAEd3f6bb2Ee75").ok(),
        _     => None,
    }
}

fn rpc_for(chain_id: u64) -> &'static str {
    match chain_id {
        1     => "https://eth.llamarpc.com",
        10    => "https://mainnet.optimism.io",
        137   => "https://polygon.drpc.org",
        8453  => "https://mainnet.base.org",
        42161 => "https://arb1.arbitrum.io/rpc",
        59144 => "https://rpc.linea.build",
        _     => "",
    }
}

const GAS_TOPUP_USD: f64 = 4.0;
const MIN_BRIDGE_USD: f64 = 1.0;

const UNISWAP_SWAP_ROUTER_BASE: &str = "0x2626664c2603336E57B271c5C0b26F421741e481"; // SwapRouter02 on Base
const USDT_BASE: &str = "0xfde4C96c8593536E31F229EA8f37b2ADa2699bb2";
const WETH_BASE: &str = "0x4200000000000000000000000000000000000006";
const WETH_ARB: &str = "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1";
const WETH_OPT: &str = "0x4200000000000000000000000000000000000006";
const MIN_BASE_ETH_FOR_OPS: f64 = 0.0008;
const USDT_SWAP_AMOUNT_USD: f64 = 5.0;

/// MayanSwift V2 contract (same address on all EVM chains).
const MAYAN_SWIFT: &str = "0x40ffe85a28dc9993541449464d7529a922142960";
/// MayanForwarder contract (same address on all EVM chains).
/// Routes through Wormhole so the order is indexed by Mayan explorer and solver network.
const MAYAN_FORWARDER: &str = "0x337685fdaB40D39bd02028545a4FfA7D287cC3E2";
/// USDC on Base.
const USDC_BASE: &str = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";
/// Mayan Wormhole chain ID for Solana.
const MAYAN_SOLANA_CHAIN_ID: u16 = 1;
/// Amount (USD) to bridge to Solana each cycle.
/// Bootstrap orders need to be large enough for Mayan's external solver network to bother filling
/// (~$5 minimum). Below $5 orders may sit unfilled until the deadline expires.
const SOLANA_BRIDGE_USD: f64 = 5.0;

/// A bridge action decided by the rebalancer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeAction {
    pub src_chain: u64,
    pub dst_chain: u64,
    pub token_symbol: String,
    pub amount_usd: f64,
    pub kind: BridgeKind,
    /// Set after execution.
    #[serde(default)]
    pub tx_hash: Option<String>,
    #[serde(default)]
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeKind {
    /// Across /api/swap → native gas token on dst chain.
    GasTopup,
    /// Across depositV3 → stables on dst chain.
    StableFill,
    /// Sweep surplus repayments from a fill chain back to the home chain.
    ClaimSweep,
    /// Swap native gas token (MATIC/ETH) on a src-only chain to USDC on home chain.
    NativeSweep,
    /// Mayan Swift V2 createOrderWithToken → USDC + SOL gas drop on Solana.
    MayanSolana,
    /// Swap USDT→ETH or bridge WETH→Base to fund operations.
    EthBootstrap,
    /// Bridge stranded USDC directly from Arb/OP to Solana when Base lacks stables.
    SolanaBootstrap,
}

/// Chain ID of the canonical home chain — surplus repayments consolidate here.
pub const HOME_CHAIN_ID: u64 = 8453; // Base

/// Across suggested-fees response (fields we actually use).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FeesResp {
    #[serde(rename = "outputAmount")]
    output_amount: String,
    timestamp: String,
    #[serde(deserialize_with = "de_u32_flexible")]
    fill_deadline: u32,
    exclusive_relayer: String,
    #[serde(deserialize_with = "de_u32_flexible")]
    exclusivity_deadline: u32,
    output_token: OutputTokenField,
}

#[derive(Debug, Deserialize)]
struct OutputTokenField { address: String }

fn de_u32_flexible<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    use serde::de::{self, Visitor};
    struct V;
    impl<'de> Visitor<'de> for V {
        type Value = u32;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result { write!(f, "u32 or str") }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<u32, E> { Ok(v as u32) }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<u32, E> { Ok(v as u32) }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<u32, E> { v.parse().map_err(de::Error::custom) }
    }
    d.deserialize_any(V)
}

/// Across /api/swap response for gas top-up (outputToken = native).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SwapResp {
    swap_tx: SwapTx,
    #[serde(default)]
    approval_txns: Vec<ApprovalTxn>,
}

/// deBridge DLN public API response for `/dln/order/create-tx`.
/// We only consume the fields we actually need to build the tx.
#[derive(Debug, Deserialize)]
struct DlnCreateTxResp {
    tx: DlnTx,
    #[serde(rename = "estimation")]
    estimation: Option<DlnEstimation>,
}

#[derive(Debug, Deserialize)]
struct DlnTx { to: String, data: String, #[serde(default)] value: Option<String> }

#[derive(Debug, Deserialize)]
struct DlnEstimation {
    #[serde(rename = "dstChainTokenOut")]
    dst_chain_token_out: Option<DlnDstTokenOut>,
}

#[derive(Debug, Deserialize)]
struct DlnDstTokenOut { amount: Option<String> }

/// Reasons the Across path can fail in a way that should trigger the deBridge
/// fallback. Anything outside this list is a hard failure (e.g. tx_guard
/// blocked, RPC dropped) and is NOT retried.
#[derive(Debug)]
enum AcrossQuoteIssue {
    /// HTTP non-2xx from Across or `outputAmount=0` — treat as no liquidity.
    NoLiquidity(String),
}

impl std::fmt::Display for AcrossQuoteIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AcrossQuoteIssue::NoLiquidity(s) => write!(f, "across_quote_failed: {s}"),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SwapTx { to: String, data: String }

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApprovalTxn { chain_id: u64, to: String, #[serde(default)] data: String }

// ── Rebalancer ────────────────────────────────────────────────────────────────

pub struct Rebalancer {
    http: reqwest::Client,
    signer: PrivateKeySigner,
    solver_addr: Address,
    pub dry_run: bool,
}

impl Rebalancer {
    pub fn new(signer: PrivateKeySigner, dry_run: bool) -> Self {
        let solver_addr = signer.address();
        Self {
            http: reqwest::Client::builder()
                .user_agent("taifoon-portfolio-sidecar/1.0")
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            signer,
            solver_addr,
            dry_run,
        }
    }

    /// Given the current snapshots and targets, compute and execute the
    /// minimum set of bridge actions to make all fill chains healthy.
    pub async fn rebalance(
        &self,
        snapshots: &[ChainSnapshot],
        targets: &[InventoryTarget],
    ) -> Vec<BridgeAction> {
        // Build a mutable working copy of stable balances so we can deduct
        // amounts already committed in this cycle.
        let mut working_stable: std::collections::HashMap<u64, f64> = snapshots
            .iter()
            .map(|s| (s.chain_id, s.stable_usd))
            .collect();

        let mut actions: Vec<BridgeAction> = Vec::new();

        // Classify every chain.
        let classified: Vec<(InventoryStatus, &InventoryTarget, &ChainSnapshot)> = targets
            .iter()
            .filter_map(|t| {
                let snap = snapshots.iter().find(|s| s.chain_id == t.chain_id)?;
                let status = t.classify(snap.stable_usd, snap.gas_eth);
                Some((status, t, snap))
            })
            .collect();

        // Phase -1: ETH bootstrap on Base.
        // Step a: unwrap any WETH already sitting on Base (e.g. from a prior WETH bridge cycle).
        // Step b: if still gas-starved, swap USDT→WETH→ETH via Uniswap V3 multicall.
        {
            let base_snap = snapshots.iter().find(|s| s.chain_id == HOME_CHAIN_ID);
            if let Some(base) = base_snap {
                if base.gas_eth < MIN_BASE_ETH_FOR_OPS {
                    // Step a: unwrap WETH on Base if any
                    if base.weth_raw > 0 {
                        info!("🔓 Unwrapping {} WETH on Base → native ETH", base.weth_raw as f64 / 1e18);
                        let action = self.unwrap_weth_on_base(base.weth_raw).await;
                        actions.push(action);
                    } else if base.secondary_stable_raw > 0 && base.secondary_stable_usd >= USDT_SWAP_AMOUNT_USD {
                        // Step b: no WETH to unwrap — swap USDT instead
                        let swap_raw = usd_to_raw(USDT_SWAP_AMOUNT_USD, base.secondary_stable_decimals);
                        info!("⛽ Base ETH low ({:.6}), swapping ${:.2} USDT→ETH", base.gas_eth, USDT_SWAP_AMOUNT_USD);
                        let action = self.usdt_to_eth_swap(swap_raw).await;
                        actions.push(action);
                    }
                }
            }
        }

        // Phase -2: unwrap WETH on Arb/Opt in-place so that chain's native ETH can fuel
        // the ETH bootstrap bridge. Across rejects small WETH amounts as AMOUNT_TOO_LOW,
        // so unwrapping locally then using Uniswap ETH→USDC on that same chain is more reliable.
        let already_bootstrapping = actions.iter().any(|a| matches!(a.kind, BridgeKind::EthBootstrap));
        if !already_bootstrapping {
            let weth_src = snapshots.iter()
                .filter(|s| s.chain_id != HOME_CHAIN_ID && s.weth_raw > 0)
                .max_by(|a, b| a.weth_raw.partial_cmp(&b.weth_raw).unwrap_or(std::cmp::Ordering::Equal));
            if let Some(src) = weth_src {
                let weth_addr = if src.chain_id == 42161 { WETH_ARB } else { WETH_OPT };
                info!("🔓 Unwrapping {} WETH on chain {} → native ETH (ETH bootstrap prep)", src.weth_raw as f64 / 1e18, src.chain_id);
                let action = self.unwrap_weth_on_chain(src.chain_id, weth_addr, src.weth_raw).await;
                actions.push(action);
            }
        }

        // Phase 0: bootstrap gas for src-only chains that have stranded stables but no gas.
        // Without gas on the src chain we can't broadcast the Across deposit from it.
        // We need at least a tiny amount of gas even if min_gas_eth = 0 in the target config.
        const SRC_MIN_GAS_ETH: f64 = 0.001; // ~$2 MATIC/ETH — enough for an approval + deposit
        for (status, target, snap) in &classified {
            if *status != InventoryStatus::SrcOnly { continue; }
            if snap.bridge_token_usd < MIN_BRIDGE_USD { continue; } // nothing to unlock
            if snap.gas_eth >= SRC_MIN_GAS_ETH { continue; }        // already has enough gas to broadcast
            info!("⛽ Src-only chain {} has ${:.2} stables but no gas — bootstrapping", target.chain_name, snap.bridge_token_usd);

            // Find any chain with ETH gas AND at least GAS_TOPUP_USD of stables.
            // Priority: chain with most gas ETH (Arbitrum usually). Optimism is cheap too.
            // Use small bootstrap amount ($2) so we don't drain critical fill chains.
            const BOOTSTRAP_USD: f64 = 1.0; // minimum $1 of stables to pay for native gas delivery
            // Bootstrap: any chain with gas ETH and at least $1 stables.
            // Ignore reserve — unlocking Polygon's $16 is worth spending $1 from OP/Arb.
            let gas_src = snapshots.iter()
                .filter(|s| s.chain_id != target.chain_id && s.gas_eth >= 0.00001)
                .filter(|s| working_stable.get(&s.chain_id).copied().unwrap_or(0.0) >= BOOTSTRAP_USD)
                .max_by(|a, b| a.gas_eth.partial_cmp(&b.gas_eth).unwrap_or(std::cmp::Ordering::Equal));
            let Some(gas_src) = gas_src else {
                warn!("No chain with gas+stables to bootstrap {} gas", target.chain_name);
                continue;
            };
            // Top up gas on the src-only chain: use up to BOOTSTRAP_USD from gas_src stables.
            // No reserve deduction — bootstrapping is high-priority.
            let working = working_stable.get(&gas_src.chain_id).copied().unwrap_or(0.0);
            let available = working.min(BOOTSTRAP_USD);
            let amount_raw = usd_to_raw(available, gas_src.bridge_token_decimals);
            info!("🌉 Bootstrap: ${:.2} stables {} → {} native gas", available, gas_src.chain_id, target.chain_name);
            let action = self.gas_topup(gas_src, target, amount_raw, available).await;
            *working_stable.entry(gas_src.chain_id).or_insert(0.0) -= available;
            actions.push(action);
        }

        // Phase 1: gas top-ups (CRITICAL / LOW_GAS fill chains).
        for (status, target, _snap) in &classified {
            if !matches!(status, InventoryStatus::LowGas | InventoryStatus::Critical) {
                continue;
            }
            info!("⚠️  {} is {:?} — gas top-up needed", target.chain_name, status);

            // Find the best surplus source for the gas top-up.
            let src = best_surplus_source(&working_stable, targets, target.chain_id, GAS_TOPUP_USD);
            let Some(src_chain_id) = src else {
                warn!("No surplus chain available for gas top-up to {}", target.chain_name);
                continue;
            };
            let Some(src_snap) = snapshots.iter().find(|s| s.chain_id == src_chain_id) else {
                warn!("No snapshot for surplus chain {} (gas top-up skipped)", src_chain_id);
                continue;
            };

            let amount_raw = usd_to_raw(GAS_TOPUP_USD, src_snap.bridge_token_decimals);
            let action = self.gas_topup(src_snap, target, amount_raw, GAS_TOPUP_USD).await;
            *working_stable.entry(src_chain_id).or_insert(0.0) -= GAS_TOPUP_USD;
            actions.push(action);
        }

        // Phase 2: stable fill (CRITICAL / LOW_FUNDS fill chains, after gas is handled).
        for (status, target, snap) in &classified {
            if !matches!(status, InventoryStatus::LowFunds | InventoryStatus::Critical) {
                continue;
            }
            let shortfall = target.stable_shortfall(snap.stable_usd);
            if shortfall < MIN_BRIDGE_USD {
                continue;
            }
            info!(
                "💸 {} needs ${:.2} stable ({} status={:?})",
                target.chain_name, shortfall, snap.stable_usd, status
            );

            // Use MIN_BRIDGE_USD as the required spare (not the full shortfall) so that
            // a source chain with $8 can partially fill a $150 shortfall.
            let src = best_surplus_source(&working_stable, targets, target.chain_id, MIN_BRIDGE_USD);
            let Some(src_chain_id) = src else {
                warn!("No chain has >${:.2} available to fund {} shortfall", MIN_BRIDGE_USD, target.chain_name);
                continue;
            };
            let Some(src_snap) = snapshots.iter().find(|s| s.chain_id == src_chain_id) else {
                warn!("No snapshot for surplus chain {} (stable fill skipped)", src_chain_id);
                continue;
            };
            let src_target = targets.iter().find(|t| t.chain_id == src_chain_id);

            // Cap at working balance on source (already deducted by Phase 1 gas top-ups),
            // minus the source's own minimum reserve (0 for src-only chains).
            let src_reserve = src_target.map(|t| if t.is_fill_chain { t.min_stable_usd } else { 0.0 }).unwrap_or(0.0);
            let working = working_stable.get(&src_chain_id).copied().unwrap_or(0.0);
            let available = working - src_reserve;
            let send_usd = shortfall.min(available).max(0.0);
            if send_usd < MIN_BRIDGE_USD {
                warn!(
                    "Source chain {} has insufficient surplus ({:.2} available after reserve)",
                    src_snap.chain_id, available
                );
                continue;
            }

            let amount_raw = usd_to_raw(send_usd, src_snap.bridge_token_decimals);
            let action = self.stable_fill(src_snap, target, amount_raw, send_usd).await;
            *working_stable.entry(src_chain_id).or_insert(0.0) -= send_usd;
            actions.push(action);
        }

        // Phase 3a: claim sweep — sweep SURPLUS fill chains back to Base.
        // Gated behind !any_critical: when a chain is CRITICAL the next-tick Phase 1/2 will
        // redistribute directly, so routing via Base first would be wasteful.
        let any_critical = classified.iter().any(|(s, t, _)| {
            t.is_fill_chain && matches!(s, InventoryStatus::Critical)
        });

        if !any_critical {
            for (status, target, snap) in &classified {
                if target.chain_id == HOME_CHAIN_ID { continue; }
                if !target.is_fill_chain { continue; }
                if *status != InventoryStatus::Surplus { continue; }

                let working = working_stable.get(&target.chain_id).copied().unwrap_or(0.0);
                let sweep_usd = (working - target.target_stable_usd).max(0.0);
                if sweep_usd < MIN_BRIDGE_USD { continue; }

                info!(
                    "💰 {} is SURPLUS (${:.2}) — sweeping ${:.2} → Base (claim sweep)",
                    target.chain_name, working, sweep_usd
                );

                let amount_raw = usd_to_raw(sweep_usd, snap.bridge_token_decimals);
                let action = self.claim_sweep(snap, HOME_CHAIN_ID, amount_raw, sweep_usd).await;
                *working_stable.entry(target.chain_id).or_insert(0.0) -= sweep_usd;
                *working_stable.entry(HOME_CHAIN_ID).or_insert(0.0) += sweep_usd;
                actions.push(action);
            }
        }

        // Phase 3b: src-only chain recovery — runs unconditionally (independent of fill chains).
        //   (a) ERC-20 stables above high_water: sweep via Across depositV3.
        //   (b) Native surplus (MATIC on Polygon etc.): swap → USDC on Base via Across swap API.
        {
            const MIN_NATIVE_SWEEP_ETH: f64 = 0.5; // minimum native to bother sweeping
            const KEEP_NATIVE_FOR_GAS: f64  = 0.3; // reserve for future Polygon gas costs
            for (_status, target, snap) in &classified {
                if target.is_fill_chain { continue; }

                // (a) Stable sweep — ERC-20 via Across depositV3
                let working = working_stable.get(&target.chain_id).copied().unwrap_or(0.0);
                if working >= target.high_water_usd + MIN_BRIDGE_USD && snap.gas_eth >= 0.0001 {
                    let sweep_usd = working - target.high_water_usd;
                    if sweep_usd >= MIN_BRIDGE_USD {
                        info!(
                            "🧹 Src-only {} has ${:.2} stables — sweeping ${:.2} → Base",
                            target.chain_name, working, sweep_usd
                        );
                        let amount_raw = usd_to_raw(sweep_usd, snap.bridge_token_decimals);
                        let action = self.claim_sweep(snap, HOME_CHAIN_ID, amount_raw, sweep_usd).await;
                        *working_stable.entry(target.chain_id).or_insert(0.0) -= sweep_usd;
                        *working_stable.entry(HOME_CHAIN_ID).or_insert(0.0) += sweep_usd;
                        actions.push(action);
                    }
                }

                // (b) Native sweep — swap native → USDC on Base via Across
                // Keep KEEP_NATIVE_FOR_GAS for future sidecar gas costs.
                // MIN_NATIVE_SWEEP_USD is lower than MIN_BRIDGE_USD — recovery sweeps are
                // worth executing even for small amounts since the capital is otherwise stranded.
                const MIN_NATIVE_SWEEP_USD: f64 = 0.50;
                if snap.gas_eth >= MIN_NATIVE_SWEEP_ETH + KEEP_NATIVE_FOR_GAS {
                    let sweep_native = snap.gas_eth - KEEP_NATIVE_FOR_GAS;
                    let sweep_wei = (sweep_native * 1e18) as u128;
                    // Rough USD estimate (Polygon: POL ~$0.22, others: ETH ~$2530)
                    let native_usd_per = if target.chain_id == 137 { 0.22f64 } else { 2530.0 };
                    let sweep_usd_est = sweep_native * native_usd_per;
                    if sweep_usd_est < MIN_NATIVE_SWEEP_USD { continue; }

                    info!(
                        "🔄 Src-only {} has {:.4} native (~${:.2}) — native-sweep → Base USDC",
                        target.chain_name, sweep_native, sweep_usd_est
                    );

                    let mut action = BridgeAction {
                        src_chain: target.chain_id,
                        dst_chain: HOME_CHAIN_ID,
                        token_symbol: "native".into(),
                        amount_usd: sweep_usd_est,
                        kind: BridgeKind::NativeSweep,
                        tx_hash: None,
                        status: if self.dry_run { "dry_run".into() } else { "pending".into() },
                    };

                    if !self.dry_run {
                        match self.execute_native_sweep(target.chain_id, sweep_wei, HOME_CHAIN_ID).await {
                            Ok(hash) => { action.tx_hash = Some(hash); action.status = "sent".into(); }
                            Err(e) => {
                                warn!("Native sweep failed on {}: {e:#}", target.chain_name);
                                action.status = format!("error: {e:#}");
                            }
                        }
                    }
                    actions.push(action);
                }
            }
        }

        // Phase 4: Mayan Solana bridge — if SOLANA_ADDRESS is set and Base has enough stables.
        // Sends SOLANA_BRIDGE_USD worth of USDC from Base to the solver's Solana address
        // via MayanSwift V2 createOrderWithToken with gasDrop=0.01 SOL.
        //
        // Bootstrap mode: if the Solana wallet has 0 SOL we skip the reserve check and
        // bridge as long as Base holds at least SOLANA_BRIDGE_USD + MIN_BRIDGE_USD in raw balance.
        //
        // Cooldown: a 10-minute global minimum between bridge attempts so a single
        // runaway sidecar loop cannot drain the wallet by firing on every tick.
        static LAST_MAYAN_BRIDGE_SECS: AtomicU64 = AtomicU64::new(0);
        const MAYAN_BRIDGE_COOLDOWN_SECS: u64 = 600; // 10 minutes
        if let Ok(solana_addr) = std::env::var("SOLANA_ADDRESS") {
            if !solana_addr.is_empty() {
                let base_snap = snapshots.iter().find(|s| s.chain_id == HOME_CHAIN_ID);
                let base_working = working_stable.get(&HOME_CHAIN_ID).copied().unwrap_or(0.0);
                let base_target = targets.iter().find(|t| t.chain_id == HOME_CHAIN_ID);
                let base_reserve = base_target.map(|t| if t.is_fill_chain { t.min_stable_usd } else { 0.0 }).unwrap_or(0.0);
                let base_available_normal = base_working - base_reserve;

                // Check Solana SOL balance to detect bootstrap condition.
                let solana_lamports = check_solana_balance(&solana_addr).await;
                let needs_bootstrap = solana_lamports < 5_000_000; // < 0.005 SOL → bootstrap

                // In bootstrap mode, allow bridging if Base has enough raw (ignoring reserve).
                let base_available = if needs_bootstrap { base_working } else { base_available_normal };

                if let Some(base_snap) = base_snap {
                    // Use the bridge token (whichever of USDC/USDT has more) to avoid
                    // "transfer amount exceeds balance" when USDC < SOLANA_BRIDGE_USD but USDT isn't.
                    let bridge_usd = base_snap.bridge_token_usd;
                    let bridge_addr = &base_snap.bridge_token_addr;
                    let bridge_decimals = base_snap.bridge_token_decimals;
                    let bridge_symbol = if bridge_addr.eq_ignore_ascii_case(USDC_BASE) { "USDC" } else { "USDT" };

                    // Cap send amount to what the bridge token actually holds minus a small buffer.
                    let send_usd = SOLANA_BRIDGE_USD.min(bridge_usd - 0.05).max(0.0);
                    let send_raw = usd_to_raw(send_usd, bridge_decimals);

                    // In bootstrap mode: require at least $5 — Mayan's auction network skips smaller
                    // orders and immediately refunds them (observed: $1.39 and $0.748 both refunded).
                    let now_secs = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let last_bridge = LAST_MAYAN_BRIDGE_SECS.load(Ordering::Relaxed);
                    let cooldown_ok = now_secs.saturating_sub(last_bridge) >= MAYAN_BRIDGE_COOLDOWN_SECS;
                    // Hard stop: never bridge when Base balance is critically low.
                    // Prevents bootstrap mode from draining the wallet when Solana has 0 SOL.
                    const MIN_BRIDGE_WALLET_USD: f64 = 15.0;
                    let wallet_safe = bridge_usd > MIN_BRIDGE_WALLET_USD;
                    let can_bridge = wallet_safe && cooldown_ok && if needs_bootstrap {
                        send_usd >= 5.0
                    } else {
                        base_available >= SOLANA_BRIDGE_USD + MIN_BRIDGE_USD && send_usd >= 0.50
                    };
                    if !wallet_safe {
                        info!("🛑 Mayan bridge disabled: Base bridge token ${:.2} < ${:.0} minimum safe threshold", bridge_usd, MIN_BRIDGE_WALLET_USD);
                    }
                    if !cooldown_ok && (send_usd >= 5.0 || (!needs_bootstrap && base_available >= SOLANA_BRIDGE_USD + MIN_BRIDGE_USD)) {
                        let secs_remaining = MAYAN_BRIDGE_COOLDOWN_SECS.saturating_sub(now_secs.saturating_sub(last_bridge));
                        info!("⏳ Mayan bridge cooldown: {}s remaining before next bridge attempt", secs_remaining);
                    }
                    if can_bridge {
                        if needs_bootstrap {
                            info!(
                                "🚀 Mayan bootstrap bridge: ${:.2} {} Base → Solana {} (sol_lamports={})",
                                send_usd, bridge_symbol, &solana_addr[..8.min(solana_addr.len())], solana_lamports
                            );
                        } else {
                            info!(
                                "🌞 Mayan bridge: ${:.2} {} Base → Solana ({})",
                                send_usd, bridge_symbol, &solana_addr[..8.min(solana_addr.len())]
                            );
                        }
                        LAST_MAYAN_BRIDGE_SECS.store(now_secs, Ordering::Relaxed);
                        let action = self.mayan_solana_bridge(base_snap, &solana_addr, send_raw, send_usd, bridge_addr, bridge_symbol).await;
                        *working_stable.entry(HOME_CHAIN_ID).or_insert(0.0) -= send_usd;
                        actions.push(action);
                    } else if needs_bootstrap && wallet_safe {
                        // Before spending ETH on a swap, check if any non-Base chain already has
                        // USDC stranded from a prior bootstrap attempt — bridge it directly.
                        const MIN_DIRECT_BRIDGE_USD: f64 = 5.0; // Mayan refunds orders < ~$5 immediately
                        const MIN_GAS_FOR_BRIDGE: f64 = 0.00003; // min ETH to pay for forwardERC20 call
                        let direct_src = {
                            let arb_snap = snapshots.iter().find(|s| s.chain_id == 42161);
                            let opt_snap = snapshots.iter().find(|s| s.chain_id == 10);
                            [arb_snap, opt_snap].into_iter().flatten()
                                .filter(|s| s.bridge_token_usd >= MIN_DIRECT_BRIDGE_USD && s.gas_eth >= MIN_GAS_FOR_BRIDGE)
                                .max_by(|a, b| a.bridge_token_usd.partial_cmp(&b.bridge_token_usd).unwrap_or(std::cmp::Ordering::Equal))
                        };
                        if let Some(src) = direct_src {
                            let chain_name = if src.chain_id == 42161 { "Arbitrum" } else { "Optimism" };
                            info!(
                                "🚀 Direct USDC bridge (stranded): ${:.2} {} {} → Solana {} (sol_lamports={})",
                                src.bridge_token_usd, src.bridge_token_addr.get(..10).unwrap_or("?"), chain_name,
                                &solana_addr[..8.min(solana_addr.len())], solana_lamports
                            );
                            let (usdc_addr, _weth_addr, _router_addr) = match src.chain_id {
                                42161 => ("0xaf88d065e77c8cC2239327C5EDb3A432268e5831", WETH_ARB, "0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45"),
                                10    => ("0x0b2C639c533813f4Aa9D7837CAf62653d097Ff85", WETH_OPT, "0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45"),
                                _     => (USDC_BASE, WETH_BASE, UNISWAP_SWAP_ROUTER_BASE),
                            };
                            let action = match self.execute_mayan_solana_bridge_on_chain(
                                src.bridge_token_raw, &solana_addr, usdc_addr, "USDC", src.chain_id
                            ).await {
                                Ok(h) => BridgeAction {
                                    src_chain: src.chain_id, dst_chain: 0,
                                    token_symbol: "USDC".into(), amount_usd: src.bridge_token_usd,
                                    kind: BridgeKind::SolanaBootstrap, tx_hash: Some(h),
                                    status: "sent".into(),
                                },
                                Err(e) => {
                                    warn!("Direct USDC bridge from {} failed: {e:#}", chain_name);
                                    BridgeAction {
                                        src_chain: src.chain_id, dst_chain: 0,
                                        token_symbol: "USDC".into(), amount_usd: src.bridge_token_usd,
                                        kind: BridgeKind::SolanaBootstrap, tx_hash: None,
                                        status: format!("error: {e:#}"),
                                    }
                                }
                            };
                            actions.push(action);
                        } else {
                        // Stablecoin exhausted — try ETH→SOL bootstrap if we have spare ETH.
                        // Swap ETH→USDC via Uniswap on the best source chain, then bridge via Mayan.
                        // Bridge enough USDC that gasDrop=0.001 SOL (~$0.09) is economical for Mayan solvers.
                        // 0.0004 ETH ≈ $1.20 USDC — minimum viable for mode=2 Mayan orders.
                        // mode=2 orders are filled by Mayan's registered solver network regardless
                        // of amount; keep 0.0002 ETH for gas on L2.
                        const ETH_BOOTSTRAP_WEI: u128 = 400_000_000_000_000; // 0.0004 ETH
                        const MIN_ETH_KEEP: f64 = 0.0002; // keep for gas
                        // Find the best ETH source: prefer Arb, then Optimism, then Base.
                        let eth_src = {
                            let threshold = MIN_ETH_KEEP + (ETH_BOOTSTRAP_WEI as f64 / 1e18);
                            let arb_snap = snapshots.iter().find(|s| s.chain_id == 42161);
                            let arb_eth = arb_snap.map(|s| s.gas_eth).unwrap_or(0.0);
                            let opt_snap = snapshots.iter().find(|s| s.chain_id == 10);
                            let opt_eth = opt_snap.map(|s| s.gas_eth).unwrap_or(0.0);
                            if arb_eth > threshold {
                                arb_snap.map(|s| (s, "Arbitrum", 42161u64))
                            } else if opt_eth > threshold {
                                opt_snap.map(|s| (s, "Optimism", 10u64))
                            } else {
                                let base_eth = base_snap.gas_eth;
                                if base_eth > threshold {
                                    Some((base_snap, "Base", HOME_CHAIN_ID))
                                } else {
                                    None
                                }
                            }
                        };
                        if let Some((eth_snap, eth_chain_name, eth_chain_id)) = eth_src {
                            info!(
                                "🚀 ETH bootstrap bridge: {} wei ETH {} → Solana {} (sol_lamports={}, stables depleted)",
                                ETH_BOOTSTRAP_WEI, eth_chain_name, &solana_addr[..8.min(solana_addr.len())], solana_lamports
                            );
                            let action = if eth_chain_id == HOME_CHAIN_ID {
                                self.mayan_solana_bridge_eth(eth_snap, &solana_addr, ETH_BOOTSTRAP_WEI).await
                            } else {
                                self.mayan_solana_bridge_eth_from_chain(eth_snap, &solana_addr, ETH_BOOTSTRAP_WEI, eth_chain_id).await
                            };
                            actions.push(action);
                        } else {
                            info!(
                                "⏭ Mayan Solana bridge skipped: Base available=${:.2} (bridge_token={} ${:.2}) < needed, ETH too low on all chains",
                                base_available, bridge_symbol, bridge_usd
                            );
                        }
                        } // closes the `else` of the direct_src check
                    } else {
                        info!(
                            "⏭ Mayan Solana bridge skipped: Base available=${:.2} (bridge_token={} ${:.2}) < ${:.2} needed",
                            base_available, bridge_symbol, bridge_usd, SOLANA_BRIDGE_USD + MIN_BRIDGE_USD,
                        );
                    }
                }
            }
        }

        actions
    }

    async fn gas_topup(
        &self,
        src: &ChainSnapshot,
        dst_target: &InventoryTarget,
        amount_raw: u128,
        amount_usd: f64,
    ) -> BridgeAction {
        let mut action = BridgeAction {
            src_chain: src.chain_id,
            dst_chain: dst_target.chain_id,
            token_symbol: src.bridge_token_addr.clone(),
            amount_usd,
            kind: BridgeKind::GasTopup,
            tx_hash: None,
            status: if self.dry_run { "dry_run".into() } else { "pending".into() },
        };

        if self.dry_run {
            info!(
                "[DRY RUN] Would bridge ${:.2} {} → {} native gas via Across swap",
                amount_usd, src.chain_id, dst_target.chain_name
            );
            return action;
        }

        match self.execute_gas_topup(src, dst_target.chain_id, amount_raw).await {
            Ok(hash) => { action.tx_hash = Some(hash); action.status = "sent".into(); }
            Err(e) => { warn!("Gas top-up failed {}: {e:#}", dst_target.chain_name); action.status = format!("error: {e:#}"); }
        }
        action
    }

    /// Bridge accumulated repayments from a surplus fill chain (or old-repayment src-only chain)
    /// back to the home chain (Base) via Across depositV3.
    async fn claim_sweep(
        &self,
        src: &ChainSnapshot,
        dst_chain: u64,
        amount_raw: u128,
        amount_usd: f64,
    ) -> BridgeAction {
        let mut action = BridgeAction {
            src_chain: src.chain_id,
            dst_chain,
            token_symbol: src.bridge_token_addr.clone(),
            amount_usd,
            kind: BridgeKind::ClaimSweep,
            tx_hash: None,
            status: if self.dry_run { "dry_run".into() } else { "pending".into() },
        };

        if self.dry_run {
            info!(
                "[DRY RUN] Would sweep ${:.2} stables {} → {} (claim sweep)",
                amount_usd, src.chain_id, dst_chain
            );
            return action;
        }

        // Reuse the same Across depositV3 path as stable_fill.
        match self.execute_stable_bridge(src, dst_chain, amount_raw, amount_usd).await {
            Ok(hash) => { action.tx_hash = Some(hash); action.status = "sent".into(); }
            Err(e) => {
                warn!("Claim sweep failed {} → {}: {e:#}", src.chain_id, dst_chain);
                action.status = format!("error: {e:#}");
            }
        }
        action
    }

    async fn stable_fill(
        &self,
        src: &ChainSnapshot,
        dst_target: &InventoryTarget,
        amount_raw: u128,
        amount_usd: f64,
    ) -> BridgeAction {
        let mut action = BridgeAction {
            src_chain: src.chain_id,
            dst_chain: dst_target.chain_id,
            token_symbol: "USDC".into(),
            amount_usd,
            kind: BridgeKind::StableFill,
            tx_hash: None,
            status: if self.dry_run { "dry_run".into() } else { "pending".into() },
        };

        if self.dry_run {
            info!(
                "[DRY RUN] Would bridge ${:.2} stables {} → {} via Across depositV3",
                amount_usd, src.chain_id, dst_target.chain_name
            );
            return action;
        }

        match self.execute_stable_bridge(src, dst_target.chain_id, amount_raw, amount_usd).await {
            Ok(hash) => { action.tx_hash = Some(hash); action.status = "sent".into(); }
            Err(e) => { warn!("Stable bridge failed {} → {}: {e:#}", src.chain_id, dst_target.chain_name); action.status = format!("error: {e:#}"); }
        }
        action
    }

    /// Bridge USDC from Base to Solana via MayanSwift V2 createOrderWithToken (direct, no Forwarder).
    async fn mayan_solana_bridge(
        &self,
        src: &ChainSnapshot,
        solana_addr: &str,
        amount_raw: u128,
        amount_usd: f64,
        token_in_addr: &str,
        token_symbol: &str,
    ) -> BridgeAction {
        // Use chain_id 0 for Solana as a sentinel (Solana has no EVM chain ID).
        let mut action = BridgeAction {
            src_chain: src.chain_id,
            dst_chain: 0, // Solana sentinel
            token_symbol: token_symbol.into(),
            amount_usd,
            kind: BridgeKind::MayanSolana,
            tx_hash: None,
            status: if self.dry_run { "dry_run".into() } else { "pending".into() },
        };

        if self.dry_run {
            info!(
                "[DRY RUN] Would Mayan-bridge ${:.2} {} Base → Solana ({})",
                amount_usd, token_symbol, &solana_addr[..8.min(solana_addr.len())]
            );
            return action;
        }

        match self.execute_mayan_solana_bridge(amount_raw, solana_addr, token_in_addr, token_symbol).await {
            Ok(hash) => { action.tx_hash = Some(hash); action.status = "sent".into(); }
            Err(e) => {
                warn!("Mayan Solana bridge failed: {e:#}");
                action.status = format!("error: {e:#}");
            }
        }
        action
    }

    /// Bridge native ETH from Base to SOL on Solana via MayanSwift V2 createOrder (payable).
    async fn mayan_solana_bridge_eth(
        &self,
        src: &ChainSnapshot,
        solana_addr: &str,
        eth_wei: u128,
    ) -> BridgeAction {
        let eth_usd = eth_wei as f64 / 1e18 * 2500.0; // rough ETH price
        let mut action = BridgeAction {
            src_chain: src.chain_id,
            dst_chain: 0,
            token_symbol: "ETH".into(),
            amount_usd: eth_usd,
            kind: BridgeKind::MayanSolana,
            tx_hash: None,
            status: if self.dry_run { "dry_run".into() } else { "pending".into() },
        };
        if self.dry_run {
            info!("[DRY RUN] Would Mayan-bridge {} wei ETH Base → Solana SOL ({})", eth_wei, &solana_addr[..8.min(solana_addr.len())]);
            return action;
        }
        match self.execute_mayan_solana_bridge_eth(eth_wei, solana_addr).await {
            Ok(hash) => { action.tx_hash = Some(hash); action.status = "sent".into(); }
            Err(e) => { warn!("Mayan ETH→SOL bridge failed: {e:#}"); action.status = format!("error: {e:#}"); }
        }
        action
    }

    /// Bridge ETH from a non-Base chain (e.g. Arbitrum) → USDC via Uniswap → Solana via Mayan.
    async fn mayan_solana_bridge_eth_from_chain(
        &self,
        src: &ChainSnapshot,
        solana_addr: &str,
        eth_wei: u128,
        chain_id: u64,
    ) -> BridgeAction {
        let eth_usd = eth_wei as f64 / 1e18 * 2500.0;
        let mut action = BridgeAction {
            src_chain: chain_id,
            dst_chain: 0,
            token_symbol: "ETH".into(),
            amount_usd: eth_usd,
            kind: BridgeKind::MayanSolana,
            tx_hash: None,
            status: if self.dry_run { "dry_run".into() } else { "pending".into() },
        };
        if self.dry_run {
            info!("[DRY RUN] Would Mayan-bridge {} wei ETH (chain {}) → Solana SOL ({})", eth_wei, chain_id, &solana_addr[..8.min(solana_addr.len())]);
            return action;
        }
        // Determine USDC address and Uniswap router for this chain
        let (usdc_addr, weth_addr, router_addr) = match chain_id {
            42161 => (
                "0xaf88d065e77c8cc2239327c5edb3a432268e5831", // USDC on Arb
                "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1", // WETH on Arb
                "0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45", // SwapRouter02 on Arb
            ),
            10 => (
                "0x0b2C639c533813f4Aa9D7837CAf62653d097Ff85", // USDC on Optimism
                "0x4200000000000000000000000000000000000006", // WETH on Optimism
                "0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45", // SwapRouter02 on Optimism
            ),
            _ => {
                warn!("ETH bootstrap from chain {} not supported", chain_id);
                action.status = "error: unsupported chain for ETH bootstrap".into();
                return action;
            }
        };
        match self.execute_mayan_solana_bridge_eth_chain(eth_wei, solana_addr, chain_id, usdc_addr, weth_addr, router_addr).await {
            Ok(hash) => { action.tx_hash = Some(hash); action.status = "sent".into(); }
            Err(e) => { warn!("Mayan ETH→SOL bridge from chain {} failed: {e:#}", chain_id); action.status = format!("error: {e:#}"); }
        }
        let _ = src; // used for type inference only
        action
    }

    /// ETH→USDC via Uniswap exactInputSingle, then bridge USDC to Solana via createOrderWithToken.
    /// Swift V2 does not support native ETH input (createOrderWithEth), so we swap first.
    async fn execute_mayan_solana_bridge_eth(&self, eth_wei: u128, solana_addr: &str) -> Result<String> {
        self.execute_mayan_solana_bridge_eth_chain(eth_wei, solana_addr, HOME_CHAIN_ID, USDC_BASE, WETH_BASE, UNISWAP_SWAP_ROUTER_BASE).await
    }

    async fn execute_mayan_solana_bridge_eth_chain(
        &self,
        eth_wei: u128,
        solana_addr: &str,
        chain_id: u64,
        usdc_addr: &str,
        weth_addr: &str,
        router_addr: &str,
    ) -> Result<String> {
        let rpc = rpc_for(chain_id);
        let eth_u256 = U256::from(eth_wei);

        // Step 1: wrap ETH → WETH, then swap WETH → USDC via Uniswap exactInputSingle.
        // Uniswap SwapRouter02 accepts ETH directly via exactInputSingle when tokenIn=WETH and value>0.
        let eth_price_usd = 2500.0_f64;
        // USDC has 6 decimals: 1 USDC = 1_000_000 raw. Apply 10% slippage floor.
        let usdc_expected = (eth_wei as f64 / 1e18 * eth_price_usd * 0.90 * 1e6) as u128;
        let min_usdc_out = U256::from(usdc_expected.max(1)); // min 1 raw to avoid div-by-zero

        let swap_params = UniswapSwapRouter::ExactInputSingleParams {
            tokenIn: weth_addr.parse().context("parse WETH")?,
            tokenOut: usdc_addr.parse().context("parse USDC")?,
            fee: alloy::primitives::Uint::<24, 1>::from(500u32), // 0.05% pool
            recipient: self.solver_addr,
            amountIn: eth_u256,
            amountOutMinimum: min_usdc_out,
            sqrtPriceLimitX96: alloy::primitives::Uint::<160, 3>::ZERO,
        };
        let swap_calldata = UniswapSwapRouter::exactInputSingleCall { params: swap_params }.abi_encode();

        info!("📤 Uniswap ETH→USDC (chain {}): {} wei ETH (expect ≥{} USDC raw)", chain_id, eth_wei, usdc_expected);

        // Send ETH with the swap (router accepts ETH and wraps it internally)
        let router: Address = router_addr.parse().context("parse router")?;
        let wallet = EthereumWallet::from(self.signer.clone());
        let wp = ProviderBuilder::new().with_recommended_fillers().wallet(wallet.clone())
            .on_http(rpc.parse().context("parse rpc")?);

        let swap_req = TransactionRequest::default()
            .to(router)
            .input(Bytes::from(swap_calldata).into())
            .value(eth_u256);
        let pending = wp.send_transaction(swap_req).await.context("uniswap swap tx")?;
        let receipt = pending.with_required_confirmations(1).get_receipt().await.context("swap receipt")?;
        if !receipt.status() {
            anyhow::bail!("Uniswap ETH→USDC swap reverted on chain {}", chain_id);
        }
        info!("✅ Uniswap ETH→USDC swap confirmed (chain {})", chain_id);

        // Step 2: read actual USDC balance and bridge entire amount to Solana.
        let usdc_token: Address = usdc_addr.parse().context("parse USDC")?;
        let provider = ProviderBuilder::new().on_http(rpc.parse().context("parse rpc")?);
        // balanceOf selector: 0x70a08231
        let bal_calldata = {
            let mut d = vec![0x70u8, 0xa0u8, 0x82u8, 0x31u8];
            d.extend_from_slice(&[0u8; 12]);
            d.extend_from_slice(self.solver_addr.as_slice());
            d
        };
        let req = TransactionRequest::default()
            .to(usdc_token)
            .input(Bytes::from(bal_calldata).into());
        let bal_raw: U256 = match provider.call(&req).await {
            Ok(b) if b.len() >= 32 => U256::from_be_slice(&b[b.len()-32..]),
            _ => U256::from(usdc_expected),
        };
        // Bridge at most usdc_expected (the new amount), leave existing balance intact.
        let bridge_amount = bal_raw.min(U256::from((usdc_expected as u128).saturating_add(1_000_000)));
        if bridge_amount.is_zero() {
            anyhow::bail!("USDC balance is zero after swap on chain {}", chain_id);
        }
        info!("💰 USDC balance after swap: {} raw, bridging {} raw to Solana (via chain {})", bal_raw, bridge_amount, chain_id);

        // Bridge from the source chain's MayanSwift (same address on all chains)
        self.execute_mayan_solana_bridge_on_chain(bridge_amount.to::<u128>(), solana_addr, usdc_addr, "USDC", chain_id).await
    }

    /// Direct path: approve tokenIn to Swift V2, call createOrderWithToken.
    /// gasDrop=0.01 SOL (10_000_000 lamports) airdropped to Solana recipient.
    /// auctionMode=2 → auction mode, Mayan solvers compete to fill on Solana.
    async fn execute_mayan_solana_bridge(&self, amount_raw: u128, solana_addr: &str, token_in_addr: &str, token_symbol: &str) -> Result<String> {
        self.execute_mayan_solana_bridge_on_chain(amount_raw, solana_addr, token_in_addr, token_symbol, HOME_CHAIN_ID).await
    }

    async fn execute_mayan_solana_bridge_on_chain(&self, amount_raw: u128, solana_addr: &str, token_in_addr: &str, token_symbol: &str, chain_id: u64) -> Result<String> {
        let rpc = rpc_for(chain_id);
        let amount_u256 = U256::from(amount_raw);

        // Decode Solana base58 pubkey → 32-byte destAddr
        let sol_bytes = bs58::decode(solana_addr)
            .into_vec()
            .context("invalid Solana base58 address")?;
        if sol_bytes.len() != 32 {
            anyhow::bail!("Solana address must be 32 bytes, got {}", sol_bytes.len());
        }
        let mut dest_arr = [0u8; 32];
        dest_arr.copy_from_slice(&sol_bytes);
        let dest_addr_fixed: alloy::primitives::FixedBytes<32> = dest_arr.into();

        // Trader = solver EVM address, left-padded to bytes32
        let trader_fixed: alloy::primitives::FixedBytes<32> = {
            let mut b = [0u8; 32];
            b[12..].copy_from_slice(self.solver_addr.as_slice());
            b.into()
        };

        // tokenOut = native SOL (So11111111111111111111111111111111111111112) as bytes32
        // We request native SOL output so the Solana wallet receives lamports directly.
        // gasDrop=0 because we're receiving SOL directly (no extra gas drop needed).
        let token_out_fixed: alloy::primitives::FixedBytes<32> = {
            let bytes = bs58::decode("So11111111111111111111111111111111111111112")
                .into_vec()
                .unwrap_or_default();
            let mut arr = [0u8; 32];
            if bytes.len() == 32 { arr.copy_from_slice(&bytes); }
            arr.into()
        };

        let zero_fixed: alloy::primitives::FixedBytes<32> = [0u8; 32].into();

        // random = timestamp-seeded bytes32 for order uniqueness
        let random_fixed: alloy::primitives::FixedBytes<32> = {
            use std::time::{SystemTime, UNIX_EPOCH};
            let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
            let mut r = [0u8; 32];
            r[16..24].copy_from_slice(&((ts >> 64) as u64).to_be_bytes());
            r[24..].copy_from_slice(&(ts as u64).to_be_bytes());
            r.into()
        };

        let deadline = {
            use std::time::{SystemTime, UNIX_EPOCH};
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() + 14400 // 4 hours
        };

        // minAmountOut in SOL lamports (9 decimals): 85% of input USDC value converted to SOL.
        // Lower slippage tolerance (85%) gives Mayan solvers more margin to fill without reverting.
        // SOL price ~$170 as of May 2026; using conservative $160 to avoid order rejection on dips.
        let sol_price_usd = 160.0_f64;
        let usdc_usd = amount_raw as f64 / 1e6;
        let min_sol_lamports = (usdc_usd / sol_price_usd * 0.85 * 1e9) as u64;

        let token_in: Address = token_in_addr.parse().context("parse tokenIn addr")?;
        let forwarder: Address = MAYAN_FORWARDER.parse().context("parse forwarder addr")?;
        let forwarder_hex = format!("{forwarder:#x}");
        let mayan_swift: Address = MAYAN_SWIFT.parse().context("parse MayanSwift addr")?;

        // Approve Forwarder to pull USDC from solver — Forwarder re-approves MayanSwift internally.
        self.ensure_allowance(token_in_addr, &forwarder_hex, amount_u256, rpc)
            .await.with_context(|| format!("approve MayanForwarder for {token_symbol}"))?;

        // Inner call: MayanSwift createOrderWithToken — encoded as protocolData for the Forwarder.
        // auctionMode=2: all real EVM→Solana orders in the wild use mode=2. mode=0 does NOT
        // cause Mayan's relay to initialize a Solana state account, so the order sits permanently
        // unfilled. With mode=2, Mayan's registered solver network picks it up via their private
        // auction and delivers SOL to the dest address.
        let order = MayanSwiftCreate::Order {
            payloadType: 1,
            trader: trader_fixed,
            destAddr: dest_addr_fixed,
            destChainId: MAYAN_SOLANA_CHAIN_ID,
            referrerAddr: zero_fixed,
            tokenOut: token_out_fixed,
            minAmountOut: min_sol_lamports,
            gasDrop: 0u64,
            cancelFee: 2000,
            refundFee: 0,
            deadline,
            referrerBps: 0,
            auctionMode: 2,
            random: random_fixed,
        };

        let protocol_data = MayanSwiftCreate::createOrderWithTokenCall {
            tokenIn: token_in,
            amountIn: amount_u256,
            order,
            customPayload: Bytes::new(),
        }.abi_encode();

        // Outer call: Forwarder.forwardERC20 — pulls USDC, calls MayanSwift, emits Wormhole VAA.
        let empty_permit = MayanForwarder::PermitParams {
            value: U256::ZERO,
            deadline: U256::ZERO,
            v: 0u8,
            r: [0u8; 32].into(),
            s: [0u8; 32].into(),
        };
        let forward_calldata = MayanForwarder::forwardERC20Call {
            tokenIn: token_in,
            amountIn: amount_u256,
            permitParams: empty_permit,
            mayanProtocol: mayan_swift,
            protocolData: Bytes::from(protocol_data),
        }.abi_encode();

        info!(
            "📤 MayanForwarder forwardERC20 (chain {}): {} {} raw → native SOL on {}, min={} lamports (auctionMode=2)",
            chain_id, amount_raw, token_symbol, &solana_addr[..8.min(solana_addr.len())], min_sol_lamports
        );

        self.send_raw(rpc, &forwarder_hex, &hex::encode(&forward_calldata)).await
    }

    async fn execute_gas_topup(&self, src: &ChainSnapshot, dst_chain: u64, amount_raw: u128) -> Result<String> {
        let url = format!(
            "https://app.across.to/api/swap?originChainId={}&destinationChainId={}&inputToken={}&outputToken=0x0000000000000000000000000000000000000000&amount={}&swapSlippage=0.01&depositor={:#x}",
            src.chain_id, dst_chain, src.bridge_token_addr, amount_raw, self.solver_addr
        );
        let resp: SwapResp = self.http.get(&url).send().await
            .context("Across swap API")?
            .json().await
            .context("parse swap response")?;

        let src_rpc = rpc_for(src.chain_id);

        // Send approval txns verbatim from the Across swap API response.
        // The API already encodes the correct approve(spender, amount) calldata.
        for appr in &resp.approval_txns {
            if appr.chain_id == src.chain_id {
                self.send_raw(src_rpc, &appr.to, &appr.data)
                    .await.context("approval for gas top-up")?;
            }
        }

        self.send_raw(src_rpc, &resp.swap_tx.to, &resp.swap_tx.data).await
    }

    /// Try to bridge stables via Across first, fall back to deBridge if the
    /// Across **quote** errors out or returns zero/insufficient liquidity.
    /// Post-quote failures (approval revert, RPC drop, etc.) are NOT retried
    /// on deBridge because the same on-chain conditions would likely block
    /// either path.
    /// Logs `bridge_selected` with the reason on every successful selection.
    async fn execute_stable_bridge(&self, src: &ChainSnapshot, dst_chain: u64, amount_raw: u128, amount_usd: f64) -> Result<String> {
        let src_token: Address = src.bridge_token_addr.parse().context("parse src token addr")?;
        match self.fetch_across_quote(src, src_token, dst_chain, amount_raw).await {
            Ok(quote) => {
                info!(
                    "bridge_selected src={} dst={} bridge=across reason=primary amount_usd={:.2}",
                    src.chain_id, dst_chain, amount_usd
                );
                self.execute_across_deposit(src, src_token, dst_chain, amount_raw, quote).await
            }
            Err(quote_err) if debridge_supports(src.chain_id, dst_chain) => {
                info!(
                    "bridge_selected src={} dst={} bridge=debridge reason=across_quote_failed amount_usd={:.2} ({})",
                    src.chain_id, dst_chain, amount_usd, quote_err
                );
                self.execute_debridge_stable_bridge(src, dst_chain, amount_raw)
                    .await
                    .with_context(|| format!("debridge fallback {} → {}", src.chain_id, dst_chain))
            }
            Err(quote_err) => {
                anyhow::bail!(
                    "Across quote failed and deBridge fallback not supported on this path ({} → {}): {quote_err}",
                    src.chain_id, dst_chain
                );
            }
        }
    }

    /// Fetch an Across V3 suggested-fees quote. Returns the parsed `FeesResp`
    /// only when the response is usable (non-zero outputAmount).
    async fn fetch_across_quote(&self, src: &ChainSnapshot, src_token: Address, dst_chain: u64, amount_raw: u128) -> Result<FeesResp, AcrossQuoteIssue> {
        let fee_url = format!(
            "https://app.across.to/api/suggested-fees?originChainId={}&destinationChainId={}&token={:#x}&amount={}",
            src.chain_id, dst_chain, src_token, amount_raw
        );
        let fees_resp = self.http.get(&fee_url).send().await
            .map_err(|e| AcrossQuoteIssue::NoLiquidity(format!("suggested-fees request: {e}")))?;
        if !fees_resp.status().is_success() {
            let status = fees_resp.status();
            let body = fees_resp.text().await.unwrap_or_default();
            return Err(AcrossQuoteIssue::NoLiquidity(format!("HTTP {status}: {body}")));
        }
        let fees: FeesResp = fees_resp.json().await
            .map_err(|e| AcrossQuoteIssue::NoLiquidity(format!("parse fees response: {e}")))?;

        let output_amount: U256 = fees.output_amount.parse().unwrap_or(U256::ZERO);
        if output_amount.is_zero() {
            return Err(AcrossQuoteIssue::NoLiquidity(
                "outputAmount=0 (insufficient liquidity)".into()
            ));
        }
        if spoke_pool(src.chain_id).is_none() {
            return Err(AcrossQuoteIssue::NoLiquidity(format!("No SpokePool for chain {}", src.chain_id)));
        }
        Ok(fees)
    }

    /// Send the Across V3 depositV3 transaction with a previously-fetched quote.
    /// Errors here (approval, RPC, revert) are hard failures — we don't retry
    /// on deBridge, since approval revert / RPC outage would block both paths.
    async fn execute_across_deposit(&self, src: &ChainSnapshot, src_token: Address, dst_chain: u64, amount_raw: u128, fees: FeesResp) -> Result<String> {
        let src_rpc = rpc_for(src.chain_id);
        let amount_u256 = U256::from(amount_raw);
        let output_amount: U256 = fees.output_amount.parse().unwrap_or(U256::ZERO);
        let output_token: Address = fees.output_token.address.parse().unwrap_or(Address::ZERO);
        let exclusive_relayer: Address = fees.exclusive_relayer.parse().unwrap_or(Address::ZERO);
        let quote_ts: u32 = fees.timestamp.parse().unwrap_or(0);

        let spoke = spoke_pool(src.chain_id)
            .ok_or_else(|| anyhow::anyhow!("No SpokePool for chain {}", src.chain_id))?;

        self.ensure_allowance(
            &src.bridge_token_addr,
            &format!("{:#x}", spoke),
            amount_u256,
            src_rpc,
        ).await.context("approval for stable bridge")?;

        let calldata = depositV3Call {
            depositor: self.solver_addr, recipient: self.solver_addr,
            inputToken: src_token, outputToken: output_token,
            inputAmount: amount_u256, outputAmount: output_amount,
            destinationChainId: U256::from(dst_chain),
            exclusiveRelayer: exclusive_relayer,
            quoteTimestamp: quote_ts,
            fillDeadline: fees.fill_deadline,
            exclusivityDeadline: fees.exclusivity_deadline,
            message: Bytes::new(),
        }.abi_encode();

        self.send_raw(src_rpc, &format!("{:#x}", spoke), &hex::encode(&calldata)).await
    }

    /// deBridge DLN fallback for stable transfers. Uses the public
    /// `https://dln.debridge.finance/v1.0/dln/order/create-tx` API to fetch
    /// ready-to-send calldata for the source-chain DlnSource contract,
    /// then approves + broadcasts via the same `ensure_allowance` /
    /// `send_raw` primitives the Across path uses.
    ///
    /// Approve target = the `tx.to` address returned by the API (DlnSource).
    /// The `recipient` is encoded inside the API call as `dstChainTokenOutRecipient`,
    /// set to our solver address — symmetric with the Across `recipient` field.
    async fn execute_debridge_stable_bridge(&self, src: &ChainSnapshot, dst_chain: u64, amount_raw: u128) -> Result<String> {
        let src_token: Address = src.bridge_token_addr.parse().context("parse src token addr")?;
        let src_rpc = rpc_for(src.chain_id);
        let amount_u256 = U256::from(amount_raw);

        // Destination USDC for each supported path. Mirrors the brief's
        // "supported paths: Arbitrum→Base, Optimism→Base, Ethereum→Base".
        let dst_usdc = debridge_dst_usdc(dst_chain)
            .ok_or_else(|| anyhow::anyhow!("deBridge: no destination USDC mapping for chain {}", dst_chain))?;

        let solver_hex = format!("{:#x}", self.solver_addr);
        let url = format!(
            "https://dln.debridge.finance/v1.0/dln/order/create-tx\
             ?srcChainId={src_chain}&srcChainTokenIn={src_token:#x}&srcChainTokenInAmount={amount_raw}\
             &dstChainId={dst_chain}&dstChainTokenOut={dst_usdc}&dstChainTokenOutAmount=auto\
             &dstChainTokenOutRecipient={solver_hex}\
             &senderAddress={solver_hex}&srcChainOrderAuthorityAddress={solver_hex}\
             &dstChainOrderAuthorityAddress={solver_hex}\
             &prependOperatingExpenses=true",
            src_chain = src.chain_id,
        );
        let resp = self.http.get(&url).send().await
            .context("deBridge create-tx request")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("deBridge create-tx HTTP {status}: {body}");
        }
        let create: DlnCreateTxResp = resp.json().await
            .context("parse deBridge create-tx response")?;

        if let Some(est) = &create.estimation {
            if let Some(out) = &est.dst_chain_token_out {
                if let Some(amount) = &out.amount {
                    info!("debridge dst_chain_token_out_amount={} (chain {} → {})", amount, src.chain_id, dst_chain);
                }
            }
        }

        // Approve DlnSource to pull our token. The `to` address comes from the
        // API response (DlnSource is the same on every chain); approving it as
        // the spender is symmetric with how we approve the Across SpokePool.
        self.ensure_allowance(&src.bridge_token_addr, &create.tx.to, amount_u256, src_rpc)
            .await.context("approval for deBridge bridge")?;

        let value = create.tx.value
            .as_ref()
            .and_then(|v| {
                let s = v.trim_start_matches("0x");
                if v.starts_with("0x") { U256::from_str_radix(s, 16).ok() } else { v.parse().ok() }
            })
            .unwrap_or(U256::ZERO);
        let data = create.tx.data.trim_start_matches("0x");
        self.send_raw_value(src_rpc, &create.tx.to, data, value).await
    }

    async fn ensure_allowance(&self, token: &str, spender: &str, amount: U256, rpc: &str) -> Result<()> {
        let token_addr: Address = token.parse().context("parse token")?;
        let spender_addr: Address = spender.parse().context("parse spender")?;

        let provider = ProviderBuilder::new().on_http(rpc.parse().context("parse rpc")?);
        let call = allowanceCall { owner: self.solver_addr, spender: spender_addr }.abi_encode();
        let req = TransactionRequest::default().to(token_addr).input(Bytes::from(call).into());
        let existing: U256 = match provider.call(&req).await {
            Ok(b) if b.len() >= 32 => U256::from_be_slice(&b[b.len() - 32..]),
            _ => U256::ZERO,
        };
        if existing >= amount { return Ok(()); }

        let approve = approveCall { spender: spender_addr, amount: U256::MAX }.abi_encode();

        // Guard: approve tx goes to a token contract — must be in the known token list.
        TxGuard::from_deployments(self.solver_addr)
            .enforce(token_addr, &approve, &[])
            .context("tx_guard blocked ensure_allowance")?;

        let wallet = EthereumWallet::from(self.signer.clone());
        let wp = ProviderBuilder::new().with_recommended_fillers().wallet(wallet)
            .on_http(rpc.parse().context("parse rpc")?);
        let req = TransactionRequest::default().to(token_addr).input(Bytes::from(approve).into());
        let pending = wp.send_transaction(req).await.context("approve tx")?;
        pending.with_required_confirmations(1).get_receipt().await.context("approve receipt")?;
        Ok(())
    }

    async fn send_raw(&self, rpc: &str, to: &str, data: &str) -> Result<String> {
        self.send_raw_value(rpc, to, data, U256::ZERO).await
    }

    async fn send_raw_value(&self, rpc: &str, to: &str, data: &str, value: U256) -> Result<String> {
        let to_addr: Address = to.parse().context("parse to")?;
        let bytes = hex::decode(data.trim_start_matches("0x")).context("decode calldata")?;

        // ── Pre-flight guard ─────────────────────────────────────────────────
        // Block any tx whose destination is not a known LWC well, bridge contract,
        // swap router, WETH, or the solver's own address.
        // `recipient`/`depositor` fields must be solver_addr — enforced in callers
        // that pass embedded_recipients; here we check only the `to` address.
        TxGuard::from_deployments(self.solver_addr)
            .enforce(to_addr, &bytes, &[])
            .context("tx_guard blocked send_raw_value")?;
        // ────────────────────────────────────────────────────────────────────

        let wallet = EthereumWallet::from(self.signer.clone());
        let provider = ProviderBuilder::new().with_recommended_fillers().wallet(wallet)
            .on_http(rpc.parse().context("parse rpc")?);
        let mut req = TransactionRequest::default().to(to_addr).input(Bytes::from(bytes).into());
        if value > U256::ZERO { req = req.value(value); }
        let pending = provider.send_transaction(req).await.context("send tx")?;
        let hash = format!("{:#x}", pending.tx_hash());
        let receipt = pending.with_required_confirmations(1).get_receipt().await.context("receipt")?;
        if receipt.status() { Ok(hash) } else { anyhow::bail!("reverted: {}", hash) }
    }

    async fn unwrap_weth_on_base(&self, weth_raw: u128) -> BridgeAction {
        self.unwrap_weth_on_chain(HOME_CHAIN_ID, WETH_BASE, weth_raw).await
    }

    async fn unwrap_weth_on_chain(&self, chain_id: u64, weth_addr: &str, weth_raw: u128) -> BridgeAction {
        let mut action = BridgeAction {
            src_chain: chain_id, dst_chain: chain_id,
            token_symbol: "WETH".into(), amount_usd: weth_raw as f64 / 1e18 * 3000.0,
            kind: BridgeKind::EthBootstrap, tx_hash: None,
            status: if self.dry_run { "dry_run".into() } else { "pending".into() },
        };
        if self.dry_run {
            info!("[DRY RUN] Would unwrap {} WETH → ETH on chain {}", weth_raw as f64 / 1e18, chain_id);
            return action;
        }
        let rpc = rpc_for(chain_id);
        let calldata = WETH::withdrawCall { wad: U256::from(weth_raw) }.abi_encode();
        match self.send_raw(rpc, weth_addr, &hex::encode(&calldata)).await {
            Ok(h) => { action.tx_hash = Some(h); action.status = "sent".into(); }
            Err(e) => { warn!("WETH unwrap chain {} failed: {e:#}", chain_id); action.status = format!("error: {e:#}"); }
        }
        action
    }

    async fn usdt_to_eth_swap(&self, usdt_raw: u128) -> BridgeAction {
        let mut action = BridgeAction {
            src_chain: 8453,
            dst_chain: 8453,
            token_symbol: "USDT".into(),
            amount_usd: usdt_raw as f64 / 1e6,
            kind: BridgeKind::EthBootstrap,
            tx_hash: None,
            status: if self.dry_run { "dry_run".into() } else { "pending".into() },
        };

        if self.dry_run {
            info!("[DRY RUN] Would swap {} USDT raw → ETH on Base via Uniswap V3 multicall", usdt_raw);
            return action;
        }

        match self.execute_usdt_to_eth_swap(usdt_raw).await {
            Ok(hash) => { action.tx_hash = Some(hash); action.status = "sent".into(); }
            Err(e) => { warn!("USDT→ETH swap failed: {e:#}"); action.status = format!("error: {e:#}"); }
        }
        action
    }

    async fn execute_usdt_to_eth_swap(&self, usdt_raw: u128) -> Result<String> {
        let rpc = rpc_for(8453);
        let router: Address = UNISWAP_SWAP_ROUTER_BASE.parse().context("parse router")?;
        let amount_u256 = U256::from(usdt_raw);

        self.ensure_allowance(USDT_BASE, UNISWAP_SWAP_ROUTER_BASE, amount_u256, rpc)
            .await.context("approve Uniswap router for USDT")?;

        // min WETH out: convert USDT (6 dec) → WETH (18 dec) at $3000/ETH, 90% floor
        let min_out = (usdt_raw as f64 / 1e6 / 3000.0 * 0.90 * 1e18) as u128;

        let swap_params = UniswapSwapRouter::ExactInputSingleParams {
            tokenIn: USDT_BASE.parse().context("parse USDT")?,
            tokenOut: WETH_BASE.parse().context("parse WETH")?,
            fee: alloy::primitives::Uint::<24, 1>::from(3000u32),
            recipient: router, // router holds WETH so unwrapWETH9 can pull it
            amountIn: amount_u256,
            amountOutMinimum: U256::from(min_out),
            sqrtPriceLimitX96: alloy::primitives::Uint::<160, 3>::ZERO,
        };
        let swap_call = UniswapSwapRouter::exactInputSingleCall { params: swap_params }.abi_encode();

        let unwrap_call = UniswapSwapRouter::unwrapWETH9Call {
            amountMinimum: U256::ZERO,
            recipient: self.solver_addr,
        }.abi_encode();

        let multicall_calldata = UniswapSwapRouter::multicallCall {
            data: vec![Bytes::from(swap_call), Bytes::from(unwrap_call)],
        }.abi_encode();

        info!(
            "📤 Uniswap multicall: {} USDT raw → ETH on Base (min_out={} wei)",
            usdt_raw, min_out
        );

        self.send_raw(rpc, UNISWAP_SWAP_ROUTER_BASE, &hex::encode(multicall_calldata)).await
    }

    /// Sweep native gas token (e.g. MATIC) from a src-only chain to USDC on Base
    /// via the Across swap API (inputToken=native, outputToken=USDC on Base).
    /// The swap tx attaches native value — no ERC-20 approval needed.
    async fn execute_native_sweep(&self, src_chain: u64, native_wei: u128, dst_chain: u64) -> Result<String> {
        let rpc = rpc_for(src_chain);
        if rpc.is_empty() { anyhow::bail!("no RPC for chain {}", src_chain); }

        // USDC on Base
        let output_token = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";
        let url = format!(
            "https://app.across.to/api/swap?originChainId={}&destinationChainId={}\
             &inputToken=0x0000000000000000000000000000000000000000\
             &outputToken={}&amount={}&slippageTolerance=1\
             &depositor={:#x}&recipient={:#x}",
            src_chain, dst_chain, output_token, native_wei,
            self.solver_addr, self.solver_addr
        );
        let resp: SwapResp = self.http.get(&url).send().await
            .context("Across native-sweep swap API")?
            .json().await
            .context("parse native-sweep swap response")?;

        // Native input — no approval txns expected, but handle defensively
        for appr in &resp.approval_txns {
            if appr.chain_id == src_chain && !appr.data.is_empty() {
                self.send_raw(rpc, &appr.to, &appr.data).await.context("native sweep approval")?;
            }
        }

        let value = U256::from(native_wei);
        self.send_raw_value(rpc, &resp.swap_tx.to, &resp.swap_tx.data, value).await
    }
}

/// Pick the chain with the most spare stable capital that can afford to send
/// `needed_usd`. Fill chains must stay above their own min_stable_usd; src-only
/// chains have no fill reserve requirement — any balance >= needed_usd qualifies.
fn best_surplus_source(
    working_stable: &std::collections::HashMap<u64, f64>,
    targets: &[InventoryTarget],
    exclude_chain: u64,
    needed_usd: f64,
) -> Option<u64> {
    targets.iter()
        .filter(|t| t.chain_id != exclude_chain)
        .filter_map(|t| {
            let bal = working_stable.get(&t.chain_id).copied().unwrap_or(0.0);
            // Fill chains must keep their own reserve; src-only chains have no fill reserve.
            let reserve = if t.is_fill_chain { t.min_stable_usd } else { 0.0 };
            let spare = bal - reserve - needed_usd;
            if spare >= 0.0 { Some((t.chain_id, spare)) } else { None }
        })
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(id, _)| id)
}

fn usd_to_raw(usd: f64, decimals: u32) -> u128 {
    if usd <= 0.0 { return 0; }
    let raw = usd * 10f64.powi(decimals as i32);
    if raw > u128::MAX as f64 { return 0; }
    raw as u128
}

/// Whether the deBridge fallback supports a given (src, dst) lane.
/// Per the rebalancer brief: Arbitrum→Base, Optimism→Base, Ethereum→Base
/// (same lanes Across already covers).
fn debridge_supports(src_chain: u64, dst_chain: u64) -> bool {
    // deBridge DLN supports all EVM↔EVM routes. We whitelist only the paths
    // where we have both chain wiring and a known dst USDC address.
    matches!(
        (src_chain, dst_chain),
        // → Base
        (42161, 8453) | (10, 8453) | (1, 8453) | (137, 8453) |
        // → Arbitrum
        (8453, 42161) | (10, 42161) | (1, 42161) | (137, 42161) |
        // → Optimism
        (8453, 10) | (42161, 10) | (1, 10) | (137, 10)
    )
}

/// Canonical destination USDC address for each deBridge fallback path.
/// Returned as a 0x-prefixed lowercase hex string for direct interpolation
/// into the DLN API URL.
fn debridge_dst_usdc(dst_chain: u64) -> Option<&'static str> {
    match dst_chain {
        8453  => Some("0x833589fcd6edb6e08f4c7c32d4f71b54bda02913"), // USDC on Base
        42161 => Some("0xaf88d065e77c8cc2239327c5edb3a432268e5831"), // USDC native on Arbitrum
        10    => Some("0x0b2c639c533813f4aa9d7837caf62653d097ff85"), // USDC native on Optimism
        _ => None,
    }
}

/// Query the Solana mainnet RPC for the lamport balance of a Solana address (base58).
/// Returns 0 on any error.
async fn check_solana_balance(address: &str) -> u64 {
    let solana_rpc = std::env::var("SOLANA_RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();
    let body = serde_json::json!({
        "jsonrpc": "2.0", "id": 1,
        "method": "getBalance",
        "params": [address]
    });
    let Ok(resp) = client.post(&solana_rpc).json(&body).send().await else { return 0 };
    let Ok(parsed) = resp.json::<serde_json::Value>().await else { return 0 };
    parsed.pointer("/result/value").and_then(|v| v.as_u64()).unwrap_or(0)
}
