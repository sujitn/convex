//! Economic properties and bounded failure contracts outside the published grid.
mod common;
use convex_core::{
    calendars::{Calendar, DynamicCalendar, WeekendType},
    cashflows::StubType,
    contracts::{ExactDecimal, FiniteF64, Version},
    daycounts::DayCountConvention,
    Date,
};
use convex_credit::*;
use convex_curves::contracts::{
    build_curve_set, CurveNode, CurvePoint, CurveRecipe, CurveRepresentation, CurveRole, CurveSet,
    CurveSource, MarketQuote, QuoteUnit,
};
use convex_curves::{ExtrapolationMethod, InterpolationMethod};
use rust_decimal::Decimal;
use std::error::Error;

type TestResult = Result<(), Box<dyn Error>>;
fn date(s: &str) -> Result<Date, Box<dyn Error>> {
    Ok(Date::parse(s)?)
}
fn close(a: f64, b: f64, absolute: f64) {
    assert!(
        (a - b).abs() <= absolute,
        "actual {a}, expected {b}, absolute {absolute}"
    );
}
fn analytic_recipe(
    template: &CurveRecipe,
    terms: &CdsTerms,
    credit: bool,
    rate: f64,
) -> CurveRecipe {
    let mut r = template.clone();
    if credit {
        r.key = terms.survival_curve.clone();
        r.role = CurveRole::Credit {
            entity_id: terms.reference.entity_id.as_str().into(),
            seniority: terms.reference.seniority.as_str().into(),
            recovery: terms.reference.recovery_fraction.get(),
        };
        r.representation = CurveRepresentation::SurvivalProbabilities;
    }
    r.source = CurveSource::Nodes(
        [0.0, 0.25, 1.0, 3.0, 12.0]
            .into_iter()
            .map(|t| CurveNode {
                pillar: CurvePoint::Tenor(t),
                quote: MarketQuote {
                    value: (-rate * t).exp(),
                    unit: if credit {
                        QuoteUnit::SurvivalProbability
                    } else {
                        QuoteUnit::DiscountFactor
                    },
                },
            })
            .collect(),
    );
    r
}
fn market(
    i: &common::Inputs,
    t: &CdsTerms,
    rate: f64,
    hazard: f64,
) -> Result<CurveSet, Box<dyn Error>> {
    Ok(build_curve_set(
        &[
            analytic_recipe(&i.discount_recipe, t, false, rate),
            analytic_recipe(&i.discount_recipe, t, true, hazard),
        ],
        &CurveSet::new(),
    )?
    .curves)
}

#[test]
fn buyer_seller_offset_every_leg_detail_and_fair_initial_payment() -> TestResult {
    let i = common::inputs()?;
    let cal = common::calendar(&i);
    let curves = common::market(&i)?;
    let t = i.template.clone();
    let c = common::build(&t, &cal)?;
    let context = t.trade_context(InitialPaymentState::NotContracted);
    let fair = price_flat_hazard(&c, &context, &curves, 0.035)?;
    let mut buyer = t.clone();
    buyer.initial_clean_upfront_fraction = Some(ExactDecimal::parse(
        &fair.clean_upfront_fraction.to_string(),
    )?);
    let b = common::build(&buyer, &cal)?;
    let ctx = buyer.trade_context(InitialPaymentState::Pending);
    let bv = price_flat_hazard(&b, &ctx, &curves, 0.035)?;
    close(bv.net_pv, 0.0, 1e-7);
    close(
        bv.initial_payment_pv,
        -bv.protection_leg_pv - bv.premium_leg_pv,
        1e-7,
    );
    let mut seller = buyer.clone();
    seller.side = CdsSide::ProtectionSeller;
    let s = common::build(&seller, &cal)?;
    let sv = price_flat_hazard(&s, &ctx, &curves, 0.035)?;
    for (x, y) in [
        (bv.protection_leg_pv, sv.protection_leg_pv),
        (bv.premium_leg_pv, sv.premium_leg_pv),
        (bv.initial_payment_pv, sv.initial_payment_pv),
        (bv.net_pv, sv.net_pv),
    ] {
        close(x, -y, 1e-9);
    }
    close(bv.clean_upfront_fraction, sv.clean_upfront_fraction, 0.0);
    close(bv.cash_settlement_amount, sv.cash_settlement_amount, 0.0);
    for (x, y) in bv.premiums.iter().zip(&sv.premiums) {
        close(
            x.conditional_coupon_amount,
            -y.conditional_coupon_amount,
            0.0,
        );
        close(x.scheduled_pv, -y.scheduled_pv, 0.0);
        close(x.accrual_on_default_pv, -y.accrual_on_default_pv, 0.0);
    }
    close(
        bv.premiums
            .iter()
            .map(|p| p.scheduled_pv + p.accrual_on_default_pv)
            .sum(),
        bv.premium_leg_pv,
        1e-7,
    );
    Ok(())
}

