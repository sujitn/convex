//! Callable perpetuals quoted to each call and to perpetuity, against the independent
//! reference `tests/scripts/perpetual_quote_reference.py`.
use convex_analytics::workflow::*;
use convex_bonds::contracts::{BondCalendars, BondTerms, PerpetualContract};
use convex_core::{
    calendars::Calendar,
    cashflows::PaymentDateEntitlement,
    contracts::{IndexId, SourceId, UtcTimestamp, Version},
    daycounts::DayCountConvention,
    fixings::{
        FixingContext, FixingObservation, FixingRequirement, FixingSeries, FixingStatus,
        ProjectionReason, RateOrigin, UnpublishedFixingPolicy,
    },
    types::Compounding,
    Date,
};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::str::FromStr;

struct Weekends;

impl Calendar for Weekends {
    fn name(&self) -> &'static str {
        "Weekends"
    }
    fn is_business_day(&self, date: Date) -> bool {
        date.weekday().num_days_from_monday() < 5
    }
}

const CALENDARS: BondCalendars<'static> = BondCalendars {
    payment: &Weekends,
    reset: &Weekends,
    observation: &Weekends,
    exercise: &Weekends,
};

const CONVENTION: DatedYieldConvention = DatedYieldConvention {
    day_count: DayCountConvention::Thirty360US,
    compounding: Compounding::SemiAnnual,
};

const SETTINGS: WorkoutSolveSettings = WorkoutSolveSettings {
    inverse: BondInverseSettings {
        lower: -0.5,
        upper: 2.0,
        price_tolerance: 1e-10,
        parameter_tolerance: 1e-12,
        max_iterations: 128,
    },
    max_dates: 64,
    max_cash_evaluations: 10_000_000,
};

fn reference() -> Value {
    serde_json::from_str(include_str!("fixtures/perpetual-quotes.json")).unwrap()
}

fn date(value: &Value) -> Date {
    Date::parse(value.as_str().unwrap()).unwrap()
}

fn decimal(value: &Value) -> Decimal {
    Decimal::from_str(value.as_str().unwrap()).unwrap()
}

fn number(value: &Value) -> f64 {
    value.as_str().unwrap().parse().unwrap()
}

fn contract(bond: &Value) -> PerpetualContract {
    let terms: BondTerms = serde_json::from_value(bond["terms"].clone()).unwrap();
    terms.build_perpetual_with_calendars(&CALENDARS).unwrap()
}

fn timestamp(day: Date, time: &str) -> UtcTimestamp {
    UtcTimestamp::parse(&format!("{day}T{time}Z")).unwrap()
}

/// A case's purchase, and its benchmark: the reference's fixings, each published at
/// 16:00 on its date, read at 18:00 on the trade date.
struct Case {
    context: WorkoutContext,
    horizon: Date,
    fixings: FixingSeries,
    publications: PublicationSchedule,
    valuation: FixingContext,
    held_level: Option<Decimal>,
}

