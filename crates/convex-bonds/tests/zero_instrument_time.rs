//! Independent native original-date zero-coupon controls.
use convex_bonds::{
    instruments::{Compounding, ZeroCouponBond},
    pricing::{discount_time::DiscountTimeBasis, zero},
    types::CalendarId,
    BondError,
};
use convex_core::types::{Currency, Date};
use rust_decimal::{prelude::ToPrimitive, Decimal};
use serde_json::Value;

fn cases() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("fixtures/future-zero-instrument.json")).unwrap()
        ["cases"]
        .as_array()
        .unwrap()
        .clone()
}

fn decimal(value: &Value) -> Decimal {
    value.as_str().unwrap().parse().unwrap()
}

fn bond(row: &Value) -> ZeroCouponBond {
    ZeroCouponBond::builder()
        .cusip_unchecked("ZERO-TIME")
        .issue_date(Date::parse(row["issue_date"].as_str().unwrap()).unwrap())
        .maturity(Date::parse(row["maturity"].as_str().unwrap()).unwrap())
        .currency(Currency::USD)
        .calendar(CalendarId::weekend_only())
        .day_count(serde_json::from_value(row["day_count"].clone()).unwrap())
        .compounding(serde_json::from_value(row["compounding"].clone()).unwrap())
        .face_value(decimal(&row["face"]))
        .redemption_value(decimal(&row["redemption"]))
        .build()
        .unwrap()
}

fn classified(error: BondError, reference: &Value) {
    match reference.as_str().unwrap() {
        "YieldNonUnique" => assert!(matches!(error, BondError::YieldNonUnique), "{error:?}"),
        "YieldNoRoot" => assert!(matches!(error, BondError::YieldNoRoot), "{error:?}"),
        other => panic!("Undeclared structural root class {other}"),
    }
}

#[test]
fn strictly_future_zero_time_instrument_prices_keep_original_absolute_redemption() {
    let mut checked = 0;
    for row in cases() {
        let instrument = bond(&row);
        let settlement = Date::parse(row["settlement"].as_str().unwrap()).unwrap();
        let basis = DiscountTimeBasis::Calendar {
            day_count: instrument.day_count(),
        };
        for check in row["price_checks"].as_array().unwrap() {
            let rate = decimal(&check["annual_decimal_yield"]);
            let contextual = instrument
                .price_from_yield_with_basis(rate, settlement, &basis)
                .unwrap();
            let actual = if row["day_count"] == "Thirty360German" {
                assert!(instrument.price_from_yield(rate, settlement).is_err());
                Ok(contextual)
            } else {
                instrument.price_from_yield(rate, settlement)
            };
            assert_eq!(
                actual.unwrap_or_else(|error| panic!("{}: {error:?}", row["id"])),
                decimal(&check["exact_absolute_cash_price"]),
                "{}",
                row["id"]
            );
            assert_eq!(contextual, decimal(&check["exact_absolute_cash_price"]));
            checked += 1;
        }
    }
    assert_eq!(checked, 180);
}

#[test]
fn zero_time_instrument_inverses_return_exact_structural_classes() {
    let mut checked = 0;
    for row in cases() {
        let instrument = bond(&row);
        let settlement = Date::parse(row["settlement"].as_str().unwrap()).unwrap();
        let basis = DiscountTimeBasis::Calendar {
            day_count: instrument.day_count(),
        };
        for check in row["inverse_checks"].as_array().unwrap() {
            let price = decimal(&check["absolute_cash_price"]);
            let expected = &check["expected_native_classification"];
            classified(
                instrument
                    .yield_from_price_with_basis(price, settlement, &basis)
                    .unwrap_err(),
                expected,
            );
            classified(
                instrument
                    .continuous_rate_with_basis(price, settlement, &basis)
                    .unwrap_err(),
                expected,
            );
            if row["day_count"] == "Thirty360German" {
                assert!(instrument.yield_from_price(price, settlement).is_err());
                assert!(instrument.continuous_rate(price, settlement).is_err());
                checked += 2;
                continue;
            }
            classified(
                instrument.yield_from_price(price, settlement).unwrap_err(),
                &check["expected_native_classification"],
            );
            classified(
                instrument.continuous_rate(price, settlement).unwrap_err(),
                &check["expected_native_classification"],
            );
            checked += 2;
        }
    }
    assert_eq!(checked, 360);
}

#[test]
fn raw_undated_zero_coupon_helpers_keep_strictly_positive_time_contracts() {
    for compounding in [
        Compounding::Annual,
        Compounding::SemiAnnual,
        Compounding::Quarterly,
        Compounding::Monthly,
        Compounding::Continuous,
    ] {
        for time in [0.0, -0.0, -1.0, f64::NAN, f64::INFINITY, 150.0001] {
            assert!(
                zero::price_from_yield(Decimal::ONE_HUNDRED, Decimal::ZERO, time, compounding)
                    .is_err()
            );
            assert!(zero::yield_from_price(
                Decimal::ONE_HUNDRED,
                Decimal::ONE_HUNDRED,
                time,
                compounding
            )
            .is_err());
        }
    }
}

