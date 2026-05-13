//! spinner-signer — scoped session-key signing for cross-chain Spinner
//! operators.
//!
//! ## The pattern
//!
//! A Spinner's master treasury (Safe on EVM, Squads multisig on Solana)
//! grants the running solver a **scoped session key** — a hot key that
//! the binary holds, but which is restricted on-chain to only the calls
//! the solver actually needs to make. Spend caps, target allowlists, and
//! expiry are enforced by the scope contract on chain, not by the binary
//! itself.
//!
//! ## Two impls share one trait
//!
//! | Chain | Master | Scope contract | Session key |
//! |---|---|---|---|
//! | EVM | Safe (`safe-smart-account`) | Zodiac Roles Module | EOA |
//! | Solana | Squads V4 (`squads-protocol/v4`) | Squads spending-limit | ed25519 keypair |
//!
//! Both expose [`SessionSigner`]. Both have a dry-run path that catches
//! policy violations client-side BEFORE broadcasting (saves gas/lamports
//! and the operator's reputation).
//!
//! ## Status
//!
//! Reference design. Not yet wired into the live fill loop in
//! `crates/executor/`. Callers construct an impl, call `wrap_for_session`
//! on each fill, then sign+broadcast through this crate instead of the
//! existing raw-EOA path. The wiring is a separate sprint — this crate
//! exists today to ground the "Spinner uses scoped session keys, not raw
//! operator keys" claim with code a reviewer can read and verify.
//!
//! ## Upstream references
//!
//! - Safe (Gnosis Safe smart account):
//!   <https://github.com/safe-global/safe-smart-account>
//! - Zodiac Roles Modifier:
//!   <https://github.com/gnosisguild/zodiac-modifier-roles>
//! - Squads V4 program + spending limits:
//!   <https://github.com/Squads-Protocol/v4>
//!   <https://docs.squads.so/main/v4/spending-limits>

#[cfg(feature = "evm")]
pub mod evm;

#[cfg(feature = "solana")]
pub mod solana;

pub mod config;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// Re-exports so downstream `taifoon-solver` crates can just
// `use spinner_signer::{SessionSigner, PolicyCheckResult, SessionConfig};`.
pub use config::{EvmSessionConfig, SessionConfig, SolanaSessionConfig};

#[cfg(feature = "evm")]
pub use evm::EvmSafeRolesSigner;

#[cfg(feature = "solana")]
pub use solana::SolanaSquadsSigner;

/// Outcome of a client-side policy dry-run.
///
/// A `Denied` result means the scope contract *would* revert when the
/// wrapped transaction is broadcast — we caught the policy violation
/// before paying gas/lamports.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum PolicyCheckResult {
    /// The scope contract permits this call.
    Allowed,
    /// The scope contract would reject this call. `reason` is the raw
    /// revert message (EVM) or `InstructionError` string (Solana). It's
    /// informational only — do NOT trust it for control flow beyond
    /// "log + abort".
    Denied { reason: String },
}

impl PolicyCheckResult {
    pub fn is_allowed(&self) -> bool {
        matches!(self, PolicyCheckResult::Allowed)
    }
}

/// Generic interface every chain's session-key signer must implement.
///
/// The four-stage lifecycle is:
///
/// 1. [`wrap_for_session`](SessionSigner::wrap_for_session) — take the
///    raw protocol fill calldata the executor wants to send, and wrap it
///    inside the scope contract's policy call (Zodiac Roles
///    `execTransactionWithRole` for EVM, Squads `spending_limit_use`
///    instruction for Solana).
/// 2. [`dry_run_policy`](SessionSigner::dry_run_policy) — client-side
///    simulation BEFORE broadcasting. Returns
///    [`PolicyCheckResult::Denied`] if the scope contract would reject
///    (cap exceeded, target not allowlisted, role expired, ...).
/// 3. [`sign`](SessionSigner::sign) — sign with the hot session key (the
///    EOA / ed25519 keypair loaded from the operator's secrets manager).
/// 4. [`broadcast`](SessionSigner::broadcast) — submit to the RPC.
///
/// Callers are expected to short-circuit between (2) and (3) when the
/// dry-run returns `Denied`.
#[async_trait]
pub trait SessionSigner: Send + Sync {
    /// Chain-specific transaction representation.
    ///
    /// EVM: [`alloy::rpc::types::TransactionRequest`].
    /// Solana: an opaque [`solana::SerializedTransaction`] (we use the
    /// crate's own hand-rolled tx envelope to avoid pulling solana-sdk,
    /// matching the convention in `crates/protocol-adapters-solana/`).
    type Transaction: Send + Sync;

    /// Signed envelope ready to broadcast.
    type SignedTransaction: Send + Sync;

    /// Type returned by the RPC after a successful broadcast.
    /// EVM: [`alloy::primitives::B256`]. Solana: base58 signature string.
    type TxHash: Send + Sync + std::fmt::Debug;

    /// Implementation error type. Both impls use [`anyhow::Error`] today.
    type Error: Send + Sync + 'static;

    /// Wrap a raw target call inside the scope contract's policy call.
    ///
    /// This DOES NOT broadcast. The returned transaction is the one that
    /// must be sent (i.e. its `to` field is the scope contract, not the
    /// original target).
    async fn wrap_for_session(
        &self,
        target_call: Self::Transaction,
    ) -> Result<Self::Transaction, Self::Error>;

    /// Run the scope contract's check client-side. The implementation
    /// MUST NOT broadcast.
    async fn dry_run_policy(
        &self,
        target_call: &Self::Transaction,
    ) -> Result<PolicyCheckResult, Self::Error>;

    /// Sign with the hot session key.
    async fn sign(
        &self,
        tx: Self::Transaction,
    ) -> Result<Self::SignedTransaction, Self::Error>;

    /// Broadcast and return the chain-specific tx hash / signature.
    async fn broadcast(
        &self,
        signed: Self::SignedTransaction,
    ) -> Result<Self::TxHash, Self::Error>;
}

// ── Errors ─────────────────────────────────────────────────────────────────────

/// Crate-wide error type. Implementations are free to define their own
/// error types via the [`SessionSigner::Error`] associated type — this
/// is exported for callers who want a single error vocabulary across both
/// chains.
#[derive(Debug, thiserror::Error)]
pub enum SignerError {
    #[error("target {target} is not in allowed_targets; refusing to wrap")]
    DisallowedTarget { target: String },

    #[error("scope contract denied: {reason}")]
    PolicyDenied { reason: String },

    #[error("hot session key env var {env} is unset")]
    MissingSessionKey { env: String },

    #[error("hot session key in env var {env} is malformed: {reason}")]
    MalformedSessionKey { env: String, reason: String },

    #[error("rpc error: {0}")]
    Rpc(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[cfg(test)]
mod policy_result_tests {
    use super::*;

    #[test]
    fn policy_result_is_allowed_helper() {
        assert!(PolicyCheckResult::Allowed.is_allowed());
        assert!(!PolicyCheckResult::Denied {
            reason: "x".into()
        }
        .is_allowed());
    }

    #[test]
    fn policy_result_round_trips_through_json() {
        let cases = [
            PolicyCheckResult::Allowed,
            PolicyCheckResult::Denied {
                reason: "SpendingLimitExceeded".into(),
            },
        ];
        for c in cases {
            let j = serde_json::to_string(&c).unwrap();
            let back: PolicyCheckResult = serde_json::from_str(&j).unwrap();
            assert_eq!(c, back);
        }
    }
}
