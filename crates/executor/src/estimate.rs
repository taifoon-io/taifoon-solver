//! Estimate harness — the validation primitive for the B-phase fill paths.
//!
//! We do NOT broadcast on mainnet during estimate. The signing wallet is
//! intentionally underfunded; `eth_estimateGas` either returns a positive gas
//! number (calldata is correct) or an error string. Errors that look like
//! "insufficient funds for gas" are *also* a green light — they prove the
//! call would have succeeded with more balance. Real bugs surface as
//! `Reverted` (ABI mismatch / protocol-level reject) or `AbiInvalid`
//! (Rust-side encoding issue, before the RPC ever runs).
//!
//! Every estimate attempt also writes a V5 attempt-bundle to spinner via
//! `POST /api/v5/proof/bundle/attempt`. If the spinner endpoint is missing or
//! unreachable, we log a warn and continue — this is best-effort telemetry.
//! TODO(spinner): add `/api/v5/proof/bundle/attempt` endpoint server-side;
//! current spinner only exposes `/api/v5/proof/bundle/<protocol>/<id>`.

use alloy::primitives::Address;
use async_trait::async_trait;
use genome_client::Intent;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

/// Outcome of an `eth_estimateGas` (or Solana `simulateTransaction`) attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EstimateOutcome {
    /// estimateGas returned a positive gas number — calldata is correct,
    /// ABI matches, contract accepted it. GREEN.
    OkGas(u64),
    /// 'insufficient funds for gas', 'not enough funds', or anything indicating
    /// the call would have succeeded with more wallet balance. ALSO GREEN
    /// (the wallet is intentionally underfunded for the estimate phase).
    InsufficientFundsLike(String),
    /// Real bug: ABI mismatch, wrong destination address, protocol-level reject.
    /// Verbatim revert reason captured.
    Reverted(String),
    /// Calldata couldn't even be encoded (Rust-side type / serde error).
    AbiInvalid(String),
    /// Solana: positive compute units consumed by simulateTransaction. GREEN.
    OkComputeUnits(u64),
    /// Solana: simulation failed because the payer didn't have enough lamports
    /// to cover the rent-exempt or fee. ALSO GREEN.
    InsufficientLamports(String),
    /// LiFi meta-router resolved to a protocol we don't have an adapter for.
    /// Counted as a soft skip, not a failure.
    RouteNotImplemented(String),
}

impl EstimateOutcome {
    /// Whether this outcome counts as a "green" estimate-clean result (i.e.
    /// the calldata + ABI are correct).
    pub fn is_green(&self) -> bool {
        matches!(
            self,
            EstimateOutcome::OkGas(_)
                | EstimateOutcome::InsufficientFundsLike(_)
                | EstimateOutcome::OkComputeUnits(_)
                | EstimateOutcome::InsufficientLamports(_)
        )
    }

    /// Short tag for logs and bundle records.
    pub fn tag(&self) -> &'static str {
        match self {
            EstimateOutcome::OkGas(_) => "ok_gas",
            EstimateOutcome::InsufficientFundsLike(_) => "insufficient_funds_like",
            EstimateOutcome::Reverted(_) => "reverted",
            EstimateOutcome::AbiInvalid(_) => "abi_invalid",
            EstimateOutcome::OkComputeUnits(_) => "ok_compute_units",
            EstimateOutcome::InsufficientLamports(_) => "insufficient_lamports",
            EstimateOutcome::RouteNotImplemented(_) => "route_not_implemented",
        }
    }
}

/// Classify an alloy / RPC error string into one of the EVM outcome variants.
///
/// The classifier is intentionally string-based: alloy 0.8 wraps RPC errors in
/// nested types where the actionable signal lives in the message body. Rather
/// than match against a moving target of error enums we look for stable
/// substrings the upstream node (geth/erigon/reth) emits.
pub fn classify_evm_error(msg: &str) -> EstimateOutcome {
    let lower = msg.to_lowercase();
    if lower.contains("insufficient funds")
        || lower.contains("not enough funds")
        || lower.contains("gas required exceeds allowance")
        || lower.contains("insufficient balance")
    {
        return EstimateOutcome::InsufficientFundsLike(msg.to_string());
    }
    if lower.contains("revert") || lower.contains("execution failed") {
        // All revert forms — including bare `data: "0x"` and stripped "execution reverted" —
        // are protocol-level rejects (RelayFilled, ExclusivityNotMet, allowedTakerDst, etc).
        // We never classify bare reverts as InsufficientFundsLike: those errors always carry
        // an explicit message ("insufficient funds", "not enough funds", etc) and are caught
        // by the branch above. Broadcasting through a bare revert wastes gas.
        return EstimateOutcome::Reverted(msg.to_string());
    }
    // Default unknown errors to AbiInvalid — safer than masking them as green.
    EstimateOutcome::AbiInvalid(msg.to_string())
}

