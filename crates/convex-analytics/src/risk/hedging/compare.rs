//! Side-by-side comparison of hedge proposals.
//!
//! Pure transformation — flattens proposals into rows and picks the
//! recommended one deterministically. No new math.

use crate::error::{AnalyticsError, AnalyticsResult};
use crate::risk::profile::RiskProfile;

use super::types::{
    ComparisonReport, ComparisonRow, Constraints, HedgeProposal, Recommendation,
    RecommendationReason,
};

/// Build a [`ComparisonReport`] from one position and a slice of proposals.
///
/// Recommendation rule (deterministic):
///   1. Prefer rows that meet `Constraints::max_residual_dv01` and
///      `Constraints::max_cost_bps` if any are supplied.
///   2. Among the remaining (or all rows if no row meets the constraints),
///      pick lowest `cost_bps`.
///   3. Tie-break by smallest `residual_krd_l1_norm`.
///   4. Final tie-break by input order.
pub fn compare_hedges(
    position: &RiskProfile,
    proposals: &[HedgeProposal],
    constraints: &Constraints,
) -> AnalyticsResult<ComparisonReport> {
    if proposals.is_empty() {
        return Err(AnalyticsError::InvalidInput(
            "compare_hedges: no proposals supplied".into(),
        ));
    }
    let rows: Vec<ComparisonRow> = proposals.iter().map(row_for).collect();
    let recommendation = recommend(&rows, constraints);
    Ok(ComparisonReport {
        currency: position.currency,
        position_market_value: position.market_value,
        position_dv01: position.dv01,
        rows,
        recommendation,
    })
}

fn row_for(p: &HedgeProposal) -> ComparisonRow {
    let hedge_dv01 = p.trades.iter().map(|t| t.dv01).sum();
    ComparisonRow {
        strategy: p.strategy.clone(),
        hedge_dv01,
        residual_dv01: p.residual.residual_dv01,
        residual_krd_l1_norm: p.residual.residual_krd_l1_norm,
        cost_bps: p.cost_bps,
        cost_total: p.cost_total,
    }
}

