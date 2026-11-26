use convex_analytics::{
    bond_quote::{
        curve_risk, quote_fixed, quote_zero, BenchmarkTenor, BondCurves, GSpreadBenchmark,
    },
    dto::{BondSpec, FixedRateSpec},
    pricing::price_from_mark,
    spreads::ZSpreadCalculator,
};
use convex_bonds::{
    instruments::{FixedRateBond, ZeroCouponBond},
    traits::{Bond, BondAnalytics},
    types::CalendarId,
};
use convex_core::{
    calendars::BusinessDayConvention,
    contracts::ErrorCode,
    daycounts::DayCountConvention,
    types::{Compounding, Date, Frequency, Mark, PriceKind},
};
use convex_curves::{
    CurveError, CurveResult, DiscreteCurve, InterpolationMethod, RateCurve, RateCurveDyn, ValueType,
};
use rust_decimal::{prelude::ToPrimitive, Decimal};
use rust_decimal_macros::dec;
use serde_json::Value;
use std::str::FromStr;

fn date(s: &str) -> Date {
    Date::parse(s).unwrap()
}
fn number(v: &Value) -> f64 {
    v.as_str().unwrap().parse().unwrap()
}
fn bond(spec: FixedRateSpec) -> FixedRateBond {
    let c = spec.coupon;
    let mut b = FixedRateBond::builder()
        .cusip_unchecked("SUPPLIED-CVX017")
        .coupon_rate(c.coupon_rate)
        .frequency(c.frequency)
        .maturity(c.maturity)
        .issue_date(c.issue)
        .day_count(c.day_count)
        .currency(c.currency)
        .face_value(c.face_value)
        .business_day_convention(c.business_day_convention);
    if let Some(t) = spec.terms {
        b = b
            .dated_date(t.dated_date)
            .calendar(CalendarId::new(t.calendar))
            .end_of_month(t.end_of_month)
            .settlement_days(t.settlement_days)
            .redemption_value(t.redemption_value);
        if let Some(d) = t.first_coupon_date {
            b = b.first_coupon_date(d);
        }
        if let Some(d) = t.penultimate_coupon_date {
            b = b.penultimate_coupon_date(d);
        }
        if let Some(r) = t.ex_coupon {
            b = b.ex_coupon_rule(r);
        }
    }
    b.build().unwrap()
}
fn regular() -> FixedRateBond {
    FixedRateBond::builder()
        .cusip_unchecked("R0")
        .issue_date(date("2024-01-15"))
        .maturity(date("2029-01-15"))
        .coupon_rate(dec!(0.05))
        .face_value(dec!(100))
        .frequency(Frequency::SemiAnnual)
        .day_count(DayCountConvention::Thirty360US)
        .calendar(CalendarId::weekend_only())
        .business_day_convention(BusinessDayConvention::Unadjusted)
        .end_of_month(false)
        .build()
        .unwrap()
}
fn curve(reference: Date, rate: f64) -> RateCurve<DiscreteCurve> {
    RateCurve::new(
        DiscreteCurve::new(
            reference,
            vec![0.01, 1.0, 5.0, 10.0],
            vec![rate; 4],
            ValueType::continuous_zero(DayCountConvention::Act365Fixed),
            InterpolationMethod::Linear,
        )
        .unwrap(),
    )
}
fn close(actual: f64, expected: f64, abs: f64, rel: f64, label: &str) {
    assert!(
        (actual - expected).abs() <= abs + rel * expected.abs(),
        "{label}: actual {actual}, expected {expected}"
    );
}
#[test]
fn supplied_contract_golden_coupon_ex_leap_stubs_eom_holiday_negative_yield() {
    let pack: Value = serde_json::from_str(include_str!("fixtures/bond-quotes.json")).unwrap();
    for case in pack["cases"].as_array().unwrap() {
        let mut constructor = case.clone();
        for metadata in [
            "id",
            "settlement",
            "yield",
            "quote_frequency",
            "quote_frequency_count",
            "accrued_fraction",
            "discount_cashflows",
            "expected",
        ] {
            constructor.as_object_mut().unwrap().remove(metadata);
        }
        let BondSpec::FixedRate(spec) = serde_json::from_value(constructor).unwrap() else {
            panic!("fixed");
        };
        let b = bond(spec);
        let settle = date(case["settlement"].as_str().unwrap());
        let frequency: Frequency = serde_json::from_value(case["quote_frequency"].clone()).unwrap();
        let y = Decimal::from_str(case["yield"].as_str().unwrap()).unwrap();
        let q = quote_fixed(
            &b,
            settle,
            &Mark::Yield {
                value: y,
                frequency,
            },
            frequency,
            BondCurves::default(),
            1.0,
            &[],
        )
        .unwrap();
        let p = q.pricing;
        let r = q.yield_risk;
        for (key, actual, tol) in [
            ("clean_price", p.clean_price_per_100, 1e-9),
            ("dirty_price", p.dirty_price_per_100, 1e-9),
            ("accrued", p.accrued_per_100, 1e-10),
            ("ytm_decimal", p.ytm_decimal, 1e-10),
            ("macaulay_duration", r.macaulay_duration, 1e-9),
            ("modified_duration", r.modified_duration, 1e-9),
            ("convexity", r.convexity, 1e-9),
            ("dv01", r.dv01_loss, 1e-9),
        ] {
            close(
                actual,
                number(&case["expected"][key]),
                tol,
                1e-11,
                &format!("{} {key}", case["id"]),
            );
        }
        let times = b.yield_cash_flows(settle, frequency).unwrap();
        let expected = case["discount_cashflows"].as_array().unwrap();
        assert_eq!(times.len(), expected.len());
        for ((time, amount), row) in times.iter().zip(expected) {
            close(*time, number(&row[0]), 1e-12, 1e-12, "discount time");
            close(*amount, number(&row[1]), 1e-10, 1e-12, "amount per100");
        }
        let back = price_from_mark(
            &b,
            settle,
            &Mark::Price {
                value: Decimal::from_f64_retain(p.clean_price_per_100).unwrap(),
                kind: PriceKind::Clean,
            },
            None,
            frequency,
        )
        .unwrap();
        close(
            back.ytm_decimal,
            y.to_f64().unwrap(),
            1e-10,
            1e-11,
            "yield inversion",
        );
        assert_eq!(q.errors.len(), 5);
        assert!(q
            .errors
            .iter()
            .all(|e| e.code() == ErrorCode::MissingMarketData));
    }
}
#[test]
fn native_risk_matches_actual_h_half_and_double_bumps_and_key_partition() {
    let b = regular();
    let settle = date("2025-02-17");
    let c = curve(date("2024-01-15"), 0.035);
    let calc = ZSpreadCalculator::new(&c);
    let z = 0.0075;
    let price = calc.try_price_with_spread(&b, z, settle).unwrap();
    for bump in [0.5, 1.0, 2.0] {
        let r = curve_risk(&b, settle, &c, z, bump, &[0.25, 0.5, 1.0, 2.0, 5.0, 10.0]).unwrap();
        assert!(r.zero_pv01_signed < 0.0);
        assert!(r.z_pv01_signed < 0.0);
        close(
            r.zero_pv01_signed,
            r.z_pv01_signed,
            1e-10,
            1e-10,
            "ACT365 zero and Z basis",
        );
        let up = calc
            .try_price_with_spread(&b, z + bump / 10000.0, settle)
            .unwrap();
        let dn = calc
            .try_price_with_spread(&b, z - bump / 10000.0, settle)
            .unwrap();
        close(
            r.z_pv01_signed,
            (up - dn) / (2.0 * bump),
            1e-12,
            1e-12,
            "signed bp normalization",
        );
        close(
            r.spread_duration,
            -r.z_pv01_signed * 10000.0 / price,
            1e-12,
            1e-12,
            "duration units",
        );
        close(
            r.key_rates.iter().map(|(_, v)| v).sum(),
            r.effective_duration,
            1e-6,
            1e-6,
            "partition sum",
        );
    }
}
#[test]
fn explicit_curve_roles_and_no_risk_or_curve_to_zero_substitution() {
    let b = regular();
    let s = date("2025-01-15");
    let c = curve(s, -0.01);
    let mark = Mark::Price {
        value: dec!(105),
        kind: PriceKind::Clean,
    };
    let q = quote_fixed(
        &b,
        s,
        &mark,
        Frequency::SemiAnnual,
        BondCurves {
            discount: Some(&c),
            ..Default::default()
        },
        1.0,
        &[],
    )
    .unwrap();
    assert!(q.z_spread_bps.is_some());
    assert!(q.curve_risk.is_some());
    assert!(q.i_spread_bps.is_none() && q.g_spread_bps.is_none() && q.asw_bps.is_none());
    assert_eq!(q.errors.len(), 3);
    assert!(quote_fixed(
        &b,
        s,
        &mark,
        Frequency::Zero,
        BondCurves::default(),
        1.0,
        &[]
    )
    .is_err());
    for bump in [0.0, -1.0, 101.0, f64::NAN, f64::INFINITY] {
        assert!(quote_fixed(
            &b,
            s,
            &mark,
            Frequency::SemiAnnual,
            BondCurves::default(),
            bump,
            &[]
        )
        .is_err());
    }
    for keys in [
        vec![1.0, 1.0],
        vec![2.0, 1.0],
        vec![f64::NAN],
        vec![150.0],
        vec![1.0; 65],
    ] {
        assert!(curve_risk(&b, s, &c, 0.0, 1.0, &keys).is_err());
    }
}
struct FailedCurve;
impl RateCurveDyn for FailedCurve {
    fn reference_date(&self) -> Date {
        date("2024-01-15")
    }
    fn max_date(&self) -> Date {
        date("2029-01-15")
    }
    fn discount_factor(&self, _: f64) -> CurveResult<f64> {
        Err(CurveError::invalid_value("required input absent"))
    }
    fn zero_rate(&self, _: f64, _: Compounding) -> CurveResult<f64> {
        Err(CurveError::invalid_value("absent"))
    }
    fn forward_rate(&self, _: f64, _: f64) -> CurveResult<f64> {
        Err(CurveError::invalid_value("absent"))
    }
    fn instantaneous_forward(&self, _: f64) -> CurveResult<f64> {
        Err(CurveError::invalid_value("absent"))
    }
}
#[test]
fn failed_curve_and_spread_mark_are_failures_with_no_plausible_zero_price() {
    let b = regular();
    let s = date("2025-01-15");
    let q = quote_fixed(
        &b,
        s,
        &Mark::Price {
            value: dec!(100),
            kind: PriceKind::Clean,
        },
        Frequency::SemiAnnual,
        BondCurves {
            discount: Some(&FailedCurve),
            ..Default::default()
        },
        1.0,
        &[],
    )
    .unwrap();
    assert!(q.z_spread_bps.is_none() && q.curve_risk.is_none());
    assert_eq!(q.errors.len(), 5);
    let mark = Mark::from_str("50 Z@SUPPLIED").unwrap();
    assert!(price_from_mark(&b, s, &mark, Some(&FailedCurve), Frequency::SemiAnnual).is_err());
    assert!(price_from_mark(&b, s, &mark, None, Frequency::SemiAnnual).is_err());
    assert!(ZSpreadCalculator::new(&FailedCurve)
        .try_price_with_spread(&b, 0.0, s)
        .is_err());
    assert!(ZSpreadCalculator::new(&FailedCurve)
        .price_with_spread(&b, 0.0, s)
        .is_nan());
}
#[test]
fn zero_actual_day_count_continuous_risk_and_per100_redemption() {
    let b = ZeroCouponBond::builder()
        .cusip_unchecked("ZERO")
        .issue_date(date("2023-01-01"))
        .maturity(date("2025-01-01"))
        .day_count(DayCountConvention::Act360)
        .compounding(convex_bonds::instruments::Compounding::Continuous)
        .face_value(dec!(1000))
        .redemption_value(dec!(1050))
        .build()
        .unwrap();
    let s = date("2024-01-01");
    let (p, r) = quote_zero(
        &b,
        s,
        &Mark::Yield {
            value: dec!(-0.02),
            frequency: Frequency::Annual,
        },
    )
    .unwrap();
    let t: f64 = 366.0 / 360.0;
    close(
        p.dirty_price_per_100,
        105.0 * 0.98_f64.powf(-t),
        1e-10,
        1e-12,
        "zero redemption/daycount",
    );
    close(r.modified_duration, t, 1e-12, 1e-12, "continuous duration");
    close(r.convexity, t * t, 1e-12, 1e-12, "continuous convexity");
    let (back, _) = quote_zero(
        &b,
        s,
        &Mark::Price {
            value: Decimal::from_f64_retain(p.clean_price_per_100).unwrap(),
            kind: PriceKind::Clean,
        },
    )
    .unwrap();
    close(
        back.ytm_decimal,
        0.98_f64.ln(),
        1e-12,
        1e-12,
        "zero inverse",
    );
    for value in [dec!(0), dec!(-1)] {
        assert!(quote_zero(
            &b,
            s,
            &Mark::Price {
                value,
                kind: PriceKind::Clean
            }
        )
        .is_err());
    }
    assert!(quote_zero(
        &b,
        date("2025-01-01"),
        &Mark::Price {
            value: dec!(100),
            kind: PriceKind::Clean
        }
    )
    .is_err());
}
#[test]
fn checked_construction_and_native_deserialization_reject_invalid_economics() {
    let b = regular();
    let mut v = serde_json::to_value(&b).unwrap();
    for (field, value) in [
        ("face_value", serde_json::json!(0)),
        ("coupon_rate", serde_json::json!(-1)),
        ("day_count", serde_json::json!("UNKNOWN")),
        ("redemption_value", serde_json::json!(0)),
    ] {
        let original = v[field].clone();
        v[field] = value;
        assert!(
            serde_json::from_value::<FixedRateBond>(v.clone()).is_err(),
            "{field}"
        );
        v[field] = original;
    }
    assert!(b.try_cash_flows(date("2029-01-15")).unwrap().is_empty());
    assert_eq!(
        b.try_accrued_interest(date("2029-01-15")).unwrap(),
        Decimal::ZERO
    );
    assert!(b
        .dirty_price_from_yield(date("2025-01-15"), -2.0, Frequency::SemiAnnual)
        .is_err());
}

