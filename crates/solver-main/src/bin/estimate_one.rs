//! `estimate_one <protocol-slug> <fixture.json>`
//!
//! Single-shot smoke test for the estimate pipeline. Loads a recorded genome
//! event JSON fixture, dispatches it to the matching `EstimateAdapter`, and
//! prints the outcome plus the V5 attempt-bundle id (when spinner is reachable).
//!
//! The MESSIAH wallet is read from the macOS keychain (entry
//! `mamba-messiah-key`); the key string itself is consumed inside
//! `solver_main::messiah::load_messiah_signer` and never logged.
//!
//! Usage:
//!   cargo run -p solver-main --bin estimate_one -- across tests/fixtures/across.json
//!
//! Env:
//!   SPINNER_API_URL       — base url for V5 attempt-bundle write (default: https://api.taifoon.dev)
//!   ETH_RPC_URL           — chain-1 RPC override
//!   RPC_URL_<chain_id>    — per-chain RPC override (falls back to a public default)

use anyhow::{anyhow, Context, Result};
use executor::{
    load_messiah_solana_pubkey_or_fallback, AcrossEstimateAdapter, DeBridgeEstimateAdapter,
    EstimateAdapter, EstimateOutcome, LiFiMetaRouter, MayanEvmEstimateAdapter,
    MayanSolanaEstimateAdapter, DEFAULT_SOLANA_RPC,
};
use genome_client::{GenomeEvent, Intent};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: estimate_one <protocol-slug> <fixture.json>");
        eprintln!("  protocol-slug ∈ {{across, debridge, mayan_evm, mayan_solana, lifi}}");
        std::process::exit(2);
    }
    let proto = args[1].to_lowercase();
    let path = &args[2];

    let spinner_base = std::env::var("SPINNER_API_URL")
        .unwrap_or_else(|_| "https://api.taifoon.dev".to_string());

    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("read fixture {}", path))?;

    // Tolerant of fixtures that carry both legacy (`token`, `amount`, ...) and
    // canonical (`src_token`, `input_amount`, ...) keys.
    let event = GenomeEvent::from_json_str(&raw)
        .with_context(|| format!("parse {} as GenomeEvent", path))?;
    let intent = Intent::from_genome_event(event)
        .context("build Intent from genome event")?;

    let messiah = solver_main::messiah::load_messiah_address()
        .context("load MESSIAH address from keychain")?;
    println!("MESSIAH address: {:#x}", messiah);
    println!("Intent: id={} protocol={} {}→{} amount={}",
             intent.id, intent.protocol, intent.src_chain, intent.dst_chain, intent.amount);

    let outcome: EstimateOutcome = match proto.as_str() {
        "across" => {
            let adapter = AcrossEstimateAdapter::new(messiah, &spinner_base);
            adapter.estimate(&intent).await
        }
        "debridge" => {
            let adapter = DeBridgeEstimateAdapter::new(messiah, &spinner_base);
            adapter.estimate(&intent).await
        }
        "mayan_evm" | "mayan" | "mayan_swift" => {
            let adapter = MayanEvmEstimateAdapter::new(messiah, &spinner_base);
            adapter.estimate(&intent).await
        }
        "lifi" => {
            let adapter = LiFiMetaRouter::new(messiah, &spinner_base);
            adapter.estimate(&intent).await
        }
        "mayan_solana" | "mayan-solana" | "mayan_swift_solana" => {
            // Solana payer pubkey: read from keychain entry
            // `mamba-messiah-solana-key` (public key only) if present, fall
            // back to the system program for the calldata-only path.
            let payer = load_messiah_solana_pubkey_or_fallback();
            let rpc = std::env::var("SOLANA_RPC_URL")
                .unwrap_or_else(|_| DEFAULT_SOLANA_RPC.to_string());
            println!("Solana payer pubkey: {}", payer);
            println!("Solana RPC: {}", rpc);
            let adapter = MayanSolanaEstimateAdapter::new(messiah, payer, rpc, &spinner_base);
            adapter.estimate(&intent).await
        }
        other => {
            return Err(anyhow!("unknown protocol '{}'", other));
        }
    };

    let green = if outcome.is_green() { "GREEN" } else { "RED" };
    println!("Outcome: {} [{}]", outcome.tag(), green);
    match &outcome {
        EstimateOutcome::OkGas(g) => println!("  gas: {}", g),
        EstimateOutcome::OkComputeUnits(g) => println!("  compute_units: {}", g),
        EstimateOutcome::InsufficientFundsLike(s)
        | EstimateOutcome::InsufficientLamports(s)
        | EstimateOutcome::Reverted(s)
        | EstimateOutcome::AbiInvalid(s)
        | EstimateOutcome::RouteNotImplemented(s) => println!("  detail: {}", s),
    }

    if !outcome.is_green() {
        std::process::exit(1);
    }
    Ok(())
}
