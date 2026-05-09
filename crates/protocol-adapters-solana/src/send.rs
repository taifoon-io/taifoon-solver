//! Solana `sendTransaction` broadcaster for Mayan Swift `fulfill`.
//!
//! Responsibilities:
//!   1. Load a Solana keypair from an env var (`SOLANA_PRIVATE_KEY`) — accepts
//!      either a base58 secret key (64-byte) or a raw hex secret scalar (32-byte).
//!   2. Fetch the latest blockhash from the configured RPC endpoint.
//!   3. Rebuild the legacy transaction from `MayanSolanaIntent`, patch the
//!      blockhash, sign with ed25519-dalek, and broadcast via `sendTransaction`.
//!   4. Return the base58 transaction signature on success.
//!
//! We deliberately avoid `solana-sdk`/`solana-client` — the entire signing and
//! wire-format layer is implemented with ed25519-dalek + the hand-rolled
//! `serialize_legacy_transaction` already in `mayan_solana.rs`.
//!
//! `SOLANA_PRIVATE_KEY` env var format accepted:
//!   - Base58 encoded 64-byte keypair (the format `solana-keygen` produces):
//!     first 32 bytes are the secret scalar, last 32 are the public key.
//!   - Hex encoded 32-byte secret scalar (the raw `ed25519` secret scalar).

use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tracing::info;

use crate::mayan_solana::{
    MayanSolanaIntent, COMPUTE_BUDGET_PROGRAM_ID, DEFAULT_PRIORITY_FEE_MICROLAMPORTS_PER_CU,
    SYSTEM_PROGRAM_ID,
};

/// Env var name for the Solana private key.
pub const SOLANA_PRIVATE_KEY_ENV: &str = "SOLANA_PRIVATE_KEY";

/// Result of a broadcast attempt.
#[derive(Debug, Clone)]
pub struct SolanaSendResult {
    /// Base58 transaction signature (the Solana tx id).
    pub signature: String,
}

/// Broadcaster that loads the signing key once and reuses it across calls.
pub struct SolanaBroadcaster {
    signing_key: SigningKey,
    rpc_url: String,
    client: reqwest::Client,
}

impl SolanaBroadcaster {
    /// Load private key from `SOLANA_PRIVATE_KEY` env var.
    /// Returns `Err` if the env var is unset or the key can't be decoded.
    pub fn from_env(rpc_url: impl Into<String>) -> Result<Self> {
        let raw = std::env::var(SOLANA_PRIVATE_KEY_ENV)
            .map_err(|_| anyhow!("{} not set", SOLANA_PRIVATE_KEY_ENV))?;
        let signing_key = load_signing_key(&raw)?;
        Ok(Self::new(signing_key, rpc_url))
    }

    pub fn new(signing_key: SigningKey, rpc_url: impl Into<String>) -> Self {
        Self {
            signing_key,
            rpc_url: rpc_url.into(),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .expect("build reqwest client"),
        }
    }

    /// Derive the base58-encoded public key of the loaded signer.
    pub fn pubkey_b58(&self) -> String {
        bs58::encode(self.signing_key.verifying_key().to_bytes()).into_string()
    }

    /// Broadcast a Mayan Swift `fulfill` transaction.
    /// Fetches a fresh blockhash, signs, and sends.
    pub async fn send_fulfill(&self, intent: &MayanSolanaIntent) -> Result<SolanaSendResult> {
        let blockhash = self.get_latest_blockhash().await
            .context("getLatestBlockhash RPC")?;

        info!("🔑 Signing Solana fulfill tx (payer={}, blockhash={}…)",
            self.pubkey_b58(), &bs58::encode(&blockhash).into_string()[..8]);

        let tx_bytes = self.build_signed_tx(intent, &blockhash)
            .context("build signed tx")?;

        let tx_b64 = BASE64.encode(&tx_bytes);
        let sig = self.send_raw_transaction(&tx_b64).await
            .map_err(|e| {
                tracing::error!("sendTransaction failed: {:#}", e);
                e
            })
            .context("sendTransaction RPC")?;

        Ok(SolanaSendResult { signature: sig })
    }

