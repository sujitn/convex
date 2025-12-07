//! Analytics trait with blanket implementations for bonds.
//!
//! This module provides the `BondAnalytics` trait that offers common analytics
//! methods for any type implementing the `Bond` trait. These are implemented
//! as blanket implementations to avoid code duplication across bond types.
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_bonds::traits::{Bond, BondAnalytics};
//! use convex_bonds::FixedRateBond;
//!
//! let bond = FixedRateBond::builder()
//!     .coupon_rate(0.05)
//!     .maturity(date!(2030-06-15))
//!     .build()?;
//!
//! let ytm = bond.yield_to_maturity(settlement, clean_price)?;
//! let duration = bond.modified_duration(settlement, ytm)?;
//! ```

use rust_decimal::prelude::*;
use rust_decimal::Decimal;

use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Date, Frequency};

use crate::error::{BondError, BondResult};
use crate::pricing::{YieldResult, YieldSolver};
use crate::traits::Bond;
use crate::types::YieldConvention;

/// Analytics extension trait for bonds.
///
/// This trait provides common analytics calculations as blanket implementations
/// for any type implementing `Bond`. It centralizes yield, duration, convexity,
/// and DV01 calculations to avoid code duplication.
///
/// # Design
///
/// - Blanket implementation for all `Bond` implementors
/// - Uses `YieldSolver` for yield calculations
/// - Provides both analytical and numerical methods
/// - All methods take settlement date and relevant market data
pub trait BondAnalytics: Bond {
    // ==================== Yield Calculations ====================

    /// Calculates yield to maturity from clean price.
    ///
    /// # Arguments
    ///
    /// * `settlement` - Settlement date
    /// * `clean_price` - Clean price per 100 face value
    /// * `frequency` - Compounding frequency (defaults to semi-annual)
    ///
    /// # Returns
    ///
    /// Yield result containing the YTM and solver metadata.
    fn yield_to_maturity(
        &self,
        settlement: Date,
        clean_price: Decimal,
        frequency: Frequency,
    ) -> BondResult<YieldResult> {
        let cash_flows = self.cash_flows(settlement);
        if cash_flows.is_empty() {
            return Err(BondError::InvalidSpec {
                reason: "no future cash flows".to_string(),
            });
        }

        let accrued = self.accrued_interest(settlement);
        let day_count = self.parse_day_count()?;

        let solver = YieldSolver::new()
            .with_convention(YieldConvention::StreetConvention);

        solver.solve(&cash_flows, clean_price, accrued, settlement, day_count, frequency)
    }

    /// Calculates yield to maturity with a specific yield convention.
    fn yield_to_maturity_with_convention(
        &self,
        settlement: Date,
        clean_price: Decimal,
        frequency: Frequency,
        convention: YieldConvention,
    ) -> BondResult<YieldResult> {
        let cash_flows = self.cash_flows(settlement);
        if cash_flows.is_empty() {
            return Err(BondError::InvalidSpec {
                reason: "no future cash flows".to_string(),
            });
        }

        let accrued = self.accrued_interest(settlement);
        let day_count = self.parse_day_count()?;

        let solver = YieldSolver::new().with_convention(convention);
        solver.solve(&cash_flows, clean_price, accrued, settlement, day_count, frequency)
    }

    // ==================== Price Calculations ====================

    /// Calculates dirty price from yield.
    ///
    /// # Arguments
    ///
    /// * `settlement` - Settlement date
    /// * `ytm` - Yield to maturity as decimal (e.g., 0.05 for 5%)
    /// * `frequency` - Compounding frequency
    ///
    /// # Returns
    ///
    /// Dirty price per 100 face value.
    fn dirty_price_from_yield(
        &self,
        settlement: Date,
        ytm: f64,
        frequency: Frequency,
    ) -> BondResult<f64> {
        let cash_flows = self.cash_flows(settlement);
        if cash_flows.is_empty() {
            return Err(BondError::InvalidSpec {
                reason: "no future cash flows".to_string(),
            });
        }

        let day_count = self.parse_day_count()?;
        let solver = YieldSolver::new();

        Ok(solver.dirty_price_from_yield(&cash_flows, ytm, settlement, day_count, frequency))
    }

    /// Calculates clean price from yield.
    fn clean_price_from_yield(
        &self,
        settlement: Date,
        ytm: f64,
        frequency: Frequency,
    ) -> BondResult<f64> {
        let cash_flows = self.cash_flows(settlement);
        if cash_flows.is_empty() {
            return Err(BondError::InvalidSpec {
                reason: "no future cash flows".to_string(),
            });
        }

        let accrued = self.accrued_interest(settlement);
        let day_count = self.parse_day_count()?;
        let solver = YieldSolver::new();

        Ok(solver.clean_price_from_yield(&cash_flows, ytm, accrued, settlement, day_count, frequency))
    }

    // ==================== Duration Calculations ====================

