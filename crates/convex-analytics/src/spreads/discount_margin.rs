//! Discount Margin (DM) calculation for Floating Rate Notes.
//!
//! The discount margin is the spread over forward rates that, when added to
//! projected coupons and used for discounting, makes the present value of
//! an FRN's cash flows equal to its market price.

use rust_decimal::prelude::*;
use rust_decimal::Decimal;

use convex_bonds::instruments::{CallableFloatingRateNote, FloatingRateNote};
use convex_bonds::traits::Bond;
use convex_core::types::{Date, Spread, SpreadType};
use convex_curves::curves::ForwardCurve;
use convex_curves::RateCurveDyn;
use convex_math::solvers::{brent, SolverConfig};

use crate::error::{AnalyticsError, AnalyticsResult};

/// Discount Margin calculator for floating rate notes.
pub struct DiscountMarginCalculator<'a, C: RateCurveDyn + ?Sized> {
    forward_curve: &'a ForwardCurve,
    discount_curve: &'a C,
    config: SolverConfig,
    in_progress_coupon: Option<Box<dyn Fn(Date, Date) -> f64 + 'a>>,
}

impl<'a, C: RateCurveDyn + ?Sized> DiscountMarginCalculator<'a, C> {
    #[must_use]
    pub fn new(forward_curve: &'a ForwardCurve, discount_curve: &'a C) -> Self {
        Self {
            forward_curve,
            discount_curve,
            config: SolverConfig::new(1e-10, 100),
            in_progress_coupon: None,
        }
    }

    #[must_use]
    pub fn with_tolerance(mut self, tolerance: f64) -> Self {
        self.config = SolverConfig::new(tolerance, self.config.max_iterations);
        self
    }

    #[must_use]
    pub fn with_max_iterations(mut self, max_iterations: u32) -> Self {
        self.config = SolverConfig::new(self.config.tolerance, max_iterations);
        self
    }

    /// Override the in-progress coupon amount for periods straddling
    /// settlement. The default uses the FRN's `current_rate`, which only
    /// reflects the last reset; pass an ARRC compound-in-arrears closure
    /// here to drive in-progress coupons off real fixings + a projection
    /// curve.
    #[must_use]
    pub fn with_in_progress_coupon<F>(mut self, f: F) -> Self
    where
        F: Fn(Date, Date) -> f64 + 'a,
    {
        self.in_progress_coupon = Some(Box::new(f));
        self
    }

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

