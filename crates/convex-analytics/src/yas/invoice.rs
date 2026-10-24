//! Settlement invoice calculations.
//!
//! This module handles the calculation of settlement amounts including
//! clean price, accrued interest, and total settlement amount.

use chrono::NaiveDate;
use convex_core::{calendars::Calendar, ConvexError, Date};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Missing supplied context or unrepresentable invoice arithmetic.
#[derive(Debug, thiserror::Error)]
pub enum SettlementInvoiceError {
    /// No replacement date, price, position or accrued metadata is inferred.
    #[error("{0} is required")]
    Missing(&'static str),
    /// The supplied date lies outside the financial ISO date range.
    #[error("{0} must have a year in 1..9999")]
    InvalidDate(&'static str),
    /// Decimal overflow or nonzero underflow; no partial invoice is returned.
    #[error("unrepresentable Decimal arithmetic for {0}")]
    Arithmetic(&'static str),
    /// The actual calendar rejects coverage, dates or bounded work.
    #[error(transparent)]
    Calendar(#[from] ConvexError),
}

/// Checked supplied-value invoice result, separate from cash posting.
pub type SettlementInvoiceResult<T> = Result<T, SettlementInvoiceError>;

/// Settlement invoice containing all settlement details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementInvoice {
    /// Settlement date
    pub settlement_date: NaiveDate,

    /// Clean price (percentage of par)
    pub clean_price: Decimal,

    /// Accrued interest (percentage of par)
    pub accrued_interest: Decimal,

    /// Dirty price (clean + accrued, percentage of par)
    pub dirty_price: Decimal,

    /// Number of accrued days
    pub accrued_days: i32,

    /// Principal amount (face x clean price / 100)
    pub principal_amount: Decimal,

    /// Accrued amount (face x accrued / 100)
    pub accrued_amount: Decimal,

    /// Total settlement amount
    pub settlement_amount: Decimal,

    /// Face value of the position
    pub face_value: Decimal,
}

impl SettlementInvoice {
    /// Create a new settlement invoice builder
    pub fn builder() -> SettlementInvoiceBuilder {
        SettlementInvoiceBuilder::default()
    }
}

/// Builder for settlement invoice
#[derive(Debug, Default)]
pub struct SettlementInvoiceBuilder {
    settlement_date: Option<NaiveDate>,
    clean_price: Option<Decimal>,
    accrued_interest: Option<Decimal>,
    accrued_days: Option<i32>,
    face_value: Option<Decimal>,
}

impl SettlementInvoiceBuilder {
    /// Set settlement date
    #[must_use]
    pub fn settlement_date(mut self, date: NaiveDate) -> Self {
        self.settlement_date = Some(date);
        self
    }

    /// Set clean price
    #[must_use]
    pub fn clean_price(mut self, price: Decimal) -> Self {
        self.clean_price = Some(price);
        self
    }

    /// Set accrued interest
    #[must_use]
    pub fn accrued_interest(mut self, accrued: Decimal) -> Self {
        self.accrued_interest = Some(accrued);
        self
    }

    /// Set accrued days
    #[must_use]
    pub fn accrued_days(mut self, days: i32) -> Self {
        self.accrued_days = Some(days);
        self
    }

    /// Set face value
    #[must_use]
    pub fn face_value(mut self, face: Decimal) -> Self {
        self.face_value = Some(face);
        self
    }

    /// Build the settlement invoice
    ///
    /// # Errors
    ///
    /// Requires all five supplied fields and representable dirty, principal,
    /// accrued and net amounts. Signed positions and accrued values are retained.
    /// No currency, settlement convention or ledger rounding is inferred.
    pub fn build(self) -> SettlementInvoiceResult<SettlementInvoice> {
        let clean_price = self
            .clean_price
            .ok_or(SettlementInvoiceError::Missing("clean_price"))?;
        let accrued_interest = self
            .accrued_interest
            .ok_or(SettlementInvoiceError::Missing("accrued_interest"))?;
        let face_value = self
            .face_value
            .ok_or(SettlementInvoiceError::Missing("face_value"))?;
        let settlement_date = self
            .settlement_date
            .ok_or(SettlementInvoiceError::Missing("settlement_date"))?;
        let accrued_days = self
            .accrued_days
            .ok_or(SettlementInvoiceError::Missing("accrued_days"))?;
        financial_date(settlement_date, "settlement_date")?;

        let dirty_price = clean_price
            .checked_add(accrued_interest)
            .ok_or(SettlementInvoiceError::Arithmetic("dirty_price"))?;
        let (principal_amount, accrued_amount, settlement_amount) =
            components(face_value, clean_price, accrued_interest)?;

        Ok(SettlementInvoice {
            settlement_date,
            clean_price,
            accrued_interest,
            dirty_price,
            accrued_days,
            principal_amount,
            accrued_amount,
            settlement_amount,
            face_value,
        })
    }
}

impl std::fmt::Display for SettlementInvoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "=== Settlement Invoice ===")?;
        writeln!(f, "Settlement Date: {}", self.settlement_date)?;
        writeln!(f, "Face Value:      {:.2}", self.face_value)?;
        writeln!(f, "Clean Price:     {:.6}%", self.clean_price)?;
        writeln!(f, "Accrued Days:    {}", self.accrued_days)?;
        writeln!(f, "Accrued Int:     {:.6}%", self.accrued_interest)?;
        writeln!(f, "Dirty Price:     {:.6}%", self.dirty_price)?;
        writeln!(f, "---")?;
        writeln!(f, "Principal:       {:.2}", self.principal_amount)?;
        writeln!(f, "Accrued:         {:.2}", self.accrued_amount)?;
        writeln!(f, "Settlement:      {:.2}", self.settlement_amount)?;
        Ok(())
    }
}

