use anyhow::Result;
use colored::Colorize;
use serde::{Deserialize, Serialize};

// Gas estimates per protocol fill call (from empirical measurements)
const GAS_ACROSS_FILL: u64 = 104_000;
const GAS_DEBRIDGE_FULFILL: u64 = 150_000;
const GAS_DEBRIDGE_UNLOCK: u64 = 80_000;
const GAS_LIFI_FILL: u64 = 120_000;
const GAS_MAYAN_UNLOCK: u64 = 100_000;

// Approximate ETH/SOL prices — overridable at runtime via ETH_PRICE_USD / SOL_PRICE_USD.
const ETH_USD_DEFAULT: f64 = 3_200.0;
const SOL_USD_DEFAULT: f64 = 150.0;
// Solana fixed fee per signature (~5000 lamports)
const SOL_LAMPORTS_PER_TX: u64 = 5_000;

#[derive(Debug, Deserialize)]
struct GasApiResponse {
    chain_id: Option<u64>,
    gas_price_gwei: f64,
    #[allow(dead_code)]
    block_number: Option<u64>,
    #[allow(dead_code)]
    used_fallback: Option<bool>,
}

#[derive(Serialize)]
pub struct ChainFees {
    pub chain_id: u64,
    pub chain_name: String,
    pub gas_price_gwei: f64,
    pub gas_price_wei: u64,
    // Across fillRelay
    pub across_fill_wei: u64,
    pub across_fill_eth: f64,
    pub across_fill_usdc: f64,
    // deBridge fulfillOrder
    pub debridge_fulfill_wei: u64,
    pub debridge_fulfill_eth: f64,
    pub debridge_fulfill_usdc: f64,
    // deBridge fulfillOrder + sendEvmUnlock combined
    pub debridge_total_wei: u64,
    pub debridge_total_eth: f64,
    pub debridge_total_usdc: f64,
    // LiFi fill
    pub lifi_fill_wei: u64,
    pub lifi_fill_eth: f64,
    pub lifi_fill_usdc: f64,
    // Mayan unlockSingle
    pub mayan_unlock_wei: u64,
    pub mayan_unlock_eth: f64,
    pub mayan_unlock_usdc: f64,
}

#[derive(Serialize)]
pub struct SolanaFees {
    pub chain: String,
    pub fee_lamports: u64,
    pub fee_sol: f64,
    pub fee_usdc: f64,
}

#[derive(Serialize)]
pub struct FeesReport {
    pub evm_chains: Vec<ChainFees>,
    pub solana: SolanaFees,
    pub eth_usd_used: f64,
    pub sol_usd_used: f64,
}

fn chain_name(id: u64) -> &'static str {
    match id {
        1 => "Ethereum",
        10 => "Optimism",
        56 => "BNB Chain",
        137 => "Polygon",
        8453 => "Base",
        42161 => "Arbitrum",
        _ => "Unknown",
    }
}

fn compute_chain_fees(chain_id: u64, gas_price_gwei: f64, eth_usd: f64) -> ChainFees {
    let gwei_per_eth: f64 = 1e9;
    let wei_per_eth: f64 = 1e18;

    let gas_price_wei = (gas_price_gwei * 1.2 * gwei_per_eth) as u64; // 1.2x buffer

    let cost_wei = |gas: u64| -> u64 { gas_price_wei * gas };
    let cost_eth = |gas: u64| -> f64 { (gas_price_wei as f64 * gas as f64) / wei_per_eth };
    let cost_usdc = |gas: u64| -> f64 { cost_eth(gas) * eth_usd };

    ChainFees {
        chain_id,
        chain_name: chain_name(chain_id).to_string(),
        gas_price_gwei,
        gas_price_wei: (gas_price_gwei * gwei_per_eth) as u64,

        across_fill_wei: cost_wei(GAS_ACROSS_FILL),
        across_fill_eth: cost_eth(GAS_ACROSS_FILL),
        across_fill_usdc: cost_usdc(GAS_ACROSS_FILL),

        debridge_fulfill_wei: cost_wei(GAS_DEBRIDGE_FULFILL),
        debridge_fulfill_eth: cost_eth(GAS_DEBRIDGE_FULFILL),
        debridge_fulfill_usdc: cost_usdc(GAS_DEBRIDGE_FULFILL),

        debridge_total_wei: cost_wei(GAS_DEBRIDGE_FULFILL + GAS_DEBRIDGE_UNLOCK),
        debridge_total_eth: cost_eth(GAS_DEBRIDGE_FULFILL + GAS_DEBRIDGE_UNLOCK),
        debridge_total_usdc: cost_usdc(GAS_DEBRIDGE_FULFILL + GAS_DEBRIDGE_UNLOCK),

        lifi_fill_wei: cost_wei(GAS_LIFI_FILL),
        lifi_fill_eth: cost_eth(GAS_LIFI_FILL),
        lifi_fill_usdc: cost_usdc(GAS_LIFI_FILL),

        mayan_unlock_wei: cost_wei(GAS_MAYAN_UNLOCK),
        mayan_unlock_eth: cost_eth(GAS_MAYAN_UNLOCK),
        mayan_unlock_usdc: cost_usdc(GAS_MAYAN_UNLOCK),
    }
}

