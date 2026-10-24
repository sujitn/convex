mod credit_support;
use convex_analytics::{credit_calibration::*, credit_risk::*};
use convex_core::{contracts::*, Date};
use convex_credit::{CdsSide, InitialPaymentState, Seniority};
use convex_curves::contracts::*;
use credit_support::*;
use std::error::Error;

#[test]
fn mixed_running_clean_dirty_quotes_fit_same_actual_original_curve() -> Result<(), Box<dyn Error>> {
    let data = inputs()?;
    let base = recipe(&data)?;
    let r = mixed(&base)?;
    let cal = calendar(&data);
    let s = supplied(&data)?;
    let reference: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/comparisons/credit-isda-1.8.3/isda-reference.json"
    ))?;
    for r in [r.clone(), global(&r)] {
        let bindings = calendars(&r, &cal);
        let out = build_credit_curve_set(
            &[CdsCurveBuildInput {
                recipe: &r,
                calendars: &bindings,
            }],
            &s,
            FitNumericalControls::default(),
        )?;
        for (i, &h) in out.calibrations[0]
            .hazards_per_act365f_year
            .iter()
            .enumerate()
        {
            let expected = reference["experiments"][0]["hazards_per_act365f_year"][i]
                .as_f64()
                .ok_or("missing actual original hazard")?;
            assert!((h - expected).abs() < 1e-9);
        }
        for (helper, q) in r
            .helpers
            .iter()
            .zip(&out.calibrations[0].curve.diagnostics().quotes)
        {
            assert_eq!(q.input, helper.quote);
            assert!(q.residual.abs() <= helper.tolerance);
        }
    }
    Ok(())
}

#[test]
fn central_risk_is_signed_bump_stable_and_duplicate_bases_cannot_aggregate(
) -> Result<(), Box<dyn Error>> {
    let data = inputs()?;
    let r = recipe(&data)?;
    let cal = calendar(&data);
    let s = supplied(&data)?;
    let bindings = calendars(&r, &cal);
    let original = trade(&r, &cal, 5, "0.01")?;
    let mut buyer_values = Vec::new();
    for side in [CdsSide::ProtectionBuyer, CdsSide::ProtectionSeller] {
        let mut terms = original.terms().clone();
        terms.side = side;
        let t = terms.build(&binding(&terms, &cal))?;
        let ctx = t.terms().trade_context(InitialPaymentState::NotContracted);
        let p = prepare_credit_risk(CreditRiskInput {
            recipe: &r,
            helper_calendars: &bindings,
            supplied: &s,
            trade: &t,
            trade_calendars: binding(t.terms(), &cal),
            context: &ctx,
            controls: FitNumericalControls::default(),
            risk_model: ModelId::new("supplied.credit.central.v1")?,
        })?;
        let mut values = Vec::new();
        for step in [0.5, 1.0, 2.0] {
            let bump = CreditRiskBump::RunningQuote {
                helper_id: r.helpers[5].id.clone(),
                step_bps: step,
            };
            let factor = factor(&r, &bump)?;
            let value =
                credit_sensitivity(&p, &factor, bump, &revisions(&r, "signed-test", false))?;
            assert_eq!(value.measure, CreditRiskMeasure::Cs01Signed);
            assert!(value.plus.calibration.is_some() && value.minus.calibration.is_some());
            values.push(value.value);
            assert!(validate_credit_risk_vector(&[value.clone(), value]).is_err());
        }
        assert!((values[0] - values[1]).abs() < 0.1);
        assert!((values[2] - values[1]).abs() < 0.1);
        if side == CdsSide::ProtectionBuyer {
            buyer_values = values;
        } else {
            for (a, b) in values.iter().zip(&buyer_values) {
                assert!((a + b).abs() < 1e-7);
            }
        }
        let bump = CreditRiskBump::Recovery {
            step_fraction: 0.01,
            credit_response: CreditResponse::HoldSurvival,
        };
        let factor = factor(&r, &bump)?;
        let value = credit_sensitivity(&p, &factor, bump, &revisions(&r, "recovery-test", false))?;
        assert!(
            (value.value
                + p.base_value().protection_leg_pv
                    / (1.0 - terms.reference.recovery_fraction.get()))
            .abs()
                < 1e-6
        );
        let mut wrong = factor.definition().clone();
        wrong.model_version = ModelId::new("wrong.risk.model")?;
        let wrong = RiskFactorKey::new(wrong)?;
        assert!(credit_sensitivity(
            &p,
            &wrong,
            CreditRiskBump::Recovery {
                step_fraction: 0.01,
                credit_response: CreditResponse::HoldSurvival
            },
            &revisions(&r, "wrong", false)
        )
        .is_err());
        assert!(evaluate_credit_scenario(
            &p,
            &CreditScenarioRequest {
                id: "full-recovery-frozen".into(),
                output_credit: CurveKey {
                    id: r.curve.key.id.clone(),
                    version: "full-frozen".into()
                },
                shock: CreditShock::Recovery {
                    fraction: FiniteF64::new(1.0)?,
                    credit_response: CreditResponse::HoldSurvival
                }
            }
        )?
        .calibration
        .is_none());
        assert!(evaluate_credit_scenario(
            &p,
            &CreditScenarioRequest {
                id: "full-recovery-recalibrated".into(),
                output_credit: CurveKey {
                    id: r.curve.key.id.clone(),
                    version: "full-rebuilt".into()
                },
                shock: CreditShock::Recovery {
                    fraction: FiniteF64::new(1.0)?,
                    credit_response: CreditResponse::RecalibrateQuotes
                }
            }
        )
        .is_err());
    }
    Ok(())
}

