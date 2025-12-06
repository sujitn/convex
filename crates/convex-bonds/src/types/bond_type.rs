//! Bond type classification.
//!
//! Provides a comprehensive enumeration of bond types for analytics dispatch.

use serde::{Deserialize, Serialize};

/// Classification of bond types.
///
/// Used for:
/// - Analytics dispatch (different pricing models for different types)
/// - Reporting and classification
/// - Risk bucketing
///
/// # Categories
///
/// - **Government**: Treasury, Gilt, Bund, JGB
/// - **Corporate Vanilla**: Fixed rate, zero coupon
/// - **Corporate Floating**: FRN variants (capped, floored, collared)
/// - **Corporate Structured**: Callable, puttable, step-up/down
/// - **Corporate Hybrid**: Convertible, perpetual, AT1
/// - **Municipal**: GO, revenue, pre-refunded
/// - **Agency**: Debentures, callable agencies
/// - **Securitized**: MBS, CMO, ABS
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BondType {
    // ==================== Government ====================
    /// US Treasury Bill (discount instrument, <= 1 year)
    TreasuryBill,
    /// US Treasury Note (2-10 year coupon-bearing)
    TreasuryNote,
    /// US Treasury Bond (>10 year coupon-bearing)
    TreasuryBond,
    /// US Treasury FRN (floating rate note)
    TreasuryFRN,
    /// Treasury Inflation-Protected Securities
    TIPS,
    /// UK Gilt (conventional)
    Gilt,
    /// UK Index-Linked Gilt
    GiltLinker,
    /// German Bund
    Bund,
    /// Japanese Government Bond
    JGB,
    /// Generic sovereign bond
    Sovereign,

    // ==================== Corporate - Vanilla ====================
    /// Fixed rate corporate bond
    FixedRateCorporate,
    /// Zero coupon corporate bond
    ZeroCoupon,

    // ==================== Corporate - Floating ====================
    /// Standard floating rate note
    FloatingRateNote,
    /// Capped floating rate note
    CappedFRN,
    /// Floored floating rate note
    FlooredFRN,
    /// Collared floating rate note (cap + floor)
    CollaredFRN,

    // ==================== Corporate - Structured ====================
    /// Callable bond (issuer can redeem early)
    Callable,
    /// Make-whole callable bond (redemption at treasury + spread)
    MakeWholeCallable,
    /// Puttable bond (holder can sell back early)
    Puttable,
    /// Both callable and puttable
    CallableAndPuttable,
    /// Sinking fund bond (scheduled principal repayments)
    SinkingFund,
    /// Step-up coupon bond
    StepUpCoupon,
    /// Step-down coupon bond
    StepDownCoupon,
    /// Payment-in-Kind bond
    PIK,

    // ==================== Corporate - Hybrid ====================
    /// Convertible bond (can convert to equity)
    Convertible,
    /// Exchangeable bond (can exchange for other company's equity)
    Exchangeable,
    /// Perpetual bond (no maturity)
    Perpetual,
    /// Additional Tier 1 Contingent Convertible
    AT1CoCo,
    /// Tier 2 subordinated debt
    Tier2,

    // ==================== Municipal ====================
    /// General Obligation bond (backed by taxing power)
    GeneralObligation,
    /// Revenue bond (backed by specific revenue source)
    Revenue,
    /// Pre-refunded municipal bond
    PreRefunded,
    /// Build America Bond
    BuildAmericaBond,
    /// Taxable municipal bond
    TaxableMunicipal,

    // ==================== Agency ====================
    /// Agency debenture (Fannie, Freddie, etc.)
    AgencyDebenture,
    /// Callable agency bond
    CallableAgency,

    // ==================== Securitized ====================
    /// Mortgage-backed security pass-through
    MBSPassThrough,
    /// Collateralized Mortgage Obligation
    CMO,
    /// Asset-backed security
    ABS,
    /// Covered bond
    CoveredBond,
    /// Collateralized Loan Obligation
    CLO,
}

impl BondType {
    /// Returns true if this is a government bond type.
    #[must_use]
    pub fn is_government(&self) -> bool {
        matches!(
            self,
            BondType::TreasuryBill
                | BondType::TreasuryNote
                | BondType::TreasuryBond
                | BondType::TreasuryFRN
                | BondType::TIPS
                | BondType::Gilt
                | BondType::GiltLinker
                | BondType::Bund
                | BondType::JGB
                | BondType::Sovereign
        )
    }

