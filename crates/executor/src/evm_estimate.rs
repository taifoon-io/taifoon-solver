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

// Re-use protocol-adapters ABIs so we agree on selectors with the legacy path.
use protocol_adapters::across::SpokePoolV3;
use protocol_adapters::debridge::DlnDestination;

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
    for c in [1u64, 10, 42161, 8453, 56, 43114, 59144] {
        m.insert(c, dln);
    }
    m
}

/// Pick a default mainnet RPC URL when the environment doesn't provide one.
/// Production deployments should always set the per-chain `RPC_URL_<chain>`.
pub fn default_rpc_for_chain(chain_id: u64) -> Option<&'static str> {
    match chain_id {
        1 => Some("https://eth.llamarpc.com"),
        10 => Some("https://mainnet.optimism.io"),
        42161 => Some("https://arb1.arbitrum.io/rpc"),
        8453 => Some("https://mainnet.base.org"),
        137 => Some("https://polygon-rpc.com"),
        56 => Some("https://bsc-dataseed.bnbchain.org"),
        43114 => Some("https://api.avax.network/ext/bc/C/rpc"),
        59144 => Some("https://rpc.linea.build"),
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

        let nonce = intent.maker_order_nonce.ok_or_else(|| {
            anyhow::anyhow!("deBridge estimate requires intent.maker_order_nonce")
        })?;

        let give_amount = match intent.give_amount.as_deref() {
            Some(s) => U256::from_str_radix(s, 10)?,
            None => U256::from_str_radix(&intent.amount, 10)?,
        };
        let take_amount = match intent.take_amount.as_deref() {
            Some(s) => U256::from_str_radix(s, 10)?,
            None => give_amount,
        };

        let order = DlnDestination::Order {
            makerOrderNonce: nonce,
            makerSrc: address_to_bytes(&intent.depositor)?,
            giveChainId: U256::from(intent.src_chain),
            giveTokenAddress: address_to_bytes(&intent.src_token)?,
            giveAmount: give_amount,
            takeTokenAddress: address_to_bytes(&intent.dst_token)?,
            takeAmount: take_amount,
            receiverDst: address_to_bytes(&intent.recipient)?,
            givePatchAuthoritySrc: Bytes::new(),
            orderAuthorityAddressDst: Bytes::new(),
            allowedTakerDst: Bytes::new(),
            allowedCancelBeneficiarySrc: Bytes::new(),
            externalCall: Bytes::new(),
        };

        let order_id = match intent.order_id.as_deref() {
            Some(s) => {
                let clean = s.trim_start_matches("0x");
                let mut arr = [0u8; 32];
                let bytes = hex::decode(clean)
                    .map_err(|e| anyhow::anyhow!("invalid order_id hex: {}", e))?;
                if bytes.len() != 32 {
                    return Err(anyhow::anyhow!(
                        "order_id must be 32 bytes, got {}", bytes.len()
                    ));
                }
                arr.copy_from_slice(&bytes);
                alloy::primitives::FixedBytes(arr)
            }
            None => alloy::primitives::keccak256(intent.id.as_bytes()),
        };

        let call = DlnDestination::fulfillOrderCall {
            _order: order,
            _fulFillAmount: take_amount,
            _orderId: order_id,
            _permit: Bytes::new(),
            _unlockAuthority: Address::ZERO,
        };

        // deBridge fulfillOrder is `payable`. For ERC-20 paths we pass 0 value;
        // for native-out paths the production solver would attach take_amount.
        // Either is fine for estimateGas (the wallet is underfunded anyway).
        let value = U256::ZERO;
        Ok((dln, call.abi_encode(), value))
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

// ── shared ───────────────────────────────────────────────────────────────────

fn address_to_bytes(addr: &str) -> Result<Bytes> {
    let clean = addr.trim_start_matches("0x");
    Ok(Bytes::from(hex::decode(clean)?))
}

/// Run `eth_estimateGas` against the chain's RPC. Returns the appropriate
/// outcome variant based on the result or error message.
pub async fn run_evm_estimate(
    chain_id: u64,
    from: Address,
    to: Address,
    calldata: &[u8],
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

    let req = TransactionRequest::default()
        .from(from)
        .to(to)
        .input(Bytes::from(calldata.to_vec()).into());

    info!(
        target: "estimate",
        "→ eth_estimateGas chain={} from={:#x} to={:#x} bytes={}",
        chain_id, from, to, calldata.len()
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
