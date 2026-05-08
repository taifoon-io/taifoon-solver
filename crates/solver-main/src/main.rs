use anyhow::{anyhow, Result};
use chrono::Utc;
use executor::{
    build_lambda_controller_from_env, Executor, LambdaClaimOutcome, LambdaExecuteOutcome,
    LiFiMetaRouter, OutcomeLog, OutcomeRecord, SkipRules,
};
use genome_client::{fetch_mayan_order_params, AcrossPoller, DeBridgePoller, GenomeClient, Intent};
use portfolio_sidecar::PortfolioSidecar;
use profit_calc::ProfitCalculator;
use protocol_adapters::AdapterFactory;
use solver_api::{
    AttemptData, IntentData, SolvedData, SolverApi, SolverEvent,
};
use solver_main::lifi_resolver::{resolve_lifi_bridge, LifiBridgeResult};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use taifoon_arb_bridge::{BalanceHighHandler, StubBridge, ThresholdHandler};
use tokio::sync::{broadcast, mpsc, Semaphore};
use tracing::{error, info, warn};
use tracing_subscriber::Layer;
use wallet_manager::WalletManager;
use t3rn_sidecar::{delivery_router, DeliveryMatrix, T3RNSidecar};

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
    // Build a SolverApi first so we can wire its log channel into tracing.
    // We delay the .with_outcome_log() call until after OUTCOME_DB_PATH is
    // resolved, but BEFORE we capture log_sender — see further down.
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

    // DST_CHAIN_FILTER: comma-separated chain IDs we will fill on (e.g. "8453,42161").
    // Empty / unset = accept all wired chains.
    let dst_chain_filter: Vec<u64> = std::env::var("DST_CHAIN_FILTER")
        .unwrap_or_default()
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();

    // MAX_INPUT_USD: drop intents whose input amount exceeds this (portfolio capacity cap).
    // Defaults to MAX_NOTIONAL_USD which is checked again at execution, but pre-filtering
    // here avoids wasting a li.quest API call and enrichment RPC on unfillable orders.
    let max_notional_usd_global: f64 = std::env::var("MAX_NOTIONAL_USD")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200.0);
    let max_input_usd: f64 = std::env::var("MAX_INPUT_USD")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(max_notional_usd_global);
    let min_input_usd: f64 = std::env::var("MIN_INPUT_USD")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);

    info!("🚀 Taifoon Solver Starting...");
    info!("📡 Genome SSE: {}", genome_sse_url);
    info!("🔌 Spinner API: {}", spinner_base);
    info!("🎯 Protocol filter: {}", protocol_filter);
    info!("💰 Min Profit: ${}", min_profit_usd);
    if !dst_chain_filter.is_empty() {
        info!("⛓️  DST_CHAIN_FILTER: {:?}", dst_chain_filter);
    }
    if min_input_usd > 0.0 || max_input_usd < max_notional_usd_global {
        info!("💵 Input range: ${:.2}–${:.2}", min_input_usd, max_input_usd);
    }
    info!("🧪 DRY_RUN: {}", dry_run);
    let api_port: u16 = std::env::var("API_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_API_PORT);
    info!("💾 Outcome DB: {}", outcome_db_path);
    info!("🌐 API Port: {}", api_port);

    // ── Solver API auth token (issue #8) ──────────────────────────────────────
    // SOLVER_API_TOKEN gates every /api/solver/* route. If unset at boot we
    // generate a 32-byte hex token, set it in this process's env so the auth
    // middleware can read it back, and print it once to stdout. Operators
    // running under systemd/Docker should pre-set it; the auto-generated
    // path is the dev/local fallback so a fresh checkout doesn't lock its
    // own dashboard out by default.
    ensure_solver_api_token();

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

    // ── LWC delivery worker (open-mamba webhook target) ───────────────────────
    if std::env::var("T3RN_LWC_ENABLED").map(|v| v == "true" || v == "1").unwrap_or(false) {
        let lwc_port: u16 = std::env::var("LWC_WORKER_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(8091);
        let private_key = std::env::var("SOLVER_PRIVATE_KEY").unwrap_or_default();
        let scan_interval_secs: u64 = std::env::var("LWC_SCAN_INTERVAL_SECS")
            .ok().and_then(|s| s.parse().ok()).unwrap_or(60);
        let mamba_url = std::env::var("MAMBA_LAKE_URL").ok();
        match private_key.parse::<alloy::signers::local::PrivateKeySigner>() {
            Ok(signer) => {
                let sidecar = Arc::new(T3RNSidecar::new(signer));
                let matrix  = Arc::new(DeliveryMatrix::new());
                let router  = delivery_router(sidecar.clone(), matrix.clone());

                // Parallel scan-and-report loop — posts matrix snapshot to open-mamba
                t3rn_sidecar::spawn_delivery_loop(matrix.clone(), scan_interval_secs, mamba_url);

                tokio::spawn(async move {
                    match tokio::net::TcpListener::bind(format!("0.0.0.0:{}", lwc_port)).await {
                        Ok(listener) => {
                            info!("✅ LWC delivery worker on :{}", lwc_port);
                            if let Err(e) = axum::serve(listener, router).await {
                                error!("LWC worker error: {}", e);
                            }
                        }
                        Err(e) => error!("LWC worker bind {}: {}", lwc_port, e),
                    }
                });
            }
            Err(e) => warn!("T3RN_LWC_ENABLED=true but SOLVER_PRIVATE_KEY invalid: {} — LWC worker skipped", e),
        }
    }

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

    // Inject a second OutcomeLog handle (separate Connection) into SolverApi so
    // the dashboard P&L endpoints can read fills from the same SQLite file.
    // The two handles are independent rusqlite Connections — SQLite's WAL mode
    // allows concurrent readers alongside the executor's write connection. The
    // OnceLock lets this run after solver_api.router() is already built.
    match OutcomeLog::open(&outcome_db_path, None) {
        Ok(l) => {
            solver_api.set_outcome_log(Arc::new(l));
            info!("📊 Dashboard P&L endpoints wired to {}", outcome_db_path);
        }
        Err(e) => {
            warn!("solver-api outcome handle failed: {} — /api/solver/{{outcomes,pnl}} will return empty", e);
        }
    }

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
    solver_api.set_wallet_manager(wallet_manager.clone());
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
            let arc = Arc::new(ctrl);
            // Register with solver-api so /api/solver/claims/:id/retry can fire
            // claimUnlock from the dashboard against the same controller the
            // background tick uses.
            solver_api.set_lambda_controller(arc.clone());
            Some(arc)
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

    // ── Lifecycle background tasks ────────────────────────────────────────────
    // These run forever alongside the fill loop in the same process:
    //   Task A: rebalancer — scans balances every SIDECAR_INTERVAL_SECS, bridges
    //           surplus to fund depleted fill chains and sweeps src-chain recoveries.
    //   Task B: claim retry — scans wallet DB every SIDECAR_INTERVAL_SECS for deBridge
    //           fills that are CONFIRMED but claimUnlock was never sent (network
    //           error, process restart, gas spike), and fires claimUnlock for each.
    let sidecar_interval_secs: u64 = std::env::var("SIDECAR_INTERVAL_SECS")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(300);

    // Task A: rebalancer
    if let Ok(sidecar) = PortfolioSidecar::from_key(
        &std::env::var("SOLVER_PRIVATE_KEY").unwrap_or_default(),
        dry_run,
    ) {
        info!("♻️  Rebalancer background task started (interval={}s dry_run={})", sidecar_interval_secs, dry_run);
        let sidecar = Arc::new(sidecar);
        sidecar.set_interval_secs(sidecar_interval_secs);
        // Register with solver-api so /api/solver/rebalance and
        // /api/solver/rebalancer/status can drive the same instance.
        solver_api.set_portfolio_sidecar(sidecar.clone());
        let sidecar_loop = sidecar.clone();
        tokio::spawn(async move {
            // Stagger first tick by 10s so fill loop logs settle first.
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            loop {
                sidecar_loop.tick().await;
                tokio::time::sleep(std::time::Duration::from_secs(sidecar_interval_secs)).await;
            }
        });
    } else {
        warn!("⚠️  Rebalancer disabled — SOLVER_PRIVATE_KEY not set");
    }

    // Task B: deBridge claim retry
    if let Some(ref ctrl) = lambda_controller {
        let ctrl_claim = Arc::clone(ctrl);
        let wallet_db_claim = wallet_db_path.clone();
        let claim_interval = sidecar_interval_secs;
        let outcome_log_for_claim = rule_skip_log.clone();
        info!("🔁 deBridge claim-retry background task started (interval={}s)", claim_interval);
        tokio::spawn(async move {
            // Stagger by 30s so rebalancer runs first.
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            loop {
                debridge_claim_retry_tick(
                    &wallet_db_claim,
                    &ctrl_claim,
                    outcome_log_for_claim.as_ref(),
                    dry_run,
                ).await;
                tokio::time::sleep(std::time::Duration::from_secs(claim_interval)).await;
            }
        });
    }

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

    // ── Genome SSE consumer + Across REST + deBridge on-chain + Mayan pollers ──
    // AcrossPoller polls the Across V3 REST API; DeBridgePoller scans eth_getLogs
    // for DlnSource.OrderCreated; MayanPoller watches the Mayan order API for
    // EVM-destination Swift orders. The SSE stream emits null-field events and is
    // kept only for block/gas signals — all fillable intents come from the pollers.
    let genome_client = GenomeClient::new(&genome_sse_url);
    let (intent_tx, mut intent_rx) = mpsc::channel(100);
    let debridge_poller = DeBridgePoller::default_mainnet();
    let solver_evm_addr = lambda_controller.as_ref()
        .map(|c| format!("{:?}", c.signer.address()).to_lowercase());
    let across_poller = AcrossPoller {
        solver_address: solver_evm_addr,
        ..AcrossPoller::default_mainnet()
    };
    let _genome_handle = tokio::spawn(async move {
        if let Err(e) = genome_client
            .subscribe_with_all_pollers(intent_tx, vec![across_poller], Some(debridge_poller))
            .await
        {
            error!("Genome stream error: {}", e);
        }
    });
    info!("✅ Genome SSE + deBridge on-chain + Across + Mayan pollers started");
    info!("⏳ Waiting for intents...");

    // Dedup: track intent IDs we've already dispatched in this session.
    // The genome stream emits deposit + placed + executed for the same
    // cross-chain transfer; we only want to act on `placed` (first time
    // a deposit_id is resolvable). TTL is session-scoped — no persistence needed.
    let mut dispatched: HashSet<String> = HashSet::new();

    // LiFi retry channel: when li.quest returns Pending we spawn a task that
    // sleeps 15s and re-queues the intent here instead of relying on genome
    // to re-emit it (which may never happen or may be too slow).
    let (lifi_retry_tx, mut lifi_retry_rx) = mpsc::channel::<Intent>(64);
    // Per-intent retry counter — abandon after 5 attempts (~75s total).
    let mut lifi_retry_counts: HashMap<String, u8> = HashMap::new();

    // Concurrency cap: at most 2 fills in-flight simultaneously.
    // Each spawned fill holds a permit for its full lifecycle (execute + claim),
    // so a third intent waits until one of the two completes its claim.
    let fill_semaphore = Arc::new(Semaphore::new(2));

    // ── Main loop ─────────────────────────────────────────────────────────────
    loop {
    let intent = tokio::select! {
        biased;
        Some(i) = lifi_retry_rx.recv() => i,
        Some(i) = intent_rx.recv() => i,
        else => break,
    };
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
            intent.src_chain,
            intent.dst_chain,
            None,
            None,
            intent.fill_deadline,
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
                    solver_id: None,
                    claim_tx_hash: None,
                    claim_fee_usd: None,
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

        // ── Portfolio-capacity pre-filters ───────────────────────────────────────
        // Drop intents outside the configured chain set or amount range *before*
        // the li.quest API call and enrichment RPC, so we waste no resources on
        // orders the wallet provably cannot fill.
        if !dst_chain_filter.is_empty() && !dst_chain_filter.contains(&intent.dst_chain) {
            info!("⏭️  chain_filter skip (dst={} not in {:?}): {}", intent.dst_chain, dst_chain_filter, intent.id);
            continue;
        }
        // intent.amount is the raw token amount (wei / micro-USDC), but Mayan
        // reports amounts as decimal ETH floats (e.g. "0.0011"). We do a rough
        // USD estimate using the token address and decimal heuristic — good enough
        // for coarse capacity gating.
        let approx_usd = {
            let s = intent.amount.trim_start_matches("0x");
            let t = intent.src_token.to_lowercase();
            let is_stable = t.contains("usdc") || t.contains("usdt")
                || t == "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913"
                || t == "0x0b2c639c533813f4aa9d7837caf62653d097ff85"
                || t == "0xaf88d065e77c8cc2239327c5edb3a432268e5831"
                || t == "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
                || t == "0xdac17f958d2ee523a2206206994597c13d831ec7"
                || t == "0xfd086bc7cd5c481dcc9c85ebe478a1c0b69fcbb9"
                || t == "0x94b008aa00579c1307b0ef2c499ad98a8ce58e58"
                || t == "0x2791bca1f2de4661ed88a30c99a7a9449aa84174"
                || t == "0x3c499c542cef5e3811e1192ce70d8cc03d5c3359"
                || t == "0xc2132d05d31c914a87c6611c10748aeb04b58e8f";
            // Mayan-style float (decimal point) — already human-readable token amount
            if s.contains('.') {
                let amt: f64 = s.parse().unwrap_or(0.0);
                if is_stable { amt } else { amt * 3700.0 }
            } else {
                let raw: u128 = s.parse::<u128>()
                    .or_else(|_| u128::from_str_radix(s, 16))
                    .unwrap_or(0);
                // Magnitude fallback: 6-dec range (100k–100M = $0.10–$100 as micro-USDC)
                let is_6dec = is_stable || (raw >= 100_000 && raw <= 100_000_000);
                if is_6dec {
                    raw as f64 / 1_000_000.0
                } else {
                    (raw as f64 / 1e18) * 3700.0
                }
            }
        };
        if approx_usd > max_input_usd && max_input_usd > 0.0 {
            info!("⏭️  amount_cap skip (≈${:.2}>max=${:.2}): {}", approx_usd, max_input_usd, intent.id);
            continue;
        }
        if approx_usd < min_input_usd {
            info!("⏭️  amount_floor skip (≈${:.2}<min=${:.2}): {}", approx_usd, min_input_usd, intent.id);
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
                        LifiBridgeResult::Resolved(res) => {
                            info!("🔍 LiFi bridge resolved via API: {} → {} (deposit_tx={:?} src_chain={:?})",
                                hash, res.bridge, res.sending_tx_hash, res.sending_chain_id);
                            if bridge.is_empty() { bridge = res.bridge; }
                            api_sending_tx = res.sending_tx_hash;
                            api_sending_chain = res.sending_chain_id;
                        }
                        LifiBridgeResult::NotRoutable => {
                            info!("⏭️  lifi skip (bridge not routable, permanent): {}", intent.id);
                            // Keep in dispatched — this intent will never be routable.
                        }
                        LifiBridgeResult::Pending => {
                            if bridge.is_empty() {
                                // li.quest hasn't indexed the tx yet. Keep in `dispatched` so
                                // duplicate genome events don't cause a double attempt, but spawn
                                // a background retry after 15 s instead of relying on genome re-emit.
                                let retry_count = lifi_retry_counts.entry(intent.id.clone()).or_insert(0);
                                if *retry_count < 8 {
                                    *retry_count += 1;
                                    let attempt = *retry_count;
                                    // Back-off: 15s for attempts 1-3, 30s for attempts 4-8.
                                    // li.quest can take >75s to index some Across txs.
                                    // Total window: 3×15 + 5×30 = 195s (~3.5 min).
                                    let delay_secs = if attempt <= 3 { 15 } else { 30 };
                                    let retry_intent = intent.clone();
                                    let retry_tx = lifi_retry_tx.clone();
                                    // Remove from dispatched so the retry attempt can re-enter the main loop.
                                    dispatched.remove(&dedup_key);
                                    // Issue #16: only log first attempt — avoids ≤8 records per intent.
                                    if attempt == 1 {
                                        log_enrichment_failure(
                                            rule_skip_log.as_ref(),
                                            &intent,
                                            "lifi_bridge_pending",
                                            None,
                                        );
                                    }
                                    tokio::spawn(async move {
                                        tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
                                        info!("🔄 LiFi retry #{} for {} (li.quest was pending, delay={}s)", attempt, retry_intent.id, delay_secs);
                                        let _ = retry_tx.send(retry_intent).await;
                                    });
                                } else {
                                    info!("⏭️  lifi give-up after 8 retries (li.quest still pending): {}", intent.id);
                                    log_enrichment_failure(
                                        rule_skip_log.as_ref(),
                                        &intent,
                                        "lifi_api_unavailable",
                                        Some("li.quest still pending after 8 retries (~3.5min)".into()),
                                    );
                                    lifi_retry_counts.remove(&intent.id);
                                }
                                continue;
                            }
                        }
                    }
                } else if bridge.is_empty() {
                    info!("⏭️  lifi skip (no tx_hash for bridge lookup): {}", intent.id);
                }
            }
            if bridge.is_empty() {
                // bridge wasn't resolved — already handled above (retry or permanent skip).
                continue;
            } else {
                let mut child = LiFiMetaRouter::project_to_child(&intent, &bridge);
                // Patch the child intent with the actual deposit tx from the LiFi API.
                // The genome event carries the LiFi Diamond tx; enrichment needs the
                // underlying Across/deBridge deposit tx to decode relay parameters.
                if let Some(ref stx) = api_sending_tx {
                    child.tx_hash = stx.clone();
                } else if child.deposit_id.is_none() {
                    // li.quest resolved the bridge but hasn't returned sending.txHash yet.
                    // Retry after 15 s rather than waiting for genome re-emit.
                    let retry_count = lifi_retry_counts.entry(intent.id.clone()).or_insert(0);
                    if *retry_count < 8 {
                        *retry_count += 1;
                        let attempt = *retry_count;
                        let delay_secs = if attempt <= 3 { 15u64 } else { 30 };
                        let retry_intent = intent.clone();
                        let retry_tx = lifi_retry_tx.clone();
                        dispatched.remove(&dedup_key);
                        tokio::spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
                            info!("🔄 LiFi retry #{} for {} (sending_tx pending, delay={}s)", attempt, retry_intent.id, delay_secs);
                            let _ = retry_tx.send(retry_intent).await;
                        });
                    } else {
                        info!("⏭️  lifi give-up after 8 retries (sending_tx still pending): {}", intent.id);
                        lifi_retry_counts.remove(&intent.id);
                    }
                    continue;
                }
                if let Some(sc) = api_sending_chain {
                    child.src_chain = sc;
                }
                // For LiFi→Mayan: query Mayan explorer to populate mayan_order_id and
                // OrderParams fields (auction_mode, random, etc.) needed to build calldata.
                if bridge == "mayan" || bridge == "mayan_swift" {
                    let lookup_tx = api_sending_tx.as_deref().unwrap_or(&child.tx_hash);
                    if lookup_tx.starts_with("0x") && lookup_tx.len() == 66 {
                        if let Some(mayan_intent) = resolve_lifi_mayan_order(lookup_tx).await {
                            child.mayan_order_id = mayan_intent.mayan_order_id;
                            child.mayan_auction_mode = mayan_intent.mayan_auction_mode;
                            child.mayan_random = mayan_intent.mayan_random;
                            child.mayan_cancel_fee = mayan_intent.mayan_cancel_fee;
                            child.mayan_refund_fee = mayan_intent.mayan_refund_fee;
                            child.mayan_gas_drop = mayan_intent.mayan_gas_drop;
                            child.mayan_referrer_addr = mayan_intent.mayan_referrer_addr;
                            child.mayan_referrer_bps = mayan_intent.mayan_referrer_bps;
                            child.deadline = mayan_intent.deadline;
                            if child.output_amount.is_none() {
                                child.output_amount = mayan_intent.output_amount;
                            }
                            if child.trader.is_none() {
                                child.trader = mayan_intent.trader;
                            }
                            info!("🔍 LiFi→Mayan enriched: order_id={:?} mode={:?}",
                                child.mayan_order_id.as_deref().map(|s| &s[..s.len().min(20)]),
                                child.mayan_auction_mode);
                        } else {
                            info!("⚠️  LiFi→Mayan: could not resolve Mayan order for tx {} — skip", &lookup_tx[..lookup_tx.len().min(18)]);
                            dispatched.remove(&dedup_key);
                            continue;
                        }
                    }
                }
                info!("🔀 LiFi→{} projection: {} intent id={} src_chain={} tx={}",
                    bridge, intent.id, child.id, child.src_chain, &child.tx_hash[..child.tx_hash.len().min(18)]);
                lifi_retry_counts.remove(&intent.id);
                // Guard against double-fill: if the child has a deposit_id (e.g. Across deposit
                // resolved by li.quest), insert its canonical dedup key so that a genome re-emit
                // of the same underlying deposit (carrying deposit_id) is deduplicated.
                if let Some(dep_id) = child.deposit_id {
                    let child_key = format!("{}:dep:{}", child.protocol, dep_id);
                    dispatched.insert(child_key);
                }
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
            // A semaphore limits to MAX 2 fills in-flight; the permit is held for the
            // entire fill+claim lifecycle so a third fill only starts after one is fully claimed.
            let ctrl = Arc::clone(ctrl);
            let api = solver_api.clone();
            let intent_owned = intent_ref.to_owned();
            let intent_id = intent.id.clone();
            let is_debridge_spawn = effective_is_debridge;
            let is_mayan_spawn = effective_is_mayan;
            let is_lifi_spawn = is_lifi;
            let sem = Arc::clone(&fill_semaphore);
            // Issue #16: enrichment-failure log handle for the spawned task.
            let enrichment_log = rule_skip_log.clone();
            tokio::spawn(async move {
                // Acquire permit — blocks until a slot is free (max 2 concurrent fills).
                let _permit = match sem.acquire().await {
                    Ok(p) => p,
                    Err(_) => return, // semaphore closed (shutdown)
                };
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
                                    // Issue #10: write claim_tx_hash + claim_fee_usd back onto
                                    // the executed-fill row so the dashboard surfaces claim
                                    // status and the retry loop can skip this intent.
                                    if let Some(log) = enrichment_log.as_ref() {
                                        let log = log.clone();
                                        let intent_id_for_claim = intent_owned.id.clone();
                                        let claim_tx_for_log = claim_tx.clone();
                                        tokio::spawn(async move {
                                            if let Err(e) = log.update_claim(
                                                &intent_id_for_claim,
                                                &claim_tx_for_log,
                                                fee_usd,
                                            ) {
                                                warn!("outcome update_claim: {}", e);
                                            }
                                        });
                                    }
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
                        // Issue #16: persist enrichment failures with structured skip_reason.
                        // `stage` values from lambda_controller that map to enrichment outages:
                        //   "spinner_test_run" — POST /api/solver/test-run non-2xx / unreachable
                        //   "proof_fetch"      — Across v5 proof bundle fetch (RPC-backed)
                        //   "calldata_build"   — deBridge / Mayan adapter build incl. estimateGas
                        // Anything else (broadcast, receipt) is post-enrichment, skip those.
                        let skip_reason = match stage {
                            "spinner_test_run" => Some("spinner_timeout"),
                            "proof_fetch" | "calldata_build" => Some("gas_rpc_error"),
                            _ => None,
                        };
                        if let Some(reason) = skip_reason {
                            log_enrichment_failure(
                                enrichment_log.as_ref(),
                                &intent_owned,
                                reason,
                                Some(e),
                            );
                        }
                    }
                    Err(e) => error!("❌ lambda_execute fatal: {}", e),
                }
            });
            continue;
        }

        // Legacy path — only reached by protocols that cleared filter_match but aren't
        // routable (e.g. Orbiter/Socket/t3rn). Skip entirely if protocol filter excludes them.
        if !filter_match {
            continue;
        }
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

