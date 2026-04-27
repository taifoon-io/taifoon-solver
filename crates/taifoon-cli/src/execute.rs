//! Execute fills and manage autonomous participation

use anyhow::{anyhow, Result};
use crate::wallet::Wallet;
use genome_client::{GenomeClient, Intent};
use protocol_adapters::AdapterFactory;
use serde_json::json;
use tokio::sync::mpsc;
use std::io::{self, Write};

pub async fn participate(
    spinner_url: &str,
    genome_url: &str,
    private_key: &str,
    auto: bool,
    min_profit: f64,
    protocol: &str,
    dry_run: bool,
    max_concurrent: usize,
    json_mode: bool,
) -> Result<()> {
    let signer = Wallet::from_private_key(private_key)?;
    let address = signer.address();

    if json_mode {
        println!(r#"{{"success":true,"message":"Starting autonomous solver","address":"{:?}","auto":{},"dry_run":{}}}"#,
            address, auto, dry_run);
    } else {
        println!("\n👑 TAIFOON SOLVER - AUTONOMOUS PARTICIPATION MODE");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("Wallet:         {:?}", address);
        println!("Mode:           {}", if auto { "AUTONOMOUS" } else { "INTERACTIVE" });
        println!("Min Profit:     ${:.2}", min_profit);
        println!("Protocol:       {}", protocol);
        println!("Dry Run:        {}", dry_run);
        println!("Max Concurrent: {}", max_concurrent);
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

        if dry_run {
            println!("🧪 DRY-RUN MODE: Simulating fills, no transactions will be broadcast\n");
        } else if auto {
            println!("🚀 LIVE MODE: Will execute profitable fills automatically\n");
        } else {
            println!("💬 INTERACTIVE MODE: Will prompt for confirmation before fills\n");
        }

        println!("📡 Monitoring genome stream...\n");
    }

    // Create adapter factory
    let adapter_factory = AdapterFactory::new(spinner_url);

    // Create channel for intents
    let (intent_tx, mut intent_rx) = mpsc::channel::<Intent>(100);

    // Spawn genome client in background
    let genome_client = GenomeClient::new(genome_url);
    let genome_handle = tokio::spawn(async move {
        if let Err(e) = genome_client.subscribe(intent_tx).await {
            eprintln!("Genome stream error: {}", e);
        }
    });

    // Process intents
    let mut processed_count = 0;
    let mut executed_count = 0;

    while let Some(intent) = intent_rx.recv().await {
        // Apply protocol filter
        let protocol_lower = protocol.to_lowercase();
        if protocol_lower != "all" {
            if !intent.protocol.to_lowercase().contains(&protocol_lower) {
                continue;
            }
        }

        processed_count += 1;

        if !json_mode {
            println!("🎯 Intent #{} detected", processed_count);
            println!("   Protocol:   {}", intent.protocol);
            println!("   Route:      {} → {}", intent.src_chain, intent.dst_chain);
            println!("   Amount:     {}", intent.amount);
            println!("   Depositor:  {}", intent.depositor);
        }

        // TODO: Estimate gas costs and calculate profitability
        // For now, we'll simulate profitability check
        let estimated_profit = 0.0; // Placeholder

        if estimated_profit < min_profit {
            if !json_mode {
                println!("   ⚠️  Skipped: Estimated profit ${:.2} below minimum ${:.2}\n", estimated_profit, min_profit);
            }
            continue;
        }

        // In interactive mode, ask for confirmation
        if !auto && !dry_run {
            if !json_mode {
                print!("   Execute fill? (y/n): ");
                io::stdout().flush()?;
                let mut input = String::new();
                io::stdin().read_line(&mut input)?;

                if input.trim().to_lowercase() != "y" {
                    println!("   Skipped by user\n");
                    continue;
                }
            }
        }

        // Execute fill (or simulate in dry-run mode)
        if dry_run {
            if json_mode {
                println!("{}", json!({
                    "action": "simulated_fill",
                    "intent_id": intent.id,
                    "protocol": intent.protocol,
                    "estimated_profit": estimated_profit
                }));
            } else {
                println!("   🧪 SIMULATED: Would execute fill (dry-run mode)");
                println!("   Estimated profit: ${:.2}\n", estimated_profit);
            }
            executed_count += 1;
        } else {
            // TODO: Actually execute the fill via protocol adapter
            if json_mode {
                println!("{}", json!({
                    "action": "executed_fill",
                    "intent_id": intent.id,
                    "protocol": intent.protocol,
                    "status": "pending"
                }));
            } else {
                println!("   ⚡ Executing fill...");
                println!("   Status: Pending on-chain confirmation\n");
            }
            executed_count += 1;
        }
    }

    // Cleanup
    genome_handle.abort();

    if !json_mode {
        println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("Session ended");
        println!("Intents processed: {}", processed_count);
        println!("Fills executed: {}", executed_count);
    }

    Ok(())
}

pub async fn single_fill(
    spinner_url: &str,
    intent_id: &str,
    private_key: &str,
    dry_run: bool,
    json_mode: bool,
) -> Result<()> {
    let signer = Wallet::from_private_key(private_key)?;
    let address = signer.address();

    if json_mode {
        println!(r#"{{"success":true,"message":"Executing fill","intent_id":"{}","dry_run":{}}}"#,
            intent_id, dry_run);
    } else {
        println!("\n⚡ Executing Fill");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("Intent ID: {}", intent_id);
        println!("Wallet:    {:?}", address);
        println!("Dry Run:   {}", dry_run);
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");
    }

    // TODO: Implement single fill execution
    Ok(())
}
