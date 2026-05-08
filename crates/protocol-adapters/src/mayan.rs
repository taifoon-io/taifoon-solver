//! Mayan Finance (Swift) Adapter
//!
//! ## Lifecycle
//! 1. User creates swap order → OrderCreated
//! 2. Solver auction (10-900s window)
//! 3. Solver fills order via fulfill()
//! 4. Settlement: Mayan protocol releases escrowed input tokens
//!
//! ## Reward: Auction model (3 bps protocol fee), solver keeps (input - output - 0.03%)

use super::*;
use alloy::primitives::Address;

pub struct MayanAdapter {
    spinner_client: SpinnerClient,
    mayan_addresses: std::collections::HashMap<u64, Address>,
}

impl Clone for MayanAdapter {
    fn clone(&self) -> Self {
        Self {
            spinner_client: SpinnerClient::new(self.spinner_client.base_url.clone()),
            mayan_addresses: self.mayan_addresses.clone(),
        }
    }
}

impl MayanAdapter {
    pub fn new(spinner_client: SpinnerClient) -> Self {
        let mut mayan_addresses = std::collections::HashMap::new();
        let addr: Address = "0x337685fdab40d39bd02028545a4ffa7d287cc3e2".parse().unwrap();
        mayan_addresses.insert(1, addr); // Ethereum
        mayan_addresses.insert(10, addr); // Optimism
        mayan_addresses.insert(42161, addr); // Arbitrum
        mayan_addresses.insert(8453, addr); // Base
        Self { spinner_client, mayan_addresses }
    }
}

#[async_trait::async_trait]
impl ProtocolAdapter for MayanAdapter {
    fn protocol_name(&self) -> &str {
        "mayan_finance"
    }

    fn can_handle(&self, intent: &Intent) -> bool {
        intent.protocol.to_lowercase().contains("mayan")
    }

    async fn estimate_gas(&self, intent: &Intent, spinner_api: &str) -> Result<GasEstimate> {
        let mayan = self.mayan_addresses.get(&intent.dst_chain)
            .ok_or_else(|| anyhow!("No Mayan on chain {}", intent.dst_chain))?;
        let client = SpinnerClient::new(spinner_api);
        client.estimate_gas(intent, &mayan.to_string(), "MayanSwift").await
    }

    async fn build_fill_tx(&self, intent: &Intent, _proof: &V5ProofBlob) -> Result<FillTransaction> {
        let mayan = self.mayan_addresses.get(&intent.dst_chain)
            .ok_or_else(|| anyhow!("No Mayan on chain {}", intent.dst_chain))?;
        Ok(FillTransaction {
            to: mayan.to_string(),
            data: format!("0x{}", hex::encode(b"fulfill()")), // Placeholder calldata
            value: None,
            chain_id: intent.dst_chain,
            estimated_gas: None,
        })
    }

    async fn execute_fill(&self, intent: &Intent, fill_tx: FillTransaction, dry_run: bool) -> Result<FillResult> {
        if dry_run {
            tracing::info!("✅ [SIMULATION] Mayan fulfill would be executed on chain {}", fill_tx.chain_id);
            return Ok(FillResult {
                tx_hash: format!("0xsim_mayan_{}", intent.id),
                gas_used: 200_000,
                block_number: 0,
                success: true,
                simulated: true,
            });
        }
        Err(anyhow!("Live Mayan execution not yet implemented - use SIMULATION_MODE=true"))
    }

    async fn claim_funds(&self, intent: &Intent, fill_result: &FillResult) -> Result<ClaimResult> {
        if fill_result.simulated {
            tracing::info!("✅ [SIMULATION] Mayan settlement would occur automatically");
            return Ok(ClaimResult {
                tx_hash: format!("0xsim_mayan_claim_{}", intent.id),
                claimed_amount: intent.amount.clone(),
                claimed_token: intent.src_token.clone(),
            });
        }
        tracing::info!("ℹ️  Mayan Swift settlement is automatic after fulfill");
        Ok(ClaimResult {
            tx_hash: "0x0".to_string(),
            claimed_amount: intent.amount.clone(),
            claimed_token: intent.src_token.clone(),
        })
    }
}
