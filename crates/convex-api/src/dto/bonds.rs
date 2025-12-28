//! Bond DTOs.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::common::{CurrencyCode, DateInput, DayCountCode, FrequencyCode};

/// Request to create a fixed rate bond.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateBondRequest {
    /// Unique bond identifier.
    pub id: String,

    /// Annual coupon rate as percentage (e.g., 5.0 for 5%).
    pub coupon_rate: f64,

    /// Maturity date.
    pub maturity: DateInput,

    /// Issue date.
    pub issue_date: DateInput,

    /// Payment frequency (default: semi_annual).
    #[serde(default)]
    pub frequency: FrequencyCode,

    /// Day count convention (default: thirty360_us).
    #[serde(default)]
    pub day_count: DayCountCode,

    /// Currency (default: USD).
    #[serde(default)]
    pub currency: CurrencyCode,

    /// Face value (default: 100).
    #[serde(default = "default_face_value")]
    pub face_value: f64,
}

fn default_face_value() -> f64 {
    100.0
}

/// Bond summary response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BondResponse {
    pub id: String,
    pub bond_type: String,
    pub coupon_rate: f64,
    pub maturity: String,
    pub issue_date: String,
    pub frequency: String,
    pub currency: String,
    pub face_value: f64,
}

/// List of bonds response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BondListResponse {
    pub bonds: Vec<BondResponse>,
    pub count: usize,
}

/// Request to calculate yield from price.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CalculateYieldRequest {
    /// Settlement date.
    pub settlement: DateInput,

    /// Clean price.
    pub clean_price: f64,
}

/// Yield calculation response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct YieldResponse {
    pub bond_id: String,
    pub settlement: String,
    pub clean_price: f64,
    pub dirty_price: f64,
    pub accrued_interest: String,
    pub yield_to_maturity_pct: f64,
}

/// Request to calculate price from yield.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CalculatePriceRequest {
    /// Settlement date.
    pub settlement: DateInput,

    /// Yield to maturity as percentage (e.g., 4.5 for 4.5%).
    pub yield_pct: f64,
}

/// Price calculation response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PriceResponse {
    pub bond_id: String,
    pub settlement: String,
    pub yield_pct: f64,
    pub clean_price: f64,
    pub dirty_price: f64,
    pub accrued_interest: String,
}

/// Request for bond analytics.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AnalyticsRequest {
    /// Settlement date.
    pub settlement: DateInput,

    /// Clean price (optional if yield_pct provided).
    pub clean_price: Option<f64>,

    /// Yield as percentage (optional if clean_price provided).
    pub yield_pct: Option<f64>,
}

/// Bond analytics response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AnalyticsResponse {
    pub bond_id: String,
    pub settlement: String,
    pub yield_to_maturity_pct: f64,
    pub clean_price: f64,
    pub dirty_price: f64,
    pub accrued_interest: String,
    pub macaulay_duration: f64,
    pub modified_duration: f64,
    pub convexity: f64,
    pub dv01: f64,
}

/// Cashflow entry.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CashflowEntry {
    pub date: String,
    pub amount: f64,
    pub flow_type: String,
}

/// Cashflow response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CashflowResponse {
    pub bond_id: String,
    pub settlement: String,
    pub cashflows: Vec<CashflowEntry>,
}

/// Request for spread calculation.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SpreadRequest {
    /// Curve ID to use as benchmark.
    pub curve_id: String,

    /// Settlement date.
    pub settlement: DateInput,

    /// Clean price.
    pub clean_price: f64,
}

/// Spread calculation response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SpreadResponse {
    pub bond_id: String,
    pub curve_id: String,
    pub settlement: String,
    pub clean_price: f64,
    pub z_spread_bps: Option<f64>,
    pub i_spread_bps: Option<f64>,
    pub g_spread_bps: Option<f64>,
}
