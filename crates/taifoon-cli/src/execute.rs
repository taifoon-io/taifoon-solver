//! Execute fills and manage autonomous participation via the Lambda controller pipeline.
//!
//! All four protocols (Across V3, deBridge DLN, Mayan EVM, Mayan Solana) and LiFi
//! (projected to its underlying child protocol) flow through the same
//! `lambda_execute` lifecycle: reserve → estimate → calldata → estimateGas gate →
//! fee-aware broadcast → receipt → release.

use anyhow::{anyhow, Result};
use executor::{
    build_lambda_controller_from_env, LambdaClaimOutcome, LambdaExecuteOutcome, LiFiMetaRouter,
};
use genome_client::{AcrossPoller, DeBridgePoller, GenomeClient, Intent};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use wallet_manager::WalletManager;

const DEFAULT_WALLET_DB: &str = "/tmp/taifoon_cli_wallet.sqlite";
const DEFAULT_OUTCOME_DB: &str = "/tmp/taifoon_cli_outcomes.sqlite";
const DEFAULT_WALLET_BUDGET: f64 = 10_000.0;

pub async fn participate(
    spinner_url: &str,
    genome_url: &str,
    private_key: &str,
    _auto: bool,
    min_profit: f64,
    protocol: &str,
    dry_run: bool,
    _max_concurrent: usize,
    json_mode: bool,
) -> Result<()> {
    // Inject private key into env so build_lambda_controller_from_env can pick it up.
    // This avoids duplicating the signer-parse logic.
    std::env::set_var("SOLVER_PRIVATE_KEY", private_key);
    std::env::set_var("WARMBED_API_URL", spinner_url);
    std::env::set_var("DRY_RUN", if dry_run { "true" } else { "false" });

    let wallet_db = std::env::var("WALLET_DB_PATH").unwrap_or_else(|_| DEFAULT_WALLET_DB.into());
    let outcome_db = std::env::var("OUTCOME_DB_PATH").unwrap_or_else(|_| DEFAULT_OUTCOME_DB.into());
    let wallet_budget: f64 = std::env::var("WALLET_BUDGET_USD")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_WALLET_BUDGET);
    let mamba_url = std::env::var("MAMBA_LAKE_URL").ok();

    let wallet_manager = Arc::new(
        WalletManager::open(&wallet_db, wallet_budget)
            .map_err(|e| anyhow!("wallet-manager: {e}"))?,
    );

    let ctrl = match build_lambda_controller_from_env(
        spinner_url,
        &outcome_db,
        mamba_url,
        dry_run,
        min_profit,
        wallet_manager,
    )? {
        Some(c) => c,
        None => return Err(anyhow!("SOLVER_PRIVATE_KEY not set — cannot execute fills")),
    };

    let solver_addr = format!("{:?}", ctrl.signer.address());

    if json_mode {
        println!(
            r#"{{"success":true,"message":"Starting solver","address":"{}","dry_run":{}}}"#,
            solver_addr, dry_run
        );
    } else {
        println!("\n👑 TAIFOON SOLVER — UNIFIED LAMBDA PIPELINE");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("Solver:     {}", solver_addr);
        println!("Chains:     {} wired", ctrl.chains.len());
        println!("Protocol:   {}", protocol);
        println!("Min Profit: ${:.2}", min_profit);
        println!("Dry Run:    {}", dry_run);
        println!("Spinner:    {}", spinner_url);
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");
    }

    let protocol_filter = protocol.to_lowercase();

    let genome_client = GenomeClient::new(genome_url);
    let (intent_tx, mut intent_rx) = mpsc::channel::<Intent>(100);
    let across_poller = AcrossPoller::default_mainnet();
    let debridge_poller = DeBridgePoller::default_mainnet();
    let _genome_handle = tokio::spawn(async move {
        if let Err(e) = genome_client
            .subscribe_with_all_pollers(intent_tx, vec![across_poller], Some(debridge_poller))
            .await
        {
            error!("genome stream error: {}", e);
        }
    });

    if !json_mode {
        println!("📡 Monitoring genome stream...\n");
    }

    let mut dispatched: HashSet<String> = HashSet::new();

    while let Some(intent) = intent_rx.recv().await {
        let proto_lower = intent.protocol.to_lowercase();

        // Protocol filter
        if protocol_filter != "all" && !protocol_filter.split(',').any(|f| proto_lower.contains(f.trim())) {
            continue;
        }

        // LiFi projection
        let effective_intent;
        let intent_ref: &Intent = if proto_lower.contains("lifi") || proto_lower.contains("li.fi") {
            let bridge = LiFiMetaRouter::resolve_bridge(&intent).unwrap_or_default();
            if bridge.is_empty() {
                info!("⏭️  lifi skip (missing bridge/tool): {}", intent.id);
                continue;
            }
            effective_intent = LiFiMetaRouter::project_to_child(&intent, &bridge);
            info!("🔀 LiFi→{} projection: {}", bridge, intent.id);
            &effective_intent
        } else {
            &intent
        };

        let eff_proto = intent_ref.protocol.to_lowercase();
        let routable = eff_proto.contains("across")
            || eff_proto.contains("debridge")
            || eff_proto.contains("dln")
            || eff_proto.contains("mayan");

        if !routable {
            info!("⏭️  unroutable protocol: {}", intent_ref.protocol);
            continue;
        }

        // Zero-amount guard
        if intent_ref.amount == "0"
            && intent_ref.output_amount.as_deref().map(|s| s == "0" || s.is_empty()).unwrap_or(true)
        {
            info!("⏭️  skip zero-amount: {}", intent_ref.id);
            continue;
        }

        // Dedup
        let dedup_key = if let Some(dep_id) = intent_ref.deposit_id {
            format!("{}:dep:{}", intent_ref.protocol, dep_id)
        } else {
            intent_ref.id.clone()
        };
        if !dispatched.insert(dedup_key.clone()) {
            continue;
        }

        info!("📥 {} ({}) {}→{} amt={}",
            intent_ref.id, intent_ref.protocol, intent_ref.src_chain, intent_ref.dst_chain, intent_ref.amount);

        match ctrl.lambda_execute(intent_ref).await {
            Ok(LambdaExecuteOutcome::Confirmed { tx_hash, gas_used }) => {
                if json_mode {
                    println!(
                        r#"{{"action":"confirmed","intent_id":"{}","tx_hash":"{}","gas_used":{}}}"#,
                        intent_ref.id, tx_hash, gas_used
                    );
                } else {
                    println!("🎉 CONFIRMED: {} — tx {}", intent_ref.id, tx_hash);
                }
                // deBridge follow-up claim
                if eff_proto.contains("debridge") || eff_proto.contains("dln") {
                    match ctrl.lambda_claim_debridge(intent_ref).await {
                        Ok(LambdaClaimOutcome::Claimed { tx_hash: claim_tx, fee_usd }) => {
                            info!("💰 deBridge claimUnlock: {} (fee ~${:.4})", claim_tx, fee_usd);
                        }
                        Ok(LambdaClaimOutcome::NotEligible { reason }) => {
                            warn!("⚠️  deBridge claim not eligible: {}", reason);
                        }
                        Ok(LambdaClaimOutcome::Failed { error: e }) => {
                            error!("❌ deBridge claimUnlock failed: {}", e);
                        }
                        Err(e) => error!("❌ deBridge claim fatal: {}", e),
                    }
                }
            }
            Ok(LambdaExecuteOutcome::Skipped { reason }) => {
                if json_mode {
                    println!(
                        r#"{{"action":"skipped","intent_id":"{}","reason":"{}"}}"#,
                        intent_ref.id, reason
                    );
                } else {
                    info!("⏭️  skipped {}: {}", intent_ref.id, reason);
                }
            }
            Ok(LambdaExecuteOutcome::Reverted { tx_hash, error: e }) => {
                error!("❌ reverted (tx {}): {}", tx_hash, e);
            }
            Ok(LambdaExecuteOutcome::Failed { stage, error: e }) => {
                error!("❌ failed at {}: {}", stage, e);
            }
            Err(e) => error!("❌ lambda_execute fatal: {}", e),
        }
    }

    Ok(())
}

pub async fn single_fill(
    _spinner_url: &str,
    intent_id: &str,
    _private_key: &str,
    _dry_run: bool,
    json_mode: bool,
) -> Result<()> {
    if json_mode {
        println!(r#"{{"success":false,"message":"single_fill not yet implemented","intent_id":"{}"}}"#, intent_id);
    } else {
        println!("⚡ single_fill for {} — not yet implemented", intent_id);
    }
    Ok(())
}
