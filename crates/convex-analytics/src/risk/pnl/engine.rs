//! Sequential-repricing attribution engine.
//!
//! Values each position at `t0` and `t1` and splits the change with a
//! **path-ordered waterfall**: carry → roll-down → curve
//! (parallel / slope / curvature / residual) → spread → residual. The
//! pricing core is reused unchanged (`price_from_mark` already takes the
//! valuation date); held-spread reprices use
//! [`ZSpreadCalculator::price_with_spread`] — no root-find.
//!
//! ## Held spread, exactly
//!
//! [`ZSpreadCalculator::calculate`] rounds the implied Z-spread to integer
//! bp. Carrying that through the held-spread reprices would dump a
//! multi-thousand-currency rounding artifact into the residual. So for a
//! **Z-spread mark** the held spread is taken *exactly from the mark* (no
//! solve, no rounding) — `price_from_mark` and `price_with_spread` then use
//! the identical Z, and the residual is machine-zero. For price/yield marks
//! we fall back to the (rounded) solve; the ≤0.5 bp gap lands in the reported
//! residual and is documented, not hidden. The demo marks sovereigns with
//! Z-spreads, so the hero numbers are clean.
//!
//! ## Swap (the hero moment)
//!
//! A swap is its fixed leg, priced Z-flat to the curve (same convention as
//! `interest_rate_swap_risk`), run through the *identical* bond waterfall,
//! then signed: `PayFixed` is short the fixed leg (negate), `ReceiveFixed`
//! is long it. Floating ≈ par at reset (the documented post-LIBOR≈0
//! approximation already shipped in the hedge advisor). So a pay-fixed swap
//! gains when rates rise — offsetting the long bonds.

use std::collections::BTreeMap;

use rust_decimal::prelude::*;
use rust_decimal::Decimal;

use convex_bonds::instruments::FixedRateBond;
use convex_bonds::traits::{Bond, CashFlowType, FixedCouponBond};
use convex_bonds::types::BondIdentifiers;
use convex_core::types::{Currency, Date, Frequency, Mark, Spread, SpreadType};
use convex_curves::bumping::{Scenario, ScenarioBump};
use convex_curves::{DiscreteCurve, RateCurve};
use rust_decimal_macros::dec;

use super::decompose::{decompose_curve_move, CurveComponent, CurveDecomposition};
use super::types::{
    Attribution, AttributionConfig, AttributionProvenance, CurveBreakdown, FactorPnl,
    InterestRateSwapPnlSpec, PnlFactor, PositionAttribution, DEFAULT_PIVOT_TENOR_YEARS,
    FACTOR_MODEL_NAME,
};
use crate::error::{AnalyticsError, AnalyticsResult};
use crate::pricing::price_from_mark;
use crate::risk::hedging::types::SwapSide;
use crate::spreads::ZSpreadCalculator;

/// One resolved position (bonds already constructed by the caller — the MCP
/// layer resolves `BondRef`). Not a wire type; the wire book lives in
/// `convex-mcp` params, mirroring how the hedge advisor resolves bonds.
pub enum ResolvedPosition {
    /// A cash bond position carrying its own t0 and t1 trader marks.
    Bond {
        /// Caller-supplied position id (echoed on the output).
        position_id: Option<String>,
        /// The resolved bond (boxed: it is far larger than the swap variant).
        bond: Box<FixedRateBond>,
        /// Signed face notional (positive = long).
        notional_face: Decimal,
        /// Mark at `t0` (price / yield / Z-spread).
        mark_t0: Mark,
        /// Mark at `t1` (price / yield / Z-spread).
        mark_t1: Mark,
    },
    /// A vanilla IRS valued at a fixed maturity (the gap-4 fix).
    Swap {
        /// Caller-supplied position id (echoed on the output).
        position_id: Option<String>,
        /// Fixed-maturity swap spec.
        spec: InterestRateSwapPnlSpec,
    },
}

/// A resolved book: single base currency, static membership over `[t0, t1]`.
pub struct ResolvedBook {
    /// Single base currency; every position must match it (v1 scope).
    pub base_currency: Currency,
    /// Positions, attributed in input order.
    pub positions: Vec<ResolvedPosition>,
}

/// Per-100 waterfall for one fixed-coupon instrument.
struct Waterfall {
    /// `(factor, pnl_per_100)` in [`PnlFactor::ORDER`]; spread carries no
    /// benchmark here (attached by the caller).
    factors: Vec<(PnlFactor, f64)>,
    curve: CurveBreakdown,
    total_per_100: f64,
}

