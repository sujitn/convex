//! Hedge advisor wire types.
//!
//! Sign convention: `dv01` is signed (long bond → +; pay-fixed swap → −).
//! `notional` is signed (long → +). A hedge neutralizes the position when
//! `position.dv01 + Σ trade.dv01 ≈ 0`.

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
    /// Liquid bond future (CBOT TY/FV/TU/US, Eurex Bund, …).
    BondFuture(BondFuture),
    /// Vanilla single-currency interest-rate swap.
    InterestRateSwap(InterestRateSwap),
}

/// Side of a swap from the position's perspective.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SwapSide {
    /// Pay fixed, receive floating. Acts as a short-bond hedge.
    PayFixed,
    /// Receive fixed, pay floating. Acts as a long-bond exposure.
    ReceiveFixed,
}

/// Bond-future descriptor. Names the contract; the strategy resolves
/// `contract_code` to a representative CTD bond internally.
///
/// `conversion_factor` is the CTD conversion factor (CF). v1 uses a synthetic
/// 6%-coupon reference deliverable so CF ≡ 1.0 by construction. Real
/// per-deliverable CFs land with v2 CTD optimization; the field is kept on
/// the wire so callers can override.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct BondFuture {
    /// Bloomberg-style contract ticker (e.g. `"TY"` for CBOT 10-Year).
    pub contract_code: String,
    /// Underlying tenor in years.
    pub underlying_tenor_years: f64,
    /// CTD conversion factor (1.0 for the v1 synthetic deliverable).
    pub conversion_factor: f64,
    /// Face per contract (e.g. 100_000 for CBOT TY).
    #[cfg_attr(feature = "schemars", schemars(with = "f64"))]
    pub contract_size_face: Decimal,
    /// Contract currency.
    pub currency: Currency,
}

/// Interest-rate swap descriptor (single-currency vanilla).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct InterestRateSwap {
    /// Tenor in years.
    pub tenor_years: f64,
    /// Fixed rate as decimal (`0.045` = 4.5%).
    pub fixed_rate_decimal: f64,
    /// Fixed-leg frequency.
    pub fixed_frequency: Frequency,
    /// Fixed-leg day count.
    pub fixed_day_count: DayCountConvention,
    /// Floating index name (`"SOFR"`, `"SONIA"`, `"ESTR"`).
    pub floating_index: String,
    /// Side from the position's perspective.
    pub side: SwapSide,
    /// Notional in `currency`.
    #[cfg_attr(feature = "schemars", schemars(with = "f64"))]
    pub notional: Decimal,
    /// Currency.
    pub currency: Currency,
}

/// One leg of a hedge proposal. `quantity` is the number of contracts for
/// futures and `1.0` (or `-1.0`) for swaps (swap size lives on the
/// instrument itself).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct HedgeTrade {
    /// The instrument.
    pub instrument: HedgeInstrument,
    /// Signed quantity. See struct doc.
    pub quantity: f64,
    /// Trade DV01 in instrument currency, signed.
    pub dv01: f64,
    /// Per-tenor DV01 buckets for the trade, on the same ladder as the
    /// position.
    pub key_rate_buckets: Vec<KeyRateBucket>,
}

/// Caller-supplied constraints on `propose_hedges`. Strategies that cannot
/// honor a constraint surface that in `TradeoffNotes::weaknesses`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Constraints {
    /// Max tolerated absolute residual DV01 in position currency.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_residual_dv01: Option<f64>,
    /// Max round-trip cost in bps of position market value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cost_bps: Option<f64>,
    /// Restrict to a subset of strategies. Empty = all.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_strategies: Vec<String>,
}

/// Residual risk after applying the trades.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ResidualRisk {
    /// `position.dv01 + Σ trade.dv01`.
    pub residual_dv01: f64,
    /// Per-tenor residual buckets.
    pub residual_buckets: Vec<KeyRateBucket>,
    /// Σ |bucket.partial_dv01| — scalar curvature measure.
    pub residual_krd_l1_norm: f64,
}

/// Structured tradeoff notes — strengths/weaknesses bullets.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct TradeoffNotes {
    /// Strengths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub strengths: Vec<String>,
    /// Weaknesses / caveats.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub weaknesses: Vec<String>,
}

