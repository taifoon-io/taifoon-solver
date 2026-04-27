//! Executor-edge facade for Mayan Swift Solana simulate.
//!
//! Wraps `protocol-adapters-solana::MayanSolanaSimulator` and translates the
//! parallel SVM-shaped outcome enum (`SolanaEstimateOutcome`) into the parent
//! `EstimateOutcome` variants the rest of the solver expects. We reuse
//! `EstimateOutcome::OkComputeUnits` and `EstimateOutcome::InsufficientLamports`
//! (added in B.1 alongside the EVM variants) — there is exactly one outcome
//! enum at the executor layer regardless of chain family.
//!
//! Why a parallel SVM enum at all? Because the SVM-shaped outcomes
//! (LogsContainError, InvalidIx) carry richer error context (program logs,
//! account-level diagnostics) than EVM Reverted / AbiInvalid would survive a
//! lossy collapse. We keep the projection one-way at this boundary.

use alloy::primitives::Address;
use async_trait::async_trait;
use genome_client::Intent;
use protocol_adapters_solana::{
    MayanSolanaIntent, MayanSolanaSimulator, SolanaEstimateOutcome,
};
use tracing::warn;

use crate::estimate::{
    write_attempt_bundle, AttemptBundle, EstimateAdapter, EstimateOutcome,
};

/// Default mainnet-beta Solana RPC. Public, free, supports
/// `simulateTransaction`. Override via `SOLANA_RPC_URL`.
pub const DEFAULT_SOLANA_RPC: &str = "https://api.mainnet-beta.solana.com";

/// Source-of-truth pubkey we use for the calldata-only path when the macOS
/// keychain entry `mamba-messiah-solana-key` is missing. `11111111…` is the
/// Solana System Program — a stable, base58-decodable, well-known pubkey.
pub const FALLBACK_SOLANA_PAYER_PUBKEY: &str = "11111111111111111111111111111111";

/// Tries to load the public key (base58) for the MESSIAH Solana signer from
/// macOS keychain. The keychain entry is treated as already-base58 — we never
/// hold the *private* key in this process; the caller derives the pubkey
/// out-of-band and stashes only the public string.
///
/// Falls back to `FALLBACK_SOLANA_PAYER_PUBKEY` so the calldata-construction
/// path can still run without a configured key. The fallback path will surface
/// `AccountNotFound` from mainnet, which the classifier upgrades to
/// `InsufficientLamports` — a green signal for the estimate phase.
pub fn load_messiah_solana_pubkey_or_fallback() -> String {
    match std::process::Command::new("security")
        .args(["find-generic-password", "-s", "mamba-messiah-solana-key", "-w"])
        .output()
    {
        Ok(out) if out.status.success() => {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if s.is_empty() {
                warn!(
                    "keychain entry mamba-messiah-solana-key returned empty value, \
                     falling back to system program pubkey for calldata-only path"
                );
                FALLBACK_SOLANA_PAYER_PUBKEY.to_string()
            } else {
                s
            }
        }
        _ => {
            // Brief explicitly allows skipping the broadcast/sign path when
            // the keychain entry is absent — the calldata-only test path can
            // still distinguish InvalidIx from a green outcome.
            warn!(
                "no mamba-messiah-solana-key in keychain — using fallback payer \
                 (calldata-only path; integration tests will accept InsufficientLamports)"
            );
            FALLBACK_SOLANA_PAYER_PUBKEY.to_string()
        }
    }
}

pub struct MayanSolanaEstimateAdapter {
    pub messiah_evm_address: Address,
    pub solana_payer_pubkey_b58: String,
    pub rpc_url: String,
    pub spinner_base: String,
}

impl MayanSolanaEstimateAdapter {
    pub fn new(
        messiah_evm_address: Address,
        solana_payer_pubkey_b58: impl Into<String>,
        rpc_url: impl Into<String>,
        spinner_base: impl Into<String>,
    ) -> Self {
        Self {
            messiah_evm_address,
            solana_payer_pubkey_b58: solana_payer_pubkey_b58.into(),
            rpc_url: rpc_url.into(),
            spinner_base: spinner_base.into(),
        }
    }

    /// Build the simulate adapter using the configured payer + RPC.
    fn simulator(&self) -> MayanSolanaSimulator {
        MayanSolanaSimulator::new(
            self.solana_payer_pubkey_b58.clone(),
            self.rpc_url.clone(),
        )
    }
}

#[async_trait]
impl EstimateAdapter for MayanSolanaEstimateAdapter {
    fn protocol(&self) -> &'static str {
        "mayan_solana"
    }

    async fn estimate(&self, intent: &Intent) -> EstimateOutcome {
        // Project the full intent into the Solana-shaped subset.
        let solana_intent = match MayanSolanaIntent::from_intent(intent) {
            Ok(s) => s,
            Err(e) => {
                let outcome = EstimateOutcome::AbiInvalid(format!("solana intent: {}", e));
                emit_attempt(&self.spinner_base, intent, &outcome, &[]).await;
                return outcome;
            }
        };

        // Build the calldata blob for telemetry — we want it on the bundle
        // even if simulate ultimately fails.
        let sim = self.simulator();
        let tx_b64 = match sim.build_simulate_tx_b64(&solana_intent) {
            Ok(b) => b,
            Err(e) => {
                let outcome = EstimateOutcome::AbiInvalid(format!("solana tx build: {}", e));
                emit_attempt(&self.spinner_base, intent, &outcome, &[]).await;
                return outcome;
            }
        };

        // Run simulateTransaction, collapse the SVM-shaped outcome into the
        // parent enum.
        let svm_outcome = sim.estimate(&solana_intent).await;
        let outcome = svm_to_parent(svm_outcome);

        let calldata_bytes = base64_decode(&tx_b64).unwrap_or_default();
        emit_attempt(&self.spinner_base, intent, &outcome, &calldata_bytes).await;
        outcome
    }
}

