//! CCTP (Circle Cross-Chain Transfer Protocol) Adapter
//!
//! ## Protocol Model
//! CCTP is Circle's native USDC bridging protocol. Unlike pool-based bridges,
//! CCTP burns USDC on the source chain and mints it on the destination — no
//! liquidity pools, no slippage. The solver acts as a fast-path relayer:
//! 1. Detect a CCTP burn on the source chain
//! 2. Fetch the Circle attestation (off-chain, ~20s)
//! 3. Call `receiveMessage()` on the destination MessageTransmitter with the attestation
//! 4. USDC is minted directly to the recipient on destination
//!
//! ## Lifecycle
//! ```text
//! 1. User burns USDC via TokenMessenger.depositForBurn()
//!    ├─ Event: DepositForBurn(nonce, burnToken, amount, depositor, mintRecipient,
//!    │         destinationDomain, destinationTokenMessenger)
//!    └─ USDC destroyed on source chain
//!
//! 2. Solver polls Circle attestation API for the nonce (~20s)
//!    └─ Returns: {attestation: "0x...", message: "0x..."}
//!
//! 3. Solver calls receiveMessage(message, attestation) on destination MessageTransmitter
//!    └─ Event: MessageReceived(caller, sourceDomain, nonce, sender, messageBody)
//!    └─ USDC minted to mintRecipient
//! ```
//!
//! ## Reward Mechanism
//! - **Model**: Off-protocol (CCTP itself has no solver fee)
//! - **Earning**: Taifoon charges a thin wrapper fee on top (via the genome portal)
//! - **Payment**: Paid by user as a pre-deducted spread on the amount
//! - **Expected Margin**: 1–3 bps (CCTP is near-zero fee, revenue from speed premium)
//!
//! ## Supported Domains (Circle CCTP V1, mainnet)
//! - Domain 0: Ethereum (chain 1)
//! - Domain 1: Avalanche (chain 43114)
//! - Domain 2: Optimism (chain 10)
//! - Domain 3: Arbitrum (chain 42161)
//! - Domain 6: Base (chain 8453)
//! - Domain 7: Polygon (chain 137)
//!
//! ## Contract Addresses (CCTP V1, verified 2026-06)
//! MessageTransmitter (receives message + attestation):
//! - Ethereum:   0x0a992d191DEeC32aFe36203Ad87D7d289a738F81
//! - Avalanche:  0x8186359aF5F57FbB40c6b14A588d2A59C0C29880
//! - Optimism:   0x4D41f22c5a0e5c74090899E5a8Fb597a8842b3e8
//! - Arbitrum:   0xC30362313FBBA5cf9163F0bb16a0e01f01A896ca
//! - Base:       0xAD09780d193884d503182aD4588450C416D6F9D4
//! - Polygon:    0xF3be9355363857F3e001be68856A2f96b4C39Ba9
//!
//! ## Circle Attestation API
//! GET https://iris-api.circle.com/attestations/{messageHash}
//! Returns: {status: "complete"|"pending_confirmations", attestation: "0x..."}

use super::*;
use alloy::primitives::{Address, Bytes};
use alloy::sol;
use alloy::sol_types::SolCall;

// ── CCTP Domain Mapping ───────────────────────────────────────────────────────

fn cctp_domain(chain_id: u64) -> Option<u32> {
    match chain_id {
        1     => Some(0), // Ethereum
        43114 => Some(1), // Avalanche
        10    => Some(2), // Optimism
        42161 => Some(3), // Arbitrum
        8453  => Some(6), // Base
        137   => Some(7), // Polygon
        _     => None,
    }
}

fn message_transmitter_address(chain_id: u64) -> Option<Address> {
    let s = match chain_id {
        1     => "0x0a992d191DEeC32aFe36203Ad87D7d289a738F81", // Ethereum
        43114 => "0x8186359aF5F57FbB40c6b14A588d2A59C0C29880", // Avalanche
        10    => "0x4D41f22c5a0e5c74090899E5a8Fb597a8842b3e8", // Optimism
        42161 => "0xC30362313FBBA5cf9163F0bb16a0e01f01A896ca", // Arbitrum
        8453  => "0xAD09780d193884d503182aD4588450C416D6F9D4", // Base
        137   => "0xF3be9355363857F3e001be68856A2f96b4C39Ba9", // Polygon
        _     => return None,
    };
    s.parse().ok()
}