#[test]
fn upfront_risk_is_a_fraction_derivative_and_wrong_cs01_or_native_unit_fails(
) -> Result<(), Box<dyn Error>> {
    let data = inputs()?;
    let r = mixed(&recipe(&data)?)?;
    let cal = calendar(&data);
    let s = supplied(&data)?;
    let bindings = calendars(&r, &cal);
    let t = trade(&r, &cal, 5, "0.01")?;
    let ctx = t.terms().trade_context(InitialPaymentState::NotContracted);
    let p = prepare_credit_risk(CreditRiskInput {
        recipe: &r,
        helper_calendars: &bindings,
        supplied: &s,
        trade: &t,
        trade_calendars: binding(t.terms(), &cal),
        context: &ctx,
        controls: FitNumericalControls::default(),
        risk_model: ModelId::new("supplied.credit.central.v1")?,
    })?;
    for kind in [CdsQuoteKind::CleanUpfront, CdsQuoteKind::DirtyUpfront] {
        let mut wrong = r.helpers[5].clone();
        wrong.quote_kind = kind;
        wrong.quote.unit = QuoteUnit::BasisPoints;
        assert!(cds_model_quote(&wrong, p.base_value()).is_err());
    }
    let bump = CreditRiskBump::UpfrontQuote {
        helper_id: r.helpers[5].id.clone(),
        step_fraction: 0.0001,
    };
    let factor = factor(&r, &bump)?;
    let value = credit_sensitivity(&p, &factor, bump, &revisions(&r, "upfront", false))?;
    assert_eq!(value.measure, CreditRiskMeasure::UpfrontDerivative);
    assert!(
        (value.value - (value.plus.shocked.net_pv - value.minus.shocked.net_pv) / 0.0002).abs()
            < 1e-7
    );
    assert!(credit_sensitivity(
        &p,
        &factor,
        CreditRiskBump::RunningQuote {
            helper_id: r.helpers[5].id.clone(),
            step_bps: 1.0
        },
        &revisions(&r, "wrong-cs01", false)
    )
    .is_err());
    assert!(evaluate_credit_scenario(
        &p,
        &CreditScenarioRequest {
            id: "wrong-quote-unit".into(),
            output_credit: CurveKey {
                id: r.curve.key.id.clone(),
                version: "wrong-unit".into()
            },
            shock: CreditShock::Quotes {
                moves: vec![CdsQuoteMove {
                    helper_id: r.helpers[5].id.clone(),
                    expected_quote_version: r.helpers[5].quote_version,
                    delta: MarketQuote {
                        value: 1.0,
                        unit: QuoteUnit::BasisPoints
                    }
                }]
            }
        }
    )
    .is_err());
    Ok(())
}

