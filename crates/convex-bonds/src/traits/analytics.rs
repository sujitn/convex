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

use std::str::FromStr;

use rust_decimal::prelude::*;
use rust_decimal::Decimal;

use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Compounding, Date, Frequency};

use crate::error::{BondError, BondResult};
use crate::pricing::{dated_cash::DatedCashYieldRisk, risk_math, YieldResult, YieldSolver};
use crate::traits::Bond;
use crate::types::YieldConvention;

fn complete_yield_risk<B: Bond + ?Sized>(
    bond: &B,
    settlement: Date,
    ytm: f64,
    frequency: Frequency,
) -> BondResult<DatedCashYieldRisk> {
    let cash = bond.yield_cash_flows(settlement, frequency)?;
    let risk = cash.yield_risk(ytm, Compounding::from(frequency))?;
    if risk.present_value.abs() < 1e-10 {
        return Err(BondError::invalid_spec("zero present value"));
    }
    Ok(risk)
}

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
        self.yield_to_maturity_with_convention(
            settlement,
            clean_price,
            frequency,
            YieldConvention::StreetConvention,
        )
    }

    /// Yield inversion over the same normalized dated cashflows as pricing/risk.
    fn yield_to_maturity_with_convention(
        &self,
        settlement: Date,
        clean_price: Decimal,
        frequency: Frequency,
        convention: YieldConvention,
    ) -> BondResult<YieldResult> {
        let cash = self.yield_cash_flows(settlement, frequency)?;
        let target = clean_price
            .checked_add(self.try_accrued_interest(settlement)?)
            .and_then(|v| v.to_f64())
            .ok_or_else(|| BondError::invalid_spec("dirty price overflow"))?;
        if target <= 0.0 {
            return Err(BondError::invalid_spec("dirty price must be positive"));
        }
        let solver = YieldSolver::new().with_convention(convention);
        solver.solve_dated_cash(
            &cash,
            target,
            solver.cash_basis(f64::from(frequency.periods_per_year()))?,
            0.05,
        )
    }

    /// Dirty price per 100 using the canonical dated yield cashflows.
    fn dirty_price_from_yield(
        &self,
        settlement: Date,
        ytm: f64,
        frequency: Frequency,
    ) -> BondResult<f64> {
        let cash = self.yield_cash_flows(settlement, frequency)?;
        cash.price(
            ytm,
            crate::pricing::yield_cash::CashYieldCompounding::Periodic {
                frequency: f64::from(frequency.periods_per_year()),
            },
        )
    }

    /// Clean price per 100 using the same accrued convention as inversion.
    fn clean_price_from_yield(
        &self,
        settlement: Date,
        ytm: f64,
        frequency: Frequency,
    ) -> BondResult<f64> {
        let dirty = self.dirty_price_from_yield(settlement, ytm, frequency)?;
        let accrued = self
            .try_accrued_interest(settlement)?
            .to_f64()
            .ok_or_else(|| BondError::invalid_spec("accrued conversion"))?;
        Ok(dirty - accrued)
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
        Ok(complete_yield_risk(self, settlement, ytm, frequency)?.macaulay_duration)
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
        Ok(complete_yield_risk(self, settlement, ytm, frequency)?.modified_duration)
    }

    /// Calculates effective duration using numerical bumping.
    ///
    /// Effective duration is computed by repricing the bond with
    /// yield shifts and using the central difference formula:
    ///
    /// `D_eff` = (`P_down` - `P_up`) / (2 × `P_0` × Δy)
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
        let bump = risk_math::yield_bump(ytm, bump_bps)?;
        let price_base = self.dirty_price_from_yield(settlement, ytm, frequency)?;
        let price_up = self.dirty_price_from_yield(settlement, ytm + bump, frequency)?;
        let price_down = self.dirty_price_from_yield(settlement, ytm - bump, frequency)?;
        risk_math::duration_stencil(price_up, price_down, price_base, bump)
    }

    // ==================== Convexity Calculations ====================

    /// Calculates analytical convexity.
    ///
    /// Convexity measures the curvature of the price-yield relationship.
    /// It captures the second-order effect that duration misses.
    fn convexity(&self, settlement: Date, ytm: f64, frequency: Frequency) -> BondResult<f64> {
        Ok(complete_yield_risk(self, settlement, ytm, frequency)?.convexity)
    }

    /// Calculates effective convexity using numerical bumping.
    ///
    /// `C_eff` = (`P_up` + `P_down` - 2 × `P_0`) / (`P_0` × Δy²)
    fn effective_convexity(
        &self,
        settlement: Date,
        ytm: f64,
        frequency: Frequency,
        bump_bps: f64,
    ) -> BondResult<f64> {
        let bump = risk_math::yield_bump(ytm, bump_bps)?;
        let price_base = self.dirty_price_from_yield(settlement, ytm, frequency)?;
        let price_up = self.dirty_price_from_yield(settlement, ytm + bump, frequency)?;
        let price_down = self.dirty_price_from_yield(settlement, ytm - bump, frequency)?;
        risk_math::convexity_stencil(price_up, price_down, price_base, bump)
    }

    // ==================== DV01 Calculations ====================

    /// Calculates DV01 (dollar value of 01 - one basis point).
    ///
    /// DV01 = Modified Duration × Dirty Price × 0.0001
    ///
    /// Returns the price change per $100 face value for a 1bp yield move.
    fn dv01(
        &self,
        settlement: Date,
        ytm: f64,
        dirty_price: f64,
        frequency: Frequency,
    ) -> BondResult<f64> {
        let mod_dur = self.modified_duration(settlement, ytm, frequency)?;
        risk_math::dv01(mod_dur, dirty_price, 100.0)
    }

    /// Calculates DV01 for signed notional face using the supplied per100 dirty price.
    /// Instrument face does not change the per100 quotation denominator.
    fn dv01_notional(
        &self,
        settlement: Date,
        ytm: f64,
        dirty_price: f64,
        notional: f64,
        frequency: Frequency,
    ) -> BondResult<f64> {
        let mod_dur = self.modified_duration(settlement, ytm, frequency)?;
        risk_math::dv01(mod_dur, dirty_price, notional)
    }

    // ==================== Price Change Estimation ====================

    /// Estimates price change for a given yield shift.
    ///
    /// Uses duration + convexity approximation:
    /// ΔP/P ≈ -`D_mod` × Δy + (1/2) × C × (Δy)²
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
        risk_math::price_change(mod_dur, convex, dirty_price, yield_change)
    }

    // ==================== Helper Methods ====================

    /// Parses the day count convention string to its enum.
    fn parse_day_count(&self) -> BondResult<DayCountConvention> {
        let dcc_str = self.day_count_convention();
        DayCountConvention::from_str(dcc_str).map_err(|e| BondError::InvalidSpec {
            reason: format!("unknown day count convention '{dcc_str}': {e}"),
        })
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
        let ytm_result = bond
            .yield_to_maturity(settlement, clean_price, Frequency::SemiAnnual)
            .unwrap();

        // Calculate clean price from YTM
        let calculated_clean = bond
            .clean_price_from_yield(settlement, ytm_result.yield_value, Frequency::SemiAnnual)
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

        let mod_dur = bond.modified_duration(settlement, ytm, Frequency::SemiAnnual);
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

        let mod_dur = bond
            .modified_duration(settlement, ytm, Frequency::SemiAnnual)
            .unwrap();
        let eff_dur = bond
            .effective_duration(settlement, ytm, Frequency::SemiAnnual, 10.0)
            .unwrap();

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
        let change = bond
            .estimate_price_change(
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
}
