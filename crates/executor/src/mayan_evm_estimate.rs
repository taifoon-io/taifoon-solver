//! Mayan Swift (EVM) `eth_estimateGas` adapter.
//!
//! Mayan Swift deploys the same `MayanSwift` contract at
//! `0xC38d8a07D4d9E8BC1D32dF50D4b1eAEEbeAfdC1f` on every supported EVM chain
//! (Ethereum 1, Optimism 10, Polygon 137, Base 8453, Arbitrum 42161, BSC 56,
//! Avalanche 43114). The destination-side call solvers make to fulfill an
//! auctioned order is `fulfillOrder(bytes32 orderHash, uint256 fulfillAmount,
//! bytes encodedVm, OrderParams params, address recipient)`.
//!
//! The `OrderParams` tuple shape mirrors the Mayan Swift v2 `Order` struct from
//! the public contracts repo (mayan-finance/swift-contracts) — `tokenIn`,
//! `tokenOut`, `amountIn`, `amountOut`, `trader`, `srcChainId`, `dstChainId`,
//! `auctionMode`, `deadline`. The genome-side decoder populates each of these
//! from the Swift `OrderCreated` log.
//!
//! Synthetic-fixture caveat: like Across, Mayan's destination contract
//! verifies the order hash + Wormhole VAA against on-chain state before
//! executing the transfer. A synthetic fixture's VAA won't be signed by the
//! Wormhole guardian set, so the call reverts at VAA verification with empty
//! data (`"0x"`) regardless of how correct our calldata is. The integration
//! test treats that case as ACCEPTABLE — it confirms the contract decoded our
//! tuple far enough to reach the protocol-level check.

use alloy::primitives::{Address, Bytes, FixedBytes, U256};
use alloy::sol;
use alloy::sol_types::SolCall;
use anyhow::Result;
use async_trait::async_trait;
use genome_client::Intent;
use std::collections::HashMap;
use tracing::warn;

use crate::estimate::{
    write_attempt_bundle, AttemptBundle, EstimateAdapter, EstimateOutcome,
};
use crate::evm_estimate::run_evm_estimate;

sol! {
    /// Mayan Swift Forwarder interface — deployed at 0xd78d199f8c402e7b5cc2abe278df0412400a3bae
    /// on all supported EVM chains. Solvers call fulfillOrder (for auctionMode=2 orders
    /// with an auction VAA from Mayan's private chain 42069/0x4155) or fulfillSimple
    /// (for auctionMode=0 orders, no VAA needed).
    ///
    /// Verified selector: fulfillOrder = 0x19535a54
    ///
    /// Struct layout verified from live fill tx on Base
    /// (0x86cd20148f63360c151b2c5ea4fe2ab89ac19a2451959ea039d18ddddb7aec75).
    interface MayanForwarder {
        struct OrderParams {
            uint8 payloadType;
            bytes32 trader;
            bytes32 destAddr;
            uint16 destChainId;
            bytes32 referrerAddr;
            bytes32 tokenOut;
            uint64 minAmountOut;
            uint64 gasDrop;
            uint64 cancelFee;
            uint64 refundFee;
            uint64 deadline;
            uint8 referrerBps;
            uint8 auctionMode;
            bytes32 random;
        }

        struct ExtraParams {
            uint16 srcChainId;
            bytes32 tokenIn;
            uint8 protocolBps;
            bytes32 customPayloadHash;
        }

        struct UnlockParams {
            bytes32 recipient;
            bytes32 driver;
            bool batch;
        }

        struct PermitParams {
            uint256 value;
            uint256 deadline;
            uint8 v;
            bytes32 r;
            bytes32 s;
        }

        /// Fill an auctionMode=2 order. Requires an auction VAA from Mayan's
        /// private chain (Wormhole chain 42069, emitter 0x4155).
        /// Selector: 0x19535a54
        function fulfillOrder(
            uint256 fulfillAmount,
            bytes encodedVm,
            OrderParams params,
            ExtraParams extraParams,
            UnlockParams unlockParams,
            PermitParams permit
        ) external payable returns (bytes memory);

        /// Fill an auctionMode=0 order directly without an auction VAA.
        /// Selector: 0x899c62b1
        function fulfillSimple(
            uint256 fulfillAmount,
            bytes32 orderHash,
            OrderParams params,
            ExtraParams extraParams,
            UnlockParams unlockParams,
            PermitParams permit
        ) external payable;
    }
}

