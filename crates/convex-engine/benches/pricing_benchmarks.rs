//! Benchmarks for the convex-engine pricing components.
//!
//! Run with: cargo bench -p convex-engine

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use convex_core::{Currency, Date};
use convex_engine::curve_builder::BuiltCurve;
use convex_engine::etf_pricing::EtfPricer;
use convex_engine::portfolio_analytics::{Portfolio, PortfolioAnalyzer, Position};
use convex_engine::pricing_router::{PricingInput, PricingRouter};
use convex_traits::ids::{CurveId, EtfId, InstrumentId, PortfolioId};
use convex_traits::output::BondQuoteOutput;
use convex_traits::reference_data::{
    BondReferenceData, BondType, EtfHoldingEntry, EtfHoldings, IssuerType,
};

// =============================================================================
// TEST DATA GENERATORS
// =============================================================================

fn create_test_curve(ref_date: Date) -> BuiltCurve {
    BuiltCurve {
        curve_id: CurveId::new("USD_SOFR"),
        reference_date: ref_date,
        points: vec![
            (0.25, 0.030),
            (0.5, 0.032),
            (1.0, 0.035),
            (2.0, 0.038),
            (3.0, 0.040),
            (5.0, 0.045),
            (7.0, 0.048),
            (10.0, 0.050),
            (30.0, 0.055),
        ],
        built_at: 0,
        inputs_hash: "bench".to_string(),
    }
}

fn create_test_bond(id: usize) -> BondReferenceData {
    let coupons = [0.02, 0.025, 0.03, 0.035, 0.04, 0.045, 0.05];
    let maturities = [2026, 2027, 2028, 2029, 2030, 2031, 2032, 2033, 2034, 2035];
    let coupon_idx = id % coupons.len();
    let maturity_idx = id % maturities.len();

    BondReferenceData {
        instrument_id: InstrumentId::new(format!("BOND_{:05}", id)),
        isin: Some(format!("US9128{:05}X", id)),
        cusip: Some(format!("9128{:05}", id)),
        sedol: None,
        bbgid: None,
        description: format!("Test Bond {}", id),
        currency: Currency::USD,
        issue_date: Date::from_ymd(2020, 1, 15).unwrap(),
        maturity_date: Date::from_ymd(maturities[maturity_idx], 6, 15).unwrap(),
        coupon_rate: Some(Decimal::from_f64_retain(coupons[coupon_idx]).unwrap()),
        frequency: 2,
        day_count: "30/360".to_string(),
        face_value: dec!(100),
        bond_type: BondType::FixedBullet,
        issuer_type: IssuerType::CorporateIG,
        issuer_id: format!("ISSUER_{}", id % 10),
        issuer_name: format!("Test Issuer {}", id % 10),
        seniority: "Senior".to_string(),
        is_callable: false,
        call_schedule: vec![],
        is_putable: false,
        is_sinkable: false,
        floating_terms: None,
        inflation_index: None,
        inflation_base_index: None,
        has_deflation_floor: false,
        country_of_risk: "US".to_string(),
        sector: "Corporate".to_string(),
        amount_outstanding: Some(dec!(1000000000)),
        first_coupon_date: Some(Date::from_ymd(2020, 7, 15).unwrap()),
        last_updated: 0,
        source: "bench".to_string(),
    }
}

fn create_bond_batch(count: usize) -> Vec<BondReferenceData> {
    (0..count).map(create_test_bond).collect()
}

fn create_pricing_inputs(
    bonds: Vec<BondReferenceData>,
    settlement: Date,
    curve: &BuiltCurve,
) -> Vec<PricingInput> {
    bonds
        .into_iter()
        .enumerate()
        .map(|(i, bond)| {
            let price_offset = (i as f64 % 10.0) - 5.0;
            let price = 100.0 + price_offset;

            PricingInput {
                bond,
                settlement_date: settlement,
                market_price: Some(Decimal::from_f64_retain(price).unwrap()),
                discount_curve: Some(curve.clone()),
                benchmark_curve: Some(curve.clone()),
                government_curve: None,
                volatility: None,
            }
        })
        .collect()
}

