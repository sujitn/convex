//! `RateCurve<T>`: interest-rate semantics over any `TermStructure`. Implements
//! `convex_core::traits::YieldCurve` so callers can pass `&dyn YieldCurve`.

use convex_core::daycounts::DayCount;
use convex_core::error::ConvexResult;
use convex_core::traits::YieldCurve;
use convex_core::types::{Compounding, Date};
use rust_decimal::Decimal;

use crate::conversion::ValueConverter;
use crate::error::{CurveError, CurveResult};
use crate::term_structure::TermStructure;
use crate::value_type::ValueType;

/// Interest-rate operations over any term structure, regardless of stored
/// value type (zero rates, discount factors, …).
///
/// ```rust,ignore
/// use convex_curves::{RateCurve, DiscreteCurve};
///
/// let curve = DiscreteCurve::new(...)?;
/// let rate_curve = RateCurve::new(curve);
///
/// // Get discount factor
/// let df = rate_curve.discount_factor(settlement_date)?;
///
/// // Get zero rate with specific compounding
/// let zero = rate_curve.zero_rate(maturity_date, Compounding::SemiAnnual)?;
///
/// // Get forward rate
/// let fwd = rate_curve.forward_rate(start, end, Compounding::Simple)?;
/// ```
#[derive(Clone, Debug)]
pub struct RateCurve<T: TermStructure> {
    /// The underlying term structure.
    inner: T,
}

impl<T: TermStructure> RateCurve<T> {
    /// Creates a new rate curve wrapper.
    pub fn new(inner: T) -> Self {
        Self { inner }
    }

    /// Returns a reference to the underlying term structure.
    #[must_use]
    pub fn inner(&self) -> &T {
        &self.inner
    }

    /// Returns the reference date.
    #[must_use]
    pub fn reference_date(&self) -> Date {
        self.inner.reference_date()
    }

    /// Converts a date to a tenor in years.
    fn date_to_tenor(&self, date: Date) -> CurveResult<f64> {
        self.inner.date_to_tenor(date)
    }

    /// Returns the discount factor at the given date.
    ///
    /// P(T) = present value of $1 received at time T.
    ///
    /// This method handles conversion from whatever value type the
    /// underlying curve stores.
    pub fn discount_factor(&self, date: Date) -> CurveResult<f64> {
        let t = self.date_to_tenor(date)?;
        self.discount_factor_at_tenor(t)
    }

    /// Returns the discount factor at a tenor (years).
    pub fn discount_factor_at_tenor(&self, t: f64) -> CurveResult<f64> {
        if !t.is_finite() || t < 0.0 {
            return Err(CurveError::invalid_value(
                "discount tenor must be finite and nonnegative",
            ));
        }
        if t == 0.0 {
            return Ok(1.0);
        }
        let value = self.inner.try_value_at(t)?;
        let value_type = self.inner.value_type();
        let discount = match value_type {
            ValueType::DiscountFactor | ValueType::SurvivalProbability => value,
            ValueType::ZeroRate { compounding, .. } => {
                crate::contracts::discount_from_rate(value, t, compounding)
                    .map_err(|error| CurveError::invalid_value(error.to_string()))?
            }
            _ => {
                return Err(CurveError::incompatible_value_type(
                    "DiscountFactor or ZeroRate",
                    format!("{value_type:?}"),
                ))
            }
        };
        if !discount.is_finite() || discount <= 0.0 {
            return Err(CurveError::invalid_value(
                "discount factor must be positive and finite without nonzero underflow",
            ));
        }
        Ok(discount)
    }

    /// Returns the continuously compounded zero rate at the given date.
    pub fn zero_rate_continuous(&self, date: Date) -> CurveResult<f64> {
        self.zero_rate(date, Compounding::Continuous)
    }

    /// Returns the zero rate at the given date with specified compounding.
    ///
    /// # Arguments
    ///
    /// * `date` - Target date
    /// * `compounding` - Desired compounding convention
    pub fn zero_rate(&self, date: Date, compounding: Compounding) -> CurveResult<f64> {
        let t = self.date_to_tenor(date)?;
        self.zero_rate_at_tenor(t, compounding)
    }

