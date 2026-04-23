use anyhow::Result;
use genome_client::GenomeClient;
use profit_calc::ProfitCalculator;
use solver_api::{SolverApi, SolverEvent, IntentData, AttemptData};
use tokio::sync::mpsc;
use tracing::{error, info};
use chrono::Utc;

const GENOME_SSE_URL: &str = "https://api.taifoon.dev/api/genome/subscribe/sse";
const MIN_PROFIT_USD: f64 = 1.0;
const SOLVER_INTEL_PATH: &str = "config/solver_intel.json";
const API_PORT: u16 = 8082;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_thread_ids(false)
        .with_file(false)
        .init();

    info!("🚀 Taifoon Solver Starting...");
    info!("📡 Genome SSE: {}", GENOME_SSE_URL);
    info!("💰 Min Profit: ${}", MIN_PROFIT_USD);
    info!("🌐 API Port: {}", API_PORT);

    // Initialize solver API
    let solver_api = SolverApi::new();
    let api_router = solver_api.router();

    // Spawn API server in background
    let api_handle = tokio::spawn(async move {
        let listener = match tokio::net::TcpListener::bind(format!("0.0.0.0:{}", API_PORT)).await {
            Ok(listener) => listener,
            Err(e) => {
                error!("Failed to bind API server to port {}: {}", API_PORT, e);
                return;
            }
        };
        info!("✅ API server listening on port {}", API_PORT);
        if let Err(e) = axum::serve(listener, api_router).await {
            error!("API server error: {}", e);
        }
    });

    // Initialize components
    let genome_client = GenomeClient::new(GENOME_SSE_URL);
    let mut profit_calc = ProfitCalculator::new(MIN_PROFIT_USD);

    // Load solver intel
    if let Err(e) = profit_calc.load_solver_intel(SOLVER_INTEL_PATH) {
        error!("⚠️  Failed to load solver intel: {}", e);
        info!("   Continuing with default 10 bps fee for unknown protocols");
    }

    // let executor = Executor::new(); // TODO: Enable when ready

    // Create intent channel
    let (intent_tx, mut intent_rx) = mpsc::channel(100);

    // Start genome stream consumer in background
    let genome_handle = tokio::spawn(async move {
        if let Err(e) = genome_client.subscribe(intent_tx).await {
            error!("Genome stream error: {}", e);
        }
    });

    info!("✅ Genome stream consumer started");
    info!("✅ Solver API started");
    info!("⏳ Waiting for intents...");

    // Main solver loop
    while let Some(intent) = intent_rx.recv().await {
        info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        info!("📥 Intent: {} ({})", intent.id, intent.protocol);
        info!("   {} → {}", intent.src_chain, intent.dst_chain);
        info!("   Amount: {} {}", intent.amount, intent.src_token);
        info!("   User: {} → {}", intent.depositor, intent.recipient);

        // Emit: Intent detected
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

        // Calculate profitability
        match profit_calc.calculate(&intent).await {
            Ok(profit_result) => {
                info!("💵 Profit: ${:.2}", profit_result.net_profit_usd);
                info!("   Protocol Fee: ${:.2}", profit_result.breakdown.protocol_fee_usd);
                info!("   Spread: ${:.2}", profit_result.breakdown.spread_usd);
                info!("   Gas Cost: ${:.2}", profit_result.breakdown.gas_cost_usd);

                // Emit: Intent attempted
                solver_api.emit_event(SolverEvent::IntentAttempted(AttemptData {
                    id: intent.id.clone(),
                    profitable: profit_result.profitable,
                    profit_usd: profit_result.net_profit_usd,
                    protocol_fee_usd: profit_result.breakdown.protocol_fee_usd,
                    gas_cost_usd: profit_result.breakdown.gas_cost_usd,
                    decision: if profit_result.profitable { "execute".to_string() } else { "skip".to_string() },
                }));

                if profit_result.profitable {
                    info!("✅ PROFITABLE - Would execute (executor not yet implemented)");
                    // TODO: executor.execute_fill(&intent).await
                    // TODO: Emit SolverEvent::IntentSolved when execution is implemented
                } else {
                    info!("⏭️  SKIP - Below ${} threshold", MIN_PROFIT_USD);
                }
            }
            Err(e) => {
                error!("❌ Profit calculation failed: {}", e);
            }
        }
    }

    // Wait for genome stream (should run forever)
    genome_handle.await?;

    Ok(())
}
