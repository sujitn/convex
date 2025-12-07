//! Money market deposit instrument.
//!
//! A deposit is the simplest instrument for curve bootstrap,
//! used for the short end of the curve (O/N, T/N, 1W to 12M).

use convex_core::daycounts::DayCountConvention;
use convex_core::Date;
use rust_decimal::prelude::ToPrimitive;

use super::{year_fraction_act360, CurveInstrument, InstrumentType};
use crate::error::{CurveError, CurveResult};
use crate::traits::Curve;

/// A money market deposit.
///
/// Deposits are used to bootstrap the short end of the yield curve,
/// typically for maturities from overnight to 12 months.
///
/// # Pricing Formula
///
/// The present value is zero when:
/// ```text
/// DF(end) = DF(start) / (1 + rate × τ)
/// ```
/// where τ is the year fraction using the specified day count convention.
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::instruments::Deposit;
///
/// // 3-month deposit at 5.25%
/// let deposit = Deposit::new(
///     start_date,
///     end_date,
///     0.0525,
/// );
/// ```
#[derive(Debug, Clone, Copy)]
pub struct Deposit {
    /// Start date (spot date, typically T+2)
    start_date: Date,
    /// End date (maturity)
    end_date: Date,
    /// Simple interest rate (e.g., 0.0525 for 5.25%)
    rate: f64,
    /// Notional amount (default 1.0)
    notional: f64,
    /// Day count convention (default ACT/360)
    day_count: DayCountConvention,
}

impl Deposit {
    /// Creates a new deposit.
    ///
    /// # Arguments
    ///
    /// * `start_date` - Deposit start date (spot)
    /// * `end_date` - Deposit maturity date
    /// * `rate` - Simple interest rate
    #[must_use]
    pub fn new(start_date: Date, end_date: Date, rate: f64) -> Self {
        Self {
            start_date,
            end_date,
            rate,
            notional: 1.0,
            day_count: DayCountConvention::Act360,
        }
    }

    /// Creates a deposit with a specified day count convention.
    #[must_use]
    pub fn with_day_count(mut self, day_count: DayCountConvention) -> Self {
        self.day_count = day_count;
        self
    }

    /// Creates a deposit with a specified notional.
    #[must_use]
    pub fn with_notional(mut self, notional: f64) -> Self {
        self.notional = notional;
        self
    }

    /// Creates a deposit from tenor string (e.g., "3M", "6M", "1Y").
    ///
    /// # Arguments
    ///
    /// * `spot_date` - The spot date (start of deposit)
    /// * `tenor` - Tenor string (e.g., "ON", "TN", "1W", "1M", "3M", "6M", "12M", "1Y")
    /// * `rate` - Simple interest rate
    pub fn from_tenor(spot_date: Date, tenor: &str, rate: f64) -> CurveResult<Self> {
        let end_date = parse_tenor(spot_date, tenor)?;
        Ok(Self::new(spot_date, end_date, rate))
    }

    /// Returns the start date.
    #[must_use]
    pub fn start_date(&self) -> Date {
        self.start_date
    }

    /// Returns the end date (maturity).
    #[must_use]
    pub fn end_date(&self) -> Date {
        self.end_date
    }

    /// Returns the deposit rate.
    #[must_use]
    pub fn rate(&self) -> f64 {
        self.rate
    }

    /// Returns the notional.
    #[must_use]
    pub fn notional(&self) -> f64 {
        self.notional
    }

    /// Returns the year fraction for the deposit period.
    #[must_use]
    pub fn year_fraction(&self) -> f64 {
        self.day_count
            .to_day_count()
            .year_fraction(self.start_date, self.end_date)
            .to_f64()
            .unwrap_or(0.0)
    }
}

impl CurveInstrument for Deposit {
    fn maturity(&self) -> Date {
        self.end_date
    }

    fn pv(&self, curve: &dyn Curve) -> CurveResult<f64> {
        // PV = N × [DF(start) - DF(end) × (1 + r × τ)]
        let t_start = year_fraction_act360(curve.reference_date(), self.start_date);
        let t_end = year_fraction_act360(curve.reference_date(), self.end_date);

        let df_start = curve.discount_factor(t_start)?;
        let df_end = curve.discount_factor(t_end)?;

        let tau = self.year_fraction();
        let pv = self.notional * (df_start - df_end * (1.0 + self.rate * tau));

        Ok(pv)
    }

    fn implied_df(&self, curve: &dyn Curve, _target_pv: f64) -> CurveResult<f64> {
        // DF(end) = DF(start) / (1 + r × τ)
        let t_start = year_fraction_act360(curve.reference_date(), self.start_date);
        let df_start = curve.discount_factor(t_start)?;
        let tau = self.year_fraction();

        Ok(df_start / (1.0 + self.rate * tau))
    }

    fn instrument_type(&self) -> InstrumentType {
        InstrumentType::Deposit
    }

    fn description(&self) -> String {
        let rate = self.rate * 100.0;
        let start_date = self.start_date;
        let end_date = self.end_date;
        format!("Deposit {rate:.4}% {start_date} to {end_date}")
    }
}