#[test]
fn actual_global_rank_failure_and_zero_weight_native_residual_are_not_accepted(
) -> Result<(), Box<dyn Error>> {
    let data = inputs()?;
    let mut r = global(&recipe(&data)?);
    let cal = calendar(&data);
    let s = supplied(&data)?;
    r.pillars.push(Date::parse("2040-06-20")?);
    let mut extra = r.helpers[5].clone();
    extra.id = "Unidentifying.Extra".into();
    r.helpers.push(extra);
    r.curve.source = CurveSource::Helpers(r.helpers.iter().map(|h| h.id.clone()).collect());
    if let CalibrationMode::CreditGlobal(g) = &mut r.curve.calibration {
        g.initial_hazards.push(0.02);
        g.quote_scales.push(0.01);
        g.weights.push(1.0);
    }
    let b = calendars(&r, &cal);
    let error = build_credit_curve_set(
        &[CdsCurveBuildInput {
            recipe: &r,
            calendars: &b,
        }],
        &s,
        FitNumericalControls::default(),
    )
    .unwrap_err();
    if let CreditCalibrationError::Global(f) = error {
        assert!(matches!(
            f.reason,
            convex_math::optimization::FitFailureReason::IllConditioned
        ));
        assert!(f.diagnostics.rank.is_some());
    } else {
        panic!("actual rank diagnostics required");
    }
    let mut r = global(&recipe(&data)?);
    let mut extra = r.helpers[5].clone();
    extra.id = "ZeroWeight.Impossible".into();
    extra.quote.value += 1.0;
    r.helpers.push(extra);
    r.curve.source = CurveSource::Helpers(r.helpers.iter().map(|h| h.id.clone()).collect());
    if let CalibrationMode::CreditGlobal(g) = &mut r.curve.calibration {
        g.quote_scales.push(0.01);
        g.weights.push(0.0);
    }
    let b = calendars(&r, &cal);
    assert!(build_credit_curve_set(
        &[CdsCurveBuildInput {
            recipe: &r,
            calendars: &b
        }],
        &s,
        FitNumericalControls::default()
    )
    .is_err());
    assert_eq!(s.len(), 1);
    Ok(())
}

#[test]
fn zero_credit_endpoint_and_seeded_nonflat_rebuilds_preserve_native_quotes(
) -> Result<(), Box<dyn Error>> {
    let data = inputs()?;
    let original = recipe(&data)?;
    let cal = calendar(&data);
    let s = supplied(&data)?;
    let mut state = 0x021u64;
    for sample in 0..65 {
        let mut r = original.clone();
        r.helpers.truncate(3);
        r.pillars.truncate(3);
        r.curve.source = CurveSource::Helpers(r.helpers.iter().map(|h| h.id.clone()).collect());
        let hazards: Vec<_> = (0..3)
            .map(|_| {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                if sample == 0 {
                    0.0
                } else {
                    0.002 + (state >> 32) as f64 / u32::MAX as f64 * 0.04
                }
            })
            .collect();
        let mut direct = r.curve.clone();
        direct.source = CurveSource::Nodes(credit_survival_nodes(
            direct.reference_date,
            &r.pillars,
            &hazards,
        )?);
        direct.calibration = CalibrationMode::Direct;
        direct.dependencies.clear();
        let market = build_curve_set(&[direct], &s)?.curves;
        for h in &mut r.helpers {
            let t = h.terms.build(&binding(&h.terms, &cal))?;
            let value = convex_credit::price_cds(&t, &h.context, &market)?;
            h.quote = cds_model_quote(h, &value)?;
        }
        let b = calendars(&r, &cal);
        let built = build_credit_curve_set(
            &[CdsCurveBuildInput {
                recipe: &r,
                calendars: &b,
            }],
            &s,
            FitNumericalControls::default(),
        )?;
        for (a, b) in built.calibrations[0]
            .hazards_per_act365f_year
            .iter()
            .zip(hazards)
        {
            assert!((a - b).abs() < 1e-9, "seed sample {sample}");
        }
    }
    Ok(())
}

