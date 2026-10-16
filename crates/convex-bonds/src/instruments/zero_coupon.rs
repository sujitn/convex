//! Zero coupon bond implementation.
//!
//! Provides a comprehensive zero coupon bond implementation with:
//! - Multiple compounding conventions
//! - Price-yield conversions
//! - Yield conversion between compounding conventions
//! - Full Bond trait implementation

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use convex_core::contracts::ExactDecimal;
use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Currency, Date, Frequency};

use crate::error::{BondError, BondResult};
use crate::pricing::discount_time::{DatedYieldCash, DiscountTimeBasis, PreparedDatedCash};
use crate::traits::{Bond, BondCashFlow};
use crate::types::{BondIdentifiers, BondType, CalendarId, Cusip, Isin};

/// Compounding convention for yield calculations.
///
/// Determines how yields are annualized and how present values are computed.
///
/// # Example
///
/// ```rust
/// use convex_bonds::instruments::Compounding;
///
/// // Convert yield from semi-annual to continuous
/// use convex_bonds::instruments::convert_yield;
/// use rust_decimal_macros::dec;
///
/// let semi_annual_yield = dec!(0.05); // 5%
/// let continuous_yield = convert_yield(
///     semi_annual_yield,
///     Compounding::SemiAnnual,
///     Compounding::Continuous,
/// ).unwrap();
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Compounding {
    /// Annual compounding (1 period per year)
    #[default]
    Annual,
    /// Semi-annual compounding (2 periods per year)
    SemiAnnual,
    /// Quarterly compounding (4 periods per year)
    Quarterly,
    /// Monthly compounding (12 periods per year)
    Monthly,
    /// Continuous compounding (e^rt)
    Continuous,
}

impl Compounding {
    /// Returns the number of compounding periods per year.
    ///
    /// Continuous compounding has no discrete periods and returns an error.
    pub fn periods_per_year(&self) -> BondResult<u32> {
        self.periods_per_year_opt().ok_or_else(|| {
            BondError::invalid_spec("continuous compounding has no discrete periods")
        })
    }

    /// Returns the number of periods per year, or None for continuous.
    #[must_use]
    pub fn periods_per_year_opt(&self) -> Option<u32> {
        match self {
            Compounding::Annual => Some(1),
            Compounding::SemiAnnual => Some(2),
            Compounding::Quarterly => Some(4),
            Compounding::Monthly => Some(12),
            Compounding::Continuous => None,
        }
    }

    /// Returns true if this is continuous compounding.
    #[must_use]
    pub fn is_continuous(&self) -> bool {
        matches!(self, Compounding::Continuous)
    }
}

impl std::fmt::Display for Compounding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Compounding::Annual => "Annual",
            Compounding::SemiAnnual => "Semi-Annual",
            Compounding::Quarterly => "Quarterly",
            Compounding::Monthly => "Monthly",
            Compounding::Continuous => "Continuous",
        };
        write!(f, "{s}")
    }
}

/// Converts a yield from one compounding convention to another.
///
/// Uses continuous compounding as an intermediate step.
///
/// # Arguments
///
/// * `yield_rate` - The yield rate as a decimal (e.g., 0.05 for 5%)
/// * `from` - Source compounding convention
/// * `to` - Target compounding convention
///
/// # Returns
///
/// The equivalent yield under the target compounding convention.
///
/// # Example
///
/// ```rust
/// use convex_bonds::instruments::{convert_yield, Compounding};
/// use rust_decimal_macros::dec;
///
/// // Convert 5% semi-annual to continuous
/// let continuous = convert_yield(dec!(0.05), Compounding::SemiAnnual, Compounding::Continuous).unwrap();
/// assert!(continuous < dec!(0.05)); // Continuous yield is lower
/// ```
pub fn convert_yield(
    yield_rate: Decimal,
    from: Compounding,
    to: Compounding,
) -> BondResult<Decimal> {
    crate::pricing::zero::convert_yield(yield_rate, from, to)
}

