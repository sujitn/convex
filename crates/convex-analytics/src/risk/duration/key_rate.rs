//! Key rate duration calculations.
//!
//! Key rate durations measure sensitivity to specific points on the yield curve,
//! allowing for more granular risk management than parallel shift measures.

use super::Duration;
use crate::error::{AnalyticsError, AnalyticsResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Standard key rate tenors
pub const STANDARD_KEY_RATE_TENORS: &[f64] = &[
    0.25, // 3 months
    0.5,  // 6 months
    1.0,  // 1 year
    2.0,  // 2 years
    3.0,  // 3 years
    5.0,  // 5 years
    7.0,  // 7 years
    10.0, // 10 years
    20.0, // 20 years
    30.0, // 30 years
];

/// Key rate duration for a specific tenor point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRateDuration {
    /// The tenor point (in years)
    pub tenor: f64,
    /// The duration at this tenor
    pub duration: Duration,
}

/// Collection of key rate durations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRateDurations {
    /// Individual key rate durations
    pub durations: Vec<KeyRateDuration>,
}

impl KeyRateDurations {
    /// Create new key rate durations
    pub fn new(durations: Vec<KeyRateDuration>) -> Self {
        Self { durations }
    }

    /// Get the total parallel duration (sum of all key rate durations)
    pub fn total_duration(&self) -> AnalyticsResult<Duration> {
        if self.durations.len() > 4096 {
            return Err(AnalyticsError::InvalidInput(
                "at most4096key rates required".into(),
            ));
        }
        if !self.durations.is_empty() {
            Self::validate_tenors(
                &self
                    .durations
                    .iter()
                    .map(|row| row.tenor)
                    .collect::<Vec<_>>(),
            )?;
        }
        let total = self.durations.iter().try_fold(Decimal::ZERO, |sum, row| {
            sum.checked_add(row.duration.years())
                .ok_or_else(|| AnalyticsError::CalculationFailed("key rate sum overflow".into()))
        })?;
        Ok(Duration::new(total))
    }

    pub(crate) fn validate_tenors(tenors: &[f64]) -> AnalyticsResult<()> {
        if tenors.is_empty() || tenors.len() > 4096 {
            return Err(AnalyticsError::InvalidInput(
                "1..4096key rate tenors required".into(),
            ));
        }
        let mut seen = HashSet::with_capacity(tenors.len());
        for &tenor in tenors {
            if !tenor.is_finite() || tenor <= 0.0 || !seen.insert(tenor.to_bits()) {
                return Err(AnalyticsError::InvalidInput(
                    "finite positive unique key rate tenors required".into(),
                ));
            }
        }
        Ok(())
    }

    /// Get one exact finite positive tenor. Invalid or ambiguous collections
    /// have no admitted match; nearby supplied factors are never substituted.
    pub fn at_tenor(&self, tenor: f64) -> Option<&KeyRateDuration> {
        if !tenor.is_finite() || tenor <= 0.0 || self.durations.len() > 4096 {
            return None;
        }
        let mut seen = HashSet::with_capacity(self.durations.len());
        if self.durations.iter().any(|row| {
            !row.tenor.is_finite() || row.tenor <= 0.0 || !seen.insert(row.tenor.to_bits())
        }) {
            return None;
        }
        self.durations
            .iter()
            .find(|row| row.tenor.to_bits() == tenor.to_bits())
    }
}

/// Calculate key rate duration at a specific tenor.
///
/// # Arguments
///
/// * `price_up` - Price when rate at tenor increases
/// * `price_down` - Price when rate at tenor decreases
/// * `price_base` - Base price
/// * `bump_size` - Size of rate bump
/// * `tenor` - The tenor point
///
/// # Returns
///
/// Key rate duration at the specified tenor
pub fn key_rate_duration_at_tenor(
    price_up: f64,
    price_down: f64,
    price_base: f64,
    bump_size: f64,
    tenor: f64,
) -> AnalyticsResult<KeyRateDuration> {
    if price_base.abs() < 1e-10 {
        return Err(AnalyticsError::CalculationFailed(format!(
            "base price is zero at tenor {}",
            tenor
        )));
    }

    if !tenor.is_finite() || tenor <= 0.0 {
        return Err(AnalyticsError::InvalidInput(
            "positive finite key rate tenor required".into(),
        ));
    }
    let value = convex_bonds::pricing::risk_math::duration_stencil(
        price_up, price_down, price_base, bump_size,
    )?;
    Ok(KeyRateDuration {
        tenor,
        duration: Duration::new(crate::risk::cash::decimal(value)?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_rate_duration_collection() {
        let krds = KeyRateDurations::new(vec![
            KeyRateDuration {
                tenor: 2.0,
                duration: Duration::try_from(1.5).unwrap(),
            },
            KeyRateDuration {
                tenor: 5.0,
                duration: Duration::try_from(2.0).unwrap(),
            },
            KeyRateDuration {
                tenor: 10.0,
                duration: Duration::try_from(1.5).unwrap(),
            },
        ]);

        let total = krds.total_duration().unwrap();
        assert!((total.as_f64() - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_key_rate_at_tenor() {
        let krds = KeyRateDurations::new(vec![
            KeyRateDuration {
                tenor: 2.0,
                duration: Duration::try_from(1.5).unwrap(),
            },
            KeyRateDuration {
                tenor: 5.0,
                duration: Duration::try_from(2.0).unwrap(),
            },
        ]);

        let krd_5y = krds.at_tenor(5.0).unwrap();
        assert!((krd_5y.duration.as_f64() - 2.0).abs() < 0.001);
    }
}
