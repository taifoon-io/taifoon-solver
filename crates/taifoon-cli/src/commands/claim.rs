//! `taifoon claim` — scan all chains for stranded balances and bridge back to consolidation chain.
//!
//! For chains with insufficient native gas, automatically bridges a small amount from Arbitrum
//! (funding chain) to acquire native gas first via Across swap+bridge to native token.
//! Then bridges the full stranded balance back to the consolidation chain.
//!
//! zkSync (324): uses zkSync native L2Bridge.withdraw() — takes 24h finalization on L1.
//! BNB (56): Across unsupported — reported only.

use alloy::{
    network::EthereumWallet,
    primitives::{address, Address, Bytes, U256},
    providers::{Provider, ProviderBuilder},
    rpc::types::TransactionRequest,
    signers::local::PrivateKeySigner,
    sol,
    sol_types::SolCall,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

// ── ABI definitions ──────────────────────────────────────────────────────────

sol! {
    #[allow(missing_docs)]
    function depositV3(
        address depositor,
        address recipient,
        address inputToken,
        address outputToken,
        uint256 inputAmount,
        uint256 outputAmount,
        uint256 destinationChainId,
        address exclusiveRelayer,
        uint32 quoteTimestamp,
        uint32 fillDeadline,
        uint32 exclusivityDeadline,
        bytes calldata message
    ) external payable;

    #[allow(missing_docs)]
    function approve(address spender, uint256 amount) external returns (bool);

    #[allow(missing_docs)]
    function allowance(address owner, address spender) external view returns (uint256);

    /// zkSync L2Bridge: withdraw ERC-20 back to Ethereum (initiates 24h exit)
    #[allow(missing_docs)]
    function withdraw(
        address _l1Receiver,
        address _l2Token,
        uint256 _amount
    ) external;
}

// ── Static chain / token config ──────────────────────────────────────────────

struct SpokePool {
    chain_id: u64,
    address: Address,
}

const SPOKE_POOLS: &[SpokePool] = &[
    SpokePool { chain_id: 1,     address: address!("5c7BCd6E7De5423a257D81B442095A1a6ced35C5") },
    SpokePool { chain_id: 10,    address: address!("6f26Bf09B1C792e3228e5467807a900A503c0281") },
    SpokePool { chain_id: 137,   address: address!("9295ee1d8C5b022Be115A2AD3c30C72E34e7F096") },
    SpokePool { chain_id: 8453,  address: address!("09aea4b2242abC8bb4BB78D537A67a245A7bEC64") },
    SpokePool { chain_id: 42161, address: address!("e35e9842fceaCA96570B734083f4a58e8F7C5f2A") },
    SpokePool { chain_id: 59144, address: address!("7e63a5f1a8F0B4D0934B2f2327DAEd3f6bb2Ee75") },
];

fn spoke_pool_for(chain_id: u64) -> Option<Address> {
    SPOKE_POOLS.iter().find(|sp| sp.chain_id == chain_id).map(|sp| sp.address)
}

#[derive(Clone)]
struct TokenSpec {
    symbol: &'static str,
    address: Address,
    decimals: u32,
}

#[derive(Clone, Copy, PartialEq)]
enum BridgeSupport {
    /// Full Across bridge support
    Across,
    /// zkSync native L2Bridge withdrawal (24h finalization)
    ZkSyncNative,
    /// No automated bridge available — report only
    Manual,
}

struct ChainSpec {
    chain_id: u64,
    name: &'static str,
    rpc: &'static str,
    tokens: Vec<TokenSpec>,
    bridge: BridgeSupport,
}

fn chain_specs() -> Vec<ChainSpec> {
    vec![
        ChainSpec {
            chain_id: 1,
            name: "Ethereum",
            rpc: "https://eth.llamarpc.com",
            tokens: vec![
                TokenSpec { symbol: "USDC", address: address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"), decimals: 6 },
                TokenSpec { symbol: "USDT", address: address!("dAC17F958D2ee523a2206206994597C13D831ec7"), decimals: 6 },
                TokenSpec { symbol: "WETH", address: address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"), decimals: 18 },
            ],
            bridge: BridgeSupport::Across,
        },
        ChainSpec {
            chain_id: 10,
            name: "Optimism",
            rpc: "https://mainnet.optimism.io",
            tokens: vec![
                TokenSpec { symbol: "USDC", address: address!("0b2C639c533813f4Aa9D7837CAf62653d097Ff85"), decimals: 6 },
                TokenSpec { symbol: "USDT", address: address!("94b008aA00579c1307B0EF2c499aD98a8ce58e58"), decimals: 6 },
                TokenSpec { symbol: "WETH", address: address!("4200000000000000000000000000000000000006"), decimals: 18 },
            ],
            bridge: BridgeSupport::Across,
        },
        ChainSpec {
            chain_id: 137,
            name: "Polygon",
            rpc: "https://polygon-bor-rpc.publicnode.com",
            tokens: vec![
                TokenSpec { symbol: "USDC.e", address: address!("2791Bca1f2de4661ED88A30C99A7a9449Aa84174"), decimals: 6 },
                TokenSpec { symbol: "USDT",   address: address!("c2132D05D31c914a87C6611C10748AEb04B58e8F"), decimals: 6 },
            ],
            bridge: BridgeSupport::Across,
        },
        ChainSpec {
            chain_id: 56,
            name: "BNB",
            rpc: "https://bsc-dataseed.binance.org/",
            tokens: vec![
                TokenSpec { symbol: "USDT", address: address!("55d398326f99059fF775485246999027B3197955"), decimals: 18 },
                TokenSpec { symbol: "USDC", address: address!("8AC76a51cc950d9822D68b83fE1Ad97B32Cd580d"), decimals: 18 },
            ],
            bridge: BridgeSupport::Manual,
        },
        ChainSpec {
            chain_id: 59144,
            name: "Linea",
            rpc: "https://rpc.linea.build",
            tokens: vec![
                TokenSpec { symbol: "USDC", address: address!("176211869cA2b568f2A7D4EE941E073a821EE1ff"), decimals: 6 },
            ],
            bridge: BridgeSupport::Across,
        },
        ChainSpec {
            chain_id: 324,
            name: "zkSync",
            rpc: "https://mainnet.era.zksync.io",
            tokens: vec![
                TokenSpec { symbol: "USDC", address: address!("1d17CBcF0D6D143135aE902365D2E5e2A16538D4"), decimals: 6 },
                TokenSpec { symbol: "USDT", address: address!("493257fD37EDB34451f62EDf8D2a0C418852bA4C"), decimals: 6 },
            ],
            bridge: BridgeSupport::ZkSyncNative,
        },
        ChainSpec {
            chain_id: 8453,
            name: "Base",
            rpc: "https://base-rpc.publicnode.com",
            tokens: vec![
                TokenSpec { symbol: "USDC", address: address!("833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"), decimals: 6 },
                TokenSpec { symbol: "WETH", address: address!("4200000000000000000000000000000000000006"), decimals: 18 },
            ],
            bridge: BridgeSupport::Across,
        },
        ChainSpec {
            chain_id: 42161,
            name: "Arbitrum",
            rpc: "https://arb1.arbitrum.io/rpc",
            tokens: vec![
                TokenSpec { symbol: "USDC", address: address!("af88d065e77c8cC2239327C5EDb3A432268e5831"), decimals: 6 },
                TokenSpec { symbol: "USDT", address: address!("Fd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9"), decimals: 6 },
            ],
            bridge: BridgeSupport::Across,
        },
    ]
}

// Arbitrum is the funding chain for auto gas top-ups
const FUNDING_CHAIN_ID: u64 = 42161;
const FUNDING_CHAIN_RPC: &str = "https://arb1.arbitrum.io/rpc";
const FUNDING_TOKEN: Address = address!("Fd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9"); // USDT on Arb
const GAS_TOPUP_USD: f64 = 3.0;
const GAS_TOPUP_AMOUNT_RAW: u64 = 3_000_000; // $3 USDT (6 dec)

// zkSync L2 shared bridge address (for ERC-20 withdrawal)
const ZKSYNC_L2_BRIDGE: Address = address!("0000000000000000000000000000000000010003");

const WETH_PRICE_USD: f64 = 3000.0;
const MIN_BRIDGE_USD: f64 = 1.0;
const MIN_GAS_ETH: f64 = 0.0005;

// ── Public API ───────────────────────────────────────────────────────────────

pub struct ClaimArgs {
    pub private_key: String,
    pub dry_run: bool,
    pub consolidate_to_chain: u64,
    pub json_mode: bool,
    pub run_loop: bool,
}

// ── JSON output types ────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct BalanceEntry {
    pub chain: u64,
    pub chain_name: String,
    pub token: String,
    pub raw: String,
    pub usd: f64,
    pub action: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BridgeEntry {
    pub from_chain: u64,
    pub from_chain_name: String,
    pub token: String,
    pub amount_usd: f64,
    pub fee_usd: f64,
    pub net_usd: f64,
    pub tx_hash: String,
    pub status: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClaimOutput {
    pub balances: Vec<BalanceEntry>,
    pub bridges: Vec<BridgeEntry>,
    pub total_stranded_usd: f64,
    pub total_bridged_usd: f64,
}

// ── Across suggested-fees API ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SuggestedFeesResp {
    relay_fee_total: String,
    #[serde(rename = "outputAmount")]
    output_amount: String,
    timestamp: String,
    #[serde(deserialize_with = "de_u32_or_str")]
    fill_deadline: u32,
    exclusive_relayer: String,
    #[serde(deserialize_with = "de_u32_or_str")]
    exclusivity_deadline: u32,
    output_token: OutputTokenField,
}

#[derive(Debug, Deserialize)]
struct OutputTokenField {
    address: String,
}

fn de_u32_or_str<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    use serde::de::{self, Visitor};
    struct U32OrStr;
    impl<'de> Visitor<'de> for U32OrStr {
        type Value = u32;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "u32 or string")
        }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<u32, E> { Ok(v as u32) }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<u32, E> { Ok(v as u32) }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<u32, E> {
            v.parse().map_err(de::Error::custom)
        }
    }
    d.deserialize_any(U32OrStr)
}