/// A zero coupon (discount) bond.
///
/// Zero coupon bonds pay no periodic coupons; instead they are issued
/// at a discount and pay face value at maturity.
///
/// # Features
///
/// - Multiple compounding conventions for yield calculations
/// - Day count convention support
/// - Price-yield conversion methods
/// - Full Bond trait implementation
///
/// # Example
///
/// ```rust,ignore
/// use convex_bonds::instruments::{ZeroCouponBond, Compounding};
/// use rust_decimal_macros::dec;
///
/// let bond = ZeroCouponBond::builder()
///     .cusip_unchecked("912796XY1")
///     .maturity(Date::from_ymd(2025, 6, 15).unwrap())
///     .issue_date(Date::from_ymd(2024, 6, 15).unwrap())
///     .compounding(Compounding::SemiAnnual)
///     .build()
///     .unwrap();
///
/// let settlement = Date::from_ymd(2024, 9, 15).unwrap();
/// let price = bond.price_from_yield(dec!(0.05), settlement);
/// let yield_rate = bond.yield_from_price(dec!(95.5), settlement);
/// ```
#[derive(Debug, Clone)]
pub struct ZeroCouponBond {
    /// Bond identifiers
    identifiers: BondIdentifiers,

    /// Maturity date
    maturity: Date,

    /// Issue date
    issue_date: Date,

    /// Issue price (optional)
    issue_price: Option<Decimal>,

    /// Day count convention for year fraction calculations
    day_count: DayCountConvention,

    /// Compounding convention for yield calculations
    compounding: Compounding,

    /// Settlement days (T+n)
    settlement_days: u32,

    /// Calendar for business day adjustments
    calendar: CalendarId,

    /// Currency
    currency: Currency,

    /// Face value (typically 100)
    face_value: Decimal,

    /// Redemption value (typically equal to face value)
    redemption_value: Decimal,
}

impl ZeroCouponBond {
    /// Creates a new builder for zero coupon bonds.
    #[must_use]
    pub fn builder() -> ZeroCouponBondBuilder {
        ZeroCouponBondBuilder::default()
    }

    /// Creates a zero coupon bond; supply its original issue date before pricing.
    #[must_use]
    pub fn new(isin: impl Into<String>, maturity: Date, currency: Currency) -> Self {
        let isin_str = isin.into();
        Self {
            identifiers: BondIdentifiers::new().with_isin(Isin::new_unchecked(&isin_str)),
            maturity,
            issue_date: maturity, // Default to maturity if not specified
            issue_price: None,
            day_count: DayCountConvention::ActActIsda,
            compounding: Compounding::SemiAnnual,
            settlement_days: 1,
            calendar: CalendarId::weekend_only(),
            currency,
            face_value: Decimal::ONE_HUNDRED,
            redemption_value: Decimal::ONE_HUNDRED,
        }
    }

    /// Sets the face and redemption value.
    #[must_use]
    pub fn with_face_value(mut self, value: Decimal) -> Self {
        self.face_value = value;
        self.redemption_value = value;
        self
    }

    /// Sets the original issue date.
    #[must_use]
    pub fn with_issue_date(mut self, date: Date) -> Self {
        self.issue_date = date;
        self
    }

    /// Returns the issue date.
    #[must_use]
    pub fn get_issue_date(&self) -> Date {
        self.issue_date
    }

    /// Returns the issue price if set.
    #[must_use]
    pub fn issue_price(&self) -> Option<Decimal> {
        self.issue_price
    }

    /// Returns the compounding convention.
    #[must_use]
    pub fn compounding(&self) -> Compounding {
        self.compounding
    }

    /// Returns the day count convention.
    #[must_use]
    pub fn day_count(&self) -> DayCountConvention {
        self.day_count
    }

    /// Returns the settlement days.
    #[must_use]
    pub fn settlement_days(&self) -> u32 {
        self.settlement_days
    }

    /// Returns the maturity date directly (not wrapped in Option).
    ///
    /// Use this when you need the raw date for calculations.
    #[must_use]
    pub fn maturity_date(&self) -> Date {
        self.maturity
    }

    /// Returns an identifier string for display purposes.
    ///
    /// Tries CUSIP, then ISIN, then ticker, then "UNKNOWN".
    #[must_use]
    pub fn identifier(&self) -> String {
        if let Some(cusip) = self.identifiers.cusip() {
            return cusip.to_string();
        }
        if let Some(isin) = self.identifiers.isin() {
            return isin.to_string();
        }
        if let Some(ticker) = self.identifiers.ticker() {
            return ticker.to_string();
        }
        "UNKNOWN".to_string()
    }

    /// Validate original active dates and absolute contractual amounts.
    fn validate_active(&self, settlement: Date) -> BondResult<()> {
        if !(1..=9999).contains(&self.issue_date.year())
            || self.issue_date >= self.maturity
            || settlement < self.issue_date
            || settlement >= self.maturity
            || self.face_value <= Decimal::ZERO
            || self.redemption_value <= Decimal::ZERO
        {
            return Err(BondError::invalid_spec("zero-coupon pricing requires positive face/redemption and active original issue/settlement/maturity"));
        }
        Ok(())
    }

