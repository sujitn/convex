# Convex pricing runtime

`PricingEngine` owns one event-driven lifecycle for a bounded fixed-bullet
valuation universe. It resolves exact inputs, calculates through `SnapshotValuator`,
commits through the [validated graph](GRAPH.md), and publishes canonical result
states through replaceable [recovery and output ports](../convex-ports/src/runtime.rs).
Local replay, process restart, failure retention and joined shutdown are exercised.
This implementation does not certify a live provider or any production release gate.

## Supported operating scope

A `RuntimePlan` supplies one immutable quantitative configuration, up to 256 exact
instrument versions, one selected quote source per instrument, supplied curve
revisions, explicit settlement, compiled numerical policy, build identity and
requested measures. The implemented measures are `clean_price_mid`,
`dirty_price_mid`, `accrued_interest` and `ytm_mid` for reference-admitted fixed
bullets. Unsupported measures, required fixing keys, products and configurations
fail explicitly. Required missing/stale/invalid curves block runtime results even
when an observed clean mark would otherwise be available. No spread, risk, price
or curve is synthesized to fill a missing dependency.

The complete manifest supplies each bond's dependencies. Every bond in a batch
consumes the same captured snapshot and configuration. Graph mutation, mutable
curve-builder reads and current reference/configuration lookups do not occur in
CPU workers. CVX-010 adds [validated configuration activation](CONFIGURATION.md):
quote freshness/skew and approved scoped exceptions can change within the same
process footprint. Accepted work freezes its activation before acknowledgment;
in-flight calculations retain their original version. Reference/curve/model,
settlement, build and process footprint changes require a compatible explicit
plan and intentional new namespace when its digest changes. Ordinary reference
repository updates retain historical pins. Complete calibration and broader
analytics remain their owning tasks.

| Mode | Input and time | Readiness meaning |
| --- | --- | --- |
| Captured | Caller supplies normalized events and both cutoffs | Complete valid historical/captured context |
| Replay | Pull source such as `FileQuoteReplay`, with original event order/time | Complete valid historical context; EOF sets `replay_complete` |
| Live | Configured `MarketDataProvider.quotes` subscription | All selected sources supplied new images in this process, data remains valid, output is acknowledged |
| Idle demo | Explicit `with_demo_mode`, no numerical runtime | Demo quality; readiness stays false |

`Live` is a provider integration point. Its broadcast transport reports lag and
closure; these stop acceptance, publish a failed/stale state when storage/output
permit it, and unsubscribe. Restart restores dependencies but waits for new images
from all selected sources. A deadline wakeup revalues expired inputs without a
periodic calculation poll. Live records include `live_valid_until`, derived from
the original dependency times. Consumers must respect that deadline, snapshot
quality and their own product freshness rules. Analytics readiness never grants
strategy or execution permission.

## Construction and lifecycle

Use `PricingEngineBuilder::with_runtime(RuntimeServices)` to supply a checked plan,
immutable snapshot/fixing repositories, `RuntimeStore`, `ResultPublisher` and input
ownership. Reference/configuration reads use the engine's authoritative storage.
Add `with_configuration_use_cases` with actual composition capabilities to adopt
managed selections. An active pointer without that composition fails startup.
`create_redb_storage_with_runtime` returns those repositories and the checkpoint
store over one local redb database. `FileSnapshotRepository` and
`JsonlResultPublisher` are local Unix adapters; alternate backends implement the
same narrow ports without importing the engine or changing pricing mathematics.

`start().await` runs once. It verifies the configured plan digest, exact references
and resource pins, configuration revision, accepted trace, immutable snapshots,
result contexts and retained last-good values. It calculates pending accepted
work, restores the graph and flushes retained output before reporting readiness.
A restored live runtime waits for fresh images. Concurrent starts have one owner;
repeated starts fail. Restart means constructing a new engine instance.
The optional accepted activation in the checkpoint restores its exact historical
selection first. After accepted pending work/output is recovered, the actor
reconciles a newer pointer before readiness. Hot activation preserves the original
plan digest and durable runtime identity; keep that bootstrap plan on restart.

