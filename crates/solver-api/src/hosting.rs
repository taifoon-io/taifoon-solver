//! Hosted solver provisioning registry.
//!
//! Allows participants to register themselves in the common hosting framework.
//! Each registered solver gets:
//!   - A unique `solver_id` (first 8 hex chars of their EVM address)
//!   - An API token for their solver instance (auth for mutation endpoints)
//!   - A signing_mode that determines key authority (session_key | remote_signer | self_hosted)
//!   - TSUL fee routing: their address receives 70% of every donut touch via BuildersRegistry
//!
//! ## Key authority model
//!
//! Taifoon NEVER holds private keys. Three modes:
//!
//! 1. `session_key` — User deploys a Safe multisig, grants Taifoon a scoped session
//!    key. Session key can ONLY call fill functions on registered adapter contracts.
//!    Spend limit enforced at Safe level. User can revoke at any time.
//!
//! 2. `remote_signer` — Solver builds unsigned transactions and POSTs them to
//!    `signer_webhook_url` (user-controlled). User's local signer approves and
//!    returns the signature. Truly non-custodial; adds one network round-trip
//!    per fill (acceptable for 30-min deadline intents).
//!
//! 3. `self_hosted` — User runs their own solver binary. This registry entry is
//!    purely for fleet-dashboard visibility and TSUL fee routing.
//!
//! ## TSUL fee routing
//!
//! Every fill calls `BuildersRegistry.recordRevenueTouch()` → 49 bps donut:
//!   - 70% → participant's `evm_address` (TSUL rule #4)
//!   - 20% → open-mamba reviewer set
//!   - 10% → ecosystem treasury
//!
//! This is enforced at the contract layer. The registry here tracks participants
//! so the dashboard can display per-solver donut accrual.

use axum::{
    extract::{Path, State},
    response::Json,
    http::StatusCode,
};
use donut_adjudicator::{
    hash_for_chain, CanonicalAdjudicator, DonutAttestation, FeeSplitAdjudicator, ZERO_HASH,
};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{info, warn};

/// TTL for SIWE nonces. After this many seconds an issued nonce is no longer
/// accepted as the `nonce` field of a SIWE message.
const SIWE_NONCE_TTL_SECS: u64 = 300;

/// Tolerance window for SIWE `expiration_time` comparison against
/// server time. Clients with slightly-fast clocks (~30-60s ahead of UTC
/// is common on un-synchronised laptops) would otherwise sign messages
/// that appear expired immediately. 60s is well below any plausible
/// stolen-signature replay window.
const SIWE_CLOCK_SKEW_SECS: i64 = 60;

/// Hard cap on the in-memory SIWE-nonce store. Without this an attacker who
/// hammers `POST /api/hosting/siwe-nonce` can grow the map without bound
/// between TTL-expirations. When the cap is hit, the oldest entries are
/// evicted (regardless of TTL) — this preserves the security invariant
/// (nonces remain single-use within their TTL) while keeping the worst-case
/// memory usage bounded. Sized for ~5MB of overhead at 64B/entry.
const SIWE_NONCE_MAX_ENTRIES: usize = 80_000;

/// Pinned `chain_id` for SIWE messages. The actual chain doesn't matter for
/// our purposes — we just need *some* fixed value so a signature scoped to
/// one chain can't be replayed against another deployment. We pick `1`
/// (Ethereum mainnet) by convention.
const SIWE_CHAIN_ID: u64 = 1;

// ── Types ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SigningMode {
    /// User deploys a Safe and grants us a scoped session key.
    SessionKey,
    /// User controls their key; we POST unsigned txs to their webhook.
    RemoteSigner,
    /// User runs their own binary. Registry entry = fleet visibility only.
    SelfHosted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostedSolver {
    /// Deterministic ID: first 8 hex chars of evm_address (lowercased, no 0x).
    pub solver_id: String,
    /// Human-readable name chosen at registration.
    pub name: String,
    /// EVM address that receives 70% TSUL donut and authenticates as solver owner.
    pub evm_address: String,
    /// Solana address (optional — for Mayan Swift Solana fills).
    pub solana_address: Option<String>,
    /// How keys are managed.
    pub signing_mode: SigningMode,
    /// For `remote_signer` mode: the user's webhook URL that receives unsigned tx.
    pub signer_webhook_url: Option<String>,
    /// For `session_key` mode: the Safe address that holds the session key.
    pub safe_address: Option<String>,
    /// Operator email for alerts.
    pub email: Option<String>,
    /// Comma-separated chain IDs this solver watches.
    pub chains: String,
    /// Comma-separated protocol IDs this solver runs.
    pub protocols: String,
    /// UTC timestamp of registration.
    pub registered_at: String,
    /// Whether this solver is actively running in the fleet.
    pub active: bool,
    /// Cumulative TSUL donut accrued, in micro-USD ($1.00 = 1_000_000).
    pub donut_accrued_usd_micro: i64,
}