#[test]
fn every_original_pillar_reprices_native_units_and_actual_survival_constraints(
) -> Result<(), Box<dyn Error>> {
    let data = inputs()?;
    let r = recipe(&data)?;
    let cal = calendar(&data);
    let supplied = supplied(&data)?;
    let bindings = calendars(&r, &cal);
    let result = build_credit_curve_set(
        &[CdsCurveBuildInput {
            recipe: &r,
            calendars: &bindings,
        }],
        &supplied,
        FitNumericalControls::default(),
    )?;
    let c = &result.calibrations[0];
    assert_eq!(c.recipe, r);
    assert_eq!(c.curve.recipe(), &r.curve);
    assert_eq!(c.curve.diagnostics().quotes.len(), 6);
    for quote in &c.curve.diagnostics().quotes {
        assert!(quote.residual.abs() <= quote.tolerance);
        assert_eq!(quote.input.unit, quote.model.unit);
    }
    let (t, q) = c.curve.interpolation_grid()?;
    assert_eq!((t[0], q[0]), (0.0, 1.0));
    assert!(q.windows(2).all(|p| p[1] > 0.0 && p[1] <= p[0]));
    assert!(c
        .hazards_per_act365f_year
        .iter()
        .all(|h| *h >= 0.0 && *h <= 1.0));
    let trade = trade(&r, &cal, 5, "0.01")?;
    let v = convex_credit::price_cds(
        &trade,
        &trade
            .terms()
            .trade_context(InitialPaymentState::NotContracted),
        &result.curves,
    )?;
    assert_eq!(
        v.credit_input,
        convex_credit::CreditInput::Supplied { curve: r.curve.key }
    );
    assert!(result.price_evaluations <= MAX_CREDIT_PRICE_EVALUATIONS);
    Ok(())
}
#[test]
fn global_fit_identifies_same_curve_and_source_order_cannot_change_results(
) -> Result<(), Box<dyn Error>> {
    let data = inputs()?;
    let r = global(&recipe(&data)?);
    let cal = calendar(&data);
    let supplied = supplied(&data)?;
    let build = |r: &CdsCurveRecipe| {
        let bindings = calendars(r, &cal);
        build_credit_curve_set(
            &[CdsCurveBuildInput {
                recipe: r,
                calendars: &bindings,
            }],
            &supplied,
            FitNumericalControls::default(),
        )
    };
    let a = build(&r)?;
    let mut reversed = r.clone();
    reversed.helpers.reverse();
    reversed.curve.source =
        CurveSource::Helpers(reversed.helpers.iter().map(|h| h.id.clone()).collect());
    if let CalibrationMode::CreditGlobal(s) = &mut reversed.curve.calibration {
        s.quote_scales.reverse();
        s.weights.reverse();
    }
    let b = build(&reversed)?;
    assert_eq!(
        a.calibrations[0].hazards_per_act365f_year,
        b.calibrations[0].hazards_per_act365f_year
    );
    if let CreditSolverDiagnostics::Global(d) = &a.calibrations[0].solver {
        assert_eq!(d.rank, Some(6));
        assert!(d.scaled_rms.is_some());
    } else {
        panic!("actual global solver required");
    }
    for (i, q) in b.calibrations[0]
        .curve
        .diagnostics()
        .quotes
        .iter()
        .enumerate()
    {
        assert_eq!(q.helper_id, reversed.helpers[i].id);
        assert!(q.residual.abs() <= q.tolerance);
    }
    Ok(())
}
#[test]
fn duplicate_missing_units_versions_recovery_and_unreachable_roots_fail_atomically(
) -> Result<(), Box<dyn Error>> {
    let data = inputs()?;
    let r = recipe(&data)?;
    let cal = calendar(&data);
    let s = supplied(&data)?;
    let failed = |r: &CdsCurveRecipe, s: &CurveSet| {
        let b = calendars(r, &cal);
        build_credit_curve_set(
            &[CdsCurveBuildInput {
                recipe: r,
                calendars: &b,
            }],
            s,
            FitNumericalControls::default(),
        )
        .is_err()
    };
    for mutation in 0..12 {
        let mut bad = r.clone();
        match mutation {
            0 => bad.helpers[1].pillar = bad.helpers[0].pillar,
            1 => bad.helpers[0].quote.unit = QuoteUnit::UpfrontFraction,
            2 => bad.helpers[0].quote.value = f64::NAN,
            3 => bad.helpers[0].tolerance = 0.0,
            4 => bad.helpers[0].terms.reference.seniority = Seniority::Subordinated,
            5 => bad.helpers[0].terms.discount_curve.version = "missing".into(),
            6 => bad.helpers[0].terms.premium_calendar.version = Version::new(2)?,
            7 => bad.pillars.reverse(),
            8 => bad.bootstrap.as_mut().unwrap().upper_hazard = 0.0001,
            9 => bad.bootstrap.as_mut().unwrap().max_iterations = 1,
            10 => bad.helpers[0].terms.reference.recovery_fraction = FiniteF64::new(1.0)?,
            _ => bad.curve.extrapolation = convex_curves::ExtrapolationMethod::Flat,
        }
        // Actual calendar pins independently remain ORIGINAL, not rewritten to match.
        let original = calendars(&r, &cal);
        assert!(
            build_credit_curve_set(
                &[CdsCurveBuildInput {
                    recipe: &bad,
                    calendars: &original
                }],
                &s,
                FitNumericalControls::default()
            )
            .is_err(),
            "mutation {mutation}"
        );
        assert_eq!(s.len(), 1);
    }
    assert!(failed(&r, &CurveSet::new()));
    let mut other = r.clone();
    other.curve.key.id = "Second.Credit".into();
    for h in &mut other.helpers {
        h.terms.survival_curve = other.curve.key.clone();
    }
    other.helpers[0].quote.value = -10.0;
    let first = calendars(&r, &cal);
    let second = calendars(&other, &cal);
    assert!(build_credit_curve_set(
        &[
            CdsCurveBuildInput {
                recipe: &r,
                calendars: &first
            },
            CdsCurveBuildInput {
                recipe: &other,
                calendars: &second
            }
        ],
        &s,
        FitNumericalControls::default()
    )
    .is_err());
    assert_eq!(s.len(), 1);
    Ok(())
}

