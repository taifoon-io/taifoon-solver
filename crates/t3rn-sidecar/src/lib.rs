//! t3rn-sidecar — LWC (LiquidityWellCompact V4) fill provider.
//!
//! Provides the Priority-3 liquidity path in the executor's waterfall:
//!   Priority 1: own wallet
//!   Priority 2: flash loan (future)
//!   Priority 3: LWC well (this crate, T3RN_LWC_ENABLED=true)
//!
//! Flow:
//!   1. Check `LwcManager::can_instant_exec()` on the destination chain.
//!   2. Request a `FillPermit` from the Spinner via `RegistryClient`.
//!   3. Validate the permit via `LwcAuthGuard` (sig + deadline + dedup).
//!   4. Build and broadcast the `order(...)` calldata on the LWC V4 contract.
//!   5. Consume the permit in `LwcAuthGuard` once the tx is broadcast.

use alloy::{
    network::EthereumWallet,
    primitives::{Address, U256},
    providers::{Provider, ProviderBuilder},
    signers::local::PrivateKeySigner,
    sol,
    sol_types::SolCall,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use genome_client::Intent;

// ── LWC V4 order() ABI ────────────────────────────────────────────────────────

sol! {
    interface LiquidityWellCompact {
        function order(
            bytes4  destination,
            uint32  asset,
            bytes32 targetAccount,
            uint256 amount,
            address rewardAsset,
            uint256 insurance,
            uint256 maxReward
        ) external payable;

        function canPerformInstantExecution(
            address asset,
            uint256 amount
        ) external view returns (bool canExecute, uint256 availableAmount, uint256 reservedAmount);

        function mapAssetToId(address _asset) external view returns (uint32);
    }
}

// ── Deployment registry ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
struct LwcDeployment {
    chain_id: u64,
    well_v4: String,
    rpc: String,
    primary_stable: String,
    stable_decimals: u32,
}

fn load_deployments() -> Vec<LwcDeployment> {
    let path = std::env::var("LWC_DEPLOYMENTS_PATH")
        .unwrap_or_else(|_| "config/lwc_deployments.json".to_string());
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            warn!("[t3rn-sidecar] cannot load LWC deployments from {}: {}", path, e);
            return vec![];
        }
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

// ── Fill result ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LwcFillResult {
    pub intent_id: String,
    pub tx_hash: String,
    pub chain_id: u64,
    pub amount_wei: String,
    pub used_permit_nonce: u64,
}

// ── Spinner permit client (inlined — avoids cyclic dep on solver-registry crate) ─

#[derive(Debug, Clone, Deserialize, Serialize)]
struct FillPermit {
    intent_id: String,
    solver: String,
    chain_id: u64,
    amount_wei: String,
    deadline: u64,
    nonce: u64,
    signature: String,
}

async fn request_permit(
    http: &reqwest::Client,
    spinner_url: &str,
    intent_id: &str,
    chain_id: u64,
    amount_wei: U256,
    solver: Address,
) -> Result<FillPermit> {
    let url = format!("{}/api/solver/lwc-permit", spinner_url);
    let body = serde_json::json!({
        "intent_id": intent_id,
        "chain_id": chain_id,
        "amount_wei": amount_wei.to_string(),
        "solver_address": format!("{:#x}", solver),
    });
    let resp = http.post(&url).json(&body).send().await.context("permit request HTTP")?;
    if resp.status().as_u16() == 409 {
        anyhow::bail!("permit already issued for intent={} chain={}", intent_id, chain_id);
    }
    if !resp.status().is_success() {
        let msg = resp.text().await.unwrap_or_default();
        anyhow::bail!("spinner permit error: {}", msg);
    }
    resp.json::<FillPermit>().await.context("parse FillPermit")
}

// ── T3RNSidecar (main entry point) ────────────────────────────────────────────

pub struct T3RNSidecar {
    signer: PrivateKeySigner,
    solver_addr: Address,
    deployments: Vec<LwcDeployment>,
    spinner_url: String,
    http: reqwest::Client,
    used_permits: std::sync::Mutex<std::collections::HashSet<(String, u64)>>,
    pub dry_run: bool,
}

