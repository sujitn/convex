//! Standalone functions for bond analytics.
//!
//! This module provides high-level standalone functions that operate on bonds.
//! These functions replace the `BondAnalytics` trait from `convex-bonds` with
//! a more functional, composable approach.
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_bonds::instruments::FixedRateBond;
//! use convex_analytics::functions::*;
//! use convex_core::types::{Date, Frequency};
//! use rust_decimal_macros::dec;
//!
//! let bond = FixedRateBond::builder()
//!     .coupon_rate(dec!(0.05))
//!     .maturity(date!(2030-06-15))
//!     .build()?;
//!
//! // Calculate YTM from clean price
//! let result = yield_to_maturity(&bond, settlement, dec!(105), Frequency::SemiAnnual)?;
//!
//! // Calculate risk metrics
//! let mod_dur = modified_duration(&bond, settlement, result.yield_value, Frequency::SemiAnnual)?;
//! let convex = convexity(&bond, settlement, result.yield_value, Frequency::SemiAnnual)?;
//! ```

use rust_decimal::prelude::*;
use rust_decimal::Decimal;

use convex_bonds::traits::Bond;
use convex_bonds::types::YieldConvention;
use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Date, Frequency};

use crate::error::{AnalyticsError, AnalyticsResult};
use crate::yields::{YieldResult, YieldSolver};

// ============================================================================
// YIELD CALCULATIONS
// ============================================================================

/// Calculates yield to maturity from clean price.
///
/// Uses the street convention methodology (ISMA 30/360 for most bonds).
///
/// # Arguments
///
/// * `bond` - The bond to analyze
/// * `settlement` - Settlement date
/// * `clean_price` - Clean price per 100 face value
/// * `frequency` - Compounding frequency
///
/// # Returns
///
/// Yield result containing the YTM and solver metadata.
///
/// # Example
///
/// ```rust,ignore
/// let result = yield_to_maturity(&bond, settlement, dec!(105), Frequency::SemiAnnual)?;
/// println!("YTM: {:.2}%", result.yield_value * 100.0);
/// ```
pub fn yield_to_maturity(
    bond: &dyn Bond,
    settlement: Date,
    clean_price: Decimal,
    frequency: Frequency,
) -> AnalyticsResult<YieldResult> {
    yield_to_maturity_with_convention(
        bond,
        settlement,
        clean_price,
        frequency,
        YieldConvention::StreetConvention,
    )
}

/// Calculates yield to maturity with a specific yield convention.
///
/// # Arguments
///
/// * `bond` - The bond to analyze
/// * `settlement` - Settlement date
/// * `clean_price` - Clean price per 100 face value
/// * `frequency` - Compounding frequency
/// * `convention` - Yield calculation convention
pub fn yield_to_maturity_with_convention(
    bond: &dyn Bond,
    settlement: Date,
    clean_price: Decimal,
    frequency: Frequency,
    convention: YieldConvention,
) -> AnalyticsResult<YieldResult> {
    let cash_flows = bond.cash_flows(settlement);
    if cash_flows.is_empty() {
        return Err(AnalyticsError::InvalidInput(
            "no future cash flows".to_string(),
        ));
    }

    let accrued = bond.accrued_interest(settlement);
    let day_count = parse_day_count(bond.day_count_convention())?;

    let solver = YieldSolver::new().with_convention(convention);
    solver
        .solve(
            &cash_flows,
            clean_price,
            accrued,
            settlement,
            day_count,
            frequency,
        )
        .map_err(|e| AnalyticsError::YieldSolverFailed {
            iterations: 100,
            reason: e.to_string(),
        })
}

// ============================================================================
// PRICE CALCULATIONS
// ============================================================================

/// Calculates dirty price from yield.
///
/// # Arguments
///
/// * `bond` - The bond to analyze
/// * `settlement` - Settlement date
/// * `ytm` - Yield to maturity as decimal (e.g., 0.05 for 5%)
/// * `frequency` - Compounding frequency
///
/// # Returns
///
/// Dirty price per 100 face value.
pub fn dirty_price_from_yield(
    bond: &dyn Bond,
    settlement: Date,
    ytm: f64,
    frequency: Frequency,
) -> AnalyticsResult<f64> {
    let cash_flows = bond.cash_flows(settlement);
    if cash_flows.is_empty() {
        return Err(AnalyticsError::InvalidInput(
            "no future cash flows".to_string(),
        ));
    }

    let day_count = parse_day_count(bond.day_count_convention())?;
    let solver = YieldSolver::new();

    Ok(solver.dirty_price_from_yield(&cash_flows, ytm, settlement, day_count, frequency))
}

