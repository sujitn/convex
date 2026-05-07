//! Per-instrument risk: each function returns DV01 + KRD for one hedge leg
//! by building a synthetic underlying and routing through
//! `compute_position_risk`. Sign convention is in [`super::types`].

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use convex_bonds::instruments::FixedRateBond;
use convex_bonds::types::BondIdentifiers;
use convex_core::types::{Currency, Date, Mark, Spread, SpreadType};
use convex_curves::{DiscreteCurve, RateCurve};

use crate::error::{AnalyticsError, AnalyticsResult};
use crate::risk::profile::{compute_position_risk, KeyRateBucket};

use super::types::{BondFuture, CashBondLeg, InterestRateSwap, SwapSide};

/// Per-contract DV01 + KRD for a [`BondFuture`].
#[derive(Debug, Clone, PartialEq)]
#[allow(missing_docs)]
pub struct BondFutureRisk {
    pub dv01_per_contract: f64,
    pub buckets_per_contract: Vec<KeyRateBucket>,
}

/// Per-contract risk via a synthetic 6% CBOT-style deliverable, divided by
/// the conversion factor. v1 ignores CTD optionality and basis.
pub fn bond_future_risk(
    spec: &BondFuture,
    curve: &RateCurve<DiscreteCurve>,
    curve_id: &str,
    settlement: Date,
    key_rate_tenors: Option<&[f64]>,
) -> AnalyticsResult<BondFutureRisk> {
    if !spec.conversion_factor.is_finite() || spec.conversion_factor <= 0.0 {
        return Err(AnalyticsError::InvalidInput(format!(
            "BondFuture: conversion_factor must be finite and strictly positive (got {})",
            spec.conversion_factor
        )));
    }
    let ctd = representative_ctd(spec, settlement)?;
    let mark = Mark::Spread {
        value: Spread::new(Decimal::ZERO, SpreadType::ZSpread),
        benchmark: curve_id.to_string(),
    };
    let profile = compute_position_risk(
        &ctd,
        settlement,
        &mark,
        spec.contract_size_face,
        curve,
        curve_id,
        None,
        key_rate_tenors,
        None,
    )?;
    let cf = spec.conversion_factor;
    Ok(BondFutureRisk {
        dv01_per_contract: profile.dv01 / cf,
        buckets_per_contract: profile
            .key_rate_buckets
            .into_iter()
            .map(|b| KeyRateBucket {
                tenor_years: b.tenor_years,
                partial_dv01: b.partial_dv01 / cf,
            })
            .collect(),
    })
}

fn representative_ctd(spec: &BondFuture, settlement: Date) -> AnalyticsResult<FixedRateBond> {
    synthetic_sovereign_bond(
        spec.currency,
        dec!(0.06),
        spec.underlying_tenor_years,
        settlement,
    )
}

/// Total swap DV01 + KRD, signed by `side`. Pay-fixed → negative DV01.
#[derive(Debug, Clone, PartialEq)]
#[allow(missing_docs)]
pub struct InterestRateSwapRisk {
    pub dv01: f64,
    pub buckets: Vec<KeyRateBucket>,
}

