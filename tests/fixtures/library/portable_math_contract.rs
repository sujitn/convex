//! Independent primitive controls from exact binary64 bits and Decimal math.
use convex_math::portable;
use serde_json::Value;

fn bits(value: &Value) -> u64 {
    value.as_str().unwrap().parse().unwrap()
}

pub fn verify() -> usize {
    let document: Value =
        serde_json::from_str(include_str!("portable-math-reference.json")).unwrap();
    assert_eq!(document["certified"], false);
    let rows = document["cases"].as_array().unwrap();
    for row in rows {
        let x = f64::from_bits(bits(&row["x_bits"]));
        let y = || f64::from_bits(bits(&row["y_bits"]));
        let actual = match row["operation"].as_str().unwrap() {
            "exp" => portable::exp(x),
            "log" => portable::log(x),
            "log1p" => portable::log1p(x),
            "expm1" => portable::expm1(x),
            "pow" => portable::pow(x, y()),
            "hypot" => portable::hypot(x, y()),
            other => panic!("undeclared operation {other}"),
        };
        let expected = f64::from_bits(bits(&row["expected_bits"]));
        assert_eq!(
            actual.is_sign_negative(),
            expected.is_sign_negative(),
            "{}: sign",
            row["id"]
        );
        assert_eq!(
            actual.is_finite(),
            expected.is_finite(),
            "{}: finite",
            row["id"]
        );
        if !expected.is_finite() || expected == 0.0 {
            assert_eq!(
                actual.to_bits(),
                expected.to_bits(),
                "{}: exact boundary",
                row["id"]
            );
        } else {
            assert!(actual != 0.0, "{}: nonzero tail", row["id"]);
            assert!(
                actual.to_bits().abs_diff(expected.to_bits()) <= row["max_ulps"].as_u64().unwrap(),
                "{}: {actual:.17e} vs {expected:.17e}",
                row["id"]
            );
        }
    }
    assert!(portable::log(-1.0).is_nan());
    assert!(portable::pow(-1.0, 0.5).is_nan());
    assert_eq!(portable::log(0.0), f64::NEG_INFINITY);
    assert_eq!(portable::log1p(-1.0), f64::NEG_INFINITY);
    assert_eq!(portable::expm1(-0.0).to_bits(), (-0.0_f64).to_bits());
    assert_eq!(portable::log1p(-0.0).to_bits(), (-0.0_f64).to_bits());
    rows.len()
}
