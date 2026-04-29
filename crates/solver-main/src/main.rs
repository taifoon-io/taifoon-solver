use anyhow::{anyhow, Result};
use chrono::Utc;
use executor::{
    build_lambda_controller_from_env, Executor, LambdaClaimOutcome, LambdaExecuteOutcome,
    LiFiMetaRouter, OutcomeLog, OutcomeRecord, SkipRules,
};
use genome_client::{AcrossPoller, DeBridgePoller, GenomeClient};
use profit_calc::ProfitCalculator;
use protocol_adapters::AdapterFactory;
use solver_api::{
    AttemptData, IntentData, SolvedData, SolverApi, SolverEvent,
};
use std::collections::HashSet;
use std::sync::Arc;
use taifoon_arb_bridge::{BalanceHighHandler, StubBridge, ThresholdHandler};
use tokio::sync::{broadcast, mpsc};
use tracing::{error, info, warn};
use tracing_subscriber::Layer;
use wallet_manager::WalletManager;

/// A tracing layer that forwards formatted log lines to a broadcast channel.
struct BroadcastLogLayer {
    tx: broadcast::Sender<String>,
}

impl<S: tracing::Subscriber> Layer<S> for BroadcastLogLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        let mut msg = String::new();
        let meta = event.metadata();
        let level = meta.level().to_string();
        struct Visitor<'a>(&'a mut String);
        impl<'a> tracing::field::Visit for Visitor<'a> {
            fn record_debug(&mut self, _field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                use std::fmt::Write;
                let _ = write!(self.0, "{:?}", value);
            }
            fn record_str(&mut self, _field: &tracing::field::Field, value: &str) {
                self.0.push_str(value);
            }
        }
        event.record(&mut Visitor(&mut msg));
        let line = format!("[{}] {}", level, msg);
        let _ = self.tx.send(line);
    }
}

const DEFAULT_GENOME_SSE_URL: &str = "https://api.taifoon.dev/api/genome/subscribe/sse";
const DEFAULT_SPINNER_BASE: &str = "https://api.taifoon.dev";
const DEFAULT_MIN_PROFIT_USD: f64 = 0.10;
const SOLVER_INTEL_PATH: &str = "config/solver_intel.json";
const DEFAULT_API_PORT: u16 = 8082;

