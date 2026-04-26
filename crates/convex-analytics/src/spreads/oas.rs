//! Option-Adjusted Spread (OAS) calculator.
//!
//! Calculates OAS for callable bonds using a Hull-White trinomial-tree
//! pricer (matching `ql.TreeCallableFixedRateBondEngine`'s lattice).
//! OAS is the constant spread that, when added to the short rate at
//! every tree node, makes the model price equal the market price.

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

use convex_bonds::instruments::CallableBond;
use convex_bonds::options::{build_event_grid, HullWhite, ShortRateModel, TrinomialTree};
use convex_bonds::traits::{Bond, CashFlowType, EmbeddedOptionBond};
use convex_core::types::{Date, Spread, SpreadType};
use convex_curves::RateCurveDyn;
use convex_curves::{Compounding, CurveResult};

use crate::error::{AnalyticsError, AnalyticsResult};

/// A wrapper curve that applies a parallel shift to all rates.
struct ShiftedCurve<'a> {
    base: &'a dyn RateCurveDyn,
    shift: f64,
}

impl<'a> ShiftedCurve<'a> {
    fn new(base: &'a dyn RateCurveDyn, shift: f64) -> Self {
        Self { base, shift }
    }
}

impl RateCurveDyn for ShiftedCurve<'_> {
    fn discount_factor(&self, t: f64) -> CurveResult<f64> {
        let base_df = self.base.discount_factor(t)?;

        if t <= 0.0 {
            return Ok(base_df);
        }

        let base_rate = -base_df.ln() / t;
        let shifted_rate = base_rate + self.shift;
        let shifted_df = (-shifted_rate * t).exp();

        Ok(shifted_df)
    }

    fn reference_date(&self) -> Date {
        self.base.reference_date()
    }

    fn max_date(&self) -> Date {
        self.base.max_date()
    }

    fn zero_rate(&self, t: f64, compounding: Compounding) -> CurveResult<f64> {
        let base_rate = self.base.zero_rate(t, compounding)?;
        Ok(base_rate + self.shift)
    }

    fn forward_rate(&self, t1: f64, t2: f64) -> CurveResult<f64> {
        let base_fwd = self.base.forward_rate(t1, t2)?;
        Ok(base_fwd + self.shift)
    }

    fn instantaneous_forward(&self, t: f64) -> CurveResult<f64> {
        let base_inst_fwd = self.base.instantaneous_forward(t)?;
        Ok(base_inst_fwd + self.shift)
    }
}

/// OAS Calculator for callable/puttable bonds.
///
/// Uses binomial tree pricing to determine the spread that makes the
/// model price equal to the market price.
pub struct OASCalculator {
    model: Box<dyn ShortRateModel>,
    tree_steps: usize,
}

