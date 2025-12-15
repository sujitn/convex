//! Comprehensive yield calculation rules.
//!
//! This module provides the complete set of rules needed for yield calculations,
//! combining yield convention, compounding method, day count, settlement rules,
//! stub period handling, and ex-dividend rules into a single configuration.

use convex_core::daycounts::DayCountConvention;
use convex_core::types::Frequency;
use serde::{Deserialize, Serialize};

use super::compounding::CompoundingMethod;
use super::ex_dividend::ExDividendRules;
use super::settlement_rules::SettlementRules;
use super::stub_rules::{ReferenceMethod, StubPeriodRules};
use super::yield_convention::{AccruedConvention, RoundingConvention, YieldConvention};

/// Complete rules for yield calculation.
///
/// This struct encapsulates all the conventions and rules needed to calculate
/// yields, prices, and accrued interest for a bond. It serves as the single
/// source of truth for calculation methodology.
///
/// # Example
///
/// ```rust
/// use convex_bonds::types::YieldCalculationRules;
///
/// // Get rules for US Treasury
/// let rules = YieldCalculationRules::us_treasury();
/// assert!(rules.compounding.is_periodic());
///
/// // Get rules for UK Gilt (has ex-dividend)
/// let gilt_rules = YieldCalculationRules::uk_gilt();
/// assert!(gilt_rules.ex_dividend_rules.is_some());
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct YieldCalculationRules {
    /// The yield convention (Street, ISMA, True Yield, etc.)
    pub convention: YieldConvention,

    /// How interest compounds
    pub compounding: CompoundingMethod,

    /// Day count for accrual period calculations
    pub accrual_day_count: DayCountConvention,

    /// Day count for discounting (may differ from accrual)
    pub discount_day_count: DayCountConvention,

    /// Coupon frequency per year
    pub frequency: Frequency,

    /// Settlement date calculation rules
    pub settlement_rules: SettlementRules,

    /// Rules for irregular (stub) periods
    pub stub_rules: StubPeriodRules,

    /// Ex-dividend rules (None if no ex-dividend period)
    pub ex_dividend_rules: Option<ExDividendRules>,

    /// Accrued interest convention
    pub accrued_convention: AccruedConvention,

    /// Rounding convention for final yield/price
    pub rounding: RoundingConvention,

    /// Whether to use sequential roll-forward for short-dated bonds
    pub use_short_date_method: bool,

    /// Threshold (in years) below which to use short-date method
    pub short_date_threshold: f64,

    /// Description of these rules
    pub description: String,
}

impl YieldCalculationRules {
    /// Creates rules for US Treasury Notes/Bonds.
    ///
    /// - Day count: Actual/Actual ICMA
    /// - Compounding: Semi-annual
    /// - Settlement: T+1
    /// - No ex-dividend period
    #[must_use]
    pub fn us_treasury() -> Self {
        Self {
            convention: YieldConvention::StreetConvention,
            compounding: CompoundingMethod::semi_annual(),
            accrual_day_count: DayCountConvention::ActActIcma,
            discount_day_count: DayCountConvention::ActActIcma,
            frequency: Frequency::SemiAnnual,
            settlement_rules: SettlementRules::us_treasury(),
            stub_rules: StubPeriodRules::bloomberg(),
            ex_dividend_rules: None,
            accrued_convention: AccruedConvention::Standard,
            rounding: RoundingConvention::None,
            use_short_date_method: true,
            short_date_threshold: 1.0,
            description: "US Treasury Note/Bond".to_string(),
        }
    }

    /// Creates rules for US Treasury Bills.
    ///
    /// - Day count: Actual/360
    /// - Yield: Discount yield
    /// - Settlement: T+1
    #[must_use]
    pub fn us_treasury_bill() -> Self {
        Self {
            convention: YieldConvention::DiscountYield,
            compounding: CompoundingMethod::Discount,
            accrual_day_count: DayCountConvention::Act360,
            discount_day_count: DayCountConvention::Act360,
            frequency: Frequency::Zero,
            settlement_rules: SettlementRules::us_treasury(),
            stub_rules: StubPeriodRules::regular(),
            ex_dividend_rules: None,
            accrued_convention: AccruedConvention::None,
            rounding: RoundingConvention::None,
            use_short_date_method: false,
            short_date_threshold: 0.0,
            description: "US Treasury Bill".to_string(),
        }
    }

