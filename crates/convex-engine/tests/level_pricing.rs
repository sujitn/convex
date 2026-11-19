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

/// A rule's width widens what the rule quotes, at its first size bucket: a yield level
/// 3 bp wide is bid 1.5 bp above it and offered 1.5 bp below, each side priced as a mid
/// at that level is, and a price 1 point wide is bid and offered half a point either side.
/// A price sent for a level-quoted bond has no level to widen and is quoted mid only; a
/// bond quoted by price whose rule sets no width takes the configuration's width. A bond
/// that is not quoted two-way is refused under a rule with a width.
#[test]
fn a_rule_width_widens_what_the_rule_quotes() {
    use convex_engine::level_pricing::{BidAsk, PricingRuleInForce, TwoWay};
    let settlement = Date::parse(SETTLEMENT).unwrap();
    let discount = curve(settlement);
    let corporate = corporate("CORP", "2030-01-15", "0.05375");
    let note = government_note("SOV-10Y", "2035-01-15", "0.04125", "91282CBB6");
    let yields = BenchmarkYieldCache::default();
    let price = |in_force: &PricingRuleInForce, bond: &BondReferenceData, input| {
        price_under_rule(
            bond,
            Some(in_force),
            input,
            settlement,
            Some(&discount),
            &yields,
        )
    };
    let width = |unit: &str, first: &str| {
        serde_json::from_value(json!({"unit": unit, "buckets": [
            {"max_face": "1000000", "width": first},
            {"width": "9"},
        ]}))
        .unwrap()
    };
    let rule = |method: serde_json::Value, unit_width: Option<(&str, &str)>| {
        let mut in_force = rule_in_force(method, &[(note.clone(), "10Y")]);
        in_force.rule.benchmark_set_id = None;
        in_force.benchmark_set = None;
        in_force.width = unit_width.map(|(unit, first)| width(unit, first));
        in_force
    };
    let two_way = |pricing: convex_engine::level_pricing::RulePricing| -> TwoWay {
        match pricing.bid_ask {
            BidAsk::RuleWidth(two_way) => two_way,
            other => panic!("{other:?}"),
        }
    };

    let by_yield = rule(json!({"kind": "yield"}), Some(("basis_points", "3")));
    let quoted = two_way(price(&by_yield, &corporate, QuotedInput::Level(dec!(0.0612))).unwrap());
    assert_eq!(
        (quoted.width.value(), quoted.bid_level, quoted.ask_level),
        (dec!(3), dec!(0.06135), dec!(0.06105))
    );
    let mid_only = rule(json!({"kind": "yield"}), None);
    let mid_at = |level| {
        price(&mid_only, &corporate, QuotedInput::Level(level))
            .unwrap()
            .clean_price_mid
    };
    assert_eq!(quoted.clean_price_bid, mid_at(dec!(0.06135)));
    assert_eq!(quoted.clean_price_ask, mid_at(dec!(0.06105)));
    assert!(quoted.clean_price_bid < mid_at(dec!(0.0612)));
    for (in_force, input) in [
        (&by_yield, QuotedInput::Price(dec!(98))),
        (&mid_only, QuotedInput::Level(dec!(0.0612))),
    ] {
        assert_eq!(
            price(in_force, &corporate, input).unwrap().bid_ask,
            BidAsk::MidOnly
        );
    }

    let by_price = rule(json!({"kind": "price"}), Some(("price_points", "1")));
    let quoted = two_way(price(&by_price, &corporate, QuotedInput::Price(dec!(98))).unwrap());
    assert_eq!(
        (quoted.clean_price_bid, quoted.clean_price_ask),
        (dec!(97.5), dec!(98.5))
    );
    assert_eq!(
        price(
            &rule(json!({"kind": "price"}), None),
            &corporate,
            QuotedInput::Price(dec!(98))
        )
        .unwrap()
        .bid_ask,
        BidAsk::ConfigurationWidth
    );

    let mut callable = corporate.clone();
    callable.is_callable = true;
    let refused = price(&by_price, &callable, QuotedInput::Price(dec!(98))).err();
    assert_eq!(
        refused.map(|error| error.to_string()),
        Some(convex_engine::ports::level_pricing::WIDTH_NEEDS_TWO_WAY.to_owned())
    );
}

/// Each two-way replaces whatever bid, ask or width the router's input held, so a rule's
/// sides are never overridden by a configured width, nor stale sides kept.
#[test]
fn a_two_way_replaces_the_inputs_bid_ask() {
    use convex_core::types::{QuoteWidth, WidthUnit};
    use convex_engine::level_pricing::{BidAsk, TwoWay};
    use convex_engine::ports::storage::BidAskSpreadConfig;
    let settlement = Date::parse(SETTLEMENT).unwrap();
    let held = || {
        let mut input = PricingInput::with_mid_price(
            corporate("CORP", "2030-01-15", "0.05375"),
            settlement,
            Some(dec!(98)),
            None,
            None,
            None,
            None,
        );
        input.market_price_bid = Some(dec!(90));
        input.market_price_ask = Some(dec!(110));
        input.bid_ask_config = Some(BidAskSpreadConfig {
            prefer_market_spread: false,
            ..BidAskSpreadConfig::symmetric(50.0)
        });
        input
    };
    let configured = BidAskSpreadConfig::symmetric(2.0);
    let two_way = TwoWay {
        width: QuoteWidth::new(WidthUnit::PricePoints, dec!(1)).unwrap(),
        bid_level: dec!(97.5),
        ask_level: dec!(98.5),
        clean_price_bid: dec!(97.5),
        clean_price_ask: dec!(98.5),
    };
    for (bid_ask, bid, ask, width) in [
        (
            BidAsk::RuleWidth(two_way),
            Some(dec!(97.5)),
            Some(dec!(98.5)),
            None,
        ),
        (
            BidAsk::ConfigurationWidth,
            None,
            None,
            Some(configured.clone()),
        ),
        (BidAsk::MidOnly, None, None, None),
    ] {
        let mut input = held();
        bid_ask.apply(&mut input, Some(&configured));
        assert_eq!(
            (
                input.market_price_bid,
                input.market_price_ask,
                input.bid_ask_config
            ),
            (bid, ask, width),
            "{bid_ask:?}"
        );
    }
}