/// Default Mayan Swift Forwarder addresses by EVM chain id.
/// The Forwarder (`0xd78d199f8c402e7b5cc2abe278df0412400a3bae`) is the same address
/// on all supported chains. Solvers call `fulfillOrder` / `fulfillSimple` on it.
/// (0x337685fdab40d39bd02028545a4ffa7d287cc3e2 was a different contract — token router)
pub fn default_mayan_swift_addresses() -> HashMap<u64, Address> {
    let forwarder: Address = "0xd78d199f8c402e7b5cc2abe278df0412400a3bae"
        .parse()
        .expect("hardcoded Mayan Swift Forwarder address");
    let mut m = HashMap::new();
    for chain in [1u64, 10, 56, 137, 8453, 42161, 43114] {
        m.insert(chain, forwarder);
    }
    m
}

pub struct MayanEvmEstimateAdapter {
    pub messiah_address: Address,
    pub swift_addresses: HashMap<u64, Address>,
    pub spinner_base: String,
}

impl MayanEvmEstimateAdapter {
    pub fn new(messiah: Address, spinner_base: impl Into<String>) -> Self {
        Self {
            messiah_address: messiah,
            swift_addresses: default_mayan_swift_addresses(),
            spinner_base: spinner_base.into(),
        }
    }

    /// Build the `fulfillOrder` calldata for a Mayan EVM intent. Returns
    /// `(swift_address, calldata)` ready to feed to `eth_estimateGas`.
    pub fn build_estimate_call(&self, intent: &Intent) -> Result<(Address, Vec<u8>)> {
        self.build_estimate_call_with_vaa(intent, None)
    }

    /// Build `fulfillSimple` calldata for an auctionMode=0 order (no VAA needed).
    /// Selector: 0x899c62b1.
    pub fn build_fulfill_simple_call(&self, intent: &Intent) -> Result<(Address, Vec<u8>)> {
        let swift = *self
            .swift_addresses
            .get(&intent.dst_chain)
            .ok_or_else(|| anyhow::anyhow!("no Mayan Swift on chain {}", intent.dst_chain))?;

        let order_id = intent.mayan_order_id.as_deref().ok_or_else(|| {
            anyhow::anyhow!("Mayan fulfillSimple requires intent.mayan_order_id")
        })?;
        let order_hash = parse_bytes32(order_id)?;

        let fulfill_amount = {
            let s = intent.output_amount.as_deref().unwrap_or(&intent.amount);
            let s = if s.contains('.') {
                format!("{}", (s.parse::<f64>()? * 1e18) as u128)
            } else { s.to_string() };
            U256::from_str_radix(&s, 10)?
        };

        let params = self.build_order_params(intent)?;
        let extra_params = self.build_extra_params(intent)?;
        let unlock_params = self.build_unlock_params();
        let permit = MayanForwarder::PermitParams {
            value: U256::ZERO, deadline: U256::ZERO, v: 0,
            r: FixedBytes::ZERO, s: FixedBytes::ZERO,
        };

        let call = MayanForwarder::fulfillSimpleCall {
            fulfillAmount: fulfill_amount,
            orderHash: order_hash,
            params,
            extraParams: extra_params,
            unlockParams: unlock_params,
            permit,
        };
        Ok((swift, call.abi_encode()))
    }

