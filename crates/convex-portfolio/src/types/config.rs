//! Configuration for portfolio analytics computation.

use super::WeightingMethod;
use serde::{Deserialize, Serialize};

/// Configuration for portfolio analytics computation.
///
/// Controls parallelism, weighting, and other computation parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsConfig {
    /// Enable parallel processing (requires 'parallel' feature).
    pub parallel: bool,

    /// Minimum holdings count to trigger parallel processing.
    /// Below this threshold, sequential is faster due to thread overhead.
    pub parallel_threshold: usize,

    /// Weighting method for aggregations.
    pub weighting: WeightingMethod,

    /// Include holdings with missing analytics in aggregations.
    /// If false, holdings without the required metric are skipped.
    pub include_incomplete: bool,

    /// Key rate tenors to use for KRD aggregation.
    /// If None, uses the standard tenors from convex-analytics.
    pub key_rate_tenors: Option<Vec<f64>>,
}

impl Default for AnalyticsConfig {
    fn default() -> Self {
        Self {
            parallel: true,
            parallel_threshold: 100, // Use parallel if >100 holdings
            weighting: WeightingMethod::MarketValue,
            include_incomplete: true,
            key_rate_tenors: None,
        }
    }
}

impl AnalyticsConfig {
    /// Creates a new config with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a config that always uses sequential processing.
    #[must_use]
    pub fn sequential() -> Self {
        Self {
            parallel: false,
            ..Self::default()
        }
    }

    /// Sets whether to use parallel processing.
    #[must_use]
    pub fn with_parallel(mut self, enabled: bool) -> Self {
        self.parallel = enabled;
        self
    }

    /// Sets the threshold for parallel processing.
    #[must_use]
    pub fn with_threshold(mut self, threshold: usize) -> Self {
        self.parallel_threshold = threshold;
        self
    }

    /// Sets the weighting method.
    #[must_use]
    pub fn with_weighting(mut self, method: WeightingMethod) -> Self {
        self.weighting = method;
        self
    }

    /// Sets whether to include incomplete holdings.
    #[must_use]
    pub fn with_include_incomplete(mut self, include: bool) -> Self {
        self.include_incomplete = include;
        self
    }

    /// Sets the key rate tenors.
    #[must_use]
    pub fn with_key_rate_tenors(mut self, tenors: Vec<f64>) -> Self {
        self.key_rate_tenors = Some(tenors);
        self
    }

    /// Returns true if parallel processing should be used for the given count.
    #[must_use]
    pub fn should_parallelize(&self, count: usize) -> bool {
        cfg!(feature = "parallel") && self.parallel && count >= self.parallel_threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default() {
        let config = AnalyticsConfig::default();
        assert!(config.parallel);
        assert_eq!(config.parallel_threshold, 100);
        assert_eq!(config.weighting, WeightingMethod::MarketValue);
        assert!(config.include_incomplete);
        assert!(config.key_rate_tenors.is_none());
    }

    #[test]
    fn test_sequential() {
        let config = AnalyticsConfig::sequential();
        assert!(!config.parallel);
    }

    #[test]
    fn test_builder_pattern() {
        let config = AnalyticsConfig::new()
            .with_parallel(true)
            .with_threshold(50)
            .with_weighting(WeightingMethod::ParValue)
            .with_include_incomplete(false);

        assert!(config.parallel);
        assert_eq!(config.parallel_threshold, 50);
        assert_eq!(config.weighting, WeightingMethod::ParValue);
        assert!(!config.include_incomplete);
    }

    #[test]
    fn test_should_parallelize() {
        let config = AnalyticsConfig::new().with_threshold(100);

        // Without the 'parallel' feature, this always returns false
        // With the feature, it depends on the count
        #[cfg(feature = "parallel")]
        {
            assert!(!config.should_parallelize(50));
            assert!(config.should_parallelize(100));
            assert!(config.should_parallelize(500));
        }

        #[cfg(not(feature = "parallel"))]
        {
            assert!(!config.should_parallelize(50));
            assert!(!config.should_parallelize(100));
            assert!(!config.should_parallelize(500));
        }
    }

    #[test]
    fn test_serde() {
        let config = AnalyticsConfig::new()
            .with_threshold(75)
            .with_weighting(WeightingMethod::EqualWeight);

        let json = serde_json::to_string(&config).unwrap();
        let parsed: AnalyticsConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.parallel_threshold, 75);
        assert_eq!(parsed.weighting, WeightingMethod::EqualWeight);
    }
}