    /// Creates rules for US Corporate Bonds (Investment Grade).
    ///
    /// - Day count: 30/360 US
    /// - Compounding: Semi-annual
    /// - Settlement: T+2
    #[must_use]
    pub fn us_corporate() -> Self {
        Self {
            convention: YieldConvention::StreetConvention,
            compounding: CompoundingMethod::semi_annual(),
            accrual_day_count: DayCountConvention::Thirty360US,
            discount_day_count: DayCountConvention::Thirty360US,
            frequency: Frequency::SemiAnnual,
            settlement_rules: SettlementRules::us_corporate(),
            stub_rules: StubPeriodRules::bloomberg(),
            ex_dividend_rules: None,
            accrued_convention: AccruedConvention::Standard,
            rounding: RoundingConvention::None,
            use_short_date_method: true,
            short_date_threshold: 1.0,
            description: "US Corporate Bond".to_string(),
        }
    }

    /// Creates rules for US Municipal Bonds.
    ///
    /// - Day count: 30/360 US
    /// - Compounding: Semi-annual
    /// - Yield: Municipal yield (tax-equivalent)
    #[must_use]
    pub fn us_municipal() -> Self {
        Self {
            convention: YieldConvention::MunicipalYield,
            compounding: CompoundingMethod::semi_annual(),
            accrual_day_count: DayCountConvention::Thirty360US,
            discount_day_count: DayCountConvention::Thirty360US,
            frequency: Frequency::SemiAnnual,
            settlement_rules: SettlementRules::us_corporate(),
            stub_rules: StubPeriodRules {
                reference_method: ReferenceMethod::USMunicipal,
                ..StubPeriodRules::bloomberg()
            },
            ex_dividend_rules: None,
            accrued_convention: AccruedConvention::Standard,
            rounding: RoundingConvention::None,
            use_short_date_method: true,
            short_date_threshold: 1.0,
            description: "US Municipal Bond".to_string(),
        }
    }

    /// Creates rules for UK Gilts.
    ///
    /// - Day count: Actual/Actual ICMA
    /// - Compounding: Semi-annual (ISMA)
    /// - Settlement: T+1
    /// - Ex-dividend: 7 business days before coupon
    #[must_use]
    pub fn uk_gilt() -> Self {
        Self {
            convention: YieldConvention::ISMA,
            compounding: CompoundingMethod::ActualPeriod { frequency: 2 },
            accrual_day_count: DayCountConvention::ActActIcma,
            discount_day_count: DayCountConvention::ActActIcma,
            frequency: Frequency::SemiAnnual,
            settlement_rules: SettlementRules::uk_gilt(),
            stub_rules: StubPeriodRules::icma(),
            ex_dividend_rules: Some(ExDividendRules::uk_gilt()),
            accrued_convention: AccruedConvention::ExDividend,
            rounding: RoundingConvention::None,
            use_short_date_method: true,
            short_date_threshold: 1.0,
            description: "UK Gilt".to_string(),
        }
    }

    /// Creates rules for German Bunds.
    ///
    /// - Day count: Actual/Actual ICMA
    /// - Compounding: Annual (ISMA)
    /// - Settlement: T+2
    /// - No ex-dividend period
    #[must_use]
    pub fn german_bund() -> Self {
        Self {
            convention: YieldConvention::ISMA,
            compounding: CompoundingMethod::ActualPeriod { frequency: 1 },
            accrual_day_count: DayCountConvention::ActActIcma,
            discount_day_count: DayCountConvention::ActActIcma,
            frequency: Frequency::Annual,
            settlement_rules: SettlementRules::german_bund(),
            stub_rules: StubPeriodRules::icma(),
            ex_dividend_rules: None,
            accrued_convention: AccruedConvention::Standard,
            rounding: RoundingConvention::None,
            use_short_date_method: true,
            short_date_threshold: 1.0,
            description: "German Bundesanleihe".to_string(),
        }
    }

    /// Creates rules for French OATs.
    ///
    /// - Day count: Actual/Actual ICMA
    /// - Compounding: Annual (ISMA)
    /// - Settlement: T+2
    #[must_use]
    pub fn french_oat() -> Self {
        Self {
            convention: YieldConvention::ISMA,
            compounding: CompoundingMethod::ActualPeriod { frequency: 1 },
            accrual_day_count: DayCountConvention::ActActIcma,
            discount_day_count: DayCountConvention::ActActIcma,
            frequency: Frequency::Annual,
            settlement_rules: SettlementRules::french_oat(),
            stub_rules: StubPeriodRules::icma(),
            ex_dividend_rules: None,
            accrued_convention: AccruedConvention::Standard,
            rounding: RoundingConvention::None,
            use_short_date_method: true,
            short_date_threshold: 1.0,
            description: "French OAT".to_string(),
        }
    }