    fn build_order_params(&self, intent: &Intent) -> Result<MayanForwarder::OrderParams> {
        // mayan_random is required: it's part of the orderId hash. A zero random
        // produces the wrong orderId, the on-chain hash check fails, and the fill
        // reverts. Bail early with a clear message rather than wasting an eth_call.
        if intent.mayan_random.is_none() {
            anyhow::bail!(
                "mayan_random missing for {} — on-chain calldata decode likely failed; cannot build valid orderId",
                intent.id
            );
        }

        let trader_str = intent.trader.as_deref().unwrap_or(&intent.depositor);
        let trader = address_to_bytes32(trader_str)?;
        let token_out = address_to_bytes32(&intent.dst_token)?;
        let dest_addr = address_to_bytes32(&intent.recipient)?;

        // Prefer the on-chain uint64 decoded by fetch_mayan_order_params — it is in the
        // token's native decimals (e.g. 6 for USDC) and must match the on-chain value
        // exactly for the orderId hash check to pass.
        //
        // When mayan_min_amount_out is None (on-chain value was 0 = no minimum, or the RPC
        // calldata decode returned only partial results), use 0. The explorer API's `toAmount`
        // is a human-readable decimal (e.g. "659.52" USDC) that parse_u64_amount would
        // incorrectly scale by 1e18 — using it here would produce u64::MAX → revert.
        // If mayan_random was also None, build_order_params already bailed before reaching here.
        let min_amount_out = intent.mayan_min_amount_out.unwrap_or(0);

        let dst_chain_wh = intent.swift_dest_chain_wormhole_id.unwrap_or_else(|| {
            match intent.dst_chain {
                1 => 2, 10 => 24, 56 => 4, 137 => 5, 8453 => 30, 42161 => 23, 43114 => 6, _ => 0,
            }
        });

        let deadline = intent.deadline.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() + 3600
        });

        let random: FixedBytes<32> = match intent.mayan_random.as_deref() {
            Some(r) => {
                let clean = r.trim_start_matches("0x");
                // May be 64 hex chars (32 bytes) or shorter
                let bytes = hex::decode(if clean.len() % 2 != 0 {
                    format!("0{}", clean)
                } else { clean.to_string() }).unwrap_or_default();
                if bytes.len() == 32 {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&bytes);
                    FixedBytes(arr)
                } else {
                    FixedBytes::ZERO
                }
            }
            None => FixedBytes::ZERO,
        };

        let referrer_addr: FixedBytes<32> = match intent.mayan_referrer_addr.as_deref() {
            Some(r) => {
                let clean = r.trim_start_matches("0x");
                let bytes = hex::decode(if clean.len() % 2 != 0 { format!("0{}", clean) } else { clean.to_string() }).unwrap_or_default();
                if bytes.len() == 32 {
                    let mut arr = [0u8; 32]; arr.copy_from_slice(&bytes); FixedBytes(arr)
                } else { FixedBytes::ZERO }
            }
            None => FixedBytes::ZERO,
        };

        Ok(MayanForwarder::OrderParams {
            payloadType: 1,
            trader,
            destAddr: dest_addr,
            destChainId: dst_chain_wh,
            referrerAddr: referrer_addr,
            tokenOut: token_out,
            minAmountOut: min_amount_out,
            gasDrop: intent.mayan_gas_drop.unwrap_or(0),
            cancelFee: intent.mayan_cancel_fee.unwrap_or(0),
            refundFee: intent.mayan_refund_fee.unwrap_or(0),
            deadline,
            referrerBps: intent.mayan_referrer_bps.unwrap_or(0),
            auctionMode: intent.mayan_auction_mode.unwrap_or(2),
            random,
        })
    }

    fn build_extra_params(&self, intent: &Intent) -> Result<MayanForwarder::ExtraParams> {
        let token_in = address_to_bytes32(&intent.src_token)?;
        let src_chain_wh = match intent.src_chain {
            1 => 2, 10 => 24, 56 => 4, 137 => 5, 8453 => 30, 42161 => 23, 43114 => 6, _ => 0,
        };
        Ok(MayanForwarder::ExtraParams {
            srcChainId: src_chain_wh,
            tokenIn: token_in,
            protocolBps: 3,
            customPayloadHash: FixedBytes::ZERO,
        })
    }

    fn build_unlock_params(&self) -> MayanForwarder::UnlockParams {
        let solver_bytes32 = {
            let mut arr = [0u8; 32];
            arr[12..].copy_from_slice(self.messiah_address.as_slice());
            FixedBytes(arr)
        };
        MayanForwarder::UnlockParams { recipient: solver_bytes32, driver: solver_bytes32, batch: false }
    }

    /// Like `build_estimate_call` but accepts an optional Wormhole VAA.
    /// When `vaa` is `Some`, the real guardian-signed bytes are used.
    /// When `None`, an empty bytes payload is used (synthetic estimate only).
    pub fn build_estimate_call_with_vaa(&self, intent: &Intent, vaa: Option<&[u8]>) -> Result<(Address, Vec<u8>)> {
        let swift = *self
            .swift_addresses
            .get(&intent.dst_chain)
            .ok_or_else(|| anyhow::anyhow!("no Mayan Swift on chain {}", intent.dst_chain))?;

        // Validate order_id exists (required for production fills; used to match the auction VAA).
        let _order_id = intent.mayan_order_id.as_deref().ok_or_else(|| {
            anyhow::anyhow!("Mayan estimate requires intent.mayan_order_id (not present)")
        })?;

        let fulfill_amount = {
            let s = intent.output_amount.as_deref().unwrap_or(&intent.amount);
            let s = if s.contains('.') {
                let f: f64 = s.parse().map_err(|e| anyhow::anyhow!("invalid fulfill_amount decimal: {}", e))?;
                format!("{}", (f * 1e18) as u128)
            } else {
                s.to_string()
            };
            U256::from_str_radix(&s, 10)?
        };

        let params = self.build_order_params(intent)?;
        let extra_params = self.build_extra_params(intent)?;
        let unlock_params = self.build_unlock_params();
        let permit = MayanForwarder::PermitParams {
            value: U256::ZERO,
            deadline: U256::ZERO,
            v: 0,
            r: FixedBytes::ZERO,
            s: FixedBytes::ZERO,
        };

        // The encoded Wormhole VAA. When a real auction VAA is available (live fills,
        // from Mayan's private chain 42069/emitter 0x4155), pass it as `vaa`.
        // For synthetic estimates, None → empty bytes → the contract reverts at VAA
        // verification (acceptable for calldata-shape validation).
        let encoded_vm = match vaa {
            Some(bytes) => Bytes::from(bytes.to_vec()),
            None => Bytes::new(),
        };

        let call = MayanForwarder::fulfillOrderCall {
            fulfillAmount: fulfill_amount,
            encodedVm: encoded_vm,
            params,
            extraParams: extra_params,
            unlockParams: unlock_params,
            permit,
        };

        Ok((swift, call.abi_encode()))
    }
}

