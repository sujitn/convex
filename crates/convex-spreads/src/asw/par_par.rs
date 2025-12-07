//! Par-par asset swap spread calculator.
//!
//! The par-par asset swap is the most common convention for investment-grade bonds.
//! In this structure:
//! - Investor pays par (100) for the bond regardless of market price
//! - The upfront payment (par - dirty price) is spread over the swap term
//! - This spread is added to/subtracted from the floating rate payments

use rust_decimal::Decimal;

use convex_bonds::traits::{Bond, FixedCouponBond};
use convex_core::types::{Date, Price, Spread, SpreadType};
use convex_curves::curves::ZeroCurve;

use crate::error::{SpreadError, SpreadResult};

/// Converts coupon frequency (payments per year) to months between payments.
fn frequency_to_months(frequency: u32) -> i32 {
    match frequency {
        1 => 12, // Annual
        4 => 3,  // Quarterly
        12 => 1, // Monthly
        _ => 6,  // Default to semi-annual
    }
}

/// Par-par asset swap spread calculator.
///
/// Calculates the spread over a floating rate that makes the total package
/// (bond + swap) trade at par.
///
/// # Formula
///
/// Par-Par ASW = (100 - Dirty Price) / Annuity
///
/// Where:
/// - Dirty Price = Clean Price + Accrued Interest
/// - Annuity = PV01 of the swap floating leg = Σ DF(t_i) × τ_i
///
/// # Example
///
/// ```rust,ignore
/// use convex_spreads::asw::ParParAssetSwap;
///
/// let calc = ParParAssetSwap::new(&swap_curve);
///
/// // Calculate par-par ASW
/// let spread = calc.calculate(&bond, clean_price, settlement)?;
/// println!("Par-Par ASW: {} bps", spread.as_bps());
///
/// // With explicit repo rate for net spread
/// let net_spread = calc.net_spread(&bond, clean_price, settlement, repo_rate)?;
/// ```
#[derive(Debug, Clone)]
pub struct ParParAssetSwap<'a> {
    /// Reference to the swap/discount curve.
    swap_curve: &'a ZeroCurve,
}

impl<'a> ParParAssetSwap<'a> {
    /// Creates a new par-par asset swap calculator.
    ///
    /// # Arguments
    ///
    /// * `swap_curve` - The swap rate/discount curve for PV calculations
    #[must_use]
    pub fn new(swap_curve: &'a ZeroCurve) -> Self {
        Self { swap_curve }
    }

    /// Returns the reference date from the swap curve.
    #[must_use]
    pub fn reference_date(&self) -> Date {
        self.swap_curve.reference_date()
    }

    /// Calculates the par-par asset swap spread.
    ///
    /// # Arguments
    ///
    /// * `bond` - The fixed coupon bond
    /// * `clean_price` - Market clean price
    /// * `settlement` - Settlement date
    ///
    /// # Returns
    ///
    /// The par-par ASW spread in basis points.
    ///
    /// # Errors
    ///
    /// Returns `SpreadError` if:
    /// - Settlement is after maturity
    /// - Curve interpolation fails
    /// - Annuity is zero (no future payments)
    pub fn calculate<B: Bond + FixedCouponBond>(
        &self,
        bond: &B,
        clean_price: Price,
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

        // Calculate dirty price
        let accrued = bond.accrued_interest(settlement);
        let dirty_price = clean_price.as_percentage() + accrued;

        // Upfront payment: par - dirty price
        // Positive when bond trades at discount (spread income to investor)
        // Negative when bond trades at premium (spread cost to investor)
        let upfront = Decimal::ONE_HUNDRED - dirty_price;

        // Calculate annuity (PV01 of swap floating leg)
        let months_between = frequency_to_months(bond.coupon_frequency());
        let annuity = self.calculate_annuity(
            settlement,
            maturity,
            months_between,
            bond.coupon_frequency(),
        )?;

        if annuity.is_zero() {
            return Err(SpreadError::invalid_input(
                "Annuity is zero - no future payments",
            ));
        }

        // Spread = upfront / annuity
        let spread_decimal = upfront / annuity;
        let spread_bps = (spread_decimal * Decimal::from(10_000)).round();

        Ok(Spread::new(spread_bps, SpreadType::AssetSwapPar))
    }

    /// Calculates gross asset swap spread (same as calculate).
    ///
    /// This is an alias for `calculate()` to match market terminology.
    #[inline]
    pub fn gross_spread<B: Bond + FixedCouponBond>(
        &self,
        bond: &B,
        clean_price: Price,
        settlement: Date,
    ) -> SpreadResult<Spread> {
        self.calculate(bond, clean_price, settlement)
    }

