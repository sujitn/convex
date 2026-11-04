//! Independent predeclared cash/yield controls for complete fixed workouts.
use convex_analytics::workflow::*;
use convex_bonds::contracts::{BondCalendars, BondContract, BondTerms};
use convex_core::{calendars::Calendar, Date};
use rust_decimal::prelude::ToPrimitive;
use serde_json::Value;

struct SuppliedCalendar {
    holidays: Vec<Date>,
}
impl Calendar for SuppliedCalendar {
    fn name(&self) -> &'static str {
        "IndependentWorkoutCalendar"
    }
    fn is_business_day(&self, date: Date) -> bool {
        date.weekday().num_days_from_monday() < 5 && !self.holidays.contains(&date)
    }
}
fn reference() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/library/workout-reference.json"
    ))
    .unwrap()
}
fn calendar(r: &Value) -> SuppliedCalendar {
    SuppliedCalendar {
        holidays: serde_json::from_value(r["calendars"][0]["holidays"].clone()).unwrap(),
    }
}
fn contract(c: &Value, cal: &SuppliedCalendar) -> BondContract {
    let terms: BondTerms = serde_json::from_value(c["bond"]["terms"].clone()).unwrap();
    terms
        .build_with_calendars(&BondCalendars {
            payment: cal,
            reset: cal,
            observation: cal,
            exercise: cal,
        })
        .unwrap()
}
fn input_context(c: &Value) -> WorkoutContext {
    let mut value = c["context"].clone();
    value["trade_date"] = serde_json::json!("2026-05-04");
    serde_json::from_value(value).unwrap()
}
fn settings() -> BondInverseSettings {
    BondInverseSettings {
        lower: -0.01,
        upper: 0.25,
        price_tolerance: 1e-8,
        parameter_tolerance: 1e-12,
        max_iterations: 96,
    }
}
fn controls() -> WorkoutSolveSettings {
    WorkoutSolveSettings {
        inverse: settings(),
        max_dates: 64,
        max_cash_evaluations: 1_000_000,
    }
}
fn close(actual: f64, expected: f64, bound: f64, label: &str) {
    assert!(
        (actual - expected).abs() <= bound,
        "{label}: {actual} != {expected}"
    );
}
fn numeric(v: &Value) -> f64 {
    v.as_str().unwrap().parse().unwrap()
}
fn cash(value: &BondValue, c: &Value) {
    let mut actual: Vec<_> = value
        .cash
        .cashflows()
        .unwrap()
        .into_iter()
        .map(|f| {
            (
                f.date.to_string(),
                if f.is_coupon() { "coupon" } else { "principal" },
                f.amount.to_f64().unwrap(),
            )
        })
        .collect();
    actual.sort_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)));
    let expected = c["expected"]["cash"].as_array().unwrap();
    assert_eq!(actual.len(), expected.len(), "{}", c["id"]);
    for ((date, kind, amount), r) in actual.iter().zip(expected) {
        assert_eq!(date, r["date"].as_str().unwrap());
        assert_eq!(*kind, r["kind"].as_str().unwrap());
        close(*amount, numeric(&r["amount"]), 1e-10, "original cash");
    }
}
#[test]
fn predeclared_full_steps_principal_notice_cash_and_yield_controls() {
    let r = reference();
    let cal = calendar(&r);
    for c in r["cases"].as_array().unwrap() {
        let bond = contract(c, &cal);
        let context: WorkoutContext = input_context(c);
        let choice: WorkoutSelection = serde_json::from_value(c["selection"].clone()).unwrap();
        let convention = serde_json::from_value(c["convention"].clone()).unwrap();
        let input = WorkoutInputs {
            contract: &bond,
            context: &context,
            exercise_calendar: (&bond.terms().calendars.exercise, &cal),
            purchase_calendar: (&bond.terms().calendars.payment, &cal),
        };
        let q =
            quote_workout_price(&input, &choice, convention, numeric(&c["decimal_yield"])).unwrap();
        assert_eq!(serde_json::to_value(&q.source).unwrap(), c["bond"]["terms"]);
        let WorkoutValue::Price(price) = q.quotation else {
            panic!("wrong quote")
        };
        cash(&price, c);
        close(
            price.dirty_per100,
            numeric(&c["dirty_per100"]),
            1e-8,
            c["id"].as_str().unwrap(),
        );
        if let Some(ex) = q.exercise {
            assert_eq!(
                serde_json::to_value(ex.dates).unwrap(),
                c["expected"]["dates"]
            );
            close(
                ex.outstanding_principal.to_f64().unwrap(),
                numeric(&c["expected"]["outstanding"]),
                1e-10,
                "remaining principal",
            );
            close(
                ex.redemption.to_f64().unwrap(),
                numeric(&c["expected"]["redemption"]),
                1e-10,
                "raw redemption",
            );
            close(
                ex.accrued_cash.to_f64().unwrap(),
                numeric(&c["expected"]["accrued_cash"]),
                1e-10,
                "partial coupon",
            );
            close(
                ex.settlement_cash.to_f64().unwrap(),
                numeric(&c["expected"]["settlement_cash"]),
                1e-10,
                "rounded settlement cash",
            );
        }
        let q = quote_workout_yield(
            &input,
            &choice,
            convention,
            BondPriceTarget::Dirty(numeric(&c["dirty_per100"])),
            settings(),
        )
        .unwrap();
        let WorkoutValue::Yield(inverse) = q.quotation else {
            panic!("wrong inverse")
        };
        close(
            inverse.decimal_parameter,
            numeric(&c["decimal_yield"]),
            1e-10,
            "independent yield",
        );
        assert!(inverse.native_residual.abs() <= settings().price_tolerance);
        assert!(inverse.price_evaluations > 0);
        cash(&inverse.valuation, c);
        let clean = quote_workout_yield(
            &input,
            &choice,
            convention,
            BondPriceTarget::Clean(price.clean_per100),
            settings(),
        )
        .unwrap();
        let WorkoutValue::Yield(clean) = clean.quotation else {
            panic!("wrong inverse")
        };
        close(
            clean.decimal_parameter,
            inverse.decimal_parameter,
            1e-10,
            "original accrued clean conversion",
        );
        assert!(
            PreparedBondCash::fixed(&bond, context.settlement, context.on_payment).is_err(),
            "hypothetical yield must not enable optional market pricing"
        );
    }
}
#[test]
fn every_actual_continuous_date_survives_and_put_cannot_lower_call_worst() {
    let r = reference();
    let c = &r["all_dates"];
    let cal = calendar(&r);
    let bond = contract(c, &cal);
    let context = input_context(c);
    let convention = serde_json::from_value(c["convention"].clone()).unwrap();
    let input = WorkoutInputs {
        contract: &bond,
        context: &context,
        exercise_calendar: (&bond.terms().calendars.exercise, &cal),
        purchase_calendar: (&bond.terms().calendars.payment, &cal),
    };
    let q = quote_workout_worst(
        &input,
        convention,
        BondPriceTarget::Dirty(numeric(&c["dirty_per100"])),
        controls(),
    )
    .unwrap();
    assert_eq!(q.quotes.len(), 13);
    assert_eq!(q.examined_dates, 14);
    assert_eq!(
        q.selected_index,
        c["selected_index"].as_u64().unwrap() as usize
    );
    for (actual, expected) in q.quotes.iter().zip(c["quotes"].as_array().unwrap()) {
        assert_eq!(
            serde_json::to_value(&actual.selection).unwrap(),
            expected["selection"]
        );
        close(
            actual.decimal_yield,
            numeric(&expected["decimal_yield"]),
            1e-10,
            "all-date independent yield",
        );
        assert!(actual.native_residual.abs() <= settings().price_tolerance);
        if let Some(ex) = &actual.exercise {
            assert_eq!(
                serde_json::to_value(&ex.dates).unwrap(),
                expected["expected"]["dates"]
            );
        }
    }
    assert!(
        q.quotes[2].decimal_yield < q.quotes[q.selected_index].decimal_yield,
        "lower holder put remains separately reported"
    );
    assert!(
        q.cash_discount_evaluations <= q.reserved_cash_evaluations
            && q.reserved_cash_evaluations <= controls().max_cash_evaluations
    );
    let exact = WorkoutSolveSettings {
        max_dates: 14,
        max_cash_evaluations: q.reserved_cash_evaluations,
        ..controls()
    };
    assert!(quote_workout_worst(&input, convention, q.target, exact).is_ok());
    for reduced in [
        WorkoutSolveSettings {
            max_dates: 13,
            ..exact
        },
        WorkoutSolveSettings {
            max_cash_evaluations: exact.max_cash_evaluations - 1,
            ..exact
        },
    ] {
        assert!(matches!(
            quote_workout_worst(&input, convention, q.target, reduced),
            Err(WorkflowError::Cashflow(
                convex_core::cashflows::CashflowError::BoundExceeded(_)
            ))
        ));
    }
}
#[test]
fn supplied_notice_forces_exact_path_and_failures_return_no_partial_quotes() {
    let r = reference();
    let c = &r["all_dates"];
    let cal = calendar(&r);
    let bond = contract(c, &cal);
    let mut context: WorkoutContext = input_context(c);
    let convention = serde_json::from_value(c["convention"].clone()).unwrap();
    context.trade_date = Date::parse("2027-06-01").unwrap();
    context.reference_date = Date::parse("2027-06-02").unwrap();
    context.settlement = Date::parse("2027-06-03").unwrap();
    context.notice = WorkoutNotice::Notified {
        provision: 0,
        exercise: Date::parse("2027-06-15").unwrap(),
        notice_date: Date::parse("2027-06-01").unwrap(),
    };
    let input = WorkoutInputs {
        contract: &bond,
        context: &context,
        exercise_calendar: (&bond.terms().calendars.exercise, &cal),
        purchase_calendar: (&bond.terms().calendars.payment, &cal),
    };
    let q =
        quote_workout_worst(&input, convention, BondPriceTarget::Dirty(72.0), controls()).unwrap();
    assert!(q.forced_by_notice);
    assert_eq!(q.quotes.len(), 1);
    assert_eq!(q.selected_index, 0);
    assert!(quote_workout_price(&input, &WorkoutSelection::Maturity {}, convention, 0.04).is_err());
    let q = quote_workout_yield(
        &input,
        &q.selected.selection,
        convention,
        BondPriceTarget::Dirty(200.0),
        settings(),
    );
    assert!(matches!(q, Err(WorkflowError::NoRoot { .. })));
}
