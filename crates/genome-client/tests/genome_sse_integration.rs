//! Integration test: spin up a local axum SSE server, subscribe with GenomeClient,
//! assert at least one Intent arrives through the channel.

use axum::{
    response::sse::{Event, Sse},
    routing::get,
    Router,
};
use futures::stream;
use genome_client::{GenomeClient, Intent};
use std::convert::Infallible;
use tokio::sync::mpsc;

/// Minimal SSE payload that parse_sse_event accepts:
/// - event type "genome"
/// - entity "proto", action "deposit"
/// - required Intent fields (src_chain, dst_chain, src_token, input_amount)
const GENOME_EVENT_JSON: &str = r#"{
    "address": "T:1/proto:lifi_v2/deposit:1:0xdeadbeef",
    "entity": "proto",
    "id": "lifi_v2",
    "action": "deposit",
    "chain_id": 1,
    "ref_hash": "0xdeadbeef",
    "src_chain": 1,
    "dst_chain": 42161,
    "depositor": "0xabcdef0123456789abcdef0123456789abcdef01",
    "recipient": "0xabcdef0123456789abcdef0123456789abcdef01",
    "src_token": "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
    "input_amount": "5000000",
    "ts": 1700000000
}"#;

/// Axum SSE handler: emits one `genome` event then closes the stream.
async fn sse_handler() -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let data = GENOME_EVENT_JSON.replace('\n', " ");
    let ev = Event::default().event("genome").data(data);
    let s = stream::once(async move { Ok::<Event, Infallible>(ev) });
    Sse::new(s)
}

#[tokio::test]
async fn genome_sse_consumer_receives_at_least_one_intent() {
    // Bind on an OS-assigned port so tests never conflict.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let app = Router::new().route("/api/genome/subscribe/sse", get(sse_handler));

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let url = format!("http://{}/api/genome/subscribe/sse", addr);
    let client = GenomeClient::new(url);

    let (tx, mut rx) = mpsc::channel::<Intent>(8);

    // subscribe() loops forever on clean close; run it in a task and cancel after receipt.
    let handle = tokio::spawn(async move {
        let _ = client.subscribe(tx).await;
    });

    // Wait up to 5 seconds for the first intent.
    let intent = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for genome intent")
        .expect("channel closed without intent");

    handle.abort();

    assert_eq!(intent.protocol, "lifi_v2");
    assert_eq!(intent.src_chain, 1);
    assert_eq!(intent.dst_chain, 42161);
    assert_eq!(intent.amount, "5000000");
}
