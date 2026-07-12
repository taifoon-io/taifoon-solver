use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;
use tracing::{info, warn};
use uuid::Uuid;

use crate::dispatch::{dispatch, ArcIntentInbound};
use crate::state::{SidecarState, StoredIntent};

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    simulation_mode: bool,
    live_fill_ok: bool,
    network: String,
    adapters_loaded: Vec<&'static str>,
}

#[derive(Serialize)]
struct IntentAccepted {
    accepted: bool,
    sidecar_intent_id: String,
    dry_run_effective: bool,
}

#[derive(Serialize, Deserialize)]
struct ErrorBody {
    error: String,
}

pub fn router(state: SidecarState) -> Router {
    Router::new()
        .route("/api/sidecar/health", get(health))
        .route("/api/sidecar/intent", post(post_intent))
        .route("/api/sidecar/intent/:id", get(get_intent))
        .with_state(state)
        .layer(CorsLayer::permissive())
}

async fn health(State(state): State<SidecarState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        simulation_mode: state.config.simulation_mode,
        live_fill_ok: state.config.live_fill_ok,
        network: state.config.network.clone(),
        // Static for now — these are the slugs AdapterFactory will match.
        adapters_loaded: vec!["across", "debridge", "mayan_swift", "lifi"],
    })
}

async fn post_intent(
    State(state): State<SidecarState>,
    Json(payload): Json<ArcIntentInbound>,
) -> Response {
    // Hard gate: live fill requested but two-key safety is closed.
    if !payload.dry_run && !state.config.live_fills_unlocked() {
        warn!(
            slug = %payload.protocol_slug,
            "live fill rejected: SIMULATION_MODE or LIVE_FILL_OK not flipped"
        );
        return (
            StatusCode::LOCKED,
            Json(ErrorBody {
                error: "live fills locked: SIMULATION_MODE must be false AND LIVE_FILL_OK=yes".into(),
            }),
        )
            .into_response();
    }

    let sidecar_intent_id = format!("sc-{}", Uuid::new_v4());
    let now = Utc::now();

    // Insert a "pending" row before kicking off dispatch so polls don't 404.
    {
        let mut intents = state.intents.write().await;
        intents.insert(
            sidecar_intent_id.clone(),
            StoredIntent {
                sidecar_intent_id: sidecar_intent_id.clone(),
                protocol_slug: payload.protocol_slug.clone(),
                src_chain_id: payload.src_chain_id,
                dst_chain_id: payload.dst_chain_id,
                dst_kind: payload.dst_kind.clone(),
                src_token: payload.src_token.clone(),
                dst_token: payload.dst_token.clone(),
                input_amount: payload.input_amount.clone(),
                output_amount: payload.output_amount.clone(),
                recipient: payload.recipient.clone(),
                dry_run_requested: payload.dry_run,
                dry_run_effective: payload.dry_run || !state.config.live_fills_unlocked(),
                status: "pending".into(),
                fill_tx: None,
                fill_block: None,
                gas_used: None,
                value_routed_usd: None,
                v5: None,
                error: None,
                created_at: now,
                updated_at: now,
            },
        );
    }

    // Synchronous dispatch — keep it simple; arc-api can poll meanwhile.
    let result = dispatch(&state, &payload, &sidecar_intent_id).await;

    {
        let mut intents = state.intents.write().await;
        match result {
            Ok(stored) => {
                info!(id = %stored.sidecar_intent_id, status = %stored.status, "dispatch ok");
                intents.insert(sidecar_intent_id.clone(), stored.clone());
                (
                    StatusCode::OK,
                    Json(IntentAccepted {
                        accepted: true,
                        sidecar_intent_id: sidecar_intent_id.clone(),
                        dry_run_effective: stored.dry_run_effective,
                    }),
                )
                    .into_response()
            }
            Err(e) => {
                let msg = e.to_string();
                warn!(error = %msg, "dispatch error");
                if let Some(prev) = intents.get_mut(&sidecar_intent_id) {
                    prev.status = "errored".into();
                    prev.error = Some(msg.clone());
                    prev.updated_at = Utc::now();
                }
                (
                    StatusCode::BAD_GATEWAY,
                    Json(ErrorBody { error: msg }),
                )
                    .into_response()
            }
        }
    }
}

async fn get_intent(
    State(state): State<SidecarState>,
    Path(id): Path<String>,
) -> Response {
    let intents = state.intents.read().await;
    match intents.get(&id) {
        Some(stored) => Json(stored.clone()).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: format!("no intent {id}"),
            }),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::SidecarConfig;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use tower::ServiceExt;

    fn test_state(sim: bool, live_ok: bool) -> SidecarState {
        SidecarState::new(SidecarConfig {
            port: 0,
            simulation_mode: sim,
            live_fill_ok: live_ok,
            network: "test".into(),
            spinner_api_url: "http://localhost:0".into(),
        })
    }

    #[tokio::test]
    async fn live_fill_locked_returns_423() {
        let app = router(test_state(true, false));
        let body = r#"{
            "protocol_slug":"across","src_chain_id":10,"dst_chain_id":8453,
            "src_token":"0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
            "dst_token":"0x833589fcd6edb6e08f4c7c32d4f71b54bda02913",
            "input_amount":"1000000","recipient":"0x000000000000000000000000000000000000dEaD",
            "dry_run": false
        }"#;
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sidecar/intent")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::LOCKED);
        let bytes = to_bytes(resp.into_body(), 4096).await.unwrap();
        let err: ErrorBody = serde_json::from_slice(&bytes).unwrap();
        assert!(err.error.contains("LIVE_FILL_OK"));
    }

    #[tokio::test]
    async fn health_reports_simulation_mode() {
        let app = router(test_state(true, false));
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/sidecar/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["status"], "ok");
        assert_eq!(json["simulation_mode"], true);
    }

    #[tokio::test]
    async fn missing_intent_returns_404() {
        let app = router(test_state(true, false));
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/sidecar/intent/sc-does-not-exist")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
