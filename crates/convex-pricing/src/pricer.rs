//! Core cash flow pricing implementation.
//!
//! This module provides the [`CurvePricer`] struct which implements the
//! [`CashFlowPricer`] trait for generic present value calculations.

use rust_decimal::Decimal;

use convex_core::error::ConvexResult;
use convex_core::traits::CashFlowPricer;
use convex_core::types::{CashFlow, Date};
use convex_curves::traits::Curve;

use crate::error::{PricingError, PricingResult};

/// Generic cash flow pricer using a discount curve.
///
/// This is the primary implementation of [`CashFlowPricer`] and serves as
/// the foundation for all pricing operations in the library.
///
/// # Example
///
/// ```rust,ignore
/// use convex_pricing::CurvePricer;
/// use convex_core::traits::CashFlowPricer;
///
/// let pricer = CurvePricer::new(&discount_curve);
///
/// // Calculate present value
/// let pv = pricer.present_value(&cash_flows, settlement)?;
///
/// // Calculate PV with a spread
/// let pv_spread = pricer.present_value_with_spread(&cash_flows, 0.0100, settlement)?;
/// ```
pub struct CurvePricer<'a, C: Curve + ?Sized> {
    curve: &'a C,
}

impl<'a, C: Curve + ?Sized> CurvePricer<'a, C> {
    /// Creates a new pricer with the given discount curve.
    pub fn new(curve: &'a C) -> Self {
        Self { curve }
    }

    /// Returns a reference to the underlying curve.
    pub fn curve(&self) -> &C {
        self.curve
    }

    /// Calculates the present value using f64 for performance.
    ///
    /// This is the internal implementation that avoids Decimal conversion
    /// in the hot path.
    pub fn present_value_f64(
        &self,
        cash_flows: &[CashFlow],
        settlement: Date,
    ) -> PricingResult<f64> {
        if cash_flows.is_empty() {
            return Err(PricingError::NoCashFlows);
        }

        let mut pv = 0.0;
        let mut has_future_cf = false;

        for cf in cash_flows {
            let cf_date = cf.date();
            if cf_date <= settlement {
                continue;
            }

            has_future_cf = true;
            let t = self.curve.year_fraction(cf_date);
            let df = self.curve.discount_factor(t)?;
            let amount = cf.amount().to_string().parse::<f64>().unwrap_or(0.0);
            pv += amount * df;
        }

        if !has_future_cf {
            return Err(PricingError::no_future_cash_flows(settlement));
        }

        Ok(pv)
    }

    /// Calculates the present value with spread using f64.
    pub fn present_value_with_spread_f64(
        &self,
        cash_flows: &[CashFlow],
        spread: f64,
        settlement: Date,
    ) -> PricingResult<f64> {
        if cash_flows.is_empty() {
            return Err(PricingError::NoCashFlows);
        }

        let mut pv = 0.0;
        let mut has_future_cf = false;

        for cf in cash_flows {
            let cf_date = cf.date();
            if cf_date <= settlement {
                continue;
            }

            has_future_cf = true;
            let t = self.curve.year_fraction(cf_date);

            // Get base discount factor and adjust for spread
            // DF_spread = DF_base * exp(-spread * t)
            let base_df = self.curve.discount_factor(t)?;
            let spread_adjustment = (-spread * t).exp();
            let df = base_df * spread_adjustment;

            let amount = cf.amount().to_string().parse::<f64>().unwrap_or(0.0);
            pv += amount * df;
        }

        if !has_future_cf {
            return Err(PricingError::no_future_cash_flows(settlement));
        }

        Ok(pv)
    }

