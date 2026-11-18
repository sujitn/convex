//! A two-way quote priced through `price_from_level`: the bid is the lower price and the
//! ask the higher, whether the level is a price, a yield or a Z-spread.

use std::str::FromStr;

use convex_analytics::bond_quote::{price_from_level, BondCurves};
use convex_bonds::instruments::FixedRateBond;
use convex_core::{
    daycounts::DayCountConvention,
    types::{Date, Frequency, Mark, SpreadType, WidthSchedule},
};
use convex_curves::{DiscreteCurve, InterpolationMethod, RateCurve, ValueType};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::Value;

fn date(text: &str) -> Date {
    Date::parse(text).unwrap()
}

#[test]
fn a_bid_prices_below_its_mid_and_an_ask_above() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/quote-widths.json")).unwrap();
    let settlement = date("2026-11-20");
    let bond = FixedRateBond::builder()
        .cusip_unchecked("QUOTE-WIDTHS")
        .coupon_rate(dec!(0.05))
        .frequency(Frequency::SemiAnnual)
        .issue_date(date("2024-01-15"))
        .maturity(date("2034-01-15"))
        .day_count(DayCountConvention::Thirty360US)
        .build()
        .unwrap();
    let discount = RateCurve::new(
        DiscreteCurve::new(
            settlement,
            vec![0.01, 1.0, 5.0, 10.0],
            vec![0.04; 4],
            ValueType::continuous_zero(DayCountConvention::Act365Fixed),
            InterpolationMethod::Linear,
        )
        .unwrap(),
    );
    let curves = BondCurves {
        discount: Some(&discount),
        ..Default::default()
    };
    let clean = |mark: &Mark| {
        price_from_level(&bond, settlement, mark, Frequency::SemiAnnual, &curves)
            .unwrap()
            .clean_price_per_100
    };

    let mut priced = 0;
    for case in fixture["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let mid = Mark::from_str(case["mid"].as_str().unwrap()).unwrap();
        // A G-spread needs government securities; the core fixture test covers its levels.
        if matches!(&mid, Mark::Spread { value, .. } if value.spread_type() != SpreadType::ZSpread)
        {
            continue;
        }
        let schedule: WidthSchedule = serde_json::from_value(case["schedule"].clone()).unwrap();
        let face = case["face"]
            .as_str()
            .map(|face| Decimal::from_str_exact(face).unwrap());
        let width = schedule.width_for(face).unwrap();
        let two_way = width.two_way(&mid).unwrap();
        let (bid, mid, ask) = (clean(&two_way.bid), clean(&mid), clean(&two_way.ask));
        if width.value().is_zero() {
            assert!(bid == mid && mid == ask, "{id}: {bid} {mid} {ask}");
        } else {
            assert!(bid < mid && mid < ask, "{id}: {bid} {mid} {ask}");
        }
        priced += 1;
    }
    assert_eq!(priced, 11);
}
