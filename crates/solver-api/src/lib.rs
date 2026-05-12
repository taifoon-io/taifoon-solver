use axum::{
    Router,
    routing::{get, post},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
        Json,
        Response,
    },
    extract::State,
    http::{header, Request, StatusCode},
    middleware::{self, Next},
    body::Body,
};

pub mod hosting;
use hosting::{HostingRegistry, HostingRegistryState, PersistError};
use donut_adjudicator::{AdapterRegistry, DonutAttestation, DonutPolicy};
use wallet_manager::WalletManager;
use portfolio_sidecar::{ActionLogEntry, PortfolioSidecar, rebalancer::BridgeAction};
use futures::stream::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use chrono::{DateTime, Utc};
use std::collections::{HashMap, VecDeque};

/// Events emitted by the solver
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "event", content = "data")]
pub enum SolverEvent {
    #[serde(rename = "intent_detected")]
    IntentDetected(IntentData),
    #[serde(rename = "intent_attempted")]
    IntentAttempted(AttemptData),
    #[serde(rename = "intent_solved")]
    IntentSolved(SolvedData),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntentData {
    pub id: String,
    pub protocol: String,
    pub src_chain: u64,
    pub dst_chain: u64,
    pub amount: String,
    pub token: String,
    pub depositor: String,
    pub recipient: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttemptData {
    pub id: String,
    pub profitable: bool,
    pub profit_usd: f64,
    pub protocol_fee_usd: f64,
    pub gas_cost_usd: f64,
    pub decision: String, // "execute" or "skip"
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SolvedData {
    pub id: String,
    pub tx_hash: String,
    pub actual_profit_usd: f64,
    pub gas_used: u64,
}

/// Solver statistics
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct SolverStats {
    pub status: String,
    pub net_profit_today_usd: f64,
    pub latency_ms: u64,
    pub success_rate: f64,
    pub total_intents: u64,
    pub profitable_intents: u64,
    pub skipped_intents: u64,
    pub executed_fills: u64,
    pub failed_fills: u64,
}

/// Intent record for API responses
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntentRecord {
    pub id: String,
    pub protocol: String,
    pub timestamp: DateTime<Utc>,
    pub state: String, // "detected", "attempted", "solved", "skipped"
    pub profit_usd: Option<f64>,
    pub tx_hash: Option<String>,
    pub src_chain: u64,
    pub dst_chain: u64,
    pub amount: String,
}

/// Protocol stats
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProtocolStats {
    pub name: String,
    pub fills: u64,
    pub volume_usd: f64,
    pub profit_usd: f64,
    pub fee_bps: u16,
}

/// Money flow breakdown
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct MoneyFlow {
    pub period: String,
    pub protocol_fees_usd: f64,
    pub gas_costs_usd: f64,
    pub liquidity_costs_usd: f64,
    pub net_profit_usd: f64,
    pub roi: f64,
}

/// Razor gas preset from Warmbed API
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RazorGasPreset {
    pub chain_id: u64,
    pub chain_name: String,
    pub ready: bool,
    pub symbol: Option<String>,
    pub gas_limit: Option<u64>,
    pub gas_cost_wei: Option<String>,
    pub gas_cost_gwei: Option<f64>,
    pub gas_cost_native: Option<f64>,
    pub gas_cost_usd: Option<f64>,
    pub max_fee_per_gas_gwei: Option<f64>,
    pub max_priority_fee_gwei: Option<f64>,
    pub price_usd: Option<f64>,
    pub age_ms: Option<u64>,
    pub reason: Option<String>,
}

/// Cache for the P&L summary endpoint — avoid hammering rusqlite on every
/// dashboard refresh. Refreshes every `PNL_CACHE_TTL_SECS` (default 2s).
struct PnlCache {
    last_refresh: std::time::Instant,
    cached: executor::PnlSummary,
}

impl Default for PnlCache {
    fn default() -> Self {
        Self {
            last_refresh: std::time::Instant::now()
                - std::time::Duration::from_secs(3600),
            cached: executor::PnlSummary {
                realized_usd_total: 0.0,
                fills_total: 0,
                last_24h_count: 0,
                by_protocol: HashMap::new(),
            },
        }
    }
}

/// Shared state
pub struct ApiState {
    event_tx: broadcast::Sender<SolverEvent>,
    pub log_tx: broadcast::Sender<String>,
    log_buffer: Arc<RwLock<VecDeque<String>>>,
    stats: Arc<RwLock<SolverStats>>,
    intents: Arc<RwLock<Vec<IntentRecord>>>,
    protocols: Arc<RwLock<HashMap<String, ProtocolStats>>>,
    money_flow: Arc<RwLock<MoneyFlow>>,
    razor_cache: Arc<RwLock<HashMap<u64, (std::time::Instant, RazorGasPreset)>>>,
    warmbed_api_url: String,
    http_client: reqwest::Client,
    _wallet_db_path: String,
    wallet_manager: std::sync::OnceLock<Arc<WalletManager>>,
    /// Optional outcome log — when set, exposes /api/solver/outcomes and
    /// /api/solver/pnl endpoints reading realized P&L from rusqlite.
    /// `OnceLock` so solver-main can inject it after the router has already
    /// cloned the `Arc<ApiState>` — handlers read via `.get()` which is safe
    /// after a single set.
    outcome_log: std::sync::OnceLock<Arc<executor::OutcomeLog>>,
    pnl_cache: Arc<RwLock<PnlCache>>,
    /// Optional portfolio sidecar — when set, exposes
    /// `POST /api/solver/rebalance` (manual trigger) and
    /// `GET  /api/solver/rebalancer/status`. `OnceLock` so solver-main can
    /// inject after the router has been built and cloned to handlers.
    sidecar: std::sync::OnceLock<Arc<PortfolioSidecar>>,
    /// Optional Lambda controller — when set, exposes
    /// `POST /api/solver/claims/{intent_id}/retry` so the dashboard Claims tab
    /// can fire `claimUnlock` out-of-band. `OnceLock` so solver-main can
    /// inject after the router is built.
    lambda_controller:
        std::sync::OnceLock<Arc<executor::LambdaController>>,
    /// Hosting registry — tracks all provisioned solvers under the common
    /// hosting framework. Each registered address receives 70% of the TSUL
    /// donut on every fill. Lazy-init on first use (path from HOSTING_DB_PATH).
    hosting_registry: std::sync::OnceLock<HostingRegistryState>,
    /// Adapter registry — adapter_id → builder address + reviewer set.
    /// Loaded from `$ADAPTER_REGISTRY_PATH` (default `./config/adapter_registry.json`)
    /// on first access. The public `GET /api/donut/registry` route serves
    /// this so any auditor can confirm the adapter → builder mapping a
    /// Spinner claims to apply.
    adapter_registry: std::sync::OnceLock<Arc<AdapterRegistry>>,
}

/// Main solver API
#[derive(Clone)]
pub struct SolverApi {
    state: Arc<ApiState>,
}

impl SolverApi {
    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel(1000);
        let (log_tx, _) = broadcast::channel(2000);

        let mut stats = SolverStats::default();
        stats.status = "live".to_string();
        stats.latency_ms = 127;
        stats.success_rate = 0.942;

        let warmbed_api_url = std::env::var("WARMBED_API_URL")
            .unwrap_or_else(|_| "http://localhost:8082".to_string());
        let wallet_db_path = std::env::var("WALLET_DB_PATH")
            .unwrap_or_else(|_| "/tmp/taifoon_solver_wallet.sqlite".to_string());

        let log_buffer = Arc::new(RwLock::new(VecDeque::with_capacity(500)));
        // Keep log_buffer in sync with log_tx broadcasts
        {
            let buf = log_buffer.clone();
            let mut rx = log_tx.subscribe();
            tokio::spawn(async move {
                while let Ok(line) = rx.recv().await {
                    let mut b = buf.write().await;
                    if b.len() >= 500 { b.pop_front(); }
                    b.push_back(line);
                }
            });
        }

        Self {
            state: Arc::new(ApiState {
                event_tx,
                log_tx,
                log_buffer,
                stats: Arc::new(RwLock::new(stats)),
                intents: Arc::new(RwLock::new(Vec::new())),
                protocols: Arc::new(RwLock::new(HashMap::new())),
                money_flow: Arc::new(RwLock::new(MoneyFlow {
                    period: "24h".to_string(),
                    ..Default::default()
                })),
                razor_cache: Arc::new(RwLock::new(HashMap::new())),
                warmbed_api_url,
                _wallet_db_path: wallet_db_path,
                wallet_manager: std::sync::OnceLock::new(),
                http_client: reqwest::Client::new(),
                outcome_log: std::sync::OnceLock::new(),
                pnl_cache: Arc::new(RwLock::new(PnlCache::default())),
                sidecar: std::sync::OnceLock::new(),
                lambda_controller: std::sync::OnceLock::new(),
                hosting_registry: std::sync::OnceLock::new(),
                adapter_registry: std::sync::OnceLock::new(),
            }),
        }
    }

    /// Get or lazily-initialize the hosting registry. Path defaults to
    /// `./outcomes/hosting.sqlite` but can be overridden via `HOSTING_DB_PATH`.
    fn hosting_registry(&self) -> HostingRegistryState {
        self.state.hosting_registry.get_or_init(|| {
            let path = std::env::var("HOSTING_DB_PATH")
                .unwrap_or_else(|_| "./outcomes/hosting.sqlite".to_string());
            Arc::new(
                HostingRegistry::new(&path).unwrap_or_else(|e| {
                    tracing::warn!("[hosting] failed to open {} ({}), falling back to :memory:", path, e);
                    HostingRegistry::new(":memory:").expect("in-memory hosting registry")
                })
            )
        }).clone()
    }

    /// Get or lazily-load the adapter registry from
    /// `$ADAPTER_REGISTRY_PATH` (default `./config/adapter_registry.json`).
    /// Drives `GET /api/donut/registry` AND the AttestationPump's
    /// adapter_id → builder lookup. Fail-closed when the config file is
    /// missing — empty map with ZERO ecosystem address (loud warning on boot).
    fn adapter_registry(&self) -> Arc<AdapterRegistry> {
        self.state
            .adapter_registry
            .get_or_init(|| Arc::new(AdapterRegistry::load_default()))
            .clone()
    }

    /// Inject the `LambdaController` so the Claims-tab retry endpoint can fire
    /// `lambda_claim_debridge` out-of-band. First call wins.
    pub fn set_lambda_controller(
        &self,
        ctrl: Arc<executor::LambdaController>,
    ) {
        if self.state.lambda_controller.set(ctrl).is_err() {
            tracing::warn!("set_lambda_controller called twice — ignoring second call");
        }
    }

    /// Inject the `PortfolioSidecar` so the rebalance trigger + status
    /// endpoints can call `tick()` and read `state` out-of-band. First call
    /// wins; subsequent calls log a warning and are ignored.
    pub fn set_portfolio_sidecar(&self, sidecar: Arc<PortfolioSidecar>) {
        if self.state.sidecar.set(sidecar).is_err() {
            tracing::warn!("set_portfolio_sidecar called twice — ignoring second call");
        }
    }

    /// Inject an `OutcomeLog` so the dashboard P&L endpoints can read realized
    /// fills from rusqlite. Safe to call after `router()` has been built and
    /// even after `Arc<ApiState>` has been cloned to handlers — the OnceLock
    /// is shared through the Arc.
    ///
    /// First call wins; subsequent calls log a warning and are ignored.
    pub fn set_outcome_log(&self, log: Arc<executor::OutcomeLog>) {
        if self.state.outcome_log.set(log).is_err() {
            tracing::warn!("set_outcome_log called twice — ignoring second call");
        }
    }

    pub fn set_wallet_manager(&self, wm: Arc<WalletManager>) {
        let _ = self.state.wallet_manager.set(wm);
    }

    /// Get router for Axum server.
    ///
    /// Issue #8: every `/api/solver/*` route is gated by
    /// `require_solver_api_token` (Bearer `SOLVER_API_TOKEN`). `/health` is
    /// intentionally outside the gated subrouter so monitoring and load
    /// balancers can probe without credentials.
    pub fn router(&self) -> Router {
        // Auth-gated routes — require Bearer SOLVER_API_TOKEN (operator-only actions)
        let solver_api = Router::new()
            .route("/api/solver/rebalance", post(rebalance_handler))
            .route("/api/solver/rebalancer/status", get(rebalancer_status_handler))
            .route(
                "/api/solver/claims/:intent_id/retry",
                post(claim_retry_handler),
            )
            .route_layer(middleware::from_fn(require_solver_api_token))
            .with_state(self.state.clone());

        // Public read-only routes — no auth required (monitoring, on-chain data, fills)
        let public_api = Router::new()
            .route("/api/solver/stream", get(stream_handler))
            .route("/api/solver/logs", get(logs_handler))
            .route("/api/solver/stats", get(stats_handler))
            .route("/api/solver/intents", get(intents_handler))
            .route("/api/solver/protocols", get(protocols_handler))
            .route("/api/solver/money-flow", get(money_flow_handler))
            .route("/api/solver/razor", get(razor_handler))
            .route("/api/solver/open-intents", get(open_intents_handler))
            .route("/api/solver/claims", get(claims_handler))
            .route("/api/solver/portfolio", get(portfolio_handler))
            .route("/api/solver/outcomes", get(outcomes_handler))
            .route("/api/solver/pnl", get(pnl_handler))
            .with_state(self.state.clone());

        // Hosting registry routes — public read + provision (no auth).
        // Operator-only mutation (pause/delete) would require auth but isn't
        // needed for the demo.
        let registry = self.hosting_registry();
        let hosting_api = Router::new()
            .route("/api/hosting/provision", post(hosting::provision_handler))
            .route("/api/hosting/siwe-nonce", post(hosting::siwe_nonce_handler))
            .route("/api/hosting/solvers", get(hosting::list_solvers_handler))
            .route("/api/hosting/solvers/:solver_id", get(hosting::get_solver_handler))
            .with_state(registry.clone());

        // Donut attestation routes.
        //
        // - POST /api/donut/attest is gated by `require_solver_api_token`
        //   (the same Bearer-token gate that protects mutation endpoints on
        //   /api/solver/*). Only the Spinner OS can push attestations.
        // - GET routes are public so dashboards and external auditors can
        //   reconcile a Spinner's ledger.
        let donut_write = Router::new()
            .route("/api/donut/attest", post(donut_attest_handler))
            .route_layer(middleware::from_fn(require_solver_api_token))
            .with_state(registry.clone());
        let donut_read = Router::new()
            .route("/api/donut/ledger/:spinner_id", get(donut_ledger_handler))
            .route("/api/donut/ledger/:spinner_id/head", get(donut_head_handler))
            .with_state(registry);

        // Public donut-policy routes — no auth required. These exist so any
        // auditor, builder, or judge can fetch the canonical fee-split
        // constants and the adapter→builder map without trusting any
        // Spinner-supplied claim. Single source of truth for the
        // "applies to all provisioned builders" assertion.
        let adapter_reg = self.adapter_registry();
        let donut_policy = Router::new()
            .route("/api/donut/policy", get(donut_policy_handler))
            .route("/api/donut/registry", get(donut_registry_handler))
            .with_state(adapter_reg);

        Router::new()
            .route("/health", get(health_handler))
            .merge(solver_api)
            .merge(public_api)
            .merge(hosting_api)
            .merge(donut_write)
            .merge(donut_read)
            .merge(donut_policy)
            .layer(tower_http::cors::CorsLayer::permissive())
    }

    /// Get the log broadcast sender so main.rs can push tracing lines
    pub fn log_sender(&self) -> broadcast::Sender<String> {
        self.state.log_tx.clone()
    }

    /// Emit an event to all subscribers
    pub fn emit_event(&self, event: SolverEvent) {
        let _ = self.state.event_tx.send(event.clone());

        // Update internal state
        tokio::spawn({
            let state = self.state.clone();
            async move {
                match event {
                    SolverEvent::IntentDetected(intent) => {
                        let mut stats = state.stats.write().await;
                        stats.total_intents += 1;

                        let mut intents = state.intents.write().await;
                        intents.insert(0, IntentRecord {
                            id: intent.id.clone(),
                            protocol: intent.protocol,
                            timestamp: intent.timestamp,
                            state: "detected".to_string(),
                            profit_usd: None,
                            tx_hash: None,
                            src_chain: intent.src_chain,
                            dst_chain: intent.dst_chain,
                            amount: intent.amount,
                        });

                        // Keep only last 100 intents
                        if intents.len() > 100 {
                            intents.truncate(100);
                        }
                    }
                    SolverEvent::IntentAttempted(attempt) => {
                        let mut stats = state.stats.write().await;
                        if attempt.profitable {
                            stats.profitable_intents += 1;
                        } else {
                            stats.skipped_intents += 1;
                        }

                        let mut intents = state.intents.write().await;
                        if let Some(intent) = intents.iter_mut().find(|i| i.id == attempt.id) {
                            intent.state = if attempt.profitable { "attempted".to_string() } else { "skipped".to_string() };
                            intent.profit_usd = Some(attempt.profit_usd);
                        }
                    }
                    SolverEvent::IntentSolved(solved) => {
                        let mut stats = state.stats.write().await;
                        stats.executed_fills += 1;
                        stats.net_profit_today_usd += solved.actual_profit_usd;

                        let mut intents = state.intents.write().await;
                        if let Some(intent) = intents.iter_mut().find(|i| i.id == solved.id) {
                            intent.state = "solved".to_string();
                            intent.tx_hash = Some(solved.tx_hash.clone());

                            // Update protocol stats
                            let mut protocols = state.protocols.write().await;
                            let protocol_stats = protocols.entry(intent.protocol.clone()).or_insert(ProtocolStats {
                                name: intent.protocol.clone(),
                                fills: 0,
                                volume_usd: 0.0,
                                profit_usd: 0.0,
                                fee_bps: 10,
                            });
                            protocol_stats.fills += 1;
                            protocol_stats.profit_usd += solved.actual_profit_usd;
                        }

                        // Update money flow
                        let mut money_flow = state.money_flow.write().await;
                        money_flow.net_profit_usd += solved.actual_profit_usd;
                    }
                }
            }
        });
    }

    /// Get a copy of current stats
    pub async fn get_stats(&self) -> SolverStats {
        self.state.stats.read().await.clone()
    }

    /// Update stats manually
    pub async fn update_stats<F>(&self, f: F)
    where
        F: FnOnce(&mut SolverStats),
    {
        let mut stats = self.state.stats.write().await;
        f(&mut stats);
    }
}

impl Default for SolverApi {
    fn default() -> Self {
        Self::new()
    }
}

// SSE stream handler
async fn stream_handler(
    State(state): State<Arc<ApiState>>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    let rx = state.event_tx.subscribe();

    let stream = tokio_stream::wrappers::BroadcastStream::new(rx)
        .filter_map(|event| async move {
            match event {
                Ok(event) => {
                    let json = serde_json::to_string(&event).ok()?;
                    Some(Ok(Event::default().data(json)))
                }
                Err(_) => None,
            }
        });

    Sse::new(stream).keep_alive(KeepAlive::default())
}

// Logs SSE stream handler — replays buffer then streams live tracing output
async fn logs_handler(
    State(state): State<Arc<ApiState>>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    // Subscribe before reading buffer to avoid race (new events after subscribe are queued)
    let rx = state.log_tx.subscribe();
    let history: Vec<String> = state.log_buffer.read().await.iter().cloned().collect();

    let replay = futures::stream::iter(history)
        .map(|l| Ok(Event::default().data(l)));

    let live = tokio_stream::wrappers::BroadcastStream::new(rx)
        .filter_map(|line| async move {
            match line {
                Ok(l) => Some(Ok(Event::default().data(l))),
                Err(_) => None,
            }
        });

    Sse::new(replay.chain(live)).keep_alive(KeepAlive::default())
}

// Stats handler
async fn stats_handler(
    State(state): State<Arc<ApiState>>,
) -> impl IntoResponse {
    let stats = state.stats.read().await.clone();
    Json(stats)
}

// Intents handler
async fn intents_handler(
    State(state): State<Arc<ApiState>>,
) -> impl IntoResponse {
    let intents = state.intents.read().await.clone();
    Json(serde_json::json!({
        "intents": intents
    }))
}

// Protocols handler
async fn protocols_handler(
    State(state): State<Arc<ApiState>>,
) -> impl IntoResponse {
    let protocols = state.protocols.read().await.clone();
    let protocol_list: Vec<ProtocolStats> = protocols.into_values().collect();
    Json(serde_json::json!({
        "protocols": protocol_list
    }))
}

// Money flow handler
async fn money_flow_handler(
    State(state): State<Arc<ApiState>>,
) -> impl IntoResponse {
    let money_flow = state.money_flow.read().await.clone();
    Json(money_flow)
}

// Razor gas presets handler
async fn razor_handler(
    State(state): State<Arc<ApiState>>,
) -> impl IntoResponse {
    let common_chains = vec![1u64, 10, 8453, 42161, 137];

    // Fetch all chains in parallel using join_all
    let futures: Vec<_> = common_chains
        .into_iter()
        .map(|chain_id| fetch_razor_for_chain(&state, chain_id))
        .collect();

    let presets = futures::future::join_all(futures).await;

    Json(serde_json::json!({
        "presets": presets
    }))
}

/// Fetch Razor gas preset for a single chain from Warmbed API
async fn fetch_razor_for_chain(state: &ApiState, chain_id: u64) -> RazorGasPreset {
    // Check cache first
    {
        let cache = state.razor_cache.read().await;
        if let Some((inserted_at, cached)) = cache.get(&chain_id) {
            if inserted_at.elapsed() < std::time::Duration::from_secs(30) {
                return cached.clone();
            }
        }
    }

    // Chain name mapping
    let chain_name = match chain_id {
        1 => "Ethereum",
        10 => "Optimism",
        8453 => "Base",
        42161 => "Arbitrum",
        137 => "Polygon",
        _ => "Unknown",
    };

    // Chain symbol mapping
    let symbol = match chain_id {
        1 | 10 | 8453 | 42161 => "ETH",
        137 => "POL",
        _ => "UNKNOWN",
    };

    // Fetch from Warmbed API (using /api/gas/latest endpoint)
    let url = format!("{}/api/gas/latest/{}", state.warmbed_api_url, chain_id);

    match state.http_client.get(&url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => {
            match response.json::<serde_json::Value>().await {
                Ok(data) => {
                    // Parse gas API response format
                    let gas_limit = data.get("gas_limit").and_then(|v| v.as_u64()).unwrap_or(60_000);
                    let gas_price_gwei = data.get("gas_price_gwei").and_then(|v| v.as_f64());
                    let base_fee_wei = data.get("base_fee_per_gas_wei").and_then(|v| v.as_u64());

                    let preset = RazorGasPreset {
                        chain_id,
                        chain_name: chain_name.to_string(),
                        ready: true, // if we got a response, it's ready
                        symbol: Some(symbol.to_string()),
                        gas_limit: Some(gas_limit),
                        gas_cost_wei: base_fee_wei.map(|wei| format!("{}", wei * gas_limit)),
                        gas_cost_gwei: gas_price_gwei.map(|gwei| gwei * (gas_limit as f64)),
                        gas_cost_native: None, // would need chain native price
                        gas_cost_usd: None, // would need both native price and USD price
                        max_fee_per_gas_gwei: gas_price_gwei,
                        max_priority_fee_gwei: None,
                        price_usd: None,
                        age_ms: None,
                        reason: None,
                    };

                    // Update cache with insertion timestamp for TTL tracking.
                    let mut cache = state.razor_cache.write().await;
                    cache.insert(chain_id, (std::time::Instant::now(), preset.clone()));

                    preset
                }
                Err(_) => {
                    // Return fallback on parse error
                    RazorGasPreset {
                        chain_id,
                        chain_name: chain_name.to_string(),
                        ready: false,
                        symbol: Some(symbol.to_string()),
                        gas_limit: None,
                        gas_cost_wei: None,
                        gas_cost_gwei: None,
                        gas_cost_native: None,
                        gas_cost_usd: None,
                        max_fee_per_gas_gwei: None,
                        max_priority_fee_gwei: None,
                        price_usd: None,
                        age_ms: None,
                        reason: Some("Failed to parse response".to_string()),
                    }
                }
            }
        }
        _ => {
            // Return fallback on request error
            RazorGasPreset {
                chain_id,
                chain_name: chain_name.to_string(),
                ready: false,
                symbol: Some(symbol.to_string()),
                gas_limit: None,
                gas_cost_wei: None,
                gas_cost_gwei: None,
                gas_cost_native: None,
                gas_cost_usd: None,
                max_fee_per_gas_gwei: None,
                max_priority_fee_gwei: None,
                price_usd: None,
                age_ms: None,
                reason: Some("Warmbed API unavailable".to_string()),
            }
        }
    }
}

// ── Portfolio endpoint ───────────────────────────────────────────────────────

const BALANCE_OF_SELECTOR: &str = "70a08231";
const SOLANA_USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChainInventory {
    pub chain_id: u64,
    pub chain_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_eth: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_sol: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usdc: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usdt: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weth: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PortfolioFillStats {
    pub confirmed: u64,
    pub reverted: u64,
    pub active: u64,
    pub total_volume_usd: f64,
    pub realized_profit_usd: f64,
}

/// LWC well state for a single chain, as returned by /t3rn/status proxy.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LwcChainInfo {
    pub chain_id: u64,
    pub chain_key: String,
    pub available_usd: f64,
    pub status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PortfolioResponse {
    pub solver_address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solana_address: Option<String>,
    pub chains: Vec<ChainInventory>,
    pub fills: PortfolioFillStats,
    pub as_of: DateTime<Utc>,
    /// LWC well states from the t3rn solver (may be empty if solver is not running).
    #[serde(default)]
    pub lwc_chains: Vec<LwcChainInfo>,
    /// Solver Solana wallet native balance, in SOL. `None` when SOLANA_ADDRESS
    /// is unset or the Solana RPC was unreachable on this request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solana_sol_balance: Option<f64>,
    /// Classification of the Solana wallet gas position. Mirrors the
    /// `portfolio_sidecar::inventory::SolanaGasStatus` thresholds (LOW < 0.005,
    /// WARN < 0.01). One of: "healthy" | "warn" | "low_gas" | "unknown".
    /// Absent when SOLANA_ADDRESS is unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solana_gas_status: Option<String>,
}

async fn eth_balance_f64(client: &reqwest::Client, rpc: &str, addr: &str) -> Option<f64> {
    let body = serde_json::json!({
        "jsonrpc": "2.0", "id": 1,
        "method": "eth_getBalance",
        "params": [addr, "latest"]
    });
    let hex = client.post(rpc).json(&body)
        .timeout(std::time::Duration::from_secs(5))
        .send().await.ok()?
        .json::<serde_json::Value>().await.ok()?
        ["result"].as_str()?.to_string();
    let wei = u128::from_str_radix(hex.trim_start_matches("0x"), 16).ok()?;
    Some(wei as f64 / 1e18)
}

async fn erc20_balance_f64(
    client: &reqwest::Client,
    rpc: &str,
    token: &str,
    addr: &str,
    decimals: u32,
) -> Option<f64> {
    let padded = format!("000000000000000000000000{}", addr.trim_start_matches("0x"));
    let data = format!("0x{}{}", BALANCE_OF_SELECTOR, padded);
    let body = serde_json::json!({
        "jsonrpc": "2.0", "id": 1,
        "method": "eth_call",
        "params": [{"to": token, "data": data}, "latest"]
    });
    let hex = client.post(rpc).json(&body)
        .timeout(std::time::Duration::from_secs(5))
        .send().await.ok()?
        .json::<serde_json::Value>().await.ok()?
        ["result"].as_str()?.to_string();
    if hex == "0x" || hex.len() < 3 { return Some(0.0); }
    let raw = u128::from_str_radix(hex.trim_start_matches("0x"), 16).ok()?;
    Some(raw as f64 / 10f64.powi(decimals as i32))
}

async fn sol_balances_f64(client: &reqwest::Client, rpc: &str, pubkey: &str) -> (Option<f64>, Option<f64>) {
    // SOL native balance
    let sol: Option<f64> = async {
        let body = serde_json::json!({
            "jsonrpc": "2.0", "id": 1,
            "method": "getBalance",
            "params": [pubkey]
        });
        let lamports = client.post(rpc).json(&body)
            .timeout(std::time::Duration::from_secs(5))
            .send().await.ok()?
            .json::<serde_json::Value>().await.ok()?
            ["result"]["value"].as_u64()?;
        Some(lamports as f64 / 1e9)
    }.await;

    // USDC SPL token balance
    let usdc: Option<f64> = async {
        let body = serde_json::json!({
            "jsonrpc": "2.0", "id": 2,
            "method": "getTokenAccountsByOwner",
            "params": [pubkey, {"mint": SOLANA_USDC_MINT}, {"encoding": "jsonParsed"}]
        });
        let data = client.post(rpc).json(&body)
            .timeout(std::time::Duration::from_secs(5))
            .send().await.ok()?
            .json::<serde_json::Value>().await.ok()?;
        let accounts = data["result"]["value"].as_array()?;
        let total: f64 = accounts.iter().filter_map(|a| {
            a["account"]["data"]["parsed"]["info"]["tokenAmount"]["uiAmount"].as_f64()
        }).sum();
        Some(total)
    }.await;

    (sol, usdc)
}

/// Load chain wiring from CHAIN_WIRING_PATH env or the default relative path.
fn load_chain_wiring_for_portfolio() -> Vec<(u64, String, String)> {
    // Returns (chain_id, chain_name, rpc_url) tuples for mainnet chains
    let path = std::env::var("CHAIN_WIRING_PATH")
        .unwrap_or_else(|_| "config/chain_wiring.json".to_string());
    let contents = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let v: serde_json::Value = match serde_json::from_str(&contents) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let obj = match v.as_object() { Some(o) => o, None => return Vec::new() };
    let mut out = Vec::new();
    for (key, val) in obj {
        if key.starts_with('_') { continue; }
        let chain_id: u64 = match key.parse() { Ok(n) => n, Err(_) => continue };
        let name = val["_chain"].as_str().unwrap_or(key.as_str()).to_string();
        if name.contains("Sepolia") || name.contains("Devnet") || name.contains("Testnet") { continue; }
        let rpc = match val["rpc_url"].as_str() { Some(r) => r.to_string(), None => continue };
        out.push((chain_id, name, rpc));
    }
    out
}

fn token_addrs_for_chain(chain_id: u64) -> (Option<&'static str>, Option<&'static str>, Option<&'static str>) {
    match chain_id {
        1 => (
            Some("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
            Some("0xdAC17F958D2ee523a2206206994597C13D831ec7"),
            Some("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"),
        ),
        10 => (
            Some("0x0b2C639c533813f4Aa9D7837CAf62653d097Ff85"),
            Some("0x94b008aA00579c1307B0EF2c499aD98a8ce58e58"), // USDT Optimism
            Some("0x4200000000000000000000000000000000000006"),
        ),
        137 => (
            Some("0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174"),
            Some("0xc2132D05D31c914a87C6611C10748AEb04B58e8F"), // USDT Polygon
            Some("0x7ceB23fD6bC0adD59E62ac25578270cFf1b9f619"),
        ),
        8453 => (
            Some("0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"),
            Some("0xfde4C96c8593536E31F229EA8f37b2ADa2699bb2"), // USDT Base
            Some("0x4200000000000000000000000000000000000006"),
        ),
        42161 => (
            Some("0xaf88d065e77c8cC2239327C5EDb3A432268e5831"),
            Some("0xFd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9"),
            Some("0x82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
        ),
        59144 => (
            Some("0x176211869cA2b568f2A7D4EE941E073a821EE1ff"),
            None,
            Some("0xe5D7C2a44FfDDf6b295A15c148167daaAf5Cf34f"), // WETH Linea
        ),
        130 => (
            Some("0x078d782b760474a361dda7ff6e249887ddf39eb0"), // USDC Unichain
            None,
            Some("0x4200000000000000000000000000000000000006"),
        ),
        324 => (
            Some("0x1d17CBcF0D6D143135aE902365D2E5e2A16538D4"), // USDC zkSync Era
            None,
            Some("0x5AEa5775959fBC2557Cc8789bC1bf90A239D9a91"), // WETH zkSync Era
        ),
        534352 => (
            Some("0x06eFdBFf2a14a7c8E15944D1F4A48F9F95F663A4"), // USDC Scroll
            Some("0xf55BEC9cafDbE8730f096Aa55dad6D22d44099Df"), // USDT Scroll
            Some("0x5300000000000000000000000000000000000004"), // WETH Scroll
        ),
        57073 => (
            Some("0x2d270e6886d130d724215a266106e6832161eaed"), // USDC Ink
            Some("0x0200C29006150606B650577BBe7B6248f58470C1"), // USDT Ink
            Some("0x4200000000000000000000000000000000000006"), // WETH Ink (OP Stack)
        ),
        34443 => (
            Some("0xd988097fb8612cc24eeC14542bC03424c656005f"), // USDC.e Mode
            Some("0xf0F161fDA2712DB8b566946122a5af183995e2eD"), // USDT Mode
            Some("0x4200000000000000000000000000000000000006"), // WETH Mode (OP Stack)
        ),
        43114 => (
            Some("0xB97EF9Ef8734C71904D8002F8b6Bc66Dd9c48a6E"), // USDC Avalanche native
            Some("0x9702230A8Ea53601f5cD2dc00fDbC13d4dF4A8c7"), // USDT Avalanche native
            None,
        ),
        _ => (None, None, None),
    }
}

/// Query parameters for /api/solver/portfolio.
#[derive(Debug, Deserialize, Default)]
struct PortfolioQuery {
    /// Optional EVM address override. When provided, chain balances are fetched
    /// for this address instead of the solver's own SOLVER_ADDRESS. Solana
    /// balance is still read from SOLANA_ADDRESS (since a Solana pubkey can't
    /// be derived from an EVM address). No auth required — balances are public
    /// on-chain data.
    address: Option<String>,
}

async fn portfolio_handler(
    State(state): State<Arc<ApiState>>,
    axum::extract::Query(q): axum::extract::Query<PortfolioQuery>,
) -> impl IntoResponse {
    let solver_addr = q.address
        .as_deref()
        .filter(|a| a.starts_with("0x") && a.len() == 42)
        .map(str::to_string)
        .unwrap_or_else(|| {
            std::env::var("SOLVER_ADDRESS")
                .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".to_string())
        });
    let solana_addr = std::env::var("SOLANA_ADDRESS").ok();

    let client = &state.http_client;

    // Load chains from chain_wiring.json; fall back to hardcoded defaults if unavailable
    let wired = load_chain_wiring_for_portfolio();
    let chain_list: Vec<(u64, String, String)> = if wired.is_empty() {
        vec![
            (1, "Ethereum".into(), "https://ethereum-rpc.publicnode.com".into()),
            (10, "Optimism".into(), "https://optimism-rpc.publicnode.com".into()),
            (137, "Polygon".into(), "https://polygon-rpc.com".into()),
            (8453, "Base".into(), "https://base-rpc.publicnode.com".into()),
            (42161, "Arbitrum".into(), "https://arbitrum-one-rpc.publicnode.com".into()),
            (59144, "Linea".into(), "https://linea-rpc.publicnode.com".into()),
        ]
    } else {
        wired
    };

    let mut chains = Vec::with_capacity(chain_list.len() + 1);

    for (chain_id, chain_name, rpc) in &chain_list {
        let native = eth_balance_f64(client, rpc, &solver_addr).await;
        let (usdc_addr, usdt_addr, weth_addr) = token_addrs_for_chain(*chain_id);
        let usdc = if let Some(t) = usdc_addr {
            erc20_balance_f64(client, rpc, t, &solver_addr, 6).await
        } else { None };
        let usdt = if let Some(t) = usdt_addr {
            erc20_balance_f64(client, rpc, t, &solver_addr, 6).await
        } else { None };
        let weth = if let Some(t) = weth_addr {
            erc20_balance_f64(client, rpc, t, &solver_addr, 18).await
        } else { None };
        chains.push(ChainInventory {
            chain_id: *chain_id,
            chain_name: chain_name.clone(),
            native_eth: native,
            native_sol: None,
            usdc,
            usdt,
            weth,
        });
    }

    // Solana balance — pick up premium RPC from chain_wiring if Solana entry exists
    let sol_rpc = chain_list.iter()
        .find(|(cid, _, _)| *cid == 1_399_811_149)
        .map(|(_, _, rpc)| rpc.clone())
        .unwrap_or_else(|| "https://api.mainnet-beta.solana.com".to_string());
    // Solana SOL gas threshold mirrors portfolio_sidecar::inventory::MIN_SOLANA_SOL
    // and WARN_SOLANA_SOL. Keep these in sync with inventory.rs.
    const MIN_SOLANA_SOL: f64 = 0.005;
    const WARN_SOLANA_SOL: f64 = 0.01;

    let mut solana_sol_balance: Option<f64> = None;
    let mut solana_gas_status: Option<String> = None;
    if let Some(ref pubkey) = solana_addr {
        let (sol, usdc) = sol_balances_f64(client, &sol_rpc, pubkey).await;
        // Classify gas status. `None` from sol_balances_f64 means RPC was
        // unreachable — surface as "unknown" rather than misreporting low_gas.
        solana_gas_status = Some(match sol {
            None => "unknown".to_string(),
            Some(s) if s < MIN_SOLANA_SOL => "low_gas".to_string(),
            Some(s) if s < WARN_SOLANA_SOL => "warn".to_string(),
            Some(_) => "healthy".to_string(),
        });
        solana_sol_balance = sol;
        if matches!(solana_gas_status.as_deref(), Some("low_gas")) {
            tracing::warn!(
                "Solana wallet low on SOL: have={:.6} sol need={:.3} sol",
                sol.unwrap_or(0.0),
                WARN_SOLANA_SOL
            );
        }
        chains.push(ChainInventory {
            chain_id: 1_399_811_149,
            chain_name: "Solana".into(),
            native_eth: None,
            native_sol: sol,
            usdc,
            usdt: None,
            weth: None,
        });
    }

    let stats = state.stats.read().await;
    let fills = PortfolioFillStats {
        confirmed: stats.executed_fills,
        reverted: stats.failed_fills,
        active: 0,
        total_volume_usd: 0.0,
        realized_profit_usd: stats.net_profit_today_usd,
    };
    drop(stats);

    // Fetch LWC well states from the t3rn solver sidecar (best-effort).
    let t3rn_port = std::env::var("T3RN_SOLVER_PORT")
        .ok().and_then(|s| s.parse::<u16>().ok()).unwrap_or(8092);
    let lwc_chains: Vec<LwcChainInfo> = match client
        .get(format!("http://127.0.0.1:{}/t3rn/status", t3rn_port))
        .timeout(std::time::Duration::from_secs(2))
        .send().await
    {
        Ok(resp) if resp.status().is_success() => {
            resp.json::<serde_json::Value>().await
                .ok()
                .and_then(|v| v.get("chains").and_then(|c| serde_json::from_value::<Vec<LwcChainInfo>>(c.clone()).ok()))
                .unwrap_or_default()
        }
        _ => vec![],
    };

    Json(PortfolioResponse {
        solver_address: solver_addr,
        solana_address: solana_addr,
        chains,
        fills,
        as_of: Utc::now(),
        lwc_chains,
        solana_sol_balance,
        solana_gas_status,
    })
}

// =============================================================================
// Frontier Hackathon — Live P&L endpoints
// =============================================================================

/// Query parameters for the outcomes endpoint.
#[derive(Debug, Deserialize)]
pub struct OutcomesQuery {
    /// Number of records to return, newest first. Default 50, max 500.
    #[serde(default)]
    pub limit: Option<i64>,
    /// Filter by solver address or name (matches `solver_id` column).
    #[serde(default)]
    pub solver_id: Option<String>,
    /// Filter by decision value (e.g. "executed", "skip", "dry_run").
    /// When omitted all decisions are returned.
    #[serde(default)]
    pub decision: Option<String>,
}

/// Single outcome record exposed via the dashboard API. Mirrors
/// `executor::OutcomeRecord` but flattens the timestamp to RFC3339 and adds
/// the chain-aware explorer URL up-front so the dashboard doesn't need to
/// know per-chain explorers.
#[derive(Debug, Serialize)]
pub struct OutcomeApiRecord {
    pub ts: String,
    pub intent_id: String,
    pub protocol: String,
    pub src_chain: u64,
    pub dst_chain: u64,
    pub decision: String,
    pub tx_hash: Option<String>,
    pub explorer_url: Option<String>,
    pub predicted_gas: Option<u64>,
    pub gas_used: Option<u64>,
    pub effective_gas_price_wei: Option<String>,
    pub predicted_profit_usd: Option<f64>,
    pub actual_profit_usd: Option<f64>,
    pub skip_reason: Option<String>,
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solver_id: Option<String>,
    /// Issue #10: source-chain `claimUnlock()` tx hash (deBridge). NULL until
    /// the claim confirms. Front-end uses presence/absence to drive the
    /// "Claim pending" badge in `ClaimsPanel`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim_tx_hash: Option<String>,
    /// Issue #10: USD value of the released spread, co-populated with
    /// `claim_tx_hash`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim_fee_usd: Option<f64>,
    /// Source-chain explorer URL for `claim_tx_hash`, computed server-side
    /// so the dashboard doesn't need per-chain explorer awareness.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim_explorer_url: Option<String>,
}

fn explorer_url_for(chain_id: u64, tx_hash: &str) -> Option<String> {
    let base = match chain_id {
        1 => "https://etherscan.io/tx/",
        10 => "https://optimistic.etherscan.io/tx/",
        137 => "https://polygonscan.com/tx/",
        8453 => "https://basescan.org/tx/",
        42161 => "https://arbiscan.io/tx/",
        59144 => "https://lineascan.build/tx/",
        324 => "https://explorer.zksync.io/tx/",
        56 => "https://bscscan.com/tx/",
        43114 => "https://snowtrace.io/tx/",
        // Solana uses base58 signatures; tx_hash already includes them.
        1_399_811_149 => "https://solscan.io/tx/",
        _ => return None,
    };
    Some(format!("{base}{tx_hash}"))
}

/// GET /api/solver/outcomes?limit=N[&decision=executed][&solver_id=...]
///
/// Returns the most recent outcome records from the rusqlite outcome log,
/// newest first. Returns an empty array when no `OutcomeLog` is configured.
/// When `decision` is supplied only records whose `decision` column matches
/// (case-insensitive prefix) are returned; the `limit` is applied *after*
/// filtering so the caller always gets up to N matching records.
async fn outcomes_handler(
    State(state): State<Arc<ApiState>>,
    axum::extract::Query(q): axum::extract::Query<OutcomesQuery>,
) -> Json<Vec<OutcomeApiRecord>> {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    let solver_id = q.solver_id.clone();
    let decision_filter = q.decision.clone().map(|d| d.to_lowercase());
    let log = match state.outcome_log.get() {
        Some(l) => l.clone(),
        None => return Json(Vec::new()),
    };

    // Fetch a larger window when a decision filter is active so we don't
    // accidentally return fewer records than `limit` after filtering.
    let fetch_limit = if decision_filter.is_some() {
        (limit * 20).clamp(1, 500)
    } else {
        limit
    };

    let log_clone = log.clone();
    let recs = tokio::task::spawn_blocking(move || {
        log_clone.recent_for(fetch_limit, solver_id.as_deref())
    })
        .await
        .unwrap_or_else(|_| Ok(Vec::new()))
        .unwrap_or_default();

    // Apply optional decision filter, then re-apply limit.
    let recs = if let Some(ref df) = decision_filter {
        recs.into_iter()
            .filter(|r| r.decision.to_lowercase().contains(df.as_str()))
            .take(limit as usize)
            .collect::<Vec<_>>()
    } else {
        recs
    };

    let api_recs: Vec<OutcomeApiRecord> = recs
        .into_iter()
        .map(|r| {
            let explorer_url = r
                .tx_hash
                .as_deref()
                .and_then(|tx| explorer_url_for(r.dst_chain, tx));
            // Claim happens on the source chain (deBridge), so resolve the
            // explorer URL against `src_chain`, not `dst_chain`.
            let claim_explorer_url = r
                .claim_tx_hash
                .as_deref()
                .and_then(|tx| explorer_url_for(r.src_chain, tx));
            OutcomeApiRecord {
                ts: r.ts.to_rfc3339(),
                intent_id: r.intent_id,
                protocol: r.protocol,
                src_chain: r.src_chain,
                dst_chain: r.dst_chain,
                decision: r.decision,
                tx_hash: r.tx_hash,
                explorer_url,
                predicted_gas: r.predicted_gas,
                gas_used: r.gas_used,
                effective_gas_price_wei: r.effective_gas_price_wei,
                predicted_profit_usd: r.predicted_profit_usd,
                actual_profit_usd: r.actual_profit_usd,
                skip_reason: r.skip_reason,
                error: r.error,
                solver_id: r.solver_id,
                claim_tx_hash: r.claim_tx_hash,
                claim_fee_usd: r.claim_fee_usd,
                claim_explorer_url,
            }
        })
        .collect();

    Json(api_recs)
}

/// GET /api/solver/pnl
///
/// Aggregate P&L summary backing the dashboard `LivePnL` panel. Result is
/// cached for `PNL_CACHE_TTL_SECS` (env, default 2s) to avoid hammering
/// rusqlite. Returns zeros when no `OutcomeLog` is configured (so the
/// dashboard renders gracefully in dev).
async fn pnl_handler(
    State(state): State<Arc<ApiState>>,
) -> Json<executor::PnlSummary> {
    let ttl_secs: u64 = std::env::var("PNL_CACHE_TTL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2);

    {
        let cache = state.pnl_cache.read().await;
        if cache.last_refresh.elapsed() < std::time::Duration::from_secs(ttl_secs) {
            return Json(cache.cached.clone());
        }
    }

    let log = match state.outcome_log.get() {
        Some(l) => l.clone(),
        None => {
            return Json(executor::PnlSummary {
                realized_usd_total: 0.0,
                fills_total: 0,
                last_24h_count: 0,
                by_protocol: HashMap::new(),
            });
        }
    };

    let log_clone = log.clone();
    let summary = tokio::task::spawn_blocking(move || log_clone.pnl_summary())
        .await
        .ok()
        .and_then(|r| r.ok())
        .unwrap_or(executor::PnlSummary {
            realized_usd_total: 0.0,
            fills_total: 0,
            last_24h_count: 0,
            by_protocol: HashMap::new(),
        });

    {
        let mut cache = state.pnl_cache.write().await;
        cache.cached = summary.clone();
        cache.last_refresh = std::time::Instant::now();
    }

    Json(summary)
}

async fn open_intents_handler(
    State(state): State<Arc<ApiState>>,
) -> impl IntoResponse {
    let now = chrono::Utc::now();
    let wm = match state.wallet_manager.get() {
        Some(w) => w.clone(),
        None => return Json(serde_json::json!({ "open_intents": [], "count": 0, "error": "wallet_manager_not_wired" })),
    };
    let open_states = ["CALLDATA_BUILD", "BROADCAST", "PENDING_CONFIRMATION", "REVERTED", "INTENT_DETECTED"];
    let mut all: Vec<serde_json::Value> = Vec::new();
    for s in &open_states {
        if let Ok(records) = wm.list_intents(Some(s), 100) {
            for r in records {
                let age_secs = (now - r.created_at).num_seconds();
                all.push(serde_json::json!({
                    "intent_id": r.intent_id,
                    "protocol": r.protocol,
                    "state": r.state,
                    "amount_usd": r.amount_usd,
                    "src_chain": r.src_chain,
                    "dst_chain": r.dst_chain,
                    "created_at": r.created_at.to_rfc3339(),
                    "age_secs": age_secs,
                    "tx_hash": r.tx_hash,
                    "error": r.error,
                }));
            }
        }
    }
    all.sort_by(|a, b| {
        let ta = a["created_at"].as_str().unwrap_or("");
        let tb = b["created_at"].as_str().unwrap_or("");
        tb.cmp(ta)
    });
    let count = all.len();
    Json(serde_json::json!({ "open_intents": all, "count": count }))
}

// =============================================================================
// Rebalancer manual trigger + status (auth-gated)
// =============================================================================

/// Bearer-token auth for `/api/solver/*`. Reads `SOLVER_API_TOKEN` from the
/// environment and compares it against the `Authorization: Bearer <token>`
/// header.
///
/// Issue #8: any failure mode (missing env, missing header, wrong scheme,
/// mismatched token) returns `401 Unauthorized` with body
/// `{"error":"unauthorized"}` — the contract the brief specified.
/// `SOLVER_API_TOKEN` unset is treated as fail-closed: callers cannot
/// distinguish a misconfigured server from a wrong token, which is the
/// correct posture for an authentication boundary.
async fn require_solver_api_token(req: Request<Body>, next: Next) -> Response {
    let unauthorized = || {
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unauthorized" })),
        )
            .into_response()
    };

    let expected = match std::env::var("SOLVER_API_TOKEN") {
        Ok(v) if !v.trim().is_empty() => v,
        _ => return unauthorized(),
    };

    let provided = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(str::trim)
        .unwrap_or("");

    // Constant-time comparison to resist timing-based token enumeration.
    let provided_b = provided.as_bytes();
    let expected_b = expected.as_bytes();
    let len_ok = provided_b.len() == expected_b.len();
    let cmp_len = provided_b.len().max(expected_b.len());
    let acc = (0..cmp_len)
        .map(|i| {
            let a = provided_b.get(i).copied().unwrap_or(0);
            let b = expected_b.get(i).copied().unwrap_or(0);
            a ^ b
        })
        .fold(0u8, |acc, x| acc | x);
    if !len_ok || acc != 0 {
        return unauthorized();
    }

    next.run(req).await
}

/// `GET /health` — unauthenticated liveness probe. Returns `200 OK` with a
/// small JSON envelope. Used by load balancers, orchestrators, and the
/// dashboard's at-a-glance availability check. Intentionally outside the
/// `require_solver_api_token` gate.
async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "service": "taifoon-solver-api",
    }))
}

// =============================================================================
// Donut attestation routes
// =============================================================================
//
// Status-code contract (matches `PersistError` variants):
//   - 200  → attestation persisted
//   - 400  → signature didn't verify (`InvalidSignature`)
//   - 409  → duplicate fill_id (`DuplicateFill`)
//   - 422  → bad chain link (`BadChain`)
//   - 500  → internal failure (`Internal`)

/// POST /api/donut/attest — Spinner pushes a signed [`DonutAttestation`].
/// Gated by `require_solver_api_token`.
async fn donut_attest_handler(
    State(registry): State<HostingRegistryState>,
    Json(att): Json<DonutAttestation>,
) -> Response {
    match registry.persist_attestation(&att) {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                "fill_id": att.fill_id,
                "spinner_id": att.spinner_id,
                "donut_take_usd_micro": att.donut_take_usd_micro,
            })),
        )
            .into_response(),
        Err(PersistError::InvalidSignature(msg)) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "invalid_signature", "detail": msg })),
        )
            .into_response(),
        Err(PersistError::DuplicateFill(id)) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "duplicate_fill_id", "fill_id": id })),
        )
            .into_response(),
        Err(PersistError::BadChain { expected, got }) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({
                "error": "bad_chain_link",
                "expected_prev_hash": expected,
                "got_prev_hash": got,
            })),
        )
            .into_response(),
        Err(PersistError::Internal(msg)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "internal", "detail": msg })),
        )
            .into_response(),
    }
}

