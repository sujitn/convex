//! Cash flow type for bond analytics.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;

use super::Date;

/// Type of cash flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CashFlowType {
    /// Regular coupon payment
    Coupon,
    /// Principal repayment at maturity
    Principal,
    /// Combined coupon and principal (final payment)
    CouponAndPrincipal,
    /// Sinking fund payment
    SinkingFund,
    /// Call redemption
    Call,
    /// Put redemption
    Put,
}

impl fmt::Display for CashFlowType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            CashFlowType::Coupon => "Coupon",
            CashFlowType::Principal => "Principal",
            CashFlowType::CouponAndPrincipal => "Coupon+Principal",
            CashFlowType::SinkingFund => "Sinking Fund",
            CashFlowType::Call => "Call",
            CashFlowType::Put => "Put",
        };
        write!(f, "{name}")
    }
}

/// A dated cash flow.
///
/// Represents a single cash flow occurring on a specific date.
///
/// # Example
///
/// ```rust
/// use convex_core::types::{CashFlow, CashFlowType, Date};
/// use rust_decimal_macros::dec;
///
/// let cf = CashFlow::new(
///     Date::from_ymd(2025, 6, 15).unwrap(),
///     dec!(2.50),
///     CashFlowType::Coupon,
/// );
/// assert_eq!(cf.amount(), dec!(2.50));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CashFlow {
    /// Payment date
    date: Date,
    /// Cash flow amount (as percentage of notional)
    amount: Decimal,
    /// Type of cash flow
    cf_type: CashFlowType,
}

impl CashFlow {
    /// Creates a new cash flow.
    #[must_use]
    pub fn new(date: Date, amount: Decimal, cf_type: CashFlowType) -> Self {
        Self {
            date,
            amount,
            cf_type,
        }
    }

    /// Creates a coupon cash flow.
    #[must_use]
    pub fn coupon(date: Date, amount: Decimal) -> Self {
        Self::new(date, amount, CashFlowType::Coupon)
    }

    /// Creates a principal cash flow.
    #[must_use]
    pub fn principal(date: Date, amount: Decimal) -> Self {
        Self::new(date, amount, CashFlowType::Principal)
    }

    /// Creates a final cash flow (coupon + principal).
    #[must_use]
    pub fn final_payment(date: Date, coupon: Decimal, principal: Decimal) -> Self {
        Self::new(date, coupon + principal, CashFlowType::CouponAndPrincipal)
    }

    /// Returns the payment date.
    #[must_use]
    pub fn date(&self) -> Date {
        self.date
    }

    /// Returns the cash flow amount.
    #[must_use]
    pub fn amount(&self) -> Decimal {
        self.amount
    }

    /// Returns the cash flow type.
    #[must_use]
    pub fn cf_type(&self) -> CashFlowType {
        self.cf_type
    }

    /// Returns true if this is a coupon payment.
    #[must_use]
    pub fn is_coupon(&self) -> bool {
        matches!(
            self.cf_type,
            CashFlowType::Coupon | CashFlowType::CouponAndPrincipal
        )
    }

    /// Returns true if this includes principal repayment.
    #[must_use]
    pub fn is_principal(&self) -> bool {
        matches!(
            self.cf_type,
            CashFlowType::Principal | CashFlowType::CouponAndPrincipal
        )
    }
}

impl fmt::Display for CashFlow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {} ({})", self.date, self.amount, self.cf_type)
    }
}

/// A schedule of cash flows.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CashFlowSchedule {
    /// Ordered list of cash flows
    cash_flows: Vec<CashFlow>,
}

impl CashFlowSchedule {
    /// Creates a new empty cash flow schedule.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cash_flows: Vec::new(),
        }
    }

    /// Creates a schedule with pre-allocated capacity.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            cash_flows: Vec::with_capacity(capacity),
        }
    }

    /// Adds a cash flow to the schedule.
    pub fn push(&mut self, cf: CashFlow) {
        self.cash_flows.push(cf);
    }

    /// Returns the cash flows as a slice.
    #[must_use]
    pub fn as_slice(&self) -> &[CashFlow] {
        &self.cash_flows
    }

    /// Returns the number of cash flows.
    #[must_use]
    pub fn len(&self) -> usize {
        self.cash_flows.len()
    }

    /// Returns true if there are no cash flows.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cash_flows.is_empty()
    }

    /// Returns an iterator over the cash flows.
    pub fn iter(&self) -> impl Iterator<Item = &CashFlow> {
        self.cash_flows.iter()
    }

    /// Returns the total of all cash flows.
    #[must_use]
    pub fn total(&self) -> Decimal {
        self.cash_flows.iter().map(|cf| cf.amount).sum()
    }

    /// Sorts cash flows by date.
    pub fn sort_by_date(&mut self) {
        self.cash_flows.sort_by_key(|cf| cf.date);
    }

    /// Filters cash flows after a given date.
    #[must_use]
    pub fn after(&self, date: Date) -> Self {
        Self {
            cash_flows: self
                .cash_flows
                .iter()
                .filter(|cf| cf.date > date)
                .copied()
                .collect(),
        }
    }
}

impl IntoIterator for CashFlowSchedule {
    type Item = CashFlow;
    type IntoIter = std::vec::IntoIter<CashFlow>;

    fn into_iter(self) -> Self::IntoIter {
        self.cash_flows.into_iter()
    }
}

impl<'a> IntoIterator for &'a CashFlowSchedule {
    type Item = &'a CashFlow;
    type IntoIter = std::slice::Iter<'a, CashFlow>;

    fn into_iter(self) -> Self::IntoIter {
        self.cash_flows.iter()
    }
}

impl FromIterator<CashFlow> for CashFlowSchedule {
    fn from_iter<I: IntoIterator<Item = CashFlow>>(iter: I) -> Self {
        Self {
            cash_flows: iter.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_cashflow_creation() {
        let date = Date::from_ymd(2025, 6, 15).unwrap();
        let cf = CashFlow::coupon(date, dec!(2.50));

        assert_eq!(cf.amount(), dec!(2.50));
        assert!(cf.is_coupon());
        assert!(!cf.is_principal());
    }

    #[test]
    fn test_final_payment() {
        let date = Date::from_ymd(2030, 6, 15).unwrap();
        let cf = CashFlow::final_payment(date, dec!(2.50), dec!(100.0));

        assert_eq!(cf.amount(), dec!(102.50));
        assert!(cf.is_coupon());
        assert!(cf.is_principal());
    }

    #[test]
    fn test_schedule() {
        let mut schedule = CashFlowSchedule::new();
        schedule.push(CashFlow::coupon(
            Date::from_ymd(2025, 6, 15).unwrap(),
            dec!(2.50),
        ));
        schedule.push(CashFlow::coupon(
            Date::from_ymd(2025, 12, 15).unwrap(),
            dec!(2.50),
        ));

        assert_eq!(schedule.len(), 2);
        assert_eq!(schedule.total(), dec!(5.0));
    }
}
