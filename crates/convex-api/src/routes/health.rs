//! Health check endpoints.

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};

use crate::state::AppState;

/// Health check response.
#[derive(Debug, Serialize, Deserialize)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum_test::TestServer;

    use crate::server::create_router;

    #[tokio::test]
    async fn test_health_check() {
        let state = AppState::new();
        let router = create_router(state);
        let server = TestServer::new(router).unwrap();

        let response = server.get("/health").await;
        response.assert_status_ok();

        let body: HealthResponse = response.json();
        assert_eq!(body.status, "healthy");
        assert!(!body.demo_mode);
        assert_eq!(body.bonds_count, 0);
        assert_eq!(body.curves_count, 0);
    }

    #[tokio::test]
    async fn test_health_check_demo_mode() {
        let state = AppState::with_demo_mode();
        let router = create_router(state);
        let server = TestServer::new(router).unwrap();

        let response = server.get("/health").await;
        response.assert_status_ok();

        let body: HealthResponse = response.json();
        assert_eq!(body.status, "healthy");
        assert!(body.demo_mode);
        assert!(body.bonds_count >= 4);
        assert!(body.curves_count >= 2);
    }
}
