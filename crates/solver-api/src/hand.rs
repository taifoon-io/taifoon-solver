//! # `/api/hand/*` — solver-side Hand surface
//!
//! HTTP extension exposing the operations a `Hand` would expose locally
//! (place / cancel / book / balances / positions / fills) over the wire.
//! The solver-api process is a first-class Hand consumer reachable from
//! language-agnostic clients (curl, dashboard, TS/Python agents).
//!
//! ## Why this lives in `taifoon-solver`
//!
//! `algotrada-brain` already uses `/api/hand/*` (status, positions, trades,
//! performance, rules, calibration). This module gives the solver the
//! matching surface, so the two systems' Hand vocabularies line up.
//!
//! ## Backend wiring
//!
//! The routes here do not depend on the `taifoon-trade` wrapper crate
//! (which lives outside the public workspace). They depend on a local
//! `HandBackend` trait. The solver binary (`solver-main`) wires a real
//! `HandBackend` impl into `SolverApi::set_hand_backend(...)` at process
//! start. Without one, every route returns `503 Service Unavailable`.
//!
//! This keeps `taifoon-solver` compilable as-is, while letting any
//! operator plug in a backend (a `taifoon-trade::Trader` shim, a thin
//! mock for tests, or an alternative client) without code changes here.
//!
//! ## Route map
//!
//! | Method | Path                                  | Purpose                                  |
//! |--------|---------------------------------------|------------------------------------------|
//! | GET    | `/api/hand/status`                    | which Hands are registered + their caps  |
//! | GET    | `/api/hand/venues`                    | list of active venue ids                 |
//! | GET    | `/api/hand/venues/:venue/balances`    | balances at one venue                    |
//! | GET    | `/api/hand/venues/:venue/positions`   | positions at one venue                   |
//! | GET    | `/api/hand/balances`                  | aggregated balances across all Hands     |
//! | GET    | `/api/hand/positions`                 | aggregated positions across all Hands    |
//! | GET    | `/api/hand/orders`                    | open orders (filter by `?venue=` or `?market=`) |
//! | POST   | `/api/hand/orders`                    | place a new order                        |
//! | GET    | `/api/hand/orders/:id`                | one order's status                       |
//! | DELETE | `/api/hand/orders/:id`                | cancel one order                         |
//! | POST   | `/api/hand/orders/:id/replace`        | cancel + place atomically (or sequenced) |
//! | DELETE | `/api/hand/orders`                    | cancel all (filter `?venue=` or `?market=`) |
//! | GET    | `/api/hand/fills`                     | recent fills                             |
//! | GET    | `/api/hand/fills/stream`              | SSE stream of fills                      |
//! | GET    | `/api/hand/book/:venue/:market`       | order book snapshot                      |
//! | GET    | `/api/hand/ticker/:venue/:market`     | ticker snapshot                          |
//! | POST   | `/api/hand/quote`                     | request RFQ from a JIT/RFQ-capable venue |
//! | POST   | `/api/hand/auction/:id/fill`          | fill an open JIT/Dutch auction           |
//!
//! Auth: mutation routes (`POST`, `DELETE`) are gated by the existing
//! `require_solver_api_token` middleware. GETs are public so the
//! dashboard can render without a token.

use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};

// ────────────────────────────────────────────────────────────────────────────
// Backend trait
// ────────────────────────────────────────────────────────────────────────────

/// Operators implement this to back the `/api/hand/*` routes.
///
/// Shape mirrors `taifoon-trade::Hand` but is intentionally string-typed
/// at the boundary so this module has no compile-time dependency on the
/// gitignored wrapper crate.
#[async_trait]
pub trait HandBackend: Send + Sync {
    async fn status(&self) -> Result<HandStatusResponse, BackendError>;
    async fn venues(&self) -> Result<Vec<VenueInfo>, BackendError>;

    async fn balances(&self, venue: Option<&str>) -> Result<Vec<BalanceDto>, BackendError>;
    async fn positions(&self, venue: Option<&str>) -> Result<Vec<PositionDto>, BackendError>;

    async fn open_orders(
        &self,
        filter: OrderFilter,
    ) -> Result<Vec<OrderDto>, BackendError>;

    async fn place(&self, req: PlaceOrderRequest) -> Result<OrderDto, BackendError>;
    async fn cancel(&self, id: &str) -> Result<(), BackendError>;
    async fn cancel_all(&self, filter: OrderFilter) -> Result<u32, BackendError>;
    async fn order(&self, id: &str) -> Result<OrderDto, BackendError>;
    async fn replace(
        &self,
        id: &str,
        req: PlaceOrderRequest,
    ) -> Result<OrderDto, BackendError>;

