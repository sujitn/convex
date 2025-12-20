//! Value type conversions for term structures.
//!
//! This module provides utilities for converting between different curve
//! representations:
//!
//! - Discount factors ↔ Zero rates
//! - Zero rates ↔ Forward rates
//! - Survival probabilities ↔ Hazard rates
//! - Compounding convention conversions
//!
//! # Mathematical Background
//!
//! ## Discount Factors and Zero Rates
//!
//! For continuous compounding:
//! - `P(t) = exp(-r(t) * t)`
//! - `r(t) = -ln(P(t)) / t`
//!
//! For periodic compounding (n times per year):
//! - `P(t) = (1 + r(t)/n)^(-n*t)`
//! - `r(t) = n * (P(t)^(-1/(n*t)) - 1)`
//!
//! ## Forward Rates
//!
//! The instantaneous forward rate is:
//! - `f(t) = -d/dt ln(P(t)) = r(t) + t * dr/dt`
//!
//! The forward rate from t1 to t2:
//! - `F(t1,t2) = (t2*r(t2) - t1*r(t1)) / (t2 - t1)` (continuous)
//!
//! ## Credit Curves
//!
//! For survival probability and hazard rate:
//! - `Q(t) = exp(-∫₀ᵗ h(s) ds)`
//! - `h(t) = -d/dt ln(Q(t))`

use convex_core::types::Compounding;

/// Value conversion utilities for term structures.
pub struct ValueConverter;

impl ValueConverter {
    // ========================================================================
    // Discount Factor ↔ Zero Rate conversions
    // ========================================================================

    /// Converts a discount factor to a zero rate.
    ///
    /// # Arguments
    ///
    /// * `df` - Discount factor (should be in (0, 1])
    /// * `t` - Time in years (must be > 0)
    /// * `compounding` - Compounding convention
    ///
    /// # Returns
    ///
    /// The zero rate as a decimal (e.g., 0.05 for 5%)
    ///
    /// # Example
    ///
    /// ```rust
    /// use convex_curves::ValueConverter;
    /// use convex_core::types::Compounding;
    ///
    /// let df = 0.9512; // Discount factor
    /// let t = 1.0;     // 1 year
    /// let rate = ValueConverter::df_to_zero(df, t, Compounding::Continuous);
    /// assert!((rate - 0.05).abs() < 0.001); // ~5%
    /// ```
    #[must_use]
    pub fn df_to_zero(df: f64, t: f64, compounding: Compounding) -> f64 {
        if t <= 0.0 || df <= 0.0 {
            return 0.0;
        }

        match compounding {
            Compounding::Continuous => -df.ln() / t,
            Compounding::Simple => (1.0 / df - 1.0) / t,
            Compounding::Annual => df.powf(-1.0 / t) - 1.0,
            Compounding::SemiAnnual => {
                let n = 2.0;
                n * (df.powf(-1.0 / (n * t)) - 1.0)
            }
            Compounding::Quarterly => {
                let n = 4.0;
                n * (df.powf(-1.0 / (n * t)) - 1.0)
            }
            Compounding::Monthly => {
                let n = 12.0;
                n * (df.powf(-1.0 / (n * t)) - 1.0)
            }
            Compounding::Daily => {
                let n = 365.0;
                n * (df.powf(-1.0 / (n * t)) - 1.0)
            }
        }
    }

    /// Converts a zero rate to a discount factor.
    ///
    /// # Arguments
    ///
    /// * `rate` - Zero rate as a decimal (e.g., 0.05 for 5%)
    /// * `t` - Time in years
    /// * `compounding` - Compounding convention
    ///
    /// # Returns
    ///
    /// Discount factor in (0, 1]
    ///
    /// # Example
    ///
    /// ```rust
    /// use convex_curves::ValueConverter;
    /// use convex_core::types::Compounding;
    ///
    /// let rate = 0.05; // 5%
    /// let t = 1.0;     // 1 year
    /// let df = ValueConverter::zero_to_df(rate, t, Compounding::Continuous);
    /// assert!((df - 0.9512).abs() < 0.001);
    /// ```
    #[must_use]
    pub fn zero_to_df(rate: f64, t: f64, compounding: Compounding) -> f64 {
        if t <= 0.0 {
            return 1.0;
        }

        match compounding {
            Compounding::Continuous => (-rate * t).exp(),
            Compounding::Simple => 1.0 / (1.0 + rate * t),
            Compounding::Annual => (1.0 + rate).powf(-t),
            Compounding::SemiAnnual => {
                let n = 2.0;
                (1.0 + rate / n).powf(-n * t)
            }
            Compounding::Quarterly => {
                let n = 4.0;
                (1.0 + rate / n).powf(-n * t)
            }
            Compounding::Monthly => {
                let n = 12.0;
                (1.0 + rate / n).powf(-n * t)
            }
            Compounding::Daily => {
                let n = 365.0;
                (1.0 + rate / n).powf(-n * t)
            }
        }
    }

