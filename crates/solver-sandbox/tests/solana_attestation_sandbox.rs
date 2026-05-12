//! Solana-protocol attestation sandbox.
//!
//! Hackathon-critical regression suite. Every Solana code path —
//! Mayan Swift (Solana-source AND Solana-destination), Mayan Flash,
//! Wormhole NTT, deBridge DLN — produces an `OutcomeRecord` shape that
//! must flow through the donut-adjudicator to a signed `DonutAttestation`
//! attributing the donut to the right Builder.
//!
//! These tests use the **same fixture pattern across all four protocols**:
//!
//!   1. Build an `OutcomeRecord` matching what
//!      `lambda_controller::append_solana_confirmed` emits on that path.
//!   2. Resolve `adapter_id_for_outcome(&record)` and assert it matches
//!      the expected Solana-specific id (no fall-through to "unknown-*").
//!   3. Run `CanonicalAdjudicator::attest(...)` against an AdapterRegistry
//!      seeded with all four Solana Builders.
//!   4. Assert: the attestation's `creator_addr` is the expected Builder,
//!      the 49 bps × 70/20/10 math sums to within 1e-9, and `verify()`
//!      reproduces the signature.
//!
//! Two additional tests cross-cut the per-protocol checks:
//!
//!   * `hash_chain_links_across_protocols` — one Spinner fills all four
//!     protocols in sequence; each attestation's `prev_hash` must equal
//!     `sha256(canonical_full(previous))`.
//!   * `multi_spinner_ledgers_stay_isolated` — two Spinners run different
//!     protocols; neither Spinner's hash chain references the other's
//!     attestations.
//!
//! Why this pattern matters: Mayan-Solana has **no on-chain donut
//! enforcement** (the protocol pays the solver EOA directly on Solana).
//! The off-chain attestation IS the audit trail. If any of these
//! per-protocol routings break, the donut for that path is silently lost.

use alloy::primitives::Address;
use alloy::signers::local::PrivateKeySigner;
use chrono::Utc;
use donut_adjudicator::{
    adapter_id_for_outcome, hash_for_chain, AdapterRegistry, CanonicalAdjudicator,
    DonutAttestation, FeeSplitAdjudicator, CREATOR_FRACTION, DONUT_BPS, ECOSYSTEM_FRACTION,
    REVIEWER_FRACTION, SOLANA_DST_DEBRIDGE, SOLANA_DST_WORMHOLE, ZERO_HASH,
};
use executor::OutcomeRecord;

// ── fixtures ─────────────────────────────────────────────────────────────────

/// Build a synthetic Solana-fill OutcomeRecord using the exact shape that
/// `crates/executor/src/lambda_controller.rs::append_solana_confirmed`
/// writes on a confirmed Solana broadcast.
fn solana_fill(
    intent_id: &str,
    protocol: &str,
    src_chain: u64,
    dst_chain: u64,
    profit_usd: f64,
) -> OutcomeRecord {
    OutcomeRecord {
        ts: Utc::now(),
        intent_id: intent_id.into(),
        protocol: protocol.into(),
        src_chain,
        dst_chain,
        decision: "executed".into(),
        tx_hash: Some(format!(
            "{}solana-tx-{}",
            intent_id,
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        )),
        predicted_gas: Some(250_000),
        gas_used: Some(250_000),
        effective_gas_price_wei: None,
        predicted_profit_usd: Some(profit_usd),
        actual_profit_usd: Some(profit_usd),
        skip_reason: None,
        error: None,
        solver_id: None,
        claim_tx_hash: None,
        claim_fee_usd: None,
    }
}

/// Two distinct dev signers — used by the multi-Spinner test to verify
/// ledger isolation. **Never used outside the test process.**
fn signer_a() -> PrivateKeySigner {
    "0x4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318"
        .parse()
        .unwrap()
}

fn signer_b() -> PrivateKeySigner {
    "0xdf57089febbacf7ba0bc227dafbffa9fc08a93fdc67e1e3c5ac6739c1bff21bd"
        .parse()
        .unwrap()
}

