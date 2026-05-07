//! taifoon-sandbox — local solver competition environment.
//!
//! Usage:
//!   taifoon-sandbox compete --solvers 3 --intents fixtures/... --wells config/lwc_deployments.json --duration 60
//!   taifoon-sandbox serve --port 8090 --intents fixtures/...  (HTTP API + SSE replay)

// Re-export library modules at crate root so api.rs can use `crate::well_sim` etc.
pub use solver_sandbox::compete_sim;
pub use solver_sandbox::genome_replay;
pub use solver_sandbox::well_sim;

mod api;

use compete_sim::{CompeteSim, SimConfig};
use genome_replay::ReplayState;
use well_sim::WellSimulator;

use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    let args: Vec<String> = std::env::args().collect();
    let subcommand = args.get(1).map(String::as_str).unwrap_or("compete");

    match subcommand {
        "compete" => run_compete(&args[2..]).await,
        "serve"   => run_serve(&args[2..]).await,
        other => {
            eprintln!("Unknown subcommand: {}. Use: compete | serve", other);
            std::process::exit(1);
        }
    }
}

async fn run_compete(args: &[String]) -> anyhow::Result<()> {
    let mut solver_count = 3usize;
    let mut intents_path = String::from("fixtures/genome_snapshot.ndjson");
    let mut duration_secs = 60u64;
    let mut speed = 10.0f64;
    let mut budget = 1000.0f64;
    let mut well_seed = 2000.0f64;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--solvers"  => { i += 1; solver_count = args[i].parse().unwrap_or(3); }
            "--intents"  => { i += 1; intents_path = args[i].clone(); }
            "--duration" => { i += 1; duration_secs = args[i].parse().unwrap_or(60); }
            "--speed"    => { i += 1; speed = args[i].parse().unwrap_or(10.0); }
            "--budget"   => { i += 1; budget = args[i].parse().unwrap_or(1000.0); }
            "--well-seed"=> { i += 1; well_seed = args[i].parse().unwrap_or(2000.0); }
            other => { eprintln!("Unknown arg: {}", other); }
        }
        i += 1;
    }

    let replay = if std::path::Path::new(&intents_path).exists() {
        ReplayState::from_ndjson(&intents_path, speed, false)?
    } else {
        info!("Intents file not found — using 50 synthetic intents");
        ReplayState::from_synthetic(50)
    };

    let config = SimConfig {
        solver_count,
        budget_per_solver_usd: budget,
        well_seed_usd: well_seed,
        well_chains: vec![8453, 42161, 10],
        duration_secs,
        speed_multiplier: speed,
    };

    info!("🏁 Starting CompeteSim: {} solvers, {} intents, {}s budget=${:.0} well_seed=${:.0}",
        solver_count, replay.events.len(), duration_secs, budget, well_seed);

    let sim = CompeteSim::new(config, replay.events);
    let lb = sim.run().await;

    println!("\n{}", serde_json::to_string_pretty(&lb)?);
    info!("🏆 Simulation complete. Total fills: {}", lb.total_fills);
    Ok(())
}

async fn run_serve(args: &[String]) -> anyhow::Result<()> {
    let mut port = 8090u16;
    let mut intents_path = String::from("fixtures/genome_snapshot.ndjson");
    let mut speed = 1.0f64;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--port"    => { i += 1; port = args[i].parse().unwrap_or(8090); }
            "--intents" => { i += 1; intents_path = args[i].clone(); }
            "--speed"   => { i += 1; speed = args[i].parse().unwrap_or(1.0); }
            other => { eprintln!("Unknown arg: {}", other); }
        }
        i += 1;
    }

    let replay = if std::path::Path::new(&intents_path).exists() {
        ReplayState::from_ndjson(&intents_path, speed, true)?
    } else {
        info!("Intents file not found — using 100 synthetic intents (looping)");
        let mut r = ReplayState::from_synthetic(100);
        r.loop_playback = true;
        r.speed_multiplier = speed;
        r
    };

    let well = Arc::new(Mutex::new(WellSimulator::new()));
    let events = Arc::new(Mutex::new(replay.events.clone()));
    let leaderboard = Arc::new(Mutex::new(None));

    let shared_replay = Arc::new(tokio::sync::RwLock::new(replay));

    let api_state = api::ApiState {
        well: well.clone(),
        events: events.clone(),
        leaderboard,
    };

    let genome_router = genome_replay::replay_router(shared_replay);
    let api_router = api::api_router(api_state);

    let app = axum::Router::new()
        .merge(genome_router)
        .merge(api_router)
        .layer(tower_http::cors::CorsLayer::permissive());

    let addr = format!("0.0.0.0:{}", port);
    info!("🌐 taifoon-sandbox serving on http://{}", addr);
    info!("   Genome SSE: http://{}/api/genome/subscribe/sse", addr);
    info!("   Well API:   http://{}/sandbox/wells", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
