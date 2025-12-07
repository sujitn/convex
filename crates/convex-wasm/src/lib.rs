//! WebAssembly bindings for Convex fixed income analytics.
//!
//! This crate provides WASM bindings for the Convex library, enabling
//! Bloomberg YAS-equivalent bond analytics in web browsers.

use wasm_bindgen::prelude::*;
use serde::{Deserialize, Serialize};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

use convex_core::types::{Date, Frequency, Currency};
use convex_core::daycounts::DayCountConvention;
use convex_core::calendars::BusinessDayConvention;
use convex_bonds::{FixedRateBond, FixedRateBondBuilder};
use convex_bonds::traits::{Bond, FixedCouponBond};
use convex_bonds::prelude::BondIdentifiers;
use convex_curves::{ZeroCurve, ZeroCurveBuilder};
use convex_curves::interpolation::InterpolationMethod;
use convex_yas::YASCalculator;

// ============================================================================
// Initialization
// ============================================================================

/// Initialize the WASM module (sets up panic hook for better error messages).
#[wasm_bindgen(start)]
pub fn init() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

// ============================================================================
// Input/Output Types
// ============================================================================

/// Bond parameters for creating a fixed coupon bond.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondParams {
    /// Coupon rate as percentage (e.g., 5.0 for 5%)
    pub coupon_rate: f64,
    /// Maturity date as "YYYY-MM-DD"
    pub maturity_date: String,
    /// Issue date as "YYYY-MM-DD"
    pub issue_date: String,
    /// Settlement date as "YYYY-MM-DD"
    pub settlement_date: String,
    /// Face value (default 100)
    pub face_value: Option<f64>,
    /// Coupon frequency: 1=annual, 2=semi-annual, 4=quarterly, 12=monthly
    pub frequency: Option<u32>,
    /// Day count convention: "30/360", "ACT/360", "ACT/365", "ACT/ACT"
    pub day_count: Option<String>,
    /// Currency: "USD", "EUR", "GBP", etc.
    pub currency: Option<String>,
    /// First coupon date as "YYYY-MM-DD" (optional)
    pub first_coupon_date: Option<String>,
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
    pub g_spread: Option<f64>,
    pub z_spread: Option<f64>,
    pub asw_spread: Option<f64>,

    // Additional info
    pub days_to_maturity: Option<i64>,
    pub years_to_maturity: Option<f64>,

    // Error message if calculation failed
    pub error: Option<String>,
}

/// Cash flow entry.
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

// ============================================================================
// Helper Functions
// ============================================================================

fn parse_date(s: &str) -> Result<Date, String> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3 {
        return Err(format!("Invalid date format: {}. Expected YYYY-MM-DD", s));
    }

    let year: i32 = parts[0].parse().map_err(|_| format!("Invalid year: {}", parts[0]))?;
    let month: u32 = parts[1].parse().map_err(|_| format!("Invalid month: {}", parts[1]))?;
    let day: u32 = parts[2].parse().map_err(|_| format!("Invalid day: {}", parts[2]))?;

    Date::from_ymd(year, month, day)
        .map_err(|e| format!("Invalid date {}: {:?}", s, e))
}

fn date_to_naive(date: Date) -> chrono::NaiveDate {
    // Date can be converted to NaiveDate via Into trait
    date.into()
}

fn parse_day_count(s: &str) -> DayCountConvention {
    match s.to_uppercase().as_str() {
        "30/360" | "30_360" | "THIRTY_360" => DayCountConvention::Thirty360US,
        "ACT/360" | "ACT_360" | "ACTUAL_360" => DayCountConvention::Act360,
        "ACT/365" | "ACT_365" | "ACTUAL_365" => DayCountConvention::Act365Fixed,
        "ACT/ACT" | "ACT_ACT" | "ACTUAL_ACTUAL" => DayCountConvention::ActActIcma,
        _ => DayCountConvention::Thirty360US, // Default for US bonds
    }
}

fn parse_frequency(f: u32) -> Frequency {
    match f {
        1 => Frequency::Annual,
        2 => Frequency::SemiAnnual,
        4 => Frequency::Quarterly,
        12 => Frequency::Monthly,
        _ => Frequency::SemiAnnual, // Default
    }
}