/// Non-blocking enrichment-failure logger. Issue #16: when a pre-execute
/// enrichment step (li.quest status, spinner /api/solver/test-run, gas-RPC)
/// fails, we want a structured row in `solver_outcomes` so the dashboard /
/// nemotron analyzer can surface enrichment outages without trawling stderr.
/// The hot path must NOT pay for the SQLite write — clone the log handle
/// (Arc-backed, micro-cheap) and `tokio::spawn` the actual `append`.
fn log_enrichment_failure(
    log: Option<&OutcomeLog>,
    intent: &Intent,
    skip_reason: &'static str,
    error: Option<String>,
) {
    let Some(log) = log else { return };
    let log = log.clone();
    let rec = OutcomeRecord {
        ts: Utc::now(),
        intent_id: intent.id.clone(),
        protocol: intent.protocol.clone(),
        src_chain: intent.src_chain,
        dst_chain: intent.dst_chain,
        decision: "enrichment_failed".into(),
        tx_hash: None,
        predicted_gas: None,
        gas_used: None,
        effective_gas_price_wei: None,
        predicted_profit_usd: None,
        actual_profit_usd: None,
        skip_reason: Some(skip_reason.into()),
        error,
        solver_id: None,
        claim_tx_hash: None,
        claim_fee_usd: None,
    };
    tokio::spawn(async move {
        if let Err(e) = log.append(rec) {
            warn!("enrichment_failed log append: {}", e);
        }
    });
}

