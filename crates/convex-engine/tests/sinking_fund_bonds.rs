//! A sinking-fund bond with mandatory sinks against the independent reference
//! `tests/scripts/sinking_fund_quote_reference.py`: a price per 100 current face is valued
//! per 100 original face at the bond's factor, on its sinking cash, and its G-spread reads
//! the benchmark set at its average life when its rule says so. Admission refuses a
//! schedule that disagrees with the bond's flag, or that the library cannot hold exactly.

#[path = "support/level_rules.rs"]
mod level_rules;
#[path = "support/products.rs"]
mod products;

use rust_decimal::{prelude::ToPrimitive, Decimal};
use serde_json::{json, Value};

use convex_analytics::bond_quote::BenchmarkTenor;
use convex_core::Date;
use convex_engine::level_pricing::{price_under_rule, BenchmarkYieldCache, QuotedInput};
use convex_engine::ports::level_pricing::TenorBasis;
use convex_engine::ports::reference_data::{BondReferenceData, BondType};
use convex_engine::ports::reference_master::ScheduledSink;
use convex_engine::ports::reference_repository::{validate_bond_reference, ReferenceError};
use convex_engine::ports::storage::BidAskSpreadConfig;
use convex_engine::pricing_router::{PricingInput, PricingRouter};
use level_rules::{corporate, curve, government_note, rule_in_force, BENCHMARK_CLEAN_PRICE};

fn reference() -> Value {
    serde_json::from_str(include_str!("fixtures/sinking-fund-quotes.json")).unwrap()
}

fn decimal(value: &Value) -> Decimal {
    value.as_str().unwrap().parse().unwrap()
}

fn date(value: &Value) -> Date {
    Date::parse(value.as_str().unwrap()).unwrap()
}

/// The reference's bond.
fn sinker() -> BondReferenceData {
    products::sinker(corporate("SINKER", "2032-03-15", "0.06"))
}

fn sinks(bond: &mut BondReferenceData) -> &mut Vec<ScheduledSink> {
    &mut bond.contractual_terms.as_mut().unwrap().sinking_schedule
}

/// A sink at par.
fn sink(day: &str, amount: &str) -> ScheduledSink {
    serde_json::from_value(json!({"date": day, "amount": amount})).unwrap()
}

/// The router's quote of `bond` at `settlement` from a clean price per 100 original face.
fn router_quote(
    bond: &BondReferenceData,
    settlement: Date,
    clean_price_mid: Decimal,
) -> convex_engine::ports::output::BondQuoteOutput {
    let input = PricingInput::with_mid_price(
        bond.clone(),
        settlement,
        Some(clean_price_mid),
        Some(curve(settlement)),
        None,
        None,
        None,
    );
    PricingRouter::new().price(&input).unwrap()
}

/// Each price per 100 current face is valued per 100 original face at the reference's
/// factor, mid only, and the router's accrued, yield and factor are the reference's: before
/// any sink, the day before a sink and on it, after a sink above par, and after one
/// between coupon dates, with accrued cut at it.
#[test]
fn a_price_per_current_face_is_valued_on_the_sinking_cash() {
    let bond = sinker();
    let yields = BenchmarkYieldCache::default();
    for case in reference()["quotes"].as_array().unwrap() {
        let settlement = date(&case["settlement"]);
        let quoted = decimal(&case["clean_price_per_current_face"]);
        let pricing = price_under_rule(
            &bond,
            None,
            QuotedInput::Price(quoted),
            settlement,
            None,
            &yields,
        )
        .unwrap();
        assert_eq!(
            pricing.clean_price_mid,
            decimal(&case["clean_price_mid"]),
            "{case}"
        );
        assert_eq!(pricing.factor, Some(decimal(&case["factor"])), "{case}");
        assert_eq!(
            pricing.per_current_face(pricing.clean_price_mid).unwrap(),
            quoted
        );
        // Quoted mid only: the configuration's width is not applied to it.
        let mut input = PricingInput::with_mid_price(
            bond.clone(),
            settlement,
            Some(pricing.clean_price_mid),
            None,
            None,
            None,
            None,
        );
        pricing.bid_ask.apply(
            &mut input,
            Some(&BidAskSpreadConfig {
                half_spread_bps: 5.0,
                ..Default::default()
            }),
        );
        assert!(input.bid_ask_config.is_none(), "{case}");
        let quote = router_quote(&bond, settlement, pricing.clean_price_mid);
        assert_eq!(quote.factor, Some(decimal(&case["factor"])), "{case}");
        assert_eq!(quote.clean_price_mid, Some(pricing.clean_price_mid));
        assert!(quote.clean_price_bid.is_none() && quote.clean_price_ask.is_none());
        let accrued = quote.accrued_interest.unwrap() - decimal(&case["accrued_interest"]);
        assert!(accrued.abs() < Decimal::new(1, 12), "{case}: {accrued}");
        let ytm = quote.ytm_mid.unwrap() - decimal(&case["ytm_mid"]);
        assert!(ytm.abs() < Decimal::new(1, 10), "{case}: {ytm}");
    }
}