#[test]
fn exact_flat_analytic_protection_including_negative_and_cancelling_forwards() -> TestResult {
    let i = common::inputs()?;
    let cal = common::calendar(&i);
    let t = &i.template;
    let c = common::build(t, &cal)?;
    let ctx = t.trade_context(InitialPaymentState::NotContracted);
    let years = t.trade_date.days_between(&t.premium_schedule.end_date) as f64 / 365.0;
    for (rate, hazard) in [
        (0.02, 0.03),
        (-0.03, 0.03),
        (-0.03000000001, 0.03),
        (0.0, 0.0),
        (-0.04, 0.025),
        (0.7, 0.1),
    ] {
        let curves = market(&i, t, rate, hazard)?;
        let v = price_cds(&c, &ctx, &curves)?;
        let flat = price_flat_hazard(&c, &ctx, &curves, hazard)?;
        let z = rate + hazard;
        let integral = if z == 0.0 {
            years
        } else {
            -(-z * years).exp_m1() / z
        };
        close(
            v.protection_leg_pv,
            10_000_000.0 * 0.6 * hazard * integral,
            1e-7,
        );
        for (x, y) in [
            (v.net_pv, flat.net_pv),
            (v.premium_leg_pv, flat.premium_leg_pv),
            (v.protection_leg_pv, flat.protection_leg_pv),
        ] {
            close(x, y, 1e-7);
        }
        assert_eq!(
            v.credit_input,
            CreditInput::Supplied {
                curve: t.survival_curve.clone()
            }
        );
    }
    Ok(())
}

#[test]
fn nonaligned_piecewise_discount_and_survival_knots_match_analytic_integrals() -> TestResult {
    let i = common::inputs()?;
    let cal = common::calendar(&i);
    let mut t = i.template.clone();
    t.coupon_decimal = ExactDecimal::new(Decimal::ZERO);
    let c = common::build(&t, &cal)?;
    let ctx = t.trade_context(InitialPaymentState::NotContracted);
    let df = |year: f64| {
        (-0.02 * year.min(1.0) + 0.01 * (year - 1.0).clamp(0.0, 3.0)
            - 0.015 * (year - 4.0).max(0.0))
        .exp()
    };
    let q = |year: f64| {
        (-0.015 * year.min(2.0)
            - 0.05 * (year - 2.0).clamp(0.0, 1.0)
            - 0.02 * (year - 3.0).max(0.0))
        .exp()
    };
    let mut d = analytic_recipe(&i.discount_recipe, &t, false, 0.0);
    d.source = CurveSource::Nodes(
        [0.0, 1.0, 4.0, 12.0]
            .into_iter()
            .map(|y| CurveNode {
                pillar: CurvePoint::Tenor(y),
                quote: MarketQuote {
                    value: df(y),
                    unit: QuoteUnit::DiscountFactor,
                },
            })
            .collect(),
    );
    let mut survival = analytic_recipe(&i.discount_recipe, &t, true, 0.0);
    survival.source = CurveSource::Nodes(
        [0.0, 2.0, 3.0, 12.0]
            .into_iter()
            .map(|y| CurveNode {
                pillar: CurvePoint::Tenor(y),
                quote: MarketQuote {
                    value: q(y),
                    unit: QuoteUnit::SurvivalProbability,
                },
            })
            .collect(),
    );
    let curves = build_curve_set(&[d, survival], &CurveSet::new())?.curves;
    let v = price_cds(&c, &ctx, &curves)?;
    let horizon = t.trade_date.days_between(&t.premium_schedule.end_date) as f64 / 365.0;
    let mut integral = 0.0;
    for (start, end, rate, hazard) in [
        (0.0, 1.0, 0.02, 0.015),
        (1.0, 2.0, -0.01, 0.015),
        (2.0, 3.0, -0.01, 0.05),
        (3.0, 4.0, -0.01, 0.02),
        (4.0, horizon, 0.015, 0.02),
    ] {
        integral += df(start)
            * q(start)
            * hazard
            * (-(-(rate + hazard) * (end - start)).exp_m1() / (rate + hazard));
    }
    close(v.protection_leg_pv, 10_000_000.0 * 0.6 * integral, 1e-7);
    assert_eq!(v.premium_leg_pv, 0.0);
    Ok(())
}

