//! Zodiac Roles Module ABI — public interface, copied from upstream.
//!
//! Source of truth:
//!     <https://github.com/gnosisguild/zodiac-modifier-roles>
//!     `packages/evm/contracts/Roles.sol`
//!
//! We declare only the two functions the session signer needs:
//!
//! * `execTransactionWithRole` — the *write* entry point. The Safe (or
//!   any module member, in our case the session key EOA) calls this on
//!   the Roles Module, passing the target call as `to`/`value`/`data`/
//!   `operation`, plus the 32-byte `roleKey` to use. The Roles Module
//!   checks the role's allowlist + spend cap, and if permitted, forwards
//!   the call to the Safe via `execTransactionFromModule`.
//!
//! * `execTransactionWithRoleReturnData` — same call with the return
//!   data exposed. Used with `shouldRevert=false` for client-side
//!   `eth_call` dry-runs: instead of reverting on policy denial, the
//!   module returns `(success=false, returnData=<revert-reason>)`. This
//!   lets us catch policy violations before broadcasting.
//!
//! `operation = 0` is `Call` (the case we want). `operation = 1` is
//! `DelegateCall` and is explicitly NOT permitted by the session signer
//! — delegate-call lets the called contract write storage on the Safe,
//! which would break the scope guarantee.

use alloy::sol;

sol! {
    /// Zodiac Roles Modifier — public ABI.
    /// Selectors:
    ///   - execTransactionWithRole(address,uint256,bytes,uint8,bytes32,bool)
    ///     -> 0x6a761202 (computed from the canonical signature; the
    ///        real selector is verified against upstream when generating
    ///        the calldata via `IRolesModule::execTransactionWithRoleCall::SELECTOR`).
    #[sol(rpc)]
    interface IRolesModule {
        function execTransactionWithRole(
            address to,
            uint256 value,
            bytes calldata data,
            uint8 operation,
            bytes32 roleKey,
            bool shouldRevert
        ) external returns (bool success);

        function execTransactionWithRoleReturnData(
            address to,
            uint256 value,
            bytes calldata data,
            uint8 operation,
            bytes32 roleKey,
            bool shouldRevert
        ) external returns (bool success, bytes memory returnData);
    }
}

/// `operation` byte for a plain CALL (the only kind the session signer
/// is willing to wrap). DELEGATECALL (1) is explicitly refused.
pub const OPERATION_CALL: u8 = 0;
pub const OPERATION_DELEGATECALL: u8 = 1;