/// Calculates clean price from yield.
///
/// # Arguments
///
/// * `bond` - The bond to analyze
/// * `settlement` - Settlement date
/// * `ytm` - Yield to maturity as decimal
/// * `frequency` - Compounding frequency
///
/// # Returns
///
/// Clean price per 100 face value.
pub fn clean_price_from_yield(
    bond: &dyn Bond,
    settlement: Date,
    ytm: f64,
    frequency: Frequency,
) -> AnalyticsResult<f64> {
    let cash_flows = bond.cash_flows(settlement);
    if cash_flows.is_empty() {
        return Err(AnalyticsError::InvalidInput(
            "no future cash flows".to_string(),
        ));
    }

    let accrued = bond.accrued_interest(settlement);
    let day_count = parse_day_count(bond.day_count_convention())?;
    let solver = YieldSolver::new();

    Ok(solver.clean_price_from_yield(&cash_flows, ytm, accrued, settlement, day_count, frequency))
}

// ============================================================================
// DURATION CALCULATIONS
// ============================================================================

/// Calculates Macaulay duration analytically.
///
/// Macaulay duration is the weighted average time to receive cash flows,
/// where weights are the present values of cash flows.
///
/// # Arguments
///
/// * `bond` - The bond to analyze
/// * `settlement` - Settlement date
/// * `ytm` - Yield to maturity as decimal
/// * `frequency` - Compounding frequency
///
/// # Returns
///
/// Duration in years.
pub fn macaulay_duration(
    bond: &dyn Bond,
    settlement: Date,
    ytm: f64,
    frequency: Frequency,
) -> AnalyticsResult<f64> {
    let cash_flows = bond.cash_flows(settlement);
    if cash_flows.is_empty() {
        return Err(AnalyticsError::InvalidInput(
            "no future cash flows".to_string(),
        ));
    }

    let day_count = parse_day_count(bond.day_count_convention())?;
    let periods_per_year = f64::from(frequency.periods_per_year());
    let rate_per_period = ytm / periods_per_year;

    let mut weighted_time = 0.0;
    let mut total_pv = 0.0;

    for cf in &cash_flows {
        if cf.date <= settlement {
            continue;
        }

        let years = day_count.to_day_count().year_fraction(settlement, cf.date);
        let years_f64 = years.to_f64().unwrap_or(0.0);
        let periods = years_f64 * periods_per_year;
        let amount = cf.amount.to_f64().unwrap_or(0.0);

        let df = 1.0 / (1.0 + rate_per_period).powf(periods);
        let pv = amount * df;

        weighted_time += years_f64 * pv;
        total_pv += pv;
    }

    if total_pv.abs() < 1e-10 {
        return Err(AnalyticsError::DurationFailed(
            "zero present value".to_string(),
        ));
    }

    Ok(weighted_time / total_pv)
}

/// Calculates modified duration from Macaulay duration.
///
/// Modified Duration = Macaulay Duration / (1 + y/f)
///
/// where y is the yield and f is the frequency.
///
/// # Arguments
///
/// * `bond` - The bond to analyze
/// * `settlement` - Settlement date
/// * `ytm` - Yield to maturity as decimal
/// * `frequency` - Compounding frequency
///
/// # Returns
///
/// Modified duration in years.
pub fn modified_duration(
    bond: &dyn Bond,
    settlement: Date,
    ytm: f64,
    frequency: Frequency,
) -> AnalyticsResult<f64> {
    let mac_dur = macaulay_duration(bond, settlement, ytm, frequency)?;
    let periods_per_year = f64::from(frequency.periods_per_year());
    Ok(mac_dur / (1.0 + ytm / periods_per_year))
}

/// Calculates effective duration using numerical bumping.
///
/// Effective duration is computed by repricing the bond with
/// yield shifts and using the central difference formula:
///
/// D_eff = (P_down - P_up) / (2 × P_0 × Δy)
///
/// # Arguments
///
/// * `bond` - The bond to analyze
/// * `settlement` - Settlement date
/// * `ytm` - Current yield to maturity
/// * `frequency` - Compounding frequency
/// * `bump_bps` - Yield bump size in basis points (default: 10)
///
/// # Returns
///
/// Effective duration in years.
pub fn effective_duration(
    bond: &dyn Bond,
    settlement: Date,
    ytm: f64,
    frequency: Frequency,
    bump_bps: f64,
) -> AnalyticsResult<f64> {
    let bump = bump_bps / 10_000.0;

    let price_base = dirty_price_from_yield(bond, settlement, ytm, frequency)?;
    let price_up = dirty_price_from_yield(bond, settlement, ytm + bump, frequency)?;
    let price_down = dirty_price_from_yield(bond, settlement, ytm - bump, frequency)?;

    if price_base.abs() < 1e-10 {
        return Err(AnalyticsError::DurationFailed(
            "zero base price".to_string(),
        ));
    }

    Ok((price_down - price_up) / (2.0 * price_base * bump))
}

