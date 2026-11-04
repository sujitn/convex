//! Zero-mean Gaussian scalar loss quantiles under explicit IID time scaling.
use super::{VaRMethod, VaRResult};
use crate::error::{AnalyticsError, AnalyticsResult};

/// Z-score for 90% confidence level (one-tailed).
pub const Z_SCORE_90: f64 = 1.282;
/// Z-score for 95% confidence level (one-tailed).
pub const Z_SCORE_95: f64 = 1.645;
/// Z-score for 99% confidence level (one-tailed).
pub const Z_SCORE_99: f64 = 2.326;

/// Gaussian loss quantile: inverse_normal(confidence)*abs(value)*volatility*sqrt(days).
/// Volatility is a finite nonnegative daily fractional standard deviation.
/// Negative lower-than-median quantiles are retained; inputs are never clamped.
/// This scalar model does not infer covariance, drift or multi-day scenarios.
pub fn parametric_var(
    portfolio_value: f64,
    daily_volatility: f64,
    confidence_level: f64,
    horizon_days: u32,
) -> AnalyticsResult<VaRResult> {
    super::admit(portfolio_value, confidence_level, horizon_days)?;
    if !daily_volatility.is_finite() || daily_volatility < 0.0 {
        return Err(AnalyticsError::InvalidInput(
            "finite nonnegative daily volatility required".into(),
        ));
    }
    let z = z_score_for_confidence(confidence_level)?;
    let loss = super::product(&[
        z,
        daily_volatility,
        f64::from(horizon_days).sqrt(),
        portfolio_value.abs(),
    ])?;
    Ok(VaRResult {
        var: crate::risk::cash::decimal(loss)?,
        confidence_level,
        horizon_days,
        method: VaRMethod::Parametric,
    })
}

fn z_score_for_confidence(confidence: f64) -> AnalyticsResult<f64> {
    convex_math::stats::standard_normal_inverse(confidence).map_err(Into::into)
}

/// Gaussian loss quantile using actual signed currency-per-bp DV01 and finite
/// nonnegative daily yield volatility in bp. The distribution scale uses abs(DV01).
pub fn parametric_var_from_dv01(
    dv01: f64,
    daily_yield_volatility_bps: f64,
    confidence_level: f64,
    horizon_days: u32,
) -> AnalyticsResult<VaRResult> {
    parametric_var(
        dv01,
        daily_yield_volatility_bps,
        confidence_level,
        horizon_days,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_parametric_var() {
        let portfolio_value = 1_000_000.0;
        let daily_vol = 0.01; // 1% daily volatility

        let var = parametric_var(portfolio_value, daily_vol, 0.95, 1).unwrap();

        // VaR = 1.645 × 0.01 × 1 × 1,000,000 = 16,450
        assert_relative_eq!(
            var.var.to_string().parse::<f64>().unwrap(),
            16450.0,
            epsilon = 10.0
        );
    }

    #[test]
    fn test_parametric_var_10_day() {
        let portfolio_value = 1_000_000.0;
        let daily_vol = 0.01;

        let var = parametric_var(portfolio_value, daily_vol, 0.95, 10).unwrap();

        // VaR = 1.645 × 0.01 × √10 × 1,000,000 ≈ 52,020
        assert_relative_eq!(
            var.var.to_string().parse::<f64>().unwrap(),
            52020.0,
            epsilon = 100.0
        );
    }

    #[test]
    fn test_z_score_standard_values() {
        assert_relative_eq!(
            z_score_for_confidence(0.90).unwrap(),
            Z_SCORE_90,
            epsilon = 0.001
        );
        assert_relative_eq!(
            z_score_for_confidence(0.95).unwrap(),
            Z_SCORE_95,
            epsilon = 0.001
        );
        assert_relative_eq!(
            z_score_for_confidence(0.99).unwrap(),
            Z_SCORE_99,
            epsilon = 0.001
        );
    }
}
