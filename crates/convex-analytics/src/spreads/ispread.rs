//! I-spread (interpolated spread) calculator.
//!
//! The I-spread is the yield spread of a bond over the interpolated
//! swap curve at the bond's maturity.

use rust_decimal::prelude::*;
use rust_decimal::Decimal;

use convex_bonds::traits::{Bond, FixedCouponBond};
use convex_core::types::{Date, Spread, SpreadType, Yield};
use convex_curves::Curve;

use crate::error::{AnalyticsError, AnalyticsResult};

/// I-spread calculator for fixed rate bonds.
///
/// Calculates the spread over the interpolated swap rate at maturity.
///
/// # Formula
///
/// I-spread = Bond YTM - Swap Rate at Maturity
///
/// Unlike Z-spread which discounts each cash flow individually, I-spread
/// is a simple yield difference at the maturity point.
pub struct ISpreadCalculator<'a> {
    /// Reference to the swap curve.
    swap_curve: &'a dyn Curve,
}

impl std::fmt::Debug for ISpreadCalculator<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ISpreadCalculator").finish_non_exhaustive()
    }
}

impl<'a> ISpreadCalculator<'a> {
    /// Creates a new I-spread calculator.
    ///
    /// # Arguments
    ///
    /// * `swap_curve` - The swap rate curve
    #[must_use]
    pub fn new(swap_curve: &'a dyn Curve) -> Self {
        Self { swap_curve }
    }

    /// Calculates the I-spread for a bond.
    ///
    /// # Arguments
    ///
    /// * `bond` - The fixed rate bond
    /// * `bond_yield` - The bond's yield to maturity
    /// * `settlement` - Settlement date
    ///
    /// # Returns
    ///
    /// The I-spread as a `Spread` in basis points.
    ///
    /// # Errors
    ///
    /// Returns `AnalyticsError` if:
    /// - Settlement is at or after maturity
    /// - Swap curve interpolation fails
    pub fn calculate<B: Bond + FixedCouponBond>(
        &self,
        bond: &B,
        bond_yield: Yield,
        settlement: Date,
    ) -> AnalyticsResult<Spread> {
        let maturity = bond.maturity().ok_or_else(|| {
            AnalyticsError::InvalidInput("Bond has no maturity (perpetual)".to_string())
        })?;

        if settlement >= maturity {
            return Err(AnalyticsError::InvalidSettlement {
                settlement: settlement.to_string(),
                maturity: maturity.to_string(),
            });
        }

        // Calculate years to maturity from curve reference date
        let ref_date = self.swap_curve.reference_date();
        let years_to_maturity = ref_date.days_between(&maturity) as f64 / 365.0;

        // Get swap rate at maturity
        let swap_rate = self
            .swap_curve
            .zero_rate(years_to_maturity, convex_curves::Compounding::SemiAnnual)
            .map_err(|e| AnalyticsError::CurveError(e.to_string()))?;

        // I-spread = Bond yield - Swap rate
        let bond_yield_f64 = bond_yield.value().to_f64().unwrap_or(0.0);
        let spread = bond_yield_f64 - swap_rate;
        let spread_bps = (spread * 10_000.0).round();

        Ok(Spread::new(
            Decimal::from_f64_retain(spread_bps).unwrap_or_default(),
            SpreadType::ISpread,
        ))
    }

    /// Returns the swap rate at the bond's maturity.
    ///
    /// Useful for debugging or displaying the benchmark rate.
    pub fn swap_rate_at_maturity(&self, maturity: Date) -> AnalyticsResult<f64> {
        let ref_date = self.swap_curve.reference_date();
        let years_to_maturity = ref_date.days_between(&maturity) as f64 / 365.0;

        self.swap_curve
            .zero_rate(years_to_maturity, convex_curves::Compounding::SemiAnnual)
            .map_err(|e| AnalyticsError::CurveError(e.to_string()))
    }

