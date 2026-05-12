//! AttestationPump — closes the donut-attestation loop.
//!
//! Every confirmed fill writes an [`OutcomeRecord`] to the executor's
//! SQLite outcome log. The donut adjudicator can produce a signed
//! [`DonutAttestation`] from each record, and the `/api/donut/attest`
//! endpoint persists them into a tamper-evident hash-chained ledger.
//!
//! This module is the **wire** between the two: a background task that
//!
//!   1. on boot, calls `GET /api/donut/ledger/{spinner_id}/head` to learn
//!      the current `prev_hash` and bootstraps a `seen_fill_ids` set from
//!      the full ledger,
//!   2. polls `OutcomeLog::list_executed` every `poll_interval`,
//!   3. signs an attestation for every new fill_id, and
//!   4. POSTs each one to `/api/donut/attest` with `Bearer` auth, advancing
//!      `last_prev_hash` on 200, treating 409 as already-persisted, and
//!      resyncing via `/head` on 422.
//!
//! The pump is **best-effort**: SQLite is the source of truth. Any HTTP
//! failure is logged and retried on the next tick — the pump never panics
//! the host process.

use anyhow::Result;
use donut_adjudicator::{
    hash_for_chain, spinner_id_from_addr, AdapterRegistry, CanonicalAdjudicator,
    DonutAttestation, FeeSplitAdjudicator, ZERO_HASH,
};
use executor::{OutcomeLog, OutcomeRecord};
use alloy::signers::local::PrivateKeySigner;
use alloy::primitives::Address;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// Synthesise the same `fill_id` the adjudicator embeds in its
/// [`DonutAttestation`]. Kept in sync with `derive_fill_id` in
/// `donut-adjudicator/src/lib.rs` — both must agree or the seen-set
/// dedup will leak duplicates past the pump and into the server.
fn derive_fill_id(rec: &OutcomeRecord) -> String {
    match rec.tx_hash.as_deref() {
        Some(h) if !h.is_empty() => format!("{}:{}", rec.intent_id, h),
        _ => format!(
            "{}:{}",
            rec.intent_id,
            rec.ts.timestamp_nanos_opt().unwrap_or(0)
        ),
    }
}

#[derive(Debug, Clone)]
pub struct AttestationPumpConfig {
    /// Solver-API base URL, e.g. `http://localhost:8082`. No trailing slash
    /// required — we strip it before composing endpoints.
    pub api_base_url: String,
    /// `SOLVER_API_TOKEN` — sent as `Authorization: Bearer <token>` on every
    /// `/api/donut/attest` POST.
    pub api_token: String,
    /// How often to poll the OutcomeLog for new fills.
    pub poll_interval: Duration,
}

impl Default for AttestationPumpConfig {
    fn default() -> Self {
        Self {
            api_base_url: "http://localhost:8082".to_string(),
            api_token: String::new(),
            poll_interval: Duration::from_secs(30),
        }
    }
}

/// Ledger-head response from `GET /api/donut/ledger/:spinner_id/head`.
#[derive(Debug, serde::Deserialize)]
struct HeadResponse {
    #[serde(default)]
    prev_hash: Option<String>,
    #[serde(default)]
    count: Option<u64>,
}

/// Full-ledger response from `GET /api/donut/ledger/:spinner_id`. We only
/// care about extracting `fill_id`s so the seen-set is correctly primed
/// against the remote on boot.
#[derive(Debug, serde::Deserialize)]
struct LedgerResponse {
    #[serde(default)]
    attestations: Vec<LedgerRow>,
}

#[derive(Debug, serde::Deserialize)]
struct LedgerRow {
    fill_id: String,
}

/// Spawn the attestation pump. Returns the [`tokio::task::JoinHandle`] of
/// the running task; callers usually let it drop and rely on tokio's
/// detached-task semantics — the loop runs until the runtime is shut down.
pub fn spawn_attestation_pump(
    outcome_log: Arc<OutcomeLog>,
    signer: PrivateKeySigner,
    config: AttestationPumpConfig,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        if let Err(e) = run_pump(outcome_log, signer, config).await {
            // run_pump should loop forever; if it returns Err we log and
            // exit gracefully rather than panicking the host process.
            error!("attestation_pump exited unexpectedly: {}", e);
        }
    })
}

