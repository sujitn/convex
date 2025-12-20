//! Credit curve wrapper providing credit/survival semantics.
//!
//! `CreditCurve<T>` wraps any `TermStructure` and provides semantic methods
//! for credit operations regardless of the underlying value type.

use convex_core::types::Date;

use crate::conversion::ValueConverter;
use crate::error::{CurveError, CurveResult};
use crate::term_structure::TermStructure;
use crate::value_type::ValueType;

/// A wrapper providing credit operations on any term structure.
///
/// This wrapper handles the conversion from whatever value type the
/// underlying curve stores to the requested credit representation.
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::{CreditCurve, DiscreteCurve};
///
/// let curve = DiscreteCurve::new(...)?;
/// let credit_curve = CreditCurve::new(curve, 0.40); // 40% recovery
///
/// // Get survival probability
/// let surv = credit_curve.survival_probability(maturity_date)?;
///
/// // Get hazard rate
/// let hazard = credit_curve.hazard_rate(date)?;
///
/// // Get risky discount factor
/// let risky_df = credit_curve.risky_discount_factor(date, &discount_curve)?;
/// ```
#[derive(Clone, Debug)]
pub struct CreditCurve<T: TermStructure> {
    /// The underlying term structure.
    inner: T,
    /// Recovery rate assumption (typically 0.40 for senior unsecured).
    recovery_rate: f64,
}

impl<T: TermStructure> CreditCurve<T> {
    /// Creates a new credit curve wrapper.
    ///
    /// # Arguments
    ///
    /// * `inner` - The underlying term structure
    /// * `recovery_rate` - Recovery rate as decimal (e.g., 0.40 for 40%)
    pub fn new(inner: T, recovery_rate: f64) -> Self {
        Self {
            inner,
            recovery_rate: recovery_rate.clamp(0.0, 1.0),
        }
    }

    /// Returns a reference to the underlying term structure.
    #[must_use]
    pub fn inner(&self) -> &T {
        &self.inner
    }

    /// Returns the recovery rate.
    #[must_use]
    pub fn recovery_rate(&self) -> f64 {
        self.recovery_rate
    }

    /// Returns the reference date.
    #[must_use]
    pub fn reference_date(&self) -> Date {
        self.inner.reference_date()
    }

    /// Converts a date to a tenor in years.
    fn date_to_tenor(&self, date: Date) -> f64 {
        self.inner.date_to_tenor(date)
    }

    /// Returns the survival probability at the given date.
    ///
    /// Q(T) = P(τ > T) where τ is the default time.
    ///
    /// # Arguments
    ///
    /// * `date` - Target date
    ///
    /// # Returns
    ///
    /// Survival probability in [0, 1].
    pub fn survival_probability(&self, date: Date) -> CurveResult<f64> {
        let t = self.date_to_tenor(date);
        self.survival_probability_at_tenor(t)
    }

    /// Returns the survival probability at a tenor (years).
    pub fn survival_probability_at_tenor(&self, t: f64) -> CurveResult<f64> {
        if t <= 0.0 {
            return Ok(1.0);
        }

        let value = self.inner.value_at(t);
        let value_type = self.inner.value_type();

        match value_type {
            ValueType::SurvivalProbability => Ok(value.clamp(0.0, 1.0)),
            ValueType::HazardRate => {
                // For constant hazard rate: Q(t) = exp(-h * t)
                Ok(ValueConverter::hazard_to_survival(value, t))
            }
            ValueType::CreditSpread { recovery, .. } => {
                // Convert spread to hazard rate, then to survival
                // Spread ≈ h * (1 - R)
                let lgd = 1.0 - recovery;
                let hazard = if lgd > 1e-10 { value / lgd } else { 0.0 };
                Ok(ValueConverter::hazard_to_survival(hazard, t))
            }
            _ => Err(CurveError::incompatible_value_type(
                "SurvivalProbability, HazardRate, or CreditSpread",
                format!("{:?}", value_type),
            )),
        }
    }

    /// Returns the default probability at the given date.
    ///
    /// P(τ ≤ T) = 1 - Q(T)
    ///
    /// # Arguments
    ///
    /// * `date` - Target date
    ///
    /// # Returns
    ///
    /// Default probability in [0, 1].
    pub fn default_probability(&self, date: Date) -> CurveResult<f64> {
        let surv = self.survival_probability(date)?;
        Ok(1.0 - surv)
    }

