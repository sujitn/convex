//! Heuristic round-trip cost in bps by instrument class.
//!
//! Numbers are plausible mid-2024 D2D mids (CBOT TCA monthlies + Bloomberg
//! MOSB on-the-run UST quotes). Replace with a real feed when one's wired
//! up; advisor outputs are tagged `cost_model = "heuristic_v1"` on
//! `Provenance` so a trader reading the JSON knows the source.

use super::types::HedgeInstrument;
use convex_core::types::Currency;

/// Stable identifier echoed in `Provenance::cost_model`.
pub const COST_MODEL_NAME: &str = "heuristic_v1";

/// Round-trip cost in bps of notional for one hedge instrument.
pub fn cost_bps(instrument: &HedgeInstrument) -> f64 {
    match instrument {
        // CBOT/Eurex/LIFFE benchmark futures: TY/FV/TU/US tightest, Bund
        // close behind, Long Gilt and off-the-runs wider.
        HedgeInstrument::BondFuture(f) => match f.contract_code.as_str() {
            "TU" | "FV" | "TY" | "US" => 0.25,
            "OE" | "RX" => 0.30,
            "G" => 0.40,
            _ => 0.50,
        },
        // SOFR/SONIA/€STR D2D: ~0.4 bp through 5Y, widening to ~1 bp at 30Y.
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
        // OTR sovereigns: USTs ~1 bp front, wider long. Bunds/Gilts mid.
        HedgeInstrument::CashBond(c) => match (c.currency, c.tenor_years) {
            (Currency::USD, t) if t <= 5.0 => 1.0,
            (Currency::USD, t) if t <= 10.0 => 1.5,
            (Currency::USD, _) => 2.5,
            (Currency::GBP, _) | (Currency::EUR, _) => 2.0,
            _ => 3.0,
        },
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
    fn ty_future_cheaper_than_unknown_future() {
        let mut unknown = match ty() {
            HedgeInstrument::BondFuture(f) => f,
            _ => unreachable!(),
        };
        unknown.contract_code = "ZZ".into();
        assert!(cost_bps(&ty()) < cost_bps(&HedgeInstrument::BondFuture(unknown)));
    }

    #[test]
    fn longer_swap_costs_more() {
        assert!(cost_bps(&swap(2.0)) < cost_bps(&swap(10.0)));
        assert!(cost_bps(&swap(10.0)) < cost_bps(&swap(30.0)));
    }

    #[test]
    fn all_costs_are_positive() {
        for tenor in [2.0, 5.0, 10.0, 30.0] {
            assert!(cost_bps(&swap(tenor)) > 0.0);
        }
        assert!(cost_bps(&ty()) > 0.0);
    }
}
