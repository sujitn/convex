# Curve recipes in immutable snapshots

`SnapshotAssembly.curve_recipes` stores the complete `CurveRecipe` JSON of each
curve, together with its source, recording time, model, quality, freshness and the
definition revisions it consumed. `convex_engine::curve_replay` decodes that
payload and passes the whole dependency graph to
`convex_curves::contracts::build_curve_set`. The curve mathematics and the shared
library API do not depend on providers, storage, message queues, a runtime or
clocks.

The ports crate stores a bounded, versioned JSON payload so that it does not
depend on the pricing crates. When a port reads a record it checks the structure,
identity, time and quality policies. The engine's strict decoder and the curve
builder then validate the financial content before assembly I/O, configuration
validation, graph activation and valuation. Reading a valid hash from storage does
not by itself validate the model.

The operator configuration schema describes the optional recipe list and its
capture envelope. The financial payload is the `CurveRecipe` schema from the
library; the ports crate does not duplicate financial types or rules.
`tests/scripts/curve_replay_schema.py` checks both exported schemas. The offline
example exports them with `--emit-schema` (the operator envelope) and
`--emit-configuration` (a fictional complete recipe document). Checks across
records, resource limits, time and financial content still need the engine.

Every source type is preserved:

- DF, zero, survival and basis nodes;
- dated deposit and FRA helpers;
- parametric parameters and observations;
- segmented and delegated models, MonotoneConvex, step zeros, FlatForward and
  UFR/LLP;
- spread, equivalence, projection and roll dependencies.

Units, source order, dates, time basis, roles, currency, representations,
compounding, numerical settings, methods and tail policies are kept as given.
Unsupported combinations fail with the same typed build errors as a direct build;
no sampled model or fitted-node substitute is installed. Calibrations from full
products, and quote-risk graphs, use the separate
[schema-3 capture and replay contract](CALIBRATION-SNAPSHOT-REPLAY.md). Keeping
helper IDs and settings does not recreate missing instruments, calendars or
published history.

## Wire and identity

A record has the following shape; the `recipe` is the complete native contract:

```json
{
  "schema_version": 1,
  "curve_id": "USD.FUNDING",
  "version": 7,
  "source_time": "2026-10-02T15:00:00Z",
  "recorded_at": "2026-10-02T15:00:00Z",
  "source": "supplied-source",
  "model_id": "supplied-native-model-v1",
  "input_versions": [],
  "quality": "Valid",
  "max_age_ms": 10000,
  "recipe": {
    "key": {"id": "USD.FUNDING", "version": "original-definition.v7"},
    "role": {"kind": "Discount", "collateral_currency": "USD"},
    "currency": "USD",
    "reference_date": "2026-10-02",
    "time_basis": "Act360",
    "representation": {"ZeroRates": {"compounding": "Continuous"}},
    "interpolation": "FlatForward",
    "extrapolation": "FlatForward",
    "dependencies": [],
    "calibration": "Direct",
    "source": {"FlatForward": {"nodes": [
      {"pillar": {"Date": "2027-10-02"}, "quote": {"unit": "PercentRate", "value": 2.3}},
      {"pillar": {"Date": "2028-10-02"}, "quote": {"unit": "DecimalRate", "value": 0.028}}
    ]}}
  }
}
```

The record's positive numeric `version` is an immutable capture revision that the
caller supplies. The key's string `version` is the definition revision. Neither is
invented, parsed from the other or substituted on replay. The record ID must match
the key. Both revisions and all payload content affect the snapshot identity, and
results keep the numeric record and definition lineage. Conflicting definition
revisions fail instead of being dropped.

