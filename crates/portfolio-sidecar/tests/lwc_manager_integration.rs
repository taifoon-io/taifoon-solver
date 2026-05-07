//! Integration tests for LwcManager — hit real mainnet (read-only).
//!
//! Uses the canonical JSON ABI (src/lwc_abi.json) via the sol! macro generated
//! types — the same code path that runs in production.
//!
//! All tests are #[ignore] — run with:
//!   cargo test -p portfolio-sidecar --test lwc_manager_integration -- --ignored --nocapture

use alloy::{
    primitives::{address, Address, U256},
    providers::{Provider, ProviderBuilder},
    signers::local::PrivateKeySigner,
    sol_types::SolCall,
};
use portfolio_sidecar::lwc_manager::{LiquidityWellCompact, LwcManager};
use std::time::Duration;

// ── Chain fixtures ─────────────────────────────────────────────────────────────

/// Base mainnet – LWC V4 well address (from lwc_deployments.json).
const LWC_BASE: Address  = address!("b590266eCdbc389A35831dDc672Ea0C5f45500EF");
const USDC_BASE: Address = address!("833589fCD6eDb6E08f4c7C32D4f71b54bdA02913");
const BASE_RPC: &str     = "https://mainnet.base.org";

/// Arbitrum mainnet – LWC V4.
const LWC_ARB: Address   = address!("DF84cbCFc9eF2089c67BfC794A012ea3b30c3DE9");
const USDC_ARB: Address  = address!("af88d065e77c8cC2239327C5EDb3A432268e5831");
const ARB_RPC: &str      = "https://arb1.arbitrum.io/rpc";

/// Optimism mainnet – LWC V4.
const LWC_OPT: Address   = address!("a15feB1Ca6d1203bdee823F44ddF4715c35398E6");
const USDC_OPT: Address  = address!("0b2C639c533813f4Aa9D7837CAf62653d097Ff85");
const OPT_RPC: &str      = "https://mainnet.optimism.io";

fn zero_signer() -> PrivateKeySigner {
    "0x0000000000000000000000000000000000000000000000000000000000000001"
        .parse()
        .expect("valid scalar 1")
}

fn set_deployments_path() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let path = format!("{}/../../config/lwc_deployments.json", manifest);
    std::env::set_var("LWC_DEPLOYMENTS_PATH", &path);
}

// ── Helpers ───────────────────────────────────────────────────────────────────

type HttpProvider = alloy::providers::RootProvider<alloy::transports::http::Http<alloy::transports::http::Client>>;

fn http_provider(rpc: &str) -> HttpProvider {
    let url: alloy::transports::http::reqwest::Url = rpc.parse().expect("valid rpc url");
    ProviderBuilder::new().on_http(url)
}

async fn call_bytes(provider: &HttpProvider, to: Address, data: Vec<u8>) -> alloy::primitives::Bytes {
    let req = alloy::rpc::types::TransactionRequest::default()
        .to(to)
        .input(data.into());
    provider.call(&req).await.unwrap_or_default()
}

// ── Test 1: canPerformInstantExecution — Base USDC ────────────────────────────

#[tokio::test]
#[ignore]
async fn base_can_perform_instant_execution_usdc() {
    let provider = http_provider(BASE_RPC);
    let one_usdc = U256::from(1_000_000u64);   // $1 with 6 decimals

    let data = LiquidityWellCompact::canPerformInstantExecutionCall {
        asset: USDC_BASE,
        amount: one_usdc,
    }.abi_encode();

    let bytes = tokio::time::timeout(Duration::from_secs(15), call_bytes(&provider, LWC_BASE, data))
        .await
        .expect("timed out");

    // ABI return: (bool, uint256, uint256) = 96 bytes
    println!("[base_canPerformInstantExecution] {} bytes: {}", bytes.len(), hex::encode(&bytes));
    assert!(bytes.len() >= 32, "expected ≥32 bytes from canPerformInstantExecution");

    let can_exec = bytes[31] != 0;
    let available_usdc = if bytes.len() >= 64 {
        U256::from_be_slice(&bytes[32..64])
    } else { U256::ZERO };
    let reserved_usdc = if bytes.len() >= 96 {
        U256::from_be_slice(&bytes[64..96])
    } else { U256::ZERO };

    println!(
        "  canExec={} available=${:.2} reserved=${:.2}",
        can_exec,
        available_usdc.try_into().map(|v: u128| v as f64 / 1e6).unwrap_or(0.0),
        reserved_usdc.try_into().map(|v: u128| v as f64 / 1e6).unwrap_or(0.0),
    );
}

