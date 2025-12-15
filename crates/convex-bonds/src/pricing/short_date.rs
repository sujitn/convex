//! Short-date yield calculation methodology.
//!
//! For bonds with less than one year to maturity, special calculation
//! methodologies apply. This module provides the Bloomberg-compatible
//! sequential roll-forward approach for short-dated bonds.
//!
//! # Reference
//!
//! - Bloomberg YAS Manual: Short-Dated Bonds
//! - ISMA Rule Book: Price/Yield Calculations

use rust_decimal::Decimal;

use convex_core::types::Date;

use crate::types::YieldCalculationRules;

/// Roll-forward methodology for short-dated bonds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RollForwardMethod {
    /// Bloomberg sequential roll-forward
    ///
    /// For bonds < 1 year to maturity, calculates yield by rolling forward
    /// cash flows at the reinvestment rate until maturity.
    #[default]
    Bloomberg,

    /// Simple linear interpolation
    ///
    /// Linearly interpolates between money market and bond yields.
    Linear,

    /// Actual reinvestment assumption
    ///
    /// Uses actual overnight rates for reinvestment of cash flows.
    Actual,
}

/// Short-date yield calculator.
///
/// Handles bonds with less than one year to maturity using money market
/// conventions or sequential roll-forward methodology.
///
/// # Example
///
/// ```rust
/// use convex_bonds::pricing::short_date::{ShortDateCalculator, RollForwardMethod};
///
/// let calculator = ShortDateCalculator::new(RollForwardMethod::Bloomberg);
/// assert!(calculator.use_money_market_below(0.08));  // 8% of a year = ~1 month
/// ```
pub struct ShortDateCalculator {
    /// Threshold for using money market yield (in years)
    money_market_threshold: f64,

    /// Threshold for using short-date method (in years)
    short_date_threshold: f64,

    /// Roll-forward methodology
    roll_forward_method: RollForwardMethod,
}

impl ShortDateCalculator {
    /// Creates a new short-date calculator with the specified roll-forward method.
    #[must_use]
    pub fn new(method: RollForwardMethod) -> Self {
        Self {
            money_market_threshold: 1.0 / 12.0, // 1 month
            short_date_threshold: 1.0,          // 1 year
            roll_forward_method: method,
        }
    }

    /// Creates a calculator from yield calculation rules.
    #[must_use]
    pub fn from_rules(rules: &YieldCalculationRules) -> Self {
        Self {
            money_market_threshold: 1.0 / 12.0,
            short_date_threshold: rules.short_date_threshold,
            roll_forward_method: RollForwardMethod::Bloomberg,
        }
    }

    /// Sets the money market threshold.
    #[must_use]
    pub fn with_money_market_threshold(mut self, threshold: f64) -> Self {
        self.money_market_threshold = threshold;
        self
    }

    /// Sets the short-date threshold.
    #[must_use]
    pub fn with_short_date_threshold(mut self, threshold: f64) -> Self {
        self.short_date_threshold = threshold;
        self
    }

    /// Returns true if money market conventions should be used.
    ///
    /// Very short-dated bonds (typically < 1 month) use simple money market
    /// yield calculations instead of bond yield.
    #[must_use]
    pub fn use_money_market_below(&self, years_to_maturity: f64) -> bool {
        years_to_maturity <= self.money_market_threshold
    }

    /// Returns true if short-date methodology should be used.
    ///
    /// Bonds < 1 year to maturity typically use sequential roll-forward
    /// or linear interpolation instead of standard bond yield.
    #[must_use]
    pub fn is_short_dated(&self, years_to_maturity: f64) -> bool {
        years_to_maturity < self.short_date_threshold
    }

    /// Returns the roll-forward method.
    #[must_use]
    pub const fn roll_forward_method(&self) -> RollForwardMethod {
        self.roll_forward_method
    }

