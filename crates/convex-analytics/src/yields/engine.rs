//! Supplied-cash yield diagnostics and checked scalar reporting.
//!
//! Scalar reporting and simple/current-yield measures have checked native owners.
//! The retained general solver and accrued methods do not qualify missing market
//! conventions or contextual cash mechanics; use the complete dated contract routes.
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_analytics::yields::{StandardYieldEngine, YieldEngine};
//! use convex_bonds::types::YieldCalculationRules;
//!
//! let engine = StandardYieldEngine::default();
//! let rules = YieldCalculationRules::us_treasury();
//!
//! let result = engine.yield_from_price(
//!     &cash_flows,
//!     clean_price,
//!     accrued,
//!     settlement,
//!     &rules,
//! )?;
//! ```

use rust_decimal::Decimal;

use convex_bonds::traits::BondCashFlow;
use convex_bonds::types::YieldCalculationRules;
use convex_core::types::Date;

use crate::error::AnalyticsResult;

/// The canonical native result, including checked reporting conversions.
pub use convex_bonds::pricing::{StandardYieldEngine, YieldEngineResult};

/// Trait for yield calculation engines.
///
/// Implementations provide methods to calculate yields from prices and vice versa,
/// for their explicitly supplied cash and rules; this is not convention qualification.
pub trait YieldEngine: Send + Sync {
    /// Calculate yield from price.
    ///
    /// # Arguments
    ///
    /// * `cash_flows` - All future cash flows (coupons and principal)
    /// * `clean_price` - Market clean price (per 100 face value)
    /// * `accrued` - Accrued interest
    /// * `settlement` - Settlement date
    /// * `rules` - Yield calculation rules
    ///
    /// # Returns
    ///
    /// The yield that makes PV(cash flows) = dirty price.
    fn yield_from_price(
        &self,
        cash_flows: &[BondCashFlow],
        clean_price: Decimal,
        accrued: Decimal,
        settlement: Date,
        rules: &YieldCalculationRules,
    ) -> AnalyticsResult<YieldEngineResult>;

    /// Calculate price from yield.
    ///
    /// # Arguments
    ///
    /// * `cash_flows` - All future cash flows
    /// * `yield_rate` - Annual yield rate as decimal
    /// * `settlement` - Settlement date
    /// * `rules` - Yield calculation rules
    ///
    /// # Returns
    ///
    /// The dirty price (PV of cash flows).
    fn price_from_yield(
        &self,
        cash_flows: &[BondCashFlow],
        yield_rate: f64,
        settlement: Date,
        rules: &YieldCalculationRules,
    ) -> AnalyticsResult<f64>;

    /// Calculate accrued interest.
    ///
    /// # Arguments
    ///
    /// * `settlement` - Settlement date
    /// * `last_coupon` - Last coupon date
    /// * `next_coupon` - Next coupon date
    /// * `coupon_rate` - Annual coupon rate as decimal
    /// * `face_value` - Face value of the bond
    /// * `rules` - Yield calculation rules
    ///
    /// # Returns
    ///
    /// Accrued interest amount.
    fn accrued_interest(
        &self,
        settlement: Date,
        last_coupon: Date,
        next_coupon: Date,
        coupon_rate: Decimal,
        face_value: Decimal,
        rules: &YieldCalculationRules,
    ) -> AnalyticsResult<Decimal>;
}

// One native implementation owns numerical checks and solver behavior.
impl YieldEngine for StandardYieldEngine {
    fn yield_from_price(
        &self,
        cash_flows: &[BondCashFlow],
        clean_price: Decimal,
        accrued: Decimal,
        settlement: Date,
        rules: &YieldCalculationRules,
    ) -> AnalyticsResult<YieldEngineResult> {
        convex_bonds::pricing::YieldEngine::yield_from_price(
            self,
            cash_flows,
            clean_price,
            accrued,
            settlement,
            rules,
        )
        .map_err(Into::into)
    }
    fn price_from_yield(
        &self,
        cash_flows: &[BondCashFlow],
        yield_rate: f64,
        settlement: Date,
        rules: &YieldCalculationRules,
    ) -> AnalyticsResult<f64> {
        convex_bonds::pricing::YieldEngine::price_from_yield(
            self, cash_flows, yield_rate, settlement, rules,
        )
        .map_err(Into::into)
    }
    fn accrued_interest(
        &self,
        settlement: Date,
        last_coupon: Date,
        next_coupon: Date,
        coupon_rate: Decimal,
        face_value: Decimal,
        rules: &YieldCalculationRules,
    ) -> AnalyticsResult<Decimal> {
        convex_bonds::pricing::YieldEngine::accrued_interest(
            self,
            settlement,
            last_coupon,
            next_coupon,
            coupon_rate,
            face_value,
            rules,
        )
        .map_err(Into::into)
    }
}

