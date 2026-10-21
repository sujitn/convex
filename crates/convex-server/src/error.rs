//! One HTTP error shape: `{"error", "code", "retryable"}` with a typed status.
//! Curve and quote handlers use it; other handlers migrate to it over time.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use convex_engine::curve_resolution::CurveResolutionError;
use convex_engine::ports::reference_repository::ReferenceError;
use convex_engine::ports::TraitError;

/// A refused request. Nothing partial or substituted accompanies it.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
    retryable: bool,
}

impl ApiError {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            retryable: false,
        }
    }
    /// Malformed or out-of-bounds request.
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "INVALID_INPUT", message)
    }
    /// The addressed record does not exist at the cutoff.
    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "NOT_FOUND", message)
    }
    /// A required configuration or market input is absent or stale.
    pub(crate) fn missing_input(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, "MISSING_INPUT", message)
    }
    /// The calculation ran and failed on the supplied inputs.
    pub(crate) fn calculation(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "CALCULATION_FAILED",
            message,
        )
    }
    /// The response could not be produced; safe to retry.
    pub(crate) fn unavailable(message: impl Into<String>) -> Self {
        Self {
            retryable: true,
            ..Self::new(StatusCode::SERVICE_UNAVAILABLE, "UNAVAILABLE", message)
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({
                "error": self.message,
                "code": self.code,
                "retryable": self.retryable,
            })),
        )
            .into_response()
    }
}

impl From<TraitError> for ApiError {
    fn from(error: TraitError) -> Self {
        let message = error.to_string();
        match error {
            TraitError::InvalidInput(_) => Self::invalid(message),
            TraitError::Conflict(_) => Self::new(StatusCode::CONFLICT, "CONFLICT", message),
            TraitError::CapacityExceeded => {
                Self::new(StatusCode::PAYLOAD_TOO_LARGE, "CAPACITY_EXCEEDED", message)
            }
            TraitError::Unsupported(_) => {
                Self::new(StatusCode::UNPROCESSABLE_ENTITY, "UNSUPPORTED", message)
            }
            TraitError::UnknownOutcome => {
                Self::new(StatusCode::SERVICE_UNAVAILABLE, "UNKNOWN_OUTCOME", message)
            }
            _ => Self::unavailable(message),
        }
    }
}

impl From<CurveResolutionError> for ApiError {
    fn from(error: CurveResolutionError) -> Self {
        match error {
            CurveResolutionError::Port(error) => error.into(),
            other => Self::missing_input(other.to_string()),
        }
    }
}

impl From<ReferenceError> for ApiError {
    fn from(error: ReferenceError) -> Self {
        let message = error.to_string();
        match error {
            ReferenceError::InvalidInput(_) => Self::invalid(message),
            ReferenceError::Conflict(_) => {
                Self::new(StatusCode::CONFLICT, "VERSION_CONFLICT", message)
            }
            ReferenceError::AmbiguousIdentifier => {
                Self::new(StatusCode::CONFLICT, "AMBIGUOUS_IDENTIFIER", message)
            }
            ReferenceError::NotFound => {
                Self::new(StatusCode::NOT_FOUND, "MISSING_REFERENCE", message)
            }
            ReferenceError::Unavailable => Self::unavailable(message),
            ReferenceError::UnknownOutcome => {
                Self::new(StatusCode::SERVICE_UNAVAILABLE, "UNKNOWN_OUTCOME", message)
            }
            ReferenceError::CapacityExceeded | ReferenceError::Unsupported(_) => {
                Self::new(StatusCode::SERVICE_UNAVAILABLE, "UNAVAILABLE", message)
            }
        }
    }
}