// ============================================================================
// CONVEXITY CALCULATIONS
// ============================================================================

/// Calculates analytical convexity.
///
/// Convexity measures the curvature of the price-yield relationship.
/// It captures the second-order effect that duration misses.
///
/// # Arguments
///
/// * `bond` - The bond to analyze
/// * `settlement` - Settlement date
/// * `ytm` - Yield to maturity as decimal
/// * `frequency` - Compounding frequency
///
/// # Returns
///
/// Convexity (in years squared).
pub fn convexity(
    bond: &dyn Bond,
    settlement: Date,
    ytm: f64,
    frequency: Frequency,
) -> AnalyticsResult<f64> {
    let cash_flows = bond.cash_flows(settlement);
    if cash_flows.is_empty() {
        return Err(AnalyticsError::InvalidInput(
            "no future cash flows".to_string(),
        ));
    }

    let day_count = parse_day_count(bond.day_count_convention())?;
    let periods_per_year = f64::from(frequency.periods_per_year());
    let rate_per_period = ytm / periods_per_year;

    let mut weighted_convexity = 0.0;
    let mut total_pv = 0.0;

    for cf in &cash_flows {
        if cf.date <= settlement {
            continue;
        }

        let years = day_count.to_day_count().year_fraction(settlement, cf.date);
        let years_f64 = years.to_f64().unwrap_or(0.0);
        let periods = years_f64 * periods_per_year;
        let amount = cf.amount.to_f64().unwrap_or(0.0);

        let df = 1.0 / (1.0 + rate_per_period).powf(periods);
        let pv = amount * df;

        // Convexity contribution: t(t+1/f) * PV / (1+y/f)^2
        let convex_term = years_f64 * (years_f64 + 1.0 / periods_per_year) * pv;
        weighted_convexity += convex_term;
        total_pv += pv;
    }

    if total_pv.abs() < 1e-10 {
        return Err(AnalyticsError::ConvexityFailed(
            "zero present value".to_string(),
        ));
    }

    let y_factor = (1.0 + rate_per_period).powi(2);
    Ok(weighted_convexity / (total_pv * y_factor))
}

/// Calculates effective convexity using numerical bumping.
///
/// C_eff = (P_up + P_down - 2 × P_0) / (P_0 × Δy²)
///
/// # Arguments
///
/// * `bond` - The bond to analyze
/// * `settlement` - Settlement date
/// * `ytm` - Current yield to maturity
/// * `frequency` - Compounding frequency
/// * `bump_bps` - Yield bump size in basis points
///
/// # Returns
///
/// Effective convexity.
pub fn effective_convexity(
    bond: &dyn Bond,
    settlement: Date,
    ytm: f64,
    frequency: Frequency,
    bump_bps: f64,
) -> AnalyticsResult<f64> {
    let bump = bump_bps / 10_000.0;

    let price_base = dirty_price_from_yield(bond, settlement, ytm, frequency)?;
    let price_up = dirty_price_from_yield(bond, settlement, ytm + bump, frequency)?;
    let price_down = dirty_price_from_yield(bond, settlement, ytm - bump, frequency)?;

    if price_base.abs() < 1e-10 {
        return Err(AnalyticsError::ConvexityFailed(
            "zero base price".to_string(),
        ));
    }

    Ok((price_up + price_down - 2.0 * price_base) / (price_base * bump * bump))
}

// ============================================================================
// DV01 CALCULATIONS
// ============================================================================