    /// Extracts cash flow data for solver operations.
    ///
    /// Returns a vector of (time, amount, spot_rate) tuples.
    pub fn extract_cash_flow_data(
        &self,
        cash_flows: &[CashFlow],
        settlement: Date,
    ) -> PricingResult<Vec<(f64, f64, f64)>> {
        let mut data = Vec::with_capacity(cash_flows.len());

        for cf in cash_flows {
            let cf_date = cf.date();
            if cf_date <= settlement {
                continue;
            }

            let t = self.curve.year_fraction(cf_date);
            let df = self.curve.discount_factor(t)?;

            // Convert DF to continuous zero rate: r = -ln(DF) / t
            let spot_rate = if t > 0.0 && df > 0.0 {
                -df.ln() / t
            } else {
                0.0
            };

            let amount = cf.amount().to_string().parse::<f64>().unwrap_or(0.0);
            data.push((t, amount, spot_rate));
        }

        if data.is_empty() {
            return Err(PricingError::no_future_cash_flows(settlement));
        }

        Ok(data)
    }
}

impl<C: Curve + ?Sized> CashFlowPricer for CurvePricer<'_, C> {
    fn present_value(&self, cash_flows: &[CashFlow], settlement: Date) -> ConvexResult<Decimal> {
        let pv = self
            .present_value_f64(cash_flows, settlement)
            .map_err(|e| convex_core::error::ConvexError::pricing_error(e.to_string()))?;

        Ok(Decimal::from_f64_retain(pv).unwrap_or(Decimal::ZERO))
    }

    fn present_value_with_spread(
        &self,
        cash_flows: &[CashFlow],
        spread: f64,
        settlement: Date,
    ) -> ConvexResult<Decimal> {
        let pv = self
            .present_value_with_spread_f64(cash_flows, spread, settlement)
            .map_err(|e| convex_core::error::ConvexError::pricing_error(e.to_string()))?;

        Ok(Decimal::from_f64_retain(pv).unwrap_or(Decimal::ZERO))
    }

    fn discount_factors(
        &self,
        cash_flows: &[CashFlow],
        settlement: Date,
    ) -> ConvexResult<Vec<(f64, f64)>> {
        let mut result = Vec::with_capacity(cash_flows.len());

        for cf in cash_flows {
            let cf_date = cf.date();
            if cf_date <= settlement {
                continue;
            }

            let t = self.curve.year_fraction(cf_date);
            let df = self.curve.discount_factor(t).map_err(|e| {
                convex_core::error::ConvexError::pricing_error(format!(
                    "Failed to get discount factor: {e}"
                ))
            })?;

            result.push((t, df));
        }

        Ok(result)
    }

    fn reference_date(&self) -> Date {
        self.curve.reference_date()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_core::types::CashFlowType;
    use rust_decimal_macros::dec;

    /// A simple flat curve for testing
    struct FlatCurve {
        rate: f64,
        ref_date: Date,
    }

    impl FlatCurve {
        fn new(rate: f64, ref_date: Date) -> Self {
            Self { rate, ref_date }
        }
    }

    impl Curve for FlatCurve {
        fn discount_factor(&self, t: f64) -> convex_curves::error::CurveResult<f64> {
            Ok((-self.rate * t).exp())
        }

        fn reference_date(&self) -> Date {
            self.ref_date
        }

        fn max_date(&self) -> Date {
            self.ref_date.add_years(100).unwrap()
        }
    }

    fn create_test_cash_flows(settlement: Date) -> Vec<CashFlow> {
        vec![
            CashFlow::new(
                settlement.add_months(6).unwrap(),
                dec!(2.5),
                CashFlowType::Coupon,
            ),
            CashFlow::new(
                settlement.add_months(12).unwrap(),
                dec!(2.5),
                CashFlowType::Coupon,
            ),
            CashFlow::new(
                settlement.add_months(12).unwrap(),
                dec!(100.0),
                CashFlowType::Principal,
            ),
        ]
    }

    #[test]
    fn test_curve_pricer_creation() {
        let settlement = Date::from_ymd(2025, 1, 1).unwrap();
        let curve = FlatCurve::new(0.05, settlement);
        let pricer = CurvePricer::new(&curve);

        assert_eq!(pricer.reference_date(), settlement);
    }

    #[test]
    fn test_present_value_f64() {
        let settlement = Date::from_ymd(2025, 1, 1).unwrap();
        let curve = FlatCurve::new(0.05, settlement);
        let pricer = CurvePricer::new(&curve);

        let cash_flows = create_test_cash_flows(settlement);
        let pv = pricer.present_value_f64(&cash_flows, settlement).unwrap();

        // PV should be positive and less than undiscounted sum (105)
        // With 5% discounting over 1 year, expect ~100
        assert!(pv > 95.0);
        assert!(pv < 105.0);
    }

    #[test]
    fn test_present_value_with_spread() {
        let settlement = Date::from_ymd(2025, 1, 1).unwrap();
        let curve = FlatCurve::new(0.05, settlement);
        let pricer = CurvePricer::new(&curve);

        let cash_flows = create_test_cash_flows(settlement);

        let pv_no_spread = pricer.present_value_f64(&cash_flows, settlement).unwrap();
        let pv_with_spread = pricer
            .present_value_with_spread_f64(&cash_flows, 0.01, settlement)
            .unwrap();

        // Adding spread should reduce PV
        assert!(pv_with_spread < pv_no_spread);
    }

    #[test]
    fn test_present_value_trait() {
        let settlement = Date::from_ymd(2025, 1, 1).unwrap();
        let curve = FlatCurve::new(0.05, settlement);
        let pricer = CurvePricer::new(&curve);

        let cash_flows = create_test_cash_flows(settlement);
        let pv = pricer.present_value(&cash_flows, settlement).unwrap();

        // Should return Decimal, PV around par value
        assert!(pv > dec!(95));
        assert!(pv < dec!(105));
    }

    #[test]
    fn test_discount_factors() {
        let settlement = Date::from_ymd(2025, 1, 1).unwrap();
        let curve = FlatCurve::new(0.05, settlement);
        let pricer = CurvePricer::new(&curve);

        let cash_flows = create_test_cash_flows(settlement);
        let dfs = pricer.discount_factors(&cash_flows, settlement).unwrap();

        // Should have 3 discount factors (for 3 cash flows)
        // But 2 are on the same date, so we get 3 entries
        assert_eq!(dfs.len(), 3);

        // All discount factors should be between 0 and 1
        for (t, df) in &dfs {
            assert!(*t > 0.0);
            assert!(*df > 0.0);
            assert!(*df < 1.0);
        }
    }

    #[test]
    fn test_no_cash_flows_error() {
        let settlement = Date::from_ymd(2025, 1, 1).unwrap();
        let curve = FlatCurve::new(0.05, settlement);
        let pricer = CurvePricer::new(&curve);

        let cash_flows: Vec<CashFlow> = vec![];
        let result = pricer.present_value_f64(&cash_flows, settlement);

        assert!(result.is_err());
    }

    #[test]
    fn test_no_future_cash_flows_error() {
        let settlement = Date::from_ymd(2025, 1, 1).unwrap();
        let curve = FlatCurve::new(0.05, settlement);
        let pricer = CurvePricer::new(&curve);

        // All cash flows before settlement
        let cash_flows = vec![CashFlow::new(
            settlement.add_days(-30),
            dec!(100.0),
            CashFlowType::Principal,
        )];

        let result = pricer.present_value_f64(&cash_flows, settlement);
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_cash_flow_data() {
        let settlement = Date::from_ymd(2025, 1, 1).unwrap();
        let curve = FlatCurve::new(0.05, settlement);
        let pricer = CurvePricer::new(&curve);

        let cash_flows = create_test_cash_flows(settlement);
        let data = pricer
            .extract_cash_flow_data(&cash_flows, settlement)
            .unwrap();

        // Should have 3 data points
        assert_eq!(data.len(), 3);

        // Each should have positive time and positive spot rate
        for (t, amount, spot_rate) in &data {
            assert!(*t > 0.0);
            assert!(*amount > 0.0);
            assert!(*spot_rate > 0.0);
        }
    }
}
