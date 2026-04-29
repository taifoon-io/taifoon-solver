//! Wormhole guardian VAA fetcher.
//!
//! Mayan Swift `fulfillOrder` (EVM) requires a guardian-signed VAA (`encodedVm`)
//! attesting the source-chain `OrderCreated` event. Guardians sign within ~13s
//! of source finality; we poll wormholescan with backoff until the VAA appears.
//!
//! API: GET https://api.wormholescan.io/api/v1/vaas?txHash=<src_tx_hash>
//! Returns the VAA(s) for a given source tx as base64 `vaa` fields.

use tracing::{info, warn};

const WORMHOLESCAN_API: &str = "https://api.wormholescan.io/api/v1/vaas";
/// Max attempts before giving up.
/// Phase 1 (attempts 1-6): every 13s = 78s to catch fast VAAs.
/// Phase 2 (attempts 7-24): every 30s = 540s additional = ~10 min total.
/// Wormhole guardians typically finalize within 5-15 minutes for slower chains.
const MAX_ATTEMPTS: u32 = 24;
const RETRY_DELAY_FAST_SECS: u64 = 13;   // first 6 attempts
const RETRY_DELAY_SLOW_SECS: u64 = 30;   // attempts 7+

/// Fetch the first Wormhole VAA for the given source transaction hash.
/// Returns the raw VAA bytes (hex-decoded from the base64 API response).
/// Retries up to MAX_ATTEMPTS times with fast then slow backoff.
pub async fn fetch_vaa_for_tx(src_tx_hash: &str) -> Option<Vec<u8>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .ok()?;

    for attempt in 1..=MAX_ATTEMPTS {
        let url = format!("{}?txHash={}", WORMHOLESCAN_API, src_tx_hash);
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let body: serde_json::Value = match resp.json().await {
                    Ok(b) => b,
                    Err(e) => {
                        warn!("wormhole parse error (attempt {}): {}", attempt, e);
                        let delay = if attempt <= 6 { RETRY_DELAY_FAST_SECS } else { RETRY_DELAY_SLOW_SECS };
                        tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                        continue;
                    }
                };
                // Response shape: {"data": [{"vaa": "<base64>", ...}, ...]}
                if let Some(vaa_b64) = body
                    .get("data")
                    .and_then(|d| d.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|v| v.get("vaa"))
                    .and_then(|v| v.as_str())
                {
                    use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
                    match B64.decode(vaa_b64) {
                        Ok(bytes) => {
                            info!("✅ Wormhole VAA fetched for {} ({} bytes)", src_tx_hash, bytes.len());
                            return Some(bytes);
                        }
                        Err(e) => {
                            warn!("wormhole base64 decode error: {}", e);
                            return None;
                        }
                    }
                }
                // VAA not yet indexed — wait and retry.
                info!("⏳ Wormhole VAA not yet available for {} (attempt {}/{})", src_tx_hash, attempt, MAX_ATTEMPTS);
            }
            Ok(resp) => {
                warn!("wormhole API HTTP {} for {} (attempt {})", resp.status(), src_tx_hash, attempt);
            }
            Err(e) => {
                warn!("wormhole API error (attempt {}): {}", attempt, e);
            }
        }
        if attempt < MAX_ATTEMPTS {
            let delay = if attempt <= 6 { RETRY_DELAY_FAST_SECS } else { RETRY_DELAY_SLOW_SECS };
            tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
        }
    }
    warn!("wormhole VAA not available after {} attempts for {}", MAX_ATTEMPTS, src_tx_hash);
    None
}
