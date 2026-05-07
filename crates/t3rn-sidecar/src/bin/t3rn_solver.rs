//! Standalone t3rn solver binary (port 8092).
//!
//! Wires OrderMonitor → SelfFill, HopRebalancer, and an HTTP status endpoint.
//!
//! Environment variables:
//!   SOLVER_PRIVATE_KEY     — hex key (no 0x prefix or with)
//!   DRY_RUN                — "true" to skip broadcasts (default: true)
//!   T3RN_SOLVER_PORT       — HTTP port (default: 8092)
//!   LWC_HOP_INTERVAL_SECS  — hop rebalancer tick interval (default: 300)
//!   LWC_DEPLOYMENTS_PATH   — path to lwc_deployments.json
//!   LWC_MAX_HOP_USD        — max USD per hop (default: 5000)
//!   LWC_LOW_THRESHOLD_USD  — deficit threshold USD (default: 100)

use std::sync::Arc;
use axum::{
    extract::State,
    response::Json,
    routing::{get, post},
    Router,
};
use serde_json::json;
use tokio::net::TcpListener;
use tracing::info;

use t3rn_sidecar::{
    deposit_all::deposit_all,
    hop_rebalancer::HopRebalancer,
    load_deployments,
    order_monitor::OrderMonitor,
    self_fill::SelfFill,
};
use portfolio_sidecar::lwc_manager::LwcManager;

#[derive(Clone)]
struct AppState {
    fill_engine:    Arc<SelfFill>,
    hop_rebalancer: Arc<HopRebalancer>,
    lwc_manager:    Arc<LwcManager>,
    solver_addr:    alloy::primitives::Address,
    dry_run:        bool,
    deployments:    Vec<t3rn_sidecar::LwcDeployment>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("t3rn_sidecar=info".parse()?)
                .add_directive("t3rn_solver=info".parse()?)
        )
        .init();

    let raw_key = std::env::var("SOLVER_PRIVATE_KEY")
        .expect("SOLVER_PRIVATE_KEY must be set");
    let signer: alloy::signers::local::PrivateKeySigner = raw_key
        .trim_start_matches("0x")
        .parse()
        .expect("invalid SOLVER_PRIVATE_KEY");

    let dry_run = std::env::var("DRY_RUN")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(true);

    let deployments = load_deployments();
    if deployments.is_empty() {
        tracing::warn!("No LWC deployments loaded — set LWC_DEPLOYMENTS_PATH");
    }

    let solver_addr = signer.address();
    let lwc_manager = Arc::new(LwcManager::new(signer.clone(), dry_run));

    // ── Startup deposit sweep ─────────────────────────────────────────────────
    // Deposit all solver stables into LWC wells before starting the event loop.
    info!("[t3rn-solver] startup: depositing all solver stables into LWC wells...");
    let deposit_results = deposit_all(&deployments, &lwc_manager, solver_addr, dry_run).await;
    let deposited_usd: f64 = deposit_results.iter()
        .filter(|r| !r.skipped)
        .map(|r| r.amount_usd)
        .sum();
    info!(
        "[t3rn-solver] startup deposit complete: ${:.2} deposited across {} chains",
        deposited_usd,
        deposit_results.iter().filter(|r| !r.skipped).count()
    );
    // ─────────────────────────────────────────────────────────────────────────

    // Order monitor + self-fill
    let (monitor, rx) = OrderMonitor::new();
    monitor.start(deployments.clone());

    let fill_engine = SelfFill::new(signer.clone(), rx);
    fill_engine.clone().start(monitor.subscribe());

    // Hop rebalancer
    let hop_rebalancer = HopRebalancer::new(lwc_manager.clone(), signer.clone());
    let hop_interval: u64 = std::env::var("LWC_HOP_INTERVAL_SECS")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(300);
    hop_rebalancer.clone().start(hop_interval);

    let state = AppState {
        fill_engine,
        hop_rebalancer,
        lwc_manager,
        solver_addr,
        dry_run,
        deployments: deployments.clone(),
    };

    let router = Router::new()
        .route("/t3rn/status",      get(status_handler))
        .route("/t3rn/hops",        get(hops_handler))
        .route("/t3rn/deposit-all", post(deposit_all_handler))
        .with_state(state);

    let port: u16 = std::env::var("T3RN_SOLVER_PORT")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(8092);
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    info!("t3rn-solver listening on 0.0.0.0:{}", port);

    axum::serve(listener, router).await?;
    Ok(())
}

async fn status_handler(State(s): State<AppState>) -> Json<serde_json::Value> {
    let chains = s.lwc_manager.scan_all().await;
    let dry_run = std::env::var("DRY_RUN")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(true);

    let chain_list: Vec<serde_json::Value> = chains.iter().map(|c| {
        json!({
            "chain_id": c.chain_id,
            "chain_key": c.chain_key,
            "available_usd": c.pool_available_usd,
            "status": format!("{:?}", c.status()),
        })
    }).collect();

    Json(json!({
        "chains": chain_list,
        "fills_last_hour": s.fill_engine.fills_count(),
        "hops_total": s.hop_rebalancer.hops_total(),
        "dry_run": dry_run,
    }))
}

async fn hops_handler(State(s): State<AppState>) -> Json<serde_json::Value> {
    let hops = s.hop_rebalancer.recent_hops(50);
    Json(json!({ "hops": hops }))
}

async fn deposit_all_handler(State(s): State<AppState>) -> Json<serde_json::Value> {
    let results = deposit_all(&s.deployments, &s.lwc_manager, s.solver_addr, s.dry_run).await;
    let total_usd: f64 = results.iter().filter(|r| !r.skipped).map(|r| r.amount_usd).sum();
    Json(json!({
        "total_deposited_usd": total_usd,
        "results": results,
    }))
}
