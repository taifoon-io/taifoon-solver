//! Integration test: genome SSE intent → AdapterFactory quote path.
//!
//! Spins up a local axum SSE server that emits one Stargate V2 intent.
//! A GenomeClient subscribes, receives the intent, and AdapterFactory.build_fill_tx
//! is called to prove the quote path is reached.

use axum::{response::sse::{Event, Sse}, routing::get, Router};
use futures::stream;
use genome_client::{GenomeClient, Intent};
use protocol_adapters::{AdapterFactory, L1SuperRoot, L2ChainHeader, L5ChainEvent, L6FinalityCommitment, V5ProofBlob};
use std::convert::Infallible;
use tokio::sync::mpsc;

/// Minimal Stargate V2 SSE payload.
const STARGATE_INTENT_JSON: &str = r#"{
    "address": "T:1000000/proto:stargate_v2/deposit:1:0xdeadcafe",
    "entity": "proto",
    "id": "stargate_v2",
    "action": "deposit",
    "chain_id": 1,
    "ref_hash": "0xdeadcafe0000000000000000000000000000000000000000000000000000cafe",
    "src_chain": 1,
    "dst_chain": 42161,
    "depositor": "0xabcdef0123456789abcdef0123456789abcdef01",
    "recipient": "0xabcdef0123456789abcdef0123456789abcdef01",
    "src_token": "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
    "input_amount": "1000000",
    "ts": 1700000001
}"#;

async fn sse_handler_stargate() -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let data = STARGATE_INTENT_JSON.replace('\n', " ");
    let ev = Event::default().event("genome").data(data);
    let s = stream::once(async move { Ok::<Event, Infallible>(ev) });
    Sse::new(s)
}

fn stub_proof(src_chain: u64, tx_hash: &str) -> V5ProofBlob {
    V5ProofBlob {
        l1_superroot: L1SuperRoot { hash: "0x0".into(), timestamp: 0, chains_included: vec![] },
        l2_chain_header: L2ChainHeader {
            chain_id: src_chain, block_number: 0,
            block_hash: "0x0".into(), parent_hash: "0x0".into(),
            state_root: "0x0".into(), timestamp: 0,
        },
        l3_superroot_proof: vec![],
        l4_block_proof: vec![],
        l5_chain_event: L5ChainEvent {
            tx_hash: tx_hash.to_string(), tx_index: 0, log_index: None,
            encoded_tx: "0x".into(), encoded_receipt: "0x".into(),
        },
        l6_finality: L6FinalityCommitment {
            finality_type: "DEPTH_BASED".into(), commitment_data: "{}".into(),
        },
    }
}

/// Published intent from genome SSE reaches AdapterFactory quote path.
#[tokio::test]
async fn genome_sse_intent_reaches_quote_path() {
    // Spin up a local SSE server.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new().route("/api/genome/subscribe/sse", get(sse_handler_stargate));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let url = format!("http://{}/api/genome/subscribe/sse", addr);
    let client = GenomeClient::new(url);

    let (tx, mut rx) = mpsc::channel::<Intent>(8);

    let handle = tokio::spawn(async move { let _ = client.subscribe(tx).await; });

    // Receive the intent from the SSE stream (5s timeout).
    let intent = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for intent from genome SSE")
        .expect("channel closed before intent arrived");

    handle.abort();

    // Verify intent fields.
    assert_eq!(intent.protocol, "stargate_v2");
    assert_eq!(intent.src_chain, 1);
    assert_eq!(intent.dst_chain, 42161);
    assert_eq!(intent.amount, "1000000");

    // Route through AdapterFactory — verifies the quote path is reachable.
    let factory = AdapterFactory::new("https://api.taifoon.dev");
    let adapter = factory.get_adapter(&intent)
        .expect("AdapterFactory must return a StargateAdapter for stargate_v2");
    assert_eq!(adapter.protocol_name(), "stargate_v2");

    // Call build_fill_tx to prove the quote path executes without panic.
    let proof = stub_proof(intent.src_chain, &intent.tx_hash);
    let fill_tx = adapter.build_fill_tx(&intent, &proof).await
        .expect("build_fill_tx must succeed for a valid Stargate intent");
    assert_eq!(fill_tx.chain_id, 1, "Stargate sends from src chain");
    assert!(fill_tx.data.starts_with("0x"), "fill_tx.data must be hex calldata");
    assert!(fill_tx.data.len() > 2, "calldata must be non-empty");

    // execute_fill in dry-run: must not broadcast, must return simulated=true.
    let result = adapter.execute_fill(&intent, fill_tx, true).await
        .expect("execute_fill(dry_run=true) must succeed");
    assert!(result.simulated, "dry-run must produce simulated=true");
    assert!(result.success, "dry-run simulation must succeed");
}