/// Fixed-leg DV01 priced Z-flat against the discount curve, signed by `side`.
/// Floating leg is treated as zero DV01 (post-LIBOR floating ≈ 0 at reset).
pub fn interest_rate_swap_risk(
    spec: &InterestRateSwap,
    curve: &RateCurve<DiscreteCurve>,
    curve_id: &str,
    settlement: Date,
    key_rate_tenors: Option<&[f64]>,
) -> AnalyticsResult<InterestRateSwapRisk> {
    if spec.tenor_years <= 0.0 {
        return Err(AnalyticsError::InvalidInput(format!(
            "InterestRateSwap: tenor_years must be > 0 (got {})",
            spec.tenor_years
        )));
    }
    if spec.notional <= Decimal::ZERO {
        return Err(AnalyticsError::InvalidInput(format!(
            "InterestRateSwap: notional must be strictly positive — encode direction via `side` \
             (PayFixed / ReceiveFixed), not by sign of notional (got {})",
            spec.notional
        )));
    }
    let fixed_rate = Decimal::from_f64_retain(spec.fixed_rate_decimal).ok_or_else(|| {
        AnalyticsError::InvalidInput(format!(
            "InterestRateSwap: fixed_rate_decimal not finite ({})",
            spec.fixed_rate_decimal
        ))
    })?;

    let fixed_leg = synthetic_fixed_leg(spec, fixed_rate, settlement)?;

    let mark = Mark::Spread {
        value: Spread::new(Decimal::ZERO, SpreadType::ZSpread),
        benchmark: curve_id.to_string(),
    };
    let leg_profile = compute_position_risk(
        &fixed_leg,
        settlement,
        &mark,
        spec.notional,
        curve,
        curve_id,
        Some(spec.fixed_frequency),
        key_rate_tenors,
        None,
    )?;

    // Pay-fixed: short the fixed leg → negate.
    let sign = match spec.side {
        SwapSide::PayFixed => -1.0,
        SwapSide::ReceiveFixed => 1.0,
    };
    Ok(InterestRateSwapRisk {
        dv01: leg_profile.dv01 * sign,
        buckets: leg_profile
            .key_rate_buckets
            .into_iter()
            .map(|b| KeyRateBucket {
                tenor_years: b.tenor_years,
                partial_dv01: b.partial_dv01 * sign,
            })
            .collect(),
    })
}

/// At inception, the fixed-leg PV01 equals the PV01 of an at-par bond with
/// the same coupon, frequency, day count, and tenor — so we reuse a bond.
fn synthetic_fixed_leg(
    spec: &InterestRateSwap,
    fixed_rate: Decimal,
    settlement: Date,
) -> AnalyticsResult<FixedRateBond> {
    if !spec.tenor_years.is_finite() || spec.tenor_years <= 0.0 {
        return Err(AnalyticsError::InvalidInput(format!(
            "InterestRateSwap: tenor_years must be a finite positive number (got {})",
            spec.tenor_years
        )));
    }
    let tenor_months = (spec.tenor_years * 12.0).round() as i32;
    if tenor_months <= 0 {
        return Err(AnalyticsError::InvalidInput(format!(
            "InterestRateSwap: tenor_years too small ({}; rounds to 0 months)",
            spec.tenor_years
        )));
    }
    let maturity = settlement
        .add_months(tenor_months)
        .map_err(|e| AnalyticsError::InvalidInput(format!("swap maturity: {e}")))?;

    FixedRateBond::builder()
        .identifiers(BondIdentifiers::new())
        .coupon_rate(fixed_rate)
        .face_value(dec!(100))
        .maturity(maturity)
        .issue_date(settlement)
        .currency(spec.currency)
        .frequency(spec.fixed_frequency)
        .day_count(spec.fixed_day_count)
        .build()
        .map_err(|e| AnalyticsError::BondError(format!("swap fixed leg build: {e}")))
}

/// Cash-bond leg DV01 + KRD, signed by `face_amount`.
#[derive(Debug, Clone, PartialEq)]
#[allow(missing_docs)]
pub struct CashBondRisk {
    pub dv01: f64,
    pub buckets: Vec<KeyRateBucket>,
}

/// DV01 + KRD of a synthetic OTR sovereign at the spec's coupon, scaled by
/// signed `face_amount`. Country preset by `currency`.
pub fn cash_bond_risk(
    spec: &CashBondLeg,
    curve: &RateCurve<DiscreteCurve>,
    curve_id: &str,
    settlement: Date,
    key_rate_tenors: Option<&[f64]>,
) -> AnalyticsResult<CashBondRisk> {
    if !spec.tenor_years.is_finite() || spec.tenor_years <= 0.0 {
        return Err(AnalyticsError::InvalidInput(format!(
            "CashBondLeg: tenor_years must be a finite positive number (got {})",
            spec.tenor_years
        )));
    }
    let coupon = Decimal::from_f64_retain(spec.coupon_rate_decimal).ok_or_else(|| {
        AnalyticsError::InvalidInput(format!(
            "CashBondLeg: coupon_rate_decimal not finite ({})",
            spec.coupon_rate_decimal
        ))
    })?;
    let bond = synthetic_sovereign_bond(spec.currency, coupon, spec.tenor_years, settlement)?;

    let mark = Mark::Spread {
        value: Spread::new(Decimal::ZERO, SpreadType::ZSpread),
        benchmark: curve_id.to_string(),
    };
    let profile = compute_position_risk(
        &bond,
        settlement,
        &mark,
        spec.face_amount,
        curve,
        curve_id,
        None,
        key_rate_tenors,
        None,
    )?;
    Ok(CashBondRisk {
        dv01: profile.dv01,
        buckets: profile.key_rate_buckets,
    })
}

