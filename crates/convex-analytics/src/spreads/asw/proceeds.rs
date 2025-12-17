//! Proceeds asset swap spread calculator.

use rust_decimal::Decimal;

use convex_bonds::traits::{Bond, FixedCouponBond};
use convex_core::types::{Date, Price, Spread, SpreadType};
use convex_curves::curves::ZeroCurve;

use crate::error::{AnalyticsError, AnalyticsResult};

/// Converts coupon frequency to months between payments.
fn frequency_to_months(frequency: u32) -> i32 {
    match frequency {
        1 => 12,
        4 => 3,
        12 => 1,
        _ => 6,
    }
}

/// Proceeds asset swap spread calculator.
#[derive(Debug, Clone)]
pub struct ProceedsAssetSwap<'a> {
    swap_curve: &'a ZeroCurve,
}

impl<'a> ProceedsAssetSwap<'a> {
    /// Creates a new proceeds asset swap calculator.
    #[must_use]
    pub fn new(swap_curve: &'a ZeroCurve) -> Self {
        Self { swap_curve }
    }

    /// Returns the reference date from the swap curve.
    #[must_use]
    pub fn reference_date(&self) -> Date {
        self.swap_curve.reference_date()
    }

    /// Calculates the proceeds asset swap spread.
    pub fn calculate<B: Bond + FixedCouponBond>(
        &self,
        bond: &B,
        clean_price: Price,
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

        let accrued = bond.accrued_interest(settlement);
        let dirty_price = clean_price.as_percentage() + accrued;

        if dirty_price.is_zero() {
            return Err(AnalyticsError::InvalidInput(
                "Dirty price is zero".to_string(),
            ));
        }

        let months_between = frequency_to_months(bond.coupon_frequency());
        let annuity = self.calculate_annuity(
            settlement,
            maturity,
            months_between,
            bond.coupon_frequency(),
        )?;

        if annuity.is_zero() {
            return Err(AnalyticsError::InvalidInput(
                "Annuity is zero - no future payments".to_string(),
            ));
        }

        let upfront = Decimal::ONE_HUNDRED - dirty_price;
        let par_par_spread = upfront / annuity;

        let proceeds_spread = par_par_spread * Decimal::ONE_HUNDRED / dirty_price;
        let spread_bps = (proceeds_spread * Decimal::from(100)).round();

        Ok(Spread::new(spread_bps, SpreadType::AssetSwapProceeds))
    }

    /// Calculates the market value asset swap spread.
    pub fn market_value_spread<B: Bond + FixedCouponBond>(
        &self,
        bond: &B,
        clean_price: Price,
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

        let accrued = bond.accrued_interest(settlement);
        let dirty_price = clean_price.as_percentage() + accrued;

        if dirty_price.is_zero() {
            return Err(AnalyticsError::InvalidInput(
                "Dirty price is zero".to_string(),
            ));
        }

        let swap_rate = self
            .swap_curve
            .zero_rate_at(maturity)
            .map_err(|e| AnalyticsError::CurveError(e.to_string()))?;

        let coupon_rate = bond.coupon_rate();
        let coupon_mismatch = coupon_rate - swap_rate;

        let mv_spread = coupon_mismatch * Decimal::ONE_HUNDRED / dirty_price;

        let price_adjustment = (Decimal::ONE_HUNDRED - dirty_price) / dirty_price;

        let years_to_mat = settlement.days_between(&maturity) as f64 / 365.0;
        let annual_adjustment =
            price_adjustment / Decimal::from_f64_retain(years_to_mat).unwrap_or(Decimal::ONE);

        let total_spread = mv_spread + annual_adjustment;
        let spread_bps = (total_spread * Decimal::from(10_000)).round();

        Ok(Spread::new(spread_bps, SpreadType::AssetSwapProceeds))
    }

    fn calculate_annuity(
        &self,
        settlement: Date,
        maturity: Date,
        months_between: i32,
        payments_per_year: u32,
    ) -> AnalyticsResult<Decimal> {
        if payments_per_year == 0 {
            return Err(AnalyticsError::InvalidInput(
                "Invalid payment frequency".to_string(),
            ));
        }

        let mut payment_dates = Vec::new();
        let mut current_date = maturity;

        while current_date > settlement {
            payment_dates.push(current_date);
            current_date = current_date
                .add_months(-months_between)
                .map_err(|e| AnalyticsError::InvalidInput(e.to_string()))?;
        }

        if payment_dates.is_empty() {
            return Err(AnalyticsError::InvalidInput(
                "No payment dates after settlement".to_string(),
            ));
        }

        let year_fraction = Decimal::ONE / Decimal::from(payments_per_year);
        let mut annuity = Decimal::ZERO;

        for payment_date in &payment_dates {
            let df = self
                .swap_curve
                .discount_factor_at(*payment_date)
                .map_err(|e| AnalyticsError::CurveError(e.to_string()))?;
            annuity += df * year_fraction;
        }

        Ok(annuity)
    }

