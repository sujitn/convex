//! Discount Margin (DM) calculation for Floating Rate Notes.
//!
//! The discount margin is the spread over forward rates that, when added to
//! projected coupons and used for discounting, makes the present value of
//! an FRN's cash flows equal to its market price.
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_spreads::DiscountMarginCalculator;
//! use convex_curves::curves::{ForwardCurve, DiscountCurve};
//! use convex_bonds::instruments::FloatingRateNote;
//!
//! let calculator = DiscountMarginCalculator::new(&forward_curve, &discount_curve);
//!
//! // Calculate discount margin from market price
//! let dm = calculator.calculate(&frn, dirty_price, settlement)?;
//! println!("Discount Margin: {} bps", dm.as_bps());
//!
//! // Price FRN with a given DM
//! let price = calculator.price_with_dm(&frn, 0.0050, settlement); // 50 bps
//!
//! // Simple margin (quick approximation)
//! let simple = DiscountMarginCalculator::simple_margin(&frn, dirty_price, current_rate, settlement);
//! ```

use rust_decimal::prelude::*;
use rust_decimal::Decimal;

use convex_bonds::instruments::FloatingRateNote;
use convex_bonds::traits::Bond;
use convex_core::types::{Date, Spread, SpreadType};
use convex_curves::curves::ForwardCurve;
use convex_curves::traits::Curve;
use convex_math::solvers::{brent, SolverConfig};

use crate::error::{SpreadError, SpreadResult};

/// Discount Margin calculator for floating rate notes.
///
/// The discount margin (DM) is the constant spread over forward rates that
/// equates the present value of projected cash flows to the market price.
/// It is the FRN equivalent of the Z-spread for fixed-rate bonds.
///
/// # Methodology
///
/// For each coupon period:
/// 1. Project the coupon rate as: forward rate + quoted spread
/// 2. Calculate the coupon payment based on the effective rate (with cap/floor)
/// 3. Discount using: base discount factor × exp(-DM × time)
///
/// The DM is the spread that solves:
/// ```text
/// Dirty Price = Σ CF_i × DF(t_i) × exp(-DM × t_i)
/// ```
///
/// # Performance
///
/// Target: < 100μs per calculation.
#[derive(Debug)]
pub struct DiscountMarginCalculator<'a, C: Curve + ?Sized> {
    /// Forward curve for projecting floating rates.
    forward_curve: &'a ForwardCurve,
    /// Discount curve for present value calculations.
    discount_curve: &'a C,
    /// Solver configuration.
    config: SolverConfig,
}

impl<'a, C: Curve + ?Sized> DiscountMarginCalculator<'a, C> {
    /// Creates a new Discount Margin calculator.
    ///
    /// # Arguments
    ///
    /// * `forward_curve` - Curve for projecting forward rates (e.g., 3M SOFR forwards)
    /// * `discount_curve` - Curve for discounting cash flows (typically OIS)
    #[must_use]
    pub fn new(forward_curve: &'a ForwardCurve, discount_curve: &'a C) -> Self {
        Self {
            forward_curve,
            discount_curve,
            config: SolverConfig::new(1e-10, 100),
        }
    }

    /// Sets the solver tolerance.
    ///
    /// Default tolerance is 1e-10 (0.0001 basis points).
    #[must_use]
    pub fn with_tolerance(mut self, tolerance: f64) -> Self {
        self.config = SolverConfig::new(tolerance, self.config.max_iterations);
        self
    }

    /// Sets the maximum iterations for the solver.
    #[must_use]
    pub fn with_max_iterations(mut self, max_iterations: u32) -> Self {
        self.config = SolverConfig::new(self.config.tolerance, max_iterations);
        self
    }