    fn years_with_basis(&self, settlement: Date, basis: &DiscountTimeBasis) -> BondResult<f64> {
        self.validate_active(settlement)?;
        if basis.day_count() != self.day_count {
            return Err(BondError::invalid_spec(
                "explicit context does not match the zero-coupon discount day count",
            ));
        }
        if let DiscountTimeBasis::Thirty360EIsda { termination_date } = basis {
            if *termination_date != self.maturity {
                return Err(BondError::invalid_spec(
                    "zero-coupon termination must match original maturity",
                ));
            }
        }
        let cash = [DatedYieldCash {
            date: self.maturity,
            amount: ExactDecimal::new(self.redemption_value),
        }];
        let prepared = PreparedDatedCash::new(&cash, settlement, basis)?;
        let years = prepared
            .discount_cash()
            .first()
            .ok_or_else(|| BondError::invalid_spec("missing zero-coupon redemption"))?
            .0;
        if years > 150.0 {
            return Err(BondError::invalid_spec(
                "zero-coupon pricing horizon exceeds150years",
            ));
        }
        Ok(years)
    }

    /// Retain the existing context-free convention boundary.
    fn years_to_maturity(&self, settlement: Date) -> BondResult<f64> {
        self.validate_active(settlement)?;
        if !matches!(
            self.day_count,
            DayCountConvention::Act360
                | DayCountConvention::Act365Fixed
                | DayCountConvention::ActActIsda
                | DayCountConvention::Thirty360US
                | DayCountConvention::Thirty360E
        ) {
            return Err(BondError::invalid_spec(
                "zero-coupon convention requires an explicit discount-time basis",
            ));
        }
        self.years_with_basis(
            settlement,
            &DiscountTimeBasis::Calendar {
                day_count: self.day_count,
            },
        )
    }

    /// Checked absolute clean cash price of the original contractual redemption.
    /// Yield is a decimal fraction. Settlement is supplied directly without lag.
    /// Unsupported day-count context, inactive terms and numerical failures error.
    pub fn price_from_yield(&self, yield_rate: Decimal, settlement: Date) -> BondResult<Decimal> {
        crate::pricing::zero::dated_price_from_yield(
            self.redemption_value,
            yield_rate,
            self.years_to_maturity(settlement)?,
            self.compounding,
        )
    }

    /// Checked decimal yield from an absolute cash price and original redemption.
    /// At positive time par returns zero. Zero time returns a structural root error.
    pub fn yield_from_price(&self, price: Decimal, settlement: Date) -> BondResult<Decimal> {
        crate::pricing::zero::dated_yield_from_price(
            self.redemption_value,
            price,
            self.years_to_maturity(settlement)?,
            self.compounding,
        )
    }

    /// Absolute original redemption price with the full supplied discount context.
    /// Contextual conventions require actual nominal/coupon/termination inputs.
    /// A strictly future zero-time redemption is retained exactly at any valid yield.
    pub fn price_from_yield_with_basis(
        &self,
        yield_rate: Decimal,
        settlement: Date,
        basis: &DiscountTimeBasis,
    ) -> BondResult<Decimal> {
        crate::pricing::zero::dated_price_from_yield(
            self.redemption_value,
            yield_rate,
            self.years_with_basis(settlement, basis)?,
            self.compounding,
        )
    }

    /// Same original cash and supplied time basis as pricing; no seed yield is inferred.
    pub fn yield_from_price_with_basis(
        &self,
        price: Decimal,
        settlement: Date,
        basis: &DiscountTimeBasis,
    ) -> BondResult<Decimal> {
        crate::pricing::zero::dated_yield_from_price(
            self.redemption_value,
            price,
            self.years_with_basis(settlement, basis)?,
            self.compounding,
        )
    }

    /// Continuous inverse over the same supplied original context and absolute cash.
    pub fn continuous_rate_with_basis(
        &self,
        price: Decimal,
        settlement: Date,
        basis: &DiscountTimeBasis,
    ) -> BondResult<Decimal> {
        crate::pricing::zero::dated_yield_from_price(
            self.redemption_value,
            price,
            self.years_with_basis(settlement, basis)?,
            Compounding::Continuous,
        )
    }

