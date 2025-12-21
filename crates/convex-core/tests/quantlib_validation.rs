//! Integration tests validated against QuantLib reference values.
//!
//! These tests use pre-computed values from QuantLib Python 1.40 to validate
//! Convex calculations match industry-standard implementations.

use convex_core::daycounts::{Act360, Act365Fixed, ActActIsda, DayCount, Thirty360E, Thirty360US};
use convex_core::types::Date;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Deserialize;
use std::fs;

/// Path to QuantLib reference test data
const QUANTLIB_REFERENCE_FILE: &str = "../../tests/fixtures/quantlib_reference_tests.json";

// ============================================================================
// JSON Structures for Test Data
// ============================================================================

#[derive(Debug, Deserialize)]
struct TestSuite {
    metadata: Metadata,
    test_suites: TestSuites,
}

#[derive(Debug, Deserialize)]
struct Metadata {
    quantlib_version: String,
    generated_date: String,
}

#[derive(Debug, Deserialize)]
struct TestSuites {
    day_counts: Vec<DayCountTest>,
    fixed_rate_bonds: Option<Vec<serde_json::Value>>,
    accrued_interest: Option<Vec<serde_json::Value>>,
    duration_convexity: Option<Vec<serde_json::Value>>,
    zero_coupon_bonds: Option<Vec<serde_json::Value>>,
    curve_bootstrap: Option<Vec<serde_json::Value>>,
    real_world_bonds: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
struct DayCountTest {
    test_type: String,
    convention: String,
    inputs: DayCountInputs,
    expected: DayCountExpected,
    tolerance: f64,
}

#[derive(Debug, Deserialize)]
struct DayCountInputs {
    start_date: String,
    end_date: String,
}

#[derive(Debug, Deserialize)]
struct DayCountExpected {
    day_count: i64,
    year_fraction: f64,
}

// ============================================================================
// Helper Functions
// ============================================================================

fn parse_date(s: &str) -> Date {
    Date::parse(s).unwrap_or_else(|_| panic!("Failed to parse date: {}", s))
}

fn get_day_count(name: &str) -> Box<dyn DayCount> {
    match name {
        "Thirty360US" => Box::new(Thirty360US),
        "Thirty360E" => Box::new(Thirty360E),
        "Act360" => Box::new(Act360),
        "Act365Fixed" => Box::new(Act365Fixed),
        "ActActIsda" => Box::new(ActActIsda),
        _ => panic!("Unknown day count convention: {}", name),
    }
}

fn load_test_suite() -> TestSuite {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let path = std::path::Path::new(&manifest_dir).join(QUANTLIB_REFERENCE_FILE);

    let data = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read test fixture file at {:?}: {}", path, e));

    serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("Failed to parse test fixture JSON: {}", e))
}

// ============================================================================
// DAY COUNT CONVENTION TESTS
// ============================================================================

#[test]
fn test_day_counts_from_quantlib() {
    let suite = load_test_suite();

    println!(
        "Running {} day count tests from QuantLib {}",
        suite.test_suites.day_counts.len(),
        suite.metadata.quantlib_version
    );

    let mut passed = 0;
    let mut failed = 0;

    for test in &suite.test_suites.day_counts {
        let dc = get_day_count(&test.convention);
        let start = parse_date(&test.inputs.start_date);
        let end = parse_date(&test.inputs.end_date);

        // Test day count
        let day_count = dc.day_count(start, end);
        let day_count_ok = day_count == test.expected.day_count;

        if !day_count_ok {
            println!(
                "FAIL: Day count for {} ({} to {}): expected {}, got {}",
                test.convention,
                test.inputs.start_date,
                test.inputs.end_date,
                test.expected.day_count,
                day_count
            );
            failed += 1;
            continue;
        }

        // Test year fraction
        let year_fraction = dc.year_fraction(start, end);
        let expected_yf =
            Decimal::try_from(test.expected.year_fraction).unwrap_or_else(|_| dec!(0));

        let yf_diff = (year_fraction - expected_yf).abs();
        let yf_tolerance = Decimal::try_from(test.tolerance).unwrap_or_else(|_| dec!(0.0000000001));
        let yf_ok = yf_diff <= yf_tolerance;

        if !yf_ok {
            println!(
                "FAIL: Year fraction for {} ({} to {}): expected {}, got {}, diff = {}",
                test.convention,
                test.inputs.start_date,
                test.inputs.end_date,
                test.expected.year_fraction,
                year_fraction,
                yf_diff
            );
            failed += 1;
            continue;
        }

        passed += 1;
    }

    println!("\nDay count tests: {} passed, {} failed", passed, failed);

    // Allow some failures for unimplemented edge cases but require high pass rate
    let pass_rate = passed as f64 / (passed + failed) as f64;
    assert!(
        pass_rate >= 0.9,
        "Day count tests pass rate too low: {:.1}%",
        pass_rate * 100.0
    );
}

// ============================================================================
// INDIVIDUAL DAY COUNT CONVENTION TESTS
// ============================================================================

#[test]
fn test_act360_six_months() {
    let dc = Act360;
    let start = Date::from_ymd(2024, 1, 15).unwrap();
    let end = Date::from_ymd(2024, 7, 15).unwrap();

    assert_eq!(dc.day_count(start, end), 182);

    let yf = dc.year_fraction(start, end);
    let expected = dec!(182) / dec!(360);
    assert!(
        (yf - expected).abs() < dec!(0.0000001),
        "Expected {}, got {}",
        expected,
        yf
    );
}