impl Case {
    fn new(contract: &PerpetualContract, case: &Value) -> Self {
        let trade = date(&case["trade_date"]);
        let benchmark = IndexId::new("USD.SWAP.5Y").unwrap();
        let fixings = case["fixings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                let observed = date(&f["observation_date"]);
                FixingObservation {
                    index_id: benchmark.clone(),
                    observation_date: observed,
                    rate: decimal(&f["rate"]),
                    published_at: timestamp(observed, "16:00:00"),
                    known_at: timestamp(observed, "16:00:00"),
                    revision: Version::new(1).unwrap(),
                    status: FixingStatus::Final,
                    source_id: SourceId::new("reference").unwrap(),
                }
            })
            .collect();
        let far = Date::from_ymd(2100, 1, 1).unwrap();
        let publications = contract
            .resets(far, &Weekends)
            .unwrap()
            .into_iter()
            .map(|reset| FixingRequirement {
                index_id: benchmark.clone(),
                observation_date: reset.fixing_date,
                expected_publication: timestamp(reset.fixing_date, "16:00:00"),
            })
            .collect();
        Self {
            context: WorkoutContext {
                reference_date: trade,
                trade_date: trade,
                settlement: date(&case["settlement"]),
                on_payment: PaymentDateEntitlement::Exclude,
                notice: WorkoutNotice::Unnotified {
                    earliest_notice: trade,
                },
            },
            horizon: date(&case["horizon"]),
            fixings: FixingSeries::new(fixings).unwrap(),
            publications: PublicationSchedule::new(publications).unwrap(),
            valuation: FixingContext {
                valuation_date: trade,
                known_at: timestamp(trade, "18:00:00"),
                unpublished: UnpublishedFixingPolicy::RequirePublished,
                final_only: true,
            },
            held_level: case["held_benchmark"].as_str().map(|v| v.parse().unwrap()),
        }
    }

    fn inputs<'a>(&'a self, contract: &'a PerpetualContract) -> PerpetualInputs<'a> {
        PerpetualInputs {
            contract,
            context: &self.context,
            calendar_ids: &contract.terms().calendars,
            calendars: &CALENDARS,
            horizon: self.horizon,
            reset_market: self.held_level.map(|held_level| ResetMarket {
                fixings: &self.fixings,
                publications: &self.publications,
                fixing: self.valuation,
                held_level,
            }),
        }
    }
}

fn label(workout: PerpetualWorkout) -> String {
    match workout {
        PerpetualWorkout::Call { date } => date.to_string(),
        PerpetualWorkout::Perpetuity => "perpetuity".into(),
    }
}

