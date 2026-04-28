//! deBridge DLN (Decentralized Liquidity Network) Adapter
//!
//! ## Lifecycle (from PROTOCOL_SOLVER_INTEGRATION_RESEARCH.md)
//!
//! ```text
//! 1. User submits cross-chain intent
//!    ├─ Calls DlnSource.createOrder()
//!    ├─ Event: OrderCreated (giveAmount, takeAmount, receiver, takeChainId)
//!    ├─ Tokens locked in source contract
//!    └─ Order broadcast to solver network
//!
//! 2. Solver fulfills order
//!    ├─ Monitors OrderCreated events
//!    ├─ Calls DlnDestination.fulfillOrder() on destination chain
//!    ├─ Provides takeAmount to receiver
//!    └─ Event: FulfilledOrder
//!
//! 3. Settlement
//!    ├─ Solver submits proof of fulfillment to source chain
//!    ├─ Claims giveAmount from escrow
//!    └─ Profit = giveAmount - takeAmount - gas
//! ```
//!
//! ## Reward Mechanism
//! - **Model**: Pure spread (0 bps protocol fee)
//! - **Earning**: Difference between giveAmount and takeAmount
//! - **Payment**: Claim giveTokens from source chain after proof submission
//! - **Expected Margin**: 10-80 bps
//!
//! ## Double-Spend Protection
//! ✅ Protocol-Level (No solver responsibility)
//! - Order has unique `orderId` (hash of params)
//! - Fulfillment can only occur once (state flag)
//! - Multiple solvers CAN attempt fill, but only first confirmed tx wins

use super::*;
use alloy::primitives::{Address, U256, Bytes, FixedBytes};
use alloy::sol;
use alloy::sol_types::SolCall;

// ── deBridge DLN Contract ABIs ────────────────────────────────────────────────

sol! {
    /// deBridge DLN Source contract interface (same address on all chains)
    interface DlnSource {
        /// Unlock locked giveTokens to the solver after a successful fulfillment.
        /// Must be called on the SOURCE chain after fulfillOrder is confirmed on dst.
        /// `_beneficiary` is where the unlocked tokens are sent (our solver wallet).
        function claimUnlock(
            bytes32 _orderId,
            address _beneficiary
        ) external;
    }

    /// deBridge DLN Destination contract interface
    interface DlnDestination {
        struct Order {
            uint64 makerOrderNonce;
            bytes makerSrc;
            uint256 giveChainId;
            bytes giveTokenAddress;
            uint256 giveAmount;
            bytes takeTokenAddress;
            uint256 takeAmount;
            bytes receiverDst;
            bytes givePatchAuthoritySrc;
            bytes orderAuthorityAddressDst;
            bytes allowedTakerDst;
            bytes allowedCancelBeneficiarySrc;
            bytes externalCall;
        }

        /// Fulfill an order on destination chain
        function fulfillOrder(
            Order calldata _order,
            uint256 _fulFillAmount,
            bytes32 _orderId,
            bytes calldata _permit,
            address _unlockAuthority
        ) external payable;

        event FulfilledOrder(
            bytes32 indexed orderId,
            address indexed beneficiary,
            uint256 fulfilledAmount
        );
    }
}

// ── deBridge Adapter Implementation ───────────────────────────────────────────

/// deBridge DLN protocol adapter
pub struct DeBridgeAdapter {
    spinner_client: SpinnerClient,
    dln_addresses: std::collections::HashMap<u64, Address>,
}

impl Clone for DeBridgeAdapter {
    fn clone(&self) -> Self {
        Self {
            spinner_client: SpinnerClient::new(self.spinner_client.base_url.clone()),
            dln_addresses: self.dln_addresses.clone(),
        }
    }
}

impl DeBridgeAdapter {
    pub fn new(spinner_client: SpinnerClient) -> Self {
        let mut dln_addresses = std::collections::HashMap::new();

        // deBridge DLN addresses (same for source and destination)
        let dln_addr: Address = "0xeF4fB24aD0916217251F553c0596F8Edc630EB66".parse().unwrap();

        dln_addresses.insert(1, dln_addr);       // Ethereum
        dln_addresses.insert(10, dln_addr);      // Optimism
        dln_addresses.insert(42161, dln_addr);   // Arbitrum
        dln_addresses.insert(8453, dln_addr);    // Base
        dln_addresses.insert(56, dln_addr);      // BSC
        dln_addresses.insert(43114, dln_addr);   // Avalanche
        dln_addresses.insert(59144, dln_addr);   // Linea

        Self {
            spinner_client,
            dln_addresses,
        }
    }

