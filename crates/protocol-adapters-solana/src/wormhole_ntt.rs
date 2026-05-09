//! Wormhole NTT (Native Token Transfers) Solana adapter.
//!
//! Flow:
//!   1. EVM source: trader calls NTT Manager `transfer()` — emits a Wormhole message
//!   2. Guardians attest the transfer and produce a VAA
//!   3. Solver fetches the VAA from Wormholescan API
//!   4. Solver calls `release_inbound_mint` or `release_inbound_unlock` on the Solana NTT program
//!      passing the VAA — releases the tokens to the recipient on Solana
//!
//! TODO(wormhole-ntt): wire in VAA polling from Wormholescan once token-specific
//! NTT manager addresses are confirmed for the target token set.

use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use ed25519_dalek::SigningKey;
use tracing::info;

use genome_client::Intent;
use crate::mayan_solana::{
    anchor_discriminator, COMPUTE_BUDGET_PROGRAM_ID, DEFAULT_PRIORITY_FEE_MICROLAMPORTS_PER_CU,
};
use crate::send::SolanaSendResult;

/// Wormhole Core Bridge program (mainnet).
pub const WORMHOLE_CORE_PROGRAM_ID: &str = "worm2ZoG2kUd4vFXhvjh93UUH596ayRfgQ2MgjNMTth";

/// Wormhole NTT program (mainnet placeholder — per-deployment address varies).
/// The actual address depends on which token/deployment is being integrated.
pub const WORMHOLE_NTT_PROGRAM_ID: &str = "NTTMgrXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX"; // placeholder

/// Wormholescan VAA API base URL.
pub const WORMHOLESCAN_API_BASE: &str = "https://api.wormholescan.io";

pub const DEFAULT_NTT_COMPUTE_UNITS: u64 = 250_000;

/// Projected Wormhole NTT intent for Solana-destination fills.
#[derive(Debug)]
pub struct WormholeNttIntent {
    pub intent_id: String,
    /// Wormhole message sequence number (used to fetch the VAA).
    pub sequence: u64,
    /// Emitter chain ID (Wormhole chain ID, e.g. 2 = Ethereum).
    pub emitter_chain: u16,
    /// Emitter address (hex, 32 bytes padded).
    pub emitter_address_hex: String,
    /// The recipient pubkey on Solana.
    pub recipient_b58: String,
    /// The SPL token mint to release.
    pub token_mint_b58: String,
    /// Amount to release (raw token units).
    pub amount: u64,
    /// NTT program for this token deployment.
    pub ntt_program_id_b58: String,
    pub compute_units_estimate: u64,
}

impl WormholeNttIntent {
    pub fn from_intent(intent: &Intent) -> Result<Self> {
        let sequence = intent.id
            .split(':')
            .last()
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(|| anyhow!("WormholeNttIntent: cannot parse sequence from id={}", intent.id))?;

        Ok(Self {
            intent_id: intent.id.clone(),
            sequence,
            emitter_chain: match intent.src_chain {
                1 => 2,      // Ethereum
                56 => 4,     // BSC
                137 => 5,    // Polygon
                43114 => 6,  // Avalanche
                42161 => 23, // Arbitrum
                10 => 24,    // Optimism
                8453 => 30,  // Base
                _ => return Err(anyhow!("WormholeNttIntent: unknown src_chain={}", intent.src_chain)),
            },
            emitter_address_hex: intent.tx_hash.trim_start_matches("0x").to_string(),
            recipient_b58: intent.recipient.clone(),
            token_mint_b58: intent.dst_token.clone(),
            amount: intent.output_amount
                .as_deref()
                .and_then(|s| s.parse().ok())
                .or_else(|| intent.amount.parse().ok())
                .unwrap_or(0),
            ntt_program_id_b58: WORMHOLE_NTT_PROGRAM_ID.into(),
            compute_units_estimate: intent
                .compute_units_estimate
                .unwrap_or(DEFAULT_NTT_COMPUTE_UNITS)
                .min(u32::MAX as u64),
        })
    }
}

