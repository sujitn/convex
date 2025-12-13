//! Z-Spread (Zero-Volatility Spread) calculation.
//!
//! The Z-spread is the constant spread that, when added to all points on the
//! spot rate curve, makes the present value of a bond's cash flows equal to
//! its market price.
//!
//! # Example
//!
//! ```rust,ignore
//! use convex_spreads::ZSpreadCalculator;
//! use convex_curves::curves::ZeroCurve;
//!
//! let curve = // ... create spot curve
//! let calculator = ZSpreadCalculator::new(&curve);
//!
//! // Calculate Z-spread from price
//! let z_spread = calculator.calculate(&cash_flows, dec!(98.50), settlement)?;
//!
//! // Price with a given spread
//! let price = calculator.price_with_spread(&cash_flows, 0.02, settlement);
//!
//! // Calculate spread DV01
//! let dv01 = calculator.spread_dv01(&cash_flows, z_spread, settlement);
//! ```

use rust_decimal::prelude::*;
use rust_decimal::Decimal;

use convex_core::types::{CashFlow, Date, Spread, SpreadType};
use convex_curves::curves::ZeroCurve;
use convex_math::solvers::{brent, SolverConfig};

use crate::error::{SpreadError, SpreadResult};

/// Z-Spread calculator.
///
/// Calculates the zero-volatility spread (Z-spread). The Z-spread is the
/// constant spread that, when added to each point on the spot rate curve,
/// makes the present value of cash flows equal to the market price.
///
/// # Bloomberg Methodology
///
/// The calculation matches Bloomberg YAS:
/// ```text
/// Price = Σ CF_i × exp(-(r_i + z) × t_i)
/// ```
///
/// where:
/// - CF_i is the i-th cash flow
/// - r_i is the continuously compounded spot rate at time t_i
/// - z is the Z-spread (what we solve for)
/// - t_i is the time to the cash flow in years
#[derive(Debug, Clone)]
pub struct ZSpreadCalculator<'a> {
    curve: &'a ZeroCurve,
    config: SolverConfig,
}

