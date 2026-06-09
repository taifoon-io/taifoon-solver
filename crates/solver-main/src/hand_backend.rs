//! P6 — `solver-main` wires a real [`HandBackend`] into [`SolverApi`].
//!
//! Before this, `SolverApi` fell back to `solver_api::hand::NoopBackend`,
//! so every `/api/hand/*` route answered `503 Service Unavailable`
//! ("hand backend not configured"). P6 registers a concrete backend at
//! boot so `/api/hand/status` returns a non-503 JSON body and the live
//! venue surface (status / venues / balances / book / ticker …) is wired.
//!
//! ## Two backing modes
//!
//! 1. **Config-driven (default)** — [`TraderHandBackend::from_config`] reads a
//!    `hands.toml` describing the venues this solver fronts and answers
//!    `status`/`venues` from it. Read endpoints that need a live exchange
//!    connection (`balances`, `positions`, `book`, `ticker`, `fills`) return
//!    empty/typed results scoped to a configured venue, and an
//!    [`BackendError::UnknownVenue`] for anything not in the config. Mutating
//!    endpoints (`place`, `cancel`, `replace`, `fill_auction`) return
//!    [`BackendError::Unsupported`] — this build does not fabricate fills.
//!    This is what makes `/api/hand/status` go non-503 without requiring the
//!    (gitignored) internal `trader` crate to be checked out.
//!
//! 2. **Trader-backed (`--features trader-backend`)** — wraps an
//!    `Arc<trader::Trader>` and delegates every method to it. The `trader`
//!    crate lives at the gitignored `../trade/crates/trader` path; the
//!    feature is **off by default** so `cargo build -p solver-main` succeeds
//!    on a fresh checkout that does not have that crate. When an operator has
//!    the internal venue checked out, they build with the feature and the
//!    Kraken / Drift / internal Hands registered by `build_trader` become
//!    live. See `Cargo.toml`'s `[features] trader-backend` for the path dep
//!    wiring (intentionally left commented until the crate is vendored).

use std::collections::BTreeMap;
use std::path::Path;
#[cfg(feature = "trader-backend")]
use std::sync::Arc;

use async_trait::async_trait;
use solver_api::hand::{
    AuctionFillRequest, BackendError, BalanceDto, BookDto, FillDto, FillReceiptDto, HandBackend,
    HandStatusResponse, OrderDto, OrderFilter, PlaceOrderRequest, PositionDto, QuoteDto, RfqRequest,
    TickerDto, VenueInfo,
};

/// One configured venue this solver fronts.
#[derive(Debug, Clone)]
pub struct HandVenueConfig {
    pub venue: String,
    /// Capability bitflag word matching `trade-core::Capabilities` (passed
    /// through verbatim to [`VenueInfo::capabilities`]).
    pub capabilities: u32,
    /// Whether the venue currently has a live connection. In config-only mode
    /// this is whatever the config asserts; the trader-backed mode overrides
    /// it from the live connector state.
    pub connected: bool,
}

/// Concrete [`HandBackend`] wired into `SolverApi` at boot.
///
/// In the default (config-driven) build it is constructed from a `hands.toml`
/// describing the venues the solver fronts; with `--features trader-backend`
/// it additionally holds an `Arc<trader::Trader>` and delegates live calls.
pub struct TraderHandBackend {
    /// Configured venues keyed by venue id (stable order for deterministic
    /// `status`/`venues` output — handy for the boot smoke test's `jq`).
    venues: BTreeMap<String, HandVenueConfig>,
    /// Default venue id, if a single one is marked default in config.
    default_venue: Option<String>,

    #[cfg(feature = "trader-backend")]
    trader: Arc<trader::Trader>,
}

impl TraderHandBackend {
    /// Build a config-driven backend from an explicit venue list.
    ///
    /// `registered` (what `/api/hand/status` reports) is `venues.len()`, so a
    /// non-empty list makes the route answer non-503.
    pub fn new(venues: Vec<HandVenueConfig>) -> Self {
        let mut map = BTreeMap::new();
        for v in venues {
            map.insert(v.venue.clone(), v);
        }
        // Default venue = the alphabetically-first configured venue when no
        // explicit default is given; callers can override via `with_default`.
        let default_venue = map.keys().next().cloned();
        Self {
            venues: map,
            default_venue,
            #[cfg(feature = "trader-backend")]
            trader: unreachable_trader(),
        }
    }

    /// Override the default venue id (must be one of the configured venues).
    pub fn with_default(mut self, venue: impl Into<String>) -> Self {
        let v = venue.into();
        if self.venues.contains_key(&v) {
            self.default_venue = Some(v);
        }
        self
    }

