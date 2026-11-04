use convex_bonds::{
    contracts::*,
    instruments::{
        CallableBond, CallableFloatingRateNote, FixedRateBond, FloatingRateNote, SinkingFundBond,
        SinkingFundPayment, SinkingFundSchedule,
    },
    traits::Bond,
    types::{
        AmortizationEntry, AmortizationSchedule, AmortizationType, CalendarId, CallEntry,
        CallSchedule, CallType, PutEntry, PutSchedule, PutType,
    },
};
use convex_core::{
    calendars::BusinessDayConvention as Bdc,
    cashflows::*,
    contracts::{IndexId, SourceId, UtcTimestamp, Version},
    daycounts::DayCountConvention as Dc,
    fixings::*,
    types::Frequency,
    Date,
};
use convex_curves::multicurve::RateIndex;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::Value;

fn date(s: &str) -> Date {
    Date::parse(s).unwrap()
}
fn instant(s: &str) -> UtcTimestamp {
    UtcTimestamp::parse(s).unwrap()
}
// The oracle retains 50 digits. Decimal cash supports at most 28 places;
// truncate only the reader representation (error <=1e-28), not the oracle or
// the declared 1e-22 comparison bound. Zero scientific values stay exact.
fn cash(v: &Value) -> Decimal {
    let value = v.as_str().unwrap();
    if let Some((mantissa, _)) = value.split_once('E') {
        let zero: Decimal = mantissa.parse().unwrap();
        assert_eq!(zero, Decimal::ZERO);
        return zero;
    }
    if let Some(dot) = value.find('.') {
        return value[..value.len().min(dot + 29)].parse().unwrap();
    }
    value.parse().unwrap()
}
fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/bond-contracts.json")).unwrap()
}
fn terms(id: &str) -> BondTerms {
    serde_json::from_value(
        fixture()["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["id"] == id)
            .unwrap()["terms"]
            .clone(),
    )
    .unwrap()
}
fn close(actual: Decimal, expected: Decimal) {
    assert!(
        (actual - expected).abs() <= dec!(0.0000000000000000000001),
        "actual={actual} expected={expected}"
    );
}
fn context(settlement: &str, valuation: &str) -> BondProjectionContext {
    BondProjectionContext {
        settlement: date(settlement),
        on_payment: PaymentDateEntitlement::Exclude,
        fixing: FixingContext {
            valuation_date: date(valuation),
            known_at: instant(&format!("{valuation}T18:00:00Z")),
            unpublished: UnpublishedFixingPolicy::RequirePublished,
            final_only: true,
        },
        overnight_accrual_cutoffs: vec![],
    }
}
fn publication(_: &IndexId, day: Date) -> Result<UtcTimestamp, CashflowError> {
    Ok(instant(&format!("{day}T16:00:00Z")))
}
fn observation(
    index: &str,
    day: &str,
    rate: Decimal,
    published: &str,
    revision: u64,
) -> FixingObservation {
    FixingObservation {
        index_id: IndexId::new(index).unwrap(),
        observation_date: date(day),
        rate,
        published_at: instant(published),
        known_at: instant(published),
        revision: Version::new(revision).unwrap(),
        status: FixingStatus::Final,
        source_id: SourceId::new("supplied").unwrap(),
    }
}
fn term_bond() -> BondTerms {
    let mut t = terms("coupon-principal-BeforeCouponBoundary");
    t.issue_date = date("2023-12-29");
    t.schedule.start_date = date("2024-01-01");
    t.schedule.end_date = date("2024-04-01");
    t.schedule.frequency = Frequency::Quarterly;
    t.profiles = BondProfiles::constant(
        date("2024-01-01"),
        date("2024-04-01"),
        Decimal::ZERO,
        dec!(0.002),
        Decimal::ONE,
        dec!(5000),
    );
    let mut resets = t.schedule.clone();
    resets.frequency = Frequency::Monthly;
    t.coupon = BondCoupon::Term(TermRateTerms {
        index_id: IndexId::new("SUPPLIED.TERM").unwrap(),
        resets,
        reset_adjustment: Bdc::Unadjusted,
        fixing_offset: DateOffset::CalendarDays(0),
        projection_day_count: Dc::Act360,
    });
    t.coupon_principal = CouponPrincipal::AccrualProfile;
    t
}
fn overnight_bond() -> BondTerms {
    let mut t = term_bond();
    t.issue_date = date("2024-09-27");
    t.schedule.start_date = date("2024-09-27");
    t.schedule.end_date = date("2024-10-02");
    t.schedule.stub_type = StubType::ShortLast;
    t.profiles = BondProfiles::constant(
        date("2024-09-27"),
        date("2024-10-02"),
        Decimal::ZERO,
        dec!(0.001),
        Decimal::ONE,
        dec!(5000),
    );
    t.coupon = BondCoupon::Overnight(OvernightRateTerms {
        index_id: IndexId::new("SUPPLIED.ON").unwrap(),
        convention: OvernightConvention {
            lookback_business_days: 0,
            observation_shift: false,
            lockout_business_days: 0,
        },
    });
    t.dates.payment_delay = DateOffset::BusinessDays(2);
    t
}
fn friday_history() -> FixingSeries {
    FixingSeries::new(vec![observation(
        "SUPPLIED.ON",
        "2024-09-27",
        dec!(0.05),
        "2024-09-27T16:00:00Z",
        1,
    )])
    .unwrap()
}

#[test]
fn supplied_decimal_goldens_cover_steps_principal_stubs_payment_and_entitlement() {
    let f = fixture();
    assert_eq!(f["certified"], false);
    for case in f["cases"].as_array().unwrap() {
        let input: BondTerms = serde_json::from_value(case["terms"].clone()).unwrap();
        let bond = input.clone().build().unwrap();
        let roundtrip: BondTerms =
            serde_json::from_slice(&serde_json::to_vec(bond.terms()).unwrap()).unwrap();
        assert_eq!(input, roundtrip);
        assert_eq!(roundtrip.build().unwrap().terms(), bond.terms());
        let on: PaymentDateEntitlement =
            serde_json::from_value(case["on_payment"].clone()).unwrap();
        let result = bond
            .fixed_projection(date(case["settlement"].as_str().unwrap()), on)
            .unwrap();
        let expected = &case["expected"];
        assert_eq!(
            result.coupons.len(),
            expected["coupons"].as_array().unwrap().len(),
            "{}",
            case["id"]
        );
        assert_eq!(
            result.principal.len(),
            expected["principal"].as_array().unwrap().len(),
            "{}",
            case["id"]
        );
        for (actual, expected) in result
            .coupons
            .iter()
            .zip(expected["coupons"].as_array().unwrap())
        {
            assert_eq!(
                actual.payment_date,
                date(expected["payment_date"].as_str().unwrap())
            );
            assert_eq!(actual.entitled, expected["entitled"].as_bool().unwrap());
            close(actual.amount, cash(&expected["amount"]));
            close(actual.accrued, cash(&expected["accrued"]));
        }
        for (actual, expected) in result
            .principal
            .iter()
            .zip(expected["principal"].as_array().unwrap())
        {
            assert_eq!(
                actual.effective_date,
                date(expected["effective_date"].as_str().unwrap())
            );
            assert_eq!(
                actual.payment_date,
                date(expected["payment_date"].as_str().unwrap())
            );
            close(actual.amount, cash(&expected["amount"]));
            close(actual.principal.before, cash(&expected["before"]));
            close(actual.principal.after, cash(&expected["after"]));
        }
        close(result.accrued, cash(&expected["accrued"]));
        close(result.outstanding_principal, cash(&expected["outstanding"]));
    }
}

#[test]
fn independent_term_resets_use_actual_cutoff_revisions_and_segment_principal() {
    let mut t = term_bond();
    t.profiles.gearing = vec![
        DatedValue {
            start: date("2024-01-01"),
            end: date("2024-02-15"),
            value: dec!(0.5),
        },
        DatedValue {
            start: date("2024-02-15"),
            end: date("2024-04-01"),
            value: dec!(1.25),
        },
    ];
    t.profiles.notional = vec![
        DatedValue {
            start: date("2024-01-01"),
            end: date("2024-03-01"),
            value: dec!(5000),
        },
        DatedValue {
            start: date("2024-03-01"),
            end: date("2024-04-01"),
            value: dec!(3000),
        },
    ];
    let bond = t.build().unwrap();
    let history = FixingSeries::new(vec![
        observation(
            "SUPPLIED.TERM",
            "2024-01-01",
            dec!(0.03),
            "2024-01-01T16:00:00Z",
            1,
        ),
        observation(
            "SUPPLIED.TERM",
            "2024-01-01",
            dec!(0.04),
            "2024-02-20T16:00:00Z",
            2,
        ),
        observation(
            "SUPPLIED.TERM",
            "2024-02-01",
            dec!(0.06),
            "2024-02-01T16:00:00Z",
            1,
        ),
        observation(
            "SUPPLIED.TERM",
            "2024-03-01",
            dec!(0.09),
            "2024-03-01T16:00:00Z",
            1,
        ),
    ])
    .unwrap();
    let mut seen = vec![];
    let mut forecast = |request: &ProjectionRequest| {
        seen.push(request.clone());
        Ok(dec!(-0.01))
    };
    let result = bond
        .project(
            &context("2024-02-15", "2024-02-15"),
            &history,
            publication,
            BondProjectionPolicy::Supplied(&mut forecast),
        )
        .unwrap();
    let c = &result.coupons[0];
    close(
        c.amount,
        (dec!(5000) * dec!(0.017) * dec!(31)
            + dec!(5000) * dec!(0.032) * dec!(14)
            + dec!(5000) * dec!(0.077) * dec!(15)
            + dec!(3000) * dec!(-0.0105) * dec!(31))
            / dec!(360),
    );
    close(
        c.accrued,
        (dec!(5000) * dec!(0.017) * dec!(31) + dec!(5000) * dec!(0.032) * dec!(14)) / dec!(360),
    );
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].rate_start, date("2024-03-01"));
    assert_eq!(seen[0].rate_end, date("2024-04-01"));
    assert!(matches!(&c.rates[0].origin, RateOrigin::Published(o) if o.revision.get()==1));
    assert_eq!(c.rates[2].rate, dec!(-0.01));
    assert_eq!(
        c.rates[2].origin,
        RateOrigin::Projected(ProjectionReason::FutureObservation)
    );
}

