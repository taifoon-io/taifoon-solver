//! Cross-chain self-fill engine.
//!
//! Subscribes to `OrderMonitor` broadcasts. For each detected `OrderCreated`
//! event, checks whether the destination chain's LWC pool can fill the amount,
//! then builds and broadcasts an `order()` transaction using the gas razor.

use alloy::{
    network::EthereumWallet,
    primitives::{Address, U256},
    providers::{Provider, ProviderBuilder},
    sol_types::SolCall,
};
use anyhow::{Context, Result};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::{
    fills_log::{FillRecord, FillsLog},
    gas_razor::{self, resolve_rpc_url},
    load_deployments,
    order_monitor::T3rnOrder,
    LwcDeployment,
    TxGuard,
};
use portfolio_sidecar::lwc_manager::LiquidityWellCompact;

/// Map bytes4 destination tag → chain_id.
pub fn destination_to_chain_id(dest: &[u8; 4]) -> Option<u64> {
    match dest {
        b"eth." | b"ethm" => Some(1),
        b"arbm"           => Some(42161),
        b"basm"           => Some(8453),
        b"opti" | b"optm" => Some(10),
        b"poly" | b"polm" => Some(137),
        b"bnbs" | b"bscm" => Some(56),
        b"line" | b"linm" => Some(59144),
        b"unic" | b"unim" => Some(130),
        _                 => None,
    }
}

/// Map chain_id → source chain bytes4 (for order() destination parameter
/// from the destination chain's perspective — pointing back at the source).
fn chain_id_to_bytes4(chain_id: u64) -> [u8; 4] {
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
    }
}

pub struct SelfFill {
    signer:      alloy::signers::local::PrivateKeySigner,
    solver_addr: Address,
    deployments:  Vec<LwcDeployment>,
    dry_run:      bool,
    seen_orders:  Mutex<HashSet<[u8; 32]>>,
    fills_count:  Mutex<u64>,
    fills_log:    Option<FillsLog>,
}

impl SelfFill {
    pub fn new(
        signer: alloy::signers::local::PrivateKeySigner,
        _rx: broadcast::Receiver<T3rnOrder>,
    ) -> Arc<Self> {
        Self::with_log(signer, _rx, None)
    }

    pub fn with_log(
        signer: alloy::signers::local::PrivateKeySigner,
        _rx: broadcast::Receiver<T3rnOrder>,
        fills_log: Option<FillsLog>,
    ) -> Arc<Self> {
        let solver_addr = signer.address();
        let dry_run = std::env::var("DRY_RUN")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(true);

        Arc::new(Self {
            signer,
            solver_addr,
            deployments: load_deployments(),
            dry_run,
            seen_orders: Mutex::new(HashSet::new()),
            fills_count: Mutex::new(0),
            fills_log,
        })
    }