/// Fetches a signed VAA from the Wormholescan API.
/// Returns the base64-encoded VAA bytes.
pub async fn fetch_vaa(
    client: &reqwest::Client,
    emitter_chain: u16,
    emitter_address_hex: &str,
    sequence: u64,
) -> Result<String> {
    let url = format!(
        "{}/v1/signed_vaa/{}/{}/{}",
        WORMHOLESCAN_API_BASE, emitter_chain, emitter_address_hex, sequence
    );
    let resp = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .context("wormholescan GET")?;
    let status = resp.status();
    if !status.is_success() {
        return Err(anyhow!("wormholescan returned {status} for seq={sequence}"));
    }
    let json: serde_json::Value = resp.json().await.context("wormholescan JSON")?;
    json.get("vaaBytes")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("wormholescan response missing vaaBytes"))
}

pub struct WormholeNttBroadcaster {
    pub(crate) signing_key: SigningKey,
    pub rpc_url: String,
}

impl WormholeNttBroadcaster {
    pub fn from_env(rpc_url: &str) -> Result<Self> {
        let raw = std::env::var(crate::send::SOLANA_PRIVATE_KEY_ENV)
            .map_err(|_| anyhow!("SOLANA_PRIVATE_KEY not set"))?;
        let signing_key = crate::keychain::parse_solana_secret(&raw)?;
        Ok(Self { signing_key, rpc_url: rpc_url.into() })
    }

    pub fn new(signing_key: SigningKey, rpc_url: impl Into<String>) -> Self {
        Self { signing_key, rpc_url: rpc_url.into() }
    }

    /// Fetch the VAA and broadcast the release_inbound_mint instruction.
    pub async fn send_release_inbound(&self, intent: &WormholeNttIntent) -> Result<SolanaSendResult> {
        let client = reqwest::Client::new();

        // Step 1: fetch VAA
        let vaa_b64 = fetch_vaa(
            &client,
            intent.emitter_chain,
            &intent.emitter_address_hex,
            intent.sequence,
        )
        .await?;
        let vaa_bytes = BASE64.decode(&vaa_b64).context("decode VAA base64")?;

        // Step 2: fetch recent blockhash
        let blockhash = self.fetch_blockhash(&client).await?;

        // Step 3: build and sign tx
        let tx = self.build_release_inbound_tx(intent, &vaa_bytes, &blockhash)?;

        // Step 4: broadcast
        let tx_b64 = BASE64.encode(&tx);
        let payload = serde_json::json!({
            "jsonrpc": "2.0", "id": 1,
            "method": "sendTransaction",
            "params": [tx_b64, {"encoding": "base64", "skipPreflight": false}]
        });
        let resp: serde_json::Value = client
            .post(&self.rpc_url)
            .json(&payload)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await?
            .json()
            .await?;
        let sig = resp["result"]
            .as_str()
            .ok_or_else(|| anyhow!("sendTransaction: no result — {:?}", resp["error"]))?
            .to_string();
        info!("Wormhole NTT release_inbound sent: {}", sig);
        Ok(SolanaSendResult { signature: sig })
    }

