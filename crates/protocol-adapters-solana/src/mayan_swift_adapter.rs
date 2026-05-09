use anyhow::Result;
use async_trait::async_trait;
use genome_client::Intent;

use crate::adapter_trait::SolanaFillAdapter;
use crate::mayan_solana::{MayanSolanaIntent, MayanSolanaSimulator};
use crate::send::SolanaBroadcaster;
use crate::simulate::SolanaEstimateOutcome;

pub struct MayanSwiftSolanaAdapter {
    simulator: MayanSolanaSimulator,
    rpc_url: String,
}

impl MayanSwiftSolanaAdapter {
    pub fn new(payer_pubkey_b58: &str, rpc_url: &str) -> Self {
        Self {
            simulator: MayanSolanaSimulator::new(payer_pubkey_b58, rpc_url),
            rpc_url: rpc_url.to_string(),
        }
    }

    /// Build from environment. Reads `SOLANA_PRIVATE_KEY` to derive the payer pubkey.
    pub fn from_env(rpc_url: &str) -> Result<Self> {
        // Build a broadcaster just to derive the pubkey; we keep rpc_url for later broadcasts.
        let broadcaster = SolanaBroadcaster::from_env(rpc_url)?;
        let pubkey = broadcaster.pubkey_b58();
        Ok(Self::new(&pubkey, rpc_url))
    }
}

#[async_trait]
impl SolanaFillAdapter for MayanSwiftSolanaAdapter {
    fn protocol_tag(&self) -> &'static str {
        "mayan_swift"
    }

    async fn simulate(&self, intent: &Intent) -> Result<SolanaEstimateOutcome> {
        let mayan_intent = MayanSolanaIntent::from_intent(intent)?;
        Ok(self.simulator.estimate(&mayan_intent).await)
    }

    async fn broadcast(&self, intent: &Intent) -> Result<String> {
        let mayan_intent = MayanSolanaIntent::from_intent(intent)?;
        let broadcaster = SolanaBroadcaster::from_env(&self.rpc_url)?;
        let result = broadcaster.send_fulfill(&mayan_intent).await?;
        Ok(result.signature)
    }
}
