//! Original native high-frequency precision failures, before kernel migration.
use convex_bonds::pricing::reinvestment;
use convex_core::types::Date;
use rust_decimal::Decimal;
use serde_json::Value;

fn decimal(value: &Value) -> Decimal {
    Decimal::from_str_exact(value.as_str().unwrap()).unwrap()
}

fn check(index: usize) {
    let references: Value =
        serde_json::from_str(include_str!("fixtures/periodic-growth.json")).unwrap();
    let row = &references["growth_cases"][index];
    let actual = reinvestment::periodic_growth(
        decimal(&row["annual_rate"]),
        row["frequency"].as_u64().unwrap() as u32,
        Date::parse(row["start"].as_str().unwrap()).unwrap(),
        Date::parse(row["end"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    let expected = decimal(&row["comparison"]);
    let bound = decimal(&row["native_absolute_bound"]);
    assert!(
        (actual - expected).abs() <= bound,
        "{}: {actual} versus {expected}",
        row["id"]
    );
}
#[test]
fn large_frequency_cannot_amplify_the_rounded_unit_base() {
    check(0);
}
#[test]
fn representable_growth_must_not_be_rejected_when_the_base_rounds_to_one() {
    check(1);
}

#[test]
fn materially_rounded_tiny_growth_cannot_multiply_actual_cash() {
    let result = reinvestment::periodic_growth(
        Decimal::from_str_exact("-0.9999999999999999999999999998").unwrap(),
        1,
        Date::parse("2024-02-29").unwrap(),
        Date::parse("2025-03-01").unwrap(),
    );
    assert!(result.is_err());
}

#[test]
fn complete_declared_growth_grid_matches_independent_decimal110_values() {
    let references: Value =
        serde_json::from_str(include_str!("fixtures/periodic-growth.json")).unwrap();
    assert_eq!(references["growth_cases"].as_array().unwrap().len(), 383);
    for index in 0..383 {
        check(index);
    }
}

#[test]
fn distinct_rate_account_projections_retain_original_cash_units_and_bounds() {
    use convex_bonds::pricing::reinvestment::{ProjectionCash, ProjectionTerms};
    let references: Value =
        serde_json::from_str(include_str!("fixtures/periodic-growth.json")).unwrap();
    let rows = references["projection_cases"].as_array().unwrap();
    assert_eq!(rows.len(), 32);
    for row in rows {
        let terms: ProjectionTerms = serde_json::from_value(row["terms"].clone()).unwrap();
        let cash: Vec<ProjectionCash> = serde_json::from_value(row["cash"].clone()).unwrap();
        let result = reinvestment::project(&cash, &terms).unwrap();
        assert_eq!(result.horizon.to_string(), row["horizon"].as_str().unwrap());
        for (key, value) in [
            ("residual_balance", result.residual_balance),
            ("reinvested_cash", result.reinvested_cash),
            ("horizon_value", result.horizon_value),
            ("annualized_return", result.annualized_return),
        ] {
            let bound = decimal(
                &row[if key == "annualized_return" {
                    "native_rate_absolute_bound"
                } else {
                    "native_cash_absolute_bound"
                }],
            );
            assert!(
                (value.decimal() - decimal(&row["comparison"][key])).abs() <= bound,
                "{} {key}",
                row["id"]
            );
        }
    }
}
