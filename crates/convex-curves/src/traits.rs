//! Core traits for yield curve operations.
//!
//! This module defines the primary [`Curve`] trait that all yield curve
//! implementations must satisfy. The trait provides a complete API for
//! retrieving discount factors, zero rates, and forward rates.

use convex_core::Date;

use crate::compounding::Compounding;
use crate::error::CurveResult;

/// The core trait for yield curves.
///
/// A yield curve provides the fundamental operations needed for discounting
/// cash flows and computing forward rates. All curve types in the library
/// implement this trait, enabling generic pricing and risk calculations.
///
/// # Required Methods
///
/// Implementations must provide:
/// - [`discount_factor`](Curve::discount_factor): The primary method for discounting
/// - [`reference_date`](Curve::reference_date): The curve's valuation date
/// - [`max_date`](Curve::max_date): The last date with market data
///
/// # Derived Methods
///
/// The trait provides default implementations for:
/// - [`zero_rate`](Curve::zero_rate): Derived from discount factors
/// - [`forward_rate`](Curve::forward_rate): Forward rate between two dates
/// - [`instantaneous_forward`](Curve::instantaneous_forward): Limiting forward rate
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::{Curve, Compounding};
///
/// fn price_zero_coupon<C: Curve>(
///     curve: &C,
///     maturity: Date,
///     face_value: f64,
/// ) -> CurveResult<f64> {
///     let df = curve.discount_factor(curve.year_fraction(maturity))?;
///     Ok(face_value * df)
/// }
/// ```
pub trait Curve: Send + Sync {
    /// Returns the discount factor from the reference date to time `t`.
    ///
    /// The discount factor represents the present value of $1 received
    /// at time `t` years from the reference date.
    ///
    /// # Arguments
    ///
    /// * `t` - Time in years from the reference date
    ///
    /// # Returns
    ///
    /// A discount factor, typically between 0 and 1 for positive rates.
    /// Returns 1.0 for t ≤ 0.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `t` is outside the curve's valid range and extrapolation is disabled
    /// - The curve data is corrupted
    fn discount_factor(&self, t: f64) -> CurveResult<f64>;

    /// Returns the zero rate at time `t` with the specified compounding.
    ///
    /// The zero rate is the constant rate that, when applied from the
    /// reference date to time `t`, gives the discount factor.
    ///
    /// # Arguments
    ///
    /// * `t` - Time in years from the reference date
    /// * `compounding` - The compounding convention for the rate
    ///
    /// # Default Implementation
    ///
    /// Computes the rate from the discount factor using the compounding formula.
    fn zero_rate(&self, t: f64, compounding: Compounding) -> CurveResult<f64> {
        let df = self.discount_factor(t)?;
        Ok(compounding.zero_rate(df, t))
    }

    /// Returns the simply-compounded forward rate between times `t1` and `t2`.
    ///
    /// This is the rate that can be locked in today for a deposit starting
    /// at `t1` and maturing at `t2`.
    ///
    /// # Arguments
    ///
    /// * `t1` - Start time in years
    /// * `t2` - End time in years (must be > t1)
    ///
    /// # Formula
    ///
    /// `F(t1, t2) = (DF(t1) / DF(t2) - 1) / (t2 - t1)`
    ///
    /// # Default Implementation
    ///
    /// Computes the forward rate from the ratio of discount factors.
    fn forward_rate(&self, t1: f64, t2: f64) -> CurveResult<f64> {
        if t2 <= t1 {
            return Ok(0.0);
        }

        let df1 = self.discount_factor(t1)?;
        let df2 = self.discount_factor(t2)?;

        if df2 <= 0.0 {
            return Ok(0.0);
        }

        let tau = t2 - t1;
        Ok((df1 / df2 - 1.0) / tau)
    }

    /// Returns the instantaneous forward rate at time `t`.
    ///
    /// This is the limiting forward rate as the forward period shrinks to zero:
    /// `f(t) = lim_{Δ→0} F(t, t+Δ) = -d(ln DF(t))/dt`
    ///
    /// # Arguments
    ///
    /// * `t` - Time in years from the reference date
    ///
    /// # Default Implementation
    ///
    /// Uses numerical differentiation with a small step size.
    fn instantaneous_forward(&self, t: f64) -> CurveResult<f64> {
        let h = 1.0 / 365.0; // One day step

        let df = self.discount_factor(t)?;
        let df_plus = self.discount_factor(t + h)?;

        if df <= 0.0 || df_plus <= 0.0 {
            return Ok(0.0);
        }

        // f(t) ≈ -[ln(DF(t+h)) - ln(DF(t))] / h
        Ok(-(df_plus.ln() - df.ln()) / h)
    }

