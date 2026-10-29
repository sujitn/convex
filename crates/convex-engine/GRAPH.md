# Calculation graph and result commits

`CalculationGraph` is a bounded, process-local dependency DAG. It validates updates
before activation, captures inputs before calculation and commits only the still
current attempt. Calculations and reference I/O run outside its state lock. This
corrects graph ordering and stale completion races; authoritative runtime
consolidation is the next integration task.

The public API and orchestration are in [calc_graph.rs](src/calc_graph.rs), with
private [DAG validation](src/calc_graph/dag.rs) and
[state transitions](src/calc_graph/state.rs). The application keeps
one short-lived `parking_lot::Mutex` around structural and state transitions.
There are no nested concurrent-map guards, recursive invalidation, new scheduler,
provider SDK, database requirement or distributed coordination protocol.

## Activation and limits

Construct a complete candidate using `NodeSpec::input` and
`NodeSpec::calculation`. Input nodes have no dependencies and require external
publication. Calculations declare every graph prerequisite. Known dependencies,
unique nodes and edges, supported roles, bounded scheduling settings and an
acyclic topology are checked before activation. Iterative Kahn traversal also
checks maximum path depth. Unknown prerequisites are rejected, never implicitly
created as missing calculations.

`activate` replaces the whole graph. `upsert_nodes` validates one administrative
batch against the active graph and rebuilds reverse edges. It invalidates changed
nodes and their dependent closure, while preserving unrelated inputs/results.
Structural changes cancel existing attempts; unaffected running work remains
pending for a new attempt. `set_node_config` performs its update under the same
transaction. Repeated registration cannot accumulate reverse edges or obsolete
timer registrations.

Rejected candidates leave topology, configuration, generations, epochs,
committed results, timestamps and active attempts unchanged. Do not activate a
partly validated candidate by making a sequence of independent writes.

| Resource | Default | Hard ceiling |
|---|---:|---:|
| Nodes | 100,000 | 100,000 |
| Directed edges | 500,000 | 500,000 |
| Nodes on a dependency path | 4,096 | 4,096 |
| Direct dependent fanout | 10,000 | 10,000 |
| Ready candidates per pass | 1,024 | 1,024 |
| Simultaneous attempts | 64 | 64 |
| Attempt lifetime | 30 seconds | 300 seconds |
| Curve points or surface expiries per value | 4,096 | 4,096 |
| Failure message | 1,024 bytes | 1,024 bytes |

`with_limits` can tighten these bounds. Ready enumeration scans the bounded DAG
and retains a bounded priority frontier. Admission does not discard pending work
outside that frontier. This bounds work and allocation; it is not a measured
memory, throughput or colocation certificate. Prepare large graphs in batches.

## Publication and work admission

`publish_input` or `publish_input_until` commits a supplied external input and
invalidates its dependent closure atomically. Input freshness deadlines travel
with captured values. Unknown external roots can be admitted under the same DAG
limits; supplied values must match their identity and valid input role. A calculated
curve is distinct from an externally supplied root curve. A calculated output
cannot use input publication to bypass a ticket.

`invalidate` advances the input generation of the affected closure, even when
nodes are already dirty. An invalidated input requires a new publication; it is
never recalculated by rereading its previous cache entry. `request_calculation`
also enables an explicit timer/on-demand retry for the target. Priority applies
only among nodes whose prerequisites have already committed complete, valid,
nonexpired results. A dependent is admitted in a later frontier after its
prerequisites commit, regardless of its priority.

Use `claim_ready(max)` for bounded parallel admission or `begin_calculation` for
an explicit ready calculation. Admission captures an opaque `CalculationTicket`
with graph ownership/epoch, target generation, unique attempt, monotonic deadline
and immutable prerequisite values/revisions. Calculate from `ticket.inputs()`;
do not query the mutable graph again after awaiting I/O.

`commit_result(&ticket, result)` returns either `Committed { completion }` or
`Discarded`. Commit checks the same graph, epoch, target generation, attempt,
prerequisite generations/revisions and deadlines under one lock. An obsolete
success, failure or cancellation cannot alter newer dirty work or an already
committed newer value. `cancel_attempt` releases only its still-current attempt.
Abandoned attempts expire on the next admission/frontier pass; late work cannot
publish. Counter exhaustion is an explicit error; clear/replacement never resets
counters to permit old tickets again.

The completion contains captured generation, monotonic result revision,
completeness, quality, primary failure and optional snapshot/configuration pins.
It is constructed at commit rather than by reading mutable state afterward.
Input generation and result revision have different meanings and must not be
substituted for one another.

## Failure and retained values

There is no `NodeValue::Empty` success. Unsupported products, missing reference,
failed calibration, stale input and expired work remain explicit failures.
Failures block dependent calculations iteratively and preserve their originating
dependency. Invalid result types, absent numerical results and nonfinite model
values are rejected. A worker must handle a rejected commit and release its
attempt, or wait for the bounded expiry/retry policy.

`get_cached` returns only current, nonexpired results. It never returns dirty,
failed, suspended or snapshot-pending values. A current partial or nonvalid
snapshot result remains inspectable with its explicit status/quality, but cannot
feed another calculation. `inspect_node` distinguishes Pending, WaitingInput,
Running, Fresh, Degraded, Failed, Blocked and Stale.

