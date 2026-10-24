use convex_analytics::{bond_contract, optional_bond::*};
use convex_bonds::{contracts::*, instruments::FixedRateBond};
use convex_core::{
    calendars::{BusinessDayConvention as Bdc, Calendar, WeekendCalendar},
    cashflows::*,
    contracts::{IndexId, SourceId, UtcTimestamp, Version},
    daycounts::DayCountConvention as Dc,
    fixings::*,
    types::{Compounding, Frequency},
    Currency, Date,
};
use convex_curves::{contracts::*, ExtrapolationMethod as Ex, InterpolationMethod as Int};
use rust_decimal::{prelude::FromPrimitive, Decimal};
use rust_decimal_macros::dec;
use serde_json::Value;

fn date(s: &str) -> Date {
    Date::parse(s).unwrap()
}
fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/optional-bonds.json")).unwrap()
}
fn context(root: Date) -> OptionalBondContext {
    OptionalBondContext {
        settlement: root,
        on_payment: PaymentDateEntitlement::Exclude,
        exercise_state: ExerciseState::AliveUnexercised,
    }
}
fn grid() -> OptionalBondSettings {
    OptionalBondSettings {
        grid: GridSettings {
            base_steps: 128,
            refinements: 3,
            absolute_price_tolerance: 0.004,
            relative_price_tolerance: 1e-8,
        },
        exercise_priority: ExercisePriority::IssuerLast,
    }
}
fn model(a: f64, s: f64) -> OptionalBondModel {
    OptionalBondModel::assumed(
        CurveKey {
            id: "supplied-hw1f".into(),
            version: "1".into(),
        },
        Hw1fParameters {
            mean_reversion: a,
            volatility: s,
        },
        "synthetic explicit assumption".into(),
    )
    .unwrap()
}
fn curve(root: Date, role: CurveRole, rate: f64) -> BuiltCurve {
    let recipe = CurveRecipe {
        key: CurveKey {
            id: if matches!(role, CurveRole::Projection { .. }) {
                "projection".into()
            } else {
                "discount".into()
            },
            version: "1".into(),
        },
        role,
        currency: Currency::USD,
        reference_date: root,
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
                pillar: CurvePoint::Tenor(10.0),
                quote: MarketQuote {
                    value: rate,
                    unit: QuoteUnit::DecimalRate,
                },
            },
        ]),
    };
    let key = recipe.key.clone();
    build_curve_set(&[recipe], &CurveSet::new())
        .unwrap()
        .curves
        .get(&key)
        .unwrap()
        .as_ref()
        .clone()
}
fn discount(root: Date, rate: f64) -> BuiltCurve {
    curve(
        root,
        CurveRole::Discount {
            collateral_currency: Currency::USD,
        },
        rate,
    )
}
fn fixed(root: Date, maturity: Date, coupon: Decimal) -> BondTerms {
    FixedRateBond::builder()
        .cusip_unchecked("SUPPLIED-CVX059")
        .issue_date(root)
        .maturity(maturity)
        .coupon_rate(coupon)
        .frequency(Frequency::Annual)
        .day_count(Dc::Act365Fixed)
        .calendar(convex_bonds::types::CalendarId::weekend_only())
        .business_day_convention(Bdc::Unadjusted)
        .face_value(dec!(100))
        .redemption_value(dec!(100))
        .end_of_month(false)
        .build()
        .unwrap()
        .contract_terms()
        .unwrap()
}
fn exercise(date: Date, price: Decimal, right: ExerciseRight) -> ExerciseTerms {
    ExerciseTerms {
        right,
        style: ExerciseStyle::European { date },
        price_per_100: price,
        notice_offset: DateOffset::CalendarDays(0),
        settlement_lag: DateOffset::CalendarDays(0),
        settlement_adjustment: Bdc::Unadjusted,
        accrued: ExerciseAccrued::PayAccrued,
        protection_end: None,
    }
}
fn inputs<'a>(
    d: &'a BuiltCurve,
    p: Option<&'a BuiltCurve>,
    m: &'a OptionalBondModel,
    calendar: &'a dyn Calendar,
) -> OptionalBondInputs<'a> {
    OptionalBondInputs {
        discount: d,
        projection: p,
        model: m,
        exercise_calendar: calendar,
    }
}
fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance + 1e-8 * expected.abs(),
        "actual={actual:.12} expected={expected:.12} difference={:.12}",
        (actual - expected).abs()
    );
}

