//! Sanitized public probes; runtime detail remains a protected operational API.
use crate::handlers::AppState;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use convex_core::contracts::DataQuality;
use convex_engine::{RuntimeMode, RuntimePhase};
use serde::Serialize;
use std::sync::Arc;

/// Low-cardinality dependency state; no resource IDs or remote failure text.
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyState {
    /// Required evidence is valid.
    Ready,
    /// Required composition/evidence is absent.
    Missing,
    /// Runtime freshness policy failed.
    Stale,
    /// A composed dependency is presently unavailable or invalid.
    Unavailable,
}
/// One dependency assessment based on actual access/runtime status.
#[derive(Debug, Serialize)]
pub struct DependencyHealth {
    /// Stable dependency class.
    pub name: &'static str,
    /// Required to serve analytics.
    pub required: bool,
    /// Current assessment.
    pub state: DependencyState,
}
/// Public analytics readiness, distinct from current live-data and strategy authorization.
#[derive(Debug, Serialize)]
pub struct Readiness {
    /// Combined required dependencies.
    pub ready: bool,
    /// Current owned lifecycle.
    pub phase: RuntimePhase,
    /// Historical/captured/live mode; historical validity is not live freshness.
    pub mode: Option<RuntimeMode>,
    /// Actual snapshot quality.
    pub quality: DataQuality,
    /// True only when runtime verifies its selected dependencies and acknowledged output.
    pub runtime_ready: bool,
    /// This server has no composed trading controls.
    pub strategy_ready: bool,
    /// Explicit dependency assessment.
    pub dependencies: Vec<DependencyHealth>,
}
async fn inspect(state: &AppState) -> Readiness {
    let runtime = state.engine.runtime_health();
    let access = state.access.health().await;
    let ready = runtime.ready && access.policy_ready && access.identity_ready;
    let missing_runtime = matches!(runtime.phase, RuntimePhase::New | RuntimePhase::Demo);
    let dependency = |name, required, valid| DependencyHealth {
        name,
        required,
        state: if valid {
            DependencyState::Ready
        } else {
            DependencyState::Unavailable
        },
    };
    Readiness {
        ready,
        phase: runtime.phase,
        mode: runtime.mode,
        quality: runtime.quality,
        runtime_ready: runtime.ready,
        strategy_ready: false,
        dependencies: vec![
            DependencyHealth {
                name: "runtime_dependencies",
                required: true,
                state: if runtime.ready {
                    DependencyState::Ready
                } else if missing_runtime {
                    DependencyState::Missing
                } else {
                    DependencyState::Unavailable
                },
            },
            DependencyHealth {
                name: "market_context",
                required: true,
                state: if runtime.quality == DataQuality::Valid {
                    DependencyState::Ready
                } else if runtime.quality == DataQuality::Stale {
                    DependencyState::Stale
                } else if missing_runtime {
                    DependencyState::Missing
                } else {
                    DependencyState::Unavailable
                },
            },
            dependency(
                "result_delivery",
                true,
                runtime.pending_publications == 0
                    && !matches!(
                        runtime.phase,
                        RuntimePhase::New
                            | RuntimePhase::Starting
                            | RuntimePhase::Failed
                            | RuntimePhase::Demo
                    ),
            ),
            dependency("access_policy", access.required, access.policy_ready),
            dependency("identity_provider", access.required, access.identity_ready),
        ],
    }
}
/// Process liveness is independent of required dependency failures.
pub async fn readiness(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let view = inspect(&state).await;
    (
        if view.ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(view),
    )
}
/// Protected dependency detail uses the same readiness calculation.
pub async fn dependencies(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(inspect(&state).await)
}
/// Fail-closed strategy probe. No risk/ownership/kill/execution subsystem is composed yet.
pub async fn strategy_readiness(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let view = inspect(&state).await;
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(
            serde_json::json!({"ready":false,"analytics_ready":view.ready,"live_inputs_ready":view.ready && view.mode == Some(RuntimeMode::Live),"risk_reservations_ready":false,"ownership_fenced":false,"execution_reconciled":false,"kill_controls_ready":false}),
        ),
    )
}