/// A 2-year at 3% maturing 15 January 2026 and a 10-year at 5% maturing 15 January 2030,
/// semi-annual yields, supplied on 15 January 2025.
fn treasury_spec() -> convex_analytics::dto::GovernmentCurveSpec {
    use convex_analytics::spreads::{GovernmentBenchmark, Sovereign};
    use convex_bonds::types::Tenor;
    use convex_core::types::Yield;
    let benchmark = |tenor, maturity, y| {
        GovernmentBenchmark::with_cusip_unchecked(
            Sovereign::UST,
            tenor,
            "SUPPLIED",
            date(maturity),
            dec!(0.04),
            Yield::new(y, Compounding::SemiAnnual),
        )
    };
    convex_analytics::dto::GovernmentCurveSpec {
        reference_date: date("2025-01-15"),
        sovereign: Sovereign::UST,
        benchmarks: vec![
            benchmark(Tenor::Y2, "2026-01-15", dec!(0.03)),
            benchmark(Tenor::Y10, "2030-01-15", dec!(0.05)),
        ],
    }
}

#[test]
fn government_yields_use_actual_securities_maturities_and_no_empty_zero_benchmark() {
    use convex_analytics::spreads::{GovernmentCurve, Sovereign};
    let b = regular();
    let s = date("2025-01-15");
    let spec = treasury_spec();
    let g = convex_analytics::bond_quote::government_curve(&spec, s, b.currency()).unwrap();
    let weight =
        s.days_between(&b.maturity().unwrap()) as f64 - s.days_between(&date("2026-01-15")) as f64;
    let width = date("2026-01-15").days_between(&date("2030-01-15")) as f64;
    close(
        g.supplied_yield(s, b.maturity().unwrap())
            .unwrap()
            .value()
            .to_f64()
            .unwrap(),
        0.03 + 0.02 * weight / width,
        1e-12,
        1e-12,
        "actual maturity interpolation",
    );
    let mark = Mark::from_str("50 G@ACTUAL").unwrap();
    let q = quote_fixed(
        &b,
        s,
        &mark,
        Frequency::SemiAnnual,
        BondCurves {
            government: Some(GSpreadBenchmark {
                curve: &g,
                tenor: BenchmarkTenor::Maturity,
            }),
            ..Default::default()
        },
        1.0,
        &[],
    )
    .unwrap();
    assert_eq!(q.g_spread_bps, Some(50.0));
    assert!(quote_fixed(
        &b,
        s,
        &mark,
        Frequency::SemiAnnual,
        BondCurves::default(),
        1.0,
        &[]
    )
    .is_err());
    let empty = GovernmentCurve::new(Sovereign::UST, s);
    let q = quote_fixed(
        &b,
        s,
        &Mark::Price {
            value: dec!(100),
            kind: PriceKind::Clean,
        },
        Frequency::SemiAnnual,
        BondCurves {
            government: Some(GSpreadBenchmark {
                curve: &empty,
                tenor: BenchmarkTenor::Maturity,
            }),
            ..Default::default()
        },
        1.0,
        &[],
    )
    .unwrap();
    assert!(q.g_spread_bps.is_none());
    assert_eq!(
        q.errors
            .iter()
            .find(|e| e.field() == Some("g_spread"))
            .unwrap()
            .code(),
        ErrorCode::MissingReference
    );
    let mut bad = spec;
    bad.benchmarks.push(bad.benchmarks[0].clone());
    assert!(convex_analytics::bond_quote::government_curve(&bad, s, b.currency()).is_err());
}

