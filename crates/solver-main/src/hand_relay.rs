//! P7 — Spinner hand-state relay.
//!
//! P6 wired a concrete [`HandBackend`] into `SolverApi` so `/api/hand/status`
//! answers a live [`HandStatusResponse`] (registered venues, default venue,
//! per-venue connectivity) instead of `503`. P7 is the **wire** that forwards
//! that hand state outward: a background task that periodically reads the
//! solver's own `HandBackend` status and POSTs a snapshot to the spinner
//! explorer so the explorer can surface which venues this solver fronts and
//! whether they are connected — without the explorer having to reach back into
//! the solver's `/api/hand/*` surface.
//!
//! ## Posture (mirrors [`crate::attestation_pump`])
//!
//! The relay is **best-effort and fail-open**: the solver's `HandBackend` is
//! the source of truth, the spinner explorer is a downstream view. Any HTTP
//! failure (explorer down, network blip, non-2xx) is logged and retried on the
//! next tick. The loop **never panics the host process** — if `run_relay`
//! returns an error it is logged and the task exits gracefully.
//!
//! ## Wire contract
//!
//! Each tick POSTs JSON to `{spinner_base}/api/solver/hand-state`:
//!
//! ```json
//! {
//!   "solver_id": "0xabc…",          // optional; the solver address when known
//!   "as_of": 1717934400,            // unix seconds
//!   "status": {                      // verbatim HandStatusResponse from P6
//!     "registered": 2,
//!     "default_venue": "internal",
//!     "venues": [ { "venue": "internal", "capabilities": 255, "connected": true }, … ]
//!   }
//! }
//! ```
//!
//! The explorer ingest is expected to upsert by `solver_id` (latest snapshot
//! wins). A 2xx is success; any other status is logged and retried next tick.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use serde::Serialize;
use solver_api::hand::{HandBackend, HandStatusResponse};
use tracing::{debug, error, info, warn};

/// How the relay is configured at boot.
#[derive(Debug, Clone)]
pub struct HandRelayConfig {
    /// Spinner explorer base URL, e.g. `https://api.taifoon.dev`. No trailing
    /// slash required — we strip it before composing the endpoint.
    pub spinner_base_url: String,
    /// Optional solver identity (wallet address) stamped onto every snapshot so
    /// the explorer can key/upsert by solver. `None` is allowed — the snapshot
    /// simply omits the field.
    pub solver_id: Option<String>,
    /// How often to read hand state and forward it.
    pub poll_interval: Duration,
}

impl Default for HandRelayConfig {
    fn default() -> Self {
        Self {
            spinner_base_url: "https://api.taifoon.dev".to_string(),
            solver_id: None,
            poll_interval: Duration::from_secs(30),
        }
    }
}

/// The snapshot envelope POSTed to the explorer each tick.
#[derive(Debug, Clone, Serialize)]
pub struct HandStateSnapshot {
    /// Solver identity (wallet address), when known. Omitted from the wire when
    /// `None` so an unidentified solver doesn't masquerade as a `null`-id one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solver_id: Option<String>,
    /// Unix seconds at which this snapshot was taken.
    pub as_of: i64,
    /// The verbatim P6 hand status (registered venues, default, connectivity).
    pub status: HandStatusResponse,
}

impl HandStateSnapshot {
    /// Build a snapshot from a live backend status + identity, stamping
    /// `as_of` from the supplied unix-seconds clock value (injected so the
    /// builder stays pure and unit-testable).
    pub fn new(solver_id: Option<String>, as_of: i64, status: HandStatusResponse) -> Self {
        Self { solver_id, as_of, status }
    }
}

/// Spawn the spinner hand-state relay. Returns the [`tokio::task::JoinHandle`]
/// of the running task; callers usually let it drop and rely on tokio's
/// detached-task semantics — the loop runs until the runtime is shut down.
pub fn spawn_hand_relay(
    backend: Arc<dyn HandBackend>,
    config: HandRelayConfig,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        if let Err(e) = run_relay(backend, config).await {
            // run_relay loops forever; if it returns Err we log and exit
            // gracefully rather than panicking the host process.
            error!("hand_relay exited unexpectedly: {}", e);
        }
    })
}