    /// Checked explicit 360 bank-discount yield, as a decimal fraction.
    ///
    /// Requires active original issue/settlement/maturity and positive face
    /// redemption. Generic non-face-redemption zero quotations use their own
    /// pricing owner. This method does not infer a currency's market convention.
    ///
    /// Discount Yield = (Face - Price) / Face * (360 / Days)
    pub fn discount_yield(&self, price: Decimal, settlement: Date) -> BondResult<Decimal> {
        let days = self.bill_quote_days(settlement)?;
        crate::pricing::bill::bank_discount_yield(price, self.face_value, days)
    }

    /// Checked fixed-365 simple price-basis yield, as a decimal fraction.
    ///
    /// Requires active original dates, face redemption and at most 182 remaining
    /// days. Treasury investment-rate quotations, including leap-year short bills
    /// and all long bills, require the separate complete dated Treasury API.
    pub fn bond_equivalent_yield(&self, price: Decimal, settlement: Date) -> BondResult<Decimal> {
        let days = self.bill_quote_days(settlement)?;
        crate::pricing::bill::simple_bill_yield(price, self.face_value, days)
    }

    fn bill_quote_days(&self, settlement: Date) -> BondResult<i64> {
        if self.issue_date >= self.maturity {
            return Err(BondError::invalid_spec(
                "bill requires an original issue date before maturity",
            ));
        }
        if settlement < self.issue_date || settlement >= self.maturity {
            return Err(BondError::invalid_spec(
                "bill settlement must be on or after issue and strictly before maturity",
            ));
        }
        if self.redemption_value != self.face_value {
            return Err(BondError::invalid_spec("bill conversions require face redemption; use the complete zero quote for other redemption economics"));
        }
        Ok(settlement.days_between(&self.maturity))
    }

    /// Checked continuous decimal rate from original redemption and absolute price.
    pub fn continuous_rate(&self, price: Decimal, settlement: Date) -> BondResult<Decimal> {
        crate::pricing::zero::dated_yield_from_price(
            self.redemption_value,
            price,
            self.years_to_maturity(settlement)?,
            Compounding::Continuous,
        )
    }
}

// Implement the Bond trait from traits/bond.rs
impl Bond for ZeroCouponBond {
    fn identifiers(&self) -> &BondIdentifiers {
        &self.identifiers
    }

    fn bond_type(&self) -> BondType {
        BondType::ZeroCoupon
    }

    fn currency(&self) -> Currency {
        self.currency
    }

    fn maturity(&self) -> Option<Date> {
        Some(self.maturity)
    }

    fn issue_date(&self) -> Date {
        self.issue_date
    }

    fn dated_date(&self) -> Date {
        self.issue_date
    }

    fn face_value(&self) -> Decimal {
        self.face_value
    }

    fn frequency(&self) -> Frequency {
        Frequency::Zero
    }

    fn cash_flows(&self, from: Date) -> Vec<BondCashFlow> {
        if from >= self.maturity {
            vec![]
        } else {
            vec![BondCashFlow::principal(
                self.maturity,
                self.redemption_value,
            )]
        }
    }

    fn next_coupon_date(&self, _after: Date) -> Option<Date> {
        None // Zero coupon bonds have no coupon dates
    }

    fn previous_coupon_date(&self, _before: Date) -> Option<Date> {
        None // Zero coupon bonds have no coupon dates
    }

    fn accrued_interest(&self, _settlement: Date) -> Decimal {
        Decimal::ZERO // Zero coupon bonds have no accrued interest
    }

    fn day_count_convention(&self) -> &str {
        match self.day_count {
            DayCountConvention::Act360 => "ACT/360",
            DayCountConvention::Act365Fixed => "ACT/365F",
            DayCountConvention::Act365Leap => "ACT/365L",
            DayCountConvention::ActActIsda => "ACT/ACT ISDA",
            DayCountConvention::ActActIcma => "ACT/ACT ICMA",
            DayCountConvention::ActActAfb => "ACT/ACT AFB",
            DayCountConvention::Thirty360US => "30/360 US",
            DayCountConvention::Thirty360E => "30E/360",
            DayCountConvention::Thirty360EIsda => "30E/360 ISDA",
            DayCountConvention::Thirty360German => "30/360 German",
        }
    }

    fn calendar(&self) -> &CalendarId {
        &self.calendar
    }

    fn redemption_value(&self) -> Decimal {
        self.redemption_value
    }
}

