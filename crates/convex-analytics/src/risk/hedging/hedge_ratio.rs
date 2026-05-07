//! DV01- and duration-neutral hedge ratios. Numeric helpers for callers
//! that want a single ratio rather than a [`super::HedgeProposal`].

use crate::error::{AnalyticsError, AnalyticsResult};
use crate::risk::dv01::DV01;

/// Notional of `hedge_instrument` to neutralize `target_dv01`. Negative
/// result = short the hedge.
pub fn dv01_hedge_ratio(target_dv01: DV01, hedge_instrument_dv01: DV01) -> AnalyticsResult<f64> {
    if hedge_instrument_dv01.as_f64().abs() < 1e-10 {
        return Err(AnalyticsError::CalculationFailed(
            "hedge instrument DV01 is zero".to_string(),
        ));
    }
    Ok(-target_dv01.as_f64() / hedge_instrument_dv01.as_f64())
}

/// Units of `hedge_instrument` (priced at `hedge_price`) to neutralize
/// duration × market-value risk. `units = -(D_t · V_t) / (D_h · P_h)`.
pub fn duration_hedge_ratio(
    target_duration: f64,
    target_value: f64,
    hedge_duration: f64,
    hedge_price: f64,
) -> AnalyticsResult<f64> {
    if hedge_duration.abs() < 1e-10 || hedge_price.abs() < 1e-10 {
        return Err(AnalyticsError::CalculationFailed(
            "hedge duration or price is zero".to_string(),
        ));
    }
    let hedge_value = -(target_duration * target_value) / hedge_duration;
    Ok(hedge_value / hedge_price)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn dv01_ratio_short_to_neutralize() {
        let ratio = dv01_hedge_ratio(DV01::from(1000.0), DV01::from(50.0)).unwrap();
        assert_relative_eq!(ratio, -20.0, epsilon = 0.01);
    }

    #[test]
    fn duration_ratio_matches_hand_calc() {
        // -(5 × 10M) / 7.5 = -6.67M, /100 = -66,667 units.
        let units = duration_hedge_ratio(5.0, 10_000_000.0, 7.5, 100.0).unwrap();
        assert_relative_eq!(units, -66_666.67, epsilon = 1.0);
    }
}
