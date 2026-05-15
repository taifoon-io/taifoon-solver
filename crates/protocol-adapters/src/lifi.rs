//! LI.FI Protocol Adapter
//!
//! ## Overview
//! LI.FI is a cross-chain bridge aggregator that routes through multiple underlying
//! protocols (Across, Stargate, Hop, Connext, etc.) to find the best route for
//! cross-chain swaps. It's a meta-protocol that doesn't have its own bridge infrastructure.
//!
//! ## Lifecycle
//! ```text
//! 1. User initiates cross-chain swap via LI.FI
//!    ├─ LI.FI API finds best route (may use Across, Stargate, etc.)
//!    ├─ User calls LI.FI Diamond contract
//!    ├─ Event: LiFiTransferStarted (transactionId, bridge, receiver, amount)
//!    └─ Funds locked and routed to underlying protocol
//!
//! 2. Solver/Relayer fulfills on destination
//!    ├─ Depends on underlying protocol used
//!    ├─ For Across: fillV3Relay()
//!    ├─ For Stargate: swap()
//!    └─ Event: LiFiTransferCompleted
//!
//! 3. Settlement
//!    ├─ Handled by underlying protocol
//!    ├─ LI.FI tracks status but doesn't manage settlement
//!    └─ Solver gets paid by underlying protocol's mechanism
//! ```
//!
//! ## Reward Mechanism
//! - **Model**: Depends on underlying protocol (Across, Stargate, etc.)
//! - **Earning**: LI.FI doesn't add fees; profit from underlying protocol
//! - **Payment**: Via underlying protocol's settlement mechanism
//! - **Expected Margin**: 5-50 bps depending on route
//!
//! ## Integration Strategy
//! Since LI.FI is an aggregator, solvers need to:
//! 1. Detect LI.FI transaction
//! 2. Identify underlying protocol from event data
//! 3. Route to appropriate protocol adapter (Across, Stargate, etc.)
//! 4. Execute fill using underlying protocol's adapter
//!
//! ## Double-Spend Protection
//! ✅ Protocol-Level (delegated to underlying protocol)
//! - LI.FI uses unique transactionId per swap
//! - Actual double-spend protection handled by underlying protocol
//! - Multiple solvers CAN attempt fill on underlying protocol

use super::*;
use alloy::primitives::Address;
use alloy::sol;

// ── LI.FI Diamond Contract ABI ────────────────────────────────────────────────

sol! {
    /// LI.FI Diamond contract interface
    interface LiFiDiamond {
        struct BridgeData {
            bytes32 transactionId;
            string bridge;
            string integrator;
            address referrer;
            address sendingAssetId;
            address receiver;
            uint256 minAmount;
            uint256 destinationChainId;
            bool hasSourceSwaps;
            bool hasDestinationCall;
        }

        /// Event emitted when transfer starts
        event LiFiTransferStarted(
            bytes32 indexed transactionId,
            string bridge,
            string bridgeData,
            address indexed receiver,
            address sendingAssetId,
            uint256 amount,
            uint256 destinationChainId
        );

        /// Event emitted when transfer completes
        event LiFiTransferCompleted(
            bytes32 indexed transactionId,
            address indexed receiver,
            address receivingAssetId,
            uint256 amount,
            uint256 timestamp
        );
    }
}

// ── LI.FI Adapter Implementation ──────────────────────────────────────────────

/// LI.FI protocol adapter
///
/// Note: This adapter primarily routes to underlying protocol adapters.
/// LI.FI itself doesn't have a direct fulfillment mechanism - it delegates
/// to protocols like Across, Stargate, Hop, etc.
pub struct LiFiAdapter {
    spinner_client: SpinnerClient,
    lifi_diamond_addresses: std::collections::HashMap<u64, Address>,
}

impl Clone for LiFiAdapter {
    fn clone(&self) -> Self {
        Self {
            spinner_client: SpinnerClient::new(self.spinner_client.base_url.clone()),
            lifi_diamond_addresses: self.lifi_diamond_addresses.clone(),
        }
    }
}

