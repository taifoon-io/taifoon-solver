//! Live genome SSE fixture decoder.
//!
//! Reads a captured SSE dump (one `data: { ... }` per line, interleaved with
//! `event: genome_entry`) and reports how many events successfully round-trip
//! through `GenomeEvent::from_json_str` -> `Intent::from_genome_event`.
//!
//! Usage:
//!     cargo run -p genome-client --bin decode_live -- <path-to-dump>

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::process::ExitCode;

use genome_client::{GenomeEvent, Intent};
use serde_json::Value;

fn is_intent_candidate(v: &Value) -> bool {
    let entity = v.get("entity").and_then(|e| e.as_str()).unwrap_or("");
    let action = v.get("action").and_then(|a| a.as_str()).unwrap_or("");
    matches!(entity, "proto" | "order")
        && matches!(action, "deposit" | "placed" | "executed")
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let path = match args.get(1) {
        Some(p) => p.clone(),
        None => {
            eprintln!("usage: decode_live <fixture-path>");
            return ExitCode::from(2);
        }
    };

    let raw = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("failed to read {}: {}", path, e);
            return ExitCode::from(2);
        }
    };

    let mut total_data_lines = 0usize;
    let mut json_parse_failures = 0usize;
    let mut non_intent_events = 0usize;
    let mut candidate_events = 0usize;
    let mut decoded_intents: Vec<Intent> = Vec::new();
    let mut decode_failures: Vec<(String, String)> = Vec::new();
    let mut entity_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut action_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut protocol_counts: BTreeMap<String, usize> = BTreeMap::new();

    let mut across_no_deposit_id: Vec<String> = Vec::new();

    for line in raw.lines() {
        let payload = match line.strip_prefix("data: ").or_else(|| line.strip_prefix("data:")) {
            Some(p) => p.trim(),
            None => continue,
        };
        if payload.is_empty() {
            continue;
        }
        total_data_lines += 1;

        let v: Value = match serde_json::from_str(payload) {
            Ok(v) => v,
            Err(e) => {
                json_parse_failures += 1;
                if decode_failures.len() < 5 {
                    decode_failures
                        .push((format!("json: {}", e), payload.chars().take(200).collect()));
                }
                continue;
            }
        };

        if let Some(ent) = v.get("entity").and_then(|e| e.as_str()) {
            *entity_counts.entry(ent.to_string()).or_default() += 1;
        }
        if let Some(act) = v.get("action").and_then(|a| a.as_str()) {
            *action_counts.entry(act.to_string()).or_default() += 1;
        }

        if !is_intent_candidate(&v) {
            non_intent_events += 1;
            continue;
        }
        candidate_events += 1;

        let event = match GenomeEvent::from_json_value(v.clone()) {
            Ok(e) => e,
            Err(e) => {
                decode_failures.push((
                    format!("GenomeEvent::from_json_value: {:#}", e),
                    payload.chars().take(200).collect(),
                ));
                continue;
            }
        };

        let proto = event
            .protocol
            .clone()
            .or_else(|| event.id.clone())
            .unwrap_or_else(|| "unknown".to_string());

        match Intent::from_genome_event(event.clone()) {
            Ok(intent) => {
                *protocol_counts.entry(intent.protocol.clone()).or_default() += 1;

                let proto_lower = intent.protocol.to_lowercase();
                if (proto_lower == "across" || proto_lower.starts_with("across"))
                    && intent.deposit_id.is_none()
                {
                    across_no_deposit_id.push(payload.to_string());
                }

                decoded_intents.push(intent);
            }
            Err(e) => {
                decode_failures.push((
                    format!("Intent::from_genome_event[{}]: {:#}", proto, e),
                    payload.chars().take(200).collect(),
                ));
            }
        }
    }

    let intent_success = decoded_intents.len();
    let intent_fail = decode_failures
        .iter()
        .filter(|(msg, _)| !msg.starts_with("json: "))
        .count();

    println!("==================== decode_live report ====================");
    println!("fixture: {}", path);
    println!("total `data:` lines:        {}", total_data_lines);
    println!("  json parse failures:      {}", json_parse_failures);
    println!("  non-intent events:        {}", non_intent_events);
    println!("  intent-candidate events:  {}", candidate_events);
    println!("    decoded successfully:   {}", intent_success);
    println!("    decode failures:        {}", intent_fail);
    if candidate_events > 0 {
        let pct = (intent_success as f64 / candidate_events as f64) * 100.0;
        println!("    decode success rate:    {:.1}%", pct);
    }
    println!();

    println!("entity histogram:");
    for (k, v) in &entity_counts {
        println!("  {:<10} {}", k, v);
    }
    println!();

    println!("action histogram:");
    for (k, v) in &action_counts {
        println!("  {:<10} {}", k, v);
    }
    println!();

    println!("protocol histogram (decoded intents):");
    for (k, v) in &protocol_counts {
        println!("  {:<20} {}", k, v);
    }
    println!();

    if !decode_failures.is_empty() {
        println!("first {} decode failures:", decode_failures.len().min(5));
        for (msg, sample) in decode_failures.iter().take(5) {
            println!("  - {}", msg);
            println!("    raw: {}", sample);
        }
        println!();
    }

    println!("Across intents with deposit_id=None: {}", across_no_deposit_id.len());
    for (i, raw) in across_no_deposit_id.iter().take(3).enumerate() {
        println!("  [{}] {}", i, raw.chars().take(400).collect::<String>());
    }
    println!();

    if let Some(sample) = decoded_intents.first() {
        println!("sample decoded Intent (first):");
        match serde_json::to_string_pretty(sample) {
            Ok(s) => println!("{}", s),
            Err(e) => println!("(serialize failed: {})", e),
        }
    } else {
        println!("no Intents decoded.");
    }

    ExitCode::SUCCESS
}