/// Scan the wallet DB for deBridge fills in CONFIRMED state (fill tx landed but
/// claimUnlock was never sent or failed) and fire claimUnlock for each.
/// Runs every SIDECAR_INTERVAL_SECS as a background task inside solver-main.
///
/// Issue #10: when an `OutcomeLog` is configured, we cross-check the wallet's
/// CONFIRMED list against `solver_outcomes.claim_tx_hash IS NULL` so already-
/// claimed fills are skipped on subsequent ticks. After a successful
/// `claimUnlock`, we write the claim tx + fee back onto the executed-fill row.
async fn debridge_claim_retry_tick(
    wallet_db_path: &str,
    ctrl: &executor::LambdaController,
    outcome_log: Option<&OutcomeLog>,
    dry_run: bool,
) {
    let wallet = match wallet_manager::WalletManager::open(wallet_db_path, 0.0) {
        Ok(w) => w,
        Err(e) => { warn!("claim_retry: wallet DB open failed: {}", e); return; }
    };
    let confirmed = match wallet.list_intents(Some("CONFIRMED"), 200) {
        Ok(v) => v,
        Err(e) => { warn!("claim_retry: list_intents failed: {}", e); return; }
    };
    let mut pending: Vec<_> = confirmed.iter().filter(|r| {
        let p = r.protocol.to_lowercase();
        p.contains("debridge") || p.contains("dln")
    }).collect();

    // Issue #10: filter out intents whose outcome row already has a
    // claim_tx_hash. Skip-set is empty (no rows match) when the outcome
    // log isn't configured or the query fails — fall through to the
    // wallet-only behaviour rather than blocking the loop.
    if let Some(log) = outcome_log {
        match log.unclaimed_debridge_intents(1000) {
            Ok(unclaimed) => {
                use std::collections::HashSet;
                let unclaimed_set: HashSet<&str> =
                    unclaimed.iter().map(|s| s.as_str()).collect();
                let before = pending.len();
                pending.retain(|r| unclaimed_set.contains(r.intent_id.as_str()));
                let skipped = before - pending.len();
                if skipped > 0 {
                    info!(
                        "claim_retry: skipped {} already-claimed fill(s) via outcome log",
                        skipped
                    );
                }
            }
            Err(e) => warn!("claim_retry: unclaimed_debridge_intents failed: {} — falling back to wallet-only filter", e),
        }
    }

    if pending.is_empty() { return; }
    info!("claim_retry: {} CONFIRMED deBridge fill(s) need claimUnlock", pending.len());

    for record in pending {
        // DeBridgePoller sets intent.id = "debridge_dln:0x<orderId>"
        // Extract the hex part after the colon for order_id.
        let order_id_hex = if record.intent_id.contains(':') {
            record.intent_id.splitn(2, ':').nth(1).unwrap_or(&record.intent_id).to_string()
        } else {
            record.intent_id.clone()
        };
        let intent = Intent {
            id: record.intent_id.clone(),
            protocol: record.protocol.clone(),
            src_chain: record.src_chain as u64,
            dst_chain: record.dst_chain as u64,
            order_id: Some(order_id_hex.clone()),
            ..Intent::default()
        };
        if dry_run {
            info!("claim_retry: [DRY_RUN] would claimUnlock orderId={} src={}", order_id_hex, record.src_chain);
            continue;
        }
        match ctrl.lambda_claim_debridge(&intent).await {
            Ok(LambdaClaimOutcome::Claimed { tx_hash, fee_usd }) => {
                info!("claim_retry: ✅ claimUnlock tx={} fee=${:.4} ({})", tx_hash, fee_usd, record.intent_id);
                // Issue #10: persist claim outcome onto the executed-fill row.
                if let Some(log) = outcome_log {
                    if let Err(e) = log.update_claim(&record.intent_id, &tx_hash, fee_usd) {
                        warn!("claim_retry: outcome update_claim failed for {}: {}", record.intent_id, e);
                    }
                }
            }
            Ok(LambdaClaimOutcome::NotEligible { reason }) => {
                info!("claim_retry: not eligible {} — {}", record.intent_id, reason);
            }
            Ok(LambdaClaimOutcome::Failed { error: e }) => {
                warn!("claim_retry: ❌ claimUnlock failed for {} — {}", record.intent_id, e);
            }
            Err(e) => {
                warn!("claim_retry: fatal for {} — {}", record.intent_id, e);
            }
        }
    }
}