/// Calculates DV01 (dollar value of 01 - one basis point).
///
/// DV01 = Modified Duration × Dirty Price × 0.0001
///
/// Returns the price change per $100 face value for a 1bp yield move.
///
/// # Arguments
///
/// * `bond` - The bond to analyze
/// * `settlement` - Settlement date
/// * `ytm` - Yield to maturity as decimal
/// * `dirty_price` - Dirty price per 100 face value
/// * `frequency` - Compounding frequency
///
/// # Returns
///
/// DV01 per 100 face value.
pub fn dv01(
    bond: &dyn Bond,
    settlement: Date,
    ytm: f64,
    dirty_price: f64,
    frequency: Frequency,
) -> AnalyticsResult<f64> {
    let mod_dur = modified_duration(bond, settlement, ytm, frequency)?;
    Ok(mod_dur * dirty_price * 0.0001)
}

/// Calculates DV01 for a specific notional amount.
///
/// # Arguments
///
/// * `bond` - The bond to analyze
/// * `settlement` - Settlement date
/// * `ytm` - Yield to maturity as decimal
/// * `dirty_price` - Dirty price per 100 face value
/// * `notional` - Notional amount
/// * `frequency` - Compounding frequency
///
/// # Returns
///
/// DV01 for the notional amount.
pub fn dv01_notional(
    bond: &dyn Bond,
    settlement: Date,
    ytm: f64,
    dirty_price: f64,
    notional: f64,
    frequency: Frequency,
) -> AnalyticsResult<f64> {
    let mod_dur = modified_duration(bond, settlement, ytm, frequency)?;
    let face = bond.face_value().to_f64().unwrap_or(100.0);
    Ok(mod_dur * dirty_price * (notional / face) * 0.0001)
}

// ============================================================================
// PRICE CHANGE ESTIMATION
// ============================================================================