    async fn fills(&self, since_ms: Option<u64>) -> Result<Vec<FillDto>, BackendError>;
    async fn book(&self, venue: &str, market: &str, depth: u32) -> Result<BookDto, BackendError>;
    async fn ticker(&self, venue: &str, market: &str) -> Result<TickerDto, BackendError>;

    async fn quote(&self, req: RfqRequest) -> Result<QuoteDto, BackendError>;
    async fn fill_auction(&self, req: AuctionFillRequest) -> Result<FillReceiptDto, BackendError>;
}

/// Default no-op backend that returns 503 from every route.
pub struct NoopBackend;

#[async_trait]
impl HandBackend for NoopBackend {
    async fn status(&self) -> Result<HandStatusResponse, BackendError> {
        Err(BackendError::NotConfigured)
    }
    async fn venues(&self) -> Result<Vec<VenueInfo>, BackendError> { Err(BackendError::NotConfigured) }
    async fn balances(&self, _: Option<&str>) -> Result<Vec<BalanceDto>, BackendError> { Err(BackendError::NotConfigured) }
    async fn positions(&self, _: Option<&str>) -> Result<Vec<PositionDto>, BackendError> { Err(BackendError::NotConfigured) }
    async fn open_orders(&self, _: OrderFilter) -> Result<Vec<OrderDto>, BackendError> { Err(BackendError::NotConfigured) }
    async fn place(&self, _: PlaceOrderRequest) -> Result<OrderDto, BackendError> { Err(BackendError::NotConfigured) }
    async fn cancel(&self, _: &str) -> Result<(), BackendError> { Err(BackendError::NotConfigured) }
    async fn cancel_all(&self, _: OrderFilter) -> Result<u32, BackendError> { Err(BackendError::NotConfigured) }
    async fn order(&self, _: &str) -> Result<OrderDto, BackendError> { Err(BackendError::NotConfigured) }
    async fn replace(&self, _: &str, _: PlaceOrderRequest) -> Result<OrderDto, BackendError> { Err(BackendError::NotConfigured) }
    async fn fills(&self, _: Option<u64>) -> Result<Vec<FillDto>, BackendError> { Err(BackendError::NotConfigured) }
    async fn book(&self, _: &str, _: &str, _: u32) -> Result<BookDto, BackendError> { Err(BackendError::NotConfigured) }
    async fn ticker(&self, _: &str, _: &str) -> Result<TickerDto, BackendError> { Err(BackendError::NotConfigured) }
    async fn quote(&self, _: RfqRequest) -> Result<QuoteDto, BackendError> { Err(BackendError::NotConfigured) }
    async fn fill_auction(&self, _: AuctionFillRequest) -> Result<FillReceiptDto, BackendError> { Err(BackendError::NotConfigured) }
}

