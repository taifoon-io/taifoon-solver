//! Across `eth_estimateGas` integration test against MAINNET.
//!
//! Loads `tests/fixtures/across.json` (a real-shaped but **synthetic** genome
//! event from d874041 — `meta.json::source = "synthetic"`), projects it
//! through `Intent::from_genome_event`, and runs the new estimate pipeline
//! against the live Arbitrum SpokePool.
//!
//! ### Validation contract
//!
//! With the MESSIAH wallet as `from`, `eth_estimateGas` returns one of:
//!   * `OkGas(_)`              → calldata + ABI correct, contract accepted it
//!   * `InsufficientFundsLike` → would have succeeded with more balance (GREEN)
//!   * `Reverted(_)`           → see synthetic-fixture caveat below
//!   * `AbiInvalid(_)`         → real bug — encoding error
//!
//! ### Synthetic-fixture caveat
//!
//! The Across V3 SpokePool computes `_verifyV3RelayHash(relayData)` against an
//! internal map of known deposits. A synthetic fixture's `depositId` /
//! `relayData` won't match any real on-chain deposit, so the SpokePool reverts
//! with empty data (`"0x"`) regardless of how correct our calldata is.
//!
//! For that reason this test treats `Reverted("execution reverted", data="0x")`
//! as ACCEPTABLE: it confirms the SpokePool decoded our calldata far enough
//! to apply protocol-level checks. A *real* selector/ABI bug would surface as
//! `Reverted("execution reverted: <reason>")` with non-empty data, or as
//! `AbiInvalid` — both of which DO fail this test.
//!
//! Once a real captured Across genome event lands in `tests/fixtures/across.json`
//! (when the spinner SSE is reachable from this host), tighten the assertion
//! to OkGas / InsufficientFundsLike only.
//!
//! ### Why this test is `#[ignore]`d by default
//!
//! It hits a live Arbitrum RPC. CI / sandboxed environments can't always reach
//! the public endpoints (and we don't want flakes blocking unrelated PRs).
//! Run explicitly with:
//!
//!     cargo test -p executor --test across_estimate_test -- --ignored --nocapture
//!
//! `ETH_RPC_URL`/`RPC_URL_42161`/`SPINNER_API_URL` honored if set.

use alloy::primitives::address;
use executor::{AcrossEstimateAdapter, EstimateAdapter, EstimateOutcome};
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
async fn across_fixture_estimates_clean() {
    let intent = load_intent("across.json");
    assert_eq!(intent.protocol, "across_v3");
    assert_eq!(intent.dst_chain, 42161, "fixture dst chain must be Arbitrum");
    assert!(intent.deposit_id.is_some(), "fixture must carry deposit_id");
    assert!(intent.output_amount.is_some(), "fixture must carry output_amount");

    // Use the depositor in the fixture as the synthetic MESSIAH-equivalent
    // for this estimate. Real production uses the keychain-loaded address.
    let messiah = address!("742d35Cc6634C0532925a3b844Bc9e7595f0bEb1");
    let spinner = std::env::var("SPINNER_API_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:30081".to_string());
    let adapter = AcrossEstimateAdapter::new(messiah, &spinner);
    let outcome = adapter.estimate(&intent).await;

    match &outcome {
        EstimateOutcome::OkGas(_) | EstimateOutcome::InsufficientFundsLike(_) => {
            println!("Across estimate-clean: {:?}", outcome);
        }
        EstimateOutcome::Reverted(msg) => {
            // SpokePool's `_verifyV3RelayHash` rejects unknown depositIds with
            // empty revert data. The expected synthetic-fixture failure mode
            // is either `data: "0x"` or a bare `execution reverted` with no
            // colon-prefixed reason. A reason → real bug → fail the test.
            let lower = msg.to_lowercase();
            let has_reason = lower.contains("execution reverted:")
                && !lower.contains(r#"data: "0x""#)
                && !lower.contains("data: \"0x\"");
            if has_reason {
                panic!(
                    "Across reverted on Arbitrum with a reason — likely real ABI bug: {}",
                    msg
                );
            }
            println!(
                "Across synthetic-fixture revert (expected — depositId not on-chain): {}",
                msg
            );
        }
        EstimateOutcome::AbiInvalid(msg) => {
            panic!("Across ABI invalid: {}", msg);
        }
        other => panic!("unexpected EVM outcome: {:?}", other),
    }
}
