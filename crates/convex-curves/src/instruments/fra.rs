//! Forward Rate Agreement (FRA) instrument.
//!
//! An FRA is an OTC derivative that allows locking in a forward interest rate.

use convex_core::daycounts::DayCountConvention;
use convex_core::Date;
use rust_decimal::prelude::ToPrimitive;

use super::{year_fraction_act360, CurveInstrument, InstrumentType};
use crate::error::{CurveError, CurveResult};
use crate::traits::Curve;

/// Forward Rate Agreement.
///
/// An FRA is a contract to exchange a fixed rate for a floating rate
/// on a notional amount for a specified future period.
///
/// # FRA Notation
///
/// FRAs are quoted as "A x B" where:
/// - A = months until the accrual period starts
/// - B = months until the accrual period ends
///
/// Example: "3x6 FRA" starts in 3 months and ends in 6 months (3-month forward).
///
/// # Pricing Formula
///
/// ```text
/// PV = N × τ × (F - K) × DF(payment_date)
/// ```
/// where:
/// - N = notional
/// - τ = year fraction of the FRA period
/// - F = forward rate = (DF(start)/DF(end) - 1) / τ
/// - K = fixed (contracted) rate
/// - payment_date = start date (FRA settles at period start)
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::instruments::FRA;
///
/// // 3x6 FRA at 5.0% (locking 3M rate starting in 3M)
/// let fra = FRA::new(
///     trade_date,
///     start_date,  // 3M from trade
///     end_date,    // 6M from trade
///     0.05,
/// );
/// ```
#[derive(Debug, Clone, Copy)]
pub struct FRA {
    /// Trade date
    trade_date: Date,
    /// Accrual period start
    start_date: Date,
    /// Accrual period end
    end_date: Date,
    /// Fixed (contracted) rate
    fixed_rate: f64,
    /// Notional amount
    notional: f64,
    /// Day count convention
    day_count: DayCountConvention,
}

impl FRA {
    /// Creates a new FRA.
    ///
    /// # Arguments
    ///
    /// * `trade_date` - The trade date
    /// * `start_date` - Accrual period start
    /// * `end_date` - Accrual period end
    /// * `fixed_rate` - The contracted fixed rate
    pub fn new(trade_date: Date, start_date: Date, end_date: Date, fixed_rate: f64) -> Self {
        Self {
            trade_date,
            start_date,
            end_date,
            fixed_rate,
            notional: 1_000_000.0,
            day_count: DayCountConvention::Act360,
        }
    }

    /// Creates an FRA from tenors (e.g., "3x6").
    ///
    /// # Arguments
    ///
    /// * `trade_date` - The trade date
    /// * `start_months` - Months until period start
    /// * `end_months` - Months until period end
    /// * `fixed_rate` - The contracted fixed rate
    pub fn from_tenors(
        trade_date: Date,
        start_months: i32,
        end_months: i32,
        fixed_rate: f64,
    ) -> CurveResult<Self> {
        let start_date = trade_date.add_months(start_months).map_err(|e| {
            CurveError::invalid_data(format!("Failed to calculate start date: {}", e))
        })?;
        let end_date = trade_date.add_months(end_months).map_err(|e| {
            CurveError::invalid_data(format!("Failed to calculate end date: {}", e))
        })?;

        Ok(Self::new(trade_date, start_date, end_date, fixed_rate))
    }

    /// Sets the notional.
    #[must_use]
    pub fn with_notional(mut self, notional: f64) -> Self {
        self.notional = notional;
        self
    }

    /// Sets the day count convention.
    #[must_use]
    pub fn with_day_count(mut self, day_count: DayCountConvention) -> Self {
        self.day_count = day_count;
        self
    }

    /// Returns the trade date.
    #[must_use]
    pub fn trade_date(&self) -> Date {
        self.trade_date
    }

    /// Returns the accrual start date.
    #[must_use]
    pub fn start_date(&self) -> Date {
        self.start_date
    }

    /// Returns the accrual end date.
    #[must_use]
    pub fn end_date(&self) -> Date {
        self.end_date
    }

    /// Returns the fixed rate.
    #[must_use]
    pub fn fixed_rate(&self) -> f64 {
        self.fixed_rate
    }

    /// Returns the notional.
    #[must_use]
    pub fn notional(&self) -> f64 {
        self.notional
    }

    /// Returns the year fraction for the FRA period.
    #[must_use]
    pub fn year_fraction(&self) -> f64 {
        self.day_count
            .to_day_count()
            .year_fraction(self.start_date, self.end_date)
            .to_f64()
            .unwrap_or(0.0)
    }

    /// Calculates the forward rate implied by the curve.
    pub fn forward_rate(&self, curve: &dyn Curve) -> CurveResult<f64> {
        let ref_date = curve.reference_date();
        let t_start = year_fraction_act360(ref_date, self.start_date);
        let t_end = year_fraction_act360(ref_date, self.end_date);

        let df_start = curve.discount_factor(t_start)?;
        let df_end = curve.discount_factor(t_end)?;

        let tau = self.year_fraction();
        if tau <= 0.0 || df_end <= 0.0 {
            return Ok(0.0);
        }

        Ok((df_start / df_end - 1.0) / tau)
    }
}

impl CurveInstrument for FRA {
    fn maturity(&self) -> Date {
        self.end_date
    }

    fn pillar_date(&self) -> Date {
        // FRA contributes DF at the end date
        self.end_date
    }

