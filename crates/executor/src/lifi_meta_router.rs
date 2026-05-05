//! LiFi meta-router.
//!
//! LiFi is an *aggregator*, not a bridge: a `LiFiTransferStarted` event names
//! the underlying bridge in its `bridge` (and `tool`) field. Solver fills
//! happen against the underlying bridge's destination contract, not against
//! the LiFi Diamond. This router parses the `bridge` field on the intent and
//! dispatches to the matching `EstimateAdapter` from B.1 / B.2.
//!
//! Supported underlying bridges in this run: `across`, `debridge`,
//! `mayan` (a.k.a. `mayan_swift`). Anything else returns
//! `RouteNotImplemented(<bridge>)` — that's a soft skip, not an error.

use alloy::primitives::Address;
use async_trait::async_trait;
use genome_client::Intent;

use crate::estimate::{EstimateAdapter, EstimateOutcome};
use crate::evm_estimate::{AcrossEstimateAdapter, DeBridgeEstimateAdapter};
use crate::mayan_evm_estimate::MayanEvmEstimateAdapter;

/// LiFi meta-router. Holds one instance of every supported underlying adapter
/// and delegates per intent based on the `bridge` field.
pub struct LiFiMetaRouter {
    pub messiah_address: Address,
    pub spinner_base: String,
}

impl LiFiMetaRouter {
    pub fn new(messiah: Address, spinner_base: impl Into<String>) -> Self {
        Self {
            messiah_address: messiah,
            spinner_base: spinner_base.into(),
        }
    }

    /// Inspect the intent and return a normalized lowercase bridge slug.
    /// Falls back to `tool` when `bridge` is absent (LiFi sometimes emits
    /// only one or the other depending on integrator).
    pub fn resolve_bridge(intent: &Intent) -> Option<String> {
        intent
            .bridge
            .as_deref()
            .or(intent.tool.as_deref())
            .map(|s| s.to_lowercase())
    }

    /// Build a child intent that the underlying adapter can consume. The
    /// child intent inherits all the chain/token/amount fields from the LiFi
    /// intent and rewrites the `protocol` to match the underlying adapter's
    /// `can_handle` contract, while propagating any underlying-bridge-specific
    /// fields the adapter needs (e.g. `deposit_id` for Across,
    /// `maker_order_nonce` for deBridge, `mayan_order_id` for Mayan).
    pub fn project_to_child(intent: &Intent, underlying: &str) -> Intent {
        let mut child = intent.clone();
        child.protocol = match underlying {
            "across" | "across_v3" => "across_v3".into(),
            "debridge" | "dln" => "debridge".into(),
            "mayan" | "mayan_swift" => "mayan_swift".into(),
            other => other.to_string(),
        };
        child.id = format!("lifi→{}:{}", underlying, intent.tx_hash);
        child
    }
}

