//! `taifoon setup-approvals` — pre-approve fill tokens on all dst chains.
//!
//! Before the solver can broadcast Across fillV3Relay or deBridge fulfillOrder,
//! the solver wallet must have:
//!   - outputToken.approve(SpokePool, MAX) on each Across dst chain
//!   - takeToken.approve(DlnDestination, MAX) on each deBridge dst chain
//!
//! Run this once after funding the solver wallet, before enabling live fills.

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
use tracing::{info, warn};

sol! {
    #[allow(missing_docs)]
    function approve(address spender, uint256 amount) external returns (bool);
    function allowance(address owner, address spender) external view returns (uint256);
}

const DLN_DESTINATION: Address = address!("E7351Fd770A37282b91D153Ee690B63579D6dd7f");

pub struct ApprovalArgs {
    pub private_key: String,
    pub dry_run: bool,
}

struct ChainApproval {
    #[allow(dead_code)]
    chain_id: u64,
    name: &'static str,
    rpc_url: &'static str,
    spender: Address,
    tokens: Vec<Address>,
}

fn approval_plan() -> Vec<ChainApproval> {
    vec![
        // ── Across SpokePools ──────────────────────────────────────────────
        ChainApproval {
            chain_id: 42161, name: "Arbitrum (Across)", rpc_url: "https://arb1.arbitrum.io/rpc",
            spender: address!("e35e9842fceaCA96570B734083f4a58e8F7C5f2A"),
            tokens: vec![
                address!("af88d065e77c8cc2239327c5edb3a432268e5831"), // USDC native
                address!("ff970a61a04b1ca14834a43f5de4533ebddb5cc8"), // USDC.e
                address!("fd086bc7cd5c481dcc9c85ebe478a1c0b69fcbb9"), // USDT
                address!("82af49447d8a07e3bd95bd0d56f35241523fbab1"), // WETH
            ],
        },
        ChainApproval {
            chain_id: 8453, name: "Base (Across)", rpc_url: "https://mainnet.base.org",
            spender: address!("09aea4b2242abC8bb4BB78D537A67a245A7bEC64"),
            tokens: vec![
                address!("833589fcd6edb6e08f4c7c32d4f71b54bda02913"), // USDC
                address!("4200000000000000000000000000000000000006"), // WETH
            ],
        },
        ChainApproval {
            chain_id: 10, name: "Optimism (Across)", rpc_url: "https://mainnet.optimism.io",
            spender: address!("6f26Bf09B1C792e3228e5467807a900A503c0281"),
            tokens: vec![
                address!("0b2c639c533813f4aa9d7837caf62653d097ff85"), // USDC native
                address!("7f5c764cbc14f9669b88837ca1490cca17c31607"), // USDC.e
                address!("94b008aA00579c1307B0EF2c499aD98a8ce58e58"), // USDT
                address!("4200000000000000000000000000000000000006"), // WETH
            ],
        },
        ChainApproval {
            chain_id: 1, name: "Ethereum (Across)", rpc_url: "https://ethereum.publicnode.com",
            spender: address!("5c7BCd6E7De5423a257D81B442095A1a6ced35C5"),
            tokens: vec![
                address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"), // USDC
                address!("dAC17F958D2ee523a2206206994597C13D831ec7"), // USDT
                address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"), // WETH
            ],
        },
        ChainApproval {
            chain_id: 137, name: "Polygon (Across)", rpc_url: "https://polygon-rpc.com",
            spender: address!("9295ee1d8C5b022Be115A2AD3c30C72E34e7F096"),
            tokens: vec![
                address!("3c499c542cEF5E3811e1192ce70d8cC03d5c3359"), // USDC native
                address!("2791Bca1f2de4661ED88A30C99A7a9449Aa84174"), // USDC.e
                address!("c2132D05D31c914a87C6611C10748AEb04B58e8F"), // USDT
                address!("7ceB23fD6bC0adD59E62ac25578270cFf1b9f619"), // WETH
            ],
        },
        ChainApproval {
            chain_id: 59144, name: "Linea (Across)", rpc_url: "https://rpc.linea.build",
            spender: address!("7e63a5f1a8F0B4D0934B2f2327DAEd3f6bb2Ee75"),
            tokens: vec![
                address!("176211869cA2b568f2A7D4EE941E073a821EE1ff"), // USDC
                address!("e5D7C2a44FfDDf6b295A15c148167daaAf5Cf34f"), // WETH
            ],
        },
        ChainApproval {
            chain_id: 534352, name: "Scroll (Across)", rpc_url: "https://rpc.scroll.io",
            spender: address!("3baD7AD0728f9917d1Bf08af5782dCbD516cDd96"),
            tokens: vec![
                address!("06eFDBff2a14a7c8E15944D1F4A48F9F95F663A4"), // USDC
                address!("5300000000000000000000000000000000000004"), // WETH
            ],
        },
        ChainApproval {
            chain_id: 34443, name: "Mode (Across)", rpc_url: "https://mainnet.mode.network",
            spender: address!("3baD7AD0728f9917d1Bf08af5782dCbD516cDd96"),
            tokens: vec![
                address!("d988097fb8612cc24eeC14542bc03424c656005f"), // USDC.e
                address!("4200000000000000000000000000000000000006"), // WETH
            ],
        },
        ChainApproval {
            chain_id: 57073, name: "Ink (Across)", rpc_url: "https://rpc-gel.inkonchain.com",
            spender: address!("eF684C38F94F48775959ECf2012D7E864ffb9dd4"),
            tokens: vec![
                address!("2d270e6886d130d724215a266106e6832161eaed"), // USDC
                address!("0200c29006150606b650577bbe7b6248f58470c1"), // USDT
                address!("4200000000000000000000000000000000000006"), // WETH
            ],
        },
        // ── deBridge DlnDestination (same address all chains) ─────────────
        ChainApproval {
            chain_id: 42161, name: "Arbitrum (deBridge)", rpc_url: "https://arb1.arbitrum.io/rpc",
            spender: DLN_DESTINATION,
            tokens: vec![
                address!("af88d065e77c8cc2239327c5edb3a432268e5831"), // USDC native
                address!("fd086bc7cd5c481dcc9c85ebe478a1c0b69fcbb9"), // USDT
                address!("82af49447d8a07e3bd95bd0d56f35241523fbab1"), // WETH
            ],
        },
        ChainApproval {
            chain_id: 8453, name: "Base (deBridge)", rpc_url: "https://mainnet.base.org",
            spender: DLN_DESTINATION,
            tokens: vec![
                address!("833589fcd6edb6e08f4c7c32d4f71b54bda02913"), // USDC
                address!("4200000000000000000000000000000000000006"), // WETH
            ],
        },
        ChainApproval {
            chain_id: 10, name: "Optimism (deBridge)", rpc_url: "https://mainnet.optimism.io",
            spender: DLN_DESTINATION,
            tokens: vec![
                address!("0b2c639c533813f4aa9d7837caf62653d097ff85"), // USDC native
                address!("94b008aA00579c1307B0EF2c499aD98a8ce58e58"), // USDT
                address!("4200000000000000000000000000000000000006"), // WETH
            ],
        },
        ChainApproval {
            chain_id: 534352, name: "Scroll (deBridge)", rpc_url: "https://rpc.scroll.io",
            spender: DLN_DESTINATION,
            tokens: vec![
                address!("06eFDBff2a14a7c8E15944D1F4A48F9F95F663A4"), // USDC
                address!("f55BEC9cafdBE8730f096Aa55dad6D22d44099Df"), // USDT
                address!("5300000000000000000000000000000000000004"), // WETH
            ],
        },
        ChainApproval {
            chain_id: 59144, name: "Linea (deBridge)", rpc_url: "https://rpc.linea.build",
            spender: DLN_DESTINATION,
            tokens: vec![
                address!("176211869cA2b568f2A7D4EE941E073a821EE1ff"), // USDC
                address!("e5D7C2a44FfDDf6b295A15c148167daaAf5Cf34f"), // WETH
            ],
        },
    ]
}