fn create_test_quotes(count: usize) -> Vec<BondQuoteOutput> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    (0..count)
        .map(|i| BondQuoteOutput {
            instrument_id: InstrumentId::new(format!("BOND_{:05}", i)),
            isin: None,
            currency: Currency::USD,
            settlement_date: Date::from_ymd(2025, 6, 17).unwrap(),
            clean_price: Some(dec!(100.0)),
            dirty_price: Some(dec!(101.0)),
            accrued_interest: Some(dec!(1.0)),
            ytm: Some(dec!(0.05)),
            ytw: None,
            ytc: None,
            z_spread: Some(dec!(100)),
            i_spread: Some(dec!(95)),
            g_spread: Some(dec!(105)),
            asw: Some(dec!(98)),
            oas: None,
            discount_margin: None,
            simple_margin: None,
            modified_duration: Some(dec!(5.0)),
            macaulay_duration: Some(dec!(5.2)),
            effective_duration: Some(dec!(5.0)),
            spread_duration: Some(dec!(4.9)),
            convexity: Some(dec!(30)),
            effective_convexity: Some(dec!(29)),
            dv01: Some(dec!(0.05)),
            pv01: Some(dec!(0.049)),
            key_rate_durations: Some(vec![
                ("2Y".to_string(), dec!(1.0)),
                ("5Y".to_string(), dec!(3.0)),
                ("10Y".to_string(), dec!(1.0)),
            ]),
            cs01: Some(dec!(0.048)),
            timestamp: now,
            pricing_model: "bench".to_string(),
            source: "bench".to_string(),
            is_stale: false,
            quality: 100,
        })
        .collect()
}

fn create_test_holdings(count: usize) -> EtfHoldings {
    let holdings: Vec<EtfHoldingEntry> = (0..count)
        .map(|i| EtfHoldingEntry {
            instrument_id: InstrumentId::new(format!("BOND_{:05}", i)),
            weight: Decimal::from_f64_retain(1.0 / count as f64).unwrap(),
            shares: dec!(1000),
            market_value: dec!(100000),
            notional_value: dec!(100000),
            accrued_interest: Some(dec!(500)),
        })
        .collect();

    EtfHoldings {
        etf_id: EtfId::new("BENCH_ETF"),
        name: "Benchmark ETF".to_string(),
        currency: convex_core::Currency::USD,
        as_of_date: Date::from_ymd(2025, 6, 15).unwrap(),
        holdings,
        total_market_value: Decimal::from(count as i64 * 100000),
        shares_outstanding: dec!(10000),
        nav_per_share: Some(Decimal::from(count as i64 * 10)),
        last_updated: 0,
        source: "bench".to_string(),
    }
}

fn create_test_portfolio(count: usize) -> Portfolio {
    let positions: Vec<Position> = (0..count)
        .map(|i| Position {
            instrument_id: InstrumentId::new(format!("BOND_{:05}", i)),
            notional: dec!(1000000),
            sector: Some(format!("Sector_{}", i % 5)),
            rating: Some(format!("Rating_{}", i % 3)),
        })
        .collect();

    Portfolio {
        portfolio_id: PortfolioId::new("BENCH_PORT"),
        name: "Benchmark Portfolio".to_string(),
        currency: Currency::USD,
        positions,
    }
}

// =============================================================================
// SINGLE BOND PRICING BENCHMARKS
// =============================================================================

fn bench_single_bond_pricing(c: &mut Criterion) {
    let router = PricingRouter::new();
    let settlement = Date::from_ymd(2025, 6, 15).unwrap();
    let curve = create_test_curve(settlement);
    let bond = create_test_bond(0);

    let input = PricingInput {
        bond,
        settlement_date: settlement,
        market_price: Some(dec!(100.0)),
        discount_curve: Some(curve.clone()),
        benchmark_curve: Some(curve.clone()),
        government_curve: None,
        volatility: None,
    };

    c.bench_function("single_bond_price", |b| {
        b.iter(|| router.price(black_box(&input)))
    });
}