    /// Calculates the discount margin for an FRN.
    ///
    /// The DM is the spread added to each forward rate for discounting that
    /// makes the present value equal to the market dirty price.
    ///
    /// # Arguments
    ///
    /// * `frn` - The floating rate note
    /// * `dirty_price` - Market dirty price (as percentage of par, e.g., 100.50)
    /// * `settlement` - Settlement date
    ///
    /// # Returns
    ///
    /// The discount margin as a `Spread` in basis points.
    ///
    /// # Errors
    ///
    /// Returns `SpreadError` if:
    /// - Settlement is at or after maturity
    /// - The solver fails to converge
    /// - No future cash flows exist
    pub fn calculate(
        &self,
        frn: &FloatingRateNote,
        dirty_price: Decimal,
        settlement: Date,
    ) -> SpreadResult<Spread> {
        // Validate dates
        let maturity = frn
            .maturity()
            .ok_or_else(|| SpreadError::invalid_input("FRN has no maturity date"))?;

        if settlement >= maturity {
            return Err(SpreadError::SettlementAfterMaturity {
                settlement: settlement.to_string(),
                maturity: maturity.to_string(),
            });
        }

        let target_price = dirty_price.to_f64().unwrap_or(100.0);

        // Objective function: price(dm) - target = 0
        let objective = |dm: f64| self.price_with_dm(frn, dm, settlement) - target_price;

        // Search for DM between -5% and +20%
        // Most FRNs trade with DMs between -100 and +500 bps
        let result = brent(objective, -0.05, 0.20, &self.config)
            .map_err(|_| SpreadError::convergence_failed(self.config.max_iterations))?;

        // Convert to basis points
        let dm_bps = (result.root * 10_000.0).round();
        Ok(Spread::new(
            Decimal::from_f64_retain(dm_bps).unwrap_or_default(),
            SpreadType::DiscountMargin,
        ))
    }

    /// Prices an FRN given a discount margin.
    ///
    /// Projects cash flows using forward rates and discounts with the
    /// spread-adjusted discount curve.
    ///
    /// # Arguments
    ///
    /// * `frn` - The floating rate note
    /// * `dm` - Discount margin as a decimal (e.g., 0.0050 for 50 bps)
    /// * `settlement` - Settlement date
    ///
    /// # Returns
    ///
    /// The dirty price as a percentage of par.
    pub fn price_with_dm(&self, frn: &FloatingRateNote, dm: f64, settlement: Date) -> f64 {
        let maturity = match frn.maturity() {
            Some(m) => m,
            None => return 0.0,
        };

        if settlement >= maturity {
            return 0.0;
        }

        let face_value = frn.face_value().to_f64().unwrap_or(100.0);
        let quoted_spread = frn.spread_decimal().to_f64().unwrap_or(0.0);
        let ref_date = self.forward_curve.reference_date();
        let mut price = 0.0;

        // Get the FRN cash flows (uses current_rate or zero for projection)
        // We'll override the coupon amounts with forward-projected rates
        let cash_flows = frn.cash_flows(settlement);

        if cash_flows.is_empty() {
            return 0.0;
        }

        for cf in &cash_flows {
            if cf.date <= settlement {
                continue;
            }

            let years_to_cf = settlement.days_between(&cf.date) as f64 / 365.0;

            // Get base discount factor
            let df = self
                .discount_curve
                .discount_factor(years_to_cf)
                .unwrap_or(1.0);

            // Apply discount margin adjustment: DF_adjusted = DF × exp(-DM × t)
            let adjusted_df = df * (-dm * years_to_cf).exp();

            // Calculate cash flow amount
            let cf_amount = if cf.is_principal() {
                // For principal payment, include both coupon and principal
                // Coupon portion: project using forward rate
                let coupon_amount = if let (Some(start), Some(end)) = (cf.accrual_start, cf.accrual_end) {
                    let t1 = ref_date.days_between(&start) as f64 / 365.0;
                    let period_years = start.days_between(&end) as f64 / 365.0;

                    // Get forward rate for this period
                    let fwd_rate = self.forward_curve.forward_rate_at(t1).unwrap_or(0.05);

                    // Effective coupon rate = forward + quoted spread
                    let coupon_rate = fwd_rate + quoted_spread;
                    let effective_rate = frn.effective_rate(
                        Decimal::from_f64_retain(coupon_rate).unwrap_or(Decimal::ZERO),
                    );

                    face_value * effective_rate.to_f64().unwrap_or(0.0) * period_years
                } else {
                    cf.amount.to_f64().unwrap_or(0.0) - face_value
                };

                coupon_amount + face_value
            } else {
                // Regular coupon: project using forward rate
                if let (Some(start), Some(end)) = (cf.accrual_start, cf.accrual_end) {
                    let t1 = ref_date.days_between(&start) as f64 / 365.0;
                    let period_years = start.days_between(&end) as f64 / 365.0;

                    // Get forward rate for this period
                    let fwd_rate = self.forward_curve.forward_rate_at(t1).unwrap_or(0.05);

                    // Effective coupon rate = forward + quoted spread
                    let coupon_rate = fwd_rate + quoted_spread;
                    let effective_rate = frn.effective_rate(
                        Decimal::from_f64_retain(coupon_rate).unwrap_or(Decimal::ZERO),
                    );

                    face_value * effective_rate.to_f64().unwrap_or(0.0) * period_years
                } else {
                    cf.amount.to_f64().unwrap_or(0.0)
                }
            };

            price += cf_amount * adjusted_df;
        }

        // Normalize to percentage of face value
        price / face_value * 100.0
    }