/// Each Solana Builder gets a distinct address so test assertions can
/// confirm the donut routes to the right one. None of these are real
/// addresses — they're sentinel patterns.
fn ecosystem_addr() -> Address {
    "0x000000000000000000000000000000000000eeee".parse().unwrap()
}

fn mayan_swift_solana_builder() -> Address {
    "0x111111111111111111111111111111111111aaaa".parse().unwrap()
}

fn mayan_flash_solana_builder() -> Address {
    "0x222222222222222222222222222222222222bbbb".parse().unwrap()
}

fn wormhole_ntt_solana_builder() -> Address {
    "0x333333333333333333333333333333333333cccc".parse().unwrap()
}

fn debridge_dln_solana_builder() -> Address {
    "0x444444444444444444444444444444444444dddd".parse().unwrap()
}

fn reviewer_set() -> Vec<Address> {
    vec![
        "0x000000000000000000000000000000000000aaaa".parse().unwrap(),
        "0x000000000000000000000000000000000000bbbb".parse().unwrap(),
    ]
}

/// Registry seeded with all four Solana adapter ids → distinct Builders +
/// the same reviewer set. Mirrors what a Spinner OS would load at boot.
fn registry_with_all_solana_adapters() -> AdapterRegistry {
    AdapterRegistry::new(ecosystem_addr())
        .with_adapter(
            "mayan-solana-swift-v1",
            mayan_swift_solana_builder(),
            reviewer_set(),
        )
        .with_adapter(
            "mayan-flash-solana-v1",
            mayan_flash_solana_builder(),
            reviewer_set(),
        )
        .with_adapter(
            "wormhole-ntt-solana-v1",
            wormhole_ntt_solana_builder(),
            reviewer_set(),
        )
        .with_adapter(
            "debridge-dln-solana-v1",
            debridge_dln_solana_builder(),
            reviewer_set(),
        )
}

/// Reusable post-attestation assertions. Every Solana protocol must
/// satisfy the same invariants.
fn assert_math_invariants(att: &DonutAttestation, expected_profit: f64) {
    let positive = expected_profit.max(0.0);
    let expected_donut = positive * DONUT_BPS;
    assert!(
        (att.donut_take_usd - expected_donut).abs() < 1e-9,
        "donut_take_usd: got {}, expected {} (profit={})",
        att.donut_take_usd,
        expected_donut,
        expected_profit
    );

    let sum = att.creator_share_usd + att.reviewer_share_usd + att.ecosystem_share_usd;
    assert!(
        (sum - att.donut_take_usd).abs() < 1e-9,
        "shares {} don't sum to donut_take {}",
        sum,
        att.donut_take_usd
    );

    let expected_creator = att.donut_take_usd * CREATOR_FRACTION;
    let expected_reviewer = att.donut_take_usd * REVIEWER_FRACTION;
    let expected_ecosystem = att.donut_take_usd * ECOSYSTEM_FRACTION;
    assert!(
        (att.creator_share_usd - expected_creator).abs() < 1e-9,
        "creator share off: {} vs {}",
        att.creator_share_usd,
        expected_creator
    );
    assert!(
        (att.reviewer_share_usd - expected_reviewer).abs() < 1e-9,
        "reviewer share off: {} vs {}",
        att.reviewer_share_usd,
        expected_reviewer
    );
    assert!(
        (att.ecosystem_share_usd - expected_ecosystem).abs() < 1e-9,
        "ecosystem share off: {} vs {}",
        att.ecosystem_share_usd,
        expected_ecosystem
    );

    let expected_keeps = expected_profit - att.donut_take_usd;
    assert!(
        (att.spinner_keeps_usd - expected_keeps).abs() < 1e-9,
        "spinner_keeps off: {} vs {}",
        att.spinner_keeps_usd,
        expected_keeps
    );
}

// ── per-protocol round-trips ─────────────────────────────────────────────────