#[test]
fn all_missing_published_history_fails_before_any_forecast_even_across_bounded_batches() {
    let bond = term_bond().build().unwrap();
    let called = std::cell::Cell::new(0);
    let mut forecast = |_: &ProjectionRequest| {
        called.set(called.get() + 1);
        Ok(dec!(0.03))
    };
    let err = bond
        .project(
            &context("2024-03-15", "2024-03-15"),
            &FixingSeries::default(),
            publication,
            BondProjectionPolicy::Supplied(&mut forecast),
        )
        .unwrap_err();
    assert!(
        matches!(err, BondContractError::Cashflow(CashflowError::MissingFixings(v)) if v.len()==3)
    );
    assert_eq!(called.get(), 0);
    // More than one core resolution batch: a missing first historical coupon
    // prevents every future curve call in this explicitly bounded long contract.
    let mut t = overnight_bond();
    t.issue_date = date("2000-01-03");
    t.schedule.start_date = t.issue_date;
    t.schedule.end_date = date("2020-01-03");
    t.schedule.stub_type = StubType::None;
    t.profiles = BondProfiles::constant(
        t.issue_date,
        t.schedule.end_date,
        Decimal::ZERO,
        dec!(0.001),
        Decimal::ONE,
        dec!(5000),
    );
    let long = t.build().unwrap();
    let err = long
        .project(
            &context("2000-01-04", "2000-01-04"),
            &FixingSeries::default(),
            publication,
            BondProjectionPolicy::Supplied(&mut forecast),
        )
        .unwrap_err();
    assert!(
        matches!(err,BondContractError::Cashflow(CashflowError::MissingFixings(v)) if v.len()==2)
    );
    assert_eq!(called.get(), 0);
}