    // ========================================================================
    // Compounding Convention conversions
    // ========================================================================

    /// Converts a rate from one compounding convention to another.
    ///
    /// # Arguments
    ///
    /// * `rate` - The rate to convert
    /// * `from` - Source compounding convention
    /// * `to` - Target compounding convention
    ///
    /// # Returns
    ///
    /// The equivalent rate under the target compounding convention.
    ///
    /// # Example
    ///
    /// ```rust
    /// use convex_curves::ValueConverter;
    /// use convex_core::types::Compounding;
    ///
    /// let continuous_rate = 0.05;
    /// let annual_rate = ValueConverter::convert_compounding(
    ///     continuous_rate,
    ///     Compounding::Continuous,
    ///     Compounding::Annual,
    /// );
    /// assert!((annual_rate - 0.05127).abs() < 0.0001);
    /// ```
    #[must_use]
    pub fn convert_compounding(rate: f64, from: Compounding, to: Compounding) -> f64 {
        if from == to {
            return rate;
        }

        // Convert to continuous as intermediate step
        let continuous = Self::to_continuous(rate, from);

        // Convert from continuous to target
        Self::from_continuous(continuous, to)
    }

    /// Converts a rate to continuous compounding.
    #[must_use]
    fn to_continuous(rate: f64, compounding: Compounding) -> f64 {
        match compounding {
            Compounding::Continuous => rate,
            Compounding::Simple => {
                // For simple interest, approximation using 1-year period
                (1.0 + rate).ln()
            }
            Compounding::Annual => (1.0 + rate).ln(),
            Compounding::SemiAnnual => 2.0 * (1.0 + rate / 2.0).ln(),
            Compounding::Quarterly => 4.0 * (1.0 + rate / 4.0).ln(),
            Compounding::Monthly => 12.0 * (1.0 + rate / 12.0).ln(),
            Compounding::Daily => 365.0 * (1.0 + rate / 365.0).ln(),
        }
    }

    /// Converts from continuous compounding to another convention.
    #[must_use]
    fn from_continuous(continuous_rate: f64, to: Compounding) -> f64 {
        match to {
            Compounding::Continuous => continuous_rate,
            Compounding::Simple => continuous_rate.exp() - 1.0,
            Compounding::Annual => continuous_rate.exp() - 1.0,
            Compounding::SemiAnnual => 2.0 * ((continuous_rate / 2.0).exp() - 1.0),
            Compounding::Quarterly => 4.0 * ((continuous_rate / 4.0).exp() - 1.0),
            Compounding::Monthly => 12.0 * ((continuous_rate / 12.0).exp() - 1.0),
            Compounding::Daily => 365.0 * ((continuous_rate / 365.0).exp() - 1.0),
        }
    }

    // ========================================================================
    // Forward Rate calculations
    // ========================================================================

    /// Computes the instantaneous forward rate from zero rate and its derivative.
    ///
    /// The instantaneous forward rate is:
    /// `f(t) = r(t) + t * dr/dt`
    ///
    /// # Arguments
    ///
    /// * `zero_rate` - Zero rate at time t (continuously compounded)
    /// * `d_zero_dt` - Derivative of zero rate with respect to t
    /// * `t` - Time in years
    ///
    /// # Returns
    ///
    /// The instantaneous forward rate.
    #[must_use]
    pub fn instantaneous_forward(zero_rate: f64, d_zero_dt: f64, t: f64) -> f64 {
        zero_rate + t * d_zero_dt
    }

    /// Computes the forward rate between two times from zero rates.
    ///
    /// For continuously compounded zero rates:
    /// `F(t1,t2) = (t2*r(t2) - t1*r(t1)) / (t2 - t1)`
    ///
    /// # Arguments
    ///
    /// * `zero1` - Zero rate at t1 (continuously compounded)
    /// * `zero2` - Zero rate at t2 (continuously compounded)
    /// * `t1` - Start time
    /// * `t2` - End time
    ///
    /// # Returns
    ///
    /// The forward rate from t1 to t2.
    #[must_use]
    pub fn forward_rate_from_zeros(zero1: f64, zero2: f64, t1: f64, t2: f64) -> f64 {
        if (t2 - t1).abs() < 1e-10 {
            return zero2; // Degenerate case
        }
        (t2 * zero2 - t1 * zero1) / (t2 - t1)
    }