pub async fn run(spinner_url: &str, json_mode: bool) -> Result<()> {
    let eth_usd: f64 = std::env::var("ETH_PRICE_USD")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(ETH_USD_DEFAULT);
    let sol_usd: f64 = std::env::var("SOL_PRICE_USD")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(SOL_USD_DEFAULT);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;

    // Try to fetch all chains at once
    let all_url = format!("{}/api/gas/latest", spinner_url);
    let chain_ids = [1u64, 10, 56, 137, 8453, 42161];

    let mut gas_map: std::collections::HashMap<u64, f64> = std::collections::HashMap::new();

    // Try bulk endpoint first
    if let Ok(resp) = client.get(&all_url).send().await {
        if let Ok(entries) = resp.json::<Vec<GasApiResponse>>().await {
            for entry in entries {
                if let Some(cid) = entry.chain_id {
                    gas_map.insert(cid, entry.gas_price_gwei);
                }
            }
        }
    }

    // Fill any missing chains individually
    for &cid in &chain_ids {
        if !gas_map.contains_key(&cid) {
            let url = format!("{}/api/gas/latest/{}", spinner_url, cid);
            if let Ok(resp) = client.get(&url).send().await {
                if let Ok(entry) = resp.json::<GasApiResponse>().await {
                    gas_map.insert(cid, entry.gas_price_gwei);
                }
            }
        }
    }

    let mut evm_chains: Vec<ChainFees> = chain_ids
        .iter()
        .map(|&cid| {
            let gwei = match gas_map.get(&cid).copied() {
                Some(g) => g,
                None => {
                    tracing::warn!("fees: no gas price for chain {} — using 25 gwei fallback", cid);
                    25.0
                }
            };
            compute_chain_fees(cid, gwei, eth_usd)
        })
        .collect();

    // Sort by chain ID for consistent output
    evm_chains.sort_by_key(|c| c.chain_id);

    let solana = SolanaFees {
        chain: "Solana".to_string(),
        fee_lamports: SOL_LAMPORTS_PER_TX,
        fee_sol: SOL_LAMPORTS_PER_TX as f64 / 1e9,
        fee_usdc: (SOL_LAMPORTS_PER_TX as f64 / 1e9) * sol_usd,
    };

    let report = FeesReport {
        evm_chains,
        solana,
        eth_usd_used: eth_usd,
        sol_usd_used: sol_usd,
    };

    if json_mode {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    print_human_fees(&report);
    Ok(())
}

fn print_human_fees(report: &FeesReport) {
    println!("{}", "═══════════════════════════════════════════════════════════".cyan());
    println!("{}", "  FILL COST ESTIMATES  (gas × 1.2 buffer)".cyan().bold());
    println!("{}",
        format!("  ETH=${:.0}  SOL=${:.0}", report.eth_usd_used, report.sol_usd_used).cyan());
    println!("{}", "═══════════════════════════════════════════════════════════".cyan());

    // Header
    println!("{:<12} {:>12} {:>20} {:>20} {:>20}",
        "Chain".bold(),
        "Gas (gwei)".bold(),
        "Across fill".bold(),
        "deBridge (fill+unlock)".bold(),
        "LiFi fill".bold(),
    );
    println!("{}", "─────────────────────────────────────────────────────────────────────────────────────────────".dimmed());

    for chain in &report.evm_chains {
        let label = format!("{} ({})", chain.chain_name, chain.chain_id);
        println!("{:<22} {:>10.4}  {:<24} {:<24} {:<24}",
            label.yellow(),
            chain.gas_price_gwei,
            format!("{:.0} wei / ${:.4}", chain.across_fill_wei, chain.across_fill_usdc),
            format!("{:.0} wei / ${:.4}", chain.debridge_total_wei, chain.debridge_total_usdc),
            format!("{:.0} wei / ${:.4}", chain.lifi_fill_wei, chain.lifi_fill_usdc),
        );
    }

    println!();
    println!("{}", "─── Solana ─────────────────────────────────────────────────".dimmed());
    println!("{:<22} {:>10}  {} lamports / {:.8} SOL / ${:.6}",
        "Solana".yellow(),
        "~0.000005",
        report.solana.fee_lamports,
        report.solana.fee_sol,
        report.solana.fee_usdc,
    );
    println!();
    println!("{}", "Note: gas prices fetched from WARMBED_API_URL (Razor API). EVM costs use 104k–150k gas estimates.".dimmed());
    println!("{}", "      Actual costs vary — always estimateGas before every tx.".dimmed());
}
