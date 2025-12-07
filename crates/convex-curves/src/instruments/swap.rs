//! Interest Rate Swap (IRS) instrument.
//!
//! IRS swaps exchange fixed for floating payments and are the primary
//! instruments for constructing the medium-to-long end of the yield curve.

use convex_core::types::Frequency;
use convex_core::Date;

use super::{year_fraction_act360, CurveInstrument, InstrumentType, RateIndex};
use crate::error::CurveResult;
use crate::traits::Curve;

/// Interest Rate Swap.
///
/// An IRS exchanges fixed rate payments for floating rate payments.
/// The floating leg is typically linked to an overnight rate (SOFR, €STR)
/// or term rate (EURIBOR).
///
/// # Pricing
///
/// ```text
/// Fixed Leg PV: Σ c × τi × DF(Ti)
/// Float Leg PV: DF(T0) - DF(Tn)  (single curve, telescoping)
/// ```
///
/// For multi-curve: Float Leg uses projection curve, discounting uses OIS.
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::instruments::Swap;
///
/// // 10-year SOFR swap at 4.25%
/// let swap = Swap::new(
///     effective_date,
///     termination_date,
///     0.0425,
///     Frequency::SemiAnnual,
/// );
/// ```
#[derive(Debug, Clone)]
pub struct Swap {
    /// Effective date
    effective_date: Date,
    /// Termination date
    termination_date: Date,
    /// Fixed rate
    fixed_rate: f64,
    /// Fixed leg payment frequency
    fixed_frequency: Frequency,
    /// Float index reference
    float_index: RateIndex,
    /// Notional amount
    notional: f64,
}

impl Swap {
    /// Creates a new IRS.
    ///
    /// # Arguments
    ///
    /// * `effective_date` - Swap start date
    /// * `termination_date` - Swap end date
    /// * `fixed_rate` - Fixed leg rate
    /// * `fixed_frequency` - Fixed leg payment frequency
    #[must_use]
    pub fn new(
        effective_date: Date,
        termination_date: Date,
        fixed_rate: f64,
        fixed_frequency: Frequency,
    ) -> Self {
        Self {
            effective_date,
            termination_date,
            fixed_rate,
            fixed_frequency,
            float_index: RateIndex::sofr_3m(),
            notional: 1_000_000.0,
        }
    }

    /// Creates a swap from tenor.
    pub fn from_tenor(
        effective_date: Date,
        tenor: &str,
        fixed_rate: f64,
        fixed_frequency: Frequency,
    ) -> CurveResult<Self> {
        let termination_date = parse_tenor(effective_date, tenor)?;
        Ok(Self::new(
            effective_date,
            termination_date,
            fixed_rate,
            fixed_frequency,
        ))
    }

    /// Creates a standard SOFR swap (annual fixed).
    #[must_use]
    pub fn sofr(effective_date: Date, termination_date: Date, fixed_rate: f64) -> Self {
        Self::new(
            effective_date,
            termination_date,
            fixed_rate,
            Frequency::Annual,
        )
    }

    /// Sets the float index.
    #[must_use]
    pub fn with_float_index(mut self, index: RateIndex) -> Self {
        self.float_index = index;
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

    /// Returns the fixed frequency.
    #[must_use]
    pub fn fixed_frequency(&self) -> Frequency {
        self.fixed_frequency
    }

    /// Generates fixed leg payment dates.
    #[must_use]
    pub fn fixed_payment_dates(&self) -> Vec<Date> {
        generate_schedule(
            self.effective_date,
            self.termination_date,
            self.fixed_frequency,
        )
    }

    /// Calculates the fixed leg PV.
    pub fn fixed_leg_pv(&self, curve: &dyn Curve) -> CurveResult<f64> {
        let ref_date = curve.reference_date();
        let dates = self.fixed_payment_dates();
        let mut pv = 0.0;

        let mut prev_date = self.effective_date;
        for &date in &dates {
            let tau = year_fraction_act360(prev_date, date);
            let t = year_fraction_act360(ref_date, date);
            let df = curve.discount_factor(t)?;
            pv += self.fixed_rate * tau * df;
            prev_date = date;
        }

        Ok(pv * self.notional)
    }

    /// Calculates the float leg PV (single curve approximation).
    ///
    /// Uses the telescoping property: Float PV = DF(start) - DF(end)
    pub fn float_leg_pv(&self, curve: &dyn Curve) -> CurveResult<f64> {
        let ref_date = curve.reference_date();
        let t_start = year_fraction_act360(ref_date, self.effective_date);
        let t_end = year_fraction_act360(ref_date, self.termination_date);

        let df_start = curve.discount_factor(t_start)?;
        let df_end = curve.discount_factor(t_end)?;

        Ok((df_start - df_end) * self.notional)
    }

    /// Returns the sum of known (already-solved) fixed leg DFs.
    ///
    /// Used in bootstrap when only the final DF is unknown.
    pub fn sum_known_fixed_leg(&self, curve: &dyn Curve) -> CurveResult<f64> {
        let ref_date = curve.reference_date();
        let dates = self.fixed_payment_dates();

        if dates.len() <= 1 {
            return Ok(0.0);
        }

        let mut pv = 0.0;
        let mut prev_date = self.effective_date;

        // Sum all but the last payment
        for &date in dates.iter().take(dates.len() - 1) {
            let tau = year_fraction_act360(prev_date, date);
            let t = year_fraction_act360(ref_date, date);
            let df = curve.discount_factor(t)?;
            pv += self.fixed_rate * tau * df;
            prev_date = date;
        }

        Ok(pv * self.notional)
    }

    /// Returns the final period year fraction.
    #[must_use]
    pub fn final_period_tau(&self) -> f64 {
        let dates = self.fixed_payment_dates();
        if dates.len() <= 1 {
            return year_fraction_act360(self.effective_date, self.termination_date);
        }

        let second_to_last = dates[dates.len() - 2];
        year_fraction_act360(second_to_last, self.termination_date)
    }
}

impl CurveInstrument for Swap {
    fn maturity(&self) -> Date {
        self.termination_date
    }

