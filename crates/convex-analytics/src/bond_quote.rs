//! Shared pure bond quote orchestration and explicit measure failures.
//! Application code supplies immutable instruments, curves and completion time.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use convex_bonds::{
    instruments::ZeroCouponBond,
    traits::{Bond, FixedCouponBond},
};
use convex_core::{
    contracts::{ErrorCode, StructuredError},
    types::{Compounding, Date, Frequency, Mark, Price, Yield},
};
use convex_curves::{CurveError, CurveResult, RateCurveDyn};
use rust_decimal::{prelude::ToPrimitive, Decimal};

use crate::{
    error::{AnalyticsError, AnalyticsResult},
    functions,
    pricing::{price_from_mark, PricingResult},
    spreads::{
        GSpreadCalculator, GovernmentCurve, ISpreadCalculator, ParParAssetSwap, ZSpreadCalculator,
    },
};

/// Analytical yield risk per 100 quote face; DV01 is the historical positive loss.
#[derive(Debug, Clone, Copy)]
pub struct YieldRisk {
    /// Years.
    pub macaulay_duration: f64,
    /// Years.
    pub modified_duration: f64,
    /// Years squared.
    pub convexity: f64,
    /// Price per 100 for one basis point in quoted yield.
    pub dv01_loss: f64,
}

/// Finite differences at frozen Z-spread, distinct from calibrated quote risk.
#[derive(Debug, Clone)]
pub struct CurveRisk {
    /// Signed price change per 100 for +1bp in continuous zero rates.
    pub zero_pv01_signed: f64,
    /// Signed price change per 100 for +1bp in continuous ACT/365F Z-spread.
    pub z_pv01_signed: f64,
    /// Parallel zero duration.
    pub effective_duration: f64,
    /// Parallel zero convexity.
    pub effective_convexity: f64,
    /// Z-spread duration.
    pub spread_duration: f64,
    /// Triangular continuous-zero durations, in caller supplied order.
    pub key_rates: Vec<(f64, f64)>,
    /// Actual central-difference half width in basis points.
    pub bump_bps: f64,
}

/// Explicit optional input roles; no discount/projection/benchmark substitution.
#[derive(Default)]
pub struct BondCurves<'a> {
    /// Discount curve for Z-spread and diagnostic zero risk.
    pub discount: Option<&'a dyn RateCurveDyn>,
    /// Swap benchmark for I-spread and the declared par-par ASW convention.
    pub swap: Option<&'a dyn RateCurveDyn>,
    /// Actual government benchmark yields for G-spread.
    pub government: Option<&'a GovernmentCurve>,
}

/// Supported maturity-equivalent analytics, plus individual unavailable measures.
#[derive(Debug, Clone)]
pub struct BondQuote {
    /// Clean/dirty/accrued/YTM in canonical units.
    pub pricing: PricingResult,
    /// Analytical quoted-yield risk.
    pub yield_risk: YieldRisk,
    /// Continuous ACT/365F spread in basis points.
    pub z_spread_bps: Option<f64>,
    /// Rounded native I-spread in basis points.
    pub i_spread_bps: Option<f64>,
    /// Rounded native G-spread in basis points.
    pub g_spread_bps: Option<f64>,
    /// Native par-par ASW in basis points.
    pub asw_bps: Option<f64>,
    /// Diagnostic zero/Z-spread risk; never calibrated market-quote risk.
    pub curve_risk: Option<CurveRisk>,
    /// Missing inputs, unsupported combinations or numerical failures.
    pub errors: Vec<StructuredError>,
}

fn finite(value: f64) -> AnalyticsResult<f64> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(AnalyticsError::CalculationFailed(
            "nonfinite bond measure".into(),
        ))
    }
}
fn decimal(value: f64) -> AnalyticsResult<Decimal> {
    Decimal::from_f64_retain(value)
        .ok_or_else(|| AnalyticsError::InvalidInput("decimal conversion".into()))
}
fn capture<T>(
    errors: &mut Vec<StructuredError>,
    field: &str,
    input: bool,
    result: AnalyticsResult<T>,
) -> AnalyticsResult<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) => {
            let code = if !input {
                ErrorCode::MissingMarketData
            } else {
                match &error {
                    AnalyticsError::YieldSolverFailed { .. }
                    | AnalyticsError::SolverConvergenceFailed { .. } => ErrorCode::SolverLimit,
                    AnalyticsError::BenchmarkNotFound(_) => ErrorCode::MissingReference,
                    _ => ErrorCode::InvalidInput,
                }
            };
            errors.push(
                StructuredError::new(
                    code,
                    Some(field.into()),
                    error.to_string().chars().take(512).collect::<String>(),
                    false,
                )
                .map_err(|e| AnalyticsError::InvalidInput(e.to_string()))?,
            );
            Ok(None)
        }
    }
}
fn missing(role: &str) -> AnalyticsError {
    AnalyticsError::InvalidInput(format!("missing explicit {role} input"))
}

