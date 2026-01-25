pub mod config;
pub mod market_data;
pub mod general;

pub use general::*;

use std::sync::Arc;

use convex_engine::PricingEngine;
use crate::websocket::WebSocketState;

/// Application state.
pub struct AppState {
    /// The pricing engine
    pub engine: Arc<PricingEngine>,
    /// WebSocket state for real-time streaming
    pub ws_state: WebSocketState,
}