/// One hedge proposal from a single strategy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct HedgeProposal {
    /// Strategy name.
    pub strategy: String,
    /// Trade legs.
    pub trades: Vec<HedgeTrade>,
    /// Residual risk.
    pub residual: ResidualRisk,
    /// Cost as bps of position market value.
    pub cost_bps: f64,
    /// Cost in position currency.
    #[cfg_attr(feature = "schemars", schemars(with = "f64"))]
    pub cost_total: Decimal,
    /// Tradeoff notes.
    pub tradeoffs: TradeoffNotes,
    /// Audit metadata.
    pub provenance: Provenance,
}

/// One row of `ComparisonReport`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ComparisonRow {
    /// Strategy name.
    pub strategy: String,
    /// Σ trade DV01.
    pub hedge_dv01: f64,
    /// Position DV01 + Σ trade DV01.
    pub residual_dv01: f64,
    /// L1 norm of residual buckets.
    pub residual_krd_l1_norm: f64,
    /// Cost in bps of position market value.
    pub cost_bps: f64,
    /// Cost in position currency.
    #[cfg_attr(feature = "schemars", schemars(with = "f64"))]
    pub cost_total: Decimal,
}

/// Side-by-side comparison of two or more proposals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ComparisonReport {
    /// Position currency.
    pub currency: Currency,
    /// Position market value at comparison time.
    #[cfg_attr(feature = "schemars", schemars(with = "f64"))]
    pub position_market_value: Decimal,
    /// Position DV01 at comparison time.
    pub position_dv01: f64,
    /// One row per proposal, in input order.
    pub rows: Vec<ComparisonRow>,
    /// Deterministic recommendation seed for the narrator.
    pub recommendation: Recommendation,
}

/// Why a row was recommended. Multiple reasons may apply (e.g. a row that
/// is both lowest-cost and meets all constraints).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RecommendationReason {
    /// Lowest `cost_bps` among the candidates.
    LowestCost,
    /// Smallest `residual_krd_l1_norm` among the candidates.
    SmallestCurvature,
    /// Met all caller-supplied constraints (`max_residual_dv01`, `max_cost_bps`).
    MeetsConstraints,
    /// Constraints were supplied but no row met them — fell back to lowest cost.
    NoRowMetConstraints,
}

/// Deterministic pick — lowest `cost_bps` honoring `Constraints`, tie-broken
/// by smallest `residual_krd_l1_norm`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Recommendation {
    /// Recommended strategy name.
    pub strategy: String,
    /// Index into `ComparisonReport::rows`.
    pub row_index: usize,
    /// Reasons the row was chosen.
    pub reasons: Vec<RecommendationReason>,
}

/// Subtract trade DV01s from position DV01 bucket-by-bucket.
#[must_use]
pub fn residual_from(position: &RiskProfile, trades: &[HedgeTrade]) -> ResidualRisk {
    let trade_dv01: f64 = trades.iter().map(|t| t.dv01).sum();
    let residual_dv01 = position.dv01 + trade_dv01;

    let mut buckets: Vec<KeyRateBucket> = position.key_rate_buckets.clone();
    for trade in trades {
        for tb in &trade.key_rate_buckets {
            if let Some(b) = buckets
                .iter_mut()
                .find(|b| (b.tenor_years - tb.tenor_years).abs() < 1e-9)
            {
                b.partial_dv01 += tb.partial_dv01;
            }
        }
    }
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
                KeyRateBucket { tenor_years: 5.0, partial_dv01: dv01 * 0.7 },
                KeyRateBucket { tenor_years: 10.0, partial_dv01: dv01 * 0.3 },
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
                KeyRateBucket { tenor_years: 5.0, partial_dv01: 0.0 },
                KeyRateBucket { tenor_years: 10.0, partial_dv01: dv01 },
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
        assert_eq!(serde_json::from_str::<HedgeInstrument>(&json).unwrap(), inst);
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
        let parsed: HedgeProposal = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
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
        let ten = r.residual_buckets.iter().find(|b| b.tenor_years == 10.0).unwrap();
        // Position 10Y = 150; trade 10Y = -500; residual = -350.
        assert!((ten.partial_dv01 - (-350.0)).abs() < 1e-9);
    }

    #[cfg(feature = "schemars")]
    #[test]
    fn proposal_has_json_schema() {
        let s = serde_json::to_string(&schemars::schema_for!(HedgeProposal)).unwrap();
        assert!(s.contains("strategy") && s.contains("trades") && s.contains("residual"));
    }
}