/// GET /api/donut/ledger/:spinner_id — public ledger read.
async fn donut_ledger_handler(
    State(registry): State<HostingRegistryState>,
    axum::extract::Path(spinner_id): axum::extract::Path<String>,
) -> Response {
    match registry.ledger_for(&spinner_id) {
        Ok(rows) => (StatusCode::OK, Json(serde_json::json!({
            "spinner_id": spinner_id,
            "count": rows.len(),
            "attestations": rows,
        })))
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// GET /api/donut/ledger/:spinner_id/head — current chain head.
async fn donut_head_handler(
    State(registry): State<HostingRegistryState>,
    axum::extract::Path(spinner_id): axum::extract::Path<String>,
) -> Response {
    match registry.ledger_head(&spinner_id) {
        Ok((prev_hash, count)) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "spinner_id": spinner_id,
                "prev_hash": prev_hash,
                "count": count,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// GET /api/donut/policy — public, unauthenticated.
///
/// Returns the canonical donut-policy constants. This is the single
/// source of truth for "the 49 bps × 70/20/10 split applied uniformly
/// across every provisioned Builder." Auditors and dashboards read this
/// to verify a Spinner's attestations match the published policy.
async fn donut_policy_handler(
    State(_reg): State<Arc<AdapterRegistry>>,
) -> Response {
    let policy = DonutPolicy::canonical();
    (StatusCode::OK, Json(policy)).into_response()
}

/// GET /api/donut/registry — public, unauthenticated.
///
/// Returns the loaded adapter → builder + reviewer map. Anyone can
/// confirm which on-chain address gets the 70% creator share for each
/// adapter, and which reviewer set gets the 20% split. The ZERO address
/// in the response means the config wasn't loaded (fail-closed) and the
/// Spinner should not be running until ADAPTER_REGISTRY_PATH points at a
/// valid file.
async fn donut_registry_handler(
    State(reg): State<Arc<AdapterRegistry>>,
) -> Response {
    let view = reg.view();
    (StatusCode::OK, Json(view)).into_response()
}

/// Single bridge action surfaced by the rebalance trigger response. Mirrors
/// the brief's contract: `src_chain`, `dst_chain`, `amount_usd`, `tx_hash`.
#[derive(Debug, Serialize)]
pub struct RebalanceActionApi {
    pub src_chain: u64,
    pub dst_chain: u64,
    pub amount_usd: f64,
    pub tx_hash: Option<String>,
}

impl From<&BridgeAction> for RebalanceActionApi {
    fn from(a: &BridgeAction) -> Self {
        Self {
            src_chain: a.src_chain,
            dst_chain: a.dst_chain,
            amount_usd: a.amount_usd,
            tx_hash: a.tx_hash.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct RebalanceTriggerResponse {
    pub bridges: Vec<RebalanceActionApi>,
    pub skipped_reason: Option<String>,
}

/// POST /api/solver/rebalance
///
/// Fires one rebalance cycle out-of-band, blocks until it completes, and
/// returns the bridge actions taken. If the background loop is currently
/// running a tick we surface `skipped_reason: "rebalance_in_progress"` rather
/// than queuing a parallel scan.
async fn rebalance_handler(
    State(state): State<Arc<ApiState>>,
) -> impl IntoResponse {
    let sidecar = match state.sidecar.get() {
        Some(s) => s.clone(),
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(RebalanceTriggerResponse {
                    bridges: Vec::new(),
                    skipped_reason: Some("rebalancer_not_configured".to_string()),
                }),
            )
                .into_response();
        }
    };

    // Refuse to queue a second concurrent tick.
    let guard = match sidecar.try_lock_tick() {
        Some(g) => g,
        None => {
            return (
                StatusCode::CONFLICT,
                Json(RebalanceTriggerResponse {
                    bridges: Vec::new(),
                    skipped_reason: Some("rebalance_in_progress".to_string()),
                }),
            )
                .into_response();
        }
    };
    // Drop the guard before calling tick(); tick() takes the lock itself.
    drop(guard);

    tracing::info!("manual rebalance trigger via /api/solver/rebalance");
    let actions = sidecar.tick().await;

    let bridges: Vec<RebalanceActionApi> = actions.iter().map(Into::into).collect();
    let skipped_reason = if bridges.is_empty() {
        Some("no_actions_needed".to_string())
    } else {
        None
    };

    (
        StatusCode::OK,
        Json(RebalanceTriggerResponse {
            bridges,
            skipped_reason,
        }),
    )
        .into_response()
}

#[derive(Debug, Serialize)]
pub struct RebalancerStatusResponse {
    pub last_run_at: Option<DateTime<Utc>>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub last_actions: Vec<ActionLogEntry>,
    pub blocked_reason: Option<String>,
    pub interval_secs: u64,
    pub cycle: u64,
}

/// GET /api/solver/rebalancer/status
///
/// Returns the most recent scan timestamp, the next scheduled tick (computed
/// from `last_run_at + interval_secs`), the last 10 logged actions, and any
/// blocked-state reason. Returns `503` when the sidecar isn't wired.
async fn rebalancer_status_handler(
    State(state): State<Arc<ApiState>>,
) -> impl IntoResponse {
    let sidecar = match state.sidecar.get() {
        Some(s) => s.clone(),
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": "rebalancer_not_configured",
                    "message": "PortfolioSidecar was not registered on this solver-api instance"
                })),
            )
                .into_response();
        }
    };

    let interval_secs = sidecar.interval_secs();
    let snapshot = sidecar.state.read().await;
    let last_run_at = snapshot.last_scan;
    let cycle = snapshot.cycle;

    // Clone the most recent 10 entries so the response stays bounded.
    let last_actions: Vec<ActionLogEntry> = snapshot
        .action_log
        .iter()
        .rev()
        .take(10)
        .cloned()
        .collect();

    let next_run_at = match (last_run_at, interval_secs) {
        (Some(t), n) if n > 0 => Some(t + chrono::Duration::seconds(n as i64)),
        _ => None,
    };

    // The currently-locked rebalance loop is the only "blocked" state we can
    // detect from outside the sidecar. Anything else (e.g. SOLVER_PRIVATE_KEY
    // unset, RPC outage) is invisible from here without coupling solver-api
    // to the rebalancer's internals.
    let blocked_reason = if sidecar.try_lock_tick().is_none() {
        Some("rebalance_in_progress".to_string())
    } else {
        None
    };

    drop(snapshot);

    (
        StatusCode::OK,
        Json(RebalancerStatusResponse {
            last_run_at,
            next_run_at,
            last_actions,
            blocked_reason,
            interval_secs,
            cycle,
        }),
    )
        .into_response()
}

