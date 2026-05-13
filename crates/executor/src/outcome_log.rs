//! Append-only outcome log for solver fills.
//!
//! Storage selection (per spec, section 4.3):
//! - SQLite always (durable local record).
//! - If MAMBA_LAKE_URL is set + reachable, mirror outcomes via async HTTP POST.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
    /// Solver identifier — solver address hex or a short human name.
    /// Allows per-solver filtering on rpc.taifoon.dev and in the dashboard.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solver_id: Option<String>,
    /// Source-chain `claimUnlock()` tx hash for protocols that require an
    /// explicit claim (deBridge today). Populated by `update_claim` after
    /// the claim receipt confirms; NULL while the claim is still pending.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_tx_hash: Option<String>,
    /// USD value of the spread released by the claim. Co-populated with
    /// `claim_tx_hash`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_fee_usd: Option<f64>,
    /// **Gross fee** paid by the intent's submitter for filling — the
    /// number the donut adjudicator redistributes per the per-adapter
    /// inflow redistribution policy (see `donut-adjudicator` crate).
    /// Decoded by the executor from the SSE Genome intent BEFORE the fill
    /// broadcasts:
    ///
    /// | Protocol      | Source of `fee_usd`                                    |
    /// |---------------|---------------------------------------------------------|
    /// | Across V3     | `inputAmount − outputAmount` × token-USD-price          |
    /// | deBridge DLN  | `giveAmount − takeAmount` × token-USD-price             |
    /// | Mayan Swift   | auction-winning fee declared in the intent              |
    /// | LiFi          | embedded relay fee in the calldata                      |
    /// | Wormhole NTT  | bridge-fee field on the NTT message                     |
    ///
    /// Distinct from `actual_profit_usd` — that's `fee_usd - gas_cost_usd`.
    /// The donut is taken from the fee (revenue), gas is the Spinner's
    /// own cost. None when the executor can't decode a fee (very old
    /// intents, dry-runs); in that case the adjudicator falls back to
    /// `max(0, actual_profit_usd)` as a conservative substitute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fee_usd: Option<f64>,

    // ── Venue attribution (introduced with `taifoon-trade` Hand routing) ────
    //
    // These three fields tell us WHERE a fill landed, independently of which
    // PROTOCOL the source intent came from. For legacy adapter-routed fills
    // they stay NULL (the `protocol` column carries the venue implicitly).
    // For Hand-routed fills the executor populates them at write time so the
    // dashboard can `GROUP BY venue` alongside `GROUP BY protocol`.

    /// Stable venue identifier — matches `trade-core::Venue` display string
    /// (e.g. "kraken", "drift-perps", "binance", "spinner", or an
    /// operator-supplied `custom:...` id). NULL for legacy adapter fills.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub venue: Option<String>,

    /// True if the fill landed on a venue this operator runs themselves
    /// (as opposed to an external venue like Kraken or Drift). The
    /// distinction matters for the donut split — internal-book fills
    /// carry the full donut, external fills only carry the routing share.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_native_book: Option<bool>,

    /// Market identifier as the venue itself uses it
    /// (e.g. "SOL-PERP" for Drift, "XBT/USD" for Kraken, "BTC/USDC" for an
    /// internal market). NULL when the fill is cross-chain rather than
    /// book-resident.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub book_market_id: Option<String>,
}

#[derive(Clone)]
pub struct OutcomeLog {
    inner: Arc<OutcomeLogInner>,
}

struct OutcomeLogInner {
    sqlite: Mutex<Connection>,
    mamba_url: Option<String>,
    /// Mirror endpoint: POST fills to rpc.taifoon.dev/api/solver/fills
    rpc_taifoon_url: Option<String>,
    http: reqwest::Client,
}

