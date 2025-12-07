//! Risk metrics for bonds.

use rust_decimal::Decimal;

use convex_core::types::{Date, Price};

use crate::error::BondResult;
use crate::instruments::{Bond, FixedBond};
use crate::pricing::BondPricer;

/// Duration calculation results.
#[derive(Debug, Clone)]
pub struct DurationResult {
    /// Macaulay duration (in years).
    pub macaulay: Decimal,
    /// Modified duration.
    pub modified: Decimal,
    /// Dollar duration (DV01 * 100).
    pub dollar: Decimal,
}

/// Comprehensive risk metrics for a bond.
#[derive(Debug, Clone)]
pub struct RiskMetrics {
    /// Duration measures.
    pub duration: DurationResult,
    /// Convexity.
    pub convexity: Decimal,
    /// DV01 (dollar value of 1 basis point).
    pub dv01: Decimal,
    /// BPV (basis point value) - same as DV01.
    pub bpv: Decimal,
}

/// Risk calculator for bonds.
pub struct RiskCalculator;

impl RiskCalculator {
    /// Calculates all risk metrics for a bond.
    ///
    /// # Arguments
    ///
    /// * `bond` - The bond
    /// * `yield_value` - Current yield (as decimal)
    /// * `settlement` - Settlement date
    pub fn calculate(
        bond: &FixedBond,
        yield_value: Decimal,
        settlement: Date,
    ) -> BondResult<RiskMetrics> {
        let duration = Self::duration(bond, yield_value, settlement)?;
        let convexity = Self::convexity(bond, yield_value, settlement)?;
        let dv01 = Self::dv01(bond, yield_value, settlement)?;

        Ok(RiskMetrics {
            duration,
            convexity,
            dv01,
            bpv: dv01,
        })
    }

    /// Calculates duration measures.
    pub fn duration(
        bond: &FixedBond,
        yield_value: Decimal,
        settlement: Date,
    ) -> BondResult<DurationResult> {
        let y = yield_value.to_string().parse::<f64>().unwrap_or(0.05);
        let freq = f64::from(bond.frequency().periods_per_year());

        let schedule = crate::cashflows::CashFlowGenerator::generate(bond, settlement)?;

        let mut pv = 0.0;
        let mut weighted_time = 0.0;

        for cf in schedule.iter() {
            let t = settlement.days_between(&cf.date()) as f64 / 365.0;
            let df = if freq > 0.0 {
                1.0 / (1.0 + y / freq).powf(freq * t)
            } else {
                (-y * t).exp()
            };
            let amount = cf.amount().to_string().parse::<f64>().unwrap_or(0.0);
            let cf_pv = amount * df;

            pv += cf_pv;
            weighted_time += t * cf_pv;
        }

        let macaulay = if pv > 0.0 { weighted_time / pv } else { 0.0 };

        // Modified duration = Macaulay / (1 + y/freq)
        let modified = if freq > 0.0 {
            macaulay / (1.0 + y / freq)
        } else {
            macaulay
        };

        let dollar = modified * pv / 100.0;

        Ok(DurationResult {
            macaulay: Decimal::from_f64_retain(macaulay).unwrap_or(Decimal::ZERO),
            modified: Decimal::from_f64_retain(modified).unwrap_or(Decimal::ZERO),
            dollar: Decimal::from_f64_retain(dollar).unwrap_or(Decimal::ZERO),
        })
    }

    /// Calculates convexity.
    pub fn convexity(
        bond: &FixedBond,
        yield_value: Decimal,
        settlement: Date,
    ) -> BondResult<Decimal> {
        let y = yield_value.to_string().parse::<f64>().unwrap_or(0.05);
        let freq = f64::from(bond.frequency().periods_per_year());

        let schedule = crate::cashflows::CashFlowGenerator::generate(bond, settlement)?;

        let mut pv = 0.0;
        let mut convex_sum = 0.0;

        for cf in schedule.iter() {
            let t = settlement.days_between(&cf.date()) as f64 / 365.0;
            let df = if freq > 0.0 {
                1.0 / (1.0 + y / freq).powf(freq * t)
            } else {
                (-y * t).exp()
            };
            let amount = cf.amount().to_string().parse::<f64>().unwrap_or(0.0);
            let cf_pv = amount * df;

            pv += cf_pv;

            // Convexity contribution: t * (t + 1/freq) * cf_pv
            let period_t = t + 1.0 / freq.max(1.0);
            convex_sum += t * period_t * cf_pv;
        }

        let convexity = if pv > 0.0 && freq > 0.0 {
            convex_sum / (pv * (1.0 + y / freq).powi(2))
        } else {
            0.0
        };

        Ok(Decimal::from_f64_retain(convexity).unwrap_or(Decimal::ZERO))
    }

    /// Calculates DV01 (dollar value of 1 basis point).
    pub fn dv01(bond: &FixedBond, yield_value: Decimal, settlement: Date) -> BondResult<Decimal> {
        let bp = Decimal::from_str_exact("0.0001").unwrap_or(Decimal::ZERO);

        let price_up = BondPricer::price_from_yield(bond, yield_value + bp, settlement)?;
        let price_down = BondPricer::price_from_yield(bond, yield_value - bp, settlement)?;

        // DV01 = (P_down - P_up) / 2
        let dv01 = (price_down.clean_price.as_percentage() - price_up.clean_price.as_percentage())
            / Decimal::TWO;

        Ok(dv01)
    }

    /// Estimates price change using duration and convexity.
    ///
    /// # Arguments
    ///
    /// * `metrics` - Risk metrics
    /// * `price` - Current price
    /// * `yield_change` - Change in yield (in decimal, e.g., 0.001 for 10bps)
    #[must_use]
    pub fn estimate_price_change(
        metrics: &RiskMetrics,
        price: Price,
        yield_change: Decimal,
    ) -> Decimal {
        let p = price.as_percentage();
        let dy = yield_change;
        let dur = metrics.duration.modified;
        let conv = metrics.convexity;

        // ΔP ≈ -D * P * Δy + 0.5 * C * P * (Δy)²
        let duration_effect = -dur * p * dy;
        let convexity_effect = conv * p * dy * dy / Decimal::TWO;

        duration_effect + convexity_effect
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instruments::FixedBondBuilder;
    use convex_core::types::{Currency, Frequency};
    use rust_decimal_macros::dec;

    fn create_test_bond() -> FixedBond {
        FixedBondBuilder::new()
            .isin("TEST")
            .coupon_rate(dec!(0.05))
            .maturity(Date::from_ymd(2030, 6, 15).unwrap())
            .frequency(Frequency::SemiAnnual)
            .currency(Currency::USD)
            .build()
            .unwrap()
    }

    #[test]
    fn test_duration() {
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();

        let result = RiskCalculator::duration(&bond, dec!(0.05), settlement).unwrap();

        // Duration should be positive and less than time to maturity
        assert!(result.macaulay > Decimal::ZERO);
        assert!(result.modified > Decimal::ZERO);

        // Modified duration should be less than Macaulay
        assert!(result.modified < result.macaulay);
    }

    #[test]
    fn test_convexity() {
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();

        let convexity = RiskCalculator::convexity(&bond, dec!(0.05), settlement).unwrap();

        // Convexity should be positive
        assert!(convexity > Decimal::ZERO);
    }

    #[test]
    fn test_dv01() {
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();

        let dv01 = RiskCalculator::dv01(&bond, dec!(0.05), settlement).unwrap();

        // DV01 should be positive (price decreases when yield increases)
        assert!(dv01 > Decimal::ZERO);
    }
}
