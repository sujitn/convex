//! Zero-coupon government bond wrapper for curve construction.

use rust_decimal::prelude::*;

use convex_core::Date;
use convex_curves::{CurveInstrument, InstrumentType, RateCurveDyn};
use convex_curves::CurveResult;

use crate::instruments::ZeroCouponBond;
use crate::traits::Bond;

use super::conventions::{day_count_factor, MarketConvention};

/// A zero-coupon government bond for curve bootstrapping.
///
/// Wraps a [`ZeroCouponBond`] with market data (settlement date and price)
/// to enable curve construction. Supports any market convention.
///
/// # Example
///
/// ```rust,ignore
/// use convex_bonds::prelude::*;
///
/// // UK T-Bill (6-month)
/// let tbill = GovernmentZeroCoupon::new(
///     ZeroCouponBond::new("GB0000123456", maturity, Currency::GBP),
///     settlement,
///     99.50,
///     MarketConvention::UKGilt,
/// );
/// ```
#[derive(Debug, Clone)]
pub struct GovernmentZeroCoupon {
    /// The underlying zero-coupon bond.
    bond: ZeroCouponBond,
    /// Settlement date for pricing.
    settlement: Date,
    /// Market price (clean = dirty for zero coupon).
    price: f64,
    /// Market convention (determines day count).
    convention: MarketConvention,
}

impl GovernmentZeroCoupon {
    /// Creates a new government zero-coupon bond for curve construction.
    ///
    /// # Arguments
    ///
    /// * `bond` - The underlying zero-coupon bond
    /// * `settlement` - Settlement date
    /// * `price` - Market price per 100 face value
    /// * `convention` - Market convention for day count
    #[must_use]
    pub fn new(
        bond: ZeroCouponBond,
        settlement: Date,
        price: f64,
        convention: MarketConvention,
    ) -> Self {
        Self {
            bond,
            settlement,
            price,
            convention,
        }
    }

    /// Creates from a discount rate quote.
    ///
    /// Converts discount rate to price using the formula:
    /// `Price = 100 * (1 - rate * days/360)`
    ///
    /// # Arguments
    ///
    /// * `bond` - The underlying zero-coupon bond
    /// * `settlement` - Settlement date
    /// * `discount_rate` - Annual discount rate as decimal (0.05 = 5%)
    /// * `convention` - Market convention
    #[must_use]
    pub fn from_discount_rate(
        bond: ZeroCouponBond,
        settlement: Date,
        discount_rate: f64,
        convention: MarketConvention,
    ) -> Self {
        let days = settlement.days_between(&bond.maturity_date()) as f64;
        // Discount instruments use 360-day year for rate calculation
        let price = 100.0 * (1.0 - discount_rate * days / 360.0);
        Self::new(bond, settlement, price, convention)
    }

    /// Returns the underlying bond.
    #[must_use]
    pub fn bond(&self) -> &ZeroCouponBond {
        &self.bond
    }

    /// Returns the settlement date.
    #[must_use]
    pub fn settlement(&self) -> Date {
        self.settlement
    }

    /// Returns the market price.
    #[must_use]
    pub fn price(&self) -> f64 {
        self.price
    }

    /// Returns the market convention.
    #[must_use]
    pub fn convention(&self) -> MarketConvention {
        self.convention
    }

    /// Calculates the implied yield (simple rate).
    ///
    /// `yield = (face_value / price - 1) / year_fraction`
    #[must_use]
    pub fn implied_yield(&self) -> f64 {
        let face_value: f64 = self.bond.face_value().to_f64().unwrap_or(100.0);
        let yf = day_count_factor(self.settlement, self.bond.maturity_date(), self.convention);
        if yf > 0.0 && self.price > 0.0 {
            (face_value / self.price - 1.0) / yf
        } else {
            0.0
        }
    }

    /// Calculates the year fraction from settlement to maturity.
    fn year_fraction(&self) -> f64 {
        day_count_factor(self.settlement, self.bond.maturity_date(), self.convention)
    }
}

impl CurveInstrument for GovernmentZeroCoupon {
    fn maturity(&self) -> Date {
        self.bond.maturity_date()
    }

