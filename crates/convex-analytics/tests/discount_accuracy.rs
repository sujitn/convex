//! Actual native reproductions using the independently declared original values.
use convex_bonds::pricing::{
    discount_time::{prepare_dated_cash, DatedYieldCash, DiscountTimeBasis},
    yield_cash::{price_cash, CashYieldCompounding},
};
use convex_core::types::Date;
use serde_json::Value;

fn original_case(index: usize) {
    let root: Value =
        serde_json::from_str(include_str!("fixtures/discount-accuracy-reproduction.json")).unwrap();
    let row = &root["cases"][index];
    let action = &row["request"]["operation"]["action"];
    let cash = if action["kind"] == "price" {
        serde_json::from_value::<Vec<(f64, f64)>>(action["cash"].clone()).unwrap()
    } else {
        let cash: Vec<DatedYieldCash> = serde_json::from_value(action["cash"].clone()).unwrap();
        let basis: DiscountTimeBasis = serde_json::from_value(action["basis"].clone()).unwrap();
        let settlement = Date::parse(action["settlement"].as_str().unwrap()).unwrap();
        prepare_dated_cash(&cash, settlement, &basis).unwrap()
    };
    let compounding: CashYieldCompounding =
        serde_json::from_value(action["compounding"].clone()).unwrap();
    let actual = price_cash(&cash, action["annual_yield"].as_f64().unwrap(), compounding).unwrap();
    let expected = row["independent_price"]
        .as_str()
        .unwrap()
        .parse::<f64>()
        .unwrap();
    assert!(
        (actual - expected).abs() <= 1e-10,
        "{}: actual {actual:.17}, independently expected {expected:.17}",
        row["id"]
    );
}
#[test]
fn full_frequency_original_price_loss() {
    original_case(0);
}
#[test]
fn small_rate_original_unit_discount_loss() {
    original_case(1);
}
#[test]
fn afb_original_backward_february_anniversary() {
    original_case(2);
}
#[test]
fn afb_original_february29_start() {
    original_case(3);
}
#[test]
fn afb_original_february29_end() {
    original_case(4);
}

#[test]
fn independent_full_frequency_and_afb_grid() {
    use convex_core::daycounts::{ActActAfb, DayCount};
    use rust_decimal::Decimal;
    let root: Value =
        serde_json::from_str(include_str!("fixtures/discount-accuracy.json")).unwrap();
    for row in root["cases"].as_array().unwrap() {
        let action = &row["request"]["operation"]["action"];
        let cash = if action["kind"] == "price" {
            serde_json::from_value::<Vec<(f64, f64)>>(action["cash"].clone()).unwrap()
        } else {
            let cash: Vec<DatedYieldCash> = serde_json::from_value(action["cash"].clone()).unwrap();
            let basis: DiscountTimeBasis = serde_json::from_value(action["basis"].clone()).unwrap();
            let settlement = Date::parse(action["settlement"].as_str().unwrap()).unwrap();
            let prepared = prepare_dated_cash(&cash, settlement, &basis).unwrap();
            let expected = row["independent_time"]
                .as_str()
                .unwrap()
                .parse::<f64>()
                .unwrap();
            assert!(
                (prepared[0].0 - expected).abs() <= 1e-14,
                "{} time {:?}",
                row["id"],
                prepared
            );
            prepared
        };
        let compounding: CashYieldCompounding =
            serde_json::from_value(action["compounding"].clone()).unwrap();
        let actual =
            price_cash(&cash, action["annual_yield"].as_f64().unwrap(), compounding).unwrap();
        let expected = row["independent_price"]
            .as_str()
            .unwrap()
            .parse::<f64>()
            .unwrap();
        assert!(
            (actual - expected).abs() <= 1e-10,
            "{}: actual {actual:.17}, independently expected {expected:.17}",
            row["id"]
        );
    }
    for row in root["raw_afb"].as_array().unwrap() {
        let start = Date::parse(row["start"].as_str().unwrap()).unwrap();
        let end = Date::parse(row["end"].as_str().unwrap()).unwrap();
        let actual = ActActAfb.year_fraction(start, end);
        let original = row["independent_time"].as_str().unwrap();
        // The API returns Decimal. Retain 24 fractional reference digits and
        // reserve the entire truncation error INSIDE the original time bound.
        // This avoids comparing an unrelated binary64 conversion at 9998 years.
        let end = original
            .find('.')
            .map_or(original.len(), |point| (point + 25).min(original.len()));
        let expected: Decimal = original[..end].parse().unwrap();
        let reserve = if end < original.len() {
            Decimal::new(1, 24)
        } else {
            Decimal::ZERO
        };
        let bound = match row["absolute_time_bound"].as_str().unwrap() {
            "0" => Decimal::ZERO,
            "1e-14" => Decimal::new(1, 14),
            other => panic!("unexpected original time bound {other}"),
        };
        assert!(
            (actual - expected).abs() + reserve <= bound,
            "{} / {}: actual native Decimal {actual}, original {original}",
            row["start"],
            row["end"]
        );
    }
}

#[test]
fn original_domain_and_resource_guards_stay_checked() {
    use CashYieldCompounding::{Annual, Continuous, Periodic};
    for frequency in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(price_cash(&[(1.0, 100.0)], 0.05, Periodic { frequency }).is_err());
    }
    for cash in [
        vec![],
        vec![(0.0, 100.0)],
        vec![(-1.0, 100.0)],
        vec![(1.0, 0.0)],
        vec![(1.0, 100.0); 4097],
    ] {
        assert!(price_cash(&cash, 0.05, Annual).is_err());
    }
    assert!(price_cash(&[(1.0, 100.0)], -2.0, Periodic { frequency: 2.0 }).is_err());
    assert!(price_cash(&[(1.0, 100.0)], -1.0, Annual).is_err());
    assert!(price_cash(&[(1.0, 100.0)], 1000.0, Continuous).is_err());
    assert!(price_cash(
        &[(1.0, 100.0)],
        f64::MIN_POSITIVE,
        Periodic {
            frequency: f64::MAX
        }
    )
    .is_err());
    assert!(price_cash(&[(1.0, f64::MAX), (1.0, f64::MAX)], 0.0, Annual).is_err());
}
