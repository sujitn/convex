//! Curve implementations.
//!
//! This module provides concrete curve types:
//!
//! - [`DiscreteCurve`]: Curve from discrete point data with interpolation
//! - [`SegmentedCurve`]: Multiple segments with different sources/interpolation
//! - [`DelegatedCurve`]: Wraps another curve with fallback handling
//! - [`DerivedCurve`]: Transforms a base curve (shift, spread, scale)
//! - [`ForwardCurve`]: Forward rate curve derived from discount curve
//!
//! # Builders
//!
//! - [`DiscountCurveBuilder`]: Simple builder for discount/zero rate curves
//! - [`ZeroCurveBuilder`]: Date-based builder for zero rate curves

mod delegated;
mod derived;
mod discrete;
mod segmented;

pub use delegated::{DelegatedCurve, DelegationFallback};
pub use derived::{CurveTransform, DerivedCurve};
pub use discrete::DiscreteCurve;
pub use segmented::{CurveSegment, SegmentedCurve, SegmentSource};

use std::sync::Arc;

use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Compounding, Date};
use rust_decimal::Decimal;

use crate::error::{CurveError, CurveResult};
use crate::value_type::ValueType;
use crate::wrappers::{RateCurve, RateCurveDyn};
use crate::InterpolationMethod;

// ============================================================================
// Type Aliases
// ============================================================================

/// Type alias for a curve that stores zero rates.
pub type ZeroCurve = RateCurve<DiscreteCurve>;

/// Type alias for discount curve (same as ZeroCurve).
pub type DiscountCurve = ZeroCurve;

// ============================================================================
// Forward Curve
// ============================================================================

/// A forward curve derived from a discount curve.
///
/// Provides forward rates for a specific tenor period (e.g., 3M SOFR forwards).
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::{ForwardCurve, RateCurve, DiscreteCurve};
/// use std::sync::Arc;
///
/// let discount_curve: Arc<dyn RateCurveDyn> = Arc::new(rate_curve);
/// let forward_3m = ForwardCurve::from_months(discount_curve, 3);
///
/// let fwd_rate = forward_3m.forward_rate_at(1.0)?; // 3M forward starting in 1Y
/// ```
#[derive(Clone)]
pub struct ForwardCurve {
    /// The underlying discount curve.
    discount_curve: Arc<dyn RateCurveDyn>,
    /// Forward period in years (e.g., 0.25 for 3 months).
    forward_tenor: f64,
}

impl std::fmt::Debug for ForwardCurve {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ForwardCurve")
            .field("forward_tenor", &self.forward_tenor)
            .field("reference_date", &self.discount_curve.reference_date())
            .finish_non_exhaustive()
    }
}

impl ForwardCurve {
    /// Creates a new forward curve from a discount curve.
    ///
    /// # Arguments
    ///
    /// * `discount_curve` - The discount curve to derive forwards from
    /// * `forward_tenor` - Forward period in years
    pub fn new(discount_curve: Arc<dyn RateCurveDyn>, forward_tenor: f64) -> Self {
        Self {
            discount_curve,
            forward_tenor,
        }
    }

    /// Creates a forward curve with a monthly tenor.
    pub fn from_months(discount_curve: Arc<dyn RateCurveDyn>, months: u32) -> Self {
        Self::new(discount_curve, months as f64 / 12.0)
    }

    /// Returns the reference date of the underlying curve.
    pub fn reference_date(&self) -> Date {
        self.discount_curve.reference_date()
    }

    /// Returns the forward rate at time t.
    ///
    /// This is the forward rate from t to t + forward_tenor.
    pub fn forward_rate_at(&self, t: f64) -> CurveResult<f64> {
        self.discount_curve.forward_rate(t, t + self.forward_tenor)
    }

    /// Returns the forward tenor in years.
    pub fn forward_tenor(&self) -> f64 {
        self.forward_tenor
    }
}

// ============================================================================
// Discount Curve Builder
// ============================================================================

/// Builder for simple discount curves.
///
/// Supports both discount factor and zero rate inputs.
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::{DiscountCurveBuilder, InterpolationMethod};
/// use convex_core::types::Date;
///
/// let curve = DiscountCurveBuilder::new(today)
///     .add_zero_rate(0.5, 0.045)
///     .add_zero_rate(1.0, 0.05)
///     .add_zero_rate(2.0, 0.055)
///     .with_interpolation(InterpolationMethod::Linear)
///     .build()?;
/// ```
pub struct DiscountCurveBuilder {
    reference_date: Date,
    tenors: Vec<f64>,
    values: Vec<f64>,
    is_zero_rate: bool,
    interpolation: InterpolationMethod,
    extrapolate: bool,
}

impl DiscountCurveBuilder {
    /// Creates a new discount curve builder.
    pub fn new(reference_date: Date) -> Self {
        Self {
            reference_date,
            tenors: Vec::new(),
            values: Vec::new(),
            is_zero_rate: false,
            interpolation: InterpolationMethod::LogLinear,
            extrapolate: false,
        }
    }

