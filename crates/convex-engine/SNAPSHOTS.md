# Immutable valuation snapshots

`SnapshotAssembler` captures resolved input values and publishes a verified,
immutable information set. `SnapshotValuator` prices that information set using a
complete supplied context and a bounded result cache. These are application use
cases in `convex-engine`; database, file and feed implementations remain behind
ports. They create no background task or additional runtime.

The current implementation supports reference-admitted fixed bullet bonds and
observed mid clean marks. The [calculation graph](GRAPH.md) now admits complete
snapshot/context tickets and commits only their current generation. The [pricing runtime](RUNTIME.md) now adopts this path for authoritative fixed-bullet
valuation, canonical publication and local recovery. Its additive supplied-context
server endpoint is equivalent to runtime valuation; earlier v1 pricing endpoints
retain their separate compatibility behavior and unresolved synthetic-curve gap.

## Capture and time

The [snapshot port](../convex-ports/src/snapshot.rs) retains the following values:

| Input | Captured content |
|---|---|
| Reference | Exact instrument revision and resolved issuer, calendar and convention revisions |
| Configuration | Exact pricing revision/values; optional retained activation sequence/digest and actually applied policy exceptions |
| Quotes | Explicit source/field requirements, retained field observations, accepted data watermark and latest state-affecting delivery watermark |
| Fixings | Required source/index/date and the selected published revision, or explicit absence |
| Curves | Required identities; original explicit zero records or complete native recipe records with actual dates, roles, units, model/helper/LLP/settings and exact dependencies |
| Policy | Positive input generation, economic `as_of`, information `known_at`, field/curve age bounds and selected-source skew bound |

Assembly resolves exact repository versions. A missing version never becomes a
lookup of the current version. Reference resources must satisfy both effective
time and information time under the [reference master contract](../convex-ports/REFERENCE_MASTER.md).
Configuration update time cannot exceed `known_at`.
The [configuration lifecycle](CONFIGURATION.md) freezes a coherent active selection
and scoped exceptions before work is admitted. Engine admission verifies a supplied
activation/policy claim against retained immutable history. New selections never
change an issued snapshot or an admitted context. Older explicit-pin snapshots
omit this optional capture and keep their original canonical hashes.

Replay accepts at most 100,000 validated quote events in nondecreasing recorded
receipt order. Only explicitly selected keys enter the snapshot. Source time must
be at or before `as_of`, and receipt time at or before `known_at`. Image, delta,
clear, reset, gap and closure semantics reuse `MarketDataAssembler`. A skipped
sequence can leave delivery sequence 3 beside accepted data sequence 1; storing
just the last accepted sequence would hide the loss. Duplicate and out-of-order
deliveries do not refresh the retained observations or state watermark.

UTC context and fixing times preserve nanoseconds. Quote schema 1 observations
carry whole UTC milliseconds; no finer source precision is invented. Freshness
uses `max(as_of, known_at)` at capture and the supplied decision time for execution
data admission. An observation aged exactly the configured limit is eligible;
one nanosecond beyond it is stale. Reading a cache never renews its age.

## Identity, quality and bounds

Snapshot identity is `sha256:<64 lowercase hexadecimal digits>` over canonical
schema-1 content with domain `convex.snapshot.v1\0`, or complete recipe schema-2
content with domain `convex.snapshot.v2\0`. Original schema-1 bytes remain exact.
See [complete recipe replay](../../docs/CURVE-SNAPSHOT-REPLAY.md). Unordered manifests are
normalized; duplicates, conflicting resource revisions, unknown fields and
out-of-scope inputs fail validation. Deserialization recomputes content identity
and derived quality. The file adapter additionally requires exact canonical bytes.
This verifies integrity; it does not authenticate a provider or licensing decision.

Each snapshot is bounded to 16 MiB, 256 instruments/selected quote keys, 1,024
fixings, 64 total original/complete curves and **4,096 total original pillars,
observations, helpers or segments**. Each quote has at most 12
fields. Quote and curve age budgets are explicit, between 1 ms and one day;
source skew is between 0 ms and one day. Product owners must choose their actual
budgets; these bounds do not certify a trading latency target.

`SnapshotQuality` separates required-input completeness from conservative data
quality and lists stable issues. Missing or cleared fields/fixings/curves are
explicit. Gap, invalid, closed, stale, delayed, indicative/manual, preliminary,
unentitled, demo and source-skew states cannot become `Valid`. A gapped snapshot
can retain all required values and still have invalid quality.

`require_execution_quality_at(decision_time)` is a necessary data condition for
an executable hedge. It refuses incomplete/nonvalid snapshots, decisions before
the context and observations that have since become stale. It grants no strategy,
risk, ownership, order or venue permission. A captured override also refuses a
decision at or after its exclusive expiry, even if its quote would otherwise meet
the extended age limit. Research/display valuation may return
retained values with their invalid quality; `Complete` means the requested
measures calculated successfully, not that a hedge is authorized.

## Complete contexts and result caching

