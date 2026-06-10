use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use protocol_adapters::AdapterFactory;

use crate::v5::V5Skeleton;

#[derive(Debug, Clone)]
pub struct SidecarConfig {
    pub port: u16,
    pub simulation_mode: bool,
    pub live_fill_ok: bool,
    pub network: String,
    pub spinner_api_url: String,
}

impl SidecarConfig {
    pub fn from_env() -> Self {
        let port = std::env::var("SIDECAR_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(8090);
        // SIMULATION_MODE defaults to true (safe). Only the exact string "false" flips it off.
        let simulation_mode = std::env::var("SIMULATION_MODE")
            .map(|v| v.to_lowercase() != "false")
            .unwrap_or(true);
        let live_fill_ok = std::env::var("LIVE_FILL_OK")
            .map(|v| v.to_lowercase() == "yes")
            .unwrap_or(false);
        let network = std::env::var("ARC_NETWORK").unwrap_or_else(|_| {
            if simulation_mode {
                "simulation".into()
            } else {
                "mainnet".into()
            }
        });
        let spinner_api_url = std::env::var("WARMBED_API_URL")
            .or_else(|_| std::env::var("SPINNER_API_URL"))
            .unwrap_or_else(|_| "https://api.taifoon.dev".into());
        Self {
            port,
            simulation_mode,
            live_fill_ok,
            network,
            spinner_api_url,
        }
    }

    /// True only if both safety keys are flipped to live.
    pub fn live_fills_unlocked(&self) -> bool {
        !self.simulation_mode && self.live_fill_ok
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredIntent {
    pub sidecar_intent_id: String,
    pub protocol_slug: String,
    pub src_chain_id: u64,
    pub dst_chain_id: u64,
    pub dst_kind: String,
    pub src_token: String,
    pub dst_token: String,
    pub input_amount: String,
    pub output_amount: Option<String>,
    pub recipient: String,
    pub dry_run_requested: bool,
    pub dry_run_effective: bool,
    pub status: String,
    pub fill_tx: Option<String>,
    pub fill_block: Option<u64>,
    pub gas_used: Option<u64>,
    pub value_routed_usd: Option<f64>,
    pub v5: Option<V5Skeleton>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct SidecarState {
    pub config: SidecarConfig,
    pub factory: Arc<AdapterFactory>,
    pub intents: Arc<RwLock<HashMap<String, StoredIntent>>>,
}

impl SidecarState {
    pub fn new(config: SidecarConfig) -> Self {
        let factory = Arc::new(AdapterFactory::new(config.spinner_api_url.clone()));
        Self {
            config,
            factory,
            intents: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}
