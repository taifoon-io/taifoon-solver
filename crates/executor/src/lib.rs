use anyhow::{Result, anyhow};
use genome_client::Intent;
use profit_calc::ProfitResult;
use protocol_adapters::AdapterFactory;
use std::sync::Arc;
use tracing::info;

pub mod across_executor;
pub mod estimate;
pub mod evm_estimate;
pub mod lambda_controller;
pub mod lifi_meta_router;
pub mod mayan_evm_estimate;
pub mod mayan_solana_estimate;
pub mod outcome_log;
pub mod router;
pub mod skip_rules;
pub mod spinner_solver;
pub mod wormhole;

pub use across_executor::{AcrossExecutor, AcrossExecutorConfig, ChainWiring, ChainWiringConfig};
pub use estimate::{
    classify_evm_error, classify_solana_error, write_attempt_bundle, AttemptBundle,
    EstimateAdapter, EstimateOutcome,
};
pub use evm_estimate::{
    default_across_spoke_pools, default_debridge_dln_addresses, default_rpc_for_chain,
    fetch_razor_gas_price_gwei, gas_price_range_for_chain, optimal_gas_price_wei,
    resolve_rpc_url, run_evm_estimate, run_evm_estimate_with_value,
    AcrossEstimateAdapter, DeBridgeEstimateAdapter,
};
pub use lambda_controller::{
    build_execute_with_proof_calldata, build_lambda_controller_from_env, claim_selector,
    intent_amount_usd, parse_chain_wiring_from_env, LambdaClaimOutcome, LambdaController,
    LambdaExecuteOutcome,
};
pub use lifi_meta_router::LiFiMetaRouter;
pub use mayan_evm_estimate::{default_mayan_swift_addresses, MayanEvmEstimateAdapter};
pub use mayan_solana_estimate::{
    default_solana_rpc, load_messiah_solana_pubkey_or_fallback, svm_to_parent,
    MayanSolanaEstimateAdapter, DEFAULT_SOLANA_RPC, FALLBACK_SOLANA_PAYER_PUBKEY,
};
pub use outcome_log::{OutcomeLog, OutcomeRecord, PnlSummary, ProtocolPnl};
pub use router::{AdapterRouter, FillRouter};
pub use skip_rules::{RulePredicate, SkipRule, SkipRules};
pub use spinner_solver::{SpinnerSolverClient, TestRunResult};

/// Execution result
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub intent_id: String,
    pub fill_tx: String,
    pub claim_tx: Option<String>,
    pub gas_used: u64,
    pub actual_profit_usd: f64,
}

/// Liquidity source priority
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiquiditySource {
    OwnFunds,    // Priority 1: Fastest, highest profit
    FlashLoan,   // Priority 2: No capital lockup
}

/// Intent executor.
///
/// All fill routing flows through `router`. The default constructor
/// installs an [`AdapterRouter`] so legacy installs behave identically to
/// the pre-router code path. solver-main can call [`Executor::with_router`]
/// at boot to swap in a Hand-aware router (typically a thin shim around
/// `trader::Trader` from the out-of-tree `taifoon-trade` workspace) — at
/// which point the per-protocol adapter dispatch in `protocol-adapters`
/// is bypassed in favour of unified Hand routing.
pub struct Executor {
    simulation_mode: bool,
    min_profit_usd: f64,
    router: Arc<dyn FillRouter>,
}

impl Executor {
    pub fn new() -> Result<Self> {
        // Load configuration from environment
        let simulation_mode = std::env::var("SIMULATION_MODE")
            .or_else(|_| std::env::var("DRY_RUN"))
            .unwrap_or_else(|_| "true".to_string())
            .parse()
            .unwrap_or(true);

        let min_profit_usd = std::env::var("MIN_PROFIT_USD")
            .unwrap_or_else(|_| "0.10".to_string())
            .parse()
            .unwrap_or(0.10);

        // Resolve the Spinner API URL the default `AdapterRouter` needs.
        // Same env-var precedence as before the router refactor.
        let spinner_api_url = std::env::var("WARMBED_API_URL")
            .or_else(|_| std::env::var("SPINNER_API_URL"))
            .unwrap_or_else(|_| "https://api.taifoon.dev".to_string());

        // Diagnostic only — the AdapterFactory is also held internally by
        // `AdapterRouter`, but constructing it here lets us log the list
        // of supported protocols at boot like we always have.
        let factory_for_logging = AdapterFactory::new(&spinner_api_url);

        info!("Executor initialized:");
        info!("   SIMULATION_MODE: {}", simulation_mode);
        info!("   MIN_PROFIT_USD: ${}", min_profit_usd);
        info!("   SPINNER_API: {}", spinner_api_url);
        info!(
            "   SUPPORTED_PROTOCOLS: {:?}",
            factory_for_logging.supported_protocols()
        );

        let router: Arc<dyn FillRouter> = Arc::new(AdapterRouter::new(spinner_api_url));
        info!("   ROUTER: {}", router.description());

        Ok(Self {
            simulation_mode,
            min_profit_usd,
            router,
        })
    }