    /// Returns true if this is a corporate bond type.
    #[must_use]
    pub fn is_corporate(&self) -> bool {
        matches!(
            self,
            BondType::FixedRateCorporate
                | BondType::ZeroCoupon
                | BondType::FloatingRateNote
                | BondType::CappedFRN
                | BondType::FlooredFRN
                | BondType::CollaredFRN
                | BondType::Callable
                | BondType::MakeWholeCallable
                | BondType::Puttable
                | BondType::CallableAndPuttable
                | BondType::SinkingFund
                | BondType::StepUpCoupon
                | BondType::StepDownCoupon
                | BondType::PIK
                | BondType::Convertible
                | BondType::Exchangeable
                | BondType::Perpetual
                | BondType::AT1CoCo
                | BondType::Tier2
        )
    }

    /// Returns true if this is a municipal bond type.
    #[must_use]
    pub fn is_municipal(&self) -> bool {
        matches!(
            self,
            BondType::GeneralObligation
                | BondType::Revenue
                | BondType::PreRefunded
                | BondType::BuildAmericaBond
                | BondType::TaxableMunicipal
        )
    }

    /// Returns true if this is an agency bond type.
    #[must_use]
    pub fn is_agency(&self) -> bool {
        matches!(self, BondType::AgencyDebenture | BondType::CallableAgency)
    }

    /// Returns true if this is a securitized product.
    #[must_use]
    pub fn is_securitized(&self) -> bool {
        matches!(
            self,
            BondType::MBSPassThrough
                | BondType::CMO
                | BondType::ABS
                | BondType::CoveredBond
                | BondType::CLO
        )
    }

    /// Returns true if this is a structured product with embedded optionality.
    #[must_use]
    pub fn is_structured(&self) -> bool {
        matches!(
            self,
            BondType::Callable
                | BondType::Puttable
                | BondType::CallableAndPuttable
                | BondType::SinkingFund
                | BondType::StepUpCoupon
                | BondType::StepDownCoupon
                | BondType::Convertible
                | BondType::Exchangeable
                | BondType::AT1CoCo
        )
    }

    /// Returns true if this bond type has embedded optionality.
    #[must_use]
    pub fn has_optionality(&self) -> bool {
        matches!(
            self,
            BondType::Callable
                | BondType::MakeWholeCallable
                | BondType::Puttable
                | BondType::CallableAndPuttable
                | BondType::CallableAgency
                | BondType::Convertible
                | BondType::Exchangeable
                | BondType::AT1CoCo
                | BondType::CappedFRN
                | BondType::FlooredFRN
                | BondType::CollaredFRN
        )
    }

    /// Returns true if this bond type requires a pricing model (not just discounting).
    #[must_use]
    pub fn requires_model(&self) -> bool {
        matches!(
            self,
            BondType::Callable
                | BondType::Puttable
                | BondType::CallableAndPuttable
                | BondType::CallableAgency
                | BondType::Convertible
                | BondType::Exchangeable
                | BondType::AT1CoCo
                | BondType::MBSPassThrough
                | BondType::CMO
        )
    }

    /// Returns true if this is a floating rate instrument.
    #[must_use]
    pub fn is_floating(&self) -> bool {
        matches!(
            self,
            BondType::TreasuryFRN
                | BondType::FloatingRateNote
                | BondType::CappedFRN
                | BondType::FlooredFRN
                | BondType::CollaredFRN
        )
    }

    /// Returns true if this is an inflation-linked instrument.
    #[must_use]
    pub fn is_inflation_linked(&self) -> bool {
        matches!(self, BondType::TIPS | BondType::GiltLinker)
    }

    /// Returns true if this is a zero coupon / discount instrument.
    #[must_use]
    pub fn is_zero_coupon(&self) -> bool {
        matches!(self, BondType::TreasuryBill | BondType::ZeroCoupon)
    }

    /// Returns true if this is a perpetual (no maturity) instrument.
    #[must_use]
    pub fn is_perpetual(&self) -> bool {
        matches!(self, BondType::Perpetual | BondType::AT1CoCo)
    }

    /// Returns the asset class category.
    #[must_use]
    pub fn asset_class(&self) -> &'static str {
        if self.is_government() {
            "Government"
        } else if self.is_corporate() {
            "Corporate"
        } else if self.is_municipal() {
            "Municipal"
        } else if self.is_agency() {
            "Agency"
        } else if self.is_securitized() {
            "Securitized"
        } else {
            "Other"
        }
    }
}