fn to_f64(d: Decimal, what: &str) -> AnalyticsResult<f64> {
    d.to_f64()
        .ok_or_else(|| AnalyticsError::InvalidInput(format!("{what}: non-finite decimal")))
}

fn dec_of(x: f64, what: &str) -> AnalyticsResult<Decimal> {
    Decimal::from_f64_retain(x)
        .ok_or_else(|| AnalyticsError::InvalidInput(format!("{what}: non-finite f64")))
}

fn bps(pnl_ccy: f64, base_ccy: f64) -> f64 {
    if base_ccy.abs() < 1e-9 {
        0.0
    } else {
        pnl_ccy / base_ccy * 1.0e4
    }
}

/// Held spread (decimal), exactly from a Z-spread mark; otherwise the
/// (integer-bp) implied Z-spread solve. See module docs.
fn held_spread<B: Bond + FixedCouponBond>(
    bond: &B,
    date: Date,
    mark: &Mark,
    curve: &RateCurve<DiscreteCurve>,
    freq: Frequency,
) -> AnalyticsResult<f64> {
    if let Mark::Spread { value, .. } = mark {
        if value.spread_type() == SpreadType::ZSpread {
            return to_f64(value.as_decimal(), "z-spread mark");
        }
    }
    let priced = price_from_mark(bond, date, mark, Some(curve), freq)?;
    let dirty = dec_of(priced.dirty_price_per_100, "dirty")?;
    let z = ZSpreadCalculator::new(curve).calculate(bond, dirty, date)?;
    to_f64(z.as_decimal(), "implied z-spread")
}

/// Coupon cash in `(t0, t1]` per 100 face. Static book (v1): a redemption in
/// the window is out of scope, so only pure `Coupon` flows count.
fn coupon_per_100<B: Bond>(bond: &B, t0: Date, t1: Date) -> AnalyticsResult<f64> {
    let face = to_f64(bond.face_value(), "face_value")?;
    if face.abs() < 1e-9 {
        return Err(AnalyticsError::InvalidInput(
            "bond face_value is zero".into(),
        ));
    }
    let mut sum = 0.0;
    for cf in bond.cash_flows(t0) {
        if cf.date > t0 && cf.date <= t1 && cf.flow_type == CashFlowType::Coupon {
            sum += to_f64(cf.amount, "coupon amount")?;
        }
    }
    Ok(sum * 100.0 / face)
}

/// Dirty price per 100 at a held Z-spread, on `curve_t0` plus an additive
/// component shift (preserves base interpolation, adds the analytic basis).
fn price_on_component<B: Bond + FixedCouponBond>(
    bond: &B,
    base_inner: &DiscreteCurve,
    decomp: &CurveDecomposition,
    component: CurveComponent,
    held: f64,
    date: Date,
) -> f64 {
    let d = decomp.clone();
    let scenario = Scenario::new("pnl_component")
        .with_bump(ScenarioBump::custom("component", move |t| {
            d.component_shift_decimal(component, t)
        }));
    let shifted = RateCurve::new(scenario.apply(base_inner));
    ZSpreadCalculator::new(&shifted).price_with_spread(bond, held, date)
}

