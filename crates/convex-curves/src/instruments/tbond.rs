//! Treasury Note/Bond instrument.
//!
//! Treasury Notes (2-10 year) and Bonds (20-30 year) are coupon-bearing
//! instruments used to construct the Treasury curve.

use convex_core::types::Frequency;
use convex_core::Date;

use super::quotes::{BondQuoteType, MarketQuote, QuoteType};
use super::{CurveInstrument, InstrumentType};
use crate::error::{CurveError, CurveResult};
use crate::traits::Curve;

/// A cash flow (coupon or principal payment).
#[derive(Debug, Clone, Copy)]
pub struct CashFlow {
    /// Payment date
    pub date: Date,
    /// Payment amount
    pub amount: f64,
}

impl CashFlow {
    /// Creates a new cash flow.
    #[must_use]
    pub fn new(date: Date, amount: f64) -> Self {
        Self { date, amount }
    }
}

/// Treasury Note or Bond.
///
/// Treasury Notes (2-10 year maturities) and Bonds (20-30 year) are
/// coupon-bearing securities issued by the US Treasury.
///
/// # Pricing
///
/// ```text
/// Dirty Price = Σ Coupon(i) × DF(Ti) + Face × DF(Tn)
/// Clean Price = Dirty Price - Accrued Interest
/// ```
///
/// # Day Count
///
/// US Treasuries use ACT/ACT ICMA for accrued interest calculations.
///
/// # Bootstrap
///
/// For curve construction, the unknown DF at maturity is solved from:
/// ```text
/// Dirty = Known_PV + (Coupon + Face) × DF(Tn)
/// DF(Tn) = (Dirty - Known_PV) / (Coupon + Face)
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::instruments::TreasuryBond;
///
/// // 10-year Treasury Note at 4.125% coupon, priced at 98.50
/// let tbond = TreasuryBond::new(
///     "912810TW",
///     settlement_date,
///     maturity_date,
///     0.04125,
///     98.50,
/// );
/// ```
#[derive(Debug, Clone)]
pub struct TreasuryBond {
    /// CUSIP identifier
    cusip: String,
    /// Settlement date
    settlement_date: Date,
    /// Maturity date
    maturity_date: Date,
    /// Annual coupon rate (e.g., 0.04125 for 4.125%)
    coupon_rate: f64,
    /// Payment frequency (always `SemiAnnual` for US Treasuries)
    frequency: Frequency,
    /// Clean price per 100 face
    clean_price: f64,
    /// Face value (typically 100)
    face_value: f64,
}

impl TreasuryBond {
    /// Creates a new Treasury Note/Bond.
    ///
    /// # Arguments
    ///
    /// * `cusip` - CUSIP identifier
    /// * `settlement_date` - Settlement date
    /// * `maturity_date` - Maturity date
    /// * `coupon_rate` - Annual coupon rate (e.g., 0.04125)
    /// * `clean_price` - Clean price per 100 face
    pub fn new(
        cusip: impl Into<String>,
        settlement_date: Date,
        maturity_date: Date,
        coupon_rate: f64,
        clean_price: f64,
    ) -> Self {
        Self {
            cusip: cusip.into(),
            settlement_date,
            maturity_date,
            coupon_rate,
            frequency: Frequency::SemiAnnual,
            clean_price,
            face_value: 100.0,
        }
    }

    /// Creates a Treasury with custom face value.
    #[must_use]
    pub fn with_face_value(mut self, face_value: f64) -> Self {
        self.face_value = face_value;
        self
    }