/// Calculate settlement date from trade date.
///
/// # Arguments
///
/// * `trade_date` - The trade date
/// * `settlement_days` - Number of business days to settlement (e.g., 1 for T+1, 2 for T+2)
/// * `calendar` - Actual caller-supplied calendar, including its coverage rules
///
/// # Returns
///
/// The checked settlement date. Zero preserves the valid supplied date without
/// querying a calendar. Positive offsets reuse the existing 366-business-day
/// and 3660-calendar-day search bounds. Custom calendar callbacks must terminate.
///
/// # Errors
///
/// Invalid financial dates, unavailable coverage, excessive offsets, exhausted
/// searches and date overflow return errors without a substitute date.
pub fn calculate_settlement_date<C: Calendar + ?Sized>(
    trade_date: NaiveDate,
    settlement_days: u32,
    calendar: &C,
) -> SettlementInvoiceResult<NaiveDate> {
    financial_date(trade_date, "trade_date")?;
    let days = i32::try_from(settlement_days).map_err(|_| ConvexError::CalendarError {
        reason: "business-day offset exceeds 366".into(),
    })?;
    let date =
        convex_core::calendars::checked::add_business_days(trade_date.into(), days, calendar)?;
    let result = date.as_naive_date();
    financial_date(result, "settlement_date")?;
    Ok(result)
}

/// Calculate proceeds from a bond sale.
///
/// # Arguments
///
/// * `face_value` - Face value of the bond
/// * `clean_price` - Clean price as percentage of par
/// * `accrued_interest` - Accrued interest as percentage of par
///
/// # Returns
///
/// Signed total proceeds with the same component calculation as the builder.
///
/// # Errors
///
/// Unrepresentable principal, accrued or net amounts return an error.
pub fn calculate_proceeds(
    face_value: Decimal,
    clean_price: Decimal,
    accrued_interest: Decimal,
) -> SettlementInvoiceResult<Decimal> {
    components(face_value, clean_price, accrued_interest).map(|(_, _, net)| net)
}

/// Calculate signed accrued interest in the face value's currency units.
///
/// # Arguments
///
/// * `face_value` - Face value of the bond
/// * `coupon_rate` - Annual coupon rate as decimal
/// * `accrued_fraction` - Day count fraction for accrued period
///
/// # Returns
///
/// Unrounded Decimal accrued amount. Ordinary calculation order is preserved;
/// only failed intermediate arithmetic tries the other two product groupings.
/// An explicitly zero factor produces exact zero.
///
/// # Errors
///
/// Overflow or nonzero underflow in every grouping returns an error.
pub fn calculate_accrued_amount(
    face_value: Decimal,
    coupon_rate: Decimal,
    accrued_fraction: Decimal,
) -> SettlementInvoiceResult<Decimal> {
    if face_value.is_zero() || coupon_rate.is_zero() || accrued_fraction.is_zero() {
        return Ok(Decimal::ZERO);
    }
    for (a, b, c) in [
        (face_value, coupon_rate, accrued_fraction),
        (face_value, accrued_fraction, coupon_rate),
        (coupon_rate, accrued_fraction, face_value),
    ] {
        if let Some(value) = product(a, b).and_then(|ab| product(ab, c)) {
            return Ok(value);
        }
    }
    Err(SettlementInvoiceError::Arithmetic("accrued_amount"))
}

fn financial_date(date: NaiveDate, field: &'static str) -> SettlementInvoiceResult<()> {
    if !(1..=9999).contains(&Date::from(date).year()) {
        return Err(SettlementInvoiceError::InvalidDate(field));
    }
    Ok(())
}

fn product(a: Decimal, b: Decimal) -> Option<Decimal> {
    a.checked_mul(b)
        .filter(|v| !v.is_zero() || a.is_zero() || b.is_zero())
}

fn per100(face: Decimal, price: Decimal, field: &'static str) -> SettlementInvoiceResult<Decimal> {
    let amount = if let Some(value) = product(face, price) {
        value.checked_div(Decimal::ONE_HUNDRED)
    } else {
        // Shift an operand's scale by exactly two; do not round it before
        // multiplication. Two operands with scale >26 cannot overflow a product.
        [(face, price), (price, face)]
            .into_iter()
            .filter(|(a, _)| a.scale() <= 26)
            .find_map(|(a, b)| {
                Decimal::try_from_i128_with_scale(a.mantissa(), a.scale() + 2)
                    .ok()
                    .and_then(|scaled| product(scaled, b))
            })
    };
    amount
        .filter(|v| !v.is_zero() || face.is_zero() || price.is_zero())
        .ok_or(SettlementInvoiceError::Arithmetic(field))
}