    /// Returns the default probability at a tenor (years).
    pub fn default_probability_at_tenor(&self, t: f64) -> CurveResult<f64> {
        let surv = self.survival_probability_at_tenor(t)?;
        Ok(1.0 - surv)
    }

    /// Returns the conditional default probability between two dates.
    ///
    /// P(τ ∈ [T1, T2] | τ > T1) = (Q(T1) - Q(T2)) / Q(T1)
    ///
    /// This is the probability of default in the period [T1, T2]
    /// conditional on having survived to T1.
    ///
    /// # Arguments
    ///
    /// * `start` - Start date of period
    /// * `end` - End date of period
    ///
    /// # Returns
    ///
    /// Conditional default probability.
    pub fn conditional_default_probability(&self, start: Date, end: Date) -> CurveResult<f64> {
        let t1 = self.date_to_tenor(start);
        let t2 = self.date_to_tenor(end);
        self.conditional_default_probability_at_tenors(t1, t2)
    }

    /// Returns the conditional default probability between two tenors.
    pub fn conditional_default_probability_at_tenors(&self, t1: f64, t2: f64) -> CurveResult<f64> {
        if t2 <= t1 {
            return Err(CurveError::invalid_value(
                "End tenor must be after start tenor",
            ));
        }

        let surv1 = self.survival_probability_at_tenor(t1)?;
        let surv2 = self.survival_probability_at_tenor(t2)?;

        if surv1 <= 1e-10 {
            // Already defaulted
            return Ok(0.0);
        }

        Ok((surv1 - surv2) / surv1)
    }

    /// Returns the marginal default probability between two dates.
    ///
    /// P(τ ∈ [T1, T2]) = Q(T1) - Q(T2)
    ///
    /// This is the unconditional probability of default in the period.
    ///
    /// # Arguments
    ///
    /// * `start` - Start date of period
    /// * `end` - End date of period
    ///
    /// # Returns
    ///
    /// Marginal default probability.
    pub fn marginal_default_probability(&self, start: Date, end: Date) -> CurveResult<f64> {
        let t1 = self.date_to_tenor(start);
        let t2 = self.date_to_tenor(end);
        self.marginal_default_probability_at_tenors(t1, t2)
    }

    /// Returns the marginal default probability between two tenors.
    pub fn marginal_default_probability_at_tenors(&self, t1: f64, t2: f64) -> CurveResult<f64> {
        let surv1 = self.survival_probability_at_tenor(t1)?;
        let surv2 = self.survival_probability_at_tenor(t2)?;
        Ok((surv1 - surv2).max(0.0))
    }

    /// Returns the instantaneous hazard rate at the given date.
    ///
    /// h(T) = -d/dT ln(Q(T)) = -Q'(T) / Q(T)
    ///
    /// The hazard rate is the instantaneous conditional default intensity.
    ///
    /// # Arguments
    ///
    /// * `date` - Target date
    ///
    /// # Returns
    ///
    /// The hazard rate.
    pub fn hazard_rate(&self, date: Date) -> CurveResult<f64> {
        let t = self.date_to_tenor(date);
        self.hazard_rate_at_tenor(t)
    }

    /// Returns the hazard rate at a tenor.
    pub fn hazard_rate_at_tenor(&self, t: f64) -> CurveResult<f64> {
        if t <= 0.0 {
            return Ok(0.0);
        }

        let value_type = self.inner.value_type();

        match value_type {
            ValueType::HazardRate => Ok(self.inner.value_at(t)),
            ValueType::SurvivalProbability => {
                // Try to get derivative for exact hazard rate
                if let Some(d_surv) = self.inner.derivative_at(t) {
                    let surv = self.inner.value_at(t);
                    Ok(ValueConverter::survival_to_hazard(surv, d_surv))
                } else {
                    // Fall back to implied constant hazard rate
                    let surv = self.inner.value_at(t);
                    Ok(ValueConverter::implied_hazard_rate(surv, t))
                }
            }
            ValueType::CreditSpread { recovery, .. } => {
                // Spread ≈ h * (1 - R)
                let spread = self.inner.value_at(t);
                let lgd = 1.0 - recovery;
                Ok(if lgd > 1e-10 { spread / lgd } else { 0.0 })
            }
            _ => Err(CurveError::incompatible_value_type(
                "HazardRate, SurvivalProbability, or CreditSpread",
                format!("{:?}", value_type),
            )),
        }
    }