#[test]
fn european_call_put_hold_independent_prices_negative_rates_and_ho_lee_limits() {
    let f: Value = serde_json::from_str(include_str!(
        "../../convex-bonds/tests/fixtures/hw1f-options.json"
    ))
    .unwrap();
    let root = date("2025-01-01");
    for id in [
        "positive-call",
        "positive-put",
        "negative-call",
        "negative-put",
        "ho_lee-call",
        "ho_lee-put",
        "deterministic-call",
        "deterministic-put",
    ] {
        let c = f["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["id"] == id)
            .unwrap();
        let expiry = root
            .checked_add_days((c["expiry"].as_f64().unwrap() * 365.0) as i64)
            .unwrap();
        let maturity = root
            .checked_add_days((c["maturity"].as_f64().unwrap() * 365.0) as i64)
            .unwrap();
        let right = if c["side"] == "call" {
            ExerciseRight::Call
        } else {
            ExerciseRight::Put
        };
        let mut t = fixed(root, maturity, dec!(0));
        t.exercise = vec![exercise(
            expiry,
            Decimal::from_f64(c["strike"].as_f64().unwrap() * 100.0).unwrap(),
            right,
        )];
        let bond = t.build().unwrap();
        let d = discount(root, c["flat_rate"].as_f64().unwrap());
        let params: Hw1fParameters = serde_json::from_value(c["parameters"].clone()).unwrap();
        let m = model(params.mean_reversion, params.volatility);
        let prepared = prepare_fixed(
            &bond,
            context(root),
            inputs(&d, None, &m, &WeekendCalendar),
            grid(),
        )
        .unwrap();
        let result = prepared.price(0.0).unwrap();
        let sign = if right == ExerciseRight::Call {
            -1.0
        } else {
            1.0
        };
        let expected = 100.0 * c["maturity_df"].as_f64().unwrap()
            + sign * 100.0 * c["expected_per_unit"].as_f64().unwrap();
        close(result.dirty_price, expected, 0.004);
        assert_eq!(result.grids.len(), 4);
        assert!(
            result
                .grids
                .iter()
                .all(|g| g.tree.minimum_probability >= 0.0
                    && g.tree.maximum_discount_residual < 1e-10)
        );
        assert!(result.exercises[0].decision_nodes > 0);
        assert_eq!(result.model.parameters(), params);
        println!("oracle {id}: {}", result.dirty_price);
    }
}
#[test]
fn fixed_notice_and_delayed_settlement_use_the_notice_exchange_option() {
    let c = fixture()["notice_zero"].clone();
    let root = date(c["root"].as_str().unwrap());
    let mut t = fixed(root, date(c["maturity"].as_str().unwrap()), dec!(0));
    let mut option = exercise(
        date(c["exercise"].as_str().unwrap()),
        dec!(98),
        ExerciseRight::Call,
    );
    option.notice_offset = DateOffset::CalendarDays(-73);
    option.settlement_lag = DateOffset::CalendarDays(10);
    t.exercise = vec![option];
    let bond = t.build().unwrap();
    let d = discount(root, 0.04);
    let m = model(0.03, 0.02);
    let p = prepare_fixed(
        &bond,
        context(root),
        inputs(&d, None, &m, &WeekendCalendar),
        grid(),
    )
    .unwrap()
    .price(0.0)
    .unwrap();
    close(
        p.dirty_price,
        c["expected_callable_price"].as_f64().unwrap(),
        0.004,
    );
    assert_eq!(p.exercises[0].dates.notice, date("2026-01-01"));
    assert_eq!(p.exercises[0].dates.settlement, date("2026-03-25"));
    let later = context(date("2026-02-01"));
    assert!(matches!(
        prepare_fixed(&bond, later, inputs(&d, None, &m, &WeekendCalendar), grid()),
        Err(OptionalBondError::Unsupported {
            capability: "outstanding notice state",
            ..
        })
    ));
}
#[test]
fn stepped_amortized_cash_notice_stub_and_payment_order_are_exact_at_zero_volatility() {
    let root = date("2025-01-01");
    let end = date("2027-01-01");
    let mid = date("2026-01-01");
    let step = date("2025-07-01");
    let mut t = fixed(root, end, dec!(0.04));
    t.profiles.coupon = vec![
        DatedValue {
            start: root,
            end: step,
            value: dec!(0.04),
        },
        DatedValue {
            start: step,
            end,
            value: dec!(0.08),
        },
    ];
    t.profiles.notional = vec![
        DatedValue {
            start: root,
            end: mid,
            value: dec!(100),
        },
        DatedValue {
            start: mid,
            end,
            value: dec!(60),
        },
    ];
    t.coupon_principal = CouponPrincipal::AccrualProfile;
    t.principal_prices = vec![PrincipalPrice {
        effective_date: mid,
        multiplier: dec!(1.02),
        amount: None,
    }];
    let mut call = exercise(mid, dec!(100), ExerciseRight::Call);
    call.notice_offset = DateOffset::CalendarDays(-31);
    t.exercise = vec![call];
    let bond = t.build().unwrap();
    let d = discount(root, 0.02);
    let m = model(0.03, 0.0);
    let straight = bond
        .fixed_projection(root, PaymentDateEntitlement::Exclude)
        .unwrap();
    let coupon = straight.coupons[0].amount;
    let expected = (coupon + dec!(40.8) + dec!(60))
        .to_string()
        .parse::<f64>()
        .unwrap()
        * (-0.02_f64).exp();
    let p = prepare_fixed(
        &bond,
        context(root),
        inputs(&d, None, &m, &WeekendCalendar),
        grid(),
    )
    .unwrap()
    .price(0.0)
    .unwrap();
    close(p.dirty_price, expected, 1e-9);
    // Mid-period exercise pays only the actual partial accrued coupon, not a flat average.
    let mut t = bond.terms().clone();
    let date = date("2026-07-01");
    t.exercise = vec![exercise(date, dec!(100), ExerciseRight::Call)];
    let bond = t.build().unwrap();
    let at = bond
        .fixed_projection(date, PaymentDateEntitlement::Exclude)
        .unwrap();
    let full_first = (coupon + dec!(40.8)).to_string().parse::<f64>().unwrap() * (-0.02_f64).exp();
    let expected = full_first
        + (dec!(60) + at.accrued).to_string().parse::<f64>().unwrap()
            * (-0.02 * root.days_between(&date) as f64 / 365.0).exp();
    let p = prepare_fixed(
        &bond,
        context(root),
        inputs(&d, None, &m, &WeekendCalendar),
        grid(),
    )
    .unwrap()
    .price(0.0)
    .unwrap();
    close(p.dirty_price, expected, 1e-9);
}
#[test]
fn oas_inversion_and_effective_risk_freeze_fractional_oas_model_and_input_curves() {
    let root = date("2025-01-01");
    let mut t = fixed(root, date("2028-01-01"), dec!(0.06));
    let mut call = exercise(date("2026-01-01"), dec!(101), ExerciseRight::Call);
    call.style = ExerciseStyle::Bermudan {
        dates: vec![date("2026-01-01"), date("2027-01-01")],
    };
    t.exercise = vec![call];
    let bond = t.build().unwrap();
    let d = discount(root, 0.04);
    let m = model(0.03, 0.01);
    let p = prepare_fixed(
        &bond,
        context(root),
        inputs(&d, None, &m, &WeekendCalendar),
        grid(),
    )
    .unwrap();
    let oas = 0.00123456;
    let target = p.price(oas).unwrap();
    let inverse = p
        .solve_oas(
            target.dirty_price,
            OasSolveSettings {
                lower: -0.05,
                upper: 0.10,
                price_tolerance: 1e-9,
                max_iterations: 128,
            },
        )
        .unwrap();
    close(inverse.oas_decimal, oas, 1e-10);
    assert!((inverse.residual_per_100).abs() < 1e-9);
    assert_ne!(inverse.oas_bps, inverse.oas_bps.round());
    for axis in [RiskAxis::Discount, RiskAxis::Oas] {
        let r = p.risk(oas, axis, 0.01, risk_settings()).unwrap();
        close(
            r.duration,
            (r.down.dirty_price - r.up.dirty_price) / (2.0 * r.base.dirty_price * r.bump),
            1e-12,
        );
        close(
            r.convexity,
            (r.down.dirty_price + r.up.dirty_price - 2.0 * r.base.dirty_price)
                / (r.base.dirty_price * r.bump * r.bump),
            1e-9,
        );
        for price in [&r.base, &r.up, &r.down] {
            assert_eq!(price.model.parameters(), m.parameters());
            assert_eq!(price.discount_curve, d.recipe().key);
        }
        if axis == RiskAxis::Discount {
            assert_eq!(r.up.oas_decimal, oas);
            assert_eq!(r.down.oas_decimal, oas);
        }
        let half = p.risk(oas, axis, 0.005, risk_settings()).unwrap();
        let double = p.risk(oas, axis, 0.02, risk_settings()).unwrap();
        // Different finite scenario widths can change the optimal exercise region.
        // Independent per-bump references below test accuracy; equality across
        // 50/100/200bp scenarios is not a valid invariant.
        assert_eq!(half.bump, 0.005);
        assert_eq!(double.bump, 0.02);
        println!(
            "risk {axis:?}: h/2={} h={} 2h={} convexities={}/{}/{}",
            half.duration,
            r.duration,
            double.duration,
            half.convexity,
            r.convexity,
            double.convexity
        );
    }
    assert!(matches!(
        p.solve_oas(
            1000.0,
            OasSolveSettings {
                lower: -0.01,
                upper: 0.01,
                price_tolerance: 1e-9,
                max_iterations: 32
            }
        ),
        Err(OptionalBondError::OasBracket { .. })
    ));
    assert!(matches!(
        p.solve_oas(
            target.dirty_price,
            OasSolveSettings {
                lower: -0.05,
                upper: 0.10,
                price_tolerance: 1e-15,
                max_iterations: 1
            }
        ),
        Err(OptionalBondError::OasConvergence { .. })
    ));
}
#[test]
fn fine_grid_failure_returns_measurements_without_a_successful_price() {
    let root = date("2025-01-01");
    let mut t = fixed(root, date("2027-01-01"), dec!(0));
    t.exercise = vec![exercise(date("2026-01-01"), dec!(96), ExerciseRight::Call)];
    let b = t.build().unwrap();
    let d = discount(root, 0.04);
    let m = model(0.03, 0.02);
    let mut s = grid();
    s.grid.base_steps = 8;
    s.grid.refinements = 2;
    s.grid.absolute_price_tolerance = 1e-12;
    s.grid.relative_price_tolerance = 0.0;
    let p = prepare_fixed(&b, context(root), inputs(&d, None, &m, &WeekendCalendar), s).unwrap();
    let Err(OptionalBondError::GridConvergence { diagnostics }) = p.price(0.0) else {
        panic!("coarse tree cannot claim convergence")
    };
    assert_eq!(diagnostics.len(), 3);
    assert!(diagnostics[2].successive_difference.unwrap() > 1e-12);
}
#[test]
fn simultaneous_call_put_priority_and_immediate_oas_plateau_are_explicit() {
    let root = date("2025-01-01");
    let mut t = fixed(root, date("2027-01-01"), dec!(0));
    t.issue_date = date("2024-12-31");
    t.exercise = vec![
        exercise(root, dec!(90), ExerciseRight::Call),
        exercise(root, dec!(105), ExerciseRight::Put),
    ];
    let b = t.build().unwrap();
    let d = discount(root, 0.04);
    let m = model(0.03, 0.01);
    let issuer = prepare_fixed(
        &b,
        context(root),
        inputs(&d, None, &m, &WeekendCalendar),
        grid(),
    )
    .unwrap();
    close(issuer.price(0.0).unwrap().dirty_price, 90.0, 1e-12);
    let mut settings = grid();
    settings.exercise_priority = ExercisePriority::HolderLast;
    let holder = prepare_fixed(
        &b,
        context(root),
        inputs(&d, None, &m, &WeekendCalendar),
        settings,
    )
    .unwrap();
    close(holder.price(0.0).unwrap().dirty_price, 105.0, 1e-12);
    assert!(matches!(
        issuer.solve_oas(
            90.0,
            OasSolveSettings {
                lower: -0.01,
                upper: 0.01,
                price_tolerance: 1e-9,
                max_iterations: 128
            }
        ),
        Err(OptionalBondError::OasBracket { .. })
    ));
}

fn floating_terms() -> BondTerms {
    let root = date("2025-01-01");
    let mut t = fixed(root, date("2027-01-01"), dec!(0));
    t.profiles = BondProfiles::constant(
        root,
        t.schedule.end_date,
        dec!(0),
        dec!(0.05),
        dec!(1),
        dec!(100),
    );
    t.coupon_principal = CouponPrincipal::AccrualProfile;
    t.negative_coupons = NegativeCouponPolicy::Allow;
    t.coupon = BondCoupon::Term(TermRateTerms {
        index_id: IndexId::new("SUPPLIED.TERM").unwrap(),
        resets: t.schedule.clone(),
        reset_adjustment: Bdc::Unadjusted,
        fixing_offset: DateOffset::CalendarDays(0),
        projection_day_count: Dc::Act365Fixed,
    });
    t.exercise = vec![exercise(
        date("2026-01-01"),
        dec!(104.8),
        ExerciseRight::Call,
    )];
    t
}
fn projection_context() -> BondProjectionContext {
    BondProjectionContext {
        settlement: date("2025-01-01"),
        on_payment: PaymentDateEntitlement::Exclude,
        fixing: FixingContext {
            valuation_date: date("2025-01-01"),
            known_at: UtcTimestamp::parse("2025-01-01T18:00:00Z").unwrap(),
            unpublished: UnpublishedFixingPolicy::RequirePublished,
            final_only: true,
        },
        overnight_accrual_cutoffs: vec![],
    }
}
fn history() -> FixingSeries {
    FixingSeries::new(vec![FixingObservation {
        index_id: IndexId::new("SUPPLIED.TERM").unwrap(),
        observation_date: date("2025-01-01"),
        rate: Decimal::from_f64(
            fixture()["joint_frn"]["eligible_initial_rate"]
                .as_f64()
                .unwrap(),
        )
        .unwrap(),
        published_at: UtcTimestamp::parse("2025-01-01T16:00:00Z").unwrap(),
        known_at: UtcTimestamp::parse("2025-01-01T16:00:00Z").unwrap(),
        revision: Version::new(1).unwrap(),
        status: FixingStatus::Final,
        source_id: SourceId::new("supplied").unwrap(),
    }])
    .unwrap()
}
fn publication(_: &IndexId, d: Date) -> Result<UtcTimestamp, CashflowError> {
    Ok(UtcTimestamp::parse(&format!("{d}T16:00:00Z")).unwrap())
}

#[test]
fn joint_floating_call_matches_independent_margin_bond_option_and_preserves_known_coupons() {
    let t = floating_terms();
    let bond = t.build().unwrap();
    let root = date("2025-01-01");
    let d = discount(root, 0.04);
    let projection = curve(
        root,
        CurveRole::Projection {
            index_id: "SUPPLIED.TERM".into(),
        },
        0.04,
    );
    let m = model(0.03, 0.02);
    let ctx = projection_context();
    let h = history();
    let p = prepare_floating(
        &bond,
        context(root),
        &ctx,
        &h,
        publication,
        inputs(&d, Some(&projection), &m, &WeekendCalendar),
        grid(),
    )
    .unwrap();
    let price = p.price(0.0).unwrap();
    close(
        price.dirty_price,
        fixture()["joint_frn"]["expected_callable_price"]
            .as_f64()
            .unwrap(),
        0.004,
    );
    assert!(price.deterministic_projection_basis);
    assert_eq!(
        price
            .fixings
            .iter()
            .filter(|r| matches!(r.origin, RateOrigin::Published(_)))
            .count(),
        1
    );
    let projection_risk = p
        .risk(0.00123456, RiskAxis::Projection, 0.01, risk_settings())
        .unwrap();
    assert_eq!(projection_risk.up.fixings, projection_risk.down.fixings);
    assert_eq!(projection_risk.up.oas_decimal, 0.00123456);
    let zero = model(0.03, 0.0);
    let deterministic = prepare_floating(
        &bond,
        context(root),
        &ctx,
        &h,
        publication,
        inputs(&d, Some(&projection), &zero, &WeekendCalendar),
        grid(),
    )
    .unwrap()
    .price(0.0)
    .unwrap();
    assert!(price.dirty_price < deterministic.dirty_price - 0.005);
    let mut plain = bond.terms().clone();
    plain.exercise.clear();
    let plain = plain.build().unwrap();
    let mut forecast =
        |q: &ProjectionRequest| bond_contract::project_from_curve(q, Currency::USD, &projection);
    let base = bond_contract::price_floating(
        &plain,
        &ctx,
        &h,
        publication,
        BondProjectionPolicy::Supplied(&mut forecast),
        &d,
    )
    .unwrap();
    close(
        base.dirty_price,
        fixture()["joint_frn"]["expected_plain_price"]
            .as_f64()
            .unwrap(),
        1e-9,
    );
    let inverse = p
        .solve_oas(
            p.price(0.00123456).unwrap().dirty_price,
            OasSolveSettings {
                lower: -0.02,
                upper: 0.04,
                price_tolerance: 1e-9,
                max_iterations: 128,
            },
        )
        .unwrap();
    close(inverse.oas_decimal, 0.00123456, 1e-10);
    let empty = FixingSeries::new(vec![]).unwrap();
    assert!(matches!(
        prepare_floating(
            &bond,
            context(root),
            &ctx,
            &empty,
            publication,
            inputs(&d, Some(&projection), &m, &WeekendCalendar),
            grid()
        ),
        Err(OptionalBondError::Bond(BondContractError::Cashflow(
            CashflowError::MissingFixings { .. }
        )))
    ));
}
#[test]
fn floating_basis_gearing_and_unsupported_state_never_become_plain_pricing() {
    let root = date("2025-01-01");
    let d = discount(root, 0.02);
    let projection = curve(
        root,
        CurveRole::Projection {
            index_id: "SUPPLIED.TERM".into(),
        },
        0.04,
    );
    let m = model(0.03, 0.01);
    let ctx = projection_context();
    let h = history();
    let mut t = floating_terms();
    t.exercise[0].price_per_100 = dec!(1000);
    let bond = t.build().unwrap();
    let p = prepare_floating(
        &bond,
        context(root),
        &ctx,
        &h,
        publication,
        inputs(&d, Some(&projection), &m, &WeekendCalendar),
        grid(),
    )
    .unwrap()
    .price(0.0)
    .unwrap();
    let mut plain = bond.terms().clone();
    plain.exercise.clear();
    let plain = plain.build().unwrap();
    let mut forecast =
        |q: &ProjectionRequest| bond_contract::project_from_curve(q, Currency::USD, &projection);
    let straight = bond_contract::price_floating(
        &plain,
        &ctx,
        &h,
        publication,
        BondProjectionPolicy::Supplied(&mut forecast),
        &d,
    )
    .unwrap();
    close(p.dirty_price, straight.dirty_price, 0.004);
    for mode in 0..4 {
        let mut t = floating_terms();
        match mode {
            0 => t.exercise[0].notice_offset = DateOffset::CalendarDays(-5),
            1 => {
                t.exercise[0].style = ExerciseStyle::European {
                    date: date("2026-02-01"),
                }
            }
            2 => t.rounding = CashRounding::NearestEven(2),
            _ => {
                if let BondCoupon::Term(r) = &mut t.coupon {
                    r.fixing_offset = DateOffset::BusinessDays(-2);
                }
            }
        };
        let b = t.build().unwrap();
        assert!(prepare_floating(
            &b,
            context(root),
            &ctx,
            &h,
            publication,
            inputs(&d, Some(&projection), &m, &WeekendCalendar),
            grid()
        )
        .is_err());
        assert!(bond_contract::price_fixed(&b, root, PaymentDateEntitlement::Exclude, &d).is_err());
    }
    let mut t = floating_terms();
    t.exercise[0].style = ExerciseStyle::American {
        start: date("2026-01-01"),
        end: date("2026-12-31"),
    };
    let b = t.build().unwrap();
    assert!(prepare_floating(
        &b,
        context(root),
        &ctx,
        &h,
        publication,
        inputs(&d, Some(&projection), &m, &WeekendCalendar),
        grid()
    )
    .is_err());
}

fn calibration_quotes() -> Vec<SwaptionCalibrationQuote> {
    fixture()["swaption_cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            let expiry = date("2025-01-01")
                .checked_add_days(c["expiry_years"].as_i64().unwrap() * 365)
                .unwrap();
            let end = expiry
                .checked_add_days(c["tail_years"].as_i64().unwrap() * 365)
                .unwrap();
            // These exact supplied annual dates avoid leap-year proxy accrual; ACT/365
            // schedule periods retain their actual day count, including a final stub.
            SwaptionCalibrationQuote {
                id: c["id"].as_str().unwrap().into(),
                exercise_date: expiry,
                fixed_schedule: ScheduleSpec {
                    start_date: expiry,
                    end_date: end,
                    frequency: Frequency::Annual,
                    stub_type: StubType::ShortLast,
                    end_of_month: false,
                    first_regular_date: None,
                    penultimate_date: None,
                },
                dates: DateRules::default(),
                fixed_day_count: Dc::Act365Fixed,
                strike_rate: c["strike"].as_f64().unwrap(),
                side: serde_json::from_value(c["side"].clone()).unwrap(),
                volatility: if c["convention"] == "normal" {
                    SwaptionVolatility::Normal {
                        annualized_decimal: c["volatility"].as_f64().unwrap(),
                    }
                } else {
                    SwaptionVolatility::ShiftedLognormal {
                        annualized_decimal: c["volatility"].as_f64().unwrap(),
                        shift: c["shift"].as_f64().unwrap(),
                    }
                },
                native_tolerance: 2e-7,
                scale: 0.01,
                weight: 1.0,
            }
        })
        .collect()
}
fn calibration_settings() -> SigmaCalibrationSettings {
    SigmaCalibrationSettings {
        initial_sigma: 0.008,
        lower_sigma: 0.001,
        upper_sigma: 0.05,
        max_iterations: 100,
        objective_tolerance: 2e-5,
        jacobian_step: 1e-6,
        minimum_singular_ratio: 1e-10,
        svd_iterations: 1000,
    }
}
#[test]
fn normal_and_shifted_log_volatility_roundtrip_and_calibration_have_native_diagnostics() {
    let root = date("2025-01-01");
    let d = discount(root, 0.04);
    let quotes = calibration_quotes();
    let params = Hw1fParameters {
        mean_reversion: 0.03,
        volatility: 0.012,
    };
    for (q, c) in quotes
        .iter()
        .zip(fixture()["swaption_cases"].as_array().unwrap())
    {
        let r = reprice_swaption(q, root, &d, &WeekendCalendar, params).unwrap();
        close(
            r.model_premium,
            c["expected_model_premium"].as_f64().unwrap(),
            2e-7,
        );
        close(r.model_quote, q.volatility.value(), 2e-7);
        assert_eq!(
            serde_json::from_str::<SwaptionCalibrationQuote>(&serde_json::to_string(q).unwrap())
                .unwrap(),
            *q
        );
    }
    let key = CurveKey {
        id: "calibrated-hw".into(),
        version: "1".into(),
    };
    let fit = calibrate_sigma(SigmaCalibrationInputs {
        model_key: key.clone(),
        valuation_date: root,
        currency: Currency::USD,
        discount: &d,
        calendar: &WeekendCalendar,
        mean_reversion: 0.03,
        quotes: &quotes,
        settings: calibration_settings(),
    })
    .unwrap();
    close(fit.parameters().volatility, 0.012, 2e-7);
    let ModelOrigin::SwaptionCalibration {
        residuals,
        solver,
        discount_curve,
        fixed_mean_reversion,
        ..
    } = fit.origin()
    else {
        panic!("fitted model must retain calibration provenance")
    };
    assert_eq!(*fixed_mean_reversion, 0.03);
    assert_eq!(*discount_curve, d.recipe().key);
    assert_eq!(solver.rank, Some(1));
    assert!(solver.iterations > 0);
    assert!(residuals
        .iter()
        .all(|r| r.native_residual.abs() <= r.tolerance));
    let mut reversed = quotes.clone();
    reversed.reverse();
    let other = calibrate_sigma(SigmaCalibrationInputs {
        model_key: key,
        valuation_date: root,
        currency: Currency::USD,
        discount: &d,
        calendar: &WeekendCalendar,
        mean_reversion: 0.03,
        quotes: &reversed,
        settings: calibration_settings(),
    })
    .unwrap();
    assert_eq!(fit.parameters(), other.parameters());
}
#[test]
fn invalid_volatility_shift_units_rank_and_iteration_failure_produce_no_model() {
    let root = date("2025-01-01");
    let d = discount(root, 0.04);
    let key = CurveKey {
        id: "calibration".into(),
        version: "1".into(),
    };
    let quotes = calibration_quotes();
    for v in [
        serde_json::json!({"convention":"unknown","annualized_decimal":0.01}),
        serde_json::json!({"convention":"normal","annualized_decimal":0.01,"shift":0.02}),
        serde_json::json!({"convention":"shifted_lognormal","annualized_decimal":0.2}),
    ] {
        assert!(serde_json::from_value::<SwaptionVolatility>(v).is_err());
    }
    let mut bad = quotes.clone();
    bad[0].volatility = SwaptionVolatility::Normal {
        annualized_decimal: 70.0,
    };
    assert!(calibrate_sigma(SigmaCalibrationInputs {
        model_key: key.clone(),
        valuation_date: root,
        currency: Currency::USD,
        discount: &d,
        calendar: &WeekendCalendar,
        mean_reversion: 0.03,
        quotes: &bad,
        settings: calibration_settings()
    })
    .is_err());
    bad[0].volatility = SwaptionVolatility::ShiftedLognormal {
        annualized_decimal: 0.2,
        shift: -0.05,
    };
    assert!(calibrate_sigma(SigmaCalibrationInputs {
        model_key: key.clone(),
        valuation_date: root,
        currency: Currency::USD,
        discount: &d,
        calendar: &WeekendCalendar,
        mean_reversion: 0.03,
        quotes: &bad,
        settings: calibration_settings()
    })
    .is_err());
    let mut settings = calibration_settings();
    settings.max_iterations = 1;
    settings.objective_tolerance = 1e-14;
    let err = calibrate_sigma(SigmaCalibrationInputs {
        model_key: key.clone(),
        valuation_date: root,
        currency: Currency::USD,
        discount: &d,
        calendar: &WeekendCalendar,
        mean_reversion: 0.03,
        quotes: &quotes,
        settings,
    })
    .unwrap_err();
    let OptionalBondError::Calibration(failure) = err else {
        panic!("must retain measured solver failure")
    };
    assert!(failure.diagnostics.iterations <= 1);
    let mut zero = quotes.clone();
    for q in &mut zero {
        q.weight = 0.0;
    }
    assert!(calibrate_sigma(SigmaCalibrationInputs {
        model_key: key.clone(),
        valuation_date: root,
        currency: Currency::USD,
        discount: &d,
        calendar: &WeekendCalendar,
        mean_reversion: 0.03,
        quotes: &zero,
        settings: calibration_settings()
    })
    .is_err());
    let duplicate = vec![quotes[0].clone(), quotes[0].clone()];
    assert!(calibrate_sigma(SigmaCalibrationInputs {
        model_key: key,
        valuation_date: root,
        currency: Currency::USD,
        discount: &d,
        calendar: &WeekendCalendar,
        mean_reversion: 0.03,
        quotes: &duplicate,
        settings: calibration_settings()
    })
    .is_err());
}