fn parse_currency(s: &str) -> Currency {
    match s.to_uppercase().as_str() {
        "USD" => Currency::USD,
        "EUR" => Currency::EUR,
        "GBP" => Currency::GBP,
        "JPY" => Currency::JPY,
        "CHF" => Currency::CHF,
        "AUD" => Currency::AUD,
        "CAD" => Currency::CAD,
        "NZD" => Currency::NZD,
        _ => Currency::USD, // Default
    }
}

fn decimal_to_f64(d: Decimal) -> f64 {
    d.to_f64().unwrap_or(0.0)
}

fn f64_to_decimal(f: f64) -> Decimal {
    Decimal::from_f64_retain(f).unwrap_or(Decimal::ZERO)
}

// ============================================================================
// Core WASM Functions
// ============================================================================

/// Calculate bond analytics given price and yield curve.
///
/// Takes bond parameters, a clean price, and curve points, returns comprehensive analytics.
#[wasm_bindgen]
pub fn analyze_bond(params: JsValue, clean_price: f64, curve_points: JsValue) -> JsValue {
    let result = analyze_bond_impl(params, clean_price, curve_points);
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}

fn analyze_bond_impl(params: JsValue, clean_price: f64, curve_points: JsValue) -> AnalysisResult {
    // Parse parameters
    let bond_params: BondParams = match serde_wasm_bindgen::from_value(params) {
        Ok(p) => p,
        Err(e) => return AnalysisResult {
            error: Some(format!("Failed to parse bond parameters: {:?}", e)),
            ..Default::default()
        },
    };

    // Parse curve points
    let points: Vec<CurvePoint> = match serde_wasm_bindgen::from_value(curve_points) {
        Ok(p) => p,
        Err(e) => return AnalysisResult {
            error: Some(format!("Failed to parse curve points: {:?}", e)),
            ..Default::default()
        },
    };

    // Build the bond
    let bond = match create_bond(&bond_params) {
        Ok(b) => b,
        Err(e) => return AnalysisResult {
            error: Some(e),
            ..Default::default()
        },
    };

    // Parse settlement date
    let settlement = match parse_date(&bond_params.settlement_date) {
        Ok(d) => d,
        Err(e) => return AnalysisResult {
            error: Some(e),
            ..Default::default()
        },
    };

    // Build the curve
    let curve = match create_curve(settlement, &points) {
        Ok(c) => c,
        Err(e) => return AnalysisResult {
            error: Some(e),
            ..Default::default()
        },
    };

    // Create calculator and analyze
    let calculator = YASCalculator::new(&curve);
    let settlement_naive = date_to_naive(settlement);

    match calculator.analyze(&bond, settlement_naive, f64_to_decimal(clean_price)) {
        Ok(result) => convert_yas_result(&result, &bond, settlement),
        Err(e) => AnalysisResult {
            error: Some(format!("Analysis failed: {:?}", e)),
            ..Default::default()
        },
    }
}

/// Get bond cash flows.
///
/// Returns all future cash flows from settlement date.
#[wasm_bindgen]
pub fn get_cash_flows(params: JsValue) -> JsValue {
    let result = get_cash_flows_impl(params);
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}

fn get_cash_flows_impl(params: JsValue) -> Vec<CashFlowEntry> {
    // Parse parameters
    let bond_params: BondParams = match serde_wasm_bindgen::from_value(params) {
        Ok(p) => p,
        Err(_) => return vec![],
    };

    // Build the bond
    let bond = match create_bond(&bond_params) {
        Ok(b) => b,
        Err(_) => return vec![],
    };

    // Parse settlement date
    let settlement = match parse_date(&bond_params.settlement_date) {
        Ok(d) => d,
        Err(_) => return vec![],
    };

    // Get cash flows
    bond.cash_flows(settlement)
        .iter()
        .map(|cf| {
            let cf_type = if cf.is_principal() && decimal_to_f64(cf.amount) > 50.0 {
                if decimal_to_f64(cf.amount) > 100.0 {
                    "coupon_and_principal"
                } else {
                    "principal"
                }
            } else {
                "coupon"
            };

            CashFlowEntry {
                date: format!("{}", cf.date),
                amount: decimal_to_f64(cf.amount),
                cf_type: cf_type.to_string(),
            }
        })
        .collect()
}

/// Calculate accrued interest.
#[wasm_bindgen]
pub fn calculate_accrued(params: JsValue) -> JsValue {
    let result = calculate_accrued_impl(params);
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}

