//! Ex-dividend rules for bond markets.
//!
//! This module defines how ex-dividend periods are handled across different
//! markets. During the ex-dividend period, the bond trades without the right
//! to receive the next coupon payment.

use serde::{Deserialize, Serialize};

/// Rules for ex-dividend period handling.
///
/// Ex-dividend rules vary significantly by market:
/// - UK Gilts: 7 business days, negative accrued
/// - Italian BTPs: Record date based
/// - German Bunds: No ex-dividend period
/// - US Treasuries: No ex-dividend period
///
/// # Example
///
/// ```rust
/// use convex_bonds::types::{ExDividendRules, DayType, ExDivAccruedMethod};
///
/// // UK Gilt ex-dividend rules
/// let uk_gilt = ExDividendRules::uk_gilt();
/// assert_eq!(uk_gilt.days, 7);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExDividendRules {
    /// Number of days before coupon date for ex-dividend.
    pub days: u32,
    /// Whether days are business days or calendar days.
    pub day_type: DayType,
    /// How accrued interest is calculated during ex-div period.
    pub accrued_method: ExDivAccruedMethod,
}

impl ExDividendRules {
    /// Creates ex-dividend rules for UK Gilts.
    ///
    /// UK Gilts go ex-dividend 7 business days before the coupon date.
    /// During this period, the buyer does not receive the next coupon,
    /// and accrued interest becomes negative (rebate to seller).
    #[must_use]
    pub const fn uk_gilt() -> Self {
        Self {
            days: 7,
            day_type: DayType::BusinessDays,
            accrued_method: ExDivAccruedMethod::NegativeAccrued,
        }
    }

    /// Creates ex-dividend rules for Italian BTPs.
    ///
    /// Italian government bonds use a record date system where
    /// the holder on the record date receives the coupon.
    /// The ex-dividend date is typically 2 business days before record.
    #[must_use]
    pub const fn italian_btp() -> Self {
        Self {
            days: 2,
            day_type: DayType::BusinessDays,
            accrued_method: ExDivAccruedMethod::RecordDate { days_before: 3 },
        }
    }

    /// Creates ex-dividend rules for Australian government bonds.
    ///
    /// Australian bonds go ex-dividend 7 calendar days before coupon.
    #[must_use]
    pub const fn australian() -> Self {
        Self {
            days: 7,
            day_type: DayType::CalendarDays,
            accrued_method: ExDivAccruedMethod::NegativeAccrued,
        }
    }

    /// Creates ex-dividend rules for South African bonds.
    ///
    /// South African government bonds have a 10 business day ex-div period.
    #[must_use]
    pub const fn south_african() -> Self {
        Self {
            days: 10,
            day_type: DayType::BusinessDays,
            accrued_method: ExDivAccruedMethod::NegativeAccrued,
        }
    }

    /// Creates custom ex-dividend rules.
    #[must_use]
    pub const fn custom(days: u32, day_type: DayType, accrued_method: ExDivAccruedMethod) -> Self {
        Self {
            days,
            day_type,
            accrued_method,
        }
    }

    /// Returns true if accrued interest can be negative.
    #[must_use]
    pub const fn can_have_negative_accrued(&self) -> bool {
        matches!(self.accrued_method, ExDivAccruedMethod::NegativeAccrued)
    }

    /// Returns true if this uses business days.
    #[must_use]
    pub const fn uses_business_days(&self) -> bool {
        matches!(self.day_type, DayType::BusinessDays)
    }

    /// Returns true if this uses a record date system.
    #[must_use]
    pub const fn uses_record_date(&self) -> bool {
        matches!(self.accrued_method, ExDivAccruedMethod::RecordDate { .. })
    }
}

impl std::fmt::Display for ExDividendRules {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {} before coupon ({})",
            self.days, self.day_type, self.accrued_method
        )
    }
}

/// Type of days used for ex-dividend calculation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum DayType {
    /// Business days (excludes weekends and holidays).
    ///
    /// Requires a calendar for calculation. Most common for
    /// developed market government bonds.
    #[default]
    BusinessDays,

    /// Calendar days (all days count).
    ///
    /// Simpler calculation, used in some markets.
    CalendarDays,
}

impl std::fmt::Display for DayType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BusinessDays => write!(f, "business days"),
            Self::CalendarDays => write!(f, "calendar days"),
        }
    }
}