    /// Spawn the background consumer. Takes ownership of the receiver.
    pub fn start(self: Arc<Self>, mut rx: broadcast::Receiver<T3rnOrder>) {
        let engine = self.clone();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(order) => {
                        let eng = engine.clone();
                        tokio::spawn(async move {
                            if let Err(e) = eng.handle_order(order).await {
                                warn!("[self_fill] handle_order failed: {}", e);
                            }
                        });
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("[self_fill] lagged {} orders, some skipped", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        info!("[self_fill] channel closed, exiting");
                        break;
                    }
                }
            }
        });
    }

    /// Returns the count of fills executed this session.
    pub fn fills_count(&self) -> u64 {
        *self.fills_count.lock().unwrap()
    }

    async fn handle_order(&self, order: T3rnOrder) -> Result<()> {
        // Dedup
        {
            let mut seen = self.seen_orders.lock().unwrap();
            if seen.contains(&order.id) { return Ok(()); }
            seen.insert(order.id);
        }

        let dst_chain_id = match destination_to_chain_id(&order.destination) {
            Some(c) => c,
            None => {
                info!(
                    "[self_fill] unknown destination {:?}, skipping",
                    std::str::from_utf8(&order.destination).unwrap_or("????")
                );
                return Ok(());
            }
        };

        // Skip self-fills where source == destination
        if dst_chain_id == order.source_chain { return Ok(()); }

        let dep = match self.dep_for(dst_chain_id) {
            Some(d) => d,
            None => {
                info!("[self_fill] no deployment for dst chain={}", dst_chain_id);
                return Ok(());
            }
        };

        if dep.chain_id == 999 { return Ok(()); } // HyperEVM: no stable

        let well: Address = dep.well_v4.parse()?;
        let asset: Address = dep.primary_stable.parse()?;
        if asset == Address::ZERO { return Ok(()); }

        // Check canPerformInstantExecution on destination chain
        let can_fill = self.check_can_fill(dst_chain_id, well, asset, order.amount).await;
        if !can_fill {
            info!(
                "[self_fill] dst chain={} pool dry for amount={}, skipping id={}",
                dst_chain_id, order.amount, hex::encode(order.id)
            );
            return Ok(());
        }

        // Profitability: ensure max_reward (in stable units) covers estimated gas cost.
        // gas_cost_wei is in ETH/native wei (18 dec); max_reward is in the stable (6 dec USDC).
        // Convert gas to stable units using a conservative ETH price before comparing.
        let calldata_placeholder = alloy::primitives::Bytes::from(vec![0u8; 228]); // ~order() calldata size
        let gas_params = gas_razor::estimate(dst_chain_id, calldata_placeholder, well).await;
        let gas_cost_eth = gas_params.gas_limit as f64 * gas_params.max_fee_per_gas as f64 / 1e18;
        let eth_price_usd = std::env::var("ETH_PRICE_USD")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(2500.0);
        let gas_cost_stable = (gas_cost_eth * eth_price_usd * 10f64.powi(dep.stable_decimals as i32)) as u128;

        if order.max_reward.to::<u128>() <= gas_cost_stable {
            info!(
                "[self_fill] not profitable: max_reward={} <= gas_cost_stable={} (gas_eth={:.8}), skipping id={}",
                order.max_reward, gas_cost_stable, gas_cost_eth, hex::encode(order.id)
            );
            return Ok(());
        }

        // Build order() calldata for the destination chain
        let asset_id = self.get_asset_id(dst_chain_id, well, asset).await.unwrap_or(0);
        let target_account = alloy::primitives::FixedBytes::<32>::from(order.target_account);
        let src_chain_bytes4: alloy::primitives::FixedBytes<4> =
            chain_id_to_bytes4(order.source_chain).into();

        let order_call = LiquidityWellCompact::orderCall {
            destination: src_chain_bytes4,
            asset: asset_id,
            targetAccount: target_account,
            amount: order.amount,
            rewardAsset: asset,
            insurance: U256::ZERO, // dead field in LWC V4 — contract ignores it
            maxReward: order.max_reward,
        };
        let calldata: alloy::primitives::Bytes = order_call.abi_encode().into();

        // Re-estimate with real calldata
        let gas_params = gas_razor::estimate(dst_chain_id, calldata.clone(), well).await;

        info!(
            "[self_fill] id={} dst_chain={} amount={} gas_limit={} max_fee={} dry_run={}",
            hex::encode(order.id), dst_chain_id, order.amount,
            gas_params.gas_limit, gas_params.max_fee_per_gas, self.dry_run
        );

        let intent_id = hex::encode(order.id);
        let solver_id = format!("{:#x}", self.solver_addr);

        if self.dry_run {
            info!(
                "[self_fill] DRY_RUN — would fill order {} on chain {}, gas_limit={}, max_fee={}",
                intent_id, dst_chain_id,
                gas_params.gas_limit, gas_params.max_fee_per_gas
            );
            if let Some(log) = &self.fills_log {
                let _ = log.append(FillRecord {
                    ts: chrono::Utc::now(),
                    intent_id: intent_id.clone(),
                    protocol: "t3rn_lwc".to_string(),
                    src_chain: order.source_chain,
                    dst_chain: dst_chain_id,
                    decision: "dry_run".to_string(),
                    tx_hash: None,
                    predicted_gas: Some(gas_params.gas_limit),
                    gas_used: None,
                    effective_gas_price_wei: None,
                    actual_profit_usd: None,
                    skip_reason: None,
                    error: None,
                    solver_id: Some(solver_id.clone()),
                });
            }
            *self.fills_count.lock().unwrap() += 1;
            return Ok(());
        }

        // Build and broadcast the fill tx
        let rpc_url = resolve_rpc_url(dst_chain_id)
            .unwrap_or_else(|| dep.rpc.clone());

        // Guard: to must be the LWC well on the destination chain
        TxGuard::from_deployments(self.solver_addr)
            .enforce(well, &calldata, &[self.solver_addr])
            .context("tx_guard blocked self_fill order()")?;

        let wallet = EthereumWallet::from(self.signer.clone());
        let provider = ProviderBuilder::new()
            .wallet(wallet)
            .on_http(rpc_url.parse()?);

        let tx_req = alloy::rpc::types::TransactionRequest::default()
            .to(well)
            .input(calldata.into())
            .gas_limit(gas_params.gas_limit)
            .max_fee_per_gas(gas_params.max_fee_per_gas)
            .max_priority_fee_per_gas(gas_params.priority_fee);

        let pending = provider.send_transaction(tx_req).await?;
        let receipt = pending.get_receipt().await?;
        let tx_hash = format!("{:#x}", receipt.transaction_hash);

        info!(
            "[self_fill] filled: id={} dst_chain={} tx={} gas_used={}",
            intent_id, dst_chain_id, tx_hash, receipt.gas_used
        );

        if let Some(log) = &self.fills_log {
            // actual_profit = max_reward (stable units) - gas_cost (wei, ETH-denominated)
            // We express profit in USD: max_reward / 10^stable_decimals - gas_cost_eth * eth_price
            // For simplicity: max_reward is in USDC (6 dec), gas_cost_wei in chain native (18 dec).
            // Use 2500 USD/ETH as a safe constant — close enough for P&L tracking.
            let gas_cost_eth = receipt.gas_used as f64 * gas_params.max_fee_per_gas as f64 / 1e18;
            let gas_cost_usd = gas_cost_eth * std::env::var("ETH_PRICE_USD")
                .ok()
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(2500.0);
            let max_reward_usd = order.max_reward.to::<u128>() as f64
                / 10f64.powi(dep.stable_decimals as i32);
            let profit_usd = max_reward_usd - gas_cost_usd;
            let _ = log.append(FillRecord {
                ts: chrono::Utc::now(),
                intent_id: intent_id.clone(),
                protocol: "t3rn_lwc".to_string(),
                src_chain: order.source_chain,
                dst_chain: dst_chain_id,
                decision: "executed".to_string(),
                tx_hash: Some(tx_hash.clone()),
                predicted_gas: Some(gas_params.gas_limit),
                gas_used: Some(receipt.gas_used as u64),
                effective_gas_price_wei: Some(gas_params.max_fee_per_gas.to_string()),
                actual_profit_usd: Some(profit_usd),
                skip_reason: None,
                error: None,
                solver_id: Some(solver_id),
            });
        }

        *self.fills_count.lock().unwrap() += 1;
        Ok(())
    }

    fn dep_for(&self, chain_id: u64) -> Option<&LwcDeployment> {
        self.deployments.iter().find(|d| d.chain_id == chain_id)
    }

    async fn check_can_fill(&self, chain_id: u64, well: Address, asset: Address, amount: U256) -> bool {
        let rpc_url = match resolve_rpc_url(chain_id) {
            Some(u) => u,
            None => return false,
        };
        let provider = match rpc_url.parse::<reqwest::Url>() {
            Ok(u) => ProviderBuilder::new().on_http(u),
            Err(_) => return false,
        };
        let call = LiquidityWellCompact::canPerformInstantExecutionCall { asset, amount };
        let req = alloy::rpc::types::TransactionRequest::default()
            .to(well)
            .input(call.abi_encode().into());
        match provider.call(&req).await {
            Ok(bytes) => bytes.first().copied().unwrap_or(0) != 0,
            Err(_) => false,
        }
    }

    async fn get_asset_id(&self, chain_id: u64, well: Address, asset: Address) -> Option<u32> {
        let rpc_url = resolve_rpc_url(chain_id)?;
        let provider = ProviderBuilder::new().on_http(rpc_url.parse().ok()?);
        let call = LiquidityWellCompact::mapAssetToIdCall { _asset: asset };
        let req = alloy::rpc::types::TransactionRequest::default()
            .to(well)
            .input(call.abi_encode().into());
        let bytes = provider.call(&req).await.ok()?;
        if bytes.len() >= 32 {
            U256::from_be_slice(&bytes[bytes.len()-32..]).try_into().ok()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destination_mapping_roundtrip() {
        assert_eq!(destination_to_chain_id(b"arbm"), Some(42161));
        assert_eq!(destination_to_chain_id(b"basm"), Some(8453));
        assert_eq!(destination_to_chain_id(b"optm"), Some(10));
        assert_eq!(destination_to_chain_id(b"unim"), Some(130));
        assert_eq!(destination_to_chain_id(b"polm"), Some(137));
        assert_eq!(destination_to_chain_id(b"linm"), Some(59144));
        assert_eq!(destination_to_chain_id(b"\0\0\0\0"), None);
    }

    #[test]
    fn chain_id_to_bytes4_roundtrip() {
        assert_eq!(chain_id_to_bytes4(8453), *b"basm");
        assert_eq!(chain_id_to_bytes4(42161), *b"arbm");
    }
}
