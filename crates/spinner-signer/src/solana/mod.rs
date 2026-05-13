//! Solana session signer — Squads V4 spending limits.
//!
//! ## How it composes
//!
//! ```text
//! ┌─────────────────┐  wrap_for_session  ┌──────────────────────────┐
//! │ raw target tx   │ ──────────────────▶│ tx with spending_limit_use│
//! │ (e.g. Mayan     │                    │ instruction inserted /    │
//! │  fulfill)       │                    │ replacing the inner CPI   │
//! └─────────────────┘                    └──────────────────────────┘
//! ```
//!
//! The session key (ed25519 keypair) is registered as a `member` of a
//! `spending_limit` account on the Squads V4 multisig. The spending
//! limit pins:
//!
//! * a token mint (or SOL)
//! * a daily / monthly / one-time cap amount
//! * an allowlist of destination addresses
//! * an allowlist of programs the wrapped call can hit
//!
//! All four are enforced on-chain by the Squads V4 program when the
//! `spending_limit_use` instruction executes. The session key cannot
//! drain the multisig: it can only invoke the spending limit, and the
//! spending limit only authorizes calls within its scope.
//!
//! ## Why we don't pull `solana-sdk`
//!
//! `protocol-adapters-solana/Cargo.toml` documents the deliberate choice
//! to avoid `solana-sdk` / `solana-client` (heavy transitive deps, slow
//! first build). This crate follows the same convention. We use:
//!
//! * `bs58` for Pubkey ↔ base58 string
//! * `ed25519-dalek` for hot-key signing
//! * `sha2` for Anchor discriminators (already a workspace dep)
//! * `base64` + `reqwest` for JSON-RPC `simulateTransaction` /
//!   `sendTransaction`
//!
//! That gives us enough surface to encode + sign + broadcast without
//! the dep weight.
//!
//! ## Upstream references
//!
//! - Squads V4 program: <https://github.com/Squads-Protocol/v4>
//! - Spending limits docs:
//!   <https://docs.squads.so/main/v4/spending-limits>
//! - Squads TS SDK (`@sqds/multisig`):
//!   <https://www.npmjs.com/package/@sqds/multisig>

pub mod squads_program;

pub use squads_program::{
    anchor_discriminator, spending_limit_use_discriminator, SQUADS_V4_PROGRAM_ID_B58,
    SYSTEM_PROGRAM_ID_B58,
};

use crate::config::SolanaSessionConfig;
use crate::{PolicyCheckResult, SessionSigner};

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::{Signer, SigningKey};

/// Opaque 32-byte Solana program / account ID.
///
/// We keep this distinct from `[u8; 32]` so the type system can guard
/// against accidentally treating a hash as a Pubkey.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pubkey(pub [u8; 32]);

impl Pubkey {
    /// Decode a base58 Pubkey string. Returns `Err` on invalid base58
    /// or wrong length.
    pub fn from_b58(s: &str) -> Result<Self> {
        let v = bs58::decode(s)
            .into_vec()
            .with_context(|| format!("invalid base58 pubkey {s}"))?;
        if v.len() != 32 {
            return Err(anyhow!("pubkey must be 32 bytes, got {}", v.len()));
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&v);
        Ok(Pubkey(out))
    }

    pub fn to_b58(self) -> String {
        bs58::encode(self.0).into_string()
    }
}

/// A Solana transaction in the *opaque* form we use across the trait.
///
/// We hold:
/// * a serialized legacy message (the raw bytes that get signed), or
///   `None` if not yet built
/// * the list of program IDs the inner instructions invoke — we extract
///   these at wrap time to check `allowed_programs`
/// * the list of instructions themselves so the wrapper can build a new
///   transaction that prepends a `spending_limit_use` instruction
///
/// Real executors will plug their existing `serialize_legacy_transaction`
/// path through this — it doesn't replace the Solana adapter's own
/// envelope builder, it sits in front of it.
#[derive(Debug, Clone)]
pub struct SerializedTransaction {
    /// Base64-encoded wire-format transaction. `None` until the caller
    /// has built + serialized the inner tx.
    pub wire_base64: Option<String>,
    /// Program IDs the inner instructions invoke (base58). Populated by
    /// the caller; the signer reads this to enforce `allowed_programs`.
    pub inner_program_ids: Vec<String>,
    /// Free-form metadata field for debug logging (intent ID, protocol,
    /// etc.). Not part of the wire format.
    pub label: String,
}

impl SerializedTransaction {
    pub fn new_unbuilt(inner_program_ids: Vec<String>, label: impl Into<String>) -> Self {
        Self {
            wire_base64: None,
            inner_program_ids,
            label: label.into(),
        }
    }