impl LiFiAdapter {
    pub fn new(spinner_client: SpinnerClient) -> Self {
        let mut lifi_diamond_addresses = std::collections::HashMap::new();

        // LI.FI Diamond proxy addresses (v1.0.0+)
        // These are the main entry points for LI.FI cross-chain swaps
        let diamond: Address = "0x1231DEB6f5749EF6cE6943a275A1D3E7486F4EaE".parse().unwrap();

        lifi_diamond_addresses.insert(1, diamond);       // Ethereum
        lifi_diamond_addresses.insert(10, diamond);      // Optimism
        lifi_diamond_addresses.insert(56, diamond);      // BSC
        lifi_diamond_addresses.insert(137, diamond);     // Polygon
        lifi_diamond_addresses.insert(42161, diamond);   // Arbitrum
        lifi_diamond_addresses.insert(8453, diamond);    // Base
        lifi_diamond_addresses.insert(43114, diamond);   // Avalanche
        lifi_diamond_addresses.insert(59144, diamond);   // Linea
        lifi_diamond_addresses.insert(100, diamond);     // Gnosis
        lifi_diamond_addresses.insert(324, diamond);         // zkSync Era
        lifi_diamond_addresses.insert(534352, diamond);      // Scroll
        lifi_diamond_addresses.insert(81457, diamond);       // Blast
        lifi_diamond_addresses.insert(1101, diamond);        // Polygon zkEVM
        lifi_diamond_addresses.insert(5000, diamond);        // Mantle
        lifi_diamond_addresses.insert(250, diamond);         // Fantom
        lifi_diamond_addresses.insert(13371, diamond);       // Immutable zkEVM
        lifi_diamond_addresses.insert(1666600000, diamond);  // Harmony
        lifi_diamond_addresses.insert(25, diamond);          // Cronos
        lifi_diamond_addresses.insert(1313161554, diamond);  // Aurora
        lifi_diamond_addresses.insert(480, diamond);         // World Chain
        lifi_diamond_addresses.insert(42220, diamond);       // Celo
        lifi_diamond_addresses.insert(252, diamond);         // Fraxtal
        lifi_diamond_addresses.insert(30, diamond);          // Rootstock
        lifi_diamond_addresses.insert(1284, diamond);        // Moonbeam
        lifi_diamond_addresses.insert(1285, diamond);        // Moonriver
        lifi_diamond_addresses.insert(1135, diamond);        // Lisk
        lifi_diamond_addresses.insert(196, diamond);         // X Layer
        lifi_diamond_addresses.insert(34443, diamond);       // Mode
        lifi_diamond_addresses.insert(204, diamond);         // opBNB
        lifi_diamond_addresses.insert(1329, diamond);        // Sei
        lifi_diamond_addresses.insert(42170, diamond);       // Arbitrum Nova
        lifi_diamond_addresses.insert(288, diamond);         // Boba
        lifi_diamond_addresses.insert(1625, diamond);        // Gravity
        lifi_diamond_addresses.insert(146, diamond);         // Sonic

        Self {
            spinner_client,
            lifi_diamond_addresses,
        }
    }

    /// Extract underlying bridge protocol from LI.FI transaction
    /// This would parse the event data to determine if it's using Across, Stargate, etc.
    fn extract_bridge_protocol(&self, _intent: &Intent) -> Result<String> {
        // In a real implementation, this would parse the LiFiTransferStarted event
        // to get the "bridge" field which indicates the underlying protocol

        // For now, return a placeholder
        // TODO: Parse intent.decoded JSON to extract bridge name
        Ok("across".to_string())
    }
}

#[async_trait::async_trait]
impl ProtocolAdapter for LiFiAdapter {
    fn protocol_name(&self) -> &str {
        "lifi"
    }

    fn can_handle(&self, intent: &Intent) -> bool {
        let proto_lower = intent.protocol.to_lowercase();
        proto_lower.contains("lifi") || proto_lower.contains("li.fi")
    }

    async fn estimate_gas(&self, intent: &Intent, spinner_api: &str) -> Result<GasEstimate> {
        let diamond = self.lifi_diamond_addresses.get(&intent.dst_chain)
            .ok_or_else(|| anyhow!("No LI.FI Diamond on chain {}", intent.dst_chain))?;

        tracing::info!(
            "🎯 LI.FI adapter estimating gas for chain {} (Diamond: {})",
            intent.dst_chain,
            diamond
        );

        // Note: LI.FI gas estimation should ideally route to underlying protocol
        // For now, we estimate against the Diamond contract directly
        let client = SpinnerClient::new(spinner_api);
        let gas_estimate = client
            .estimate_gas(intent, &diamond.to_string(), "LiFiDiamond")
            .await?;

        Ok(gas_estimate)
    }

    async fn build_fill_tx(&self, intent: &Intent, _proof: &V5ProofBlob) -> Result<FillTransaction> {
        // LI.FI fills are complex because they depend on the underlying protocol
        // In production, this should:
        // 1. Parse the LiFiTransferStarted event to get bridge name
        // 2. Route to the appropriate underlying protocol adapter
        // 3. Build fill tx using that adapter

        // For now, return a placeholder indicating the underlying protocol
        let bridge_protocol = self.extract_bridge_protocol(intent)?;

        tracing::info!(
            "🔀 LI.FI transaction uses underlying bridge: {}",
            bridge_protocol
        );

        let diamond = self.lifi_diamond_addresses.get(&intent.dst_chain)
            .ok_or_else(|| anyhow!("No LI.FI Diamond on chain {}", intent.dst_chain))?;

        // Placeholder transaction - in practice, this should delegate to underlying protocol
        Ok(FillTransaction {
            to: diamond.to_string(),
            data: format!("0x{}", hex::encode(b"lifi_delegate_to_underlying")),
            value: None,
            chain_id: intent.dst_chain,
            estimated_gas: None,
        })
    }