#[test]
fn unpublished_observation_has_distinct_policy_and_origin() {
    let bond = term_bond().build().unwrap();
    let mut c = context("2024-01-01", "2024-01-01");
    let publication = |_: &IndexId, d: Date| {
        Ok(instant(&format!(
            "{}T16:00:00Z",
            d.checked_add_days(1).unwrap()
        )))
    };
    let mut forecast = |_: &ProjectionRequest| Ok(dec!(0.03));
    assert!(bond
        .project(
            &c,
            &FixingSeries::default(),
            publication,
            BondProjectionPolicy::Supplied(&mut forecast)
        )
        .is_err());
    c.fixing.unpublished = UnpublishedFixingPolicy::Forecast;
    let projected = bond
        .project(
            &c,
            &FixingSeries::default(),
            publication,
            BondProjectionPolicy::Supplied(&mut forecast),
        )
        .unwrap();
    assert_eq!(
        projected.coupons[0].rates[0].origin,
        RateOrigin::Projected(ProjectionReason::AwaitingPublication)
    );
    assert!(bond
        .project(
            &c,
            &FixingSeries::default(),
            publication,
            BondProjectionPolicy::RequirePublished
        )
        .is_err());
}

#[test]
fn overnight_cash_and_accrued_use_actual_weekend_product_and_fixing_origins() {
    let bond = overnight_bond().build().unwrap();
    let mut seen = vec![];
    let mut forecast = |r: &ProjectionRequest| {
        seen.push((r.rate_start, r.rate_end));
        Ok(if r.observation_date == date("2024-09-30") {
            dec!(-0.01)
        } else {
            dec!(0.06)
        })
    };
    let result = bond
        .project(
            &context("2024-09-29", "2024-09-29"),
            &friday_history(),
            publication,
            BondProjectionPolicy::Supplied(&mut forecast),
        )
        .unwrap();
    let c = &result.coupons[0];
    let product = (Decimal::ONE + dec!(0.05) * dec!(3) / dec!(360))
        * (Decimal::ONE - dec!(0.01) / dec!(360))
        * (Decimal::ONE + dec!(0.06) / dec!(360));
    close(
        c.amount,
        dec!(5000) * ((product - Decimal::ONE) + dec!(0.001) * dec!(5) / dec!(360)),
    );
    close(
        c.accrued,
        dec!(5000) * (dec!(0.05) + dec!(0.001)) * dec!(2) / dec!(360),
    );
    assert_eq!(
        seen,
        vec![
            (date("2024-09-30"), date("2024-10-01")),
            (date("2024-10-01"), date("2024-10-02"))
        ]
    );
    let o = c.overnight.as_ref().unwrap();
    assert_eq!(o.observations.len(), 3);
    assert!(matches!(o.observations[0].origin, RateOrigin::Published(_)));
    assert_eq!(
        o.observations[1].origin,
        RateOrigin::Projected(ProjectionReason::FutureObservation)
    );
    assert_eq!(c.payment_date, date("2024-10-04"));
}

