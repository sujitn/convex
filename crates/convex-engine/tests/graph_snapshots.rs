#[path = "../../convex-ports/tests/support/snapshot_fixtures.rs"]
mod fixture;

use convex_core::{contracts::*, Currency};
use convex_engine::{
    context::*, snapshot::*, CalculationGraph, CommitOutcome, GraphError, NodeId, NodePhase,
    NodeSpec, NodeValue,
};
use convex_ports::snapshot::MarketSnapshot;
use std::sync::Arc;

fn snapshot(generation: u64) -> Arc<MarketSnapshot> {
    let mut content = fixture::content();
    content.generation = fixture::version(generation);
    Arc::new(MarketSnapshot::new(content).unwrap())
}
fn bond() -> NodeId {
    NodeId::BondPrice {
        instrument_id: "BOND-001".into(),
    }
}
fn definition() -> Vec<NodeSpec> {
    vec![
        NodeSpec::calculation(bond(), vec![]),
        NodeSpec::calculation(
            NodeId::Portfolio {
                portfolio_id: convex_core::ids::PortfolioId::new("BOOK"),
            },
            vec![bond()],
        ),
    ]
}
fn context(snapshot: &MarketSnapshot, measures: &[&str]) -> ValuationContext {
    ValuationContext::new(ValuationContextSpec {
        snapshot_id: snapshot.id().clone(),
        generation: snapshot.content().generation,
        subject: ValuationSubject::InstrumentVersion {
            instrument_id: InstrumentId::new("BOND-001").unwrap(),
            version: fixture::version(1),
        },
        valuation_time: snapshot.content().as_of,
        known_at: snapshot.content().known_at,
        settlement_date: BusinessDate::parse("2026-10-06").unwrap(),
        configuration: ConfigurationPin {
            id: SourceId::new(&snapshot.content().configuration.config_id).unwrap(),
            version: fixture::version(1),
        },
        model_id: ModelId::new(BOND_SNAPSHOT_MODEL).unwrap(),
        build_id: ModelId::new("graph-fixture-build").unwrap(),
        reporting_currency: Currency::USD,
        fixing_known_at: snapshot.content().known_at,
        scenario: ValuationScenario::Base,
        numerical_settings: NumericalSettings {
            policy_id: ModelId::new(BOND_NUMERICAL_POLICY).unwrap(),
            bump_bps: FiniteF64::new(10.0).unwrap(),
        },
        measures: measures
            .iter()
            .map(|m| MeasureId::new(*m).unwrap())
            .collect(),
    })
    .unwrap()
}
fn calculate(
    snapshot: &MarketSnapshot,
    context: &ValuationContext,
) -> Arc<SnapshotValuationResult> {
    Arc::new(
        SnapshotValuator::new(ModelId::new("graph-fixture-build").unwrap(), 16)
            .unwrap()
            .value(
                snapshot,
                context,
                RequestId::new("graph-snapshot-request").unwrap(),
                fixture::time("2026-10-02T15:00:00.100Z"),
            )
            .unwrap(),
    )
}