| Snapshot content | Required schema | Hash domain |
|---|---|---|
| `curves` only, with no complete recipes | 1 | `convex.snapshot.v1\0` |
| One or more `curve_recipes`, optionally alongside separate zero records | 2 | `convex.snapshot.v2\0` |
| Either, as assembly writes every new snapshot | 4 | `convex.snapshot.v4\0`, by section ([identity](../crates/convex-engine/SNAPSHOTS.md#identity-quality-and-bounds)) |

An empty `curve_recipes` is omitted, so schema-1 canonical bytes and hashes are
unchanged. Assembly writes schema 4 whatever the curves; schemas 1 and 2 still
verify. Direct content construction must declare a version whose rules the content
meets; inconsistent or unknown versions fail. The configuration and runtime request
formats accept the omitted optional field. New records change the content digest
and go through normal configuration staging and activation. No old identity is
overwritten or relabelled.

All records and dependencies must appear in `required_curves`. IDs cannot collide
between the zero-record list and the complete-record list. Dependencies of a full
recipe resolve only to other complete records at their exact versions; an older
zero record cannot supply a missing full recipe. A missing record, a cycle or a
version mismatch returns no curve set. Record order is canonicalized; recipe
arrays and source values are untouched.

## Application behavior and bounds

The assembler, configuration dry-run, staging and activation, the runtime graph,
the file snapshot repository, result provenance and recovery all use these
records. Freshness and derived quality include every required dependency. Runtime
expiry includes the recipe source deadlines. Restart verifies the same snapshot
and configuration digests and queries no current curve or provider.

The fixed-bullet observed-mid valuation can use complete rate recipes through a
thin `RateCurveDyn` bridge. Discount selection requires the exact
Discount/collateral/currency role; a swap benchmark requires an explicit
Projection role and currency. Dates use the curve's time basis. The bridge passes
DF, zero and period-forward queries to the curve code. At zero, an equivalent zero
rate stays undefined. Model-specific instantaneous forwards still need their own
query and side contracts; no small-period derivative is substituted. `max_date`
uses the inverse time conversion with explicit `AtOrBefore`; the model-time
horizon is still available.

The runtime endpoint keeps its response contract: a rejected numerical input can
return HTTP 200 with a complete `Failed` measure envelope, zero successful values
and explicit errors. It does not produce a successful price or change readiness
because of a numerical failure.

Snapshots are bounded to 16 MiB, 64 records in total, and 4,096 nodes,
observations, helpers and segments in total across both record lists. Each payload
has at most 32,768 JSON values, 64 nested levels, 4,096 collection entries and
8,192 bytes per text or key. The 1,024-node, 150-year, dependency-depth, model,
helper and solver limits of the curve code also apply. Result provenance keeps its
256-record bound. Rebuilding is setup work with bounded allocation; it is not
measured for throughput or latency.

## Migration and offline replay

Older `SnapshotCurve` records remain ACT/365F continuous-zero records. Selecting
`FlatForward` on them still fails. To migrate, supply the complete source with the
correct units, time basis, model and revisions, and publish a new snapshot that
holds the recipe. Do not guess whether an old policy meant raw flat zeros or
integrated forwards. Old MonotoneConvex records with a negative interval are still
rejected.

`BuiltCurve::rebuild_inner` now returns a `Result`. An invalid rebuild clears the
representation of the changed object, and builders fail before installing it in
the cache. The previous cache entry keeps its identity. A failed rebuild never
attaches an old model to new points or falls back to linear or zero values.
The older day/zero graph conversion can fail: fractional pillars and complete
recipes must go through immutable snapshot publication. Full recipe curves expose
no `points` grid. Integer-day zero inputs that were already compatible use the
same route as before. The older raw-value and convenience paths are not covered by
recipe replay.

`POST /api/v1/curves` stores one complete `SnapshotCurveRecipe` revision in
`CurveStore`, the same write as `POST /api/v1/analytics/curves`. The record is
validated before storage; a different record under a stored version conflicts.
`GET /api/v1/curves/{id}` and its zero, discount and forward queries read the
revision current at the supplied `known_at` (the current time when omitted) and report it in
`x-convex-curve-version`.

Run the offline consumer in a directory whose parent already exists:

```sh
cargo run --locked -p convex-engine --example curve_snapshot_replay -- demo /tmp/convex-curve-snapshots
```

Use the returned snapshot ID with the existing administration example:

```sh
cargo run --locked -p convex-engine --example snapshot_replay -- verify /tmp/convex-curve-snapshots SNAPSHOT_ID
cargo run --locked -p convex-engine --example snapshot_replay -- query-curves /tmp/convex-curve-snapshots SNAPSHOT_ID QUERIES_JSON
```

`QUERIES_JSON` contains 1–4,096 `{ "curve": { "id", "version" }, "query": ... }`
records using the `CurveQuery` schema. Queries keep the full side, compounding and
date-period inputs. A failed query or an exact-version failure returns no partial
batch. `verify` rebuilds the curves as well as checking integrity. The demo uses
fictional fixtures and is not market data.

The replay pack copies 2,563 requests unchanged: 2,208 successes and 355
failures in the curve code, input handling or queries. After a roundtrip it
compares the recipes, diagnostics and query values. The financial controls are
separate from this transport comparison. Tests of file reopen, configuration,
runtime restart and the server envelope add to the schema-1 capture taken before
the change.

Not yet covered: snapshots of full products and quote risk, the remaining library
operations, and testing on Windows, Excel and in a browser. This work is partial
and not certified, and no throughput, hot-loop allocation or latency guarantee
is claimed.
