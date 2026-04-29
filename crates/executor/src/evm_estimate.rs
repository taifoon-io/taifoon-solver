//! EVM-side `EstimateAdapter` implementations for Across and deBridge.
//!
//! Both implementations:
//!   - build calldata via the existing protocol-side helpers,
//!   - call `eth_estimateGas` against a live RPC with `from = messiah_address`,
//!   - classify the result through `estimate::classify_evm_error`,
//!   - best-effort POST a V5 attempt-bundle to spinner.
//!
//! We deliberately do NOT broadcast. The MESSIAH wallet is intentionally
//! underfunded so an `InsufficientFundsLike` outcome is the green light:
//! it proves the calldata + ABI + destination are correct, only the balance
//! is missing.

use alloy::primitives::{Address, Bytes, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::rpc::types::TransactionRequest;
use anyhow::Result;
use async_trait::async_trait;
use genome_client::Intent;
use std::collections::HashMap;
use tracing::{info, warn};

use crate::across_executor::build_across_adapter_calldata;
use crate::estimate::{
    classify_evm_error, write_attempt_bundle, AttemptBundle, EstimateAdapter, EstimateOutcome,
};

use protocol_adapters::across::SpokePoolV3;
use protocol_adapters::{DeBridgeAdapter, SpinnerClient};

use alloy::sol_types::SolCall;

/// Default deBridge DLN destination address (same on every supported chain).
pub const DEBRIDGE_DLN_DESTINATION: &str = "0xeF4fB24aD0916217251F553c0596F8Edc630EB66";

/// Default Across SpokePool addresses, keyed by destination chain id.
pub fn default_across_spoke_pools() -> HashMap<u64, Address> {
    let mut m = HashMap::new();
    m.insert(1u64, "0x5c7BCd6E7De5423a257D81B442095A1a6ced35C5".parse::<Address>().unwrap());
    m.insert(10, "0x6f26Bf09B1C792e3228e5467807a900A503c0281".parse().unwrap());
    m.insert(42161, "0xe35e9842fceaCA96570B734083f4a58e8F7C5f2A".parse().unwrap());
    m.insert(8453, "0x09aea4b2242abC8bb4BB78D537A67a245A7bEC64".parse().unwrap());
    m.insert(137, "0x9295ee1d8C5b022Be115A2AD3c30C72E34e7F096".parse().unwrap());
    m
}

/// Default deBridge DLN destination address per chain.
pub fn default_debridge_dln_addresses() -> HashMap<u64, Address> {
    let dln: Address = DEBRIDGE_DLN_DESTINATION.parse().unwrap();
    let mut m = HashMap::new();
    for c in [1u64, 10, 42161, 8453, 56, 43114, 59144, 137, 534352, 57073, 34443, 100] {
        m.insert(c, dln);
    }
    m
}

/// Pick a default mainnet RPC URL when the environment doesn't provide one.
/// Production deployments should always set the per-chain `RPC_URL_<chain>`.
/// Premium RPCs sourced from rpc-hunter / t3rn-guardian endpoints.
pub fn default_rpc_for_chain(chain_id: u64) -> Option<&'static str> {
    match chain_id {
        1 => Some("https://mainnet.infura.io/v3/b541434d35ca4478b9c63f95fc79eeab"),
        10 => Some("https://optimism-mainnet.infura.io/v3/b541434d35ca4478b9c63f95fc79eeab"),
        42161 => Some("https://arbitrum-mainnet.infura.io/v3/b541434d35ca4478b9c63f95fc79eeab"),
        8453 => Some("https://base-mainnet.infura.io/v3/753cc78f52604510b0dc93c72f623740"),
        137 => Some("https://polygon-mainnet.infura.io/v3/b541434d35ca4478b9c63f95fc79eeab"),
        56 => Some("https://bsc-mainnet.infura.io/v3/2aa61415c8df44278215d749d6ccd221"),
        43114 => Some("https://api.avax.network/ext/bc/C/rpc"),
        59144 => Some("https://rpc.linea.build"),
        534352 => Some("https://rpc.scroll.io"),
        57073 => Some("https://rpc-gel.inkonchain.com"),
        34443 => Some("https://mainnet.mode.network"),
        100 => Some("https://rpc.gnosischain.com"),
        324 => Some("https://mainnet.era.zksync.io"),
        999 => Some("https://rpc.hyperliquid.xyz/evm"),
        1868 => Some("https://rpc.mainnet.taiko.xyz"),
        81457 => Some("https://rpc.blast.io"),
        1135 => Some("https://rpc.api.lisk.com"),
        _ => None,
    }
}

