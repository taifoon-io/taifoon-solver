//! wallet-manager — intent lifecycle bookkeeping + funds reservation.
//!
//! Persists every detected intent through its full state machine in SQLite,
//! tracks reserved (in-flight) USD against the solver's wallet budget, and
//! exposes a small read-only HTTP surface (`/api/wallet/status`,
//! `/api/wallet/intents`) for the dashboard.

use anyhow::{anyhow, Result};
use axum::{
    extract::{Query, State},
    response::Json,
    routing::get,
    Router,
};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use thiserror::Error;

/// Lifecycle states for a tracked intent. Persisted as the lowercase string form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IntentState {
    IntentDetected,
    ProfitabilityCheck,
    SkipUnprofitable,
    ProofFetch,
    ProofMissing,
    CalldataBuild,
    CalldataError,
    Broadcast,
    PendingConfirmation,
    Confirmed,
    /// claimUnlock() sent to source chain; waiting for receipt.
    ClaimPending,
    /// claimUnlock() confirmed; giveTokens credited to solver.
    Claimed,
    Reverted,
    RetryQueued,
}

impl IntentState {
    pub fn as_str(&self) -> &'static str {
        match self {
            IntentState::IntentDetected => "INTENT_DETECTED",
            IntentState::ProfitabilityCheck => "PROFITABILITY_CHECK",
            IntentState::SkipUnprofitable => "SKIP_UNPROFITABLE",
            IntentState::ProofFetch => "PROOF_FETCH",
            IntentState::ProofMissing => "PROOF_MISSING",
            IntentState::CalldataBuild => "CALLDATA_BUILD",
            IntentState::CalldataError => "CALLDATA_ERROR",
            IntentState::Broadcast => "BROADCAST",
            IntentState::PendingConfirmation => "PENDING_CONFIRMATION",
            IntentState::Confirmed => "CONFIRMED",
            IntentState::ClaimPending => "CLAIM_PENDING",
            IntentState::Claimed => "CLAIMED",
            IntentState::Reverted => "REVERTED",
            IntentState::RetryQueued => "RETRY_QUEUED",
        }
    }

    /// Terminal states do not hold reserved funds and cannot transition further.
    /// Note: `Confirmed` is NOT terminal — deBridge fills must continue to
    /// `ClaimPending → Claimed`. Only `Claimed` is the final Confirmed-path terminal.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            IntentState::SkipUnprofitable
                | IntentState::ProofMissing
                | IntentState::CalldataError
                | IntentState::Claimed
                | IntentState::Reverted
        )
    }
}

impl FromStr for IntentState {
    type Err = WalletError;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s {
            "INTENT_DETECTED" => IntentState::IntentDetected,
            "PROFITABILITY_CHECK" => IntentState::ProfitabilityCheck,
            "SKIP_UNPROFITABLE" => IntentState::SkipUnprofitable,
            "PROOF_FETCH" => IntentState::ProofFetch,
            "PROOF_MISSING" => IntentState::ProofMissing,
            "CALLDATA_BUILD" => IntentState::CalldataBuild,
            "CALLDATA_ERROR" => IntentState::CalldataError,
            "BROADCAST" => IntentState::Broadcast,
            "PENDING_CONFIRMATION" => IntentState::PendingConfirmation,
            "CONFIRMED" => IntentState::Confirmed,
            "CLAIM_PENDING" => IntentState::ClaimPending,
            "CLAIMED" => IntentState::Claimed,
            "REVERTED" => IntentState::Reverted,
            "RETRY_QUEUED" => IntentState::RetryQueued,
            other => return Err(WalletError::UnknownState(other.to_string())),
        })
    }
}