    /// Creates rules for Italian BTPs.
    ///
    /// - Day count: Actual/Actual ICMA
    /// - Compounding: Semi-annual (ISMA)
    /// - Settlement: T+2
    /// - Ex-dividend: Record date based
    #[must_use]
    pub fn italian_btp() -> Self {
        Self {
            convention: YieldConvention::ISMA,
            compounding: CompoundingMethod::ActualPeriod { frequency: 2 },
            accrual_day_count: DayCountConvention::ActActIcma,
            discount_day_count: DayCountConvention::ActActIcma,
            frequency: Frequency::SemiAnnual,
            settlement_rules: SettlementRules::italian_btp(),
            stub_rules: StubPeriodRules::icma(),
            ex_dividend_rules: Some(ExDividendRules::italian_btp()),
            accrued_convention: AccruedConvention::RecordDate,
            rounding: RoundingConvention::None,
            use_short_date_method: true,
            short_date_threshold: 1.0,
            description: "Italian BTP".to_string(),
        }
    }

    /// Creates rules for Spanish Bonos.
    ///
    /// - Day count: Actual/Actual ICMA
    /// - Compounding: Annual (ISMA)
    /// - Settlement: T+2
    #[must_use]
    pub fn spanish_bono() -> Self {
        Self {
            convention: YieldConvention::ISMA,
            compounding: CompoundingMethod::ActualPeriod { frequency: 1 },
            accrual_day_count: DayCountConvention::ActActIcma,
            discount_day_count: DayCountConvention::ActActIcma,
            frequency: Frequency::Annual,
            settlement_rules: SettlementRules::eurobond(),
            stub_rules: StubPeriodRules::icma(),
            ex_dividend_rules: None,
            accrued_convention: AccruedConvention::Standard,
            rounding: RoundingConvention::None,
            use_short_date_method: true,
            short_date_threshold: 1.0,
            description: "Spanish Bono".to_string(),
        }
    }

    /// Creates rules for Japanese JGBs.
    ///
    /// - Day count: Actual/365 Fixed
    /// - Compounding: Simple (no compounding)
    /// - Settlement: T+2
    #[must_use]
    pub fn japanese_jgb() -> Self {
        Self {
            convention: YieldConvention::SimpleYield,
            compounding: CompoundingMethod::Simple,
            accrual_day_count: DayCountConvention::Act365Fixed,
            discount_day_count: DayCountConvention::Act365Fixed,
            frequency: Frequency::SemiAnnual,
            settlement_rules: SettlementRules::japanese_jgb(),
            stub_rules: StubPeriodRules {
                reference_method: ReferenceMethod::Japanese,
                ..StubPeriodRules::regular()
            },
            ex_dividend_rules: None,
            accrued_convention: AccruedConvention::Standard,
            rounding: RoundingConvention::None,
            use_short_date_method: false,
            short_date_threshold: 0.0,
            description: "Japanese Government Bond".to_string(),
        }
    }

    /// Creates rules for Swiss Confederation bonds.
    ///
    /// - Day count: Actual/Actual ICMA
    /// - Compounding: Annual (ISMA)
    /// - Settlement: T+2
    #[must_use]
    pub fn swiss() -> Self {
        Self {
            convention: YieldConvention::ISMA,
            compounding: CompoundingMethod::ActualPeriod { frequency: 1 },
            accrual_day_count: DayCountConvention::ActActIcma,
            discount_day_count: DayCountConvention::ActActIcma,
            frequency: Frequency::Annual,
            settlement_rules: SettlementRules::swiss(),
            stub_rules: StubPeriodRules::icma(),
            ex_dividend_rules: None,
            accrued_convention: AccruedConvention::Standard,
            rounding: RoundingConvention::None,
            use_short_date_method: true,
            short_date_threshold: 1.0,
            description: "Swiss Confederation Bond".to_string(),
        }
    }