// =============================================================================
// Claims tab — deBridge claim lifecycle visibility + manual retry (auth-gated)
// =============================================================================

/// Single row returned by `GET /api/solver/claims`. The brief calls for
/// `intent_id, fill_tx_hash, claim_tx_hash, claim_fee_usd, created_at,
/// age_minutes` — see field-level docs for how each is derived.
#[derive(Debug, Serialize)]
pub struct ClaimRow {
    pub intent_id: String,
    pub protocol: String,
    pub src_chain: i64,
    pub dst_chain: i64,
    pub amount_usd: f64,
    /// Wallet state. One of `CONFIRMED` (claim never sent), `CLAIM_PENDING`
    /// (claim broadcast, awaiting receipt), `CLAIMED` (terminal — claim tx
    /// confirmed), or `REVERTED` (fill or claim reverted).
    pub wallet_state: String,
    /// "pending" | "claimed" | "reverted" — derived from `wallet_state` so
    /// dashboards don't need to know the wallet state machine. Matches the
    /// brief: Pending vs Claimed.
    pub claim_status: String,
    /// Original fill tx hash. Recovered from `solver_outcomes` because the
    /// wallet `tx_hash` field is overwritten by the `Claimed` transition on
    /// successful claim.
    pub fill_tx_hash: Option<String>,
    /// `null` when the claim is pending. Set to the wallet `tx_hash` on
    /// `CLAIMED` rows (which is the claim tx after the transition).
    pub claim_tx_hash: Option<String>,
    /// Realized claim fee in USD. From `wallet.revenue.profit_usd`, populated
    /// only after `lambda_claim_debridge` calls `record_revenue`.
    pub claim_fee_usd: Option<f64>,
    pub created_at: DateTime<Utc>,
    pub age_minutes: i64,
    pub error: Option<String>,
}