#[test]
fn actual_snapshot_valuation_commits_only_its_captured_generation_and_complete_context() {
    let graph = CalculationGraph::new();
    let seven = snapshot(7);
    let eight = snapshot(8);
    graph
        .activate_snapshot(definition(), seven.clone())
        .unwrap();
    assert!(matches!(
        graph.begin_calculation(&bond()),
        Err(GraphError::ContextRequired)
    ));
    let ctx7 = context(&seven, &["clean_price_mid", "ytm_mid"]);
    let old = graph.begin_valuation(&bond(), ctx7.clone()).unwrap();
    let old_result = calculate(&seven, &ctx7);
    graph
        .activate_snapshot(definition(), eight.clone())
        .unwrap();
    assert_eq!(
        graph
            .commit_result(&old, Ok(NodeValue::Valuation(old_result)))
            .unwrap(),
        CommitOutcome::Discarded
    );
    assert!(graph.is_dirty(&bond()));
    assert!(graph.get_cached(&bond()).is_none());
    assert!(matches!(
        graph.begin_valuation(&bond(), ctx7),
        Err(GraphError::ContextConflict)
    ));
    let ctx8 = context(&eight, &["clean_price_mid", "ytm_mid"]);
    let ticket = graph.begin_valuation(&bond(), ctx8.clone()).unwrap();
    let mut wrong = ctx8.spec().clone();
    wrong.settlement_date = BusinessDate::parse("2026-10-07").unwrap();
    let wrong = ValuationContext::new(wrong).unwrap();
    let bad = calculate(&eight, &wrong);
    assert_eq!(
        graph
            .commit_result(&ticket, Ok(NodeValue::Valuation(bad)))
            .unwrap_err(),
        GraphError::ContextConflict
    );
    assert!(graph.is_dirty(&bond()));
    let result = calculate(&eight, &ctx8);
    assert_eq!(result.envelope().status(), ResultStatus::Complete);
    assert_eq!(result.envelope().quality(), DataQuality::Valid);
    graph
        .commit_result(&ticket, Ok(NodeValue::Valuation(result.clone())))
        .unwrap();
    let cached = graph.get_cached(&bond()).unwrap();
    assert_eq!(cached.generation, 8);
    assert_eq!(cached.snapshot_id.as_ref(), Some(eight.id()));
    assert_eq!(
        cached.configuration.as_ref(),
        Some(&ctx8.spec().configuration)
    );
    assert!(matches!(&cached.value, NodeValue::Valuation(value) if Arc::ptr_eq(value, &result)));
    let portfolio = NodeId::Portfolio {
        portfolio_id: convex_core::ids::PortfolioId::new("BOOK"),
    };
    let aggregate = graph.begin_calculation(&portfolio).unwrap();
    assert_eq!(aggregate.snapshot().unwrap().id(), eight.id());
    assert_eq!(
        aggregate.inputs()[0].value.snapshot_id.as_ref(),
        Some(eight.id())
    );
    assert_eq!(aggregate.inputs()[0].value.generation, 8);
    // No arithmetic/hedge certificate is claimed; the framework refuses mixed lineages.
    graph.activate_snapshot(definition(), snapshot(9)).unwrap();
    assert_eq!(
        graph
            .commit_result(
                &aggregate,
                Err(convex_engine::CalculationFailure::fixed(
                    ErrorCode::UnsupportedProduct,
                    "portfolio model not implemented"
                ))
            )
            .unwrap(),
        CommitOutcome::Discarded
    );
    assert!(graph.get_cached(&portfolio).is_none());
}

#[test]
fn snapshot_invalidation_requires_a_new_frozen_information_set_and_cannot_fall_back_to_live_inputs()
{
    let graph = CalculationGraph::new();
    let seven = snapshot(7);
    graph
        .activate_snapshot(definition(), seven.clone())
        .unwrap();
    let ctx = context(&seven, &["clean_price_mid"]);
    let ticket = graph.begin_valuation(&bond(), ctx.clone()).unwrap();
    graph
        .commit_result(&ticket, Ok(NodeValue::Valuation(calculate(&seven, &ctx))))
        .unwrap();
    let original = graph.get_cached(&bond()).unwrap();
    assert_eq!(graph.invalidate(&bond()).unwrap(), 8);
    assert!(graph.get_nodes_to_calculate().unwrap().is_empty());
    assert!(matches!(
        graph.begin_valuation(&bond(), ctx),
        Err(GraphError::NotReady)
    ));
    assert_eq!(
        graph.inspect_node(&bond()).unwrap().retained_quality,
        Some(DataQuality::Stale)
    );
    assert!(Arc::ptr_eq(
        &graph.inspect_node(&bond()).unwrap().last_good.unwrap(),
        &original
    ));
    let quote = NodeId::Quote {
        instrument_id: "BOND-001".into(),
    };
    assert_eq!(
        graph
            .publish_input(
                &quote,
                NodeValue::Quote {
                    bid: None,
                    ask: None,
                    mid: Some(rust_decimal::Decimal::from(100))
                }
            )
            .unwrap_err(),
        GraphError::ContextConflict
    );
    assert_eq!(
        graph
            .activate_snapshot(definition(), snapshot(8))
            .unwrap_err(),
        GraphError::ContextConflict
    );
    graph.activate_snapshot(definition(), snapshot(9)).unwrap();
    assert_eq!(graph.current_revision(), 9);
    assert!(graph.get_cached(&bond()).is_none());
}

