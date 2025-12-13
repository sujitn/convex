//! Configuration for yield calculations.
//!
//! This module provides [`YieldCalculatorConfig`] for configuring yield
//! calculation behavior, including method selection, solver parameters,
//! and money market threshold settings.

use convex_core::types::YieldMethod;

// Re-export solver constants from convex-math for consistency
pub use convex_math::solvers::{DEFAULT_MAX_ITERATIONS, DEFAULT_TOLERANCE};

/// Money market threshold for US markets (182 days).
///
/// For bonds settling within 182 days of maturity, use money market yield.
pub const MM_THRESHOLD_US: u32 = 182;

/// Money market threshold for Canadian markets (365 days).
///
/// Canadian conventions use a longer threshold period.
pub const MM_THRESHOLD_CAD: u32 = 365;

/// Configuration for yield calculations.
///
/// This struct encapsulates all parameters needed for yield calculations:
/// - The calculation method (Compounded, Simple, Discount, AddOn)
/// - Money market threshold for automatic method switching
/// - Solver parameters for Newton-Raphson iteration
///
/// # Example
///
/// ```rust
/// use convex_yas::yields::YieldCalculatorConfig;
/// use convex_core::types::YieldMethod;
///
/// // US Treasury configuration
/// let config = YieldCalculatorConfig::builder()
///     .method(YieldMethod::Compounded)
///     .money_market_threshold(182)
///     .build();
///
/// // Check if money market method should be used
/// assert!(config.should_use_money_market(90));
/// assert!(!config.should_use_money_market(200));
/// ```
#[derive(Debug, Clone)]
pub struct YieldCalculatorConfig {
    /// The yield calculation method to use.
    method: YieldMethod,

    /// Days-to-maturity threshold for switching to money market yield.
    ///
    /// If set, bonds with fewer days to maturity than this threshold
    /// will use AddOn (money market) yield instead of the configured method.
    money_market_threshold: Option<u32>,

    /// Solver tolerance for Newton-Raphson iteration.
    tolerance: f64,

    /// Maximum number of Newton-Raphson iterations.
    max_iterations: u32,
}

impl YieldCalculatorConfig {
    /// Creates a new builder for `YieldCalculatorConfig`.
    #[must_use]
    pub fn builder() -> YieldCalculatorConfigBuilder {
        YieldCalculatorConfigBuilder::default()
    }

    /// Returns the configured yield method.
    #[must_use]
    pub const fn method(&self) -> YieldMethod {
        self.method
    }

    /// Returns the money market threshold in days.
    #[must_use]
    pub const fn money_market_threshold(&self) -> Option<u32> {
        self.money_market_threshold
    }

    /// Returns the solver tolerance.
    #[must_use]
    pub const fn tolerance(&self) -> f64 {
        self.tolerance
    }

    /// Returns the maximum number of iterations.
    #[must_use]
    pub const fn max_iterations(&self) -> u32 {
        self.max_iterations
    }

    /// Determines if money market yield should be used based on days to maturity.
    ///
    /// Returns `true` if:
    /// - A threshold is configured AND
    /// - The days to maturity is less than or equal to the threshold
    #[must_use]
    pub fn should_use_money_market(&self, days_to_maturity: u32) -> bool {
        self.money_market_threshold
            .map_or(false, |threshold| days_to_maturity <= threshold)
    }

    /// Returns the effective yield method based on days to maturity.
    ///
    /// If the bond is within the money market threshold, returns `AddOn`.
    /// Otherwise, returns the configured method.
    #[must_use]
    pub fn effective_method(&self, days_to_maturity: u32) -> YieldMethod {
        if self.should_use_money_market(days_to_maturity) {
            YieldMethod::AddOn
        } else {
            self.method
        }
    }

    /// Creates a configuration for US Treasury bonds.
    ///
    /// - Method: Compounded
    /// - Money market threshold: 182 days
    #[must_use]
    pub fn us_treasury() -> Self {
        Self::builder()
            .method(YieldMethod::Compounded)
            .money_market_threshold(MM_THRESHOLD_US)
            .build()
    }

    /// Creates a configuration for US Corporate bonds.
    ///
    /// - Method: Compounded
    /// - Money market threshold: 182 days
    #[must_use]
    pub fn us_corporate() -> Self {
        Self::builder()
            .method(YieldMethod::Compounded)
            .money_market_threshold(MM_THRESHOLD_US)
            .build()
    }

    /// Creates a configuration for European government bonds (ICMA).
    ///
    /// - Method: Compounded (ICMA convention)
    /// - No money market threshold
    #[must_use]
    pub fn european_govt() -> Self {
        Self::builder()
            .method(YieldMethod::Compounded)
            .build()
    }

    /// Creates a configuration for Japanese Government Bonds.
    ///
    /// - Method: Simple (JGB convention)
    /// - No money market threshold
    #[must_use]
    pub fn japanese_jgb() -> Self {
        Self::builder()
            .method(YieldMethod::Simple)
            .build()
    }

    /// Creates a configuration for T-Bills.
    ///
    /// - Method: Discount
    /// - No money market threshold (always discount)
    #[must_use]
    pub fn t_bill() -> Self {
        Self::builder()
            .method(YieldMethod::Discount)
            .build()
    }

