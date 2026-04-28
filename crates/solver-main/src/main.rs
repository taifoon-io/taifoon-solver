use anyhow::{anyhow, Result};
use chrono::Utc;
use executor::{
    build_lambda_controller_from_env, Executor, LambdaClaimOutcome, LambdaExecuteOutcome,
    LiFiMetaRouter, OutcomeLog, OutcomeRecord, SkipRules,
};
use genome_client::{AcrossPoller, DeBridgePoller, GenomeClient};
use profit_calc::ProfitCalculator;
use solver_api::{
    AttemptData, IntentData, SolvedData, SolverApi, SolverEvent,
};
use std::collections::HashSet;
use std::sync::Arc;
use taifoon_arb_bridge::{BalanceHighHandler, StubBridge, ThresholdHandler};
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use wallet_manager::WalletManager;

const DEFAULT_GENOME_SSE_URL: &str = "https://api.taifoon.dev/api/genome/subscribe/sse";
const DEFAULT_SPINNER_BASE: &str = "https://api.taifoon.dev";
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
    let spinner_base = std::env::var("WARMBED_API_URL")
        .or_else(|_| std::env::var("SPINNER_API_URL"))
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

    // ── Wallet manager (col-p2) — backs the Lambda controller's state machine
    // and exposes /api/wallet/{status,intents} on the solver-event API port.
    let wallet_db_path = std::env::var("WALLET_DB_PATH")
        .unwrap_or_else(|_| "/tmp/taifoon_solver_wallet.sqlite".to_string());
    let wallet_budget_usd: f64 = std::env::var("WALLET_BUDGET_USD")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10_000.0);
    let wallet_manager = Arc::new(
        WalletManager::open(&wallet_db_path, wallet_budget_usd)
            .map_err(|e| anyhow!("wallet-manager open: {e}"))?,
    );
    info!(
        "💼 Wallet manager: db={} budget=${}",
        wallet_db_path, wallet_budget_usd
    );

    // ── Lambda controller (col-p4) — replaces the standalone Across executor.
    // Built only when SOLVER_PRIVATE_KEY is set and at least one chain is wired.
    let lambda_controller = match build_lambda_controller_from_env(
        &spinner_base,
        &outcome_db_path,
        mamba_lake_url.clone(),
        dry_run,
        min_profit_usd,
        wallet_manager.clone(),
    ) {
        Ok(Some(ctrl)) => {
            info!(
                "✅ Lambda controller live — solver={:?}",
                ctrl.signer.address()
            );
            Some(ctrl)
        }
        Ok(None) => {
            warn!(
                "⚠️  SOLVER_PRIVATE_KEY not set — Lambda controller disabled (legacy adapter path only)"
            );
            None
        }
        Err(e) => {
            error!("Lambda controller init failed: {}", e);
            None
        }
    };

    // ── Legacy executor (kept for non-Across protocols) ───────────────────────
    let legacy_executor = Executor::new()?;

    // ── col-p3: balance_high consolidation handler ────────────────────────────
    // STUB path: the real trigger will be a per-chain idle-USDC poll once that
    // primitive lands on wallet-manager. For now we fire the handler once at
    // startup if `WALLET_BALANCE_HIGH_USDC` is set, so the wiring is exercised
    // and observable in logs. `taifoon-arb` does not yet provide a Rust bridge,
    // so `StubBridge` just logs — see crates/taifoon-arb-bridge/src/lib.rs.
    let balance_handler = ThresholdHandler::new(StubBridge);
    if let Ok(raw) = std::env::var("WALLET_BALANCE_HIGH_USDC") {
        if let Ok(balance) = raw.parse::<f64>() {
            let src_chain: u64 = std::env::var("WALLET_BALANCE_SRC_CHAIN")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(8453); // Base
            let dst_chain: u64 = std::env::var("WALLET_BALANCE_DST_CHAIN")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1399811149); // Solana mainnet (informational chain id)
            if let Err(e) = balance_handler
                .on_balance_high(src_chain, dst_chain, balance)
                .await
            {
                warn!("balance_high handler: {}", e);
            }
        }
    }

    // ── Genome SSE consumer + Across REST + deBridge on-chain pollers ────────
    // The genome SSE stream currently only emits block/gas events — it does NOT
    // publish protocol deposit events. AcrossPoller polls the Across V3 REST API
    // directly; DeBridgePoller scans eth_getLogs for DlnSource.OrderCreated events.
    let genome_client = GenomeClient::new(&genome_sse_url);
    let (intent_tx, mut intent_rx) = mpsc::channel(100);
    let across_poller = AcrossPoller::default_mainnet();
    let debridge_poller = DeBridgePoller::default_mainnet();
    let _genome_handle = tokio::spawn(async move {
        if let Err(e) = genome_client
            .subscribe_with_all_pollers(intent_tx, vec![across_poller], Some(debridge_poller))
            .await
        {
            error!("Genome stream error: {}", e);
        }
    });
    info!("✅ Genome SSE + Across REST + deBridge on-chain pollers started");
    info!("⏳ Waiting for intents...");

    // Dedup: track intent IDs we've already dispatched in this session.
    // The genome stream emits deposit + placed + executed for the same
    // cross-chain transfer; we only want to act on `placed` (first time
    // a deposit_id is resolvable). TTL is session-scoped — no persistence needed.
    let mut dispatched: HashSet<String> = HashSet::new();

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

        // Dedup by canonical intent key (protocol + deposit_id if present, else tx_hash).
        // This prevents double-fills when deposit + placed + executed all arrive.
        let dedup_key = if let Some(dep_id) = intent.deposit_id {
            format!("{}:dep:{}", intent.protocol, dep_id)
        } else {
            intent.id.clone()
        };
        if !dispatched.insert(dedup_key.clone()) {
            info!("⏭️  dedup skip (already dispatched): {}", dedup_key);
            continue;
        }

        let proto_lower = intent.protocol.to_lowercase();

        // Lambda controller path: handles Across V3, deBridge DLN, and LiFi (via meta-router).
        //   wallet.reserve → [spinner.test_run] → [spinner.fetch_v5_proof]
        //   → build_calldata → broadcast → receipt
        //   → on CONFIRMED: wallet.release + emit genome feedback
        //   → for deBridge: fire lambda_claim_debridge (claimUnlock on src chain)
        let is_across = proto_lower.contains("across");
        let is_debridge = proto_lower.contains("debridge") || proto_lower.contains("dln");
        let is_lifi = proto_lower.contains("lifi") || proto_lower.contains("li.fi");
        let is_mayan = proto_lower.contains("mayan");
        let filter_match = protocol_filter == "all"
            || protocol_filter.split(',').any(|f| proto_lower.contains(f.trim()));

        // For LiFi, project through the meta-router to get the underlying child intent
        // then dispatch as if it were the underlying protocol directly.
        let effective_intent;
        let intent_ref = if is_lifi {
            let bridge = LiFiMetaRouter::resolve_bridge(&intent).unwrap_or_default();
            if bridge.is_empty() {
                info!("⏭️  lifi skip (missing bridge/tool field): {}", intent.id);
                // Fall through to legacy path for logging
                &intent
            } else {
                effective_intent = LiFiMetaRouter::project_to_child(&intent, &bridge);
                info!("🔀 LiFi→{} projection: {} intent id={}", bridge, intent.id, effective_intent.id);
                &effective_intent
            }
        } else {
            &intent
        };
        let effective_proto_lower = intent_ref.protocol.to_lowercase();
        let effective_is_across = effective_proto_lower.contains("across");
        let effective_is_debridge = effective_proto_lower.contains("debridge") || effective_proto_lower.contains("dln");
        let effective_is_mayan = effective_proto_lower.contains("mayan");

        let routable = is_across
            || is_debridge
            || is_mayan
            || (is_lifi && (effective_is_across || effective_is_debridge || effective_is_mayan));

        if filter_match && routable {
            // Accept intents that have:
            //   a) a depositId directly (from order/placed events), OR
            //   b) a numeric suffix in the ID (legacy "across_v3::across_197928"), OR
            //   c) a real tx_hash (proto/deposit events) — lambda controller enrichment
            //      (Strategy B) will decode the depositId from the on-chain receipt.
            // Across-specific: require depositId or real tx_hash for enrichment.
            if effective_is_across && intent_ref.deposit_id.is_none() {
                let has_id_in_str = intent_ref.id.rsplit(&[':', '_'][..])
                    .find_map(|s| s.parse::<i64>().ok())
                    .is_some();
                let has_real_tx = intent_ref.tx_hash.starts_with("0x")
                    && intent_ref.tx_hash.len() == 66
                    && !intent_ref.tx_hash.starts_with("synthetic_");
                if !has_id_in_str && !has_real_tx {
                    info!("⏭️  across_v3 skip (no depositId or tx_hash in intent): {}", intent_ref.id);
                    continue;
                }
            }
            // Skip zero-amount intents regardless of protocol.
            if intent_ref.amount == "0" && intent_ref.output_amount.as_deref().map(|s| s == "0" || s.is_empty()).unwrap_or(true) {
                info!("⏭️  {} skip (zero input+output amount): {}", intent_ref.protocol, intent_ref.id);
                continue;
            }
            let Some(ctrl) = lambda_controller.as_ref() else {
                info!("⏭️  Lambda controller disabled, skipping {}", intent_ref.id);
                continue;
            };
            match ctrl.lambda_execute(intent_ref).await {
                Ok(LambdaExecuteOutcome::Confirmed { tx_hash, gas_used }) => {
                    let proto_tag = if effective_is_debridge { "deBridge" } else if effective_is_mayan { "Mayan" } else if is_lifi { "LiFi" } else { "Across" };
                    info!("🎉 {} fill confirmed: {}", proto_tag, tx_hash);
                    solver_api.emit_event(SolverEvent::IntentSolved(SolvedData {
                        id: intent.id.clone(),
                        tx_hash,
                        actual_profit_usd: 0.0,
                        gas_used,
                    }));
                    // deBridge requires a follow-up claimUnlock on the source chain
                    // to collect the locked giveTokens.
                    if effective_is_debridge {
                        match ctrl.lambda_claim_debridge(intent_ref).await {
                            Ok(LambdaClaimOutcome::Claimed { tx_hash: claim_tx, fee_usd }) => {
                                info!("💰 deBridge claimUnlock confirmed: {} (fee ~${:.4})", claim_tx, fee_usd);
                            }
                            Ok(LambdaClaimOutcome::NotEligible { reason }) => {
                                warn!("⚠️  deBridge claim not eligible: {}", reason);
                            }
                            Ok(LambdaClaimOutcome::Failed { error: e }) => {
                                error!("❌ deBridge claimUnlock failed: {}", e);
                            }
                            Err(e) => error!("❌ deBridge lambda_claim_debridge fatal: {}", e),
                        }
                    }
                }
                Ok(LambdaExecuteOutcome::Skipped { reason }) => {
                    info!("⏭️  Skipped {}: {}", intent_ref.id, reason);
                    solver_api.emit_event(SolverEvent::IntentAttempted(AttemptData {
                        id: intent.id.clone(),
                        profitable: false,
                        profit_usd: 0.0,
                        protocol_fee_usd: 0.0,
                        gas_cost_usd: 0.0,
                        decision: "skip".into(),
                    }));
                }
                Ok(LambdaExecuteOutcome::Reverted { tx_hash, error: e }) => {
                    error!("❌ fill reverted (tx {}): {}", tx_hash, e);
                }
                Ok(LambdaExecuteOutcome::Failed { stage, error: e }) => {
                    error!("❌ lambda_execute failed at {}: {}", stage, e);
                }
                Err(e) => error!("❌ lambda_execute fatal: {}", e),
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

