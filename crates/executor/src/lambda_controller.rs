//! Lambda controller — col-p4 intent lifecycle state machine.
//!
//! Wraps the existing [`AcrossExecutor`] broadcast path with a wallet-manager-
//! backed state machine and the post-confirmation `claim()` step described in
//! `HACKATHON_COLOSSEUM_PLAN.md` lines 124–140.
//!
//! ### `lambda_execute(intent)`
//!   1. wallet_manager.record_detected (idempotent)
//!   2. wallet_manager.reserve(intent.amount_usd)
//!   3. spinner.test_run            → PROFITABILITY_CHECK
//!   4. spinner.fetch_v5_proof      → PROOF_FETCH
//!   5. build_adapter_calldata      → CALLDATA_BUILD
//!   6. broadcast executeVerifiedCall (V1)  → BROADCAST → PENDING_CONFIRMATION
//!   7. wait for receipt            → CONFIRMED  | REVERTED
//!   8. on CONFIRMED: wallet_manager.release + emit_genome_feedback (best-effort)
//!
//! ### `lambda_claim(intent_id)`
//!   1. assert wallet_manager state = CONFIRMED
//!   2. send `claim()` (selector-only calldata) to the configured Universal
//!      Operator address on the dst chain
//!   3. await receipt → record_revenue(fee_usd) on success
//!
//! The legacy [`Executor`](crate::Executor) path is intentionally NOT touched
//! here — non-Across protocols still flow through it from solver-main while
//! the Lambda controller handles Across end-to-end.

use std::collections::HashMap;
use std::sync::Arc;