/// GET /api/solver/claims
/// Token-gated. Returns the rows from the wallet `intents` table where
/// protocol matches `%debridge%`/`%dln%` and the intent has reached the claim
/// lifecycle (CONFIRMED/CLAIM_PENDING/CLAIMED/REVERTED). The fill tx is
/// recovered from `solver_outcomes` so the dashboard always sees the original
/// fill even after the wallet row's `tx_hash` is overwritten by the claim.
async fn claims_handler(
    State(state): State<Arc<ApiState>>,
) -> impl IntoResponse {
    let wallet = match state.wallet_manager.get() {
        Some(w) => w.clone(),
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": "wallet_manager_not_wired",
                    "claims": [],
                    "pending_count": 0,
                })),
            )
                .into_response();
        }
    };

    let rows = match wallet.list_debridge_claims(500) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("claims_handler: list_debridge_claims failed: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("wallet read failed: {e}"),
                    "claims": [],
                    "pending_count": 0,
                })),
            )
                .into_response();
        }
    };

    let outcome_log = state.outcome_log.get().cloned();
    let now = Utc::now();
    let mut claims: Vec<ClaimRow> = Vec::with_capacity(rows.len());
    for r in rows {
        let (claim_status, claim_tx_hash, fill_tx_hash) = match r.state.as_str() {
            "CLAIMED" => {
                // wallet.tx_hash was overwritten by the claim tx; recover the
                // original fill tx from the outcome log.
                let fill_tx = match outcome_log.as_ref() {
                    Some(log) => log
                        .first_fill_tx_for(&r.intent_id)
                        .ok()
                        .flatten(),
                    None => None,
                };
                (
                    "claimed".to_string(),
                    r.wallet_tx_hash.clone(),
                    fill_tx,
                )
            }
            "REVERTED" => (
                "reverted".to_string(),
                None,
                r.wallet_tx_hash.clone(),
            ),
            // CONFIRMED or CLAIM_PENDING — wallet.tx_hash is still the fill tx.
            _ => (
                "pending".to_string(),
                None,
                r.wallet_tx_hash.clone(),
            ),
        };

        let age_minutes = ((now - r.created_at).num_seconds() / 60).max(0);
        claims.push(ClaimRow {
            intent_id: r.intent_id,
            protocol: r.protocol,
            src_chain: r.src_chain,
            dst_chain: r.dst_chain,
            amount_usd: r.amount_usd,
            wallet_state: r.state,
            claim_status,
            fill_tx_hash,
            claim_tx_hash,
            claim_fee_usd: r.claim_fee_usd,
            created_at: r.created_at,
            age_minutes,
            error: r.error,
        });
    }

    let pending_count = claims
        .iter()
        .filter(|c| c.claim_status == "pending")
        .count();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "claims": claims,
            "pending_count": pending_count,
            "as_of": now,
        })),
    )
        .into_response()
}

