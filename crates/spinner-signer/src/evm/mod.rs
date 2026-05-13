//! EVM session signer — Safe + Zodiac Roles Module.
//!
//! ## How it composes
//!
//! ```text
//! ┌────────────┐ wrap_for_session ┌──────────────────┐ sign+broadcast ┌─────────┐
//! │ raw target │ ────────────────▶│ Roles Module     │ ─────────────▶│ chain   │
//! │ call       │  exec…WithRole   │ execTransaction… │  (session key) │ (Safe   │
//! │ (executor) │                  │   WithRole call  │                │  state) │
//! └────────────┘                  └──────────────────┘                └─────────┘
//! ```
//!
//! The session key (hot EOA) is added to a scoped Role on the Roles
//! Module. The Role pins:
//!
//! * the list of target contracts the call can hit (e.g. only the
//!   Across V3 SpokePool, only the Mayan Flash router, ...)
//! * per-target allowed function selectors
//! * spend caps (per-token, per-period)
//! * an optional `expiresAt` timestamp
//!
//! All four are enforced on-chain by the Roles Module before the call
//! reaches the Safe. The session key cannot drain the Safe by signing
//! anything outside the Role's scope.
//!
//! ## Upstream references
//!
//! - Safe Smart Account:
//!   <https://github.com/safe-global/safe-smart-account>
//!     - canonical singleton (1.4.1):
//!       `0x41675C099F32341bf84BFc5382aF534df5C7461a`
//!     - Safe Proxy Factory (1.4.1):
//!       `0x4e1DCf7AD4e460CfD30791CCC4F9c8a4f820ec67`
//!     - These addresses ship on every chain Safe supports; verify on
//!       <https://docs.safe.global/advanced/smart-account-supported-networks>
//!       before depending on them.
//! - Zodiac Roles Modifier:
//!   <https://github.com/gnosisguild/zodiac-modifier-roles>

pub mod roles_abi;

pub use roles_abi::{IRolesModule, OPERATION_CALL, OPERATION_DELEGATECALL};

use crate::config::EvmSessionConfig;
use crate::{PolicyCheckResult, SessionSigner};

use alloy::primitives::{Address, Bytes, B256, U256};
use alloy::rpc::types::TransactionRequest;
use alloy::sol_types::{SolCall, SolValue};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use std::str::FromStr;

/// Session-key signer for a Safe-protected EVM treasury.
///
/// Construct via [`EvmSafeRolesSigner::from_config`]. Implements
/// [`SessionSigner`] with:
///
/// * `Transaction = TransactionRequest`
/// * `SignedTransaction = SignedTransaction` (wrapper around the raw
///   RLP bytes the provider returns from `eth_sendRawTransaction`'s
///   precursor — kept opaque so we don't lock the public API to a
///   single alloy minor version).
/// * `TxHash = B256`
/// * `Error = anyhow::Error`
#[derive(Debug)]
pub struct EvmSafeRolesSigner {
    config: EvmSessionConfig,
    /// Parsed once at construction so `wrap_for_session` doesn't have to
    /// re-parse the role-key hex on every call.
    role_key_bytes: B256,
    roles_module_addr: Address,
    /// Lowercased target addresses for O(1) allowlist lookup.
    allowed_targets_normalized: Vec<Address>,
}

/// Opaque signed-transaction wrapper.
///
/// We hold the raw RLP envelope the alloy signer produced. `broadcast`
/// hands this to `eth_sendRawTransaction` via the provider.
#[derive(Debug, Clone)]
pub struct SignedTransaction {
    pub raw: Bytes,
    /// Pre-computed tx hash (keccak256 of the RLP envelope) — alloy
    /// returns this from its sign call.
    pub hash: B256,
}