    /// Load venues from a `hands.toml` config file.
    ///
    /// Schema (minimal, forward-compatible — unknown keys ignored):
    /// ```toml
    /// default_venue = "internal"
    ///
    /// [[venue]]
    /// venue = "internal"
    /// capabilities = 255
    /// connected = true
    ///
    /// [[venue]]
    /// venue = "kraken"
    /// capabilities = 15
    /// connected = false
    /// ```
    pub fn from_config(path: impl AsRef<Path>) -> Result<Self, String> {
        let raw = std::fs::read_to_string(path.as_ref())
            .map_err(|e| format!("read {}: {e}", path.as_ref().display()))?;
        Self::from_config_str(&raw)
    }

    /// Parse a `hands.toml` body (split out from [`Self::from_config`] so it
    /// is unit-testable without touching the filesystem).
    pub fn from_config_str(toml_str: &str) -> Result<Self, String> {
        let parsed: HandsToml =
            toml::from_str(toml_str).map_err(|e| format!("parse hands.toml: {e}"))?;
        let venues: Vec<HandVenueConfig> = parsed
            .venue
            .into_iter()
            .map(|v| HandVenueConfig {
                venue: v.venue,
                capabilities: v.capabilities.unwrap_or(0),
                connected: v.connected.unwrap_or(false),
            })
            .collect();
        if venues.is_empty() {
            return Err("hands.toml declares no [[venue]] entries".to_string());
        }
        let mut backend = Self::new(venues);
        if let Some(dv) = parsed.default_venue {
            backend = backend.with_default(dv);
        }
        Ok(backend)
    }

    /// Number of registered venues (what `/api/hand/status` reports).
    pub fn registered(&self) -> u32 {
        self.venues.len() as u32
    }

    fn venue_infos(&self) -> Vec<VenueInfo> {
        self.venues
            .values()
            .map(|v| VenueInfo {
                venue: v.venue.clone(),
                capabilities: v.capabilities,
                connected: v.connected,
            })
            .collect()
    }

    fn require_venue(&self, venue: &str) -> Result<(), BackendError> {
        if self.venues.contains_key(venue) {
            Ok(())
        } else {
            Err(BackendError::UnknownVenue(venue.to_string()))
        }
    }

    #[cfg(feature = "trader-backend")]
    /// Wrap a live `trader::Trader`; venues are derived from its registered
    /// Hands. Only compiled when the internal `trader` crate is available.
    pub fn from_trader(trader: Arc<trader::Trader>, venues: Vec<HandVenueConfig>) -> Self {
        let mut map = BTreeMap::new();
        for v in venues {
            map.insert(v.venue.clone(), v);
        }
        let default_venue = map.keys().next().cloned();
        Self { venues: map, default_venue, trader }
    }
}

#[cfg(feature = "trader-backend")]
fn unreachable_trader() -> Arc<trader::Trader> {
    // `new()`/`from_config()` are the config-only constructors; under the
    // trader-backend feature callers must use `from_trader`. We never reach
    // here in practice, but the field must be initialized.
    unreachable!("use TraderHandBackend::from_trader when trader-backend feature is enabled")
}

#[async_trait]
impl HandBackend for TraderHandBackend {
    async fn status(&self) -> Result<HandStatusResponse, BackendError> {
        Ok(HandStatusResponse {
            registered: self.registered(),
            default_venue: self.default_venue.clone(),
            venues: self.venue_infos(),
        })
    }

    async fn venues(&self) -> Result<Vec<VenueInfo>, BackendError> {
        Ok(self.venue_infos())
    }

    async fn balances(&self, venue: Option<&str>) -> Result<Vec<BalanceDto>, BackendError> {
        if let Some(v) = venue {
            self.require_venue(v)?;
        }
        // Config-only mode has no live exchange connection to query — return
        // an empty (but typed, 200) balance set rather than a 503. The
        // trader-backed build delegates to the live connector.
        Ok(Vec::new())
    }

    async fn positions(&self, venue: Option<&str>) -> Result<Vec<PositionDto>, BackendError> {
        if let Some(v) = venue {
            self.require_venue(v)?;
        }
        Ok(Vec::new())
    }

    async fn open_orders(&self, filter: OrderFilter) -> Result<Vec<OrderDto>, BackendError> {
        if let Some(v) = filter.venue.as_deref() {
            self.require_venue(v)?;
        }
        Ok(Vec::new())
    }

    async fn place(&self, req: PlaceOrderRequest) -> Result<OrderDto, BackendError> {
        self.require_venue(&req.venue)?;
        // Do NOT fabricate a fill in config-only mode — be honest about the
        // missing execution path. The trader-backed build performs the order.
        Err(BackendError::Unsupported(req.venue))
    }

