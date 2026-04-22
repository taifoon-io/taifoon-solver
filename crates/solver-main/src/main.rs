use anyhow::Result;
use genome_client::GenomeClient;
use profit_calc::ProfitCalculator;
use tokio::sync::mpsc;
use tracing::{error, info};

const GENOME_SSE_URL: &str = "https://api.taifoon.dev/api/genome/subscribe/sse";
const MIN_PROFIT_USD: f64 = 1.0;
const SOLVER_INTEL_PATH: &str = "config/solver_intel.json";

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
    info!("⏳ Waiting for intents...");

    // Main solver loop
    while let Some(intent) = intent_rx.recv().await {
        info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        info!("📥 Intent: {} ({})", intent.id, intent.protocol);
        info!("   {} → {}", intent.src_chain, intent.dst_chain);
        info!("   Amount: {} {}", intent.amount, intent.src_token);
        info!("   User: {} → {}", intent.depositor, intent.recipient);

        // Calculate profitability
        match profit_calc.calculate(&intent).await {
            Ok(profit_result) => {
                info!("💵 Profit: ${:.2}", profit_result.net_profit_usd);
                info!("   Protocol Fee: ${:.2}", profit_result.breakdown.protocol_fee_usd);
                info!("   Spread: ${:.2}", profit_result.breakdown.spread_usd);
                info!("   Gas Cost: ${:.2}", profit_result.breakdown.gas_cost_usd);

                if profit_result.profitable {
                    info!("✅ PROFITABLE - Would execute (executor not yet implemented)");
                    // TODO: executor.execute_fill(&intent).await
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
