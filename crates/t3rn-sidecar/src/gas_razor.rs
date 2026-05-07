//! 16-probe binary-sweep gas estimator with EIP-1559 Type-2 pricing.
//!
//! Copied gas helper functions from executor/src/evm_estimate.rs to avoid
//! circular dependency (executor depends on t3rn-sidecar).

use alloy::{
    primitives::{Address, Bytes},
    providers::{Provider, ProviderBuilder},
    rpc::types::TransactionRequest,
};
use futures::future::join_all;
use tracing::warn;

// ── Per-chain gas price ranges ─────────────────────────────────────────────

pub struct GasPriceRange {
    pub min_gwei:     f64,
    pub max_gwei:     f64,
    pub typical_gwei: f64,
}

fn gas_price_range_for_chain(chain_id: u64) -> GasPriceRange {
    match chain_id {
        1      => GasPriceRange { min_gwei: 1.0,   max_gwei: 200.0, typical_gwei: 10.0  },
        10     => GasPriceRange { min_gwei: 0.001, max_gwei: 0.5,   typical_gwei: 0.001 },
        56     => GasPriceRange { min_gwei: 1.0,   max_gwei: 20.0,  typical_gwei: 3.0   },
        130    => GasPriceRange { min_gwei: 0.001, max_gwei: 0.5,   typical_gwei: 0.001 }, // Unichain
        137    => GasPriceRange { min_gwei: 30.0,  max_gwei: 500.0, typical_gwei: 50.0  },
        8453   => GasPriceRange { min_gwei: 0.001, max_gwei: 0.5,   typical_gwei: 0.05  },
        42161  => GasPriceRange { min_gwei: 0.01,  max_gwei: 0.5,   typical_gwei: 0.1   },
        59144  => GasPriceRange { min_gwei: 0.05,  max_gwei: 5.0,   typical_gwei: 0.1   },
        _      => GasPriceRange { min_gwei: 0.001, max_gwei: 200.0, typical_gwei: 1.0   },
    }
}

fn default_rpc_for_chain(chain_id: u64) -> Option<&'static str> {
    match chain_id {
        1     => Some("https://ethereum-rpc.publicnode.com"),
        10    => Some("https://mainnet.optimism.io"),
        42161 => Some("https://arb1.arbitrum.io/rpc"),
        8453  => Some("https://base-rpc.publicnode.com"),
        137   => Some("https://polygon-bor-rpc.publicnode.com"),
        56    => Some("https://bsc-dataseed.binance.org"),
        59144 => Some("https://rpc.linea.build"),
        130   => Some("https://mainnet.unichain.org"),
        _     => None,
    }
}

pub fn resolve_rpc_url(chain_id: u64) -> Option<String> {
    if let Ok(url) = std::env::var(format!("RPC_URL_{}", chain_id)) {
        if !url.is_empty() { return Some(url); }
    }
    if chain_id == 1 {
        if let Ok(url) = std::env::var("ETH_RPC_URL") {
            if !url.is_empty() { return Some(url); }
        }
    }
    default_rpc_for_chain(chain_id).map(String::from)
}

async fn fetch_razor_gas_price_gwei(chain_id: u64, warmbed_base: &str) -> Option<f64> {
    #[derive(serde::Deserialize)]
    struct GasResp { gas_price_gwei: Option<f64> }

    let url = format!("{}/api/gas/latest/{}", warmbed_base.trim_end_matches('/'), chain_id);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build().ok()?;
    let resp = client.get(&url).send().await.ok()?;
    if !resp.status().is_success() { return None; }
    resp.json::<GasResp>().await.ok()?.gas_price_gwei
}

async fn fetch_rpc_gas_price_gwei(chain_id: u64) -> Option<f64> {
    let rpc_url = resolve_rpc_url(chain_id)?;
    let provider = ProviderBuilder::new().on_http(rpc_url.parse().ok()?);
    let wei = provider.get_gas_price().await.ok()?;
    Some(wei as f64 / 1_000_000_000.0)
}

// ── Public API ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct GasParams {
    pub gas_limit:       u64,
    pub max_fee_per_gas: u128,
    pub priority_fee:    u128,
}

/// 16-probe binary sweep to find the minimum gas_limit that succeeds,
/// then price with EIP-1559 from Razor API → RPC → static fallback.
pub async fn estimate(chain_id: u64, calldata: Bytes, to: Address) -> GasParams {
    let gas_limit = sweep_gas_limit(chain_id, calldata, to).await;
    let (max_fee_per_gas, priority_fee) = eip1559_price(chain_id).await;
    GasParams { gas_limit, max_fee_per_gas, priority_fee }
}

