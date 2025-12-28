//! API error types.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use thiserror::Error;

/// API error type.
#[derive(Debug, Error)]
pub enum ApiError {
    /// Resource not found.
    #[error("Not found: {0}")]
    NotFound(String),

    /// Bad request (invalid input).
    #[error("Bad request: {0}")]
    BadRequest(String),

    /// Validation error.
    #[error("Validation error: {0}")]
    Validation(String),

    /// Calculation failed.
    #[error("Calculation failed: {0}")]
    CalculationFailed(String),

    /// Internal server error.
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Error response body.
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: ErrorBody,
}

/// Error body details.
#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            ApiError::NotFound(_) => (StatusCode::NOT_FOUND, "NOT_FOUND"),
            ApiError::BadRequest(_) => (StatusCode::BAD_REQUEST, "BAD_REQUEST"),
            ApiError::Validation(_) => (StatusCode::UNPROCESSABLE_ENTITY, "VALIDATION_ERROR"),
            ApiError::CalculationFailed(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "CALCULATION_ERROR")
            }
            ApiError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
        };

        let body = Json(ErrorResponse {
            error: ErrorBody {
                code: code.to_string(),
                message: self.to_string(),
            },
        });

        (status, body).into_response()
    }
}

// Conversions from domain errors
impl From<convex_bonds::BondError> for ApiError {
    fn from(err: convex_bonds::BondError) -> Self {
        ApiError::BadRequest(err.to_string())
    }
}

impl From<convex_curves::CurveError> for ApiError {
    fn from(err: convex_curves::CurveError) -> Self {
        ApiError::BadRequest(err.to_string())
    }
}

impl From<convex_analytics::AnalyticsError> for ApiError {
    fn from(err: convex_analytics::AnalyticsError) -> Self {
        ApiError::CalculationFailed(err.to_string())
    }
}

impl From<convex_core::ConvexError> for ApiError {
    fn from(err: convex_core::ConvexError) -> Self {
        ApiError::BadRequest(err.to_string())
    }
}

/// Result type for API operations.
pub type ApiResult<T> = Result<T, ApiError>;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

    #[test]
    fn test_api_error_not_found() {
        let err = ApiError::NotFound("Bond 'XYZ' not found".to_string());
        assert!(err.to_string().contains("Bond 'XYZ' not found"));
    }

    #[test]
    fn test_api_error_bad_request() {
        let err = ApiError::BadRequest("Invalid coupon".to_string());
        assert!(err.to_string().contains("Invalid coupon"));
    }

    #[test]
    fn test_api_error_validation() {
        let err = ApiError::Validation("Date out of range".to_string());
        assert!(err.to_string().contains("Date out of range"));
    }

    #[test]
    fn test_api_error_calculation_failed() {
        let err = ApiError::CalculationFailed("Convergence failed".to_string());
        assert!(err.to_string().contains("Convergence failed"));
    }

    #[test]
    fn test_api_error_internal() {
        let err = ApiError::Internal("Database error".to_string());
        assert!(err.to_string().contains("Database error"));
    }

    #[tokio::test]
    async fn test_not_found_response_status() {
        let err = ApiError::NotFound("Test".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_bad_request_response_status() {
        let err = ApiError::BadRequest("Test".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_validation_response_status() {
        let err = ApiError::Validation("Test".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn test_calculation_failed_response_status() {
        let err = ApiError::CalculationFailed("Test".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_internal_response_status() {
        let err = ApiError::Internal("Test".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
