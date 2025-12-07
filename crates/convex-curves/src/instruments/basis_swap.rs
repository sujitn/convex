//! Basis swap instrument.
//!
//! Basis swaps exchange floating rates of different tenors or currencies.

use convex_core::Date;

use super::{CurveInstrument, InstrumentType, RateIndex};
use crate::error::CurveResult;
use crate::traits::Curve;

/// Basis Swap.
///
/// A basis swap exchanges two floating rates, either:
/// - Tenor basis: Same currency, different tenors (e.g., 1M SOFR vs 3M SOFR)
/// - Cross-currency basis: Different currencies (e.g., USD vs EUR)
///
/// The spread is quoted on one of the legs.
///
/// # Example
///
/// ```rust,ignore
/// use convex_curves::instruments::BasisSwap;
///
/// // 1M vs 3M SOFR basis swap with 5bp spread on 1M leg
/// let basis = BasisSwap::tenor_basis(
///     effective_date,
///     termination_date,
///     RateIndex::sofr_1m(),
///     RateIndex::sofr_3m(),
///     0.0005,  // 5bp spread
/// );
/// ```
#[derive(Debug, Clone)]
pub struct BasisSwap {
    /// Effective date
    effective_date: Date,
    /// Termination date
    termination_date: Date,
    /// Pay leg index
    pay_index: RateIndex,
    /// Receive leg index
    receive_index: RateIndex,
    /// Spread on pay leg (basis points as decimal)
    spread: f64,
    /// Notional amount
    notional: f64,
}

impl BasisSwap {
    /// Creates a new basis swap.
    #[must_use]
    pub fn new(
        effective_date: Date,
        termination_date: Date,
        pay_index: RateIndex,
        receive_index: RateIndex,
        spread: f64,
    ) -> Self {
        Self {
            effective_date,
            termination_date,
            pay_index,
            receive_index,
            spread,
            notional: 1_000_000.0,
        }
    }

    /// Creates a tenor basis swap (same currency, different tenors).
    #[must_use]
    pub fn tenor_basis(
        effective_date: Date,
        termination_date: Date,
        short_tenor: RateIndex,
        long_tenor: RateIndex,
        spread: f64,
    ) -> Self {
        Self::new(
            effective_date,
            termination_date,
            short_tenor,
            long_tenor,
            spread,
        )
    }

    /// Sets the notional.
    #[must_use]
    pub fn with_notional(mut self, notional: f64) -> Self {
        self.notional = notional;
        self
    }

    /// Returns the effective date.
    #[must_use]
    pub fn effective_date(&self) -> Date {
        self.effective_date
    }

    /// Returns the termination date.
    #[must_use]
    pub fn termination_date(&self) -> Date {
        self.termination_date
    }

    /// Returns the pay index.
    #[must_use]
    pub fn pay_index(&self) -> &RateIndex {
        &self.pay_index
    }

    /// Returns the receive index.
    #[must_use]
    pub fn receive_index(&self) -> &RateIndex {
        &self.receive_index
    }

    /// Returns the spread.
    #[must_use]
    pub fn spread(&self) -> f64 {
        self.spread
    }
}

impl CurveInstrument for BasisSwap {
    fn maturity(&self) -> Date {
        self.termination_date
    }

    fn pv(&self, _curve: &dyn Curve) -> CurveResult<f64> {
        // Basis swap PV requires multi-curve framework
        // For now, return 0 (at-market assumption)
        Ok(0.0)
    }

    fn implied_df(&self, curve: &dyn Curve, _target_pv: f64) -> CurveResult<f64> {
        // For basis swaps, we typically solve for the spread curve
        // rather than the discount curve directly.
        // Return the base curve DF as approximation.
        let ref_date = curve.reference_date();
        let t = ref_date.days_between(&self.termination_date) as f64 / 365.0;
        curve.discount_factor(t)
    }

    fn instrument_type(&self) -> InstrumentType {
        InstrumentType::BasisSwap
    }

    fn description(&self) -> String {
        format!(
            "Basis {} vs {} + {:.2}bp",
            self.pay_index,
            self.receive_index,
            self.spread * 10000.0
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basis_swap_basic() {
        let eff = Date::from_ymd(2025, 1, 3).unwrap();
        let term = Date::from_ymd(2030, 1, 3).unwrap();

        let basis = BasisSwap::tenor_basis(
            eff,
            term,
            RateIndex::sofr_1m(),
            RateIndex::sofr_3m(),
            0.0005,
        );

        assert_eq!(basis.effective_date(), eff);
        assert_eq!(basis.termination_date(), term);
        assert_eq!(basis.spread(), 0.0005);
        assert_eq!(basis.instrument_type(), InstrumentType::BasisSwap);
    }

    #[test]
    fn test_basis_swap_description() {
        let eff = Date::from_ymd(2025, 1, 3).unwrap();
        let term = Date::from_ymd(2030, 1, 3).unwrap();

        let basis = BasisSwap::tenor_basis(
            eff,
            term,
            RateIndex::sofr_1m(),
            RateIndex::sofr_3m(),
            0.0010,
        );

        let desc = basis.description();
        assert!(desc.contains("Basis"));
        assert!(desc.contains("SOFR"));
        assert!(desc.contains("10.00bp"));
    }
}
