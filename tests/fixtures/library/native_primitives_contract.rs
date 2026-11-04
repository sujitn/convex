//! Compose public primitive calls over independently declared dated cash inputs.
use convex::{cashflows::*, contracts::*, fixings::*, Date, DayCountConvention, Frequency};
use convex_core::calendars::{BusinessDayConvention, WeekendCalendar};
use rust_decimal::Decimal;
use serde_json::{json, Value};

fn date(text: &str) -> Date {
    Date::parse(text).unwrap()
}
fn time(text: &str) -> UtcTimestamp {
    UtcTimestamp::parse(text).unwrap()
}
fn profile(pack: &Value, name: &str, kind: ProfileKind) -> DatedProfile {
    DatedProfile::new(
        kind,
        serde_json::from_value(pack["profiles"][name].clone()).unwrap(),
    )
    .unwrap()
}

pub fn run() -> Value {
    let pack: Value =
        serde_json::from_str(include_str!("native-primitives-reference.json")).unwrap();
    let tolerance: Decimal = pack["absolute_cash_bound"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let rules = DateRules {
        payment_delay: DateOffset::BusinessDays(1),
        payment_adjustment: BusinessDayConvention::Following,
        ..DateRules::default()
    };
    let schedule = CouponSchedule::generate(
        &ScheduleSpec {
            start_date: date("2025-01-01"),
            end_date: date("2025-07-01"),
            frequency: Frequency::Quarterly,
            stub_type: StubType::None,
            end_of_month: false,
            first_regular_date: None,
            penultimate_date: None,
        },
        &rules,
        &WeekendCalendar,
    )
    .unwrap();
    let profiles = CouponProfiles::new(
        profile(&pack, "coupon", ProfileKind::Coupon),
        profile(&pack, "spread", ProfileKind::Spread),
        profile(&pack, "gearing", ProfileKind::Gearing),
        profile(&pack, "notional", ProfileKind::Notional),
    )
    .unwrap();
    let index = IndexId::new("SYNTHETIC.TERM.3M").unwrap();
    let requirements = [
        FixingRequirement {
            index_id: index.clone(),
            observation_date: date("2025-01-01"),
            expected_publication: time("2025-01-02T08:00:00Z"),
        },
        FixingRequirement {
            index_id: index.clone(),
            observation_date: date("2025-04-01"),
            expected_publication: time("2025-04-02T08:00:00Z"),
        },
    ];
    let context = FixingContext {
        valuation_date: date(pack["valuation_date"].as_str().unwrap()),
        known_at: time(pack["known_at"].as_str().unwrap()),
        unpublished: UnpublishedFixingPolicy::Forecast,
        final_only: true,
    };
    let observation = |date_text, rate: &str, revision, published, received| FixingObservation {
        index_id: index.clone(),
        observation_date: date(date_text),
        rate: rate.parse().unwrap(),
        published_at: time(published),
        known_at: time(received),
        revision: Version::new(revision).unwrap(),
        status: FixingStatus::Final,
        source_id: SourceId::new("SYNTHETIC.RATES.v1").unwrap(),
    };
    let history = FixingSeries::new(vec![
        observation(
            "2025-01-01",
            "0.04",
            1,
            "2025-01-02T08:00:00Z",
            "2025-01-02T08:00:00Z",
        ),
        observation(
            "2025-01-01",
            "0.06",
            2,
            "2025-01-03T08:00:00Z",
            "2025-02-15T13:00:00Z",
        ),
        observation(
            "2025-04-01",
            "0.09",
            1,
            "2025-04-02T08:00:00Z",
            "2025-04-02T08:00:00Z",
        ),
    ])
    .unwrap();
    let absent = FixingSeries::default()
        .resolve(&requirements, context)
        .unwrap();
    assert_eq!(absent.missing(), &requirements[..1]);
    let mut callbacks = 0;
    assert!(matches!(
        absent.project(|_| {
            callbacks += 1;
            Ok(Decimal::ZERO)
        }),
        Err(CashflowError::MissingFixings(_))
    ));
    assert_eq!(callbacks, 0);
    let resolution = history.resolve(&requirements, context).unwrap();
    assert!(resolution.missing().is_empty());
    let mut cash_controls = 0;
    let mut known_cash = None;
    for case in pack["cases"].as_array().unwrap() {
        let forecast: Decimal = case["forecast_rate"].as_str().unwrap().parse().unwrap();
        let mut calls = 0;
        let rates = resolution
            .project(|requirement| {
                assert_eq!(requirement, &requirements[1]);
                calls += 1;
                Ok(forecast)
            })
            .unwrap();
        assert_eq!(calls, 1);
        assert!(
            matches!(&rates[0].origin, RateOrigin::Published(value) if value.revision.get() == 1)
        );
        assert!(matches!(
            rates[1].origin,
            RateOrigin::Projected(ProjectionReason::FutureObservation)
        ));
        let expected = case["coupons"].as_array().unwrap();
        assert_eq!(schedule.periods().len(), expected.len());
        for (i, ((period, rate), control)) in schedule
            .periods()
            .iter()
            .zip(&rates)
            .zip(expected)
            .enumerate()
        {
            assert_eq!(
                period.accrual_dates(),
                (
                    date(control["start"].as_str().unwrap()),
                    date(control["end"].as_str().unwrap())
                )
            );
            assert_eq!(
                period.payment_date(),
                date(control["payment_date"].as_str().unwrap())
            );
            let amount = profiles
                .coupon_amount(
                    period,
                    CouponCalculation {
                        index_rate: rate.rate,
                        day_count: DayCountConvention::Act360,
                        principal: CouponPrincipal::AccrualProfile,
                        rounding: CashRounding::Unrounded,
                        negative_coupons: NegativeCouponPolicy::Allow,
                    },
                )
                .unwrap();
            let expected: Decimal = control["amount"].as_str().unwrap().parse().unwrap();
            assert!(
                (amount - expected).abs() <= tolerance,
                "{amount} != {expected}"
            );
            if i == 0 {
                if let Some(prior) = known_cash {
                    assert_eq!(amount, prior);
                } else {
                    known_cash = Some(amount);
                }
            }
            cash_controls += 1;
        }
    }
    let principal = principal_cashflows(
        profiles.notional(),
        &rules,
        &WeekendCalendar,
        Decimal::ONE,
        CashRounding::Unrounded,
    )
    .unwrap();
    let expected = pack["principal"].as_array().unwrap();
    assert_eq!(principal.len(), expected.len());
    for (flow, control) in principal.iter().zip(expected) {
        assert_eq!(
            flow.effective_date,
            date(control["effective_date"].as_str().unwrap())
        );
        assert_eq!(
            flow.payment_date,
            date(control["payment_date"].as_str().unwrap())
        );
        assert_eq!(flow.amount.to_string(), control["amount"].as_str().unwrap());
    }
    let mut duplicate = requirements.to_vec();
    duplicate.push(requirements[0].clone());
    assert!(matches!(
        history.resolve(&duplicate, context),
        Err(CashflowError::InvalidTerms {
            field: "requirements",
            ..
        })
    ));
    assert!(resolution.project(|_| Ok(Decimal::from(11))).is_err());
    json!({"status":"passed", "coupon_cash_controls":cash_controls, "principal_cash_controls":principal.len(),
           "missing_history_preflight":true,"known_cash_unchanged_by_forecasts":true,
           "cash_kind":"projected_contractual","certified":false})
}
