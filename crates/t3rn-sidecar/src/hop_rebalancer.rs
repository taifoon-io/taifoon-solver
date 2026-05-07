//! Greedy cross-well hop rebalancer.
//!
//! Periodically scans all LWC chains, finds the chain with the most surplus
//! and the one with the lowest liquidity, and bridges funds using Across V3.
//! Records every hop in a local SQLite table for observability.

use alloy::{
    network::EthereumWallet,
    primitives::{Address, Bytes, U256},
    providers::{Provider, ProviderBuilder},
    sol,
    sol_types::SolCall,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::Mutex as TokioMutex;
use tracing::{info, warn};

use crate::{gas_razor::resolve_rpc_url, TxGuard};
use portfolio_sidecar::lwc_manager::{LwcChainState, LwcManager};

// ── Across V3 depositV3 ABI (minimal) ────────────────────────────────────────

sol! {
    function depositV3(
        address depositor,
        address recipient,
        address inputToken,
        address outputToken,
        uint256 inputAmount,
        uint256 outputAmount,
        uint256 destinationChainId,
        address exclusiveRelayer,
        uint32 quoteTimestamp,
        uint32 fillDeadline,
        uint32 exclusivityDeadline,
        bytes message
    ) external payable;

    function approve(address spender, uint256 amount) external returns (bool);
    function allowance(address owner, address spender) external view returns (uint256);
}

// ── Across spoke pools ────────────────────────────────────────────────────────

fn spoke_pool(chain_id: u64) -> Option<Address> {
    let m: HashMap<u64, &str> = [
        (1u64,   "0x5c7BCd6E7De5423a257D81B442095A1a6ced35C5"),
        (10,     "0x6f26Bf09B1C792e3228e5467807a900A503c0281"),
        (137,    "0x9295ee1d8C5b022Be115A2AD3c30C72E34e7F096"),
        (8453,   "0x09aea4b2242abC8bb4BB78D537A67a245A7bEC64"),
        (42161,  "0xe35e9842fceaCA96570B734083f4a58e8F7C5f2A"),
        (59144,  "0x7e63a5f1a8F0B4D0934B2f2327DAEd3f6bb2Ee75"),
        (56,     "0x0000000000000000000000000000000000000000"), // BSC not supported by Across
        (130,    "0x0000000000000000000000000000000000000000"), // Unichain TBD
    ].into_iter().collect();
    let addr_str = m.get(&chain_id)?;
    let addr: Address = addr_str.parse().ok()?;
    if addr == Address::ZERO { return None; }
    Some(addr)
}

// ── Across API response ───────────────────────────────────────────────────────

#[derive(Deserialize)]
struct FeesResp {
    #[serde(rename = "outputAmount")]
    output_amount: String,
    #[serde(rename = "outputToken")]
    output_token: OutputToken,
    #[serde(rename = "exclusiveRelayer")]
    exclusive_relayer: String,
    timestamp: String,
    #[serde(rename = "fillDeadline")]
    fill_deadline: u32,
    #[serde(rename = "exclusivityDeadline")]
    exclusivity_deadline: u32,
}

#[derive(Deserialize)]
struct OutputToken {
    address: String,
}

// ── Hop record ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HopRecord {
    pub ts:         i64,
    pub from_chain: u64,
    pub to_chain:   u64,
    pub amount_usd: f64,
    pub tx_src:     Option<String>,
    pub tx_dst:     Option<String>,
    pub status:     String, // pending/bridging/done/failed
}

// ── Rebalancer ────────────────────────────────────────────────────────────────

pub struct HopRebalancer {
    lwc_manager:       Arc<LwcManager>,
    signer:            alloy::signers::local::PrivateKeySigner,
    solver_addr:       Address,
    dry_run:           bool,
    hop_lock:          Arc<TokioMutex<()>>,
    hops_total:        Mutex<u64>,
    max_hop_usd:       f64,
    low_threshold_usd: f64,
    db_path:           String,
    http:              reqwest::Client,
}

