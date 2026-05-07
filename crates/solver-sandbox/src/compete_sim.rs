//! CompeteSim — multi-solver competition simulation runner.
//!
//! Spawns N in-process solver agents, each backed by a local wallet + budget.
//! All agents compete on the same GenomeReplay stream and draw from the same
//! WellSimulator.  A local mock Spinner issues FillPermits so the auth guard
//! works without a real Spinner service.
//!
//! At the end of the run (or --duration), prints a leaderboard JSON.

use crate::well_sim::WellSimulator;
use crate::genome_replay::GenomeEvent;
use serde::{Deserialize, Serialize};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimConfig {
    pub solver_count: usize,
    /// Initial budget per solver in USD.
    pub budget_per_solver_usd: f64,
    /// Initial seeded liquidity per chain/asset in USD.
    pub well_seed_usd: f64,
    /// List of chain IDs that have wells.
    pub well_chains: Vec<u64>,
    /// How long to run the simulation (seconds, 0 = until events exhausted).
    pub duration_secs: u64,
    pub speed_multiplier: f64,
}

impl Default for SimConfig {
    fn default() -> Self {
        Self {
            solver_count: 3,
            budget_per_solver_usd: 500.0,
            well_seed_usd: 1000.0,
            well_chains: vec![8453, 42161, 10],
            duration_secs: 60,
            speed_multiplier: 10.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolverStats {
    pub solver_id: String,
    pub fills: u64,
    pub gross_usd: f64,
    pub gas_cost_usd: f64,
    pub net_usd: f64,
    pub permits_issued: u64,
    pub permits_rejected: u64,
    pub lwc_draws_usd: f64,
}

impl SolverStats {
    fn new(id: &str) -> Self {
        Self {
            solver_id: id.to_string(),
            fills: 0,
            gross_usd: 0.0,
            gas_cost_usd: 0.0,
            net_usd: 0.0,
            permits_issued: 0,
            permits_rejected: 0,
            lwc_draws_usd: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Leaderboard {
    pub config: SimConfig,
    pub duration_secs: f64,
    pub total_intents: u64,
    pub total_fills: u64,
    pub well_drawdown_usd: f64,
    pub solvers: Vec<SolverStats>,
}

pub struct CompeteSim {
    config: SimConfig,
    well: Arc<Mutex<WellSimulator>>,
    events: Vec<GenomeEvent>,
}

impl CompeteSim {
    pub fn new(config: SimConfig, events: Vec<GenomeEvent>) -> Self {
        let mut sim = WellSimulator::new();
        for &chain_id in &config.well_chains {
            sim.seed(chain_id, "USDC", config.well_seed_usd, "t3rn_protocol");
        }
        Self {
            config,
            well: Arc::new(Mutex::new(sim)),
            events,
        }
    }

    /// Run the competition and return the leaderboard.
    pub async fn run(self) -> Leaderboard {
        let config = self.config.clone();
        let n = config.solver_count;
        let start = Instant::now();
        let deadline = if config.duration_secs > 0 {
            Some(start + Duration::from_secs(config.duration_secs))
        } else {
            None
        };

        // Per-solver stats (shared across tasks)
        let stats: Vec<Arc<Mutex<SolverStats>>> = (0..n)
            .map(|i| Arc::new(Mutex::new(SolverStats::new(&format!("solver_{:02}", i)))))
            .collect();

        // Track initial well depth for drawdown calculation
        let initial_well_usd: f64 = config.well_chains.len() as f64 * config.well_seed_usd;

        let events = Arc::new(self.events);
        let well = self.well.clone();

        let mut handles = Vec::new();
        for solver_idx in 0..n {
            let stats_arc = stats[solver_idx].clone();
            let well_arc = well.clone();
            let events_arc = events.clone();
            let cfg = config.clone();
            let deadline_copy = deadline;

            let handle = tokio::spawn(async move {
                run_solver(solver_idx, stats_arc, well_arc, events_arc, cfg, deadline_copy).await;
            });
            handles.push(handle);
        }

        for h in handles {
            let _ = h.await;
        }

        let elapsed = start.elapsed().as_secs_f64();

        // Collect stats
        let mut solver_results = Vec::new();
        for s in &stats {
            solver_results.push(s.lock().await.clone());
        }
        solver_results.sort_by(|a, b| b.net_usd.partial_cmp(&a.net_usd).unwrap_or(std::cmp::Ordering::Equal));

        let total_fills = solver_results.iter().map(|s| s.fills).sum();

        // Remaining well depth
        let remaining: f64 = {
            let w = well.lock().await;
            w.snapshot().iter().map(|s| s.total_usd).sum()
        };
        let well_drawdown_usd = initial_well_usd - remaining;

        Leaderboard {
            config,
            duration_secs: elapsed,
            total_intents: events.len() as u64,
            total_fills,
            well_drawdown_usd: well_drawdown_usd.max(0.0),
            solvers: solver_results,
        }
    }
}

async fn run_solver(
    idx: usize,
    stats: Arc<Mutex<SolverStats>>,
    well: Arc<Mutex<WellSimulator>>,
    events: Arc<Vec<GenomeEvent>>,
    config: SimConfig,
    deadline: Option<Instant>,
) {
    let mut budget = config.budget_per_solver_usd;
    let interval = Duration::from_millis((50.0 / config.speed_multiplier.max(0.1)) as u64);

    for ev in events.iter() {
        if let Some(dl) = deadline {
            if Instant::now() > dl { break; }
        }
        if ev.kind != "order" { continue; }

        let amount_usd = extract_amount_usd(&ev.meta);
        if amount_usd <= 0.0 { continue; }

        let gas_cost = 0.30; // simulated $0.30 per fill
        let reward = amount_usd * 0.0015; // 0.15% tip
        let net = reward - gas_cost;
        if net <= 0.0 { continue; }

        let dst_chain = ev.meta.get("dst_chain")
            .and_then(|v| v.as_u64())
            .unwrap_or(8453);

        // Try own funds first
        let used_lwc = if budget >= amount_usd {
            budget -= amount_usd;
            false
        } else {
            // Try drawing from the well
            let mut w = well.lock().await;
            if w.can_instant_exec(dst_chain, "USDC", amount_usd) {
                // Simulate permit request (mock — always succeeds in sandbox)
                match w.reserve(dst_chain, "USDC", amount_usd) {
                    Ok(_) => {
                        // Settle after simulated fill
                        w.settle(dst_chain, "USDC", amount_usd);
                        true
                    }
                    Err(e) => {
                        warn!("[solver_{:02}] well reserve failed: {}", idx, e);
                        let mut s = stats.lock().await;
                        s.permits_rejected += 1;
                        continue;
                    }
                }
            } else {
                // Skip — no funds available
                continue;
            }
        };

        tokio::time::sleep(interval).await;

        let mut s = stats.lock().await;
        s.fills += 1;
        s.gross_usd += reward;
        s.gas_cost_usd += gas_cost;
        s.net_usd += net;
        s.permits_issued += 1;
        if used_lwc {
            s.lwc_draws_usd += amount_usd;
        }

        info!(
            "[solver_{:02}] fill intent={} amount=${:.2} net=${:.2} lwc={}",
            idx, ev.id, amount_usd, net, used_lwc
        );
    }
}

fn extract_amount_usd(meta: &serde_json::Value) -> f64 {
    // Try amount as string (wei-like), assume USDC 6 decimals for simplicity
    if let Some(v) = meta.get("amount").and_then(|v| v.as_str()) {
        if let Ok(n) = v.parse::<u128>() {
            return n as f64 / 1_000_000.0; // 6 decimals
        }
    }
    if let Some(v) = meta.get("amount").and_then(|v| v.as_f64()) {
        return v;
    }
    0.0
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::genome_replay::ReplayState;

    #[tokio::test]
    async fn compete_sim_runs_and_produces_leaderboard() {
        let state = ReplayState::from_synthetic(20);
        let config = SimConfig {
            solver_count: 2,
            budget_per_solver_usd: 1000.0,
            well_seed_usd: 500.0,
            well_chains: vec![8453],
            duration_secs: 0,
            speed_multiplier: 1000.0,
        };
        let sim = CompeteSim::new(config, state.events);
        let lb = sim.run().await;
        assert_eq!(lb.solvers.len(), 2);
        assert!(lb.total_intents > 0);
    }

    #[test]
    fn extract_amount_usd_parses_string_wei() {
        let meta = serde_json::json!({ "amount": "1000000" });
        assert!((extract_amount_usd(&meta) - 1.0).abs() < 1e-9);
    }
}
