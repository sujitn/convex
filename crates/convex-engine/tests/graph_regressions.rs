use convex_core::contracts::{DataQuality, ErrorCode};
use convex_engine::ports::config::{NodeConfig, UpdateFrequency};
use convex_engine::{
    CalculationFailure, CalculationGraph, CommitOutcome, GraphError, GraphLimits, NodeId,
    NodePhase, NodeSpec, NodeValue,
};
use std::collections::HashSet;
use std::sync::{mpsc, Arc, Barrier};
use std::time::{Duration, Instant};

fn curve(id: &str) -> NodeId {
    NodeId::Curve {
        curve_id: id.into(),
    }
}
fn value(rate: f64) -> NodeValue {
    NodeValue::Curve {
        points: vec![(365, rate), (730, rate)],
    }
}
fn commit(graph: &CalculationGraph, node: &NodeId, rate: f64) -> u64 {
    let ticket = graph.begin_calculation(node).unwrap();
    match graph.commit_result(&ticket, Ok(value(rate))).unwrap() {
        CommitOutcome::Committed { completion } => completion.revision,
        CommitOutcome::Discarded => panic!("current work was discarded"),
    }
}
fn fail() -> CalculationFailure {
    CalculationFailure::new(ErrorCode::CalibrationFailed, "fixture calibration failure").unwrap()
}

#[test]
fn cvx006_ac1_generation7_completion_cannot_clear_generation8_success_or_failure() {
    for success in [true, false] {
        let graph = Arc::new(CalculationGraph::new());
        let node = curve("RACE");
        graph
            .activate(vec![NodeSpec::calculation(node.clone(), vec![])])
            .unwrap();
        for _ in 0..6 {
            graph.invalidate(&node).unwrap();
        }
        assert_eq!(graph.current_revision(), 7);
        let captured = Arc::new(Barrier::new(2));
        let finish = Arc::new(Barrier::new(2));
        let worker = {
            let graph = graph.clone();
            let node = node.clone();
            let captured = captured.clone();
            let finish = finish.clone();
            std::thread::spawn(move || {
                let ticket = graph.begin_calculation(&node).unwrap();
                assert_eq!(ticket.generation(), 7);
                captured.wait();
                finish.wait();
                graph
                    .commit_result(
                        &ticket,
                        if success {
                            Ok(value(0.04))
                        } else {
                            Err(fail())
                        },
                    )
                    .unwrap()
            })
        };
        captured.wait();
        graph.invalidate(&node).unwrap();
        assert_eq!(graph.current_revision(), 8);
        let new = graph.begin_calculation(&node).unwrap();
        finish.wait();
        assert_eq!(worker.join().unwrap(), CommitOutcome::Discarded);
        assert!(graph.is_dirty(&node));
        assert_eq!(graph.inspect_node(&node).unwrap().phase, NodePhase::Running);
        assert!(graph.get_cached(&node).is_none());
        assert_eq!(new.generation(), 8);
        assert!(matches!(
            graph.commit_result(&new, Ok(value(0.05))).unwrap(),
            CommitOutcome::Committed { .. }
        ));
        let cached = graph.get_cached(&node).unwrap();
        assert_eq!(cached.generation, 8);
        assert!(matches!(&cached.value, NodeValue::Curve { points } if points[0].1 == 0.05));
    }
}

