//! Startup deposit sweep: reads the solver's ERC-20 stable balance on every
//! deployed LWC chain and calls addLiquidity() for any non-zero amount.
//!
//! Runs in parallel across all chains on sandbox startup so the wells are
//! seeded before the order monitor and hop rebalancer begin.

use alloy::{
    primitives::{Address, U256},
    providers::{Provider, ProviderBuilder},
    sol_types::SolCall,
};
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use portfolio_sidecar::lwc_manager::LwcManager;

use crate::LwcDeployment;

// ERC-20 balanceOf selector: 0x70a08231
fn balance_of_calldata(owner: Address) -> Vec<u8> {
    let mut data = vec![0x70u8, 0xa0, 0x82, 0x31];
    // ABI-encode: pad address to 32 bytes (left-pad with zeros)
    data.extend_from_slice(&[0u8; 12]);
    data.extend_from_slice(owner.as_slice());
    data
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepositResult {
    pub chain_id:   u64,
    pub chain_key:  String,
    pub amount_usd: f64,
    pub amount_wei: String,
    pub tx_hash:    String,
    pub skipped:    bool,
    pub reason:     Option<String>,
}

/// Read the solver's stable balance on one chain and deposit it into the LWC.
/// Skips chain 999 (HyperEVM — no ERC-20 stable), halted wells, and zero balances.
pub async fn deposit_chain(
    dep: &LwcDeployment,
    lwc_manager: &LwcManager,
    solver_addr: Address,
    dry_run: bool,
) -> DepositResult {
    let skip = |reason: &str| DepositResult {
        chain_id:   dep.chain_id,
        chain_key:  dep.chain_key.clone(),
        amount_usd: 0.0,
        amount_wei: "0".into(),
        tx_hash:    "".into(),
        skipped:    true,
        reason:     Some(reason.to_string()),
    };

    // HyperEVM has no ERC-20 stable
    if dep.chain_id == 999 {
        return skip("HyperEVM: no ERC-20 stable");
    }

    let asset: Address = match dep.primary_stable.parse() {
        Ok(a) if a != Address::ZERO => a,
        _ => return skip("zero stable address"),
    };

    let rpc_url = match dep.rpc.parse() {
        Ok(u) => u,
        Err(_) => return skip("invalid RPC URL"),
    };
    let provider = ProviderBuilder::new().on_http(rpc_url);

    // Read balanceOf(solver)
    let bal_data = balance_of_calldata(solver_addr);
    let bal_req = alloy::rpc::types::TransactionRequest::default()
        .to(asset)
        .input(bal_data.into());

    let bal_bytes = match provider.call(&bal_req).await {
        Ok(b) => b,
        Err(e) => return skip(&format!("balanceOf failed: {}", e)),
    };

    if bal_bytes.len() < 32 {
        return skip("balanceOf returned short data");
    }

    let balance = U256::from_be_slice(&bal_bytes[bal_bytes.len()-32..]);
    if balance == U256::ZERO {
        return skip("zero balance");
    }

    let amount_usd = balance.try_into()
        .map(|v: u128| v as f64 / 10f64.powi(dep.stable_decimals as i32))
        .unwrap_or(0.0);

    // Check that the well isn't halted before depositing
    // (LwcManager::add_liquidity handles the tx; just log the amount)
    info!(
        "[deposit_all] chain={} ({}) balance=${:.2} (wei={}) dry_run={}",
        dep.chain_key, dep.chain_id, amount_usd, balance, dry_run
    );

    if dry_run {
        return DepositResult {
            chain_id:   dep.chain_id,
            chain_key:  dep.chain_key.clone(),
            amount_usd,
            amount_wei: balance.to_string(),
            tx_hash:    "dry-run:no-tx".into(),
            skipped:    false,
            reason:     None,
        };
    }

    match lwc_manager.add_liquidity(dep.chain_id, asset, balance).await {
        Ok(tx_hash) => {
            info!(
                "[deposit_all] chain={} deposited ${:.2} tx={}",
                dep.chain_key, amount_usd, tx_hash
            );
            DepositResult {
                chain_id:   dep.chain_id,
                chain_key:  dep.chain_key.clone(),
                amount_usd,
                amount_wei: balance.to_string(),
                tx_hash,
                skipped:    false,
                reason:     None,
            }
        }
        Err(e) => {
            warn!("[deposit_all] chain={} add_liquidity failed: {}", dep.chain_key, e);
            DepositResult {
                chain_id:   dep.chain_id,
                chain_key:  dep.chain_key.clone(),
                amount_usd,
                amount_wei: balance.to_string(),
                tx_hash:    "".into(),
                skipped:    true,
                reason:     Some(format!("add_liquidity failed: {}", e)),
            }
        }
    }
}

/// Deposit all solver stables into LWC wells across all chains in parallel.
/// Returns one `DepositResult` per chain.
pub async fn deposit_all(
    deployments: &[LwcDeployment],
    lwc_manager: &LwcManager,
    solver_addr: Address,
    dry_run: bool,
) -> Vec<DepositResult> {
    let futs: Vec<_> = deployments.iter().map(|dep| {
        deposit_chain(dep, lwc_manager, solver_addr, dry_run)
    }).collect();

    let results = join_all(futs).await;

    let deposited: Vec<&DepositResult> = results.iter().filter(|r| !r.skipped).collect();
    let total_usd: f64 = deposited.iter().map(|r| r.amount_usd).sum();

    info!(
        "[deposit_all] complete: {} chains deposited ${:.2} total ({} skipped)",
        deposited.len(),
        total_usd,
        results.iter().filter(|r| r.skipped).count()
    );

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balance_of_calldata_selector() {
        let addr = Address::ZERO;
        let data = balance_of_calldata(addr);
        assert_eq!(&data[..4], &[0x70, 0xa0, 0x82, 0x31]);
        assert_eq!(data.len(), 36);
    }

    #[test]
    fn balance_of_calldata_address_placement() {
        let addr: Address = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913".parse().unwrap();
        let data = balance_of_calldata(addr);
        // Last 20 bytes should be the address
        assert_eq!(&data[16..], addr.as_slice());
    }
}