impl HopRebalancer {
    pub fn new(
        lwc_manager: Arc<LwcManager>,
        signer: alloy::signers::local::PrivateKeySigner,
    ) -> Arc<Self> {
        let solver_addr = signer.address();
        let dry_run = std::env::var("DRY_RUN")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(true);
        let max_hop_usd: f64 = std::env::var("LWC_MAX_HOP_USD")
            .ok().and_then(|s| s.parse().ok()).unwrap_or(5000.0);
        let low_threshold_usd: f64 = std::env::var("LWC_LOW_THRESHOLD_USD")
            .ok().and_then(|s| s.parse().ok()).unwrap_or(100.0);
        let db_path = std::env::var("SIDECAR_DB_PATH")
            .unwrap_or_else(|_| "sidecar.db".to_string());

        Arc::new(Self {
            lwc_manager,
            signer,
            solver_addr,
            dry_run,
            hop_lock: Arc::new(TokioMutex::new(())),
            hops_total: Mutex::new(0),
            max_hop_usd,
            low_threshold_usd,
            db_path,
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        })
    }

    pub fn hops_total(&self) -> u64 {
        *self.hops_total.lock().unwrap()
    }

    /// Spawn the background hop loop.
    pub fn start(self: Arc<Self>, interval_secs: u64) {
        let rebalancer = self.clone();
        tokio::spawn(async move {
            let interval = std::time::Duration::from_secs(interval_secs.max(60));
            loop {
                tokio::time::sleep(interval).await;
                if let Err(e) = rebalancer.tick().await {
                    warn!("[hop_rebalancer] tick failed: {}", e);
                }
            }
        });
    }

    async fn tick(&self) -> Result<()> {
        // Ensure only one hop runs at a time (tokio Mutex supports try_lock without await)
        let _lock = match self.hop_lock.try_lock() {
            Ok(g) => g,
            Err(_) => {
                info!("[hop_rebalancer] previous hop still in progress, skipping");
                return Ok(());
            }
        };

        let states = self.lwc_manager.scan_all().await;

        // Filter: skip HyperEVM (999) and halted chains
        let live: Vec<&LwcChainState> = states.iter()
            .filter(|s| s.chain_id != 999 && !s.is_halted)
            .collect();

        if live.is_empty() { return Ok(()); }

        // Find chain with highest surplus above low_threshold
        let max_surplus = live.iter()
            .max_by(|a, b| a.pool_available_usd.partial_cmp(&b.pool_available_usd).unwrap_or(std::cmp::Ordering::Equal));

        // Find chain below low_threshold (deficit)
        let min_deficit = live.iter()
            .filter(|s| s.pool_available_usd < self.low_threshold_usd)
            .min_by(|a, b| a.pool_available_usd.partial_cmp(&b.pool_available_usd).unwrap_or(std::cmp::Ordering::Equal));

        let (surplus_chain, deficit_chain) = match (max_surplus, min_deficit) {
            (Some(s), Some(d)) => (s, d),
            _ => {
                info!("[hop_rebalancer] all chains healthy (>{} USD), no hop needed", self.low_threshold_usd);
                return Ok(());
            }
        };

        if surplus_chain.chain_id == deficit_chain.chain_id { return Ok(()); }

        // Hop amount: 30% of surplus, capped at deficit gap and MAX_HOP_USD
        let surplus = surplus_chain.pool_available_usd - self.low_threshold_usd;
        let deficit_gap = self.low_threshold_usd - deficit_chain.pool_available_usd;
        let hop_usd = (surplus * 0.3).min(deficit_gap).min(self.max_hop_usd);

        if hop_usd < 1.0 {
            info!("[hop_rebalancer] hop_usd={:.2} too small, skipping", hop_usd);
            return Ok(());
        }

        info!(
            "[hop_rebalancer] plan: hop ${:.2} from chain={} (avail=${:.2}) → chain={} (avail=${:.2})",
            hop_usd, surplus_chain.chain_id, surplus_chain.pool_available_usd,
            deficit_chain.chain_id, deficit_chain.pool_available_usd
        );

        if self.dry_run {
            info!(
                "[hop_rebalancer] DRY_RUN — would hop ${:.2} from chain {} to chain {}",
                hop_usd, surplus_chain.chain_id, deficit_chain.chain_id
            );
            *self.hops_total.lock().unwrap() += 1;
            return Ok(());
        }

        self.execute_hop(
            surplus_chain.chain_id,
            deficit_chain.chain_id,
            hop_usd,
        ).await
    }

