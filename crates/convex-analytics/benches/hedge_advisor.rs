//! Hedge advisor benchmarks. Baselines tracked in
//! `docs/perf-baselines.md`.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use rust_decimal_macros::dec;

use convex_analytics::risk::{
    barbell_futures, cash_bond_pair, compare_hedges, compute_position_risk, duration_futures,
    interest_rate_swap, key_rate_futures, narrate, Constraints,
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

    c.bench_function("propose_five_strategies", |b| {
        b.iter(|| {
            let f =
                duration_futures(&profile, &cs, &curve, "usd_sofr", settlement, &[], None).unwrap();
            let bb =
                barbell_futures(&profile, &cs, &curve, "usd_sofr", settlement, &[], None).unwrap();
            let kr =
                key_rate_futures(&profile, &cs, &curve, "usd_sofr", settlement, &[], None).unwrap();
            let cb = cash_bond_pair(&profile, &cs, &curve, "usd_sofr", settlement, None).unwrap();
            let s =
                interest_rate_swap(&profile, &cs, &curve, "usd_sofr", settlement, None).unwrap();
            black_box((f, bb, kr, cb, s))
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
            let f = duration_futures(&p, &cs, &curve, "usd_sofr", settlement, &[], None).unwrap();
            let bb = barbell_futures(&p, &cs, &curve, "usd_sofr", settlement, &[], None).unwrap();
            let kr = key_rate_futures(&p, &cs, &curve, "usd_sofr", settlement, &[], None).unwrap();
            let cb = cash_bond_pair(&p, &cs, &curve, "usd_sofr", settlement, None).unwrap();
            let s = interest_rate_swap(&p, &cs, &curve, "usd_sofr", settlement, None).unwrap();
            let report = compare_hedges(&p, &[f, bb, kr, cb, s], &cs).unwrap();
            let text = narrate(&report);
            black_box(text)
        })
    });
}

// ---- PnL narrator -------------------------------------------------------

use convex_analytics::risk::pnl::{
    attribute_pnl, narrate_attribution, AttributionConfig, InterestRateSwapPnlSpec, ResolvedBook,
    ResolvedPosition,
};
use convex_analytics::risk::SwapSide;
use convex_core::types::{Spread, SpreadType};
use rust_decimal::Decimal;

fn eur_curve(ref_date: Date, bump_bps: f64) -> RateCurve<DiscreteCurve> {
    let tenors: Vec<f64> = vec![0.25, 0.5, 1.0, 2.0, 3.0, 5.0, 7.0, 10.0, 20.0, 30.0];
    let rates: Vec<f64> = tenors
        .iter()
        .map(|t| 0.022 + 0.0010 * t.sqrt() + bump_bps * 1e-4)
        .collect();
    let dc = DiscreteCurve::new(
        ref_date,
        tenors,
        rates,
        ValueType::ZeroRate {
            compounding: Compounding::Continuous,
            day_count: DayCountConvention::Act365Fixed,
        },
        InterpolationMethod::Linear,
    )
    .unwrap();
    RateCurve::new(dc)
}

fn eur_sov(cusip: &str, coupon: f64, mat: Date) -> FixedRateBond {
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

fn z(bps: f64, b: &str) -> Mark {
    Mark::Spread {
        value: Spread::new(Decimal::from_f64_retain(bps).unwrap(), SpreadType::ZSpread),
        benchmark: b.into(),
    }
}

fn demo_book() -> ResolvedBook {
    ResolvedBook {
        base_currency: Currency::EUR,
        positions: vec![
            ResolvedPosition::Bond {
                position_id: Some("OAT".into()),
                bond: Box::new(eur_sov("OAT10Y", 0.0275, d(2034, 5, 25))),
                notional_face: dec!(10_000_000),
                mark_t0: z(12.0, "FR.OAT-DE.BUND"),
                mark_t1: z(14.0, "FR.OAT-DE.BUND"),
            },
            ResolvedPosition::Bond {
                position_id: Some("BTP".into()),
                bond: Box::new(eur_sov("BTP10Y", 0.04, d(2035, 2, 1))),
                notional_face: dec!(5_000_000),
                mark_t0: z(135.0, "IT.BTP-DE.BUND"),
                mark_t1: z(141.0, "IT.BTP-DE.BUND"),
            },
            ResolvedPosition::Bond {
                position_id: Some("BUND".into()),
                bond: Box::new(eur_sov("BUND10Y", 0.025, d(2034, 8, 15))),
                notional_face: dec!(10_000_000),
                mark_t0: z(0.0, "DE.BUND"),
                mark_t1: z(0.0, "DE.BUND"),
            },
            ResolvedPosition::Swap {
                position_id: Some("EUR_SWAP".into()),
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
        ],
    }
}

fn bench_pnl(c: &mut Criterion) {
    let book = demo_book();
    let c0 = eur_curve(d(2026, 5, 7), 0.0);
    let c1 = eur_curve(d(2026, 5, 8), 6.0);
    let (t0, t1) = (d(2026, 5, 7), d(2026, 5, 8));
    let cfg = AttributionConfig::default();

    c.bench_function("attribute_pnl_demo_book", |b| {
        b.iter(|| {
            let a = attribute_pnl(&book, t0, t1, &c0, "c0", &c1, "c1", &cfg).unwrap();
            black_box(a)
        })
    });

    c.bench_function("attribute_pnl_then_narrate", |b| {
        b.iter(|| {
            let a = attribute_pnl(&book, t0, t1, &c0, "c0", &c1, "c1", &cfg).unwrap();
            black_box(narrate_attribution(&a))
        })
    });
}

criterion_group!(benches, bench_advisor, bench_pnl);
criterion_main!(benches);