/// The path-ordered per-100 waterfall for one fixed-coupon bond.
///
/// `mark_t1`'s `Mark::Spread` benchmark (if any) is the caller's concern; this
/// returns the spread factor unlabeled.
#[allow(clippy::too_many_arguments)]
fn attribute_fixed_bond<B: Bond + FixedCouponBond>(
    bond: &B,
    t0: Date,
    t1: Date,
    curve_t0: &RateCurve<DiscreteCurve>,
    curve_t1: &RateCurve<DiscreteCurve>,
    mark_t0: &Mark,
    mark_t1: &Mark,
    freq: Frequency,
    analysis_tenors: &[f64],
    pivot: f64,
) -> AnalyticsResult<Waterfall> {
    let p0 = price_from_mark(bond, t0, mark_t0, Some(curve_t0), freq)?;
    let p1 = price_from_mark(bond, t1, mark_t1, Some(curve_t1), freq)?;
    let big_p0 = p0.dirty_price_per_100;
    let big_p1 = p1.dirty_price_per_100;
    let ytm0 = p0.ytm_decimal;

    let s0 = held_spread(bond, t0, mark_t0, curve_t0, freq)?;
    let s1 = held_spread(bond, t1, mark_t1, curve_t1, freq)?;
    let coupon = coupon_per_100(bond, t0, t1)?;

    let z0 = ZSpreadCalculator::new(curve_t0);
    let z1 = ZSpreadCalculator::new(curve_t1);

    // Time: advance the date on the static t0 curve at the held t0 spread.
    let v_t1_c0_s0 = z0.price_with_spread(bond, s0, t1);
    let time = (v_t1_c0_s0 - big_p0) + coupon;
    // Carry: pull-to-par at the constant t0 yield (exact, no curve).
    let yield_mark = Mark::Yield {
        value: dec_of(ytm0, "ytm0")?,
        frequency: freq,
    };
    let v_carry = price_from_mark(bond, t1, &yield_mark, None, freq)?.dirty_price_per_100;
    let carry = (v_carry - big_p0) + coupon;
    let roll_down = time - carry;

    // Curve: swap the curve, hold date & spread. Decompose into factors.
    let v_t1_c1_s0 = z1.price_with_spread(bond, s0, t1);
    let curve_total = v_t1_c1_s0 - v_t1_c0_s0;
    let decomp = decompose_curve_move(curve_t0, curve_t1, analysis_tenors, pivot)?;
    let base_inner = curve_t0.inner().clone();
    let parallel = price_on_component(bond, &base_inner, &decomp, CurveComponent::Parallel, s0, t1)
        - v_t1_c0_s0;
    let slope =
        price_on_component(bond, &base_inner, &decomp, CurveComponent::Slope, s0, t1) - v_t1_c0_s0;
    let curvature = price_on_component(
        bond,
        &base_inner,
        &decomp,
        CurveComponent::Curvature,
        s0,
        t1,
    ) - v_t1_c0_s0;
    let curve_residual = curve_total - (parallel + slope + curvature);

    // Spread: move to the t1 spread, curve & date fixed.
    let v_t1_c1_s1 = z1.price_with_spread(bond, s1, t1);
    let spread = v_t1_c1_s1 - v_t1_c1_s0;

    let total = (big_p1 - big_p0) + coupon;
    let explained = carry + roll_down + parallel + slope + curvature + curve_residual + spread;
    let residual = total - explained;

    Ok(Waterfall {
        factors: vec![
            (PnlFactor::Carry, carry),
            (PnlFactor::RollDown, roll_down),
            (PnlFactor::CurveParallel, parallel),
            (PnlFactor::CurveSlope, slope),
            (PnlFactor::CurveCurvature, curvature),
            (PnlFactor::CurveResidual, curve_residual),
            (PnlFactor::Spread, spread),
            (PnlFactor::Residual, residual),
        ],
        curve: CurveBreakdown {
            parallel_bps: decomp.parallel_bps,
            slope_bps: decomp.slope_bps,
            curvature_bps: decomp.curvature_bps,
            pivot_tenor_years: pivot,
            fit_residual_l1_bps: decomp.fit_residual_l1_bps(),
        },
        total_per_100: total,
    })
}

/// Build the swap's fixed-leg bond with maturity & rate **pinned at trade**
/// (the gap-4 fix vs the constant-maturity `interest_rate_swap_risk`).
fn swap_fixed_leg(spec: &InterestRateSwapPnlSpec) -> AnalyticsResult<FixedRateBond> {
    let coupon = dec_of(spec.fixed_rate_decimal, "fixed_rate_decimal")?;
    FixedRateBond::builder()
        .identifiers(BondIdentifiers::new())
        .coupon_rate(coupon)
        .face_value(dec!(100))
        .maturity(spec.maturity)
        .issue_date(spec.trade_date)
        .currency(spec.currency)
        .frequency(spec.fixed_frequency)
        .day_count(spec.fixed_day_count)
        .build()
        .map_err(|e| AnalyticsError::BondError(format!("swap fixed leg build: {e}")))
}

/// Mark's benchmark label, if it is a spread mark.
fn mark_benchmark(mark: &Mark) -> Option<String> {
    match mark {
        Mark::Spread { benchmark, .. } => Some(benchmark.clone()),
        _ => None,
    }
}

