//! LiFi status-API resolver — extracted from `main.rs` so the response-parsing
//! branch (the interesting bit) is unit-testable without spinning up a network
//! mock. The HTTP fetch is a thin wrapper around `reqwest`; everything below
//! the wire is `parse_lifi_status_body`, which is pure.
//!
//! Used by `solver-main` when a LiFi genome event lacks `bridge`/`tool` *or*
//! when we need to recover the underlying-deposit tx hash that the LiFi
//! Diamond tx hides. See `main.rs::resolve_lifi_bridge` call site.

use serde_json::Value;

/// Rich resolution result from the LiFi status API.
/// Carries the bridge slug plus the actual source-side deposit tx details so the
/// enrichment path in lambda_controller can decode V3FundsDeposited correctly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifiResolution {
    pub bridge: String,
    /// txHash of the actual underlying deposit (e.g. V3FundsDeposited tx), NOT the LiFi Diamond tx.
    pub sending_tx_hash: Option<String>,
    /// Chain id where the deposit tx was emitted.
    pub sending_chain_id: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifiBridgeResult {
    /// Bridge resolved and is fillable.
    Resolved(LifiResolution),
    /// li.quest returned a known bridge slug we don't handle — skip permanently.
    NotRoutable,
    /// API unavailable or tx not yet indexed — retry on next genome event.
    Pending,
}

