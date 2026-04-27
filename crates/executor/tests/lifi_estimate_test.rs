//! LiFi meta-router `eth_estimateGas` integration test against MAINNET.
//!
//! Loads `tests/fixtures/lifi.json` (a synthetic real-shaped genome event from
//! d874041 with `bridge=across`), projects it to a child Across intent via
//! `LiFiMetaRouter`, and runs `eth_estimateGas` against the live Arbitrum
//! SpokePool. The test asserts the outcome matches the same envelope as the
//! direct Across path: OkGas / InsufficientFundsLike / synthetic-empty revert.
//!
//! The unit-level routing tests (in `lifi_meta_router::tests`) cover the
//! "route to mayan", "route to debridge", and "unknown bridge ⇒
//! RouteNotImplemented" branches — this integration test only exercises the
//! across branch end-to-end (the only one for which a free public mainnet RPC
//! reliably accepts read calls without API keys).
//!
//! Run explicitly with:
//!     cargo test -p executor --test lifi_estimate_test -- --ignored --nocapture
//!
//! `RPC_URL_42161`/`ETH_RPC_URL`/`SPINNER_API_URL` honored if set.

use alloy::primitives::address;
use executor::{EstimateAdapter, EstimateOutcome, LiFiMetaRouter};
use genome_client::{GenomeEvent, Intent};
use std::path::PathBuf;

fn fixture_path(name: &str) -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("../..").join("tests/fixtures").join(name)
}

fn load_intent(name: &str) -> Intent {
    let raw = std::fs::read_to_string(fixture_path(name))
        .unwrap_or_else(|e| panic!("read fixture {}: {}", name, e));
    let event = GenomeEvent::from_json_str(&raw)
        .unwrap_or_else(|e| panic!("parse {} as GenomeEvent: {}", name, e));
    Intent::from_genome_event(event)
        .unwrap_or_else(|e| panic!("project {} → Intent: {}", name, e))
}

#[tokio::test]
#[ignore = "hits live mainnet RPC; run with --ignored"]
async fn lifi_fixture_routes_through_across() {
    let intent = load_intent("lifi.json");
    assert_eq!(intent.protocol, "lifi");
    assert_eq!(intent.dst_chain, 42161, "fixture dst chain must be Arbitrum");
    assert_eq!(
        intent.bridge.as_deref(),
        Some("across"),
        "fixture must route through Across"
    );
    assert!(
        intent.deposit_id.is_some(),
        "fixture must carry deposit_id for the Across child path"
    );

    let messiah = address!("742d35Cc6634C0532925a3b844Bc9e7595f0bEb1");
    let spinner = std::env::var("SPINNER_API_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:30081".to_string());
    let router = LiFiMetaRouter::new(messiah, &spinner);
    let outcome = router.estimate(&intent).await;

    match &outcome {
        EstimateOutcome::OkGas(_) | EstimateOutcome::InsufficientFundsLike(_) => {
            println!("LiFi-via-Across estimate-clean: {:?}", outcome);
        }
        EstimateOutcome::Reverted(msg) => {
            // Same caveat as the Across direct test: synthetic depositId →
            // empty revert is acceptable. A colon-prefixed reason is a real bug.
            let lower = msg.to_lowercase();
            let has_reason = lower.contains("execution reverted:")
                && !lower.contains(r#"data: "0x""#)
                && !lower.contains("data: \"0x\"");
            if has_reason {
                panic!(
                    "LiFi-via-Across reverted on Arbitrum with a reason — likely real ABI bug: {}",
                    msg
                );
            }
            println!(
                "LiFi-via-Across synthetic-fixture revert (expected — depositId not on-chain): {}",
                msg
            );
        }
        EstimateOutcome::AbiInvalid(msg) => {
            panic!("LiFi meta-router AbiInvalid (likely missing field on fixture): {}", msg);
        }
        EstimateOutcome::RouteNotImplemented(s) => {
            panic!(
                "LiFi meta-router returned RouteNotImplemented({}) — fixture's bridge field is wrong",
                s
            );
        }
        other => panic!("unexpected EVM outcome: {:?}", other),
    }
}
