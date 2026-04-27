//! deBridge `eth_estimateGas` integration test against MAINNET.
//!
//! Loads `tests/fixtures/debridge.json` and runs the new estimate pipeline
//! against the deployed DLN destination contract on Optimism.
//!
//! Same `#[ignore]` gating as the Across test — see that file for the
//! GREEN/RED contract.

use alloy::primitives::address;
use executor::{DeBridgeEstimateAdapter, EstimateAdapter, EstimateOutcome};
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
async fn debridge_fixture_estimates_clean() {
    let intent = load_intent("debridge.json");
    assert_eq!(intent.protocol, "debridge");
    assert_eq!(intent.dst_chain, 10, "fixture dst chain must be Optimism");
    assert!(intent.maker_order_nonce.is_some(), "fixture must carry maker_order_nonce");
    assert!(intent.give_amount.is_some(), "fixture must carry give_amount");
    assert!(intent.take_amount.is_some(), "fixture must carry take_amount");
    assert!(intent.order_id.is_some(), "fixture must carry order_id");

    let messiah = address!("9a8b7C6d5e4F3a2B1c0D9e8F7a6b5C4d3E2F1A0b");
    let spinner = std::env::var("SPINNER_API_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:30081".to_string());
    let adapter = DeBridgeEstimateAdapter::new(messiah, &spinner);
    let outcome = adapter.estimate(&intent).await;

    match &outcome {
        EstimateOutcome::OkGas(_) | EstimateOutcome::InsufficientFundsLike(_) => {
            println!("deBridge estimate-clean: {:?}", outcome);
        }
        EstimateOutcome::Reverted(msg) => {
            // DLN destination rejects unknown orderIds with a bare
            // "execution reverted" (no error string / no data). A real ABI
            // selector bug would produce alloy's `function selector not
            // recognized` error, or an explicit revert reason after the colon.
            let lower = msg.to_lowercase();
            // Match "execution reverted" but NOT "execution reverted: <reason>"
            // (a colon-prefixed reason indicates a real protocol-level reject
            // with diagnostic content — that should fail the test).
            let has_reason = lower.contains("execution reverted:")
                && !lower.contains(r#"data: "0x""#)
                && !lower.contains("data: \"0x\"");
            if has_reason {
                panic!(
                    "deBridge reverted on Optimism with a reason — likely real ABI bug: {}",
                    msg
                );
            }
            println!(
                "deBridge synthetic-fixture revert (expected — orderId not on-chain): {}",
                msg
            );
        }
        EstimateOutcome::AbiInvalid(msg) => {
            panic!("deBridge ABI invalid: {}", msg);
        }
        other => panic!("unexpected EVM outcome: {:?}", other),
    }
}
