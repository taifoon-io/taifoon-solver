use anyhow::Result;
use async_trait::async_trait;
use genome_client::Intent;

use crate::simulate::SolanaEstimateOutcome;

/// Unified interface for all Solana fill protocol adapters.
/// Implementors: MayanSwiftAdapter (wraps MayanSolanaSimulator + SolanaBroadcaster),
/// future: DlnSolanaAdapter, MayanFlashAdapter, RelayProtocolSolanaAdapter.
#[async_trait]
pub trait SolanaFillAdapter: Send + Sync {
    /// Protocol identifier tag used in logs and metrics.
    fn protocol_tag(&self) -> &'static str;

    /// Simulate the fill. Returns GREEN (OkComputeUnits or InsufficientLamports) or RED.
    /// A GREEN result means the calldata + program ABI is valid; payer funding is separate.
    async fn simulate(&self, intent: &Intent) -> Result<SolanaEstimateOutcome>;

    /// Fetch a recent blockhash, sign the transaction, broadcast via sendTransaction.
    /// Returns the base58 transaction signature on success.
    async fn broadcast(&self, intent: &Intent) -> Result<String>;
}
