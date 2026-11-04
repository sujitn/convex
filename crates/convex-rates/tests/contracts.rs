//! Independent supplied-contract date/cash fixtures, history and rejection tests.
use convex_core::{
    calendars::Calendar,
    cashflows::{
        BoundarySide, CashRounding, CashflowError, DateOffset, DatedValue, NegativeCouponPolicy,
        PaymentDateEntitlement,
    },
    contracts::{IndexId, SourceId, UtcTimestamp, Version},
    daycounts::DayCountConvention,
    fixings::{
        FixingContext, FixingObservation, FixingSeries, FixingStatus, ProjectionReason, RateOrigin,
        UnpublishedFixingPolicy,
    },
    Currency, Date,
};
use convex_rates::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    str::FromStr,
};

fn d(s: &str) -> Date {
    Date::parse(s).unwrap()
}
fn ts(s: &str) -> UtcTimestamp {
    serde_json::from_value(json!(s)).unwrap()
}
fn number(v: &Value) -> Decimal {
    Decimal::from_str(v.as_str().unwrap()).unwrap()
}

#[derive(Deserialize)]
struct Fixture {
    cases: Vec<Case>,
}
#[derive(Deserialize, Clone)]
struct Case {
    id: String,
    terms: SwapTerms,
    calendar_holidays: BTreeMap<String, Vec<String>>,
    rates: BTreeMap<String, String>,
    constant_rate: Option<String>,
    expected: Value,
}
fn fixture() -> Vec<Case> {
    serde_json::from_str::<Fixture>(include_str!("fixtures/swaps.json"))
        .unwrap()
        .cases
}
struct Holidays(BTreeSet<Date>);
impl Calendar for Holidays {
    fn name(&self) -> &'static str {
        "supplied contract fixture"
    }
    fn is_business_day(&self, at: Date) -> bool {
        !at.is_weekend() && !self.0.contains(&at)
    }
}
fn calendars(c: &Case) -> BTreeMap<String, Holidays> {
    c.calendar_holidays
        .iter()
        .map(|(id, dates)| (id.clone(), Holidays(dates.iter().map(|s| d(s)).collect())))
        .collect()
}
fn build(t: &SwapTerms, c: &BTreeMap<String, Holidays>) -> Result<SwapContract, SwapError> {
    fn bind<'a>(pin: &'a ReferencePin, c: &'a BTreeMap<String, Holidays>) -> CalendarBinding<'a> {
        CalendarBinding {
            pin,
            calendar: c.get(pin.id.as_str()).unwrap(),
        }
    }
    fn leg<'a>(pins: &'a LegCalendarPins, c: &'a BTreeMap<String, Holidays>) -> LegCalendars<'a> {
        LegCalendars {
            accrual: bind(&pins.accrual, c),
            payment: bind(&pins.payment, c),
        }
    }
    let (reset, fixing, observation) = match &t.floating {
        FloatingLegTerms::Term(f) => (
            Some(bind(&f.resets.reset_calendar, c)),
            Some(bind(&f.resets.fixing_calendar, c)),
            None,
        ),
        FloatingLegTerms::Overnight(f) => (None, None, Some(bind(&f.observation_calendar, c))),
    };
    t.clone().build(&SwapCalendars {
        fixed: leg(&t.fixed.leg.calendars, c),
        floating: leg(&t.floating.leg().calendars, c),
        reset,
        fixing,
        observation,
    })
}
fn context(at: Date) -> CashflowContext {
    CashflowContext {
        fixings: FixingContext {
            valuation_date: at,
            known_at: ts(&format!("{at}T23:59:59Z")),
            unpublished: UnpublishedFixingPolicy::Forecast,
            final_only: true,
        },
        on_payment: PaymentDateEntitlement::Include,
    }
}
fn publication(_: &IndexId, at: Date) -> Result<UtcTimestamp, CashflowError> {
    Ok(ts(&format!("{at}T08:00:00Z")))
}
fn forecast(c: &Case, r: &ProjectionRequest) -> Result<ForecastRate, CashflowError> {
    let rate = c
        .rates
        .get(&r.requirement.observation_date.to_string())
        .or(c.constant_rate.as_ref())
        .unwrap();
    Ok(ForecastRate {
        rate: rate.parse().unwrap(),
        projection_curve: r.projection_curve.clone(),
    })
}
fn observation(
    index: &IndexId,
    at: Date,
    rate: Decimal,
    revision: u64,
    known: UtcTimestamp,
) -> FixingObservation {
    FixingObservation {
        index_id: index.clone(),
        observation_date: at,
        rate,
        published_at: ts(&format!("{at}T08:00:00Z")),
        known_at: known,
        revision: Version::new(revision).unwrap(),
        status: FixingStatus::Final,
        source_id: SourceId::new("fixture").unwrap(),
    }
}
fn historical(t: &SwapContract, ctx: CashflowContext, c: &Case) -> FixingSeries {
    let req = t.fixing_requirements(ctx, publication).unwrap();
    FixingSeries::new(
        req.iter()
            .filter(|r| r.observation_date <= ctx.fixings.valuation_date)
            .map(|r| {
                let rate = c
                    .rates
                    .get(&r.observation_date.to_string())
                    .or(c.constant_rate.as_ref())
                    .unwrap()
                    .parse()
                    .unwrap();
                observation(
                    &r.index_id,
                    r.observation_date,
                    rate,
                    1,
                    r.expected_publication,
                )
            })
            .collect(),
    )
    .unwrap()
}
fn assert_period(p: &convex_core::cashflows::CouponPeriod, v: &Value) {
    assert_eq!(
        json!([
            p.contractual_dates().0.to_string(),
            p.contractual_dates().1.to_string()
        ]),
        v["contractual"]
    );
    assert_eq!(
        json!([
            p.accrual_dates().0.to_string(),
            p.accrual_dates().1.to_string()
        ]),
        v["accrual"]
    );
    assert_eq!(p.payment_date().to_string(), v["payment"].as_str().unwrap());
}
fn close(a: Decimal, b: Decimal, tol: Decimal) {
    assert!(
        (a - b).abs() <= tol,
        "{a} != {b}, difference {}",
        (a - b).abs()
    );
}

