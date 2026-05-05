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

    /// Return the most recent N outcome records, newest first.
    /// Used by the solver-api `/api/solver/outcomes` endpoint and by the
    /// dashboard P&L panel.
    pub fn recent(&self, limit: i64) -> Result<Vec<OutcomeRecord>> {
        let conn = self.inner.sqlite.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT ts, intent_id, protocol, src_chain, dst_chain, decision, tx_hash,
                    predicted_gas, gas_used, effective_gas_price_wei,
                    predicted_profit_usd, actual_profit_usd, skip_reason, error
             FROM solver_outcomes
             ORDER BY ts DESC
             LIMIT ?",
        )?;
        let rows = stmt.query_map(params![limit], |r| {
            let ts_str: String = r.get(0)?;
            let ts = chrono::DateTime::parse_from_rfc3339(&ts_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            Ok(OutcomeRecord {
                ts,
                intent_id: r.get(1)?,
                protocol: r.get(2)?,
                src_chain: r.get::<_, i64>(3)? as u64,
                dst_chain: r.get::<_, i64>(4)? as u64,
                decision: r.get(5)?,
                tx_hash: r.get(6)?,
                predicted_gas: r.get::<_, Option<i64>>(7)?.map(|v| v as u64),
                gas_used: r.get::<_, Option<i64>>(8)?.map(|v| v as u64),
                effective_gas_price_wei: r.get(9)?,
                predicted_profit_usd: r.get(10)?,
                actual_profit_usd: r.get(11)?,
                skip_reason: r.get(12)?,
                error: r.get(13)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Aggregate P&L summary for the dashboard: realized USD totals, fill
    /// counts, and per-protocol breakdown.
    pub fn pnl_summary(&self) -> Result<PnlSummary> {
        let conn = self.inner.sqlite.lock().unwrap();

        // fills_total: confirmed/execute rows regardless of whether actual_profit_usd
        // is populated yet (fills pending confirmation still count).
        // realized_usd_total: only rows where actual_profit_usd is known.
        // last_24h_count: all rows in the last 24 h.
        let (total_realized, total_fills, last_24h_count): (f64, i64, i64) = conn.query_row(
            "SELECT
                COALESCE(SUM(CASE WHEN actual_profit_usd IS NOT NULL THEN actual_profit_usd ELSE 0.0 END), 0.0),
                COALESCE(SUM(CASE WHEN decision IN ('confirmed', 'execute') THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN ts >= datetime('now', '-1 day') THEN 1 ELSE 0 END), 0)
             FROM solver_outcomes",
            [],
            |r| Ok((r.get::<_, f64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)),
        ).unwrap_or((0.0, 0, 0));

        let mut by_protocol = std::collections::HashMap::<String, ProtocolPnl>::new();
        let mut stmt = conn.prepare(
            "SELECT protocol,
                    COUNT(*) AS fills,
                    COALESCE(SUM(actual_profit_usd), 0.0) AS realized,
                    COALESCE(AVG(actual_profit_usd), 0.0) AS avg_profit
             FROM solver_outcomes
             WHERE actual_profit_usd IS NOT NULL
             GROUP BY protocol",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, f64>(2)?,
                r.get::<_, f64>(3)?,
            ))
        })?;
        for row in rows {
            let (protocol, fills, realized, avg) = row?;
            by_protocol.insert(
                protocol,
                ProtocolPnl {
                    fills,
                    realized_usd: realized,
                    avg_profit_usd: avg,
                },
            );
        }

        Ok(PnlSummary {
            realized_usd_total: total_realized,
            fills_total: total_fills,
            last_24h_count,
            by_protocol,
        })
    }
}

/// Aggregate P&L snapshot returned by `OutcomeLog::pnl_summary`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PnlSummary {
    pub realized_usd_total: f64,
    pub fills_total: i64,
    pub last_24h_count: i64,
    pub by_protocol: std::collections::HashMap<String, ProtocolPnl>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolPnl {
    pub fills: i64,
    pub realized_usd: f64,
    pub avg_profit_usd: f64,
}