// Custom Serialize implementation for ZeroCouponBond
impl Serialize for ZeroCouponBond {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("ZeroCouponBond", 11)?;
        state.serialize_field("identifiers", &self.identifiers)?;
        state.serialize_field("maturity", &self.maturity)?;
        state.serialize_field("issue_date", &self.issue_date)?;
        state.serialize_field("issue_price", &self.issue_price.map(ExactDecimal::new))?;
        state.serialize_field("day_count", &self.day_count)?;
        state.serialize_field("compounding", &self.compounding)?;
        state.serialize_field("settlement_days", &self.settlement_days)?;
        state.serialize_field("calendar", &self.calendar)?;
        state.serialize_field("currency", &self.currency)?;
        state.serialize_field("face_value", &ExactDecimal::new(self.face_value))?;
        state.serialize_field(
            "redemption_value",
            &ExactDecimal::new(self.redemption_value),
        )?;
        state.end()
    }
}

// Custom Deserialize implementation for ZeroCouponBond
impl<'de> Deserialize<'de> for ZeroCouponBond {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ZeroCouponBondData {
            identifiers: BondIdentifiers,
            maturity: Date,
            issue_date: Date,
            issue_price: Option<ExactDecimal>,
            day_count: DayCountConvention,
            compounding: Compounding,
            settlement_days: u32,
            calendar: CalendarId,
            currency: Currency,
            face_value: ExactDecimal,
            redemption_value: ExactDecimal,
        }

        let data = ZeroCouponBondData::deserialize(deserializer)?;
        Ok(ZeroCouponBond {
            identifiers: data.identifiers,
            maturity: data.maturity,
            issue_date: data.issue_date,
            issue_price: data.issue_price.map(ExactDecimal::decimal),
            day_count: data.day_count,
            compounding: data.compounding,
            settlement_days: data.settlement_days,
            calendar: data.calendar,
            currency: data.currency,
            face_value: data.face_value.decimal(),
            redemption_value: data.redemption_value.decimal(),
        })
    }
}

/// Builder for `ZeroCouponBond`.
#[derive(Debug, Clone, Default)]
pub struct ZeroCouponBondBuilder {
    identifiers: Option<BondIdentifiers>,
    maturity: Option<Date>,
    issue_date: Option<Date>,
    issue_price: Option<Decimal>,
    day_count: Option<DayCountConvention>,
    compounding: Option<Compounding>,
    settlement_days: Option<u32>,
    calendar: Option<CalendarId>,
    currency: Option<Currency>,
    face_value: Option<Decimal>,
    redemption_value: Option<Decimal>,
}

impl ZeroCouponBondBuilder {
    /// Creates a new builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the bond identifiers.
    #[must_use]
    pub fn identifiers(mut self, ids: BondIdentifiers) -> Self {
        self.identifiers = Some(ids);
        self
    }

    /// Sets the CUSIP identifier with validation.
    pub fn cusip(mut self, cusip: &str) -> Result<Self, crate::error::IdentifierError> {
        let cusip = Cusip::new(cusip)?;
        self.identifiers = Some(BondIdentifiers::new().with_cusip(cusip));
        Ok(self)
    }

    /// Sets the CUSIP identifier without validation.
    #[must_use]
    pub fn cusip_unchecked(mut self, cusip: &str) -> Self {
        let cusip = Cusip::new_unchecked(cusip);
        self.identifiers = Some(BondIdentifiers::new().with_cusip(cusip));
        self
    }

    /// Sets the ISIN identifier without validation.
    #[must_use]
    pub fn isin_unchecked(mut self, isin: &str) -> Self {
        self.identifiers = Some(BondIdentifiers::new().with_isin(Isin::new_unchecked(isin)));
        self
    }

    /// Sets the maturity date.
    #[must_use]
    pub fn maturity(mut self, date: Date) -> Self {
        self.maturity = Some(date);
        self
    }

    /// Sets the issue date.
    #[must_use]
    pub fn issue_date(mut self, date: Date) -> Self {
        self.issue_date = Some(date);
        self
    }

    /// Sets the issue price.
    #[must_use]
    pub fn issue_price(mut self, price: Decimal) -> Self {
        self.issue_price = Some(price);
        self
    }

    /// Sets the day count convention.
    #[must_use]
    pub fn day_count(mut self, dc: DayCountConvention) -> Self {
        self.day_count = Some(dc);
        self
    }

    /// Sets the compounding convention.
    #[must_use]
    pub fn compounding(mut self, comp: Compounding) -> Self {
        self.compounding = Some(comp);
        self
    }