    /// Creates a configuration for Canadian Government bonds.
    ///
    /// - Method: Compounded
    /// - Money market threshold: 365 days
    #[must_use]
    pub fn canadian_govt() -> Self {
        Self::builder()
            .method(YieldMethod::Compounded)
            .money_market_threshold(MM_THRESHOLD_CAD)
            .build()
    }
}

impl Default for YieldCalculatorConfig {
    fn default() -> Self {
        Self {
            method: YieldMethod::Compounded,
            money_market_threshold: None,
            tolerance: DEFAULT_TOLERANCE,
            max_iterations: DEFAULT_MAX_ITERATIONS,
        }
    }
}

/// Builder for `YieldCalculatorConfig`.
#[derive(Debug, Clone, Default)]
pub struct YieldCalculatorConfigBuilder {
    method: Option<YieldMethod>,
    money_market_threshold: Option<u32>,
    tolerance: Option<f64>,
    max_iterations: Option<u32>,
}

impl YieldCalculatorConfigBuilder {
    /// Sets the yield calculation method.
    #[must_use]
    pub fn method(mut self, method: YieldMethod) -> Self {
        self.method = Some(method);
        self
    }

    /// Sets the money market threshold in days.
    #[must_use]
    pub fn money_market_threshold(mut self, days: u32) -> Self {
        self.money_market_threshold = Some(days);
        self
    }

    /// Sets the solver tolerance.
    #[must_use]
    pub fn tolerance(mut self, tolerance: f64) -> Self {
        self.tolerance = Some(tolerance);
        self
    }

    /// Sets the maximum number of iterations.
    #[must_use]
    pub fn max_iterations(mut self, max: u32) -> Self {
        self.max_iterations = Some(max);
        self
    }

    /// Builds the `YieldCalculatorConfig`.
    #[must_use]
    pub fn build(self) -> YieldCalculatorConfig {
        let default = YieldCalculatorConfig::default();

        YieldCalculatorConfig {
            method: self.method.unwrap_or(default.method),
            money_market_threshold: self.money_market_threshold,
            tolerance: self.tolerance.unwrap_or(default.tolerance),
            max_iterations: self.max_iterations.unwrap_or(default.max_iterations),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = YieldCalculatorConfig::default();
        assert_eq!(config.method(), YieldMethod::Compounded);
        assert_eq!(config.money_market_threshold(), None);
        assert!((config.tolerance() - 1e-10).abs() < 1e-15);
        assert_eq!(config.max_iterations(), 100);
    }

    #[test]
    fn test_builder() {
        let config = YieldCalculatorConfig::builder()
            .method(YieldMethod::Simple)
            .money_market_threshold(182)
            .tolerance(1e-8)
            .max_iterations(50)
            .build();

        assert_eq!(config.method(), YieldMethod::Simple);
        assert_eq!(config.money_market_threshold(), Some(182));
        assert!((config.tolerance() - 1e-8).abs() < 1e-15);
        assert_eq!(config.max_iterations(), 50);
    }

    #[test]
    fn test_should_use_money_market() {
        let config = YieldCalculatorConfig::builder()
            .money_market_threshold(182)
            .build();

        // Below threshold
        assert!(config.should_use_money_market(90));
        assert!(config.should_use_money_market(182));

        // Above threshold
        assert!(!config.should_use_money_market(183));
        assert!(!config.should_use_money_market(365));
    }

    #[test]
    fn test_should_use_money_market_no_threshold() {
        let config = YieldCalculatorConfig::default();

        // Without threshold, never use money market
        assert!(!config.should_use_money_market(30));
        assert!(!config.should_use_money_market(90));
        assert!(!config.should_use_money_market(365));
    }

    #[test]
    fn test_effective_method() {
        let config = YieldCalculatorConfig::builder()
            .method(YieldMethod::Compounded)
            .money_market_threshold(182)
            .build();

        // Below threshold - use AddOn
        assert_eq!(config.effective_method(90), YieldMethod::AddOn);

        // Above threshold - use configured method
        assert_eq!(config.effective_method(200), YieldMethod::Compounded);
    }

    #[test]
    fn test_us_treasury_preset() {
        let config = YieldCalculatorConfig::us_treasury();
        assert_eq!(config.method(), YieldMethod::Compounded);
        assert_eq!(config.money_market_threshold(), Some(182));
    }

    #[test]
    fn test_japanese_jgb_preset() {
        let config = YieldCalculatorConfig::japanese_jgb();
        assert_eq!(config.method(), YieldMethod::Simple);
        assert_eq!(config.money_market_threshold(), None);
    }

    #[test]
    fn test_t_bill_preset() {
        let config = YieldCalculatorConfig::t_bill();
        assert_eq!(config.method(), YieldMethod::Discount);
        assert_eq!(config.money_market_threshold(), None);
    }

    #[test]
    fn test_canadian_govt_preset() {
        let config = YieldCalculatorConfig::canadian_govt();
        assert_eq!(config.method(), YieldMethod::Compounded);
        assert_eq!(config.money_market_threshold(), Some(365));
    }
}
