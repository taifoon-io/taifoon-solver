//! One-shot runner: iterate `tests/fixtures/*_live.json`, decode each wrapped
//! genome event, dispatch through the matching `EstimateAdapter` (or LiFi
//! meta-router), classify the outcome, and print a per-protocol summary table.
//!
//! Live fixtures wrap events in `{ events: [...] }` (one or more events captured
//! from the live SSE stream). For each fixture this runner:
//!   1. parses each event through `GenomeEvent::from_json_value` (canonical
//!      key normalization),
//!   2. projects it to `Intent`,
//!   3. picks an adapter by the intent's `protocol` field,
//!   4. runs the adapter and records the outcome.
//!
//! Run:
//!     SPINNER_API_URL=http://127.0.0.1:30081 \
//!     ETH_RPC_URL=https://mainnet.base.org \
//!     MIN_PROFIT_USD=0.0 \
//!     cargo run -p executor --example run_live_fixtures

use alloy::primitives::address;
use executor::{
    AcrossEstimateAdapter, DeBridgeEstimateAdapter, EstimateAdapter, EstimateOutcome,
    LiFiMetaRouter, MayanEvmEstimateAdapter,
};
use genome_client::{GenomeEvent, Intent};
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("../..").join("tests/fixtures")
}

fn list_live_fixtures() -> Vec<PathBuf> {
    let dir = fixtures_dir();
    let rd = match std::fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(e) => {
            eprintln!("read_dir({:?}): {}", dir, e);
            return vec![];
        }
    };
    let mut out = vec![];
    for entry in rd.flatten() {
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) == Some("json")
            && p.file_name()
                .and_then(|s| s.to_str())
                .map(|n| n.ends_with("_live.json"))
                .unwrap_or(false)
        {
            out.push(p);
        }
    }
    out.sort();
    out
}

#[tokio::main]
async fn main() {
    let messiah = address!("742d35Cc6634C0532925a3b844Bc9e7595f0bEb1");
    let spinner = std::env::var("SPINNER_API_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:30081".to_string());

    let fixtures = list_live_fixtures();
    if fixtures.is_empty() {
        println!("(no *_live.json fixtures in tests/fixtures/)");
        return;
    }

    println!("# live-estimate-run\n");
    println!("messiah  = {:#x}", messiah);
    println!("spinner  = {}", spinner);
    println!("fixtures = {}\n", fixtures.len());

    let mut rows: Vec<(String, String, String, String)> = vec![]; // (file, protocol, tag, msg/gas)

    for path in fixtures {
        let raw = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                rows.push((
                    path.file_name().unwrap().to_string_lossy().into(),
                    "?".into(),
                    "read_error".into(),
                    e.to_string(),
                ));
                continue;
            }
        };
        let value: serde_json::Value = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(e) => {
                rows.push((
                    path.file_name().unwrap().to_string_lossy().into(),
                    "?".into(),
                    "json_error".into(),
                    e.to_string(),
                ));
                continue;
            }
        };

        // Live fixtures wrap events in {events: [...]}. Static fixtures are
        // a single event object — accept both.
        let events: Vec<serde_json::Value> = if let Some(arr) = value.get("events").and_then(|v| v.as_array()) {
            arr.clone()
        } else {
            vec![value]
        };

        let fname = path.file_name().unwrap().to_string_lossy().to_string();
        for (i, ev_val) in events.into_iter().enumerate() {
            let event = match GenomeEvent::from_json_value(ev_val) {
                Ok(e) => e,
                Err(e) => {
                    rows.push((
                        format!("{}#{}", fname, i),
                        "?".into(),
                        "event_decode_error".into(),
                        e.to_string(),
                    ));
                    continue;
                }
            };
            let intent = match Intent::from_genome_event(event) {
                Ok(i) => i,
                Err(e) => {
                    rows.push((
                        format!("{}#{}", fname, i),
                        "?".into(),
                        "intent_decode_error".into(),
                        e.to_string(),
                    ));
                    continue;
                }
            };

            let proto = intent.protocol.clone();
            let outcome = dispatch(&intent, messiah, &spinner).await;
            let (tag, msg) = describe(&outcome);
            rows.push((format!("{}#{}", fname, i), proto, tag, msg));
        }
    }

    // Print a Markdown-ish table to stdout.
    println!("| fixture | protocol | outcome_tag | gas_or_message (truncated) |");
    println!("|---|---|---|---|");
    for (fixture, proto, tag, msg) in &rows {
        let trimmed: String = msg.chars().take(140).collect();
        let safe = trimmed.replace('\n', " ").replace('|', "/");
        println!("| {} | {} | {} | {} |", fixture, proto, tag, safe);
    }

    // Summary counts.
    println!("\n## Counts\n");
    let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
    for (_, _, tag, _) in &rows {
        *counts.entry(tag.clone()).or_default() += 1;
    }
    for (tag, n) in &counts {
        println!("- {} : {}", tag, n);
    }

    // Print full Reverted / AbiInvalid messages for inspection.
    let critical: Vec<_> = rows
        .iter()
        .filter(|(_, _, t, _)| t == "reverted" || t == "abi_invalid")
        .collect();
    if !critical.is_empty() {
        println!("\n## Critical outcomes (full text)\n");
        for (fixture, proto, tag, msg) in critical {
            println!("- **{}** [{}] `{}`:\n  > {}\n", fixture, proto, tag, msg);
        }
    }
}

async fn dispatch(intent: &Intent, messiah: alloy::primitives::Address, spinner: &str) -> EstimateOutcome {
    match intent.protocol.as_str() {
        "across" | "across_v3" => {
            AcrossEstimateAdapter::new(messiah, spinner).estimate(intent).await
        }
        "debridge" | "dln" => {
            DeBridgeEstimateAdapter::new(messiah, spinner).estimate(intent).await
        }
        "mayan" | "mayan_swift" => {
            MayanEvmEstimateAdapter::new(messiah, spinner).estimate(intent).await
        }
        "lifi" | "lifi_v2" => {
            LiFiMetaRouter::new(messiah, spinner).estimate(intent).await
        }
        other => EstimateOutcome::RouteNotImplemented(format!("unsupported protocol: {}", other)),
    }
}

fn describe(o: &EstimateOutcome) -> (String, String) {
    let tag = o.tag().to_string();
    let msg = match o {
        EstimateOutcome::OkGas(g) | EstimateOutcome::OkComputeUnits(g) => g.to_string(),
        EstimateOutcome::InsufficientFundsLike(s)
        | EstimateOutcome::InsufficientLamports(s)
        | EstimateOutcome::Reverted(s)
        | EstimateOutcome::AbiInvalid(s)
        | EstimateOutcome::RouteNotImplemented(s) => s.clone(),
    };
    (tag, msg)
}
