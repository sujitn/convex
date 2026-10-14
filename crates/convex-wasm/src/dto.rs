//! Serde-derived input/output types crossing the JS/Rust boundary.

use serde::{Deserialize, Serialize};

/// Bond parameters for creating a fixed coupon bond.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BondParams {
    /// Coupon rate as percentage (e.g., 5.0 for 5%)
    pub coupon_rate: f64,
    /// Maturity date as "YYYY-MM-DD"
    pub maturity_date: String,
    /// Issue date as "YYYY-MM-DD"
    pub issue_date: String,
    /// Settlement date as "YYYY-MM-DD"
    pub settlement_date: String,
    /// Absolute currency face value (default100); price/risk/accrued outputs are per100.
    pub face_value: Option<f64>,
    /// Coupon frequency: 1=annual, 2=semi-annual, 4=quarterly, 12=monthly
    pub frequency: Option<u32>,
    /// Day count convention:
    /// - "30/360" or "30/360 US" - US (NASD) method
    /// - "30E/360" or "30/360 EU" - European (ISMA) method
    /// - "ACT/360" - Actual/360
    /// - "ACT/365" - Actual/365 Fixed
    /// - "ACT/ACT" - Actual/Actual ISDA; ICMA must be explicit
    pub day_count: Option<String>,
    /// Currency: "USD", "EUR", "GBP", etc.
    pub currency: Option<String>,
    /// First coupon date as "YYYY-MM-DD" (optional)
    pub first_coupon_date: Option<String>,
    // === New convention parameters ===
    /// Market: "US", "UK", "Germany", "France", "Italy", "Japan", etc.
    pub market: Option<String>,
    /// Instrument type: "GovernmentBond", "Corporate", "Municipal", "Agency", etc.
    pub instrument_type: Option<String>,
    /// Yield convention: "Street", "True", "ISMA", "Japanese", "USMunicipal", etc.
    pub yield_convention: Option<String>,
    /// Compounding method: "SemiAnnual", "Annual", "Quarterly", "Continuous", "Simple"
    pub compounding: Option<String>,
    /// Settlement days (T+N)
    pub settlement_days: Option<u32>,
    /// Ex-dividend days before coupon
    pub ex_dividend_days: Option<u32>,
    /// Whether this market uses business days for settlement
    pub use_business_days: Option<bool>,
}

/// Analysis results returned from bond calculations.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnalysisResult {
    // Price metrics
    pub clean_price: Option<f64>,
    pub dirty_price: Option<f64>,
    pub accrued_interest: Option<f64>,

    // Yield metrics
    pub ytm: Option<f64>,
    pub current_yield: Option<f64>,
    pub simple_yield: Option<f64>,
    pub money_market_yield: Option<f64>,

    // Risk metrics
    pub modified_duration: Option<f64>,
    pub macaulay_duration: Option<f64>,
    pub convexity: Option<f64>,
    pub dv01: Option<f64>,

    // Spread metrics (in basis points)
    pub z_spread: Option<f64>,
    pub asw_spread: Option<f64>,

    // Additional info
    pub days_to_maturity: Option<i64>,
    pub years_to_maturity: Option<f64>,

    // Convention info (returned for display)
    pub market: Option<String>,
    pub instrument_type: Option<String>,
    pub yield_convention: Option<String>,
    pub compounding_method: Option<String>,
    pub settlement_days: Option<u32>,
    pub ex_dividend_days: Option<u32>,
    pub is_ex_dividend: Option<bool>,

    // Error message if calculation failed
    pub error: Option<String>,
}

/// Projected cashflow entry; amount is absolute bond-currency cash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CashFlowEntry {
    pub date: String,
    pub amount: f64,
    pub cf_type: String, // "coupon", "principal", "coupon_and_principal"
}

/// Curve point for yield curve construction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurvePoint {
    /// Date as "YYYY-MM-DD"
    pub date: String,
    /// Rate as percentage (e.g., 4.5 for 4.5%)
    pub rate: f64,
}

/// Result from price-from-yield / price-from-spread calculations.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PriceFromYieldResult {
    pub clean_price: Option<f64>,
    pub dirty_price: Option<f64>,
    pub accrued_interest: Option<f64>,
    pub error: Option<String>,
}

/// Available convention options for UI dropdowns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConventionOptions {
    pub markets: Vec<ConventionOption>,
    pub instrument_types: Vec<ConventionOption>,
    pub yield_conventions: Vec<ConventionOption>,
    pub compounding_methods: Vec<ConventionOption>,
}

/// Single option for dropdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConventionOption {
    pub value: String,
    pub label: String,
}

/// Default convention values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefaultConventions {
    pub day_count: String,
    pub yield_convention: String,
    pub compounding: String,
    pub settlement_days: u32,
    pub ex_dividend_days: Option<u32>,
    pub use_business_days: bool,
}
