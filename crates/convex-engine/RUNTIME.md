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
requested measures. The implemented measures are the price, yield, yield-risk,
spread, curve-risk and key-rate measures listed in `SNAPSHOTS.md`
(`RUNTIME_MEASURES`), each at most once per plan, for reference-admitted fixed
bullets and zeros, the OAS measures for at most 8 fixed callables with explicit
call terms, and the discount-margin measures for uncapped floating rate notes with a
rate index. Each instrument requests the plan measures its own model publishes. A
plan pins no fixing keys: each snapshot derives a note's due fixings from its contract
and reads them from the durable fixing store the server's
`POST /api/v1/analytics/fixings` writes. Unsupported measures, products and
configurations fail explicitly. A missing, stale or invalid required curve blocks the
results that read it, even when an observed clean mark would otherwise be available.
No spread, risk, price or curve is synthesized to fill a missing dependency.

The complete manifest supplies each bond's dependencies. Every bond in a batch
consumes the same captured snapshot and configuration, but each result is judged
only on the inputs its own valuation reads: its selected quotes, a floating rate
note's fixings on its index and fixing source, and the configured curves it reads. A
callable reads the discount curve, a floating rate note the discount and FRN
reference curves, and another bond the discount and swap curves only when it asks for
a spread or curve-risk measure. No runtime valuation reads the government curve.
Source skew is measured among one bond's own quotes. A stale or
missing input fails only the bonds that read it. The snapshot keeps its own
snapshot-wide quality for its identity, configuration fixture checks and the hedge
execution gate. Each snapshot is verified once, when it is made (D67),
its curves are built once, at graph activation, and every bond's valuation reads that
build. Graph mutation, mutable
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
| Captured | Caller supplies normalized events and both cutoffs | At least one complete result on valid captured inputs |
| Replay | Pull source such as `FileQuoteReplay`, with original event order/time | At least one complete result on valid historical inputs; EOF sets `replay_complete` |
| Live | `RuntimeInput::Live` quote source subscription, such as the server's HTTP ingress | At least one complete result on valid inputs from this process, also after a restart that follows a delivery loss |
| Idle demo | Explicit `with_demo_mode`, no numerical runtime | Demo quality; readiness stays false |

Every mode also needs the accepted configuration activation, acknowledged output
and no pending work. One quiet or stale bond does not close readiness: it publishes
its own failed result, and health counts it in `failed_instruments`. Health
`quality` is `Stale` once a data deadline has passed, else the first refused result
quality, else `Demo` if any result is `Demo`, else `Valid`.

`Live` is a provider integration point. Its broadcast transport reports lag and
closure; these stop acceptance, publish a failed/stale state when storage/output
permit it, and unsubscribe. Restart restores dependencies, and each bond fails until
its source supplies a new image. A restart is a fresh start, also after a delivery
loss: the stored Live image never holds the loss, so one accepted result makes the
runtime ready, as at any start (D71). A deadline wakeup revalues expired inputs without
a periodic calculation poll. It is armed at the earliest expiry among the results
whose quality is accepted, so a bond that is already stale never re-runs the tick.
Live records include `live_valid_until`, derived from that bond's own dependency
times. Consumers must respect that deadline, snapshot
quality and their own product freshness rules. Analytics readiness never grants
strategy or execution permission.

Live market data is not stored. The actor ingests each event once into quote
state held in memory and resolves each snapshot from it without publishing the
snapshot. A result's `snapshot_id` therefore names content that cannot be read
back, and an intraday live valuation cannot be reproduced later. Each tick is a
new snapshot, and the actor's result cache keeps only the current snapshot's
contexts (`SNAPSHOTS.md`), so a long-running runtime never fills it. Live plans take
no seed events. The checkpoint holds no events, results or outbox. Storage is
written when the runtime first starts, when result revisions or generations pass
the stored ceiling (each write reserves 2^20 more), and when the accepted
activation changes. A restart recalculates from no quotes above the stored
ceilings, which writes one new reservation. Until new images arrive, every
instrument publishes a failed result with no retained last-good value.

