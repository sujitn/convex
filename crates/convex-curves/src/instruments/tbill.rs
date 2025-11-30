//! Treasury Bill (T-Bill) instrument.
//!
//! T-Bills are discount instruments used for the short end of the Treasury curve.

use convex_core::Date;

use super::quotes::{BondQuoteType, MarketQuote, QuoteType};
use super::{year_fraction_act360, CurveInstrument, InstrumentType};
use crate::error::{CurveError, CurveResult};
use crate::traits::Curve;

/// Treasury Bill.
///
/// T-Bills are zero-coupon discount instruments issued by the US Treasury
/// with maturities of 4, 8, 13, 17, 26, or 52 weeks.
///
/// # Pricing
///
/// T-Bills are quoted on a discount basis:
/// ```text
/// Price = Face × (1 - Discount_Rate × Days / 360)
/// ```
///
/// Or equivalently:
/// ```text
/// Discount Factor = Price / Face
/// ```
///
/// # Yield Conventions
///
/// - **Bank Discount Rate**: `d = (Face - Price) / Face × (360 / Days)`
/// - **Bond Equivalent Yield**: `BEY = (Face - Price) / Price × (365 / Days)`
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::instruments::TreasuryBill;
///
/// // 13-week T-Bill at 99.50
/// let tbill = TreasuryBill::new(
///     "912796XY",          // CUSIP
///     settlement_date,
///     maturity_date,
///     99.50,               // Price per 100 face
/// );
///
/// let df = tbill.implied_df(&curve, 0.0)?;  // ≈ 0.995
/// ```
#[derive(Debug, Clone)]
pub struct TreasuryBill {
    /// CUSIP identifier
    cusip: String,
    /// Settlement date
    settlement_date: Date,
    /// Maturity date
    maturity_date: Date,
    /// Price per 100 face value
    price: f64,
    /// Face value (typically 100)
    face_value: f64,
}

impl TreasuryBill {
    /// Creates a new T-Bill.
    ///
    /// # Arguments
    ///
    /// * `cusip` - CUSIP identifier
    /// * `settlement_date` - Settlement date
    /// * `maturity_date` - Maturity date
    /// * `price` - Price per 100 face value
    pub fn new(
        cusip: impl Into<String>,
        settlement_date: Date,
        maturity_date: Date,
        price: f64,
    ) -> Self {
        Self {
            cusip: cusip.into(),
            settlement_date,
            maturity_date,
            price,
            face_value: 100.0,
        }
    }

    /// Creates a T-Bill with custom face value.
    #[must_use]
    pub fn with_face_value(mut self, face_value: f64) -> Self {
        self.face_value = face_value;
        self
    }

    /// Creates a T-Bill from a discount rate.
    ///
    /// Converts the bank discount rate to price:
    /// `Price = Face × (1 - Discount_Rate × Days / 360)`
    ///
    /// # Arguments
    ///
    /// * `cusip` - CUSIP identifier
    /// * `settlement_date` - Settlement date
    /// * `maturity_date` - Maturity date
    /// * `discount_rate` - Bank discount rate (e.g., 0.05 for 5%)
    pub fn from_discount_rate(
        cusip: impl Into<String>,
        settlement_date: Date,
        maturity_date: Date,
        discount_rate: f64,
    ) -> Self {
        let days = settlement_date.days_between(&maturity_date) as f64;
        let price = 100.0 * (1.0 - discount_rate * days / 360.0);
        Self::new(cusip, settlement_date, maturity_date, price)
    }

