//! LWC (LiquidityWellCompact V4) manager.
//!
//! Reads live well state across all deployed chains and provides add/remove
//! liquidity operations so the rebalancer can top up or withdraw from the
//! t3rn liquidity wells using the solver's own surplus funds.

use alloy::{
    network::EthereumWallet,
    primitives::{Address, U256},
    providers::{Provider, ProviderBuilder, RootProvider},
    transports::http::{Client, Http},
    signers::local::PrivateKeySigner,
    sol,
    sol_types::SolCall,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::tx_guard::TxGuard;

// ── V4 LWC ABI — loaded from the canonical JSON shipped with portal-2026 ──────
//
// The JSON file is the single source of truth for all function signatures.
// We keep a minimal sol! block here only for the ERC-20 helpers (approve /
// allowance) which are not in the LWC ABI file.

sol! {
    #[sol(rpc)]
    #[derive(Debug)]
    LiquidityWellCompact,
    "src/lwc_abi.json"
}

sol! {
    function approve(address spender, uint256 amount) external returns (bool);
    function allowance(address owner, address spender) external view returns (uint256);
}

// ── Config ────────────────────────────────────────────────────────────────────

/// One row from config/lwc_deployments.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LwcDeployment {
    pub chain_id: u64,
    pub chain_key: String,
    pub well_v4: String,
    pub rpc: String,
    pub primary_stable: String,
    pub stable_decimals: u32,
}

/// Load deployments from the JSON config file.
pub fn load_deployments() -> Vec<LwcDeployment> {
    let path = std::env::var("LWC_DEPLOYMENTS_PATH")
        .unwrap_or_else(|_| "config/lwc_deployments.json".to_string());

    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            warn!("Could not read LWC deployments from {}: {} — using empty list", path, e);
            return vec![];
        }
    };

    match serde_json::from_str::<Vec<LwcDeployment>>(&raw) {
        Ok(v) => v,
        Err(e) => {
            warn!("Failed to parse LWC deployments: {} — using empty list", e);
            vec![]
        }
    }
}

// ── State ─────────────────────────────────────────────────────────────────────

/// Live snapshot of one chain's well state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LwcChainState {
    pub chain_id: u64,
    pub chain_key: String,
    pub well_address: String,
    /// Total stables available for instant execution (USD).
    pub pool_available_usd: f64,
    /// Total stables in the well including reserved (USD).
    pub pool_total_usd: f64,
    /// Solver's own LP position in USD.
    pub lp_balance_usd: f64,
    /// Whether either ingress or egress is halted.
    pub is_halted: bool,
    /// Whether `canPerformInstantExecution` returned true for $1.
    pub can_instant_exec: bool,
}

/// Classification of a well's liquidity depth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LwcStatus {
    /// Well available > threshold, not halted — can be used for fills.
    Healthy,
    /// Available below threshold but > 0 — may run out soon.
    LowPool,
    /// Available == 0 or canInstantExec returned false.
    EmptyPool,
    /// isIngressHalted or isEgressHalted is true.
    Halted,
    /// No contract deployed on this chain.
    NotDeployed,
}

impl LwcChainState {
    pub fn status(&self) -> LwcStatus {
        if self.is_halted {
            return LwcStatus::Halted;
        }
        if !self.can_instant_exec || self.pool_available_usd <= 0.0 {
            return LwcStatus::EmptyPool;
        }
        const LOW_POOL_THRESHOLD_USD: f64 = 100.0;
        if self.pool_available_usd < LOW_POOL_THRESHOLD_USD {
            LwcStatus::LowPool
        } else {
            LwcStatus::Healthy
        }
    }
}

// ── Manager ───────────────────────────────────────────────────────────────────

pub struct LwcManager {
    pub deployments: Vec<LwcDeployment>,
    solver_addr: Address,
    signer: PrivateKeySigner,
    pub dry_run: bool,
    http: reqwest::Client,
}

