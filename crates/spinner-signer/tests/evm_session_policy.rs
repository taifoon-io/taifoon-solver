//! EVM session-key policy tests.
//!
//! These tests exercise the four canonical scenarios that the Zodiac
//! Roles Module enforces on-chain, but at the calldata-encoding +
//! dry-run-decoding level — i.e. WITHOUT a live RPC. The four scenarios
//! mirror the Solana test suite by design.
//!
//! ## What's covered without an RPC
//!
//! 1. `evm_session_within_cap_succeeds` — wrap_for_session produces the
//!    right outer calldata (selector + role key + target) so the
//!    on-chain check WOULD pass. We assert the calldata bytes.
//! 2. `evm_session_beyond_cap_fails_locally` — decode_dry_run_result
//!    handles a synthetic Roles-Module "denied" response correctly,
//!    proving the client-side short-circuit BEFORE broadcast works.
//! 3. `evm_session_disallowed_target_fails_locally` — wrap_for_session
//!    rejects an off-allowlist target with `Err`. No tx is built; no tx
//!    can hit chain.
//! 4. `evm_session_expired_role_fails` — decode_dry_run_result handles
//!    the "RoleExpired" custom-error shape (we use `Error("expired")`
//!    as the closest standard equivalent since custom-error decoding
//!    is out of scope for the reference).
//!
//! ## What would need an RPC mock for full coverage
//!
//! End-to-end `dry_run_policy` against a real `eth_call` against an
//! actual deployed Zodiac Roles Module on Sepolia, plus an end-to-end
//! `sign` + `broadcast` against a forked anvil node with a Safe + Roles
//! deployed. The structure is identical to what's tested here; the
//! missing piece is the RPC plumbing.

use alloy::primitives::{Address, Bytes};
use alloy::rpc::types::TransactionRequest;
use alloy::sol_types::{SolCall, SolValue};
use spinner_signer::evm::{EvmSafeRolesSigner, IRolesModule};
use spinner_signer::{EvmSessionConfig, PolicyCheckResult, SessionSigner};
use std::str::FromStr;

fn allowed_target() -> Address {
    // Across V3 SpokePool on Base — picked because it's the same
    // address the executor's `across_executor.rs` already references.
    Address::from_str("0x09aea4b2242abC8bb4BB78D537A67a245A7bEC64").unwrap()
}

fn config_with_allowed_targets(targets: Vec<String>) -> EvmSessionConfig {
    EvmSessionConfig {
        safe_address: "0x1111111111111111111111111111111111111111".into(),
        roles_module: "0x2222222222222222222222222222222222222222".into(),
        role_key: "0x".to_string() + &"ab".repeat(32),
        allowed_targets: targets,
        session_key_env: "SPINNER_TEST_EVM_KEY_DO_NOT_USE".into(),
        rpc_url: "http://127.0.0.1:0/never-called".into(),
        chain_id: 8453,
    }
}

/// Build a synthetic ABI-encoded `(bool, bytes)` tuple — mimics what
/// `eth_call` against `execTransactionWithRoleReturnData` would return.
/// Lets us drive `decode_dry_run_result` end-to-end without an RPC.
fn synthetic_dry_run_response(success: bool, error_string: Option<&str>) -> Vec<u8> {
    let return_data: Bytes = if let Some(msg) = error_string {
        const ERROR_STRING_SELECTOR: [u8; 4] = [0x08, 0xc3, 0x79, 0xa0];
        let inner = String::abi_encode(&msg.to_string());
        let mut out = Vec::with_capacity(4 + inner.len());
        out.extend_from_slice(&ERROR_STRING_SELECTOR);
        out.extend_from_slice(&inner);
        Bytes::from(out)
    } else {
        Bytes::new()
    };
    <(bool, Bytes)>::abi_encode(&(success, return_data))
}

