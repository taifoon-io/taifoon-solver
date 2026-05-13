//! Generic in-memory liquidity-well simulator. Tracks pool depth, reservations,
//! and settlement for sandbox tests.
//!
//! Used by the CompeteSim binary so competing solvers run against a realistic
//! well without touching mainnet.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WellError {
    #[error("Insufficient available liquidity: have ${have:.2}, need ${need:.2}")]
    InsufficientLiquidity { have: f64, need: f64 },
    #[error("Solver has no LP position on chain {chain_id}")]
    NoLpPosition { chain_id: u64 },
    #[error("LP amount exceeds solver's position: have ${have:.2}, want ${want:.2}")]
    LpUnderflow { have: f64, want: f64 },
    #[error("Well is halted")]
    Halted,
}

/// One asset pool within a well.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WellPool {
    /// Total stables deposited (USD equiv).
    pub total_usd: f64,
    /// Available for fills (total minus reserved).
    pub available_usd: f64,
    /// Reserved by in-flight fill commitments.
    pub reserved_usd: f64,
    /// Per-solver LP balances (address → USD).
    pub lp_balances: HashMap<String, f64>,
    /// Halted flag.
    pub halted: bool,
}

impl Default for WellPool {
    fn default() -> Self {
        Self {
            total_usd: 0.0,
            available_usd: 0.0,
            reserved_usd: 0.0,
            lp_balances: HashMap::new(),
            halted: false,
        }
    }
}

/// Snapshot of a single chain's simulated well state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WellSnapshot {
    pub chain_id: u64,
    pub total_usd: f64,
    pub available_usd: f64,
    pub reserved_usd: f64,
    pub lp_count: usize,
}

/// In-memory liquidity-well simulator keyed by (chain_id, asset_symbol).
pub struct WellSimulator {
    pools: HashMap<(u64, String), WellPool>,
}

impl WellSimulator {
    pub fn new() -> Self {
        Self { pools: HashMap::new() }
    }

    fn pool_mut(&mut self, chain_id: u64, asset: &str) -> &mut WellPool {
        self.pools.entry((chain_id, asset.to_string())).or_default()
    }

    fn pool(&self, chain_id: u64, asset: &str) -> Option<&WellPool> {
        self.pools.get(&(chain_id, asset.to_string()))
    }

    /// Seed a pool with initial liquidity (for test setup).
    pub fn seed(&mut self, chain_id: u64, asset: &str, amount_usd: f64, owner: &str) {
        let pool = self.pool_mut(chain_id, asset);
        pool.total_usd += amount_usd;
        pool.available_usd += amount_usd;
        *pool.lp_balances.entry(owner.to_string()).or_default() += amount_usd;
    }

    /// Add liquidity on behalf of `solver`.
    pub fn add_liquidity(&mut self, chain_id: u64, asset: &str, amount_usd: f64, solver: &str) -> Result<(), WellError> {
        let pool = self.pool_mut(chain_id, asset);
        if pool.halted {
            return Err(WellError::Halted);
        }
        pool.total_usd += amount_usd;
        pool.available_usd += amount_usd;
        *pool.lp_balances.entry(solver.to_string()).or_default() += amount_usd;
        Ok(())
    }

    /// Remove liquidity on behalf of `solver` (proportional to LP share).
    pub fn remove_liquidity(&mut self, chain_id: u64, asset: &str, amount_usd: f64, solver: &str) -> Result<(), WellError> {
        let pool = self.pool_mut(chain_id, asset);
        if pool.halted {
            return Err(WellError::Halted);
        }
        let lp = pool.lp_balances.get(solver).copied().unwrap_or(0.0);
        if lp < amount_usd {
            return Err(WellError::LpUnderflow { have: lp, want: amount_usd });
        }
        if pool.available_usd < amount_usd {
            return Err(WellError::InsufficientLiquidity {
                have: pool.available_usd,
                need: amount_usd,
            });
        }
        pool.total_usd -= amount_usd;
        pool.available_usd -= amount_usd;
        *pool.lp_balances.get_mut(solver).unwrap() -= amount_usd;
        if pool.lp_balances[solver] < 1e-9 {
            pool.lp_balances.remove(solver);
        }
        Ok(())
    }

    /// Reserve capacity for an in-flight fill.
    pub fn reserve(&mut self, chain_id: u64, asset: &str, amount_usd: f64) -> Result<(), WellError> {
        let pool = self.pool_mut(chain_id, asset);
        if pool.halted {
            return Err(WellError::Halted);
        }
        if pool.available_usd < amount_usd {
            return Err(WellError::InsufficientLiquidity {
                have: pool.available_usd,
                need: amount_usd,
            });
        }
        pool.available_usd -= amount_usd;
        pool.reserved_usd += amount_usd;
        Ok(())
    }

    /// Release a previously reserved amount (fill completed or cancelled).
    pub fn release(&mut self, chain_id: u64, asset: &str, amount_usd: f64) {
        if let Some(pool) = self.pools.get_mut(&(chain_id, asset.to_string())) {
            let release = amount_usd.min(pool.reserved_usd);
            pool.reserved_usd -= release;
            pool.available_usd += release;
        }
    }

