//! Genome fixture parse test.
//!
//! Loads each fixture under `taifoon-solver/tests/fixtures/<protocol>.json` and
//! verifies it round-trips through a mirror of `spinner::da-api::genome_encoder::GenomeEntry`.
//! The mirror exists because pulling da-api as a dev-dependency would drag in rocksdb
//! (~500 MB build). When da-api's GenomeEntry schema changes, update the mirror below.
//!
//! Source of truth:
//!   spinner/rust/crates/da-api/src/genome_encoder.rs

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct V5Anchor {
    #[allow(dead_code)]
    superroot_hash: String,
    #[allow(dead_code)]
    chain_index: u32,
    #[allow(dead_code)]
    twig_index: u32,
    #[allow(dead_code)]
    block_index: u16,
}

#[derive(Debug, Deserialize)]
struct ProtoPayload {
    #[allow(dead_code)]
    protocol: String,
    #[serde(default)]
    #[allow(dead_code)]
    event_topic: Option<String>,
    src_chain: u64,
    #[serde(default)]
    dst_chain: Option<u64>,
    #[serde(default)]
    depositor: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    recipient: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    token: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    amount: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    tx_hash: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    log_index: Option<u32>,
    #[serde(default)]
    #[allow(dead_code)]
    contract: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum GenomePayload {
    Proto(ProtoPayload),
    Other(serde_json::Value),
}

#[derive(Debug, Deserialize)]
struct GenomeEntry {
    #[allow(dead_code)]
    addr: String,
    #[allow(dead_code)]
    ts: u64,
    #[allow(dead_code)]
    batch_id: u64,
    #[allow(dead_code)]
    chain_id: u64,
    entity: String,
    action: String,
    #[serde(default)]
    #[allow(dead_code)]
    id: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    ref_hash: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    v5_anchor: Option<V5Anchor>,
    #[serde(flatten)]
    payload: GenomePayload,
}

fn fixture_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("../..").join("tests/fixtures")
}

fn load(name: &str) -> serde_json::Value {
    let path = fixture_dir().join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    serde_json::from_str::<serde_json::Value>(&text)
        .unwrap_or_else(|e| panic!("parse json {}: {}", path.display(), e))
}

fn parse_entry(name: &str) -> GenomeEntry {
    let raw = load(name);
    serde_json::from_value(raw)
        .unwrap_or_else(|e| panic!("GenomeEntry deserialize {}: {}", name, e))
}

fn assert_field_present(name: &str, field: &str) {
    let v = load(name);
    let obj = v.as_object().unwrap_or_else(|| panic!("{} not object", name));
    assert!(
        obj.contains_key(field),
        "fixture {} missing required field '{}'",
        name,
        field
    );
    assert!(
        !obj.get(field).map(|v| v.is_null()).unwrap_or(true),
        "fixture {} has null '{}'",
        name,
        field
    );
}

#[test]
fn across_fixture_parses() {
    let entry = parse_entry("across.json");
    assert_eq!(entry.entity, "proto");
    assert_eq!(entry.action, "deposit");
    match entry.payload {
        GenomePayload::Proto(p) => {
            assert_eq!(p.src_chain, 1);
            assert_eq!(p.dst_chain, Some(42161));
            assert!(p.depositor.is_some());
        }
        GenomePayload::Other(_) => panic!("expected ProtoPayload"),
    }
    for f in ["deposit_id", "input_amount", "output_amount", "src_token", "dst_token"] {
        assert_field_present("across.json", f);
    }
}

#[test]
fn debridge_fixture_parses() {
    let entry = parse_entry("debridge.json");
    assert_eq!(entry.entity, "proto");
    match entry.payload {
        GenomePayload::Proto(p) => {
            assert_eq!(p.src_chain, 56);
            assert_eq!(p.dst_chain, Some(10));
        }
        GenomePayload::Other(_) => panic!("expected ProtoPayload"),
    }
    for f in [
        "maker_order_nonce",
        "give_token_address",
        "give_amount",
        "take_token_address",
        "take_amount",
        "order_id",
    ] {
        assert_field_present("debridge.json", f);
    }
}

#[test]
fn mayan_evm_fixture_parses() {
    let entry = parse_entry("mayan_evm.json");
    assert_eq!(entry.entity, "proto");
    match entry.payload {
        GenomePayload::Proto(p) => {
            assert_eq!(p.src_chain, 137);
            assert_eq!(p.dst_chain, Some(8453));
        }
        GenomePayload::Other(_) => panic!("expected ProtoPayload"),
    }
    for f in [
        "mayan_order_id",
        "min_amount_out",
        "trader",
        "deadline",
        "swift_dest_chain_wormhole_id",
    ] {
        assert_field_present("mayan_evm.json", f);
    }
}

#[test]
fn mayan_solana_fixture_parses() {
    let entry = parse_entry("mayan_solana.json");
    assert_eq!(entry.entity, "proto");
    match entry.payload {
        GenomePayload::Proto(p) => {
            assert_eq!(p.src_chain, 1399811149);
            assert_eq!(p.dst_chain, Some(1));
        }
        GenomePayload::Other(_) => panic!("expected ProtoPayload"),
    }
    for f in [
        "mayan_order_id",
        "swift_program_id",
        "state_account",
        "vault_account",
        "compute_units_estimate",
        "is_solana_source",
    ] {
        assert_field_present("mayan_solana.json", f);
    }
}

#[test]
fn lifi_fixture_parses() {
    let entry = parse_entry("lifi.json");
    assert_eq!(entry.entity, "proto");
    match entry.payload {
        GenomePayload::Proto(p) => {
            assert_eq!(p.src_chain, 1);
            assert_eq!(p.dst_chain, Some(42161));
        }
        GenomePayload::Other(_) => panic!("expected ProtoPayload"),
    }
    for f in [
        "lifi_quote_id",
        "bridge",
        "tool",
        "lifi_transaction_id",
        "min_amount_out",
    ] {
        assert_field_present("lifi.json", f);
    }
}

#[test]
fn meta_files_present_for_each_fixture() {
    for proto in ["across", "debridge", "mayan_evm", "mayan_solana", "lifi"] {
        let meta = fixture_dir().join(format!("{}.meta.json", proto));
        assert!(meta.exists(), "missing meta file: {}", meta.display());
        let text = std::fs::read_to_string(&meta).expect("read meta");
        let v: serde_json::Value = serde_json::from_str(&text).expect("parse meta");
        assert_eq!(
            v.get("source").and_then(|s| s.as_str()),
            Some("synthetic"),
            "meta {} must declare source=synthetic",
            proto
        );
    }
}