    /// Creates a T-Bill from a market quote.
    ///
    /// Supports both price and discount rate quote types.
    ///
    /// # Arguments
    ///
    /// * `cusip` - CUSIP identifier
    /// * `settlement_date` - Settlement date
    /// * `maturity_date` - Maturity date
    /// * `quote` - Market quote (price or discount rate)
    ///
    /// # Errors
    ///
    /// Returns an error if the quote type is not supported for T-Bills.
    pub fn from_quote(
        cusip: impl Into<String>,
        settlement_date: Date,
        maturity_date: Date,
        quote: &MarketQuote,
    ) -> CurveResult<Self> {
        let cusip = cusip.into();
        match quote.quote_type {
            QuoteType::Bond(BondQuoteType::CleanPrice) | QuoteType::Bond(BondQuoteType::DirtyPrice) => {
                // For T-Bills, clean = dirty (no coupon)
                Ok(Self::new(cusip, settlement_date, maturity_date, quote.mid()))
            }
            QuoteType::Bond(BondQuoteType::DiscountRate) => {
                Ok(Self::from_discount_rate(cusip, settlement_date, maturity_date, quote.mid()))
            }
            QuoteType::Bond(BondQuoteType::YieldToMaturity) => {
                // For T-Bills, convert BEY to price
                // BEY = (Face - Price) / Price × (365 / Days)
                // Price = Face / (1 + BEY × Days / 365)
                let days = settlement_date.days_between(&maturity_date) as f64;
                let price = 100.0 / (1.0 + quote.mid() * days / 365.0);
                Ok(Self::new(cusip, settlement_date, maturity_date, price))
            }
            _ => Err(CurveError::invalid_data(format!(
                "Unsupported quote type {:?} for T-Bill",
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

    /// Returns the price.
    #[must_use]
    pub fn price(&self) -> f64 {
        self.price
    }

    /// Returns the face value.
    #[must_use]
    pub fn face_value(&self) -> f64 {
        self.face_value
    }

    /// Returns the number of days to maturity.
    #[must_use]
    pub fn days_to_maturity(&self) -> i64 {
        self.settlement_date.days_between(&self.maturity_date)
    }

    /// Returns the discount factor.
    ///
    /// DF = Price / Face
    #[must_use]
    pub fn discount_factor(&self) -> f64 {
        self.price / self.face_value
    }

    /// Returns the bank discount rate.
    ///
    /// `d = (Face - Price) / Face × (360 / Days)`
    #[must_use]
    pub fn discount_rate(&self) -> f64 {
        let days = self.days_to_maturity() as f64;
        if days <= 0.0 {
            return 0.0;
        }
        (self.face_value - self.price) / self.face_value * (360.0 / days)
    }

    /// Returns the bond equivalent yield (BEY).
    ///
    /// `BEY = (Face - Price) / Price × (365 / Days)`
    #[must_use]
    pub fn bond_equivalent_yield(&self) -> f64 {
        let days = self.days_to_maturity() as f64;
        if days <= 0.0 || self.price <= 0.0 {
            return 0.0;
        }
        (self.face_value - self.price) / self.price * (365.0 / days)
    }

    /// Returns the money market yield.
    ///
    /// `MMY = (Face - Price) / Price × (360 / Days)`
    #[must_use]
    pub fn money_market_yield(&self) -> f64 {
        let days = self.days_to_maturity() as f64;
        if days <= 0.0 || self.price <= 0.0 {
            return 0.0;
        }
        (self.face_value - self.price) / self.price * (360.0 / days)
    }

    /// Returns the continuously compounded zero rate.
    #[must_use]
    pub fn zero_rate(&self) -> f64 {
        let df = self.discount_factor();
        if df <= 0.0 {
            return 0.0;
        }
        let t = self.days_to_maturity() as f64 / 365.0;
        if t <= 0.0 {
            return 0.0;
        }
        -df.ln() / t
    }
}

impl CurveInstrument for TreasuryBill {
    fn maturity(&self) -> Date {
        self.maturity_date
    }

    fn pillar_date(&self) -> Date {
        self.maturity_date
    }

    fn pv(&self, curve: &dyn Curve) -> CurveResult<f64> {
        // PV = Face × DF(maturity) - Price
        let ref_date = curve.reference_date();
        let t = year_fraction_act360(ref_date, self.maturity_date);
        let df = curve.discount_factor(t)?;

        let theoretical = self.face_value * df;
        Ok(theoretical - self.price)
    }

    fn implied_df(&self, _curve: &dyn Curve, _target_pv: f64) -> CurveResult<f64> {
        // Direct calculation: DF = Price / Face
        Ok(self.discount_factor())
    }

    fn instrument_type(&self) -> InstrumentType {
        InstrumentType::TreasuryBill
    }

    fn description(&self) -> String {
        let weeks = self.days_to_maturity() / 7;
        format!(
            "T-Bill {} {}W @ {:.3} ({:.2}% BEY)",
            self.cusip,
            weeks,
            self.price,
            self.bond_equivalent_yield() * 100.0
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_tbill_basic() {
        let settle = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2025, 4, 15).unwrap();

        let tbill = TreasuryBill::new("912796XY", settle, maturity, 99.50);

        assert_eq!(tbill.cusip(), "912796XY");
        assert_eq!(tbill.settlement_date(), settle);
        assert_eq!(tbill.maturity_date(), maturity);
        assert_eq!(tbill.price(), 99.50);
        assert_eq!(tbill.face_value(), 100.0);
        assert_eq!(tbill.instrument_type(), InstrumentType::TreasuryBill);
    }

    #[test]
    fn test_tbill_discount_factor() {
        let settle = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2025, 4, 15).unwrap();

        let tbill = TreasuryBill::new("912796XY", settle, maturity, 99.50);

        // DF = 99.50 / 100 = 0.995
        assert_relative_eq!(tbill.discount_factor(), 0.995, epsilon = 1e-10);
    }

    #[test]
    fn test_tbill_discount_rate() {
        let settle = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2025, 4, 15).unwrap(); // 90 days

        let tbill = TreasuryBill::new("912796XY", settle, maturity, 99.50);

        // d = (100 - 99.50) / 100 × (360 / 90) = 0.005 × 4 = 0.02 = 2%
        let days = tbill.days_to_maturity() as f64;
        let expected = 0.50 / 100.0 * (360.0 / days);
        assert_relative_eq!(tbill.discount_rate(), expected, epsilon = 1e-6);
    }

    #[test]
    fn test_tbill_bond_equivalent_yield() {
        let settle = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2025, 4, 15).unwrap(); // 90 days

        let tbill = TreasuryBill::new("912796XY", settle, maturity, 99.50);

        // BEY = (100 - 99.50) / 99.50 × (365 / 90)
        let days = tbill.days_to_maturity() as f64;
        let expected = 0.50 / 99.50 * (365.0 / days);
        assert_relative_eq!(tbill.bond_equivalent_yield(), expected, epsilon = 1e-6);
    }

    #[test]
    fn test_tbill_zero_rate() {
        let settle = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2025, 4, 15).unwrap();

        let tbill = TreasuryBill::new("912796XY", settle, maturity, 99.50);

        // Zero rate = -ln(DF) / t
        let df = tbill.discount_factor();
        let t = tbill.days_to_maturity() as f64 / 365.0;
        let expected = -df.ln() / t;

        assert_relative_eq!(tbill.zero_rate(), expected, epsilon = 1e-10);
    }

    #[test]
    fn test_tbill_implied_df() {
        let settle = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2025, 4, 15).unwrap();

        let tbill = TreasuryBill::new("912796XY", settle, maturity, 99.50);

        // Build a dummy curve (needs at least 2 pillars)
        use crate::curves::DiscountCurveBuilder;
        use crate::interpolation::InterpolationMethod;

        let curve = DiscountCurveBuilder::new(settle)
            .add_pillar(0.0, 1.0)
            .add_pillar(0.5, 0.99)
            .with_interpolation(InterpolationMethod::LogLinear)
            .build()
            .unwrap();

        let implied = tbill.implied_df(&curve, 0.0).unwrap();
        assert_relative_eq!(implied, 0.995, epsilon = 1e-10);
    }

    #[test]
    fn test_tbill_pv_at_par() {
        let settle = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2025, 4, 15).unwrap();

        let tbill = TreasuryBill::new("912796XY", settle, maturity, 99.50);

        // Build curve with DF matching the T-Bill price (needs at least 2 pillars)
        use crate::curves::DiscountCurveBuilder;
        use crate::interpolation::InterpolationMethod;

        let t = tbill.days_to_maturity() as f64 / 360.0;
        let curve = DiscountCurveBuilder::new(settle)
            .add_pillar(0.0, 1.0)
            .add_pillar(t, 0.995)
            .with_interpolation(InterpolationMethod::LogLinear)
            .build()
            .unwrap();

        let pv = tbill.pv(&curve).unwrap();
        // PV = 100 × 0.995 - 99.50 = 0
        assert_relative_eq!(pv, 0.0, epsilon = 0.01);
    }

