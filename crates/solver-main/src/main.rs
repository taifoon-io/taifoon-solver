use anyhow::{anyhow, Result};
use chrono::Utc;
use executor::{
    AcrossExecutor, ChainWiring, Executor, OutcomeLog, OutcomeRecord, SkipRules,
    SpinnerSolverClient,
};
use genome_client::GenomeClient;
use profit_calc::ProfitCalculator;
use solver_api::{
    AttemptData, IntentData, SolvedData, SolverApi, SolverEvent,
};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

const DEFAULT_GENOME_SSE_URL: &str = "http://46.4.96.124:30081/api/genome/subscribe/sse";
const DEFAULT_SPINNER_BASE: &str = "http://46.4.96.124:30081";
const DEFAULT_MIN_PROFIT_USD: f64 = 0.10;
const SOLVER_INTEL_PATH: &str = "config/solver_intel.json";
const API_PORT: u16 = 8082;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_thread_ids(false)
        .with_file(false)
        .init();

    // ── Configuration ─────────────────────────────────────────────────────────
    let genome_sse_url = std::env::var("GENOME_SSE_URL")
        .unwrap_or_else(|_| DEFAULT_GENOME_SSE_URL.to_string());
    let spinner_base = std::env::var("SPINNER_API_URL")
        .unwrap_or_else(|_| DEFAULT_SPINNER_BASE.to_string());
    let min_profit_usd: f64 = std::env::var("MIN_PROFIT_USD")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_MIN_PROFIT_USD);
    let dry_run = std::env::var("DRY_RUN")
        .map(|v| v != "false" && v != "0")
        .unwrap_or(true);
    let outcome_db_path = std::env::var("OUTCOME_DB_PATH")
        .unwrap_or_else(|_| "/tmp/taifoon_solver_outcomes.sqlite".to_string());
    let mamba_lake_url = std::env::var("MAMBA_LAKE_URL").ok();
    let protocol_filter = std::env::var("PROTOCOL_FILTER")
        .unwrap_or_else(|_| "across".to_string())
        .to_lowercase();

    info!("🚀 Taifoon Solver Starting...");
    info!("📡 Genome SSE: {}", genome_sse_url);
    info!("🔌 Spinner API: {}", spinner_base);
    info!("🎯 Protocol filter: {}", protocol_filter);
    info!("💰 Min Profit: ${}", min_profit_usd);
    info!("🧪 DRY_RUN: {}", dry_run);
    info!("💾 Outcome DB: {}", outcome_db_path);
    info!("🌐 API Port: {}", API_PORT);

    // ── Solver event API (SSE for dashboard) ──────────────────────────────────
    let solver_api = SolverApi::new();
    let api_router = solver_api.router();
    tokio::spawn(async move {
        let listener = match tokio::net::TcpListener::bind(format!("0.0.0.0:{}", API_PORT)).await {
            Ok(l) => l,
            Err(e) => {
                error!("API bind {}: {}", API_PORT, e);
                return;
            }
        };
        info!("✅ Solver event API on :{}", API_PORT);
        if let Err(e) = axum::serve(listener, api_router).await {
            error!("API server error: {}", e);
        }
    });

    // ── Profit calc (used as a sanity check beside Spinner test-run) ──────────
    let mut profit_calc = ProfitCalculator::new(min_profit_usd);
    if let Err(e) = profit_calc.load_solver_intel(SOLVER_INTEL_PATH) {
        warn!("solver_intel load: {} (continuing with defaults)", e);
    }

    // ── Skip-rules (X1: self-learning loop) ───────────────────────────────────
    // Fetch once at startup. Rules are published weekly by the nemotron
    // analyzer (jarvis-ai/scripts/solver_skip_rules_weekly.py), so a periodic
    // refresh inside the running process is unnecessary — a restart picks
    // up the new set.
    let skip_rules = match mamba_lake_url.as_deref() {
        Some(url) => SkipRules::fetch(url).await,
        None => { info!("📐 skip-rules: MAMBA_LAKE_URL unset, no rules loaded"); SkipRules::empty() }
    };
    info!("📐 skip-rules active: {}", skip_rules.len());

    // Outcome log used for rule-skip records (non-Across path doesn't have one).
    let rule_skip_log = match OutcomeLog::open(&outcome_db_path, mamba_lake_url.clone()) {
        Ok(l) => Some(l),
        Err(e) => { warn!("rule-skip log init failed: {} — rule skips won't be recorded", e); None }
    };

    // ── Across executor (only built if SOLVER_PRIVATE_KEY is set) ─────────────
    let across_executor = match build_across_executor(&spinner_base, &outcome_db_path,
                                                     mamba_lake_url.clone(), dry_run, min_profit_usd) {
        Ok(Some(ex)) => {
            info!("✅ Across executor live — solver={:?}", ex.signer_address());
            Some(ex)
        }
        Ok(None) => {
            warn!("⚠️  SOLVER_PRIVATE_KEY not set — Across executor disabled (legacy adapter path only)");
            None
        }
        Err(e) => {
            error!("Across executor init failed: {}", e);
            None
        }
    };

    // ── Legacy executor (kept for non-Across protocols) ───────────────────────
    let legacy_executor = Executor::new()?;

    // ── Genome SSE consumer ───────────────────────────────────────────────────
    let genome_client = GenomeClient::new(&genome_sse_url);
    let (intent_tx, mut intent_rx) = mpsc::channel(100);
    let _genome_handle = tokio::spawn(async move {
        if let Err(e) = genome_client.subscribe(intent_tx).await {
            error!("Genome stream error: {}", e);
        }
    });
    info!("✅ Genome SSE subscriber started");
    info!("⏳ Waiting for intents...");

    // ── Main loop ─────────────────────────────────────────────────────────────
    while let Some(intent) = intent_rx.recv().await {
        info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        info!("📥 {} ({}) {}→{} amt={}", intent.id, intent.protocol,
              intent.src_chain, intent.dst_chain, intent.amount);

        solver_api.emit_event(SolverEvent::IntentDetected(IntentData {
            id: intent.id.clone(),
            protocol: intent.protocol.clone(),
            src_chain: intent.src_chain,
            dst_chain: intent.dst_chain,
            amount: intent.amount.clone(),
            token: intent.src_token.clone(),
            depositor: intent.depositor.clone(),
            recipient: intent.recipient.clone(),
            timestamp: Utc::now(),
        }));

        // ── Skip-rule fast-path (X1) ──────────────────────────────────────────
        // intent_amount_usd / current_gas_gwei aren't free at this point in the
        // pipeline (price oracle + RPC call). For now we evaluate with both
        // None — only rules that don't require those inputs (e.g. dst_chain-
        // only) can fire. Once the price oracle lands, plumb its result here.
        if let Some(reason) = skip_rules.evaluate(
            &intent.protocol,
            intent.dst_chain,
            None,
            None,
        ) {
            info!("⏭️  skip-rule fired for {} — {}", intent.id, reason);
            if let Some(log) = rule_skip_log.as_ref() {
                let _ = log.append(OutcomeRecord {
                    ts: Utc::now(),
                    intent_id: intent.id.clone(),
                    protocol: intent.protocol.clone(),
                    src_chain: intent.src_chain,
                    dst_chain: intent.dst_chain,
                    decision: "skip_rule".into(),
                    tx_hash: None,
                    predicted_gas: None,
                    gas_used: None,
                    effective_gas_price_wei: None,
                    predicted_profit_usd: None,
                    actual_profit_usd: None,
                    skip_reason: Some(reason),
                    error: None,
                });
            }
            solver_api.emit_event(SolverEvent::IntentAttempted(AttemptData {
                id: intent.id.clone(),
                profitable: false,
                profit_usd: 0.0,
                protocol_fee_usd: 0.0,
                gas_cost_usd: 0.0,
                decision: "skip".into(),
            }));
            continue;
        }

        let proto_lower = intent.protocol.to_lowercase();

        // Across path: full SSE → test-run → proof → executeWithProof → log
        if proto_lower.contains(&protocol_filter) && proto_lower.contains("across") {
            let Some(ex) = across_executor.as_ref() else {
                info!("⏭️  Across executor disabled, skipping {}", intent.id);
                continue;
            };
            match ex.fill(&intent).await {
                Ok(Some(tx)) => {
                    info!("🎉 Across fill broadcast: {}", tx);
                    solver_api.emit_event(SolverEvent::IntentSolved(SolvedData {
                        id: intent.id.clone(),
                        tx_hash: tx,
                        actual_profit_usd: 0.0,
                        gas_used: 0,
                    }));
                }
                Ok(None) => {
                    solver_api.emit_event(SolverEvent::IntentAttempted(AttemptData {
                        id: intent.id.clone(),
                        profitable: false,
                        profit_usd: 0.0,
                        protocol_fee_usd: 0.0,
                        gas_cost_usd: 0.0,
                        decision: "skip".into(),
                    }));
                }
                Err(e) => error!("❌ Across fill failed: {}", e),
            }
            continue;
        }

        // Legacy path for everything else
        match profit_calc.calculate(&intent).await {
            Ok(p) => {
                solver_api.emit_event(SolverEvent::IntentAttempted(AttemptData {
                    id: intent.id.clone(),
                    profitable: p.profitable,
                    profit_usd: p.net_profit_usd,
                    protocol_fee_usd: p.breakdown.protocol_fee_usd,
                    gas_cost_usd: p.breakdown.gas_cost_usd,
                    decision: if p.profitable { "execute".into() } else { "skip".into() },
                }));
                if p.profitable {
                    match legacy_executor.execute_fill(&intent, &p).await {
                        Ok(r) => {
                            info!("🎉 EXECUTED (legacy): {}", r.fill_tx);
                            solver_api.emit_event(SolverEvent::IntentSolved(SolvedData {
                                id: intent.id.clone(),
                                tx_hash: r.fill_tx,
                                actual_profit_usd: r.actual_profit_usd,
                                gas_used: r.gas_used,
                            }));
                        }
                        Err(e) => error!("❌ legacy execute: {}", e),
                    }
                }
            }
            Err(e) => error!("❌ profit calc: {}", e),
        }
    }

    Ok(())
}

