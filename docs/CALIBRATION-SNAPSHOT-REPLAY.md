# Calibration snapshots and replay

A snapshot can store a complete rate, CDS and cash calibration: the inputs, the
fitted curves and every diagnostic. Replay rebuilds the curves from the inputs
and checks that the result matches what was stored. This document covers that
capture format and the replay workflow.

`SnapshotAssembly.calibration` holds the capture. `convex::library::CalibrationReplay`
reuses the existing Rust resource resolution, the validating product builders
and the quote-risk operations. `convex_engine::calibration_replay` validates a
capture when it is taken, compares replay output against the stored output, and
connects replay to the application. The ports crate keeps bounded JSON and
metadata and does not depend on the financial crates. The engine adds one
dependency on the existing pure facade, and no external package, backend or
provider enters pricing.

## What a capture holds

The record keeps the complete `LibraryInputs`:

- calendar tables and their revisions;
- fixing revisions and publication decisions;
- curves you supply;
- helper, product and legal terms;
- quote units and revisions;
- calibration controls and exact dependencies.

Every supplied, fitted and view curve has a `CalibrationCurvePin`. The pin
holds the curve's opaque definition version and a separately supplied positive
numeric capture revision. Missing, extra or mismatched pins fail. No numeric
revision is parsed from a string.

The capture's `expected` field holds the output of the original build:

- the fitted recipes and grids;
- the helper valuations and residuals;
- the piecewise roots, or the global rank, conditioning and work diagnostics;
- the build order;
- the publications consumed.

Replay rebuilds the graph from the inputs, reprices every source through its
original builder, and compares the whole canonical output. Stored fitted nodes
and `Helpers` IDs are never decoded as a product graph or installed as a replay
cache.

A diagnostic mismatch fails. If a platform or build computes different
canonical diagnostics, replay cannot silently accept the earlier capture.
Replay is a reproducibility check. It does not show bitwise equality across
platforms, financial correctness or latency.

New captures need the real inputs and explicit metadata. `capture_calibration`
only seals an empty `expected` object, and it cannot replace an existing
capture.

## Schema versions