/// Classify a Solana simulation error string into one of the SVM outcome variants.
pub fn classify_solana_error(msg: &str) -> EstimateOutcome {
    let lower = msg.to_lowercase();
    if lower.contains("insufficient lamports")
        || lower.contains("insufficient funds for fee")
        || lower.contains("account does not have enough sol")
    {
        return EstimateOutcome::InsufficientLamports(msg.to_string());
    }
    if lower.contains("custom program error") || lower.contains("program failed") {
        return EstimateOutcome::Reverted(msg.to_string());
    }
    EstimateOutcome::AbiInvalid(msg.to_string())
}

/// Adapter trait: every protocol implementation produces an EstimateOutcome
/// for a given intent. Implementations choose their own RPC primitive
/// (`eth_estimateGas` for EVM, `simulateTransaction` for Solana) and feed the
/// result through the classifier.
#[async_trait]
pub trait EstimateAdapter: Send + Sync {
    /// Protocol slug ("across", "debridge", ...).
    fn protocol(&self) -> &'static str;
    /// Run the estimate for this intent.
    async fn estimate(&self, intent: &Intent) -> EstimateOutcome;
}

// ── V5 attempt-bundle write (best-effort) ────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttemptBundle {
    pub intent_id: String,
    pub protocol: String,
    pub outcome_tag: String,
    pub gas_or_error: String,
    pub calldata_hex: String,
    pub from_address: String,
    pub to_address: String,
    pub chain_id: u64,
    /// Unix-millis timestamp.
    pub ts: u64,
}

impl AttemptBundle {
    pub fn new(
        intent: &Intent,
        protocol: &str,
        outcome: &EstimateOutcome,
        calldata: &[u8],
        from: Address,
        to: Address,
        chain_id: u64,
    ) -> Self {
        let gas_or_error = match outcome {
            EstimateOutcome::OkGas(g) | EstimateOutcome::OkComputeUnits(g) => g.to_string(),
            EstimateOutcome::InsufficientFundsLike(s)
            | EstimateOutcome::InsufficientLamports(s)
            | EstimateOutcome::Reverted(s)
            | EstimateOutcome::AbiInvalid(s)
            | EstimateOutcome::RouteNotImplemented(s) => s.clone(),
        };
        Self {
            intent_id: intent.id.clone(),
            protocol: protocol.to_string(),
            outcome_tag: outcome.tag().to_string(),
            gas_or_error,
            calldata_hex: format!("0x{}", hex::encode(calldata)),
            from_address: format!("{:#x}", from),
            to_address: format!("{:#x}", to),
            chain_id,
            ts: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        }
    }
}

