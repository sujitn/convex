//! Configuration management handlers (Admin API).

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;

use convex_traits::ids::{CurveId, InstrumentId};
use convex_traits::storage::{BondPricingConfig, CurveConfig, PriceOverride};

use crate::handlers::AppState;

// =============================================================================
// BOND PRICING CONFIGS
// =============================================================================

/// List bond pricing configs.
pub async fn list_bond_configs(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.engine.storage().configs.list().await {
        Ok(configs) => (StatusCode::OK, Json(configs)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    }
}

/// Create/Update bond pricing config.
pub async fn create_bond_config(
    State(state): State<Arc<AppState>>,
    Json(config): Json<BondPricingConfig>,
) -> impl IntoResponse {
    match state.engine.storage().configs.save(&config).await {
        Ok(_) => {
            // Trigger resolver reload if possible, or wait for polling
            // Ideally we'd have a notification mechanism.
            // For now, let's force a reload on the engine's resolver.
            let _ = state.engine.config_resolver().reload().await;
            (StatusCode::OK, Json(config)).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    }
}

/// Get bond pricing config.
pub async fn get_bond_config(
    State(state): State<Arc<AppState>>,
    Path(config_id): Path<String>,
) -> impl IntoResponse {
    match state.engine.storage().configs.get(&config_id).await {
        Ok(Some(config)) => (StatusCode::OK, Json(config)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Config not found" })),
        ).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    }
}

/// Delete bond pricing config.
pub async fn delete_bond_config(
    State(state): State<Arc<AppState>>,
    Path(config_id): Path<String>,
) -> impl IntoResponse {
    match state.engine.storage().configs.delete(&config_id).await {
        Ok(true) => {
            let _ = state.engine.config_resolver().reload().await;
            (StatusCode::NO_CONTENT, Json(serde_json::json!({}))).into_response()
        },
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Config not found" })),
        ).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    }
}

// =============================================================================
// CURVE CONFIGS
// =============================================================================

/// List curve configs.
pub async fn list_curve_configs(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.engine.storage().curves.list_configs().await {
        Ok(configs) => (StatusCode::OK, Json(configs)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    }
}

/// Create/Update curve config.
pub async fn create_curve_config(
    State(state): State<Arc<AppState>>,
    Json(config): Json<CurveConfig>,
) -> impl IntoResponse {
    match state.engine.storage().curves.save_config(&config).await {
        Ok(_) => (StatusCode::OK, Json(config)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    }
}

/// Get curve config.
pub async fn get_curve_config(
    State(state): State<Arc<AppState>>,
    Path(curve_id): Path<String>,
) -> impl IntoResponse {
    let id = CurveId::new(curve_id);
    match state.engine.storage().curves.get_config(&id).await {
        Ok(Some(config)) => (StatusCode::OK, Json(config)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Curve config not found" })),
        ).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    }
}

/// Delete curve config.
pub async fn delete_curve_config(
    State(state): State<Arc<AppState>>,
    Path(curve_id): Path<String>,
) -> impl IntoResponse {
    let id = CurveId::new(curve_id);
    match state.engine.storage().curves.delete_config(&id).await {
        Ok(true) => (StatusCode::NO_CONTENT, Json(serde_json::json!({}))).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Curve config not found" })),
        ).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    }
}

// =============================================================================
// PRICE OVERRIDES
// =============================================================================

/// List active overrides.
pub async fn list_overrides(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.engine.storage().overrides.get_active().await {
        Ok(overrides) => (StatusCode::OK, Json(overrides)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    }
}

/// Create/Update override.
pub async fn create_override(
    State(state): State<Arc<AppState>>,
    Json(override_): Json<PriceOverride>,
) -> impl IntoResponse {
    match state.engine.storage().overrides.save(&override_).await {
        Ok(_) => {
            // Reprice the bond immediately to reflect override
            let _ = state.engine.reprice_bond(&override_.instrument_id).await;
            (StatusCode::OK, Json(override_)).into_response()
        },
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    }
}

/// Get override.
pub async fn get_override(
    State(state): State<Arc<AppState>>,
    Path(instrument_id): Path<String>,
) -> impl IntoResponse {
    let id = InstrumentId::new(instrument_id);
    match state.engine.storage().overrides.get(&id).await {
        Ok(Some(override_)) => (StatusCode::OK, Json(override_)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Override not found" })),
        ).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    }
}

/// Delete override.
pub async fn delete_override(
    State(state): State<Arc<AppState>>,
    Path(instrument_id): Path<String>,
) -> impl IntoResponse {
    let id = InstrumentId::new(instrument_id);
    match state.engine.storage().overrides.delete(&id).await {
        Ok(true) => {
            // Reprice the bond immediately to remove override effect
            let _ = state.engine.reprice_bond(&id).await;
            (StatusCode::NO_CONTENT, Json(serde_json::json!({}))).into_response()
        },
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Override not found" })),
        ).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ).into_response(),
    }
}