Live ticks conflate quotes. After the actor takes one quote, it also admits the
quotes already queued behind it, up to 1,024 per tick in all (the HTTP ingress
queue's capacity, so a full queue fits in one tick), then assembles and values
once. Each quote is still validated and admitted in order and advances the
generation and cutoff, so a tick's results carry the generation of its newest
quote. A quote superseded within one tick gets no result of its own. Under load a
quote waits for at most the running tick and its own, not for every queued tick
before it. Replay and Captured value every event.

A Live tick values only the instruments whose inputs changed since the tick before
(D55). An instrument is valued when its reference, its quotes, a floating note's
fixings on its index and source, a curve its valuation reads, or its derived
quality differs, or when the configuration, its applied overrides, the skew policy
or the UTC date of `as_of` does. Quality depends on time, so a quote that ages out
counts as a change. Every other instrument keeps its record exactly as valued: its
revision, generation, snapshot, valuation time, input versions and
`live_valid_until` (D60). A kept record is not published again, so the stream
carries only changed instruments, and `/api/v1/runtime/results` can hold records
from earlier ticks than the generation and snapshot health reports. The first
tick after start and after an activation refresh values every instrument. Replay
and Captured store their checkpoints, whose results all share one generation and
snapshot, so they value every instrument at each event (D59).

Records are shared, not copied. A tick that values some instruments copies pointers
to the others' records, and a tick that values none keeps the results it has. The
curves each instrument's valuation reads are worked out once, at the first tick of
an activation or of a new frame, so a tick builds a valuation context only for the
instruments it values. Each context derives its cache key once, when it is made.

A Live runtime resolves its pricing configuration and pinned references, with
their issuer, calendar, convention and rate-index records, once per accepted
activation, at the first snapshot that needs them, and reuses them at every tick.
All are exact pinned versions, so none changes while that selection holds, and each
snapshot still checks them at its own cutoff. Every tick's snapshot shares one copy of
the references, written as JSON once and checked once apart from the cutoff (D68).
Quotes, fixings and stored curve revisions are read at every snapshot's cutoff. A Live quote does not read the active
configuration: the runtime keeps the selection it accepted until the engine's
activation refresh or a restart applies another. An activation committed without
the engine therefore waits for one of those. Captured and Replay read the active
selection at each event.

A Live plan that pins no curve data reads its required curves from the engine's
curve store each time it builds a snapshot, at that snapshot's cutoff. A re-marked
curve therefore reaches the stream at the next quote, or at the deadline when the
previous revision expires. A stale revision is still used and makes the snapshot
`Stale`. An incomplete or oversized curve graph makes it `Invalid` (curve missing). Only a
storage failure stops the runtime. Each result lists the curve revisions it
consumed in `provenance.input_versions`.

## Construction and lifecycle

Use `PricingEngineBuilder::with_runtime(RuntimeServices)` to supply a checked plan,
immutable snapshot/fixing repositories, `RuntimeStore`, `ResultPublisher` and input
ownership. Reference/configuration reads use the engine's authoritative storage.
Add `with_configuration_use_cases` with actual composition capabilities to adopt
managed selections. An active pointer without that composition fails startup.
`open_redb_storage` returns those repositories, the checkpoint store,
`RedbSnapshotRepository` and `RedbResultPublisher` over one local redb database on
Linux and Windows; the server composes these. `FileSnapshotRepository` and
`JsonlResultPublisher` remain local Unix file adapters. Alternate backends implement
the same narrow ports without importing the engine or changing pricing mathematics.

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
at once, with default 4 and valid range 1 through 64. Each finished job is
replaced at once, rather than after the slowest of a batch; result revisions still
follow plan order, never completion order. Interactive
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

`RedbResultPublisher` commits each result in its own transaction, accepts identical
redelivery of a key and rejects changed content. `JsonlResultPublisher` acknowledges
after file sync, reconciles identical delivery keys and rejects changed content. Reopening validates complete bounded records.
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
| Retained accepted trace | 100,000 normalized events; Live retains none |
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
In memory a checkpoint shares its results and outbox, so copying it costs the same
at any width; they are stored as plain arrays. Older code does not process the new checkpoint/outbox and must not run its old
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
message carries the canonical record and durable delivery key. It is the only
WebSocket data message: REST calculators do not broadcast, and the bond, ETF and
portfolio subscriptions are retired. Lag returns `RESYNC_REQUIRED`;
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

`quotes_file` is canonical JSONL in this profile, not an unsequenced CSV. Market
snapshots and published results are stored in the `storage_path` redb database.
The plan must select `mode=replay`, empty `seed_events` and this executable's build
identity. Compile a release with `CONVEX_BUILD_ID` set to the pinned artifact ID;
it is a compile-time variable, not a server runtime setting. The default local
build ID is `convex-server-0.13.0-local-unverified`. SIGINT/SIGTERM joins and drains.
Empty providers still require explicit demo mode. Unconfigured production and
non-loopback HTTP upstreams remain rejected. [The access guide](../convex-server/ACCESS.md)
documents required identity verification, role/data/book scopes, TLS proxy ingress
and dependency/strategy readiness. This command explicitly opts into local
development. v1 synthetic-curve behavior remains an enforced G04 failure.

The loopback-only `live` profile runs a plan with `mode=live` and no `quotes_file`.
The plan comes from exactly one of two places:
- `CONVEX_RUNTIME_PLAN`: a plan file.
- `CONVEX_CONFIGURATION_ID`: the stored active configuration with that id, compiled
  from books and stored inputs with `convex-config compile`
  ([configuration guide](CONFIGURATION.md#compile-a-live-configuration-from-books)).
  The stored assembly does not change, so each restart plans the same digest.

Quotes arrive only over HTTP:

```sh
CONVEX_PROFILE=live CONVEX_HOST=127.0.0.1 CONVEX_PORT=8080 CONVEX_ACCESS_MODE=local_development \
CONVEX_STORAGE_PATH=/absolute/path/inputs.redb \
CONVEX_CONFIGURATION_ID=SHOWCASE-USD \
cargo run --locked -p convex-server
```

A new Live checkpoint starts at the current time, after the plan cutoff and the
activation that selected it. Live cutoffs never move backwards: if the wall clock
steps back, ticks are valued at the last cutoff until the clock catches up. Pinned
curve recipes age from their recording time: once a recipe passes its `max_age_ms`
(at most one day), valuations report the curve as stale until a configuration with
fresh recipes is compiled and activated.

The `demo` profile can also run a stored configuration live with
`CONVEX_CONFIGURATION_ID`. Its composition is an explicit demonstration: every
response and WebSocket message is labelled demo, so inputs of `Demo` quality are
valued, results keep `Demo` quality, and readiness accepts them. Every other
composition values and serves only `Valid` inputs. The showcase container runs this
way ([deploy/showcase](../../deploy/showcase/README.md)).

`POST /api/v1/market/quotes` takes `{"events": [QuoteEvent, ...]}` with 1 to 256
canonical events. The server replaces each event's `receive_time_ms` with its own
receipt time, stamped and queued under one lock so delivery order is receipt order.
Receipt times never step back. While the wall clock is behind the last stamp by at
most 5 s, a batch is stamped with the last stamp, so its quotes can read up to 5 s
ahead of the clock. A larger step refuses every batch with 503 until the clock
catches up. A step is logged once, at warn. `FUTURE_SOURCE_TIME` compares with the
stamp (D70). A batch is queued whole or not at all:

| Status | Meaning |
| --- | --- |
| 202 | Queued for the runtime, not yet priced. Body: `accepted`, `receive_time_ms` |
| 400 | Empty batch, more than 256 events, or a body that is not a JSON quote batch (malformed JSON, wrong shape, unknown field, or not `application/json`) |
| 422 `QUOTES_REJECTED` | Per-row `INVALID_EVENT`, `UNKNOWN_KEY` (not a plan instrument/source) or `FUTURE_SOURCE_TIME` |
| 429 | The runtime's 1,024-event queue cannot take the batch; retry |
| 503 | No live runtime is composed or subscribed, or the server clock moved backwards by more than 5 s |

The ingress refuses anything the Live runtime would not accept, because the runtime
stops on such an event. Results reach WebSocket subscribers and
`/api/v1/runtime/results`; they are not archived, and no snapshot is stored. The
route requires `reference_writer` with market data and each event's instrument and
source grants. The public showcase does not expose it.

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
Graph notifications are not streamed; the WebSocket carries runtime results only.
Standalone scheduler/listener utilities are outside this lifecycle;
the runtime does not start their tasks or support additional timer policies.

Pricing mathematics, units, numerical tolerances and existing DTOs are unchanged.
The only dependency-list addition is the already locked `async-trait` package for
fault adapters in engine tests. Configuration activation and expiry now use the
shared control use cases and retained lineage. Complete calibration/products/risk,
durable managed publication, distributed ownership, v2 API/security, full operational
runbooks, dedicated CPU isolation and measured latency remain separate work.
No LB0/LB1 or R0/R1/R2/R3 certificate is complete.