    /// Computes the forward rate between two times from discount factors.
    ///
    /// `F(t1,t2) = ln(P(t1)/P(t2)) / (t2 - t1)`
    ///
    /// # Arguments
    ///
    /// * `df1` - Discount factor at t1
    /// * `df2` - Discount factor at t2
    /// * `t1` - Start time
    /// * `t2` - End time
    /// * `compounding` - Compounding convention for the result
    ///
    /// # Returns
    ///
    /// The forward rate from t1 to t2.
    #[must_use]
    pub fn forward_rate_from_dfs(
        df1: f64,
        df2: f64,
        t1: f64,
        t2: f64,
        compounding: Compounding,
    ) -> f64 {
        let dt = t2 - t1;
        if dt.abs() < 1e-10 || df2 <= 0.0 {
            return 0.0;
        }

        let ratio = df1 / df2;

        match compounding {
            Compounding::Continuous => ratio.ln() / dt,
            Compounding::Simple => (ratio - 1.0) / dt,
            _ => {
                // For periodic compounding, convert to continuous then back
                let cont_fwd = ratio.ln() / dt;
                Self::from_continuous(cont_fwd, compounding)
            }
        }
    }

    // ========================================================================
    // Credit Curve conversions
    // ========================================================================

    /// Computes hazard rate from survival probability and its derivative.
    ///
    /// `h(t) = -d/dt ln(Q(t)) = -Q'(t) / Q(t)`
    ///
    /// # Arguments
    ///
    /// * `survival_prob` - Survival probability Q(t)
    /// * `d_survival_dt` - Derivative of survival probability
    ///
    /// # Returns
    ///
    /// The hazard rate (instantaneous default intensity).
    #[must_use]
    pub fn survival_to_hazard(survival_prob: f64, d_survival_dt: f64) -> f64 {
        if survival_prob <= 0.0 {
            return 0.0;
        }
        -d_survival_dt / survival_prob
    }

    /// Computes survival probability from constant hazard rate.
    ///
    /// For constant hazard rate h:
    /// `Q(t) = exp(-h * t)`
    ///
    /// # Arguments
    ///
    /// * `hazard_rate` - Constant hazard rate
    /// * `t` - Time in years
    ///
    /// # Returns
    ///
    /// The survival probability.
    #[must_use]
    pub fn hazard_to_survival(hazard_rate: f64, t: f64) -> f64 {
        (-hazard_rate * t).exp()
    }

    /// Computes implied hazard rate from survival probability.
    ///
    /// For constant hazard rate:
    /// `h = -ln(Q(t)) / t`
    ///
    /// # Arguments
    ///
    /// * `survival_prob` - Survival probability at time t
    /// * `t` - Time in years
    ///
    /// # Returns
    ///
    /// The implied constant hazard rate.
    #[must_use]
    pub fn implied_hazard_rate(survival_prob: f64, t: f64) -> f64 {
        if t <= 0.0 || survival_prob <= 0.0 {
            return 0.0;
        }
        -survival_prob.ln() / t
    }

