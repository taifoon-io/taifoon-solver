//! Protocol Adapters — Cross-chain intent fulfillment with live .estimateGas() integration
//!
//! ## Purpose
//!
//! Provides protocol-specific adapters that connect taifoon-solver to actual cross-chain
//! protocols (Across, deBridge, Mayan, etc.) with complete lifecycle implementation:
//! 1. Detect deposit intent from genome stream
//! 2. Fetch V5 MMR proof from Spinner DA layer
//! 3. Estimate gas cost via .estimateGas() RPC call
//! 4. Build fill transaction for destination chain
//! 5. Submit fill (or simulate in dry-run mode)
//! 6. Claim funds on source chain (protocol-dependent)
//!
//! ## Integration with Spinner Solver APIs
//!
//! This crate uses the Spinner solver APIs built in Phase 2-3:
//! - `POST /api/v5/proof/bundle` — Generate V5 proof for source transaction
//! - `POST /api/solver/estimate-gas` — Get live .estimateGas() result for fill
//! - `POST /api/solver/test-run` — Dry-run profitability analysis
//!
//! ## Supported Protocols
//!
//! - **Across V3**: Dynamic fee model, deposit → fill → settle
//! - **deBridge DLN**: Pure spread model, createOrder → fulfillOrder → claim
//! - **Mayan Finance**: Auction model, orderCreate → fulfill → settlement
//! - More protocols to be added (Relay, Connext, Stargate, etc.)

use anyhow::{Result, anyhow};
use genome_client::Intent;
use serde::{Deserialize, Serialize};

pub mod across;
pub mod debridge;
pub mod lifi;
pub mod mayan;
pub mod orbiter;
pub mod stargate;

// ── Protocol Adapter Trait ────────────────────────────────────────────────────

/// Protocol adapter trait for cross-chain intent fulfillment
#[async_trait::async_trait]
pub trait ProtocolAdapter: Send + Sync {
    /// Protocol name (e.g., "across", "debridge", "mayan")
    fn protocol_name(&self) -> &str;

    /// Check if this adapter can handle the given intent
    fn can_handle(&self, intent: &Intent) -> bool;

    /// Estimate gas cost for filling this intent on destination chain
    /// Uses Spinner /api/solver/estimate-gas endpoint
    async fn estimate_gas(&self, intent: &Intent, spinner_api: &str) -> Result<GasEstimate>;

    /// Build fill transaction for destination chain
    /// Returns unsigned transaction ready for .estimateGas() or broadcast
    async fn build_fill_tx(&self, intent: &Intent, proof: &V5ProofBlob) -> Result<FillTransaction>;

    /// Execute fill transaction (or simulate if dry_run=true)
    async fn execute_fill(&self, intent: &Intent, fill_tx: FillTransaction, dry_run: bool) -> Result<FillResult>;

    /// Claim source chain funds after fill (protocol-dependent)
    async fn claim_funds(&self, intent: &Intent, fill_result: &FillResult) -> Result<ClaimResult>;
}

// ── Data Structures ───────────────────────────────────────────────────────────