fn bench_single_bond_no_curves(c: &mut Criterion) {
    let router = PricingRouter::new();
    let settlement = Date::from_ymd(2025, 6, 15).unwrap();
    let bond = create_test_bond(0);

    let input = PricingInput {
        bond,
        settlement_date: settlement,
        market_price: Some(dec!(100.0)),
        discount_curve: None,
        benchmark_curve: None,
        government_curve: None,
        volatility: None,
    };

    c.bench_function("single_bond_price_no_curves", |b| {
        b.iter(|| router.price(black_box(&input)))
    });
}

// =============================================================================
// BATCH PRICING BENCHMARKS
// =============================================================================

fn bench_batch_pricing_sequential(c: &mut Criterion) {
    let router = PricingRouter::new();
    let settlement = Date::from_ymd(2025, 6, 15).unwrap();
    let curve = create_test_curve(settlement);

    let mut group = c.benchmark_group("batch_sequential");
    group.sample_size(50);

    for size in [10, 50, 100, 500].iter() {
        let bonds = create_bond_batch(*size);
        let inputs = create_pricing_inputs(bonds, settlement, &curve);

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &inputs, |b, inputs| {
            b.iter(|| router.price_batch(black_box(inputs)))
        });
    }
    group.finish();
}

fn bench_batch_pricing_parallel(c: &mut Criterion) {
    let router = PricingRouter::new();
    let settlement = Date::from_ymd(2025, 6, 15).unwrap();
    let curve = create_test_curve(settlement);

    let mut group = c.benchmark_group("batch_parallel");
    group.sample_size(50);

    for size in [10, 50, 100, 500, 1000].iter() {
        let bonds = create_bond_batch(*size);
        let inputs = create_pricing_inputs(bonds, settlement, &curve);

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &inputs, |b, inputs| {
            b.iter(|| router.price_batch_parallel(black_box(inputs)))
        });
    }
    group.finish();
}

fn bench_batch_comparison(c: &mut Criterion) {
    let router = PricingRouter::new();
    let settlement = Date::from_ymd(2025, 6, 15).unwrap();
    let curve = create_test_curve(settlement);

    let mut group = c.benchmark_group("batch_comparison_200");
    group.sample_size(50);

    let bonds = create_bond_batch(200);
    let inputs = create_pricing_inputs(bonds, settlement, &curve);

    group.throughput(Throughput::Elements(200));

    group.bench_function("sequential", |b| {
        b.iter(|| router.price_batch(black_box(&inputs)))
    });

    group.bench_function("parallel", |b| {
        b.iter(|| router.price_batch_parallel(black_box(&inputs)))
    });

    group.finish();
}

// =============================================================================
// ETF PRICING BENCHMARKS
// =============================================================================

fn bench_etf_inav(c: &mut Criterion) {
    let pricer = EtfPricer::new();
    let settlement = Date::from_ymd(2025, 6, 17).unwrap();

    let mut group = c.benchmark_group("etf_inav");

    for size in [50, 100, 500, 1000].iter() {
        let holdings = create_test_holdings(*size);
        let quotes = create_test_quotes(*size);

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &(holdings, quotes),
            |b, (holdings, quotes)| {
                b.iter(|| {
                    pricer.calculate_inav(black_box(holdings), black_box(quotes), settlement)
                })
            },
        );
    }
    group.finish();
}

fn bench_etf_batch(c: &mut Criterion) {
    let pricer = EtfPricer::new();
    let settlement = Date::from_ymd(2025, 6, 17).unwrap();

    let mut group = c.benchmark_group("etf_batch");
    group.sample_size(30);

    // Create multiple ETFs with 100 holdings each
    for num_etfs in [5, 10, 20].iter() {
        let etfs: Vec<EtfHoldings> = (0..*num_etfs)
            .map(|i| {
                let mut h = create_test_holdings(100);
                h.etf_id = EtfId::new(format!("ETF_{}", i));
                h
            })
            .collect();
        let quotes = create_test_quotes(100);

        group.throughput(Throughput::Elements(*num_etfs as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(num_etfs),
            &(etfs, quotes),
            |b, (etfs, quotes)| {
                b.iter(|| {
                    pricer.calculate_inav_batch(black_box(etfs), black_box(quotes), settlement)
                })
            },
        );
    }
    group.finish();
}

// =============================================================================
// PORTFOLIO ANALYTICS BENCHMARKS
// =============================================================================

fn bench_portfolio_analytics(c: &mut Criterion) {
    let analyzer = PortfolioAnalyzer::new();

    let mut group = c.benchmark_group("portfolio_analytics");

    for size in [50, 100, 500, 1000].iter() {
        let portfolio = create_test_portfolio(*size);
        let quotes = create_test_quotes(*size);

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &(portfolio, quotes),
            |b, (portfolio, quotes)| {
                b.iter(|| analyzer.calculate(black_box(portfolio), black_box(quotes)))
            },
        );
    }
    group.finish();
}