`shutdown().await` closes admission, stops pulling the source, drains already
queued commands, finishes/checkpoints accepted work and output, unsubscribes, and
joins the actor and every owned CPU job. A publication/storage/worker failure is
returned, with recoverable state retained where its durable commit succeeded.
Successful shutdown is idempotent. Dropping an engine signals emergency stopping;
callers must await shutdown for the joining guarantee.

Event calculations admit at most `EngineConfig.max_concurrent_calcs` blocking jobs
at once, with default 4 and valid range 1 through 64. Interactive
`evaluate_context` requests have a separate bounded budget of the same size and
fail capacity admission rather than building an unbounded queue. Canceled request
replies do not detach their CPU jobs; shutdown still joins them. Tokio's shared
blocking executor is used. Dedicated pool isolation and hard CPU preemption are
not claimed. I/O waits have a ten-second cooperative budget; graph attempts retain
the graph's job deadline. A job that completes too late cannot publish a fresh
result. No managed or colocated latency target has been measured here.

## Durable acceptance and publication

`submit_update` is available only in Captured mode. It validates the selected
source/instrument, event shape, cutoffs and delivery order. The actor persists the
new generation and `pending=true` before returning `AcceptedUpdate`. Queueing
alone is not acknowledgment. If the caller loses its reply, reconcile the durable
checkpoint before retrying. Normalized event sequences are still checked by the
canonical assembler; no trade/cash/execution coalescing is introduced.

The actor assembles and publishes an immutable manifest, captures graph tickets,
executes outside the graph lock and commits only current attempts. It then commits
canonical result state and the output batch together. `pending=false` means the
batch is checkpointed, rather than proof of downstream delivery. The outbox is
cleared only after publisher acknowledgment. Failed/uncertain checkpoint writes
are loaded and compared to the exact candidate before acceptance or retry.

`PublishedValuation` contains a stable runtime identity, durable monotonic revision,
captured generation, complete context key, current canonical result and optional
original last-good result. The delivery deduplication key is `(runtime_id,
revision)`. Revisions survive process restart and differ from local graph counters.
An output adapter may receive the same delivery key again after a crash or lost
acknowledgment. This is an at-least-once contract, not exactly-once execution.

`JsonlResultPublisher` acknowledges after file sync, reconciles identical delivery
keys and rejects changed content. Reopening validates complete bounded records.
Torn/corrupt tails require explicit repair; the adapter never silently skips them.
Stop writers and retain the damaged evidence, restore the archive or verify and
repair its final unacknowledged record, then reopen the publisher. The runtime's
unchanged outbox supplies the retry. Rotating an archive requires retaining prior
files and their delivery keys under the operator's retention policy.

A failed refresh omits current numeric results, preserves structured errors and
retains the original last-good envelope only with `retained_quality=Stale`.
Neither its calculation time nor its original snapshot changes. Restart restores
this retention in both graph and result state. Required invalid input never
becomes zero, par, Empty-success or a current timestamp on an old value.

## Bounds and local recovery

| Resource | Bound |
| --- | --- |
| Runtime universe | 256 instruments, one exact configuration |
| Captured command queue | 128; explicit capacity error |
| Retained accepted trace | 100,000 normalized events |
| Checkpoint | 32 MiB; one atomic image/outbox, schema 1 |
| Results and output batch | 256 records each |
| Local checkpoint namespaces | 64 |
| Local result archive | 256 MiB, 100,000 delivery keys, 512 KiB per record |
| CPU jobs | 1 through 64 per budget, default 4 |
| Graph | Existing node/edge/depth/frontier/fanout/job limits |

No automatic archival, compaction, counter reset or durable event loss occurs when
a bound is reached. Plan the retention window and stop/reconfigure explicitly.
The replay implementation rebuilds snapshots from the retained bounded trace;
per-event work can grow with that trace. This is an administrative/replay runtime,
not a certified allocation-free hot loop or an event-throughput certificate.

The additive `pricing_runtime_v1` redb table contains one CAS checkpoint per runtime.
It is covered by the same database backup as authoritative inputs. Immutable
snapshot files and result archives must be retained/backed up with that database.
The application schema remains version 1; checkpoint records have their own schema.
Older code does not process the new checkpoint/outbox and must not run its old
pricing lifecycle against this state. Keep a closed pre-upgrade backup for rollback;
forward recovery uses the same plan and verified assets. A changed plan digest,
corrupt checkpoint, missing last-good snapshot or conflicting replay prefix refuses
readiness instead of resetting history.