    /// Returns the implied constant hazard rate to the given date.
    ///
    /// This is the flat hazard rate that would produce the observed
    /// survival probability: h = -ln(Q(T)) / T
    ///
    /// # Arguments
    ///
    /// * `date` - Target date
    ///
    /// # Returns
    ///
    /// The implied constant hazard rate.
    pub fn implied_hazard_rate(&self, date: Date) -> CurveResult<f64> {
        let t = self.date_to_tenor(date);
        let surv = self.survival_probability_at_tenor(t)?;
        Ok(ValueConverter::implied_hazard_rate(surv, t))
    }

    /// Returns the credit spread at the given date.
    ///
    /// The spread is defined as:
    /// s(T) ≈ h(T) * (1 - R)
    ///
    /// where h is the hazard rate and R is the recovery rate.
    ///
    /// # Arguments
    ///
    /// * `date` - Target date
    ///
    /// # Returns
    ///
    /// The credit spread (as a decimal, e.g., 0.01 for 100bps).
    pub fn credit_spread(&self, date: Date) -> CurveResult<f64> {
        let t = self.date_to_tenor(date);
        self.credit_spread_at_tenor(t)
    }

    /// Returns the credit spread at a tenor.
    pub fn credit_spread_at_tenor(&self, t: f64) -> CurveResult<f64> {
        let value_type = self.inner.value_type();

        match value_type {
            ValueType::CreditSpread { .. } => Ok(self.inner.value_at(t)),
            ValueType::HazardRate | ValueType::SurvivalProbability => {
                let hazard = self.hazard_rate_at_tenor(t)?;
                let lgd = 1.0 - self.recovery_rate;
                Ok(hazard * lgd)
            }
            _ => Err(CurveError::incompatible_value_type(
                "CreditSpread, HazardRate, or SurvivalProbability",
                format!("{:?}", value_type),
            )),
        }
    }

    /// Returns the credit spread in basis points.
    pub fn credit_spread_bps(&self, date: Date) -> CurveResult<f64> {
        Ok(self.credit_spread(date)? * 10000.0)
    }

    /// Returns the risky discount factor at the given date.
    ///
    /// The risky discount factor combines the risk-free discount factor
    /// with the survival probability:
    ///
    /// P_risky(T) = P(T) * [Q(T) + (1 - Q(T)) * R]
    ///
    /// where P is the risk-free DF, Q is survival probability, and R is recovery.
    ///
    /// # Arguments
    ///
    /// * `date` - Target date
    /// * `discount_curve` - Risk-free discount curve
    ///
    /// # Returns
    ///
    /// The risky discount factor.
    pub fn risky_discount_factor<D: TermStructure>(
        &self,
        date: Date,
        discount_curve: &crate::wrappers::RateCurve<D>,
    ) -> CurveResult<f64> {
        let t = self.date_to_tenor(date);
        self.risky_discount_factor_at_tenor(t, discount_curve)
    }

    /// Returns the risky discount factor at a tenor.
    pub fn risky_discount_factor_at_tenor<D: TermStructure>(
        &self,
        t: f64,
        discount_curve: &crate::wrappers::RateCurve<D>,
    ) -> CurveResult<f64> {
        let df = discount_curve.discount_factor_at_tenor(t)?;
        let surv = self.survival_probability_at_tenor(t)?;
        Ok(ValueConverter::risky_discount_factor(
            df,
            surv,
            self.recovery_rate,
        ))
    }

    /// Returns the expected loss at the given date.
    ///
    /// Expected Loss = (1 - R) * P(default by T)
    ///
    /// # Arguments
    ///
    /// * `date` - Target date
    ///
    /// # Returns
    ///
    /// Expected loss as a fraction of exposure.
    pub fn expected_loss(&self, date: Date) -> CurveResult<f64> {
        let pd = self.default_probability(date)?;
        let lgd = 1.0 - self.recovery_rate;
        Ok(pd * lgd)
    }

