//! Supplied-input HTTP transport; calculation and validation stay in the library.
#![forbid(unsafe_code)]
#![deny(unused_must_use)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Extension,
};

use crate::{access::Admission, handlers::AppState};

#[cfg(test)]
#[path = "analytics_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;

/// Execute the original bounded JSON contract without altering financial digits.
pub(crate) async fn calculate(
    State(state): State<Arc<AppState>>,
    Extension(admission): Extension<Arc<Admission>>,
    bytes: Bytes,
) -> Response {
    let input = match String::from_utf8(bytes.to_vec()) {
        Ok(input) => input,
        Err(_) => return crate::access::http::response(crate::access::AccessError::InvalidRequest),
    };
    let permit = match state.analytics_slots.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => return crate::access::http::response(crate::access::AccessError::Quota),
    };
    // The permit and transport admission belong to the calculation, not its
    // waiting HTTP future, including when that future is cancelled.
    match tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let _admission = admission;
        convex::library::execute_json(&input)
    })
    .await
    {
        Ok(output) if output.len() <= crate::access::http::MAX_RESPONSE => {
            ([(header::CONTENT_TYPE, "application/json")], output).into_response()
        }
        Ok(_) => (
            StatusCode::PAYLOAD_TOO_LARGE,
            axum::Json(serde_json::json!({"code":"ANALYTICS_RESPONSE_TOO_LARGE"})),
        )
            .into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(serde_json::json!({"code":"ANALYTICS_WORKER_FAILED"})),
        )
            .into_response(),
    }
}