    /// Build the signed legacy transaction bytes.
    pub(crate) fn build_signed_tx(&self, intent: &MayanSolanaIntent, blockhash: &[u8; 32]) -> Result<Vec<u8>> {
        let payer = self.signing_key.verifying_key().to_bytes();
        let program = decode_b58_32(&intent.swift_program_id_b58)
            .context("decode swift program id")?;
        let state = decode_b58_32(&intent.state_account_b58)
            .context("decode state account")?;
        let vault = decode_b58_32(&intent.vault_account_b58)
            .context("decode vault account")?;
        let trader = decode_b58_32(&intent.trader_pubkey_b58)
            .context("decode trader pubkey")?;
        let system = decode_b58_32(SYSTEM_PROGRAM_ID).expect("system program");
        let cb = decode_b58_32(COMPUTE_BUDGET_PROGRAM_ID).expect("compute budget program");

        // ComputeBudget: SetComputeUnitLimit (tag 0x02 + u32 LE)
        let cu_limit: u32 = intent.compute_units_estimate.min(u32::MAX as u64) as u32;
        let mut cu_limit_data = vec![0x02u8];
        cu_limit_data.extend_from_slice(&cu_limit.to_le_bytes());

        // ComputeBudget: SetComputeUnitPrice (tag 0x03 + u64 LE micro-lamports/CU)
        let mut cu_price_data = vec![0x03u8];
        cu_price_data.extend_from_slice(&DEFAULT_PRIORITY_FEE_MICROLAMPORTS_PER_CU.to_le_bytes());

        let mut ix_data = Vec::with_capacity(8 + 32 + 8 + 8);
        ix_data.extend_from_slice(&anchor_discriminator("fulfill"));
        let order_id = decode_hex_32(&intent.mayan_order_id_hex)
            .context("decode mayan_order_id hex")?;
        ix_data.extend_from_slice(&order_id);
        ix_data.extend_from_slice(&intent.min_amount_out.to_le_bytes());
        ix_data.extend_from_slice(&intent.deadline.to_le_bytes());

        let fulfill_metas: Vec<AccountMeta> = vec![
            AccountMeta { pubkey: payer, is_signer: true, is_writable: true },
            AccountMeta { pubkey: state, is_signer: false, is_writable: true },
            AccountMeta { pubkey: vault, is_signer: false, is_writable: true },
            AccountMeta { pubkey: trader, is_signer: false, is_writable: false },
            AccountMeta { pubkey: system, is_signer: false, is_writable: false },
        ];

        let instructions: Vec<(/* program */ [u8; 32], Vec<AccountMeta>, Vec<u8>)> = vec![
            (cb, vec![], cu_limit_data),
            (cb, vec![], cu_price_data),
            (program, fulfill_metas, ix_data),
        ];

        // Build message bytes with all three instructions (ComputeBudget × 2 + fulfill).
        let msg = build_message_multi(payer, &instructions, blockhash)?;

        // Sign the message bytes.
        let sig = self.signing_key.sign(&msg);
        let sig_bytes = sig.to_bytes();

        // Prepend signature: compact-u16(1) || sig[64] || message
        let mut tx = Vec::with_capacity(1 + 64 + msg.len());
        write_compact_u16(&mut tx, 1);
        tx.extend_from_slice(&sig_bytes);
        tx.extend_from_slice(&msg);

        if tx.len() > 1232 {
            return Err(anyhow!("signed tx is {} bytes (>1232 limit)", tx.len()));
        }
        Ok(tx)
    }