impl EvmSafeRolesSigner {
    /// Construct from declarative config.
    ///
    /// This does NOT read the env var pointed at by
    /// `config.session_key_env` — the hot key is loaded lazily on the
    /// first call to [`sign`](Self::sign). That way a misconfigured
    /// operator who hasn't set the env var yet still gets useful errors
    /// from [`wrap_for_session`](Self::wrap_for_session) and
    /// [`dry_run_policy`](Self::dry_run_policy) (which don't need the
    /// private key).
    pub fn from_config(config: EvmSessionConfig) -> Result<Self> {
        let roles_module_addr = Address::from_str(&config.roles_module)
            .with_context(|| format!("roles_module {} not a valid address", config.roles_module))?;

        let role_key_hex = config
            .role_key
            .strip_prefix("0x")
            .unwrap_or(&config.role_key);
        let role_key_bytes = hex::decode(role_key_hex)
            .with_context(|| format!("role_key {} not valid hex", config.role_key))?;
        if role_key_bytes.len() != 32 {
            return Err(anyhow!(
                "role_key must be 32 bytes, got {}",
                role_key_bytes.len()
            ));
        }
        let role_key_bytes = B256::from_slice(&role_key_bytes);

        let mut allowed_targets_normalized = Vec::with_capacity(config.allowed_targets.len());
        for t in &config.allowed_targets {
            let parsed = Address::from_str(t)
                .with_context(|| format!("allowed_target {} not a valid address", t))?;
            allowed_targets_normalized.push(parsed);
        }

        Ok(Self {
            config,
            role_key_bytes,
            roles_module_addr,
            allowed_targets_normalized,
        })
    }

    /// Read-only accessor for the parsed Roles Module address.
    pub fn roles_module(&self) -> Address {
        self.roles_module_addr
    }

    /// Read-only accessor for the role key.
    pub fn role_key(&self) -> B256 {
        self.role_key_bytes
    }

    /// Returns `true` iff `target` is in the allowlist. Public so tests
    /// can assert allowlist semantics without going through
    /// `wrap_for_session`.
    pub fn is_target_allowed(&self, target: &Address) -> bool {
        self.allowed_targets_normalized.contains(target)
    }

    /// Encode the calldata that will be sent to the Roles Module.
    /// Pure function — no I/O, no signing. Public so callers and tests
    /// can inspect the exact bytes that would be broadcast.
    pub fn encode_exec_with_role_calldata(
        &self,
        target: Address,
        value: U256,
        data: Bytes,
        should_revert: bool,
    ) -> Bytes {
        let call = IRolesModule::execTransactionWithRoleCall {
            to: target,
            value,
            data,
            operation: OPERATION_CALL,
            roleKey: self.role_key_bytes,
            shouldRevert: should_revert,
        };
        Bytes::from(call.abi_encode())
    }

    /// Encode the `execTransactionWithRoleReturnData` calldata for an
    /// `eth_call` dry-run (note `shouldRevert=false` so the module
    /// returns the revert reason in returnData instead of reverting).
    pub fn encode_dry_run_calldata(
        &self,
        target: Address,
        value: U256,
        data: Bytes,
    ) -> Bytes {
        let call = IRolesModule::execTransactionWithRoleReturnDataCall {
            to: target,
            value,
            data,
            operation: OPERATION_CALL,
            roleKey: self.role_key_bytes,
            shouldRevert: false,
        };
        Bytes::from(call.abi_encode())
    }

    /// Decode the `(bool success, bytes returnData)` tuple returned by a
    /// successful `eth_call` on `execTransactionWithRoleReturnData`.
    ///
    /// On policy denial: `success == false` and `returnData` carries the
    /// ABI-encoded revert reason (typically `Error(string)` — selector
    /// `0x08c379a0` — though Zodiac sometimes uses custom errors).
    pub fn decode_dry_run_result(raw: &[u8]) -> Result<PolicyCheckResult> {
        // Tuple ABI-decode: (bool, bytes).
        let (success, return_data) = <(bool, Bytes)>::abi_decode(raw, true)
            .context("decode (bool, bytes) from execTransactionWithRoleReturnData")?;
        if success {
            return Ok(PolicyCheckResult::Allowed);
        }
        let reason = parse_revert_reason(&return_data);
        Ok(PolicyCheckResult::Denied { reason })
    }
}

/// Best-effort decoder for an EVM revert payload.
///
/// Recognizes the standard `Error(string)` shape (selector
/// `0x08c379a0`); falls back to a hex preview of the raw bytes so
/// the caller still sees *something* useful for custom-error reverts.
fn parse_revert_reason(return_data: &Bytes) -> String {
    const ERROR_STRING_SELECTOR: [u8; 4] = [0x08, 0xc3, 0x79, 0xa0];
    if return_data.len() >= 4 && return_data[..4] == ERROR_STRING_SELECTOR {
        if let Ok(s) = String::abi_decode(&return_data[4..], true) {
            return s;
        }
    }
    if return_data.is_empty() {
        return "scope contract denied (empty return data)".into();
    }
    format!("0x{}", hex::encode(return_data.as_ref()))
}