    async fn execute_fill(
        &self,
        intent: &Intent,
        fill_tx: FillTransaction,
        dry_run: bool,
    ) -> Result<FillResult> {
        if dry_run {
            tracing::info!("✅ [SIMULATION] LI.FI fill would delegate to underlying protocol");
            tracing::info!("   Note: Actual filling happens via bridge like Across or Stargate");
            tracing::info!("   To: {}", fill_tx.to);
            tracing::info!("   Chain: {}", fill_tx.chain_id);

            return Ok(FillResult {
                tx_hash: format!("0xsim_lifi_{}", intent.id),
                gas_used: 200_000, // Estimate
                block_number: 0,
                success: true,
                simulated: true,
            });
        }

        Err(anyhow!(
            "Live LI.FI execution not yet implemented - use SIMULATION_MODE=true. \
             LI.FI requires routing to underlying protocol adapters."
        ))
    }

    async fn claim_funds(&self, intent: &Intent, fill_result: &FillResult) -> Result<ClaimResult> {
        if fill_result.simulated {
            tracing::info!("✅ [SIMULATION] LI.FI settlement delegated to underlying protocol");
            return Ok(ClaimResult {
                tx_hash: format!("0xsim_lifi_claim_{}", intent.id),
                claimed_amount: intent.amount.clone(),
                claimed_token: intent.src_token.clone(),
            });
        }

        tracing::info!(
            "ℹ️  LI.FI settlement is handled by underlying bridge protocol"
        );
        tracing::info!("   Fill tx: {}", fill_result.tx_hash);
        tracing::info!(
            "   Claim mechanism depends on which bridge was used (Across, Stargate, etc.)"
        );

        Ok(ClaimResult {
            tx_hash: "0x0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            claimed_amount: intent.amount.clone(),
            claimed_token: intent.src_token.clone(),
        })
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_can_handle_lifi() {
        let spinner_client = SpinnerClient::new("http://localhost:8081");
        let adapter = LiFiAdapter::new(spinner_client);

        let mut intent = Intent {
            id: "lifi:test_123".to_string(),
            protocol: "lifi".to_string(),
            src_chain: 1,
            dst_chain: 42161,
            src_token: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string(),
            dst_token: "0xaf88d065e77c8cC2239327C5EDb3A432268e5831".to_string(),
            amount: "1000000".to_string(),
            depositor: "0xuser".to_string(),
            recipient: "0xuser".to_string(),
            tx_hash: "0xabc".to_string(),
            detected_at: 0,
            ..Default::default()
        };

        assert!(adapter.can_handle(&intent));

        // Test case-insensitive
        intent.protocol = "LIFI".to_string();
        assert!(adapter.can_handle(&intent));

        intent.protocol = "li.fi".to_string();
        assert!(adapter.can_handle(&intent));

        intent.protocol = "LI.FI".to_string();
        assert!(adapter.can_handle(&intent));
    }

    #[tokio::test]
    async fn test_build_fill_tx() {
        let spinner_client = SpinnerClient::new("http://localhost:8081");
        let adapter = LiFiAdapter::new(spinner_client);

        let intent = Intent {
            id: "lifi:test_123".to_string(),
            protocol: "lifi".to_string(),
            src_chain: 1,
            dst_chain: 42161,
            src_token: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string(),
            dst_token: "0xaf88d065e77c8cC2239327C5EDb3A432268e5831".to_string(),
            amount: "1000000".to_string(),
            depositor: "0x1234567890123456789012345678901234567890".to_string(),
            recipient: "0x1234567890123456789012345678901234567890".to_string(),
            tx_hash: "0xabc123".to_string(),
            detected_at: 0,
            ..Default::default()
        };

        let proof = V5ProofBlob {
            l1_superroot: L1SuperRoot {
                hash: "0xroot".to_string(),
                timestamp: 0,
                chains_included: vec![1, 42161],
            },
            l2_chain_header: L2ChainHeader {
                chain_id: 1,
                block_number: 1000,
                block_hash: "0xblock".to_string(),
                parent_hash: "0xparent".to_string(),
                state_root: "0xstate".to_string(),
                timestamp: 0,
            },
            l3_superroot_proof: vec![],
            l4_block_proof: vec![],
            l5_chain_event: L5ChainEvent {
                tx_hash: "0xtx".to_string(),
                tx_index: 0,
                log_index: Some(0),
                encoded_tx: "0x".to_string(),
                encoded_receipt: "0x".to_string(),
            },
            l6_finality: L6FinalityCommitment {
                finality_type: "ETH_POS_CHECKPOINT".to_string(),
                commitment_data: "{}".to_string(),
            },
        };

        let fill_tx = adapter.build_fill_tx(&intent, &proof).await.unwrap();

        assert_eq!(fill_tx.chain_id, 42161);
        assert_eq!(fill_tx.to, "0x1231DEB6f5749EF6cE6943a275A1D3E7486F4EaE");
        assert!(fill_tx.data.starts_with("0x"));
    }
}
