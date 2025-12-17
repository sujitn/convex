//! # Bloomberg YAS (Yield Analysis System) Replication
//!
//! This module provides comprehensive yield analysis matching Bloomberg's YAS function,
//! including:
//!
//! - **Yield Calculations**: Street convention, true yield, current yield, simple yield
//! - **Spread Calculations**: G-spread, I-spread, Z-spread, ASW spread
//! - **Risk Metrics**: Duration, convexity, DV01
//! - **Settlement Invoice**: Accrued interest, settlement amount
//!
//! ## Bloomberg Validation
//!
//! All calculations are validated against Bloomberg YAS for the reference bond:
//! - Boeing 7.5% 06/15/2025 (CUSIP: 097023AH7)
//! - Settlement: 04/29/2020
//! - Price: 110.503
//!
//! ## Example
//!
//! ```rust,ignore
//! use convex_analytics::yas::*;
//! use convex_bonds::FixedRateBond;
//!
//! let bond = FixedRateBond::builder()
//!     .cusip("097023AH7")
//!     .coupon_rate(0.075)
//!     .maturity(date!(2025-06-15))
//!     .build()?;
//!
//! let calculator = YASCalculator::new(&curve);
//! let result = calculator.analyze(&bond, settlement, dec!(110.503))?;
//!
//! println!("Street Convention: {}%", result.ytm);
//! println!("G-Spread: {} bps", result.g_spread.as_bps());
//! println!("Modified Duration: {}", result.modified_duration());
//! ```

mod analysis;
mod calculator;
mod invoice;

pub use analysis::{YasAnalysis, YasAnalysisBuilder};
pub use calculator::{
    BatchYASCalculator, BloombergReference, ValidationFailure, YASCalculator, YASResult,
};
pub use invoice::{
    calculate_accrued_amount, calculate_proceeds, calculate_settlement_date, SettlementInvoice,
    SettlementInvoiceBuilder,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_exports() {
        // Verify all types are accessible
        let _ = BloombergReference::boeing_2025();
    }
}