#[test]
fn cvx006_ac2_rejected_candidates_preserve_graph_cache_epochs_and_work() {
    let graph = CalculationGraph::with_limits(GraphLimits {
        max_nodes: 4,
        max_edges: 3,
        max_depth: 3,
        max_fanout: 2,
        ..GraphLimits::default()
    })
    .unwrap();
    let a = curve("A");
    let b = curve("B");
    let c = curve("C");
    let d = curve("D");
    graph
        .activate(vec![
            NodeSpec::calculation(a.clone(), vec![]),
            NodeSpec::calculation(b.clone(), vec![a.clone()]),
        ])
        .unwrap();
    commit(&graph, &a, 0.04);
    let cached = graph.get_cached(&a).unwrap();
    let in_flight = graph.begin_calculation(&b).unwrap();
    let epoch = graph.current_epoch();
    let generation = graph.current_revision();
    let cases = vec![
        (
            vec![
                NodeSpec::calculation(a.clone(), vec![b.clone()]),
                NodeSpec::calculation(b.clone(), vec![a.clone()]),
            ],
            GraphError::Cycle,
        ),
        (
            vec![NodeSpec::calculation(a.clone(), vec![a.clone()])],
            GraphError::Cycle,
        ),
        (
            vec![NodeSpec::calculation(a.clone(), vec![d.clone()])],
            GraphError::UnknownNode(d.clone()),
        ),
        (
            vec![
                NodeSpec::calculation(a.clone(), vec![]),
                NodeSpec::calculation(a.clone(), vec![]),
            ],
            GraphError::DuplicateNode(a.clone()),
        ),
        (
            vec![
                NodeSpec::calculation(a.clone(), vec![]),
                NodeSpec::calculation(b.clone(), vec![a.clone(), a.clone()]),
            ],
            GraphError::DuplicateDependency(b.clone()),
        ),
        (
            (0..5)
                .map(|i| NodeSpec::calculation(curve(&format!("N{i}")), vec![]))
                .collect(),
            GraphError::Limit("nodes"),
        ),
        (
            vec![
                NodeSpec::calculation(a.clone(), vec![]),
                NodeSpec::calculation(b.clone(), vec![a.clone()]),
                NodeSpec::calculation(c.clone(), vec![b.clone()]),
                NodeSpec::calculation(d.clone(), vec![c.clone()]),
            ],
            GraphError::Limit("depth"),
        ),
        (
            vec![
                NodeSpec::calculation(a.clone(), vec![]),
                NodeSpec::calculation(b.clone(), vec![a.clone()]),
                NodeSpec::calculation(c.clone(), vec![a.clone()]),
                NodeSpec::calculation(d.clone(), vec![a.clone()]),
            ],
            GraphError::Limit("fanout"),
        ),
        (
            vec![
                NodeSpec::calculation(a.clone(), vec![]),
                NodeSpec::calculation(b.clone(), vec![a.clone()]),
                NodeSpec::calculation(c.clone(), vec![a.clone(), b.clone()]),
                NodeSpec::calculation(d.clone(), vec![c.clone()]),
            ],
            GraphError::Limit("edges"),
        ),
    ];
    for (candidate, expected) in cases {
        assert_eq!(graph.activate(candidate).unwrap_err(), expected);
        assert_eq!(graph.current_epoch(), epoch);
        assert_eq!(graph.current_revision(), generation);
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.in_flight_count(), 1);
        assert_eq!(graph.get_dependencies(&b), vec![a.clone()]);
        assert!(Arc::ptr_eq(&cached, &graph.get_cached(&a).unwrap()));
        assert_eq!(graph.inspect_node(&b).unwrap().phase, NodePhase::Running);
    }
    assert!(matches!(
        graph.commit_result(&in_flight, Ok(value(0.05))).unwrap(),
        CommitOutcome::Committed { .. }
    ));
    assert_eq!(
        graph
            .upsert_nodes(vec![NodeSpec::calculation(a.clone(), vec![b.clone()])])
            .unwrap_err(),
        GraphError::Cycle
    );
    assert_eq!(graph.current_epoch(), epoch);
}

#[test]
fn cvx006_ac3_priority_only_applies_to_ready_nodes_and_revisions_are_captured() {
    let graph = CalculationGraph::new();
    let a = curve("LOW");
    let b = curve("HIGH");
    let c = curve("INDEPENDENT");
    let config = NodeConfig {
        priority: i32::MAX,
        ..NodeConfig::default()
    };
    graph
        .activate(vec![
            NodeSpec::calculation(a.clone(), vec![]),
            NodeSpec::calculation(b.clone(), vec![a.clone()]).with_config(config),
            NodeSpec::calculation(c.clone(), vec![]).with_config(NodeConfig {
                priority: 10,
                ..NodeConfig::default()
            }),
        ])
        .unwrap();
    assert_eq!(
        graph.get_nodes_to_calculate().unwrap(),
        vec![c.clone(), a.clone()]
    );
    assert!(matches!(
        graph.begin_calculation(&b),
        Err(GraphError::NotReady)
    ));
    let independent = graph.begin_calculation(&c).unwrap();
    let revision_a = commit(&graph, &a, 0.04);
    assert_eq!(graph.get_nodes_to_calculate().unwrap(), vec![b.clone()]);
    let ticket = graph.begin_calculation(&b).unwrap();
    assert_eq!(ticket.inputs().len(), 1);
    assert_eq!(ticket.input(&a).unwrap().revision, revision_a);
    // Invalidating an unrelated branch does not discard otherwise current work.
    graph.invalidate(&c).unwrap();
    assert_eq!(
        graph.commit_result(&independent, Ok(value(0.06))).unwrap(),
        CommitOutcome::Discarded
    );
    assert!(matches!(
        graph.commit_result(&ticket, Ok(value(0.05))).unwrap(),
        CommitOutcome::Committed { .. }
    ));
    let cached_b = graph.get_cached(&b).unwrap();
    assert_eq!(cached_b.dependencies[0].revision, revision_a);
    assert_eq!(cached_b.dependencies[0].node_id, a);
    assert!(cached_b.revision > revision_a);
}