    /// Calculates Macaulay duration analytically.
    ///
    /// Macaulay duration is the weighted average time to receive cash flows,
    /// where weights are the present values of cash flows.
    ///
    /// # Arguments
    ///
    /// * `settlement` - Settlement date
    /// * `ytm` - Yield to maturity as decimal
    /// * `frequency` - Compounding frequency
    fn macaulay_duration(
        &self,
        settlement: Date,
        ytm: f64,
        frequency: Frequency,
    ) -> BondResult<f64> {
        let cash_flows = self.cash_flows(settlement);
        if cash_flows.is_empty() {
            return Err(BondError::InvalidSpec {
                reason: "no future cash flows".to_string(),
            });
        }

        let day_count = self.parse_day_count()?;
        let periods_per_year = frequency.periods_per_year() as f64;
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
            return Err(BondError::InvalidSpec {
                reason: "zero present value".to_string(),
            });
        }

        Ok(weighted_time / total_pv)
    }

    /// Calculates modified duration from Macaulay duration.
    ///
    /// Modified Duration = Macaulay Duration / (1 + y/f)
    ///
    /// where y is the yield and f is the frequency.
    fn modified_duration(
        &self,
        settlement: Date,
        ytm: f64,
        frequency: Frequency,
    ) -> BondResult<f64> {
        let mac_dur = self.macaulay_duration(settlement, ytm, frequency)?;
        let periods_per_year = frequency.periods_per_year() as f64;
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
    /// * `settlement` - Settlement date
    /// * `ytm` - Current yield to maturity
    /// * `frequency` - Compounding frequency
    /// * `bump_bps` - Yield bump size in basis points (default: 10)
    fn effective_duration(
        &self,
        settlement: Date,
        ytm: f64,
        frequency: Frequency,
        bump_bps: f64,
    ) -> BondResult<f64> {
        let bump = bump_bps / 10_000.0;

        let price_base = self.dirty_price_from_yield(settlement, ytm, frequency)?;
        let price_up = self.dirty_price_from_yield(settlement, ytm + bump, frequency)?;
        let price_down = self.dirty_price_from_yield(settlement, ytm - bump, frequency)?;

        if price_base.abs() < 1e-10 {
            return Err(BondError::InvalidSpec {
                reason: "zero base price".to_string(),
            });
        }

        Ok((price_down - price_up) / (2.0 * price_base * bump))
    }

    // ==================== Convexity Calculations ====================

    /// Calculates analytical convexity.
    ///
    /// Convexity measures the curvature of the price-yield relationship.
    /// It captures the second-order effect that duration misses.
    fn convexity(
        &self,
        settlement: Date,
        ytm: f64,
        frequency: Frequency,
    ) -> BondResult<f64> {
        let cash_flows = self.cash_flows(settlement);
        if cash_flows.is_empty() {
            return Err(BondError::InvalidSpec {
                reason: "no future cash flows".to_string(),
            });
        }

        let day_count = self.parse_day_count()?;
        let periods_per_year = frequency.periods_per_year() as f64;
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

            // Convexity contribution: t(t+1) * PV / (1+y/f)^2
            let convex_term = years_f64 * (years_f64 + 1.0 / periods_per_year) * pv;
            weighted_convexity += convex_term;
            total_pv += pv;
        }

        if total_pv.abs() < 1e-10 {
            return Err(BondError::InvalidSpec {
                reason: "zero present value".to_string(),
            });
        }

        let y_factor = (1.0 + rate_per_period).powi(2);
        Ok(weighted_convexity / (total_pv * y_factor))
    }

    /// Calculates effective convexity using numerical bumping.
    ///
    /// C_eff = (P_up + P_down - 2 × P_0) / (P_0 × Δy²)
    fn effective_convexity(
        &self,
        settlement: Date,
        ytm: f64,
        frequency: Frequency,
        bump_bps: f64,
    ) -> BondResult<f64> {
        let bump = bump_bps / 10_000.0;

        let price_base = self.dirty_price_from_yield(settlement, ytm, frequency)?;
        let price_up = self.dirty_price_from_yield(settlement, ytm + bump, frequency)?;
        let price_down = self.dirty_price_from_yield(settlement, ytm - bump, frequency)?;

        if price_base.abs() < 1e-10 {
            return Err(BondError::InvalidSpec {
                reason: "zero base price".to_string(),
            });
        }

        Ok((price_up + price_down - 2.0 * price_base) / (price_base * bump * bump))
    }

    // ==================== DV01 Calculations ====================

    /// Calculates DV01 (dollar value of 01 - one basis point).
    ///
    /// DV01 = Modified Duration × Dirty Price × 0.0001
    ///
    /// Returns the price change per $100 face value for a 1bp yield move.
    fn dv01(&self, settlement: Date, ytm: f64, dirty_price: f64, frequency: Frequency) -> BondResult<f64> {
        let mod_dur = self.modified_duration(settlement, ytm, frequency)?;
        Ok(mod_dur * dirty_price * 0.0001)
    }

    /// Calculates DV01 for a specific notional amount.
    fn dv01_notional(
        &self,
        settlement: Date,
        ytm: f64,
        dirty_price: f64,
        notional: f64,
        frequency: Frequency,
    ) -> BondResult<f64> {
        let mod_dur = self.modified_duration(settlement, ytm, frequency)?;
        let face = self.face_value().to_f64().unwrap_or(100.0);
        Ok(mod_dur * dirty_price * (notional / face) * 0.0001)
    }

    // ==================== Price Change Estimation ====================

    /// Estimates price change for a given yield shift.
    ///
    /// Uses duration + convexity approximation:
    /// ΔP/P ≈ -D_mod × Δy + (1/2) × C × (Δy)²
    fn estimate_price_change(
        &self,
        settlement: Date,
        ytm: f64,
        dirty_price: f64,
        yield_change: f64,
        frequency: Frequency,
    ) -> BondResult<f64> {
        let mod_dur = self.modified_duration(settlement, ytm, frequency)?;
        let convex = self.convexity(settlement, ytm, frequency)?;

        let duration_effect = -mod_dur * dirty_price * yield_change;
        let convexity_effect = 0.5 * convex * dirty_price * yield_change.powi(2);

        Ok(duration_effect + convexity_effect)
    }

    // ==================== Helper Methods ====================

    /// Parses the day count convention string to enum.
    ///
    /// This method converts the string returned by `day_count_convention()`
    /// back to the `DayCountConvention` enum.
    fn parse_day_count(&self) -> BondResult<DayCountConvention> {
        let dcc_str = self.day_count_convention();
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
            _ => Err(BondError::InvalidSpec {
                reason: format!("unknown day count convention: {}", dcc_str),
            }),
        }
    }
}

