use anyhow::{Context, Result};
use genome_client::Intent;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

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

/// Gas price from Warmbed API
#[derive(Debug, Clone, Deserialize)]
struct GasMetrics {
    chain_id: u64,
    block_number: u64,
    timestamp: u64,
    gas_used: u64,
    gas_limit: u64,
    gas_price: Option<u64>,
    tx_count: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct GasResponse {
    chain_id: u64,
    block_number: u64,
    timestamp: u64,
    base_fee_per_gas_wei: Option<String>,
    gas_price_gwei: Option<f64>,
    gas_used: u64,
    gas_limit: u64,
    utilization_pct: f64,
}

/// Cached gas price data
#[derive(Debug, Clone)]
struct CachedGasPrice {
    gas_price_gwei: f64,
    timestamp: u64,
}

/// Profit calculator configuration
pub struct ProfitCalculator {
    min_profit_usd: f64,
    protocol_fees: HashMap<String, f64>, // protocol_id -> fee in bps
    eth_price_usd: f64,                 // Cached ETH price
    warmbed_api_url: String,            // Warmbed API base URL
    http_client: reqwest::Client,       // HTTP client for API calls
    gas_price_cache: Arc<RwLock<HashMap<u64, CachedGasPrice>>>, // chain_id -> cached price
}

impl ProfitCalculator {
    /// Create new profit calculator
    pub fn new(min_profit_usd: f64) -> Self {
        Self {
            min_profit_usd,
            protocol_fees: HashMap::new(),
            eth_price_usd: 3000.0, // Default, will fetch real price
            warmbed_api_url: std::env::var("WARMBED_API_URL")
                .unwrap_or_else(|_| "https://api.taifoon.dev".to_string()),
            http_client: reqwest::Client::new(),
            gas_price_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Fetch real gas price from Warmbed API with caching
    async fn fetch_gas_price(&self, chain_id: u64) -> Result<f64> {
        const CACHE_TTL_SECS: u64 = 30; // Cache for 30 seconds

        // Check cache first
        {
            let cache = self.gas_price_cache.read().await;
            if let Some(cached) = cache.get(&chain_id) {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();

                if now - cached.timestamp < CACHE_TTL_SECS {
                    tracing::debug!("💾 Using cached gas price for chain {}: {} gwei", chain_id, cached.gas_price_gwei);
                    return Ok(cached.gas_price_gwei);
                }
            }
        }

        // Fetch from API
        let url = format!("{}/api/gas/latest/{}", self.warmbed_api_url, chain_id);
        tracing::debug!("🌐 Fetching gas price from Warmbed API: {}", url);

        match self.http_client
            .get(&url)
            .timeout(std::time::Duration::from_secs(2))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<GasResponse>().await {
                    Ok(gas_data) => {
                        let gas_price_gwei = gas_data.gas_price_gwei.unwrap_or(25.0);

                        // Update cache
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs();

                        let mut cache = self.gas_price_cache.write().await;
                        cache.insert(chain_id, CachedGasPrice {
                            gas_price_gwei,
                            timestamp: now,
                        });

                        tracing::info!("✅ Warmbed API returned gas price for chain {}: {} gwei (block {})",
                            chain_id, gas_price_gwei, gas_data.block_number);
                        Ok(gas_price_gwei)
                    }
                    Err(e) => {
                        tracing::warn!("⚠️  Failed to parse Warmbed API response: {}", e);
                        Err(anyhow::anyhow!("Failed to parse gas response"))
                    }
                }
            }
            Ok(resp) => {
                tracing::warn!("⚠️  Warmbed API returned error status: {}", resp.status());
                Err(anyhow::anyhow!("API returned error status"))
            }
            Err(e) => {
                tracing::warn!("⚠️  Failed to fetch from Warmbed API: {}", e);
                Err(anyhow::anyhow!("Failed to fetch gas price"))
            }
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

        // 2. Parse amount and detect token decimals - WITH OVERFLOW DETECTION
        let amount_raw: u128 = intent.amount.parse()
            .context(format!("Failed to parse amount '{}' as u128 (possible overflow or invalid format)", intent.amount))?;

        // Validate amount is not zero (catches bugs early)
        if amount_raw == 0 {
            anyhow::bail!("Intent has zero amount - invalid or parsing failed (original: '{}')", intent.amount);
        }

        // Detect decimals from token address
        let decimals = self.detect_token_decimals(&intent.src_token, intent.src_chain);
        let token_price_usd = self.get_token_price(&intent.src_token, intent.src_chain);

        // Convert to human-readable amount
        let divisor = 10_f64.powi(decimals as i32);
        let amount_human = amount_raw as f64 / divisor;
        let amount_usd = amount_human * token_price_usd;

        tracing::info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        tracing::info!("📊 VERBOSE AMOUNT DECODING:");
        tracing::info!("   Token Address: {}", intent.src_token);
        tracing::info!("   Chain ID: {}", intent.src_chain);
        tracing::info!("   ────────────────────────────────────────────────────────────────");
        tracing::info!("   RAW AMOUNT (wei/smallest unit): {}", amount_raw);
        tracing::info!("   DETECTED DECIMALS: {}", decimals);
        tracing::info!("   CONVERSION DIVISOR: 10^{} = {}", decimals, divisor);
        tracing::info!("   ────────────────────────────────────────────────────────────────");
        tracing::info!("   HUMAN-READABLE AMOUNT: {:.18}", amount_human);
        tracing::info!("   (full precision: {} / {} = {:.18})", amount_raw, divisor, amount_human);
        tracing::info!("   ────────────────────────────────────────────────────────────────");
        tracing::info!("   TOKEN PRICE (USD): ${:.6}", token_price_usd);
        tracing::info!("   TOTAL VALUE (USD): ${:.6}", amount_usd);
        tracing::info!("   (calculation: {:.18} × ${:.6} = ${:.6})", amount_human, token_price_usd, amount_usd);
        tracing::info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

        // 3. Calculate protocol fee in USD
        let protocol_fee_usd = amount_usd * (protocol_fee_bps / 10000.0);

        // 4. Estimate gas costs
        let (src_gas_usd, dst_gas_usd) = self.estimate_gas_costs(intent).await;
        let total_gas_usd = src_gas_usd + dst_gas_usd;

        // 5. Calculate spread (for now, assume 0 - user gets exact amount they requested)
        let spread_usd = 0.0;

        // 6. Liquidity cost (own funds = 0)
        let liquidity_cost_usd = 0.0;

        // 7. Net profit
        let net_profit_usd = protocol_fee_usd + spread_usd - total_gas_usd - liquidity_cost_usd;

        tracing::info!("💰 PROFIT CALCULATION:");
        tracing::info!("   Protocol Fee ({} bps): ${:.6}", protocol_fee_bps, protocol_fee_usd);
        tracing::info!("   (calculation: ${:.6} × {} / 10000 = ${:.6})", amount_usd, protocol_fee_bps, protocol_fee_usd);
        tracing::info!("   Gas Cost (src: ${:.2} + dst: ${:.2}): ${:.2}", src_gas_usd, dst_gas_usd, total_gas_usd);
        tracing::info!("   Spread: ${:.2}", spread_usd);
        tracing::info!("   Liquidity Cost: ${:.2}", liquidity_cost_usd);
        tracing::info!("   ────────────────────────────────────────────────────────────────");
        tracing::info!("   NET PROFIT: ${:.6}", net_profit_usd);
        tracing::info!("   (calculation: ${:.6} + ${:.2} - ${:.2} - ${:.2} = ${:.6})",
            protocol_fee_usd, spread_usd, total_gas_usd, liquidity_cost_usd, net_profit_usd);
        tracing::info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

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

    async fn estimate_gas_costs(&self, intent: &Intent) -> (f64, f64) {
        let src_gas_usd = self.estimate_chain_gas_cost(intent.src_chain).await;
        let dst_gas_usd = self.estimate_chain_gas_cost(intent.dst_chain).await;

        (src_gas_usd, dst_gas_usd)
    }

    async fn estimate_chain_gas_cost(&self, chain_id: u64) -> f64 {
        // Estimate gas units for a typical cross-chain fill transaction
        let estimated_gas_units = match chain_id {
            1 => 150_000u64,   // Ethereum mainnet (complex contract calls)
            10 => 100_000u64,  // Optimism
            8453 => 100_000u64, // Base
            42161 => 100_000u64, // Arbitrum
            _ => 120_000u64,   // Default
        };

        tracing::debug!("📊 Estimating gas cost for chain {} (estimated {} gas units)", chain_id, estimated_gas_units);

        // Try to fetch real gas price from Warmbed API
        match self.fetch_gas_price(chain_id).await {
            Ok(gas_price_gwei) => {
                // Calculate cost in USD
                // Formula: gas_units × (gas_price_gwei / 1e9) = ETH cost
                // Then: ETH cost × ETH_price_USD = USD cost
                let gas_price_eth = gas_price_gwei / 1_000_000_000.0; // Convert gwei to ETH
                let gas_cost_eth = (estimated_gas_units as f64) * gas_price_eth;
                let gas_cost_usd = gas_cost_eth * self.eth_price_usd;

                tracing::info!("💰 Real gas cost for chain {}: {} gwei (={:.9} ETH/gas) × {} units = {:.6} ETH = ${:.2}",
                    chain_id, gas_price_gwei, gas_price_eth, estimated_gas_units, gas_cost_eth, gas_cost_usd);

                gas_cost_usd
            }
            Err(_) => {
                // Fallback to hardcoded estimates if API fails
                let fallback = match chain_id {
                    1 => 5.0,      // Ethereum mainnet (expensive)
                    10 => 0.05,    // Optimism (cheap)
                    8453 => 0.05,  // Base (cheap)
                    42161 => 0.10, // Arbitrum (cheap)
                    _ => 1.0,      // Default
                };

                tracing::warn!("⚠️  Using fallback gas estimate for chain {}: ${:.2} (Warmbed API unavailable)",
                    chain_id, fallback);

                fallback
            }
        }
    }

    /// Detect token decimals from address
    fn detect_token_decimals(&self, token_addr: &str, chain_id: u64) -> u8 {
        let addr_lower = token_addr.to_lowercase();

        tracing::debug!("🔍 Detecting decimals for token: {} on chain {}", addr_lower, chain_id);

        // 6-decimal stablecoins (USDC / USDT across all chains)
        const SIX_DEC: &[&str] = &[
            "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48", // USDC mainnet
            "0xaf88d065e77c8cc2239327c5edb3a432268e5831", // USDC Arbitrum native
            "0xff970a61a04b1ca14834a43f5de4533ebddb5cc8", // USDC.e Arbitrum bridged
            "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913", // USDC Base
            "0x0b2c639c533813f4aa9d7837caf62653d097ff85", // USDC Optimism native
            "0x7f5c764cbc14f9669b88837ca1490cca17c31607", // USDC.e Optimism bridged
            "0x3c499c542cef5e3811e1192ce70d8cc03d5c3359", // USDC Polygon native
            "0x2791bca1f2de4661ed88a30c99a7a9449aa84174", // USDC.e Polygon bridged
            "0x176211869ca2b568f2a7d4ee941e073a821ee1ff", // USDC Linea
            "0x06efdbff2a14a7c8e15944d1f4a48f9f95f663a4", // USDC Scroll
            "0x2a22f9c3b484c3629090feed35f17ff8f88f76f0", // USDC.e Gnosis
            "0xddafbb505ad214d7b80b1f830fccc89b60fb7a83", // USDC native Gnosis
            "0x1c7d4b196cb0c7b01d743fbc6116a902379c7238", // USDC Sepolia
            "0x036cbd53842c5426634e7929541ec2318f3dcf7e", // USDC Base Sepolia
            "0xdac17f958d2ee523a2206206994597c13d831ec7", // USDT mainnet
            "0xfd086bc7cd5c481dcc9c85ebe478a1c0b69fcbb9", // USDT Arbitrum
            "0x94b008aa00579c1307b0ef2c499ad98a8ce58e58", // USDT Optimism
            "0x55d398326f99059ff775485246999027b3197955", // USDT BSC
            "0x049d68029688eabf473097a2fc38ef61633a3c7a", // USDT Fantom
            "0xc2132d05d31c914a87c6611c10748aeb04b58e8f", // USDT Polygon
            "usdc", "usdt",
        ];
        if SIX_DEC.contains(&addr_lower.as_str()) {
            tracing::info!("✅ Detected 6-decimal stablecoin: {}", addr_lower);
            return 6;
        }

        // Native tokens and most ERC20s (18 decimals)
        tracing::debug!("ℹ️  Defaulting to 18 decimals for token: {}", addr_lower);
        18
    }

    /// Get token price in USD
    fn get_token_price(&self, token_addr: &str, chain_id: u64) -> f64 {
        let addr_lower = token_addr.to_lowercase();

        // Stablecoins (USDC, USDT, DAI, etc.) — reuse the same 6-dec list
        const STABLECOINS: &[&str] = &[
            "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
            "0xaf88d065e77c8cc2239327c5edb3a432268e5831",
            "0xff970a61a04b1ca14834a43f5de4533ebddb5cc8",
            "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913",
            "0x0b2c639c533813f4aa9d7837caf62653d097ff85",
            "0x7f5c764cbc14f9669b88837ca1490cca17c31607",
            "0x3c499c542cef5e3811e1192ce70d8cc03d5c3359",
            "0x2791bca1f2de4661ed88a30c99a7a9449aa84174",
            "0x176211869ca2b568f2a7d4ee941e073a821ee1ff",
            "0x06efdbff2a14a7c8e15944d1f4a48f9f95f663a4",
            "0x2a22f9c3b484c3629090feed35f17ff8f88f76f0",
            "0xddafbb505ad214d7b80b1f830fccc89b60fb7a83",
            "0x1c7d4b196cb0c7b01d743fbc6116a902379c7238",
            "0x036cbd53842c5426634e7929541ec2318f3dcf7e",
            "0xdac17f958d2ee523a2206206994597c13d831ec7",
            "0xfd086bc7cd5c481dcc9c85ebe478a1c0b69fcbb9",
            "0x94b008aa00579c1307b0ef2c499ad98a8ce58e58",
            "0x55d398326f99059ff775485246999027b3197955",
            "0x049d68029688eabf473097a2fc38ef61633a3c7a",
            "0xc2132d05d31c914a87c6611c10748aeb04b58e8f",
            // DAI
            "0x6b175474e89094c44da98b954eedeac495271d0f",
            "0xda10009cbd5d07dd0cecc66161fc93d7c9000da1", // DAI Arbitrum
            "0xe91d153e0b41518a2ce8dd3d7944fa863463a97d", // WXDAI Gnosis
            "usdc", "usdt", "dai",
        ];
        let is_stablecoin = STABLECOINS.contains(&addr_lower.as_str());

        if is_stablecoin {
            tracing::info!("💵 Token price: $1.00 (stablecoin)");
            return 1.0;
        }

        // ETH/WETH/Native (use cached ETH price)
        tracing::info!("💵 Token price: ${:.2} (ETH/WETH)", self.eth_price_usd);
        self.eth_price_usd
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
            ..Default::default()
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
