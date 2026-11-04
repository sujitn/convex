//! Checked horizon classification and explicit fixed-basis bill measures.
//! No vendor roll-forward or overnight model is selected by this helper.
use crate::{types::YieldCalculationRules, BondError, BondResult};
use convex_core::{cashflows::MAX_CONTRACT_DAYS, types::Date};
use rust_decimal::Decimal;

/// Validated numeric thresholds. Pricing and market conventions remain separate.
/// ```
/// use convex_bonds::pricing::ShortDateCalculator;
/// let calculator=ShortDateCalculator::default();
/// assert!(calculator.use_money_market_below(0.08).unwrap());
/// ```
#[derive(Debug, Clone, Copy)]
pub struct ShortDateCalculator {
    money_market_threshold: f64,
    short_date_threshold: f64,
    enabled: bool,
}
fn valid(money: f64, short: f64) -> BondResult<()> {
    if !money.is_finite() || !short.is_finite() || money <= 0.0 || short < money {
        return Err(BondError::invalid_spec(
            "finite positive ordered short-date thresholds required",
        ));
    }
    Ok(())
}
fn years(value: f64) -> BondResult<()> {
    if !value.is_finite() || value <= 0.0 {
        return Err(BondError::invalid_spec(
            "positive finite remaining years required",
        ));
    }
    Ok(())
}
impl ShortDateCalculator {
    /// Construct actual ordered thresholds; this selects no pricing methodology.
    pub fn new(money_market_threshold: f64, short_date_threshold: f64) -> BondResult<Self> {
        valid(money_market_threshold, short_date_threshold)?;
        Ok(Self {
            money_market_threshold,
            short_date_threshold,
            enabled: true,
        })
    }
    /// Consume only the actual threshold and enabled flag from the supplied rules.
    pub fn from_rules(rules: &YieldCalculationRules) -> BondResult<Self> {
        Ok(Self::new(1.0 / 12.0, rules.short_date_threshold)?
            .with_enabled(rules.use_short_date_method))
    }
    /// Set whether classification can select either short-date regime.
    #[must_use]
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    /// Set a validated threshold without creating an invalid intermediate policy.
    pub fn with_money_market_threshold(mut self, value: f64) -> BondResult<Self> {
        valid(value, self.short_date_threshold)?;
        self.money_market_threshold = value;
        Ok(self)
    }
    /// Set a validated threshold without discarding the supplied enabled flag.
    pub fn with_short_date_threshold(mut self, value: f64) -> BondResult<Self> {
        valid(self.money_market_threshold, value)?;
        self.short_date_threshold = value;
        Ok(self)
    }
    /// Original configured money-market threshold, in years.
    pub const fn money_market_threshold(&self) -> f64 {
        self.money_market_threshold
    }
    /// Original configured short-date threshold, in years.
    pub const fn short_date_threshold(&self) -> f64 {
        self.short_date_threshold
    }
    /// Actual supplied classification flag.
    pub const fn enabled(&self) -> bool {
        self.enabled
    }
    /// Bounded-data classification only; this is not a quotation convention.
    pub fn use_money_market_below(&self, value: f64) -> BondResult<bool> {
        years(value)?;
        Ok(self.enabled && value <= self.money_market_threshold)
    }
    /// Bounded-data classification only, retaining the original strict threshold.
    pub fn is_short_dated(&self, value: f64) -> BondResult<bool> {
        years(value)?;
        Ok(self.enabled && value < self.short_date_threshold)
    }
    /// Explicit 360 price-basis bill investment rate, in annual decimal units.
    pub fn money_market_yield(
        &self,
        price: Decimal,
        face: Decimal,
        days: i64,
    ) -> BondResult<Decimal> {
        super::bill::money_market_yield(price, face, days)
    }
    /// Explicit short-term fixed365 simple bill rate; it is not a coupon-bond model.
    pub fn bond_equivalent_yield(&self, rate: Decimal, days: i64) -> BondResult<Decimal> {
        super::bill::simple_bill_yield_from_money_market(rate, days)
    }
    /// Checked positive remaining ACT/365F time from the two actual dates.
    pub fn years_to_maturity(settlement: Date, maturity: Date) -> BondResult<f64> {
        let days = settlement.days_between(&maturity);
        if !(1..=9999).contains(&settlement.year())
            || !(1..=9999).contains(&maturity.year())
            || days <= 0
            || days > MAX_CONTRACT_DAYS
        {
            return Err(BondError::invalid_spec(
                "invalid or excessive remaining maturity interval",
            ));
        }
        Ok(days as f64 / 365.0)
    }
}
impl Default for ShortDateCalculator {
    fn default() -> Self {
        Self {
            money_market_threshold: 1.0 / 12.0,
            short_date_threshold: 1.0,
            enabled: true,
        }
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
}
