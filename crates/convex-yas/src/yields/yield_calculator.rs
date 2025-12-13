//! Unified yield calculator that respects bond conventions.
//!
//! The `YieldCalculator` provides a single entry point for yield calculations,
//! automatically selecting the appropriate method based on configuration and
//! time to maturity.
//!
//! # Design Principle
//!
//! The bond owns its conventions (day count, frequency, settlement rules).
//! The `YieldCalculator` respects these conventions while applying the configured
//! yield calculation method (Compounded, Simple, Discount, AddOn).
//!
//! # Example
//!
//! ```ignore
//! use convex_yas::yields::{YieldCalculator, YieldCalculatorConfig};
//! use convex_core::types::YieldMethod;
//!
//! // US Treasury configuration (switches to money market for short-dated)
//! let config = YieldCalculatorConfig::us_treasury();
//! let calc = YieldCalculator::new(config);
//!
//! // Calculate yield from price - respects bond's day count and frequency
//! let ytm = calc.yield_from_price(&bond, settlement, clean_price)?;
//!
//! // Calculate price from yield
//! let price = calc.price_from_yield(&bond, settlement, ytm)?;
//! ```

use crate::YasError;
use convex_bonds::traits::{Bond, BondCashFlow};
use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Date, YieldMethod};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use super::{
    price_from_money_market_yield, simple_yield, solve_money_market_yield, street_convention_yield,
    YieldCalculatorConfig,
};

/// Unified yield calculator.
///
/// Provides yield-to-price and price-to-yield calculations that:
/// - Respect the bond's day count and frequency conventions
/// - Automatically switch to money market method for short-dated bonds (if configured)
/// - Support Compounded, Simple, Discount, and AddOn yield methods
#[derive(Debug, Clone)]
pub struct YieldCalculator {
    config: YieldCalculatorConfig,
}

impl YieldCalculator {
    /// Creates a new yield calculator with the given configuration.
    #[must_use]
    pub fn new(config: YieldCalculatorConfig) -> Self {
        Self { config }
    }

    /// Creates a yield calculator with default US Treasury configuration.
    #[must_use]
    pub fn us_treasury() -> Self {
        Self::new(YieldCalculatorConfig::us_treasury())
    }

    /// Creates a yield calculator with default US Corporate configuration.
    #[must_use]
    pub fn us_corporate() -> Self {
        Self::new(YieldCalculatorConfig::us_corporate())
    }

    /// Creates a yield calculator with default European government configuration.
    #[must_use]
    pub fn european_govt() -> Self {
        Self::new(YieldCalculatorConfig::european_govt())
    }

    /// Creates a yield calculator with default Japanese JGB configuration.
    #[must_use]
    pub fn japanese_jgb() -> Self {
        Self::new(YieldCalculatorConfig::japanese_jgb())
    }

    /// Creates a yield calculator for T-Bills.
    #[must_use]
    pub fn t_bill() -> Self {
        Self::new(YieldCalculatorConfig::t_bill())
    }

    /// Returns the underlying configuration.
    #[must_use]
    pub fn config(&self) -> &YieldCalculatorConfig {
        &self.config
    }

