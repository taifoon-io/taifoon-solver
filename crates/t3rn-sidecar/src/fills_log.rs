//! Lightweight fill recorder for the t3rn self-fill engine.
//!
//! Writes into the same `solver_outcomes` SQLite schema used by the executor's
//! `OutcomeLog`, so a single DB file (`sidecar.db` / `OUTCOME_DB_PATH`) holds
//! fills from both the main solver and the t3rn sidecar. Also mirrors each
//! confirmed fill to `rpc.taifoon.dev/api/solver/fills` via async HTTP POST.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tracing::{debug, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillRecord {
    pub ts: DateTime<Utc>,
    pub intent_id: String,
    pub protocol: String,
    pub src_chain: u64,
    pub dst_chain: u64,
    pub decision: String,
    pub tx_hash: Option<String>,
    pub predicted_gas: Option<u64>,
    pub gas_used: Option<u64>,
    pub effective_gas_price_wei: Option<String>,
    pub actual_profit_usd: Option<f64>,
    pub skip_reason: Option<String>,
    pub error: Option<String>,
    pub solver_id: Option<String>,
}

struct Inner {
    sqlite: Mutex<Connection>,
    rpc_url: Option<String>,
    http: reqwest::Client,
}

#[derive(Clone)]
pub struct FillsLog {
    inner: Arc<Inner>,
}

impl FillsLog {
    pub fn open(db_path: &str) -> Result<Self> {
        let conn = Connection::open(db_path)
            .with_context(|| format!("FillsLog::open {}", db_path))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS solver_outcomes (
                ts                       TEXT    NOT NULL,
                intent_id                TEXT    NOT NULL,
                protocol                 TEXT    NOT NULL,
                src_chain                INTEGER NOT NULL,
                dst_chain                INTEGER NOT NULL,
                decision                 TEXT    NOT NULL,
                tx_hash                  TEXT,
                predicted_gas            INTEGER,
                gas_used                 INTEGER,
                effective_gas_price_wei  TEXT,
                predicted_profit_usd     REAL,
                actual_profit_usd        REAL,
                skip_reason              TEXT,
                error                    TEXT,
                solver_id                TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_outcomes_ts     ON solver_outcomes(ts);
            CREATE INDEX IF NOT EXISTS idx_outcomes_solver ON solver_outcomes(solver_id);",
        )?;
        // Backfill columns added in later schema versions.
        // SQLite error code 1 (SQLITE_ERROR) = "duplicate column name" — ignore it.
        for sql in &[
            "ALTER TABLE solver_outcomes ADD COLUMN predicted_gas INTEGER",
            "ALTER TABLE solver_outcomes ADD COLUMN skip_reason TEXT",
            "ALTER TABLE solver_outcomes ADD COLUMN solver_id TEXT",
        ] {
            match conn.execute(sql, []) {
                Ok(_) => {},
                Err(rusqlite::Error::SqliteFailure(e, _))
                    if e.extended_code == 1 || e.code == rusqlite::ErrorCode::Unknown => {},
                Err(e) => return Err(e.into()),
            }
        }

        let rpc_url = std::env::var("TAIFOON_RPC_URL")
            .or_else(|_| std::env::var("WARMBED_API_URL"))
            .ok()
            .filter(|s| !s.is_empty());

        Ok(Self {
            inner: Arc::new(Inner {
                sqlite: Mutex::new(conn),
                rpc_url,
                http: reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(3))
                    .build()
                    .unwrap_or_default(),
            }),
        })
    }

    pub fn append(&self, rec: FillRecord) -> Result<()> {
        {
            let conn = self.inner.sqlite.lock().unwrap();
            conn.execute(
                "INSERT INTO solver_outcomes
                    (ts, intent_id, protocol, src_chain, dst_chain, decision, tx_hash,
                     predicted_gas, gas_used, effective_gas_price_wei,
                     predicted_profit_usd, actual_profit_usd, skip_reason, error, solver_id)
                 VALUES (?,?,?,?,?,?,?,?,?,?,NULL,?,?,?,?)",
                params![
                    rec.ts.to_rfc3339(),
                    rec.intent_id,
                    rec.protocol,
                    rec.src_chain as i64,
                    rec.dst_chain as i64,
                    rec.decision,
                    rec.tx_hash,
                    rec.predicted_gas.map(|g| g as i64),
                    rec.gas_used.map(|g| g as i64),
                    rec.effective_gas_price_wei,
                    rec.actual_profit_usd,
                    rec.skip_reason,
                    rec.error,
                    rec.solver_id,
                ],
            )?;
        }
        debug!("💾 t3rn fill logged: {} {}", rec.intent_id, rec.decision);

        if let Some(url) = self.inner.rpc_url.clone() {
            let http = self.inner.http.clone();
            let payload = rec;
            tokio::spawn(async move {
                let endpoint = format!("{}/api/solver/fills", url.trim_end_matches('/'));
                if let Err(e) = http.post(&endpoint).json(&payload).send().await {
                    warn!("📡 rpc.taifoon fills mirror failed: {}", e);
                }
            });
        }

        Ok(())
    }
}