#[test]
fn complete_native_zero_contexts_price_invert_and_reprice_all_day_counts() {
    let pack: Value =
        serde_json::from_str(include_str!("fixtures/zero-instrument-context.json")).unwrap();
    let mut checked = 0;
    for row in pack["cases"].as_array().unwrap() {
        let instrument = bond(row);
        let settlement = Date::parse(row["settlement"].as_str().unwrap()).unwrap();
        let basis: DiscountTimeBasis = serde_json::from_value(row["basis"].clone()).unwrap();
        let price = instrument
            .price_from_yield_with_basis(decimal(&row["annual_decimal_yield"]), settlement, &basis)
            .unwrap();
        let target = decimal(&row["inverse_absolute_price"]);
        let rate = instrument
            .yield_from_price_with_basis(target, settlement, &basis)
            .unwrap();
        let continuous = instrument
            .continuous_rate_with_basis(target, settlement, &basis)
            .unwrap();
        for (actual, reference, bound) in [
            (price, "expected_absolute_price", "price_absolute_bound"),
            (rate, "expected_annual_yield", "yield_absolute_bound"),
            (
                continuous,
                "expected_continuous_rate",
                "yield_absolute_bound",
            ),
        ] {
            let expected = row[reference].as_str().unwrap().parse::<f64>().unwrap();
            let bound = row[bound].as_str().unwrap().parse::<f64>().unwrap();
            assert!(
                (actual.to_f64().unwrap() - expected).abs() <= bound,
                "{} {reference} {actual}",
                row["id"]
            );
        }
        let repriced = instrument
            .price_from_yield_with_basis(rate, settlement, &basis)
            .unwrap();
        assert!((repriced - target).abs() <= decimal(&row["residual_absolute_bound"]));
        let contextual_only = matches!(
            row["day_count"].as_str().unwrap(),
            "ActActAfb" | "Thirty360German" | "Thirty360EIsda" | "ActActIcma" | "Act365Leap"
        );
        if contextual_only {
            assert!(instrument
                .price_from_yield(decimal(&row["annual_decimal_yield"]), settlement)
                .is_err());
        } else {
            assert_eq!(
                instrument
                    .price_from_yield(decimal(&row["annual_decimal_yield"]), settlement)
                    .unwrap(),
                price
            );
        }
        let wrong = DiscountTimeBasis::Calendar {
            day_count: if instrument.day_count()
                == convex_core::daycounts::DayCountConvention::Act360
            {
                convex_core::daycounts::DayCountConvention::Act365Fixed
            } else {
                convex_core::daycounts::DayCountConvention::Act360
            },
        };
        assert!(instrument
            .price_from_yield_with_basis(Decimal::ZERO, settlement, &wrong)
            .is_err());
        assert!(instrument
            .yield_from_price_with_basis(target, settlement, &wrong)
            .is_err());
        assert!(instrument
            .continuous_rate_with_basis(target, settlement, &wrong)
            .is_err());
        checked += 1;
    }
    assert_eq!(checked, 150);
}

#[test]
fn zero_instrument_requires_complete_context_and_valid_original_domains() {
    let pack: Value =
        serde_json::from_str(include_str!("fixtures/zero-instrument-context.json")).unwrap();
    for row in pack["cases"].as_array().unwrap().iter().step_by(15) {
        let instrument = bond(row);
        let settlement = Date::parse(row["settlement"].as_str().unwrap()).unwrap();
        let mut basis: DiscountTimeBasis = serde_json::from_value(row["basis"].clone()).unwrap();
        match &mut basis {
            DiscountTimeBasis::Icma {
                reference_periods, ..
            } => {
                reference_periods.pop();
            }
            DiscountTimeBasis::Act365L { coupon_periods, .. } => {
                coupon_periods.pop();
            }
            DiscountTimeBasis::Thirty360EIsda { termination_date } => {
                *termination_date = termination_date.add_days(1);
            }
            _ => continue,
        }
        assert!(instrument
            .price_from_yield_with_basis(Decimal::ZERO, settlement, &basis)
            .is_err());
        assert!(instrument
            .yield_from_price_with_basis(Decimal::ONE_HUNDRED, settlement, &basis)
            .is_err());
        assert!(instrument
            .continuous_rate_with_basis(Decimal::ONE_HUNDRED, settlement, &basis)
            .is_err());
        let absent = DiscountTimeBasis::Calendar {
            day_count: instrument.day_count(),
        };
        assert!(instrument
            .price_from_yield_with_basis(Decimal::ZERO, settlement, &absent)
            .is_err());
    }
    for row in cases()
        .iter()
        .step_by(4)
        .filter(|r| r["day_count"] == "Thirty360US")
    {
        let instrument = bond(row);
        let settlement = Date::parse(row["settlement"].as_str().unwrap()).unwrap();
        if let Some(frequency) = instrument.compounding().periods_per_year_opt() {
            for rate in [
                -Decimal::from(frequency),
                -Decimal::from(frequency) - Decimal::ONE,
            ] {
                assert!(instrument.price_from_yield(rate, settlement).is_err());
            }
        }
        for target in [Decimal::ZERO, -Decimal::ONE] {
            assert!(matches!(
                instrument.yield_from_price(target, settlement),
                Err(BondError::InvalidSpec { .. })
            ));
            assert!(matches!(
                instrument.continuous_rate(target, settlement),
                Err(BondError::InvalidSpec { .. })
            ));
        }
        for invalid in [
            Date::parse(row["issue_date"].as_str().unwrap())
                .unwrap()
                .add_days(-1),
            Date::parse(row["maturity"].as_str().unwrap()).unwrap(),
        ] {
            assert!(instrument.price_from_yield(Decimal::ZERO, invalid).is_err());
            assert!(instrument
                .yield_from_price(decimal(&row["redemption"]), invalid)
                .is_err());
        }
    }
}
