#[path = "../../convex/examples/support/cash_fixture.rs"]
mod cash_support;
mod credit_support;
#[path = "support/futures.rs"]
mod futures_support;
#[path = "../../convex/examples/support/rate_fixture.rs"]
mod rate_support;
#[path = "../../convex/examples/support/quote_fixture.rs"]
mod support;
use convex_analytics::{
    cash_calibration::*, credit_calibration::*, quote_risk::*, rate_calibration::*,
};
use convex_core::{contracts::*, fixings::FixingSeries, Currency};
use convex_curves::contracts::*;
use support::{failure, key, position, settings, snapshot, Error};
fn close(a: f64, b: f64, t: f64) {
    assert!(
        (a - b).abs() <= t,
        "actual={a:.12} expected={b:.12} delta={:.12}",
        a - b
    );
}

#[test]
fn original_full_stepped_cash_yield_has_independent_central_and_half_step_reference(
) -> Result<(), Error> {
    let f = support::Fixture::load()?;
    for c in f.data["cases"].as_array().ok_or("missing cases")? {
        let num = |name: &str| -> Result<f64, Error> {
            Ok(c[name]
                .as_str()
                .ok_or("missing Decimal reference")?
                .parse()?)
        };
        let result = f.raw_yield(&f.helper.terms.face_value.to_string(), num("bump")?)?;
        for (a, n) in [
            (result.base_pv, "base"),
            (result.plus_pv, "plus"),
            (result.minus_pv, "minus"),
            (result.half_plus_pv, "half_plus"),
            (result.half_minus_pv, "half_minus"),
            (result.value, "value"),
            (result.half_value, "half_value"),
        ] {
            close(a, num(n)?, 1e-8);
        }
        close(result.dv01_loss()?, -result.value, 0.0);
        assert!(result.value < 0.0);
    }
    Ok(())
}
#[test]
fn long_short_zero_and_quantity_scaling_use_original_face_once() -> Result<(), Error> {
    let f = support::Fixture::load()?;
    let base = f.raw_yield("5000", 0.0001)?;
    for (q, scale) in [("10000", 2.0), ("-15000", -3.0), ("0", 0.0)] {
        let value = f.raw_yield(q, 0.0001)?;
        close(value.value, base.value * scale, 1e-9);
        close(value.base_pv, base.base_pv * scale, 1e-8);
    }
    Ok(())
}
#[test]
fn incompatible_units_nonfinite_currency_and_reference_quantity_fail() -> Result<(), Error> {
    let f = support::Fixture::load()?;
    let r = f.raw_yield("5000", 0.0001)?;
    for q in ["-1", "0"] {
        let mut p = r.position.clone();
        p.reference_quantity = ExactDecimal::parse(q)?;
        assert!(p.scale().is_err());
    }
    let mut p = r.position.clone();
    p.quantity_unit = RiskQuantityUnit::Contracts;
    p.quantity = ExactDecimal::parse("0.5")?;
    assert!(p.scale().is_err());
    let mut definition = r.key.definition().clone();
    definition.unit = QuoteRiskUnit::CurrencyPerFraction;
    assert!(QuoteRiskKey::new(definition).is_err());
    assert!(risk_reference_pv(f64::NAN, Currency::USD).is_err());
    assert!(reference_quote_sensitivity(
        r.key.clone(),
        r.position.clone(),
        snapshot()?,
        settings(0.0001)?,
        |_| risk_reference_pv(100., Currency::EUR)
    )
    .is_err());
    for bump in [f64::INFINITY, 0.0, -1.0, 0.2] {
        assert!(reference_quote_sensitivity(
            r.key.clone(),
            r.position.clone(),
            snapshot()?,
            settings(bump)?,
            |_| risk_reference_pv(100., Currency::USD)
        )
        .is_err());
    }
    Ok(())
}
#[test]
fn unstable_or_failed_bumps_return_no_accepted_sensitivity() -> Result<(), Error> {
    let f = support::Fixture::load()?;
    let r = f.raw_yield("5000", 0.0001)?;
    let nonlinear = reference_quote_sensitivity(
        r.key.clone(),
        r.position.clone(),
        snapshot()?,
        settings(0.0001)?,
        |d| risk_reference_pv(1e15 * d * d * d, Currency::USD),
    );
    assert!(nonlinear.is_err());
    let mut calls = 0;
    assert!(
        reference_quote_sensitivity(r.key, r.position, snapshot()?, settings(0.0001)?, |d| {
            calls += 1;
            if d < 0.0 {
                Err(failure("missing original curve"))
            } else {
                risk_reference_pv(100., Currency::USD)
            }
        })
        .is_err()
    );
    assert_eq!(calls, 3);
    Ok(())
}
#[test]
fn strict_key_and_retained_sensitivity_json_fail_closed() -> Result<(), Error> {
    let f = support::Fixture::load()?;
    let r = f.raw_yield("5000", 0.0001)?;
    let json = serde_json::to_string(&r)?;
    let mut read: QuoteSensitivity = serde_json::from_str(&json)?;
    read.validate()?;
    read.value *= 10000.;
    assert!(read.validate().is_err());
    let mut json = serde_json::to_value(&r)?;
    json["ignored"] = "extra".into();
    assert!(serde_json::from_value::<QuoteSensitivity>(json).is_err());
    let mut json = serde_json::to_value(&r.key)?;
    json["native_unit"] = "DiscountFactor".into();
    assert!(serde_json::from_value::<QuoteRiskKey>(json).is_err());
    Ok(())
}
#[test]
fn distinct_quote_bases_response_and_native_units_have_distinct_complete_keys() -> Result<(), Error>
{
    let curve = CurveKey {
        id: "same-object".into(),
        version: "original-v1".into(),
    };
    let mut keys = std::collections::HashSet::new();
    for (q, c, u, r, t) in [
        (
            QuoteType::Yield,
            RiskCoordinate::Raw {
                input_id: "same-object".into(),
                version: Version::new(1)?,
            },
            QuoteUnit::DecimalRate,
            RiskResponse::FrozenCurves,
            "spot",
        ),
        (
            QuoteType::ZeroRate,
            RiskCoordinate::Zero {
                curve: curve.clone(),
                node: None,
            },
            QuoteUnit::DecimalRate,
            RiskResponse::FrozenCurves,
            "parallel",
        ),
        (
            QuoteType::SwapParRate,
            RiskCoordinate::Quote {
                curve: curve.clone(),
                helper_id: "same-quote".into(),
                quote_version: Version::new(1)?,
            },
            QuoteUnit::DecimalRate,
            RiskResponse::RebuildDependents,
            "5Y",
        ),
        (
            QuoteType::ZSpread,
            RiskCoordinate::Raw {
                input_id: "same-object".into(),
                version: Version::new(1)?,
            },
            QuoteUnit::DecimalRate,
            RiskResponse::FrozenCurves,
            "spot",
        ),
        (
            QuoteType::CdsSpread,
            RiskCoordinate::Quote {
                curve: curve.clone(),
                helper_id: "same-quote".into(),
                quote_version: Version::new(1)?,
            },
            QuoteUnit::DecimalRate,
            RiskResponse::RebuildDependents,
            "5Y",
        ),
        (
            QuoteType::RecoveryRate,
            RiskCoordinate::Recovery {
                curve: curve.clone(),
            },
            QuoteUnit::DecimalRate,
            RiskResponse::RebuildDependents,
            "parallel",
        ),
        (
            QuoteType::RecoveryRate,
            RiskCoordinate::Recovery {
                curve: curve.clone(),
            },
            QuoteUnit::DecimalRate,
            RiskResponse::FrozenCurves,
            "parallel",
        ),
        (
            QuoteType::ResidualBasis,
            RiskCoordinate::Basis {
                curve: curve.clone(),
                node: None,
            },
            QuoteUnit::DecimalRate,
            RiskResponse::FrozenCurves,
            "parallel",
        ),
        (
            QuoteType::HazardRate,
            RiskCoordinate::Hazard {
                curve: curve.clone(),
                node: None,
            },
            QuoteUnit::DecimalRate,
            RiskResponse::FrozenCurves,
            "parallel",
        ),
    ] {
        assert!(keys.insert(key(q, c, u, r, t)?));
    }
    assert_eq!(keys.len(), 9);
    for k in &keys {
        if matches!(
            k.definition().coordinate,
            RiskCoordinate::Zero { .. } | RiskCoordinate::Basis { .. }
        ) {
            let mut invalid = k.definition().clone();
            invalid.native_unit = QuoteUnit::BasisPoints;
            assert!(QuoteRiskKey::new(invalid).is_err());
        }
        if matches!(
            k.definition().coordinate,
            RiskCoordinate::Zero { node: None, .. }
                | RiskCoordinate::Basis { node: None, .. }
                | RiskCoordinate::Hazard { node: None, .. }
        ) {
            let mut invalid = k.definition().clone();
            let mut factor = invalid.factor.definition().clone();
            factor.tenor = FactorTenor::new("1Y")?;
            invalid.factor = RiskFactorKey::new(factor)?;
            assert!(QuoteRiskKey::new(invalid).is_err());
        }
    }
    let decimal = keys
        .iter()
        .find(|k| k.definition().factor.definition().quote_type == QuoteType::CdsSpread)
        .ok_or("missing CDS")?;
    let mut percent = decimal.definition().clone();
    percent.native_unit = QuoteUnit::PercentRate;
    assert_ne!(decimal, &QuoteRiskKey::new(percent)?);
    Ok(())
}
#[test]
fn government_quote_bumps_rebuild_actual_issuer_inputs_and_preserve_last_good() -> Result<(), Error>
{
    for name in ["positive", "negative"] {
        let f = cash_support::Fixture::load(name)?;
        let resources = f.resources(&f.recipes);
        let supplied = f.supplied()?;
        let history = FixingSeries::new(vec![])?;
        let inputs = f
            .recipes
            .iter()
            .zip(&resources)
            .map(|(recipe, resources)| CashCurveBuildInput { recipe, resources })
            .collect::<Vec<_>>();
        let graph = prepare_quote_risk_graph(
            QuoteRiskGraphInput {
                rates: &[],
                credit: &[],
                cash: &inputs,
                supplied: &supplied,
                history: &history,
                rate_context: None,
                controls: FitNumericalControls::default(),
            },
            |_, _| {
                Err(convex_core::cashflows::CashflowError::BoundExceeded(
                    "unexpected history",
                ))
            },
        )?;
        let gov = f
            .recipes
            .iter()
            .find(|r| matches!(r.curve.role, CurveRole::Government { .. }))
            .ok_or("missing gov")?;
        let CashCalibrationHelper::Bond { input: h } = &gov.helpers[1] else {
            return Err("missing actual cash".into());
        };
        let k = key(
            QuoteType::CashCleanPrice,
            RiskCoordinate::Quote {
                curve: gov.curve.key.clone(),
                helper_id: h.id.clone(),
                quote_version: h.quote_version,
            },
            h.quote.unit,
            RiskResponse::RebuildDependents,
            "1Y",
        )?;
        let changed = evaluate_quote_graph(&graph, &k, 0.01)?;
        assert_eq!(changed.repricing.len(), 8);
        assert!(changed.price_evaluations > 0);
        assert_eq!(changed.build_order.len(), 3);
        for q in &changed.repricing {
            assert!(q.residual.abs() <= q.tolerance);
        }
        let basis = f
            .recipes
            .iter()
            .find(|r| r.basis_model.is_some())
            .ok_or("basis")?;
        assert_ne!(
            changed
                .curves
                .get(&basis.curve.key)?
                .interpolation_grid()?
                .1,
            graph
                .baseline()
                .curves
                .get(&basis.curve.key)?
                .interpolation_grid()?
                .1
        );
        assert_eq!(gov.helpers[1].quote(), h.quote);
        let mut wrong = k.definition().clone();
        if let RiskCoordinate::Quote { quote_version, .. } = &mut wrong.coordinate {
            *quote_version = Version::new(2)?;
        }
        assert!(evaluate_quote_graph(&graph, &QuoteRiskKey::new(wrong)?, 0.01).is_err());
        assert_eq!(graph.baseline().repricing.len(), 8);
    }
    Ok(())
}
#[test]
fn mixed_rate_quote_risk_prices_full_irs_ois_and_preserves_known_history() -> Result<(), Error> {
    for name in ["nonflat-positive", "nonflat-negative"] {
        let f = rate_support::Fixture::load(name)?;
        let resources = f.resources(&f.recipes);
        let supplied = CurveSet::new();
        let versions = f
            .recipes
            .iter()
            .map(|r| vec![Version::new(1).unwrap(); r.helpers.len()])
            .collect::<Vec<_>>();
        let inputs = f
            .recipes
            .iter()
            .zip(&resources)
            .zip(&versions)
            .map(|((recipe, resources), quote_versions)| RateRiskCurveInput {
                build: RateCurveBuildInput { recipe, resources },
                quote_versions,
            })
            .collect::<Vec<_>>();
        let graph = prepare_quote_risk_graph(
            QuoteRiskGraphInput {
                rates: &inputs,
                credit: &[],
                cash: &[],
                supplied: &supplied,
                history: &f.history,
                rate_context: Some(f.context()?),
                controls: FitNumericalControls::default(),
            },
            |i, d| f.publication(i, d),
        )?;
        for (ri, hi, q) in [
            (0, 2, QuoteType::OisParRate),
            (1, 2, QuoteType::SwapParRate),
        ] {
            let r = &f.recipes[ri];
            let h = &r.helpers[hi];
            let RateCalibrationTerms::Swap(t) = &h.terms else {
                return Err("full swap needed".into());
            };
            let native = t.as_ref().clone().build(&f.swap_calendars(t))?;
            let k = key(
                q,
                RiskCoordinate::Quote {
                    curve: r.curve.key.clone(),
                    helper_id: h.id.clone(),
                    quote_version: Version::new(1)?,
                },
                h.quote.unit,
                RiskResponse::RebuildDependents,
                "1Y",
            )?;
            let mut base: Option<QuoteSensitivity> = None;
            for (qty, scale) in [("1", 1.0), ("-2", -2.0), ("3", 3.0)] {
                let result = curve_quote_sensitivity(
                    &graph,
                    k.clone(),
                    position(qty, "1", RiskQuantityUnit::TradeCopies)?,
                    snapshot()?,
                    settings(0.0001)?,
                    |s| {
                        let v = convex_rates::pricing::price_swap(
                            &native,
                            &convex_rates::pricing::SwapValuationInputs {
                                curves: &s.curves,
                                fixings: &f.history,
                            },
                            f.context().map_err(failure)?.cashflows,
                            |i, d| f.publication(i, d),
                        )
                        .map_err(failure)?;
                        Ok(v.present_value)
                    },
                )?;
                if let Some(first) = &base {
                    close(result.value, first.value * scale, 1e-8);
                } else {
                    assert!(result.value.abs() > 1.);
                    base = Some(result);
                }
            }
            let moved = evaluate_quote_graph(&graph, &k, 0.0001)?;
            assert_eq!(moved.repricing.len(), 9);
            assert_eq!(moved.publications, graph.baseline().publications);
        }
    }
    Ok(())
}

