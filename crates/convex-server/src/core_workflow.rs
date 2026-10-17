//! HTTP transport for the existing-port core snapshot use cases.
#![forbid(unsafe_code)]
#![deny(
    unused_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]
use crate::{access::Admission, handlers::AppState};
use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use convex_core::contracts::SnapshotId;
use convex_engine::core_workflow::{CoreWorkflow, CoreWorkflowError};
use convex_engine::ports::{
    core_workflow::CoreSnapshotPlan, fixing::PublishedFixing, snapshot::SnapshotCurveRecipe,
    TraitError,
};
use std::sync::Arc;

type Service = Option<Extension<Arc<CoreWorkflow>>>;
fn service(value: Service) -> Result<Arc<CoreWorkflow>, (StatusCode, Json<serde_json::Value>)> {
    value.map(|Extension(v)| v).ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"code":"CORE_NOT_CONFIGURED"})),
        )
    })
}
fn failure(error: CoreWorkflowError) -> Response {
    match &error {
        CoreWorkflowError::SnapshotUnknown(id) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"code":"UNKNOWN_OUTCOME","snapshot_id":id})),
            )
                .into_response()
        }
        CoreWorkflowError::ResultUnknown(id) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"code":"UNKNOWN_OUTCOME","result_id":id})),
            )
                .into_response()
        }
        _ => {}
    }
    let (status, code) = match error {
        CoreWorkflowError::NotFound => (StatusCode::NOT_FOUND, "CORE_RECORD_NOT_FOUND"),
        CoreWorkflowError::Port(TraitError::Conflict(_)) => (StatusCode::CONFLICT, "CORE_CONFLICT"),
        CoreWorkflowError::Port(TraitError::InvalidInput(_)) => {
            (StatusCode::BAD_REQUEST, "INVALID_CORE_INPUT")
        }
        CoreWorkflowError::Port(TraitError::CapacityExceeded) => {
            (StatusCode::PAYLOAD_TOO_LARGE, "CORE_CAPACITY_EXCEEDED")
        }
        CoreWorkflowError::Port(TraitError::Unsupported(_)) => {
            (StatusCode::UNPROCESSABLE_ENTITY, "CORE_UNSUPPORTED")
        }
        CoreWorkflowError::Port(TraitError::UnknownOutcome) => {
            (StatusCode::SERVICE_UNAVAILABLE, "UNKNOWN_OUTCOME")
        }
        _ => (StatusCode::SERVICE_UNAVAILABLE, "CORE_UNAVAILABLE"),
    };
    (status, Json(serde_json::json!({"code":code}))).into_response()
}
async fn admitted(
    state: Arc<AppState>,
    admission: Arc<Admission>,
    work: impl std::future::Future<Output = Result<Response, CoreWorkflowError>> + Send + 'static,
) -> Response {
    let permit = match state.analytics_slots.clone().try_acquire_owned() {
        Ok(p) => p,
        Err(_) => return crate::access::http::response(crate::access::AccessError::Quota),
    };
    // Caller cancellation cannot release admission or interrupt the durable
    // result commit after its blocking calculation has already started.
    match tokio::spawn(async move {
        let _permit = permit;
        let _admission = admission;
        work.await
    })
    .await
    {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => failure(e),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"code":"CORE_WORKER_FAILED"})),
        )
            .into_response(),
    }
}
pub(crate) async fn save_curve(core: Service, Json(recipe): Json<SnapshotCurveRecipe>) -> Response {
    let core = match service(core) {
        Ok(v) => v,
        Err(r) => return r.into_response(),
    };
    match core.curves.save_recipe(&recipe).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => failure(e.into()),
    }
}
pub(crate) async fn save_fixings(
    core: Service,
    Json(values): Json<Vec<PublishedFixing>>,
) -> Response {
    let core = match service(core) {
        Ok(v) => v,
        Err(r) => return r.into_response(),
    };
    match core.fixings.publish(&values).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => failure(e.into()),
    }
}
pub(crate) async fn assemble(
    State(state): State<Arc<AppState>>,
    Extension(admission): Extension<Arc<Admission>>,
    core: Service,
    Json(plan): Json<CoreSnapshotPlan>,
) -> Response {
    let core = match service(core) {
        Ok(v) => v,
        Err(r) => return r.into_response(),
    };
    admitted(state, admission, async move {
        let snapshot = core.assemble(plan).await?;
        Ok(Json(serde_json::json!({"snapshot_id":snapshot.id})).into_response())
    })
    .await
}
pub(crate) async fn snapshot(core: Service, Path(id): Path<String>) -> Response {
    let core = match service(core) {
        Ok(v) => v,
        Err(r) => return r.into_response(),
    };
    let id = match SnapshotId::new(id) {
        Ok(v) => v,
        Err(_) => return failure(TraitError::InvalidInput("snapshot identity").into()),
    };
    match core.snapshot(&id).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => failure(e),
    }
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RunOptions {
    #[serde(default)]
    recompute: bool,
}
pub(crate) async fn run(
    State(state): State<Arc<AppState>>,
    Extension(admission): Extension<Arc<Admission>>,
    core: Service,
    Path((id, name)): Path<(String, String)>,
    Json(options): Json<RunOptions>,
) -> Response {
    let core = match service(core) {
        Ok(v) => v,
        Err(r) => return r.into_response(),
    };
    let id = match SnapshotId::new(id) {
        Ok(v) => v,
        Err(_) => return failure(TraitError::InvalidInput("snapshot identity").into()),
    };
    admitted(state, admission, async move {
        Ok(Json(core.run(id, name, options.recompute).await?).into_response())
    })
    .await
}
pub(crate) async fn result(core: Service, Path(id): Path<String>) -> Response {
    let core = match service(core) {
        Ok(v) => v,
        Err(r) => return r.into_response(),
    };
    match core.result(&id).await {
        Ok(v) => ([(header::CONTENT_TYPE, "application/json")], v.output).into_response(),
        Err(e) => failure(e),
    }
}
