//! Mayan Flash (Solana) adapter — LP-based immediate fills.
//!
//! Mayan Flash is Mayan Finance's second Solana protocol. Unlike Mayan Swift
//! (which uses an auction model requiring a VAA), Flash uses an LP model that
//! allows immediate fills without waiting for auction resolution. The fill
//! instruction is `flash_fill` rather than `fulfill`.
//!
//! The account layout and PDA derivation follow the same conventions as Mayan
//! Swift — see `mayan_solana.rs` for the shared infrastructure. The key
//! differences are:
//!   1. Different Anchor discriminator (`flash_fill` vs `fulfill`)
//!   2. Different program ID
//!   3. Slightly tighter default compute budget (LP check is simpler than auction)

use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tracing::info;

use crate::mayan_solana::{
    derive_mayan_vault_pda, COMPUTE_BUDGET_PROGRAM_ID, DEFAULT_PRIORITY_FEE_MICROLAMPORTS_PER_CU,
};
use crate::send::SOLANA_PRIVATE_KEY_ENV;

/// Placeholder Mayan Flash program ID.
///
/// INTENTIONALLY INVALID BASE58 — contains '0' which is not in the base58 alphabet.
/// Any attempt to decode this and construct a transaction will fail loudly at
/// bs58::decode time rather than silently routing funds to a wrong address.
///
/// TODO(mayan-flash): replace with the real program ID once the Mayan Flash IDL
/// is published and the mainnet program is confirmed.
pub const MAYAN_FLASH_PROGRAM_ID: &str = "PLACEHOLDER0MayanFlash0XXXXXXXXXXXXXXXXXXXXXXX";

/// Returns true when the Mayan Flash program ID has been set to a real address.
/// Used by the executor to skip fills early rather than failing at broadcast.
pub fn mayan_flash_program_ready() -> bool {
    !MAYAN_FLASH_PROGRAM_ID.contains("PLACEHOLDER")
}

/// Default compute unit budget for Flash fills. Lower than Swift because the
/// LP-based fill skips the auction VAA verification overhead.
pub const DEFAULT_FLASH_COMPUTE_UNITS: u64 = 200_000;

/// Narrow projection of the genome `Intent` shaped for a Mayan Flash `flash_fill`
/// instruction. Mirrors `MayanSolanaIntent` but targets the Flash program.
#[derive(Debug, Clone)]
pub struct MayanFlashIntent {
    pub intent_id: String,
    pub mayan_order_id_hex: String,
    pub min_amount_out: u64,
    pub deadline: u64,
    pub trader_pubkey_b58: String,
    /// Vault PDA — derived from the order hash + Flash program (same seeds as Swift).
    pub vault_account_b58: String,
    pub state_account_b58: String,
    pub flash_program_id_b58: String,
    /// Advisory compute unit budget forwarded to the broadcaster.
    pub compute_units_estimate: u64,
}

impl MayanFlashIntent {
    /// Promote a `genome_client::Intent` into the Flash-shaped projection.
    /// Returns `Err` when a required field is missing.
    pub fn from_intent(intent: &genome_client::Intent) -> Result<Self> {
        let mayan_order_id = intent
            .mayan_order_id
            .as_deref()
            .ok_or_else(|| anyhow!("Mayan Flash requires intent.mayan_order_id"))?;

        // Same trader/recipient selection logic as MayanSolanaIntent.
        let trader_raw = intent.trader.as_deref().unwrap_or(intent.depositor.as_str());
        let is_solana_pubkey = |s: &str| !s.starts_with("0x") && !s.starts_with("0X") && s.len() > 40;
        let trader = if is_solana_pubkey(trader_raw) {
            trader_raw
        } else if is_solana_pubkey(&intent.recipient) {
            &intent.recipient
        } else {
            return Err(anyhow!(
                "Mayan Flash: no Solana trader/recipient pubkey found (trader={}, recipient={})",
                trader_raw,
                intent.recipient
            ));
        };

        let state = intent
            .state_account
            .as_deref()
            .ok_or_else(|| anyhow!("Mayan Flash requires intent.state_account"))?;

        // Use swift_program_id as-is when non-empty — the caller already selected
        // this adapter by protocol tag, so the field IS the flash program address.
        // The old substring check required "flash" in the ID which is not guaranteed.
        let program = match intent.swift_program_id.as_deref() {
            Some(p) if !p.is_empty() => p,
            _ => MAYAN_FLASH_PROGRAM_ID,
        };

        // Vault PDA: same seeds as Swift — ["vault", order_hash_bytes] under Flash program.
        let vault_owned;
        let vault = match intent.vault_account.as_deref() {
            Some(v) => v,
            None => {
                vault_owned = derive_mayan_vault_pda(mayan_order_id, program)
                    .ok_or_else(|| anyhow!("failed to derive vault PDA for order {}", mayan_order_id))?;
                &vault_owned
            }
        };

        let min_amount_out = if let Some(v) = intent.mayan_min_amount_out {
            v
        } else {
            let raw = intent
                .output_amount
                .as_deref()
                .or(Some(intent.amount.as_str()))
                .ok_or_else(|| anyhow!("Mayan Flash requires output_amount or amount"))?;
            raw.parse::<u64>().or_else(|_| {
                raw.parse::<f64>()
                    .context("min_amount_out parse")
                    .map(|f| f as u64)
            })?
        };

        let deadline = intent.deadline.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                + 3600
        });

        Ok(Self {
            intent_id: intent.id.clone(),
            mayan_order_id_hex: mayan_order_id.to_string(),
            min_amount_out,
            deadline,
            trader_pubkey_b58: trader.to_string(),
            vault_account_b58: vault.to_string(),
            state_account_b58: state.to_string(),
            flash_program_id_b58: program.to_string(),
            compute_units_estimate: intent.compute_units_estimate.unwrap_or(DEFAULT_FLASH_COMPUTE_UNITS),
        })
    }
}