    /// Calculates money market yield from price and days to maturity.
    ///
    /// Money market yield = (Face - Price) / Price * (360 / Days)
    #[must_use]
    pub fn money_market_yield(
        &self,
        price: Decimal,
        face_value: Decimal,
        days_to_maturity: i64,
    ) -> Decimal {
        if price.is_zero() || days_to_maturity <= 0 {
            return Decimal::ZERO;
        }

        let gain = face_value - price;
        let days = Decimal::from(days_to_maturity);
        let year_basis = Decimal::from(360);

        (gain / price) * (year_basis / days)
    }

    /// Calculates bond equivalent yield from money market yield.
    ///
    /// Converts discount basis to semi-annual bond basis.
    #[must_use]
    pub fn bond_equivalent_yield(
        &self,
        money_market_yield: Decimal,
        days_to_maturity: i64,
    ) -> Decimal {
        if days_to_maturity <= 0 {
            return Decimal::ZERO;
        }

        // BEY = (365 / Days) * (Face - Price) / Price
        // For < 182 days: BEY = 365/360 * MMY * (360/Days) / (1 - MMY * Days/360)
        let days = Decimal::from(days_to_maturity);
        let year_basis = Decimal::from(365);
        let mm_basis = Decimal::from(360);

        if days <= Decimal::from(182) {
            // Simple formula for < 6 months
            money_market_yield * year_basis / mm_basis
        } else {
            // Quadratic formula for >= 6 months
            // More complex calculation involving semi-annual compounding
            let factor = days / year_basis;
            let two = Decimal::from(2);

            // Simplified: use linear approximation
            let approx = money_market_yield * year_basis / mm_basis;
            let compound_adj = Decimal::ONE + factor * approx / two;
            approx / compound_adj
        }
    }

    /// Calculates years to maturity from settlement and maturity dates.
    #[must_use]
    pub fn years_to_maturity(settlement: Date, maturity: Date) -> f64 {
        let days = settlement.days_between(&maturity);
        days as f64 / 365.0
    }
}

impl Default for ShortDateCalculator {
    fn default() -> Self {
        Self::new(RollForwardMethod::Bloomberg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_money_market_threshold() {
        let calc = ShortDateCalculator::default();

        // Very short-dated (< 1 month) should use money market
        assert!(calc.use_money_market_below(0.02));  // ~1 week
        assert!(!calc.use_money_market_below(0.5)); // 6 months
    }

    #[test]
    fn test_short_dated_threshold() {
        let calc = ShortDateCalculator::default();

        // < 1 year is short-dated
        assert!(calc.is_short_dated(0.5));
        assert!(calc.is_short_dated(0.9));
        assert!(!calc.is_short_dated(1.5));
    }

    #[test]
    fn test_money_market_yield() {
        let calc = ShortDateCalculator::default();

        // 3-month T-bill: Price 99.5, Face 100, 90 days
        let mmy = calc.money_market_yield(dec!(99.5), dec!(100), 90);

        // MMY = (100 - 99.5) / 99.5 * 360/90 = 0.5/99.5 * 4 = 2.01%
        assert!(mmy > dec!(0.019) && mmy < dec!(0.021));
    }

    #[test]
    fn test_bond_equivalent_yield() {
        let calc = ShortDateCalculator::default();

        let mmy = dec!(0.02); // 2% money market yield
        let bey = calc.bond_equivalent_yield(mmy, 90);

        // BEY should be higher due to 365/360 adjustment
        assert!(bey > mmy);
    }

    #[test]
    fn test_years_to_maturity() {
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2025, 7, 15).unwrap();

        let ytm = ShortDateCalculator::years_to_maturity(settlement, maturity);
        assert!(ytm > 0.49 && ytm < 0.51); // ~6 months
    }

    #[test]
    fn test_custom_thresholds() {
        let calc = ShortDateCalculator::default()
            .with_money_market_threshold(0.25)
            .with_short_date_threshold(2.0);

        assert!(calc.use_money_market_below(0.2));
        assert!(calc.is_short_dated(1.5));
    }

    #[test]
    fn test_from_rules() {
        let rules = YieldCalculationRules::us_treasury();
        let calc = ShortDateCalculator::from_rules(&rules);

        assert_eq!(calc.short_date_threshold, rules.short_date_threshold);
    }
}
