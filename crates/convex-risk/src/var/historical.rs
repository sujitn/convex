//! Historical VaR calculation.

use super::{VaRMethod, VaRResult};
use crate::RiskError;
use rust_decimal::Decimal;

/// Calculate historical VaR from a series of returns.
///
/// # Arguments
///
/// * `returns` - Historical returns (as decimals, e.g., -0.01 for -1%)
/// * `portfolio_value` - Current portfolio value
/// * `confidence_level` - Confidence level (e.g., 0.95 for 95%)
/// * `horizon_days` - Time horizon in days
///
/// # Returns
///
/// VaR result
pub fn historical_var(
    returns: &[f64],
    portfolio_value: f64,
    confidence_level: f64,
    horizon_days: u32,
) -> Result<VaRResult, RiskError> {
    if returns.is_empty() {
        return Err(RiskError::InsufficientData(
            "no returns provided".to_string(),
        ));
    }

    if confidence_level <= 0.0 || confidence_level >= 1.0 {
        return Err(RiskError::InvalidInput(
            "confidence level must be between 0 and 1".to_string(),
        ));
    }

    // Sort returns (ascending - worst returns first)
    let mut sorted_returns = returns.to_vec();
    sorted_returns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    // Find the VaR percentile
    let var_index = ((1.0 - confidence_level) * sorted_returns.len() as f64).floor() as usize;
    let var_return = sorted_returns[var_index];

    // Scale for time horizon (assumes daily returns)
    let scaled_return = var_return * (horizon_days as f64).sqrt();

    // Calculate VaR in absolute terms
    let var = (-scaled_return * portfolio_value).abs();

    Ok(VaRResult {
        var: Decimal::from_f64_retain(var).unwrap_or(Decimal::ZERO),
        confidence_level,
        horizon_days,
        method: VaRMethod::Historical,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_historical_var() {
        // Generate some sample returns
        let returns: Vec<f64> = vec![
            -0.02, -0.015, -0.01, -0.005, 0.0, 0.005, 0.01, 0.015, 0.02, 0.025,
        ];
        let portfolio_value = 1_000_000.0;

        let var = historical_var(&returns, portfolio_value, 0.95, 1).unwrap();

        // At 95% confidence with 10 observations, we take the worst return
        assert!(var.var > Decimal::ZERO);
        assert!((var.confidence_level - 0.95).abs() < f64::EPSILON);
        assert_eq!(var.horizon_days, 1);
    }

    #[test]
    fn test_historical_var_empty() {
        let returns: Vec<f64> = vec![];
        let result = historical_var(&returns, 1_000_000.0, 0.95, 1);
        assert!(result.is_err());
    }
}