| Content | Schema | Hash domain |
|---|---|---|
| Zero records | 1 | `convex.snapshot.v1\0` |
| Direct recipes | 2 | `convex.snapshot.v2\0` |
| Product-calibration capture | 3 | `convex.snapshot.v3\0` |
| Any of these, as assembly writes every new snapshot | 4 | `convex.snapshot.v4\0`, by section ([identity](../crates/convex-engine/SNAPSHOTS.md#identity-quality-and-bounds)) |

Leaving out `calibration` keeps both older canonical identities. Schema 3
requires a capture, schemas 1 and 2 reject one, and schema 4 holds one or none.

Migration creates a new content-addressed snapshot with the full graph.
Historical snapshots are never overwritten, relabelled or completed from
current providers. Separate direct-recipe graphs can coexist only with disjoint
identities. They cannot supply inputs that the embedded calibration graph
omitted. Its dependencies stay inside its own inputs, and older zero records
cannot supply missing calibration dependencies.

A standalone product and risk snapshot (schema 3, or schema 4 with a
calibration) can omit unrelated bond references and market marks. The schema 1
and 2 reference requirements are unchanged. The fixed-bullet pricing use case
in the runtime still needs its instrument references and quotes. A risk graph
built from node values you supply keeps its complete curves and product action
even when no product fit is needed.

## Time and validation rules

- All valuation dates must match the snapshot's economic date.
- A rate calibration instant must match `as_of`.
- Input publication and known-at times cannot exceed the snapshot cutoff.
- Restated captures can keep later known revisions while the original
  calculation keeps its earlier fixing cutoff. Replay preserves that cutoff and
  its revision-selection policy.
- Missing history, unknown or discarded product fields, wrong resource
  versions, unidentified fits, exhausted work and residual failures return no
  curve set or risk result.
- Financial tolerances and solver defaults are unchanged.

## Engine integration

The assembler validates before it reads or publishes from the repository.
Configuration dry-run, stage and activation, snapshot selection, runtime graph
activation, valuation and recovery all use the complete capture.

Fixed-bullet observed-mid pricing can use the checked rate curves without
resampling them or losing their time basis and roles. Result lineage includes
the graph capture, each curve's numeric capture revision and the definition
pins you supplied. Quote, legal, calendar and history provenance stays inside
the captured inputs and the fit output.

The offline operation adds no server route. Other server product, job and risk
endpoints are not covered.

Rust `Build`, full product `Price`, the quote, node and recovery `Scenario`,
`ProductScenario`, `ProductRisk` and bucket reconciliation all reuse the
calibration action. Exact quote keys and units, frozen versus rebuilt
response, five-PV and half-step stability, and product and model restrictions
still apply. Risk, scenario and bucket actions must name this exact snapshot
ID, and their product time and cutoffs get the same capture-time checks.

If an action fails, the inputs and last good results are unchanged. Data
quality and provenance are returned with the result. The calculation does not
authorize execution.

The capture's source timestamp and age budget are applied conservatively to
every retained curve and dependency. Runtime expiry uses that deadline and the
quote and override deadlines. Replay does not refresh inputs. Source-specific
graph assembly and managed provider and fixing services are application work,
and callers must supply conservative source metadata.

## Limits

- Snapshots keep the shared 16 MiB limit, 64 curves in total, and 4,096 input
  nodes, helpers, observations and segments.
- Inputs keep the library's 2 MiB and 32,768 JSON values, 64 levels, 4,096
  entries and 8,192-byte text bounds.
- Output diagnostics allow 262,144 JSON values within the snapshot byte limit.
- The 64-recipe and 128-helper graph limits and the family, grid, history and
  solver bounds are unchanged.
- Result lineage keeps its 256-record limit.

This is bounded setup and batch work with ordinary allocations. There is no
hot-loop or colocation latency claim.

## Running it

Run the file reopen and the full rate quote-risk consumer:

```sh
cargo run --locked -p convex-engine --example calibration_snapshot_replay -- demo /tmp/convex-calibration-snapshot-replay
cargo run --locked -p convex-engine --example calibration_snapshot_replay -- --emit-configuration
cargo run --locked -p convex-engine --example calibration_snapshot_replay -- --emit-schema
cargo test --locked -p convex --all-features --test calibration_replay -p convex-engine --test calibration_snapshot_replay --test calibration_snapshot_runtime -p convex-ports --test calibration_snapshots
python3 tests/scripts/calibration_replay_inputs.py --check
```

The demo prints the snapshot ID and its `risk_action`. Save that action as a
JSON file, then run it against the reopened snapshot:

```sh
cargo run --locked -p convex-engine --example snapshot_replay -- verify DIRECTORY SNAPSHOT_ID
cargo run --locked -p convex-engine --example snapshot_replay -- calibration DIRECTORY SNAPSHOT_ID ACTION_JSON
cargo run --locked -p convex-engine --example snapshot_replay -- query-curves DIRECTORY SNAPSHOT_ID QUERIES_JSON
```

The parent directory must already exist. Action input rejects duplicate or
discarded economic fields and any loss of Decimal precision, using the strict
decoder. A failure exits with a nonzero status and no result on stdout. The
published operator schema describes the capture envelope. The request schema
and the Rust decoder own the financial input rules. The exported schemas are
exercised by `tests/scripts/calibration_replay_schema.py`.

## Evidence and related contracts

Thirty-two calibration and risk requests and two independent Decimal(60)
dated-deposit controls back this. They do not cover all market and method
combinations. [Product parametric fits](PRODUCT-PARAMETRIC-CALIBRATION.md) keep
their exact models and graph diagnostics through this replay path.

Not yet covered: other method, model, hedge and workout operations, and runs on
Windows, Excel, wasm32 and browsers.

Exponential UFR and speed fitting over exact analytic bases, required one-sided
joined forwards, quote risk and schema 3 replay are described in
[the UFR product contract](PRODUCT-UFR-CALIBRATION.md). Complete `Helpers`
recipes and mixed-unit parameters and diagnostics are kept. No sampled
objective, regulatory procedure or unsupported base is substituted.

Reconstruction installs each supplied dependency and product output in
dependency order. Sorting curves by identity cannot place a fitted tail before
its own base. An identity collision with another captured record is rejected
before installation. Missing or invalid dependencies keep their original
errors.

ICMA cash-helper recipes keep their source, nominal mesh, calendars, quotation
policy, compounding, quantity and inverse and work controls. Publishing a file
and reopening it rebuilds the full government and issuer-reference graph and
the signed quote risk, and compares the captured diagnostics and provenance.
Tampering fails. See [the helper contract](ICMA-CASH-HELPERS.md).