fn risk_settings(bump: f64, tenors: &[f64]) -> AnalyticsResult<()> {
    if !bump.is_finite()
        || bump <= 0.0
        || bump > 100.0
        || tenors.len() > 64
        || tenors
            .iter()
            .any(|t| !t.is_finite() || *t <= 0.0 || *t >= 150.0)
        || tenors.windows(2).any(|w| w[1] <= w[0])
    {
        return Err(AnalyticsError::InvalidInput(
            "invalid risk bump/tenors".into(),
        ));
    }
    Ok(())
}

/// Validate actual supplied government securities; never derive them from zero rates.
pub fn government_curve(
    spec: &crate::dto::GovernmentCurveSpec,
    settlement: Date,
    currency: convex_core::types::Currency,
) -> AnalyticsResult<GovernmentCurve> {
    let mut maturities = std::collections::BTreeSet::new();
    let mut tenors = std::collections::BTreeSet::new();
    if spec.reference_date > settlement
        || spec.sovereign.currency() != currency
        || spec.benchmarks.is_empty()
        || spec.benchmarks.len() > 128
        || spec.benchmarks.iter().any(|b| {
            b.sovereign != spec.sovereign
                || b.maturity <= settlement
                || !maturities.insert(b.maturity)
                || !tenors.insert(b.tenor.months())
                || b.coupon < Decimal::ZERO
                || b.price.is_some_and(|p| p <= Decimal::ZERO)
        })
    {
        return Err(AnalyticsError::InvalidInput(
            "invalid government benchmark dates/currency/count/duplicates/economics".into(),
        ));
    }
    Ok(GovernmentCurve::from_benchmarks(
        spec.sovereign,
        spec.reference_date,
        spec.benchmarks.clone(),
    ))
}

/// Common mark-based yield risk, used by native, engine and C ABI routes.
pub fn yield_risk<B: Bond + FixedCouponBond>(
    bond: &B,
    settlement: Date,
    pricing: &PricingResult,
    frequency: Frequency,
) -> AnalyticsResult<YieldRisk> {
    let y = pricing.ytm_decimal;
    Ok(YieldRisk {
        macaulay_duration: finite(functions::macaulay_duration(
            bond, settlement, y, frequency,
        )?)?,
        modified_duration: finite(functions::modified_duration(
            bond, settlement, y, frequency,
        )?)?,
        convexity: finite(functions::convexity(bond, settlement, y, frequency)?)?,
        dv01_loss: finite(functions::dv01(
            bond,
            settlement,
            y,
            pricing.dirty_price_per_100,
            frequency,
        )?)?,
    })
}