/// Estimates price change for a given yield shift.
///
/// Uses duration + convexity approximation:
/// ΔP/P ≈ -D_mod × Δy + (1/2) × C × (Δy)²
///
/// # Arguments
///
/// * `bond` - The bond to analyze
/// * `settlement` - Settlement date
/// * `ytm` - Current yield to maturity
/// * `dirty_price` - Current dirty price
/// * `yield_change` - Yield change (e.g., 0.01 for 100 bps)
/// * `frequency` - Compounding frequency
///
/// # Returns
///
/// Estimated price change (absolute, not percentage).
pub fn estimate_price_change(
    bond: &dyn Bond,
    settlement: Date,
    ytm: f64,
    dirty_price: f64,
    yield_change: f64,
    frequency: Frequency,
) -> AnalyticsResult<f64> {
    let mod_dur = modified_duration(bond, settlement, ytm, frequency)?;
    let convex = convexity(bond, settlement, ytm, frequency)?;

    let duration_effect = -mod_dur * dirty_price * yield_change;
    let convexity_effect = 0.5 * convex * dirty_price * yield_change.powi(2);

    Ok(duration_effect + convexity_effect)
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Parses a day count convention string to the enum.
///
/// Supports common formats used in bond specifications.
pub fn parse_day_count(dcc_str: &str) -> AnalyticsResult<DayCountConvention> {
    match dcc_str {
        "ACT/360" => Ok(DayCountConvention::Act360),
        "ACT/365F" | "ACT/365 Fixed" => Ok(DayCountConvention::Act365Fixed),
        "ACT/365L" | "ACT/365 Leap" => Ok(DayCountConvention::Act365Leap),
        "ACT/ACT ISDA" | "ACT/ACT" => Ok(DayCountConvention::ActActIsda),
        "ACT/ACT ICMA" => Ok(DayCountConvention::ActActIcma),
        "ACT/ACT AFB" => Ok(DayCountConvention::ActActAfb),
        "30/360 US" | "30/360" => Ok(DayCountConvention::Thirty360US),
        "30E/360" | "30/360 E" => Ok(DayCountConvention::Thirty360E),
        "30E/360 ISDA" => Ok(DayCountConvention::Thirty360EIsda),
        "30/360 German" => Ok(DayCountConvention::Thirty360German),
        _ => Err(AnalyticsError::DayCountError(format!(
            "unknown day count convention: {dcc_str}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_bonds::instruments::FixedRateBond;
    use rust_decimal_macros::dec;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    fn create_test_bond() -> FixedRateBond {
        FixedRateBond::builder()
            .issue_date(date(2020, 6, 15))
            .maturity(date(2025, 6, 15))
            .coupon_rate(dec!(0.075))
            .face_value(dec!(100))
            .frequency(Frequency::SemiAnnual)
            .day_count(DayCountConvention::Thirty360US)
            .cusip_unchecked("097023AH7")
            .build()
            .unwrap()
    }

    #[test]
    fn test_ytm_at_par() {
        let bond = create_test_bond();
        let settlement = date(2020, 6, 15);
        let clean_price = dec!(100);

        let result = yield_to_maturity(&bond, settlement, clean_price, Frequency::SemiAnnual);
        assert!(result.is_ok());

        let ytm = result.unwrap().yield_value;
        // At par, YTM should equal coupon rate (7.5%)
        assert!((ytm - 0.075).abs() < 0.001);
    }

    #[test]
    fn test_ytm_price_roundtrip() {
        let bond = create_test_bond();
        let settlement = date(2021, 1, 15);
        let clean_price = dec!(105);

        // Calculate YTM from price
        let ytm_result =
            yield_to_maturity(&bond, settlement, clean_price, Frequency::SemiAnnual).unwrap();

        // Calculate clean price from YTM
        let calculated_clean = clean_price_from_yield(
            &bond,
            settlement,
            ytm_result.yield_value,
            Frequency::SemiAnnual,
        )
        .unwrap();

        // Should round-trip
        let diff = (calculated_clean - clean_price.to_f64().unwrap()).abs();
        assert!(diff < 0.001, "Price roundtrip error: {}", diff);
    }

    #[test]
    fn test_modified_duration() {
        let bond = create_test_bond();
        let settlement = date(2020, 6, 15);
        let ytm = 0.075;

        let mod_dur = modified_duration(&bond, settlement, ytm, Frequency::SemiAnnual);
        assert!(mod_dur.is_ok());

        let dur = mod_dur.unwrap();
        // 5-year bond should have duration around 4.0-4.5
        assert!(
            dur > 3.5 && dur < 5.0,
            "Modified duration {} out of range",
            dur
        );
    }

    #[test]
    fn test_convexity() {
        let bond = create_test_bond();
        let settlement = date(2020, 6, 15);
        let ytm = 0.075;

        let convex = convexity(&bond, settlement, ytm, Frequency::SemiAnnual);
        assert!(convex.is_ok());

        let c = convex.unwrap();
        // Convexity should be positive
        assert!(c > 0.0, "Convexity should be positive");
        // 5-year bond convexity typically in range 15-25
        assert!(c > 10.0 && c < 30.0, "Convexity {} out of range", c);
    }

    #[test]
    fn test_dv01() {
        let bond = create_test_bond();
        let settlement = date(2020, 6, 15);
        let ytm = 0.075;
        let dirty_price = 100.0;

        let dv01_value = dv01(&bond, settlement, ytm, dirty_price, Frequency::SemiAnnual);
        assert!(dv01_value.is_ok());

        let d = dv01_value.unwrap();
        // DV01 for $100 should be around 0.04-0.05 for a 4-year duration bond
        assert!(d > 0.03 && d < 0.06, "DV01 {} out of range", d);
    }

    #[test]
    fn test_effective_vs_analytical_duration() {
        let bond = create_test_bond();
        let settlement = date(2020, 6, 15);
        let ytm = 0.075;

        let mod_dur = modified_duration(&bond, settlement, ytm, Frequency::SemiAnnual).unwrap();
        let eff_dur =
            effective_duration(&bond, settlement, ytm, Frequency::SemiAnnual, 10.0).unwrap();

        // For vanilla bonds, effective should be close to analytical
        let diff = (mod_dur - eff_dur).abs();
        assert!(
            diff < 0.1,
            "Duration mismatch: analytical={}, effective={}",
            mod_dur,
            eff_dur
        );
    }

    #[test]
    fn test_price_change_estimation() {
        let bond = create_test_bond();
        let settlement = date(2020, 6, 15);
        let ytm = 0.075;
        let dirty_price = 100.0;

        // Estimate price change for +100 bps
        let change = estimate_price_change(
            &bond,
            settlement,
            ytm,
            dirty_price,
            0.01, // 100 bps
            Frequency::SemiAnnual,
        )
        .unwrap();

        // Price should drop when yield rises
        assert!(change < 0.0);
        // For ~4 duration, expect ~4% drop
        assert!(
            change > -5.0 && change < -3.0,
            "Price change {} out of range",
            change
        );
    }

    #[test]
    fn test_parse_day_count() {
        assert_eq!(
            parse_day_count("ACT/360").unwrap(),
            DayCountConvention::Act360
        );
        assert_eq!(
            parse_day_count("30/360").unwrap(),
            DayCountConvention::Thirty360US
        );
        assert_eq!(
            parse_day_count("ACT/ACT ISDA").unwrap(),
            DayCountConvention::ActActIsda
        );
        assert!(parse_day_count("INVALID").is_err());
    }
}