[ValuationContext](src/context.rs) requires every compatibility dimension:
snapshot ID, captured generation, exact instrument/trade subject revision,
valuation time, `known_at`, settlement date, exact configuration, model, executable
build, reporting currency, fixing cutoff, scenario, numerical policy/bump and a
nonempty unique requested measure set. Measure order is normalized. Binding
requires the exact snapshot generation, subject/configuration revisions and time
cutoffs before any cache read.

The composition root supplies an actual immutable build identity. Reusing a build
label after changing mathematics invalidates this contract. The context key is
SHA-256 with domain `convex.valuation-context.v1\0`; results retain the full
context and generation alongside the key. Request correlation and calculation
completion time do not change compatibility. Identical retries and concurrent
misses keep the first retained result and its original calculation time; only
request correlation is rebound. Conflicting values or lineage fail explicitly.

`ValuationCache` has an explicit 1..=100,000 entry bound. A full cache returns
`CapacityExceeded`; it performs no arbitrary eviction. `clear_cache` is an
administrative release of retained contexts and leaves issued values immutable.
This process-local memoization is neither a durable result journal nor shared
ownership across servers. Every successful measure or per-measure error must
account for exactly one requested measure.

The first model is `convex-fixed-bullet-router-v1`, with compiled numerical policy
`convex-bond-router-v1`, base scenario, explicit settlement and the bond's currency.
Only `Auto` or `MarketQuote` with `Mid` selection is admitted, using exactly one
declared source with required `mid_price`. No bid/ask midpoint, FX conversion,
trade economics, unsupported scenario/model or missing field is substituted.

| Measure | Canonical quantity |
|---|---|
| `clean_price_mid` | Exact observed price per 100 |
| `dirty_price_mid` | Finite model price per 100 |
| `accrued_interest` | Finite model price per 100 |
| `ytm_mid` | Finite model decimal rate |
| `modified_duration_mid` | Finite model years |
| `convexity_mid` | Finite model years squared |
| `dv01_mid` | Finite model price per 100: the loss for +1 bp of the bond's own quoted yield, not factor risk |
| `z_spread_mid` | Finite model basis points over the configuration's discount curve |
| `i_spread_mid` | Finite model basis points: the yield over the configuration's swap curve |
| `asw_mid` | Finite model basis points: par-par asset swap over the swap curve |
| `macaulay_duration_mid` | Finite model years |
| `effective_duration_mid` | Finite model years: parallel continuous-zero shift of the discount curve at the solved, frozen Z-spread |
| `effective_convexity_mid` | Finite model years squared, from the same shift |
| `spread_duration_mid` | Finite model years: Z-spread shift with the discount curve held |
| `pv01_mid` | Finite model price per 100: the loss for +1 bp in continuous zero rates at the frozen Z-spread |
| `cs01_mid` | Finite model price per 100: the loss for +1 bp in the Z-spread |
| `krd_3m_mid` … `krd_30y_mid` | Finite model years: triangular key-rate zero durations at 3M, 6M, 1Y, 2Y, 3Y, 5Y, 7Y, 10Y, 20Y and 30Y |

The spread measures need the configuration's swap curve role; without it they fail
with the router's `MissingMarketData` reason. The curve-risk measures (effective
duration and convexity, spread duration, PV01, CS01 and the key rates) share one
router calculation: when it fails, each of them carries that calculation's code and
reason. A zero has no spread or curve-risk convention in this contract, so those
measures fail with `UnsupportedConvention` while its price, yield and yield-risk
measures publish.

Calculated values remain model quantities. There is no hidden cash/ledger rounding
to fit an exact decimal coefficient. Missing marks or unavailable numerical
calculations produce explicit measure errors; a measure the router could not
produce keeps the router's own error code and reason. Unsupported measures produce
`InvalidInput`. Existing native formulas, solver limits, tolerances, v1 economic
units and shared JSON/FFI contracts are unchanged.

## Supplied curves and fixing history

`SnapshotCurve` represents supplied continuously compounded zero rates on
ACT/365F year fractions. It preserves fractional pillars and an explicit reference
date, with linear or monotone-convex interpolation and an explicit flat-zero
extension. Its older unsupported flat-forward selection rejects. At least two
strictly increasing positive finite tenors and finite rates are required;
signed rates require an admissible explicitly selected model. `BuiltCurve::from_snapshot`
reconstructs these values without reading today's date or inferring a policy.
`SnapshotCurveRecipe` retains every original native source, date, unit, role,
currency, time basis, model, method/tail, observation/settings and dependency
version. The engine reconstructs the entire checked native graph before publication
or use. `BuiltCurve::from_recipe` delegates actual date/DF/zero/forward semantics
without resampling; full method/side queries remain available. Original full
product-calibration graph snapshots and certificates remain separate owning work.

The new [fixing repository](../convex-ports/src/fixing.rs) requires separate actual
publication and recording timestamps. `JsonFixingRepository` freezes a strict
regular JSON file of at most 16 MiB and 100,000 revisions. Both timestamps must be
at or before `known_at`; the highest eligible revision wins. Duplicate revisions
and reversed revision recording times fail. Reopen a new repository to capture
an appended history. No prior-date fixing or forecast is substituted.

