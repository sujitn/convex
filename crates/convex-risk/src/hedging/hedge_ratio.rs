//! Hedge ratio calculations.
//!
//! Calculate the optimal hedge ratio to neutralize interest rate risk.

use crate::dv01::DV01;
use crate::RiskError;

/// Calculate DV01-neutral hedge ratio.
///
/// # Arguments
///
/// * `target_dv01` - DV01 of the position to hedge
/// * `hedge_instrument_dv01` - DV01 of the hedging instrument (per unit notional)
///
/// # Returns
///
/// The notional amount of hedge instrument needed (negative for short position)
pub fn dv01_hedge_ratio(target_dv01: DV01, hedge_instrument_dv01: DV01) -> Result<f64, RiskError> {
    if hedge_instrument_dv01.as_f64().abs() < 1e-10 {
        return Err(RiskError::DivisionByZero {
            context: "hedge instrument DV01 is zero".to_string(),
        });
    }

    // To neutralize: target_dv01 + hedge_ratio × hedge_dv01 = 0
    // hedge_ratio = -target_dv01 / hedge_dv01
    let ratio = -target_dv01.as_f64() / hedge_instrument_dv01.as_f64();
    Ok(ratio)
}

/// Calculate duration-neutral hedge ratio.
///
/// # Arguments
///
/// * `target_duration` - Duration of the position to hedge
/// * `target_value` - Market value of the position
/// * `hedge_duration` - Duration of the hedging instrument
/// * `hedge_price` - Price of the hedging instrument (per unit)
pub fn duration_hedge_ratio(
    target_duration: f64,
    target_value: f64,
    hedge_duration: f64,
    hedge_price: f64,
) -> Result<f64, RiskError> {
    if hedge_duration.abs() < 1e-10 || hedge_price.abs() < 1e-10 {
        return Err(RiskError::DivisionByZero {
            context: "hedge duration or price is zero".to_string(),
        });
    }

    // To neutralize: D_target × V_target + D_hedge × V_hedge = 0
    // V_hedge = -(D_target × V_target) / D_hedge
    // Units = V_hedge / price
    let hedge_value = -(target_duration * target_value) / hedge_duration;
    let units = hedge_value / hedge_price;
    Ok(units)
}

/// Hedge result containing recommended hedge positions.
#[derive(Debug, Clone)]
pub struct HedgeRecommendation {
    /// Notional amount of hedge instrument
    pub notional: f64,
    /// Direction: positive = long, negative = short
    pub direction: HedgeDirection,
    /// Residual DV01 after hedge
    pub residual_dv01: f64,
}

/// Hedge direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HedgeDirection {
    Long,
    Short,
}

impl HedgeRecommendation {
    /// Create a new hedge recommendation
    pub fn new(notional: f64, residual_dv01: f64) -> Self {
        Self {
            notional: notional.abs(),
            direction: if notional >= 0.0 {
                HedgeDirection::Long
            } else {
                HedgeDirection::Short
            },
            residual_dv01,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_dv01_hedge_ratio() {
        // Long position with $1000 DV01, hedge with instrument having $50 DV01 per unit
        let target = DV01::from(1000.0);
        let hedge = DV01::from(50.0);

        let ratio = dv01_hedge_ratio(target, hedge).unwrap();

        // Need to short 20 units to neutralize
        assert_relative_eq!(ratio, -20.0, epsilon = 0.01);
    }

    #[test]
    fn test_duration_hedge_ratio() {
        let target_dur = 5.0;
        let target_value = 10_000_000.0;
        let hedge_dur = 7.5;
        let hedge_price = 100.0;

        let units = duration_hedge_ratio(target_dur, target_value, hedge_dur, hedge_price).unwrap();

        // Hedge value = -(5 × 10M) / 7.5 = -6.67M
        // Units = -6.67M / 100 = -66,667
        assert_relative_eq!(units, -66666.67, epsilon = 1.0);
    }
}