    async fn fetch_blockhash(&self, client: &reqwest::Client) -> Result<[u8; 32]> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0", "id": 1,
            "method": "getLatestBlockhash",
            "params": [{"commitment": "confirmed"}]
        });
        let resp: serde_json::Value = client
            .post(&self.rpc_url)
            .json(&payload)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await?
            .json()
            .await?;
        let bh_str = resp
            .pointer("/result/value/blockhash")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("getLatestBlockhash: no blockhash"))?;
        let bh_bytes = bs58::decode(bh_str)
            .into_vec()
            .map_err(|e| anyhow!("blockhash bs58: {e}"))?;
        bh_bytes
            .try_into()
            .map_err(|_| anyhow!("blockhash not 32 bytes"))
    }

    pub(crate) fn build_release_inbound_tx(
        &self,
        intent: &WormholeNttIntent,
        vaa_bytes: &[u8],
        blockhash: &[u8; 32],
    ) -> Result<Vec<u8>> {
        let payer = self.signing_key.verifying_key().to_bytes();
        let program_bytes = bs58::decode(&intent.ntt_program_id_b58)
            .into_vec()
            .map_err(|e| anyhow!("decode ntt program: {e}"))?;
        let program: [u8; 32] = program_bytes
            .try_into()
            .map_err(|_| anyhow!("ntt program not 32 bytes"))?;

        // release_inbound_mint discriminator
        let disc = anchor_discriminator("release_inbound_mint");

        // Instruction data: [discriminator(8)][vaa_len_u16_le(2)][vaa_bytes]
        let mut ix_data = Vec::with_capacity(8 + 2 + vaa_bytes.len());
        ix_data.extend_from_slice(&disc);
        ix_data.extend_from_slice(&(vaa_bytes.len() as u16).to_le_bytes());
        ix_data.extend_from_slice(vaa_bytes);

        let cu_limit = intent.compute_units_estimate.min(u32::MAX as u64) as u32;
        let cb = bs58::decode(COMPUTE_BUDGET_PROGRAM_ID)
            .into_vec()
            .map_err(|e| anyhow!("decode compute budget: {e}"))?;
        let cb: [u8; 32] = cb
            .try_into()
            .map_err(|_| anyhow!("compute budget program not 32 bytes"))?;

        let mut cu_limit_data = vec![0x02u8];
        cu_limit_data.extend_from_slice(&cu_limit.to_le_bytes());
        let mut cu_price_data = vec![0x03u8];
        cu_price_data.extend_from_slice(&DEFAULT_PRIORITY_FEE_MICROLAMPORTS_PER_CU.to_le_bytes());

        // TODO(wormhole-ntt): add the full NTT account list (config, registered_emitter,
        // posted_vaa, token_authority, mint, recipient_token_account, payer, system_program)
        // once the per-token NTT manager addresses are confirmed.
        let instructions: Vec<([u8; 32], Vec<NttAccountMeta>, Vec<u8>)> = vec![
            (cb, vec![], cu_limit_data),
            (cb, vec![], cu_price_data),
            (
                program,
                vec![NttAccountMeta { pubkey: payer, is_signer: true, is_writable: true }],
                ix_data,
            ),
        ];

        let msg = build_ntt_message(payer, &instructions, blockhash)?;

        use ed25519_dalek::Signer;
        let sig = self.signing_key.sign(&msg);

        let mut tx = Vec::with_capacity(1 + 64 + msg.len());
        tx.push(1u8);
        tx.extend_from_slice(&sig.to_bytes());
        tx.extend_from_slice(&msg);
        Ok(tx)
    }
}

// ── Minimal wire-format helpers (local to this module) ───────────────────────

#[derive(Debug, Clone, Copy)]
struct NttAccountMeta {
    pubkey: [u8; 32],
    is_signer: bool,
    is_writable: bool,
}

