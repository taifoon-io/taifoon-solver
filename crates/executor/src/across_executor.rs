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

    /// Across V3 SpokePool relay tuple — must match the deployed
    /// `taifoon-eco/contracts/adapters/AcrossAdapter.sol` `IAcrossSpokePool.V3RelayData`
    /// (note: depositId is `int64`, NOT uint32 — the AcrossAdapter contract
    /// `abi.decode`s relayData as this exact tuple).
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

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let relay = V3RelayData {
        depositor,
        recipient,
        exclusiveRelayer: Address::ZERO,
        inputToken: input_token,
        outputToken: output_token,
        inputAmount: input_amount,
        outputAmount: output_amount,
        originChainId: U256::from(intent.src_chain),
        depositId: deposit_id,
        fillDeadline: (now + 3600) as u32,
        exclusivityDeadline: 0,
        message: Bytes::new(),
    };

    let encoded = alloy::sol_types::SolValue::abi_encode(&relay);

    let call = IAcrossAdapter::fillCall {
        depositId: deposit_id,
        relayData: Bytes::from(encoded),
        repaymentChainId: U256::from(intent.src_chain),
    };
    Ok(call.abi_encode())
}

/// Legacy parser kept as a fallback when `intent.deposit_id` is missing.
fn parse_deposit_id_legacy(s: &str) -> Option<i64> {
    s.split(&[':', '_', '/'][..])
        .filter_map(|p| p.parse::<i64>().ok())
        .last()
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