// ────────────────────────────────────────────────────────────────────────────
// DTOs — string-typed at the wire boundary on purpose
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VenueInfo {
    /// Stable venue identifier (e.g. "kraken", "drift-perps", "binance",
    /// "spinner", or an operator-supplied custom id).
    pub venue: String,
    /// Capability bitflag word matching `trade-core::Capabilities`.
    pub capabilities: u32,
    pub connected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandStatusResponse {
    pub registered: u32,
    pub default_venue: Option<String>,
    pub venues: Vec<VenueInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceDto {
    pub venue: String,
    pub asset: String,
    pub free: String,   // decimal as string
    pub locked: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionDto {
    pub venue: String,
    pub market: String,
    pub side: String,             // "long" | "short" | "flat"
    pub size: String,
    pub entry_price: String,
    pub mark_price: String,
    pub unrealized_pnl: String,
    pub leverage: Option<String>,
    pub liquidation_price: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderDto {
    pub id: String,
    pub client_id: String,
    pub venue: String,
    pub market: String,
    pub side: String,             // "buy" | "sell"
    pub kind: String,             // serialized OrderKind tag
    pub price: Option<String>,    // limit/stop/etc., absent for market
    pub size: String,
    pub filled: String,
    pub remaining: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OrderFilter {
    pub venue: Option<String>,
    pub market: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlaceOrderRequest {
    pub venue: String,
    pub market: String,
    pub side: String,
    pub size: String,
    /// Serialized `OrderKind` (one of: market, limit, post_only, stop,
    /// take_profit, stop_limit, trailing_stop, ioc, fok, oracle_limit,
    /// dutch_decay). Accompanying fields (price, stop, etc.) are top-level.
    pub kind: String,
    pub price: Option<String>,
    pub stop: Option<String>,
    pub take: Option<String>,
    pub trail_percent: Option<String>,
    pub trail_absolute: Option<String>,
    pub offset_bps: Option<i32>,
    pub tif: Option<String>,         // "gtc" | "gtt" | "day"
    pub gtt_until: Option<String>,
    pub reduce_only: Option<bool>,
    pub client_id: Option<String>,
    pub attribution: Option<AttributionDto>,
    pub auction: Option<AuctionParamsDto>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AttributionDto {
    pub source_chain_id: Option<u64>,
    pub source_tx_hash: Option<String>,
    pub protocol: Option<String>,
    pub builder: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuctionParamsDto {
    pub auction_id: Option<String>,
    pub exclusive_filler: Option<String>,
    pub exclusivity_secs: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillDto {
    pub order_id: String,
    pub trade_id: String,
    pub venue: String,
    pub market: String,
    pub side: String,
    pub price: String,
    pub size: String,
    pub fee: String,
    pub fee_asset: String,
    pub is_maker: bool,
    pub ts: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookDto {
    pub venue: String,
    pub market: String,
    pub bids: Vec<[String; 2]>,    // [price, size]
    pub asks: Vec<[String; 2]>,
    pub ts: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickerDto {
    pub venue: String,
    pub market: String,
    pub bid: Option<String>,
    pub ask: Option<String>,
    pub last: Option<String>,
    pub volume_24h: Option<String>,
    pub ts: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RfqRequest {
    pub venue: String,
    pub market: String,
    pub side: String,
    pub size: String,
    pub max_age_ms: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteDto {
    pub venue: String,
    pub price: String,
    pub valid_until: String,
    pub quote_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuctionFillRequest {
    pub venue: String,
    pub auction_id: String,
    pub quote_id: Option<String>,
    pub filler: String,
    pub price: String,
    pub size: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillReceiptDto {
    pub fills: Vec<FillDto>,
    pub settlement_tx: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FillsQuery {
    pub since_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BookQuery {
    pub depth: Option<u32>,
}

// ────────────────────────────────────────────────────────────────────────────
// Errors
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("hand backend not configured on this solver — wire one via SolverApi::set_hand_backend(...)")]
    NotConfigured,
    #[error("venue rejected: {0}")]
    Rejected(String),
    #[error("unknown venue: {0}")]
    UnknownVenue(String),
    #[error("unknown order id: {0}")]
    UnknownOrder(String),
    #[error("unsupported capability for venue {0}")]
    Unsupported(String),
    #[error("network: {0}")]
    Network(String),
    #[error("internal: {0}")]
    Internal(String),
}

impl IntoResponse for BackendError {
    fn into_response(self) -> Response {
        let status = match &self {
            BackendError::NotConfigured => StatusCode::SERVICE_UNAVAILABLE,
            BackendError::UnknownVenue(_) | BackendError::UnknownOrder(_) => StatusCode::NOT_FOUND,
            BackendError::Rejected(_) | BackendError::Unsupported(_) => StatusCode::BAD_REQUEST,
            BackendError::Network(_) | BackendError::Internal(_) => StatusCode::BAD_GATEWAY,
        };
        let body = serde_json::json!({ "error": self.to_string() });
        (status, Json(body)).into_response()
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Router builder
// ────────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct HandRouterState {
    pub backend: Arc<dyn HandBackend>,
}

pub fn router(backend: Arc<dyn HandBackend>) -> Router {
    let state = HandRouterState { backend };
    Router::new()
        // read
        .route("/api/hand/status",                       get(status_handler))
        .route("/api/hand/venues",                       get(venues_handler))
        .route("/api/hand/balances",                     get(balances_all_handler))
        .route("/api/hand/positions",                    get(positions_all_handler))
        .route("/api/hand/venues/:venue/balances",       get(balances_venue_handler))
        .route("/api/hand/venues/:venue/positions",      get(positions_venue_handler))
        .route("/api/hand/orders",                       get(open_orders_handler).post(place_handler).delete(cancel_all_handler))
        .route("/api/hand/orders/:id",                   get(order_handler).delete(cancel_handler))
        .route("/api/hand/orders/:id/replace",           post(replace_handler))
        .route("/api/hand/fills",                        get(fills_handler))
        .route("/api/hand/book/:venue/:market",          get(book_handler))
        .route("/api/hand/ticker/:venue/:market",        get(ticker_handler))
        .route("/api/hand/quote",                        post(quote_handler))
        .route("/api/hand/auction/:id/fill",             post(auction_fill_handler))
        .with_state(state)
}

// ────────────────────────────────────────────────────────────────────────────
// Handlers — every one is a thin dispatcher onto the backend trait
// ────────────────────────────────────────────────────────────────────────────

async fn status_handler(State(s): State<HandRouterState>) -> Result<Json<HandStatusResponse>, BackendError> {
    s.backend.status().await.map(Json)
}

async fn venues_handler(State(s): State<HandRouterState>) -> Result<Json<Vec<VenueInfo>>, BackendError> {
    s.backend.venues().await.map(Json)
}

async fn balances_all_handler(State(s): State<HandRouterState>) -> Result<Json<Vec<BalanceDto>>, BackendError> {
    s.backend.balances(None).await.map(Json)
}

async fn balances_venue_handler(
    Path(venue): Path<String>,
    State(s): State<HandRouterState>,
) -> Result<Json<Vec<BalanceDto>>, BackendError> {
    s.backend.balances(Some(&venue)).await.map(Json)
}

async fn positions_all_handler(State(s): State<HandRouterState>) -> Result<Json<Vec<PositionDto>>, BackendError> {
    s.backend.positions(None).await.map(Json)
}

async fn positions_venue_handler(
    Path(venue): Path<String>,
    State(s): State<HandRouterState>,
) -> Result<Json<Vec<PositionDto>>, BackendError> {
    s.backend.positions(Some(&venue)).await.map(Json)
}

async fn open_orders_handler(
    Query(filter): Query<OrderFilter>,
    State(s): State<HandRouterState>,
) -> Result<Json<Vec<OrderDto>>, BackendError> {
    s.backend.open_orders(filter).await.map(Json)
}

async fn place_handler(
    State(s): State<HandRouterState>,
    Json(req): Json<PlaceOrderRequest>,
) -> Result<Json<OrderDto>, BackendError> {
    s.backend.place(req).await.map(Json)
}

async fn cancel_all_handler(
    Query(filter): Query<OrderFilter>,
    State(s): State<HandRouterState>,
) -> Result<Json<serde_json::Value>, BackendError> {
    let n = s.backend.cancel_all(filter).await?;
    Ok(Json(serde_json::json!({ "cancelled": n })))
}

async fn order_handler(
    Path(id): Path<String>,
    State(s): State<HandRouterState>,
) -> Result<Json<OrderDto>, BackendError> {
    s.backend.order(&id).await.map(Json)
}

async fn cancel_handler(
    Path(id): Path<String>,
    State(s): State<HandRouterState>,
) -> Result<StatusCode, BackendError> {
    s.backend.cancel(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn replace_handler(
    Path(id): Path<String>,
    State(s): State<HandRouterState>,
    Json(req): Json<PlaceOrderRequest>,
) -> Result<Json<OrderDto>, BackendError> {
    s.backend.replace(&id, req).await.map(Json)
}

async fn fills_handler(
    Query(q): Query<FillsQuery>,
    State(s): State<HandRouterState>,
) -> Result<Json<Vec<FillDto>>, BackendError> {
    s.backend.fills(q.since_ms).await.map(Json)
}

async fn book_handler(
    Path((venue, market)): Path<(String, String)>,
    Query(q): Query<BookQuery>,
    State(s): State<HandRouterState>,
) -> Result<Json<BookDto>, BackendError> {
    s.backend.book(&venue, &market, q.depth.unwrap_or(50)).await.map(Json)
}

async fn ticker_handler(
    Path((venue, market)): Path<(String, String)>,
    State(s): State<HandRouterState>,
) -> Result<Json<TickerDto>, BackendError> {
    s.backend.ticker(&venue, &market).await.map(Json)
}

async fn quote_handler(
    State(s): State<HandRouterState>,
    Json(req): Json<RfqRequest>,
) -> Result<Json<QuoteDto>, BackendError> {
    s.backend.quote(req).await.map(Json)
}

async fn auction_fill_handler(
    Path(_id): Path<String>,
    State(s): State<HandRouterState>,
    Json(req): Json<AuctionFillRequest>,
) -> Result<Json<FillReceiptDto>, BackendError> {
    s.backend.fill_auction(req).await.map(Json)
}
