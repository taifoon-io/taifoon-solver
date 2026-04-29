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
/// Max attempts before giving up (13s × 6 = ~78s total, covering typical finality).
const MAX_ATTEMPTS: u32 = 6;
const RETRY_DELAY_SECS: u64 = 13;

/// Fetch the first Wormhole VAA for the given source transaction hash.
/// Returns the raw VAA bytes (hex-decoded from the base64 API response).
/// Retries up to MAX_ATTEMPTS times with RETRY_DELAY_SECS between each.
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
                        tokio::time::sleep(std::time::Duration::from_secs(RETRY_DELAY_SECS)).await;
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
            tokio::time::sleep(std::time::Duration::from_secs(RETRY_DELAY_SECS)).await;
        }
    }
    warn!("wormhole VAA not available after {} attempts for {}", MAX_ATTEMPTS, src_tx_hash);
    None
}