#[test]
fn scenario_axes_preserve_other_inputs_and_unknown_risk_is_never_zero() -> Result<(), Box<dyn Error>>
{
    let data = inputs()?;
    let r = recipe(&data)?;
    let cal = calendar(&data);
    let s = supplied(&data)?;
    let bindings = calendars(&r, &cal);
    let t = trade(&r, &cal, 5, "0.01")?;
    let ctx = t.terms().trade_context(InitialPaymentState::NotContracted);
    let p = prepare_credit_risk(CreditRiskInput {
        recipe: &r,
        helper_calendars: &bindings,
        supplied: &s,
        trade: &t,
        trade_calendars: binding(t.terms(), &cal),
        context: &ctx,
        controls: FitNumericalControls::default(),
        risk_model: ModelId::new("supplied.credit.central.v1")?,
    })?;
    for (i, shock) in [
        CreditShock::DiscountZero {
            output_discount: CurveKey {
                id: t.terms().discount_curve.id.clone(),
                version: "rates-plus".into(),
            },
            shift_decimal: 0.0001,
            credit_response: CreditResponse::HoldSurvival,
        },
        CreditShock::Recovery {
            fraction: FiniteF64::new(0.45)?,
            credit_response: CreditResponse::RecalibrateQuotes,
        },
        CreditShock::Quotes {
            moves: vec![CdsQuoteMove {
                helper_id: r.helpers[5].id.clone(),
                expected_quote_version: r.helpers[5].quote_version,
                delta: MarketQuote {
                    value: 0.01,
                    unit: QuoteUnit::PercentRate,
                },
            }],
        },
    ]
    .into_iter()
    .enumerate()
    {
        let v = evaluate_credit_scenario(
            &p,
            &CreditScenarioRequest {
                id: format!("Scenario.{i}"),
                output_credit: CurveKey {
                    id: r.curve.key.id.clone(),
                    version: format!("scenario-{i}"),
                },
                shock,
            },
        )?;
        assert_eq!(v.base, p.base_value().clone());
        assert_eq!(v.pv_change_currency, v.shocked.net_pv - v.base.net_pv);
        match i {
            0 => {
                assert!(v.calibration.is_none());
                assert_eq!(
                    v.credit_curve.interpolation_grid()?.1,
                    p.baseline().curve.interpolation_grid()?.1
                );
                assert_eq!(
                    v.scenario_recipe
                        .helpers
                        .iter()
                        .map(|h| h.quote)
                        .collect::<Vec<_>>(),
                    r.helpers.iter().map(|h| h.quote).collect::<Vec<_>>()
                );
            }
            1 => {
                assert_eq!(v.shocked.discount_curve, t.terms().discount_curve);
                assert!(v.calibration.is_some());
                assert_eq!(
                    v.scenario_recipe
                        .helpers
                        .iter()
                        .map(|h| h.quote)
                        .collect::<Vec<_>>(),
                    r.helpers.iter().map(|h| h.quote).collect::<Vec<_>>()
                );
            }
            _ => {
                assert_eq!(v.scenario_recipe.curve.role, r.curve.role);
                assert_eq!(v.shocked.discount_curve, t.terms().discount_curve);
            }
        }
    }
    let failed = CreditScenarioRequest {
        id: "missing".into(),
        output_credit: CurveKey {
            id: r.curve.key.id.clone(),
            version: "missing-1".into(),
        },
        shock: CreditShock::Quotes {
            moves: vec![CdsQuoteMove {
                helper_id: "absent".into(),
                expected_quote_version: Version::new(1)?,
                delta: MarketQuote {
                    value: 1.0,
                    unit: QuoteUnit::BasisPoints,
                },
            }],
        },
    };
    assert!(evaluate_credit_scenario(&p, &failed).is_err());
    Ok(())
}