/// Build the Across executor from env. Returns Ok(None) if SOLVER_PRIVATE_KEY is missing
/// (operator runs in observation-only mode).
fn build_across_executor(
    spinner_base: &str,
    outcome_db_path: &str,
    mamba_url: Option<String>,
    dry_run: bool,
    profit_threshold_usd: f64,
) -> Result<Option<AcrossExecutor>> {
    let pk = match std::env::var("SOLVER_PRIVATE_KEY") {
        Ok(v) if !v.is_empty() => v,
        _ => return Ok(None),
    };
    let signer: alloy::signers::local::PrivateKeySigner = pk.parse()
        .map_err(|e| anyhow!("SOLVER_PRIVATE_KEY parse: {}", e))?;

    let chains = parse_chain_wiring()?;
    if chains.is_empty() {
        return Err(anyhow!(
            "no chain wiring — set CHAIN_WIRING_JSON or per-chain RPC_/OPERATOR_/ADAPTER_ vars"
        ));
    }

    let log = OutcomeLog::open(outcome_db_path, mamba_url)?;
    let spinner = SpinnerSolverClient::new(spinner_base);

    Ok(Some(AcrossExecutor::new(
        spinner,
        signer,
        chains,
        log,
        dry_run,
        profit_threshold_usd,
    )))
}

