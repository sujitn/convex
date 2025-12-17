//! Discount Margin (DM) calculation for Floating Rate Notes.
//!
//! The discount margin is the spread over forward rates that, when added to
//! projected coupons and used for discounting, makes the present value of
//! an FRN's cash flows equal to its market price.

use rust_decimal::prelude::*;
use rust_decimal::Decimal;

use convex_bonds::instruments::FloatingRateNote;
use convex_bonds::traits::Bond;
use convex_core::types::{Date, Spread, SpreadType};
use convex_curves::curves::ForwardCurve;
use convex_curves::traits::Curve;
use convex_math::solvers::{brent, SolverConfig};

use crate::error::{AnalyticsError, AnalyticsResult};

/// Discount Margin calculator for floating rate notes.
#[derive(Debug)]
pub struct DiscountMarginCalculator<'a, C: Curve + ?Sized> {
    forward_curve: &'a ForwardCurve,
    discount_curve: &'a C,
    config: SolverConfig,
}

impl<'a, C: Curve + ?Sized> DiscountMarginCalculator<'a, C> {
    /// Creates a new Discount Margin calculator.
    #[must_use]
    pub fn new(forward_curve: &'a ForwardCurve, discount_curve: &'a C) -> Self {
        Self {
            forward_curve,
            discount_curve,
            config: SolverConfig::new(1e-10, 100),
        }
    }

    /// Sets the solver tolerance.
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
    pub fn calculate(
        &self,
        frn: &FloatingRateNote,
        dirty_price: Decimal,
        settlement: Date,
    ) -> AnalyticsResult<Spread> {
        let maturity = frn
            .maturity()
            .ok_or_else(|| AnalyticsError::InvalidInput("FRN has no maturity date".to_string()))?;

        if settlement >= maturity {
            return Err(AnalyticsError::InvalidSettlement {
                settlement: settlement.to_string(),
                maturity: maturity.to_string(),
            });
        }

        let target_price = dirty_price.to_f64().unwrap_or(100.0);

        let objective = |dm: f64| self.price_with_dm(frn, dm, settlement) - target_price;

        let result = brent(objective, -0.05, 0.20, &self.config).map_err(|_| {
            AnalyticsError::SolverConvergenceFailed {
                solver: "DM Brent".to_string(),
                iterations: self.config.max_iterations,
                residual: 0.0,
            }
        })?;

        let dm_bps = (result.root * 10_000.0).round();
        Ok(Spread::new(
            Decimal::from_f64_retain(dm_bps).unwrap_or_default(),
            SpreadType::DiscountMargin,
        ))
    }

    /// Prices an FRN given a discount margin.
    pub fn price_with_dm(&self, frn: &FloatingRateNote, dm: f64, settlement: Date) -> f64 {
        let Some(maturity) = frn.maturity() else {
            return 0.0;
        };

        if settlement >= maturity {
            return 0.0;
        }

        let face_value = frn.face_value().to_f64().unwrap_or(100.0);
        let quoted_spread = frn.spread_decimal().to_f64().unwrap_or(0.0);
        let ref_date = self.forward_curve.reference_date();
        let mut price = 0.0;

        let cash_flows = frn.cash_flows(settlement);

        if cash_flows.is_empty() {
            return 0.0;
        }

        for cf in &cash_flows {
            if cf.date <= settlement {
                continue;
            }

            let years_to_cf = settlement.days_between(&cf.date) as f64 / 365.0;

            let df = self
                .discount_curve
                .discount_factor(years_to_cf)
                .unwrap_or(1.0);

            let adjusted_df = df * (-dm * years_to_cf).exp();

            let cf_amount = if cf.is_principal() {
                let coupon_amount =
                    if let (Some(start), Some(end)) = (cf.accrual_start, cf.accrual_end) {
                        let t1 = ref_date.days_between(&start) as f64 / 365.0;
                        let period_years = start.days_between(&end) as f64 / 365.0;

                        let fwd_rate = self.forward_curve.forward_rate_at(t1).unwrap_or(0.05);

                        let coupon_rate = fwd_rate + quoted_spread;
                        let effective_rate = frn.effective_rate(
                            Decimal::from_f64_retain(coupon_rate).unwrap_or(Decimal::ZERO),
                        );

                        face_value * effective_rate.to_f64().unwrap_or(0.0) * period_years
                    } else {
                        cf.amount.to_f64().unwrap_or(0.0) - face_value
                    };

                coupon_amount + face_value
            } else if let (Some(start), Some(end)) = (cf.accrual_start, cf.accrual_end) {
                let t1 = ref_date.days_between(&start) as f64 / 365.0;
                let period_years = start.days_between(&end) as f64 / 365.0;

                let fwd_rate = self.forward_curve.forward_rate_at(t1).unwrap_or(0.05);

                let coupon_rate = fwd_rate + quoted_spread;
                let effective_rate = frn
                    .effective_rate(Decimal::from_f64_retain(coupon_rate).unwrap_or(Decimal::ZERO));

                face_value * effective_rate.to_f64().unwrap_or(0.0) * period_years
            } else {
                cf.amount.to_f64().unwrap_or(0.0)
            };

            price += cf_amount * adjusted_df;
        }

        price / face_value * 100.0
    }