/// Best-effort write of an attempt-bundle to spinner. Returns the server-issued
/// bundle id when the endpoint is reachable, or `None` otherwise.
pub async fn write_attempt_bundle(
    spinner_base: &str,
    bundle: &AttemptBundle,
) -> Option<String> {
    let url = format!("{}/api/v5/proof/bundle/attempt", spinner_base.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .ok()?;
    debug!(target: "estimate", "→ POST {} ({})", url, bundle.outcome_tag);
    match client.post(&url).json(bundle).send().await {
        Ok(resp) if resp.status().is_success() => {
            let id = resp
                .json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|v| v.get("bundle_id").and_then(|s| s.as_str()).map(String::from));
            id
        }
        Ok(resp) => {
            warn!(target: "estimate", "attempt-bundle endpoint returned {}", resp.status());
            None
        }
        Err(e) => {
            warn!(target: "estimate", "attempt-bundle write failed: {} (best-effort)", e);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::address;

    fn dummy_intent() -> Intent {
        Intent {
            id: "across:0xdeadbeef".into(),
            protocol: "across_v3".into(),
            src_chain: 1,
            dst_chain: 42161,
            src_token: "0xA0b86991c6218B36c1d19D4a2e9Eb0cE3606eB48".into(),
            dst_token: "0xaf88d065e77c8cC2239327C5EDb3A432268e5831".into(),
            amount: "100000000".into(),
            depositor: "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb1".into(),
            recipient: "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb1".into(),
            tx_hash: "0xabc".into(),
            detected_at: 1745928012,
            output_amount: Some("99850000".into()),
            deposit_id: Some(4321987),
            ..Default::default()
        }
    }

    #[test]
    fn ok_gas_classifier() {
        // Synthetic gas number: this is the input shape the adapter feeds in.
        let outcome = EstimateOutcome::OkGas(180_000);
        assert!(outcome.is_green());
        assert_eq!(outcome.tag(), "ok_gas");
    }

    #[test]
    fn insufficient_funds_classifier() {
        let outcome = classify_evm_error("err: insufficient funds for gas * price + value");
        assert!(matches!(outcome, EstimateOutcome::InsufficientFundsLike(_)));
        assert!(outcome.is_green());

        // Variant: "not enough funds"
        let outcome2 = classify_evm_error("RPC error: not enough funds in account");
        assert!(matches!(outcome2, EstimateOutcome::InsufficientFundsLike(_)));

        // Variant: gas-required-exceeds-allowance (a balance-shaped failure)
        let outcome3 = classify_evm_error("gas required exceeds allowance (123456)");
        assert!(matches!(outcome3, EstimateOutcome::InsufficientFundsLike(_)));
    }

    #[test]
    fn revert_classifier() {
        let outcome = classify_evm_error("execution reverted: bad selector 0xdeadbeef");
        assert!(matches!(outcome, EstimateOutcome::Reverted(_)));
        assert!(!outcome.is_green());
        assert_eq!(outcome.tag(), "reverted");
    }

    #[test]
    fn abi_invalid_classifier() {
        // alloy encoding errors don't say "revert" or "insufficient" — they fall through
        // to AbiInvalid by default.
        let outcome = classify_evm_error("abi: type mismatch at field depositId expected int64 got uint32");
        assert!(matches!(outcome, EstimateOutcome::AbiInvalid(_)));
        assert!(!outcome.is_green());
    }

    #[test]
    fn solana_classifier_lamports() {
        let outcome = classify_solana_error("Transaction simulation failed: insufficient lamports for fee");
        assert!(matches!(outcome, EstimateOutcome::InsufficientLamports(_)));
        assert!(outcome.is_green());
    }

    #[test]
    fn route_not_implemented_is_not_green() {
        let outcome = EstimateOutcome::RouteNotImplemented("stargate".into());
        assert!(!outcome.is_green());
        assert_eq!(outcome.tag(), "route_not_implemented");
    }

    #[test]
    fn attempt_bundle_serializes_outcome_tag() {
        // Build an attempt-bundle and confirm the tag + gas-or-error get filled correctly.
        let intent = dummy_intent();
        let from = address!("742d35Cc6634C0532925a3b844Bc9e7595f0bEb1");
        let to = address!("5c7BCd6E7De5423a257D81B442095A1a6ced35C5");
        let calldata = vec![0xde, 0xad, 0xbe, 0xef];

        let outcome = EstimateOutcome::OkGas(180_000);
        let bundle = AttemptBundle::new(&intent, "across", &outcome, &calldata, from, to, 42161);
        assert_eq!(bundle.outcome_tag, "ok_gas");
        assert_eq!(bundle.gas_or_error, "180000");
        assert_eq!(bundle.calldata_hex, "0xdeadbeef");
        assert_eq!(bundle.chain_id, 42161);

        // round-trip serialization
        let json = serde_json::to_string(&bundle).expect("serialize");
        let parsed: AttemptBundle = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.outcome_tag, "ok_gas");
        assert_eq!(parsed.intent_id, "across:0xdeadbeef");

        // And revert: error message survives into gas_or_error
        let revert = EstimateOutcome::Reverted("execution reverted: not authorized".into());
        let b2 = AttemptBundle::new(&intent, "across", &revert, &calldata, from, to, 42161);
        assert_eq!(b2.outcome_tag, "reverted");
        assert!(b2.gas_or_error.contains("not authorized"));
    }
}