    /// Calculates implied yield from I-spread.
    ///
    /// Given an I-spread, returns what the bond's yield would be.
    ///
    /// # Arguments
    ///
    /// * `i_spread` - I-spread in basis points
    /// * `maturity` - Bond maturity date
    ///
    /// # Returns
    ///
    /// Implied bond yield.
    pub fn implied_yield(&self, i_spread: Spread, maturity: Date) -> AnalyticsResult<Yield> {
        let swap_rate = self.swap_rate_at_maturity(maturity)?;
        let spread_decimal = i_spread.as_decimal().to_f64().unwrap_or(0.0) / 10_000.0;

        let implied = swap_rate + spread_decimal;
        Ok(Yield::new(
            Decimal::from_f64_retain(implied).unwrap_or_default(),
            convex_core::types::Compounding::SemiAnnual,
        ))
    }
}

/// Convenience function to calculate I-spread.
///
/// # Arguments
///
/// * `bond` - The fixed rate bond
/// * `bond_yield` - Bond's yield to maturity
/// * `swap_curve` - Swap rate curve
/// * `settlement` - Settlement date
///
/// # Returns
///
/// I-spread in basis points.
pub fn i_spread<B: Bond + FixedCouponBond>(
    bond: &B,
    bond_yield: Yield,
    swap_curve: &dyn Curve,
    settlement: Date,
) -> AnalyticsResult<Spread> {
    ISpreadCalculator::new(swap_curve).calculate(bond, bond_yield, settlement)
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_core::types::Compounding;
    use convex_curves::curves::DiscountCurveBuilder;
    use rust_decimal_macros::dec;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    fn create_swap_curve(rate: f64) -> impl Curve {
        let ref_date = date(2024, 1, 15);
        DiscountCurveBuilder::new(ref_date)
            .add_pillar(1.0, (-rate * 1.0).exp())
            .add_pillar(2.0, (-rate * 2.0).exp())
            .add_pillar(5.0, (-rate * 5.0).exp())
            .add_pillar(10.0, (-rate * 10.0).exp())
            .add_pillar(30.0, (-rate * 30.0).exp())
            .with_extrapolation()
            .build()
            .unwrap()
    }

    // Mock bond for testing
    struct MockBond {
        maturity: Date,
        calendar: convex_bonds::types::CalendarId,
    }

    impl MockBond {
        fn new(maturity: Date) -> Self {
            Self {
                maturity,
                calendar: convex_bonds::types::CalendarId::us_government(),
            }
        }
    }

    impl Bond for MockBond {
        fn identifiers(&self) -> &convex_bonds::types::BondIdentifiers {
            unimplemented!("Not needed for test")
        }

        fn bond_type(&self) -> convex_bonds::types::BondType {
            convex_bonds::types::BondType::FixedRateCorporate
        }

        fn currency(&self) -> convex_core::Currency {
            convex_core::Currency::USD
        }

        fn maturity(&self) -> Option<Date> {
            Some(self.maturity)
        }

        fn issue_date(&self) -> Date {
            date(2020, 1, 15)
        }

        fn first_settlement_date(&self) -> Date {
            date(2020, 1, 15)
        }

        fn dated_date(&self) -> Date {
            date(2020, 1, 15)
        }

        fn face_value(&self) -> Decimal {
            dec!(100)
        }

        fn frequency(&self) -> convex_core::types::Frequency {
            convex_core::types::Frequency::SemiAnnual
        }

        fn cash_flows(&self, _from: Date) -> Vec<convex_bonds::traits::BondCashFlow> {
            Vec::new()
        }

        fn next_coupon_date(&self, _after: Date) -> Option<Date> {
            None
        }

        fn previous_coupon_date(&self, _before: Date) -> Option<Date> {
            None
        }

        fn accrued_interest(&self, _settlement: Date) -> Decimal {
            dec!(0)
        }

        fn day_count_convention(&self) -> &'static str {
            "ACT/ACT"
        }

        fn calendar(&self) -> &convex_bonds::types::CalendarId {
            &self.calendar
        }
    }

    impl FixedCouponBond for MockBond {
        fn coupon_rate(&self) -> Decimal {
            dec!(0.05)
        }

        fn coupon_frequency(&self) -> u32 {
            2
        }

        fn first_coupon_date(&self) -> Option<Date> {
            None
        }

        fn last_coupon_date(&self) -> Option<Date> {
            None
        }
    }

    #[test]
    fn test_i_spread_calculation() {
        let swap_curve = create_swap_curve(0.04);
        let calc = ISpreadCalculator::new(&swap_curve);

        let bond = MockBond::new(date(2029, 1, 15));
        let settlement = date(2024, 1, 17);

        // Bond yield = 5%, swap rate ≈ 4%
        let bond_yield = Yield::new(dec!(0.05), Compounding::SemiAnnual);
        let spread = calc.calculate(&bond, bond_yield, settlement).unwrap();

        // I-spread should be ~100 bps (5% - 4%)
        // Allow for some tolerance due to curve interpolation
        assert!(
            spread.as_bps() > dec!(80) && spread.as_bps() < dec!(120),
            "Expected ~100 bps, got {}",
            spread.as_bps()
        );
    }

    #[test]
    fn test_i_spread_zero() {
        let swap_curve = create_swap_curve(0.05);
        let calc = ISpreadCalculator::new(&swap_curve);

        let bond = MockBond::new(date(2029, 1, 15));
        let settlement = date(2024, 1, 17);

        // Bond yield = 5%, swap rate ≈ 5%
        let bond_yield = Yield::new(dec!(0.05), Compounding::SemiAnnual);
        let spread = calc.calculate(&bond, bond_yield, settlement).unwrap();

        // I-spread should be near zero
        assert!(
            spread.as_bps().abs() < dec!(20),
            "Expected near-zero spread, got {}",
            spread.as_bps()
        );
    }

    #[test]
    fn test_i_spread_negative() {
        let swap_curve = create_swap_curve(0.06);
        let calc = ISpreadCalculator::new(&swap_curve);

        let bond = MockBond::new(date(2029, 1, 15));
        let settlement = date(2024, 1, 17);

        // Bond yield = 5%, swap rate ≈ 6%
        let bond_yield = Yield::new(dec!(0.05), Compounding::SemiAnnual);
        let spread = calc.calculate(&bond, bond_yield, settlement).unwrap();

        // I-spread should be negative (~-100 bps)
        assert!(
            spread.as_bps() < Decimal::ZERO,
            "Expected negative spread, got {}",
            spread.as_bps()
        );
    }

    #[test]
    fn test_swap_rate_at_maturity() {
        let swap_curve = create_swap_curve(0.05);
        let calc = ISpreadCalculator::new(&swap_curve);

        let maturity = date(2029, 1, 15);
        let swap_rate = calc.swap_rate_at_maturity(maturity).unwrap();

        // Should be around 5%
        assert!(
            (swap_rate - 0.05).abs() < 0.01,
            "Expected ~5%, got {:.4}%",
            swap_rate * 100.0
        );
    }

    #[test]
    fn test_implied_yield() {
        let swap_curve = create_swap_curve(0.04);
        let calc = ISpreadCalculator::new(&swap_curve);

        let maturity = date(2029, 1, 15);
        let i_spread = Spread::new(dec!(100), SpreadType::ISpread); // 100 bps

        let implied = calc.implied_yield(i_spread, maturity).unwrap();

        // Should be swap rate + 100 bps ≈ 5%
        let implied_f64 = implied.value().to_f64().unwrap();
        assert!(
            (implied_f64 - 0.05).abs() < 0.01,
            "Expected ~5%, got {:.4}%",
            implied_f64 * 100.0
        );
    }

    #[test]
    fn test_settlement_after_maturity() {
        let swap_curve = create_swap_curve(0.05);
        let calc = ISpreadCalculator::new(&swap_curve);

        let bond = MockBond::new(date(2024, 1, 15)); // Already matured
        let settlement = date(2024, 6, 15);
        let bond_yield = Yield::new(dec!(0.05), Compounding::SemiAnnual);

        let result = calc.calculate(&bond, bond_yield, settlement);
        assert!(result.is_err());
    }
}