    /// Creates a Treasury Bond from yield to maturity.
    ///
    /// Converts YTM to clean price using standard bond pricing:
    /// ```text
    /// Price = Σ Coupon(i) / (1 + y/2)^(2*ti) + Face / (1 + y/2)^(2*tn)
    /// ```
    ///
    /// # Arguments
    ///
    /// * `cusip` - CUSIP identifier
    /// * `settlement_date` - Settlement date
    /// * `maturity_date` - Maturity date
    /// * `coupon_rate` - Annual coupon rate (e.g., 0.04125)
    /// * `ytm` - Yield to maturity (e.g., 0.05 for 5%)
    #[must_use]
    pub fn from_ytm(
        cusip: impl Into<String>,
        settlement_date: Date,
        maturity_date: Date,
        coupon_rate: f64,
        ytm: f64,
    ) -> Self {
        // Create a temporary bond at par to get cash flows
        let temp_bond = Self::new(
            cusip.into(),
            settlement_date,
            maturity_date,
            coupon_rate,
            100.0,
        );
        let flows = temp_bond.cash_flows();

        // Calculate dirty price from YTM
        let y_semi = ytm / 2.0; // Semi-annual yield
        let mut dirty_price = 0.0;

        for cf in &flows {
            let t = temp_bond.year_fraction(cf.date);
            // Number of semi-annual periods (can be fractional)
            let n = t * 2.0;
            let df = (1.0 + y_semi).powf(-n);
            dirty_price += cf.amount * df;
        }

        // Clean price = Dirty price - Accrued interest
        let clean_price = dirty_price - temp_bond.accrued_interest();

        Self::new(
            temp_bond.cusip,
            settlement_date,
            maturity_date,
            coupon_rate,
            clean_price,
        )
    }

    /// Creates a Treasury Bond from a market quote.
    ///
    /// Supports clean price, dirty price, and yield to maturity quotes.
    ///
    /// # Arguments
    ///
    /// * `cusip` - CUSIP identifier
    /// * `settlement_date` - Settlement date
    /// * `maturity_date` - Maturity date
    /// * `coupon_rate` - Annual coupon rate
    /// * `quote` - Market quote
    ///
    /// # Errors
    ///
    /// Returns an error if the quote type is not supported for Treasury Bonds.
    pub fn from_quote(
        cusip: impl Into<String>,
        settlement_date: Date,
        maturity_date: Date,
        coupon_rate: f64,
        quote: &MarketQuote,
    ) -> CurveResult<Self> {
        let cusip = cusip.into();
        match quote.quote_type {
            QuoteType::Bond(BondQuoteType::CleanPrice) => Ok(Self::new(
                cusip,
                settlement_date,
                maturity_date,
                coupon_rate,
                quote.mid(),
            )),
            QuoteType::Bond(BondQuoteType::DirtyPrice) => {
                // Convert dirty to clean by subtracting accrued
                let temp_bond = Self::new(
                    cusip.clone(),
                    settlement_date,
                    maturity_date,
                    coupon_rate,
                    100.0,
                );
                let accrued = temp_bond.accrued_interest();
                let clean_price = quote.mid() - accrued;
                Ok(Self::new(
                    cusip,
                    settlement_date,
                    maturity_date,
                    coupon_rate,
                    clean_price,
                ))
            }
            QuoteType::Bond(BondQuoteType::YieldToMaturity) => Ok(Self::from_ytm(
                cusip,
                settlement_date,
                maturity_date,
                coupon_rate,
                quote.mid(),
            )),
            QuoteType::Bond(BondQuoteType::DiscountRate) => Err(CurveError::invalid_data(
                "Discount rate not supported for coupon bonds. Use YTM or Clean Price.",
            )),
            _ => Err(CurveError::invalid_data(format!(
                "Unsupported quote type {:?} for Treasury Bond",
                quote.quote_type
            ))),
        }
    }

    /// Returns the CUSIP.
    #[must_use]
    pub fn cusip(&self) -> &str {
        &self.cusip
    }

    /// Returns the settlement date.
    #[must_use]
    pub fn settlement_date(&self) -> Date {
        self.settlement_date
    }

    /// Returns the maturity date.
    #[must_use]
    pub fn maturity_date(&self) -> Date {
        self.maturity_date
    }

    /// Returns the coupon rate.
    #[must_use]
    pub fn coupon_rate(&self) -> f64 {
        self.coupon_rate
    }

    /// Returns the frequency.
    #[must_use]
    pub fn frequency(&self) -> Frequency {
        self.frequency
    }

    /// Returns the clean price.
    #[must_use]
    pub fn clean_price(&self) -> f64 {
        self.clean_price
    }

    /// Returns the face value.
    #[must_use]
    pub fn face_value(&self) -> f64 {
        self.face_value
    }

    /// Returns the semi-annual coupon amount.
    #[must_use]
    pub fn coupon_amount(&self) -> f64 {
        self.face_value * self.coupon_rate / f64::from(self.frequency.periods_per_year())
    }