/// Run 16 parallel eth_estimateGas probes with graduated gas caps.
/// Returns the recommended gas_limit (breakpoint × 1.1, capped at 3M).
pub async fn sweep_gas_limit(chain_id: u64, calldata: Bytes, to: Address) -> u64 {
    const MIN: u64 = 21_000;
    const MAX: u64 = 3_000_000;
    const PROBES: u64 = 16;
    let step = (MAX - MIN) / (PROBES - 1);

    let rpc_url = match resolve_rpc_url(chain_id) {
        Some(u) => u,
        None => {
            warn!("[gas_razor] no RPC for chain={}, using default 300k", chain_id);
            return 300_000;
        }
    };
    let url = match rpc_url.parse() {
        Ok(u) => u,
        Err(_) => return 300_000,
    };

    let provider = ProviderBuilder::new().on_http(url);

    // Build 16 probes: [MIN, MIN+step, MIN+2*step, ..., MAX]
    let caps: Vec<u64> = (0..PROBES).map(|i| MIN + i * step).collect();

    let futs: Vec<_> = caps.iter().map(|&cap| {
        let req = TransactionRequest::default()
            .to(to)
            .input(calldata.clone().into())
            .gas_limit(cap);
        let p = provider.clone();
        async move {
            match p.estimate_gas(&req).await {
                Ok(g) => Some((cap, g)),
                Err(_) => None,
            }
        }
    }).collect();

    let results = join_all(futs).await;

    // Collect successful estimates
    let successes: Vec<(u64, u64)> = results.into_iter().flatten().collect();

    if successes.is_empty() {
        // All failed — return max as conservative fallback
        warn!("[gas_razor] all 16 probes failed chain={}, using 3M", chain_id);
        return MAX;
    }

    // Breakpoint = minimum cap at which estimate succeeded
    let breakpoint = successes.iter().map(|(cap, _)| *cap).min().unwrap_or(MAX);

    // Median of successful estimated values
    let mut estimates: Vec<u64> = successes.iter().map(|(_, g)| *g).collect();
    estimates.sort_unstable();
    let median = estimates[estimates.len() / 2];

    // recommended = max(median × 1.1, breakpoint), capped at MAX
    let recommended = ((median as f64 * 1.1) as u64).max(breakpoint).min(MAX);

    tracing::info!(
        "[gas_razor] chain={} breakpoint={} median={} recommended={}",
        chain_id, breakpoint, median, recommended
    );

    recommended
}

/// Compute EIP-1559 max_fee_per_gas and priority_fee for the given chain.
///
/// Priority: Razor API → eth_gasPrice RPC → static typical.
/// max_fee = base_price × 2 (EIP-1559 headroom for baseFee spikes).
/// priority = max(1_gwei, range.min_gwei × 1e9).
pub async fn eip1559_price(chain_id: u64) -> (u128, u128) {
    let range = gas_price_range_for_chain(chain_id);

    let warmbed_base = std::env::var("WARMBED_API_URL")
        .or_else(|_| std::env::var("SPINNER_API_URL"))
        .unwrap_or_else(|_| "https://api.taifoon.dev".to_string());

    let base_gwei = if let Some(live) = fetch_razor_gas_price_gwei(chain_id, &warmbed_base).await {
        let clamped = live.max(range.min_gwei).min(range.max_gwei);
        clamped
    } else if let Some(rpc) = fetch_rpc_gas_price_gwei(chain_id).await {
        let clamped = rpc.max(range.min_gwei).min(range.max_gwei);
        warn!("[gas_razor] Razor unavailable chain={}, using eth_gasPrice={:.4}gwei", chain_id, rpc);
        clamped
    } else {
        warn!("[gas_razor] both Razor and RPC failed chain={}, using typical={:.4}gwei", chain_id, range.typical_gwei);
        range.typical_gwei
    };

    // EIP-1559: max_fee = baseFee × 2 (gives 100% headroom for baseFee increases)
    let max_fee_per_gas = (base_gwei * 2.0 * 1_000_000_000.0) as u128;
    // Priority fee: floor at 1 gwei or range minimum
    let min_priority = (range.min_gwei * 1_000_000_000.0) as u128;
    let priority_fee = min_priority.max(1_000_000_000u128); // at least 1 gwei

    (max_fee_per_gas, priority_fee)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gas_range_unichain() {
        let r = gas_price_range_for_chain(130);
        assert!(r.min_gwei < 0.01);
    }

    #[test]
    fn gas_range_polygon() {
        let r = gas_price_range_for_chain(137);
        assert!(r.typical_gwei > 10.0);
    }

    #[tokio::test]
    #[ignore] // live network
    async fn sweep_base_mainnet() {
        let calldata = alloy::primitives::Bytes::from(vec![0u8; 4]);
        let to: Address = "0xb590266eCdbc389A35831dDc672Ea0C5f45500EF".parse().unwrap();
        let params = estimate(8453, calldata, to).await;
        assert!(params.gas_limit >= 21_000);
        assert!(params.gas_limit <= 3_000_000);
        assert!(params.max_fee_per_gas > 0);
        println!("Base gas params: {:?}", params);
    }
}