File replay restarts by verifying every accepted prefix record against the reopened
source, then continuing at the next record. Changed/truncated prefix data fails.
Local redb CAS and process-local graph locking do not implement distributed leases,
egress fencing, shared storage or protection from disk/host/site loss.

## Server and client access

The server provides `/live` and the compatible `/health` liveness response.
`/ready` returns 200 only for ready analytics and 503 otherwise.
`/api/v1/runtime` exposes phase, mode, quality, snapshot/generation, durable revision,
undelivered count, replay completion and failure classification.
`/api/v1/runtime/results` returns canonical result state for display resynchronization.
`POST /api/v1/runtime/valuation` takes a verified snapshot, complete context,
request identity and explicit calculation time, and uses the same bounded
quantitative path as runtime workers. It rejects a different configured build;
capacity is HTTP 429. This additive endpoint does not replace the planned v2 API.

WebSocket clients explicitly opt in with
`{"type":"subscribe_valuations","instrument_ids":["BOND-001"]}`. A `valuation`
message carries the canonical record and durable delivery key. Existing bond/ETF/
portfolio message contracts remain compatible. Lag returns `RESYNC_REQUIRED`;
read current results and deduplicate by delivery key. The display channel is
transient; it does not guarantee durable catch-up. Joined shutdown closes sessions.

The executable supports an explicit loopback-only `replay` profile. Supply a
complete legitimate plan and previously imported authoritative references/configuration:

```sh
CONVEX_PROFILE=replay CONVEX_HOST=127.0.0.1 CONVEX_PORT=8080 CONVEX_ACCESS_MODE=local_development \
CONVEX_STORAGE_PATH=/absolute/path/inputs.redb \
CONVEX_RUNTIME_PLAN=/absolute/path/runtime.json \
CONVEX_QUOTES_FILE=/absolute/path/quotes.jsonl \
cargo run --locked -p convex-server
```

`quotes_file` is canonical JSONL in this profile, not an unsequenced CSV. Snapshot
files use `<storage_path>.snapshots`; output uses `<storage_path>.results.jsonl`.
The plan must select `mode=replay`, empty `seed_events` and this executable's build
identity. Compile a release with `CONVEX_BUILD_ID` set to the pinned artifact ID;
it is a compile-time variable, not a server runtime setting. The default local
build ID is `convex-server-0.13.0-local-unverified`. SIGINT/SIGTERM joins and drains.
Empty providers still require explicit demo mode. Unconfigured production and
non-loopback HTTP upstreams remain rejected. [The access guide](../convex-server/ACCESS.md)
documents required identity verification, role/data/book scopes, TLS proxy ingress
and dependency/strategy readiness. This command explicitly opts into local
development. v1 synthetic-curve behavior remains an enforced G04 failure.

Run the actual lifecycle fixtures with:

```sh
cargo test --locked -p convex-engine --test runtime
cargo test --locked -p convex-server --test runtime_integration_tests -- --test-threads=1
```

The shared fictional setup is in `tests/support/runtime_setup.rs`; its supplied
curves, issuer/resources and quote events are test data, never startup defaults.
The server suite runs a separate process, sends SIGTERM, reopens redb and verifies
unchanged published state. macOS test sandboxes may require temporary loopback access.

## Migration and remaining work

Remove `ReactiveEngine`, its builder/factory, `process_dirty_nodes` and the unused
engine `with_output` bulk-publisher dependency. Use `with_runtime`, `start`,
`submit_update`, canonical result subscription and awaited `shutdown`.
`publish_node_update` replaces the old unjoined WebSocket bridge for explicit graph
notifications. Standalone scheduler/listener utilities are outside this lifecycle;
the runtime does not start their tasks or support additional timer policies.

Pricing mathematics, units, numerical tolerances and existing DTOs are unchanged.
The only dependency-list addition is the already locked `async-trait` package for
fault adapters in engine tests. Configuration activation and expiry now use the
shared control use cases and retained lineage. Complete calibration/products/risk,
durable managed publication, distributed ownership, v2 API/security, full operational
runbooks, dedicated CPU isolation and measured latency remain separate work.
No LB0/LB1 or R0/R1/R2/R3 certificate is complete.