    fn pv(&self, curve: &dyn RateCurveDyn) -> CurveResult<f64> {
        let face_value: f64 = self.bond.face_value().to_f64().unwrap_or(100.0);
        let yf = self.year_fraction();
        let df = curve.discount_factor(yf)?;

        // PV = face_value * DF - price
        // Should be ~0 when curve is calibrated correctly
        Ok(face_value * df - self.price)
    }

    fn implied_df(&self, _curve: &dyn RateCurveDyn, target_pv: f64) -> CurveResult<f64> {
        let face_value: f64 = self.bond.face_value().to_f64().unwrap_or(100.0);

        // PV = face_value * DF - price = target_pv
        // DF = (target_pv + price) / face_value
        Ok((target_pv + self.price) / face_value)
    }

    fn instrument_type(&self) -> InstrumentType {
        InstrumentType::GovernmentZeroCoupon
    }

    fn description(&self) -> String {
        format!(
            "{} {} @ {:.4} ({})",
            self.bond.identifier(),
            self.bond.maturity_date(),
            self.price,
            self.convention
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_core::types::Currency;
    use convex_curves::curves::DiscountCurveBuilder;
    use convex_curves::InterpolationMethod;

    fn create_test_bond() -> ZeroCouponBond {
        ZeroCouponBond::new(
            "TEST001",
            Date::from_ymd(2025, 7, 15).unwrap(),
            Currency::GBP,
        )
    }

    #[test]
    fn test_implied_yield() {
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();

        let gov_zero = GovernmentZeroCoupon::new(
            bond,
            settlement,
            97.50, // Price at discount
            MarketConvention::UKGilt,
        );

        // 6 months at discount should give positive yield
        let yield_rate = gov_zero.implied_yield();
        assert!(yield_rate > 0.0);
        assert!(yield_rate < 0.10); // Reasonable rate (< 10%)
    }

    #[test]
    fn test_from_discount_rate() {
        let bond = ZeroCouponBond::new(
            "TEST002",
            Date::from_ymd(2025, 7, 15).unwrap(),
            Currency::USD,
        );
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();

        let gov_zero = GovernmentZeroCoupon::from_discount_rate(
            bond,
            settlement,
            0.05, // 5% discount rate
            MarketConvention::USTreasury,
        );

        // Price should be less than 100
        assert!(gov_zero.price() < 100.0);
        assert!(gov_zero.price() > 95.0);
    }

    #[test]
    fn test_curve_instrument_pv() {
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();

        // Calculate expected price from a 5% flat curve
        let yf = day_count_factor(settlement, bond.maturity_date(), MarketConvention::UKGilt);
        let df = (-0.05 * yf).exp();
        let fair_price = 100.0 * df;

        let gov_zero =
            GovernmentZeroCoupon::new(bond, settlement, fair_price, MarketConvention::UKGilt);

        // Create a flat 5% curve
        let curve = DiscountCurveBuilder::new(settlement)
            .add_pillar(0.0, 1.0)
            .add_pillar(1.0, (-0.05_f64).exp())
            .add_pillar(2.0, (-0.10_f64).exp())
            .with_interpolation(InterpolationMethod::LogLinear)
            .build()
            .unwrap();

        // PV should be approximately 0
        let pv = gov_zero.pv(&curve).unwrap();
        assert!(pv.abs() < 0.01, "PV = {} should be near zero", pv);
    }

    #[test]
    fn test_implied_df() {
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();
        let price = 97.50;

        let gov_zero = GovernmentZeroCoupon::new(bond, settlement, price, MarketConvention::UKGilt);

        // Create a dummy curve (not used for zero-coupon implied_df)
        let curve = DiscountCurveBuilder::new(settlement)
            .add_pillar(0.0, 1.0)
            .add_pillar(1.0, 0.95)
            .build()
            .unwrap();

        // implied_df with target_pv = 0 should give price/face_value
        let df = gov_zero.implied_df(&curve, 0.0).unwrap();
        assert!((df - price / 100.0).abs() < 1e-10);
    }

    #[test]
    fn test_instrument_type() {
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();

        let gov_zero = GovernmentZeroCoupon::new(bond, settlement, 97.50, MarketConvention::UKGilt);

        assert_eq!(
            gov_zero.instrument_type(),
            InstrumentType::GovernmentZeroCoupon
        );
    }
}
