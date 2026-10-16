//! Supplied fixed-bond yield, spread, risk and invoice diagnostics.
//!
//! [`YASCalculator`] computes supported measures from actual supplied inputs;
//! missing convention or pricing context remains an error. Supplied historical
//! comparison values are engineering examples, not vendor/model certification.
//! The unused value-only analysis builder, which manufactured zero for missing
//! measurements, is retired. See `docs/REINVESTMENT.md` for migration details.

mod calculator;
mod invoice;

pub use calculator::{
    BatchYASCalculator, BloombergReference, ValidationFailure, YASCalculator, YASResult,
};
pub use invoice::{
    calculate_accrued_amount, calculate_proceeds, calculate_settlement_date, SettlementInvoice,
    SettlementInvoiceBuilder, SettlementInvoiceError, SettlementInvoiceResult,
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