    /// Calculates net asset swap spread after funding cost.
    ///
    /// The net spread adjusts for the cost of funding the bond position
    /// using repo or other financing.
    ///
    /// # Arguments
    ///
    /// * `bond` - The fixed coupon bond
    /// * `clean_price` - Market clean price
    /// * `settlement` - Settlement date
    /// * `repo_rate` - Repo/funding rate (as decimal, e.g., 0.05 for 5%)
    ///
    /// # Formula
    ///
    /// Net ASW = Gross ASW - Funding Adjustment
    /// Funding Adjustment = (Dirty Price / 100 - 1) × Repo Rate
    ///
    /// For premium bonds, this reduces the spread (cost of funding premium).
    /// For discount bonds, this increases the spread (benefit of funding discount).
    pub fn net_spread<B: Bond + FixedCouponBond>(
        &self,
        bond: &B,
        clean_price: Price,
        settlement: Date,
        repo_rate: Decimal,
    ) -> SpreadResult<Spread> {
        let gross = self.calculate(bond, clean_price, settlement)?;

        // Calculate funding adjustment
        let accrued = bond.accrued_interest(settlement);
        let dirty_price = clean_price.as_percentage() + accrued;

        // Funding spread: (DirtyPrice/100 - 1) × repo_rate
        // Premium bond (DP > 100): positive adjustment, reduces net spread
        // Discount bond (DP < 100): negative adjustment, increases net spread
        let funding_adjustment = (dirty_price / Decimal::ONE_HUNDRED - Decimal::ONE) * repo_rate;
        let funding_bps = (funding_adjustment * Decimal::from(10_000)).round();

        let net_bps = gross.as_bps() - funding_bps;
        Ok(Spread::new(net_bps, SpreadType::AssetSwapPar))
    }

    /// Calculates the swap annuity (PV01 of floating leg).
    ///
    /// Annuity = Σ DF(t_i) × τ_i
    ///
    /// Where:
    /// - DF(t_i) = discount factor at payment date i
    /// - τ_i = year fraction for period i
    fn calculate_annuity(
        &self,
        settlement: Date,
        maturity: Date,
        months_between: i32,
        payments_per_year: u32,
    ) -> SpreadResult<Decimal> {
        if payments_per_year == 0 {
            return Err(SpreadError::invalid_input("Invalid payment frequency"));
        }

        // Calculate payment dates going backwards from maturity
        let mut payment_dates = Vec::new();
        let mut current_date = maturity;

        while current_date > settlement {
            payment_dates.push(current_date);
            // Go back by months_between using add_months with negative value
            current_date = current_date
                .add_months(-months_between)
                .map_err(|e| SpreadError::invalid_input(e.to_string()))?;
        }

        if payment_dates.is_empty() {
            return Err(SpreadError::invalid_input(
                "No payment dates after settlement",
            ));
        }

        // Calculate annuity
        let year_fraction = Decimal::ONE / Decimal::from(payments_per_year);
        let mut annuity = Decimal::ZERO;

        for payment_date in &payment_dates {
            let df = self
                .swap_curve
                .discount_factor_at(*payment_date)
                .map_err(|e| SpreadError::curve_error(e.to_string()))?;
            annuity += df * year_fraction;
        }

        Ok(annuity)
    }

    /// Returns the swap annuity for a given bond.
    ///
    /// This is useful for analyzing the duration of the swap leg.
    pub fn annuity<B: Bond + FixedCouponBond>(
        &self,
        bond: &B,
        settlement: Date,
    ) -> SpreadResult<Decimal> {
        let maturity = bond
            .maturity()
            .ok_or_else(|| SpreadError::invalid_input("Bond has no maturity (perpetual)"))?;

        let months_between = frequency_to_months(bond.coupon_frequency());
        self.calculate_annuity(
            settlement,
            maturity,
            months_between,
            bond.coupon_frequency(),
        )
    }

    /// Calculates the implied bond price from a given ASW spread.
    ///
    /// This is the inverse calculation: given a target ASW spread,
    /// what would the bond price need to be?
    ///
    /// # Formula
    ///
    /// Dirty Price = 100 - (ASW Spread × Annuity)
    /// Clean Price = Dirty Price - Accrued Interest
    pub fn implied_price<B: Bond + FixedCouponBond>(
        &self,
        bond: &B,
        asw_spread: Spread,
        settlement: Date,
    ) -> SpreadResult<Price> {
        let maturity = bond
            .maturity()
            .ok_or_else(|| SpreadError::invalid_input("Bond has no maturity (perpetual)"))?;

        if settlement >= maturity {
            return Err(SpreadError::SettlementAfterMaturity {
                settlement: settlement.to_string(),
                maturity: maturity.to_string(),
            });
        }

        let months_between = frequency_to_months(bond.coupon_frequency());
        let annuity = self.calculate_annuity(
            settlement,
            maturity,
            months_between,
            bond.coupon_frequency(),
        )?;

        // Convert spread from bps to decimal
        let spread_decimal = asw_spread.as_bps() / Decimal::from(10_000);

        // Dirty price = 100 - (spread × annuity)
        let dirty_price = Decimal::ONE_HUNDRED - spread_decimal * annuity;

        // Clean price = dirty - accrued
        let accrued = bond.accrued_interest(settlement);
        let clean_price = dirty_price - accrued;

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

    // Mock bond for testing
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
            // Return some accrued for testing
            dec!(1.25)
        }