// Blanket implementation for all Bond types
impl<T: Bond + ?Sized> BondAnalytics for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instruments::FixedRateBond;
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

        let result = bond.yield_to_maturity(settlement, clean_price, Frequency::SemiAnnual);
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
        let ytm_result = bond.yield_to_maturity(settlement, clean_price, Frequency::SemiAnnual).unwrap();

        // Calculate clean price from YTM
        let calculated_clean = bond.clean_price_from_yield(
            settlement,
            ytm_result.yield_value,
            Frequency::SemiAnnual,
        ).unwrap();

        // Should round-trip
        let diff = (calculated_clean - clean_price.to_f64().unwrap()).abs();
        assert!(diff < 0.001, "Price roundtrip error: {}", diff);
    }

    #[test]
    fn test_modified_duration() {
        let bond = create_test_bond();
        let settlement = date(2020, 6, 15);
        let ytm = 0.075;

        let mod_dur = bond.modified_duration(settlement, ytm, Frequency::SemiAnnual);
        assert!(mod_dur.is_ok());

        let dur = mod_dur.unwrap();
        // 5-year bond should have duration around 4.0-4.5
        assert!(dur > 3.5 && dur < 5.0, "Modified duration {} out of range", dur);
    }

    #[test]
    fn test_convexity() {
        let bond = create_test_bond();
        let settlement = date(2020, 6, 15);
        let ytm = 0.075;

        let convex = bond.convexity(settlement, ytm, Frequency::SemiAnnual);
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

        let dv01 = bond.dv01(settlement, ytm, dirty_price, Frequency::SemiAnnual);
        assert!(dv01.is_ok());

        let d = dv01.unwrap();
        // DV01 for $100 should be around 0.04-0.05 for a 4-year duration bond
        assert!(d > 0.03 && d < 0.06, "DV01 {} out of range", d);
    }

    #[test]
    fn test_effective_vs_analytical_duration() {
        let bond = create_test_bond();
        let settlement = date(2020, 6, 15);
        let ytm = 0.075;

        let mod_dur = bond.modified_duration(settlement, ytm, Frequency::SemiAnnual).unwrap();
        let eff_dur = bond.effective_duration(settlement, ytm, Frequency::SemiAnnual, 10.0).unwrap();

        // For vanilla bonds, effective should be close to analytical
        let diff = (mod_dur - eff_dur).abs();
        assert!(diff < 0.1, "Duration mismatch: analytical={}, effective={}", mod_dur, eff_dur);
    }

    #[test]
    fn test_price_change_estimation() {
        let bond = create_test_bond();
        let settlement = date(2020, 6, 15);
        let ytm = 0.075;
        let dirty_price = 100.0;

        // Estimate price change for +100 bps
        let change = bond.estimate_price_change(
            settlement,
            ytm,
            dirty_price,
            0.01, // 100 bps
            Frequency::SemiAnnual,
        ).unwrap();

        // Price should drop when yield rises
        assert!(change < 0.0);
        // For ~4 duration, expect ~4% drop
        assert!(change > -5.0 && change < -3.0, "Price change {} out of range", change);
    }
}