    async fn execute_hop(&self, from_chain: u64, to_chain: u64, hop_usd: f64) -> Result<()> {
        let from_dep = self.lwc_manager.deployments.iter()
            .find(|d| d.chain_id == from_chain)
            .context("no deployment for from_chain")?;
        let to_dep = self.lwc_manager.deployments.iter()
            .find(|d| d.chain_id == to_chain)
            .context("no deployment for to_chain")?;

        let from_asset: Address = from_dep.primary_stable.parse().context("invalid from stable")?;
        let to_asset: Address   = to_dep.primary_stable.parse().context("invalid to stable")?;

        if from_asset == Address::ZERO || to_asset == Address::ZERO {
            anyhow::bail!("hop: no stable on from/to chain");
        }

        let hop_amount_raw = (hop_usd * 10f64.powi(from_dep.stable_decimals as i32)) as u128;
        let hop_amount_wei = U256::from(hop_amount_raw);

        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default().as_secs() as i64;

        // Phase 1: remove_liquidity from surplus chain
        info!("[hop_rebalancer] phase1: remove_liquidity chain={} amount_wei={}", from_chain, hop_amount_wei);
        let tx_src = match self.lwc_manager.remove_liquidity(from_chain, from_asset, hop_amount_wei).await {
            Ok(h) => h,
            Err(e) => {
                self.record_hop(&HopRecord {
                    ts, from_chain, to_chain, amount_usd: hop_usd,
                    tx_src: None, tx_dst: None, status: "failed".to_string(),
                }).await;
                return Err(e).context("remove_liquidity failed");
            }
        };

        info!("[hop_rebalancer] phase1 done: tx_src={}", tx_src);

        // Phase 2: bridge via Across depositV3
        let spoke = match spoke_pool(from_chain) {
            Some(s) => s,
            None => {
                warn!("[hop_rebalancer] no Across spoke for chain={}, aborting hop", from_chain);
                self.record_hop(&HopRecord {
                    ts, from_chain, to_chain, amount_usd: hop_usd,
                    tx_src: Some(tx_src), tx_dst: None, status: "failed".to_string(),
                }).await;
                anyhow::bail!("no Across spoke for chain {}", from_chain);
            }
        };

        info!("[hop_rebalancer] phase2: Across bridge chain={} → chain={} amount={}", from_chain, to_chain, hop_amount_wei);
        let tx_bridge = match self.bridge_via_across(from_chain, to_chain, from_asset, to_asset, spoke, hop_amount_wei).await {
            Ok(h) => h,
            Err(e) => {
                self.record_hop(&HopRecord {
                    ts, from_chain, to_chain, amount_usd: hop_usd,
                    tx_src: Some(tx_src), tx_dst: None, status: "failed".to_string(),
                }).await;
                return Err(e).context("Across bridge failed");
            }
        };

        info!("[hop_rebalancer] phase2 done: tx_bridge={}", tx_bridge);

        // Phase 3: poll for Across receipt (up to 4h, every 30s)
        let received_amount = self.wait_for_bridge_receipt(&tx_bridge, hop_amount_wei, to_chain).await
            .unwrap_or(hop_amount_wei); // best-effort: assume full receipt

        // Phase 4: add_liquidity on destination chain
        info!("[hop_rebalancer] phase3: add_liquidity chain={} amount_wei={}", to_chain, received_amount);
        match self.lwc_manager.add_liquidity(to_chain, to_asset, received_amount).await {
            Ok(tx_dst) => {
                info!("[hop_rebalancer] phase3 done: tx_dst={}", tx_dst);
                self.record_hop(&HopRecord {
                    ts, from_chain, to_chain, amount_usd: hop_usd,
                    tx_src: Some(tx_src), tx_dst: Some(tx_dst), status: "done".to_string(),
                }).await;
                *self.hops_total.lock().unwrap() += 1;
                Ok(())
            }
            Err(e) => {
                self.record_hop(&HopRecord {
                    ts, from_chain, to_chain, amount_usd: hop_usd,
                    tx_src: Some(tx_src), tx_dst: None, status: "failed".to_string(),
                }).await;
                Err(e).context("add_liquidity failed")
            }
        }
    }