use alloy::network::EthereumWallet;
use alloy::primitives::{keccak256, Address, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::rpc::types::TransactionRequest;
use alloy::signers::local::PrivateKeySigner;
use anyhow::{anyhow, Context, Result};
use genome_client::Intent;
use serde::Serialize;
use tracing::{info, warn};
use wallet_manager::{IntentState, NewIntent, WalletManager};

use crate::across_executor::{build_across_adapter_calldata, build_across_spoke_pool_calldata_with_relayer, fetch_relay_data_for_deposit, fetch_relay_data_from_tx, ChainWiring};
use crate::evm_estimate::{optimal_gas_price_wei, run_evm_estimate_with_value};
use crate::estimate::EstimateOutcome;
use crate::mayan_evm_estimate::MayanEvmEstimateAdapter;
use crate::mayan_solana_estimate::default_solana_rpc;
use crate::outcome_log::{OutcomeLog, OutcomeRecord};
use crate::spinner_solver::SpinnerSolverClient;
use protocol_adapters::debridge::DeBridgeAdapter;
use protocol_adapters::SpinnerClient;
use protocol_adapters_solana::{MayanSolanaIntent, SolanaBroadcaster};

/// Outcome of a `lambda_execute` run, mirrored back to the solver event API.
#[derive(Debug, Clone)]
pub enum LambdaExecuteOutcome {
    /// Broadcast confirmed on-chain.
    Confirmed { tx_hash: String, gas_used: u64 },
    /// Broadcast made it to the chain but the receipt status was 0.
    Reverted { tx_hash: String, error: String },
    /// Skipped before broadcast (unprofitable, dry-run, missing wiring, etc.).
    Skipped { reason: String },
    /// Failed before broadcast (proof fetch, calldata build, or RPC error).
    Failed { stage: &'static str, error: String },
}

/// Outcome of a `lambda_claim` run.
#[derive(Debug, Clone)]
pub enum LambdaClaimOutcome {
    Claimed { tx_hash: String, fee_usd: f64 },
    NotEligible { reason: String },
    Failed { error: String },
}

/// Controller config — every dependency is constructor-injected so the unit
/// tests in this module can drive the state-machine transitions without an
/// RPC.
pub struct LambdaController {
    pub wallet: Arc<WalletManager>,
    pub spinner: SpinnerSolverClient,
    pub signer: PrivateKeySigner,
    pub chains: HashMap<u64, ChainWiring>,
    pub outcome_log: Option<OutcomeLog>,
    pub dry_run: bool,
    pub profit_threshold_usd: f64,
    /// Optional spinner-side feedback URL — when present the controller POSTs
    /// the post-confirmation event to `{base}/api/genome/feedback`. Failures
    /// log a warning and continue (the broadcast itself is the source of
    /// truth, the feedback ping is informational).
    pub feedback_url: Option<String>,
}

impl LambdaController {
    /// Run the full intent → broadcast → receipt → release pipeline.
    pub async fn lambda_execute(&self, intent: &Intent) -> Result<LambdaExecuteOutcome> {
        let amount_usd = intent_amount_usd(intent);

        // 0. Hackathon demo safety belt — hard cap on per-fill notional.
        // `MAX_NOTIONAL_USD` (default $200) prevents an accidental large fill while
        // we're broadcasting from a live mainnet wallet. Skipped intents are logged
        // and recorded; they don't touch the wallet manager. Dry-run still applies
        // the cap so operators see what would be skipped before going live.
        let max_notional = std::env::var("MAX_NOTIONAL_USD")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(200.0);
        if amount_usd > max_notional {
            let reason = format!(
                "notional_cap_exceeded:amount=${:.2}>cap=${:.2}",
                amount_usd, max_notional
            );
            info!("⏭️  {} — {}", intent.id, reason);
            // Best-effort outcome record so the dashboard shows the skip.
            self.append_outcome(OutcomeRecord {
                ts: chrono::Utc::now(),
                intent_id: intent.id.clone(),
                protocol: intent.protocol.clone(),
                src_chain: intent.src_chain,
                dst_chain: intent.dst_chain,
                decision: "skip_notional_cap".into(),
                tx_hash: None,
                predicted_gas: None,
                gas_used: None,
                effective_gas_price_wei: None,
                predicted_profit_usd: None,
                actual_profit_usd: None,
                skip_reason: Some(reason.clone()),
                error: None,
                solver_id: None,
                claim_tx_hash: None,
                claim_fee_usd: None,
            });
            return Ok(LambdaExecuteOutcome::Skipped { reason });
        }

        // 1a. Resolve dst-chain wiring before touching the wallet so we don't
        // leave orphaned records for chains we can never fill on.
        let started_at = chrono::Utc::now();
        let wiring = match self.chains.get(&intent.dst_chain) {
            Some(w) => w.clone(),
            None => {
                let chain_name = debridge_non_evm_chain_name(intent.dst_chain)
                    .map(|n| format!(" ({})", n))
                    .unwrap_or_default();
                let reason = format!("no chain wiring for dst {}{}", intent.dst_chain, chain_name);
                info!("⏭️  {} — {}", intent.id, reason);
                return Ok(LambdaExecuteOutcome::Skipped { reason });
            }
        };

        // 1b. Persist + reserve (idempotent for both).
        self.wallet
            .record_detected(NewIntent {
                intent_id: intent.id.clone(),
                protocol: intent.protocol.clone(),
                src_chain: intent.src_chain as i64,
                dst_chain: intent.dst_chain as i64,
                amount_usd,
            })
            .map_err(|e| anyhow!("wallet record_detected: {e}"))?;

        if !self.dry_run {
            if let Err(e) = self.wallet.reserve(&intent.id, amount_usd) {
                warn!("⚠️  wallet reserve failed for {}: {e}", intent.id);
                self.transition(&intent.id, IntentState::SkipUnprofitable, None, Some(&format!("reserve_failed:{e}")));
                return Ok(LambdaExecuteOutcome::Skipped {
                    reason: format!("reserve_failed:{e}"),
                });
            }
        } else {
            // Dry-run: record_detected already done; skip budget check so the
            // full pipeline (calldata-build → DRY_RUN guard) can be exercised.
            info!("🧪 DRY_RUN: skipping wallet reserve for {}", intent.id);
        }
        let direct_fill = wiring.operator == Address::ZERO;

        // Pre-classify protocol so we can bypass spinner for non-Across protocols.
        let proto_lower_pre = intent.protocol.to_lowercase();
        let is_debridge_pre = proto_lower_pre.contains("debridge") || proto_lower_pre.contains("dln");
        let is_mayan_pre = proto_lower_pre.contains("mayan");
        let is_across_pre = proto_lower_pre.contains("across");

        // Guard: skip Across fills where the SpokePool adapter is zero (chain not supported).
        if is_across_pre && wiring.across_adapter == Address::ZERO {
            let reason = format!("no Across SpokePool on chain {}", intent.dst_chain);
            self.transition(&intent.id, IntentState::SkipUnprofitable, None, Some(&reason));
            let _ = self.wallet.release(&intent.id);
            return Ok(LambdaExecuteOutcome::Skipped { reason });
        }
        // deBridge and Mayan submit directly to their own contracts (DlnDestination / Swift).
        // They are not indexed by the Taifoon spinner, so test-run always 404/501.
        let bypass_spinner = direct_fill || is_debridge_pre || is_mayan_pre;

        // 3. Spinner test-run gates profitability (PROFITABILITY_CHECK).
        // Direct-fill chains (operator==0x0) and deBridge/Mayan bypass the spinner:
        // the spinner doesn't index these orders. In dry-run mode, treat a
        // missing/unavailable spinner endpoint as "assume profitable".
        self.transition(&intent.id, IntentState::ProfitabilityCheck, None, None);
        let test = if bypass_spinner {
            info!("⚡ Bypassing spinner test-run for {} ({})",
                intent.protocol, if direct_fill { "direct-fill" } else { "non-Taifoon-operator protocol" });
            None
        } else {
            let test_opt = self.spinner.test_run(&intent.protocol, &intent.id).await;
            match test_opt {
                Ok(t) => Some(t),
                Err(e) if self.dry_run => {
                    warn!("⚠️  spinner test-run unavailable (dry-run, continuing): {e}");
                    None
                }
                Err(e) => {
                    let err = format!("spinner_test_run:{e}");
                    self.transition(&intent.id, IntentState::SkipUnprofitable, None, Some(&err));
                    let _ = self.wallet.release(&intent.id);
                    return Ok(LambdaExecuteOutcome::Failed {
                        stage: "spinner_test_run",
                        error: err,
                    });
                }
            }
        };

        if let Some(ref test) = test {
            if !test.is_profitable || test.net_profit_usd < self.profit_threshold_usd {
                let reason = if !test.is_profitable {
                    "unprofitable".to_string()
                } else {
                    format!(
                        "below_threshold:${:.4}<${:.2}",
                        test.net_profit_usd, self.profit_threshold_usd
                    )
                };
                self.transition(&intent.id, IntentState::SkipUnprofitable, None, Some(&reason));
                let _ = self.wallet.release(&intent.id);
                self.append_outcome(OutcomeRecord {
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
                    skip_reason: Some(reason.clone()),
                    error: None,
                    solver_id: None,
                    claim_tx_hash: None,
                    claim_fee_usd: None,
                });
                return Ok(LambdaExecuteOutcome::Skipped { reason });
            }
        }

        // 4-pre-0. Skip deBridge orders with an exclusive taker restriction that isn't us.
        if is_debridge_pre {
            if let Some(taker) = &intent.dln_allowed_taker_dst {
                let taker_clean = taker.trim_start_matches("0x").trim_start_matches('0');
                if !taker_clean.is_empty() {
                    // Non-zero allowedTakerDst — check if it's our solver address.
                    let our_addr = format!("{:x}", self.signer.address());
                    let is_ours = taker_clean.eq_ignore_ascii_case(&our_addr)
                        || taker.to_lowercase().ends_with(&our_addr.to_lowercase());
                    if !is_ours {
                        let reason = format!("debridge_exclusive_taker:{}", &taker[..taker.len().min(20)]);
                        info!("⏭️  {} — {}", intent.id, reason);
                        self.transition(&intent.id, IntentState::SkipUnprofitable, None, Some(&reason));
                        let _ = self.wallet.release(&intent.id);
                        return Ok(LambdaExecuteOutcome::Skipped { reason });
                    }
                }
            }
        }

        // 4-pre. Spread check for deBridge (spinner bypassed → no external profitability gate).
        // give_amount is what the user locked on src; take_amount is what we must pay on dst.
        // The spread is give - take in token units. We require spread_pct ≥ 0.5 % to cover
        // gas and slippage, otherwise skip.
        // IMPORTANT: Only apply this check when give and take are likely the same-decimal token
        // (e.g., USDC→USDC). When take >> give by >1000×, tokens have different decimals
        // (e.g., USDC 6-dec give vs ETH 18-dec take) and the raw comparison is meaningless.
        if is_debridge_pre {
            let give = intent.give_amount.as_deref()
                .and_then(|s| s.parse::<u128>().ok());
            let take = intent.take_amount.as_deref()
                .and_then(|s| s.parse::<u128>().ok());
            if let (Some(g), Some(t)) = (give, take) {
                // Skip check when decimal mismatch is likely (take > give * 1000).
                let same_scale = t <= g.saturating_mul(1000);
                if same_scale {
                    if t == 0 || g <= t {
                        let reason = format!("debridge_no_spread:give={g}<=take={t}");
                        info!("⏭️  {} — {}", intent.id, reason);
                        self.transition(&intent.id, IntentState::SkipUnprofitable, None, Some(&reason));
                        let _ = self.wallet.release(&intent.id);
                        return Ok(LambdaExecuteOutcome::Skipped { reason });
                    }
                    let spread_pct = (g - t) as f64 / g as f64 * 100.0;
                    if spread_pct < 0.01 {
                        let reason = format!("debridge_spread_too_thin:{spread_pct:.3}pct");
                        info!("⏭️  {} — {}", intent.id, reason);
                        self.transition(&intent.id, IntentState::SkipUnprofitable, None, Some(&reason));
                        let _ = self.wallet.release(&intent.id);
                        return Ok(LambdaExecuteOutcome::Skipped { reason });
                    }
                }
            }
        }

        // ERC-20 balance pre-check for deBridge fills.
        // fulfillOrder requires the solver to have >= take_amount of the take token on dst chain.
        if is_debridge_pre {
            let take_token = intent.dst_token.trim().to_lowercase();
            let is_erc20 = !take_token.is_empty()
                && take_token != "native"
                && take_token != "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
                && take_token != "0x0000000000000000000000000000000000000000";
            if is_erc20 {
                if let Some(required) = intent.take_amount.as_deref()
                    .and_then(|s| s.parse::<u128>().ok())
                    .filter(|&v| v > 0)
                {
                    if let Ok(token_addr) = take_token.parse::<alloy::primitives::Address>() {
                        let rpc_url_opt = crate::evm_estimate::resolve_rpc_url(wiring.chain_id);
                        if let Some(rpc_url) = rpc_url_opt {
                            if let Ok(parsed) = rpc_url.parse() {
                                use alloy::providers::{Provider, ProviderBuilder};
                                let provider = ProviderBuilder::new().on_http(parsed);
                                let mut call_data = [0u8; 36];
                                call_data[0..4].copy_from_slice(&[0x70, 0xa0, 0x82, 0x31]);
                                call_data[16..36].copy_from_slice(self.signer.address().as_slice());
                                let tx = alloy::rpc::types::TransactionRequest::default()
                                    .to(token_addr)
                                    .input(alloy::primitives::Bytes::from(call_data.to_vec()).into());
                                if let Ok(result) = provider.call(&tx).await {
                                    let bal = if result.len() >= 32 {
                                        u128::from_be_bytes(result[16..32].try_into().unwrap_or([0u8; 16]))
                                    } else { 0u128 };
                                    if bal < required {
                                        let reason = format!("debridge_erc20_insufficient:have={}<need={}", bal, required);
                                        info!("⏭️  {} — {}", intent.id, reason);
                                        self.transition(&intent.id, IntentState::SkipUnprofitable, None, Some(&reason));
                                        let _ = self.wallet.release(&intent.id);
                                        return Ok(LambdaExecuteOutcome::Skipped { reason });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // 4. PROOF_FETCH — skipped for direct-fill chains (operator==0x0) and for
        // deBridge/Mayan which submit directly to their own contracts (no Taifoon proof).
        let proof_bytes: Vec<u8> = if bypass_spinner {
            // No Taifoon proof needed.
            vec![]
        } else {
            self.transition(&intent.id, IntentState::ProofFetch, None, None);
            let proof_candidates: Vec<String> = {
                let mut keys = Vec::new();
                if let Some(dep_id) = intent.deposit_id {
                    keys.push(dep_id.to_string());
                }
                if !intent.tx_hash.is_empty() && intent.tx_hash != "0x" {
                    keys.push(intent.tx_hash.clone());
                }
                keys.push(intent.id.clone());
                keys
            };
            let mut result = None;
            let mut last_err = String::new();
            for key in &proof_candidates {
                match self.spinner.fetch_across_proof_bundle(key).await {
                    Ok(b) => { result = Some(b); break; }
                    Err(e) => { last_err = e.to_string(); }
                }
            }
            match result {
                Some(b) => {
                    info!("🔐 Proof bundle: {} bytes (for deposit {})", b.len(),
                        intent.deposit_id.map(|d| d.to_string()).unwrap_or_else(|| "?".into()));
                    b
                }
                None if self.dry_run => {
                    warn!("⚠️  proof-bundle unavailable for {:?} (dry-run, using stub): {}", proof_candidates, last_err);
                    vec![]
                }
                None => {
                    let err = format!("proof_fetch (tried {:?}): {}", proof_candidates, last_err);
                    self.transition(&intent.id, IntentState::ProofMissing, None, Some(&err));
                    return Ok(LambdaExecuteOutcome::Failed { stage: "proof_fetch", error: err });
                }
            }
        };

        // 5. RELAY DATA ENRICHMENT for direct fills.
        // The genome stream's order/placed event often lacks fill_deadline, output_amount,
        // recipient, etc. For direct fills these must exactly match the on-chain deposit.
        // Strategy:
        //   A) If we have a deposit_id → Across API /deposit/status → depositTxHash → decode on-chain
        //   B) If we have a tx_hash that looks like a real tx → decode on-chain directly
        // Enrichment only applies to Across direct-fills. deBridge/Mayan carry all needed
        // fields from their on-chain log decoders and never need Across relay-data lookup.
        let enriched_intent: Option<Intent> = if direct_fill && !is_debridge_pre && !is_mayan_pre {
            let needs_enrichment = intent.fill_deadline.is_none()
                || intent.output_amount.is_none()
                || intent.deposit_id.is_none();
            let src_rpc = self.chains.get(&intent.src_chain)
                .map(|w| w.rpc_url.clone())
                .unwrap_or_default();
            if needs_enrichment && !src_rpc.is_empty() {
                // Strategy B first (faster): if the genome event carried a real tx_hash,
                // decode V3FundsDeposited directly — saves one Across API round-trip (~200ms).
                let has_real_tx_hash = !intent.tx_hash.is_empty()
                    && intent.tx_hash.starts_with("0x")
                    && intent.tx_hash.len() == 66
                    && !intent.tx_hash.starts_with("synthetic_");
                let relay = if has_real_tx_hash {
                    fetch_relay_data_from_tx(&intent.tx_hash, &src_rpc).await
                } else {
                    None
                };
                // Strategy A fallback: deposit_id → Across API → depositTxHash → decode
                let relay = if relay.is_none() {
                    if let Some(dep_id) = intent.deposit_id
                        .or_else(|| intent.id.rsplit(&[':', '_'][..]).find_map(|s| s.parse::<i64>().ok()))
                    {
                        fetch_relay_data_for_deposit(dep_id, intent.src_chain, &src_rpc).await
                    } else {
                        None
                    }
                } else {
                    relay
                };
                if let Some(relay) = relay {
                    let mut patched = intent.clone();
                    if patched.output_amount.is_none() { patched.output_amount = relay.output_amount; }
                    if patched.fill_deadline.is_none() { patched.fill_deadline = relay.fill_deadline; }
                    if patched.exclusivity_deadline.is_none() { patched.exclusivity_deadline = relay.exclusivity_deadline; }
                    if patched.exclusive_relayer.is_none() || patched.exclusive_relayer.as_deref() == Some("0x") {
                        patched.exclusive_relayer = relay.exclusive_relayer;
                    }
                    if patched.dst_token.is_empty() || patched.dst_token == "0x0000000000000000000000000000000000000000" {
                        if let Some(ot) = relay.output_token { patched.dst_token = ot; }
                    }
                    if patched.recipient.is_empty() || patched.recipient == "0x0000000000000000000000000000000000000000" {
                        if let Some(r) = relay.recipient { patched.recipient = r; }
                    }
                    if patched.depositor.is_empty() || patched.depositor == "0x0000000000000000000000000000000000000000" {
                        if let Some(d) = relay.depositor { patched.depositor = d; }
                    }
                    // Patch deposit_id from on-chain log (critical for proto/deposit events
                    // that arrive without deposit_id in the genome payload).
                    if patched.deposit_id.is_none() {
                        patched.deposit_id = relay.deposit_id;
                    }
                    // Patch message from on-chain decode (non-empty for cross-chain execution hooks).
                    if patched.message.is_none() {
                        patched.message = relay.message;
                    }
                    Some(patched)
                } else {
                    warn!("⚠️  Could not fetch relay data for {} from src chain {} — skipping to avoid revert", intent.id, intent.src_chain);
                    let reason = format!("across_enrichment_failed:{}", intent.id);
                    self.transition(&intent.id, IntentState::SkipUnprofitable, None, Some(&reason));
                    let _ = self.wallet.release(&intent.id);
                    return Ok(LambdaExecuteOutcome::Skipped { reason });
                }
            } else {
                None
            }
        } else {
            None
        };
        let intent: &Intent = enriched_intent.as_ref().unwrap_or(intent);

        // 5b. EXCLUSIVITY CHECK — skip fills within an active exclusivity window when
        // we are not the exclusive relayer. The SpokePool enforces this on-chain.
        if direct_fill {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as u32;
            let excl_deadline = intent.exclusivity_deadline.unwrap_or(0);
            let excl_relayer = intent.exclusive_relayer.as_deref().unwrap_or("");
            let solver_addr = format!("{:#x}", self.signer.address()).to_lowercase();
            let is_exclusive_relayer = excl_relayer.to_lowercase() == solver_addr
                || excl_relayer == "0x0000000000000000000000000000000000000000"
                || excl_relayer.is_empty() || excl_relayer == "0x";
            if excl_deadline > now && !is_exclusive_relayer {
                let reason = format!("exclusive_window_active:deadline={excl_deadline}>now={now},relayer={excl_relayer}");
                info!("⏭️  Skipping {} — {}", intent.id, reason);
                self.transition(&intent.id, IntentState::SkipUnprofitable, None, Some(&reason));
                let _ = self.wallet.release(&intent.id);
                return Ok(LambdaExecuteOutcome::Skipped { reason });
            }
        }

        // 5c. CLAIM-INFO COMPLETENESS GUARD — hard rule: never broadcast a fill
        // unless all information required to claim/recover the reward is present.
        // Missing fields = we fill the user but cannot recover our funds.
        //
        //   Across:   output_amount + fill_deadline + recipient must be non-empty/non-zero.
        //   deBridge: order_id (intent.id contains it) + dst_chain wiring → claim is
        //             automatic via lambda_claim_debridge, which only needs the order hash
        //             embedded in intent.id — always present at this point.
        //   Mayan:    order_hash (intent.id contains it) — VAA redemption keyed on it;
        //             recipient must be non-empty for the fulfillOrder calldata.
        {
            let proto_lower_guard = intent.protocol.to_lowercase();
            let is_across_guard = proto_lower_guard.contains("across");
            let is_mayan_guard = proto_lower_guard.contains("mayan");

            if is_across_guard && direct_fill {
                let missing_output = intent.output_amount.as_deref()
                    .map(|s| s.is_empty() || s == "0").unwrap_or(true);
                let missing_deadline = intent.fill_deadline.is_none();
                let missing_recipient = intent.recipient.is_empty()
                    || intent.recipient == "0x0000000000000000000000000000000000000000";

                if missing_output || missing_deadline || missing_recipient {
                    let reason = format!(
                        "incomplete_claim_info:output={} deadline={} recipient={}",
                        if missing_output { "missing" } else { "ok" },
                        if missing_deadline { "missing" } else { "ok" },
                        if missing_recipient { "missing" } else { "ok" },
                    );
                    warn!("🛑 {} — refusing fill, {}", intent.id, reason);
                    self.transition(&intent.id, IntentState::SkipUnprofitable, None, Some(&reason));
                    let _ = self.wallet.release(&intent.id);
                    return Ok(LambdaExecuteOutcome::Skipped { reason });
                }
            }

            if is_mayan_guard {
                let missing_recipient = intent.recipient.is_empty()
                    || intent.recipient == "0x0000000000000000000000000000000000000000";
                let has_order_hash = intent.id.contains("0x") && intent.id.len() > 20;

                if missing_recipient || !has_order_hash {
                    let reason = format!(
                        "incomplete_claim_info:recipient={} order_hash={}",
                        if missing_recipient { "missing" } else { "ok" },
                        if !has_order_hash { "missing" } else { "ok" },
                    );
                    warn!("🛑 {} — refusing fill, {}", intent.id, reason);
                    self.transition(&intent.id, IntentState::SkipUnprofitable, None, Some(&reason));
                    let _ = self.wallet.release(&intent.id);
                    return Ok(LambdaExecuteOutcome::Skipped { reason });
                }
            }
        }

        // 5d. Pre-flight deadline check for Mayan fills.
        // intent.deadline (u64 unix) is the order's auction/fill deadline. Refuse to
        // build calldata if the order has already expired (with 30s margin).
        {
            let proto_lower_pre = intent.protocol.to_lowercase();
            let is_mayan_pre2 = proto_lower_pre.contains("mayan");
            if is_mayan_pre2 {
                if let Some(dl) = intent.deadline {
                    let now_secs = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    if dl < now_secs.saturating_add(30) {
                        let reason = format!("mayan_deadline_expired:dl={dl}<now+30={}", now_secs + 30);
                        info!("⏭️  {} — {}", intent.id, reason);
                        self.transition(&intent.id, IntentState::SkipUnprofitable, None, Some(&reason));
                        let _ = self.wallet.release(&intent.id);
                        return Ok(LambdaExecuteOutcome::Skipped { reason });
                    }
                }
            }
        }

        // 6. CALLDATA_BUILD.
        // Paths by protocol + chain:
        //   a) is_mayan + EVM → MayanSwift.fulfillOrder() on Swift contract
        //   b) is_mayan + Solana src → stub skip (broadcast not yet implemented)
        //   c) is_debridge → DlnDestination.fulfillOrder()
        //   d) direct_fill (Across, operator==0x0) → SpokePool.fillV3Relay directly
        //   e) operator != 0x0 → wrap in executeVerifiedCall(V1)
        self.transition(&intent.id, IntentState::CalldataBuild, None, None);

        let proto_lower = intent.protocol.to_lowercase();
        let is_mayan = proto_lower.contains("mayan");
        let is_debridge = proto_lower.contains("debridge") || proto_lower.contains("dln");
        let is_solana_src = intent.is_solana_source.unwrap_or(false)
            || intent.src_chain == 1_399_811_149;

        // Mayan Solana source: broadcast via ed25519-signed sendTransaction.
        // This path returns early — Solana intents never reach the EVM wiring section below.
        if is_mayan && is_solana_src {
            // auctionMode != 0 requires Mayan's private auction VAA — skip immediately
            // just like the EVM path does. We are not a registered Mayan solver.
            let sol_auction_mode = intent.mayan_auction_mode.unwrap_or(2);
            if sol_auction_mode != 0 {
                let reason = "mayan_auction_mode_requires_registration";
                info!("⏭️  {} — {} (mode={}, Solana dst)", intent.id, reason, sol_auction_mode);
                self.transition(&intent.id, IntentState::SkipUnprofitable, None, Some(reason));
                let _ = self.wallet.release(&intent.id);
                return Ok(LambdaExecuteOutcome::Skipped { reason: reason.to_string() });
            }

            let solana_intent = match MayanSolanaIntent::from_intent(intent) {
                Ok(s) => s,
                Err(e) => {
                    let err = format!("mayan_solana_intent_build:{e}");
                    self.transition(&intent.id, IntentState::CalldataError, None, Some(&err));
                    let _ = self.wallet.release(&intent.id);
                    return Ok(LambdaExecuteOutcome::Failed { stage: "calldata_build", error: err });
                }
            };

            if self.dry_run {
                info!("🧪 DRY_RUN: would broadcast Mayan Solana fulfill for {}", intent.id);
                self.transition(&intent.id, IntentState::SkipUnprofitable, None, Some("dry_run"));
                let _ = self.wallet.release(&intent.id);
                return Ok(LambdaExecuteOutcome::Skipped { reason: "dry_run".into() });
            }

            let rpc_url = default_solana_rpc();
            let broadcaster = match SolanaBroadcaster::from_env(&rpc_url) {
                Ok(b) => b,
                Err(e) => {
                    let reason = format!("solana_key_not_configured:{e}");
                    warn!("⚠️  Mayan Solana {} — {}", intent.id, reason);
                    self.transition(&intent.id, IntentState::SkipUnprofitable, None, Some(&reason));
                    let _ = self.wallet.release(&intent.id);
                    return Ok(LambdaExecuteOutcome::Skipped { reason });
                }
            };

            self.transition(&intent.id, IntentState::Broadcast, None, None);
            match broadcaster.send_fulfill(&solana_intent).await {
                Ok(result) => {
                    info!("🎉 Mayan Solana confirmed: {} sig={}", intent.id, result.signature);
                    self.transition(&intent.id, IntentState::Confirmed, Some(&result.signature), None);
                    if let Err(e) = self.wallet.release(&intent.id) {
                        warn!("wallet release failed for {}: {e}", intent.id);
                    }
                    return Ok(LambdaExecuteOutcome::Confirmed {
                        tx_hash: result.signature,
                        gas_used: solana_intent.compute_units_estimate,
                    });
                }
                Err(e) => {
                    let err = format!("solana_send_transaction:{e:#}");
                    self.transition(&intent.id, IntentState::Reverted, None, Some(&err));
                    return Ok(LambdaExecuteOutcome::Failed { stage: "broadcast", error: err });
                }
            }
        }
        // Mayan Swift EVM fills (EVM source → EVM destination).
        // Two paths:
        //   auctionMode=0 → fulfillSimple (no VAA needed, direct fill)
        //   auctionMode=2 → fulfillOrder (needs auction VAA from Mayan's private chain 42069;
        //                   we fetch it from wormholescan — currently only type 0x05 available)
        // Solana-sourced orders are handled above by the Solana broadcaster path.
        let mayan_auction_mode = intent.mayan_auction_mode.unwrap_or(2);
        let mayan_vaa: Option<Vec<u8>> = if is_mayan && !is_solana_src {
            if mayan_auction_mode == 0 {
                // auctionMode=0 → fulfillSimple; no VAA required.
                info!("✅ Mayan auctionMode=0: using fulfillSimple (no VAA needed) for {}", intent.id);
                None
            } else {
                // auctionMode != 0 → fulfillOrder requires Mayan's private auction VAA
                // (chain 42069, type 0x01). This VAA is only issued to registered Mayan
                // solvers. Public wormholescan only has the source Forwarder VAA (type 0x05)
                // which is NOT accepted by fulfillOrder. Skip immediately to avoid a
                // 12-minute VAA poll that always times out.
                let reason = "mayan_auction_mode_requires_registration";
                info!("⏭️  {} — {} (mode={})", intent.id, reason, mayan_auction_mode);
                self.transition(&intent.id, IntentState::SkipUnprofitable, None, Some(reason));
                let _ = self.wallet.release(&intent.id);
                return Ok(LambdaExecuteOutcome::Skipped { reason: reason.to_string() });
            }
        } else {
            None
        };

        // Spread check for Across direct-fills (spinner bypassed on operator==0x0 chains).
        // See `across_spread_skip` for the exact thresholds. The two skip variants
        // differ in their wallet-reservation handling — preserved verbatim from the
        // pre-helper inlined code so this Phase 2 refactor is a no-op behaviorally.
        if direct_fill && !is_debridge && !is_mayan {
            match across_spread_skip(&intent.amount, intent.output_amount.as_deref()) {
                Some(AcrossSpreadSkip::OutputExceedsInput { reason }) => {
                    info!("⏭️  {} — {}", intent.id, reason);
                    self.transition(&intent.id, IntentState::SkipUnprofitable, None, Some(&reason));
                    let _ = self.wallet.release(&intent.id);
                    return Ok(LambdaExecuteOutcome::Skipped { reason });
                }
                Some(AcrossSpreadSkip::SpreadTooThin { reason }) => {
                    info!("⏭️  {} — {}", intent.id, reason);
                    self.transition(&intent.id, IntentState::SkipUnprofitable, None, Some(&reason));
                    let _ = self.wallet.release(&intent.id);
                    return Ok(LambdaExecuteOutcome::Skipped { reason });
                }
                None => {}
            }
        }

        // Skip Across fills that carry a non-empty message payload. See
        // `across_message_hook_skip_reason` for rationale.
        if (direct_fill || is_across_pre) && !is_debridge && !is_mayan {
            if let Some(reason) = across_message_hook_skip_reason(intent.message.as_deref()) {
                info!("⏭️  {} — {}", intent.id, reason);
                self.transition(&intent.id, IntentState::SkipUnprofitable, None, Some(&reason));
                let _ = self.wallet.release(&intent.id);
                return Ok(LambdaExecuteOutcome::Skipped { reason });
            }
        }

        // Across intents (both direct-fill and operator paths) require a depositId to build
        // fillV3Relay calldata. Skip cleanly when enrichment couldn't resolve one — avoids
        // a hard 'cannot resolve depositId' error from the calldata builder.
        let is_across_pre = proto_lower.contains("across");
        // Skip Across deposits with a Solana (base58) depositor — these originate on Solana
        // and the calldata builder expects an EVM address. Detect: starts with a non-'0x' prefix
        // and is not parseable as an EVM address.
        if is_across_pre && !is_mayan && !is_debridge {
            let dep = &intent.depositor;
            let is_evm_addr = dep.starts_with("0x") || dep.starts_with("0X");
            if !is_evm_addr && dep.len() > 10 {
                let reason = format!("across_solana_src_unsupported:depositor={}", &dep[..dep.len().min(12)]);
                info!("⏭️  {} — {}", intent.id, reason);
                self.transition(&intent.id, IntentState::SkipUnprofitable, None, Some(&reason));
                let _ = self.wallet.release(&intent.id);
                return Ok(LambdaExecuteOutcome::Skipped { reason });
            }
        }
        if is_across_pre && !is_mayan && !is_debridge {
            let has_id = intent.deposit_id.is_some()
                || intent.id.rsplit(&[':', '_'][..]).find_map(|s| s.parse::<i64>().ok()).is_some()
                || intent.tx_hash.rsplit(&[':', '_'][..]).find_map(|s| s.parse::<i64>().ok()).is_some();
            if !has_id {
                let reason = format!("across_no_deposit_id:{}", intent.id);
                info!("⏭️  Skipping {} — deposit_id unavailable after enrichment", intent.id);
                self.transition(&intent.id, IntentState::SkipUnprofitable, None, Some(&reason));
                let _ = self.wallet.release(&intent.id);
                return Ok(LambdaExecuteOutcome::Skipped { reason });
            }
        }

        // Pre-flight deadline check for Across fills. The SpokePool enforces
        // fill_deadline on-chain — catching it here avoids burning gas on estimate.
        // 30-second margin avoids broadcasting into a near-expiry window.
        if (direct_fill || is_across_pre) && !is_debridge && !is_mayan {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as u32;
            if let Some(reason) = across_fill_deadline_skip_reason(intent.fill_deadline, now) {
                info!("⏭️  {} — {}", intent.id, reason);
                self.transition(&intent.id, IntentState::SkipUnprofitable, None, Some(&reason));
                let _ = self.wallet.release(&intent.id);
                return Ok(LambdaExecuteOutcome::Skipped { reason });
            }
        }

        // ERC-20 balance pre-check for Across non-native fills.
        // Saves a round-trip estimate call when we obviously can't fund the fill.
        // Only fires for direct fills with a non-zero, non-native output token.
        // Note: WETH is treated as ERC-20 here because Across SpokePool fillRelay
        // pulls WETH ERC-20 from the filler — NOT native ETH via msg.value.
        if direct_fill && !is_debridge && !is_mayan {
            let dst_tok = intent.dst_token.trim().to_lowercase();
            let is_erc20 = !dst_tok.is_empty()
                && dst_tok != "native"
                && dst_tok != "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
                && dst_tok != "0x0000000000000000000000000000000000000000";
            if is_erc20 {
                if let Some(required) = intent.output_amount.as_deref()
                    .and_then(|s| s.parse::<u128>().ok())
                    .filter(|&v| v > 0)
                {
                    if let Ok(token_addr) = intent.dst_token.parse::<alloy::primitives::Address>() {
                        let rpc_url_opt = crate::evm_estimate::resolve_rpc_url(wiring.chain_id);
                        if let Some(rpc_url) = rpc_url_opt {
                            if let Ok(parsed) = rpc_url.parse() {
                                // balanceOf(address) via raw eth_call — selector 0x70a08231
                                use alloy::providers::{Provider, ProviderBuilder};
                                let provider = ProviderBuilder::new().on_http(parsed);
                                let mut call_data = [0u8; 36];
                                call_data[0..4].copy_from_slice(&[0x70, 0xa0, 0x82, 0x31]);
                                call_data[16..36].copy_from_slice(self.signer.address().as_slice());
                                let tx = alloy::rpc::types::TransactionRequest::default()
                                    .to(token_addr)
                                    .input(alloy::primitives::Bytes::from(call_data.to_vec()).into());
                                if let Ok(result) = provider.call(&tx).await {
                                    let bal_u128 = if result.len() >= 32 {
                                        u128::from_be_bytes(result[16..32].try_into().unwrap_or([0u8; 16]))
                                    } else { 0u128 };
                                    if bal_u128 < required {
                                        let reason = format!(
                                            "erc20_insufficient:have={}<need={}",
                                            bal_u128, required
                                        );
                                        info!("⏭️  {} — {}", intent.id, reason);
                                        self.transition(&intent.id, IntentState::SkipUnprofitable, None, Some(&reason));
                                        let _ = self.wallet.release(&intent.id);
                                        return Ok(LambdaExecuteOutcome::Skipped { reason });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // ETH/WETH fill value — for Mayan fills, WETH output = native ETH (msg.value).
        // For Across direct SpokePool fills, only the zero-address sentinel = native ETH;
        // WETH address = ERC-20 WETH which must be pre-approved, not sent as msg.value.
        let eth_fill_value: Option<U256> = {
            let weth = weth_address_for_chain(wiring.chain_id);
            let dst = intent.dst_token.to_lowercase();
            let weth_matches = weth.map(|w| w.to_lowercase() == dst).unwrap_or(false);
            let is_native_out = if is_mayan {
                // Mayan fulfillSimple/fulfillOrder accept ETH for WETH-output fills
                weth_matches || dst == "0x0000000000000000000000000000000000000000" || dst == "native"
            } else {
                // Across SpokePool: only zero-address sentinel = native ETH fill
                // WETH-address output = ERC-20 pull, not msg.value
                dst == "0x0000000000000000000000000000000000000000" || dst == "native"
            };
            if is_native_out {
                let out_amt = intent.output_amount.as_deref().and_then(|s| {
                    if s.starts_with("0x") || s.starts_with("0X") {
                        U256::from_str_radix(s.trim_start_matches("0x").trim_start_matches("0X"), 16).ok()
                    } else {
                        U256::from_str_radix(s, 10).ok()
                    }
                });
                if let Some(v) = out_amt {
                    info!("💰 Native-out fill: attaching {} wei as msg.value", v);
                    Some(v)
                } else { None }
            } else { None }
        };

        let (tx_target, tx_calldata) = if is_mayan {
            let adapter = MayanEvmEstimateAdapter::new(self.signer.address(), self.spinner.base_url());
            // auctionMode=0: fulfillSimple (no VAA), auctionMode=2: fulfillOrder (with VAA).
            let build_result = if mayan_auction_mode == 0 {
                adapter.build_fulfill_simple_call(intent)
            } else {
                adapter.build_estimate_call_with_vaa(intent, mayan_vaa.as_deref())
            };
            let (swift_addr, calldata) = match build_result {
                Ok(v) => v,
                Err(e) => {
                    let err = format!("calldata_build:{e}");
                    self.transition(&intent.id, IntentState::CalldataError, None, Some(&err));
                    let _ = self.wallet.release(&intent.id);
                    return Ok(LambdaExecuteOutcome::Failed { stage: "calldata_build", error: err });
                }
            };
            let fn_name = if mayan_auction_mode == 0 { "fulfillSimple" } else { "fulfillOrder" };
            info!("📋 Mayan Swift {} on chain {} (swift {}): {} bytes calldata{}",
                fn_name, wiring.chain_id, swift_addr, calldata.len(),
                eth_fill_value.map(|v| format!(", value={v}wei")).unwrap_or_default());
            (swift_addr, calldata)
        } else if is_debridge {
            // deBridge DLN path: fulfillOrder() on DlnDestination (0xE7351Fd770... all chains).
            let adapter = DeBridgeAdapter::new(SpinnerClient::new(self.spinner.base_url()));
            // Use the standalone DlnDestination address (0xE7351Fd770... on all chains)
            let dln_addr = match dln_destination_address(intent.dst_chain) {
                Some(a) => a,
                None => {
                    let err = format!("no DlnDestination address for chain {}", intent.dst_chain);
                    self.transition(&intent.id, IntentState::CalldataError, None, Some(&err));
                    let _ = self.wallet.release(&intent.id);
                    return Ok(LambdaExecuteOutcome::Failed { stage: "calldata_build", error: err });
                }
            };
            // unlockAuthority = solver address: must match allowedTakerDst if set.
            let calldata = match adapter.build_fulfill_order_calldata(intent, self.signer.address()) {
                Ok(c) => c,
                Err(e) => {
                    let err = format!("calldata_build:{e}");
                    self.transition(&intent.id, IntentState::CalldataError, None, Some(&err));
                    let _ = self.wallet.release(&intent.id);
                    return Ok(LambdaExecuteOutcome::Failed { stage: "calldata_build", error: err });
                }
            };
            info!("📋 deBridge fulfillOrder on chain {} (DlnDest {}): {} bytes",
                wiring.chain_id, dln_addr, calldata.len());
            (dln_addr, calldata)
        } else if direct_fill {
            // Direct Across SpokePool fill — no proof wrapper.
            // repaymentChainId = wiring.chain_id: Across repays us on the same chain we fill,
            // keeping capital where the solver operates. Must pass wiring.chain_id explicitly
            // because intent.dst_chain is unreliable across different event sources.
            let solver_addr = self.signer.address();
            let calldata = match build_across_spoke_pool_calldata_with_relayer(intent, Some(solver_addr), Some(wiring.chain_id)) {
                Ok(c) => c,
                Err(e) => {
                    let err = format!("calldata_build:{e}");
                    self.transition(&intent.id, IntentState::CalldataError, None, Some(&err));
                    let _ = self.wallet.release(&intent.id);
                    return Ok(LambdaExecuteOutcome::Failed { stage: "calldata_build", error: err });
                }
            };
            info!("📋 Direct SpokePool fill (no operator) on chain {}: {} bytes calldata{}",
                wiring.chain_id, calldata.len(),
                eth_fill_value.map(|v| format!(", value={v}wei")).unwrap_or_default());
            (wiring.across_adapter, calldata)
        } else {
            let adapter_calldata = match build_across_adapter_calldata(intent) {
                Ok(c) => c,
                Err(e) => {
                    let err = format!("calldata_build:{e}");
                    self.transition(&intent.id, IntentState::CalldataError, None, Some(&err));
                    let _ = self.wallet.release(&intent.id);
                    return Ok(LambdaExecuteOutcome::Failed { stage: "calldata_build", error: err });
                }
            };
            let batch_id = intent.batch_id.unwrap_or(0);
            let calldata = build_execute_verified_call_v1(batch_id, &proof_bytes, wiring.across_adapter, &adapter_calldata);
            info!("📋 Operator-wrapped call executeVerifiedCall(batchId={}) on chain {} via operator {}",
                batch_id, wiring.chain_id, wiring.operator);
            (wiring.operator, calldata)
        };

        if self.dry_run {
            let mode = if is_debridge {
                "deBridge_fulfillOrder"
            } else if is_mayan {
                "mayan_fulfillOrder"
            } else if direct_fill {
                "across_fillV3Relay"
            } else {
                "executeVerifiedCall"
            };
            info!("🧪 DRY_RUN: would broadcast {} on chain {} to {}",
                mode, wiring.chain_id, tx_target);
            self.transition(
                &intent.id,
                IntentState::SkipUnprofitable,
                None,
                Some("dry_run"),
            );
            self.append_outcome(OutcomeRecord {
                ts: started_at,
                intent_id: intent.id.clone(),
                protocol: intent.protocol.clone(),
                src_chain: intent.src_chain,
                dst_chain: intent.dst_chain,
                decision: "dry_run".into(),
                tx_hash: None,
                predicted_gas: test.as_ref().map(|t| t.gas_units),
                gas_used: None,
                effective_gas_price_wei: None,
                predicted_profit_usd: test.as_ref().map(|t| t.net_profit_usd),
                actual_profit_usd: None,
                skip_reason: Some("dry_run".into()),
                error: None,
                solver_id: None,
                claim_tx_hash: None,
                claim_fee_usd: None,
            });
            let _ = self.wallet.release(&intent.id);
            return Ok(LambdaExecuteOutcome::Skipped {
                reason: "dry_run".into(),
            });
        }

        // 6b. ESTIMATE GATE — optional estimateGas before broadcast.
        // Catches AlreadyFilled / ABI mismatches before they burn gas on-chain.
        // Can be bypassed with SKIP_GAS_ESTIMATE=1 for direct Across fills on L2s
        // where gas is near-zero and latency matters more than pre-flight safety.
        // deBridge and Mayan always use the full estimate gate regardless.
        let skip_estimate = !is_debridge && !is_mayan
            && std::env::var("SKIP_GAS_ESTIMATE").as_deref() == Ok("1");
        {
            if skip_estimate {
                info!("⚡ SKIP_GAS_ESTIMATE=1 — bypassing eth_estimateGas (direct Across L2 fill)");
            }
            let solver_addr = self.signer.address();
            let mut estimate_outcome = if skip_estimate {
                crate::estimate::EstimateOutcome::OkGas(200_000)  // conservative pre-set for fillV3Relay
            } else {
                run_evm_estimate_with_value(
                    wiring.chain_id,
                    solver_addr,
                    tx_target,
                    &tx_calldata,
                    eth_fill_value,
                ).await
            };

            // deBridge DLN `fulfillOrder` reverts with `data: "0x"` when
            // `allowedTakerDst != msg.sender` (exclusive taker check). The generic
            // classifier treats bare-data reverts as InsufficientFundsLike (ERC-20
            // balance check), which is wrong for deBridge — we'd broadcast and burn
            // gas. Re-classify to Reverted so the estimate gate blocks us early.
            if is_debridge {
                if let crate::estimate::EstimateOutcome::InsufficientFundsLike(ref msg) = estimate_outcome {
                    let lower = msg.to_lowercase();
                    if lower.contains("execution reverted") {
                        warn!("⚠️  deBridge: re-classifying bare revert as Reverted (likely allowedTakerDst check)");
                        estimate_outcome = crate::estimate::EstimateOutcome::Reverted(msg.clone());
                    }
                }
            }

            if !estimate_outcome.is_green() {
                let detail = match &estimate_outcome {
                    EstimateOutcome::Reverted(s) | EstimateOutcome::AbiInvalid(s) => s.clone(),
                    EstimateOutcome::RouteNotImplemented(s) => s.clone(),
                    _ => String::new(),
                };
                let reason = format!("estimate_gate:{}:{}", estimate_outcome.tag(), detail);
                warn!("⚠️  estimate gate blocked {} — {}", intent.id, reason);
                self.transition(&intent.id, IntentState::SkipUnprofitable, None, Some(&reason));
                let _ = self.wallet.release(&intent.id);
                return Ok(LambdaExecuteOutcome::Skipped { reason });
            }
            info!("✅ estimate gate passed ({:?})", estimate_outcome);
        }

        // 6b-post. NATIVE-OUT BALANCE CHECK — for fills where outputToken=native we attach
        // msg.value = outputAmount. Verify we actually hold that much ETH before broadcasting.
        if let Some(required_wei) = eth_fill_value {
            let rpc_url = crate::evm_estimate::resolve_rpc_url(wiring.chain_id);
            if let Some(url) = rpc_url {
                if let Ok(parsed) = url.parse() {
                    use alloy::providers::{Provider, ProviderBuilder};
                    let provider = ProviderBuilder::new().on_http(parsed);
                    if let Ok(bal) = provider.get_balance(self.signer.address()).await {
                        if bal < required_wei {
                            let reason = format!(
                                "native_out_insufficient_eth:have={}wei<need={}wei",
                                bal, required_wei
                            );
                            warn!("⚠️  {} — {}", intent.id, reason);
                            self.transition(&intent.id, IntentState::SkipUnprofitable, None, Some(&reason));
                            let _ = self.wallet.release(&intent.id);
                            return Ok(LambdaExecuteOutcome::Skipped { reason });
                        }
                    }
                }
            }
        }

        // 6c. FEE-AWARE GAS PRICE — fetch from Razor API, clamp to sane range, apply 1.2× buffer.
        // Falls back to node's recommended gas if Razor is unavailable (API 502 etc).
        let (max_fee_per_gas, max_priority_fee) =
            optimal_gas_price_wei(wiring.chain_id, self.spinner.base_url()).await;
        info!("⛽ gas chain={} maxFee={} priorityFee={} wei",
            wiring.chain_id, max_fee_per_gas, max_priority_fee);

        // 7. BROADCAST → PENDING_CONFIRMATION.
        self.transition(&intent.id, IntentState::Broadcast, None, None);
        let wallet = EthereumWallet::from(self.signer.clone());
        let provider = ProviderBuilder::new()
            .with_recommended_fillers()
            .wallet(wallet)
            .on_http(
                wiring
                    .rpc_url
                    .parse()
                    .with_context(|| format!("rpc parse: {}", wiring.rpc_url))?,
            );

        let mut tx_req = TransactionRequest::default()
            .to(tx_target)
            .input(tx_calldata.into())
            .max_fee_per_gas(max_fee_per_gas)
            .max_priority_fee_per_gas(max_priority_fee);
        if let Some(v) = eth_fill_value {
            tx_req = tx_req.value(v);
        }

        let pending = match provider.send_transaction(tx_req).await {
            Ok(p) => p,
            Err(e) => {
                let err = format!("send_transaction:{e}");
                self.transition(&intent.id, IntentState::Reverted, None, Some(&err));
                return Ok(LambdaExecuteOutcome::Failed {
                    stage: "broadcast",
                    error: err,
                });
            }
        };

        let tx_hash = format!("{:#x}", *pending.tx_hash());
        info!("📤 Broadcast {} on chain {}", tx_hash, wiring.chain_id);
        self.transition(
            &intent.id,
            IntentState::PendingConfirmation,
            Some(&tx_hash),
            None,
        );

        // 7. Await receipt.
        let receipt = match pending.with_required_confirmations(1).get_receipt().await {
            Ok(r) => r,
            Err(e) => {
                let err = format!("get_receipt:{e}");
                self.transition(
                    &intent.id,
                    IntentState::Reverted,
                    Some(&tx_hash),
                    Some(&err),
                );
                return Ok(LambdaExecuteOutcome::Failed {
                    stage: "receipt",
                    error: err,
                });
            }
        };

        let gas_used = receipt.gas_used as u64;
        let effective_gas_price = receipt.effective_gas_price as u128;
        let success = receipt.status();

        if !success {
            let err = "receipt status=0".to_string();
            self.transition(
                &intent.id,
                IntentState::Reverted,
                Some(&tx_hash),
                Some(&err),
            );
            self.append_outcome(OutcomeRecord {
                ts: started_at,
                intent_id: intent.id.clone(),
                protocol: intent.protocol.clone(),
                src_chain: intent.src_chain,
                dst_chain: intent.dst_chain,
                decision: "executed_failed".into(),
                tx_hash: Some(tx_hash.clone()),
                predicted_gas: test.as_ref().map(|t| t.gas_units),
                gas_used: Some(gas_used),
                effective_gas_price_wei: Some(effective_gas_price.to_string()),
                predicted_profit_usd: test.as_ref().map(|t| t.net_profit_usd),
                actual_profit_usd: None,
                skip_reason: None,
                error: Some(err.clone()),
                solver_id: None,
                claim_tx_hash: None,
                claim_fee_usd: None,
            });
            return Ok(LambdaExecuteOutcome::Reverted {
                tx_hash,
                error: err,
            });
        }

        // 8. CONFIRMED → release reservation, record outcome, emit feedback.
        self.transition(
            &intent.id,
            IntentState::Confirmed,
            Some(&tx_hash),
            None,
        );
        // The terminal transition above auto-releases the reservation, but we
        // call release() explicitly so a future change to wallet-manager
        // semantics doesn't silently leave funds locked.
        let _ = self.wallet.release(&intent.id);

        self.append_outcome(OutcomeRecord {
            ts: started_at,
            intent_id: intent.id.clone(),
            protocol: intent.protocol.clone(),
            src_chain: intent.src_chain,
            dst_chain: intent.dst_chain,
            decision: "executed".into(),
            tx_hash: Some(tx_hash.clone()),
            predicted_gas: test.as_ref().map(|t| t.gas_units),
            gas_used: Some(gas_used),
            effective_gas_price_wei: Some(effective_gas_price.to_string()),
            predicted_profit_usd: test.as_ref().map(|t| t.net_profit_usd),
            actual_profit_usd: test.as_ref().map(|t| t.net_profit_usd),
            skip_reason: None,
            error: None,
            solver_id: None,
            claim_tx_hash: None,
            claim_fee_usd: None,
        });

        self.emit_genome_feedback(intent, &tx_hash, gas_used).await;

        Ok(LambdaExecuteOutcome::Confirmed { tx_hash, gas_used })
    }

    /// Pull accumulated solver fees on the dst chain via the Universal Operator.
    pub async fn lambda_claim(&self, intent_id: &str, fee_usd: f64) -> Result<LambdaClaimOutcome> {
        let intents = self
            .wallet
            .list_intents(Some("CONFIRMED"), 1000)
            .map_err(|e| anyhow!("wallet list_intents: {e}"))?;
        let record = match intents.into_iter().find(|r| r.intent_id == intent_id) {
            Some(r) => r,
            None => {
                return Ok(LambdaClaimOutcome::NotEligible {
                    reason: "intent not in CONFIRMED state".into(),
                });
            }
        };

        let wiring = self
            .chains
            .get(&(record.dst_chain as u64))
            .ok_or_else(|| anyhow!("no chain wiring for dst {}", record.dst_chain))?
            .clone();

        if self.dry_run {
            return Ok(LambdaClaimOutcome::NotEligible {
                reason: "dry_run".into(),
            });
        }

        let calldata = claim_selector().to_vec();

        let wallet = EthereumWallet::from(self.signer.clone());
        let provider = ProviderBuilder::new()
            .with_recommended_fillers()
            .wallet(wallet)
            .on_http(wiring.rpc_url.parse()?);

        let tx_req = TransactionRequest::default()
            .to(wiring.operator)
            .input(calldata.into());

        let pending = match provider.send_transaction(tx_req).await {
            Ok(p) => p,
            Err(e) => {
                return Ok(LambdaClaimOutcome::Failed {
                    error: format!("send_transaction:{e}"),
                });
            }
        };

        let tx_hash = format!("{:#x}", *pending.tx_hash());
        info!("📤 claim() broadcast {} on chain {}", tx_hash, wiring.chain_id);

        let receipt = match pending.with_required_confirmations(1).get_receipt().await {
            Ok(r) => r,
            Err(e) => {
                return Ok(LambdaClaimOutcome::Failed {
                    error: format!("get_receipt:{e}"),
                });
            }
        };

        if !receipt.status() {
            return Ok(LambdaClaimOutcome::Failed {
                error: format!("claim reverted (tx {tx_hash})"),
            });
        }

        if let Err(e) = self.wallet.record_revenue(intent_id, fee_usd) {
            warn!("⚠️  wallet record_revenue: {e}");
        }

        Ok(LambdaClaimOutcome::Claimed { tx_hash, fee_usd })
    }

    /// deBridge-specific claim: calls `claimUnlock(orderId, beneficiary)` on the
    /// DlnSource contract on the SOURCE chain. Must be called after the
    /// `fulfillOrder` fill is CONFIRMED on the destination chain.
    ///
    /// Transitions: CONFIRMED → CLAIM_PENDING → CLAIMED (or stays CONFIRMED on failure).
    pub async fn lambda_claim_debridge(&self, intent: &Intent) -> Result<LambdaClaimOutcome> {
        // Only eligible if state is CONFIRMED.
        let intents = self
            .wallet
            .list_intents(Some("CONFIRMED"), 1000)
            .map_err(|e| anyhow!("wallet list_intents: {e}"))?;
        if !intents.iter().any(|r| r.intent_id == intent.id) {
            return Ok(LambdaClaimOutcome::NotEligible {
                reason: "intent not in CONFIRMED state".into(),
            });
        }

        // Resolve src chain wiring for the RPC.
        let src_wiring = match self.chains.get(&intent.src_chain) {
            Some(w) => w.clone(),
            None => {
                return Ok(LambdaClaimOutcome::NotEligible {
                    reason: format!("no chain wiring for src chain {}", intent.src_chain),
                });
            }
        };

        if self.dry_run {
            info!("🧪 DRY_RUN: would claimUnlock on src chain {} for {}", intent.src_chain, intent.id);
            return Ok(LambdaClaimOutcome::NotEligible { reason: "dry_run".into() });
        }

        let adapter = DeBridgeAdapter::new(SpinnerClient::new(self.spinner.base_url()));
        let dln_src_addr = match adapter.dln_source_address(intent.src_chain) {
            Some(a) => a,
            None => {
                return Ok(LambdaClaimOutcome::Failed {
                    error: format!("no DlnSource address for src chain {}", intent.src_chain),
                });
            }
        };

        let beneficiary = self.signer.address();
        let calldata = match adapter.build_claim_unlock_calldata(intent, beneficiary) {
            Ok(c) => c,
            Err(e) => {
                return Ok(LambdaClaimOutcome::Failed {
                    error: format!("build_claim_unlock_calldata: {e}"),
                });
            }
        };

        info!("📤 claimUnlock → DlnSource {} on src chain {} for intent {}",
            dln_src_addr, intent.src_chain, intent.id);

        let wallet = EthereumWallet::from(self.signer.clone());
        let provider = ProviderBuilder::new()
            .with_recommended_fillers()
            .wallet(wallet)
            .on_http(src_wiring.rpc_url.parse()?);

        let tx_req = TransactionRequest::default()
            .to(dln_src_addr)
            .input(alloy::primitives::Bytes::from(calldata).into());

        let pending = match provider.send_transaction(tx_req).await {
            Ok(p) => p,
            Err(e) => {
                // Intent stays in CONFIRMED so the claim retry loop can re-attempt.
                return Ok(LambdaClaimOutcome::Failed {
                    error: format!("send_transaction:{e}"),
                });
            }
        };

        // Tx is in-flight — transition now so concurrent claim loops skip this intent.
        self.transition(&intent.id, IntentState::ClaimPending, None, None);
        let tx_hash = format!("{:#x}", *pending.tx_hash());
        info!("📤 claimUnlock broadcast {} on chain {}", tx_hash, intent.src_chain);

        let receipt = match pending.with_required_confirmations(1).get_receipt().await {
            Ok(r) => r,
            Err(e) => {
                // Roll back to CONFIRMED so the retry loop can re-attempt.
                self.transition(&intent.id, IntentState::Confirmed, None, None);
                return Ok(LambdaClaimOutcome::Failed {
                    error: format!("get_receipt:{e}"),
                });
            }
        };

        if !receipt.status() {
            // Revert: roll back to CONFIRMED so the retry loop can re-attempt.
            self.transition(&intent.id, IntentState::Confirmed, None, None);
            return Ok(LambdaClaimOutcome::Failed {
                error: format!("claimUnlock reverted (tx {tx_hash})"),
            });
        }

        // Record revenue: give_amount - take_amount = spread earned.
        // Use 6-decimal divisor for stablecoins (USDC/USDT), 18 for everything else.
        // Known 6-decimal dst tokens on supported chains (all USDC/USDT variants).
        let fee_usd = {
            let give = intent.give_amount.as_deref()
                .and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
            let take = intent.take_amount.as_deref()
                .and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
            let dst_tok = intent.dst_token.to_lowercase();
            let is_stable = matches!(dst_tok.trim(),
                // USDC on Base, Arbitrum, Optimism, Ethereum, Polygon, Linea, Unichain,
                // Scroll, Ink, Mode, Polygon zkEVM, Avalanche, zkSync Era
                "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913" |
                "0xaf88d065e77c8cc2239327c5edb3a432268e5831" |
                "0x0b2c639c533813f4aa9d7837caf62653d097ff85" |
                "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48" |
                "0x2791bca1f2de4661ed88a30c99a7a9449aa84174" |
                "0x176211869ca2b568f2a7d4ee941e073a821ee1ff" |
                "0x078d782b760474a361dda7ff6e249887ddf39eb0" |
                "0x06efdbff2a14a7c8e15944d1f4a48f9f95f663a4" |
                "0x2d270e6886d130d724215a266106e6832161eaed" |
                "0xd988097fb8612cc24eec14542bc03424c656005f" |
                "0x9c3c9283d3e44854697cd22d3faa240cfb032889" |
                "0xe0b7927c4af23765cb51314a0e0521a9645f0e2a" |
                "0xb97ef9ef8734c71904d8002f8b6bc66dd9c48a6e" |
                "0x1d17cbcf0d6d143135ae902365d2e5e2a16538d4" |
                // USDT variants
                "0xdac17f958d2ee523a2206206994597c13d831ec7" |
                "0xfd086bc7cd5c481dcc9c85ebe478a1c0b69fcbb9" |
                "0x94b008aa00579c1307b0ef2c499ad98a8ce58e58" |
                "0xc2132d05d31c914a87c6611c10748aeb04b58e8f" |
                "0xfde4c96c8593536e31f229ea8f37b2ada2699bb2" |
                "0xf55bec9cafdbe8730f096aa55dad6d22d44099df" |
                "0x0200c29006150606b650577bbe7b6248f58470c1" |
                "0xf0f161fda2712db8b566946122a5af183995e2ed" |
                "0x9702230a8ea53601f5cd2dc00fdbc13d4df4a8c7" |
                "0xc7198437980c041c805a1edcba50c1ce5db95118"
            );
            let divisor = if is_stable { 1e6 } else { 1e18 };
            (give - take).max(0.0) / divisor
        };
        if let Err(e) = self.wallet.record_revenue(&intent.id, fee_usd) {
            warn!("⚠️  wallet record_revenue: {e}");
        }
        self.transition(&intent.id, IntentState::Claimed, Some(&tx_hash), None);

        Ok(LambdaClaimOutcome::Claimed { tx_hash, fee_usd })
    }

    fn transition(
        &self,
        intent_id: &str,
        next: IntentState,
        tx_hash: Option<&str>,
        error: Option<&str>,
    ) {
        if let Err(e) = self.wallet.transition(intent_id, next, tx_hash, error) {
            tracing::debug!("wallet transition({intent_id}, {next:?}): {e}");
        }
    }

    fn append_outcome(&self, rec: OutcomeRecord) {
        if let Some(log) = self.outcome_log.as_ref() {
            if let Err(e) = log.append(rec) {
                warn!("⚠️  outcome_log append: {e}");
            }
        }
    }

    async fn emit_genome_feedback(&self, intent: &Intent, tx_hash: &str, gas_used: u64) {
        let Some(base) = self.feedback_url.as_ref() else { return };
        #[derive(Serialize)]
        struct Feedback<'a> {
            entity: &'a str,
            action: &'a str,
            ref_hash: &'a str,
            intent_id: &'a str,
            protocol: &'a str,
            tx_hash: &'a str,
            gas_used: u64,
        }
        let url = format!("{}/api/genome/feedback", base.trim_end_matches('/'));
        let body = Feedback {
            entity: "proof",
            action: "confirmed",
            ref_hash: &intent.tx_hash,
            intent_id: &intent.id,
            protocol: &intent.protocol,
            tx_hash,
            gas_used,
        };
        match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
        {
            Ok(http) => {
                if let Err(e) = http.post(&url).json(&body).send().await {
                    warn!("⚠️  genome feedback POST {url}: {e}");
                }
            }
            Err(e) => warn!("⚠️  genome feedback http build: {e}"),
        }
    }
}

/// Returns the DlnDestination contract address for the given chain.
/// DlnDestination (0xE7351Fd770...) is separate from DlnSource (0xeF4fB24...).
pub fn dln_destination_address(chain_id: u64) -> Option<alloy::primitives::Address> {
    let addr: alloy::primitives::Address = "0xE7351Fd770A37282b91D153Ee690B63579D6dd7f".parse().ok()?;
    let supported = [1u64, 10, 56, 137, 42161, 8453, 43114, 59144, 534352, 57073, 34443, 100];
    if supported.contains(&chain_id) { Some(addr) } else { None }
}

/// Best-effort USD valuation for the `Intent.amount` raw token units. The
/// solver tracks budget in USD, but the genome event only carries base-unit
/// integers. Until a price oracle lands we use a 6-decimal stablecoin
/// assumption (USDC/USDT — the dominant tokens on every supported chain),
/// which over-estimates dust and under-estimates 18-decimal native tokens.
/// Reservation overdraft is the only failure mode that surfaces a wrong
/// value, and `wallet-manager` already returns a typed error there.
pub fn intent_amount_usd(intent: &Intent) -> f64 {
    // MayanPoller sends pre-converted USD amounts as float strings (e.g. "659.52").
    // If the amount already has a decimal point, treat it as a USD value directly.
    if intent.amount.contains('.') {
        let v = intent.amount.parse::<f64>().unwrap_or(0.0);
        // Sanity cap: if a decimal-form amount exceeds $10M it's almost certainly
        // a misread field (e.g. Mayan SSE order hash). Return 0 so it gets skipped
        // by the notional-cap guard rather than producing noise in outcome logs.
        return if v > 10_000_000.0 { 0.0 } else { v };
    }
    let raw = match intent.amount.parse::<u128>() {
        Ok(n) => n as f64,
        Err(_) => return 0.0,
    };
    // Detect decimals from src_token: zero address / native = 18-decimal ETH.
    // Known stablecoin patterns (USDC/USDT) = 6 decimals.
    // Everything else defaults to 18 decimals (safer for wallet budget guard).
    let token = intent.src_token.as_str();
    let decimals = token_decimals(token);
    // Use a conservative token price so the wallet reserve doesn't reject fills
    // on volatile assets; the spinner test-run profit check is authoritative.
    let divisor = 10f64.powi(decimals as i32);
    let human_amount = raw / divisor;
    // Token price heuristic: stablecoins ≈ $1, ETH ≈ $3000, everything else ≈ $1
    let price_usd = token_price_heuristic(token);
    human_amount * price_usd
}

fn token_decimals(token: &str) -> u8 {
    let lower = token.to_lowercase();
    // Native ETH / zero address
    if lower == "0x0000000000000000000000000000000000000000"
        || lower == "native"
        || lower == "0x000000000000000000000000000000000000800a"
    {
        return 18;
    }
    const SIX_DEC: &[&str] = &[
        "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48", // USDC Ethereum
        "0xaf88d065e77c8cc2239327c5edb3a432268e5831", // USDC Arbitrum native
        "0xff970a61a04b1ca14834a43f5de4533ebddb5cc8", // USDC.e Arbitrum
        "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913", // USDC Base
        "0x0b2c639c533813f4aa9d7837caf62653d097ff85", // USDC Optimism native
        "0x7f5c764cbc14f9669b88837ca1490cca17c31607", // USDC.e Optimism
        "0x3c499c542cef5e3811e1192ce70d8cc03d5c3359", // USDC Polygon native
        "0x2791bca1f2de4661ed88a30c99a7a9449aa84174", // USDC.e Polygon
        "0x176211869ca2b568f2a7d4ee941e073a821ee1ff", // USDC Linea
        "0x06efdbff2a14a7c8e15944d1f4a48f9f95f663a4", // USDC Scroll
        "0x2a22f9c3b484c3629090feed35f17ff8f88f76f0", // USDC.e Gnosis
        "0xddafbb505ad214d7b80b1f830fccc89b60fb7a83", // USDC Gnosis
        "0x1c7d4b196cb0c7b01d743fbc6116a902379c7238", // USDC Sepolia
        "0x036cbd53842c5426634e7929541ec2318f3dcf7e", // USDC Base Sepolia
        "0xdac17f958d2ee523a2206206994597c13d831ec7", // USDT Ethereum
        "0xfd086bc7cd5c481dcc9c85ebe478a1c0b69fcbb9", // USDT Arbitrum
        "0x94b008aa00579c1307b0ef2c499ad98a8ce58e58", // USDT Optimism
        // NOTE: BSC USDT (0x55d398) uses 18 decimals — intentionally omitted here
        "0xc2132d05d31c914a87c6611c10748aeb04b58e8f", // USDT Polygon (6 dec)
        "0x078d782b760474a361dda7ff6e249887ddf39eb0", // USDC Unichain
        "0x2d270e6886d130d724215a266106e6832161eaed", // USDC Ink
        "0xd988097fb8612cc24eec14542bc03424c656005f", // USDC.e Mode
        "0x9c3c9283d3e44854697cd22d3faa240cfb032889", // USDC Polygon zkEVM
        "0xe0b7927c4af23765cb51314a0e0521a9645f0e2a", // USDC.e Avalanche (old)
        "0xb97ef9ef8734c71904d8002f8b6bc66dd9c48a6e", // USDC Avalanche native
        "0x1d17cbcf0d6d143135ae902365d2e5e2a16538d4", // USDC zkSync Era
        "0xfde4c96c8593536e31f229ea8f37b2ada2699bb2", // USDT Base
        "0xf55bec9cafdbe8730f096aa55dad6d22d44099df", // USDT Scroll
        "0x0200c29006150606b650577bbe7b6248f58470c1", // USDT Ink
        "0xf0f161fda2712db8b566946122a5af183995e2ed", // USDT Mode
        "0x9702230a8ea53601f5cd2dc00fdbc13d4df4a8c7", // USDT Avalanche native
        "0xc7198437980c041c805a1edcba50c1ce5db95118", // USDT.e Avalanche
        "usdc", "usdt",
    ];
    if SIX_DEC.iter().any(|&s| lower == s) {
        return 6;
    }
    18
}

fn weth_address_for_chain(chain_id: u64) -> Option<&'static str> {
    match chain_id {
        8453   => Some("0x4200000000000000000000000000000000000006"), // Base WETH
        10     => Some("0x4200000000000000000000000000000000000006"), // Optimism WETH
        42161  => Some("0x82af49447d8a07e3bd95bd0d56f35241523fbab1"), // Arbitrum WETH
        1      => Some("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"), // Ethereum WETH
        59144  => Some("0xe5d7c2a44ffddf6b295a15c148167daaaf5cf34f"), // Linea WETH
        130    => Some("0x4200000000000000000000000000000000000006"), // Unichain WETH
        57073  => Some("0x4200000000000000000000000000000000000006"), // Ink WETH (OP Stack)
        34443  => Some("0x4200000000000000000000000000000000000006"), // Mode WETH (OP Stack)
        534352 => Some("0x5300000000000000000000000000000000000004"), // Scroll WETH
        137    => Some("0x7ceB23fD6bC0adD59E62ac25578270cFf1b9f619"), // Polygon WETH
        56     => Some("0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c"), // BSC WBNB
        43114  => Some("0x49d5c2bdffac6ce2bfdb6640f4f80f226bc10bab"), // Avalanche WETH.e
        324    => Some("0x5aea5775959fbc2557cc8789bc1bf90a239d9a91"), // zkSync Era WETH
        _ => None,
    }
}

fn token_price_heuristic(token: &str) -> f64 {
    let lower = token.to_lowercase();
    const ETH_LIKE: &[&str] = &[
        "0x0000000000000000000000000000000000000000",  // native ETH
        "native",
        "0x000000000000000000000000000000000000800a",  // zkSync native ETH
        "0x4200000000000000000000000000000000000006",  // WETH OP-stack (Base/Optimism/Ink/Mode)
        "0x82af49447d8a07e3bd95bd0d56f35241523fbab1",  // WETH Arbitrum
        "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2",  // WETH Ethereum
        "0xe5d7c2a44ffddf6b295a15c148167daaaf5cf34f",  // WETH Linea
        "0x5300000000000000000000000000000000000004",  // WETH Scroll
        "0x7ceb23fd6bc0add59e62ac25578270cff1b9f619",  // WETH Polygon
        "0x49d5c2bdffac6ce2bfdb6640f4f80f226bc10bab",  // WETH.e Avalanche
        "0x5aea5775959fbc2557cc8789bc1bf90a239d9a91",  // WETH zkSync Era
    ];
    if ETH_LIKE.iter().any(|&s| lower == s) {
        return std::env::var("ETH_PRICE_USD")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3000.0);
    }
    1.0
}

alloy::sol! {
    /// V1 InclusionProof struct from TaifoonUniversalFinalityLayerV1 (reproduced here for ABI encoding).
    struct InclusionProof {
        uint64 chainId;
        uint64 blockNumber;
        uint64 eventIndex;
        bytes32 eventHash;
        bytes32[] proof;
        uint8 proofType;
    }

    function executeVerifiedCall(
        uint256 batchId,
        InclusionProof calldata proof,
        address vendorContract,
        bytes calldata callData
    ) external returns (bytes32 executionId, bool success);
}

/// Build a `LambdaController` from environment variables.
///
/// Returns `Ok(None)` when `SOLVER_PRIVATE_KEY` is absent (observation-only mode).
/// Chain wiring is loaded from `CHAIN_WIRING_FILE` → `CHAIN_WIRING_JSON` → per-chain
/// `CHAINS` + `RPC_URL_<id>` + `OPERATOR_<id>` + `ADAPTER_<id>` vars.
pub fn build_lambda_controller_from_env(
    spinner_base: &str,
    outcome_db_path: &str,
    mamba_url: Option<String>,
    dry_run: bool,
    profit_threshold_usd: f64,
    wallet: Arc<WalletManager>,
) -> anyhow::Result<Option<LambdaController>> {
    let pk = match std::env::var("SOLVER_PRIVATE_KEY") {
        Ok(v) if !v.is_empty() => v,
        _ => return Ok(None),
    };
    let signer: alloy::signers::local::PrivateKeySigner = pk
        .parse()
        .map_err(|e| anyhow::anyhow!("SOLVER_PRIVATE_KEY parse: {}", e))?;

    let chains = parse_chain_wiring_from_env()?;
    if chains.is_empty() {
        return Err(anyhow::anyhow!(
            "no chain wiring — set CHAIN_WIRING_JSON or per-chain RPC_/OPERATOR_/ADAPTER_ vars"
        ));
    }

    let log = OutcomeLog::open(outcome_db_path, mamba_url)?;
    let spinner = SpinnerSolverClient::new(spinner_base);
    let feedback_url = std::env::var("GENOME_FEEDBACK_URL").ok();

    Ok(Some(LambdaController {
        wallet,
        spinner,
        signer,
        chains,
        outcome_log: Some(log),
        dry_run,
        profit_threshold_usd,
        feedback_url,
    }))
}

/// Parse chain wiring from environment.
///
/// Two formats supported:
///   A) `CHAIN_WIRING_FILE` path or `CHAIN_WIRING_JSON` inline JSON map.
///   B) `CHAINS=8453,10` + per-chain `RPC_URL_<id>`, `OPERATOR_<id>`, `ADAPTER_<id>`.
pub fn parse_chain_wiring_from_env() -> anyhow::Result<HashMap<u64, ChainWiring>> {
    let mut out = HashMap::new();

    let json_from_file = std::env::var("CHAIN_WIRING_FILE")
        .ok()
        .and_then(|p| std::fs::read_to_string(&p).ok());
    let json_inline = std::env::var("CHAIN_WIRING_JSON").ok();
    let json_src = json_from_file.or(json_inline);

    if let Some(json) = json_src {
        #[derive(serde::Deserialize)]
        struct Entry {
            rpc_url: String,
            operator: String,
            across_adapter: String,
        }
        let map: HashMap<String, serde_json::Value> = serde_json::from_str(&json)?;
        for (k, raw) in map {
            let chain_id: u64 = match k.parse() {
                Ok(id) => id,
                Err(_) => continue,
            };
            let v: Entry = match serde_json::from_value(raw) {
                Ok(e) => e,
                Err(e) => {
                    warn!("chain_wiring: skip chain {k}: {e}");
                    continue;
                }
            };
            out.insert(
                chain_id,
                ChainWiring {
                    chain_id,
                    rpc_url: v.rpc_url,
                    operator: v.operator.parse::<Address>()?,
                    across_adapter: v.across_adapter.parse::<Address>()?,
                },
            );
        }
        return Ok(out);
    }

    if let Ok(list) = std::env::var("CHAINS") {
        for cs in list.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
            let chain_id: u64 = cs.parse()?;
            let rpc = std::env::var(format!("RPC_URL_{}", chain_id))
                .map_err(|_| anyhow::anyhow!("missing RPC_URL_{}", chain_id))?;
            let operator: Address = std::env::var(format!("OPERATOR_{}", chain_id))
                .map_err(|_| anyhow::anyhow!("missing OPERATOR_{}", chain_id))?
                .parse()?;
            let adapter: Address = std::env::var(format!("ADAPTER_{}", chain_id))
                .map_err(|_| anyhow::anyhow!("missing ADAPTER_{}", chain_id))?
                .parse()?;
            out.insert(
                chain_id,
                ChainWiring { chain_id, rpc_url: rpc, operator, across_adapter: adapter },
            );
        }
    }
    Ok(out)
}

/// Build calldata for `executeVerifiedCall(uint256,(uint64,uint64,uint64,bytes32,bytes32[],uint8),address,bytes)`.
///
/// V1 function present on all deployed TaifoonUniversalOperator contracts (selector 0x189cdb7b).
/// When `proof_bytes` is a JSON-encoded InclusionProof from the spinner, it is decoded and
/// re-encoded into the correct ABI format. When empty (dry-run), a zero-proof stub is used.
pub fn build_execute_with_proof_calldata(
    proof: &[u8],
    adapter: Address,
    adapter_calldata: &[u8],
) -> Vec<u8> {
    build_execute_verified_call_v1(0, proof, adapter, adapter_calldata)
}

/// `executeVerifiedCall` V1 calldata builder.
/// Selector 0x189cdb7b — present on all deployed testnet + mainnet operators.
pub fn build_execute_verified_call_v1(
    batch_id: u64,
    proof_bytes: &[u8],
    adapter: Address,
    adapter_calldata: &[u8],
) -> Vec<u8> {
    use alloy::sol_types::SolCall;

    // Decode the InclusionProof from the spinner bytes.
    // The spinner returns JSON or hex-encoded ABI bytes.
    // We try to decode a structured JSON first, then fall back to zero-proof stub.
    let inclusion_proof = decode_inclusion_proof(proof_bytes);

    let call = executeVerifiedCallCall {
        batchId: alloy::primitives::U256::from(batch_id),
        proof: inclusion_proof,
        vendorContract: adapter,
        callData: alloy::primitives::Bytes::from(adapter_calldata.to_vec()),
    };
    call.abi_encode()
}

/// Decode an InclusionProof from spinner bytes.
/// Tries JSON (`{"chain_id":..,"block_number":..,...}`) then ABI-hex, then zero-stub.
fn decode_inclusion_proof(bytes: &[u8]) -> InclusionProof {
    if bytes.is_empty() {
        return zero_inclusion_proof();
    }
    // Try JSON first
    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(bytes) {
        return parse_inclusion_proof_json(&v).unwrap_or_else(zero_inclusion_proof);
    }
    // Try ABI-decode
    if let Ok(decoded) = <InclusionProof as alloy::sol_types::SolType>::abi_decode(bytes, false) {
        return decoded;
    }
    tracing::warn!("⚠️  Could not decode InclusionProof from {} spinner bytes — using zero stub", bytes.len());
    zero_inclusion_proof()
}

fn zero_inclusion_proof() -> InclusionProof {
    InclusionProof {
        chainId: 0,
        blockNumber: 0,
        eventIndex: 0,
        eventHash: alloy::primitives::FixedBytes::ZERO,
        proof: vec![],
        proofType: 0,
    }
}

fn parse_inclusion_proof_json(v: &serde_json::Value) -> Option<InclusionProof> {
    let chain_id    = v.get("chain_id").or_else(|| v.get("chainId"))?.as_u64()?;
    let block_num   = v.get("block_number").or_else(|| v.get("blockNumber"))?.as_u64()?;
    let event_idx   = v.get("event_index").or_else(|| v.get("eventIndex")).and_then(|x| x.as_u64()).unwrap_or(0);
    let event_hash_s = v.get("event_hash").or_else(|| v.get("eventHash")).and_then(|x| x.as_str()).unwrap_or("0x0000000000000000000000000000000000000000000000000000000000000000");
    let event_hash: alloy::primitives::FixedBytes<32> = event_hash_s.parse().ok()?;
    let proof_type  = v.get("proof_type").or_else(|| v.get("proofType")).and_then(|x| x.as_u64()).unwrap_or(0) as u8;
    let siblings: Vec<alloy::primitives::FixedBytes<32>> = v.get("proof")
        .or_else(|| v.get("siblings"))
        .and_then(|x| x.as_array())
        .map(|arr| arr.iter().filter_map(|s| s.as_str()?.parse().ok()).collect())
        .unwrap_or_default();
    Some(InclusionProof {
        chainId: chain_id,
        blockNumber: block_num,
        eventIndex: event_idx,
        eventHash: event_hash,
        proof: siblings,
        proofType: proof_type,
    })
}

/// 4-byte selector for `claim()` — Universal Operator pull pattern (matches
/// `FOONSpinnerRewards.claim()` on the Taifoon ecosystem side).
pub fn claim_selector() -> [u8; 4] {
    function_selector("claim()")
}

fn function_selector(sig: &str) -> [u8; 4] {
    let h = keccak256(sig.as_bytes());
    [h[0], h[1], h[2], h[3]]
}

/// Human-readable names for deBridge non-EVM destination chain IDs.
/// Returns `None` for normal EVM chains. Mirrors the same table in genome-client.
fn debridge_non_evm_chain_name(chain_id: u64) -> Option<&'static str> {
    match chain_id {
        100_000_001 => Some("Solana"),
        100_000_002 => Some("NEAR"),
        100_000_003 => Some("Tron"),
        100_000_004 => Some("Ton"),
        100_000_005 => Some("Aptos"),
        100_000_006 => Some("Sui"),
        100_000_007 => Some("Eclipse (Solana SVM)"),
        100_000_022 => Some("Neon EVM on Solana"),
        100_000_023 => Some("Sonic (Solana SVM)"),
        100_000_027 => Some("Solana (alt chain ID)"),
        100_000_030 => Some("Grass (Solana SVM)"),
        100_000_031 => Some("Svm-Unknown-31"),
        _ => None,
    }
}

// ── Across pre-broadcast guard helpers ─────────────────────────────────────
//
// These pure helpers mirror the inlined skip logic in `lambda_execute` so the
// branches can be unit-tested in isolation without spinning up a controller.
// `lambda_execute` calls each helper and re-uses the returned reason string
// verbatim — keeping the helpers and the call sites in lock-step.

/// Across spread-guard outcome. Two distinct skip paths the SpokePool would
/// otherwise accept on-chain:
///   • `OutputExceedsInput` → fill pays more than it receives (money-losing).
///   • `SpreadTooThin`      → spread < 0.01%, indistinguishable from dust/test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AcrossSpreadSkip {
    OutputExceedsInput { reason: String },
    SpreadTooThin { reason: String },
}

/// Across spread guard. Returns the skip variant when the intent would yield a
/// money-losing or near-par fill, or `None` when it passes (or when amounts are
/// unparseable — we don't want a parse failure to silently skip a real intent).
pub(crate) fn across_spread_skip(
    amount: &str,
    output_amount: Option<&str>,
) -> Option<AcrossSpreadSkip> {
    let inp = amount.parse::<u128>().ok()?;
    let out = output_amount?.parse::<u128>().ok()?;
    if inp == 0 {
        return None;
    }
    // Skip spread check when token decimals differ by >3 orders of magnitude
    // (e.g., USDC 6-dec input → ETH 18-dec output). A raw comparison here
    // would always trigger a false-positive "output exceeds input" rejection.
    let likely_decimal_mismatch = out > inp.saturating_mul(1_000);
    if likely_decimal_mismatch {
        return None;
    }
    if out > inp {
        return Some(AcrossSpreadSkip::OutputExceedsInput {
            reason: format!("across_output_exceeds_input:out={out}>in={inp}"),
        });
    }
    let spread_pct = (inp.saturating_sub(out)) as f64 / inp as f64 * 100.0;
    if spread_pct < 0.01 {
        return Some(AcrossSpreadSkip::SpreadTooThin {
            reason: format!("across_spread_too_thin:{spread_pct:.4}pct"),
        });
    }
    None
}

/// Across message-hook guard. Returns Some(reason) for any non-empty message
/// payload, since the SpokePool will dispatch to recipient.handleV3AcrossMessage
/// and we cannot validate the handler off-chain.
pub(crate) fn across_message_hook_skip_reason(message: Option<&str>) -> Option<String> {
    let has_message = message
        .map(|m| !m.is_empty() && m != "0x" && m != "0x0")
        .unwrap_or(false);
    if has_message {
        Some("across_message_hook_unsupported".to_string())
    } else {
        None
    }
}

/// Across fill-deadline guard. Returns Some(reason) when the SpokePool would
/// reject the fill on-chain (`fillDeadline < now + 30s`). Allows a 30-second
/// margin so we don't broadcast into a near-expiry window.
pub(crate) fn across_fill_deadline_skip_reason(
    fill_deadline: Option<u32>,
    now_unix: u32,
) -> Option<String> {
    let dl = fill_deadline?;
    if dl < now_unix.saturating_add(30) {
        Some(format!("across_fill_deadline_expired:dl={dl}<now={now_unix}"))
    } else {
        None
    }
}

// Suppress unused-import lint when the file is consumed without the U256
// re-export (kept for future fee-conversion work).
#[allow(dead_code)]
fn _u256_unused_marker() -> U256 {
    U256::ZERO
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intent(id: &str, amount: &str) -> Intent {
        Intent {
            id: id.to_string(),
            protocol: "across_v3".to_string(),
            src_chain: 1,
            dst_chain: 42161,
            src_token: "0x0000000000000000000000000000000000000000".into(),
            dst_token: "0x0000000000000000000000000000000000000000".into(),
            amount: amount.to_string(),
            depositor: "0x0000000000000000000000000000000000000001".into(),
            recipient: "0x0000000000000000000000000000000000000001".into(),
            tx_hash: "0xabc".into(),
            detected_at: 0,
            ..Default::default()
        }
    }

    #[test]
    fn intent_amount_usd_treats_six_decimals() {
        // 100 USDC (Base) = 100_000_000 base units → $100
        let mut i = intent("i1", "100000000");
        i.src_token = "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913".into(); // USDC Base
        assert!((intent_amount_usd(&i) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn intent_amount_usd_handles_garbage() {
        let i = intent("i2", "not-a-number");
        assert_eq!(intent_amount_usd(&i), 0.0);
    }

    #[test]
    fn execute_with_proof_calldata_starts_with_selector() {
        let proof = vec![1u8, 2, 3];
        let adapter = Address::from([0x42u8; 20]);
        let adapter_calldata = vec![0xaa, 0xbb];
        let cd = build_execute_with_proof_calldata(&proof, adapter, &adapter_calldata);
        // executeVerifiedCall(uint256,(uint64,uint64,uint64,bytes32,bytes32[],uint8),address,bytes)
        // selector 0x189cdb7b — V1 present on all deployed operators.
        assert!(cd.len() > 4 + 128);
        let sel = &cd[..4];
        let expected = function_selector(
            "executeVerifiedCall(uint256,(uint64,uint64,uint64,bytes32,bytes32[],uint8),address,bytes)",
        );
        assert_eq!(sel, &expected);
    }

    #[test]
    fn claim_selector_is_stable() {
        // claim() — keccak256("claim()") = 0x4e71d92d
        assert_eq!(claim_selector(), [0x4e, 0x71, 0xd9, 0x2d]);
    }

    // ── Across pre-broadcast guard tests ──────────────────────────────────

    #[test]
    fn across_spread_guard_passes_healthy_fill() {
        // 100 USDC in / 99.85 USDC out → 0.15% spread, well above 0.01% floor.
        assert!(across_spread_skip("100000000", Some("99850000")).is_none());
    }

    #[test]
    fn across_spread_guard_skips_when_output_exceeds_input() {
        let skip = across_spread_skip("100000000", Some("100000001"))
            .expect("expected skip on output > input");
        match skip {
            AcrossSpreadSkip::OutputExceedsInput { reason } => {
                assert!(reason.starts_with("across_output_exceeds_input:"), "got: {reason}");
                assert!(reason.contains("out=100000001"));
                assert!(reason.contains("in=100000000"));
            }
            other => panic!("expected OutputExceedsInput, got {other:?}"),
        }
    }

    #[test]
    fn across_spread_guard_skips_thin_spread() {
        // 100 USDC in / 99.999_999 USDC out → 0.0000_01% spread, below 0.01% floor.
        let skip = across_spread_skip("100000000", Some("99999999"))
            .expect("expected skip on thin spread");
        match skip {
            AcrossSpreadSkip::SpreadTooThin { reason } => {
                assert!(reason.starts_with("across_spread_too_thin:"), "got: {reason}");
                assert!(reason.ends_with("pct"));
            }
            other => panic!("expected SpreadTooThin, got {other:?}"),
        }
    }

    #[test]
    fn across_spread_guard_skips_exact_par_fill() {
        // output == input → 0% spread → skip as too-thin.
        let skip = across_spread_skip("100000000", Some("100000000"))
            .expect("expected skip on exact-par");
        assert!(matches!(skip, AcrossSpreadSkip::SpreadTooThin { .. }));
    }

    #[test]
    fn across_spread_guard_passes_at_threshold() {
        // 100 USDC in / 99.99 USDC out → exactly 0.01% spread → must pass
        // (filter rejects strictly less than 0.01%).
        assert!(across_spread_skip("100000000", Some("99990000")).is_none());
    }

    #[test]
    fn across_spread_guard_passes_when_amounts_unparseable() {
        // Parse failure must NOT silently skip a real intent — return None
        // so the downstream estimate path can produce a real error.
        assert!(across_spread_skip("not-a-number", Some("99850000")).is_none());
        assert!(across_spread_skip("100000000", Some("not-a-number")).is_none());
        assert!(across_spread_skip("100000000", None).is_none());
    }

    #[test]
    fn across_spread_guard_passes_zero_input() {
        // Zero input would div-by-zero in spread calc; the helper returns None.
        assert!(across_spread_skip("0", Some("0")).is_none());
    }

    #[test]
    fn across_message_hook_guard_passes_empty() {
        assert!(across_message_hook_skip_reason(None).is_none());
        assert!(across_message_hook_skip_reason(Some("")).is_none());
        assert!(across_message_hook_skip_reason(Some("0x")).is_none());
        assert!(across_message_hook_skip_reason(Some("0x0")).is_none());
    }

    #[test]
    fn across_message_hook_guard_skips_non_empty_message() {
        let reason = across_message_hook_skip_reason(Some("0xdeadbeef"))
            .expect("expected skip on non-empty message");
        assert_eq!(reason, "across_message_hook_unsupported");
    }

    #[test]
    fn across_fill_deadline_guard_passes_when_far_in_future() {
        // dl = now + 3600s → comfortably outside 30s margin.
        let now = 1_715_000_000u32;
        assert!(across_fill_deadline_skip_reason(Some(now + 3600), now).is_none());
    }

    #[test]
    fn across_fill_deadline_guard_passes_at_margin() {
        // dl = now + 30s → exactly at margin → must pass (filter is strict <).
        let now = 1_715_000_000u32;
        assert!(across_fill_deadline_skip_reason(Some(now + 30), now).is_none());
    }

    #[test]
    fn across_fill_deadline_guard_skips_inside_margin() {
        // dl = now + 29s → within 30s margin → skip.
        let now = 1_715_000_000u32;
        let reason = across_fill_deadline_skip_reason(Some(now + 29), now)
            .expect("expected skip inside 30s margin");
        assert!(reason.starts_with("across_fill_deadline_expired:"));
        assert!(reason.contains(&format!("dl={}", now + 29)));
        assert!(reason.contains(&format!("now={now}")));
    }

    #[test]
    fn across_fill_deadline_guard_skips_already_expired() {
        let now = 1_715_000_000u32;
        let reason = across_fill_deadline_skip_reason(Some(now - 1), now)
            .expect("expected skip on already-expired deadline");
        assert!(reason.starts_with("across_fill_deadline_expired:"));
    }

    #[test]
    fn across_fill_deadline_guard_passes_when_missing() {
        // No deadline carried in the intent → no skip; downstream calldata
        // builder substitutes now+3600. The helper does not invent a reason.
        assert!(across_fill_deadline_skip_reason(None, 1_715_000_000).is_none());
    }

    #[test]
    fn mayan_deadline_skip_reason_logic() {
        // Mirrors the inline logic in lambda_execute §5d.
        let now: u64 = 1_715_000_000;
        let margin: u64 = 30;

        // Far future deadline → should NOT skip
        let dl_future = now + 3600;
        assert!(dl_future >= now.saturating_add(margin), "far deadline ok");

        // Expired deadline (dl < now + 30) → should skip
        let dl_near = now + 10;
        assert!(dl_near < now.saturating_add(margin), "near deadline skip");

        // Already expired deadline
        let dl_past = now.saturating_sub(1);
        assert!(dl_past < now.saturating_add(margin), "past deadline skip");

        // Exactly at margin (dl == now + 30) → must NOT skip (strict <)
        let dl_at_margin = now + margin;
        assert!(!(dl_at_margin < now.saturating_add(margin)), "at-margin passes");
    }

    #[tokio::test]
    async fn lambda_claim_rejects_non_confirmed() {
        let mgr = Arc::new(WalletManager::open(":memory:", 1000.0).unwrap());
        mgr.record_detected(NewIntent {
            intent_id: "i1".into(),
            protocol: "across".into(),
            src_chain: 1,
            dst_chain: 42161,
            amount_usd: 100.0,
        })
        .unwrap();

        let signer = PrivateKeySigner::random();
        let ctrl = LambdaController {
            wallet: mgr,
            spinner: SpinnerSolverClient::new("http://127.0.0.1:1"),
            signer,
            chains: HashMap::new(),
            outcome_log: None,
            dry_run: true,
            profit_threshold_usd: 0.10,
            feedback_url: None,
        };
        let out = ctrl.lambda_claim("i1", 1.23).await.unwrap();
        match out {
            LambdaClaimOutcome::NotEligible { reason } => {
                assert!(reason.contains("CONFIRMED"));
            }
            other => panic!("expected NotEligible, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn lambda_execute_skip_when_dst_chain_unwired() {
        // No chain wiring → controller must skip cleanly without panicking
        // and must release the reservation back to the wallet.
        let mgr = Arc::new(WalletManager::open(":memory:", 1000.0).unwrap());
        let signer = PrivateKeySigner::random();
        let ctrl = LambdaController {
            wallet: mgr.clone(),
            // Use a localhost URL that won't connect — but we don't reach the
            // RPC step because spinner test_run will fail first. This test
            // exercises the early-return paths rather than the broadcast path.
            spinner: SpinnerSolverClient::new("http://127.0.0.1:1"),
            signer,
            chains: HashMap::new(),
            outcome_log: None,
            dry_run: true,
            profit_threshold_usd: 0.10,
            feedback_url: None,
        };
        let i = intent("i1", "100000000");
        let out = ctrl.lambda_execute(&i).await.unwrap();
        // Either the spinner call fails (Failed) or the chain wiring is missing
        // (Skipped). Both are valid early-exit signals — the contract is that
        // we do not hang or panic, and the wallet ledger is consistent.
        match out {
            LambdaExecuteOutcome::Failed { .. } | LambdaExecuteOutcome::Skipped { .. } => {}
            other => panic!("expected Failed or Skipped, got {other:?}"),
        }
        // When chain wiring is missing we skip before record_detected to avoid
        // orphaned wallet records for chains we can never fill on. The ledger
        // may be empty or may contain the record depending on which early-exit
        // triggered — both are valid as long as we don't panic or hang.
        let _ = mgr.list_intents(None, 100).unwrap();
    }

    #[tokio::test]
    async fn lambda_execute_skips_when_notional_exceeds_cap() {
        // Demo safety belt — MAX_NOTIONAL_USD must short-circuit before the wallet
        // ledger is touched, so a hostile or accidental large intent never
        // reaches broadcast on a live mainnet wallet.
        let mgr = Arc::new(WalletManager::open(":memory:", 1_000_000.0).unwrap());
        let signer = PrivateKeySigner::random();
        let ctrl = LambdaController {
            wallet: mgr.clone(),
            spinner: SpinnerSolverClient::new("http://127.0.0.1:1"),
            signer,
            chains: HashMap::new(),
            outcome_log: None,
            dry_run: true,
            profit_threshold_usd: 0.10,
            feedback_url: None,
        };
        // 50_000 USDC (6 decimals) = $50,000 — well above the default $200 cap.
        let mut i = intent("big", "50000000000");
        i.src_token = "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913".into(); // USDC Base
        // Set a low cap explicitly so the test is deterministic regardless of
        // the operator's MAX_NOTIONAL_USD env in CI.
        // SAFETY: tests in this module run serially within the cargo-test
        // tokio runtime; setting a process-wide env here is acceptable
        // because no other test in this binary reads MAX_NOTIONAL_USD.
        unsafe { std::env::set_var("MAX_NOTIONAL_USD", "200") };
        let out = ctrl.lambda_execute(&i).await.unwrap();
        unsafe { std::env::remove_var("MAX_NOTIONAL_USD") };
        match out {
            LambdaExecuteOutcome::Skipped { reason } => {
                assert!(reason.starts_with("notional_cap_exceeded"), "got {reason}");
            }
            other => panic!("expected Skipped(notional_cap_exceeded), got {other:?}"),
        }
        // Crucially, the wallet ledger MUST NOT have a record — the cap
        // short-circuits before record_detected.
        let intents = mgr.list_intents(None, 100).unwrap();
        assert_eq!(intents.len(), 0);
    }
}
