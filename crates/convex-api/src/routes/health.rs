//! Health check endpoints.

use axum::{extract::State, Json};
use serde::Serialize;

use crate::state::AppState;

/// Health check response.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub demo_mode: bool,
    pub bonds_count: usize,
    pub curves_count: usize,
}

/// Health check endpoint.
pub async fn health_check(State(state): State<AppState>) -> Json<HealthResponse> {
    let bonds_count = state.bonds.read().unwrap().len();
    let curves_count = state.curves.read().unwrap().len();

    Json(HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        demo_mode: state.demo_mode,
        bonds_count,
        curves_count,
    })
}