#[async_trait]
impl EstimateAdapter for LiFiMetaRouter {
    fn protocol(&self) -> &'static str {
        "lifi"
    }

    async fn estimate(&self, intent: &Intent) -> EstimateOutcome {
        let bridge = match Self::resolve_bridge(intent) {
            Some(b) => b,
            None => {
                tracing::debug!(
                    target: "lifi_meta_router::resolution",
                    intent_id = %intent.id,
                    tx_hash = %intent.tx_hash,
                    bridge = "none",
                    resolved = false,
                    "lifi intent missing bridge/tool field"
                );
                return EstimateOutcome::RouteNotImplemented(
                    "lifi intent missing bridge/tool field".into(),
                );
            }
        };

        let routable = matches!(
            bridge.as_str(),
            "across" | "across_v3" | "debridge" | "dln" | "mayan" | "mayan_swift"
        );
        tracing::debug!(
            target: "lifi_meta_router::resolution",
            intent_id = %intent.id,
            tx_hash = %intent.tx_hash,
            bridge = %bridge,
            resolved = routable,
            "lifi underlying bridge resolved"
        );

        let child = Self::project_to_child(intent, &bridge);

        match bridge.as_str() {
            "across" | "across_v3" => {
                let inner =
                    AcrossEstimateAdapter::new(self.messiah_address, &self.spinner_base);
                inner.estimate(&child).await
            }
            "debridge" | "dln" => {
                let inner =
                    DeBridgeEstimateAdapter::new(self.messiah_address, &self.spinner_base);
                inner.estimate(&child).await
            }
            "mayan" | "mayan_swift" => {
                let inner =
                    MayanEvmEstimateAdapter::new(self.messiah_address, &self.spinner_base);
                inner.estimate(&child).await
            }
            other => EstimateOutcome::RouteNotImplemented(other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lifi_intent_with(bridge: Option<&str>, tool: Option<&str>) -> Intent {
        Intent {
            id: "lifi:0xe5f60718".into(),
            protocol: "lifi".into(),
            src_chain: 1,
            dst_chain: 42161,
            src_token: "0xA0b86991c6218B36c1d19D4a2e9Eb0cE3606eB48".into(),
            dst_token: "0xaf88d065e77c8cC2239327C5EDb3A432268e5831".into(),
            amount: "100000000".into(),
            depositor: "0xb1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f70819ab".into(),
            recipient: "0xb1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f70819ab".into(),
            tx_hash: "0xe5f607182a3b4c5d6e7f8091a2b3c4d5e6f70819203a4b5c6d7e8f9a0b1c2d3e".into(),
            detected_at: 1745928040,
            output_amount: Some("99820000".into()),
            deposit_id: Some(7654321),
            bridge: bridge.map(String::from),
            tool: tool.map(String::from),
            lifi_quote_id: Some(
                "0x5f4e3d2c1b0a9988776655443322110ffeeddccbbaa99887766554433221100f".into(),
            ),
            lifi_transaction_id: Some(
                "0xa9b8c7d6e5f4a3b2c1d0e9f8a7b6c5d4e3f2a1b0c9d8e7f6a5b4c3d2e1f0a9b8".into(),
            ),
            ..Default::default()
        }
    }

    #[test]
    fn resolve_bridge_prefers_bridge_over_tool() {
        let intent = lifi_intent_with(Some("across"), Some("stargate"));
        assert_eq!(
            LiFiMetaRouter::resolve_bridge(&intent).as_deref(),
            Some("across")
        );
    }

    #[test]
    fn resolve_bridge_falls_back_to_tool() {
        let intent = lifi_intent_with(None, Some("debridge"));
        assert_eq!(
            LiFiMetaRouter::resolve_bridge(&intent).as_deref(),
            Some("debridge")
        );
    }

    #[test]
    fn project_to_child_rewrites_protocol_for_across() {
        let intent = lifi_intent_with(Some("across"), Some("across"));
        let child = LiFiMetaRouter::project_to_child(&intent, "across");
        assert_eq!(child.protocol, "across_v3");
        assert_eq!(child.deposit_id, Some(7654321));
        assert_eq!(child.dst_chain, 42161);
        assert!(
            child.id.starts_with("lifi→across:"),
            "child id should record the meta-routing hop, got {}",
            child.id
        );
    }

    #[test]
    fn project_to_child_rewrites_protocol_for_mayan() {
        let mut intent = lifi_intent_with(Some("mayan"), Some("mayan"));
        intent.mayan_order_id = Some(
            "0x7d8c9b0a1f2e3d4c5b6a708192a3b4c5d6e7f80918273645546372818091a0b1".into(),
        );
        let child = LiFiMetaRouter::project_to_child(&intent, "mayan");
        assert_eq!(child.protocol, "mayan_swift");
        assert_eq!(
            child.mayan_order_id.as_deref(),
            Some("0x7d8c9b0a1f2e3d4c5b6a708192a3b4c5d6e7f80918273645546372818091a0b1")
        );
    }

    #[tokio::test]
    async fn lifi_with_unknown_bridge_returns_route_not_implemented() {
        let router = LiFiMetaRouter::new(Address::ZERO, "http://127.0.0.1:30081");
        let intent = lifi_intent_with(Some("stargate"), Some("stargate"));
        let outcome = router.estimate(&intent).await;
        match outcome {
            EstimateOutcome::RouteNotImplemented(s) => assert_eq!(s, "stargate"),
            other => panic!("expected RouteNotImplemented(stargate), got {:?}", other),
        }
    }

    #[tokio::test]
    async fn lifi_with_no_bridge_returns_route_not_implemented() {
        let router = LiFiMetaRouter::new(Address::ZERO, "http://127.0.0.1:30081");
        let intent = lifi_intent_with(None, None);
        let outcome = router.estimate(&intent).await;
        assert!(
            matches!(outcome, EstimateOutcome::RouteNotImplemented(_)),
            "missing bridge/tool should soft-skip, got {:?}",
            outcome
        );
    }
}