// LiFi status-API resolver moved to `solver_main::lifi_resolver` so the
// response-parser is unit-testable without spinning up a network mock. Call
// site is unchanged: `resolve_lifi_bridge(hash).await` above.

/// Query the Mayan explorer API for an order whose `sourceTxHash` matches `tx_hash`.
/// Returns a partial Intent with Mayan-specific fields populated, or `None` if the
/// order isn't found or is not in ORDER_CREATED state.
async fn resolve_lifi_mayan_order(tx_hash: &str) -> Option<Intent> {
    let url = format!(
        "https://explorer-api.mayan.finance/v3/swaps?sourceTxHash={}&service=SWIFT_V2",
        tx_hash
    );
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .user_agent("taifoon-solver/1.0")
        .build()
        .ok()?;
    let body: serde_json::Value = client.get(&url).send().await.ok()?.json().await.ok()?;
    let orders = body.get("data")?.as_array()?;
    for order in orders {
        let status = order.get("status").and_then(|v| v.as_str()).unwrap_or("");
        if status != "ORDER_CREATED" {
            continue;
        }
        let order_hash = order.get("orderHash").and_then(|v| v.as_str())?;
        let auction_mode = order.get("auctionMode").and_then(|v| v.as_u64()).map(|v| v as u8);
        let trader = order.get("trader").and_then(|v| v.as_str()).map(String::from);
        let to_amount = order.get("toAmount").and_then(|v| v.as_str()).map(String::from);
        let src_chain_mayan = order.get("sourceChain").and_then(|v| v.as_str()).unwrap_or("0");
        let src_chain = match src_chain_mayan {
            "2" => 1u64, "4" => 56, "5" => 137, "6" => 43114,
            "23" => 42161, "24" => 10, "30" => 8453, _ => 0,
        };
        // Decode the on-chain createOrderWithToken calldata so we have the full
        // OrderParams (random, fees, deadline). These must match exactly for the
        // fulfillSimple/fulfillOrder orderId hash check to pass on-chain.
        let order_params = fetch_mayan_order_params(&client, src_chain, tx_hash).await;
        return Some(Intent {
            id: format!("lifi→mayan_enriched:{}", order_hash),
            protocol: "mayan_swift".into(),
            mayan_order_id: Some(order_hash.to_string()),
            mayan_auction_mode: auction_mode.or(order_params.auction_mode),
            output_amount: to_amount,
            trader,
            src_chain,
            mayan_random: order_params.random,
            mayan_cancel_fee: order_params.cancel_fee,
            mayan_refund_fee: order_params.refund_fee,
            mayan_gas_drop: order_params.gas_drop,
            mayan_referrer_addr: order_params.referrer_addr,
            mayan_referrer_bps: order_params.referrer_bps,
            deadline: order_params.deadline,
            ..Default::default()
        });
    }
    None
}