/// A 30/360 corporate and an ACT/ACT ICMA Treasury, each 7 to 15 years from settlement.
fn level_bond(
    day_count: DayCountConvention,
    frequency: Frequency,
    years: i32,
    coupon: Decimal,
) -> FixedRateBond {
    FixedRateBond::builder()
        .cusip_unchecked("LEVEL")
        .issue_date(date("2025-11-15"))
        .maturity(date("2025-11-15").add_years(years).unwrap())
        .coupon_rate(coupon)
        .face_value(dec!(100))
        .frequency(frequency)
        .day_count(day_count)
        .calendar(CalendarId::weekend_only())
        .business_day_convention(BusinessDayConvention::Unadjusted)
        .end_of_month(false)
        .build()
        .unwrap()
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(64))]
    /// `price_from_level` turns a yield, a Z-, G- or I-spread into a price, and
    /// `quote_fixed` on that price publishes the same level: the stage and the router
    /// agree by construction, for a 30/360 corporate and an ACT/ACT ICMA Treasury, paying
    /// annual or semi-annual coupons and quoted semi-annually.
    #[test]
    fn a_level_priced_then_quoted_from_its_price_is_the_same_level(
        icma in proptest::bool::ANY,
        annual in proptest::bool::ANY,
        years in 7i32..=15,
        coupon_bp in 0u32..=900,
        level_bp in -100i64..=1_200,
        benchmark_bp in 0i64..=800,
    ) {
        use convex_analytics::{
            bond_quote::price_from_level,
            dto::GovernmentCurveSpec,
            spreads::{GovernmentBenchmark, Sovereign},
        };
        use convex_bonds::types::Tenor;
        use convex_core::types::{Spread, SpreadType, Yield};
        let day_count = if icma {
            DayCountConvention::ActActIcma
        } else {
            DayCountConvention::Thirty360US
        };
        let frequency = if annual { Frequency::Annual } else { Frequency::SemiAnnual };
        let b = level_bond(day_count, frequency, years, Decimal::new(coupon_bp.into(), 4));
        let s = date("2026-11-17");
        let discount = curve(s, 0.04);
        let swap = curve(s, 0.035);
        let benchmark_yield = Decimal::new(benchmark_bp, 4);
        let spec = GovernmentCurveSpec {
            reference_date: s,
            sovereign: Sovereign::UST,
            benchmarks: vec![GovernmentBenchmark::with_cusip_unchecked(
                Sovereign::UST,
                Tenor::Y10,
                "BENCHMARK",
                date("2035-11-15"),
                dec!(0.04125),
                Yield::new(benchmark_yield, Compounding::SemiAnnual),
            )],
        };
        let g = convex_analytics::bond_quote::government_curve(&spec, s, b.currency()).unwrap();
        let curves = || BondCurves {
            discount: Some(&discount as &dyn RateCurveDyn),
            swap: Some(&swap as &dyn RateCurveDyn),
            government: Some(GSpreadBenchmark {
                curve: &g,
                tenor: BenchmarkTenor::Maturity,
            }),
        };
        let level = Decimal::new(level_bp, 4);
        let spread = |kind| Mark::Spread {
            value: Spread::new(Decimal::new(level_bp, 0), kind),
            benchmark: "LEVEL".into(),
        };
        let marks = [
            Mark::Yield { value: level + dec!(0.02), frequency: Frequency::SemiAnnual },
            spread(SpreadType::ZSpread),
            spread(SpreadType::GSpread),
            spread(SpreadType::ISpread),
        ];
        for mark in marks {
            let priced = price_from_level(&b, s, &mark, Frequency::SemiAnnual, &curves()).unwrap();
            let clean = Decimal::from_f64_retain(priced.clean_price_per_100).unwrap();
            let quoted = quote_fixed(
                &b,
                s,
                &Mark::Price { value: clean, kind: PriceKind::Clean },
                Frequency::SemiAnnual,
                curves(),
                1.0,
                &[],
            )
            .unwrap();
            match &mark {
                Mark::Yield { value, .. } => close(
                    quoted.pricing.ytm_decimal,
                    value.to_f64().unwrap(),
                    1e-10,
                    0.0,
                    "yield",
                ),
                Mark::Spread { value, .. } if value.spread_type() == SpreadType::ZSpread => close(
                    quoted.z_spread_bps.unwrap(),
                    level_bp as f64,
                    1e-6,
                    0.0,
                    "z-spread",
                ),
                Mark::Spread { value, .. } if value.spread_type() == SpreadType::GSpread => close(
                    quoted.g_spread_bps.unwrap(),
                    level_bp as f64,
                    1e-6,
                    0.0,
                    "g-spread",
                ),
                // Published in whole basis points, so a whole level comes back exactly.
                _ => proptest::prop_assert_eq!(quoted.i_spread_bps, Some(level_bp as f64)),
            }
        }
    }
}