#[test]
fn pinned_independent_stub_eom_calendar_reset_and_cash_contracts() {
    for c in fixture() {
        let cal = calendars(&c);
        let swap = build(&c.terms, &cal).unwrap_or_else(|e| panic!("{}: {e}", c.id));
        let ctx = context(c.terms.effective_date.checked_add_days(-10).unwrap());
        let history = historical(&swap, ctx, &c);
        let result = swap
            .cashflows(ctx, &history, publication, |r| forecast(&c, r))
            .unwrap();
        assert_eq!(result.terms, c.terms);
        assert_eq!(result.collateral, c.terms.collateral);
        for (cash, expected) in [
            (&result.fixed, &c.expected["fixed"]),
            (&result.floating, &c.expected["floating"]),
        ] {
            assert_eq!(cash.len(), expected.as_array().unwrap().len(), "{}", c.id);
            for (p, v) in cash.iter().zip(expected.as_array().unwrap()) {
                assert_period(&p.period, v);
                close(p.amount, number(&v["amount"]), dec!(0.000000000000000001));
                match &p.evidence {
                    CouponEvidence::Term(slices) => {
                        assert_eq!(slices.len(), v["resets"].as_array().unwrap().len());
                        for (s, e) in slices.iter().zip(v["resets"].as_array().unwrap()) {
                            assert_eq!(
                                json!([
                                    s.dates.accrual_start.to_string(),
                                    s.dates.accrual_end.to_string()
                                ]),
                                e["accrual"]
                            );
                            assert_eq!(
                                s.dates.observation_date.to_string(),
                                e["observation"].as_str().unwrap()
                            );
                            assert_eq!(
                                json!([
                                    s.dates.rate_start.to_string(),
                                    s.dates.rate_end.to_string()
                                ]),
                                e["rate"]
                            );
                        }
                    }
                    CouponEvidence::Overnight(coupon) => {
                        close(
                            coupon.period_factor,
                            number(&v["factor"]),
                            dec!(0.000000000000000000000001),
                        );
                        assert_eq!(
                            coupon.observations.len(),
                            v["observations"].as_array().unwrap().len()
                        );
                        for (o, e) in coupon
                            .observations
                            .iter()
                            .zip(v["observations"].as_array().unwrap())
                        {
                            assert_eq!(
                                o.dates.observation_date.to_string(),
                                e["observation"].as_str().unwrap()
                            );
                            assert_eq!(
                                o.dates.rate_end.to_string(),
                                e["rate_end"].as_str().unwrap()
                            );
                            assert_eq!(
                                json!([
                                    o.dates.weight_start.to_string(),
                                    o.dates.weight_end.to_string()
                                ]),
                                e["weight"]
                            );
                            assert_eq!(
                                o.dates.weight_start.days_between(&o.dates.weight_end),
                                e["days"].as_i64().unwrap()
                            );
                        }
                    }
                    CouponEvidence::Fixed => (),
                }
            }
        }
        if let Some(reset) = swap.term_reset_schedule() {
            assert_eq!(
                reset.periods().len(),
                c.expected["reset_grid"].as_array().unwrap().len()
            );
            for (p, v) in reset
                .periods()
                .iter()
                .zip(c.expected["reset_grid"].as_array().unwrap())
            {
                assert_eq!(
                    json!([
                        p.contractual_dates().0.to_string(),
                        p.contractual_dates().1.to_string()
                    ]),
                    v["contractual"]
                );
                assert_eq!(
                    json!([
                        p.accrual_dates().0.to_string(),
                        p.accrual_dates().1.to_string()
                    ]),
                    v["accrual"]
                );
            }
        }
        // Exact signed cash offsets for receiver/payer, with no principal cash.
        let mut receiver = c.terms.clone();
        receiver.side = SwapSide::ReceiveFixed;
        let reversed = build(&receiver, &cal)
            .unwrap()
            .cashflows(ctx, &history, publication, |r| forecast(&c, r))
            .unwrap();
        for (left, right) in result
            .fixed
            .iter()
            .chain(&result.floating)
            .zip(reversed.fixed.iter().chain(&reversed.floating))
        {
            assert_eq!(left.signed_amount, -right.signed_amount);
        }
    }
}

