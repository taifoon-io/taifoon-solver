use anyhow::{Result, anyhow};
use genome_client::Intent;
use profit_calc::ProfitResult;
use t3rn_sidecar::T3RNSidecar;
use protocol_adapters::AdapterFactory;
use alloy::signers::local::PrivateKeySigner;
use tracing::{info, warn};

pub mod across_executor;
pub mod estimate;
pub mod evm_estimate;
pub mod lambda_controller;
pub mod lifi_meta_router;
pub mod mayan_evm_estimate;
pub mod mayan_solana_estimate;
pub mod outcome_log;
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
    T3RNSidecar, // Priority 3: Backup liquidity
}

/// Intent executor with multi-source liquidity
pub struct Executor {
    simulation_mode: bool,
    t3rn_sidecar: Option<T3RNSidecar>,
    min_profit_usd: f64,
    adapter_factory: AdapterFactory,
    spinner_api_url: String,
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

        // Initialize T3RN sidecar if enabled
        let t3rn_sidecar = if std::env::var("T3RN_LWC_ENABLED").unwrap_or_default() == "true" {
            match std::env::var("WALLET_PRIVATE_KEY") {
                Ok(pk) => {
                    let wallet: PrivateKeySigner = pk.parse()?;
                    Some(T3RNSidecar::new(wallet))
                }
                Err(_) => {
                    warn!("T3RN_LWC_ENABLED=true but WALLET_PRIVATE_KEY not set, disabling T3RN sidecar");
                    None
                }
            }
        } else {
            None
        };

        // Initialize protocol adapter factory
        let spinner_api_url = std::env::var("WARMBED_API_URL")
            .or_else(|_| std::env::var("SPINNER_API_URL"))
            .unwrap_or_else(|_| "https://api.taifoon.dev".to_string());

        let adapter_factory = AdapterFactory::new(&spinner_api_url);

        info!("🔧 Executor initialized:");
        info!("   SIMULATION_MODE: {}", simulation_mode);
        info!("   MIN_PROFIT_USD: ${}", min_profit_usd);
        info!("   T3RN_LWC: {}", if t3rn_sidecar.is_some() { "enabled" } else { "disabled" });
        info!("   SPINNER_API: {}", spinner_api_url);
        info!("   SUPPORTED_PROTOCOLS: {:?}", adapter_factory.supported_protocols());

        Ok(Self {
            simulation_mode,
            t3rn_sidecar,
            min_profit_usd,
            adapter_factory,
            spinner_api_url,
        })
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
            LiquiditySource::T3RNSidecar => self.execute_with_t3rn(intent, profit).await,
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

        // Priority 3: Fallback to T3RN LWC
        if let Some(ref t3rn) = self.t3rn_sidecar {
            let amount_wei: alloy::primitives::U256 = intent.amount.parse().unwrap_or_default();
            if t3rn.can_provide_liquidity(intent.dst_chain, amount_wei).await {
                return Ok(LiquiditySource::T3RNSidecar);
            }
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

    /// Execute with own funds (Priority 1) - USING PROTOCOL ADAPTERS
    async fn execute_with_own_funds(
        &self,
        intent: &Intent,
        profit: &ProfitResult,
    ) -> Result<ExecutionResult> {
        // Get protocol-specific adapter
        let adapter = self.adapter_factory.get_adapter(intent)?;

        info!("📦 Using protocol adapter: {}", adapter.protocol_name());

        // Fetch V5 proof bundle from Spinner
        info!("🔐 Fetching V5 proof bundle from Spinner...");
        let proof = adapter.estimate_gas(intent, &self.spinner_api_url).await?;
        info!("✅ Gas estimate: {} units @ {:.2} gwei = ${:.2}",
            proof.gas_units, proof.gas_price_gwei, proof.total_usd);

        if self.simulation_mode {
            info!("✅ [SIMULATION] Would execute protocol fill via {}", adapter.protocol_name());
            return Ok(ExecutionResult {
                intent_id: intent.id.clone(),
                fill_tx: format!("0xsim_{}_{}", adapter.protocol_name(), intent.id),
                claim_tx: None,
                gas_used: proof.gas_units,
                actual_profit_usd: profit.net_profit_usd,
            });
        }

        // TODO: Implement actual protocol-based execution
        Err(anyhow!("Live protocol execution not yet implemented - use SIMULATION_MODE=true"))
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

    /// Execute with T3RN LWC (Priority 3)
    async fn execute_with_t3rn(
        &self,
        intent: &Intent,
        profit: &ProfitResult,
    ) -> Result<ExecutionResult> {
        let t3rn = self.t3rn_sidecar.as_ref()
            .ok_or_else(|| anyhow!("T3RN sidecar not initialized"))?;

        if self.simulation_mode {
            info!("✅ [SIMULATION] Would execute with T3RN LWC: {}", intent.id);
            return Ok(ExecutionResult {
                intent_id: intent.id.clone(),
                fill_tx: format!("0xsim_t3rn_{}", intent.id),
                claim_tx: None,
                gas_used: 300_000,
                actual_profit_usd: profit.net_profit_usd * 0.90, // LWC fees + insurance
            });
        }

        // Create LWC order (not yet implemented)
        let _ = t3rn;

        // TODO: Wait for LWC to provide liquidity on destination
        // TODO: Execute fill transaction
        //
        // T3RN LWC fee distribution: fees are collected by the LiquidityWellCompact
        // contract on the destination chain and the protocol distributes the solver
        // reward to the signer address that called `order()`. No explicit claim call
        // is required — `T3RNSidecar` exposes `can_provide_liquidity` and `fill` only,
        // with no `claim`/`withdraw`/`settle` method. See README.md §"LWC Order Flow"
        // step 4 ("LWC automatically claims from source chain").

        Err(anyhow!("T3RN LWC execution not yet implemented"))
    }
}

impl Default for Executor {
    fn default() -> Self {
        Self::new().expect("Failed to initialize executor")
    }
}