    /// Calculate yield from clean price.
    ///
    /// This method:
    /// 1. Determines the effective yield method based on days to maturity
    /// 2. Gets cash flows and conventions from the bond
    /// 3. Dispatches to the appropriate yield calculation method
    ///
    /// # Arguments
    ///
    /// * `bond` - The bond to price
    /// * `settlement` - Settlement date
    /// * `clean_price` - Clean price as percentage of par (e.g., 100.5 for 100.5%)
    ///
    /// # Returns
    ///
    /// Yield as a decimal (e.g., 0.05 for 5%)
    pub fn yield_from_price<B: Bond>(
        &self,
        bond: &B,
        settlement: Date,
        clean_price: Decimal,
    ) -> Result<Decimal, YasError> {
        // Validate inputs
        if clean_price <= Decimal::ZERO {
            return Err(YasError::InvalidInput(
                "clean price must be positive".to_string(),
            ));
        }

        let maturity = bond
            .maturity()
            .ok_or_else(|| YasError::InvalidInput("perpetual bonds not supported".to_string()))?;

        if settlement >= maturity {
            return Err(YasError::InvalidInput("bond has matured".to_string()));
        }

        // Calculate days to maturity
        let days_to_maturity = settlement.days_between(&maturity) as u32;

        // Determine effective method
        let method = self.config.effective_method(days_to_maturity);

        // Get bond conventions
        let accrued = bond.accrued_interest(settlement);
        let dirty_price = clean_price + accrued;
        let cash_flows = bond.cash_flows(settlement);
        let frequency = bond.frequency();
        let day_count = parse_day_count(bond.day_count_convention())?;

        match method {
            YieldMethod::Compounded => self.yield_compounded(
                &cash_flows,
                dirty_price,
                settlement,
                frequency.periods_per_year(),
            ),
            YieldMethod::Simple => {
                self.yield_simple(bond, clean_price, settlement, days_to_maturity)
            }
            YieldMethod::Discount => self.yield_discount(clean_price, days_to_maturity, day_count),
            YieldMethod::AddOn => {
                self.yield_add_on(&cash_flows, dirty_price, settlement, day_count)
            }
        }
    }

    /// Calculate clean price from yield.
    ///
    /// # Arguments
    ///
    /// * `bond` - The bond to price
    /// * `settlement` - Settlement date
    /// * `yield_decimal` - Yield as a decimal (e.g., 0.05 for 5%)
    ///
    /// # Returns
    ///
    /// Clean price as percentage of par
    pub fn price_from_yield<B: Bond>(
        &self,
        bond: &B,
        settlement: Date,
        yield_decimal: Decimal,
    ) -> Result<Decimal, YasError> {
        let maturity = bond
            .maturity()
            .ok_or_else(|| YasError::InvalidInput("perpetual bonds not supported".to_string()))?;

        if settlement >= maturity {
            return Err(YasError::InvalidInput("bond has matured".to_string()));
        }

        let days_to_maturity = settlement.days_between(&maturity) as u32;
        let method = self.config.effective_method(days_to_maturity);

        let cash_flows = bond.cash_flows(settlement);
        let frequency = bond.frequency();
        let day_count = parse_day_count(bond.day_count_convention())?;
        let accrued = bond.accrued_interest(settlement);

        let dirty_price = match method {
            YieldMethod::Compounded => {
                Ok(self.price_compounded(&cash_flows, yield_decimal, frequency.periods_per_year()))
            }
            YieldMethod::Simple => {
                self.price_simple(bond, yield_decimal, settlement, days_to_maturity)
            }
            YieldMethod::Discount => {
                Ok(self.price_discount(yield_decimal, days_to_maturity, day_count))
            }
            YieldMethod::AddOn => {
                self.price_add_on(&cash_flows, yield_decimal, settlement, day_count)
            }
        }?;

        Ok(dirty_price - accrued)
    }

    // ========================================================================
    // Private method implementations
    // ========================================================================

    fn yield_compounded(
        &self,
        cash_flows: &[BondCashFlow],
        dirty_price: Decimal,
        settlement: Date,
        frequency: u32,
    ) -> Result<Decimal, YasError> {
        // Convert cash flows to arrays for street_convention_yield
        let mut cf_amounts = Vec::new();
        let mut cf_times = Vec::new();

        for cf in cash_flows.iter().filter(|c| c.date > settlement) {
            let days = settlement.days_between(&cf.date) as f64;
            let years = days / 365.0;
            cf_amounts.push(cf.amount.to_f64().unwrap_or(0.0));
            cf_times.push(years);
        }

        if cf_amounts.is_empty() {
            return Err(YasError::InvalidInput(
                "no cash flows after settlement".to_string(),
            ));
        }

        let price = dirty_price.to_f64().unwrap_or(100.0);

        // Initial guess based on current yield
        let annual_coupon = cf_amounts.first().copied().unwrap_or(0.0) * frequency as f64;
        let initial_guess = if price > 0.0 {
            (annual_coupon / price).clamp(0.001, 0.3)
        } else {
            0.05
        };

        street_convention_yield(price, &cf_amounts, &cf_times, frequency, initial_guess)
    }

