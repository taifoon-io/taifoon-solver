//! Declarative `SessionConfig` — read at boot from
//! `config/session_config.json`. Same shape across both chains so
//! `taifoon-solver` doesn't fork its config-loader per chain.
//!
//! ## Wire format
//!
//! The JSON discriminates on the `chain` tag:
//!
//! ```json
//! {
//!   "chain": "evm",
//!   "safe_address": "0x…",
//!   "roles_module": "0x…",
//!   "role_key": "0x…32 bytes…",
//!   "allowed_targets": ["0xAcrossSpokePool…"],
//!   "session_key_env": "SPINNER_EVM_SESSION_KEY",
//!   "rpc_url": "https://…",
//!   "chain_id": 8453
//! }
//! ```
//!
//! or
//!
//! ```json
//! {
//!   "chain": "solana",
//!   "squads_multisig": "…base58…",
//!   "spending_limit": "…base58…",
//!   "allowed_programs": ["JUP6LkbZ…", "swiftDp…"],
//!   "session_key_env": "SPINNER_SOLANA_SESSION_KEY",
//!   "rpc_url": "https://api.mainnet-beta.solana.com"
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "chain", rename_all = "lowercase")]
pub enum SessionConfig {
    Evm(EvmSessionConfig),
    Solana(SolanaSessionConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvmSessionConfig {
    /// Safe address that holds the operator's treasury.
    pub safe_address: String,

    /// Zodiac Roles Module address installed on the Safe.
    pub roles_module: String,

    /// 32-byte role key the session key is a member of. Stored as
    /// `0x`-prefixed lowercase hex (64 hex chars + `0x`).
    pub role_key: String,

    /// Allowed target contracts. The signer MUST refuse to wrap calls to
    /// any address not in this list — this is a client-side mirror of
    /// what the role permits on-chain, and exists to fail fast before
    /// burning gas on a dry-run that will deny.
    pub allowed_targets: Vec<String>,

    /// Hot session key — 0x-prefixed 32-byte secp256k1 hex. Loaded from
    /// the operator's secrets manager via this env var name. Never
    /// committed.
    pub session_key_env: String,

    /// RPC endpoint for the destination chain.
    pub rpc_url: String,

    /// EIP-155 chain ID.
    pub chain_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SolanaSessionConfig {
    /// Squads V4 multisig PDA address (base58).
    pub squads_multisig: String,

    /// Spending-limit account address (base58). One spending-limit
    /// account encodes one cap/period config. An operator typically has
    /// several spending-limit accounts (e.g. one per token mint), and
    /// picks the right one for the call being wrapped.
    pub spending_limit: String,

    /// Programs the session key is permitted to invoke through the
    /// spending limit. Must match what the multisig members approved
    /// when creating the spending-limit account. Base58 program IDs.
    pub allowed_programs: Vec<String>,

    /// Hot session key env var name — base58 64-byte keypair string
    /// (the format `solana-keygen` produces).
    pub session_key_env: String,

    /// Solana RPC endpoint.
    pub rpc_url: String,
}

impl SessionConfig {
    /// Read + parse a `SessionConfig` from a path on disk.
    pub fn load_from_path(path: &Path) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        let parsed = serde_json::from_str(&raw)?;
        Ok(parsed)
    }

    /// Read + parse from a JSON string. Useful in tests so we don't
    /// touch the filesystem.
    pub fn from_json_str(raw: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(raw)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evm_config_round_trips() {
        let cfg = SessionConfig::Evm(EvmSessionConfig {
            safe_address: "0x1111111111111111111111111111111111111111".into(),
            roles_module: "0x2222222222222222222222222222222222222222".into(),
            role_key: "0x".to_string() + &"ab".repeat(32),
            allowed_targets: vec!["0x09aea4b2242abC8bb4BB78D537A67a245A7bEC64".into()],
            session_key_env: "SPINNER_EVM_SESSION_KEY".into(),
            rpc_url: "https://example.invalid".into(),
            chain_id: 8453,
        });
        let s = serde_json::to_string(&cfg).unwrap();
        let back = SessionConfig::from_json_str(&s).unwrap();
        assert_eq!(cfg, back);
        assert!(s.contains("\"chain\":\"evm\""));
    }

    #[test]
    fn solana_config_round_trips() {
        let cfg = SessionConfig::Solana(SolanaSessionConfig {
            squads_multisig: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into(),
            spending_limit: "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB".into(),
            allowed_programs: vec![
                "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4".into(),
            ],
            session_key_env: "SPINNER_SOLANA_SESSION_KEY".into(),
            rpc_url: "https://api.mainnet-beta.solana.com".into(),
        });
        let s = serde_json::to_string(&cfg).unwrap();
        let back = SessionConfig::from_json_str(&s).unwrap();
        assert_eq!(cfg, back);
        assert!(s.contains("\"chain\":\"solana\""));
    }
}
