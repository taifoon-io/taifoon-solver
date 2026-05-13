//! Squads V4 program IDs + Anchor instruction discriminators.
//!
//! Source of truth:
//!     <https://github.com/Squads-Protocol/v4>
//!     <https://docs.squads.so/main/v4>
//!
//! Squads V4 is an Anchor program; each instruction is identified by an
//! 8-byte discriminator equal to the first 8 bytes of
//! `sha256("global:<instruction_name>")`. We compute the discriminators
//! at runtime via a `OnceLock` so they don't drift if the upstream
//! renames an instruction. Constants below cache the *names*.
//!
//! ## Program addresses
//!
//! The mainnet program ID for Squads Multisig V4 is fixed and
//! verifiable against the upstream repo's `Anchor.toml`:
//!   `SQDS4ep65T869zMMBKyuUq6aD6EgTu8psMjkvj52pCf`
//!
//! ## Instruction discriminators we care about
//!
//! * `spending_limit_use` — invokes a previously-approved spending
//!   limit. The session key is registered as a `member` of the
//!   spending-limit account. The instruction transfers up to the cap
//!   amount to a destination the spending limit pre-approved.
//! * `multisig_create` / `config_transaction_create` are NOT used here;
//!   those are master-treasury operations done once at setup time by
//!   the operator (see `scripts/session-setup/solana/`).

use sha2::{Digest, Sha256};
use std::sync::OnceLock;

/// Squads V4 mainnet program ID (base58).
///
/// Verify against the upstream repo:
///   <https://github.com/Squads-Protocol/v4/blob/main/Anchor.toml>
pub const SQUADS_V4_PROGRAM_ID_B58: &str = "SQDS4ep65T869zMMBKyuUq6aD6EgTu8psMjkvj52pCf";

/// System program ID — used as a fallback "allowed" target in tests, and
/// in real configs to permit lamport-only transfers.
pub const SYSTEM_PROGRAM_ID_B58: &str = "11111111111111111111111111111111";

/// Anchor instruction name for invoking a spending limit. The 8-byte
/// discriminator is `sha256("global:spending_limit_use")[..8]`.
pub const IX_SPENDING_LIMIT_USE: &str = "spending_limit_use";

/// Compute the 8-byte Anchor discriminator for an instruction name.
///
/// This is the standard Anchor convention; we recompute rather than
/// hard-coding so a typo in the constant can't drift from the upstream
/// IDL silently.
pub fn anchor_discriminator(instruction_name: &str) -> [u8; 8] {
    let mut h = Sha256::new();
    h.update(b"global:");
    h.update(instruction_name.as_bytes());
    let digest = h.finalize();
    let mut out = [0u8; 8];
    out.copy_from_slice(&digest[..8]);
    out
}

/// Cached discriminator for the `spending_limit_use` instruction.
pub fn spending_limit_use_discriminator() -> [u8; 8] {
    static CACHE: OnceLock<[u8; 8]> = OnceLock::new();
    *CACHE.get_or_init(|| anchor_discriminator(IX_SPENDING_LIMIT_USE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discriminator_is_deterministic_8_bytes() {
        let d1 = spending_limit_use_discriminator();
        let d2 = spending_limit_use_discriminator();
        assert_eq!(d1, d2);
        assert_eq!(d1.len(), 8);
    }

    #[test]
    fn discriminators_for_different_names_differ() {
        assert_ne!(
            anchor_discriminator("spending_limit_use"),
            anchor_discriminator("multisig_create")
        );
    }

    #[test]
    fn program_id_decodes_as_base58() {
        let bytes = bs58::decode(SQUADS_V4_PROGRAM_ID_B58)
            .into_vec()
            .expect("Squads V4 program ID must be valid base58");
        assert_eq!(bytes.len(), 32, "Solana program IDs are 32 bytes");
    }

    #[test]
    fn system_program_id_decodes_to_all_zero() {
        let bytes = bs58::decode(SYSTEM_PROGRAM_ID_B58).into_vec().unwrap();
        assert_eq!(bytes, vec![0u8; 32]);
    }
}