    /// Parse Order struct from genome intent.
    ///
    /// Pulls all DLN-specific fields from the genome event payload
    /// (maker_order_nonce, give_amount, take_amount). Falls back to
    /// trailing-int parsing of `intent.id` only when `maker_order_nonce`
    /// is absent — that path is for legacy events.
    fn parse_order(&self, intent: &Intent) -> Result<DlnDestination::Order> {
        let give_amount = match intent.give_amount.as_deref() {
            Some(s) => U256::from_str_radix(s, 10)?,
            None => U256::from_str_radix(&intent.amount, 10)?,
        };
        let take_amount = match intent.take_amount.as_deref() {
            Some(s) => U256::from_str_radix(s, 10)?,
            None => give_amount,
        };

        Ok(DlnDestination::Order {
            makerOrderNonce: self.extract_order_nonce(intent)?,
            makerSrc: self.address_to_bytes(&intent.depositor)?,
            giveChainId: U256::from(intent.src_chain),
            giveTokenAddress: self.address_to_bytes(&intent.src_token)?,
            giveAmount: give_amount,
            takeTokenAddress: self.address_to_bytes(&intent.dst_token)?,
            takeAmount: take_amount,
            receiverDst: self.address_to_bytes(&intent.recipient)?,
            givePatchAuthoritySrc: Bytes::new(),
            orderAuthorityAddressDst: Bytes::new(),
            allowedTakerDst: Bytes::new(),
            allowedCancelBeneficiarySrc: Bytes::new(),
            externalCall: Bytes::new(),
        })
    }

    fn extract_order_nonce(&self, intent: &Intent) -> Result<u64> {
        if let Some(n) = intent.maker_order_nonce {
            return Ok(n);
        }
        let parts: Vec<&str> = intent.id.split(&[':', '_'][..]).collect();
        let nonce_str = parts
            .last()
            .ok_or_else(|| anyhow!("Failed to extract nonce from intent ID: {}", intent.id))?;
        nonce_str.parse::<u64>()
            .map_err(|e| anyhow!("Failed to parse nonce '{}': {}", nonce_str, e))
    }

    fn address_to_bytes(&self, addr: &str) -> Result<Bytes> {
        let addr_clean = addr.trim_start_matches("0x");
        let bytes = hex::decode(addr_clean)?;
        Ok(Bytes::from(bytes))
    }

    /// Resolve the DLN orderId. Prefer the value from the genome event payload;
    /// fall back to keccak(intent.id) for legacy events that don't carry it.
    fn extract_order_id(&self, intent: &Intent) -> Result<FixedBytes<32>> {
        if let Some(s) = intent.order_id.as_deref() {
            let clean = s.trim_start_matches("0x");
            let bytes = hex::decode(clean)
                .map_err(|e| anyhow!("invalid order_id hex '{}': {}", s, e))?;
            if bytes.len() != 32 {
                return Err(anyhow!("order_id must be 32 bytes, got {}", bytes.len()));
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            return Ok(FixedBytes(arr));
        }
        use alloy::primitives::keccak256;
        Ok(keccak256(intent.id.as_bytes()))
    }
}

#[async_trait::async_trait]
impl ProtocolAdapter for DeBridgeAdapter {
    fn protocol_name(&self) -> &str {
        "debridge_dln"
    }

    fn can_handle(&self, intent: &Intent) -> bool {
        let proto_lower = intent.protocol.to_lowercase();
        proto_lower.contains("debridge") || proto_lower.contains("dln")
    }

    async fn estimate_gas(&self, intent: &Intent, spinner_api: &str) -> Result<GasEstimate> {
        let dln_dest = self.dln_addresses.get(&intent.dst_chain)
            .ok_or_else(|| anyhow!("No deBridge DLN on chain {}", intent.dst_chain))?;

        tracing::info!("🎯 deBridge adapter estimating gas for chain {} (DlnDest: {})",
            intent.dst_chain, dln_dest);

        let client = SpinnerClient::new(spinner_api);
        let gas_estimate = client.estimate_gas(intent, &dln_dest.to_string(), "DeBridgeDLN").await?;
        Ok(gas_estimate)
    }