    async fn get_latest_blockhash(&self) -> Result<[u8; 32]> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getLatestBlockhash",
            "params": [{"commitment": "finalized"}]
        });
        let resp = self.client.post(&self.rpc_url).json(&body).send().await
            .context("getLatestBlockhash HTTP")?;
        if !resp.status().is_success() {
            return Err(anyhow!("getLatestBlockhash HTTP {}", resp.status()));
        }
        let parsed: serde_json::Value = resp.json().await
            .context("getLatestBlockhash parse")?;
        let bh_str = parsed.pointer("/result/value/blockhash")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("getLatestBlockhash: missing result.value.blockhash"))?;
        let bh_bytes = bs58::decode(bh_str).into_vec()
            .map_err(|e| anyhow!("blockhash base58 decode: {}", e))?;
        if bh_bytes.len() != 32 {
            return Err(anyhow!("blockhash wrong length: {} bytes", bh_bytes.len()));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bh_bytes);
        Ok(arr)
    }

    async fn send_raw_transaction(&self, tx_b64: &str) -> Result<String> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "sendTransaction",
            "params": [
                tx_b64,
                {
                    "encoding": "base64",
                    "skipPreflight": false,
                    "preflightCommitment": "processed",
                    "maxRetries": 3
                }
            ]
        });
        let resp = self.client.post(&self.rpc_url).json(&body).send().await
            .context("sendTransaction HTTP")?;
        let status = resp.status();
        let parsed: serde_json::Value = resp.json().await
            .context("sendTransaction parse")?;
        if let Some(err) = parsed.get("error") {
            return Err(anyhow!("sendTransaction RPC error: {}", serde_json::to_string(err).unwrap_or_else(|_| err.to_string())));
        }
        if !status.is_success() {
            return Err(anyhow!("sendTransaction HTTP {}: {}", status, serde_json::to_string(&parsed).unwrap_or_default()));
        }
        let sig = parsed.get("result")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("sendTransaction: missing result (signature)"))?;
        info!("✅ Solana tx broadcast: {}", sig);
        Ok(sig.to_string())
    }
}

/// Load a `SigningKey` from a base58-encoded 64-byte keypair or hex 32-byte secret.
pub(crate) fn load_signing_key(raw: &str) -> Result<SigningKey> {
    let trimmed = raw.trim();
    // Try base58 decode first (64-byte Solana keypair format).
    if let Ok(bytes) = bs58::decode(trimmed).into_vec() {
        if bytes.len() == 64 {
            let secret: [u8; 32] = bytes[..32].try_into()
                .map_err(|_| anyhow!("secret scalar slice failed"))?;
            return Ok(SigningKey::from_bytes(&secret));
        }
        if bytes.len() == 32 {
            let secret: [u8; 32] = bytes.try_into()
                .map_err(|_| anyhow!("32-byte base58 secret failed"))?;
            return Ok(SigningKey::from_bytes(&secret));
        }
    }
    // Try hex decode (32-byte raw secret scalar).
    let clean = trimmed.trim_start_matches("0x");
    if let Ok(bytes) = hex::decode(clean) {
        if bytes.len() == 32 {
            let secret: [u8; 32] = bytes.try_into()
                .map_err(|_| anyhow!("hex 32-byte secret failed"))?;
            return Ok(SigningKey::from_bytes(&secret));
        }
    }
    Err(anyhow!(
        "SOLANA_PRIVATE_KEY must be a base58-encoded 64-byte keypair or hex 32-byte secret scalar; got {} chars",
        trimmed.len()
    ))
}

// ── Wire-format helpers (duplicated from mayan_solana.rs to avoid coupling) ─

#[derive(Debug, Clone, Copy)]
struct AccountMeta {
    pubkey: [u8; 32],
    is_signer: bool,
    is_writable: bool,
}