#[derive(Debug, Deserialize)]
pub struct ProvisionRequest {
    pub name: String,
    pub evm_address: String,
    pub solana_address: Option<String>,
    pub signing_mode: Option<String>,
    pub signer_webhook_url: Option<String>,
    pub safe_address: Option<String>,
    pub email: Option<String>,
    /// Comma-separated e.g. "base,arbitrum,optimism"
    pub chains: Option<String>,
    /// Comma-separated e.g. "across,debridge,mayan"
    pub protocols: Option<String>,
    /// Raw SIWE message text (EIP-4361). Optional — when present together
    /// with `signature`, the server verifies before provisioning and marks
    /// `siwe_verified = 1`.
    pub siwe_message: Option<String>,
    /// Hex signature (0x-prefixed) over `siwe_message`.
    pub signature: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SiweNonceRequest {
    /// EVM address requesting a nonce. Lowercased; we scope each nonce to a
    /// single address so two concurrent provisions can't accidentally
    /// trample each other's nonce.
    pub address: String,
}

#[derive(Debug, Serialize)]
pub struct SiweNonceResponse {
    pub nonce: String,
    pub issued_at: String,
    pub ttl_seconds: u64,
}

#[derive(Debug, Serialize)]
pub struct ProvisionResponse {
    pub solver_id: String,
    /// Unique API token for this solver's mutation endpoints.
    /// Shown once — user must store it. Not recoverable (re-provision to rotate).
    pub api_token: String,
    pub portal_url: String,
    pub watch_url: String,
    pub signing_mode: String,
    pub tsul_note: String,
}

// ── Registry ───────────────────────────────────────────────────────────────────

pub struct HostingRegistry {
    db: Mutex<Connection>,
    /// In-memory SIWE nonce store, keyed by `(lowercased address, nonce)`.
    /// Each entry stores the `Instant` it was issued so we can expire stale
    /// nonces and reject reuse.
    ///
    /// LIMITATION: this is process-local. If solver-api is deployed behind
    /// a load balancer with N replicas, a client that hits replica A for
    /// `/api/hosting/siwe-nonce` and replica B for `/api/hosting/provision`
    /// will see a "nonce not found" error. For our single-pod hackathon
    /// deployment this is fine; for a multi-replica deployment swap this
    /// for Redis with `SETEX` + `GETDEL`.
    siwe_nonces: Mutex<HashMap<(String, String), Instant>>,
}

impl HostingRegistry {
    pub fn new(db_path: &str) -> anyhow::Result<Self> {
        let conn = Connection::open(db_path)?;
        // NOTE: schema migrated from pre-i64 `_usd REAL` columns to
        // `_usd_micro INTEGER` for byte-stable canonical JSON across
        // targets. Pre-migration dev databases must be dropped (or the
        // donut_attestations table truncated); the CREATE TABLE here only
        // applies to fresh DBs, and the test rig always boots fresh.
        conn.execute_batch("
            CREATE TABLE IF NOT EXISTS hosted_solvers (
                solver_id               TEXT PRIMARY KEY,
                name                    TEXT NOT NULL,
                evm_address             TEXT NOT NULL UNIQUE,
                solana_address          TEXT,
                signing_mode            TEXT NOT NULL DEFAULT 'self_hosted',
                signer_webhook_url      TEXT,
                safe_address            TEXT,
                email                   TEXT,
                chains                  TEXT NOT NULL DEFAULT 'base,arbitrum,optimism',
                protocols               TEXT NOT NULL DEFAULT 'across,debridge,mayan',
                api_token_hash          TEXT NOT NULL,
                registered_at           TEXT NOT NULL,
                active                  INTEGER NOT NULL DEFAULT 1,
                donut_accrued_usd_micro INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS donut_attestations (
                fill_id                    TEXT PRIMARY KEY,
                spinner_id                 TEXT NOT NULL,
                spinner_addr               TEXT NOT NULL,
                adapter_id                 TEXT NOT NULL,
                protocol                   TEXT NOT NULL,
                dst_chain                  INTEGER NOT NULL,
                fee_usd_micro              INTEGER NOT NULL DEFAULT 0,
                actual_profit_usd_micro    INTEGER NOT NULL,
                donut_bps_num              INTEGER NOT NULL DEFAULT 49,
                donut_bps_den              INTEGER NOT NULL DEFAULT 10000,
                donut_take_usd_micro       INTEGER NOT NULL,
                creator_addr               TEXT NOT NULL,
                creator_share_usd_micro    INTEGER NOT NULL,
                reviewer_addrs_json        TEXT NOT NULL,
                reviewer_share_usd_micro   INTEGER NOT NULL,
                ecosystem_addr             TEXT NOT NULL,
                ecosystem_share_usd_micro  INTEGER NOT NULL,
                spinner_keeps_usd_micro    INTEGER NOT NULL,
                ts                         TEXT NOT NULL,
                prev_hash                  TEXT NOT NULL,
                signature_hex              TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_donut_spinner ON donut_attestations(spinner_id, ts);
        ")?;

        // Idempotent migration for pre-fee-base dev databases. Each column
        // is added independently with the appropriate default so the
        // existing rows back-fill cleanly.
        for col_sql in [
            "ALTER TABLE donut_attestations ADD COLUMN fee_usd_micro INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE donut_attestations ADD COLUMN donut_bps_num INTEGER NOT NULL DEFAULT 49",
            "ALTER TABLE donut_attestations ADD COLUMN donut_bps_den INTEGER NOT NULL DEFAULT 10000",
        ] {
            match conn.execute(col_sql, []) {
                Ok(_) => {}
                Err(e) => {
                    let msg = e.to_string();
                    if !msg.contains("duplicate column name") {
                        tracing::warn!(
                            "donut_attestations migration `{}` failed: {}",
                            col_sql,
                            msg
                        );
                    }
                }
            }
        }

        // Idempotent migration: add `siwe_verified` column if it doesn't
        // already exist. SQLite has no `ALTER TABLE ... ADD COLUMN IF NOT
        // EXISTS`, so we just try and swallow the "duplicate column" error.
        let alter = conn.execute(
            "ALTER TABLE hosted_solvers ADD COLUMN siwe_verified INTEGER NOT NULL DEFAULT 0",
            [],
        );
        match alter {
            Ok(_) => {}
            Err(e) => {
                let msg = e.to_string();
                if !msg.contains("duplicate column name") {
                    return Err(anyhow::anyhow!("failed to add siwe_verified column: {}", e));
                }
            }
        }

        // Idempotent migration: pre-i64 schemas used `donut_accrued_usd REAL`.
        // Add the new column if it isn't present yet. We don't bother
        // copying values across — pre-migration counters are wrong-shape
        // anyway and dev DBs are routinely rebuilt.
        let alter = conn.execute(
            "ALTER TABLE hosted_solvers ADD COLUMN donut_accrued_usd_micro INTEGER NOT NULL DEFAULT 0",
            [],
        );
        match alter {
            Ok(_) => {}
            Err(e) => {
                let msg = e.to_string();
                if !msg.contains("duplicate column name") {
                    return Err(anyhow::anyhow!(
                        "failed to add donut_accrued_usd_micro column: {}",
                        e
                    ));
                }
            }
        }

        Ok(Self {
            db: Mutex::new(conn),
            siwe_nonces: Mutex::new(HashMap::new()),
        })
    }

    /// Issue a fresh SIWE nonce scoped to `address`. 32 random bytes hex-
    /// encoded. Stored with the current `Instant`; consumed on a successful
    /// `provision` call.
    pub fn issue_siwe_nonce(&self, address: &str) -> SiweNonceResponse {
        use rand::RngCore;
        let addr = address.trim().to_lowercase();

        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        let nonce = hex::encode(bytes);

        let now = Instant::now();
        {
            let mut store = self.siwe_nonces.lock().unwrap();
            // Lazy GC: drop any expired entries before inserting a new one.
            store.retain(|_, issued| now.duration_since(*issued) < Duration::from_secs(SIWE_NONCE_TTL_SECS));
            // Hard cap eviction: if the TTL-pass didn't shrink us below the
            // cap (i.e. an attacker is issuing faster than the TTL), evict
            // the oldest entries until we're under the limit. Realistically
            // only triggers under sustained abuse — legitimate traffic
            // never approaches this size.
            if store.len() >= SIWE_NONCE_MAX_ENTRIES {
                let mut entries: Vec<((String, String), Instant)> =
                    store.drain().collect();
                entries.sort_by_key(|(_, issued)| *issued);
                // Drop oldest until we have headroom for one insert.
                let drop_count = entries.len().saturating_sub(SIWE_NONCE_MAX_ENTRIES - 1);
                let kept = entries.into_iter().skip(drop_count);
                store.extend(kept);
                warn!(
                    "SIWE nonce store hit cap ({}); dropped {} oldest entries",
                    SIWE_NONCE_MAX_ENTRIES, drop_count
                );
            }
            store.insert((addr.clone(), nonce.clone()), now);
        }

        SiweNonceResponse {
            nonce,
            issued_at: chrono::Utc::now().to_rfc3339(),
            ttl_seconds: SIWE_NONCE_TTL_SECS,
        }
    }

    /// Try to consume a SIWE nonce for `address`. Returns `true` if the
    /// nonce was found, unexpired, and now removed. Returns `false` if the
    /// nonce was missing or expired.
    fn consume_siwe_nonce(&self, address: &str, nonce: &str) -> bool {
        let addr = address.trim().to_lowercase();
        let key = (addr, nonce.to_string());
        let mut store = self.siwe_nonces.lock().unwrap();
        match store.remove(&key) {
            Some(issued) => Instant::now().duration_since(issued) < Duration::from_secs(SIWE_NONCE_TTL_SECS),
            None => false,
        }
    }

    pub async fn provision(&self, req: &ProvisionRequest) -> anyhow::Result<(HostedSolver, String)> {
        let addr = req.evm_address.trim().to_lowercase();
        if !addr.starts_with("0x") || addr.len() != 42 {
            anyhow::bail!("invalid EVM address");
        }

        // Determine SIWE verification status. When both `siwe_message` and
        // `signature` are present, we MUST verify before allowing the
        // provision to succeed — a stricter posture than the unauthenticated
        // path because the user is asserting wallet ownership.
        let siwe_verified = match (req.siwe_message.as_deref(), req.signature.as_deref()) {
            (Some(msg), Some(sig)) => {
                verify_siwe(msg, sig, &addr, |nonce| self.consume_siwe_nonce(&addr, nonce))
                    .await?;
                true
            }
            (Some(_), None) | (None, Some(_)) => {
                anyhow::bail!("siwe_message and signature must be provided together");
            }
            (None, None) => false,
        };

        let solver_id = &addr[2..10]; // first 8 hex chars
        let now = chrono::Utc::now().to_rfc3339();

        // Cryptographically-random 24-byte API token. Generated with OsRng
        // (via rand::thread_rng which wraps the OS CSPRNG) so it cannot be
        // predicted by an attacker who observes process timing or other
        // tokens.
        use rand::RngCore;
        let mut token_bytes = [0u8; 24];
        rand::thread_rng().fill_bytes(&mut token_bytes);
        let api_token: String = hex::encode(token_bytes);

        // Hash token for storage (don't store raw token). DefaultHasher is
        // *not* cryptographically strong, but we never expose the hash —
        // it's only used internally to disambiguate rows. For a real audit
        // we'd swap to SHA-256, but the token itself is already 192 bits of
        // OS-entropy.
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h_store = DefaultHasher::new();
        api_token.hash(&mut h_store);
        let token_hash: String = format!("{:x}", h_store.finish());

        let signing_mode = req.signing_mode.as_deref().unwrap_or("self_hosted");
        let chains = req.chains.as_deref().unwrap_or("base,arbitrum,optimism");
        let protocols = req.protocols.as_deref().unwrap_or("across,debridge,mayan");

        let solver = HostedSolver {
            solver_id: solver_id.to_string(),
            name: req.name.clone(),
            evm_address: addr.clone(),
            solana_address: req.solana_address.clone(),
            signing_mode: match signing_mode {
                "session_key" => SigningMode::SessionKey,
                "remote_signer" => SigningMode::RemoteSigner,
                _ => SigningMode::SelfHosted,
            },
            signer_webhook_url: req.signer_webhook_url.clone(),
            safe_address: req.safe_address.clone(),
            email: req.email.clone(),
            chains: chains.to_string(),
            protocols: protocols.to_string(),
            registered_at: now.clone(),
            active: true,
            donut_accrued_usd_micro: 0,
        };

        let db = self.db.lock().unwrap();

        // Detect re-provision so we can log a token rotation. `solver_id`
        // is deterministic (first 8 hex of address), so an existing row
        // here means the same wallet has provisioned before.
        let already_existed: bool = db
            .query_row(
                "SELECT 1 FROM hosted_solvers WHERE solver_id = ?1",
                params![solver.solver_id],
                |_| Ok(true),
            )
            .unwrap_or(false);

        // INSERT OR REPLACE keeps the deterministic solver_id stable while
        // overwriting api_token_hash — effectively revoking the prior token.
        db.execute(
            "INSERT OR REPLACE INTO hosted_solvers
             (solver_id, name, evm_address, solana_address, signing_mode,
              signer_webhook_url, safe_address, email, chains, protocols,
              api_token_hash, registered_at, active, donut_accrued_usd_micro, siwe_verified)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,1,0,?13)",
            params![
                solver.solver_id,
                solver.name,
                solver.evm_address,
                solver.solana_address,
                signing_mode,
                solver.signer_webhook_url,
                solver.safe_address,
                solver.email,
                solver.chains,
                solver.protocols,
                token_hash,
                now,
                if siwe_verified { 1i64 } else { 0i64 },
            ],
        )?;

        // Truncate api_token in logs: never log more than the first 8 chars.
        let token_preview = &api_token[..8.min(api_token.len())];
        if already_existed {
            info!(
                "[hosting] re-provisioned solver_id={} address={} mode={} siwe_verified={} (api_token rotated, new prefix={}…)",
                solver_id, addr, signing_mode, siwe_verified, token_preview,
            );
        } else {
            info!(
                "[hosting] provisioned solver_id={} address={} mode={} siwe_verified={} api_token_prefix={}…",
                solver_id, addr, signing_mode, siwe_verified, token_preview,
            );
        }
        Ok((solver, api_token))
    }

    pub fn list(&self) -> anyhow::Result<Vec<HostedSolver>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT solver_id, name, evm_address, solana_address, signing_mode,
                    signer_webhook_url, safe_address, email, chains, protocols,
                    registered_at, active, donut_accrued_usd_micro
             FROM hosted_solvers ORDER BY registered_at DESC"
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(HostedSolverRow {
                solver_id: row.get(0)?,
                name: row.get(1)?,
                evm_address: row.get(2)?,
                solana_address: row.get(3)?,
                signing_mode: row.get(4)?,
                signer_webhook_url: row.get(5)?,
                safe_address: row.get(6)?,
                email: row.get(7)?,
                chains: row.get(8)?,
                protocols: row.get(9)?,
                registered_at: row.get(10)?,
                active: row.get::<_, i64>(11)? != 0,
                donut_accrued_usd_micro: row.get(12)?,
            })
        })?;