impl<'a> ZSpreadCalculator<'a> {
    /// Creates a new Z-spread calculator.
    #[must_use]
    pub fn new(curve: &'a ZeroCurve) -> Self {
        Self {
            curve,
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

    /// Returns a reference to the underlying curve.
    pub fn curve(&self) -> &ZeroCurve {
        self.curve
    }

    /// Returns the solver configuration.
    pub fn solver_config(&self) -> &SolverConfig {
        &self.config
    }

    /// Calculates Z-spread from cash flows and dirty price.
    ///
    /// # Arguments
    ///
    /// * `cash_flows` - Vector of cash flows
    /// * `dirty_price` - Market dirty price (clean + accrued)
    /// * `settlement` - Settlement date
    ///
    /// # Returns
    ///
    /// The Z-spread as a `Spread` in basis points.
    pub fn calculate(
        &self,
        cash_flows: &[CashFlow],
        dirty_price: Decimal,
        settlement: Date,
    ) -> SpreadResult<Spread> {
        let target = dirty_price.to_f64().unwrap_or(100.0);

        let cf_data: Vec<(f64, f64)> = cash_flows
            .iter()
            .filter(|cf| cf.date() > settlement)
            .map(|cf| {
                let t = settlement.days_between(&cf.date()) as f64 / 365.0;
                let amount = cf.amount().to_f64().unwrap_or(0.0);
                (t, amount)
            })
            .collect();

        if cf_data.is_empty() {
            return Err(SpreadError::NoFutureCashFlows);
        }

        let spot_rates: Vec<f64> = cf_data
            .iter()
            .map(|(t, _)| self.spot_rate_at_time(*t))
            .collect();

        let objective = |z: f64| {
            let mut pv = 0.0;
            for (i, (t, amount)) in cf_data.iter().enumerate() {
                let df = (-(spot_rates[i] + z) * t).exp();
                pv += amount * df;
            }
            pv - target
        };

        let result = brent(objective, -0.05, 0.50, &self.config)
            .map_err(|_| SpreadError::convergence_failed(self.config.max_iterations))?;

        let z_spread_bps = (result.root * 10_000.0).round();
        Ok(Spread::new(
            Decimal::from_f64_retain(z_spread_bps).unwrap_or_default(),
            SpreadType::ZSpread,
        ))
    }

    /// Prices cash flows with a given Z-spread.
    ///
    /// # Arguments
    ///
    /// * `cash_flows` - Vector of cash flows
    /// * `z_spread` - Z-spread as a decimal (e.g., 0.02 for 200 bps)
    /// * `settlement` - Settlement date
    ///
    /// # Returns
    ///
    /// The dirty price.
    pub fn price_with_spread(
        &self,
        cash_flows: &[CashFlow],
        z_spread: f64,
        settlement: Date,
    ) -> f64 {
        let mut price = 0.0;

        for cf in cash_flows {
            if cf.date() <= settlement {
                continue;
            }

            let t = settlement.days_between(&cf.date()) as f64 / 365.0;
            let spot_rate = self.spot_rate_at_time(t);
            let df = (-(spot_rate + z_spread) * t).exp();

            price += cf.amount().to_f64().unwrap_or(0.0) * df;
        }

        price
    }

    /// Calculates spread DV01 (price sensitivity to 1bp spread change).
    pub fn spread_dv01(
        &self,
        cash_flows: &[CashFlow],
        z_spread: Spread,
        settlement: Date,
    ) -> Decimal {
        let spread_decimal = z_spread.as_decimal() / Decimal::from(10_000);
        let base_spread = spread_decimal.to_f64().unwrap_or(0.0);

        let base_price = self.price_with_spread(cash_flows, base_spread, settlement);
        let bumped_price = self.price_with_spread(cash_flows, base_spread + 0.0001, settlement);

        Decimal::from_f64_retain(base_price - bumped_price).unwrap_or(Decimal::ZERO)
    }

    /// Calculates spread duration (percentage price sensitivity).
    pub fn spread_duration(
        &self,
        cash_flows: &[CashFlow],
        z_spread: Spread,
        settlement: Date,
    ) -> Decimal {
        let spread_decimal = z_spread.as_decimal() / Decimal::from(10_000);
        let base_spread = spread_decimal.to_f64().unwrap_or(0.0);

        let base_price = self.price_with_spread(cash_flows, base_spread, settlement);

        if base_price <= 0.0 {
            return Decimal::ZERO;
        }

        let dv01 = self.spread_dv01(cash_flows, z_spread, settlement);
        dv01 / Decimal::from_f64_retain(base_price).unwrap_or(Decimal::ONE) * Decimal::from(10_000)
    }

    fn spot_rate_at_time(&self, t: f64) -> f64 {
        if t <= 0.0 {
            return 0.0;
        }

        let days = (t * 365.0).round() as i64;
        let target_date = self.curve.reference_date() + days;

        self.curve
            .zero_rate_at(target_date)
            .map(|r| r.to_f64().unwrap_or(0.0))
            .unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_curves::prelude::{InterpolationMethod, ZeroCurveBuilder};
    use rust_decimal_macros::dec;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    fn create_test_curve() -> ZeroCurve {
        ZeroCurveBuilder::new()
            .reference_date(date(2025, 1, 15))
            .add_rate(date(2025, 4, 15), dec!(0.04))
            .add_rate(date(2025, 7, 15), dec!(0.042))
            .add_rate(date(2026, 1, 15), dec!(0.045))
            .add_rate(date(2027, 1, 15), dec!(0.047))
            .add_rate(date(2028, 1, 15), dec!(0.048))
            .add_rate(date(2030, 1, 15), dec!(0.049))
            .add_rate(date(2035, 1, 15), dec!(0.050))
            .interpolation(InterpolationMethod::Linear)
            .build()
            .unwrap()
    }

    fn create_test_cash_flows(settlement: Date) -> Vec<CashFlow> {
        let coupon = dec!(2.5);
        let face = dec!(100);

        let mut flows = Vec::new();
        let coupon_dates = [
            date(2025, 6, 15),
            date(2025, 12, 15),
            date(2026, 6, 15),
            date(2026, 12, 15),
            date(2027, 6, 15),
        ];

        for (i, &cf_date) in coupon_dates.iter().enumerate() {
            if cf_date <= settlement {
                continue;
            }
            if i == coupon_dates.len() - 1 {
                flows.push(CashFlow::final_payment(cf_date, coupon, face));
            } else {
                flows.push(CashFlow::coupon(cf_date, coupon));
            }
        }

        flows
    }

    #[test]
    fn test_z_spread_calculator_creation() {
        let curve = create_test_curve();
        let calc = ZSpreadCalculator::new(&curve)
            .with_tolerance(1e-8)
            .with_max_iterations(50);

        assert!(calc.config.tolerance < 1e-7);
        assert_eq!(calc.config.max_iterations, 50);
    }

    #[test]
    fn test_price_with_spread() {
        let curve = create_test_curve();
        let calc = ZSpreadCalculator::new(&curve);

        let settlement = date(2025, 1, 15);
        let cash_flows = create_test_cash_flows(settlement);

        let price_zero = calc.price_with_spread(&cash_flows, 0.0, settlement);
        assert!(price_zero > 90.0 && price_zero < 120.0);

        let price_200bps = calc.price_with_spread(&cash_flows, 0.02, settlement);
        assert!(price_200bps < price_zero);

        let price_neg = calc.price_with_spread(&cash_flows, -0.01, settlement);
        assert!(price_neg > price_zero);
    }

    #[test]
    fn test_z_spread_calculation() {
        let curve = create_test_curve();
        let calc = ZSpreadCalculator::new(&curve);

        let settlement = date(2025, 1, 15);
        let cash_flows = create_test_cash_flows(settlement);

        let price_at_200bps = calc.price_with_spread(&cash_flows, 0.02, settlement);

        let result = calc
            .calculate(
                &cash_flows,
                Decimal::from_f64_retain(price_at_200bps).unwrap(),
                settlement,
            )
            .unwrap();

        let spread_bps = result.as_bps().to_f64().unwrap();
        assert!(
            (spread_bps - 200.0).abs() < 1.0,
            "Expected ~200 bps, got {} bps",
            spread_bps
        );
    }

    #[test]
    fn test_spread_dv01() {
        let curve = create_test_curve();
        let calc = ZSpreadCalculator::new(&curve);

        let settlement = date(2025, 1, 15);
        let cash_flows = create_test_cash_flows(settlement);

        let z_spread = Spread::new(dec!(200), SpreadType::ZSpread);

        let dv01 = calc.spread_dv01(&cash_flows, z_spread, settlement);

        assert!(dv01 > Decimal::ZERO);
        assert!(dv01 < dec!(0.1));
    }

    #[test]
    fn test_spread_duration() {
        let curve = create_test_curve();
        let calc = ZSpreadCalculator::new(&curve);

        let settlement = date(2025, 1, 15);
        let cash_flows = create_test_cash_flows(settlement);

        let z_spread = Spread::new(dec!(200), SpreadType::ZSpread);

        let duration = calc.spread_duration(&cash_flows, z_spread, settlement);

        assert!(duration > Decimal::ZERO);
        assert!(duration < dec!(10));
    }

    #[test]
    fn test_roundtrip() {
        let curve = create_test_curve();
        let calc = ZSpreadCalculator::new(&curve);

        let settlement = date(2025, 1, 15);
        let cash_flows = create_test_cash_flows(settlement);

        for spread_bps in [50.0, 100.0, 200.0, 300.0, 400.0] {
            let spread = spread_bps / 10_000.0;
            let price = calc.price_with_spread(&cash_flows, spread, settlement);

            let calculated_spread = calc
                .calculate(
                    &cash_flows,
                    Decimal::from_f64_retain(price).unwrap(),
                    settlement,
                )
                .unwrap();

            let calculated_bps = calculated_spread.as_bps().to_f64().unwrap();
            assert!(
                (calculated_bps - spread_bps).abs() < 0.5,
                "Spread {}: expected {} bps, got {} bps",
                spread_bps,
                spread_bps,
                calculated_bps
            );
        }
    }

    #[test]
    fn test_empty_cash_flows() {
        let curve = create_test_curve();
        let calc = ZSpreadCalculator::new(&curve);

        let settlement = date(2030, 1, 15);
        let cash_flows = create_test_cash_flows(date(2025, 1, 15));

        let result = calc.calculate(&cash_flows, dec!(100), settlement);
        assert!(result.is_err());
    }
}