impl LwcManager {
    pub fn new(solver_key: PrivateKeySigner, dry_run: bool) -> Self {
        let solver_addr = solver_key.address();
        Self {
            deployments: load_deployments(),
            solver_addr,
            signer: solver_key,
            dry_run,
            http: reqwest::Client::builder()
                .user_agent("taifoon-lwc-manager/1.0")
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    /// Scan all deployed wells in parallel and return their states.
    pub async fn scan_all(&self) -> Vec<LwcChainState> {
        let futs = self.deployments.iter().map(|d| {
            let solver = self.solver_addr;
            let http = self.http.clone();
            async move { scan_well(&http, d, solver).await }
        });
        futures::future::join_all(futs).await
    }

    /// Check whether a specific well can execute a fill of `amount_wei` of `asset`.
    pub async fn can_instant_exec(&self, chain_id: u64, asset: Address, amount_wei: U256) -> bool {
        let dep = match self.deployments.iter().find(|d| d.chain_id == chain_id) {
            Some(d) => d,
            None => return false,
        };
        let well: Address = match dep.well_v4.parse() {
            Ok(a) => a,
            Err(_) => return false,
        };
        let rpc_url: alloy::transports::http::reqwest::Url = match dep.rpc.parse() {
            Ok(u) => u,
            Err(_) => return false,
        };
        let provider: HttpProvider = ProviderBuilder::new().on_http(rpc_url);
        let call = LiquidityWellCompact::canPerformInstantExecutionCall { asset, amount: amount_wei };
        eth_call_bool(&provider, well, call.abi_encode()).await
    }

    /// Add liquidity to a well. Approves the stable first if needed.
    pub async fn add_liquidity(
        &self,
        chain_id: u64,
        asset: Address,
        amount_wei: U256,
    ) -> Result<String> {
        let dep = self.deployments.iter()
            .find(|d| d.chain_id == chain_id)
            .context("no LWC deployment for chain")?;

        let well: Address = dep.well_v4.parse().context("invalid well address")?;
        let rpc_url = dep.rpc.parse().context("invalid rpc url")?;

        info!(
            "[LWC] add_liquidity chain={} well={} amount_wei={} dry_run={}",
            chain_id, dep.well_v4, amount_wei, self.dry_run
        );

        if self.dry_run {
            return Ok("dry-run:no-tx".to_string());
        }

        let wallet = EthereumWallet::from(self.signer.clone());
        let provider = ProviderBuilder::new()
            .wallet(wallet)
            .on_http(rpc_url);

        // Approve the well if allowance is insufficient (ERC-20 path; skip for native).
        let zero = Address::ZERO;
        if asset != zero {
            let allowance_call = allowanceCall { owner: self.solver_addr, spender: well };
            let allowance_data: alloy::rpc::types::TransactionRequest =
                alloy::rpc::types::TransactionRequest::default()
                    .to(asset)
                    .input(allowance_call.abi_encode().into());
            let result = provider.call(&allowance_data).await.unwrap_or_default();
            let current_allowance = if result.len() >= 32 {
                U256::from_be_slice(&result[result.len() - 32..])
            } else {
                U256::ZERO
            };

            if current_allowance < amount_wei {
                let approve_call = approveCall { spender: well, amount: U256::MAX };
                let approve_req = alloy::rpc::types::TransactionRequest::default()
                    .to(asset)
                    .input(approve_call.abi_encode().into());
                let pending = provider.send_transaction(approve_req).await
                    .context("approve tx failed")?;
                let receipt = pending.get_receipt().await.context("approve receipt failed")?;
                if !receipt.status() {
                    anyhow::bail!("[LWC] approve tx reverted for asset {}", asset);
                }
                info!("[LWC] approved asset {} for well: {:?}", asset, receipt.transaction_hash);
            }
        }

        // Call addLiquidity
        let add_call = LiquidityWellCompact::addLiquidityCall { _asset: asset, _amount: amount_wei };
        let calldata = add_call.abi_encode();

        // Guard: to must be a known LWC well address
        TxGuard::from_deployments(self.solver_addr)
            .enforce(well, &calldata, &[])
            .context("tx_guard blocked add_liquidity")?;

        let value = if asset == zero { amount_wei } else { U256::ZERO };
        let mut req = alloy::rpc::types::TransactionRequest::default()
            .to(well)
            .input(calldata.into());
        if value > U256::ZERO {
            req = req.value(value);
        }

        let pending = provider.send_transaction(req).await.context("addLiquidity tx failed")?;
        let receipt = pending.get_receipt().await.context("addLiquidity receipt failed")?;
        let hash = format!("{:?}", receipt.transaction_hash);
        if !receipt.status() {
            anyhow::bail!("[LWC] addLiquidity reverted on-chain (chain={} tx={})", chain_id, hash);
        }
        info!("[LWC] addLiquidity confirmed: chain={} tx={}", chain_id, hash);
        Ok(hash)
    }

    /// Remove liquidity from a well (reclaim the solver's LP share).
    pub async fn remove_liquidity(
        &self,
        chain_id: u64,
        asset: Address,
        amount_wei: U256,
    ) -> Result<String> {
        let dep = self.deployments.iter()
            .find(|d| d.chain_id == chain_id)
            .context("no LWC deployment for chain")?;

        let well: Address = dep.well_v4.parse().context("invalid well address")?;
        let rpc_url = dep.rpc.parse().context("invalid rpc url")?;

        info!(
            "[LWC] remove_liquidity chain={} well={} amount_wei={} dry_run={}",
            chain_id, dep.well_v4, amount_wei, self.dry_run
        );

        if self.dry_run {
            return Ok("dry-run:no-tx".to_string());
        }

        let wallet = EthereumWallet::from(self.signer.clone());
        let provider = ProviderBuilder::new()
            .wallet(wallet)
            .on_http(rpc_url);

        let rm_call = LiquidityWellCompact::removeLiquidityCall { _asset: asset, _amount: amount_wei };
        let calldata = rm_call.abi_encode();

        // Guard: to must be a known LWC well address
        TxGuard::from_deployments(self.solver_addr)
            .enforce(well, &calldata, &[])
            .context("tx_guard blocked remove_liquidity")?;

        let req = alloy::rpc::types::TransactionRequest::default()
            .to(well)
            .input(calldata.into());

        let pending = provider.send_transaction(req).await.context("removeLiquidity tx failed")?;
        let receipt = pending.get_receipt().await.context("removeLiquidity receipt failed")?;
        let hash = format!("{:?}", receipt.transaction_hash);
        if !receipt.status() {
            anyhow::bail!("[LWC] removeLiquidity reverted on-chain (chain={} tx={})", chain_id, hash);
        }
        info!("[LWC] removeLiquidity confirmed: chain={} tx={}", chain_id, hash);
        Ok(hash)
    }
}

// ── Per-chain scan ─────────────────────────────────────────────────────────────

async fn scan_well(
    _http: &reqwest::Client,
    dep: &LwcDeployment,
    solver: Address,
) -> LwcChainState {
    let zero = LwcChainState {
        chain_id: dep.chain_id,
        chain_key: dep.chain_key.clone(),
        well_address: dep.well_v4.clone(),
        pool_available_usd: 0.0,
        pool_total_usd: 0.0,
        lp_balance_usd: 0.0,
        is_halted: false,
        can_instant_exec: false,
    };

    let well: Address = match dep.well_v4.parse() {
        Ok(a) => a,
        Err(_) => {
            warn!("[LWC] bad well address for chain {}", dep.chain_id);
            return zero;
        }
    };
    let rpc_url: alloy::transports::http::reqwest::Url = match dep.rpc.parse() {
        Ok(u) => u,
        Err(_) => return zero,
    };
    let provider: HttpProvider = ProviderBuilder::new().on_http(rpc_url);
    let asset: Address = dep.primary_stable.parse().unwrap_or(Address::ZERO);
    let dec = dep.stable_decimals;

    // isEgressHalted + isIngressHalted
    let egress_halted = eth_call_bool(&provider, well, LiquidityWellCompact::isEgressHaltedCall {}.abi_encode()).await;
    let ingress_halted = eth_call_bool(&provider, well, LiquidityWellCompact::isIngressHaltedCall {}.abi_encode()).await;
    let is_halted = egress_halted || ingress_halted;

    // getAvailableLiquidity — the primary_stable balance available for fills.
    let available_raw = if asset != Address::ZERO {
        eth_call_u256(&provider, well, LiquidityWellCompact::getAvailableLiquidityCall { _asset: asset }.abi_encode()).await
    } else {
        U256::ZERO
    };
    let pool_available_usd = u256_to_usd(available_raw, dec);

    // getCurrentLiquidityInWell returns an internal accounting unit (not USD).
    // We intentionally set pool_total_usd = pool_available_usd as the best
    // approximation we can get without a price oracle, keeping the field
    // consistent across all chains.
    let _pool_total_usd = pool_available_usd;

    // getLPTokenBalance — need assetId first
    let lp_balance_usd = if asset != Address::ZERO {
        let asset_id_raw = eth_call_u256(
            &provider, well,
            LiquidityWellCompact::mapAssetToIdCall { _asset: asset }.abi_encode()
        ).await;
        let asset_id: u32 = asset_id_raw.try_into().unwrap_or(0);
        let lp_raw = eth_call_u256(
            &provider, well,
            LiquidityWellCompact::getLPTokenBalanceCall { _assetId: asset_id, _account: solver }.abi_encode()
        ).await;
        u256_to_usd(lp_raw, dec)
    } else {
        0.0
    };

    // canPerformInstantExecution — probe with $1 to get (canExec, available, reserved).
    // The `available` return value is authoritative; use it to override pool_available_usd
    // when it's more precise (e.g. when getAvailableLiquidity returns 0 due to rounding).
    let one_dollar_wei = U256::from(10u64.pow(dec));
    let (can_instant_exec, pool_available_usd) = if asset != Address::ZERO && !is_halted {
        let result = eth_call_bytes(
            &provider, well,
            LiquidityWellCompact::canPerformInstantExecutionCall { asset, amount: one_dollar_wei }.abi_encode()
        ).await;
        let can_exec = result.first().copied().unwrap_or(0) != 0;
        // Second word (bytes 32..64) is the available amount from the contract.
        let contract_available = if result.len() >= 64 {
            u256_to_usd(U256::from_be_slice(&result[32..64]), dec)
        } else {
            pool_available_usd
        };
        // Use whichever is larger — getAvailableLiquidity sometimes misses reserved amounts.
        let best_available = if contract_available > pool_available_usd { contract_available } else { pool_available_usd };
        (can_exec, best_available)
    } else {
        (false, pool_available_usd)
    };

    LwcChainState {
        chain_id: dep.chain_id,
        chain_key: dep.chain_key.clone(),
        well_address: dep.well_v4.clone(),
        pool_available_usd,
        pool_total_usd: pool_available_usd, // keep in sync
        lp_balance_usd,
        is_halted,
        can_instant_exec,
    }
}

// ── RPC helpers ───────────────────────────────────────────────────────────────

type HttpProvider = RootProvider<Http<Client>>;

async fn eth_call_bool(provider: &HttpProvider, to: Address, data: Vec<u8>) -> bool {
    let req = alloy::rpc::types::TransactionRequest::default()
        .to(to)
        .input(data.into());
    match provider.call(&req).await {
        Ok(bytes) if bytes.len() >= 32 => bytes[31] != 0,
        _ => false,
    }
}

async fn eth_call_u256(provider: &HttpProvider, to: Address, data: Vec<u8>) -> U256 {
    let req = alloy::rpc::types::TransactionRequest::default()
        .to(to)
        .input(data.into());
    match provider.call(&req).await {
        Ok(bytes) if bytes.len() >= 32 => U256::from_be_slice(&bytes[bytes.len() - 32..]),
        _ => U256::ZERO,
    }
}

async fn eth_call_bytes(provider: &HttpProvider, to: Address, data: Vec<u8>) -> Vec<u8> {
    let req = alloy::rpc::types::TransactionRequest::default()
        .to(to)
        .input(data.into());
    provider.call(&req).await.map(|b| b.to_vec()).unwrap_or_default()
}

fn u256_to_usd(raw: U256, decimals: u32) -> f64 {
    if raw.is_zero() { return 0.0; }
    // Convert to f64 via the lower 128 bits (safe for any realistic stable balance)
    let lo: u128 = raw.try_into().unwrap_or(u128::MAX);
    lo as f64 / 10f64.powi(decimals as i32)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lwc_status_healthy() {
        let s = LwcChainState {
            chain_id: 8453, chain_key: "base".into(), well_address: "0x0".into(),
            pool_available_usd: 500.0, pool_total_usd: 600.0,
            lp_balance_usd: 100.0, is_halted: false, can_instant_exec: true,
        };
        assert_eq!(s.status(), LwcStatus::Healthy);
    }

    #[test]
    fn lwc_status_low_pool() {
        let s = LwcChainState {
            chain_id: 8453, chain_key: "base".into(), well_address: "0x0".into(),
            pool_available_usd: 50.0, pool_total_usd: 80.0,
            lp_balance_usd: 30.0, is_halted: false, can_instant_exec: true,
        };
        assert_eq!(s.status(), LwcStatus::LowPool);
    }

    #[test]
    fn lwc_status_empty() {
        let s = LwcChainState {
            chain_id: 8453, chain_key: "base".into(), well_address: "0x0".into(),
            pool_available_usd: 0.0, pool_total_usd: 0.0,
            lp_balance_usd: 0.0, is_halted: false, can_instant_exec: false,
        };
        assert_eq!(s.status(), LwcStatus::EmptyPool);
    }

    #[test]
    fn lwc_status_halted() {
        let s = LwcChainState {
            chain_id: 8453, chain_key: "base".into(), well_address: "0x0".into(),
            pool_available_usd: 500.0, pool_total_usd: 600.0,
            lp_balance_usd: 100.0, is_halted: true, can_instant_exec: false,
        };
        assert_eq!(s.status(), LwcStatus::Halted);
    }

    #[test]
    fn load_deployments_returns_list_or_empty() {
        // Should not panic even if the file is absent
        let deps = load_deployments();
        // In test environment the file may not be found; just assert no panic
        let _ = deps;
    }
}
