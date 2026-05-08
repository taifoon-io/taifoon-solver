//! Across Protocol V3 Adapter
//!
//! ## Lifecycle (from PROTOCOL_SOLVER_INTEGRATION_RESEARCH.md)
//!
//! ```text
//! 1. User calls deposit() on source chain
//!    ├─ Event: FilledV3Relay emitted
//!    ├─ Input token + amount locked
//!    └─ Destination chain + recipient specified
//!
//! 2. Relayer (solver) monitors deposits via genome SSE
//!    ├─ Fetches V5 proof bundle from Spinner
//!    ├─ Calls fillV3Relay() on destination chain
//!    ├─ Pays out output token to recipient
//!    └─ Event: FilledV3Relay emitted
//!
//! 3. Settlement (UMA optimistic oracle)
//!    ├─ Relayer submits proof bundle after finality (~60-120s)
//!    ├─ 2-hour challenge window
//!    └─ Relayer reimbursed + fee on source chain
//! ```
//!
//! ## Reward Mechanism
//! - **Model**: Dynamic fee (0 bps base, market-driven)
//! - **Earning**: Relayer receives `outputAmount - inputAmount + realizedLpFeePct`
//! - **Payment**: On source chain after settlement
//! - **Expected Margin**: 5-50 bps depending on route liquidity
//!
//! ## Double-Spend Protection
//! ✅ Protocol-Level (No solver responsibility)
//! - Each deposit has unique `depositId` (hash of deposit params + nonce)
//! - Fill can only happen once per `depositId` (contract enforces)
//! - Multiple solvers CAN attempt fill, but only first confirmed tx wins

use super::*;
use alloy::primitives::{Address, U256, Bytes};
use alloy::sol;
use alloy::sol_types::SolCall;

// ── Across V3 Contract ABIs ───────────────────────────────────────────────────

sol! {
    /// Across V3 SpokePool contract interface — `depositId` is `int64` to
    /// match the deployed Taifoon AcrossAdapter (`taifoon-eco/contracts/adapters/AcrossAdapter.sol`).
    interface SpokePoolV3 {
        struct V3RelayData {
            address depositor;
            address recipient;
            address exclusiveRelayer;
            address inputToken;
            address outputToken;
            uint256 inputAmount;
            uint256 outputAmount;
            uint256 originChainId;
            int64  depositId;
            uint32 fillDeadline;
            uint32 exclusivityDeadline;
            bytes  message;
        }

        function fillV3Relay(
            V3RelayData calldata relayData,
            uint256 repaymentChainId
        ) external;

        event FilledV3Relay(
            address indexed depositor,
            address indexed recipient,
            address indexed exclusiveRelayer,
            address inputToken,
            address outputToken,
            uint256 inputAmount,
            uint256 outputAmount,
            uint256 originChainId,
            int64  depositId,
            uint32 fillDeadline,
            uint32 exclusivityDeadline,
            bytes  message,
            address relayer,
            uint256 repaymentChainId
        );
    }
}

// ── Across Adapter Implementation ─────────────────────────────────────────────

/// Across V3 protocol adapter
pub struct AcrossAdapter {
    spinner_client: SpinnerClient,
    spoke_pool_addresses: std::collections::HashMap<u64, Address>, // chain_id → SpokePool address
}

impl Clone for AcrossAdapter {
    fn clone(&self) -> Self {
        Self {
            spinner_client: SpinnerClient::new(self.spinner_client.base_url.clone()),
            spoke_pool_addresses: self.spoke_pool_addresses.clone(),
        }
    }
}