pub async fn run(args: ApprovalArgs) -> Result<()> {
    let pk = args.private_key.trim().trim_start_matches("0x");
    let signer: PrivateKeySigner = pk
        .parse()
        .context("invalid SOLVER_PRIVATE_KEY")?;
    let solver_addr = signer.address();
    info!("🔑 Solver address: {solver_addr:#x}");

    let plan = approval_plan();
    let mut approved = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;

    // 1e36 threshold — effectively infinite approval already in place
    let threshold = U256::from(10u128).pow(U256::from(36u32));

    for entry in &plan {
        let rpc_url = entry.rpc_url.parse().context("bad rpc_url")?;
        let provider = ProviderBuilder::new().on_http(rpc_url);

        for &token in &entry.tokens {
            // Check existing allowance
            let allowance_calldata = allowanceCall {
                owner: solver_addr,
                spender: entry.spender,
            }.abi_encode();
            let allowance_req = TransactionRequest::default()
                .to(token)
                .input(Bytes::from(allowance_calldata).into());
            let current_allowance = match provider.call(&allowance_req).await {
                Ok(bytes) if bytes.len() >= 32 => {
                    U256::from_be_slice(&bytes[bytes.len() - 32..])
                }
                _ => U256::ZERO,
            };

            if current_allowance >= threshold {
                info!("⏭️  {token:#x} → {:#x} already approved on {}", entry.spender, entry.name);
                skipped += 1;
                continue;
            }

            if args.dry_run {
                info!("🧪 DRY_RUN: would approve {token:#x} → {:#x} on {} (current={current_allowance})",
                    entry.spender, entry.name);
                approved += 1;
                continue;
            }

            // Send approval tx
            let wallet = EthereumWallet::from(signer.clone());
            let write_provider = ProviderBuilder::new()
                .with_recommended_fillers()
                .wallet(wallet)
                .on_http(entry.rpc_url.parse()?);

            let approve_calldata = approveCall {
                spender: entry.spender,
                amount: U256::MAX,
            }.abi_encode();

            let tx_req = TransactionRequest::default()
                .to(token)
                .input(Bytes::from(approve_calldata).into());

            match write_provider.send_transaction(tx_req).await {
                Ok(pending) => {
                    let hash = *pending.tx_hash();
                    info!("📤 approve({token:#x} → {:#x}) on {} tx={hash:#x}",
                        entry.spender, entry.name);
                    match pending.with_required_confirmations(1).get_receipt().await {
                        Ok(r) if r.status() => {
                            info!("✅ approved {token:#x} on {}", entry.name);
                            approved += 1;
                        }
                        Ok(_) => {
                            warn!("❌ approval reverted for {token:#x} on {}", entry.name);
                            failed += 1;
                        }
                        Err(e) => {
                            warn!("❌ receipt error for {token:#x} on {}: {e}", entry.name);
                            failed += 1;
                        }
                    }
                }
                Err(e) => {
                    warn!("❌ send_transaction failed for {token:#x} on {}: {e}", entry.name);
                    failed += 1;
                }
            }
        }
    }

    info!("📊 Approvals: sent={approved} skipped={skipped} failed={failed}");
    if failed > 0 {
        anyhow::bail!("{failed} approval(s) failed — check logs above");
    }
    Ok(())
}
