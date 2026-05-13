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
pub mod kamino_intents;
pub mod rebalancer;
pub mod scanner;
pub mod tx_guard;

pub use kamino_intents::{KaminoIntentClient, KaminoPortfolioIntent};

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::{info, warn};

use inventory::{
    classify_solana_gas, default_targets, InventoryStatus, InventoryTarget,
    SolanaGasStatus, MIN_SOLANA_SOL, WARN_SOLANA_SOL,
};
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
    /// Last-known Solana wallet SOL balance, if SOLANA_ADDRESS is configured.
    /// `None` means the RPC was unreachable on the last probe (Unknown status).
    #[serde(default)]
    pub solana_sol_balance: Option<f64>,
    /// Classification of the Solana wallet gas position. `Unknown` if the RPC
    /// was unreachable, or the field is absent if SOLANA_ADDRESS is unset.
    #[serde(default)]
    pub solana_gas_status: Option<SolanaGasStatus>,
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
    /// Serialises overlapping `tick` calls. The background loop and the
    /// manual `POST /api/solver/rebalance` handler both contend on this so a
    /// trigger that lands during an in-flight cycle waits rather than running
    /// two scans in parallel.
    tick_lock: Mutex<()>,
    /// Seconds between scheduled background ticks. Set by the runner via
    /// `set_interval_secs`; the status endpoint reads this to compute
    /// `next_run_at`. `0` means "not scheduled" (manual-trigger only).
    interval_secs: AtomicU64,
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
            tick_lock: Mutex::new(()),
            interval_secs: AtomicU64::new(0),
        })
    }

    /// Record the schedule's interval so the API status endpoint can compute
    /// `next_run_at`. Idempotent; later calls overwrite the value.
    pub fn set_interval_secs(&self, secs: u64) {
        self.interval_secs.store(secs, Ordering::Relaxed);
    }

    /// Read the recorded background interval, or `0` if no schedule was set.
    pub fn interval_secs(&self) -> u64 {
        self.interval_secs.load(Ordering::Relaxed)
    }

    /// Try to acquire the tick lock without waiting. Returns `None` if a tick
    /// is already in flight — callers should treat this as "rebalance already
    /// running, skip rather than queue another cycle."
    pub fn try_lock_tick(&self) -> Option<tokio::sync::MutexGuard<'_, ()>> {
        self.tick_lock.try_lock().ok()
    }

    /// Run one scan + rebalance cycle. Returns the actions taken.
    pub async fn tick(&self) -> Vec<BridgeAction> {
        let _guard = self.tick_lock.lock().await;
        let cycle = {
            let s = self.state.read().await;
            s.cycle + 1
        };

        info!("📊 Portfolio sidecar cycle #{} — scanning {} chains...", cycle, self.targets.len());

        // 1. Scan own balances
        let snapshots = scan_all(self.solver_addr).await;

        // 2. Classify own balances
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

        // 3. Solana SOL gas probe — non-fatal, never blocks the EVM rebalancer.
        let (solana_sol_balance, solana_gas_status) = scan_solana_gas().await;

        // 4. Rebalance own funds
        let actions = self.rebalancer.rebalance(&snapshots, &self.targets).await;

        if actions.is_empty() {
            info!("  All fill chains healthy — no bridges needed.");
        } else {
            info!("  {} bridge action(s) this cycle.", actions.len());
        }

        // 5. Update shared state
        {
            let mut s = self.state.write().await;
            s.cycle = cycle;
            s.last_scan = Some(Utc::now());
            s.snapshots = snapshots;
            s.classified = classified;
            s.pending_actions = actions.clone();
            s.solana_sol_balance = solana_sol_balance;
            s.solana_gas_status = solana_gas_status;
            for a in &actions {
                s.action_log.push(ActionLogEntry {
                    ts: Utc::now(),
                    cycle,
                    action: a.clone(),
                });
            }
            // Drain oldest entries in one pass rather than one remove(0) per push.
            if s.action_log.len() > 200 {
                let excess = s.action_log.len() - 200;
                s.action_log.drain(0..excess);
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

// ── Solana gas probe ──────────────────────────────────────────────────────────

/// Fetch SOL balance for `address` via SOLANA_RPC_URL `getBalance`.
///
/// Returns `Ok(None)` only when SOLANA_ADDRESS is unset (caller's responsibility
/// — this fn requires `address` non-empty), `Ok(Some(sol))` on a successful
/// probe, and `Err(_)` when the RPC was unreachable or returned a malformed
/// response. Errors here MUST NOT crash the sidecar tick — the caller logs and
/// continues, leaving the gas status as `Unknown`.
async fn fetch_solana_sol_balance(address: &str) -> anyhow::Result<f64> {
    use anyhow::{anyhow, Context};
    let solana_rpc = std::env::var("SOLANA_RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .context("build reqwest client for solana balance probe")?;
    let body = serde_json::json!({
        "jsonrpc": "2.0", "id": 1,
        "method": "getBalance",
        "params": [address]
    });
    let resp = client
        .post(&solana_rpc)
        .json(&body)
        .send()
        .await
        .context("solana getBalance request failed")?;
    let parsed: serde_json::Value = resp
        .json()
        .await
        .context("solana getBalance response not json")?;
    if let Some(err) = parsed.get("error") {
        return Err(anyhow!("solana rpc error: {}", err));
    }
    let lamports = parsed
        .pointer("/result/value")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow!("solana getBalance missing result.value"))?;
    Ok(lamports as f64 / 1e9)
}

/// One full Solana gas probe — reads SOLANA_ADDRESS, queries SOL balance,
/// classifies, logs the actionable WARN at low-gas, and logs+swallows any RPC
/// error. Returns the (balance, status) pair to persist on `SidecarState`.
///
/// Returns `(None, None)` if SOLANA_ADDRESS is unset (the solver does not run
/// any Solana path on this deployment) so the API surface stays absent rather
/// than reporting a meaningless `Unknown`.
async fn scan_solana_gas() -> (Option<f64>, Option<SolanaGasStatus>) {
    let solana_addr = match std::env::var("SOLANA_ADDRESS") {
        Ok(a) if !a.trim().is_empty() => a,
        _ => return (None, None),
    };

    match fetch_solana_sol_balance(&solana_addr).await {
        Ok(sol) => {
            let status = classify_solana_gas(Some(sol));
            if status == SolanaGasStatus::LowGas {
                warn!(
                    "⛽ Solana wallet low on SOL: have={:.6} sol min={:.3} sol (status={})",
                    sol, MIN_SOLANA_SOL, status.as_str()
                );
            } else {
                let icon = match status {
                    SolanaGasStatus::Healthy => "✅",
                    SolanaGasStatus::Warn => "⚠️",
                    _ => "—",
                };
                info!(
                    "  {} Solana wallet SOL={:.6} (min={:.3}, warn={:.3}) [{}]",
                    icon, sol, MIN_SOLANA_SOL, WARN_SOLANA_SOL, status.as_str()
                );
            }
            (Some(sol), Some(status))
        }
        Err(e) => {
            // Non-fatal: log and continue. Status becomes Unknown so the API
            // can render a yellow "?" rather than misreporting healthy/low.
            warn!("Solana gas probe failed (non-fatal): {}", e);
            (None, Some(SolanaGasStatus::Unknown))
        }
    }
}

