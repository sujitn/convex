//! PnL attribution wire types, config, and provenance.
//!
//! Sign convention: a long bond gains when its price rises; a pay-fixed swap
//! gains when rates rise (it is short the fixed leg). PnL is in the book's
//! base currency; `*_bps` figures are relative to the **t0** market value of
//! the relevant scope (book or position).
//!
//! These are JSON wire types — the field name is the doc, surfaced through
//! schemars to the MCP schema. Conventions mirror `risk::hedging::types`.

#![allow(missing_docs)]

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Currency, Date, Frequency};

use crate::risk::hedging::types::SwapSide;

/// Factor-model id stamped on every attribution's provenance.
pub const FACTOR_MODEL_NAME: &str = "level_slope_curv_v1";
/// Default slope-basis pivot tenor (years) when the caller doesn't set one.
pub const DEFAULT_PIVOT_TENOR_YEARS: f64 = 2.0;

/// Vanilla single-currency IRS valued for PnL at a **fixed** maturity and
/// fixed rate, both pinned at trade.
///
/// Distinct from [`crate::risk::hedging::types::InterestRateSwap`], whose
/// synthetic fixed leg re-derives its maturity from the valuation date
/// (constant-maturity) — correct for a risk snapshot but wrong for the value
/// change of a static swap over a period. PnL needs the swap to age: at `t1`
/// it has `maturity − t1` left, not the original tenor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SwapPnlSpec {
    /// Trade / effective date (the synthetic fixed leg's issue date).
    pub trade_date: Date,
    /// Fixed maturity — does **not** move with the valuation date.
    pub maturity: Date,
    pub fixed_rate_decimal: f64,
    pub fixed_frequency: Frequency,
    pub fixed_day_count: DayCountConvention,
    /// `PayFixed` → gains when rates rise (short the fixed leg).
    pub side: SwapSide,
    /// Strictly positive; direction lives on `side`.
    #[cfg_attr(feature = "schemars", schemars(with = "f64"))]
    pub notional: Decimal,
    pub currency: Currency,
}

/// One attribution factor.
///
/// `carry`/`roll_down` are the time effects on the **static t0 curve** at the
/// held t0 spread (carry = coupon + pull-to-par at constant yield; roll-down
/// = slide along the unchanged curve). The four `curve_*` factors decompose
/// the **observed** `curve_t1 − curve_t0` move; `curve_residual` is the part
/// the level/slope/curvature basis doesn't explain. `spread` is per
/// benchmark. `residual` closes the identity (path order + second-order
/// cross terms) and is reported, never hidden.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PnlFactor {
    Carry,
    RollDown,
    CurveParallel,
    CurveSlope,
    CurveCurvature,
    CurveResidual,
    Spread,
    Residual,
}

impl PnlFactor {
    /// Stable display order for narration and book aggregation.
    pub const ORDER: [PnlFactor; 8] = [
        PnlFactor::Carry,
        PnlFactor::RollDown,
        PnlFactor::CurveParallel,
        PnlFactor::CurveSlope,
        PnlFactor::CurveCurvature,
        PnlFactor::CurveResidual,
        PnlFactor::Spread,
        PnlFactor::Residual,
    ];

    /// Human-readable label for the narrator.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            PnlFactor::Carry => "carry",
            PnlFactor::RollDown => "roll-down",
            PnlFactor::CurveParallel => "curve parallel",
            PnlFactor::CurveSlope => "curve slope",
            PnlFactor::CurveCurvature => "curve curvature",
            PnlFactor::CurveResidual => "curve residual",
            PnlFactor::Spread => "spread",
            PnlFactor::Residual => "residual",
        }
    }
}

/// One factor's PnL contribution. `benchmark` is `Some` only for
/// [`PnlFactor::Spread`] (the mark's benchmark id, e.g. `"DE.BUND.10Y"`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct FactorPnl {
    pub factor: PnlFactor,
    #[cfg_attr(feature = "schemars", schemars(with = "f64"))]
    pub pnl_ccy: Decimal,
    pub pnl_bps: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub benchmark: Option<String>,
}

/// Decomposed curve move (loadings in bp) plus the L1 fit residual. The
/// slope basis matches `ScenarioBump::steepener`'s linear-about-pivot shape
/// so synthetic and decomposed slope agree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CurveBreakdown {
    pub parallel_bps: f64,
    pub slope_bps: f64,
    pub curvature_bps: f64,
    pub pivot_tenor_years: f64,
    /// Σ|unexplained Δr| across the analysis tenors, in bp.
    pub fit_residual_l1_bps: f64,
}

/// Per-position attribution. `kind` is `"bond"` or `"swap"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct PositionAttribution {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position_id: Option<String>,
    pub kind: String,
    #[cfg_attr(feature = "schemars", schemars(with = "f64"))]
    pub market_value_t0: Decimal,
    #[cfg_attr(feature = "schemars", schemars(with = "f64"))]
    pub total_pnl_ccy: Decimal,
    pub total_pnl_bps: f64,
    #[serde(default)]
    pub factors: Vec<FactorPnl>,
    pub curve: CurveBreakdown,
}

/// Audit metadata stamped on every attribution.
///
/// Deterministic by design — **no timestamp**: provenance is the set of
/// inputs that determine the result, not wall-clock. This keeps
/// same-input → same-bytes for reproducible demos.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct AttributionProvenance {
    pub curve_t0_id: String,
    pub curve_t1_id: String,
    pub factor_model: String,
    pub pivot_tenor_years: f64,
    pub tool_version: String,
}