/// Method for calculating accrued interest during ex-dividend period.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ExDivAccruedMethod {
    /// Negative accrued interest (UK Gilts style).
    ///
    /// During the ex-dividend period, accrued interest is calculated
    /// as a negative amount representing the remaining days until the
    /// next coupon. The seller receives a rebate.
    ///
    /// Formula: -Coupon × (Days to coupon / Days in period)
    #[default]
    NegativeAccrued,

    /// Zero accrued during ex-div period.
    ///
    /// Some markets simply set accrued to zero during the ex-dividend
    /// period rather than calculating negative accrued.
    ZeroAccrued,

    /// Record date based calculation (Italian BTPs).
    ///
    /// The coupon goes to whoever holds the bond on the record date.
    /// If settlement is after the record date, the buyer pays no
    /// accrued interest.
    ///
    /// * `days_before` - Number of days before coupon for record date
    RecordDate {
        /// Days before coupon date for record date
        days_before: u32,
    },

    /// Standard accrued continues through ex-div period.
    ///
    /// Used in some markets where the ex-div mechanism is handled
    /// through price adjustment rather than accrued interest.
    StandardContinues,
}

impl ExDivAccruedMethod {
    /// Returns the record date days, if applicable.
    #[must_use]
    pub const fn record_date_days(&self) -> Option<u32> {
        match self {
            Self::RecordDate { days_before } => Some(*days_before),
            _ => None,
        }
    }
}

impl std::fmt::Display for ExDivAccruedMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NegativeAccrued => write!(f, "negative accrued"),
            Self::ZeroAccrued => write!(f, "zero accrued"),
            Self::RecordDate { days_before } => write!(f, "record date T-{}", days_before),
            Self::StandardContinues => write!(f, "standard continues"),
        }
    }
}

/// Ex-dividend status for a settlement date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExDividendStatus {
    /// Settlement is before ex-dividend date (cum-dividend).
    CumDividend,
    /// Settlement is in ex-dividend period.
    ExDividend,
    /// Settlement is on the coupon date (special handling).
    OnCouponDate,
}

impl ExDividendStatus {
    /// Returns true if the buyer will receive the next coupon.
    #[must_use]
    pub const fn receives_coupon(&self) -> bool {
        matches!(self, Self::CumDividend)
    }

    /// Returns true if in the ex-dividend period.
    #[must_use]
    pub const fn is_ex_dividend(&self) -> bool {
        matches!(self, Self::ExDividend)
    }
}

impl std::fmt::Display for ExDividendStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CumDividend => write!(f, "Cum-Dividend"),
            Self::ExDividend => write!(f, "Ex-Dividend"),
            Self::OnCouponDate => write!(f, "On Coupon Date"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uk_gilt_rules() {
        let rules = ExDividendRules::uk_gilt();
        assert_eq!(rules.days, 7);
        assert!(rules.uses_business_days());
        assert!(rules.can_have_negative_accrued());
        assert!(!rules.uses_record_date());
    }

    #[test]
    fn test_italian_btp_rules() {
        let rules = ExDividendRules::italian_btp();
        assert_eq!(rules.days, 2);
        assert!(rules.uses_business_days());
        assert!(rules.uses_record_date());
        assert_eq!(
            rules.accrued_method.record_date_days(),
            Some(3)
        );
    }

    #[test]
    fn test_australian_rules() {
        let rules = ExDividendRules::australian();
        assert_eq!(rules.days, 7);
        assert!(!rules.uses_business_days());
        assert_eq!(rules.day_type, DayType::CalendarDays);
    }

    #[test]
    fn test_ex_dividend_status() {
        assert!(ExDividendStatus::CumDividend.receives_coupon());
        assert!(!ExDividendStatus::ExDividend.receives_coupon());
        assert!(ExDividendStatus::ExDividend.is_ex_dividend());
    }

    #[test]
    fn test_display_formats() {
        let rules = ExDividendRules::uk_gilt();
        assert!(format!("{}", rules).contains("7 business days"));

        assert_eq!(
            format!("{}", ExDivAccruedMethod::NegativeAccrued),
            "negative accrued"
        );
        assert_eq!(
            format!("{}", ExDivAccruedMethod::RecordDate { days_before: 3 }),
            "record date T-3"
        );
    }
}