#[async_trait]
impl SessionSigner for EvmSafeRolesSigner {
    type Transaction = TransactionRequest;
    type SignedTransaction = SignedTransaction;
    type TxHash = B256;
    type Error = anyhow::Error;

    async fn wrap_for_session(
        &self,
        target_call: TransactionRequest,
    ) -> Result<TransactionRequest, Self::Error> {
        let target = target_call
            .to
            .as_ref()
            .and_then(|t| t.to().copied())
            .ok_or_else(|| anyhow!("target_call.to is unset or a contract-creation tx"))?;

        if !self.is_target_allowed(&target) {
            return Err(anyhow!(
                "target {target:?} not in allowed_targets ({} entries)",
                self.allowed_targets_normalized.len()
            ));
        }

        let value = target_call.value.unwrap_or(U256::ZERO);
        let data = target_call.input.input().cloned().unwrap_or_default();

        let wrapped_calldata =
            self.encode_exec_with_role_calldata(target, value, data, /*should_revert*/ true);

        // Build a new request that calls the Roles Module instead of
        // hitting the target directly. Value is zero on the *outer* call
        // (the Roles Module forwards `value` from its calldata to the
        // Safe's `execTransactionFromModule`).
        let wrapped = TransactionRequest::default()
            .to(self.roles_module_addr)
            .input(wrapped_calldata.into());

        Ok(wrapped)
    }

    async fn dry_run_policy(
        &self,
        target_call: &TransactionRequest,
    ) -> Result<PolicyCheckResult, Self::Error> {
        // Re-validate target locally so a caller who forgot to call
        // wrap_for_session still gets the right answer.
        let target = target_call
            .to
            .as_ref()
            .and_then(|t| t.to().copied())
            .ok_or_else(|| anyhow!("target_call.to is unset"))?;

        if !self.is_target_allowed(&target) {
            return Ok(PolicyCheckResult::Denied {
                reason: format!("target {target:?} not in allowed_targets"),
            });
        }

        let value = target_call.value.unwrap_or(U256::ZERO);
        let data = target_call.input.input().cloned().unwrap_or_default();

        let dry_run_calldata = self.encode_dry_run_calldata(target, value, data);

        // The actual eth_call is a thin wrapper. We POST a minimal JSON-RPC
        // payload via reqwest rather than instantiating an alloy provider
        // here, because the provider construction wants a Reqwest URL and
        // alloy's higher-level provider trait surface churns between
        // minor versions. This keeps the public API of this crate stable.
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_call",
            "params": [
                {
                    "to": format!("{:#x}", self.roles_module_addr),
                    "data": format!("0x{}", hex::encode(dry_run_calldata.as_ref())),
                },
                "latest"
            ]
        });

        let resp = reqwest::Client::new()
            .post(&self.config.rpc_url)
            .json(&payload)
            .send()
            .await
            .context("eth_call request")?;
        let body: serde_json::Value = resp.json().await.context("eth_call response json")?;

        if let Some(err) = body.get("error") {
            // A `-32000 execution reverted` with no return data means the
            // module reverted *before* getting to the policy check — for
            // example, the role key isn't even installed on the module.
            // We surface that as Denied so callers don't broadcast.
            let msg = err
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("execution reverted")
                .to_string();
            return Ok(PolicyCheckResult::Denied { reason: msg });
        }

        let result_hex = body
            .get("result")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("eth_call response missing result field: {body}"))?;
        let raw = hex::decode(result_hex.trim_start_matches("0x"))
            .context("decode eth_call result hex")?;

        Self::decode_dry_run_result(&raw)
    }

    async fn sign(&self, tx: TransactionRequest) -> Result<SignedTransaction, Self::Error> {
        // Load the hot session key on first use. Production callers
        // should construct one signer per process and reuse it — this is
        // not a hot path.
        let session_key_hex = std::env::var(&self.config.session_key_env).map_err(|_| {
            anyhow!(
                "session-key env var {} is unset",
                self.config.session_key_env
            )
        })?;
        let session_key_hex = session_key_hex.trim_start_matches("0x");
        let session_key_bytes = hex::decode(session_key_hex)
            .context("session-key env var did not contain valid hex")?;
        if session_key_bytes.len() != 32 {
            return Err(anyhow!(
                "session-key must be 32 bytes, got {}",
                session_key_bytes.len()
            ));
        }
        let signer = alloy::signers::local::PrivateKeySigner::from_slice(&session_key_bytes)
            .context("construct PrivateKeySigner from session-key bytes")?;

        // Build a fully-populated envelope. Real wiring will use a fillerful
        // alloy provider (gas estimate, nonce, fee fields) — for the
        // reference impl we delegate to the executor which already has that
        // logic. Here we sign whatever the caller built.
        //
        // The reference impl uses alloy's `sign_transaction_sync` via the
        // legacy `SignerSync` trait when the request is fully populated.
        // For a request with missing fields we punt: a real executor
        // populates them via `ProviderBuilder::new().with_recommended_fillers()`
        // BEFORE handing the tx to this signer.
        let envelope = build_signed_envelope(&signer, tx, self.config.chain_id)
            .context("sign envelope")?;
        Ok(envelope)
    }

    async fn broadcast(&self, signed: SignedTransaction) -> Result<B256, Self::Error> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_sendRawTransaction",
            "params": [format!("0x{}", hex::encode(signed.raw.as_ref()))]
        });

        let resp = reqwest::Client::new()
            .post(&self.config.rpc_url)
            .json(&payload)
            .send()
            .await
            .context("eth_sendRawTransaction request")?;
        let body: serde_json::Value = resp.json().await.context("response json")?;

        if let Some(err) = body.get("error") {
            return Err(anyhow!("eth_sendRawTransaction error: {err}"));
        }

        // The RPC returns the tx hash. Cross-check it against the locally
        // computed hash — a mismatch means we somehow signed something
        // different from what we broadcast (config / nonce drift). Better
        // to fail loudly than to silently lose the tx.
        let rpc_hash_hex = body
            .get("result")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("eth_sendRawTransaction missing result: {body}"))?;
        let rpc_hash = B256::from_str(rpc_hash_hex)
            .with_context(|| format!("parse tx hash {rpc_hash_hex}"))?;
        if rpc_hash != signed.hash {
            return Err(anyhow!(
                "tx-hash mismatch: local {:?} vs rpc {:?}",
                signed.hash,
                rpc_hash
            ));
        }
        Ok(rpc_hash)
    }
}

