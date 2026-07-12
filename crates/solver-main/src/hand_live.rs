//! P8 — `solver-main` consumes algotrada-brain **live-mode** signals via the
//! `HandBackend` relay.
//!
//! P6 wired a concrete [`HandBackend`] from static `hands.toml` so
//! `/api/hand/status` stopped returning `503`. P7 forwarded that hand state
//! outward to the spinner explorer. P8 closes the loop the other way: instead
//! of answering the hand surface purely from local config, `solver-main` can
//! **reflect the algotrada-brain's live-mode venue state** — so when the brain
//! flips a venue live (the off-by-default gate landed in brain `18b70e8d`), the
//! solver's `/api/hand/*` surface (and therefore the P7 relay snapshot the
//! explorer sees) reports that live venue/connectivity.
//!
//! ## Env gate (off by default — mirrors the brain's `ALGOTRADA_LIVE`)
//!
//! Live mode is **opt-in** and engages only when BOTH hold:
//!
//! * `ALGOTRADA_LIVE` is truthy (`1`/`true`/`yes`/`on`), AND
//! * `BRAIN_LIVE_MODE_URL` names the brain's hand surface base URL
//!   (e.g. `http://127.0.0.1:8083`).
//!
//! When the gate is off (the default), [`maybe_live_backend`] returns `None`
//! and `main` keeps the existing config-driven [`TraderHandBackend`] verbatim —
//! a fresh checkout, CI, and every operator who has not explicitly opted in are
//! completely unaffected.
//!
//! ## Posture: fail-open to the static fallback (mirrors [`crate::hand_relay`])
//!
//! The brain is a *downstream signal*, not a hard dependency. Every read of the
//! brain's live status is best-effort: on any transport error, non-2xx, or
//! parse failure, [`LiveModeHandBackend::status`] (and the venue/connectivity
//! reads derived from it) **fall back to the static config backend** rather than
//! returning `503`. The solver never blocks on, or panics because of, the brain
//! being down. Mutating endpoints stay exactly as the inner config backend
//! defines them — live *reflection* never fabricates an execution path.
//!
//! ## Wire contract (the brain's live-mode hand surface)
//!
//! `GET {BRAIN_LIVE_MODE_URL}/api/hand/status` → a JSON [`HandStatusResponse`]
//! (the same shape P6 serves and P7 relays):
//!
//! ```json
//! { "registered": 1, "default_venue": "drift-perps",
//!   "venues": [ { "venue": "drift-perps", "capabilities": 255, "connected": true } ] }
//! ```

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use solver_api::hand::{
    AuctionFillRequest, BackendError, BalanceDto, BookDto, FillDto, FillReceiptDto, HandBackend,
    HandStatusResponse, OrderDto, OrderFilter, PlaceOrderRequest, PositionDto, QuoteDto, RfqRequest,
    TickerDto, VenueInfo,
};
use tracing::{info, warn};

/// How the live-mode reflection is configured at boot.
#[derive(Debug, Clone)]
pub struct LiveModeConfig {
    /// Brain live-mode hand-surface base URL, e.g. `http://127.0.0.1:8083`.
    /// No trailing slash required — it is stripped before composing the endpoint.
    pub brain_base_url: String,
    /// HTTP timeout per status read. Kept short so a hung brain never stalls
    /// the solver's own `/api/hand/*` responses.
    pub timeout: Duration,
}

impl LiveModeConfig {
    /// Resolve the live-mode config from the environment, returning `None` when
    /// the gate is off (the default). Live mode engages only when
    /// `ALGOTRADA_LIVE` is truthy AND `BRAIN_LIVE_MODE_URL` is set & non-empty.
    pub fn from_env() -> Option<Self> {
        if !env_truthy("ALGOTRADA_LIVE") {
            return None;
        }
        let url = std::env::var("BRAIN_LIVE_MODE_URL").ok()?;
        let url = url.trim();
        if url.is_empty() {
            return None;
        }
        let timeout_secs = std::env::var("BRAIN_LIVE_MODE_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(5);
        Some(Self {
            brain_base_url: url.trim_end_matches('/').to_string(),
            timeout: Duration::from_secs(timeout_secs),
        })
    }
}

/// True when `var` is set to a truthy value (`1`/`true`/`yes`/`on`, case-insensitive).
/// Anything else — including unset, `0`, `false`, empty — is false (off by default).
fn env_truthy(var: &str) -> bool {
    std::env::var(var)
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            v == "1" || v == "true" || v == "yes" || v == "on"
        })
        .unwrap_or(false)
}