/// Mayan Swift **Solana-source → EVM-destination** (the production-most
/// flow from `tests/fixtures/mayan_solana.json`).
#[tokio::test]
async fn mayan_swift_solana_source_routes_to_solana_builder() {
    let adj = CanonicalAdjudicator;
    let signer = signer_a();
    let reg = registry_with_all_solana_adapters();

    // src=1_399_811_149 (Solana), dst=1 (Ethereum) — exact shape of the
    // on-disk fixture.
    let fill = solana_fill(
        "mayan-fixture-1",
        "mayan_swift",
        SOLANA_DST_WORMHOLE,
        1,
        0.42,
    );

    let resolved = adapter_id_for_outcome(&fill);
    assert_eq!(resolved, "mayan-solana-swift-v1");

    let att = adj.attest(&fill, &reg, &signer, ZERO_HASH).await.unwrap();
    assert_eq!(att.adapter_id, "mayan-solana-swift-v1");
    assert_eq!(att.creator_addr, mayan_swift_solana_builder());
    assert_math_invariants(&att, 0.42);
    adj.verify(&att).unwrap();
}

/// Mayan Swift **EVM-source → Solana-destination** (the auction-fill flow).
#[tokio::test]
async fn mayan_swift_solana_dest_routes_to_solana_builder() {
    let adj = CanonicalAdjudicator;
    let signer = signer_a();
    let reg = registry_with_all_solana_adapters();

    let fill = solana_fill(
        "mayan-dest-1",
        "mayan_swift",
        8453, // Base source
        SOLANA_DST_WORMHOLE,
        0.55,
    );

    let resolved = adapter_id_for_outcome(&fill);
    assert_eq!(resolved, "mayan-solana-swift-v1");

    let att = adj.attest(&fill, &reg, &signer, ZERO_HASH).await.unwrap();
    assert_eq!(att.creator_addr, mayan_swift_solana_builder());
    assert_math_invariants(&att, 0.55);
    adj.verify(&att).unwrap();
}

/// Mayan **Flash** (separate Anchor program from Swift) must route to its
/// own Builder, not collapse into the Swift adapter.
#[tokio::test]
async fn mayan_flash_solana_routes_to_flash_builder() {
    let adj = CanonicalAdjudicator;
    let signer = signer_a();
    let reg = registry_with_all_solana_adapters();

    let fill = solana_fill(
        "flash-1",
        "mayan_flash",
        SOLANA_DST_WORMHOLE,
        8453,
        0.75,
    );

    let resolved = adapter_id_for_outcome(&fill);
    assert_eq!(resolved, "mayan-flash-solana-v1");
    assert_ne!(resolved, "mayan-solana-swift-v1");

    let att = adj.attest(&fill, &reg, &signer, ZERO_HASH).await.unwrap();
    assert_eq!(att.creator_addr, mayan_flash_solana_builder());
    assert_ne!(att.creator_addr, mayan_swift_solana_builder());
    assert_math_invariants(&att, 0.75);
    adj.verify(&att).unwrap();
}

/// Wormhole NTT (EVM→Solana via Guardian VAA relay). Pre-Solana-alignment
/// patch, this fell through to "unknown-wormhole_ntt-…" and burned the
/// Builder's donut to ecosystem.
#[tokio::test]
async fn wormhole_ntt_solana_routes_to_ntt_builder() {
    let adj = CanonicalAdjudicator;
    let signer = signer_a();
    let reg = registry_with_all_solana_adapters();

    let fill = solana_fill(
        "ntt-1",
        "wormhole_ntt",
        8453,
        SOLANA_DST_WORMHOLE,
        0.30,
    );

    let resolved = adapter_id_for_outcome(&fill);
    assert_eq!(resolved, "wormhole-ntt-solana-v1");

    let att = adj.attest(&fill, &reg, &signer, ZERO_HASH).await.unwrap();
    assert_eq!(att.creator_addr, wormhole_ntt_solana_builder());
    assert_math_invariants(&att, 0.30);
    adj.verify(&att).unwrap();
}