/// Circle attestation API base URL.
/// Override via CCTP_ATTESTATION_API env var for testing.
fn attestation_api() -> String {
    std::env::var("CCTP_ATTESTATION_API")
        .unwrap_or_else(|_| "https://iris-api.circle.com".to_string())
}

// ── CCTP MessageTransmitter ABI ───────────────────────────────────────────────

sol! {
    /// Circle CCTP MessageTransmitter V1
    interface IMessageTransmitter {
        /// Receive a cross-chain USDC message and mint USDC on destination.
        /// `message` is the raw ABI-encoded message from the burn event.
        /// `attestation` is the 65-byte ECDSA signature from Circle.
        function receiveMessage(
            bytes calldata message,
            bytes calldata attestation
        ) external returns (bool success);

        /// The domain of this transmitter (used for verification).
        function localDomain() external view returns (uint32);
    }
}

// ── CctpAdapter ──────────────────────────────────────────────────────────────

pub struct CctpAdapter {
    spinner_client: SpinnerClient,
    http_client: reqwest::Client,
}

impl Clone for CctpAdapter {
    fn clone(&self) -> Self {
        Self {
            spinner_client: SpinnerClient::new(self.spinner_client.base_url.clone()),
            http_client: reqwest::Client::new(),
        }
    }
}

impl CctpAdapter {
    pub fn new(spinner_client: SpinnerClient) -> Self {
        Self {
            spinner_client,
            http_client: reqwest::Client::new(),
        }
    }

    /// Fetch a Circle attestation for the given message hash.
    /// Returns (message_hex, attestation_hex) on success.
    async fn fetch_attestation(&self, message_hash: &str) -> Result<(String, String)> {
        #[derive(serde::Deserialize)]
        struct AttestationResponse {
            status: String,
            attestation: Option<String>,
            message: Option<String>,
        }

        let url = format!("{}/attestations/{}", attestation_api(), message_hash);
        let resp = self.http_client
            .get(&url)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| anyhow!("CCTP attestation fetch failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(anyhow!("CCTP API error: {}", resp.status()));
        }

        let body: AttestationResponse = resp.json().await
            .map_err(|e| anyhow!("CCTP API parse error: {}", e))?;

        if body.status != "complete" {
            return Err(anyhow!("CCTP attestation not yet complete: status={}", body.status));
        }

        let message = body.message.ok_or_else(|| anyhow!("CCTP: missing message in response"))?;
        let attestation = body.attestation.ok_or_else(|| anyhow!("CCTP: missing attestation in response"))?;
        Ok((message, attestation))
    }

    /// Encode a `receiveMessage(message, attestation)` call.
    /// In the live path, `message` and `attestation` come from `fetch_attestation()`.
    /// For build_fill_tx (offline / gas estimation) we use the tx_hash as a placeholder
    /// message and a zeroed 65-byte attestation — the calldata shape is correct for
    /// .estimateGas() even though it will revert if actually broadcast without a real attestation.
    fn encode_receive_message(
        &self,
        intent: &Intent,
        message_hex: &str,
        attestation_hex: &str,
    ) -> Result<FillTransaction> {
        let transmitter = message_transmitter_address(intent.dst_chain)
            .ok_or_else(|| anyhow!("CCTP: no MessageTransmitter on chain {}", intent.dst_chain))?;

        let message_bytes = hex::decode(message_hex.trim_start_matches("0x"))
            .unwrap_or_else(|_| vec![0u8; 32]);
        let attestation_bytes = hex::decode(attestation_hex.trim_start_matches("0x"))
            .unwrap_or_else(|_| vec![0u8; 65]);

        let call = IMessageTransmitter::receiveMessageCall {
            message: Bytes::from(message_bytes),
            attestation: Bytes::from(attestation_bytes),
        };
        let calldata = hex::encode(call.abi_encode());

        Ok(FillTransaction {
            to: transmitter.to_string(),
            data: format!("0x{}", calldata),
            value: Some("0x0".to_string()), // CCTP receiveMessage sends no ETH
            chain_id: intent.dst_chain,
            estimated_gas: Some(100_000),
        })
    }
}

#[async_trait::async_trait]
impl ProtocolAdapter for CctpAdapter {
    fn protocol_name(&self) -> &str {
        "cctp"
    }

    fn can_handle(&self, intent: &Intent) -> bool {
        let p = intent.protocol.to_lowercase();
        p.contains("cctp") || p.contains("circle_bridge") || p.contains("circle_transfer")
    }

