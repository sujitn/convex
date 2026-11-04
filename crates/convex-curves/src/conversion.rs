//! Checked conversions between explicitly supplied curve representations.
//!
//! Rates use the existing shared compounding kernel and the actual supplied
//! period. Negative rates and factors above one are valid. Survival helpers
//! describe constant intensity or an explicitly supplied terminal recovery payoff;
//! they do not infer a CDS convention, recovery model or default settlement.

use convex_core::types::Compounding;
use convex_math::portable;

use crate::contracts::{CurveContractError, CurveContractResult};
use crate::error::{CurveError, CurveResult};

fn error(error: &CurveContractError) -> CurveError {
    CurveError::invalid_value(error.to_string())
}
fn conversion(value: CurveContractResult<f64>) -> CurveResult<f64> {
    value.map_err(|source| error(&source))
}
fn finite(value: f64) -> CurveResult<f64> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(CurveError::invalid_value(
            "conversion requires finite inputs and results",
        ))
    }
}
fn time(value: f64, positive: bool) -> CurveResult<()> {
    crate::contracts::fraction(value).map_err(|source| error(&source))?;
    if positive && value == 0.0 {
        return Err(CurveError::invalid_value(
            "annualization requires a positive period",
        ));
    }
    Ok(())
}
fn ordered_times(start: f64, end: f64) -> CurveResult<f64> {
    time(start, false)?;
    time(end, false)?;
    if end <= start {
        return Err(CurveError::invalid_value(
            "forward end must follow its start",
        ));
    }
    Ok(end - start)
}
fn probability(value: f64, positive: bool) -> CurveResult<()> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) || (positive && value == 0.0) {
        return Err(CurveError::invalid_value(
            "probability outside its admitted domain",
        ));
    }
    Ok(())
}
fn nonzero_result(value: f64, nonzero_input: bool) -> CurveResult<f64> {
    finite(value)?;
    if nonzero_input && value == 0.0 {
        return Err(CurveError::invalid_value(
            "conversion erased a nonzero result",
        ));
    }
    Ok(value)
}

/// Pure checked conversions; all time inputs use the shared contract span.
pub struct ValueConverter;
impl ValueConverter {
    /// Recover an annualized rate from a positive finite discount and positive time.
    /// Factors above one retain valid negative rates.
    ///
    /// # Errors
    /// Rejects invalid domains, nonfinite arithmetic and nonzero erasure.
    ///
    /// ```
    /// use convex_curves::ValueConverter;
    /// use convex_core::types::Compounding;
    /// let rate = ValueConverter::df_to_zero(0.9512, 1.0, Compounding::Continuous).unwrap();
    /// assert!((rate - 0.05).abs() < 0.001);
    /// ```
    pub fn df_to_zero(df: f64, t: f64, compounding: Compounding) -> CurveResult<f64> {
        nonzero_result(
            conversion(crate::contracts::rate_from_discount(df, t, compounding))?,
            df != 1.0,
        )
    }

    /// Discount a supplied rate over its actual nonnegative time. P(0)=1.
    ///
    /// # Errors
    /// Rejects nonfinite inputs, invalid compounding bases and lost factors.
    ///
    /// ```
    /// use convex_curves::ValueConverter;
    /// use convex_core::types::Compounding;
    /// let df = ValueConverter::zero_to_df(0.05, 1.0, Compounding::Continuous).unwrap();
    /// assert!((df - 0.9512).abs() < 0.001);
    /// ```
    pub fn zero_to_df(rate: f64, t: f64, compounding: Compounding) -> CurveResult<f64> {
        conversion(crate::contracts::discount_from_rate(rate, t, compounding))
    }