/// Attribute a book's `t0 → t1` PnL. See module docs for the waterfall.
#[allow(clippy::too_many_arguments)]
pub fn attribute_pnl(
    book: &ResolvedBook,
    t0: Date,
    t1: Date,
    curve_t0: &RateCurve<DiscreteCurve>,
    curve_t0_id: &str,
    curve_t1: &RateCurve<DiscreteCurve>,
    curve_t1_id: &str,
    config: &AttributionConfig,
) -> AnalyticsResult<Attribution> {
    if book.positions.is_empty() {
        return Err(AnalyticsError::InvalidInput(
            "attribute_pnl: book has no positions".into(),
        ));
    }
    if t1 < t0 {
        return Err(AnalyticsError::InvalidInput(format!(
            "attribute_pnl: t1 ({t1}) precedes t0 ({t0})"
        )));
    }
    let pivot = config
        .pivot_tenor_years
        .unwrap_or(DEFAULT_PIVOT_TENOR_YEARS);
    let analysis_tenors: Vec<f64> = match &config.analysis_tenors {
        Some(v) => v.clone(),
        None => curve_t0.inner().tenors().to_vec(),
    };

    let mut positions: Vec<PositionAttribution> = Vec::with_capacity(book.positions.len());
    // (factor, benchmark) → summed pnl_ccy, for the book roll-up.
    let mut book_factor: BTreeMap<(usize, Option<String>), f64> = BTreeMap::new();
    let mut book_pnl = 0.0_f64;
    let mut book_mv = 0.0_f64;

    for pos in &book.positions {
        let (position_id, kind, currency, base_ccy, scale, sign, wf, benchmark) = match pos {
            ResolvedPosition::Bond {
                position_id,
                bond,
                notional_face,
                mark_t0,
                mark_t1,
            } => {
                let bond: &FixedRateBond = bond;
                if bond.currency() != book.base_currency {
                    return Err(AnalyticsError::InvalidInput(format!(
                        "attribute_pnl: position {:?} currency {:?} != book base {:?}",
                        position_id,
                        bond.currency(),
                        book.base_currency
                    )));
                }
                let freq = bond.frequency();
                let wf = attribute_fixed_bond(
                    bond,
                    t0,
                    t1,
                    curve_t0,
                    curve_t1,
                    mark_t0,
                    mark_t1,
                    freq,
                    &analysis_tenors,
                    pivot,
                )?;
                let notional = to_f64(*notional_face, "notional_face")?;
                let p0 =
                    price_from_mark(bond, t0, mark_t0, Some(curve_t0), freq)?.dirty_price_per_100;
                let scale = notional / 100.0;
                let mv_t0 = scale * p0;
                (
                    position_id.clone(),
                    "bond",
                    bond.currency(),
                    mv_t0,
                    scale,
                    1.0,
                    wf,
                    mark_benchmark(mark_t1),
                )
            }
            ResolvedPosition::Swap { position_id, spec } => {
                if spec.currency != book.base_currency {
                    return Err(AnalyticsError::InvalidInput(format!(
                        "attribute_pnl: swap {:?} currency {:?} != book base {:?}",
                        position_id, spec.currency, book.base_currency
                    )));
                }
                let leg = swap_fixed_leg(spec)?;
                let freq = spec.fixed_frequency;
                // Z-flat to the curve, both dates (same convention as
                // interest_rate_swap_risk). s0 = s1 = 0 → spread factor ≡ 0.
                let zflat = |id: &str| Mark::Spread {
                    value: Spread::new(Decimal::ZERO, SpreadType::ZSpread),
                    benchmark: id.to_string(),
                };
                let wf = attribute_fixed_bond(
                    &leg,
                    t0,
                    t1,
                    curve_t0,
                    curve_t1,
                    &zflat(curve_t0_id),
                    &zflat(curve_t1_id),
                    freq,
                    &analysis_tenors,
                    pivot,
                )?;
                let notional = to_f64(spec.notional, "swap notional")?;
                // PayFixed is short the fixed leg → negate.
                let sign = match spec.side {
                    SwapSide::PayFixed => -1.0,
                    SwapSide::ReceiveFixed => 1.0,
                };
                let scale = notional / 100.0;
                (
                    position_id.clone(),
                    "swap",
                    spec.currency,
                    notional, // swap PV≈0 → bps base is notional
                    scale,
                    sign,
                    wf,
                    None,
                )
            }
        };
        debug_assert_eq!(currency, book.base_currency);

        let pos_pnl_ccy = wf.total_per_100 * scale * sign;
        let mut factors = Vec::with_capacity(wf.factors.len());
        for (idx, (factor, per_100)) in wf.factors.iter().enumerate() {
            let f_ccy = per_100 * scale * sign;
            let bmk = if *factor == PnlFactor::Spread {
                benchmark.clone()
            } else {
                None
            };
            factors.push(FactorPnl {
                factor: *factor,
                pnl_ccy: dec_of(f_ccy, "factor pnl")?,
                pnl_bps: bps(f_ccy, base_ccy),
                benchmark: bmk.clone(),
            });
            *book_factor.entry((idx, bmk)).or_insert(0.0) += f_ccy;
        }

        book_pnl += pos_pnl_ccy;
        book_mv += if kind == "bond" { base_ccy } else { 0.0 };

        positions.push(PositionAttribution {
            position_id,
            kind: kind.to_string(),
            market_value_t0: dec_of(base_ccy, "market_value_t0")?,
            total_pnl_ccy: dec_of(pos_pnl_ccy, "position pnl")?,
            total_pnl_bps: bps(pos_pnl_ccy, base_ccy),
            factors,
            curve: wf.curve,
        });
    }

    // Book factors in PnlFactor::ORDER; spread expanded per benchmark.
    let mut book_factors: Vec<FactorPnl> = Vec::new();
    for (idx, factor) in PnlFactor::ORDER.iter().enumerate() {
        let rows: Vec<(&Option<String>, &f64)> = book_factor
            .iter()
            .filter(|((i, _), _)| *i == idx)
            .map(|((_, b), v)| (b, v))
            .collect();
        for (bmk, v) in rows {
            book_factors.push(FactorPnl {
                factor: *factor,
                pnl_ccy: dec_of(*v, "book factor")?,
                pnl_bps: bps(*v, book_mv),
                benchmark: bmk.clone(),
            });
        }
    }

    Ok(Attribution {
        currency: book.base_currency,
        t0,
        t1,
        book_market_value_t0: dec_of(book_mv, "book mv")?,
        total_pnl_ccy: dec_of(book_pnl, "book pnl")?,
        total_pnl_bps: bps(book_pnl, book_mv),
        factors: book_factors,
        positions,
        provenance: AttributionProvenance {
            curve_t0_id: curve_t0_id.to_string(),
            curve_t1_id: curve_t1_id.to_string(),
            factor_model: FACTOR_MODEL_NAME.to_string(),
            pivot_tenor_years: pivot,
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_core::daycounts::DayCountConvention;
    use convex_core::types::Compounding;
    use convex_curves::{InterpolationMethod, ValueType};

    fn d(y: i32, m: u32, day: u32) -> Date {
        Date::from_ymd(y, m, day).unwrap()
    }

    const PILLARS: &[f64] = &[0.25, 0.5, 1.0, 2.0, 3.0, 5.0, 7.0, 10.0, 20.0, 30.0];

    fn flat_curve(ref_date: Date, rate: f64) -> RateCurve<DiscreteCurve> {
        let dc = DiscreteCurve::new(
            ref_date,
            PILLARS.to_vec(),
            vec![rate; PILLARS.len()],
            ValueType::ZeroRate {
                compounding: Compounding::Continuous,
                day_count: DayCountConvention::Act365Fixed,
            },
            InterpolationMethod::Linear,
        )
        .unwrap();
        RateCurve::new(dc)
    }

    fn shifted_flat(ref_date: Date, rate: f64, bump_bps: f64) -> RateCurve<DiscreteCurve> {
        flat_curve(ref_date, rate + bump_bps * 1e-4)
    }

    fn bond_10y(coupon: f64) -> FixedRateBond {
        FixedRateBond::builder()
            .cusip_unchecked("OAT10Y")
            .coupon_rate(Decimal::from_f64_retain(coupon).unwrap())
            .maturity(d(2036, 5, 7))
            .issue_date(d(2026, 5, 7))
            .frequency(Frequency::Annual)
            .day_count(DayCountConvention::ActActIsda)
            .currency(Currency::EUR)
            .face_value(dec!(100))
            .build()
            .unwrap()
    }

    fn z_mark(bps: f64) -> Mark {
        Mark::Spread {
            value: Spread::new(Decimal::from_f64_retain(bps).unwrap(), SpreadType::ZSpread),
            benchmark: "EUR.GOVT".into(),
        }
    }

    fn book(positions: Vec<ResolvedPosition>) -> ResolvedBook {
        ResolvedBook {
            base_currency: Currency::EUR,
            positions,
        }
    }

    #[test]
    fn zero_move_zero_pnl() {
        // Same curve, same Z-spread mark, t1 == t0 → every factor and the
        // total are exactly zero (the prompt's mandated edge case).
        let c = flat_curve(d(2026, 5, 7), 0.025);
        let b = book(vec![ResolvedPosition::Bond {
            position_id: Some("OAT".into()),
            bond: Box::new(bond_10y(0.027)),
            notional_face: dec!(10_000_000),
            mark_t0: z_mark(20.0),
            mark_t1: z_mark(20.0),
        }]);
        let a = attribute_pnl(
            &b,
            d(2026, 5, 7),
            d(2026, 5, 7),
            &c,
            "c0",
            &c,
            "c0",
            &AttributionConfig::default(),
        )
        .unwrap();
        assert!(a.total_pnl_ccy.abs() < dec!(0.01));
        for f in &a.positions[0].factors {
            assert!(
                f.pnl_ccy.abs() < dec!(0.01),
                "factor {:?} = {}",
                f.factor,
                f.pnl_ccy
            );
        }
    }

    #[test]
    fn identity_closes_sum_of_factors_equals_total() {
        let c0 = flat_curve(d(2026, 5, 7), 0.025);
        let c1 = shifted_flat(d(2026, 5, 8), 0.025, 8.0); // +8bp parallel
        let b = book(vec![
            ResolvedPosition::Bond {
                position_id: Some("OAT".into()),
                bond: Box::new(bond_10y(0.027)),
                notional_face: dec!(10_000_000),
                mark_t0: z_mark(15.0),
                mark_t1: z_mark(18.0),
            },
            ResolvedPosition::Swap {
                position_id: Some("EUR_SWAP".into()),
                spec: InterestRateSwapPnlSpec {
                    trade_date: d(2026, 5, 1),
                    maturity: d(2036, 5, 1),
                    fixed_rate_decimal: 0.026,
                    fixed_frequency: Frequency::Annual,
                    fixed_day_count: DayCountConvention::Thirty360E,
                    side: SwapSide::PayFixed,
                    notional: dec!(10_000_000),
                    currency: Currency::EUR,
                },
            },
        ]);
        let a = attribute_pnl(
            &b,
            d(2026, 5, 7),
            d(2026, 5, 8),
            &c0,
            "c0",
            &c1,
            "c1",
            &AttributionConfig::default(),
        )
        .unwrap();
        for p in &a.positions {
            let sum: Decimal = p.factors.iter().map(|f| f.pnl_ccy).sum();
            assert!(
                (sum - p.total_pnl_ccy).abs() < dec!(0.01),
                "position {:?}: Σfactors {} vs total {}",
                p.position_id,
                sum,
                p.total_pnl_ccy
            );
        }
        let pos_sum: Decimal = a.positions.iter().map(|p| p.total_pnl_ccy).sum();
        assert!((pos_sum - a.total_pnl_ccy).abs() < dec!(0.01));
        let bf_sum: Decimal = a.factors.iter().map(|f| f.pnl_ccy).sum();
        assert!((bf_sum - a.total_pnl_ccy).abs() < dec!(0.01));
    }

    #[test]
    fn parallel_move_lands_in_curve_parallel() {
        // Flat curve +10bp parallel, same date (isolate curve), Z mark
        // unchanged. Curve PnL ≈ −DV01·10bp; it must sit in curve_parallel,
        // not slope/curvature/residual.
        let c0 = flat_curve(d(2026, 5, 7), 0.025);
        let c1 = shifted_flat(d(2026, 5, 7), 0.025, 10.0);
        let b = book(vec![ResolvedPosition::Bond {
            position_id: Some("OAT".into()),
            bond: Box::new(bond_10y(0.027)),
            notional_face: dec!(10_000_000),
            mark_t0: z_mark(0.0),
            mark_t1: z_mark(0.0),
        }]);
        let a = attribute_pnl(
            &b,
            d(2026, 5, 7),
            d(2026, 5, 7),
            &c0,
            "c0",
            &c1,
            "c1",
            &AttributionConfig::default(),
        )
        .unwrap();
        let f = |which: PnlFactor| {
            a.positions[0]
                .factors
                .iter()
                .find(|x| x.factor == which)
                .unwrap()
                .pnl_ccy
                .to_f64()
                .unwrap()
        };
        let par = f(PnlFactor::CurveParallel);
        assert!(par < 0.0, "long bond loses on a +10bp move; got {par}");
        // Analytic: ≈ −MV · ModDur · 10bp. 10Y annual ~2.7% bond, mod dur ~8.7.
        let mv = a.positions[0].market_value_t0.to_f64().unwrap();
        let approx = -mv * 8.7 * 10.0 * 1e-4;
        assert!(
            (par - approx).abs() / approx.abs() < 0.10,
            "curve_parallel {par} vs analytic {approx}"
        );
        assert!(f(PnlFactor::CurveSlope).abs() < par.abs() * 0.02);
        assert!(f(PnlFactor::CurveCurvature).abs() < par.abs() * 0.02);
        assert!(f(PnlFactor::CurveResidual).abs() < par.abs() * 0.02);
        assert!(f(PnlFactor::Spread).abs() < 1.0);
        assert!(f(PnlFactor::Carry).abs() < 1.0);
        assert!(f(PnlFactor::Residual).abs() < par.abs() * 0.02);
    }

    #[test]
    fn pay_fixed_swap_offsets_long_bond_when_rates_rise() {
        // The hero-moment guard: rates rise → long bond loses, pay-fixed
        // swap gains, partially offsetting.
        let c0 = flat_curve(d(2026, 5, 7), 0.025);
        let c1 = shifted_flat(d(2026, 5, 8), 0.025, 15.0); // +15bp
        let b = book(vec![
            ResolvedPosition::Bond {
                position_id: Some("OAT".into()),
                bond: Box::new(bond_10y(0.027)),
                notional_face: dec!(10_000_000),
                mark_t0: z_mark(10.0),
                mark_t1: z_mark(10.0),
            },
            ResolvedPosition::Swap {
                position_id: Some("EUR_SWAP".into()),
                spec: InterestRateSwapPnlSpec {
                    trade_date: d(2026, 5, 1),
                    maturity: d(2036, 5, 1),
                    fixed_rate_decimal: 0.026,
                    fixed_frequency: Frequency::Annual,
                    fixed_day_count: DayCountConvention::Thirty360E,
                    side: SwapSide::PayFixed,
                    notional: dec!(10_000_000),
                    currency: Currency::EUR,
                },
            },
        ]);
        let a = attribute_pnl(
            &b,
            d(2026, 5, 7),
            d(2026, 5, 8),
            &c0,
            "c0",
            &c1,
            "c1",
            &AttributionConfig::default(),
        )
        .unwrap();
        let bond_pnl = a.positions[0].total_pnl_ccy;
        let swap_pnl = a.positions[1].total_pnl_ccy;
        assert!(bond_pnl < dec!(0), "long bond loses on +15bp: {bond_pnl}");
        assert!(
            swap_pnl > dec!(0),
            "pay-fixed swap gains on +15bp: {swap_pnl}"
        );
        // Partial offset: book loss is smaller than the bond's standalone loss.
        assert!(
            a.total_pnl_ccy > bond_pnl,
            "swap must offset: book {} vs bond {}",
            a.total_pnl_ccy,
            bond_pnl
        );
        // Swap is ~pure curve: no spread, negligible carry.
        let swap_spread = a.positions[1]
            .factors
            .iter()
            .find(|f| f.factor == PnlFactor::Spread)
            .unwrap();
        assert!(swap_spread.pnl_ccy.abs() < dec!(0.01));
    }

    #[test]
    fn rejects_currency_mismatch_and_empty_book() {
        let c = flat_curve(d(2026, 5, 7), 0.025);
        let cfg = AttributionConfig::default();
        assert!(matches!(
            attribute_pnl(
                &book(vec![]),
                d(2026, 5, 7),
                d(2026, 5, 8),
                &c,
                "c0",
                &c,
                "c1",
                &cfg
            ),
            Err(AnalyticsError::InvalidInput(_))
        ));
        let usd_bond = FixedRateBond::builder()
            .cusip_unchecked("UST")
            .coupon_rate(dec!(0.04))
            .maturity(d(2036, 5, 7))
            .issue_date(d(2026, 5, 7))
            .frequency(Frequency::SemiAnnual)
            .day_count(DayCountConvention::ActActIsda)
            .currency(Currency::USD)
            .face_value(dec!(100))
            .build()
            .unwrap();
        let b = book(vec![ResolvedPosition::Bond {
            position_id: None,
            bond: Box::new(usd_bond),
            notional_face: dec!(1_000_000),
            mark_t0: z_mark(0.0),
            mark_t1: z_mark(0.0),
        }]);
        assert!(matches!(
            attribute_pnl(&b, d(2026, 5, 7), d(2026, 5, 8), &c, "c0", &c, "c1", &cfg),
            Err(AnalyticsError::InvalidInput(_))
        ));
    }

    // ---- canonical demo fixture (gate sanity + regression guard) ---------

    fn eur_curve(ref_date: Date, base: &[f64]) -> RateCurve<DiscreteCurve> {
        let dc = DiscreteCurve::new(
            ref_date,
            PILLARS.to_vec(),
            base.to_vec(),
            ValueType::ZeroRate {
                compounding: Compounding::Continuous,
                day_count: DayCountConvention::Act365Fixed,
            },
            InterpolationMethod::Linear,
        )
        .unwrap();
        RateCurve::new(dc)
    }

    fn sov(cusip: &str, coupon: f64, mat: Date) -> FixedRateBond {
        FixedRateBond::builder()
            .cusip_unchecked(cusip)
            .coupon_rate(Decimal::from_f64_retain(coupon).unwrap())
            .maturity(mat)
            .issue_date(d(2024, 2, 15))
            .frequency(Frequency::Annual)
            .day_count(DayCountConvention::ActActIsda)
            .currency(Currency::EUR)
            .face_value(dec!(100))
            .build()
            .unwrap()
    }

    fn bmk(bps: f64, b: &str) -> Mark {
        Mark::Spread {
            value: Spread::new(Decimal::from_f64_retain(bps).unwrap(), SpreadType::ZSpread),
            benchmark: b.into(),
        }
    }

    /// The hedge-advisor book + the swap, May 7 → May 8 2026. Rates up ~6bp
    /// with mild steepening; BTP-Bund and OAT-Bund widen a touch.
    #[test]
    fn demo_book_attribution_shape() {
        // EUR govt (Bund) curve t0, then a +6bp parallel / slight steepener.
        let r0: Vec<f64> = PILLARS.iter().map(|t| 0.022 + 0.0010 * t.sqrt()).collect();
        let r1: Vec<f64> = PILLARS
            .iter()
            .zip(&r0)
            .map(|(&t, r)| r + 0.0006 + 0.00004 * (t - 2.0).max(0.0))
            .collect();
        let c0 = eur_curve(d(2026, 5, 7), &r0);
        let c1 = eur_curve(d(2026, 5, 8), &r1);

        let b = book(vec![
            ResolvedPosition::Bond {
                position_id: Some("OAT_2.75_2034".into()),
                bond: Box::new(sov("OAT10Y", 0.0275, d(2034, 5, 25))),
                notional_face: dec!(10_000_000),
                mark_t0: bmk(12.0, "FR.OAT-DE.BUND"),
                mark_t1: bmk(14.0, "FR.OAT-DE.BUND"),
            },
            ResolvedPosition::Bond {
                position_id: Some("BTP_4.0_2035".into()),
                bond: Box::new(sov("BTP10Y", 0.04, d(2035, 2, 1))),
                notional_face: dec!(5_000_000),
                mark_t0: bmk(135.0, "IT.BTP-DE.BUND"),
                mark_t1: bmk(141.0, "IT.BTP-DE.BUND"), // BTP-Bund widens 6bp
            },
            ResolvedPosition::Bond {
                position_id: Some("BUND_2.5_2034".into()),
                bond: Box::new(sov("BUND10Y", 0.025, d(2034, 8, 15))),
                notional_face: dec!(10_000_000),
                mark_t0: bmk(0.0, "DE.BUND"), // Bund ~ the benchmark itself
                mark_t1: bmk(0.0, "DE.BUND"),
            },
            ResolvedPosition::Swap {
                position_id: Some("EUR_SWAP_10Y_PAYFIXED".into()),
                spec: InterestRateSwapPnlSpec {
                    trade_date: d(2026, 5, 1),
                    maturity: d(2036, 5, 1),
                    fixed_rate_decimal: 0.0265,
                    fixed_frequency: Frequency::Annual,
                    fixed_day_count: DayCountConvention::Thirty360E,
                    side: SwapSide::PayFixed,
                    notional: dec!(10_000_000),
                    currency: Currency::EUR,
                },
            },
        ]);

        let a = attribute_pnl(
            &b,
            d(2026, 5, 7),
            d(2026, 5, 8),
            &c0,
            "eur_govt_2026_05_07",
            &c1,
            "eur_govt_2026_05_08",
            &AttributionConfig::default(),
        )
        .unwrap();

        // Structural: 4 positions, full provenance, factor identity closes.
        assert_eq!(a.positions.len(), 4);
        assert_eq!(a.provenance.factor_model, FACTOR_MODEL_NAME);
        let bf_sum: Decimal = a.factors.iter().map(|f| f.pnl_ccy).sum();
        assert!((bf_sum - a.total_pnl_ccy).abs() < dec!(0.05));

        // Economics: long sovereigns lose on +6bp; pay-fixed swap gains and
        // partially offsets (the hero moment).
        let oat = &a.positions[0].total_pnl_ccy;
        let swap = &a.positions[3];
        assert!(*oat < dec!(0), "long OAT loses on +6bp: {oat}");
        assert!(
            swap.total_pnl_ccy > dec!(0),
            "pay-fixed swap gains on +6bp: {}",
            swap.total_pnl_ccy
        );
        assert_eq!(swap.kind, "swap");
        // Swap is ~pure curve: spread factor ~ 0.
        let swap_spread = swap
            .factors
            .iter()
            .find(|f| f.factor == PnlFactor::Spread)
            .unwrap();
        assert!(swap_spread.pnl_ccy.abs() < dec!(0.01));

        if std::env::var("PNL_DUMP").is_ok() {
            println!("{}", serde_json::to_string_pretty(&a).unwrap());
        }
    }
}
