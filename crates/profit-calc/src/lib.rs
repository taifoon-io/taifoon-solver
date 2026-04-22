use anyhow::{Context, Result};
use genome_client::Intent;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Profit calculation result
#[derive(Debug, Clone)]
pub struct ProfitResult {
    pub net_profit_usd: f64,
    pub profitable: bool,
    pub breakdown: ProfitBreakdown,
}

#[derive(Debug, Clone)]
pub struct ProfitBreakdown {
    pub protocol_fee_usd: f64,
    pub spread_usd: f64,
    pub gas_cost_usd: f64,
    pub liquidity_cost_usd: f64,
}

#[derive(Debug, Deserialize)]
struct SolverIntel {
    protocols: HashMap<String, ProtocolInfo>,
}

#[derive(Debug, Deserialize)]
struct ProtocolInfo {
    #[serde(default)]
    avg_fee_bps: Option<f64>,
    #[serde(default)]
    fills_168h: Option<u64>,
}

/// Profit calculator configuration
pub struct ProfitCalculator {
    min_profit_usd: f64,
    protocol_fees: HashMap<String, f64>, // protocol_id -> fee in bps
    eth_price_usd: f64,                 // Cached ETH price
}

impl ProfitCalculator {
    /// Create new profit calculator
    pub fn new(min_profit_usd: f64) -> Self {
        Self {
            min_profit_usd,
            protocol_fees: HashMap::new(),
            eth_price_usd: 3000.0, // Default, will fetch real price
        }
    }

    /// Load protocol fees from solver_intel.json
    pub fn load_solver_intel(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let content = fs::read_to_string(path.as_ref())
            .context("Failed to read solver_intel.json")?;

        let intel: SolverIntel = serde_json::from_str(&content)
            .context("Failed to parse solver_intel.json")?;

        for (protocol_id, info) in intel.protocols {
            if let Some(fee_bps) = info.avg_fee_bps {
                // Normalize protocol ID (remove underscores, lowercase)
                let normalized = protocol_id.to_lowercase().replace('_', "");
                self.protocol_fees.insert(normalized.clone(), fee_bps);

                // Also store original format
                self.protocol_fees.insert(protocol_id.clone(), fee_bps);

                tracing::debug!("Loaded protocol {}: {} bps fee", protocol_id, fee_bps);
            }
        }

        tracing::info!("✅ Loaded {} protocol fees from solver intel", self.protocol_fees.len());
        Ok(())
    }

    /// Calculate profitability for an intent
    pub async fn calculate(&self, intent: &Intent) -> Result<ProfitResult> {
        // 1. Get protocol fee
        let protocol_fee_bps = self.get_protocol_fee(&intent.protocol);

        // 2. Parse amount (assuming USDC with 6 decimals)
        let amount_raw: u128 = intent.amount.parse()
            .unwrap_or(0);
        let amount_usd = amount_raw as f64 / 1_000_000.0; // USDC has 6 decimals

        // 3. Calculate protocol fee in USD
        let protocol_fee_usd = amount_usd * (protocol_fee_bps / 10000.0);

        // 4. Estimate gas costs
        let (src_gas_usd, dst_gas_usd) = self.estimate_gas_costs(intent);
        let total_gas_usd = src_gas_usd + dst_gas_usd;

        // 5. Calculate spread (for now, assume 0 - user gets exact amount they requested)
        let spread_usd = 0.0;

        // 6. Liquidity cost (own funds = 0)
        let liquidity_cost_usd = 0.0;

        // 7. Net profit
        let net_profit_usd = protocol_fee_usd + spread_usd - total_gas_usd - liquidity_cost_usd;

        Ok(ProfitResult {
            net_profit_usd,
            profitable: net_profit_usd > self.min_profit_usd,
            breakdown: ProfitBreakdown {
                protocol_fee_usd,
                spread_usd,
                gas_cost_usd: total_gas_usd,
                liquidity_cost_usd,
            },
        })
    }

    fn get_protocol_fee(&self, protocol: &str) -> f64 {
        // Try exact match first
        if let Some(&fee) = self.protocol_fees.get(protocol) {
            return fee;
        }

        // Try normalized (no underscores, lowercase)
        let normalized = protocol.to_lowercase().replace('_', "");
        if let Some(&fee) = self.protocol_fees.get(&normalized) {
            return fee;
        }

        // Default to 10 bps if unknown
        tracing::warn!("Unknown protocol {}, using default 10 bps", protocol);
        10.0
    }

    fn estimate_gas_costs(&self, intent: &Intent) -> (f64, f64) {
        // Simplified gas estimation based on chain
        let src_gas_usd = match intent.src_chain {
            1 => 5.0,      // Ethereum mainnet (expensive)
            10 => 0.05,    // Optimism (cheap)
            8453 => 0.05,  // Base (cheap)
            42161 => 0.10, // Arbitrum (cheap)
            _ => 1.0,      // Default
        };

        let dst_gas_usd = match intent.dst_chain {
            1 => 5.0,      // Ethereum mainnet (expensive)
            10 => 0.05,    // Optimism (cheap)
            8453 => 0.05,  // Base (cheap)
            42161 => 0.10, // Arbitrum (cheap)
            _ => 1.0,      // Default
        };

        (src_gas_usd, dst_gas_usd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profit_calculation() {
        let mut calc = ProfitCalculator::new(1.0);

        // Manually set LiFi fee (49 bps)
        calc.protocol_fees.insert("lifi_v2".to_string(), 49.0);

        let intent = Intent {
            id: "test".to_string(),
            protocol: "lifi_v2".to_string(),
            src_chain: 1,
            dst_chain: 42161,
            src_token: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string(),
            dst_token: "0xaf88d065e77c8cC2239327C5EDb3A432268e5831".to_string(),
            amount: "10000000000".to_string(), // 10,000 USDC
            depositor: "0xuser".to_string(),
            recipient: "0xuser".to_string(),
            tx_hash: "0xabc".to_string(),
            detected_at: 0,
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(calc.calculate(&intent)).unwrap();

        // 10,000 USDC * 49 bps = $49 fee
        // Gas: $5 (ETH) + $0.10 (Arb) = $5.10
        // Net profit: $49 - $5.10 = $43.90
        assert!(result.breakdown.protocol_fee_usd > 48.0);
        assert!(result.breakdown.protocol_fee_usd < 50.0);
        assert!(result.net_profit_usd > 40.0);
        assert!(result.profitable);
    }
}