/// deBridge DLN **Solana-destination** (chain id `100_000_001`). Must
/// route to the Solana DLN Builder, not the EVM DLN Builder.
#[tokio::test]
async fn debridge_dln_solana_routes_to_dln_solana_builder() {
    let adj = CanonicalAdjudicator;
    let signer = signer_a();
    let reg = registry_with_all_solana_adapters();

    let fill = solana_fill(
        "dln-sol-1",
        "debridge_dln",
        1, // Ethereum source
        SOLANA_DST_DEBRIDGE,
        0.20,
    );

    let resolved = adapter_id_for_outcome(&fill);
    assert_eq!(resolved, "debridge-dln-solana-v1");

    let att = adj.attest(&fill, &reg, &signer, ZERO_HASH).await.unwrap();
    assert_eq!(att.creator_addr, debridge_dln_solana_builder());
    assert_math_invariants(&att, 0.20);
    adj.verify(&att).unwrap();
}

/// Losing fill across every Solana protocol must produce zero donut and a
/// negative `spinner_keeps_usd`. Verifies the attestation still signs and
/// verifies — Spinners absorb the loss without being levied.
#[tokio::test]
async fn losing_solana_fills_emit_zero_donut_per_protocol() {
    let adj = CanonicalAdjudicator;
    let signer = signer_a();
    let reg = registry_with_all_solana_adapters();

    let cases = [
        ("mayan_swift", SOLANA_DST_WORMHOLE, 1u64),
        ("mayan_flash", SOLANA_DST_WORMHOLE, 8453u64),
        ("wormhole_ntt", 8453, SOLANA_DST_WORMHOLE),
        ("debridge_dln", 1, SOLANA_DST_DEBRIDGE),
    ];

    for (protocol, src_chain, dst_chain) in cases {
        let fill = solana_fill(
            &format!("losing-{}-{}", protocol, dst_chain),
            protocol,
            src_chain,
            dst_chain,
            -0.50,
        );
        let att = adj.attest(&fill, &reg, &signer, ZERO_HASH).await.unwrap();
        assert_eq!(att.donut_take_usd, 0.0, "protocol={} donut should be 0 on loss", protocol);
        assert_eq!(att.creator_share_usd, 0.0);
        assert_eq!(att.reviewer_share_usd, 0.0);
        assert_eq!(att.ecosystem_share_usd, 0.0);
        assert!(
            (att.spinner_keeps_usd - (-0.50)).abs() < 1e-12,
            "protocol={} spinner_keeps_usd off: {}",
            protocol,
            att.spinner_keeps_usd
        );
        adj.verify(&att).unwrap();
    }
}

// ── cross-cutting tests ──────────────────────────────────────────────────────

/// One Spinner fills all four Solana protocols in sequence. Each
/// attestation's `prev_hash` must equal the previous attestation's full
/// canonical sha256. Tampering with any record in the middle would break
/// the chain at the next link.
#[tokio::test]
async fn hash_chain_links_across_protocols() {
    let adj = CanonicalAdjudicator;
    let signer = signer_a();
    let reg = registry_with_all_solana_adapters();

    let fills = [
        solana_fill("chain-1", "mayan_swift", SOLANA_DST_WORMHOLE, 1, 0.10),
        solana_fill("chain-2", "mayan_flash", SOLANA_DST_WORMHOLE, 8453, 0.20),
        solana_fill("chain-3", "wormhole_ntt", 8453, SOLANA_DST_WORMHOLE, 0.30),
        solana_fill("chain-4", "debridge_dln", 1, SOLANA_DST_DEBRIDGE, 0.40),
    ];

    let mut prev = ZERO_HASH.to_string();
    let mut atts: Vec<DonutAttestation> = Vec::new();
    for fill in &fills {
        let att = adj.attest(fill, &reg, &signer, &prev).await.unwrap();
        assert_eq!(att.prev_hash, prev);
        adj.verify(&att).unwrap();
        prev = hash_for_chain(&att).unwrap();
        atts.push(att);
    }

    // Sanity check — the chain has four distinct hashes, none are ZERO.
    for (i, att) in atts.iter().enumerate() {
        if i == 0 {
            assert_eq!(att.prev_hash, ZERO_HASH);
        } else {
            assert_ne!(att.prev_hash, ZERO_HASH);
            assert_eq!(att.prev_hash, hash_for_chain(&atts[i - 1]).unwrap());
        }
    }
}