/// A [`HandBackend`] that reflects the algotrada-brain's live-mode venue state,
/// falling back to a static `inner` backend on any failure.
///
/// `status`/`venues`/`book`/`ticker` connectivity is derived from the brain's
/// live status when reachable; everything else (and every read when the brain is
/// unreachable) delegates to `inner`, so behaviour degrades gracefully to the
/// config-driven P6 backend rather than to `503`.
pub struct LiveModeHandBackend {
    inner: Arc<dyn HandBackend>,
    config: LiveModeConfig,
    http: reqwest::Client,
}

impl LiveModeHandBackend {
    /// Wrap a static `inner` backend with brain live-mode reflection.
    pub fn new(inner: Arc<dyn HandBackend>, config: LiveModeConfig) -> Self {
        let http = reqwest::Client::builder()
            .timeout(config.timeout)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { inner, config, http }
    }

    /// Fetch the brain's live `HandStatusResponse`. Returns `None` (logged) on
    /// any transport error, non-2xx, or parse failure — the caller then falls
    /// back to `inner`. Never panics, never blocks past the configured timeout.
    async fn fetch_live_status(&self) -> Option<HandStatusResponse> {
        let url = format!("{}/api/hand/status", self.config.brain_base_url);
        match self.http.get(&url).send().await {
            Ok(resp) => {
                let code = resp.status();
                if !code.is_success() {
                    warn!(
                        "🧠 hand_live: brain status {} returned {} — falling back to config backend",
                        url,
                        code.as_u16()
                    );
                    return None;
                }
                match resp.json::<HandStatusResponse>().await {
                    Ok(status) => Some(status),
                    Err(e) => {
                        warn!("🧠 hand_live: brain status parse failed ({e}) — falling back");
                        None
                    }
                }
            }
            Err(e) => {
                warn!("🧠 hand_live: brain status unreachable ({e}) — falling back to config backend");
                None
            }
        }
    }
}

#[async_trait]
impl HandBackend for LiveModeHandBackend {
    async fn status(&self) -> Result<HandStatusResponse, BackendError> {
        // Live status wins when the brain is reachable; otherwise the config
        // backend's status (fail-open). Never surfaces a 503 from the brain.
        if let Some(live) = self.fetch_live_status().await {
            return Ok(live);
        }
        self.inner.status().await
    }

    async fn venues(&self) -> Result<Vec<VenueInfo>, BackendError> {
        if let Some(live) = self.fetch_live_status().await {
            return Ok(live.venues);
        }
        self.inner.venues().await
    }

    // ── Everything below delegates to the static inner backend. Live mode
    //    reflects venue *state*; it does not fabricate balances, fills, books,
    //    or an execution path the inner backend does not itself provide.
    async fn balances(&self, venue: Option<&str>) -> Result<Vec<BalanceDto>, BackendError> {
        self.inner.balances(venue).await
    }

    async fn positions(&self, venue: Option<&str>) -> Result<Vec<PositionDto>, BackendError> {
        self.inner.positions(venue).await
    }

    async fn open_orders(&self, filter: OrderFilter) -> Result<Vec<OrderDto>, BackendError> {
        self.inner.open_orders(filter).await
    }

    async fn place(&self, req: PlaceOrderRequest) -> Result<OrderDto, BackendError> {
        self.inner.place(req).await
    }

    async fn cancel(&self, id: &str) -> Result<(), BackendError> {
        self.inner.cancel(id).await
    }

    async fn cancel_all(&self, filter: OrderFilter) -> Result<u32, BackendError> {
        self.inner.cancel_all(filter).await
    }

    async fn order(&self, id: &str) -> Result<OrderDto, BackendError> {
        self.inner.order(id).await
    }

    async fn replace(&self, id: &str, req: PlaceOrderRequest) -> Result<OrderDto, BackendError> {
        self.inner.replace(id, req).await
    }

    async fn fills(&self, since_ms: Option<u64>) -> Result<Vec<FillDto>, BackendError> {
        self.inner.fills(since_ms).await
    }

    async fn book(&self, venue: &str, market: &str, depth: u32) -> Result<BookDto, BackendError> {
        self.inner.book(venue, market, depth).await
    }

    async fn ticker(&self, venue: &str, market: &str) -> Result<TickerDto, BackendError> {
        self.inner.ticker(venue, market).await
    }

    async fn quote(&self, req: RfqRequest) -> Result<QuoteDto, BackendError> {
        self.inner.quote(req).await
    }

    async fn fill_auction(&self, req: AuctionFillRequest) -> Result<FillReceiptDto, BackendError> {
        self.inner.fill_auction(req).await
    }
}