    /// Convert equivalent rates over the same explicitly supplied positive time.
    /// Simple compounding has no implicit one-year period. Identity conversion
    /// validates its domain and preserves the original admitted rate bits.
    ///
    /// # Errors
    /// Rejects invalid time/bases, nonfinite arithmetic and nonzero erasure.
    ///
    /// ```
    /// use convex_curves::ValueConverter;
    /// use convex_core::types::Compounding;
    /// let annual = ValueConverter::convert_compounding(0.05, 1.0,
    ///     Compounding::Continuous, Compounding::Annual).unwrap();
    /// assert!((annual - 0.05127).abs() < 0.0001);
    /// ```
    pub fn convert_compounding(
        rate: f64,
        t: f64,
        from: Compounding,
        to: Compounding,
    ) -> CurveResult<f64> {
        let result = conversion(crate::contracts::convert_rate(rate, t, from, to))?;
        nonzero_result(if from == to { rate } else { result }, rate != 0.0)
    }

    /// Instantaneous forward of a differentiable zero curve in its supplied quotation.
    ///
    /// # Errors
    /// Rejects invalid time, nonfinite inputs/arithmetic and lost derivative terms.
    pub fn instantaneous_forward(
        zero_rate: f64,
        d_zero_dt: f64,
        t: f64,
        compounding: Compounding,
    ) -> CurveResult<f64> {
        time(t, false)?;
        finite(zero_rate)?;
        finite(d_zero_dt)?;
        let slope = nonzero_result(t * d_zero_dt, t != 0.0 && d_zero_dt != 0.0)?;
        match compounding {
            Compounding::Continuous => finite(zero_rate + slope),
            Compounding::Simple => {
                let base = 1.0 + zero_rate * t;
                if !base.is_finite() || base <= 0.0 {
                    return Err(CurveError::invalid_value(
                        "simple accumulation base must be positive and finite",
                    ));
                }
                let numerator = finite(zero_rate + slope)?;
                nonzero_result(numerator / base, numerator != 0.0)
            }
            _ => {
                let periods = f64::from(compounding.periods_per_year());
                let base = 1.0 + zero_rate / periods;
                // Periodic-to-continuous quotation is horizon independent.
                // Simple compounding uses its actual t in the separate branch.
                let continuous = Self::convert_compounding(
                    zero_rate,
                    1.0,
                    compounding,
                    Compounding::Continuous,
                )?;
                let adjustment = nonzero_result(slope / base, slope != 0.0)?;
                finite(continuous + adjustment)
            }
        }
    }

    /// Continuously compounded forward between strictly ordered supplied times.
    /// No short-period threshold replaces a forward with a spot rate.
    ///
    /// # Errors
    /// Rejects invalid inputs, nonfinite arithmetic and lost nonzero adjustments.
    pub fn forward_rate_from_zeros(zero1: f64, zero2: f64, t1: f64, t2: f64) -> CurveResult<f64> {
        let period = ordered_times(t1, t2)?;
        finite(zero1)?;
        finite(zero2)?;
        if zero1 == zero2 {
            return Ok(zero2);
        }
        let weight = t1 / period;
        let difference = zero2 - zero1;
        let result = if difference.is_finite() {
            let adjustment = nonzero_result(weight * difference, t1 != 0.0)?;
            zero2 + adjustment
        } else {
            finite(zero2 * (1.0 + weight))? - finite(zero1 * weight)?
        };
        finite(result)
    }

    /// Forward rate from positive discounts over the actual supplied period.
    /// Close factors use log1p; extreme ratios use a difference of finite logs.
    ///
    /// # Errors
    /// Rejects invalid discounts/time, overflow and nonzero rate erasure.
    pub fn forward_rate_from_dfs(
        df1: f64,
        df2: f64,
        t1: f64,
        t2: f64,
        compounding: Compounding,
    ) -> CurveResult<f64> {
        let period = ordered_times(t1, t2)?;
        if !df1.is_finite() || !df2.is_finite() || df1 <= 0.0 || df2 <= 0.0 {
            return Err(CurveError::invalid_value(
                "forward discounts must be positive and finite",
            ));
        }
        let relative = (df1 - df2) / df2;
        let log_ratio = if relative.is_finite() && relative.abs() <= 0.5 {
            portable::log1p(relative)
        } else {
            let ratio = df1 / df2;
            if ratio.is_finite() && ratio > 0.0 {
                portable::log(ratio)
            } else {
                portable::log(df1) - portable::log(df2)
            }
        };
        nonzero_result(
            conversion(crate::contracts::rate_from_log_accumulation(
                log_ratio,
                period,
                compounding,
            ))?,
            df1 != df2,
        )
    }