#[tokio::main]
async fn main() -> Result<()> {
    // Build a SolverApi first so we can wire its log channel into tracing
    let solver_api = SolverApi::new();
    let log_tx = solver_api.log_sender();

    use tracing_subscriber::prelude::*;
    use tracing_subscriber::EnvFilter;
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_thread_ids(false)
                .with_file(false)
                .with_filter(EnvFilter::new("info")),
        )
        .with(BroadcastLogLayer { tx: log_tx }.with_filter(EnvFilter::new("info")))
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
        .or_else(|_| std::env::var("SIMULATION_MODE"))
        .map(|v| v != "false" && v != "0")
        .unwrap_or(true);
    let outcome_db_path = std::env::var("OUTCOME_DB_PATH")
        .unwrap_or_else(|_| "/tmp/taifoon_solver_outcomes.sqlite".to_string());
    let mamba_lake_url = std::env::var("MAMBA_LAKE_URL").ok();
    let protocol_filter = std::env::var("PROTOCOL_FILTER")
        .unwrap_or_else(|_| "all".to_string())
        .to_lowercase();

    info!("🚀 Taifoon Solver Starting...");
    info!("📡 Genome SSE: {}", genome_sse_url);
    info!("🔌 Spinner API: {}", spinner_base);
    info!("🎯 Protocol filter: {}", protocol_filter);
    info!("💰 Min Profit: ${}", min_profit_usd);
    info!("🧪 DRY_RUN: {}", dry_run);
    let api_port: u16 = std::env::var("API_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_API_PORT);
    info!("💾 Outcome DB: {}", outcome_db_path);
    info!("🌐 API Port: {}", api_port);

    // ── Solver event API (SSE for dashboard) ──────────────────────────────────
    let api_router = solver_api.router();
    tokio::spawn(async move {
        let listener = match tokio::net::TcpListener::bind(format!("0.0.0.0:{}", api_port)).await {
            Ok(l) => l,
            Err(e) => {
                error!("API bind {}: {}", api_port, e);
                return;
            }
        };
        info!("✅ Solver event API on :{}", api_port);
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
    let lambda_controller: Option<Arc<executor::LambdaController>> = match build_lambda_controller_from_env(
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
            Some(Arc::new(ctrl))
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
    let adapter_factory = AdapterFactory::new(
        std::env::var("WARMBED_API_URL").unwrap_or_else(|_| "https://api.taifoon.dev".into())
    );

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

        // Emit detected only after dedup passes — lifecycle re-fires (placed/executed)
        // for the same intent must not create dangling detected records in the API.
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
        // When genome omits bridge/tool, attempt async resolution via LiFi status API.
        // The API also returns the actual deposit tx (sending.txHash / sending.chainId)
        // which we patch onto the child intent so enrichment decodes V3FundsDeposited
        // from the right tx on the right chain (not the LiFi Diamond tx).
        let effective_intent;
        let intent_ref = if is_lifi {
            let mut bridge = LiFiMetaRouter::resolve_bridge(&intent).unwrap_or_default();
            let mut api_sending_tx: Option<String> = None;
            let mut api_sending_chain: Option<u64> = None;
            // Always call li.quest when deposit_id is absent: the intent.tx_hash for LiFi
            // events is the LiFi Diamond tx, NOT the underlying deposit tx. We must fetch
            // sending.txHash + sending.chainId so lambda_controller can decode V3FundsDeposited.
            // Even when genome already provides bridge/tool, the Diamond tx still needs replacing.
            let need_deposit_tx = intent.deposit_id.is_none();
            if bridge.is_empty() || need_deposit_tx {
                // Attempt to resolve bridge + deposit tx from LiFi status API.
                // Intent IDs look like: lifi_v2::lifi_0x<txhash> or lifi_v2:0x<txhash>
                let tx_hash_from_id = if intent.id.contains("lifi_0x") {
                    intent.id.split("lifi_0x").nth(1).map(|s| format!("0x{}", s))
                } else {
                    None
                };
                let lookup_hash = if intent.tx_hash.starts_with("0x") && intent.tx_hash.len() == 66 {
                    Some(intent.tx_hash.clone())
                } else {
                    tx_hash_from_id
                };
                if let Some(ref hash) = lookup_hash {
                    match resolve_lifi_bridge(hash).await {
                        Some(res) => {
                            info!("🔍 LiFi bridge resolved via API: {} → {} (deposit_tx={:?} src_chain={:?})",
                                hash, res.bridge, res.sending_tx_hash, res.sending_chain_id);
                            if bridge.is_empty() { bridge = res.bridge; }
                            api_sending_tx = res.sending_tx_hash;
                            api_sending_chain = res.sending_chain_id;
                        }
                        None => {
                            if bridge.is_empty() {
                                info!("⏭️  lifi skip (bridge not routable): {}", intent.id);
                            }
                        }
                    }
                } else if bridge.is_empty() {
                    info!("⏭️  lifi skip (no tx_hash for bridge lookup): {}", intent.id);
                }
            }
            if bridge.is_empty() {
                // Bridge not routable — don't fall to legacy executor
                continue;
            } else {
                let mut child = LiFiMetaRouter::project_to_child(&intent, &bridge);
                // Patch the child intent with the actual deposit tx from the LiFi API.
                // The genome event carries the LiFi Diamond tx; enrichment needs the
                // underlying Across/deBridge deposit tx to decode relay parameters.
                if let Some(stx) = api_sending_tx {
                    child.tx_hash = stx;
                }
                if let Some(sc) = api_sending_chain {
                    child.src_chain = sc;
                }
                info!("🔀 LiFi→{} projection: {} intent id={} src_chain={} tx={}",
                    bridge, intent.id, child.id, child.src_chain, &child.tx_hash[..child.tx_hash.len().min(18)]);
                effective_intent = child;
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
            // Spawn each intent execution concurrently so the main loop stays free
            // to receive new intents (critical for short-lived Mayan Swift orders).
            let ctrl = Arc::clone(ctrl);
            let api = solver_api.clone();
            let intent_owned = intent_ref.to_owned();
            let intent_id = intent.id.clone();
            let is_debridge_spawn = effective_is_debridge;
            let is_mayan_spawn = effective_is_mayan;
            let is_lifi_spawn = is_lifi;
            tokio::spawn(async move {
                match ctrl.lambda_execute(&intent_owned).await {
                    Ok(LambdaExecuteOutcome::Confirmed { tx_hash, gas_used }) => {
                        let proto_tag = if is_debridge_spawn { "deBridge" } else if is_mayan_spawn { "Mayan" } else if is_lifi_spawn { "LiFi" } else { "Across" };
                        info!("🎉 {} fill confirmed: {}", proto_tag, tx_hash);
                        api.emit_event(SolverEvent::IntentSolved(SolvedData {
                            id: intent_id.clone(),
                            tx_hash,
                            actual_profit_usd: 0.0,
                            gas_used,
                        }));
                        if is_debridge_spawn {
                            match ctrl.lambda_claim_debridge(&intent_owned).await {
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
                        info!("⏭️  Skipped {}: {}", intent_owned.id, reason);
                        let is_dry_run_skip = reason == "dry_run";
                        api.emit_event(SolverEvent::IntentAttempted(AttemptData {
                            id: intent_id,
                            profitable: is_dry_run_skip,
                            profit_usd: 0.0,
                            protocol_fee_usd: 0.0,
                            gas_cost_usd: 0.0,
                            decision: if is_dry_run_skip { "dry_run".into() } else { "skip".into() },
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
            });
            continue;
        }

        // Legacy path — profit-calc for all, execute only for protocols with adapters.
        // Orbiter/Socket/t3rn etc. are tracked for observability but not yet executable.
        let has_adapter = adapter_factory.get_adapter(&intent).is_ok();
        match profit_calc.calculate(&intent).await {
            Ok(p) => {
                solver_api.emit_event(SolverEvent::IntentAttempted(AttemptData {
                    id: intent.id.clone(),
                    profitable: p.profitable,
                    profit_usd: p.net_profit_usd,
                    protocol_fee_usd: p.breakdown.protocol_fee_usd,
                    gas_cost_usd: p.breakdown.gas_cost_usd,
                    decision: if p.profitable && has_adapter { "execute".into() } else if p.profitable { "no_adapter".into() } else { "skip".into() },
                }));
                if p.profitable && has_adapter {
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
                } else if p.profitable && !has_adapter {
                    info!("⏭️  {} profitable (${:.4}) but no adapter yet — skipping execute",
                        intent.protocol, p.net_profit_usd);
                }
            }
            Err(e) => error!("❌ profit calc: {}", e),
        }
    }

    Ok(())
}

/// Rich resolution result from the LiFi status API.
/// Carries the bridge slug plus the actual source-side deposit tx details so the
/// enrichment path in lambda_controller can decode V3FundsDeposited correctly.
struct LifiResolution {
    bridge: String,
    /// txHash of the actual underlying deposit (e.g. V3FundsDeposited tx), NOT the LiFi Diamond tx.
    sending_tx_hash: Option<String>,
    /// Chain id where the deposit tx was emitted.
    sending_chain_id: Option<u64>,
}

/// Resolve the underlying bridge for a LiFi intent via the LiFi status API.
/// Returns a `LifiResolution` with bridge slug + deposit tx details, or None
/// if the underlying bridge is not something we can fill.
async fn resolve_lifi_bridge(tx_hash: &str) -> Option<LifiResolution> {
    let url = format!("https://li.quest/v1/status?txHash={}", tx_hash);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .ok()?;
    let resp = client.get(&url).send().await.ok()?;
    if !resp.status().is_success() { return None; }
    let body: serde_json::Value = resp.json().await.ok()?;
    let raw = body.get("tool")
        .or_else(|| body.get("bridge"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase())?;
    let bridge = match raw.as_str() {
        "across" | "across_v3" => "across".to_string(),
        "debridge" | "dln" | "debridge_dln" => "debridge".to_string(),
        "mayan" | "mayan_swift" | "mayanswift" => "mayan".to_string(),
        _ => {
            tracing::debug!("LiFi bridge '{}' not routable (not Across/deBridge/Mayan)", raw);
            return None;
        }
    };
    // Extract the actual deposit-side tx so lambda_controller can decode it
    // directly (e.g. V3FundsDeposited log for Across) without relying on the
    // LiFi Diamond tx, which may be on an unwired or archive-pruned chain.
    let sending = body.get("sending");
    let sending_tx_hash = sending
        .and_then(|s| s.get("txHash"))
        .and_then(|v| v.as_str())
        .filter(|s| s.starts_with("0x") && s.len() == 66)
        .map(String::from);
    let sending_chain_id = sending
        .and_then(|s| s.get("chainId"))
        .and_then(|v| v.as_u64());
    Some(LifiResolution { bridge, sending_tx_hash, sending_chain_id })
}