/// Boot helper: if the env gate is on, wrap `inner` in a [`LiveModeHandBackend`]
/// and return it; otherwise return `None` so `main` keeps the static backend
/// verbatim. Logs once which path was taken.
pub fn maybe_live_backend(inner: Arc<dyn HandBackend>) -> Option<Arc<dyn HandBackend>> {
    match LiveModeConfig::from_env() {
        Some(config) => {
            info!(
                "🧠 Hand backend: algotrada-brain LIVE mode ON → reflecting {} (fail-open to config; timeout {}s)",
                config.brain_base_url,
                config.timeout.as_secs()
            );
            Some(Arc::new(LiveModeHandBackend::new(inner, config)))
        }
        None => None,
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A static inner backend with a fixed status and a delegate-call counter,
    /// so tests can prove fail-open delegation without a live brain.
    struct StubInner {
        status_calls: AtomicUsize,
    }

    impl StubInner {
        fn new() -> Self {
            Self { status_calls: AtomicUsize::new(0) }
        }
    }

    #[async_trait]
    impl HandBackend for StubInner {
        async fn status(&self) -> Result<HandStatusResponse, BackendError> {
            self.status_calls.fetch_add(1, Ordering::SeqCst);
            Ok(HandStatusResponse {
                registered: 1,
                default_venue: Some("internal".into()),
                venues: vec![VenueInfo {
                    venue: "internal".into(),
                    capabilities: 0,
                    connected: false,
                }],
            })
        }
        async fn venues(&self) -> Result<Vec<VenueInfo>, BackendError> {
            Ok(vec![VenueInfo { venue: "internal".into(), capabilities: 0, connected: false }])
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
        async fn fills(&self, _s: Option<u64>) -> Result<Vec<FillDto>, BackendError> {
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

    fn live_cfg(base: String) -> LiveModeConfig {
        LiveModeConfig { brain_base_url: base, timeout: Duration::from_secs(2) }
    }

    /// Spawn a one-shot HTTP server returning `status_line` + `body` to one GET,
    /// returning its base URL. Mirrors hand_relay's test harness style.
    async fn one_shot_server(status_line: &'static str, body: &'static str) -> (String, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = sock.read(&mut buf).await;
                let resp = format!(
                    "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            }
        });
        (format!("http://{}", addr), handle)
    }

    #[test]
    fn env_truthy_only_for_known_truthy_values() {
        for (v, want) in [("1", true), ("true", true), ("YES", true), ("on", true), ("0", false), ("false", false), ("", false), ("nope", false)] {
            std::env::set_var("ALGOTRADA_LIVE_TEST_TMP", v);
            assert_eq!(env_truthy("ALGOTRADA_LIVE_TEST_TMP"), want, "value {v:?}");
        }
        std::env::remove_var("ALGOTRADA_LIVE_TEST_TMP");
        assert!(!env_truthy("DEFINITELY_UNSET_VAR_XYZ"));
    }

    #[tokio::test]
    async fn live_status_reflects_brain_when_reachable() {
        let body = r#"{"registered":2,"default_venue":"drift-perps","venues":[{"venue":"drift-perps","capabilities":255,"connected":true},{"venue":"kraken","capabilities":15,"connected":true}]}"#;
        let (base, server) = one_shot_server("HTTP/1.1 200 OK", body).await;
        let inner = Arc::new(StubInner::new());
        let be = LiveModeHandBackend::new(inner.clone(), live_cfg(base));

        let s = be.status().await.unwrap();
        assert_eq!(s.registered, 2, "brain live status must win over config");
        assert_eq!(s.default_venue.as_deref(), Some("drift-perps"));
        assert!(s.venues.iter().any(|v| v.venue == "drift-perps" && v.connected));
        // Inner status was NOT consulted — the brain answered.
        assert_eq!(inner.status_calls.load(Ordering::SeqCst), 0);
        let _ = server.await;
    }

    #[tokio::test]
    async fn falls_back_to_config_when_brain_unreachable() {
        // Point at a closed port (nothing listening) → transport error → fallback.
        let inner = Arc::new(StubInner::new());
        let be = LiveModeHandBackend::new(
            inner.clone(),
            // 127.0.0.1:1 is reserved/closed; the short timeout keeps this fast.
            LiveModeConfig { brain_base_url: "http://127.0.0.1:1".into(), timeout: Duration::from_millis(300) },
        );
        let s = be.status().await.unwrap();
        // Config backend's status (registered=1, internal) — never a 503.
        assert_eq!(s.registered, 1);
        assert_eq!(s.default_venue.as_deref(), Some("internal"));
        assert_eq!(inner.status_calls.load(Ordering::SeqCst), 1, "inner must be the fallback");
    }

    #[tokio::test]
    async fn falls_back_on_non_2xx() {
        let (base, server) = one_shot_server("HTTP/1.1 503 Service Unavailable", "down").await;
        let inner = Arc::new(StubInner::new());
        let be = LiveModeHandBackend::new(inner.clone(), live_cfg(base));
        let s = be.status().await.unwrap();
        assert_eq!(s.registered, 1, "non-2xx from brain → config fallback, not a surfaced 503");
        assert_eq!(inner.status_calls.load(Ordering::SeqCst), 1);
        let _ = server.await;
    }

    #[tokio::test]
    async fn falls_back_on_malformed_body() {
        let (base, server) = one_shot_server("HTTP/1.1 200 OK", "not json at all").await;
        let inner = Arc::new(StubInner::new());
        let be = LiveModeHandBackend::new(inner.clone(), live_cfg(base));
        let s = be.status().await.unwrap();
        assert_eq!(s.registered, 1, "unparseable brain body → config fallback");
        assert_eq!(inner.status_calls.load(Ordering::SeqCst), 1);
        let _ = server.await;
    }

    #[tokio::test]
    async fn venues_reflect_brain_when_reachable() {
        let body = r#"{"registered":1,"default_venue":"drift-perps","venues":[{"venue":"drift-perps","capabilities":255,"connected":true}]}"#;
        let (base, server) = one_shot_server("HTTP/1.1 200 OK", body).await;
        let be = LiveModeHandBackend::new(Arc::new(StubInner::new()), live_cfg(base));
        let v = be.venues().await.unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].venue, "drift-perps");
        assert!(v[0].connected);
        let _ = server.await;
    }

    #[tokio::test]
    async fn mutating_path_delegates_to_inner_and_never_fabricates() {
        // Even with the brain reachable, place() must delegate to inner (which
        // is Unsupported in config-only mode) — live reflection ≠ fabricated fill.
        let body = r#"{"registered":1,"default_venue":"drift-perps","venues":[{"venue":"drift-perps","capabilities":255,"connected":true}]}"#;
        let (base, server) = one_shot_server("HTTP/1.1 200 OK", body).await;
        let be = LiveModeHandBackend::new(Arc::new(StubInner::new()), live_cfg(base));
        let req = PlaceOrderRequest {
            venue: "drift-perps".into(),
            market: "BTC-PERP".into(),
            side: "buy".into(),
            size: "1".into(),
            kind: "market".into(),
            price: None,
            stop: None,
            take: None,
            trail_percent: None,
            trail_absolute: None,
            offset_bps: None,
            tif: None,
            gtt_until: None,
            reduce_only: None,
            client_id: None,
            attribution: None,
            auction: None,
        };
        assert!(matches!(be.place(req).await, Err(BackendError::Unsupported(_))));
        let _ = server.await;
    }

    #[test]
    fn config_from_env_gate_off_by_default() {
        // Snapshot + clear both gate vars so the test is deterministic.
        let prev_live = std::env::var("ALGOTRADA_LIVE").ok();
        let prev_url = std::env::var("BRAIN_LIVE_MODE_URL").ok();
        std::env::remove_var("ALGOTRADA_LIVE");
        std::env::remove_var("BRAIN_LIVE_MODE_URL");

        // Gate off → None.
        assert!(LiveModeConfig::from_env().is_none(), "default (unset) must be off");

        // ALGOTRADA_LIVE on but no URL → still None.
        std::env::set_var("ALGOTRADA_LIVE", "1");
        assert!(LiveModeConfig::from_env().is_none(), "live flag without URL must be off");

        // Both set → Some, URL trailing slash stripped.
        std::env::set_var("BRAIN_LIVE_MODE_URL", "http://127.0.0.1:8083/");
        let cfg = LiveModeConfig::from_env().expect("both set → live");
        assert_eq!(cfg.brain_base_url, "http://127.0.0.1:8083", "trailing slash stripped");

        // ALGOTRADA_LIVE explicitly false → off even with URL set.
        std::env::set_var("ALGOTRADA_LIVE", "false");
        assert!(LiveModeConfig::from_env().is_none(), "ALGOTRADA_LIVE=false must be off");

        // Restore prior env.
        match prev_live { Some(v) => std::env::set_var("ALGOTRADA_LIVE", v), None => std::env::remove_var("ALGOTRADA_LIVE") }
        match prev_url { Some(v) => std::env::set_var("BRAIN_LIVE_MODE_URL", v), None => std::env::remove_var("BRAIN_LIVE_MODE_URL") }
    }
}