/// A G-spread reads the supplied government yields at the tenor its caller names: the
/// bond's maturity or its average life.
mod benchmark_tenor {
    use super::{close, date, regular, treasury_spec};
    use convex_analytics::{
        bond_quote::{
            government_curve, price_from_level, quote_fixed, BenchmarkTenor, BondCurves,
            GSpreadBenchmark,
        },
        spreads::GovernmentCurve,
        AnalyticsError,
    };
    use convex_bonds::contracts::AverageLife;
    use convex_bonds::instruments::{SinkingFundBond, SinkingFundPayment, SinkingFundSchedule};
    use convex_bonds::traits::Bond;
    use convex_core::types::{Date, Frequency, Mark, PriceKind};
    use rust_decimal::{prelude::ToPrimitive, Decimal};
    use rust_decimal_macros::dec;
    use std::str::FromStr;

    fn settlement() -> Date {
        date("2025-01-15")
    }

    /// The regular bullet sinking a quarter of its face on each 15 January from 2026 to
    /// 2028.
    fn sinker() -> SinkingFundBond {
        let schedule = ["2026-01-15", "2027-01-15", "2028-01-15"]
            .into_iter()
            .fold(SinkingFundSchedule::new(), |schedule, sink| {
                schedule.with_payment(SinkingFundPayment::new(date(sink), 25.0))
            });
        SinkingFundBond::new(regular(), schedule)
    }

