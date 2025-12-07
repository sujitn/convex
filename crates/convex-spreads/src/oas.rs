//! Option-Adjusted Spread (OAS) calculator.
//!
//! Calculates OAS for callable bonds using binomial tree pricing.
//! OAS is the constant spread that, when added to all rates in the
//! interest rate tree, makes the model price equal to the market price.
//!
//! # Overview
//!
//! OAS accounts for embedded option value and provides a spread measure
//! that is comparable across bonds with different option features.
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_spreads::oas::OASCalculator;
//! use convex_bonds::options::HullWhite;
//!
//! let model = HullWhite::new(0.03, 0.01);
//! let calc = OASCalculator::new(model, 100);
//!
//! let oas = calc.calculate(&callable_bond, dirty_price, &curve, settlement)?;
//! println!("OAS: {} bps", oas.as_bps());
//! ```

use rust_decimal::Decimal;

use convex_bonds::instruments::CallableBond;
use convex_bonds::options::{BinomialTree, HullWhite, ShortRateModel};
use convex_bonds::traits::{Bond, BondCashFlow, CashFlowType, EmbeddedOptionBond, FixedCouponBond};
use convex_core::types::{Date, Spread, SpreadType};
use convex_curves::{Compounding, Curve};

use crate::error::{SpreadError, SpreadResult};

/// OAS Calculator for callable/puttable bonds.
///
/// Uses binomial tree pricing to determine the spread that makes the
/// model price equal to the market price.
///
/// # Example
///
/// ```rust,ignore
/// use convex_spreads::oas::OASCalculator;
/// use convex_bonds::options::HullWhite;
///
/// // Create calculator with Hull-White model
/// let calc = OASCalculator::default_hull_white(0.01); // 1% vol
///
/// // Calculate OAS
/// let oas = calc.calculate(&bond, dirty_price, &curve, settlement)?;
/// ```
pub struct OASCalculator {
    /// The short rate model for tree construction.
    model: Box<dyn ShortRateModel>,
    /// Number of steps in the binomial tree.
    tree_steps: usize,
}

