//! Independent full coupon model PV and graph h/h2 sensitivities.
#[path = "../../convex/examples/support/quote_fixture.rs"]
mod support;
use convex_analytics::{floating_coupon::*, quote_risk::*, workflow::*};
use convex_bonds::contracts::*;
use convex_core::{
    contracts::*, daycounts::DayCountConvention, fixings::FixingSeries, types::Compounding,
    Currency, Date,
};
use convex_curves::{contracts::*, ExtrapolationMethod, InterpolationMethod};
use serde_json::Value;
use std::cell::Cell;
fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/coupon-graph-risk.json")).unwrap()
}
fn build_curves(reference: Date) -> CurveSet {
    let recipes = [
        (
            "discount",
            CurveRole::Discount {
                collateral_currency: Currency::USD,
            },
            0.03,
        ),
        (
            "projection",
            CurveRole::Projection {
                index_id: "SUPPLIED.TERM".into(),
            },
            0.04,
        ),
        (
            "unused",
            CurveRole::Discount {
                collateral_currency: Currency::USD,
            },
            0.07,
        ),
    ]
    .into_iter()
    .map(|(id, role, rate)| CurveRecipe {
        key: CurveKey {
            id: id.into(),
            version: "v1".into(),
        },
        role,
        currency: Currency::USD,
        reference_date: reference,
        time_basis: DayCountConvention::Act365Fixed,
        representation: CurveRepresentation::ZeroRates {
            compounding: Compounding::Continuous,
        },
        interpolation: InterpolationMethod::Linear,
        extrapolation: ExtrapolationMethod::Flat,
        dependencies: vec![],
        calibration: CalibrationMode::Direct,
        source: CurveSource::Nodes(
            [0.01, 30.]
                .into_iter()
                .map(|t| CurveNode {
                    pillar: CurvePoint::Tenor(t),
                    quote: MarketQuote {
                        value: rate,
                        unit: QuoteUnit::DecimalRate,
                    },
                })
                .collect(),
        ),
    })
    .collect::<Vec<_>>();
    build_curve_set(&recipes, &CurveSet::new()).unwrap().curves
}
fn key(axis: &str) -> QuoteRiskKey {
    let curve = CurveKey {
        id: axis.into(),
        version: "v1".into(),
    };
    if axis != "joint" {
        return support::key(
            QuoteType::ZeroRate,
            RiskCoordinate::Zero { curve, node: None },
            QuoteUnit::DecimalRate,
            RiskResponse::FrozenCurves,
            "parallel",
        )
        .unwrap();
    }
    QuoteRiskKey::new(QuoteRiskDefinition {
        factor: RiskFactorKey::new(FactorDefinition {
            family: FactorFamily::InterestRate,
            target: FactorTarget::CurvePair {
                discount_curve_id: CurveId::new("discount").unwrap(),
                discount_curve_version: "v1".into(),
                projection_curve_id: CurveId::new("projection").unwrap(),
                projection_curve_version: "v1".into(),
            },
            currency: Currency::USD,
            tenor: FactorTenor::new("parallel").unwrap(),
            quote_type: QuoteType::JointZeroRate,
            basis: RiskBasis::ZeroNode,
            convention_version: Version::new(1).unwrap(),
            model_version: ModelId::new("supplied.normal.full.coupon").unwrap(),
        })
        .unwrap(),
        coordinate: RiskCoordinate::Raw {
            input_id: "joint_zero".into(),
            version: Version::new(1).unwrap(),
        },
        native_unit: QuoteUnit::DecimalRate,
        response: RiskResponse::FrozenCurves,
        unit: QuoteRiskUnit::CurrencyPerBp,
    })
    .unwrap()
}
#[test]
fn full_coupon_graph_prices_and_signed_h_half_h_risk_match_independent_dates_and_law() {
    let f = fixture();
    let row = &f["context"];
    let bond: BondTerms = serde_json::from_value(row["terms"].clone()).unwrap();
    let bond = bond.build().unwrap();
    let context: BondProjectionContext = serde_json::from_value(row["context"].clone()).unwrap();
    let history =
        FixingSeries::new(serde_json::from_value(row["history"].clone()).unwrap()).unwrap();
    let options: TermCouponOptionInputs = serde_json::from_value(row["options"].clone()).unwrap();
    let curves = build_curves(context.settlement);
    let calls = Cell::new(0);
    let prepared = prepare_term_coupon_model(
        TermCouponModelInputs {
            bond: &bond,
            context: &context,
            history: &history,
            options: &options,
            projection: Some(CurveKey {
                id: "projection".into(),
                version: "v1".into(),
            }),
            discount: CurveKey {
                id: "discount".into(),
                version: "v1".into(),
            },
        },
        &curves,
        |_, date| {
            calls.set(calls.get() + 1);
            Ok(UtcTimestamp::parse(&format!("{date}T16:00:00Z")).unwrap())
        },
    )
    .unwrap();
    let original_calls = calls.get();
    assert_eq!(original_calls, 3);
    let graph = prepare_quote_risk_graph(
        QuoteRiskGraphInput {
            rates: &[],
            credit: &[],
            cash: &[],
            supplied: &curves,
            history: &history,
            rate_context: None,
            controls: FitNumericalControls::default(),
        },
        |_, _| panic!("no calibration fixing"),
    )
    .unwrap();
    for r in f["cases"].as_array().unwrap() {
        let result = term_coupon_graph_sensitivity(
            &prepared,
            &graph,
            key(r["axis"].as_str().unwrap()),
            support::position("5000", "5000", RiskQuantityUnit::Notional).unwrap(),
            support::snapshot().unwrap(),
            QuoteRiskSettings {
                bump: 0.0001,
                stability: RiskTolerance {
                    absolute: 1e-6,
                    relative: 0.,
                },
                policy_version: Version::new(1).unwrap(),
            },
        )
        .unwrap();
        let v = &result.sensitivity;
        for (name, actual) in [
            ("base", v.base_pv),
            ("plus", v.plus_pv),
            ("minus", v.minus_pv),
            ("half_plus", v.half_plus_pv),
            ("half_minus", v.half_minus_pv),
            ("value", v.value),
            ("half_value", v.half_value),
        ] {
            assert!(
                (actual - r[name].as_f64().unwrap()).abs() <= 1e-8,
                "{} {name}: {actual}",
                r["axis"]
            );
        }
        assert_eq!(result.valuations.len(), 5);
        assert_eq!(calls.get(), original_calls);
        for observation in &result.valuations {
            assert_eq!(
                serde_json::to_value(&observation.valuation.terms).unwrap(),
                row["terms"]
            );
            assert_eq!(
                serde_json::to_value(&observation.valuation.segments[0].rate).unwrap(),
                serde_json::to_value(&prepared.baseline().segments[0].rate).unwrap()
            );
        }
    }
    let factors = f["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| GraphHedgeFactor {
            key: key(r["axis"].as_str().unwrap()),
            settings: QuoteRiskSettings {
                bump: 0.0001,
                stability: RiskTolerance {
                    absolute: 1e-6,
                    relative: 0.,
                },
                policy_version: Version::new(1).unwrap(),
            },
        })
        .collect();
    let pin = |id: &str| convex_credit::ReferencePin {
        id: ReferenceDataId::new(id).unwrap(),
        version: Version::new(1).unwrap(),
    };
    let input = GraphHedgeRiskInput {
        identity: GraphHedgeIdentity {
            id: TradeId::new("actual.coupon.position").unwrap(),
            instrument: pin("original.coupon.security"),
            economics: pin("full.coupon.terms.history"),
            calculation: pin("joint.normal.model.sources"),
            entity: EntityId::new("DESK.A").unwrap(),
        },
        reference_position: support::position("5000", "5000", RiskQuantityUnit::Notional).unwrap(),
        snapshot_id: support::snapshot().unwrap(),
        factors,
        max_factor_evaluations: 15,
    };
    let risk = term_coupon_hedge_risk(&prepared, &graph, &input).unwrap();
    assert_eq!(risk.reserved_factor_evaluations, 15);
    assert_eq!(risk.vector.sensitivities.len(), 3);
    assert_eq!(risk.coupon_factors.len(), 3);
    assert_eq!(risk.vector.reference_position, input.reference_position);
    assert_eq!(calls.get(), original_calls);
    for (actual, expected) in risk
        .vector
        .sensitivities
        .iter()
        .zip(f["cases"].as_array().unwrap())
    {
        assert!((actual.value - expected["value"].as_f64().unwrap()).abs() <= 1e-8);
        actual.validate().unwrap();
    }
    for (quantity, scale) in [("10000", 2.0), ("-15000", -3.0), ("0", 0.0)] {
        let scaled = term_coupon_graph_sensitivity(
            &prepared,
            &graph,
            key("projection"),
            support::position(quantity, "5000", RiskQuantityUnit::Notional).unwrap(),
            support::snapshot().unwrap(),
            input.factors[1].settings.clone(),
        )
        .unwrap();
        assert!(
            (scaled.sensitivity.value - f["cases"][1]["value"].as_f64().unwrap() * scale).abs()
                <= 1e-8
        );
    }
    let zero = term_coupon_graph_sensitivity(
        &prepared,
        &graph,
        key("unused"),
        input.reference_position.clone(),
        input.snapshot_id.clone(),
        input.factors[0].settings.clone(),
    )
    .unwrap();
    assert_eq!(zero.sensitivity.value, 0.0);
    assert_eq!(zero.sensitivity.half_value, 0.0);
    assert_eq!(zero.valuations.len(), 5);
    assert!(prepared
        .admit_baseline(&build_curves(Date::parse("2024-01-16").unwrap()))
        .is_err());
    for mutation in 0..4 {
        let mut changed = input.clone();
        match mutation {
            0 => changed.max_factor_evaluations = 14,
            1 => changed.factors.push(changed.factors[0].clone()),
            2 => {
                changed.reference_position.reference_quantity = ExactDecimal::parse("100").unwrap()
            }
            _ => {
                changed.factors[1].settings.stability.absolute = 1e-12;
                changed.factors[1].settings.stability.relative = 0.0;
            }
        }
        assert!(term_coupon_hedge_risk(&prepared, &graph, &changed).is_err());
    }
    assert_eq!(calls.get(), original_calls);
    use convex_analytics::risk::hedging::constrained as h;
    let problem = h::HedgeProblem {
        snapshot_id: input.snapshot_id.clone(),
        entity: input.identity.entity.clone(),
        settlement: context.settlement,
        reporting_currency: Currency::USD,
        position_state: pin("actual.current.pending.state"),
        expected_holdings: vec![risk.vector.id.clone()],
        holdings: vec![h::HedgeHolding {
            risk: risk.vector.clone(),
            current_quantity: ExactDecimal::parse("10000").unwrap(),
            pending_quantity: ExactDecimal::parse("-2500").unwrap(),
        }],
        factors: input
            .factors
            .iter()
            .map(|f| h::HedgeFactor {
                key: f.key.clone(),
                risk_settings: f.settings.clone(),
                credit: None,
                scale: 1.,
                weight: 1.,
                target: 0.,
                lower: -1e8,
                upper: 1e8,
            })
            .collect(),
        candidates: vec![],
        max_cost: 0.,
        settings: h::HedgeSolveSettings {
            policy_version: Version::new(1).unwrap(),
            rank_relative_tolerance: 1e-8,
            max_condition: 1000.,
            solver_tolerance: 1e-8,
            factor_tolerance: 1e-8,
            cost_tolerance: 1e-8,
            cost_scale: 1.,
            cost_weight: 0.,
            max_work: 1000,
            svd_iterations: 1000,
        },
    };
    let exposure = h::aggregate_hedge_risk(&problem).unwrap();
    for (i, expected) in f["cases"].as_array().unwrap().iter().enumerate() {
        let reference = expected["value"].as_f64().unwrap();
        assert!((exposure.current[i] - 2. * reference).abs() <= 1e-8);
        assert!((exposure.pending[i] + 0.5 * reference).abs() <= 1e-8);
        assert!((exposure.before[i] - 1.5 * reference).abs() <= 1e-8);
    }
}
