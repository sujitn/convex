//! Curve DTOs.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::common::DateInput;

/// Interpolation method.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum InterpolationMethod {
    Linear,
    LogLinear,
    CubicSpline,
    #[default]
    MonotoneConvex,
}

impl From<InterpolationMethod> for convex_curves::InterpolationMethod {
    fn from(method: InterpolationMethod) -> Self {
        match method {
            InterpolationMethod::Linear => convex_curves::InterpolationMethod::Linear,
            InterpolationMethod::LogLinear => convex_curves::InterpolationMethod::LogLinear,
            InterpolationMethod::CubicSpline => convex_curves::InterpolationMethod::CubicSpline,
            InterpolationMethod::MonotoneConvex => convex_curves::InterpolationMethod::MonotoneConvex,
        }
    }
}

/// Request to create a curve from zero rates.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateCurveRequest {
    /// Unique curve identifier.
    pub id: String,

    /// Reference date.
    pub reference_date: DateInput,

    /// Tenor points in years.
    pub tenors: Vec<f64>,

    /// Zero rates as percentages (e.g., 4.5 for 4.5%).
    pub rates: Vec<f64>,

    /// Interpolation method (default: monotone_convex).
    #[serde(default)]
    pub interpolation: InterpolationMethod,
}

/// Curve summary response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CurveResponse {
    pub id: String,
    pub reference_date: String,
    pub tenor_count: usize,
    pub min_tenor: f64,
    pub max_tenor: f64,
}

/// List of curves response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CurveListResponse {
    pub curves: Vec<CurveResponse>,
    pub count: usize,
}

/// Curve point data.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CurvePoint {
    pub tenor: f64,
    pub zero_rate_pct: f64,
    pub discount_factor: f64,
}

/// Detailed curve response with points.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CurveDetailResponse {
    pub id: String,
    pub reference_date: String,
    pub interpolation: String,
    pub points: Vec<CurvePoint>,
}

/// Query parameters for zero rate.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ZeroRateQuery {
    /// Tenor in years.
    pub tenor: f64,
}

/// Query parameters for forward rate.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ForwardRateQuery {
    /// Start tenor in years.
    pub t1: f64,

    /// End tenor in years.
    pub t2: f64,
}

/// Query parameters for discount factor.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct DiscountFactorQuery {
    /// Tenor in years.
    pub tenor: f64,
}

/// Rate query response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RateQueryResponse {
    pub curve_id: String,
    pub tenor: f64,
    pub value: f64,
    pub value_type: String,
}

/// Bootstrap instrument.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BootstrapInstrument {
    /// Tenor in years.
    pub tenor: f64,

    /// Rate as percentage (e.g., 4.5 for 4.5%).
    pub rate: f64,
}

/// Bootstrap calibration method.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationMethod {
    #[default]
    Global,
    Sequential,
}

/// Request to bootstrap a curve.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BootstrapRequest {
    /// Unique curve identifier.
    pub id: String,

    /// Reference date.
    pub reference_date: DateInput,

    /// Deposit instruments.
    #[serde(default)]
    pub deposits: Vec<BootstrapInstrument>,

    /// Swap instruments.
    #[serde(default)]
    pub swaps: Vec<BootstrapInstrument>,

    /// OIS instruments.
    #[serde(default)]
    pub ois: Vec<BootstrapInstrument>,

    /// Calibration method (default: global).
    #[serde(default)]
    pub method: CalibrationMethod,
}

/// Bootstrap response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BootstrapResponse {
    pub id: String,
    pub reference_date: String,
    pub method: String,
    pub iterations: u32,
    pub rms_error: f64,
    pub tenor_count: usize,
}