/// One pure calculation route for fixed-coupon maturity analytics.
/// Optional-model price/OAS and floating coupons are distinct APIs.
pub fn quote_fixed<B: Bond + FixedCouponBond>(
    bond: &B,
    settlement: Date,
    mark: &Mark,
    quote_frequency: Frequency,
    curves: BondCurves<'_>,
    bump_bps: f64,
    key_tenors: &[f64],
) -> AnalyticsResult<BondQuote> {
    risk_settings(bump_bps, key_tenors)?;
    let government_mark;
    let (mark, price_curve) = match mark {
        Mark::Spread { value, .. }
            if value.spread_type() == convex_core::types::SpreadType::GSpread =>
        {
            let curve = curves
                .government
                .ok_or_else(|| missing("government benchmarks for G-spread mark"))?;
            if curve.sovereign().currency() != bond.currency() {
                return Err(AnalyticsError::InvalidInput("benchmark currency".into()));
            }
            let benchmark = curve.supplied_yield(
                settlement,
                bond.maturity().ok_or_else(|| missing("maturity"))?,
            )?;
            let value = benchmark
                .value()
                .checked_add(value.as_decimal())
                .ok_or_else(|| missing("bounded G-spread yield"))?;
            government_mark = Mark::Yield {
                value,
                frequency: Frequency::SemiAnnual,
            };
            (&government_mark, curves.discount)
        }
        Mark::Spread { value, .. }
            if value.spread_type() == convex_core::types::SpreadType::ISpread =>
        {
            (
                mark,
                Some(
                    curves
                        .swap
                        .ok_or_else(|| missing("swap curve for I-spread mark"))?,
                ),
            )
        }
        _ => (mark, curves.discount),
    };
    let pricing = price_from_mark(bond, settlement, mark, price_curve, quote_frequency)?;
    for value in [
        pricing.clean_price_per_100,
        pricing.dirty_price_per_100,
        pricing.accrued_per_100,
        pricing.ytm_decimal,
    ] {
        finite(value)?;
    }
    let yield_risk = yield_risk(bond, settlement, &pricing, quote_frequency)?;
    let mut errors = Vec::new();
    let z = capture(
        &mut errors,
        "z_spread",
        curves.discount.is_some(),
        (|| {
            let curve = curves.discount.ok_or_else(|| missing("discount curve"))?;
            let value = ZSpreadCalculator::new(curve)
                .calculate(bond, decimal(pricing.dirty_price_per_100)?, settlement)?
                .as_bps();
            finite(value.to_f64().ok_or_else(|| missing("finite Z-spread"))?)
        })(),
    )?;
    let compounding = Compounding::try_from_periods_per_year(quote_frequency.periods_per_year())
        .ok_or_else(|| AnalyticsError::InvalidInput("unsupported yield frequency".into()))?;
    let y = Yield::new(decimal(pricing.ytm_decimal)?, compounding);
    let i = capture(
        &mut errors,
        "i_spread",
        curves.swap.is_some(),
        (|| {
            let curve = curves.swap.ok_or_else(|| missing("swap benchmark curve"))?;
            finite(
                ISpreadCalculator::new(curve)
                    .calculate(bond, y, settlement)?
                    .as_bps()
                    .to_f64()
                    .ok_or_else(|| missing("finite I-spread"))?,
            )
        })(),
    )?;
    let g = capture(
        &mut errors,
        "g_spread",
        curves.government.is_some(),
        (|| {
            let curve = curves
                .government
                .ok_or_else(|| missing("government benchmarks"))?;
            if curve.sovereign().currency() != bond.currency() || curve.benchmark_count() > 128 {
                return Err(AnalyticsError::InvalidInput(
                    "government benchmark currency/count".into(),
                ));
            }
            let maturity = bond.maturity().ok_or_else(|| missing("bond maturity"))?;
            let benchmark = curve.supplied_yield(settlement, maturity)?;
            finite(
                GSpreadCalculator::new(curve)
                    .with_benchmark(crate::spreads::BenchmarkSpec::ExplicitYield(benchmark))
                    .calculate(bond, y, settlement)?
                    .as_bps()
                    .to_f64()
                    .ok_or_else(|| missing("finite G-spread"))?,
            )
        })(),
    )?;
    let asw = capture(
        &mut errors,
        "asw",
        curves.swap.is_some(),
        (|| {
            let curve = curves.swap.ok_or_else(|| missing("swap benchmark curve"))?;
            finite(
                ParParAssetSwap::new(curve)
                    .calculate(
                        bond,
                        Price::new(decimal(pricing.clean_price_per_100)?, bond.currency()),
                        settlement,
                    )?
                    .as_bps()
                    .to_f64()
                    .ok_or_else(|| missing("finite ASW"))?,
            )
        })(),
    )?;
    let risk = capture(
        &mut errors,
        "curve_risk",
        curves.discount.is_some(),
        (|| {
            let curve = curves.discount.ok_or_else(|| missing("discount curve"))?;
            let z = z.ok_or_else(|| missing("successfully solved Z-spread"))? / 10_000.0;
            curve_risk(bond, settlement, curve, z, bump_bps, key_tenors)
        })(),
    )?;
    Ok(BondQuote {
        pricing,
        yield_risk,
        z_spread_bps: z,
        i_spread_bps: i,
        g_spread_bps: g,
        asw_bps: asw,
        curve_risk: risk,
        errors,
    })
}

