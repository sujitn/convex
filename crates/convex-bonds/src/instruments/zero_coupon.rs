//! Zero coupon bond.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use convex_core::types::{Currency, Date, Frequency};

use crate::instruments::Bond;

/// A zero coupon (discount) bond.
///
/// Zero coupon bonds pay no periodic coupons; instead they are issued
/// at a discount and pay face value at maturity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZeroCouponBond {
    /// ISIN or identifier.
    isin: String,

    /// Maturity date.
    maturity: Date,

    /// Currency.
    currency: Currency,

    /// Face value (default 100).
    face_value: Decimal,

    /// Issue date.
    issue_date: Option<Date>,
}

impl ZeroCouponBond {
    /// Creates a new zero coupon bond.
    #[must_use]
    pub fn new(
        isin: impl Into<String>,
        maturity: Date,
        currency: Currency,
    ) -> Self {
        Self {
            isin: isin.into(),
            maturity,
            currency,
            face_value: Decimal::ONE_HUNDRED,
            issue_date: None,
        }
    }

    /// Sets the face value.
    #[must_use]
    pub fn with_face_value(mut self, value: Decimal) -> Self {
        self.face_value = value;
        self
    }

    /// Sets the issue date.
    #[must_use]
    pub fn with_issue_date(mut self, date: Date) -> Self {
        self.issue_date = Some(date);
        self
    }

    /// Returns the issue date.
    #[must_use]
    pub fn issue_date(&self) -> Option<Date> {
        self.issue_date
    }
}

impl Bond for ZeroCouponBond {
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
        Frequency::Zero
    }

    fn is_zero_coupon(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_zero_coupon_bond() {
        let bond = ZeroCouponBond::new(
            "US912796XY12",
            Date::from_ymd(2025, 6, 15).unwrap(),
            Currency::USD,
        );

        assert_eq!(bond.identifier(), "US912796XY12");
        assert_eq!(bond.face_value(), dec!(100));
        assert!(bond.is_zero_coupon());
        assert_eq!(bond.frequency(), Frequency::Zero);
    }

    #[test]
    fn test_with_face_value() {
        let bond = ZeroCouponBond::new(
            "TEST",
            Date::from_ymd(2025, 6, 15).unwrap(),
            Currency::USD,
        )
        .with_face_value(dec!(1000));

        assert_eq!(bond.face_value(), dec!(1000));
    }
}
