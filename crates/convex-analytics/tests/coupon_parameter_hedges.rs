//! Independent full-contract native/canonical parameter risk and complete hedges.
#[path = "../../../tests/fixtures/packed.rs"]
mod packed;
use convex_analytics::{floating_coupon::*, quote_risk::*, workflow::*};
use convex_bonds::contracts::*;
use convex_core::{contracts::*, fixings::FixingSeries, Currency};
use convex_curves::contracts::*;
use serde_json::Value;
use std::{cell::Cell, collections::HashSet};

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/coupon-parameter-hedges.json")).unwrap()
}
fn number(value: &Value) -> f64 {
    value.as_str().unwrap().parse().unwrap()
}
fn with_prepared(
    row: &Value,
    f: impl FnOnce(&PreparedTermCouponModel<'_>, &CurveSet, &FixingSeries),
) {
    let input = &row["input"];
    let terms: BondTerms = serde_json::from_value(input["bond"]["terms"].clone()).unwrap();
    let bond = terms.build().unwrap();
    let context: BondProjectionContext = serde_json::from_value(input["context"].clone()).unwrap();
    let options: TermCouponOptionInputs = serde_json::from_value(input["options"].clone()).unwrap();
    let history =
        FixingSeries::new(serde_json::from_value(row["resources"]["fixings"].clone()).unwrap())
            .unwrap();
    let recipes: Vec<CurveRecipe> =
        serde_json::from_value(row["resources"]["curves"].clone()).unwrap();
    let curves = build_curve_set(&recipes, &CurveSet::new()).unwrap().curves;
    let calls = Cell::new(0);
    let prepared = prepare_term_coupon_model(
        TermCouponModelInputs {
            bond: &bond,
            context: &context,
            history: &history,
            options: &options,
            projection: serde_json::from_value(input["projection"].clone()).unwrap(),
            discount: serde_json::from_value(input["discount"].clone()).unwrap(),
        },
        &curves,
        |_, day| {
            calls.set(calls.get() + 1);
            Ok(UtcTimestamp::parse(&format!("{day}T16:00:00Z")).unwrap())
        },
    )
    .unwrap();
    assert_eq!(calls.get(), 3);
    let original = serde_json::to_value(prepared.baseline()).unwrap();
    f(&prepared, &curves, &history);
    assert_eq!(calls.get(), 3);
    assert_eq!(
        serde_json::to_value(prepared.price(&curves).unwrap()).unwrap(),
        original
    );
}

#[test]
fn full_native_parameter_risk_preserves_all_units_signed_face_and_refinement() {
    let f = fixture();
    let mut keys = HashSet::new();
    for row in f["cases"].as_array().unwrap() {
        with_prepared(row, |prepared, curves, _| {
            let key: QuoteRiskKey = serde_json::from_value(row["key"].clone()).unwrap();
            let point: CouponParameterCoordinate =
                serde_json::from_value(row["coordinate"].clone()).unwrap();
            assert_eq!(coupon_parameter_coordinate(&key).unwrap(), point);
            assert_eq!(
                coupon_parameter_key(key.definition().factor.clone(), point).unwrap(),
                key
            );
            keys.insert(key.clone());
            let position: RiskPosition = serde_json::from_value(row["position"].clone()).unwrap();
            let snapshot: SnapshotId = serde_json::from_value(row["snapshot_id"].clone()).unwrap();
            let settings: QuoteRiskSettings =
                serde_json::from_value(row["risk_settings"].clone()).unwrap();
            let native: CouponParameterDerivativeSettings =
                serde_json::from_value(row["settings"].clone()).unwrap();
            let result = term_coupon_parameter_sensitivity(
                prepared,
                curves,
                key.clone(),
                position.clone(),
                snapshot.clone(),
                settings.clone(),
                native,
            )
            .unwrap();
            result.sensitivity.validate().unwrap();
            let value = serde_json::to_value(&result).unwrap();
            for (field, reference) in [
                ("base_pv", "base"),
                ("plus_pv", "plus"),
                ("minus_pv", "minus"),
                ("half_plus_pv", "half_plus"),
                ("half_minus_pv", "half_minus"),
                ("value", "first"),
                ("half_value", "half_first"),
            ] {
                let bound = if field == "value" || field == "half_value" {
                    number(&row["absolute_scaled_first_bound"])
                } else {
                    number(&row["absolute_scaled_pv_bound"])
                };
                assert!(
                    (value["sensitivity"][field].as_f64().unwrap()
                        - number(&row["expected"][reference]))
                    .abs()
                        <= bound,
                    "{} {field}",
                    row["id"]
                );
            }
            for field in ["base", "plus", "minus", "half_plus", "half_minus"] {
                assert!(
                    (value["native"][field]["present_value"].as_f64().unwrap()
                        - number(&row["reference_expected"][field]))
                    .abs()
                        <= f["inherited_reference_policy"]["absolute_pv_bound"]
                            .as_f64()
                            .unwrap()
                );
                assert_eq!(
                    value["native"][field]["terms"],
                    row["input"]["bond"]["terms"]
                );
                assert_eq!(value["native"][field]["context"], row["input"]["context"]);
            }
            for field in ["first", "half_first", "second", "half_second"] {
                let bound = if field.contains("second") {
                    "absolute_second_bound"
                } else {
                    "absolute_first_bound"
                };
                assert!(
                    (value["native"][field].as_f64().unwrap()
                        - number(&row["reference_expected"][field]))
                    .abs()
                        <= f["inherited_reference_policy"][bound].as_f64().unwrap()
                );
            }
            assert_eq!(result.sensitivity.key, key);
            assert_eq!(result.sensitivity.position, position);
            assert_eq!(result.sensitivity.snapshot_id, snapshot);
            let calls = Cell::new(0);
            assert!(reference_quote_sensitivity(
                key.clone(),
                position.clone(),
                snapshot.clone(),
                settings.clone(),
                |_| {
                    calls.set(calls.get() + 1);
                    risk_reference_pv(0.0, Currency::USD)
                }
            )
            .is_err());
            assert_eq!(calls.get(), 0);
            let wrong = CouponParameterDerivativeSettings {
                bump: native.bump / 2.0,
                ..native
            };
            assert!(term_coupon_parameter_sensitivity(
                prepared,
                curves,
                key.clone(),
                position.clone(),
                snapshot.clone(),
                settings.clone(),
                wrong
            )
            .is_err());
            assert!(validate_quote_graph_moves(&[QuoteGraphMove {
                key: key.clone(),
                delta: 0.001
            }])
            .is_err());
            for unit in [
                QuoteUnit::CouponNormalVolatility,
                QuoteUnit::CouponLognormalVolatility,
                QuoteUnit::CouponLognormalDisplacement,
                QuoteUnit::CouponNormalFactorLoading,
            ] {
                assert!(MarketQuote { value: 0.01, unit }.validate().is_err());
            }
        });
    }
    assert_eq!(keys.len(), 5);
}

#[test]
fn complete_coupon_columns_aggregate_current_pending_and_solve_original_lots() {
    let f = fixture();
    let row = &f["cases"][12];
    let corpus: Value = serde_json::from_str(packed::text(
        "tests/fixtures/library/coupon-parameter-hedges.json",
    ))
    .unwrap();
    let request = corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "coupon_parameter_risk.complete_hedge")
        .unwrap();
    let operation = &request["request"]["operation"];
    with_prepared(row, |prepared, curves, history| {
        let input: GraphHedgeRiskInput = serde_json::from_value(operation["risk"].clone()).unwrap();
        let policies: Vec<CouponParameterHedgePolicy> =
            serde_json::from_value(operation["parameter_policies"].clone()).unwrap();
        let graph = prepare_quote_risk_graph(
            QuoteRiskGraphInput {
                rates: &[],
                credit: &[],
                cash: &[],
                supplied: curves,
                history,
                rate_context: None,
                controls: serde_json::from_value(operation["graph"]["controls"].clone()).unwrap(),
            },
            |_, _| panic!("no calibration fixing"),
        )
        .unwrap();
        let result = term_coupon_complete_hedge_risk(prepared, &graph, &input, &policies).unwrap();
        assert_eq!(result.reserved_factor_evaluations, 25);
        assert_eq!(result.factors.len(), 5);
        assert_eq!(result.vector.sensitivities.len(), 5);
        for (i, reference) in f["complete_hedge"]["factors"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            let s = &result.vector.sensitivities[i];
            s.validate().unwrap();
            assert_eq!(*s, *result.factors[i].sensitivity());
            assert!(
                (s.value - number(&reference["expected_value"])).abs()
                    <= number(&reference["absolute_bound"])
            );
            assert!(
                (s.half_value - number(&reference["expected_half_value"])).abs()
                    <= number(&reference["absolute_bound"])
            );
        }
        use convex_analytics::risk::hedging::constrained as h;
        let pin = |id| convex_credit::ReferencePin {
            id: ReferenceDataId::new(id).unwrap(),
            version: Version::new(1).unwrap(),
        };
        let exact = |n| ExactDecimal::parse(n).unwrap();
        let problem = h::HedgeProblem {
            snapshot_id: input.snapshot_id.clone(),
            entity: input.identity.entity.clone(),
            settlement: prepared.settlement(),
            reporting_currency: Currency::USD,
            position_state: pin("supplied.current.pending"),
            expected_holdings: vec![result.vector.id.clone()],
            holdings: vec![h::HedgeHolding {
                risk: result.vector.clone(),
                current_quantity: exact("10000"),
                pending_quantity: exact("-2500"),
            }],
            factors: input
                .factors
                .iter()
                .map(|factor| h::HedgeFactor {
                    key: factor.key.clone(),
                    risk_settings: factor.settings.clone(),
                    credit: None,
                    scale: 1.,
                    weight: 1.,
                    target: 0.,
                    lower: -1e8,
                    upper: 1e8,
                })
                .collect(),
            candidates: vec![h::HedgeCandidate {
                risk: result.vector.clone(),
                eligibility: h::HedgeEligibility {
                    eligible: true,
                    decision: pin("supplied.eligibility"),
                    reason: "Independent fictional proposal control".into(),
                },
                trade_lower: exact("-10000"),
                trade_upper: exact("10000"),
                holding_lower: exact("-10000"),
                holding_upper: exact("20000"),
                buy_available: exact("10000"),
                sell_available: exact("10000"),
                lot: Some(exact("500")),
                costs: h::HedgeCosts {
                    currency: Currency::USD,
                    buy_per_reference: exact("0"),
                    sell_per_reference: exact("0"),
                    observation: pin("supplied.zero.cost"),
                },
            }],
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
        for (i, reference) in f["complete_hedge"]["factors"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            assert!(
                (exposure.before[i] - number(&reference["expected_current_pending_value"])).abs()
                    <= number(&reference["absolute_aggregate_bound"])
            );
            let value = number(&reference["expected_value"]);
            assert!(
                (exposure.current[i] - 2. * value).abs()
                    <= 2. * number(&reference["absolute_bound"])
            );
            assert!(
                (exposure.pending[i] + 0.5 * value).abs()
                    <= 0.5 * number(&reference["absolute_bound"])
            );
        }
        let report = h::solve_hedge_proposal(&problem).unwrap();
        assert_eq!(report.status, h::HedgeSolveStatus::Feasible);
        let proposal = report.proposal.unwrap();
        assert_eq!(proposal.quantities, vec![exact("-7500")]);
    });
}