/// Book-level attribution: totals + per-factor + per-position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Attribution {
    pub currency: Currency,
    pub t0: Date,
    pub t1: Date,
    #[cfg_attr(feature = "schemars", schemars(with = "f64"))]
    pub book_market_value_t0: Decimal,
    #[cfg_attr(feature = "schemars", schemars(with = "f64"))]
    pub total_pnl_ccy: Decimal,
    pub total_pnl_bps: f64,
    /// Book factors, summed across positions, in [`PnlFactor::ORDER`]; one
    /// extra `spread` row per distinct benchmark.
    #[serde(default)]
    pub factors: Vec<FactorPnl>,
    /// Per-position, input order preserved.
    #[serde(default)]
    pub positions: Vec<PositionAttribution>,
    #[serde(default)]
    pub provenance: AttributionProvenance,
}

/// Caller-supplied config. Two knobs only — see `docs/pnl-narrator-plan.md`
/// §3.6 (anti-overengineering).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct AttributionConfig {
    /// Slope-basis pivot (years). Defaults to [`DEFAULT_PIVOT_TENOR_YEARS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pivot_tenor_years: Option<f64>,
    /// Analysis grid (years) the curve move is decomposed on. Defaults to the
    /// `curve_t0` pillar tenors (decompose the move where it was observed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis_tenors: Option<Vec<f64>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn d(y: i32, m: u32, day: u32) -> Date {
        Date::from_ymd(y, m, day).unwrap()
    }

    fn sample_attribution() -> Attribution {
        Attribution {
            currency: Currency::EUR,
            t0: d(2026, 5, 7),
            t1: d(2026, 5, 8),
            book_market_value_t0: dec!(25_000_000),
            total_pnl_ccy: dec!(-12_345.67),
            total_pnl_bps: -4.94,
            factors: vec![
                FactorPnl {
                    factor: PnlFactor::Carry,
                    pnl_ccy: dec!(1_900.00),
                    pnl_bps: 0.76,
                    benchmark: None,
                },
                FactorPnl {
                    factor: PnlFactor::Spread,
                    pnl_ccy: dec!(-3_100.00),
                    pnl_bps: -1.24,
                    benchmark: Some("IT.BTP.10Y".into()),
                },
            ],
            positions: vec![PositionAttribution {
                position_id: Some("OAT_10Y".into()),
                kind: "bond".into(),
                market_value_t0: dec!(10_000_000),
                total_pnl_ccy: dec!(-5_000.00),
                total_pnl_bps: -5.0,
                factors: vec![FactorPnl {
                    factor: PnlFactor::CurveParallel,
                    pnl_ccy: dec!(-4_800.00),
                    pnl_bps: -4.8,
                    benchmark: None,
                }],
                curve: CurveBreakdown {
                    parallel_bps: 6.0,
                    slope_bps: 1.5,
                    curvature_bps: -0.5,
                    pivot_tenor_years: 2.0,
                    fit_residual_l1_bps: 0.3,
                },
            }],
            provenance: AttributionProvenance {
                curve_t0_id: "eur_govt_2026_05_07".into(),
                curve_t1_id: "eur_govt_2026_05_08".into(),
                factor_model: FACTOR_MODEL_NAME.into(),
                pivot_tenor_years: 2.0,
                tool_version: env!("CARGO_PKG_VERSION").into(),
            },
        }
    }

    #[test]
    fn attribution_round_trips_via_json() {
        let a = sample_attribution();
        let parsed: Attribution =
            serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
        assert_eq!(a, parsed);
    }

    #[test]
    fn swap_spec_round_trips_via_json() {
        let s = SwapPnlSpec {
            trade_date: d(2026, 5, 1),
            maturity: d(2036, 5, 1),
            fixed_rate_decimal: 0.0285,
            fixed_frequency: Frequency::Annual,
            fixed_day_count: DayCountConvention::Thirty360E,
            side: SwapSide::PayFixed,
            notional: dec!(10_000_000),
            currency: Currency::EUR,
        };
        let parsed: SwapPnlSpec =
            serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(s, parsed);
    }

    #[test]
    fn pnl_factor_serializes_snake_case() {
        let j = serde_json::to_string(&PnlFactor::CurveParallel).unwrap();
        assert_eq!(j, "\"curve_parallel\"");
        let back: PnlFactor = serde_json::from_str("\"roll_down\"").unwrap();
        assert_eq!(back, PnlFactor::RollDown);
    }

    #[test]
    fn factor_order_and_labels_are_total() {
        // Every variant is in ORDER exactly once and has a non-empty label.
        assert_eq!(PnlFactor::ORDER.len(), 8);
        for f in PnlFactor::ORDER {
            assert!(!f.label().is_empty());
        }
    }

    #[test]
    fn config_defaults_are_none() {
        let c = AttributionConfig::default();
        assert!(c.pivot_tenor_years.is_none());
        assert!(c.analysis_tenors.is_none());
    }

    #[cfg(feature = "schemars")]
    #[test]
    fn json_schema_is_derived() {
        let a = serde_json::to_string(&schemars::schema_for!(Attribution)).unwrap();
        assert!(a.contains("positions") && a.contains("provenance") && a.contains("factors"));
        let b = serde_json::to_string(&schemars::schema_for!(SwapPnlSpec)).unwrap();
        assert!(b.contains("maturity") && b.contains("fixed_rate_decimal"));
    }
}
