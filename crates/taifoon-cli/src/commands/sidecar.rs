//! `taifoon sidecar` — full fill lifecycle manager.
//!
//! Each cycle (default 300s):
//!   1. **Claim retry** — scans wallet DB for deBridge fills in CONFIRMED state
//!      (fill tx confirmed but claimUnlock never sent or failed) and fires
//!      claimUnlock on the src chain to release the maker's locked funds.
//!   2. **Rebalance** — scans balances, classifies fill chains, bridges surplus
//!      to fund depleted chains and sweeps src-chain recoveries to Base.
//!
//! The solver fill loop (solver-main) runs as a separate process alongside this.
//! deBridge: inline claimUnlock fires immediately after each confirmed fill in
//! solver-main. This sidecar catches any that were missed (network error, gas
//! spike, process restart) by checking the wallet DB every cycle.

use anyhow::Result;
use executor::{build_lambda_controller_from_env, LambdaClaimOutcome, LambdaController};
use genome_client::Intent;
use portfolio_sidecar::{inventory::InventoryStatus, PortfolioSidecar};
use std::sync::Arc;
use tracing::{info, warn};
use wallet_manager::WalletManager;

pub struct SidecarArgs {
    pub private_key: String,
    pub dry_run: bool,
    pub interval_secs: u64,
    pub json_mode: bool,
    pub max_cycles: Option<u64>,
    /// Retry deBridge claimUnlock for CONFIRMED fills each cycle.
    pub claim_retry: bool,
    /// Path to the wallet DB written by solver-main (for claim retry).
    pub wallet_db_path: String,
    /// Path to the outcome DB (for LambdaController construction).
    pub outcome_db_path: String,
}

pub async fn run(args: SidecarArgs) -> Result<()> {
    let sidecar = PortfolioSidecar::from_key(&args.private_key, args.dry_run)
        .map_err(|e| anyhow::anyhow!("invalid SOLVER_PRIVATE_KEY: {e}"))?;

    // Build claim controller if retry is enabled.
    // We set SOLVER_PRIVATE_KEY in env so build_lambda_controller_from_env can read it.
    let claim_ctrl: Option<Arc<LambdaController>> = if args.claim_retry {
        // Temporarily expose the key via env so the shared constructor works.
        // This is a CLI process — no concurrency concern here.
        std::env::set_var("SOLVER_PRIVATE_KEY", &args.private_key);
        let wallet = Arc::new(
            WalletManager::open(&args.wallet_db_path, 0.0)
                .map_err(|e| anyhow::anyhow!("wallet DB open: {e}"))?,
        );
        let spinner = std::env::var("SPINNER_API_URL")
            .unwrap_or_else(|_| "https://api.taifoon.dev".into());
        let mamba = std::env::var("MAMBA_LAKE_URL").ok();
        match build_lambda_controller_from_env(&spinner, &args.outcome_db_path, mamba, args.dry_run, 0.0, wallet) {
            Ok(Some(ctrl)) => {
                info!("🔁 Claim retry enabled — will sweep CONFIRMED deBridge fills each cycle");
                Some(Arc::new(ctrl))
            }
            Ok(None) => {
                warn!("claim_retry enabled but SOLVER_PRIVATE_KEY missing — retries disabled");
                None
            }
            Err(e) => {
                warn!("claim_retry controller init failed: {e} — retries disabled");
                None
            }
        }
    } else {
        None
    };

    if !args.json_mode {
        println!("\nPortfolio sidecar starting");
        println!("  dry_run:      {}", args.dry_run);
        println!("  interval:     {}s", args.interval_secs);
        println!("  claim_retry:  {}", args.claim_retry);
        println!();
    }
    let mut cycle = 0u64;

    loop {
        cycle += 1;

        // ── Phase 1: claim retry ────────────────────────────────────────────
        if let Some(ref ctrl) = claim_ctrl {
            claim_retry_tick(&args.wallet_db_path, ctrl, args.dry_run).await;
        }

        // ── Phase 2: rebalance ──────────────────────────────────────────────
        let actions = sidecar.tick().await;

        // ── Output ──────────────────────────────────────────────────────────
        if args.json_mode {
            let state = sidecar.state.read().await;
            println!("{}", serde_json::to_string_pretty(&*state).unwrap_or_default());
        } else {
            let state = sidecar.state.read().await;
            println!("\n── Cycle #{} ─────────────────────────────────────────", cycle);
            println!("{:<14} {:<12} {:<12} {}", "Chain", "Stables", "Gas ETH", "Status");
            println!("{}", "─".repeat(54));
            for c in &state.classified {
                let icon = match c.status {
                    InventoryStatus::Healthy  => "✅ HEALTHY",
                    InventoryStatus::Surplus  => "💰 SURPLUS",
                    InventoryStatus::LowGas   => "⛽ LOW_GAS",
                    InventoryStatus::LowFunds => "💸 LOW_FUNDS",
                    InventoryStatus::Critical => "🚨 CRITICAL",
                    InventoryStatus::SrcOnly  => "   src-only",
                };
                println!(
                    "{:<14} {:<12} {:<12} {}",
                    c.chain_name,
                    format!("${:.2}", c.stable_usd),
                    format!("{:.5}", c.gas_eth),
                    icon,
                );
            }
            if actions.is_empty() {
                println!("\n  All fill chains healthy — no bridges needed.");
            } else {
                println!("\n  {} bridge action(s):", actions.len());
                for a in &actions {
                    let kind = format!("{:?}", a.kind);
                    let hash = a.tx_hash.as_deref().unwrap_or("-");
                    println!(
                        "    chain {} → {} ${:.2} {} [{}] tx={}",
                        a.src_chain, a.dst_chain, a.amount_usd, a.token_symbol, kind, hash
                    );
                }
            }
        }

        if let Some(max) = args.max_cycles {
            if cycle >= max {
                break;
            }
        }

        if !args.json_mode {
            println!("\n  Sleeping {}s...", args.interval_secs);
        }
        tokio::time::sleep(std::time::Duration::from_secs(args.interval_secs)).await;
    }

    Ok(())
}