    /// Calculates the Z-spread equivalent of the proceeds ASW.
    pub fn z_spread_equivalent<B: Bond + FixedCouponBond>(
        &self,
        bond: &B,
        clean_price: Price,
        settlement: Date,
    ) -> AnalyticsResult<Spread> {
        let proceeds_asw = self.calculate(bond, clean_price, settlement)?;

        let accrued = bond.accrued_interest(settlement);
        let dirty_price = clean_price.as_percentage() + accrued;

        let z_spread_bps = proceeds_asw.as_bps() * dirty_price / Decimal::ONE_HUNDRED;

        Ok(Spread::new(z_spread_bps.round(), SpreadType::ZSpread))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_curves::curves::ZeroCurveBuilder;
    use rust_decimal_macros::dec;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    fn create_flat_curve(rate: Decimal) -> ZeroCurve {
        ZeroCurveBuilder::new()
            .reference_date(date(2024, 1, 15))
            .add_rate(date(2024, 7, 15), rate)
            .add_rate(date(2025, 1, 15), rate)
            .add_rate(date(2026, 1, 15), rate)
            .add_rate(date(2029, 1, 15), rate)
            .add_rate(date(2034, 1, 15), rate)
            .build()
            .unwrap()
    }

    struct MockBond {
        maturity: Date,
        coupon_rate: Decimal,
        frequency: u32,
        calendar: convex_bonds::types::CalendarId,
    }

    impl MockBond {
        fn new(maturity: Date, coupon_rate: Decimal, frequency: u32) -> Self {
            Self {
                maturity,
                coupon_rate,
                frequency,
                calendar: convex_bonds::types::CalendarId::us_government(),
            }
        }
    }

    impl Bond for MockBond {
        fn identifiers(&self) -> &convex_bonds::types::BondIdentifiers {
            unimplemented!("Not needed for test")
        }

        fn bond_type(&self) -> convex_bonds::types::BondType {
            convex_bonds::types::BondType::FixedRateCorporate
        }

        fn currency(&self) -> convex_core::Currency {
            convex_core::Currency::USD
        }

        fn maturity(&self) -> Option<Date> {
            Some(self.maturity)
        }

        fn issue_date(&self) -> Date {
            date(2020, 1, 15)
        }

        fn first_settlement_date(&self) -> Date {
            date(2020, 1, 15)
        }

        fn dated_date(&self) -> Date {
            date(2020, 1, 15)
        }

        fn face_value(&self) -> Decimal {
            Decimal::ONE_HUNDRED
        }

        fn frequency(&self) -> convex_core::types::Frequency {
            convex_core::types::Frequency::SemiAnnual
        }

        fn cash_flows(&self, _from: Date) -> Vec<convex_bonds::traits::BondCashFlow> {
            Vec::new()
        }

        fn next_coupon_date(&self, _after: Date) -> Option<Date> {
            None
        }

        fn previous_coupon_date(&self, _before: Date) -> Option<Date> {
            None
        }

        fn accrued_interest(&self, _settlement: Date) -> Decimal {
            dec!(1.25)
        }

        fn day_count_convention(&self) -> &'static str {
            "ACT/ACT"
        }

        fn calendar(&self) -> &convex_bonds::types::CalendarId {
            &self.calendar
        }
    }

    impl FixedCouponBond for MockBond {
        fn coupon_rate(&self) -> Decimal {
            self.coupon_rate
        }

        fn coupon_frequency(&self) -> u32 {
            self.frequency
        }

        fn first_coupon_date(&self) -> Option<Date> {
            None
        }

        fn last_coupon_date(&self) -> Option<Date> {
            None
        }
    }

    #[test]
    fn test_proceeds_calculator_creation() {
        let curve = create_flat_curve(dec!(0.05));
        let calc = ProceedsAssetSwap::new(&curve);
        assert_eq!(calc.reference_date(), date(2024, 1, 15));
    }

    #[test]
    fn test_proceeds_asw_at_par() {
        let curve = create_flat_curve(dec!(0.05));
        let calc = ProceedsAssetSwap::new(&curve);

        let bond = MockBond::new(date(2029, 1, 15), dec!(0.05), 2);
        let clean_price = Price::new(dec!(98.75), convex_core::Currency::USD);
        let settlement = date(2024, 1, 17);

        let spread = calc.calculate(&bond, clean_price, settlement).unwrap();

        assert!(
            spread.as_bps().abs() < dec!(5),
            "Expected near-zero spread at par, got {}",
            spread.as_bps()
        );
    }

    #[test]
    fn test_settlement_after_maturity() {
        let curve = create_flat_curve(dec!(0.05));
        let calc = ProceedsAssetSwap::new(&curve);

        let bond = MockBond::new(date(2024, 1, 15), dec!(0.05), 2);
        let clean_price = Price::new(dec!(100.0), convex_core::Currency::USD);
        let settlement = date(2024, 6, 15);

        let result = calc.calculate(&bond, clean_price, settlement);
        assert!(result.is_err());
    }
}
