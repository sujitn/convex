//! Discount factor curve.

use rust_decimal::Decimal;

use convex_core::{ConvexResult, Date};
use convex_core::traits::YieldCurve;

use crate::curves::ZeroCurve;
use crate::error::CurveResult;

/// A discount factor curve.
///
/// Wraps a zero curve and provides discount factor-centric API.
#[derive(Debug, Clone)]
pub struct DiscountCurve {
    /// Underlying zero curve.
    zero_curve: ZeroCurve,
}

impl DiscountCurve {
    /// Creates a discount curve from a zero curve.
    #[must_use]
    pub fn from_zero_curve(zero_curve: ZeroCurve) -> Self {
        Self { zero_curve }
    }

    /// Returns the reference date.
    #[must_use]
    pub fn reference_date(&self) -> Date {
        self.zero_curve.reference_date()
    }

    /// Returns the discount factor for a given date.
    pub fn discount_factor(&self, date: Date) -> CurveResult<Decimal> {
        self.zero_curve.discount_factor_at(date)
    }

    /// Returns the forward discount factor between two dates.
    ///
    /// Forward DF = DF(end) / DF(start)
    pub fn forward_discount_factor(&self, start: Date, end: Date) -> CurveResult<Decimal> {
        let df_start = self.discount_factor(start)?;
        let df_end = self.discount_factor(end)?;

        if df_start == Decimal::ZERO {
            return Ok(Decimal::ZERO);
        }

        Ok(df_end / df_start)
    }

    /// Returns the underlying zero curve.
    #[must_use]
    pub fn zero_curve(&self) -> &ZeroCurve {
        &self.zero_curve
    }
}

impl YieldCurve for DiscountCurve {
    fn reference_date(&self) -> Date {
        self.zero_curve.reference_date()
    }

    fn discount_factor(&self, date: Date) -> ConvexResult<Decimal> {
        self.zero_curve.discount_factor(date)
    }

    fn zero_rate(&self, date: Date) -> ConvexResult<Decimal> {
        self.zero_curve.zero_rate(date)
    }

    fn max_date(&self) -> Date {
        self.zero_curve.max_date()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curves::ZeroCurveBuilder;
    use crate::interpolation::InterpolationMethod;
    use rust_decimal_macros::dec;

    #[test]
    fn test_discount_curve() {
        let zero_curve = ZeroCurveBuilder::new()
            .reference_date(Date::from_ymd(2025, 1, 1).unwrap())
            .add_rate(Date::from_ymd(2025, 7, 1).unwrap(), dec!(0.04))
            .add_rate(Date::from_ymd(2026, 1, 1).unwrap(), dec!(0.05))
            .interpolation(InterpolationMethod::Linear)
            .build()
            .unwrap();

        let discount_curve = DiscountCurve::from_zero_curve(zero_curve);

        let df = discount_curve
            .discount_factor(Date::from_ymd(2025, 7, 1).unwrap())
            .unwrap();

        // DF should be less than 1
        assert!(df < Decimal::ONE);
    }

    #[test]
    fn test_forward_discount_factor() {
        let zero_curve = ZeroCurveBuilder::new()
            .reference_date(Date::from_ymd(2025, 1, 1).unwrap())
            .add_rate(Date::from_ymd(2025, 7, 1).unwrap(), dec!(0.04))
            .add_rate(Date::from_ymd(2026, 1, 1).unwrap(), dec!(0.05))
            .interpolation(InterpolationMethod::Linear)
            .build()
            .unwrap();

        let discount_curve = DiscountCurve::from_zero_curve(zero_curve);

        let fwd_df = discount_curve
            .forward_discount_factor(
                Date::from_ymd(2025, 7, 1).unwrap(),
                Date::from_ymd(2026, 1, 1).unwrap(),
            )
            .unwrap();

        // Forward DF should be less than 1
        assert!(fwd_df < Decimal::ONE);
    }
}