    /// Generates all cash flows after settlement.
    #[must_use]
    pub fn cash_flows(&self) -> Vec<CashFlow> {
        let coupon = self.coupon_amount();
        let months_per_period = self.frequency.months_per_period() as i32;

        let mut flows = Vec::new();
        let mut date = self.maturity_date;

        // Walk backwards from maturity
        while date > self.settlement_date {
            let amount = if date == self.maturity_date {
                coupon + self.face_value
            } else {
                coupon
            };
            flows.push(CashFlow::new(date, amount));

            // Go back one period
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
        let months_per_period = self.frequency.months_per_period() as i32;
        let mut next_coupon = self.maturity_date;

        // Walk back from maturity to find the next coupon after settlement
        while next_coupon > self.settlement_date {
            if let Ok(prev) = next_coupon.add_months(-months_per_period) {
                if prev <= self.settlement_date {
                    break;
                }
                next_coupon = prev;
            } else {
                break;
            }
        }

        let prev_coupon = next_coupon
            .add_months(-months_per_period)
            .unwrap_or(self.settlement_date);

        (prev_coupon, next_coupon)
    }

    /// Calculates accrued interest using ACT/ACT.
    ///
    /// Accrued = Coupon × (Days since last coupon / Days in period)
    #[must_use]
    pub fn accrued_interest(&self) -> f64 {
        let (prev_coupon, next_coupon) = self.coupon_dates();

        let days_accrued = prev_coupon.days_between(&self.settlement_date) as f64;
        let days_in_period = prev_coupon.days_between(&next_coupon) as f64;

        if days_in_period <= 0.0 {
            return 0.0;
        }

        self.coupon_amount() * (days_accrued / days_in_period)
    }

    /// Returns the dirty price (clean + accrued).
    #[must_use]
    pub fn dirty_price(&self) -> f64 {
        self.clean_price + self.accrued_interest()
    }

    /// Calculates year fraction from settlement to date using ACT/365.
    fn year_fraction(&self, date: Date) -> f64 {
        self.settlement_date.days_between(&date) as f64 / 365.0
    }
}

impl CurveInstrument for TreasuryBond {
    fn maturity(&self) -> Date {
        self.maturity_date
    }

    fn pillar_date(&self) -> Date {
        self.maturity_date
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
            return Ok(self.dirty_price() / self.face_value);
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
        InstrumentType::TreasuryBond
    }

    fn description(&self) -> String {
        let years = self.settlement_date.days_between(&self.maturity_date) as f64 / 365.0;
        let term = if years > 10.0 { "Bond" } else { "Note" };
        format!(
            "T-{} {} {:.3}% @ {:.3}",
            term,
            self.cusip,
            self.coupon_rate * 100.0,
            self.clean_price
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curves::DiscountCurveBuilder;
    use crate::interpolation::InterpolationMethod;
    use approx::assert_relative_eq;

    fn flat_curve(ref_date: Date, rate: f64) -> impl Curve {
        DiscountCurveBuilder::new(ref_date)
            .add_zero_rate(0.5, rate)
            .add_zero_rate(1.0, rate)
            .add_zero_rate(2.0, rate)
            .add_zero_rate(5.0, rate)
            .add_zero_rate(10.0, rate)
            .with_interpolation(InterpolationMethod::LogLinear)
            .with_extrapolation()
            .build()
            .unwrap()
    }

    #[test]
    fn test_tbond_basic() {
        let settle = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2035, 1, 15).unwrap();

        let tbond = TreasuryBond::new("912810TW", settle, maturity, 0.04125, 98.50);

        assert_eq!(tbond.cusip(), "912810TW");
        assert_eq!(tbond.settlement_date(), settle);
        assert_eq!(tbond.maturity_date(), maturity);
        assert_eq!(tbond.coupon_rate(), 0.04125);
        assert_eq!(tbond.clean_price(), 98.50);
        assert_eq!(tbond.instrument_type(), InstrumentType::TreasuryBond);
    }

    #[test]
    fn test_tbond_coupon_amount() {
        let settle = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2027, 1, 15).unwrap();

        let tbond = TreasuryBond::new("912810TW", settle, maturity, 0.04, 100.0);

        // 4% annual, semi-annual = 2% per period
        assert_relative_eq!(tbond.coupon_amount(), 2.0, epsilon = 1e-10);
    }

    #[test]
    fn test_tbond_cash_flows() {
        let settle = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2027, 1, 15).unwrap();