    /// Calculates the spread DV01.
    pub fn spread_dv01(&self, frn: &FloatingRateNote, dm: Spread, settlement: Date) -> Decimal {
        let base_dm = dm.as_decimal().to_f64().unwrap_or(0.0) / 10_000.0;

        let base_price = self.price_with_dm(frn, base_dm, settlement);
        let bumped_price = self.price_with_dm(frn, base_dm + 0.0001, settlement);

        Decimal::from_f64_retain(base_price - bumped_price).unwrap_or(Decimal::ZERO)
    }

    /// Calculates spread duration.
    pub fn spread_duration(&self, frn: &FloatingRateNote, dm: Spread, settlement: Date) -> Decimal {
        let base_dm = dm.as_decimal().to_f64().unwrap_or(0.0) / 10_000.0;
        let base_price = self.price_with_dm(frn, base_dm, settlement);

        if base_price <= 0.0 {
            return Decimal::ZERO;
        }

        let dv01 = self.spread_dv01(frn, dm, settlement);
        dv01 / Decimal::from_f64_retain(base_price).unwrap_or(Decimal::ONE) * Decimal::from(10_000)
    }

    /// Calculates effective spread duration.
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

        let price_up = self.price_with_dm(frn, base_dm + rate_shift, settlement);
        let price_down = self.price_with_dm(frn, base_dm - rate_shift, settlement);

        let duration = (price_down - price_up) / (2.0 * base_price * rate_shift);
        Decimal::from_f64_retain(duration).unwrap_or(Decimal::ZERO)
    }
}

/// Simple margin calculation (flat forward assumption).
pub fn simple_margin(
    frn: &FloatingRateNote,
    dirty_price: Decimal,
    current_index: Decimal,
    settlement: Date,
) -> Spread {
    let Some(maturity) = frn.maturity() else {
        return Spread::new(Decimal::ZERO, SpreadType::DiscountMargin);
    };

    let remaining_years = settlement.days_between(&maturity) as f64 / 365.0;

    if remaining_years <= 0.0 || dirty_price <= Decimal::ZERO {
        return Spread::new(Decimal::ZERO, SpreadType::DiscountMargin);
    }

    let face = frn.face_value();
    let price = dirty_price;
    let quoted_spread = frn.spread_decimal();

    let coupon_rate = current_index + quoted_spread;

    let annual_coupon = face * coupon_rate;
    let current_yield = annual_coupon / price;

    let redemption_effect = (face - price)
        / (price * Decimal::from_f64_retain(remaining_years).unwrap_or(Decimal::ONE));

    let simple_margin = current_yield + redemption_effect - current_index;

    let margin_bps = (simple_margin * Decimal::from(10_000)).round();

    Spread::new(margin_bps, SpreadType::DiscountMargin)
}

/// Calculates the Z-DM (zero discount margin) for an FRN.
pub fn z_discount_margin<C: Curve + ?Sized>(
    frn: &FloatingRateNote,
    dirty_price: Decimal,
    forward_curve: &ForwardCurve,
    discount_curve: &C,
    settlement: Date,
) -> AnalyticsResult<Spread> {
    DiscountMarginCalculator::new(forward_curve, discount_curve).calculate(
        frn,
        dirty_price,
        settlement,
    )
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
            .spread_bps(50)
            .face_value(dec!(100))
            .maturity(date(2027, 6, 15))
            .issue_date(date(2025, 6, 15))
            .corporate_sofr()
            .build()
            .unwrap()
    }

    fn create_sample_discount_curve() -> impl Curve {
        DiscountCurveBuilder::new(date(2025, 6, 15))
            .add_pillar(0.25, 0.9875)
            .add_pillar(0.5, 0.975)
            .add_pillar(1.0, 0.95)
            .add_pillar(2.0, 0.90)
            .add_pillar(5.0, 0.78)
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

        let price_zero_dm = calc.price_with_dm(&frn, 0.0, settlement);
        assert!(
            price_zero_dm > 90.0 && price_zero_dm < 110.0,
            "Price {} out of range",
            price_zero_dm
        );

        let price_50bps = calc.price_with_dm(&frn, 0.0050, settlement);
        assert!(price_50bps < price_zero_dm, "Price with DM should be lower");
    }

    #[test]
    fn test_simple_margin() {
        let frn = create_sample_frn();
        let settlement = date(2025, 6, 15);
        let dirty_price = dec!(99.50);
        let current_index = dec!(0.0525);

        let margin = simple_margin(&frn, dirty_price, current_index, settlement);

        assert!(
            margin.as_bps() > Decimal::ZERO,
            "Margin should be positive for discount bond"
        );
    }

    #[test]
    fn test_simple_margin_par() {
        let frn = create_sample_frn();
        let settlement = date(2025, 6, 15);
        let dirty_price = dec!(100.0);
        let current_index = dec!(0.05);

        let margin = simple_margin(&frn, dirty_price, current_index, settlement);

        let margin_bps = margin.as_bps().to_f64().unwrap();
        assert!(
            (margin_bps - 50.0).abs() < 5.0,
            "At par, simple margin should be close to quoted spread: {} bps",
            margin_bps
        );
    }
}