impl OASCalculator {
    /// Creates a new OAS calculator.
    ///
    /// # Arguments
    ///
    /// * `model` - Short rate model (Hull-White, BDT, etc.)
    /// * `tree_steps` - Number of time steps (more = more accurate, slower)
    pub fn new<M: ShortRateModel + 'static>(model: M, tree_steps: usize) -> Self {
        Self {
            model: Box::new(model),
            tree_steps: tree_steps.max(10),
        }
    }

    /// Creates a calculator with default Hull-White model.
    ///
    /// Uses 3% mean reversion and 100 tree steps.
    #[must_use]
    pub fn default_hull_white(volatility: f64) -> Self {
        Self::new(HullWhite::new(0.03, volatility), 100)
    }

    /// Creates a calculator with high precision settings.
    #[must_use]
    pub fn high_precision(volatility: f64) -> Self {
        Self::new(HullWhite::new(0.03, volatility), 500)
    }

    /// Returns the number of tree steps.
    #[must_use]
    pub fn tree_steps(&self) -> usize {
        self.tree_steps
    }

    /// Calculates OAS for a callable bond.
    ///
    /// # Arguments
    ///
    /// * `bond` - The callable bond
    /// * `dirty_price` - Market dirty price
    /// * `curve` - Interest rate curve
    /// * `settlement` - Settlement date
    ///
    /// # Returns
    ///
    /// OAS spread in basis points.
    pub fn calculate(
        &self,
        bond: &CallableBond,
        dirty_price: Decimal,
        curve: &dyn RateCurveDyn,
        settlement: Date,
    ) -> AnalyticsResult<Spread> {
        let maturity = bond.maturity().ok_or_else(|| {
            AnalyticsError::InvalidInput("Bond has no maturity (perpetual)".to_string())
        })?;

        if settlement >= maturity {
            return Err(AnalyticsError::InvalidSettlement {
                settlement: settlement.to_string(),
                maturity: maturity.to_string(),
            });
        }

        let target_price = dirty_price.to_f64().unwrap_or(100.0);

        // Binary search for OAS
        let mut low = -0.05;
        let mut high = 0.10;
        let tolerance = 1e-6;
        let max_iterations = 100;

        for _ in 0..max_iterations {
            let mid = (low + high) / 2.0;
            let model_price = self.price_with_oas(bond, curve, mid, settlement)?;

            if (model_price - target_price).abs() < tolerance {
                let oas_bps = mid * 10000.0;
                return Ok(Spread::new(
                    Decimal::from_f64_retain(oas_bps.round()).unwrap_or(Decimal::ZERO),
                    SpreadType::OAS,
                ));
            }

            if model_price > target_price {
                low = mid;
            } else {
                high = mid;
            }
        }

        let oas = (low + high) / 2.0;
        let oas_bps = oas * 10000.0;
        Ok(Spread::new(
            Decimal::from_f64_retain(oas_bps.round()).unwrap_or(Decimal::ZERO),
            SpreadType::OAS,
        ))
    }

    /// Prices a callable on an event-aligned HW1F trinomial tree:
    /// every cashflow and step-down boundary lands on a layer exactly
    /// (mirrors QL's `TimeGrid` inside `TreeCallableFixedRateBondEngine`).
    pub fn price_with_oas(
        &self,
        bond: &CallableBond,
        curve: &dyn RateCurveDyn,
        oas: f64,
        settlement: Date,
    ) -> AnalyticsResult<f64> {
        let maturity = bond.maturity().ok_or_else(|| {
            AnalyticsError::InvalidInput("Bond has no maturity (perpetual)".to_string())
        })?;

        let maturity_years = settlement.days_between(&maturity) as f64 / 365.0;
        if maturity_years <= 0.0 {
            return Err(AnalyticsError::InvalidInput(
                "Maturity before settlement".to_string(),
            ));
        }

        let base_bond = bond.base_bond();
        let call_schedule = bond
            .call_schedule()
            .ok_or_else(|| AnalyticsError::InvalidInput("Bond has no call schedule".to_string()))?;
        let cash_flows = base_bond.cash_flows(settlement);
        let face_value = base_bond.face_value().to_f64().unwrap_or(100.0);

        // Mandatory event times in (0, T): coupon/principal payments and
        // each step-down boundary date (where the call price changes).
        let mut mandatory: Vec<f64> = Vec::new();
        for cf in &cash_flows {
            if cf.flow_type != CashFlowType::Coupon
                && cf.flow_type != CashFlowType::Principal
                && cf.flow_type != CashFlowType::CouponAndPrincipal
            {
                continue;
            }
            let t = settlement.days_between(&cf.date) as f64 / 365.0;
            if t > 0.0 && t < maturity_years {
                mandatory.push(t);
            }
        }
        for entry in &call_schedule.entries {
            if entry.start_date > settlement && entry.start_date < maturity {
                let t = settlement.days_between(&entry.start_date) as f64 / 365.0;
                mandatory.push(t);
            }
        }

        let times = build_event_grid(maturity_years, &mandatory, self.tree_steps);

        let a = self.model.mean_reversion();
        let sigma = self.model.volatility(0.0);
        let zero_rates = |t: f64| -> f64 {
            if t <= 0.0 {
                return 0.0;
            }
            curve.zero_rate(t, Compounding::Continuous).unwrap_or(0.05)
        };
        let tree = TrinomialTree::build_hull_white_on_grid(zero_rates, a, sigma, &times);

        self.trinomial_backward_induction(bond, &tree, oas, settlement, face_value)
    }

    fn trinomial_backward_induction(
        &self,
        bond: &CallableBond,
        tree: &TrinomialTree,
        oas: f64,
        settlement: Date,
        face_value: f64,
    ) -> AnalyticsResult<f64> {
        let base_bond = bond.base_bond();
        let call_schedule = bond
            .call_schedule()
            .ok_or_else(|| AnalyticsError::InvalidInput("Bond has no call schedule".to_string()))?;
        let cash_flows = base_bond.cash_flows(settlement);

        let n = tree.steps;
        let mut step_amount = vec![0.0_f64; n + 1];
        let mut step_call: Vec<Option<f64>> = vec![None; n + 1];

        for cf in &cash_flows {
            if !matches!(
                cf.flow_type,
                CashFlowType::Coupon | CashFlowType::Principal | CashFlowType::CouponAndPrincipal
            ) {
                continue;
            }
            let cf_t = settlement.days_between(&cf.date) as f64 / 365.0;
            if cf_t <= 0.0 {
                continue;
            }
            // Cashflows on or after maturity (e.g., BDC-adjusted final flows)
            // bucket into the maturity layer; everything else must hit a
            // mandatory layer exactly.
            let i = tree.step_at_time(cf_t).unwrap_or(n).min(n);
            step_amount[i] += cf.amount.to_f64().unwrap_or(0.0);
        }

        // If the bond emits only coupons (no terminal principal flow),
        // inject face at maturity so the bullet leg prices.
        if step_amount[n] < face_value * 0.5 {
            step_amount[n] += face_value;
        }

        // Call cap = clean + accrued (QL's `Callability(price, Clean, date)`
        // ⇒ issuer pays clean + accrued). Coupon-on-call convention follows
        // QL's `withinNextWeek` snap: receive the coupon at the first
        // callable layer (no prior callability to snap to), forfeit it
        // elsewhere. Forfeit is encoded by `cap - cashflow` since
        // `min(cont, cap - cf) + cf == min(cont + cf, cap)`.
        let first_callable_date = call_schedule.first_call_date();
        for i in 1..=n {
            let days = (tree.times[i] * 365.0).round() as i64;
            let tree_date = settlement.add_days(days);
            if !call_schedule.is_callable_on(tree_date) {
                continue;
            }
            let clean_cap = call_schedule.call_price_on(tree_date).unwrap_or(100.0);
            let accrued = base_bond
                .accrued_interest(tree_date)
                .to_f64()
                .unwrap_or(0.0);
            let dirty_cap = clean_cap + accrued;
            let receive = first_callable_date.is_some_and(|d| tree_date == d);
            step_call[i] = Some(if receive {
                dirty_cap
            } else {
                dirty_cap - step_amount[i]
            });
        }

        Ok(tree.price(oas, |i| step_amount[i], |i| step_call[i]))
    }

    /// Calculates effective duration using OAS.
    pub fn effective_duration(
        &self,
        bond: &CallableBond,
        curve: &dyn RateCurveDyn,
        oas: f64,
        settlement: Date,
    ) -> AnalyticsResult<f64> {
        let shift = 0.0001;

        let price = self.price_with_oas(bond, curve, oas, settlement)?;

        let curve_up = ShiftedCurve::new(curve, shift);
        let curve_down = ShiftedCurve::new(curve, -shift);

        let price_up = self.price_with_oas(bond, &curve_up, oas, settlement)?;
        let price_down = self.price_with_oas(bond, &curve_down, oas, settlement)?;

        if price.abs() < 1e-10 {
            return Err(AnalyticsError::InvalidInput("Price is zero".to_string()));
        }

        Ok((price_down - price_up) / (2.0 * price * shift))
    }

    /// Calculates effective convexity using OAS.
    pub fn effective_convexity(
        &self,
        bond: &CallableBond,
        curve: &dyn RateCurveDyn,
        oas: f64,
        settlement: Date,
    ) -> AnalyticsResult<f64> {
        let shift = 0.0001;

        let price = self.price_with_oas(bond, curve, oas, settlement)?;

        let curve_up = ShiftedCurve::new(curve, shift);
        let curve_down = ShiftedCurve::new(curve, -shift);

        let price_up = self.price_with_oas(bond, &curve_up, oas, settlement)?;
        let price_down = self.price_with_oas(bond, &curve_down, oas, settlement)?;

        if price.abs() < 1e-10 {
            return Err(AnalyticsError::InvalidInput("Price is zero".to_string()));
        }

        Ok((price_down + price_up - 2.0 * price) / (price * shift * shift))
    }

    /// Returns the option value embedded in the callable bond.
    pub fn option_value(
        &self,
        bond: &CallableBond,
        curve: &dyn RateCurveDyn,
        oas: f64,
        settlement: Date,
    ) -> AnalyticsResult<f64> {
        let callable_price = self.price_with_oas(bond, curve, oas, settlement)?;

        let base_bond = bond.base_bond();
        let cash_flows = base_bond.cash_flows(settlement);

        let mut straight_price = 0.0;
        for cf in &cash_flows {
            let t = settlement.days_between(&cf.date) as f64 / 365.0;
            let df = curve.discount_factor(t).unwrap_or(1.0) * (-oas * t).exp();
            let amount = cf.amount.to_f64().unwrap_or(0.0);
            straight_price += amount * df;
        }

        Ok(straight_price - callable_price)
    }

    /// Calculates OAS spread duration.
    pub fn oas_duration(
        &self,
        bond: &CallableBond,
        curve: &dyn RateCurveDyn,
        oas: f64,
        settlement: Date,
    ) -> AnalyticsResult<f64> {
        let shift = 0.0001;

        let price = self.price_with_oas(bond, curve, oas, settlement)?;
        let price_up = self.price_with_oas(bond, curve, oas + shift, settlement)?;
        let price_down = self.price_with_oas(bond, curve, oas - shift, settlement)?;

        if price.abs() < 1e-10 {
            return Err(AnalyticsError::InvalidInput("Price is zero".to_string()));
        }

        Ok(-(price_up - price_down) / (2.0 * price * shift))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_bonds::instruments::FixedRateBond;
    use convex_bonds::types::{CallEntry, CallSchedule, CallType};
    use convex_curves::curves::DiscountCurveBuilder;
    use rust_decimal_macros::dec;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    fn create_flat_curve(rate: f64) -> impl RateCurveDyn {
        let ref_date = date(2024, 1, 15);
        // Start from t=0 with df=1 to ensure correct zero rate calculations
        // for very short tenors (avoids extrapolation issues with df->zero conversion)
        DiscountCurveBuilder::new(ref_date)
            .add_zero_rate(0.0001, rate) // Near-zero anchor point
            .add_zero_rate(0.01, rate)
            .add_zero_rate(0.25, rate)
            .add_zero_rate(0.5, rate)
            .add_zero_rate(1.0, rate)
            .add_zero_rate(2.0, rate)
            .add_zero_rate(5.0, rate)
            .add_zero_rate(10.0, rate)
            .with_extrapolation()
            .build()
            .unwrap()
    }

    fn create_callable_bond() -> CallableBond {
        let base = FixedRateBond::builder()
            .cusip_unchecked("123456789")
            .coupon_percent(5.0)
            .maturity(date(2029, 1, 15))
            .issue_date(date(2020, 1, 15))
            .us_corporate()
            .build()
            .unwrap();

        let call_schedule = CallSchedule::new(CallType::American)
            .with_entry(CallEntry::new(date(2025, 1, 15), 102.0))
            .with_entry(CallEntry::new(date(2027, 1, 15), 101.0))
            .with_entry(CallEntry::new(date(2028, 1, 15), 100.0));

        CallableBond::new(base, call_schedule)
    }

    #[test]
    fn test_calculator_creation() {
        let calc = OASCalculator::default_hull_white(0.01);
        assert_eq!(calc.tree_steps(), 100);
    }

    #[test]
    fn test_high_precision() {
        let calc = OASCalculator::high_precision(0.01);
        assert_eq!(calc.tree_steps(), 500);
    }

    #[test]
    fn test_price_with_zero_oas() {
        let calc = OASCalculator::new(HullWhite::new(0.03, 0.01), 50);
        let bond = create_callable_bond();
        let curve = create_flat_curve(0.05);
        let settlement = date(2024, 1, 17);

        let price = calc.price_with_oas(&bond, &curve, 0.0, settlement);
        assert!(price.is_ok());
        let p = price.unwrap();

        assert!(
            p > 70.0 && p < 130.0,
            "Price {} is out of reasonable range",
            p
        );
    }

    #[test]
    fn test_price_increases_with_negative_oas() {
        let calc = OASCalculator::new(HullWhite::new(0.03, 0.01), 50);
        let bond = create_callable_bond();
        let curve = create_flat_curve(0.05);
        let settlement = date(2024, 1, 17);

        let price_0 = calc.price_with_oas(&bond, &curve, 0.0, settlement).unwrap();
        let price_neg = calc
            .price_with_oas(&bond, &curve, -0.01, settlement)
            .unwrap();

        assert!(
            price_neg > price_0,
            "Price with negative OAS should be higher"
        );
    }

    #[test]
    fn test_effective_duration() {
        let calc = OASCalculator::new(HullWhite::new(0.03, 0.01), 50);
        let bond = create_callable_bond();
        let curve = create_flat_curve(0.05);
        let settlement = date(2024, 1, 17);

        let duration = calc.effective_duration(&bond, &curve, 0.005, settlement);
        assert!(duration.is_ok());

        let dur = duration.unwrap();
        assert!(dur > 0.0 && dur < 15.0, "Duration {} is out of range", dur);
    }

    #[test]
    fn test_settlement_after_maturity() {
        let calc = OASCalculator::default_hull_white(0.01);
        let bond = create_callable_bond();
        let curve = create_flat_curve(0.05);
        let settlement = date(2030, 1, 15);

        let result = calc.calculate(&bond, dec!(100), &curve, settlement);
        assert!(result.is_err());
    }
}