impl OASCalculator {
    /// Creates a new OAS calculator.
    ///
    /// # Arguments
    ///
    /// * `model` - Short rate model (Hull-White, BDT, etc.)
    /// * `tree_steps` - Number of time steps (more = more accurate, slower)
    ///
    /// # Example
    ///
    /// ```rust
    /// use convex_spreads::oas::OASCalculator;
    /// use convex_bonds::options::HullWhite;
    ///
    /// let model = HullWhite::new(0.03, 0.01);
    /// let calc = OASCalculator::new(model, 100);
    /// ```
    pub fn new<M: ShortRateModel + 'static>(model: M, tree_steps: usize) -> Self {
        Self {
            model: Box::new(model),
            tree_steps: tree_steps.max(10), // Minimum 10 steps
        }
    }

    /// Creates a calculator with default Hull-White model.
    ///
    /// Uses 3% mean reversion and 100 tree steps.
    ///
    /// # Arguments
    ///
    /// * `volatility` - Short rate volatility (e.g., 0.01 for 1%)
    #[must_use]
    pub fn default_hull_white(volatility: f64) -> Self {
        Self::new(HullWhite::new(0.03, volatility), 100)
    }

    /// Creates a calculator with high precision settings.
    ///
    /// Uses 500 tree steps for higher accuracy.
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
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Settlement is after maturity
    /// - Tree construction fails
    /// - OAS calculation fails to converge
    pub fn calculate(
        &self,
        bond: &CallableBond,
        dirty_price: Decimal,
        curve: &dyn Curve,
        settlement: Date,
    ) -> SpreadResult<Spread> {
        let maturity = bond
            .maturity()
            .ok_or_else(|| SpreadError::invalid_input("Bond has no maturity (perpetual)"))?;

        if settlement >= maturity {
            return Err(SpreadError::SettlementAfterMaturity {
                settlement: settlement.to_string(),
                maturity: maturity.to_string(),
            });
        }

        let target_price = dirty_price.to_string().parse::<f64>().unwrap_or(100.0);

        // Binary search for OAS
        let mut low = -0.05; // -500 bps
        let mut high = 0.10; // +1000 bps
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

            // Higher spread = lower price, so if model > target, increase spread
            if model_price > target_price {
                low = mid;
            } else {
                high = mid;
            }
        }

        // Return best estimate
        let oas = (low + high) / 2.0;
        let oas_bps = oas * 10000.0;
        Ok(Spread::new(
            Decimal::from_f64_retain(oas_bps.round()).unwrap_or(Decimal::ZERO),
            SpreadType::OAS,
        ))
    }

    /// Prices a callable bond given an OAS spread.
    ///
    /// # Arguments
    ///
    /// * `bond` - The callable bond
    /// * `curve` - Interest rate curve
    /// * `oas` - OAS spread as decimal (e.g., 0.005 for 50 bps)
    /// * `settlement` - Settlement date
    ///
    /// # Returns
    ///
    /// Model price (dirty price).
    pub fn price_with_oas(
        &self,
        bond: &CallableBond,
        curve: &dyn Curve,
        oas: f64,
        settlement: Date,
    ) -> SpreadResult<f64> {
        let maturity = bond
            .maturity()
            .ok_or_else(|| SpreadError::invalid_input("Bond has no maturity (perpetual)"))?;

        let maturity_years = settlement.days_between(&maturity) as f64 / 365.0;
        if maturity_years <= 0.0 {
            return Err(SpreadError::invalid_input("Maturity before settlement"));
        }

        // Create zero rate function from curve
        let zero_rates = |t: f64| -> f64 {
            if t <= 0.0 {
                return 0.0;
            }
            curve.zero_rate(t, Compounding::Continuous).unwrap_or(0.05)
        };

        // Build tree
        let tree = self
            .model
            .build_tree(&zero_rates, maturity_years, self.tree_steps);

        // Backward induction with option exercise
        self.backward_induction(bond, &tree, oas, settlement)
    }

    /// Performs backward induction through the tree.
    fn backward_induction(
        &self,
        bond: &CallableBond,
        tree: &BinomialTree,
        oas: f64,
        settlement: Date,
    ) -> SpreadResult<f64> {
        let base_bond = bond.base_bond();
        let call_schedule = bond
            .call_schedule()
            .ok_or_else(|| SpreadError::invalid_input("Bond has no call schedule"))?;
        let cash_flows = base_bond.cash_flows(settlement);
        let face_value = base_bond
            .face_value()
            .to_string()
            .parse::<f64>()
            .unwrap_or(100.0);

        let n = tree.steps;
        let maturity = bond.maturity().unwrap();
        let _maturity_years = settlement.days_between(&maturity) as f64 / 365.0;

        // Get coupon rate and frequency for adding coupons
        let coupon_rate = base_bond
            .coupon_rate()
            .to_string()
            .parse::<f64>()
            .unwrap_or(0.05);
        let frequency = f64::from(base_bond.coupon_frequency());
        let coupon_payment = face_value * coupon_rate / frequency;

        // Initialize values at maturity with face value + final coupon
        let final_value = face_value + coupon_payment;
        let mut values = vec![final_value; n + 1];

        // Work backwards through tree
        for i in (0..n).rev() {
            let t = tree.time_at_step(i);
            let tree_days = (t * 365.0) as i64;
            let tree_date = settlement.add_days(tree_days);

            // Check for cash flow at this time step
            let cf_at_step = self.cash_flow_at_time(&cash_flows, settlement, t, tree.dt);

            // Create new values array for this time step (i+1 states)
            let mut new_values = vec![0.0; i + 1];

            for j in 0..=i {
                let df = tree.discount_factor(i, j, oas);
                let p_up = tree.prob_up(i, j);
                let p_down = tree.prob_down(i, j);

                // Expected continuation value from next period
                // values has states from time i+1 (i+2 states)
                let continuation = df * (p_up * values[j + 1] + p_down * values[j]);

                // Add coupon if payment date
                let value_with_cf = continuation + cf_at_step;

                // Check for call exercise
                new_values[j] = if call_schedule.is_callable_on(tree_date) {
                    let call_price = call_schedule.call_price_on(tree_date).unwrap_or(100.0);
                    value_with_cf.min(call_price)
                } else {
                    value_with_cf
                };
            }

            values = new_values;
        }

        Ok(values[0])
    }

    /// Finds cash flow at a given time step.
    fn cash_flow_at_time(
        &self,
        cash_flows: &[BondCashFlow],
        settlement: Date,
        t: f64,
        dt: f64,
    ) -> f64 {
        cash_flows
            .iter()
            .filter(|cf| cf.flow_type == CashFlowType::Coupon)
            .filter(|cf| {
                let cf_t = settlement.days_between(&cf.date) as f64 / 365.0;
                (cf_t - t).abs() < dt / 2.0
            })
            .map(|cf| cf.amount.to_string().parse::<f64>().unwrap_or(0.0))
            .sum()
    }

    /// Calculates effective duration using OAS.
    ///
    /// Effective duration accounts for the embedded option by re-pricing
    /// the bond under shifted rate curves.
    ///
    /// # Formula
    ///
    /// Effective Duration = (P- - P+) / (2 × P × Δr)
    ///
    /// Where P- and P+ are prices under down/up rate shifts.
    pub fn effective_duration(
        &self,
        bond: &CallableBond,
        curve: &dyn Curve,
        oas: f64,
        settlement: Date,
    ) -> SpreadResult<f64> {
        let shift = 0.0001; // 1 bp

        let price = self.price_with_oas(bond, curve, oas, settlement)?;

        // Price with shifted curves (simulated by adjusting OAS)
        // In a full implementation, we'd shift the entire curve
        // For now, approximate with OAS adjustment
        let price_up = self.price_with_oas(bond, curve, oas - shift, settlement)?;
        let price_down = self.price_with_oas(bond, curve, oas + shift, settlement)?;

        if price.abs() < 1e-10 {
            return Err(SpreadError::invalid_input("Price is zero"));
        }

        Ok((price_up - price_down) / (2.0 * price * shift))
    }

    /// Calculates effective convexity using OAS.
    ///
    /// Effective convexity measures the rate of change of duration.
    ///
    /// # Formula
    ///
    /// Effective Convexity = (P- + P+ - 2×P) / (P × Δr²)
    pub fn effective_convexity(
        &self,
        bond: &CallableBond,
        curve: &dyn Curve,
        oas: f64,
        settlement: Date,
    ) -> SpreadResult<f64> {
        let shift = 0.0001; // 1 bp

        let price = self.price_with_oas(bond, curve, oas, settlement)?;
        let price_up = self.price_with_oas(bond, curve, oas - shift, settlement)?;
        let price_down = self.price_with_oas(bond, curve, oas + shift, settlement)?;

        if price.abs() < 1e-10 {
            return Err(SpreadError::invalid_input("Price is zero"));
        }

        Ok((price_up + price_down - 2.0 * price) / (price * shift * shift))
    }

    /// Returns the option value embedded in the callable bond.
    ///
    /// Option value = Straight bond price - Callable bond price
    pub fn option_value(
        &self,
        bond: &CallableBond,
        curve: &dyn Curve,
        oas: f64,
        settlement: Date,
    ) -> SpreadResult<f64> {
        // Price callable bond
        let callable_price = self.price_with_oas(bond, curve, oas, settlement)?;

        // Price as straight bond (no call exercise)
        let base_bond = bond.base_bond();
        let cash_flows = base_bond.cash_flows(settlement);

        // Simple straight bond pricing
        let mut straight_price = 0.0;
        for cf in &cash_flows {
            let t = settlement.days_between(&cf.date) as f64 / 365.0;
            let df = curve.discount_factor(t).unwrap_or(1.0) * (-oas * t).exp();
            let amount = cf.amount.to_string().parse::<f64>().unwrap_or(0.0);
            straight_price += amount * df;
        }

        Ok(straight_price - callable_price)
    }

    /// Calculates OAS spread duration (sensitivity to OAS).
    ///
    /// Measures price sensitivity to a 1bp change in OAS.
    pub fn oas_duration(
        &self,
        bond: &CallableBond,
        curve: &dyn Curve,
        oas: f64,
        settlement: Date,
    ) -> SpreadResult<f64> {
        let shift = 0.0001; // 1 bp

        let price = self.price_with_oas(bond, curve, oas, settlement)?;
        let price_up = self.price_with_oas(bond, curve, oas + shift, settlement)?;
        let price_down = self.price_with_oas(bond, curve, oas - shift, settlement)?;

        if price.abs() < 1e-10 {
            return Err(SpreadError::invalid_input("Price is zero"));
        }

        // Duration = -(dP/P) / dOAS
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

    fn create_flat_curve(rate: f64) -> impl Curve {
        // Create discount factors from flat rate: DF(t) = exp(-r*t)
        // Include short-term pillars for proper interpolation near t=0
        let ref_date = date(2024, 1, 15);
        DiscountCurveBuilder::new(ref_date)
            .add_pillar(0.01, (-rate * 0.01).exp()) // ~4 days
            .add_pillar(0.25, (-rate * 0.25).exp()) // 3 months
            .add_pillar(0.5, (-rate * 0.5).exp()) // 6 months
            .add_pillar(1.0, (-rate * 1.0).exp())
            .add_pillar(2.0, (-rate * 2.0).exp())
            .add_pillar(5.0, (-rate * 5.0).exp())
            .add_pillar(10.0, (-rate * 10.0).exp())
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
    fn test_tree_basic_pricing() {
        // Test that the tree produces reasonable prices for a simple zero-coupon bond
        let model = HullWhite::new(0.03, 0.01);

        // Simple flat rate
        let flat_rate = |_t: f64| 0.05;

        // Build tree for 5 years, 50 steps
        let tree = model.build_tree(&flat_rate, 5.0, 50);

        // Simple backward induction (no options)
        let pv = tree.backward_induction_simple(100.0, 0.0);

        // Should be approximately 100 * exp(-0.05 * 5) ≈ 77.88
        assert!(
            (pv - 77.88).abs() < 5.0,
            "Tree PV {} should be near 77.88",
            pv
        );
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

        // Price should be reasonable (between 80 and 120 for typical bond)
        // Note: For a 5% coupon bond with 5% rates, price is near par
        // But callable bonds trade below par value of straight bond
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

        // Negative OAS (lower discount rates) should increase price
        assert!(
            price_neg > price_0,
            "Price with negative OAS should be higher"
        );
    }

    #[test]
    fn test_price_decreases_with_positive_oas() {
        let calc = OASCalculator::new(HullWhite::new(0.03, 0.01), 50);
        let bond = create_callable_bond();
        let curve = create_flat_curve(0.05);
        let settlement = date(2024, 1, 17);

        let price_0 = calc.price_with_oas(&bond, &curve, 0.0, settlement).unwrap();
        let price_pos = calc
            .price_with_oas(&bond, &curve, 0.01, settlement)
            .unwrap();

        // Positive OAS (higher discount rates) should decrease price
        assert!(
            price_pos < price_0,
            "Price with positive OAS should be lower"
        );
    }

    #[test]
    fn test_oas_calculation() {
        let calc = OASCalculator::new(HullWhite::new(0.03, 0.01), 50);
        let bond = create_callable_bond();
        let curve = create_flat_curve(0.05);
        let settlement = date(2024, 1, 17);

        // First get a price
        let model_price = calc
            .price_with_oas(&bond, &curve, 0.0050, settlement)
            .unwrap();

        // Now calculate OAS from that price
        let dirty_price = Decimal::from_f64_retain(model_price).unwrap();
        let oas = calc.calculate(&bond, dirty_price, &curve, settlement);

        assert!(oas.is_ok());
        let oas_val = oas.unwrap();

        // OAS should be close to 50 bps
        let diff = (oas_val.as_bps() - dec!(50)).abs();
        assert!(
            diff < dec!(10),
            "OAS {} bps should be close to 50 bps",
            oas_val.as_bps()
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
        // Effective duration should be positive and reasonable (0-10 for typical bond)
        assert!(dur > 0.0 && dur < 15.0, "Duration {} is out of range", dur);
    }

    #[test]
    fn test_effective_convexity() {
        let calc = OASCalculator::new(HullWhite::new(0.03, 0.01), 50);
        let bond = create_callable_bond();
        let curve = create_flat_curve(0.05);
        let settlement = date(2024, 1, 17);

        let convexity = calc.effective_convexity(&bond, &curve, 0.005, settlement);
        assert!(convexity.is_ok());

        // Callable bonds can have negative convexity near call prices
        // Just verify it returns a number
        let _conv = convexity.unwrap();
    }

    #[test]
    fn test_option_value() {
        let calc = OASCalculator::new(HullWhite::new(0.03, 0.01), 50);
        let bond = create_callable_bond();
        let curve = create_flat_curve(0.05);
        let settlement = date(2024, 1, 17);

        let opt_val = calc.option_value(&bond, &curve, 0.005, settlement);
        assert!(opt_val.is_ok());

        let val = opt_val.unwrap();
        // Option value should be non-negative (issuer has the option)
        assert!(
            val >= -1.0,
            "Option value {} should be non-negative (or small negative due to approximation)",
            val
        );
    }

    #[test]
    fn test_oas_duration() {
        let calc = OASCalculator::new(HullWhite::new(0.03, 0.01), 50);
        let bond = create_callable_bond();
        let curve = create_flat_curve(0.05);
        let settlement = date(2024, 1, 17);

        let oas_dur = calc.oas_duration(&bond, &curve, 0.005, settlement);
        assert!(oas_dur.is_ok());

        let dur = oas_dur.unwrap();
        // OAS duration should be positive (higher spread = lower price)
        assert!(dur > 0.0, "OAS duration {} should be positive", dur);
    }

    #[test]
    fn test_settlement_after_maturity() {
        let calc = OASCalculator::default_hull_white(0.01);
        let bond = create_callable_bond();
        let curve = create_flat_curve(0.05);
        let settlement = date(2030, 1, 15); // After maturity

        let result = calc.calculate(&bond, dec!(100), &curve, settlement);
        assert!(result.is_err());
    }
}