/// Synthetic at-par sovereign issued on `settlement` (zero accrued), country
/// preset from `currency`. Shared by the CBOT-reference CTD and the OTR
/// cash-bond hedge leg.
fn synthetic_sovereign_bond(
    currency: Currency,
    coupon: Decimal,
    tenor_years: f64,
    settlement: Date,
) -> AnalyticsResult<FixedRateBond> {
    let tenor_months = (tenor_years * 12.0).round() as i32;
    if tenor_months <= 0 {
        return Err(AnalyticsError::InvalidInput(format!(
            "synthetic sovereign: tenor_years must round to >0 months (got {tenor_years})"
        )));
    }
    let maturity = settlement
        .add_months(tenor_months)
        .map_err(|e| AnalyticsError::InvalidInput(format!("sovereign maturity: {e}")))?;

    let mut builder = FixedRateBond::builder()
        .identifiers(BondIdentifiers::new())
        .coupon_rate(coupon)
        .face_value(dec!(100))
        .maturity(maturity)
        .issue_date(settlement);

    builder = match currency {
        Currency::USD => builder.us_treasury(),
        Currency::GBP => builder.uk_gilt(),
        Currency::EUR => builder.german_bund(),
        other => {
            return Err(AnalyticsError::InvalidInput(format!(
                "synthetic sovereign: no preset for currency {other:?}"
            )))
        }
    };

    builder
        .build()
        .map_err(|e| AnalyticsError::BondError(format!("sovereign build: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use convex_core::daycounts::DayCountConvention;
    use convex_core::types::{Compounding, Frequency};
    use convex_curves::{InterpolationMethod, ValueType};
    use rust_decimal_macros::dec;

    fn d(y: i32, m: u32, day: u32) -> Date {
        Date::from_ymd(y, m, day).unwrap()
    }

    fn flat_curve(rate: f64) -> RateCurve<DiscreteCurve> {
        let dc = DiscreteCurve::new(
            d(2026, 1, 15),
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

    fn ty_future(cf: f64) -> BondFuture {
        BondFuture {
            contract_code: "TY".into(),
            underlying_tenor_years: 10.0,
            conversion_factor: cf,
            contract_size_face: dec!(100_000),
            currency: Currency::USD,
        }
    }

    #[test]
    fn ty_future_dv01_is_positive_and_sized_per_contract() {
        let curve = flat_curve(0.05);
        let risk = bond_future_risk(&ty_future(1.0), &curve, "c", d(2026, 1, 15), None).unwrap();
        // 10Y 6% bond at 5% has dirty ~107.7, mod-dur ~7.6, DV01 per $100 ~$0.082.
        // For $100k face that's ~$82 per contract.
        assert!(
            risk.dv01_per_contract > 60.0 && risk.dv01_per_contract < 110.0,
            "DV01/contract = {} (expected ~$60-110)",
            risk.dv01_per_contract
        );
    }

    #[test]
    fn conversion_factor_scales_dv01_inversely() {
        let curve = flat_curve(0.05);
        let cf_one = bond_future_risk(&ty_future(1.0), &curve, "c", d(2026, 1, 15), None).unwrap();
        let cf_half = bond_future_risk(&ty_future(0.5), &curve, "c", d(2026, 1, 15), None).unwrap();
        assert_relative_eq!(
            cf_half.dv01_per_contract,
            cf_one.dv01_per_contract * 2.0,
            epsilon = 1e-6
        );
        for (a, b) in cf_one
            .buckets_per_contract
            .iter()
            .zip(cf_half.buckets_per_contract.iter())
        {
            assert_relative_eq!(b.partial_dv01, a.partial_dv01 * 2.0, epsilon = 1e-6);
        }
    }

    #[test]
    fn ten_year_future_concentrates_krd_at_ten_years() {
        let curve = flat_curve(0.05);
        let tenors = [2.0, 5.0, 10.0, 30.0];
        let risk =
            bond_future_risk(&ty_future(1.0), &curve, "c", d(2026, 1, 15), Some(&tenors)).unwrap();
        let by_tenor: std::collections::HashMap<_, _> = risk
            .buckets_per_contract
            .iter()
            .map(|b| ((b.tenor_years * 10.0) as i64, b.partial_dv01))
            .collect();
        let ten = by_tenor[&100];
        let two = by_tenor[&20];
        let thirty = by_tenor[&300];
        assert!(ten.abs() > two.abs() * 5.0, "10Y bucket should dominate 2Y");
        assert!(
            ten.abs() > thirty.abs() * 5.0,
            "10Y bucket should dominate 30Y"
        );
    }

    #[test]
    fn zero_conversion_factor_errors() {
        let curve = flat_curve(0.05);
        let err = bond_future_risk(&ty_future(0.0), &curve, "c", d(2026, 1, 15), None);
        assert!(matches!(err, Err(AnalyticsError::InvalidInput(_))));
    }

    #[test]
    fn negative_conversion_factor_errors() {
        let curve = flat_curve(0.05);
        let err = bond_future_risk(&ty_future(-1.0), &curve, "c", d(2026, 1, 15), None);
        assert!(
            matches!(err, Err(AnalyticsError::InvalidInput(_))),
            "negative CF must be rejected — would silently flip hedge sign"
        );
    }

    #[test]
    fn non_finite_conversion_factor_errors() {
        let curve = flat_curve(0.05);
        for cf in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let err = bond_future_risk(&ty_future(cf), &curve, "c", d(2026, 1, 15), None);
            assert!(
                matches!(err, Err(AnalyticsError::InvalidInput(_))),
                "non-finite CF ({cf}) must be rejected"
            );
        }
    }

    #[test]
    fn unsupported_currency_errors() {
        let curve = flat_curve(0.05);
        let mut spec = ty_future(1.0);
        spec.currency = Currency::JPY;
        let err = bond_future_risk(&spec, &curve, "c", d(2026, 1, 15), None);
        assert!(matches!(err, Err(AnalyticsError::InvalidInput(_))));
    }

    #[test]
    fn gbp_long_gilt_future_builds_a_gilt_ctd() {
        let curve = flat_curve(0.04);
        let spec = BondFuture {
            contract_code: "G".into(),
            underlying_tenor_years: 10.0,
            conversion_factor: 1.0,
            contract_size_face: dec!(100_000),
            currency: Currency::GBP,
        };
        let risk = bond_future_risk(&spec, &curve, "c", d(2026, 1, 15), None).unwrap();
        assert!(risk.dv01_per_contract > 0.0);
    }

    // -- InterestRateSwap --------------------------------------------------

    fn sofr_swap(side: SwapSide, tenor_years: f64, notional: Decimal) -> InterestRateSwap {
        InterestRateSwap {
            tenor_years,
            fixed_rate_decimal: 0.045,
            fixed_frequency: Frequency::SemiAnnual,
            fixed_day_count: DayCountConvention::Act360,
            floating_index: "SOFR".into(),
            side,
            notional,
            currency: Currency::USD,
        }
    }

    #[test]
    fn pay_fixed_swap_has_negative_dv01() {
        let curve = flat_curve(0.045);
        let swap = sofr_swap(SwapSide::PayFixed, 10.0, dec!(10_000_000));
        let risk = interest_rate_swap_risk(&swap, &curve, "c", d(2026, 1, 15), None).unwrap();
        assert!(
            risk.dv01 < 0.0,
            "pay-fixed DV01 should be negative; got {}",
            risk.dv01
        );
        for b in &risk.buckets {
            assert!(
                b.partial_dv01 <= 0.0 || b.partial_dv01.abs() < 1e-6,
                "pay-fixed bucket {} should be ≤ 0 (got {})",
                b.tenor_years,
                b.partial_dv01
            );
        }
    }

    #[test]
    fn receive_fixed_is_negation_of_pay_fixed() {
        let curve = flat_curve(0.045);
        let pay = interest_rate_swap_risk(
            &sofr_swap(SwapSide::PayFixed, 10.0, dec!(10_000_000)),
            &curve,
            "c",
            d(2026, 1, 15),
            None,
        )
        .unwrap();
        let recv = interest_rate_swap_risk(
            &sofr_swap(SwapSide::ReceiveFixed, 10.0, dec!(10_000_000)),
            &curve,
            "c",
            d(2026, 1, 15),
            None,
        )
        .unwrap();
        assert_relative_eq!(pay.dv01, -recv.dv01, epsilon = 1e-6);
        for (a, b) in pay.buckets.iter().zip(recv.buckets.iter()) {
            assert_relative_eq!(a.partial_dv01, -b.partial_dv01, epsilon = 1e-6);
        }
    }

    #[test]
    fn swap_dv01_scales_linearly_with_notional() {
        let curve = flat_curve(0.045);
        let small = interest_rate_swap_risk(
            &sofr_swap(SwapSide::PayFixed, 10.0, dec!(1_000_000)),
            &curve,
            "c",
            d(2026, 1, 15),
            None,
        )
        .unwrap();
        let big = interest_rate_swap_risk(
            &sofr_swap(SwapSide::PayFixed, 10.0, dec!(10_000_000)),
            &curve,
            "c",
            d(2026, 1, 15),
            None,
        )
        .unwrap();
        assert_relative_eq!(big.dv01, small.dv01 * 10.0, epsilon = 1e-6);
    }

    #[test]
    fn ten_year_swap_concentrates_krd_at_ten_years() {
        let curve = flat_curve(0.045);
        let tenors = [2.0, 5.0, 10.0, 30.0];
        let risk = interest_rate_swap_risk(
            &sofr_swap(SwapSide::PayFixed, 10.0, dec!(10_000_000)),
            &curve,
            "c",
            d(2026, 1, 15),
            Some(&tenors),
        )
        .unwrap();
        let by: std::collections::HashMap<_, _> = risk
            .buckets
            .iter()
            .map(|b| ((b.tenor_years * 10.0) as i64, b.partial_dv01))
            .collect();
        let ten = by[&100].abs();
        let two = by[&20].abs();
        assert!(
            ten > two * 5.0,
            "10Y swap should concentrate KRD at 10Y (got |10Y|={ten}, |2Y|={two})"
        );
    }

    #[test]
    fn swap_dv01_magnitude_is_realistic_for_a_10y_par_swap() {
        let curve = flat_curve(0.045);
        let risk = interest_rate_swap_risk(
            &sofr_swap(SwapSide::PayFixed, 10.0, dec!(10_000_000)),
            &curve,
            "c",
            d(2026, 1, 15),
            None,
        )
        .unwrap();
        // Textbook: 10Y par swap PV01 ≈ 8 × notional × 1bp ≈ $8,000 on $10mm.
        let mag = risk.dv01.abs();
        assert!(
            mag > 6_000.0 && mag < 11_000.0,
            "|DV01| for 10Y $10mm swap should be ~$6-11k; got {mag}"
        );
    }

    #[test]
    fn zero_tenor_swap_errors() {
        let curve = flat_curve(0.045);
        let mut spec = sofr_swap(SwapSide::PayFixed, 10.0, dec!(1_000_000));
        spec.tenor_years = 0.0;
        let err = interest_rate_swap_risk(&spec, &curve, "c", d(2026, 1, 15), None);
        assert!(matches!(err, Err(AnalyticsError::InvalidInput(_))));
    }

    #[test]
    fn non_positive_swap_notional_errors() {
        let curve = flat_curve(0.045);
        for n in [Decimal::ZERO, dec!(-1_000_000)] {
            let spec = sofr_swap(SwapSide::PayFixed, 10.0, n);
            let err = interest_rate_swap_risk(&spec, &curve, "c", d(2026, 1, 15), None);
            assert!(
                matches!(err, Err(AnalyticsError::InvalidInput(_))),
                "notional={n} must be rejected — direction encoded by `side`, not sign"
            );
        }
    }

    // ---- CashBondLeg ----------------------------------------------------

    fn ust(face: Decimal, tenor: f64) -> CashBondLeg {
        CashBondLeg {
            tenor_years: tenor,
            coupon_rate_decimal: 0.045,
            currency: Currency::USD,
            face_amount: face,
        }
    }

    #[test]
    fn long_cash_bond_has_positive_dv01() {
        let curve = flat_curve(0.045);
        let r = cash_bond_risk(
            &ust(dec!(10_000_000), 10.0),
            &curve,
            "c",
            d(2026, 1, 15),
            None,
        )
        .unwrap();
        assert!(
            r.dv01 > 0.0,
            "long UST DV01 should be positive; got {}",
            r.dv01
        );
    }

    #[test]
    fn short_cash_bond_flips_dv01_sign() {
        let curve = flat_curve(0.045);
        let long = cash_bond_risk(
            &ust(dec!(10_000_000), 10.0),
            &curve,
            "c",
            d(2026, 1, 15),
            None,
        )
        .unwrap();
        let short = cash_bond_risk(
            &ust(dec!(-10_000_000), 10.0),
            &curve,
            "c",
            d(2026, 1, 15),
            None,
        )
        .unwrap();
        assert_relative_eq!(long.dv01, -short.dv01, epsilon = 1e-6);
    }

    #[test]
    fn cash_bond_dv01_scales_linearly_with_face() {
        let curve = flat_curve(0.045);
        let small = cash_bond_risk(
            &ust(dec!(1_000_000), 10.0),
            &curve,
            "c",
            d(2026, 1, 15),
            None,
        )
        .unwrap();
        let big = cash_bond_risk(
            &ust(dec!(10_000_000), 10.0),
            &curve,
            "c",
            d(2026, 1, 15),
            None,
        )
        .unwrap();
        assert_relative_eq!(big.dv01, small.dv01 * 10.0, epsilon = 1e-6);
    }

    #[test]
    fn ten_year_cash_bond_concentrates_krd_at_ten_years() {
        let curve = flat_curve(0.045);
        let tenors = [2.0, 5.0, 10.0, 30.0];
        let r = cash_bond_risk(
            &ust(dec!(10_000_000), 10.0),
            &curve,
            "c",
            d(2026, 1, 15),
            Some(&tenors),
        )
        .unwrap();
        let by: std::collections::HashMap<i64, f64> = r
            .buckets
            .iter()
            .map(|b| ((b.tenor_years * 10.0) as i64, b.partial_dv01))
            .collect();
        let ten = by[&100].abs();
        let two = by[&20].abs();
        assert!(
            ten > two * 5.0,
            "10Y bond should concentrate KRD at 10Y (got |10Y|={ten}, |2Y|={two})"
        );
    }

    #[test]
    fn cash_bond_zero_tenor_errors() {
        let curve = flat_curve(0.045);
        let err = cash_bond_risk(
            &ust(dec!(1_000_000), 0.0),
            &curve,
            "c",
            d(2026, 1, 15),
            None,
        );
        assert!(matches!(err, Err(AnalyticsError::InvalidInput(_))));
    }

    #[test]
    fn cash_bond_unsupported_currency_errors() {
        let curve = flat_curve(0.045);
        let mut spec = ust(dec!(1_000_000), 10.0);
        spec.currency = Currency::JPY;
        let err = cash_bond_risk(&spec, &curve, "c", d(2026, 1, 15), None);
        assert!(matches!(err, Err(AnalyticsError::InvalidInput(_))));
    }
}