    /// Instantaneous intensity from positive survival and a nonpositive derivative.
    ///
    /// # Errors
    /// Rejects invalid probability/derivative, overflow and nonzero erasure.
    pub fn survival_to_hazard(survival_prob: f64, d_survival_dt: f64) -> CurveResult<f64> {
        probability(survival_prob, true)?;
        finite(d_survival_dt)?;
        if d_survival_dt > 0.0 {
            return Err(CurveError::invalid_value(
                "survival derivative must be nonpositive",
            ));
        }
        nonzero_result(-d_survival_dt / survival_prob, d_survival_dt != 0.0)
    }

    /// Survival under an explicitly constant, finite nonnegative intensity.
    ///
    /// # Errors
    /// Rejects invalid intensity/time and an unrepresentable positive probability.
    pub fn hazard_to_survival(hazard_rate: f64, t: f64) -> CurveResult<f64> {
        finite(hazard_rate)?;
        if hazard_rate < 0.0 {
            return Err(CurveError::invalid_value(
                "constant intensity must be nonnegative",
            ));
        }
        Self::zero_to_df(hazard_rate, t, Compounding::Continuous)
    }

    /// Constant intensity implied by positive survival over a positive period.
    ///
    /// # Errors
    /// Rejects invalid probability/time and nonfinite or erased intensity.
    pub fn implied_hazard_rate(survival_prob: f64, t: f64) -> CurveResult<f64> {
        probability(survival_prob, true)?;
        Self::df_to_zero(survival_prob, t, Compounding::Continuous)
    }