#[test]
fn overnight_lockout_reuses_rate_and_lookback_does_not_shift_day_weights() {
    let mut t = overnight_bond();
    if let BondCoupon::Overnight(r) = &mut t.coupon {
        r.convention.lockout_business_days = 1;
    }
    let bond = t.build().unwrap();
    let mut called = vec![];
    let mut forecast = |r: &ProjectionRequest| {
        called.push(r.observation_date);
        Ok(dec!(0.04))
    };
    let projected = bond
        .project(
            &context("2024-09-29", "2024-09-29"),
            &friday_history(),
            publication,
            BondProjectionPolicy::Supplied(&mut forecast),
        )
        .unwrap();
    assert_eq!(called, vec![date("2024-09-30")]);
    let o = projected.coupons[0].overnight.as_ref().unwrap();
    assert_eq!(
        o.observations[1].dates.observation_date,
        o.observations[2].dates.observation_date
    );
    let mut t = overnight_bond();
    if let BondCoupon::Overnight(r) = &mut t.coupon {
        r.convention.lookback_business_days = 2;
    }
    let bond = t.build().unwrap();
    let requirements = bond
        .fixing_requirements(
            date("2024-09-27"),
            PaymentDateEntitlement::Exclude,
            publication,
        )
        .unwrap();
    assert_eq!(
        requirements
            .iter()
            .map(|r| r.observation_date)
            .collect::<Vec<_>>(),
        vec![date("2024-09-25"), date("2024-09-26"), date("2024-09-27")]
    );
    let history = FixingSeries::new(vec![
        observation(
            "SUPPLIED.ON",
            "2024-09-25",
            dec!(0.03),
            "2024-09-25T16:00:00Z",
            1,
        ),
        observation(
            "SUPPLIED.ON",
            "2024-09-26",
            dec!(0.04),
            "2024-09-26T16:00:00Z",
            1,
        ),
        observation(
            "SUPPLIED.ON",
            "2024-09-27",
            dec!(0.05),
            "2024-09-27T16:00:00Z",
            1,
        ),
    ])
    .unwrap();
    let result = bond
        .project(
            &context("2024-09-29", "2024-09-29"),
            &history,
            publication,
            BondProjectionPolicy::RequirePublished,
        )
        .unwrap();
    close(
        result.coupons[0].overnight.as_ref().unwrap().observations[0].year_fraction,
        dec!(3) / dec!(360),
    );
}

