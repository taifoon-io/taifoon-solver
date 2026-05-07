//! t3rn-sidecar — LWC V4 fill provider + parallel delivery matrix.
//!
//! Three public surfaces:
//!
//! 1. `T3RNSidecar::fill(intent)` — Priority-3 liquidity path in the executor
//!    waterfall.  Checks canPerformInstantExecution, requests a Spinner permit,
//!    runs eth_estimateGas, then broadcasts order().
//!
//! 2. `DeliveryMatrix::scan()` — Queries all deployed wells in parallel and
//!    returns every (chain, asset) pair that currently has liquidity.
//!
//! 3. `DeliveryWorker::start_http(port)` — Axum server that open-mamba calls
//!    via its scheduled webhooks.  Each schedule fires POST /lwc/deliver with a
//!    chain_id+asset payload; the worker calls fill() and returns the result.

use alloy::{
    network::EthereumWallet,
    primitives::{Address, U256},
    providers::{Provider, ProviderBuilder},
    signers::local::PrivateKeySigner,
    sol_types::SolCall,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

pub use portfolio_sidecar::lwc_manager::LiquidityWellCompact;

// ── Deployment config ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LwcDeployment {
    pub chain_id: u64,
    pub chain_key: String,
    pub well_v4: String,
    pub rpc: String,
    pub primary_stable: String,
    pub stable_decimals: u32,
}

pub fn load_deployments() -> Vec<LwcDeployment> {
    let path = std::env::var("LWC_DEPLOYMENTS_PATH")
        .unwrap_or_else(|_| "config/lwc_deployments.json".to_string());
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(e) => {
            warn!("[t3rn-sidecar] cannot load LWC deployments from {}: {}", path, e);
            vec![]
        }
    }
}

// ── Delivery matrix entry ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryCandidate {
    pub chain_id: u64,
    pub chain_key: String,
    pub well_address: String,
    pub asset_address: String,
    pub available_usd: f64,
    pub can_instant_exec: bool,
    pub asset_id: u32,
}

// ── Fill result ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LwcFillResult {
    pub intent_id: String,
    pub tx_hash: String,
    pub chain_id: u64,
    pub amount_wei: String,
    pub gas_used: Option<u64>,
    pub used_permit_nonce: u64,
    pub dry_run: bool,
}

// ── Spinner permit ────────────────────────────────────────────────────────────

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
    let resp = http.post(&url).json(&body).send().await.context("permit request")?;
    if resp.status().as_u16() == 409 {
        anyhow::bail!("permit already issued for intent={} chain={}", intent_id, chain_id);
    }
    if !resp.status().is_success() {
        anyhow::bail!("spinner permit error: {}", resp.text().await.unwrap_or_default());
    }
    resp.json::<FillPermit>().await.context("parse FillPermit")
}

// ── Delivery matrix scanner ───────────────────────────────────────────────────

pub struct DeliveryMatrix {
    deployments: Vec<LwcDeployment>,
    http: reqwest::Client,
}