/// V5 MMR Proof Blob (from Spinner)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V5ProofBlob {
    pub l1_superroot: L1SuperRoot,
    pub l2_chain_header: L2ChainHeader,
    pub l3_superroot_proof: Vec<String>, // Merkle siblings
    pub l4_block_proof: Vec<String>,     // Twig siblings
    pub l5_chain_event: L5ChainEvent,
    pub l6_finality: L6FinalityCommitment,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L1SuperRoot {
    pub hash: String,
    pub timestamp: u64,
    pub chains_included: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L2ChainHeader {
    pub chain_id: u64,
    pub block_number: u64,
    pub block_hash: String,
    pub parent_hash: String,
    pub state_root: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L5ChainEvent {
    pub tx_hash: String,
    pub tx_index: u32,
    pub log_index: Option<u32>,
    pub encoded_tx: String,      // RLP-encoded transaction
    pub encoded_receipt: String, // RLP-encoded receipt
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L6FinalityCommitment {
    pub finality_type: String,
    pub commitment_data: String, // JSON-encoded finality proof
}

/// Gas estimation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasEstimate {
    pub gas_units: u64,
    pub gas_price_gwei: f64,
    pub total_eth: f64,
    pub total_usd: f64,
    pub destination_chain: u64,
}

/// Test run result (profitability analysis)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestRunResult {
    pub profitable: bool,
    pub net_profit_usd: f64,
    pub gas_cost_usd: f64,
    pub recommendation: String,
}

/// Fill transaction (unsigned, ready for .estimateGas or broadcast)
#[derive(Debug, Clone)]
pub struct FillTransaction {
    pub to: String,               // Contract address
    pub data: String,             // Calldata (hex)
    pub value: Option<String>,    // ETH value (hex, for native token fills)
    pub chain_id: u64,
    pub estimated_gas: Option<u64>,
}

/// Fill execution result
#[derive(Debug, Clone)]
pub struct FillResult {
    pub tx_hash: String,
    pub gas_used: u64,
    pub block_number: u64,
    pub success: bool,
    pub simulated: bool, // true if dry-run mode
}

/// Claim result (source chain funds retrieval)
#[derive(Debug, Clone)]
pub struct ClaimResult {
    pub tx_hash: String,
    pub claimed_amount: String,
    pub claimed_token: String,
}

// ── Spinner API Client ────────────────────────────────────────────────────────

/// Client for Spinner solver APIs
#[derive(Clone)]
pub struct SpinnerClient {
    base_url: String,
    http_client: reqwest::Client,
}

impl SpinnerClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            http_client: reqwest::Client::new(),
        }
    }

    /// Fetch V5 proof bundle for a transaction
    /// POST /api/v5/proof/bundle
    pub async fn fetch_proof_bundle(&self, intent: &Intent) -> Result<V5ProofBlob> {
        #[derive(Serialize)]
        struct ProofRequest {
            src_chain_id: u64,
            tx_hash: String,
            protocol: String,
            order_id: String,
        }

        let url = format!("{}/api/v5/proof/bundle", self.base_url);
        let req = ProofRequest {
            src_chain_id: intent.src_chain,
            tx_hash: intent.tx_hash.clone(),
            protocol: intent.protocol.clone(),
            order_id: intent.id.clone(),
        };

        tracing::debug!("📡 Fetching V5 proof bundle from Spinner: {}", url);

        let resp = self.http_client
            .post(&url)
            .json(&req)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!("Spinner API error: {}", resp.status()));
        }

        #[derive(Deserialize)]
        struct ProofResponse {
            proof: V5ProofBlob,
        }

        let proof_resp: ProofResponse = resp.json().await?;
        Ok(proof_resp.proof)
    }

    /// Estimate gas for filling an intent
    /// POST /api/solver/estimate-gas
    pub async fn estimate_gas(
        &self,
        intent: &Intent,
        adapter_address: &str,
        adapter_name: &str,
    ) -> Result<GasEstimate> {
        #[derive(Serialize)]
        struct GasRequest {
            protocol: String,
            order_id: String,
            dst_chain_id: u64,
            adapter_address: String,
            adapter_name: String,
        }

        let url = format!("{}/api/solver/estimate-gas", self.base_url);
        let req = GasRequest {
            protocol: intent.protocol.clone(),
            order_id: intent.id.clone(),
            dst_chain_id: intent.dst_chain,
            adapter_address: adapter_address.to_string(),
            adapter_name: adapter_name.to_string(),
        };

        tracing::debug!("📡 Estimating gas via Spinner: {}", url);

        let resp = self.http_client
            .post(&url)
            .json(&req)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!("Spinner gas estimation error: {}", resp.status()));
        }

        let gas_estimate: GasEstimate = resp.json().await?;
        Ok(gas_estimate)
    }

    /// Run dry-run profitability test
    /// POST /api/solver/test-run
    pub async fn test_run(&self, intent: &Intent) -> Result<TestRunResult> {
        #[derive(Serialize)]
        struct TestRequest {
            protocol: String,
            order_id: String,
        }

        let url = format!("{}/api/solver/test-run", self.base_url);
        let req = TestRequest {
            protocol: intent.protocol.clone(),
            order_id: intent.id.clone(),
        };

        let resp = self.http_client
            .post(&url)
            .json(&req)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!("Spinner test-run error: {}", resp.status()));
        }

        let result: TestRunResult = resp.json().await?;
        Ok(result)
    }
}

// ── Adapter Factory ───────────────────────────────────────────────────────────

/// Factory for creating protocol adapters
pub struct AdapterFactory {
    spinner_client: SpinnerClient,
}

impl AdapterFactory {
    pub fn new(spinner_api_url: impl Into<String>) -> Self {
        Self {
            spinner_client: SpinnerClient::new(spinner_api_url),
        }
    }

    /// Get adapter for a specific intent
    pub fn get_adapter(&self, intent: &Intent) -> Result<Box<dyn ProtocolAdapter>> {
        let protocol_lower = intent.protocol.to_lowercase();

        // Match protocol (handle both "across" and "across_v3")
        if protocol_lower.contains("across") {
            return Ok(Box::new(across::AcrossAdapter::new(
                self.spinner_client.clone()
            )));
        }

        if protocol_lower.contains("debridge") {
            return Ok(Box::new(debridge::DeBridgeAdapter::new(
                self.spinner_client.clone()
            )));
        }

        if protocol_lower.contains("mayan") {
            return Ok(Box::new(mayan::MayanAdapter::new(
                self.spinner_client.clone()
            )));
        }

        if protocol_lower.contains("lifi") || protocol_lower.contains("li.fi") {
            return Ok(Box::new(lifi::LiFiAdapter::new(
                self.spinner_client.clone()
            )));
        }

        if protocol_lower.contains("orbiter") {
            return Ok(Box::new(orbiter::OrbiterAdapter::new(
                self.spinner_client.clone()
            )));
        }

        Err(anyhow!("No adapter found for protocol: {}", intent.protocol))
    }

    /// List all supported protocols
    pub fn supported_protocols(&self) -> Vec<&'static str> {
        vec!["across", "across_v3", "debridge", "dln", "lifi", "li.fi", "mayan", "mayan_finance", "mayan_swift", "orbiter_finance", "orbiter"]
    }
}

// Re-export common types
pub use across::AcrossAdapter;
pub use debridge::DeBridgeAdapter;
pub use lifi::LiFiAdapter;
pub use mayan::MayanAdapter;
pub use orbiter::OrbiterAdapter;