    fn pv(&self, curve: &dyn Curve) -> CurveResult<f64> {
        // PV = Fixed Leg - Float Leg
        let fixed = self.fixed_leg_pv(curve)?;
        let float = self.float_leg_pv(curve)?;
        Ok(fixed - float)
    }

    fn implied_df(&self, curve: &dyn Curve, _target_pv: f64) -> CurveResult<f64> {
        // Solve: Σ c×τi×DF(Ti) = DF(T0) - DF(Tn)
        // For the last period:
        // sum_known + c×τn×DF(Tn) = DF(T0) - DF(Tn)
        // DF(Tn) × (1 + c×τn) = DF(T0) - sum_known/N
        // DF(Tn) = (DF(T0) - sum_known/N) / (1 + c×τn)

        let ref_date = curve.reference_date();
        let t_start = year_fraction_act360(ref_date, self.effective_date);
        let df_start = curve.discount_factor(t_start)?;

        let sum_known = self.sum_known_fixed_leg(curve)? / self.notional;
        let tau_n = self.final_period_tau();

        Ok((df_start - sum_known) / (1.0 + self.fixed_rate * tau_n))
    }

    fn instrument_type(&self) -> InstrumentType {
        InstrumentType::Swap
    }

    fn description(&self) -> String {
        let dates = self.fixed_payment_dates();
        let years = dates.len() as f64 / f64::from(self.fixed_frequency.periods_per_year());
        let float_index = &self.float_index;
        let rate = self.fixed_rate * 100.0;
        format!("IRS {years:.0}Y {float_index} at {rate:.4}%")
    }
}

/// Generates a payment schedule.
fn generate_schedule(start: Date, end: Date, frequency: Frequency) -> Vec<Date> {
    let months_per_period = frequency.months_per_period() as i32;
    if months_per_period == 0 {
        return vec![end];
    }

    let mut dates = Vec::new();
    let mut current = start;

    loop {
        if let Ok(next) = current.add_months(months_per_period) {
            if next > end {
                break;
            }
            dates.push(next);
            current = next;
        } else {
            break;
        }
    }

    // Ensure termination date is included
    if dates.last() != Some(&end) {
        dates.push(end);
    }

    dates
}

/// Parse tenor string.
fn parse_tenor(start: Date, tenor: &str) -> CurveResult<Date> {
    let tenor = tenor.to_uppercase();

    if tenor.ends_with('Y') {
        let years: i32 = tenor[..tenor.len() - 1].parse().map_err(|_| {
            crate::error::CurveError::invalid_data(format!("Invalid tenor: {tenor}"))
        })?;
        start.add_years(years).map_err(|e| {
            crate::error::CurveError::invalid_data(format!("Failed to calculate end date: {e}"))
        })
    } else if tenor.ends_with('M') {
        let months: i32 = tenor[..tenor.len() - 1].parse().map_err(|_| {
            crate::error::CurveError::invalid_data(format!("Invalid tenor: {tenor}"))
        })?;
        start.add_months(months).map_err(|e| {
            crate::error::CurveError::invalid_data(format!("Failed to calculate end date: {e}"))
        })
    } else {
        Err(crate::error::CurveError::invalid_data(format!(
            "Invalid tenor format: {tenor}"
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
            .add_zero_rate(0.5, rate)
            .add_zero_rate(1.0, rate)
            .add_zero_rate(2.0, rate)
            .add_zero_rate(5.0, rate)
            .add_zero_rate(10.0, rate)
            .with_interpolation(InterpolationMethod::LogLinear)
            .with_extrapolation()
            .build()
            .unwrap()
    }

    #[test]
    fn test_swap_basic() {
        let eff = Date::from_ymd(2025, 1, 3).unwrap();
        let term = Date::from_ymd(2030, 1, 3).unwrap();
        let swap = Swap::new(eff, term, 0.045, Frequency::SemiAnnual);

        assert_eq!(swap.effective_date(), eff);
        assert_eq!(swap.termination_date(), term);
        assert_eq!(swap.fixed_rate(), 0.045);
        assert_eq!(swap.instrument_type(), InstrumentType::Swap);
    }

    #[test]
    fn test_swap_from_tenor() {
        let eff = Date::from_ymd(2025, 1, 3).unwrap();
        let swap = Swap::from_tenor(eff, "5Y", 0.045, Frequency::SemiAnnual).unwrap();

        assert_eq!(swap.termination_date(), Date::from_ymd(2030, 1, 3).unwrap());
    }

    #[test]
    fn test_generate_schedule_semi_annual() {
        let start = Date::from_ymd(2025, 1, 15).unwrap();
        let end = Date::from_ymd(2027, 1, 15).unwrap();

        let dates = generate_schedule(start, end, Frequency::SemiAnnual);

        // 2 years, semi-annual = 4 payments
        assert_eq!(dates.len(), 4);
        assert_eq!(dates[0], Date::from_ymd(2025, 7, 15).unwrap());
        assert_eq!(dates[1], Date::from_ymd(2026, 1, 15).unwrap());
        assert_eq!(dates[2], Date::from_ymd(2026, 7, 15).unwrap());
        assert_eq!(dates[3], Date::from_ymd(2027, 1, 15).unwrap());
    }

    #[test]
    fn test_generate_schedule_annual() {
        let start = Date::from_ymd(2025, 1, 15).unwrap();
        let end = Date::from_ymd(2027, 1, 15).unwrap();

        let dates = generate_schedule(start, end, Frequency::Annual);

        // 2 years, annual = 2 payments
        assert_eq!(dates.len(), 2);
        assert_eq!(dates[0], Date::from_ymd(2026, 1, 15).unwrap());
        assert_eq!(dates[1], Date::from_ymd(2027, 1, 15).unwrap());
    }

    #[test]
    fn test_swap_payment_dates() {
        let eff = Date::from_ymd(2025, 1, 15).unwrap();
        let swap = Swap::from_tenor(eff, "2Y", 0.045, Frequency::SemiAnnual).unwrap();

        let dates = swap.fixed_payment_dates();
        assert_eq!(dates.len(), 4);
    }

    #[test]
    fn test_swap_fixed_leg_pv() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        let swap = Swap::from_tenor(ref_date, "1Y", 0.05, Frequency::Annual)
            .unwrap()
            .with_notional(1_000_000.0);

        let curve = flat_curve(ref_date, 0.05);
        let fixed_pv = swap.fixed_leg_pv(&curve).unwrap();

        // Fixed PV = c × τ × DF(1Y) × N
        // ≈ 0.05 × 1.0 × exp(-0.05) × 1M
        let expected = 0.05 * (360.0 / 360.0) * (-0.05_f64).exp() * 1_000_000.0;
        assert_relative_eq!(fixed_pv, expected, epsilon = 1000.0); // ~$1000 tolerance
    }

    #[test]
    fn test_swap_float_leg_pv() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        let swap = Swap::from_tenor(ref_date, "1Y", 0.05, Frequency::Annual)
            .unwrap()
            .with_notional(1_000_000.0);

        let curve = flat_curve(ref_date, 0.05);
        let float_pv = swap.float_leg_pv(&curve).unwrap();

        // Float PV = (DF(0) - DF(1)) × N = (1 - exp(-0.05)) × 1M
        let expected = (1.0 - (-0.05_f64).exp()) * 1_000_000.0;
        assert_relative_eq!(float_pv, expected, epsilon = 1000.0);
    }

    #[test]
    fn test_swap_pv_at_par() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();

        // At the par swap rate, PV should be approximately zero
        // For a 1Y swap with flat 5% curve, par rate is approximately 5%
        let swap = Swap::from_tenor(ref_date, "1Y", 0.05, Frequency::Annual)
            .unwrap()
            .with_notional(1_000_000.0);

        let curve = flat_curve(ref_date, 0.05);
        let pv = swap.pv(&curve).unwrap();

        // PV should be close to zero
        assert!(pv.abs() < 5000.0); // Within $5000 on $1M notional
    }

    #[test]
    fn test_swap_implied_df() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();
        let swap = Swap::from_tenor(ref_date, "1Y", 0.05, Frequency::Annual).unwrap();

        let curve = flat_curve(ref_date, 0.05);
        let implied = swap.implied_df(&curve, 0.0).unwrap();

        // For 1Y swap, implied DF ≈ 1 / (1 + r)
        assert!(implied > 0.0);
        assert!(implied < 1.0);
        assert_relative_eq!(implied, 1.0 / (1.0 + 0.05), epsilon = 0.01);
    }
}