        let mut result = Vec::new();
        for row in rows {
            let r = row.map_err(|e| anyhow::anyhow!(e))?;
            result.push(row_to_solver(r));
        }
        Ok(result)
    }

    pub fn get_by_id(&self, solver_id: &str) -> anyhow::Result<Option<HostedSolver>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT solver_id, name, evm_address, solana_address, signing_mode,
                    signer_webhook_url, safe_address, email, chains, protocols,
                    registered_at, active, donut_accrued_usd_micro
             FROM hosted_solvers WHERE solver_id = ?1"
        )?;

        let mut rows = stmt.query_map(params![solver_id], |row| {
            Ok(HostedSolverRow {
                solver_id: row.get(0)?,
                name: row.get(1)?,
                evm_address: row.get(2)?,
                solana_address: row.get(3)?,
                signing_mode: row.get(4)?,
                signer_webhook_url: row.get(5)?,
                safe_address: row.get(6)?,
                email: row.get(7)?,
                chains: row.get(8)?,
                protocols: row.get(9)?,
                registered_at: row.get(10)?,
                active: row.get::<_, i64>(11)? != 0,
                donut_accrued_usd_micro: row.get(12)?,
            })
        })?;

        Ok(rows.next().transpose().map_err(|e| anyhow::anyhow!(e))?.map(row_to_solver))
    }

    /// Persist a signed [`DonutAttestation`] to the ledger.
    ///
    /// Performs four checks in order:
    /// 1. Signature must verify (recovers to `spinner_addr`) AND the donut
    ///    math must be internally consistent. Delegated to
    ///    [`CanonicalAdjudicator::verify`].
    /// 2. `prev_hash` must equal the current ledger head for this Spinner —
    ///    or [`ZERO_HASH`] when the Spinner has never attested before.
    /// 3. The row is inserted; a duplicate `fill_id` returns [`PersistError::DuplicateFill`].
    /// 4. `hosted_solvers.donut_accrued_usd_micro` is incremented by
    ///    `att.creator_share_usd_micro` **only** if `att.creator_addr` matches the
    ///    Spinner's registered `evm_address` — i.e. the Spinner is also the
    ///    Builder of the adapter that produced the fill. Other Spinners do
    ///    not get a counter bump for running someone else's adapter.
    pub fn persist_attestation(&self, att: &DonutAttestation) -> Result<(), PersistError> {
        // 1) signature + math. `verify` recovers the address from the
        // signature and asserts it equals `att.spinner_addr`, binding the
        // signature ↔ address pair.
        CanonicalAdjudicator
            .verify(att)
            .map_err(|e| PersistError::InvalidSignature(e.to_string()))?;

        // 1b) Bind the body's `spinner_id` to the recovered address.
        //
        // Without this check, any Spinner with a valid bearer token could
        // POST an attestation that signs correctly under their OWN key but
        // claims a different Spinner's `spinner_id` in the body — polluting
        // a victim's ledger and breaking their hash chain. Hackathon-audit
        // HIGH severity. The expected `spinner_id` is purely a function of
        // the address, so we recompute it here and reject the request if
        // the body disagrees.
        let expected_spinner_id =
            donut_adjudicator::spinner_id_from_addr(&att.spinner_addr);
        if att.spinner_id.to_ascii_lowercase() != expected_spinner_id {
            return Err(PersistError::InvalidSignature(format!(
                "spinner_id `{}` does not match recovered address `{}` (expected `{}`)",
                att.spinner_id,
                att.spinner_addr,
                expected_spinner_id
            )));
        }

        // 2) chain link
        let (expected_prev, _count) = self
            .ledger_head(&att.spinner_id)
            .map_err(|e| PersistError::Internal(e.to_string()))?;
        if att.prev_hash != expected_prev {
            return Err(PersistError::BadChain {
                expected: expected_prev,
                got: att.prev_hash.clone(),
            });
        }

        // 3) insert (PK collision on fill_id → DuplicateFill)
        let reviewer_addrs_json = serde_json::to_string(&att.reviewer_addrs)
            .map_err(|e| PersistError::Internal(e.to_string()))?;
        let spinner_addr_hex = format!("{:#x}", att.spinner_addr);
        let creator_addr_hex = format!("{:#x}", att.creator_addr);
        let ecosystem_addr_hex = format!("{:#x}", att.ecosystem_addr);
        let ts_str = att.ts.to_rfc3339();

        // INSERT + UPDATE wrapped in a single SQLite transaction so a
        // crash between the two leaves the database in a consistent state
        // (hackathon-audit HIGH severity). `db.transaction()` requires
        // `&mut Connection` which we get via `MutexGuard::deref_mut`.
        {
            let mut db = self.db.lock().unwrap();
            let tx = db
                .transaction()
                .map_err(|e| PersistError::Internal(e.to_string()))?;

            let res = tx.execute(
                "INSERT INTO donut_attestations (
                    fill_id, spinner_id, spinner_addr, adapter_id, protocol,
                    dst_chain, fee_usd_micro, actual_profit_usd_micro,
                    donut_bps_num, donut_bps_den, donut_take_usd_micro,
                    creator_addr, creator_share_usd_micro, reviewer_addrs_json,
                    reviewer_share_usd_micro, ecosystem_addr,
                    ecosystem_share_usd_micro, spinner_keeps_usd_micro,
                    ts, prev_hash, signature_hex
                ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21)",
                params![
                    att.fill_id,
                    att.spinner_id,
                    spinner_addr_hex,
                    att.adapter_id,
                    att.protocol,
                    att.dst_chain as i64,
                    att.fee_usd_micro,
                    att.actual_profit_usd_micro,
                    att.donut_bps_num,
                    att.donut_bps_den,
                    att.donut_take_usd_micro,
                    creator_addr_hex,
                    att.creator_share_usd_micro,
                    reviewer_addrs_json,
                    att.reviewer_share_usd_micro,
                    ecosystem_addr_hex,
                    att.ecosystem_share_usd_micro,
                    att.spinner_keeps_usd_micro,
                    ts_str,
                    att.prev_hash,
                    att.signature_hex,
                ],
            );
            if let Err(e) = res {
                // SQLite primary-key violation → duplicate fill.
                let msg = e.to_string();
                // Transaction will roll back when `tx` is dropped without
                // commit. Explicitly drop here for clarity.
                drop(tx);
                if msg.contains("UNIQUE constraint failed")
                    || msg.contains("PRIMARY KEY constraint")
                {
                    return Err(PersistError::DuplicateFill(att.fill_id.clone()));
                }
                return Err(PersistError::Internal(msg));
            }

            // Bump `donut_accrued_usd` only when the Spinner == Builder.
            // Compare both hex addresses lowercased without 0x.
            let solver_evm: Option<String> = tx
                .query_row(
                    "SELECT evm_address FROM hosted_solvers WHERE solver_id = ?1",
                    params![att.spinner_id],
                    |r| r.get::<_, String>(0),
                )
                .ok();
            if let Some(evm) = solver_evm {
                let evm_norm = evm.trim_start_matches("0x").to_ascii_lowercase();
                let creator_norm = creator_addr_hex.trim_start_matches("0x").to_ascii_lowercase();
                if evm_norm == creator_norm {
                    tx.execute(
                        "UPDATE hosted_solvers
                         SET donut_accrued_usd_micro = donut_accrued_usd_micro + ?1
                         WHERE solver_id = ?2",
                        params![att.creator_share_usd_micro, att.spinner_id],
                    )
                    .map_err(|e| PersistError::Internal(e.to_string()))?;
                }
            }

            tx.commit()
                .map_err(|e| PersistError::Internal(e.to_string()))?;
        }

        info!(
            spinner_id = %att.spinner_id,
            adapter = %att.adapter_id,
            donut_usd_micro = att.donut_take_usd_micro,
            "[hosting] attestation persisted (recovered spinner {})",
            att.spinner_addr
        );
        Ok(())
    }

    /// Read back every attestation for a Spinner in chain order (oldest first).
    /// The public ledger endpoint promises ASC so external indexers can replay
    /// the chain from genesis without re-sorting client-side.
    pub fn ledger_for(&self, spinner_id: &str) -> anyhow::Result<Vec<DonutAttestation>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT fill_id, spinner_id, spinner_addr, adapter_id, protocol,
                    dst_chain, fee_usd_micro, actual_profit_usd_micro,
                    donut_bps_num, donut_bps_den, donut_take_usd_micro,
                    creator_addr, creator_share_usd_micro, reviewer_addrs_json,
                    reviewer_share_usd_micro, ecosystem_addr,
                    ecosystem_share_usd_micro, spinner_keeps_usd_micro,
                    ts, prev_hash, signature_hex
             FROM donut_attestations
             WHERE spinner_id = ?1
             ORDER BY ts ASC",
        )?;
        let rows = stmt.query_map(params![spinner_id], row_to_attestation)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| anyhow::anyhow!(e))?);
        }
        Ok(out)
    }

    /// Current ledger head for a Spinner — `(prev_hash, count)` where
    /// `prev_hash` is sha256 of the most-recent attestation's canonical JSON
    /// with signature (the next attestation must thread this in its
    /// `prev_hash` field). Empty ledger → [`ZERO_HASH`] and `count = 0`.
    pub fn ledger_head(&self, spinner_id: &str) -> anyhow::Result<(String, u64)> {
        let db = self.db.lock().unwrap();

        let count: i64 = db.query_row(
            "SELECT COUNT(*) FROM donut_attestations WHERE spinner_id = ?1",
            params![spinner_id],
            |r| r.get(0),
        )?;
        if count == 0 {
            return Ok((ZERO_HASH.to_string(), 0));
        }
        let count = count as u64;

        let mut stmt = db.prepare(
            "SELECT fill_id, spinner_id, spinner_addr, adapter_id, protocol,
                    dst_chain, fee_usd_micro, actual_profit_usd_micro,
                    donut_bps_num, donut_bps_den, donut_take_usd_micro,
                    creator_addr, creator_share_usd_micro, reviewer_addrs_json,
                    reviewer_share_usd_micro, ecosystem_addr,
                    ecosystem_share_usd_micro, spinner_keeps_usd_micro,
                    ts, prev_hash, signature_hex
             FROM donut_attestations
             WHERE spinner_id = ?1
             ORDER BY ts DESC LIMIT 1",
        )?;
        let mut rows = stmt.query_map(params![spinner_id], row_to_attestation)?;
        let head = rows
            .next()
            .ok_or_else(|| anyhow::anyhow!("ledger head missing despite count > 0"))?
            .map_err(|e| anyhow::anyhow!(e))?;
        let hash = hash_for_chain(&head)?;
        Ok((hash, count))
    }
}