#[test]
fn lookback_shift_lockout_have_distinct_contractual_weights_and_cash() {
    let cases = fixture();
    let cash = |id: &str| {
        let c = cases.iter().find(|c| c.id == id).unwrap();
        let cal = calendars(c);
        let s = build(&c.terms, &cal).unwrap();
        s.cashflows(
            context(d("2024-03-20")),
            &FixingSeries::default(),
            publication,
            |r| forecast(c, r),
        )
        .unwrap()
    };
    let a = cash("ois-lookback");
    let b = cash("ois-shift");
    let lock = cash("ois-lockout");
    assert_ne!(a.floating[0].amount, b.floating[0].amount);
    assert_ne!(a.floating[0].amount, lock.floating[0].amount);
    assert_eq!(lock.rates.len(), 2); // 4 weights consume only 2 unique observations.
    let weights = |result: &SwapCashflows| match &result.floating[0].evidence {
        CouponEvidence::Overnight(c) => c
            .observations
            .iter()
            .map(|o| o.dates.weight_start.days_between(&o.dates.weight_end))
            .collect::<Vec<_>>(),
        _ => panic!("wrong evidence"),
    };
    assert_eq!(weights(&a), vec![1, 4, 1, 1]);
    assert_eq!(weights(&b), vec![1, 1, 1, 4]);
}

#[test]
fn joint_shift_lockout_and_overnight_boundary_notional_changes_are_explicit() {
    let mut c = fixture()[13].clone();
    if let FloatingLegTerms::Overnight(t) = &mut c.terms.floating {
        t.convention.observation_shift = true;
    }
    let cal = calendars(&c);
    let s = build(&c.terms, &cal).unwrap();
    let result = s
        .cashflows(
            context(d("2024-03-20")),
            &FixingSeries::default(),
            publication,
            |r| forecast(&c, r),
        )
        .unwrap();
    let CouponEvidence::Overnight(coupon) = &result.floating[0].evidence else {
        panic!("wrong mechanics")
    };
    let expected = (Decimal::ONE + dec!(0.01) / dec!(360))
        * (Decimal::ONE + dec!(0.02) / dec!(360))
        * (Decimal::ONE + dec!(0.02) / dec!(360))
        * (Decimal::ONE + dec!(0.02) * dec!(4) / dec!(360));
    close(
        coupon.period_factor,
        expected,
        dec!(0.000000000000000000000001),
    );
    assert_eq!(result.rates.len(), 2);
    if let FloatingLegTerms::Overnight(t) = &mut c.terms.floating {
        t.convention.observation_shift = false;
        t.convention.lookback_business_days = 0;
        t.convention.lockout_business_days = 0;
        t.leg.schedule.end_date = d("2024-06-04");
        t.leg.schedule.stub_type = convex_core::cashflows::StubType::None;
        t.leg.notionals = vec![
            DatedValue {
                start: d("2024-04-04"),
                end: d("2024-05-04"),
                value: dec!(1000000),
            },
            DatedValue {
                start: d("2024-05-04"),
                end: d("2024-06-04"),
                value: dec!(750000),
            },
        ];
        t.profiles.spreads[0].end = d("2024-06-04");
        t.profiles.spreads[0].value = Decimal::ZERO;
        t.profiles.gearings[0].end = d("2024-06-04");
    }
    c.terms.maturity_date = d("2024-06-04");
    c.terms.fixed.leg.schedule.end_date = c.terms.maturity_date;
    c.terms.fixed.leg.schedule.stub_type = convex_core::cashflows::StubType::None;
    c.terms.fixed.leg.notionals[0].end = c.terms.maturity_date;
    c.terms.fixed.rates[0].end = c.terms.maturity_date;
    let run = |terms: &SwapTerms| {
        build(terms, &cal)
            .unwrap()
            .cashflows(
                context(d("2024-03-20")),
                &FixingSeries::default(),
                publication,
                |r| {
                    Ok(ForecastRate {
                        rate: dec!(0.04),
                        projection_curve: r.projection_curve.clone(),
                    })
                },
            )
            .unwrap()
    };
    let amortized = run(&c.terms);
    if let FloatingLegTerms::Overnight(t) = &mut c.terms.floating {
        t.leg.notionals[1].value = dec!(1000000);
    }
    let flat = run(&c.terms);
    assert_eq!(amortized.floating[0].amount, flat.floating[0].amount);
    close(
        amortized.floating[1].amount,
        flat.floating[1].amount * dec!(0.75),
        dec!(0.000000000000000001),
    );
}

