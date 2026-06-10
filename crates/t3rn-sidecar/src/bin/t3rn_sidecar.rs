//! Boot binary for the t3rn-sidecar HTTP receiver.
//!
//! Reads `SIDECAR_PORT` (default 8090), `SIMULATION_MODE`, `LIVE_FILL_OK`,
//! `ARC_NETWORK`, `WARMBED_API_URL` from the environment. Binds 0.0.0.0
//! so docker can route in.

use std::net::SocketAddr;

use anyhow::Result;
use tracing::info;
use tracing_subscriber::EnvFilter;

use t3rn_sidecar::{router, SidecarConfig, SidecarState};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with_target(false)
        .init();

    let config = SidecarConfig::from_env();
    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    info!(
        port = config.port,
        simulation = config.simulation_mode,
        live_fill_ok = config.live_fill_ok,
        network = %config.network,
        "t3rn-sidecar starting"
    );

    let state = SidecarState::new(config);
    let app = router(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