    /// Returns the zero rate at a tenor with specified compounding. At the
    /// origin a supplied zero quotation defines its limit. A discount source
    /// requires its actual normalized origin and analytic derivative.
    pub fn zero_rate_at_tenor(&self, t: f64, compounding: Compounding) -> CurveResult<f64> {
        if !t.is_finite() || t < 0.0 {
            return Err(CurveError::invalid_value(
                "zero-rate tenor must be finite and nonnegative",
            ));
        }
        let value = self.inner.try_value_at(t)?;
        let value_type = self.inner.value_type();
        if t == 0.0 {
            return self.origin_zero_rate(value, &value_type, compounding);
        }
        match value_type {
            ValueType::DiscountFactor => ValueConverter::df_to_zero(value, t, compounding),
            ValueType::ZeroRate {
                compounding: stored,
                ..
            } => ValueConverter::convert_compounding(value, t, stored, compounding),
            _ => Err(CurveError::incompatible_value_type(
                "DiscountFactor or ZeroRate",
                format!("{value_type:?}"),
            )),
        }
    }

    fn origin_zero_rate(
        &self,
        value: f64,
        kind: &ValueType,
        target: Compounding,
    ) -> CurveResult<f64> {
        if !value.is_finite() {
            return Err(CurveError::invalid_value("nonfinite origin quotation"));
        }
        let (rate, source) = match kind {
            ValueType::ZeroRate { compounding, .. } => (value, *compounding),
            ValueType::DiscountFactor => {
                if value != 1.0 {
                    return Err(CurveError::invalid_value(
                        "discount origin must be normalized to one",
                    ));
                }
                let derivative = self
                    .inner
                    .derivative_at(0.0)
                    .ok_or(CurveError::DerivativeNotAvailable { tenor: 0.0 })?;
                if !derivative.is_finite() {
                    return Err(CurveError::invalid_value("nonfinite origin derivative"));
                }
                (-derivative, Compounding::Continuous)
            }
            _ => {
                return Err(CurveError::incompatible_value_type(
                    "DiscountFactor or ZeroRate",
                    format!("{kind:?}"),
                ))
            }
        };
        // Simple and continuous quotations have the same limit at zero time.
        // Periodic-to-continuous conversion is horizon independent. No Simple
        // quotation is converted using an invented one-year period.
        let continuous = match source {
            Compounding::Continuous | Compounding::Simple => rate,
            _ => ValueConverter::convert_compounding(rate, 1.0, source, Compounding::Continuous)?,
        };
        if source == target {
            return Ok(rate);
        }
        match target {
            Compounding::Continuous | Compounding::Simple => Ok(continuous),
            _ => ValueConverter::convert_compounding(
                continuous,
                1.0,
                Compounding::Continuous,
                target,
            ),
        }
    }

    /// Returns the forward rate between two dates.
    ///
    /// # Arguments
    ///
    /// * `start` - Start date of forward period
    /// * `end` - End date of forward period
    /// * `compounding` - Compounding for the forward rate
    pub fn forward_rate(
        &self,
        start: Date,
        end: Date,
        compounding: Compounding,
    ) -> CurveResult<f64> {
        let t1 = self.date_to_tenor(start)?;
        let t2 = self.date_to_tenor(end)?;
        let period = crate::contracts::year_fraction(start, end, self.inner.tenor_day_count())
            .map_err(|error| CurveError::invalid_value(error.to_string()))?;
        let start_discount = self.discount_factor_at_tenor(t1)?;
        let end_discount = self.discount_factor_at_tenor(t2)?;
        ValueConverter::forward_rate_from_dfs(
            start_discount,
            end_discount,
            0.0,
            period,
            compounding,
        )
    }

    /// Returns the forward rate between two tenors.
    pub fn forward_rate_at_tenors(
        &self,
        t1: f64,
        t2: f64,
        compounding: Compounding,
    ) -> CurveResult<f64> {
        if !t1.is_finite() || !t2.is_finite() || t1 < 0.0 || t2 <= t1 {
            return Err(CurveError::invalid_value(
                "forward tenors must be finite, nonnegative and strictly ordered",
            ));
        }
        let df1 = self.discount_factor_at_tenor(t1)?;
        let df2 = self.discount_factor_at_tenor(t2)?;
        ValueConverter::forward_rate_from_dfs(df1, df2, t1, t2, compounding)
    }