    /// Swap the active `FillRouter`. solver-main (or any operator) calls
    /// this at boot when they want to bypass the legacy adapter dispatch
    /// — e.g. to install a Hand-aware router backed by `trader::Trader`.
    pub fn with_router(mut self, router: Arc<dyn FillRouter>) -> Self {
        info!("Executor router swapped: {}", router.description());
        self.router = router;
        self
    }

    /// Borrow the currently-installed router. Mostly for diagnostics +
    /// tests that want to assert which strategy is in use.
    pub fn router(&self) -> &Arc<dyn FillRouter> {
        &self.router
    }

    /// Execute an intent fill with liquidity waterfall
    pub async fn execute_fill(
        &self,
        intent: &Intent,
        profit: &ProfitResult,
    ) -> Result<ExecutionResult> {
        // Safety check: Minimum profit threshold
        if profit.net_profit_usd < self.min_profit_usd {
            return Err(anyhow!(
                "Profit ${:.2} below minimum ${:.2}",
                profit.net_profit_usd,
                self.min_profit_usd
            ));
        }

        info!("🎯 Executing fill for intent {} (${:.2} profit)", intent.id, profit.net_profit_usd);

        // Determine liquidity source
        let liquidity_source = self.select_liquidity_source(intent).await?;

        info!("💰 Using liquidity source: {:?}", liquidity_source);

        // Execute based on source
        match liquidity_source {
            LiquiditySource::OwnFunds => self.execute_with_own_funds(intent, profit).await,
            LiquiditySource::FlashLoan => self.execute_with_flash_loan(intent, profit).await,
        }
    }

    /// Select best available liquidity source
    async fn select_liquidity_source(&self, intent: &Intent) -> Result<LiquiditySource> {
        // Priority 1: Check if we have own funds
        if self.has_own_funds(intent).await? {
            return Ok(LiquiditySource::OwnFunds);
        }

        // Priority 2: Try flash loans
        if self.can_use_flash_loan(intent).await? {
            return Ok(LiquiditySource::FlashLoan);
        }

        Err(anyhow!("No liquidity source available for intent {}", intent.id))
    }

    /// Check if we have sufficient own funds
    async fn has_own_funds(&self, _intent: &Intent) -> Result<bool> {
        // Own-funds path is active when a solver key is configured.
        // Actual balance check happens inside the protocol adapter's estimate_gas.
        Ok(std::env::var("SOLVER_PRIVATE_KEY").is_ok())
    }

    /// Check if flash loan is available
    async fn can_use_flash_loan(&self, _intent: &Intent) -> Result<bool> {
        // TODO: Check Aave/Uniswap flash loan availability
        // For now, not implemented
        Ok(false)
    }

    /// Execute with own funds (Priority 1) — delegates to the installed
    /// [`FillRouter`]. The default router ([`AdapterRouter`]) preserves
    /// the legacy adapter-factory dispatch verbatim; a Hand-aware router
    /// installed via [`Executor::with_router`] takes over here.
    async fn execute_with_own_funds(
        &self,
        intent: &Intent,
        profit: &ProfitResult,
    ) -> Result<ExecutionResult> {
        self.router
            .route_and_execute(intent, profit, self.simulation_mode)
            .await
    }

    /// Execute with flash loan (Priority 2)
    async fn execute_with_flash_loan(
        &self,
        intent: &Intent,
        profit: &ProfitResult,
    ) -> Result<ExecutionResult> {
        if self.simulation_mode {
            info!("✅ [SIMULATION] Would execute with flash loan: {}", intent.id);
            return Ok(ExecutionResult {
                intent_id: intent.id.clone(),
                fill_tx: format!("0xsim_flashloan_{}", intent.id),
                claim_tx: None,
                gas_used: 250_000,
                actual_profit_usd: profit.net_profit_usd * 0.95, // Flash loan fee
            });
        }

        // TODO: Implement actual flash loan execution
        Err(anyhow!("Flash loan execution not yet implemented"))
    }

}

impl Default for Executor {
    fn default() -> Self {
        Self::new().expect("Failed to initialize executor")
    }
}