    fn treasuries() -> GovernmentCurve {
        government_curve(&treasury_spec(), settlement(), convex_core::Currency::USD).unwrap()
    }

    /// Years of 365 days from settlement to `to`.
    fn years_to(to: &str) -> f64 {
        settlement().days_between(&date(to)) as f64 / 365.0
    }

    /// The treasuries' yields linearly interpolated at `years` from settlement.
    fn interpolated(years: f64) -> f64 {
        let (two, ten) = (years_to("2026-01-15"), years_to("2030-01-15"));
        0.03 + 0.02 * (years - two) / (ten - two)
    }

    fn read(curve: &GovernmentCurve, tenor: BenchmarkTenor, bond: &dyn Bond) -> f64 {
        GSpreadBenchmark { curve, tenor }
            .benchmark_yield(bond, settlement())
            .unwrap()
            .value()
            .to_f64()
            .unwrap()
    }

    fn average_life(bond: &SinkingFundBond) -> AverageLife {
        bond.average_life(settlement()).unwrap()
    }

    #[test]
    fn each_tenor_reads_the_yield_at_its_own_point() {
        let (curve, bond) = (treasuries(), sinker());
        close(
            read(&curve, BenchmarkTenor::Maturity, &bond),
            interpolated(years_to("2029-01-15")),
            1e-12,
            0.0,
            "maturity",
        );
        let life = average_life(&bond);
        // A quarter of the face each 365, 730, 1,095 and 1,461 days from settlement.
        assert_eq!(
            life.years(),
            Decimal::from(365 + 730 + 1095 + 1461) / dec!(1460)
        );
        close(
            read(&curve, BenchmarkTenor::AverageLife(life), &bond),
            interpolated(life.years().to_f64().unwrap()),
            1e-12,
            0.0,
            "average life",
        );
    }