// ── Across swap+bridge API (outputs native gas token on dst chain) ─────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SwapResp {
    swap_tx: SwapTx,
    #[serde(default)]
    approval_txns: Vec<ApprovalTxn>,
    #[serde(rename = "expectedOutputAmount")]
    expected_output_amount: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SwapTx {
    to: String,
    data: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApprovalTxn {
    chain_id: u64,
    to: String,
    data: String,
}

// ── Raw JSON-RPC helpers ──────────────────────────────────────────────────────

async fn erc20_raw_balance(
    http: &reqwest::Client,
    rpc: &str,
    token: Address,
    owner: Address,
) -> Option<U256> {
    let padded = format!("000000000000000000000000{:x}", owner);
    let data = format!("0x70a08231{}", padded);
    let body = serde_json::json!({
        "jsonrpc": "2.0", "id": 1,
        "method": "eth_call",
        "params": [{"to": format!("{:#x}", token), "data": data}, "latest"]
    });
    let resp = http.post(rpc).json(&body)
        .timeout(std::time::Duration::from_secs(10))
        .send().await.ok()?;
    let v: serde_json::Value = resp.json().await.ok()?;
    let hex = v["result"].as_str()?;
    if hex == "0x" || hex.len() < 3 { return Some(U256::ZERO); }
    let trimmed = hex.trim_start_matches("0x");
    if trimmed.is_empty() { return Some(U256::ZERO); }
    let bytes = hex::decode(if trimmed.len() % 2 != 0 { format!("0{}", trimmed) } else { trimmed.to_string() }).ok()?;
    if bytes.len() > 32 { return None; }
    let mut buf = [0u8; 32];
    buf[32 - bytes.len()..].copy_from_slice(&bytes);
    Some(U256::from_be_bytes(buf))
}

async fn eth_balance_f64(http: &reqwest::Client, rpc: &str, owner: Address) -> Option<f64> {
    let body = serde_json::json!({
        "jsonrpc": "2.0", "id": 1,
        "method": "eth_getBalance",
        "params": [format!("{:#x}", owner), "latest"]
    });
    let resp = http.post(rpc).json(&body)
        .timeout(std::time::Duration::from_secs(10))
        .send().await.ok()?;
    let v: serde_json::Value = resp.json().await.ok()?;
    let hex = v["result"].as_str()?;
    let wei = u128::from_str_radix(hex.trim_start_matches("0x"), 16).ok()?;
    Some(wei as f64 / 1e18)
}

fn usd_value(raw: U256, decimals: u32, symbol: &str) -> f64 {
    let amount = raw.to::<u128>() as f64 / 10f64.powi(decimals as i32);
    if symbol == "WETH" { amount * WETH_PRICE_USD } else { amount }
}

async fn send_raw_tx(
    signer: &PrivateKeySigner,
    rpc: &str,
    to: &str,
    data: &str,
) -> Result<String> {
    let wallet = EthereumWallet::from(signer.clone());
    let provider = ProviderBuilder::new()
        .with_recommended_fillers()
        .wallet(wallet)
        .on_http(rpc.parse().context("bad rpc")?);
    let to_addr: Address = to.parse().context("bad to address")?;
    let calldata = hex::decode(data.trim_start_matches("0x")).context("bad hex data")?;
    let tx_req = TransactionRequest::default()
        .to(to_addr)
        .input(Bytes::from(calldata).into());
    let pending = provider.send_transaction(tx_req).await.context("send_transaction failed")?;
    let hash = format!("{:#x}", pending.tx_hash());
    let receipt = pending.with_required_confirmations(1).get_receipt().await.context("receipt wait failed")?;
    if receipt.status() { Ok(hash) } else { anyhow::bail!("tx reverted: {}", hash) }
}

// ── Across suggested-fees fetch ───────────────────────────────────────────────

async fn fetch_suggested_fees(
    http: &reqwest::Client,
    src_chain: u64,
    dst_chain: u64,
    token: Address,
    amount: U256,
) -> Result<SuggestedFeesResp> {
    let url = format!(
        "https://app.across.to/api/suggested-fees?originChainId={}&destinationChainId={}&token={:#x}&amount={}",
        src_chain, dst_chain, token, amount
    );
    let resp = http.get(&url).timeout(std::time::Duration::from_secs(15)).send().await
        .context("Across suggested-fees request failed")?;
    if !resp.status().is_success() {
        let status = resp.status();
        anyhow::bail!("Across API {}: {}", status, resp.text().await.unwrap_or_default());
    }
    resp.json::<SuggestedFeesResp>().await.context("Failed to parse Across suggested-fees response")
}

// ── Across swap+bridge for native gas top-up ──────────────────────────────────

async fn fetch_swap_to_native(
    http: &reqwest::Client,
    src_chain: u64,
    dst_chain: u64,
    input_token: Address,
    amount_raw: u64,
    depositor: Address,
) -> Result<SwapResp> {
    let url = format!(
        "https://app.across.to/api/swap?originChainId={}&destinationChainId={}&inputToken={:#x}&outputToken=0x0000000000000000000000000000000000000000&amount={}&swapSlippage=0.01&depositor={:#x}",
        src_chain, dst_chain, input_token, amount_raw, depositor
    );
    let resp = http.get(&url).timeout(std::time::Duration::from_secs(15)).send().await
        .context("Across swap API request failed")?;
    if !resp.status().is_success() {
        let status = resp.status();
        anyhow::bail!("Across swap API {}: {}", status, resp.text().await.unwrap_or_default());
    }
    resp.json::<SwapResp>().await.context("Failed to parse Across swap response")
}

// ── ERC-20 approval ───────────────────────────────────────────────────────────

async fn ensure_allowance(
    signer: &PrivateKeySigner,
    solver_addr: Address,
    token: Address,
    spender: Address,
    amount: U256,
    rpc: &str,
) -> Result<()> {
    let check_provider = ProviderBuilder::new().on_http(rpc.parse().context("bad rpc")?);
    let allowance_call = allowanceCall { owner: solver_addr, spender }.abi_encode();
    let req = TransactionRequest::default().to(token).input(Bytes::from(allowance_call).into());
    let existing: U256 = match check_provider.call(&req).await {
        Ok(bytes) if bytes.len() >= 32 => U256::from_be_slice(&bytes[bytes.len() - 32..]),
        _ => U256::ZERO,
    };
    if existing >= amount { return Ok(()); }

    let wallet = EthereumWallet::from(signer.clone());
    let write_provider = ProviderBuilder::new()
        .with_recommended_fillers().wallet(wallet)
        .on_http(rpc.parse().context("bad rpc")?);
    let approve_call = approveCall { spender, amount: U256::MAX }.abi_encode();
    let approve_req = TransactionRequest::default().to(token).input(Bytes::from(approve_call).into());
    let pending = write_provider.send_transaction(approve_req).await.context("approve tx failed")?;
    pending.with_required_confirmations(1).get_receipt().await.context("approval receipt failed")?;
    Ok(())
}

// ── Gas top-up: bridge $3 stables from Arbitrum → native gas on dst chain ────

struct GasTopupResult {
    tx_hash: String,
    native_out: f64,
}

async fn topup_gas(
    http: &reqwest::Client,
    signer: &PrivateKeySigner,
    solver_addr: Address,
    dst_chain_id: u64,
    dst_chain_name: &str,
    dry_run: bool,
    json_mode: bool,
) -> Result<Option<GasTopupResult>> {
    // Verify Arbitrum has enough USDT to fund the top-up
    let arb_usdt = erc20_raw_balance(http, FUNDING_CHAIN_RPC, FUNDING_TOKEN, solver_addr)
        .await.unwrap_or(U256::ZERO);
    if arb_usdt.to::<u128>() as f64 / 1e6 < GAS_TOPUP_USD {
        if !json_mode {
            eprintln!("  WARNING: Not enough USDT on Arbitrum for gas top-up to {}", dst_chain_name);
        }
        return Ok(None);
    }

    if !json_mode {
        println!("  Auto gas top-up: $3 USDT Arbitrum → {} native gas via Across swap...", dst_chain_name);
    }

    let swap = fetch_swap_to_native(http, FUNDING_CHAIN_ID, dst_chain_id, FUNDING_TOKEN, GAS_TOPUP_AMOUNT_RAW, solver_addr)
        .await.with_context(|| format!("Across swap quote for {} gas top-up failed", dst_chain_name))?;

    let native_out = swap.expected_output_amount.as_deref()
        .and_then(|s| s.parse::<u128>().ok())
        .unwrap_or(0) as f64 / 1e18;

    if dry_run {
        if !json_mode {
            println!("  [DRY RUN] Would top-up ~{:.4} native on {}", native_out, dst_chain_name);
        }
        return Ok(Some(GasTopupResult { tx_hash: String::new(), native_out }));
    }

    // Approve SpokePool for the funding token if needed
    for approval in &swap.approval_txns {
        if approval.chain_id == FUNDING_CHAIN_ID {
            ensure_allowance(signer, solver_addr, FUNDING_TOKEN,
                approval.to.parse().unwrap_or(Address::ZERO),
                U256::from(GAS_TOPUP_AMOUNT_RAW), FUNDING_CHAIN_RPC).await
                .context("Gas top-up approval failed")?;
        }
    }

    let tx_hash = send_raw_tx(signer, FUNDING_CHAIN_RPC, &swap.swap_tx.to, &swap.swap_tx.data)
        .await.with_context(|| format!("Gas top-up tx failed for {}", dst_chain_name))?;

    if !json_mode {
        println!("  Gas top-up sent tx={} — waiting 2 min for Across fill on {}...", tx_hash, dst_chain_name);
    }
    tokio::time::sleep(std::time::Duration::from_secs(120)).await;

    Ok(Some(GasTopupResult { tx_hash, native_out }))
}

// ── zkSync native bridge withdrawal ──────────────────────────────────────────

async fn zksync_withdraw(
    signer: &PrivateKeySigner,
    solver_addr: Address,
    token: &TokenSpec,
    raw_amount: U256,
    dry_run: bool,
    json_mode: bool,
) -> Result<BridgeEntry> {
    if !json_mode {
        println!(
            "  zkSync native withdrawal: {:.4} {} → Ethereum (24h finalization){}",
            raw_amount.to::<u128>() as f64 / 10f64.powi(token.decimals as i32),
            token.symbol,
            if dry_run { " [DRY RUN]" } else { "" }
        );
    }

    let amount_usd = usd_value(raw_amount, token.decimals, token.symbol);

    if dry_run {
        return Ok(BridgeEntry {
            from_chain: 324,
            from_chain_name: "zkSync".to_string(),
            token: token.symbol.to_string(),
            amount_usd,
            fee_usd: 0.0,
            net_usd: amount_usd,
            tx_hash: String::new(),
            status: "zksync_withdraw_dry_run".to_string(),
        });
    }

    // First approve ZKSYNC_L2_BRIDGE
    ensure_allowance(signer, solver_addr, token.address, ZKSYNC_L2_BRIDGE, raw_amount,
        "https://mainnet.era.zksync.io").await
        .context("zkSync bridge approval failed")?;

    let calldata = withdrawCall {
        _l1Receiver: solver_addr,
        _l2Token: token.address,
        _amount: raw_amount,
    }.abi_encode();

    let wallet = EthereumWallet::from(signer.clone());
    let provider = ProviderBuilder::new()
        .with_recommended_fillers().wallet(wallet)
        .on_http("https://mainnet.era.zksync.io".parse().context("bad rpc")?);

    let tx_req = TransactionRequest::default()
        .to(ZKSYNC_L2_BRIDGE)
        .input(Bytes::from(calldata).into());

    match provider.send_transaction(tx_req).await {
        Ok(pending) => {
            let hash = format!("{:#x}", pending.tx_hash());
            match pending.with_required_confirmations(1).get_receipt().await {
                Ok(r) if r.status() => {
                    if !json_mode {
                        println!("  zkSync withdrawal initiated tx={} (finalize on L1 after ~24h)", hash);
                    }
                    Ok(BridgeEntry {
                        from_chain: 324,
                        from_chain_name: "zkSync".to_string(),
                        token: token.symbol.to_string(),
                        amount_usd,
                        fee_usd: 0.0,
                        net_usd: amount_usd,
                        tx_hash: hash,
                        status: "zksync_withdraw_pending_l1_finalization".to_string(),
                    })
                }
                Ok(_) => Ok(BridgeEntry {
                    from_chain: 324, from_chain_name: "zkSync".to_string(),
                    token: token.symbol.to_string(), amount_usd,
                    fee_usd: 0.0, net_usd: 0.0, tx_hash: hash,
                    status: "reverted".to_string(),
                }),
                Err(e) => Ok(BridgeEntry {
                    from_chain: 324, from_chain_name: "zkSync".to_string(),
                    token: token.symbol.to_string(), amount_usd,
                    fee_usd: 0.0, net_usd: 0.0, tx_hash: String::new(),
                    status: format!("receipt_error: {e}"),
                }),
            }
        }
        Err(e) => anyhow::bail!("zkSync withdrawal tx failed: {e}"),
    }
}

// ── Across depositV3 bridge ───────────────────────────────────────────────────

async fn bridge_token_across(
    http: &reqwest::Client,
    signer: &PrivateKeySigner,
    solver_addr: Address,
    chain: &ChainSpec,
    token: &TokenSpec,
    raw_amount: U256,
    amount_usd: f64,
    dst_chain: u64,
    dry_run: bool,
) -> Result<BridgeEntry> {
    let fees = fetch_suggested_fees(http, chain.chain_id, dst_chain, token.address, raw_amount)
        .await.with_context(|| format!("Across fees failed for {} on chain {}", token.symbol, chain.chain_id))?;

    let relay_fee_raw: U256 = fees.relay_fee_total.parse().unwrap_or(U256::ZERO);
    let output_amount_raw: U256 = fees.output_amount.parse().unwrap_or(U256::ZERO);
    let fee_usd = usd_value(relay_fee_raw, token.decimals, token.symbol);
    let net_usd = amount_usd - fee_usd;
    let quote_timestamp: u32 = fees.timestamp.parse().unwrap_or(0);
    let exclusive_relayer: Address = fees.exclusive_relayer.parse().unwrap_or(Address::ZERO);
    let output_token: Address = fees.output_token.address.parse().unwrap_or(Address::ZERO);

    if dry_run {
        return Ok(BridgeEntry {
            from_chain: chain.chain_id,
            from_chain_name: chain.name.to_string(),
            token: token.symbol.to_string(),
            amount_usd, fee_usd, net_usd,
            tx_hash: String::new(),
            status: "dry_run".to_string(),
        });
    }

    let calldata = depositV3Call {
        depositor: solver_addr, recipient: solver_addr,
        inputToken: token.address, outputToken: output_token,
        inputAmount: raw_amount, outputAmount: output_amount_raw,
        destinationChainId: U256::from(dst_chain),
        exclusiveRelayer: exclusive_relayer,
        quoteTimestamp: quote_timestamp,
        fillDeadline: fees.fill_deadline,
        exclusivityDeadline: fees.exclusivity_deadline,
        message: Bytes::new(),
    }.abi_encode();

    let spoke_pool = spoke_pool_for(chain.chain_id)
        .ok_or_else(|| anyhow::anyhow!("No SpokePool for chain {}", chain.chain_id))?;

    let wallet = EthereumWallet::from(signer.clone());
    let provider = ProviderBuilder::new()
        .with_recommended_fillers().wallet(wallet)
        .on_http(chain.rpc.parse().context("bad rpc")?);

    let tx_req = TransactionRequest::default()
        .to(spoke_pool)
        .input(Bytes::from(calldata).into());

    match provider.send_transaction(tx_req).await {
        Ok(pending) => {
            let hash = format!("{:#x}", pending.tx_hash());
            match pending.with_required_confirmations(1).get_receipt().await {
                Ok(r) if r.status() => Ok(BridgeEntry {
                    from_chain: chain.chain_id, from_chain_name: chain.name.to_string(),
                    token: token.symbol.to_string(), amount_usd, fee_usd, net_usd,
                    tx_hash: hash, status: "sent".to_string(),
                }),
                Ok(_) => Ok(BridgeEntry {
                    from_chain: chain.chain_id, from_chain_name: chain.name.to_string(),
                    token: token.symbol.to_string(), amount_usd, fee_usd: 0.0, net_usd: 0.0,
                    tx_hash: hash, status: "reverted".to_string(),
                }),
                Err(e) => Ok(BridgeEntry {
                    from_chain: chain.chain_id, from_chain_name: chain.name.to_string(),
                    token: token.symbol.to_string(), amount_usd, fee_usd: 0.0, net_usd: 0.0,
                    tx_hash: String::new(), status: format!("receipt_error: {e}"),
                }),
            }
        }
        Err(e) => anyhow::bail!("send_transaction failed: {e}"),
    }
}

// ── Main run function ─────────────────────────────────────────────────────────

pub async fn run(args: ClaimArgs) -> Result<()> {
    let pk = args.private_key.trim().trim_start_matches("0x");
    let signer: PrivateKeySigner = pk.parse().context("invalid SOLVER_PRIVATE_KEY")?;
    let solver_addr = signer.address();

    let mut cycle = 0u32;
    loop {
        cycle += 1;
        if !args.json_mode && args.run_loop {
            println!("\n{}", "=".repeat(60));
            println!("=== Claim cycle #{} ===", cycle);
        }
        run_once(&args, &signer, solver_addr).await?;
        if !args.run_loop { break; }
        eprintln!("\nSleeping 30 minutes before next scan...");
        tokio::time::sleep(std::time::Duration::from_secs(1800)).await;
    }
    Ok(())
}

async fn run_once(
    args: &ClaimArgs,
    signer: &PrivateKeySigner,
    solver_addr: Address,
) -> Result<()> {
    let http = reqwest::Client::builder()
        .user_agent("taifoon-solver/1.0")
        .build().context("reqwest client build")?;

    let chains = chain_specs();
    let consolidate_chain_id = args.consolidate_to_chain;
    let consolidate_name = chains.iter()
        .find(|c| c.chain_id == consolidate_chain_id)
        .map(|c| c.name).unwrap_or("Base");

    if !args.json_mode {
        println!("\nScanning solver {:#x} across {} chains...\n", solver_addr, chains.len());
        println!("{:<14} {:<8} {:<14} {:<12} {}", "Chain", "Token", "Balance", "USD Est.", "Action");
        println!("{}", "\u{2500}".repeat(64));
    }

    struct BalanceScan {
        chain_id: u64,
        chain_name: &'static str,
        token: TokenSpec,
        raw: U256,
        usd: f64,
        action: String,
        is_home: bool,
        bridge: BridgeSupport,
        needs_gas_topup: bool,
    }

    let mut scanned: Vec<BalanceScan> = Vec::new();

    for chain in &chains {
        let gas = eth_balance_f64(&http, chain.rpc, solver_addr).await.unwrap_or(0.0);
        let is_home = chain.chain_id == consolidate_chain_id;
        let needs_gas_topup = gas < MIN_GAS_ETH && !is_home && chain.bridge != BridgeSupport::Manual;

        for token in &chain.tokens {
            let raw = erc20_raw_balance(&http, chain.rpc, token.address, solver_addr)
                .await.unwrap_or(U256::ZERO);
            let usd = usd_value(raw, token.decimals, token.symbol);

            let action = if is_home {
                "home chain".to_string()
            } else if chain.bridge == BridgeSupport::Manual {
                "manual bridge (BNB unsupported)".to_string()
            } else if usd < MIN_BRIDGE_USD {
                format!("dust (< ${:.2})", MIN_BRIDGE_USD)
            } else if chain.bridge == BridgeSupport::ZkSyncNative {
                "zkSync native withdraw → ETH L1 (24h)".to_string()
            } else if needs_gas_topup {
                format!("bridge -> {} (auto gas top-up first)", consolidate_name)
            } else {
                format!("bridge -> {}", consolidate_name)
            };

            if !args.json_mode {
                println!("{:<14} {:<8} {:<14} {:<12} {}",
                    chain.name, token.symbol,
                    format!("{:.4}", raw.to::<u128>() as f64 / 10f64.powi(token.decimals as i32)),
                    format!("${:.2}", usd),
                    action);
            }

            scanned.push(BalanceScan {
                chain_id: chain.chain_id, chain_name: chain.name,
                token: token.clone(), raw, usd, action,
                is_home, bridge: chain.bridge, needs_gas_topup,
            });
        }
    }

    let total_stranded_usd: f64 = scanned.iter()
        .filter(|s| !s.is_home && s.bridge != BridgeSupport::Manual && s.usd >= MIN_BRIDGE_USD)
        .map(|s| s.usd).sum();
    let total_manual_usd: f64 = scanned.iter()
        .filter(|s| s.bridge == BridgeSupport::Manual)
        .map(|s| s.usd).sum();

    if !args.json_mode {
        println!();
        if total_stranded_usd > 0.0 {
            print!("Total stranded: ${:.2}", total_stranded_usd);
            if total_manual_usd > 0.0 {
                println!(" (+ ${:.2} on BNB requiring manual bridge)", total_manual_usd);
            } else {
                println!();
            }
        } else {
            println!("No stranded balances found.");
        }
        println!();
        if args.dry_run { println!("[DRY RUN] Would bridge:"); } else { println!("Bridging..."); }
    }

    let candidates: Vec<&BalanceScan> = scanned.iter()
        .filter(|s| !s.is_home && s.bridge != BridgeSupport::Manual && s.usd >= MIN_BRIDGE_USD)
        .collect();

    let mut bridges: Vec<BridgeEntry> = Vec::new();
    let mut gas_topped_up: std::collections::HashSet<u64> = std::collections::HashSet::new();

    for scan in &candidates {
        let chain_spec = chains.iter().find(|c| c.chain_id == scan.chain_id).unwrap();

        // ── zkSync: native L2Bridge withdrawal ──
        if scan.bridge == BridgeSupport::ZkSyncNative {
            // zkSync has ETH for gas natively (tiny amounts from protocol), just try
            match zksync_withdraw(signer, solver_addr, &scan.token, scan.raw, args.dry_run, args.json_mode).await {
                Ok(entry) => bridges.push(entry),
                Err(e) => {
                    if !args.json_mode { eprintln!("  ERROR zkSync withdrawal: {e}"); }
                    bridges.push(BridgeEntry {
                        from_chain: 324, from_chain_name: "zkSync".to_string(),
                        token: scan.token.symbol.to_string(), amount_usd: scan.usd,
                        fee_usd: 0.0, net_usd: 0.0, tx_hash: String::new(),
                        status: format!("error: {e}"),
                    });
                }
            }
            continue;
        }

        // ── Across chains: gas top-up if needed ──
        if scan.needs_gas_topup && !gas_topped_up.contains(&scan.chain_id) {
            if scan.chain_id == FUNDING_CHAIN_ID {
                if !args.json_mode {
                    eprintln!("  WARNING: No ETH on Arbitrum (funding chain) — cannot bridge");
                }
                bridges.push(BridgeEntry {
                    from_chain: scan.chain_id, from_chain_name: scan.chain_name.to_string(),
                    token: scan.token.symbol.to_string(), amount_usd: scan.usd,
                    fee_usd: 0.0, net_usd: 0.0, tx_hash: String::new(),
                    status: "skipped_no_gas".to_string(),
                });
                continue;
            }

            gas_topped_up.insert(scan.chain_id);
            match topup_gas(&http, signer, solver_addr, scan.chain_id, chain_spec.name, args.dry_run, args.json_mode).await {
                Ok(Some(r)) => {
                    bridges.push(BridgeEntry {
                        from_chain: FUNDING_CHAIN_ID, from_chain_name: "Arbitrum".to_string(),
                        token: "USDT".to_string(), amount_usd: GAS_TOPUP_USD,
                        fee_usd: 0.0, net_usd: r.native_out * WETH_PRICE_USD,
                        tx_hash: r.tx_hash,
                        status: if args.dry_run { "gas_topup_dry_run".to_string() } else { "gas_topup_sent".to_string() },
                    });
                    // After top-up in live mode, verify gas actually landed
                    if !args.dry_run {
                        let gas_after = eth_balance_f64(&http, chain_spec.rpc, solver_addr).await.unwrap_or(0.0);
                        if gas_after < MIN_GAS_ETH {
                            if !args.json_mode {
                                eprintln!("  Top-up not yet landed on {} ({:.6} ETH) — deferred to next cycle", chain_spec.name, gas_after);
                            }
                            bridges.push(BridgeEntry {
                                from_chain: scan.chain_id, from_chain_name: scan.chain_name.to_string(),
                                token: scan.token.symbol.to_string(), amount_usd: scan.usd,
                                fee_usd: 0.0, net_usd: 0.0, tx_hash: String::new(),
                                status: "deferred_gas_pending".to_string(),
                            });
                            continue;
                        }
                    }
                }
                Ok(None) => {
                    bridges.push(BridgeEntry {
                        from_chain: scan.chain_id, from_chain_name: scan.chain_name.to_string(),
                        token: scan.token.symbol.to_string(), amount_usd: scan.usd,
                        fee_usd: 0.0, net_usd: 0.0, tx_hash: String::new(),
                        status: "skipped_topup_no_funds".to_string(),
                    });
                    continue;
                }
                Err(e) => {
                    if !args.json_mode { eprintln!("  Gas top-up error for {}: {e}", chain_spec.name); }
                    bridges.push(BridgeEntry {
                        from_chain: scan.chain_id, from_chain_name: scan.chain_name.to_string(),
                        token: scan.token.symbol.to_string(), amount_usd: scan.usd,
                        fee_usd: 0.0, net_usd: 0.0, tx_hash: String::new(),
                        status: format!("skipped_topup_error: {e}"),
                    });
                    continue;
                }
            }
        }

        // ── Ensure ERC-20 allowance ──
        if !args.dry_run {
            if let Some(spoke) = spoke_pool_for(scan.chain_id) {
                if let Err(e) = ensure_allowance(signer, solver_addr, scan.token.address, spoke, scan.raw, chain_spec.rpc).await {
                    if !args.json_mode { eprintln!("  WARNING: approval failed for {} on {}: {e}", scan.token.symbol, chain_spec.name); }
                }
            }
        }

        // ── Bridge via Across ──
        match bridge_token_across(&http, signer, solver_addr, chain_spec, &scan.token, scan.raw, scan.usd, consolidate_chain_id, args.dry_run).await {
            Ok(entry) => {
                if !args.json_mode {
                    if args.dry_run {
                        println!("  {}: {:.4} {} -> {} via Across (fee ~${:.2}, net ${:.2})",
                            chain_spec.name,
                            scan.raw.to::<u128>() as f64 / 10f64.powi(scan.token.decimals as i32),
                            scan.token.symbol, consolidate_name, entry.fee_usd, entry.net_usd);
                    } else {
                        let icon = if entry.status == "sent" { "OK" } else { "!!" };
                        println!("  {} {} {:.4} {} -> {} tx={} {}",
                            icon, chain_spec.name,
                            scan.raw.to::<u128>() as f64 / 10f64.powi(scan.token.decimals as i32),
                            scan.token.symbol, consolidate_name,
                            if entry.tx_hash.is_empty() { "-".to_string() } else { entry.tx_hash.clone() },
                            icon);
                    }
                }
                bridges.push(entry);
            }
            Err(e) => {
                if !args.json_mode { eprintln!("  ERROR bridging {} from {}: {e}", scan.token.symbol, chain_spec.name); }
                bridges.push(BridgeEntry {
                    from_chain: scan.chain_id, from_chain_name: scan.chain_name.to_string(),
                    token: scan.token.symbol.to_string(), amount_usd: scan.usd,
                    fee_usd: 0.0, net_usd: 0.0, tx_hash: String::new(),
                    status: format!("error: {e}"),
                });
            }
        }
    }

    let total_bridged_usd: f64 = bridges.iter()
        .filter(|b| b.status == "sent" || b.status == "dry_run")
        .map(|b| b.net_usd).sum();

    if !args.json_mode {
        if candidates.is_empty() {
            println!("Nothing to bridge.");
        } else if !args.dry_run && total_bridged_usd > 0.0 {
            println!("\nBridged ${:.2} net. Arrival ~2 min.", total_bridged_usd);
        } else if args.dry_run && !candidates.is_empty() {
            println!("\nRun with --execute to send transactions.");
        }
    }

    if args.json_mode {
        let output = ClaimOutput {
            balances: scanned.iter().map(|s| BalanceEntry {
                chain: s.chain_id, chain_name: s.chain_name.to_string(),
                token: s.token.symbol.to_string(), raw: s.raw.to_string(),
                usd: s.usd, action: s.action.clone(),
            }).collect(),
            bridges, total_stranded_usd, total_bridged_usd,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    }

    Ok(())
}
