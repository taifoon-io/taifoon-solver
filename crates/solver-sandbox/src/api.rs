//! Sandbox HTTP API — exposes well state, leaderboard, and test controls.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::well_sim::{WellSimulator, WellSnapshot};
use crate::genome_replay::GenomeEvent;

#[derive(Clone)]
pub struct ApiState {
    pub well: Arc<Mutex<WellSimulator>>,
    pub events: Arc<Mutex<Vec<GenomeEvent>>>,
    pub leaderboard: Arc<Mutex<Option<crate::compete_sim::Leaderboard>>>,
}

pub fn api_router(state: ApiState) -> Router {
    Router::new()
        .route("/sandbox/wells", get(get_wells))
        .route("/sandbox/wells/fund", post(fund_well))
        .route("/sandbox/wells/halt/:chain_id", post(halt_well))
        .route("/sandbox/intents/inject", post(inject_intent))
        .route("/sandbox/leaderboard", get(get_leaderboard))
        .route("/sandbox/reset", post(reset))
        .with_state(state)
}

async fn get_wells(State(s): State<ApiState>) -> Json<Vec<WellSnapshot>> {
    Json(s.well.lock().await.snapshot())
}

#[derive(Deserialize)]
struct FundRequest {
    chain_id: u64,
    asset: String,
    amount_usd: f64,
    owner: Option<String>,
}

async fn fund_well(
    State(s): State<ApiState>,
    Json(req): Json<FundRequest>,
) -> StatusCode {
    let owner = req.owner.as_deref().unwrap_or("sandbox_admin");
    s.well.lock().await.seed(req.chain_id, &req.asset, req.amount_usd, owner);
    StatusCode::OK
}

#[derive(Deserialize)]
struct HaltRequest {
    asset: String,
    halted: bool,
}

async fn halt_well(
    State(s): State<ApiState>,
    Path(chain_id): Path<u64>,
    Json(req): Json<HaltRequest>,
) -> StatusCode {
    s.well.lock().await.set_halted(chain_id, &req.asset, req.halted);
    StatusCode::OK
}

async fn inject_intent(
    State(s): State<ApiState>,
    Json(ev): Json<GenomeEvent>,
) -> StatusCode {
    s.events.lock().await.push(ev);
    StatusCode::CREATED
}

async fn get_leaderboard(
    State(s): State<ApiState>,
) -> Result<Json<crate::compete_sim::Leaderboard>, (StatusCode, &'static str)> {
    match s.leaderboard.lock().await.clone() {
        Some(lb) => Ok(Json(lb)),
        None => Err((StatusCode::NOT_FOUND, "simulation not yet complete")),
    }
}

async fn reset(State(s): State<ApiState>) -> StatusCode {
    *s.well.lock().await = WellSimulator::new();
    s.events.lock().await.clear();
    *s.leaderboard.lock().await = None;
    StatusCode::OK
}
