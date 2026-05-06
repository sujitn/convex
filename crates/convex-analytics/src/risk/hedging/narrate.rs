//! Deterministic template narrator. No LLM call.
//!
//! Takes a [`ComparisonReport`] and produces a trader-brief paragraph that
//! states the position, lists each candidate with its key tradeoff number,
//! and ends with the recommended pick + the reason tags.

use rust_decimal::prelude::ToPrimitive;

use super::types::{ComparisonReport, RecommendationReason};

fn reason_phrase(r: RecommendationReason) -> &'static str {
    match r {
        RecommendationReason::LowestCost => "lowest cost",
        RecommendationReason::SmallestCurvature => "smallest curvature residual",
        RecommendationReason::MeetsConstraints => "meets your constraints",
        RecommendationReason::NoRowMetConstraints => "no candidate met your constraints",
    }
}

/// Render a `ComparisonReport` into a trader-brief paragraph.
pub fn narrate(report: &ComparisonReport) -> String {
    use std::fmt::Write;

    let mut out = String::with_capacity(512);
    let mv = report.position_market_value.to_f64().unwrap_or(0.0);
    let _ = write!(
        out,
        "Position: {} {:.0} market value, DV01 {} {:.0}.",
        report.currency.code(),
        mv,
        report.currency.code(),
        report.position_dv01,
    );

    if report.rows.is_empty() {
        out.push_str(" No proposals to compare.");
        return out;
    }

    out.push_str(" Candidates: ");
    for (i, row) in report.rows.iter().enumerate() {
        if i > 0 {
            out.push_str("; ");
        }
        let _ = write!(
            out,
            "{} costs {:.2} bp ({} {:.0}), residual DV01 {:.0}, residual KRD L1 {:.0}",
            row.strategy,
            row.cost_bps,
            report.currency.code(),
            row.cost_total.to_f64().unwrap_or(0.0),
            row.residual_dv01,
            row.residual_krd_l1_norm,
        );
    }
    out.push('.');

    let rec = &report.recommendation;
    let reason = if rec.reasons.is_empty() {
        "tie-broken by input order".to_string()
    } else {
        rec.reasons
            .iter()
            .map(|r| reason_phrase(*r))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let _ = write!(out, " Recommend {} ({reason}).", rec.strategy);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::hedging::compare::compare_hedges;
    use crate::risk::hedging::types::{
        BondFuture, Constraints, HedgeInstrument, HedgeProposal, HedgeTrade, ResidualRisk,
        TradeoffNotes,
    };
    use crate::risk::profile::{KeyRateBucket, Provenance, RiskProfile};
    use convex_core::types::{Currency, Date};
    use rust_decimal_macros::dec;

    fn position() -> RiskProfile {
        RiskProfile {
            position_id: None,
            currency: Currency::USD,
            settlement: Date::from_ymd(2026, 1, 15).unwrap(),
            notional_face: dec!(10_000_000),
            clean_price_per_100: 100.0,
            dirty_price_per_100: 100.0,
            accrued_per_100: 0.0,
            market_value: dec!(10_000_000),
            ytm_decimal: 0.05,
            modified_duration_years: 7.0,
            macaulay_duration_years: 7.18,
            convexity: 50.0,
            dv01: 7000.0,
            key_rate_buckets: vec![KeyRateBucket {
                tenor_years: 10.0,
                partial_dv01: 7000.0,
            }],
            provenance: Provenance {
                curves_used: vec!["sofr".into()],
                cost_model: "heuristic_v1".into(),
                advisor_version: env!("CARGO_PKG_VERSION").into(),
            },
        }
    }

    fn proposal(strategy: &str, cost_bps: f64) -> HedgeProposal {
        HedgeProposal {
            strategy: strategy.into(),
            trades: vec![HedgeTrade {
                instrument: HedgeInstrument::BondFuture(BondFuture {
                    contract_code: "TY".into(),
                    underlying_tenor_years: 10.0,
                    conversion_factor: 1.0,
                    contract_size_face: dec!(100_000),
                    currency: Currency::USD,
                }),
                quantity: -85.0,
                dv01: -7000.0,
                key_rate_buckets: vec![],
            }],
            residual: ResidualRisk {
                residual_dv01: 0.0,
                residual_buckets: vec![],
                residual_krd_l1_norm: 1500.0,
            },
            cost_bps,
            cost_total: dec!(490),
            tradeoffs: TradeoffNotes::default(),
            provenance: position().provenance,
        }
    }

    #[test]
    fn narration_mentions_every_strategy_and_recommendation() {
        let proposals = [
            proposal("DurationFutures", 0.25),
            proposal("InterestRateSwap", 0.6),
        ];
        let report = compare_hedges(&position(), &proposals, &Constraints::default()).unwrap();
        let text = narrate(&report);
        assert!(text.contains("DurationFutures"));
        assert!(text.contains("InterestRateSwap"));
        assert!(text.contains("Recommend DurationFutures"));
        assert!(text.contains("USD"));
    }

    #[test]
    fn narrator_is_deterministic() {
        let proposals = [
            proposal("DurationFutures", 0.25),
            proposal("InterestRateSwap", 0.6),
        ];
        let report = compare_hedges(&position(), &proposals, &Constraints::default()).unwrap();
        let a = narrate(&report);
        let b = narrate(&report);
        assert_eq!(a, b);
    }

    #[test]
    fn narration_handles_empty_rows() {
        // Edge case: an empty rows ComparisonReport (e.g., constructed manually).
        let proposals = [proposal("DurationFutures", 0.25)];
        let mut report = compare_hedges(&position(), &proposals, &Constraints::default()).unwrap();
        report.rows.clear();
        let text = narrate(&report);
        assert!(text.contains("No proposals"));
    }
}