        let dm_bps = result.root * 10_000.0;
        Ok(Spread::new(
            Decimal::from_f64_retain(dm_bps).unwrap_or_default(),
            SpreadType::DiscountMargin,
        ))
    }

    pub fn price_with_dm(&self, frn: &FloatingRateNote, dm: f64, settlement: Date) -> f64 {
        let cash_flows = frn.cash_flows(settlement);
        self.price_with_dm_for_flows(frn, &cash_flows, dm, settlement, None)
    }

    /// PV of an arbitrary cash-flow slice at `dm`. Pass `redemption` to
    /// override the terminal principal (workout-bullet); `None` means use
    /// each cash flow's own principal.
    fn price_with_dm_for_flows(
        &self,
        frn: &FloatingRateNote,
        cash_flows: &[convex_bonds::traits::BondCashFlow],
        dm: f64,
        settlement: Date,
        redemption: Option<f64>,
    ) -> f64 {
        let Some(maturity) = frn.maturity() else {
            return 0.0;
        };
        if settlement >= maturity {
            return 0.0;
        }

        let face_value = frn.face_value().to_f64().unwrap_or(100.0);
        // Each curve's tenors anchor to its own reference date.
        let disc_ref = self.discount_curve.reference_date();
        let fwd_ref = self.forward_curve.reference_date();
        let day_count = frn.day_count().to_day_count();

        if cash_flows.is_empty() {
            return 0.0;
        }

        let t_settle = disc_ref.days_between(&settlement) as f64 / 365.0;
        let df_settle = match self.discount_curve.discount_factor(t_settle.max(0.0)) {
            Ok(v) if v > 0.0 => v,
            _ => return 0.0,
        };

        let mut price = 0.0;
        for cf in cash_flows {
            if cf.date <= settlement {
                continue;
            }
            let t_cf = disc_ref.days_between(&cf.date) as f64 / 365.0;
            let dt = settlement.days_between(&cf.date) as f64 / 365.0;
            let Ok(df_cf) = self.discount_curve.discount_factor(t_cf) else {
                return 0.0;
            };
            let adjusted_df = (df_cf / df_settle) * (-dm * dt).exp();

            let coupon = match (cf.accrual_start, cf.accrual_end) {
                // Future period — project via the forward curve.
                (Some(start), Some(end)) if start >= settlement => {
                    let yf = day_count
                        .period_year_fraction(start, end, start, end)
                        .to_f64()
                        .unwrap_or(0.0);
                    if yf <= 0.0 {
                        return 0.0;
                    }
                    // Forward simple rate consistent with the bond's own day
                    // count: (DF(start)/DF(end) - 1) / yf_bond. A different
                    // span here silently scales the projected coupon.
                    let t_start = fwd_ref.days_between(&start) as f64 / 365.0;
                    let t_end = fwd_ref.days_between(&end) as f64 / 365.0;
                    let (Ok(df_s), Ok(df_e)) = (
                        self.forward_curve
                            .discount_curve()
                            .discount_factor(t_start.max(0.0)),
                        self.forward_curve.discount_curve().discount_factor(t_end),
                    ) else {
                        return 0.0;
                    };
                    if df_e <= 0.0 {
                        return 0.0;
                    }
                    let simple_fwd = (df_s / df_e - 1.0) / yf;
                    // effective_rate adds the bond's spread internally and
                    // applies any cap/floor.
                    let rate = frn
                        .effective_rate(Decimal::from_f64_retain(simple_fwd).unwrap_or_default());
                    face_value * rate.to_f64().unwrap_or(0.0) * yf
                }
                (Some(start), Some(end)) => {
                    if let Some(f) = self.in_progress_coupon.as_deref() {
                        f(start, end)
                    } else {
                        let raw = cf.amount.to_f64().unwrap_or(0.0);
                        if cf.is_principal() {
                            raw - face_value
                        } else {
                            raw
                        }
                    }
                }
                _ => {
                    let raw = cf.amount.to_f64().unwrap_or(0.0);
                    if cf.is_principal() {
                        raw - face_value
                    } else {
                        raw
                    }
                }
            };

            let principal = if cf.is_principal() {
                redemption.unwrap_or(face_value)
            } else {
                0.0
            };
            price += (coupon + principal) * adjusted_df;
        }

        price / face_value * 100.0
    }

    /// Minimum DM across DM-to-each-call (using each entry's call price)
    /// and plain DM-to-maturity. Returns `(dm, workout_date)`.
    pub fn discount_margin_to_worst(
        &self,
        cfrn: &CallableFloatingRateNote,
        dirty_price: Decimal,
        settlement: Date,
    ) -> AnalyticsResult<(Spread, Date)> {
        let frn = cfrn.base_frn();
        let maturity = frn
            .maturity()
            .ok_or_else(|| AnalyticsError::InvalidInput("FRN has no maturity date".to_string()))?;

        let dm_mat = self.calculate(frn, dirty_price, settlement)?;
        let mut worst_bps = dm_mat.as_bps();
        let mut worst_date = maturity;

        for call_date in cfrn.all_workout_dates(settlement) {
            let Some(call_price) = cfrn.call_price_on(call_date) else {
                continue;
            };
            let dm =
                self.calculate_to_workout(frn, dirty_price, settlement, call_date, call_price)?;
            if dm.as_bps() < worst_bps {
                worst_bps = dm.as_bps();
                worst_date = call_date;
            }
        }

        Ok((
            Spread::new(worst_bps, SpreadType::DiscountMargin),
            worst_date,
        ))
    }

    /// Solves for the DM that prices the bond to `dirty_price` using a
    /// truncated, workout-bullet cash flow set: cash flows up to
    /// `workout_date` plus a redemption of `call_price` (per 100 face) at
    /// `workout_date`. Mirrors `CallableBond::yield_to_call_date` for FRNs.
    pub fn calculate_to_workout(
        &self,
        frn: &FloatingRateNote,
        dirty_price: Decimal,
        settlement: Date,
        workout_date: Date,
        call_price: f64,
    ) -> AnalyticsResult<Spread> {
        if workout_date <= settlement {
            return Err(AnalyticsError::InvalidInput(
                "workout_date must be after settlement".into(),
            ));
        }
        // Past-maturity workout would append a second principal flow on top
        // of the bond's own maturity principal — silently double-paid.
        if let Some(maturity) = frn.maturity() {
            if workout_date > maturity {
                return Err(AnalyticsError::InvalidInput(
                    "workout_date must be on or before maturity".into(),
                ));
            }
        }
        let face_value = frn.face_value().to_f64().unwrap_or(100.0);
        let workout_redemption = call_price / 100.0 * face_value;
        let workout_flows = workout_cash_flows(frn, settlement, workout_date, workout_redemption);
        if workout_flows.is_empty() {
            return Err(AnalyticsError::InvalidInput(
                "no cash flows up to workout date".into(),
            ));
        }
        let target_price = dirty_price.to_f64().unwrap_or(100.0);
        let objective = |dm: f64| {
            self.price_with_dm_for_flows(
                frn,
                &workout_flows,
                dm,
                settlement,
                Some(workout_redemption),
            ) - target_price
        };
        let result = brent(objective, -0.05, 0.20, &self.config).map_err(|_| {
            AnalyticsError::SolverConvergenceFailed {
                solver: "DM-to-workout Brent".to_string(),
                iterations: self.config.max_iterations,
                residual: 0.0,
            }
        })?;
        let dm_bps = result.root * 10_000.0;
        Ok(Spread::new(
            Decimal::from_f64_retain(dm_bps).unwrap_or_default(),
            SpreadType::DiscountMargin,
        ))
    }

    pub fn spread_dv01(&self, frn: &FloatingRateNote, dm: Spread, settlement: Date) -> Decimal {
        let base_dm = dm.as_decimal().to_f64().unwrap_or(0.0) / 10_000.0;

        let base_price = self.price_with_dm(frn, base_dm, settlement);
        let bumped_price = self.price_with_dm(frn, base_dm + 0.0001, settlement);

        Decimal::from_f64_retain(base_price - bumped_price).unwrap_or(Decimal::ZERO)
    }

    pub fn spread_duration(&self, frn: &FloatingRateNote, dm: Spread, settlement: Date) -> Decimal {
        let base_dm = dm.as_decimal().to_f64().unwrap_or(0.0) / 10_000.0;
        let base_price = self.price_with_dm(frn, base_dm, settlement);

        if base_price <= 0.0 {
            return Decimal::ZERO;
        }

        let dv01 = self.spread_dv01(frn, dm, settlement);
        dv01 / Decimal::from_f64_retain(base_price).unwrap_or(Decimal::ONE) * Decimal::from(10_000)
    }

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

