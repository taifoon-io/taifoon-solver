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

/// Shared state
pub struct ApiState {
    event_tx: broadcast::Sender<SolverEvent>,
    stats: Arc<RwLock<SolverStats>>,
    intents: Arc<RwLock<Vec<IntentRecord>>>,
    protocols: Arc<RwLock<HashMap<String, ProtocolStats>>>,
    money_flow: Arc<RwLock<MoneyFlow>>,
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
                            intent.tx_hash = Some(solved.tx_hash);
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
