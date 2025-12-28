//! Analytics DTOs.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::common::DateInput;

/// Batch yield calculation request.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct BatchYieldRequest {
    /// List of bond IDs.
    pub bond_ids: Vec<String>,

    /// Settlement date.
    pub settlement: DateInput,

    /// Clean prices (one per bond).
    pub clean_prices: Vec<f64>,
}

/// Batch yield result for a single bond.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BatchYieldResult {
    pub bond_id: String,
    pub clean_price: f64,
    pub yield_to_maturity_pct: Option<f64>,
    pub error: Option<String>,
}

/// Batch yield response.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BatchYieldResponse {
    pub settlement: String,
    pub results: Vec<BatchYieldResult>,
    pub success_count: usize,
    pub error_count: usize,
}

/// Batch analytics request.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct BatchAnalyticsRequest {
    /// List of bond IDs.
    pub bond_ids: Vec<String>,

    /// Settlement date.
    pub settlement: DateInput,

    /// Clean prices (one per bond).
    pub clean_prices: Vec<f64>,
}

/// Batch analytics result for a single bond.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BatchAnalyticsResult {
    pub bond_id: String,
    pub yield_to_maturity_pct: Option<f64>,
    pub modified_duration: Option<f64>,
    pub convexity: Option<f64>,
    pub dv01: Option<f64>,
    pub error: Option<String>,
}

/// Batch analytics response.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BatchAnalyticsResponse {
    pub settlement: String,
    pub results: Vec<BatchAnalyticsResult>,
    pub success_count: usize,
    pub error_count: usize,
}