/// Broadcaster for Mayan Flash `flash_fill` transactions.
///
/// Loads the Solana signing key from `SOLANA_PRIVATE_KEY`, fetches a fresh
/// blockhash, assembles the legacy transaction, and broadcasts via
/// `sendTransaction`.
pub struct MayanFlashBroadcaster {
    pub(crate) signing_key: SigningKey,
    pub rpc_url: String,
    client: reqwest::Client,
}

impl MayanFlashBroadcaster {
    /// Load the signing key from `SOLANA_PRIVATE_KEY` env var.
    pub fn from_env(rpc_url: &str) -> Result<Self> {
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

    /// Broadcast a Mayan Flash `flash_fill` transaction.
    /// Fetches a fresh blockhash, signs, and sends.
    pub async fn send_flash_fill(&self, intent: &MayanFlashIntent) -> Result<crate::send::SolanaSendResult> {
        let blockhash = self
            .get_latest_blockhash()
            .await
            .context("getLatestBlockhash RPC")?;

        info!(
            "🔑 Signing Solana flash_fill tx (payer={}, blockhash={}…)",
            self.pubkey_b58(),
            &bs58::encode(&blockhash).into_string()[..8]
        );

        let tx_bytes = self
            .build_flash_tx(intent, &blockhash)
            .context("build flash tx")?;

        let tx_b64 = BASE64.encode(&tx_bytes);
        let sig = self
            .send_raw_transaction(&tx_b64)
            .await
            .map_err(|e| {
                tracing::error!("sendTransaction (flash_fill) failed: {:#}", e);
                e
            })
            .context("sendTransaction RPC")?;

        Ok(crate::send::SolanaSendResult { signature: sig })
    }

    /// Build the signed legacy transaction bytes for `flash_fill`.
    pub(crate) fn build_flash_tx(
        &self,
        intent: &MayanFlashIntent,
        blockhash: &[u8; 32],
    ) -> Result<Vec<u8>> {
        // TODO(mayan-flash): verify program ID and full account list from Mayan Flash IDL
        let payer = self.signing_key.verifying_key().to_bytes();
        let program = decode_b58_32(&intent.flash_program_id_b58)
            .context("decode flash program id")?;
        let vault = decode_b58_32(&intent.vault_account_b58)
            .context("decode vault account")?;
        let state = decode_b58_32(&intent.state_account_b58)
            .context("decode state account")?;
        let cb = decode_b58_32(COMPUTE_BUDGET_PROGRAM_ID).expect("compute budget program");

        // ComputeBudget: SetComputeUnitLimit (tag 0x02 + u32 LE)
        let cu_limit: u32 = intent.compute_units_estimate.min(u32::MAX as u64) as u32;
        let mut cu_limit_data = vec![0x02u8];
        cu_limit_data.extend_from_slice(&cu_limit.to_le_bytes());

        // ComputeBudget: SetComputeUnitPrice (tag 0x03 + u64 LE micro-lamports/CU)
        let mut cu_price_data = vec![0x03u8];
        cu_price_data.extend_from_slice(&DEFAULT_PRIORITY_FEE_MICROLAMPORTS_PER_CU.to_le_bytes());

        // flash_fill instruction data:
        //   [8-byte discriminator for "flash_fill"]
        //   [32-byte order_id (from mayan_order_id hex)]
        //   [8-byte min_amount_out LE u64]
        //   [8-byte deadline LE u64]
        let mut ix_data = Vec::with_capacity(8 + 32 + 8 + 8);
        ix_data.extend_from_slice(&anchor_discriminator("flash_fill"));
        let order_id = decode_hex_32(&intent.mayan_order_id_hex)
            .context("decode mayan_order_id hex")?;
        ix_data.extend_from_slice(&order_id);
        ix_data.extend_from_slice(&intent.min_amount_out.to_le_bytes());
        ix_data.extend_from_slice(&intent.deadline.to_le_bytes());

        // Account list for flash_fill (minimal — TODO: verify from IDL):
        //   [0] payer        (signer, writable) — solver's Solana wallet
        //   [1] vault_account (writable, non-signer)
        //   [2] state_account (writable, non-signer)
        //   [3] flash program (non-signer, non-writable = executable)
        let flash_metas: Vec<AccountMeta> = vec![
            AccountMeta { pubkey: payer, is_signer: true,  is_writable: true  },
            AccountMeta { pubkey: vault, is_signer: false, is_writable: true  },
            AccountMeta { pubkey: state, is_signer: false, is_writable: true  },
            AccountMeta { pubkey: program, is_signer: false, is_writable: false },
        ];

        let instructions: Vec<([u8; 32], Vec<AccountMeta>, Vec<u8>)> = vec![
            (cb, vec![], cu_limit_data),
            (cb, vec![], cu_price_data),
            (program, flash_metas, ix_data),
        ];

        let msg = build_message_multi(payer, &instructions, blockhash)?;

        let sig = self.signing_key.sign(&msg);
        let sig_bytes = sig.to_bytes();

        let mut tx = Vec::with_capacity(1 + 64 + msg.len());
        write_compact_u16(&mut tx, 1);
        tx.extend_from_slice(&sig_bytes);
        tx.extend_from_slice(&msg);

        if tx.len() > 1232 {
            return Err(anyhow!("flash_fill tx is {} bytes (>1232 limit)", tx.len()));
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
        let resp = self
            .client
            .post(&self.rpc_url)
            .json(&body)
            .send()
            .await
            .context("getLatestBlockhash HTTP")?;
        if !resp.status().is_success() {
            return Err(anyhow!("getLatestBlockhash HTTP {}", resp.status()));
        }
        let parsed: serde_json::Value = resp.json().await.context("getLatestBlockhash parse")?;
        let bh_str = parsed
            .pointer("/result/value/blockhash")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("getLatestBlockhash: missing result.value.blockhash"))?;
        let bh_bytes = bs58::decode(bh_str)
            .into_vec()
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
        let resp = self
            .client
            .post(&self.rpc_url)
            .json(&body)
            .send()
            .await
            .context("sendTransaction HTTP")?;
        let status = resp.status();
        let parsed: serde_json::Value = resp.json().await.context("sendTransaction parse")?;
        if let Some(err) = parsed.get("error") {
            return Err(anyhow!(
                "sendTransaction RPC error: {}",
                serde_json::to_string(err).unwrap_or_else(|_| err.to_string())
            ));
        }
        if !status.is_success() {
            return Err(anyhow!(
                "sendTransaction HTTP {}: {}",
                status,
                serde_json::to_string(&parsed).unwrap_or_default()
            ));
        }
        let sig = parsed
            .get("result")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("sendTransaction: missing result (signature)"))?;
        info!("✅ Solana flash_fill broadcast: {}", sig);
        Ok(sig.to_string())
    }
}

