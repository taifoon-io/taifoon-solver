//! Append-only outcome log for solver fills.
//!
//! Storage selection (per spec, section 4.3):
//! - SQLite always (durable local record).
//! - If MAMBA_LAKE_URL is set + reachable, mirror outcomes via async HTTP POST.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutcomeRecord {
    pub ts: DateTime<Utc>,
    pub intent_id: String,
    pub protocol: String,
    pub src_chain: u64,
    pub dst_chain: u64,
    pub decision: String,
    pub tx_hash: Option<String>,
    /// Pre-flight estimate from spinner /api/solver/test-run. Always set when
    /// available (skip + execute paths), so the weekly analyzer has both
    /// the prediction and the receipt-derived `gas_used`.
    #[serde(default)]
    pub predicted_gas: Option<u64>,
    /// Receipt-derived `gasUsed`. Serialized as `actual_gas` upstream and
    /// kept as `gas_used` locally for SQLite back-compat.
    #[serde(rename = "actual_gas", alias = "gas_used")]
    pub gas_used: Option<u64>,
    pub effective_gas_price_wei: Option<String>,
    pub predicted_profit_usd: Option<f64>,
    pub actual_profit_usd: Option<f64>,
    /// Why the intent was skipped, when `decision` starts with `skip_`.
    /// Free-form ("unprofitable", "below_threshold", "no_route", ...).
    #[serde(default)]
    pub skip_reason: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct OutcomeLog {
    inner: Arc<OutcomeLogInner>,
}

struct OutcomeLogInner {
    sqlite: Mutex<Connection>,
    mamba_url: Option<String>,
    http: reqwest::Client,
}

impl OutcomeLog {
    /// Open / create a SQLite-backed log, optionally mirroring to mamba DuckDB.
    pub fn open(sqlite_path: &str, mamba_url: Option<String>) -> Result<Self> {
        let conn = Connection::open(sqlite_path)
            .with_context(|| format!("open {}", sqlite_path))?;
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
                error                    TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_outcomes_intent ON solver_outcomes(intent_id);
            CREATE INDEX IF NOT EXISTS idx_outcomes_ts     ON solver_outcomes(ts);",
        )?;
        // Older DBs created before X1: backfill the new columns. SQLite will
        // error if the column already exists, so we ignore those errors.
        let _ = conn.execute_batch(
            "ALTER TABLE solver_outcomes ADD COLUMN predicted_gas INTEGER;
             ALTER TABLE solver_outcomes ADD COLUMN skip_reason   TEXT;",
        );

        let mamba_url = match mamba_url {
            Some(url) if !url.is_empty() => {
                info!("📊 OutcomeLog: mamba mirror configured at {}", url);
                Some(url)
            }
            _ => None,
        };

        Ok(Self {
            inner: Arc::new(OutcomeLogInner {
                sqlite: Mutex::new(conn),
                mamba_url,
                http: reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(3))
                    .build()
                    .unwrap_or_else(|_| reqwest::Client::new()),
            }),
        })
    }

    pub fn append(&self, rec: OutcomeRecord) -> Result<()> {
        {
            let conn = self.inner.sqlite.lock().unwrap();
            conn.execute(
                "INSERT INTO solver_outcomes
                    (ts, intent_id, protocol, src_chain, dst_chain, decision, tx_hash,
                     predicted_gas, gas_used, effective_gas_price_wei,
                     predicted_profit_usd, actual_profit_usd, skip_reason, error)
                 VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
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
                    rec.predicted_profit_usd,
                    rec.actual_profit_usd,
                    rec.skip_reason,
                    rec.error,
                ],
            )?;
        }
        debug!("💾 outcome logged: {} {}", rec.intent_id, rec.decision);

        if let Some(url) = self.inner.mamba_url.clone() {
            let http = self.inner.http.clone();
            let payload = rec;
            tokio::spawn(async move {
                let endpoint = format!("{}/api/solver/outcomes", url.trim_end_matches('/'));
                if let Err(e) = http.post(&endpoint).json(&payload).send().await {
                    warn!("📊 mamba mirror failed: {}", e);
                }
            });
        }
        Ok(())
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self.inner.sqlite.lock().unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM solver_outcomes", [], |r| r.get(0))?;
        Ok(n)
    }
}