async fn run_pump(
    outcome_log: Arc<OutcomeLog>,
    signer: PrivateKeySigner,
    config: AttestationPumpConfig,
) -> Result<()> {
    let registry = AdapterRegistry::load_default();
    let adapters = registry.builders.len();
    if registry.ecosystem == Address::ZERO {
        warn!(
            "📮 attestation_pump: AdapterRegistry ecosystem is Address::ZERO — fail-closed posture is active. \
             Set ADAPTER_REGISTRY_PATH to a valid file before going live; every donut share will route to ZERO otherwise."
        );
    } else {
        info!(
            "📮 attestation_pump: AdapterRegistry loaded ({} adapters, ecosystem={})",
            adapters,
            registry.ecosystem
        );
    }

    let token_preview: String = if config.api_token.len() >= 8 {
        config.api_token[..8].to_string()
    } else {
        // Never log the full token; truncate to whatever we have.
        config.api_token.chars().take(8).collect()
    };
    let base = config.api_base_url.trim_end_matches('/').to_string();
    let spinner_addr = signer.address();
    let spinner_id = spinner_id_from_addr(&spinner_addr);
    info!(
        "📮 attestation_pump: spinner_id={} spinner_addr={} api_base={} token_prefix={}…",
        spinner_id, spinner_addr, base, token_preview
    );

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    // ── Bootstrap state ──────────────────────────────────────────────────
    let mut seen: HashSet<String> = HashSet::new();
    match bootstrap_seen_set(&http, &base, &spinner_id).await {
        Ok(s) => {
            info!(
                "📮 attestation_pump: bootstrapped {} fill_id(s) from remote ledger",
                s.len()
            );
            seen = s;
        }
        Err(e) => {
            warn!(
                "📮 attestation_pump: ledger bootstrap failed ({}) — proceeding with empty seen-set; duplicates will be deduped server-side via 409",
                e
            );
        }
    }

    let mut last_prev_hash = match fetch_head(&http, &base, &spinner_id).await {
        Ok(h) => {
            info!("📮 attestation_pump: head prev_hash={}", h);
            h
        }
        Err(e) => {
            warn!(
                "📮 attestation_pump: head fetch failed ({}) — starting from ZERO_HASH",
                e
            );
            ZERO_HASH.to_string()
        }
    };

    let adjudicator = CanonicalAdjudicator;
    let mut last_ts: Option<chrono::DateTime<chrono::Utc>> = None;

    loop {
        let mut scanned = 0usize;
        let mut attested = 0usize;
        let mut skipped = 0usize;
        let mut failed = 0usize;

        let rows = match outcome_log.list_executed(500, last_ts) {
            Ok(rs) => rs,
            Err(e) => {
                warn!("📮 attestation_pump: list_executed failed: {}", e);
                tokio::time::sleep(config.poll_interval).await;
                continue;
            }
        };

        for rec in rows {
            scanned += 1;
            let fid = derive_fill_id(&rec);
            // Track ts watermark even for skips so we don't re-scan rows
            // we've already decided about.
            if last_ts.map(|t| rec.ts > t).unwrap_or(true) {
                last_ts = Some(rec.ts);
            }
            if seen.contains(&fid) {
                skipped += 1;
                continue;
            }

            match attest_and_post(
                &adjudicator,
                &rec,
                &registry,
                &signer,
                &last_prev_hash,
                &http,
                &base,
                &config.api_token,
                &spinner_id,
            )
            .await
            {
                Ok(PostOutcome::Persisted(next_hash)) => {
                    seen.insert(fid);
                    last_prev_hash = next_hash;
                    attested += 1;
                }
                Ok(PostOutcome::Duplicate) => {
                    // Server already has this fill_id — record it locally so
                    // we don't keep retrying. Refresh prev_hash so the next
                    // attestation lines up with the server's actual head.
                    seen.insert(fid);
                    skipped += 1;
                    if let Ok(h) = fetch_head(&http, &base, &spinner_id).await {
                        last_prev_hash = h;
                    }
                }
                Ok(PostOutcome::Resynced(h)) => {
                    // 422 → bad chain link, retried once with refreshed head
                    // and still failed. Skip; operator will need to investigate.
                    last_prev_hash = h;
                    failed += 1;
                }
                Err(e) => {
                    debug!("📮 attestation_pump: post failed for {}: {}", fid, e);
                    failed += 1;
                }
            }
        }

        info!(
            "📮 attestation_pump tick: scanned={} attested={} skipped={} failed={} seen_total={}",
            scanned,
            attested,
            skipped,
            failed,
            seen.len()
        );

        tokio::time::sleep(config.poll_interval).await;
    }
}

