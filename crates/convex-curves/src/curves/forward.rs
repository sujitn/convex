//! Forward rate curve.

use rust_decimal::Decimal;

use convex_core::Date;

use crate::curves::ZeroCurve;
use crate::error::CurveResult;

/// A forward rate curve.
///
/// Computes forward rates from an underlying zero curve.
#[derive(Debug, Clone)]
pub struct ForwardCurve {
    /// Underlying zero curve.
    zero_curve: ZeroCurve,
    /// Tenor for forward rates (in months).
    tenor_months: u32,
}

impl ForwardCurve {
    /// Creates a forward curve from a zero curve with a specific tenor.
    ///
    /// # Arguments
    ///
    /// * `zero_curve` - The underlying zero curve
    /// * `tenor_months` - Forward rate tenor (e.g., 3 for 3-month forwards)
    #[must_use]
    pub fn new(zero_curve: ZeroCurve, tenor_months: u32) -> Self {
        Self {
            zero_curve,
            tenor_months,
        }
    }

    /// Returns the reference date.
    #[must_use]
    pub fn reference_date(&self) -> Date {
        self.zero_curve.reference_date()
    }

    /// Returns the forward tenor in months.
    #[must_use]
    pub fn tenor_months(&self) -> u32 {
        self.tenor_months
    }

    /// Returns the instantaneous forward rate at a given date.
    ///
    /// Calculated as: `f(t) = -d(ln(DF))/dt`
    pub fn instantaneous_forward(&self, date: Date) -> CurveResult<Decimal> {
        let h = 1.0 / 365.0; // One day step

        let df1 = self.zero_curve.discount_factor_at(date)?;
        let date_plus = date.add_days(1);
        let df2 = self.zero_curve.discount_factor_at(date_plus)?;

        let df1_f64 = df1.to_string().parse::<f64>().unwrap_or(1.0);
        let df2_f64 = df2.to_string().parse::<f64>().unwrap_or(1.0);

        if df1_f64 <= 0.0 || df2_f64 <= 0.0 {
            return Ok(Decimal::ZERO);
        }

        // f(t) ≈ -[ln(DF(t+h)) - ln(DF(t))] / h
        let fwd = -(df2_f64.ln() - df1_f64.ln()) / h;

        Ok(Decimal::from_f64_retain(fwd).unwrap_or(Decimal::ZERO))
    }

    /// Returns the forward rate from start date for the specified tenor.
    ///
    /// # Arguments
    ///
    /// * `start` - Start date of the forward period
    pub fn forward_rate(&self, start: Date) -> CurveResult<Decimal> {
        let end = start.add_months(self.tenor_months as i32)?;

        let df_start = self.zero_curve.discount_factor_at(start)?;
        let df_end = self.zero_curve.discount_factor_at(end)?;

        let df_start_f64 = df_start.to_string().parse::<f64>().unwrap_or(1.0);
        let df_end_f64 = df_end.to_string().parse::<f64>().unwrap_or(1.0);

        if df_end_f64 <= 0.0 {
            return Ok(Decimal::ZERO);
        }

        // Forward rate from DFs: F = (DF_start/DF_end - 1) / tau
        let tau = start.days_between(&end) as f64 / 360.0; // ACT/360
        let fwd = (df_start_f64 / df_end_f64 - 1.0) / tau;

        Ok(Decimal::from_f64_retain(fwd).unwrap_or(Decimal::ZERO))
    }

    /// Returns the underlying zero curve.
    #[must_use]
    pub fn zero_curve(&self) -> &ZeroCurve {
        &self.zero_curve
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curves::ZeroCurveBuilder;
    use crate::interpolation::InterpolationMethod;
    use rust_decimal_macros::dec;

    #[test]
    fn test_forward_curve() {
        let zero_curve = ZeroCurveBuilder::new()
            .reference_date(Date::from_ymd(2025, 1, 1).unwrap())
            .add_rate(Date::from_ymd(2025, 4, 1).unwrap(), dec!(0.04))
            .add_rate(Date::from_ymd(2025, 7, 1).unwrap(), dec!(0.045))
            .add_rate(Date::from_ymd(2026, 1, 1).unwrap(), dec!(0.05))
            .interpolation(InterpolationMethod::Linear)
            .build()
            .unwrap();

        let fwd_curve = ForwardCurve::new(zero_curve, 3);

        let fwd_rate = fwd_curve
            .forward_rate(Date::from_ymd(2025, 4, 1).unwrap())
            .unwrap();

        // Forward rate should be positive
        assert!(fwd_rate > Decimal::ZERO);
    }

    #[test]
    fn test_instantaneous_forward() {
        let zero_curve = ZeroCurveBuilder::new()
            .reference_date(Date::from_ymd(2025, 1, 1).unwrap())
            .add_rate(Date::from_ymd(2025, 7, 1).unwrap(), dec!(0.04))
            .add_rate(Date::from_ymd(2026, 1, 1).unwrap(), dec!(0.05))
            .interpolation(InterpolationMethod::Linear)
            .build()
            .unwrap();

        let fwd_curve = ForwardCurve::new(zero_curve, 3);

        let inst_fwd = fwd_curve
            .instantaneous_forward(Date::from_ymd(2025, 7, 1).unwrap())
            .unwrap();

        // Should be positive and reasonable
        assert!(inst_fwd > Decimal::ZERO && inst_fwd < dec!(0.2));
    }
}