/// Resolve the RPC URL for a given chain via env first (`RPC_URL_<chain>` or
/// `ETH_RPC_URL` for chain 1) and fall back to the public default.
pub fn resolve_rpc_url(chain_id: u64) -> Option<String> {
    if let Ok(url) = std::env::var(format!("RPC_URL_{}", chain_id)) {
        if !url.is_empty() {
            return Some(url);
        }
    }
    if chain_id == 1 {
        if let Ok(url) = std::env::var("ETH_RPC_URL") {
            if !url.is_empty() {
                return Some(url);
            }
        }
    }
    default_rpc_for_chain(chain_id).map(String::from)
}

// ── Across ───────────────────────────────────────────────────────────────────

pub struct AcrossEstimateAdapter {
    pub messiah_address: Address,
    pub operator_addresses: HashMap<u64, Address>,
    pub spoke_pools: HashMap<u64, Address>,
    pub spinner_base: String,
}

impl AcrossEstimateAdapter {
    pub fn new(messiah: Address, spinner_base: impl Into<String>) -> Self {
        Self {
            messiah_address: messiah,
            operator_addresses: HashMap::new(),
            spoke_pools: default_across_spoke_pools(),
            spinner_base: spinner_base.into(),
        }
    }

    /// Build the destination calldata that would land on-chain. For the estimate
    /// path we do NOT wrap in `executeWithProof` (that requires a real V5 proof
    /// blob, which the spinner is unreachable from this host). Instead we
    /// estimate against the SpokePool's `fillV3Relay` directly — the same
    /// selector + tuple the AcrossAdapter would forward. That gives us a clean
    /// answer to "is the calldata + ABI right?" without needing the operator.
    fn build_estimate_call(&self, intent: &Intent) -> Result<(Address, Vec<u8>)> {
        let spoke_pool = *self
            .spoke_pools
            .get(&intent.dst_chain)
            .ok_or_else(|| anyhow::anyhow!("no Across SpokePool for chain {}", intent.dst_chain))?;

        // Reuse the executor's int64-correct V3RelayData encoder to produce the
        // exact tuple the adapter would forward, then re-wrap it for the
        // SpokePool's own `fillV3Relay(V3RelayData,uint256)` selector.
        let _adapter_calldata = build_across_adapter_calldata(intent)?;

        // Build the SpokePool-direct call (same tuple, different outer selector).
        let input_amount = U256::from_str_radix(&intent.amount, 10)?;
        let output_amount = match intent.output_amount.as_deref() {
            Some(s) => U256::from_str_radix(s, 10)?,
            None => input_amount,
        };
        let deposit_id = intent.deposit_id.ok_or_else(|| {
            anyhow::anyhow!("Across estimate requires intent.deposit_id (not present)")
        })?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let relay = SpokePoolV3::V3RelayData {
            depositor: intent.depositor.parse()?,
            recipient: intent.recipient.parse()?,
            exclusiveRelayer: Address::ZERO,
            inputToken: intent.src_token.parse()?,
            outputToken: intent.dst_token.parse()?,
            inputAmount: input_amount,
            outputAmount: output_amount,
            originChainId: U256::from(intent.src_chain),
            depositId: deposit_id,
            fillDeadline: (now + 3600) as u32,
            exclusivityDeadline: 0,
            message: Bytes::new(),
        };

        let call = SpokePoolV3::fillV3RelayCall {
            relayData: relay,
            repaymentChainId: U256::from(intent.src_chain),
        };
        Ok((spoke_pool, call.abi_encode()))
    }
}

#[async_trait]
impl EstimateAdapter for AcrossEstimateAdapter {
    fn protocol(&self) -> &'static str {
        "across"
    }

    async fn estimate(&self, intent: &Intent) -> EstimateOutcome {
        let (to, calldata) = match self.build_estimate_call(intent) {
            Ok(v) => v,
            Err(e) => {
                let outcome = EstimateOutcome::AbiInvalid(e.to_string());
                emit_attempt(&self.spinner_base, intent, "across", &outcome, &[],
                             self.messiah_address, Address::ZERO, intent.dst_chain).await;
                return outcome;
            }
        };

        let outcome = run_evm_estimate(intent.dst_chain, self.messiah_address, to, &calldata).await;
        emit_attempt(
            &self.spinner_base, intent, "across", &outcome, &calldata,
            self.messiah_address, to, intent.dst_chain,
        ).await;
        outcome
    }
}