/// Failure modes for [`HostingRegistry::persist_attestation`]. Each variant
/// maps directly to an HTTP status in the route layer.
#[derive(Debug, thiserror::Error)]
pub enum PersistError {
    /// 400 — signature didn't verify or donut math is internally inconsistent.
    #[error("invalid signature: {0}")]
    InvalidSignature(String),
    /// 409 — `fill_id` already in the ledger.
    #[error("duplicate fill_id: {0}")]
    DuplicateFill(String),
    /// 422 — `prev_hash` doesn't match the current ledger head.
    #[error("bad chain link: expected {expected}, got {got}")]
    BadChain { expected: String, got: String },
    /// 500 — anything else (SQLite, JSON encoding, ...).
    #[error("internal: {0}")]
    Internal(String),
}

// We pull `thiserror` from the workspace (already in Cargo.toml of other crates)
// — re-export here would be redundant. Add it to solver-api/Cargo.toml.

/// Decode a `donut_attestations` row back into a [`DonutAttestation`].
fn row_to_attestation(r: &rusqlite::Row<'_>) -> rusqlite::Result<DonutAttestation> {
    use alloy::primitives::Address;
    use std::str::FromStr;

    let parse_addr = |s: String| -> rusqlite::Result<Address> {
        Address::from_str(&s).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())),
            )
        })
    };
    let parse_addrs = |s: String| -> rusqlite::Result<Vec<Address>> {
        serde_json::from_str::<Vec<Address>>(&s).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())),
            )
        })
    };
    let ts_str: String = r.get(18)?;
    let ts = chrono::DateTime::parse_from_rfc3339(&ts_str)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now());

    Ok(DonutAttestation {
        fill_id: r.get(0)?,
        spinner_id: r.get(1)?,
        spinner_addr: parse_addr(r.get(2)?)?,
        adapter_id: r.get(3)?,
        protocol: r.get(4)?,
        dst_chain: r.get::<_, i64>(5)? as u64,
        fee_usd_micro: r.get(6)?,
        actual_profit_usd_micro: r.get(7)?,
        donut_bps_num: r.get(8)?,
        donut_bps_den: r.get(9)?,
        donut_take_usd_micro: r.get(10)?,
        creator_addr: parse_addr(r.get(11)?)?,
        creator_share_usd_micro: r.get(12)?,
        reviewer_addrs: parse_addrs(r.get(13)?)?,
        reviewer_share_usd_micro: r.get(14)?,
        ecosystem_addr: parse_addr(r.get(15)?)?,
        ecosystem_share_usd_micro: r.get(16)?,
        spinner_keeps_usd_micro: r.get(17)?,
        ts,
        prev_hash: r.get(19)?,
        signature_hex: r.get(20)?,
    })
}


