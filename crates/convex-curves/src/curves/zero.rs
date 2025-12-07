//! Zero-coupon yield curve.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use convex_core::traits::YieldCurve;
use convex_core::{ConvexResult, Date};
use convex_math::interpolation::{CubicSpline, Interpolator, LinearInterpolator};

use crate::error::{CurveError, CurveResult};
use crate::interpolation::InterpolationMethod;

/// A zero-coupon yield curve.
///
/// Represents continuously compounded zero rates for various maturities.
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::prelude::*;
/// use rust_decimal_macros::dec;
///
/// let curve = ZeroCurveBuilder::new()
///     .reference_date(Date::from_ymd(2025, 1, 15).unwrap())
///     .add_rate(Date::from_ymd(2025, 4, 15).unwrap(), dec!(0.045))
///     .add_rate(Date::from_ymd(2025, 7, 15).unwrap(), dec!(0.048))
///     .interpolation(InterpolationMethod::Linear)
///     .build()
///     .unwrap();
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZeroCurve {
    /// Reference (valuation) date.
    reference_date: Date,

    /// Curve pillar dates.
    dates: Vec<Date>,

    /// Zero rates at each pillar (continuously compounded).
    rates: Vec<Decimal>,

    /// Interpolation method.
    interpolation: InterpolationMethod,

    /// Time fractions from reference date (cached for performance).
    #[serde(skip)]
    time_fractions: Vec<f64>,

    /// Rates as f64 for interpolation.
    #[serde(skip)]
    rates_f64: Vec<f64>,
}

impl ZeroCurve {
    /// Creates a new zero curve.
    fn new(
        reference_date: Date,
        dates: Vec<Date>,
        rates: Vec<Decimal>,
        interpolation: InterpolationMethod,
    ) -> CurveResult<Self> {
        if dates.is_empty() {
            return Err(CurveError::EmptyCurve);
        }

        if dates.len() != rates.len() {
            return Err(CurveError::invalid_data(format!(
                "dates and rates must have same length: {} vs {}",
                dates.len(),
                rates.len()
            )));
        }

        // Calculate time fractions
        let time_fractions: Vec<f64> = dates
            .iter()
            .map(|d| reference_date.days_between(d) as f64 / 365.0)
            .collect();

        let rates_f64: Vec<f64> = rates
            .iter()
            .map(|r| r.to_string().parse().unwrap_or(0.0))
            .collect();

        Ok(Self {
            reference_date,
            dates,
            rates,
            interpolation,
            time_fractions,
            rates_f64,
        })
    }

    /// Returns the reference date.
    #[must_use]
    pub fn reference_date(&self) -> Date {
        self.reference_date
    }

    /// Returns the pillar dates.
    #[must_use]
    pub fn dates(&self) -> &[Date] {
        &self.dates
    }

    /// Returns the zero rates.
    #[must_use]
    pub fn rates(&self) -> &[Decimal] {
        &self.rates
    }

    /// Returns the interpolation method.
    #[must_use]
    pub fn interpolation(&self) -> InterpolationMethod {
        self.interpolation
    }

    /// Returns the interpolated zero rate at a given date.
    pub fn zero_rate_at(&self, date: Date) -> CurveResult<Decimal> {
        if date <= self.reference_date {
            return Ok(self.rates[0]);
        }

        let t = self.reference_date.days_between(&date) as f64 / 365.0;

        let rate = match self.interpolation {
            InterpolationMethod::Linear => {
                let interp =
                    LinearInterpolator::new(self.time_fractions.clone(), self.rates_f64.clone())
                        .map_err(|e| CurveError::InterpolationFailed {
                            reason: e.to_string(),
                        })?
                        .with_extrapolation();

                interp
                    .interpolate(t)
                    .map_err(|e| CurveError::InterpolationFailed {
                        reason: e.to_string(),
                    })?
            }
            InterpolationMethod::CubicSpline if self.time_fractions.len() >= 3 => {
                let interp = CubicSpline::new(self.time_fractions.clone(), self.rates_f64.clone())
                    .map_err(|e| CurveError::InterpolationFailed {
                        reason: e.to_string(),
                    })?
                    .with_extrapolation();

                interp
                    .interpolate(t)
                    .map_err(|e| CurveError::InterpolationFailed {
                        reason: e.to_string(),
                    })?
            }
            _ => {
                // Fall back to linear for other methods
                let interp =
                    LinearInterpolator::new(self.time_fractions.clone(), self.rates_f64.clone())
                        .map_err(|e| CurveError::InterpolationFailed {
                            reason: e.to_string(),
                        })?
                        .with_extrapolation();

                interp
                    .interpolate(t)
                    .map_err(|e| CurveError::InterpolationFailed {
                        reason: e.to_string(),
                    })?
            }
        };

        Ok(Decimal::from_f64_retain(rate).unwrap_or(Decimal::ZERO))
    }

    /// Returns the discount factor for a given date.
    pub fn discount_factor_at(&self, date: Date) -> CurveResult<Decimal> {
        if date <= self.reference_date {
            return Ok(Decimal::ONE);
        }

        let rate = self.zero_rate_at(date)?;
        let t = self.reference_date.days_between(&date) as f64 / 365.0;
        let r = rate.to_string().parse::<f64>().unwrap_or(0.0);

        // Continuous compounding: DF = e^(-r*t)
        let df = (-r * t).exp();

        Ok(Decimal::from_f64_retain(df).unwrap_or(Decimal::ONE))
    }
}

