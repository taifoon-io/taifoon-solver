use anyhow::{Context, Result};
use genome_client::Intent;
use serde::Deserialize;
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
}

#[derive(Debug, Clone, Deserialize)]
struct GasResponse {
    #[allow(dead_code)]
    chain_id: u64,
    #[allow(dead_code)]
    block_number: u64,
    #[allow(dead_code)]
    timestamp: u64,
    #[allow(dead_code)]
    base_fee_per_gas_wei: Option<String>,
    gas_price_gwei: Option<f64>,
    #[allow(dead_code)]
    gas_used: u64,
    #[allow(dead_code)]
    gas_limit: u64,
    #[allow(dead_code)]
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
            eth_price_usd: std::env::var("ETH_PRICE_USD")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3000.0),
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
                    .unwrap_or_default()
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
                            .unwrap_or_default()
                            .as_secs();

                        let mut cache = self.gas_price_cache.write().await;
                        cache.insert(chain_id, CachedGasPrice {
                            gas_price_gwei,
                            timestamp: now,
                        });

                        tracing::debug!("profit-calc: gas chain={} gwei={} block={}",
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

        tracing::debug!("profit-calc: token={} chain={} raw={} decimals={} human={:.6} price=${:.4} usd=${:.4}",
            intent.src_token, intent.src_chain, amount_raw, decimals, amount_human, token_price_usd, amount_usd);

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

        tracing::debug!("profit-calc: fee_bps={} fee_usd=${:.4} gas_usd=${:.4} net=${:.4}",
            protocol_fee_bps, protocol_fee_usd, total_gas_usd, net_profit_usd);

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
        // The solver submits one fill tx on the destination chain only.
        // Source-chain gas is paid by the depositor — not our cost.
        let dst_gas_usd = self.estimate_chain_gas_cost(intent.dst_chain, &intent.protocol).await;
        (0.0, dst_gas_usd)
    }

    async fn estimate_chain_gas_cost(&self, chain_id: u64, protocol: &str) -> f64 {
        // Protocol-aware gas units: deBridge ~300k (OrderManager + DlnDestination),
        // Mayan ~250k (fulfillSimple + ATA setup), Across ~200k (fillRelay).
        let proto_lower = protocol.to_lowercase();
        let protocol_gas_units: u64 = if proto_lower.contains("debridge") || proto_lower.contains("dln") {
            300_000
        } else if proto_lower.contains("mayan") {
            250_000
        } else {
            // Across and default
            200_000
        };

        // Ethereum L1 intrinsic costs are higher; scale down L2s.
        let estimated_gas_units = if chain_id == 1 {
            protocol_gas_units.saturating_add(50_000)
        } else {
            protocol_gas_units
        };

        match self.fetch_gas_price(chain_id).await {
            Ok(gas_price_gwei) => {
                let gas_cost_eth = (estimated_gas_units as f64) * (gas_price_gwei / 1_000_000_000.0);
                let gas_cost_usd = gas_cost_eth * self.eth_price_usd;
                tracing::debug!("profit-calc: chain={} proto={} gwei={} units={} gas_usd=${:.4}",
                    chain_id, protocol, gas_price_gwei, estimated_gas_units, gas_cost_usd);
                gas_cost_usd
            }
            Err(_) => {
                let fallback = match chain_id {
                    1 => 1.20,
                    10 => 0.02,
                    8453 => 0.02,
                    42161 => 0.05,
                    137 => 0.05,
                    56 => 0.05,
                    _ => 0.30,
                };
                tracing::warn!("profit-calc: gas API unavailable for chain={}, fallback=${:.2}", chain_id, fallback);
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
            "0x078d782b760474a361dda7ff6e249887ddf39eb0", // USDC Unichain
            "0x2d270e6886d130d724215a266106e6832161eaed", // USDC Ink
            "0xd988097fb8612cc24eec14542bc03424c656005f", // USDC.e Mode
            "0x9c3c9283d3e44854697cd22d3faa240cfb032889", // USDC Polygon zkEVM
            "0xe0b7927c4af23765cb51314a0e0521a9645f0e2a", // USDC.e Avalanche (old)
            "0xb97ef9ef8734c71904d8002f8b6bc66dd9c48a6e", // USDC Avalanche native
            "0x1d17cbcf0d6d143135ae902365d2e5e2a16538d4", // USDC zkSync Era
            "0xfde4c96c8593536e31f229ea8f37b2ada2699bb2", // USDT Base
            "0xf55bec9cafdbe8730f096aa55dad6d22d44099df", // USDT Scroll
            "0x0200c29006150606b650577bbe7b6248f58470c1", // USDT Ink
            "0xf0f161fda2712db8b566946122a5af183995e2ed", // USDT Mode
            "0x9702230a8ea53601f5cd2dc00fdbc13d4df4a8c7", // USDT Avalanche native
            "0xc7198437980c041c805a1edcba50c1ce5db95118", // USDT.e Avalanche
            "usdc", "usdt",
        ];
        if SIX_DEC.contains(&addr_lower.as_str()) {
            return 6;
        }

        // Native tokens and most ERC20s (18 decimals)
        tracing::debug!("ℹ️  Defaulting to 18 decimals for token: {}", addr_lower);
        18
    }

    /// Get token price in USD
    fn get_token_price(&self, token_addr: &str, _chain_id: u64) -> f64 {
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
            "0x078d782b760474a361dda7ff6e249887ddf39eb0", // USDC Unichain
            "0x2d270e6886d130d724215a266106e6832161eaed", // USDC Ink
            "0xd988097fb8612cc24eec14542bc03424c656005f", // USDC.e Mode
            "0x9c3c9283d3e44854697cd22d3faa240cfb032889", // USDC Polygon zkEVM
            "0xe0b7927c4af23765cb51314a0e0521a9645f0e2a", // USDC.e Avalanche (old)
            "0xb97ef9ef8734c71904d8002f8b6bc66dd9c48a6e", // USDC Avalanche native
            "0x1d17cbcf0d6d143135ae902365d2e5e2a16538d4", // USDC zkSync Era
            "0xfde4c96c8593536e31f229ea8f37b2ada2699bb2", // USDT Base
            "0xf55bec9cafdbe8730f096aa55dad6d22d44099df", // USDT Scroll
            "0x0200c29006150606b650577bbe7b6248f58470c1", // USDT Ink
            "0xf0f161fda2712db8b566946122a5af183995e2ed", // USDT Mode
            "0x9702230a8ea53601f5cd2dc00fdbc13d4df4a8c7", // USDT Avalanche native
            "0xc7198437980c041c805a1edcba50c1ce5db95118", // USDT.e Avalanche
            // DAI
            "0x6b175474e89094c44da98b954eedeac495271d0f",
            "0xda10009cbd5d07dd0cecc66161fc93d7c9000da1", // DAI Arbitrum
            "0xe91d153e0b41518a2ce8dd3d7944fa863463a97d", // WXDAI Gnosis
            "usdc", "usdt", "dai",
        ];
        let is_stablecoin = STABLECOINS.contains(&addr_lower.as_str());

        if is_stablecoin {
            tracing::debug!("profit-calc: token={} price=$1.00 (stablecoin)", addr_lower);
            return 1.0;
        }

        // ETH/WETH/Native (use cached ETH price)
        tracing::debug!("profit-calc: token={} price=${:.2} (eth)", addr_lower, self.eth_price_usd);
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