    /// Calculates the spread DV01 (price sensitivity to 1bp DM change).
    ///
    /// # Arguments
    ///
    /// * `frn` - The floating rate note
    /// * `dm` - Current discount margin
    /// * `settlement` - Settlement date
    ///
    /// # Returns
    ///
    /// The price change for a 1 basis point increase in DM.
    pub fn spread_dv01(
        &self,
        frn: &FloatingRateNote,
        dm: Spread,
        settlement: Date,
    ) -> Decimal {
        let base_dm = dm.as_decimal().to_f64().unwrap_or(0.0) / 10_000.0;

        let base_price = self.price_with_dm(frn, base_dm, settlement);
        let bumped_price = self.price_with_dm(frn, base_dm + 0.0001, settlement);

        // DV01 is positive (price decreases when spread increases)
        Decimal::from_f64_retain(base_price - bumped_price).unwrap_or(Decimal::ZERO)
    }

    /// Calculates spread duration (percentage price sensitivity).
    ///
    /// # Arguments
    ///
    /// * `frn` - The floating rate note
    /// * `dm` - Current discount margin
    /// * `settlement` - Settlement date
    ///
    /// # Returns
    ///
    /// Spread duration = DV01 / Price × 10000
    pub fn spread_duration(
        &self,
        frn: &FloatingRateNote,
        dm: Spread,
        settlement: Date,
    ) -> Decimal {
        let base_dm = dm.as_decimal().to_f64().unwrap_or(0.0) / 10_000.0;
        let base_price = self.price_with_dm(frn, base_dm, settlement);

        if base_price <= 0.0 {
            return Decimal::ZERO;
        }

        let dv01 = self.spread_dv01(frn, dm, settlement);
        dv01 / Decimal::from_f64_retain(base_price).unwrap_or(Decimal::ONE) * Decimal::from(10_000)
    }