#[test]
fn entire_missing_history_precedes_forecast_even_across_resolution_batches() {
    let c = &fixture()[9];
    let cal = calendars(c);
    let s = build(&c.terms, &cal).unwrap();
    let ctx = context(d("2024-09-20"));
    let req = s.fixing_requirements(ctx, publication).unwrap();
    let mut forecasts = 0;
    let e = s
        .cashflows(ctx, &FixingSeries::default(), publication, |_| {
            forecasts += 1;
            Err(CashflowError::Arithmetic("must not execute"))
        })
        .unwrap_err();
    let SwapError::Cashflow(CashflowError::MissingFixings(missing)) = e else {
        panic!("wrong error")
    };
    assert_eq!(
        missing,
        req.into_iter()
            .filter(|r| r.observation_date <= ctx.fixings.valuation_date)
            .collect::<Vec<_>>()
    );
    assert_eq!(forecasts, 0);
    // A 40-year swap retains >4096 outstanding observations across coupons.
    // Missing due history still blocks projections throughout its future tail.
    let mut long = fixture()[10].clone();
    long.terms.maturity_date = d("2064-04-04");
    {
        let leg = &mut long.terms.fixed.leg;
        leg.schedule.end_date = long.terms.maturity_date;
        leg.schedule.frequency = convex_core::types::Frequency::Annual;
        leg.schedule.stub_type = convex_core::cashflows::StubType::None;
        leg.notionals[0].end = long.terms.maturity_date;
    }
    long.terms.fixed.rates[0].end = long.terms.maturity_date;
    if let FloatingLegTerms::Overnight(t) = &mut long.terms.floating {
        t.leg = long.terms.fixed.leg.clone();
        t.profiles.spreads[0].end = long.terms.maturity_date;
        t.profiles.gearings[0].end = long.terms.maturity_date;
    }
    let s = build(&long.terms, &cal).unwrap();
    let ctx = context(d("2039-01-01"));
    let req = s.fixing_requirements(ctx, publication).unwrap();
    assert!(req.len() > 4096);
    let known: Vec<_> = req
        .iter()
        .filter(|r| r.observation_date <= ctx.fixings.valuation_date)
        .collect();
    let missing_date = known[known.len() - 1].observation_date;
    let hist = FixingSeries::new(
        known
            .iter()
            .filter(|r| r.observation_date != missing_date)
            .map(|r| {
                observation(
                    &r.index_id,
                    r.observation_date,
                    dec!(0.04),
                    1,
                    r.expected_publication,
                )
            })
            .collect(),
    )
    .unwrap();
    let mut calls = 0;
    let err = s
        .cashflows(ctx, &hist, publication, |_| {
            calls += 1;
            Err(CashflowError::Arithmetic("must not execute"))
        })
        .unwrap_err();
    assert!(
        matches!(err,SwapError::Cashflow(CashflowError::MissingFixings(ref m)) if m.len()==1 && m[0].observation_date==missing_date)
    );
    assert_eq!(calls, 0);
}