fn recovery_key(r: &CdsCurveRecipe, response: RiskResponse) -> Result<QuoteRiskKey, Error> {
    let k = key(
        QuoteType::RecoveryRate,
        RiskCoordinate::Recovery {
            curve: r.curve.key.clone(),
        },
        QuoteUnit::DecimalRate,
        response,
        "parallel",
    )?;
    let mut d = k.definition().clone();
    let mut f = d.factor.definition().clone();
    f.target = FactorTarget::Entity {
        entity_id: r.helpers[0].terms.reference.entity_id.clone(),
    };
    d.factor = RiskFactorKey::new(f)?;
    Ok(QuoteRiskKey::new(d)?)
}
#[test]
fn full_cds_quote_rebuild_has_protection_signs_and_matches_existing_native_reference(
) -> Result<(), Error> {
    let data = credit_support::inputs()?;
    let r = credit_support::recipe(&data)?;
    let calendar = credit_support::calendar(&data);
    let supplied = credit_support::supplied(&data)?;
    let calendars = credit_support::calendars(&r, &calendar);
    let history = FixingSeries::new(vec![])?;
    let inputs = [CdsCurveBuildInput {
        recipe: &r,
        calendars: &calendars,
    }];
    let graph = prepare_quote_risk_graph(
        QuoteRiskGraphInput {
            rates: &[],
            credit: &inputs,
            cash: &[],
            supplied: &supplied,
            history: &history,
            rate_context: None,
            controls: FitNumericalControls::default(),
        },
        |_, _| {
            Err(convex_core::cashflows::CashflowError::BoundExceeded(
                "unexpected",
            ))
        },
    )?;
    let trade = credit_support::trade(&r, &calendar, 5, "0.01")?;
    let ctx = trade
        .terms()
        .trade_context(convex_credit::InitialPaymentState::NotContracted);
    let h = &r.helpers[5];
    let k = key(
        QuoteType::CdsSpread,
        RiskCoordinate::Quote {
            curve: r.curve.key.clone(),
            helper_id: h.id.clone(),
            quote_version: h.quote_version,
        },
        h.quote.unit,
        RiskResponse::RebuildDependents,
        "5Y",
    )?;
    let mut first = None;
    for (q, s) in [("1", 1.), ("-2", -2.), ("3", 3.)] {
        let result = curve_quote_sensitivity(
            &graph,
            k.clone(),
            position(q, "1", RiskQuantityUnit::TradeCopies)?,
            snapshot()?,
            settings(0.0001)?,
            |s| {
                let v = convex_credit::price_cds(&trade, &ctx, &s.curves).map_err(failure)?;
                risk_reference_pv(v.net_pv, v.currency)
            },
        )?;
        if let Some(v) = first {
            close(result.value, v * s, 1e-8);
        } else {
            assert!(result.value > 0.);
            first = Some(result.value);
        }
    }
    let prepared = convex_analytics::credit_risk::prepare_credit_risk(
        convex_analytics::credit_risk::CreditRiskInput {
            recipe: &r,
            helper_calendars: &calendars,
            supplied: &supplied,
            trade: &trade,
            trade_calendars: credit_support::binding(trade.terms(), &calendar),
            context: &ctx,
            controls: FitNumericalControls::default(),
            risk_model: ModelId::new("supplied.credit.central.v1")?,
        },
    )?;
    let bump = convex_analytics::credit_risk::CreditRiskBump::RunningQuote {
        helper_id: h.id.clone(),
        step_bps: 1.,
    };
    let native = convex_analytics::credit_risk::credit_sensitivity(
        &prepared,
        &credit_support::factor(&r, &bump)?,
        bump,
        &credit_support::revisions(&r, "CVX024-reference", false),
    )?;
    close(first.ok_or("no risk")?, native.value, 1e-6);
    let mut seller = trade.terms().clone();
    seller.side = convex_credit::CdsSide::ProtectionSeller;
    let seller = seller.build(&credit_support::binding(&seller, &calendar))?;
    let value = curve_quote_sensitivity(
        &graph,
        k,
        position("1", "1", RiskQuantityUnit::TradeCopies)?,
        snapshot()?,
        settings(0.0001)?,
        |s| {
            let v = convex_credit::price_cds(&seller, &ctx, &s.curves).map_err(failure)?;
            risk_reference_pv(v.net_pv, v.currency)
        },
    )?;
    close(value.value, -first.ok_or("no risk")?, 1e-6);
    Ok(())
}
#[test]
fn cds_recovery_and_hazard_are_separate_held_and_rebuilt_economics() -> Result<(), Error> {
    let data = credit_support::inputs()?;
    let r = credit_support::recipe(&data)?;
    let calendar = credit_support::calendar(&data);
    let supplied = credit_support::supplied(&data)?;
    let calendars = credit_support::calendars(&r, &calendar);
    let history = FixingSeries::new(vec![])?;
    let inputs = [CdsCurveBuildInput {
        recipe: &r,
        calendars: &calendars,
    }];
    let graph = prepare_quote_risk_graph(
        QuoteRiskGraphInput {
            rates: &[],
            credit: &inputs,
            cash: &[],
            supplied: &supplied,
            history: &history,
            rate_context: None,
            controls: FitNumericalControls::default(),
        },
        |_, _| {
            Err(convex_core::cashflows::CashflowError::BoundExceeded(
                "unexpected",
            ))
        },
    )?;
    let trade = credit_support::trade(&r, &calendar, 5, "0.01")?;
    let ctx = trade
        .terms()
        .trade_context(convex_credit::InitialPaymentState::NotContracted);
    let mut values = Vec::new();
    for response in [RiskResponse::FrozenCurves, RiskResponse::RebuildDependents] {
        let k = recovery_key(&r, response)?;
        let result = curve_quote_sensitivity(
            &graph,
            k.clone(),
            position("1", "1", RiskQuantityUnit::TradeCopies)?,
            snapshot()?,
            settings(0.001)?,
            |s| {
                let mut t = trade.terms().clone();
                let CurveRole::Credit { recovery, .. } = s.curves.get(&r.curve.key)?.recipe().role
                else {
                    return Err(failure("credit role"));
                };
                t.reference.recovery_fraction = FiniteF64::new(recovery).map_err(failure)?;
                let c = t
                    .clone()
                    .build(&credit_support::binding(&t, &calendar))
                    .map_err(failure)?;
                {
                    let v = convex_credit::price_cds(&c, &ctx, &s.curves).map_err(failure)?;
                    risk_reference_pv(v.net_pv, v.currency)
                }
            },
        )?;
        assert_eq!(
            result.key.definition().unit,
            QuoteRiskUnit::CurrencyPerFraction
        );
        values.push(result);
        let scenario = evaluate_quote_graph(&graph, &k, 0.001)?;
        assert_eq!(
            scenario.repricing.is_empty(),
            response == RiskResponse::FrozenCurves
        );
        assert!(evaluate_quote_graph(&graph, &k, 1.0).is_err());
    }
    assert_ne!(values[0].key, values[1].key);
    assert!((values[0].value - values[1].value).abs() > 1.0);
    assert!(values[0].value < 0.0);
    let k = key(
        QuoteType::HazardRate,
        RiskCoordinate::Hazard {
            curve: r.curve.key.clone(),
            node: None,
        },
        QuoteUnit::DecimalRate,
        RiskResponse::FrozenCurves,
        "parallel",
    )?;
    let result = curve_quote_sensitivity(
        &graph,
        k,
        position("1", "1", RiskQuantityUnit::TradeCopies)?,
        snapshot()?,
        settings(0.0001)?,
        |s| {
            let v = convex_credit::price_cds(&trade, &ctx, &s.curves).map_err(failure)?;
            risk_reference_pv(v.net_pv, v.currency)
        },
    )?;
    assert!(result.value > 0.0);
    assert_eq!(
        result.key.definition().unit,
        QuoteRiskUnit::CurrencyPerAnnualRate
    );
    Ok(())
}
#[test]
fn cds_quote_shock_rebuilds_full_bond_residual_basis_and_recovery_is_not_merged(
) -> Result<(), Error> {
    let f = cash_support::CreditFixture::load()?;
    let calendar = &f.calendar;
    let calendars = credit_support::calendars(&f.cds, calendar);
    let discount = f.cds.helpers[0].terms.discount_curve.clone();
    let supplied = CurveSet::new().with_curve(f.supplied.get(&discount)?.as_ref().clone())?;
    let history = FixingSeries::new(vec![])?;
    let recipes = [f.recipe.clone()];
    let resources = cash_support::resources(&recipes, calendar);
    let cash = [CashCurveBuildInput {
        recipe: &recipes[0],
        resources: &resources[0],
    }];
    let credit = [CdsCurveBuildInput {
        recipe: &f.cds,
        calendars: &calendars,
    }];
    let graph = prepare_quote_risk_graph(
        QuoteRiskGraphInput {
            rates: &[],
            credit: &credit,
            cash: &cash,
            supplied: &supplied,
            history: &history,
            rate_context: None,
            controls: FitNumericalControls::default(),
        },
        |_, _| {
            Err(convex_core::cashflows::CashflowError::BoundExceeded(
                "unexpected",
            ))
        },
    )?;
    let h = &f.cds.helpers[0];
    let k = key(
        QuoteType::CdsSpread,
        RiskCoordinate::Quote {
            curve: f.cds.curve.key.clone(),
            helper_id: h.id.clone(),
            quote_version: h.quote_version,
        },
        h.quote.unit,
        RiskResponse::RebuildDependents,
        "1Y",
    )?;
    let changed = evaluate_quote_graph(&graph, &k, 0.0001)?;
    assert_eq!(changed.repricing.len(), 8);
    assert_eq!(
        changed.build_order,
        [f.cds.curve.key.clone(), f.recipe.curve.key.clone()]
    );
    assert_ne!(
        changed
            .curves
            .get(&f.recipe.curve.key)?
            .interpolation_grid()?
            .1,
        graph
            .baseline()
            .curves
            .get(&f.recipe.curve.key)?
            .interpolation_grid()?
            .1
    );
    assert_eq!(
        changed.curves.get(&f.cds.curve.key)?.recipe().role,
        graph.baseline().curves.get(&f.cds.curve.key)?.recipe().role
    );
    let k = recovery_key(&f.cds, RiskResponse::RebuildDependents)?;
    let rec = evaluate_quote_graph(&graph, &k, 0.001)?;
    assert_eq!(rec.repricing.len(), 8);
    for q in &rec.repricing {
        assert!(q.residual.abs() <= q.tolerance);
    }
    Ok(())
}
#[test]
fn native_rate_futures_projection_risk_reverses_sign_and_fully_fixed_has_zero_exposure(
) -> Result<(), Error> {
    use convex_analytics::futures::*;
    use rust_decimal::prelude::ToPrimitive;
    let fixture = futures_support::fixture();
    for coverage in ["partial", "fixed"] {
        let c = fixture["rates"]["cases"]
            .as_array()
            .ok_or("rates")?
            .iter()
            .find(|c| c["coverage"] == coverage && c["averaging"] == "compounded")
            .ok_or("case")?;
        let pack = futures_support::RatePack::new(c);
        let supplied = CurveSet::new().with_curve(pack.projection.clone())?;
        let history = FixingSeries::new(vec![])?;
        let graph = prepare_quote_risk_graph(
            QuoteRiskGraphInput {
                rates: &[],
                credit: &[],
                cash: &[],
                supplied: &supplied,
                history: &history,
                rate_context: None,
                controls: FitNumericalControls::default(),
            },
            |_, _| {
                Err(convex_core::cashflows::CashflowError::BoundExceeded(
                    "unexpected",
                ))
            },
        )?;
        let k = key(
            QuoteType::ZeroRate,
            RiskCoordinate::Zero {
                curve: pack.projection.recipe().key.clone(),
                node: None,
            },
            QuoteUnit::DecimalRate,
            RiskResponse::FrozenCurves,
            "parallel",
        )?;
        let base = price_rate_future(&pack.inputs(), |i, d| pack.publication(i, d))?;
        let mut first = None;
        for (q, scale) in [("1", 1.), ("-2", -2.), ("4", 4.)] {
            let r = curve_quote_sensitivity(
                &graph,
                k.clone(),
                position(q, "1", RiskQuantityUnit::Contracts)?,
                snapshot()?,
                settings(0.0001)?,
                |s| {
                    let mut inputs = pack.inputs();
                    inputs.projection = Some(FuturesCurve {
                        curve: s.curves.get(&pack.projection.recipe().key)?.as_ref(),
                        stamp: &pack.projection_stamp,
                    });
                    let v = price_rate_future(&inputs, |i, d| pack.publication(i, d))
                        .map_err(failure)?;
                    for (a, b) in v.observations.iter().zip(&base.observations) {
                        if matches!(a.origin, FuturesRateOrigin::Published { .. }) {
                            assert_eq!(a, b);
                        }
                    }
                    let level = v
                        .model_price_points
                        .decimal()
                        .to_f64()
                        .ok_or_else(|| failure("points"))?;
                    let start = base
                        .model_price_points
                        .decimal()
                        .to_f64()
                        .ok_or_else(|| failure("start"))?;
                    let m = pack
                        .contract
                        .currency_per_price_point
                        .decimal()
                        .to_f64()
                        .ok_or_else(|| failure("multiplier"))?;
                    risk_reference_pv((level - start) * m, pack.contract.currency)
                },
            )?;
            if let Some(v) = first {
                close(r.value, v * scale, 1e-8);
            } else {
                if coverage == "fixed" {
                    close(r.value, 0.0, 1e-8);
                } else {
                    assert!(r.value < 0.0);
                }
                first = Some(r.value);
            }
        }
    }
    Ok(())
}
#[test]
fn full_bond_future_basket_risk_and_quote_delta_use_integer_contract_multiplier_once(
) -> Result<(), Error> {
    use convex_analytics::futures::*;
    use rust_decimal::prelude::ToPrimitive;
    let pack = futures_support::BondPack::new();
    let members = pack.deliverables();
    let inputs = pack.inputs(&members);
    let original = price_bond_future(&inputs)?;
    let base = original
        .fair_price_points
        .decimal()
        .to_f64()
        .ok_or("base")?;
    let multiplier = pack
        .contract
        .currency_per_price_point
        .decimal()
        .to_f64()
        .ok_or("multiplier")?;
    let k = key(
        QuoteType::ZeroRate,
        RiskCoordinate::Zero {
            curve: pack.discount.recipe().key.clone(),
            node: None,
        },
        QuoteUnit::DecimalRate,
        RiskResponse::FrozenCurves,
        "parallel",
    )?;
    let mut first = None;
    for (qty, scale) in [("1", 1.), ("-2", -2.), ("3", 3.)] {
        let result = reference_quote_sensitivity(
            k.clone(),
            position(qty, "1", RiskQuantityUnit::Contracts)?,
            snapshot()?,
            settings(0.0001)?,
            |d| {
                if d == 0.0 {
                    return risk_reference_pv(0., pack.contract.currency);
                }
                let value = bond_futures_risk(
                    &inputs,
                    BondFuturesRiskAxis::DiscountZeros {
                        discount: FuturesCurve {
                            curve: &pack.discount,
                            stamp: &pack.curve_stamp,
                        },
                    },
                    1,
                    ExactDecimal::new(
                        rust_decimal::Decimal::from_f64_retain(d.abs())
                            .ok_or_else(|| failure("bump"))?,
                    ),
                )
                .map_err(failure)?;
                let shocked = if d > 0.0 { value.up } else { value.down };
                assert_eq!(shocked.alternatives.len(), 10);
                let points = shocked
                    .fair_price_points
                    .decimal()
                    .to_f64()
                    .ok_or_else(|| failure("points"))?;
                risk_reference_pv((points - base) * multiplier, pack.contract.currency)
            },
        )?;
        if let Some(v) = first {
            close(result.value, v * scale, 1e-8);
        } else {
            assert!(result.value < 0.0);
            first = Some(result.value);
        }
    }
    let q = key(
        QuoteType::FuturesPrice,
        RiskCoordinate::Raw {
            input_id: pack.contract.instrument_id.as_str().into(),
            version: pack.contract.stamp.version,
        },
        QuoteUnit::PricePer100,
        RiskResponse::FrozenCurves,
        "spot",
    )?;
    let result = reference_quote_sensitivity(
        q,
        position("-2", "1", RiskQuantityUnit::Contracts)?,
        snapshot()?,
        settings(0.03125)?,
        |d| risk_reference_pv(d * multiplier, pack.contract.currency),
    )?;
    let native = futures_quote_risk(&pack.contract, inputs.settlement, -2)?;
    close(
        result.value,
        native
            .currency_per_price_point
            .value()
            .decimal()
            .to_f64()
            .ok_or("native")?,
        1e-8,
    );
    assert_eq!(
        result.key.definition().unit,
        QuoteRiskUnit::CurrencyPerPricePoint
    );
    Ok(())
}