    async fn estimate_gas(&self, intent: &Intent, spinner_api: &str) -> Result<GasEstimate> {
        let transmitter = message_transmitter_address(intent.dst_chain)
            .ok_or_else(|| anyhow!("CCTP: no MessageTransmitter on chain {}", intent.dst_chain))?;
        let client = SpinnerClient::new(spinner_api);
        client.estimate_gas(intent, &transmitter.to_string(), "CCTPMessageTransmitter").await
    }

    async fn build_fill_tx(&self, intent: &Intent, _proof: &V5ProofBlob) -> Result<FillTransaction> {
        // Validate domain support first
        cctp_domain(intent.src_chain)
            .ok_or_else(|| anyhow!("CCTP: unsupported src chain {}", intent.src_chain))?;
        cctp_domain(intent.dst_chain)
            .ok_or_else(|| anyhow!("CCTP: unsupported dst chain {}", intent.dst_chain))?;

        // Use tx_hash as placeholder message + zero attestation for gas-estimation calldata
        self.encode_receive_message(intent, &intent.tx_hash, "0x")
    }

    async fn execute_fill(&self, intent: &Intent, fill_tx: FillTransaction, dry_run: bool) -> Result<FillResult> {
        if dry_run {
            tracing::info!("✅ [SIMULATION] CCTP receiveMessage() would execute:");
            tracing::info!("   MessageTransmitter: {}", fill_tx.to);
            tracing::info!("   src_domain={} dst_domain={}", intent.src_chain, intent.dst_chain);
            return Ok(FillResult {
                tx_hash: format!("0xsim_cctp_{}", &intent.id[..intent.id.len().min(16)]),
                gas_used: fill_tx.estimated_gas.unwrap_or(100_000),
                block_number: 0,
                success: true,
                simulated: true,
            });
        }

        // Live path: fetch Circle attestation, then broadcast
        tracing::info!("🔵 CCTP: fetching Circle attestation for tx {}", intent.tx_hash);
        let (_message, _attestation) = self.fetch_attestation(&intent.tx_hash).await?;
        // TODO: broadcast receiveMessage with real attestation
        Err(anyhow!("CCTP live broadcast not yet implemented (attestation fetched OK — broadcast wiring pending)"))
    }

