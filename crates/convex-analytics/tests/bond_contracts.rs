use convex_analytics::{bond_contract::*, dto::BondTerms};
use convex_bonds::contracts::*;
use convex_core::{
    cashflows::*,
    contracts::{IndexId, UtcTimestamp},
    daycounts::DayCountConvention as Dc,
    fixings::*,
    types::{Compounding, Currency},
    Date,
};
use convex_curves::{contracts::*, ExtrapolationMethod as Ex, InterpolationMethod as Int};
use rust_decimal_macros::dec;
use serde_json::Value;
fn date(s: &str) -> Date {
    Date::parse(s).unwrap()
}
fn instant(s: &str) -> UtcTimestamp {
    UtcTimestamp::parse(s).unwrap()
}
fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../convex-bonds/tests/fixtures/bond-contracts.json"
    ))
    .unwrap()
}
fn supplied(reference: Date, currency: Currency, role: CurveRole, rate: f64) -> BuiltCurve {
    let recipe = CurveRecipe {
        key: CurveKey {
            id: "supplied".into(),
            version: "v1".into(),
        },
        role,
        currency,
        reference_date: reference,
        time_basis: Dc::Act365Fixed,
        representation: CurveRepresentation::ZeroRates {
            compounding: Compounding::Continuous,
        },
        interpolation: Int::Linear,
        extrapolation: Ex::Flat,
        dependencies: vec![],
        calibration: CalibrationMode::Direct,
        source: CurveSource::Nodes(vec![
            CurveNode {
                pillar: CurvePoint::Tenor(0.01),
                quote: MarketQuote {
                    value: rate,
                    unit: QuoteUnit::DecimalRate,
                },
            },
            CurveNode {
                pillar: CurvePoint::Tenor(30.0),
                quote: MarketQuote {
                    value: rate,
                    unit: QuoteUnit::DecimalRate,
                },
            },
        ]),
    };
    build_curve_set(&[recipe], &CurveSet::new())
        .unwrap()
        .curves
        .get(&CurveKey {
            id: "supplied".into(),
            version: "v1".into(),
        })
        .unwrap()
        .as_ref()
        .clone()
}
fn close(actual: f64, expected: &Value) {
    let expected: f64 = expected.as_str().unwrap().parse().unwrap();
    assert!(
        (actual - expected).abs() <= 1e-9 + 1e-11 * expected.abs(),
        "actual={actual} expected={expected}"
    );
}
#[test]
fn model_pv_consumes_all_actual_stepped_and_allocated_cash_in_original_face_units() {
    let fixture = fixture();
    for case in fixture["cases"].as_array().unwrap() {
        let terms: BondTerms = serde_json::from_value(case["terms"].clone()).unwrap();
        let currency = terms.currency;
        let discount = supplied(
            terms.issue_date,
            currency,
            CurveRole::Discount {
                collateral_currency: currency,
            },
            0.03,
        );
        let native = terms.build().unwrap();
        let on = serde_json::from_value(case["on_payment"].clone()).unwrap();
        let result = price_fixed(
            &native,
            date(case["settlement"].as_str().unwrap()),
            on,
            &discount,
        )
        .unwrap();
        let expected = &case["expected"];
        close(result.present_value, &expected["present_value"]);
        close(result.clean_price, &expected["clean_price"]);
        close(result.dirty_price, &expected["dirty_price"]);
        assert_eq!(result.discount_curve, discount.recipe().key);
        assert!(!result.projection.conditional_on_no_exercise);
    }
}
#[test]
fn exact_projection_curve_policy_preserves_index_currency_and_reset_interval() {
    let request = ProjectionRequest {
        index_id: IndexId::new("USD.TERM").unwrap(),
        observation_date: date("2024-01-29"),
        rate_start: date("2024-02-01"),
        rate_end: date("2024-03-01"),
        day_count: Dc::Act360,
    };
    let projection = supplied(
        date("2024-01-01"),
        Currency::USD,
        CurveRole::Projection {
            index_id: "USD.TERM".into(),
        },
        0.03,
    );
    let actual = project_from_curve(&request, Currency::USD, &projection).unwrap();
    let expected = ((0.03_f64 * 29.0 / 365.0).exp() - 1.0) * 360.0 / 29.0;
    assert!((actual.to_string().parse::<f64>().unwrap() - expected).abs() < 1e-13);
    let discount = supplied(
        date("2024-01-01"),
        Currency::USD,
        CurveRole::Discount {
            collateral_currency: Currency::USD,
        },
        0.03,
    );
    assert!(project_from_curve(&request, Currency::USD, &discount).is_err());
    assert!(project_from_curve(&request, Currency::EUR, &projection).is_err());
    let mut wrong = request;
    wrong.index_id = IndexId::new("USD.OTHER").unwrap();
    assert!(project_from_curve(&wrong, Currency::USD, &projection).is_err());
}
#[test]
fn floating_model_price_requires_fixings_and_a_separate_supplied_projection_role() {
    let mut terms: BondTerms =
        serde_json::from_value(fixture()["cases"][1]["terms"].clone()).unwrap();
    terms.profiles = BondProfiles::constant(
        terms.schedule.start_date,
        terms.schedule.end_date,
        dec!(0),
        dec!(0.002),
        dec!(1),
        terms.face_value,
    );
    terms.coupon_principal = CouponPrincipal::AccrualProfile;
    terms.coupon = BondCoupon::Term(TermRateTerms {
        index_id: IndexId::new("USD.TERM").unwrap(),
        resets: terms.schedule.clone(),
        reset_adjustment: convex_core::calendars::BusinessDayConvention::Unadjusted,
        fixing_offset: DateOffset::CalendarDays(0),
        projection_day_count: Dc::Act360,
    });
    let bond = terms.build().unwrap();
    let context = BondProjectionContext {
        settlement: date("2024-01-01"),
        on_payment: PaymentDateEntitlement::Exclude,
        fixing: FixingContext {
            valuation_date: date("2023-12-31"),
            known_at: instant("2023-12-31T18:00:00Z"),
            unpublished: UnpublishedFixingPolicy::RequirePublished,
            final_only: true,
        },
        overnight_accrual_cutoffs: vec![],
    };
    let discount = supplied(
        date("2024-01-01"),
        Currency::USD,
        CurveRole::Discount {
            collateral_currency: Currency::USD,
        },
        0.03,
    );
    let projection = supplied(
        date("2024-01-01"),
        Currency::USD,
        CurveRole::Projection {
            index_id: "USD.TERM".into(),
        },
        0.04,
    );
    let publication = |_: &IndexId, day: Date| Ok(instant(&format!("{day}T16:00:00Z")));
    assert!(price_floating(
        &bond,
        &context,
        &FixingSeries::default(),
        publication,
        BondProjectionPolicy::RequirePublished,
        &discount
    )
    .is_err());
    let mut forecast =
        |request: &ProjectionRequest| project_from_curve(request, Currency::USD, &projection);
    let result = price_floating(
        &bond,
        &context,
        &FixingSeries::default(),
        publication,
        BondProjectionPolicy::Supplied(&mut forecast),
        &discount,
    )
    .unwrap();
    assert!(result.clean_price > 100.0);
    assert!(result.projection.coupons.iter().all(|c| c
        .rates
        .iter()
        .all(|r| matches!(r.origin, RateOrigin::Projected(_)))));
    let mut missing = context;
    missing.fixing.valuation_date = date("2024-07-02");
    missing.fixing.known_at = instant("2024-07-02T18:00:00Z");
    assert!(matches!(
        price_floating(
            &bond,
            &missing,
            &FixingSeries::default(),
            publication,
            BondProjectionPolicy::Supplied(&mut forecast),
            &discount
        ),
        Err(BondContractError::Cashflow(CashflowError::MissingFixings(
            _
        )))
    ));
}
#[test]
fn optional_and_incompatible_discount_contracts_cannot_be_valued_as_plain_bonds() {
    let mut terms: BondTerms =
        serde_json::from_value(fixture()["cases"][1]["terms"].clone()).unwrap();
    terms.exercise = vec![ExerciseTerms {
        right: ExerciseRight::Call,
        style: ExerciseStyle::European {
            date: date("2024-07-01"),
        },
        price_per_100: dec!(101),
        notice_offset: DateOffset::CalendarDays(-7),
        settlement_lag: DateOffset::BusinessDays(2),
        settlement_adjustment: convex_core::calendars::BusinessDayConvention::Following,
        accrued: ExerciseAccrued::PayAccrued,
        protection_end: None,
    }];
    let discount = supplied(
        terms.issue_date,
        terms.currency,
        CurveRole::Discount {
            collateral_currency: terms.currency,
        },
        0.03,
    );
    let optional = terms.clone().build().unwrap();
    assert!(matches!(
        price_fixed(
            &optional,
            terms.issue_date,
            PaymentDateEntitlement::Exclude,
            &discount
        ),
        Err(BondContractError::Unsupported { .. })
    ));
    terms.exercise.clear();
    let native = terms.clone().build().unwrap();
    let wrong = supplied(
        terms.issue_date,
        Currency::EUR,
        CurveRole::Discount {
            collateral_currency: Currency::EUR,
        },
        0.03,
    );
    assert!(price_fixed(
        &native,
        terms.issue_date,
        PaymentDateEntitlement::Exclude,
        &wrong
    )
    .is_err());
    let wrong = supplied(
        terms.issue_date,
        Currency::USD,
        CurveRole::Government {
            benchmark_id: "UST".into(),
        },
        0.03,
    );
    assert!(price_fixed(
        &native,
        terms.issue_date,
        PaymentDateEntitlement::Exclude,
        &wrong
    )
    .is_err());
}