#[derive(Debug, Error)]
pub enum WalletError {
    #[error("unknown intent state: {0}")]
    UnknownState(String),
    #[error("intent not found: {0}")]
    NotFound(String),
    #[error("intent {0} is in terminal state {1}; cannot transition")]
    TerminalTransition(String, String),
    #[error("insufficient wallet budget: requested ${requested:.2} reserved=${reserved:.2} budget=${budget:.2}")]
    InsufficientBudget {
        requested: f64,
        reserved: f64,
        budget: f64,
    },
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("other: {0}")]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentRecord {
    pub intent_id: String,
    pub protocol: String,
    pub src_chain: i64,
    pub dst_chain: i64,
    pub amount_usd: f64,
    pub state: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub tx_hash: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewIntent {
    pub intent_id: String,
    pub protocol: String,
    pub src_chain: i64,
    pub dst_chain: i64,
    pub amount_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletStatus {
    pub budget_usd: f64,
    pub reserved_usd: f64,
    pub available_usd: f64,
    pub realized_revenue_usd: f64,
    pub active_intents: i64,
    pub confirmed_intents: i64,
    pub reverted_intents: i64,
}

/// In-memory + SQLite-backed wallet state. Cloneable via `Arc` for axum sharing.
pub struct WalletManager {
    conn: Mutex<Connection>,
    budget_usd: f64,
}

impl WalletManager {
    /// Open or create the wallet ledger at `db_path` (use `:memory:` for tests).
    pub fn open(db_path: &str, budget_usd: f64) -> Result<Self, WalletError> {
        let conn = Connection::open(db_path)?;
        let mgr = Self {
            conn: Mutex::new(conn),
            budget_usd,
        };
        mgr.init_schema()?;
        Ok(mgr)
    }

    fn init_schema(&self) -> Result<(), WalletError> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS intents (
                intent_id    TEXT PRIMARY KEY,
                protocol     TEXT NOT NULL,
                src_chain    INTEGER NOT NULL,
                dst_chain    INTEGER NOT NULL,
                amount_usd   REAL NOT NULL,
                state        TEXT NOT NULL,
                created_at   TEXT NOT NULL,
                updated_at   TEXT NOT NULL,
                tx_hash      TEXT,
                error        TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_intents_state    ON intents(state);
            CREATE INDEX IF NOT EXISTS idx_intents_protocol ON intents(protocol);

            CREATE TABLE IF NOT EXISTS reservations (
                intent_id   TEXT PRIMARY KEY,
                amount_usd  REAL NOT NULL,
                created_at  TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS revenue (
                intent_id    TEXT PRIMARY KEY,
                profit_usd   REAL NOT NULL,
                recorded_at  TEXT NOT NULL
            );
            "#,
        )?;
        Ok(())
    }

    /// Insert a freshly-detected intent in `INTENT_DETECTED`. Idempotent on
    /// `intent_id` collisions (returns the existing record).
    pub fn record_detected(&self, new: NewIntent) -> Result<IntentRecord, WalletError> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now();
        let state = IntentState::IntentDetected;

        let existing: Option<IntentRecord> = conn
            .query_row(
                "SELECT intent_id, protocol, src_chain, dst_chain, amount_usd, state, \
                 created_at, updated_at, tx_hash, error FROM intents WHERE intent_id = ?1",
                params![new.intent_id],
                row_to_record,
            )
            .optional()?;

        if let Some(rec) = existing {
            return Ok(rec);
        }

        conn.execute(
            "INSERT INTO intents \
             (intent_id, protocol, src_chain, dst_chain, amount_usd, state, created_at, updated_at, tx_hash, error) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, NULL)",
            params![
                new.intent_id,
                new.protocol,
                new.src_chain,
                new.dst_chain,
                new.amount_usd,
                state.as_str(),
                now.to_rfc3339(),
                now.to_rfc3339(),
            ],
        )?;

        Ok(IntentRecord {
            intent_id: new.intent_id,
            protocol: new.protocol,
            src_chain: new.src_chain,
            dst_chain: new.dst_chain,
            amount_usd: new.amount_usd,
            state: state.as_str().to_string(),
            created_at: now,
            updated_at: now,
            tx_hash: None,
            error: None,
        })
    }

    /// Move an intent into `next_state`. Optionally attaches a tx_hash (for
    /// `BROADCAST`/`CONFIRMED`) or an error message (for the *_ERROR / REVERTED
    /// branches). Refuses transitions out of terminal states.
    pub fn transition(
        &self,
        intent_id: &str,
        next_state: IntentState,
        tx_hash: Option<&str>,
        error: Option<&str>,
    ) -> Result<IntentRecord, WalletError> {
        let conn = self.conn.lock().unwrap();
        let current: IntentRecord = conn
            .query_row(
                "SELECT intent_id, protocol, src_chain, dst_chain, amount_usd, state, \
                 created_at, updated_at, tx_hash, error FROM intents WHERE intent_id = ?1",
                params![intent_id],
                row_to_record,
            )
            .optional()?
            .ok_or_else(|| WalletError::NotFound(intent_id.to_string()))?;

        let current_state = IntentState::from_str(&current.state)?;
        if current_state.is_terminal() {
            return Err(WalletError::TerminalTransition(
                intent_id.to_string(),
                current.state,
            ));
        }

        let now = Utc::now();
        conn.execute(
            "UPDATE intents SET state = ?1, updated_at = ?2, \
             tx_hash = COALESCE(?3, tx_hash), error = COALESCE(?4, error) \
             WHERE intent_id = ?5",
            params![
                next_state.as_str(),
                now.to_rfc3339(),
                tx_hash,
                error,
                intent_id,
            ],
        )?;

        // Auto-release reservations when entering a terminal state.
        if next_state.is_terminal() {
            conn.execute(
                "DELETE FROM reservations WHERE intent_id = ?1",
                params![intent_id],
            )?;
        }

        Ok(IntentRecord {
            state: next_state.as_str().to_string(),
            updated_at: now,
            tx_hash: tx_hash.map(str::to_string).or(current.tx_hash),
            error: error.map(str::to_string).or(current.error),
            ..current
        })
    }

    /// Reserve `amount_usd` against the wallet budget for `intent_id`. Fails if
    /// the new total reserved would exceed the configured budget. Idempotent
    /// per intent_id (re-reserving the same intent is a no-op).
    pub fn reserve(&self, intent_id: &str, amount_usd: f64) -> Result<(), WalletError> {
        let conn = self.conn.lock().unwrap();
        let already: Option<f64> = conn
            .query_row(
                "SELECT amount_usd FROM reservations WHERE intent_id = ?1",
                params![intent_id],
                |r| r.get(0),
            )
            .optional()?;
        if already.is_some() {
            return Ok(());
        }

        let reserved: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(amount_usd), 0.0) FROM reservations",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0.0);

        if reserved + amount_usd > self.budget_usd {
            return Err(WalletError::InsufficientBudget {
                requested: amount_usd,
                reserved,
                budget: self.budget_usd,
            });
        }

        conn.execute(
            "INSERT INTO reservations (intent_id, amount_usd, created_at) VALUES (?1, ?2, ?3)",
            params![intent_id, amount_usd, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Release reservations whose intents have been stuck in a non-terminal state
    /// for longer than `max_age_secs`. Prevents budget exhaustion from crash-orphaned
    /// reservations that will never be resolved by normal fill/skip/revert paths.
    ///
    /// Targets: all states except CLAIM_PENDING/CLAIMED (which represent a
    /// deBridge claimUnlock in-flight where capital is still at risk).
    /// BROADCAST/PENDING_CONFIRMATION/CONFIRMED are included: the explicit
    /// wallet.release() call in lambda_controller always runs on success, and
    /// after 4 h any reservation in those states is crash-orphaned.
    pub fn release_stale_reservations(&self, max_age_secs: i64) -> Result<usize, WalletError> {
        let conn = self.conn.lock().unwrap();
        let cutoff = (Utc::now() - Duration::seconds(max_age_secs)).to_rfc3339();
        let n = conn.execute(
            "DELETE FROM reservations WHERE intent_id IN (
               SELECT r.intent_id FROM reservations r
               JOIN intents i ON i.intent_id = r.intent_id
               WHERE i.state NOT IN ('CLAIM_PENDING','CLAIMED')
                 AND r.created_at < ?1
             )",
            params![cutoff],
        )?;
        Ok(n)
    }

    /// Drop a previously reserved hold without recording revenue. Safe to call
    /// for an intent that holds no reservation.
    pub fn release(&self, intent_id: &str) -> Result<(), WalletError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM reservations WHERE intent_id = ?1",
            params![intent_id],
        )?;
        Ok(())
    }

    /// Record realized profit (positive = earned, negative = loss) for a
    /// confirmed intent and release any outstanding reservation.
    pub fn record_revenue(&self, intent_id: &str, profit_usd: f64) -> Result<(), WalletError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO revenue (intent_id, profit_usd, recorded_at) VALUES (?1, ?2, ?3)",
            params![intent_id, profit_usd, Utc::now().to_rfc3339()],
        )?;
        conn.execute(
            "DELETE FROM reservations WHERE intent_id = ?1",
            params![intent_id],
        )?;
        Ok(())
    }

    pub fn status(&self) -> Result<WalletStatus, WalletError> {
        let conn = self.conn.lock().unwrap();
        let reserved: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(amount_usd), 0.0) FROM reservations",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0.0);
        let realized: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(profit_usd), 0.0) FROM revenue",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0.0);

        let mut state_counts: HashMap<String, i64> = HashMap::new();
        let mut stmt = conn.prepare("SELECT state, COUNT(*) FROM intents GROUP BY state")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        for r in rows {
            let (s, n) = r?;
            state_counts.insert(s, n);
        }

        let confirmed = *state_counts.get("CONFIRMED").unwrap_or(&0);
        let reverted = *state_counts.get("REVERTED").unwrap_or(&0);
        let active: i64 = state_counts
            .iter()
            .filter(|(s, _)| {
                IntentState::from_str(s)
                    .map(|st| !st.is_terminal())
                    .unwrap_or(false)
            })
            .map(|(_, n)| *n)
            .sum();

        Ok(WalletStatus {
            budget_usd: self.budget_usd,
            reserved_usd: reserved,
            available_usd: (self.budget_usd - reserved).max(0.0),
            realized_revenue_usd: realized,
            active_intents: active,
            confirmed_intents: confirmed,
            reverted_intents: reverted,
        })
    }

    pub fn list_intents(
        &self,
        state_filter: Option<&str>,
        limit: i64,
    ) -> Result<Vec<IntentRecord>, WalletError> {
        let conn = self.conn.lock().unwrap();
        let limit = limit.clamp(1, 1000);
        let mut out = Vec::new();
        if let Some(state) = state_filter {
            let mut stmt = conn.prepare(
                "SELECT intent_id, protocol, src_chain, dst_chain, amount_usd, state, \
                 created_at, updated_at, tx_hash, error FROM intents \
                 WHERE state = ?1 ORDER BY updated_at DESC LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![state, limit], row_to_record)?;
            for r in rows {
                out.push(r?);
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT intent_id, protocol, src_chain, dst_chain, amount_usd, state, \
                 created_at, updated_at, tx_hash, error FROM intents \
                 ORDER BY updated_at DESC LIMIT ?1",
            )?;
            let rows = stmt.query_map(params![limit], row_to_record)?;
            for r in rows {
                out.push(r?);
            }
        }
        Ok(out)
    }

    /// Read deBridge claim rows for the dashboard Claims tab. Returns one row
    /// per intent that ever entered the claim lifecycle (CONFIRMED/CLAIM_PENDING/
    /// CLAIMED/REVERTED) for any protocol whose name contains "debridge" or
    /// "dln". The current `tx_hash` column is the latest broadcast on that
    /// intent — for `CLAIMED` rows it's the claim tx; for `CONFIRMED` /
    /// `CLAIM_PENDING` it's the fill tx. The caller (solver-api) joins
    /// `solver_outcomes` to recover the original fill_tx_hash when the wallet
    /// row has been overwritten by the claim transition.
    ///
    /// `claim_fee_usd` is read from the `revenue` table — only populated when
    /// `lambda_claim_debridge` calls `record_revenue` on a successful claim.
    pub fn list_debridge_claims(&self, limit: i64) -> Result<Vec<DebridgeClaimRow>, WalletError> {
        let conn = self.conn.lock().unwrap();
        let limit = limit.clamp(1, 1000);
        let sql = "SELECT i.intent_id, i.protocol, i.src_chain, i.dst_chain, \
                          i.amount_usd, i.state, i.created_at, i.updated_at, \
                          i.tx_hash, i.error, r.profit_usd \
                   FROM intents i \
                   LEFT JOIN revenue r ON r.intent_id = i.intent_id \
                   WHERE (lower(i.protocol) LIKE '%debridge%' OR lower(i.protocol) LIKE '%dln%') \
                     AND i.state IN ('CONFIRMED','CLAIM_PENDING','CLAIMED','REVERTED') \
                   ORDER BY i.updated_at DESC \
                   LIMIT ?1";
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![limit], |r| {
            let created: String = r.get(6)?;
            let updated: String = r.get(7)?;
            Ok(DebridgeClaimRow {
                intent_id: r.get(0)?,
                protocol: r.get(1)?,
                src_chain: r.get(2)?,
                dst_chain: r.get(3)?,
                amount_usd: r.get(4)?,
                state: r.get(5)?,
                created_at: parse_ts(&created),
                updated_at: parse_ts(&updated),
                wallet_tx_hash: r.get(8)?,
                error: r.get(9)?,
                claim_fee_usd: r.get(10)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}

/// Joined row used by the dashboard `/api/solver/claims` endpoint. The
/// `wallet_tx_hash` field is the latest tx on the intent record — solver-api
/// must classify it as fill_tx_hash or claim_tx_hash based on `state` and
/// fall back to `solver_outcomes` for the fill tx after a claim transition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebridgeClaimRow {
    pub intent_id: String,
    pub protocol: String,
    pub src_chain: i64,
    pub dst_chain: i64,
    pub amount_usd: f64,
    pub state: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub wallet_tx_hash: Option<String>,
    pub error: Option<String>,
    pub claim_fee_usd: Option<f64>,
}

fn row_to_record(r: &rusqlite::Row<'_>) -> rusqlite::Result<IntentRecord> {
    let created: String = r.get(6)?;
    let updated: String = r.get(7)?;
    Ok(IntentRecord {
        intent_id: r.get(0)?,
        protocol: r.get(1)?,
        src_chain: r.get(2)?,
        dst_chain: r.get(3)?,
        amount_usd: r.get(4)?,
        state: r.get(5)?,
        created_at: parse_ts(&created),
        updated_at: parse_ts(&updated),
        tx_hash: r.get(8)?,
        error: r.get(9)?,
    })
}

fn parse_ts(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

// ---------------------------------------------------------------------------
// Axum surface
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub state: Option<String>,
    pub limit: Option<i64>,
}

pub fn router(manager: Arc<WalletManager>) -> Router {
    Router::new()
        .route("/api/wallet/status", get(get_status))
        .route("/api/wallet/intents", get(get_intents))
        .with_state(manager)
}

async fn get_status(
    State(mgr): State<Arc<WalletManager>>,
) -> Result<Json<WalletStatus>, (axum::http::StatusCode, String)> {
    mgr.status()
        .map(Json)
        .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn get_intents(
    State(mgr): State<Arc<WalletManager>>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<IntentRecord>>, (axum::http::StatusCode, String)> {
    let limit = q.limit.unwrap_or(100);
    mgr.list_intents(q.state.as_deref(), limit)
        .map(Json)
        .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

// Re-exports surface a tidy public API for downstream crates.
pub use anyhow::Error as AnyhowError;

#[allow(dead_code)]
fn _ensure_anyhow_used() -> anyhow::Result<()> {
    Err(anyhow!("placeholder"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mgr() -> WalletManager {
        WalletManager::open(":memory:", 1000.0).unwrap()
    }

    fn intent(id: &str, amt: f64) -> NewIntent {
        NewIntent {
            intent_id: id.to_string(),
            protocol: "across".to_string(),
            src_chain: 1,
            dst_chain: 42161,
            amount_usd: amt,
        }
    }

    #[test]
    fn detect_then_transition_chain() {
        let m = mgr();
        m.record_detected(intent("i1", 100.0)).unwrap();
        let r = m
            .transition("i1", IntentState::ProfitabilityCheck, None, None)
            .unwrap();
        assert_eq!(r.state, "PROFITABILITY_CHECK");
        let r = m
            .transition("i1", IntentState::CalldataBuild, None, None)
            .unwrap();
        assert_eq!(r.state, "CALLDATA_BUILD");
    }

    #[test]
    fn reserve_release_round_trip() {
        let m = mgr();
        m.record_detected(intent("i1", 200.0)).unwrap();
        m.reserve("i1", 200.0).unwrap();
        let s = m.status().unwrap();
        assert_eq!(s.reserved_usd, 200.0);
        assert_eq!(s.available_usd, 800.0);
        m.release("i1").unwrap();
        assert_eq!(m.status().unwrap().reserved_usd, 0.0);
    }

    #[test]
    fn reserve_rejects_overdraft() {
        let m = mgr();
        m.record_detected(intent("i1", 600.0)).unwrap();
        m.reserve("i1", 600.0).unwrap();
        m.record_detected(intent("i2", 500.0)).unwrap();
        let err = m.reserve("i2", 500.0).unwrap_err();
        assert!(matches!(err, WalletError::InsufficientBudget { .. }));
    }

    #[test]
    fn record_revenue_releases_reservation() {
        let m = mgr();
        m.record_detected(intent("i1", 100.0)).unwrap();
        m.reserve("i1", 100.0).unwrap();
        m.record_revenue("i1", 4.25).unwrap();
        let s = m.status().unwrap();
        assert_eq!(s.reserved_usd, 0.0);
        assert!((s.realized_revenue_usd - 4.25).abs() < 1e-9);
    }

    #[test]
    fn terminal_state_blocks_transition() {
        let m = mgr();
        m.record_detected(intent("i1", 50.0)).unwrap();
        // Reverted IS terminal — a second transition must be rejected.
        m.transition("i1", IntentState::Reverted, Some("0xabc"), None)
            .unwrap();
        let err = m
            .transition("i1", IntentState::Claimed, None, None)
            .unwrap_err();
        assert!(matches!(err, WalletError::TerminalTransition(_, _)));
    }

    #[test]
    fn confirmed_not_terminal_reservation_held() {
        let m = mgr();
        m.record_detected(intent("i1", 75.0)).unwrap();
        m.reserve("i1", 75.0).unwrap();
        m.transition("i1", IntentState::Confirmed, Some("0xdead"), None)
            .unwrap();
        // Confirmed is NOT terminal — reservation stays held until Claimed.
        assert_eq!(m.status().unwrap().reserved_usd, 75.0);
        // Claiming releases the hold.
        m.transition("i1", IntentState::ClaimPending, None, None).unwrap();
        m.transition("i1", IntentState::Claimed, Some("0xclaim"), None).unwrap();
        assert_eq!(m.status().unwrap().reserved_usd, 0.0);
    }

    #[test]
    fn list_intents_filters_by_state() {
        let m = mgr();
        m.record_detected(intent("a", 10.0)).unwrap();
        m.record_detected(intent("b", 20.0)).unwrap();
        m.transition("a", IntentState::ProfitabilityCheck, None, None)
            .unwrap();
        let out = m.list_intents(Some("INTENT_DETECTED"), 100).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].intent_id, "b");
    }

    #[test]
    fn idempotent_record_detected() {
        let m = mgr();
        let a = m.record_detected(intent("i1", 100.0)).unwrap();
        let b = m.record_detected(intent("i1", 999.0)).unwrap();
        assert_eq!(a.intent_id, b.intent_id);
        // Second insert is a no-op; original amount preserved.
        assert_eq!(b.amount_usd, 100.0);
    }

    fn debridge_intent(id: &str, amt: f64) -> NewIntent {
        NewIntent {
            intent_id: id.to_string(),
            protocol: "debridge_dln".to_string(),
            src_chain: 42161,
            dst_chain: 8453,
            amount_usd: amt,
        }
    }

    #[test]
    fn list_debridge_claims_filters_by_protocol_and_state() {
        let m = mgr();
        // Two debridge intents, one Across.
        m.record_detected(debridge_intent("debridge_dln:0xabc", 50.0))
            .unwrap();
        m.record_detected(debridge_intent("debridge_dln:0xdef", 80.0))
            .unwrap();
        m.record_detected(intent("across_only", 30.0)).unwrap();

        // Move first deBridge to CONFIRMED (claim pending), second to CLAIMED.
        m.transition(
            "debridge_dln:0xabc",
            IntentState::Confirmed,
            Some("0xfilltx_abc"),
            None,
        )
        .unwrap();
        // For the second: CONFIRMED → CLAIM_PENDING → CLAIMED. CLAIMED transition
        // overwrites tx_hash with the claim tx (mirroring lambda_claim_debridge).
        // We need a fresh non-terminal intent to walk the chain — use a
        // throwaway record that simulates the in-flight broadcast.
        m.transition(
            "debridge_dln:0xdef",
            IntentState::ClaimPending,
            Some("0xfilltx_def"),
            None,
        )
        .unwrap();
        m.transition(
            "debridge_dln:0xdef",
            IntentState::Claimed,
            Some("0xclaimtx_def"),
            None,
        )
        .unwrap();
        m.record_revenue("debridge_dln:0xdef", 0.42).unwrap();

        // Across one stays in INTENT_DETECTED — must NOT be in claims output.
        let claims = m.list_debridge_claims(100).unwrap();
        let ids: Vec<&str> = claims.iter().map(|c| c.intent_id.as_str()).collect();
        assert!(ids.contains(&"debridge_dln:0xabc"));
        assert!(ids.contains(&"debridge_dln:0xdef"));
        assert!(
            !ids.contains(&"across_only"),
            "Across must be filtered out by protocol"
        );

        let claimed = claims
            .iter()
            .find(|c| c.intent_id == "debridge_dln:0xdef")
            .unwrap();
        assert_eq!(claimed.state, "CLAIMED");
        // wallet_tx_hash is the claim tx after the CLAIMED transition.
        assert_eq!(claimed.wallet_tx_hash.as_deref(), Some("0xclaimtx_def"));
        // Revenue join populates claim_fee_usd.
        assert!((claimed.claim_fee_usd.unwrap_or(0.0) - 0.42).abs() < 1e-9);

        let pending = claims
            .iter()
            .find(|c| c.intent_id == "debridge_dln:0xabc")
            .unwrap();
        assert_eq!(pending.state, "CONFIRMED");
        // No revenue recorded yet — claim_fee_usd is None.
        assert!(pending.claim_fee_usd.is_none());
        // Wallet tx_hash is the fill tx.
        assert_eq!(pending.wallet_tx_hash.as_deref(), Some("0xfilltx_abc"));
    }

    #[test]
    fn list_debridge_claims_excludes_pre_claim_lifecycle_states() {
        let m = mgr();
        m.record_detected(debridge_intent("debridge_dln:0xnew", 10.0))
            .unwrap();
        // Stays in INTENT_DETECTED — pre-claim, must not appear in claims tab.
        let claims = m.list_debridge_claims(100).unwrap();
        assert!(claims.is_empty());
    }
}