#[test]
fn forward_protection_start_and_zero_spread_endpoint_remain_explicit() -> TestResult {
    let i = common::inputs()?;
    let cal = common::calendar(&i);
    let mut t = i.template.clone();
    let curves = market(&i, &t, 0.02, 0.03)?;
    t.premium_schedule.start_date = date("2022-09-20")?;
    t.protection_start_date = t.premium_schedule.start_date;
    let c = common::build(&t, &cal)?;
    let ctx = t.trade_context(InitialPaymentState::NotContracted);
    let v = price_cds(&c, &ctx, &curves)?;
    assert_eq!(v.accrued_days, 0);
    let start = (t.trade_date.days_between(&t.protection_start_date) - 1) as f64 / 365.0;
    let end = t.trade_date.days_between(&t.premium_schedule.end_date) as f64 / 365.0;
    close(
        v.protection_leg_pv,
        10_000_000.0 * 0.6 * 0.03 * ((-0.05 * start).exp() - (-0.05 * end).exp()) / 0.05,
        1e-7,
    );
    let zero = convert_cds(
        &c,
        &ctx,
        &curves,
        CdsQuote::RunningSpread { decimal_rate: 0.0 },
        ConversionSettings::default(),
    )?;
    assert_eq!(zero.hazard_per_act365f_year, 0.0);
    assert_eq!(zero.native_quote_residual, 0.0);
    assert_eq!(zero.iterations, 0);
    Ok(())
}

#[test]
fn default_accrual_toggle_zero_hazard_coupon_and_full_recovery() -> TestResult {
    let i = common::inputs()?;
    let cal = common::calendar(&i);
    let curves = common::market(&i)?;
    let mut t = i.template.clone();
    let ctx = t.trade_context(InitialPaymentState::NotContracted);
    let c = common::build(&t, &cal)?;
    let with = price_flat_hazard(&c, &ctx, &curves, 0.04)?;
    t.pay_accrual_on_default = false;
    let c = common::build(&t, &cal)?;
    let without = price_flat_hazard(&c, &ctx, &curves, 0.04)?;
    close(with.protection_leg_pv, without.protection_leg_pv, 0.0);
    assert!(with.premium_leg_pv < without.premium_leg_pv);
    assert!(without
        .premiums
        .iter()
        .all(|p| p.accrual_on_default_pv == 0.0));
    let zero = price_flat_hazard(&c, &ctx, &curves, 0.0)?;
    assert_eq!(zero.protection_leg_pv, 0.0);
    assert_eq!(zero.par_spread_decimal, Some(0.0));
    t.coupon_decimal = ExactDecimal::new(Decimal::ZERO);
    let c = common::build(&t, &cal)?;
    let zero_coupon = price_flat_hazard(&c, &ctx, &curves, 0.04)?;
    assert_eq!(zero_coupon.premium_leg_pv, 0.0);
    assert_eq!(zero_coupon.accrued_premium, 0.0);
    t.reference.recovery_fraction = FiniteF64::new(1.0)?;
    let c = common::build(&t, &cal)?;
    let full = price_flat_hazard(&c, &ctx, &curves, 0.04)?;
    assert_eq!(full.protection_leg_pv, 0.0);
    assert!(matches!(
        convert_cds(
            &c,
            &ctx,
            &curves,
            CdsQuote::RunningSpread { decimal_rate: 0.01 },
            ConversionSettings::default()
        ),
        Err(CdsError::Unsupported(_))
    ));
    Ok(())
}

