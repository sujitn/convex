//! Par-par asset swap spread calculator.

use rust_decimal::prelude::ToPrimitive;
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

/// Par-par asset swap spread calculator.
#[derive(Debug, Clone)]
pub struct ParParAssetSwap<'a> {
    swap_curve: &'a ZeroCurve,
}

impl<'a> ParParAssetSwap<'a> {
    /// Creates a new par-par asset swap calculator.
    #[must_use]
    pub fn new(swap_curve: &'a ZeroCurve) -> Self {
        Self { swap_curve }
    }

    /// Returns the reference date from the swap curve.
    #[must_use]
    pub fn reference_date(&self) -> Date {
        self.swap_curve.reference_date()
    }

    /// Par-par ASW: `ASW · A = (100 − dirty) + Σ DFᵢ · τᵢ · (c − Lᵢ)`, with
    /// `Lᵢ` the simple-compounded forward over period i.
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

        let upfront = Decimal::ONE_HUNDRED - dirty_price;

        let months_between = frequency_to_months(bond.coupon_frequency());

        let (annuity, mismatch_pct) = self.annuity_and_mismatch_pct(
            settlement,
            maturity,
            months_between,
            bond.coupon_frequency(),
            bond.coupon_rate(),
        )?;

        if annuity.is_zero() {
            return Err(AnalyticsError::InvalidInput(
                "Annuity is zero - no future payments".to_string(),
            ));
        }