#[derive(Debug, Serialize)]
pub struct ClaimRetryResponse {
    pub intent_id: String,
    pub outcome: String,
    pub claim_tx_hash: Option<String>,
    pub fee_usd: Option<f64>,
    pub error: Option<String>,
}

/// POST /api/solver/claims/:intent_id/retry
/// Token-gated. Reconstructs a synthetic `Intent` (id, protocol, src/dst
/// chain, order_id) from the wallet record — same approach as the periodic
/// `debridge_claim_retry_tick` in solver-main — and calls
/// `LambdaController::lambda_claim_debridge`. Returns the outcome variant.
async fn claim_retry_handler(
    State(state): State<Arc<ApiState>>,
    axum::extract::Path(intent_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let wallet = match state.wallet_manager.get() {
        Some(w) => w.clone(),
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ClaimRetryResponse {
                    intent_id,
                    outcome: "service_unavailable".into(),
                    claim_tx_hash: None,
                    fee_usd: None,
                    error: Some("wallet_manager_not_wired".into()),
                }),
            )
                .into_response();
        }
    };
    let ctrl = match state.lambda_controller.get() {
        Some(c) => c.clone(),
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ClaimRetryResponse {
                    intent_id,
                    outcome: "service_unavailable".into(),
                    claim_tx_hash: None,
                    fee_usd: None,
                    error: Some(
                        "lambda_controller_not_wired (solver may be running without SOLVER_PRIVATE_KEY)"
                            .into(),
                    ),
                }),
            )
                .into_response();
        }
    };

    // Find the intent record. The retry only proceeds for protocols matching
    // debridge/dln — same filter the background tick uses. We look up across
    // all states because the dashboard may want to retry a CLAIM_PENDING that
    // got stuck.
    let candidates = match wallet.list_debridge_claims(1000) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ClaimRetryResponse {
                    intent_id,
                    outcome: "wallet_error".into(),
                    claim_tx_hash: None,
                    fee_usd: None,
                    error: Some(format!("wallet list failed: {e}")),
                }),
            )
                .into_response();
        }
    };
    let record = match candidates.into_iter().find(|r| r.intent_id == intent_id) {
        Some(r) => r,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(ClaimRetryResponse {
                    intent_id,
                    outcome: "not_found".into(),
                    claim_tx_hash: None,
                    fee_usd: None,
                    error: Some("intent not in deBridge claim lifecycle".into()),
                }),
            )
                .into_response();
        }
    };

    // DeBridgePoller sets intent.id = "debridge_dln:0x<orderId>". Match the
    // shape that `solver_main::debridge_claim_retry_tick` builds.
    let order_id_hex = if record.intent_id.contains(':') {
        record
            .intent_id
            .splitn(2, ':')
            .nth(1)
            .unwrap_or(&record.intent_id)
            .to_string()
    } else {
        record.intent_id.clone()
    };
    let synthetic = genome_client::Intent {
        id: record.intent_id.clone(),
        protocol: record.protocol.clone(),
        src_chain: record.src_chain as u64,
        dst_chain: record.dst_chain as u64,
        order_id: Some(order_id_hex),
        ..genome_client::Intent::default()
    };

    tracing::info!(
        "🔁 manual claim retry via /api/solver/claims/{}/retry",
        intent_id
    );
    match ctrl.lambda_claim_debridge(&synthetic).await {
        Ok(executor::LambdaClaimOutcome::Claimed { tx_hash, fee_usd }) => (
            StatusCode::OK,
            Json(ClaimRetryResponse {
                intent_id,
                outcome: "claimed".into(),
                claim_tx_hash: Some(tx_hash),
                fee_usd: Some(fee_usd),
                error: None,
            }),
        )
            .into_response(),
        Ok(executor::LambdaClaimOutcome::NotEligible { reason }) => (
            StatusCode::CONFLICT,
            Json(ClaimRetryResponse {
                intent_id,
                outcome: "not_eligible".into(),
                claim_tx_hash: None,
                fee_usd: None,
                error: Some(reason),
            }),
        )
            .into_response(),
        Ok(executor::LambdaClaimOutcome::Failed { error: e }) => (
            StatusCode::BAD_GATEWAY,
            Json(ClaimRetryResponse {
                intent_id,
                outcome: "failed".into(),
                claim_tx_hash: None,
                fee_usd: None,
                error: Some(e),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ClaimRetryResponse {
                intent_id,
                outcome: "fatal".into(),
                claim_tx_hash: None,
                fee_usd: None,
                error: Some(e.to_string()),
            }),
        )
            .into_response(),
    }
}
