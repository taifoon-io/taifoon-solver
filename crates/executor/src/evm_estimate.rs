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

use crate::across_executor::build_across_spoke_pool_calldata_with_relayer;
use crate::estimate::{
    classify_evm_error, write_attempt_bundle, AttemptBundle, EstimateAdapter, EstimateOutcome,
};

use protocol_adapters::{DeBridgeAdapter, SpinnerClient};

/// DlnDestination contract address — where solvers call fulfillOrder (same on all chains).
/// Different from DlnSource (0xeF4fB24...) which is for order creation / claimUnlock.
pub const DEBRIDGE_DLN_DESTINATION: &str = "0xE7351Fd770A37282b91D153Ee690B63579D6dd7f";

/// Default Across SpokePool addresses, keyed by destination chain id.
/// Must stay in sync with chain_wiring.json across_adapter entries.
pub fn default_across_spoke_pools() -> HashMap<u64, Address> {
    let mut m = HashMap::new();
    m.insert(1u64,  "0x5c7BCd6E7De5423a257D81B442095A1a6ced35C5".parse::<Address>().unwrap()); // Ethereum
    m.insert(10,    "0x6f26Bf09B1C792e3228e5467807a900A503c0281".parse().unwrap()); // Optimism
    m.insert(137,   "0x9295ee1d8C5b022Be115A2AD3c30C72E34e7F096".parse().unwrap()); // Polygon
    m.insert(324,   "0xe0B015E54d54fc84a6cB9B666099c46adE9335FF".parse().unwrap()); // zkSync Era
    m.insert(8453,  "0x09aea4b2242abC8bb4BB78D537A67a245A7bEC64".parse().unwrap()); // Base
    m.insert(34443, "0x3baD7AD0728f9917d1Bf08af5782dCbD516cDd96".parse().unwrap()); // Mode
    m.insert(42161, "0xe35e9842fceaCA96570B734083f4a58e8F7C5f2A".parse().unwrap()); // Arbitrum
    m.insert(57073, "0xeF684C38F94F48775959ECf2012D7E864ffb9dd4".parse().unwrap()); // Ink
    m.insert(59144, "0x7e63a5f1a8F0B4D0934B2f2327DAEd3f6bb2Ee75".parse().unwrap()); // Linea
    m.insert(534352,"0x3baD7AD0728f9917d1Bf08af5782dCbD516cDd96".parse().unwrap()); // Scroll
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
/// Premium RPCs sourced from rpc-hunter / partner guardian endpoints.
pub fn default_rpc_for_chain(chain_id: u64) -> Option<&'static str> {
    match chain_id {
        1 => Some("https://ethereum-rpc.publicnode.com"),
        10 => Some("https://mainnet.optimism.io"),
        42161 => Some("https://arb1.arbitrum.io/rpc"),
        8453 => Some("https://base-rpc.publicnode.com"),
        137 => Some("https://polygon-bor-rpc.publicnode.com"),
        56 => Some("https://bsc-dataseed.binance.org"),
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

    /// Build the destination calldata for gas estimation.
    /// Uses the same `fillRelay(bytes32-RelayData, repaymentChainId, repaymentAddress)` calldata
    /// as the actual broadcast path so selector and ABI encoding are identical.
    /// The messiah address stands in as the relayer for the estimate.
    fn build_estimate_call(&self, intent: &Intent) -> Result<(Address, Vec<u8>)> {
        let spoke_pool = *self
            .spoke_pools
            .get(&intent.dst_chain)
            .ok_or_else(|| anyhow::anyhow!("no Across SpokePool for chain {}", intent.dst_chain))?;

        // Delegate to the broadcast calldata builder with messiah as the repaymentAddress.
        // This ensures estimate and broadcast use the same selector (0xdeff4b24) and tuple layout.
        let calldata = build_across_spoke_pool_calldata_with_relayer(intent, Some(self.messiah_address), Some(intent.dst_chain))?;
        Ok((spoke_pool, calldata))
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
        let calldata = adapter.build_fulfill_order_calldata(intent, self.messiah_address)?;

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


/// Per-chain gas price ranges (gwei). Mirrors the partner guardian GAS_PRICE_RANGES.
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
        8453    => GasPriceRange { min_gwei: 0.001, max_gwei: 0.5,   typical_gwei: 0.05  }, // Base
        42161   => GasPriceRange { min_gwei: 0.01,  max_gwei: 0.5,   typical_gwei: 0.1   }, // Arbitrum
        43114   => GasPriceRange { min_gwei: 25.0,  max_gwei: 100.0, typical_gwei: 30.0  }, // Avalanche
        59144   => GasPriceRange { min_gwei: 0.05,  max_gwei: 5.0,   typical_gwei: 0.1   }, // Linea
        324     => GasPriceRange { min_gwei: 0.05,  max_gwei: 5.0,   typical_gwei: 0.25  }, // zkSync Era
        534352  => GasPriceRange { min_gwei: 0.001, max_gwei: 1.0,   typical_gwei: 0.005 }, // Scroll
        34443   => GasPriceRange { min_gwei: 0.001, max_gwei: 0.5,   typical_gwei: 0.001 }, // Mode
        57073   => GasPriceRange { min_gwei: 0.001, max_gwei: 0.5,   typical_gwei: 0.001 }, // Ink
        100     => GasPriceRange { min_gwei: 1.0,   max_gwei: 20.0,  typical_gwei: 2.0   }, // Gnosis
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

/// Fetch `eth_gasPrice` from the chain's RPC as a secondary gas price source.
/// Returns the price in gwei, or `None` on failure.
async fn fetch_rpc_gas_price_gwei(chain_id: u64) -> Option<f64> {
    let rpc_url = resolve_rpc_url(chain_id)?;
    let url = rpc_url.parse().ok()?;
    let provider = ProviderBuilder::new().on_http(url);
    let wei = provider.get_gas_price().await.ok()?;
    Some(wei as f64 / 1_000_000_000.0)
}

/// Compute the optimal maxFeePerGas (wei) for a chain.
///
/// Strategy (mirrors the partner guardian + Python filler):
///   1. Try Razor API for live gas price.
///   2. Clamp result against per-chain min/max from `GAS_PRICE_RANGES`.
///   3. Apply 1.2× safety buffer.
///   4. If Razor unavailable, try `eth_gasPrice` RPC as secondary fallback.
///   5. If both fail, use chain `typical` as static floor.
///
/// Returns (max_fee_per_gas_wei, priority_fee_wei).
pub async fn optimal_gas_price_wei(chain_id: u64, warmbed_base: &str) -> (u128, u128) {
    let range = gas_price_range_for_chain(chain_id);

    let gwei = if let Some(live) = fetch_razor_gas_price_gwei(chain_id, warmbed_base).await {
        // Clamp against known-sane range, then apply 1.2× buffer.
        let clamped = live.max(range.min_gwei).min(range.max_gwei);
        clamped * 1.2
    } else if let Some(rpc_price) = fetch_rpc_gas_price_gwei(chain_id).await {
        // Razor unavailable — use live eth_gasPrice from the node, clamped and buffered.
        let clamped = rpc_price.max(range.min_gwei).min(range.max_gwei);
        warn!(
            target: "estimate",
            "gas: Razor unavailable for chain={}, using eth_gasPrice={:.4} gwei", chain_id, rpc_price
        );
        clamped * 1.2
    } else {
        // Both Razor and RPC failed — use chain typical as static floor.
        warn!(
            target: "estimate",
            "gas: both Razor and eth_gasPrice failed for chain={}, using typical={} gwei",
            chain_id, range.typical_gwei
        );
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
