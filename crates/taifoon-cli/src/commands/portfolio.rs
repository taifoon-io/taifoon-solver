use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// ERC-20 token addresses per chain (checksummed)
const USDC_BASE: &str = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";
const USDC_OP: &str = "0x0b2C639c533813f4Aa9D7837CAf62653d097Ff85";
const USDC_ARB: &str = "0xaf88d065e77c8cC2239327C5EDb3A432268e5831";
const USDC_ETH: &str = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48";
const USDC_POLYGON: &str = "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174";
const USDC_LINEA: &str = "0x176211869cA2b568f2A7D4EE941E073a821EE1ff";
const USDT_ARB: &str = "0xFd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9";
const USDT_ETH: &str = "0xdAC17F958D2ee523a2206206994597C13D831ec7";
const WETH_BASE: &str = "0x4200000000000000000000000000000000000006";
const WETH_OP: &str = "0x4200000000000000000000000000000000000006";
const WETH_ARB: &str = "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1";
const WETH_ETH: &str = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2";
const WETH_POLYGON: &str = "0x7ceB23fD6bC0adD59E62ac25578270cFf1b9f619";

/// Solana mainnet USDC mint
const SOLANA_USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

/// ERC-20 balanceOf(address) selector
const BALANCE_OF_SELECTOR: &str = "70a08231";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainBalance {
    pub chain_id: u64,
    pub chain_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_eth: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_sol: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usdc: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usdt: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weth: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillStats {
    pub confirmed: u64,
    pub reverted: u64,
    pub active: u64,
    pub total_volume_usd: f64,
    pub realized_profit_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Portfolio {
    pub solver_address: String,
    pub chains: Vec<ChainBalance>,
    pub fills: FillStats,
    pub as_of: String,
}

pub struct PortfolioArgs {
    pub private_key: String,
    pub json_mode: bool,
    pub spinner_url: String,
}

/// Per-chain token wiring spec
struct ChainSpec {
    chain_id: u64,
    name: &'static str,
    rpc: String,
    usdc: Option<&'static str>,
    usdt: Option<&'static str>,
    weth: Option<&'static str>,
    is_solana: bool,
}

/// Load chain specs from chain_wiring.json (if available) or fall back to defaults.
fn load_chain_specs() -> Vec<ChainSpec> {
    // Try to find chain_wiring.json relative to typical install locations
    let wiring_paths = [
        std::env::var("CHAIN_WIRING_PATH").unwrap_or_default(),
        "config/chain_wiring.json".to_string(),
        "/etc/taifoon/chain_wiring.json".to_string(),
    ];
    let wiring: Option<serde_json::Value> = wiring_paths.iter()
        .filter(|p| !p.is_empty())
        .find_map(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok());

    let mut specs = Vec::new();

    if let Some(ref w) = wiring {
        if let Some(obj) = w.as_object() {
            for (key, val) in obj {
                if key.starts_with('_') { continue; }
                let chain_id: u64 = match key.parse() { Ok(n) => n, Err(_) => continue };
                let rpc = val.get("rpc_url").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if rpc.is_empty() { continue; }
                let chain_name = val.get("_chain").and_then(|v| v.as_str()).unwrap_or(key.as_str()).to_string();
                // Only include mainnet chains (skip testnets/devnets) based on name
                if chain_name.contains("Sepolia") || chain_name.contains("Devnet")
                    || chain_name.contains("Testnet") || chain_name.contains("devnet") { continue; }
                specs.push(build_chain_spec_dynamic(chain_id, chain_name, rpc));
            }
        }
    }

    if specs.is_empty() {
        specs = default_chain_specs();
    }

    // Always include Solana (it's not in chain_wiring.json as an EVM chain)
    specs.push(ChainSpec {
        chain_id: 1_399_811_149,
        name: "Solana",
        rpc: "https://api.mainnet-beta.solana.com".to_string(),
        usdc: None,
        usdt: None,
        weth: None,
        is_solana: true,
    });

    specs
}

fn build_chain_spec_dynamic(chain_id: u64, chain_name: String, rpc: String) -> ChainSpec {
    let (usdc, usdt, weth) = match chain_id {
        1 => (Some(USDC_ETH), Some(USDT_ETH), Some(WETH_ETH)),
        10 => (Some(USDC_OP), None, Some(WETH_OP)),
        137 => (Some(USDC_POLYGON), None, Some(WETH_POLYGON)),
        8453 => (Some(USDC_BASE), None, Some(WETH_BASE)),
        42161 => (Some(USDC_ARB), Some(USDT_ARB), Some(WETH_ARB)),
        59144 => (Some(USDC_LINEA), None, None),
        _ => (None, None, None),
    };
    // Leak chain_name to get 'static lifetime — acceptable for a short-lived CLI process
    let name_leaked: &'static str = Box::leak(chain_name.into_boxed_str());
    ChainSpec { chain_id, name: name_leaked, rpc, usdc, usdt, weth, is_solana: false }
}

fn default_chain_specs() -> Vec<ChainSpec> {
    vec![
        ChainSpec {
            chain_id: 1, name: "Ethereum",
            rpc: "https://ethereum-rpc.publicnode.com".to_string(),
            usdc: Some(USDC_ETH), usdt: Some(USDT_ETH), weth: Some(WETH_ETH), is_solana: false,
        },
        ChainSpec {
            chain_id: 8453, name: "Base",
            rpc: "https://base-rpc.publicnode.com".to_string(),
            usdc: Some(USDC_BASE), usdt: None, weth: Some(WETH_BASE), is_solana: false,
        },
        ChainSpec {
            chain_id: 10, name: "Optimism",
            rpc: "https://optimism-rpc.publicnode.com".to_string(),
            usdc: Some(USDC_OP), usdt: None, weth: Some(WETH_OP), is_solana: false,
        },
        ChainSpec {
            chain_id: 42161, name: "Arbitrum",
            rpc: "https://arbitrum-one-rpc.publicnode.com".to_string(),
            usdc: Some(USDC_ARB), usdt: Some(USDT_ARB), weth: Some(WETH_ARB), is_solana: false,
        },
        ChainSpec {
            chain_id: 137, name: "Polygon",
            rpc: "https://polygon-rpc.com".to_string(),
            usdc: Some(USDC_POLYGON), usdt: None, weth: Some(WETH_POLYGON), is_solana: false,
        },
        ChainSpec {
            chain_id: 59144, name: "Linea",
            rpc: "https://linea-rpc.publicnode.com".to_string(),
            usdc: Some(USDC_LINEA), usdt: None, weth: None, is_solana: false,
        },
    ]
}

/// Call eth_getBalance via JSON-RPC for native ETH
async fn eth_get_balance(client: &reqwest::Client, rpc: &str, addr: &str) -> Option<f64> {
    let body = serde_json::json!({
        "jsonrpc": "2.0", "id": 1,
        "method": "eth_getBalance",
        "params": [addr, "latest"]
    });
    let resp = client.post(rpc).json(&body)
        .timeout(std::time::Duration::from_secs(5))
        .send().await.ok()?;
    let data: serde_json::Value = resp.json().await.ok()?;
    let hex = data["result"].as_str()?;
    let wei = u128::from_str_radix(hex.trim_start_matches("0x"), 16).ok()?;
    Some(wei as f64 / 1e18)
}

/// Call ERC-20 balanceOf(addr) via eth_call
async fn erc20_balance(
    client: &reqwest::Client,
    rpc: &str,
    token: &str,
    addr: &str,
    decimals: u32,
) -> Option<f64> {
    let padded = format!("000000000000000000000000{}", addr.trim_start_matches("0x"));
    let data = format!("0x{}{}", BALANCE_OF_SELECTOR, padded);
    let body = serde_json::json!({
        "jsonrpc": "2.0", "id": 1,
        "method": "eth_call",
        "params": [{"to": token, "data": data}, "latest"]
    });
    let resp = client.post(rpc).json(&body)
        .timeout(std::time::Duration::from_secs(5))
        .send().await.ok()?;
    let data: serde_json::Value = resp.json().await.ok()?;
    let hex = data["result"].as_str()?;
    if hex == "0x" || hex.len() < 3 { return Some(0.0); }
    let raw = u128::from_str_radix(hex.trim_start_matches("0x"), 16).ok()?;
    Some(raw as f64 / 10f64.powi(decimals as i32))
}

/// Query Solana balance (SOL) and USDC SPL token balance via Solana JSON-RPC
async fn solana_balances(client: &reqwest::Client, rpc: &str, pubkey: &str) -> (Option<f64>, Option<f64>) {
    // SOL balance
    let sol_body = serde_json::json!({
        "jsonrpc": "2.0", "id": 1,
        "method": "getBalance",
        "params": [pubkey]
    });
    let sol: Option<f64> = async {
        let resp = client.post(rpc).json(&sol_body)
            .timeout(std::time::Duration::from_secs(5))
            .send().await.ok()?;
        let data: serde_json::Value = resp.json().await.ok()?;
        let lamports = data["result"]["value"].as_u64()?;
        Some(lamports as f64 / 1e9)
    }.await;

    // USDC SPL token balance via getTokenAccountsByOwner
    let usdc_body = serde_json::json!({
        "jsonrpc": "2.0", "id": 2,
        "method": "getTokenAccountsByOwner",
        "params": [
            pubkey,
            {"mint": SOLANA_USDC_MINT},
            {"encoding": "jsonParsed"}
        ]
    });
    let usdc: Option<f64> = async {
        let resp = client.post(rpc).json(&usdc_body)
            .timeout(std::time::Duration::from_secs(5))
            .send().await.ok()?;
        let data: serde_json::Value = resp.json().await.ok()?;
        let accounts = data["result"]["value"].as_array()?;
        let total: f64 = accounts.iter().filter_map(|acct| {
            acct["account"]["data"]["parsed"]["info"]["tokenAmount"]["uiAmount"].as_f64()
        }).sum();
        Some(total)
    }.await;

    (sol, usdc)
}

/// Derive EVM solver address using `cast wallet address` (Foundry)
fn derive_evm_address(private_key: &str) -> Option<String> {
    let key = private_key.trim_start_matches("0x");
    let out = std::process::Command::new("cast")
        .args(["wallet", "address", "--private-key", &format!("0x{}", key)])
        .output();
    if let Ok(o) = out {
        if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if !s.is_empty() { return Some(s); }
        }
    }
    None
}

/// Read Solana pubkey from env SOLANA_ADDRESS or ~/.taifoon/solver.toml
fn resolve_solana_address() -> Option<String> {
    if let Ok(v) = std::env::var("SOLANA_ADDRESS") {
        if !v.is_empty() { return Some(v); }
    }
    // Try reading from solver.toml
    let home = dirs::home_dir()?;
    let path = home.join(".taifoon/solver.toml");
    let contents = std::fs::read_to_string(path).ok()?;
    for line in contents.lines() {
        if line.starts_with("solana_address") {
            if let Some(val) = line.splitn(2, '=').nth(1) {
                let v = val.trim().trim_matches('"').trim_matches('\'').to_string();
                if !v.is_empty() { return Some(v); }
            }
        }
    }
    None
}

pub async fn run(args: PortfolioArgs) -> Result<()> {
    let evm_addr = match derive_evm_address(&args.private_key) {
        Some(a) => a,
        None => anyhow::bail!("could not derive EVM address (install foundry cast or check key format)"),
    };

    let solana_addr = resolve_solana_address();

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (compatible; taifoon-solver/1.0)")
        .build()?;

    let specs = load_chain_specs();

    // Fetch all chain balances in parallel via join_all
    let futures: Vec<_> = specs.iter().map(|s| {
        let client_ref = &client;
        let addr_evm = evm_addr.as_str();
        let addr_sol = solana_addr.as_deref();
        async move {
            if s.is_solana {
                let pubkey = addr_sol.unwrap_or("11111111111111111111111111111111");
                let (sol, usdc) = solana_balances(client_ref, &s.rpc, pubkey).await;
                ChainBalance {
                    chain_id: s.chain_id,
                    chain_name: s.name.to_string(),
                    native_eth: None,
                    native_sol: sol,
                    usdc,
                    usdt: None,
                    weth: None,
                }
            } else {
                let native = eth_get_balance(client_ref, &s.rpc, addr_evm).await;
                let usdc = if let Some(t) = s.usdc {
                    erc20_balance(client_ref, &s.rpc, t, addr_evm, 6).await
                } else { None };
                let usdt = if let Some(t) = s.usdt {
                    erc20_balance(client_ref, &s.rpc, t, addr_evm, 6).await
                } else { None };
                let weth = if let Some(t) = s.weth {
                    erc20_balance(client_ref, &s.rpc, t, addr_evm, 18).await
                } else { None };
                ChainBalance {
                    chain_id: s.chain_id,
                    chain_name: s.name.to_string(),
                    native_eth: native,
                    native_sol: None,
                    usdc,
                    usdt,
                    weth,
                }
            }
        }
    }).collect();

    let chains = futures::future::join_all(futures).await;

    let wallet_db = std::env::var("WALLET_DB_PATH")
        .unwrap_or_else(|_| "/tmp/taifoon_cli_wallet.sqlite".into());
    let fills = query_fill_stats(&wallet_db);

    let portfolio = Portfolio {
        solver_address: evm_addr.clone(),
        chains,
        fills,
        as_of: chrono::Utc::now().to_rfc3339(),
    };

    if args.json_mode {
        println!("{}", serde_json::to_string_pretty(&portfolio)?);
    } else {
        print_portfolio_human(&portfolio, solana_addr.as_deref());
    }

    Ok(())
}

fn query_fill_stats(db_path: &str) -> FillStats {
    let conn = match rusqlite::Connection::open(db_path) {
        Ok(c) => c,
        Err(_) => return FillStats::default_zeroes(),
    };

    let confirmed: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM intents WHERE state IN ('CONFIRMED','CLAIMED')",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0) as u64;

    let reverted: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM intents WHERE state = 'REVERTED'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0) as u64;

    let active: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM intents WHERE state NOT IN \
             ('CONFIRMED','CLAIMED','REVERTED','SKIP_UNPROFITABLE','PROOF_MISSING','CALLDATA_ERROR')",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0) as u64;

    let volume: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(amount_usd), 0.0) FROM intents WHERE state IN ('CONFIRMED','CLAIMED')",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0.0);

    let profit: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(profit_usd), 0.0) FROM revenue",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0.0);

    FillStats { confirmed, reverted, active, total_volume_usd: volume, realized_profit_usd: profit }
}

