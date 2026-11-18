//! The level stage against a benchmark that pays annually: a benchmark spread is added to
//! the benchmark's yield compounded semi-annually, and that is the yield reported.

#[path = "support/level_rules.rs"]
mod level_rules;

use rust_decimal::{prelude::ToPrimitive, Decimal};
use rust_decimal_macros::dec;
use serde_json::json;

use convex_core::Date;
use convex_engine::curve_builder::BuiltCurve;
use convex_engine::level_pricing::{price_under_rule, BenchmarkYieldCache, QuotedInput};
use convex_engine::ports::reference_data::BondReferenceData;
use convex_engine::pricing_router::{PricingInput, PricingRouter};
use level_rules::{
    corporate, curve, government_note, rule_in_force, BENCHMARK_CLEAN_PRICE, SETTLEMENT,
};

/// `bond`'s street yield at its own frequency, from `clean_price_mid`.
fn street_yield(
    bond: &BondReferenceData,
    clean_price_mid: Decimal,
    settlement: Date,
    discount: &BuiltCurve,
) -> f64 {
    let input = PricingInput::with_mid_price(
        bond.clone(),
        settlement,
        Some(clean_price_mid),
        Some(discount.clone()),
        None,
        None,
        None,
    );
    PricingRouter::new()
        .price(&input)
        .unwrap()
        .ytm_mid
        .unwrap()
        .to_f64()
        .unwrap()
}

#[test]
fn a_benchmark_spread_over_an_annual_pay_benchmark_is_over_its_semi_annual_yield() {
    let settlement = Date::parse(SETTLEMENT).unwrap();
    let discount = curve(settlement);
    let corporate = corporate("CORP", "2030-01-15", "0.05375");
    let mut note = government_note("SOV-10Y", "2035-01-15", "0.04125", "91282CBB6");
    note.frequency = 1;
    let annual = street_yield(&note, BENCHMARK_CLEAN_PRICE, settlement, &discount);
    let semi_annual = 2.0 * ((1.0 + annual).sqrt() - 1.0);

    let in_force = rule_in_force(
        json!({"kind": "benchmark_spread", "tenor": "10Y"}),
        &[(note, "10Y")],
    );
    let pricing = price_under_rule(
        &corporate,
        Some(&in_force),
        QuotedInput::Level(dec!(125)),
        settlement,
        Some(&discount),
        &BenchmarkYieldCache::default(),
    )
    .unwrap();
    let reported = pricing.benchmark.unwrap().ytm;
    assert!(
        (annual - semi_annual).abs() > 1e-5,
        "{annual} {semi_annual}"
    );
    assert!(
        (reported - semi_annual).abs() < 1e-9,
        "{reported} {semi_annual}"
    );

    // The corporate pays semi-annually, so its street yield is the reported yield plus
    // the spread.
    let corporate_yield = street_yield(&corporate, pricing.clean_price_mid, settlement, &discount);
    assert!(
        (corporate_yield - (reported + 0.0125)).abs() < 1e-9,
        "{corporate_yield} {reported}"
    );
}