```json
{
  "schema_version": 1,
  "fixings": [{
    "schema_version": 1,
    "key": {"index_id": "USD-SOFR", "observation_date": "2026-10-01", "source": "administrator"},
    "version": 1,
    "rate": {"value": "0.041", "unit": "decimal_rate"},
    "published_at": "2026-10-02T12:00:00Z",
    "recorded_at": "2026-10-02T12:00:01Z",
    "is_preliminary": false,
    "entitled": true,
    "evidence_reference": "fixture:published-fixing"
  }]
}
```

The earlier fixing CSV source is removed. Supply verified publication and
recording metadata to use historical assembly.

JSON model values use Serde's `float_roundtrip` parsing to preserve finite binary
values on reopen. Snapshot configuration encoding also preserves exact decimal
strings, including `RealYield.index_ratio`, under workspace feature unification.
Changing schema or canonical encoding requires an explicit identity migration.

## Local persistence and replay

The server stores snapshots with
[RedbSnapshotRepository](../convex-ext-redb/src/snapshot_repository.rs) in its one
redb database, on Linux and Windows. Each publication is one transaction keyed by
content digest; identical retries succeed, changed bytes conflict, and reads
re-verify identity and canonical bytes.

[FileSnapshotRepository](../convex-ext-file/src/snapshot_repository.rs) creates or
opens an explicit real directory on a local Unix filesystem. Its parent must
already exist. It writes a same-directory staging file, synchronizes the file,
publishes by atomic no-clobber hard link and synchronizes the directory before
acknowledging. Identical retries compare retained content; conflicting or corrupt
files are never overwritten. A post-publication sync failure is `UnknownOutcome`:
read the exact ID, reconcile the content, then retry the identical publication.

Reads reject noncanonical bytes, tampering, invalid IDs, oversized/nonregular
files and symlinks at the root or data file. This requires a trusted host directory;
the checks are not race-proof protection against an adversary replacing path
components. Publication has been exercised on local macOS only. Network/shared
filesystems, Windows, host loss, retention/garbage collection, backup/restore and
distributed consensus are not certified. File/redb calls are synchronous
administration paths and belong off latency-sensitive loops.

The offline [snapshot_replay example](examples/snapshot_replay.rs) exposes:

```text
snapshot_replay store-config DATABASE CONFIG_JSON
snapshot_replay assemble DATABASE FIXINGS_JSON QUOTES_JSONL ASSEMBLY_JSON DIRECTORY
snapshot_replay verify DIRECTORY SNAPSHOT_ID
snapshot_replay query-curves DIRECTORY SNAPSHOT_ID QUERIES_JSON
snapshot_replay value DIRECTORY CONTEXT_JSON REQUEST_ID CALCULATED_AT BUILD_ID
```

Run a mode using `cargo run --locked -p convex-engine --example snapshot_replay --`
followed by its arguments. Seed verified references using the existing
`convex-server` `reference_master` example, then store an exact configuration.
Supply [SnapshotAssembly](src/snapshot.rs), fixing history and sequenced quote
files. Use the returned ID in a complete context with matching generation/times
and an actual build identity. `value` reopens only the stored snapshot; it never
queries reference, market or configuration sources. Validate paths before running:
redb opens can create a database, and the snapshot adapter can create its leaf
directory. Keep input files immutable while loading.

The [engine acceptance tests](tests/snapshots.rs),
[port contract tests](../convex-ports/tests/snapshot_contracts.rs) and
[file conformance tests](../convex-ext-file/tests/snapshot_repository.rs) exercise
reopen, cutoff boundaries, cache dimensions, concurrent retries, corruption and
invalid execution data. They do not certify live providers, distributed operation,
calibration accuracy or low-latency execution.

## Rust API migration and next integration

The unused mutable calculation context and quote cache were replaced by the
complete immutable context/result cache. `DataState`, `FieldObservation` and
`QuoteSnapshot` now live in `convex-ports`; the engine ingestion module reexports
the same canonical types. There is no duplicate implementation.

Replace engine `BuiltCurve::from_points` with validated explicit
`BuiltCurve::from_snapshot`. The removed convenience constructor inferred today's
date/default policy and discarded fractional pillars. Native pure curve
constructors remain available. Builder input hashes now include actual input
content instead of the formatted input string's length.

The graph now captures generations before calculation, admits work in dependency
order, propagates structured failures and rejects obsolete commits. Its
[contract and migration guide](GRAPH.md) covers snapshot/context tickets and current
runtime limits. The [authoritative lifecycle](RUNTIME.md) now calls these use cases,
publishes consistent snapshot/context/results, retains stale/error states and
restores accepted work before readiness. The older coordinator is removed. Complete
calibration, synthetic v1 server curves, expanded products and release certificates
remain unresolved production work.


Complete original product graph capture, fitted-output/solver/helper diagnostic
comparison and original quote-risk replay are documented in
[the schema-3 replay contract](../../docs/CALIBRATION-SNAPSHOT-REPLAY.md). Pure native calculations remain provider/storage/MQ/clock neutral.
Existing schema1/2 identities and original numerical controls remain; CVX-063
and all mandatory library/market/model/host/production/latency certificates stay open.