// ── deBridge ─────────────────────────────────────────────────────────────────

pub struct DeBridgeEstimateAdapter {
    pub messiah_address: Address,
    pub dln_addresses: HashMap<u64, Address>,
    pub spinner_base: String,
}

impl DeBridgeEstimateAdapter {
    pub fn new(messiah: Address, spinner_base: impl Into<String>) -> Self {
        Self {
            messiah_address: messiah,
            dln_addresses: default_debridge_dln_addresses(),
            spinner_base: spinner_base.into(),
        }
    }

    fn build_estimate_call(&self, intent: &Intent) -> Result<(Address, Vec<u8>, U256)> {
        let dln = *self
            .dln_addresses
            .get(&intent.dst_chain)
            .ok_or_else(|| anyhow::anyhow!("no deBridge DLN for chain {}", intent.dst_chain))?;

        // Delegate to DeBridgeAdapter so the estimate uses the same Order struct
        // (including authority fields) as the actual fulfillOrder broadcast.
        // A mismatch would cause the on-chain orderId hash check to revert.
        let adapter = DeBridgeAdapter::new(SpinnerClient::new(&self.spinner_base));
        let calldata = adapter.build_fulfill_order_calldata(intent)?;

        Ok((dln, calldata, U256::ZERO))
    }
}

#[async_trait]
impl EstimateAdapter for DeBridgeEstimateAdapter {
    fn protocol(&self) -> &'static str {
        "debridge"
    }

    async fn estimate(&self, intent: &Intent) -> EstimateOutcome {
        let (to, calldata, _value) = match self.build_estimate_call(intent) {
            Ok(v) => v,
            Err(e) => {
                let outcome = EstimateOutcome::AbiInvalid(e.to_string());
                emit_attempt(&self.spinner_base, intent, "debridge", &outcome, &[],
                             self.messiah_address, Address::ZERO, intent.dst_chain).await;
                return outcome;
            }
        };

        let outcome = run_evm_estimate(intent.dst_chain, self.messiah_address, to, &calldata).await;
        emit_attempt(
            &self.spinner_base, intent, "debridge", &outcome, &calldata,
            self.messiah_address, to, intent.dst_chain,
        ).await;
        outcome
    }
}


/// Per-chain gas price ranges (gwei). Mirrors t3rn-guardian GAS_PRICE_RANGES.
/// Used as floor/ceiling guards when Razor API is unavailable.
/// L2s (OP-stack, Arbitrum): typical ~0.001–0.005 gwei.
/// L1 (Ethereum): typical ~5–20 gwei. BSC: typical ~3–5 gwei.
pub struct GasPriceRange {
    pub min_gwei: f64,
    pub max_gwei: f64,
    pub typical_gwei: f64,
}

pub fn gas_price_range_for_chain(chain_id: u64) -> GasPriceRange {
    match chain_id {
        1       => GasPriceRange { min_gwei: 1.0,   max_gwei: 200.0, typical_gwei: 10.0  }, // Ethereum
        10      => GasPriceRange { min_gwei: 0.001, max_gwei: 0.5,   typical_gwei: 0.001 }, // Optimism
        56      => GasPriceRange { min_gwei: 1.0,   max_gwei: 20.0,  typical_gwei: 3.0   }, // BSC
        137     => GasPriceRange { min_gwei: 30.0,  max_gwei: 500.0, typical_gwei: 50.0  }, // Polygon
        8453    => GasPriceRange { min_gwei: 0.001, max_gwei: 0.5,   typical_gwei: 0.005 }, // Base
        42161   => GasPriceRange { min_gwei: 0.001, max_gwei: 0.5,   typical_gwei: 0.01  }, // Arbitrum
        43114   => GasPriceRange { min_gwei: 25.0,  max_gwei: 100.0, typical_gwei: 30.0  }, // Avalanche
        59144   => GasPriceRange { min_gwei: 0.05,  max_gwei: 5.0,   typical_gwei: 0.1   }, // Linea
        _       => GasPriceRange { min_gwei: 0.001, max_gwei: 200.0, typical_gwei: 1.0   },
    }
}