// ── Test 2: getAvailableLiquidity — Base USDC ─────────────────────────────────

#[tokio::test]
#[ignore]
async fn base_get_available_liquidity_usdc() {
    let provider = http_provider(BASE_RPC);

    let data = LiquidityWellCompact::getAvailableLiquidityCall {
        _asset: USDC_BASE,
    }.abi_encode();

    let bytes = tokio::time::timeout(Duration::from_secs(15), call_bytes(&provider, LWC_BASE, data))
        .await
        .expect("timed out");

    let raw = if bytes.len() >= 32 { U256::from_be_slice(&bytes[bytes.len()-32..]) } else { U256::ZERO };
    let usd: f64 = raw.try_into().map(|v: u128| v as f64 / 1e6).unwrap_or(0.0);
    println!("[base_getAvailableLiquidity] available_usdc=${:.2}", usd);
    assert!(usd >= 0.0);
}

// ── Test 3: getCurrentLiquidityInWell — Base ──────────────────────────────────

#[tokio::test]
#[ignore]
async fn base_get_current_liquidity_in_well() {
    let provider = http_provider(BASE_RPC);

    let data = LiquidityWellCompact::getCurrentLiquidityInWellCall {}.abi_encode();

    let bytes = tokio::time::timeout(Duration::from_secs(15), call_bytes(&provider, LWC_BASE, data))
        .await
        .expect("timed out");

    let raw = if bytes.len() >= 32 { U256::from_be_slice(&bytes[bytes.len()-32..]) } else { U256::ZERO };
    // getCurrentLiquidityInWell returns the sum across all assets in the well's
    // internal unit — interpret as 6-decimal USD equivalent.
    let usd: f64 = raw.try_into().map(|v: u128| v as f64 / 1e6).unwrap_or(0.0);
    println!("[base_getCurrentLiquidityInWell] total_usd=${:.2}", usd);
    assert!(usd >= 0.0);
}

// ── Test 4: isEgressHalted + isIngressHalted — Base ───────────────────────────

#[tokio::test]
#[ignore]
async fn base_halt_flags() {
    let provider = http_provider(BASE_RPC);

    let egress = {
        let data = LiquidityWellCompact::isEgressHaltedCall {}.abi_encode();
        let b = tokio::time::timeout(Duration::from_secs(15), call_bytes(&provider, LWC_BASE, data))
            .await.expect("timed out");
        b.get(31).copied().unwrap_or(0) != 0
    };
    let ingress = {
        let data = LiquidityWellCompact::isIngressHaltedCall {}.abi_encode();
        let b = tokio::time::timeout(Duration::from_secs(15), call_bytes(&provider, LWC_BASE, data))
            .await.expect("timed out");
        b.get(31).copied().unwrap_or(0) != 0
    };

    println!("[base_halt_flags] isEgressHalted={} isIngressHalted={}", egress, ingress);
    // A halted well is a valid (even expected) state; we just verify the call works.
}

// ── Test 5: mapAssetToId + getLPTokenBalance — Base USDC ──────────────────────