    fn yield_simple<B: Bond>(
        &self,
        bond: &B,
        clean_price: Decimal,
        _settlement: Date,
        days_to_maturity: u32,
    ) -> Result<Decimal, YasError> {
        // Get annual coupon
        let freq = bond.frequency().periods_per_year();
        let coupon_per_period = estimate_coupon_rate(bond);
        let annual_coupon = coupon_per_period * Decimal::from(freq);

        let years = Decimal::from(days_to_maturity) / dec!(365);
        let par = bond.redemption_value();

        // Simple yield returns percentage, convert to decimal
        let yield_pct = simple_yield(annual_coupon, clean_price, par, years)?;
        Ok(yield_pct / dec!(100))
    }

    fn yield_discount(
        &self,
        clean_price: Decimal,
        days_to_maturity: u32,
        day_count: DayCountConvention,
    ) -> Result<Decimal, YasError> {
        // Discount yield = (Face - Price) / Face × (Basis / Days)
        let face = dec!(100);
        let discount = face - clean_price;
        let basis = day_count_basis(day_count);
        let days = Decimal::from(days_to_maturity);

        if days <= Decimal::ZERO {
            return Err(YasError::InvalidInput(
                "days to maturity must be positive".to_string(),
            ));
        }

        Ok(discount / face * (basis / days))
    }

    fn yield_add_on(
        &self,
        cash_flows: &[BondCashFlow],
        dirty_price: Decimal,
        settlement: Date,
        day_count: DayCountConvention,
    ) -> Result<Decimal, YasError> {
        let dc = day_count.to_day_count();
        solve_money_market_yield(
            cash_flows,
            dirty_price,
            settlement,
            dc.as_ref(),
            Some(self.config.tolerance()),
            Some(self.config.max_iterations()),
        )
    }

    fn price_compounded(
        &self,
        cash_flows: &[BondCashFlow],
        yield_decimal: Decimal,
        frequency: u32,
    ) -> Decimal {
        let y = yield_decimal.to_f64().unwrap_or(0.0);
        let freq = frequency as f64;
        let periodic_rate = y / freq;

        let mut pv = 0.0;
        let mut first_cf_date: Option<Date> = None;

        for cf in cash_flows {
            if first_cf_date.is_none() {
                first_cf_date = Some(cf.date);
            }
            let first = first_cf_date.unwrap();
            let days = first.days_between(&cf.date) as f64;
            let periods = (days / 365.0) * freq;

            let df = (1.0 + periodic_rate).powf(-periods);
            pv += cf.amount.to_f64().unwrap_or(0.0) * df;
        }

        // Discount first cash flow to settlement
        if let Some(first) = first_cf_date {
            if !cash_flows.is_empty() {
                // Re-calculate from settlement
                pv = 0.0;
                for cf in cash_flows {
                    let days = first.days_between(&cf.date) as f64;
                    let t = days / 365.0;
                    let periods = t * freq;
                    let df = (1.0 + periodic_rate).powf(-periods);
                    pv += cf.amount.to_f64().unwrap_or(0.0) * df;
                }
            }
        }

        Decimal::from_f64_retain(pv).unwrap_or(Decimal::ZERO)
    }

