//! Real-time `OrderCreated` event poller for all LWC V4 chains.
//!
//! Polls `eth_getLogs` per chain at chain-appropriate intervals, decodes
//! `OrderCreated` events and broadcasts them on a `tokio::sync::broadcast` channel.

use alloy::{
    primitives::{Address, FixedBytes, U256},
    providers::{Provider, ProviderBuilder},
    rpc::types::{BlockNumberOrTag, Filter},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::{gas_razor::resolve_rpc_url, LwcDeployment};

// OrderCreated(bytes32 indexed id, bytes4 indexed destination, bytes32 targetAccount,
//              uint256 amount, address rewardAsset, uint256 insurance, uint256 maxReward,
//              uint32 asset, address sourceAccount)
const ORDER_CREATED_TOPIC: &str =
    "0x3bb399125b923176baf5098f432689e4843dee54b68daf1d7cadd91d99a63601";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct T3rnOrder {
    /// bytes32 order id
    pub id: [u8; 32],
    /// chain where the order was placed
    pub source_chain: u64,
    /// bytes4 destination tag (e.g. b"arbm")
    pub destination: [u8; 4],
    pub target_account: [u8; 32],
    pub amount: U256,
    pub reward_asset: Address,
    pub insurance: U256,
    pub max_reward: U256,
    pub asset: u32,
}

pub struct OrderMonitor {
    tx: broadcast::Sender<T3rnOrder>,
}

impl OrderMonitor {
    pub fn new() -> (Self, broadcast::Receiver<T3rnOrder>) {
        let (tx, rx) = broadcast::channel(256);
        (Self { tx }, rx)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<T3rnOrder> {
        self.tx.subscribe()
    }

    /// Spawn one poller task per chain. Skips chain 999 (HyperEVM — no ERC-20 stable).
    pub fn start(&self, deployments: Vec<LwcDeployment>) {
        for dep in deployments {
            if dep.chain_id == 999 { continue; }
            let tx = self.tx.clone();
            let dep = Arc::new(dep);
            tokio::spawn(async move {
                poll_chain(dep, tx).await;
            });
        }
    }
}

impl Default for OrderMonitor {
    fn default() -> Self {
        let (m, _) = Self::new();
        m
    }
}

/// Poll interval in milliseconds based on chain type.
fn poll_interval_ms(chain_id: u64) -> u64 {
    match chain_id {
        // L2s with ~2s block times
        42161 | 8453 | 10 | 59144 | 130 => 2_000,
        // Polygon, BSC ~3s
        137 | 56 => 3_000,
        // Ethereum ~12s
        1 => 12_000,
        // default 4s
        _ => 4_000,
    }
}

async fn poll_chain(dep: Arc<LwcDeployment>, tx: broadcast::Sender<T3rnOrder>) {
    let chain_id = dep.chain_id;
    let interval = tokio::time::Duration::from_millis(poll_interval_ms(chain_id));

    let rpc_url = match resolve_rpc_url(chain_id).or_else(|| Some(dep.rpc.clone())) {
        Some(u) => u,
        None => {
            warn!("[order_monitor] no RPC for chain={}", chain_id);
            return;
        }
    };

    let url = match rpc_url.parse() {
        Ok(u) => u,
        Err(e) => {
            warn!("[order_monitor] invalid RPC url chain={}: {}", chain_id, e);
            return;
        }
    };

    let well: Address = match dep.well_v4.parse() {
        Ok(a) => a,
        Err(e) => {
            warn!("[order_monitor] invalid well address chain={}: {}", chain_id, e);
            return;
        }
    };

    let topic0: FixedBytes<32> = ORDER_CREATED_TOPIC.parse().expect("valid topic0");

    let provider = ProviderBuilder::new().on_http(url);

    // Start from current block
    let mut last_block: u64 = match provider.get_block_number().await {
        Ok(n) => n.saturating_sub(1),
        Err(_) => 0,
    };

    info!("[order_monitor] chain={} starting from block={}", chain_id, last_block);

    loop {
        tokio::time::sleep(interval).await;

        let current_block = match provider.get_block_number().await {
            Ok(n) => n,
            Err(e) => {
                warn!("[order_monitor] chain={} get_block_number failed: {}", chain_id, e);
                continue;
            }
        };

        if current_block <= last_block {
            continue;
        }

        let filter = Filter::new()
            .address(well)
            .event_signature(topic0)
            .from_block(BlockNumberOrTag::Number(last_block + 1))
            .to_block(BlockNumberOrTag::Number(current_block));

        let logs = match provider.get_logs(&filter).await {
            Ok(l) => l,
            Err(e) => {
                warn!("[order_monitor] chain={} get_logs failed: {}", chain_id, e);
                continue;
            }
        };

        for log in &logs {
            if let Some(order) = decode_order_created(log, chain_id) {
                info!(
                    "[order_monitor] chain={} OrderCreated id={} dst={:?} amount={}",
                    chain_id,
                    hex::encode(order.id),
                    std::str::from_utf8(&order.destination).unwrap_or("????"),
                    order.amount
                );
                let _ = tx.send(order);
            }
        }

        last_block = current_block;
    }
}

fn decode_order_created(log: &alloy::rpc::types::Log, source_chain: u64) -> Option<T3rnOrder> {
    let topics = &log.topics();

    // topic0 = event sig, topic1 = id (indexed bytes32), topic2 = destination (indexed bytes4)
    if topics.len() < 3 { return None; }

    let id: [u8; 32] = topics[1].into();

    // destination is bytes4 stored right-padded in the topic bytes32
    let dst_topic: [u8; 32] = topics[2].into();
    let destination: [u8; 4] = dst_topic[..4].try_into().ok()?;

    // Non-indexed fields are ABI-encoded in log.data
    let data = log.data().data.as_ref();
    if data.len() < 7 * 32 { return None; }

    let target_account: [u8; 32] = data[0..32].try_into().ok()?;
    let amount = U256::from_be_slice(&data[32..64]);
    let reward_asset = Address::from_slice(&data[76..96]); // right-aligned in 32-byte word
    let insurance = U256::from_be_slice(&data[96..128]);
    let max_reward = U256::from_be_slice(&data[128..160]);
    let asset: u32 = U256::from_be_slice(&data[160..192]).try_into().unwrap_or(0);

    Some(T3rnOrder {
        id,
        source_chain,
        destination,
        target_account,
        amount,
        reward_asset,
        insurance,
        max_reward,
        asset,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_interval_l2() {
        assert_eq!(poll_interval_ms(8453), 2_000);
        assert_eq!(poll_interval_ms(42161), 2_000);
    }

    #[test]
    fn poll_interval_ethereum() {
        assert_eq!(poll_interval_ms(1), 12_000);
    }

    #[test]
    fn poll_interval_polygon() {
        assert_eq!(poll_interval_ms(137), 3_000);
    }

    #[test]
    fn decode_short_data_returns_none() {
        // Too-short data should not panic
        let log = alloy::rpc::types::Log::default();
        assert!(decode_order_created(&log, 1).is_none());
    }
}