        let tbond = TreasuryBond::new("912810TW", settle, maturity, 0.04, 100.0);
        let flows = tbond.cash_flows();

        // 2 years, semi-annual = 4 cash flows
        assert_eq!(flows.len(), 4);

        // First 3 flows are coupon only
        for cf in flows.iter().take(3) {
            assert_relative_eq!(cf.amount, 2.0, epsilon = 1e-10);
        }

        // Last flow is coupon + principal
        assert_relative_eq!(flows[3].amount, 102.0, epsilon = 1e-10);
    }

    #[test]
    fn test_tbond_accrued_interest() {
        // Settlement is 1 month after last coupon (July 15)
        let settle = Date::from_ymd(2025, 8, 15).unwrap();
        let maturity = Date::from_ymd(2027, 1, 15).unwrap();

        let tbond = TreasuryBond::new("912810TW", settle, maturity, 0.04, 100.0);
        let accrued = tbond.accrued_interest();

        // Approximately 1 month of accrual on 2.0 semi-annual coupon
        // ~31 days / ~184 days in period ≈ 0.17
        assert!(accrued > 0.0);
        assert!(accrued < 2.0); // Less than full coupon
    }

    #[test]
    fn test_tbond_dirty_price() {
        let settle = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2027, 1, 15).unwrap();

        let tbond = TreasuryBond::new("912810TW", settle, maturity, 0.04, 100.0);

        let dirty = tbond.dirty_price();
        let clean = tbond.clean_price();
        let accrued = tbond.accrued_interest();

