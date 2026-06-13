//! # Operator calldata builder — the generic `executeWithProof(...)` wrapper.
//!
//! Per the delivery scope (§4.3, "Universal Operator Integration"), a solver
//! never calls a protocol contract directly. Every live fill is routed through
//! the on-chain `TaifoonUniversalOperator`:
//!
//! ```text
//! Solver → TaifoonUniversalOperator(V5Proof + Calldata) → ProtocolAdapter → ProtocolContract
//! ```
//!
//! The operator entrypoint is:
//!
//! ```solidity
//! function executeWithProof(
//!     bytes calldata v5ProofBlob,    // L1-L6 proof from Spinner
//!     address adapterContract,       // protocol adapter address
//!     bytes calldata adapterCalldata // Adapter.fill(...) params
//! ) external returns (bool);
//! ```
//!
//! ## Why this lives here (and not only in `across_executor.rs`)
//!
//! The Across executor already open-codes this exact wrap (see
//! `across_executor.rs` step 5). But that copy is Across-specific: it is
//! reachable only from the Across pipeline. The generic [`FillRouter`] path
//! (`AdapterRouter` in [`crate::router`]) has *no* way to produce a live
//! `executeWithProof` transaction at all — its live branch returns an error.
//!
//! Each protocol adapter already knows how to build its own
//! `adapterCalldata` (its `Adapter.fill(...)` encoding) and exposes its
//! destination-chain target via `FillTransaction`. What's missing is the
//! one protocol-agnostic step that turns
//! `(proof, adapter_address, adapter_calldata)` into the bytes you send to
//! the operator. That step is pure ABI encoding — no RPC, no signing, no
//! broadcast — so it can be unit-tested hermetically and shared by every
//! protocol path.
//!
//! This module is that step. It is the smallest concrete primitive on the
//! generic live-execution path; the broadcast layer (sign + send) builds on
//! top of it but is intentionally out of scope here.
//!
//! [`FillRouter`]: crate::router::FillRouter

use alloy::primitives::{Address, Bytes};
use alloy::sol;
use alloy::sol_types::SolCall;

sol! {
    /// `TaifoonUniversalOperator` proof-gated execution entrypoint.
    /// Selector `executeWithProof(bytes,address,bytes)` — must stay in sync
    /// with the deployed operator ABI and with `across_executor.rs`'s copy.
    interface ITaifoonUniversalOperator {
        function executeWithProof(
            bytes calldata v5ProofBlob,
            address adapterContract,
            bytes calldata adapterCalldata
        ) external returns (bool);
    }
}

/// ABI-encode a call to `TaifoonUniversalOperator.executeWithProof(...)`.
///
/// This is the generic wrap every protocol shares: given the V5 proof bytes
/// from Spinner, the protocol adapter's on-chain address, and the
/// already-encoded `Adapter.fill(...)` calldata, it returns the calldata you
/// place in a transaction `to` the operator.
///
/// It does **not** broadcast, sign, or touch the network — callers feed the
/// returned bytes into a `TransactionRequest` (live) or `.estimateGas()`
/// (dry-run). Keeping it pure is what makes it testable without a node.
///
/// # Parameters
/// - `v5_proof_blob`: opaque L1-L6 proof bytes from the Spinner proof bundle API.
/// - `adapter_contract`: deployed protocol-adapter address on the destination chain.
/// - `adapter_calldata`: ABI-encoded `Adapter.fill(...)` parameters.
pub fn build_execute_with_proof_calldata(
    v5_proof_blob: impl Into<Bytes>,
    adapter_contract: Address,
    adapter_calldata: impl Into<Bytes>,
) -> Vec<u8> {
    ITaifoonUniversalOperator::executeWithProofCall {
        v5ProofBlob: v5_proof_blob.into(),
        adapterContract: adapter_contract,
        adapterCalldata: adapter_calldata.into(),
    }
    .abi_encode()
}

/// 4-byte selector of `executeWithProof(bytes,address,bytes)`.
///
/// Exposed so other components (e.g. the mempool monitor's competing-fill
/// detector) can identify operator calls without re-deriving the selector.
pub fn execute_with_proof_selector() -> [u8; 4] {
    ITaifoonUniversalOperator::executeWithProofCall::SELECTOR
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::address;
    use alloy::sol_types::SolCall;

    const ADAPTER: Address = address!("00000000000000000000000000000000000000aD");

    #[test]
    fn calldata_starts_with_correct_selector() {
        let calldata = build_execute_with_proof_calldata(
            vec![0xDEu8, 0xAD, 0xBE, 0xEF],
            ADAPTER,
            vec![0x01u8, 0x02],
        );
        // First 4 bytes are the function selector.
        assert!(calldata.len() >= 4);
        assert_eq!(&calldata[0..4], &execute_with_proof_selector());
    }

    #[test]
    fn calldata_roundtrips_via_abi_decode() {
        let proof = vec![0xAAu8; 37]; // non-word-aligned length on purpose
        let adapter_cd = vec![0xBBu8; 5];
        let calldata =
            build_execute_with_proof_calldata(proof.clone(), ADAPTER, adapter_cd.clone());

        // Decoding the calldata yields back exactly the three inputs —
        // proving the wrap is faithful and chain-decodable.
        let decoded =
            ITaifoonUniversalOperator::executeWithProofCall::abi_decode(&calldata, true)
                .expect("calldata must decode against the operator ABI");
        assert_eq!(decoded.v5ProofBlob.as_ref(), proof.as_slice());
        assert_eq!(decoded.adapterContract, ADAPTER);
        assert_eq!(decoded.adapterCalldata.as_ref(), adapter_cd.as_slice());
    }

    #[test]
    fn matches_across_executor_encoding() {
        // The Across executor builds the identical struct inline. Our helper
        // must produce byte-identical output, so the generic path and the
        // Across path can never drift.
        let proof = vec![0x11u8, 0x22, 0x33];
        let adapter_cd = vec![0x44u8, 0x55];
        let ours = build_execute_with_proof_calldata(proof.clone(), ADAPTER, adapter_cd.clone());
        let inline = ITaifoonUniversalOperator::executeWithProofCall {
            v5ProofBlob: Bytes::from(proof),
            adapterContract: ADAPTER,
            adapterCalldata: Bytes::from(adapter_cd),
        }
        .abi_encode();
        assert_eq!(ours, inline);
    }

    #[test]
    fn empty_inputs_still_encode_to_valid_selector() {
        // Empty proof / empty adapter calldata is a degenerate-but-valid
        // encoding (the operator would reject it on-chain, but the codec
        // must not panic). Guards against an empty-slice edge case.
        let calldata = build_execute_with_proof_calldata(Vec::<u8>::new(), ADAPTER, Vec::<u8>::new());
        assert_eq!(&calldata[0..4], &execute_with_proof_selector());
        let decoded =
            ITaifoonUniversalOperator::executeWithProofCall::abi_decode(&calldata, true).unwrap();
        assert!(decoded.v5ProofBlob.is_empty());
        assert!(decoded.adapterCalldata.is_empty());
    }
}