fn components(
    face: Decimal,
    clean: Decimal,
    accrued: Decimal,
) -> SettlementInvoiceResult<(Decimal, Decimal, Decimal)> {
    let principal = per100(face, clean, "principal_amount")?;
    let accrued = per100(face, accrued, "accrued_amount")?;
    let net = principal
        .checked_add(accrued)
        .ok_or(SettlementInvoiceError::Arithmetic("settlement_amount"))?;
    Ok((principal, accrued, net))
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_core::calendars::WeekendCalendar;
    use rust_decimal_macros::dec;

    #[test]
    fn test_settlement_invoice_builder() {
        let invoice = SettlementInvoice::builder()
            .settlement_date(NaiveDate::from_ymd_opt(2020, 4, 29).unwrap())
            .clean_price(dec!(110.503))
            .accrued_interest(dec!(2.698611))
            .accrued_days(134)
            .face_value(dec!(1000000.0))
            .build()
            .unwrap();

        assert_eq!(invoice.dirty_price, dec!(113.201611));
        assert_eq!(invoice.accrued_days, 134);
    }

    #[test]
    fn test_settlement_amounts() {
        let incomplete = SettlementInvoice::builder()
            .clean_price(dec!(100.0))
            .accrued_interest(dec!(2.5))
            .face_value(dec!(1000000.0))
            .build();
        assert!(matches!(
            incomplete,
            Err(SettlementInvoiceError::Missing("settlement_date"))
        ));
        let invoice = SettlementInvoice::builder()
            .settlement_date(NaiveDate::from_ymd_opt(2020, 4, 29).unwrap())
            .accrued_days(134)
            .clean_price(dec!(100.0))
            .accrued_interest(dec!(2.5))
            .face_value(dec!(1000000.0))
            .build()
            .unwrap();

        // Principal = 1M x 100 / 100 = 1M
        // Accrued = 1M x 2.5 / 100 = 25K
        assert_eq!(invoice.principal_amount, dec!(1000000.0));
        assert_eq!(invoice.accrued_amount, dec!(25000.0));
        assert_eq!(invoice.settlement_amount, dec!(1025000.0));
    }

    #[test]
    fn test_calculate_proceeds() {
        let face = dec!(1000000.0);
        let clean = dec!(110.503);
        let accrued = dec!(2.698611);

        let proceeds = calculate_proceeds(face, clean, accrued).unwrap();

        // Principal = 1M x 110.503% = 1,105,030
        // Accrued = 1M x 2.698611% = 26,986.11
        // Total = 1,132,016.11
        let expected = dec!(1132016.11);
        let diff = (proceeds - expected).abs();
        assert!(diff < dec!(0.01));
    }

    #[test]
    fn test_calculate_accrued_amount() {
        let face = dec!(1000000.0);
        let coupon = dec!(0.075); // 7.5%
        let fraction = dec!(0.3722222); // 134/360

        let accrued = calculate_accrued_amount(face, coupon, fraction).unwrap();

        // 1M x 7.5% x (134/360) = 27,916.67
        let expected = dec!(27916.67);
        let diff = (accrued - expected).abs();
        assert!(diff < dec!(1.0));
    }

    #[test]
    fn test_settlement_date() {
        let trade = NaiveDate::from_ymd_opt(2020, 4, 27).unwrap(); // Monday

        // Explicitly supplied weekend-only rule; no holiday data is inferred.
        let settle = calculate_settlement_date(trade, 2, &WeekendCalendar).unwrap();

        // T+2 from Monday = Wednesday
        assert_eq!(settle, NaiveDate::from_ymd_opt(2020, 4, 29).unwrap());
    }

    #[test]
    fn test_invoice_display() {
        let incomplete = SettlementInvoice::builder()
            .settlement_date(NaiveDate::from_ymd_opt(2020, 4, 29).unwrap())
            .clean_price(dec!(100.0))
            .accrued_interest(dec!(2.5))
            .face_value(dec!(1000000.0))
            .build();
        assert!(matches!(
            incomplete,
            Err(SettlementInvoiceError::Missing("accrued_days"))
        ));
        let invoice = SettlementInvoice::builder()
            .settlement_date(NaiveDate::from_ymd_opt(2020, 4, 29).unwrap())
            .accrued_days(134)
            .clean_price(dec!(100.0))
            .accrued_interest(dec!(2.5))
            .face_value(dec!(1000000.0))
            .build()
            .unwrap();

        let display = format!("{}", invoice);
        assert!(display.contains("Settlement Invoice"));
        assert!(display.contains("Clean Price"));
        assert!(display.contains("Settlement:"));
    }
}