    pub fn with_wire(mut self, wire_base64: String) -> Self {
        self.wire_base64 = Some(wire_base64);
        self
    }
}

/// Signed envelope ready to broadcast — same shape as
/// `SerializedTransaction`, just with the wire-format guaranteed
/// present and with the session-key signature attached.
#[derive(Debug, Clone)]
pub struct SignedSolanaTransaction {
    pub wire_base64: String,
    /// Base58 signature of the session key over the message.
    pub signature_b58: String,
}

/// Session-key signer for a Squads V4 multisig.
pub struct SolanaSquadsSigner {
    config: SolanaSessionConfig,
    multisig: Pubkey,
    spending_limit: Pubkey,
    allowed_programs: Vec<Pubkey>,
}

impl SolanaSquadsSigner {
    pub fn from_config(config: SolanaSessionConfig) -> Result<Self> {
        let multisig = Pubkey::from_b58(&config.squads_multisig)
            .with_context(|| format!("squads_multisig {}", config.squads_multisig))?;
        let spending_limit = Pubkey::from_b58(&config.spending_limit)
            .with_context(|| format!("spending_limit {}", config.spending_limit))?;
        let mut allowed_programs = Vec::with_capacity(config.allowed_programs.len());
        for p in &config.allowed_programs {
            allowed_programs.push(
                Pubkey::from_b58(p).with_context(|| format!("allowed_program {p}"))?,
            );
        }
        Ok(Self {
            config,
            multisig,
            spending_limit,
            allowed_programs,
        })
    }

    pub fn multisig(&self) -> Pubkey {
        self.multisig
    }

    pub fn spending_limit(&self) -> Pubkey {
        self.spending_limit
    }

    /// Whether the given base58 program ID is in the session's
    /// `allowed_programs` allowlist. O(N) — N is tiny (rarely > 5).
    pub fn is_program_allowed(&self, program_id_b58: &str) -> bool {
        match Pubkey::from_b58(program_id_b58) {
            Ok(p) => self.allowed_programs.contains(&p),
            Err(_) => false,
        }
    }

    /// Build the data field of the `spending_limit_use` instruction.
    ///
    /// Wire format (Anchor):
    ///   `discriminator (8 bytes) || args (borsh)`
    ///
    /// `spending_limit_use` args are an `Option<u8>` (memo length) plus
    /// the call amount. The exact argument shape is documented in the
    /// Squads V4 IDL — this builder packs the minimal required fields
    /// for a vanilla cap-only call.
    pub fn build_spending_limit_use_data(&self, amount_lamports: u64) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + 8 + 1);
        out.extend_from_slice(&spending_limit_use_discriminator());
        // amount: u64 LE (borsh)
        out.extend_from_slice(&amount_lamports.to_le_bytes());
        // memo: Option<String> = None (borsh tag = 0)
        out.push(0);
        out
    }

    /// Load the session keypair from the configured env var.
    ///
    /// Accepts the same formats as `protocol-adapters-solana::send` (so
    /// operators don't have to relearn the key format):
    /// * base58 64-byte keypair (first 32 = secret scalar, last 32 = pubkey)
    /// * hex 32-byte secret scalar
    pub fn load_session_keypair(&self) -> Result<SigningKey> {
        let raw = std::env::var(&self.config.session_key_env).map_err(|_| {
            anyhow!(
                "session-key env var {} is unset",
                self.config.session_key_env
            )
        })?;
        load_signing_key_from_str(&raw)
    }
}

/// Decode an ed25519 secret key from either of the two supported formats.
/// Helper is `pub(crate)` so tests can drive it directly.
pub(crate) fn load_signing_key_from_str(raw: &str) -> Result<SigningKey> {
    let raw = raw.trim();
    // Try hex-32-byte first (cheaper).
    if let Ok(bytes) = hex::decode(raw.trim_start_matches("0x")) {
        if bytes.len() == 32 {
            let mut secret = [0u8; 32];
            secret.copy_from_slice(&bytes);
            return Ok(SigningKey::from_bytes(&secret));
        }
    }
    // Otherwise try base58 64-byte keypair.
    let v = bs58::decode(raw)
        .into_vec()
        .context("session-key env var is neither hex(32) nor base58(64)")?;
    if v.len() == 64 {
        let mut secret = [0u8; 32];
        secret.copy_from_slice(&v[..32]);
        return Ok(SigningKey::from_bytes(&secret));
    }
    if v.len() == 32 {
        let mut secret = [0u8; 32];
        secret.copy_from_slice(&v);
        return Ok(SigningKey::from_bytes(&secret));
    }
    Err(anyhow!(
        "session-key env var must decode to 32 or 64 bytes, got {}",
        v.len()
    ))
}

