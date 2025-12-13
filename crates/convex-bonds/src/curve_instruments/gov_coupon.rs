//! Fixed coupon government bond wrapper for curve construction.

use rust_decimal::prelude::*;

use convex_core::Date;
use convex_curves::instruments::{CashFlow, CurveInstrument, InstrumentType};
use convex_curves::traits::Curve;
use convex_curves::CurveResult;

use crate::instruments::{Bond, FixedBond};

use super::conventions::{day_count_factor, MarketConvention};

/// A fixed coupon government bond for curve bootstrapping.
///
/// Wraps a [`FixedBond`] with market data (settlement date and price)
/// to enable curve construction. Supports any market convention.
///
/// # Pricing
///
/// ```text
/// Dirty Price = Σ Coupon(i) × DF(Ti) + Face × DF(Tn)
/// Clean Price = Dirty Price - Accrued Interest
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use convex_bonds::prelude::*;
///
/// // UK Gilt 10 years, 4% coupon
/// let bond = FixedBondBuilder::new()
///     .isin("GB0009997999")
///     .coupon_rate(dec!(0.04))
///     .maturity(maturity)
///     .frequency(Frequency::SemiAnnual)
///     .currency(Currency::GBP)
///     .build()
///     .unwrap();
///
/// let gilt = GovernmentCouponBond::new(
///     bond,
///     settlement,
///     98.50,
///     MarketConvention::UKGilt,
/// );
/// ```
#[derive(Debug, Clone)]
pub struct GovernmentCouponBond {
    /// The underlying fixed coupon bond.
    bond: FixedBond,
    /// Settlement date for pricing.
    settlement: Date,
    /// Clean price per 100 face value.
    clean_price: f64,
    /// Market convention (determines day count).
    convention: MarketConvention,
}

impl GovernmentCouponBond {
    /// Creates a new government coupon bond for curve construction.
    ///
    /// # Arguments
    ///
    /// * `bond` - The underlying fixed coupon bond
    /// * `settlement` - Settlement date
    /// * `clean_price` - Clean price per 100 face value
    /// * `convention` - Market convention for day count
    #[must_use]
    pub fn new(
        bond: FixedBond,
        settlement: Date,
        clean_price: f64,
        convention: MarketConvention,
    ) -> Self {
        Self {
            bond,
            settlement,
            clean_price,
            convention,
        }
    }

    /// Creates from yield to maturity.
    ///
    /// Converts YTM to clean price using standard bond pricing with the
    /// appropriate compounding frequency for the market.
    ///
    /// # Arguments
    ///
    /// * `bond` - The underlying fixed coupon bond
    /// * `settlement` - Settlement date
    /// * `ytm` - Yield to maturity as decimal (0.05 = 5%)
    /// * `convention` - Market convention
    #[must_use]
    pub fn from_ytm(
        bond: FixedBond,
        settlement: Date,
        ytm: f64,
        convention: MarketConvention,
    ) -> Self {
        // Create temporary bond to get cash flows
        let temp = Self::new(bond.clone(), settlement, 100.0, convention);
        let flows = temp.cash_flows();

        // Use appropriate compounding frequency
        let periods_per_year = convention.coupons_per_year();
        let y_periodic = ytm / f64::from(periods_per_year);

        let mut dirty_price = 0.0;
        for cf in &flows {
            let t = temp.year_fraction(cf.date);
            let n = t * f64::from(periods_per_year);
            let df = (1.0 + y_periodic).powf(-n);
            dirty_price += cf.amount * df;
        }

        let clean_price = dirty_price - temp.accrued_interest();

        Self::new(bond, settlement, clean_price, convention)
    }

    /// Returns the underlying bond.
    #[must_use]
    pub fn bond(&self) -> &FixedBond {
        &self.bond
    }

    /// Returns the settlement date.
    #[must_use]
    pub fn settlement(&self) -> Date {
        self.settlement
    }

    /// Returns the clean price.
    #[must_use]
    pub fn clean_price(&self) -> f64 {
        self.clean_price
    }