/// Exact bounded shifts of supplied DFs. This preserves the original interpolation
/// function and settlement normalization; no curve-node resampling occurs.
struct Shift<'a> {
    base: &'a dyn RateCurveDyn,
    bump: f64,
    key: Option<[f64; 3]>,
}
impl RateCurveDyn for Shift<'_> {
    fn discount_factor(&self, t: f64) -> CurveResult<f64> {
        if !t.is_finite() || !(0.0..=150.0).contains(&t) {
            return Err(CurveError::invalid_value("shift time outside domain"));
        }
        let weight = match self.key {
            None => 1.0,
            Some([left, center, right]) => {
                if (left == 0.0 && t <= center) || (right == 150.0 && t >= center) {
                    1.0
                } else if t <= left || t >= right {
                    0.0
                } else if t <= center {
                    (t - left) / (center - left)
                } else {
                    (right - t) / (right - center)
                }
            }
        };
        let value = self.base.discount_factor(t)? * (-self.bump * weight * t).exp();
        if !value.is_finite() || value <= 0.0 {
            return Err(CurveError::invalid_value("invalid shifted DF"));
        }
        Ok(value)
    }
    fn zero_rate(&self, t: f64, compounding: Compounding) -> CurveResult<f64> {
        convex_curves::contracts::rate_from_discount(self.discount_factor(t)?, t, compounding)
            .map_err(|e| CurveError::invalid_value(e.to_string()))
    }
    fn forward_rate(&self, a: f64, b: f64) -> CurveResult<f64> {
        if b <= a {
            return Err(CurveError::invalid_value("invalid forward interval"));
        }
        Ok((self.discount_factor(a)? / self.discount_factor(b)?).ln() / (b - a))
    }
    fn instantaneous_forward(&self, t: f64) -> CurveResult<f64> {
        self.forward_rate(t, t + 1e-5)
    }
    fn reference_date(&self) -> Date {
        self.base.reference_date()
    }
    fn max_date(&self) -> Date {
        self.base.max_date()
    }
    fn date_to_tenor(&self, date: Date) -> f64 {
        self.base.date_to_tenor(date)
    }
}

/// Diagnostic zero/Z-spread central differences at frozen continuous Z-spread.
/// These are per-100 sensitivities, not monetary position or recalibrated quote risk.
pub fn curve_risk<B: Bond + FixedCouponBond>(
    bond: &B,
    settlement: Date,
    curve: &dyn RateCurveDyn,
    z: f64,
    bump_bps: f64,
    tenors: &[f64],
) -> AnalyticsResult<CurveRisk> {
    risk_settings(bump_bps, tenors)?;
    let h = bump_bps / 10_000.0;
    let price = |c: &dyn RateCurveDyn, spread| {
        ZSpreadCalculator::new(c).try_price_with_spread(bond, spread, settlement)
    };
    let base = price(curve, z)?;
    let up = price(
        &Shift {
            base: curve,
            bump: h,
            key: None,
        },
        z,
    )?;
    let down = price(
        &Shift {
            base: curve,
            bump: -h,
            key: None,
        },
        z,
    )?;
    let z_up = price(curve, z + h)?;
    let z_down = price(curve, z - h)?;
    let mut keys = Vec::with_capacity(tenors.len());
    for (index, &center) in tenors.iter().enumerate() {
        let key = Some([
            if index == 0 { 0.0 } else { tenors[index - 1] },
            center,
            tenors.get(index + 1).copied().unwrap_or(150.0),
        ]);
        let ku = price(
            &Shift {
                base: curve,
                bump: h,
                key,
            },
            z,
        )?;
        let kd = price(
            &Shift {
                base: curve,
                bump: -h,
                key,
            },
            z,
        )?;
        keys.push((center, finite((kd - ku) / (2.0 * h * base))?));
    }
    Ok(CurveRisk {
        zero_pv01_signed: finite((up - down) / (2.0 * bump_bps))?,
        z_pv01_signed: finite((z_up - z_down) / (2.0 * bump_bps))?,
        effective_duration: finite((down - up) / (2.0 * h * base))?,
        effective_convexity: finite((up + down - 2.0 * base) / (base * h * h))?,
        spread_duration: finite((z_down - z_up) / (2.0 * h * base))?,
        key_rates: keys,
        bump_bps,
    })
}