impl AcrossAdapter {
    pub fn new(spinner_client: SpinnerClient) -> Self {
        let mut spoke_pool_addresses = std::collections::HashMap::new();

        // Across V3 SpokePool addresses (production addresses from Across docs)
        spoke_pool_addresses.insert(
            1, // Ethereum
            "0x5c7BCd6E7De5423a257D81B442095A1a6ced35C5".parse().unwrap(),
        );
        spoke_pool_addresses.insert(
            10, // Optimism
            "0x6f26Bf09B1C792e3228e5467807a900A503c0281".parse().unwrap(),
        );
        spoke_pool_addresses.insert(
            42161, // Arbitrum
            "0xe35e9842fceaCA96570B734083f4a58e8F7C5f2A".parse().unwrap(),
        );
        spoke_pool_addresses.insert(
            8453, // Base
            "0x09aea4b2242abC8bb4BB78D537A67a245A7bEC64".parse().unwrap(),
        );
        spoke_pool_addresses.insert(
            137, // Polygon
            "0x9295ee1d8C5b022Be115A2AD3c30C72E34e7F096".parse().unwrap(),
        );
        spoke_pool_addresses.insert(
            59144, // Linea
            "0x7e63a5f1a8F0B4D0934B2f2327DAEd3f6bb2Ee75".parse().unwrap(),
        );
        spoke_pool_addresses.insert(
            324, // zkSync Era
            "0xe0B015E54d54fc84a6cB9B666099c46adE9335FF".parse().unwrap(),
        );
        spoke_pool_addresses.insert(
            534352, // Scroll
            "0x3baD7AD0728f9917d1Bf08af5782dCbD516cDd96".parse().unwrap(),
        );
        spoke_pool_addresses.insert(
            57073, // Ink
            "0xeF684C38F94F48775959ECf2012D7E864ffb9dd4".parse().unwrap(),
        );
        spoke_pool_addresses.insert(
            34443, // Mode
            "0x3baD7AD0728f9917d1Bf08af5782dCbD516cDd96".parse().unwrap(),
        );

        Self {
            spinner_client,
            spoke_pool_addresses,
        }
    }

    /// Parse V3RelayData from genome intent
    fn parse_relay_data(&self, intent: &Intent) -> Result<SpokePoolV3::V3RelayData> {
        let input_amount = U256::from_str_radix(&intent.amount, 10)?;
        let output_amount = match intent.output_amount.as_deref() {
            Some(s) => U256::from_str_radix(s, 10)?,
            None => input_amount,
        };

        Ok(SpokePoolV3::V3RelayData {
            depositor: intent.depositor.parse()?,
            recipient: intent.recipient.parse()?,
            exclusiveRelayer: Address::ZERO,
            inputToken: intent.src_token.parse()?,
            outputToken: intent.dst_token.parse()?,
            inputAmount: input_amount,
            outputAmount: output_amount,
            originChainId: U256::from(intent.src_chain),
            depositId: self.extract_deposit_id(intent)?,
            fillDeadline: intent.fill_deadline.unwrap_or_else(|| {
                (std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
                    + 3600) as u32
            }),
            exclusivityDeadline: 0,
            message: Bytes::new(),
        })
    }

    /// Extract depositId — prefers `intent.deposit_id` (plumbed from the
    /// genome event payload) and falls back to a trailing-int parse of `intent.id`
    /// for legacy events.
    fn extract_deposit_id(&self, intent: &Intent) -> Result<i64> {
        if let Some(id) = intent.deposit_id {
            return Ok(id);
        }
        let parts: Vec<&str> = intent.id.split(&[':', '_'][..]).collect();
        let deposit_id_str = parts
            .last()
            .ok_or_else(|| anyhow!("Failed to extract depositId from intent ID: {}", intent.id))?;
        deposit_id_str
            .parse::<i64>()
            .map_err(|e| anyhow!("Failed to parse depositId '{}': {}", deposit_id_str, e))
    }
}

#[async_trait::async_trait]
impl ProtocolAdapter for AcrossAdapter {
    fn protocol_name(&self) -> &str {
        "across_v3"
    }

    fn can_handle(&self, intent: &Intent) -> bool {
        let proto_lower = intent.protocol.to_lowercase();
        proto_lower.contains("across")
    }

    async fn estimate_gas(&self, intent: &Intent, spinner_api: &str) -> Result<GasEstimate> {
        // Get SpokePool address for destination chain
        let spoke_pool = self
            .spoke_pool_addresses
            .get(&intent.dst_chain)
            .ok_or_else(|| {
                anyhow!(
                    "No Across SpokePool deployed on chain {}",
                    intent.dst_chain
                )
            })?;

        tracing::info!(
            "🎯 Across adapter estimating gas for chain {} (SpokePool: {})",
            intent.dst_chain,
            spoke_pool
        );

        // Use Spinner API to estimate gas
        let client = SpinnerClient::new(spinner_api);
        let gas_estimate = client
            .estimate_gas(
                intent,
                &spoke_pool.to_string(),
                "AcrossV3SpokePool",
            )
            .await?;

        Ok(gas_estimate)
    }