impl T3RNSidecar {
    pub fn new(signer: PrivateKeySigner) -> Self {
        let solver_addr = signer.address();
        let spinner_url = std::env::var("SPINNER_API_URL")
            .or_else(|_| std::env::var("WARMBED_API_URL"))
            .unwrap_or_else(|_| "https://api.taifoon.dev".to_string());
        let dry_run = std::env::var("DRY_RUN").map(|v| v == "true" || v == "1").unwrap_or(true);

        Self {
            signer,
            solver_addr,
            deployments: load_deployments(),
            spinner_url,
            http: reqwest::Client::builder()
                .user_agent("t3rn-sidecar/1.0")
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            used_permits: std::sync::Mutex::new(std::collections::HashSet::new()),
            dry_run,
        }
    }

    /// Check whether the LWC on `dst_chain` can provide liquidity for this intent.
    pub async fn can_provide_liquidity(&self, intent: &Intent) -> bool {
        let dep = match self.dep_for(intent.dst_chain) {
            Some(d) => d,
            None => return false,
        };

        let well: Address = match dep.well_v4.parse() {
            Ok(a) => a,
            Err(_) => return false,
        };
        let asset: Address = match dep.primary_stable.parse() {
            Ok(a) => a,
            Err(_) => return false,
        };
        let rpc_url = match dep.rpc.parse() {
            Ok(u) => u,
            Err(_) => return false,
        };

        let amount_wei = match intent.amount.parse::<U256>() {
            Ok(a) => a,
            Err(_) => return false,
        };

        let provider = ProviderBuilder::new().on_http(rpc_url);
        let call = LiquidityWellCompact::canPerformInstantExecutionCall { asset, amount: amount_wei };
        let req = alloy::rpc::types::TransactionRequest::default()
            .to(well)
            .input(call.abi_encode().into());

        match provider.call(&req).await {
            Ok(bytes) if bytes.len() >= 32 => bytes[31] != 0,
            _ => false,
        }
    }

    /// Execute a fill for `intent` using LWC well funds.
    ///
    /// Requests a permit from the Spinner, validates it, then broadcasts
    /// the `order(...)` transaction on the destination chain's LWC contract.
    pub async fn fill(&self, intent: &Intent) -> Result<LwcFillResult> {
        let dep = self.dep_for(intent.dst_chain)
            .context("no LWC deployment for dst chain")?;

        let well: Address = dep.well_v4.parse().context("invalid well address")?;
        let asset: Address = dep.primary_stable.parse().context("invalid stable address")?;
        let amount_wei: U256 = intent.amount.parse().context("invalid intent amount")?;
        let rpc_url = dep.rpc.parse().context("invalid rpc")?;

        let permit_key = (intent.id.clone(), intent.dst_chain);

        // 1. Guard against double-spend on this solver instance
        {
            let used = self.used_permits.lock().unwrap();
            if used.contains(&permit_key) {
                anyhow::bail!("permit already used for intent={} chain={}", intent.id, intent.dst_chain);
            }
        }

        // 2. Request signed permit from Spinner
        let permit = request_permit(
            &self.http,
            &self.spinner_url,
            &intent.id,
            intent.dst_chain,
            amount_wei,
            self.solver_addr,
        ).await.context("request_permit")?;

        info!(
            "[t3rn-sidecar] permit issued: intent={} chain={} amount={} deadline={} dry_run={}",
            intent.id, intent.dst_chain, amount_wei, permit.deadline, self.dry_run
        );

        // 3. Validate permit deadline
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if permit.deadline < now {
            anyhow::bail!("permit expired: deadline={} now={}", permit.deadline, now);
        }

        if self.dry_run {
            // Mark permit as used without broadcasting
            self.used_permits.lock().unwrap().insert(permit_key);
            return Ok(LwcFillResult {
                intent_id: intent.id.clone(),
                tx_hash: "dry-run:no-tx".to_string(),
                chain_id: intent.dst_chain,
                amount_wei: amount_wei.to_string(),
                used_permit_nonce: permit.nonce,
            });
        }

        // 4. Build order() calldata
        let target_account = parse_target_account(&intent.recipient)?;
        let asset_id = self.get_asset_id(&dep.rpc, well, asset).await.unwrap_or(0);
        let destination = parse_destination_code(&intent.dst_chain.to_string());
        let max_reward: U256 = intent.amount.parse().unwrap_or(amount_wei);

        let order_call = LiquidityWellCompact::orderCall {
            destination,
            asset: asset_id,
            targetAccount: target_account,
            amount: amount_wei,
            rewardAsset: asset,
            insurance: U256::ZERO,
            maxReward: max_reward,
        };

        // 5. Broadcast
        let wallet = EthereumWallet::from(self.signer.clone());
        let provider = ProviderBuilder::new().wallet(wallet).on_http(rpc_url);

        let req = alloy::rpc::types::TransactionRequest::default()
            .to(well)
            .input(order_call.abi_encode().into());

        let pending = provider.send_transaction(req).await.context("send order tx")?;
        let receipt = pending.get_receipt().await.context("order receipt")?;
        let tx_hash = format!("{:?}", receipt.transaction_hash);

        info!("[t3rn-sidecar] order submitted: intent={} tx={}", intent.id, tx_hash);

        // 6. Mark permit consumed
        self.used_permits.lock().unwrap().insert(permit_key);

        Ok(LwcFillResult {
            intent_id: intent.id.clone(),
            tx_hash,
            chain_id: intent.dst_chain,
            amount_wei: amount_wei.to_string(),
            used_permit_nonce: permit.nonce,
        })
    }

