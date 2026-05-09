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
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::net::TcpListener;
use tracing::info;
use chrono;

use alloy::{
    network::EthereumWallet,
    primitives::{Address, U256},
    providers::{Provider, ProviderBuilder},
    sol_types::SolCall,
};
use t3rn_sidecar::{
    deposit_all::deposit_all,
    gas_razor,
    hop_rebalancer::HopRebalancer,
    load_deployments,
    order_monitor::OrderMonitor,
    self_fill::SelfFill,
};
use portfolio_sidecar::lwc_manager::LwcManager;
use portfolio_sidecar::lwc_manager::LiquidityWellCompact;
use t3rn_sidecar::fills_log::FillsLog;

#[derive(Clone)]
struct AppState {
    fill_engine:    Arc<SelfFill>,
    hop_rebalancer: Arc<HopRebalancer>,
    lwc_manager:    Arc<LwcManager>,
    solver_addr:    alloy::primitives::Address,
    signer:         alloy::signers::local::PrivateKeySigner,
    dry_run:        bool,
    deployments:    Vec<t3rn_sidecar::LwcDeployment>,
    fills_db:       String,
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

    // Prominent DRY_RUN warning — operators forget this and wonder why nothing fills
    if dry_run {
        tracing::warn!("╔══════════════════════════════════════════════════════╗");
        tracing::warn!("║  DRY_RUN MODE ENABLED — no fills will be broadcast  ║");
        tracing::warn!("║  Set DRY_RUN=false to enable live fills              ║");
        tracing::warn!("╚══════════════════════════════════════════════════════╝");
    }

    let deployments = load_deployments();
    if deployments.is_empty() {
        if dry_run {
            tracing::warn!("[t3rn-solver] No LWC deployments loaded — set LWC_DEPLOYMENTS_PATH");
        } else {
            anyhow::bail!(
                "No LWC deployments loaded and DRY_RUN=false — cannot operate. \
                 Set LWC_DEPLOYMENTS_PATH to a valid lwc_deployments.json"
            );
        }
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

    // Fills log — writes every fill to SQLite + mirrors to rpc.taifoon.dev
    let fills_db = std::env::var("OUTCOME_DB_PATH")
        .unwrap_or_else(|_| "sidecar.db".to_string());
    let fills_log = match FillsLog::open(&fills_db) {
        Ok(l) => {
            info!("[t3rn-solver] fills log: {}", fills_db);
            Some(l)
        }
        Err(e) => {
            tracing::warn!("[t3rn-solver] fills log unavailable: {} — fills won't be persisted", e);
            None
        }
    };

    // Order monitor + self-fill
    let (monitor, rx) = OrderMonitor::new();
    monitor.start(deployments.clone());

    let fill_engine = SelfFill::with_log(signer.clone(), rx, fills_log);
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
        signer: signer.clone(),
        dry_run,
        deployments: deployments.clone(),
        fills_db: fills_db.clone(),
    };

    let router = Router::new()
        .route("/t3rn/status",      get(status_handler))
        .route("/t3rn/hops",        get(hops_handler))
        .route("/t3rn/fills",       get(fills_handler))
        .route("/t3rn/deposit-all", post(deposit_all_handler))
        .route("/t3rn/test-fill",   post(test_fill_handler))
        .layer(tower_http::cors::CorsLayer::permissive())
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

    // fills_last_hour: DB-backed so it survives restarts
    let fills_last_hour = {
        let db_path = s.fills_db.clone();
        let fallback = s.fill_engine.fills_count();
        tokio::task::spawn_blocking(move || -> u64 {
            let Ok(conn) = rusqlite::Connection::open(&db_path) else { return fallback };
            let cutoff = chrono::Utc::now()
                .checked_sub_signed(chrono::Duration::hours(1))
                .map(|t| t.to_rfc3339())
                .unwrap_or_default();
            conn.query_row(
                "SELECT COUNT(*) FROM solver_outcomes WHERE decision='executed' AND ts >= ?1",
                rusqlite::params![cutoff],
                |row| row.get::<_, u64>(0),
            ).unwrap_or(fallback)
        }).await.unwrap_or(s.fill_engine.fills_count())
    };

    Json(json!({
        "chains": chain_list,
        "fills_last_hour": fills_last_hour,
        "fills_since_restart": s.fill_engine.fills_count(),
        "hops_total": s.hop_rebalancer.hops_total(),
        "dry_run": dry_run,
    }))
}

async fn hops_handler(State(s): State<AppState>) -> Json<serde_json::Value> {
    let hops = s.hop_rebalancer.recent_hops(50);
    Json(json!({ "hops": hops }))
}

