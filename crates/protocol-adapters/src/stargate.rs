//! Stargate V2 Adapter (LayerZero-based)
//!
//! ## Note: Stargate is pool-based, not pure intent
//! This adapter provides a stub for future LayerZero-based filling opportunities

use super::*;
use alloy::primitives::Address;

pub struct StargateAdapter {
    spinner_client: SpinnerClient,
}

impl Clone for StargateAdapter {
    fn clone(&self) -> Self {
        Self {
            spinner_client: SpinnerClient::new(self.spinner_client.base_url.clone()),
        }
    }
}

impl StargateAdapter {
    pub fn new(spinner_client: SpinnerClient) -> Self {
        Self { spinner_client }
    }
}

#[async_trait::async_trait]
impl ProtocolAdapter for StargateAdapter {
    fn protocol_name(&self) -> &str {
        "stargate_v2"
    }

    fn can_handle(&self, intent: &Intent) -> bool {
        intent.protocol.to_lowercase().contains("stargate")
    }

    async fn estimate_gas(&self, _intent: &Intent, _spinner_api: &str) -> Result<GasEstimate> {
        Err(anyhow!("Stargate adapter not yet implemented - pool-based protocol"))
    }

    async fn build_fill_tx(&self, _intent: &Intent, _proof: &V5ProofBlob) -> Result<FillTransaction> {
        Err(anyhow!("Stargate adapter not yet implemented - pool-based protocol"))
    }

    async fn execute_fill(&self, _intent: &Intent, _fill_tx: FillTransaction, _dry_run: bool) -> Result<FillResult> {
        Err(anyhow!("Stargate adapter not yet implemented - pool-based protocol"))
    }

    async fn claim_funds(&self, _intent: &Intent, _fill_result: &FillResult) -> Result<ClaimResult> {
        Err(anyhow!("Stargate adapter not yet implemented - pool-based protocol"))
    }
}