    fn pv(&self, curve: &dyn Curve) -> CurveResult<f64> {
        // PV = N × τ × (F - K) × DF(payment)
        let ref_date = curve.reference_date();
        let t_start = year_fraction_act360(ref_date, self.start_date);

        let forward = self.forward_rate(curve)?;
        let tau = self.year_fraction();
        let df_pay = curve.discount_factor(t_start)?; // FRA settles at start

        Ok(self.notional * tau * (forward - self.fixed_rate) * df_pay)
    }

    fn implied_df(&self, curve: &dyn Curve, _target_pv: f64) -> CurveResult<f64> {
        // Solve: (DF(start)/DF(end) - 1) / τ = fixed_rate
        // DF(end) = DF(start) / (1 + fixed_rate × τ)
        let ref_date = curve.reference_date();
        let t_start = year_fraction_act360(ref_date, self.start_date);

        let df_start = curve.discount_factor(t_start)?;
        let tau = self.year_fraction();

        Ok(df_start / (1.0 + self.fixed_rate * tau))
    }

    fn instrument_type(&self) -> InstrumentType {
        InstrumentType::FRA
    }

    fn description(&self) -> String {
        let start_months =
            (self.start_date.days_between(&self.trade_date).abs() as f64 / 30.0).round() as i32;
        let end_months =
            (self.end_date.days_between(&self.trade_date).abs() as f64 / 30.0).round() as i32;
        format!("FRA {}x{} at {:.4}%", start_months, end_months, self.fixed_rate * 100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curves::DiscountCurveBuilder;
    use crate::interpolation::InterpolationMethod;
    use approx::assert_relative_eq;

    fn flat_curve(ref_date: Date, rate: f64) -> impl Curve {
        DiscountCurveBuilder::new(ref_date)
            .add_zero_rate(0.25, rate)
            .add_zero_rate(0.5, rate)
            .add_zero_rate(1.0, rate)
            .with_interpolation(InterpolationMethod::LogLinear)
            .with_extrapolation()
            .build()
            .unwrap()
    }

    #[test]
    fn test_fra_basic() {
        let trade_date = Date::from_ymd(2025, 1, 15).unwrap();
        let start_date = Date::from_ymd(2025, 4, 15).unwrap();
        let end_date = Date::from_ymd(2025, 7, 15).unwrap();

        let fra = FRA::new(trade_date, start_date, end_date, 0.05);

        assert_eq!(fra.trade_date(), trade_date);
        assert_eq!(fra.start_date(), start_date);
        assert_eq!(fra.end_date(), end_date);
        assert_eq!(fra.fixed_rate(), 0.05);
        assert_eq!(fra.instrument_type(), InstrumentType::FRA);
    }

    #[test]
    fn test_fra_from_tenors() {
        let trade_date = Date::from_ymd(2025, 1, 15).unwrap();
        let fra = FRA::from_tenors(trade_date, 3, 6, 0.05).unwrap();

        assert_eq!(fra.start_date(), Date::from_ymd(2025, 4, 15).unwrap());
        assert_eq!(fra.end_date(), Date::from_ymd(2025, 7, 15).unwrap());
    }

    #[test]
    fn test_fra_forward_rate_flat_curve() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        let fra = FRA::from_tenors(ref_date, 3, 6, 0.05).unwrap();

        // For a flat curve, forward rate should approximately equal the zero rate
        let curve = flat_curve(ref_date, 0.05);
        let fwd = fra.forward_rate(&curve).unwrap();

        // Forward should be close to 5% for flat curve
        assert_relative_eq!(fwd, 0.05, epsilon = 0.001);
    }

    #[test]
    fn test_fra_pv_at_market() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        let curve = flat_curve(ref_date, 0.05);

        // FRA at the market forward rate should have PV = 0
        let fra = FRA::from_tenors(ref_date, 3, 6, 0.05).unwrap();
        let pv = fra.pv(&curve).unwrap();

        // PV should be approximately zero
        assert_relative_eq!(pv, 0.0, epsilon = 100.0); // ~$100 tolerance on $1M notional
    }

    #[test]
    fn test_fra_implied_df() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        let fra = FRA::from_tenors(ref_date, 3, 6, 0.05).unwrap();

        let curve = flat_curve(ref_date, 0.05);
        let implied = fra.implied_df(&curve, 0.0).unwrap();

        // Should be positive and less than DF at start
        let t_start = year_fraction_act360(ref_date, fra.start_date());
        let df_start = curve.discount_factor(t_start).unwrap();

        assert!(implied > 0.0);
        assert!(implied < df_start);
    }

    #[test]
    fn test_fra_pv_positive_when_rate_below_market() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        let curve = flat_curve(ref_date, 0.06); // Market at 6%

        // FRA locked in at 5% (below market) - we receive 6%, pay 5%
        let fra = FRA::from_tenors(ref_date, 3, 6, 0.05)
            .unwrap()
            .with_notional(1_000_000.0);
        let pv = fra.pv(&curve).unwrap();

        // Should have positive PV (we're receiving above what we pay)
        assert!(pv > 0.0);
    }

    #[test]
    fn test_fra_description() {
        let trade_date = Date::from_ymd(2025, 1, 15).unwrap();
        let fra = FRA::from_tenors(trade_date, 3, 6, 0.05).unwrap();

        let desc = fra.description();
        assert!(desc.contains("FRA"));
        assert!(desc.contains("5.0"));
    }
}