#[test]
fn original_cds_bucket_sum_reconciles_to_actual_parallel_quote_rebuild() -> Result<(), Error> {
    let data = credit_support::inputs()?;
    let r = credit_support::recipe(&data)?;
    let calendar = credit_support::calendar(&data);
    let supplied = credit_support::supplied(&data)?;
    let calendars = credit_support::calendars(&r, &calendar);
    let history = FixingSeries::new(vec![])?;
    let inputs = [CdsCurveBuildInput {
        recipe: &r,
        calendars: &calendars,
    }];
    let graph = prepare_quote_risk_graph(
        QuoteRiskGraphInput {
            rates: &[],
            credit: &inputs,
            cash: &[],
            supplied: &supplied,
            history: &history,
            rate_context: None,
            controls: FitNumericalControls::default(),
        },
        |_, _| {
            Err(convex_core::cashflows::CashflowError::BoundExceeded(
                "unexpected",
            ))
        },
    )?;
    let trade = credit_support::trade(&r, &calendar, 5, "0.01")?;
    let ctx = trade
        .terms()
        .trade_context(convex_credit::InitialPaymentState::NotContracted);
    let calculate = |k| {
        curve_quote_sensitivity(
            &graph,
            k,
            position("1", "1", RiskQuantityUnit::TradeCopies).map_err(failure)?,
            snapshot().map_err(failure)?,
            settings(0.0001).map_err(failure)?,
            |s| {
                let v = convex_credit::price_cds(&trade, &ctx, &s.curves).map_err(failure)?;
                risk_reference_pv(v.net_pv, v.currency)
            },
        )
    };
    let mut buckets = Vec::new();
    let mut quotes = Vec::new();
    for h in &r.helpers {
        let k = key(
            QuoteType::CdsSpread,
            RiskCoordinate::Quote {
                curve: r.curve.key.clone(),
                helper_id: h.id.clone(),
                quote_version: h.quote_version,
            },
            h.quote.unit,
            RiskResponse::RebuildDependents,
            &format!("{}D", r.curve.reference_date.days_between(&h.pillar)),
        )?;
        quotes.push((h.id.clone(), h.quote_version, h.quote.unit));
        buckets.push(calculate(k)?);
    }
    quotes.sort_by(|a, b| a.0.cmp(&b.0));
    let k = key(
        QuoteType::CdsSpread,
        RiskCoordinate::ParallelQuotes {
            curve: r.curve.key.clone(),
            quotes,
        },
        QuoteUnit::DecimalRate,
        RiskResponse::RebuildDependents,
        "parallel",
    )?;
    let parallel = calculate(k)?;
    let tolerance = RiskTolerance {
        absolute: 0.10,
        relative: 1e-4,
    };
    let report = reconcile_quote_buckets(&buckets, &parallel, tolerance)?;
    assert!(report.passed, "{report:?}");
    assert!(reconcile_quote_buckets(&buckets[1..], &parallel, tolerance).is_err());
    let mut duplicate = buckets.clone();
    duplicate[1] = duplicate[0].clone();
    assert!(reconcile_quote_buckets(&duplicate, &parallel, tolerance).is_err());
    let mut unstable = parallel.clone();
    unstable.value *= 2.0;
    assert!(reconcile_quote_buckets(&buckets, &unstable, tolerance).is_err());
    Ok(())
}
#[test]
fn global_original_credit_quotes_rebuild_with_unchanged_native_tolerances() -> Result<(), Error> {
    let data = credit_support::inputs()?;
    let r = credit_support::global(&credit_support::recipe(&data)?);
    let calendar = credit_support::calendar(&data);
    let supplied = credit_support::supplied(&data)?;
    let calendars = credit_support::calendars(&r, &calendar);
    let history = FixingSeries::new(vec![])?;
    let inputs = [CdsCurveBuildInput {
        recipe: &r,
        calendars: &calendars,
    }];
    let graph = prepare_quote_risk_graph(
        QuoteRiskGraphInput {
            rates: &[],
            credit: &inputs,
            cash: &[],
            supplied: &supplied,
            history: &history,
            rate_context: None,
            controls: FitNumericalControls::default(),
        },
        |_, _| {
            Err(convex_core::cashflows::CashflowError::BoundExceeded(
                "unexpected",
            ))
        },
    )?;
    let h = &r.helpers[3];
    let k = key(
        QuoteType::CdsSpread,
        RiskCoordinate::Quote {
            curve: r.curve.key.clone(),
            helper_id: h.id.clone(),
            quote_version: h.quote_version,
        },
        h.quote.unit,
        RiskResponse::RebuildDependents,
        "3Y",
    )?;
    for d in [-0.0001, 0.0001] {
        let out = evaluate_quote_graph(&graph, &k, d)?;
        for (q, h) in out.repricing.iter().zip(&r.helpers) {
            assert_eq!(q.tolerance, h.tolerance);
            assert!(q.residual.abs() <= h.tolerance);
        }
    }
    Ok(())
}
#[test]
fn node_zero_and_quote_calibration_risk_remain_distinct_on_same_swap() -> Result<(), Error> {
    let f = rate_support::Fixture::load("nonflat-positive")?;
    let resources = f.resources(&f.recipes);
    let supplied = CurveSet::new();
    let versions = f
        .recipes
        .iter()
        .map(|r| vec![Version::new(1).unwrap(); r.helpers.len()])
        .collect::<Vec<_>>();
    let inputs = f
        .recipes
        .iter()
        .zip(&resources)
        .zip(&versions)
        .map(|((recipe, resources), quote_versions)| RateRiskCurveInput {
            build: RateCurveBuildInput { recipe, resources },
            quote_versions,
        })
        .collect::<Vec<_>>();
    let graph = prepare_quote_risk_graph(
        QuoteRiskGraphInput {
            rates: &inputs,
            credit: &[],
            cash: &[],
            supplied: &supplied,
            history: &f.history,
            rate_context: Some(f.context()?),
            controls: FitNumericalControls::default(),
        },
        |i, d| f.publication(i, d),
    )?;
    let r = &f.recipes[1];
    let h = &r.helpers[2];
    let RateCalibrationTerms::Swap(t) = &h.terms else {
        return Err("full swap".into());
    };
    let swap = t.as_ref().clone().build(&f.swap_calendars(t))?;
    let node = key(
        QuoteType::ZeroRate,
        RiskCoordinate::Zero {
            curve: r.curve.key.clone(),
            node: None,
        },
        QuoteUnit::DecimalRate,
        RiskResponse::FrozenCurves,
        "parallel",
    )?;
    let quote = key(
        QuoteType::SwapParRate,
        RiskCoordinate::Quote {
            curve: r.curve.key.clone(),
            helper_id: h.id.clone(),
            quote_version: Version::new(1)?,
        },
        h.quote.unit,
        RiskResponse::RebuildDependents,
        "1Y",
    )?;
    let calc = |k| {
        curve_quote_sensitivity(
            &graph,
            k,
            position("1", "1", RiskQuantityUnit::TradeCopies).map_err(failure)?,
            snapshot().map_err(failure)?,
            settings(0.0001).map_err(failure)?,
            |s| {
                Ok(convex_rates::pricing::price_swap(
                    &swap,
                    &convex_rates::pricing::SwapValuationInputs {
                        curves: &s.curves,
                        fixings: &f.history,
                    },
                    f.context().map_err(failure)?.cashflows,
                    |i, d| f.publication(i, d),
                )
                .map_err(failure)?
                .present_value)
            },
        )
    };
    let zero = calc(node)?;
    let par = calc(quote)?;
    assert_ne!(zero.key, par.key);
    assert!((zero.value - par.value).abs() > 0.1);
    Ok(())
}
#[test]
fn complete_graph_cycles_and_output_overwrites_fail_before_publications() -> Result<(), Error> {
    let f = cash_support::Fixture::load("positive")?;
    let history = FixingSeries::new(vec![])?;
    let supplied = f.supplied()?;
    let mut recipes = f.recipes.clone();
    let gov = recipes
        .iter_mut()
        .find(|r| matches!(r.curve.role, CurveRole::Government { .. }))
        .ok_or("gov")?;
    gov.curve.dependencies.push(gov.curve.key.clone());
    let resources = f.resources(&recipes);
    let inputs = recipes
        .iter()
        .zip(&resources)
        .map(|(recipe, resources)| CashCurveBuildInput { recipe, resources })
        .collect::<Vec<_>>();
    let mut calls = 0;
    let result = prepare_quote_risk_graph(
        QuoteRiskGraphInput {
            rates: &[],
            credit: &[],
            cash: &inputs,
            supplied: &supplied,
            history: &history,
            rate_context: None,
            controls: FitNumericalControls::default(),
        },
        |_, _| {
            calls += 1;
            Err(convex_core::cashflows::CashflowError::BoundExceeded(
                "unexpected",
            ))
        },
    );
    assert!(result.is_err());
    assert_eq!(calls, 0);
    let resources = f.resources(&f.recipes);
    let inputs = f
        .recipes
        .iter()
        .zip(&resources)
        .map(|(recipe, resources)| CashCurveBuildInput { recipe, resources })
        .collect::<Vec<_>>();
    let full = cash_support::build(&f.recipes, &resources, &supplied)?.curves;
    assert!(prepare_quote_risk_graph(
        QuoteRiskGraphInput {
            rates: &[],
            credit: &[],
            cash: &inputs,
            supplied: &full,
            history: &history,
            rate_context: None,
            controls: FitNumericalControls::default()
        },
        |_, _| Err(convex_core::cashflows::CashflowError::BoundExceeded(
            "unexpected"
        ))
    )
    .is_err());
    Ok(())
}
#[test]
fn raw_optional_oas_risk_keeps_model_parameters_and_has_separate_z_spread_identity(
) -> Result<(), Error> {
    use convex_analytics::optional_bond::*;
    use convex_bonds::contracts::*;
    use convex_core::{calendars::WeekendCalendar, cashflows::*};
    let settle = convex_core::Date::parse("2025-01-01")?;
    let mut terms = convex_bonds::instruments::FixedRateBond::builder()
        .cusip_unchecked("SUPPLIED-CVX024-OPTION")
        .issue_date(settle)
        .maturity(convex_core::Date::parse("2028-01-01")?)
        .coupon_rate(rust_decimal::Decimal::new(6, 2))
        .frequency(convex_core::types::Frequency::Annual)
        .day_count(convex_core::daycounts::DayCountConvention::Act365Fixed)
        .calendar(convex_bonds::types::CalendarId::weekend_only())
        .business_day_convention(convex_core::calendars::BusinessDayConvention::Unadjusted)
        .face_value(rust_decimal::Decimal::ONE_HUNDRED)
        .redemption_value(rust_decimal::Decimal::ONE_HUNDRED)
        .end_of_month(false)
        .build()?
        .contract_terms()?;
    terms.exercise = vec![ExerciseTerms {
        right: ExerciseRight::Call,
        style: ExerciseStyle::Bermudan {
            dates: vec![
                convex_core::Date::parse("2026-01-01")?,
                convex_core::Date::parse("2027-01-01")?,
            ],
        },
        price_per_100: rust_decimal::Decimal::from(101),
        notice_offset: DateOffset::CalendarDays(0),
        settlement_lag: DateOffset::CalendarDays(0),
        settlement_adjustment: convex_core::calendars::BusinessDayConvention::Unadjusted,
        accrued: ExerciseAccrued::PayAccrued,
        protection_end: None,
    }];
    let bond = terms.build()?;
    let discount = futures_support::curve(
        CurveRole::Discount {
            collateral_currency: Currency::USD,
        },
        0.04,
        settle,
    );
    let model = OptionalBondModel::assumed(
        CurveKey {
            id: "HW1F.CVX024".into(),
            version: "explicit-v1".into(),
        },
        Hw1fParameters {
            mean_reversion: 0.03,
            volatility: 0.01,
        },
        "explicit synthetic original assumption".into(),
    )?;
    let prepared = prepare_fixed(
        &bond,
        OptionalBondContext {
            settlement: settle,
            on_payment: PaymentDateEntitlement::Exclude,
            exercise_state: ExerciseState::AliveUnexercised,
        },
        OptionalBondInputs {
            discount: &discount,
            projection: None,
            model: &model,
            exercise_calendar: &WeekendCalendar,
        },
        OptionalBondSettings {
            grid: GridSettings {
                base_steps: 128,
                refinements: 3,
                absolute_price_tolerance: 0.004,
                relative_price_tolerance: 1e-8,
            },
            exercise_priority: ExercisePriority::IssuerLast,
        },
    )?;
    let k = key(
        QuoteType::OasSpread,
        RiskCoordinate::Raw {
            input_id: "OPTIONAL.CVX024".into(),
            version: Version::new(1)?,
        },
        QuoteUnit::DecimalRate,
        RiskResponse::FrozenCurves,
        "spot",
    )?;
    let native_settings = RiskSettings {
        absolute_duration_tolerance: 0.02,
        relative_duration_tolerance: 0.01,
        absolute_convexity_tolerance: 0.25,
        relative_convexity_tolerance: 0.01,
    };
    assert!(optional_oas_quote_sensitivity(
        &prepared,
        k.clone(),
        position("1", "1", RiskQuantityUnit::TradeCopies)?,
        snapshot()?,
        settings(0.0001)?,
        0.00123456,
        native_settings
    )
    .is_err());
    let result = optional_oas_quote_sensitivity(
        &prepared,
        k.clone(),
        position("1", "1", RiskQuantityUnit::TradeCopies)?,
        snapshot()?,
        settings(0.01)?,
        0.00123456,
        native_settings,
    )?;
    assert!(result.sensitivity.value < 0.0);
    assert_eq!(result.native.base.model.parameters(), model.parameters());
    assert_eq!(result.half.base.model.parameters(), model.parameters());
    let z = key(
        QuoteType::ZSpread,
        RiskCoordinate::Raw {
            input_id: "OPTIONAL.CVX024".into(),
            version: Version::new(1)?,
        },
        QuoteUnit::DecimalRate,
        RiskResponse::FrozenCurves,
        "spot",
    )?;
    assert_ne!(k, z);
    Ok(())
}

