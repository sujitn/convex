//! Core Bond trait definition.
//!
//! The `Bond` trait defines the common interface for all bond types.

use convex_core::types::Frequency;
use convex_core::{Currency, Date};
use rust_decimal::Decimal;

use crate::types::{BondIdentifiers, BondType, CalendarId};

/// Type of cash flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CashFlowType {
    /// Interest/coupon payment
    Coupon,
    /// Principal payment (redemption or amortization)
    Principal,
    /// Combined coupon and principal payment
    CouponAndPrincipal,
    /// Fee payment
    Fee,
}

/// A single bond cash flow.
///
/// Represents a payment from the bond, including both the amount and timing.
#[derive(Debug, Clone)]
pub struct BondCashFlow {
    /// Payment date
    pub date: Date,
    /// Cash flow amount (absolute value)
    pub amount: Decimal,
    /// Type of cash flow
    pub flow_type: CashFlowType,
    /// Accrual start date (for coupons)
    pub accrual_start: Option<Date>,
    /// Accrual end date (for coupons)
    pub accrual_end: Option<Date>,
    /// Remaining factor (for amortizing bonds)
    pub factor: Decimal,
    /// Reference rate for floating rate instruments (projected or actual)
    pub reference_rate: Option<Decimal>,
}

impl From<BondCashFlow> for convex_core::types::CashFlow {
    fn from(bcf: BondCashFlow) -> Self {
        let cf_type = match bcf.flow_type {
            CashFlowType::Coupon => convex_core::types::CashFlowType::Coupon,
            CashFlowType::Principal => convex_core::types::CashFlowType::Principal,
            CashFlowType::CouponAndPrincipal => {
                convex_core::types::CashFlowType::CouponAndPrincipal
            }
            CashFlowType::Fee => convex_core::types::CashFlowType::Coupon, // Map Fee to Coupon
        };

        let mut cf = convex_core::types::CashFlow::new(bcf.date, bcf.factored_amount(), cf_type);

        if let (Some(start), Some(end)) = (bcf.accrual_start, bcf.accrual_end) {
            cf = cf.with_accrual(start, end);
        }

        if let Some(rate) = bcf.reference_rate {
            cf = cf.with_reference_rate(rate);
        }

        cf
    }
}

impl From<&BondCashFlow> for convex_core::types::CashFlow {
    fn from(bcf: &BondCashFlow) -> Self {
        bcf.clone().into()
    }
}

impl BondCashFlow {
    /// Creates a new coupon cash flow.
    #[must_use]
    pub fn coupon(date: Date, amount: Decimal) -> Self {
        Self {
            date,
            amount,
            flow_type: CashFlowType::Coupon,
            accrual_start: None,
            accrual_end: None,
            factor: Decimal::ONE,
            reference_rate: None,
        }
    }

    /// Creates a new principal cash flow.
    #[must_use]
    pub fn principal(date: Date, amount: Decimal) -> Self {
        Self {
            date,
            amount,
            flow_type: CashFlowType::Principal,
            accrual_start: None,
            accrual_end: None,
            factor: Decimal::ONE,
            reference_rate: None,
        }
    }

    /// Creates a combined coupon and principal cash flow.
    #[must_use]
    pub fn coupon_and_principal(date: Date, coupon: Decimal, principal: Decimal) -> Self {
        Self {
            date,
            amount: coupon + principal,
            flow_type: CashFlowType::CouponAndPrincipal,
            accrual_start: None,
            accrual_end: None,
            factor: Decimal::ONE,
            reference_rate: None,
        }
    }

    /// Sets the accrual period.
    #[must_use]
    pub fn with_accrual(mut self, start: Date, end: Date) -> Self {
        self.accrual_start = Some(start);
        self.accrual_end = Some(end);
        self
    }

    /// Sets the remaining factor.
    #[must_use]
    pub fn with_factor(mut self, factor: Decimal) -> Self {
        self.factor = factor;
        self
    }

    /// Sets the reference rate (for floating rate instruments).
    #[must_use]
    pub fn with_reference_rate(mut self, rate: Decimal) -> Self {
        self.reference_rate = Some(rate);
        self
    }

    /// Returns the factored amount (amount * factor).
    #[must_use]
    pub fn factored_amount(&self) -> Decimal {
        self.amount * self.factor
    }

    /// Returns true if this is a coupon payment.
    #[must_use]
    pub fn is_coupon(&self) -> bool {
        matches!(
            self.flow_type,
            CashFlowType::Coupon | CashFlowType::CouponAndPrincipal
        )
    }

    /// Returns true if this is a principal payment.
    #[must_use]
    pub fn is_principal(&self) -> bool {
        matches!(
            self.flow_type,
            CashFlowType::Principal | CashFlowType::CouponAndPrincipal
        )
    }
}