fn write_compact_u16_ntt(buf: &mut Vec<u8>, mut n: u16) {
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

/// Build a signed Solana legacy transaction MESSAGE for the NTT release_inbound
/// instruction. Mirrors `build_message_multi` in `send.rs` but is local to this
/// module to avoid coupling to the private `AccountMeta` type there.
fn build_ntt_message(
    payer: [u8; 32],
    instructions: &[([u8; 32], Vec<NttAccountMeta>, Vec<u8>)],
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
    write_compact_u16_ntt(&mut msg, keys.len() as u16);
    for k in &keys {
        msg.extend_from_slice(k);
    }
    msg.extend_from_slice(blockhash);
    write_compact_u16_ntt(&mut msg, instructions.len() as u16);
    for (prog, metas, data) in instructions {
        let prog_idx = key_index(prog)?;
        let acct_idxs: Vec<u8> = metas
            .iter()
            .map(|m| key_index(&m.pubkey))
            .collect::<Result<_>>()?;
        msg.push(prog_idx);
        write_compact_u16_ntt(&mut msg, acct_idxs.len() as u16);
        msg.extend_from_slice(&acct_idxs);
        write_compact_u16_ntt(&mut msg, data.len() as u16);
        msg.extend_from_slice(data);
    }
    Ok(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use genome_client::Intent;

    fn ntt_intent_fixture() -> Intent {
        Intent {
            id: "wormhole_ntt:42".into(),
            protocol: "wormhole_ntt".into(),
            src_chain: 8453, // Base → Wormhole chain 30
            dst_chain: 1399811149,
            src_token: "0xA0b86991c6218B36c1d19D4a2e9Eb0cE3606eB48".into(),
            dst_token: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into(),
            amount: "1000000".into(),
            depositor: "0xdeadbeef".into(),
            recipient: "9wK4N3pTzXyZ8vQ5mB2hWnQ7tR9uVaCfDgFhJiKkMnPp".into(),
            tx_hash: "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890ab".into(),
            detected_at: 1745928045,
            output_amount: Some("990000".into()),
            compute_units_estimate: Some(250_000),
            ..Default::default()
        }
    }

    #[test]
    fn from_intent_parses_sequence() {
        let intent = ntt_intent_fixture();
        let ntt = WormholeNttIntent::from_intent(&intent).expect("from_intent");
        assert_eq!(ntt.sequence, 42);
        assert_eq!(ntt.emitter_chain, 30); // Base = Wormhole chain 30
        assert_eq!(ntt.amount, 990_000);
        assert_eq!(ntt.compute_units_estimate, 250_000);
    }

    #[test]
    fn from_intent_rejects_unknown_src_chain() {
        let mut intent = ntt_intent_fixture();
        intent.src_chain = 99999;
        let err = WormholeNttIntent::from_intent(&intent).unwrap_err();
        assert!(
            err.to_string().contains("unknown src_chain"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn from_intent_rejects_non_numeric_sequence() {
        let mut intent = ntt_intent_fixture();
        intent.id = "wormhole_ntt:not-a-number".into();
        let err = WormholeNttIntent::from_intent(&intent).unwrap_err();
        assert!(
            err.to_string().contains("cannot parse sequence"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn build_tx_produces_valid_blob() {
        let key = ed25519_dalek::SigningKey::from_bytes(&[0x42u8; 32]);
        let broadcaster = WormholeNttBroadcaster::new(key, "http://localhost");
        let ntt_intent = WormholeNttIntent {
            intent_id: "test:1".into(),
            sequence: 1,
            emitter_chain: 30,
            emitter_address_hex: "abcd".into(),
            recipient_b58: "11111111111111111111111111111111".into(),
            token_mint_b58: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into(),
            amount: 1_000_000,
            ntt_program_id_b58: "11111111111111111111111111111111".into(), // system program as placeholder
            compute_units_estimate: 250_000,
        };
        let vaa_bytes = vec![0xAAu8; 32];
        let tx = broadcaster
            .build_release_inbound_tx(&ntt_intent, &vaa_bytes, &[0u8; 32])
            .expect("build_release_inbound_tx");
        // First byte = sig count = 1
        assert_eq!(tx[0], 1u8);
        // 64-byte signature follows
        assert!(tx.len() > 65, "tx blob too small: {} bytes", tx.len());
        assert!(tx.len() <= 1232, "tx blob exceeds Solana legacy limit: {} bytes", tx.len());
    }

    #[test]
    fn release_inbound_discriminator_present_in_tx() {
        let key = ed25519_dalek::SigningKey::from_bytes(&[0x11u8; 32]);
        let broadcaster = WormholeNttBroadcaster::new(key, "http://localhost");
        let ntt_intent = WormholeNttIntent {
            intent_id: "test:2".into(),
            sequence: 2,
            emitter_chain: 2,
            emitter_address_hex: "1234".into(),
            recipient_b58: "11111111111111111111111111111111".into(),
            token_mint_b58: "11111111111111111111111111111111".into(),
            amount: 500_000,
            ntt_program_id_b58: "11111111111111111111111111111111".into(),
            compute_units_estimate: 200_000,
        };
        let vaa_bytes = vec![0xBBu8; 16];
        let tx = broadcaster
            .build_release_inbound_tx(&ntt_intent, &vaa_bytes, &[0u8; 32])
            .expect("build tx");
        let disc = anchor_discriminator("release_inbound_mint");
        let found = tx.windows(8).any(|w| w == disc);
        assert!(found, "release_inbound_mint discriminator not found in tx bytes");
    }
}