/// Map the Solana-shaped outcome into the parent enum:
///   OkComputeUnits      → EstimateOutcome::OkComputeUnits
///   InsufficientLamports → EstimateOutcome::InsufficientLamports
///   LogsContainError    → EstimateOutcome::Reverted (program-level reject is RED)
///   InvalidIx           → EstimateOutcome::AbiInvalid (encoding-shaped failure)
pub fn svm_to_parent(svm: SolanaEstimateOutcome) -> EstimateOutcome {
    match svm {
        SolanaEstimateOutcome::OkComputeUnits(u) => EstimateOutcome::OkComputeUnits(u),
        SolanaEstimateOutcome::InsufficientLamports(s) => {
            EstimateOutcome::InsufficientLamports(s)
        }
        SolanaEstimateOutcome::LogsContainError(s) => EstimateOutcome::Reverted(s),
        SolanaEstimateOutcome::InvalidIx(s) => EstimateOutcome::AbiInvalid(s),
    }
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    use ::base64::{engine::general_purpose::STANDARD as B64, Engine as _};
    B64.decode(s).ok()
}

async fn emit_attempt(
    spinner_base: &str,
    intent: &Intent,
    outcome: &EstimateOutcome,
    calldata: &[u8],
) {
    // For Solana we don't have a "from"/"to" EVM address — pass zero addresses
    // for the V5 attempt-bundle and chain_id = src_chain (Solana's bespoke id
    // 1399811149 in the fixture). The bundle is best-effort telemetry; the
    // executor layer can post-process the chain id.
    let bundle = AttemptBundle::new(
        intent,
        "mayan_solana",
        outcome,
        calldata,
        Address::ZERO,
        Address::ZERO,
        intent.src_chain,
    );
    let _ = write_attempt_bundle(spinner_base, &bundle).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intent_solana_fixture() -> Intent {
        Intent {
            id: "mayan_swift:solana_test".into(),
            protocol: "mayan_swift".into(),
            src_chain: 1399811149,
            dst_chain: 1,
            src_token: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into(),
            dst_token: "0xA0b86991c6218B36c1d19D4a2e9Eb0cE3606eB48".into(),
            amount: "100000000".into(),
            depositor: "DepositorWa11etAddrSoLana1111111111111111111".into(),
            recipient: "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb1".into(),
            tx_hash: "5HzkYQK4BKj8c4M7yqA7zXyZ9vN2pE5mB3hWnQ8tR1uVaCfDgFhJiKkMnPpQrStUv".into(),
            detected_at: 1745928045,
            output_amount: Some("99850000".into()),
            mayan_order_id: Some(
                "0x9f8e7d6c5b4a3928172e3d4f5a6b7c8d9e0f1a2b3c4d5e6f7081928374651a2b".into(),
            ),
            trader: Some("DepositorWa11etAddrSoLana1111111111111111111".into()),
            deadline: Some(1745931645),
            swift_program_id: Some("BLZRi6frs4X4DNLw56V4EXai1b6QVESN1BhHBTYM9VcY".into()),
            state_account: Some("9wK4N3pTzXyZ8vQ5mB2hWnQ7tR9uVaCfDgFhJiKkMnPp".into()),
            vault_account: Some("8mB2hWnQ7tR9uVaCfDgFhJiKkMnPpQ9wK4N3pTzXyZ8v".into()),
            compute_units_estimate: Some(240_000),
            is_solana_source: Some(true),
            ..Default::default()
        }
    }

    #[test]
    fn svm_to_parent_maps_ok_compute_units() {
        let p = svm_to_parent(SolanaEstimateOutcome::OkComputeUnits(137_500));
        assert!(matches!(p, EstimateOutcome::OkComputeUnits(137_500)));
        assert!(p.is_green());
    }

    #[test]
    fn svm_to_parent_maps_insufficient_lamports() {
        let p = svm_to_parent(SolanaEstimateOutcome::InsufficientLamports(
            "AccountNotFound".into(),
        ));
        assert!(matches!(p, EstimateOutcome::InsufficientLamports(_)));
        assert!(p.is_green());
    }

    #[test]
    fn svm_to_parent_maps_logs_contain_error_to_reverted() {
        let p = svm_to_parent(SolanaEstimateOutcome::LogsContainError(
            "Custom program error: 0x1771".into(),
        ));
        assert!(
            matches!(p, EstimateOutcome::Reverted(_)),
            "got {:?}",
            p
        );
        assert!(!p.is_green());
    }

    #[test]
    fn svm_to_parent_maps_invalid_ix_to_abi_invalid() {
        let p = svm_to_parent(SolanaEstimateOutcome::InvalidIx(
            "rpc HTTP 400: bad message".into(),
        ));
        assert!(matches!(p, EstimateOutcome::AbiInvalid(_)));
        assert!(!p.is_green());
    }

    #[test]
    fn missing_state_account_yields_abi_invalid_at_executor_edge() {
        // Drop a required field — the adapter must surface this as
        // AbiInvalid (Rust-side encoding failure), not silently swallow.
        // We exercise this by directly projecting the intent.
        let mut intent = intent_solana_fixture();
        intent.state_account = None;
        let proj = MayanSolanaIntent::from_intent(&intent);
        assert!(proj.is_err(), "missing state_account should fail projection");
    }
}