        let spread_bps = ((upfront + mismatch_pct) / annuity * Decimal::from(100)).round();
        Ok(Spread::new(spread_bps, SpreadType::AssetSwapPar))
    }

    /// Calculates gross asset swap spread (same as calculate).
    #[inline]
    pub fn gross_spread<B: Bond + FixedCouponBond>(
        &self,
        bond: &B,
        clean_price: Price,
        settlement: Date,
    ) -> AnalyticsResult<Spread> {
        self.calculate(bond, clean_price, settlement)
    }

    /// Calculates net asset swap spread after funding cost.
    pub fn net_spread<B: Bond + FixedCouponBond>(
        &self,
        bond: &B,
        clean_price: Price,
        settlement: Date,
        repo_rate: Decimal,
    ) -> AnalyticsResult<Spread> {
        let gross = self.calculate(bond, clean_price, settlement)?;

        let accrued = bond.accrued_interest(settlement);
        let dirty_price = clean_price.as_percentage() + accrued;

        let funding_adjustment = (dirty_price / Decimal::ONE_HUNDRED - Decimal::ONE) * repo_rate;
        let funding_bps = (funding_adjustment * Decimal::from(10_000)).round();

        let net_bps = gross.as_bps() - funding_bps;
        Ok(Spread::new(net_bps, SpreadType::AssetSwapPar))
    }

    /// Walks the schedule and returns `(annuity, coupon_mismatch_pct)` where
    /// `coupon_mismatch_pct = Σ DFᵢ · τᵢ · (c − Lᵢ) · 100` in price units.
    /// `Lᵢ` is the simple-compounded forward over [tᵢ₋₁, tᵢ].
    fn annuity_and_mismatch_pct(
        &self,
        settlement: Date,
        maturity: Date,
        months_between: i32,
        payments_per_year: u32,
        coupon_rate: Decimal,
    ) -> AnalyticsResult<(Decimal, Decimal)> {
        if payments_per_year == 0 {
            return Err(AnalyticsError::InvalidInput(
                "Invalid payment frequency".to_string(),
            ));
        }

        let mut payment_dates: Vec<Date> = Vec::new();
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
        payment_dates.reverse();

        let tau = 1.0 / payments_per_year as f64;
        let coupon = coupon_rate.to_f64().unwrap();

        let mut annuity = 0.0;
        let mut mismatch = 0.0;
        let mut prev_df = self
            .swap_curve
            .discount_factor(settlement)
            .map_err(|e| AnalyticsError::CurveError(e.to_string()))?;

        for date in &payment_dates {
            let df = self
                .swap_curve
                .discount_factor(*date)
                .map_err(|e| AnalyticsError::CurveError(e.to_string()))?;
            let fwd = (prev_df / df - 1.0) / tau;
            annuity += tau * df;
            mismatch += df * tau * (coupon - fwd) * 100.0;
            prev_df = df;
        }

        Ok((
            Decimal::from_f64_retain(annuity).unwrap_or_default(),
            Decimal::from_f64_retain(mismatch).unwrap_or_default(),
        ))
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
            let df_f64 = self
                .swap_curve
                .discount_factor(*payment_date)
                .map_err(|e| AnalyticsError::CurveError(e.to_string()))?;
            let df = Decimal::from_f64_retain(df_f64).unwrap_or(Decimal::ZERO);
            annuity += df * year_fraction;
        }

        Ok(annuity)
    }

    /// Returns the swap annuity for a given bond.
    pub fn annuity<B: Bond + FixedCouponBond>(
        &self,
        bond: &B,
        settlement: Date,
    ) -> AnalyticsResult<Decimal> {
        let maturity = bond.maturity().ok_or_else(|| {
            AnalyticsError::InvalidInput("Bond has no maturity (perpetual)".to_string())
        })?;

        let months_between = frequency_to_months(bond.coupon_frequency());
        self.calculate_annuity(
            settlement,
            maturity,
            months_between,
            bond.coupon_frequency(),
        )
    }

    /// Calculates the implied bond price from a given ASW spread.
    pub fn implied_price<B: Bond + FixedCouponBond>(
        &self,
        bond: &B,
        asw_spread: Spread,
        settlement: Date,
    ) -> AnalyticsResult<Price> {
        let maturity = bond.maturity().ok_or_else(|| {
            AnalyticsError::InvalidInput("Bond has no maturity (perpetual)".to_string())
        })?;

        if settlement >= maturity {
            return Err(AnalyticsError::InvalidSettlement {
                settlement: settlement.to_string(),
                maturity: maturity.to_string(),
            });
        }

        // Inverse of `calculate`. Forward solves
        //   spread_bps = ((100 − dirty) + mismatch_pct) / annuity · 100
        // (the `× 100` converts %-per-year to bps), so the inverse is
        //   dirty = 100 + mismatch_pct − (spread_bps / 100) · annuity.
        let months_between = frequency_to_months(bond.coupon_frequency());
        let (annuity, mismatch_pct) = self.annuity_and_mismatch_pct(
            settlement,
            maturity,
            months_between,
            bond.coupon_frequency(),
            bond.coupon_rate(),
        )?;

        let spread_pct = asw_spread.as_bps() / Decimal::from(100);
        let dirty_price = Decimal::ONE_HUNDRED + mismatch_pct - spread_pct * annuity;

        let clean_price = dirty_price - bond.accrued_interest(settlement);
        Ok(Price::new(clean_price, bond.currency()))
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
    fn test_par_par_calculator_creation() {
        let curve = create_flat_curve(dec!(0.05));
        let calc = ParParAssetSwap::new(&curve);
        assert_eq!(calc.reference_date(), date(2024, 1, 15));
    }

    #[test]
    fn test_par_par_asw_at_par() {
        let curve = create_flat_curve(dec!(0.05));
        let calc = ParParAssetSwap::new(&curve);

        let bond = MockBond::new(date(2029, 1, 15), dec!(0.05), 2);
        let clean_price = Price::new(dec!(98.75), convex_core::Currency::USD);
        let settlement = date(2024, 1, 17);

        let spread = calc.calculate(&bond, clean_price, settlement).unwrap();
        // Small residual (~7 bp) from semi-annual coupon vs continuous curve.
        assert!(spread.as_bps().abs() < dec!(15));
    }

    #[test]
    fn test_par_par_asw_discount() {
        let curve = create_flat_curve(dec!(0.05));
        let calc = ParParAssetSwap::new(&curve);

        let bond = MockBond::new(date(2029, 1, 15), dec!(0.05), 2);
        let clean_price = Price::new(dec!(95.0), convex_core::Currency::USD);
        let settlement = date(2024, 1, 17);

        let spread = calc.calculate(&bond, clean_price, settlement).unwrap();

        assert!(
            spread.as_bps() > Decimal::ZERO,
            "Expected positive spread for discount bond"
        );
    }

    #[test]
    fn test_par_par_captures_coupon_minus_floating() {
        // 8% bond on a 3% flat curve at par: ASW must reflect the ~500 bp
        // coupon excess. Upfront-only formula returned ~0.
        let curve = create_flat_curve(dec!(0.03));
        let bond = MockBond::new(date(2029, 1, 15), dec!(0.08), 2);
        let spread = ParParAssetSwap::new(&curve)
            .calculate(
                &bond,
                Price::new(dec!(100.0), convex_core::Currency::USD),
                date(2024, 1, 17),
            )
            .unwrap();
        assert!(spread.as_bps() >= dec!(400) && spread.as_bps() <= dec!(600));
    }

    #[test]
    fn test_calculate_implied_price_round_trip() {
        // implied_price must invert calculate. Was broken when calculate
        // started embedding the coupon-floating mismatch term.
        let curve = create_flat_curve(dec!(0.04));
        let calc = ParParAssetSwap::new(&curve);
        let bond = MockBond::new(date(2029, 1, 15), dec!(0.06), 2);
        let settlement = date(2024, 1, 17);

        let original = Price::new(dec!(102.0), convex_core::Currency::USD);
        let spread = calc.calculate(&bond, original, settlement).unwrap();
        let recovered = calc.implied_price(&bond, spread, settlement).unwrap();

        let diff = (recovered.as_percentage() - original.as_percentage()).abs();
        assert!(diff < dec!(0.05), "round-trip drift: {diff}");
    }

    #[test]
    fn test_settlement_after_maturity() {
        let curve = create_flat_curve(dec!(0.05));
        let calc = ParParAssetSwap::new(&curve);

        let bond = MockBond::new(date(2024, 1, 15), dec!(0.05), 2);
        let clean_price = Price::new(dec!(100.0), convex_core::Currency::USD);
        let settlement = date(2024, 6, 15);

        let result = calc.calculate(&bond, clean_price, settlement);
        assert!(result.is_err());
    }
}