/// GET /t3rn/fills?limit=N&solver_id=0x...
#[derive(Debug, Default, Deserialize, Serialize)]
struct FillsQuery {
    limit: Option<i64>,
    solver_id: Option<String>,
}

async fn fills_handler(
    State(s): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<FillsQuery>,
) -> Json<serde_json::Value> {
    let limit = q.limit.unwrap_or(100).clamp(1, 1000);
    let db_path = s.fills_db.clone();
    let solver_id = q.solver_id.clone();

    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<serde_json::Value>> {
        let conn = rusqlite::Connection::open(&db_path)?;
        let sql = if solver_id.is_some() {
            "SELECT ts, intent_id, protocol, src_chain, dst_chain, decision, tx_hash,
                    predicted_gas, gas_used, effective_gas_price_wei,
                    actual_profit_usd, skip_reason, error, solver_id
             FROM solver_outcomes
             WHERE solver_id = ?1
             ORDER BY ts DESC LIMIT ?2"
        } else {
            "SELECT ts, intent_id, protocol, src_chain, dst_chain, decision, tx_hash,
                    predicted_gas, gas_used, effective_gas_price_wei,
                    actual_profit_usd, skip_reason, error, solver_id
             FROM solver_outcomes
             ORDER BY ts DESC LIMIT ?2"
        };
        let mut stmt = conn.prepare(sql)?;
        let rows: Vec<serde_json::Value> = if let Some(ref sid) = solver_id {
            stmt.query_map(rusqlite::params![sid, limit], row_to_json)?
                .filter_map(|r| r.ok())
                .collect()
        } else {
            stmt.query_map(rusqlite::params![rusqlite::types::Null, limit], row_to_json)?
                .filter_map(|r| r.ok())
                .collect()
        };
        Ok(rows)
    }).await;

    match result {
        Ok(Ok(rows)) => { let n = rows.len(); Json(json!({ "fills": rows, "count": n })) }
        Ok(Err(e))   => Json(json!({ "error": e.to_string(), "fills": [] })),
        Err(e)       => Json(json!({ "error": e.to_string(), "fills": [] })),
    }
}

fn row_to_json(r: &rusqlite::Row<'_>) -> rusqlite::Result<serde_json::Value> {
    Ok(json!({
        "ts":                       r.get::<_, String>(0)?,
        "intent_id":                r.get::<_, String>(1)?,
        "protocol":                 r.get::<_, String>(2)?,
        "src_chain":                r.get::<_, i64>(3)?,
        "dst_chain":                r.get::<_, i64>(4)?,
        "decision":                 r.get::<_, String>(5)?,
        "tx_hash":                  r.get::<_, Option<String>>(6)?,
        "predicted_gas":            r.get::<_, Option<i64>>(7)?,
        "gas_used":                 r.get::<_, Option<i64>>(8)?,
        "effective_gas_price_wei":  r.get::<_, Option<String>>(9)?,
        "actual_profit_usd":        r.get::<_, Option<f64>>(10)?,
        "skip_reason":              r.get::<_, Option<String>>(11)?,
        "error":                    r.get::<_, Option<String>>(12)?,
        "solver_id":                r.get::<_, Option<String>>(13)?,
    }))
}

async fn deposit_all_handler(State(s): State<AppState>) -> Json<serde_json::Value> {
    let results = deposit_all(&s.deployments, &s.lwc_manager, s.solver_addr, s.dry_run).await;
    let total_usd: f64 = results.iter().filter(|r| !r.skipped).map(|r| r.amount_usd).sum();
    Json(json!({
        "total_deposited_usd": total_usd,
        "results": results,
    }))
}

/// Optional overrides for the Base→Optimism test fill.
#[derive(Debug, Default, Deserialize, Serialize)]
struct TestFillRequest {
    /// Amount in USDC base units (6 decimals). Default 1_000_000 = $1.
    amount_usdc: Option<u64>,
    /// Max reward in USDC base units. Default 1_100_000 = $1.10.
    max_reward_usdc: Option<u64>,
}