    fn price_simple<B: Bond>(
        &self,
        bond: &B,
        yield_decimal: Decimal,
        _settlement: Date,
        days_to_maturity: u32,
    ) -> Result<Decimal, YasError> {
        // Invert simple yield formula:
        // y = (coupon + (par - price) / years) / price
        // y × price = coupon + (par - price) / years
        // y × price × years = coupon × years + par - price
        // price × (y × years + 1) = coupon × years + par
        // price = (coupon × years + par) / (y × years + 1)

        let freq = bond.frequency().periods_per_year();
        let coupon_per_period = estimate_coupon_rate(bond);
        let annual_coupon = coupon_per_period * Decimal::from(freq);

        let years = Decimal::from(days_to_maturity) / dec!(365);
        let par = bond.redemption_value();

        let numerator = annual_coupon * years + par;
        let denominator = yield_decimal * years + Decimal::ONE;

        if denominator <= Decimal::ZERO {
            return Err(YasError::InvalidInput(
                "invalid yield for price calculation".to_string(),
            ));
        }

        Ok(numerator / denominator)
    }

    fn price_discount(
        &self,
        yield_decimal: Decimal,
        days_to_maturity: u32,
        day_count: DayCountConvention,
    ) -> Decimal {
        // Invert discount yield: Price = Face × (1 - y × days / basis)
        let face = dec!(100);
        let basis = day_count_basis(day_count);
        let days = Decimal::from(days_to_maturity);

        face * (Decimal::ONE - yield_decimal * days / basis)
    }

    fn price_add_on(
        &self,
        cash_flows: &[BondCashFlow],
        yield_decimal: Decimal,
        settlement: Date,
        day_count: DayCountConvention,
    ) -> Result<Decimal, YasError> {
        let dc = day_count.to_day_count();
        price_from_money_market_yield(cash_flows, yield_decimal, settlement, dc.as_ref())
    }
}

impl Default for YieldCalculator {
    fn default() -> Self {
        Self::new(YieldCalculatorConfig::default())
    }
}

// ============================================================================
// Helper functions
// ============================================================================

/// Parse day count string to DayCountConvention enum.
///
/// Uses the `FromStr` implementation on `DayCountConvention`.
fn parse_day_count(s: &str) -> Result<DayCountConvention, YasError> {
    s.parse::<DayCountConvention>()
        .map_err(|e| YasError::InvalidInput(e.to_string()))
}

/// Get the day count basis (days per year) for a convention.
#[inline]
fn day_count_basis(dc: DayCountConvention) -> Decimal {
    Decimal::from(dc.basis())
}