    /// Returns the curve's reference (valuation) date.
    ///
    /// All times are measured from this date. A time of 1.0 represents
    /// one year from the reference date.
    fn reference_date(&self) -> Date;

    /// Returns the maximum date for which market data is available.
    ///
    /// Beyond this date, the curve may extrapolate (if enabled) or
    /// return an error.
    fn max_date(&self) -> Date;

    /// Returns the year fraction from the reference date to the given date.
    ///
    /// Uses ACT/365 Fixed convention by default.
    fn year_fraction(&self, date: Date) -> f64 {
        let ref_date = self.reference_date();
        ref_date.days_between(&date) as f64 / 365.0
    }

    /// Returns the discount factor for a specific date.
    ///
    /// Convenience method that converts the date to a year fraction.
    fn discount_factor_at(&self, date: Date) -> CurveResult<f64> {
        let t = self.year_fraction(date);
        self.discount_factor(t)
    }

    /// Returns the zero rate for a specific date.
    ///
    /// Convenience method that converts the date to a year fraction.
    fn zero_rate_at(&self, date: Date, compounding: Compounding) -> CurveResult<f64> {
        let t = self.year_fraction(date);
        self.zero_rate(t, compounding)
    }

    /// Returns the forward rate between two dates.
    ///
    /// Convenience method that converts dates to year fractions.
    fn forward_rate_between(&self, start: Date, end: Date) -> CurveResult<f64> {
        let t1 = self.year_fraction(start);
        let t2 = self.year_fraction(end);
        self.forward_rate(t1, t2)
    }

    /// Returns the par yield at maturity `t` with semi-annual compounding.
    ///
    /// The par yield is the coupon rate that makes a bond price equal to par (100).
    /// This is equivalent to the CMT (Constant Maturity Treasury) rate published
    /// by the US Treasury.
    ///
    /// # Formula
    ///
    /// For semi-annual coupon bonds:
    /// ```text
    /// Par Yield = 2 × (1 - DF(T)) / Σ DF(ti)
    /// ```
    /// where ti = 0.5, 1.0, 1.5, ..., T are the coupon payment dates.
    ///
    /// # Arguments
    ///
    /// * `t` - Maturity in years
    ///
    /// # Returns
    ///
    /// The par yield as a decimal (e.g., 0.045 for 4.5%).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Get the 10-year par yield (comparable to 10Y CMT)
    /// let par_10y = curve.par_yield(10.0)?;
    /// ```
    fn par_yield(&self, t: f64) -> CurveResult<f64> {
        // For very short maturities (< 6 months), use zero rate
        if t <= 0.5 {
            return self.zero_rate(t, Compounding::SemiAnnual);
        }

        let df_t = self.discount_factor(t)?;

        // Sum discount factors at each semi-annual coupon date
        let mut sum_df = 0.0;
        let mut coupon_time = 0.5;
        while coupon_time <= t + 0.001 {
            sum_df += self.discount_factor(coupon_time)?;
            coupon_time += 0.5;
        }

        if sum_df <= 0.0 {
            return self.zero_rate(t, Compounding::SemiAnnual);
        }

        // Par yield = 2 × (1 - DF(T)) / Σ DF(ti)
        Ok(2.0 * (1.0 - df_t) / sum_df)
    }

    /// Returns the par yield for a specific date.
    ///
    /// Convenience method that converts the date to a year fraction.
    fn par_yield_at(&self, date: Date) -> CurveResult<f64> {
        let t = self.year_fraction(date);
        self.par_yield(t)
    }
}

/// Extension trait for curves that support bumping/shifting.
pub trait BumpableCurve: Curve {
    /// Returns a parallel-shifted curve.
    ///
    /// # Arguments
    ///
    /// * `shift` - The shift in basis points (1bp = 0.0001)
    fn parallel_shift(&self, shift_bps: f64) -> Box<dyn Curve>;

    /// Returns a curve with key rate shifts.
    ///
    /// # Arguments
    ///
    /// * `shifts` - Map of tenor (in years) to shift (in bps)
    fn key_rate_shift(&self, shifts: &[(f64, f64)]) -> Box<dyn Curve>;
}

/// Extension trait for curves that can be frozen at a point.
pub trait FreezeableCurve: Curve {
    /// Creates a frozen curve where all forwards equal the forward at time `t`.
    fn freeze_at(&self, t: f64) -> Box<dyn Curve>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A simple flat curve for testing
    struct FlatCurve {
        rate: f64,
        ref_date: Date,
    }

    impl FlatCurve {
        fn new(rate: f64, ref_date: Date) -> Self {
            Self { rate, ref_date }
        }
    }

