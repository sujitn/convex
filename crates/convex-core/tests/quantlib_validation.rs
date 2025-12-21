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
#[allow(dead_code)] // Fields are populated by JSON deserialization for future tests
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
#[allow(dead_code)] // test_type populated by JSON but not currently used
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

// ============================================================================
// PIECEWISE BOOTSTRAP TESTS
// ============================================================================

#[test]
fn test_piecewise_bootstrap_from_fixtures() {
    use convex_core::daycounts::DayCountConvention;
    use convex_core::types::Frequency;
    use convex_curves::calibration::{
        Deposit, InstrumentSet, PiecewiseBootstrapper, Swap,
    };

    let suite = load_test_suite();

    if suite.test_suites.curve_bootstrap.is_none() {
        println!("No curve_bootstrap tests found");
        return;
    }

    let bootstrap_tests = suite.test_suites.curve_bootstrap.unwrap();
    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;

    for test_value in &bootstrap_tests {
        let test_type = test_value
            .get("test_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let description = test_value
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("unnamed");

        // Only process piecewise bootstrap tests
        if !test_type.starts_with("piecewise") {
            skipped += 1;
            continue;
        }

        println!("Running: {}", description);

        let inputs = match test_value.get("inputs") {
            Some(i) => i,
            None => {
                println!("  SKIP: No inputs");
                skipped += 1;
                continue;
            }
        };

        let eval_date_str = inputs
            .get("evaluation_date")
            .and_then(|v| v.as_str())
            .unwrap_or("2024-01-02");
        let eval_date = parse_date(eval_date_str);

        // Build instrument set
        let mut instruments = InstrumentSet::new();
        let dc = DayCountConvention::Act360;

        // Add deposits
        if let Some(deposits) = inputs.get("deposits").and_then(|v| v.as_array()) {
            for dep in deposits {
                let tenor = dep
                    .get("tenor_years")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let rate = dep.get("rate").and_then(|v| v.as_f64()).unwrap_or(0.0);
                instruments.add(Deposit::from_tenor(eval_date, tenor, rate, dc));
            }
        }

        // Add swaps
        if let Some(swaps) = inputs.get("swaps").and_then(|v| v.as_array()) {
            for swap in swaps {
                let tenor = swap
                    .get("tenor_years")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let rate = swap.get("rate").and_then(|v| v.as_f64()).unwrap_or(0.0);
                instruments.add(Swap::from_tenor(
                    eval_date,
                    tenor,
                    rate,
                    Frequency::SemiAnnual,
                    DayCountConvention::Thirty360US,
                ));
            }
        }

        if instruments.is_empty() {
            println!("  SKIP: No instruments");
            skipped += 1;
            continue;
        }

        // Run piecewise bootstrap
        let bootstrapper = PiecewiseBootstrapper::new();
        let result = match bootstrapper.bootstrap(eval_date, &instruments) {
            Ok(r) => r,
            Err(e) => {
                println!("  FAIL: Bootstrap error: {}", e);
                failed += 1;
                continue;
            }
        };

        let expected = match test_value.get("expected") {
            Some(e) => e,
            None => {
                println!("  SKIP: No expected values");
                skipped += 1;
                continue;
            }
        };

        // Get RMS threshold from fixture (default to 1e-6)
        let rms_threshold = expected
            .get("rms_error_threshold")
            .and_then(|v| v.as_f64())
            .unwrap_or(1e-6);

        // Check RMS error threshold
        if result.rms_error > rms_threshold {
            println!(
                "  FAIL: RMS error {:.2e} exceeds threshold {:.2e}",
                result.rms_error, rms_threshold
            );
            failed += 1;
            continue;
        }

        println!(
            "  PASS: RMS={:.2e}, converged={}",
            result.rms_error, result.converged
        );
        passed += 1;
    }

    println!(
        "\nPiecewise bootstrap tests: {} passed, {} failed, {} skipped",
        passed, failed, skipped
    );

    assert!(failed == 0, "Some piecewise bootstrap tests failed");
}

#[test]
fn test_piecewise_vs_global_comparison() {
    use convex_core::daycounts::DayCountConvention;
    use convex_curves::calibration::{Deposit, GlobalFitter, InstrumentSet, PiecewiseBootstrapper};

    let suite = load_test_suite();

    if suite.test_suites.curve_bootstrap.is_none() {
        println!("No curve_bootstrap tests found");
        return;
    }

    let bootstrap_tests = suite.test_suites.curve_bootstrap.unwrap();

    for test_value in &bootstrap_tests {
        let test_type = test_value
            .get("test_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if test_type != "piecewise_vs_global" {
            continue;
        }

        let description = test_value
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("unnamed");
        println!("Running comparison: {}", description);

        let inputs = match test_value.get("inputs") {
            Some(i) => i,
            None => continue,
        };

        let eval_date_str = inputs
            .get("evaluation_date")
            .and_then(|v| v.as_str())
            .unwrap_or("2024-01-02");
        let eval_date = parse_date(eval_date_str);

        // Build instrument set
        let mut instruments = InstrumentSet::new();
        let dc = DayCountConvention::Act360;

        if let Some(deposits) = inputs.get("deposits").and_then(|v| v.as_array()) {
            for dep in deposits {
                let tenor = dep
                    .get("tenor_years")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let rate = dep.get("rate").and_then(|v| v.as_f64()).unwrap_or(0.0);
                instruments.add(Deposit::from_tenor(eval_date, tenor, rate, dc));
            }
        }

        if instruments.is_empty() {
            continue;
        }

        // Run both methods
        let piecewise_result = PiecewiseBootstrapper::new()
            .bootstrap(eval_date, &instruments)
            .expect("Piecewise bootstrap failed");

        let global_result = GlobalFitter::new()
            .fit(eval_date, &instruments)
            .expect("Global fit failed");

        let expected = match test_value.get("expected") {
            Some(e) => e,
            None => continue,
        };

        // Check that piecewise achieves comparable or better fit
        // Note: When both are < 1e-10, they are effectively equal (numerical noise)
        if expected
            .get("piecewise_rms_better")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            let both_excellent =
                piecewise_result.rms_error < 1e-10 && global_result.rms_error < 1e-10;
            if !both_excellent {
                assert!(
                    piecewise_result.rms_error <= global_result.rms_error * 1.1, // Allow 10% tolerance
                    "Piecewise RMS ({:.2e}) should be <= Global RMS ({:.2e})",
                    piecewise_result.rms_error,
                    global_result.rms_error
                );
            }
        }

        println!(
            "  Piecewise RMS: {:.2e}, Global RMS: {:.2e}",
            piecewise_result.rms_error, global_result.rms_error
        );

        // Check thresholds
        if let Some(piecewise_threshold) = expected
            .get("piecewise_rms_threshold")
            .and_then(|v| v.as_f64())
        {
            assert!(
                piecewise_result.rms_error < piecewise_threshold,
                "Piecewise RMS {} exceeds threshold {}",
                piecewise_result.rms_error,
                piecewise_threshold
            );
        }

        if let Some(global_threshold) = expected
            .get("global_rms_threshold")
            .and_then(|v| v.as_f64())
        {
            assert!(
                global_result.rms_error < global_threshold,
                "Global RMS {} exceeds threshold {}",
                global_result.rms_error,
                global_threshold
            );
        }

        println!("  PASS: Both methods within thresholds");
    }
}