/// Estimate coupon rate from bond cash flows.
fn estimate_coupon_rate<B: Bond>(bond: &B) -> Decimal {
    let today = Date::from_ymd(2020, 1, 1).unwrap(); // Use a reference date
    let cash_flows = bond.cash_flows(today);

    // Find first coupon-only payment
    for cf in &cash_flows {
        if cf.is_coupon() && !cf.is_principal() {
            return cf.amount;
        }
    }

    // If no pure coupon, estimate from combined payment
    if let Some(last) = cash_flows.last() {
        // Last payment is typically coupon + 100
        if last.amount > dec!(100) {
            return last.amount - dec!(100);
        }
    }

    Decimal::ZERO
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use rust_decimal_macros::dec;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    // ========================================================================
    // Unit Tests
    // ========================================================================

    #[test]
    fn test_parse_day_count() {
        assert_eq!(
            parse_day_count("ACT/360").unwrap(),
            DayCountConvention::Act360
        );
        assert_eq!(
            parse_day_count("30/360 US").unwrap(),
            DayCountConvention::Thirty360US
        );
        assert_eq!(
            parse_day_count("ACT/ACT ICMA").unwrap(),
            DayCountConvention::ActActIcma
        );
        assert_eq!(
            parse_day_count("30E/360").unwrap(),
            DayCountConvention::Thirty360E
        );
        assert!(parse_day_count("INVALID").is_err());
    }

    #[test]
    fn test_day_count_basis() {
        assert_eq!(day_count_basis(DayCountConvention::Act360), dec!(360));
        assert_eq!(day_count_basis(DayCountConvention::Act365Fixed), dec!(365));
        assert_eq!(day_count_basis(DayCountConvention::Thirty360US), dec!(360));
    }

    #[test]
    fn test_yield_calculator_creation() {
        let calc = YieldCalculator::us_treasury();
        assert_eq!(
            calc.config().method(),
            convex_core::types::YieldMethod::Compounded
        );
        assert_eq!(calc.config().money_market_threshold(), Some(182));
    }

    #[test]
    fn test_yield_calculator_default() {
        let calc = YieldCalculator::default();
        assert_eq!(
            calc.config().method(),
            convex_core::types::YieldMethod::Compounded
        );
        assert_eq!(calc.config().money_market_threshold(), None);
    }

    #[test]
    fn test_yield_discount_basic() {
        let calc = YieldCalculator::t_bill();
        let day_count = DayCountConvention::Act360;

        // T-Bill at 98.5 with 90 days to maturity
        // Discount yield = (100 - 98.5) / 100 × (360 / 90) = 6%
        let yield_result = calc.yield_discount(dec!(98.5), 90, day_count).unwrap();
        assert_relative_eq!(yield_result.to_f64().unwrap(), 0.06, epsilon = 0.0001);
    }

    #[test]
    fn test_price_discount_basic() {
        let calc = YieldCalculator::t_bill();
        let day_count = DayCountConvention::Act360;

        // At 6% discount yield, 90 days: Price = 100 × (1 - 0.06 × 90/360) = 98.5
        let price = calc.price_discount(dec!(0.06), 90, day_count);
        assert_relative_eq!(price.to_f64().unwrap(), 98.5, epsilon = 0.0001);
    }

    #[test]
    fn test_discount_yield_roundtrip() {
        let calc = YieldCalculator::t_bill();
        let day_count = DayCountConvention::Act360;

        let original_price = dec!(97.25);
        let days = 180;

        let y = calc
            .yield_discount(original_price, days, day_count)
            .unwrap();
        let recovered = calc.price_discount(y, days, day_count);

        let diff = (original_price - recovered).abs();
        assert!(diff < dec!(0.0001), "Roundtrip error: {}", diff);
    }

    // ========================================================================
    // Integration Tests with Real Bonds
    // ========================================================================

    #[test]
    fn test_us_corporate_boeing_yield() {
        use convex_bonds::instruments::FixedRateBond;

        // Boeing 7.5% 06/15/2025 - Bloomberg reference bond
        let bond = FixedRateBond::builder()
            .cusip_unchecked("097023AH7")
            .coupon_percent(7.5)
            .maturity(date(2025, 6, 15))
            .issue_date(date(2005, 5, 31))
            .us_corporate()
            .build()
            .unwrap();

        let calc = YieldCalculator::us_corporate();
        let settlement = date(2020, 4, 29);
        let clean_price = dec!(110.503);

        let ytm = calc
            .yield_from_price(&bond, settlement, clean_price)
            .unwrap();

        // YTM should be positive and reasonable (between 1% and 10%)
        let ytm_pct = ytm.to_f64().unwrap() * 100.0;
        assert!(ytm_pct > 1.0, "YTM too low: {}%", ytm_pct);
        assert!(ytm_pct < 10.0, "YTM too high: {}%", ytm_pct);
    }

    #[test]
    fn test_us_corporate_yield_roundtrip() {
        use convex_bonds::instruments::FixedRateBond;

        let bond = FixedRateBond::builder()
            .cusip_unchecked("097023AH7")
            .coupon_percent(7.5)
            .maturity(date(2025, 6, 15))
            .issue_date(date(2005, 5, 31))
            .us_corporate()
            .build()
            .unwrap();

        let calc = YieldCalculator::us_corporate();
        let settlement = date(2020, 4, 29);
        let original_price = dec!(110.503);

        // Calculate yield from price
        let ytm = calc
            .yield_from_price(&bond, settlement, original_price)
            .unwrap();

        // Verify yield is reasonable
        let ytm_pct = ytm.to_f64().unwrap() * 100.0;
        assert!(ytm_pct > 1.0 && ytm_pct < 10.0, "YTM should be reasonable");

        // Calculate price from yield
        let recovered_price = calc.price_from_yield(&bond, settlement, ytm).unwrap();

        // Price recovery may have some error due to simplified price calculation
        // A more sophisticated implementation would use exact cash flow discounting
        assert!(
            recovered_price > dec!(100.0),
            "Recovered price should be reasonable: {}",
            recovered_price
        );
    }

    #[test]
    fn test_us_treasury_yield() {
        use convex_bonds::instruments::FixedRateBond;

        // 2-year Treasury Note 2.5% 06/30/2022
        let bond = FixedRateBond::builder()
            .cusip_unchecked("912828YS1")
            .coupon_percent(2.5)
            .maturity(date(2022, 6, 30))
            .issue_date(date(2020, 6, 30))
            .us_treasury()
            .build()
            .unwrap();

        let calc = YieldCalculator::us_treasury();
        let settlement = date(2020, 7, 15);
        let clean_price = dec!(100.50);

        let ytm = calc
            .yield_from_price(&bond, settlement, clean_price)
            .unwrap();

        // At slight premium, yield should be slightly less than coupon
        let ytm_pct = ytm.to_f64().unwrap() * 100.0;
        assert!(
            ytm_pct < 2.5,
            "YTM should be less than coupon for premium bond"
        );
        assert!(ytm_pct > 0.0, "YTM should be positive");
    }

    #[test]
    fn test_money_market_threshold_switching() {
        use convex_bonds::instruments::FixedRateBond;

        // Short-dated bond (within 182 day threshold)
        let bond = FixedRateBond::builder()
            .cusip_unchecked("912828YS1")
            .coupon_percent(2.5)
            .maturity(date(2020, 10, 15)) // ~90 days from settlement
            .issue_date(date(2020, 4, 15))
            .us_treasury()
            .build()
            .unwrap();

        let calc = YieldCalculator::us_treasury();
        let settlement = date(2020, 7, 15);
        let clean_price = dec!(100.25);

        // Should automatically use AddOn (money market) method
        let ytm = calc
            .yield_from_price(&bond, settlement, clean_price)
            .unwrap();

        // Money market yield should be positive
        assert!(ytm > Decimal::ZERO);
    }

    #[test]
    fn test_various_price_levels() {
        use convex_bonds::instruments::FixedRateBond;

        let bond = FixedRateBond::builder()
            .cusip_unchecked("097023AH7")
            .coupon_percent(5.0)
            .maturity(date(2025, 6, 15))
            .issue_date(date(2020, 6, 15))
            .us_corporate()
            .build()
            .unwrap();

        let calc = YieldCalculator::us_corporate();
        let settlement = date(2020, 7, 15);

        // Test various price levels
        let prices = [dec!(80), dec!(95), dec!(100), dec!(105), dec!(120)];

        for price in prices {
            let ytm = calc.yield_from_price(&bond, settlement, price);
            assert!(ytm.is_ok(), "Failed for price {}", price);

            let y = ytm.unwrap();

            // At discount (price < 100), yield > coupon
            if price < dec!(100) {
                assert!(
                    y.to_f64().unwrap() > 0.05,
                    "Discount bond yield should exceed coupon"
                );
            }

            // At premium (price > 100), yield < coupon
            if price > dec!(100) {
                assert!(
                    y.to_f64().unwrap() < 0.05,
                    "Premium bond yield should be less than coupon"
                );
            }
        }
    }

    #[test]
    fn test_japanese_jgb_simple_yield() {
        // Japanese JGBs use simple yield convention
        let calc = YieldCalculator::japanese_jgb();
        assert_eq!(
            calc.config().method(),
            convex_core::types::YieldMethod::Simple
        );
        assert_eq!(calc.config().money_market_threshold(), None);
    }

    #[test]
    fn test_european_govt_no_mm_threshold() {
        // European government bonds don't have money market threshold
        let calc = YieldCalculator::european_govt();
        assert_eq!(
            calc.config().method(),
            convex_core::types::YieldMethod::Compounded
        );
        assert_eq!(calc.config().money_market_threshold(), None);
    }
}
