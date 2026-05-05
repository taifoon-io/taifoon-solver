use axum::{
    Router,
    routing::get,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
        Json,
    },
    extract::State,
};
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
    razor_cache: Arc<RwLock<HashMap<u64, RazorGasPreset>>>,
    warmbed_api_url: String,
    http_client: reqwest::Client,
    /// Optional outcome log — when set, exposes /api/solver/outcomes and
    /// /api/solver/pnl endpoints reading realized P&L from rusqlite.
    /// `OnceLock` so solver-main can inject it after the router has already
    /// cloned the `Arc<ApiState>` — handlers read via `.get()` which is safe
    /// after a single set.
    outcome_log: std::sync::OnceLock<Arc<executor::OutcomeLog>>,
    pnl_cache: Arc<RwLock<PnlCache>>,
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
                http_client: reqwest::Client::new(),
                outcome_log: std::sync::OnceLock::new(),
                pnl_cache: Arc::new(RwLock::new(PnlCache::default())),
            }),
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

    /// Get router for Axum server
    pub fn router(&self) -> Router {
        Router::new()
            .route("/api/solver/stream", get(stream_handler))
            .route("/api/solver/logs", get(logs_handler))
            .route("/api/solver/stats", get(stats_handler))
            .route("/api/solver/intents", get(intents_handler))
            .route("/api/solver/protocols", get(protocols_handler))
            .route("/api/solver/money-flow", get(money_flow_handler))
            .route("/api/solver/razor", get(razor_handler))
            .route("/api/solver/portfolio", get(portfolio_handler))
            .route("/api/solver/outcomes", get(outcomes_handler))
            .route("/api/solver/pnl", get(pnl_handler))
            .layer(tower_http::cors::CorsLayer::permissive())
            .with_state(self.state.clone())
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
        if let Some(cached) = cache.get(&chain_id) {
            // Return cached if less than 30 seconds old
            if let Some(age_ms) = cached.age_ms {
                if age_ms < 30_000 {
                    return cached.clone();
                }
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

                    // Update cache
                    let mut cache = state.razor_cache.write().await;
                    cache.insert(chain_id, preset.clone());

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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PortfolioResponse {
    pub solver_address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solana_address: Option<String>,
    pub chains: Vec<ChainInventory>,
    pub fills: PortfolioFillStats,
    pub as_of: DateTime<Utc>,
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
            None,
            Some("0x4200000000000000000000000000000000000006"),
        ),
        137 => (
            Some("0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174"),
            None,
            Some("0x7ceB23fD6bC0adD59E62ac25578270cFf1b9f619"),
        ),
        8453 => (
            Some("0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"),
            None,
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
            None,
        ),
        _ => (None, None, None),
    }
}

async fn portfolio_handler(
    State(state): State<Arc<ApiState>>,
) -> impl IntoResponse {
    let solver_addr = std::env::var("SOLVER_ADDRESS")
        .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".to_string());
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
    let sol_rpc = {
        let w = load_chain_wiring_for_portfolio();
        w.iter()
            .find(|(cid, _, _)| *cid == 1_399_811_149)
            .map(|(_, _, rpc)| rpc.clone())
            .unwrap_or_else(|| "https://api.mainnet-beta.solana.com".to_string())
    };
    if let Some(ref pubkey) = solana_addr {
        let (sol, usdc) = sol_balances_f64(client, &sol_rpc, pubkey).await;
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

    Json(PortfolioResponse {
        solver_address: solver_addr,
        solana_address: solana_addr,
        chains,
        fills,
        as_of: Utc::now(),
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

/// GET /api/solver/outcomes?limit=N
///
/// Returns the most recent outcome records from the rusqlite outcome log,
/// newest first. Returns an empty array when no `OutcomeLog` is configured.
async fn outcomes_handler(
    State(state): State<Arc<ApiState>>,
    axum::extract::Query(q): axum::extract::Query<OutcomesQuery>,
) -> Json<Vec<OutcomeApiRecord>> {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    let log = match state.outcome_log.get() {
        Some(l) => l.clone(),
        None => return Json(Vec::new()),
    };

    let log_clone = log.clone();
    let recs = tokio::task::spawn_blocking(move || log_clone.recent(limit))
        .await
        .unwrap_or_else(|_| Ok(Vec::new()))
        .unwrap_or_default();

    let api_recs: Vec<OutcomeApiRecord> = recs
        .into_iter()
        .map(|r| {
            let explorer_url = r
                .tx_hash
                .as_deref()
                .and_then(|tx| explorer_url_for(r.dst_chain, tx));
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
