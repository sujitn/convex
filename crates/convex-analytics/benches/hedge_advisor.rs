//! Hedge advisor benchmarks. Baselines tracked in
//! `docs/perf-baselines.md`.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use rust_decimal_macros::dec;

use convex_analytics::risk::{
    barbell_futures, cash_bond_pair, compare_hedges, compute_position_risk, duration_futures,
    interest_rate_swap, narrate, Constraints,
};
use convex_bonds::instruments::FixedRateBond;
use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Compounding, Currency, Date, Frequency, Mark};
use convex_curves::{DiscreteCurve, InterpolationMethod, RateCurve, ValueType};

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

fn aapl_10y() -> FixedRateBond {
    FixedRateBond::builder()
        .cusip_unchecked("AAPL10Y")
        .coupon_rate(dec!(0.0485))
        .maturity(d(2034, 5, 10))
        .issue_date(d(2024, 5, 10))
        .frequency(Frequency::SemiAnnual)
        .day_count(DayCountConvention::Thirty360US)
        .currency(Currency::USD)
        .face_value(dec!(100))
        .build()
        .unwrap()
}

fn bench_advisor(c: &mut Criterion) {
    let bond = aapl_10y();
    let curve = flat_curve(0.045);
    let mark = Mark::Yield {
        value: dec!(0.0535),
        frequency: Frequency::SemiAnnual,
    };
    let settlement = d(2026, 1, 15);
    let tenors = [2.0_f64, 5.0, 10.0, 30.0];

    c.bench_function("risk_profile_apple_10y", |b| {
        b.iter(|| {
            let p = compute_position_risk(
                &bond,
                settlement,
                &mark,
                dec!(10_000_000),
                &curve,
                "usd_sofr",
                None,
                Some(&tenors),
                None,
            )
            .unwrap();
            black_box(p)
        })
    });

    let profile = compute_position_risk(
        &bond,
        settlement,
        &mark,
        dec!(10_000_000),
        &curve,
        "usd_sofr",
        None,
        Some(&tenors),
        None,
    )
    .unwrap();
    let cs = Constraints::default();

    c.bench_function("propose_four_strategies", |b| {
        b.iter(|| {
            let f = duration_futures(&profile, &cs, &curve, "usd_sofr", settlement).unwrap();
            let bb = barbell_futures(&profile, &cs, &curve, "usd_sofr", settlement).unwrap();
            let cb = cash_bond_pair(&profile, &cs, &curve, "usd_sofr", settlement).unwrap();
            let s = interest_rate_swap(&profile, &cs, &curve, "usd_sofr", settlement).unwrap();
            black_box((f, bb, cb, s))
        })
    });

    c.bench_function("end_to_end", |b| {
        b.iter(|| {
            let p = compute_position_risk(
                &bond,
                settlement,
                &mark,
                dec!(10_000_000),
                &curve,
                "usd_sofr",
                None,
                Some(&tenors),
                None,
            )
            .unwrap();
            let f = duration_futures(&p, &cs, &curve, "usd_sofr", settlement).unwrap();
            let bb = barbell_futures(&p, &cs, &curve, "usd_sofr", settlement).unwrap();
            let cb = cash_bond_pair(&p, &cs, &curve, "usd_sofr", settlement).unwrap();
            let s = interest_rate_swap(&p, &cs, &curve, "usd_sofr", settlement).unwrap();
            let report = compare_hedges(&p, &[f, bb, cb, s], &cs).unwrap();
            let text = narrate(&report);
            black_box(text)
        })
    });
}

criterion_group!(benches, bench_advisor);
criterion_main!(benches);