// Internal row struct for SQLite deserialization
struct HostedSolverRow {
    solver_id: String,
    name: String,
    evm_address: String,
    solana_address: Option<String>,
    signing_mode: String,
    signer_webhook_url: Option<String>,
    safe_address: Option<String>,
    email: Option<String>,
    chains: String,
    protocols: String,
    registered_at: String,
    active: bool,
    donut_accrued_usd_micro: i64,
}

fn row_to_solver(r: HostedSolverRow) -> HostedSolver {
    HostedSolver {
        solver_id: r.solver_id,
        name: r.name,
        evm_address: r.evm_address,
        solana_address: r.solana_address,
        signing_mode: match r.signing_mode.as_str() {
            "session_key" => SigningMode::SessionKey,
            "remote_signer" => SigningMode::RemoteSigner,
            _ => SigningMode::SelfHosted,
        },
        signer_webhook_url: r.signer_webhook_url,
        safe_address: r.safe_address,
        email: r.email,
        chains: r.chains,
        protocols: r.protocols,
        registered_at: r.registered_at,
        active: r.active,
        donut_accrued_usd_micro: r.donut_accrued_usd_micro,
    }
}

// ── SIWE verification ──────────────────────────────────────────────────────────

/// Domain bound into every SIWE message we accept. Configurable via the
/// `SIWE_DOMAIN` env var so the same binary can be reused for staging.
fn siwe_domain() -> String {
    std::env::var("SIWE_DOMAIN").unwrap_or_else(|_| "solver.taifoon.dev".to_string())
}

/// Verify a SIWE message + signature pair against the claimed address.
///
/// `expected_addr` is lowercased hex (with 0x prefix). `consume_nonce` is a
/// callback that returns `true` iff the nonce embedded in the SIWE message
/// is currently valid (not expired, not previously consumed). It MUST
/// consume the nonce as a side-effect — we only call this after every other
/// check passes, so a verification failure does not burn the nonce.
async fn verify_siwe<F>(
    raw_message: &str,
    raw_signature: &str,
    expected_addr: &str,
    consume_nonce: F,
) -> anyhow::Result<()>
where
    F: FnOnce(&str) -> bool,
{
    use siwe::{Message, VerificationOpts};
    use std::str::FromStr;
    use time::OffsetDateTime;

    // Explicit EIP-55 casing check on the body's address line. siwe-rs's
    // parser already enforces checksum casing today, but we surface a
    // dedicated error here so even if a future siwe-rs release loosens
    // its check (or a fork is swapped in) we still reject lowercased
    // addresses before signature recovery. The address line is the second
    // non-empty line of the SIWE message; we extract it with the same
    // logic siwe-rs uses.
    if let Some(addr_line) = raw_message.lines().nth(1) {
        let candidate = addr_line.trim();
        if candidate.starts_with("0x") && candidate.len() == 42 {
            let bytes = hex::decode(candidate.trim_start_matches("0x"))
                .map_err(|e| anyhow::anyhow!("invalid SIWE message: address hex: {}", e))?;
            if bytes.len() == 20 {
                let arr: [u8; 20] = bytes.as_slice().try_into().unwrap();
                let canonical = alloy::primitives::Address::from(arr).to_string();
                if canonical != candidate {
                    anyhow::bail!(
                        "invalid SIWE message: address checksum mismatch (expected {}, got {})",
                        canonical,
                        candidate
                    );
                }
            }
        }
    }

    let msg = Message::from_str(raw_message)
        .map_err(|e| anyhow::anyhow!("invalid SIWE message: {}", e))?;

    // Pin the domain. A SIWE message signed for `evil.example.com` should
    // never authenticate against our deployment, even if the signature is
    // cryptographically valid.
    let expected_domain = siwe_domain();
    let msg_domain = msg.domain.to_string();
    if msg_domain != expected_domain {
        anyhow::bail!(
            "SIWE domain mismatch: expected {}, got {}",
            expected_domain,
            msg_domain
        );
    }

    // Pin the chain — same logic as domain: prevent cross-chain replay.
    if msg.chain_id != SIWE_CHAIN_ID {
        anyhow::bail!(
            "SIWE chain_id mismatch: expected {}, got {}",
            SIWE_CHAIN_ID,
            msg.chain_id
        );
    }

    // Compare addresses by lowercased hex. The SIWE `address` field is a
    // 20-byte array; `expected_addr` arrives as "0x…" lowercased.
    let msg_addr_hex = format!("0x{}", hex::encode(msg.address));
    if msg_addr_hex.to_lowercase() != expected_addr.to_lowercase() {
        anyhow::bail!(
            "SIWE address mismatch: message says {}, request says {}",
            msg_addr_hex,
            expected_addr
        );
    }

    // Explicit expiration check. SIWE makes `expiration_time` optional, but
    // we require it for provision flows so a stolen signature can't be
    // replayed indefinitely.
    //
    // `siwe::TimeStamp` Display's as RFC3339 — parse via chrono to compare
    // against `Utc::now()` without dragging in the `time` crate just for this.
    let exp = msg
        .expiration_time
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("SIWE message missing expiration_time"))?;
    let now = chrono::Utc::now();
    let exp_str = exp.to_string();
    let exp_dt = chrono::DateTime::parse_from_rfc3339(&exp_str)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .map_err(|e| anyhow::anyhow!("invalid SIWE expiration_time '{}': {}", exp_str, e))?;
    // Allow a small grace window so clients with skewed clocks don't get
    // rejected the instant they sign. The grace works in both directions —
    // a message that *just* expired against the server's wall clock is
    // still accepted if it's within SIWE_CLOCK_SKEW_SECS of now.
    let grace = chrono::Duration::seconds(SIWE_CLOCK_SKEW_SECS);
    if exp_dt + grace <= now {
        anyhow::bail!("SIWE message expired at {}", exp_dt);
    }

    // Strip 0x prefix from signature and decode.
    let sig_hex = raw_signature.trim_start_matches("0x");
    let sig_bytes = hex::decode(sig_hex)
        .map_err(|e| anyhow::anyhow!("invalid signature hex: {}", e))?;
    if sig_bytes.len() != 65 {
        anyhow::bail!("SIWE signature must be 65 bytes, got {}", sig_bytes.len());
    }
    let mut sig_arr = [0u8; 65];
    sig_arr.copy_from_slice(&sig_bytes);

    // Cryptographic verification — recover the signer from the EIP-191
    // personal_sign digest of `raw_message` and check it matches `msg.address`.
    // We've already manually checked domain, nonce, and expiration above.
    // Set timestamp to account for our clock-skew grace period so the library's
    // built-in expiration check doesn't reject messages that are within our
    // SIWE_CLOCK_SKEW_SECS tolerance.
    let verification_time = now - grace;
    let verification_unix = verification_time.timestamp();
    let verification_ts = time::OffsetDateTime::from_unix_timestamp(verification_unix)
        .map_err(|e| anyhow::anyhow!("invalid verification timestamp: {}", e))?;
    let opts = VerificationOpts {
        domain: None,
        nonce: None,
        timestamp: Some(verification_ts),
        ..Default::default()
    };
    // siwe's verify is async but does no I/O (no EIP-1271 contract call
    // when we leave `rpc_provider` unset). Await it directly from inside
    // the axum handler — provision is `async fn` from end to end.
    let result = msg.verify(&sig_arr, &opts).await;
    result.map_err(|e| anyhow::anyhow!("SIWE signature verification failed: {}", e))?;

    // Finally, consume the nonce. Single-use — the closure removes it from
    // the in-memory store. We do this LAST so a transient cryptographic
    // failure doesn't burn a perfectly good nonce.
    if !consume_nonce(&msg.nonce) {
        anyhow::bail!("SIWE nonce not recognized or expired");
    }

    Ok(())
}