    impl Curve for FlatCurve {
        fn discount_factor(&self, t: f64) -> CurveResult<f64> {
            Ok((-self.rate * t).exp())
        }

        fn reference_date(&self) -> Date {
            self.ref_date
        }

        fn max_date(&self) -> Date {
            self.ref_date.add_years(100).unwrap()
        }
    }

    #[test]
    fn test_flat_curve_discount_factor() {
        let curve = FlatCurve::new(0.05, Date::from_ymd(2025, 1, 1).unwrap());
        let df = curve.discount_factor(1.0).unwrap();
        assert!((df - (-0.05_f64).exp()).abs() < 1e-10);
    }

    #[test]
    fn test_zero_rate_from_df() {
        let curve = FlatCurve::new(0.05, Date::from_ymd(2025, 1, 1).unwrap());
        let rate = curve.zero_rate(1.0, Compounding::Continuous).unwrap();
        assert!((rate - 0.05).abs() < 1e-10);
    }

    #[test]
    fn test_forward_rate_flat_curve() {
        let curve = FlatCurve::new(0.05, Date::from_ymd(2025, 1, 1).unwrap());
        // For a flat curve, all forward rates should equal the flat rate
        let fwd = curve.forward_rate(1.0, 2.0).unwrap();
        // Simple forward rate from continuous rate
        let df1 = curve.discount_factor(1.0).unwrap();
        let df2 = curve.discount_factor(2.0).unwrap();
        let expected = (df1 / df2 - 1.0) / 1.0;
        assert!((fwd - expected).abs() < 1e-10);
    }

    #[test]
    fn test_instantaneous_forward() {
        let curve = FlatCurve::new(0.05, Date::from_ymd(2025, 1, 1).unwrap());
        let inst_fwd = curve.instantaneous_forward(1.0).unwrap();
        // For a flat continuous curve, instantaneous forward = rate
        assert!((inst_fwd - 0.05).abs() < 1e-4);
    }

    #[test]
    fn test_year_fraction() {
        let curve = FlatCurve::new(0.05, Date::from_ymd(2025, 1, 1).unwrap());
        let date = Date::from_ymd(2026, 1, 1).unwrap();
        let yf = curve.year_fraction(date);
        assert!((yf - 1.0).abs() < 0.01); // 365 days / 365 ≈ 1.0
    }

    #[test]
    fn test_discount_factor_at_date() {
        let curve = FlatCurve::new(0.05, Date::from_ymd(2025, 1, 1).unwrap());
        let date = Date::from_ymd(2026, 1, 1).unwrap();
        let df = curve.discount_factor_at(date).unwrap();
        assert!((df - (-0.05_f64).exp()).abs() < 0.01);
    }

    #[test]
    fn test_par_yield_flat_curve() {
        // For a flat curve, par yield should equal the flat rate (approximately)
        let curve = FlatCurve::new(0.05, Date::from_ymd(2025, 1, 1).unwrap());

        // 2-year par yield
        let par_2y = curve.par_yield(2.0).unwrap();
        // Par yield should be close to 5% for a flat 5% curve
        // (small difference due to continuous vs semi-annual compounding)
        assert!((par_2y - 0.05).abs() < 0.005);

        // 10-year par yield
        let par_10y = curve.par_yield(10.0).unwrap();
        assert!((par_10y - 0.05).abs() < 0.005);
    }

    #[test]
    fn test_par_yield_short_maturity() {
        let curve = FlatCurve::new(0.04, Date::from_ymd(2025, 1, 1).unwrap());

        // For maturities <= 0.5 years, par_yield returns zero rate
        let par_3m = curve.par_yield(0.25).unwrap();
        let zero_3m = curve.zero_rate(0.25, Compounding::SemiAnnual).unwrap();
        assert!((par_3m - zero_3m).abs() < 1e-10);
    }

    #[test]
    fn test_par_yield_formula() {
        // Verify the par yield formula: Par = 2 * (1 - DF(T)) / Σ DF(ti)
        let curve = FlatCurve::new(0.04, Date::from_ymd(2025, 1, 1).unwrap());

        let t = 2.0; // 2-year maturity
        let df_t = curve.discount_factor(t).unwrap();

        // Sum discount factors at 0.5, 1.0, 1.5, 2.0
        let sum_df = curve.discount_factor(0.5).unwrap()
            + curve.discount_factor(1.0).unwrap()
            + curve.discount_factor(1.5).unwrap()
            + curve.discount_factor(2.0).unwrap();

        let expected_par = 2.0 * (1.0 - df_t) / sum_df;
        let actual_par = curve.par_yield(t).unwrap();

        assert!((actual_par - expected_par).abs() < 1e-10);
    }
}