    /// Adds a pillar point (tenor in years, discount factor).
    pub fn add_pillar(mut self, tenor: f64, df: f64) -> Self {
        self.tenors.push(tenor);
        self.values.push(df);
        self.is_zero_rate = false;
        self
    }

    /// Adds a zero rate point.
    pub fn add_zero_rate(mut self, tenor: f64, rate: f64) -> Self {
        self.tenors.push(tenor);
        self.values.push(rate);
        self.is_zero_rate = true;
        self
    }

    /// Sets the interpolation method.
    pub fn with_interpolation(mut self, method: InterpolationMethod) -> Self {
        self.interpolation = method;
        self
    }

    /// Enables flat extrapolation.
    pub fn with_extrapolation(mut self) -> Self {
        self.extrapolate = true;
        self
    }

    /// Builds the discount curve.
    pub fn build(self) -> CurveResult<RateCurve<DiscreteCurve>> {
        let value_type = if self.is_zero_rate {
            ValueType::ZeroRate {
                compounding: Compounding::Continuous,
                day_count: DayCountConvention::Act365Fixed,
            }
        } else {
            ValueType::DiscountFactor
        };

        let curve = DiscreteCurve::new(
            self.reference_date,
            self.tenors,
            self.values,
            value_type,
            self.interpolation,
        )?;

        Ok(RateCurve::new(curve))
    }
}

// ============================================================================
// Zero Curve Builder (Date-based API)
// ============================================================================

/// Builder for zero rate curves using dates.
///
/// This builder provides a date-based API for constructing zero rate curves,
/// useful for test fixtures and simple curve construction.
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::ZeroCurveBuilder;
/// use convex_core::types::Date;
/// use rust_decimal_macros::dec;
///
/// let curve = ZeroCurveBuilder::new()
///     .reference_date(today)
///     .add_rate(today.add_years(1), dec!(0.05))
///     .add_rate(today.add_years(2), dec!(0.055))
///     .build()?;
/// ```
pub struct ZeroCurveBuilder {
    reference_date: Option<Date>,
    dates: Vec<Date>,
    rates: Vec<Decimal>,
    interpolation: InterpolationMethod,
}

impl Default for ZeroCurveBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ZeroCurveBuilder {
    /// Creates a new zero curve builder.
    pub fn new() -> Self {
        Self {
            reference_date: None,
            dates: Vec::new(),
            rates: Vec::new(),
            interpolation: InterpolationMethod::Linear,
        }
    }

    /// Sets the reference date.
    pub fn reference_date(mut self, date: Date) -> Self {
        self.reference_date = Some(date);
        self
    }

    /// Adds a zero rate at a specific date.
    pub fn add_rate(mut self, date: Date, rate: Decimal) -> Self {
        self.dates.push(date);
        self.rates.push(rate);
        self
    }

    /// Sets the interpolation method.
    pub fn interpolation(mut self, method: InterpolationMethod) -> Self {
        self.interpolation = method;
        self
    }

    /// Builds the zero rate curve.
    pub fn build(self) -> CurveResult<ZeroCurve> {
        let ref_date = self
            .reference_date
            .ok_or_else(|| CurveError::invalid_value("Reference date not set"))?;

        if self.dates.is_empty() {
            return Err(CurveError::invalid_value("No rate points provided"));
        }

        // Convert dates to tenors (years from reference date)
        let tenors: Vec<f64> = self
            .dates
            .iter()
            .map(|d| ref_date.days_between(d) as f64 / 365.0)
            .collect();

        // Convert Decimal rates to f64
        let values: Vec<f64> = self
            .rates
            .iter()
            .map(|r| r.to_string().parse::<f64>().unwrap_or(0.0))
            .collect();

        let value_type = ValueType::ZeroRate {
            compounding: Compounding::Continuous,
            day_count: DayCountConvention::Act365Fixed,
        };

        let curve = DiscreteCurve::new(ref_date, tenors, values, value_type, self.interpolation)?;

        Ok(RateCurve::new(curve))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discount_curve_builder() {
        let today = Date::from_ymd(2024, 1, 1).unwrap();
        let curve = DiscountCurveBuilder::new(today)
            .add_pillar(0.0, 1.0)
            .add_pillar(1.0, 0.95)
            .add_pillar(2.0, 0.90)
            .build()
            .unwrap();

        let df = curve.discount_factor_at_tenor(1.0).unwrap();
        assert!((df - 0.95).abs() < 1e-10);
    }

    #[test]
    fn test_discount_curve_builder_zero_rates() {
        let today = Date::from_ymd(2024, 1, 1).unwrap();
        let curve = DiscountCurveBuilder::new(today)
            .add_zero_rate(0.5, 0.05)
            .add_zero_rate(1.0, 0.05)
            .add_zero_rate(2.0, 0.05)
            .with_interpolation(InterpolationMethod::Linear)
            .build()
            .unwrap();

        // At 5% continuous rate, DF at 1Y should be exp(-0.05*1) ≈ 0.9512
        let df = curve.discount_factor_at_tenor(1.0).unwrap();
        assert!((df - (-0.05_f64).exp()).abs() < 1e-6);
    }
}