impl YieldCurve for ZeroCurve {
    fn reference_date(&self) -> Date {
        self.reference_date
    }

    fn discount_factor(&self, date: Date) -> ConvexResult<Decimal> {
        self.discount_factor_at(date).map_err(|e| {
            convex_core::ConvexError::CurveConstructionFailed {
                reason: e.to_string(),
            }
        })
    }

    fn zero_rate(&self, date: Date) -> ConvexResult<Decimal> {
        self.zero_rate_at(date)
            .map_err(|e| convex_core::ConvexError::CurveConstructionFailed {
                reason: e.to_string(),
            })
    }

    fn max_date(&self) -> Date {
        self.dates.last().copied().unwrap_or(self.reference_date)
    }
}

/// Builder for constructing zero curves.
#[derive(Debug, Clone, Default)]
pub struct ZeroCurveBuilder {
    reference_date: Option<Date>,
    points: Vec<(Date, Decimal)>,
    interpolation: InterpolationMethod,
}

impl ZeroCurveBuilder {
    /// Creates a new builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the reference (valuation) date.
    #[must_use]
    pub fn reference_date(mut self, date: Date) -> Self {
        self.reference_date = Some(date);
        self
    }

    /// Adds a rate at a specific date.
    #[must_use]
    pub fn add_rate(mut self, date: Date, rate: Decimal) -> Self {
        self.points.push((date, rate));
        self
    }

    /// Adds multiple rates.
    #[must_use]
    pub fn add_rates(mut self, rates: impl IntoIterator<Item = (Date, Decimal)>) -> Self {
        self.points.extend(rates);
        self
    }

    /// Sets the interpolation method.
    #[must_use]
    pub fn interpolation(mut self, method: InterpolationMethod) -> Self {
        self.interpolation = method;
        self
    }

    /// Builds the zero curve.
    ///
    /// # Errors
    ///
    /// Returns an error if reference date is not set or if there are no data points.
    pub fn build(mut self) -> CurveResult<ZeroCurve> {
        let reference_date = self
            .reference_date
            .ok_or(CurveError::MissingReferenceDate)?;

        if self.points.is_empty() {
            return Err(CurveError::EmptyCurve);
        }

        // Sort by date
        self.points.sort_by_key(|(date, _)| *date);

        let (dates, rates): (Vec<_>, Vec<_>) = self.points.into_iter().unzip();

        ZeroCurve::new(reference_date, dates, rates, self.interpolation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_zero_curve_builder() {
        let curve = ZeroCurveBuilder::new()
            .reference_date(Date::from_ymd(2025, 1, 15).unwrap())
            .add_rate(Date::from_ymd(2025, 4, 15).unwrap(), dec!(0.045))
            .add_rate(Date::from_ymd(2025, 7, 15).unwrap(), dec!(0.048))
            .add_rate(Date::from_ymd(2026, 1, 15).unwrap(), dec!(0.050))
            .interpolation(InterpolationMethod::Linear)
            .build()
            .unwrap();

        assert_eq!(curve.dates().len(), 3);
    }

    #[test]
    fn test_zero_curve_interpolation() {
        let curve = ZeroCurveBuilder::new()
            .reference_date(Date::from_ymd(2025, 1, 1).unwrap())
            .add_rate(Date::from_ymd(2025, 7, 1).unwrap(), dec!(0.04))
            .add_rate(Date::from_ymd(2026, 1, 1).unwrap(), dec!(0.05))
            .build()
            .unwrap();

        // Interpolated rate should be between 4% and 5%
        let rate = curve
            .zero_rate_at(Date::from_ymd(2025, 10, 1).unwrap())
            .unwrap();
        assert!(rate > dec!(0.04) && rate < dec!(0.05));
    }

    #[test]
    fn test_discount_factor() {
        let curve = ZeroCurveBuilder::new()
            .reference_date(Date::from_ymd(2025, 1, 1).unwrap())
            .add_rate(Date::from_ymd(2025, 7, 1).unwrap(), dec!(0.05))
            .add_rate(Date::from_ymd(2026, 1, 1).unwrap(), dec!(0.05))
            .build()
            .unwrap();

        let df = curve
            .discount_factor_at(Date::from_ymd(2026, 1, 1).unwrap())
            .unwrap();

        // DF should be approximately e^(-0.05*1) ≈ 0.9512
        let df_f64 = df.to_string().parse::<f64>().unwrap();
        assert!((df_f64 - 0.9512).abs() < 0.01);
    }

    #[test]
    fn test_missing_reference_date() {
        let result = ZeroCurveBuilder::new()
            .add_rate(Date::from_ymd(2025, 7, 1).unwrap(), dec!(0.04))
            .build();

        assert!(result.is_err());
    }

    #[test]
    fn test_empty_curve() {
        let result = ZeroCurveBuilder::new()
            .reference_date(Date::from_ymd(2025, 1, 1).unwrap())
            .build();

        assert!(result.is_err());
    }
}