        assert_relative_eq!(dirty, clean + accrued, epsilon = 1e-10);
    }

    #[test]
    fn test_tbond_implied_df() {
        let settle = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2026, 1, 15).unwrap(); // 1 year

        // At par (coupon = yield)
        let tbond = TreasuryBond::new("912810TW", settle, maturity, 0.04, 100.0);

        let curve = flat_curve(settle, 0.04);
        let implied = tbond.implied_df(&curve, 0.0).unwrap();

        // Should be close to the 1-year DF
        assert!(implied > 0.9);
        assert!(implied < 1.0);
    }

    #[test]
    fn test_tbond_pv_near_zero() {
        let settle = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2027, 1, 15).unwrap();

        // At a 4% curve, a 4% coupon bond should be near par
        let tbond = TreasuryBond::new("912810TW", settle, maturity, 0.04, 100.0);
        let curve = flat_curve(settle, 0.04);

        let pv = tbond.pv(&curve).unwrap();

        // PV should be close to zero for at-par bond
        assert!(pv.abs() < 2.0); // Within $2 of zero
    }

    #[test]
    fn test_tbond_pv_discount() {
        let settle = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2027, 1, 15).unwrap();

        // 4% coupon priced at discount (95)
        let tbond = TreasuryBond::new("912810TW", settle, maturity, 0.04, 95.0);

        // 6% curve (higher than coupon)
        let curve = flat_curve(settle, 0.06);

        let pv = tbond.pv(&curve).unwrap();

        // Bond priced at 95 is "cheap" vs theoretical (~96-97 at 6% curve)
        // Theoretical > Dirty, so PV = Theoretical - Dirty > 0
        assert!(pv > 0.0);
    }

    #[test]
    fn test_tbond_description() {
        let settle = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2027, 1, 15).unwrap();

        let tbond = TreasuryBond::new("912810TW", settle, maturity, 0.04125, 98.50);
        let desc = tbond.description();

        assert!(desc.contains("T-Note"));
        assert!(desc.contains("912810TW"));
        assert!(desc.contains("4.125%"));
    }

    #[test]
    fn test_tbond_long_maturity_is_bond() {
        let settle = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2055, 1, 15).unwrap(); // 30 years

        let tbond = TreasuryBond::new("912810TW", settle, maturity, 0.04, 100.0);
        let desc = tbond.description();

        assert!(desc.contains("T-Bond"));
    }

    #[test]
    fn test_tbond_from_ytm_at_par() {
        let settle = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2027, 1, 15).unwrap(); // 2 years

        // When coupon = YTM, price should be ~100
        let tbond = TreasuryBond::from_ytm("912810TW", settle, maturity, 0.04, 0.04);

        // Should be close to par (100), allowing for day count differences
        assert!((tbond.clean_price() - 100.0).abs() < 1.0);
    }

    #[test]
    fn test_tbond_from_ytm_discount() {
        let settle = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2027, 1, 15).unwrap();

        // Higher YTM than coupon = discount bond
        let tbond = TreasuryBond::from_ytm("912810TW", settle, maturity, 0.04, 0.06);

        // Price should be below par
        assert!(tbond.clean_price() < 100.0);
    }

    #[test]
    fn test_tbond_from_ytm_premium() {
        let settle = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2027, 1, 15).unwrap();

        // Lower YTM than coupon = premium bond
        let tbond = TreasuryBond::from_ytm("912810TW", settle, maturity, 0.04, 0.02);

        // Price should be above par
        assert!(tbond.clean_price() > 100.0);
    }

    #[test]
    fn test_tbond_from_quote_clean_price() {
        use crate::instruments::quotes::MarketQuote;

        let settle = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2027, 1, 15).unwrap();

        let quote = MarketQuote::clean_price(98.50);
        let tbond = TreasuryBond::from_quote("912810TW", settle, maturity, 0.04, &quote).unwrap();

        assert_relative_eq!(tbond.clean_price(), 98.50, epsilon = 1e-10);
    }

    #[test]
    fn test_tbond_from_quote_dirty_price() {
        use crate::instruments::quotes::{BondQuoteType, MarketQuote, QuoteType};

        let settle = Date::from_ymd(2025, 7, 15).unwrap(); // Mid-period
        let maturity = Date::from_ymd(2027, 1, 15).unwrap();

        // First create bond at clean price to calculate expected accrued
        let temp_bond = TreasuryBond::new("912810TW", settle, maturity, 0.04, 100.0);
        let accrued = temp_bond.accrued_interest();

        // Quote dirty price = clean + accrued
        let dirty = 98.50 + accrued;
        let quote = MarketQuote::new(dirty, QuoteType::Bond(BondQuoteType::DirtyPrice));
        let tbond = TreasuryBond::from_quote("912810TW", settle, maturity, 0.04, &quote).unwrap();

        // Clean price should be 98.50
        assert_relative_eq!(tbond.clean_price(), 98.50, epsilon = 0.01);
    }

    #[test]
    fn test_tbond_from_quote_ytm() {
        use crate::instruments::quotes::MarketQuote;

        let settle = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2027, 1, 15).unwrap();

        let quote = MarketQuote::ytm(0.05);
        let tbond = TreasuryBond::from_quote("912810TW", settle, maturity, 0.04, &quote).unwrap();

        // Should match from_ytm
        let tbond2 = TreasuryBond::from_ytm("912810TW", settle, maturity, 0.04, 0.05);
        assert_relative_eq!(tbond.clean_price(), tbond2.clean_price(), epsilon = 1e-10);
    }

    #[test]
    fn test_tbond_from_quote_with_bid_ask() {
        use crate::instruments::quotes::MarketQuote;

        let settle = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2027, 1, 15).unwrap();

        let quote = MarketQuote::clean_price(98.50).with_bid_ask(98.25, 98.75);
        let tbond = TreasuryBond::from_quote("912810TW", settle, maturity, 0.04, &quote).unwrap();

        // Should use mid price
        assert_relative_eq!(tbond.clean_price(), 98.50, epsilon = 1e-10);
    }

    #[test]
    fn test_tbond_from_quote_unsupported_type() {
        use crate::instruments::quotes::{MarketQuote, QuoteType, RateQuoteType};

        let settle = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2027, 1, 15).unwrap();

        let quote = MarketQuote::new(0.05, QuoteType::Rate(RateQuoteType::Simple));
        let result = TreasuryBond::from_quote("912810TW", settle, maturity, 0.04, &quote);

        assert!(result.is_err());
    }

    #[test]
    fn test_tbond_from_quote_discount_rate_unsupported() {
        use crate::instruments::quotes::{BondQuoteType, MarketQuote, QuoteType};

        let settle = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2027, 1, 15).unwrap();

        // Discount rate doesn't make sense for coupon bonds
        let quote = MarketQuote::new(0.05, QuoteType::Bond(BondQuoteType::DiscountRate));
        let result = TreasuryBond::from_quote("912810TW", settle, maturity, 0.04, &quote);

        assert!(result.is_err());
    }
}