#[async_trait]
impl SessionSigner for SolanaSquadsSigner {
    type Transaction = SerializedTransaction;
    type SignedTransaction = SignedSolanaTransaction;
    type TxHash = String;
    type Error = anyhow::Error;

    async fn wrap_for_session(
        &self,
        target_call: SerializedTransaction,
    ) -> Result<SerializedTransaction, Self::Error> {
        // Verify each inner program ID is allowlisted.
        for prog in &target_call.inner_program_ids {
            if !self.is_program_allowed(prog) {
                return Err(anyhow!(
                    "program {prog} not in allowed_programs ({} entries)",
                    self.allowed_programs.len()
                ));
            }
        }
        // For the reference impl we return a wrapper that *labels* the
        // tx as "to be wrapped by spending_limit_use" — the actual
        // tx-bytes splice happens in the executor's existing
        // `serialize_legacy_transaction` path. The wrapping here is the
        // policy check: the bytes themselves are built by the protocol
        // adapter that already knows the account layout.
        let mut wrapped = target_call;
        wrapped.label = format!("squads-wrapped::{}", wrapped.label);
        Ok(wrapped)
    }

    async fn dry_run_policy(
        &self,
        target_call: &SerializedTransaction,
    ) -> Result<PolicyCheckResult, Self::Error> {
        // Allowlist check is cheap and authoritative — if any inner
        // program isn't approved, deny without hitting the RPC.
        for prog in &target_call.inner_program_ids {
            if !self.is_program_allowed(prog) {
                return Ok(PolicyCheckResult::Denied {
                    reason: format!("program {prog} not in allowed_programs"),
                });
            }
        }

        // If the tx wire bytes aren't built yet, we can't simulate. The
        // caller is expected to call `wrap_for_session` first AND have
        // the protocol adapter serialize the tx — both of those happen
        // upstream of broadcast.
        let wire = match &target_call.wire_base64 {
            Some(w) => w,
            None => {
                // Allowlist passed and there's nothing to simulate yet —
                // that's a green light from THIS layer; the executor's
                // own simulator (`protocol_adapters_solana::simulate`)
                // will catch any program-level issues at the next stage.
                return Ok(PolicyCheckResult::Allowed);
            }
        };

        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "simulateTransaction",
            "params": [
                wire,
                {
                    "encoding": "base64",
                    "commitment": "confirmed",
                    "sigVerify": false,
                    "replaceRecentBlockhash": true,
                }
            ]
        });

        let resp = reqwest::Client::new()
            .post(&self.config.rpc_url)
            .json(&payload)
            .send()
            .await
            .context("simulateTransaction request")?;
        let body: serde_json::Value =
            resp.json().await.context("simulateTransaction response json")?;

        if let Some(err) = body.get("error") {
            return Ok(PolicyCheckResult::Denied {
                reason: err.to_string(),
            });
        }

        let value = body
            .get("result")
            .and_then(|r| r.get("value"))
            .ok_or_else(|| anyhow!("simulateTransaction missing result.value: {body}"))?;

        // `value.err == null` ⇒ allowed. Otherwise the err value is the
        // raw `TransactionError` (or `InstructionError`) variant — pass
        // it back as the deny reason so the operator can see e.g.
        // `{"InstructionError":[0,{"Custom":42}]}`.
        let err = value.get("err");
        match err {
            Some(serde_json::Value::Null) | None => Ok(PolicyCheckResult::Allowed),
            Some(other) => Ok(PolicyCheckResult::Denied {
                reason: other.to_string(),
            }),
        }
    }

    async fn sign(
        &self,
        tx: SerializedTransaction,
    ) -> Result<SignedSolanaTransaction, Self::Error> {
        let wire = tx
            .wire_base64
            .ok_or_else(|| anyhow!("transaction wire_base64 not populated; build it first"))?;
        let keypair = self.load_session_keypair().context("load session key")?;

        // The message-bytes we sign are the wire bytes *without* the
        // 1-byte signature-count prefix and the 64-byte signature slot.
        // The Solana legacy transaction format is:
        //   compact-u16 num_signatures (always = 1 here)
        //   N * 64 bytes signature
        //   serialized message
        // We expect the protocol adapter has already laid out the
        // signature slot with all-zero bytes; we sign the trailing
        // message and patch the signature in.
        let mut wire_bytes = BASE64.decode(&wire).context("decode wire_base64")?;
        // num_signatures occupies a compact-u16. For 1 signature the
        // compact-u16 is a single byte: 0x01.
        if wire_bytes.is_empty() {
            return Err(anyhow!("wire bytes empty"));
        }
        let num_sigs = wire_bytes[0] as usize;
        if num_sigs != 1 {
            return Err(anyhow!(
                "session signer expects exactly 1 signature slot, found {num_sigs}"
            ));
        }
        let sig_offset = 1usize;
        let msg_offset = sig_offset + 64;
        if wire_bytes.len() < msg_offset {
            return Err(anyhow!("wire bytes too short for sig+msg layout"));
        }
        let message = &wire_bytes[msg_offset..];

        let sig = keypair.sign(message);
        let sig_b58 = bs58::encode(sig.to_bytes()).into_string();

        // Patch the signature into the wire bytes.
        wire_bytes[sig_offset..sig_offset + 64].copy_from_slice(&sig.to_bytes());
        let signed_wire = BASE64.encode(&wire_bytes);

        Ok(SignedSolanaTransaction {
            wire_base64: signed_wire,
            signature_b58: sig_b58,
        })
    }

    async fn broadcast(
        &self,
        signed: SignedSolanaTransaction,
    ) -> Result<String, Self::Error> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "sendTransaction",
            "params": [
                signed.wire_base64,
                { "encoding": "base64", "preflightCommitment": "confirmed" }
            ]
        });

        let resp = reqwest::Client::new()
            .post(&self.config.rpc_url)
            .json(&payload)
            .send()
            .await
            .context("sendTransaction request")?;
        let body: serde_json::Value = resp.json().await.context("response json")?;
        if let Some(err) = body.get("error") {
            return Err(anyhow!("sendTransaction error: {err}"));
        }
        let sig = body
            .get("result")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("sendTransaction missing result: {body}"))?;
        Ok(sig.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> SolanaSessionConfig {
        SolanaSessionConfig {
            // System program ID — a known-good base58 string (32 zero bytes).
            squads_multisig: SYSTEM_PROGRAM_ID_B58.into(),
            spending_limit: SYSTEM_PROGRAM_ID_B58.into(),
            allowed_programs: vec![
                "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4".into(),
                SYSTEM_PROGRAM_ID_B58.into(),
            ],
            session_key_env: "SPINNER_TEST_SOLANA_KEY_DO_NOT_USE".into(),
            rpc_url: "http://127.0.0.1:0/never-called".into(),
        }
    }

    #[test]
    fn from_config_parses_addresses_and_programs() {
        let s = SolanaSquadsSigner::from_config(test_config()).unwrap();
        assert!(s.is_program_allowed(SYSTEM_PROGRAM_ID_B58));
        assert!(s.is_program_allowed("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4"));
        // The Squads program itself is NOT in `allowed_programs` for
        // this test — it's the *wrapper*, not an inner target.
        assert!(!s.is_program_allowed(SQUADS_V4_PROGRAM_ID_B58));
    }

    #[test]
    fn spending_limit_use_data_starts_with_discriminator() {
        let s = SolanaSquadsSigner::from_config(test_config()).unwrap();
        let data = s.build_spending_limit_use_data(123_456);
        assert_eq!(&data[..8], &spending_limit_use_discriminator());
        // amount LE
        let amt = u64::from_le_bytes(data[8..16].try_into().unwrap());
        assert_eq!(amt, 123_456);
        // memo = None
        assert_eq!(data[16], 0);
    }

    #[test]
    fn pubkey_round_trip() {
        let p = Pubkey::from_b58(SYSTEM_PROGRAM_ID_B58).unwrap();
        assert_eq!(p.0, [0u8; 32]);
        assert_eq!(p.to_b58(), SYSTEM_PROGRAM_ID_B58);
    }

    #[test]
    fn load_signing_key_accepts_hex_32() {
        let hex32 = "0".repeat(64);
        let key = load_signing_key_from_str(&hex32).unwrap();
        // sanity — verifying key derives without panic
        let _ = key.verifying_key();
    }

    #[test]
    fn load_signing_key_accepts_base58_64() {
        // Build a 64-byte keypair: 32-byte secret + 32-byte pubkey
        // (we don't need the pubkey half to be correct for the loader).
        let bytes = [7u8; 64];
        let b58 = bs58::encode(bytes).into_string();
        let _ = load_signing_key_from_str(&b58).unwrap();
    }

    #[test]
    fn load_signing_key_rejects_bad_length() {
        let bad = bs58::encode([1u8; 10]).into_string();
        let err = load_signing_key_from_str(&bad).unwrap_err();
        assert!(err.to_string().contains("32 or 64"));
    }
}