fn recommend(rows: &[ComparisonRow], constraints: &Constraints) -> Recommendation {
    let meets = |row: &ComparisonRow| -> bool {
        if let Some(max) = constraints.max_residual_dv01 {
            if row.residual_dv01.abs() > max {
                return false;
            }
        }
        if let Some(max) = constraints.max_cost_bps {
            if row.cost_bps > max {
                return false;
            }
        }
        true
    };

    let candidate_indices: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter_map(|(i, r)| if meets(r) { Some(i) } else { None })
        .collect();
    let any_met = !candidate_indices.is_empty();
    let pool: Vec<usize> = if any_met {
        candidate_indices
    } else {
        (0..rows.len()).collect()
    };

    // Lowest cost; tie-break smallest residual KRD L1; final tie-break by index.
    let best = pool
        .iter()
        .copied()
        .min_by(|&a, &b| {
            let ra = &rows[a];
            let rb = &rows[b];
            ra.cost_bps
                .partial_cmp(&rb.cost_bps)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(
                    ra.residual_krd_l1_norm
                        .partial_cmp(&rb.residual_krd_l1_norm)
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
                .then(a.cmp(&b))
        })
        .expect("pool is non-empty");

    let mut reasons: Vec<RecommendationReason> = Vec::new();
    let row = &rows[best];
    if rows.iter().all(|r| r.cost_bps >= row.cost_bps - 1e-12) {
        reasons.push(RecommendationReason::LowestCost);
    }
    if rows
        .iter()
        .all(|r| r.residual_krd_l1_norm >= row.residual_krd_l1_norm - 1e-12)
    {
        reasons.push(RecommendationReason::SmallestCurvature);
    }
    if any_met {
        reasons.push(RecommendationReason::MeetsConstraints);
    } else if constraints.max_residual_dv01.is_some() || constraints.max_cost_bps.is_some() {
        reasons.push(RecommendationReason::NoRowMetConstraints);
    }

    Recommendation {
        strategy: row.strategy.clone(),
        row_index: best,
        reasons,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::hedging::types::{
        BondFuture, HedgeInstrument, HedgeProposal, HedgeTrade, ResidualRisk, TradeoffNotes,
    };
    use crate::risk::profile::{KeyRateBucket, Provenance};
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

    fn proposal_named(strategy: &str, cost_bps: f64, residual_krd: f64) -> HedgeProposal {
        let trade = HedgeTrade {
            instrument: HedgeInstrument::BondFuture(BondFuture {
                contract_code: "TY".into(),
                underlying_tenor_years: 10.0,
                conversion_factor: 1.0,
                contract_size_face: dec!(100_000),
                currency: Currency::USD,
            }),
            quantity: -85.0,
            dv01: -7000.0,
            key_rate_buckets: vec![KeyRateBucket {
                tenor_years: 10.0,
                partial_dv01: -7000.0,
            }],
        };
        let residual = ResidualRisk {
            residual_dv01: 0.0,
            residual_buckets: vec![],
            residual_krd_l1_norm: residual_krd,
        };
        HedgeProposal {
            strategy: strategy.into(),
            trades: vec![trade],
            residual,
            cost_bps,
            cost_total: dec!(0),
            tradeoffs: TradeoffNotes::default(),
            provenance: position().provenance,
        }
    }

    #[test]
    fn empty_proposals_errors() {
        let err = compare_hedges(&position(), &[], &Constraints::default());
        assert!(matches!(err, Err(AnalyticsError::InvalidInput(_))));
    }

    #[test]
    fn picks_lowest_cost_when_unconstrained() {
        let proposals = [
            proposal_named("DurationFutures", 0.25, 1500.0),
            proposal_named("InterestRateSwap", 0.6, 200.0),
        ];
        let r = compare_hedges(&position(), &proposals, &Constraints::default()).unwrap();
        assert_eq!(r.recommendation.strategy, "DurationFutures");
        assert_eq!(r.recommendation.row_index, 0);
        assert!(r
            .recommendation
            .reasons
            .contains(&RecommendationReason::LowestCost));
    }

    #[test]
    fn ties_break_by_residual_krd() {
        let proposals = [
            proposal_named("DurationFutures", 0.5, 1500.0),
            proposal_named("InterestRateSwap", 0.5, 200.0),
        ];
        let r = compare_hedges(&position(), &proposals, &Constraints::default()).unwrap();
        assert_eq!(r.recommendation.strategy, "InterestRateSwap");
    }

    #[test]
    fn cost_constraint_filters_pool() {
        let proposals = [
            proposal_named("DurationFutures", 0.25, 1500.0),
            proposal_named("InterestRateSwap", 0.6, 200.0),
        ];
        let constraints = Constraints {
            max_cost_bps: Some(0.4),
            ..Default::default()
        };
        let r = compare_hedges(&position(), &proposals, &constraints).unwrap();
        // Only DurationFutures meets the constraint.
        assert_eq!(r.recommendation.strategy, "DurationFutures");
        assert!(r
            .recommendation
            .reasons
            .contains(&RecommendationReason::MeetsConstraints));
    }

    #[test]
    fn rows_preserve_input_order() {
        let proposals = [
            proposal_named("Z_strategy", 1.0, 100.0),
            proposal_named("A_strategy", 0.1, 100.0),
        ];
        let r = compare_hedges(&position(), &proposals, &Constraints::default()).unwrap();
        assert_eq!(r.rows[0].strategy, "Z_strategy");
        assert_eq!(r.rows[1].strategy, "A_strategy");
        // But recommendation picks A by lower cost.
        assert_eq!(r.recommendation.strategy, "A_strategy");
    }

    #[test]
    fn falls_back_when_no_proposal_meets_constraints() {
        let proposals = [
            proposal_named("DurationFutures", 5.0, 1500.0),
            proposal_named("InterestRateSwap", 6.0, 200.0),
        ];
        let constraints = Constraints {
            max_cost_bps: Some(0.5),
            ..Default::default()
        };
        let r = compare_hedges(&position(), &proposals, &constraints).unwrap();
        // Neither meets; fall back to lowest cost overall (DurationFutures).
        assert_eq!(r.recommendation.strategy, "DurationFutures");
    }

    #[test]
    fn comparison_round_trips_via_json() {
        let proposals = [proposal_named("DurationFutures", 0.25, 1500.0)];
        let r = compare_hedges(&position(), &proposals, &Constraints::default()).unwrap();
        let parsed: ComparisonReport =
            serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(parsed, r);
    }
}