#[test]
fn known_term_reset_revisions_cutoffs_and_future_rates_are_separate() {
    let c = &fixture()[6];
    let cal = calendars(c);
    let s = build(&c.terms, &cal).unwrap();
    let ctx = context(d("2024-05-15"));
    let req = s.fixing_requirements(ctx, publication).unwrap();
    let past: Vec<_> = req
        .iter()
        .filter(|r| r.observation_date <= ctx.fixings.valuation_date)
        .collect();
    let mut obs: Vec<_> = past
        .iter()
        .map(|r| {
            observation(
                &r.index_id,
                r.observation_date,
                dec!(0.02),
                1,
                r.expected_publication,
            )
        })
        .collect();
    let last = past.last().unwrap();
    obs.push(observation(
        &last.index_id,
        last.observation_date,
        dec!(0.025),
        2,
        ts("2024-05-15T12:00:00Z"),
    ));
    obs.push(observation(
        &last.index_id,
        last.observation_date,
        dec!(0.99),
        3,
        ts("2024-05-16T12:00:00Z"),
    ));
    let future = req
        .iter()
        .find(|r| r.observation_date > ctx.fixings.valuation_date)
        .unwrap();
    obs.push(observation(
        &future.index_id,
        future.observation_date,
        dec!(0.7),
        1,
        ts(&format!("{}T12:00:00Z", future.observation_date)),
    ));
    let hist = FixingSeries::new(obs).unwrap();
    let run = |f| {
        s.cashflows(ctx, &hist, publication, |r| {
            Ok(ForecastRate {
                rate: f,
                projection_curve: r.projection_curve.clone(),
            })
        })
        .unwrap()
    };
    let a = run(dec!(0.04));
    let b = run(dec!(0.06));
    let known = |r: &SwapCashflows| {
        r.rates
            .iter()
            .filter(|v| matches!(v.resolved.origin, RateOrigin::Published(_)))
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(known(&a), known(&b));
    assert!(a.rates.iter().any(|r| r.resolved.rate == dec!(0.025)));
    assert!(!a
        .rates
        .iter()
        .any(|r| r.resolved.rate == dec!(0.99) || r.resolved.rate == dec!(0.7)));
    assert!(a
        .rates
        .iter()
        .filter(|r| r.projection_source.is_some())
        .all(|r| r.resolved.origin == RateOrigin::Projected(ProjectionReason::FutureObservation)));
    assert_ne!(
        a.floating.last().unwrap().amount,
        b.floating.last().unwrap().amount
    );
    assert_eq!(a.floating[0].amount, b.floating[0].amount);
}

#[test]
fn partially_known_overnight_preserves_published_rates_under_forecast_changes() {
    let c = &fixture()[10];
    let cal = calendars(c);
    let s = build(&c.terms, &cal).unwrap();
    let ctx = context(d("2024-04-09"));
    let hist = historical(&s, ctx, c);
    let mut called = Vec::new();
    let a = s
        .cashflows(ctx, &hist, publication, |r| {
            called.push(r.requirement.observation_date);
            forecast(c, r)
        })
        .unwrap();
    assert_eq!(called, vec![d("2024-04-10")]);
    let b = s
        .cashflows(ctx, &hist, publication, |r| {
            Ok(ForecastRate {
                rate: dec!(0.1),
                projection_curve: r.projection_curve.clone(),
            })
        })
        .unwrap();
    for (l, r) in a.rates.iter().zip(&b.rates).take(3) {
        assert_eq!(l, r);
        assert!(l.projection_source.is_none());
    }
    assert_ne!(a.floating[0].amount, b.floating[0].amount);
}

#[test]
fn unpublished_final_only_policy_and_projection_source_fail_closed() {
    let c = &fixture()[10];
    let cal = calendars(c);
    let s = build(&c.terms, &cal).unwrap();
    let mut ctx = context(d("2024-04-04"));
    ctx.fixings.known_at = ts("2024-04-04T07:00:00Z");
    let result = s
        .cashflows(ctx, &FixingSeries::default(), publication, |r| {
            forecast(c, r)
        })
        .unwrap();
    assert_eq!(
        result.rates[0].resolved.origin,
        RateOrigin::Projected(ProjectionReason::AwaitingPublication)
    );
    let mut preliminary = observation(
        c.terms.floating.index_id(),
        d("2024-04-04"),
        dec!(0.01),
        1,
        ts("2024-04-04T08:00:00Z"),
    );
    preliminary.status = FixingStatus::Preliminary;
    let hist = FixingSeries::new(vec![preliminary]).unwrap();
    assert!(matches!(
        s.cashflows(context(d("2024-04-04")), &hist, publication, |r| forecast(
            c, r
        )),
        Err(SwapError::Cashflow(CashflowError::MissingFixings(_)))
    ));
    let mut strict = c.terms.clone();
    if let FloatingLegTerms::Overnight(t) = &mut strict.floating {
        t.fixing_policy.unpublished = UnpublishedFixingPolicy::RequirePublished;
    }
    let s2 = build(&strict, &cal).unwrap();
    ctx.fixings.unpublished = UnpublishedFixingPolicy::RequirePublished;
    assert!(matches!(
        s2.cashflows(ctx, &hist, publication, |r| forecast(c, r)),
        Err(SwapError::Cashflow(CashflowError::MissingFixings(_)))
    ));
    ctx.fixings.unpublished = UnpublishedFixingPolicy::Forecast;
    assert!(s2.fixing_requirements(ctx, publication).is_err());
    let mut source = c.terms.floating.projection_curve().clone();
    source.version = "wrong".into();
    assert!(s
        .cashflows(
            context(d("2024-03-20")),
            &FixingSeries::default(),
            publication,
            |_| Ok(ForecastRate {
                rate: dec!(0.04),
                projection_curve: source.clone()
            })
        )
        .is_err());
}

#[test]
fn payment_date_state_delay_maturity_and_notional_do_not_record_cash() {
    let c = &fixture()[0];
    let cal = calendars(c);
    let s = build(&c.terms, &cal).unwrap();
    let pay = s.fixed_schedule().periods()[0].payment_date();
    let before = context(pay);
    let hist = historical(&s, before, c);
    let a = s
        .cashflows(before, &hist, publication, |r| forecast(c, r))
        .unwrap();
    let mut after = before;
    after.on_payment = PaymentDateEntitlement::Exclude;
    let b = s
        .cashflows(after, &hist, publication, |r| forecast(c, r))
        .unwrap();
    assert_eq!(a.fixed.len(), b.fixed.len() + 1);
    assert_eq!(a.floating.len(), b.floating.len() + 1);
    let final_payment = s.fixed_schedule().periods().last().unwrap().payment_date();
    let cash = s
        .cashflows(
            context(final_payment.checked_add_days(1).unwrap()),
            &FixingSeries::default(),
            publication,
            |_| panic!("matured projection"),
        )
        .unwrap();
    assert!(cash.fixed.is_empty() && cash.floating.is_empty() && cash.rates.is_empty());
    assert_eq!(
        s.outstanding_notional(true, c.terms.maturity_date, BoundarySide::Before)
            .unwrap(),
        dec!(1000000)
    );
    assert_eq!(
        s.outstanding_notional(true, c.terms.maturity_date, BoundarySide::After)
            .unwrap(),
        Decimal::ZERO
    );
    assert_eq!(
        s.outstanding_notional(false, c.terms.effective_date, BoundarySide::Before)
            .unwrap(),
        Decimal::ZERO
    );
}

#[test]
fn strict_exact_json_preserves_trade_profiles_and_rejects_ignored_payoffs() {
    let mut c = fixture()[0].clone();
    c.terms.fixed.leg.notionals[0].value = dec!(1000000.000000000000000000001);
    let serialized = serde_json::to_value(&c.terms).unwrap();
    assert_eq!(
        serialized["fixed"]["leg"]["notionals"][0]["value"],
        json!("1000000.000000000000000000001")
    );
    let parsed: SwapTerms = serde_json::from_value(serialized.clone()).unwrap();
    assert_eq!(parsed, c.terms);
    assert_eq!(build(&parsed, &calendars(&c)).unwrap().terms(), &c.terms);
    for pointer in [
        "/fixed",
        "/floating",
        "/floating/resets",
        "/fixed/leg/dates",
        "/collateral",
        "",
    ] {
        let mut bad = serialized.clone();
        bad.pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("cap".into(), json!(0.04));
        assert!(
            serde_json::from_value::<SwapTerms>(bad).is_err(),
            "{pointer}"
        );
    }
    let mut bad = serialized.clone();
    bad["fixed"]["leg"]["notionals"][0]["value"] = json!(1000000.01);
    assert!(serde_json::from_value::<SwapTerms>(bad).is_err());
    let mut bad = serialized;
    bad["fixed"]["leg"]["notionals"] = json!(vec![
        json!({"start":"2024-01-31","end":"2024-07-31","value":"1"});
        4097
    ]);
    assert!(serde_json::from_value::<SwapTerms>(bad).is_err());
}

#[test]
fn icma_long_stub_keeps_actual_reference_periods_and_owed_cash() {
    let mut c = fixture()[2].clone();
    c.terms.fixed.leg.day_count = DayCountConvention::ActActIcma;
    let cal = calendars(&c);
    let s = build(&c.terms, &cal).unwrap();
    let p = &s.fixed_schedule().periods()[0];
    let refs = p.reference_periods();
    assert_eq!(
        refs.iter().map(|r| (r.start, r.end)).collect::<Vec<_>>(),
        vec![
            (d("2023-10-31"), d("2024-01-31")),
            (d("2024-01-31"), d("2024-04-30"))
        ]
    );
    let expected =
        dec!(1000000) * dec!(0.04) * (Decimal::from(21) / Decimal::from(4 * 92) + dec!(0.25));
    let cash = s
        .cashflows(
            context(d("2023-12-15")),
            &FixingSeries::default(),
            publication,
            |r| forecast(&c, r),
        )
        .unwrap();
    close(cash.fixed[0].amount, expected, dec!(0.000000000000000001));
}

#[test]
fn invalid_economics_profiles_dates_and_unsupported_combinations_fail() {
    let c = fixture()[0].clone();
    let cal = calendars(&c);
    let mut bad = c.terms.clone();
    bad.schema_version = 2;
    assert!(build(&bad, &cal).is_err());
    let mut bad = c.terms.clone();
    bad.collateral.currency = Currency::EUR;
    assert!(matches!(
        build(&bad, &cal),
        Err(SwapError::Unsupported { .. })
    ));
    let mut bad = c.terms.clone();
    bad.fixed.leg.notionals[0].value = Decimal::ZERO;
    assert!(build(&bad, &cal).is_err());
    let mut bad = c.terms.clone();
    bad.fixed.leg.notionals[0].start = d("2024-02-01");
    assert!(build(&bad, &cal).is_err());
    let mut bad = c.terms.clone();
    bad.fixed.leg.rounding = CashRounding::NearestEven(29);
    assert!(build(&bad, &cal).is_err());
    let mut bad = c.terms.clone();
    bad.fixed.leg.dates.payment_delay = DateOffset::BusinessDays(-1);
    assert!(build(&bad, &cal).is_err());
    let mut bad = c.terms.clone();
    bad.fixed.leg.dates.fixing_offset = Some(DateOffset::CalendarDays(0));
    assert!(build(&bad, &cal).is_err());
    let mut bad = c.terms.clone();
    bad.fixed.leg.negative_coupons = NegativeCouponPolicy::Reject;
    bad.fixed.rates[0].value = dec!(-0.01);
    assert!(build(&bad, &cal).is_err());
    let mut bad = c.terms.clone();
    if let FloatingLegTerms::Term(t) = &mut bad.floating {
        t.resets.fixing_offset = DateOffset::CalendarDays(1)
    }
    assert!(build(&bad, &cal).is_err());
    let mut bad = c.terms.clone();
    if let FloatingLegTerms::Term(t) = &mut bad.floating {
        t.resets.tenor_months = 121
    }
    assert!(build(&bad, &cal).is_err());
    let mut bad = c.terms.clone();
    if let FloatingLegTerms::Term(t) = &mut bad.floating {
        t.resets.index_end_of_month = true
    }
    assert!(build(&bad, &cal).is_err());
    let mut bad = c.terms.clone();
    if let FloatingLegTerms::Term(t) = &mut bad.floating {
        t.resets.projection_day_count = DayCountConvention::ActActIcma
    }
    assert!(build(&bad, &cal).is_err());
    let mut c = fixture()[10].clone();
    if let FloatingLegTerms::Overnight(t) = &mut c.terms.floating {
        t.leg.notionals = vec![
            DatedValue {
                start: d("2024-04-04"),
                end: d("2024-04-09"),
                value: dec!(1000000),
            },
            DatedValue {
                start: d("2024-04-09"),
                end: d("2024-04-11"),
                value: dec!(500000),
            },
        ];
    }
    assert!(matches!(
        build(&c.terms, &cal),
        Err(SwapError::Unsupported { .. })
    ));
    if let FloatingLegTerms::Overnight(t) = &mut c.terms.floating {
        t.leg.notionals = fixture()[10].terms.floating.leg().notionals.clone();
        t.convention.lockout_business_days = 4;
    }
    assert!(build(&c.terms, &cal).is_err());
}

#[test]
fn actual_calendar_versions_are_checked_and_missing_resources_are_not_inferred() {
    let c = &fixture()[0];
    let cal = calendars(c);
    let pins = &c.terms.fixed.leg.calendars;
    let mut wrong = pins.accrual.clone();
    wrong.version = Version::new(2).unwrap();
    let accrual = CalendarBinding {
        pin: &wrong,
        calendar: cal.get("ACCRUAL").unwrap(),
    };
    let payment = CalendarBinding {
        pin: &pins.payment,
        calendar: cal.get("PAYMENT").unwrap(),
    };
    let actual = SwapCalendars {
        fixed: LegCalendars { accrual, payment },
        floating: LegCalendars { accrual, payment },
        reset: None,
        fixing: None,
        observation: None,
    };
    assert!(matches!(
        c.terms.clone().build(&actual),
        Err(SwapError::Cashflow(CashflowError::InvalidTerms {
            field: "accrual_calendar",
            ..
        }))
    ));
    let matching = CalendarBinding {
        pin: &pins.accrual,
        calendar: cal.get("ACCRUAL").unwrap(),
    };
    let actual = SwapCalendars {
        fixed: LegCalendars {
            accrual: matching,
            payment,
        },
        floating: LegCalendars {
            accrual: matching,
            payment,
        },
        reset: None,
        fixing: None,
        observation: None,
    };
    assert!(matches!(
        c.terms.clone().build(&actual),
        Err(SwapError::Cashflow(CashflowError::InvalidTerms {
            field: "reset_calendar",
            ..
        }))
    ));
}

#[test]
fn shared_generator_keeps_all_five_calendar_transformations_independent() {
    use convex_core::calendars::BusinessDayConvention as Bdc;
    use convex_core::cashflows::{
        CouponSchedule, DateRules, ExCouponBoundary, ExCouponRule, ExDateAnchor, ScheduleCalendars,
    };
    let c = &fixture()[0];
    let cal = calendars(c);
    let observation = Holidays(BTreeSet::from([d("2024-01-30")]));
    let ex = Holidays(BTreeSet::from([d("2024-03-04")]));
    let rules = DateRules {
        accrual_adjustment: Bdc::ModifiedFollowing,
        payment_adjustment: Bdc::Following,
        payment_delay: DateOffset::BusinessDays(2),
        fixing_offset: Some(DateOffset::BusinessDays(-2)),
        observation_offset: Some(DateOffset::BusinessDays(-1)),
        ex_coupon: Some(ExCouponRule {
            offset: DateOffset::BusinessDays(-1),
            anchor: ExDateAnchor::PaymentDate,
            boundary: ExCouponBoundary::OnDate,
        }),
    };
    let schedule = CouponSchedule::generate_with_calendars(
        &c.terms.fixed.leg.schedule,
        &rules,
        &ScheduleCalendars {
            accrual: cal.get("ACCRUAL").unwrap(),
            payment: cal.get("PAYMENT").unwrap(),
            fixing: cal.get("FIXING").unwrap(),
            observation: &observation,
            ex_coupon: &ex,
        },
    )
    .unwrap();
    let p = &schedule.periods()[0];
    assert_eq!(p.contractual_dates(), (d("2024-01-31"), d("2024-02-29")));
    assert_eq!(p.accrual_dates(), (d("2024-01-31"), d("2024-02-28")));
    assert_eq!(p.payment_date(), d("2024-03-05"));
    assert_eq!(p.fixing_date(), Some(d("2024-01-26")));
    assert_eq!(
        p.observation_dates(),
        Some((d("2024-01-29"), d("2024-02-27")))
    );
    assert_eq!(
        p.ex_coupon(),
        Some((d("2024-03-01"), ExCouponBoundary::OnDate))
    );
}

#[test]
fn construction_work_bounds_and_projection_failures_return_no_partial_cash() {
    let mut c = fixture()[0].clone();
    let cal = calendars(&c);
    let a = c.terms.effective_date;
    let b = a.add_years(150).unwrap();
    c.terms.maturity_date = b;
    c.terms.fixed.leg.schedule.end_date = b;
    c.terms.fixed.rates[0].end = b;
    let entries = (0..4096)
        .map(|i| DatedValue {
            start: a.checked_add_days(a.days_between(&b) * i / 4096).unwrap(),
            end: a
                .checked_add_days(a.days_between(&b) * (i + 1) / 4096)
                .unwrap(),
            value: dec!(1000000),
        })
        .collect();
    c.terms.fixed.leg.notionals = entries;
    if let FloatingLegTerms::Term(t) = &mut c.terms.floating {
        t.leg = c.terms.fixed.leg.clone();
        t.resets.schedule = t.leg.schedule.clone();
        t.profiles.spreads[0].end = b;
        t.profiles.gearings[0].end = b;
    }
    assert!(matches!(
        build(&c.terms, &cal),
        Err(SwapError::Cashflow(CashflowError::BoundExceeded(
            "profile visits"
        )))
    ));
    let c = &fixture()[0];
    let s = build(&c.terms, &cal).unwrap();
    let ctx = context(d("2024-01-01"));
    assert!(matches!(
        s.cashflows(ctx, &FixingSeries::default(), publication, |_| Err(
            CashflowError::Arithmetic("supplied curve failure")
        )),
        Err(SwapError::Cashflow(CashflowError::Arithmetic(
            "supplied curve failure"
        )))
    ));
    assert!(s
        .cashflows(ctx, &FixingSeries::default(), publication, |r| Ok(
            ForecastRate {
                rate: dec!(11),
                projection_curve: r.projection_curve.clone()
            }
        ))
        .is_err());
    assert!(s
        .cashflows(
            ctx,
            &FixingSeries::default(),
            |_, _| Ok(ts("1900-01-01T08:00:00Z")),
            |r| forecast(c, r)
        )
        .is_err());
}

#[test]
fn deterministic_date_variations_and_negative_rates_conserve_exact_coupon_cash() {
    let mut seed = 818_u64;
    for _ in 0..128 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let year = 2020 + (seed % 25) as i32;
        let month = 1 + ((seed >> 7) % 12) as u32;
        let a = Date::from_ymd(year, month, 15).unwrap();
        let b = a.add_years(1).unwrap();
        let mut c = fixture()[0].clone();
        c.terms.effective_date = a;
        c.terms.maturity_date = b;
        {
            let p = &mut c.terms.fixed.leg.schedule;
            p.start_date = a;
            p.end_date = b;
        }
        c.terms.fixed.leg.notionals[0].start = a;
        c.terms.fixed.leg.notionals[0].end = b;
        c.terms.fixed.rates[0].start = a;
        c.terms.fixed.rates[0].end = b;
        c.terms.fixed.rates[0].value = dec!(-0.005);
        if let FloatingLegTerms::Term(t) = &mut c.terms.floating {
            t.leg = c.terms.fixed.leg.clone();
            t.resets.schedule = t.leg.schedule.clone();
            for v in [&mut t.profiles.spreads[0], &mut t.profiles.gearings[0]] {
                v.start = a;
                v.end = b;
            }
        }
        let cal = calendars(&c);
        let s = build(&c.terms, &cal).unwrap();
        let r = s
            .cashflows(
                context(a.checked_add_days(-10).unwrap()),
                &FixingSeries::default(),
                publication,
                |_| {
                    Ok(ForecastRate {
                        rate: dec!(-0.01),
                        projection_curve: c.terms.floating.projection_curve().clone(),
                    })
                },
            )
            .unwrap();
        let total = r.fixed.iter().map(|v| v.amount).sum::<Decimal>();
        close(
            total,
            dec!(1000000) * dec!(-0.005) * Decimal::from(a.days_between(&b)) / Decimal::from(360),
            dec!(0.000000000000000001),
        );
        assert!(r.floating.iter().all(|c| c.amount < Decimal::ZERO));
    }
}