async fn run_relay(
    backend: Arc<dyn HandBackend>,
    config: HandRelayConfig,
) -> anyhow::Result<()> {
    let base = config.spinner_base_url.trim_end_matches('/').to_string();
    info!(
        "🤝 hand_relay: forwarding hand state to {} every {}s (solver_id={})",
        base,
        config.poll_interval.as_secs(),
        config.solver_id.as_deref().unwrap_or("<unset>"),
    );

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    loop {
        // ── Read hand state (P6's HandBackend::status). A backend error here
        //    is not fatal — log and retry next tick.
        match backend.status().await {
            Ok(status) => {
                let snapshot = HandStateSnapshot::new(
                    config.solver_id.clone(),
                    Utc::now().timestamp(),
                    status,
                );
                match post_snapshot(&http, &base, &snapshot).await {
                    Ok(true) => {
                        debug!(
                            "🤝 hand_relay: forwarded {} venue(s) to explorer",
                            snapshot.status.registered
                        );
                    }
                    Ok(false) => { /* non-2xx already logged in post_snapshot */ }
                    Err(e) => {
                        // Network/transport error — explorer unreachable, etc.
                        warn!("🤝 hand_relay: POST hand-state failed (retrying next tick): {}", e);
                    }
                }
            }
            Err(e) => {
                warn!("🤝 hand_relay: backend.status() errored (retrying next tick): {:?}", e);
            }
        }

        tokio::time::sleep(config.poll_interval).await;
    }
}