#[test]
fn immediate_default_cash_is_explicit_pending_and_buyer_seller_offset() -> Result<(), Box<dyn Error>>
{
    let data = inputs()?;
    let r = recipe(&data)?;
    let cal = calendar(&data);
    let s = supplied(&data)?;
    let bindings = calendars(&r, &cal);
    let mut values = Vec::new();
    for side in [CdsSide::ProtectionBuyer, CdsSide::ProtectionSeller] {
        let original = trade(&r, &cal, 5, "0.01")?;
        let mut terms = original.terms().clone();
        terms.side = side;
        let t = terms.build(&binding(&terms, &cal))?;
        let ctx = t.terms().trade_context(InitialPaymentState::NotContracted);
        let p = prepare_credit_risk(CreditRiskInput {
            recipe: &r,
            helper_calendars: &bindings,
            supplied: &s,
            trade: &t,
            trade_calendars: binding(t.terms(), &cal),
            context: &ctx,
            controls: FitNumericalControls::default(),
            risk_model: ModelId::new("supplied.credit.central.v1")?,
        })?;
        let mut event = ImmediateDefaultScenario {
            event_id: "Default.supplied".into(),
            entity_id: terms.reference.entity_id.clone(),
            seniority: terms.reference.seniority,
            default_date: ctx.step_in_date,
            cash_settlement_date: ctx.cash_settlement_date,
            accrual_day_fraction: FiniteF64::new(0.5)?,
            auction_recovery: None,
            auction_reference: None,
        };
        assert!(matches!(
            immediate_jump_to_default(&p, &event)?,
            DefaultScenarioResult::Pending { .. }
        ));
        event.auction_recovery = Some(FiniteF64::new(0.25)?);
        event.auction_reference = Some(terms.reference.document.clone());
        let result = immediate_jump_to_default(&p, &event)?;
        if let DefaultScenarioResult::Valued {
            protection_cash,
            accrued_premium_cash,
            post_default_pv,
            jump_to_default_currency,
            ..
        } = result
        {
            let sign = if side == CdsSide::ProtectionBuyer {
                1.0
            } else {
                -1.0
            };
            assert!((protection_cash - sign * 7_500_000.0).abs() < 1e-7);
            assert!((accrued_premium_cash + sign * 10_000_000.0 * 0.01 * 1.5 / 360.0).abs() < 1e-7);
            let df = s
                .get(&terms.discount_curve)?
                .query(&CurveQuery::DiscountFactor {
                    point: CurvePoint::Date(ctx.cash_settlement_date),
                })?
                .value;
            assert!(
                (post_default_pv - sign * (7_500_000.0 - 10_000_000.0 * 0.01 * 1.5 / 360.0) * df)
                    .abs()
                    < 1e-7
            );
            assert_eq!(
                jump_to_default_currency,
                post_default_pv - p.base_value().net_pv
            );
            values.push(jump_to_default_currency);
        } else {
            panic!("valued event expected");
        }
        event.default_date = Date::parse("2022-06-23")?;
        assert!(immediate_jump_to_default(&p, &event).is_err());
    }
    assert!((values[0] + values[1]).abs() < 1e-7);
    Ok(())
}

