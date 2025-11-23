//! Fixed coupon bond.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use convex_core::types::{Currency, Date, Frequency};

use crate::error::{BondError, BondResult};
use crate::instruments::Bond;

/// A fixed coupon bond.
///
/// Represents a bond with fixed periodic coupon payments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixedBond {
    /// ISIN or identifier.
    isin: String,

    /// Coupon rate as decimal (0.05 = 5%).
    coupon_rate: Decimal,

    /// Maturity date.
    maturity: Date,

    /// First coupon date (optional, for odd first coupon).
    first_coupon_date: Option<Date>,

    /// Issue date.
    issue_date: Option<Date>,

    /// Payment frequency.
    frequency: Frequency,

    /// Currency.
    currency: Currency,

    /// Face value (default 100).
    face_value: Decimal,

    /// Day count convention name.
    day_count: String,
}

impl FixedBond {
    /// Returns the coupon rate as a decimal.
    #[must_use]
    pub fn coupon_rate(&self) -> Decimal {
        self.coupon_rate
    }

    /// Returns the annual coupon amount per 100 face value.
    #[must_use]
    pub fn annual_coupon(&self) -> Decimal {
        self.coupon_rate * self.face_value
    }

    /// Returns the coupon amount per period.
    #[must_use]
    pub fn coupon_per_period(&self) -> Decimal {
        let periods = self.frequency.periods_per_year();
        if periods == 0 {
            Decimal::ZERO
        } else {
            self.annual_coupon() / Decimal::from(periods)
        }
    }

    /// Returns the first coupon date.
    #[must_use]
    pub fn first_coupon_date(&self) -> Option<Date> {
        self.first_coupon_date
    }

    /// Returns the issue date.
    #[must_use]
    pub fn issue_date(&self) -> Option<Date> {
        self.issue_date
    }

    /// Returns the day count convention name.
    #[must_use]
    pub fn day_count(&self) -> &str {
        &self.day_count
    }
}

impl Bond for FixedBond {
    fn identifier(&self) -> &str {
        &self.isin
    }

    fn maturity(&self) -> Date {
        self.maturity
    }

    fn currency(&self) -> Currency {
        self.currency
    }

    fn face_value(&self) -> Decimal {
        self.face_value
    }

    fn frequency(&self) -> Frequency {
        self.frequency
    }
}

/// Builder for fixed coupon bonds.
#[derive(Debug, Clone, Default)]
pub struct FixedBondBuilder {
    isin: Option<String>,
    coupon_rate: Option<Decimal>,
    maturity: Option<Date>,
    first_coupon_date: Option<Date>,
    issue_date: Option<Date>,
    frequency: Frequency,
    currency: Currency,
    face_value: Decimal,
    day_count: String,
}

impl FixedBondBuilder {
    /// Creates a new builder with defaults.
    #[must_use]
    pub fn new() -> Self {
        Self {
            face_value: Decimal::ONE_HUNDRED,
            day_count: "ACT/ACT".to_string(),
            ..Default::default()
        }
    }

    /// Sets the ISIN.
    #[must_use]
    pub fn isin(mut self, isin: impl Into<String>) -> Self {
        self.isin = Some(isin.into());
        self
    }

    /// Sets the coupon rate (as decimal, 0.05 = 5%).
    #[must_use]
    pub fn coupon_rate(mut self, rate: Decimal) -> Self {
        self.coupon_rate = Some(rate);
        self
    }

    /// Sets the maturity date.
    #[must_use]
    pub fn maturity(mut self, date: Date) -> Self {
        self.maturity = Some(date);
        self
    }

    /// Sets the first coupon date (for odd first coupons).
    #[must_use]
    pub fn first_coupon_date(mut self, date: Date) -> Self {
        self.first_coupon_date = Some(date);
        self
    }

    /// Sets the issue date.
    #[must_use]
    pub fn issue_date(mut self, date: Date) -> Self {
        self.issue_date = Some(date);
        self
    }

    /// Sets the payment frequency.
    #[must_use]
    pub fn frequency(mut self, freq: Frequency) -> Self {
        self.frequency = freq;
        self
    }

    /// Sets the currency.
    #[must_use]
    pub fn currency(mut self, currency: Currency) -> Self {
        self.currency = currency;
        self
    }

    /// Sets the face value.
    #[must_use]
    pub fn face_value(mut self, value: Decimal) -> Self {
        self.face_value = value;
        self
    }

    /// Sets the day count convention.
    #[must_use]
    pub fn day_count(mut self, dc: impl Into<String>) -> Self {
        self.day_count = dc.into();
        self
    }

    /// Builds the fixed bond.
    ///
    /// # Errors
    ///
    /// Returns an error if required fields are missing.
    pub fn build(self) -> BondResult<FixedBond> {
        let isin = self.isin.ok_or_else(|| BondError::missing_field("isin"))?;
        let coupon_rate = self
            .coupon_rate
            .ok_or_else(|| BondError::missing_field("coupon_rate"))?;
        let maturity = self
            .maturity
            .ok_or_else(|| BondError::missing_field("maturity"))?;

        // Validate coupon rate
        if coupon_rate < Decimal::ZERO {
            return Err(BondError::invalid_spec("Coupon rate cannot be negative"));
        }

        Ok(FixedBond {
            isin,
            coupon_rate,
            maturity,
            first_coupon_date: self.first_coupon_date,
            issue_date: self.issue_date,
            frequency: self.frequency,
            currency: self.currency,
            face_value: self.face_value,
            day_count: self.day_count,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_fixed_bond_builder() {
        let bond = FixedBondBuilder::new()
            .isin("US912828Z229")
            .coupon_rate(dec!(0.025))
            .maturity(Date::from_ymd(2030, 5, 15).unwrap())
            .frequency(Frequency::SemiAnnual)
            .currency(Currency::USD)
            .build()
            .unwrap();

        assert_eq!(bond.identifier(), "US912828Z229");
        assert_eq!(bond.coupon_rate(), dec!(0.025));
        assert_eq!(bond.annual_coupon(), dec!(2.5));
        assert_eq!(bond.coupon_per_period(), dec!(1.25));
    }

    #[test]
    fn test_missing_fields() {
        let result = FixedBondBuilder::new().build();
        assert!(result.is_err());

        let result = FixedBondBuilder::new()
            .isin("TEST")
            .coupon_rate(dec!(0.05))
            .build();
        assert!(result.is_err()); // Missing maturity
    }

    #[test]
    fn test_negative_coupon_error() {
        let result = FixedBondBuilder::new()
            .isin("TEST")
            .coupon_rate(dec!(-0.01))
            .maturity(Date::from_ymd(2030, 1, 1).unwrap())
            .build();

        assert!(result.is_err());
    }
}