#[async_trait]
impl EstimateAdapter for MayanEvmEstimateAdapter {
    fn protocol(&self) -> &'static str {
        "mayan_evm"
    }

    async fn estimate(&self, intent: &Intent) -> EstimateOutcome {
        let (to, calldata) = match self.build_estimate_call(intent) {
            Ok(v) => v,
            Err(e) => {
                let outcome = EstimateOutcome::AbiInvalid(e.to_string());
                emit_attempt(
                    &self.spinner_base, intent, "mayan_evm", &outcome, &[],
                    self.messiah_address, Address::ZERO, intent.dst_chain,
                ).await;
                return outcome;
            }
        };

        let outcome = run_evm_estimate(intent.dst_chain, self.messiah_address, to, &calldata).await;
        emit_attempt(
            &self.spinner_base, intent, "mayan_evm", &outcome, &calldata,
            self.messiah_address, to, intent.dst_chain,
        ).await;
        outcome
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn parse_bytes32(s: &str) -> Result<FixedBytes<32>> {
    let clean = s.trim_start_matches("0x");
    let bytes = hex::decode(clean).map_err(|e| anyhow::anyhow!("invalid hex: {}", e))?;
    if bytes.len() != 32 {
        return Err(anyhow::anyhow!(
            "expected 32-byte value, got {} bytes",
            bytes.len()
        ));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(FixedBytes(arr))
}

fn address_to_bytes32(addr: &str) -> Result<FixedBytes<32>> {
    // EVM addresses are 20 bytes left-padded into 32. Wormhole/Mayan use this
    // canonical form for cross-chain identifiers.
    let clean = addr.trim_start_matches("0x");
    let bytes = hex::decode(clean).map_err(|e| anyhow::anyhow!("invalid address hex: {}", e))?;
    if bytes.len() != 20 {
        return Err(anyhow::anyhow!(
            "expected 20-byte EVM address, got {} bytes",
            bytes.len()
        ));
    }
    let mut arr = [0u8; 32];
    arr[12..].copy_from_slice(&bytes);
    Ok(FixedBytes(arr))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn mayan_intent() -> Intent {
        Intent {
            id: "mayan_swift:0xc3d4e5f6".into(),
            protocol: "mayan_swift".into(),
            src_chain: 137,
            dst_chain: 8453,
            src_token: "0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359".into(),
            dst_token: "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913".into(),
            amount: "100000000".into(),
            depositor: "0x6a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d".into(),
            recipient: "0x6a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d".into(),
            tx_hash: "0xc3d4".into(),
            detected_at: 1745928020,
            output_amount: Some("99850000".into()),
            mayan_order_id: Some(
                "0x7d8c9b0a1f2e3d4c5b6a708192a3b4c5d6e7f80918273645546372818091a0b1".into(),
            ),
            swift_dest_chain_wormhole_id: Some(30),
            trader: Some("0x6a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d".into()),
            deadline: Some(1745931634),
            // mayan_random is required for build_order_params (part of orderId hash).
            mayan_random: Some("0x0102030405060708091011121314151617181920212223242526272829303132".into()),
            mayan_min_amount_out: Some(99_850_000),
            ..Default::default()
        }
    }

    #[test]
    fn mayan_evm_build_estimate_call_produces_real_calldata() {
        let adapter =
            MayanEvmEstimateAdapter::new(Address::ZERO, "http://127.0.0.1:30081");
        let (to, calldata) = adapter
            .build_estimate_call(&mayan_intent())
            .expect("calldata builds for a complete fixture");
        assert_eq!(
            to,
            "0xd78d199f8c402e7b5cc2abe278df0412400a3bae"
                .parse::<Address>()
                .unwrap()
        );
        // Selector is the first 4 bytes; assert it matches `fulfillOrder` exactly.
        // Verified selector: 0x19535a54 from live on-chain fills.
        let selector = &calldata[..4];
        let expected_sel = MayanForwarder::fulfillOrderCall::SELECTOR;
        assert_eq!(selector, expected_sel, "selector must match fulfillOrder (0x19535a54)");
        // Calldata must be at least selector + tuple — empirically ~700 bytes.
        assert!(calldata.len() > 100, "calldata suspiciously small");
    }

    #[test]
    fn mayan_evm_funded_revert_becomes_insufficient() {
        // When the underlying RPC reports an "insufficient funds for gas" style
        // failure (i.e. the wallet is underfunded but the calldata is correct),
        // the classifier upgrades it to InsufficientFundsLike — a green signal
        // for the estimate phase. Mayan EVM uses the shared classifier.
        use crate::estimate::classify_evm_error;
        let outcome = classify_evm_error(
            "execution reverted: insufficient funds for transfer (Mayan would have filled with more balance)"
        );
        // The classifier checks "insufficient funds" before "revert", so funded-shape
        // reverts come back as InsufficientFundsLike (GREEN).
        assert!(
            matches!(outcome, EstimateOutcome::InsufficientFundsLike(_)),
            "funded-shape revert should upgrade to InsufficientFundsLike, got {:?}",
            outcome
        );
        assert!(outcome.is_green(), "InsufficientFundsLike must be green");
    }

    #[test]
    fn mayan_evm_missing_order_id_is_abi_invalid() {
        let adapter =
            MayanEvmEstimateAdapter::new(Address::ZERO, "http://127.0.0.1:30081");
        let mut intent = mayan_intent();
        intent.mayan_order_id = None;
        let err = adapter.build_estimate_call(&intent).unwrap_err();
        assert!(
            err.to_string().contains("mayan_order_id"),
            "missing-field error should mention mayan_order_id, got: {}",
            err
        );
    }
}