/// Scan wallet DB for deBridge fills that are CONFIRMED but not yet claimed,
/// and fire claimUnlock on the src chain for each.
async fn claim_retry_tick(wallet_db_path: &str, ctrl: &LambdaController, dry_run: bool) {
    let wallet = match WalletManager::open(wallet_db_path, 0.0) {
        Ok(w) => w,
        Err(e) => {
            warn!("claim_retry: cannot open wallet DB at {}: {}", wallet_db_path, e);
            return;
        }
    };

    let confirmed = match wallet.list_intents(Some("CONFIRMED"), 200) {
        Ok(v) => v,
        Err(e) => {
            warn!("claim_retry: list_intents failed: {}", e);
            return;
        }
    };

    let pending: Vec<_> = confirmed
        .iter()
        .filter(|r| {
            let p = r.protocol.to_lowercase();
            p.contains("debridge") || p.contains("dln")
        })
        .collect();

    if pending.is_empty() {
        info!("claim_retry: no unclaimed deBridge fills");
        return;
    }

    info!("claim_retry: {} unclaimed deBridge fill(s) — firing claimUnlock", pending.len());

    for record in pending {
        // intent.order_id drives claimUnlock calldata. DeBridgePoller sets
        // intent.id = "debridge_dln:0x<orderId>" and intent.order_id = Some("0x<orderId>").
        // The wallet DB stores the full intent.id in intent_id. Extract the hex part.
        let order_id_hex = if record.intent_id.contains(':') {
            record.intent_id.splitn(2, ':').nth(1).unwrap_or(&record.intent_id).to_string()
        } else {
            record.intent_id.clone()
        };

        let intent = Intent {
            id: record.intent_id.clone(),
            protocol: record.protocol.clone(),
            src_chain: record.src_chain as u64,
            dst_chain: record.dst_chain as u64,
            order_id: Some(order_id_hex.clone()),
            ..Intent::default()
        };

        if dry_run {
            info!(
                "claim_retry: [DRY_RUN] would claimUnlock orderId={} src_chain={}",
                order_id_hex, record.src_chain
            );
            continue;
        }

        match ctrl.lambda_claim_debridge(&intent).await {
            Ok(LambdaClaimOutcome::Claimed { tx_hash, fee_usd }) => {
                info!(
                    "claim_retry: ✅ claimUnlock confirmed tx={} fee=${:.4} ({})",
                    tx_hash, fee_usd, record.intent_id
                );
            }
            Ok(LambdaClaimOutcome::NotEligible { reason }) => {
                // Already claimed or claim pending from the inline path — benign.
                info!("claim_retry: not eligible for {} — {}", record.intent_id, reason);
            }
            Ok(LambdaClaimOutcome::Failed { error }) => {
                warn!("claim_retry: ❌ claimUnlock failed for {} — {}", record.intent_id, error);
                // Stays CONFIRMED; will retry next cycle.
            }
            Err(e) => {
                warn!("claim_retry: fatal for {} — {}", record.intent_id, e);
            }
        }
    }
}