#[test]
fn quarterly_roll_weekend_step_in_settlement_stubs_and_final_maturity_day() -> TestResult {
    for (d, before, after) in [
        ("2022-06-19", "2022-03-20", "2022-06-20"),
        ("2022-06-20", "2022-06-20", "2022-09-20"),
        ("2022-06-21", "2022-06-20", "2022-09-20"),
        ("2022-01-01", "2021-12-20", "2022-03-20"),
    ] {
        assert_eq!(cds_roll_on_or_before(date(d)?)?, date(before)?);
        assert_eq!(cds_roll_after(date(d)?)?, date(after)?);
    }
    assert!(cds_roll_on_or_before(date("0001-01-01")?).is_err());
    assert!(cds_roll_after(date("9999-12-20")?).is_err());
    let i = common::inputs()?;
    let cal = common::calendar(&i);
    let mut t = i.template.clone();
    t.trade_date = date("2022-06-17")?;
    t.step_in_date = date("2022-06-18")?;
    t.premium_schedule.start_date = date("2022-03-21")?;
    t.protection_start_date = t.step_in_date;
    t.cash_settlement_date = date("2022-06-23")?;
    t.premium_schedule.end_date = date("2027-06-20")?;
    let c = common::build(&t, &cal)?;
    assert!(!cal.is_business_day(t.step_in_date));
    assert_eq!(c.premium_periods()[0].payment_date, date("2022-06-21")?);
    let last = c.premium_periods().last().ok_or("missing premium")?;
    assert_eq!(last.accrual_end, date("2027-06-21")?);
    assert_eq!(last.payment_date, date("2027-06-21")?);
    assert_eq!(last.contractual_end, t.premium_schedule.end_date);
    t.premium_schedule.end_date = date("2027-07-07")?;
    assert!(common::build(&t, &cal).is_err());
    t.maturity_rule = CdsMaturityRule::ExplicitDate;
    t.premium_schedule.stub_type = StubType::ShortLast;
    let c = common::build(&t, &cal)?;
    assert_eq!(
        c.premium_periods().last().ok_or("last stub")?.accrual_end,
        date("2027-07-08")?
    );
    Ok(())
}

#[test]
fn strict_terms_reference_units_calendars_dates_and_recovery_fail_closed() -> TestResult {
    let i = common::inputs()?;
    let cal = common::calendar(&i);
    let t = i.template.clone();
    let roundtrip: CdsTerms = serde_json::from_str(&serde_json::to_string(&t)?)?;
    assert_eq!(roundtrip, t);
    for (path, value) in [
        ("reference.entity_id", serde_json::json!("")),
        ("reference.seniority", serde_json::json!("Unknown")),
        ("reference.document.version", serde_json::json!(0)),
        ("reference.restructuring_clause.id", serde_json::json!("")),
        ("version", serde_json::json!(0)),
        ("coupon_decimal", serde_json::json!(100)),
        ("model", serde_json::json!("StandardLatest")),
        ("currency", serde_json::json!("UNKNOWN")),
    ] {
        let mut v = serde_json::to_value(&t)?;
        let parts: Vec<_> = path.split('.').collect();
        let mut node = &mut v;
        for key in &parts[..parts.len() - 1] {
            node = &mut node[*key];
        }
        node[parts[parts.len() - 1]] = value;
        assert!(
            serde_json::from_value::<CdsTerms>(v).is_err(),
            "accepted {path}"
        );
    }
    let mut v = serde_json::to_value(&t)?;
    v["auction_state"] = serde_json::json!("defaulted");
    assert!(serde_json::from_value::<CdsTerms>(v).is_err());
    for mutation in 0..10 {
        let mut x = t.clone();
        match mutation {
            0 => x.notional = ExactDecimal::new(Decimal::ZERO),
            1 => x.notional = ExactDecimal::parse("-1")?,
            2 => x.coupon_decimal = ExactDecimal::parse("100")?,
            3 => x.reference.recovery_fraction = FiniteF64::new(-0.1)?,
            4 => x.reference.recovery_fraction = FiniteF64::new(1.01)?,
            5 => x.step_in_date = x.step_in_date.checked_add_days(1)?,
            6 => x.cash_settlement_date = x.cash_settlement_date.checked_add_days(1)?,
            7 => x.settlement_lag_business_days = 367,
            8 => x.protection_start_date = x.trade_date,
            _ => x.premium_day_count = DayCountConvention::Thirty360US,
        }
        assert!(
            common::build(&x, &cal).is_err(),
            "accepted mutation {mutation}"
        );
    }
    let mut wrong = t.premium_calendar.clone();
    wrong.version = Version::new(2)?;
    assert!(t
        .build(&CdsCalendars {
            premium: CalendarBinding {
                pin: &wrong,
                calendar: &cal
            },
            settlement: CalendarBinding {
                pin: &t.settlement_calendar,
                calendar: &cal
            }
        })
        .is_err());
    struct Closed;
    impl Calendar for Closed {
        fn name(&self) -> &'static str {
            "closed"
        }
        fn is_business_day(&self, _: Date) -> bool {
            false
        }
        fn is_holiday(&self, _: Date) -> bool {
            true
        }
    }
    assert!(t
        .build(&CdsCalendars {
            premium: CalendarBinding {
                pin: &t.premium_calendar,
                calendar: &Closed
            },
            settlement: CalendarBinding {
                pin: &t.settlement_calendar,
                calendar: &Closed
            }
        })
        .is_err());
    Ok(())
}

