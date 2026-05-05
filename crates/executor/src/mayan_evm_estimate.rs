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
    /// Mayan Swift v2 destination interface.
    ///
    /// `OrderParams` matches the on-chain `Order` struct field-for-field. We
    /// only build the `fulfillOrder` call; the `Settle` event is captured for
    /// reference — solvers monitor it post-fill.
    interface MayanSwift {
        struct OrderParams {
            bytes32 trader;
            uint16 srcChainId;
            bytes32 tokenIn;
            uint64 amountIn;
            bytes32 destAddr;
            uint16 destChainId;
            bytes32 tokenOut;
            uint64 minAmountOut;
            uint64 gasDrop;
            uint64 cancelFee;
            uint64 refundFee;
            uint64 deadline;
            uint8 auctionMode;
            bytes32 random;
        }

        /// Solver-side fulfill call. The encoded VAA is the Wormhole guardian
        /// signature attesting to the source-side `OrderCreated` event.
        function fulfillOrder(
            bytes32 orderHash,
            uint256 fulfillAmount,
            bytes encodedVm,
            OrderParams params,
            address recipient
        ) external payable returns (uint64 sequence);
    }
}

/// Default Mayan Swift addresses by EVM chain id.
/// Verified live: 0x337685fdab40d39bd02028545a4ffa7d287cc3e2 has bytecode on all chains.
/// (0xC38d8a07D4d9E8BC1D32dF50D4b1eAEEbeAfdC1f was stale — no code exists there)
pub fn default_mayan_swift_addresses() -> HashMap<u64, Address> {
    let swift: Address = "0x337685fdab40d39bd02028545a4ffa7d287cc3e2"
        .parse()
        .expect("hardcoded Mayan Swift address");
    let mut m = HashMap::new();
    for chain in [1u64, 10, 56, 137, 8453, 42161, 43114] {
        m.insert(chain, swift);
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

    /// Like `build_estimate_call` but accepts an optional Wormhole VAA.
    /// When `vaa` is `Some`, the real guardian-signed bytes are used.
    /// When `None`, an empty bytes payload is used (synthetic estimate only).
    pub fn build_estimate_call_with_vaa(&self, intent: &Intent, vaa: Option<&[u8]>) -> Result<(Address, Vec<u8>)> {
        let swift = *self
            .swift_addresses
            .get(&intent.dst_chain)
            .ok_or_else(|| anyhow::anyhow!("no Mayan Swift on chain {}", intent.dst_chain))?;

        let order_id = intent.mayan_order_id.as_deref().ok_or_else(|| {
            anyhow::anyhow!("Mayan estimate requires intent.mayan_order_id (not present)")
        })?;
        let order_hash = parse_bytes32(order_id)?;

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

        // The trader address is the depositor on the src chain. Falls back to
        // the intent.depositor when the canonical `trader` field isn't present.
        let trader_str = intent
            .trader
            .as_deref()
            .unwrap_or(&intent.depositor);
        let trader = address_to_bytes32(trader_str)?;

        let token_in = address_to_bytes32(&intent.src_token)?;
        let token_out = address_to_bytes32(&intent.dst_token)?;
        let dest_addr = address_to_bytes32(&intent.recipient)?;

        let amount_in = parse_u64_amount(&intent.amount, "amount")?;
        let min_amount_out = match intent.output_amount.as_deref() {
            Some(s) => parse_u64_amount(s, "output_amount")?,
            None => amount_in,
        };

        let dst_chain_wh = intent.swift_dest_chain_wormhole_id.unwrap_or_else(|| {
            // Fallback wormhole-id mapping for the EVM chains Mayan supports.
            // (Mayan would normally provide this in the genome event.)
            match intent.dst_chain {
                1 => 2,         // Ethereum
                10 => 24,       // Optimism
                56 => 4,        // BSC
                137 => 5,       // Polygon
                8453 => 30,     // Base
                42161 => 23,    // Arbitrum
                43114 => 6,     // Avalanche
                _ => 0,
            }
        });
        let src_chain_wh = match intent.src_chain {
            1 => 2,
            10 => 24,
            56 => 4,
            137 => 5,
            8453 => 30,
            42161 => 23,
            43114 => 6,
            _ => 0,
        };

        let deadline = intent.deadline.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + 3600
        });

        let params = MayanSwift::OrderParams {
            trader,
            srcChainId: src_chain_wh,
            tokenIn: token_in,
            amountIn: amount_in,
            destAddr: dest_addr,
            destChainId: dst_chain_wh,
            tokenOut: token_out,
            minAmountOut: min_amount_out,
            gasDrop: 0,
            cancelFee: 0,
            refundFee: 0,
            deadline,
            auctionMode: 2,
            // The random field is a deterministic 32-byte salt the source-side
            // emitted; we pass zero here because the synthetic fixture doesn't
            // carry it. (A real captured event would have the value.)
            random: FixedBytes::ZERO,
        };

        // The encoded Wormhole VAA. When a real VAA is available (live fills),
        // pass it as `vaa`. For synthetic estimates, None → empty bytes → the
        // contract reverts at VAA verification, surfaces as a synthetic-fixture
        // revert with empty data (acceptable for calldata-shape validation).
        let encoded_vm = match vaa {
            Some(bytes) => Bytes::from(bytes.to_vec()),
            None => Bytes::new(),
        };

        let call = MayanSwift::fulfillOrderCall {
            orderHash: order_hash,
            fulfillAmount: fulfill_amount,
            encodedVm: encoded_vm,
            params,
            recipient: intent.recipient.parse()?,
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

fn parse_u64_amount(s: &str, field: &str) -> Result<u64> {
    // Mayan's OrderParams uses uint64 for amounts (their on-chain norm). USDC
    // at 6 decimals fits comfortably (max ~1.8e13 USD). Native ETH at 18
    // decimals would overflow above ~18 ETH; we cap and warn.
    //
    // MayanPoller stores human-readable decimals from the explorer API (e.g.
    // "0.0216374004" ETH). Detect the decimal point and convert to 18-decimal
    // wei representation so the u64 parse succeeds.
    let s = if s.contains('.') {
        // Parse as f64 and convert to wei (18 decimals). Precision loss above
        // ~18 ETH but acceptable for amount-in estimation.
        let f: f64 = s.parse().map_err(|e| anyhow::anyhow!("invalid {} decimal: {}", field, e))?;
        let wei = (f * 1e18) as u128;
        format!("{}", wei)
    } else {
        s.to_string()
    };
    let big = U256::from_str_radix(&s, 10)
        .map_err(|e| anyhow::anyhow!("invalid {}: {}", field, e))?;
    if big > U256::from(u64::MAX) {
        warn!(
            "Mayan amount field {} exceeds u64::MAX ({}); capping at u64::MAX. \
             18-decimal native amounts above ~18 ETH need a different ABI shape.",
            field, big
        );
        return Ok(u64::MAX);
    }
    Ok(big.try_into().unwrap_or(u64::MAX))
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
            "0x337685fdab40d39bd02028545a4ffa7d287cc3e2"
                .parse::<Address>()
                .unwrap()
        );
        // Selector is the first 4 bytes; assert it matches `fulfillOrder` exactly.
        let selector = &calldata[..4];
        let expected_sel = MayanSwift::fulfillOrderCall::SELECTOR;
        assert_eq!(selector, expected_sel, "selector must match fulfillOrder");
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