    /// Calculates effective spread duration accounting for caps and floors.
    ///
    /// For collared FRNs, this uses finite differences with rate shifts
    /// to capture the optionality effect.
    ///
    /// # Arguments
    ///
    /// * `frn` - The floating rate note
    /// * `dm` - Current discount margin
    /// * `settlement` - Settlement date
    /// * `rate_shift` - Parallel shift in rates (typically 0.01 for 1%)
    ///
    /// # Returns
    ///
    /// Effective duration accounting for embedded options.
    pub fn effective_duration(
        &self,
        frn: &FloatingRateNote,
        dm: Spread,
        settlement: Date,
        rate_shift: f64,
    ) -> Decimal {
        let base_dm = dm.as_decimal().to_f64().unwrap_or(0.0) / 10_000.0;
        let base_price = self.price_with_dm(frn, base_dm, settlement);

        if base_price <= 0.0 {
            return Decimal::ZERO;
        }

        // For FRNs, rate changes mostly affect through the DM
        // A proper implementation would shift the entire forward curve
        // Here we approximate by shifting the DM
        let price_up = self.price_with_dm(frn, base_dm + rate_shift, settlement);
        let price_down = self.price_with_dm(frn, base_dm - rate_shift, settlement);

        let duration = (price_down - price_up) / (2.0 * base_price * rate_shift);
        Decimal::from_f64_retain(duration).unwrap_or(Decimal::ZERO)
    }
}

/// Simple margin calculation (flat forward assumption).
///
/// The simple margin is a quick approximation that assumes:
/// - Forward rates remain constant at the current index rate
/// - Linear discounting approximation
///
/// This is useful for quick screening but less accurate than full DM.
///
/// # Arguments
///
/// * `frn` - The floating rate note
/// * `dirty_price` - Market dirty price (as percentage of par)
/// * `current_index` - Current index rate as a decimal (e.g., 0.0525 for 5.25%)
/// * `settlement` - Settlement date
///
/// # Returns
///
/// Simple margin as a `Spread` in basis points.
///
/// # Formula
///
/// ```text
/// Simple Margin = Current Yield + (Par - Price) / (Price × Years to Maturity) - Current Index
/// ```
///
/// where Current Yield = (Index + Quoted Spread) × Face / Price
pub fn simple_margin(
    frn: &FloatingRateNote,
    dirty_price: Decimal,
    current_index: Decimal,
    settlement: Date,
) -> Spread {
    let maturity = match frn.maturity() {
        Some(m) => m,
        None => return Spread::new(Decimal::ZERO, SpreadType::DiscountMargin),
    };

    let remaining_years = settlement.days_between(&maturity) as f64 / 365.0;

    if remaining_years <= 0.0 || dirty_price <= Decimal::ZERO {
        return Spread::new(Decimal::ZERO, SpreadType::DiscountMargin);
    }

    let face = frn.face_value();
    let price = dirty_price;
    let quoted_spread = frn.spread_decimal();

    // Current coupon rate (index + quoted spread)
    let coupon_rate = current_index + quoted_spread;

    // Current yield = (face × coupon_rate) / price
    let annual_coupon = face * coupon_rate;
    let current_yield = annual_coupon / price;

    // Redemption adjustment = (face - price) / (price × remaining_years)
    let redemption_effect = (face - price) / (price * Decimal::from_f64_retain(remaining_years).unwrap_or(Decimal::ONE));

    // Simple margin = current yield + redemption effect - current index
    let simple_margin = current_yield + redemption_effect - current_index;

    // Convert to basis points
    let margin_bps = (simple_margin * Decimal::from(10_000)).round();

    Spread::new(margin_bps, SpreadType::DiscountMargin)
}