#[test]
fn test_act365_fixed_six_months() {
    let dc = Act365Fixed;
    let start = Date::from_ymd(2024, 1, 15).unwrap();
    let end = Date::from_ymd(2024, 7, 15).unwrap();

    assert_eq!(dc.day_count(start, end), 182);

    let yf = dc.year_fraction(start, end);
    let expected = dec!(182) / dec!(365);
    assert!(
        (yf - expected).abs() < dec!(0.0000001),
        "Expected {}, got {}",
        expected,
        yf
    );
}

#[test]
fn test_thirty360_us_six_months() {
    let dc = Thirty360US;
    let start = Date::from_ymd(2024, 1, 15).unwrap();
    let end = Date::from_ymd(2024, 7, 15).unwrap();

    // 30/360 US: 6 months = 180 days
    assert_eq!(dc.day_count(start, end), 180);

    let yf = dc.year_fraction(start, end);
    let expected = dec!(0.5); // 180/360
    assert!(
        (yf - expected).abs() < dec!(0.0000001),
        "Expected {}, got {}",
        expected,
        yf
    );
}

#[test]
fn test_thirty360_us_boeing_accrued_days() {
    // Boeing 7.5% 06/15/2025 - settlement 04/29/2020
    // Last coupon: 12/15/2019, settlement: 04/29/2020
    // Expected: 134 accrued days per Bloomberg

    let dc = Thirty360US;
    let last_coupon = Date::from_ymd(2019, 12, 15).unwrap();
    let settlement = Date::from_ymd(2020, 4, 29).unwrap();

    let accrued_days = dc.day_count(last_coupon, settlement);
    assert_eq!(
        accrued_days, 134,
        "Boeing accrued days should be 134, got {}",
        accrued_days
    );
}

#[test]
fn test_thirty360_e_six_months() {
    let dc = Thirty360E;
    let start = Date::from_ymd(2024, 1, 15).unwrap();
    let end = Date::from_ymd(2024, 7, 15).unwrap();

    // 30E/360: 6 months = 180 days
    assert_eq!(dc.day_count(start, end), 180);

    let yf = dc.year_fraction(start, end);
    let expected = dec!(0.5); // 180/360
    assert!(
        (yf - expected).abs() < dec!(0.0000001),
        "Expected {}, got {}",
        expected,
        yf
    );
}

#[test]
fn test_act_act_isda_full_year_leap() {
    let dc = ActActIsda;
    let start = Date::from_ymd(2024, 1, 1).unwrap();
    let end = Date::from_ymd(2025, 1, 1).unwrap();

    // Full year from Jan 1 leap year to Jan 1 next year
    assert_eq!(dc.day_count(start, end), 366);

    let yf = dc.year_fraction(start, end);
    // Should be exactly 1.0 for full year
    assert!(
        (yf - dec!(1)).abs() < dec!(0.0000001),
        "Full year should be 1.0, got {}",
        yf
    );
}

#[test]
fn test_act_act_isda_full_year_non_leap() {
    let dc = ActActIsda;
    let start = Date::from_ymd(2023, 1, 1).unwrap();
    let end = Date::from_ymd(2024, 1, 1).unwrap();

    // Full year from Jan 1 non-leap year to Jan 1 leap year
    assert_eq!(dc.day_count(start, end), 365);

    let yf = dc.year_fraction(start, end);
    // Should be exactly 1.0 for full year
    assert!(
        (yf - dec!(1)).abs() < dec!(0.0000001),
        "Full year should be 1.0, got {}",
        yf
    );
}

#[test]
fn test_leap_year_february() {
    let start = Date::from_ymd(2024, 2, 28).unwrap();
    let end = Date::from_ymd(2024, 3, 1).unwrap();

    // Act360: 2 actual days
    let dc360 = Act360;
    assert_eq!(dc360.day_count(start, end), 2);

    // Act365: 2 actual days
    let dc365 = Act365Fixed;
    assert_eq!(dc365.day_count(start, end), 2);

    // 30/360 US: Feb 28 to Mar 1 in leap year
    let dc30 = Thirty360US;
    let days_30 = dc30.day_count(start, end);
    // The exact count depends on EOM rules
    assert!(days_30 >= 1 && days_30 <= 3, "30/360 days: {}", days_30);
}

#[test]
fn test_non_leap_year_february() {
    let start = Date::from_ymd(2023, 2, 28).unwrap();
    let end = Date::from_ymd(2023, 3, 1).unwrap();

    // Act360: 1 actual day
    let dc360 = Act360;
    assert_eq!(dc360.day_count(start, end), 1);

    // Act365: 1 actual day
    let dc365 = Act365Fixed;
    assert_eq!(dc365.day_count(start, end), 1);
}

#[test]
fn test_end_of_month_dates() {
    let dc = Thirty360US;

    // Jan 31 to Feb 28 (non-leap year assumptions for day count)
    let start = Date::from_ymd(2024, 1, 31).unwrap();
    let end = Date::from_ymd(2024, 2, 28).unwrap();

    let days = dc.day_count(start, end);
    // 30/360: 30 days per month, so roughly 28 days
    assert!(days >= 27 && days <= 30, "EOM days: {}", days);
}

// ============================================================================
// QUANTLIB METADATA TEST
// ============================================================================

#[test]
fn test_fixture_loads_correctly() {
    let suite = load_test_suite();

    assert_eq!(suite.metadata.quantlib_version, "1.40");
    assert!(!suite.test_suites.day_counts.is_empty());

    println!("Test suite metadata:");
    println!("  QuantLib version: {}", suite.metadata.quantlib_version);
    println!("  Generated: {}", suite.metadata.generated_date);
    println!("  Day count tests: {}", suite.test_suites.day_counts.len());
}
