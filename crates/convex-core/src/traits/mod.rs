//! Core traits for the Convex library.
//!
//! This module defines the fundamental abstractions used throughout Convex:
//!
//! - [`YieldCurve`]: Trait for yield curves providing discount factors and rates
//! - [`PricingEngine`]: Trait for bond pricing implementations
//! - [`RiskCalculator`]: Trait for risk metric calculations
//! - [`Discountable`]: Trait for cash flows that can be discounted
//! - [`CashFlowPricer`]: Generic trait for pricing cash flow streams
//! - [`SpreadSolver`]: Generic trait for solving spread values

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

// ============================================================================
// Generic Pricing Traits (Phase 1 - Foundation for convex-pricing)
// ============================================================================

/// Generic trait for pricing streams of cash flows.
///
/// This trait provides a unified interface for pricing any sequence of
/// discountable cash flows, independent of the instrument type. It serves
/// as the foundation for the `convex-pricing` crate and enables code reuse
/// across bonds, spreads, and risk calculations.
///
/// # Design Rationale
///
/// Previously, PV calculations were duplicated across:
/// - `convex-bonds/src/pricing/mod.rs` (BondPricer)
/// - `convex-spreads/src/zspread.rs` (ZSpreadCalculator)
/// - `convex-spreads/src/discount_margin.rs` (DiscountMarginCalculator)
///
/// This trait consolidates that logic into a single, generic abstraction.
///
/// # Example
///
/// ```ignore
/// use convex_core::traits::CashFlowPricer;
///
/// // Any CashFlowPricer implementation can price any cash flow stream
/// let pv = pricer.present_value(&cash_flows, settlement)?;
/// let pv_with_spread = pricer.present_value_with_spread(&cash_flows, 0.0050, settlement)?;
/// ```
pub trait CashFlowPricer: Send + Sync {
    /// Calculates the present value of a stream of cash flows.
    ///
    /// # Arguments
    ///
    /// * `cash_flows` - The cash flows to price
    /// * `settlement` - The settlement date (valuation date)
    ///
    /// # Returns
    ///
    /// The present value as a Decimal.
    fn present_value(&self, cash_flows: &[CashFlow], settlement: Date) -> ConvexResult<Decimal>;

    /// Calculates the present value with a constant spread added to all discount rates.
    ///
    /// This is the core calculation for Z-spread, OAS, and similar spread measures.
    /// The spread is applied to the continuously compounded zero rate:
    /// `DF_spread = exp(-(r + spread) * t)`
    ///
    /// # Arguments
    ///
    /// * `cash_flows` - The cash flows to price
    /// * `spread` - The spread to add (as a decimal, e.g., 0.01 for 100 bps)
    /// * `settlement` - The settlement date
    ///
    /// # Returns
    ///
    /// The present value with the spread applied.
    fn present_value_with_spread(
        &self,
        cash_flows: &[CashFlow],
        spread: f64,
        settlement: Date,
    ) -> ConvexResult<Decimal>;

    /// Calculates individual discount factors for each cash flow.
    ///
    /// # Arguments
    ///
    /// * `cash_flows` - The cash flows
    /// * `settlement` - The settlement date
    ///
    /// # Returns
    ///
    /// A vector of (time_in_years, discount_factor) tuples.
    fn discount_factors(
        &self,
        cash_flows: &[CashFlow],
        settlement: Date,
    ) -> ConvexResult<Vec<(f64, f64)>>;

    /// Returns the reference date of the underlying curve.
    fn reference_date(&self) -> Date;
}

/// Generic trait for solving spread values.
///
/// This trait provides a unified interface for finding the spread that
/// equates the present value of cash flows to a target price. It is used
/// for Z-spread, discount margin, and similar calculations.
///
/// # Design Rationale
///
/// Spread solving follows a common pattern:
/// 1. Define objective function: `PV(spread) - target_price = 0`
/// 2. Use root-finding algorithm (Brent, Newton-Raphson)
/// 3. Return the spread that satisfies the equation
///
/// This trait abstracts that pattern for reuse.
pub trait SpreadSolver: Send + Sync {
    /// Solves for the spread that makes the PV equal to the target price.
    ///
    /// # Arguments
    ///
    /// * `cash_flows` - The cash flows to price
    /// * `target_price` - The target dirty price
    /// * `settlement` - The settlement date
    ///
    /// # Returns
    ///
    /// The spread as a decimal (e.g., 0.0150 for 150 bps).
    fn solve_spread(
        &self,
        cash_flows: &[CashFlow],
        target_price: Decimal,
        settlement: Date,
    ) -> ConvexResult<f64>;

    /// Solves for the spread with custom bounds.
    ///
    /// # Arguments
    ///
    /// * `cash_flows` - The cash flows to price
    /// * `target_price` - The target dirty price
    /// * `settlement` - The settlement date
    /// * `lower_bound` - Lower bound for spread search
    /// * `upper_bound` - Upper bound for spread search
    ///
    /// # Returns
    ///
    /// The spread as a decimal.
    fn solve_spread_bounded(
        &self,
        cash_flows: &[CashFlow],
        target_price: Decimal,
        settlement: Date,
        lower_bound: f64,
        upper_bound: f64,
    ) -> ConvexResult<f64>;
}

/// Generic trait for yield solving.
///
/// This trait provides a unified interface for finding the yield that
/// equates the present value of cash flows to a target price, using
/// a specified compounding convention.
pub trait YieldSolver: Send + Sync {
    /// Solves for the yield given a target price.
    ///
    /// # Arguments
    ///
    /// * `cash_flows` - The cash flows
    /// * `target_price` - The target dirty price
    /// * `settlement` - The settlement date
    /// * `frequency` - Compounding frequency (e.g., 2 for semi-annual)
    ///
    /// # Returns
    ///
    /// The yield as a decimal (e.g., 0.0525 for 5.25%).
    fn solve_yield(
        &self,
        cash_flows: &[CashFlow],
        target_price: Decimal,
        settlement: Date,
        frequency: u32,
    ) -> ConvexResult<f64>;

    /// Calculates the price given a yield.
    ///
    /// # Arguments
    ///
    /// * `cash_flows` - The cash flows
    /// * `yield_value` - The yield (as decimal)
    /// * `settlement` - The settlement date
    /// * `frequency` - Compounding frequency
    ///
    /// # Returns
    ///
    /// The dirty price.
    fn price_from_yield(
        &self,
        cash_flows: &[CashFlow],
        yield_value: f64,
        settlement: Date,
        frequency: u32,
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