fn decode_b58_32(s: &str) -> Result<[u8; 32]> {
    let bytes = bs58::decode(s).into_vec()
        .map_err(|e| anyhow!("base58 decode '{}': {}", s, e))?;
    if bytes.len() != 32 {
        return Err(anyhow!("expected 32-byte pubkey, got {} bytes from '{}'", bytes.len(), s));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(arr)
}

fn decode_hex_32(s: &str) -> Result<[u8; 32]> {
    let clean = s.trim_start_matches("0x");
    let bytes = hex::decode(clean).map_err(|e| anyhow!("hex decode: {}", e))?;
    if bytes.len() != 32 {
        return Err(anyhow!("expected 32 bytes, got {}", bytes.len()));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(arr)
}

fn anchor_discriminator(ix_name: &str) -> [u8; 8] {
    let mut h = Sha256::new();
    h.update(b"global:");
    h.update(ix_name.as_bytes());
    let digest = h.finalize();
    let mut disc = [0u8; 8];
    disc.copy_from_slice(&digest[..8]);
    disc
}

fn write_compact_u16(buf: &mut Vec<u8>, mut n: u16) {
    loop {
        let mut byte = (n & 0x7f) as u8;
        n >>= 7;
        if n == 0 {
            buf.push(byte);
            return;
        }
        byte |= 0x80;
        buf.push(byte);
    }
}

/// Build a Solana legacy transaction MESSAGE (no signature prefix) with multiple
/// instructions. Mirrors `serialize_legacy_transaction_multi` in `mayan_solana.rs`
/// but accepts a real blockhash and returns only the message bytes (caller adds sigs).
///
/// `instructions` is `(program_id, account_metas, instruction_data)`.
fn build_message_multi(
    payer: [u8; 32],
    instructions: &[([u8; 32], Vec<AccountMeta>, Vec<u8>)],
    blockhash: &[u8; 32],
) -> Result<Vec<u8>> {
    use std::collections::HashMap;
    let mut caps: HashMap<[u8; 32], (bool, bool)> = HashMap::new();
    caps.insert(payer, (true, true));

    for (prog, metas, _) in instructions {
        for m in metas {
            let e = caps.entry(m.pubkey).or_insert((false, false));
            e.0 = e.0 || m.is_signer;
            e.1 = e.1 || m.is_writable;
        }
        caps.entry(*prog).or_insert((false, false));
    }

    let push_unique = |dst: &mut Vec<[u8; 32]>, key: [u8; 32]| {
        if !dst.iter().any(|k| *k == key) { dst.push(key); }
    };

    let mut signer_w: Vec<[u8; 32]> = vec![payer];
    let mut signer_r: Vec<[u8; 32]> = Vec::new();
    let mut nonsign_w: Vec<[u8; 32]> = Vec::new();
    let mut nonsign_r: Vec<[u8; 32]> = Vec::new();

    for (prog, metas, _) in instructions {
        for m in metas {
            if m.pubkey == payer { continue; }
            let (is_s, is_w) = caps.get(&m.pubkey).copied().unwrap_or((false, false));
            match (is_s, is_w) {
                (true,  true)  => push_unique(&mut signer_w, m.pubkey),
                (true,  false) => push_unique(&mut signer_r, m.pubkey),
                (false, true)  => push_unique(&mut nonsign_w, m.pubkey),
                (false, false) => push_unique(&mut nonsign_r, m.pubkey),
            }
        }
        push_unique(&mut nonsign_r, *prog);
    }

    let num_required_sigs    = (signer_w.len() + signer_r.len()) as u8;
    let num_readonly_signed   = signer_r.len() as u8;
    let num_readonly_unsigned = nonsign_r.len() as u8;

    let mut keys: Vec<[u8; 32]> = Vec::new();
    keys.extend(signer_w);
    keys.extend(signer_r);
    keys.extend(nonsign_w);
    keys.extend(nonsign_r);

    let key_index = |k: &[u8; 32]| -> Result<u8> {
        keys.iter().position(|x| x == k).map(|i| i as u8)
            .ok_or_else(|| anyhow!("internal: pubkey not in deduped key list"))
    };

    let mut msg = Vec::with_capacity(512);
    msg.push(num_required_sigs);
    msg.push(num_readonly_signed);
    msg.push(num_readonly_unsigned);
    write_compact_u16(&mut msg, keys.len() as u16);
    for k in &keys { msg.extend_from_slice(k); }
    msg.extend_from_slice(blockhash);
    write_compact_u16(&mut msg, instructions.len() as u16);
    for (prog, metas, data) in instructions {
        let prog_idx = key_index(prog)?;
        let acct_idxs: Vec<u8> = metas.iter().map(|m| key_index(&m.pubkey)).collect::<Result<_>>()?;
        msg.push(prog_idx);
        write_compact_u16(&mut msg, acct_idxs.len() as u16);
        msg.extend_from_slice(&acct_idxs);
        write_compact_u16(&mut msg, data.len() as u16);
        msg.extend_from_slice(data);
    }
    Ok(msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_signing_key_from_hex_32() {
        let hex_key = "a".repeat(64); // 32 bytes of 0xaa
        let key = load_signing_key(&hex_key).expect("hex 32-byte key");
        assert_eq!(key.verifying_key().to_bytes().len(), 32);
    }

    #[test]
    fn load_signing_key_rejects_garbage() {
        let err = load_signing_key("not-a-key").unwrap_err();
        assert!(err.to_string().contains("SOLANA_PRIVATE_KEY must be"), "{}", err);
    }

    #[test]
    fn pubkey_b58_is_base58() {
        let key = load_signing_key(&"b".repeat(64)).expect("hex key");
        let broadcaster = SolanaBroadcaster::new(key, "http://localhost");
        let pubkey = broadcaster.pubkey_b58();
        // Base58 chars: 1-9 A-H J-N P-Z a-k m-z
        assert!(pubkey.chars().all(|c| c.is_ascii_alphanumeric() && c != '0' && c != 'l' && c != 'O' && c != 'I'),
            "unexpected char in pubkey: {}", pubkey);
        assert!(pubkey.len() >= 32 && pubkey.len() <= 44, "unexpected pubkey length: {}", pubkey.len());
    }

    #[test]
    fn signed_tx_includes_compute_budget_instructions() {
        use crate::mayan_solana::{DEFAULT_MAYAN_SWIFT_PROGRAM, DEFAULT_PRIORITY_FEE_MICROLAMPORTS_PER_CU};
        // Build a signed tx with a dummy key and a zero blockhash.
        let key = load_signing_key(&"c".repeat(64)).expect("hex key");
        let broadcaster = SolanaBroadcaster::new(key, "http://localhost");
        let intent = crate::mayan_solana::MayanSolanaIntent {
            intent_id: "test".into(),
            mayan_order_id_hex: "0x9f8e7d6c5b4a3928172e3d4f5a6b7c8d9e0f1a2b3c4d5e6f7081928374651a2b".into(),
            min_amount_out: 99_850_000,
            deadline: 1745931645,
            trader_pubkey_b58: crate::mayan_solana::SYSTEM_PROGRAM_ID.into(),
            state_account_b58: crate::mayan_solana::SYSTEM_PROGRAM_ID.into(),
            vault_account_b58: crate::mayan_solana::SYSTEM_PROGRAM_ID.into(),
            swift_program_id_b58: DEFAULT_MAYAN_SWIFT_PROGRAM.into(),
            compute_units_estimate: 240_000,
        };
        let tx = broadcaster.build_signed_tx(&intent, &[0u8; 32])
            .expect("build_signed_tx");

        // SetComputeUnitPrice (tag 0x03) with a non-zero 8-byte LE price must appear.
        // Pattern: {0x09, 0x03, price[8]} — compact-u16(9) length prefix + tag + u64.
        let found_price = tx.windows(10).find_map(|w| {
            if w[0] == 0x09 && w[1] == 0x03 {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(&w[2..10]);
                let p = u64::from_le_bytes(buf);
                if p > 0 { Some(p) } else { None }
            } else { None }
        });
        assert_eq!(
            found_price,
            Some(DEFAULT_PRIORITY_FEE_MICROLAMPORTS_PER_CU),
            "broadcast tx must include SetComputeUnitPrice with the configured fee"
        );

        // SetComputeUnitLimit (tag 0x02) must also appear.
        let found_limit = tx.windows(6).any(|w| w[0] == 0x05 && w[1] == 0x02);
        assert!(found_limit, "broadcast tx must include SetComputeUnitLimit");
    }
}
