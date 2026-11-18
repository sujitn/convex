//! The level stage alone: one bond priced from its level, for each method, beside
//! the router's price path it feeds. A quote from a level costs the stage plus the router;
//! from a price, the router alone. `stage` solves each benchmark's yield, as the first
//! bond a benchmark price reaches does; `stage_shared` and `stage_and_router` reuse the
//! yields already solved, as every later bond does.
//!
//! Run with: cargo bench -p convex-engine --bench level_benchmarks

#[path = "../tests/support/level_rules.rs"]
mod level_rules;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rust_decimal_macros::dec;
use serde_json::json;

use convex_core::Date;
use convex_engine::level_pricing::{price_under_rule, BenchmarkYieldCache, QuotedInput};
use convex_engine::pricing_router::{PricingInput, PricingRouter};
use level_rules::{corporate, curve, government_note, rule_in_force, SETTLEMENT};

fn bench_level_stage(c: &mut Criterion) {
    let settlement = Date::parse(SETTLEMENT).unwrap();
    let discount = curve(settlement);
    let corporate = corporate("CORP", "2030-01-15", "0.05375");
    let notes = [
        (
            government_note("UST-2Y", "2028-01-15", "0.035", "91282CAA9"),
            "2Y",
        ),
        (
            government_note("UST-10Y", "2035-01-15", "0.04125", "91282CBB6"),
            "10Y",
        ),
    ];
    let router = PricingRouter::new();
    let mut group = c.benchmark_group("level_stage");
    for (name, method, level) in [
        ("yield", json!({"kind": "yield"}), dec!(0.0612)),
        ("z_spread", json!({"kind": "z_spread"}), dec!(150)),
        (
            "benchmark_spread",
            json!({"kind": "benchmark_spread", "tenor": "10Y"}),
            dec!(125),
        ),
    ] {
        let in_force = rule_in_force(method, &notes);
        group.bench_function(format!("stage/{name}"), |b| {
            b.iter(|| {
                price_under_rule(
                    black_box(&corporate),
                    Some(&in_force),
                    QuotedInput::Level(level),
                    settlement,
                    Some(&discount),
                    &BenchmarkYieldCache::default(),
                )
                .unwrap()
            })
        });
        let shared = BenchmarkYieldCache::default();
        group.bench_function(format!("stage_shared/{name}"), |b| {
            b.iter(|| {
                price_under_rule(
                    black_box(&corporate),
                    Some(&in_force),
                    QuotedInput::Level(level),
                    settlement,
                    Some(&discount),
                    &shared,
                )
                .unwrap()
            })
        });
        group.bench_function(format!("stage_and_router/{name}"), |b| {
            b.iter(|| {
                let mut pricing = price_under_rule(
                    black_box(&corporate),
                    Some(&in_force),
                    QuotedInput::Level(level),
                    settlement,
                    Some(&discount),
                    &shared,
                )
                .unwrap();
                router
                    .price(&PricingInput::with_mid_price(
                        corporate.clone(),
                        settlement,
                        Some(pricing.clean_price_mid),
                        Some(discount.clone()),
                        None,
                        pricing.government_curve.take(),
                        None,
                    ))
                    .unwrap()
            })
        });
    }
    group.bench_function("router_from_price", |b| {
        b.iter(|| {
            router
                .price(&PricingInput::with_mid_price(
                    black_box(corporate.clone()),
                    settlement,
                    Some(dec!(99.5)),
                    Some(discount.clone()),
                    None,
                    None,
                    None,
                ))
                .unwrap()
        })
    });
    group.finish();
}

criterion_group!(level, bench_level_stage);
criterion_main!(level);