fn calculate_accrued_impl(params: JsValue) -> Result<f64, String> {
    // Parse parameters
    let bond_params: BondParams = serde_wasm_bindgen::from_value(params)
        .map_err(|e| format!("Failed to parse bond parameters: {:?}", e))?;

    // Build the bond
    let bond = create_bond(&bond_params)?;

    // Parse settlement date
    let settlement = parse_date(&bond_params.settlement_date)?;

    // Calculate accrued
    let accrued = bond.accrued_interest(settlement);
    Ok(decimal_to_f64(accrued))
}

/// Simple yield calculation without curve (only basic metrics).
#[wasm_bindgen]
pub fn calculate_simple_metrics(params: JsValue, clean_price: f64) -> JsValue {
    let result = calculate_simple_metrics_impl(params, clean_price);
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}

fn calculate_simple_metrics_impl(params: JsValue, clean_price: f64) -> AnalysisResult {
    // Parse parameters
    let bond_params: BondParams = match serde_wasm_bindgen::from_value(params) {
        Ok(p) => p,
        Err(e) => return AnalysisResult {
            error: Some(format!("Failed to parse bond parameters: {:?}", e)),
            ..Default::default()
        },
    };

    // Build the bond
    let bond = match create_bond(&bond_params) {
        Ok(b) => b,
        Err(e) => return AnalysisResult {
            error: Some(e),
            ..Default::default()
        },
    };

    // Parse settlement date
    let settlement = match parse_date(&bond_params.settlement_date) {
        Ok(d) => d,
        Err(e) => return AnalysisResult {
            error: Some(e),
            ..Default::default()
        },
    };

    // Calculate basic metrics
    let accrued = decimal_to_f64(bond.accrued_interest(settlement));
    let dirty_price = clean_price + accrued;

    let (days_to_mat, years_to_mat) = match bond.maturity() {
        Some(maturity) => {
            let days = settlement.days_between(&maturity);
            (days, days as f64 / 365.0)
        },
        None => (0, 0.0),
    };

    // Current yield = annual coupon / clean price
    let annual_coupon = decimal_to_f64(bond.coupon_rate()) * decimal_to_f64(bond.face_value()) / 100.0;
    let current_yield = if clean_price > 0.0 {
        Some(annual_coupon / clean_price * 100.0)
    } else {
        None
    };

    AnalysisResult {
        clean_price: Some(clean_price),
        dirty_price: Some(dirty_price),
        accrued_interest: Some(accrued),
        current_yield,
        days_to_maturity: Some(days_to_mat),
        years_to_maturity: Some(years_to_mat),
        error: None,
        ..Default::default()
    }
}

// ============================================================================
// Internal Helpers
// ============================================================================

fn create_bond(params: &BondParams) -> Result<FixedRateBond, String> {
    let issue_date = parse_date(&params.issue_date)?;
    let maturity_date = parse_date(&params.maturity_date)?;

    // Convert coupon rate from percentage to decimal (e.g., 5.0% -> 0.05)
    let coupon = f64_to_decimal(params.coupon_rate / 100.0);
    let face = f64_to_decimal(params.face_value.unwrap_or(100.0));
    let frequency = parse_frequency(params.frequency.unwrap_or(2));
    let day_count = parse_day_count(params.day_count.as_deref().unwrap_or("30/360"));
    let currency = parse_currency(params.currency.as_deref().unwrap_or("USD"));

    let first_coupon = params.first_coupon_date
        .as_ref()
        .and_then(|s| parse_date(s).ok());

    // Create empty identifiers (WASM users don't need bond identifiers)
    let identifiers = BondIdentifiers::new();

    let mut builder = FixedRateBondBuilder::new()
        .identifiers(identifiers)
        .issue_date(issue_date)
        .maturity(maturity_date)
        .coupon_rate(coupon)
        .face_value(face)
        .frequency(frequency)
        .day_count(day_count)
        .currency(currency)
        .business_day_convention(BusinessDayConvention::ModifiedFollowing);

    if let Some(fc) = first_coupon {
        builder = builder.first_coupon_date(fc);
    }

    builder.build()
        .map_err(|e| format!("Failed to create bond: {:?}", e))
}

