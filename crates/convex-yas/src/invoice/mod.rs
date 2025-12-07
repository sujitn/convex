//! Settlement invoice calculations.
//!
//! This module handles the calculation of settlement amounts including
//! clean price, accrued interest, and total settlement amount.

mod settlement;

pub use settlement::*;

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

    /// Principal amount (face × clean price / 100)
    pub principal_amount: Decimal,

    /// Accrued amount (face × accrued / 100)
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
    pub fn settlement_date(mut self, date: NaiveDate) -> Self {
        self.settlement_date = Some(date);
        self
    }

    /// Set clean price
    pub fn clean_price(mut self, price: Decimal) -> Self {
        self.clean_price = Some(price);
        self
    }

    /// Set accrued interest
    pub fn accrued_interest(mut self, accrued: Decimal) -> Self {
        self.accrued_interest = Some(accrued);
        self
    }

    /// Set accrued days
    pub fn accrued_days(mut self, days: i32) -> Self {
        self.accrued_days = Some(days);
        self
    }

    /// Set face value
    pub fn face_value(mut self, face: Decimal) -> Self {
        self.face_value = Some(face);
        self
    }

    /// Build the settlement invoice
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

#[cfg(test)]
mod tests {
    use super::*;
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

        // Principal = 1M × 100 / 100 = 1M
        // Accrued = 1M × 2.5 / 100 = 25K
        assert_eq!(invoice.principal_amount, dec!(1000000.0));
        assert_eq!(invoice.accrued_amount, dec!(25000.0));
        assert_eq!(invoice.settlement_amount, dec!(1025000.0));
    }
}