/// POST one snapshot to `{base}/api/solver/hand-state`. Returns `Ok(true)` on a
/// 2xx, `Ok(false)` on a non-2xx (logged, retried next tick), and `Err` only on
/// a transport-level failure. Never panics.
async fn post_snapshot(
    http: &reqwest::Client,
    base: &str,
    snapshot: &HandStateSnapshot,
) -> anyhow::Result<bool> {
    let url = format!("{}/api/solver/hand-state", base);
    let resp = http.post(&url).json(snapshot).send().await?;
    let status = resp.status();
    if status.is_success() {
        Ok(true)
    } else {
        let body = resp.text().await.unwrap_or_default();
        warn!(
            "🤝 hand_relay: explorer returned {} for hand-state POST: {}",
            status.as_u16(),
            body
        );
        Ok(false)
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use solver_api::hand::{
        AuctionFillRequest, BackendError, BalanceDto, BookDto, FillDto, FillReceiptDto, OrderDto,
        OrderFilter, PlaceOrderRequest, PositionDto, QuoteDto, RfqRequest, TickerDto, VenueInfo,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A backend stub that returns a fixed status and counts how many times
    /// `status()` was polled — enough to exercise the relay's read path and
    /// the snapshot builder without a live trader.
    struct StubBackend {
        polls: AtomicUsize,
    }

    impl StubBackend {
        fn new() -> Self {
            Self { polls: AtomicUsize::new(0) }
        }
    }

    #[async_trait]
    impl HandBackend for StubBackend {
        async fn status(&self) -> Result<HandStatusResponse, BackendError> {
            self.polls.fetch_add(1, Ordering::SeqCst);
            Ok(HandStatusResponse {
                registered: 2,
                default_venue: Some("internal".into()),
                venues: vec![
                    VenueInfo { venue: "internal".into(), capabilities: 255, connected: true },
                    VenueInfo { venue: "kraken".into(), capabilities: 15, connected: false },
                ],
            })
        }
        async fn venues(&self) -> Result<Vec<VenueInfo>, BackendError> {
            Ok(Vec::new())
        }
        async fn balances(&self, _v: Option<&str>) -> Result<Vec<BalanceDto>, BackendError> {
            Ok(Vec::new())
        }
        async fn positions(&self, _v: Option<&str>) -> Result<Vec<PositionDto>, BackendError> {
            Ok(Vec::new())
        }
        async fn open_orders(&self, _f: OrderFilter) -> Result<Vec<OrderDto>, BackendError> {
            Ok(Vec::new())
        }
        async fn place(&self, req: PlaceOrderRequest) -> Result<OrderDto, BackendError> {
            Err(BackendError::Unsupported(req.venue))
        }
        async fn cancel(&self, _id: &str) -> Result<(), BackendError> {
            Err(BackendError::Unsupported("cancel".into()))
        }
        async fn cancel_all(&self, _f: OrderFilter) -> Result<u32, BackendError> {
            Ok(0)
        }
        async fn order(&self, id: &str) -> Result<OrderDto, BackendError> {
            Err(BackendError::UnknownOrder(id.into()))
        }
        async fn replace(&self, _id: &str, req: PlaceOrderRequest) -> Result<OrderDto, BackendError> {
            Err(BackendError::Unsupported(req.venue))
        }
        async fn fills(&self, _since: Option<u64>) -> Result<Vec<FillDto>, BackendError> {
            Ok(Vec::new())
        }
        async fn book(&self, v: &str, _m: &str, _d: u32) -> Result<BookDto, BackendError> {
            Err(BackendError::Unsupported(v.into()))
        }
        async fn ticker(&self, v: &str, _m: &str) -> Result<TickerDto, BackendError> {
            Err(BackendError::Unsupported(v.into()))
        }
        async fn quote(&self, req: RfqRequest) -> Result<QuoteDto, BackendError> {
            Err(BackendError::Unsupported(req.venue))
        }
        async fn fill_auction(&self, req: AuctionFillRequest) -> Result<FillReceiptDto, BackendError> {
            Err(BackendError::Unsupported(req.venue))
        }
    }

    #[tokio::test]
    async fn snapshot_carries_backend_status_verbatim() {
        let backend = StubBackend::new();
        let status = backend.status().await.unwrap();
        let snap = HandStateSnapshot::new(Some("0xabc".into()), 1_717_934_400, status);

        assert_eq!(snap.solver_id.as_deref(), Some("0xabc"));
        assert_eq!(snap.as_of, 1_717_934_400);
        assert_eq!(snap.status.registered, 2);
        assert_eq!(snap.status.default_venue.as_deref(), Some("internal"));
        assert_eq!(snap.status.venues.len(), 2);
        // The backend was polled exactly once for this snapshot.
        assert_eq!(backend.polls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn snapshot_serializes_to_explorer_contract() {
        let status = HandStatusResponse {
            registered: 1,
            default_venue: Some("internal".into()),
            venues: vec![VenueInfo {
                venue: "internal".into(),
                capabilities: 255,
                connected: true,
            }],
        };
        let snap = HandStateSnapshot::new(Some("0xdef".into()), 1_717_934_400, status);
        let v: serde_json::Value = serde_json::to_value(&snap).unwrap();

        assert_eq!(v["solver_id"], "0xdef");
        assert_eq!(v["as_of"], 1_717_934_400_i64);
        assert_eq!(v["status"]["registered"], 1);
        assert_eq!(v["status"]["default_venue"], "internal");
        assert_eq!(v["status"]["venues"][0]["venue"], "internal");
        assert_eq!(v["status"]["venues"][0]["connected"], true);
    }

    #[test]
    fn snapshot_omits_solver_id_when_unset() {
        let status = HandStatusResponse {
            registered: 0,
            default_venue: None,
            venues: vec![],
        };
        let snap = HandStateSnapshot::new(None, 1_717_934_400, status);
        let v: serde_json::Value = serde_json::to_value(&snap).unwrap();
        // `solver_id` must be absent (not null) when unset.
        assert!(v.get("solver_id").is_none(), "solver_id should be omitted when None");
        assert_eq!(v["status"]["registered"], 0);
        assert!(v["status"]["default_venue"].is_null());
    }

    #[tokio::test]
    async fn post_snapshot_returns_false_on_non_2xx_and_does_not_panic() {
        // Bind an ephemeral listener that immediately returns 500 to one POST,
        // proving non-2xx is handled (Ok(false)) without panicking.
        use tokio::io::AsyncWriteExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                // Drain the request (best-effort) then reply 500.
                let mut buf = [0u8; 1024];
                let _ = tokio::io::AsyncReadExt::read(&mut sock, &mut buf).await;
                let _ = sock
                    .write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 3\r\n\r\nerr")
                    .await;
            }
        });

        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let status = HandStatusResponse {
            registered: 1,
            default_venue: Some("internal".into()),
            venues: vec![VenueInfo { venue: "internal".into(), capabilities: 1, connected: true }],
        };
        let snap = HandStateSnapshot::new(None, 1_717_934_400, status);
        let base = format!("http://{}", addr);
        let ok = post_snapshot(&http, &base, &snap).await.unwrap();
        assert!(!ok, "non-2xx must return Ok(false), not panic");
        let _ = server.await;
    }

    #[tokio::test]
    async fn post_snapshot_returns_true_on_2xx() {
        use tokio::io::AsyncWriteExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = tokio::io::AsyncReadExt::read(&mut sock, &mut buf).await;
                let _ = sock
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                    .await;
            }
        });

        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let status = HandStatusResponse { registered: 0, default_venue: None, venues: vec![] };
        let snap = HandStateSnapshot::new(Some("0x1".into()), 1, status);
        let base = format!("http://{}/", addr); // trailing slash exercised by run_relay's trim
        let ok = post_snapshot(&http, base.trim_end_matches('/'), &snap)
            .await
            .unwrap();
        assert!(ok, "2xx must return Ok(true)");
        let _ = server.await;
    }
}