fn risk_settings() -> RiskSettings {
    RiskSettings {
        absolute_duration_tolerance: 0.02,
        relative_duration_tolerance: 0.01,
        absolute_convexity_tolerance: 0.25,
        relative_convexity_tolerance: 0.01,
    }
}

#[test]
fn independent_optional_risk_matches_each_bump_and_refuses_undersampling() {
    let c = fixture()["risk_case"].clone();
    let root = date(c["root"].as_str().unwrap());
    let mut t = fixed(root, date(c["maturity"].as_str().unwrap()), dec!(0));
    t.exercise = vec![exercise(
        date(c["exercise"].as_str().unwrap()),
        dec!(96),
        ExerciseRight::Call,
    )];
    let bond = t.build().unwrap();
    let d = discount(root, 0.04);
    let m = model(0.03, 0.02);
    let p = prepare_fixed(
        &bond,
        context(root),
        inputs(&d, None, &m, &WeekendCalendar),
        grid(),
    )
    .unwrap();
    let oas = c["oas_decimal"].as_f64().unwrap();
    for row in c["rows"].as_array().unwrap() {
        let bump = row["bump_decimal"].as_f64().unwrap();
        let risk = p
            .risk(oas, RiskAxis::Discount, bump, risk_settings())
            .unwrap();
        close(risk.duration, row["duration"].as_f64().unwrap(), 0.005);
        close(risk.convexity, row["convexity"].as_f64().unwrap(), 0.5);
        close(risk.pv01, row["pv01_per_100"].as_f64().unwrap(), 0.00005);
        assert!(risk.convexity < 0.0); // Option-boundary movement is captured.
        println!(
            "independent risk bump={bump}: duration={} convexity={} grids={:?}",
            risk.duration, risk.convexity, risk.grids
        );
    }
    let result = p.risk(oas, RiskAxis::Discount, 0.0001, risk_settings());
    assert!(matches!(
        result,
        Err(OptionalBondError::RiskResolution { bump: 0.0001, .. })
    ));
    let strict = RiskSettings {
        absolute_duration_tolerance: 1e-12,
        relative_duration_tolerance: 0.0,
        absolute_convexity_tolerance: 1e-12,
        relative_convexity_tolerance: 0.0,
    };
    assert!(matches!(
        p.risk(oas, RiskAxis::Discount, 0.01, strict),
        Err(OptionalBondError::RiskConvergence { .. })
    ));
}