#[test]
fn exact_credit_entity_seniority_recovery_and_curve_versions_are_required() -> TestResult {
    let i = common::inputs()?;
    let t = &i.template;
    let cal = common::calendar(&i);
    let c = common::build(t, &cal)?;
    let ctx = t.trade_context(InitialPaymentState::NotContracted);
    let d = analytic_recipe(&i.discount_recipe, t, false, 0.02);
    let q = analytic_recipe(&i.discount_recipe, t, true, 0.03);
    for mutation in 0..8 {
        let mut d = d.clone();
        let mut q = q.clone();
        match mutation {
            0 => {
                q.role = CurveRole::Credit {
                    entity_id: "another.entity".into(),
                    seniority: t.reference.seniority.as_str().into(),
                    recovery: 0.4,
                }
            }
            1 => {
                q.role = CurveRole::Credit {
                    entity_id: t.reference.entity_id.as_str().into(),
                    seniority: "subordinated".into(),
                    recovery: 0.4,
                }
            }
            2 => {
                q.role = CurveRole::Credit {
                    entity_id: t.reference.entity_id.as_str().into(),
                    seniority: t.reference.seniority.as_str().into(),
                    recovery: 0.2,
                }
            }
            3 => q.key.version = "other".into(),
            4 => d.key.version = "other".into(),
            5 => {
                d.role = CurveRole::Government {
                    benchmark_id: "govt".into(),
                }
            }
            6 => d.interpolation = InterpolationMethod::Linear,
            _ => q.extrapolation = ExtrapolationMethod::Flat,
        }
        let curves = build_curve_set(&[d, q], &CurveSet::new())?.curves;
        assert!(
            price_cds(&c, &ctx, &curves).is_err(),
            "accepted mutation {mutation}"
        );
    }
    let empty = CurveSet::new();
    assert!(price_cds(&c, &ctx, &empty).is_err());
    let mut short = d.clone();
    short.source = CurveSource::Nodes(vec![
        CurveNode {
            pillar: CurvePoint::Tenor(0.0),
            quote: MarketQuote {
                value: 1.0,
                unit: QuoteUnit::DiscountFactor,
            },
        },
        CurveNode {
            pillar: CurvePoint::Tenor(0.1),
            quote: MarketQuote {
                value: 0.99,
                unit: QuoteUnit::DiscountFactor,
            },
        },
    ]);
    let curves = build_curve_set(&[short, q], &CurveSet::new())?.curves;
    assert!(price_cds(&c, &ctx, &curves).is_err());
    Ok(())
}

