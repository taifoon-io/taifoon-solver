//! Across V3 SpokePool calldata regression tests (Phase 2 acceptance gate).
//!
//! For every Across-shaped fixture under `tests/fixtures/across*.json` (including
//! the LiFi-via-Across projection), build the `fillRelay` calldata via
//! `build_across_spoke_pool_calldata_with_relayer` and assert the canonical
//! invariants the lambda controller relies on:
//!
//!   (a) calldata starts with the canonical `fillRelay` selector
//!       0xdeff4b24 — keccak("fillRelay((bytes32,bytes32,bytes32,bytes32,bytes32,
//!                                 uint256,uint256,uint256,uint256,uint32,uint32,bytes),
//!                                 uint256,bytes32)")[..4].
//!   (b) decoded `outputAmount` ≤ decoded `inputAmount` — broadcasting the
//!       inverse would mean we pay out more than we receive.
//!   (c) the encoded `originChainId` matches the intent's `src_chain`.
//!       Note: the SpokePool itself is deployed on `intent.dst_chain`; the
//!       calldata's `originChainId` field carries the source-chain id, which
//!       is what the V3 relay tuple semantically requires.
//!
//! These three properties were the bug surface that motivated commits
//! `cb39d83` (estimate↔broadcast drift) and `0549550` (selector parity).

use alloy::primitives::U256;
use alloy::sol;
use alloy::sol_types::SolCall;
use executor::across_executor::build_across_spoke_pool_calldata_with_relayer;
use genome_client::{GenomeEvent, Intent};
use std::path::PathBuf;

// Local re-declaration of the relay tuple — kept identical to the producer side
// so we can ABI-decode the calldata without re-exporting the producer's types.
sol! {
    interface IAcrossSpokePool {
        function fillRelay(
            RelayData calldata relayData,
            uint256 repaymentChainId,
            bytes32 repaymentAddress
        ) external;
    }

    struct RelayData {
        bytes32 depositor;
        bytes32 recipient;
        bytes32 exclusiveRelayer;
        bytes32 inputToken;
        bytes32 outputToken;
        uint256 inputAmount;
        uint256 outputAmount;
        uint256 originChainId;
        uint256 depositId;
        uint32  fillDeadline;
        uint32  exclusivityDeadline;
        bytes   message;
    }
}

/// Canonical selector for `fillRelay((bytes32×5, uint256×4, uint32×2, bytes), uint256, bytes32)`.
const FILL_RELAY_SELECTOR: [u8; 4] = [0xde, 0xff, 0x4b, 0x24];

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

/// All Across-shaped fixtures the executor must be able to encode for fillRelay.
/// Includes the LiFi-via-Across projection — once routed through the meta-router
/// it lands in the same code path as a native Across event.
fn across_fixtures() -> Vec<&'static str> {
    vec!["across.json", "lifi_via_across_base.json"]
}

/// (a) selector + (b) outputAmount ≤ inputAmount + (c) originChainId == src_chain.
#[test]
fn across_fixtures_round_trip_through_fill_relay_calldata() {
    let fixtures = across_fixtures();
    assert!(!fixtures.is_empty(), "expected at least one Across fixture");

    for name in fixtures {
        let intent = load_intent(name);
        // sanity: load_intent gave us an Across-shaped projection
        assert!(
            intent.deposit_id.is_some(),
            "fixture {name} missing deposit_id after enrichment"
        );

        let calldata = build_across_spoke_pool_calldata_with_relayer(&intent, None, Some(intent.dst_chain))
            .unwrap_or_else(|e| panic!("build calldata for {name}: {e:?}"));

        // (a) selector
        assert!(
            calldata.len() > 4,
            "fixture {name}: calldata too short to carry a selector"
        );
        assert_eq!(
            &calldata[..4],
            &FILL_RELAY_SELECTOR,
            "fixture {name}: selector mismatch — expected fillRelay 0xdeff4b24"
        );

        // ABI-decode to extract the encoded fields.
        let decoded = IAcrossSpokePool::fillRelayCall::abi_decode(&calldata, true)
            .unwrap_or_else(|e| panic!("abi_decode fillRelay for {name}: {e:?}"));

        // (b) outputAmount ≤ inputAmount
        assert!(
            decoded.relayData.outputAmount <= decoded.relayData.inputAmount,
            "fixture {name}: output ({}) > input ({}) — would be a money-losing fill",
            decoded.relayData.outputAmount,
            decoded.relayData.inputAmount,
        );

        // (c) originChainId == intent.src_chain
        assert_eq!(
            decoded.relayData.originChainId,
            U256::from(intent.src_chain),
            "fixture {name}: encoded originChainId != intent.src_chain"
        );

        // Cross-check input amount against the intent's amount string. This catches
        // a class of bug where `intent.amount` and the encoded `inputAmount` drift
        // (e.g., a future refactor that lossy-converts u256 → u128 → u256).
        let expected_input = U256::from_str_radix(&intent.amount, 10)
            .unwrap_or_else(|e| panic!("fixture {name}: intent.amount parse: {e:?}"));
        assert_eq!(
            decoded.relayData.inputAmount, expected_input,
            "fixture {name}: encoded inputAmount drifts from intent.amount"
        );
    }
}

/// Determinism: building the same calldata twice yields byte-identical output.
/// Locks the estimate↔broadcast parity property exercised by commits
/// `cb39d83` and `0549550`. If a future refactor introduces non-determinism
/// (e.g. relayer-default that depends on system clock), this lights up first.
#[test]
fn across_fixture_calldata_is_deterministic() {
    for name in across_fixtures() {
        let intent = load_intent(name);
        let a = build_across_spoke_pool_calldata_with_relayer(&intent, None, Some(intent.dst_chain))
            .unwrap_or_else(|e| panic!("first build {name}: {e:?}"));
        let b = build_across_spoke_pool_calldata_with_relayer(&intent, None, Some(intent.dst_chain))
            .unwrap_or_else(|e| panic!("second build {name}: {e:?}"));
        assert_eq!(
            a, b,
            "fixture {name}: calldata builder is non-deterministic"
        );
    }
}