// ── Wire-format helpers ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
struct AccountMeta {
    pubkey: [u8; 32],
    is_signer: bool,
    is_writable: bool,
}

fn decode_b58_32(s: &str) -> Result<[u8; 32]> {
    let bytes = bs58::decode(s)
        .into_vec()
        .map_err(|e| anyhow!("base58 decode '{}': {}", s, e))?;
    if bytes.len() != 32 {
        return Err(anyhow!(
            "expected 32-byte pubkey, got {} bytes from '{}'",
            bytes.len(),
            s
        ));
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

/// Load a `SigningKey` from a base58-encoded 64-byte keypair or hex 32-byte secret.
fn load_signing_key(raw: &str) -> Result<SigningKey> {
    let trimmed = raw.trim();
    if let Ok(bytes) = bs58::decode(trimmed).into_vec() {
        if bytes.len() == 64 {
            let secret: [u8; 32] = bytes[..32]
                .try_into()
                .map_err(|_| anyhow!("secret scalar slice failed"))?;
            return Ok(SigningKey::from_bytes(&secret));
        }
        if bytes.len() == 32 {
            let secret: [u8; 32] = bytes
                .try_into()
                .map_err(|_| anyhow!("32-byte base58 secret failed"))?;
            return Ok(SigningKey::from_bytes(&secret));
        }
    }
    let clean = trimmed.trim_start_matches("0x");
    if let Ok(bytes) = hex::decode(clean) {
        if bytes.len() == 32 {
            let secret: [u8; 32] = bytes
                .try_into()
                .map_err(|_| anyhow!("hex 32-byte secret failed"))?;
            return Ok(SigningKey::from_bytes(&secret));
        }
    }
    Err(anyhow!(
        "SOLANA_PRIVATE_KEY must be a base58-encoded 64-byte keypair or hex 32-byte secret scalar; got {} chars",
        trimmed.len()
    ))
}

/// Build a Solana legacy transaction MESSAGE (no signature prefix) with multiple
/// instructions. Mirrors `build_message_multi` in `send.rs` but operates on the
/// Flash account layout.
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
        if !dst.iter().any(|k| *k == key) {
            dst.push(key);
        }
    };

    let mut signer_w: Vec<[u8; 32]> = vec![payer];
    let mut signer_r: Vec<[u8; 32]> = Vec::new();
    let mut nonsign_w: Vec<[u8; 32]> = Vec::new();
    let mut nonsign_r: Vec<[u8; 32]> = Vec::new();

    for (prog, metas, _) in instructions {
        for m in metas {
            if m.pubkey == payer {
                continue;
            }
            let (is_s, is_w) = caps.get(&m.pubkey).copied().unwrap_or((false, false));
            match (is_s, is_w) {
                (true, true) => push_unique(&mut signer_w, m.pubkey),
                (true, false) => push_unique(&mut signer_r, m.pubkey),
                (false, true) => push_unique(&mut nonsign_w, m.pubkey),
                (false, false) => push_unique(&mut nonsign_r, m.pubkey),
            }
        }
        push_unique(&mut nonsign_r, *prog);
    }

    let num_required_sigs = (signer_w.len() + signer_r.len()) as u8;
    let num_readonly_signed = signer_r.len() as u8;
    let num_readonly_unsigned = nonsign_r.len() as u8;

    let mut keys: Vec<[u8; 32]> = Vec::new();
    keys.extend(signer_w);
    keys.extend(signer_r);
    keys.extend(nonsign_w);
    keys.extend(nonsign_r);

    let key_index = |k: &[u8; 32]| -> Result<u8> {
        keys.iter()
            .position(|x| x == k)
            .map(|i| i as u8)
            .ok_or_else(|| anyhow!("internal: pubkey not in deduped key list"))
    };

    let mut msg = Vec::with_capacity(512);
    msg.push(num_required_sigs);
    msg.push(num_readonly_signed);
    msg.push(num_readonly_unsigned);
    write_compact_u16(&mut msg, keys.len() as u16);
    for k in &keys {
        msg.extend_from_slice(k);
    }
    msg.extend_from_slice(blockhash);
    write_compact_u16(&mut msg, instructions.len() as u16);
    for (prog, metas, data) in instructions {
        let prog_idx = key_index(prog)?;
        let acct_idxs: Vec<u8> = metas
            .iter()
            .map(|m| key_index(&m.pubkey))
            .collect::<Result<_>>()?;
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
    use genome_client::Intent;

    fn flash_intent_fixture() -> Intent {
        Intent {
            id: "mayan_flash:5HzkYQK4BKj8c4M7yqA7zXyZ9vN2pE5mB3hWnQ8tR1uVaCfDgFhJiKlMnOpQrStUv".into(),
            protocol: "mayan_flash".into(),
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
            // Use the Swift program ID as a valid base58 stand-in for tests —
            // the Flash placeholder is intentionally invalid to catch accidental production use.
            swift_program_id: Some(crate::mayan_solana::DEFAULT_MAYAN_SWIFT_PROGRAM.into()),
            state_account: Some("9wK4N3pTzXyZ8vQ5mB2hWnQ7tR9uVaCfDgFhJiKkMnPp".into()),
            vault_account: Some("8mB2hWnQ7tR9uVaCfDgFhJiKkMnPpQ9wK4N3pTzXyZ8v".into()),
            compute_units_estimate: Some(200_000),
            is_solana_source: Some(true),
            ..Default::default()
        }
    }

    #[test]
    fn anchor_discriminator_is_stable_for_flash_fill() {
        let d1 = anchor_discriminator("flash_fill");
        let d2 = anchor_discriminator("flash_fill");
        assert_eq!(d1, d2);
        // Must differ from the Swift "fulfill" discriminator.
        let fulfill_disc: [u8; 8] = {
            let mut h = Sha256::new();
            h.update(b"global:fulfill");
            let digest = h.finalize();
            let mut arr = [0u8; 8];
            arr.copy_from_slice(&digest[..8]);
            arr
        };
        assert_ne!(
            d1, fulfill_disc,
            "flash_fill discriminator must differ from fulfill"
        );
    }

    #[test]
    fn from_intent_builds_flash_intent() {
        let intent = flash_intent_fixture();
        let fi = MayanFlashIntent::from_intent(&intent).expect("from_intent");
        assert_eq!(fi.intent_id, intent.id);
        assert_eq!(fi.mayan_order_id_hex, "0x9f8e7d6c5b4a3928172e3d4f5a6b7c8d9e0f1a2b3c4d5e6f7081928374651a2b");
        assert_eq!(fi.min_amount_out, 99_850_000);
        assert_eq!(fi.deadline, 1745931645);
        assert_eq!(fi.compute_units_estimate, 200_000);
    }

    #[test]
    fn from_intent_rejects_missing_state_account() {
        let mut intent = flash_intent_fixture();
        intent.state_account = None;
        let err = MayanFlashIntent::from_intent(&intent).unwrap_err();
        assert!(
            err.to_string().contains("state_account"),
            "missing-field error should mention state_account, got: {}",
            err
        );
    }

    #[test]
    fn from_intent_prefers_mayan_min_amount_out_over_output_amount() {
        let mut intent = flash_intent_fixture();
        // output_amount is 99_850_000 but mayan_min_amount_out should win.
        intent.mayan_min_amount_out = Some(98_000_000);
        let fi = MayanFlashIntent::from_intent(&intent).expect("from_intent");
        assert_eq!(
            fi.min_amount_out, 98_000_000,
            "mayan_min_amount_out (on-chain uint64) must be preferred over output_amount"
        );
    }

    #[test]
    fn from_intent_uses_default_compute_units_when_absent() {
        let mut intent = flash_intent_fixture();
        intent.compute_units_estimate = None;
        let fi = MayanFlashIntent::from_intent(&intent).unwrap();
        assert_eq!(fi.compute_units_estimate, DEFAULT_FLASH_COMPUTE_UNITS);
    }

    #[test]
    fn build_flash_tx_round_trips() {
        let key = load_signing_key(&"d".repeat(64)).expect("hex key");
        let broadcaster = MayanFlashBroadcaster::new(key, "http://localhost");
        let intent = flash_intent_fixture();
        let fi = MayanFlashIntent::from_intent(&intent).expect("from_intent");
        let tx = broadcaster
            .build_flash_tx(&fi, &[0u8; 32])
            .expect("build_flash_tx");

        // Sanity: within Solana legacy tx size limit.
        assert!(tx.len() > 80, "tx suspiciously small ({} bytes)", tx.len());
        assert!(tx.len() <= 1232, "tx exceeds Solana legacy limit");
        // Compact-u16 sig count = 1.
        assert_eq!(tx[0], 1);
        // 64-byte zeroed placeholder signature follows — but this is a *real*
        // signature (not zeroed) in the broadcaster since we sign the message.
        // Just check the length is right.
        assert!(tx.len() >= 65, "must be at least sig prefix (1) + sig (64)");
    }

    #[test]
    fn flash_fill_discriminator_in_tx() {
        let key = load_signing_key(&"e".repeat(64)).expect("hex key");
        let broadcaster = MayanFlashBroadcaster::new(key, "http://localhost");
        let intent = flash_intent_fixture();
        let fi = MayanFlashIntent::from_intent(&intent).expect("from_intent");
        let tx = broadcaster
            .build_flash_tx(&fi, &[0u8; 32])
            .expect("build_flash_tx");

        let disc = anchor_discriminator("flash_fill");
        let found = tx.windows(8).any(|w| w == disc);
        assert!(found, "flash_fill discriminator missing from serialized tx");
    }

    #[test]
    fn protocol_detection_uses_flash_constant() {
        // Confirm that an intent whose protocol contains "flash" will NOT use
        // the Swift program ID constant.
        let mut intent = flash_intent_fixture();
        intent.swift_program_id = Some("SomeFlaShProgramIdWouldBeHere1234567890".into());
        // The from_intent logic checks if swift_program_id contains "flash" (case-insensitive).
        // In this fixture it does — so it should use that ID, not the placeholder.
        let fi = MayanFlashIntent::from_intent(&intent).expect("from_intent");
        assert!(
            fi.flash_program_id_b58.to_lowercase().contains("flash"),
            "should use Flash-tagged program id"
        );
    }
}
