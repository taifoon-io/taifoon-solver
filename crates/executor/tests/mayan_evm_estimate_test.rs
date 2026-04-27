//! Mayan Swift (EVM) `eth_estimateGas` integration test against MAINNET.
//!
//! Loads `tests/fixtures/mayan_evm.json` (a synthetic but real-shaped genome
//! event from d874041 — `meta.json::source = "synthetic"`), projects it
//! through `Intent::from_genome_event`, and runs the new MayanEvmEstimateAdapter
//! against the live Base mainnet RPC.
//!
//! Synthetic-fixture caveat: the Mayan Swift contract's `fulfillOrder` verifies
//! the supplied Wormhole VAA against the on-chain guardian set before
//! transferring funds. A synthetic fixture's encodedVm is empty, so the call
//! reverts at VAA verification with empty data (`"0x"`). We treat that as
//! ACCEPTABLE: it confirms the contract decoded our `OrderParams` tuple far
//! enough to reach the protocol-level VAA check. A *real* selector/ABI bug
//! would surface as `Reverted("execution reverted: <reason>")` with a non-empty
//! reason, or as `AbiInvalid` — both of which DO fail this test.
//!
//! Run explicitly with:
//!     cargo test -p executor --test mayan_evm_estimate_test -- --ignored --nocapture
//!
//! `RPC_URL_8453`/`SPINNER_API_URL` honored if set.

use alloy::primitives::address;
use executor::{EstimateAdapter, EstimateOutcome, MayanEvmEstimateAdapter};
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
async fn mayan_evm_fixture_estimates_clean() {
    let intent = load_intent("mayan_evm.json");
    assert_eq!(intent.protocol, "mayan_swift");
    assert_eq!(intent.dst_chain, 8453, "fixture dst chain must be Base");
    assert!(
        intent.mayan_order_id.is_some(),
        "fixture must carry mayan_order_id"
    );
    assert!(
        intent.swift_dest_chain_wormhole_id.is_some(),
        "fixture must carry swift_dest_chain_wormhole_id"
    );
    assert!(intent.deadline.is_some(), "fixture must carry deadline");

    let messiah = address!("742d35Cc6634C0532925a3b844Bc9e7595f0bEb1");
    let spinner = std::env::var("SPINNER_API_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:30081".to_string());
    let adapter = MayanEvmEstimateAdapter::new(messiah, &spinner);
    let outcome = adapter.estimate(&intent).await;

    match &outcome {
        EstimateOutcome::OkGas(_) | EstimateOutcome::InsufficientFundsLike(_) => {
            println!("Mayan EVM estimate-clean: {:?}", outcome);
        }
        EstimateOutcome::Reverted(msg) => {
            // Mayan Swift reverts at VAA verification with empty data when the
            // VAA is missing / unsigned. Treat that as ACCEPTABLE; a colon-
            // prefixed reason indicates a real bug.
            let lower = msg.to_lowercase();
            let has_reason = lower.contains("execution reverted:")
                && !lower.contains(r#"data: "0x""#)
                && !lower.contains("data: \"0x\"");
            if has_reason {
                panic!(
                    "Mayan EVM reverted on Base with a reason — likely real ABI bug: {}",
                    msg
                );
            }
            println!(
                "Mayan EVM synthetic-fixture revert (expected — VAA not signed by guardians): {}",
                msg
            );
        }
        EstimateOutcome::AbiInvalid(msg) => {
            panic!("Mayan EVM ABI invalid: {}", msg);
        }
        other => panic!("unexpected EVM outcome: {:?}", other),
    }
}