/// Two ways to configure chain wiring:
///   A) CHAIN_WIRING_JSON='{"11155111":{"rpc_url":"...","operator":"0x...","across_adapter":"0x..."}}'
///   B) per-chain triplet:
///      CHAINS=11155111,84532
///      RPC_URL_11155111=...   OPERATOR_11155111=...   ADAPTER_11155111=...
fn parse_chain_wiring() -> Result<HashMap<u64, ChainWiring>> {
    use alloy::primitives::Address;
    let mut out = HashMap::new();

    if let Ok(json) = std::env::var("CHAIN_WIRING_JSON") {
        #[derive(serde::Deserialize)]
        struct Entry {
            rpc_url: String,
            operator: String,
            across_adapter: String,
        }
        let map: HashMap<String, Entry> = serde_json::from_str(&json)?;
        for (k, v) in map {
            let chain_id: u64 = k.parse()?;
            out.insert(
                chain_id,
                ChainWiring {
                    chain_id,
                    rpc_url: v.rpc_url,
                    operator: v.operator.parse::<Address>()?,
                    across_adapter: v.across_adapter.parse::<Address>()?,
                },
            );
        }
        return Ok(out);
    }

    if let Ok(list) = std::env::var("CHAINS") {
        for cs in list.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
            let chain_id: u64 = cs.parse()?;
            let rpc = std::env::var(format!("RPC_URL_{}", chain_id))
                .map_err(|_| anyhow!("missing RPC_URL_{}", chain_id))?;
            let operator: Address = std::env::var(format!("OPERATOR_{}", chain_id))
                .map_err(|_| anyhow!("missing OPERATOR_{}", chain_id))?
                .parse()?;
            let adapter: Address = std::env::var(format!("ADAPTER_{}", chain_id))
                .map_err(|_| anyhow!("missing ADAPTER_{}", chain_id))?
                .parse()?;
            out.insert(
                chain_id,
                ChainWiring { chain_id, rpc_url: rpc, operator, across_adapter: adapter },
            );
        }
    }
    Ok(out)
}
