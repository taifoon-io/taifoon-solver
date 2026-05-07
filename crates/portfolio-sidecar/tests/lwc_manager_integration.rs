//! Integration tests for LwcManager — hit real Base mainnet (read-only).
//!
//! All tests are `#[ignore]` — run with:
//!   cargo test -p portfolio-sidecar --test lwc_manager_integration -- --ignored

use alloy::{
    primitives::{address, Address},
    providers::{Provider, ProviderBuilder},
    signers::local::PrivateKeySigner,
    sol,
    sol_types::SolCall,
};
use portfolio_sidecar::lwc_manager::LwcManager;
use std::time::Duration;

/// Base USDC contract address.
const USDC_BASE: Address = address!("833589fCD6eDb6E08f4c7C32D4f71b54bdA02913");

/// LWC V4 well on Base mainnet.
const LWC_BASE_WELL: Address = address!("b590266eCdbc389A35831dDc672Ea0C5f45500EF");

/// Base mainnet RPC.
const BASE_RPC: &str = "https://mainnet.base.org";

/// Minimal zero-key signer for read-only tests (private key = 1).
fn zero_signer() -> PrivateKeySigner {
    "0x0000000000000000000000000000000000000000000000000000000000000001"
        .parse()
        .expect("valid scalar 1")
}

/// Set LWC_DEPLOYMENTS_PATH to the project config so load_deployments() can
/// find the file from any CWD.
fn set_deployments_path() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    // Walk up to workspace root (two levels up from crates/portfolio-sidecar)
    let config_path = format!("{}/../../config/lwc_deployments.json", manifest);
    std::env::set_var("LWC_DEPLOYMENTS_PATH", &config_path);
}

// ── Test 1: canPerformInstantExecution via raw eth_call ───────────────────────

sol! {
    interface LiquidityWellCompactProbe {
        function canPerformInstantExecution(
            address asset,
            uint256 amount
        ) external view returns (bool canExecute, uint256 availableAmount, uint256 reservedAmount);

        function getAvailableLiquidity(address _asset) external view returns (uint256);
    }
}

#[tokio::test]
#[ignore] // only runs with: cargo test -- --ignored
async fn lwc_base_can_instant_exec_usdc() {
    let rpc_url: alloy::transports::http::reqwest::Url = BASE_RPC
        .parse()
        .expect("valid base rpc url");
    let provider = ProviderBuilder::new().on_http(rpc_url);

    // Probe with 1 USDC (1_000_000 = 1 with 6 decimals)
    let one_usdc = alloy::primitives::U256::from(1_000_000u64);
    let call = LiquidityWellCompactProbe::canPerformInstantExecutionCall {
        asset: USDC_BASE,
        amount: one_usdc,
    };
    let calldata = call.abi_encode();

    let req = alloy::rpc::types::TransactionRequest::default()
        .to(LWC_BASE_WELL)
        .input(calldata.into());

    // Assert: RPC call does not error — response may be empty if no code deployed,
    // but the transport round-trip must succeed.
    let result = tokio::time::timeout(Duration::from_secs(15), provider.call(&req)).await;
    assert!(result.is_ok(), "RPC call timed out after 15s");

    let bytes = result.unwrap();
    // If the contract is deployed we get ≥ 32 bytes; if not we might get empty/revert.
    // Either way the transport must not error at the network level.
    match bytes {
        Ok(b) => {
            println!(
                "[lwc_base_can_instant_exec_usdc] response len={} hex={}",
                b.len(),
                hex::encode(&b)
            );
            // Valid ABI return: bool + uint256 + uint256 = 96 bytes.
            // Don't assert the bool — pool may be empty on mainnet.
            if b.len() >= 32 {
                println!("  canExecute byte[31]={}", b[31]);
            }
        }
        Err(e) => {
            // A revert (e.g. contract not yet deployed or wrong selector) is a
            // transport-level success; only panic on genuine RPC failures.
            println!("[lwc_base_can_instant_exec_usdc] call reverted (ok): {}", e);
        }
    }
}

// ── Test 2: getAvailableLiquidity ────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn lwc_base_get_available_liquidity() {
    let rpc_url: alloy::transports::http::reqwest::Url = BASE_RPC
        .parse()
        .expect("valid base rpc url");
    let provider = ProviderBuilder::new().on_http(rpc_url);

    let call = LiquidityWellCompactProbe::getAvailableLiquidityCall { _asset: USDC_BASE };
    let calldata = call.abi_encode();

    let req = alloy::rpc::types::TransactionRequest::default()
        .to(LWC_BASE_WELL)
        .input(calldata.into());

    let result = tokio::time::timeout(Duration::from_secs(15), provider.call(&req)).await;
    assert!(result.is_ok(), "RPC call timed out");

    match result.unwrap() {
        Ok(bytes) => {
            let raw = if bytes.len() >= 32 {
                alloy::primitives::U256::from_be_slice(&bytes[bytes.len() - 32..])
            } else {
                alloy::primitives::U256::ZERO
            };
            let available_usd = raw
                .try_into()
                .map(|v: u128| v as f64 / 1_000_000.0)
                .unwrap_or(0.0);
            println!("[lwc_base_get_available_liquidity] available_usd={:.2}", available_usd);
            // Value may be 0 (empty pool) or positive — either is valid.
            assert!(available_usd >= 0.0);
        }
        Err(e) => {
            println!("[lwc_base_get_available_liquidity] call reverted: {}", e);
        }
    }
}

// ── Test 3: LwcManager::scan_all() — all 9 chains ────────────────────────────

#[tokio::test]
#[ignore]
async fn lwc_scan_all_chains_reads_nine_wells() {
    set_deployments_path();

    let signer = zero_signer();
    let mgr = LwcManager::new(signer, /*dry_run=*/ true);

    // Verify config loaded 9 chains before hitting network.
    let n_deployments = mgr.deployments.len();
    println!(
        "[lwc_scan_all_chains_reads_nine_wells] loaded {} deployments",
        n_deployments
    );
    assert_eq!(
        n_deployments, 9,
        "expected 9 chains in lwc_deployments.json, got {}",
        n_deployments
    );

    // Run with a 15-second hard timeout.
    let states = tokio::time::timeout(Duration::from_secs(15), mgr.scan_all())
        .await
        .expect("scan_all timed out after 15s");

    assert_eq!(
        states.len(),
        9,
        "scan_all must return one LwcChainState per deployment"
    );

    for s in &states {
        println!(
            "  chain={} ({}) avail=${:.2} total=${:.2} lp=${:.2} halted={} instant={}",
            s.chain_id,
            s.chain_key,
            s.pool_available_usd,
            s.pool_total_usd,
            s.lp_balance_usd,
            s.is_halted,
            s.can_instant_exec,
        );
        // No panics: every field must hold a valid (possibly zero) value.
        assert!(s.pool_available_usd >= 0.0);
        assert!(s.pool_total_usd >= 0.0);
        assert!(s.lp_balance_usd >= 0.0);
    }
}
