//! Round-trip cost in bps by instrument class.
//!
//! [`CostFeed`] is the seam — strategies call it to size the "Cost" column
//! on each proposal. [`HeuristicCostFeed`] is the default: plausible mid-2024
//! D2D mids (CBOT TCA monthlies + Bloomberg MOSB on-the-run UST quotes),
//! tagged `cost_model = "heuristic_v1"` on `Provenance`. Live feeds plug in
//! by implementing [`CostFeed`] against a real `QuoteSource` adapter.

use super::types::HedgeInstrument;
use convex_core::types::Currency;

/// Stable identifier of the heuristic feed; surfaced via [`HeuristicCostFeed::name`]
/// and echoed in `Provenance::cost_model`. Re-exported for callers that
/// build a default `Provenance` ahead of any cost computation.
pub const COST_MODEL_NAME: &str = "heuristic_v1";

/// Round-trip cost source for hedge proposals. Sync — pre-fetch quotes at
/// the boundary if your underlying source is async.
pub trait CostFeed {
    /// Round-trip cost in bps of notional for one hedge instrument.
    fn cost_bps(&self, instrument: &HedgeInstrument) -> f64;
    /// Identifier echoed on `Provenance::cost_model` so the JSON output
    /// names its source.
    fn name(&self) -> &str;
}

/// Default feed: hardcoded plausible-mid bps by instrument class. Replace
/// with a real quote-driven feed in production.
#[derive(Debug, Clone, Copy, Default)]
pub struct HeuristicCostFeed;

impl CostFeed for HeuristicCostFeed {
    fn cost_bps(&self, instrument: &HedgeInstrument) -> f64 {
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

    fn name(&self) -> &str {
        COST_MODEL_NAME
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::hedging::ctd::Deliverable;
    use crate::risk::hedging::types::{BondFuture, InterestRateSwap, SwapSide};
    use convex_core::daycounts::DayCountConvention;
    use convex_core::types::{Currency, Date, Frequency};
    use rust_decimal_macros::dec;

    fn ty() -> HedgeInstrument {
        HedgeInstrument::BondFuture(BondFuture {
            contract_code: "TY".into(),
            underlying_tenor_years: 10.0,
            deliverable_basket: vec![Deliverable {
                name: None,
                coupon_rate_decimal: 0.045,
                maturity: Date::from_ymd(2036, 1, 15).unwrap(),
                conversion_factor: 0.85,
            }],
            delivery_months: 3,
            repo_rate_decimal: 0.043,
            futures_price: None,
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
        let feed = HeuristicCostFeed;
        let mut unknown = match ty() {
            HedgeInstrument::BondFuture(f) => f,
            _ => unreachable!(),
        };
        unknown.contract_code = "ZZ".into();
        assert!(feed.cost_bps(&ty()) < feed.cost_bps(&HedgeInstrument::BondFuture(unknown)));
    }

    #[test]
    fn longer_swap_costs_more() {
        let feed = HeuristicCostFeed;
        assert!(feed.cost_bps(&swap(2.0)) < feed.cost_bps(&swap(10.0)));
        assert!(feed.cost_bps(&swap(10.0)) < feed.cost_bps(&swap(30.0)));
    }

    #[test]
    fn all_costs_are_positive() {
        let feed = HeuristicCostFeed;
        for tenor in [2.0, 5.0, 10.0, 30.0] {
            assert!(feed.cost_bps(&swap(tenor)) > 0.0);
        }
        assert!(feed.cost_bps(&ty()) > 0.0);
    }

    #[test]
    fn name_is_stable_identifier() {
        assert_eq!(HeuristicCostFeed.name(), COST_MODEL_NAME);
    }
}