    /// Returns the expected loss at a tenor.
    pub fn expected_loss_at_tenor(&self, t: f64) -> CurveResult<f64> {
        let pd = self.default_probability_at_tenor(t)?;
        let lgd = 1.0 - self.recovery_rate;
        Ok(pd * lgd)
    }

    /// Returns the annualized default probability to the given date.
    ///
    /// This is useful for comparing default probabilities across different
    /// time horizons.
    ///
    /// PD_annual = 1 - (1 - PD)^(1/T)
    ///
    /// # Arguments
    ///
    /// * `date` - Target date
    ///
    /// # Returns
    ///
    /// Annualized default probability.
    pub fn annualized_default_probability(&self, date: Date) -> CurveResult<f64> {
        let t = self.date_to_tenor(date);
        if t <= 0.0 {
            return Ok(0.0);
        }

        let surv = self.survival_probability_at_tenor(t)?;
        let annual_surv = surv.powf(1.0 / t);
        Ok(1.0 - annual_surv)
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

// Implement TermStructure for CreditCurve so it can be nested
impl<T: TermStructure> TermStructure for CreditCurve<T> {
    fn reference_date(&self) -> Date {
        self.inner.reference_date()
    }

    fn value_at(&self, t: f64) -> f64 {
        self.inner.value_at(t)
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curves::DiscreteCurve;
    use crate::wrappers::RateCurve;
    use crate::InterpolationMethod;
    use approx::assert_relative_eq;
    use convex_core::daycounts::DayCountConvention;
    use convex_core::types::Compounding;

    fn sample_survival_curve() -> CreditCurve<DiscreteCurve> {
        let today = Date::from_ymd(2024, 1, 1).unwrap();
        // Create survival probabilities for 2% constant hazard rate
        let tenors = vec![1.0, 2.0, 3.0, 5.0, 10.0];
        let survivals: Vec<f64> = tenors.iter().map(|&t| ((-0.02 * t) as f64).exp()).collect();

        let curve = DiscreteCurve::new(
            today,
            tenors,
            survivals,
            ValueType::SurvivalProbability,
            InterpolationMethod::LogLinear,
        )
        .unwrap();

        CreditCurve::new(curve, 0.40) // 40% recovery
    }

    fn sample_hazard_curve() -> CreditCurve<DiscreteCurve> {
        let today = Date::from_ymd(2024, 1, 1).unwrap();
        // Flat hazard rate of 2%
        let tenors = vec![1.0, 2.0, 3.0, 5.0, 10.0];
        let hazards = vec![0.02; 5];

        let curve = DiscreteCurve::new(
            today,
            tenors,
            hazards,
            ValueType::HazardRate,
            InterpolationMethod::Linear,
        )
        .unwrap();

        CreditCurve::new(curve, 0.40)
    }

    fn sample_discount_curve() -> RateCurve<DiscreteCurve> {
        let today = Date::from_ymd(2024, 1, 1).unwrap();
        let tenors = vec![1.0, 2.0, 3.0, 5.0, 10.0];
        let dfs: Vec<f64> = tenors.iter().map(|&t| ((-0.05 * t) as f64).exp()).collect();

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

    #[test]
    fn test_survival_probability() {
        let curve = sample_survival_curve();
        let t = 5.0;

        let surv = curve.survival_probability_at_tenor(t).unwrap();
        let expected = (-0.02 * t).exp();

        assert_relative_eq!(surv, expected, epsilon = 1e-6);
    }

    #[test]
    fn test_survival_from_hazard() {
        let curve = sample_hazard_curve();
        let t = 5.0;

        let surv = curve.survival_probability_at_tenor(t).unwrap();
        let expected = (-0.02 * t).exp();

        assert_relative_eq!(surv, expected, epsilon = 1e-6);
    }

    #[test]
    fn test_default_probability() {
        let curve = sample_survival_curve();
        let t = 5.0;

        let pd = curve.default_probability_at_tenor(t).unwrap();
        let surv = curve.survival_probability_at_tenor(t).unwrap();

        assert_relative_eq!(pd + surv, 1.0, epsilon = 1e-10);
    }

    #[test]
    fn test_conditional_default_probability() {
        let curve = sample_survival_curve();

        let cond_pd = curve
            .conditional_default_probability_at_tenors(1.0, 2.0)
            .unwrap();

        // For constant hazard rate h, conditional PD over [t1, t2] = 1 - exp(-h*(t2-t1))
        let expected = 1.0 - (-0.02 * 1.0_f64).exp();

        assert_relative_eq!(cond_pd, expected, epsilon = 1e-4);
    }

    #[test]
    fn test_marginal_default_probability() {
        let curve = sample_survival_curve();

        let marginal_pd = curve
            .marginal_default_probability_at_tenors(1.0, 2.0)
            .unwrap();

        // Q(1) - Q(2) = exp(-0.02) - exp(-0.04)
        let expected = (-0.02_f64).exp() - (-0.04_f64).exp();

        assert_relative_eq!(marginal_pd, expected, epsilon = 1e-6);
    }

    #[test]
    fn test_implied_hazard_rate() {
        let curve = sample_survival_curve();
        let today = curve.reference_date();
        let target = today.add_days((5.0 * 365.0) as i64);

        let hazard = curve.implied_hazard_rate(target).unwrap();

        // Should be approximately 2%
        assert_relative_eq!(hazard, 0.02, epsilon = 1e-4);
    }

    #[test]
    fn test_credit_spread() {
        let curve = sample_survival_curve();
        let t = 5.0;

        let spread = curve.credit_spread_at_tenor(t).unwrap();

        // Spread = h * (1 - R) = 0.02 * 0.60 = 0.012 = 120bps
        let expected = 0.02 * 0.60;
        assert_relative_eq!(spread, expected, epsilon = 1e-4);
    }

    #[test]
    fn test_credit_spread_bps() {
        let curve = sample_survival_curve();
        let today = curve.reference_date();
        let target = today.add_days((5.0 * 365.0) as i64);

        let spread_bps = curve.credit_spread_bps(target).unwrap();

        // Should be ~120 bps
        assert_relative_eq!(spread_bps, 120.0, epsilon = 1.0);
    }

    #[test]
    fn test_risky_discount_factor() {
        let credit = sample_survival_curve();
        let discount = sample_discount_curve();
        let t = 5.0;

        let risky_df = credit.risky_discount_factor_at_tenor(t, &discount).unwrap();

        // P_risky = P * (Q + (1-Q) * R)
        let df = (-0.05 * t).exp();
        let surv = (-0.02 * t).exp();
        let expected = df * (surv + (1.0 - surv) * 0.40);

        assert_relative_eq!(risky_df, expected, epsilon = 1e-6);
    }

    #[test]
    fn test_expected_loss() {
        let curve = sample_survival_curve();
        let t = 5.0;

        let el = curve.expected_loss_at_tenor(t).unwrap();

        // EL = (1 - R) * PD = 0.60 * (1 - exp(-0.02 * 5))
        let pd = 1.0 - (-0.02 * t).exp();
        let expected = 0.60 * pd;

        assert_relative_eq!(el, expected, epsilon = 1e-6);
    }

    #[test]
    fn test_survival_at_zero() {
        let curve = sample_survival_curve();
        let surv = curve.survival_probability_at_tenor(0.0).unwrap();
        assert_relative_eq!(surv, 1.0, epsilon = 1e-10);
    }

    #[test]
    fn test_default_at_zero() {
        let curve = sample_survival_curve();
        let pd = curve.default_probability_at_tenor(0.0).unwrap();
        assert_relative_eq!(pd, 0.0, epsilon = 1e-10);
    }

    #[test]
    fn test_recovery_rate() {
        let curve = sample_survival_curve();
        assert_relative_eq!(curve.recovery_rate(), 0.40, epsilon = 1e-10);
    }

    #[test]
    fn test_annualized_default_probability() {
        let curve = sample_survival_curve();
        let today = curve.reference_date();
        let target = today.add_days((5.0 * 365.0) as i64);

        let annual_pd = curve.annualized_default_probability(target).unwrap();

        // For constant hazard rate, annual PD = 1 - exp(-h) ≈ 0.0198
        let expected = 1.0 - (-0.02_f64).exp();
        assert_relative_eq!(annual_pd, expected, epsilon = 1e-4);
    }

    #[test]
    fn test_tenor_bounds() {
        let curve = sample_survival_curve();
        let (min, max) = curve.tenor_bounds();
        assert_relative_eq!(min, 1.0, epsilon = 1e-10);
        assert_relative_eq!(max, 10.0, epsilon = 1e-10);
    }
}
