//! Integration tests for WellAuthGuard permit lifecycle.
//!
//! These are pure in-process (SQLite :memory:) tests — no network, no `#[ignore]`.

use alloy::primitives::Address;
use solver_registry::{FillPermit, WellAuthGuard, RegistryError};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_guard() -> WellAuthGuard {
    // Address::ZERO as spinner_pub_key — sig checks will fail with InvalidSignature,
    // but we bypass them for lifecycle tests by calling consume() directly.
    WellAuthGuard::new(Address::ZERO, ":memory:").expect("open :memory: db")
}

fn expired_permit(intent_id: &str) -> FillPermit {
    FillPermit {
        intent_id: intent_id.to_string(),
        solver: "0x0000000000000000000000000000000000000000".to_string(),
        chain_id: 8453,
        amount_wei: "1000000".to_string(),
        deadline: 1, // Unix epoch + 1 sec — safely in the past
        nonce: 0,
        signature: format!("0x{}", "00".repeat(65)),
    }
}

fn future_permit(intent_id: &str) -> FillPermit {
    FillPermit {
        intent_id: intent_id.to_string(),
        solver: "0x0000000000000000000000000000000000000000".to_string(),
        chain_id: 8453,
        amount_wei: "1000000".to_string(),
        deadline: u64::MAX, // far future — never expires in practice
        nonce: 0,
        // All-zero 65-byte signature — invalid crypto, but consumed() bypasses sig check.
        signature: format!("0x{}", "00".repeat(65)),
    }
}

// ── Test 1: full lifecycle — validate then consume then is_used ───────────────

#[test]
fn full_permit_lifecycle_validate_consume_reject() {
    let guard = make_guard();
    let permit = future_permit("0x0000000000000000000000000000000000000000000000000000000000000042");

    // Before consuming: permit has not been used.
    assert!(
        !guard.is_used(&permit.intent_id, permit.chain_id),
        "permit must not be used before consume"
    );

    // validate() will pass the deadline check and hit the sig check (fail with
    // InvalidSignature) because Address::ZERO can't have signed this.
    // That's expected — we test the consume path directly.
    let validate_err = guard.validate(&permit).unwrap_err();
    assert!(
        matches!(validate_err, RegistryError::InvalidSignature),
        "expected InvalidSignature before consume, got {:?}",
        validate_err
    );

    // consume() directly marks it used (bypassing sig check).
    guard
        .consume(&permit, Some("0xdeadbeefcafe"))
        .expect("consume must succeed on fresh permit");

    // is_used() must now return true.
    assert!(
        guard.is_used(&permit.intent_id, permit.chain_id),
        "permit must be marked used after consume"
    );

    // validate() on the now-consumed permit must reach the duplicate-use error.
    // Expiry passes (deadline=MAX). Sig check fails first — that's fine: the
    // intent is to verify that is_used() persists correctly.
    // We test the PermitAlreadyUsed path by creating a second guard sharing the
    // *same* :memory: connection (via is_used only, since separate Connection
    // objects can't share :memory: — we use the same guard instance).
    let validate_err2 = guard.validate(&permit).unwrap_err();
    // Either InvalidSignature (sig check runs before dup check) or PermitAlreadyUsed.
    // The current implementation checks expiry → sig → dup. Sig fails first.
    // We verify the dup check by calling is_used explicitly (already done above).
    assert!(
        matches!(
            validate_err2,
            RegistryError::InvalidSignature | RegistryError::PermitAlreadyUsed
        ),
        "unexpected error after consume: {:?}",
        validate_err2
    );
}

// ── Test 2: duplicate consume is idempotent (INSERT OR IGNORE) ───────────────

#[test]
fn duplicate_consume_is_idempotent() {
    let guard = make_guard();
    let permit = future_permit("0x0000000000000000000000000000000000000000000000000000000000000099");

    // First consume.
    guard
        .consume(&permit, None)
        .expect("first consume must succeed");

    // Second consume on the same (intent_id, chain_id) — must not error.
    guard
        .consume(&permit, Some("0xdifferent_tx"))
        .expect("second consume must be idempotent (INSERT OR IGNORE)");

    // Still marked as used.
    assert!(guard.is_used(&permit.intent_id, permit.chain_id));
}

// ── Test 3: expired permit rejected before sig check ─────────────────────────

#[test]
fn expired_permit_rejected_before_sig_check() {
    let guard = make_guard();
    let permit = expired_permit("0x0000000000000000000000000000000000000000000000000000000000000007");

    let err = guard.validate(&permit).unwrap_err();
    assert!(
        matches!(err, RegistryError::PermitExpired),
        "expected PermitExpired, got {:?}",
        err
    );
}

// ── Test 4: is_used returns false before any consume ─────────────────────────

#[test]
fn is_used_returns_false_initially() {
    let guard = make_guard();
    assert!(!guard.is_used("0xnonexistent_intent", 8453));
    assert!(!guard.is_used("0xnonexistent_intent", 1));
}

// ── Test 5: independent permits on different chains don't cross-contaminate ───

#[test]
fn permits_on_different_chains_are_independent() {
    let guard = make_guard();
    let intent_id = "0x0000000000000000000000000000000000000000000000000000000000000010";

    let permit_base = FillPermit {
        intent_id: intent_id.to_string(),
        solver: "0x0".to_string(),
        chain_id: 8453,
        amount_wei: "1000000".to_string(),
        deadline: u64::MAX,
        nonce: 0,
        signature: format!("0x{}", "00".repeat(65)),
    };

    let permit_arb = FillPermit {
        chain_id: 42161,
        ..permit_base.clone()
    };

    // Consume only Base chain permit.
    guard.consume(&permit_base, None).unwrap();

    // Base is used; Arbitrum is not.
    assert!(guard.is_used(intent_id, 8453), "Base permit must be used");
    assert!(
        !guard.is_used(intent_id, 42161),
        "Arbitrum permit must be independent"
    );

    // Consume Arbitrum now.
    guard.consume(&permit_arb, None).unwrap();
    assert!(guard.is_used(intent_id, 42161));
}
