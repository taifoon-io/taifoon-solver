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
pub mod tx_guard;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::{info, warn};

use inventory::{
    classify_lwc, classify_solana_gas, default_targets, InventoryStatus, InventoryTarget,
    LwcStatus, SolanaGasStatus, LWC_LOW_POOL_THRESHOLD_USD, MIN_SOLANA_SOL, WARN_SOLANA_SOL,
};
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

        // 3b. Solana SOL gas probe — non-fatal, never blocks the EVM rebalancer.
        let (solana_sol_balance, solana_gas_status) = scan_solana_gas().await;

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

        // Phase 5a — top up a low well from surplus own funds.
        // Use primary_stable_usd (not stable_usd) because the well only accepts
        // the primary stable — including secondary stable in the amount would
        // produce an amount_wei that exceeds the actual primary balance.
        if own.status == InventoryStatus::Surplus
            && (lwc.lwc_status == LwcStatus::LowPool || lwc.lwc_status == LwcStatus::EmptyPool)
        {
            let deposit_usd = (snap.primary_stable_usd * LWC_DEPOSIT_FRACTION).max(0.0);
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