/// DRY_RUN=true prevents broadcast — execute_fill returns simulated result.
#[tokio::test]
async fn dry_run_prevents_broadcast() {
    let factory = AdapterFactory::new("https://api.taifoon.dev");

    // Relay intent
    let relay_intent = Intent {
        id: "relay:test:dry_run".into(),
        protocol: "relay".into(),
        src_chain: 1,
        dst_chain: 42161,
        src_token: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".into(),
        dst_token: "0xaf88d065e77c8cC2239327C5EDb3A432268e5831".into(),
        amount: "500000".into(),
        depositor: "0x1111111111111111111111111111111111111111".into(),
        recipient: "0x2222222222222222222222222222222222222222".into(),
        tx_hash: "0xaaaa000000000000000000000000000000000000000000000000000000000aaa".into(),
        detected_at: 1700000002,
        ..Default::default()
    };

    let adapter = factory.get_adapter(&relay_intent)
        .expect("RelayAdapter must be returned for 'relay' protocol");
    assert_eq!(adapter.protocol_name(), "relay");

    let proof = stub_proof(relay_intent.src_chain, &relay_intent.tx_hash);
    let fill_tx = adapter.build_fill_tx(&relay_intent, &proof).await
        .expect("build_fill_tx must succeed");

    // dry_run = true → no broadcast
    let result = adapter.execute_fill(&relay_intent, fill_tx, true).await
        .expect("dry_run execute_fill must succeed");
    assert!(result.simulated);
    assert!(result.success);

    // dry_run = false would broadcast for real — we don't test that path here,
    // but we can verify the simulated=false path would be set by the adapter.
}

/// CCTP intent flows through quote path end-to-end (build_fill_tx → dry-run).
#[tokio::test]
async fn cctp_intent_quote_path_end_to_end() {
    let factory = AdapterFactory::new("https://api.taifoon.dev");

    let cctp_intent = Intent {
        id: "cctp:test:0xbeef".into(),
        protocol: "cctp".into(),
        src_chain: 1,
        dst_chain: 42161,
        src_token: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".into(),
        dst_token: "0xaf88d065e77c8cC2239327C5EDb3A432268e5831".into(),
        amount: "2000000".into(),
        depositor: "0x3333333333333333333333333333333333333333".into(),
        recipient: "0x4444444444444444444444444444444444444444".into(),
        tx_hash: "0xbeef000000000000000000000000000000000000000000000000000000000eef".into(),
        detected_at: 1700000003,
        ..Default::default()
    };

    let adapter = factory.get_adapter(&cctp_intent)
        .expect("CctpAdapter must be returned for 'cctp' protocol");
    assert_eq!(adapter.protocol_name(), "cctp");

    let proof = stub_proof(cctp_intent.src_chain, &cctp_intent.tx_hash);
    let fill_tx = adapter.build_fill_tx(&cctp_intent, &proof).await
        .expect("CctpAdapter.build_fill_tx must succeed");
    assert_eq!(fill_tx.chain_id, 42161, "CCTP receiveMessage on dst chain");

    let result = adapter.execute_fill(&cctp_intent, fill_tx, true).await
        .expect("CCTP dry-run must succeed");
    assert!(result.simulated);
    assert!(result.success);
}