#[test]
fn bond_cds_hedge_requires_actual_entity_priority_basis_recovery_and_versions(
) -> Result<(), Box<dyn Error>> {
    let data = inputs()?;
    let r = recipe(&data)?;
    let cal = calendar(&data);
    let s = supplied(&data)?;
    let bindings = calendars(&r, &cal);
    let output = build_credit_curve_set(
        &[CdsCurveBuildInput {
            recipe: &r,
            calendars: &bindings,
        }],
        &s,
        FitNumericalControls::default(),
    )?;
    let t = trade(&r, &cal, 5, "0.01")?;
    let credit = &output.calibrations[0].curve;
    let bond = BondCreditIdentity {
        instrument_id: InstrumentId::new("Supplied.Bond")?,
        instrument_version: Version::new(1)?,
        reference: t.terms().reference.document.clone(),
        entity_id: t.terms().reference.entity_id.clone(),
        seniority: t.terms().reference.seniority,
        currency: t.terms().currency,
        maturity: Date::parse("2032-06-20")?,
    };
    let basis = BondBasisInput {
        pin: t.terms().reference.restructuring_clause.clone(),
        currency: bond.currency,
        entity_id: bond.entity_id.clone(),
        seniority: bond.seniority,
        recovery_fraction: t.terms().reference.recovery_fraction,
        model_version: ModelId::new("continuous-zero-residual.supplied.v1")?,
        reference_date: r.curve.reference_date,
        dated_spreads: vec![
            (
                r.curve.reference_date,
                MarketQuote {
                    value: 0.0,
                    unit: QuoteUnit::BasisPoints,
                },
            ),
            (
                bond.maturity,
                MarketQuote {
                    value: 20.0,
                    unit: QuoteUnit::BasisPoints,
                },
            ),
        ],
        liquidity_assumption: t.terms().reference.document.clone(),
    };
    let mapping = BondCdsMapping {
        pin: t.terms().reference.document.clone(),
        bond: bond.clone(),
        cds_reference: t.terms().reference.clone(),
        credit_curve: r.curve.key.clone(),
        basis: basis.pin.clone(),
        liquidity_assumption: basis.liquidity_assumption.clone(),
    };
    assert!(admit_bond_cds_hedge(None, &bond, &t, credit, &basis).is_err());
    let admitted = admit_bond_cds_hedge(Some(&mapping), &bond, &t, credit, &basis)?;
    assert_eq!(admitted.basis(), &basis);
    for i in 0..8 {
        let mut m = mapping.clone();
        let mut b = basis.clone();
        match i {
            0 => m.bond.entity_id = EntityId::new("Other")?,
            1 => m.bond.seniority = Seniority::Subordinated,
            2 => m.bond.instrument_version = Version::new(2)?,
            3 => m.credit_curve.version = "missing".into(),
            4 => b.dated_spreads.clear(),
            5 => b.recovery_fraction = FiniteF64::new(0.5)?,
            6 => b.liquidity_assumption.version = Version::new(2)?,
            _ => b.dated_spreads[1].0 = Date::parse("2027-06-20")?,
        }
        assert!(
            admit_bond_cds_hedge(Some(&m), &bond, &t, credit, &b).is_err(),
            "mapping mutation {i}"
        );
    }
    Ok(())
}