/// POST /t3rn/test-fill
///
/// Submits a real `order()` on Base LWC (chain 8453) with destination = "optm"
/// (Optimism, chain 10). The self_fill engine will detect the resulting
/// OrderCreated event and fill it on Optimism — creating a closed Base→OP loop.
async fn test_fill_handler(
    State(s): State<AppState>,
    body: Option<axum::extract::Json<TestFillRequest>>,
) -> Json<serde_json::Value> {
    let req = body.map(|b| b.0).unwrap_or_default();

    // Base LWC constants
    const BASE_CHAIN_ID: u64 = 8453;
    const BASE_WELL: &str   = "0xb590266eCdbc389A35831dDc672Ea0C5f45500EF";
    const BASE_USDC: &str   = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";
    const BASE_RPC: &str    = "https://base-rpc.publicnode.com";

    let amount: U256 = U256::from(req.amount_usdc.unwrap_or(1_000_000u64));
    let max_reward: U256 = U256::from(req.max_reward_usdc.unwrap_or(1_100_000u64));

    // Encode target account as bytes32 (solver's own address, left-padded)
    let mut target_bytes = [0u8; 32];
    target_bytes[12..].copy_from_slice(s.solver_addr.as_slice());
    let target_account: alloy::primitives::FixedBytes<32> = target_bytes.into();

    let well: Address = match BASE_WELL.parse() {
        Ok(a) => a,
        Err(e) => return Json(json!({ "error": format!("invalid well addr: {e}") })),
    };
    let usdc: Address = match BASE_USDC.parse() {
        Ok(a) => a,
        Err(e) => return Json(json!({ "error": format!("invalid usdc addr: {e}") })),
    };

    // Get USDC asset_id on Base
    let rpc_url: reqwest::Url = match BASE_RPC.parse() {
        Ok(u) => u,
        Err(e) => return Json(json!({ "error": format!("invalid rpc url: {e}") })),
    };
    let read_provider = ProviderBuilder::new().on_http(rpc_url.clone());
    let id_call = LiquidityWellCompact::mapAssetToIdCall { _asset: usdc };
    let id_req = alloy::rpc::types::TransactionRequest::default()
        .to(well)
        .input(id_call.abi_encode().into());
    let asset_id: u32 = match read_provider.call(&id_req).await {
        Ok(bytes) if bytes.len() >= 32 => {
            alloy::primitives::U256::from_be_slice(&bytes[bytes.len()-32..])
                .try_into().unwrap_or(1)
        }
        _ => 1,
    };

    // Build order() calldata: destination = "optm" (Optimism)
    let dest: alloy::primitives::FixedBytes<4> = alloy::primitives::FixedBytes(*b"optm");
    let order_call = LiquidityWellCompact::orderCall {
        destination: dest,
        asset: asset_id,
        targetAccount: target_account,
        amount,
        rewardAsset: usdc,
        insurance: U256::ZERO,
        maxReward: max_reward,
    };
    let calldata: alloy::primitives::Bytes = order_call.abi_encode().into();

    if s.dry_run {
        return Json(json!({
            "dry_run": true,
            "message": "DRY_RUN — would submit order() on Base → Optimism",
            "base_well": BASE_WELL,
            "destination": "optm",
            "amount_usdc": req.amount_usdc.unwrap_or(1_000_000),
            "max_reward_usdc": req.max_reward_usdc.unwrap_or(1_100_000),
            "asset_id": asset_id,
        }));
    }

    // Estimate gas
    let gas_params = gas_razor::estimate(BASE_CHAIN_ID, calldata.clone(), well).await;

    // Guard: destination must be the Base LWC well
    let guard = t3rn_sidecar::TxGuard::from_deployments(s.solver_addr);
    if let Err(e) = guard.enforce(well, &calldata, &[s.solver_addr]) {
        return Json(json!({ "error": format!("tx_guard: {e}") }));
    }

    let wallet = EthereumWallet::from(s.signer.clone());
    let provider = ProviderBuilder::new()
        .wallet(wallet)
        .on_http(rpc_url);

    let tx_req = alloy::rpc::types::TransactionRequest::default()
        .to(well)
        .input(calldata.into())
        .gas_limit(gas_params.gas_limit)
        .max_fee_per_gas(gas_params.max_fee_per_gas)
        .max_priority_fee_per_gas(gas_params.priority_fee);

    match provider.send_transaction(tx_req).await {
        Err(e) => Json(json!({ "error": format!("send_transaction: {e}") })),
        Ok(pending) => {
            match pending.get_receipt().await {
                Err(e) => Json(json!({ "error": format!("get_receipt: {e}") })),
                Ok(receipt) => {
                    let tx_hash = format!("{:#x}", receipt.transaction_hash);
                    if !receipt.status() {
                        return Json(json!({ "error": format!("order() reverted on-chain: tx={tx_hash}") }));
                    }
                    info!(
                        "[test_fill] Base→Optimism order submitted: tx={} gas_used={}",
                        tx_hash, receipt.gas_used
                    );
                    Json(json!({
                        "tx_hash": tx_hash,
                        "gas_used": receipt.gas_used,
                        "base_well": BASE_WELL,
                        "destination": "optm",
                        "amount_usdc": req.amount_usdc.unwrap_or(1_000_000),
                        "max_reward_usdc": req.max_reward_usdc.unwrap_or(1_100_000),
                        "status": "submitted — self_fill will pick up OrderCreated on Base and fill on Optimism",
                    }))
                }
            }
        }
    }
}
