//! Backward compatibility layer for convex-bonds integration.
//!
//! This module provides compatibility types that bridge the new `TermStructure`
//! framework with the API expected by convex-bonds and convex-analytics.

use std::sync::Arc;

use convex_core::types::{Compounding, Date};

use crate::error::CurveResult;
use crate::term_structure::TermStructure;
use crate::wrappers::RateCurve;

// ============================================================================
// Curve Trait (Backward Compatibility)
// ============================================================================

/// Trait for discount curves used in bond pricing and analytics.
///
/// This trait provides a comprehensive interface for accessing curve values,
/// compatible with both convex-bonds and convex-analytics APIs.
pub trait Curve: Send + Sync {
    /// Returns the discount factor for a given year fraction.
    ///
    /// # Arguments
    ///
    /// * `t` - Time in years from reference date
    fn discount_factor(&self, t: f64) -> CurveResult<f64>;

    /// Returns the zero rate for a given year fraction with specified compounding.
    ///
    /// # Arguments
    ///
    /// * `t` - Time in years from reference date
    /// * `compounding` - Compounding convention for the rate
    fn zero_rate(&self, t: f64, compounding: Compounding) -> CurveResult<f64>;

    /// Returns the forward rate between two year fractions.
    ///
    /// # Arguments
    ///
    /// * `t1` - Start time in years from reference date
    /// * `t2` - End time in years from reference date
    fn forward_rate(&self, t1: f64, t2: f64) -> CurveResult<f64>;

    /// Returns the instantaneous forward rate at time t.
    ///
    /// The instantaneous forward rate is the limit of forward rates as the
    /// forward period approaches zero.
    fn instantaneous_forward(&self, t: f64) -> CurveResult<f64>;

    /// Returns the reference date of the curve.
    fn reference_date(&self) -> Date;

    /// Returns the maximum date for which the curve is defined.
    fn max_date(&self) -> Date;
}

// Implement Curve for RateCurve<T>
impl<T: TermStructure> Curve for RateCurve<T> {
    fn discount_factor(&self, t: f64) -> CurveResult<f64> {
        self.discount_factor_at_tenor(t)
    }

    fn zero_rate(&self, t: f64, compounding: Compounding) -> CurveResult<f64> {
        self.zero_rate_at_tenor(t, compounding)
    }

    fn forward_rate(&self, t1: f64, t2: f64) -> CurveResult<f64> {
        self.forward_rate_at_tenors(t1, t2, Compounding::Continuous)
    }

    fn instantaneous_forward(&self, t: f64) -> CurveResult<f64> {
        self.instantaneous_forward_at_tenor(t)
    }

    fn reference_date(&self) -> Date {
        RateCurve::reference_date(self)
    }

    fn max_date(&self) -> Date {
        self.inner().max_date()
    }
}

// ============================================================================
// Forward Curve (for FRN pricing)
// ============================================================================

/// A forward curve derived from a discount curve.
///
/// Provides forward rates for a specific tenor period (e.g., 3M SOFR forwards).
#[derive(Clone)]
pub struct ForwardCurve {
    /// The underlying discount curve.
    discount_curve: Arc<dyn Curve>,
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
    pub fn new(discount_curve: Arc<dyn Curve>, forward_tenor: f64) -> Self {
        Self {
            discount_curve,
            forward_tenor,
        }
    }

