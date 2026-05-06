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
use tracing::{info, warn};

use crate::{
    inventory::{InventoryStatus, InventoryTarget},
    scanner::ChainSnapshot,
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
}

/// Chain ID of the canonical home chain — surplus repayments consolidate here.
pub const HOME_CHAIN_ID: u64 = 8453; // Base

/// Across suggested-fees response (fields we actually use).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FeesResp {
    relay_fee_total: String,
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
    #[serde(rename = "expectedOutputAmount")]
    expected_output_amount: Option<String>,
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
            let src_snap = snapshots.iter().find(|s| s.chain_id == src_chain_id).unwrap();

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
            let src_snap = snapshots.iter().find(|s| s.chain_id == src_chain_id).unwrap();
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

    async fn execute_stable_bridge(&self, src: &ChainSnapshot, dst_chain: u64, amount_raw: u128, _amount_usd: f64) -> Result<String> {
        let src_token: Address = src.bridge_token_addr.parse()
            .context("parse src token addr")?;
        let src_rpc = rpc_for(src.chain_id);
        let amount_u256 = U256::from(amount_raw);

        // Fetch Across suggested fees.
        let fee_url = format!(
            "https://app.across.to/api/suggested-fees?originChainId={}&destinationChainId={}&token={:#x}&amount={}",
            src.chain_id, dst_chain, src_token, amount_raw
        );
        let fees: FeesResp = self.http.get(&fee_url).send().await
            .context("Across suggested-fees")?
            .json().await
            .context("parse fees response")?;

        let output_amount: U256 = fees.output_amount.parse().unwrap_or(U256::ZERO);
        let output_token: Address = fees.output_token.address.parse().unwrap_or(Address::ZERO);
        let exclusive_relayer: Address = fees.exclusive_relayer.parse().unwrap_or(Address::ZERO);
        let quote_ts: u32 = fees.timestamp.parse().unwrap_or(0);

        let spoke = spoke_pool(src.chain_id)
            .ok_or_else(|| anyhow::anyhow!("No SpokePool for chain {}", src.chain_id))?;

        // Approve SpokePool.
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

        let wallet = EthereumWallet::from(self.signer.clone());
        let wp = ProviderBuilder::new().with_recommended_fillers().wallet(wallet)
            .on_http(rpc.parse().context("parse rpc")?);
        let approve = approveCall { spender: spender_addr, amount: U256::MAX }.abi_encode();
        let req = TransactionRequest::default().to(token_addr).input(Bytes::from(approve).into());
        let pending = wp.send_transaction(req).await.context("approve tx")?;
        pending.with_required_confirmations(1).get_receipt().await.context("approve receipt")?;
        Ok(())
    }

    async fn send_raw(&self, rpc: &str, to: &str, data: &str) -> Result<String> {
        self.send_raw_value(rpc, to, data, U256::ZERO).await
    }

    async fn send_raw_value(&self, rpc: &str, to: &str, data: &str, value: U256) -> Result<String> {
        let wallet = EthereumWallet::from(self.signer.clone());
        let provider = ProviderBuilder::new().with_recommended_fillers().wallet(wallet)
            .on_http(rpc.parse().context("parse rpc")?);
        let to_addr: Address = to.parse().context("parse to")?;
        let bytes = hex::decode(data.trim_start_matches("0x")).context("decode calldata")?;
        let mut req = TransactionRequest::default().to(to_addr).input(Bytes::from(bytes).into());
        if value > U256::ZERO { req = req.value(value); }
        let pending = provider.send_transaction(req).await.context("send tx")?;
        let hash = format!("{:#x}", pending.tx_hash());
        let receipt = pending.with_required_confirmations(1).get_receipt().await.context("receipt")?;
        if receipt.status() { Ok(hash) } else { anyhow::bail!("reverted: {}", hash) }
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
    (usd * 10f64.powi(decimals as i32)) as u128
}