    #[test]
    fn beyond_the_shortest_and_longest_securities_the_yield_is_flat() {
        let curve = treasuries();
        for (maturity, flat) in [("2025-07-15", 0.03), ("2035-01-15", 0.05)] {
            let supplied = curve.supplied_yield(settlement(), date(maturity)).unwrap();
            close(
                supplied.value().to_f64().unwrap(),
                flat,
                1e-12,
                0.0,
                maturity,
            );
        }
        // Nine tenths of the face sinks in six months: an average life inside a year.
        let bond = SinkingFundBond::new(
            regular(),
            SinkingFundSchedule::new()
                .with_payment(SinkingFundPayment::new(date("2025-07-15"), 90.0)),
        );
        let life = average_life(&bond);
        assert!(life.years() < Decimal::ONE);
        close(
            read(&curve, BenchmarkTenor::AverageLife(life), &bond),
            0.03,
            1e-12,
            0.0,
            "average life",
        );
    }

    #[test]
    fn a_bullets_average_life_reads_the_yield_its_maturity_does() {
        let (curve, bond) = (treasuries(), regular());
        let life = bond
            .contract_terms()
            .unwrap()
            .build()
            .unwrap()
            .average_life(settlement())
            .unwrap();
        assert_eq!(
            read(&curve, BenchmarkTenor::AverageLife(life), &bond).to_bits(),
            read(&curve, BenchmarkTenor::Maturity, &bond).to_bits()
        );
    }

