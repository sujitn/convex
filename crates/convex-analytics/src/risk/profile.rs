//! Per-position risk profile. Sign convention lives on the wire types in
//! `risk::hedging::types` and applies here too.

use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use convex_bonds::instruments::CallableBond;
use convex_bonds::traits::{Bond, FixedCouponBond};
use convex_core::types::{Compounding, Currency, Date, Frequency, Mark, SpreadType};
use convex_curves::bumping::KeyRateBump;
use convex_curves::{DiscreteCurve, RateCurve, RateCurveDyn};

use crate::error::{AnalyticsError, AnalyticsResult};
use crate::pricing::{price_callable_from_mark, price_from_mark};
use crate::risk::calculator::BondRiskCalculator;
use crate::risk::duration::STANDARD_KEY_RATE_TENORS;
use crate::risk::hedging::cost::COST_MODEL_NAME;
use crate::spreads::{OASCalculator, ZSpreadCalculator};

/// Default KRD ladder for the hedge advisor. Chosen for liquid sovereign
/// benchmarks: 2Y/5Y/10Y/30Y. Pricing crates that want a deeper ladder can
/// pass their own slice into [`compute_position_risk`].
pub const ADVISOR_KEY_RATE_TENORS: &[f64] = &[2.0, 5.0, 10.0, 30.0];

/// Position-scaled partial DV01 from a +1bp shock at one tenor.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[allow(missing_docs)]
pub struct KeyRateBucket {
    pub tenor_years: f64,
    pub partial_dv01: f64,
}

/// Audit metadata stamped on every advisor output. `oas_volatility` is set
/// only on the callable OAS path (so a snapshot can be reproduced without
/// re-fetching the original request).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[allow(missing_docs)]
pub struct Provenance {
    #[serde(default)]
    pub curves_used: Vec<String>,
    #[serde(default)]
    pub cost_model: String,
    #[serde(default)]
    pub advisor_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oas_volatility: Option<f64>,
}

/// Risk profile of a single position. `notional_face` is signed (long → +).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[allow(missing_docs)]
pub struct RiskProfile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position_id: Option<String>,
    pub currency: Currency,
    pub settlement: Date,
    #[cfg_attr(feature = "schemars", schemars(with = "f64"))]
    pub notional_face: Decimal,
    pub clean_price_per_100: f64,
    pub dirty_price_per_100: f64,
    pub accrued_per_100: f64,
    #[cfg_attr(feature = "schemars", schemars(with = "f64"))]
    pub market_value: Decimal,
    pub ytm_decimal: f64,
    pub modified_duration_years: f64,
    pub macaulay_duration_years: f64,
    pub convexity: f64,
    pub dv01: f64,
    #[serde(default)]
    pub key_rate_buckets: Vec<KeyRateBucket>,
    #[serde(default)]
    pub provenance: Provenance,
}

/// Per-position risk: price the bond, derive analytical metrics via
/// [`BondRiskCalculator`], then bucket DV01 with ±1bp triangular bumps at
/// each `key_rate_tenor` holding the implied Z-spread fixed.
#[allow(clippy::too_many_arguments)]
pub fn compute_position_risk<B>(
    bond: &B,
    settlement: Date,
    mark: &Mark,
    notional_face: Decimal,
    discount_curve: &RateCurve<DiscreteCurve>,
    discount_curve_id: &str,
    quote_frequency: Option<Frequency>,
    key_rate_tenors: Option<&[f64]>,
    position_id: Option<String>,
) -> AnalyticsResult<RiskProfile>
where
    B: Bond + FixedCouponBond,
{
    let freq = quote_frequency.unwrap_or_else(|| bond.frequency());
    let priced = price_from_mark(bond, settlement, mark, Some(discount_curve), freq)?;

    // Position scaling: face/100 multiplies per-100 quantities into currency.
    let face_f64 = notional_face
        .to_f64()
        .ok_or_else(|| AnalyticsError::InvalidInput("notional_face: non-finite".into()))?;
    let face_scale = face_f64 / 100.0;

    let market_value = notional_face
        * Decimal::from_f64_retain(priced.dirty_price_per_100).ok_or_else(|| {
            AnalyticsError::InvalidInput("dirty price not representable as Decimal".into())
        })?
        / Decimal::from(100);

    let calc = BondRiskCalculator::from_bond(
        bond,
        settlement,
        priced.dirty_price_per_100,
        priced.ytm_decimal,
        Compounding::from(freq),
    )?;
    let metrics = calc.all_metrics()?;
    let dv01 = metrics.dv01_per_100.as_f64() * face_scale;

    // Implied Z-spread, held fixed during the KRD curve bumps.
    let dirty_dec = Decimal::from_f64_retain(priced.dirty_price_per_100).ok_or_else(|| {
        AnalyticsError::InvalidInput("dirty price not representable as Decimal".into())
    })?;
    let z = ZSpreadCalculator::new(discount_curve).calculate(bond, dirty_dec, settlement)?;
    let z_decimal = z
        .as_decimal()
        .to_f64()
        .ok_or_else(|| AnalyticsError::InvalidInput("z-spread not finite".into()))?;

    let tenors = key_rate_tenors.unwrap_or(STANDARD_KEY_RATE_TENORS);
    let base_inner = discount_curve.inner().clone();
    let bump_bps = 1.0_f64;
    let mut buckets = Vec::with_capacity(tenors.len());
    for &tenor in tenors {
        let up = RateCurve::new(KeyRateBump::new(tenor, bump_bps).apply(&base_inner));
        let dn = RateCurve::new(KeyRateBump::new(tenor, -bump_bps).apply(&base_inner));
        let dirty_up = ZSpreadCalculator::new(&up).price_with_spread(bond, z_decimal, settlement);
        let dirty_dn = ZSpreadCalculator::new(&dn).price_with_spread(bond, z_decimal, settlement);
        // Per-100 partial DV01 = (dirty_dn − dirty_up) / 2 for ±1bp.
        let partial_per_100 = (dirty_dn - dirty_up) * 0.5;
        buckets.push(KeyRateBucket {
            tenor_years: tenor,
            partial_dv01: partial_per_100 * face_scale,
        });
    }

    Ok(RiskProfile {
        position_id,
        currency: bond.currency(),
        settlement,
        notional_face,
        clean_price_per_100: priced.clean_price_per_100,
        dirty_price_per_100: priced.dirty_price_per_100,
        accrued_per_100: priced.accrued_per_100,
        market_value,
        ytm_decimal: priced.ytm_decimal,
        modified_duration_years: metrics.modified_duration.as_f64(),
        macaulay_duration_years: metrics.macaulay_duration.as_f64(),
        convexity: metrics.convexity.as_f64(),
        dv01,
        key_rate_buckets: buckets,
        provenance: Provenance {
            curves_used: vec![discount_curve_id.to_string()],
            cost_model: COST_MODEL_NAME.to_string(),
            advisor_version: env!("CARGO_PKG_VERSION").to_string(),
            oas_volatility: None,
        },
    })
}

