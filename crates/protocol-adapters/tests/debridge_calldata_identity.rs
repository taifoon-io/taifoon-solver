//! Phase 3 regression locks for the deBridge `fulfillOrder` calldata path.
//!
//! Two properties are pinned here:
//!
//! 1. **Calldata identity (estimate ≡ broadcast).** The pre-flight gas estimate
//!    and the on-chain broadcast must produce byte-identical calldata. This
//!    invariant was broken twice during the spinner integration (commits
//!    `cb39d83`, `0549550`); the live `DeBridgeEstimateAdapter` now delegates
//!    to `DeBridgeAdapter::build_fulfill_order_calldata` (see
//!    `crates/executor/src/evm_estimate.rs::DeBridgeEstimateAdapter::build_estimate_call`),
//!    so the test asserts the canonical builder is stable when invoked twice
//!    against the same `Intent` and that the result decodes back to the same
//!    `Order` shape it encoded.
//!
//! 2. **Order-struct round-trip idempotency.** Decode the calldata produced
//!    from a reference Intent, rebuild a fresh Intent from the decoded fields,
//!    re-encode, and assert byte equality. This locks the multi-slot
//!    bytes-field decoder fix from commit `2a03ed6`: any silent regression
//!    that drops or truncates one of the five DLN authority fields
//!    (`givePatchAuthoritySrc`, `orderAuthorityAddressDst`, `allowedTakerDst`,
//!    `allowedCancelBeneficiarySrc`, `externalCall`) flips this assertion.

use alloy::primitives::{Address, Bytes, U256};
use alloy::sol;
use alloy::sol_types::SolCall;
use genome_client::Intent;
use protocol_adapters::{DeBridgeAdapter, SpinnerClient};

// Mirror the `sol!`-generated types from `protocol_adapters::debridge` so the
// integration test can ABI-decode the calldata it receives. These are not
// re-exported from the crate root and the macro-generated types live inside
// the private `debridge` module — duplicating the ABI here is the canonical
// pattern for external test access.
sol! {
    interface DlnDestination {
        struct Order {
            uint64 makerOrderNonce;
            bytes makerSrc;
            uint256 giveChainId;
            bytes giveTokenAddress;
            uint256 giveAmount;
            uint256 takeChainId;
            bytes takeTokenAddress;
            uint256 takeAmount;
            bytes receiverDst;
            bytes givePatchAuthoritySrc;
            bytes orderAuthorityAddressDst;
            bytes allowedTakerDst;
            bytes allowedCancelBeneficiarySrc;
            bytes externalCall;
        }

        function fulfillOrder(
            Order calldata _order,
            uint256 _fulFillAmount,
            bytes32 _orderId,
            bytes calldata _permit,
            address _unlockAuthority
        ) external payable;
    }
}

/// Build a fully-populated reference Intent that exercises every optional DLN
/// field. The values mirror the shape of `tests/fixtures/debridge.json` but
/// also fill in all five authority fields so the multi-slot bytes-field
/// round-trip is meaningfully exercised (the fixture leaves them blank).
fn reference_intent() -> Intent {
    Intent {
        id: "debridge_dln:0x4f5e6d7c8b9a0123456789abcdef0123456789abcdef0123456789abcdef0123".to_string(),
        protocol: "debridge".to_string(),
        src_chain: 56,
        dst_chain: 10,
        src_token: "0x55d398326f99059fF775485246999027B3197955".to_string(),
        dst_token: "0x94b008aA00579c1307B0EF2c499aD98a8ce58e58".to_string(),
        amount: "100000000000000000000".to_string(),
        depositor: "0x9a8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b".to_string(),
        recipient: "0xabcdef1234567890abcdef1234567890abcdef12".to_string(),
        tx_hash: "0xb2c3d4e5f60a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8091a2b3c4d5e6f70a".to_string(),
        detected_at: 1745928023,
        maker_order_nonce: Some(8472913),
        give_amount: Some("100000000000000000000".to_string()),
        take_amount: Some("99700000".to_string()),
        order_id: Some("0x4f5e6d7c8b9a0123456789abcdef0123456789abcdef0123456789abcdef0123".to_string()),
        // All five DLN authority fields populated with distinct values so any
        // truncation/swap in the decoder shows up in the round-trip assertion.
        dln_give_patch_authority_src: Some("0x1111111111111111111111111111111111111111".to_string()),
        dln_order_authority_address_dst: Some("0x2222222222222222222222222222222222222222".to_string()),
        dln_allowed_taker_dst: Some("0x3333333333333333333333333333333333333333".to_string()),
        dln_allowed_cancel_beneficiary_src: Some("0x4444444444444444444444444444444444444444".to_string()),
        dln_external_call: Some("0xdeadbeefcafebabe".to_string()),
        ..Default::default()
    }
}