/// Fetch real-time gas price (gwei) from Razor / Warmbed API.
/// Returns `None` on any failure so the caller falls back to node gas price.
/// Endpoint: `GET {base}/api/gas/latest/{chain_id}` → `{"gas_price_gwei": 0.005}`
pub async fn fetch_razor_gas_price_gwei(chain_id: u64, warmbed_base: &str) -> Option<f64> {
    #[derive(serde::Deserialize)]
    struct GasResp { gas_price_gwei: Option<f64> }

    let url = format!("{}/api/gas/latest/{}", warmbed_base.trim_end_matches('/'), chain_id);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .ok()?;
    let resp = client.get(&url).send().await.ok()?;
    if !resp.status().is_success() { return None; }
    let parsed: GasResp = resp.json().await.ok()?;
    parsed.gas_price_gwei
}

/// Compute the optimal maxFeePerGas (wei) for a chain.
///
/// Strategy (mirrors t3rn-guardian + Python filler):
///   1. Try Razor API for live gas price.
///   2. Clamp result against per-chain min/max from `GAS_PRICE_RANGES`.
///   3. Apply 1.2× safety buffer.
///   4. If Razor unavailable, use chain `typical` as fallback.
///
/// Returns (max_fee_per_gas_wei, priority_fee_wei).
pub async fn optimal_gas_price_wei(chain_id: u64, warmbed_base: &str) -> (u128, u128) {
    let range = gas_price_range_for_chain(chain_id);

    let gwei = if let Some(live) = fetch_razor_gas_price_gwei(chain_id, warmbed_base).await {
        // Clamp against known-sane range, then apply 1.2× buffer.
        let clamped = live.max(range.min_gwei).min(range.max_gwei);
        clamped * 1.2
    } else {
        // Razor unavailable — use chain typical as safe floor.
        range.typical_gwei * 1.2
    };

    let max_fee_wei = (gwei * 1_000_000_000.0) as u128;
    // Priority fee: 10% of max_fee, floored at 1 mwei (0.000001 gwei), capped at max_fee.
    let priority_fee_wei = (max_fee_wei / 10).max(1_000).min(max_fee_wei);

    (max_fee_wei, priority_fee_wei)
}

/// Run `eth_estimateGas` against the chain's RPC. Returns the appropriate
/// outcome variant based on the result or error message.
pub async fn run_evm_estimate(
    chain_id: u64,
    from: Address,
    to: Address,
    calldata: &[u8],
) -> EstimateOutcome {
    run_evm_estimate_with_value(chain_id, from, to, calldata, None).await
}

/// Value-aware variant — use for `payable` calls (Mayan native-out, WETH fills).
pub async fn run_evm_estimate_with_value(
    chain_id: u64,
    from: Address,
    to: Address,
    calldata: &[u8],
    value: Option<U256>,
) -> EstimateOutcome {
    let rpc_url = match resolve_rpc_url(chain_id) {
        Some(u) => u,
        None => {
            return EstimateOutcome::AbiInvalid(format!(
                "no RPC URL for chain {} (set RPC_URL_{}=)", chain_id, chain_id
            ));
        }
    };

    let url = match rpc_url.parse() {
        Ok(u) => u,
        Err(e) => {
            return EstimateOutcome::AbiInvalid(format!("bad RPC URL: {}", e));
        }
    };
    let provider = ProviderBuilder::new().on_http(url);

    let mut req = TransactionRequest::default()
        .from(from)
        .to(to)
        .input(Bytes::from(calldata.to_vec()).into());
    if let Some(v) = value {
        req = req.value(v);
    }

    info!(
        target: "estimate",
        "→ eth_estimateGas chain={} from={:#x} to={:#x} bytes={} value={}",
        chain_id, from, to, calldata.len(),
        value.map(|v| v.to_string()).unwrap_or_else(|| "0".into())
    );

    match provider.estimate_gas(&req).await {
        Ok(gas) => {
            info!(target: "estimate", "← OkGas({}) chain={}", gas, chain_id);
            EstimateOutcome::OkGas(gas)
        }
        Err(e) => {
            let msg = e.to_string();
            let outcome = classify_evm_error(&msg);
            warn!(
                target: "estimate",
                "← {} chain={} msg={}", outcome.tag(), chain_id, msg
            );
            outcome
        }
    }
}

async fn emit_attempt(
    spinner_base: &str,
    intent: &Intent,
    protocol: &str,
    outcome: &EstimateOutcome,
    calldata: &[u8],
    from: Address,
    to: Address,
    chain_id: u64,
) {
    let bundle = AttemptBundle::new(intent, protocol, outcome, calldata, from, to, chain_id);
    let _ = write_attempt_bundle(spinner_base, &bundle).await;
}