    async fn bridge_via_across(
        &self,
        from_chain: u64,
        to_chain: u64,
        input_token: Address,
        _output_token: Address,
        spoke: Address,
        amount: U256,
    ) -> Result<String> {
        // Fetch Across suggested fees
        let fee_url = format!(
            "https://app.across.to/api/suggested-fees?originChainId={}&destinationChainId={}&token={:#x}&amount={}",
            from_chain, to_chain, input_token, amount
        );
        let fees_resp = self.http.get(&fee_url).send().await.context("Across fees request")?;
        if !fees_resp.status().is_success() {
            let body = fees_resp.text().await.unwrap_or_default();
            anyhow::bail!("Across fees API error: {}", body);
        }
        let fees: FeesResp = fees_resp.json().await.context("parse Across fees")?;

        let output_amount: U256 = fees.output_amount.parse().unwrap_or(U256::ZERO);
        let resolved_output_token: Address = fees.output_token.address.parse().unwrap_or(input_token);
        let exclusive_relayer: Address = fees.exclusive_relayer.parse().unwrap_or(Address::ZERO);
        let quote_ts: u32 = fees.timestamp.parse().unwrap_or(0);

        // Ensure approval
        let rpc = resolve_rpc_url(from_chain).context("no RPC for from_chain")?;
        let wallet = EthereumWallet::from(self.signer.clone());
        let provider = ProviderBuilder::new()
            .with_recommended_fillers()
            .wallet(wallet)
            .on_http(rpc.parse().context("invalid rpc url")?);

        // Check and set allowance
        let allowance_call = allowanceCall { owner: self.solver_addr, spender: spoke };
        let allowance_req = alloy::rpc::types::TransactionRequest::default()
            .to(input_token)
            .input(allowance_call.abi_encode().into());
        let allowance_bytes = provider.call(&allowance_req).await.unwrap_or_default();
        let current_allowance = if allowance_bytes.len() >= 32 {
            U256::from_be_slice(&allowance_bytes[allowance_bytes.len()-32..])
        } else {
            U256::ZERO
        };

        if current_allowance < amount {
            let approve_call = approveCall { spender: spoke, amount: U256::MAX };
            let approve_calldata = approve_call.abi_encode();

            // Guard: approve tx to a token contract must be in the known token list
            TxGuard::from_deployments(self.solver_addr)
                .enforce(input_token, &approve_calldata, &[])
                .context("tx_guard blocked hop approve")?;

            let approve_req = alloy::rpc::types::TransactionRequest::default()
                .to(input_token)
                .input(approve_calldata.into());
            let pending = provider.send_transaction(approve_req).await.context("approve failed")?;
            pending.get_receipt().await.context("approve receipt")?;
        }

        // depositV3
        let calldata = depositV3Call {
            depositor:            self.solver_addr,
            recipient:            self.solver_addr,
            inputToken:           input_token,
            outputToken:          resolved_output_token,
            inputAmount:          amount,
            outputAmount:         output_amount,
            destinationChainId:   U256::from(to_chain),
            exclusiveRelayer:     exclusive_relayer,
            quoteTimestamp:       quote_ts,
            fillDeadline:         fees.fill_deadline,
            exclusivityDeadline:  fees.exclusivity_deadline,
            message:              Bytes::new(),
        }.abi_encode();

        let deposit_calldata = Bytes::from(calldata);

        // Guard: depositV3 to Across spoke pool; recipient = solver_addr
        TxGuard::from_deployments(self.solver_addr)
            .enforce(spoke, &deposit_calldata, &[self.solver_addr])
            .context("tx_guard blocked hop depositV3")?;

        let tx_req = alloy::rpc::types::TransactionRequest::default()
            .to(spoke)
            .input(deposit_calldata.into());

        let pending = provider.send_transaction(tx_req).await.context("depositV3 failed")?;
        let receipt = pending.get_receipt().await.context("depositV3 receipt")?;
        let hash = format!("{:#x}", receipt.transaction_hash);
        info!("[hop_rebalancer] Across deposit tx={}", hash);
        Ok(hash)
    }

