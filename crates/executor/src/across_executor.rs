//! Across-protocol fill executor (E2E: SSE → test-run → proof → executeWithProof → log)
//!
//! Pipeline (per spec, section 4.3):
//!   1. caller passes Across intent + Spinner client
//!   2. POST /api/solver/test-run -> profit decision
//!   3. GET  /api/v5/proof/bundle/across/<order_id> -> V5 proof bytes
//!   4. build executeWithProof(v5ProofBlob, adapter, calldata)
//!   5. sign with SOLVER_PRIVATE_KEY, broadcast to dst chain
//!   6. wait receipt, record actual gas + actual profit to outcome log

use alloy::network::EthereumWallet;
use alloy::primitives::{Address, Bytes, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::signers::local::PrivateKeySigner;
use alloy::sol;
use alloy::sol_types::SolCall;
use alloy::rpc::types::TransactionRequest;
use anyhow::{anyhow, Context, Result};
use genome_client::Intent;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::info;

use crate::outcome_log::{OutcomeLog, OutcomeRecord};
use crate::spinner_solver::SpinnerSolverClient;

sol! {
    interface ITaifoonUniversalOperator {
        function executeWithProof(
            bytes calldata v5ProofBlob,
            address adapterContract,
            bytes calldata adapterCalldata
        ) external returns (bool);
    }

    interface IAcrossAdapter {
        function fill(
            int64 depositId,
            bytes calldata relayData,
            uint256 repaymentChainId
        ) external returns (bool);
    }

    /// Across V3 SpokePool direct-fill interface (no adapter, no operator required).
    /// Selector 0xdeff4b24 — verified against Base SpokePool 0x09aea4b2242abC8bb4BB78D537A67a245A7bEC64.
    /// Uses bytes32 for address fields (cross-chain compatible encoding) and a 3rd
    /// repaymentAddress parameter specifying where the relayer gets repaid.
    interface IAcrossSpokePool {
        function fillRelay(
            RelayData calldata relayData,
            uint256 repaymentChainId,
            bytes32 repaymentAddress
        ) external;
    }

    /// Across V3 SpokePool relay tuple.
    /// Fields use bytes32 (not address) for cross-chain address encoding.
    /// depositId is uint256 (not int64).
    /// Selector check: fillRelay((bytes32,bytes32,bytes32,bytes32,bytes32,uint256,uint256,uint256,uint256,uint32,uint32,bytes),uint256,bytes32) = 0xdeff4b24
    struct RelayData {
        bytes32 depositor;
        bytes32 recipient;
        bytes32 exclusiveRelayer;
        bytes32 inputToken;
        bytes32 outputToken;
        uint256 inputAmount;
        uint256 outputAmount;
        uint256 originChainId;
        uint256 depositId;
        uint32  fillDeadline;
        uint32  exclusivityDeadline;
        bytes   message;
    }

    /// Legacy Across V3 SpokePool relay tuple — used by older deployments (Linea etc.)
    /// and by the AcrossAdapter on operator-path chains.
    /// Selector 0x7bfcc68f — fillV3Relay((address,address,address,address,address,uint256,uint256,uint256,int64,uint32,uint32,bytes),uint256)
    struct V3RelayData {
        address depositor;
        address recipient;
        address exclusiveRelayer;
        address inputToken;
        address outputToken;
        uint256 inputAmount;
        uint256 outputAmount;
        uint256 originChainId;
        int64  depositId;
        uint32 fillDeadline;
        uint32 exclusivityDeadline;
        bytes  message;
    }
}

/// Resolved per-chain wiring
#[derive(Clone, Debug)]
pub struct ChainWiring {
    pub chain_id: u64,
    pub rpc_url: String,
    pub operator: Address,
    pub across_adapter: Address,
}

/// Across executor — owns the wallet, chain wiring, and outcome log.
pub struct AcrossExecutor {
    spinner: SpinnerSolverClient,
    signer: PrivateKeySigner,
    chains: HashMap<u64, ChainWiring>,
    log: OutcomeLog,
    dry_run: bool,
    profit_threshold_usd: f64,
}

impl AcrossExecutor {
    pub fn new(
        spinner: SpinnerSolverClient,
        signer: PrivateKeySigner,
        chains: HashMap<u64, ChainWiring>,
        log: OutcomeLog,
        dry_run: bool,
        profit_threshold_usd: f64,
    ) -> Self {
        Self {
            spinner,
            signer,
            chains,
            log,
            dry_run,
            profit_threshold_usd,
        }
    }

    pub fn signer_address(&self) -> Address {
        self.signer.address()
    }

    /// Run the full pipeline for a single Across intent. Returns Ok(Some(tx_hash)) on broadcast,
    /// Ok(None) when skipped (unprofitable / unsupported / dry-run), Err on hard failure.
    pub async fn fill(&self, intent: &Intent) -> Result<Option<String>> {
        if !intent.protocol.to_lowercase().contains("across") {
            return Err(anyhow!("not an Across intent: {}", intent.protocol));
        }

        let started_at = chrono::Utc::now();

        // 1. Profit decision via Spinner
        let test = self.spinner.test_run(&intent.protocol, &intent.id).await
            .context("spinner /api/solver/test-run")?;

        if !test.is_profitable {
            info!("⏭️  Across intent {} not profitable (net=${:.4})", intent.id, test.net_profit_usd);
            self.log.append(OutcomeRecord {
                ts: started_at,
                intent_id: intent.id.clone(),
                protocol: intent.protocol.clone(),
                src_chain: intent.src_chain,
                dst_chain: intent.dst_chain,
                decision: "skip_unprofitable".into(),
                tx_hash: None,
                predicted_gas: Some(test.gas_units),
                gas_used: None,
                effective_gas_price_wei: None,
                predicted_profit_usd: Some(test.net_profit_usd),
                actual_profit_usd: None,
                skip_reason: Some("unprofitable".into()),
                error: None,
            })?;
            return Ok(None);
        }

        if test.net_profit_usd < self.profit_threshold_usd {
            info!(
                "⏭️  Across intent {} below threshold (${:.4} < ${:.2})",
                intent.id, test.net_profit_usd, self.profit_threshold_usd
            );
            self.log.append(OutcomeRecord {
                ts: started_at,
                intent_id: intent.id.clone(),
                protocol: intent.protocol.clone(),
                src_chain: intent.src_chain,
                dst_chain: intent.dst_chain,
                decision: "skip_threshold".into(),
                tx_hash: None,
                predicted_gas: Some(test.gas_units),
                gas_used: None,
                effective_gas_price_wei: None,
                predicted_profit_usd: Some(test.net_profit_usd),
                actual_profit_usd: None,
                skip_reason: Some(format!(
                    "below_threshold:${:.4}<${:.2}",
                    test.net_profit_usd, self.profit_threshold_usd
                )),
                error: None,
            })?;
            return Ok(None);
        }

        // 2. Resolve dst-chain wiring
        let wiring = self
            .chains
            .get(&intent.dst_chain)
            .ok_or_else(|| anyhow!("no chain wiring for dst {}", intent.dst_chain))?
            .clone();

        // 3. Fetch V5 proof bundle (raw bytes — Operator decodes on-chain)
        let proof_bytes = self
            .spinner
            .fetch_across_proof_bundle(&intent.id)
            .await
            .context("spinner /api/v5/proof/bundle/across")?;
        info!("🔐 V5 proof bundle: {} bytes", proof_bytes.len());

        // 4. Build adapter calldata: AcrossAdapter.fill(depositId, relayData, repaymentChainId)
        let adapter_calldata = self.build_across_adapter_calldata(intent)?;

        // 5. Wrap in Operator.executeWithProof
        let operator_calldata = ITaifoonUniversalOperator::executeWithProofCall {
            v5ProofBlob: Bytes::from(proof_bytes),
            adapterContract: wiring.across_adapter,
            adapterCalldata: Bytes::from(adapter_calldata),
        }
        .abi_encode();

        if self.dry_run {
            info!(
                "🧪 [DRY-RUN] Would broadcast executeWithProof to {} on chain {} ({} bytes calldata)",
                wiring.operator,
                wiring.chain_id,
                operator_calldata.len()
            );
            self.log.append(OutcomeRecord {
                ts: started_at,
                intent_id: intent.id.clone(),
                protocol: intent.protocol.clone(),
                src_chain: intent.src_chain,
                dst_chain: intent.dst_chain,
                decision: "dry_run".into(),
                tx_hash: None,
                predicted_gas: Some(test.gas_units),
                gas_used: None,
                effective_gas_price_wei: None,
                predicted_profit_usd: Some(test.net_profit_usd),
                actual_profit_usd: None,
                skip_reason: Some("dry_run".into()),
                error: None,
            })?;
            return Ok(None);
        }

        // 6. Sign + broadcast
        let wallet = EthereumWallet::from(self.signer.clone());
        let provider = ProviderBuilder::new()
            .with_recommended_fillers()
            .wallet(wallet)
            .on_http(wiring.rpc_url.parse()?);

        let tx_req = TransactionRequest::default()
            .to(wiring.operator)
            .input(operator_calldata.into());

        let pending = provider
            .send_transaction(tx_req)
            .await
            .context("send_transaction failed")?;

        let tx_hash = format!("{:#x}", *pending.tx_hash());
        info!("📤 Broadcast {} on chain {}", tx_hash, wiring.chain_id);

        // 7. Wait for receipt
        let receipt = pending
            .with_required_confirmations(1)
            .get_receipt()
            .await
            .context("get_receipt failed")?;

        let gas_used = receipt.gas_used as u64;
        let effective_gas_price = receipt.effective_gas_price as u128;
        let success = receipt.status();

        info!(
            "📬 Receipt {} status={} gas_used={} effective_gas_price={} wei",
            tx_hash, success, gas_used, effective_gas_price
        );

        // 8. Compute actual profit (predicted minus actual gas drift)
        // Predicted gas cost was baked into test.gas_cost_usd. Actual cost = gas_used * gas_price.
        // We don't have a USD price at receipt time without an oracle call; record raw values
        // and a coarse profit estimate using test.gas_cost_usd / test.gas_units ratio.
        let actual_profit = test.net_profit_usd
            - estimate_gas_overrun_usd(&test, gas_used, effective_gas_price);

        self.log.append(OutcomeRecord {
            ts: started_at,
            intent_id: intent.id.clone(),
            protocol: intent.protocol.clone(),
            src_chain: intent.src_chain,
            dst_chain: intent.dst_chain,
            decision: if success { "executed" } else { "executed_failed" }.into(),
            tx_hash: Some(tx_hash.clone()),
            predicted_gas: Some(test.gas_units),
            gas_used: Some(gas_used),
            effective_gas_price_wei: Some(effective_gas_price.to_string()),
            predicted_profit_usd: Some(test.net_profit_usd),
            actual_profit_usd: Some(actual_profit),
            skip_reason: None,
            error: None,
        })?;

        Ok(Some(tx_hash))
    }

    pub fn build_across_adapter_calldata(&self, intent: &Intent) -> Result<Vec<u8>> {
        build_across_adapter_calldata(intent)
    }
}

/// Build calldata for `IAcrossAdapter.fill(int64 depositId, bytes relayData, uint256 repaymentChainId)`.
/// Public so the estimate harness can call it without an executor instance.
pub fn build_across_adapter_calldata(intent: &Intent) -> Result<Vec<u8>> {
    // Prefer the value plumbed through from the genome event payload.
    // Fall back to legacy trailing-digit parser only when the event lacks it.
    let deposit_id = intent.deposit_id
        .or_else(|| parse_deposit_id_legacy(&intent.id))
        .or_else(|| parse_deposit_id_legacy(&intent.tx_hash))
        .ok_or_else(|| anyhow!("cannot resolve depositId for intent {}", intent.id))?;

    let depositor: Address = intent.depositor.parse()
        .context("invalid depositor")?;
    let recipient: Address = intent.recipient.parse()
        .context("invalid recipient")?;
    let input_token: Address = intent.src_token.parse()
        .context("invalid src_token")?;
    let output_token: Address = intent.dst_token.parse()
        .context("invalid dst_token")?;
    let input_amount = U256::from_str_radix(&intent.amount, 10)
        .context("invalid input amount")?;

    // outputAmount must come from the genome event payload (Across enforces
    // it on-chain; using inputAmount caused step-15 reverts per the audit).
    // If the field is absent, fall back to inputAmount but log loudly — that
    // path is for legacy fixtures only.
    let output_amount = match intent.output_amount.as_deref() {
        Some(s) => U256::from_str_radix(s, 10)
            .context("invalid output_amount in intent payload")?,
        None => {
            tracing::warn!(
                "intent {} missing output_amount in payload — falling back to input_amount (will revert in production)",
                intent.id
            );
            input_amount
        }
    };

    // fillDeadline MUST match what's on-chain — use the value from the genome
    // event, not a local clock estimate. Across enforces exact match on-chain.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let fill_deadline = intent.fill_deadline.unwrap_or_else(|| {
        tracing::warn!(
            "intent {} missing fill_deadline — using now+3600 (will likely revert on mainnet)",
            intent.id
        );
        (now + 3600) as u32
    });

    let exclusivity_deadline = intent.exclusivity_deadline.unwrap_or(0);

    let exclusive_relayer: Address = intent.exclusive_relayer
        .as_deref()
        .filter(|s| !s.is_empty() && *s != "0x")
        .and_then(|s| s.parse().ok())
        .unwrap_or(Address::ZERO);

    let message_bytes: Bytes = intent.message
        .as_deref()
        .filter(|s| !s.is_empty() && *s != "0x")
        .and_then(|s| hex::decode(s.trim_start_matches("0x")).ok())
        .map(Bytes::from)
        .unwrap_or_default();

    let relay = V3RelayData {
        depositor,
        recipient,
        exclusiveRelayer: exclusive_relayer,
        inputToken: input_token,
        outputToken: output_token,
        inputAmount: input_amount,
        outputAmount: output_amount,
        originChainId: U256::from(intent.src_chain),
        depositId: deposit_id,
        fillDeadline: fill_deadline,
        exclusivityDeadline: exclusivity_deadline,
        message: message_bytes,
    };

    let encoded = alloy::sol_types::SolValue::abi_encode(&relay);

    let call = IAcrossAdapter::fillCall {
        depositId: deposit_id,
        relayData: Bytes::from(encoded),
        repaymentChainId: U256::from(intent.src_chain),
    };
    Ok(call.abi_encode())
}

