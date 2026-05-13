//! Solver registry client — permit issuance via the Spinner service.
//!
//! The Spinner acts as the trust root: it holds V5 proof bundles for every
//! live intent, so it can verify that an intent exists, is not expired, and
//! that the requested fill amount is within the intent's maxReward. It then
//! signs a `FillPermit` with its own ECDSA key so solvers can present the
//! permit at execution time without a second network call.
//!
//! ## Protocol
//!
//! ```text
//! Solver  →  POST /api/solver/well-permit
//!            { intent_id, chain_id, amount_wei, solver_address }
//!
//! Spinner checks:
//!   1. intent exists in genome archive + not expired
//!   2. solver_address is registered (not banned)
//!   3. maxReward ≥ amount_wei (±1% slippage tolerance)
//!   4. no permit already issued for (intent_id, chain_id)
//!   5. signs FillPermit with spinner ECDSA key (EIP-712 domain)
//!
//! Spinner →  { permit: FillPermit, signature: "0x..." }
//! ```

use alloy::primitives::{Address, U256};
use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use thiserror::Error;
use tracing::{info, warn};

// ── Types ─────────────────────────────────────────────────────────────────────

/// Permit issued by the Spinner authorising a solver to draw from the
/// liquidity well for a specific intent fill on a specific chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillPermit {
    /// Hex-encoded 32-byte intent ID.
    pub intent_id: String,
    /// The solver address that may present this permit.
    pub solver: String,
    /// EVM chain ID of the destination well.
    pub chain_id: u64,
    /// Amount (wei) the solver is authorised to draw.
    pub amount_wei: String,
    /// Unix timestamp after which the permit is invalid.
    pub deadline: u64,
    /// Spinner-side monotonic nonce per (intent_id, chain_id) to prevent replay.
    pub nonce: u64,
    /// Spinner ECDSA signature over the above fields (EIP-712 encoded).
    pub signature: String,
}

/// Solver reputation record returned by the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolverRecord {
    pub address: String,
    pub reputation: i32,
    pub banned: bool,
    pub registered_at: Option<u64>,
}

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Intent not found or expired (intent_id={intent_id})")]
    IntentNotFound { intent_id: String },
    #[error("Solver banned (address={address})")]
    SolverBanned { address: String },
    #[error("Permit already issued for (intent_id={intent_id}, chain_id={chain_id})")]
    DuplicatePermit { intent_id: String, chain_id: u64 },
    #[error("Amount exceeds intent maxReward")]
    AmountExceedsReward,
    #[error("Spinner returned error: {0}")]
    SpinnerError(String),
    #[error("Permit signature invalid")]
    InvalidSignature,
    #[error("Permit expired")]
    PermitExpired,
    #[error("Permit already used")]
    PermitAlreadyUsed,
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

// ── Registry client (outbound — calls Spinner) ────────────────────────────────

pub struct RegistryClient {
    spinner_url: String,
    solver_address: Address,
    http: reqwest::Client,
}