        fn day_count_convention(&self) -> &str {
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
        // Clean price that results in dirty price ≈ 100
        let clean_price = Price::new(dec!(98.75), convex_core::Currency::USD);
        let settlement = date(2024, 1, 17);

        let spread = calc.calculate(&bond, clean_price, settlement).unwrap();

        // At par (dirty ≈ 100), ASW should be close to 0
        assert!(
            spread.as_bps().abs() < dec!(5),
            "Expected near-zero spread at par, got {}",
            spread.as_bps()
        );
    }

    #[test]
    fn test_par_par_asw_discount() {
        let curve = create_flat_curve(dec!(0.05));
        let calc = ParParAssetSwap::new(&curve);

        let bond = MockBond::new(date(2029, 1, 15), dec!(0.05), 2);
        // Discount bond: clean price 95, dirty ≈ 96.25
        let clean_price = Price::new(dec!(95.0), convex_core::Currency::USD);
        let settlement = date(2024, 1, 17);

        let spread = calc.calculate(&bond, clean_price, settlement).unwrap();

        // Discount bond should have positive ASW (spread income)
        assert!(
            spread.as_bps() > Decimal::ZERO,
            "Expected positive spread for discount bond"
        );
    }

    #[test]
    fn test_par_par_asw_premium() {
        let curve = create_flat_curve(dec!(0.05));
        let calc = ParParAssetSwap::new(&curve);

        let bond = MockBond::new(date(2029, 1, 15), dec!(0.05), 2);
        // Premium bond: clean price 105, dirty ≈ 106.25
        let clean_price = Price::new(dec!(105.0), convex_core::Currency::USD);
        let settlement = date(2024, 1, 17);

        let spread = calc.calculate(&bond, clean_price, settlement).unwrap();

        // Premium bond should have negative ASW (spread cost)
        assert!(
            spread.as_bps() < Decimal::ZERO,
            "Expected negative spread for premium bond"
        );
    }

    #[test]
    fn test_net_spread() {
        let curve = create_flat_curve(dec!(0.05));
        let calc = ParParAssetSwap::new(&curve);

        let bond = MockBond::new(date(2029, 1, 15), dec!(0.05), 2);
        let clean_price = Price::new(dec!(95.0), convex_core::Currency::USD);
        let settlement = date(2024, 1, 17);
        let repo_rate = dec!(0.05);

        let gross = calc.gross_spread(&bond, clean_price, settlement).unwrap();
        let net = calc
            .net_spread(&bond, clean_price, settlement, repo_rate)
            .unwrap();

        // For discount bond, net spread should be higher than gross
        // (benefit from funding at discount)
        assert!(
            net.as_bps() > gross.as_bps(),
            "Expected net > gross for discount bond"
        );
    }

    #[test]
    fn test_settlement_after_maturity() {
        let curve = create_flat_curve(dec!(0.05));
        let calc = ParParAssetSwap::new(&curve);

        let bond = MockBond::new(date(2024, 1, 15), dec!(0.05), 2);
        let clean_price = Price::new(dec!(100.0), convex_core::Currency::USD);
        let settlement = date(2024, 6, 15); // After maturity

        let result = calc.calculate(&bond, clean_price, settlement);
        assert!(result.is_err());
    }

    #[test]
    fn test_annuity_calculation() {
        let curve = create_flat_curve(dec!(0.05));
        let calc = ParParAssetSwap::new(&curve);

        let bond = MockBond::new(date(2029, 1, 15), dec!(0.05), 2);
        let settlement = date(2024, 1, 17);

        let annuity = calc.annuity(&bond, settlement).unwrap();

        // Annuity should be roughly the years to maturity for a flat curve
        // 5 years × 2 payments/year × 0.5 year fraction × DF ≈ 4-5
        assert!(
            annuity > dec!(3) && annuity < dec!(6),
            "Annuity {} seems wrong",
            annuity
        );
    }

    #[test]
    fn test_implied_price() {
        let curve = create_flat_curve(dec!(0.05));
        let calc = ParParAssetSwap::new(&curve);

        let bond = MockBond::new(date(2029, 1, 15), dec!(0.05), 2);
        let settlement = date(2024, 1, 17);

        // At 0 spread, implied dirty price should be 100
        let zero_spread = Spread::new(dec!(0), SpreadType::AssetSwapPar);
        let implied = calc.implied_price(&bond, zero_spread, settlement).unwrap();

        // Clean = 100 - accrued (1.25) = 98.75
        let expected_clean = dec!(98.75);
        let diff = (implied.as_percentage() - expected_clean).abs();
        assert!(
            diff < dec!(0.01),
            "Expected clean price ~{}, got {}",
            expected_clean,
            implied.as_percentage()
        );
    }
}