    /// Computes risky discount factor from risk-free DF and survival probability.
    ///
    /// The risky discount factor combines time value and credit risk:
    /// `P_risky(t) = P(t) * Q(t)`
    ///
    /// For recovery:
    /// `P_risky(t) = P(t) * [Q(t) + (1 - Q(t)) * R]`
    ///
    /// # Arguments
    ///
    /// * `df` - Risk-free discount factor
    /// * `survival_prob` - Survival probability
    /// * `recovery` - Recovery rate (typically 0.40)
    ///
    /// # Returns
    ///
    /// The risky discount factor.
    #[must_use]
    pub fn risky_discount_factor(df: f64, survival_prob: f64, recovery: f64) -> f64 {
        df * (survival_prob + (1.0 - survival_prob) * recovery)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_df_to_zero_continuous() {
        let df = (-0.05_f64).exp(); // 5% for 1 year
        let rate = ValueConverter::df_to_zero(df, 1.0, Compounding::Continuous);
        assert_relative_eq!(rate, 0.05, epsilon = 1e-10);
    }

    #[test]
    fn test_zero_to_df_continuous() {
        let rate = 0.05;
        let df = ValueConverter::zero_to_df(rate, 1.0, Compounding::Continuous);
        assert_relative_eq!(df, (-0.05_f64).exp(), epsilon = 1e-10);
    }

    #[test]
    fn test_df_zero_roundtrip() {
        for compounding in [
            Compounding::Continuous,
            Compounding::Annual,
            Compounding::SemiAnnual,
            Compounding::Quarterly,
            Compounding::Monthly,
        ] {
            let original_df = 0.95;
            let rate = ValueConverter::df_to_zero(original_df, 1.0, compounding);
            let recovered_df = ValueConverter::zero_to_df(rate, 1.0, compounding);
            assert_relative_eq!(original_df, recovered_df, epsilon = 1e-10);
        }
    }

    #[test]
    fn test_compounding_conversion() {
        // 5% continuous should be about 5.127% annual
        let continuous = 0.05;
        let annual = ValueConverter::convert_compounding(
            continuous,
            Compounding::Continuous,
            Compounding::Annual,
        );
        assert_relative_eq!(annual, 0.05_f64.exp() - 1.0, epsilon = 1e-10);

        // Roundtrip
        let back = ValueConverter::convert_compounding(
            annual,
            Compounding::Annual,
            Compounding::Continuous,
        );
        assert_relative_eq!(back, continuous, epsilon = 1e-10);
    }

    #[test]
    fn test_same_compounding_conversion() {
        let rate = 0.05;
        let result = ValueConverter::convert_compounding(
            rate,
            Compounding::Continuous,
            Compounding::Continuous,
        );
        assert_eq!(rate, result);
    }

    #[test]
    fn test_instantaneous_forward_flat_curve() {
        // For a flat curve, forward rate equals zero rate
        let zero_rate = 0.05;
        let d_zero_dt = 0.0; // Flat curve
        let t = 2.0;

        let fwd = ValueConverter::instantaneous_forward(zero_rate, d_zero_dt, t);
        assert_relative_eq!(fwd, zero_rate, epsilon = 1e-10);
    }

    #[test]
    fn test_instantaneous_forward_upward_sloping() {
        // For upward sloping curve, forward > spot
        let zero_rate = 0.05;
        let d_zero_dt = 0.005; // 50bps per year slope
        let t = 2.0;

        let fwd = ValueConverter::instantaneous_forward(zero_rate, d_zero_dt, t);
        assert!(fwd > zero_rate);
        assert_relative_eq!(fwd, 0.05 + 2.0 * 0.005, epsilon = 1e-10);
    }

    #[test]
    fn test_forward_rate_from_zeros() {
        let zero1 = 0.04; // 4% at 1Y
        let zero2 = 0.05; // 5% at 2Y
        let t1 = 1.0;
        let t2 = 2.0;

        // 1Y forward 1Y rate = (2*0.05 - 1*0.04) / (2-1) = 0.06
        let fwd = ValueConverter::forward_rate_from_zeros(zero1, zero2, t1, t2);
        assert_relative_eq!(fwd, 0.06, epsilon = 1e-10);
    }

    #[test]
    fn test_forward_rate_from_dfs() {
        let df1 = (-0.04_f64).exp(); // 4% at 1Y
        let df2 = (-0.05 * 2.0_f64).exp(); // 5% at 2Y

        let fwd = ValueConverter::forward_rate_from_dfs(df1, df2, 1.0, 2.0, Compounding::Continuous);

        // Forward should be ~6%
        assert_relative_eq!(fwd, 0.06, epsilon = 0.001);
    }

    #[test]
    fn test_survival_hazard_conversion() {
        let hazard = 0.02; // 2% hazard rate
        let t = 5.0;

        let survival = ValueConverter::hazard_to_survival(hazard, t);
        assert_relative_eq!(survival, (-0.02 * 5.0_f64).exp(), epsilon = 1e-10);

        let implied = ValueConverter::implied_hazard_rate(survival, t);
        assert_relative_eq!(implied, hazard, epsilon = 1e-10);
    }

    #[test]
    fn test_risky_discount_factor() {
        let df = 0.95; // Risk-free DF
        let survival = 0.98; // 98% survival
        let recovery = 0.40; // 40% recovery

        let risky_df = ValueConverter::risky_discount_factor(df, survival, recovery);

        // Expected: 0.95 * (0.98 + 0.02 * 0.40) = 0.95 * 0.988 = 0.9386
        let expected = df * (survival + (1.0 - survival) * recovery);
        assert_relative_eq!(risky_df, expected, epsilon = 1e-10);
    }

    #[test]
    fn test_edge_cases() {
        // Zero time
        assert_eq!(
            ValueConverter::zero_to_df(0.05, 0.0, Compounding::Continuous),
            1.0
        );

        // Zero rate
        assert_eq!(
            ValueConverter::zero_to_df(0.0, 1.0, Compounding::Continuous),
            1.0
        );

        // DF = 1 should give zero rate
        assert_eq!(
            ValueConverter::df_to_zero(1.0, 1.0, Compounding::Continuous),
            0.0
        );
    }
}