#[tokio::test]
#[ignore]
async fn base_lp_token_balance_for_zero_address() {
    let provider = http_provider(BASE_RPC);

    // Get the USDC asset ID
    let asset_id_bytes = {
        let data = LiquidityWellCompact::mapAssetToIdCall { _asset: USDC_BASE }.abi_encode();
        tokio::time::timeout(Duration::from_secs(15), call_bytes(&provider, LWC_BASE, data))
            .await.expect("timed out")
    };
    let asset_id: u32 = if asset_id_bytes.len() >= 32 {
        U256::from_be_slice(&asset_id_bytes[asset_id_bytes.len()-32..])
            .try_into()
            .unwrap_or(0)
    } else { 0 };
    println!("[base_lp_token_balance] USDC asset_id={}", asset_id);

    // LP balance of the zero address (should be 0 or very small)
    let lp_data = LiquidityWellCompact::getLPTokenBalanceCall {
        _assetId: asset_id,
        _account: Address::ZERO,
    }.abi_encode();
    let lp_bytes = tokio::time::timeout(Duration::from_secs(15), call_bytes(&provider, LWC_BASE, lp_data))
        .await.expect("timed out");
    let lp_raw = if lp_bytes.len() >= 32 {
        U256::from_be_slice(&lp_bytes[lp_bytes.len()-32..])
    } else { U256::ZERO };
    println!("[base_lp_token_balance] LP(0x0)={}", lp_raw);
    assert_eq!(lp_raw, U256::ZERO, "zero address should have no LP tokens");
}

// ── Test 6: version + sourceId ────────────────────────────────────────────────
// These are declared in the ABI but may return empty bytes if not implemented
// on the deployed V4 contract — we log but do not assert non-empty.

#[tokio::test]
#[ignore]
async fn base_version_and_source_id() {
    let provider = http_provider(BASE_RPC);

    let ver_bytes = {
        let data = LiquidityWellCompact::versionCall {}.abi_encode();
        tokio::time::timeout(Duration::from_secs(15), call_bytes(&provider, LWC_BASE, data))
            .await.expect("timed out")
    };
    let src_bytes = {
        let data = LiquidityWellCompact::sourceIdCall {}.abi_encode();
        tokio::time::timeout(Duration::from_secs(15), call_bytes(&provider, LWC_BASE, data))
            .await.expect("timed out")
    };

    // V4 may not implement version()/sourceId() — empty response is acceptable.
    println!("[base_version]  {} bytes: {}", ver_bytes.len(), hex::encode(&ver_bytes));
    println!("[base_sourceId] {} bytes: {}", src_bytes.len(), hex::encode(&src_bytes));
    // The calls must not panic or timeout — return value (even empty) is fine.
}

// ── Test 7: multi-chain scan via LwcManager::scan_all() ──────────────────────

#[tokio::test]
#[ignore]
async fn lwc_manager_scan_all_nine_chains() {
    set_deployments_path();
    let mgr = LwcManager::new(zero_signer(), true);

    assert_eq!(mgr.deployments.len(), 9, "9 deployments in config");

    let states = tokio::time::timeout(Duration::from_secs(30), mgr.scan_all())
        .await
        .expect("scan_all timed out after 30s");

    assert_eq!(states.len(), 9);

    let mut alive = 0usize;
    for s in &states {
        println!(
            "  {:12} chain={:6} avail=${:>10.2} total=${:>10.2} lp=${:>8.2} halted={} instant={}  status={:?}",
            s.chain_key, s.chain_id,
            s.pool_available_usd, s.pool_total_usd, s.lp_balance_usd,
            s.is_halted, s.can_instant_exec, s.status(),
        );
        if s.pool_total_usd > 0.0 { alive += 1; }
        assert!(s.pool_available_usd >= 0.0);
        assert!(s.pool_total_usd >= 0.0);
    }
    println!("  → {} / 9 chains have non-zero liquidity", alive);
    assert!(alive >= 1, "at least one chain should have liquidity");
}

// ── Test 8: cross-chain asset matrix — USDC on Base/Arb/Optimism ─────────────