    #[test]
    fn test_tbill_from_discount_rate() {
        let settle = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2025, 4, 15).unwrap(); // ~90 days

        // Create from discount rate
        let tbill = TreasuryBill::from_discount_rate("912796XY", settle, maturity, 0.02);

        // Verify price: Price = 100 × (1 - 0.02 × 90/360) = 100 × 0.995 = 99.50
        let days = settle.days_between(&maturity) as f64;
        let expected_price = 100.0 * (1.0 - 0.02 * days / 360.0);
        assert_relative_eq!(tbill.price(), expected_price, epsilon = 1e-10);

        // Verify round-trip: discount_rate should return ~0.02
        assert_relative_eq!(tbill.discount_rate(), 0.02, epsilon = 1e-10);
    }

    #[test]
    fn test_tbill_from_quote_price() {
        use crate::instruments::quotes::MarketQuote;

        let settle = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2025, 4, 15).unwrap();

        let quote = MarketQuote::clean_price(99.50);
        let tbill = TreasuryBill::from_quote("912796XY", settle, maturity, &quote).unwrap();

        assert_relative_eq!(tbill.price(), 99.50, epsilon = 1e-10);
    }

    #[test]
    fn test_tbill_from_quote_discount_rate() {
        use crate::instruments::quotes::{BondQuoteType, MarketQuote, QuoteType};

        let settle = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2025, 4, 15).unwrap();

        let quote = MarketQuote::new(0.02, QuoteType::Bond(BondQuoteType::DiscountRate));
        let tbill = TreasuryBill::from_quote("912796XY", settle, maturity, &quote).unwrap();

        // Should match from_discount_rate
        let tbill2 = TreasuryBill::from_discount_rate("912796XY", settle, maturity, 0.02);
        assert_relative_eq!(tbill.price(), tbill2.price(), epsilon = 1e-10);
    }

    #[test]
    fn test_tbill_from_quote_bey() {
        use crate::instruments::quotes::MarketQuote;

        let settle = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2025, 4, 15).unwrap();

        // First create a T-Bill from price to get its BEY
        let tbill1 = TreasuryBill::new("912796XY", settle, maturity, 99.50);
        let bey = tbill1.bond_equivalent_yield();

        // Now create from BEY quote
        let quote = MarketQuote::ytm(bey);
        let tbill2 = TreasuryBill::from_quote("912796XY", settle, maturity, &quote).unwrap();

        // Prices should be close (not exact due to approximation)
        assert_relative_eq!(tbill2.price(), 99.50, epsilon = 0.01);
    }

    #[test]
    fn test_tbill_from_quote_with_bid_ask() {
        use crate::instruments::quotes::MarketQuote;

        let settle = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2025, 4, 15).unwrap();

        // Quote with bid/ask spread
        let quote = MarketQuote::clean_price(99.50).with_bid_ask(99.48, 99.52);
        let tbill = TreasuryBill::from_quote("912796XY", settle, maturity, &quote).unwrap();

        // Should use mid price (99.50)
        assert_relative_eq!(tbill.price(), 99.50, epsilon = 1e-10);
    }

    #[test]
    fn test_tbill_from_quote_unsupported_type() {
        use crate::instruments::quotes::{MarketQuote, QuoteType, RateQuoteType};

        let settle = Date::from_ymd(2025, 1, 15).unwrap();
        let maturity = Date::from_ymd(2025, 4, 15).unwrap();

        // Rate quote type not supported for T-Bills
        let quote = MarketQuote::new(0.05, QuoteType::Rate(RateQuoteType::Simple));
        let result = TreasuryBill::from_quote("912796XY", settle, maturity, &quote);

        assert!(result.is_err());
    }
}
