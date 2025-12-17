//! Settlement invoice calculations.
//!
//! This module handles the calculation of settlement amounts including
//! clean price, accrued interest, and total settlement amount.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

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

    /// Calculate the total settlement amount
    #[must_use]
    pub fn calculate_settlement(&self) -> Decimal {
        self.principal_amount + self.accrued_amount
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
    /// Returns an error if required fields are missing.
    pub fn build(self) -> Result<SettlementInvoice, &'static str> {
        let clean_price = self.clean_price.ok_or("clean_price is required")?;
        let accrued_interest = self
            .accrued_interest
            .ok_or("accrued_interest is required")?;
        let face_value = self.face_value.ok_or("face_value is required")?;

        let dirty_price = clean_price + accrued_interest;
        let principal_amount = face_value * clean_price / Decimal::ONE_HUNDRED;
        let accrued_amount = face_value * accrued_interest / Decimal::ONE_HUNDRED;
        let settlement_amount = principal_amount + accrued_amount;

        Ok(SettlementInvoice {
            settlement_date: self
                .settlement_date
                .unwrap_or_else(|| NaiveDate::from_ymd_opt(2000, 1, 1).unwrap()),
            clean_price,
            accrued_interest,
            dirty_price,
            accrued_days: self.accrued_days.unwrap_or(0),
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
        writeln!(f, "Face Value:      ${:.2}", self.face_value)?;
        writeln!(f, "Clean Price:     {:.6}%", self.clean_price)?;
        writeln!(f, "Accrued Days:    {}", self.accrued_days)?;
        writeln!(f, "Accrued Int:     {:.6}%", self.accrued_interest)?;
        writeln!(f, "Dirty Price:     {:.6}%", self.dirty_price)?;
        writeln!(f, "---")?;
        writeln!(f, "Principal:       ${:.2}", self.principal_amount)?;
        writeln!(f, "Accrued:         ${:.2}", self.accrued_amount)?;
        writeln!(f, "Settlement:      ${:.2}", self.settlement_amount)?;
        Ok(())
    }
}

/// Calculate settlement date from trade date.
///
/// # Arguments
///
/// * `trade_date` - The trade date
/// * `settlement_days` - Number of business days to settlement (e.g., 1 for T+1, 2 for T+2)
/// * `is_business_day` - Function to check if a date is a business day
///
/// # Returns
///
/// The settlement date
pub fn calculate_settlement_date<F>(
    trade_date: NaiveDate,
    settlement_days: u32,
    is_business_day: F,
) -> NaiveDate
where
    F: Fn(NaiveDate) -> bool,
{
    let mut date = trade_date;
    let mut business_days = 0;

    while business_days < settlement_days {
        date = date.succ_opt().unwrap_or(date);
        if is_business_day(date) {
            business_days += 1;
        }
    }

    date
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
/// Total proceeds
#[must_use]
pub fn calculate_proceeds(
    face_value: Decimal,
    clean_price: Decimal,
    accrued_interest: Decimal,
) -> Decimal {
    let principal = face_value * clean_price / Decimal::ONE_HUNDRED;
    let accrued = face_value * accrued_interest / Decimal::ONE_HUNDRED;
    principal + accrued
}

/// Calculate the dollar amount of accrued interest.
///
/// # Arguments
///
/// * `face_value` - Face value of the bond
/// * `coupon_rate` - Annual coupon rate as decimal
/// * `accrued_fraction` - Day count fraction for accrued period
///
/// # Returns
///
/// Dollar amount of accrued interest
#[must_use]
pub fn calculate_accrued_amount(
    face_value: Decimal,
    coupon_rate: Decimal,
    accrued_fraction: Decimal,
) -> Decimal {
    face_value * coupon_rate * accrued_fraction
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;
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
        let invoice = SettlementInvoice::builder()
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

        let proceeds = calculate_proceeds(face, clean, accrued);

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

        let accrued = calculate_accrued_amount(face, coupon, fraction);

        // 1M x 7.5% x (134/360) = 27,916.67
        let expected = dec!(27916.67);
        let diff = (accrued - expected).abs();
        assert!(diff < dec!(1.0));
    }

    #[test]
    fn test_settlement_date() {
        let trade = NaiveDate::from_ymd_opt(2020, 4, 27).unwrap(); // Monday

        // Simple weekend check (not a full calendar)
        let is_business_day = |d: NaiveDate| {
            let weekday = d.weekday();
            weekday != chrono::Weekday::Sat && weekday != chrono::Weekday::Sun
        };

        let settle = calculate_settlement_date(trade, 2, is_business_day);

        // T+2 from Monday = Wednesday
        assert_eq!(settle, NaiveDate::from_ymd_opt(2020, 4, 29).unwrap());
    }

    #[test]
    fn test_invoice_display() {
        let invoice = SettlementInvoice::builder()
            .settlement_date(NaiveDate::from_ymd_opt(2020, 4, 29).unwrap())
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