/// Build `fillRelay(relayData, repaymentChainId, repaymentAddress)` calldata for chains where
/// the Taifoon operator is not deployed (operator address == 0x0 in chain_wiring).
/// Calls the Across V3 SpokePool directly — no proof wrapper required.
///
/// Uses the new-style SpokePool interface with bytes32 address fields and a repaymentAddress
/// parameter (selector 0xdeff4b24, verified on Base 0x09aea4b2242abC8bb4BB78D537A67a245A7bEC64).
pub fn build_across_spoke_pool_calldata(intent: &Intent) -> Result<Vec<u8>> {
    build_across_spoke_pool_calldata_with_relayer(intent, None)
}

/// Same as `build_across_spoke_pool_calldata` but allows specifying the repayment address
/// (the relayer's address where Across will repay the fill on the origin chain).
pub fn build_across_spoke_pool_calldata_with_relayer(intent: &Intent, relayer_address: Option<Address>) -> Result<Vec<u8>> {
    let deposit_id = intent.deposit_id
        .or_else(|| parse_deposit_id_legacy(&intent.id))
        .or_else(|| parse_deposit_id_legacy(&intent.tx_hash))
        .ok_or_else(|| anyhow!("cannot resolve depositId for intent {}", intent.id))?;

    let depositor: Address = intent.depositor.parse().context("invalid depositor")?;
    let recipient: Address = intent.recipient.parse().context("invalid recipient")?;
    let input_token: Address = intent.src_token.parse().context("invalid src_token")?;
    let output_token: Address = intent.dst_token.parse().context("invalid dst_token")?;
    let input_amount = U256::from_str_radix(&intent.amount, 10).context("invalid input amount")?;

    let output_amount = match intent.output_amount.as_deref() {
        Some(s) => U256::from_str_radix(s, 10).context("invalid output_amount")?,
        None => {
            tracing::warn!("intent {} missing output_amount — falling back to input_amount", intent.id);
            input_amount
        }
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let fill_deadline = intent.fill_deadline.unwrap_or_else(|| {
        tracing::warn!("intent {} missing fill_deadline — using now+3600", intent.id);
        (now + 3600) as u32
    });
    let exclusivity_deadline = intent.exclusivity_deadline.unwrap_or(0);
    let exclusive_relayer: Address = intent.exclusive_relayer
        .as_deref()
        .filter(|s| !s.is_empty() && *s != "0x")
        .and_then(|s| s.parse().ok())
        .unwrap_or(Address::ZERO);
    let message_bytes: Bytes = intent.message
        .as_deref()
        .filter(|s| !s.is_empty() && *s != "0x")
        .and_then(|s| hex::decode(s.trim_start_matches("0x")).ok())
        .map(Bytes::from)
        .unwrap_or_default();

    // Convert address to bytes32 (left-padded with 12 zero bytes)
    let addr_to_b32 = |addr: Address| -> alloy::primitives::FixedBytes<32> {
        let mut b = [0u8; 32];
        b[12..].copy_from_slice(addr.as_slice());
        alloy::primitives::FixedBytes::<32>::from(b)
    };

    let relay = RelayData {
        depositor: addr_to_b32(depositor),
        recipient: addr_to_b32(recipient),
        exclusiveRelayer: addr_to_b32(exclusive_relayer),
        inputToken: addr_to_b32(input_token),
        outputToken: addr_to_b32(output_token),
        inputAmount: input_amount,
        outputAmount: output_amount,
        originChainId: U256::from(intent.src_chain),
        depositId: U256::from(deposit_id as u64),
        fillDeadline: fill_deadline,
        exclusivityDeadline: exclusivity_deadline,
        message: message_bytes,
    };

    // repaymentAddress is where Across repays the relayer on the origin chain.
    // Default to the depositor's chain address (our solver address would be ideal but
    // requires the solver address to be passed in; for now we use the relayer address
    // from the intent or the depositor as a safe fallback).
    let repayment_addr = relayer_address.unwrap_or(depositor);
    let repayment_address = addr_to_b32(repayment_addr);

    let call = IAcrossSpokePool::fillRelayCall {
        relayData: relay,
        repaymentChainId: U256::from(intent.src_chain),
        repaymentAddress: repayment_address,
    };
    Ok(call.abi_encode())
}

/// Legacy parser kept as a fallback when `intent.deposit_id` is missing.
fn parse_deposit_id_legacy(s: &str) -> Option<i64> {
    s.split(&[':', '_', '/'][..])
        .filter_map(|p| p.parse::<i64>().ok())
        .last()
}

/// Relay data fetched from the Across protocol API for a given deposit.
/// Used to fill in missing fields (fill_deadline, output_amount, etc.)
/// when the genome stream's `order/placed` event lacks them.
#[derive(Debug, Clone, Default)]
pub struct AcrossRelayData {
    pub depositor: Option<String>,
    pub recipient: Option<String>,
    pub exclusive_relayer: Option<String>,
    pub input_token: Option<String>,
    pub output_token: Option<String>,
    pub input_amount: Option<String>,
    pub output_amount: Option<String>,
    pub fill_deadline: Option<u32>,
    pub exclusivity_deadline: Option<u32>,
    pub message: Option<String>,
    /// Deposit ID decoded from topics[2] of V3FundsDeposited.
    pub deposit_id: Option<i64>,
}

/// Look up the deposit tx hash from the Across API, then decode relay params on-chain.
/// The genome stream's order/placed event has deposit_id but not the tx hash needed
/// to decode V3FundsDeposited. The Across API /deposit/status endpoint returns it.
pub async fn fetch_relay_data_for_deposit(
    deposit_id: i64,
    origin_chain_id: u64,
    src_chain_rpc: &str,
) -> Option<AcrossRelayData> {
    let url = format!(
        "https://app.across.to/api/deposit/status?depositId={}&originChainId={}",
        deposit_id, origin_chain_id
    );
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(4))
        .build()
        .ok()?;
    let resp = http.get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        tracing::warn!("Across API deposit/status returned {} for deposit {}/{}", resp.status(), deposit_id, origin_chain_id);
        return None;
    }
    let v: serde_json::Value = resp.json().await.ok()?;
    // Extract depositTxHash (Across API returns "depositTxHash" or "depositTxnRef")
    let tx_hash = v.get("depositTxHash")
        .or_else(|| v.get("depositTxnRef"))
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty() && s.starts_with("0x"))?;
    tracing::info!("🔍 Across API: deposit {}/{} → tx {}", deposit_id, origin_chain_id, tx_hash);
    // Now decode relay data from the tx receipt on-chain
    fetch_relay_data_from_tx(tx_hash, src_chain_rpc).await
}