/// Pure parser for a li.quest `/v1/status` response body. Mirrors the logic
/// previously inlined in `main.rs::resolve_lifi_bridge` so the network path
/// stays a one-liner over this function.
///
/// - missing `tool`/`bridge` → `Pending` (tx not indexed yet).
/// - bridge in the across/debridge/mayan family → `Resolved(...)`, with the
///   `sending.txHash` + `sending.chainId` carried through if present.
/// - any other bridge slug → `NotRoutable` (Stargate, Hop, etc. are out of
///   scope for this run).
pub fn parse_lifi_status_body(body: &Value) -> LifiBridgeResult {
    let raw = match body
        .get("tool")
        .or_else(|| body.get("bridge"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase())
    {
        Some(r) => r,
        // Response parsed but no tool/bridge field — tx not yet indexed.
        None => return LifiBridgeResult::Pending,
    };
    let bridge = match raw.as_str() {
        "across" | "across_v3" => "across".to_string(),
        "debridge" | "dln" | "debridge_dln" => "debridge".to_string(),
        "mayan" | "mayan_swift" | "mayanswift" => "mayan".to_string(),
        _ => {
            tracing::debug!("LiFi bridge '{}' not routable (not Across/deBridge/Mayan)", raw);
            return LifiBridgeResult::NotRoutable;
        }
    };
    let sending = body.get("sending");
    let sending_tx_hash = sending
        .and_then(|s| s.get("txHash"))
        .and_then(|v| v.as_str())
        .filter(|s| s.starts_with("0x") && s.len() == 66)
        .map(String::from);
    let sending_chain_id = sending
        .and_then(|s| s.get("chainId"))
        .and_then(|v| v.as_u64());
    LifiBridgeResult::Resolved(LifiResolution {
        bridge,
        sending_tx_hash,
        sending_chain_id,
    })
}

/// HTTP wrapper. Fetches `https://li.quest/v1/status?txHash=...` with a 10s
/// timeout and feeds the response body to [`parse_lifi_status_body`]. Any
/// network/parse failure returns [`LifiBridgeResult::Pending`] so the caller
/// retries on the next genome event.
pub async fn resolve_lifi_bridge(tx_hash: &str) -> LifiBridgeResult {
    let url = format!("https://li.quest/v1/status?txHash={}", tx_hash);
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(_) => return LifiBridgeResult::Pending,
    };
    let mut req = client.get(&url);
    if let Ok(key) = std::env::var("LIFI_API_KEY") {
        req = req.header("x-lifi-api-key", key);
    }
    let resp = match req.send().await {
        Ok(r) => r,
        Err(_) => return LifiBridgeResult::Pending,
    };
    if !resp.status().is_success() {
        return LifiBridgeResult::Pending;
    }
    let body: Value = match resp.json().await {
        Ok(b) => b,
        Err(_) => return LifiBridgeResult::Pending,
    };
    parse_lifi_status_body(&body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Canned response shaped like a real li.quest /v1/status payload for a
    /// LiFi-via-Across hop, including the underlying-deposit `sending` block.
    /// This exercises the path that fires when the genome LiFi event lacks
    /// the `tool` (and `bridge`) field — `parse_lifi_status_body` must
    /// recover both the bridge slug and the source-side deposit tx hash.
    #[test]
    fn parses_across_with_sending_tx_into_resolved() {
        let body = json!({
            "tool": "across",
            "sending": {
                "txHash": "0xe5f607182a3b4c5d6e7f8091a2b3c4d5e6f70819203a4b5c6d7e8f9a0b1c2d3e",
                "chainId": 1u64
            }
        });
        match parse_lifi_status_body(&body) {
            LifiBridgeResult::Resolved(res) => {
                assert_eq!(res.bridge, "across");
                assert_eq!(
                    res.sending_tx_hash.as_deref(),
                    Some("0xe5f607182a3b4c5d6e7f8091a2b3c4d5e6f70819203a4b5c6d7e8f9a0b1c2d3e")
                );
                assert_eq!(res.sending_chain_id, Some(1));
            }
            other => panic!("expected Resolved, got {:?}", other),
        }
    }

    /// `bridge` is the canonical fallback when `tool` is absent — main.rs's
    /// `resolve_lifi_bridge` uses this exact precedence.
    #[test]
    fn falls_back_to_bridge_when_tool_missing() {
        let body = json!({
            "bridge": "deBridge",
            "sending": {
                "txHash": "0xf6a7b8c9d0e1f2a3b4c5d6e7f8091011121314151617181920f6a7b8c9d0e1f2",
                "chainId": 42161u64
            }
        });
        match parse_lifi_status_body(&body) {
            LifiBridgeResult::Resolved(res) => {
                assert_eq!(res.bridge, "debridge");
                assert!(res.sending_tx_hash.is_some());
                assert_eq!(res.sending_chain_id, Some(42161));
            }
            other => panic!("expected Resolved, got {:?}", other),
        }
    }

    /// Mayan slug normalization — `mayanSwift` (camelCase from li.quest)
    /// must collapse to canonical `mayan`.
    #[test]
    fn normalizes_mayanswift_to_mayan() {
        let body = json!({ "tool": "mayanSwift" });
        match parse_lifi_status_body(&body) {
            LifiBridgeResult::Resolved(res) => assert_eq!(res.bridge, "mayan"),
            other => panic!("expected Resolved, got {:?}", other),
        }
    }

    /// Out-of-scope bridges (Stargate, Hop, Connext, etc.) collapse to
    /// `NotRoutable` so the caller stops retrying.
    #[test]
    fn unknown_bridge_yields_not_routable() {
        let body = json!({ "tool": "stargate" });
        assert_eq!(parse_lifi_status_body(&body), LifiBridgeResult::NotRoutable);
    }

    /// Empty / not-yet-indexed response → `Pending` so the next genome event
    /// retries the lookup.
    #[test]
    fn missing_tool_and_bridge_yields_pending() {
        assert_eq!(parse_lifi_status_body(&json!({})), LifiBridgeResult::Pending);
        assert_eq!(
            parse_lifi_status_body(&json!({ "status": "PENDING" })),
            LifiBridgeResult::Pending
        );
    }

    /// `sending` block missing or malformed must not panic — Resolved with
    /// `None`/`None` is the contract so the caller can fall back to the
    /// Diamond tx.
    #[test]
    fn resolved_without_sending_block_returns_none_fields() {
        let body = json!({ "tool": "across" });
        match parse_lifi_status_body(&body) {
            LifiBridgeResult::Resolved(res) => {
                assert_eq!(res.bridge, "across");
                assert!(res.sending_tx_hash.is_none());
                assert!(res.sending_chain_id.is_none());
            }
            other => panic!("expected Resolved without sending, got {:?}", other),
        }
    }

    /// Malformed sending.txHash (wrong length / no 0x) is dropped silently —
    /// we don't want to feed downstream enrichment a broken hash.
    #[test]
    fn malformed_sending_tx_hash_is_dropped() {
        let body = json!({
            "tool": "across",
            "sending": { "txHash": "not-a-hash", "chainId": 1u64 }
        });
        match parse_lifi_status_body(&body) {
            LifiBridgeResult::Resolved(res) => {
                assert!(res.sending_tx_hash.is_none(), "malformed hash must be dropped");
                assert_eq!(res.sending_chain_id, Some(1));
            }
            other => panic!("expected Resolved, got {:?}", other),
        }
    }
}