/// Build a workout-bullet cash flow vector for an FRN: future flows up to
/// `workout_date`, with the principal at the terminal flow set to
/// `redemption_amount` (in face-value units, not per-100). If the workout
/// date doesn't coincide with a coupon date, an extra principal-only flow is
/// appended; otherwise the existing terminal flow's `flow_type` and amount are
/// adjusted so it carries the call redemption alongside its coupon.
fn workout_cash_flows(
    frn: &FloatingRateNote,
    settlement: Date,
    workout_date: Date,
    redemption_amount: f64,
) -> Vec<convex_bonds::traits::BondCashFlow> {
    use convex_bonds::traits::{BondCashFlow, CashFlowType};
    let mut flows = frn.cash_flows(settlement);
    flows.retain(|cf| cf.date <= workout_date);
    let face = frn.face_value();
    let redemption_dec = Decimal::from_f64_retain(redemption_amount).unwrap_or(face);

    if let Some(last) = flows.last_mut() {
        if last.date == workout_date {
            // Replace any embedded principal in the terminal flow with the
            // call redemption, and force the type to coupon+principal.
            let coupon = if last.is_principal() {
                last.amount.saturating_sub(face)
            } else {
                last.amount
            };
            last.amount = coupon + redemption_dec;
            last.flow_type = CashFlowType::CouponAndPrincipal;
            return flows;
        }
    }
    // No flow at the workout date — append a redemption-only flow.
    flows.push(BondCashFlow::principal(workout_date, redemption_dec));
    flows
}

