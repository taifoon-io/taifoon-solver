//! Spinner solver-API client used by the Across executor.
//!
//! Endpoints (per task spec, section 4.3):
//!   GET  /api/v5/proof/bundle/across/<order_id>   -> raw V5ProofBlob bytes
//!   POST /api/solver/test-run {protocol, order_id} -> profit decision

use anyhow::{anyhow, Context, Result};
use serde::Serialize;

#[derive(Clone)]
pub struct SpinnerSolverClient {
    base_url: String,
    http: reqwest::Client,
}

impl SpinnerSolverClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(8))
                .build()
                .expect("reqwest client"),
        }
    }

    /// POST /api/solver/test-run
    pub async fn test_run(&self, protocol: &str, order_id: &str) -> Result<TestRunResult> {
        let url = format!("{}/api/solver/test-run", self.base_url);
        #[derive(Serialize)]
        struct Req<'a> {
            protocol: &'a str,
            order_id: &'a str,
        }
        let resp = self
            .http
            .post(&url)
            .json(&Req { protocol, order_id })
            .send()
            .await
            .with_context(|| format!("POST {}", url))?;
        if !resp.status().is_success() {
            return Err(anyhow!(
                "test-run http {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let raw: serde_json::Value = resp.json().await?;
        TestRunResult::from_value(raw)
    }

    /// GET /api/v5/proof/bundle/across/<order_id>
    /// Returns the raw proof blob bytes (operator decodes via V5Codec on-chain).
    pub async fn fetch_across_proof_bundle(&self, order_id: &str) -> Result<Vec<u8>> {
        let url = format!("{}/api/v5/proof/bundle/across/{}", self.base_url, order_id);
        let resp = self.http.get(&url).send().await
            .with_context(|| format!("GET {}", url))?;
        if !resp.status().is_success() {
            return Err(anyhow!(
                "proof-bundle http {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let ct = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        if ct.starts_with("application/octet-stream") {
            return Ok(resp.bytes().await?.to_vec());
        }
        // JSON shape: {"proof_blob_hex":"0x..."} or {"proof":"0x..."} or full V5 layered struct
        let v: serde_json::Value = resp.json().await?;
        if let Some(s) = v.get("proof_blob_hex").and_then(|x| x.as_str()) {
            return Ok(decode_hex(s)?);
        }
        if let Some(s) = v.get("proof").and_then(|x| x.as_str()) {
            return Ok(decode_hex(s)?);
        }
        if let Some(s) = v.get("blob").and_then(|x| x.as_str()) {
            return Ok(decode_hex(s)?);
        }
        // Fallback: encode the JSON body directly so the operator-side codec can take over.
        Ok(serde_json::to_vec(&v)?)
    }
}

fn decode_hex(s: &str) -> Result<Vec<u8>> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    hex::decode(s).map_err(Into::into)
}

#[derive(Debug, Clone, Default)]
pub struct TestRunResult {
    pub is_profitable: bool,
    pub net_profit_usd: f64,
    pub gas_units: u64,
    pub gas_price_wei: Option<u128>,
    pub gas_cost_usd: f64,
    pub recommendation: String,
}

impl TestRunResult {
    fn from_value(v: serde_json::Value) -> Result<Self> {
        // Tolerate both the documented shape (nested profitability/gas_estimate) and a flat one.
        let (is_profitable, net_profit_usd, recommendation) = if let Some(p) = v.get("profitability")
        {
            (
                p.get("is_profitable").and_then(|x| x.as_bool()).unwrap_or(false),
                p.get("net_profit_usd").and_then(|x| x.as_f64()).unwrap_or(0.0),
                p.get("recommendation")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
            )
        } else {
            (
                v.get("is_profitable").and_then(|x| x.as_bool()).unwrap_or_else(|| {
                    v.get("profitable").and_then(|x| x.as_bool()).unwrap_or(false)
                }),
                v.get("net_profit_usd").and_then(|x| x.as_f64()).unwrap_or(0.0),
                v.get("recommendation").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            )
        };

        let (gas_units, gas_cost_usd, gas_price_wei) = if let Some(g) = v.get("gas_estimate") {
            let units = g
                .get("total_gas_units")
                .or_else(|| g.get("gas_units"))
                .and_then(|x| x.as_u64())
                .unwrap_or(0);
            let cost = g
                .get("gas_cost_usd")
                .or_else(|| g.get("total_cost_usd"))
                .and_then(|x| x.as_f64())
                .unwrap_or(0.0);
            let price_wei = g
                .get("gas_price_wei")
                .and_then(|x| x.as_u64())
                .map(|x| x as u128)
                .or_else(|| {
                    g.get("gas_price_gwei")
                        .and_then(|x| x.as_f64())
                        .map(|gwei| (gwei * 1_000_000_000.0) as u128)
                });
            (units, cost, price_wei)
        } else {
            (
                v.get("gas_units").and_then(|x| x.as_u64()).unwrap_or(0),
                v.get("gas_cost_usd").and_then(|x| x.as_f64()).unwrap_or(0.0),
                None,
            )
        };

        Ok(Self {
            is_profitable,
            net_profit_usd,
            gas_units,
            gas_price_wei,
            gas_cost_usd,
            recommendation,
        })
    }
}