fn bench_duration_contribution(c: &mut Criterion) {
    let analyzer = PortfolioAnalyzer::new();

    let mut group = c.benchmark_group("duration_contribution");

    for size in [50, 100, 500].iter() {
        let portfolio = create_test_portfolio(*size);
        let quotes = create_test_quotes(*size);

        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &(portfolio, quotes),
            |b, (portfolio, quotes)| {
                b.iter(|| analyzer.duration_contribution(black_box(portfolio), black_box(quotes)))
            },
        );
    }
    group.finish();
}

fn bench_portfolio_batch(c: &mut Criterion) {
    let analyzer = PortfolioAnalyzer::new();

    let mut group = c.benchmark_group("portfolio_batch");
    group.sample_size(30);

    for num_portfolios in [5, 10, 20].iter() {
        let portfolios: Vec<Portfolio> = (0..*num_portfolios)
            .map(|i| {
                let mut p = create_test_portfolio(100);
                p.portfolio_id = PortfolioId::new(format!("PORT_{}", i));
                p
            })
            .collect();
        let quotes = create_test_quotes(100);

        group.throughput(Throughput::Elements(*num_portfolios as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(num_portfolios),
            &(portfolios, quotes),
            |b, (portfolios, quotes)| {
                b.iter(|| analyzer.calculate_batch(black_box(portfolios), black_box(quotes)))
            },
        );
    }
    group.finish();
}

// =============================================================================
// CURVE OPERATIONS BENCHMARKS
// =============================================================================

fn bench_curve_interpolation(c: &mut Criterion) {
    let ref_date = Date::from_ymd(2025, 6, 15).unwrap();
    let curve = create_test_curve(ref_date);

    let mut group = c.benchmark_group("curve_operations");

    group.bench_function("interpolate_single", |b| {
        b.iter(|| curve.interpolate_rate(black_box(5.5)))
    });

    // Batch interpolation
    let tenors: Vec<f64> = (0..100).map(|i| i as f64 * 0.3).collect();
    group.bench_function("interpolate_100_tenors", |b| {
        b.iter(|| {
            tenors
                .iter()
                .map(|t| curve.interpolate_rate(*t))
                .collect::<Vec<_>>()
        })
    });

    use convex_curves::RateCurveDyn;

    group.bench_function("discount_factor", |b| {
        b.iter(|| curve.discount_factor(black_box(5.0)))
    });

    group.bench_function("zero_rate", |b| {
        b.iter(|| curve.zero_rate(black_box(5.0), convex_curves::Compounding::SemiAnnual))
    });

    group.bench_function("forward_rate", |b| {
        b.iter(|| curve.forward_rate(black_box(2.0), black_box(5.0)))
    });

    group.finish();
}

// =============================================================================
// CRITERION GROUPS
// =============================================================================

criterion_group!(
    single_bond,
    bench_single_bond_pricing,
    bench_single_bond_no_curves,
);

criterion_group!(
    batch_pricing,
    bench_batch_pricing_sequential,
    bench_batch_pricing_parallel,
    bench_batch_comparison,
);

criterion_group!(etf_pricing, bench_etf_inav, bench_etf_batch,);

criterion_group!(
    portfolio,
    bench_portfolio_analytics,
    bench_duration_contribution,
    bench_portfolio_batch,
);

criterion_group!(curve_ops, bench_curve_interpolation,);

criterion_main!(single_bond, batch_pricing, etf_pricing, portfolio, curve_ops);