#[test]
fn cvx006_ac3_deep_wide_diamond_traversal_is_iterative_and_does_not_deadlock() {
    let (done_tx, done_rx) = mpsc::channel();
    let handle = std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(move || {
            let graph = CalculationGraph::new();
            let depth = 4_096;
            let width = 2_000;
            let mut nodes = Vec::new();
            for i in 0..depth {
                nodes.push(NodeSpec::calculation(
                    curve(&format!("D{i}")),
                    if i == 0 {
                        vec![]
                    } else {
                        vec![curve(&format!("D{}", i - 1))]
                    },
                ));
            }
            for i in 0..width {
                nodes.push(NodeSpec::calculation(
                    curve(&format!("W{i}")),
                    vec![curve("D0")],
                ));
            }
            nodes.push(NodeSpec::calculation(
                curve("DIAMOND"),
                vec![curve("W0"), curve("W1")],
            ));
            graph.activate(nodes).unwrap();
            // Commit the actual chain; children enter the frontier only after their dependency commits.
            for i in 0..depth {
                commit(&graph, &curve(&format!("D{i}")), 0.04);
            }
            for i in 0..width {
                commit(&graph, &curve(&format!("W{i}")), 0.04);
            }
            commit(&graph, &curve("DIAMOND"), 0.04);
            assert!(graph.get_dirty_nodes().is_empty());
            let generation = graph.invalidate(&curve("D0")).unwrap();
            assert_eq!(graph.get_dirty_nodes().len(), depth + width + 1);
            assert_eq!(
                graph.inspect_node(&curve("D4095")).unwrap().generation,
                generation
            );
            assert_eq!(graph.get_dependents(&curve("D0")).len(), width + 1);
            let ticket = graph.begin_calculation(&curve("D0")).unwrap();
            graph.commit_result(&ticket, Err(fail())).unwrap();
            assert_eq!(
                graph.inspect_node(&curve("D4095")).unwrap().phase,
                NodePhase::Blocked
            );
            assert_eq!(
                graph.inspect_node(&curve("DIAMOND")).unwrap().phase,
                NodePhase::Blocked
            );
            assert_eq!(
                graph
                    .inspect_node(&curve("DIAMOND"))
                    .unwrap()
                    .retained_quality,
                Some(DataQuality::Stale)
            );
            assert!(graph.get_nodes_to_calculate().unwrap().is_empty());
            done_tx.send(()).unwrap();
        })
        .unwrap();
    done_rx
        .recv_timeout(Duration::from_secs(20))
        .expect("bounded graph traversal stalled/deadlocked");
    handle.join().unwrap();
}

#[test]
fn replacement_rebuilds_reverse_edges_without_duplicates_and_preserves_unrelated_inputs() {
    let graph = CalculationGraph::new();
    let a = curve("FIRST");
    let b = curve("SECOND");
    let c = curve("CHILD");
    graph
        .activate(vec![
            NodeSpec::input(a.clone()),
            NodeSpec::input(b.clone()),
            NodeSpec::calculation(c.clone(), vec![a.clone()]),
        ])
        .unwrap();
    graph.publish_input(&a, value(0.04)).unwrap();
    graph.publish_input(&b, value(0.05)).unwrap();
    let retained = graph.get_cached(&b).unwrap();
    for _ in 0..3 {
        graph.add_node(c.clone(), vec![a.clone()]).unwrap();
    }
    assert_eq!(graph.get_dependents(&a), vec![c.clone()]);
    assert!(Arc::ptr_eq(&retained, &graph.get_cached(&b).unwrap()));
    graph.add_node(c.clone(), vec![b.clone()]).unwrap();
    assert!(graph.get_dependents(&a).is_empty());
    assert_eq!(graph.get_dependents(&b), vec![c.clone()]);
    commit(&graph, &c, 0.06);
    graph.publish_input(&a, value(0.07)).unwrap();
    assert!(!graph.is_dirty(&c));
    graph.publish_input(&b, value(0.08)).unwrap();
    assert!(graph.is_dirty(&c));
}

