//! Heuristic transaction-cost model for hedge proposals.
//!
//! Real cost feeds (TCA, broker quotes) are out of scope for v1. The
//! [`HeuristicCostModel`] returns plausible bid-ask half-spreads in basis
//! points by instrument class. Every proposal that uses it stamps
//! `provenance.cost_model = "heuristic_v1"` so the source is unambiguous.

use super::types::HedgeInstrument;

/// Source of cost numbers, for traceability on outputs.
pub trait CostModel {
    /// Round-trip cost in bps of notional for a single hedge instrument.
    fn cost_bps(&self, instrument: &HedgeInstrument) -> f64;
    /// Stable identifier echoed in `Provenance::cost_model`.
    fn name(&self) -> &'static str;
}

/// Default v1 cost model. Numbers are deliberately conservative and labeled.
#[derive(Debug, Clone, Copy, Default)]
pub struct HeuristicCostModel;

impl CostModel for HeuristicCostModel {
    fn cost_bps(&self, instrument: &HedgeInstrument) -> f64 {
        match instrument {
            // CBOT/Eurex/LIFFE listed bond futures: ~0.25 bp round trip on the
            // benchmark contract. TY/FV/Bund are tighter, off-the-runs wider.
            // Bloomberg tickers: TU/FV/TY/US (CBOT), OE/RX (Eurex Schatz/Bobl/Bund), G (Liffe Long Gilt).
            HedgeInstrument::BondFuture(f) => match f.contract_code.as_str() {
                "TU" | "FV" | "TY" | "US" => 0.25,
                "OE" | "RX" => 0.30,
                "G" => 0.40,
                _ => 0.50,
            },
            // Vanilla SOFR/SONIA/€STR swaps: bid-ask widens with tenor; D2D
            // benchmark swaps trade at ~0.5 bp through 10Y, ~1 bp at 30Y.
            // Pay/receive sides are symmetric.
            HedgeInstrument::InterestRateSwap(s) => {
                if s.tenor_years <= 5.0 {
                    0.4
                } else if s.tenor_years <= 10.0 {
                    0.6
                } else if s.tenor_years <= 20.0 {
                    0.8
                } else {
                    1.0
                }
            }
        }
    }

    fn name(&self) -> &'static str {
        "heuristic_v1"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::hedging::types::{BondFuture, InterestRateSwap, SwapSide};
    use convex_core::daycounts::DayCountConvention;
    use convex_core::types::{Currency, Frequency};
    use rust_decimal_macros::dec;

    fn ty() -> HedgeInstrument {
        HedgeInstrument::BondFuture(BondFuture {
            contract_code: "TY".into(),
            underlying_tenor_years: 10.0,
            conversion_factor: 1.0,
            contract_size_face: dec!(100_000),
            currency: Currency::USD,
        })
    }

    fn swap(tenor: f64) -> HedgeInstrument {
        HedgeInstrument::InterestRateSwap(InterestRateSwap {
            tenor_years: tenor,
            fixed_rate_decimal: 0.045,
            fixed_frequency: Frequency::SemiAnnual,
            fixed_day_count: DayCountConvention::Act360,
            floating_index: "SOFR".into(),
            side: SwapSide::PayFixed,
            notional: dec!(10_000_000),
            currency: Currency::USD,
        })
    }

    #[test]
    fn name_is_heuristic_v1() {
        assert_eq!(HeuristicCostModel.name(), "heuristic_v1");
    }

    #[test]
    fn ty_future_cheaper_than_unknown_future() {
        let mut unknown = match ty() {
            HedgeInstrument::BondFuture(f) => f,
            _ => unreachable!(),
        };
        unknown.contract_code = "ZZ".into();
        let cost_ty = HeuristicCostModel.cost_bps(&ty());
        let cost_zz = HeuristicCostModel.cost_bps(&HedgeInstrument::BondFuture(unknown));
        assert!(cost_ty < cost_zz);
    }

    #[test]
    fn longer_swap_costs_more() {
        let m = HeuristicCostModel;
        assert!(m.cost_bps(&swap(2.0)) < m.cost_bps(&swap(10.0)));
        assert!(m.cost_bps(&swap(10.0)) < m.cost_bps(&swap(30.0)));
    }

    #[test]
    fn all_costs_are_positive() {
        let m = HeuristicCostModel;
        for tenor in [2.0, 5.0, 10.0, 30.0] {
            assert!(m.cost_bps(&swap(tenor)) > 0.0);
        }
        assert!(m.cost_bps(&ty()) > 0.0);
    }
}