    /// Settle a fill: the reserved amount is consumed (removed from total).
    pub fn settle(&mut self, chain_id: u64, asset: &str, amount_usd: f64) {
        if let Some(pool) = self.pools.get_mut(&(chain_id, asset.to_string())) {
            let consume = amount_usd.min(pool.reserved_usd);
            pool.reserved_usd -= consume;
            pool.total_usd -= consume;
        }
    }

    /// Mirrors `canPerformInstantExecution` on the V4 contract.
    pub fn can_instant_exec(&self, chain_id: u64, asset: &str, amount_usd: f64) -> bool {
        self.pool(chain_id, asset)
            .map(|p| !p.halted && p.available_usd >= amount_usd)
            .unwrap_or(false)
    }

    /// Halt or unhalt a well.
    pub fn set_halted(&mut self, chain_id: u64, asset: &str, halted: bool) {
        self.pool_mut(chain_id, asset).halted = halted;
    }

    /// Snapshot of all chains.
    pub fn snapshot(&self) -> Vec<WellSnapshot> {
        let mut seen: HashMap<u64, WellSnapshot> = HashMap::new();
        for ((chain_id, _), pool) in &self.pools {
            let e = seen.entry(*chain_id).or_insert(WellSnapshot {
                chain_id: *chain_id,
                total_usd: 0.0,
                available_usd: 0.0,
                reserved_usd: 0.0,
                lp_count: 0,
            });
            e.total_usd += pool.total_usd;
            e.available_usd += pool.available_usd;
            e.reserved_usd += pool.reserved_usd;
            e.lp_count += pool.lp_balances.len();
        }
        let mut snaps: Vec<_> = seen.into_values().collect();
        snaps.sort_by_key(|s| s.chain_id);
        snaps
    }
}

impl Default for WellSimulator {
    fn default() -> Self { Self::new() }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_remove_liquidity() {
        let mut sim = WellSimulator::new();
        sim.add_liquidity(8453, "USDC", 500.0, "0xSolver1").unwrap();
        assert!((sim.pool(8453, "USDC").unwrap().available_usd - 500.0).abs() < 1e-9);

        sim.remove_liquidity(8453, "USDC", 200.0, "0xSolver1").unwrap();
        assert!((sim.pool(8453, "USDC").unwrap().available_usd - 300.0).abs() < 1e-9);
    }

    #[test]
    fn reserve_and_release() {
        let mut sim = WellSimulator::new();
        sim.seed(8453, "USDC", 1000.0, "0xLP1");
        sim.reserve(8453, "USDC", 200.0).unwrap();
        let pool = sim.pool(8453, "USDC").unwrap();
        assert!((pool.available_usd - 800.0).abs() < 1e-9);
        assert!((pool.reserved_usd - 200.0).abs() < 1e-9);

        sim.release(8453, "USDC", 200.0);
        assert!((sim.pool(8453, "USDC").unwrap().available_usd - 1000.0).abs() < 1e-9);
    }

    #[test]
    fn reserve_fails_when_insufficient() {
        let mut sim = WellSimulator::new();
        sim.seed(8453, "USDC", 50.0, "0xLP1");
        let err = sim.reserve(8453, "USDC", 100.0).unwrap_err();
        assert!(matches!(err, WellError::InsufficientLiquidity { .. }));
    }

    #[test]
    fn remove_fails_when_lp_underflow() {
        let mut sim = WellSimulator::new();
        sim.seed(8453, "USDC", 100.0, "0xLP1");
        let err = sim.remove_liquidity(8453, "USDC", 200.0, "0xLP1").unwrap_err();
        assert!(matches!(err, WellError::LpUnderflow { .. }));
    }

    #[test]
    fn halted_well_rejects_all_writes() {
        let mut sim = WellSimulator::new();
        sim.seed(8453, "USDC", 100.0, "0xLP1");
        sim.set_halted(8453, "USDC", true);
        assert!(matches!(sim.add_liquidity(8453, "USDC", 10.0, "x"), Err(WellError::Halted)));
        assert!(matches!(sim.remove_liquidity(8453, "USDC", 10.0, "0xLP1"), Err(WellError::Halted)));
        assert!(matches!(sim.reserve(8453, "USDC", 10.0), Err(WellError::Halted)));
    }

    #[test]
    fn can_instant_exec() {
        let mut sim = WellSimulator::new();
        sim.seed(8453, "USDC", 500.0, "0xLP1");
        assert!(sim.can_instant_exec(8453, "USDC", 100.0));
        assert!(!sim.can_instant_exec(8453, "USDC", 600.0));
    }

    #[test]
    fn settle_consumes_from_total() {
        let mut sim = WellSimulator::new();
        sim.seed(8453, "USDC", 1000.0, "0xLP1");
        sim.reserve(8453, "USDC", 100.0).unwrap();
        sim.settle(8453, "USDC", 100.0);
        let pool = sim.pool(8453, "USDC").unwrap();
        assert!((pool.total_usd - 900.0).abs() < 1e-9);
        assert!((pool.reserved_usd).abs() < 1e-9);
    }

    #[test]
    fn snapshot_aggregates_chains() {
        let mut sim = WellSimulator::new();
        sim.seed(8453, "USDC", 1000.0, "0xLP1");
        sim.seed(42161, "USDC", 500.0, "0xLP1");
        let snaps = sim.snapshot();
        assert_eq!(snaps.len(), 2);
    }
}