/// A yield rule prices the reference's yield back to its clean price per original face.
#[test]
fn a_yield_prices_the_sinking_cash_back() {
    let bond = sinker();
    let mut in_force = rule_in_force(json!({"kind": "yield"}), &[]);
    in_force.benchmark_set = None;
    in_force.rule.benchmark_set_id = None;
    for case in reference()["quotes"].as_array().unwrap() {
        let pricing = price_under_rule(
            &bond,
            Some(&in_force),
            QuotedInput::Level(decimal(&case["ytm_mid"])),
            date(&case["settlement"]),
            None,
            &BenchmarkYieldCache::default(),
        )
        .unwrap();
        let clean = pricing.clean_price_mid - decimal(&case["clean_price_mid"]);
        assert!(clean.abs() < Decimal::new(1, 8), "{case}: {clean}");
    }
}

/// Under a rule over a set with an average-life tenor basis, the G-spread is the bond's
/// yield less the set's yield interpolated, on a 365-day axis, at the reference's average
/// life, not at maturity, in whole basis points as the library reports it.
#[test]
fn the_g_spread_reads_the_set_at_the_average_life() {
    let bond = sinker();
    let case = &reference()["quotes"][0];
    let settlement = date(&case["settlement"]);
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
    let note_yield = |note: &BondReferenceData| {
        router_quote(note, settlement, BENCHMARK_CLEAN_PRICE)
            .ytm_mid
            .unwrap()
            .to_f64()
            .unwrap()
    };
    let years =
        |note: &BondReferenceData| settlement.days_between(&note.maturity_date) as f64 / 365.0;
    let (short, long) = (&notes[0].0, &notes[1].0);
    let at = |time: f64| {
        note_yield(short)
            + (note_yield(long) - note_yield(short)) * (time - years(short))
                / (years(long) - years(short))
    };
    let g_spread = |basis: TenorBasis| {
        let mut in_force = rule_in_force(json!({"kind": "price"}), &notes);
        in_force.rule.tenor_basis = basis;
        let pricing = price_under_rule(
            &bond,
            Some(&in_force),
            QuotedInput::Price(decimal(&case["clean_price_per_current_face"])),
            settlement,
            None,
            &BenchmarkYieldCache::default(),
        )
        .unwrap();
        let set = pricing.g_spread_set.unwrap();
        assert_eq!(
            matches!(set.tenor, BenchmarkTenor::AverageLife(_)),
            basis == TenorBasis::AverageLife
        );
        let input = PricingInput::with_mid_price(
            bond.clone(),
            settlement,
            Some(pricing.clean_price_mid),
            Some(curve(settlement)),
            None,
            Some(set),
            None,
        );
        PricingRouter::new()
            .price(&input)
            .unwrap()
            .g_spread_mid
            .unwrap()
            .to_f64()
            .unwrap()
    };
    let ytm = decimal(&case["ytm_mid"]).to_f64().unwrap();
    let average_life = decimal(&case["average_life_years"]).to_f64().unwrap();
    // The library reports a G-spread in whole basis points.
    let by_average_life = g_spread(TenorBasis::AverageLife);
    let expected = ((ytm - at(average_life)) * 10_000.0).round();
    assert_eq!(by_average_life, expected);
    let by_maturity = g_spread(TenorBasis::Maturity);
    let expected = ((ytm - at(years(&bond))) * 10_000.0).round();
    assert_eq!(by_maturity, expected);
    assert!((by_maturity - by_average_life).abs() > 1.0);
}

/// Against a set whose longest security matures after the bond's average life but before
/// its maturity, the G-spread is published at the average life and refused at maturity,
/// saying why.
#[test]
fn the_g_spread_is_published_where_its_read_point_lies_within_the_set() {
    let bond = sinker();
    let case = &reference()["quotes"][0];
    let notes = [
        (
            government_note("UST-2Y", "2028-01-15", "0.035", "91282CAA9"),
            "2Y",
        ),
        (
            government_note("UST-5Y", "2031-07-15", "0.0375", "91282CAC5"),
            "5Y",
        ),
    ];
    let priced = |basis: TenorBasis| {
        let mut in_force = rule_in_force(json!({"kind": "price"}), &notes);
        in_force.rule.tenor_basis = basis;
        price_under_rule(
            &bond,
            Some(&in_force),
            QuotedInput::Price(decimal(&case["clean_price_per_current_face"])),
            date(&case["settlement"]),
            None,
            &BenchmarkYieldCache::default(),
        )
        .unwrap()
    };
    let by_average_life = priced(TenorBasis::AverageLife);
    assert!(by_average_life.g_spread_set.is_some());
    assert!(by_average_life.g_spread_refused.is_none());
    let by_maturity = priced(TenorBasis::Maturity);
    assert!(by_maturity.g_spread_set.is_none());
    assert_eq!(
        by_maturity.g_spread_refused,
        Some("the bond matures outside its benchmark set's maturities")
    );
}

