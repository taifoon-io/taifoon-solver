//! Phase 1 — executor-edge integration test for the Mayan Swift Solana path.
//!
//! Validates two assertions the brief calls out:
//!
//!   1. The synthetic fixture decodes through `Intent::from_genome_event`
//!      → `MayanSolanaIntent::from_intent` cleanly. If a future genome
//!      enrichment change drops one of the Solana-specific fields
//!      (`mayan_order_id`, `state_account`, `vault_account`, `swift_program_id`,
//!      `is_solana_source`) this test catches the regression.
//!
//!   2. The classifier (`classify_solana_simulate_result`) recognizes a
//!      synthetic "insufficient lamports" RPC response as GREEN. Reaching
//!      the funding check is what we care about at this phase — it means
//!      the calldata + program ABI matched far enough that the only thing
//!      between us and a successful broadcast is funding the payer, which
//!      is a pre-mainnet-broadcast prerequisite handled by the operator.
//!
//! What we explicitly do NOT do here:
//!   - hit any RPC (the test must run offline)
//!   - touch SolanaBroadcaster::send_fulfill (per the Phase 1 brief)
//!
//! Run with `cargo test -p executor mayan_solana --release`.

use genome_client::{GenomeEvent, Intent};
use protocol_adapters_solana::{
    classify_solana_simulate_result, MayanSolanaIntent, SolanaEstimateOutcome,
};
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

#[test]
fn mayan_solana_fixture_decodes_to_solana_intent() {
    // The fixture goes through the same projection pipeline the live
    // executor uses: GenomeEvent → Intent (genome-client) → MayanSolanaIntent
    // (protocol-adapters-solana). All three layers must accept it.
    let intent = load_intent("mayan_solana.json");
    assert_eq!(intent.protocol, "mayan_swift");
    assert_eq!(intent.is_solana_source, Some(true));

    let solana = MayanSolanaIntent::from_intent(&intent)
        .expect("Mayan Solana fixture must project cleanly");

    // Smoke-check the projection actually copied through the fields the
    // simulator + broadcaster will read. None of these can be empty without
    // breaking later stages.
    assert!(!solana.mayan_order_id_hex.is_empty(), "mayan_order_id_hex empty");
    assert!(!solana.state_account_b58.is_empty(), "state_account_b58 empty");
    assert!(!solana.vault_account_b58.is_empty(), "vault_account_b58 empty");
    assert!(!solana.swift_program_id_b58.is_empty(), "swift_program_id_b58 empty");
    assert!(!solana.trader_pubkey_b58.is_empty(), "trader_pubkey_b58 empty");

    // Sane numeric ranges on the projected scalars.
    assert!(
        solana.compute_units_estimate >= 200_000
            && solana.compute_units_estimate <= 1_400_000,
        "compute_units_estimate {} out of [200_000, 1_400_000]",
        solana.compute_units_estimate
    );
    assert!(
        solana.deadline >= 1_700_000_000,
        "deadline {} looks unreasonably low",
        solana.deadline
    );
}

#[test]
fn mayan_solana_classify_synthetic_insufficient_lamports_is_green() {
    // Brief: "Call SolanaSimulator::classify_solana_simulate_result on a
    // synthetic 'insufficient lamports' RPC response and assert it classifies
    // as ... Green (the fund check is the LAST step before broadcast, so
    // reaching it = the calldata + program ABI matched)."
    //
    // Build the value the validator returns inside `result.value` for an
    // intentionally underfunded payer. The exact JSON shape mirrors what
    // mainnet-beta emits on a fresh keypair (the validator stops before
    // running the program because the payer can't pay the rent/fee).
    let value = serde_json::json!({
        "err": { "InstructionError": [0, "InsufficientFunds"] },
        "logs": [
            "Program 11111111111111111111111111111111 invoke [1]",
            "Transfer: insufficient lamports 0, need 5000",
            "Program 11111111111111111111111111111111 failed: custom program error: 0x1",
        ],
        "unitsConsumed": 0
    });

    let outcome = classify_solana_simulate_result(&value);
    assert!(
        matches!(outcome, SolanaEstimateOutcome::InsufficientLamports(_)),
        "expected InsufficientLamports (GREEN), got {:?}",
        outcome
    );
    assert!(
        outcome.is_green(),
        "InsufficientLamports must classify as GREEN — reaching the funding \
         check means calldata + ABI both validated"
    );
    assert_eq!(outcome.tag(), "insufficient_lamports");
}

#[test]
fn mayan_solana_classify_account_not_found_is_green() {
    // Variant: Helius / mainnet-beta returns a top-level `AccountNotFound`
    // string when the payer keypair has never been funded (no transfer ever
    // landed on it). This is the same "wallet underfunded" shape and must
    // also classify as GREEN.
    let value = serde_json::json!({
        "err": "AccountNotFound",
        "logs": [],
        "unitsConsumed": 0
    });
    let outcome = classify_solana_simulate_result(&value);
    assert!(
        matches!(outcome, SolanaEstimateOutcome::InsufficientLamports(_)),
        "AccountNotFound on a fresh-key payer should be lamport-shape, got {:?}",
        outcome
    );
    assert!(outcome.is_green(), "AccountNotFound must be GREEN");
}

#[test]
fn mayan_solana_classify_program_reject_is_red() {
    // Negative-control: a real program-level reject (Custom error) must NOT
    // be classified as GREEN. The synthetic fixture's state PDA is a
    // throwaway pubkey so when the keychain has the real Solana key, the
    // mainnet validator returns a Custom error from inside the Swift
    // program. That's RED at the integration layer (handled separately by
    // the live `mayan_solana_estimate_test`).
    let value = serde_json::json!({
        "err": { "InstructionError": [0, { "Custom": 6001 }] },
        "logs": [
            "Program BLZRi6frs4X4DNLw56V4EXai1b6QVESN1BhHBTYM9VcY invoke [1]",
            "Program log: Custom program error: 0x1771",
            "Program BLZRi6frs4X4DNLw56V4EXai1b6QVESN1BhHBTYM9VcY failed: custom program error",
        ],
        "unitsConsumed": 4500
    });
    let outcome = classify_solana_simulate_result(&value);
    assert!(
        matches!(outcome, SolanaEstimateOutcome::LogsContainError(_)),
        "Custom program error must be LogsContainError (RED), got {:?}",
        outcome
    );
    assert!(
        !outcome.is_green(),
        "program-level reject must NOT classify as GREEN"
    );
}