/// Calculate current yield.
///
/// Current yield = Annual Coupon / Clean Price
///
/// This is a simple measure that ignores time value of money
/// and capital gains/losses.
pub fn current_yield_simple(annual_coupon: f64, clean_price: f64) -> AnalyticsResult<f64> {
    convex_bonds::pricing::current_yield_simple(annual_coupon, clean_price).map_err(Into::into)
}

/// Arithmetic per100 simple-yield estimate as an annual decimal rate.
/// This does not identify a dated market convention.
pub fn simple_yield_f64(
    annual_coupon: f64,
    clean_price: f64,
    years_to_maturity: f64,
) -> AnalyticsResult<f64> {
    convex_bonds::pricing::simple_yield(annual_coupon, clean_price, years_to_maturity)
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use rust_decimal::prelude::ToPrimitive;
    use rust_decimal_macros::dec;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    fn create_coupon_cash_flows(
        settlement: Date,
        maturity: Date,
        coupon_rate: Decimal,
        face_value: Decimal,
        frequency: convex_core::types::Frequency,
    ) -> Vec<BondCashFlow> {
        let mut flows = Vec::new();
        let periods_per_year = frequency.periods_per_year();
        let coupon_amount = coupon_rate * face_value / Decimal::from(periods_per_year);

        let mut coupon_date = maturity;
        let months_per_period = 12 / periods_per_year;
        let mut dates = Vec::new();

        while coupon_date > settlement {
            dates.push(coupon_date);
            let (y, m, d) = (coupon_date.year(), coupon_date.month(), coupon_date.day());
            let new_month = if m as i32 - months_per_period as i32 <= 0 {
                (m as i32 + 12 - months_per_period as i32) as u32
            } else {
                m - months_per_period
            };
            let new_year = if m as i32 - months_per_period as i32 <= 0 {
                y - 1
            } else {
                y
            };
            coupon_date = Date::from_ymd(new_year, new_month, d.min(28)).unwrap_or(coupon_date);
            if coupon_date <= settlement {
                break;
            }
        }

        dates.reverse();

        for (i, &cf_date) in dates.iter().enumerate() {
            if i == dates.len() - 1 {
                flows.push(BondCashFlow::coupon_and_principal(
                    cf_date,
                    coupon_amount,
                    face_value,
                ));
            } else {
                flows.push(BondCashFlow::coupon(cf_date, coupon_amount));
            }
        }

        flows
    }

    #[test]
    fn test_engine_yield_at_par() {
        let settlement = date(2025, 1, 15);
        let maturity = date(2030, 6, 15);
        let coupon_rate = dec!(0.05);
        let face_value = dec!(100);

        let cash_flows = create_coupon_cash_flows(
            settlement,
            maturity,
            coupon_rate,
            face_value,
            convex_core::types::Frequency::SemiAnnual,
        );

        let engine = StandardYieldEngine::new();
        let rules = YieldCalculationRules::us_corporate();

        let result = engine
            .yield_from_price(&cash_flows, dec!(100), dec!(0), settlement, &rules)
            .unwrap();

        // At par, yield should equal coupon rate
        assert_relative_eq!(result.yield_value, 0.05, epsilon = 0.001);
    }

    #[test]
    fn test_engine_yield_discount_bond() {
        let settlement = date(2025, 1, 15);
        let maturity = date(2030, 6, 15);
        let coupon_rate = dec!(0.05);
        let face_value = dec!(100);

        let cash_flows = create_coupon_cash_flows(
            settlement,
            maturity,
            coupon_rate,
            face_value,
            convex_core::types::Frequency::SemiAnnual,
        );

        let engine = StandardYieldEngine::new();
        let rules = YieldCalculationRules::us_corporate();

        let result = engine
            .yield_from_price(&cash_flows, dec!(95), dec!(0), settlement, &rules)
            .unwrap();

        // Discount bond should have yield > coupon rate
        assert!(result.yield_value > 0.05);
    }

    #[test]
    fn test_engine_price_from_yield() {
        let settlement = date(2025, 1, 15);
        let maturity = date(2030, 6, 15);
        let coupon_rate = dec!(0.05);
        let face_value = dec!(100);

        let cash_flows = create_coupon_cash_flows(
            settlement,
            maturity,
            coupon_rate,
            face_value,
            convex_core::types::Frequency::SemiAnnual,
        );

        let engine = StandardYieldEngine::new();
        let rules = YieldCalculationRules::us_corporate();

        let price = engine
            .price_from_yield(&cash_flows, 0.05, settlement, &rules)
            .unwrap();

        // At par yield, price should be close to par
        assert_relative_eq!(price, 100.0, epsilon = 1.0);
    }

    #[test]
    fn test_engine_roundtrip() {
        let settlement = date(2025, 1, 15);
        let maturity = date(2030, 6, 15);
        let coupon_rate = dec!(0.05);
        let face_value = dec!(100);

        let cash_flows = create_coupon_cash_flows(
            settlement,
            maturity,
            coupon_rate,
            face_value,
            convex_core::types::Frequency::SemiAnnual,
        );

        let engine = StandardYieldEngine::new();
        let rules = YieldCalculationRules::us_corporate();

        let original_price = dec!(98.50);
        let accrued = dec!(1.25);

        // Price -> Yield
        let result = engine
            .yield_from_price(&cash_flows, original_price, accrued, settlement, &rules)
            .unwrap();

        // Yield -> Price
        let calculated_dirty = engine
            .price_from_yield(&cash_flows, result.yield_value, settlement, &rules)
            .unwrap();
        let calculated_clean = calculated_dirty - accrued.to_f64().unwrap();

        // Should round-trip
        assert_relative_eq!(
            calculated_clean,
            original_price.to_f64().unwrap(),
            epsilon = 0.01
        );
    }

    #[test]
    fn test_uk_gilt_rules() {
        let settlement = date(2025, 1, 15);
        let maturity = date(2030, 6, 15);
        let coupon_rate = dec!(0.04);
        let face_value = dec!(100);

        let cash_flows = create_coupon_cash_flows(
            settlement,
            maturity,
            coupon_rate,
            face_value,
            convex_core::types::Frequency::SemiAnnual,
        );

        let engine = StandardYieldEngine::new();
        let rules = YieldCalculationRules::uk_gilt();

        let result = engine
            .yield_from_price(&cash_flows, dec!(100), dec!(0), settlement, &rules)
            .unwrap();

        // Should converge
        assert!(result.iterations < 50);
        assert!(result.residual.abs() < 1e-6);
    }

    #[test]
    fn test_german_bund_rules() {
        let settlement = date(2025, 1, 15);
        let maturity = date(2030, 6, 15);
        let coupon_rate = dec!(0.03);
        let face_value = dec!(100);

        // Annual coupon for Bunds
        let cash_flows = create_coupon_cash_flows(
            settlement,
            maturity,
            coupon_rate,
            face_value,
            convex_core::types::Frequency::Annual,
        );

        let engine = StandardYieldEngine::new();
        let rules = YieldCalculationRules::german_bund();

        let result = engine
            .yield_from_price(&cash_flows, dec!(100), dec!(0), settlement, &rules)
            .unwrap();

        // Should converge with annual compounding
        assert!(result.iterations < 50);
    }

    #[test]
    fn test_current_yield() {
        let cy = current_yield_simple(5.0, 100.0).unwrap();
        assert_relative_eq!(cy, 0.05, epsilon = 0.0001);

        let cy = current_yield_simple(5.0, 110.0).unwrap();
        assert!(cy < 0.05);
    }

    #[test]
    fn test_simple_yield() {
        // At par, simple yield = coupon rate
        let sy = simple_yield_f64(5.0, 100.0, 5.0).unwrap();
        assert_relative_eq!(sy, 0.05, epsilon = 0.0001);

        // Discount bond
        let sy = simple_yield_f64(5.0, 95.0, 5.0).unwrap();
        assert!(sy > 0.05); // Should include capital gain
    }

    #[test]
    fn test_discount_yield() {
        // 3-month T-Bill at 99
        let dy = convex_bonds::pricing::bill::bank_discount_yield(dec!(99), dec!(100), 90).unwrap();
        assert!(dy > dec!(0));
        assert!(dy < dec!(0.05));
    }
}