/// Calculates the Z-DM (zero discount margin) for an FRN.
///
/// The Z-DM is the constant spread over the zero curve (not forward curve)
/// that prices the FRN. This is analogous to Z-spread for fixed bonds.
///
/// # Arguments
///
/// * `frn` - The floating rate note
/// * `dirty_price` - Market dirty price
/// * `forward_curve` - For projecting coupons
/// * `discount_curve` - Zero curve for discounting
/// * `settlement` - Settlement date
///
/// # Returns
///
/// Z-DM as a Spread in basis points.
pub fn z_discount_margin<C: Curve + ?Sized>(
    frn: &FloatingRateNote,
    dirty_price: Decimal,
    forward_curve: &ForwardCurve,
    discount_curve: &C,
    settlement: Date,
) -> SpreadResult<Spread> {
    DiscountMarginCalculator::new(forward_curve, discount_curve)
        .calculate(frn, dirty_price, settlement)
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_bonds::types::RateIndex;
    use convex_curves::curves::DiscountCurveBuilder;
    use convex_curves::interpolation::InterpolationMethod;
    use rust_decimal_macros::dec;
    use std::sync::Arc;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    fn create_sample_frn() -> FloatingRateNote {
        FloatingRateNote::builder()
            .cusip_unchecked("TEST12345")
            .index(RateIndex::SOFR)
            .spread_bps(50) // 50 bps spread over SOFR
            .face_value(dec!(100))
            .maturity(date(2027, 6, 15))
            .issue_date(date(2025, 6, 15))
            .corporate_sofr()
            .build()
            .unwrap()
    }

    fn create_sample_discount_curve() -> impl Curve {
        DiscountCurveBuilder::new(date(2025, 6, 15))
            .add_pillar(0.25, 0.9875) // 3M: ~5%
            .add_pillar(0.5, 0.975)   // 6M: ~5%
            .add_pillar(1.0, 0.95)    // 1Y: ~5.13%
            .add_pillar(2.0, 0.90)    // 2Y: ~5.27%
            .add_pillar(5.0, 0.78)    // 5Y: ~5.0%
            .with_interpolation(InterpolationMethod::LogLinear)
            .with_extrapolation()
            .build()
            .unwrap()
    }

    fn create_sample_forward_curve(discount_curve: Arc<dyn Curve>) -> ForwardCurve {
        ForwardCurve::from_months(discount_curve, 3)
    }

    #[test]
    fn test_calculator_creation() {
        let discount = create_sample_discount_curve();
        let discount_arc: Arc<dyn Curve> = Arc::new(discount);
        let forward = create_sample_forward_curve(discount_arc.clone());

        let _calc = DiscountMarginCalculator::new(&forward, discount_arc.as_ref())
            .with_tolerance(1e-8)
            .with_max_iterations(50);
    }

    #[test]
    fn test_price_with_dm_par() {
        let discount = create_sample_discount_curve();
        let discount_arc: Arc<dyn Curve> = Arc::new(discount);
        let forward = create_sample_forward_curve(discount_arc.clone());
        let frn = create_sample_frn();
        let settlement = date(2025, 6, 15);

        let calc = DiscountMarginCalculator::new(&forward, discount_arc.as_ref());

        // Price with zero DM
        let price_zero_dm = calc.price_with_dm(&frn, 0.0, settlement);
        assert!(price_zero_dm > 90.0 && price_zero_dm < 110.0, "Price {} out of range", price_zero_dm);

        // Price with positive DM should be lower
        let price_50bps = calc.price_with_dm(&frn, 0.0050, settlement);
        assert!(price_50bps < price_zero_dm, "Price with DM should be lower");

        // Price with negative DM should be higher
        let price_neg = calc.price_with_dm(&frn, -0.0050, settlement);
        assert!(price_neg > price_zero_dm, "Price with negative DM should be higher");
    }

    #[test]
    fn test_dm_calculation_roundtrip() {
        let discount = create_sample_discount_curve();
        let discount_arc: Arc<dyn Curve> = Arc::new(discount);
        let forward = create_sample_forward_curve(discount_arc.clone());
        let frn = create_sample_frn();
        let settlement = date(2025, 6, 15);

        let calc = DiscountMarginCalculator::new(&forward, discount_arc.as_ref());

        // Price at 50 bps DM
        let price_at_50bps = calc.price_with_dm(&frn, 0.0050, settlement);

        // Calculate DM from that price - should get back ~50 bps
        let dirty_price = Decimal::from_f64_retain(price_at_50bps).unwrap();
        let calculated_dm = calc.calculate(&frn, dirty_price, settlement).unwrap();

        let dm_bps = calculated_dm.as_bps().to_f64().unwrap();
        assert!(
            (dm_bps - 50.0).abs() < 1.0,
            "Expected ~50 bps, got {} bps",
            dm_bps
        );
    }

    #[test]
    fn test_dm_roundtrip_various_levels() {
        let discount = create_sample_discount_curve();
        let discount_arc: Arc<dyn Curve> = Arc::new(discount);
        let forward = create_sample_forward_curve(discount_arc.clone());
        let frn = create_sample_frn();
        let settlement = date(2025, 6, 15);

        let calc = DiscountMarginCalculator::new(&forward, discount_arc.as_ref());

        // Test at various DM levels
        for dm_bps in [25.0, 50.0, 100.0, 200.0] {
            let dm = dm_bps / 10_000.0;
            let price = calc.price_with_dm(&frn, dm, settlement);

            let calculated_dm = calc
                .calculate(&frn, Decimal::from_f64_retain(price).unwrap(), settlement)
                .unwrap();

            let calculated_bps = calculated_dm.as_bps().to_f64().unwrap();
            assert!(
                (calculated_bps - dm_bps).abs() < 0.5,
                "DM {}: expected {} bps, got {} bps",
                dm_bps,
                dm_bps,
                calculated_bps
            );
        }
    }

    #[test]
    fn test_spread_dv01() {
        let discount = create_sample_discount_curve();
        let discount_arc: Arc<dyn Curve> = Arc::new(discount);
        let forward = create_sample_forward_curve(discount_arc.clone());
        let frn = create_sample_frn();
        let settlement = date(2025, 6, 15);

        let calc = DiscountMarginCalculator::new(&forward, discount_arc.as_ref());
        let dm = Spread::new(dec!(50), SpreadType::DiscountMargin); // 50 bps

        let dv01 = calc.spread_dv01(&frn, dm, settlement);

        // DV01 should be positive and reasonable
        assert!(dv01 > Decimal::ZERO, "DV01 should be positive");
        assert!(dv01 < dec!(0.1), "DV01 should be less than 10 cents per 100");
    }

    #[test]
    fn test_spread_duration() {
        let discount = create_sample_discount_curve();
        let discount_arc: Arc<dyn Curve> = Arc::new(discount);
        let forward = create_sample_forward_curve(discount_arc.clone());
        let frn = create_sample_frn();
        let settlement = date(2025, 6, 15);

        let calc = DiscountMarginCalculator::new(&forward, discount_arc.as_ref());
        let dm = Spread::new(dec!(50), SpreadType::DiscountMargin);

        let duration = calc.spread_duration(&frn, dm, settlement);

        // Duration should be positive and reasonable (around 2 for 2Y FRN)
        assert!(duration > Decimal::ZERO, "Duration should be positive");
        assert!(duration < dec!(10), "Duration should be less than 10");
    }

    #[test]
    fn test_simple_margin() {
        let frn = create_sample_frn();
        let settlement = date(2025, 6, 15);
        let dirty_price = dec!(99.50); // Slight discount
        let current_index = dec!(0.0525); // 5.25% SOFR

        let margin = simple_margin(&frn, dirty_price, current_index, settlement);

        // Simple margin should be positive for discount bond
        // and include both current yield spread and redemption effect
        let margin_bps = margin.as_bps();
        assert!(margin_bps > Decimal::ZERO, "Margin should be positive for discount bond");
    }

    #[test]
    fn test_simple_margin_par() {
        let frn = create_sample_frn();
        let settlement = date(2025, 6, 15);
        let dirty_price = dec!(100.0); // At par
        let current_index = dec!(0.05); // 5% SOFR

        let margin = simple_margin(&frn, dirty_price, current_index, settlement);

        // At par, simple margin should equal the quoted spread (50 bps)
        let margin_bps = margin.as_bps().to_f64().unwrap();
        assert!(
            (margin_bps - 50.0).abs() < 5.0,
            "At par, simple margin should be close to quoted spread: {} bps",
            margin_bps
        );
    }

    #[test]
    fn test_settlement_after_maturity() {
        let discount = create_sample_discount_curve();
        let discount_arc: Arc<dyn Curve> = Arc::new(discount);
        let forward = create_sample_forward_curve(discount_arc.clone());
        let frn = create_sample_frn();
        let settlement = date(2028, 1, 1); // After maturity

        let calc = DiscountMarginCalculator::new(&forward, discount_arc.as_ref());

        let result = calc.calculate(&frn, dec!(100), settlement);
        assert!(result.is_err());
    }

    #[test]
    fn test_z_discount_margin_convenience() {
        let discount = create_sample_discount_curve();
        let discount_arc: Arc<dyn Curve> = Arc::new(discount);
        let forward = create_sample_forward_curve(discount_arc.clone());
        let frn = create_sample_frn();
        let settlement = date(2025, 6, 15);

        let price = dec!(99.50);

        let dm = z_discount_margin(&frn, price, &forward, discount_arc.as_ref(), settlement);
        assert!(dm.is_ok());
    }

    #[test]
    fn test_dm_with_cap() {
        // Create FRN with cap
        let frn = FloatingRateNote::builder()
            .cusip_unchecked("CAPPED001")
            .index(RateIndex::SOFR)
            .spread_bps(50)
            .cap(dec!(0.08)) // 8% cap
            .face_value(dec!(100))
            .maturity(date(2027, 6, 15))
            .issue_date(date(2025, 6, 15))
            .corporate_sofr()
            .build()
            .unwrap();

        let discount = create_sample_discount_curve();
        let discount_arc: Arc<dyn Curve> = Arc::new(discount);
        let forward = create_sample_forward_curve(discount_arc.clone());
        let settlement = date(2025, 6, 15);

        let calc = DiscountMarginCalculator::new(&forward, discount_arc.as_ref());

        // Should still calculate DM
        let price = calc.price_with_dm(&frn, 0.005, settlement);
        assert!(price > 90.0 && price < 110.0);
    }

    #[test]
    fn test_dm_with_floor() {
        // Create FRN with floor
        let frn = FloatingRateNote::builder()
            .cusip_unchecked("FLOORED01")
            .index(RateIndex::SOFR)
            .spread_bps(50)
            .floor(dec!(0.02)) // 2% floor
            .face_value(dec!(100))
            .maturity(date(2027, 6, 15))
            .issue_date(date(2025, 6, 15))
            .corporate_sofr()
            .build()
            .unwrap();

        let discount = create_sample_discount_curve();
        let discount_arc: Arc<dyn Curve> = Arc::new(discount);
        let forward = create_sample_forward_curve(discount_arc.clone());
        let settlement = date(2025, 6, 15);

        let calc = DiscountMarginCalculator::new(&forward, discount_arc.as_ref());

        // Should still calculate DM
        let price = calc.price_with_dm(&frn, 0.005, settlement);
        assert!(price > 90.0 && price < 110.0);
    }

    #[test]
    fn test_effective_duration() {
        let discount = create_sample_discount_curve();
        let discount_arc: Arc<dyn Curve> = Arc::new(discount);
        let forward = create_sample_forward_curve(discount_arc.clone());
        let frn = create_sample_frn();
        let settlement = date(2025, 6, 15);

        let calc = DiscountMarginCalculator::new(&forward, discount_arc.as_ref());
        let dm = Spread::new(dec!(50), SpreadType::DiscountMargin);

        let eff_dur = calc.effective_duration(&frn, dm, settlement, 0.01);

        // Effective duration should be positive for FRN
        assert!(eff_dur > Decimal::ZERO, "Effective duration should be positive");
    }
}