    #[test]
    fn an_average_life_from_another_settlement_is_refused() {
        let (curve, bond) = (treasuries(), sinker());
        let life = bond.average_life(date("2025-01-16")).unwrap();
        let refused = GSpreadBenchmark {
            curve: &curve,
            tenor: BenchmarkTenor::AverageLife(life),
        }
        .benchmark_yield(&bond, settlement());
        assert!(matches!(refused, Err(AnalyticsError::InvalidInput(_))));
    }

    #[test]
    fn a_g_spread_priced_at_a_tenor_quotes_back_at_that_tenor() {
        let (curve, bond) = (treasuries(), sinker());
        let curves = |tenor| BondCurves {
            government: Some(GSpreadBenchmark {
                curve: &curve,
                tenor,
            }),
            ..Default::default()
        };
        let at_life = BenchmarkTenor::AverageLife(average_life(&bond));
        let mark = Mark::from_str("50 G@USD.TSY").unwrap();
        let priced = price_from_level(
            &bond,
            settlement(),
            &mark,
            Frequency::SemiAnnual,
            &curves(at_life),
        )
        .unwrap();
        let clean = Mark::Price {
            value: Decimal::from_f64_retain(priced.clean_price_per_100).unwrap(),
            kind: PriceKind::Clean,
        };
        let g_spread = |tenor| {
            quote_fixed(
                &bond,
                settlement(),
                &clean,
                Frequency::SemiAnnual,
                curves(tenor),
                1.0,
                &[],
            )
            .unwrap()
            .g_spread_bps
            .unwrap()
        };
        let quoted = g_spread(at_life);
        assert!((quoted - 50.0).abs() < 1e-6, "{quoted}");
        // Read at maturity, the same price is a G-spread over a higher benchmark yield.
        let at_maturity = g_spread(BenchmarkTenor::Maturity);
        assert!(quoted - at_maturity > 10.0, "{quoted} {at_maturity}");
    }
}