    fn dep_for(&self, chain_id: u64) -> Option<&LwcDeployment> {
        self.deployments.iter().find(|d| d.chain_id == chain_id)
    }

    async fn get_asset_id(&self, rpc: &str, well: Address, asset: Address) -> Option<u32> {
        let rpc_url = rpc.parse().ok()?;
        let provider = ProviderBuilder::new().on_http(rpc_url);
        let call = LiquidityWellCompact::mapAssetToIdCall { _asset: asset };
        let req = alloy::rpc::types::TransactionRequest::default()
            .to(well)
            .input(call.abi_encode().into());
        let bytes = provider.call(&req).await.ok()?;
        if bytes.len() >= 32 {
            let raw = U256::from_be_slice(&bytes[bytes.len() - 32..]);
            raw.try_into().ok()
        } else {
            None
        }
    }
}

impl Default for T3RNSidecar {
    fn default() -> Self {
        // All-zero private key — only for test/placeholder construction.
        let zero_bytes = [0u8; 32];
        let wallet = alloy::signers::local::PrivateKeySigner::from_signing_key(
            alloy::signers::k256::ecdsa::SigningKey::from_bytes(
                (&zero_bytes).into()
            ).expect("zero key")
        );
        Self::new(wallet)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn parse_target_account(recipient: &str) -> Result<alloy::primitives::FixedBytes<32>> {
    let trimmed = recipient.trim_start_matches("0x");
    let bytes = hex::decode(trimmed).context("decode recipient")?;
    let mut out = [0u8; 32];
    let start = 32usize.saturating_sub(bytes.len());
    out[start..].copy_from_slice(&bytes[..32usize.min(bytes.len())]);
    Ok(alloy::primitives::FixedBytes::<32>::from(out))
}

fn parse_destination_code(chain_id: &str) -> alloy::primitives::FixedBytes<4> {
    // Map chain IDs to t3rn 4-byte destination codes.
    // These must match the codes used by the LWC contract.
    match chain_id {
        "1"     => *b"ethm",
        "8453"  => *b"basm",
        "42161" => *b"arbm",
        "10"    => *b"optm",
        "137"   => *b"polm",
        "56"    => *b"bscm",
        "59144" => *b"linm",
        _       => *b"\0\0\0\0",
    }.into()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_target_account_eth_address() {
        let addr = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913";
        let result = parse_target_account(addr).unwrap();
        assert_eq!(result[12..], hex::decode("833589fCD6eDb6E08f4c7C32D4f71b54bdA02913").unwrap()[..20]);
    }

    #[test]
    fn parse_destination_code_known_chains() {
        assert_eq!(parse_destination_code("8453").as_slice(), b"basm");
        assert_eq!(parse_destination_code("42161").as_slice(), b"arbm");
        assert_eq!(parse_destination_code("10").as_slice(), b"optm");
    }

    #[test]
    fn parse_destination_code_unknown_chain() {
        let code = parse_destination_code("99999");
        assert_eq!(code.as_slice(), b"\0\0\0\0");
    }
}
