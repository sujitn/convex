//! Overnight Index Swap (OIS) instrument.
//!
//! OIS swaps are used to construct the primary discounting curve.

use convex_core::Date;

use super::{year_fraction_act360, CurveInstrument, InstrumentType};
use crate::error::CurveResult;
use crate::traits::Curve;

/// Overnight Index Swap.
///
/// An OIS exchanges a fixed rate for the daily compounded overnight rate
/// (e.g., SOFR, €STR, SONIA) over the swap term.
///
/// # Pricing
///
/// For single-period OIS:
/// ```text
/// Fixed Leg: c × τ × DF(end)
/// Float Leg: DF(start) - DF(end)  (daily compounding approximation)
/// ```
///
/// At par: `c × τ × DF(end) = DF(start) - DF(end)`
///
/// Solving: `DF(end) = DF(start) / (1 + c × τ)`
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::instruments::OIS;
///
/// // 5-year SOFR OIS at 4.50%
/// let ois = OIS::new(
///     effective_date,
///     termination_date,
///     0.0450,
/// );
/// ```
#[derive(Debug, Clone)]
pub struct OIS {
    /// Effective date (start)
    effective_date: Date,
    /// Termination date (end)
    termination_date: Date,
    /// Fixed rate
    fixed_rate: f64,
    /// Payment lag in days (typically 2 for SOFR)
    payment_lag: u32,
    /// Notional amount
    notional: f64,
}

impl OIS {
    /// Creates a new OIS.
    ///
    /// # Arguments
    ///
    /// * `effective_date` - Swap start date
    /// * `termination_date` - Swap end date
    /// * `fixed_rate` - Fixed rate
    pub fn new(effective_date: Date, termination_date: Date, fixed_rate: f64) -> Self {
        Self {
            effective_date,
            termination_date,
            fixed_rate,
            payment_lag: 2, // Standard SOFR payment lag
            notional: 1_000_000.0,
        }
    }

    /// Creates an OIS from tenor string.
    ///
    /// # Arguments
    ///
    /// * `effective_date` - Swap start date
    /// * `tenor` - Tenor string (e.g., "1Y", "5Y", "10Y")
    /// * `fixed_rate` - Fixed rate
    pub fn from_tenor(effective_date: Date, tenor: &str, fixed_rate: f64) -> CurveResult<Self> {
        let termination_date = parse_swap_tenor(effective_date, tenor)?;
        Ok(Self::new(effective_date, termination_date, fixed_rate))
    }

    /// Sets the payment lag.
    #[must_use]
    pub fn with_payment_lag(mut self, days: u32) -> Self {
        self.payment_lag = days;
        self
    }

    /// Sets the notional.
    #[must_use]
    pub fn with_notional(mut self, notional: f64) -> Self {
        self.notional = notional;
        self
    }

    /// Returns the effective date.
    #[must_use]
    pub fn effective_date(&self) -> Date {
        self.effective_date
    }

    /// Returns the termination date.
    #[must_use]
    pub fn termination_date(&self) -> Date {
        self.termination_date
    }

    /// Returns the fixed rate.
    #[must_use]
    pub fn fixed_rate(&self) -> f64 {
        self.fixed_rate
    }

    /// Returns the year fraction.
    #[must_use]
    pub fn year_fraction(&self) -> f64 {
        year_fraction_act360(self.effective_date, self.termination_date)
    }
}

impl CurveInstrument for OIS {
    fn maturity(&self) -> Date {
        self.termination_date
    }

    fn pv(&self, curve: &dyn Curve) -> CurveResult<f64> {
        let ref_date = curve.reference_date();
        let t_start = year_fraction_act360(ref_date, self.effective_date);
        let t_end = year_fraction_act360(ref_date, self.termination_date);

        let df_start = curve.discount_factor(t_start)?;
        let df_end = curve.discount_factor(t_end)?;

        let tau = self.year_fraction();

        // Fixed leg - Float leg
        let fixed_pv = self.fixed_rate * tau * df_end;
        let float_pv = df_start - df_end;

        Ok(self.notional * (fixed_pv - float_pv))
    }

    fn implied_df(&self, curve: &dyn Curve, _target_pv: f64) -> CurveResult<f64> {
        // DF(end) = DF(start) / (1 + c × τ)
        let ref_date = curve.reference_date();
        let t_start = year_fraction_act360(ref_date, self.effective_date);

        let df_start = curve.discount_factor(t_start)?;
        let tau = self.year_fraction();

        Ok(df_start / (1.0 + self.fixed_rate * tau))
    }