impl std::fmt::Display for BondType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            BondType::TreasuryBill => "Treasury Bill",
            BondType::TreasuryNote => "Treasury Note",
            BondType::TreasuryBond => "Treasury Bond",
            BondType::TreasuryFRN => "Treasury FRN",
            BondType::TIPS => "TIPS",
            BondType::Gilt => "Gilt",
            BondType::GiltLinker => "Index-Linked Gilt",
            BondType::Bund => "Bund",
            BondType::JGB => "JGB",
            BondType::Sovereign => "Sovereign",
            BondType::FixedRateCorporate => "Fixed Rate Corporate",
            BondType::ZeroCoupon => "Zero Coupon",
            BondType::FloatingRateNote => "Floating Rate Note",
            BondType::CappedFRN => "Capped FRN",
            BondType::FlooredFRN => "Floored FRN",
            BondType::CollaredFRN => "Collared FRN",
            BondType::Callable => "Callable",
            BondType::MakeWholeCallable => "Make-Whole Callable",
            BondType::Puttable => "Puttable",
            BondType::CallableAndPuttable => "Callable & Puttable",
            BondType::SinkingFund => "Sinking Fund",
            BondType::StepUpCoupon => "Step-Up Coupon",
            BondType::StepDownCoupon => "Step-Down Coupon",
            BondType::PIK => "Payment-in-Kind",
            BondType::Convertible => "Convertible",
            BondType::Exchangeable => "Exchangeable",
            BondType::Perpetual => "Perpetual",
            BondType::AT1CoCo => "AT1 CoCo",
            BondType::Tier2 => "Tier 2",
            BondType::GeneralObligation => "General Obligation",
            BondType::Revenue => "Revenue",
            BondType::PreRefunded => "Pre-Refunded",
            BondType::BuildAmericaBond => "Build America Bond",
            BondType::TaxableMunicipal => "Taxable Municipal",
            BondType::AgencyDebenture => "Agency Debenture",
            BondType::CallableAgency => "Callable Agency",
            BondType::MBSPassThrough => "MBS Pass-Through",
            BondType::CMO => "CMO",
            BondType::ABS => "ABS",
            BondType::CoveredBond => "Covered Bond",
            BondType::CLO => "CLO",
        };
        write!(f, "{}", s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_government() {
        assert!(BondType::TreasuryNote.is_government());
        assert!(BondType::Gilt.is_government());
        assert!(!BondType::FixedRateCorporate.is_government());
    }

    #[test]
    fn test_is_corporate() {
        assert!(BondType::FixedRateCorporate.is_corporate());
        assert!(BondType::Callable.is_corporate());
        assert!(!BondType::TreasuryBond.is_corporate());
    }

    #[test]
    fn test_is_municipal() {
        assert!(BondType::GeneralObligation.is_municipal());
        assert!(BondType::Revenue.is_municipal());
        assert!(!BondType::Callable.is_municipal());
    }

    #[test]
    fn test_has_optionality() {
        assert!(BondType::Callable.has_optionality());
        assert!(BondType::Puttable.has_optionality());
        assert!(BondType::Convertible.has_optionality());
        assert!(!BondType::FixedRateCorporate.has_optionality());
    }

    #[test]
    fn test_requires_model() {
        assert!(BondType::Callable.requires_model());
        assert!(BondType::MBSPassThrough.requires_model());
        assert!(!BondType::TreasuryNote.requires_model());
    }

    #[test]
    fn test_is_floating() {
        assert!(BondType::FloatingRateNote.is_floating());
        assert!(BondType::TreasuryFRN.is_floating());
        assert!(!BondType::FixedRateCorporate.is_floating());
    }

    #[test]
    fn test_is_inflation_linked() {
        assert!(BondType::TIPS.is_inflation_linked());
        assert!(BondType::GiltLinker.is_inflation_linked());
        assert!(!BondType::TreasuryNote.is_inflation_linked());
    }

    #[test]
    fn test_asset_class() {
        assert_eq!(BondType::TreasuryNote.asset_class(), "Government");
        assert_eq!(BondType::Callable.asset_class(), "Corporate");
        assert_eq!(BondType::GeneralObligation.asset_class(), "Municipal");
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", BondType::TreasuryNote), "Treasury Note");
        assert_eq!(format!("{}", BondType::AT1CoCo), "AT1 CoCo");
    }
}
