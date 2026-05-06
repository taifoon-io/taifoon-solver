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
pub mod rebalancer;
pub mod scanner;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

use inventory::{default_targets, InventoryStatus, InventoryTarget};
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
}

impl PortfolioSidecar {
    /// Construct from a hex private key string (with or without 0x prefix).
    pub fn from_key(private_key: &str, dry_run: bool) -> anyhow::Result<Self> {
        use alloy::signers::local::PrivateKeySigner;
        use anyhow::Context as _;
        let pk = private_key.trim().trim_start_matches("0x");
        let signer: PrivateKeySigner = pk.parse().context("invalid private key")?;
        let solver_addr = signer.address();
        Ok(Self {
            rebalancer: Rebalancer::new(signer, dry_run),
            targets: default_targets(),
            solver_addr,
            state: Arc::new(RwLock::new(SidecarState::default())),
        })
    }

    /// Run one scan + rebalance cycle. Returns the actions taken.
    pub async fn tick(&self) -> Vec<BridgeAction> {
        let cycle = {
            let s = self.state.read().await;
            s.cycle + 1
        };

        info!("📊 Portfolio sidecar cycle #{} — scanning {} chains...", cycle, self.targets.len());

        // 1. Scan
        let snapshots = scan_all(self.solver_addr).await;

        // 2. Classify
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

        // Log the classification.
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

        // 3. Rebalance
        let actions = self.rebalancer.rebalance(&snapshots, &self.targets).await;

        if actions.is_empty() {
            info!("  All fill chains healthy — no bridges needed.");
        } else {
            info!("  {} bridge action(s) this cycle.", actions.len());
        }

        // 4. Update shared state
        {
            let mut s = self.state.write().await;
            s.cycle = cycle;
            s.last_scan = Some(Utc::now());
            s.snapshots = snapshots;
            s.classified = classified;
            s.pending_actions = actions.clone();
            for a in &actions {
                s.action_log.push(ActionLogEntry {
                    ts: Utc::now(),
                    cycle,
                    action: a.clone(),
                });
                // Keep last 200 entries
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
