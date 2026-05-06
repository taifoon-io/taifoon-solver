//! Wormhole guardian VAA fetcher for Mayan Swift EVM fills.
//!
//! `fulfillOrder` requires a guardian-signed VAA (`encodedVm`) attesting the
//! source-chain `OrderCreated` event. The VAA is emitted by the Mayan Swift
//! Forwarder contract (`0xd78d199f8c402e7b5cc2abe278df0412400a3bae`) on the
//! source chain — the same address on every supported EVM chain.
//!
//! Payload layout (35 bytes):
//!   [0]     = 0x05  (Mayan Swift message type)
//!   [1..2]  = reserved / padding
//!   [3..34] = 32-byte order hash (matches `mayan_order_id`)
//!
//! Lookup strategy: query wormholescan by emitter chain + emitter address,
//! scan recent VAAs descending, and return the first one whose payload
//! matches the target order hash.  This is ~4× faster than the old
//! `?txHash=` approach (which indexed by the Wormhole bridge contract tx,
//! NOT Mayan's user swap tx) and actually works.

use tracing::{info, warn};

const WORMHOLESCAN_API: &str = "https://api.wormholescan.io/api/v1/vaas";
const MAYAN_FORWARDER: &str = "0xd78d199f8c402e7b5cc2abe278df0412400a3bae";

/// Max attempts before giving up.  Phase 1 (1-6): 13 s each.  Phase 2 (7-24): 30 s each.
const MAX_ATTEMPTS: u32 = 24;
const RETRY_DELAY_FAST_SECS: u64 = 13;
const RETRY_DELAY_SLOW_SECS: u64 = 30;

/// How many recent VAAs to fetch per page when scanning for the order hash.
const PAGE_SIZE: u32 = 20;

/// Map an EVM chain ID to its Wormhole chain ID (Mayan uses the same numbers).
fn evm_to_wormhole_chain(evm_chain: u64) -> Option<u64> {
    match evm_chain {
        1      => Some(2),    // Ethereum
        56     => Some(4),    // BSC
        137    => Some(5),    // Polygon
        43114  => Some(6),    // Avalanche
        42161  => Some(23),   // Arbitrum
        8453   => Some(30),   // Base
        10     => Some(24),   // Optimism
        _      => None,
    }
}

/// Decode raw VAA bytes and return the 32-byte order hash from the Mayan payload,
/// or `None` if the VAA is not a Mayan Swift v2 order message.
fn extract_order_hash_from_vaa(raw: &[u8]) -> Option<[u8; 32]> {
    if raw.len() < 6 { return None; }
    let num_sigs = raw[5] as usize;
    let sig_offset = 6 + num_sigs * 66;
    if raw.len() < sig_offset + 51 { return None; }
    let core = &raw[sig_offset..];
    // core[0..4]=timestamp, [4..8]=nonce, [8..10]=emitter_chain, [10..42]=emitter_addr,
    // [42..50]=sequence, [50]=consistency, [51..]=payload
    if core.len() < 51 + 35 { return None; }
    let payload = &core[51..];
    if payload.len() < 35 || payload[0] != 0x05 { return None; }
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&payload[3..35]);
    Some(hash)
}

/// Fetch the Wormhole VAA for a Mayan Swift order.
///
/// `src_evm_chain` is the EVM chain ID of the source chain.
/// `order_hash` is the 32-byte Mayan order hash (hex string, with or without 0x prefix).
///
/// Retries up to MAX_ATTEMPTS times with backoff, scanning the emitter's
/// recent VAAs each attempt until the matching one appears.
pub async fn fetch_vaa_for_mayan_order(src_evm_chain: u64, order_hash: &str) -> Option<Vec<u8>> {
    let wh_chain = match evm_to_wormhole_chain(src_evm_chain) {
        Some(c) => c,
        None => {
            warn!("fetch_vaa: no wormhole chain for EVM chain {}", src_evm_chain);
            return None;
        }
    };

    let order_hash_clean = order_hash.strip_prefix("0x").unwrap_or(order_hash);
    let target: [u8; 32] = match hex::decode(order_hash_clean) {
        Ok(b) if b.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&b);
            arr
        }
        _ => {
            warn!("fetch_vaa: invalid order hash '{}'", order_hash);
            return None;
        }
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .ok()?;

    let url_base = format!(
        "{}/{}/{emitter}?pageSize={PAGE_SIZE}&sortOrder=DESC",
        WORMHOLESCAN_API,
        wh_chain,
        emitter = MAYAN_FORWARDER,
    );

    for attempt in 1..=MAX_ATTEMPTS {
        match client.get(&url_base).send().await {
            Ok(resp) if resp.status().is_success() => {
                let body: serde_json::Value = match resp.json().await {
                    Ok(b) => b,
                    Err(e) => {
                        warn!("wormhole parse error (attempt {}): {}", attempt, e);
                        sleep_attempt(attempt).await;
                        continue;
                    }
                };

                if let Some(arr) = body.get("data").and_then(|d| d.as_array()) {
                    for entry in arr {
                        let vaa_b64 = match entry.get("vaa").and_then(|v| v.as_str()) {
                            Some(s) => s,
                            None => continue,
                        };
                        use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
                        let raw = match B64.decode(vaa_b64) {
                            Ok(b) => b,
                            Err(_) => continue,
                        };
                        if let Some(hash) = extract_order_hash_from_vaa(&raw) {
                            if hash == target {
                                let seq = entry.get("sequence").and_then(|s| s.as_u64()).unwrap_or(0);
                                info!("✅ Mayan VAA found: chain={} seq={} order_hash={}", wh_chain, seq, order_hash);
                                return Some(raw);
                            }
                        }
                    }
                    // None of the recent VAAs matched yet
                    info!(
                        "⏳ Mayan VAA not yet indexed for order {} (attempt {}/{}, scanned {} VAAs)",
                        order_hash, attempt, MAX_ATTEMPTS,
                        arr.len()
                    );
                }
            }
            Ok(resp) => {
                warn!("wormhole API HTTP {} (attempt {})", resp.status(), attempt);
            }
            Err(e) => {
                warn!("wormhole API error (attempt {}): {}", attempt, e);
            }
        }
        if attempt < MAX_ATTEMPTS {
            sleep_attempt(attempt).await;
        }
    }
    warn!(
        "Mayan VAA not found after {} attempts for order {}",
        MAX_ATTEMPTS, order_hash
    );
    None
}

async fn sleep_attempt(attempt: u32) {
    let delay = if attempt <= 6 { RETRY_DELAY_FAST_SECS } else { RETRY_DELAY_SLOW_SECS };
    tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
}