    /// Returns the instantaneous forward rate at the given date.
    ///
    /// f(T) = -d/dT ln(P(T))
    ///
    /// This requires the curve to support derivatives.
    pub fn instantaneous_forward(&self, date: Date) -> CurveResult<f64> {
        let t = self.date_to_tenor(date)?;
        self.instantaneous_forward_at_tenor(t)
    }

    /// Returns the instantaneous forward rate at a tenor.
    pub fn instantaneous_forward_at_tenor(&self, t: f64) -> CurveResult<f64> {
        if !t.is_finite() || t < 0.0 {
            return Err(CurveError::invalid_value(
                "instantaneous-forward tenor must be finite and nonnegative",
            ));
        }
        let kind = self.inner.value_type();
        if !matches!(kind, ValueType::ZeroRate { .. } | ValueType::DiscountFactor) {
            return Err(CurveError::incompatible_value_type(
                "DiscountFactor or ZeroRate",
                format!("{kind:?}"),
            ));
        }
        let value = self.inner.try_value_at(t)?;
        let derivative = self
            .inner
            .derivative_at(t)
            .ok_or(CurveError::DerivativeNotAvailable { tenor: t })?;
        if !value.is_finite() || !derivative.is_finite() {
            return Err(CurveError::invalid_value(
                "instantaneous-forward value and derivative must be finite",
            ));
        }
        let (result, nonzero_numerator) = match kind {
            ValueType::ZeroRate { compounding, .. } => (
                ValueConverter::instantaneous_forward(value, derivative, t, compounding)?,
                false,
            ),
            ValueType::DiscountFactor => {
                if value <= 0.0 {
                    return Err(CurveError::invalid_value(
                        "discount factor must be positive",
                    ));
                }
                (-derivative / value, derivative != 0.0)
            }
            _ => {
                return Err(CurveError::incompatible_value_type(
                    "DiscountFactor or ZeroRate",
                    format!("{kind:?}"),
                ))
            }
        };
        if !result.is_finite() || (nonzero_numerator && result == 0.0) {
            return Err(CurveError::invalid_value(
                "unrepresentable instantaneous forward",
            ));
        }
        Ok(result)
    }

    /// Returns the par swap rate for a given maturity.
    ///
    /// The par rate is the fixed rate that makes the swap value zero.
    ///
    /// # Arguments
    ///
    /// * `maturity` - Swap maturity date
    /// * `frequency` - Payment frequency
    /// * `day_count` - Day count for fixed leg
    pub fn par_swap_rate(
        &self,
        maturity: Date,
        frequency: convex_core::types::Frequency,
        day_count: &dyn DayCount,
    ) -> CurveResult<f64> {
        let ref_date = self.reference_date();
        let t_mat = self.date_to_tenor(maturity)?;

        if t_mat <= 0.0 {
            return Err(CurveError::invalid_value(
                "Maturity must be after reference date",
            ));
        }

        // Number of periods
        let periods_per_year = frequency.periods_per_year() as f64;
        if periods_per_year == 0.0 {
            return Err(CurveError::invalid_value("Invalid frequency for par rate"));
        }

        let period_length = 1.0 / periods_per_year;
        let num_periods = (t_mat / period_length).round() as usize;

        if num_periods == 0 {
            return Err(CurveError::invalid_value(
                "Maturity too short for given frequency",
            ));
        }

        // Sum of discount factors at payment dates
        let mut annuity = 0.0;
        for i in 1..=num_periods {
            let t = i as f64 * period_length;
            let df = self.discount_factor_at_tenor(t)?;
            let year_frac = day_count.year_fraction(
                ref_date.add_days(((i - 1) as f64 * period_length * 365.0) as i64),
                ref_date.add_days((i as f64 * period_length * 365.0) as i64),
            );
            annuity += df
                * rust_decimal::prelude::ToPrimitive::to_f64(&year_frac).unwrap_or(period_length);
        }

        let df_mat = self.discount_factor_at_tenor(t_mat)?;

        if annuity.abs() < 1e-10 {
            return Err(CurveError::math_error("Annuity is zero"));
        }

        // Par rate = (1 - DF_maturity) / annuity
        Ok((1.0 - df_mat) / annuity)
    }