    /// Creates rules for Australian government bonds.
    ///
    /// - Day count: Actual/Actual ICMA
    /// - Compounding: Semi-annual
    /// - Settlement: T+2
    /// - Ex-dividend: 7 calendar days before coupon
    #[must_use]
    pub fn australian() -> Self {
        Self {
            convention: YieldConvention::ISMA,
            compounding: CompoundingMethod::ActualPeriod { frequency: 2 },
            accrual_day_count: DayCountConvention::ActActIcma,
            discount_day_count: DayCountConvention::ActActIcma,
            frequency: Frequency::SemiAnnual,
            settlement_rules: SettlementRules::australian(),
            stub_rules: StubPeriodRules::icma(),
            ex_dividend_rules: Some(ExDividendRules::australian()),
            accrued_convention: AccruedConvention::ExDividend,
            rounding: RoundingConvention::None,
            use_short_date_method: true,
            short_date_threshold: 1.0,
            description: "Australian Government Bond".to_string(),
        }
    }

    /// Creates rules for Canadian government bonds.
    ///
    /// - Day count: Actual/365 Fixed
    /// - Compounding: Semi-annual
    /// - Settlement: T+2
    #[must_use]
    pub fn canadian() -> Self {
        Self {
            convention: YieldConvention::StreetConvention,
            compounding: CompoundingMethod::semi_annual(),
            accrual_day_count: DayCountConvention::Act365Fixed,
            discount_day_count: DayCountConvention::Act365Fixed,
            frequency: Frequency::SemiAnnual,
            settlement_rules: SettlementRules::canadian(),
            stub_rules: StubPeriodRules::bloomberg(),
            ex_dividend_rules: None,
            accrued_convention: AccruedConvention::Standard,
            rounding: RoundingConvention::None,
            use_short_date_method: true,
            short_date_threshold: 1.0,
            description: "Canadian Government Bond".to_string(),
        }
    }

    /// Creates rules for Eurobonds (EUR-denominated international bonds).
    ///
    /// - Day count: Actual/Actual ICMA
    /// - Compounding: Annual (ISMA)
    /// - Settlement: T+2
    #[must_use]
    pub fn eurobond() -> Self {
        Self {
            convention: YieldConvention::ISMA,
            compounding: CompoundingMethod::ActualPeriod { frequency: 1 },
            accrual_day_count: DayCountConvention::ActActIcma,
            discount_day_count: DayCountConvention::ActActIcma,
            frequency: Frequency::Annual,
            settlement_rules: SettlementRules::eurobond(),
            stub_rules: StubPeriodRules::icma(),
            ex_dividend_rules: None,
            accrued_convention: AccruedConvention::Standard,
            rounding: RoundingConvention::None,
            use_short_date_method: true,
            short_date_threshold: 1.0,
            description: "Eurobond".to_string(),
        }
    }

    /// Creates rules for continuous compounding (theoretical/derivatives).
    #[must_use]
    pub fn continuous() -> Self {
        Self {
            convention: YieldConvention::Continuous,
            compounding: CompoundingMethod::Continuous,
            accrual_day_count: DayCountConvention::Act365Fixed,
            discount_day_count: DayCountConvention::Act365Fixed,
            frequency: Frequency::Annual,
            settlement_rules: SettlementRules::us_corporate(),
            stub_rules: StubPeriodRules::regular(),
            ex_dividend_rules: None,
            accrued_convention: AccruedConvention::Standard,
            rounding: RoundingConvention::None,
            use_short_date_method: false,
            short_date_threshold: 0.0,
            description: "Continuous Compounding".to_string(),
        }
    }

    /// Creates rules based on the yield convention.
    ///
    /// Useful for quick construction when you know the convention.
    #[must_use]
    pub fn from_convention(convention: YieldConvention) -> Self {
        match convention {
            YieldConvention::StreetConvention => Self::us_corporate(),
            YieldConvention::ISMA => Self::eurobond(),
            YieldConvention::TrueYield => {
                let mut rules = Self::us_treasury();
                rules.convention = YieldConvention::TrueYield;
                rules.description = "True Yield".to_string();
                rules
            }
            YieldConvention::SimpleYield => Self::japanese_jgb(),
            YieldConvention::DiscountYield => Self::us_treasury_bill(),
            YieldConvention::BondEquivalentYield => {
                let mut rules = Self::us_treasury_bill();
                rules.convention = YieldConvention::BondEquivalentYield;
                rules.description = "Bond Equivalent Yield".to_string();
                rules
            }
            YieldConvention::MunicipalYield => Self::us_municipal(),
            YieldConvention::Moosmuller => {
                let mut rules = Self::german_bund();
                rules.convention = YieldConvention::Moosmuller;
                rules.description = "Moosmüller Yield".to_string();
                rules
            }
            YieldConvention::BraessFangmeyer => {
                let mut rules = Self::german_bund();
                rules.convention = YieldConvention::BraessFangmeyer;
                rules.description = "Braess-Fangmeyer Yield".to_string();
                rules
            }
            YieldConvention::Annual => {
                let mut rules = Self::eurobond();
                rules.convention = YieldConvention::Annual;
                rules.compounding = CompoundingMethod::annual();
                rules.description = "Annual Yield".to_string();
                rules
            }
            YieldConvention::Continuous => Self::continuous(),
        }
    }