/// Checked zero price/yield and risk with explicit day count and compounding.
/// Prices and redemption normalize to 100; expired/invalid inputs are errors.
pub fn quote_zero(
    bond: &ZeroCouponBond,
    settlement: Date,
    mark: &Mark,
) -> AnalyticsResult<(PricingResult, YieldRisk)> {
    if settlement < bond.issue_date() || settlement >= bond.maturity_date() {
        return Err(AnalyticsError::InvalidInput(
            "invalid zero settlement".into(),
        ));
    }
    let time = finite(
        convex_curves::contracts::year_fraction(settlement, bond.maturity_date(), bond.day_count())
            .map_err(|e| AnalyticsError::InvalidInput(e.to_string()))?,
    )?;
    if time <= 0.0 || time > 150.0 {
        return Err(AnalyticsError::InvalidInput("invalid zero horizon".into()));
    }
    let compounding = match bond.compounding() {
        convex_bonds::instruments::Compounding::Annual => Compounding::Annual,
        convex_bonds::instruments::Compounding::SemiAnnual => Compounding::SemiAnnual,
        convex_bonds::instruments::Compounding::Quarterly => Compounding::Quarterly,
        convex_bonds::instruments::Compounding::Monthly => Compounding::Monthly,
        convex_bonds::instruments::Compounding::Continuous => Compounding::Continuous,
    };
    let face = bond
        .face_value()
        .to_f64()
        .ok_or_else(|| missing("zero face"))?;
    let redemption = bond
        .redemption_value()
        .to_f64()
        .ok_or_else(|| missing("zero redemption"))?
        / face
        * 100.0;
    if face <= 0.0 || !redemption.is_finite() || redemption <= 0.0 {
        return Err(AnalyticsError::InvalidInput(
            "invalid zero face/redemption".into(),
        ));
    }
    let (price, y) = match mark {
        Mark::Price { value, .. } => {
            let price = value.to_f64().ok_or_else(|| missing("zero price"))?;
            let y =
                convex_curves::contracts::rate_from_discount(price / redemption, time, compounding)
                    .map_err(|e| AnalyticsError::InvalidInput(e.to_string()))?;
            (price, y)
        }
        Mark::Yield { value, frequency } => {
            let y = value.to_f64().ok_or_else(|| missing("zero yield"))?;
            let mark_compounding =
                Compounding::try_from_periods_per_year(frequency.periods_per_year()).ok_or_else(
                    || AnalyticsError::InvalidInput("unsupported zero mark frequency".into()),
                )?;
            let df = convex_curves::contracts::discount_from_rate(y, time, mark_compounding)
                .map_err(|e| AnalyticsError::InvalidInput(e.to_string()))?;
            let y = convex_curves::contracts::rate_from_discount(df, time, compounding)
                .map_err(|e| AnalyticsError::InvalidInput(e.to_string()))?;
            (redemption * df, y)
        }
        _ => {
            return Err(AnalyticsError::InvalidInput(
                "zero spread marks require a separately declared convention".into(),
            ))
        }
    };
    if !price.is_finite() || price <= 0.0 {
        return Err(AnalyticsError::InvalidInput("invalid zero price".into()));
    }
    let denom = if compounding == Compounding::Continuous {
        1.0
    } else {
        1.0 + y / f64::from(compounding.periods_per_year())
    };
    let modified = time / denom;
    let convexity = if compounding == Compounding::Continuous {
        time * time
    } else {
        time * (time + 1.0 / f64::from(compounding.periods_per_year())) / (denom * denom)
    };
    Ok((
        PricingResult {
            clean_price_per_100: finite(price)?,
            dirty_price_per_100: price,
            accrued_per_100: 0.0,
            ytm_decimal: finite(y)?,
            z_spread_bps: None,
            oas_bps: None,
        },
        YieldRisk {
            macaulay_duration: time,
            modified_duration: finite(modified)?,
            convexity: finite(convexity)?,
            dv01_loss: finite(modified * price * 0.0001)?,
        },
    ))
}