    /// Creates a forward curve with a monthly tenor.
    pub fn from_months(discount_curve: Arc<dyn Curve>, months: u32) -> Self {
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
// Zero Curve (type alias for clarity)
// ============================================================================

/// Type alias for a curve that stores zero rates.
///
/// This is the same as a RateCurve but provides semantic clarity.
pub type ZeroCurve = RateCurve<crate::curves::DiscreteCurve>;

// ============================================================================
// Curve Instrument Types
// ============================================================================

/// Type of curve calibration instrument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InstrumentType {
    /// Cash deposit
    Deposit,
    /// Forward rate agreement
    Fra,
    /// Interest rate future
    Future,
    /// Interest rate swap
    Swap,
    /// Overnight index swap
    Ois,
    /// Basis swap
    BasisSwap,
    /// Zero-coupon government bond
    GovernmentZeroCoupon,
    /// Fixed coupon government bond
    GovernmentCouponBond,
    /// Treasury bill
    TBill,
    /// Other instrument type
    Other,
}

impl std::fmt::Display for InstrumentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstrumentType::Deposit => write!(f, "Deposit"),
            InstrumentType::Fra => write!(f, "FRA"),
            InstrumentType::Future => write!(f, "Future"),
            InstrumentType::Swap => write!(f, "Swap"),
            InstrumentType::Ois => write!(f, "OIS"),
            InstrumentType::BasisSwap => write!(f, "Basis Swap"),
            InstrumentType::GovernmentZeroCoupon => write!(f, "Govt Zero"),
            InstrumentType::GovernmentCouponBond => write!(f, "Govt Coupon"),
            InstrumentType::TBill => write!(f, "T-Bill"),
            InstrumentType::Other => write!(f, "Other"),
        }
    }
}

/// Trait for instruments used in curve construction.
///
/// Curve instruments are used to bootstrap or calibrate yield curves.
/// Each instrument provides methods to:
/// - Report its maturity date
/// - Calculate present value given a curve
/// - Derive the implied discount factor
pub trait CurveInstrument: Send + Sync {
    /// Returns the maturity date of the instrument.
    fn maturity(&self) -> Date;

    /// Returns the pillar date (typically same as maturity).
    ///
    /// For some instruments like swaps, this might differ from maturity.
    fn pillar_date(&self) -> Date {
        self.maturity()
    }

    /// Calculates the present value given a curve.
    ///
    /// For calibration, this should return the difference between
    /// theoretical and market price.
    fn pv(&self, curve: &dyn Curve) -> CurveResult<f64>;

    /// Returns the implied discount factor at maturity.
    ///
    /// Given the known portion of the curve, this returns the discount
    /// factor that would make the instrument price correctly.
    fn implied_df(&self, curve: &dyn Curve, target_pv: f64) -> CurveResult<f64>;

    /// Returns the instrument type.
    fn instrument_type(&self) -> InstrumentType;

    /// Returns a description of the instrument.
    fn description(&self) -> String;
}

// ============================================================================
// Discount Curve Builder (Backward Compatibility)
// ============================================================================

use crate::curves::DiscreteCurve;
use crate::value_type::ValueType;
use crate::InterpolationMethod;

/// Builder for simple discount curves (backward compatibility).
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
                compounding: convex_core::types::Compounding::Continuous,
                day_count: convex_core::daycounts::DayCountConvention::Act365Fixed,
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

use rust_decimal::Decimal;

/// Builder for zero rate curves using dates (backward compatibility).
///
/// This builder provides a date-based API for constructing zero rate curves,
/// useful for test fixtures and simple curve construction.
pub struct ZeroCurveBuilder {
    reference_date: Option<Date>,
    dates: Vec<Date>,
    rates: Vec<Decimal>,
    interpolation: crate::InterpolationMethod,
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
            interpolation: crate::InterpolationMethod::Linear,
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
    pub fn interpolation(mut self, method: crate::InterpolationMethod) -> Self {
        self.interpolation = method;
        self
    }

    /// Builds the zero rate curve.
    pub fn build(self) -> CurveResult<ZeroCurve> {
        let ref_date = self.reference_date.ok_or_else(|| {
            crate::error::CurveError::invalid_value("Reference date not set")
        })?;

        if self.dates.is_empty() {
            return Err(crate::error::CurveError::invalid_value(
                "No rate points provided",
            ));
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
            day_count: convex_core::daycounts::DayCountConvention::Act365Fixed,
        };

        let curve = DiscreteCurve::new(
            ref_date,
            tenors,
            values,
            value_type,
            self.interpolation,
        )?;

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
