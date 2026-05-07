//! Hedge advisor wire types.
//!
//! Sign convention: a long bond has positive DV01; a pay-fixed swap has
//! negative DV01. A hedge neutralizes when `position.dv01 + Σ trade.dv01 ≈ 0`.
//! `RiskProfile::notional_face` and `CashBondLeg::face_amount` are signed;
//! `InterestRateSwap::notional` is unsigned (direction lives on `SwapSide`).
//!
//! These are JSON wire types — the field name is the doc, surfaced through
//! schemars to the MCP schema.

#![allow(missing_docs)]

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Currency, Frequency};

use crate::risk::profile::{KeyRateBucket, Provenance, RiskProfile};

/// Hedge instrument variants the advisor can recommend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "instrument", rename_all = "snake_case")]
pub enum HedgeInstrument {
    /// CBOT/Eurex/LIFFE listed bond future.
    BondFuture(BondFuture),
    /// Single-currency vanilla IRS.
    InterestRateSwap(InterestRateSwap),
    /// On-the-run sovereign cash bond.
    CashBond(CashBondLeg),
}

/// Swap side from the position's perspective.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SwapSide {
    /// Pay fixed → short-bond hedge.
    PayFixed,
    /// Receive fixed → long-bond exposure.
    ReceiveFixed,
}

/// Bond future descriptor. v1 prices through a synthetic 6%-coupon
/// deliverable so `conversion_factor` is 1.0 by construction; the field is
/// kept for callers that supply real CFs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct BondFuture {
    pub contract_code: String,
    pub underlying_tenor_years: f64,
    pub conversion_factor: f64,
    #[cfg_attr(feature = "schemars", schemars(with = "f64"))]
    pub contract_size_face: Decimal,
    pub currency: Currency,
}

/// Cash on-the-run sovereign hedge leg. The bond's country preset is picked
/// from `currency` (USD → UST, GBP → Gilt, EUR → Bund). `face_amount` is
/// signed (positive = long).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CashBondLeg {
    pub tenor_years: f64,
    pub coupon_rate_decimal: f64,
    pub currency: Currency,
    #[cfg_attr(feature = "schemars", schemars(with = "f64"))]
    pub face_amount: Decimal,
}

/// Vanilla single-currency IRS. Direction is on `side`; `notional` is
/// strictly positive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct InterestRateSwap {
    pub tenor_years: f64,
    pub fixed_rate_decimal: f64,
    pub fixed_frequency: Frequency,
    pub fixed_day_count: DayCountConvention,
    /// `"SOFR"` / `"SONIA"` / `"ESTR"`.
    pub floating_index: String,
    pub side: SwapSide,
    #[cfg_attr(feature = "schemars", schemars(with = "f64"))]
    pub notional: Decimal,
    pub currency: Currency,
}

/// One leg of a hedge proposal.
///
/// `quantity` is the number of contracts for `BondFuture`. For `InterestRateSwap`
/// and `CashBond` it's just `±1.0` recording direction — the actual size lives on
/// the instrument's `notional` / `face_amount`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct HedgeTrade {
    pub instrument: HedgeInstrument,
    pub quantity: f64,
    /// Signed trade DV01 in instrument currency.
    pub dv01: f64,
    /// Optional on the wire — round-trips from LLM agents may drop it.
    #[serde(default)]
    pub key_rate_buckets: Vec<KeyRateBucket>,
}

/// Caller-supplied constraints. Surfaced in [`TradeoffNotes::weaknesses`]
/// per proposal and applied by [`crate::risk::hedging::compare_hedges`]
/// when picking a recommendation.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Constraints {
    /// Max |residual DV01| in position currency.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_residual_dv01: Option<f64>,
    /// Max round-trip cost as bps of position market value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cost_bps: Option<f64>,
    /// Allow-list of strategy names. Empty = all.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_strategies: Vec<String>,
}

/// Residual risk after applying trades.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ResidualRisk {
    /// `position.dv01 + Σ trade.dv01`.
    pub residual_dv01: f64,
    /// Optional on the wire — see [`HedgeTrade::key_rate_buckets`].
    #[serde(default)]
    pub residual_buckets: Vec<KeyRateBucket>,
    /// Σ |bucket.partial_dv01|.
    pub residual_krd_l1_norm: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct TradeoffNotes {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub strengths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub weaknesses: Vec<String>,
}

