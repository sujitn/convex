//! Parametric (variance-covariance) VaR calculation.

use super::{VaRMethod, VaRResult};
use crate::error::{AnalyticsError, AnalyticsResult};
use rust_decimal::Decimal;

/// Z-scores for common confidence levels
pub const Z_SCORE_90: f64 = 1.282;
pub const Z_SCORE_95: f64 = 1.645;
pub const Z_SCORE_99: f64 = 2.326;

/// Calculate parametric VaR using the variance-covariance method.
///
/// # Arguments
///
/// * `portfolio_value` - Current portfolio value
/// * `daily_volatility` - Daily portfolio volatility (standard deviation)
/// * `confidence_level` - Confidence level (e.g., 0.95 for 95%)
/// * `horizon_days` - Time horizon in days
///
/// # Returns
///
/// VaR result
pub fn parametric_var(
    portfolio_value: f64,
    daily_volatility: f64,
    confidence_level: f64,
    horizon_days: u32,
) -> AnalyticsResult<VaRResult> {
    if daily_volatility < 0.0 {
        return Err(AnalyticsError::InvalidInput(
            "volatility cannot be negative".to_string(),
        ));
    }

    if confidence_level <= 0.0 || confidence_level >= 1.0 {
        return Err(AnalyticsError::InvalidInput(
            "confidence level must be between 0 and 1".to_string(),
        ));
    }

    // Get z-score for confidence level (using common approximations)
    let z_score = z_score_for_confidence(confidence_level);

    // VaR = z × σ × √t × V
    let var = z_score * daily_volatility * (horizon_days as f64).sqrt() * portfolio_value;

    Ok(VaRResult {
        var: Decimal::from_f64_retain(var).unwrap_or(Decimal::ZERO),
        confidence_level,
        horizon_days,
        method: VaRMethod::Parametric,
    })
}

/// Get z-score for a given confidence level.
///
/// Uses linear interpolation for non-standard confidence levels.
fn z_score_for_confidence(confidence: f64) -> f64 {
    match confidence {
        c if (c - 0.90).abs() < 0.001 => Z_SCORE_90,
        c if (c - 0.95).abs() < 0.001 => Z_SCORE_95,
        c if (c - 0.99).abs() < 0.001 => Z_SCORE_99,
        _ => {
            // Simple interpolation for other values
            if confidence < 0.95 {
                Z_SCORE_90 + (confidence - 0.90) / (0.95 - 0.90) * (Z_SCORE_95 - Z_SCORE_90)
            } else {
                Z_SCORE_95 + (confidence - 0.95) / (0.99 - 0.95) * (Z_SCORE_99 - Z_SCORE_95)
            }
        }
    }
}

/// Calculate parametric VaR using DV01 for interest rate risk.
///
/// # Arguments
///
/// * `dv01` - Portfolio DV01
/// * `daily_yield_volatility` - Daily yield volatility in basis points
/// * `confidence_level` - Confidence level
/// * `horizon_days` - Time horizon in days
pub fn parametric_var_from_dv01(
    dv01: f64,
    daily_yield_volatility_bps: f64,
    confidence_level: f64,
    horizon_days: u32,
) -> AnalyticsResult<VaRResult> {
    let z_score = z_score_for_confidence(confidence_level);

    // VaR = z × σ_yield × √t × DV01
    let var = z_score * daily_yield_volatility_bps * (horizon_days as f64).sqrt() * dv01;

    Ok(VaRResult {
        var: Decimal::from_f64_retain(var).unwrap_or(Decimal::ZERO),
        confidence_level,
        horizon_days,
        method: VaRMethod::Parametric,
    })
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
        assert_relative_eq!(z_score_for_confidence(0.90), Z_SCORE_90, epsilon = 0.001);
        assert_relative_eq!(z_score_for_confidence(0.95), Z_SCORE_95, epsilon = 0.001);
        assert_relative_eq!(z_score_for_confidence(0.99), Z_SCORE_99, epsilon = 0.001);
    }
}