/// Outcome of a single attestation POST. Mirrors the status codes documented
/// at the top of `solver-api::donut_attest_handler`.
enum PostOutcome {
    /// 200 — server persisted the attestation; carries the new chain head.
    Persisted(String),
    /// 409 — server already has this fill_id; nothing to do but mark it seen.
    Duplicate,
    /// 422 → re-fetched head → retry → still failed. Caller skips this rec.
    /// Carries the most recent head we observed.
    Resynced(String),
}

#[allow(clippy::too_many_arguments)]
async fn attest_and_post(
    adjudicator: &CanonicalAdjudicator,
    rec: &OutcomeRecord,
    registry: &AdapterRegistry,
    signer: &PrivateKeySigner,
    prev_hash: &str,
    http: &reqwest::Client,
    base: &str,
    api_token: &str,
    spinner_id: &str,
) -> Result<PostOutcome> {
    let att = adjudicator
        .attest(rec, registry, signer, prev_hash)
        .await?;
    let outcome = post_attestation(http, base, api_token, &att).await?;
    match outcome {
        PostStatus::Ok => {
            let next = hash_for_chain(&att)?;
            Ok(PostOutcome::Persisted(next))
        }
        PostStatus::Duplicate => Ok(PostOutcome::Duplicate),
        PostStatus::BadChain => {
            // Re-fetch head, rebuild attestation against the new prev_hash,
            // and try exactly once more before giving up.
            let new_head = fetch_head(http, base, spinner_id).await.unwrap_or_else(|_| {
                warn!("📮 attestation_pump: head re-fetch after 422 failed");
                prev_hash.to_string()
            });
            let att2 = adjudicator
                .attest(rec, registry, signer, &new_head)
                .await?;
            match post_attestation(http, base, api_token, &att2).await? {
                PostStatus::Ok => {
                    let next = hash_for_chain(&att2)?;
                    Ok(PostOutcome::Persisted(next))
                }
                PostStatus::Duplicate => Ok(PostOutcome::Duplicate),
                PostStatus::BadChain => {
                    error!(
                        "📮 attestation_pump: chain link still bad after resync for fill_id={} — operator should investigate",
                        att2.fill_id
                    );
                    Ok(PostOutcome::Resynced(new_head))
                }
                PostStatus::Other(code, body) => {
                    warn!(
                        "📮 attestation_pump: retry after 422 returned {}: {}",
                        code, body
                    );
                    Ok(PostOutcome::Resynced(new_head))
                }
            }
        }
        PostStatus::Other(code, body) => {
            warn!(
                "📮 attestation_pump: server returned {} for fill_id={}: {}",
                code, att.fill_id, body
            );
            // Not seen-set'd — next poll cycle naturally retries.
            anyhow::bail!("server returned {}: {}", code, body)
        }
    }
}

enum PostStatus {
    Ok,
    Duplicate,
    BadChain,
    Other(u16, String),
}