/// Sum a per-position [`RiskProfile`] slice into one book-level profile.
///
/// All positions must share `currency` and `settlement` (which is the
/// evaluation date, not trade settlement — see [`compute_position_risk`]).
/// Mismatches reject; preprocess to a common eval date if your desk has
/// mixed T+1 / T+2 settlements.
///
/// Duration, Macaulay duration, and convexity are **DV01-weighted with
/// signed weights** — `D = Σ(D_i · DV01_i) / Σ(DV01_i)`. This is the
/// duration of the net DV01 exposure, which is what the futures-contract
/// picker downstream actually wants to hedge. For a long-2Y/short-30Y book
/// where the 30Y short dominates DV01, the reported duration tracks the
/// long end, not an MV-weighted midpoint. When `Σ DV01 ≈ 0` (DV01-neutral
/// book) all three collapse to `0.0` — the trader is already flat and the
/// contract picker is undefined here; treat as informational only.
///
/// `ytm_decimal` is `0.0`. Yields don't compound linearly under any
/// weighting; "weighted average yield" is a Bloomberg-PORT informational
/// field, not a tradable number. We zero it rather than emit a misuse
/// hazard; compute portfolio IRR externally if you need one. (NaN would
/// be more honest but JSON can't round-trip it.)
///
/// Per-100 prices (`clean/dirty/accrued_per_100`) are face-weighted by
/// `|notional_face|`. For a long-only book this is the volume-weighted
/// price; for a long-short book it's a magnitude-weighted average. Both
/// fall back to `0.0` when all faces are zero.
///
/// `provenance.cost_model` is `"mixed"` if positions disagree (audit
/// honesty over single-source clarity); `oas_volatility` is dropped on
/// every aggregation (ambiguous on a mixed-vol book).
pub fn aggregate_risk_profiles(
    profiles: &[RiskProfile],
    book_id: Option<String>,
) -> AnalyticsResult<RiskProfile> {
    let first = profiles.first().ok_or_else(|| {
        AnalyticsError::InvalidInput("aggregate_risk_profiles: positions list is empty".into())
    })?;

    let currency = first.currency;
    let settlement = first.settlement;
    for (i, p) in profiles.iter().enumerate().skip(1) {
        if p.currency != currency {
            return Err(AnalyticsError::InvalidInput(format!(
                "aggregate_risk_profiles: currency mismatch at position {i}: {:?} vs {:?}",
                p.currency, currency
            )));
        }
        if p.settlement != settlement {
            return Err(AnalyticsError::InvalidInput(format!(
                "aggregate_risk_profiles: evaluation date mismatch at position {i}: {} vs {}",
                p.settlement, settlement
            )));
        }
    }

    let mut notional_face = Decimal::ZERO;
    let mut market_value = Decimal::ZERO;
    let mut dv01 = 0.0_f64;
    // DV01-weighted numerators (signed): for hedging policy we want the
    // duration of the net DV01 exposure, not an MV-weighted average.
    let mut mod_dur_num = 0.0_f64;
    let mut mac_dur_num = 0.0_f64;
    let mut convexity_num = 0.0_f64;
    // Face-weighted numerators for per-100 prices, weighted by |face| so
    // long-short books still produce a defined number.
    let mut clean_num = 0.0_f64;
    let mut dirty_num = 0.0_f64;
    let mut accrued_num = 0.0_f64;
    let mut abs_face_total = 0.0_f64;
    let mut buckets: Vec<KeyRateBucket> = Vec::new();
    let mut curves_used: Vec<String> = Vec::new();
    let mut cost_model_mixed = false;

    for p in profiles {
        notional_face += p.notional_face;
        market_value += p.market_value;
        dv01 += p.dv01;

        mod_dur_num += p.modified_duration_years * p.dv01;
        mac_dur_num += p.macaulay_duration_years * p.dv01;
        convexity_num += p.convexity * p.dv01;

        let abs_face = p.notional_face.to_f64().unwrap_or(0.0).abs();
        abs_face_total += abs_face;
        clean_num += p.clean_price_per_100 * abs_face;
        dirty_num += p.dirty_price_per_100 * abs_face;
        accrued_num += p.accrued_per_100 * abs_face;

        merge_key_rate_buckets(&mut buckets, &p.key_rate_buckets);

        for curve in &p.provenance.curves_used {
            if !curves_used.iter().any(|c| c == curve) {
                curves_used.push(curve.clone());
            }
        }
        if p.provenance.cost_model != first.provenance.cost_model {
            cost_model_mixed = true;
        }
    }
    buckets.sort_by(|a, b| {
        a.tenor_years
            .partial_cmp(&b.tenor_years)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Zero on near-flat-DV01 books; JSON can't round-trip NaN, and the
    // contract picker downstream isn't meaningful at the limit anyway.
    let dur_book = if dv01.abs() > 1e-12 {
        mod_dur_num / dv01
    } else {
        0.0
    };
    let mac_book = if dv01.abs() > 1e-12 {
        mac_dur_num / dv01
    } else {
        0.0
    };
    let conv_book = if dv01.abs() > 1e-12 {
        convexity_num / dv01
    } else {
        0.0
    };
    let face_weight = if abs_face_total > 1e-12 {
        abs_face_total
    } else {
        1.0
    };

    Ok(RiskProfile {
        position_id: book_id,
        currency,
        settlement,
        notional_face,
        clean_price_per_100: clean_num / face_weight,
        dirty_price_per_100: dirty_num / face_weight,
        accrued_per_100: accrued_num / face_weight,
        market_value,
        ytm_decimal: 0.0,
        modified_duration_years: dur_book,
        macaulay_duration_years: mac_book,
        convexity: conv_book,
        dv01,
        key_rate_buckets: buckets,
        provenance: Provenance {
            curves_used,
            cost_model: if cost_model_mixed {
                "mixed".into()
            } else {
                first.provenance.cost_model.clone()
            },
            advisor_version: first.provenance.advisor_version.clone(),
            oas_volatility: None,
        },
    })
}

/// Sum partial DV01s into a running bucket vector, matching tenors within
/// 1e-9. Shared with `risk::hedging::types::residual_from`.
pub(crate) fn merge_key_rate_buckets(into: &mut Vec<KeyRateBucket>, from: &[KeyRateBucket]) {
    for tb in from {
        if let Some(existing) = into
            .iter_mut()
            .find(|b| (b.tenor_years - tb.tenor_years).abs() < 1e-9)
        {
            existing.partial_dv01 += tb.partial_dv01;
        } else {
            into.push(*tb);
        }
    }
}

/// Per-position risk for a [`CallableBond`] under an OAS spread mark.
///
/// Routes the price through [`price_callable_from_mark`] (HW1F trinomial),
/// then derives **effective** duration and convexity by reshocking the
/// discount curve ±1bp at constant OAS via the same HW1F pricer. KRD: bump
/// the curve at each tenor (triangular weight) and reprice — again, OAS
/// held constant. The reported `dv01` is the position-scaled effective
/// DV01, not the bullet-cashflow YTM DV01.
///
/// `volatility_decimal` is the annual normal vol (decimal, `0.01` = 1%) and
/// is required. Non-OAS marks forward to the generic [`compute_position_risk`]
/// against the callable's bullet cashflows, with `volatility_decimal`
/// ignored — useful when callers want a uniform entry point regardless of
/// mark type.
///
/// Conventions on the returned profile:
/// - `modified_duration_years` carries the **effective** duration. For an
///   ITM callable this is shorter than the bullet modified duration because
///   the call truncates upside.
/// - `convexity` is **effective** convexity and may be **negative** when
///   the call is in-the-money — the textbook signature of a callable.
/// - `macaulay_duration_years` is the **bullet** YTM-equivalent Macaulay
///   against the underlying cashflows (i.e. duration if the bond ran to
///   maturity with no call). Strict Macaulay isn't defined for callables;
///   this is the "bullet reference" number, not an option-adjusted figure.
#[allow(clippy::too_many_arguments)]
pub fn compute_callable_position_risk(
    bond: &CallableBond,
    settlement: Date,
    mark: &Mark,
    notional_face: Decimal,
    discount_curve: &RateCurve<DiscreteCurve>,
    discount_curve_id: &str,
    quote_frequency: Option<Frequency>,
    key_rate_tenors: Option<&[f64]>,
    position_id: Option<String>,
    volatility_decimal: f64,
) -> AnalyticsResult<RiskProfile> {
    // Non-OAS marks: forward to the bullet path. Vol ignored on this branch.
    let oas_bps_input = match mark {
        Mark::Spread { value, .. } if value.spread_type() == SpreadType::OAS => value
            .as_bps()
            .to_f64()
            .ok_or_else(|| AnalyticsError::InvalidInput("OAS bps not finite".into()))?,
        _ => {
            return compute_position_risk(
                bond,
                settlement,
                mark,
                notional_face,
                discount_curve,
                discount_curve_id,
                quote_frequency,
                key_rate_tenors,
                position_id,
            )
        }
    };

    if !volatility_decimal.is_finite() || volatility_decimal <= 0.0 {
        return Err(AnalyticsError::InvalidInput(format!(
            "OAS volatility must be finite and strictly positive (got {volatility_decimal})"
        )));
    }

    let freq = quote_frequency.unwrap_or_else(|| bond.frequency());
    let priced = price_callable_from_mark(
        bond,
        settlement,
        mark,
        Some(discount_curve),
        freq,
        Some(volatility_decimal),
    )?;

    let face_f64 = notional_face
        .to_f64()
        .ok_or_else(|| AnalyticsError::InvalidInput("notional_face: non-finite".into()))?;
    let face_scale = face_f64 / 100.0;

    let market_value = notional_face
        * Decimal::from_f64_retain(priced.dirty_price_per_100).ok_or_else(|| {
            AnalyticsError::InvalidInput("dirty price not representable as Decimal".into())
        })?
        / Decimal::from(100);

    let oas_decimal = oas_bps_input / 10_000.0;
    let calculator = OASCalculator::default_hull_white(volatility_decimal);

    // Effective duration / convexity at constant OAS via parallel curve shifts.
    let eff_duration =
        calculator.effective_duration(bond, discount_curve, oas_decimal, settlement)?;
    let eff_convexity =
        calculator.effective_convexity(bond, discount_curve, oas_decimal, settlement)?;

    // Bullet (YTM-equivalent) Macaulay against the underlying cashflows. Not
    // strictly meaningful for instruments with embedded options, but a useful
    // reference number — what the duration would be if the bond ran to
    // maturity with no call exercise.
    let bullet_calc = BondRiskCalculator::from_bond(
        bond,
        settlement,
        priced.dirty_price_per_100,
        priced.ytm_decimal,
        Compounding::from(freq),
    )?;
    let bullet_macaulay = bullet_calc.all_metrics()?.macaulay_duration.as_f64();

    // DV01_per_100 = effective_duration * dirty_per_100 * 1bp.
    let dv01_per_100 = eff_duration * priced.dirty_price_per_100 * 1.0e-4;
    let dv01 = dv01_per_100 * face_scale;

    // KRD: bump curve at each tenor, reprice via HW1F at the same OAS.
    let tenors = key_rate_tenors.unwrap_or(STANDARD_KEY_RATE_TENORS);
    let base_inner = discount_curve.inner().clone();
    let bump_bps = 1.0_f64;
    let mut buckets = Vec::with_capacity(tenors.len());
    for &tenor in tenors {
        let up = RateCurve::new(KeyRateBump::new(tenor, bump_bps).apply(&base_inner));
        let dn = RateCurve::new(KeyRateBump::new(tenor, -bump_bps).apply(&base_inner));
        let dirty_up =
            calculator.price_with_oas(bond, &up as &dyn RateCurveDyn, oas_decimal, settlement)?;
        let dirty_dn =
            calculator.price_with_oas(bond, &dn as &dyn RateCurveDyn, oas_decimal, settlement)?;
        let partial_per_100 = (dirty_dn - dirty_up) * 0.5;
        buckets.push(KeyRateBucket {
            tenor_years: tenor,
            partial_dv01: partial_per_100 * face_scale,
        });
    }

    Ok(RiskProfile {
        position_id,
        currency: bond.currency(),
        settlement,
        notional_face,
        clean_price_per_100: priced.clean_price_per_100,
        dirty_price_per_100: priced.dirty_price_per_100,
        accrued_per_100: priced.accrued_per_100,
        market_value,
        ytm_decimal: priced.ytm_decimal,
        modified_duration_years: eff_duration,
        macaulay_duration_years: bullet_macaulay,
        convexity: eff_convexity,
        dv01,
        key_rate_buckets: buckets,
        provenance: Provenance {
            curves_used: vec![discount_curve_id.to_string()],
            cost_model: COST_MODEL_NAME.to_string(),
            advisor_version: env!("CARGO_PKG_VERSION").to_string(),
            oas_volatility: Some(volatility_decimal),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use convex_bonds::instruments::FixedRateBond;
    use convex_core::daycounts::DayCountConvention;
    use convex_curves::{InterpolationMethod, ValueType};
    use rust_decimal_macros::dec;

    fn d(y: i32, m: u32, day: u32) -> Date {
        Date::from_ymd(y, m, day).unwrap()
    }

    fn bond_5pct_10y() -> FixedRateBond {
        FixedRateBond::builder()
            .cusip_unchecked("TEST10Y5")
            .coupon_rate(dec!(0.05))
            .maturity(d(2035, 1, 15))
            .issue_date(d(2025, 1, 15))
            .frequency(Frequency::SemiAnnual)
            .day_count(DayCountConvention::Thirty360US)
            .currency(Currency::USD)
            .face_value(dec!(100))
            .build()
            .unwrap()
    }

    fn flat_curve(rate: f64) -> RateCurve<DiscreteCurve> {
        let dc = DiscreteCurve::new(
            d(2025, 1, 15),
            vec![0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 30.0],
            vec![rate; 8],
            ValueType::ZeroRate {
                compounding: Compounding::Continuous,
                day_count: DayCountConvention::Act365Fixed,
            },
            InterpolationMethod::Linear,
        )
        .unwrap();
        RateCurve::new(dc)
    }

    fn sample() -> RiskProfile {
        RiskProfile {
            position_id: Some("P1".into()),
            currency: Currency::USD,
            settlement: Date::from_ymd(2026, 5, 4).unwrap(),
            notional_face: dec!(1000000),
            clean_price_per_100: 99.0,
            dirty_price_per_100: 100.0,
            accrued_per_100: 1.0,
            market_value: dec!(1000000),
            ytm_decimal: 0.05,
            modified_duration_years: 5.0,
            macaulay_duration_years: 5.13,
            convexity: 30.0,
            dv01: 500.0,
            key_rate_buckets: vec![
                KeyRateBucket {
                    tenor_years: 2.0,
                    partial_dv01: 50.0,
                },
                KeyRateBucket {
                    tenor_years: 5.0,
                    partial_dv01: 450.0,
                },
            ],
            provenance: Provenance {
                curves_used: vec!["sofr".into()],
                cost_model: "heuristic_v1".into(),
                advisor_version: env!("CARGO_PKG_VERSION").into(),
                oas_volatility: None,
            },
        }
    }

    #[test]
    fn round_trips_via_json() {
        let p = sample();
        let parsed: RiskProfile =
            serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(p, parsed);
    }

    #[test]
    fn buckets_sum_close_to_parallel_dv01() {
        let p = sample();
        let bucket_sum: f64 = p.key_rate_buckets.iter().map(|b| b.partial_dv01).sum();
        assert!((bucket_sum - p.dv01).abs() < 1e-6);
    }

    #[cfg(feature = "schemars")]
    #[test]
    fn json_schema_is_derived() {
        let s = serde_json::to_string(&schemars::schema_for!(RiskProfile)).unwrap();
        assert!(s.contains("key_rate_buckets") && s.contains("provenance"));
    }

    // -- compute_position_risk -------------------------------------------------

    #[test]
    fn long_bond_priced_at_yield_has_positive_dv01() {
        let bond = bond_5pct_10y();
        let curve = flat_curve(0.05);
        let mark = Mark::Yield {
            value: dec!(0.05),
            frequency: Frequency::SemiAnnual,
        };

        let profile = compute_position_risk(
            &bond,
            d(2026, 1, 15),
            &mark,
            dec!(10_000_000),
            &curve,
            "test_flat_5pct",
            None,
            None,
            Some("P1".into()),
        )
        .unwrap();

        assert_eq!(profile.currency, Currency::USD);
        assert_eq!(profile.position_id.as_deref(), Some("P1"));
        assert!(profile.dv01 > 0.0, "long bond DV01 should be positive");
        // Marked at coupon -> clean price ~100.
        assert_relative_eq!(profile.clean_price_per_100, 100.0, epsilon = 0.01);
        // 9Y bullet @5% SA: modified duration ~7.1 years (well below maturity).
        assert!(profile.modified_duration_years > 6.5);
        assert!(profile.modified_duration_years < 7.5);
    }

    #[test]
    fn short_position_flips_sign_of_dv01() {
        let bond = bond_5pct_10y();
        let curve = flat_curve(0.05);
        let mark = Mark::Yield {
            value: dec!(0.05),
            frequency: Frequency::SemiAnnual,
        };
        let long = compute_position_risk(
            &bond,
            d(2026, 1, 15),
            &mark,
            dec!(10_000_000),
            &curve,
            "c",
            None,
            None,
            None,
        )
        .unwrap();
        let short = compute_position_risk(
            &bond,
            d(2026, 1, 15),
            &mark,
            dec!(-10_000_000),
            &curve,
            "c",
            None,
            None,
            None,
        )
        .unwrap();
        assert_relative_eq!(long.dv01, -short.dv01, epsilon = 1e-6);
        for (l, s) in long
            .key_rate_buckets
            .iter()
            .zip(short.key_rate_buckets.iter())
        {
            assert_eq!(l.tenor_years, s.tenor_years);
            assert_relative_eq!(l.partial_dv01, -s.partial_dv01, epsilon = 1e-6);
        }
    }

    #[test]
    fn dv01_scales_linearly_with_notional() {
        let bond = bond_5pct_10y();
        let curve = flat_curve(0.05);
        let mark = Mark::Yield {
            value: dec!(0.05),
            frequency: Frequency::SemiAnnual,
        };
        let p1 = compute_position_risk(
            &bond,
            d(2026, 1, 15),
            &mark,
            dec!(1_000_000),
            &curve,
            "c",
            None,
            None,
            None,
        )
        .unwrap();
        let p10 = compute_position_risk(
            &bond,
            d(2026, 1, 15),
            &mark,
            dec!(10_000_000),
            &curve,
            "c",
            None,
            None,
            None,
        )
        .unwrap();
        assert_relative_eq!(p10.dv01, p1.dv01 * 10.0, epsilon = 1e-6);
    }

    #[test]
    fn key_rate_buckets_sum_close_to_parallel_dv01() {
        let bond = bond_5pct_10y();
        let curve = flat_curve(0.05);
        let mark = Mark::Yield {
            value: dec!(0.05),
            frequency: Frequency::SemiAnnual,
        };
        // Use the 4-tenor advisor ladder (ends padded).
        let tenors = [2.0, 5.0, 10.0, 30.0];
        let profile = compute_position_risk(
            &bond,
            d(2026, 1, 15),
            &mark,
            dec!(1_000_000),
            &curve,
            "c",
            None,
            Some(&tenors),
            None,
        )
        .unwrap();

        let bucket_sum: f64 = profile
            .key_rate_buckets
            .iter()
            .map(|b| b.partial_dv01)
            .sum();
        // Triangular bumps with 4 tenors don't cover the full ladder, so the sum
        // is approximate. For a 9Y bullet with most weight at 10Y, the residual
        // gap to parallel DV01 should be a few percent.
        let ratio = bucket_sum / profile.dv01;
        assert!(
            (0.85..=1.15).contains(&ratio),
            "bucket_sum/dv01 = {ratio}, expected ~1"
        );
    }

    #[test]
    fn price_mark_and_yield_mark_agree_on_dv01() {
        let bond = bond_5pct_10y();
        let curve = flat_curve(0.05);
        let yield_mark = Mark::Yield {
            value: dec!(0.05),
            frequency: Frequency::SemiAnnual,
        };
        // Price the bond at yield, then re-mark via dirty price; metrics should match.
        let p_y = compute_position_risk(
            &bond,
            d(2026, 1, 15),
            &yield_mark,
            dec!(1_000_000),
            &curve,
            "c",
            None,
            None,
            None,
        )
        .unwrap();
        let price_mark = Mark::Price {
            value: Decimal::from_f64_retain(p_y.clean_price_per_100).unwrap(),
            kind: convex_core::types::PriceKind::Clean,
        };
        let p_p = compute_position_risk(
            &bond,
            d(2026, 1, 15),
            &price_mark,
            dec!(1_000_000),
            &curve,
            "c",
            None,
            None,
            None,
        )
        .unwrap();
        assert_relative_eq!(p_y.dv01, p_p.dv01, epsilon = 1e-3);
        assert_relative_eq!(
            p_y.modified_duration_years,
            p_p.modified_duration_years,
            epsilon = 1e-6
        );
    }

    #[test]
    fn provenance_carries_curve_and_cost_model() {
        let bond = bond_5pct_10y();
        let curve = flat_curve(0.05);
        let mark = Mark::Yield {
            value: dec!(0.05),
            frequency: Frequency::SemiAnnual,
        };
        let profile = compute_position_risk(
            &bond,
            d(2026, 1, 15),
            &mark,
            dec!(1_000_000),
            &curve,
            "usd_sofr",
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(profile.provenance.curves_used, vec!["usd_sofr"]);
        assert_eq!(profile.provenance.cost_model, "heuristic_v1");
        assert_eq!(
            profile.provenance.advisor_version,
            env!("CARGO_PKG_VERSION")
        );
    }

    // ---- compute_callable_position_risk ----------------------------------

    fn callable_5pct_5y() -> CallableBond {
        use convex_bonds::types::{CallEntry, CallSchedule, CallType};
        let base = FixedRateBond::builder()
            .cusip_unchecked("CALL5Y5")
            .coupon_rate(dec!(0.05))
            .maturity(d(2030, 1, 15))
            .issue_date(d(2025, 1, 15))
            .frequency(Frequency::SemiAnnual)
            .day_count(DayCountConvention::Thirty360US)
            .currency(Currency::USD)
            .face_value(dec!(100))
            .build()
            .unwrap();
        let schedule = CallSchedule::new(CallType::American)
            .with_entry(CallEntry::new(d(2027, 1, 15), 102.0))
            .with_entry(CallEntry::new(d(2028, 1, 15), 101.0))
            .with_entry(CallEntry::new(d(2029, 1, 15), 100.0));
        CallableBond::new(base, schedule)
    }

    fn oas_mark(bps: f64) -> Mark {
        use convex_core::types::Spread;
        Mark::Spread {
            value: Spread::new(Decimal::from_f64_retain(bps).unwrap(), SpreadType::OAS),
            benchmark: "USD.SOFR".into(),
        }
    }

    #[test]
    fn callable_risk_long_position_has_positive_dv01() {
        let bond = callable_5pct_5y();
        let curve = flat_curve(0.04);
        let profile = compute_callable_position_risk(
            &bond,
            d(2025, 4, 15),
            &oas_mark(50.0),
            dec!(10_000_000),
            &curve,
            "usd_sofr",
            None,
            None,
            Some("CALL_P1".into()),
            0.01,
        )
        .unwrap();
        assert_eq!(profile.currency, Currency::USD);
        assert_eq!(profile.position_id.as_deref(), Some("CALL_P1"));
        assert!(profile.dv01 > 0.0, "long callable DV01 should be positive");
        assert!(profile.modified_duration_years > 0.0);
        // Audit trail: the OAS vol used flows through Provenance so the
        // snapshot can be reproduced without re-fetching the request.
        assert_eq!(profile.provenance.oas_volatility, Some(0.01));
    }

    #[test]
    fn callable_effective_duration_shorter_when_itm_than_otm() {
        // Textbook signature: a callable's effective duration shortens as the
        // call moves into the money (rates fall well below coupon). Compare
        // the same bond under low-rate (ITM, call likely) vs high-rate (OTM,
        // bond runs to maturity) scenarios — effective duration must shrink
        // in the ITM regime.
        let bond = callable_5pct_5y();
        let mark = oas_mark(50.0);

        let itm = compute_callable_position_risk(
            &bond,
            d(2025, 4, 15),
            &mark,
            dec!(1_000_000),
            &flat_curve(0.02), // 5% coupon vs 2% rate => deeply ITM
            "c",
            None,
            None,
            None,
            0.01,
        )
        .unwrap();
        let otm = compute_callable_position_risk(
            &bond,
            d(2025, 4, 15),
            &mark,
            dec!(1_000_000),
            &flat_curve(0.08), // 5% coupon vs 8% rate => deeply OTM
            "c",
            None,
            None,
            None,
            0.01,
        )
        .unwrap();

        assert!(
            itm.modified_duration_years < otm.modified_duration_years,
            "ITM effective duration {} should be < OTM effective duration {}",
            itm.modified_duration_years,
            otm.modified_duration_years,
        );
    }

    #[test]
    fn callable_effective_dv01_scales_linearly_with_notional() {
        let bond = callable_5pct_5y();
        let curve = flat_curve(0.04);
        let mark = oas_mark(50.0);
        let p1 = compute_callable_position_risk(
            &bond,
            d(2025, 4, 15),
            &mark,
            dec!(1_000_000),
            &curve,
            "c",
            None,
            None,
            None,
            0.01,
        )
        .unwrap();
        let p10 = compute_callable_position_risk(
            &bond,
            d(2025, 4, 15),
            &mark,
            dec!(10_000_000),
            &curve,
            "c",
            None,
            None,
            None,
            0.01,
        )
        .unwrap();
        assert_relative_eq!(p10.dv01, p1.dv01 * 10.0, epsilon = 1e-3);
    }

    #[test]
    fn callable_short_position_flips_dv01_sign() {
        let bond = callable_5pct_5y();
        let curve = flat_curve(0.04);
        let mark = oas_mark(50.0);
        let long = compute_callable_position_risk(
            &bond,
            d(2025, 4, 15),
            &mark,
            dec!(10_000_000),
            &curve,
            "c",
            None,
            None,
            None,
            0.01,
        )
        .unwrap();
        let short = compute_callable_position_risk(
            &bond,
            d(2025, 4, 15),
            &mark,
            dec!(-10_000_000),
            &curve,
            "c",
            None,
            None,
            None,
            0.01,
        )
        .unwrap();
        assert_relative_eq!(long.dv01, -short.dv01, epsilon = 1e-3);
    }

    #[test]
    fn callable_key_rate_buckets_sum_close_to_effective_dv01() {
        // Use the dense STANDARD_KEY_RATE_TENORS (0.25/0.5/1/2/3/5/7/10/20/30Y);
        // sparse ladders like [2,5,10,30] don't span a 5Y bond's life and
        // would leave a large gap. With dense triangular weights, the bucket
        // sum should approach the effective DV01 within a few percent.
        let bond = callable_5pct_5y();
        let curve = flat_curve(0.04);
        let mark = oas_mark(50.0);
        let profile = compute_callable_position_risk(
            &bond,
            d(2025, 4, 15),
            &mark,
            dec!(1_000_000),
            &curve,
            "c",
            None,
            None, // dense default ladder
            None,
            0.01,
        )
        .unwrap();
        let sum: f64 = profile
            .key_rate_buckets
            .iter()
            .map(|b| b.partial_dv01)
            .sum();
        let ratio = sum / profile.dv01;
        assert!(
            (0.85..=1.15).contains(&ratio),
            "bucket_sum/dv01 = {ratio} (sum={sum}, dv01={})",
            profile.dv01,
        );
    }

    #[test]
    fn callable_risk_non_oas_mark_forwards_to_bullet_path() {
        // A yield mark on a callable should produce the same answer as
        // compute_position_risk on the same callable — function is just a
        // forwarder for the non-OAS case (vol ignored).
        let bond = callable_5pct_5y();
        let curve = flat_curve(0.04);
        let yield_mark = Mark::Yield {
            value: dec!(0.05),
            frequency: Frequency::SemiAnnual,
        };
        let via_callable = compute_callable_position_risk(
            &bond,
            d(2025, 4, 15),
            &yield_mark,
            dec!(1_000_000),
            &curve,
            "c",
            None,
            None,
            None,
            0.01,
        )
        .unwrap();
        let via_generic = compute_position_risk(
            &bond,
            d(2025, 4, 15),
            &yield_mark,
            dec!(1_000_000),
            &curve,
            "c",
            None,
            None,
            None,
        )
        .unwrap();
        assert_relative_eq!(via_callable.dv01, via_generic.dv01, epsilon = 1e-9);
        assert_relative_eq!(
            via_callable.modified_duration_years,
            via_generic.modified_duration_years,
            epsilon = 1e-9
        );
    }

    #[test]
    fn callable_risk_rejects_non_positive_volatility() {
        let bond = callable_5pct_5y();
        let curve = flat_curve(0.04);
        let mark = oas_mark(50.0);
        for bad in [0.0, -0.01, f64::NAN, f64::INFINITY] {
            let err = compute_callable_position_risk(
                &bond,
                d(2025, 4, 15),
                &mark,
                dec!(1_000_000),
                &curve,
                "c",
                None,
                None,
                None,
                bad,
            );
            assert!(
                matches!(err, Err(AnalyticsError::InvalidInput(_))),
                "vol={bad} should be rejected, got {err:?}"
            );
        }
    }

    // ---- aggregate_risk_profiles -----------------------------------------

    fn sample_with(
        position_id: &str,
        notional: Decimal,
        market_value: Decimal,
        dv01: f64,
        modified_duration_years: f64,
        krd: Vec<(f64, f64)>,
        curve_id: &str,
    ) -> RiskProfile {
        let mut p = sample();
        p.position_id = Some(position_id.into());
        p.notional_face = notional;
        p.market_value = market_value;
        p.dv01 = dv01;
        p.modified_duration_years = modified_duration_years;
        p.key_rate_buckets = krd
            .into_iter()
            .map(|(t, d)| KeyRateBucket {
                tenor_years: t,
                partial_dv01: d,
            })
            .collect();
        p.provenance.curves_used = vec![curve_id.into()];
        p
    }

    #[test]
    fn aggregate_sums_dv01_and_unions_krd() {
        let a = sample_with(
            "A",
            dec!(10_000_000),
            dec!(10_000_000),
            7000.0,
            7.0,
            vec![(5.0, 2000.0), (10.0, 5000.0)],
            "sofr",
        );
        let b = sample_with(
            "B",
            dec!(5_000_000),
            dec!(5_100_000),
            3500.0,
            3.0,
            vec![(2.0, 500.0), (5.0, 3000.0)],
            "tsy",
        );
        let book = aggregate_risk_profiles(&[a, b], Some("BOOK".into())).unwrap();

        assert_eq!(book.position_id.as_deref(), Some("BOOK"));
        assert_eq!(book.notional_face, dec!(15_000_000));
        assert_eq!(book.market_value, dec!(15_100_000));
        assert_relative_eq!(book.dv01, 10500.0, epsilon = 1e-9);

        // KRD union: 2Y from B, 5Y is summed, 10Y from A.
        let by_tenor: std::collections::HashMap<i64, f64> = book
            .key_rate_buckets
            .iter()
            .map(|b| ((b.tenor_years * 10.0) as i64, b.partial_dv01))
            .collect();
        assert_relative_eq!(by_tenor[&20], 500.0, epsilon = 1e-9);
        assert_relative_eq!(by_tenor[&50], 5000.0, epsilon = 1e-9);
        assert_relative_eq!(by_tenor[&100], 5000.0, epsilon = 1e-9);

        // Buckets are sorted ascending by tenor.
        let tenors: Vec<f64> = book
            .key_rate_buckets
            .iter()
            .map(|b| b.tenor_years)
            .collect();
        assert_eq!(tenors, vec![2.0, 5.0, 10.0]);

        // DV01-weighted modified duration: (7×7000 + 3×3500) / 10500 ≈ 5.667.
        // The 10Y leg dominates DV01, so the book duration tilts long of
        // the MV-weighted midpoint.
        assert_relative_eq!(book.modified_duration_years, 5.667, epsilon = 1e-3);

        // Face-weighted per-100 prices: both positions share the sample's
        // (clean=99, dirty=100, accrued=1), so the weighted averages match.
        assert_relative_eq!(book.clean_price_per_100, 99.0, epsilon = 1e-9);
        assert_relative_eq!(book.dirty_price_per_100, 100.0, epsilon = 1e-9);
        assert_relative_eq!(book.accrued_per_100, 1.0, epsilon = 1e-9);

        // YTM is 0 at the book level — yields don't compound linearly;
        // we zero it rather than emit a number that invites misuse.
        assert_eq!(book.ytm_decimal, 0.0);

        // Curves union, deduped, insertion order.
        assert_eq!(book.provenance.curves_used, vec!["sofr", "tsy"]);
        assert_eq!(book.provenance.cost_model, "heuristic_v1");
        assert_eq!(book.provenance.oas_volatility, None);
    }

    #[test]
    fn aggregate_single_position_passes_through_book_metrics() {
        // No fast path — single-position aggregation still runs the full
        // math, which means ytm becomes NaN and position_id is set from
        // book_id (or None). DV01-weighted duration on one position equals
        // its own duration.
        let p = sample();
        let book = aggregate_risk_profiles(std::slice::from_ref(&p), Some("BOOK".into())).unwrap();
        assert_eq!(book.position_id.as_deref(), Some("BOOK"));
        assert_eq!(book.dv01, p.dv01);
        assert_eq!(book.market_value, p.market_value);
        assert_relative_eq!(book.modified_duration_years, p.modified_duration_years);
        assert_eq!(book.ytm_decimal, 0.0);
    }

    #[test]
    fn aggregate_empty_errors() {
        let err = aggregate_risk_profiles(&[], None);
        assert!(matches!(err, Err(AnalyticsError::InvalidInput(_))));
    }

    #[test]
    fn aggregate_currency_mismatch_errors() {
        let a = sample();
        let mut b = sample();
        b.currency = Currency::EUR;
        let err = aggregate_risk_profiles(&[a, b], None);
        match err {
            Err(AnalyticsError::InvalidInput(msg)) => assert!(msg.contains("currency")),
            other => panic!("expected currency InvalidInput, got {other:?}"),
        }
    }

    #[test]
    fn aggregate_evaluation_date_mismatch_errors() {
        let a = sample();
        let mut b = sample();
        b.settlement = Date::from_ymd(2026, 6, 1).unwrap();
        let err = aggregate_risk_profiles(&[a, b], None);
        match err {
            Err(AnalyticsError::InvalidInput(msg)) => {
                assert!(msg.contains("evaluation date"), "got: {msg}");
            }
            other => panic!("expected evaluation-date InvalidInput, got {other:?}"),
        }
    }

    #[test]
    fn aggregate_drops_oas_volatility_on_mixed_book() {
        let mut a = sample();
        a.provenance.oas_volatility = Some(0.01);
        let b = sample();
        let book = aggregate_risk_profiles(&[a, b], None).unwrap();
        assert_eq!(book.provenance.oas_volatility, None);
    }

    #[test]
    fn aggregate_dv01_dominated_long_short_picks_dominant_duration() {
        // Long $10mm 2Y (DV01 +1900) + short $10mm 30Y (DV01 -15000).
        // Net DV01 ≈ -13100, dominated by the 30Y short. The DV01-weighted
        // duration tracks the long end; downstream pick_future_contract
        // sees a long-end exposure and picks the right contract.
        let long_2y = sample_with(
            "L2Y",
            dec!(10_000_000),
            dec!(10_000_000),
            1900.0,
            2.0,
            vec![(2.0, 1900.0)],
            "sofr",
        );
        let short_30y = sample_with(
            "S30Y",
            dec!(-10_000_000),
            dec!(-10_000_000),
            -15000.0,
            30.0,
            vec![(30.0, -15000.0)],
            "sofr",
        );
        let book = aggregate_risk_profiles(&[long_2y, short_30y], None).unwrap();

        // Net DV01 = 1900 - 15000 = -13100. Sign carries through.
        assert_relative_eq!(book.dv01, -13100.0, epsilon = 1e-9);
        // DV01-weighted: (2 × 1900 + 30 × -15000) / (1900 - 15000)
        //              = (3800 - 450000) / -13100 ≈ 34.06
        assert!(
            book.modified_duration_years > 30.0,
            "expected duration > 30Y (long-end dominant), got {}",
            book.modified_duration_years
        );
    }

    #[test]
    fn aggregate_dv01_neutral_book_reports_nan_duration() {
        // Long $10mm 5Y (DV01 +5000) + short matched DV01 → net flat.
        // No contract picker can run on this; we surface NaN.
        let mut a = sample();
        a.dv01 = 5000.0;
        a.modified_duration_years = 5.0;
        let mut b = sample();
        b.dv01 = -5000.0;
        b.modified_duration_years = 5.0;
        let book = aggregate_risk_profiles(&[a, b], None).unwrap();
        assert_relative_eq!(book.dv01, 0.0, epsilon = 1e-9);
        // Duration / convexity collapse to 0 on near-flat-DV01 books.
        assert_eq!(book.modified_duration_years, 0.0);
        assert_eq!(book.macaulay_duration_years, 0.0);
        assert_eq!(book.convexity, 0.0);
    }

    #[test]
    fn aggregate_marks_cost_model_mixed_when_positions_disagree() {
        let a = sample();
        let mut b = sample();
        b.provenance.cost_model = "live_v1".into();
        let book = aggregate_risk_profiles(&[a, b], None).unwrap();
        assert_eq!(book.provenance.cost_model, "mixed");
    }
}