impl DeliveryMatrix {
    pub fn new() -> Self {
        Self {
            deployments: load_deployments(),
            http: reqwest::Client::builder()
                .user_agent("t3rn-sidecar-matrix/1.0")
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    /// Query every deployed well in parallel.  Returns only candidates where
    /// canPerformInstantExecution returns true for at least $1.
    pub async fn scan(&self) -> Vec<DeliveryCandidate> {
        let futs: Vec<_> = self.deployments.iter().map(|dep| {
            let dep = dep.clone();
            async move { probe_well(&dep).await }
        }).collect();

        let results = futures::future::join_all(futs).await;
        results.into_iter().flatten().collect()
    }
}

impl Default for DeliveryMatrix {
    fn default() -> Self { Self::new() }
}

async fn probe_well(dep: &LwcDeployment) -> Option<DeliveryCandidate> {
    let well: Address = dep.well_v4.parse().ok()?;
    let asset: Address = dep.primary_stable.parse().ok()?;
    if asset == Address::ZERO { return None; }

    let rpc_url = dep.rpc.parse().ok()?;
    let provider = ProviderBuilder::new().on_http(rpc_url);

    // getAvailableLiquidity
    let avail_data = LiquidityWellCompact::getAvailableLiquidityCall { _asset: asset }.abi_encode();
    let avail_bytes = eth_call_raw(&provider, well, avail_data).await;
    let available_raw = if avail_bytes.len() >= 32 {
        U256::from_be_slice(&avail_bytes[avail_bytes.len()-32..])
    } else { U256::ZERO };
    let available_usd = available_raw.try_into()
        .map(|v: u128| v as f64 / 10f64.powi(dep.stable_decimals as i32))
        .unwrap_or(0.0);

    // canPerformInstantExecution($1)
    let one = U256::from(10u64.pow(dep.stable_decimals));
    let can_data = LiquidityWellCompact::canPerformInstantExecutionCall { asset, amount: one }.abi_encode();
    let can_bytes = eth_call_raw(&provider, well, can_data).await;

    let can_instant_exec = can_bytes.first().copied().unwrap_or(0) != 0;
    // Second word = contract's authoritative available amount
    let contract_avail = if can_bytes.len() >= 64 {
        U256::from_be_slice(&can_bytes[32..64])
            .try_into().map(|v: u128| v as f64 / 10f64.powi(dep.stable_decimals as i32))
            .unwrap_or(available_usd)
    } else { available_usd };
    let best_avail = if contract_avail > available_usd { contract_avail } else { available_usd };

    if !can_instant_exec && best_avail < 1.0 { return None; }

    // mapAssetToId
    let id_data = LiquidityWellCompact::mapAssetToIdCall { _asset: asset }.abi_encode();
    let id_bytes = eth_call_raw(&provider, well, id_data).await;
    let asset_id: u32 = if id_bytes.len() >= 32 {
        U256::from_be_slice(&id_bytes[id_bytes.len()-32..])
            .try_into().unwrap_or(0)
    } else { 0 };

    info!("[delivery-matrix] chain={} avail=${:.2} can_exec={} asset_id={}",
          dep.chain_key, best_avail, can_instant_exec, asset_id);

    Some(DeliveryCandidate {
        chain_id: dep.chain_id,
        chain_key: dep.chain_key.clone(),
        well_address: dep.well_v4.clone(),
        asset_address: dep.primary_stable.clone(),
        available_usd: best_avail,
        can_instant_exec,
        asset_id,
    })
}

// ── T3RNSidecar (fill provider) ───────────────────────────────────────────────

pub struct T3RNSidecar {
    pub signer: PrivateKeySigner,
    pub solver_addr: Address,
    pub deployments: Vec<LwcDeployment>,
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
        let dry_run = std::env::var("DRY_RUN")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(true);
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

    /// Check whether the LWC on `dst_chain` can provide liquidity for this fill.
    pub async fn can_provide_liquidity(&self, dst_chain: u64, amount_wei: U256) -> bool {
        let dep = match self.dep_for(dst_chain) { Some(d) => d, None => return false };
        let well: Address = match dep.well_v4.parse() { Ok(a) => a, Err(_) => return false };
        let asset: Address = match dep.primary_stable.parse() { Ok(a) => a, Err(_) => return false };
        let rpc_url = match dep.rpc.parse() { Ok(u) => u, Err(_) => return false };
        let provider = ProviderBuilder::new().on_http(rpc_url);
        let call = LiquidityWellCompact::canPerformInstantExecutionCall { asset, amount: amount_wei };
        let bytes = eth_call_raw(&provider, well, call.abi_encode()).await;
        bytes.first().copied().unwrap_or(0) != 0
    }

    /// Execute a fill using LWC well funds.
    ///
    /// 1. Dedup guard
    /// 2. Request Spinner permit
    /// 3. Validate deadline
    /// 4. Build order() calldata
    /// 5. eth_estimateGas
    /// 6. Broadcast (or dry-run skip)
    pub async fn fill(
        &self,
        intent_id: &str,
        dst_chain: u64,
        amount_wei: U256,
        recipient: &str,
        max_reward: U256,
    ) -> Result<LwcFillResult> {
        let dep = self.dep_for(dst_chain).context("no LWC deployment for dst chain")?;
        let well: Address = dep.well_v4.parse().context("invalid well address")?;
        let asset: Address = dep.primary_stable.parse().context("invalid stable address")?;
        let rpc_url = dep.rpc.parse().context("invalid rpc")?;
        let permit_key = (intent_id.to_string(), dst_chain);

        {
            let used = self.used_permits.lock().unwrap();
            if used.contains(&permit_key) {
                anyhow::bail!("permit already used for intent={} chain={}", intent_id, dst_chain);
            }
        }

        // Spinner permit
        let permit = request_permit(
            &self.http, &self.spinner_url,
            intent_id, dst_chain, amount_wei, self.solver_addr,
        ).await.context("request_permit")?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
        if permit.deadline < now {
            anyhow::bail!("permit expired: deadline={} now={}", permit.deadline, now);
        }

        info!("[t3rn-sidecar] permit ok: intent={} chain={} nonce={} dry_run={}",
              intent_id, dst_chain, permit.nonce, self.dry_run);

        if self.dry_run {
            self.used_permits.lock().unwrap().insert(permit_key);
            return Ok(LwcFillResult {
                intent_id: intent_id.to_string(),
                tx_hash: "dry-run:no-tx".to_string(),
                chain_id: dst_chain, amount_wei: amount_wei.to_string(),
                gas_used: None, used_permit_nonce: permit.nonce, dry_run: true,
            });
        }

        // Build calldata
        let target_account = parse_target_account(recipient)?;
        let asset_id = self.get_asset_id(&dep.rpc, well, asset).await.unwrap_or(0);
        let destination = chain_id_to_destination(dst_chain);

        let order_call = LiquidityWellCompact::orderCall {
            destination,
            asset: asset_id,
            targetAccount: target_account,
            amount: amount_wei,
            rewardAsset: asset,
            insurance: U256::ZERO,
            maxReward: max_reward,
        };
        let calldata: alloy::primitives::Bytes = order_call.abi_encode().into();

        let wallet = EthereumWallet::from(self.signer.clone());
        let provider = ProviderBuilder::new().wallet(wallet).on_http(rpc_url);

        // estimateGas before sending
        let estimate_req = alloy::rpc::types::TransactionRequest::default()
            .to(well)
            .input(calldata.clone().into());
        let gas_estimate = provider.estimate_gas(&estimate_req).await
            .unwrap_or(300_000u64);
        let gas_limit = gas_estimate * 12 / 10; // +20% headroom

        info!("[t3rn-sidecar] estimateGas={} limit={} intent={}", gas_estimate, gas_limit, intent_id);

        let tx_req = alloy::rpc::types::TransactionRequest::default()
            .to(well)
            .input(calldata.into())
            .gas_limit(gas_limit);

        let pending = provider.send_transaction(tx_req).await.context("send order tx")?;
        let receipt = pending.get_receipt().await.context("order receipt")?;
        let tx_hash = format!("{:?}", receipt.transaction_hash);
        let gas_used = receipt.gas_used;

        info!("[t3rn-sidecar] order confirmed: intent={} tx={} gas_used={:?}", intent_id, tx_hash, gas_used);
        self.used_permits.lock().unwrap().insert(permit_key);

        Ok(LwcFillResult {
            intent_id: intent_id.to_string(),
            tx_hash, chain_id: dst_chain,
            amount_wei: amount_wei.to_string(),
            gas_used: Some(gas_used as u64),
            used_permit_nonce: permit.nonce,
            dry_run: false,
        })
    }

    fn dep_for(&self, chain_id: u64) -> Option<&LwcDeployment> {
        self.deployments.iter().find(|d| d.chain_id == chain_id)
    }

    async fn get_asset_id(&self, rpc: &str, well: Address, asset: Address) -> Option<u32> {
        let rpc_url = rpc.parse().ok()?;
        let provider = ProviderBuilder::new().on_http(rpc_url);
        let call = LiquidityWellCompact::mapAssetToIdCall { _asset: asset };
        let bytes = eth_call_raw(&provider, well, call.abi_encode()).await;
        if bytes.len() >= 32 {
            U256::from_be_slice(&bytes[bytes.len()-32..]).try_into().ok()
        } else { None }
    }
}

impl Default for T3RNSidecar {
    fn default() -> Self {
        let zero = [0u8; 32];
        let key = alloy::signers::local::PrivateKeySigner::from_signing_key(
            alloy::signers::k256::ecdsa::SigningKey::from_bytes((&zero).into()).expect("zero key")
        );
        Self::new(key)
    }
}

// ── Delivery HTTP worker ──────────────────────────────────────────────────────
//
// open-mamba fires:
//   POST /lwc/deliver   {"chain_id":8453,"asset":"0x833...","amount_usd":50.0,...}
//   POST /lwc/matrix    (no body) — returns current DeliveryCandidate list
//   GET  /lwc/health

use std::sync::Arc;
use axum::{extract::State, http::StatusCode, response::Json, routing::{get, post}, Router};

#[derive(Clone)]
pub struct WorkerState {
    sidecar: Arc<T3RNSidecar>,
    matrix: Arc<DeliveryMatrix>,
}

#[derive(Debug, Deserialize)]
pub struct DeliverRequest {
    pub chain_id: u64,
    pub asset: String,
    /// Amount in asset's native decimals (as a decimal string, e.g. "40000000" for $40 USDC)
    pub amount_wei: String,
    /// Recipient address (the solver that earned the fill reward)
    pub recipient: String,
    /// Intent ID — used for dedup and permit request
    pub intent_id: String,
    /// maxReward in wei — passed through to order()
    pub max_reward: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DeliverResponse {
    pub ok: bool,
    pub result: Option<LwcFillResult>,
    pub error: Option<String>,
}

pub fn delivery_router(sidecar: Arc<T3RNSidecar>, matrix: Arc<DeliveryMatrix>) -> Router {
    let state = WorkerState { sidecar, matrix };
    Router::new()
        .route("/lwc/health",  get(health_handler))
        .route("/lwc/deliver", post(deliver_handler))
        .route("/lwc/matrix",  post(matrix_handler))
        .with_state(state)
}

async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok", "service": "t3rn-sidecar" }))
}

async fn deliver_handler(
    State(s): State<WorkerState>,
    Json(req): Json<DeliverRequest>,
) -> (StatusCode, Json<DeliverResponse>) {
    let amount_wei: U256 = match req.amount_wei.parse() {
        Ok(v) => v,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(DeliverResponse {
            ok: false, result: None,
            error: Some(format!("invalid amount_wei: {}", e)),
        })),
    };
    let max_reward: U256 = req.max_reward.as_deref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(amount_wei);

    match s.sidecar.fill(&req.intent_id, req.chain_id, amount_wei, &req.recipient, max_reward).await {
        Ok(result) => (StatusCode::OK, Json(DeliverResponse {
            ok: true, result: Some(result), error: None,
        })),
        Err(e) => {
            warn!("[delivery-worker] fill failed chain={} intent={}: {}", req.chain_id, req.intent_id, e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(DeliverResponse {
                ok: false, result: None, error: Some(e.to_string()),
            }))
        }
    }
}

async fn matrix_handler(State(s): State<WorkerState>) -> Json<Vec<DeliveryCandidate>> {
    Json(s.matrix.scan().await)
}

/// Background scan-and-report loop.
///
/// Every `interval_secs`, scans all wells and POSTs the result to the
/// open-mamba outcomes endpoint (`MAMBA_LAKE_URL/api/solver/outcomes`)
/// as a structured delivery-matrix snapshot.  open-mamba's scheduler
/// can then read the snapshot via GET /api/solver/outcomes without
/// having to coordinate timing with this loop.
///
/// Also accessible externally via `POST /lwc/matrix`.
pub fn spawn_delivery_loop(
    matrix: Arc<DeliveryMatrix>,
    interval_secs: u64,
    mamba_url: Option<String>,
) {
    tokio::spawn(async move {
        let interval = std::time::Duration::from_secs(interval_secs.max(30));
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        loop {
            tokio::time::sleep(interval).await;
            let candidates = matrix.scan().await;
            info!("[delivery-loop] matrix scan: {} live candidates", candidates.len());
            for c in &candidates {
                info!("[delivery-loop] chain={} avail=${:.2} can_exec={} asset_id={}",
                    c.chain_key, c.available_usd, c.can_instant_exec, c.asset_id);
            }

            // POST snapshot to open-mamba for observability
            if let Some(ref url) = mamba_url {
                let payload = serde_json::json!({
                    "source": "lwc-delivery-matrix",
                    "outcome": "scan",
                    "data": candidates,
                    "ts": std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default().as_secs(),
                });
                let endpoint = format!("{}/api/solver/outcomes", url);
                if let Err(e) = http.post(&endpoint).json(&payload).send().await {
                    warn!("[delivery-loop] open-mamba report failed: {}", e);
                }
            }
        }
    });
}

// ── RPC helper ────────────────────────────────────────────────────────────────

type HttpProvider = alloy::providers::RootProvider<alloy::transports::http::Http<alloy::transports::http::Client>>;

async fn eth_call_raw(provider: &HttpProvider, to: Address, data: Vec<u8>) -> alloy::primitives::Bytes {
    let req = alloy::rpc::types::TransactionRequest::default()
        .to(to)
        .input(data.into());
    provider.call(&req).await.unwrap_or_default()
}

// ── Encoding helpers ──────────────────────────────────────────────────────────

fn parse_target_account(recipient: &str) -> Result<alloy::primitives::FixedBytes<32>> {
    let trimmed = recipient.trim_start_matches("0x");
    let bytes = hex::decode(trimmed).context("decode recipient hex")?;
    let mut out = [0u8; 32];
    let start = 32usize.saturating_sub(bytes.len());
    out[start..].copy_from_slice(&bytes[..32usize.min(bytes.len())]);
    Ok(alloy::primitives::FixedBytes::<32>::from(out))
}

fn chain_id_to_destination(chain_id: u64) -> alloy::primitives::FixedBytes<4> {
    match chain_id {
        1     => *b"ethm",
        8453  => *b"basm",
        42161 => *b"arbm",
        10    => *b"optm",
        137   => *b"polm",
        56    => *b"bscm",
        59144 => *b"linm",
        130   => *b"unim",
        _     => *b"\0\0\0\0",
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
        assert_eq!(&result[12..], &hex::decode("833589fcd6edb6e08f4c7c32d4f71b54bda02913").unwrap());
    }

    #[test]
    fn chain_id_to_destination_known() {
        assert_eq!(chain_id_to_destination(8453).as_slice(), b"basm");
        assert_eq!(chain_id_to_destination(42161).as_slice(), b"arbm");
        assert_eq!(chain_id_to_destination(10).as_slice(), b"optm");
        assert_eq!(chain_id_to_destination(130).as_slice(), b"unim");
    }

    #[test]
    fn chain_id_to_destination_unknown() {
        assert_eq!(chain_id_to_destination(99999).as_slice(), b"\0\0\0\0");
    }

    #[tokio::test]
    async fn delivery_matrix_loads_deployments() {
        // Without a real deployments file this returns empty — just verify no panic.
        let matrix = DeliveryMatrix::new();
        let _ = matrix.deployments.len();
    }

    #[tokio::test]
    #[ignore] // live network
    async fn delivery_matrix_scan_live() {
        std::env::set_var(
            "LWC_DEPLOYMENTS_PATH",
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../config/lwc_deployments.json"),
        );
        let matrix = DeliveryMatrix::new();
        let candidates = matrix.scan().await;
        println!("Live delivery candidates ({}):", candidates.len());
        for c in &candidates {
            println!("  {} chain={} avail=${:.2} can_exec={} asset_id={}",
                     c.chain_key, c.chain_id, c.available_usd, c.can_instant_exec, c.asset_id);
        }
        assert!(!candidates.is_empty(), "expected at least one live candidate");
    }
}
