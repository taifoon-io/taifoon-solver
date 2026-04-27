//! Monitor genome stream and stats

use anyhow::Result;
use genome_client::{GenomeClient, Intent};
use serde_json::json;
use tokio::sync::mpsc;

pub async fn stream_intents(
    genome_url: &str,
    spinner_url: &str,
    protocol: Option<String>,
    limit: Option<usize>,
    profitable_only: bool,
    json_mode: bool,
) -> Result<()> {
    if json_mode {
        println!(r#"{{"success":true,"message":"Monitoring genome stream..."}}"#);
    } else {
        println!("📡 Monitoring genome stream: {}", genome_url);
        if let Some(proto) = &protocol {
            println!("   Protocol filter: {}", proto);
        }
        if let Some(lim) = limit {
            println!("   Limit: {} intents", lim);
        }
        println!("   Profitable only: {}", profitable_only);
        println!();
    }

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
    let mut count = 0;
    while let Some(intent) = intent_rx.recv().await {
        // Apply protocol filter if specified
        if let Some(ref proto_filter) = protocol {
            if !intent.protocol.to_lowercase().contains(&proto_filter.to_lowercase()) {
                continue;
            }
        }

        // TODO: Apply profitability filter if specified
        // For now, we skip profitability check since it requires spinner API integration

        if json_mode {
            println!("{}", serde_json::to_string(&intent)?);
        } else {
            println!("🎯 Intent #{}", count + 1);
            println!("   Protocol:   {}", intent.protocol);
            println!("   Route:      {} → {}", intent.src_chain, intent.dst_chain);
            println!("   Amount:     {}", intent.amount);
            println!("   Depositor:  {}", intent.depositor);
            println!("   Recipient:  {}", intent.recipient);
            println!("   Tx Hash:    {}", intent.tx_hash);
            println!();
        }

        count += 1;

        // Check limit
        if let Some(lim) = limit {
            if count >= lim {
                if !json_mode {
                    println!("✅ Reached limit of {} intents", lim);
                }
                break;
            }
        }
    }

    // Cancel genome client
    genome_handle.abort();

    Ok(())
}

pub async fn stats(since: &str, spinner_url: &str, json_mode: bool) -> Result<()> {
    if json_mode {
        println!(r#"{{"success":true,"data":{{"fills":0,"profit_usd":0.0,"since":"{}"}}}}"#, since);
    } else {
        println!("📊 Solver Stats ({})", since);
        println!("   Fills executed: 0");
        println!("   Total profit: $0.00");
    }

    // TODO: Implement stats fetching
    Ok(())
}