#[test]
fn exact_model_root_curve_role_and_unsupported_economic_json_are_checked() {
    let root = date("2025-01-01");
    let d = discount(root, 0.04);
    let quotes = calibration_quotes();
    let calibrated = calibrate_sigma(SigmaCalibrationInputs {
        model_key: CurveKey {
            id: "calibrated".into(),
            version: "1".into(),
        },
        valuation_date: root,
        currency: Currency::USD,
        discount: &d,
        calendar: &WeekendCalendar,
        mean_reversion: 0.03,
        quotes: &quotes,
        settings: calibration_settings(),
    })
    .unwrap();
    let mut t = fixed(root, date("2027-01-01"), dec!(0));
    t.exercise = vec![exercise(date("2026-01-01"), dec!(96), ExerciseRight::Call)];
    let b = t.clone().build().unwrap();
    prepare_fixed(
        &b,
        context(root),
        inputs(&d, None, &calibrated, &WeekendCalendar),
        grid(),
    )
    .unwrap();
    let mut revised = d.recipe().clone();
    revised.key.version = "2".into();
    let key = revised.key.clone();
    let set = build_curve_set(&[revised], &CurveSet::new()).unwrap();
    let other = set.curves.get(&key).unwrap();
    assert!(prepare_fixed(
        &b,
        context(root),
        inputs(other, None, &calibrated, &WeekendCalendar),
        grid()
    )
    .is_err());
    assert!(prepare_fixed(
        &b,
        context(date("2025-01-02")),
        inputs(&d, None, &calibrated, &WeekendCalendar),
        grid()
    )
    .is_err());
    for field in ["cap", "floor", "make_whole"] {
        let mut input = serde_json::to_value(&t).unwrap();
        input["coupon"][field] = serde_json::json!(0.05);
        assert!(serde_json::from_value::<BondTerms>(input).is_err());
    }
    let mut t = b.terms().clone();
    t.exercise[0].style = ExerciseStyle::American {
        start: date("2026-01-01"),
        end: date("2026-12-31"),
    };
    let b = t.build().unwrap();
    assert!(prepare_fixed(
        &b,
        context(root),
        inputs(&d, None, &calibrated, &WeekendCalendar),
        grid()
    )
    .is_err());
    assert!(OptionalBondModel::assumed(
        CurveKey {
            id: "assumed".into(),
            version: "1".into()
        },
        Hw1fParameters {
            mean_reversion: 0.03,
            volatility: f64::NAN
        },
        "reason".into()
    )
    .is_err());
}
