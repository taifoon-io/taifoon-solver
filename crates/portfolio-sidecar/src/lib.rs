//! portfolio-sidecar — proactive cross-chain inventory manager for Taifoon solver.
//!
//! ## Problem
//!
//! The Taifoon solver fills Across V3 intents on dst chains (Base, Arbitrum, Optimism).
//! Each fill spends the solver's stablecoins on the dst chain and the solver is repaid
//! there (via Across repaymentChainId = dst_chain). If a dst chain runs out of either:
//!   (a) stablecoins — the executor skips fills (`reserve_failed` / `insufficient_balance`)
//!   (b) native gas  — the broadcast tx fails
//! then the solver misses revenue even though profitable intents are flowing.
//!
//! ## Solution
//!
//! The sidecar runs every `interval` seconds alongside the solver. Each tick it:
//!   1. Scans live balances across all 8 chains (Base, Arbitrum, Optimism, Ethereum,
//!      Polygon, zkSync, Linea, Scroll).
//!   2. Classifies each fill chain as HEALTHY / LOW_GAS / LOW_FUNDS / CRITICAL / SURPLUS.
//!   3. Sends the minimum set of Across bridge intents to restore healthy positions:
//!      - Gas top-up: Across /api/swap → native token on dst chain.
//!      - Stable fill: Across depositV3 → USDC on dst chain.
//!   4. Logs every decision as a structured JSON record.
//!   5. Exposes the latest snapshot and action history via `SidecarState` (for API).
//!
//! ## Data flow
//!
//! ```text
//!  [Across fills on Base/Arb/OP]
//!        │  solver spends tokens
//!        ▼
//!  [dst chain balances drop]
//!        │  sidecar detects LOW_FUNDS
//!        ▼
//!  [sidecar picks surplus chain]    ← repayments accumulate here
//!        │  Across depositV3
//!        ▼
//!  [dst chain topped up → HEALTHY]
//!        │  solver can fill again
//!        ▼
//!  [claim command]  ← runs separately, consolidates excess back to Base
//! ```

pub mod inventory;
pub mod lwc_manager;
pub mod rebalancer;
pub mod scanner;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

use inventory::{classify_lwc, default_targets, InventoryStatus, InventoryTarget, LwcStatus, LWC_LOW_POOL_THRESHOLD_USD};
use lwc_manager::{LwcChainState, LwcManager};
use rebalancer::{BridgeAction, Rebalancer};
use scanner::{scan_all, ChainSnapshot};