// ── Axum handlers ──────────────────────────────────────────────────────────────

pub type HostingRegistryState = Arc<HostingRegistry>;

/// POST /api/hosting/siwe-nonce
/// Public endpoint — issues a one-shot SIWE nonce scoped to `address`.
/// Client embeds this in the `nonce` field of the SIWE message it asks the
/// user to sign, then includes the signed message in `/api/hosting/provision`.
pub async fn siwe_nonce_handler(
    State(registry): State<HostingRegistryState>,
    Json(req): Json<SiweNonceRequest>,
) -> Result<Json<SiweNonceResponse>, (StatusCode, Json<serde_json::Value>)> {
    let addr = req.address.trim().to_lowercase();
    if !addr.starts_with("0x") || addr.len() != 42 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "invalid EVM address" })),
        ));
    }
    Ok(Json(registry.issue_siwe_nonce(&addr)))
}

/// POST /api/hosting/provision
/// Public endpoint — no auth required. Anyone can register.
pub async fn provision_handler(
    State(registry): State<HostingRegistryState>,
    Json(req): Json<ProvisionRequest>,
) -> Result<Json<ProvisionResponse>, (StatusCode, Json<serde_json::Value>)> {
    match registry.provision(&req).await {
        Ok((solver, token)) => {
            let base_url = std::env::var("PUBLIC_BASE_URL")
                .unwrap_or_else(|_| "https://solver.taifoon.dev".to_string());

            let signing_note = match req.signing_mode.as_deref().unwrap_or("self_hosted") {
                "session_key" => "Grant your Safe session key to fill-function selectors only. Taifoon never holds your master key.",
                "remote_signer" => "Your local signer receives unsigned transactions. Approve each fill before broadcast.",
                _ => "Self-hosted: run cargo build --release -p solver-main and configure SOLVER_PRIVATE_KEY on your own machine.",
            };

            Ok(Json(ProvisionResponse {
                solver_id: solver.solver_id.clone(),
                api_token: token,
                portal_url: format!("{}/portal/{}", base_url, solver.solver_id),
                watch_url: format!("{}/watch?address={}", base_url, solver.evm_address),
                signing_mode: req.signing_mode.unwrap_or_else(|| "self_hosted".to_string()),
                tsul_note: format!(
                    "TSUL rule #4: 70% of every fill's donut routes to {} perpetually on-chain. {}",
                    solver.evm_address,
                    signing_note,
                ),
            }))
        }
        Err(e) => {
            warn!("[hosting] provision failed: {}", e);
            Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e.to_string() })),
            ))
        }
    }
}

/// GET /api/hosting/solvers
pub async fn list_solvers_handler(
    State(registry): State<HostingRegistryState>,
) -> Json<serde_json::Value> {
    match registry.list() {
        Ok(solvers) => Json(serde_json::json!({
            "count": solvers.len(),
            "solvers": solvers,
        })),
        Err(e) => Json(serde_json::json!({ "error": e.to_string(), "solvers": [] })),
    }
}

