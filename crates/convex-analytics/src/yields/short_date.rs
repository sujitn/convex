//! Thin native short-date delegation with the established analytics error boundary.
use crate::error::AnalyticsResult;
use convex_bonds::{pricing::ShortDateCalculator as Native, types::YieldCalculationRules};
use convex_core::types::Date;
use rust_decimal::Decimal;

/// Native numeric classification; actual market conventions are separate.
/// ```
/// use convex_analytics::yields::ShortDateCalculator;
/// let calculator=ShortDateCalculator::default();
/// assert!(calculator.use_money_market_below(0.08).unwrap());
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct ShortDateCalculator {
    inner: Native,
}
impl ShortDateCalculator {
    /// Construct the actual ordered threshold policy through its native owner.
    pub fn new(money: f64, short: f64) -> AnalyticsResult<Self> {
        Native::new(money, short)
            .map(|inner| Self { inner })
            .map_err(Into::into)
    }
    /// Retain the actual supplied threshold and enabled flag.
    pub fn from_rules(rules: &YieldCalculationRules) -> AnalyticsResult<Self> {
        Native::from_rules(rules)
            .map(|inner| Self { inner })
            .map_err(Into::into)
    }
    /// Explicit classification flag; this selects no reinvestment model.
    #[must_use]
    pub fn with_enabled(mut self, value: bool) -> Self {
        self.inner = self.inner.with_enabled(value);
        self
    }
    /// Delegate threshold validation without duplicate numeric rules.
    pub fn with_money_market_threshold(self, value: f64) -> AnalyticsResult<Self> {
        self.inner
            .with_money_market_threshold(value)
            .map(|inner| Self { inner })
            .map_err(Into::into)
    }
    /// Delegate threshold validation without duplicate numeric rules.
    pub fn with_short_date_threshold(self, value: f64) -> AnalyticsResult<Self> {
        self.inner
            .with_short_date_threshold(value)
            .map(|inner| Self { inner })
            .map_err(Into::into)
    }
    /// Actual original money-market threshold.
    pub fn money_market_threshold(&self) -> f64 {
        self.inner.money_market_threshold()
    }
    /// Actual original short-date threshold.
    pub fn short_date_threshold(&self) -> f64 {
        self.inner.short_date_threshold()
    }
    /// Actual supplied enabled flag.
    pub fn enabled(&self) -> bool {
        self.inner.enabled()
    }
    /// Native validated time classification.
    pub fn use_money_market_below(&self, years: f64) -> AnalyticsResult<bool> {
        self.inner.use_money_market_below(years).map_err(Into::into)
    }
    /// Native validated time classification.
    pub fn is_short_dated(&self, years: f64) -> AnalyticsResult<bool> {
        self.inner.is_short_dated(years).map_err(Into::into)
    }
    /// Preserve the established analytics bill error vocabulary and exact units.
    pub fn money_market_yield(
        &self,
        price: Decimal,
        face: Decimal,
        days: i64,
    ) -> AnalyticsResult<Decimal> {
        self.inner
            .money_market_yield(price, face, days)
            .map_err(super::money_market::bill_error)
    }
    /// Preserve the established analytics bill error vocabulary and exact units.
    pub fn bond_equivalent_yield(&self, rate: Decimal, days: i64) -> AnalyticsResult<Decimal> {
        self.inner
            .bond_equivalent_yield(rate, days)
            .map_err(super::money_market::bill_error)
    }
    /// Native checked remaining ACT/365F time.
    pub fn years_to_maturity(settlement: Date, maturity: Date) -> AnalyticsResult<f64> {
        Native::years_to_maturity(settlement, maturity).map_err(Into::into)
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
        assert!(calc.use_money_market_below(0.02).unwrap()); // ~1 week
        assert!(!calc.use_money_market_below(0.5).unwrap()); // 6 months
    }

    #[test]
    fn test_short_dated_threshold() {
        let calc = ShortDateCalculator::default();

        // < 1 year is short-dated
        assert!(calc.is_short_dated(0.5).unwrap());
        assert!(calc.is_short_dated(0.9).unwrap());
        assert!(!calc.is_short_dated(1.5).unwrap());
    }

    #[test]
    fn test_money_market_yield() {
        let calc = ShortDateCalculator::default();

        // 3-month T-bill: Price 99.5, Face 100, 90 days
        let mmy = calc.money_market_yield(dec!(99.5), dec!(100), 90).unwrap();

        // MMY = (100 - 99.5) / 99.5 * 360/90 = 0.5/99.5 * 4 = 2.01%
        assert!(mmy > dec!(0.019) && mmy < dec!(0.021));
    }

    #[test]
    fn test_bond_equivalent_yield() {
        let calc = ShortDateCalculator::default();

        let mmy = dec!(0.02); // 2% money market yield
        let bey = calc.bond_equivalent_yield(mmy, 90).unwrap();

        // BEY should be higher due to 365/360 adjustment
        assert!(bey > mmy);
    }

    #[test]
    fn test_years_to_maturity() {
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2025, 7, 15).unwrap();

        let ytm = ShortDateCalculator::years_to_maturity(settlement, maturity).unwrap();
        assert!(ytm > 0.49 && ytm < 0.51); // ~6 months
    }

    #[test]
    fn test_custom_thresholds() {
        let calc = ShortDateCalculator::default()
            .with_money_market_threshold(0.25)
            .unwrap()
            .with_short_date_threshold(2.0)
            .unwrap();

        assert!(calc.use_money_market_below(0.2).unwrap());
        assert!(calc.is_short_dated(1.5).unwrap());
    }

    #[test]
    fn test_from_rules() {
        let rules = YieldCalculationRules::us_treasury();
        let calc = ShortDateCalculator::from_rules(&rules).unwrap();

        assert_eq!(calc.short_date_threshold(), rules.short_date_threshold);
    }

    #[test]
    fn test_zero_price() {
        let calc = ShortDateCalculator::default();
        assert!(calc.money_market_yield(dec!(0), dec!(100), 90).is_err());
    }

    #[test]
    fn test_zero_days() {
        let calc = ShortDateCalculator::default();
        assert!(calc.money_market_yield(dec!(99.5), dec!(100), 0).is_err());
        assert!(calc.bond_equivalent_yield(dec!(0.02), 0).is_err());
    }
}
