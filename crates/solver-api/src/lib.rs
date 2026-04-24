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
use std::collections::HashMap;

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

/// Shared state
pub struct ApiState {
    event_tx: broadcast::Sender<SolverEvent>,
    stats: Arc<RwLock<SolverStats>>,
    intents: Arc<RwLock<Vec<IntentRecord>>>,
    protocols: Arc<RwLock<HashMap<String, ProtocolStats>>>,
    money_flow: Arc<RwLock<MoneyFlow>>,
    razor_cache: Arc<RwLock<HashMap<u64, RazorGasPreset>>>,
    warmbed_api_url: String,
    http_client: reqwest::Client,
}

/// Main solver API
pub struct SolverApi {
    state: Arc<ApiState>,
}

impl SolverApi {
    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel(1000);

        let mut stats = SolverStats::default();
        stats.status = "live".to_string();
        stats.latency_ms = 127;
        stats.success_rate = 0.942;

        let warmbed_api_url = std::env::var("WARMBED_API_URL")
            .unwrap_or_else(|_| "http://localhost:8082".to_string());

        Self {
            state: Arc::new(ApiState {
                event_tx,
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
            }),
        }
    }

    /// Get router for Axum server
    pub fn router(&self) -> Router {
        Router::new()
            .route("/api/solver/stream", get(stream_handler))
            .route("/api/solver/stats", get(stats_handler))
            .route("/api/solver/intents", get(intents_handler))
            .route("/api/solver/protocols", get(protocols_handler))
            .route("/api/solver/money-flow", get(money_flow_handler))
            .route("/api/solver/razor", get(razor_handler))
            .layer(tower_http::cors::CorsLayer::permissive())
            .with_state(self.state.clone())
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