    /// Returns the market convention.
    #[must_use]
    pub fn convention(&self) -> MarketConvention {
        self.convention
    }

    /// Returns the face value.
    #[must_use]
    pub fn face_value(&self) -> f64 {
        self.bond.face_value().to_f64().unwrap_or(100.0)
    }

    /// Returns the coupon amount per period.
    #[must_use]
    pub fn coupon_per_period(&self) -> f64 {
        self.bond.coupon_per_period().to_f64().unwrap_or(0.0)
    }

    /// Returns the dirty price (clean + accrued).
    #[must_use]
    pub fn dirty_price(&self) -> f64 {
        self.clean_price + self.accrued_interest()
    }

    /// Calculates accrued interest.
    ///
    /// Uses the appropriate day count convention:
    /// `Accrued = Coupon × (Days since last coupon / Days in period)`
    #[must_use]
    pub fn accrued_interest(&self) -> f64 {
        let (prev_coupon, next_coupon) = self.coupon_dates();

        let days_accrued = prev_coupon.days_between(&self.settlement) as f64;
        let days_in_period = prev_coupon.days_between(&next_coupon) as f64;

        if days_in_period <= 0.0 {
            return 0.0;
        }

        self.coupon_per_period() * (days_accrued / days_in_period)
    }

    /// Generates all cash flows after settlement.
    #[must_use]
    pub fn cash_flows(&self) -> Vec<CashFlow> {
        let coupon = self.coupon_per_period();
        let face = self.face_value();
        let maturity = self.bond.maturity();
        let months_per_period = self.bond.frequency().months_per_period() as i32;

        let mut flows = Vec::new();
        let mut date = maturity;

        // Walk backwards from maturity
        while date > self.settlement {
            let amount = if date == maturity {
                coupon + face
            } else {
                coupon
            };
            flows.push(CashFlow::new(date, amount));

            if let Ok(prev) = date.add_months(-months_per_period) {
                date = prev;
            } else {
                break;
            }
        }

        flows.reverse();
        flows
    }

    /// Finds the previous and next coupon dates relative to settlement.
    fn coupon_dates(&self) -> (Date, Date) {
        let months_per_period = self.bond.frequency().months_per_period() as i32;
        let mut next_coupon = self.bond.maturity();

        // Walk back from maturity to find the next coupon after settlement
        while next_coupon > self.settlement {
            if let Ok(prev) = next_coupon.add_months(-months_per_period) {
                if prev <= self.settlement {
                    break;
                }
                next_coupon = prev;
            } else {
                break;
            }
        }

        let prev_coupon = next_coupon
            .add_months(-months_per_period)
            .unwrap_or(self.settlement);

        (prev_coupon, next_coupon)
    }

    /// Calculates year fraction from settlement to date using the convention.
    fn year_fraction(&self, date: Date) -> f64 {
        day_count_factor(self.settlement, date, self.convention)
    }
}

impl CurveInstrument for GovernmentCouponBond {
    fn maturity(&self) -> Date {
        self.bond.maturity()
    }

    fn pillar_date(&self) -> Date {
        self.bond.maturity()
    }

    fn pv(&self, curve: &dyn Curve) -> CurveResult<f64> {
        // Theoretical price = Σ CF(i) × DF(Ti)
        let mut theoretical = 0.0;

        for cf in self.cash_flows() {
            let t = self.year_fraction(cf.date);
            let df = curve.discount_factor(t)?;
            theoretical += cf.amount * df;
        }

        // PV = Theoretical - Dirty Price
        Ok(theoretical - self.dirty_price())
    }

