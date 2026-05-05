//! LiFi child-intent projection test (Phase 4).
//!
//! For each `tests/fixtures/lifi*.json` fixture, walk the projection that the
//! solver-main loop performs:
//!
//!   1. Parse the JSON as a `GenomeEvent`, then `Intent::from_genome_event`.
//!   2. Resolve the underlying bridge via `LiFiMetaRouter::resolve_bridge`
//!      (prefers `bridge`, falls back to `tool`).
//!   3. Project to a child intent via `LiFiMetaRouter::project_to_child`.
//!   4. Assert the rewritten `protocol` matches the canonical adapter name
//!      (`across_v3` / `debridge` / `mayan_swift`) so the downstream
//!      `lambda_controller` dispatch picks the right adapter.
//!   5. Assert chain/token/amount/depositor/recipient pass through unchanged
//!      — the meta-router must not mutate the economics, only the routing tag.
//!
//! Pure unit-level: no network. Runs in the default `cargo test` set.

use executor::LiFiMetaRouter;
use genome_client::{GenomeEvent, Intent};
use std::path::PathBuf;

struct FixtureExpectation {
    fixture: &'static str,
    /// Underlying-bridge slug (lowercased) that resolve_bridge must return.
    expected_bridge: &'static str,
    /// Canonical child protocol after project_to_child.
    expected_child_protocol: &'static str,
}

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

fn cases() -> Vec<FixtureExpectation> {
    vec![
        // The original synthetic d874041 fixture — bridge=across (Ethereum→Arbitrum, USDC).
        FixtureExpectation {
            fixture: "lifi.json",
            expected_bridge: "across",
            expected_child_protocol: "across_v3",
        },
        // Phase 4 corpus: one fixture per supported underlying.
        FixtureExpectation {
            fixture: "lifi_via_across_base.json",
            expected_bridge: "across",
            expected_child_protocol: "across_v3",
        },
        FixtureExpectation {
            fixture: "lifi_via_debridge.json",
            expected_bridge: "debridge",
            expected_child_protocol: "debridge",
        },
        FixtureExpectation {
            fixture: "lifi_via_mayan.json",
            expected_bridge: "mayan",
            expected_child_protocol: "mayan_swift",
        },
    ]
}

#[test]
fn lifi_fixtures_project_to_canonical_child_protocol() {
    for case in cases() {
        let intent = load_intent(case.fixture);

        // Sanity: every LiFi fixture must come through tagged `lifi` so the
        // meta-router branch in solver-main even fires.
        assert_eq!(
            intent.protocol, "lifi",
            "{}: parent protocol must be `lifi`, got {}",
            case.fixture, intent.protocol
        );

        // Resolution.
        let resolved = LiFiMetaRouter::resolve_bridge(&intent).unwrap_or_else(|| {
            panic!(
                "{}: resolve_bridge returned None — bridge/tool field missing from fixture",
                case.fixture
            )
        });
        assert_eq!(
            resolved, case.expected_bridge,
            "{}: resolved bridge mismatch",
            case.fixture
        );

        // Projection.
        let child = LiFiMetaRouter::project_to_child(&intent, &resolved);

        assert_eq!(
            child.protocol, case.expected_child_protocol,
            "{}: child.protocol must be canonical adapter name",
            case.fixture
        );

        // Chain/token/amount/depositor/recipient inherited unchanged.
        assert_eq!(child.src_chain, intent.src_chain, "{}: src_chain mutated", case.fixture);
        assert_eq!(child.dst_chain, intent.dst_chain, "{}: dst_chain mutated", case.fixture);
        assert_eq!(child.src_token, intent.src_token, "{}: src_token mutated", case.fixture);
        assert_eq!(child.dst_token, intent.dst_token, "{}: dst_token mutated", case.fixture);
        assert_eq!(child.amount, intent.amount, "{}: amount mutated", case.fixture);
        assert_eq!(child.depositor, intent.depositor, "{}: depositor mutated", case.fixture);
        assert_eq!(child.recipient, intent.recipient, "{}: recipient mutated", case.fixture);
        assert_eq!(child.output_amount, intent.output_amount, "{}: output_amount mutated", case.fixture);

        // Underlying-bridge-specific identifiers must propagate so the child
        // adapter has everything it needs to build calldata.
        match case.expected_child_protocol {
            "across_v3" => {
                assert_eq!(
                    child.deposit_id, intent.deposit_id,
                    "{}: across child must inherit deposit_id",
                    case.fixture
                );
            }
            "debridge" => {
                assert_eq!(
                    child.maker_order_nonce, intent.maker_order_nonce,
                    "{}: debridge child must inherit maker_order_nonce",
                    case.fixture
                );
                assert_eq!(
                    child.order_id, intent.order_id,
                    "{}: debridge child must inherit order_id",
                    case.fixture
                );
            }
            "mayan_swift" => {
                assert_eq!(
                    child.mayan_order_id, intent.mayan_order_id,
                    "{}: mayan child must inherit mayan_order_id",
                    case.fixture
                );
            }
            _ => {}
        }

        // Child id must record the meta-routing hop so logs make the indirection
        // visible (lifi→across:0x… etc.).
        let prefix = format!("lifi→{}:", resolved);
        assert!(
            child.id.starts_with(&prefix),
            "{}: child.id should start with `{}` for traceability, got {}",
            case.fixture, prefix, child.id
        );
    }
}

/// `resolve_bridge` falls back to `tool` only when `bridge` is absent — the
/// LiFi-via-deBridge fixture sets `bridge=debridge` and `tool=dln`, exercising
/// the prefer-bridge-over-tool path. Add a focused assertion to lock the policy
/// in case a future fixture drops `bridge` and relies on `tool`.
#[test]
fn lifi_via_debridge_prefers_bridge_field_over_tool() {
    let intent = load_intent("lifi_via_debridge.json");
    assert_eq!(intent.bridge.as_deref(), Some("debridge"));
    assert_eq!(intent.tool.as_deref(), Some("dln"));
    let resolved = LiFiMetaRouter::resolve_bridge(&intent);
    assert_eq!(resolved.as_deref(), Some("debridge"));
}