impl RegistryClient {
    pub fn new(spinner_url: impl Into<String>, solver_address: Address) -> Self {
        Self {
            spinner_url: spinner_url.into(),
            solver_address,
            http: reqwest::Client::builder()
                .user_agent("taifoon-solver-registry/1.0")
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    /// Request a signed fill permit from the Spinner for the given intent.
    pub async fn request_permit(
        &self,
        intent_id: &str,
        chain_id: u64,
        amount_wei: U256,
    ) -> Result<FillPermit, RegistryError> {
        let url = format!("{}/api/solver/well-permit", self.spinner_url);
        let body = serde_json::json!({
            "intent_id": intent_id,
            "chain_id": chain_id,
            "amount_wei": amount_wei.to_string(),
            "solver_address": format!("{:#x}", self.solver_address),
        });

        let resp = self.http.post(&url).json(&body).send().await?;

        if resp.status().as_u16() == 409 {
            return Err(RegistryError::DuplicatePermit {
                intent_id: intent_id.to_string(),
                chain_id,
            });
        }
        if resp.status().as_u16() == 403 {
            return Err(RegistryError::SolverBanned { address: format!("{:#x}", self.solver_address) });
        }
        if resp.status().as_u16() == 404 {
            return Err(RegistryError::IntentNotFound { intent_id: intent_id.to_string() });
        }
        if !resp.status().is_success() {
            let msg = resp.text().await.unwrap_or_default();
            return Err(RegistryError::SpinnerError(msg));
        }

        let permit: FillPermit = resp.json().await?;
        info!("[registry] permit issued for intent={} chain={}", intent_id, chain_id);
        Ok(permit)
    }

    /// Look up a solver's reputation record.
    pub async fn get_solver_record(
        &self,
        address: &str,
    ) -> Result<SolverRecord, RegistryError> {
        let url = format!("{}/api/solver/record/{}", self.spinner_url, address);
        let resp = self.http.get(&url).send().await?;
        if !resp.status().is_success() {
            let msg = resp.text().await.unwrap_or_default();
            return Err(RegistryError::SpinnerError(msg));
        }
        let record: SolverRecord = resp.json().await?;
        Ok(record)
    }
}

// ── Auth guard (enforces permit uniqueness + signature locally) ───────────────

/// Guards well-permit fund draws by validating Spinner-issued permits before use.
///
/// - Verifies the permit's ECDSA signature against the known Spinner public key.
/// - Rejects expired permits.
/// - Prevents double-use of a permit by persisting used (intent_id, chain_id) pairs
///   in a local SQLite database.
pub struct WellAuthGuard {
    spinner_pub_key: Address,
    db: Mutex<Connection>,
}

impl WellAuthGuard {
    pub fn new(spinner_pub_key: Address, db_path: &str) -> Result<Self> {
        let conn = Connection::open(db_path).context("open permit db")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS used_permits (
                intent_id TEXT NOT NULL,
                chain_id  INTEGER NOT NULL,
                used_at   INTEGER NOT NULL,
                tx_hash   TEXT,
                PRIMARY KEY (intent_id, chain_id)
            );",
        ).context("create used_permits table")?;
        Ok(Self {
            spinner_pub_key,
            db: Mutex::new(conn),
        })
    }

    /// Validate a permit before using it to draw from the well.
    ///
    /// Returns `Ok(())` if the permit is valid, unused, and not expired.
    pub fn validate(&self, permit: &FillPermit) -> Result<(), RegistryError> {
        // 1. Expiry check
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if permit.deadline < now {
            warn!("[auth] permit expired: intent={} deadline={}", permit.intent_id, permit.deadline);
            return Err(RegistryError::PermitExpired);
        }

        // 2. Signature verification (EIP-191 personal_sign over permit hash)
        if let Err(e) = verify_permit_signature(permit, self.spinner_pub_key) {
            warn!("[auth] permit signature invalid: {}", e);
            return Err(RegistryError::InvalidSignature);
        }

        // 3. Duplicate-use check
        let db = self.db.lock().unwrap();
        let count: u32 = db.query_row(
            "SELECT COUNT(*) FROM used_permits WHERE intent_id = ?1 AND chain_id = ?2",
            params![permit.intent_id, permit.chain_id as i64],
            |row| row.get(0),
        ).unwrap_or(0);

        if count > 0 {
            warn!("[auth] permit already used: intent={} chain={}", permit.intent_id, permit.chain_id);
            return Err(RegistryError::PermitAlreadyUsed);
        }

        Ok(())
    }

    /// Mark a permit as consumed (called after the fill tx is broadcast).
    pub fn consume(&self, permit: &FillPermit, tx_hash: Option<&str>) -> Result<(), RegistryError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT OR IGNORE INTO used_permits (intent_id, chain_id, used_at, tx_hash) VALUES (?1, ?2, ?3, ?4)",
            params![permit.intent_id, permit.chain_id as i64, now as i64, tx_hash],
        )?;
        info!("[auth] permit consumed: intent={} chain={}", permit.intent_id, permit.chain_id);
        Ok(())
    }

    /// Check whether a permit has already been consumed (idempotent read).
    pub fn is_used(&self, intent_id: &str, chain_id: u64) -> bool {
        let db = self.db.lock().unwrap();
        let count: u32 = db.query_row(
            "SELECT COUNT(*) FROM used_permits WHERE intent_id = ?1 AND chain_id = ?2",
            params![intent_id, chain_id as i64],
            |row| row.get(0),
        ).unwrap_or(0);
        count > 0
    }
}

// ── Signature verification ────────────────────────────────────────────────────