#[test]
fn actual_payment_state_seasoned_normalization_and_maturity_are_explicit() -> TestResult {
    let i = common::inputs()?;
    let cal = common::calendar(&i);
    let mut t = i.template.clone();
    let curves = market(&i, &t, 0.02, 0.03)?;
    t.initial_clean_upfront_fraction = Some(ExactDecimal::parse("0.05")?);
    let c = common::build(&t, &cal)?;
    for state in [
        InitialPaymentState::NotContracted,
        InitialPaymentState::Settled,
    ] {
        assert!(price_cds(&c, &t.trade_context(state), &curves).is_err());
    }
    let mut ctx = t.trade_context(InitialPaymentState::Pending);
    ctx.valuation_date = t.cash_settlement_date;
    ctx.step_in_date = ctx.valuation_date.checked_add_days(1)?;
    let due = price_cds(&c, &ctx, &curves)?;
    assert!(due.initial_payment_pv < 0.0);
    ctx.initial_payment_state = InitialPaymentState::Settled;
    let paid = price_cds(&c, &ctx, &curves)?;
    assert_eq!(paid.initial_payment_pv, 0.0);
    close(paid.net_pv - due.net_pv, -due.initial_payment_pv, 1e-7);
    close(
        paid.protection_leg_pv,
        10_000_000.0
            * 0.6
            * 0.03
            * (-(-0.05
                * (ctx
                    .valuation_date
                    .days_between(&t.premium_schedule.end_date) as f64
                    / 365.0))
                .exp_m1()
                / 0.05),
        1e-7,
    );
    ctx.valuation_date = ctx.valuation_date.checked_add_days(1)?;
    ctx.step_in_date = ctx.valuation_date.checked_add_days(1)?;
    ctx.cash_settlement_date = ctx.valuation_date;
    ctx.initial_payment_state = InitialPaymentState::Pending;
    assert!(price_cds(&c, &ctx, &curves).is_err());
    ctx.initial_payment_state = InitialPaymentState::Settled;
    ctx.valuation_date = t.premium_schedule.end_date.checked_add_days(2)?;
    ctx.step_in_date = ctx.valuation_date.checked_add_days(1)?;
    ctx.cash_settlement_date = ctx.valuation_date;
    let matured = price_cds(&c, &ctx, &curves)?;
    assert_eq!(matured.net_pv, 0.0);
    assert!(matured.premiums.is_empty());
    assert_eq!(matured.par_spread_decimal, None);
    assert!(convert_cds(
        &c,
        &ctx,
        &curves,
        CdsQuote::RunningSpread { decimal_rate: 0.01 },
        ConversionSettings::default()
    )
    .is_err());
    // A delayed final payment is not silently labelled paid when protection ends.
    ctx.valuation_date = t.premium_schedule.end_date;
    ctx.step_in_date = ctx.valuation_date.checked_add_days(1)?;
    ctx.cash_settlement_date = ctx.valuation_date;
    assert!(matches!(
        price_cds(&c, &ctx, &curves),
        Err(CdsError::Unsupported(_))
    ));
    Ok(())
}

