//! Per-position contribution to a book-level [`RiskProfile`].

use serde::{Deserialize, Serialize};

use crate::risk::profile::{KeyRateBucket, RiskProfile};

/// One position's contribution to a book aggregate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[allow(missing_docs)]
pub struct PositionContribution {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position_id: Option<String>,
    /// Signed DV01 — echoes [`RiskProfile::dv01`].
    pub dv01: f64,
    /// `|dv01| / Σ |dv01|` as a percentage. Gross denominator so a
    /// market-neutral book's longs and shorts each get a positive share
    /// summing to 100% (signed shares would divide by zero here).
    pub gross_dv01_share_pct: f64,
    #[serde(default)]
    pub key_rate_buckets: Vec<KeyRateBucket>,
}

/// Decompose a slice of position [`RiskProfile`]s into per-position
/// contributions. Order preserved.
#[must_use]
pub fn position_contributions(profiles: &[RiskProfile]) -> Vec<PositionContribution> {
    let gross: f64 = profiles.iter().map(|p| p.dv01.abs()).sum();
    profiles
        .iter()
        .map(|p| PositionContribution {
            position_id: p.position_id.clone(),
            dv01: p.dv01,
            gross_dv01_share_pct: if gross > 1e-12 {
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
    fn long_short_book_shares_use_gross_denominator() {
        // Long-short netting to zero: shares still sum to 100% of gross,
        // signed dv01 preserves direction.
        let contribs = position_contributions(&[
            profile("LONG", 5000.0, vec![]),
            profile("SHORT", -5000.0, vec![]),
        ]);
        assert_relative_eq!(contribs[0].gross_dv01_share_pct, 50.0, epsilon = 1e-9);
        assert_relative_eq!(contribs[1].gross_dv01_share_pct, 50.0, epsilon = 1e-9);
        assert_eq!(contribs[0].dv01, 5000.0);
        assert_eq!(contribs[1].dv01, -5000.0);
    }

    #[test]
    fn proportional_to_gross_dv01() {
        // 1k vs -3k => 25% / 75%. Direction-blind for the share.
        let contribs = position_contributions(&[
            profile("SMALL", 1000.0, vec![]),
            profile("BIG", -3000.0, vec![]),
        ]);
        assert_relative_eq!(contribs[0].gross_dv01_share_pct, 25.0, epsilon = 1e-9);
        assert_relative_eq!(contribs[1].gross_dv01_share_pct, 75.0, epsilon = 1e-9);
    }

    #[test]
    fn zero_book_does_not_divide_by_zero() {
        let contribs =
            position_contributions(&[profile("A", 0.0, vec![]), profile("B", 0.0, vec![])]);
        assert!(contribs.iter().all(|c| c.gross_dv01_share_pct == 0.0));
    }
}