    async fn build_fill_tx(&self, intent: &Intent, _proof: &V5ProofBlob) -> Result<FillTransaction> {
        let dln_dest = self.dln_addresses.get(&intent.dst_chain)
            .ok_or_else(|| anyhow!("No deBridge DLN on chain {}", intent.dst_chain))?;

        let order = self.parse_order(intent)?;
        let order_id = self.extract_order_id(intent)?;
        let fulfill_amount = U256::from_str_radix(&intent.amount, 10)?;

        let call = DlnDestination::fulfillOrderCall {
            _order: order,
            _fulFillAmount: fulfill_amount,
            _orderId: order_id,
            _permit: Bytes::new(),
            _unlockAuthority: Address::ZERO,
        };

        Ok(FillTransaction {
            to: dln_dest.to_string(),
            data: format!("0x{}", hex::encode(&call.abi_encode())),
            value: None,
            chain_id: intent.dst_chain,
            estimated_gas: None,
        })
    }

    async fn execute_fill(&self, intent: &Intent, fill_tx: FillTransaction, dry_run: bool) -> Result<FillResult> {
        if dry_run {
            tracing::info!("✅ [SIMULATION] deBridge fulfillOrder would be executed:");
            tracing::info!("   To: {}", fill_tx.to);
            tracing::info!("   Chain: {}", fill_tx.chain_id);
            return Ok(FillResult {
                tx_hash: format!("0xsim_debridge_fill_{}", intent.id),
                gas_used: fill_tx.estimated_gas.unwrap_or(180_000),
                block_number: 0,
                success: true,
                simulated: true,
            });
        }
        Err(anyhow!("Live deBridge fill execution not yet implemented - use SIMULATION_MODE=true"))
    }

    async fn claim_funds(&self, intent: &Intent, fill_result: &FillResult) -> Result<ClaimResult> {
        if fill_result.simulated {
            tracing::info!("✅ [SIMULATION] deBridge claimUnlock would fire on src chain {}", intent.src_chain);
            return Ok(ClaimResult {
                tx_hash: format!("0xsim_debridge_claim_{}", intent.id),
                claimed_amount: intent.amount.clone(),
                claimed_token: intent.src_token.clone(),
            });
        }
        Err(anyhow!(
            "Live deBridge claim execution not yet implemented - use SIMULATION_MODE=true. \
             Call build_claim_unlock_calldata() to get calldata then broadcast via lambda_claim_debridge()"
        ))
    }
}

impl DeBridgeAdapter {
    /// Build `fulfillOrder(order, fulfillAmount, orderId, permit, unlockAuthority)` calldata
    /// for the DlnDestination contract on the destination chain.
    pub fn build_fulfill_order_calldata(&self, intent: &Intent) -> Result<Vec<u8>> {
        let order = self.parse_order(intent)?;
        let order_id = self.extract_order_id(intent)?;
        let fulfill_amount = U256::from_str_radix(&intent.amount, 10)?;
        let call = DlnDestination::fulfillOrderCall {
            _order: order,
            _fulFillAmount: fulfill_amount,
            _orderId: order_id,
            _permit: Bytes::new(),
            _unlockAuthority: Address::ZERO,
        };
        Ok(call.abi_encode())
    }

    /// Build `claimUnlock(orderId, beneficiary)` calldata for the DlnSource contract
    /// on the SOURCE chain. Must be called after fulfillOrder is confirmed on dst chain.
    pub fn build_claim_unlock_calldata(&self, intent: &Intent, beneficiary: Address) -> Result<Vec<u8>> {
        let order_id = self.extract_order_id(intent)?;
        let call = DlnSource::claimUnlockCall {
            _orderId: order_id,
            _beneficiary: beneficiary,
        };
        Ok(call.abi_encode())
    }

    /// Address of DlnSource on the given source chain (same contract handles both sides).
    pub fn dln_source_address(&self, src_chain: u64) -> Option<Address> {
        self.dln_addresses.get(&src_chain).copied()
    }
}