/// Calculates the Z-DM (zero discount margin) for an FRN.
pub fn z_discount_margin<C: RateCurveDyn + ?Sized>(
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
    use convex_curves::InterpolationMethod;
    use rust_decimal_macros::dec;
    use std::sync::Arc;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    fn create_sample_frn() -> FloatingRateNote {
        FloatingRateNote::builder()
            .cusip_unchecked("TEST12345")
            .index(RateIndex::Sofr)
            .spread_bps(50)
            .face_value(dec!(100))
            .maturity(date(2027, 6, 15))
            .issue_date(date(2025, 6, 15))
            .corporate_sofr()
            .build()
            .unwrap()
    }

    fn create_sample_discount_curve() -> impl RateCurveDyn {
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

    fn create_sample_forward_curve(discount_curve: Arc<dyn RateCurveDyn>) -> ForwardCurve {
        ForwardCurve::from_months(discount_curve, 3)
    }

    #[test]
    fn test_calculator_creation() {
        let discount = create_sample_discount_curve();
        let discount_arc: Arc<dyn RateCurveDyn> = Arc::new(discount);
        let forward = create_sample_forward_curve(discount_arc.clone());

        let _calc = DiscountMarginCalculator::new(&forward, discount_arc.as_ref())
            .with_tolerance(1e-8)
            .with_max_iterations(50);
    }

    #[test]
    fn test_in_progress_coupon_override_is_applied() {
        // Settlement mid-period (between 2025-06-15 and 2025-09-15) so
        // there's exactly one in-progress flow. A constant override moves
        // the in-progress coupon to a known number; pricing at dm=0 should
        // reflect the change relative to the default.
        use std::cell::Cell;
        let discount = create_sample_discount_curve();
        let discount_arc: Arc<dyn RateCurveDyn> = Arc::new(discount);
        let forward = create_sample_forward_curve(discount_arc.clone());
        let frn = create_sample_frn();
        let settlement = date(2025, 8, 1);

        let baseline = DiscountMarginCalculator::new(&forward, discount_arc.as_ref())
            .price_with_dm(&frn, 0.0, settlement);

        let calls = Cell::new(0u32);
        let bumped = DiscountMarginCalculator::new(&forward, discount_arc.as_ref())
            .with_in_progress_coupon(|_start, _end| {
                calls.set(calls.get() + 1);
                5.0
            })
            .price_with_dm(&frn, 0.0, settlement);

        assert!(calls.get() >= 1, "override never invoked");
        assert!(
            (bumped - baseline).abs() > 1e-6,
            "override had no effect: baseline={baseline} bumped={bumped}"
        );
    }

    #[test]
    fn test_price_with_dm_par() {
        let discount = create_sample_discount_curve();
        let discount_arc: Arc<dyn RateCurveDyn> = Arc::new(discount);
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
    fn test_callable_frn_dm_to_worst_premium_bond() {
        // Premium-priced callable FRN: investor's worst case is being called
        // at par on the first call date — DM-to-call should be lower than
        // DM-to-maturity, and the chosen workout should be the first call.
        use convex_bonds::instruments::{CallableFloatingRateNote, FloatingRateNote};
        use convex_bonds::types::{CallEntry, CallSchedule, CallType, RateIndex};
        let discount = create_sample_discount_curve();
        let discount_arc: Arc<dyn RateCurveDyn> = Arc::new(discount);
        let forward = create_sample_forward_curve(discount_arc.clone());

        let frn = FloatingRateNote::builder()
            .cusip_unchecked("CFRNTEST1")
            .index(RateIndex::Sofr)
            .spread_bps(125)
            .face_value(dec!(100))
            .maturity(date(2030, 6, 15))
            .issue_date(date(2025, 6, 15))
            .corporate_sofr()
            .build()
            .unwrap();

        let schedule = CallSchedule::new(CallType::Bermudan)
            .with_entry(CallEntry::new(date(2027, 6, 15), 100.0))
            .with_entry(CallEntry::new(date(2028, 6, 15), 100.0));
        let cfrn = CallableFloatingRateNote::new(frn.clone(), schedule);

        let calc = DiscountMarginCalculator::new(&forward, discount_arc.as_ref());
        let settlement = date(2026, 6, 15);
        let dirty_price = dec!(101.0); // premium

        let dm_to_mat = calc.calculate(&frn, dirty_price, settlement).unwrap();
        let (dm_worst, worst_date) = calc
            .discount_margin_to_worst(&cfrn, dirty_price, settlement)
            .unwrap();

        // Premium bond → call shortens duration, DM-to-call < DM-to-maturity.
        assert!(
            dm_worst.as_bps() <= dm_to_mat.as_bps(),
            "DM-to-worst {} should be <= DM-to-maturity {}",
            dm_worst.as_bps(),
            dm_to_mat.as_bps()
        );
        // First call dates exist between 2027-06 and 2028-06; worst should be one of them.
        assert!(
            worst_date == date(2027, 6, 15) || worst_date == date(2028, 6, 15),
            "expected a call workout, got {worst_date}"
        );
    }

    #[test]
    fn test_callable_frn_dm_to_worst_discount_bond() {
        // Discount-priced callable FRN: investor expects to hold to maturity
        // (call is OTM for issuer), so DM-to-worst should equal DM-to-maturity
        // and the workout date should be the maturity itself.
        use convex_bonds::instruments::{CallableFloatingRateNote, FloatingRateNote};
        use convex_bonds::types::{CallEntry, CallSchedule, CallType, RateIndex};
        let discount = create_sample_discount_curve();
        let discount_arc: Arc<dyn RateCurveDyn> = Arc::new(discount);
        let forward = create_sample_forward_curve(discount_arc.clone());

        let frn = FloatingRateNote::builder()
            .cusip_unchecked("CFRNTEST2")
            .index(RateIndex::Sofr)
            .spread_bps(50)
            .face_value(dec!(100))
            .maturity(date(2030, 6, 15))
            .issue_date(date(2025, 6, 15))
            .corporate_sofr()
            .build()
            .unwrap();

        let maturity = frn.maturity().unwrap();
        let schedule = CallSchedule::new(CallType::Bermudan)
            .with_entry(CallEntry::new(date(2028, 6, 15), 100.0));
        let cfrn = CallableFloatingRateNote::new(frn.clone(), schedule);

        let calc = DiscountMarginCalculator::new(&forward, discount_arc.as_ref());
        let settlement = date(2026, 6, 15);
        let dirty_price = dec!(98.5);

        let dm_to_mat = calc.calculate(&frn, dirty_price, settlement).unwrap();
        let (dm_worst, worst_date) = calc
            .discount_margin_to_worst(&cfrn, dirty_price, settlement)
            .unwrap();

        // Discount bond → calling at par makes DM-to-call higher; min stays
        // on the maturity branch.
        assert_eq!(worst_date, maturity);
        assert!(
            (dm_worst.as_bps() - dm_to_mat.as_bps()).abs() <= Decimal::from(1),
            "expected DM-to-worst ≈ DM-to-maturity, got {} vs {}",
            dm_worst.as_bps(),
            dm_to_mat.as_bps()
        );
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