/// Two Spinners (signer_a, signer_b) each fill two Solana protocols.
/// Their `spinner_id`s, addresses, and hash chains must be independent —
/// neither Spinner's chain references the other's hashes.
#[tokio::test]
async fn multi_spinner_ledgers_stay_isolated() {
    let adj = CanonicalAdjudicator;
    let reg = registry_with_all_solana_adapters();
    let a = signer_a();
    let b = signer_b();

    // Spinner A: Mayan Swift Solana-source, then Wormhole NTT.
    let a_fills = [
        solana_fill("a-1", "mayan_swift", SOLANA_DST_WORMHOLE, 1, 0.15),
        solana_fill("a-2", "wormhole_ntt", 8453, SOLANA_DST_WORMHOLE, 0.25),
    ];
    let mut a_prev = ZERO_HASH.to_string();
    let mut a_atts = Vec::new();
    for fill in &a_fills {
        let att = adj.attest(fill, &reg, &a, &a_prev).await.unwrap();
        a_prev = hash_for_chain(&att).unwrap();
        a_atts.push(att);
    }

    // Spinner B: Mayan Flash, then deBridge DLN Solana.
    let b_fills = [
        solana_fill("b-1", "mayan_flash", SOLANA_DST_WORMHOLE, 8453, 0.35),
        solana_fill("b-2", "debridge_dln", 1, SOLANA_DST_DEBRIDGE, 0.45),
    ];
    let mut b_prev = ZERO_HASH.to_string();
    let mut b_atts = Vec::new();
    for fill in &b_fills {
        let att = adj.attest(fill, &reg, &b, &b_prev).await.unwrap();
        b_prev = hash_for_chain(&att).unwrap();
        b_atts.push(att);
    }

    // The two Spinners must have different addresses and spinner_ids.
    assert_ne!(a_atts[0].spinner_addr, b_atts[0].spinner_addr);
    assert_ne!(a_atts[0].spinner_id, b_atts[0].spinner_id);

    // Neither Spinner's hash chain references the other's hashes.
    let a_hashes: Vec<String> = a_atts.iter().map(|x| hash_for_chain(x).unwrap()).collect();
    let b_prev_hashes: Vec<String> = b_atts.iter().map(|x| x.prev_hash.clone()).collect();
    for ah in &a_hashes {
        assert!(
            !b_prev_hashes.contains(ah),
            "Spinner B's chain referenced Spinner A's hash {}",
            ah
        );
    }

    // Every attestation verifies independently.
    for att in a_atts.iter().chain(b_atts.iter()) {
        adj.verify(att).unwrap();
    }
}

/// Negative case: a fill that goes through an *unregistered* Solana
/// adapter (e.g. a brand-new protocol) must route the Builder's share
/// to the ecosystem treasury, not silently to the Spinner.
#[tokio::test]
async fn unregistered_solana_adapter_routes_to_ecosystem() {
    let adj = CanonicalAdjudicator;
    let signer = signer_a();
    // Empty registry — no Solana adapters known.
    let reg = AdapterRegistry::new(ecosystem_addr());

    let fill = solana_fill(
        "unknown-sol-1",
        "mayan_swift",
        SOLANA_DST_WORMHOLE,
        1,
        1.00,
    );
    let att = adj.attest(&fill, &reg, &signer, ZERO_HASH).await.unwrap();

    // adapter_id resolution still succeeds — it's the registry lookup
    // that fails-closed.
    assert_eq!(att.adapter_id, "mayan-solana-swift-v1");
    assert_eq!(att.creator_addr, ecosystem_addr());
    assert_eq!(att.reviewer_addrs, vec![ecosystem_addr()]);
    adj.verify(&att).unwrap();
}