    /// Returns the tenor bounds of the underlying curve.
    #[must_use]
    pub fn tenor_bounds(&self) -> (f64, f64) {
        self.inner.tenor_bounds()
    }

    /// Returns the maximum date of the curve.
    #[must_use]
    pub fn max_date(&self) -> Date {
        self.inner.max_date()
    }
}

// Implement TermStructure for RateCurve so it can be nested
impl<T: TermStructure> TermStructure for RateCurve<T> {
    fn reference_date(&self) -> Date {
        self.inner.reference_date()
    }

    fn value_at(&self, t: f64) -> f64 {
        self.inner.value_at(t)
    }

    fn try_value_at(&self, t: f64) -> CurveResult<f64> {
        self.inner.try_value_at(t)
    }

    fn tenor_bounds(&self) -> (f64, f64) {
        self.inner.tenor_bounds()
    }

    fn value_type(&self) -> ValueType {
        self.inner.value_type()
    }

    fn derivative_at(&self, t: f64) -> Option<f64> {
        self.inner.derivative_at(t)
    }

    fn max_date(&self) -> Date {
        self.inner.max_date()
    }

    fn tenor_day_count(&self) -> convex_core::daycounts::DayCountConvention {
        self.inner.tenor_day_count()
    }

    fn date_to_tenor(&self, date: Date) -> CurveResult<f64> {
        self.inner.date_to_tenor(date)
    }
}

// ============================================================================
// Backward Compatibility: Implement YieldCurve from convex-core
// ============================================================================

/// Implement `YieldCurve` for `RateCurve<T>` to enable backward compatibility.
///
/// This allows `RateCurve` instances to be used wherever `&dyn YieldCurve`
/// is expected, enabling gradual migration from the old curve interface.
impl<T: TermStructure> YieldCurve for RateCurve<T> {
    fn reference_date(&self) -> Date {
        self.inner.reference_date()
    }

    fn discount_factor(&self, date: Date) -> ConvexResult<Decimal> {
        let df = self
            .discount_factor(date)
            .map_err(|e| convex_core::error::ConvexError::curve_error(e.to_string()))?;
        Decimal::from_f64_retain(df)
            .filter(|value| df == 0.0 || !value.is_zero())
            .ok_or_else(|| {
                convex_core::error::ConvexError::curve_error(
                    "curve result is outside nonzero Decimal representation",
                )
            })
    }

    fn zero_rate(&self, date: Date) -> ConvexResult<Decimal> {
        let rate = self
            .zero_rate_continuous(date)
            .map_err(|e| convex_core::error::ConvexError::curve_error(e.to_string()))?;
        Decimal::from_f64_retain(rate)
            .filter(|value| rate == 0.0 || !value.is_zero())
            .ok_or_else(|| {
                convex_core::error::ConvexError::curve_error(
                    "curve result is outside nonzero Decimal representation",
                )
            })
    }

    fn forward_rate(&self, start: Date, end: Date) -> ConvexResult<Decimal> {
        let fwd = self
            .forward_rate(start, end, Compounding::Continuous)
            .map_err(|e| convex_core::error::ConvexError::curve_error(e.to_string()))?;
        Decimal::from_f64_retain(fwd)
            .filter(|value| fwd == 0.0 || !value.is_zero())
            .ok_or_else(|| {
                convex_core::error::ConvexError::curve_error(
                    "curve result is outside nonzero Decimal representation",
                )
            })
    }

