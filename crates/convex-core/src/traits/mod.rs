//! Core traits for the Convex library.
//!
//! This module defines the fundamental abstractions used throughout Convex:
//!
//! - [`YieldCurve`]: Trait for yield curves providing discount factors and rates
//! - [`PricingEngine`]: Trait for bond pricing implementations
//! - [`RiskCalculator`]: Trait for risk metric calculations
//! - [`Discountable`]: Trait for cash flows that can be discounted

use rust_decimal::Decimal;

use crate::error::ConvexResult;
use crate::types::{CashFlow, Date, Price, Yield};

/// Trait for yield curves.
///
/// A yield curve provides discount factors and zero rates for any date.
/// Implementations may use different interpolation methods.
pub trait YieldCurve: Send + Sync {
    /// Returns the curve's reference (valuation) date.
    fn reference_date(&self) -> Date;

    /// Returns the discount factor for a given date.
    ///
    /// The discount factor represents the present value of $1 received
    /// at the given date.
    ///
    /// # Arguments
    ///
    /// * `date` - The date for which to calculate the discount factor
    ///
    /// # Returns
    ///
    /// A value between 0 and 1, where 1 means no discounting (date = reference_date).
    fn discount_factor(&self, date: Date) -> ConvexResult<Decimal>;

    /// Returns the continuously compounded zero rate for a given date.
    ///
    /// # Arguments
    ///
    /// * `date` - The date for which to calculate the zero rate
    fn zero_rate(&self, date: Date) -> ConvexResult<Decimal>;

    /// Returns the forward rate between two dates.
    ///
    /// # Arguments
    ///
    /// * `start` - Start date of the forward period
    /// * `end` - End date of the forward period
    fn forward_rate(&self, start: Date, end: Date) -> ConvexResult<Decimal> {
        let df_start = self.discount_factor(start)?;
        let df_end = self.discount_factor(end)?;

        if df_end == Decimal::ZERO {
            return Ok(Decimal::ZERO);
        }

        let days = start.days_between(&end) as f64;
        let years = days / 365.0;

        if years <= 0.0 {
            return Ok(Decimal::ZERO);
        }

        let ratio = df_start / df_end;
        let ratio_f64 = ratio.to_string().parse::<f64>().unwrap_or(1.0);
        let rate = (ratio_f64.powf(1.0 / years) - 1.0) * years / (days / 365.0);

        Ok(Decimal::from_f64_retain(rate).unwrap_or(Decimal::ZERO))
    }

    /// Returns the maximum date for which the curve is defined.
    fn max_date(&self) -> Date;
}

/// Trait for bond pricing engines.
///
/// Pricing engines calculate the present value of a bond's cash flows
/// using a yield curve for discounting.
pub trait PricingEngine: Send + Sync {
    /// The type of bond this engine can price.
    type Bond;

    /// Calculates the price of a bond.
    ///
    /// # Arguments
    ///
    /// * `bond` - The bond to price
    /// * `curve` - The yield curve for discounting
    /// * `settlement_date` - The settlement date for the trade
    fn price(
        &self,
        bond: &Self::Bond,
        curve: &dyn YieldCurve,
        settlement_date: Date,
    ) -> ConvexResult<Price>;

    /// Calculates the yield-to-maturity given a price.
    ///
    /// # Arguments
    ///
    /// * `bond` - The bond
    /// * `price` - The market price
    /// * `settlement_date` - The settlement date
    fn yield_to_maturity(
        &self,
        bond: &Self::Bond,
        price: Price,
        settlement_date: Date,
    ) -> ConvexResult<Yield>;
}

/// Trait for risk calculations.
pub trait RiskCalculator: Send + Sync {
    /// The type of bond this calculator works with.
    type Bond;

    /// Calculates the modified duration.
    ///
    /// Modified duration measures the percentage price change for a
    /// 1% change in yield.
    fn modified_duration(
        &self,
        bond: &Self::Bond,
        curve: &dyn YieldCurve,
        settlement_date: Date,
    ) -> ConvexResult<Decimal>;

    /// Calculates the Macaulay duration.
    ///
    /// Macaulay duration is the weighted average time to receive
    /// the bond's cash flows.
    fn macaulay_duration(
        &self,
        bond: &Self::Bond,
        curve: &dyn YieldCurve,
        settlement_date: Date,
    ) -> ConvexResult<Decimal>;

    /// Calculates the convexity.
    ///
    /// Convexity measures the curvature of the price-yield relationship.
    fn convexity(
        &self,
        bond: &Self::Bond,
        curve: &dyn YieldCurve,
        settlement_date: Date,
    ) -> ConvexResult<Decimal>;

    /// Calculates the DV01 (dollar value of one basis point).
    ///
    /// DV01 is the dollar change in price for a 1bp change in yield.
    fn dv01(
        &self,
        bond: &Self::Bond,
        curve: &dyn YieldCurve,
        settlement_date: Date,
    ) -> ConvexResult<Decimal>;
}

/// Trait for objects that can be discounted.
pub trait Discountable {
    /// Returns the date of the cash flow.
    fn payment_date(&self) -> Date;

    /// Returns the amount of the cash flow.
    fn amount(&self) -> Decimal;

    /// Calculates the present value given a yield curve.
    fn present_value(&self, curve: &dyn YieldCurve) -> ConvexResult<Decimal> {
        let df = curve.discount_factor(self.payment_date())?;
        Ok(self.amount() * df)
    }
}

impl Discountable for CashFlow {
    fn payment_date(&self) -> Date {
        self.date()
    }

    fn amount(&self) -> Decimal {
        CashFlow::amount(self)
    }
}

/// Trait for spread calculations.
pub trait SpreadCalculator: Send + Sync {
    /// The type of bond this calculator works with.
    type Bond;

    /// Calculates the Z-spread over a benchmark curve.
    ///
    /// The Z-spread is the constant spread that, when added to each point
    /// on the benchmark curve, makes the present value equal to the market price.
    fn z_spread(
        &self,
        bond: &Self::Bond,
        price: Price,
        curve: &dyn YieldCurve,
        settlement_date: Date,
    ) -> ConvexResult<Decimal>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CashFlowType;
    use rust_decimal_macros::dec;

    #[test]
    fn test_discountable_cashflow() {
        let cf = CashFlow::new(
            Date::from_ymd(2025, 6, 15).unwrap(),
            dec!(100),
            CashFlowType::Principal,
        );

        assert_eq!(cf.payment_date(), Date::from_ymd(2025, 6, 15).unwrap());
        assert_eq!(Discountable::amount(&cf), dec!(100));
    }
}