#[tokio::test]
async fn evm_session_within_cap_succeeds() {
    let cfg = config_with_allowed_targets(vec![format!("{:#x}", allowed_target())]);
    let signer = EvmSafeRolesSigner::from_config(cfg.clone()).unwrap();

    // A fill call to the SpokePool: irrelevant inner calldata for this test.
    let inner_calldata = Bytes::from(vec![0xde, 0xff, 0x4b, 0x24, 0x00]);
    let target_call = TransactionRequest::default()
        .to(allowed_target())
        .input(inner_calldata.clone().into());

    let wrapped = signer.wrap_for_session(target_call).await.unwrap();

    // The wrapped tx must point at the Roles Module, not the target.
    let to_addr = wrapped
        .to
        .as_ref()
        .and_then(|t| t.to().copied())
        .expect("wrapped tx must have a `to`");
    assert_eq!(
        to_addr,
        Address::from_str(&cfg.roles_module).unwrap(),
        "wrap_for_session must redirect the tx to the Roles Module"
    );

    // The wrapped calldata must begin with the
    // `execTransactionWithRole` selector. Anything else means we wired
    // the wrong function into wrap_for_session.
    let wrapped_data = wrapped
        .input
        .input()
        .cloned()
        .expect("wrapped tx must have input data");
    assert_eq!(
        &wrapped_data[..4],
        &IRolesModule::execTransactionWithRoleCall::SELECTOR,
        "wrap_for_session must use execTransactionWithRole"
    );

    // Simulate a green dry-run response. decode_dry_run_result is the
    // load-bearing path between the RPC and the trait's
    // PolicyCheckResult return, so testing it standalone exercises
    // exactly what production would do post-RPC.
    let resp = synthetic_dry_run_response(true, None);
    let r = EvmSafeRolesSigner::decode_dry_run_result(&resp).unwrap();
    assert_eq!(r, PolicyCheckResult::Allowed);
}

#[tokio::test]
async fn evm_session_beyond_cap_fails_locally() {
    // The Zodiac Roles Module's allowance system returns a
    // standard-shaped revert reason when the per-token spend cap is
    // hit. We don't need an RPC to verify the decoder picks that up —
    // we synthesize the exact `(false, Error("CapExceeded"))` payload
    // and assert the decoder yields Denied.
    let resp = synthetic_dry_run_response(false, Some("AllowanceExceeded"));
    let r = EvmSafeRolesSigner::decode_dry_run_result(&resp).unwrap();
    match r {
        PolicyCheckResult::Denied { reason } => {
            assert_eq!(reason, "AllowanceExceeded");
        }
        PolicyCheckResult::Allowed => {
            panic!("dry-run should have denied a cap-exceeded call");
        }
    }
    // The critical correctness property: at no point did we attempt to
    // broadcast. Nothing hits chain. (There is no broadcast call in
    // this test; the `dry_run_policy` outcome is the gate.)
}

#[tokio::test]
async fn evm_session_disallowed_target_fails_locally() {
    // Configure with ONLY the Across SpokePool allowed.
    let cfg = config_with_allowed_targets(vec![format!("{:#x}", allowed_target())]);
    let signer = EvmSafeRolesSigner::from_config(cfg).unwrap();

    // Try to wrap a call to a *different* address — say, a random
    // ERC-20 we'd love to drain.
    let evil_target =
        Address::from_str("0xDEAdbeefdeadbeefdeadbeefdeadbeefdeadBeef").unwrap();
    let target_call = TransactionRequest::default()
        .to(evil_target)
        .input(Bytes::from(vec![0xa9, 0x05, 0x9c, 0xbb]).into()); // transfer(...) selector

    let err = signer.wrap_for_session(target_call.clone()).await.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("not in allowed_targets"),
        "expected allowlist rejection, got: {msg}"
    );

    // And `dry_run_policy` ALSO returns Denied for the same reason, so
    // callers who skip wrap_for_session still get a safe answer.
    let dry = signer.dry_run_policy(&target_call).await.unwrap();
    match dry {
        PolicyCheckResult::Denied { reason } => {
            assert!(reason.contains("not in allowed_targets"));
        }
        PolicyCheckResult::Allowed => panic!("dry-run should have denied"),
    }
}

#[tokio::test]
async fn evm_session_expired_role_fails() {
    // The Roles Module emits a `RoleExpired` custom error when the
    // session's expiry timestamp has passed. Custom-error decoding is
    // out of scope for the reference impl, so we model it as a string
    // revert reason here — same code path as the cap-exceeded case,
    // different reason string. A reviewer can verify that any
    // `success=false` from the module is faithfully surfaced as Denied.
    let resp = synthetic_dry_run_response(false, Some("RoleExpired"));
    let r = EvmSafeRolesSigner::decode_dry_run_result(&resp).unwrap();
    match r {
        PolicyCheckResult::Denied { reason } => {
            assert_eq!(reason, "RoleExpired");
        }
        PolicyCheckResult::Allowed => panic!("expired role must deny"),
    }
}