#[test]
fn shifted_overnight_accrual_requires_explicit_prefix_and_rejects_unused_or_bad_cutoffs() {
    let mut t = overnight_bond();
    if let BondCoupon::Overnight(r) = &mut t.coupon {
        r.convention.observation_shift = true;
        r.convention.lookback_business_days = 2;
    }
    let bond = t.build().unwrap();
    let history = FixingSeries::new(vec![
        observation(
            "SUPPLIED.ON",
            "2024-09-25",
            dec!(0.03),
            "2024-09-25T16:00:00Z",
            1,
        ),
        observation(
            "SUPPLIED.ON",
            "2024-09-26",
            dec!(0.04),
            "2024-09-26T16:00:00Z",
            1,
        ),
        observation(
            "SUPPLIED.ON",
            "2024-09-27",
            dec!(0.05),
            "2024-09-27T16:00:00Z",
            1,
        ),
    ])
    .unwrap();
    let mut c = context("2024-09-29", "2024-09-29");
    assert!(matches!(
        bond.project(
            &c,
            &history,
            publication,
            BondProjectionPolicy::RequirePublished
        ),
        Err(BondContractError::Unsupported { .. })
    ));
    c.overnight_accrual_cutoffs = vec![OvernightAccrualCutoff {
        period_start: date("2024-09-27"),
        weight_end: date("2024-09-27"),
    }];
    let p = bond
        .project(
            &c,
            &history,
            publication,
            BondProjectionPolicy::RequirePublished,
        )
        .unwrap();
    close(
        p.accrued,
        dec!(5000)
            * ((Decimal::ONE + dec!(0.03) / dec!(360)) * (Decimal::ONE + dec!(0.04) / dec!(360))
                - Decimal::ONE)
            + dec!(5000) * dec!(0.001) * dec!(2) / dec!(360),
    );
    c.overnight_accrual_cutoffs[0].weight_end = date("2024-10-10");
    assert!(bond
        .project(
            &c,
            &history,
            publication,
            BondProjectionPolicy::RequirePublished
        )
        .is_err());
    c.overnight_accrual_cutoffs[0].period_start = date("2024-09-26");
    assert!(bond
        .project(
            &c,
            &history,
            publication,
            BondProjectionPolicy::RequirePublished
        )
        .is_err());
}

#[test]
fn overnight_cannot_average_intracoupon_principal_or_gearing_and_checks_effective_negative_policy()
{
    for kind in ["notional", "gearing"] {
        let mut t = overnight_bond();
        let split = date("2024-09-30");
        let p = if kind == "notional" {
            &mut t.profiles.notional
        } else {
            &mut t.profiles.gearing
        };
        let old = p[0];
        p[0].end = split;
        p.push(DatedValue {
            start: split,
            end: old.end,
            value: old.value / dec!(2),
        });
        assert!(matches!(
            t.build(),
            Err(BondContractError::Unsupported { .. })
        ));
    }
    let mut t = overnight_bond();
    t.negative_coupons = NegativeCouponPolicy::Reject;
    t.profiles.spread[0].value = dec!(-0.01);
    let bond = t.build().unwrap();
    let mut forecast = |_: &ProjectionRequest| Ok(dec!(0.05));
    assert!(bond
        .project(
            &context("2024-09-29", "2024-09-29"),
            &friday_history(),
            publication,
            BondProjectionPolicy::Supplied(&mut forecast)
        )
        .is_ok());
}

#[test]
fn invalid_profiles_reset_grids_and_bound_violations_never_become_simpler_bonds() {
    let base = terms("independent-steps-mid-coupon-amortization");
    let mut bad = vec![];
    let mut t = base.clone();
    t.profiles.coupon[0].end = date("2024-04-02");
    bad.push(t);
    let mut t = base.clone();
    t.profiles.coupon[0].end = date("2024-03-31");
    bad.push(t);
    let mut t = base.clone();
    t.profiles.coupon.reverse();
    bad.push(t);
    let mut t = base.clone();
    t.profiles.notional[1].value = dec!(6000);
    bad.push(t);
    let mut t = base.clone();
    t.profiles.gearing[0].value = Decimal::ONE;
    bad.push(t);
    let mut t = base.clone();
    t.profiles.notional[0].value = dec!(4999);
    bad.push(t);
    let mut t = base.clone();
    t.calendars.payment = CalendarId::new("unknown");
    bad.push(t);
    let mut t = base.clone();
    t.dates.payment_delay = DateOffset::BusinessDays(367);
    bad.push(t);
    let mut t = base.clone();
    t.rounding = CashRounding::NearestEven(29);
    bad.push(t);
    let mut t = base.clone();
    t.schema_version = 2;
    bad.push(t);
    let mut t = base.clone();
    t.redemption_multiplier = dec!(11);
    bad.push(t);
    let mut t = base.clone();
    t.profiles.coupon = vec![t.profiles.coupon[0]; 4097];
    bad.push(t);
    let mut t = term_bond();
    if let BondCoupon::Term(r) = &mut t.coupon {
        r.resets.start_date = date("2024-02-01");
    }
    bad.push(t);
    let mut t = term_bond();
    if let BondCoupon::Term(r) = &mut t.coupon {
        r.resets.first_regular_date = Some(date("2024-03-15"));
        r.resets.penultimate_date = Some(date("2024-02-01"));
    }
    bad.push(t);
    for (i, t) in bad.into_iter().enumerate() {
        assert!(t.build().is_err(), "bad variant {i}");
    }
    let bond = base.build().unwrap();
    assert!(bond
        .fixed_projection(date("2023-12-31"), PaymentDateEntitlement::Exclude)
        .is_err());
    assert!(bond
        .fixed_projection(date("2026-01-01"), PaymentDateEntitlement::Exclude)
        .unwrap()
        .cashflows()
        .unwrap()
        .is_empty());
}

