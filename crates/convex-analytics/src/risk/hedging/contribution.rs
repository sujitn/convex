//! Per-position contribution to a book-level [`RiskProfile`].
//!
//! After [`aggregate_risk_profiles`] collapses N positions into a single
//! book profile, traders often want to see which positions drove the
//! aggregate: a long-IBM + short-AAPL book that nets to ~zero DV01 looks
//! "hedged" by the aggregate alone but still carries gross exposure worth
//! surfacing.
//!
//! `dv01_share_pct` is keyed off **gross** `Σ |dv01|` rather than signed
//! `Σ dv01` so:
//! - shares always sum to 100%,
//! - long-short books that net to zero don't divide by zero,
//! - long-and-short positions in the same book each receive a positive
//!   share that reflects their gross contribution.
//!
//! Direction is preserved on the signed `dv01` field.
//!
//! [`aggregate_risk_profiles`]: crate::risk::profile::aggregate_risk_profiles

use serde::{Deserialize, Serialize};

use crate::risk::profile::{KeyRateBucket, RiskProfile};

/// One position's contribution to a book aggregate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[allow(missing_docs)]
pub struct PositionContribution {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position_id: Option<String>,
    /// Position's signed DV01 (echoes [`RiskProfile::dv01`]).
    pub dv01: f64,
    /// `|dv01| / Σ |dv01|` as a percentage. Keyed off gross |DV01| so
    /// long-short books still sum to 100% (see module docs).
    pub dv01_share_pct: f64,
    /// Position's KRD ladder, echoed verbatim.
    #[serde(default)]
    pub key_rate_buckets: Vec<KeyRateBucket>,
}

/// Decompose a slice of position [`RiskProfile`]s into per-position
/// contributions. Preserves input order. Returns an empty vector for an
/// empty input.
#[must_use]
pub fn position_contributions(profiles: &[RiskProfile]) -> Vec<PositionContribution> {
    let gross: f64 = profiles.iter().map(|p| p.dv01.abs()).sum();
    profiles
        .iter()
        .map(|p| PositionContribution {
            position_id: p.position_id.clone(),
            dv01: p.dv01,
            dv01_share_pct: if gross > 1e-12 {
                p.dv01.abs() / gross * 100.0
            } else {
                0.0
            },
            key_rate_buckets: p.key_rate_buckets.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::profile::Provenance;
    use approx::assert_relative_eq;
    use convex_core::types::{Currency, Date};
    use rust_decimal_macros::dec;

    fn profile(id: &str, dv01: f64, buckets: Vec<(f64, f64)>) -> RiskProfile {
        RiskProfile {
            position_id: Some(id.into()),
            currency: Currency::USD,
            settlement: Date::from_ymd(2026, 1, 1).unwrap(),
            notional_face: dec!(1_000_000),
            clean_price_per_100: 100.0,
            dirty_price_per_100: 100.0,
            accrued_per_100: 0.0,
            market_value: dec!(1_000_000),
            ytm_decimal: 0.05,
            modified_duration_years: 5.0,
            macaulay_duration_years: 5.13,
            convexity: 30.0,
            dv01,
            key_rate_buckets: buckets
                .into_iter()
                .map(|(t, d)| KeyRateBucket {
                    tenor_years: t,
                    partial_dv01: d,
                })
                .collect(),
            provenance: Provenance::default(),
        }
    }

    #[test]
    fn dv01_echoes_input() {
        let profiles = vec![
            profile("A", 1000.0, vec![(5.0, 700.0), (10.0, 300.0)]),
            profile("B", 500.0, vec![(5.0, 200.0), (10.0, 300.0)]),
        ];
        let contribs = position_contributions(&profiles);
        assert_eq!(contribs.len(), 2);
        assert_eq!(contribs[0].dv01, 1000.0);
        assert_eq!(contribs[1].dv01, 500.0);
    }

    #[test]
    fn shares_sum_to_one_hundred_for_long_only_book() {
        let profiles = vec![
            profile("A", 1000.0, vec![]),
            profile("B", 500.0, vec![]),
            profile("C", 250.0, vec![]),
        ];
        let total: f64 = position_contributions(&profiles)
            .iter()
            .map(|c| c.dv01_share_pct)
            .sum();
        assert_relative_eq!(total, 100.0, epsilon = 1e-9);
    }

    #[test]
    fn shares_sum_to_one_hundred_for_long_short_book() {
        // Long-short netting to zero: shares still sum to 100% of gross.
        let profiles = vec![
            profile("LONG", 5000.0, vec![]),
            profile("SHORT", -5000.0, vec![]),
        ];
        let contribs = position_contributions(&profiles);
        let total: f64 = contribs.iter().map(|c| c.dv01_share_pct).sum();
        assert_relative_eq!(total, 100.0, epsilon = 1e-9);
        assert_relative_eq!(contribs[0].dv01_share_pct, 50.0, epsilon = 1e-9);
        assert_relative_eq!(contribs[1].dv01_share_pct, 50.0, epsilon = 1e-9);
        // Direction preserved on the signed field.
        assert_eq!(contribs[0].dv01, 5000.0);
        assert_eq!(contribs[1].dv01, -5000.0);
    }

    #[test]
    fn zero_book_does_not_divide_by_zero() {
        // All-zero DV01s -> shares are 0, no NaN.
        let profiles = vec![profile("A", 0.0, vec![]), profile("B", 0.0, vec![])];
        let contribs = position_contributions(&profiles);
        assert!(contribs.iter().all(|c| c.dv01_share_pct.is_finite()));
        assert!(contribs.iter().all(|c| c.dv01_share_pct == 0.0));
    }

    #[test]
    fn input_order_preserved() {
        let profiles = vec![
            profile("Z", 100.0, vec![]),
            profile("A", 200.0, vec![]),
            profile("M", 300.0, vec![]),
        ];
        let contribs = position_contributions(&profiles);
        let ids: Vec<&str> = contribs
            .iter()
            .map(|c| c.position_id.as_deref().unwrap())
            .collect();
        assert_eq!(ids, vec!["Z", "A", "M"]);
    }

    #[test]
    fn empty_input_returns_empty() {
        assert!(position_contributions(&[]).is_empty());
    }

    #[test]
    fn key_rate_buckets_echoed_verbatim() {
        let profiles = vec![profile(
            "A",
            1000.0,
            vec![(2.0, 100.0), (5.0, 700.0), (10.0, 200.0)],
        )];
        let c = &position_contributions(&profiles)[0];
        assert_eq!(c.key_rate_buckets.len(), 3);
        assert_eq!(c.key_rate_buckets[1].tenor_years, 5.0);
        assert_eq!(c.key_rate_buckets[1].partial_dv01, 700.0);
    }

    #[test]
    fn missing_position_id_propagates() {
        let mut p = profile("X", 100.0, vec![]);
        p.position_id = None;
        let c = &position_contributions(std::slice::from_ref(&p))[0];
        assert!(c.position_id.is_none());
    }

    #[test]
    fn share_proportional_to_gross_dv01() {
        // 1k + 3k => 25% / 75%. Direction-blind for the share.
        let profiles = vec![
            profile("SMALL", 1000.0, vec![]),
            profile("BIG", -3000.0, vec![]),
        ];
        let contribs = position_contributions(&profiles);
        assert_relative_eq!(contribs[0].dv01_share_pct, 25.0, epsilon = 1e-9);
        assert_relative_eq!(contribs[1].dv01_share_pct, 75.0, epsilon = 1e-9);
    }
}
