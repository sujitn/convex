//! Original-face normalization for actual per100 current-yield quotations.
use convex_bonds::instruments::FixedRateBond;
use convex_core::types::Date;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn bond(face: Decimal) -> FixedRateBond {
    FixedRateBond::builder()
        .cusip_unchecked("SUPPLIED.SCALAR.BOND")
        .coupon_rate(dec!(0.05))
        .face_value(face)
        .issue_date(Date::from_ymd(2020, 1, 1).unwrap())
        .maturity(Date::from_ymd(2030, 1, 1).unwrap())
        .build()
        .unwrap()
}
#[test]
fn original_face_cannot_change_native_per100_current_yield() {
    for face in [dec!(100), dec!(5000), dec!(1000.625)] {
        let actual =
            convex_bonds::pricing::current_yield_from_bond(&bond(face), dec!(100)).unwrap();
        assert!(
            (actual - 0.05).abs() <= 1e-14,
            "face={face} actual={actual}"
        );
    }
}
#[test]
fn original_face_cannot_change_analytics_per100_current_yield_percent() {
    for face in [dec!(100), dec!(5000), dec!(1000.625)] {
        let actual =
            convex_analytics::yields::current_yield_from_bond(&bond(face), dec!(100)).unwrap();
        assert_eq!(actual, dec!(5), "face={face}");
    }
}