    /// Sets the settlement days.
    #[must_use]
    pub fn settlement_days(mut self, days: u32) -> Self {
        self.settlement_days = Some(days);
        self
    }

    /// Sets the calendar.
    #[must_use]
    pub fn calendar(mut self, cal: CalendarId) -> Self {
        self.calendar = Some(cal);
        self
    }

    /// Sets the currency.
    #[must_use]
    pub fn currency(mut self, currency: Currency) -> Self {
        self.currency = Some(currency);
        self
    }

    /// Sets the face value.
    #[must_use]
    pub fn face_value(mut self, value: Decimal) -> Self {
        self.face_value = Some(value);
        self
    }

    /// Sets the redemption value.
    #[must_use]
    pub fn redemption_value(mut self, value: Decimal) -> Self {
        self.redemption_value = Some(value);
        self
    }

    /// Applies US Treasury bill conventions.
    ///
    /// - Day count: ACT/360
    /// - Compounding: Semi-annual for generic zero quotation metadata
    /// - Settlement: T+1
    ///
    /// This construction preset does not implement Treasury investment-rate
    /// quotation. Use the complete dated Treasury API for that convention;
    /// bill-method settlement is supplied explicitly, without applying this lag.
    #[must_use]
    pub fn us_treasury_bill(mut self) -> Self {
        self.day_count = Some(DayCountConvention::Act360);
        self.compounding = Some(Compounding::SemiAnnual);
        self.settlement_days = Some(1);
        self.calendar = Some(CalendarId::us_government());
        self.currency = Some(Currency::USD);
        self
    }

    /// Applies German Bubill conventions.
    ///
    /// - Day count: ACT/360
    /// - Compounding: Annual
    /// - Settlement: T+2
    #[must_use]
    pub fn german_bubill(mut self) -> Self {
        self.day_count = Some(DayCountConvention::Act360);
        self.compounding = Some(Compounding::Annual);
        self.settlement_days = Some(2);
        self.calendar = Some(CalendarId::target2());
        self.currency = Some(Currency::EUR);
        self
    }

    /// Applies UK Treasury bill conventions.
    ///
    /// - Day count: ACT/365
    /// - Compounding: Annual
    /// - Settlement: T+1
    #[must_use]
    pub fn uk_treasury_bill(mut self) -> Self {
        self.day_count = Some(DayCountConvention::Act365Fixed);
        self.compounding = Some(Compounding::Annual);
        self.settlement_days = Some(1);
        self.calendar = Some(CalendarId::uk());
        self.currency = Some(Currency::GBP);
        self
    }