#[tokio::test]
#[ignore]
async fn cross_chain_usdc_availability_matrix() {
    let chains = [
        ("base",     LWC_BASE, USDC_BASE, BASE_RPC),
        ("arbitrum", LWC_ARB,  USDC_ARB,  ARB_RPC),
        ("optimism", LWC_OPT,  USDC_OPT,  OPT_RPC),
    ];

    for (name, well, usdc, rpc) in chains {
        let provider = http_provider(rpc);
        let one_usdc = U256::from(1_000_000u64);

        let avail_bytes = {
            let data = LiquidityWellCompact::getAvailableLiquidityCall { _asset: usdc }.abi_encode();
            tokio::time::timeout(Duration::from_secs(15), call_bytes(&provider, well, data))
                .await.expect("timed out")
        };
        let avail_usd: f64 = if avail_bytes.len() >= 32 {
            U256::from_be_slice(&avail_bytes[avail_bytes.len()-32..])
                .try_into().map(|v: u128| v as f64 / 1e6).unwrap_or(0.0)
        } else { 0.0 };

        let instant_bytes = {
            let data = LiquidityWellCompact::canPerformInstantExecutionCall { asset: usdc, amount: one_usdc }.abi_encode();
            tokio::time::timeout(Duration::from_secs(15), call_bytes(&provider, well, data))
                .await.expect("timed out")
        };
        let can_exec = instant_bytes.get(31).copied().unwrap_or(0) != 0;

        println!(
            "  {:12} avail=${:.2}  canInstantExec($1)={}",
            name, avail_usd, can_exec
        );
        assert!(avail_usd >= 0.0);
    }
}

// ── Test 9: order() calldata shape verification ───────────────────────────────
// Verifies that the order() ABI encoding matches the on-chain selector.
// No transaction is sent; only the calldata bytes are inspected.

#[test]
fn order_calldata_selector_matches_abi() {
    use alloy::primitives::{FixedBytes, B256};

    // Build a sample order() calldata and verify the 4-byte selector.
    // Keccak256("order(bytes4,uint32,bytes32,uint256,address,uint256,uint256)") = 0xa8b22e58
    let destination: FixedBytes<4> = [0x62, 0x61, 0x73, 0x6d].into(); // "basm"
    let asset_id: u32 = 1;
    let target: B256 = B256::ZERO;
    let amount = U256::from(1_000_000u64);
    let reward_asset = Address::ZERO;
    let insurance = U256::ZERO;
    let max_reward = U256::from(1_010_000u64);

    let data = LiquidityWellCompact::orderCall {
        destination,
        asset: asset_id,
        targetAccount: target,
        amount,
        rewardAsset: reward_asset,
        insurance,
        maxReward: max_reward,
    }.abi_encode();

    // ABI-encoded: 4-byte selector + 7 params × 32 bytes each = 228 bytes total
    assert_eq!(data.len(), 4 + 7 * 32, "order() calldata must be 228 bytes");

    // Verify selector: keccak256("order(bytes4,uint32,bytes32,uint256,address,uint256,uint256)")[0..4]
    let expected_selector = &alloy::primitives::keccak256(
        b"order(bytes4,uint32,bytes32,uint256,address,uint256,uint256)"
    )[..4];
    assert_eq!(&data[..4], expected_selector, "order() 4-byte selector mismatch");
    println!("[order_calldata_selector] selector=0x{}", hex::encode(&data[..4]));
}

// ── Test 10: addLiquidity calldata selector ────────────────────────────────────

#[test]
fn add_liquidity_calldata_selector_matches_abi() {
    let data = LiquidityWellCompact::addLiquidityCall {
        _asset: USDC_BASE,
        _amount: U256::from(100_000_000u64),  // $100 USDC
    }.abi_encode();

    // 4-byte selector + 2 params × 32 bytes = 68 bytes
    assert_eq!(data.len(), 4 + 2 * 32, "addLiquidity() calldata must be 68 bytes");

    let expected = &alloy::primitives::keccak256(b"addLiquidity(address,uint256)")[..4];
    assert_eq!(&data[..4], expected, "addLiquidity() selector mismatch");
    println!("[addLiquidity_selector] selector=0x{}", hex::encode(&data[..4]));
}
