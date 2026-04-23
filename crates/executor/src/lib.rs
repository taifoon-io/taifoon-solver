use anyhow::{Result, anyhow};
use genome_client::Intent;
use profit_calc::ProfitResult;
use t3rn_sidecar::T3RNSidecar;
use alloy::signers::local::PrivateKeySigner;
use tracing::{info, warn, error};

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
}

impl Executor {
    pub fn new() -> Result<Self> {
        // Load configuration from environment
        let simulation_mode = std::env::var("SIMULATION_MODE")
            .unwrap_or_else(|_| "true".to_string())
            .parse()
            .unwrap_or(true);

        let min_profit_usd = std::env::var("MIN_PROFIT_USD")
            .unwrap_or_else(|_| "1.0".to_string())
            .parse()
            .unwrap_or(1.0);

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

        info!("🔧 Executor initialized:");
        info!("   SIMULATION_MODE: {}", simulation_mode);
        info!("   MIN_PROFIT_USD: ${}", min_profit_usd);
        info!("   T3RN_LWC: {}", if t3rn_sidecar.is_some() { "enabled" } else { "disabled" });

        Ok(Self {
            simulation_mode,
            t3rn_sidecar,
            min_profit_usd,
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
            if t3rn.can_provide_liquidity(intent).await? {
                return Ok(LiquiditySource::T3RNSidecar);
            }
        }

        Err(anyhow!("No liquidity source available for intent {}", intent.id))
    }

    /// Check if we have sufficient own funds
    async fn has_own_funds(&self, _intent: &Intent) -> Result<bool> {
        // TODO: Check wallet balance on destination chain
        // For now, assume no own funds (forcing fallback to other sources)
        Ok(false)
    }

    /// Check if flash loan is available
    async fn can_use_flash_loan(&self, _intent: &Intent) -> Result<bool> {
        // TODO: Check Aave/Uniswap flash loan availability
        // For now, not implemented
        Ok(false)
    }

    /// Execute with own funds (Priority 1)
    async fn execute_with_own_funds(
        &self,
        intent: &Intent,
        profit: &ProfitResult,
    ) -> Result<ExecutionResult> {
        if self.simulation_mode {
            info!("✅ [SIMULATION] Would execute with own funds: {}", intent.id);
            return Ok(ExecutionResult {
                intent_id: intent.id.clone(),
                fill_tx: format!("0xsim_ownfunds_{}", intent.id),
                claim_tx: None,
                gas_used: 150_000,
                actual_profit_usd: profit.net_profit_usd,
            });
        }

        // TODO: Implement actual own-funds execution
        Err(anyhow!("Own funds execution not yet implemented"))
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

        // Create LWC order
        let lwc_order = t3rn.create_order(intent).await?;
        info!("📦 T3RN LWC order created: {}", lwc_order.order_id);

        // TODO: Wait for LWC to provide liquidity on destination
        // TODO: Execute fill transaction
        // TODO: LWC will automatically claim from source

        Ok(ExecutionResult {
            intent_id: intent.id.clone(),
            fill_tx: lwc_order.tx_hash,
            claim_tx: None,
            gas_used: 300_000,
            actual_profit_usd: profit.net_profit_usd * 0.90,
        })
    }
}

impl Default for Executor {
    fn default() -> Self {
        Self::new().expect("Failed to initialize executor")
    }
}