/// Parses a tenor string and returns the end date.
fn parse_tenor(start: Date, tenor: &str) -> CurveResult<Date> {
    let tenor = tenor.to_uppercase();

    match tenor.as_str() {
        "ON" | "O/N" => Ok(start.add_days(1)),
        "TN" | "T/N" => Ok(start.add_days(2)),
        "SN" | "S/N" => Ok(start.add_days(3)),
        _ => {
            // Parse numeric tenor like "1W", "3M", "1Y"
            let (num_str, unit) = if tenor.ends_with('W') {
                (&tenor[..tenor.len() - 1], 'W')
            } else if tenor.ends_with('M') {
                (&tenor[..tenor.len() - 1], 'M')
            } else if tenor.ends_with('Y') {
                (&tenor[..tenor.len() - 1], 'Y')
            } else {
                return Err(crate::error::CurveError::invalid_data(format!(
                    "Invalid tenor format: {tenor}"
                )));
            };

            let num: i64 = num_str.parse().map_err(|_| {
                crate::error::CurveError::invalid_data(format!("Invalid tenor number: {num_str}"))
            })?;

            match unit {
                'W' => Ok(start.add_days(num * 7)),
                'M' => start
                    .add_months(num as i32)
                    .map_err(|e| CurveError::invalid_data(format!("Failed to add months: {e}"))),
                'Y' => start
                    .add_years(num as i32)
                    .map_err(|e| CurveError::invalid_data(format!("Failed to add years: {e}"))),
                _ => unreachable!(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curves::DiscountCurveBuilder;
    use crate::interpolation::InterpolationMethod;
    use approx::assert_relative_eq;

    fn flat_curve(rate: f64) -> impl Curve {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        DiscountCurveBuilder::new(ref_date)
            .add_zero_rate(0.25, rate)
            .add_zero_rate(1.0, rate)
            .add_zero_rate(5.0, rate)
            .with_interpolation(InterpolationMethod::LogLinear)
            .with_extrapolation()
            .build()
            .unwrap()
    }

    #[test]
    fn test_deposit_basic() {
        let start = Date::from_ymd(2025, 1, 3).unwrap();
        let end = Date::from_ymd(2025, 4, 3).unwrap();
        let deposit = Deposit::new(start, end, 0.05);

        assert_eq!(deposit.start_date(), start);
        assert_eq!(deposit.end_date(), end);
        assert_eq!(deposit.rate(), 0.05);
        assert_eq!(deposit.instrument_type(), InstrumentType::Deposit);
    }

    #[test]
    fn test_deposit_year_fraction() {
        let start = Date::from_ymd(2025, 1, 1).unwrap();
        let end = Date::from_ymd(2025, 7, 1).unwrap();
        let deposit = Deposit::new(start, end, 0.05);

        // 181 days / 360
        let expected = 181.0 / 360.0;
        assert_relative_eq!(deposit.year_fraction(), expected, epsilon = 1e-10);
    }

    #[test]
    fn test_deposit_implied_df() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        let start = ref_date;
        let end = Date::from_ymd(2025, 4, 1).unwrap();

        let deposit = Deposit::new(start, end, 0.05);

        // Create a simple curve where DF(start) = 1.0
        let curve = flat_curve(0.05);

        // DF(end) = 1 / (1 + 0.05 * τ)
        let tau = deposit.year_fraction();
        let expected_df = 1.0 / (1.0 + 0.05 * tau);

        let implied = deposit.implied_df(&curve, 0.0).unwrap();
        assert_relative_eq!(implied, expected_df, epsilon = 1e-6);
    }

    #[test]
    fn test_deposit_pv_at_par() {
        // When the curve is built from this deposit, PV should be ~0
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        let end = Date::from_ymd(2025, 4, 1).unwrap();

        let deposit = Deposit::new(ref_date, end, 0.05);
        let implied_df = deposit.implied_df(&flat_curve(0.05), 0.0).unwrap();

        // Build a curve with this implied DF
        let curve = DiscountCurveBuilder::new(ref_date)
            .add_pillar(0.0, 1.0) // DF at start
            .add_pillar(deposit.year_fraction(), implied_df)
            .with_interpolation(InterpolationMethod::LogLinear)
            .build()
            .unwrap();

        let pv = deposit.pv(&curve).unwrap();
        assert_relative_eq!(pv, 0.0, epsilon = 1e-10);
    }

    #[test]
    fn test_parse_tenor_overnight() {
        let start = Date::from_ymd(2025, 1, 15).unwrap();

        let on = parse_tenor(start, "ON").unwrap();
        assert_eq!(on, start.add_days(1));

        let tn = parse_tenor(start, "TN").unwrap();
        assert_eq!(tn, start.add_days(2));
    }

    #[test]
    fn test_parse_tenor_weeks() {
        let start = Date::from_ymd(2025, 1, 15).unwrap();

        let one_week = parse_tenor(start, "1W").unwrap();
        assert_eq!(one_week, start.add_days(7));

        let two_weeks = parse_tenor(start, "2W").unwrap();
        assert_eq!(two_weeks, start.add_days(14));
    }

    #[test]
    fn test_parse_tenor_months() {
        let start = Date::from_ymd(2025, 1, 15).unwrap();

        let three_months = parse_tenor(start, "3M").unwrap();
        assert_eq!(three_months, Date::from_ymd(2025, 4, 15).unwrap());

        let six_months = parse_tenor(start, "6M").unwrap();
        assert_eq!(six_months, Date::from_ymd(2025, 7, 15).unwrap());
    }

    #[test]
    fn test_parse_tenor_years() {
        let start = Date::from_ymd(2025, 1, 15).unwrap();

        let one_year = parse_tenor(start, "1Y").unwrap();
        assert_eq!(one_year, Date::from_ymd(2026, 1, 15).unwrap());
    }

    #[test]
    fn test_deposit_from_tenor() {
        let spot = Date::from_ymd(2025, 1, 3).unwrap();
        let deposit = Deposit::from_tenor(spot, "3M", 0.05).unwrap();

        assert_eq!(deposit.start_date(), spot);
        assert_eq!(deposit.end_date(), Date::from_ymd(2025, 4, 3).unwrap());
    }
}