#[test]
fn exercise_and_callable_floating_terms_roundtrip_notice_settlement_and_model_requirement() {
    let mut t = term_bond();
    t.exercise = vec![
        ExerciseTerms {
            right: ExerciseRight::Call,
            style: ExerciseStyle::American {
                start: date("2024-02-01"),
                end: date("2024-03-25"),
            },
            price_per_100: dec!(102.125),
            notice_offset: DateOffset::BusinessDays(-2),
            settlement_lag: DateOffset::BusinessDays(2),
            settlement_adjustment: Bdc::Following,
            accrued: ExerciseAccrued::IncludedInPrice,
            protection_end: Some(date("2024-02-05")),
        },
        ExerciseTerms {
            right: ExerciseRight::Put,
            style: ExerciseStyle::Bermudan {
                dates: vec![date("2024-02-15"), date("2024-03-15")],
            },
            price_per_100: dec!(101),
            notice_offset: DateOffset::CalendarDays(-7),
            settlement_lag: DateOffset::CalendarDays(1),
            settlement_adjustment: Bdc::Following,
            accrued: ExerciseAccrued::PayAccrued,
            protection_end: None,
        },
    ];
    let native = t.clone().build().unwrap();
    let json = serde_json::to_vec(native.terms()).unwrap();
    let reconstructed: BondTerms = serde_json::from_slice(&json).unwrap();
    assert_eq!(t, reconstructed);
    let rebuilt = reconstructed.build().unwrap();
    assert!(matches!(
        rebuilt.require_plain_valuation(),
        Err(BondContractError::Unsupported { .. })
    ));
    let dates = rebuilt.terms().exercise[0]
        .dates(date("2024-02-12"), &convex_core::calendars::WeekendCalendar)
        .unwrap();
    assert_eq!(dates.notice, date("2024-02-08"));
    assert_eq!(dates.settlement, date("2024-02-14"));
    assert!(rebuilt.terms().exercise[0]
        .dates(date("2024-02-02"), &convex_core::calendars::WeekendCalendar)
        .is_err());
    let mut value: Value = serde_json::from_slice(&json).unwrap();
    value["exercise"][0]["style"] = Value::String("make_whole".into());
    assert!(serde_json::from_value::<BondTerms>(value).is_err());
    let mut overlap = t.clone();
    overlap.exercise.push(overlap.exercise[0].clone());
    assert!(overlap.build().is_err());
    let mut invalid = t.clone();
    invalid.exercise[0].notice_offset = DateOffset::CalendarDays(1);
    assert!(invalid.build().is_err());
    let mut invalid = t;
    invalid.exercise[1].style = ExerciseStyle::Bermudan {
        dates: vec![date("2024-03-15"), date("2024-02-15")],
    };
    assert!(invalid.build().is_err());
}