/// Admission holds the schedule to the bond: a sinkable flag and a schedule go together,
/// on an amortizing bond with nothing else embedded; sinks fall after issue and by
/// maturity in date order, each exact in the library's binary schedule, and retire at most
/// the original face, all of it only with a sink on maturity.
#[test]
fn admission_refuses_a_schedule_that_does_not_fit_the_bond() {
    let bond = sinker();
    validate_bond_reference(&bond).unwrap();
    let refused = |change: &dyn Fn(&mut BondReferenceData)| {
        let mut bond = sinker();
        change(&mut bond);
        validate_bond_reference(&bond).unwrap_err()
    };
    assert!(matches!(
        refused(&|bond| bond.is_sinkable = false),
        ReferenceError::InvalidInput(reason) if reason.contains("sinkable exactly")
    ));
    assert!(matches!(
        refused(&|bond| sinks(bond).clear()),
        ReferenceError::InvalidInput(reason) if reason.contains("sinkable exactly")
    ));
    for change in [
        &(|bond: &mut BondReferenceData| bond.bond_type = BondType::FixedBullet)
            as &dyn Fn(&mut BondReferenceData),
        &|bond| bond.is_putable = true,
    ] {
        assert!(matches!(
            refused(change),
            ReferenceError::InvalidInput(reason) if reason.contains("amortizing fixed-coupon")
        ));
    }
    assert!(matches!(
        refused(&|bond| {
            let first = sinks(bond)[0].clone();
            sinks(bond).resize(convex_engine::ports::reference_master::MAX_SINKS + 1, first);
        }),
        ReferenceError::InvalidInput(reason) if reason.contains("1,024")
    ));
    for change in [
        // Not exact in the library's binary schedule.
        &(|bond: &mut BondReferenceData| {
            sinks(bond)[0] = sink("2025-03-15", "10.00000000000000001")
        }) as &dyn Fn(&mut BondReferenceData),
        &|bond| sinks(bond).swap(0, 1),
        &|bond| sinks(bond)[0] = sink("2020-03-15", "10"),
        &|bond| sinks(bond)[4] = sink("2032-03-16", "20"),
        &|bond| sinks(bond)[0] = sink("2025-03-15", "0"),
        &|bond| sinks(bond)[0].price = serde_json::from_value(json!("1000.01")).unwrap(),
        // 110 retired.
        &|bond| sinks(bond)[4] = sink("2028-03-15", "70"),
        // All the face retired before maturity.
        &|bond| sinks(bond)[4] = sink("2028-03-15", "62.7"),
    ] {
        assert!(matches!(refused(change), ReferenceError::InvalidInput(_)));
    }
    // Fractional amounts and prices of up to 15 significant digits are held exactly;
    // 17 are not.
    for (amount, price) in [
        ("3.33", "100"),
        ("14.285714", "100.125"),
        ("1.23456789012345", "99.9999999999999"),
    ] {
        let mut fractional = sinker();
        sinks(&mut fractional)[0] =
            serde_json::from_value(json!({"date": "2025-03-15", "amount": amount, "price": price}))
                .unwrap();
        validate_bond_reference(&fractional).unwrap();
    }
    assert!(matches!(
        refused(&|bond| sinks(bond)[0] = sink("2025-03-15", "1.2345678901234567")),
        ReferenceError::InvalidInput(_)
    ));
    // All of it retired by a sink on maturity is a schedule, and is priced: its last sink
    // retires the rest at its own price.
    let mut on_maturity = sinker();
    sinks(&mut on_maturity).push(sink("2032-03-15", "42.7"));
    validate_bond_reference(&on_maturity).unwrap();
    let settlement = Date::parse("2031-06-10").unwrap();
    let quote = router_quote(&on_maturity, settlement, "40".parse().unwrap());
    assert_eq!(quote.factor, Some("0.427".parse().unwrap()));
    // A sinkable flag on a bullet with no schedule is refused, as before it was stored.
    let mut flagged = corporate("FLAGGED", "2030-01-15", "0.05");
    flagged.is_sinkable = true;
    assert!(validate_bond_reference(&flagged).is_err());
}