    async fn claim_funds(&self, intent: &Intent, fill_result: &FillResult) -> Result<ClaimResult> {
        // CCTP mints USDC directly to the recipient — no separate claim step.
        tracing::info!("ℹ️  CCTP: USDC minted on chain {} via receiveMessage tx {}", intent.dst_chain, fill_result.tx_hash);
        Ok(ClaimResult {
            tx_hash: fill_result.tx_hash.clone(),
            claimed_amount: intent.amount.clone(),
            claimed_token: intent.dst_token.clone(),
        })
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use genome_client::Intent;

    fn usdc_cctp_intent() -> Intent {
        Intent {
            id: "cctp:test_cc_001".to_string(),
            protocol: "cctp".to_string(),
            src_chain: 1,     // Ethereum (domain 0)
            dst_chain: 42161, // Arbitrum (domain 3)
            src_token: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string(),
            dst_token: "0xaf88d065e77c8cC2239327C5EDb3A432268e5831".to_string(),
            amount: "2000000".to_string(), // 2 USDC
            depositor: "0x3333333333333333333333333333333333333333".to_string(),
            recipient: "0x4444444444444444444444444444444444444444".to_string(),
            tx_hash: "0x" .to_string() + &"ab".repeat(32), // placeholder message hash (64 hex chars)
            detected_at: 1700000000,
            ..Default::default()
        }
    }

    fn test_proof() -> V5ProofBlob {
        V5ProofBlob {
            l1_superroot: L1SuperRoot { hash: "0x0".to_string(), timestamp: 0, chains_included: vec![] },
            l2_chain_header: L2ChainHeader { chain_id: 1, block_number: 0, block_hash: "0x0".to_string(), parent_hash: "0x0".to_string(), state_root: "0x0".to_string(), timestamp: 0 },
            l3_superroot_proof: vec![], l4_block_proof: vec![],
            l5_chain_event: L5ChainEvent { tx_hash: "0x0".to_string(), tx_index: 0, log_index: None, encoded_tx: "0x".to_string(), encoded_receipt: "0x".to_string() },
            l6_finality: L6FinalityCommitment { finality_type: "POW".to_string(), commitment_data: "{}".to_string() },
        }
    }

    #[tokio::test]
    async fn cctp_can_handle_cctp_intent() {
        let sc = SpinnerClient::new("https://api.taifoon.dev");
        let adapter = CctpAdapter::new(sc);
        let intent = usdc_cctp_intent();
        assert!(adapter.can_handle(&intent));
        assert_eq!(adapter.protocol_name(), "cctp");
    }

    #[tokio::test]
    async fn cctp_handles_circle_bridge_alias() {
        let sc = SpinnerClient::new("https://api.taifoon.dev");
        let adapter = CctpAdapter::new(sc);
        let mut intent = usdc_cctp_intent();
        intent.protocol = "circle_bridge".to_string();
        assert!(adapter.can_handle(&intent));
    }

    #[tokio::test]
    async fn cctp_rejects_unsupported_src_chain() {
        let sc = SpinnerClient::new("https://api.taifoon.dev");
        let adapter = CctpAdapter::new(sc);
        let mut intent = usdc_cctp_intent();
        intent.src_chain = 9999; // no CCTP domain
        let result = adapter.build_fill_tx(&intent, &test_proof()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unsupported src chain"));
    }

    #[tokio::test]
    async fn cctp_rejects_unsupported_dst_chain() {
        let sc = SpinnerClient::new("https://api.taifoon.dev");
        let adapter = CctpAdapter::new(sc);
        let mut intent = usdc_cctp_intent();
        intent.dst_chain = 9999; // no CCTP domain
        let result = adapter.build_fill_tx(&intent, &test_proof()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unsupported dst chain"));
    }

    #[tokio::test]
    async fn cctp_builds_well_formed_fill_tx() {
        let sc = SpinnerClient::new("https://api.taifoon.dev");
        let adapter = CctpAdapter::new(sc);
        let intent = usdc_cctp_intent();
        let fill_tx = adapter.build_fill_tx(&intent, &test_proof()).await.unwrap();
        assert_eq!(fill_tx.chain_id, 42161); // dst chain (Arbitrum)
        assert!(fill_tx.data.starts_with("0x"));
        assert!(fill_tx.data.len() > 10);
        assert_eq!(fill_tx.value.as_deref(), Some("0x0")); // no ETH value for CCTP
        // MessageTransmitter on Arbitrum
        assert_eq!(fill_tx.to.to_lowercase(), "0xc30362313fbba5cf9163f0bb16a0e01f01a896ca");
    }

    #[tokio::test]
    async fn cctp_simulated_fill_succeeds() {
        let sc = SpinnerClient::new("https://api.taifoon.dev");
        let adapter = CctpAdapter::new(sc);
        let intent = usdc_cctp_intent();
        let fill_tx = adapter.build_fill_tx(&intent, &test_proof()).await.unwrap();
        let result = adapter.execute_fill(&intent, fill_tx, true).await.unwrap();
        assert!(result.simulated);
        assert!(result.success);
        assert!(result.tx_hash.contains("cctp"));
    }

    #[tokio::test]
    async fn cctp_claim_returns_minted_amount() {
        let sc = SpinnerClient::new("https://api.taifoon.dev");
        let adapter = CctpAdapter::new(sc);
        let intent = usdc_cctp_intent();
        let fill_result = FillResult { tx_hash: "0xmint_tx".to_string(), gas_used: 0, block_number: 0, success: true, simulated: true };
        let claim = adapter.claim_funds(&intent, &fill_result).await.unwrap();
        assert_eq!(claim.claimed_amount, "2000000");
        assert_eq!(claim.tx_hash, "0xmint_tx");
    }

    #[tokio::test]
    async fn cctp_all_domain_pairs_buildable() {
        let sc = SpinnerClient::new("https://api.taifoon.dev");
        let adapter = CctpAdapter::new(sc);
        // All supported CCTP chains as dst
        let dst_chains = [1u64, 43114, 10, 42161, 8453, 137];
        for dst in dst_chains {
            if dst == 1 { continue; } // skip src==dst
            let mut intent = usdc_cctp_intent();
            intent.dst_chain = dst;
            let result = adapter.build_fill_tx(&intent, &test_proof()).await;
            assert!(result.is_ok(), "CCTP dst chain {} failed: {:?}", dst, result);
        }
    }
}
