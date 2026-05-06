//! Per-position risk profile — output of `compute_position_risk`.
//!
//! `dv01` is signed: long fixed-coupon bond → positive DV01 (P&L for +1bp =
//! `−dv01`). `notional_face` follows the same convention. All `_per_100`
//! fields are per 100 face. KRD buckets are partial DV01 by tenor — Σ ≈
//! parallel `dv01` for small shifts.

use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use convex_bonds::traits::{Bond, FixedCouponBond};
use convex_core::types::{Compounding, Currency, Date, Frequency, Mark};
use convex_curves::bumping::KeyRateBump;
use convex_curves::{DiscreteCurve, RateCurve};

use crate::error::{AnalyticsError, AnalyticsResult};
use crate::pricing::price_from_mark;
use crate::risk::calculator::BondRiskCalculator;
use crate::risk::duration::STANDARD_KEY_RATE_TENORS;
use crate::spreads::ZSpreadCalculator;

/// One bucket on the key-rate ladder: partial DV01 attributable to a +1bp
/// shock at `tenor_years` only.
///
/// Different from [`crate::risk::duration::KeyRateDuration`] (which carries
/// duration only, per-unit). This one is position-scaled DV01.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct KeyRateBucket {
    /// Key tenor center in years.
    pub tenor_years: f64,
    /// Partial DV01 in `RiskProfile::currency`.
    pub partial_dv01: f64,
}

/// Audit metadata stamped on advisor outputs. Only the non-redundant bits
/// (the bond and curve are already visible to the caller).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Provenance {
    /// Curve ids used (discount, projection, govt).
    pub curves_used: Vec<String>,
    /// Cost-model name (`"heuristic_v1"` for v1).
    pub cost_model: String,
    /// `convex-analytics` crate version.
    pub advisor_version: String,
}

/// Risk profile of a single position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct RiskProfile {
    /// Caller-supplied position id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position_id: Option<String>,
    /// Position currency.
    pub currency: Currency,
    /// Settlement date used for pricing.
    pub settlement: Date,
    /// Face notional. Positive = long, negative = short.
    #[cfg_attr(feature = "schemars", schemars(with = "f64"))]
    pub notional_face: Decimal,
    /// Clean price per 100.
    pub clean_price_per_100: f64,
    /// Dirty price per 100.
    pub dirty_price_per_100: f64,
    /// Accrued interest per 100.
    pub accrued_per_100: f64,
    /// Dirty market value = `notional_face × dirty / 100`.
    #[cfg_attr(feature = "schemars", schemars(with = "f64"))]
    pub market_value: Decimal,
    /// Yield to maturity (decimal).
    pub ytm_decimal: f64,
    /// Modified duration of the underlying.
    pub modified_duration_years: f64,
    /// Macaulay duration of the underlying.
    pub macaulay_duration_years: f64,
    /// Analytical convexity of the underlying.
    pub convexity: f64,
    /// Total position DV01 in `currency`.
    pub dv01: f64,
    /// Per-tenor DV01 buckets, ascending.
    pub key_rate_buckets: Vec<KeyRateBucket>,
    /// Audit metadata.
    pub provenance: Provenance,
}

/// Compute per-position risk against a discount curve.
///
/// Mirrors the canonical Bloomberg-parity path used in `convex-ffi::dispatch`:
/// price the bond against the trader mark, derive analytical macaulay /
/// modified / convexity / DV01 via `BondRiskCalculator`, and bucket DV01 by
/// tenor with ±1bp triangular bumps holding the implied Z-spread fixed.
///
/// `notional_face` is signed (positive long, negative short). All scalar risk
/// fields scale linearly with notional.
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

    let market_value =
        notional_face * Decimal::from_f64_retain(priced.dirty_price_per_100).ok_or_else(|| {
            AnalyticsError::InvalidInput("dirty price not representable as Decimal".into())
        })? / Decimal::from(100);

    // Analytical risk metrics from BondRiskCalculator (per 100 face).
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
            cost_model: "heuristic_v1".to_string(),
            advisor_version: env!("CARGO_PKG_VERSION").to_string(),
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
                KeyRateBucket { tenor_years: 2.0, partial_dv01: 50.0 },
                KeyRateBucket { tenor_years: 5.0, partial_dv01: 450.0 },
            ],
            provenance: Provenance {
                curves_used: vec!["sofr".into()],
                cost_model: "heuristic_v1".into(),
                advisor_version: env!("CARGO_PKG_VERSION").into(),
            },
        }
    }

    #[test]
    fn round_trips_via_json() {
        let p = sample();
        let parsed: RiskProfile = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
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

        let bucket_sum: f64 = profile.key_rate_buckets.iter().map(|b| b.partial_dv01).sum();
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
        assert_eq!(profile.provenance.advisor_version, env!("CARGO_PKG_VERSION"));
    }
}