fn fresh_adapter() -> DeBridgeAdapter {
    DeBridgeAdapter::new(SpinnerClient::new("http://127.0.0.1:0"))
}

#[test]
fn debridge_fulfill_order_calldata_is_deterministic() {
    let intent = reference_intent();
    let adapter = fresh_adapter();

    let a = adapter
        .build_fulfill_order_calldata(&intent, Address::ZERO)
        .expect("first build");
    let b = adapter
        .build_fulfill_order_calldata(&intent, Address::ZERO)
        .expect("second build");

    assert_eq!(
        a, b,
        "build_fulfill_order_calldata must be deterministic — the estimate \
         path delegates to this exact call (see DeBridgeEstimateAdapter), \
         so a non-deterministic builder would silently desync estimate vs \
         broadcast and reproduce the cb39d83 / 0549550 drift."
    );

    // Selector check: first 4 bytes are keccak256("fulfillOrder(...)") truncated.
    let expected_selector = DlnDestination::fulfillOrderCall::SELECTOR;
    assert_eq!(
        &a[..4],
        &expected_selector,
        "calldata must start with the fulfillOrder selector"
    );
}

#[test]
fn debridge_fulfill_order_calldata_decodes_to_expected_fields() {
    let intent = reference_intent();
    let adapter = fresh_adapter();

    let calldata = adapter
        .build_fulfill_order_calldata(&intent, Address::ZERO)
        .expect("encode");

    let decoded = DlnDestination::fulfillOrderCall::abi_decode(&calldata, true)
        .expect("calldata must decode through the canonical ABI");

    let order = &decoded._order;

    // Scalars
    assert_eq!(order.makerOrderNonce, 8472913);
    assert_eq!(order.giveChainId, U256::from(56));
    assert_eq!(order.takeChainId, U256::from(10));
    assert_eq!(order.giveAmount, U256::from_str_radix("100000000000000000000", 10).unwrap());
    assert_eq!(order.takeAmount, U256::from_str_radix("99700000", 10).unwrap());
    assert_eq!(decoded._fulFillAmount, order.takeAmount);

    // 32-byte orderId
    let want_order_id = hex::decode("4f5e6d7c8b9a0123456789abcdef0123456789abcdef0123456789abcdef0123")
        .unwrap();
    assert_eq!(decoded._orderId.as_slice(), want_order_id.as_slice());

    // permit / unlockAuthority defaults
    assert_eq!(decoded._permit, Bytes::new());
    assert_eq!(decoded._unlockAuthority, Address::ZERO);

    // Bytes fields — the multi-slot decoder regression target.
    assert_eq!(
        order.makerSrc.as_ref(),
        hex::decode("9a8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b").unwrap().as_slice()
    );
    assert_eq!(
        order.giveTokenAddress.as_ref(),
        hex::decode("55d398326f99059fF775485246999027B3197955").unwrap().as_slice()
    );
    assert_eq!(
        order.takeTokenAddress.as_ref(),
        hex::decode("94b008aA00579c1307B0EF2c499aD98a8ce58e58").unwrap().as_slice()
    );
    assert_eq!(
        order.receiverDst.as_ref(),
        hex::decode("abcdef1234567890abcdef1234567890abcdef12").unwrap().as_slice()
    );

    // The five authority/external_call fields each populated distinctly —
    // any swap or truncation in the multi-slot bytes decoder lights up here.
    assert_eq!(
        order.givePatchAuthoritySrc.as_ref(),
        hex::decode("1111111111111111111111111111111111111111").unwrap().as_slice()
    );
    assert_eq!(
        order.orderAuthorityAddressDst.as_ref(),
        hex::decode("2222222222222222222222222222222222222222").unwrap().as_slice()
    );
    assert_eq!(
        order.allowedTakerDst.as_ref(),
        hex::decode("3333333333333333333333333333333333333333").unwrap().as_slice()
    );
    assert_eq!(
        order.allowedCancelBeneficiarySrc.as_ref(),
        hex::decode("4444444444444444444444444444444444444444").unwrap().as_slice()
    );
    assert_eq!(
        order.externalCall.as_ref(),
        hex::decode("deadbeefcafebabe").unwrap().as_slice()
    );
}