    fn instrument_type(&self) -> InstrumentType {
        InstrumentType::OIS
    }

    fn description(&self) -> String {
        let years = self.year_fraction();
        format!("OIS {:.1}Y at {:.4}%", years, self.fixed_rate * 100.0)
    }
}

/// Parse swap tenor string.
fn parse_swap_tenor(start: Date, tenor: &str) -> CurveResult<Date> {
    let tenor = tenor.to_uppercase();

    if tenor.ends_with('Y') {
        let years: i32 = tenor[..tenor.len() - 1].parse().map_err(|_| {
            crate::error::CurveError::invalid_data(format!("Invalid tenor: {}", tenor))
        })?;
        start.add_years(years).map_err(|e| {
            crate::error::CurveError::invalid_data(format!("Failed to calculate end date: {}", e))
        })
    } else if tenor.ends_with('M') {
        let months: i32 = tenor[..tenor.len() - 1].parse().map_err(|_| {
            crate::error::CurveError::invalid_data(format!("Invalid tenor: {}", tenor))
        })?;
        start.add_months(months).map_err(|e| {
            crate::error::CurveError::invalid_data(format!("Failed to calculate end date: {}", e))
        })
    } else if tenor.ends_with('W') {
        let weeks: i64 = tenor[..tenor.len() - 1].parse().map_err(|_| {
            crate::error::CurveError::invalid_data(format!("Invalid tenor: {}", tenor))
        })?;
        Ok(start.add_days(weeks * 7))
    } else {
        Err(crate::error::CurveError::invalid_data(format!(
            "Invalid tenor format: {}",
            tenor
        )))
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
            .add_zero_rate(1.0, rate)
            .add_zero_rate(5.0, rate)
            .add_zero_rate(10.0, rate)
            .with_interpolation(InterpolationMethod::LogLinear)
            .with_extrapolation()
            .build()
            .unwrap()
    }

    #[test]
    fn test_ois_basic() {
        let eff = Date::from_ymd(2025, 1, 3).unwrap();
        let term = Date::from_ymd(2030, 1, 3).unwrap();
        let ois = OIS::new(eff, term, 0.045);

        assert_eq!(ois.effective_date(), eff);
        assert_eq!(ois.termination_date(), term);
        assert_eq!(ois.fixed_rate(), 0.045);
        assert_eq!(ois.instrument_type(), InstrumentType::OIS);
    }

    #[test]
    fn test_ois_from_tenor() {
        let eff = Date::from_ymd(2025, 1, 3).unwrap();
        let ois = OIS::from_tenor(eff, "5Y", 0.045).unwrap();

        assert_eq!(ois.termination_date(), Date::from_ymd(2030, 1, 3).unwrap());
    }

    #[test]
    fn test_ois_implied_df() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        let ois = OIS::from_tenor(ref_date, "1Y", 0.05).unwrap();

        let curve = flat_curve(ref_date, 0.05);
        let implied = ois.implied_df(&curve, 0.0).unwrap();

        // DF = 1 / (1 + 0.05 * τ) where τ ≈ 1.0
        let tau = ois.year_fraction();
        let expected = 1.0 / (1.0 + 0.05 * tau);

        assert_relative_eq!(implied, expected, epsilon = 1e-6);
    }

    #[test]
    fn test_ois_pv_at_par() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        let rate = 0.05;

        // Build curve consistent with the OIS rate
        let ois = OIS::from_tenor(ref_date, "1Y", rate).unwrap();
        let implied_df = ois.implied_df(&flat_curve(ref_date, rate), 0.0).unwrap();

        let curve = DiscountCurveBuilder::new(ref_date)
            .add_pillar(0.0, 1.0)
            .add_pillar(ois.year_fraction(), implied_df)
            .with_interpolation(InterpolationMethod::LogLinear)
            .build()
            .unwrap();

        let pv = ois.pv(&curve).unwrap();
        assert_relative_eq!(pv, 0.0, epsilon = 1.0); // ~$1 on $1M notional
    }

    #[test]
    fn test_parse_swap_tenor() {
        let start = Date::from_ymd(2025, 1, 15).unwrap();

        let end_1y = parse_swap_tenor(start, "1Y").unwrap();
        assert_eq!(end_1y, Date::from_ymd(2026, 1, 15).unwrap());

        let end_6m = parse_swap_tenor(start, "6M").unwrap();
        assert_eq!(end_6m, Date::from_ymd(2025, 7, 15).unwrap());

        let end_2w = parse_swap_tenor(start, "2W").unwrap();
        assert_eq!(end_2w, start.add_days(14));
    }
}