    fn implied_df(&self, curve: &dyn Curve, _target_pv: f64) -> CurveResult<f64> {
        // Solve for DF at maturity given known DFs for earlier coupons
        // Dirty = Known_PV + Final_CF × DF(maturity)
        // DF(maturity) = (Dirty - Known_PV) / Final_CF

        let flows = self.cash_flows();
        if flows.is_empty() {
            return Ok(self.dirty_price() / self.face_value());
        }

        let dirty = self.dirty_price();

        // PV of all flows except the last one
        let mut known_pv = 0.0;
        for cf in flows.iter().take(flows.len() - 1) {
            let t = self.year_fraction(cf.date);
            let df = curve.discount_factor(t)?;
            known_pv += cf.amount * df;
        }

        let final_cf = flows.last().unwrap().amount;
        if final_cf <= 0.0 {
            return Ok(0.0);
        }

        Ok((dirty - known_pv) / final_cf)
    }

    fn instrument_type(&self) -> InstrumentType {
        InstrumentType::GovernmentCouponBond
    }

    fn description(&self) -> String {
        let coupon_pct = self.bond.coupon_rate().to_f64().unwrap_or(0.0) * 100.0;
        format!(
            "{} {:.3}% @ {:.3} ({})",
            self.bond.identifier(),
            coupon_pct,
            self.clean_price,
            self.convention
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instruments::FixedBondBuilder;
    use convex_core::types::{Currency, Frequency};
    use convex_curves::curves::DiscountCurveBuilder;
    use convex_curves::interpolation::InterpolationMethod;
    use rust_decimal_macros::dec;

    fn create_test_bond() -> FixedBond {
        FixedBondBuilder::new()
            .isin("GB0009997999")
            .coupon_rate(dec!(0.04)) // 4%
            .maturity(Date::from_ymd(2027, 1, 15).unwrap())
            .frequency(Frequency::SemiAnnual)
            .currency(Currency::GBP)
            .build()
            .unwrap()
    }

    fn flat_curve(ref_date: Date, rate: f64) -> impl Curve {
        DiscountCurveBuilder::new(ref_date)
            .add_zero_rate(0.5, rate)
            .add_zero_rate(1.0, rate)
            .add_zero_rate(2.0, rate)
            .add_zero_rate(5.0, rate)
            .with_interpolation(InterpolationMethod::LogLinear)
            .with_extrapolation()
            .build()
            .unwrap()
    }

    #[test]
    fn test_government_coupon_bond_basic() {
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();

        let gov_bond = GovernmentCouponBond::new(bond, settlement, 98.50, MarketConvention::UKGilt);

        assert_eq!(gov_bond.clean_price(), 98.50);
        assert_eq!(gov_bond.convention(), MarketConvention::UKGilt);
        assert_eq!(
            gov_bond.instrument_type(),
            InstrumentType::GovernmentCouponBond
        );
    }

    #[test]
    fn test_coupon_per_period() {
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();

        let gov_bond = GovernmentCouponBond::new(bond, settlement, 100.0, MarketConvention::UKGilt);

        // 4% annual, semi-annual = 2% per period = 2.0 on 100 face
        assert!((gov_bond.coupon_per_period() - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_cash_flows() {
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();

        let gov_bond = GovernmentCouponBond::new(bond, settlement, 100.0, MarketConvention::UKGilt);

        let flows = gov_bond.cash_flows();

        // 2 years, semi-annual = 4 cash flows
        assert_eq!(flows.len(), 4);

        // First 3 are coupon only
        for cf in flows.iter().take(3) {
            assert!((cf.amount - 2.0).abs() < 1e-10);
        }

        // Last is coupon + principal
        assert!((flows[3].amount - 102.0).abs() < 1e-10);
    }

    #[test]
    fn test_accrued_interest() {
        let bond = FixedBondBuilder::new()
            .isin("GB0009997999")
            .coupon_rate(dec!(0.04))
            .maturity(Date::from_ymd(2027, 1, 15).unwrap())
            .frequency(Frequency::SemiAnnual)
            .currency(Currency::GBP)
            .build()
            .unwrap();

        // Settlement 1 month after last coupon
        let settlement = Date::from_ymd(2025, 8, 15).unwrap();

        let gov_bond = GovernmentCouponBond::new(bond, settlement, 100.0, MarketConvention::UKGilt);

        let accrued = gov_bond.accrued_interest();

        // Should be positive, less than full coupon
        assert!(accrued > 0.0);
        assert!(accrued < 2.0);
    }

    #[test]
    fn test_dirty_price() {
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();

        let gov_bond = GovernmentCouponBond::new(bond, settlement, 98.50, MarketConvention::UKGilt);

        let dirty = gov_bond.dirty_price();
        let clean = gov_bond.clean_price();
        let accrued = gov_bond.accrued_interest();

        assert!((dirty - (clean + accrued)).abs() < 1e-10);
    }

    #[test]
    fn test_pv_near_zero() {
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();

        // At 4% curve, 4% coupon bond should be near par
        let gov_bond = GovernmentCouponBond::new(bond, settlement, 100.0, MarketConvention::UKGilt);

        let curve = flat_curve(settlement, 0.04);
        let pv = gov_bond.pv(&curve).unwrap();

        // Should be close to zero
        assert!(pv.abs() < 2.0);
    }

    #[test]
    fn test_implied_df() {
        let bond = FixedBondBuilder::new()
            .isin("GB0009997999")
            .coupon_rate(dec!(0.04))
            .maturity(Date::from_ymd(2026, 1, 15).unwrap()) // 1 year
            .frequency(Frequency::SemiAnnual)
            .currency(Currency::GBP)
            .build()
            .unwrap();

        let settlement = Date::from_ymd(2025, 1, 15).unwrap();

        let gov_bond = GovernmentCouponBond::new(bond, settlement, 100.0, MarketConvention::UKGilt);

        let curve = flat_curve(settlement, 0.04);
        let implied = gov_bond.implied_df(&curve, 0.0).unwrap();

        // Should be a reasonable DF
        assert!(implied > 0.9);
        assert!(implied < 1.0);
    }

    #[test]
    fn test_from_ytm_at_par() {
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();

        // When coupon = YTM, price should be ~100
        let gov_bond =
            GovernmentCouponBond::from_ytm(bond, settlement, 0.04, MarketConvention::UKGilt);

        // Should be close to par
        assert!((gov_bond.clean_price() - 100.0).abs() < 1.0);
    }

    #[test]
    fn test_from_ytm_discount() {
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();

        // Higher YTM than coupon = discount bond
        let gov_bond =
            GovernmentCouponBond::from_ytm(bond, settlement, 0.06, MarketConvention::UKGilt);

        assert!(gov_bond.clean_price() < 100.0);
    }

    #[test]
    fn test_from_ytm_premium() {
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();

        // Lower YTM than coupon = premium bond
        let gov_bond =
            GovernmentCouponBond::from_ytm(bond, settlement, 0.02, MarketConvention::UKGilt);

        assert!(gov_bond.clean_price() > 100.0);
    }

    #[test]
    fn test_description() {
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();

        let gov_bond = GovernmentCouponBond::new(bond, settlement, 98.50, MarketConvention::UKGilt);

        let desc = gov_bond.description();
        assert!(desc.contains("GB0009997999"));
        assert!(desc.contains("4."));
        assert!(desc.contains("UK Gilt"));
    }

    #[test]
    fn test_german_bund_annual() {
        // Bunds pay annual coupons
        let bond = FixedBondBuilder::new()
            .isin("DE0001102341")
            .coupon_rate(dec!(0.025)) // 2.5%
            .maturity(Date::from_ymd(2030, 1, 15).unwrap())
            .frequency(Frequency::Annual)
            .currency(Currency::EUR)
            .build()
            .unwrap();

        let settlement = Date::from_ymd(2025, 1, 15).unwrap();

        let bund = GovernmentCouponBond::new(bond, settlement, 100.0, MarketConvention::GermanBund);

        // 5 years, annual = 5 cash flows
        let flows = bund.cash_flows();
        assert_eq!(flows.len(), 5);

        // Coupon per period should be 2.5 (annual coupon)
        assert!((bund.coupon_per_period() - 2.5).abs() < 1e-10);
    }
}