#[test]
fn failed_recalculation_retains_original_good_value_only_as_stale_and_blocks_dependents() {
    let graph = CalculationGraph::new();
    let a = curve("A");
    let b = curve("B");
    graph
        .activate(vec![
            NodeSpec::calculation(a.clone(), vec![]),
            NodeSpec::calculation(b.clone(), vec![a.clone()]),
        ])
        .unwrap();
    commit(&graph, &a, 0.04);
    commit(&graph, &b, 0.05);
    let original = graph.get_cached(&a).unwrap();
    let original_b = graph.get_cached(&b).unwrap();
    graph.invalidate(&a).unwrap();
    assert!(graph.get_cached(&a).is_none());
    assert!(graph.get_cached(&b).is_none());
    let ticket = graph.begin_calculation(&a).unwrap();
    graph.commit_result(&ticket, Err(fail())).unwrap();
    let view = graph.inspect_node(&a).unwrap();
    assert_eq!(view.phase, NodePhase::Failed);
    assert_eq!(view.failure.unwrap().code, ErrorCode::CalibrationFailed);
    assert_eq!(view.retained_quality, Some(DataQuality::Stale));
    assert!(Arc::ptr_eq(&view.last_good.unwrap(), &original));
    let blocked = graph.inspect_node(&b).unwrap();
    assert_eq!(blocked.phase, NodePhase::Blocked);
    assert_eq!(blocked.failure.unwrap().dependency, Some(a.clone()));
    assert!(Arc::ptr_eq(&blocked.last_good.unwrap(), &original_b));
    assert!(graph.get_nodes_to_calculate().unwrap().is_empty());
    graph.request_calculation(&a).unwrap();
    commit(&graph, &a, 0.06);
    commit(&graph, &b, 0.07);
    assert_eq!(graph.inspect_node(&b).unwrap().phase, NodePhase::Fresh);
}

#[test]
fn multiple_invalidations_reach_already_dirty_descendants_and_source_publication_is_atomic() {
    let graph = CalculationGraph::new();
    let a = curve("INPUT");
    let b = curve("B");
    let c = curve("C");
    graph
        .activate(vec![
            NodeSpec::input(a.clone()),
            NodeSpec::calculation(b.clone(), vec![a.clone()]),
            NodeSpec::calculation(c.clone(), vec![b.clone()]),
        ])
        .unwrap();
    assert!(graph.get_nodes_to_calculate().unwrap().is_empty());
    graph.publish_input(&a, value(0.04)).unwrap();
    let old = graph.begin_calculation(&b).unwrap();
    graph.publish_input(&a, value(0.05)).unwrap();
    let generation = graph.current_revision();
    assert_eq!(graph.inspect_node(&b).unwrap().generation, generation);
    assert_eq!(graph.inspect_node(&c).unwrap().generation, generation);
    assert!(!graph.is_dirty(&a));
    assert_eq!(graph.get_nodes_to_calculate().unwrap(), vec![b.clone()]);
    assert_eq!(
        graph.commit_result(&old, Ok(value(0.03))).unwrap(),
        CommitOutcome::Discarded
    );
    commit(&graph, &b, 0.06);
    commit(&graph, &c, 0.07);
    graph.evict_cached(&a).unwrap();
    assert!(graph.get_cached(&a).is_none());
    assert!(graph.get_cached(&c).is_none());
    assert_eq!(graph.inspect_node(&c).unwrap().phase, NodePhase::Blocked);
    graph.publish_input(&a, value(0.08)).unwrap();
    assert_eq!(graph.get_nodes_to_calculate().unwrap(), vec![b]);
}

#[test]
fn graph_replacement_clear_foreign_graph_and_cancel_cannot_reuse_old_tickets() {
    let graph = CalculationGraph::new();
    let foreign = CalculationGraph::new();
    let a = curve("A");
    let definition = || vec![NodeSpec::calculation(a.clone(), vec![])];
    graph.activate(definition()).unwrap();
    foreign.activate(definition()).unwrap();
    let old = graph.begin_calculation(&a).unwrap();
    let other = foreign.begin_calculation(&a).unwrap();
    assert_eq!(
        graph.commit_result(&other, Ok(value(0.04))).unwrap(),
        CommitOutcome::Discarded
    );
    assert!(!graph.cancel_attempt(&other));
    graph.clear().unwrap();
    graph.activate(definition()).unwrap();
    let new = graph.begin_calculation(&a).unwrap();
    assert!(!graph.cancel_attempt(&old));
    assert_eq!(
        graph.commit_result(&old, Err(fail())).unwrap(),
        CommitOutcome::Discarded
    );
    assert!(graph.is_dirty(&a));
    assert_eq!(graph.in_flight_count(), 1);
    assert!(graph.cancel_attempt(&new));
    assert!(graph.is_dirty(&a));
    commit(&graph, &a, 0.05);
}