    fn max_date(&self) -> Date {
        self.inner.max_date()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curves::DiscreteCurve;
    use crate::InterpolationMethod;
    use approx::assert_relative_eq;
    use convex_core::daycounts::DayCountConvention;

    fn sample_df_curve() -> RateCurve<DiscreteCurve> {
        let today = Date::from_ymd(2024, 1, 1).unwrap();
        // Create discount factors for a 5% flat curve
        let tenors = vec![0.5, 1.0, 2.0, 5.0, 10.0];
        let dfs: Vec<f64> = tenors.iter().map(|&t| f64::exp(-0.05 * t)).collect();

        let curve = DiscreteCurve::new(
            today,
            tenors,
            dfs,
            ValueType::DiscountFactor,
            InterpolationMethod::LogLinear,
        )
        .unwrap();

        RateCurve::new(curve)
    }

    fn sample_zero_curve() -> RateCurve<DiscreteCurve> {
        let today = Date::from_ymd(2024, 1, 1).unwrap();
        let tenors = vec![0.5, 1.0, 2.0, 5.0, 10.0];
        let rates = vec![0.04, 0.045, 0.05, 0.055, 0.06];

        let curve = DiscreteCurve::new(
            today,
            tenors,
            rates,
            ValueType::ZeroRate {
                compounding: Compounding::Continuous,
                day_count: DayCountConvention::Act365Fixed,
            },
            InterpolationMethod::Linear,
        )
        .unwrap();

        RateCurve::new(curve)
    }

    #[test]
    fn test_discount_factor_from_df_curve() {
        let curve = sample_df_curve();
        let t = 2.0;

        let df = curve.discount_factor_at_tenor(t).unwrap();
        let expected = (-0.05 * t).exp();

        assert_relative_eq!(df, expected, epsilon = 1e-6);
    }

    #[test]
    fn test_discount_factor_from_zero_curve() {
        let curve = sample_zero_curve();
        let t = 2.0;

        let df = curve.discount_factor_at_tenor(t).unwrap();
        let rate = 0.05; // 5% at 2Y
        let expected = (-rate * t).exp();

        assert_relative_eq!(df, expected, epsilon = 1e-6);
    }

    #[test]
    fn test_zero_rate_from_df_curve() {
        let curve = sample_df_curve();
        let t = 2.0;

        let rate = curve
            .zero_rate_at_tenor(t, Compounding::Continuous)
            .unwrap();
        assert_relative_eq!(rate, 0.05, epsilon = 1e-6);
    }

    #[test]
    fn test_zero_rate_from_zero_curve() {
        let curve = sample_zero_curve();
        let t = 2.0;

        let rate = curve
            .zero_rate_at_tenor(t, Compounding::Continuous)
            .unwrap();
        assert_relative_eq!(rate, 0.05, epsilon = 1e-6);
    }

    #[test]
    fn test_compounding_conversion() {
        let curve = sample_zero_curve();
        let t = 1.0;

        let cont = curve
            .zero_rate_at_tenor(t, Compounding::Continuous)
            .unwrap();
        let annual = curve.zero_rate_at_tenor(t, Compounding::Annual).unwrap();

        // Annual should be slightly higher than continuous
        assert!(annual > cont);

        // Check relationship: exp(r_cont) = 1 + r_annual
        assert_relative_eq!((cont).exp(), 1.0 + annual, epsilon = 1e-6);
    }

    #[test]
    fn test_forward_rate() {
        let curve = sample_df_curve();

        let fwd = curve
            .forward_rate_at_tenors(1.0, 2.0, Compounding::Continuous)
            .unwrap();

        // For flat curve, forward = spot = 5%
        assert_relative_eq!(fwd, 0.05, epsilon = 1e-4);
    }

    #[test]
    fn test_forward_rate_upward_sloping() {
        let curve = sample_zero_curve();

        // 1Y to 2Y forward
        let fwd = curve
            .forward_rate_at_tenors(1.0, 2.0, Compounding::Continuous)
            .unwrap();

        // For upward sloping curve, 1Y1Y forward should be higher than 1Y spot
        // 1Y spot = 4.5%, 2Y spot = 5%, so 1Y1Y forward = (2*5% - 1*4.5%) = 5.5%
        assert_relative_eq!(fwd, 0.055, epsilon = 1e-3);
    }

    #[test]
    fn test_discount_factor_at_zero() {
        let curve = sample_df_curve();
        let df = curve.discount_factor_at_tenor(0.0).unwrap();
        assert_relative_eq!(df, 1.0, epsilon = 1e-10);
    }

    #[test]
    fn test_zero_rate_at_zero() {
        let curve = sample_zero_curve();
        let rate = curve
            .zero_rate_at_tenor(0.0, Compounding::Continuous)
            .unwrap();
        assert_relative_eq!(rate, 0.04, epsilon = 1e-10);
    }

    #[test]
    fn test_tenor_bounds() {
        let curve = sample_zero_curve();
        let (min, max) = curve.tenor_bounds();
        assert_relative_eq!(min, 0.5, epsilon = 1e-10);
        assert_relative_eq!(max, 10.0, epsilon = 1e-10);
    }
}