    /// Discounted terminal payoff with supplied survival and recovery at maturity.
    /// This arithmetic is not a CDS or recovery-on-default-time pricing model.
    ///
    /// # Errors
    /// Rejects invalid discounts/probabilities and nonzero payoff erasure.
    pub fn risky_discount_factor(df: f64, survival_prob: f64, recovery: f64) -> CurveResult<f64> {
        if !df.is_finite() || df <= 0.0 {
            return Err(CurveError::invalid_value(
                "discount factor must be positive and finite",
            ));
        }
        probability(survival_prob, false)?;
        probability(recovery, false)?;
        let payoff = survival_prob + (1.0 - survival_prob) * recovery;
        nonzero_result(df * payoff, payoff != 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_df_to_zero_continuous() {
        let df = (-0.05_f64).exp(); // 5% for 1 year
        let rate = ValueConverter::df_to_zero(df, 1.0, Compounding::Continuous).unwrap();
        assert_relative_eq!(rate, 0.05, epsilon = 1e-10);
    }

    #[test]
    fn test_zero_to_df_continuous() {
        let rate = 0.05;
        let df = ValueConverter::zero_to_df(rate, 1.0, Compounding::Continuous).unwrap();
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
            let rate = ValueConverter::df_to_zero(original_df, 1.0, compounding).unwrap();
            let recovered_df = ValueConverter::zero_to_df(rate, 1.0, compounding).unwrap();
            assert_relative_eq!(original_df, recovered_df, epsilon = 1e-10);
        }
    }

    #[test]
    fn test_compounding_conversion() {
        // 5% continuous should be about 5.127% annual
        let continuous = 0.05;
        let annual = ValueConverter::convert_compounding(
            continuous,
            1.0,
            Compounding::Continuous,
            Compounding::Annual,
        )
        .unwrap();
        assert_relative_eq!(annual, 0.05_f64.exp() - 1.0, epsilon = 1e-10);

        // Roundtrip
        let back = ValueConverter::convert_compounding(
            annual,
            1.0,
            Compounding::Annual,
            Compounding::Continuous,
        )
        .unwrap();
        assert_relative_eq!(back, continuous, epsilon = 1e-10);
    }

    #[test]
    fn test_same_compounding_conversion() {
        let rate = 0.05;
        let result = ValueConverter::convert_compounding(
            rate,
            1.0,
            Compounding::Continuous,
            Compounding::Continuous,
        )
        .unwrap();
        assert_eq!(rate, result);
    }

    #[test]
    fn test_instantaneous_forward_flat_curve() {
        // For a flat curve, forward rate equals zero rate
        let zero_rate = 0.05;
        let d_zero_dt = 0.0; // Flat curve
        let t = 2.0;

        let fwd =
            ValueConverter::instantaneous_forward(zero_rate, d_zero_dt, t, Compounding::Continuous)
                .unwrap();
        assert_relative_eq!(fwd, zero_rate, epsilon = 1e-10);
    }

    #[test]
    fn test_instantaneous_forward_upward_sloping() {
        // For upward sloping curve, forward > spot
        let zero_rate = 0.05;
        let d_zero_dt = 0.005; // 50bps per year slope
        let t = 2.0;

        let fwd =
            ValueConverter::instantaneous_forward(zero_rate, d_zero_dt, t, Compounding::Continuous)
                .unwrap();
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
        let fwd = ValueConverter::forward_rate_from_zeros(zero1, zero2, t1, t2).unwrap();
        assert_relative_eq!(fwd, 0.06, epsilon = 1e-10);
    }

    #[test]
    fn test_forward_rate_from_dfs() {
        let df1 = (-0.04_f64).exp(); // 4% at 1Y
        let df2 = (-0.05 * 2.0_f64).exp(); // 5% at 2Y

        let fwd =
            ValueConverter::forward_rate_from_dfs(df1, df2, 1.0, 2.0, Compounding::Continuous)
                .unwrap();

        // Forward should be ~6%
        assert_relative_eq!(fwd, 0.06, epsilon = 0.001);
    }

    #[test]
    fn test_survival_hazard_conversion() {
        let hazard = 0.02; // 2% hazard rate
        let t = 5.0;

        let survival = ValueConverter::hazard_to_survival(hazard, t).unwrap();
        assert_relative_eq!(survival, (-0.02 * 5.0_f64).exp(), epsilon = 1e-10);

        let implied = ValueConverter::implied_hazard_rate(survival, t).unwrap();
        assert_relative_eq!(implied, hazard, epsilon = 1e-10);
    }

    #[test]
    fn test_risky_discount_factor() {
        let df = 0.95; // Risk-free DF
        let survival = 0.98; // 98% survival
        let recovery = 0.40; // 40% recovery

        let risky_df = ValueConverter::risky_discount_factor(df, survival, recovery).unwrap();

        // Expected: 0.95 * (0.98 + 0.02 * 0.40) = 0.95 * 0.988 = 0.9386
        let expected = df * (survival + (1.0 - survival) * recovery);
        assert_relative_eq!(risky_df, expected, epsilon = 1e-10);
    }

    #[test]
    fn test_edge_cases() {
        // Zero time
        assert_eq!(
            ValueConverter::zero_to_df(0.05, 0.0, Compounding::Continuous).unwrap(),
            1.0
        );

        // Zero rate
        assert_eq!(
            ValueConverter::zero_to_df(0.0, 1.0, Compounding::Continuous).unwrap(),
            1.0
        );

        // DF = 1 should give zero rate
        assert_eq!(
            ValueConverter::df_to_zero(1.0, 1.0, Compounding::Continuous).unwrap(),
            0.0
        );
    }
}