/// Core bond trait.
///
/// This trait defines the common interface that all bond types must implement.
/// It provides methods for accessing bond characteristics, generating cash flows,
/// and basic pricing.
///
/// # Design Principles
///
/// - **Interface Segregation**: Small, focused methods that can be implemented
///   efficiently for all bond types
/// - **Composability**: Extension traits add specialized behavior
/// - **Flexibility**: Works with both object-safe dyn dispatch and static dispatch
///
/// # Example
///
/// ```rust,ignore
/// use convex_bonds::traits::Bond;
///
/// fn print_bond_info(bond: &dyn Bond) {
///     println!("Bond: {:?}", bond.identifiers().primary_id());
///     println!("Maturity: {}", bond.maturity());
///     println!("Currency: {}", bond.currency());
/// }
/// ```
pub trait Bond {
    // ==================== Identity ====================

    /// Returns the bond's identifiers (ISIN, CUSIP, etc.).
    fn identifiers(&self) -> &BondIdentifiers;

    /// Returns the bond type classification.
    fn bond_type(&self) -> BondType;

    // ==================== Basic Terms ====================

    /// Returns the bond's currency.
    fn currency(&self) -> Currency;

    /// Returns the maturity date.
    ///
    /// For perpetual bonds, this returns None.
    fn maturity(&self) -> Option<Date>;

    /// Returns the issue date.
    fn issue_date(&self) -> Date;

    /// Returns the first settlement date.
    fn first_settlement_date(&self) -> Date;

    /// Returns the dated date (when interest starts accruing).
    ///
    /// This is typically the issue date but can differ.
    fn dated_date(&self) -> Date;

    /// Returns the face/par value per unit.
    fn face_value(&self) -> Decimal;

    /// Returns the coupon payment frequency.
    fn frequency(&self) -> Frequency;

    // ==================== Cash Flow Generation ====================

    /// Generates all cash flows from the given date forward.
    ///
    /// Returns a vector of cash flows sorted by payment date.
    fn cash_flows(&self, from: Date) -> Vec<BondCashFlow>;

    /// Returns the next coupon date after the given date.
    fn next_coupon_date(&self, after: Date) -> Option<Date>;

    /// Returns the previous coupon date before the given date.
    fn previous_coupon_date(&self, before: Date) -> Option<Date>;

    // ==================== Accrued Interest ====================

    /// Calculates accrued interest as of the settlement date.
    ///
    /// Returns the accrued interest per unit of face value.
    fn accrued_interest(&self, settlement: Date) -> Decimal;

    /// Returns the day count convention for accrual calculations.
    fn day_count_convention(&self) -> &str;

    // ==================== Calendar ====================

    /// Returns the payment calendar.
    fn calendar(&self) -> &CalendarId;

    // ==================== Redemption ====================

    /// Returns the redemption value per unit at maturity (typically 100).
    fn redemption_value(&self) -> Decimal {
        Decimal::ONE_HUNDRED
    }

    // ==================== Convenience ====================

    /// Returns true if the bond has matured as of the given date.
    fn has_matured(&self, as_of: Date) -> bool {
        match self.maturity() {
            Some(maturity) => as_of >= maturity,
            None => false, // Perpetual bonds never mature
        }
    }

    /// Returns the years to maturity from the given date.
    fn years_to_maturity(&self, from: Date) -> Option<f64> {
        self.maturity().map(|mat| {
            let days = mat.days_between(&from);
            days as f64 / 365.0
        })
    }

    /// Returns true if this bond type requires a pricing model.
    fn requires_model(&self) -> bool {
        self.bond_type().requires_model()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> Date {
        Date::from_ymd(y, m, d).unwrap()
    }

    #[test]
    fn test_cash_flow_type() {
        let coupon = BondCashFlow::coupon(date(2025, 6, 15), Decimal::new(25, 1));
        assert!(coupon.is_coupon());
        assert!(!coupon.is_principal());

        let principal = BondCashFlow::principal(date(2025, 6, 15), Decimal::ONE_HUNDRED);
        assert!(!principal.is_coupon());
        assert!(principal.is_principal());

        let combined = BondCashFlow::coupon_and_principal(
            date(2025, 6, 15),
            Decimal::new(25, 1),
            Decimal::ONE_HUNDRED,
        );
        assert!(combined.is_coupon());
        assert!(combined.is_principal());
    }

    #[test]
    fn test_factored_amount() {
        let cf = BondCashFlow::coupon(date(2025, 6, 15), Decimal::ONE_HUNDRED)
            .with_factor(Decimal::new(5, 1)); // 0.5 factor

        assert_eq!(cf.factored_amount(), Decimal::new(50, 0));
    }
}