async fn post_attestation(
    http: &reqwest::Client,
    base: &str,
    api_token: &str,
    att: &DonutAttestation,
) -> Result<PostStatus> {
    let url = format!("{}/api/donut/attest", base);
    let resp = http
        .post(&url)
        .bearer_auth(api_token)
        .json(att)
        .send()
        .await?;
    let status = resp.status();
    match status.as_u16() {
        200 => Ok(PostStatus::Ok),
        409 => Ok(PostStatus::Duplicate),
        422 => Ok(PostStatus::BadChain),
        other => {
            let body = resp.text().await.unwrap_or_default();
            Ok(PostStatus::Other(other, body))
        }
    }
}

async fn fetch_head(
    http: &reqwest::Client,
    base: &str,
    spinner_id: &str,
) -> Result<String> {
    let url = format!("{}/api/donut/ledger/{}/head", base, spinner_id);
    let resp = http.get(&url).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("head endpoint returned {}", resp.status());
    }
    let head: HeadResponse = resp.json().await?;
    Ok(head.prev_hash.unwrap_or_else(|| ZERO_HASH.to_string()))
}

async fn bootstrap_seen_set(
    http: &reqwest::Client,
    base: &str,
    spinner_id: &str,
) -> Result<HashSet<String>> {
    let url = format!("{}/api/donut/ledger/{}", base, spinner_id);
    let resp = http.get(&url).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("ledger endpoint returned {}", resp.status());
    }
    let ledger: LedgerResponse = resp.json().await?;
    Ok(ledger.attestations.into_iter().map(|r| r.fill_id).collect())
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn rec(intent: &str, tx: Option<&str>) -> OutcomeRecord {
        OutcomeRecord {
            ts: Utc::now(),
            intent_id: intent.into(),
            protocol: "across".into(),
            src_chain: 1,
            dst_chain: 42161,
            decision: "executed".into(),
            tx_hash: tx.map(|s| s.into()),
            predicted_gas: None,
            gas_used: Some(100_000),
            effective_gas_price_wei: None,
            predicted_profit_usd: Some(0.50),
            actual_profit_usd: Some(0.50),
            skip_reason: None,
            error: None,
            solver_id: Some("00000000".into()),
            claim_tx_hash: None,
            claim_fee_usd: None,
            fee_usd: Some(0.50),
        }
    }

    /// The pump's dedup logic: a fill whose `fill_id` is already in
    /// `seen_fill_ids` MUST be skipped. Exercises the in-memory branch
    /// without touching HTTP or SQLite.
    #[test]
    fn pump_skips_already_seen_fill_ids() {
        let r1 = rec("intent-1", Some("0xaaa"));
        let r2 = rec("intent-1", Some("0xaaa")); // identical fill_id
        let r3 = rec("intent-2", Some("0xbbb")); // distinct

        let mut seen: HashSet<String> = HashSet::new();

        let fid1 = derive_fill_id(&r1);
        assert!(!seen.contains(&fid1));
        seen.insert(fid1.clone());

        let fid2 = derive_fill_id(&r2);
        assert!(
            seen.contains(&fid2),
            "duplicate fill_id should be detected by the seen-set"
        );

        let fid3 = derive_fill_id(&r3);
        assert!(
            !seen.contains(&fid3),
            "distinct fill_id should NOT be considered seen"
        );

        // And the fill_ids derived here must match what the adjudicator
        // embeds — both helpers key off (intent_id, tx_hash) the same way.
        assert_eq!(fid1, "intent-1:0xaaa");
        assert_eq!(fid3, "intent-2:0xbbb");
    }

    /// Fallback path: when `tx_hash` is missing we synthesise the fill_id
    /// from `intent_id` + `ts.nanos`. Two records with the same intent
    /// but distinct timestamps must produce different fill_ids so the
    /// seen-set never collapses unrelated skip rows.
    #[test]
    fn fill_id_falls_back_to_ts_when_tx_hash_missing() {
        let mut a = rec("intent-x", None);
        let mut b = rec("intent-x", None);
        a.ts = chrono::DateTime::<Utc>::from_timestamp(1, 0).unwrap();
        b.ts = chrono::DateTime::<Utc>::from_timestamp(2, 0).unwrap();
        assert_ne!(derive_fill_id(&a), derive_fill_id(&b));
    }
}