fn cases() -> Vec<(PerpetualContract, Value)> {
    reference()["bonds"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|bond| {
            let contract = contract(bond);
            bond["cases"]
                .as_array()
                .unwrap()
                .iter()
                .map(move |case| (contract.clone(), case.clone()))
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn each_workout_yields_the_reference_and_the_worst_is_the_lowest() {
    for (contract, case) in cases() {
        let supplied = Case::new(&contract, &case);
        let accrued = number(&case["accrued_per100"]);
        for quote in case["yield_to_worst"].as_array().unwrap() {
            let clean = number(&quote["clean_price"]);
            let worst = quote_perpetual_worst(
                &supplied.inputs(&contract),
                CONVENTION,
                BondPriceTarget::Clean(clean),
                SETTINGS,
            )
            .unwrap();
            let expected = quote["yields"].as_array().unwrap();
            assert_eq!(worst.quotes.len(), expected.len());
            for (ours, theirs) in worst.quotes.iter().zip(expected) {
                assert_eq!(label(ours.workout), theirs["workout"].as_str().unwrap());
                let gap = (ours.decimal_yield - number(&theirs["yield"])).abs();
                assert!(
                    gap < 1e-10,
                    "{} {clean} {theirs}: {gap}",
                    case["settlement"]
                );
                assert!((ours.accrued_per100 - accrued).abs() < 1e-12);
                assert!((ours.clean_per100 - clean).abs() < 1e-9);
            }
            assert_eq!(
                label(worst.quotes[worst.selected_index].workout),
                quote["worst"].as_str().unwrap()
            );
            assert!(worst.cash_discount_evaluations <= worst.reserved_cash_evaluations);
        }
    }
}

#[test]
fn each_workout_prices_at_the_reference_and_the_worst_is_the_lowest() {
    for (contract, case) in cases() {
        let supplied = Case::new(&contract, &case);
        for quote in case["price_to_worst"].as_array().unwrap() {
            let y = number(&quote["yield"]);
            let worst =
                quote_perpetual_price_to_worst(&supplied.inputs(&contract), CONVENTION, y).unwrap();
            let expected = quote["prices"].as_array().unwrap();
            assert_eq!(worst.quotes.len(), expected.len(), "{y}");
            for (ours, theirs) in worst.quotes.iter().zip(expected) {
                assert_eq!(label(ours.workout), theirs["workout"].as_str().unwrap());
                for (field, value) in [("clean", ours.clean_per100), ("dirty", ours.dirty_per100)] {
                    let gap = (value - number(&theirs[field])).abs();
                    assert!(
                        gap < 1e-9,
                        "{} {y} {field} {theirs}: {gap}",
                        case["settlement"]
                    );
                }
            }
            assert_eq!(
                label(worst.quotes[worst.selected_index].workout),
                quote["worst"].as_str().unwrap()
            );
        }
    }
}

#[test]
fn the_composition_names_its_calls_its_tail_and_each_reset() {
    for (contract, case) in cases() {
        let supplied = Case::new(&contract, &case);
        let worst =
            quote_perpetual_price_to_worst(&supplied.inputs(&contract), CONVENTION, 0.05).unwrap();
        let composition = &worst.composition;
        let calls: Vec<Date> = case["calls"].as_array().unwrap().iter().map(date).collect();
        assert_eq!(composition.calls, calls);
        assert_eq!(composition.tail.start, date(&case["tail_start"]));
        assert_eq!(composition.tail.coupon_rate, decimal(&case["tail_rate"]));
        assert_eq!(composition.held_level, supplied.held_level);
        // A fixed coupon reads no benchmark. A reset reads the published fixing where
        // there is one, and holds the current level after it, up to the tail.
        let published: Vec<Date> = case["fixings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| date(&f["observation_date"]))
            .collect();
        for reset in &composition.resets {
            let observed = reset.benchmark.requirement.observation_date;
            if published.contains(&observed) {
                assert!(matches!(reset.benchmark.origin, RateOrigin::Published(_)));
            } else {
                assert_eq!(
                    reset.benchmark.origin,
                    RateOrigin::Projected(ProjectionReason::FutureObservation)
                );
                assert_eq!(Some(reset.benchmark.rate), supplied.held_level);
                assert!(reset.reset_date >= composition.tail.start);
            }
        }
        assert_eq!(composition.resets.is_empty(), supplied.held_level.is_none());
    }
}

#[test]
fn a_notice_given_forces_its_call() {
    let reference = reference();
    let bond = &reference["bonds"][0];
    let (contract, case) = (contract(bond), &bond["cases"][0]);
    let mut supplied = Case::new(&contract, case);
    let first_call = date(&case["calls"][0]);
    supplied.context.notice = WorkoutNotice::Notified {
        provision: 0,
        exercise: first_call,
        notice_date: supplied.context.trade_date,
    };
    let quote = &case["yield_to_worst"][0];
    let worst = quote_perpetual_worst(
        &supplied.inputs(&contract),
        CONVENTION,
        BondPriceTarget::Clean(number(&quote["clean_price"])),
        SETTINGS,
    )
    .unwrap();
    assert!(worst.composition.forced_by_notice);
    assert_eq!(worst.quotes.len(), 1);
    assert_eq!(
        worst.quotes[0].workout,
        PerpetualWorkout::Call { date: first_call }
    );
    assert!((worst.quotes[0].decimal_yield - number(&quote["yields"][0]["yield"])).abs() < 1e-10);
    // A notice of a date that is not a call is refused.
    supplied.context.notice = WorkoutNotice::Notified {
        provision: 0,
        exercise: first_call.add_days(1),
        notice_date: supplied.context.trade_date,
    };
    assert!(quote_perpetual_price_to_worst(&supplied.inputs(&contract), CONVENTION, 0.05).is_err());
}

#[test]
fn a_reset_reads_its_published_fixing_or_is_refused() {
    let reference = reference();
    let bond = &reference["bonds"][1];
    let (contract, case) = (contract(bond), &bond["cases"][1]);
    let mut supplied = Case::new(&contract, case);
    supplied.fixings = FixingSeries::new(vec![]).unwrap();
    let missing =
        quote_perpetual_price_to_worst(&supplied.inputs(&contract), CONVENTION, 0.05).unwrap_err();
    assert!(
        matches!(&missing, WorkflowError::MissingInput(inputs) if matches!(inputs[..], [MissingInput::Fixing(_)])),
        "{missing}"
    );
    supplied.held_level = None;
    assert!(matches!(
        quote_perpetual_price_to_worst(&supplied.inputs(&contract), CONVENTION, 0.05),
        Err(WorkflowError::Invalid {
            field: "perpetual.reset_market",
            ..
        })
    ));
    // A fixed coupon reads no benchmark.
    let fixed = contract_of(0);
    let mut supplied = Case::new(&fixed, &reference["bonds"][0]["cases"][0]);
    supplied.held_level = Some(Decimal::new(385, 4));
    assert!(matches!(
        quote_perpetual_price_to_worst(&supplied.inputs(&fixed), CONVENTION, 0.05),
        Err(WorkflowError::Invalid {
            field: "perpetual.reset_market",
            ..
        })
    ));
}

fn contract_of(bond: usize) -> PerpetualContract {
    contract(&reference()["bonds"][bond])
}

#[test]
fn only_a_compounded_30_360_yield_is_admitted() {
    let reference = reference();
    let contract = contract_of(0);
    let supplied = Case::new(&contract, &reference["bonds"][0]["cases"][0]);
    for convention in [
        DatedYieldConvention {
            day_count: DayCountConvention::Act360,
            ..CONVENTION
        },
        DatedYieldConvention {
            compounding: Compounding::Simple,
            ..CONVENTION
        },
    ] {
        assert!(matches!(
            quote_perpetual_price_to_worst(&supplied.inputs(&contract), convention, 0.05),
            Err(WorkflowError::Unsupported(_))
        ));
    }
}

#[test]
fn the_work_budget_is_reserved_before_any_path_is_solved() {
    let reference = reference();
    let contract = contract_of(0);
    let supplied = Case::new(&contract, &reference["bonds"][0]["cases"][0]);
    let target = BondPriceTarget::Clean(100.0);
    let budget = |max_dates, max_cash_evaluations| WorkoutSolveSettings {
        max_dates,
        max_cash_evaluations,
        ..SETTINGS
    };
    let enough = quote_perpetual_worst(&supplied.inputs(&contract), CONVENTION, target, SETTINGS)
        .unwrap()
        .reserved_cash_evaluations;
    assert!(quote_perpetual_worst(
        &supplied.inputs(&contract),
        CONVENTION,
        target,
        budget(64, enough - 1)
    )
    .is_err());
    // Three calls are quoted; a budget of two dates refuses rather than truncates.
    assert!(quote_perpetual_worst(
        &supplied.inputs(&contract),
        CONVENTION,
        target,
        budget(2, SETTINGS.max_cash_evaluations)
    )
    .is_err());
}

#[test]
fn a_call_whose_notice_is_already_past_is_not_quoted() {
    let reference = reference();
    let contract = contract_of(0);
    let case = &reference["bonds"][0]["cases"][0];
    let mut supplied = Case::new(&contract, case);
    let first_call = date(&case["calls"][0]);
    // Thirty days' notice: a notice given no earlier than 29 days before misses it.
    supplied.context.notice = WorkoutNotice::Unnotified {
        earliest_notice: first_call.add_days(-29),
    };
    let worst =
        quote_perpetual_price_to_worst(&supplied.inputs(&contract), CONVENTION, 0.05).unwrap();
    let expected: Vec<Date> = case["calls"].as_array().unwrap()[1..]
        .iter()
        .map(date)
        .collect();
    assert_eq!(worst.composition.calls, expected);
}

#[test]
fn a_horizon_before_the_first_call_quotes_the_perpetuity_alone() {
    let reference = reference();
    let contract = contract_of(0);
    let case = &reference["bonds"][0]["cases"][0];
    let mut supplied = Case::new(&contract, case);
    supplied.horizon = Date::from_ymd(2028, 1, 15).unwrap();
    let quote = &case["price_to_worst"][0];
    let worst = quote_perpetual_price_to_worst(
        &supplied.inputs(&contract),
        CONVENTION,
        number(&quote["yield"]),
    )
    .unwrap();
    assert!(worst.composition.calls.is_empty());
    assert_eq!(worst.quotes.len(), 1);
    assert_eq!(worst.quotes[0].workout, PerpetualWorkout::Perpetuity);
    let perpetuity = quote["prices"].as_array().unwrap().last().unwrap();
    assert!((worst.quotes[0].clean_per100 - number(&perpetuity["clean"])).abs() < 1e-9);
}

#[test]
fn calendars_bound_to_other_identities_are_refused() {
    let reference = reference();
    let contract = contract_of(0);
    let supplied = Case::new(&contract, &reference["bonds"][0]["cases"][0]);
    let other = convex_bonds::contracts::BondCalendarIds::single(
        convex_bonds::types::CalendarId::new("USNY"),
    );
    let inputs = PerpetualInputs {
        calendar_ids: &other,
        ..supplied.inputs(&contract)
    };
    assert!(matches!(
        quote_perpetual_price_to_worst(&inputs, CONVENTION, 0.05),
        Err(WorkflowError::Invalid {
            field: "perpetual.calendars",
            ..
        })
    ));
}

#[test]
fn settled_on_a_call_date_at_its_coupon_the_perpetuity_is_par() {
    let reference = reference();
    let contract = contract_of(0);
    let mut supplied = Case::new(&contract, &reference["bonds"][0]["cases"][0]);
    // Settling on the first call, a coupon date: nothing accrues, the call has passed, and
    // at its 6.5% coupon the perpetuity is worth par.
    let (trade, settlement) = (
        Date::from_ymd(2030, 1, 11).unwrap(),
        Date::from_ymd(2030, 1, 15).unwrap(),
    );
    supplied.context.trade_date = trade;
    supplied.context.reference_date = trade;
    supplied.context.settlement = settlement;
    supplied.context.notice = WorkoutNotice::Unnotified {
        earliest_notice: trade,
    };
    let worst =
        quote_perpetual_price_to_worst(&supplied.inputs(&contract), CONVENTION, 0.065).unwrap();
    assert_eq!(
        worst.composition.calls.first(),
        Some(&Date::from_ymd(2030, 7, 15).unwrap())
    );
    let perpetuity = worst.quotes.last().unwrap();
    assert_eq!(perpetuity.workout, PerpetualWorkout::Perpetuity);
    assert_eq!(perpetuity.accrued_per100, 0.0);
    assert!(
        (perpetuity.dirty_per100 - 100.0).abs() < 1e-9,
        "{}",
        perpetuity.dirty_per100
    );
}

#[test]
fn settled_ex_coupon_before_the_tail_the_perpetuity_is_its_tail_alone() {
    let mut terms = reference()["bonds"][0]["terms"].clone();
    terms["dates"]["ex_coupon"] = json!({
        "offset": {"BusinessDays": -7},
        "anchor": "ContractualEnd",
        "boundary": "OnDate"
    });
    let contract = contract(&json!({ "terms": terms }));
    // Settled Monday 8 July 2030, after the ex date of the 15 July coupon: the buyer has
    // no explicit cash, only the tail from 15 July.
    let case = json!({
        "trade_date": "2030-07-04",
        "settlement": "2030-07-08",
        "horizon": "2031-01-15",
        "fixings": []
    });
    let supplied = Case::new(&contract, &case);
    let worst =
        quote_perpetual_price_to_worst(&supplied.inputs(&contract), CONVENTION, 0.065).unwrap();
    assert_eq!(
        worst.composition.tail.start,
        Date::from_ymd(2030, 7, 15).unwrap()
    );
    let perpetuity = worst.quotes.last().unwrap();
    assert_eq!(perpetuity.workout, PerpetualWorkout::Perpetuity);
    assert!(perpetuity.accrued_per100 < 0.0);
    // At its coupon the tail is par on 15 July, discounted seven 30/360 days.
    let expected = 100.0 * 1.0325_f64.powf(-2.0 * 7.0 / 360.0);
    assert!(
        (perpetuity.dirty_per100 - expected).abs() < 1e-9,
        "{}",
        perpetuity.dirty_per100
    );
    let solved = quote_perpetual_worst(
        &supplied.inputs(&contract),
        CONVENTION,
        BondPriceTarget::Dirty(expected),
        SETTINGS,
    )
    .unwrap();
    let quote = solved.quotes.last().unwrap();
    assert_eq!(quote.workout, PerpetualWorkout::Perpetuity);
    assert!((quote.decimal_yield - 0.065).abs() < 1e-10, "{quote:?}");
}

#[test]
fn a_perpetuity_is_priced_and_solved_at_the_yield_floor_and_left_out_below_it() {
    let contract = contract_of(0);
    let mut supplied = Case::new(&contract, &reference()["bonds"][0]["cases"][0]);
    supplied.horizon = Date::from_ymd(2028, 1, 15).unwrap();
    let inputs = supplied.inputs(&contract);
    assert!(matches!(
        quote_perpetual_price_to_worst(&inputs, CONVENTION, MIN_PERPETUITY_YIELD / 2.0),
        Err(WorkflowError::Invalid {
            field: "perpetual.selection",
            ..
        })
    ));
    let priced = quote_perpetual_price_to_worst(&inputs, CONVENTION, MIN_PERPETUITY_YIELD).unwrap();
    let solved = quote_perpetual_worst(
        &inputs,
        CONVENTION,
        BondPriceTarget::Dirty(priced.quotes[0].dirty_per100),
        SETTINGS,
    )
    .unwrap();
    assert_eq!(solved.quotes[0].decimal_yield, MIN_PERPETUITY_YIELD);
}

#[test]
fn fixings_read_on_another_date_than_the_quote_are_refused() {
    let reference = reference();
    let bond = &reference["bonds"][1];
    let (contract, case) = (contract(bond), &bond["cases"][0]);
    let mut supplied = Case::new(&contract, case);
    // Read as of the first reset's fixing, the quote would take a fixing not yet made.
    supplied.valuation.valuation_date = Date::from_ymd(2030, 1, 11).unwrap();
    supplied.valuation.known_at = timestamp(supplied.valuation.valuation_date, "18:00:00");
    assert!(matches!(
        quote_perpetual_price_to_worst(&supplied.inputs(&contract), CONVENTION, 0.07),
        Err(WorkflowError::Invalid {
            field: "perpetual.reset_market",
            ..
        })
    ));
}

#[test]
fn a_reset_fixed_on_the_valuation_date_is_read_published() {
    let reference = reference();
    let contract = contract_of(1);
    // The 15 January 2030 reset fixes on Friday 11 January: trade on it.
    let case = &reference["bonds"][1]["cases"][1];
    let mut supplied = Case::new(&contract, case);
    let (trade, settlement) = (
        Date::from_ymd(2030, 1, 11).unwrap(),
        Date::from_ymd(2030, 1, 15).unwrap(),
    );
    supplied.context.trade_date = trade;
    supplied.context.reference_date = trade;
    supplied.context.settlement = settlement;
    supplied.context.notice = WorkoutNotice::Unnotified {
        earliest_notice: trade,
    };
    supplied.valuation.valuation_date = trade;
    supplied.valuation.known_at = timestamp(trade, "18:00:00");
    let worst =
        quote_perpetual_price_to_worst(&supplied.inputs(&contract), CONVENTION, 0.07).unwrap();
    let first = &worst.composition.resets[0];
    assert_eq!(first.reset_date, settlement);
    assert!(matches!(first.benchmark.origin, RateOrigin::Published(_)));
    assert_eq!(
        first.coupon_rate,
        decimal(&case["fixings"][0]["rate"]) + Decimal::new(412, 4)
    );
    assert_eq!(
        worst.composition.tail.start,
        Date::from_ymd(2035, 1, 15).unwrap()
    );
    // Read before its publication, the fixing is due and missing.
    supplied.valuation.known_at = timestamp(trade, "12:00:00");
    assert!(matches!(
        quote_perpetual_price_to_worst(&supplied.inputs(&contract), CONVENTION, 0.07),
        Err(WorkflowError::MissingInput(_))
    ));
}
