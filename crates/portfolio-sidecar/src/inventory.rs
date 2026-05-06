//! Per-chain inventory targets and classification.
//!
//! The solver fills intents on dst chains (Base, Arbitrum, Optimism) and is
//! repaid there. Each dst chain must maintain:
//!   - enough stablecoins to cover the next fill (min_stable_usd)
//!   - enough native gas to broadcast the fill tx (min_gas_eth)
//!
//! Src chains (Ethereum, Polygon, zkSync, Linea, Scroll) never need pre-funded
//! stables — the solver does not fill there. They only appear in the inventory
//! scan for reporting (and to catch any future stray balances).

use serde::{Deserialize, Serialize};

/// Classification of a chain's current token + gas position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryStatus {
    /// Stables ≥ min AND gas ≥ min — nothing to do.
    Healthy,
    /// Stables OK, gas < min_gas_eth — needs native gas top-up via swap bridge.
    LowGas,
    /// Stables < min, gas OK — needs stable bridged in.
    LowFunds,
    /// Both stables and gas below minimum — needs gas top-up first, then stable bridge.
    Critical,
    /// Stables > high_water_usd — surplus available to fund other chains.
    Surplus,
    /// Chain is a src-only chain (deposits come from here, solver never fills here).
    /// Scanned for stray balances but not pre-funded.
    SrcOnly,
}

/// Per-chain funding targets. Configured via environment variables or defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryTarget {
    pub chain_id: u64,
    pub chain_name: &'static str,
    /// Minimum stablecoins (USD) to keep ready for fills.
    pub min_stable_usd: f64,
    /// Comfortable target to bridge toward when filling up.
    pub target_stable_usd: f64,
    /// Surplus threshold — above this, chain can fund others.
    pub high_water_usd: f64,
    /// Minimum native gas balance (in ETH-equivalent units).
    pub min_gas_eth: f64,
    /// Whether the solver actively fills on this chain (dst chain).
    pub is_fill_chain: bool,
}

impl InventoryTarget {
    pub fn classify(&self, stable_usd: f64, gas_eth: f64) -> InventoryStatus {
        if !self.is_fill_chain {
            return InventoryStatus::SrcOnly;
        }
        let gas_ok = gas_eth >= self.min_gas_eth;
        let stable_ok = stable_usd >= self.min_stable_usd;
        let surplus = stable_usd >= self.high_water_usd;

        if surplus && gas_ok {
            InventoryStatus::Surplus
        } else if gas_ok && stable_ok {
            InventoryStatus::Healthy
        } else if gas_ok && !stable_ok {
            InventoryStatus::LowFunds
        } else if !gas_ok && stable_ok {
            InventoryStatus::LowGas
        } else {
            InventoryStatus::Critical
        }
    }
}

/// How much stable to bridge in when topping up a chain (bridge to target, not just min).
impl InventoryTarget {
    pub fn stable_shortfall(&self, current_usd: f64) -> f64 {
        (self.target_stable_usd - current_usd).max(0.0)
    }
}