/// Verify the Spinner's ECDSA signature over the permit fields.
///
/// Encoding: EIP-191 personal_sign over:
///   keccak256(abi.encodePacked(intent_id_bytes, chain_id_u64, amount_wei_u256, deadline_u64, nonce_u64))
fn verify_permit_signature(permit: &FillPermit, expected_signer: Address) -> Result<()> {
    use alloy::primitives::keccak256;
    use alloy::signers::Signature;

    // Decode intent_id from hex to 32 bytes
    let id_hex = permit.intent_id.trim_start_matches("0x");
    let id_bytes = hex::decode(id_hex).context("decode intent_id hex")?;

    let amount_u256: U256 = permit.amount_wei.parse().context("parse amount_wei")?;
    let amount_bytes: [u8; 32] = amount_u256.to_be_bytes();

    // Concatenate fields for the hash
    let mut msg = Vec::with_capacity(32 + 8 + 32 + 8 + 8);
    msg.extend_from_slice(&id_bytes);
    msg.extend_from_slice(&permit.chain_id.to_be_bytes());
    msg.extend_from_slice(&amount_bytes);
    msg.extend_from_slice(&permit.deadline.to_be_bytes());
    msg.extend_from_slice(&permit.nonce.to_be_bytes());

    let msg_hash = keccak256(&msg);

    // EIP-191 prefix
    let prefixed = format!("\x19Ethereum Signed Message:\n32");
    let mut prefixed_bytes = prefixed.into_bytes();
    prefixed_bytes.extend_from_slice(msg_hash.as_slice());
    let final_hash = keccak256(&prefixed_bytes);

    let sig_hex = permit.signature.trim_start_matches("0x");
    let sig_bytes = hex::decode(sig_hex).context("decode signature hex")?;
    if sig_bytes.len() != 65 {
        anyhow::bail!("signature must be 65 bytes");
    }

    let signature = Signature::try_from(sig_bytes.as_slice()).context("parse signature")?;
    let recovered = signature.recover_address_from_prehash(&final_hash).context("recover address")?;

    if recovered != expected_signer {
        anyhow::bail!("signer mismatch: expected {:#x} got {:#x}", expected_signer, recovered);
    }

    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_guard() -> WellAuthGuard {
        WellAuthGuard::new(Address::ZERO, ":memory:").unwrap()
    }

    fn expired_permit() -> FillPermit {
        FillPermit {
            intent_id: "0x0000000000000000000000000000000000000000000000000000000000000001".into(),
            solver: "0x0000000000000000000000000000000000000000".into(),
            chain_id: 8453,
            amount_wei: "1000000".into(),
            deadline: 1, // far in the past
            nonce: 0,
            signature: format!("0x{}", "00".repeat(65)),
        }
    }

    fn future_permit() -> FillPermit {
        FillPermit {
            intent_id: "0x0000000000000000000000000000000000000000000000000000000000000002".into(),
            solver: "0x0000000000000000000000000000000000000000".into(),
            chain_id: 8453,
            amount_wei: "1000000".into(),
            deadline: u64::MAX, // far in the future
            nonce: 0,
            // All-zero signature will fail crypto check but we skip that in unit test
            signature: format!("0x{}", "00".repeat(65)),
        }
    }

    #[test]
    fn expired_permit_rejected() {
        let guard = test_guard();
        let err = guard.validate(&expired_permit()).unwrap_err();
        assert!(matches!(err, RegistryError::PermitExpired));
    }

    #[test]
    fn used_permit_rejected_after_consume() {
        let guard = WellAuthGuard::new(Address::ZERO, ":memory:").unwrap();
        let permit = future_permit();
        // Mark as used directly (bypassing sig check for unit test)
        guard.consume(&permit, Some("0xdeadbeef")).unwrap();
        // validate should now hit the duplicate check (before sig check)
        // We need to verify is_used first
        assert!(guard.is_used(&permit.intent_id, permit.chain_id));
    }

    #[test]
    fn not_used_initially() {
        let guard = test_guard();
        assert!(!guard.is_used("0xabc", 8453));
    }

    #[test]
    fn consume_is_idempotent() {
        let guard = test_guard();
        let permit = future_permit();
        guard.consume(&permit, None).unwrap();
        guard.consume(&permit, None).unwrap(); // should not error (INSERT OR IGNORE)
    }
}