/// Decode relay parameters from the V3FundsDeposited event in a tx receipt.
/// Topic[0] = keccak256("V3FundsDeposited(address,address,address,address,address,uint256,uint256,uint256,int64,uint32,uint32,uint32,bytes)")
/// = 0x32ed1a409ef04c7b0227189c3a103dc5ac10e775a15b785dcc510201f7c25ad3
pub async fn fetch_relay_data_from_tx(
    tx_hash: &str,
    src_chain_rpc: &str,
) -> Option<AcrossRelayData> {
    if tx_hash.is_empty() || tx_hash == "0x" || tx_hash.starts_with("synthetic_") {
        return None;
    }
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .ok()?;

    // eth_getTransactionReceipt
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_getTransactionReceipt",
        "params": [tx_hash]
    });
    let resp = http.post(src_chain_rpc).json(&payload).send().await.ok()?;
    let v: serde_json::Value = resp.json().await.ok()?;
    let logs = v["result"]["logs"].as_array()?;

    // Two known V3FundsDeposited topics — same non-indexed data layout, different sig versions:
    //   NEW (Optimism, Base, Arbitrum deployments): 0xa123dc29...
    //   OLD (Linea, older deployments):             0x32ed1a40...
    // Both: Indexed: destinationChainId (topic[1]), depositId (topic[2]), depositor (topic[3])
    // Non-indexed data: inputToken, outputToken, inputAmount, outputAmount, quoteTimestamp,
    //   fillDeadline, exclusivityDeadline, recipient, exclusiveRelayer, message
    const V3_DEPOSITED_TOPICS: &[&str] = &[
        "0xa123dc29aebf7d0c3322c8eeb5b999e859f39937950ed31056532713d0de396f",
        "0x32ed1a409ef04c7b0227189c3a103dc5ac10e775a15b785dcc510201f7c25ad3",
    ];

    for log in logs {
        let topics = log["topics"].as_array()?;
        let topic0 = topics.first()?.as_str()?;
        if !V3_DEPOSITED_TOPICS.contains(&topic0) {
            continue;
        }
        // topics[1] = destinationChainId, topics[2] = depositId, topics[3] = depositor
        let data_hex = log["data"].as_str()?.strip_prefix("0x")?;
        let data = hex::decode(data_hex).ok()?;
        if data.len() < 320 {
            continue;
        }
        // ABI decode the non-indexed fields (32 bytes each):
        // [0]:  inputToken (address, padded)
        // [1]:  outputToken (address, padded)
        // [2]:  inputAmount (uint256)
        // [3]:  outputAmount (uint256)
        // [4]:  quoteTimestamp (uint32)
        // [5]:  fillDeadline (uint32)
        // [6]:  exclusivityDeadline (uint32)
        // [7]:  recipient (address, padded)
        // [8]:  exclusiveRelayer (address, padded)
        // [9]:  message offset
        // [10]: message length (if present)
        let read_u256 = |offset: usize| -> Option<U256> {
            if offset + 32 > data.len() { return None; }
            Some(U256::from_be_slice(&data[offset..offset + 32]))
        };
        let read_addr = |offset: usize| -> Option<String> {
            if offset + 32 > data.len() { return None; }
            Some(format!("0x{}", hex::encode(&data[offset + 12..offset + 32])))
        };

        let input_token  = read_addr(0)?;
        let output_token = read_addr(32)?;
        let input_amount = read_u256(64)?.to_string();
        let output_amount = read_u256(96)?.to_string();
        // quoteTimestamp at 128, fillDeadline at 160, exclusivityDeadline at 192
        let fill_deadline_u256 = read_u256(160)?;
        let fill_deadline: u32 = fill_deadline_u256.to::<u32>();
        let excl_deadline_u256 = read_u256(192)?;
        let exclusivity_deadline: u32 = excl_deadline_u256.to::<u32>();
        let recipient = read_addr(224)?;
        let exclusive_relayer = read_addr(256)?;
        // depositor from topics[3]
        let depositor_topic = topics.get(3)?.as_str()?;
        let depositor = format!("0x{}", depositor_topic.trim_start_matches("0x").get(24..)?);
        // depositId from topics[2] (indexed uint32/int64 — decode as i64)
        let deposit_id = topics.get(2).and_then(|t| t.as_str()).and_then(|s| {
            let s = s.trim_start_matches("0x");
            i64::from_str_radix(s, 16).ok()
        });

        tracing::info!("🔍 On-chain relay data for {}: depositId={:?} outputAmount={} fillDeadline={} recipient={} exclRelayer={}",
            tx_hash, deposit_id, output_amount, fill_deadline, recipient, exclusive_relayer);

        return Some(AcrossRelayData {
            depositor: Some(depositor),
            recipient: Some(recipient),
            exclusive_relayer: Some(exclusive_relayer),
            input_token: Some(input_token),
            output_token: Some(output_token),
            input_amount: Some(input_amount),
            output_amount: Some(output_amount),
            fill_deadline: Some(fill_deadline),
            exclusivity_deadline: Some(exclusivity_deadline),
            message: None,
            deposit_id,
        });
    }
    None
}

fn estimate_gas_overrun_usd(
    test: &crate::spinner_solver::TestRunResult,
    actual_gas_used: u64,
    actual_gas_price_wei: u128,
) -> f64 {
    let predicted_units = test.gas_units.max(1);
    let predicted_cost_usd = test.gas_cost_usd;
    let unit_cost_usd = predicted_cost_usd / predicted_units as f64;
    // Convert actual to predicted-equivalent units, scaled by price ratio
    let predicted_price_wei = test.gas_price_wei.unwrap_or(0).max(1);
    let price_ratio = actual_gas_price_wei as f64 / predicted_price_wei as f64;
    let actual_cost_usd = unit_cost_usd * actual_gas_used as f64 * price_ratio;
    (actual_cost_usd - predicted_cost_usd).max(0.0)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AcrossExecutorConfig {
    pub spinner_base_url: String,
    pub solver_private_key: String,
    pub chains: HashMap<u64, ChainWiringConfig>,
    pub outcome_db_path: String,
    pub dry_run: bool,
    pub profit_threshold_usd: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChainWiringConfig {
    pub rpc_url: String,
    pub operator: String,
    pub across_adapter: String,
}