Last valid complete results are available only through `NodeView.last_good` with
`retained_quality`. Invalidating or failing a refresh labels these values Stale
and preserves their original generation, revision and timestamp. A failed
snapshot result also retains its full checked context and every requested-measure
error in `NodeView.failed_valuation`. No missing risk or price becomes zero/par.
Node staleness policy and earliest consumed input deadline are both enforced;
cache reads do not extend deadlines.

The [pricing runtime](RUNTIME.md) owns canonical result publication and durable
revisions, and its results channel is the only WebSocket source. Graph completions
are not streamed. Canonical valuation streaming is explicitly subscribed, reports
lag and supports current-state resynchronization; durable managed catch-up and v2
API adoption remain later work.

## Immutable snapshot use

`activate_snapshot(nodes, Arc<MarketSnapshot>)` builds the curves of the CVX-008
information set and requires its positive generation to advance beyond the current
graph counter. The snapshot itself was verified when it was made or deserialized and
is not re-verified. `activate_built(nodes, &BuiltSnapshot)` does the same for a
snapshot `BuiltSnapshot::build` has already built, so the runtime
builds once and passes the same build to `SnapshotValuator::value_built` for every
node. All nodes in that activation share the same snapshot/configuration.
For bond pricing, call `begin_valuation(node, ValuationContext)` with the complete
matching context, then call the existing `SnapshotValuator` and commit
`NodeValue::Valuation(result)`. The ticket holds the actual immutable snapshot and
context. Commit rejects a different settlement/model/measure/build context even
when it happens to produce equal numbers.

A dependent captures only committed complete/valid values with the activation's
snapshot identity. Another snapshot activation invalidates every earlier ticket.
Invalidating snapshot work suspends admission until a newly frozen information
set with a newer generation is activated. Mutable input writes, administrative
upserts and switching back to an unbound graph are rejected. Clearing the graph
does not silently enable a live-input fallback. Create another graph deliberately
if an independent compatibility workload is needed.

See [snapshot replay and context contracts](SNAPSHOTS.md) and
[actual snapshot valuation tests](tests/graph_snapshots.rs). This metadata is a
necessary consistency condition; it grants no execution authorization, market
data entitlement or current-time freshness certificate.

## Migration and runtime scope

| Previous internal API | Current use |
|---|---|
| `add_node` with unknown dependencies | Explicit roots plus one `upsert_nodes` batch; handle `Result` |
| `update_cache` / `update_cache_until` | `publish_input[_until]` for external inputs; captured ticket plus `commit_result` for calculations |
| `NodeValue::Empty` | Typed `CalculationFailure`, or a valid checked numerical result |
| Arbitrary priority-sorted dirty list | Bounded dependency-ready frontier; claim before work |
| `get_cached` for old values | `inspect_node().last_good` and its explicit retained quality |
| Force recalculation that clears work | `request_calculation` for an independently owned graph; runtime uses captured updates |
| Raw sharding constructor | Checked `with_sharding`; local partition hints do not fence execution |
| Notification interpreted as success | Inspect `NodeUpdate.completion` and its status/quality |

These are internal engine Rust API changes. Pure mathematical APIs, FFI, JSON
pricing units and existing financial tolerances are preserved; there is no
storage migration. Graph state is process-local and can be rebuilt from validated
administrative definitions and pinned snapshots after restart.

`PricingEngine` now owns one [event-driven lifecycle](RUNTIME.md). It assembles
complete immutable reference/configuration/curve/quote information sets, captures
contexts, runs bounded CPU jobs, commits checked results and checkpoints publication.
The independent reactive calculator and placeholder coordinator are removed.
Reference/settlement/configuration compatibility reads no longer drive runtime
valuation. Graph curve calibration and unsupported ETF/portfolio mathematics remain
their owning tasks; the runtime admits only its explicit fixed-bullet scope.

Joined shutdown, local recovery and canonical publication are exercised. Cooperative
I/O deadlines do not imply hard CPU preemption. Multi-host leases/ownership, live
provider certification, complete curve calibration, durable managed catch-up and
measured latency remain separate work. Production and library gates are incomplete.

## Validation

[Graph regression tests](tests/graph_regressions.rs) force generation 7/8 late
success/failure, rejected candidates, hostile priorities, bounded parallel claims,
clear/replacement/cancellation, expiry, last-good retention and a 4,096-node chain
with 2,000-node fanout on a 128 KiB thread stack. Native unit tests check counter
exhaustion without partial mutation. [Snapshot tests](tests/graph_snapshots.rs)
exercise the actual snapshot valuator and reject wrong/mixed contexts, partial
coverage and failed inputs. Reactive unit tests exercise the actual bond router
after an async yield, and a server unit test verifies failed completion metadata
through the existing WebSocket bridge.

Run from the repository root:

```sh
cargo fmt --all -- --check
cargo test --locked -p convex-engine
cargo clippy --locked -p convex-engine -p convex-server --all-targets --all-features -- -D warnings
cargo test --locked -p convex-server --test api_integration_tests production_readiness_ -- --include-ignored --test-threads=1
```

The last command is a separately enforced production probe. Its success is not a
complete pricing/risk/library or production release certificate.
