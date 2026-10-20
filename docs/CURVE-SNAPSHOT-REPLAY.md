# Complete curve recipes in immutable snapshots

`SnapshotAssembly.curve_recipes` captures the complete original native
`CurveRecipe` JSON together with explicit source, recording, model, quality,
freshness and consumed definition revisions. `convex_engine::curve_replay`
decodes that payload and delegates the entire exact dependency graph to
`convex_curves::contracts::build_curve_set`. Pure curve mathematics and the shared
library API remain independent of providers, storage, MQ, runtime and clocks.

Ports retain a bounded versioned JSON payload to avoid depending on pricing
crates. Port deserialization verifies structural integrity, identity, time and
quality policies. The engine's strict native decoder and complete builder perform
financial admission before assembly I/O, configuration validation, graph
activation and valuation. Reading a valid hash from storage alone is not a
native model validation or certificate.

The existing operator configuration schema describes the optional recipe list
and its strict capture envelope. The financial payload remains the original pure
library `CurveRecipe` schema; ports do not duplicate financial types or rules.
Both actual exported schemas are checked by `tests/scripts/curve_replay_schema.py`.
The offline consumer exports them with `--emit-schema` (operator envelope) and
`--emit-configuration` (fictional complete recipe document). Cross-record,
resource, time and financial admission still require the checked engine.

Every original source survives: supplied DF/zero/survival/basis nodes, complete
dated deposit/FRA helpers, parametric parameters and observations, segmented and
delegated models, MonotoneConvex, step zeros, FlatForward, UFR/LLP, and exact
spread/equivalence/projection/roll dependencies. Native units, source order,
actual dates, time basis, roles, currency, representations, compounding, numerical
settings, methods and tail policies remain intact. Unsupported native combinations
fail with their original typed build diagnostics; no sampled model or fitted-node
alias is installed. Full product-helper calibration and quote-risk graphs use the separate complete
[schema-3 capture/replay contract](CALIBRATION-SNAPSHOT-REPLAY.md). Retaining helper IDs/settings
does not reconstruct missing original instruments, calendars or published history.

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

The record's positive numeric `version` is a caller-supplied immutable capture
revision. The native key's string `version` is the original definition revision.
Neither is invented, parsed from the other or substituted on replay. The record
ID must match the native key. Both revisions and all payload content affect the
snapshot identity; results retain original numeric record/definition lineage.
Conflicting consumed definition revisions fail instead of disappearing.

| Snapshot content | Required schema | Hash domain |
|---|---|---|
| Original `curves`, with no complete recipes | 1 | `convex.snapshot.v1\0` |
| One or more `curve_recipes`, optionally alongside separate original zero records | 2 | `convex.snapshot.v2\0` |

Empty `curve_recipes` is omitted so original schema-1 canonical bytes and hashes
survive. Assembly chooses schema 2 when full records are present. Direct content
construction must declare the matching version; inconsistent or unknown versions
fail. Existing configuration/runtime request formats accept the omitted optional
field; new records change their content digest and require normal immutable
configuration staging/activation. No old identity is overwritten or relabelled.

All retained records and dependencies must appear in `required_curves`. IDs
cannot collide across the original and complete record lists. Full native
dependencies resolve only other complete records at their exact native versions;
an older zero record cannot supply a missing full recipe. Missing/cyclic/version
mismatches return no accepted curve set. Record ordering is canonicalized, while
original recipe arrays and source economics remain untouched.

## Application behavior and bounds

The existing assembler, configuration dry-run/staging/activation, runtime graph,
file snapshot repository, result provenance and recovery use these records.
Freshness and derived quality include every retained required dependency.
Runtime expiry includes recipe source deadlines. Restart verifies the same
snapshot and configuration digests; it queries no current curve/provider.

The existing fixed-bullet observed-mid valuation slice can use complete rate
recipes through a thin `RateCurveDyn` bridge. Discount selection requires the
exact Discount/collateral/currency role; a swap benchmark requires an explicit
Projection role and currency. Dates use the original curve time basis. The
bridge delegates DF, zero and period-forward equations to native queries. At
zero, an equivalent zero rate remains undefined. Model-specific instantaneous
forwards still require their native query and side contracts; no small-period
derivative is manufactured. `max_date` uses the original inverse time conversion
with explicit `AtOrBefore`; the native model-time horizon remains available.

The runtime endpoint preserves its existing response contract: a rejected
numerical input can return HTTP 200 with a complete `Failed` measure envelope,
zero successful values and explicit errors. It does not produce a successful
price or change readiness from a numerical failure.

Snapshots remain bounded to 16 MiB, 64 total records and 4,096 total original
nodes/observations/helpers/segments across both record lists. Each payload has
at most 32,768 JSON values, 64 nested levels, 4,096 collection entries and
8,192 bytes per text/key. Original native 1,024-node, 150-year, dependency-depth,
model, helper and solver limits also apply. Result provenance retains its
original 256-record bound. Rebuilding is setup/control work with normal bounded
allocation; no throughput, hot-loop allocation or latency certificate is claimed.

## Migration and offline replay

Older `SnapshotCurve` records remain explicit ACT/365F continuous-zero records.
Their unsupported `FlatForward` selection continues to fail. Migration requires
an actually supplied complete native source, correct original units/time basis,
model and revisions, and a new schema-2 snapshot. It must not guess whether an old
policy meant raw flat zeros or integrated forwards. Original negative-interval
MonotoneConvex records continue to reject.

`BuiltCurve::rebuild_inner` now returns a checked result. Invalid rebuilds clear
the changed object's representation; builders fail before installing it in the
cache. The previous cache entry retains its original identity. No failed rebuild
attaches an old model to new points or falls back to linear/zero values.
The older day/zero graph conversion is fallible: fractional pillars and complete
recipes require immutable snapshot publication. Full recipe curves expose no
fabricated `points` grid. Existing compatible integer-day zero inputs retain
their original route. Other older raw-value/convenience qualification remains
separate from complete recipe replay.

The existing `POST /api/v1/curves` point constructor requires at least two
distinct positive pillars for its checked MonotoneConvex owner. Empty and
single-pillar requests return HTTP 400 before installing a curve. A single
point no longer produces a successful unbuilt object or an implicit flat curve;
callers must supply the original complete source and explicit model they need.

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
records using the original `CurveQuery` schema. Queries preserve full side,
compounding and date-period inputs. A query or exact-version failure returns no
partial query batch. `verify` performs native reconstruction as well as integrity
checks. The demo uses explicitly fictional fixtures; it is not market admission.

The replay pack copies 2,563 original requests unchanged: 2,208 successes and
355 native/ingress/query failures. It compares full original recipes, diagnostics
and native query values after roundtrip. Independent existing financial controls
remain separate from this transport comparison. Actual file reopen, configuration,
runtime restart and server-envelope controls supplement the frozen pre-edit
schema-1 capture. CVX-063 remains partial and uncertified; all mandatory library
records remain Planned. Full product snapshots/quote risk, remaining library
operations and actual Windows/Excel/browser qualification remain required.
