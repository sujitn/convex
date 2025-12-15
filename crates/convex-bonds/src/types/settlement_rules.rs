//! Settlement rules for bond markets.
//!
//! This module defines how settlement dates are calculated and
//! what adjustments apply to different markets.

use serde::{Deserialize, Serialize};

/// Rules for settlement date calculation.
///
/// Settlement conventions vary by market:
/// - US Treasuries: T+1
/// - US Corporates: T+2
/// - UK Gilts: T+1
/// - German Bunds: T+2
/// - Eurobonds: T+2
///
/// # Example
///
/// ```rust
/// use convex_bonds::types::SettlementRules;
///
/// // US Treasury settlement
/// let treasury = SettlementRules::us_treasury();
/// assert_eq!(treasury.days, 1);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SettlementRules {
    /// Number of days for settlement (T+n).
    pub days: u32,
    /// Whether to use business days or calendar days.
    pub use_business_days: bool,
    /// How to handle holidays/weekends.
    pub adjustment: SettlementAdjustment,
    /// Whether same-day settlement is allowed.
    pub allow_same_day: bool,
}

impl SettlementRules {
    /// Creates settlement rules for US Treasuries (T+1).
    #[must_use]
    pub const fn us_treasury() -> Self {
        Self {
            days: 1,
            use_business_days: true,
            adjustment: SettlementAdjustment::Following,
            allow_same_day: false,
        }
    }

    /// Creates settlement rules for US Corporates (T+2).
    #[must_use]
    pub const fn us_corporate() -> Self {
        Self {
            days: 2,
            use_business_days: true,
            adjustment: SettlementAdjustment::Following,
            allow_same_day: false,
        }
    }

    /// Creates settlement rules for UK Gilts (T+1).
    #[must_use]
    pub const fn uk_gilt() -> Self {
        Self {
            days: 1,
            use_business_days: true,
            adjustment: SettlementAdjustment::Following,
            allow_same_day: false,
        }
    }

    /// Creates settlement rules for German Bunds (T+2).
    #[must_use]
    pub const fn german_bund() -> Self {
        Self {
            days: 2,
            use_business_days: true,
            adjustment: SettlementAdjustment::Following,
            allow_same_day: false,
        }
    }

    /// Creates settlement rules for Eurobonds (T+2).
    #[must_use]
    pub const fn eurobond() -> Self {
        Self {
            days: 2,
            use_business_days: true,
            adjustment: SettlementAdjustment::Following,
            allow_same_day: false,
        }
    }

    /// Creates settlement rules for Japanese JGBs (T+2).
    #[must_use]
    pub const fn japanese_jgb() -> Self {
        Self {
            days: 2,
            use_business_days: true,
            adjustment: SettlementAdjustment::Following,
            allow_same_day: false,
        }
    }

    /// Creates settlement rules for French OATs (T+2).
    #[must_use]
    pub const fn french_oat() -> Self {
        Self {
            days: 2,
            use_business_days: true,
            adjustment: SettlementAdjustment::Following,
            allow_same_day: false,
        }
    }

    /// Creates settlement rules for Italian BTPs (T+2).
    #[must_use]
    pub const fn italian_btp() -> Self {
        Self {
            days: 2,
            use_business_days: true,
            adjustment: SettlementAdjustment::Following,
            allow_same_day: false,
        }
    }

    /// Creates settlement rules for Swiss Confederation bonds (T+2).
    #[must_use]
    pub const fn swiss() -> Self {
        Self {
            days: 2,
            use_business_days: true,
            adjustment: SettlementAdjustment::Following,
            allow_same_day: false,
        }
    }

    /// Creates settlement rules for Australian government bonds (T+2).
    #[must_use]
    pub const fn australian() -> Self {
        Self {
            days: 2,
            use_business_days: true,
            adjustment: SettlementAdjustment::Following,
            allow_same_day: false,
        }
    }

    /// Creates settlement rules for Canadian government bonds (T+2).
    #[must_use]
    pub const fn canadian() -> Self {
        Self {
            days: 2,
            use_business_days: true,
            adjustment: SettlementAdjustment::Following,
            allow_same_day: false,
        }
    }

