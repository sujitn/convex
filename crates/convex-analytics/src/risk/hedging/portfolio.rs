//! Checked numeric aggregation in one caller-declared common currency/unit.
//! Instrument valuation, FX conversion and mixed-currency netting belong to
//! their supplied-term owners; this helper does not infer any of them.
use crate::error::{AnalyticsError, AnalyticsResult};
use crate::risk::duration::Duration;
use crate::risk::dv01::DV01;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Portfolio risk summary in the caller's common currency/unit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioRisk {
    /// Actual signed total market value.
    pub market_value: Decimal,
    /// Signed-value-weighted duration, unavailable when abs(net value)<=1e-10.
    pub weighted_duration: Option<Duration>,
    /// Actual signed total DV01.
    pub total_dv01: DV01,
    /// Number of supplied positions.
    pub position_count: usize,
}

/// Individual position in a caller-declared common currency/unit.
#[derive(Debug, Clone)]
pub struct Position {
    /// Exact position identifier, nonempty and unique within one aggregate.
    pub id: String,
    /// Actual signed market value.
    pub market_value: f64,
    /// Modified duration.
    pub duration: Duration,
    /// Actual signed position DV01.
    pub dv01: DV01,
}
impl Position {
    /// Hold supplied inputs; aggregate_portfolio_risk validates the complete set.
    pub fn new(id: impl Into<String>, market_value: f64, duration: Duration, dv01: DV01) -> Self {
        Self {
            id: id.into(),
            market_value,
            duration,
            dv01,
        }
    }
}

/// Aggregate at most65,536 supplied positions with checked Decimal arithmetic.
/// Empty exposure has actual zero value/DV01 and no defined weighted duration.
pub fn aggregate_portfolio_risk(positions: &[Position]) -> AnalyticsResult<PortfolioRisk> {
    let bad = || {
        AnalyticsError::InvalidInput(
            "bounded unique positions and representable common-unit risk required".into(),
        )
    };
    if positions.len() > 65_536 {
        return Err(bad());
    }
    let mut ids = HashSet::new();
    let (mut value, mut dv01, mut numerator) = (Decimal::ZERO, Decimal::ZERO, Decimal::ZERO);
    for position in positions {
        if position.id.trim().is_empty()
            || position.id.len() > 8192
            || !ids.insert(position.id.as_str())
        {
            return Err(bad());
        }
        let amount = crate::risk::cash::decimal(position.market_value)?;
        let term = amount
            .checked_mul(position.duration.years())
            .ok_or_else(bad)?;
        if !amount.is_zero() && !position.duration.years().is_zero() && term.is_zero() {
            return Err(bad());
        }
        value = value.checked_add(amount).ok_or_else(bad)?;
        dv01 = dv01.checked_add(position.dv01.value()).ok_or_else(bad)?;
        numerator = numerator.checked_add(term).ok_or_else(bad)?;
    }
    let weighted_duration = if value.as_f64().abs() > 1e-10 {
        let weighted = numerator.checked_div(value).ok_or_else(bad)?;
        if !numerator.is_zero() && weighted.is_zero() {
            return Err(bad());
        }
        Some(Duration::new(weighted))
    } else {
        None
    };
    Ok(PortfolioRisk {
        market_value: value,
        weighted_duration,
        total_dv01: DV01::new(dv01),
        position_count: positions.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_aggregate_portfolio_risk() {
        let positions = vec![
            Position::new(
                "Bond1",
                1_000_000.0,
                Duration::try_from(5.0).unwrap(),
                DV01::try_from(500.0).unwrap(),
            ),
            Position::new(
                "Bond2",
                2_000_000.0,
                Duration::try_from(3.0).unwrap(),
                DV01::try_from(600.0).unwrap(),
            ),
        ];

        let risk = aggregate_portfolio_risk(&positions).unwrap();

        assert_relative_eq!(
            risk.market_value.to_string().parse::<f64>().unwrap(),
            3_000_000.0,
            epsilon = 1.0
        );
        assert_relative_eq!(risk.total_dv01.as_f64(), 1100.0, epsilon = 0.1);

        // Weighted duration = (5 × 1M + 3 × 2M) / 3M = 11/3 ≈ 3.67
        assert_relative_eq!(
            risk.weighted_duration.unwrap().as_f64(),
            3.67,
            epsilon = 0.01
        );
        assert_eq!(risk.position_count, 2);
    }
}