/// Sign a `TransactionRequest` with the given signer.
///
/// This is intentionally narrow — it expects the request to already
/// carry `nonce`, `gas`, fee fields, and `chain_id`. In production the
/// executor populates those via alloy's recommended fillers before
/// handing the tx down to the session signer. In tests we construct the
/// request with all fields set.
fn build_signed_envelope(
    signer: &alloy::signers::local::PrivateKeySigner,
    mut tx: TransactionRequest,
    chain_id: u64,
) -> Result<SignedTransaction> {
    use alloy::consensus::SignableTransaction;
    use alloy::network::TxSignerSync;

    // Ensure chain_id is pinned so we can't be replayed cross-chain.
    if tx.chain_id.is_none() {
        tx.chain_id = Some(chain_id);
    }

    // Build the typed EIP-1559 transaction envelope. The reference impl
    // only supports EIP-1559 (type 2) — legacy and EIP-2930 are not
    // wired up here because every chain a Safe runs on supports 1559.
    let mut typed: alloy::consensus::TxEip1559 = tx
        .build_typed_tx()
        .map_err(|e| anyhow!("build_typed_tx: {e:?}"))?
        .eip1559()
        .ok_or_else(|| anyhow!("not an EIP-1559 transaction"))?
        .clone();

    let sig = signer
        .sign_transaction_sync(&mut typed)
        .context("sign_transaction_sync")?;
    let signed = typed.into_signed(sig);
    let hash = *signed.hash();

    // Encode the envelope. The wire format for EIP-1559 is
    // `0x02 || rlp([...fields, v, r, s])`.
    use alloy::eips::eip2718::Encodable2718;
    let mut raw = Vec::new();
    let envelope: alloy::consensus::TxEnvelope = signed.into();
    envelope.encode_2718(&mut raw);

    Ok(SignedTransaction {
        raw: Bytes::from(raw),
        hash,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> EvmSessionConfig {
        EvmSessionConfig {
            safe_address: "0x1111111111111111111111111111111111111111".into(),
            roles_module: "0x2222222222222222222222222222222222222222".into(),
            role_key: "0x".to_string() + &"ab".repeat(32),
            allowed_targets: vec!["0x09aea4b2242abC8bb4BB78D537A67a245A7bEC64".into()],
            session_key_env: "SPINNER_TEST_EVM_KEY_DO_NOT_USE".into(),
            rpc_url: "http://127.0.0.1:0/never-called".into(),
            chain_id: 8453,
        }
    }

    #[test]
    fn from_config_parses_role_key_and_targets() {
        let s = EvmSafeRolesSigner::from_config(test_config()).unwrap();
        assert_eq!(
            s.roles_module(),
            Address::from_str("0x2222222222222222222222222222222222222222").unwrap()
        );
        assert_eq!(s.role_key().as_slice(), &[0xab; 32]);
        assert!(s.is_target_allowed(
            &Address::from_str("0x09aea4b2242abC8bb4BB78D537A67a245A7bEC64").unwrap()
        ));
        // Different address — must not be allowed.
        assert!(!s.is_target_allowed(
            &Address::from_str("0xDEAdbeefdeadbeefdeadbeefdeadbeefdeadBeef").unwrap()
        ));
    }

    #[test]
    fn encode_dry_run_calldata_has_right_selector() {
        let s = EvmSafeRolesSigner::from_config(test_config()).unwrap();
        let target =
            Address::from_str("0x09aea4b2242abC8bb4BB78D537A67a245A7bEC64").unwrap();
        let cd = s.encode_dry_run_calldata(target, U256::ZERO, Bytes::from(vec![0x12, 0x34]));
        assert_eq!(
            &cd[..4],
            &IRolesModule::execTransactionWithRoleReturnDataCall::SELECTOR
        );
    }

    #[test]
    fn encode_exec_calldata_has_right_selector_and_should_revert_flag() {
        let s = EvmSafeRolesSigner::from_config(test_config()).unwrap();
        let target =
            Address::from_str("0x09aea4b2242abC8bb4BB78D537A67a245A7bEC64").unwrap();
        let cd = s.encode_exec_with_role_calldata(
            target,
            U256::ZERO,
            Bytes::from(vec![0xde, 0xad]),
            /*should_revert*/ true,
        );
        assert_eq!(&cd[..4], &IRolesModule::execTransactionWithRoleCall::SELECTOR);
        // The shouldRevert bool is the last static-ABI word in the encoding —
        // last 32 bytes will encode `true` as a left-padded 0x01.
        // (Plus tail bytes for the `data` blob — but the static portion
        // contains: to(32) + value(32) + dataOffset(32) + operation(32)
        // + roleKey(32) + shouldRevert(32) = 6*32 = 192 = static head.
        let head_end = 4 + 6 * 32;
        let should_revert_word = &cd[head_end - 32..head_end];
        assert_eq!(&should_revert_word[31], &1u8);
    }

    #[test]
    fn decode_dry_run_result_allowed() {
        // ABI-encode (true, b"") and feed it back through the decoder.
        let blob = <(bool, Bytes)>::abi_encode(&(true, Bytes::new()));
        let r = EvmSafeRolesSigner::decode_dry_run_result(&blob).unwrap();
        assert_eq!(r, PolicyCheckResult::Allowed);
    }

    #[test]
    fn decode_dry_run_result_denied_with_error_string() {
        // Manually craft `Error("SpendLimitExceeded")` payload as the
        // returnData, then wrap in (false, bytes).
        const ERROR_STRING_SELECTOR: [u8; 4] = [0x08, 0xc3, 0x79, 0xa0];
        let inner = String::abi_encode(&"SpendLimitExceeded".to_string());
        let mut return_data = Vec::new();
        return_data.extend_from_slice(&ERROR_STRING_SELECTOR);
        return_data.extend_from_slice(&inner);
        let blob = <(bool, Bytes)>::abi_encode(&(false, Bytes::from(return_data)));
        let r = EvmSafeRolesSigner::decode_dry_run_result(&blob).unwrap();
        match r {
            PolicyCheckResult::Denied { reason } => {
                assert_eq!(reason, "SpendLimitExceeded");
            }
            other => panic!("expected Denied, got {other:?}"),
        }
    }

    #[test]
    fn from_config_rejects_bad_role_key_length() {
        let mut cfg = test_config();
        cfg.role_key = "0xabab".into(); // 2 bytes, not 32
        let err = EvmSafeRolesSigner::from_config(cfg).unwrap_err();
        assert!(err.to_string().contains("32 bytes"));
    }
}