// ── Public state (shared with API) ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SidecarState {
    pub last_scan: Option<DateTime<Utc>>,
    pub snapshots: Vec<ChainSnapshot>,
    pub classified: Vec<ClassifiedChain>,
    pub pending_actions: Vec<BridgeAction>,
    pub action_log: Vec<ActionLogEntry>,
    pub cycle: u64,
    /// LWC well states across all deployed chains (populated when T3RN_LWC_ENABLED=true).
    pub lwc_states: Vec<LwcChainState>,
    /// Per-chain LWC classification (parallel to lwc_states).
    pub lwc_classified: Vec<ClassifiedLwcChain>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifiedChain {
    pub chain_id: u64,
    pub chain_name: String,
    pub status: InventoryStatus,
    pub stable_usd: f64,
    pub gas_eth: f64,
    pub min_stable_usd: f64,
    pub min_gas_eth: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifiedLwcChain {
    pub chain_id: u64,
    pub chain_key: String,
    pub lwc_status: LwcStatus,
    pub pool_available_usd: f64,
    pub pool_total_usd: f64,
    pub lp_balance_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionLogEntry {
    pub ts: DateTime<Utc>,
    pub cycle: u64,
    pub action: BridgeAction,
}

pub type SharedState = Arc<RwLock<SidecarState>>;

// ── Sidecar ──────────────────────────────────────────────────────────────────

pub struct PortfolioSidecar {
    rebalancer: Rebalancer,
    targets: Vec<InventoryTarget>,
    solver_addr: alloy::primitives::Address,
    pub state: SharedState,
    /// Optional LWC manager — present when T3RN_LWC_ENABLED=true.
    lwc_manager: Option<LwcManager>,
}

impl PortfolioSidecar {
    /// Construct from a hex private key string (with or without 0x prefix).
    pub fn from_key(private_key: &str, dry_run: bool) -> anyhow::Result<Self> {
        use alloy::signers::local::PrivateKeySigner;
        use anyhow::Context as _;
        let pk = private_key.trim().trim_start_matches("0x");
        let signer: PrivateKeySigner = pk.parse().context("invalid private key")?;
        let solver_addr = signer.address();

        let lwc_enabled = std::env::var("T3RN_LWC_ENABLED")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

        let lwc_manager = if lwc_enabled {
            let signer2: PrivateKeySigner = pk.parse().context("invalid private key for lwc")?;
            info!("🏦 T3RN LWC enabled — loading well deployments");
            Some(LwcManager::new(signer2, dry_run))
        } else {
            None
        };

        Ok(Self {
            rebalancer: Rebalancer::new(signer, dry_run),
            targets: default_targets(),
            solver_addr,
            state: Arc::new(RwLock::new(SidecarState::default())),
            lwc_manager,
        })
    }

    /// Run one scan + rebalance cycle. Returns the actions taken.
    pub async fn tick(&self) -> Vec<BridgeAction> {
        let cycle = {
            let s = self.state.read().await;
            s.cycle + 1
        };

        info!("📊 Portfolio sidecar cycle #{} — scanning {} chains...", cycle, self.targets.len());

        // 1. Scan own balances
        let mut snapshots = scan_all(self.solver_addr).await;

        // 2. Scan LWC wells (if enabled) and merge into snapshots
        let (lwc_states, lwc_classified) = if let Some(ref mgr) = self.lwc_manager {
            info!("  🏦 Scanning {} LWC well(s)...", mgr.deployments.len());
            let states = mgr.scan_all().await;

            // Merge LWC data into the matching ChainSnapshot by chain_id
            for snap in &mut snapshots {
                if let Some(lwc) = states.iter().find(|l| l.chain_id == snap.chain_id) {
                    snap.lwc_pool_available_usd = lwc.pool_available_usd;
                    snap.lwc_pool_total_usd = lwc.pool_total_usd;
                    snap.lwc_lp_usd = lwc.lp_balance_usd;
                    snap.lwc_is_halted = lwc.is_halted;
                    snap.lwc_can_instant_exec = lwc.can_instant_exec;
                }
            }

            let classified_lwc: Vec<ClassifiedLwcChain> = states.iter().map(|ls| {
                let status = classify_lwc(
                    ls.pool_available_usd,
                    ls.can_instant_exec,
                    ls.is_halted,
                    LWC_LOW_POOL_THRESHOLD_USD,
                );
                let icon = match status {
                    LwcStatus::Healthy     => "🟢",
                    LwcStatus::LowPool     => "🟡",
                    LwcStatus::EmptyPool   => "🔴",
                    LwcStatus::Halted      => "🛑",
                    LwcStatus::NotDeployed => "—",
                };
                info!(
                    "  {} well chain={} ({}) avail=${:.2} total=${:.2} lp_own=${:.2}",
                    icon, ls.chain_id, ls.chain_key,
                    ls.pool_available_usd, ls.pool_total_usd, ls.lp_balance_usd
                );
                ClassifiedLwcChain {
                    chain_id: ls.chain_id,
                    chain_key: ls.chain_key.clone(),
                    lwc_status: status,
                    pool_available_usd: ls.pool_available_usd,
                    pool_total_usd: ls.pool_total_usd,
                    lp_balance_usd: ls.lp_balance_usd,
                }
            }).collect();

            (states, classified_lwc)
        } else {
            (vec![], vec![])
        };

        // 3. Classify own balances
        let classified: Vec<ClassifiedChain> = self.targets.iter().filter_map(|t| {
            let snap = snapshots.iter().find(|s| s.chain_id == t.chain_id)?;
            let status = t.classify(snap.stable_usd, snap.gas_eth);
            Some(ClassifiedChain {
                chain_id: t.chain_id,
                chain_name: t.chain_name.to_string(),
                status,
                stable_usd: snap.stable_usd,
                gas_eth: snap.gas_eth,
                min_stable_usd: t.min_stable_usd,
                min_gas_eth: t.min_gas_eth,
            })
        }).collect();

        for c in &classified {
            if c.status == InventoryStatus::SrcOnly { continue; }
            let icon = match c.status {
                InventoryStatus::Healthy  => "✅",
                InventoryStatus::Surplus  => "💰",
                InventoryStatus::LowGas   => "⛽",
                InventoryStatus::LowFunds => "💸",
                InventoryStatus::Critical => "🚨",
                InventoryStatus::SrcOnly  => "—",
            };
            info!(
                "  {} chain={} ({}) stable=${:.2} gas={:.4} ETH  [{:?}]",
                icon, c.chain_id, c.chain_name, c.stable_usd, c.gas_eth, c.status
            );
        }

        // 4. Rebalance (own funds + LWC phase)
        let actions = self.rebalancer.rebalance(&snapshots, &self.targets).await;

        // 5. Execute LWC deposit/withdraw actions (Phase 5 from plan)
        if let Some(ref mgr) = self.lwc_manager {
            execute_lwc_phase(mgr, &classified, &lwc_classified, &snapshots).await;
        }

        if actions.is_empty() {
            info!("  All fill chains healthy — no bridges needed.");
        } else {
            info!("  {} bridge action(s) this cycle.", actions.len());
        }

        // 6. Update shared state
        {
            let mut s = self.state.write().await;
            s.cycle = cycle;
            s.last_scan = Some(Utc::now());
            s.snapshots = snapshots;
            s.classified = classified;
            s.pending_actions = actions.clone();
            s.lwc_states = lwc_states;
            s.lwc_classified = lwc_classified;
            for a in &actions {
                s.action_log.push(ActionLogEntry {
                    ts: Utc::now(),
                    cycle,
                    action: a.clone(),
                });
                if s.action_log.len() > 200 {
                    s.action_log.remove(0);
                }
            }
        }

        actions
    }

    /// Run the sidecar loop forever. `interval_secs` is the pause between cycles.
    pub async fn run_loop(&self, interval_secs: u64) -> Result<()> {
        loop {
            self.tick().await;
            info!("😴 Portfolio sidecar sleeping {}s...", interval_secs);
            tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;
        }
    }
}

// ── LWC Phase 5: deposit/withdraw helper ──────────────────────────────────────

/// Execute LWC deposits or withdrawals based on own-fund surplus and well health.
///
/// - If a fill chain has `Surplus` own-funds AND the matching well is `LowPool`/`EmptyPool`,
///   deposit a portion of the surplus into the well.
/// - If a chain is `SrcOnly` AND the solver holds an idle LP position there,
///   withdraw (consolidate) the LP back to the solver wallet.
async fn execute_lwc_phase(
    mgr: &LwcManager,
    classified: &[ClassifiedChain],
    lwc_classified: &[ClassifiedLwcChain],
    snapshots: &[ChainSnapshot],
) {
    const LWC_DEPOSIT_FRACTION: f64 = 0.25; // deposit 25% of surplus into the well
    const MIN_LWC_DEPOSIT_USD: f64 = 10.0;
    const LWC_WITHDRAW_IDLE_THRESHOLD_USD: f64 = 1.0;

    for own in classified {
        let lwc = match lwc_classified.iter().find(|l| l.chain_id == own.chain_id) {
            Some(l) => l,
            None => continue,
        };
        let snap = match snapshots.iter().find(|s| s.chain_id == own.chain_id) {
            Some(s) => s,
            None => continue,
        };

        // Phase 5a — top up a low well from surplus own funds
        if own.status == InventoryStatus::Surplus
            && (lwc.lwc_status == LwcStatus::LowPool || lwc.lwc_status == LwcStatus::EmptyPool)
        {
            let deposit_usd = (snap.stable_usd * LWC_DEPOSIT_FRACTION).max(0.0);
            if deposit_usd >= MIN_LWC_DEPOSIT_USD {
                let asset: alloy::primitives::Address = snap.primary_stable_addr.parse()
                    .unwrap_or(alloy::primitives::Address::ZERO);
                let amount_wei = alloy::primitives::U256::from(
                    (deposit_usd * 10f64.powi(snap.primary_stable_decimals as i32)) as u128
                );
                info!(
                    "[LWC Phase 5a] Depositing ${:.2} into well on chain {} (well status: {:?})",
                    deposit_usd, own.chain_id, lwc.lwc_status
                );
                match mgr.add_liquidity(own.chain_id, asset, amount_wei).await {
                    Ok(tx) => info!("[LWC] deposit tx: {}", tx),
                    Err(e) => warn!("[LWC] deposit failed on chain {}: {}", own.chain_id, e),
                }
            }
        }

        // Phase 5b — withdraw idle LP from src-only chains
        if own.status == InventoryStatus::SrcOnly
            && lwc.lp_balance_usd > LWC_WITHDRAW_IDLE_THRESHOLD_USD
        {
            let asset: alloy::primitives::Address = snap.primary_stable_addr.parse()
                .unwrap_or(alloy::primitives::Address::ZERO);
            let amount_wei = alloy::primitives::U256::from(
                (lwc.lp_balance_usd * 10f64.powi(snap.primary_stable_decimals as i32)) as u128
            );
            info!(
                "[LWC Phase 5b] Withdrawing ${:.2} LP from src-only chain {}",
                lwc.lp_balance_usd, own.chain_id
            );
            match mgr.remove_liquidity(own.chain_id, asset, amount_wei).await {
                Ok(tx) => info!("[LWC] withdraw tx: {}", tx),
                Err(e) => warn!("[LWC] withdraw failed on chain {}: {}", own.chain_id, e),
            }
        }
    }
}