fn create_curve(reference_date: Date, points: &[CurvePoint]) -> Result<ZeroCurve, String> {
    if points.is_empty() {
        return Err("Curve must have at least one point".to_string());
    }

    let mut builder = ZeroCurveBuilder::new()
        .reference_date(reference_date)
        .interpolation(InterpolationMethod::Linear);

    for point in points {
        let date = parse_date(&point.date)?;
        // Convert percentage to decimal (e.g., 4.5% -> 0.045)
        let rate = f64_to_decimal(point.rate / 100.0);
        builder = builder.add_rate(date, rate);
    }

    builder.build()
        .map_err(|e| format!("Failed to create curve: {:?}", e))
}

fn convert_yas_result(
    result: &convex_yas::YASResult,
    bond: &FixedRateBond,
    settlement: Date,
) -> AnalysisResult {
    let (days_to_mat, years_to_mat) = match bond.maturity() {
        Some(maturity) => {
            let days = settlement.days_between(&maturity);
            (days, days as f64 / 365.0)
        },
        None => (0, 0.0),
    };

    // Get invoice details for clean/dirty price
    let clean_price = decimal_to_f64(result.invoice.clean_price);
    let accrued = decimal_to_f64(result.invoice.accrued_interest);
    let dirty_price = decimal_to_f64(result.invoice.dirty_price);

    AnalysisResult {
        clean_price: Some(clean_price),
        dirty_price: Some(dirty_price),
        accrued_interest: Some(accrued),

        ytm: Some(decimal_to_f64(result.ytm)),
        current_yield: Some(decimal_to_f64(result.current_yield)),
        simple_yield: Some(decimal_to_f64(result.simple_yield)),
        money_market_yield: result.money_market_yield.map(decimal_to_f64),

        modified_duration: Some(decimal_to_f64(result.modified_duration())),
        macaulay_duration: Some(decimal_to_f64(result.risk.macaulay_duration.years())),
        convexity: Some(decimal_to_f64(result.convexity())),
        dv01: Some(decimal_to_f64(result.dv01())),

        g_spread: Some(decimal_to_f64(result.g_spread.as_bps())),
        z_spread: Some(decimal_to_f64(result.z_spread.as_bps())),
        asw_spread: result.asw_spread.as_ref().map(|s| decimal_to_f64(s.as_bps())),

        days_to_maturity: Some(days_to_mat),
        years_to_maturity: Some(years_to_mat),

        error: None,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_date() {
        let date = parse_date("2024-06-15").unwrap();
        assert_eq!(date, Date::from_ymd(2024, 6, 15).unwrap());
    }

    #[test]
    fn test_parse_date_invalid() {
        assert!(parse_date("invalid").is_err());
        assert!(parse_date("2024/06/15").is_err());
    }

    #[test]
    fn test_parse_day_count() {
        assert!(matches!(parse_day_count("30/360"), DayCountConvention::Thirty360US));
        assert!(matches!(parse_day_count("ACT/365"), DayCountConvention::Act365Fixed));
        assert!(matches!(parse_day_count("act/act"), DayCountConvention::ActActIcma));
    }

    #[test]
    fn test_parse_frequency() {
        assert!(matches!(parse_frequency(1), Frequency::Annual));
        assert!(matches!(parse_frequency(2), Frequency::SemiAnnual));
        assert!(matches!(parse_frequency(4), Frequency::Quarterly));
    }

    #[test]
    fn test_create_bond() {
        let params = BondParams {
            coupon_rate: 5.0, // 5% as percentage
            maturity_date: "2030-06-15".to_string(),
            issue_date: "2020-06-15".to_string(),
            settlement_date: "2024-06-15".to_string(),
            face_value: Some(100.0),
            frequency: Some(2),
            day_count: Some("30/360".to_string()),
            currency: Some("USD".to_string()),
            first_coupon_date: None,
        };

        let bond = create_bond(&params).unwrap();
        // Coupon rate stored as decimal (0.05 for 5%)
        assert_eq!(decimal_to_f64(bond.coupon_rate()), 0.05);
    }

    #[test]
    fn test_create_curve() {
        let reference = Date::from_ymd(2024, 6, 15).unwrap();
        let points = vec![
            CurvePoint { date: "2025-06-15".to_string(), rate: 4.0 },
            CurvePoint { date: "2026-06-15".to_string(), rate: 4.5 },
            CurvePoint { date: "2029-06-15".to_string(), rate: 5.0 },
        ];

        let curve = create_curve(reference, &points).unwrap();
        assert_eq!(curve.reference_date(), reference);
    }
}
