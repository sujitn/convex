//! Inverse empirical CDF of actual signed losses, with explicit IID time scaling.
use super::{VaRMethod, VaRResult};
use crate::error::{AnalyticsError, AnalyticsResult};
#[cfg(test)]
use rust_decimal::Decimal;

/// Maximum observations admitted by one scalar historical calculation.
pub const MAX_HISTORICAL_OBSERVATIONS: usize = 1_000_000;

/// Nearest-rank signed loss quantile, rank=ceil(exact binary64 confidence*N).
/// Loss=-signed_value*return. Gains retain their negative sign. Horizon scaling
/// is sqrt(days), an explicit IID assumption rather than actual multi-day data.
pub fn historical_var(
    returns: &[f64],
    portfolio_value: f64,
    confidence_level: f64,
    horizon_days: u32,
) -> AnalyticsResult<VaRResult> {
    super::admit(portfolio_value, confidence_level, horizon_days)?;
    if returns.is_empty()
        || returns.len() > MAX_HISTORICAL_OBSERVATIONS
        || returns.iter().any(|r| !r.is_finite())
    {
        return Err(AnalyticsError::InvalidInput(
            "1..1000000 finite historical returns required".into(),
        ));
    }
    let mut losses = Vec::new();
    losses.try_reserve_exact(returns.len()).map_err(|_| {
        AnalyticsError::CalculationFailed("historical loss allocation failed".into())
    })?;
    for value in returns {
        losses.push(super::product(&[-portfolio_value, *value])?);
    }
    losses.sort_by(f64::total_cmp);
    let rank = empirical_rank(confidence_level, returns.len());
    let loss = rank
        .checked_sub(1)
        .and_then(|index| losses.get(index))
        .ok_or_else(|| {
            AnalyticsError::CalculationFailed("empirical rank outside supplied history".into())
        })?;
    let scaled = super::product(&[*loss, f64::from(horizon_days).sqrt()])?;
    Ok(VaRResult {
        var: crate::risk::cash::decimal(scaled)?,
        confidence_level,
        horizon_days,
        method: VaRMethod::Historical,
    })
}

// For an admitted 0<p<1, p=significand/2^shift. A53bit significand times
// at most1million rows fits u128. Even subnormal p has rank1, without rounding
// 1-p to1 or multiplying p*N into an ambiguous binary64 integer boundary.
fn empirical_rank(probability: f64, count: usize) -> usize {
    let bits = probability.to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as u32;
    let fraction = bits & ((1_u64 << 52) - 1);
    let (significand, shift) = if exponent == 0 {
        (fraction, 1074)
    } else {
        (fraction | (1_u64 << 52), 1075 - exponent)
    };
    let numerator = u128::from(significand) * count as u128;
    if shift >= 128 {
        return 1;
    }
    let denominator = 1_u128 << shift;
    numerator.div_ceil(denominator) as usize
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