fn scalar_fixture() -> serde_json::Value {
    serde_json::from_str(include_str!("fixtures/yield-scalars.json")).unwrap()
}
#[test]
fn original_scalar_apis_match_all_independent_rational_units_and_bounds() {
    use convex_analytics::yields as a;
    use convex_bonds::pricing::{self as b, yield_scalars as k};
    use serde_json::{json, Value};
    let fixture = scalar_fixture();
    assert_eq!(fixture["cases"].as_array().unwrap().len(), 126);
    for row in fixture["cases"].as_array().unwrap() {
        let op = &row["operation"];
        let f = |name: &str| op[name].as_f64().unwrap();
        let d = |name: &str| op[name].as_str().unwrap().parse::<Decimal>().unwrap();
        let value = match op["kind"].as_str().unwrap() {
            "current_binary64" => {
                let value = b::current_yield_simple(f("annual_coupon"), f("clean_price")).unwrap();
                assert_eq!(
                    value,
                    a::current_yield_simple(f("annual_coupon"), f("clean_price")).unwrap()
                );
                json!(value)
            }
            "simple_binary64" => {
                let value =
                    b::simple_yield(f("annual_coupon"), f("clean_price"), f("years_to_maturity"))
                        .unwrap();
                assert_eq!(
                    value,
                    a::simple_yield_f64(
                        f("annual_coupon"),
                        f("clean_price"),
                        f("years_to_maturity")
                    )
                    .unwrap()
                );
                json!(value)
            }
            "current_decimal_rate" => {
                let value = b::current_yield(d("annual_coupon"), d("clean_price")).unwrap();
                assert_eq!(
                    value,
                    a::current_yield_decimal(d("annual_coupon"), d("clean_price")).unwrap()
                );
                json!(value)
            }
            "current_percent_from_rate" => {
                json!(a::current_yield(d("annual_rate"), d("clean_price_per100"))
                    .unwrap()
                    .to_string())
            }
            "current_percent_from_amount" => json!(a::current_yield_from_amount(
                d("annual_coupon_per100"),
                d("clean_price_per100")
            )
            .unwrap()
            .to_string()),
            "simple_decimal_percent" => json!(a::simple_yield(
                d("annual_coupon"),
                d("clean_price"),
                d("par_value"),
                d("years_to_maturity")
            )
            .unwrap()
            .to_string()),
            "fixed_current_decimal_rate" => json!(k::fixed_current_decimal_rate(
                d("annual_rate"),
                d("original_face"),
                d("clean_price_per100")
            )
            .unwrap()),
            "fixed_current_percent" => json!(k::fixed_current_percent(
                d("annual_rate"),
                d("original_face"),
                d("clean_price_per100")
            )
            .unwrap()
            .to_string()),
            "report_percent" | "report_decimal" => {
                // Probe reporting of an explicitly supplied scalar; these are not
                // claims that a market yield solver produced this test record.
                let basic = b::YieldResult {
                    yield_value: f("yield_value"),
                    iterations: 1,
                    residual: 0.,
                };
                let engine = b::YieldEngineResult {
                    yield_value: f("yield_value"),
                    iterations: 1,
                    residual: 0.,
                    convention: convex_bonds::types::YieldCalculationRules::us_treasury()
                        .convention,
                };
                let analytics: a::YieldEngineResult = engine;
                if op["kind"] == "report_percent" {
                    let value = basic.yield_percent().unwrap();
                    assert_eq!(value, engine.yield_percent().unwrap());
                    assert_eq!(value, analytics.yield_percent().unwrap());
                    json!(value)
                } else {
                    let value = basic.yield_decimal().unwrap();
                    assert_eq!(value, engine.yield_decimal().unwrap());
                    assert_eq!(value, analytics.yield_decimal().unwrap());
                    json!(value.to_string())
                }
            }
            kind => panic!("undeclared scalar kind: {kind}"),
        };
        if row["representation"] == "decimal" {
            let parse = |v: &Value| v.as_str().unwrap().parse::<Decimal>().unwrap();
            assert!(
                (parse(&value) - parse(&row["expected_decimal"])).abs()
                    <= parse(&row["absolute_bound"]),
                "{}: {value}",
                row["id"]
            );
        } else {
            let reference = row["expected"].as_str().unwrap().parse::<f64>().unwrap();
            let bound = row["absolute_bound"]
                .as_str()
                .unwrap()
                .parse::<f64>()
                .unwrap();
            assert!(
                (value.as_f64().unwrap() - reference).abs() <= bound,
                "{}: {value}",
                row["id"]
            );
        }
    }
}
#[test]
fn invalid_scalar_reporting_and_nonzero_underflow_cannot_become_zero() {
    use convex_bonds::pricing::{self as b, yield_scalars as k};
    for rate in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let value = b::YieldResult {
            yield_value: rate,
            iterations: 1,
            residual: 0.,
        };
        assert!(value.yield_percent().is_err());
        assert!(value.yield_decimal().is_err());
        assert!(b::current_yield_simple(rate, 100.).is_err());
        assert!(b::current_yield_simple(5., rate).is_err());
        assert!(b::simple_yield(5., 100., rate).is_err());
    }
    for rate in [
        1e-100,
        -1e-100,
        f64::from_bits(1),
        -f64::from_bits(1),
        f64::MAX,
    ] {
        assert!(k::to_decimal(rate).is_err());
    }
    assert!(k::to_percent(f64::MAX).is_err());
    assert!(b::current_yield_simple(1e-308, 1e308).is_err());
    assert!(b::current_yield_simple(1e308, 1e-308).is_err());
    assert!(b::current_yield(dec!(0.0000000000000000000000000001), Decimal::MAX).is_err());
    assert_eq!(b::current_yield(Decimal::ZERO, Decimal::MAX).unwrap(), 0.);
    assert_eq!(k::to_decimal(0.).unwrap(), Decimal::ZERO);
    assert!(k::simple_decimal_percent(Decimal::MAX, dec!(1), dec!(100), dec!(1)).is_err());
    for face in [Decimal::ZERO, dec!(-1)] {
        assert!(k::fixed_current_decimal_rate(dec!(0.05), face, dec!(100)).is_err());
        assert!(k::fixed_current_percent(dec!(0.05), face, dec!(100)).is_err());
    }
    assert_eq!(
        k::fixed_current_percent(dec!(-0.01), dec!(5000), dec!(100)).unwrap(),
        dec!(-1)
    );
}