    /// Builds the `ZeroCouponBond`.
    ///
    /// # Errors
    ///
    /// Returns an error if required fields are missing.
    pub fn build(self) -> BondResult<ZeroCouponBond> {
        let identifiers = self
            .identifiers
            .ok_or_else(|| BondError::missing_field("identifiers"))?;
        let maturity = self
            .maturity
            .ok_or_else(|| BondError::missing_field("maturity"))?;
        let issue_date = self.issue_date.unwrap_or(maturity);

        // Validate
        if maturity < issue_date {
            return Err(BondError::invalid_spec(
                "maturity must be on or after issue_date",
            ));
        }

        let face_value = self.face_value.unwrap_or(Decimal::ONE_HUNDRED);

        Ok(ZeroCouponBond {
            identifiers,
            maturity,
            issue_date,
            issue_price: self.issue_price,
            day_count: self.day_count.unwrap_or(DayCountConvention::ActActIsda),
            compounding: self.compounding.unwrap_or(Compounding::SemiAnnual),
            settlement_days: self.settlement_days.unwrap_or(1),
            calendar: self.calendar.unwrap_or_else(CalendarId::weekend_only),
            currency: self.currency.unwrap_or(Currency::USD),
            face_value,
            redemption_value: self.redemption_value.unwrap_or(face_value),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    /// Helper to create a date.
    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    #[test]
    fn test_zero_coupon_bond() {
        let bond = ZeroCouponBond::new("US912796XY12", date(2025, 6, 15), Currency::USD);

        assert_eq!(
            bond.identifiers().isin().map(|i| i.as_str()),
            Some("US912796XY12")
        );
        assert_eq!(bond.face_value(), dec!(100));
        assert_eq!(bond.bond_type(), BondType::ZeroCoupon);
        assert_eq!(bond.accrued_interest(date(2024, 6, 15)), Decimal::ZERO);
    }

    #[test]
    fn test_with_face_value() {
        let bond = ZeroCouponBond::new("TEST", date(2025, 6, 15), Currency::USD)
            .with_face_value(dec!(1000));

        assert_eq!(bond.face_value(), dec!(1000));
    }

    #[test]
    fn test_builder() {
        let bond = ZeroCouponBond::builder()
            .cusip_unchecked("912796XY1")
            .maturity(date(2025, 6, 15))
            .issue_date(date(2024, 6, 15))
            .compounding(Compounding::SemiAnnual)
            .day_count(DayCountConvention::ActActIsda)
            .build()
            .unwrap();

        assert_eq!(bond.compounding(), Compounding::SemiAnnual);
        assert_eq!(bond.day_count(), DayCountConvention::ActActIsda);
    }

    #[test]
    fn test_us_treasury_bill() {
        let bond = ZeroCouponBond::builder()
            .cusip_unchecked("912796XY1")
            .maturity(date(2025, 6, 15))
            .issue_date(date(2025, 3, 15))
            .us_treasury_bill()
            .build()
            .unwrap();

        assert_eq!(bond.day_count(), DayCountConvention::Act360);
        assert_eq!(bond.settlement_days(), 1);
        assert_eq!(bond.currency(), Currency::USD);
    }

    #[test]
    fn test_price_from_yield_semi_annual() {
        // 1-year zero coupon at 5% semi-annual
        let bond = ZeroCouponBond::builder()
            .cusip_unchecked("TEST12345")
            .maturity(date(2025, 6, 15))
            .issue_date(date(2024, 6, 15))
            .compounding(Compounding::SemiAnnual)
            .day_count(DayCountConvention::ActActIsda)
            .build()
            .unwrap();

        let settlement = date(2024, 6, 15);
        let price = bond.price_from_yield(dec!(0.05), settlement).unwrap();

        // Price = 100 / (1 + 0.025)^2 = 100 / 1.050625 = 95.1814
        assert!(price > dec!(95.0));
        assert!(price < dec!(96.0));
    }

    #[test]
    fn test_price_from_yield_continuous() {
        // 1-year zero coupon at 5% continuous
        let bond = ZeroCouponBond::builder()
            .cusip_unchecked("TEST12345")
            .maturity(date(2025, 6, 15))
            .issue_date(date(2024, 6, 15))
            .compounding(Compounding::Continuous)
            .day_count(DayCountConvention::ActActIsda)
            .build()
            .unwrap();

        let settlement = date(2024, 6, 15);
        let price = bond.price_from_yield(dec!(0.05), settlement).unwrap();

        // Price = 100 * e^(-0.05 * 1) = 100 * 0.9512... = 95.12
        assert!(price > dec!(95.0));
        assert!(price < dec!(96.0));
    }

    #[test]
    fn test_yield_from_price() {
        let bond = ZeroCouponBond::builder()
            .cusip_unchecked("TEST12345")
            .maturity(date(2025, 6, 15))
            .issue_date(date(2024, 6, 15))
            .compounding(Compounding::SemiAnnual)
            .day_count(DayCountConvention::ActActIsda)
            .build()
            .unwrap();

        let settlement = date(2024, 6, 15);
        let yield_rate = bond.yield_from_price(dec!(95.18), settlement).unwrap();

        // Should be close to 5%
        assert!(yield_rate > dec!(0.049));
        assert!(yield_rate < dec!(0.051));
    }

    #[test]
    fn test_price_yield_roundtrip() {
        let bond = ZeroCouponBond::builder()
            .cusip_unchecked("TEST12345")
            .maturity(date(2026, 6, 15))
            .issue_date(date(2024, 6, 15))
            .compounding(Compounding::Annual)
            .build()
            .unwrap();

        let settlement = date(2024, 6, 15);
        let original_yield = dec!(0.045); // 4.5%

        let price = bond.price_from_yield(original_yield, settlement).unwrap();
        let recovered_yield = bond.yield_from_price(price, settlement).unwrap();

        // Should recover original yield within 1bp
        let diff = (original_yield - recovered_yield).abs();
        assert!(diff < dec!(0.0001));
    }

    #[test]
    fn test_compounding_periods() {
        assert_eq!(Compounding::Annual.periods_per_year().unwrap(), 1);
        assert_eq!(Compounding::SemiAnnual.periods_per_year().unwrap(), 2);
        assert_eq!(Compounding::Quarterly.periods_per_year().unwrap(), 4);
        assert_eq!(Compounding::Monthly.periods_per_year().unwrap(), 12);
        assert_eq!(Compounding::Continuous.periods_per_year_opt(), None);
    }

    #[test]
    fn test_convert_yield() {
        let semi_annual = dec!(0.05);

        // Convert to continuous
        let continuous = convert_yield(
            semi_annual,
            Compounding::SemiAnnual,
            Compounding::Continuous,
        )
        .unwrap();
        // 2 * ln(1 + 0.05/2) = 2 * ln(1.025) = 0.04939...
        assert!(continuous > dec!(0.049));
        assert!(continuous < dec!(0.05));

        // Convert back
        let back_to_semi =
            convert_yield(continuous, Compounding::Continuous, Compounding::SemiAnnual).unwrap();
        let diff = (semi_annual - back_to_semi).abs();
        assert!(diff < dec!(0.0001));
    }

    #[test]
    fn test_convert_yield_same_convention() {
        let rate = dec!(0.05);
        let result = convert_yield(rate, Compounding::Annual, Compounding::Annual).unwrap();
        assert_eq!(result, rate);
    }

    #[test]
    fn test_discount_yield() {
        // 90-day T-Bill priced at 98.75
        let bond = ZeroCouponBond::builder()
            .cusip_unchecked("912796AB1")
            .maturity(date(2024, 6, 15))
            .issue_date(date(2024, 3, 17))
            .us_treasury_bill()
            .build()
            .unwrap();

        let settlement = date(2024, 3, 17);
        let discount_yield = bond.discount_yield(dec!(98.75), settlement).unwrap();

        // Discount yield = (100 - 98.75) / 100 * 360 / 90 = 0.05 = 5%
        assert!(discount_yield > dec!(0.049));
        assert!(discount_yield < dec!(0.051));
    }

    #[test]
    fn test_cash_flows() {
        let bond = ZeroCouponBond::builder()
            .cusip_unchecked("TEST12345")
            .maturity(date(2025, 6, 15))
            .issue_date(date(2024, 6, 15))
            .build()
            .unwrap();

        let flows = bond.cash_flows(date(2024, 6, 15));
        assert_eq!(flows.len(), 1);
        assert!(flows[0].is_principal());
        assert_eq!(flows[0].amount, dec!(100));

        // After maturity - no cash flows
        let flows = bond.cash_flows(date(2025, 7, 1));
        assert!(flows.is_empty());
    }

    #[test]
    fn test_bond_trait_methods() {
        let bond = ZeroCouponBond::builder()
            .cusip_unchecked("TEST12345")
            .maturity(date(2025, 6, 15))
            .issue_date(date(2024, 6, 15))
            .build()
            .unwrap();

        assert_eq!(bond.maturity(), Some(date(2025, 6, 15)));
        assert_eq!(bond.issue_date(), date(2024, 6, 15));
        assert_eq!(bond.next_coupon_date(date(2024, 6, 15)), None);
        assert_eq!(bond.previous_coupon_date(date(2024, 6, 15)), None);
        assert!(!bond.has_matured(date(2024, 6, 15)));
        assert!(bond.has_matured(date(2025, 7, 1)));
    }

    #[test]
    fn test_missing_fields() {
        let result = ZeroCouponBond::builder().build();
        assert!(result.is_err());

        let result = ZeroCouponBond::builder().cusip_unchecked("TEST").build();
        assert!(result.is_err()); // Missing maturity
    }

    /// price_from_yield (closed-form) ≡ Bond::clean_price_from_yield (analytics).
    #[test]
    fn test_zero_closed_form_matches_trait() {
        use crate::traits::BondAnalytics;
        use convex_core::types::Frequency;

        let bond = ZeroCouponBond::builder()
            .cusip_unchecked("STRIP_TEST")
            .maturity(date(2055, 11, 15))
            .issue_date(date(2025, 11, 15))
            .compounding(Compounding::SemiAnnual)
            .day_count(DayCountConvention::ActActIsda)
            .build()
            .unwrap();

        let settle = date(2025, 12, 31);
        for y in [0.02, 0.045, 0.0625, 0.10] {
            let closed = bond
                .price_from_yield(Decimal::try_from(y).unwrap(), settle)
                .unwrap()
                .to_string()
                .parse::<f64>()
                .unwrap();
            let via_trait = bond
                .clean_price_from_yield(settle, y, Frequency::SemiAnnual)
                .unwrap();
            assert!(
                (closed - via_trait).abs() < 1e-8,
                "y={y}: closed={closed}, trait={via_trait}",
            );
        }
    }
}