impl FillStats {
    fn default_zeroes() -> Self {
        FillStats { confirmed: 0, reverted: 0, active: 0, total_volume_usd: 0.0, realized_profit_usd: 0.0 }
    }
}

fn print_portfolio_human(p: &Portfolio, solana_addr: Option<&str>) {
    println!("\n TAIFOON PORTFOLIO");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("EVM Solver: {}", p.solver_address);
    if let Some(sol) = solana_addr {
        println!("Solana:     {}", sol);
    }
    println!("As of:      {}", p.as_of);
    println!();
    println!("{:<12} {:>12} {:>12} {:>12} {:>12} {:>10}", "Chain", "ETH/SOL", "USDC", "USDT", "WETH", "Chain ID");
    println!("{}", "─".repeat(76));
    for c in &p.chains {
        let native = c.native_eth.map(|v| format!("{:.4}", v))
            .or_else(|| c.native_sol.map(|v| format!("{:.4} SOL", v)))
            .unwrap_or_else(|| "n/a".into());
        println!(
            "{:<12} {:>12} {:>12} {:>12} {:>12} {:>10}",
            c.chain_name,
            native,
            c.usdc.map(|v| format!("{:.2}", v)).unwrap_or_else(|| "—".into()),
            c.usdt.map(|v| format!("{:.2}", v)).unwrap_or_else(|| "—".into()),
            c.weth.map(|v| format!("{:.4}", v)).unwrap_or_else(|| "—".into()),
            c.chain_id,
        );
    }
    println!();
    println!("Fill Stats:");
    println!("  Confirmed:    {}", p.fills.confirmed);
    println!("  Reverted:     {}", p.fills.reverted);
    println!("  In-flight:    {}", p.fills.active);
    println!("  Volume:       ${:.2}", p.fills.total_volume_usd);
    println!("  P&L:          ${:.4}", p.fills.realized_profit_usd);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");
}