    /// Creates custom settlement rules.
    #[must_use]
    pub const fn custom(
        days: u32,
        use_business_days: bool,
        adjustment: SettlementAdjustment,
        allow_same_day: bool,
    ) -> Self {
        Self {
            days,
            use_business_days,
            adjustment,
            allow_same_day,
        }
    }

    /// Creates same-day settlement rules.
    #[must_use]
    pub const fn same_day() -> Self {
        Self {
            days: 0,
            use_business_days: true,
            adjustment: SettlementAdjustment::Following,
            allow_same_day: true,
        }
    }

    /// Returns the settlement period notation (e.g., "T+2").
    #[must_use]
    pub fn notation(&self) -> String {
        if self.days == 0 {
            "T+0".to_string()
        } else {
            format!("T+{}", self.days)
        }
    }
}

impl Default for SettlementRules {
    fn default() -> Self {
        Self::us_corporate()
    }
}

impl std::fmt::Display for SettlementRules {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let day_type = if self.use_business_days {
            "business"
        } else {
            "calendar"
        };
        write!(
            f,
            "{} ({} days, {})",
            self.notation(),
            day_type,
            self.adjustment
        )
    }
}

/// How to adjust settlement date when it falls on a non-business day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum SettlementAdjustment {
    /// Move to the following business day.
    #[default]
    Following,
    /// Move to the preceding business day.
    Preceding,
    /// Move to following unless it crosses month boundary, then preceding.
    ModifiedFollowing,
    /// Move to preceding unless it crosses month boundary, then following.
    ModifiedPreceding,
    /// No adjustment (settle on calendar day even if holiday).
    NoAdjustment,
}

impl SettlementAdjustment {
    /// Returns the name of this adjustment method.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Following => "Following",
            Self::Preceding => "Preceding",
            Self::ModifiedFollowing => "Modified Following",
            Self::ModifiedPreceding => "Modified Preceding",
            Self::NoAdjustment => "No Adjustment",
        }
    }
}

impl std::fmt::Display for SettlementAdjustment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Special settlement situations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum SettlementType {
    /// Regular settlement (standard T+n).
    #[default]
    Regular,
    /// Cash settlement (same day or next day).
    Cash,
    /// Skip settlement (T+n skipping certain days).
    Skip,
    /// When-issued settlement (before auction settles).
    WhenIssued,
    /// Corporate action related settlement.
    CorporateAction,
}

impl std::fmt::Display for SettlementType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Regular => write!(f, "Regular"),
            Self::Cash => write!(f, "Cash"),
            Self::Skip => write!(f, "Skip"),
            Self::WhenIssued => write!(f, "When-Issued"),
            Self::CorporateAction => write!(f, "Corporate Action"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_us_treasury_settlement() {
        let rules = SettlementRules::us_treasury();
        assert_eq!(rules.days, 1);
        assert!(rules.use_business_days);
        assert_eq!(rules.notation(), "T+1");
    }

    #[test]
    fn test_us_corporate_settlement() {
        let rules = SettlementRules::us_corporate();
        assert_eq!(rules.days, 2);
        assert_eq!(rules.notation(), "T+2");
    }

    #[test]
    fn test_same_day_settlement() {
        let rules = SettlementRules::same_day();
        assert_eq!(rules.days, 0);
        assert!(rules.allow_same_day);
        assert_eq!(rules.notation(), "T+0");
    }

    #[test]
    fn test_settlement_display() {
        let rules = SettlementRules::us_treasury();
        let display = format!("{}", rules);
        assert!(display.contains("T+1"));
        assert!(display.contains("business"));
    }

    #[test]
    fn test_settlement_adjustment_default() {
        assert_eq!(
            SettlementAdjustment::default(),
            SettlementAdjustment::Following
        );
    }

    #[test]
    fn test_market_specific_rules() {
        assert_eq!(SettlementRules::uk_gilt().days, 1);
        assert_eq!(SettlementRules::german_bund().days, 2);
        assert_eq!(SettlementRules::french_oat().days, 2);
        assert_eq!(SettlementRules::italian_btp().days, 2);
        assert_eq!(SettlementRules::japanese_jgb().days, 2);
    }
}