#[test]
fn debridge_order_struct_round_trip_is_idempotent() {
    let intent = reference_intent();
    let adapter = fresh_adapter();

    // Pass 1: encode straight from the reference intent.
    let calldata_a = adapter
        .build_fulfill_order_calldata(&intent, Address::ZERO)
        .expect("encode pass 1");

    // ABI-decode back into the canonical Order struct.
    let decoded = DlnDestination::fulfillOrderCall::abi_decode(&calldata_a, true)
        .expect("decode pass 1");
    let order = &decoded._order;

    // Reconstruct an Intent from ONLY what the decoded Order carries —
    // simulating the genome-side OrderCreated → Intent path. If the multi-slot
    // bytes decoder regresses (drops a field, swaps two, leaks padding), the
    // reconstructed intent will diverge from the original and pass-2 calldata
    // will not match calldata_a byte-for-byte.
    fn bytes_to_hex_addr(b: &[u8]) -> String {
        format!("0x{}", hex::encode(b))
    }

    let reconstructed = Intent {
        id: format!("debridge_dln:0x{}", hex::encode(decoded._orderId.as_slice())),
        protocol: "debridge".to_string(),
        src_chain: order.giveChainId.to::<u64>(),
        dst_chain: order.takeChainId.to::<u64>(),
        src_token: bytes_to_hex_addr(order.giveTokenAddress.as_ref()),
        dst_token: bytes_to_hex_addr(order.takeTokenAddress.as_ref()),
        amount: order.giveAmount.to_string(),
        depositor: bytes_to_hex_addr(order.makerSrc.as_ref()),
        recipient: bytes_to_hex_addr(order.receiverDst.as_ref()),
        tx_hash: intent.tx_hash.clone(),
        detected_at: intent.detected_at,
        maker_order_nonce: Some(order.makerOrderNonce),
        give_amount: Some(order.giveAmount.to_string()),
        take_amount: Some(order.takeAmount.to_string()),
        order_id: Some(format!("0x{}", hex::encode(decoded._orderId.as_slice()))),
        dln_give_patch_authority_src: Some(bytes_to_hex_addr(order.givePatchAuthoritySrc.as_ref())),
        dln_order_authority_address_dst: Some(bytes_to_hex_addr(order.orderAuthorityAddressDst.as_ref())),
        dln_allowed_taker_dst: Some(bytes_to_hex_addr(order.allowedTakerDst.as_ref())),
        dln_allowed_cancel_beneficiary_src: Some(bytes_to_hex_addr(order.allowedCancelBeneficiarySrc.as_ref())),
        dln_external_call: Some(bytes_to_hex_addr(order.externalCall.as_ref())),
        ..Default::default()
    };

    // Pass 2: encode from the reconstructed intent.
    let calldata_b = adapter
        .build_fulfill_order_calldata(&reconstructed, Address::ZERO)
        .expect("encode pass 2");

    assert_eq!(
        calldata_a, calldata_b,
        "Order-struct round-trip must be idempotent: encode → decode → \
         reconstruct intent → encode must produce byte-identical calldata. \
         A mismatch indicates the multi-slot bytes-field decoder fix from \
         commit 2a03ed6 has regressed, or one of the five DLN authority \
         fields is being dropped along the path."
    );
}