// ─── Issue #8: Solver API bearer-token bootstrap ─────────────────────────────

/// Resolve `SOLVER_API_TOKEN` for the current process. If the env var is set
/// to a non-empty value, log a redacted notice and return. Otherwise generate
/// a 32-byte hex token, set it in the process env (so the auth middleware
/// reads it back), and print it once to stdout with a clearly-labelled line.
fn ensure_solver_api_token() {
    if let Ok(t) = std::env::var("SOLVER_API_TOKEN") {
        if !t.trim().is_empty() {
            info!(
                "🔐 SOLVER_API_TOKEN: configured from env (len={})",
                t.trim().len()
            );
            return;
        }
    }

    let token = generate_hex_token_32();
    // SAFETY: env mutation happens before we spawn the API task, so no
    // concurrent reads race with this write. The auth middleware reads via
    // std::env::var, which performs its own locking.
    std::env::set_var("SOLVER_API_TOKEN", &token);

    // Stdout, not tracing, so the line survives even if RUST_LOG is wonky.
    // Format matches the brief: `SOLVER API TOKEN: <token>`.
    println!(
        "\n────────────────────────────────────────────────────────────────────\n\
         SOLVER API TOKEN: {token}\n\
         (auto-generated; set SOLVER_API_TOKEN to override before next start)\n\
         ────────────────────────────────────────────────────────────────────\n"
    );
    info!("🔐 SOLVER_API_TOKEN: auto-generated (printed to stdout once)");
}

/// Read 32 random bytes from `/dev/urandom` and return them hex-encoded.
/// `/dev/urandom` is the right primitive on macOS and Linux — both deliver
/// cryptographically secure output. On read failure (extremely unlikely),
/// fall back to a time-derived token *and log a warning* so an operator
/// notices the degraded state.
fn generate_hex_token_32() -> String {
    use std::io::Read;

    fn try_urandom() -> std::io::Result<[u8; 32]> {
        let mut buf = [0u8; 32];
        std::fs::File::open("/dev/urandom")?.read_exact(&mut buf)?;
        Ok(buf)
    }

    match try_urandom() {
        Ok(bytes) => bytes
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>(),
        Err(e) => {
            warn!(
                "/dev/urandom read failed ({}) — falling back to time-derived token. \
                 Set SOLVER_API_TOKEN explicitly in production.",
                e
            );
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let pid = std::process::id();
            // Mix nanos + pid to at least vary the fallback across restarts.
            // Not crypto-grade — that's why we logged a warning above.
            format!("{:032x}{:032x}", now, pid as u128)
        }
    }
}

