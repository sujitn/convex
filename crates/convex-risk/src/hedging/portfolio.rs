//! Portfolio-level risk aggregation.

use crate::duration::Duration;
use crate::dv01::DV01;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Portfolio risk summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioRisk {
    /// Total market value
    pub market_value: Decimal,
    /// Weighted average duration
    pub weighted_duration: Duration,
    /// Total DV01
    pub total_dv01: DV01,
    /// Number of positions
    pub position_count: usize,
}

/// Individual position for portfolio aggregation
#[derive(Debug, Clone)]
pub struct Position {
    /// Position identifier
    pub id: String,
    /// Market value
    pub market_value: f64,
    /// Modified duration
    pub duration: Duration,
    /// DV01
    pub dv01: DV01,
}

impl Position {
    /// Create a new position
    pub fn new(id: impl Into<String>, market_value: f64, duration: Duration, dv01: DV01) -> Self {
        Self {
            id: id.into(),
            market_value,
            duration,
            dv01,
        }
    }
}

/// Calculate aggregate portfolio risk.
///
/// # Arguments
///
/// * `positions` - Vector of portfolio positions
///
/// # Returns
///
/// Aggregated portfolio risk metrics
pub fn aggregate_portfolio_risk(positions: &[Position]) -> PortfolioRisk {
    let total_value: f64 = positions.iter().map(|p| p.market_value).sum();
    let total_dv01: f64 = positions.iter().map(|p| p.dv01.as_f64()).sum();

    let weighted_duration = if total_value.abs() > 1e-10 {
        positions
            .iter()
            .map(|p| p.duration.as_f64() * p.market_value)
            .sum::<f64>()
            / total_value
    } else {
        0.0
    };

    PortfolioRisk {
        market_value: Decimal::from_f64_retain(total_value).unwrap_or(Decimal::ZERO),
        weighted_duration: Duration::from(weighted_duration),
        total_dv01: DV01::from(total_dv01),
        position_count: positions.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_aggregate_portfolio_risk() {
        let positions = vec![
            Position::new("Bond1", 1_000_000.0, Duration::from(5.0), DV01::from(500.0)),
            Position::new("Bond2", 2_000_000.0, Duration::from(3.0), DV01::from(600.0)),
        ];

        let risk = aggregate_portfolio_risk(&positions);

        assert_relative_eq!(
            risk.market_value.to_string().parse::<f64>().unwrap(),
            3_000_000.0,
            epsilon = 1.0
        );
        assert_relative_eq!(risk.total_dv01.as_f64(), 1100.0, epsilon = 0.1);

        // Weighted duration = (5 × 1M + 3 × 2M) / 3M = 11/3 ≈ 3.67
        assert_relative_eq!(risk.weighted_duration.as_f64(), 3.67, epsilon = 0.01);
        assert_eq!(risk.position_count, 2);
    }
}
