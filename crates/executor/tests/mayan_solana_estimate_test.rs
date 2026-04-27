//! Mayan Swift (Solana) `simulateTransaction` integration test against MAINNET.
//!
//! Loads `tests/fixtures/mayan_solana.json` (a synthetic but real-shaped
//! genome event — `meta.json::source = "synthetic"`), projects it through
//! `Intent::from_genome_event`, and runs the `MayanSolanaEstimateAdapter`
//! against the live Solana mainnet-beta RPC.
//!
//! Synthetic-fixture caveats — there are TWO independent ones for Solana:
//!
//! 1) **Order state isn't real.** The Mayan Swift program loads the on-chain
//!    state PDA, hashes the order, and rejects with a Custom error if the
//!    hash doesn't match. Our fixture's state PDA points at an arbitrary
//!    base58 string that is unlikely to correspond to a real funded account.
//!    That surfaces as `EstimateOutcome::Reverted` with a custom-program-error
//!    log line — ACCEPTABLE for the estimate phase (it proves our instruction
//!    decoded far enough to reach the program logic).
//!
//! 2) **Payer might be unfunded.** When the keychain entry
//!    `mamba-messiah-solana-key` is missing we use the Solana System Program's
//!    pubkey as the placeholder payer. mainnet-beta returns `AccountNotFound`
//!    for any unfunded payer, and the classifier maps that to
//!    `EstimateOutcome::InsufficientLamports` — also GREEN.
//!
//! What does FAIL the test:
//! - `EstimateOutcome::AbiInvalid` (we couldn't even build the calldata —
//!   missing fixture field, base58 decode failure, etc.)
//! - `EstimateOutcome::Reverted` with a message that does NOT contain a
//!   custom-program-error / instructionerror substring — that would mean we
//!   built bytes the BPF loader couldn't parse at all.
//!
//! Run explicitly with:
//!     cargo test -p executor --test mayan_solana_estimate_test -- --ignored --nocapture
//!
//! `SOLANA_RPC_URL` / `SPINNER_API_URL` honored if set.

use alloy::primitives::address;
use executor::{
    load_messiah_solana_pubkey_or_fallback, EstimateAdapter, EstimateOutcome,
    MayanSolanaEstimateAdapter, DEFAULT_SOLANA_RPC,
};
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
async fn mayan_solana_fixture_estimates_clean() {
    let intent = load_intent("mayan_solana.json");
    assert_eq!(intent.protocol, "mayan_swift");
    assert_eq!(
        intent.src_chain, 1399811149,
        "fixture src chain must be Solana mainnet-beta"
    );
    assert!(
        intent.mayan_order_id.is_some(),
        "fixture must carry mayan_order_id"
    );
    assert!(
        intent.swift_program_id.is_some(),
        "fixture must carry swift_program_id"
    );
    assert!(
        intent.state_account.is_some() && intent.vault_account.is_some(),
        "fixture must carry state_account + vault_account"
    );
    assert_eq!(intent.is_solana_source, Some(true));

    // EVM messiah address is unused by the Solana adapter (it's only on the
    // bundle envelope) — pass the canonical placeholder.
    let messiah_evm = address!("742d35Cc6634C0532925a3b844Bc9e7595f0bEb1");
    // Solana payer pubkey — keychain or fallback (system program).
    let solana_pk = load_messiah_solana_pubkey_or_fallback();
    let rpc = std::env::var("SOLANA_RPC_URL")
        .unwrap_or_else(|_| DEFAULT_SOLANA_RPC.to_string());
    let spinner = std::env::var("SPINNER_API_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:30081".to_string());

    let adapter = MayanSolanaEstimateAdapter::new(messiah_evm, solana_pk, rpc, spinner);
    let outcome = adapter.estimate(&intent).await;

    match &outcome {
        EstimateOutcome::OkComputeUnits(u) => {
            assert!(*u > 0, "OkComputeUnits with zero units is suspicious");
            println!("Mayan Solana estimate GREEN — compute_units={}", u);
        }
        EstimateOutcome::InsufficientLamports(msg) => {
            // Either an unfunded payer (calldata-only path) or genuine
            // lamport shortfall — both GREEN. We've reached the validator.
            println!("Mayan Solana estimate GREEN (lamport-shape): {}", msg);
        }
        EstimateOutcome::Reverted(msg) => {
            // Custom program errors are ACCEPTABLE — they prove the
            // instruction decoded far enough to reach Mayan's logic. A
            // bare-revert without that signal would indicate a real bug.
            let lower = msg.to_lowercase();
            let acceptable = lower.contains("custom")
                || lower.contains("instructionerror")
                || lower.contains("programfailedtocomplete")
                || lower.contains("invalidaccountdata")
                || lower.contains("custom program error")
                || lower.contains("program failed");
            if !acceptable {
                panic!(
                    "Mayan Solana reverted with non-program-error message — \
                     likely real instruction-encoding bug: {}",
                    msg
                );
            }
            println!(
                "Mayan Solana synthetic-fixture program reject (expected — order PDA is fake): {}",
                msg
            );
        }
        EstimateOutcome::AbiInvalid(msg) => {
            panic!("Mayan Solana ABI invalid (calldata didn't even build): {}", msg);
        }
        other => panic!("unexpected Solana outcome: {:?}", other),
    }
}