/// One strategy's proposed hedge. Optional fields default on the wire so an
/// LLM round-trip that drops them still parses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct HedgeProposal {
    pub strategy: String,
    pub trades: Vec<HedgeTrade>,
    pub residual: ResidualRisk,
    /// Cost as bps of position market value.
    pub cost_bps: f64,
    #[cfg_attr(feature = "schemars", schemars(with = "f64"))]
    pub cost_total: Decimal,
    #[serde(default)]
    pub tradeoffs: TradeoffNotes,
    #[serde(default)]
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ComparisonRow {
    pub strategy: String,
    /// Σ trade DV01.
    pub hedge_dv01: f64,
    pub residual_dv01: f64,
    pub residual_krd_l1_norm: f64,
    pub cost_bps: f64,
    #[cfg_attr(feature = "schemars", schemars(with = "f64"))]
    pub cost_total: Decimal,
    /// Source of `cost_bps` / `cost_total` (mirrors `Provenance::cost_model`),
    /// e.g. `"heuristic_v1"`. Surfaced inline so the costs aren't mistaken
    /// for a live broker feed in the JSON output.
    pub cost_source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ComparisonReport {
    pub currency: Currency,
    #[cfg_attr(feature = "schemars", schemars(with = "f64"))]
    pub position_market_value: Decimal,
    pub position_dv01: f64,
    /// One row per proposal, input order.
    pub rows: Vec<ComparisonRow>,
    pub recommendation: Recommendation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RecommendationReason {
    LowestCost,
    SmallestCurvature,
    /// Row met all caller-supplied constraints.
    MeetsConstraints,
    /// Constraints were supplied but nothing met them — fell back to lowest cost.
    NoRowMetConstraints,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Recommendation {
    pub strategy: String,
    /// Index into [`ComparisonReport::rows`].
    pub row_index: usize,
    pub reasons: Vec<RecommendationReason>,
}

/// Sum position + trade DV01 bucket-by-bucket. Tenors are unioned (1e-9
/// match), so trades with off-position tenors contribute rather than drop.
#[must_use]
pub fn residual_from(position: &RiskProfile, trades: &[HedgeTrade]) -> ResidualRisk {
    let trade_dv01: f64 = trades.iter().map(|t| t.dv01).sum();
    let residual_dv01 = position.dv01 + trade_dv01;

    let mut buckets: Vec<KeyRateBucket> = position.key_rate_buckets.clone();
    let mut add_to_bucket = |row: &KeyRateBucket| {
        if let Some(existing) = buckets
            .iter_mut()
            .find(|b| (b.tenor_years - row.tenor_years).abs() < 1e-9)
        {
            existing.partial_dv01 += row.partial_dv01;
        } else {
            buckets.push(*row);
        }
    };
    for trade in trades {
        for tb in &trade.key_rate_buckets {
            add_to_bucket(tb);
        }
    }
    buckets.sort_by(|a, b| {
        a.tenor_years
            .partial_cmp(&b.tenor_years)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let residual_krd_l1_norm = buckets.iter().map(|b| b.partial_dv01.abs()).sum();
    ResidualRisk {
        residual_dv01,
        residual_buckets: buckets,
        residual_krd_l1_norm,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_core::types::{Currency, Date, Frequency};
    use rust_decimal_macros::dec;

    fn position(dv01: f64) -> RiskProfile {
        RiskProfile {
            position_id: None,
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
            key_rate_buckets: vec![
                KeyRateBucket {
                    tenor_years: 5.0,
                    partial_dv01: dv01 * 0.7,
                },
                KeyRateBucket {
                    tenor_years: 10.0,
                    partial_dv01: dv01 * 0.3,
                },
            ],
            provenance: Provenance {
                curves_used: vec!["sofr".into()],
                cost_model: "heuristic_v1".into(),
                advisor_version: env!("CARGO_PKG_VERSION").into(),
            },
        }
    }

    fn future_trade(dv01: f64) -> HedgeTrade {
        HedgeTrade {
            instrument: HedgeInstrument::BondFuture(BondFuture {
                contract_code: "TY".into(),
                underlying_tenor_years: 10.0,
                conversion_factor: 0.85,
                contract_size_face: dec!(100_000),
                currency: Currency::USD,
            }),
            quantity: -50.0,
            dv01,
            key_rate_buckets: vec![
                KeyRateBucket {
                    tenor_years: 5.0,
                    partial_dv01: 0.0,
                },
                KeyRateBucket {
                    tenor_years: 10.0,
                    partial_dv01: dv01,
                },
            ],
        }
    }

    #[test]
    fn instrument_serde_uses_snake_case_tag() {
        let inst = HedgeInstrument::InterestRateSwap(InterestRateSwap {
            tenor_years: 10.0,
            fixed_rate_decimal: 0.045,
            fixed_frequency: Frequency::SemiAnnual,
            fixed_day_count: DayCountConvention::Act360,
            floating_index: "SOFR".into(),
            side: SwapSide::PayFixed,
            notional: dec!(10_000_000),
            currency: Currency::USD,
        });
        let json = serde_json::to_string(&inst).unwrap();
        assert!(json.contains("\"instrument\":\"interest_rate_swap\""));
        assert!(json.contains("\"side\":\"pay_fixed\""));
        assert_eq!(
            serde_json::from_str::<HedgeInstrument>(&json).unwrap(),
            inst
        );
    }

    #[test]
    fn proposal_round_trips() {
        let pos = position(500.0);
        let trade = future_trade(-500.0);
        let p = HedgeProposal {
            strategy: "DurationFutures".into(),
            trades: vec![trade.clone()],
            residual: residual_from(&pos, &[trade]),
            cost_bps: 0.5,
            cost_total: dec!(50.0),
            tradeoffs: TradeoffNotes {
                strengths: vec!["liquid".into()],
                weaknesses: vec!["curvature residual".into()],
            },
            provenance: pos.provenance,
        };
        let parsed: HedgeProposal =
            serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(p, parsed);
    }

    #[test]
    fn residual_zero_when_dv01_matched() {
        let pos = position(500.0);
        let trade = future_trade(-500.0);
        let r = residual_from(&pos, &[trade]);
        assert!(r.residual_dv01.abs() < 1e-9);
    }

    #[test]
    fn residual_dv01_signs_compose() {
        let pos = position(500.0);
        let trade = future_trade(-200.0);
        let r = residual_from(&pos, &[trade]);
        assert!((r.residual_dv01 - 300.0).abs() < 1e-9);
    }

    #[test]
    fn residual_buckets_align_to_position_tenors() {
        let pos = position(500.0);
        let trade = future_trade(-500.0);
        let r = residual_from(&pos, &[trade]);
        let ten = r
            .residual_buckets
            .iter()
            .find(|b| b.tenor_years == 10.0)
            .unwrap();
        // Position 10Y = 150; trade 10Y = -500; residual = -350.
        assert!((ten.partial_dv01 - (-350.0)).abs() < 1e-9);
    }

    #[test]
    fn residual_unions_off_position_trade_tenors() {
        let pos = position(500.0); // position has buckets at 5Y and 10Y only.
        let trade = HedgeTrade {
            instrument: HedgeInstrument::BondFuture(BondFuture {
                contract_code: "TY".into(),
                underlying_tenor_years: 10.0,
                conversion_factor: 1.0,
                contract_size_face: dec!(100_000),
                currency: Currency::USD,
            }),
            quantity: -1.0,
            dv01: -100.0,
            // Trade exposes 2Y and 30Y too — neither in position's ladder.
            key_rate_buckets: vec![
                KeyRateBucket {
                    tenor_years: 2.0,
                    partial_dv01: -25.0,
                },
                KeyRateBucket {
                    tenor_years: 10.0,
                    partial_dv01: -50.0,
                },
                KeyRateBucket {
                    tenor_years: 30.0,
                    partial_dv01: -25.0,
                },
            ],
        };
        let r = residual_from(&pos, &[trade]);
        let by_tenor: std::collections::HashMap<i64, f64> = r
            .residual_buckets
            .iter()
            .map(|b| ((b.tenor_years * 10.0) as i64, b.partial_dv01))
            .collect();
        // Off-position tenors must appear in the residual.
        assert_eq!(by_tenor[&20], -25.0); // 2Y bucket from trade only.
        assert_eq!(by_tenor[&300], -25.0); // 30Y bucket from trade only.
                                           // 10Y: position 150 + trade -50 = 100.
        assert_eq!(by_tenor[&100], 150.0 - 50.0);
        // L1 should include the off-position contributions.
        assert!((r.residual_krd_l1_norm - (25.0 + 350.0 + 100.0 + 25.0)).abs() < 1e-9);
    }

    #[cfg(feature = "schemars")]
    #[test]
    fn proposal_has_json_schema() {
        let s = serde_json::to_string(&schemars::schema_for!(HedgeProposal)).unwrap();
        assert!(s.contains("strategy") && s.contains("trades") && s.contains("residual"));
    }
}
