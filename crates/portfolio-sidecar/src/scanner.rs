//! Live balance scanner — reads ERC-20 and native ETH balances across all chains.

use alloy::primitives::Address;
use serde::{Deserialize, Serialize};

const WETH_PRICE_USD_DEFAULT: f64 = 3000.0;

pub fn weth_price_usd() -> f64 {
    std::env::var("ETH_PRICE_USD")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(WETH_PRICE_USD_DEFAULT)
}


/// Token addresses per chain for the fill-path tokens the solver uses.
pub struct ChainTokens {
    pub chain_id: u64,
    pub rpc: &'static str,
    /// Primary stable (USDC or USDT, 6 dec) — used for fills.
    pub primary_stable: Option<(&'static str, u32)>,
    /// Secondary stable — counted in total but not used as bridge source.
    pub secondary_stable: Option<(&'static str, u32)>,
    /// WETH (18 dec) — counted at spot price.
    pub weth: Option<&'static str>,
}

pub fn chain_token_map() -> Vec<ChainTokens> {
    vec![
        ChainTokens {
            chain_id: 8453,
            rpc: "https://mainnet.base.org",
            primary_stable: Some(("0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913", 6)), // USDC
            secondary_stable: Some(("0xfde4C96c8593536E31F229EA8f37b2ADa2699bb2", 6)), // USDT (bridged)
            weth: Some("0x4200000000000000000000000000000000000006"),
        },
        ChainTokens {
            chain_id: 42161,
            rpc: "https://arb1.arbitrum.io/rpc",
            primary_stable: Some(("0xaf88d065e77c8cC2239327C5EDb3A432268e5831", 6)), // USDC
            secondary_stable: Some(("0xFd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9", 6)), // USDT
            weth: Some("0x82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
        },
        ChainTokens {
            chain_id: 10,
            rpc: "https://mainnet.optimism.io",
            primary_stable: Some(("0x0b2C639c533813f4Aa9D7837CAf62653d097Ff85", 6)), // USDC
            secondary_stable: Some(("0x94b008aA00579c1307B0EF2c499aD98a8ce58e58", 6)), // USDT
            weth: Some("0x4200000000000000000000000000000000000006"),
        },
        // Src-only chains — scanned for stray balances
        ChainTokens {
            chain_id: 1,
            rpc: "https://eth.llamarpc.com",
            primary_stable: Some(("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48", 6)),
            secondary_stable: Some(("0xdAC17F958D2ee523a2206206994597C13D831ec7", 6)),
            weth: Some("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"),
        },
        ChainTokens {
            chain_id: 137,
            rpc: "https://polygon-bor-rpc.publicnode.com",
            primary_stable: Some(("0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174", 6)),
            secondary_stable: Some(("0xc2132D05D31c914a87C6611C10748AEb04B58e8F", 6)),
            weth: None,
        },
        ChainTokens {
            chain_id: 324,
            rpc: "https://mainnet.era.zksync.io",
            primary_stable: Some(("0x1d17CBcF0D6D143135aE902365D2E5e2A16538D4", 6)),
            secondary_stable: Some(("0x493257fD37EDB34451f62EDf8D2a0C418852bA4C", 6)),
            weth: None,
        },
        ChainTokens {
            chain_id: 59144,
            rpc: "https://rpc.linea.build",
            primary_stable: Some(("0x176211869cA2b568f2A7D4EE941E073a821EE1ff", 6)),
            secondary_stable: None,
            weth: None,
        },
        ChainTokens {
            chain_id: 534352,
            rpc: "https://rpc.scroll.io",
            primary_stable: Some(("0x06eFdBFf2a14a7c8E15944D1F4A48F9F95F663A4", 6)),
            secondary_stable: None,
            weth: None,
        },
    ]
}

/// Snapshot of a single chain's balances at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainSnapshot {
    pub chain_id: u64,
    /// Combined stable USD (primary + secondary).
    pub stable_usd: f64,
    /// Primary stable raw (for bridge decisions — use as bridge source).
    pub primary_stable_raw: u128,
    pub primary_stable_addr: String,
    pub primary_stable_decimals: u32,
    pub primary_stable_usd: f64,
    /// Secondary stable (e.g. USDT on Arbitrum/Polygon).
    pub secondary_stable_usd: f64,
    pub secondary_stable_raw: u128,
    pub secondary_stable_addr: String,
    pub secondary_stable_decimals: u32,
    /// Native gas in ETH units.
    pub gas_eth: f64,
    /// WETH balance in USD.
    pub weth_usd: f64,
    /// WETH balance in raw wei (18 dec).
    pub weth_raw: u128,

    /// Best available bridge token: primary if non-zero, else secondary.
    /// Callers should use these fields rather than choosing manually.
    pub bridge_token_addr: String,
    pub bridge_token_decimals: u32,
    pub bridge_token_raw: u128,
    pub bridge_token_usd: f64,
}

/// Read all chain snapshots in parallel.
pub async fn scan_all(solver: Address) -> Vec<ChainSnapshot> {
    let http = reqwest::Client::builder()
        .user_agent("taifoon-portfolio-sidecar/1.0")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let tokens = chain_token_map();
    let futures = tokens.iter().map(|ct| {
        let http = http.clone();
        async move { scan_chain(&http, ct, solver).await }
    });
    futures::future::join_all(futures).await
}

async fn scan_chain(http: &reqwest::Client, ct: &ChainTokens, solver: Address) -> ChainSnapshot {
    let gas_eth = eth_balance(http, ct.rpc, solver).await.unwrap_or(0.0);

    let (primary_stable_raw, primary_stable_usd, primary_stable_addr, primary_stable_decimals) =
        if let Some((addr, dec)) = ct.primary_stable {
            let raw = erc20_balance_raw(http, ct.rpc, addr, solver).await.unwrap_or(0);
            let usd = raw as f64 / 10f64.powi(dec as i32);
            (raw, usd, addr.to_string(), dec)
        } else {
            (0, 0.0, String::new(), 6)
        };

    let (secondary_stable_raw, secondary_stable_usd, secondary_stable_addr, secondary_stable_decimals) =
        if let Some((addr, dec)) = ct.secondary_stable {
            let raw = erc20_balance_raw(http, ct.rpc, addr, solver).await.unwrap_or(0);
            let usd = raw as f64 / 10f64.powi(dec as i32);
            (raw, usd, addr.to_string(), dec)
        } else {
            (0, 0.0, String::new(), 6)
        };

    let (weth_usd, weth_raw) = if let Some(addr) = ct.weth {
        let raw = erc20_balance_raw(http, ct.rpc, addr, solver).await.unwrap_or(0);
        ((raw as f64 / 1e18) * weth_price_usd(), raw)
    } else {
        (0.0, 0)
    };

    // Best bridge token: pick whichever has the larger balance.
    // This handles cases where USDT arrived via bridge but USDC is near-zero.
    let (bridge_token_addr, bridge_token_decimals, bridge_token_raw, bridge_token_usd) =
        if primary_stable_usd >= secondary_stable_usd && primary_stable_raw > 0 {
            (primary_stable_addr.clone(), primary_stable_decimals, primary_stable_raw, primary_stable_usd)
        } else if secondary_stable_raw > 0 {
            (secondary_stable_addr.clone(), secondary_stable_decimals, secondary_stable_raw, secondary_stable_usd)
        } else {
            (primary_stable_addr.clone(), primary_stable_decimals, 0u128, 0.0)
        };

    ChainSnapshot {
        chain_id: ct.chain_id,
        stable_usd: primary_stable_usd + secondary_stable_usd,
        primary_stable_raw,
        primary_stable_addr,
        primary_stable_decimals,
        primary_stable_usd,
        secondary_stable_usd,
        secondary_stable_raw,
        secondary_stable_addr,
        secondary_stable_decimals,
        gas_eth,
        weth_usd,
        weth_raw,
        bridge_token_addr,
        bridge_token_decimals,
        bridge_token_raw,
        bridge_token_usd,
    }
}

async fn eth_balance(http: &reqwest::Client, rpc: &str, owner: Address) -> Option<f64> {
    let body = serde_json::json!({
        "jsonrpc": "2.0", "id": 1,
        "method": "eth_getBalance",
        "params": [format!("{:#x}", owner), "latest"]
    });
    let v: serde_json::Value = http.post(rpc).json(&body).send().await.ok()?.json().await.ok()?;
    let hex = v["result"].as_str()?;
    let wei = u128::from_str_radix(hex.trim_start_matches("0x"), 16).ok()?;
    Some(wei as f64 / 1e18)
}

pub async fn erc20_balance_raw(
    http: &reqwest::Client,
    rpc: &str,
    token: &str,
    owner: Address,
) -> Option<u128> {
    let padded = format!("000000000000000000000000{:x}", owner);
    let data = format!("0x70a08231{}", padded);
    let body = serde_json::json!({
        "jsonrpc": "2.0", "id": 1,
        "method": "eth_call",
        "params": [{"to": token, "data": data}, "latest"]
    });
    let v: serde_json::Value = http.post(rpc).json(&body).send().await.ok()?.json().await.ok()?;
    let hex = v["result"].as_str()?;
    if hex == "0x" || hex.len() < 3 { return Some(0); }
    let trimmed = hex.trim_start_matches("0x");
    let padded = if trimmed.len() % 2 != 0 { format!("0{}", trimmed) } else { trimmed.to_string() };
    let bytes = hex::decode(&padded).ok()?;
    if bytes.len() > 16 {
        // Take last 16 bytes (u128 max is enough for any stable balance in practice)
        let mut buf = [0u8; 16];
        let src = &bytes[bytes.len().saturating_sub(16)..];
        buf[16 - src.len()..].copy_from_slice(src);
        Some(u128::from_be_bytes(buf))
    } else {
        let mut buf = [0u8; 16];
        buf[16 - bytes.len()..].copy_from_slice(&bytes);
        Some(u128::from_be_bytes(buf))
    }
}