#[test]
fn partial_invalid_and_failed_snapshot_measures_block_aggregation_and_keep_full_diagnostics() {
    for (missing_quote, measures, expected_status) in [
        (
            false,
            vec!["clean_price_mid", "unimplemented_measure"],
            ResultStatus::Partial,
        ),
        (
            true,
            vec!["clean_price_mid", "ytm_mid"],
            ResultStatus::Failed,
        ),
    ] {
        let graph = CalculationGraph::new();
        let mut content = fixture::content();
        if missing_quote {
            content.quotes[0].image = None;
        }
        let snapshot = Arc::new(MarketSnapshot::new(content).unwrap());
        graph
            .activate_snapshot(definition(), snapshot.clone())
            .unwrap();
        let ctx = context(&snapshot, &measures);
        let result = calculate(&snapshot, &ctx);
        assert_eq!(result.envelope().status(), expected_status);
        let ticket = graph.begin_valuation(&bond(), ctx).unwrap();
        graph
            .commit_result(&ticket, Ok(NodeValue::Valuation(result.clone())))
            .unwrap();
        let view = graph.inspect_node(&bond()).unwrap();
        assert!(view.last_good.is_none());
        let portfolio = NodeId::Portfolio {
            portfolio_id: convex_core::ids::PortfolioId::new("BOOK"),
        };
        assert!(matches!(
            graph.begin_calculation(&portfolio),
            Err(GraphError::NotReady)
        ));
        assert!(graph.get_nodes_to_calculate().unwrap().is_empty());
        if expected_status == ResultStatus::Failed {
            assert_eq!(view.phase, NodePhase::Failed);
            assert!(graph.get_cached(&bond()).is_none());
            assert!(Arc::ptr_eq(&view.failed_valuation.unwrap(), &result));
            assert_eq!(
                graph.inspect_node(&portfolio).unwrap().phase,
                NodePhase::Blocked
            );
        } else {
            assert_eq!(view.phase, NodePhase::Degraded);
            let cached = graph.get_cached(&bond()).unwrap();
            assert_eq!(cached.status, ResultStatus::Partial);
            assert!(
                matches!(&cached.value, NodeValue::Valuation(value) if !value.envelope().errors().is_empty())
            );
        }
    }
}

/// D58: one stale bond leaves a shared curve node Valid, so a valuation that depends on
/// the curve is still ready. A curve the snapshot does not hold is Invalid.
#[test]
fn a_stale_bond_beside_a_shared_curve_node_leaves_the_curve_valid() {
    let graph = CalculationGraph::new();
    let mut content = fixture::content();
    fixture::add_bond(&mut content, "BOND-002", 20_000);
    let snapshot = Arc::new(MarketSnapshot::new(content).unwrap());
    assert_ne!(snapshot.quality().quality, DataQuality::Valid);
    let curve = |id: &str| NodeId::Curve {
        curve_id: convex_core::ids::CurveId::new(id),
    };
    graph
        .activate_snapshot(
            vec![
                NodeSpec::calculation(curve("USD-FIXTURE"), vec![]),
                NodeSpec::calculation(curve("USD-ABSENT"), vec![]),
                NodeSpec::calculation(bond(), vec![curve("USD-FIXTURE")]),
            ],
            snapshot.clone(),
        )
        .unwrap();
    for (id, quality) in [
        ("USD-FIXTURE", DataQuality::Valid),
        ("USD-ABSENT", DataQuality::Invalid),
    ] {
        let ticket = graph.begin_calculation(&curve(id)).unwrap();
        graph
            .commit_result(
                &ticket,
                Ok(NodeValue::Curve {
                    points: vec![(365, 0.04)],
                }),
            )
            .unwrap();
        assert_eq!(graph.get_cached(&curve(id)).unwrap().quality, quality);
    }
    let ctx = context(&snapshot, &["clean_price_mid", "ytm_mid"]);
    let ticket = graph.begin_valuation(&bond(), ctx.clone()).unwrap();
    let result = calculate(&snapshot, &ctx);
    assert_eq!(result.envelope().status(), ResultStatus::Complete);
    assert_eq!(result.envelope().quality(), DataQuality::Valid);
    graph
        .commit_result(&ticket, Ok(NodeValue::Valuation(result)))
        .unwrap();
}