#[test]
fn ready_queue_and_concurrent_claims_are_bounded_without_dropping_pending_work() {
    let graph = Arc::new(
        CalculationGraph::with_limits(GraphLimits {
            max_ready: 2,
            max_in_flight: 4,
            ..GraphLimits::default()
        })
        .unwrap(),
    );
    graph
        .activate(
            (0..8)
                .map(|i| {
                    NodeSpec::calculation(curve(&format!("N{i}")), vec![]).with_config(NodeConfig {
                        priority: i,
                        ..NodeConfig::default()
                    })
                })
                .collect(),
        )
        .unwrap();
    assert_eq!(
        graph.get_nodes_to_calculate().unwrap(),
        vec![curve("N7"), curve("N6")]
    );
    assert!(matches!(
        graph.claim_ready(3),
        Err(GraphError::Limit("ready batch"))
    ));
    let claimed = Arc::new(Barrier::new(9));
    let finish = Arc::new(Barrier::new(9));
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let graph = graph.clone();
            let claimed = claimed.clone();
            let finish = finish.clone();
            std::thread::spawn(move || {
                let tickets = graph.claim_ready(1).unwrap();
                claimed.wait();
                finish.wait();
                tickets
                    .into_iter()
                    .map(|ticket| {
                        let id = ticket.node_id().clone();
                        assert!(matches!(
                            graph.commit_result(&ticket, Ok(value(0.04))).unwrap(),
                            CommitOutcome::Committed { .. }
                        ));
                        id
                    })
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    claimed.wait();
    assert_eq!(graph.in_flight_count(), 4);
    finish.wait();
    let ids: Vec<_> = workers
        .into_iter()
        .flat_map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(ids.len(), 4);
    assert_eq!(ids.iter().collect::<HashSet<_>>().len(), 4);
    assert_eq!(graph.get_dirty_nodes().len(), 4);
    while !graph.get_dirty_nodes().is_empty() {
        for ticket in graph.claim_ready(2).unwrap() {
            graph.commit_result(&ticket, Ok(value(0.05))).unwrap();
        }
    }
    assert_eq!(graph.in_flight_count(), 0);
}

#[test]
fn input_and_job_expiry_cannot_publish_or_keep_capacity_claimed_indefinitely() {
    let graph = CalculationGraph::with_limits(GraphLimits {
        max_job_lifetime: Duration::from_millis(1),
        ..GraphLimits::default()
    })
    .unwrap();
    let a = curve("A");
    let b = curve("B");
    graph
        .activate(vec![
            NodeSpec::input(a.clone()),
            NodeSpec::calculation(b.clone(), vec![a.clone()]),
        ])
        .unwrap();
    graph
        .publish_input_until(
            &a,
            value(0.04),
            Some(Instant::now() + Duration::from_millis(1)),
        )
        .unwrap();
    let ticket = graph.begin_calculation(&b).unwrap();
    std::thread::sleep(Duration::from_millis(3));
    assert!(graph.get_nodes_to_calculate().unwrap().is_empty());
    assert_eq!(graph.in_flight_count(), 0);
    assert_eq!(
        graph.commit_result(&ticket, Ok(value(0.05))).unwrap(),
        CommitOutcome::Discarded
    );
    assert!(graph.get_cached(&a).is_none());
    assert!(graph.get_cached(&b).is_none());
    graph.publish_input(&a, value(0.06)).unwrap();
    let ticket = graph.begin_calculation(&b).unwrap();
    std::thread::sleep(Duration::from_millis(3));
    assert_eq!(
        graph.commit_result(&ticket, Ok(value(0.07))).unwrap(),
        CommitOutcome::Discarded
    );
    assert_eq!(
        graph.inspect_node(&b).unwrap().failure.unwrap().code,
        ErrorCode::DeadlineExceeded
    );
    assert_eq!(graph.in_flight_count(), 0);
}

#[test]
fn unavailable_invalid_and_unversioned_outputs_never_become_numeric_success() {
    let graph = CalculationGraph::new();
    let a = curve("A");
    graph
        .activate(vec![NodeSpec::calculation(a.clone(), vec![])])
        .unwrap();
    assert!(matches!(
        graph.publish_input(&a, value(0.04)),
        Err(GraphError::InvalidNode(_))
    ));
    let ticket = graph.begin_calculation(&a).unwrap();
    assert!(matches!(
        graph.commit_result(&ticket, Ok(value(f64::NAN))),
        Err(GraphError::InvalidNode(_))
    ));
    assert!(graph.is_dirty(&a));
    assert!(graph.get_cached(&a).is_none());
    assert!(graph.cancel_attempt(&ticket));
    assert_eq!(
        graph
            .activate(vec![NodeSpec::input(a.clone()).with_config(NodeConfig {
                frequency: UpdateFrequency::Throttled {
                    interval: Duration::ZERO
                },
                ..NodeConfig::default()
            })])
            .unwrap_err(),
        GraphError::InvalidNode("invalid update interval")
    );
    assert!(CalculationGraph::with_limits(GraphLimits {
        max_nodes: 100_001,
        ..GraphLimits::default()
    })
    .is_err());
    assert!(CalculationGraph::with_sharding(convex_engine::ShardConfig::hash_based(1, 0)).is_err());
}

#[test]
fn on_demand_work_requires_a_trigger_and_does_not_clear_current_value() {
    let graph = CalculationGraph::new();
    let a = curve("A");
    graph
        .activate(vec![NodeSpec::calculation(a.clone(), vec![]).with_config(
            NodeConfig {
                frequency: UpdateFrequency::OnDemand,
                ..NodeConfig::default()
            },
        )])
        .unwrap();
    assert!(graph.get_nodes_to_calculate().unwrap().is_empty());
    graph.request_calculation(&a).unwrap();
    assert_eq!(graph.get_nodes_to_calculate().unwrap(), vec![a.clone()]);
    commit(&graph, &a, 0.04);
    let original = graph.get_cached(&a).unwrap();
    graph.request_calculation(&a).unwrap();
    assert!(graph.get_cached(&a).is_none());
    assert!(graph.is_dirty(&a));
    assert!(Arc::ptr_eq(
        &graph.inspect_node(&a).unwrap().last_good.unwrap(),
        &original
    ));
    assert_eq!(
        graph.inspect_node(&a).unwrap().retained_quality,
        Some(DataQuality::Stale)
    );
}

#[test]
fn newer_committed_value_survives_late_success_failure_and_cancellation() {
    for success in [true, false] {
        let graph = CalculationGraph::new();
        let node = curve("LATE");
        graph.add_node(node.clone(), vec![]).unwrap();
        let old = graph.begin_calculation(&node).unwrap();
        graph.invalidate(&node).unwrap();
        commit(&graph, &node, 0.06);
        let new = graph.get_cached(&node).unwrap();
        assert_eq!(
            graph
                .commit_result(
                    &old,
                    if success {
                        Ok(value(0.01))
                    } else {
                        Err(fail())
                    }
                )
                .unwrap(),
            CommitOutcome::Discarded
        );
        assert!(!graph.cancel_attempt(&old));
        assert!(Arc::ptr_eq(&new, &graph.get_cached(&node).unwrap()));
        assert_eq!(graph.inspect_node(&node).unwrap().phase, NodePhase::Fresh);
        assert!(!graph.is_dirty(&node));
    }
}

#[test]
fn node_staleness_policy_is_enforced_without_refreshing_original_completion_time() {
    let graph = CalculationGraph::new();
    let node = curve("AGED");
    graph
        .activate(vec![NodeSpec::calculation(node.clone(), vec![])
            .with_config(NodeConfig {
                stale_threshold: Some(Duration::from_millis(1)),
                ..NodeConfig::default()
            })])
        .unwrap();
    commit(&graph, &node, 0.04);
    let good = graph.get_cached(&node).unwrap();
    std::thread::sleep(Duration::from_millis(3));
    assert!(graph.get_cached(&node).is_none());
    assert_eq!(graph.inspect_node(&node).unwrap().phase, NodePhase::Stale);
    assert!(graph.get_nodes_to_calculate().unwrap().is_empty());
    let view = graph.inspect_node(&node).unwrap();
    assert_eq!(view.failure.unwrap().code, ErrorCode::StaleInput);
    assert!(Arc::ptr_eq(&view.last_good.unwrap(), &good));
}