    /// Returns true if this convention uses ex-dividend.
    #[must_use]
    pub const fn has_ex_dividend(&self) -> bool {
        self.ex_dividend_rules.is_some()
    }

    /// Returns true if this is a short-dated bond methodology.
    #[must_use]
    pub fn is_short_dated(&self, years_to_maturity: f64) -> bool {
        self.use_short_date_method && years_to_maturity < self.short_date_threshold
    }

    /// Returns the compounding periods per year.
    #[must_use]
    pub fn periods_per_year(&self) -> u32 {
        self.frequency.periods_per_year()
    }

    /// Returns whether the rules are for a discount instrument.
    #[must_use]
    pub const fn is_discount_instrument(&self) -> bool {
        matches!(
            self.convention,
            YieldConvention::DiscountYield | YieldConvention::BondEquivalentYield
        )
    }
}

impl Default for YieldCalculationRules {
    fn default() -> Self {
        Self::us_corporate()
    }
}

impl std::fmt::Display for YieldCalculationRules {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} ({}, {}, {})",
            self.description, self.convention, self.compounding, self.settlement_rules
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_us_treasury_rules() {
        let rules = YieldCalculationRules::us_treasury();
        assert_eq!(rules.convention, YieldConvention::StreetConvention);
        assert_eq!(rules.frequency, Frequency::SemiAnnual);
        assert!(!rules.has_ex_dividend());
        assert_eq!(rules.settlement_rules.days, 1);
    }

    #[test]
    fn test_uk_gilt_rules() {
        let rules = YieldCalculationRules::uk_gilt();
        assert_eq!(rules.convention, YieldConvention::ISMA);
        assert!(rules.has_ex_dividend());
        assert_eq!(rules.ex_dividend_rules.as_ref().unwrap().days, 7);
    }

    #[test]
    fn test_german_bund_rules() {
        let rules = YieldCalculationRules::german_bund();
        assert_eq!(rules.convention, YieldConvention::ISMA);
        assert_eq!(rules.frequency, Frequency::Annual);
        assert!(!rules.has_ex_dividend());
    }

    #[test]
    fn test_japanese_jgb_rules() {
        let rules = YieldCalculationRules::japanese_jgb();
        assert_eq!(rules.convention, YieldConvention::SimpleYield);
        assert!(rules.compounding.is_simple());
    }

    #[test]
    fn test_italian_btp_rules() {
        let rules = YieldCalculationRules::italian_btp();
        assert!(rules.has_ex_dividend());
        assert!(rules.ex_dividend_rules.as_ref().unwrap().uses_record_date());
    }

    #[test]
    fn test_from_convention() {
        let rules = YieldCalculationRules::from_convention(YieldConvention::StreetConvention);
        assert_eq!(rules.convention, YieldConvention::StreetConvention);

        let rules = YieldCalculationRules::from_convention(YieldConvention::SimpleYield);
        assert_eq!(rules.convention, YieldConvention::SimpleYield);
    }

    #[test]
    fn test_is_short_dated() {
        let rules = YieldCalculationRules::us_treasury();
        assert!(rules.is_short_dated(0.5));
        assert!(!rules.is_short_dated(2.0));
    }

    #[test]
    fn test_is_discount_instrument() {
        let tbill = YieldCalculationRules::us_treasury_bill();
        assert!(tbill.is_discount_instrument());

        let treasury = YieldCalculationRules::us_treasury();
        assert!(!treasury.is_discount_instrument());
    }

    #[test]
    fn test_display() {
        let rules = YieldCalculationRules::us_treasury();
        let display = format!("{}", rules);
        assert!(display.contains("US Treasury"));
        assert!(display.contains("Street Convention"));
    }
}
