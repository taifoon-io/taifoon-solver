//! t3rn-sidecar — inbound HTTP receiver that lets taifoon-arc dispatch fills
//! into this solver's adapter fleet.
//!
//! Pipeline:
//!   arc-api `POST /v1/fill`
//!     -> sidecar `POST /api/sidecar/intent`
//!       -> AdapterFactory.get_adapter(intent)
//!         -> adapter.execute_fill(..., dry_run = !live_mode)
//!           -> sidecar caches FillReceipt
//!     <- sidecar replies { accepted, sidecar_intent_id }
//!   arc-api polls `GET /api/sidecar/intent/:id` for the receipt
//!
//! Two-key live-fill gate:
//!   SIMULATION_MODE != "true" AND LIVE_FILL_OK == "yes"
//! Otherwise dry-run is forced regardless of payload.dry_run.

pub mod dispatch;
pub mod routes;
pub mod state;
pub mod v5;

pub use state::{SidecarConfig, SidecarState};
pub use routes::router;
