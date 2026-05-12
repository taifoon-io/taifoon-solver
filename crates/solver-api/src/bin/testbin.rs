//! Test-only binary for the Python integration rig.
//!
//! Two modes:
//!
//! 1. **Default** (no args): boots `SolverApi::router()` against a
//!    `HOSTING_DB_PATH`-controlled SQLite with `SOLVER_API_TOKEN` from the
//!    environment, listening on `127.0.0.1:$PORT` (default `18080`).
//!
//! 2. **`sign-attestation`**: reads a JSON spec from stdin and prints a
//!    signed `DonutAttestation` JSON to stdout. Lets Python tests obtain a
//!    valid signature without re-implementing the canonical-JSON byte-stable
//!    serialization in Python (Python's `json` and Rust's `serde_json` use
//!    different float formatters in edge cases).
//!
//! `sign-attestation` stdin shape:
//!
//! ```json
//! {
//!   "priv_key_hex": "0x4c08...",
//!   "intent_id": "intent-xyz",
//!   "tx_hash": "0xfeed",
//!   "protocol": "mayan_swift",
//!   "src_chain": 1399811149,
//!   "dst_chain": 1,
//!   "actual_profit_usd": 0.42,
//!   "creator_addr": "0x111...aaaa",
//!   "reviewer_addrs": ["0x...aaaa", "0x...bbbb"],
//!   "ecosystem_addr": "0x...eeee",
//!   "prev_hash": "0x0000000000000000000000000000000000000000000000000000000000000000"
//! }
//! ```
//!
//! This mode does NO HTTP — it's a pure signer for use by `pytest` shelling
//! out via `subprocess.run`. Production never uses this.

use std::net::SocketAddr;
use std::str::FromStr;

use alloy::primitives::Address;
use alloy::signers::local::PrivateKeySigner;
use anyhow::Context;
use chrono::Utc;
use donut_adjudicator::{
    canonical_json_for_signing, hash_for_chain, AdapterRegistry, CanonicalAdjudicator,
    DonutAttestation, FeeSplitAdjudicator, ZERO_HASH,
};
use executor::OutcomeRecord;
use serde::Deserialize;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "sign-attestation" {
        return sign_from_stdin();
    }
    run_server().await
}

async fn run_server() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new("solver_api=info,tower_http=warn,info")
    });
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(18_080);
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();

    let api = solver_api::SolverApi::new();
    let router = api.router();

    tracing::info!(
        "solver-api-testbin listening on http://{} (hosting_db={:?})",
        addr,
        std::env::var("HOSTING_DB_PATH").ok(),
    );

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router).await?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct SignSpec {
    priv_key_hex: String,
    intent_id: String,
    tx_hash: Option<String>,
    protocol: String,
    src_chain: u64,
    dst_chain: u64,
    actual_profit_usd: f64,
    creator_addr: String,
    reviewer_addrs: Vec<String>,
    ecosystem_addr: String,
    #[serde(default = "default_prev_hash")]
    prev_hash: String,
}

fn default_prev_hash() -> String {
    ZERO_HASH.to_string()
}

fn sign_from_stdin() -> anyhow::Result<()> {
    let mut buf = String::new();
    use std::io::Read;
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("read sign-attestation spec from stdin")?;
    let spec: SignSpec = serde_json::from_str(&buf).context("parse spec JSON")?;

    let signer = PrivateKeySigner::from_str(&spec.priv_key_hex).context("parse priv key")?;
    let creator_addr =
        Address::from_str(&spec.creator_addr).context("parse creator_addr")?;
    let ecosystem_addr =
        Address::from_str(&spec.ecosystem_addr).context("parse ecosystem_addr")?;
    let reviewer_addrs: Vec<Address> = spec
        .reviewer_addrs
        .iter()
        .map(|s| Address::from_str(s).context("parse reviewer addr"))
        .collect::<anyhow::Result<Vec<_>>>()?;

    // Build a synthetic OutcomeRecord exactly as the executor would.
    let rec = OutcomeRecord {
        ts: Utc::now(),
        intent_id: spec.intent_id.clone(),
        protocol: spec.protocol.clone(),
        src_chain: spec.src_chain,
        dst_chain: spec.dst_chain,
        decision: "executed".into(),
        tx_hash: spec.tx_hash.clone(),
        predicted_gas: Some(250_000),
        gas_used: Some(250_000),
        effective_gas_price_wei: None,
        predicted_profit_usd: Some(spec.actual_profit_usd),
        actual_profit_usd: Some(spec.actual_profit_usd),
        skip_reason: None,
        error: None,
        solver_id: None,
        claim_tx_hash: None,
        claim_fee_usd: None,
        fee_usd: Some(spec.actual_profit_usd),
    };

    // Build a registry that maps the resolved adapter_id → the supplied
    // creator + reviewers. The canonical adjudicator routes to ecosystem
    // when the registry is empty, which is the fail-closed path — pass
    // empty registry from Python to exercise that.
    //
    // adapter_id_for_outcome is what the adjudicator uses internally; we
    // need to call it the same way to know which key to insert into the
    // registry. Since registry mapping is keyed on the resolved id, we
    // mirror that here.
    let adapter_id = donut_adjudicator::adapter_id_for_outcome(&rec);
    let registry = AdapterRegistry::new(ecosystem_addr)
        .with_adapter(&adapter_id, creator_addr, reviewer_addrs.clone());

    // Run the canonical adjudicator and emit the signed attestation as
    // JSON. The Rust verify() will round-trip this output byte-stable.
    let att: DonutAttestation = futures::executor::block_on(async {
        CanonicalAdjudicator
            .attest(&rec, &registry, &signer, &spec.prev_hash)
            .await
    })?;

    // Diagnostic — produce the signing pre-image and the hash chain link
    // alongside the attestation, so Python tests can chain without having
    // to recompute them.
    let signing_preimage = canonical_json_for_signing(&att)?;
    let next_prev = hash_for_chain(&att)?;

    let envelope = serde_json::json!({
        "attestation": att,
        "signing_preimage": signing_preimage,
        "next_prev_hash": next_prev,
    });
    println!("{}", serde_json::to_string(&envelope)?);
    Ok(())
}