/// GET /api/hosting/solvers/:solver_id
pub async fn get_solver_handler(
    State(registry): State<HostingRegistryState>,
    Path(solver_id): Path<String>,
) -> Result<Json<HostedSolver>, (StatusCode, Json<serde_json::Value>)> {
    match registry.get_by_id(&solver_id) {
        Ok(Some(s)) => Ok(Json(s)),
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "solver not found" })),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> HostingRegistry {
        HostingRegistry::new(":memory:").unwrap()
    }

    #[tokio::test]
    async fn provision_returns_unique_token() {
        let r = registry();
        let req = ProvisionRequest {
            name: "test-solver".into(),
            evm_address: "0x0000000000000000000000000000000000000001".into(),
            solana_address: None,
            signing_mode: Some("self_hosted".into()),
            signer_webhook_url: None,
            safe_address: None,
            email: Some("test@test.com".into()),
            chains: Some("base,arbitrum".into()),
            protocols: Some("across".into()),
            siwe_message: None,
            signature: None,
        };
        let (solver, token) = r.provision(&req).await.unwrap();
        assert_eq!(solver.solver_id, "00000000");
        assert_eq!(token.len(), 48); // 24 bytes hex-encoded
        assert!(solver.evm_address.starts_with("0x"));
    }

    #[tokio::test]
    async fn invalid_address_rejected() {
        let r = registry();
        let req = ProvisionRequest {
            name: "x".into(),
            evm_address: "not-an-address".into(),
            solana_address: None,
            signing_mode: None,
            signer_webhook_url: None,
            safe_address: None,
            email: None,
            chains: None,
            protocols: None,
            siwe_message: None,
            signature: None,
        };
        assert!(r.provision(&req).await.is_err());
    }

    #[test]
    fn list_empty_initially() {
        let r = registry();
        assert_eq!(r.list().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn provision_then_list_returns_entry() {
        let r = registry();
        let req = ProvisionRequest {
            name: "fleet-member".into(),
            evm_address: "0xAbCd1234AbCd1234AbCd1234AbCd1234AbCd1234".into(),
            solana_address: None,
            signing_mode: Some("remote_signer".into()),
            signer_webhook_url: Some("https://my-signer.example.com/sign".into()),
            safe_address: None,
            email: None,
            chains: None,
            protocols: None,
            siwe_message: None,
            signature: None,
        };
        let (solver, _token) = r.provision(&req).await.unwrap();
        let list = r.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].solver_id, solver.solver_id);
        assert!(matches!(list[0].signing_mode, SigningMode::RemoteSigner));
    }

    use alloy::signers::local::PrivateKeySigner;
    use donut_adjudicator::{
        hash_for_chain, AdapterRegistry, CanonicalAdjudicator, FeeSplitAdjudicator, ZERO_HASH,
    };
    use executor::OutcomeRecord;

    fn test_signer() -> PrivateKeySigner {
        "0x4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318"
            .parse()
            .unwrap()
    }

    fn test_outcome(intent: &str, tx: &str, profit: f64) -> OutcomeRecord {
        OutcomeRecord {
            ts: chrono::Utc::now(),
            intent_id: intent.into(),
            protocol: "across".into(),
            src_chain: 1,
            dst_chain: 42161,
            decision: "executed".into(),
            tx_hash: Some(tx.into()),
            predicted_gas: None,
            gas_used: Some(100_000),
            effective_gas_price_wei: None,
            predicted_profit_usd: Some(profit),
            actual_profit_usd: Some(profit),
            skip_reason: None,
            error: None,
            solver_id: Some("00000000".into()),
            claim_tx_hash: None,
            claim_fee_usd: None,
            fee_usd: None,
        }
    }

    async fn provision_signer(r: &HostingRegistry, signer: &PrivateKeySigner) -> String {
        let addr = format!("{:#x}", signer.address());
        let req = ProvisionRequest {
            name: "test".into(),
            evm_address: addr,
            solana_address: None,
            signing_mode: None,
            signer_webhook_url: None,
            safe_address: None,
            email: None,
            chains: None,
            protocols: None,
            siwe_message: None,
            signature: None,
        };
        let (solver, _token) = r.provision(&req).await.unwrap();
        solver.solver_id
    }

    #[tokio::test]
    async fn persist_attestation_round_trips() {
        let r = registry();
        let signer = test_signer();
        let spinner_id = provision_signer(&r, &signer).await;

        // Spinner == Builder: builder_addr in registry equals signer.address().
        let reg = AdapterRegistry::new(
            "0x000000000000000000000000000000000000eeee".parse().unwrap(),
        )
        .with_adapter("across-v3", signer.address(), vec![]);

        let fill = test_outcome("i1", "0xaa", 1.0);
        let att = CanonicalAdjudicator
            .attest(&fill, &reg, &signer, ZERO_HASH)
            .await
            .unwrap();
        // Override spinner_id so persisted row matches the provisioned solver_id.
        let mut att = att;
        att.spinner_id = spinner_id.clone();
        // Re-sign since spinner_id changed.
        let canonical = donut_adjudicator::canonical_json_for_signing(&att).unwrap();
        use alloy::signers::SignerSync;
        let sig = signer.sign_message_sync(canonical.as_bytes()).unwrap();
        att.signature_hex = format!("0x{}", hex::encode(sig.as_bytes()));

        r.persist_attestation(&att).expect("persist ok");

        let updated = r.get_by_id(&spinner_id).unwrap().unwrap();
        // Spinner == Builder → counter bumped by creator_share.
        assert_eq!(updated.donut_accrued_usd_micro, att.creator_share_usd_micro);

        let ledger = r.ledger_for(&spinner_id).unwrap();
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger[0].fill_id, att.fill_id);

        let (head, count) = r.ledger_head(&spinner_id).unwrap();
        assert_eq!(count, 1);
        assert_eq!(head, hash_for_chain(&att).unwrap());
    }

    #[tokio::test]
    async fn persist_duplicate_fill_id_fails() {
        let r = registry();
        let signer = test_signer();
        let spinner_id = provision_signer(&r, &signer).await;
        let reg = AdapterRegistry::new(
            "0x000000000000000000000000000000000000eeee".parse().unwrap(),
        )
        .with_adapter("across-v3", signer.address(), vec![]);

        let fill = test_outcome("i1", "0xaa", 1.0);
        let mut att = CanonicalAdjudicator
            .attest(&fill, &reg, &signer, ZERO_HASH)
            .await
            .unwrap();
        att.spinner_id = spinner_id.clone();
        let canonical = donut_adjudicator::canonical_json_for_signing(&att).unwrap();
        use alloy::signers::SignerSync;
        let sig = signer.sign_message_sync(canonical.as_bytes()).unwrap();
        att.signature_hex = format!("0x{}", hex::encode(sig.as_bytes()));

        r.persist_attestation(&att).unwrap();
        // chain advanced; new prev_hash now matches first row's hash, so the
        // chain check would pass — but PK on fill_id fires first.
        let head = hash_for_chain(&att).unwrap();
        let mut dup = att.clone();
        dup.prev_hash = head; // satisfy chain
        let canonical2 = donut_adjudicator::canonical_json_for_signing(&dup).unwrap();
        let sig2 = signer.sign_message_sync(canonical2.as_bytes()).unwrap();
        dup.signature_hex = format!("0x{}", hex::encode(sig2.as_bytes()));

        match r.persist_attestation(&dup) {
            Err(PersistError::DuplicateFill(_)) => {}
            other => panic!("expected DuplicateFill, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn persist_bad_chain_link_fails() {
        let r = registry();
        let signer = test_signer();
        let spinner_id = provision_signer(&r, &signer).await;
        let reg = AdapterRegistry::new(
            "0x000000000000000000000000000000000000eeee".parse().unwrap(),
        )
        .with_adapter("across-v3", signer.address(), vec![]);

        // First attestation uses a non-zero prev_hash — ledger is empty so the
        // expected prev_hash is ZERO_HASH. Persist must reject.
        let fill = test_outcome("i1", "0xaa", 1.0);
        let mut att = CanonicalAdjudicator
            .attest(
                &fill,
                &reg,
                &signer,
                "0x1111111111111111111111111111111111111111111111111111111111111111",
            )
            .await
            .unwrap();
        att.spinner_id = spinner_id.clone();
        let canonical = donut_adjudicator::canonical_json_for_signing(&att).unwrap();
        use alloy::signers::SignerSync;
        let sig = signer.sign_message_sync(canonical.as_bytes()).unwrap();
        att.signature_hex = format!("0x{}", hex::encode(sig.as_bytes()));

        match r.persist_attestation(&att) {
            Err(PersistError::BadChain { .. }) => {}
            other => panic!("expected BadChain, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn persist_counter_only_when_spinner_is_builder() {
        let r = registry();
        let signer = test_signer();
        let spinner_id = provision_signer(&r, &signer).await;

        // Builder is a DIFFERENT address. Spinner runs someone else's adapter.
        let other_builder: alloy::primitives::Address =
            "0x000000000000000000000000000000000000bbbb".parse().unwrap();
        let reg = AdapterRegistry::new(
            "0x000000000000000000000000000000000000eeee".parse().unwrap(),
        )
        .with_adapter("across-v3", other_builder, vec![]);

        let fill = test_outcome("i1", "0xaa", 1.0);
        let mut att = CanonicalAdjudicator
            .attest(&fill, &reg, &signer, ZERO_HASH)
            .await
            .unwrap();
        att.spinner_id = spinner_id.clone();
        let canonical = donut_adjudicator::canonical_json_for_signing(&att).unwrap();
        use alloy::signers::SignerSync;
        let sig = signer.sign_message_sync(canonical.as_bytes()).unwrap();
        att.signature_hex = format!("0x{}", hex::encode(sig.as_bytes()));

        r.persist_attestation(&att).unwrap();
        let updated = r.get_by_id(&spinner_id).unwrap().unwrap();
        assert_eq!(updated.donut_accrued_usd_micro, 0);
    }

    // ── SIWE / provision verification tests ────────────────────────────────────

    /// Helper — build a SIWE message string and sign it with a known key.
    /// `domain` and `chain_id` default to the production values; tests
    /// override them to exercise the rejection paths.
    fn build_siwe(
        signer: &PrivateKeySigner,
        nonce: &str,
        expiration: chrono::DateTime<chrono::Utc>,
        domain: &str,
        chain_id: u64,
    ) -> (String, String) {
        // siwe-rs enforces EIP-55 checksum in `Message::from_str`. Alloy's
        // `Address::to_string()` produces the checksummed form.
        let addr_eip55 = signer.address().to_string();
        build_siwe_with_addr(signer, &addr_eip55, nonce, expiration, domain, chain_id)
    }

    /// Same as [`build_siwe`] but with the body-address as a separate arg.
    /// Used by the EIP-55 casing rejection test to embed a lowercased
    /// address in the message body while the signer is still the same key.
    fn build_siwe_with_addr(
        signer: &PrivateKeySigner,
        body_addr: &str,
        nonce: &str,
        expiration: chrono::DateTime<chrono::Utc>,
        domain: &str,
        chain_id: u64,
    ) -> (String, String) {
        use alloy::signers::SignerSync;

        let issued_at = chrono::Utc::now().to_rfc3339();
        let exp_str = expiration.to_rfc3339();

        let message = format!(
            "{domain} wants you to sign in with your Ethereum account:\n\
             {addr}\n\
             \n\
             Sign to provision a Taifoon solver pod. This signature is used to prove address ownership and is not a transaction. No funds are moved.\n\
             \n\
             URI: https://{domain}\n\
             Version: 1\n\
             Chain ID: {chain_id}\n\
             Nonce: {nonce}\n\
             Issued At: {issued_at}\n\
             Expiration Time: {exp_str}",
            domain = domain,
            addr = body_addr,
            chain_id = chain_id,
            nonce = nonce,
            issued_at = issued_at,
            exp_str = exp_str,
        );

        let sig = signer.sign_message_sync(message.as_bytes()).unwrap();
        let sig_hex = format!("0x{}", hex::encode(sig.as_bytes()));
        (message, sig_hex)
    }

    fn siwe_signer() -> PrivateKeySigner {
        // Fresh key distinct from `test_signer()` so re-use across files
        // can't accidentally make a test pass via a stale ledger entry.
        "0x1111111111111111111111111111111111111111111111111111111111111111"
            .parse()
            .unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn siwe_provision_verifies_and_marks_row() {
        let r = registry();
        let signer = siwe_signer();
        let addr_hex = format!("0x{}", hex::encode(signer.address()));

        // Issue a real nonce — the verifier consumes from the registry's
        // in-memory store, not a hand-rolled value.
        let nonce_resp = r.issue_siwe_nonce(&addr_hex);
        let exp = chrono::Utc::now() + chrono::Duration::minutes(2);
        let (msg, sig) = build_siwe(&signer, &nonce_resp.nonce, exp, "solver.taifoon.dev", 1);

        let req = ProvisionRequest {
            name: "siwe-pod".into(),
            evm_address: addr_hex.clone(),
            solana_address: None,
            signing_mode: Some("self_hosted".into()),
            signer_webhook_url: None,
            safe_address: None,
            email: None,
            chains: None,
            protocols: None,
            siwe_message: Some(msg),
            signature: Some(sig),
        };

        let (solver, _token) = r.provision(&req).await.expect("siwe provision should succeed");
        // Read back the verified flag from sqlite directly — it's not in
        // the public HostedSolver struct.
        let db = r.db.lock().unwrap();
        let v: i64 = db
            .query_row(
                "SELECT siwe_verified FROM hosted_solvers WHERE solver_id = ?1",
                params![solver.solver_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(v, 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn siwe_expired_message_is_rejected() {
        let r = registry();
        let signer = siwe_signer();
        let addr_hex = format!("0x{}", hex::encode(signer.address()));
        let nonce_resp = r.issue_siwe_nonce(&addr_hex);

        // 1 minute in the past — siwe `expiration_time` must be future.
        let exp = chrono::Utc::now() - chrono::Duration::minutes(1);
        let (msg, sig) = build_siwe(&signer, &nonce_resp.nonce, exp, "solver.taifoon.dev", 1);

        let req = ProvisionRequest {
            name: "x".into(),
            evm_address: addr_hex,
            solana_address: None,
            signing_mode: None,
            signer_webhook_url: None,
            safe_address: None,
            email: None,
            chains: None,
            protocols: None,
            siwe_message: Some(msg),
            signature: Some(sig),
        };
        let err = r.provision(&req).await.err().expect("must reject expired siwe");
        let msg = err.to_string().to_lowercase();
        assert!(msg.contains("expired") || msg.contains("expiration"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn siwe_nonce_reuse_is_rejected() {
        let r = registry();
        let signer = siwe_signer();
        let addr_hex = format!("0x{}", hex::encode(signer.address()));
        let nonce_resp = r.issue_siwe_nonce(&addr_hex);

        let exp = chrono::Utc::now() + chrono::Duration::minutes(2);
        let (msg, sig) = build_siwe(&signer, &nonce_resp.nonce, exp, "solver.taifoon.dev", 1);

        let make_req = || ProvisionRequest {
            name: "x".into(),
            evm_address: addr_hex.clone(),
            solana_address: None,
            signing_mode: None,
            signer_webhook_url: None,
            safe_address: None,
            email: None,
            chains: None,
            protocols: None,
            siwe_message: Some(msg.clone()),
            signature: Some(sig.clone()),
        };

        // First call consumes the nonce.
        r.provision(&make_req()).await.expect("first provision ok");
        // Second call with the same SIWE message must fail because the nonce
        // is single-use.
        let err = r.provision(&make_req()).await.err().expect("second call must fail");
        assert!(err.to_string().to_lowercase().contains("nonce"));
    }

    #[tokio::test]
    async fn provision_without_siwe_marks_row_unverified() {
        let r = registry();
        let req = ProvisionRequest {
            name: "no-siwe".into(),
            evm_address: "0x2222222222222222222222222222222222222222".into(),
            solana_address: None,
            signing_mode: None,
            signer_webhook_url: None,
            safe_address: None,
            email: None,
            chains: None,
            protocols: None,
            siwe_message: None,
            signature: None,
        };
        let (solver, _token) = r.provision(&req).await.expect("provision without siwe must succeed");

        let db = r.db.lock().unwrap();
        let v: i64 = db
            .query_row(
                "SELECT siwe_verified FROM hosted_solvers WHERE solver_id = ?1",
                params![solver.solver_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(v, 0);
    }

    #[tokio::test]
    async fn reprovision_rotates_token_but_keeps_solver_id() {
        let r = registry();
        let addr = "0x3333333333333333333333333333333333333333";
        let make_req = || ProvisionRequest {
            name: "rotator".into(),
            evm_address: addr.into(),
            solana_address: None,
            signing_mode: None,
            signer_webhook_url: None,
            safe_address: None,
            email: None,
            chains: None,
            protocols: None,
            siwe_message: None,
            signature: None,
        };

        let (solver1, token1) = r.provision(&make_req()).await.unwrap();
        // Tokens come from OsRng, so collision probability is ~2^-192.
        // The previous implementation relied on a sleep so the nanosecond
        // seed differed; that's no longer necessary.
        let (solver2, token2) = r.provision(&make_req()).await.unwrap();

        // solver_id derived from address — must be stable.
        assert_eq!(solver1.solver_id, solver2.solver_id);
        // New api_token must be different — the previous token is revoked
        // because `api_token_hash` in the row has been overwritten.
        assert_ne!(token1, token2);
    }

    // ── Fix 1: EIP-55 SIWE casing rejection ────────────────────────────────────

    #[tokio::test(flavor = "multi_thread")]
    async fn siwe_rejects_lowercased_address_in_body() {
        let r = registry();
        let signer = siwe_signer();
        let addr_hex = format!("{:#x}", signer.address()); // lowercased "0x…"
        let nonce_resp = r.issue_siwe_nonce(&addr_hex);

        // Build a SIWE message with a *lowercased* address in the body
        // instead of the EIP-55 checksummed form. siwe-rs requires
        // checksummed casing and we ALSO have an explicit pre-parser check
        // — verify_siwe must reject this.
        let exp = chrono::Utc::now() + chrono::Duration::minutes(2);
        let (msg, sig) = build_siwe_with_addr(
            &signer,
            &addr_hex.to_lowercase(), // ← intentionally lowercased
            &nonce_resp.nonce,
            exp,
            "solver.taifoon.dev",
            1,
        );

        let req = ProvisionRequest {
            name: "casing".into(),
            evm_address: addr_hex.clone(),
            siwe_message: Some(msg),
            signature: Some(sig),
            solana_address: None,
            signing_mode: None,
            signer_webhook_url: None,
            safe_address: None,
            email: None,
            chains: None,
            protocols: None,
        };
        let err = r
            .provision(&req)
            .await
            .expect_err("must reject lowercased SIWE address");
        let s = err.to_string();
        assert!(
            s.contains("invalid SIWE message")
                || s.contains("checksum")
                || s.contains("address"),
            "expected casing rejection, got: {}",
            s
        );
    }

    // ── Fix 2: SIWE clock-skew grace ───────────────────────────────────────────

    #[tokio::test(flavor = "multi_thread")]
    async fn siwe_clock_skew_grace_accepts_just_expired() {
        let r = registry();
        let signer = siwe_signer();
        let addr_hex = format!("0x{}", hex::encode(signer.address()));
        let nonce_resp = r.issue_siwe_nonce(&addr_hex);

        // expiration_time = now - 30s — within SIWE_CLOCK_SKEW_SECS (60s)
        // grace window, so provision must succeed.
        let exp = chrono::Utc::now() - chrono::Duration::seconds(30);
        let (msg, sig) = build_siwe(&signer, &nonce_resp.nonce, exp, "solver.taifoon.dev", 1);

        let req = ProvisionRequest {
            name: "skew-grace".into(),
            evm_address: addr_hex,
            solana_address: None,
            signing_mode: Some("self_hosted".into()),
            signer_webhook_url: None,
            safe_address: None,
            email: None,
            chains: None,
            protocols: None,
            siwe_message: Some(msg),
            signature: Some(sig),
        };
        let res = r.provision(&req).await;
        assert!(
            res.is_ok(),
            "just-expired SIWE message should be accepted under clock-skew grace, got: {:?}",
            res.err()
        );
    }
}