fn base_fixed() -> FixedRateBond {
    FixedRateBond::builder()
        .cusip_unchecked("SUPPLIED-CVX055")
        .coupon_rate(dec!(0.05))
        .issue_date(date("2024-01-01"))
        .maturity(date("2025-01-01"))
        .face_value(dec!(5000))
        .frequency(Frequency::SemiAnnual)
        .day_count(Dc::Act360)
        .calendar(CalendarId::weekend_only())
        .business_day_convention(Bdc::Unadjusted)
        .end_of_month(false)
        .redemption_value(dec!(5500))
        .build()
        .unwrap()
}
#[test]
fn existing_instruments_compose_checked_profiles_sinks_amortization_and_exercise() {
    let fixed = base_fixed();
    let stepped = fixed
        .with_profiles(terms("independent-steps-mid-coupon-amortization").profiles)
        .unwrap();
    assert_eq!(stepped.terms().redemption_multiplier, dec!(1.1));
    let sink = SinkingFundBond::new(
        fixed.clone(),
        SinkingFundSchedule::new()
            .with_payment(SinkingFundPayment::with_price(
                date("2024-08-10"),
                20.0,
                102.0,
            ))
            .with_payment(SinkingFundPayment::with_price(
                date("2025-01-01"),
                30.0,
                103.0,
            )),
    );
    let cash = sink
        .try_contract()
        .unwrap()
        .fixed_projection(date("2024-07-01"), PaymentDateEntitlement::Exclude)
        .unwrap();
    assert_eq!(cash.principal.len(), 3);
    assert_eq!(cash.principal[0].effective_date, date("2024-08-10"));
    assert_eq!(
        cash.principal.iter().map(|p| p.amount).sum::<Decimal>(),
        dec!(5315)
    );
    assert_eq!(sink.try_cash_flows(date("2024-07-01")).unwrap().len(), 4);
    let amort = AmortizationSchedule::new(AmortizationType::Custom)
        .with_original_face(5000.0)
        .with_entry(AmortizationEntry::new(date("2024-08-10"), 20.0).with_remaining_factor(0.8));
    assert_eq!(
        fixed
            .with_amortization(&amort)
            .unwrap()
            .outstanding_principal(date("2024-08-10"), BoundarySide::After)
            .unwrap(),
        dec!(4000)
    );
    let bad = AmortizationSchedule::new(AmortizationType::Custom)
        .with_entry(AmortizationEntry::new(date("2024-08-10"), 20.0).with_remaining_factor(0.9));
    assert!(fixed.with_amortization(&bad).is_err());
    assert!(fixed
        .with_amortization(&AmortizationSchedule::new(AmortizationType::PassThrough))
        .is_err());
    for schedule in [
        SinkingFundSchedule::new().with_delivery_option(),
        SinkingFundSchedule::new().with_double_up(),
        SinkingFundSchedule::new().as_optional(),
    ] {
        assert!(SinkingFundBond::new(fixed.clone(), schedule)
            .try_contract()
            .is_err());
    }
    let callable = CallableBond::new(
        fixed.clone(),
        CallSchedule::new(CallType::Bermudan)
            .with_entry(CallEntry::new(date("2024-07-01"), 102.0).with_notice_days(10)),
    )
    .with_put_schedule(
        PutSchedule::new(PutType::European).with_entry(PutEntry::new(date("2024-08-15"), 101.0)),
    );
    let terms = callable.contract_terms().unwrap();
    assert_eq!(terms.exercise.len(), 2);
    assert_eq!(
        terms.exercise[0].notice_offset,
        DateOffset::CalendarDays(-10)
    );
    let rebuilt: BondTerms = serde_json::from_str(&serde_json::to_string(&terms).unwrap()).unwrap();
    assert_eq!(rebuilt, terms);
    rebuilt.build().unwrap();
    assert!(CallableBond::new(fixed, CallSchedule::make_whole(20.0))
        .contract_terms()
        .is_err());
}

#[test]
fn native_frn_checked_routes_require_fixings_and_preserve_callable_floating_terms() {
    let mut frn = FloatingRateNote::builder()
        .index(RateIndex::Euribor3M)
        .spread_bps(20)
        .face_value(dec!(5000))
        .issue_date(date("2023-12-29"))
        .maturity(date("2024-12-29"))
        .day_count(Dc::Act360)
        .build()
        .unwrap();
    frn.set_current_rate(dec!(0.09));
    assert!(frn.try_cash_flows(date("2024-03-01")).is_err());
    assert!(frn.try_accrued_interest(date("2024-03-01")).is_err());
    let mut resets = term_bond().schedule;
    resets.start_date = date("2023-12-29");
    resets.end_date = date("2024-12-29");
    let coupon = BondCoupon::Term(TermRateTerms {
        index_id: IndexId::new("EUR.TERM.3M").unwrap(),
        resets,
        reset_adjustment: Bdc::Unadjusted,
        fixing_offset: DateOffset::BusinessDays(-2),
        projection_day_count: Dc::Act360,
    });
    let callable = CallableFloatingRateNote::new(
        frn,
        CallSchedule::new(CallType::European)
            .with_entry(CallEntry::new(date("2024-06-29"), 101.0).with_notice_days(20)),
    );
    let t = callable.contract_terms(coupon).unwrap();
    let decoded: BondTerms = serde_json::from_slice(&serde_json::to_vec(&t).unwrap()).unwrap();
    assert_eq!(t, decoded);
    decoded.build().unwrap();
    let mut wire = serde_json::to_value(callable.base_frn()).unwrap();
    wire["day_count"] = Value::String("unknown".into());
    assert!(serde_json::from_value::<FloatingRateNote>(wire).is_err());
    let capped = FloatingRateNote::builder()
        .index(RateIndex::Sofr)
        .cap(dec!(0.05))
        .issue_date(date("2024-09-27"))
        .maturity(date("2024-10-02"))
        .build()
        .unwrap();
    assert!(capped.contract_terms(overnight_bond().coupon).is_err());
}