impl OutcomeLog {
    /// Open / create a SQLite-backed log, optionally mirroring to mamba DuckDB
    /// and to rpc.taifoon.dev.
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
                error                    TEXT,
                solver_id                TEXT,
                venue                    TEXT,
                is_native_book           INTEGER,
                book_market_id           TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_outcomes_intent   ON solver_outcomes(intent_id);
            CREATE INDEX IF NOT EXISTS idx_outcomes_ts       ON solver_outcomes(ts);
            CREATE INDEX IF NOT EXISTS idx_outcomes_solver   ON solver_outcomes(solver_id);
            CREATE INDEX IF NOT EXISTS idx_outcomes_venue    ON solver_outcomes(venue);",
        )?;
        // Backfill columns added in later schema versions. SQLite errors if column
        // already exists — those are intentionally ignored. Each ALTER is run
        // standalone so a single column already being present doesn't block the
        // others from applying.
        for stmt in [
            "ALTER TABLE solver_outcomes ADD COLUMN predicted_gas  INTEGER",
            "ALTER TABLE solver_outcomes ADD COLUMN skip_reason    TEXT",
            "ALTER TABLE solver_outcomes ADD COLUMN solver_id      TEXT",
            "ALTER TABLE solver_outcomes ADD COLUMN claim_tx_hash  TEXT",
            "ALTER TABLE solver_outcomes ADD COLUMN claim_fee_usd  REAL",
            "ALTER TABLE solver_outcomes ADD COLUMN fee_usd        REAL",
            // Venue attribution columns (added with taifoon-trade Hand routing).
            // SQLite has no native BOOLEAN — `is_native_book` stores 0/1 as INTEGER.
            "ALTER TABLE solver_outcomes ADD COLUMN venue          TEXT",
            "ALTER TABLE solver_outcomes ADD COLUMN is_native_book INTEGER",
            "ALTER TABLE solver_outcomes ADD COLUMN book_market_id TEXT",
        ] {
            let _ = conn.execute(stmt, []);
        }
        // The venue index is created here because the column may not exist on
        // pre-migration DBs at the time the CREATE TABLE block runs above
        // (CREATE INDEX IF NOT EXISTS is fine even if the column predates the
        // index — re-running is a no-op).
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_outcomes_venue ON solver_outcomes(venue)",
            [],
        );

        let mamba_url = match mamba_url {
            Some(url) if !url.is_empty() => {
                info!("📊 OutcomeLog: mamba mirror configured at {}", url);
                Some(url)
            }
            _ => None,
        };

        let rpc_taifoon_url = match std::env::var("TAIFOON_RPC_URL")
            .or_else(|_| std::env::var("WARMBED_API_URL"))
        {
            Ok(url) if !url.is_empty() => {
                info!("📡 OutcomeLog: rpc.taifoon.dev mirror at {}", url);
                Some(url)
            }
            _ => None,
        };

        Ok(Self {
            inner: Arc::new(OutcomeLogInner {
                sqlite: Mutex::new(conn),
                mamba_url,
                rpc_taifoon_url,
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
                     predicted_profit_usd, actual_profit_usd, skip_reason, error, solver_id,
                     claim_tx_hash, claim_fee_usd, fee_usd,
                     venue, is_native_book, book_market_id)
                 VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
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
                    rec.solver_id,
                    rec.claim_tx_hash,
                    rec.claim_fee_usd,
                    rec.fee_usd,
                    rec.venue,
                    rec.is_native_book.map(|b| if b { 1_i64 } else { 0_i64 }),
                    rec.book_market_id,
                ],
            )?;
        }
        debug!("💾 outcome logged: {} {}", rec.intent_id, rec.decision);

        let http = self.inner.http.clone();
        let payload = rec;

        if let Some(url) = self.inner.mamba_url.clone() {
            let h = http.clone();
            let p = payload.clone();
            tokio::spawn(async move {
                let endpoint = format!("{}/api/solver/outcomes", url.trim_end_matches('/'));
                if let Err(e) = h.post(&endpoint).json(&p).send().await {
                    warn!("📊 mamba mirror failed: {}", e);
                }
            });
        }

        if let Some(url) = self.inner.rpc_taifoon_url.clone() {
            let p = payload;
            tokio::spawn(async move {
                let endpoint = format!("{}/api/solver/fills", url.trim_end_matches('/'));
                if let Err(e) = http.post(&endpoint).json(&p).send().await {
                    warn!("📡 rpc.taifoon mirror failed: {}", e);
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
    /// If `solver_id` is Some, only return records for that solver.
    pub fn recent(&self, limit: i64) -> Result<Vec<OutcomeRecord>> {
        self.recent_for(limit, None)
    }

    pub fn recent_for(&self, limit: i64, solver_id: Option<&str>) -> Result<Vec<OutcomeRecord>> {
        let conn = self.inner.sqlite.lock().unwrap();
        let sql = if solver_id.is_some() {
            "SELECT ts, intent_id, protocol, src_chain, dst_chain, decision, tx_hash,
                    predicted_gas, gas_used, effective_gas_price_wei,
                    predicted_profit_usd, actual_profit_usd, skip_reason, error, solver_id,
                    claim_tx_hash, claim_fee_usd, fee_usd,
                    venue, is_native_book, book_market_id
             FROM solver_outcomes
             WHERE solver_id = ?1
             ORDER BY ts DESC
             LIMIT ?2"
        } else {
            "SELECT ts, intent_id, protocol, src_chain, dst_chain, decision, tx_hash,
                    predicted_gas, gas_used, effective_gas_price_wei,
                    predicted_profit_usd, actual_profit_usd, skip_reason, error, solver_id,
                    claim_tx_hash, claim_fee_usd, fee_usd,
                    venue, is_native_book, book_market_id
             FROM solver_outcomes
             ORDER BY ts DESC
             LIMIT ?1"
        };

        let parse_row = |r: &rusqlite::Row<'_>| -> rusqlite::Result<OutcomeRecord> {
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
                solver_id: r.get(14)?,
                claim_tx_hash: r.get(15)?,
                claim_fee_usd: r.get(16)?,
                fee_usd: r.get(17)?,
                venue: r.get(18)?,
                is_native_book: r.get::<_, Option<i64>>(19)?.map(|v| v != 0),
                book_market_id: r.get(20)?,
            })
        };

        let mut out = Vec::new();
        if let Some(sid) = solver_id {
            let mut stmt = conn.prepare(sql)?;
            let rows = stmt.query_map(params![sid, limit], parse_row)?;
            for r in rows { out.push(r?); }
        } else {
            let mut stmt = conn.prepare(sql)?;
            let rows = stmt.query_map(params![limit], parse_row)?;
            for r in rows { out.push(r?); }
        }
        Ok(out)
    }

    /// Mark the most recent fill row for `intent_id` as claimed. Issue #10:
    /// after `claimUnlock()` confirms on the source chain, we write the claim
    /// tx hash and the spread USD onto the executed-fill row so the dashboard
    /// (`/api/solver/outcomes`) can render claim status without a separate
    /// table, and the sidecar's retry loop can skip `claim_tx_hash IS NOT NULL`
    /// rows on subsequent ticks.
    ///
    /// Targets the newest row carrying a non-NULL `tx_hash` for the intent —
    /// matches the `executed` row written by `lambda_execute` and avoids the
    /// `enrichment_failed` / `skip_*` rows that share the same `intent_id`
    /// but have no tx_hash to claim against.
    pub fn update_claim(
        &self,
        intent_id: &str,
        claim_tx_hash: &str,
        claim_fee_usd: f64,
    ) -> Result<usize> {
        let conn = self.inner.sqlite.lock().unwrap();
        let n = conn.execute(
            "UPDATE solver_outcomes
             SET claim_tx_hash = ?1, claim_fee_usd = ?2
             WHERE rowid = (
                SELECT rowid FROM solver_outcomes
                WHERE intent_id = ?3 AND tx_hash IS NOT NULL
                ORDER BY ts DESC LIMIT 1
             )",
            params![claim_tx_hash, claim_fee_usd, intent_id],
        )?;
        if n == 0 {
            warn!(
                "update_claim: no executed row found for intent_id={} — claim revenue not recorded in outcomes table",
                intent_id
            );
        }
        Ok(n)
    }

    /// Return intent_ids of fills (deBridge-protocol rows with a tx_hash)
    /// that have not yet had `claim_tx_hash` populated. Used by the
    /// `debridge_claim_retry_tick` loop to skip already-claimed fills
    /// (Issue #10 acceptance criterion).
    pub fn unclaimed_debridge_intents(&self, limit: i64) -> Result<Vec<String>> {
        let conn = self.inner.sqlite.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT intent_id FROM solver_outcomes
             WHERE claim_tx_hash IS NULL
               AND tx_hash IS NOT NULL
               AND (LOWER(protocol) LIKE '%debridge%' OR LOWER(protocol) LIKE '%dln%')
               AND decision IN ('executed','confirmed','execute')
             ORDER BY ts DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Look up the most recent `tx_hash` for an intent across the outcome log.
    /// Used by `/api/solver/claims` to recover the original fill tx after a
    /// successful claim has overwritten `wallet.intents.tx_hash` with the
    /// claim tx. Returns the newest non-NULL tx_hash for the intent — when
    /// multiple decisions exist (e.g. `execute` then `claim_unlock`) the
    /// newest fires first, so callers should prefer this only for fill-tx
    /// recovery on rows whose wallet state is `CLAIMED`.
    pub fn first_fill_tx_for(&self, intent_id: &str) -> Result<Option<String>> {
        let conn = self.inner.sqlite.lock().unwrap();
        let row: rusqlite::Result<Option<String>> = conn.query_row(
            "SELECT tx_hash FROM solver_outcomes \
             WHERE intent_id = ?1 \
               AND tx_hash IS NOT NULL \
               AND decision IN ('confirmed','execute','executed') \
             ORDER BY ts ASC LIMIT 1",
            params![intent_id],
            |r| r.get::<_, Option<String>>(0),
        );
        // query_row returns NotFound as Err; normalize to Ok(None).
        match row {
            Ok(v) => Ok(v),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Read up to `limit` `executed` rows whose `ts` is strictly greater
    /// than `since_ts` (or all `executed` rows when `since_ts` is None).
    /// Returns in `ts ASC` order so the AttestationPump can replay them
    /// in chain-link order.
    ///
    /// "Executed" here means the row's `decision` is one of
    /// `executed` / `execute` / `confirmed` AND `tx_hash` is non-NULL —
    /// matches the criterion the donut-adjudicator uses to derive a stable
    /// `fill_id`.
    pub fn list_executed(
        &self,
        limit: usize,
        since_ts: Option<DateTime<Utc>>,
    ) -> Result<Vec<OutcomeRecord>> {
        let conn = self.inner.sqlite.lock().unwrap();
        let parse_row = |r: &rusqlite::Row<'_>| -> rusqlite::Result<OutcomeRecord> {
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
                solver_id: r.get(14)?,
                claim_tx_hash: r.get(15)?,
                claim_fee_usd: r.get(16)?,
                fee_usd: r.get(17)?,
                venue: r.get(18)?,
                is_native_book: r.get::<_, Option<i64>>(19)?.map(|v| v != 0),
                book_market_id: r.get(20)?,
            })
        };

        let base_select = "SELECT ts, intent_id, protocol, src_chain, dst_chain, decision, tx_hash,
                    predicted_gas, gas_used, effective_gas_price_wei,
                    predicted_profit_usd, actual_profit_usd, skip_reason, error, solver_id,
                    claim_tx_hash, claim_fee_usd, fee_usd,
                    venue, is_native_book, book_market_id
             FROM solver_outcomes
             WHERE decision IN ('executed','execute','confirmed')
               AND tx_hash IS NOT NULL";

        let mut out = Vec::new();
        match since_ts {
            Some(ts) => {
                let sql = format!("{} AND ts > ?1 ORDER BY ts ASC LIMIT ?2", base_select);
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map(params![ts.to_rfc3339(), limit as i64], parse_row)?;
                for r in rows {
                    out.push(r?);
                }
            }
            None => {
                let sql = format!("{} ORDER BY ts ASC LIMIT ?1", base_select);
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map(params![limit as i64], parse_row)?;
                for r in rows {
                    out.push(r?);
                }
            }
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
        // last_24h_count: executed fills (not skips) in the last 24 h — matches the
        //   dashboard "LAST 24H" pill which is labeled as fills, not all events.
        let (total_realized, total_fills, last_24h_count): (f64, i64, i64) = conn.query_row(
            "SELECT
                COALESCE(SUM(CASE WHEN actual_profit_usd IS NOT NULL THEN actual_profit_usd ELSE 0.0 END), 0.0),
                COALESCE(SUM(CASE WHEN decision IN ('confirmed', 'execute', 'executed') THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN decision IN ('confirmed', 'execute', 'executed')
                                   AND ts >= datetime('now', '-1 day') THEN 1 ELSE 0 END), 0)
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

    /// Per-venue P&L aggregation. Same shape as `pnl_summary()` but the
    /// breakdown is keyed by the `venue` column (Hand-routed fills) rather
    /// than `protocol`. Rows with NULL `venue` (legacy adapter-routed fills)
    /// are bucketed under the literal key `"legacy"` so the dashboard can
    /// surface them alongside the new venue-attributed counts.
    pub fn pnl_by_venue(&self) -> Result<HashMap<String, ProtocolPnl>> {
        let conn = self.inner.sqlite.lock().unwrap();
        let mut by_venue = HashMap::<String, ProtocolPnl>::new();
        let mut stmt = conn.prepare(
            "SELECT COALESCE(venue, 'legacy') AS v,
                    COUNT(*)                  AS fills,
                    COALESCE(SUM(actual_profit_usd), 0.0) AS realized,
                    COALESCE(AVG(actual_profit_usd), 0.0) AS avg_profit
             FROM solver_outcomes
             WHERE actual_profit_usd IS NOT NULL
             GROUP BY v",
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
            let (venue, fills, realized, avg) = row?;
            by_venue.insert(
                venue,
                ProtocolPnl {
                    fills,
                    realized_usd: realized,
                    avg_profit_usd: avg,
                },
            );
        }
        Ok(by_venue)
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────
//
// These cover the venue-attribution columns end-to-end:
//   1. fresh DB round-trips a record with venue/is_native_book/book_market_id
//   2. pnl_by_venue() groups correctly, NULLs bucket as "legacy"
//   3. schema migration is idempotent on a pre-existing legacy DB
//   4. legacy rows (no venue) read back as `venue: None` after the migration

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(intent_id: &str, profit: f64, venue: Option<&str>, native: Option<bool>) -> OutcomeRecord {
        OutcomeRecord {
            ts: Utc::now(),
            intent_id: intent_id.into(),
            protocol: "across".into(),
            src_chain: 1,
            dst_chain: 8453,
            decision: "executed".into(),
            tx_hash: Some("0xdead".into()),
            predicted_gas: Some(200_000),
            gas_used: Some(195_000),
            effective_gas_price_wei: Some("3000000000".into()),
            predicted_profit_usd: Some(profit),
            actual_profit_usd: Some(profit),
            skip_reason: None,
            error: None,
            solver_id: Some("solver-a".into()),
            claim_tx_hash: None,
            claim_fee_usd: None,
            fee_usd: Some(profit + 0.05),
            venue: venue.map(|s| s.into()),
            is_native_book: native,
            book_market_id: venue.map(|_| "ETH/USDC".into()),
        }
    }

    #[test]
    fn round_trip_venue_columns() {
        let log = OutcomeLog::open(":memory:", None).expect("open");
        log.append(rec("i1", 1.23, Some("kraken"), Some(false))).unwrap();
        log.append(rec("i2", 4.56, Some("internal-sol"), Some(true))).unwrap();
        log.append(rec("i3", 7.89, None, None)).unwrap();
        let rows = log.recent(10).unwrap();
        assert_eq!(rows.len(), 3);
        // Rows are returned newest-first, but we don't care about order — assert by intent_id.
        let i1 = rows.iter().find(|r| r.intent_id == "i1").unwrap();
        assert_eq!(i1.venue.as_deref(), Some("kraken"));
        assert_eq!(i1.is_native_book, Some(false));
        assert_eq!(i1.book_market_id.as_deref(), Some("ETH/USDC"));
        let i2 = rows.iter().find(|r| r.intent_id == "i2").unwrap();
        assert_eq!(i2.venue.as_deref(), Some("internal-sol"));
        assert_eq!(i2.is_native_book, Some(true));
        let i3 = rows.iter().find(|r| r.intent_id == "i3").unwrap();
        assert!(i3.venue.is_none());
        assert!(i3.is_native_book.is_none());
        assert!(i3.book_market_id.is_none());
    }

    #[test]
    fn pnl_by_venue_groups_and_buckets_legacy() {
        let log = OutcomeLog::open(":memory:", None).expect("open");
        log.append(rec("a", 1.00, Some("kraken"), Some(false))).unwrap();
        log.append(rec("b", 2.00, Some("kraken"), Some(false))).unwrap();
        log.append(rec("c", 3.00, Some("drift-perps"), Some(false))).unwrap();
        log.append(rec("d", 4.00, None, None)).unwrap(); // legacy
        let by_venue = log.pnl_by_venue().unwrap();
        assert_eq!(by_venue.get("kraken").unwrap().fills, 2);
        assert!((by_venue.get("kraken").unwrap().realized_usd - 3.00).abs() < 1e-9);
        assert_eq!(by_venue.get("drift-perps").unwrap().fills, 1);
        assert_eq!(by_venue.get("legacy").unwrap().fills, 1);
        assert!((by_venue.get("legacy").unwrap().realized_usd - 4.00).abs() < 1e-9);
    }

    #[test]
    fn migration_is_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("legacy.sqlite");
        let path_str = path.to_str().unwrap();

        // First open creates the schema fresh — equivalent to a brand-new DB.
        {
            let log = OutcomeLog::open(path_str, None).unwrap();
            log.append(rec("legacy-1", 0.50, None, None)).unwrap();
        }
        // Re-open the same file. The CREATE TABLE IF NOT EXISTS + ALTER TABLE
        // statements must all be no-ops — opening again must not raise.
        let log = OutcomeLog::open(path_str, None).unwrap();
        // And the legacy row must still parse cleanly with NULL venue columns.
        let rows = log.recent(10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].intent_id, "legacy-1");
        assert!(rows[0].venue.is_none());
        assert!(rows[0].is_native_book.is_none());
    }

    #[test]
    fn pnl_summary_still_works_alongside_venue_columns() {
        let log = OutcomeLog::open(":memory:", None).expect("open");
        log.append(rec("x", 10.0, Some("kraken"), Some(false))).unwrap();
        log.append(rec("y", 20.0, None, None)).unwrap();
        let s = log.pnl_summary().unwrap();
        assert_eq!(s.fills_total, 2);
        assert!((s.realized_usd_total - 30.0).abs() < 1e-9);
        // By-protocol should still see them under `across` (the protocol field).
        assert_eq!(s.by_protocol.get("across").unwrap().fills, 2);
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