#[test]
fn bounded_conversion_rejects_bad_units_nonfinite_brackets_and_work_limits() -> TestResult {
    let i = common::inputs()?;
    let cal = common::calendar(&i);
    let t = &i.template;
    let curves = common::market(&i)?;
    let c = common::build(t, &cal)?;
    let ctx = t.trade_context(InitialPaymentState::NotContracted);
    for quote in [
        CdsQuote::RunningSpread {
            decimal_rate: f64::NAN,
        },
        CdsQuote::RunningSpread {
            decimal_rate: 100.0,
        },
        CdsQuote::RunningSpread {
            decimal_rate: -0.01,
        },
        CdsQuote::CleanUpfront {
            fraction: f64::INFINITY,
        },
        CdsQuote::DirtyUpfront { fraction: 11.0 },
    ] {
        assert!(convert_cds(&c, &ctx, &curves, quote, ConversionSettings::default()).is_err());
    }
    for settings in [
        ConversionSettings {
            upper_hazard: 0.0,
            ..Default::default()
        },
        ConversionSettings {
            lower_hazard: -0.1,
            ..Default::default()
        },
        ConversionSettings {
            upper_hazard: 6.0,
            ..Default::default()
        },
        ConversionSettings {
            native_quote_tolerance: 0.0,
            ..Default::default()
        },
        ConversionSettings {
            max_iterations: 257,
            ..Default::default()
        },
        ConversionSettings {
            max_iterations: 0,
            ..Default::default()
        },
    ] {
        assert!(convert_cds(
            &c,
            &ctx,
            &curves,
            CdsQuote::RunningSpread { decimal_rate: 0.01 },
            settings
        )
        .is_err());
    }
    assert!(convert_cds(
        &c,
        &ctx,
        &curves,
        CdsQuote::RunningSpread { decimal_rate: 0.1 },
        ConversionSettings {
            upper_hazard: 0.001,
            ..Default::default()
        }
    )
    .is_err());
    assert!(convert_cds(
        &c,
        &ctx,
        &curves,
        CdsQuote::RunningSpread { decimal_rate: 0.01 },
        ConversionSettings {
            max_iterations: 1,
            ..Default::default()
        }
    )
    .is_err());
    for h in [-0.01, f64::NAN, 5.01] {
        assert!(price_flat_hazard(&c, &ctx, &curves, h).is_err());
    }
    let mut dense = i.discount_recipe.clone();
    dense.source = CurveSource::Nodes(
        (0..=1024)
            .map(|x| CurveNode {
                pillar: CurvePoint::Tenor(x as f64 / 1000.0),
                quote: MarketQuote {
                    value: (-0.02 * x as f64 / 1000.0).exp(),
                    unit: QuoteUnit::DiscountFactor,
                },
            })
            .collect(),
    );
    assert!(matches!(
        build_curve_set(&[dense], &CurveSet::new()),
        Err(convex_curves::contracts::CurveContractError::BoundExceeded(
            "curve nodes"
        ))
    ));
    Ok(())
}

#[test]
fn seeded_running_clean_dirty_roundtrips_and_side_offsets() -> TestResult {
    let i = common::inputs()?;
    let cal = common::calendar(&i);
    let curves = common::market(&i)?;
    let mut seed = 0x020_cafe_u64;
    for _ in 0..128 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let case = &i.cases[(seed as usize) % i.cases.len()];
        let mut t = common::terms(&i, case)?;
        let c = common::build(&t, &cal)?;
        let ctx = t.trade_context(InitialPaymentState::NotContracted);
        let spread = case.spread_bps.parse::<f64>()? / 10000.0;
        let a = convert_cds(
            &c,
            &ctx,
            &curves,
            CdsQuote::RunningSpread {
                decimal_rate: spread,
            },
            ConversionSettings::default(),
        )?;
        for q in [
            CdsQuote::CleanUpfront {
                fraction: a.valuation.clean_upfront_fraction,
            },
            CdsQuote::DirtyUpfront {
                fraction: a.valuation.dirty_upfront_fraction,
            },
        ] {
            let b = convert_cds(&c, &ctx, &curves, q, ConversionSettings::default())?;
            close(b.conventional_spread_decimal, spread, 1e-10);
            assert!(b.native_quote_residual.abs() <= b.settings.native_quote_tolerance);
        }
        t.side = CdsSide::ProtectionSeller;
        let c = common::build(&t, &cal)?;
        let seller = price_flat_hazard(&c, &ctx, &curves, a.hazard_per_act365f_year)?;
        close(a.valuation.net_pv, -seller.net_pv, 1e-9);
    }
    Ok(())
}

#[test]
fn separate_premium_and_settlement_calendars_use_actual_supplied_holidays() -> TestResult {
    let i = common::inputs()?;
    let t = &i.template;
    let premium = common::calendar(&i);
    let settlement = DynamicCalendar::from_dates(
        "another",
        WeekendType::SaturdaySunday,
        [date("2022-06-22")?],
    );
    assert!(t
        .build(&CdsCalendars {
            premium: CalendarBinding {
                pin: &t.premium_calendar,
                calendar: &premium
            },
            settlement: CalendarBinding {
                pin: &t.settlement_calendar,
                calendar: &settlement
            }
        })
        .is_err());
    Ok(())
}