#[test]
fn seeded_step_cases_preserve_exact_principal_and_do_not_average_coupon() {
    let mut seed = 0x055_c0ffe_u64;
    for _ in 0..128 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let coupon_day = 30 + (seed % 100) as i64;
        let notional_day = 180 + (seed % 100) as i64;
        let mut t = terms("independent-steps-mid-coupon-amortization");
        let start = t.schedule.start_date;
        let end = t.schedule.end_date;
        let coupon_step = start.checked_add_days(coupon_day).unwrap();
        let principal_step = start.checked_add_days(notional_day).unwrap();
        t.profiles.coupon = vec![
            DatedValue {
                start,
                end: coupon_step,
                value: dec!(0.04),
            },
            DatedValue {
                start: coupon_step,
                end,
                value: dec!(0.08),
            },
        ];
        t.profiles.notional = vec![
            DatedValue {
                start,
                end: principal_step,
                value: dec!(5000),
            },
            DatedValue {
                start: principal_step,
                end,
                value: dec!(3000),
            },
        ];
        let result = t
            .build()
            .unwrap()
            .fixed_projection(start, PaymentDateEntitlement::Exclude)
            .unwrap();
        let expected = (dec!(5000) * dec!(0.04) * Decimal::from(coupon_day)
            + dec!(5000) * dec!(0.08) * Decimal::from(notional_day - coupon_day)
            + dec!(3000) * dec!(0.08) * Decimal::from(366 - notional_day))
            / dec!(360);
        close(result.coupons[0].amount, expected);
        assert_eq!(
            result
                .principal
                .iter()
                .map(|p| p.principal.repayment)
                .sum::<Decimal>(),
            dec!(5000)
        );
    }
}

#[test]
fn front_back_short_long_stubs_keep_steps_and_exact_total_actual_accrual() {
    for stub in [
        StubType::ShortFirst,
        StubType::LongFirst,
        StubType::ShortLast,
        StubType::LongLast,
    ] {
        let mut t = terms("independent-steps-mid-coupon-amortization");
        t.issue_date = date("2023-12-01");
        t.schedule.start_date = date("2023-12-01");
        t.schedule.end_date = date("2025-01-15");
        t.schedule.frequency = Frequency::SemiAnnual;
        t.schedule.stub_type = stub;
        t.profiles = BondProfiles::constant(
            t.schedule.start_date,
            t.schedule.end_date,
            dec!(0.04),
            dec!(0),
            dec!(0),
            dec!(5000),
        );
        t.profiles.coupon = vec![
            DatedValue {
                start: t.schedule.start_date,
                end: date("2024-06-01"),
                value: dec!(0.04),
            },
            DatedValue {
                start: date("2024-06-01"),
                end: t.schedule.end_date,
                value: dec!(0.08),
            },
        ];
        let start = t.schedule.start_date;
        let bond = t.build().unwrap();
        let periods = bond.periods().collect::<Vec<_>>();
        assert_eq!(periods[0].contractual_dates().0, start);
        assert_eq!(
            periods.last().unwrap().contractual_dates().1,
            date("2025-01-15")
        );
        let projection = bond
            .fixed_projection(start, PaymentDateEntitlement::Exclude)
            .unwrap();
        let expected = dec!(5000)
            * (dec!(0.04) * Decimal::from(start.days_between(&date("2024-06-01")))
                + dec!(0.08) * Decimal::from(date("2024-06-01").days_between(&date("2025-01-15"))))
            / dec!(360);
        close(projection.coupons.iter().map(|p| p.amount).sum(), expected);
        assert_eq!(
            projection
                .principal
                .iter()
                .map(|p| p.principal.repayment)
                .sum::<Decimal>(),
            dec!(5000)
        );
    }
}

#[test]
fn future_projection_moves_never_rewrite_known_term_rates_or_accrued() {
    let bond = term_bond().build().unwrap();
    let history = FixingSeries::new(vec![
        observation(
            "SUPPLIED.TERM",
            "2024-01-01",
            dec!(0.03),
            "2024-01-01T16:00:00Z",
            1,
        ),
        observation(
            "SUPPLIED.TERM",
            "2024-02-01",
            dec!(0.06),
            "2024-02-01T16:00:00Z",
            1,
        ),
    ])
    .unwrap();
    let context = context("2024-02-15", "2024-02-15");
    let mut low = |_: &ProjectionRequest| Ok(dec!(0.01));
    let mut high = |_: &ProjectionRequest| Ok(dec!(0.10));
    let a = bond
        .project(
            &context,
            &history,
            publication,
            BondProjectionPolicy::Supplied(&mut low),
        )
        .unwrap();
    let b = bond
        .project(
            &context,
            &history,
            publication,
            BondProjectionPolicy::Supplied(&mut high),
        )
        .unwrap();
    assert_eq!(a.accrued, b.accrued);
    assert_eq!(a.coupons[0].rates[..2], b.coupons[0].rates[..2]);
    close(
        b.coupons[0].amount - a.coupons[0].amount,
        dec!(5000) * dec!(0.09) * dec!(31) / dec!(360),
    );
}
