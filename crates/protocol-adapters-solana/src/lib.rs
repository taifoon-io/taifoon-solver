//! Mayan Swift (Solana) `simulateTransaction` adapter — the SVM analogue of
//! the EVM `eth_estimateGas` harness.
//!
//! On Solana, the validation primitive is `simulateTransaction` (not
//! `estimateGas` — different runtime). With `sigVerify=false` and
//! `replaceRecentBlockhash=true`, the validator decodes the message, runs the
//! BPF program against the supplied accounts, and reports back compute units
//! consumed or a program-level error. Wallet-side balance is checked as part
//! of the simulation: a payer with insufficient lamports surfaces as a
//! recognizable error string, which we treat as GREEN (the calldata + program
//! ABI matched far enough to reach the funding check).
//!
//! Why no `solana-sdk`/`solana-client`? Those crates drag in ~1000 transitive
//! deps and would push the workspace build past 5 minutes from cold. We only
//! need: bs58 for Pubkey strings, sha256 for the Anchor 8-byte discriminator,
//! base64 for the JSON-RPC payload, and serde_json for the response. The
//! transaction wire format is the documented Solana legacy layout (see
//! `serialize_message`/`serialize_legacy_transaction` below) — small enough
//! to write by hand and verify with the public protocol spec.

#![allow(clippy::needless_range_loop)]

pub mod keychain;
pub mod mayan_solana;
pub mod send;
pub mod simulate;

pub use keychain::load_solana_signer;
pub use mayan_solana::{
    DEFAULT_MAYAN_SWIFT_PROGRAM, MayanSolanaIntent, MayanSolanaSimulator, derive_mayan_vault_pda,
};
pub use send::{SolanaBroadcaster, SolanaSendResult, SOLANA_PRIVATE_KEY_ENV};
pub use simulate::{
    SolanaEstimateOutcome, SolanaSimulator, classify_solana_simulate_result,
};

#[cfg(test)]
mod solana_tests;