#[test]
fn actual_grid_bucket_completeness_is_checked_even_when_a_missing_node_has_zero_risk(
) -> Result<(), Error> {
    let pack = futures_support::BondPack::new();
    let supplied = CurveSet::new().with_curve(pack.discount.clone())?;
    let history = FixingSeries::new(vec![])?;
    let graph = prepare_quote_risk_graph(
        QuoteRiskGraphInput {
            rates: &[],
            credit: &[],
            cash: &[],
            supplied: &supplied,
            history: &history,
            rate_context: None,
            controls: FitNumericalControls::default(),
        },
        |_, _| {
            Err(convex_core::cashflows::CashflowError::BoundExceeded(
                "unexpected",
            ))
        },
    )?;
    let curve = pack.discount.recipe().key.clone();
    let calc = |node, tenor| -> Result<QuoteSensitivity, Error> {
        let k = key(
            QuoteType::ZeroRate,
            RiskCoordinate::Zero {
                curve: curve.clone(),
                node,
            },
            QuoteUnit::DecimalRate,
            RiskResponse::FrozenCurves,
            tenor,
        )?;
        Ok(curve_quote_sensitivity(
            &graph,
            k,
            position("1", "1", RiskQuantityUnit::TradeCopies)?,
            snapshot()?,
            settings(0.0001)?,
            |s| {
                let p = convex_analytics::bond_contract::price_fixed(
                    &pack.bonds[0],
                    convex_core::Date::parse("2026-10-08").map_err(failure)?,
                    convex_core::cashflows::PaymentDateEntitlement::Exclude,
                    s.curves.get(&curve)?.as_ref(),
                )
                .map_err(failure)?;
                risk_reference_pv(p.present_value, Currency::USD)
            },
        )?)
    };
    let n = pack.discount.interpolation_grid()?.0.len();
    let buckets = (0..n)
        .map(|i| calc(Some(i), "spot"))
        .collect::<Result<Vec<_>, _>>()?;
    let parallel = calc(None, "parallel")?;
    let tolerance = RiskTolerance {
        absolute: 0.10,
        relative: 1e-4,
    };
    assert!(reconcile_curve_node_buckets(&graph, &buckets, &parallel, tolerance)?.passed);
    assert!(reconcile_curve_node_buckets(&graph, &buckets[1..], &parallel, tolerance).is_err());
    assert!(reconcile_quote_buckets(&buckets, &parallel, tolerance).is_err());
    Ok(())
}
#[test]
fn repo_quote_and_residual_basis_risk_price_actual_dated_carry_and_cash() -> Result<(), Error> {
    let f = cash_support::Fixture::load("positive")?;
    let resources = f.resources(&f.recipes);
    let supplied = f.supplied()?;
    let history = FixingSeries::new(vec![])?;
    let inputs = f
        .recipes
        .iter()
        .zip(&resources)
        .map(|(recipe, resources)| CashCurveBuildInput { recipe, resources })
        .collect::<Vec<_>>();
    let graph = prepare_quote_risk_graph(
        QuoteRiskGraphInput {
            rates: &[],
            credit: &[],
            cash: &inputs,
            supplied: &supplied,
            history: &history,
            rate_context: None,
            controls: FitNumericalControls::default(),
        },
        |_, _| {
            Err(convex_core::cashflows::CashflowError::BoundExceeded(
                "unexpected",
            ))
        },
    )?;
    let repo = f
        .recipes
        .iter()
        .find(|r| matches!(r.curve.role, CurveRole::Repo { .. }))
        .ok_or("repo")?;
    let CashCalibrationHelper::Repo { input: h } = &repo.helpers[1] else {
        return Err("loan".into());
    };
    let k = key(
        QuoteType::RepoRate,
        RiskCoordinate::Quote {
            curve: repo.curve.key.clone(),
            helper_id: h.id.clone(),
            quote_version: h.quote_version,
        },
        h.quote.unit,
        RiskResponse::RebuildDependents,
        "1M",
    )?;
    let terms = &f.forwards[0];
    let carry_resources = f.forward_resources(terms);
    let face: f64 = terms.spot.terms.face_value.to_string().parse()?;
    let repo = curve_quote_sensitivity(
        &graph,
        k,
        position("1", "1", RiskQuantityUnit::TradeCopies)?,
        snapshot()?,
        settings(0.0001)?,
        |s| {
            let p = price_repo_bond_forward(&RepoBondForwardInputs {
                terms,
                resources: &carry_resources,
                delivery_calendar: f.delivery_calendar(),
                curves: &s.curves,
            })
            .map_err(failure)?;
            risk_reference_pv(
                p.dirty_forward_per100 * face / 100.,
                terms.spot.terms.currency,
            )
        },
    )?;
    assert!(repo.value.abs() > 0.001);
    let r = f
        .recipes
        .iter()
        .find(|r| r.basis_model.is_some())
        .ok_or("basis")?;
    let CashCalibrationHelper::Bond { input: h } = &r.helpers[0] else {
        return Err("bond".into());
    };
    let resource = cash_support::bond_resources(h, &f.calendar);
    let k = key(
        QuoteType::ResidualBasis,
        RiskCoordinate::Basis {
            curve: r.curve.key.clone(),
            node: None,
        },
        QuoteUnit::DecimalRate,
        RiskResponse::FrozenCurves,
        "parallel",
    )?;
    let basis = curve_quote_sensitivity(
        &graph,
        k,
        position("1", "1", RiskQuantityUnit::TradeCopies)?,
        snapshot()?,
        settings(0.0001)?,
        |s| {
            let p = price_basis_bond(
                h,
                &resource,
                r.basis_model
                    .as_ref()
                    .ok_or_else(|| failure("basis model"))?,
                s.curves.get(&r.curve.key)?.as_ref(),
                &s.curves,
            )
            .map_err(failure)?;
            risk_reference_pv(p.price.present_value, h.terms.currency)
        },
    )?;
    assert!(basis.value < 0.0);
    assert_ne!(repo.key, basis.key);
    Ok(())
}
