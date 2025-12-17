//! Key rate duration calculations.
//!
//! Key rate durations measure sensitivity to specific points on the yield curve,
//! allowing for more granular risk management than parallel shift measures.

use super::Duration;
use crate::error::{AnalyticsError, AnalyticsResult};
use serde::{Deserialize, Serialize};

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
    pub fn total_duration(&self) -> Duration {
        let total: f64 = self.durations.iter().map(|krd| krd.duration.as_f64()).sum();
        Duration::from(total)
    }

    /// Get duration at a specific tenor
    pub fn at_tenor(&self, tenor: f64) -> Option<&KeyRateDuration> {
        self.durations
            .iter()
            .find(|krd| (krd.tenor - tenor).abs() < 0.001)
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

    let krd = (price_down - price_up) / (2.0 * price_base * bump_size);

    Ok(KeyRateDuration {
        tenor,
        duration: Duration::from(krd),
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
                duration: Duration::from(1.5),
            },
            KeyRateDuration {
                tenor: 5.0,
                duration: Duration::from(2.0),
            },
            KeyRateDuration {
                tenor: 10.0,
                duration: Duration::from(1.5),
            },
        ]);

        let total = krds.total_duration();
        assert!((total.as_f64() - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_key_rate_at_tenor() {
        let krds = KeyRateDurations::new(vec![
            KeyRateDuration {
                tenor: 2.0,
                duration: Duration::from(1.5),
            },
            KeyRateDuration {
                tenor: 5.0,
                duration: Duration::from(2.0),
            },
        ]);

        let krd_5y = krds.at_tenor(5.0).unwrap();
        assert!((krd_5y.duration.as_f64() - 2.0).abs() < 0.001);
    }
}
