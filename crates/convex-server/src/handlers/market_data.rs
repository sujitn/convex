//! Market data handlers for real-time updates.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use rust_decimal::Decimal;
use serde::Deserialize;

use convex_engine::curve_builder::BuiltCurve;
use convex_traits::ids::{CurveId, InstrumentId};
use convex_traits::market_data::CompositeQuote;

use crate::handlers::AppState;

/// Request for quote update.
#[derive(Debug, Deserialize)]
pub struct QuoteUpdateRequest {
    /// Instrument ID
    pub instrument_id: String,

    // Legacy fields (optional)
    pub bid: Option<Decimal>,
    pub ask: Option<Decimal>,

    // New composite fields
    pub bid_price: Option<Decimal>,
    pub ask_price: Option<Decimal>,
    pub mid_price: Option<Decimal>,

    pub bid_yield: Option<Decimal>,
    pub ask_yield: Option<Decimal>,
    pub mid_yield: Option<Decimal>,

    pub z_spread: Option<Decimal>,
    pub g_spread: Option<Decimal>,
    pub i_spread: Option<Decimal>,
}

/// Handle quote update.
pub async fn update_quote(
    State(state): State<Arc<AppState>>,
    Json(request): Json<QuoteUpdateRequest>,
) -> impl IntoResponse {
    let id = InstrumentId::new(&request.instrument_id);

    // Map request to CompositeQuote
    // Use legacy fields if new ones are missing
    let quote = CompositeQuote {
        bid_price: request.bid_price.or(request.bid),
        ask_price: request.ask_price.or(request.ask),
        mid_price: request.mid_price, // Or calc from bid/ask

        bid_yield: request.bid_yield,
        ask_yield: request.ask_yield,
        mid_yield: request.mid_yield,

        z_spread: request.z_spread,
        g_spread: request.g_spread,
        i_spread: request.i_spread,
        timestamp: chrono::Utc::now().timestamp(),
    };

    state.engine.on_quote_update(&id, quote);

    StatusCode::ACCEPTED
}

/// Request for curve update.
#[derive(Debug, Deserialize)]
pub struct CurveUpdateRequest {
    /// Curve ID
    pub curve_id: String,
    /// Curve data
    pub curve: BuiltCurve,
}

/// Handle curve update.
pub async fn update_curve(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CurveUpdateRequest>,
) -> impl IntoResponse {
    let id = CurveId::new(&request.curve_id);

    state.engine.on_curve_update(&id, &request.curve);

    StatusCode::ACCEPTED
}