/// Load default inventory targets.
/// Overrideable via env: SIDECAR_MIN_STABLE_<CHAIN_ID>, SIDECAR_TARGET_STABLE_<CHAIN_ID>.
pub fn default_targets() -> Vec<InventoryTarget> {
    let mut targets = vec![
        // ── Fill chains (solver spends + gets repaid here) ──────────────────
        InventoryTarget {
            chain_id: 8453,
            chain_name: "Base",
            min_stable_usd: 50.0,
            target_stable_usd: 150.0,
            high_water_usd: 400.0,
            min_gas_eth: 0.002,
            is_fill_chain: true,
        },
        InventoryTarget {
            chain_id: 42161,
            chain_name: "Arbitrum",
            min_stable_usd: 30.0,
            target_stable_usd: 100.0,
            high_water_usd: 300.0,
            min_gas_eth: 0.002,
            is_fill_chain: true,
        },
        InventoryTarget {
            chain_id: 10,
            chain_name: "Optimism",
            min_stable_usd: 20.0,
            target_stable_usd: 80.0,
            high_water_usd: 200.0,
            min_gas_eth: 0.002,
            is_fill_chain: true,
        },
        // ── Src-only chains (deposits originate here; solver does not fill) ──
        InventoryTarget {
            chain_id: 1,
            chain_name: "Ethereum",
            min_stable_usd: 0.0,
            target_stable_usd: 0.0,
            high_water_usd: 1.0,
            min_gas_eth: 0.0,
            is_fill_chain: false,
        },
        InventoryTarget {
            chain_id: 137,
            chain_name: "Polygon",
            min_stable_usd: 0.0,
            target_stable_usd: 0.0,
            high_water_usd: 1.0,
            min_gas_eth: 0.0,
            is_fill_chain: false,
        },
        InventoryTarget {
            chain_id: 324,
            chain_name: "zkSync",
            min_stable_usd: 0.0,
            target_stable_usd: 0.0,
            high_water_usd: 1.0,
            min_gas_eth: 0.0,
            is_fill_chain: false,
        },
        InventoryTarget {
            chain_id: 59144,
            chain_name: "Linea",
            min_stable_usd: 0.0,
            target_stable_usd: 0.0,
            high_water_usd: 1.0,
            min_gas_eth: 0.0,
            is_fill_chain: false,
        },
        InventoryTarget {
            chain_id: 534352,
            chain_name: "Scroll",
            min_stable_usd: 0.0,
            target_stable_usd: 0.0,
            high_water_usd: 1.0,
            min_gas_eth: 0.0,
            is_fill_chain: false,
        },
    ];

    // Apply env overrides: SIDECAR_MIN_STABLE_8453=50 etc.
    for t in &mut targets {
        let key_min = format!("SIDECAR_MIN_STABLE_{}", t.chain_id);
        let key_target = format!("SIDECAR_TARGET_STABLE_{}", t.chain_id);
        let key_high = format!("SIDECAR_HIGH_WATER_{}", t.chain_id);
        let key_gas = format!("SIDECAR_MIN_GAS_{}", t.chain_id);
        if let Ok(v) = std::env::var(&key_min) {
            if let Ok(f) = v.parse::<f64>() { t.min_stable_usd = f; }
        }
        if let Ok(v) = std::env::var(&key_target) {
            if let Ok(f) = v.parse::<f64>() { t.target_stable_usd = f; }
        }
        if let Ok(v) = std::env::var(&key_high) {
            if let Ok(f) = v.parse::<f64>() { t.high_water_usd = f; }
        }
        if let Ok(v) = std::env::var(&key_gas) {
            if let Ok(f) = v.parse::<f64>() { t.min_gas_eth = f; }
        }
    }

    targets
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(min: f64, target: f64, high: f64, gas: f64) -> InventoryTarget {
        InventoryTarget {
            chain_id: 8453, chain_name: "Base",
            min_stable_usd: min, target_stable_usd: target,
            high_water_usd: high, min_gas_eth: gas, is_fill_chain: true,
        }
    }

    #[test]
    fn healthy_when_both_ok() {
        let t = target(50.0, 150.0, 400.0, 0.002);
        assert_eq!(t.classify(100.0, 0.005), InventoryStatus::Healthy);
    }

    #[test]
    fn low_gas_when_only_gas_missing() {
        let t = target(50.0, 150.0, 400.0, 0.002);
        assert_eq!(t.classify(100.0, 0.0001), InventoryStatus::LowGas);
    }

    #[test]
    fn low_funds_when_only_stable_missing() {
        let t = target(50.0, 150.0, 400.0, 0.002);
        assert_eq!(t.classify(10.0, 0.005), InventoryStatus::LowFunds);
    }

    #[test]
    fn critical_when_both_missing() {
        let t = target(50.0, 150.0, 400.0, 0.002);
        assert_eq!(t.classify(5.0, 0.0001), InventoryStatus::Critical);
    }

    #[test]
    fn surplus_when_above_high_water() {
        let t = target(50.0, 150.0, 400.0, 0.002);
        assert_eq!(t.classify(500.0, 0.005), InventoryStatus::Surplus);
    }

    #[test]
    fn src_only_regardless_of_balance() {
        let mut t = target(0.0, 0.0, 1.0, 0.0);
        t.is_fill_chain = false;
        assert_eq!(t.classify(999.0, 999.0), InventoryStatus::SrcOnly);
    }

    #[test]
    fn shortfall_calculation() {
        let t = target(50.0, 150.0, 400.0, 0.002);
        assert!((t.stable_shortfall(80.0) - 70.0).abs() < 0.01);
        assert_eq!(t.stable_shortfall(200.0), 0.0);
    }
}