    async fn build_fill_tx(&self, intent: &Intent, _proof: &V5ProofBlob) -> Result<FillTransaction> {
        // Get SpokePool address
        let spoke_pool = self
            .spoke_pool_addresses
            .get(&intent.dst_chain)
            .ok_or_else(|| {
                anyhow!(
                    "No Across SpokePool deployed on chain {}",
                    intent.dst_chain
                )
            })?;

        // Parse relay data
        let relay_data = self.parse_relay_data(intent)?;

        // Encode fillV3Relay call
        let repayment_chain_id = U256::from(intent.src_chain); // Get reimbursed on source chain
        let call = SpokePoolV3::fillV3RelayCall {
            relayData: relay_data,
            repaymentChainId: repayment_chain_id,
        };

        let calldata = call.abi_encode();

        Ok(FillTransaction {
            to: spoke_pool.to_string(),
            data: format!("0x{}", hex::encode(&calldata)),
            value: None, // No ETH value needed for ERC20 fills
            chain_id: intent.dst_chain,
            estimated_gas: None, // Will be filled by .estimateGas()
        })
    }

    async fn execute_fill(
        &self,
        intent: &Intent,
        fill_tx: FillTransaction,
        dry_run: bool,
    ) -> Result<FillResult> {
        if dry_run {
            tracing::info!("✅ [SIMULATION] Across fillV3Relay would be executed:");
            tracing::info!("   To: {}", fill_tx.to);
            tracing::info!("   Chain: {}", fill_tx.chain_id);
            tracing::info!("   Calldata: {}...{}", &fill_tx.data[..20], &fill_tx.data[fill_tx.data.len()-20..]);

            return Ok(FillResult {
                tx_hash: format!("0xsim_across_fill_{}", intent.id),
                gas_used: fill_tx.estimated_gas.unwrap_or(150_000),
                block_number: 0,
                success: true,
                simulated: true,
            });
        }

        // TODO: Actual transaction broadcast via alloy provider
        Err(anyhow!("Live Across fill execution not yet implemented - use SIMULATION_MODE=true"))
    }

    async fn claim_funds(&self, intent: &Intent, fill_result: &FillResult) -> Result<ClaimResult> {
        if fill_result.simulated {
            tracing::info!("✅ [SIMULATION] Across settlement would occur after ~2h challenge window");
            return Ok(ClaimResult {
                tx_hash: format!("0xsim_across_claim_{}", intent.id),
                claimed_amount: intent.amount.clone(),
                claimed_token: intent.src_token.clone(),
            });
        }

        // NOTE: Across settlement is automatic after challenge window
        // Relayer submits proof bundle via Across API, not directly on-chain
        tracing::info!("ℹ️  Across settlement is handled by Across backend after 2h challenge window");
        tracing::info!("   Fill tx: {}", fill_result.tx_hash);
        tracing::info!("   Relayer will be automatically reimbursed on chain {}", intent.src_chain);

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
    fn test_extract_deposit_id() {
        let spinner_client = SpinnerClient::new("http://localhost:8081");
        let adapter = AcrossAdapter::new(spinner_client);

        let mut intent = Intent {
            id: "across:across_v3::12345".to_string(),
            protocol: "across_v3".to_string(),
            src_chain: 1,
            dst_chain: 42161,
            src_token: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string(),
            dst_token: "0xaf88d065e77c8cC2239327C5EDb3A432268e5831".to_string(),
            amount: "100000000".to_string(),
            depositor: "0xuser".to_string(),
            recipient: "0xuser".to_string(),
            tx_hash: "0xabc".to_string(),
            detected_at: 0,
            ..Default::default()
        };

        let deposit_id = adapter.extract_deposit_id(&intent).unwrap();
        assert_eq!(deposit_id, 12345);

        // Test alternate format
        intent.id = "across_v3_1_42161_67890".to_string();
        let deposit_id2 = adapter.extract_deposit_id(&intent).unwrap();
        assert_eq!(deposit_id2, 67890);
    }

    #[tokio::test]
    async fn test_build_fill_tx() {
        let spinner_client = SpinnerClient::new("http://localhost:8081");
        let adapter = AcrossAdapter::new(spinner_client);

        let intent = Intent {
            id: "across:across_v3::12345".to_string(),
            protocol: "across_v3".to_string(),
            src_chain: 1,
            dst_chain: 42161,
            src_token: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string(),
            dst_token: "0xaf88d065e77c8cC2239327C5EDb3A432268e5831".to_string(),
            amount: "100000000".to_string(), // 100 USDC
            depositor: "0x1234567890123456789012345678901234567890".to_string(),
            recipient: "0x1234567890123456789012345678901234567890".to_string(),
            tx_hash: "0xabc123".to_string(),
            detected_at: 0,
            deposit_id: Some(12345),
            output_amount: Some("99850000".to_string()),
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
        assert!(fill_tx.data.starts_with("0x"));
        assert!(fill_tx.data.len() > 10); // Should have encoded calldata
    }
}
