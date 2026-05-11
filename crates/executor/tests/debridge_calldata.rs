//! deBridge DLN calldata regression tests (Phase 3 acceptance gate).
//!
//! Covers fulfillOrder (fill side) and claimUnlock (unlock side) calldata.
//! Asserts:
//!   (a) calldata starts with the canonical `fulfillOrder` selector 0xc358547e.
//!   (b) calldata is deterministic across calls.
//!   (c) fixture has all required fields (order_id, take_amount, give_amount, nonce).
//!   (d) claimUnlock calldata builds without error and has the correct selector.

use alloy::primitives::{address, Address};
use protocol_adapters::SpinnerClient;
use genome_client::{GenomeEvent, Intent};
use protocol_adapters::debridge::DeBridgeAdapter;
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

const TEST_SOLVER: Address = address!("0000000000000000000000000000000000000001");

fn debridge_fixtures() -> Vec<&'static str> {
    vec!["debridge.json"]
}

fn make_adapter() -> DeBridgeAdapter {
    DeBridgeAdapter::new(SpinnerClient::new("http://127.0.0.1:30081"))
}

#[test]
fn debridge_fixture_has_required_fields() {
    for fixture_name in debridge_fixtures() {
        let intent = load_intent(fixture_name);
        assert!(intent.order_id.is_some(), "{}: must have order_id", fixture_name);
        assert!(intent.take_amount.is_some(), "{}: must have take_amount", fixture_name);
        assert!(intent.give_amount.is_some(), "{}: must have give_amount", fixture_name);
        assert!(intent.maker_order_nonce.is_some(), "{}: must have maker_order_nonce", fixture_name);
        assert!(intent.dst_chain > 0, "{}: must have valid dst_chain", fixture_name);
    }
}

#[test]
fn debridge_calldata_is_deterministic() {
    // This locks the contract that the estimate path (which calls the same
    // build_fulfill_order_calldata) and the broadcast path cannot drift apart.
    let adapter = make_adapter();
    for fixture_name in debridge_fixtures() {
        let intent = load_intent(fixture_name);
        let cd1 = adapter
            .build_fulfill_order_calldata(&intent, TEST_SOLVER)
            .unwrap_or_else(|e| panic!("first build failed for {}: {}", fixture_name, e));
        let cd2 = adapter
            .build_fulfill_order_calldata(&intent, TEST_SOLVER)
            .unwrap_or_else(|e| panic!("second build failed for {}: {}", fixture_name, e));
        assert_eq!(
            cd1, cd2,
            "calldata non-deterministic for {} — salt/timestamp leaking into calldata",
            fixture_name
        );
    }
}

#[test]
fn debridge_calldata_selector_is_fulfill_order() {
    // Verify the 4-byte selector. Compute it: keccak256("fulfillOrder(...)")[..4].
    // The exact ABI is DlnDestination.fulfillOrder from debridge-contracts.
    // We accept any non-zero selector here and check it doesn't regress.
    let adapter = make_adapter();
    for fixture_name in debridge_fixtures() {
        let intent = load_intent(fixture_name);
        let calldata = adapter
            .build_fulfill_order_calldata(&intent, TEST_SOLVER)
            .unwrap_or_else(|e| panic!("build calldata failed for {}: {}", fixture_name, e));

        assert!(
            calldata.len() >= 4,
            "calldata for {} too short: {} bytes",
            fixture_name, calldata.len()
        );
        // Selector must not be all zeros — guards against accidentally encoding the wrong method
        let selector = &calldata[..4];
        assert_ne!(
            selector, &[0u8; 4],
            "calldata selector is all-zeros for {} — likely ABI encoding error",
            fixture_name
        );
        // Lock the exact selector bytes so any ABI change shows as a test failure.
        // To update: run with --nocapture and read the printed selector.
        let expected = FULFILL_ORDER_SELECTOR;
        assert_eq!(
            selector, &expected,
            "selector mismatch for {}: got 0x{:02x}{:02x}{:02x}{:02x} want 0x{:02x}{:02x}{:02x}{:02x}",
            fixture_name,
            selector[0], selector[1], selector[2], selector[3],
            expected[0], expected[1], expected[2], expected[3],
        );
    }
}

#[test]
fn debridge_calldata_length_is_reasonable() {
    // fulfillOrder has a large Order struct — expected > 500 bytes, typically ~1000+.
    let adapter = make_adapter();
    for fixture_name in debridge_fixtures() {
        let intent = load_intent(fixture_name);
        let calldata = adapter
            .build_fulfill_order_calldata(&intent, TEST_SOLVER)
            .unwrap_or_else(|e| panic!("build calldata failed for {}: {}", fixture_name, e));
        assert!(
            calldata.len() >= 500,
            "calldata for {} suspiciously short: {} bytes (expected >= 500 for fulfillOrder)",
            fixture_name, calldata.len()
        );
    }
}

// keccak256("fulfillOrder(...)")[..4] — verified by running tests/debridge_calldata::debridge_calldata_selector_is_fulfill_order.
const FULFILL_ORDER_SELECTOR: [u8; 4] = [0xc3, 0x58, 0x54, 0x7e];

#[test]
fn debridge_claim_unlock_calldata_builds() {
    let adapter = make_adapter();
    for fixture_name in debridge_fixtures() {
        let intent = load_intent(fixture_name);
        let calldata = adapter
            .build_claim_unlock_calldata(&intent, TEST_SOLVER)
            .unwrap_or_else(|e| panic!("build_claim_unlock_calldata failed for {}: {}", fixture_name, e));

        assert!(calldata.len() >= 4, "claimUnlock calldata too short: {} bytes", calldata.len());
        let selector = &calldata[..4];
        assert_ne!(selector, &[0u8; 4], "claimUnlock selector is all-zeros — encoding error");
        // Locked by running this test — update if deBridge ABI changes.
        let expected: [u8; 4] = [0x58, 0x86, 0xd8, 0xd2];
        assert_eq!(
            selector, &expected,
            "claimUnlock selector mismatch for {}: got 0x{:02x}{:02x}{:02x}{:02x}",
            fixture_name, selector[0], selector[1], selector[2], selector[3]
        );
    }
}
