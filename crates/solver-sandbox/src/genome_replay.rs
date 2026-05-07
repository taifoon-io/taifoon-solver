//! GenomeReplay — serves a local SSE stream from a recorded NDJSON snapshot.
//!
//! Mimics the genome.taifoon.dev SSE endpoint so competing solver binaries
//! can point their GENOME_SSE_URL at the local sandbox and replay real intent
//! history at any speed multiplier.

use axum::{
    extract::State,
    response::sse::{Event, Sse},
    routing::get,
    Router,
};
use futures::stream::{self};
use serde::{Deserialize, Serialize};
use std::{
    convert::Infallible,
    sync::Arc,
    time::Duration,
};
use tokio::sync::RwLock;
use tracing::info;

/// One line from the genome NDJSON snapshot.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GenomeEvent {
    pub id: String,
    pub kind: String,
    pub summary: String,
    #[serde(default)]
    pub meta: serde_json::Value,
    pub ts: u64,
}

pub struct ReplayState {
    pub events: Vec<GenomeEvent>,
    pub speed_multiplier: f64,
    pub loop_playback: bool,
}

impl ReplayState {
    pub fn from_ndjson(path: &str, speed: f64, loop_it: bool) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("cannot read genome snapshot {}: {}", path, e))?;

        let events: Vec<GenomeEvent> = raw
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();

        info!("GenomeReplay: loaded {} events from {}", events.len(), path);
        Ok(Self { events, speed_multiplier: speed, loop_playback: loop_it })
    }

    pub fn from_synthetic(count: usize) -> Self {
        // Vary amounts across 3 sizes: $500, $1200, $3000 (in USDC 6-decimals)
        let amounts = ["500000000", "1200000000", "3000000000"];
        let dst_chains = [8453u64, 42161, 10];
        let events = (0..count).map(|i| {
            let amount = amounts[i % amounts.len()];
            let max_reward_usd = amount.parse::<u128>().unwrap() + amount.parse::<u128>().unwrap() / 100; // +1%
            GenomeEvent {
                id: format!("0x{:064x}", i + 1),
                kind: "order".to_string(),
                summary: format!("ETH→USDC fill #{} (${:.0})", i + 1,
                    amount.parse::<u128>().unwrap_or(0) as f64 / 1_000_000.0),
                meta: serde_json::json!({
                    "src_chain": 1,
                    "dst_chain": dst_chains[i % dst_chains.len()],
                    "amount": amount,
                    "src_token": "0x0000000000000000000000000000000000000000",
                    "dst_token": "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913",
                    "max_reward": max_reward_usd.to_string(),
                    "recipient": "0x0000000000000000000000000000000000000001",
                }),
                ts: 1700000000 + (i as u64 * 30),
            }
        }).collect();
        Self { events, speed_multiplier: 10.0, loop_playback: false }
    }
}

pub type SharedReplay = Arc<RwLock<ReplayState>>;

/// Build the axum router for the local genome SSE endpoint.
pub fn replay_router(state: SharedReplay) -> Router {
    Router::new()
        .route("/api/genome/subscribe/sse", get(sse_handler))
        .with_state(state)
}

async fn sse_handler(
    State(state): State<SharedReplay>,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let replay = state.read().await;
    let events = replay.events.clone();
    let speed = replay.speed_multiplier;
    let loop_it = replay.loop_playback;
    drop(replay);

    let base_interval_ms = (1000.0 / speed.max(0.001)) as u64;

    let stream = stream::unfold(
        (events, 0usize, loop_it),
        move |(evs, idx, lp)| async move {
            if idx >= evs.len() {
                if lp {
                    // loop back to start
                    tokio::time::sleep(Duration::from_millis(base_interval_ms)).await;
                    Some((Ok(Event::default().data("loop_restart")), (evs, 0, lp)))
                } else {
                    None
                }
            } else {
                tokio::time::sleep(Duration::from_millis(base_interval_ms)).await;
                let ev = &evs[idx];
                let data = serde_json::to_string(ev).unwrap_or_default();
                Some((Ok(Event::default().id(&ev.id).event(&ev.kind).data(data)), (evs, idx + 1, lp)))
            }
        },
    );

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_events_generated() {
        let state = ReplayState::from_synthetic(5);
        assert_eq!(state.events.len(), 5);
        assert_eq!(state.events[0].kind, "order");
    }

    #[test]
    fn ndjson_parse_returns_empty_on_missing_file() {
        let result = ReplayState::from_ndjson("/nonexistent/path.ndjson", 1.0, false);
        assert!(result.is_err());
    }
}