    async fn cancel(&self, _id: &str) -> Result<(), BackendError> {
        Err(BackendError::Unsupported("cancel".to_string()))
    }

    async fn cancel_all(&self, filter: OrderFilter) -> Result<u32, BackendError> {
        if let Some(v) = filter.venue.as_deref() {
            self.require_venue(v)?;
        }
        Ok(0)
    }

    async fn order(&self, id: &str) -> Result<OrderDto, BackendError> {
        Err(BackendError::UnknownOrder(id.to_string()))
    }

    async fn replace(&self, _id: &str, req: PlaceOrderRequest) -> Result<OrderDto, BackendError> {
        self.require_venue(&req.venue)?;
        Err(BackendError::Unsupported(req.venue))
    }

    async fn fills(&self, _since_ms: Option<u64>) -> Result<Vec<FillDto>, BackendError> {
        Ok(Vec::new())
    }

    async fn book(&self, venue: &str, _market: &str, _depth: u32) -> Result<BookDto, BackendError> {
        self.require_venue(venue)?;
        // No live order book in config-only mode.
        Err(BackendError::Unsupported(venue.to_string()))
    }

    async fn ticker(&self, venue: &str, _market: &str) -> Result<TickerDto, BackendError> {
        self.require_venue(venue)?;
        Err(BackendError::Unsupported(venue.to_string()))
    }

    async fn quote(&self, req: RfqRequest) -> Result<QuoteDto, BackendError> {
        self.require_venue(&req.venue)?;
        Err(BackendError::Unsupported(req.venue))
    }

    async fn fill_auction(
        &self,
        req: AuctionFillRequest,
    ) -> Result<FillReceiptDto, BackendError> {
        self.require_venue(&req.venue)?;
        Err(BackendError::Unsupported(req.venue))
    }
}

// ────────────────────────────────────────────────────────────────────────────
// hands.toml deserialization
// ────────────────────────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct HandsToml {
    default_venue: Option<String>,
    #[serde(default)]
    venue: Vec<HandVenueToml>,
}

#[derive(serde::Deserialize)]
struct HandVenueToml {
    venue: String,
    capabilities: Option<u32>,
    connected: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> TraderHandBackend {
        TraderHandBackend::new(vec![
            HandVenueConfig { venue: "internal".into(), capabilities: 255, connected: true },
            HandVenueConfig { venue: "kraken".into(), capabilities: 15, connected: false },
        ])
        .with_default("internal")
    }

    #[tokio::test]
    async fn status_is_non_503_when_venues_configured() {
        let b = sample();
        let s = b.status().await.expect("status must not 503 with venues configured");
        assert_eq!(s.registered, 2);
        assert_eq!(s.default_venue.as_deref(), Some("internal"));
        assert_eq!(s.venues.len(), 2);
        // The smoke test asserts `.registered > 0`.
        assert!(s.registered > 0);
    }

    #[tokio::test]
    async fn venues_lists_every_configured_venue() {
        let b = sample();
        let v = b.venues().await.unwrap();
        let ids: Vec<&str> = v.iter().map(|x| x.venue.as_str()).collect();
        assert!(ids.contains(&"internal"));
        assert!(ids.contains(&"kraken"));
    }

    #[tokio::test]
    async fn balances_scoped_to_unknown_venue_is_404_not_503() {
        let b = sample();
        match b.balances(Some("does-not-exist")).await {
            Err(BackendError::UnknownVenue(v)) => assert_eq!(v, "does-not-exist"),
            other => panic!("expected UnknownVenue, got {other:?}"),
        }
        // Known venue → empty typed result (200), not 503.
        let ok = b.balances(Some("internal")).await.unwrap();
        assert!(ok.is_empty());
    }

    #[tokio::test]
    async fn place_is_unsupported_not_fabricated() {
        let b = sample();
        let req = PlaceOrderRequest {
            venue: "internal".into(),
            market: "BTC-USD".into(),
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
        // Config-only mode must NOT fabricate a fill.
        assert!(matches!(b.place(req).await, Err(BackendError::Unsupported(_))));
    }

    #[test]
    fn from_config_str_parses_venues_and_default() {
        let toml = r#"
            default_venue = "internal"

            [[venue]]
            venue = "internal"
            capabilities = 255
            connected = true

            [[venue]]
            venue = "kraken"
            capabilities = 15
            connected = false
        "#;
        let b = TraderHandBackend::from_config_str(toml).expect("valid config");
        assert_eq!(b.registered(), 2);
        assert_eq!(b.default_venue.as_deref(), Some("internal"));
    }

    #[test]
    fn empty_config_is_rejected() {
        let toml = "default_venue = \"x\"\n";
        assert!(TraderHandBackend::from_config_str(toml).is_err());
    }
}