    /// Poll Across receipt API every 30s, up to 4h.
    async fn wait_for_bridge_receipt(
        &self,
        tx_hash: &str,
        fallback: U256,
        dst_chain: u64,
    ) -> Option<U256> {
        let url = format!(
            "https://app.across.to/api/receipt?originChainId={}&txHash={}",
            dst_chain, tx_hash
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4 * 3600);

        while std::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;

            #[derive(Deserialize)]
            struct ReceiptResp { filled: Option<bool>, #[serde(rename = "outputAmount")] output_amount: Option<String> }

            if let Ok(resp) = self.http.get(&url).send().await {
                if resp.status().is_success() {
                    if let Ok(r) = resp.json::<ReceiptResp>().await {
                        if r.filled == Some(true) {
                            let amount = r.output_amount
                                .and_then(|s| s.parse::<U256>().ok())
                                .unwrap_or(fallback);
                            info!("[hop_rebalancer] Across receipt confirmed, output={}", amount);
                            return Some(amount);
                        }
                    }
                }
            }
        }

        warn!("[hop_rebalancer] Across receipt poll timed out for tx={}", tx_hash);
        Some(fallback)
    }

    async fn record_hop(&self, hop: &HopRecord) {
        // SQLite record — best effort, don't fail the hop on DB error
        if let Ok(conn) = rusqlite::Connection::open(&self.db_path) {
            let _ = conn.execute(
                "CREATE TABLE IF NOT EXISTS lwc_hops (
                    id INTEGER PRIMARY KEY,
                    ts INTEGER NOT NULL,
                    from_chain INTEGER NOT NULL,
                    to_chain INTEGER NOT NULL,
                    amount_usd REAL NOT NULL,
                    tx_src TEXT,
                    tx_dst TEXT,
                    status TEXT NOT NULL DEFAULT 'pending'
                )",
                [],
            );
            let _ = conn.execute(
                "INSERT INTO lwc_hops (ts, from_chain, to_chain, amount_usd, tx_src, tx_dst, status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    hop.ts, hop.from_chain as i64, hop.to_chain as i64,
                    hop.amount_usd,
                    hop.tx_src.as_deref(),
                    hop.tx_dst.as_deref(),
                    hop.status,
                ],
            );
        }
    }

    /// Load recent hop history from SQLite.
    pub fn recent_hops(&self, limit: usize) -> Vec<HopRecord> {
        let conn = match rusqlite::Connection::open(&self.db_path) {
            Ok(c) => c,
            Err(_) => return vec![],
        };

        let _ = conn.execute(
            "CREATE TABLE IF NOT EXISTS lwc_hops (
                id INTEGER PRIMARY KEY, ts INTEGER NOT NULL, from_chain INTEGER NOT NULL,
                to_chain INTEGER NOT NULL, amount_usd REAL NOT NULL,
                tx_src TEXT, tx_dst TEXT, status TEXT NOT NULL DEFAULT 'pending'
            )", [],
        );

        let mut stmt = match conn.prepare(
            "SELECT ts, from_chain, to_chain, amount_usd, tx_src, tx_dst, status
             FROM lwc_hops ORDER BY ts DESC LIMIT ?1"
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };

        stmt.query_map(rusqlite::params![limit as i64], |row| {
            Ok(HopRecord {
                ts:         row.get(0)?,
                from_chain: row.get::<_, i64>(1)? as u64,
                to_chain:   row.get::<_, i64>(2)? as u64,
                amount_usd: row.get(3)?,
                tx_src:     row.get(4)?,
                tx_dst:     row.get(5)?,
                status:     row.get(6)?,
            })
        })
        .ok()
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spoke_pool_known_chains() {
        assert!(spoke_pool(8453).is_some());
        assert!(spoke_pool(42161).is_some());
        assert!(spoke_pool(10).is_some());
    }

    #[test]
    fn spoke_pool_unsupported_chains() {
        assert!(spoke_pool(999).is_none()); // HyperEVM
        assert!(spoke_pool(130).is_none()); // Unichain TBD
    }
}
